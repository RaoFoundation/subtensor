#!/usr/bin/env bash
# Install the pre-push hook that runs scripts/preflight.sh before every push.
#
#   scripts/install-hooks.sh
#
# The hook runs the fast gate by default and the full gate (release node build,
# SDK bindings drift, try-runtime) when the pushed commits touch runtime/,
# pallets/, or sdk/. A red gate aborts the push. `git push --no-verify`
# bypasses this hook and is forbidden for agents.
set -euo pipefail

cd "$(git rev-parse --show-toplevel)"
hooks_dir=$(git rev-parse --git-path hooks)
mkdir -p "$hooks_dir"
hook="$hooks_dir/pre-push"
if [[ -e "$hook" ]] && ! grep -q 'Installed by scripts/install-hooks.sh' "$hook"; then
  mv "$hook" "$hook.bak.$(date +%s)"
  echo "existing pre-push hook moved to $hook.bak.*; re-add anything you need to it"
fi

cat >"$hook" <<'EOF'
#!/usr/bin/env bash
# Installed by scripts/install-hooks.sh. Do not bypass with --no-verify.
# Gates the revisions being pushed, not the working tree: each pushed commit
# is checked out into a detached worktree and the gate runs there. The gate
# also verifies that the credential for this destination belongs to unarbos.
set -euo pipefail
cd "$(git rev-parse --show-toplevel)"
export PREFLIGHT_PUSH_REMOTE=$1 PREFLIGHT_PUSH_URL=$2
if [[ ! -x scripts/preflight.sh ]]; then
  echo "pre-push: scripts/preflight.sh not found on this branch; nothing to gate" >&2
  exit 0
fi
head=$(git rev-parse HEAD)
# stdin: <local ref> <local sha> <remote ref> <remote sha>, one line per ref.
while read -r local_ref local_sha _remote_ref remote_sha; do
  [[ "$local_sha" =~ ^0+$ ]] && continue # deleting a remote branch
  if [[ "$remote_sha" =~ ^0+$ ]] || ! git cat-file -e "$remote_sha" 2>/dev/null; then
    range="$(git merge-base "${PREFLIGHT_BASE:-origin/main}" "$local_sha")..$local_sha"
  else
    range="$remote_sha..$local_sha"
  fi
  mode=--fast
  if git diff --name-only "$range" | grep -qE '^(runtime|pallets|sdk)/'; then
    mode=
  fi
  if [[ "$local_sha" == "$head" ]] && dirty=$(git status --porcelain --untracked-files=no) && [[ -n "$dirty" ]]; then
    echo "pre-push: rejected. The working tree differs from the pushed HEAD ${head:0:9}; the gate" >&2
    echo "would verify edits that are not in this push. Commit or stash them first:" >&2
    echo "$dirty" >&2
    exit 1
  fi
  echo "pre-push: gating $local_ref (${local_sha:0:9}) ${mode:-with the full gate}" >&2
  scripts/preflight.sh --rev "$local_sha" $mode
done
EOF
chmod +x "$hook"
echo "installed $hook"
echo "every push now runs scripts/preflight.sh; do not use git push --no-verify"
