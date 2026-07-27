#!/usr/bin/env python3
"""Extract a docs-stripped structural fingerprint of subtensor's frozen surface.

This is the discoverability-migration safety oracle: if the fingerprint matches
the committed baseline, Tier A–C surfaces (storage names, call indices/names,
event/error order+names, construct_runtime, RPC methods, runtime API methods,
precompile indices/selectors) are unchanged. Doc comments are ignored.

Usage:
  python3 scripts/extract_metadata_fingerprint.py
  python3 scripts/extract_metadata_fingerprint.py --write refactor/metadata-baseline.txt
  python3 scripts/extract_metadata_fingerprint.py --check refactor/metadata-baseline.txt
"""

from __future__ import annotations

import argparse
import hashlib
import re
import sys
from difflib import unified_diff
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]

CALL_INDEX_RE = re.compile(
    r"#\[pallet::call_index\((\d+)\)\]\s*(?:#\[[^\]]+\]\s*)*"
    r"(?:pub(?:\([^)]*\))?\s+)?(?:async\s+)?fn\s+(\w+)",
    re.MULTILINE,
)
CONSTRUCT_RUNTIME_RE = re.compile(
    r"construct_runtime!\s*\(\s*pub\s+struct\s+Runtime\s*\{([\s\S]*?)\n\s*\}\s*\)",
    re.MULTILINE,
)
RUNTIME_ENTRY_RE = re.compile(r"^\s*(\w+)\s*:\s*[\w:]+(?:\s*=\s*(\d+))?", re.MULTILINE)
PRECOMPILE_INDEX_RE = re.compile(
    r"(?:const\s+INDEX\s*:\s*u64\s*=\s*(\d+)|H160::from_low_u64_be\((\d+)\))",
)
SOLIDITY_PUBLIC_RE = re.compile(r'#\[precompile::public\("([^"]+)"\)\]')
RPC_METHOD_RE = re.compile(r'#\[method\(name\s*=\s*"([^"]+)"\)\]')
API_TRAIT_FN_RE = re.compile(r"^\s*fn\s+(\w+)\s*\(", re.MULTILINE)


def read(path: Path) -> str:
    try:
        return path.read_text(encoding="utf-8")
    except FileNotFoundError:
        return ""


def strip_line_docs(src: str) -> str:
    out = []
    for line in src.splitlines():
        s = line.lstrip()
        if s.startswith("///") or s.startswith("//!"):
            continue
        if s.startswith("#[doc"):
            continue
        out.append(line)
    return "\n".join(out)


def iter_pallet_rs() -> list[Path]:
    files: list[Path] = []
    pallets = ROOT / "pallets"
    if not pallets.is_dir():
        return files
    for path in sorted(pallets.rglob("*.rs")):
        parts = path.parts
        if "tests" in parts or path.name in {"weights.rs", "benchmarking.rs"}:
            continue
        if "benchmarks" in parts:
            continue
        if "mock.rs" in path.name:
            continue
        files.append(path)
    return files


def collect_storage(paths: list[Path]) -> list[str]:
    """After #[pallet::storage], the next `pub type Name` is the storage item."""
    items: list[str] = []
    type_re = re.compile(r"\b(?:pub(?:\([^)]*\))?\s+)?type\s+(\w+)\s*<")
    for path in paths:
        lines = strip_line_docs(read(path)).splitlines()
        rel = path.relative_to(ROOT).as_posix()
        i = 0
        while i < len(lines):
            if "#[pallet::storage]" in lines[i]:
                j = i + 1
                while j < len(lines):
                    m = type_re.search(lines[j])
                    if m:
                        items.append(f"storage\t{rel}\t{m.group(1)}")
                        break
                    # Stop if we hit another pallet attr without finding a type
                    if lines[j].strip().startswith("#[pallet::") and "storage" not in lines[j]:
                        break
                    j += 1
                    if j > i + 15:
                        break
            i += 1
    return sorted(items)


def collect_calls(paths: list[Path]) -> list[str]:
    items: list[str] = []
    for path in paths:
        text = strip_line_docs(read(path))
        rel = path.relative_to(ROOT).as_posix()
        for idx, name in CALL_INDEX_RE.findall(text):
            items.append(f"call\t{rel}\t{idx}\t{name}")
    return sorted(items)


def collect_pallet_enum(path: Path, kind: str) -> list[str]:
    """Collect Event/Error variant names in declaration order."""
    text = strip_line_docs(read(path))
    marker = f"#[pallet::{kind}]"
    idx = text.find(marker)
    if idx < 0:
        return []
    # Find `enum ... {`
    rest = text[idx:]
    m = re.search(r"\benum\s+\w+[^{]*\{", rest)
    if not m:
        return []
    body_start = idx + m.end()
    # Brace-depth scan to end of enum
    depth = 1
    i = body_start
    while i < len(text) and depth:
        c = text[i]
        if c == "{":
            depth += 1
        elif c == "}":
            depth -= 1
        i += 1
    body = text[body_start : i - 1]
    rel = path.relative_to(ROOT).as_posix()
    items: list[str] = []
    order = 0
    # Variant at line start: Name( or Name,
    # Ignore deeper-indented type args by requiring the line's first token.
    for line in body.splitlines():
        # Skip attribute-only lines
        stripped = line.strip()
        if not stripped or stripped.startswith("#["):
            continue
        vm = re.match(r"^([A-Z][A-Za-z0-9]*)\s*(\(|,|\{|$)", stripped)
        if not vm:
            continue
        items.append(f"{kind}\t{rel}\t{order}\t{vm.group(1)}")
        order += 1
    return items


def collect_enums(paths: list[Path]) -> list[str]:
    items: list[str] = []
    for path in paths:
        items.extend(collect_pallet_enum(path, "event"))
        items.extend(collect_pallet_enum(path, "error"))
    return items


def refine_enum_items(items: list[str]) -> list[str]:
    """Drop false-positive 'variants' that are type args (deeper indent than true variants).

    Re-parse from source with indent tracking for accuracy.
    """
    # Group by (kind, rel)
    by_key: dict[tuple[str, str], list[tuple[int, str]]] = {}
    for line in items:
        kind, rel, order, name = line.split("\t")
        by_key.setdefault((kind, rel), []).append((int(order), name))

    # Re-extract with indent filter
    out: list[str] = []
    for (kind, rel), _ in by_key.items():
        path = ROOT / rel
        text = strip_line_docs(read(path))
        marker = f"#[pallet::{kind}]"
        idx = text.find(marker)
        if idx < 0:
            continue
        rest = text[idx:]
        m = re.search(r"\benum\s+\w+[^{]*\{", rest)
        if not m:
            continue
        body_start = idx + m.end()
        depth = 1
        i = body_start
        while i < len(text) and depth:
            if text[i] == "{":
                depth += 1
            elif text[i] == "}":
                depth -= 1
            i += 1
        body = text[body_start : i - 1]

        candidates: list[tuple[int, str]] = []
        for line in body.splitlines():
            if not line.strip() or line.strip().startswith("#["):
                continue
            # Only consider lines at paren-depth 0 relative to the enum body line.
            # Approximate: count net parens before this line in body — skip if > 0.
            pass

        # Indent-based: find minimum indent among PascalCase lines ending with ( or ,
        raw: list[tuple[int, str]] = []
        for line in body.splitlines():
            m2 = re.match(r"^(\s*)([A-Z][A-Za-z0-9]*)\s*(\(|,|\{)\s*", line)
            if not m2:
                continue
            indent = len(m2.group(1).expandtabs(4))
            raw.append((indent, m2.group(2)))
        if not raw:
            continue
        min_indent = min(i for i, _ in raw)
        order = 0
        for indent, name in raw:
            if indent != min_indent:
                continue
            out.append(f"{kind}\t{rel}\t{order}\t{name}")
            order += 1
    return out


def collect_construct_runtime() -> list[str]:
    text = strip_line_docs(read(ROOT / "runtime/src/lib.rs"))
    m = CONSTRUCT_RUNTIME_RE.search(text)
    if not m:
        return ["construct_runtime\tMISSING"]
    items: list[str] = []
    for name, idx in RUNTIME_ENTRY_RE.findall(m.group(1)):
        items.append(f"runtime\t{name}\t{idx or '?'}")
    return items


def collect_precompiles() -> list[str]:
    """Collect precompile INDEX values and Solidity selectors without source paths.

    Paths are omitted so `foo.rs` → `foo/mod.rs` file splits do not change the
    fingerprint when INDEX / `#[precompile::public]` selectors are unchanged.
    """
    items: list[str] = []
    pre_root = ROOT / "precompiles/src"
    if not pre_root.exists():
        return items
    for path in sorted(pre_root.rglob("*.rs")):
        text = strip_line_docs(read(path))
        for a, b in PRECOMPILE_INDEX_RE.findall(text):
            items.append(f"precompile_index\t{a or b}")
        for sig in SOLIDITY_PUBLIC_RE.findall(text):
            items.append(f"precompile_selector\t{sig}")
    return sorted(items)


def collect_rpc() -> list[str]:
    items: list[str] = []
    for path in sorted(ROOT.glob("pallets/*/rpc/src/**/*.rs")):
        text = strip_line_docs(read(path))
        rel = path.relative_to(ROOT).as_posix()
        for name in RPC_METHOD_RE.findall(text):
            items.append(f"rpc\t{rel}\t{name}")
    return sorted(items)


def collect_runtime_apis() -> list[str]:
    items: list[str] = []
    for path in sorted(ROOT.glob("pallets/*/runtime-api/src/**/*.rs")):
        text = strip_line_docs(read(path))
        rel = path.relative_to(ROOT).as_posix()
        for trait in re.findall(r"pub\s+trait\s+(\w+)", text):
            items.append(f"runtime_api_trait\t{rel}\t{trait}")
        for fn in API_TRAIT_FN_RE.findall(text):
            items.append(f"runtime_api_fn\t{rel}\t{fn}")
    return sorted(items)


def build_fingerprint() -> str:
    sources = iter_pallet_rs()
    lines: list[str] = [
        "# subtensor frozen-surface fingerprint (docs stripped)",
        "# tiers: storage, call, event, error, runtime, precompile, rpc, runtime_api",
    ]
    lines.extend(collect_storage(sources))
    lines.extend(collect_calls(sources))
    raw_enums = collect_enums(sources)
    lines.extend(refine_enum_items(raw_enums))
    lines.extend(collect_construct_runtime())
    lines.extend(collect_precompiles())
    lines.extend(collect_rpc())
    lines.extend(collect_runtime_apis())
    body = "\n".join(lines) + "\n"
    digest = hashlib.sha256(body.encode("utf-8")).hexdigest()
    return f"sha256:{digest}\n\n{body}"


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--write", type=Path, help="Write fingerprint to this path")
    parser.add_argument("--check", type=Path, help="Compare against baseline; exit 1 on mismatch")
    args = parser.parse_args()
    fp = build_fingerprint()

    if args.write:
        args.write.parent.mkdir(parents=True, exist_ok=True)
        args.write.write_text(fp, encoding="utf-8")
        print(f"wrote {args.write}", file=sys.stderr)
        print(fp.splitlines()[0])
        return 0

    if args.check:
        baseline = args.check.read_text(encoding="utf-8")
        if baseline != fp:
            print("METADATA FINGERPRINT MISMATCH", file=sys.stderr)
            print(f"baseline: {baseline.splitlines()[0]}", file=sys.stderr)
            print(f"current:  {fp.splitlines()[0]}", file=sys.stderr)
            for i, line in enumerate(
                unified_diff(
                    baseline.splitlines(),
                    fp.splitlines(),
                    fromfile="baseline",
                    tofile="current",
                    lineterm="",
                )
            ):
                if i >= 80:
                    print("...", file=sys.stderr)
                    break
                print(line, file=sys.stderr)
            return 1
        print(f"OK {fp.splitlines()[0]}")
        return 0

    sys.stdout.write(fp)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
