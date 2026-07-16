#!/usr/bin/env bash

# Secret-safe configuration boundary for the shared R2 sccache backend.
#
# prepare MODE CONFIG_FILE OUTPUT_FILE
#   Fetches and validates the MMDS reader contract, or materializes the trusted
#   writer contract from the protected Environment credentials.
#
# activate CONFIG_FILE ENV_FILE OUTPUT_FILE
#   Starts sccache against R2 and exports the wrapper only after startup works.
#
# Writer mode is write-through and content-addressed. Each successful rustc
# invocation is independently reusable even if a later compile or test fails;
# changed inputs produce a different key instead of replacing older artifacts.

set -u

readonly MMDS_TOKEN_URL_DEFAULT="http://169.254.169.254/latest/api/token"
readonly MMDS_METADATA_URL_DEFAULT="http://169.254.169.254/latest/meta-data/sccache"
readonly SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
readonly SCCACHE_CONFIG_TOOL="$SCRIPT_DIR/sccache-config.py"

warning() {
  printf '::warning::%s\n' "$1"
}

set_output() {
  local output_file="$1"
  local name="$2"
  local value="$3"
  printf '%s=%s\n' "$name" "$value" >> "$output_file"
}

disable_prepare() {
  local config_file="$1"
  local output_file="$2"
  local reason="$3"
  if [[ -n "$config_file" ]]; then
    rm -f "$config_file"
  fi
  set_output "$output_file" available false
  warning "sccache disabled: $reason"
  exit 0
}

disable_activate() {
  local config_file="$1"
  local output_file="$2"
  local reason="$3"
  if [[ -n "$config_file" ]]; then
    rm -f "$config_file"
  fi
  set_output "$output_file" enabled false
  warning "sccache disabled: $reason"
  exit 0
}

mask_config_credentials() {
  local config_file="$1"
  local -a credentials=()
  local credential
  while IFS= read -r credential; do
    credentials+=("$credential")
  done < <(
    "$SCCACHE_CONFIG_TOOL" credentials "$config_file"
  )
  if [[ ${#credentials[@]} -gt 0 ]]; then
    for credential in "${credentials[@]}"; do
      [[ -n "$credential" ]] && printf '::add-mask::%s\n' "$credential"
    done
  fi
}

prepare_gha() {
  local config_file="$1"
  printf '{"mode":"gha"}' > "$config_file"
  chmod 0600 "$config_file"
}

fallback_reader() {
  local config_file="$1"
  local output_file="$2"
  local reason="$3"
  if [[ "${SCCACHE_GHA_FALLBACK:-true}" == true ]]; then
    prepare_gha "$config_file"
    warning "R2 reader unavailable; using GitHub Actions sccache: $reason"
    return 0
  fi
  disable_prepare "$config_file" "$output_file" "$reason"
}

prepare_reader() {
  local config_file="$1"
  local output_file="$2"
  local token_url="${MMDS_TOKEN_URL:-$MMDS_TOKEN_URL_DEFAULT}"
  local metadata_url="${MMDS_METADATA_URL:-$MMDS_METADATA_URL_DEFAULT}"
  local token

  if ! token="$(curl --fail --silent --show-error --connect-timeout 1 --max-time 2 \
    --request PUT \
    --header 'X-Metadata-Token-TTL-Seconds: 60' \
    "$token_url" 2>/dev/null)"; then
    fallback_reader "$config_file" "$output_file" "MMDSv2 token service is unavailable"
    return
  fi

  if [[ -z "$token" ]]; then
    fallback_reader "$config_file" "$output_file" "MMDSv2 returned an empty token"
    return
  fi

  if ! curl --fail --silent --show-error --connect-timeout 1 --max-time 3 \
    --header "X-Metadata-Token: $token" \
    --header 'Accept: application/json' \
    --output "$config_file" \
    "$metadata_url" 2>/dev/null; then
    fallback_reader "$config_file" "$output_file" "MMDSv2 sccache metadata is unavailable"
    return
  fi
  chmod 0600 "$config_file"

  if ! "$SCCACHE_CONFIG_TOOL" normalize-reader \
    "$config_file" "${SCCACHE_LOCAL_TIER_MODE:-auto}" 2>/dev/null; then
    fallback_reader "$config_file" "$output_file" "MMDSv2 sccache metadata failed validation"
    return
  fi
}

attach_local_metadata() {
  local config_file="$1"
  local local_mode="${SCCACHE_LOCAL_TIER_MODE:-auto}"
  local token_url="${MMDS_TOKEN_URL:-$MMDS_TOKEN_URL_DEFAULT}"
  local metadata_url="${MMDS_METADATA_URL:-$MMDS_METADATA_URL_DEFAULT}"
  local metadata_file="${config_file}.mmds"
  local token

  [[ "$local_mode" == auto || "$local_mode" == disabled ]] || return 1
  [[ "$local_mode" == auto ]] || return 0
  if ! token="$(curl --fail --silent --show-error --connect-timeout 1 --max-time 2 \
    --request PUT --header 'X-Metadata-Token-TTL-Seconds: 60' "$token_url" 2>/dev/null)"; then
    warning "local sccache tier unavailable to trusted writer; using direct R2"
    return 0
  fi
  if ! curl --fail --silent --show-error --connect-timeout 1 --max-time 3 \
    --header "X-Metadata-Token: $token" --header 'Accept: application/json' \
    --output "$metadata_file" "$metadata_url" 2>/dev/null; then
    warning "local sccache metadata unavailable to trusted writer; using direct R2"
    rm -f "$metadata_file"
    return 0
  fi
  if ! "$SCCACHE_CONFIG_TOOL" attach-local \
    "$config_file" "$metadata_file" 2>/dev/null; then
    warning "local sccache metadata failed validation; using direct R2"
  fi
  rm -f "$metadata_file"
}

prepare_writer() {
  local config_file="$1"
  local output_file="$2"

  if ! writer_source_is_trusted; then
    disable_prepare "$config_file" "$output_file" "writer mode is restricted to trusted cache sources"
  fi

  if [[ -z "${AWS_ACCESS_KEY_ID:-}" || -z "${AWS_SECRET_ACCESS_KEY:-}" ]]; then
    disable_prepare "$config_file" "$output_file" "protected writer credentials are unavailable"
  fi
  if [[ "$AWS_ACCESS_KEY_ID" == *$'\n'* || "$AWS_SECRET_ACCESS_KEY" == *$'\n'* ]]; then
    disable_prepare "$config_file" "$output_file" "protected writer credentials are malformed"
  fi

  if ! "$SCCACHE_CONFIG_TOOL" write-writer "$config_file" 2>/dev/null; then
    disable_prepare "$config_file" "$output_file" "writer configuration could not be materialized"
  fi
  attach_local_metadata "$config_file" ||
    disable_prepare "$config_file" "$output_file" "invalid local tier mode"
}

writer_source_is_trusted() {
  "$SCCACHE_CONFIG_TOOL" source-trusted "${GITHUB_EVENT_PATH:-}" \
    >/dev/null 2>&1
}

prepare_auto() {
  local config_file="$1"
  local output_file="$2"

  if writer_source_is_trusted &&
      [[ -n "${AWS_ACCESS_KEY_ID:-}" && -n "${AWS_SECRET_ACCESS_KEY:-}" ]] &&
      [[ "$AWS_ACCESS_KEY_ID" != *$'\n'* && "$AWS_SECRET_ACCESS_KEY" != *$'\n'* ]]; then
    prepare_writer "$config_file" "$output_file"
    return
  fi

  prepare_reader "$config_file" "$output_file"
}

prepare() {
  local mode="$1"
  local config_file="$2"
  local output_file="$3"

  umask 077
  mkdir -p "$(dirname "$config_file")"
  rm -f "$config_file"

  case "$mode" in
    reader) prepare_reader "$config_file" "$output_file" ;;
    writer) prepare_writer "$config_file" "$output_file" ;;
    auto) prepare_auto "$config_file" "$output_file" ;;
    *) disable_prepare "$config_file" "$output_file" "unknown credential mode" ;;
  esac

  mask_config_credentials "$config_file"
  set_output "$output_file" available true
  set_output "$output_file" config-file "$config_file"
  printf 'sccache %s configuration validated\n' "$mode"
}

activate() {
  local config_file="$1"
  local env_file="$2"
  local output_file="$3"
  local install_outcome="${SCCACHE_INSTALL_OUTCOME:-success}"
  local sccache_bin="${SCCACHE_PATH:-}"
  local start_log="${RUNNER_TEMP:-/tmp}/sccache-start-${GITHUB_RUN_ID:-local}-${GITHUB_JOB:-test}.log"
  local fields_file="${RUNNER_TEMP:-/tmp}/sccache-fields-${GITHUB_RUN_ID:-local}-${GITHUB_JOB:-test}"
  local field_name field_value
  local config_mode=""
  local config_bucket=""
  local config_endpoint=""
  local config_region=""
  local config_key_prefix=""
  local config_access_key_id=""
  local config_secret_access_key=""
  local config_local_enabled=false
  local config_local_endpoint=""
  local config_local_key_prefix=""
  local config_local_username=""
  local config_local_password=""

  umask 077
  if [[ ! -f "$config_file" ]]; then
    disable_activate "$config_file" "$output_file" "validated configuration is unavailable"
  fi
  if [[ "$install_outcome" != "success" ]]; then
    disable_activate "$config_file" "$output_file" "sccache installation failed"
  fi
  if [[ -z "$sccache_bin" ]]; then
    sccache_bin="$(command -v sccache || true)"
  fi
  if [[ -z "$sccache_bin" || ! -x "$sccache_bin" ]]; then
    disable_activate "$config_file" "$output_file" "sccache executable is unavailable"
  fi

  if ! "$SCCACHE_CONFIG_TOOL" fields "$config_file" \
    >"$fields_file" 2>/dev/null; then
    rm -f "$fields_file"
    disable_activate "$config_file" "$output_file" "validated configuration could not be parsed"
  fi
  while IFS= read -r -d '' field_name && IFS= read -r -d '' field_value; do
    case "$field_name" in
      mode) config_mode="$field_value" ;;
      bucket) config_bucket="$field_value" ;;
      endpoint) config_endpoint="$field_value" ;;
      region) config_region="$field_value" ;;
      key_prefix) config_key_prefix="$field_value" ;;
      access_key_id) config_access_key_id="$field_value" ;;
      secret_access_key) config_secret_access_key="$field_value" ;;
      local_enabled) config_local_enabled="$field_value" ;;
      local_endpoint) config_local_endpoint="$field_value" ;;
      local_key_prefix) config_local_key_prefix="$field_value" ;;
      local_username) config_local_username="$field_value" ;;
      local_password) config_local_password="$field_value" ;;
      *)
        rm -f "$fields_file"
        disable_activate "$config_file" "$output_file" "validated configuration contains an unknown field"
        ;;
    esac
  done < "$fields_file"
  rm -f "$fields_file"
  if [[ -z "$config_mode" ]]; then
    disable_activate "$config_file" "$output_file" "validated configuration is incomplete"
  fi

  if [[ "$config_mode" == gha ]]; then
    export SCCACHE_GHA_ENABLED=true
    export SCCACHE_IGNORE_SERVER_IO_ERROR=1
    "$sccache_bin" --stop-server >/dev/null 2>&1 || true
    if ! "$sccache_bin" --start-server >"$start_log" 2>&1; then
      "$sccache_bin" --stop-server >/dev/null 2>&1 || true
      rm -f "$start_log"
      disable_activate "$config_file" "$output_file" "GitHub Actions backend startup failed"
    fi
    rm -f "$start_log" "$config_file"
    {
      printf 'SCCACHE_ENABLED=true\n'
      printf 'SCCACHE_BACKEND=gha\n'
      printf 'SCCACHE_GHA_ENABLED=true\n'
      printf 'SCCACHE_IGNORE_SERVER_IO_ERROR=1\n'
      printf 'RUSTC_WRAPPER=sccache\n'
      printf 'CARGO_INCREMENTAL=0\n'
    } >> "$env_file"
    set_output "$output_file" enabled true
    printf 'sccache GitHub Actions backend enabled\n'
    return
  fi

  printf '::add-mask::%s\n' "$config_access_key_id"
  printf '::add-mask::%s\n' "$config_secret_access_key"

  export SCCACHE_BUCKET="$config_bucket"
  export SCCACHE_ENDPOINT="$config_endpoint"
  export SCCACHE_REGION="$config_region"
  export SCCACHE_S3_USE_SSL=true
  export SCCACHE_S3_KEY_PREFIX="$config_key_prefix"
  # v0.15.0 has no S3 read/write-mode environment variable. Reader mode is
  # enforced by the bucket-scoped Object Read credential validated above.
  # sccache otherwise fails a build when its server or remote backend becomes
  # unavailable after startup. This makes every such failure compile locally.
  export SCCACHE_IGNORE_SERVER_IO_ERROR=1
  export AWS_ACCESS_KEY_ID="$config_access_key_id"
  export AWS_SECRET_ACCESS_KEY="$config_secret_access_key"

  local local_tier=false
  if [[ "$config_local_enabled" == true ]]; then
    printf '::add-mask::%s\n' "$config_local_username"
    printf '::add-mask::%s\n' "$config_local_password"
    export SCCACHE_MULTILEVEL_CHAIN=webdav,s3
    export SCCACHE_MULTILEVEL_WRITE_ERROR_POLICY=ignore
    export SCCACHE_WEBDAV_ENDPOINT="$config_local_endpoint"
    export SCCACHE_WEBDAV_KEY_PREFIX="$config_local_key_prefix"
    export SCCACHE_WEBDAV_USERNAME="$config_local_username"
    export SCCACHE_WEBDAV_PASSWORD="$config_local_password"
    local_tier=true
  fi

  "$sccache_bin" --stop-server >/dev/null 2>&1 || true
  if ! "$sccache_bin" --start-server >"$start_log" 2>&1; then
    if [[ "$local_tier" == true ]]; then
      warning "local sccache tier startup failed; retrying direct R2"
      "$sccache_bin" --stop-server >/dev/null 2>&1 || true
      unset SCCACHE_MULTILEVEL_CHAIN SCCACHE_MULTILEVEL_WRITE_ERROR_POLICY
      unset SCCACHE_WEBDAV_ENDPOINT SCCACHE_WEBDAV_KEY_PREFIX
      unset SCCACHE_WEBDAV_USERNAME SCCACHE_WEBDAV_PASSWORD
      local_tier=false
      if ! "$sccache_bin" --start-server >"$start_log" 2>&1; then
        "$sccache_bin" --stop-server >/dev/null 2>&1 || true
        rm -f "$start_log"
        disable_activate "$config_file" "$output_file" "R2 backend startup check failed"
      fi
    else
      "$sccache_bin" --stop-server >/dev/null 2>&1 || true
      rm -f "$start_log"
      disable_activate "$config_file" "$output_file" "R2 backend startup check failed"
    fi
  fi
  rm -f "$start_log" "$config_file"

  {
    printf 'SCCACHE_ENABLED=true\n'
    printf 'SCCACHE_BACKEND=r2\n'
    printf 'RUSTC_WRAPPER=sccache\n'
    printf 'CARGO_INCREMENTAL=0\n'
    printf 'SCCACHE_BUCKET=%s\n' "$SCCACHE_BUCKET"
    printf 'SCCACHE_ENDPOINT=%s\n' "$SCCACHE_ENDPOINT"
    printf 'SCCACHE_REGION=%s\n' "$SCCACHE_REGION"
    printf 'SCCACHE_S3_USE_SSL=true\n'
    printf 'SCCACHE_S3_KEY_PREFIX=%s\n' "$SCCACHE_S3_KEY_PREFIX"
    printf 'SCCACHE_IGNORE_SERVER_IO_ERROR=1\n'
    printf 'AWS_ACCESS_KEY_ID=%s\n' "$AWS_ACCESS_KEY_ID"
    printf 'AWS_SECRET_ACCESS_KEY=%s\n' "$AWS_SECRET_ACCESS_KEY"
    printf 'SCCACHE_LOCAL_TIER=%s\n' "$local_tier"
    if [[ "$local_tier" == true ]]; then
      printf 'SCCACHE_MULTILEVEL_CHAIN=%s\n' "$SCCACHE_MULTILEVEL_CHAIN"
      printf 'SCCACHE_MULTILEVEL_WRITE_ERROR_POLICY=%s\n' "$SCCACHE_MULTILEVEL_WRITE_ERROR_POLICY"
      printf 'SCCACHE_WEBDAV_ENDPOINT=%s\n' "$SCCACHE_WEBDAV_ENDPOINT"
      printf 'SCCACHE_WEBDAV_KEY_PREFIX=%s\n' "$SCCACHE_WEBDAV_KEY_PREFIX"
      printf 'SCCACHE_WEBDAV_USERNAME=%s\n' "$SCCACHE_WEBDAV_USERNAME"
      printf 'SCCACHE_WEBDAV_PASSWORD=%s\n' "$SCCACHE_WEBDAV_PASSWORD"
    fi
  } >> "$env_file"
  set_output "$output_file" enabled true
  printf 'sccache R2 backend enabled in %s mode\n' "$config_mode"
}

if [[ $# -lt 1 ]]; then
  printf 'usage: %s prepare|activate ...\n' "$0" >&2
  exit 2
fi

case "$1" in
  prepare)
    [[ $# -eq 4 ]] || { printf 'usage: %s prepare MODE CONFIG_FILE OUTPUT_FILE\n' "$0" >&2; exit 2; }
    prepare "$2" "$3" "$4"
    ;;
  activate)
    [[ $# -eq 4 ]] || { printf 'usage: %s activate CONFIG_FILE ENV_FILE OUTPUT_FILE\n' "$0" >&2; exit 2; }
    activate "$2" "$3" "$4"
    ;;
  *)
    printf 'unknown command: %s\n' "$1" >&2
    exit 2
    ;;
esac
