#!/usr/bin/env python3
"""
Index Gittensor (SN74) miners by walking completed issues in the on-chain
issues-v0 contract and asking GitHub which merged PR closed each issue.
The PR's author is then known to be a Gittensor miner who has won at least
one bounty.

Output: .github/ai-review/known-gittensor-accounts.json
  {
    "_last_indexed_iso": "2026-05-05T12:34:56Z",
    "_completed_issues_seen": 123,
    "accounts": { "<github_login>": { "bounty_count": N, "issues": [...] } }
  }

Coverage caveat: only catches miners who have won at least one bounty. PAT-only
farmers who have not won a bounty are invisible to this indexer; add them to
gittensor-accounts.txt manually.
"""

from __future__ import annotations

import json
import os
import subprocess
import sys
from datetime import datetime, timezone
from pathlib import Path
from typing import Any

import bittensor as bt
from gittensor.validator.issue_competitions.contract_client import (
    IssueCompetitionContractClient,
    IssueStatus,
)

NETWORK = os.environ.get("BITTENSOR_NETWORK", "finney")
CONTRACT_ADDRESS = os.environ.get(
    "GITTENSOR_CONTRACT_ADDRESS",
    "5FWNdk8YNtNcHKrAx2krqenFrFAZG7vmsd2XN2isJSew3MrD",
)
INDEX_PATH = Path(__file__).parent / "known-gittensor-accounts.json"


def gh_closing_pr_authors(repo: str, issue_number: int) -> list[str]:
    """Return the logins of authors of merged PRs that closed the given issue."""
    if "/" not in repo:
        return []
    owner, name = repo.split("/", 1)
    query = """
    query($owner: String!, $name: String!, $number: Int!) {
      repository(owner: $owner, name: $name) {
        issue(number: $number) {
          closedByPullRequestsReferences(first: 10, includeClosedPrs: true) {
            nodes { number, merged, author { login } }
          }
        }
      }
    }
    """
    try:
        result = subprocess.run(
            ["gh", "api", "graphql",
             "-f", f"query={query}",
             "-F", f"owner={owner}",
             "-F", f"name={name}",
             "-F", f"number={issue_number}"],
            capture_output=True, text=True, timeout=30, check=True,
        )
    except subprocess.CalledProcessError as e:
        print(f"gh query failed for {repo}#{issue_number}: {e.stderr.strip()}", file=sys.stderr)
        return []
    payload = json.loads(result.stdout)
    issue = (payload.get("data") or {}).get("repository", {}).get("issue") or {}
    refs = (issue.get("closedByPullRequestsReferences") or {}).get("nodes") or []
    authors: list[str] = []
    for ref in refs:
        if not ref.get("merged"):
            continue
        login = ((ref.get("author") or {}).get("login") or "").strip()
        if login:
            authors.append(login)
    return authors


def load_state() -> dict[str, Any]:
    if INDEX_PATH.exists():
        try:
            return json.loads(INDEX_PATH.read_text())
        except json.JSONDecodeError:
            pass
    return {"accounts": {}}


def save_state(state: dict[str, Any]) -> None:
    state["accounts"] = {k: state["accounts"][k] for k in sorted(state["accounts"])}
    INDEX_PATH.write_text(json.dumps(state, indent=2) + "\n")


def main() -> int:
    state = load_state()
    accounts: dict[str, dict[str, Any]] = state.setdefault("accounts", {})

    print(f"connecting to bittensor network={NETWORK}", file=sys.stderr)
    subtensor = bt.subtensor(network=NETWORK)
    client = IssueCompetitionContractClient(CONTRACT_ADDRESS, subtensor)

    completed = client.get_issues_by_status(IssueStatus.COMPLETED)
    print(f"found {len(completed)} completed issues on chain", file=sys.stderr)

    new_pairs = 0
    for issue in completed:
        repo = issue.repository_full_name
        issue_number = issue.issue_number
        if not repo or not issue_number:
            continue

        authors = gh_closing_pr_authors(repo, issue_number)
        if not authors:
            continue

        evidence_key = f"{repo}#{issue_number}"
        for login in authors:
            entry = accounts.setdefault(login, {"bounty_count": 0, "issues": []})
            if evidence_key not in entry["issues"]:
                entry["issues"].append(evidence_key)
                entry["bounty_count"] = len(entry["issues"])
                new_pairs += 1

    state["_last_indexed_iso"] = datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")
    state["_completed_issues_seen"] = len(completed)
    save_state(state)
    print(f"added {new_pairs} new (login, issue) pairs; total accounts={len(accounts)}",
          file=sys.stderr)
    return 0


if __name__ == "__main__":
    sys.exit(main())
