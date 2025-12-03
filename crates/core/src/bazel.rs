use anyhow::{bail, Context, Result};
use bazel_differrous_proto::{analysis, build};
use prost::Message;
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use tempfile::NamedTempFile;
use tokio::process::Command;

#[derive(Debug, Clone, Default)]
pub struct BazelOptions {
    pub workspace: PathBuf,
    pub bazel_path: PathBuf,
    pub startup_options: Vec<String>,
    pub command_options: Vec<String>,
    pub cquery_options: Vec<String>,
    pub use_cquery: bool,
    pub keep_going: bool,
}

impl BazelOptions {
    pub fn bazel_binary(&self) -> &Path {
        if self.bazel_path.as_os_str().is_empty() {
            Path::new("bazel")
        } else {
            &self.bazel_path
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct BazelVersion {
    pub major: u32,
    pub minor: u32,
    pub patch: u32,
}

impl BazelVersion {
    pub fn new(major: u32, minor: u32, patch: u32) -> Self {
        Self {
            major,
            minor,
            patch,
        }
    }

    pub fn at_least(&self, major: u32, minor: u32, patch: u32) -> bool {
        (self.major, self.minor, self.patch) >= (major, minor, patch)
    }
}

pub async fn bazel_version(opts: &BazelOptions) -> Result<BazelVersion> {
    let mut cmd = Command::new(opts.bazel_binary());
    cmd.arg("--version");
    cmd.current_dir(&opts.workspace);
    cmd.args(&opts.startup_options);

    let output = cmd
        .output()
        .await
        .context("failed to run bazel --version")?;
    if !output.status.success() {
        bail!("bazel --version failed with {}", output.status);
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let line = stdout.lines().next().unwrap_or_default();
    // Expected: "bazel X.Y.Z" with optional suffixes.
    let version_str = line.trim().strip_prefix("bazel ").unwrap_or(line.trim());
    let base = version_str.split('-').next().unwrap_or(version_str);
    let mut parts = base.split('.').map(|s| s.parse::<u32>());
    let major = parts.next().transpose()?.unwrap_or(0);
    let minor = parts.next().transpose()?.unwrap_or(0);
    let patch = parts.next().transpose()?.unwrap_or(0);
    Ok(BazelVersion::new(major, minor, patch))
}

pub async fn bazel_output_base(opts: &BazelOptions) -> Result<PathBuf> {
    let mut cmd = Command::new(opts.bazel_binary());
    cmd.args(&opts.startup_options);
    cmd.arg("info");
    cmd.arg("output_base");
    cmd.current_dir(&opts.workspace);

    let output = cmd.output().await.context("failed to run bazel info")?;
    if !output.status.success() {
        bail!("bazel info output_base failed with {}", output.status);
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let path = stdout
        .lines()
        .next()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty())
        .ok_or_else(|| anyhow::anyhow!("bazel info output_base returned no path"))?;

    Ok(PathBuf::from(path))
}

pub fn build_query_expression(patterns: &[String]) -> String {
    patterns
        .iter()
        .map(|p| format!("'{p}'"))
        .collect::<Vec<_>>()
        .join(" + ")
}

pub async fn run_query(opts: &BazelOptions, expression: &str) -> Result<Vec<build::Target>> {
    let stdout = execute_bazel(opts, "query", expression, &opts.command_options, false).await?;
    decode_streamed_targets(&stdout)
}

pub async fn run_cquery(opts: &BazelOptions, expression: &str) -> Result<Vec<build::Target>> {
    let compatible = compatible_target_set(opts, expression)
        .await
        .unwrap_or_default();
    let stdout = execute_bazel(opts, "cquery", expression, &opts.cquery_options, true).await?;
    let mut targets = decode_cquery_results(&stdout)?;
    if !compatible.is_empty() {
        targets.retain(|t| {
            target_label(t)
                .map(|label| compatible.contains(label))
                .unwrap_or(false)
        });
    }
    Ok(targets)
}

async fn execute_bazel(
    opts: &BazelOptions,
    subcommand: &str,
    expression: &str,
    command_opts: &[String],
    is_cquery: bool,
) -> Result<Vec<u8>> {
    let query_file =
        NamedTempFile::new_in(&opts.workspace).context("failed to create temporary query file")?;
    fs::write(query_file.path(), expression).context("failed to write query expression")?;

    let mut cmd = Command::new(opts.bazel_binary());
    cmd.args(&opts.startup_options);
    cmd.arg(subcommand);

    if is_cquery {
        cmd.arg("--transitions=lite");
        cmd.arg("--output=streamed_proto");
    } else {
        cmd.arg("--output=streamed_proto");
        cmd.arg("--order_output=no");
    }

    if opts.keep_going {
        cmd.arg("--keep_going");
    }

    cmd.args(command_opts);
    if is_cquery {
        cmd.arg("--consistent_labels");
    }

    cmd.arg("--query_file");
    cmd.arg(query_file.path());
    cmd.current_dir(&opts.workspace);

    let output = cmd.output().await.with_context(|| {
        format!(
            "failed to run bazel {} with query file {}",
            subcommand,
            query_file.path().display()
        )
    })?;

    if !is_allowed_status(&output.status, opts.keep_going) {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("bazel {subcommand} failed: {stderr}");
    }

    Ok(output.stdout)
}

async fn compatible_target_set(opts: &BazelOptions, expression: &str) -> Result<HashSet<String>> {
    let starlark = r#"
def format(target):
    if providers(target) == None:
        return ""
    if "IncompatiblePlatformProvider" not in providers(target):
        target_repr = repr(target)
        if "<alias target" in target_repr:
            return target_repr.split(" ")[2]
        return str(target.label)
    return ""
"#;

    let query_file = NamedTempFile::new_in(&opts.workspace)?;
    fs::write(query_file.path(), expression)?;

    let starlark_file = NamedTempFile::new_in(&opts.workspace)?;
    fs::write(starlark_file.path(), starlark)?;

    let mut cmd = Command::new(opts.bazel_binary());
    cmd.args(&opts.startup_options);
    cmd.arg("cquery");
    cmd.arg("--output");
    cmd.arg("starlark");
    cmd.arg("--starlark:file");
    cmd.arg(starlark_file.path());
    if opts.keep_going {
        cmd.arg("--keep_going");
    }
    cmd.args(&opts.cquery_options);
    cmd.arg("--consistent_labels");
    cmd.arg("--query_file");
    cmd.arg(query_file.path());
    cmd.current_dir(&opts.workspace);

    let output = cmd.output().await?;
    if !is_allowed_status(&output.status, opts.keep_going) {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("bazel cquery (compat) failed: {stderr}");
    }

    let set = String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| l.trim().to_string())
        .collect::<HashSet<_>>();
    Ok(set)
}

fn decode_cquery_results(bytes: &[u8]) -> Result<Vec<build::Target>> {
    let mut targets = Vec::new();
    for result in decode_length_delimited::<analysis::CqueryResult>(bytes)? {
        for configured in result.results {
            if let Some(target) = configured.target {
                targets.push(target);
            }
        }
    }
    Ok(targets)
}

fn decode_length_delimited<T>(mut bytes: &[u8]) -> Result<Vec<T>>
where
    T: Message + Default,
{
    let mut out = Vec::new();
    while !bytes.is_empty() {
        let message = T::decode_length_delimited(&mut bytes)
            .context("failed to decode streamed protobuf message")?;
        out.push(message);
    }
    Ok(out)
}

fn target_label(target: &build::Target) -> Option<&str> {
    target
        .rule
        .as_ref()
        .map(|r| r.name.as_str())
        .or_else(|| target.source_file.as_ref().map(|s| s.name.as_str()))
        .or_else(|| target.generated_file.as_ref().map(|g| g.name.as_str()))
}

fn is_allowed_status(status: &std::process::ExitStatus, keep_going: bool) -> bool {
    status.success() || (keep_going && matches!(status.code(), Some(3)))
}

fn decode_streamed_targets(bytes: &[u8]) -> Result<Vec<build::Target>> {
    let mut targets = Vec::new();
    let mut slice = bytes;
    while !slice.is_empty() {
        if let Ok((qr, remaining)) = decode_single::<build::QueryResult>(slice) {
            targets.extend(qr.target);
            slice = remaining;
            continue;
        }

        if let Ok((cqr, remaining)) = decode_single::<analysis::CqueryResult>(slice) {
            for ct in cqr.results {
                if let Some(t) = ct.target {
                    targets.push(t);
                }
            }
            slice = remaining;
            continue;
        }

        if let Ok((ct, remaining)) = decode_single::<analysis::ConfiguredTarget>(slice) {
            if let Some(t) = ct.target {
                targets.push(t);
            }
            slice = remaining;
            continue;
        }

        if let Ok((target, remaining)) = decode_single::<build::Target>(slice) {
            targets.push(target);
            slice = remaining;
            continue;
        }

        bail!("failed to decode streamed protobuf message");
    }
    Ok(targets)
}

fn decode_single<T>(input: &[u8]) -> Result<(T, &[u8]), prost::DecodeError>
where
    T: Message + Default,
{
    let mut buf = input;
    let message = T::decode_length_delimited(&mut buf)?;
    let consumed = input.len() - buf.len();
    Ok((message, &input[consumed..]))
}
