#!/usr/bin/env bash
# Render a release announcement and, unless this is a dry run, POST it.
# A pull_request job never calls Discord. A missing model key posts the
# human summary. A model summary that adds a version, hash, or URL is dropped.
# The chain release does not depend on this script succeeding.

set -euo pipefail

script_dir=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
repo_root=$(cd -- "$script_dir/../.." && pwd)
cd "$repo_root"

if [ "${GITHUB_EVENT_NAME:-}" = "pull_request" ]; then
  ANNOUNCE_DRY_RUN=true
  DISCORD_WEBHOOK_URL=
  ANNOUNCE_OPENAI_API_KEY=
fi

dry=${ANNOUNCE_DRY_RUN:-false}
case "$dry" in
  true|false) ;;
  *)
    echo "dry-run must be true or false" >&2
    exit 1
    ;;
esac

utc=${ANNOUNCE_UTC:-$(date -u +%Y-%m-%dT%H:%M:%SZ)}
repo=${ANNOUNCE_REPO:-${GITHUB_REPOSITORY:-RaoFoundation/subtensor}}
context=${ANNOUNCE_STATUS_CONTEXT:-}
sha=${ANNOUNCE_SHA:?ANNOUNCE_SHA is required}

args=(
  python3 "$repo_root/.github/scripts/render-announce.py"
  --kind "${ANNOUNCE_KIND:?ANNOUNCE_KIND is required}"
  --sdk-version "${ANNOUNCE_SDK_VERSION:?ANNOUNCE_SDK_VERSION is required}"
  --core-version "${ANNOUNCE_CORE_VERSION:?ANNOUNCE_CORE_VERSION is required}"
  --sha "$sha"
  --utc "$utc"
  --repo "$repo"
  --root "$repo_root"
)
if [ -n "${ANNOUNCE_SPEC:-}" ]; then
  args+=(--spec "$ANNOUNCE_SPEC")
fi
if [ -n "${ANNOUNCE_SHA256:-}" ]; then
  args+=(--sha256 "$ANNOUNCE_SHA256")
fi
if [ -n "${ANNOUNCE_NOTES_FILE:-}" ]; then
  args+=(--notes "$ANNOUNCE_NOTES_FILE")
fi
if [ -n "${ANNOUNCE_REVISION:-}" ]; then
  args+=(--revision "$ANNOUNCE_REVISION")
fi
if [ "${ANNOUNCE_ALLOW_MISSING:-false}" = true ]; then
  args+=(--allow-missing-notes)
fi
if [ "${ANNOUNCE_SAMPLE:-false}" = true ]; then
  args+=(--sample)
fi
if [ -n "${ANNOUNCE_PUBLIC_PATH:-}" ]; then
  args+=(--public-path "$ANNOUNCE_PUBLIC_PATH")
fi

workdir=$(mktemp -d)
trap 'rm -rf "$workdir"' EXIT

render() {
  "${args[@]}" \
    --out "$workdir/message.txt" \
    --summary-out "$workdir/summary.txt" \
    --facts-out "$workdir/facts.txt" \
    >/dev/null
}

render

rewrite_summary() {
  if [ -z "${ANNOUNCE_OPENAI_API_KEY:-}" ]; then
    echo "ANNOUNCE_OPENAI_API_KEY is not set; using the human summary."
    return 0
  fi
  local body code
  if ! body=$(jq -n \
    --rawfile summary "$workdir/summary.txt" \
    --arg model "${ANNOUNCE_MODEL:-gpt-4o-mini}" \
    '{
      model: $model,
      temperature: 0,
      max_tokens: 400,
      messages: [
        {
          role: "system",
          content: "You rewrite a release summary. Return only the rewritten summary. Do not add a version number, a hash, a URL, a command, or a date. Do not add claims that are not in the summary."
        },
        {role: "user", content: $summary}
      ]
    }'); then
    echo "Could not build the OpenAI request; using the human summary." >&2
    return 0
  fi
  code=$(curl -sS --max-time 25 -o "$workdir/openai.json" -w '%{http_code}' \
    -H "Authorization: Bearer ${ANNOUNCE_OPENAI_API_KEY}" \
    -H "Content-Type: application/json" \
    --data-binary "$body" \
    https://api.openai.com/v1/chat/completions || true)
  if [ "$code" != "200" ]; then
    echo "OpenAI request failed (HTTP ${code:-none}); using the human summary." >&2
    return 0
  fi
  if ! jq -er '.choices[0].message.content' "$workdir/openai.json" >"$workdir/model.txt"; then
    echo "OpenAI response had no summary; using the human summary." >&2
    rm -f "$workdir/model.txt"
    return 0
  fi
  args+=(--model-summary-file "$workdir/model.txt")
  render
}

if [ "$dry" != true ]; then
  rewrite_summary
fi

write_summary() {
  if [ -z "${GITHUB_STEP_SUMMARY:-}" ]; then
    return 0
  fi
  {
    echo "## Discord dry run"
    echo ""
    echo "Discord was not called. The wasm sha256, when present, is only there to show the message shape."
    echo ""
    echo '```'
    cat "$workdir/message.txt"
    echo '```'
  } >>"$GITHUB_STEP_SUMMARY"
}

if [ "$dry" = true ]; then
  write_summary
  cat "$workdir/message.txt"
  exit 0
fi

if [ "${GITHUB_EVENT_NAME:-}" = "pull_request" ]; then
  echo "refusing to call Discord from a pull_request job" >&2
  exit 1
fi
if [ -n "${GITHUB_REF:-}" ] && [ "$GITHUB_REF" != "refs/heads/main" ]; then
  echo "refusing to call Discord from $GITHUB_REF" >&2
  exit 1
fi

if [ -z "${DISCORD_WEBHOOK_URL:-}" ]; then
  echo "DISCORD_WEBHOOK_URL is empty; not posting." >&2
  exit 1
fi
if [ -z "$context" ]; then
  echo "ANNOUNCE_STATUS_CONTEXT is required when posting." >&2
  exit 1
fi
case "$context" in
  announce/runtime-proposed|announce/runtime-live|announce/python-stable) ;;
  *)
    echo "refusing status context $context" >&2
    exit 1
    ;;
esac

if ! gh api "repos/${GITHUB_REPOSITORY}/commits/${sha}/status" >"$workdir/status.json"; then
  echo "could not read commit statuses; not posting." >&2
  exit 1
fi
current=$(jq -r --arg context "$context" \
  'first(.statuses[] | select(.context == $context) | .state) // ""' \
  "$workdir/status.json")
if [ "$current" = "success" ]; then
  echo "Commit status $context is already success. Not posting again."
  exit 0
fi

jq -n \
  --arg username "RaoFoundation" \
  --rawfile content "$workdir/message.txt" \
  '{username: $username, content: $content, allowed_mentions: {parse: []}}' >"$workdir/payload.json"

code=$(curl -sS --max-time 20 -o "$workdir/discord.txt" -w '%{http_code}' \
  -H "Content-Type: application/json" \
  -X POST \
  --data-binary @"$workdir/payload.json" \
  "$DISCORD_WEBHOOK_URL" || true)
case "$code" in
  2??) ;;
  *)
    echo "Discord returned HTTP ${code:-none}." >&2
    exit 1
    ;;
esac

description="Discord post sent"
target="${GITHUB_SERVER_URL:-https://github.com}/${GITHUB_REPOSITORY}/actions/runs/${GITHUB_RUN_ID:-0}"
attempt=1
while [ "$attempt" -le 3 ]; do
  if gh api --method POST "repos/${GITHUB_REPOSITORY}/statuses/${sha}" \
    -f state=success \
    -f context="$context" \
    -f description="$description" \
    -f target_url="$target" >/dev/null; then
    echo "Posted to Discord and recorded $context."
    exit 0
  fi
  attempt=$((attempt + 1))
  sleep 2
done
echo "Discord accepted the post, but the commit status write failed." >&2
exit 1
