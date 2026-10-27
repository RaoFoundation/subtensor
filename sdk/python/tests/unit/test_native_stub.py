"""Catch drift between the native module's shipped stubs and PyO3 exports.

Only introspects signatures: no GPU/device, keyfile, network, or mining calls.
The same check runs against either a published wheel or a local native build.
"""

from __future__ import annotations

import ast
import inspect
from pathlib import Path
from types import ModuleType

import bittensor_core
import pytest

STUB = Path(__file__).resolve().parents[3] / "bittensor-core-py" / "bittensor_core.pyi"
TREE = ast.parse(STUB.read_text())
# Executing a stub defines only its declarations; deferred annotations keep
# forward references inert. Signatures can then be compared with inspect.
DECLARATIONS: dict = {}
exec(
    compile("from __future__ import annotations\n" + STUB.read_text(), str(STUB), "exec"),
    DECLARATIONS,
)
NATIVE_CLASSES = {"Keypair", "StorageEntry", "Runtime", "PowGpuMiner", "LedgerDevice"}


def test_stub_covers_every_public_native_export():
    exports = {
        name
        for name, value in vars(bittensor_core).items()
        if not name.startswith("_") and not isinstance(value, ModuleType)
    }
    declared = DECLARATIONS.keys() | DECLARATIONS["__annotations__"].keys()
    assert exports <= declared
    assert isinstance(bittensor_core.__core_version__, str)


def _cases():
    for item in TREE.body:
        if isinstance(item, ast.FunctionDef):
            yield item.name, None
        elif isinstance(item, ast.ClassDef) and item.name in NATIVE_CLASSES:
            for member in item.body:
                if isinstance(member, ast.FunctionDef):
                    yield item.name, member.name


@pytest.mark.parametrize("name,member", list(_cases()))
def test_stub_signature_matches_native(name, member):
    if name == "LedgerDevice" and not hasattr(bittensor_core, name):
        pytest.skip("native wheel was built without the optional ledger feature")
    native = getattr(bittensor_core, name)
    stub = DECLARATIONS[name]
    if member == "__init__":
        stub = stub.__init__
    elif member is not None:
        native = getattr(native, member)
        stub = getattr(stub, member)
        if isinstance(stub, property):
            assert inspect.isdatadescriptor(native)
            return
    expected = inspect.signature(stub)
    actual = inspect.signature(native)
    # PyO3 marks self as positional-only; ordinary .pyi methods need not.
    expected_params = [p for p in expected.parameters.values() if p.name != "self"]
    actual_params = [p for p in actual.parameters.values() if p.name != "self"]
    assert [(p.name, p.kind) for p in expected_params] == [(p.name, p.kind) for p in actual_params]
    for expected_param, actual_param in zip(expected_params, actual_params):
        if actual_param.default is Ellipsis:
            # PyO3 reports Rust constant defaults as ellipsis. It still tells
            # us the argument is optional, but cannot expose its value.
            assert expected_param.default is not inspect.Parameter.empty
        else:
            assert expected_param.default == actual_param.default


@pytest.mark.parametrize("name", sorted(NATIVE_CLASSES))
def test_stub_covers_native_class_members(name):
    if name == "LedgerDevice" and not hasattr(bittensor_core, name):
        pytest.skip("native wheel was built without the optional ledger feature")
    native_members = {n for n in vars(getattr(bittensor_core, name)) if not n.startswith("_")}
    assert native_members <= vars(DECLARATIONS[name]).keys()
