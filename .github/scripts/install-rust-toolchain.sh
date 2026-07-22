#!/usr/bin/env bash

set -euo pipefail

extra_components="${1:-}"

# With no explicit TOOLCHAIN argument rustup installs the active toolchain,
# including the channel, profile, components, and targets selected by the
# repository's rust-toolchain.toml. This is intentionally different from
# installing the moving `stable` alias: an exact version bump must be a slow
# cache miss, never a setup failure caused by installing the wrong compiler.
rustup toolchain install --no-self-update

if [[ -n "${extra_components}" ]]; then
  IFS=',' read -r -a components <<< "${extra_components}"
  for component in "${components[@]}"; do
    component="${component//[[:space:]]/}"
    [[ -z "${component}" ]] && continue
    if [[ ! "${component}" =~ ^[a-z0-9][a-z0-9._-]*$ ]]; then
      echo "invalid Rust component: ${component}" >&2
      exit 2
    fi
    rustup component add "${component}"
  done
fi

# Exercise the repository override now so a malformed or unavailable pinned
# toolchain fails at setup with a useful error, not halfway through a build.
cargo --version
rustc --version
