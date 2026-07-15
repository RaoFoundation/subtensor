#!/usr/bin/env bash

set -euo pipefail

image="${1:?usage: test-localnet-image.sh IMAGE}"
suffix="${GITHUB_RUN_ID:-local}-$$"
fast_container="subtensor-localnet-fast-${suffix}"
standard_container="subtensor-localnet-standard-${suffix}"
persistent_container="subtensor-localnet-persistent-${suffix}"
failure_container="subtensor-localnet-failure-${suffix}"
state_volume="subtensor-localnet-state-${suffix}"

cleanup() {
  docker rm -f "$fast_container" "$standard_container" "$persistent_container" \
    "$failure_container" \
    >/dev/null 2>&1 || true
  docker volume rm -f "$state_volume" >/dev/null 2>&1 || true
}
trap cleanup EXIT

show_failure() {
  local container="$1"
  echo "--- $container logs ---" >&2
  docker logs "$container" >&2 || true
  echo "--- $container inspect ---" >&2
  docker inspect "$container" >&2 || true
}

wait_healthy() {
  local container="$1"
  local attempts="${2:-90}"

  for ((attempt = 1; attempt <= attempts; attempt++)); do
    local running health
    running="$(docker inspect --format '{{.State.Running}}' "$container")"
    health="$(docker inspect --format '{{.State.Health.Status}}' "$container")"
    if [[ "$running" != true ]]; then
      show_failure "$container"
      return 1
    fi
    if [[ "$health" == healthy ]]; then
      return 0
    fi
    if [[ "$health" == unhealthy ]]; then
      show_failure "$container"
      return 1
    fi
    sleep 2
  done

  show_failure "$container"
  return 1
}

assert_rpc() {
  local container="$1"
  local port="$2"
  docker exec "$container" /scripts/localnet_healthcheck.sh "$port"
}

block_number() {
  local container="$1"
  local response hex
  response="$(
    docker exec "$container" curl --fail --silent --max-time 2 \
      -H 'content-type: application/json' \
      --data '{"id":1,"jsonrpc":"2.0","method":"chain_getHeader","params":[]}' \
      http://127.0.0.1:9944
  )"
  hex="$(sed -n 's/.*"number":"0x\([0-9a-fA-F]*\)".*/\1/p' <<<"$response")"
  [[ -n "$hex" ]] || {
    echo "could not parse block number from: $response" >&2
    return 1
  }
  printf '%d\n' "$((16#$hex))"
}

block_hash() {
  local container="$1"
  local number="$2"
  local response hash
  response="$(
    docker exec "$container" curl --fail --silent --max-time 2 \
      -H 'content-type: application/json' \
      --data "{\"id\":1,\"jsonrpc\":\"2.0\",\"method\":\"chain_getBlockHash\",\"params\":[$number]}" \
      http://127.0.0.1:9944
  )"
  hash="$(sed -n 's/.*"result":"\(0x[0-9a-fA-F]*\)".*/\1/p' <<<"$response")"
  [[ -n "$hash" ]] || {
    echo "could not parse block hash from: $response" >&2
    return 1
  }
  printf '%s\n' "$hash"
}

docker run --rm --entrypoint /target/fast-runtime/release/node-subtensor \
  "$image" --help >/dev/null
docker run --rm --entrypoint /target/non-fast-runtime/release/node-subtensor \
  "$image" --help >/dev/null

docker run -d --name "$fast_container" "$image" True >/dev/null
wait_healthy "$fast_container"
assert_rpc "$fast_container" 9944
assert_rpc "$fast_container" 9945
docker stop --time 40 "$fast_container" >/dev/null

docker run -d --name "$standard_container" "$image" False >/dev/null
wait_healthy "$standard_container"
assert_rpc "$standard_container" 9944
assert_rpc "$standard_container" 9945
docker stop --time 40 "$standard_container" >/dev/null

# The entrypoint supervises all three authorities. If one dies, the container
# must fail instead of silently leaving CI connected to a partial network.
docker run -d --name "$failure_container" "$image" True >/dev/null
wait_healthy "$failure_container"
docker exec "$failure_container" pkill -TERM -o node-subtensor
for _ in {1..30}; do
  if [[ "$(docker inspect --format '{{.State.Running}}' "$failure_container")" == false ]]; then
    break
  fi
  sleep 1
done
if [[ "$(docker inspect --format '{{.State.Running}}' "$failure_container")" != false ]]; then
  show_failure "$failure_container"
  exit 1
fi
if [[ "$(docker inspect --format '{{.State.ExitCode}}' "$failure_container")" == 0 ]]; then
  echo "localnet container succeeded after an authority exited" >&2
  exit 1
fi

docker volume create "$state_volume" >/dev/null
docker run -d --name "$persistent_container" -v "$state_volume:/tmp" \
  "$image" True >/dev/null
wait_healthy "$persistent_container"
before_restart="$(block_number "$persistent_container")"
before_hash="$(block_hash "$persistent_container" "$before_restart")"
docker stop --time 40 "$persistent_container" >/dev/null
docker rm "$persistent_container" >/dev/null

docker run -d --name "$persistent_container" -v "$state_volume:/tmp" \
  "$image" True --no-purge >/dev/null
wait_healthy "$persistent_container"
after_restart="$(block_number "$persistent_container")"
if ((after_restart < before_restart)); then
  echo "--no-purge lost chain state: before=$before_restart after=$after_restart" >&2
  exit 1
fi
after_hash="$(block_hash "$persistent_container" "$before_restart")"
if [[ "$after_hash" != "$before_hash" ]]; then
  echo "--no-purge changed block $before_restart: before=$before_hash after=$after_hash" >&2
  exit 1
fi

docker stop --time 40 "$persistent_container" >/dev/null
echo "localnet image compatibility smoke tests passed"
