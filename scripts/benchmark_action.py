#!/usr/bin/env python3
"""Helpers for scripts/benchmark_action.sh."""

from __future__ import annotations

import csv
import dataclasses
import os
import re
import sys
from typing import Dict, List, Optional, Tuple


SECTION_SPECS = [
    ("pub trait WeightInfo", r'pub\s+trait\s+WeightInfo\s*\{', True),
    ("SubstrateWeight impl", r'impl\s*<[^\{]*>\s*WeightInfo\s+for\s+SubstrateWeight\s*<[^\{]*>\s*\{', True),
    ("unit WeightInfo impl", r'impl\s+WeightInfo\s+for\s+\(\)\s*\{', False),
]


def parse_selectors(value: str, label: str, allow_hyphens: bool = False) -> List[str]:
    """Normalize explicit names without accepting shell patterns or partial names."""
    if not value.strip():
        return ["*"]
    names = [name for name in re.split(r"[,\s]+", value.strip()) if name]
    if names == ["*"]:
        return names
    pattern = r"[A-Za-z_][A-Za-z0-9_-]*" if allow_hyphens else r"[A-Za-z_][A-Za-z0-9_]*"
    if not names or any(re.fullmatch(pattern, name) is None for name in names):
        raise ValueError(f"invalid {label}: use exact names separated by commas or whitespace, or '*' alone")
    return sorted(set(names))


def select_pallets(args: List[str]) -> int:
    if args:
        raise ValueError("usage: benchmark_action.py select-pallets < discovery-output")

    pallets = {}
    aliases: Dict[str, List[str]] = {}
    for line in sys.stdin:
        if not line.strip():
            continue
        fields = line.split()
        if len(fields) != 2 or re.fullmatch(r"[A-Za-z_][A-Za-z0-9_]*", fields[0]) is None:
            raise ValueError(f"invalid pallet discovery row: {line.strip()!r}")
        pallet, path = fields
        match = re.fullmatch(r"pallets/([A-Za-z0-9_-]+)/src/weights\.rs", path)
        if match is None or pallet in pallets:
            raise ValueError(f"invalid or duplicate pallet discovery row: {line.strip()!r}")
        pallets[pallet] = path
        for alias in (pallet, match.group(1)):
            aliases.setdefault(alias, []).append(pallet)
    if not pallets:
        raise ValueError("no registered benchmarks found in pallet discovery")

    requested = parse_selectors(os.environ.get("PALLET_DIRS", ""), "pallet selection", allow_hyphens=True)
    extrinsics = parse_selectors(os.environ.get("EXTRINSICS", ""), "extrinsic selection")
    if requested == ["*"]:
        selected = set(pallets)
    else:
        unknown = [name for name in requested if name not in aliases]
        if unknown:
            raise ValueError(f"Selected pallets ({', '.join(unknown)}) have no registered benchmarks")
        selected = {pallet for name in requested for pallet in aliases[name]}
    for pallet in sorted(selected):
        print(pallet, pallets[pallet], ",".join(extrinsics), sep="\t")
    return 0


def select_benchmarks(args: List[str]) -> int:
    if len(args) != 2:
        raise ValueError("usage: benchmark_action.py select-benchmarks <selection.tsv> <available.csv>")

    selection = {}
    with open(args[0], encoding="utf-8") as source:
        for row in csv.reader(source, delimiter="\t"):
            if not row:
                continue
            if len(row) != 3 or re.fullmatch(r"[A-Za-z_][A-Za-z0-9_]*", row[0]) is None:
                raise ValueError(f"invalid pallet selection row: {row!r}")
            pallet, path, selectors = row
            if pallet in selection or re.fullmatch(r"pallets/[A-Za-z0-9_-]+/src/weights\.rs", path) is None:
                raise ValueError(f"invalid or duplicate pallet selection row: {row!r}")
            selection[pallet] = (path, parse_selectors(selectors, "extrinsic selection"))
    if not selection:
        raise ValueError("no pallets selected")

    available: Dict[str, set] = {}
    with open(args[1], encoding="utf-8") as source:
        for row in csv.reader(source, skipinitialspace=True):
            if not row:
                continue
            row = [field.strip() for field in row]
            if len(row) != 2 or any(re.fullmatch(r"[A-Za-z_][A-Za-z0-9_]*", field) is None for field in row):
                raise ValueError(f"invalid runtime benchmark listing row: {row!r}")
            pallet, extrinsic = row
            available.setdefault(pallet, set()).add(extrinsic)
    if not available:
        raise ValueError("runtime benchmark registry is empty")
    absent = sorted(set(selection) - set(available))
    if absent:
        raise ValueError(f"Selected pallets ({', '.join(absent)}) have no registered benchmarks in this runtime")

    requested = {name for _, names in selection.values() for name in names if name != "*"}
    supported = {name for pallet in selection for name in available[pallet]}
    unknown = sorted(requested - supported)
    if unknown:
        raise ValueError(f"Unknown extrinsics in selected pallets: {', '.join(unknown)}")

    rows = []
    for pallet, (path, names) in sorted(selection.items()):
        matched = ["*"] if names == ["*"] else sorted(set(names) & available[pallet])
        if matched:
            rows.append((pallet, path, ",".join(matched)))
        else:
            print(f"Skipping {pallet}: no selected extrinsics", file=sys.stderr)
    if not rows:
        raise ValueError("no benchmarks match the requested selection")
    for row in rows:
        print(*row, sep="\t")
    return 0


@dataclasses.dataclass
class RustItem:
    name: str
    start: int
    end: int
    text: str


@dataclasses.dataclass
class RustSection:
    name: str
    start: int
    body_start: int
    body_end: int
    end: int
    items: List[RustItem]


def find_matching_brace(text: str, open_index: int) -> int:
    depth = 0
    i = open_index
    n = len(text)
    state = "normal"
    raw_hashes = 0

    while i < n:
        ch = text[i]
        nxt = text[i + 1] if i + 1 < n else ""

        if state == "line_comment":
            if ch == "\n":
                state = "normal"
            i += 1
            continue

        if state == "block_comment":
            if ch == "*" and nxt == "/":
                state = "normal"
                i += 2
            else:
                i += 1
            continue

        if state == "string":
            if ch == "\\":
                i += 2
            elif ch == '"':
                state = "normal"
                i += 1
            else:
                i += 1
            continue

        if state == "char":
            if ch == "\\":
                i += 2
            elif ch == "'":
                state = "normal"
                i += 1
            else:
                i += 1
            continue

        if state == "raw_string":
            if ch == '"' and text.startswith("#" * raw_hashes, i + 1):
                i += 1 + raw_hashes
                state = "normal"
            else:
                i += 1
            continue

        if ch == "/" and nxt == "/":
            state = "line_comment"
            i += 2
            continue
        if ch == "/" and nxt == "*":
            state = "block_comment"
            i += 2
            continue
        if ch == '"':
            state = "string"
            i += 1
            continue
        if ch == "'":
            state = "char"
            i += 1
            continue
        if ch == "r":
            match = re.match(r'r(#+)"|r"', text[i:])
            if match:
                raw_hashes = len(match.group(1) or "")
                state = "raw_string"
                i += 2 + raw_hashes
                continue
        if ch == "{":
            depth += 1
        elif ch == "}":
            depth -= 1
            if depth == 0:
                return i
        i += 1

    raise ValueError(f"unmatched '{{' at byte offset {open_index}")


def find_signature_terminator(text: str, start: int) -> Tuple[str, int]:
    paren_depth = 0
    square_depth = 0
    i = start
    n = len(text)
    state = "normal"

    while i < n:
        ch = text[i]
        nxt = text[i + 1] if i + 1 < n else ""

        if state == "line_comment":
            if ch == "\n":
                state = "normal"
            i += 1
            continue
        if state == "block_comment":
            if ch == "*" and nxt == "/":
                state = "normal"
                i += 2
            else:
                i += 1
            continue
        if state == "string":
            if ch == "\\":
                i += 2
            elif ch == '"':
                state = "normal"
                i += 1
            else:
                i += 1
            continue
        if state == "char":
            if ch == "\\":
                i += 2
            elif ch == "'":
                state = "normal"
                i += 1
            else:
                i += 1
            continue

        if ch == "/" and nxt == "/":
            state = "line_comment"
            i += 2
            continue
        if ch == "/" and nxt == "*":
            state = "block_comment"
            i += 2
            continue
        if ch == '"':
            state = "string"
            i += 1
            continue
        if ch == "'":
            state = "char"
            i += 1
            continue

        if ch == "(":
            paren_depth += 1
        elif ch == ")":
            paren_depth -= 1
        elif ch == "[":
            square_depth += 1
        elif ch == "]":
            square_depth -= 1
        elif paren_depth == 0 and square_depth == 0 and ch in "{;":
            return ch, i
        i += 1

    raise ValueError(f"could not find function terminator after byte offset {start}")


def include_leading_docs(text: str, lower_bound: int, fn_start: int) -> int:
    item_start = text.rfind("\n", 0, fn_start) + 1
    while item_start > lower_bound:
        prev_newline = text.rfind("\n", lower_bound, item_start - 1)
        prev_start = lower_bound if prev_newline == -1 else prev_newline + 1
        line = text[prev_start:item_start]
        stripped = line.strip()
        if stripped == "" or stripped.startswith("///") or stripped.startswith("#["):
            item_start = prev_start
            continue
        break
    return item_start


def include_trailing_newline(text: str, item_end: int, upper_bound: int) -> int:
    if item_end < upper_bound and text[item_end:item_end + 1] == "\n":
        return item_end + 1
    return item_end


def parse_items(text: str, body_start: int, body_end: int) -> List[RustItem]:
    items: List[RustItem] = []
    pattern = re.compile(r'(?m)^[ \t]*(?:pub(?:\([^)]*\))?\s+)?fn\s+([A-Za-z_][A-Za-z0-9_]*)\s*\(')

    for match in pattern.finditer(text, body_start, body_end):
        name = match.group(1)
        item_start = include_leading_docs(text, body_start, match.start())
        terminator, terminator_at = find_signature_terminator(text, match.start())
        if terminator == ";":
            item_end = terminator_at + 1
        else:
            item_end = find_matching_brace(text, terminator_at) + 1
        item_end = include_trailing_newline(text, item_end, body_end)
        items.append(RustItem(name=name, start=item_start, end=item_end, text=text[item_start:item_end]))

    return items


def find_section(text: str, name: str, pattern: str, required: bool = True) -> Optional[RustSection]:
    match = re.search(pattern, text, re.S)
    if not match:
        if required:
            raise ValueError(f"could not find {name} section")
        return None
    open_index = text.find("{", match.start(), match.end())
    if open_index == -1:
        raise ValueError(f"could not find opening brace for {name} section")
    close_index = find_matching_brace(text, open_index)
    body_start = open_index + 1
    body_end = close_index
    return RustSection(
        name=name,
        start=match.start(),
        body_start=body_start,
        body_end=body_end,
        end=close_index + 1,
        items=parse_items(text, body_start, body_end),
    )


def patch_section(current_text: str, generated_text: str, spec: Tuple[str, str, bool], targets: List[str]) -> Tuple[str, int]:
    section_name, pattern, required = spec
    old_section = find_section(current_text, section_name, pattern, required=required)
    new_section = find_section(generated_text, section_name, pattern, required=required)
    if old_section is None or new_section is None:
        return current_text, 0

    old_by_name: Dict[str, RustItem] = {item.name: item for item in old_section.items}
    new_by_name: Dict[str, RustItem] = {item.name: item for item in new_section.items}
    old_live_names = {
        item.name
        for item in old_section.items
        if not (item.name in targets and item.name not in new_by_name)
    }

    inserts_before: Dict[str, List[str]] = {}
    inserts_after: Dict[str, List[str]] = {}
    tail_inserts: List[str] = []

    new_order = [item.name for item in new_section.items]
    for idx, name in enumerate(new_order):
        if name not in targets or name in old_by_name:
            continue
        next_anchor = None
        for later in new_order[idx + 1:]:
            if later in old_live_names:
                next_anchor = later
                break
        if next_anchor is not None:
            inserts_before.setdefault(next_anchor, []).append(name)
            continue
        prev_anchor = None
        for earlier in reversed(new_order[:idx]):
            if earlier in old_live_names:
                prev_anchor = earlier
                break
        if prev_anchor is not None:
            inserts_after.setdefault(prev_anchor, []).append(name)
        else:
            tail_inserts.append(name)

    out: List[str] = []
    cursor = old_section.body_start
    changed = 0

    for item in old_section.items:
        out.append(current_text[cursor:item.start])
        for inserted in inserts_before.get(item.name, []):
            out.append(new_by_name[inserted].text)
            changed += 1

        if item.name in targets:
            replacement = new_by_name.get(item.name)
            if replacement is not None:
                out.append(replacement.text)
                if replacement.text != item.text:
                    changed += 1
            else:
                changed += 1
        else:
            out.append(item.text)

        for inserted in inserts_after.get(item.name, []):
            out.append(new_by_name[inserted].text)
            changed += 1
        cursor = item.end

    for inserted in tail_inserts:
        out.append(new_by_name[inserted].text)
        changed += 1
    out.append(current_text[cursor:old_section.body_end])

    new_body = "".join(out)
    if new_body == current_text[old_section.body_start:old_section.body_end]:
        return current_text, 0

    return (
        current_text[:old_section.body_start]
        + new_body
        + current_text[old_section.body_end:],
        changed,
    )


def selective_patch_weights_file(args: List[str]) -> int:
    if len(args) < 3:
        print("usage: benchmark_action.py selective-patch <committed> <generated> <benchmark>...", file=sys.stderr)
        return 1

    committed_path = args[0]
    generated_path = args[1]
    targets = list(dict.fromkeys(args[2:]))

    current = open(committed_path, encoding="utf-8").read()
    generated = open(generated_path, encoding="utf-8").read()

    known_names = set()
    for text in (current, generated):
        section = find_section(text, *SECTION_SPECS[0])
        known_names.update(item.name for item in section.items)
    missing = sorted(set(targets) - known_names)
    if missing:
        raise ValueError(f"no matching benchmark functions found for: {', '.join(missing)}")

    for spec in SECTION_SPECS:
        current, _ = patch_section(current, generated, spec, targets)

    with open(committed_path, "w", encoding="utf-8") as handle:
        handle.write(current)
    return 0


def merge_measured_weights_file(args: List[str]) -> int:
    if len(args) != 3:
        raise ValueError("usage: benchmark_action.py merge-measured <committed> <generated> <csv-names>")
    committed_path, generated_path, names = args
    targets = parse_selectors(names, "measured benchmark selection")
    if targets == ["*"]:
        raise ValueError("merge-measured requires explicit benchmark names")

    with open(committed_path, encoding="utf-8") as source:
        current = source.read()
    with open(generated_path, encoding="utf-8") as source:
        generated = source.read()

    # Validate every generated implementation before writing anything. A partial
    # reference file must contain all and only the requested measurements.
    expected = set(targets)
    for section_name, pattern, required in SECTION_SPECS:
        old_section = find_section(current, section_name, pattern, required=required)
        new_section = find_section(generated, section_name, pattern, required=required or old_section is not None)
        if new_section is None:
            continue
        actual = [item.name for item in new_section.items]
        if len(actual) != len(set(actual)):
            raise ValueError(f"duplicate benchmark functions in generated {section_name}")
        if set(actual) != expected:
            missing = sorted(expected - set(actual))
            extra = sorted(set(actual) - expected)
            details = []
            if missing:
                details.append(f"missing {', '.join(missing)}")
            if extra:
                details.append(f"unexpected {', '.join(extra)}")
            raise ValueError(f"generated {section_name} does not match selected benchmarks: {'; '.join(details)}")

    for spec in SECTION_SPECS:
        current, _ = patch_section(current, generated, spec, targets)
    with open(committed_path, "w", encoding="utf-8") as destination:
        destination.write(current)
    return 0


def print_drifted_benchmarks(args: List[str]) -> int:
    if len(args) != 1:
        print("usage: benchmark_action.py drifted-benchmarks <compare-log>", file=sys.stderr)
        return 1

    seen = set()
    with open(args[0], encoding="utf-8", errors="replace") as handle:
        for line in handle:
            if "\u274c" not in line:
                continue
            parts = line.split()
            if len(parts) < 2:
                continue
            name = parts[1]
            if name in seen:
                continue
            seen.add(name)
            print(name)
    return 0


def main() -> int:
    commands = {
        "select-pallets": select_pallets,
        "select-benchmarks": select_benchmarks,
        "merge-measured": merge_measured_weights_file,
        "selective-patch": selective_patch_weights_file,
        "drifted-benchmarks": print_drifted_benchmarks,
    }
    if len(sys.argv) < 2:
        print(f"usage: benchmark_action.py <{'|'.join(commands)}> ...", file=sys.stderr)
        return 1

    command = sys.argv[1]
    if command not in commands:
        print(f"unknown command: {command}", file=sys.stderr)
        return 1
    try:
        return commands[command](sys.argv[2:])
    except (OSError, ValueError, csv.Error) as error:
        print(f"ERROR: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
