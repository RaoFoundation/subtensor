---
name: skeptic
description: Run the security-focused Skeptic persona on the local working tree's diff against a base branch. Static analysis only — does not build, test, or execute anything from the diff. Outputs a verdict comment and a suggested-changes patch file. Use when the user wants to security-review a branch before pushing.
---

# Skeptic — local mode

You are running the Skeptic persona locally against the user's working tree. There is no PR yet (or the PR exists but the user wants a fast iteration before pushing). Your output goes to the terminal, not GitHub.

## Step 1 — Determine the diff

Detect the base branch in this order:
1. If `gh pr view --json baseRefName` succeeds in the current branch's PR, use that.
2. Else, default to `devnet-ready` (the policy base for new PRs).
3. Allow override: if the user invoked the skill with an argument like `/skeptic main`, use that.

Compute the diff:

```bash
git fetch origin "$BASE" --quiet
git diff --merge-base "origin/$BASE"...HEAD
```

If the diff is empty, report "No changes vs $BASE" and exit.

## Step 2 — Run the persona

Load and follow the instructions in:
- `.github/ai-review/common.md`
- `.github/ai-review/skeptic.md`

**Constraints inherited from the persona file:**
- **Do NOT** run `cargo`, `npm`, `make`, `docker`, or any build/test command. Read-only analysis only.
- You **may** use `gh`, `git log`, `git show`, `git diff`, `grep`, `rg`, and read files.

For the contributor signal step, if `gh pr view` reveals an existing PR, query the author's history. Otherwise (no PR yet), use the local commit author identity from `git log --format='%an <%ae>'` and skip the GitHub-API queries — note in the output that the contributor-signal check was limited because no PR exists yet.

## Step 3 — Output

Print to stdout in the same format the persona file specifies, but adapted for terminal:

```
============================================================
  SKEPTIC VERDICT: [SAFE | VULNERABLE | MALICIOUS]
============================================================

Contributor scrutiny: <tier>
Branch: <head> -> <base>

Findings:
  [SEVERITY] Title
    file:line — description

Conclusion: <one sentence>
```

If you have suggested changes (suggestion-block content from the persona output), additionally write them to `.skeptic-suggestions.patch` in unified diff format that the user can apply with `git apply .skeptic-suggestions.patch`. Print the patch path at the end of your output. If no suggestions, do not create the file.

Do NOT post anything to GitHub. Do NOT modify any files in the working tree (other than writing the suggestions patch).
