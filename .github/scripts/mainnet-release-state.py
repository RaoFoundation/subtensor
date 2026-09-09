#!/usr/bin/env python3
"""Resolve the immutable finalized-mainnet release identity.

This control-plane script deliberately contains no package or deployment logic.
It is run from a trusted workflow checkout and fails closed on every ambiguous
chain or GitHub response.
"""
from __future__ import annotations

import argparse
import json
import os
import re
import subprocess
import sys
import tempfile
import urllib.error
import urllib.request
import zipfile
from pathlib import Path
from typing import Any

API = "https://api.github.com"
SHA_RE = re.compile(r"^[0-9a-f]{40}$")
HASH_RE = re.compile(r"^0x[0-9a-f]{64}$")
HEAD_RE = re.compile(r"^0x[0-9a-f]{64}$")
ASSET_NAMES = (
    "subtensor.wasm",
    "subtensor-digest.json",
    "proxy_proxy_blob.hex",
    "pending-release.json",
    "upgrade-manifest.json",
)


class StateError(RuntimeError):
    pass


class NotFound(StateError):
    pass


def env_required(name: str) -> str:
    value = os.environ.get(name, "")
    if not value:
        raise StateError(f"{name} is required")
    return value


def json_object(raw: bytes, context: str) -> dict[str, Any]:
    try:
        value = json.loads(raw)
    except (UnicodeDecodeError, json.JSONDecodeError) as exc:
        raise StateError(f"{context} returned malformed JSON") from exc
    if not isinstance(value, dict):
        raise StateError(f"{context} returned a non-object")
    return value


def request_json(path: str, token: str) -> dict[str, Any]:
    request = urllib.request.Request(
        API + "/" + path.lstrip("/"),
        headers={"Accept": "application/vnd.github+json", "Authorization": f"Bearer {token}", "User-Agent": "subtensor-release-state/1"},
    )
    try:
        with urllib.request.urlopen(request, timeout=30) as response:
            if response.status != 200:
                raise StateError(f"GitHub {path} returned HTTP {response.status}")
            return json_object(response.read(), path)
    except urllib.error.HTTPError as exc:
        if exc.code == 404:
            raise NotFound(path) from exc
        raise StateError(f"GitHub {path} returned HTTP {exc.code}") from exc
    except (urllib.error.URLError, TimeoutError) as exc:
        raise StateError(f"GitHub {path} request failed") from exc


def rpc(method: str, params: list[Any], endpoint: str) -> Any:
    payload = json.dumps({"id": 1, "jsonrpc": "2.0", "method": method, "params": params}).encode()
    request = urllib.request.Request(endpoint, data=payload, headers={"Content-Type": "application/json"})
    try:
        with urllib.request.urlopen(request, timeout=30) as response:
            if response.status != 200:
                raise StateError(f"RPC {method} returned HTTP {response.status}")
            value = json.loads(response.read())
    except (urllib.error.URLError, TimeoutError, json.JSONDecodeError) as exc:
        raise StateError(f"RPC {method} failed") from exc
    if not isinstance(value, dict) or value.get("error") is not None or "result" not in value:
        raise StateError(f"RPC {method} returned an invalid result")
    return value["result"]


def finalized_identity(endpoint: str) -> tuple[int, str, str]:
    head = rpc("chain_getFinalizedHead", [], endpoint)
    if not isinstance(head, str) or HEAD_RE.fullmatch(head) is None:
        raise StateError("finalized head is not a 32-byte block hash")
    version = rpc("state_getRuntimeVersion", [head], endpoint)
    if not isinstance(version, dict) or isinstance(version.get("specVersion"), bool) or not isinstance(version.get("specVersion"), int) or version["specVersion"] < 0:
        raise StateError("runtime specVersion is not a non-negative integer")
    code_hash = rpc("state_getStorageHash", ["0x3a636f6465", head], endpoint)
    if not isinstance(code_hash, str) or HASH_RE.fullmatch(code_hash) is None:
        raise StateError("runtime code hash is not a 32-byte hash")
    return version["specVersion"], head, code_hash


def local_spec() -> int:
    text = Path("runtime/src/lib.rs").read_text(encoding="utf-8")
    match = re.search(r"spec_version:\s*(\d+)", text)
    if match is None:
        raise StateError("could not parse local spec_version")
    return int(match.group(1))


def release_for_tag(repo: str, token: str, tag: str) -> dict[str, Any] | None:
    try:
        return request_json(f"repos/{repo}/releases/tags/{tag}", token)
    except NotFound:
        return None


def require_tag(repo: str, token: str, tag: str) -> str:
    ref = request_json(f"repos/{repo}/git/ref/tags/{tag}", token)
    obj = ref.get("object")
    if not isinstance(obj, dict) or obj.get("type") != "commit" or not isinstance(obj.get("sha"), str) or SHA_RE.fullmatch(obj["sha"]) is None:
        raise StateError("release tag must be a lightweight Git tag resolving to a commit")
    sha = obj["sha"]
    compare = request_json(f"repos/{repo}/compare/{sha}...main", token)
    if compare.get("status") not in ("ahead", "identical"):
        raise StateError("release tag commit is not an ancestor of main")
    return sha


def mirror_matches(repo: str, token: str, sha: str) -> None:
    ref = request_json(f"repos/{repo}/git/ref/heads/mainnet", token)
    obj = ref.get("object")
    if not isinstance(obj, dict) or obj.get("type") != "commit" or obj.get("sha") != sha:
        raise StateError("protected mainnet mirror does not match release commit")


def verify_final_assets(repo: str, token: str, release: dict[str, Any], spec: int, sha: str, code_hash: str) -> None:
    assets = release.get("assets")
    if not isinstance(assets, list):
        raise StateError("final release assets are malformed")
    by_name = {a.get("name"): a for a in assets if isinstance(a, dict)}
    if any(name not in by_name for name in ASSET_NAMES):
        raise StateError("final release is missing a verifier-required runtime asset")
    with tempfile.TemporaryDirectory() as directory:
        archive = Path(directory) / "runtime-assets.zip"
        with zipfile.ZipFile(archive, "w", compression=zipfile.ZIP_STORED) as output:
            for name in ASSET_NAMES:
                asset_id = by_name[name].get("id")
                if not isinstance(asset_id, int):
                    raise StateError(f"asset {name} has no numeric id")
                raw = download_asset(repo, token, asset_id)
                output.writestr(name, raw)
        command = [sys.executable, str(Path(__file__).with_name("verify-release-artifact.py")), str(archive), "--spec", str(spec), "--commit", sha, "--code-hash", code_hash]
        result = subprocess.run(command, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True, check=False)
        if result.returncode != 0:
            raise StateError("final release runtime assets failed verification")


def download_asset(repo: str, token: str, asset_id: int) -> bytes:
    request = urllib.request.Request(
        f"{API}/repos/{repo}/releases/assets/{asset_id}",
        headers={"Accept": "application/octet-stream", "Authorization": f"Bearer {token}", "User-Agent": "subtensor-release-state/1"},
    )
    try:
        with urllib.request.urlopen(request, timeout=30) as response:
            if response.status != 200:
                raise StateError(f"release asset {asset_id} returned HTTP {response.status}")
            return response.read()
    except urllib.error.HTTPError as exc:
        raise StateError(f"release asset {asset_id} returned HTTP {exc.code}") from exc
    except (urllib.error.URLError, TimeoutError) as exc:
        raise StateError(f"release asset {asset_id} download failed") from exc


def resolve_artifact(spec: int, sha: str, code_hash: str, repo: str, token: str) -> int:
    environment = os.environ | {"GITHUB_REPOSITORY": repo, "GH_TOKEN": token}
    command = [str(Path(__file__).with_name("resolve-release-artifact.sh")), str(spec), sha, code_hash]
    result = subprocess.run(command, env=environment, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True, check=False)
    if result.returncode != 0 or not re.fullmatch(r"[0-9]+", result.stdout.strip()):
        raise StateError("no trusted release-train artifact matches finalized identity")
    return int(result.stdout.strip())


def compute(mode: str) -> dict[str, Any]:
    repo = env_required("GITHUB_REPOSITORY")
    token = env_required("GH_TOKEN")
    endpoint = env_required("MAINNET_HTTP")
    spec, finalized_head, code_hash = finalized_identity(endpoint)
    if spec > local_spec():
        return {"eligible": False}
    if spec <= 432:
        command = ["gh", "release", "list", "--repo", repo, "--limit", "300", "--json", "tagName,isDraft,isPrerelease"]
        result = subprocess.run(command, env=os.environ | {"GH_TOKEN": token}, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True, check=False)
        if result.returncode != 0:
            raise StateError("historical release lookup failed")
        try:
            releases = json.loads(result.stdout)
        except json.JSONDecodeError as exc:
            raise StateError("historical release lookup returned malformed JSON") from exc
        if not isinstance(releases, list) or not any(
            isinstance(release, dict)
            and release.get("isDraft") is False
            and release.get("isPrerelease") is False
            and isinstance(release.get("tagName"), str)
            and (release["tagName"] == f"v{spec}" or release["tagName"].endswith(f"-{spec}"))
            for release in releases
        ):
            raise StateError(f"historical runtime v{spec} has no final GitHub release")
        return {"eligible": False}
    tag = f"v{spec}"
    sha = require_tag(repo, token, tag)
    release = release_for_tag(repo, token, tag)
    release_needed = True
    if release is not None:
        draft = release.get("draft")
        prerelease = release.get("prerelease")
        if not isinstance(draft, bool) or not isinstance(prerelease, bool):
            raise StateError("release draft/prerelease fields are malformed")
        if release.get("target_commitish") != sha:
            raise StateError("release target does not match immutable tag")
        if not draft and not prerelease:
            mirror_matches(repo, token, sha)
            verify_final_assets(repo, token, release, spec, sha, code_hash)
            release_needed = False
    if mode == "published" and release_needed:
        # Downstream publishers may run only after the independent metadata
        # reconciler has finalized the exact GitHub release.
        return {"eligible": False}
    artifact_id = None
    if release_needed:
        artifact_id = resolve_artifact(spec, sha, code_hash, repo, token)
    return {"eligible": True, "spec_version": spec, "release_tag": tag, "sha": sha, "code_hash": code_hash, "finalized_head": finalized_head, "release_needed": release_needed, "artifact_id": artifact_id}


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--mode", choices=("metadata", "published"), required=True)
    parser.add_argument("--output", required=True)
    args = parser.parse_args(argv)
    target = Path(args.output)
    try:
        # Never let a failed invocation leave a stale successful selection.
        target.unlink(missing_ok=True)
        value = compute(args.mode)
        target.parent.mkdir(parents=True, exist_ok=True)
        temporary = target.with_name(target.name + ".tmp")
        temporary.write_text(json.dumps(value, separators=(",", ":")) + "\n", encoding="utf-8")
        os.replace(temporary, target)
        return 0
    except (OSError, StateError) as exc:
        print(f"mainnet release state unavailable: {exc}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
