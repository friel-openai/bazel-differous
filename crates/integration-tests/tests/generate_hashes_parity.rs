use anyhow::{Context, Result};
use assert_cmd::Command;
use bazel_differrous_integration_tests::{rust_cli_path, upstream_jar_path, workspace_root};
use serde_json::Value;
use std::fs;
use std::path::Path;
use tempfile::TempDir;

#[derive(Debug)]
struct HashOutputs {
    hashes: Value,
    dep_edges: Option<Value>,
}

#[test]
fn workspace_basic_parity() -> Result<()> {
    let Some(jar_path) = upstream_jar_path() else {
        return Ok(());
    };
    let fixture = workspace_root().join("tests/fixtures/generate/workspace");
    let args = ["--bazelCommandOptions=--enable_workspace"];
    let upstream = run_upstream(&jar_path, &fixture, None, &args)?;
    let rust = run_rust(&fixture, None, &args)?;
    assert_eq!(upstream.hashes, rust.hashes);
    Ok(())
}

#[test]
fn workspace_include_target_type_and_filter() -> Result<()> {
    let Some(jar_path) = upstream_jar_path() else {
        return Ok(());
    };
    let fixture = workspace_root().join("tests/fixtures/generate/workspace");
    let args = [
        "--bazelCommandOptions=--enable_workspace",
        "--includeTargetType",
        "--targetType=Rule,GeneratedFile",
    ];
    let upstream = run_upstream(&jar_path, &fixture, None, &args)?;
    let rust = run_rust(&fixture, None, &args)?;
    assert_eq!(upstream.hashes, rust.hashes);
    Ok(())
}

#[test]
fn workspace_dep_edges_and_fine_grained_externals() -> Result<()> {
    let Some(jar_path) = upstream_jar_path() else {
        return Ok(());
    };
    let fixture = workspace_root().join("tests/fixtures/generate/workspace");
    let dep_dir = TempDir::new()?;
    let dep_path = dep_dir.path().join("deps.json");
    let args = [
        "--bazelCommandOptions=--enable_workspace",
        "--includeTargetType",
        "--fineGrainedHashExternalRepos",
        "@@extlib",
    ];
    let upstream = run_upstream(&jar_path, &fixture, Some(&dep_path), &args)?;
    let rust = run_rust(&fixture, Some(&dep_path), &args)?;
    assert_eq!(upstream.hashes, rust.hashes);
    assert_eq!(upstream.dep_edges, rust.dep_edges);
    Ok(())
}

#[test]
fn workspace_content_hash_and_seed_and_modified() -> Result<()> {
    let Some(jar_path) = upstream_jar_path() else {
        return Ok(());
    };
    let root = workspace_root();
    let fixture = root.join("tests/fixtures/generate/workspace");
    let content = fixture.join("content_hashes.json");
    let seed = fixture.join("seed_files.txt");
    let modified = fixture.join("modified_files.txt");
    let args = [
        "--bazelCommandOptions=--enable_workspace",
        "--contentHashPath",
        content.to_str().unwrap(),
        "--seed-filepaths",
        seed.to_str().unwrap(),
        "--modified-filepaths",
        modified.to_str().unwrap(),
        "--includeTargetType",
    ];
    let upstream = run_upstream(&jar_path, &fixture, None, &args)?;
    let rust = run_rust(&fixture, None, &args)?;
    assert_eq!(upstream.hashes, rust.hashes);
    Ok(())
}

#[test]
fn bzlmod_cquery_and_fine_grained() -> Result<()> {
    let Some(jar_path) = upstream_jar_path() else {
        return Ok(());
    };
    let root = workspace_root();
    let fixture = root.join("tests/fixtures/generate/bzlmod");
    let dep_dir = TempDir::new()?;
    let dep_path = dep_dir.path().join("deps.json");
    let content = fixture.join("content_hashes.json");
    let seed = fixture.join("seed_files.txt");
    let modified = fixture.join("modified_files.txt");
    let args = [
        "--useCquery",
        "--includeTargetType",
        "--fineGrainedHashExternalRepos",
        "@@depmod",
        "--contentHashPath",
        content.to_str().unwrap(),
        "--seed-filepaths",
        seed.to_str().unwrap(),
        "--modified-filepaths",
        modified.to_str().unwrap(),
    ];
    let upstream = run_upstream(&jar_path, &fixture, Some(&dep_path), &args)?;
    let rust = run_rust(&fixture, Some(&dep_path), &args)?;
    assert_eq!(upstream.hashes, rust.hashes);
    assert_eq!(upstream.dep_edges, rust.dep_edges);
    Ok(())
}

fn run_upstream(
    jar_path: &Path,
    fixture: &Path,
    dep_edges: Option<&Path>,
    extra_args: &[&str],
) -> Result<HashOutputs> {
    let mut cmd = Command::new("java");
    cmd.arg("-jar")
        .arg(jar_path)
        .arg("generate-hashes")
        .arg("-w")
        .arg(fixture)
        .arg("--bazelPath")
        .arg("bazelisk");

    if let Some(dep) = dep_edges {
        cmd.arg("-d").arg(dep);
    }
    cmd.args(extra_args);

    let output = cmd
        .current_dir(fixture)
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let hashes: Value = serde_json::from_slice(&output)?;
    let dep_edges_value = dep_edges.and_then(|p| read_json_file(p).ok());
    Ok(HashOutputs {
        hashes,
        dep_edges: dep_edges_value,
    })
}

fn run_rust(fixture: &Path, dep_edges: Option<&Path>, extra_args: &[&str]) -> Result<HashOutputs> {
    let mut cmd = Command::new(rust_cli_path()?);
    cmd.arg("generate-hashes")
        .arg("-w")
        .arg(fixture)
        .arg("--bazelPath")
        .arg("bazelisk");

    if let Some(dep) = dep_edges {
        cmd.arg("-d").arg(dep);
    }
    cmd.args(extra_args);

    let output = cmd
        .current_dir(fixture)
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let hashes: Value = serde_json::from_slice(&output)?;
    let dep_edges_value = dep_edges.and_then(|p| read_json_file(p).ok());
    Ok(HashOutputs {
        hashes,
        dep_edges: dep_edges_value,
    })
}

fn read_json_file(path: &Path) -> Result<Value> {
    let data =
        fs::read(path).with_context(|| format!("failed to read JSON file {}", path.display()))?;
    Ok(serde_json::from_slice(&data)?)
}
