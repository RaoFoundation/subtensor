#!/usr/bin/env bash
set -euo pipefail

script_dir=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
repo_root=$(cd "$script_dir/../.." && pwd)
registry=${1:-$repo_root/.github/rust-ci-paths.txt}

if [[ ! -s "$registry" ]]; then
  echo "Rust CI path registry is missing or empty: $registry" >&2
  echo "Fix: restore .github/rust-ci-paths.txt with one sorted repository-relative directory prefix per line." >&2
  echo "Verify: .github/scripts/test-rust-ci-paths.sh" >&2
  exit 1
fi

prefixes=()
while IFS= read -r prefix; do
  prefixes+=("$prefix")
done < <(sed '/^[[:space:]]*$/d' "$registry")
for prefix in "${prefixes[@]}"; do
  if [[ ! "$prefix" =~ ^[A-Za-z0-9_-]+(/[A-Za-z0-9_-]+)*$ ]]; then
    echo "Invalid Rust CI path prefix: $prefix" >&2
    echo "Fix: use a repository-relative directory such as pallets or sdk/bittensor-core in .github/rust-ci-paths.txt." >&2
    echo "Verify: .github/scripts/test-rust-ci-paths.sh" >&2
    exit 1
  fi
  if [[ ! -d "$repo_root/$prefix" ]]; then
    echo "Registered Rust CI path does not exist: $prefix" >&2
    echo "Fix: correct or remove the stale line in .github/rust-ci-paths.txt." >&2
    echo "Verify: .github/scripts/test-rust-ci-paths.sh" >&2
    exit 1
  fi
done

sorted=$(printf '%s\n' "${prefixes[@]}" | LC_ALL=C sort -u)
if [[ "$sorted" != "$(printf '%s\n' "${prefixes[@]}")" ]]; then
  echo "Rust CI path registry must be sorted and contain no duplicates" >&2
  echo "Fix: run LC_ALL=C sort -u .github/rust-ci-paths.txt -o .github/rust-ci-paths.txt" >&2
  echo "Verify: .github/scripts/test-rust-ci-paths.sh" >&2
  exit 1
fi

metadata=$(cd "$repo_root" && cargo metadata --format-version 1 --no-deps)
package_directories=()
while IFS= read -r package_directory; do
  package_directories+=("$package_directory")
done < <(
  jq -r --arg root "$repo_root/" '
    .workspace_members[] as $member
    | .packages[]
    | select(.id == $member)
    | .manifest_path
    | sub("/Cargo.toml$"; "")
    | select(. != ($root | rtrimstr("/")))
    | ltrimstr($root)
  ' <<< "$metadata" | LC_ALL=C sort -u
)

missing=()
for package_directory in "${package_directories[@]}"; do
  covered=false
  for prefix in "${prefixes[@]}"; do
    if [[ "$package_directory" == "$prefix" || "$package_directory" == "$prefix/"* ]]; then
      covered=true
      break
    fi
  done
  if [[ "$covered" != true ]]; then
    missing+=("$package_directory")
  fi
done

if (( ${#missing[@]} > 0 )); then
  echo "Rust workspace packages have no CI path owner:" >&2
  printf '  - %s\n' "${missing[@]}" >&2
  echo "Fix: add the narrowest stable parent directory for each package to .github/rust-ci-paths.txt; do not add individual crates when an existing area such as pallets or support owns them." >&2
  echo "The trusted classifier reads this registry automatically; do not add a second path pattern to check-rust.yml." >&2
  echo "Verify: .github/scripts/test-rust-ci-paths.sh" >&2
  exit 1
fi

echo "Validated ${#package_directories[@]} non-root Rust workspace packages across ${#prefixes[@]} CI path prefixes."
