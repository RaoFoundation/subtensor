"""bittensor.sp_core against a native core that predates its newest helpers.

The module is loaded from source over a stub ``bittensor_core``, so these
tests need no native build and leave the real ``bittensor.sp_core`` alone.
"""

from __future__ import annotations

import importlib.util
import sys
import types
from pathlib import Path

import pytest

ALICE = "5GrwvaEF5zXb26Fz9rcQpDWS57CtERHpNehXCPcNoHGKutQY"
NEW_HELPER_ARGS = {
    "verify_strict": (b"message", bytes(64), ALICE),
    "encode_password_for_environment": ("WALLET_PASSWORD", "password"),
}


def _stub_backend(missing: tuple[str, ...]) -> types.ModuleType:
    backend = types.ModuleType("bittensor_core")

    def __getattr__(name: str) -> object:
        if name in missing or name.startswith("__"):
            raise AttributeError(name)
        value = object()
        setattr(backend, name, value)
        return value

    backend.__getattr__ = __getattr__
    return backend


def _load_sp_core(monkeypatch: pytest.MonkeyPatch, backend: types.ModuleType) -> types.ModuleType:
    package = importlib.util.find_spec("bittensor")
    assert package is not None and package.submodule_search_locations
    path = Path(package.submodule_search_locations[0]) / "sp_core.py"
    monkeypatch.setitem(sys.modules, "bittensor_core", backend)
    spec = importlib.util.spec_from_file_location("_sp_core_with_stub_backend", path)
    assert spec is not None and spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


@pytest.mark.parametrize(
    "missing",
    [
        ("verify_strict",),
        ("encode_password_for_environment",),
        ("verify_strict", "encode_password_for_environment"),
    ],
)
def test_import_survives_core_without_new_helpers(
    monkeypatch: pytest.MonkeyPatch, missing: tuple[str, ...]
) -> None:
    backend = _stub_backend(missing)
    sp_core = _load_sp_core(monkeypatch, backend)

    assert sp_core.verify is backend.verify
    assert sp_core.get_password_from_environment is backend.get_password_from_environment
    for name, args in NEW_HELPER_ARGS.items():
        if name not in missing:
            assert getattr(sp_core, name) is getattr(backend, name)
            continue
        with pytest.raises(
            RuntimeError, match=rf"bittensor\.sp_core\.{name} requires bittensor-core>=0\.1\.10"
        ):
            getattr(sp_core, name)(*args)


def test_new_helpers_resolve_to_native_names_when_present(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    backend = _stub_backend(())
    sp_core = _load_sp_core(monkeypatch, backend)

    assert set(sp_core._CORE_HELPERS_SINCE) == set(NEW_HELPER_ARGS)
    for name in NEW_HELPER_ARGS:
        assert getattr(sp_core, name) is getattr(backend, name)
