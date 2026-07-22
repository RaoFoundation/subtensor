#!/usr/bin/env bash
set -euo pipefail

usage() {
  echo "usage: $0 [--all] OUTPUT_FILE" >&2
  exit 2
}

all=false
if [[ "${1:-}" == --all ]]; then
  all=true
  shift
fi
[[ $# -eq 1 ]] || usage
output_file=$1

script_dir=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
registry=${RUST_CI_PATH_REGISTRY:-$script_dir/../rust-ci-paths.txt}
if [[ ! -s "$registry" ]] || grep -qEv '^[A-Za-z0-9_-]+(/[A-Za-z0-9_-]+)*$' "$registry"; then
  echo "Rust CI path registry is missing or invalid: $registry" >&2
  exit 1
fi
prefixes=()
while IFS= read -r prefix; do
  prefixes+=("$prefix")
done < "$registry"

rust=$all
if [[ "$all" != true ]]; then
  while IFS= read -r path; do
    [[ -n "$path" ]] || continue
    for prefix in "${prefixes[@]}"; do
      if [[ "$path" == "$prefix/"* ]]; then
        rust=true
        break
      fi
    done
    [[ "$rust" != true ]] || continue

    case "$path" in
      Cargo.toml|*/Cargo.toml|Cargo.lock|build.rs|rust-toolchain.toml|zepter.yaml|rustfmt.toml|clippy.toml|.cargo/*)
        rust=true
        ;;
      .github/rust-ci-paths.txt|.github/workflows/check-rust.yml|.github/actions/rust-setup/*|.github/actions/sccache-setup/*|.github/scripts/classify-rust-changes.sh|.github/scripts/test-rust-ci-paths.sh|.github/scripts/validate-rust-ci-paths.sh|.github/scripts/extract-pull-file-paths.sh|.github/scripts/rust-setup-preflight.sh|.github/scripts/install-rust-toolchain.sh|.github/scripts/sccache-configure.sh|.github/scripts/sccache-config.py|.github/scripts/sccache-report.sh)
        rust=true
        ;;
    esac
  done
fi

echo "rust=$rust" >> "$output_file"
