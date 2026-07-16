#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
preflight="${repo_root}/.github/scripts/rust-setup-preflight.sh"
action="${repo_root}/.github/actions/rust-setup/action.yml"
tmp="$(mktemp -d)"
trap 'rm -rf "${tmp}"' EXIT

contract_dir="${tmp}/contract"
mock_bin="${tmp}/bin"
mkdir -p "${contract_dir}" "${mock_bin}"

cat > "${mock_bin}/dpkg-query" <<'EOF'
#!/usr/bin/env bash
package="${!#}"
if [[ "${package}" == "${MOCK_MISSING_PACKAGE:-}" ]]; then
  exit 1
fi
printf 'ii '
EOF

cat > "${mock_bin}/cargo" <<'EOF'
#!/usr/bin/env bash
[[ "${MOCK_CARGO_FAIL:-false}" != true ]]
EOF

cat > "${mock_bin}/rustc" <<'EOF'
#!/usr/bin/env bash
exit 0
EOF

cat > "${mock_bin}/rustup" <<'EOF'
#!/usr/bin/env bash
if [[ "${1:-}" == component && "${2:-}" == list && "${3:-}" == --installed ]]; then
  printf '%s\n' ${MOCK_RUST_COMPONENTS:-cargo-x86_64-unknown-linux-gnu rustc-x86_64-unknown-linux-gnu}
  exit 0
fi
exit 1
EOF
chmod +x "${mock_bin}"/*

cat > "${contract_dir}/packages.txt" <<'EOF'
build-essential
clang
curl
git
libssl-dev
libudev-dev
llvm
make
pkg-config
protobuf-compiler
python3
python3-dev
EOF
touch "${contract_dir}/image-contract.env"

run_preflight() {
  local output="${tmp}/output"
  : > "${output}"
  PATH="${mock_bin}:${PATH}" \
    FIREACTIONS_RUNNER_IMAGE_CONTRACT_DIR="${contract_dir}" \
    RUST_SETUP_COMPONENTS="${1:-}" \
    MOCK_MISSING_PACKAGE="${MOCK_MISSING_PACKAGE:-}" \
    MOCK_CARGO_FAIL="${MOCK_CARGO_FAIL:-false}" \
    MOCK_RUST_COMPONENTS="${MOCK_RUST_COMPONENTS:-}" \
    "${preflight}" "${output}" >/dev/null
  cat "${output}"
}

assert_output() {
  local output="$1"
  local expected="$2"
  grep -Fxq "${expected}" <<< "${output}" || {
    echo "missing '${expected}' in preflight output:" >&2
    echo "${output}" >&2
    exit 1
  }
}

output="$(run_preflight)"
assert_output "${output}" "system_ready=true"
assert_output "${output}" "toolchain_ready=true"

MOCK_RUST_COMPONENTS="cargo-x86_64-unknown-linux-gnu rustc-x86_64-unknown-linux-gnu clippy-x86_64-unknown-linux-gnu rustfmt-x86_64-unknown-linux-gnu"
output="$(run_preflight 'clippy,rustfmt')"
assert_output "${output}" "system_ready=true"
assert_output "${output}" "toolchain_ready=true"

output="$(run_preflight 'clippy,rust-src')"
assert_output "${output}" "toolchain_ready=false"

MOCK_MISSING_PACKAGE=llvm
output="$(run_preflight)"
assert_output "${output}" "system_ready=false"
assert_output "${output}" "toolchain_ready=true"
unset MOCK_MISSING_PACKAGE

MOCK_CARGO_FAIL=true
output="$(run_preflight)"
assert_output "${output}" "toolchain_ready=false"
unset MOCK_CARGO_FAIL

mv "${contract_dir}/image-contract.env" "${contract_dir}/image-contract.env.missing"
output="$(run_preflight)"
assert_output "${output}" "system_ready=false"
assert_output "${output}" "toolchain_ready=false"

grep -Fq 'run: .github/scripts/rust-setup-preflight.sh "$GITHUB_OUTPUT"' "${action}"
grep -Fq "if: steps.runner-image.outputs.system_ready != 'true'" "${action}"
grep -Fq "if: steps.runner-image.outputs.toolchain_ready != 'true'" "${action}"

echo "rust setup preflight tests passed"
