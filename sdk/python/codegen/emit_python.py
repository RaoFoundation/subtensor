"""Emit the Python wire layer (``bittensor/_generated/``) from the metadata IR.

Two artifacts:

- ``errors.py``: an exhaustive ``(pallet_index, error_index) -> ErrorInfo`` catalog
  of every chain error with its docs. It backs the ``--names`` coverage gate (which
  asserts the SDK's name->code table stays valid across runtime upgrades) and serves
  as the agent-facing error reference.
- ``calls.py``: a typed builder per extrinsic that returns ``(module, function,
  params)``, so intents never hand-transcribe pallet/call/param names.

Output is deterministic (sorted) so the CI regenerate-and-diff gate is stable.
"""

from __future__ import annotations

import keyword
from pathlib import Path

from .metadata import MetadataIR

_HEADER = '''"""Generated from runtime metadata by codegen. DO NOT EDIT BY HAND.

Regenerate with: python -m codegen <ws-endpoint>
Spec version: {spec_version}
"""
'''

# Header variant with a one-line description folded into the module docstring,
# so descriptor files stay a single docstring followed by imports (E402-clean).
_HEADER_WITH_DOC = '''"""Generated from runtime metadata by codegen. DO NOT EDIT BY HAND.

Regenerate with: python -m codegen <ws-endpoint>
Spec version: {spec_version}

{doc}
"""
'''


def _py_name(name: str) -> str:
    """Make a chain identifier a safe Python identifier."""
    return f"{name}_" if keyword.iskeyword(name) else name


def emit_errors(ir: MetadataIR) -> str:
    lines = [_HEADER.format(spec_version=ir.spec_version)]
    lines.append("from dataclasses import dataclass\n\n")
    lines.append("@dataclass(frozen=True)\n")
    lines.append("class ErrorInfo:\n")
    lines.append("    pallet: str\n")
    lines.append("    name: str\n")
    lines.append("    docs: str\n\n\n")
    lines.append("ERRORS: dict[tuple[int, int], ErrorInfo] = {\n")
    for pallet in sorted(ir.pallets, key=lambda p: p.index):
        for error in pallet.errors:
            key = f"({pallet.index}, {error.index})"
            lines.append(
                f"    {key}: ErrorInfo({pallet.name!r}, {error.name!r}, {error.docs!r}),\n"
            )
    lines.append("}\n")
    return "".join(lines)


def emit_calls(ir: MetadataIR) -> str:
    lines = [_HEADER.format(spec_version=ir.spec_version)]
    lines.append("from typing import Any, NamedTuple\n\n\n")
    lines.append("class Call(NamedTuple):\n")
    lines.append('    """A composed call target: (module, function, params).\n\n')
    lines.append("    A typed 3-tuple, so calls are trivially inspectable and testable.\n")
    lines.append('    """\n\n')
    lines.append("    module: str\n")
    lines.append("    function: str\n")
    lines.append("    params: dict[str, Any]\n\n\n")

    for pallet in sorted(ir.pallets, key=lambda p: p.index):
        if not pallet.calls:
            continue
        lines.append(f"class {pallet.name}:\n")
        lines.append(f'    """Call builders for the {pallet.name} pallet."""\n\n')
        for call in sorted(pallet.calls, key=lambda c: c.name):
            params = [_py_name(a) for a in call.args]
            sig = ", ".join(params)
            lines.append("    @staticmethod\n")
            lines.append(f"    def {_py_name(call.name)}({sig}) -> Call:\n")
            if call.docs:
                lines.append(f"        {call.docs!r}\n")
            param_dict = ", ".join(f"{a!r}: {_py_name(a)}" for a in call.args)
            lines.append(
                f"        return Call({pallet.name!r}, {call.name!r}, {{{param_dict}}})\n\n"
            )
        lines.append("\n")
    return "".join(lines)


def _emit_item_classes(
    ir: MetadataIR,
    header_doc: str,
    item_class: str,
    groups: list[tuple[str, list[str]]],
) -> str:
    """Shared emitter for descriptor files: per-group classes of (container, name) tuples."""
    lines = [_HEADER_WITH_DOC.format(spec_version=ir.spec_version, doc=header_doc)]
    lines.append("from typing import NamedTuple\n\n\n")
    lines.append(f"class {item_class}(NamedTuple):\n")
    lines.append('    """A (container, name) pair; unpack into query/constant calls."""\n\n')
    lines.append("    container: str\n")
    lines.append("    name: str\n\n\n")
    for group_name, names in groups:
        if not names:
            continue
        lines.append(f"class {group_name}:\n")
        for name in names:
            lines.append(f"    {_py_name(name)} = {item_class}({group_name!r}, {name!r})\n")
        lines.append("\n")
    return "".join(lines)


def emit_storage(ir: MetadataIR) -> str:
    groups = [
        (pallet.name, list(pallet.storage)) for pallet in sorted(ir.pallets, key=lambda p: p.index)
    ]
    return _emit_item_classes(
        ir, "Storage item descriptors: unpack into substrate.query/query_map.", "Item", groups
    )


def emit_constants(ir: MetadataIR) -> str:
    groups = [
        (pallet.name, list(pallet.constants))
        for pallet in sorted(ir.pallets, key=lambda p: p.index)
    ]
    return _emit_item_classes(
        ir, "Pallet constant descriptors: unpack into substrate.constant.", "Item", groups
    )


def emit_runtime_apis(ir: MetadataIR) -> str:
    groups = [
        (api.name, list(api.methods)) for api in sorted(ir.runtime_apis, key=lambda a: a.name)
    ]
    return _emit_item_classes(
        ir, "Runtime API method descriptors: unpack into substrate.runtime_call.", "Method", groups
    )


def artifacts(ir: MetadataIR) -> dict[str, str]:
    """Filename -> content for every generated file. Single source for write + drift check."""
    return {
        "__init__.py": '"""Generated wire layer. DO NOT EDIT BY HAND."""\n',
        "errors.py": emit_errors(ir),
        "calls.py": emit_calls(ir),
        "storage.py": emit_storage(ir),
        "constants.py": emit_constants(ir),
        "runtime_apis.py": emit_runtime_apis(ir),
    }


def write(ir: MetadataIR, out_dir: Path) -> list[Path]:
    out_dir.mkdir(parents=True, exist_ok=True)
    written = []
    for filename, content in artifacts(ir).items():
        path = out_dir / filename
        path.write_text(content)
        written.append(path)
    return written
