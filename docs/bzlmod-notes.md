# Bzlmod notes

This document captures working notes about Bazel bzlmod behavior that affect tools like bazel-differrous.

## Defaults and high level behavior

- Bazel 8 enables bzlmod by default for new workspaces.
- With bzlmod, external dependencies are described in `MODULE.bazel` files rather than `WORKSPACE`.
- The legacy `//external` pseudo repository is removed in pure bzlmod setups; labels point directly at canonical repositories instead.

Tooling that assumes `//external` exists or that all external labels share a single prefix will break under bzlmod.

## Canonical repository names

Under bzlmod, Bazel computes canonical repository names that:

- Are not required to match the apparent names used in `MODULE.bazel`.
- Often contain `+` characters that encode parts of the module graph.
- Can change across Bazel versions or with small changes to the module graph.

Key implications:

- Do not parse canonical repo names with ad hoc string logic.
- Treat canonical names as opaque identifiers and use Bazel supplied mappings to relate them back to apparent names.
- Avoid persisting canonical names in user facing output where stability matters.

## Repo mapping

To understand how canonical and apparent repository names relate, bzlmod exposes repo mapping data. Tools should:

- Prefer Bazel provided mappings over heuristic string manipulation.
- Use `bazel mod` commands (see below) to obtain module and repo information when needed.
- Normalize labels into a stable, user facing form before hashing or diffing.

This approach keeps outputs stable even if Bazel adjusts how canonical names are spelled internally.

## Canonical naming changes across versions

Canonical naming schemes have evolved across Bazel releases, and may continue to do so. Typical changes include:

- Different placement or count of `+` characters.
- Different treatment of transitive dependencies or extension generated repos.
- Different defaults around when canonical names are exposed in labels.

Because of this, bazel-differrous should:

- Avoid hard coding exact canonical name formats.
- Use feature detection and version checks when behavior genuinely differs by Bazel version.
- Prefer stable concepts (module name, apparent repo name, label path) when computing hashes.

## Relevant bazel mod commands

The following `bazel mod` commands are particularly relevant to tooling:

- `bazel mod query` for inspecting the module graph.
- `bazel mod dump_repo_mapping` for obtaining repo mappings between apparent and canonical names.
- `bazel mod show_repo` for inspecting a single repository.

These commands, combined with standard query or aquery output, should provide enough information to build bzlmod aware hash and diff pipelines without depending on internal string formats.

