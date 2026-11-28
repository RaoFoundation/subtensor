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

# --- The canonical rails: the local rig (scripts/rails) ---

# Start the rig from nothing: a localnet, anvil playing Base, Hyperlane, the contracts, the hub's configuration.
rails-up:
  scripts/rails/up.sh

# Stop the rig. `just rails-down --purge` also deletes its state under .rails/.
rails-down *ARGS:
  scripts/rails/down.sh {{ARGS}}

# Run the rails end to end against the rig: deposit, stake, token, sale, credits, refund, exit.
rails-e2e:
  bash -c 'source scripts/rails/env.sh && rails_python e2e.py && rails_python e2e_cli.py'

# Serve the swap page against the rig at http://localhost:3000/swap.
rails-page *ARGS:
  scripts/rails/page.sh {{ARGS}}

# Show the last lines of the Hyperlane agents' logs.
rails-logs:
  scripts/rails/agents.sh logs

# Build and test the rails' contracts: unit, fuzz and invariant tests.
rails-contracts:
  cd contracts/evm && forge soldeer install && forge build && forge test
