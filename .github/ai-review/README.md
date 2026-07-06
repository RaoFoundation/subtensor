# AI Review — Operational Notes

This directory contains the persona prompts and supporting scripts for the
two-persona AI PR review driven by [`ai-review.yml`](../workflows/ai-review.yml).

## Files

| File | Purpose |
| --- | --- |
| `common.md` | Shared review context (repo topology, branch policy, output discipline) |
| `skeptic.md` | Skeptic persona: security review, static-only, no network or build |
| `auditor.md` | Auditor persona: domain review after Skeptic clears |
| `prefetch.sh` | Pre-fetches all GitHub context into `/tmp/ai-review-context/` so Codex doesn't need tokens or network |
| `gittensor-accounts.txt` | Nucleus-curated supplement to the on-chain Gittensor index |
| `known-gittensor-accounts.json` | Auto-maintained on-chain index |
| `index_gittensor.py` | Indexer that walks the SN74 `issues-v0` contract to build the index |

## Required repo secrets

| Secret | Used by | Required |
| --- | --- | --- |
| `OPENAI_API_KEY` | Codex (skeptic + auditor) | **Yes** |

## Optional — GitHub App for narrow-scope tokens

If left unconfigured, the workflow uses the default `GITHUB_TOKEN`. To narrow
the blast radius of any token leak, configure a dedicated GitHub App and the
workflow will automatically use its token instead.

### Setup

1. Create a GitHub App under the `opentensor` org:
   - Settings → Developer settings → GitHub Apps → New GitHub App.
   - Webhook: not needed; disable.
   - Repository permissions:
     - **Pull requests**: Read & Write (for comments + body edits)
     - **Contents**: Read & Write (for auditor auto-fix commits)
     - **Issues**: Read & Write (for sticky issue comments)
     - **Metadata**: Read
   - User permissions: none.
   - "Where can this GitHub App be installed?": Only on this account.
2. Install the App on the `opentensor/subtensor` repo (only).
3. From the App settings page, generate a private key (`.pem` file).
4. In repo Settings → Secrets and variables → Actions:
   - Variables tab: add `AI_REVIEW_APP_ID` = the App's numeric ID.
   - Secrets tab: add `AI_REVIEW_APP_PRIVATE_KEY` = the contents of the `.pem`.

The workflow detects `AI_REVIEW_APP_ID` and mints a short-lived installation
token via [`actions/create-github-app-token`](https://github.com/actions/create-github-app-token).
Token lifetime is 1 hour; permissions are exactly what the App grants and
nothing more. The token is masked in logs and is never passed to Codex.

## Security model

| Trust boundary | Mechanism |
| --- | --- |
| Persona prompts can be modified by PR | Loaded from base branch via `git show origin/$BASE:...` into `/tmp/ai-review-trusted/`; PR-side copies are never loaded |
| Codex prompt-injection might call `gh` | Codex runs with no `GH_TOKEN` and no `OPENAI_API_KEY` in env; prefetch step holds tokens, post-comment step holds tokens; Codex only reads files |
| Malicious `build.rs` in PR-added dep | Same env-stripping above means cargo subprocesses inherit no tokens; `drop-sudo` strategy removes sudo from the runner |
| Upstream Gittensor compromise | Indexer workflow installs gittensor pinned to commit SHA, runs in a job with `contents: read` only; a separate job with `contents: write` publishes the resulting JSON via PR — never executing third-party code |
| `OPENAI_API_KEY` leakage from Codex | Held only in the proxy's process memory (codex-action handles this), shielded by `drop-sudo` |

## Updating pinned action versions

Every third-party action used in the AI-review workflows is pinned to an
immutable commit SHA (with the major-version tag in a trailing comment), e.g.
`openai/codex-action@e0fdf01220eb9a88167c4898839d273e3f2609d1 # v1`. Mutable
tags like `@v1` would let an upstream maintainer (or compromised account)
silently swap in attacker-controlled code that runs with our OpenAI key and
GitHub App credentials.

To update a pinned action:

```bash
# Look up the current SHA for the desired ref
gh api repos/<owner>/<repo>/git/refs/tags/<ref> --jq '.object.sha'
```

Open a PR that updates the SHA and the trailing version comment. The skeptic
will re-evaluate the change.

## Fork PR handling

Repository secrets (`OPENAI_API_KEY`, `AI_REVIEW_APP_PRIVATE_KEY`) are not
exposed to `pull_request` events from forks, and the default token is read-
only, so the Codex steps cannot run on a fork auto-trigger.

The persona jobs do still run on fork PRs — they fail-fast in the very first
"Fork PR advisory" step with a clear error message directing maintainers to
invoke the workflow manually. This is intentional: a skipped required check
is treated by GitHub Branch Protection as satisfied, which would silently
bypass the security gate for exactly the contributor class that needs it most
(fork PRs from untrusted authors). Failing the check instead keeps the gate
red until a maintainer explicitly clears it.

**To AI-review a fork PR:** a nucleus member dispatches the workflow with
the PR number. `workflow_dispatch` runs in base context with secrets
available, performs the real review, and the required checks turn green.

```bash
gh workflow run ai-review.yml --repo opentensor/subtensor -f pr_number=<N>
```

## Required-checks setup

After the first successful run, add these to branch protection on `devnet-ready`
(and other protected branches) under Settings → Branches → Branch protection rules:

- `ai-review / skeptic`
- `ai-review / auditor`

## Index refresh

Manual trigger:

```bash
gh workflow run ai-review-index-gittensor.yml --repo opentensor/subtensor
```

Daily cron is already configured (06:17 UTC). The indexer opens a PR with any
new entries; nucleus reviews and merges.
