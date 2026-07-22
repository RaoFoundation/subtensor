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
  libclang-dev
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
rustup_ready=false
toolchain_ready=false

if command -v rustup >/dev/null 2>&1; then
  rustup_ready=true
fi

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
  && [[ -r rust-toolchain.toml ]] \
  && command -v rustup >/dev/null 2>&1 \
  && command -v cargo >/dev/null 2>&1 \
  && command -v rustc >/dev/null 2>&1; then
  requested_toolchain=""
  requested_toolchain_count=0
  while IFS= read -r channel; do
    requested_toolchain="${channel}"
    requested_toolchain_count=$((requested_toolchain_count + 1))
  done < <(
    sed -nE 's/^[[:space:]]*channel[[:space:]]*=[[:space:]]*"([^"[:space:]]+)".*$/\1/p' \
      rust-toolchain.toml
  )
  if [[ "${requested_toolchain_count}" -ne 1 ]] \
    || [[ ! "${requested_toolchain}" =~ ^[a-zA-Z0-9][a-zA-Z0-9._-]*$ ]]; then
    requested_toolchain=""
  fi

  installed_toolchain=""
  while IFS= read -r toolchain_line; do
    candidate="${toolchain_line%% *}"
    if [[ "${candidate}" == "${requested_toolchain}" \
      || "${candidate}" == "${requested_toolchain}-"* ]]; then
      installed_toolchain="${candidate}"
      break
    fi
  done < <(rustup toolchain list 2>/dev/null || true)

  if [[ -n "${requested_toolchain}" && -n "${installed_toolchain}" ]]; then
    toolchain_ready=true
    installed_components="$(
      rustup component list --installed --toolchain "${installed_toolchain}" 2>/dev/null || true
    )"
    required_components=(cargo rustc rust-std)
    if [[ -n "${RUST_SETUP_COMPONENTS:-}" ]]; then
      IFS=',' read -r -a extra_components <<< "${RUST_SETUP_COMPONENTS}"
      required_components+=("${extra_components[@]}")
    fi
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
  echo "rustup_ready=${rustup_ready}"
  echo "toolchain_ready=${toolchain_ready}"
} >> "${output_file}"

echo "runner image preflight: system_ready=${system_ready} rustup_ready=${rustup_ready} toolchain_ready=${toolchain_ready}"
