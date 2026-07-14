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


# Type idents that annotate directly as builtins.
_BUILTIN_IDENTS = ("bool", "str")

# Integer-primitive idents: aliased to ``int`` (their wire value's Python type).
_INT_IDENTS = (
    "u8",
    "u16",
    "u32",
    "u64",
    "u128",
    "u256",
    "i8",
    "i16",
    "i32",
    "i64",
    "i128",
    "i256",
)


def _annotation(type_ident: str) -> str:
    """The parameter annotation for one call arg's type ident.

    Named idents ("TaoBalance", "NetUid", "AccountId32") and primitives
    annotate as themselves via module-level aliases; structural idents that
    are not Python identifiers ("Vec<u16>", "(u16, u16)") degrade to ``Any``.

    Annotations are emitted as string literals so the ident survives to
    runtime in ``__annotations__`` (an evaluated ``TaoBalance`` alias would
    collapse to ``int``); the intents layer keys unit enforcement on it.
    """
    if type_ident in _BUILTIN_IDENTS:
        return type_ident
    if type_ident.isidentifier() and not keyword.iskeyword(type_ident):
        return type_ident
    return "Any"


def _check_unique(scope: str, names: list[str], reserved: tuple[str, ...] = ()) -> None:
    """Fail codegen loudly on emitted-name collisions (the last def would silently win)."""
    seen: set[str] = set()
    for name in names:
        if name in reserved:
            raise ValueError(f"codegen name collision in {scope}: {name!r} shadows a preamble type")
        if name in seen:
            raise ValueError(f"codegen name collision in {scope}: {name!r} emitted twice")
        seen.add(name)


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

    # Type-identity aliases: the runtime's own name for each parameter type.
    # Wire values stay plain Python (ints, ss58 strings, dicts) — the aliases
    # exist so builder signatures read like the runtime declares them (e.g.
    # add_stake's amount_staked is a TaoBalance while remove_stake's
    # amount_unstaked is an AlphaBalance), not to constrain callers.
    idents = sorted(
        {arg.type_ident for pallet in ir.pallets for call in pallet.calls for arg in call.args}
    )
    aliases = [i for i in idents if i not in _BUILTIN_IDENTS and _annotation(i) == i]
    _check_unique("calls.py type aliases", aliases, reserved=("Call", "Any", "NamedTuple"))
    for ident in aliases:
        lines.append(f"{ident} = {'int' if ident in _INT_IDENTS else 'Any'}\n")
    if aliases:
        lines.append("\n\n")

    _check_unique(
        "calls.py pallet classes",
        [p.name for p in ir.pallets if p.calls],
        reserved=("Call", *aliases),
    )
    for pallet in sorted(ir.pallets, key=lambda p: p.index):
        if not pallet.calls:
            continue
        _check_unique(
            f"calls.py class {pallet.name}",
            [_py_name(c.name) for c in pallet.calls],
            reserved=("Call",),
        )
        lines.append(f"class {pallet.name}:\n")
        lines.append(f'    """Call builders for the {pallet.name} pallet."""\n\n')
        for call in sorted(pallet.calls, key=lambda c: c.name):
            params = [f"{_py_name(a.name)}: {_annotation(a.type_ident)!r}" for a in call.args]
            sig = ", ".join(params)
            lines.append("    @staticmethod\n")
            lines.append(f"    def {_py_name(call.name)}({sig}) -> Call:\n")
            if call.docs:
                lines.append(f"        {call.docs!r}\n")
            param_dict = ", ".join(f"{a.name!r}: {_py_name(a.name)}" for a in call.args)
            lines.append(
                f"        return Call({pallet.name!r}, {call.name!r}, {{{param_dict}}})\n\n"
            )
        lines.append("\n")
    return "".join(lines)


def _emit_item_classes(
    ir: MetadataIR,
    header_doc: str,
    item_class: str,
    groups: list[tuple[str, list]],
    extra_field: str = "",
) -> str:
    """Shared emitter for descriptor files: per-group classes of (container, name) tuples.

    ``extra_field`` grows the item tuple with one more, defaulted str field;
    group items are then (name, value) pairs instead of bare names.
    """
    lines = [_HEADER_WITH_DOC.format(spec_version=ir.spec_version, doc=header_doc)]
    lines.append("from typing import NamedTuple\n\n\n")
    lines.append(f"class {item_class}(NamedTuple):\n")
    lines.append('    """A (container, name) pair; unpack into query/constant calls."""\n\n')
    lines.append("    container: str\n")
    lines.append("    name: str\n")
    if extra_field:
        lines.append(f"    {extra_field}: str = ''\n")
        lines.append("\n")
        lines.append("    def __iter__(self):\n")
        lines.append(
            f"        # Unpacking feeds (module, storage_function) signatures; {extra_field}\n"
        )
        lines.append("        # is metadata for normalization, reached by attribute only.\n")
        lines.append("        return iter((self.container, self.name))\n")
    lines.append("\n\n")
    _check_unique(
        f"{item_class} descriptor groups",
        [g for g, entries in groups if entries],
        reserved=(item_class,),
    )
    for group_name, entries in groups:
        if not entries:
            continue
        names = [entry[0] if extra_field else entry for entry in entries]
        _check_unique(
            f"descriptor class {group_name}",
            [_py_name(n) for n in names],
            reserved=(item_class,),
        )
        lines.append(f"class {group_name}:\n")
        for entry in entries:
            if extra_field:
                name, extra = entry
                lines.append(
                    f"    {_py_name(name)} = {item_class}({group_name!r}, {name!r}, {extra!r})\n"
                )
            else:
                lines.append(f"    {_py_name(entry)} = {item_class}({group_name!r}, {entry!r})\n")
        lines.append("\n")
    return "".join(lines)


def emit_storage(ir: MetadataIR) -> str:
    groups: list[tuple[str, list]] = [
        (pallet.name, [(s.name, s.value_type_ident) for s in pallet.storage])
        for pallet in sorted(ir.pallets, key=lambda p: p.index)
    ]
    return _emit_item_classes(
        ir,
        "Storage item descriptors: unpack into substrate.query/query_map. Each "
        "carries its VALUE's type identity (value_type_ident) so normalization "
        "can key on the runtime's own type names without a node round-trip.",
        "Item",
        groups,
        extra_field="value_type_ident",
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
    out = {
        "__init__.py": '"""Generated wire layer. DO NOT EDIT BY HAND."""\n',
        "errors.py": emit_errors(ir),
        "calls.py": emit_calls(ir),
        "storage.py": emit_storage(ir),
        "constants.py": emit_constants(ir),
        "runtime_apis.py": emit_runtime_apis(ir),
    }
    # Fail codegen (not the eventual import) on any syntax error in emitted source.
    for filename, content in out.items():
        compile(content, filename, "exec")
    return out


def write(ir: MetadataIR, out_dir: Path) -> list[Path]:
    out_dir.mkdir(parents=True, exist_ok=True)
    written = []
    for filename, content in artifacts(ir).items():
        path = out_dir / filename
        path.write_text(content)
        written.append(path)
    return written
