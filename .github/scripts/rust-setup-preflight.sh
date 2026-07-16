#!/usr/bin/env bash
set -euo pipefail

output_file="${1:?usage: rust-setup-preflight.sh GITHUB_OUTPUT}"
contract_dir="${FIREACTIONS_RUNNER_IMAGE_CONTRACT_DIR:-/etc/fireactions-runner-image}"
contract_file="${contract_dir}/image-contract.env"
packages_file="${contract_dir}/packages.txt"

required_packages=(
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
)

system_ready=false
toolchain_ready=false

if [[ -r "${contract_file}" && -r "${packages_file}" ]] && command -v dpkg-query >/dev/null 2>&1; then
  system_ready=true
  for package in "${required_packages[@]}"; do
    if ! grep -Fxq -- "${package}" "${packages_file}"; then
      system_ready=false
      break
    fi
    package_status="$(dpkg-query -W -f='${db:Status-Abbrev}' "${package}" 2>/dev/null || true)"
    if [[ "${package_status}" != "ii " ]]; then
      system_ready=false
      break
    fi
  done
fi

if [[ -r "${contract_file}" ]] \
  && command -v rustup >/dev/null 2>&1 \
  && command -v cargo >/dev/null 2>&1 \
  && command -v rustc >/dev/null 2>&1 \
  && cargo --version >/dev/null 2>&1 \
  && rustc --version >/dev/null 2>&1; then
  toolchain_ready=true
  installed_components="$(rustup component list --installed 2>/dev/null || true)"
  if [[ -n "${RUST_SETUP_COMPONENTS:-}" ]]; then
    IFS=',' read -r -a required_components <<< "${RUST_SETUP_COMPONENTS}"
    for component in "${required_components[@]}"; do
      component="${component//[[:space:]]/}"
      [[ -z "${component}" ]] && continue
      component_ready=false
      while IFS= read -r installed_component; do
        if [[ "${installed_component}" == "${component}" || "${installed_component}" == "${component}-"* ]]; then
          component_ready=true
          break
        fi
      done <<< "${installed_components}"
      if [[ "${component_ready}" != true ]]; then
        toolchain_ready=false
        break
      fi
    done
  fi
fi

{
  echo "system_ready=${system_ready}"
  echo "toolchain_ready=${toolchain_ready}"
} >> "${output_file}"

echo "runner image preflight: system_ready=${system_ready} toolchain_ready=${toolchain_ready}"
