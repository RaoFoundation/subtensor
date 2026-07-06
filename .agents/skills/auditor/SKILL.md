---
name: auditor
description: Run the domain-focused Auditor persona on the local working tree's diff against a base branch. May build/test if needed for confirmation. Outputs a verdict, optional suggested-changes patch, and (if relevant) a proposed PR description. Use after the Skeptic has cleared the branch, or directly when the user trusts their own code and wants the domain review.
---

# Auditor — local mode

You are running the Auditor persona locally against the user's working tree. The Skeptic has either already passed (or the user is running you directly because they wrote the code themselves and trust intent). Your output goes to the terminal, not GitHub.

## Step 1 — Determine the diff

Same detection as the Skeptic skill:
1. PR base via `gh pr view --json baseRefName` if a PR exists.
2. Default to `devnet-ready`.
3. Override via skill argument: `/auditor main`.

Compute the diff:

```bash
git fetch origin "$BASE" --quiet
git diff --merge-base "origin/$BASE"...HEAD
```

If the diff is empty, report "No changes vs $BASE" and exit.

## Step 2 — Run the persona

Load and follow:
- `.github/ai-review/common.md`
- `.github/ai-review/auditor.md`

**Local-mode adaptations:**

- **PR description handling**: if a PR exists, follow the persona's auto-fill / discrepancy-comment logic but do NOT actually call `gh pr edit`. Instead, write the proposed description to `.auditor-pr-description.md` and tell the user. If no PR exists, generate a draft description and write it to the same file — the user will use it when they open the PR.
- **Auto-fix CI failures**: you MAY run `./scripts/fix_rust.sh` against the working tree if lints / formatting are off, but DO NOT commit. Leave changes in the working tree for the user to review.
- **Spec version bump**: if the diff touches `runtime/` or `pallets/` and `spec_version` in `runtime/src/lib.rs` was not bumped, do NOT modify the file. Instead, surface this as a finding the user must address.
- **Build/test escalation**: same rules as the workflow — only build/test when a finding requires runtime confirmation. Use `cargo test -p <pallet> <test>` for targeted tests rather than the full workspace.
- **Duplicate-work check**: if a PR exists, run the same `gh pr list` check the persona file describes. If no PR exists, skip this step (no duplicates to check yet).

## Step 3 — Output

```
============================================================
  AUDITOR VERDICT: 👍 | 👎
============================================================

Gittensor: KNOWN | LIKELY | UNKNOWN
Spec version: <bumped | NOT BUMPED — required>
Auto-fix: <ran fix_rust.sh, modified N files | not needed>

Description: <see .auditor-pr-description.md | already adequate>
Duplicates: <none | PR #N is the better candidate>

Findings:
  [SEVERITY] Title
    file:line — description

Suggested new files:
  path/to/new_test.rs (see .auditor-suggestions.patch)

Conclusion: <one or two sentences>
```

Write any suggested code changes to `.auditor-suggestions.patch` (apply with `git apply`). Write any proposed new files into the patch as well, as added-file diffs. Write the proposed PR description (if generated) to `.auditor-pr-description.md`.

Do NOT post anything to GitHub. Do NOT commit. Do NOT push.
