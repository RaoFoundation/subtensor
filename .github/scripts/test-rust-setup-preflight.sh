#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
preflight="${repo_root}/.github/scripts/rust-setup-preflight.sh"
installer="${repo_root}/.github/scripts/install-rust-toolchain.sh"
action="${repo_root}/.github/actions/rust-setup/action.yml"
repo_toolchain="$(
  sed -nE 's/^[[:space:]]*channel[[:space:]]*=[[:space:]]*"([^"[:space:]]+)".*$/\1/p' \
    "${repo_root}/rust-toolchain.toml"
)"
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
if [[ -n "${MOCK_PROXY_LOG:-}" ]]; then
  printf 'cargo invoked\n' >> "${MOCK_PROXY_LOG}"
  exit 99
fi
exit 0
EOF

cat > "${mock_bin}/rustc" <<'EOF'
#!/usr/bin/env bash
if [[ -n "${MOCK_PROXY_LOG:-}" ]]; then
  printf 'rustc invoked\n' >> "${MOCK_PROXY_LOG}"
  exit 99
fi
exit 0
EOF

cat > "${mock_bin}/rustup" <<'EOF'
#!/usr/bin/env bash
if [[ -n "${MOCK_RUSTUP_LOG:-}" ]]; then
  printf '%s\n' "$*" >> "${MOCK_RUSTUP_LOG}"
fi
if [[ "${1:-}" == component && "${2:-}" == list && "${3:-}" == --installed ]]; then
  printf '%s\n' ${MOCK_RUST_COMPONENTS:-cargo-x86_64-unknown-linux-gnu rustc-x86_64-unknown-linux-gnu rust-std-x86_64-unknown-linux-gnu}
  exit 0
fi
if [[ "${1:-}" == toolchain && "${2:-}" == list ]]; then
  printf '%s\n' ${MOCK_RUST_TOOLCHAINS:-1.89-x86_64-unknown-linux-gnu}
  exit 0
fi
if [[ "${1:-}" == toolchain && "${2:-}" == install ]]; then
  exit 0
fi
if [[ "${1:-}" == component && "${2:-}" == add ]]; then
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
libclang-dev
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
proxy_log="${tmp}/proxy.log"
: > "${proxy_log}"

run_preflight() {
  local output="${tmp}/output"
  : > "${output}"
  PATH="${mock_bin}:${PATH}" \
    FIREACTIONS_RUNNER_IMAGE_CONTRACT_DIR="${contract_dir}" \
    RUST_SETUP_COMPONENTS="${1:-}" \
    MOCK_MISSING_PACKAGE="${MOCK_MISSING_PACKAGE:-}" \
    MOCK_RUST_COMPONENTS="${MOCK_RUST_COMPONENTS:-}" \
    MOCK_RUST_TOOLCHAINS="${MOCK_RUST_TOOLCHAINS:-${repo_toolchain}-x86_64-unknown-linux-gnu}" \
    MOCK_PROXY_LOG="${proxy_log}" \
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
assert_output "${output}" "rustup_ready=true"
assert_output "${output}" "toolchain_ready=true"
test ! -s "${proxy_log}"

MOCK_RUST_COMPONENTS="cargo-x86_64-unknown-linux-gnu rustc-x86_64-unknown-linux-gnu rust-std-x86_64-unknown-linux-gnu clippy-x86_64-unknown-linux-gnu rustfmt-x86_64-unknown-linux-gnu"
output="$(run_preflight 'clippy,rustfmt')"
assert_output "${output}" "system_ready=true"
assert_output "${output}" "toolchain_ready=true"

output="$(run_preflight 'clippy,rust-src')"
assert_output "${output}" "toolchain_ready=false"

MOCK_MISSING_PACKAGE=libclang-dev
output="$(run_preflight)"
assert_output "${output}" "system_ready=false"
assert_output "${output}" "toolchain_ready=true"
unset MOCK_MISSING_PACKAGE

MOCK_RUST_TOOLCHAINS=0.0-x86_64-unknown-linux-gnu
output="$(run_preflight)"
assert_output "${output}" "toolchain_ready=false"
unset MOCK_RUST_TOOLCHAINS

mv "${contract_dir}/image-contract.env" "${contract_dir}/image-contract.env.missing"
output="$(run_preflight)"
assert_output "${output}" "system_ready=false"
assert_output "${output}" "toolchain_ready=false"

grep -Fq 'run: .github/scripts/rust-setup-preflight.sh "$GITHUB_OUTPUT"' "${action}"
grep -Fq "if: steps.runner-image.outputs.system_ready != 'true'" "${action}"
grep -Fq "if: steps.runner-image.outputs.toolchain_ready != 'true'" "${action}"
grep -Fq "steps.runner-image.outputs.rustup_ready != 'true'" "${action}"
grep -Fq 'RUST_SETUP_COMPONENTS: ${{ inputs.components }}' "${action}"
grep -Fq 'run: .github/scripts/install-rust-toolchain.sh "$RUST_SETUP_COMPONENTS"' "${action}"
grep -Fq 'libclang-dev' "${action}"

rustup_log="${tmp}/rustup.log"
: > "${rustup_log}"
PATH="${mock_bin}:${PATH}" \
  MOCK_RUSTUP_LOG="${rustup_log}" \
  "${installer}" 'clippy, rustfmt' >/dev/null
grep -Fxq 'toolchain install --no-self-update' "${rustup_log}"
grep -Fxq 'component add clippy' "${rustup_log}"
grep -Fxq 'component add rustfmt' "${rustup_log}"

if PATH="${mock_bin}:${PATH}" "${installer}" 'clippy,bad/component' >/dev/null 2>&1; then
  echo "expected an invalid extra component to fail" >&2
  exit 1
fi

echo "rust setup preflight tests passed"
