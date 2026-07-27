#!/usr/bin/env python3
"""Generate refactor/refactor-manifest.json with exclusive file ownership shards."""

from __future__ import annotations

import json
import os
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]


def rs_files(base: Path) -> list[str]:
    if not base.exists():
        return []
    out = []
    for p in sorted(base.rglob("*.rs")):
        # Never assign generated weights to a mutating shard
        if p.name == "weights.rs" and "subtensor" in p.parts:
            continue
        out.append(p.relative_to(ROOT).as_posix())
    return out


def shard(sid: str, wave: int, task: str, files: list[str], notes: str = "") -> dict:
    return {
        "id": sid,
        "wave": wave,
        "task": task,
        "status": "pending",
        "notes": notes,
        "files": files,
    }


def main() -> None:
    shards: list[dict] = []

    # Wave 1 — independent crates
    small_pallets = [
        "admin-utils",
        "alpha-assets",
        "commitments",
        "crowdloan",
        "drand",
        "limit-orders",
        "shield",
        "transaction-fee",
        "proxy",
        "utility",
    ]
    for name in small_pallets:
        files = rs_files(ROOT / "pallets" / name)
        shards.append(
            shard(
                f"w1-{name}",
                1,
                "discoverability",
                files,
                "Docs on storage/calls/events/errors; rename private helpers; split files >1000 lines",
            )
        )

    # swap is its own tree
    shards.append(
        shard(
            "w1-swap",
            1,
            "discoverability",
            rs_files(ROOT / "pallets" / "swap"),
            "Pallet swap + rpc + runtime-api; freeze Solidity/RPC strings",
        )
    )

    for name, path in [
        ("node", ROOT / "node"),
        ("common", ROOT / "common"),
        ("primitives", ROOT / "primitives"),
        ("support", ROOT / "support"),
        ("chain-extensions", ROOT / "chain-extensions"),
        ("runtime", ROOT / "runtime"),
    ]:
        files = rs_files(path)
        # runtime: docs-only / internal helpers — construct_runtime frozen
        notes = "Internal renames + docs; construct_runtime names/indices frozen" if name == "runtime" else ""
        shards.append(shard(f"w1-{name}", 1, "discoverability", files, notes))

    # precompiles split by file size into two shards
    pre_files = rs_files(ROOT / "precompiles")
    mid = (len(pre_files) + 1) // 2
    shards.append(
        shard(
            "w1-precompiles-a",
            1,
            "discoverability",
            pre_files[:mid],
            "INDEX values and #[precompile::public] selectors frozen",
        )
    )
    shards.append(
        shard(
            "w1-precompiles-b",
            1,
            "discoverability",
            pre_files[mid:],
            "INDEX values and #[precompile::public] selectors frozen",
        )
    )

    # Wave 2 — pallet-subtensor by subtree
    st = ROOT / "pallets" / "subtensor"
    subtrees = [
        "coinbase",
        "epoch",
        "staking",
        "subnets",
        "swap",
        "rpc_info",
        "utils",
        "guards",
        "extensions",
        "benchmarks",
    ]
    for sub in subtrees:
        files = rs_files(st / "src" / sub)
        shards.append(
            shard(
                f"w2-src-{sub}",
                2,
                "discoverability",
                files,
                f"pallet-subtensor src/{sub}",
            )
        )

    # rpc + runtime-api crates
    shards.append(
        shard(
            "w2-rpc",
            2,
            "discoverability",
            rs_files(st / "rpc") + rs_files(st / "runtime-api"),
            "RPC method strings and runtime API trait/method names frozen",
        )
    )

    # docs-only frozen surfaces
    frozen_docs = [
        ("w2-docs-storage", [ "pallets/subtensor/src/lib.rs" ], "DOCS ONLY on #[pallet::storage] items; do not rename types"),
        ("w2-docs-dispatches", [ "pallets/subtensor/src/macros/dispatches.rs" ], "DOCS ONLY; call names and call_index frozen"),
        ("w2-docs-events", [ "pallets/subtensor/src/macros/events.rs" ], "DOCS ONLY; variant order and names frozen"),
        ("w2-docs-errors", [ "pallets/subtensor/src/macros/errors.rs" ], "DOCS ONLY; variant order and names frozen"),
        (
            "w2-docs-migrations",
            [p.relative_to(ROOT).as_posix() for p in sorted((st / "src" / "migrations").rglob("*.rs"))],
            "DOCS ONLY; migration name strings frozen",
        ),
        (
            "w2-docs-macros-other",
            [
                p.relative_to(ROOT).as_posix()
                for p in sorted((st / "src" / "macros").glob("*.rs"))
                if p.name not in {"dispatches.rs", "events.rs", "errors.rs"}
            ],
            "Docs + safe internal renames in remaining macros",
        ),
    ]
    for sid, files, notes in frozen_docs:
        shards.append(shard(sid, 2, "docs-only" if "DOCS ONLY" in notes else "discoverability", files, notes))

    # Giant test files — one shard each for the biggest
    tests_dir = st / "src" / "tests"
    test_files = sorted(tests_dir.glob("*.rs"), key=lambda p: p.stat().st_size, reverse=True)
    # Top giants get their own shard; remainder bundled
    giants = []
    remainder = []
    for p in test_files:
        if p.name == "mod.rs":
            continue
        rel = p.relative_to(ROOT).as_posix()
        lines = sum(1 for _ in open(p, encoding="utf-8", errors="ignore"))
        if lines >= 2500:
            giants.append((p.stem, [rel], lines))
        else:
            remainder.append(rel)

    for stem, files, lines in giants:
        shards.append(
            shard(
                f"w2-test-{stem}",
                2,
                "split-and-name",
                files,
                f"Split ~{lines}-line test file into concept-named modules under tests/{stem}/",
            )
        )

    # mod.rs + smaller tests
    mod_rel = (tests_dir / "mod.rs").relative_to(ROOT).as_posix()
    shards.append(
        shard(
            "w2-test-remainder",
            2,
            "discoverability",
            [mod_rel] + sorted(remainder),
            "Wire mod.rs after giant splits land; improve smaller test modules",
        )
    )

    # Wave 3
    shards.append(
        shard(
            "w3-rename-queue",
            3,
            "cross-cutting",
            ["refactor/rename-proposals.md"],
            "Process rename-proposals.md serially",
        )
    )
    shards.append(
        shard(
            "w3-glossary",
            3,
            "cross-cutting",
            ["AGENTS.md", ".agents/skills/write-discoverable-code/SKILL.md"],
            "Glossary consistency pass; finalize AGENTS.md",
        )
    )

    # Ownership check: no file in two shards of the same wave
    by_wave: dict[int, dict[str, str]] = {}
    conflicts = []
    for s in shards:
        owned = by_wave.setdefault(s["wave"], {})
        for f in s["files"]:
            if f in owned:
                conflicts.append((s["wave"], f, owned[f], s["id"]))
            else:
                owned[f] = s["id"]
    if conflicts:
        raise SystemExit(f"ownership conflicts: {conflicts[:10]}")

    manifest = {
        "branch": "refactor/discoverability",
        "baseline": "refactor/metadata-baseline.txt",
        "conventions": [
            "AGENTS.md",
            ".agents/skills/write-discoverable-code/SKILL.md",
        ],
        "oracle": "scripts/check_metadata_unchanged.sh",
        "shards": shards,
    }
    out = ROOT / "refactor" / "refactor-manifest.json"
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text(json.dumps(manifest, indent=2) + "\n", encoding="utf-8")
    print(f"wrote {out} ({len(shards)} shards)")
    for w in (1, 2, 3):
        n = sum(1 for s in shards if s["wave"] == w)
        files = sum(len(s["files"]) for s in shards if s["wave"] == w)
        print(f"  wave {w}: {n} shards, {files} files")


if __name__ == "__main__":
    main()
