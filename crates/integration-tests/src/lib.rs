use anyhow::{anyhow, Context, Result};
use once_cell::sync::OnceCell;
use std::path::PathBuf;
use std::process::Command as StdCommand;

static RUST_CLI_PATH: OnceCell<PathBuf> = OnceCell::new();

/// Root of the workspace (two levels up from this crate).
pub fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root is two directories above the integration-tests crate")
        .to_path_buf()
}

/// Location of the upstream bazel-diff jar if it has been built.
pub fn upstream_jar_path() -> Option<PathBuf> {
    let jar = workspace_root().join("target/upstream/bazel-diff_deploy.jar");
    if jar.is_file() {
        Some(jar)
    } else {
        eprintln!(
            "skipping bazel-diff parity tests; missing upstream jar at {}",
            jar.display()
        );
        None
    }
}

/// Build (once) and return the path to the bazel-differrous CLI binary.
pub fn rust_cli_path() -> Result<PathBuf> {
    RUST_CLI_PATH.get_or_try_init(build_rust_cli).cloned()
}

fn bail_build_failed(status: std::process::ExitStatus) -> Result<()> {
    Err(anyhow!(
        "cargo build for bazel-differrous failed with {status:?}"
    ))
}

fn build_rust_cli() -> Result<PathBuf> {
    let root = workspace_root();
    let status = StdCommand::new("cargo")
        .args([
            "build",
            "-p",
            "bazel-differrous-cli",
            "--bin",
            "bazel-differrous",
        ])
        .current_dir(&root)
        .status()
        .context("failed to start cargo build for bazel-differrous")?;

    if !status.success() {
        bail_build_failed(status)?;
    }

    let mut path = root.join("target/debug/bazel-differrous");
    if cfg!(windows) {
        path.set_extension("exe");
    }
    Ok(path)
}
