"""Public descriptors and automatic setup for rotating hashed accounts.

Registration publishes commitments only. It must precede funding: an unknown
32-byte address cannot tell a sender whether its owner intended a hashed wallet.
"""

from __future__ import annotations

from dataclasses import fields, replace
from pathlib import Path
from typing import Any

from ._generated import calls
from .keyfiles import Keyfile, KeyfileError
from .receiving import (
    Recipient,
    coerce_payment_address,
    is_receiving_address,
    parse_recipient,
)
from .sp_core import HASHED_CRYPTO_TYPES, Keypair

_REGISTRATION_TARGETS = {
    "transfer": "dest_ss58",
    "transfer_all": "dest_ss58",
    "announce_coldkey_swap": "new_coldkey_ss58",
    "swap_coldkey_announced": "new_coldkey_ss58",
    "swap_hotkey": "new_hotkey_ss58",
    "burned_register": "hotkey_ss58",
    "pow_register": "hotkey_ss58",
    "root_register": "hotkey_ss58",
    "register_subnet": "hotkey_ss58",
}


async def hashed_accounts_enabled(substrate, *, block_hash=None):
    """Read the live switch; None means this runtime has no hashed support.

    Older experimental runtimes exposed a constant instead. Never cache the
    storage value: sudo can change it without changing runtime metadata.
    """
    legacy = await substrate.constant("HashedAccounts", "Enabled")
    if isinstance(legacy, bool):
        return legacy
    if await substrate.constant("HashedAccounts", "RegistrationDeposit") is None:
        return None
    return await substrate.query("AdminUtils", "HashedAccountsEnabled", block_hash=block_hash)


async def prepare_recipient_intent(substrate, intent):
    """Resolve typed recipients only at the call boundary, retaining their guards.

    Return a copy with chain AccountIds and a map of the complete recipients.
    Neither display/plan arguments nor stored contacts lose their descriptor.
    """
    from .signing import is_address_param

    changes, recipients = {}, {}
    for field in fields(intent):
        if not is_address_param(field.name):
            continue
        value = await coerce_payment_address(substrate, getattr(intent, field.name), field.name)
        values = value if isinstance(value, (list, tuple)) else [value]
        resolved = []
        for index, item in enumerate(values):
            if is_receiving_address(item):
                recipient = parse_recipient(item)
                if await hashed_accounts_enabled(substrate) is not True:
                    raise ValueError("hashed receiving addresses are not supported on this chain")
                name = (
                    field.name if not isinstance(value, (list, tuple)) else f"{field.name}[{index}]"
                )
                recipients[name] = recipient
                resolved.append(recipient.account)
            else:
                resolved.append(item)
        normalized = resolved if isinstance(value, (list, tuple)) else resolved[0]
        if normalized != getattr(intent, field.name):
            changes[field.name] = normalized
    return (replace(intent, **changes) if changes else intent), recipients


def raw_bytes(value: Any) -> bytes:
    return bytes.fromhex(value.removeprefix("0x")) if isinstance(value, str) else bytes(value)


def descriptor_bytes(value: dict) -> bytes:
    """Validate the versioned public descriptor decoded from chain storage."""
    scheme = value["scheme"]
    if isinstance(scheme, dict) and len(scheme) == 1:
        scheme = next(iter(scheme))
    schemes = {"Sr25519": 1, "MlDsa65": 2, "Ed25519": 3}
    if (
        (value["version"] != 1 and (value["version"], scheme) != (2, "MlDsa65"))
        or not isinstance(scheme, str)
        or scheme not in schemes
    ):
        raise ValueError("unsupported hashed descriptor")
    commitment = raw_bytes(value["initial_commitment"])
    if len(commitment) != 32:
        raise ValueError("hashed key commitments must contain 32 bytes")
    return bytes((value["version"], schemes[scheme])) + commitment


def descriptor_value(descriptor: bytes) -> dict:
    """Convert the native key's SCALE descriptor to runtime call parameters."""
    if len(descriptor) != 34 or (descriptor[0], descriptor[1]) not in (
        (1, 1),
        (1, 2),
        (1, 3),
        (2, 2),
    ):
        raise ValueError("unsupported hashed descriptor")
    return {
        "version": descriptor[0],
        "scheme": {1: "Sr25519", 2: "MlDsa65", 3: "Ed25519"}[descriptor[1]],
        "initial_commitment": "0x" + descriptor[2:].hex(),
    }


def _local_descriptor(wallet: Any, address: str) -> bytes | None:
    """Discover only public companion files; never prompt or open private keys."""
    wallet = getattr(wallet, "_wallet", wallet)
    path = getattr(wallet, "path", None)
    if not isinstance(path, (str, Path)):
        return None
    root = Path(path).expanduser()
    for pattern in ("*/coldkeypub.txt", "*/hotkeys/*pub.txt"):
        for file in sorted(root.glob(pattern)):
            public_file = Keyfile(file)
            try:
                if public_file.is_encrypted():
                    continue
                key = public_file.get_keypair()
            except (OSError, ValueError, KeyfileError):
                continue
            if key.ss58_address == address and key.crypto_type in HASHED_CRYPTO_TYPES:
                return bytes(key.hashed_descriptor)
    return None


def has_local_hashed_recipient(wallet: Any, intent: Any) -> bool:
    """Detect public setup information before an imported-call fast path."""
    from .signing import public_view

    parameter = _REGISTRATION_TARGETS.get(intent.op)
    address = getattr(intent, parameter, None) if parameter else None
    if address is None and parameter == "hotkey_ss58":
        return public_view(wallet, "hotkey").crypto_type in HASHED_CRYPTO_TYPES
    return isinstance(address, str) and _local_descriptor(wallet, address) is not None


async def has_receiving_setup_inputs(substrate, wallet: Any, intent: Any, depth: int = 0) -> bool:
    """Imported call bytes cannot be authenticated by unrelated display metadata."""
    if depth > 16:
        raise ValueError("intent nesting is too deep to verify recipient setup")
    semantic = intent.semantic_intent()
    if semantic.op == "fund_evm_key":
        # Its recipient is resolved dynamically from the alias registry, not an
        # address field. Imported bytes may encode the obsolete mirror instead.
        enabled = await hashed_accounts_enabled(substrate)
        if enabled is not False:
            return enabled is True
        # Disabling registration does not remove existing alias bindings.
        from .evm.addresses import resolve_evm_recipient

        recipient = await resolve_evm_recipient(substrate, semantic.evm_address)
        return recipient.descriptor is not None
    _, recipients = await prepare_recipient_intent(substrate, semantic)
    if recipients or getattr(semantic, "hashed_descriptor", None) is not None:
        return True
    if has_local_hashed_recipient(wallet, semantic):
        return True
    for child in getattr(semantic, "_children", ()):
        if await has_receiving_setup_inputs(substrate, wallet, child, depth + 1):
            return True
    return False


async def with_recipient_registration(
    substrate,
    wallet: Any,
    intent: Any,
    call: Any,
    *,
    recipients: dict[str, Recipient] | None = None,
    as_calls: bool = False,
):
    """Guard a known hashed recipient and its operation in one atomic batch.

    The caller adds origin wrappers after this step, keeping setup under the
    same sponsor authorization. No mutation or private-key access occurs here,
    so plan/dry-run has the same call and fee as submission.

    Coldkey operations with finalized destinations and fee-free PoW registration
    use direct calls. Initial announcements may atomically register a new recipient.
    Require the recipient's permanent registration in finalized state instead
    of adding a batch that the lock rejects or that loses the PoW fee exemption.
    """
    from .signing import public_view

    recipients = dict(recipients or {})
    parameter = _REGISTRATION_TARGETS.get(intent.op)
    if parameter is not None:
        address = getattr(intent, parameter, None)
        default_descriptor = None
        if address is None and parameter == "hotkey_ss58":
            public = public_view(wallet, "hotkey")
            address = public.ss58_address
            if public.crypto_type in HASHED_CRYPTO_TYPES:
                default_descriptor = bytes(public.hashed_descriptor)
        if not isinstance(address, str):
            raise ValueError("recipient account address is missing")
        explicit = getattr(intent, "hashed_descriptor", None)
        typed = recipients.get(parameter)
        descriptor = (
            raw_bytes(explicit)
            if explicit is not None
            else typed.descriptor
            if typed is not None
            else default_descriptor
            if default_descriptor is not None
            else _local_descriptor(wallet, address)
        )
        if descriptor is not None:
            public = Keypair.from_hashed_descriptor(descriptor)
            if public.ss58_address != address or (typed and typed.descriptor != descriptor):
                raise ValueError("hashed descriptor does not match the destination address")
            recipients[parameter] = typed or Recipient(address, address, descriptor)
    if not recipients:
        return (await atomic_calls(substrate, call) if as_calls else call), {}
    if await hashed_accounts_enabled(substrate) is not True:
        raise ValueError("hashed accounts are not enabled on this chain")
    finalized_hash = None
    direct = intent.op in ("swap_coldkey_announced", "pow_register")
    if intent.op == "announce_coldkey_swap":
        # Existing announcements lock out Utility.batch_all, even when it only
        # checks registration. Keep first-use setup atomic, but require finality
        # and a direct call once the destination has a permanent registration.
        destination = recipients["new_coldkey_ss58"].account
        direct = await substrate.query("HashedAccounts", "Accounts", [destination]) is not None
    if direct:
        operation = "PoW registration" if intent.op == "pow_register" else "coldkey swap"
        if as_calls:
            raise ValueError(f"submit the {operation} directly, not inside a batch")
        finalized_hash = await substrate.block_hash(await substrate.finalized_block_number())
        if not finalized_hash:
            raise ValueError("could not verify finalized hashed destination registration")
    unique = {recipient.account: recipient for recipient in recipients.values()}
    registrations, new_accounts = [], []
    for address, recipient in unique.items():
        record = await substrate.query(
            "HashedAccounts", "Accounts", [address], block_hash=finalized_hash
        )
        if record is not None:
            if descriptor_bytes(record["descriptor"]) != recipient.descriptor:
                raise ValueError("registered hashed descriptor does not match the destination")
        else:
            if finalized_hash is not None:
                raise ValueError(
                    "hashed destination registration is not finalized; register the recipient "
                    f"and wait for finalization before executing the {operation}"
                )
            if (
                parameter not in recipients
                or recipients[parameter].account != address
                or len(unique) != 1
            ):
                raise ValueError(
                    "recipient is not registered; send it a direct payment "
                    "before using this operation"
                )
            new_accounts.append(address)
        if finalized_hash is not None:
            continue
        guard = (
            calls.HashedAccounts.check_registered
            if record is not None
            else calls.HashedAccounts.register
        )
        registrations.append(
            await substrate.compose(guard(descriptor=descriptor_value(recipient.descriptor)))
        )
    if finalized_hash is not None:
        # Registrations cannot be removed or have their descriptor changed.
        # Finality therefore supplies the safety normally provided by the guard.
        return call, {"hashed_registration_finalized_at": finalized_hash}
    deposit = int(await substrate.constant("HashedAccounts", "RegistrationDeposit"))
    # A check-only guard fails closed after a reorg without granting proxies the
    # authority to sponsor registration. Only first-use registration reserves.
    guarded = [*registrations, *await atomic_calls(substrate, call)]
    batch = guarded if as_calls else await substrate.compose(calls.Utility.batch_all(calls=guarded))
    extras = {
        "hashed_registration_guards": list(unique),
        "hashed_registration_max_deposit_rao": deposit * len(new_accounts),
    }
    if new_accounts:
        extras.update(
            hashed_registration=new_accounts[0],
            hashed_registration_deposit_rao=deposit * len(new_accounts),
        )
    return batch, extras


async def atomic_calls(substrate, call: Any) -> list[Any]:
    """Flatten only same-origin atomic batches; preserve every other wrapper.

    Decoding is necessary for production CallBytes, whose contents are opaque.
    Recompose decoded children because the transport's display dictionaries are
    different from the enum values accepted by its SCALE encoder.
    """
    from .fee_filters import _arg_value, _call_dict

    decoded = _call_dict(call)
    if decoded is None:
        decoded = await substrate.decode_scale("Call", call.data)
    if (decoded.get("call_module"), decoded.get("call_function")) != ("Utility", "batch_all"):
        if isinstance(call, dict):
            call = await substrate.compose(
                calls.Call(
                    decoded["call_module"],
                    decoded["call_function"],
                    {arg["name"]: _call_variants(arg["value"]) for arg in decoded["call_args"]},
                )
            )
        return [call]
    flattened = []
    for child in _arg_value(decoded, "calls"):
        flattened.extend(await atomic_calls(substrate, child))
    return flattened


def _call_variants(value: Any) -> Any:
    """Restore nested RuntimeCall enum values without changing their wrappers."""
    if isinstance(value, dict):
        if "call_module" in value and "call_function" in value:
            return {
                value["call_module"]: {
                    value["call_function"]: {
                        arg["name"]: _call_variants(arg["value"]) for arg in value["call_args"]
                    }
                }
            }
        return {key: _call_variants(item) for key, item in value.items()}
    if isinstance(value, list):
        return [_call_variants(item) for item in value]
    return value
