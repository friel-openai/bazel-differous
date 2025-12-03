set dotenv-load := false

default:
    @just --list

build:
    cargo build --workspace

fmt:
    cargo fmt --all

lint:
    cargo clippy --workspace --all-targets --all-features -- -D warnings

test:
    cargo test --workspace

nextest:
    cargo nextest run --workspace

integration-tests:
    cargo test -p bazel-differrous-integration-tests

build-upstream-bazel-diff:
    cd vendor/bazel-diff && RULES_PYTHON_ALLOW_BUILD_AS_ROOT=1 bazel build //cli:bazel-diff_deploy.jar --repo_env=RULES_PYTHON_ALLOW_BUILD_AS_ROOT=1 --action_env=RULES_PYTHON_ALLOW_BUILD_AS_ROOT=1 --host_action_env=RULES_PYTHON_ALLOW_BUILD_AS_ROOT=1
    rm -rf target/upstream
    mkdir -p target/upstream
    cp vendor/bazel-diff/bazel-bin/cli/bazel-diff_deploy.jar target/upstream/

ci:
    just fmt && just lint && just nextest

act-ci:
    act -j ci
