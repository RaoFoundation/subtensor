#!/usr/bin/env python3
"""Shared checks and Discord text for runtime and stable Python releases.

Spec 492 and the Python versions committed with it have no release page.
Those exact values are grandfathered until a later bump adds notes. The
exception is not a release claim. A notes file, when present, is still checked.
"""

from __future__ import annotations

import json
import re
import subprocess
import sys
from dataclasses import dataclass
from pathlib import Path


GRANDFATHER_SPEC = 492
GRANDFATHER_SDK = "11.3.4"
GRANDFATHER_CORE = "0.1.11"

RELEASES_DIR = Path(
    "website/apps/bittensor-website/src/app/(pages-without-footer)/releases"
)
NOTES_DIR = Path(".github/release-notes")
SCHEMA_PATH = NOTES_DIR / "schema.json"
RUNTIME_NOTES_DIR = NOTES_DIR / "runtime"
PYTHON_NOTES_DIR = NOTES_DIR / "python"

VERSION_RE = re.compile(r"^[0-9]+\.[0-9]+\.[0-9]+$")
SLUG_RE = re.compile(r"^[a-z0-9]+(?:-[a-z0-9]+)*$")
PUBLIC_PATH_RE = re.compile(r"^/releases/[a-z0-9]+(?:-[a-z0-9]+)*$")
SHA_RE = re.compile(r"^[0-9a-f]{40}$")
SHA256_RE = re.compile(r"^[0-9a-f]{64}$")
UTC_RE = re.compile(r"^[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2}Z$")
REPO_RE = re.compile(r"^[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+$")
REVISION_RE = re.compile(r"^[0-9a-f]{40}$")
SPEC_LINE_RE = re.compile(r"(?m)^\s*spec_version:\s*([0-9]+)\s*,\s*$")

URL_RE = re.compile(r"https?://[^\s<>'\"`]+")
HASH_RE = re.compile(r"\b(?:[0-9a-fA-F]{64}|[0-9a-fA-F]{40})\b")
PY_VERSION_RE = re.compile(r"\b[0-9]+\.[0-9]+\.[0-9]+(?:rc[0-9]+|\.dev[0-9]+)?\b")
SPEC_TOKEN_RE = re.compile(r"\b[0-9]{3,}\b")

NOTES_KEYS = {
    "kind",
    "spec_version",
    "packages",
    "page_path",
    "public_path",
    "audiences",
    "install_commands",
    "summary",
}
REQUIRED_KEYS = (
    "kind",
    "page_path",
    "public_path",
    "audiences",
    "install_commands",
    "summary",
)
MAX_NOTES_BYTES = 64 * 1024
MAX_SUMMARY_CHARS = 4000
DISCORD_LIMIT = 2000
IMAGE_REPO = "ghcr.io/raofoundation/subtensor"
DEFAULT_REPO = "RaoFoundation/subtensor"

SAMPLE_BANNER = "DRY RUN SAMPLE. Discord was not called. This is not a release note."
SAMPLE_SUMMARY = (
    "Sample only. This text is not a release note and does not describe what shipped."
)
MISSING_SUMMARY = (
    "Release notes for this version were not written yet. The lines below are the release facts."
)

KIND_TITLES = {
    "runtime-proposed": "Runtime upgrade proposed",
    "runtime-live": "Runtime upgrade is live",
    "python-stable": "Stable Python packages published",
}
RUNTIME_KINDS = frozenset({"runtime-proposed", "runtime-live"})


class NotesError(Exception):
    """The notes file or the rendered message is not usable."""


@dataclass(frozen=True)
class CheckResult:
    ok: bool
    grandfathered: bool
    detail: str


@dataclass(frozen=True)
class LoadedNotes:
    kind: str
    summary: str
    install_commands: tuple[str, ...]
    audiences: tuple[str, ...]
    public_path: str
    spec_version: int | None
    sdk_version: str | None
    core_version: str | None


@dataclass(frozen=True)
class Announcement:
    message: str
    summary: str
    facts: str
    model_status: str


def grandfather_spec_detail() -> str:
    return (
        f"spec {GRANDFATHER_SPEC} is grandfathered. There is no v{GRANDFATHER_SPEC} "
        "release page and no "
        f".github/release-notes/runtime/{GRANDFATHER_SPEC}.json. "
        "A later spec bump needs both. This check does not invent release claims."
    )


def grandfather_python_detail() -> str:
    return (
        f"bittensor {GRANDFATHER_SDK} and bittensor-core {GRANDFATHER_CORE} are "
        "grandfathered. There is no "
        f".github/release-notes/python/{GRANDFATHER_SDK}.json. "
        "A later Python version bump needs that file and a real releases page."
    )


def _fail(message: str) -> None:
    raise NotesError(message)


def validate_relative(relative: str) -> Path:
    path = Path(relative)
    if relative == "" or path.is_absolute() or "\\" in relative or ".." in path.parts:
        _fail(f"path must stay inside the repository: {relative}")
    return path


def validate_revision(revision: str) -> str:
    if REVISION_RE.fullmatch(revision) is None:
        _fail("revision must be a 40-character commit sha")
    return revision


def read_text(
    root: Path,
    relative: str,
    revision: str | None,
    *,
    limit: int | None = None,
) -> str | None:
    relative_path = validate_relative(relative)
    if revision is None:
        file_path = root / relative_path
        if not file_path.is_file():
            return None
        data = file_path.read_bytes()
    else:
        validate_revision(revision)
        completed = subprocess.run(
            ["git", "show", f"{revision}:{relative_path.as_posix()}"],
            cwd=root,
            check=False,
            capture_output=True,
        )
        if completed.returncode != 0:
            error = completed.stderr.decode("utf-8", errors="replace")
            missing = "does not exist" in error or "but not in" in error
            if missing:
                return None
            _fail(f"could not read {relative} at {revision}: {error.strip()}")
        data = completed.stdout
    if limit is not None and len(data) > limit:
        _fail(f"{relative} is larger than {limit} bytes")
    return data.decode("utf-8")


def read_spec(text: str) -> int:
    matches = SPEC_LINE_RE.findall(text)
    if len(matches) != 1:
        _fail(f"expected exactly one spec_version line, found {len(matches)}")
    return int(matches[0])


def python_versions_from_files(sdk_manifest: Path, core_manifest: Path) -> tuple[str, str]:
    script = Path(__file__).with_name("release-python-versions.py")
    completed = subprocess.run(
        [sys.executable, str(script), str(sdk_manifest), str(core_manifest)],
        check=False,
        capture_output=True,
        text=True,
    )
    if completed.returncode != 0:
        detail = completed.stderr.strip() or completed.stdout.strip()
        _fail(detail or "could not read Python versions")
    try:
        value = json.loads(completed.stdout)
    except json.JSONDecodeError as error:
        _fail(f"Python version script did not return JSON: {error}")
    if not isinstance(value, dict):
        _fail("Python version script did not return an object")
    sdk = value.get("sdk")
    core = value.get("core")
    if not isinstance(sdk, str) or VERSION_RE.fullmatch(sdk) is None:
        _fail("SDK release version must be X.Y.Z")
    if not isinstance(core, str) or VERSION_RE.fullmatch(core) is None:
        _fail("core release version must be X.Y.Z")
    return sdk, core


def versions_from_root(root: Path) -> dict[str, object]:
    runtime = read_text(root, "runtime/src/lib.rs", None)
    if runtime is None:
        _fail("runtime/src/lib.rs is missing")
    sdk_path = root / "sdk/python/pyproject.toml"
    core_path = root / "sdk/bittensor-core-py/pyproject.toml"
    if not sdk_path.is_file() or not core_path.is_file():
        _fail("Python manifests are missing")
    sdk, core = python_versions_from_files(sdk_path, core_path)
    return {"spec": read_spec(runtime), "sdk": sdk, "core": core}


def _nonempty_lines(value: object, label: str) -> tuple[str, ...]:
    if not isinstance(value, list) or len(value) == 0:
        _fail(f"{label} must be a non-empty list")
    lines: list[str] = []
    for item in value:
        if not isinstance(item, str) or item.strip() == "":
            _fail(f"{label} must be non-empty strings")
        if "\n" in item or "\r" in item or "`" in item:
            _fail(f"{label} entries must be single lines without backticks")
        lines.append(item.strip())
    return tuple(lines)


def _package_map(value: object) -> dict[str, str]:
    if not isinstance(value, list) or len(value) != 2:
        _fail("packages must list bittensor and bittensor-core")
    found: dict[str, str] = {}
    for item in value:
        if not isinstance(item, dict):
            _fail("each package entry must be an object")
        extra = set(item) - {"name", "version"}
        if extra:
            _fail(f"package entry has unknown fields: {sorted(extra)}")
        name = item.get("name")
        version = item.get("version")
        if name not in {"bittensor", "bittensor-core"}:
            _fail("package name must be bittensor or bittensor-core")
        if name in found:
            _fail(f"duplicate package {name}")
        if not isinstance(version, str) or VERSION_RE.fullmatch(version) is None:
            _fail(f"{name} version must be X.Y.Z")
        found[name] = version
    if set(found) != {"bittensor", "bittensor-core"}:
        _fail("packages must include bittensor and bittensor-core")
    return found


def check_schema_document(root: Path) -> None:
    text = read_text(root, SCHEMA_PATH.as_posix(), None, limit=MAX_NOTES_BYTES)
    if text is None:
        _fail(f"missing {SCHEMA_PATH.as_posix()}")
    try:
        document = json.loads(text)
    except json.JSONDecodeError as error:
        _fail(f"schema.json is not valid JSON: {error}")
    if not isinstance(document, dict):
        _fail("schema.json must be an object")
    required = document.get("required")
    if not isinstance(required, list) or tuple(required) != REQUIRED_KEYS:
        _fail("schema.json required fields drifted from the checker")
    kind = document.get("properties", {})
    if not isinstance(kind, dict):
        _fail("schema.json properties must be an object")
    kind_enum = kind.get("kind", {}).get("enum") if isinstance(kind.get("kind"), dict) else None
    if kind_enum != ["runtime", "python"]:
        _fail("schema.json kind enum drifted from the checker")
    if document.get("additionalProperties") is not False:
        _fail("schema.json must set additionalProperties to false")


def validate_notes_document(document: object) -> LoadedNotes:
    if not isinstance(document, dict):
        _fail("release notes must be a JSON object")
    extra = set(document) - NOTES_KEYS
    if extra:
        _fail(f"release notes have unknown fields: {sorted(extra)}")
    for key in REQUIRED_KEYS:
        if key not in document:
            _fail(f"release notes are missing {key}")
    kind = document["kind"]
    if kind not in {"runtime", "python"}:
        _fail("kind must be runtime or python")
    page_path = document["page_path"]
    public_path = document["public_path"]
    if not isinstance(page_path, str) or not isinstance(public_path, str):
        _fail("page_path and public_path must be strings")
    if PUBLIC_PATH_RE.fullmatch(public_path) is None:
        _fail("public_path must look like /releases/<slug>")
    summary = document["summary"]
    if not isinstance(summary, str) or summary.strip() == "":
        _fail("summary must be non-empty")
    if len(summary) > MAX_SUMMARY_CHARS:
        _fail(f"summary is longer than {MAX_SUMMARY_CHARS} characters")
    audiences = _nonempty_lines(document["audiences"], "audiences")
    install_commands = _nonempty_lines(document["install_commands"], "install_commands")
    spec_version: int | None = None
    sdk_version: str | None = None
    core_version: str | None = None
    if kind == "runtime":
        if "packages" in document:
            _fail("runtime notes must not include packages")
        if "spec_version" not in document or isinstance(document["spec_version"], bool):
            _fail("runtime notes must include spec_version")
        if not isinstance(document["spec_version"], int) or document["spec_version"] < 1:
            _fail("spec_version must be an integer >= 1")
        spec_version = document["spec_version"]
    else:
        if "spec_version" in document:
            _fail("python notes must not include spec_version")
        packages = _package_map(document.get("packages"))
        sdk_version = packages["bittensor"]
        core_version = packages["bittensor-core"]
    return LoadedNotes(
        kind=kind,
        summary=summary.strip(),
        install_commands=install_commands,
        audiences=audiences,
        public_path=public_path,
        spec_version=spec_version,
        sdk_version=sdk_version,
        core_version=core_version,
    )


def validate_page(root: Path, page_path: str, public_path: str, revision: str | None) -> None:
    path = validate_relative(page_path)
    prefix = RELEASES_DIR.parts
    if path.parts[: len(prefix)] != prefix:
        _fail(f"page_path must be under {RELEASES_DIR.as_posix()}")
    if path.name != "page.tsx" or len(path.parts) != len(prefix) + 2:
        _fail("page_path must be releases/<slug>/page.tsx")
    slug = path.parts[-2]
    if SLUG_RE.fullmatch(slug) is None:
        _fail("release page folder is not a slug")
    if public_path != f"/releases/{slug}":
        _fail(f"public_path must be /releases/{slug}")
    if read_text(root, path.as_posix(), revision) is None:
        _fail(f"page_path does not exist: {path.as_posix()}")


def load_notes_file(root: Path, relative: str, revision: str | None) -> tuple[LoadedNotes, str]:
    text = read_text(root, relative, revision, limit=MAX_NOTES_BYTES)
    if text is None:
        _fail(f"missing release notes: {relative}")
    try:
        document = json.loads(text)
    except json.JSONDecodeError as error:
        _fail(f"{relative} is not valid JSON: {error}")
    notes = validate_notes_document(document)
    if not isinstance(document, dict) or not isinstance(document.get("page_path"), str):
        _fail("page_path must be a string")
    validate_page(root, document["page_path"], notes.public_path, revision)
    return notes, document["page_path"]


def runtime_notes_relative(spec: int) -> str:
    if spec < 1:
        _fail("spec must be >= 1")
    return f"{RUNTIME_NOTES_DIR.as_posix()}/{spec}.json"


def python_notes_relative(sdk: str) -> str:
    if VERSION_RE.fullmatch(sdk) is None:
        _fail("SDK version must be X.Y.Z")
    return f"{PYTHON_NOTES_DIR.as_posix()}/{sdk}.json"


def check_runtime(root: Path, spec: int, revision: str | None = None) -> CheckResult:
    if spec < 1:
        return CheckResult(False, False, "spec must be >= 1")
    relative = runtime_notes_relative(spec)
    try:
        text = read_text(root, relative, revision, limit=MAX_NOTES_BYTES)
    except NotesError as error:
        return CheckResult(False, False, str(error))
    if text is None:
        if spec == GRANDFATHER_SPEC:
            return CheckResult(True, True, grandfather_spec_detail())
        return CheckResult(
            False,
            False,
            f"missing {relative}. Add it and a real releases page before proposing spec {spec}.",
        )
    try:
        document = json.loads(text)
        notes = validate_notes_document(document)
        if notes.spec_version != spec:
            _fail(f"{relative} spec_version is {notes.spec_version}, expected {spec}")
        if not isinstance(document, dict) or not isinstance(document.get("page_path"), str):
            _fail("page_path must be a string")
        validate_page(root, document["page_path"], notes.public_path, revision)
    except (NotesError, json.JSONDecodeError) as error:
        return CheckResult(False, False, str(error))
    return CheckResult(True, False, f"runtime notes for spec {spec} match this commit")


def check_python(
    root: Path,
    sdk: str,
    core: str,
    revision: str | None = None,
) -> CheckResult:
    if VERSION_RE.fullmatch(sdk) is None or VERSION_RE.fullmatch(core) is None:
        return CheckResult(False, False, "Python versions must be X.Y.Z")
    relative = python_notes_relative(sdk)
    try:
        text = read_text(root, relative, revision, limit=MAX_NOTES_BYTES)
    except NotesError as error:
        return CheckResult(False, False, str(error))
    if text is None:
        if sdk == GRANDFATHER_SDK and core == GRANDFATHER_CORE:
            return CheckResult(True, True, grandfather_python_detail())
        return CheckResult(
            False,
            False,
            f"missing {relative} for bittensor {sdk} and bittensor-core {core}.",
        )
    try:
        document = json.loads(text)
        notes = validate_notes_document(document)
        if notes.sdk_version != sdk or notes.core_version != core:
            _fail(
                f"{relative} lists bittensor {notes.sdk_version} and "
                f"bittensor-core {notes.core_version}, expected {sdk} and {core}"
            )
        if not isinstance(document, dict) or not isinstance(document.get("page_path"), str):
            _fail("page_path must be a string")
        validate_page(root, document["page_path"], notes.public_path, revision)
    except (NotesError, json.JSONDecodeError) as error:
        return CheckResult(False, False, str(error))
    return CheckResult(
        True,
        False,
        f"python notes for bittensor {sdk} and bittensor-core {core} match this commit",
    )


def _iter_notes(root: Path, folder: Path) -> list[Path]:
    directory = root / folder
    if not directory.exists():
        return []
    if not directory.is_dir():
        _fail(f"{folder.as_posix()} is not a directory")
    files = sorted(directory.iterdir())
    notes: list[Path] = []
    for file_path in files:
        if file_path.name.startswith("."):
            continue
        if not file_path.is_file() or file_path.suffix != ".json":
            _fail(f"unexpected release-notes entry: {file_path.relative_to(root).as_posix()}")
        notes.append(file_path)
    return notes


def check_tree(root: Path) -> CheckResult:
    try:
        check_schema_document(root)
        errors: list[str] = []
        for file_path in _iter_notes(root, RUNTIME_NOTES_DIR):
            stem = file_path.stem
            if re.fullmatch(r"[1-9][0-9]*", stem) is None:
                errors.append(f"{file_path.name} must be named <spec>.json")
                continue
            result = check_runtime(root, int(stem))
            if not result.ok:
                errors.append(result.detail)
        for file_path in _iter_notes(root, PYTHON_NOTES_DIR):
            if VERSION_RE.fullmatch(file_path.stem) is None:
                errors.append(f"{file_path.name} must be named <bittensor version>.json")
                continue
            text = file_path.read_text(encoding="utf-8")
            try:
                document = json.loads(text)
                notes = validate_notes_document(document)
            except (NotesError, json.JSONDecodeError) as error:
                errors.append(f"{file_path.name}: {error}")
                continue
            if notes.sdk_version != file_path.stem:
                errors.append(
                    f"{file_path.name} must list bittensor {file_path.stem}, "
                    f"found {notes.sdk_version}"
                )
                continue
            result = check_python(root, notes.sdk_version, notes.core_version or "")
            if not result.ok:
                errors.append(result.detail)
    except NotesError as error:
        return CheckResult(False, False, str(error))
    if errors:
        return CheckResult(False, False, "\n".join(errors))
    return CheckResult(True, False, "release-notes files match the schema")


def check_diff(
    root: Path,
    *,
    base_spec: int,
    head_spec: int,
    base_sdk: str,
    head_sdk: str,
    base_core: str,
    head_core: str,
) -> CheckResult:
    messages: list[str] = []
    errors: list[str] = []
    grandfathered = False
    if head_spec != base_spec:
        result = check_runtime(root, head_spec)
        grandfathered = grandfathered or result.grandfathered
        (messages if result.ok else errors).append(result.detail)
    if (head_sdk, head_core) != (base_sdk, base_core):
        result = check_python(root, head_sdk, head_core)
        grandfathered = grandfathered or result.grandfathered
        (messages if result.ok else errors).append(result.detail)
    tree = check_tree(root)
    if not tree.ok:
        errors.append(tree.detail)
    if errors:
        return CheckResult(False, False, "\n".join(errors))
    if not messages:
        messages.append("no spec or Python version bump")
    return CheckResult(True, grandfathered, "\n".join(messages))


def _strip_url(url: str) -> str:
    return url.rstrip(".,);]>")


def fact_tokens(text: str) -> set[tuple[str, str]]:
    found: set[tuple[str, str]] = set()
    for match in URL_RE.finditer(text):
        found.add(("url", _strip_url(match.group(0))))
    for match in HASH_RE.finditer(text):
        found.add(("hash", match.group(0).lower()))
    for match in PY_VERSION_RE.finditer(text):
        found.add(("version", match.group(0)))
    for match in SPEC_TOKEN_RE.finditer(text):
        found.add(("number", match.group(0)))
    return found


def unexpected_model_tokens(model_text: str, facts: str) -> list[str]:
    allowed = fact_tokens(facts)
    extra: list[str] = []
    for kind, value in sorted(fact_tokens(model_text)):
        if (kind, value) not in allowed:
            extra.append(f"{kind} {value}")
    return extra


def default_install(sdk: str) -> tuple[str, ...]:
    return (
        "uv venv && source .venv/bin/activate",
        f'uv pip install "bittensor=={sdk}"',
    )


def default_audiences(kind: str) -> tuple[str, ...]:
    if kind in RUNTIME_KINDS:
        return ("node operators", "Python users")
    return ("Python users",)


def _images(kind: str, spec: int | None) -> tuple[str, ...]:
    if spec is None:
        return ()
    tagged = f"{IMAGE_REPO}:v{spec}"
    if kind == "runtime-proposed":
        return (tagged,)
    if kind == "runtime-live":
        return (tagged, f"{IMAGE_REPO}:mainnet", f"{IMAGE_REPO}:latest")
    return ()


def _build_facts(
    *,
    kind: str,
    spec: int | None,
    sdk: str,
    core: str,
    sha: str,
    sha256: str | None,
    utc: str,
    repo: str,
    public_path: str | None,
    audiences: tuple[str, ...],
    install_commands: tuple[str, ...],
) -> str:
    lines = ["Facts", f"- UTC: `{utc}`"]
    if spec is not None:
        lines.append(f"- Spec: `{spec}`")
    lines.append(f"- Commit: `{sha}`")
    if sha256 is not None:
        lines.append(f"- Wasm sha256: `{sha256}`")
    lines.append(f"- bittensor: `{sdk}`")
    lines.append(f"- bittensor-core: `{core}`")
    if kind in RUNTIME_KINDS and spec is not None:
        lines.append(f"- Release: https://github.com/{repo}/releases/tag/v{spec}")
    if public_path:
        lines.append(f"- Notes: https://bittensor.com{public_path}")
    for image in _images(kind, spec):
        lines.append(f"- Image: `{image}`")
    if kind in {"runtime-live", "python-stable"}:
        lines.append(f"- PyPI bittensor: https://pypi.org/project/bittensor/{sdk}/")
        lines.append(f"- PyPI bittensor-core: https://pypi.org/project/bittensor-core/{core}/")
    lines.append(f"- Audiences: {', '.join(audiences)}")
    lines.append("- Install:")
    lines.append("```")
    lines.extend(install_commands)
    lines.append("```")
    return "\n".join(lines)


def _assemble(banner: str | None, title: str, summary: str, facts: str) -> str:
    parts: list[str] = []
    if banner:
        parts.extend([banner, ""])
    parts.extend([f"**{title}**", "", summary.rstrip(), "", facts.rstrip()])
    return "\n".join(parts).rstrip() + "\n"


def fit_message(banner: str | None, title: str, summary: str, facts: str) -> str:
    message = _assemble(banner, title, summary, facts)
    if len(message) <= DISCORD_LIMIT:
        return message
    body = summary.rstrip()
    if len(_assemble(banner, title, "", facts)) > DISCORD_LIMIT - 40:
        _fail("fact block is too long for a Discord message")
    while body and len(_assemble(banner, title, body + "...", facts)) > DISCORD_LIMIT:
        body = body[:-1].rstrip()
    trimmed = (body + "...") if body else ""
    message = _assemble(banner, title, trimmed, facts)
    if not trimmed or len(message) > DISCORD_LIMIT:
        _fail("Discord message is longer than 2000 characters")
    return message


def render_announcement(
    *,
    kind: str,
    spec: int | None,
    sdk: str,
    core: str,
    sha: str,
    sha256: str | None,
    utc: str,
    repo: str = DEFAULT_REPO,
    root: Path | None = None,
    notes_file: str | None = None,
    revision: str | None = None,
    public_path: str | None = None,
    sample: bool = False,
    allow_missing_notes: bool = False,
    model_summary: str | None = None,
) -> Announcement:
    if kind not in KIND_TITLES:
        _fail("kind must be runtime-proposed, runtime-live, or python-stable")
    if VERSION_RE.fullmatch(sdk) is None or VERSION_RE.fullmatch(core) is None:
        _fail("Python versions must be X.Y.Z")
    if SHA_RE.fullmatch(sha) is None:
        _fail("sha must be 40 lowercase hex characters")
    if UTC_RE.fullmatch(utc) is None:
        _fail("utc must look like 2026-11-19T13:28:00Z")
    if REPO_RE.fullmatch(repo) is None:
        _fail("repo must look like owner/name")
    if kind in RUNTIME_KINDS:
        if spec is None or spec < 1:
            _fail("runtime announcements need a spec")
        if sha256 is None or SHA256_RE.fullmatch(sha256) is None:
            _fail("runtime announcements need a 64-character sha256")
    elif spec is not None or sha256 is not None:
        _fail("python announcements do not take a spec or a wasm sha256")

    summary = SAMPLE_SUMMARY if sample else MISSING_SUMMARY
    install_commands = default_install(sdk)
    audiences = default_audiences(kind)
    notes_public = public_path
    repo_root = root or Path(".")

    if notes_file:
        notes, _page = load_notes_file(repo_root, notes_file, revision)
        expected_kind = "runtime" if kind in RUNTIME_KINDS else "python"
        if notes.kind != expected_kind:
            _fail(f"{notes_file} kind does not match {kind}")
        if kind in RUNTIME_KINDS and notes.spec_version != spec:
            _fail(f"{notes_file} spec does not match {spec}")
        if kind == "python-stable" and (notes.sdk_version != sdk or notes.core_version != core):
            _fail(f"{notes_file} package versions do not match {sdk} and {core}")
        if not sample:
            summary = notes.summary
        install_commands = notes.install_commands
        audiences = notes.audiences
        notes_public = notes.public_path
    elif not sample and not allow_missing_notes:
        _fail("release notes are required")
    elif not sample:
        grandfather_ok = (kind in RUNTIME_KINDS and spec == GRANDFATHER_SPEC) or (
            kind == "python-stable" and sdk == GRANDFATHER_SDK and core == GRANDFATHER_CORE
        )
        if not grandfather_ok:
            _fail("release notes are missing and this version is not grandfathered")
        notes_public = None
    elif notes_public is not None and PUBLIC_PATH_RE.fullmatch(notes_public) is None:
        _fail("public_path must look like /releases/<slug>")

    facts = _build_facts(
        kind=kind,
        spec=spec,
        sdk=sdk,
        core=core,
        sha=sha,
        sha256=sha256,
        utc=utc,
        repo=repo,
        public_path=notes_public,
        audiences=audiences,
        install_commands=install_commands,
    )
    model_status = "absent"
    chosen = summary
    if model_summary is not None and model_summary.strip() != "":
        extra = unexpected_model_tokens(model_summary, facts)
        if extra:
            model_status = "rejected: " + ", ".join(extra)
        elif len(model_summary.strip()) > MAX_SUMMARY_CHARS:
            model_status = "rejected: summary is too long"
        else:
            chosen = model_summary.strip()
            model_status = "accepted"
    banner = SAMPLE_BANNER if sample else None
    message = fit_message(banner, KIND_TITLES[kind], chosen, facts)
    return Announcement(message=message, summary=chosen, facts=facts, model_status=model_status)
