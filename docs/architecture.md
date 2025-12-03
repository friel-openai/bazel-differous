# Architecture

This document describes how the bazel-differrous workspace is structured and how the hashing and impact pipelines mirror the upstream `bazel-diff` behavior.

## Crate layout

- `crates/cli`
  Binary crate for the `bazel-differrous` CLI. Uses `clap` for argument parsing, configures `tracing`/`tracing-subscriber`, and dispatches subcommands into the core library.

- `crates/core`
  Library crate that owns domain models and core algorithms:
  - Target hash computation (rules, generated files, source files, dep-edges) matching the Java implementation.
  - Bazel adapters that run query/cquery with streamed proto output and assemble the in-memory graph.
  - Impact diff pipeline used by `get-impacted-targets`.
  - Label normalization for bzlmod canonical repos and fine-grained external hashing.

- `crates/proto`
  Prost-generated Bazel streamed proto definitions (`Target`, `Rule`, `Attribute`, etc.) rebuilt from the proto files under `crates/proto/proto/`.

- `crates/integration-tests`
 Test-only crate that builds the Rust CLI, runs it against fixtures, and compares stdout/stderr/exit codes with the upstream jar. Tests automatically skip when the jar is absent.

## Impact diff pipeline (implemented)

`get-impacted-targets` is fully wired through `crates/cli` into `crates/core`:

1. CLI parses paths for `startingHashes`, `finalHashes`, optional `depEdgesFile`, and optional `--targetType` filters.
2. The core crate loads hashes into maps keyed by target label.
3. It computes impacted targets (additions, removals, changes) and orders results deterministically.
4. Target type filtering enforces that hashes were generated with `--includeTargetType`.
5. When dependency edges are supplied, package/target distances are computed from direct changes.
6. Output is either newline labels or JSON distances matching upstream formatting.

The flow is deterministic and cheap so CI can run it frequently.

## Generate-hashes pipeline (implemented)

`generate-hashes` shells out to Bazel `query`/`cquery` with `--output=streamed_proto`, decodes streamed protos with `prost`, and hashes targets to mirror the Java logic:

- **Bazel adapter** builds query/cquery invocations (respecting startup/command options, `--keep_going`, `--useCquery`, `--excludeExternalTargets`) and collects streamed protos. Pure bzlmod workspaces without `//external` are handled transparently.
- **Graph assembly** constructs a `BazelGraph` of rules, generated files, and sources. Fine-grained external repo patterns expand into additional query patterns.
- **Hash engine** reproduces upstream hashing: rule attribute hashing with ignored attributes, seed hash mixing, content hash overrides, modified-file filtering, target type annotations, dep-edge tracking, and fine-grained external repo handling (canonical names with trailing `+` preserved).
- **Outputs** are ordered JSON maps identical to `bazel-diff`; dep-edges JSON is emitted when requested with `-d/--depEdgesFile`.

## Bzlmod handling

- Canonical repo names containing `+` are treated as opaque; normalization only removes leading `@`.
- Fine-grained hashing supports both user-specified apparent repos and canonical `@@repo+` labels.
- `//external` may be absent under bzlmod; queries adapt accordingly.

## Tracing and profiling

The CLI configures `tracing-subscriber` with an environment-driven filter (`-v` or `RUST_LOG=debug/trace`). The async pipeline remains compatible with standard profilers (`perf`, `cargo flamegraph`, `tokio-console`) without rebuild flags.

## Compatibility strategy

- Command names, flags, exit codes, and output formats are kept compatible with the upstream `bazel-diff` CLI.
- Parity tests in `crates/integration-tests` compare the Rust implementation directly against the Java jar using byte-for-byte comparisons of stdout, stderr, dep-edge files, and exit codes across WORKSPACE and MODULE fixtures.
- Label normalization keeps outputs stable across Bazel versions even when canonical name spellings evolve.

These constraints ensure users can swap binaries with minimal friction while gaining Rust performance and observability benefits.
