use bazel_differrous_integration_tests::workspace_root;

#[test]
fn smoke_workspace_root_exists() {
    assert!(workspace_root().join("Cargo.toml").is_file());
}
