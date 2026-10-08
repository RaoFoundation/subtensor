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
    check_network,
    coerce_payment_address,
    is_receiving_address,
    parse_recipient,
)
from .sp_core import CRYPTO_HASHED, Keypair

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
                await check_network(substrate, recipient)
                if await substrate.constant("HashedAccounts", "Enabled") is not True:
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
    if value["version"] != 1 or value["scheme"] not in ("Sr25519", {"Sr25519": None}):
        raise ValueError("unsupported hashed descriptor")
    commitment = raw_bytes(value["initial_commitment"])
    if len(commitment) != 32:
        raise ValueError("hashed key commitments must contain 32 bytes")
    return b"\x01\x01" + commitment


def descriptor_value(descriptor: bytes) -> dict:
    """Convert the native key's SCALE descriptor to runtime call parameters."""
    if len(descriptor) != 34 or descriptor[:2] != b"\x01\x01":
        raise ValueError("unsupported hashed descriptor")
    return {
        "version": 1,
        "scheme": "Sr25519",
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
            if key.ss58_address == address and key.crypto_type == CRYPTO_HASHED:
                return bytes(key.hashed_descriptor)
    return None


def has_local_hashed_recipient(wallet: Any, intent: Any) -> bool:
    """Detect public setup information before an imported-call fast path."""
    from .signing import public_view

    parameter = _REGISTRATION_TARGETS.get(intent.op)
    address = getattr(intent, parameter, None) if parameter else None
    if address is None and parameter == "hotkey_ss58":
        return public_view(wallet, "hotkey").crypto_type == CRYPTO_HASHED
    return isinstance(address, str) and _local_descriptor(wallet, address) is not None


async def has_receiving_setup_inputs(substrate, wallet: Any, intent: Any, depth: int = 0) -> bool:
    """Imported call bytes cannot be authenticated by unrelated display metadata."""
    if depth > 16:
        raise ValueError("intent nesting is too deep to verify recipient setup")
    semantic = intent.semantic_intent()
    if semantic.op == "fund_evm_key":
        # Its recipient is resolved dynamically from the alias registry, not an
        # address field. Imported bytes may encode the obsolete mirror instead.
        return await substrate.constant("HashedAccounts", "Enabled") is True
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
    """Register a known hashed recipient and its operation in one atomic batch.

    The caller adds origin wrappers after this step, keeping setup under the
    same sponsor authorization. No mutation or private-key access occurs here,
    so plan/dry-run has the same call and fee as submission.
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
            if public.crypto_type == CRYPTO_HASHED:
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
        return ([call] if as_calls else call), {}
    if await substrate.constant("HashedAccounts", "Enabled") is not True:
        raise ValueError("hashed accounts are not enabled on this chain")
    unique = {recipient.account: recipient for recipient in recipients.values()}
    registrations, new_accounts = [], []
    for address, recipient in unique.items():
        record = await substrate.query("HashedAccounts", "Accounts", [address])
        if record is not None:
            if descriptor_bytes(record["descriptor"]) != recipient.descriptor:
                raise ValueError("registered hashed descriptor does not match the destination")
        else:
            if (
                parameter not in recipients
                or recipients[parameter].account != address
                or len(unique) != 1
                or intent.op == "swap_coldkey_announced"
            ):
                raise ValueError(
                    "recipient is not registered; send it a direct payment "
                    "before using this operation"
                )
            new_accounts.append(address)
        registrations.append(
            await substrate.compose(
                calls.HashedAccounts.register(descriptor=descriptor_value(recipient.descriptor))
            )
        )
    deposit = int(await substrate.constant("HashedAccounts", "RegistrationDeposit"))
    # Always guard, including already registered recipients: a storage read can
    # be invalidated by a reorg. Idempotence protects authority and avoids a
    # second reserve. Unsupported new registrations fail inside this atomic batch.
    guarded = [*registrations, call]
    batch = guarded if as_calls else await substrate.compose(calls.Utility.batch_all(calls=guarded))
    extras = {
        "hashed_registration_guards": list(unique),
        "hashed_registration_max_deposit_rao": deposit * len(unique),
    }
    if new_accounts:
        extras.update(
            hashed_registration=new_accounts[0],
            hashed_registration_deposit_rao=deposit * len(new_accounts),
        )
    return batch, extras
