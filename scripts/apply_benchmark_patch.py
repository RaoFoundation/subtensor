#!/usr/bin/env python3
"""Download the latest completed benchmark result for one exact PR source."""

import argparse
import json
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
        (args.output_dir / "benchmark_patch.diff").write_bytes(patch)
        print("Validated benchmark patch is ready to apply.")
    else:
        print("Latest benchmark run completed with no weight changes to apply.")


if __name__ == "__main__":
    try:
        main()
    except (ValidationError, OSError) as error:
        print(f"ERROR: {error}", file=sys.stderr)
        sys.exit(1)
