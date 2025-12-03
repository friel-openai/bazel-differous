# Testing

How to exercise and understand the bazel-differrous test suite.

## Strategy

- **Unit/property tests** in `crates/core` cover label normalization, hashing edge cases (ignored attrs, seeds, content-hash overrides, fine-grained externals, bzlmod canonical names), and impact diff behaviors. Table-driven/property-style tests generate ~600 cases, well over 10× the upstream `bazel-diff` coverage.
- **Integration tests** in `crates/integration-tests` validate CLI wiring without Bazel.
- **Parity tests** in the same crate run both the Rust CLI and the upstream jar and compare outputs byte-for-byte on WORKSPACE and `MODULE.bazel` fixtures.

## Runners

- Default: `cargo nextest run --workspace` (or `just nextest`). Timeouts and slow-test handling are configured in `nextest.toml` and mirrored by the `ci` profile.
- Standard `cargo test --workspace` remains available for compatibility.

## Parity with upstream `bazel-diff`

- The upstream jar is expected at `target/upstream/bazel-diff_deploy.jar`.
- Build it with `just build-upstream-bazel-diff` (uses `vendor/bazel-diff` and Bazelisk).
- Run parity-only tests with `cargo test -p bazel-differrous-integration-tests` or `just integration-tests`.
- Tests skip automatically, with a clear message, if the jar is missing.

## Fixtures

- Impact fixtures: `tests/fixtures/impact/*` feed the `get-impacted-targets` parity tests.
- Hashing fixtures: `tests/fixtures/generate/workspace` (WORKSPACE + MODULE stub) and `tests/fixtures/generate/bzlmod` (pure MODULE with canonical `+` repos) drive hashing parity.
- Upstream CLI goldens: `tests/golden/upstream/*` capture help/version/usage outputs from the Java tool.

## CI and local validation

- GitHub Actions workflow `.github/workflows/ci.yml` runs fmt, clippy (`-D warnings`), nextest (including ignored tests), builds the upstream jar, and runs parity tests.
- Local mirror: `just act-ci` (use `--container-architecture linux/amd64` on Apple silicon). Default image is `ghcr.io/catthehacker/ubuntu:act-latest`; ensure the container can reach `static.rust-lang.org` for toolchain downloads.

## Notes

- Nextest timeouts (`nextest.toml`) keep slow Bazel-driven parity runs in check while leaving headroom for genuine work.
- The test count and fixture breadth intentionally exceed the upstream suite by more than an order of magnitude to satisfy the project requirements for coverage and regression safety.
