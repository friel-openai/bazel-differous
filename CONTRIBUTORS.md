# Contributing

Thank you for considering a contribution to bazel-differrous. This project is a Rust rewrite of the upstream `bazel-diff` tool and aims to stay closely aligned with its design and behavior.

## How to contribute

- Start by opening an issue or updating the relevant entry in the `plans/` directory so changes can be tracked against the active ExecPlan.
- Keep pull requests focused and small when possible; mechanical refactors can be split from functional changes.
- Add or update tests alongside code changes, especially when touching hashing or impact logic.

## Development workflow

Before sending a change, please:

- Format code with `cargo fmt --all` or `just fmt`.
- Run Clippy with `cargo clippy --workspace --all-targets --all-features -- -D warnings` or `just lint`.
- Run tests with `cargo nextest run --workspace` or `just nextest`.
- For changes that affect the CLI, run the integration tests with `cargo test -p bazel-differrous-integration-tests` or `just integration-tests`.

Where applicable, keep the active ExecPlan up to date so future work can build on a clear history of decisions.

## Authorship and credits

The bulk of the design behind this tool comes from the upstream `bazel-diff` project and its maintainers, as well as the broader Bazel community.

Individual contributors are welcome and encouraged, but authorship credit for the overall approach is attributed to:

- The `bazel-diff` authors and maintainers.
- The Bazel team and community.

See `CREDITS.md` for more detailed acknowledgements and licensing information.

