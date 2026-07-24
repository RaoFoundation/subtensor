#!/usr/bin/env python3
"""Fail-closed Vercel controls for trusted docs-preview deployments."""

from __future__ import annotations

import argparse
import json
import os
import sys
import urllib.error
import urllib.parse
import urllib.request
from dataclasses import dataclass
from typing import Dict, Iterable, List, Optional, Sequence


API_BASE = "https://api.vercel.com"


class ApiError(RuntimeError):
    """A Vercel API request failed."""


@dataclass(frozen=True)
class UnsafeEnvironmentVariable:
    source: str
    key: str


class VercelClient:
    def __init__(self, token: str, team_id: str, timeout: int = 30):
        if not token or not team_id:
            raise ValueError("Vercel token and team ID are required")
        self._token = token
        self.team_id = team_id
        self.timeout = timeout

    def request(
        self,
        method: str,
        path: str,
        query: Optional[Dict[str, object]] = None,
    ) -> object:
        parameters = {"teamId": self.team_id}
        if query:
            parameters.update(query)
        url = API_BASE + path + "?" + urllib.parse.urlencode(parameters)
        request = urllib.request.Request(
            url,
            method=method,
            headers={
                "Authorization": f"Bearer {self._token}",
                "Accept": "application/json",
            },
        )
        try:
            with urllib.request.urlopen(request, timeout=self.timeout) as response:
                body = response.read()
        except urllib.error.HTTPError as error:
            raise ApiError(
                f"Vercel API {method} {path} returned HTTP {error.code}"
            ) from error
        except urllib.error.URLError as error:
            raise ApiError(f"Vercel API {method} {path} failed: {error.reason}") from error
        if not body:
            return {}
        try:
            return json.loads(body)
        except (UnicodeDecodeError, json.JSONDecodeError) as error:
            raise ApiError(f"Vercel API {method} {path} returned invalid JSON") from error

    def paginated(
        self,
        path: str,
        item_key: str,
        query: Optional[Dict[str, object]] = None,
    ) -> List[object]:
        parameters = dict(query or {})
        parameters.setdefault("limit", 100)
        items: List[object] = []
        seen_cursors = set()
        for _ in range(1_000):
            response = self.request("GET", path, parameters)
            if not isinstance(response, dict) or not isinstance(
                response.get(item_key), list
            ):
                raise ApiError(f"Vercel API {path} returned an unexpected response")
            items.extend(response[item_key])
            pagination = response.get("pagination") or {}
            cursor = pagination.get("next") if isinstance(pagination, dict) else None
            if cursor is None:
                return items
            cursor = str(cursor)
            if cursor in seen_cursors:
                raise ApiError(f"Vercel API {path} repeated a pagination cursor")
            seen_cursors.add(cursor)
            parameters["until"] = cursor
        raise ApiError(f"Vercel API {path} exceeded the pagination limit")


def _targets_preview(target: object) -> bool:
    if isinstance(target, str):
        return target.lower() == "preview"
    if isinstance(target, list):
        return any(
            isinstance(value, str) and value.lower() == "preview" for value in target
        )
    # Unknown target shapes cannot prove that the variable is absent from Preview.
    return True


def unsafe_preview_variables(
    project_variables: Sequence[object],
    shared_variables: Sequence[object],
) -> List[UnsafeEnvironmentVariable]:
    unsafe: List[UnsafeEnvironmentVariable] = []
    for source, variables in (
        ("project", project_variables),
        ("team-shared", shared_variables),
    ):
        for variable in variables:
            if not isinstance(variable, dict):
                unsafe.append(UnsafeEnvironmentVariable(source, "<invalid-response>"))
                continue
            if variable.get("system") is True:
                continue
            if _targets_preview(variable.get("target")):
                key = variable.get("key")
                unsafe.append(
                    UnsafeEnvironmentVariable(
                        source,
                        key if isinstance(key, str) and key else "<unnamed>",
                    )
                )
    return sorted(unsafe, key=lambda item: (item.source, item.key))


def deployment_ids_for_pr(
    deployments: Sequence[object],
    pr_number: str,
    keep_deployment_id: Optional[str] = None,
) -> List[str]:
    matches = []
    for deployment in deployments:
        if not isinstance(deployment, dict):
            raise ApiError("Vercel deployments response contains an invalid entry")
        metadata = deployment.get("meta")
        deployment_id = deployment.get("uid") or deployment.get("id")
        if (
            isinstance(metadata, dict)
            and str(metadata.get("docsPreviewPr", "")) == pr_number
            and isinstance(deployment_id, str)
            and deployment_id
            and deployment_id != keep_deployment_id
        ):
            matches.append(deployment_id)
    return sorted(set(matches))


def audit_environment(client: VercelClient, project_id: str) -> None:
    project_path = "/v10/projects/{}/env".format(
        urllib.parse.quote(project_id, safe="")
    )
    project_variables = client.paginated(project_path, "envs", {"decrypt": "false"})
    shared_variables = client.paginated(
        "/v1/env",
        "data",
        {"projectId": project_id},
    )
    unsafe = unsafe_preview_variables(project_variables, shared_variables)
    if unsafe:
        descriptions = ", ".join(f"{item.source}:{item.key}" for item in unsafe)
        raise ApiError(
            "docs-preview project exposes non-system Preview environment "
            f"variables ({descriptions}); refusing to deploy PR-controlled functions"
        )


def delete_pr_deployments(
    client: VercelClient,
    project_id: str,
    pr_number: str,
    keep_deployment_id: Optional[str] = None,
) -> int:
    deployments = client.paginated(
        "/v7/deployments",
        "deployments",
        {"projectId": project_id},
    )
    deployment_ids = deployment_ids_for_pr(
        deployments, pr_number, keep_deployment_id=keep_deployment_id
    )
    for deployment_id in deployment_ids:
        client.request(
            "DELETE",
            "/v13/deployments/{}".format(
                urllib.parse.quote(deployment_id, safe="")
            ),
        )
    return len(deployment_ids)


def _parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--project-id", required=True)
    parser.add_argument("--team-id", required=True)
    subparsers = parser.add_subparsers(dest="command", required=True)
    subparsers.add_parser("audit-environment")
    cleanup = subparsers.add_parser("delete-pr-deployments")
    cleanup.add_argument("--pr-number", required=True)
    cleanup.add_argument("--keep-deployment-id")
    return parser


def main(arguments: Optional[Iterable[str]] = None) -> int:
    args = _parser().parse_args(arguments)
    token = os.environ.get("VERCEL_TOKEN", "")
    if not token:
        print("VERCEL_TOKEN is required", file=sys.stderr)
        return 1
    try:
        client = VercelClient(token, args.team_id)
        if args.command == "audit-environment":
            audit_environment(client, args.project_id)
            print("Vercel docs-preview project has no non-system Preview variables")
        elif args.command == "delete-pr-deployments":
            count = delete_pr_deployments(
                client,
                args.project_id,
                args.pr_number,
                keep_deployment_id=args.keep_deployment_id,
            )
            print(f"Deleted {count} obsolete docs-preview deployment(s)")
        else:
            raise AssertionError(args.command)
    except (ApiError, ValueError) as error:
        print(f"docs-preview Vercel guard failed: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
