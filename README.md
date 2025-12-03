# bazel-differrous

Rust rewrite of Tinder's `bazel-diff`, designed as a drop-in CLI and output compatible replacement with faster, traced, async execution and first-class Bazel 8/9 bzlmod support.

## Highlights

- Drop-in compatibility: `generate-hashes` and `get-impacted-targets` match the upstream jar byte-for-byte on WORKSPACE and `MODULE.bazel` fixtures, including fine-grained external repo hashing and dep-edges output.
- Bzlmod ready: canonical repo names with trailing `+` are preserved, `@@` labels hash identically to upstream, and pure bzlmod workspaces (no `//external`) are handled.
- Modern Rust pipeline: async Bazel invocations, streamed proto decoding, deterministic hashing, structured `tracing` output, and optional verbose logging via `-v`/`RUST_LOG`.
- Tests and tooling: nearly 600 Rust tests plus a parity harness against the upstream jar, `cargo-nextest` with timeouts (`nextest.toml`), and GitHub Actions validated with `act`.

## Quick start

```bash
just build            # cargo build --workspace
just nextest          # runs the full suite with timeouts
just build-upstream-bazel-diff
just integration-tests
```

- The upstream jar lives at `target/upstream/bazel-diff_deploy.jar` and is built from the `vendor/bazel-diff` submodule. `just build-upstream-bazel-diff` automates this.
- Local CI: `just act-ci` (uses the `ghcr.io/catthehacker/ubuntu:act-latest` image; set `--container-architecture linux/amd64` on Apple silicon).
- No system `protoc` is required; the prost build uses `protoc-bin-vendored` during compilation.

## CLI usage

### generate-hashes

```bash
bazel-differrous generate-hashes \
  -w /path/to/workspace \
  --bazelPath bazelisk \
  --bazelCommandOptions=--enable_workspace \
  --includeTargetType \
  --fineGrainedHashExternalRepos @@depmod \
  --contentHashPath content_hashes.json \
  --seed-filepaths seed_files.txt \
  --modified-filepaths modified_files.txt \
  -d dep_edges.json > hashes.json
```

- Supports `--useCquery`, `--excludeExternalTargets`, `--ignoredRuleHashingAttributes`, `--fineGrainedHashExternalRepos[File]`, `--seed-filepaths`, `--contentHashPath`, `--modified-filepaths`, and `--targetType/-tt` exactly like the Java tool.
- Outputs hash JSON (and optional dep-edges JSON) identically to `bazel-diff` for both legacy WORKSPACE and bzlmod projects.

### get-impacted-targets

```bash
bazel-differrous get-impacted-targets \
  -sh starting.json -fh final.json \
  [-d dep_edges.json] \
  [--targetType Rule,SourceFile] \
  [-o impacted.json]
```

- Without `-d`, emits newline labels; with dep-edges it emits JSON with distance metrics, matching upstream ordering and exit codes.

## Testing and verification

- `cargo nextest run --workspace` exercises ~600 unit/property tests (label normalization, hashing edge cases, bzlmod canonical names) plus integration tests; timeouts are configured in `nextest.toml`.
- Parity harness (`crates/integration-tests`) runs the Rust CLI and the upstream jar with identical flags and compares stdout/stderr/exit codes byte-for-byte on WORKSPACE and MODULE fixtures. Tests skip automatically if the jar is absent.
- GitHub Actions workflow `.github/workflows/ci.yml` mirrors the `Justfile` targets and is validated locally with `act`.

## Observability and profiling

- `-v` or `RUST_LOG=debug` enables detailed tracing spans; outputs remain stable for parity tests.
- Binaries are compatible with standard profilers (`perf`, `cargo flamegraph`, `tokio-console`) without rebuild flags.

## Project layout

- `crates/core`: hashing engine, Bazel adapters, impact diff logic, and label/attribute normalization.
- `crates/cli`: Clap-based CLI wiring, tracing init, and file I/O.
- `crates/proto`: prost-generated Bazel streamed proto types.
- `crates/integration-tests`: parity harness and fixtures.
- `tests/fixtures` + `tests/golden`: shared fixtures and captured upstream outputs.
- `vendor/bazel-diff`: upstream submodule used for goldens and behavioral reference.

## Credits

This project builds directly on the ideas and formats from the upstream `bazel-diff` authors at Tinder/Match Group and the wider Bazel community. See `CREDITS.md` for acknowledgements and licensing details.
