use anyhow::Result;
use assert_cmd::Command;
use bazel_differrous_integration_tests::{rust_cli_path, upstream_jar_path, workspace_root};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
struct ImpactFixtures {
    start: PathBuf,
    final_: PathBuf,
    deps: PathBuf,
}

#[test]
fn parity_with_dep_edges() -> Result<()> {
    let Some(jar_path) = upstream_jar_path() else {
        return Ok(());
    };

    let fixtures = impact_fixtures();

    let upstream = run_upstream(&jar_path, &fixtures, true, None)?;
    let rust = run_rust_cli(&fixtures, true, None)?;

    assert_eq!(upstream, rust);
    Ok(())
}

#[test]
fn parity_without_dep_edges_newline_output() -> Result<()> {
    let Some(jar_path) = upstream_jar_path() else {
        return Ok(());
    };

    let fixtures = impact_fixtures();

    let upstream = run_upstream(&jar_path, &fixtures, false, None)?;
    let rust = run_rust_cli(&fixtures, false, None)?;

    assert_eq!(upstream, rust);
    Ok(())
}

#[test]
fn parity_with_target_type_filter() -> Result<()> {
    let Some(jar_path) = upstream_jar_path() else {
        return Ok(());
    };

    let fixtures = impact_fixtures();

    let upstream = run_upstream(&jar_path, &fixtures, true, Some("Rule"))?;
    let rust = run_rust_cli(&fixtures, true, Some("Rule"))?;

    assert_eq!(upstream, rust);
    Ok(())
}

fn run_upstream(
    jar_path: &Path,
    fixtures: &ImpactFixtures,
    include_deps: bool,
    target_type: Option<&str>,
) -> Result<Vec<u8>> {
    let mut cmd = Command::new("java");
    cmd.arg("-jar")
        .arg(jar_path)
        .arg("get-impacted-targets")
        .arg("-sh")
        .arg(&fixtures.start)
        .arg("-fh")
        .arg(&fixtures.final_);

    if include_deps {
        cmd.arg("-d").arg(&fixtures.deps);
    }

    if let Some(kind) = target_type {
        cmd.arg("--targetType").arg(kind);
    }

    let output = cmd.assert().success().get_output().stdout.clone();

    Ok(output)
}

fn run_rust_cli(
    fixtures: &ImpactFixtures,
    include_deps: bool,
    target_type: Option<&str>,
) -> Result<Vec<u8>> {
    let mut cmd = Command::new(rust_cli_path()?);
    cmd.arg("get-impacted-targets")
        .arg("-sh")
        .arg(&fixtures.start)
        .arg("-fh")
        .arg(&fixtures.final_);

    if include_deps {
        cmd.arg("-d").arg(&fixtures.deps);
    }

    if let Some(kind) = target_type {
        cmd.arg("--targetType").arg(kind);
    }

    let output = cmd.assert().success().get_output().stdout.clone();

    Ok(output)
}

fn impact_fixtures() -> ImpactFixtures {
    let base = workspace_root().join("tests/fixtures/impact");
    ImpactFixtures {
        start: base.join("starting.json"),
        final_: base.join("final.json"),
        deps: base.join("dep_edges.json"),
    }
}
