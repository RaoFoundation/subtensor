"""Reuse approved dynamic inputs while verifying an entire multisig call.

This context is only used during reconstruction followed by exact byte equality.
It is not permission to accept an imported call or to skip recipient guards.
"""

from __future__ import annotations

from contextlib import contextmanager
from contextvars import ContextVar

from ._generated.calls import Call

_inputs: ContextVar[_Inputs | None] = ContextVar("multisig_pinned_inputs", default=None)


class _Inputs:
    def __init__(self, substrate, data):
        self.substrate = substrate
        self.data = data
        self.calls = None
        self.indices = {}
        self.positions = {}

    async def current(self, module, function):
        if self.calls is None:
            decoded_calls = {}

            def visit(value):
                if isinstance(value, dict):
                    if "call_module" in value and "call_function" in value:
                        key = (value["call_module"], value["call_function"])
                        args = {arg["name"]: arg["value"] for arg in value["call_args"]}
                        self.positions[(key, len(decoded_calls.get(key, [])))] = len(self.positions)
                        decoded_calls.setdefault(key, []).append(args)
                    for child in value.values():
                        visit(child)
                elif isinstance(value, list):
                    for child in value:
                        visit(child)

            visit(await self.substrate.decode_scale("Call", self.data))
            self.calls = decoded_calls
        key = (module, function)
        index = self.indices.get(key, 0)
        try:
            return self.calls[key][index]
        except (KeyError, IndexError) as error:
            raise ValueError(
                "imported multisig call does not match the staking operation"
            ) from error

    def advance(self, module, function):
        key = (module, function)
        self.indices[key] = self.indices.get(key, 0) + 1

    async def choice(self, module, functions):
        candidates = []
        for function in functions:
            try:
                await self.current(module, function)
            except ValueError:
                continue
            key = (module, function)
            candidates.append((self.positions[(key, self.indices.get(key, 0))], function))
        if not candidates:
            raise ValueError("imported multisig call does not match the approved operation")
        return min(candidates)[1]


@contextmanager
def pinned_inputs(substrate, data):
    token = _inputs.set(_Inputs(substrate, data))
    try:
        yield
    finally:
        _inputs.reset(token)


async def compose_staking_call(substrate, function, params, *, derived=None):
    return await compose_pinned_call(
        substrate, "SubtensorModule", function, params, derived=derived
    )


async def compose_pinned_call(substrate, module, function, params, *, derived=None):
    """Freeze only inputs normally derived from mutable chain state.

    Explicit arguments are never overwritten. Participating intents use this path
    for explicit amounts too, so repeated calls consume their own approved inputs.
    The executor subsequently compares the full reconstructed call, including all
    guards and origin wrappers, against the pinned bytes before permitting signing.
    """
    derived = derived or {}
    state = _inputs.get()
    approved = await state.current(module, function) if state is not None else None
    params = dict(params)
    for name, resolve in derived.items():
        if approved is None:
            params[name] = await resolve()
        else:
            if name not in approved:
                raise ValueError("imported multisig call is missing an approved staking input")
            params[name] = approved[name]
    call = await substrate.compose(Call(module, function, params))
    if state is not None:
        state.advance(module, function)
    return call


async def approved_staking_amount(function, parameter):
    return await approved_amount("SubtensorModule", function, parameter)


async def approved_amount(module, function, parameter):
    """Read the originally resolved amount for an `all` request, if replaying."""
    state = _inputs.get()
    if state is None:
        return None
    approved = await state.current(module, function)
    if parameter not in approved:
        raise ValueError("imported multisig call is missing its approved staking amount")
    return int(approved[parameter])


async def approved_function(module, functions):
    """Preserve an approved state-dependent branch, including repeated calls."""
    state = _inputs.get()
    return None if state is None else await state.choice(module, functions)
