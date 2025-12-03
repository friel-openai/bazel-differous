# Credits

bazel-differrous builds directly on the ideas and experience from the upstream `bazel-diff` project and the wider Bazel ecosystem.

## Upstream bazel-diff

The original `bazel-diff` tool was developed at Tinder and Match Group and is licensed under the BSD 3 Clause license. Its authors and maintainers defined the core workflows, data formats, and many of the ideas this project reuses.

This repository vendors `bazel-diff` under `vendor/bazel-diff` for reference, fixtures, and parity testing. Any reuse of behavior or data formats is intended to remain compatible with the upstream license.

## Bazel team and community

We also thank:

- The Bazel team for the build system, remote execution model, and the streaming APIs that make incremental diff tools possible.
- The broader Bazel community for rulesets, documentation, and examples that inform real world usage.

## License alignment

The bazel-differrous project is designed to be license compatible with the upstream BSD 3 Clause `bazel-diff` license. When in doubt, refer to the upstream `LICENSE` file under `vendor/bazel-diff` and ensure that contributions and reuse respect those terms.

