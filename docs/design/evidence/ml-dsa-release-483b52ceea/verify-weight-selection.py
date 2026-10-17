"""Verify imported benchmark formulas, allowing only the removed activation setter."""

from __future__ import annotations

import hashlib
import importlib.util
import json
import re
import subprocess
import sys
from pathlib import Path

ROOT = Path("/home/johnreed/Desktop/work/subtensor")
REFERENCE = Path("/tmp/ms-reference-39060946210")
OUT = Path(__file__).parent / "weight-selection-verification.json"
spec = importlib.util.spec_from_file_location("benchmark_action", ROOT / "scripts/benchmark_action.py")
module = importlib.util.module_from_spec(spec)
sys.modules[spec.name] = module
spec.loader.exec_module(module)
manifest = json.loads((REFERENCE / "selection.json").read_text())
sections = [
    ("trait", r"pub\s+trait\s+WeightInfo\s*\{"),
    ("generic", r"impl\s*<[^\{]*>\s*WeightInfo\s+for\s+SubstrateWeight\s*<[^\{]*>\s*\{"),
    ("fallback", r"impl\s+WeightInfo\s+for\s*\(\)\s*\{"),
]


def sha256(data):
    return hashlib.sha256(data).hexdigest()


def normalized(source):
    source = re.sub(r"//[^\n]*", "", source)
    source = re.sub(r"\s+", "", source)
    return source.replace(",)", ")")


checks = []
files = []
selected_methods = []
for relative, entry in manifest["methods"].items():
    old_path = REFERENCE / f"original-{Path(relative).parts[1]}.rs"
    reference_path = REFERENCE / ".bench_patch/generated" / entry["generated"]
    current_path = ROOT / relative
    original_bytes = old_path.read_bytes()
    reference_bytes = reference_path.read_bytes()
    current_bytes = current_path.read_bytes()
    assert sha256(original_bytes) == entry["original_sha256"], (relative, "original hash mismatch")
    assert sha256(reference_bytes) == entry["generated_sha256"], (relative, "reference hash mismatch")
    old, current, reference = [data.decode() for data in (original_bytes, current_bytes, reference_bytes)]
    removed = {"set_mldsa_enabled"} if "hashed-accounts" in relative else set()
    imported = set(entry["names"]) - removed
    selected_methods.extend({"file": relative, "method": name} for name in sorted(imported))
    files.append(
        {
            "file": relative,
            "original_sha256": sha256(original_bytes),
            "reference_sha256": sha256(reference_bytes),
            "current_sha256": sha256(current_bytes),
            "removed_methods": sorted(removed),
        }
    )
    for section, pattern in sections:
        before, now, ref = [
            {item.name: item.text for item in module.find_section(text, section, pattern).items}
            for text in (old, current, reference)
        ]
        expected_methods = (set(before) | set(entry["names"])) - removed
        assert set(now) == expected_methods, (relative, section, "unexpected methods", set(now) ^ expected_methods)
        assert imported <= set(ref), (relative, section, "reference incomplete")
        assert not (removed & set(now)), (relative, section, "setter remains")
        for method, body in now.items():
            expected = (ref if method in imported else before)[method]
            assert normalized(body) == normalized(expected), (relative, section, method, "formula changed")
            checks.append(
                {
                    "file": relative,
                    "section": section,
                    "method": method,
                    "source": "reference" if method in imported else "unchanged",
                    "normalized_sha256": sha256(normalized(body).encode()),
                }
            )
    assert "Weight::MAX" not in current, (relative, "unmeasured weight remains")

assert len(selected_methods) == 16
report = {
    "passed": True,
    "head": subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT, text=True).strip(),
    "reference_run_id": manifest["run_id"],
    "reference_benchmark_commit": manifest["benchmark_commit"],
    "reference_artifact_sha256": manifest["artifact_sha256"],
    "imported_method_count": len(selected_methods),
    "checked_trait_and_implementation_items": len(checks),
    "removed_method": "pallets/hashed-accounts/src/weights.rs::set_mldsa_enabled",
    "comparison": "Formula token equality after removing comments/whitespace and optional trailing argument commas",
    "scope": "Static comparison only; no benchmark, build, or fee execution performed",
    "conservative_reads": "Reference formulas retain charges for the removed subordinate gate storage reads; no remaining measured formula was reduced or otherwise altered.",
    "files": files,
    "selected_methods": selected_methods,
    "checks": checks,
}
OUT.write_text(json.dumps(report, indent=2) + "\n")
print(json.dumps({key: report[key] for key in ("passed", "head", "imported_method_count", "checked_trait_and_implementation_items", "removed_method")}, indent=2))
print(f"Saved {OUT}")
