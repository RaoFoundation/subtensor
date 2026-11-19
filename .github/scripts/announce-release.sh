#!/usr/bin/env bash
# Render a release announcement and, unless this is a dry run, POST it.
# A pull_request job never calls Discord and never calls the model.
# The model may replace only the prose section, and only with text that
# passed the sanitizer. Versions, hashes, install commands, and links stay
# in the template. A model failure posts the human summary.
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

printf '%s\n' '{"status":"skipped","text":"","reason":"no committed release notes"}' >"$workdir/model.json"
if [ "${ANNOUNCE_SAMPLE:-false}" = true ]; then
  printf '%s\n' '{"status":"skipped","text":"","reason":"sample text is not sent to the model"}' >"$workdir/model.json"
elif [ -n "${ANNOUNCE_NOTES_FILE:-}" ]; then
  if ! python3 "$repo_root/.github/scripts/rewrite-announce-summary.py" \
    --notes "$ANNOUNCE_NOTES_FILE" \
    --root "$repo_root" \
    ${ANNOUNCE_REVISION:+--revision "$ANNOUNCE_REVISION"} \
    --out "$workdir/model.json"; then
    printf '%s\n' '{"status":"error","text":"","reason":"model rewrite failed"}' >"$workdir/model.json"
  fi
fi
# The Discord request must not carry the model key.
unset ANNOUNCE_OPENAI_API_KEY

model_status=$(jq -r '.status // "error"' "$workdir/model.json")
if [ "$model_status" = "accepted" ]; then
  jq -rj '.text' "$workdir/model.json" >"$workdir/model.txt"
  args+=(--model-summary-file "$workdir/model.txt")
fi

render() {
  "${args[@]}" \
    --out "$workdir/message.txt" \
    --summary-out "$workdir/summary.txt" \
    --human-out "$workdir/human.txt" \
    --facts-out "$workdir/facts.txt" \
    --model-call-json "$workdir/model.json" \
    --dry-run-summary-out "$workdir/dry-run.md" \
    >/dev/null
}

render

write_summary() {
  if [ -n "${GITHUB_STEP_SUMMARY:-}" ]; then
    cat "$workdir/dry-run.md" >>"$GITHUB_STEP_SUMMARY"
  fi
  # The step summary is the readable copy. The log is shielded so a line in
  # the model prose cannot be read as a workflow command.
  python3 -c 'import sys; sys.path.insert(0, sys.argv[1]); import release_notes; sys.stdout.write(release_notes.shield_actions_log(open(sys.argv[2], encoding="utf-8").read()))' \
    "$repo_root/.github/scripts" "$workdir/dry-run.md"
}

if [ "$dry" = true ]; then
  write_summary
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
