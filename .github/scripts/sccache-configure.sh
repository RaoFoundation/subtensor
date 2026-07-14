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
    python3 -c '
import json, sys
data = json.load(open(sys.argv[1], encoding="utf-8"))
if data.get("mode") == "gha":
    raise SystemExit(0)
for key in ("access_key_id", "secret_access_key"):
    print(data[key])
' "$config_file"
  )
  if [[ ${#credentials[@]} -eq 2 ]]; then
    printf '::add-mask::%s\n' "${credentials[0]}"
    printf '::add-mask::%s\n' "${credentials[1]}"
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

  if ! python3 -c '
import json, os, sys
path = sys.argv[1]
expected = {
    "bucket": "subtensor-ci-sccache",
    "endpoint": "https://3dc4cebb791314d78848969042fb3382.r2.cloudflarestorage.com",
    "region": "auto",
    "s3_use_ssl": True,
    "s3_rw_mode": "READ_ONLY",
    "key_prefix": "subtensor/v1",
}
with open(path, encoding="utf-8") as handle:
    data = json.load(handle)
for key, value in expected.items():
    if data.get(key) != value:
        raise ValueError(f"invalid {key}")
for key in ("access_key_id", "secret_access_key"):
    value = data.get(key)
    if not isinstance(value, str) or not value or "\n" in value or "\r" in value:
        raise ValueError(f"invalid {key}")
data["mode"] = "reader"
tmp = path + ".normalized"
with open(tmp, "w", encoding="utf-8") as handle:
    json.dump(data, handle, separators=(",", ":"))
os.chmod(tmp, 0o600)
os.replace(tmp, path)
' "$config_file" 2>/dev/null; then
    fallback_reader "$config_file" "$output_file" "MMDSv2 sccache metadata failed validation"
    return
  fi
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

  if ! CONFIG_FILE="$config_file" python3 -c '
import json, os
path = os.environ["CONFIG_FILE"]
data = {
    "mode": "writer",
    "bucket": "subtensor-ci-sccache",
    "endpoint": "https://3dc4cebb791314d78848969042fb3382.r2.cloudflarestorage.com",
    "region": "auto",
    "s3_use_ssl": True,
    "s3_rw_mode": "READ_WRITE",
    "key_prefix": "subtensor/v1",
    "access_key_id": os.environ["AWS_ACCESS_KEY_ID"],
    "secret_access_key": os.environ["AWS_SECRET_ACCESS_KEY"],
}
with open(path, "w", encoding="utf-8") as handle:
    json.dump(data, handle, separators=(",", ":"))
os.chmod(path, 0o600)
' 2>/dev/null; then
    disable_prepare "$config_file" "$output_file" "writer configuration could not be materialized"
  fi
}

writer_source_is_trusted() {
  case "${GITHUB_EVENT_NAME:-}:${GITHUB_REF:-}" in
    push:refs/heads/main|push:refs/heads/devnet|push:refs/heads/testnet|schedule:refs/heads/main)
      return 0
      ;;
    workflow_dispatch:refs/heads/main)
      [[ -f "${GITHUB_EVENT_PATH:-}" ]] || return 1
      GITHUB_REPOSITORY="${GITHUB_REPOSITORY:-}" python3 -c '
import json, os, sys
with open(sys.argv[1], encoding="utf-8") as handle:
    event = json.load(handle)
source_ref = event.get("inputs", {}).get("source_ref")
if source_ref not in {"main", "devnet", "testnet"}:
    raise SystemExit(1)
' "$GITHUB_EVENT_PATH" >/dev/null 2>&1
      return
      ;;
    pull_request:refs/pull/*/merge)
      [[ -f "${GITHUB_EVENT_PATH:-}" && -n "${GITHUB_REPOSITORY:-}" ]] || return 1
      GITHUB_REPOSITORY="$GITHUB_REPOSITORY" python3 -c '
import json, os, re, sys
with open(sys.argv[1], encoding="utf-8") as handle:
    event = json.load(handle)
pull = event.get("pull_request")
if not isinstance(pull, dict):
    raise SystemExit(1)
head_repo = pull.get("head", {}).get("repo")
if not isinstance(head_repo, dict):
    raise SystemExit(1)
trusted = (
    re.fullmatch(r"refs/pull/[0-9]+/merge", os.environ.get("GITHUB_REF", ""))
    and head_repo.get("full_name") == os.environ["GITHUB_REPOSITORY"]
    and head_repo.get("fork") is False
    and pull.get("user", {}).get("login") != "dependabot[bot]"
)
raise SystemExit(0 if trusted else 1)
' "$GITHUB_EVENT_PATH" >/dev/null 2>&1
      return
      ;;
    *)
      return 1
      ;;
  esac
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
  local -a values=()
  local value

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

  if ! python3 -c '
import json, sys
data = json.load(open(sys.argv[1], encoding="utf-8"))
mode = data.get("mode")
if mode not in ("gha", "reader", "writer"):
    raise ValueError("invalid mode")
sys.stdout.write(mode + "\0")
if mode == "gha":
    raise SystemExit(0)
for key in ("bucket", "endpoint", "region", "key_prefix", "access_key_id", "secret_access_key"):
    value = data.get(key)
    if not isinstance(value, str) or not value or "\n" in value or "\r" in value:
        raise ValueError(f"invalid {key}")
    sys.stdout.write(value + "\0")
if data.get("s3_use_ssl") is not True:
    raise ValueError("invalid s3_use_ssl")
' "$config_file" >"$fields_file" 2>/dev/null; then
    rm -f "$fields_file"
    disable_activate "$config_file" "$output_file" "validated configuration could not be parsed"
  fi
  while IFS= read -r -d '' value; do
    values+=("$value")
  done < "$fields_file"
  rm -f "$fields_file"
  if [[ ${#values[@]} -ne 1 && ${#values[@]} -ne 7 ]]; then
    disable_activate "$config_file" "$output_file" "validated configuration is incomplete"
  fi

  if [[ "${values[0]}" == gha ]]; then
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

  printf '::add-mask::%s\n' "${values[5]}"
  printf '::add-mask::%s\n' "${values[6]}"

  export SCCACHE_BUCKET="${values[1]}"
  export SCCACHE_ENDPOINT="${values[2]}"
  export SCCACHE_REGION="${values[3]}"
  export SCCACHE_S3_USE_SSL=true
  export SCCACHE_S3_KEY_PREFIX="${values[4]}"
  # v0.15.0 has no S3 read/write-mode environment variable. Reader mode is
  # enforced by the bucket-scoped Object Read credential validated above.
  # sccache otherwise fails a build when its server or remote backend becomes
  # unavailable after startup. This makes every such failure compile locally.
  export SCCACHE_IGNORE_SERVER_IO_ERROR=1
  export AWS_ACCESS_KEY_ID="${values[5]}"
  export AWS_SECRET_ACCESS_KEY="${values[6]}"

  "$sccache_bin" --stop-server >/dev/null 2>&1 || true
  if ! "$sccache_bin" --start-server >"$start_log" 2>&1; then
    "$sccache_bin" --stop-server >/dev/null 2>&1 || true
    rm -f "$start_log"
    disable_activate "$config_file" "$output_file" "R2 backend startup check failed"
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
  } >> "$env_file"
  set_output "$output_file" enabled true
  printf 'sccache R2 backend enabled in %s mode\n' "${values[0]}"
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
