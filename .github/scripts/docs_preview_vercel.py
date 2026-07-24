#!/usr/bin/env python3
"""Fail-closed Vercel controls for trusted docs-preview deployments."""

from __future__ import annotations

import argparse
import json
import os
import re
import sys
import tempfile
import urllib.error
import urllib.parse
import urllib.request
from dataclasses import dataclass
from pathlib import Path
from typing import Iterable, Optional, Sequence


API_BASE = "https://api.vercel.com"
IDENTIFIER_PATTERN = re.compile(r"^[A-Za-z0-9_-]{1,128}$")
DOMAIN_PATTERN = re.compile(r"^[a-z0-9](?:[a-z0-9.-]*[a-z0-9])?$")
DEPLOYMENT_ID_PATTERN = re.compile(r"^dpl_[A-Za-z0-9]+$")
DEPLOYMENT_URL_PATTERN = re.compile(r"^https://[A-Za-z0-9.-]+$")


class ApiError(RuntimeError):
    """A Vercel API request failed or returned an unsafe shape."""


@dataclass(frozen=True)
class UnsafeEnvironmentVariable:
    source: str
    key: str


class VercelClient:
    def __init__(self, token: str, team_id: str, timeout: int = 30):
        if not token or not IDENTIFIER_PATTERN.fullmatch(team_id):
            raise ValueError("valid Vercel token and team ID are required")
        self._token = token
        self.team_id = team_id
        self.timeout = timeout

    def request(
        self,
        method: str,
        path: str,
        query: Optional[dict[str, object]] = None,
        body: Optional[dict[str, object]] = None,
        allow_missing: bool = False,
    ) -> object:
        parameters = {"teamId": self.team_id}
        if query:
            parameters.update(query)
        url = API_BASE + path + "?" + urllib.parse.urlencode(parameters)
        encoded_body = json.dumps(body).encode("utf-8") if body is not None else None
        headers = {
            "Authorization": f"Bearer {self._token}",
            "Accept": "application/json",
        }
        if encoded_body is not None:
            headers["Content-Type"] = "application/json"
        request = urllib.request.Request(
            url,
            data=encoded_body,
            method=method,
            headers=headers,
        )
        try:
            with urllib.request.urlopen(request, timeout=self.timeout) as response:
                raw = response.read(16 * 1024 * 1024 + 1)
        except urllib.error.HTTPError as error:
            if allow_missing and error.code == 404:
                return None
            raise ApiError(
                f"Vercel API {method} {path} returned HTTP {error.code}"
            ) from error
        except urllib.error.URLError as error:
            raise ApiError(
                f"Vercel API {method} {path} failed: {error.reason}"
            ) from error
        if len(raw) > 16 * 1024 * 1024:
            raise ApiError(f"Vercel API {path} response is too large")
        if not raw:
            return {}
        try:
            return json.loads(raw)
        except (UnicodeDecodeError, json.JSONDecodeError) as error:
            raise ApiError(
                f"Vercel API {method} {path} returned invalid JSON"
            ) from error

    def paginated(
        self,
        path: str,
        item_key: str,
        query: Optional[dict[str, object]] = None,
    ) -> list[object]:
        parameters = dict(query or {})
        parameters.setdefault("limit", 100)
        items: list[object] = []
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


def validate_configuration(
    team_id: str,
    project_id: str,
    production_project_id: str,
    docs_domain: str,
) -> None:
    if not all(
        IDENTIFIER_PATTERN.fullmatch(value)
        for value in (team_id, project_id, production_project_id)
    ):
        raise ValueError(
            "set valid VERCEL_ORG_ID, VERCEL_DOCS_PROJECT_ID, and "
            "VERCEL_DOCS_PREVIEW_PROJECT_ID repository variables"
        )
    if project_id == production_project_id:
        raise ValueError(
            "VERCEL_DOCS_PREVIEW_PROJECT_ID must identify a dedicated, "
            "non-production project"
        )
    if (
        len(docs_domain) > 253
        or ".." in docs_domain
        or not DOMAIN_PATTERN.fullmatch(docs_domain)
    ):
        raise ValueError("DOCS_DOMAIN must be a valid lowercase DNS name")


def write_project_link(
    directory: Path,
    team_id: str,
    project_id: str,
    include_build_settings: bool,
) -> None:
    payload: dict[str, object] = {"projectId": project_id, "orgId": team_id}
    if include_build_settings:
        payload["settings"] = {
            "framework": "nextjs",
            "devCommand": None,
            "installCommand": None,
            "buildCommand": None,
            "outputDirectory": None,
            "rootDirectory": "website/apps/bittensor-website",
            "directoryListing": False,
            "nodeVersion": "24.x",
        }
    destination = directory.resolve() / ".vercel" / "project.json"
    destination.parent.mkdir(parents=True, exist_ok=True)
    fd, temporary_name = tempfile.mkstemp(
        prefix=".project.",
        suffix=".json",
        dir=destination.parent,
    )
    try:
        with os.fdopen(fd, "w", encoding="utf-8") as temporary:
            json.dump(payload, temporary, sort_keys=True, separators=(",", ":"))
        os.replace(temporary_name, destination)
    except Exception:
        try:
            os.close(fd)
        except OSError:
            pass
        Path(temporary_name).unlink(missing_ok=True)
        raise


def _targets_preview(target: object) -> bool:
    if isinstance(target, str):
        return target.lower() == "preview"
    if isinstance(target, list):
        return any(
            isinstance(value, str) and value.lower() == "preview" for value in target
        )
    return True


def unsafe_preview_variables(
    project_variables: Sequence[object],
    shared_variables: Sequence[object],
) -> list[UnsafeEnvironmentVariable]:
    unsafe: list[UnsafeEnvironmentVariable] = []
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


def audit_project(
    client: VercelClient,
    project_id: str,
    preview_domain: str,
) -> None:
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

    domain = client.request(
        "GET",
        "/v9/projects/{}/domains/{}".format(
            urllib.parse.quote(project_id, safe=""),
            urllib.parse.quote(preview_domain, safe=""),
        ),
    )
    if (
        not isinstance(domain, dict)
        or domain.get("name") != preview_domain
        or domain.get("verified") is not True
    ):
        raise ApiError(
            f"preview project must pre-provision and verify {preview_domain}"
        )


def deployment_id_for_url(
    client: VercelClient,
    project_id: str,
    deployment_url: str,
) -> str:
    if not DEPLOYMENT_URL_PATTERN.fullmatch(deployment_url):
        raise ValueError("Vercel CLI returned an invalid deployment URL")
    host = deployment_url.removeprefix("https://")
    response = client.request(
        "GET",
        f"/v13/deployments/{urllib.parse.quote(host, safe='')}",
    )
    deployment_id = response.get("id") if isinstance(response, dict) else None
    response_project_id = (
        response.get("projectId") if isinstance(response, dict) else None
    )
    if (
        not isinstance(deployment_id, str)
        or not DEPLOYMENT_ID_PATTERN.fullmatch(deployment_id)
        or response_project_id != project_id
    ):
        raise ApiError("Vercel deployment response has an invalid ID")
    return deployment_id


def set_alias(client: VercelClient, deployment_id: str, alias: str) -> None:
    if not DEPLOYMENT_ID_PATTERN.fullmatch(deployment_id):
        raise ValueError("invalid Vercel deployment ID")
    if not DOMAIN_PATTERN.fullmatch(alias):
        raise ValueError("invalid docs-preview alias")
    client.request(
        "POST",
        f"/v2/deployments/{urllib.parse.quote(deployment_id, safe='')}/aliases",
        body={"alias": alias},
    )


def remove_alias(
    client: VercelClient,
    project_id: str,
    alias: str,
) -> bool:
    if not DOMAIN_PATTERN.fullmatch(alias):
        raise ValueError("invalid docs-preview alias")
    response = client.request(
        "GET",
        f"/now/aliases/{urllib.parse.quote(alias, safe='')}",
        allow_missing=True,
    )
    if response is None:
        return False
    alias_id = response.get("uid") if isinstance(response, dict) else None
    alias_project_id = response.get("projectId") if isinstance(response, dict) else None
    if not isinstance(alias_id, str) or not alias_id or alias_project_id != project_id:
        raise ApiError("refusing to remove an alias outside the preview project")
    client.request(
        "DELETE",
        f"/now/aliases/{urllib.parse.quote(alias_id, safe='')}",
    )
    return True


def deployment_ids_for_pr(
    deployments: Sequence[object],
    pr_number: str,
    keep_deployment_id: Optional[str] = None,
) -> list[str]:
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


def delete_deployment(client: VercelClient, deployment_id: str) -> None:
    if not DEPLOYMENT_ID_PATTERN.fullmatch(deployment_id):
        raise ValueError("invalid Vercel deployment ID")
    client.request(
        "DELETE",
        f"/v13/deployments/{urllib.parse.quote(deployment_id, safe='')}",
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
        deployments,
        pr_number,
        keep_deployment_id=keep_deployment_id,
    )
    for deployment_id in deployment_ids:
        delete_deployment(client, deployment_id)
    return len(deployment_ids)


def _write_output(name: str, value: str) -> None:
    output_path = os.environ.get("GITHUB_OUTPUT")
    if not output_path:
        raise ApiError("GITHUB_OUTPUT is unavailable")
    if not re.fullmatch(r"[A-Za-z0-9_.:/-]+", value):
        raise ApiError(f"unsafe GitHub Actions output value for {name}")
    with open(output_path, "a", encoding="utf-8") as output:
        output.write(f"{name}={value}\n")


def _parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--project-id", required=True)
    parser.add_argument("--team-id", required=True)
    commands = parser.add_subparsers(dest="command", required=True)

    validate = commands.add_parser("validate-config")
    validate.add_argument("--production-project-id", required=True)
    validate.add_argument("--docs-domain", required=True)

    link = commands.add_parser("link-project")
    link.add_argument("--directory", type=Path, required=True)
    link.add_argument("--include-build-settings", action="store_true")

    audit = commands.add_parser("audit-project")
    audit.add_argument("--preview-domain", required=True)

    deployment = commands.add_parser("deployment-id")
    deployment.add_argument("--deployment-url", required=True)

    alias = commands.add_parser("set-alias")
    alias.add_argument("--deployment-id", required=True)
    alias.add_argument("--alias", required=True)

    remove = commands.add_parser("remove-alias")
    remove.add_argument("--alias", required=True)

    cleanup = commands.add_parser("delete-pr-deployments")
    cleanup.add_argument("--pr-number", required=True)
    cleanup.add_argument("--keep-deployment-id")

    delete = commands.add_parser("delete-deployment")
    delete.add_argument("--deployment-id", required=True)
    return parser


def main(arguments: Optional[Iterable[str]] = None) -> int:
    args = _parser().parse_args(arguments)
    token_commands = {
        "audit-project",
        "deployment-id",
        "set-alias",
        "remove-alias",
        "delete-pr-deployments",
        "delete-deployment",
    }
    try:
        if args.command == "validate-config":
            validate_configuration(
                args.team_id,
                args.project_id,
                args.production_project_id,
                args.docs_domain,
            )
            return 0
        if args.command == "link-project":
            write_project_link(
                args.directory,
                args.team_id,
                args.project_id,
                args.include_build_settings,
            )
            return 0

        token = os.environ.get("VERCEL_TOKEN", "")
        if args.command in token_commands and not token:
            raise ValueError("VERCEL_TOKEN is required")
        client = VercelClient(token, args.team_id)
        if args.command == "audit-project":
            audit_project(client, args.project_id, args.preview_domain)
            print("Vercel preview project and domain satisfy the trusted boundary")
        elif args.command == "deployment-id":
            deployment_id = deployment_id_for_url(
                client,
                args.project_id,
                args.deployment_url,
            )
            _write_output("deployment_id", deployment_id)
            _write_output("deployment_url", args.deployment_url)
        elif args.command == "set-alias":
            set_alias(client, args.deployment_id, args.alias)
        elif args.command == "remove-alias":
            removed = remove_alias(client, args.project_id, args.alias)
            print(
                "Removed preview alias"
                if removed
                else "Preview alias was already absent"
            )
        elif args.command == "delete-pr-deployments":
            count = delete_pr_deployments(
                client,
                args.project_id,
                args.pr_number,
                keep_deployment_id=args.keep_deployment_id,
            )
            print(f"Deleted {count} obsolete docs-preview deployment(s)")
        elif args.command == "delete-deployment":
            delete_deployment(client, args.deployment_id)
        else:
            raise AssertionError(args.command)
    except (ApiError, OSError, ValueError) as error:
        print(f"docs-preview Vercel control failed: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
