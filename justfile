#!/usr/bin/env just --justfile

export RUST_BACKTRACE := "full"
export SKIP_WASM_BUILD := "1"
export RUST_BIN_DIR := "target/x86_64-unknown-linux-gnu"
export TARGET := "x86_64-unknown-linux-gnu"
export RELEASE_NAME := "development"

fmt:
  @echo "Running cargo fmt..."
  cargo fmt --all

check:
  @echo "Running cargo check..."
  cargo check --workspace

test:
  @echo "Running cargo test..."
  cargo test --workspace

benchmarks:
  @echo "Running cargo test with benchmarks..."
  cargo test --workspace --features=runtime-benchmarks

clippy:
  @echo "Running cargo clippy..."
  cargo clippy --workspace --all-targets -- \
                            -D clippy::todo \
                            -D clippy::unimplemented

clippy-fix:
    @echo "Running cargo clippy with automatic fixes on potentially dirty code..."
    cargo clippy --fix --allow-dirty --allow-staged --workspace --all-targets -- \
        -A clippy::todo \
        -A clippy::unimplemented \
        -A clippy::indexing_slicing

fix:
  @echo "Running cargo fix..."
  cargo fix --workspace
  git diff --exit-code || (echo "There are local changes after running 'cargo fix --workspace' ❌" && exit 1)

lint:
  @echo "Running cargo fmt..."
  just fmt
  @echo "Running cargo clippy with automatic fixes on potentially dirty code..."
  just clippy-fix
  @echo "Running cargo clippy..."
  just clippy

production:
  @echo "Running cargo build with metadata-hash generation..."
  cargo build --profile production --features="metadata-hash"
