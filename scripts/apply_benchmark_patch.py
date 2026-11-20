#!/usr/bin/env python3
"""Download the latest completed benchmark result for one exact PR source."""

import argparse
import json
import os
from pathlib import Path, PurePosixPath
import re
import subprocess
import sys
import tarfile
import tempfile


class ValidationError(ValueError):
    """The latest benchmark result cannot safely be applied."""


def matching_runs(runs, repo, branch, sha):
    """Return eligible runs newest-attempt-first, independent of their scope."""
    matches = []
    for run in runs:
        source_shas = {run.get("head_sha")}
        source_shas.update(
            pr.get("head", {}).get("sha") for pr in run.get("pull_requests", [])
        )
        if (
            run.get("event") in {"pull_request", "workflow_dispatch", "schedule"}
            and run.get("display_title") != "Benchmark gate only"
            and (run.get("head_repository") or {}).get("full_name") == repo
            and run.get("head_branch") == branch
            and sha in source_shas
        ):
            matches.append(run)
    return sorted(
        matches,
        key=lambda run: (
            run.get("run_started_at") or run.get("created_at") or "",
            run["id"],
        ),
        reverse=True,
    )


def select_latest_run(runs, repo, branch, sha, jobs_for_run):
    """Skip gate-only runs; never fall back past an actual benchmark attempt."""
    for run in matching_runs(runs, repo, branch, sha):
        jobs = [
            job for job in jobs_for_run(run) if job.get("name") == "validate-benchmarks"
        ]
        if len(jobs) != 1:
            raise ValidationError(
                f"Run {run['id']} has no unique benchmark job yet; "
                "wait for it to finish and reapply the label."
            )
        job = jobs[0]
        if job.get("conclusion") == "skipped":
            continue
        if run.get("status") != "completed" or job.get("status") != "completed":
            raise ValidationError(
                f"Latest benchmark run {run['id']} is still running; "
                "wait for it to finish and reapply the label."
            )
        # Weight drift deliberately fails the job, so its conclusion is not a gate.
        return run
    raise ValidationError(f"No benchmark run found for {repo}:{branch} at {sha}.")


def select_artifact(artifacts):
    """Require the latest named artifact from the already-selected run."""
    patches = [item for item in artifacts if item.get("name") == "bench-patch"]
    if not patches:
        raise ValidationError("Latest benchmark run has no bench-patch artifact.")
    artifact = max(patches, key=lambda item: (item.get("created_at", ""), item["id"]))
    if artifact.get("expired") is not False:
        raise ValidationError(
            "Latest bench-patch artifact has expired; rerun benchmarks."
        )
    return artifact


def read_patch_archive(archive_path, run, sha):
    """Read the two permitted files without extracting arbitrary archive paths."""
    limits = {"summary.txt": 1024 * 1024, "benchmark_patch.diff": 32 * 1024 * 1024}
    files = {}
    try:
        with tarfile.open(archive_path, "r:gz") as archive:
            for member in archive:
                path = PurePosixPath(member.name)
                if (
                    path.is_absolute()
                    or ".." in path.parts
                    or not (member.isfile() or member.isdir())
                ):
                    raise ValidationError(
                        f"Unsafe benchmark archive member: {member.name}"
                    )
                if len(path.parts) != 2 or path.parts[0] != ".bench_patch":
                    continue
                name = path.name
                if name not in limits:
                    continue
                if name in files or not member.isfile() or member.size > limits[name]:
                    raise ValidationError(
                        f"Invalid benchmark archive member: {member.name}"
                    )
                source = archive.extractfile(member)
                if source is None:
                    raise ValidationError(
                        f"Unreadable benchmark archive member: {member.name}"
                    )
                files[name] = source.read()
    except (tarfile.TarError, OSError, EOFError) as error:
        raise ValidationError(f"Cannot read benchmark archive: {error}") from error
    if "summary.txt" not in files:
        raise ValidationError("Latest benchmark artifact has no summary.txt.")
    try:
        summary = files["summary.txt"].decode("utf-8")
    except UnicodeDecodeError as error:
        raise ValidationError("Benchmark summary is not UTF-8.") from error
    expected = {
        "Head SHA": sha,
        "Run ID": str(run["id"]),
        "Run attempt": str(run["run_attempt"]),
        "Status": "COMPLETE",
    }
    for key, value in expected.items():
        values = [
            line[len(key) + 2 :]
            for line in summary.splitlines()
            if line.startswith(f"{key}: ")
        ]
        if values != [value]:
            raise ValidationError(
                f"Benchmark summary {key} does not match {value!r}; "
                "rerun benchmarks for the current PR head."
            )
    patch = files.get("benchmark_patch.diff")
    return summary, patch if patch and patch.strip() else None


def stage_patch(checkout, patch_path, sha):
    """Stage only regular, existing weight-file modifications at an exact source."""
    if not re.fullmatch(r"[0-9a-f]{40}", sha):
        raise ValidationError("Expected a full lowercase source commit SHA.")
    checkout = Path(checkout).resolve()
    patch = Path(patch_path).read_bytes()
    environment = os.environ.copy()
    environment.pop("GIT_INDEX_FILE", None)

    def git(*args, index=None, input_data=None):
        env = environment.copy()
        if index is not None:
            env["GIT_INDEX_FILE"] = str(index)
        try:
            return subprocess.check_output(
                [
                    "git",
                    "--no-optional-locks",
                    "-c",
                    "core.hooksPath=/dev/null",
                    "-c",
                    "core.fsmonitor=false",
                    "-C",
                    str(checkout),
                    *args,
                ],
                input=input_data,
                stderr=subprocess.PIPE,
                env=env,
            )
        except (subprocess.CalledProcessError, OSError) as error:
            raise ValidationError(f"Cannot stage benchmark patch: {error}") from error

    def require_clean_source():
        if git("rev-parse", "HEAD").decode().strip() != sha:
            raise ValidationError("Checkout HEAD does not match the benchmark source.")
        if git("status", "--porcelain=v1", "--untracked-files=no"):
            raise ValidationError(
                "Benchmark source has tracked changes; refusing to apply."
            )

    require_clean_source()
    numstat = git("apply", "--numstat", "-z", input_data=patch).split(b"\0")
    path_pattern = rb"pallets/[A-Za-z0-9_-]+/src/weights\.rs"
    if numstat[-1] != b"" or len(numstat) < 2:
        raise ValidationError("Benchmark patch has no weight-file changes.")
    for record in numstat[:-1]:
        if not re.fullmatch(rb"[0-9]+\t[0-9]+\t" + path_pattern, record):
            raise ValidationError(
                "Benchmark patch must contain only text weight-file changes."
            )

    # Validate the resulting tree before allowing any patch content into the
    # source index or worktree. Path checks alone do not reject mode changes,
    # additions, deletions, symlinks, or renames between otherwise valid paths.
    with tempfile.TemporaryDirectory(prefix="benchmark-patch-index-") as temporary:
        index = Path(temporary) / "index"
        git("read-tree", sha, index=index)
        git("apply", "--cached", index=index, input_data=patch)
        records = git(
            "diff",
            "--cached",
            "--raw",
            "--no-renames",
            "--no-ext-diff",
            "--no-textconv",
            "-z",
            sha,
            "--",
            index=index,
        ).split(b"\0")
        if records[-1] != b"" or len(records) < 3 or len(records) % 2 != 1:
            raise ValidationError(
                "Benchmark patch does not modify existing weight files."
            )
        for metadata, path in zip(records[0:-1:2], records[1:-1:2]):
            fields = metadata.split()
            if (
                len(fields) != 5
                or fields[0:2] != [b":100644", b"100644"]
                or fields[4] != b"M"
                or not re.fullmatch(path_pattern, path)
            ):
                raise ValidationError(
                    "Benchmark patch may only modify existing regular weight files."
                )

    require_clean_source()
    git("apply", "--index", input_data=patch)


def gh(*args):
    try:
        return subprocess.check_output(["gh", *args], text=True)
    except (subprocess.CalledProcessError, OSError) as error:
        raise ValidationError(f"GitHub CLI request failed: {error}") from error


def api_items(endpoint, field, **parameters):
    args = [
        "api",
        "--method",
        "GET",
        "--paginate",
        "--jq",
        f".{field}[] | @json",
        endpoint,
    ]
    for key, value in {"per_page": 100, **parameters}.items():
        args.extend(["-f", f"{key}={value}"])
    response = gh(*args)
    try:
        return [json.loads(line) for line in response.splitlines() if line.strip()]
    except (ValueError, KeyError, TypeError) as error:
        raise ValidationError(f"Invalid GitHub API response for {endpoint}.") from error


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--repo", required=True)
    parser.add_argument("--branch", required=True)
    parser.add_argument("--sha", required=True)
    parser.add_argument("--output-dir", required=True, type=Path)
    parser.add_argument("--apply-to", type=Path)
    args = parser.parse_args()
    if not re.fullmatch(r"[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+", args.repo):
        raise ValidationError("Expected an owner/repository name.")
    if not re.fullmatch(r"[0-9a-f]{40}", args.sha):
        raise ValidationError("Expected a full lowercase source commit SHA.")
    if args.output_dir.exists() and any(args.output_dir.iterdir()):
        raise ValidationError("Output directory must be empty.")
    prefix = f"repos/{args.repo}/actions"
    runs = api_items(
        f"{prefix}/workflows/run-benchmarks.yml/runs",
        "workflow_runs",
        branch=args.branch,
    )
    run = select_latest_run(
        runs,
        args.repo,
        args.branch,
        args.sha,
        lambda item: api_items(
            f"{prefix}/runs/{item['id']}/attempts/{item['run_attempt']}/jobs", "jobs"
        ),
    )
    select_artifact(api_items(f"{prefix}/runs/{run['id']}/artifacts", "artifacts"))
    print(
        f"Selected benchmark run {run['id']} (attempt {run['run_attempt']}): "
        f"{run.get('html_url', '')}"
    )
    with tempfile.TemporaryDirectory(prefix="benchmark-patch-") as temporary:
        gh(
            "run",
            "download",
            str(run["id"]),
            "--repo",
            args.repo,
            "--name",
            "bench-patch",
            "--dir",
            temporary,
        )
        summary, patch = read_patch_archive(
            Path(temporary) / "bench-patch.tgz", run, args.sha
        )
    args.output_dir.mkdir(parents=True, exist_ok=True)
    (args.output_dir / "summary.txt").write_text(summary, encoding="utf-8")
    if patch is not None:
        patch_path = args.output_dir / "benchmark_patch.diff"
        patch_path.write_bytes(patch)
        if args.apply_to is not None:
            stage_patch(args.apply_to, patch_path, args.sha)
            print("Validated benchmark patch staged for commit.")
        else:
            print("Validated benchmark patch is ready to apply.")
    else:
        print("Latest benchmark run completed with no weight changes to apply.")


if __name__ == "__main__":
    try:
        main()
    except (ValidationError, OSError) as error:
        print(f"ERROR: {error}", file=sys.stderr)
        sys.exit(1)
