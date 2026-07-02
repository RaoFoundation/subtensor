#!/usr/bin/env bash
set -euo pipefail

# CI benchmark validation: generate weights, compare with threshold, prepare patch if drifted.
# Exit: 0 = ok, 1 = error, 2 = drift (patch in .bench_patch/)

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"

NODE_BIN="$ROOT_DIR/target/production/node-subtensor"
RUNTIME_WASM="$ROOT_DIR/target/production/wbuild/node-subtensor-runtime/node_subtensor_runtime.compact.compressed.wasm"
TEMPLATE="$ROOT_DIR/.maintain/frame-weight-template.hbs"
WEIGHT_CMP="$ROOT_DIR/target/production/weight-compare"

PATCH_DIR="$ROOT_DIR/.bench_patch"
THRESHOLD="${THRESHOLD:-55}"
STEPS="${STEPS:-50}"
REPEAT="${REPEAT:-20}"

die() { echo "ERROR: $1" >&2; exit 1; }

selective_patch_weights_file() {
    local committed="$1"
    local generated="$2"
    shift 2

    python3 - "$committed" "$generated" "$@" <<'PY_SELECTIVE_PATCH_WEIGHTS'
import dataclasses
import re
import sys
from typing import Dict, List, Optional, Tuple


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

        # normal state
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
            m = re.match(r'r(#+)"|r"', text[i:])
            if m:
                raw_hashes = len(m.group(1) or "")
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


def main() -> int:
    if len(sys.argv) < 4:
        print("usage: selective patch helper <committed> <generated> <benchmark>...", file=sys.stderr)
        return 1

    committed_path = sys.argv[1]
    generated_path = sys.argv[2]
    targets = list(dict.fromkeys(sys.argv[3:]))

    current = open(committed_path, encoding="utf-8").read()
    generated = open(generated_path, encoding="utf-8").read()

    section_specs = [
        ("pub trait WeightInfo", r'pub\s+trait\s+WeightInfo\s*\{', True),
        ("SubstrateWeight impl", r'impl\s*<[^\{]*>\s*WeightInfo\s+for\s+SubstrateWeight\s*<[^\{]*>\s*\{', True),
        ("unit WeightInfo impl", r'impl\s+WeightInfo\s+for\s+\(\)\s*\{', False),
    ]

    total_changes = 0
    for spec in section_specs:
        current, changes = patch_section(current, generated, spec, targets)
        total_changes += changes

    if total_changes == 0:
        missing = ", ".join(targets)
        print(f"no matching generated benchmark functions found for: {missing}", file=sys.stderr)
        return 1

    with open(committed_path, "w", encoding="utf-8") as handle:
        handle.write(current)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
PY_SELECTIVE_PATCH_WEIGHTS
}

# ── Auto-discover pallets ────────────────────────────────────────────────────
declare -A OUTPUTS
while read -r name path; do
  OUTPUTS[$name]="$path"
done < <("$SCRIPT_DIR/discover_pallets.sh")

(( ${#OUTPUTS[@]} > 0 )) || die "no benchmarked pallets found"

# PALLET_DIRS (space-separated directory names under pallets/, set by CI for
# pallet-only diffs) restricts the run to the changed pallets. Unset/empty
# means the full suite.
if [[ -n "${PALLET_DIRS:-}" ]]; then
  declare -A KEEP
  for dir in $PALLET_DIRS; do
    for pallet in "${!OUTPUTS[@]}"; do
      [[ "${OUTPUTS[$pallet]}" == "pallets/$dir/"* ]] && KEEP[$pallet]="${OUTPUTS[$pallet]}"
    done
  done
  if (( ${#KEEP[@]} == 0 )); then
    echo "Changed pallets ($PALLET_DIRS) have no registered benchmarks; nothing to validate."
    exit 0
  fi
  echo "Restricting to changed pallets: ${!KEEP[*]}"
  unset OUTPUTS
  declare -A OUTPUTS
  for pallet in "${!KEEP[@]}"; do OUTPUTS[$pallet]="${KEEP[$pallet]}"; done
fi

mkdir -p "$PATCH_DIR"

# Build if needed
[[ -x "$NODE_BIN" ]] || cargo build --profile production -p node-subtensor --features runtime-benchmarks
cargo build --profile production -p subtensor-weight-tools --bin weight-compare
[[ -x "$NODE_BIN" ]] || die "node binary not found"
[[ -f "$RUNTIME_WASM" ]] || die "runtime WASM not found"
[[ -x "$WEIGHT_CMP" ]] || die "weight-compare not found"

PATCHED=()
SUMMARY=()
FAILED=0

for pallet in "${!OUTPUTS[@]}"; do
  output="${OUTPUTS[$pallet]}"
  committed="$ROOT_DIR/$output"
  tmp=$(mktemp)

  echo ""
  echo "════ $pallet ════"

  if ! "$NODE_BIN" benchmark pallet \
    --runtime="$RUNTIME_WASM" \
    --genesis-builder=runtime \
    --genesis-builder-preset=benchmark \
    --wasm-execution=compiled \
    --pallet="$pallet" \
    --extrinsic="*" \
    --steps="$STEPS" \
    --repeat="$REPEAT" \
    --no-storage-info \
    --no-min-squares \
    --no-median-slopes \
    --output="$tmp" \
    --template="$TEMPLATE" 2>&1; then
    SUMMARY+=("$pallet: FAILED"); FAILED=1; rm -f "$tmp"; continue
  fi

  if [[ ! -f "$committed" ]]; then
    cp "$tmp" "$committed"; PATCHED+=("$output"); SUMMARY+=("$pallet: NEW")
  else
    compare_log=$(mktemp)
    if "$WEIGHT_CMP" --old "$committed" --new "$tmp" --threshold "$THRESHOLD" 2>&1 | tee "$compare_log"; then
        rc=0
    else
        rc=${PIPESTATUS[0]}
    fi

    if (( rc == 2 )); then
        drifted_benchmarks=()
        while IFS= read -r benchmark_name; do
            drifted_benchmarks+=("$benchmark_name")
        done < <(python3 - "$compare_log" <<'PY_DRIFTED_BENCHMARKS'
import sys

seen = set()
with open(sys.argv[1], encoding="utf-8", errors="replace") as handle:
    for line in handle:
        if "❌" not in line:
            continue
        parts = line.split()
        if len(parts) < 2:
            continue
        name = parts[1]
        if name in seen:
            continue
        seen.add(name)
        print(name)
PY_DRIFTED_BENCHMARKS
        )

        if (( ${#drifted_benchmarks[@]} == 0 )); then
            SUMMARY+=("$pallet: COMPARE FAILED"); FAILED=1
        else
            selective_patch_weights_file "$committed" "$tmp" "${drifted_benchmarks[@]}"
            PATCHED+=("$output")
            SUMMARY+=("$pallet: UPDATED ${#drifted_benchmarks[@]} benchmark(s): ${drifted_benchmarks[*]}")
        fi
    elif (( rc == 0 )); then
        SUMMARY+=("$pallet: OK")
    else
        SUMMARY+=("$pallet: COMPARE FAILED"); FAILED=1
    fi
    rm -f "$compare_log"
  fi
  rm -f "$tmp"
done

echo ""; printf '%s\n' "${SUMMARY[@]}"

(( FAILED )) && { printf '%s\n' "${SUMMARY[@]}" > "$PATCH_DIR/summary.txt"; exit 1; }
(( ${#PATCHED[@]} == 0 )) && { echo "All weights within tolerance."; exit 0; }

# Prepare patch
cd "$ROOT_DIR"
git add "${PATCHED[@]}"
{ echo "Head SHA: $(git rev-parse HEAD)"; echo ""; printf '%s\n' "${SUMMARY[@]}"; echo ""; git diff --cached --stat; } > "$PATCH_DIR/summary.txt"
git diff --cached --binary > "$PATCH_DIR/benchmark_patch.diff"
git reset HEAD -- "${PATCHED[@]}" >/dev/null 2>&1 || true
echo "Patch ready at $PATCH_DIR/benchmark_patch.diff — add 'apply-benchmark-patch' label to apply."
exit 2
