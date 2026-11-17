"""Packaging and import diagnostics enforce the shared-policy backend floor."""

import importlib.util
import json
import sys
from pathlib import Path
from types import ModuleType

import pytest
from packaging.requirements import Requirement
from packaging.version import Version

try:
    import tomllib
except ModuleNotFoundError:
    import tomli as tomllib  # pytest's TOML dependency on Python 3.10

SDK = Path(__file__).resolve().parents[2]


def load_policy(monkeypatch, backend):
    monkeypatch.setitem(sys.modules, "bittensor_core", backend)
    spec = importlib.util.spec_from_file_location(
        "_policy_backend_check", SDK / "bittensor/_policy.py"
    )
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


# Package resolution must exclude cores that cannot import the SDK.
# PROTECTED_REGRESSION: changes require explicit human approval.
def test_python_dependency_excludes_cores_before_shared_policy_release():
    project = tomllib.loads((SDK / "pyproject.toml").read_text())["project"]
    core = tomllib.loads((SDK.parent / "bittensor-core-py/pyproject.toml").read_text())["project"]
    requirement = next(
        Requirement(item) for item in project["dependencies"] if item.startswith("bittensor-core")
    )
    assert Version("0.1.10") not in requirement.specifier
    assert Version("0.1.11") not in requirement.specifier
    assert Version(core["version"]) in requirement.specifier
    assert Version("0.2.0") not in requirement.specifier


@pytest.mark.parametrize("export", [None, 42])
def test_incompatible_policy_backend_has_actionable_import_error(monkeypatch, export):
    backend = ModuleType("bittensor_core")
    if export is not None:
        backend.policy_evaluate = export
    with pytest.raises(ImportError, match=r"bittensor-core>=0\.1\.12.*shared Rust policy binding"):
        load_policy(monkeypatch, backend)


def test_supported_policy_backend_preserves_native_results_and_errors(monkeypatch):
    backend = ModuleType("bittensor_core")
    calls = []

    def evaluate(operation, payload):
        calls.append((operation, json.loads(payload)))
        return json.dumps({"version": 1, "value": "9007199254740993"})

    backend.policy_evaluate = evaluate
    policy = load_policy(monkeypatch, backend)
    assert policy.evaluate_policy("example", {"amount": 2**53 + 1}) == "9007199254740993"
    assert calls == [("example", {"amount": "9007199254740993"})]
    backend.policy_evaluate = lambda *_: json.dumps(
        {"version": 1, "error": {"kind": "range", "message": "invalid proportion"}}
    )
    with pytest.raises(ValueError, match="invalid proportion"):
        policy.evaluate_policy("example", {})
