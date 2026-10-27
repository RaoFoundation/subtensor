#!/usr/bin/env python3
"""GitHub-side control plane for trusted docs-preview deployments."""

from __future__ import annotations

import argparse
import json
import os
import re
import sys
import urllib.error
import urllib.parse
import urllib.request
from dataclasses import dataclass
from pathlib import Path
from typing import Iterable, Optional

from docs_preview_artifact import ArtifactError, extract_artifact, read_intent


API_BASE = "https://api.github.com"
MAX_JSON_BYTES = 16 * 1024 * 1024
MAX_ARTIFACT_BYTES = 2_200_000_000
SHA_PATTERN = re.compile(r"^[0-9a-f]{40}$")
REPOSITORY_PATTERN = re.compile(r"^[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+$")
DEPLOYMENT_ID_PATTERN = re.compile(r"^dpl_[A-Za-z0-9]+$")
PREVIEW_URL_PATTERN = re.compile(
    r"^https://pr-[0-9]+\.preview\.[a-z0-9](?:[a-z0-9.-]*[a-z0-9])?$"
)


class ApiError(RuntimeError):
    """A GitHub API response violated the trusted workflow contract."""


@dataclass(frozen=True)
class PullState:
    number: str
    state: str
    head_sha: str
    head_repository: str


class _SafeRedirectHandler(urllib.request.HTTPRedirectHandler):
    """Never forward GitHub credentials to artifact storage hosts."""

    def redirect_request(self, request, fp, code, message, headers, new_url):
        redirected = super().redirect_request(
            request, fp, code, message, headers, new_url
        )
        if redirected is None:
            return None
        parsed = urllib.parse.urlsplit(new_url)
        if parsed.scheme != "https":
            raise ApiError("GitHub API attempted a non-HTTPS redirect")
        if parsed.hostname != urllib.parse.urlsplit(request.full_url).hostname:
            redirected.remove_header("Authorization")
            redirected.remove_header("X-GitHub-Api-Version")
        return redirected


class GitHubClient:
    def __init__(self, token: str, repository: str, timeout: int = 30):
        if not token:
            raise ValueError("GitHub token is required")
        if not REPOSITORY_PATTERN.fullmatch(repository):
            raise ValueError("repository must have owner/name form")
        self._token = token
        self.repository = repository
        self.timeout = timeout
        self._opener = urllib.request.build_opener(_SafeRedirectHandler())

    def _url(self, path: str, query: Optional[dict[str, object]] = None) -> str:
        if path.startswith("/") or ".." in path.split("/"):
            raise ValueError("unsafe GitHub API path")
        owner, name = self.repository.split("/", 1)
        url = "{}/repos/{}/{}/{}".format(
            API_BASE,
            urllib.parse.quote(owner, safe=""),
            urllib.parse.quote(name, safe=""),
            path,
        )
        if query:
            url += "?" + urllib.parse.urlencode(query)
        return url

    def _open(
        self,
        method: str,
        path: str,
        query: Optional[dict[str, object]] = None,
        body: Optional[dict[str, object]] = None,
    ):
        encoded_body = None
        headers = {
            "Accept": "application/vnd.github+json",
            "Authorization": f"Bearer {self._token}",
            "X-GitHub-Api-Version": "2022-11-28",
        }
        if body is not None:
            encoded_body = json.dumps(body).encode("utf-8")
            headers["Content-Type"] = "application/json"
        request = urllib.request.Request(
            self._url(path, query),
            data=encoded_body,
            method=method,
            headers=headers,
        )
        try:
            return self._opener.open(request, timeout=self.timeout)
        except urllib.error.HTTPError as error:
            raise ApiError(
                f"GitHub API {method} {path} returned HTTP {error.code}"
            ) from error
        except urllib.error.URLError as error:
            raise ApiError(
                f"GitHub API {method} {path} failed: {error.reason}"
            ) from error

    def request_json(
        self,
        method: str,
        path: str,
        query: Optional[dict[str, object]] = None,
        body: Optional[dict[str, object]] = None,
    ) -> object:
        with self._open(method, path, query, body) as response:
            raw = response.read(MAX_JSON_BYTES + 1)
        if len(raw) > MAX_JSON_BYTES:
            raise ApiError(f"GitHub API {path} response is too large")
        try:
            return json.loads(raw)
        except (UnicodeDecodeError, json.JSONDecodeError) as error:
            raise ApiError(f"GitHub API {path} returned invalid JSON") from error

    def download(self, path: str, destination: Path, maximum_bytes: int) -> None:
        destination.parent.mkdir(parents=True, exist_ok=True)
        if destination.exists():
            raise ApiError(f"download destination already exists: {destination}")
        copied = 0
        try:
            with self._open("GET", path) as response, destination.open("xb") as output:
                while True:
                    chunk = response.read(1024 * 1024)
                    if not chunk:
                        break
                    copied += len(chunk)
                    if copied > maximum_bytes:
                        raise ApiError(
                            "GitHub artifact download exceeded its size limit"
                        )
                    output.write(chunk)
        except Exception:
            destination.unlink(missing_ok=True)
            raise


def _pull_from_response(number: str, response: object) -> PullState:
    if not isinstance(response, dict):
        raise ApiError("GitHub pull request response is not an object")
    head = response.get("head")
    repository = head.get("repo") if isinstance(head, dict) else None
    state = response.get("state")
    head_sha = head.get("sha") if isinstance(head, dict) else None
    head_repository = (
        repository.get("full_name") if isinstance(repository, dict) else None
    )
    if (
        state not in {"open", "closed"}
        or not isinstance(head_sha, str)
        or not SHA_PATTERN.fullmatch(head_sha)
        or not isinstance(head_repository, str)
    ):
        raise ApiError("GitHub pull request response has an invalid state or head")
    return PullState(number, state, head_sha, head_repository)


def pull_state(client: GitHubClient, number: str) -> PullState:
    if not number.isdigit():
        raise ValueError("pull request number must be digits")
    return _pull_from_response(
        number,
        client.request_json("GET", f"pulls/{number}"),
    )


def workflow_pr_number(workflow_pulls_json: str) -> Optional[str]:
    """Resolve an unambiguous PR directly from the workflow_run payload."""

    try:
        workflow_pulls = json.loads(workflow_pulls_json or "[]")
    except json.JSONDecodeError as error:
        raise ApiError("workflow_run pull request list is invalid JSON") from error
    if not isinstance(workflow_pulls, list):
        raise ApiError("workflow_run pull request list is not an array")
    numbers = {
        pull.get("number")
        for pull in workflow_pulls
        if isinstance(pull, dict)
        and isinstance(pull.get("number"), int)
        and pull["number"] > 0
    }
    if len(numbers) > 1:
        raise ApiError("workflow_run is associated with multiple pull requests")
    if numbers:
        return str(numbers.pop())
    return None


def associated_pr_number(
    associated_pulls: object,
    head_sha: str,
    repository: str,
) -> str:
    """Resolve the one same-repository PR associated with a commit."""

    if not isinstance(associated_pulls, list):
        raise ApiError("commit association response is not an array")
    matches = []
    for pull in associated_pulls:
        if not isinstance(pull, dict):
            continue
        head = pull.get("head")
        head_repo = head.get("repo") if isinstance(head, dict) else None
        number = pull.get("number")
        if (
            isinstance(head, dict)
            and head.get("sha") == head_sha
            and isinstance(head_repo, dict)
            and head_repo.get("full_name") == repository
            and isinstance(number, int)
            and number > 0
        ):
            matches.append(number)
    if len(set(matches)) != 1:
        raise ApiError(
            f"expected one same-repository pull request for {head_sha}, "
            f"found {len(set(matches))}"
        )
    return str(matches[0])


def reconcile_mode(
    action: str,
    pull: PullState,
    expected_head_sha: str,
    repository: str,
) -> str:
    if action not in {"deploy", "cleanup"}:
        raise ValueError("invalid docs-preview action")
    if pull.state == "closed":
        return "cleanup"
    if action == "cleanup":
        return "noop"
    if pull.head_sha == expected_head_sha and pull.head_repository == repository:
        return "deploy"
    return "noop"


def _write_output(name: str, value: str) -> None:
    if not re.fullmatch(r"[A-Za-z0-9_.:/-]+", value):
        raise ApiError(f"unsafe GitHub Actions output value for {name}")
    output_path = os.environ.get("GITHUB_OUTPUT")
    if not output_path:
        raise ApiError("GITHUB_OUTPUT is unavailable")
    with open(output_path, "a", encoding="utf-8") as output:
        output.write(f"{name}={value}\n")


def _summary(message: str) -> None:
    path = os.environ.get("GITHUB_STEP_SUMMARY")
    if path:
        with open(path, "a", encoding="utf-8") as summary:
            summary.write(message + "\n")


def prepare(
    client: GitHubClient,
    workflow_pulls_json: str,
    workflow_head_sha: str,
    workflow_run_id: str,
    docs_domain: str,
    artifact_zip: Path,
    artifact_directory: Path,
) -> None:
    if not SHA_PATTERN.fullmatch(workflow_head_sha):
        raise ValueError("workflow head SHA is invalid")
    if not workflow_run_id.isdigit():
        raise ValueError("workflow run ID must be digits")

    pr_number = workflow_pr_number(workflow_pulls_json)
    if pr_number is None:
        associated = client.request_json(
            "GET",
            f"commits/{workflow_head_sha}/pulls",
        )
        pr_number = associated_pr_number(
            associated,
            workflow_head_sha,
            client.repository,
        )
    artifact_name = f"docs-preview-{pr_number}"
    response = client.request_json(
        "GET",
        f"actions/runs/{workflow_run_id}/artifacts",
        {"name": artifact_name, "per_page": 100},
    )
    if not isinstance(response, dict) or response.get("total_count") != 1:
        count = response.get("total_count") if isinstance(response, dict) else "invalid"
        raise ApiError(f"expected one {artifact_name} artifact, found {count}")
    artifacts = response.get("artifacts")
    artifact = (
        artifacts[0] if isinstance(artifacts, list) and len(artifacts) == 1 else None
    )
    if not isinstance(artifact, dict):
        raise ApiError("GitHub artifact response is invalid")
    artifact_id = artifact.get("id")
    artifact_size = artifact.get("size_in_bytes")
    if (
        not isinstance(artifact_id, int)
        or artifact_id <= 0
        or not isinstance(artifact_size, int)
        or artifact_size < 0
        or artifact_size > MAX_ARTIFACT_BYTES
        or artifact.get("expired") is not False
    ):
        raise ApiError("GitHub artifact is expired or violates its size contract")

    client.download(
        f"actions/artifacts/{artifact_id}/zip",
        artifact_zip,
        MAX_ARTIFACT_BYTES,
    )
    extract_artifact(artifact_zip, artifact_directory)
    intent = read_intent(artifact_directory, pr_number)
    pull = pull_state(client, pr_number)
    mode = reconcile_mode(intent.action, pull, workflow_head_sha, client.repository)
    alias = f"pr-{pr_number}.preview.{docs_domain}"
    _write_output("pr", pr_number)
    _write_output("action", intent.action)
    _write_output("mode", mode)
    _write_output("alias", alias)
    _write_output("url", f"https://{alias}")
    _summary(
        f"Resolved artifact action {intent.action} and PR state "
        f"{pull.state} to {mode}."
    )


def recheck(
    client: GitHubClient,
    pr_number: str,
    expected_head_sha: str,
) -> None:
    pull = pull_state(client, pr_number)
    authorized = (
        pull.state == "open"
        and pull.head_sha == expected_head_sha
        and pull.head_repository == client.repository
    )
    _write_output("authorized", str(authorized).lower())
    if not authorized:
        _summary(
            "Deployment became stale while queued; a newer run will reconcile "
            "the preview."
        )


def upsert_preview_comment(
    client: GitHubClient,
    pr_number: str,
    deployment_id: str,
    preview_url: str,
) -> None:
    if not pr_number.isdigit():
        raise ValueError("pull request number must be digits")
    if not DEPLOYMENT_ID_PATTERN.fullmatch(deployment_id):
        raise ValueError("invalid Vercel deployment ID")
    if ".." in preview_url or not PREVIEW_URL_PATTERN.fullmatch(preview_url):
        raise ValueError("invalid docs-preview URL")

    marker = "<!-- docs-preview-url -->"
    body = "\n".join(
        [
            marker,
            f"<!-- docs-preview-deployment:{deployment_id} -->",
            "### Docs preview",
            "",
            f"{preview_url}/docs",
            "",
            "Stable for this PR; it updates after each docs or website change.",
        ]
    )
    existing_id = None
    for page in range(1, 101):
        response = client.request_json(
            "GET",
            f"issues/{pr_number}/comments",
            {"per_page": 100, "page": page},
        )
        if not isinstance(response, list):
            raise ApiError("GitHub issue comments response is not an array")
        for comment in response:
            if (
                isinstance(comment, dict)
                and isinstance(comment.get("body"), str)
                and marker in comment["body"]
                and isinstance(comment.get("id"), int)
            ):
                existing_id = comment["id"]
                break
        if existing_id is not None or len(response) < 100:
            break
    else:
        raise ApiError("GitHub issue comment scan exceeded 10,000 comments")
    if existing_id is None:
        client.request_json(
            "POST",
            f"issues/{pr_number}/comments",
            body={"body": body},
        )
    else:
        client.request_json(
            "PATCH",
            f"issues/comments/{existing_id}",
            body={"body": body},
        )


def _parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--repository", required=True)
    commands = parser.add_subparsers(dest="command", required=True)

    prepare_command = commands.add_parser("prepare")
    prepare_command.add_argument("--workflow-pulls-json", required=True)
    prepare_command.add_argument("--workflow-head-sha", required=True)
    prepare_command.add_argument("--workflow-run-id", required=True)
    prepare_command.add_argument("--docs-domain", required=True)
    prepare_command.add_argument("--artifact-zip", type=Path, required=True)
    prepare_command.add_argument("--artifact-directory", type=Path, required=True)

    recheck_command = commands.add_parser("recheck")
    recheck_command.add_argument("--pr-number", required=True)
    recheck_command.add_argument("--workflow-head-sha", required=True)

    comment = commands.add_parser("comment")
    comment.add_argument("--pr-number", required=True)
    comment.add_argument("--deployment-id", required=True)
    comment.add_argument("--preview-url", required=True)
    return parser


def main(arguments: Optional[Iterable[str]] = None) -> int:
    args = _parser().parse_args(arguments)
    token = os.environ.get("GH_TOKEN", "")
    try:
        client = GitHubClient(token, args.repository)
        if args.command == "prepare":
            prepare(
                client,
                args.workflow_pulls_json,
                args.workflow_head_sha,
                args.workflow_run_id,
                args.docs_domain,
                args.artifact_zip,
                args.artifact_directory,
            )
        elif args.command == "recheck":
            recheck(client, args.pr_number, args.workflow_head_sha)
        elif args.command == "comment":
            upsert_preview_comment(
                client,
                args.pr_number,
                args.deployment_id,
                args.preview_url,
            )
        else:
            raise AssertionError(args.command)
    except (ApiError, ArtifactError, OSError, ValueError) as error:
        print(f"docs-preview GitHub control failed: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
