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
from .mldsa import supports_fixed_profile
from .receiving import (
    Recipient,
    coerce_payment_address,
    is_receiving_address,
    parse_recipient,
)
from .sp_core import Keypair, is_stateless_pq, is_stateless_pq_descriptor, requires_registration

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

# Association needs descriptor/ownership checks, but must not implicitly create
# an account under a potentially incompatible owner.
_DESCRIPTOR_TARGETS = {**_REGISTRATION_TARGETS, "associate_hotkey": "hotkey_ss58"}


async def hashed_accounts_enabled(substrate, *, block_hash=None):
    """Read the live switch; None means this runtime has no hashed support.

    Never cache the storage value: sudo can change it without changing runtime
    metadata. RegistrationDeposit identifies runtimes that support the pallet.
    """
    if await substrate.constant("HashedAccounts", "RegistrationDeposit") is None:
        return None
    return await substrate.query("AdminUtils", "HashedAccountsEnabled", block_hash=block_hash)


def descriptor_storage(descriptor: bytes) -> str:
    """Return the distinct permanent registry, validating the descriptor first."""
    descriptor_value(descriptor)
    return "MlDsaAccounts" if descriptor[0] == 2 else "Accounts"


async def protected_account_record(substrate, account: str, *, block_hash=None):
    """Resolve direct and derived protection without querying absent post-quantum metadata.

    Derived records are authority policies, never direct signing descriptors.
    Registrations remain authoritative when authorization/setup is paused.
    """
    record = await substrate.query("HashedAccounts", "Accounts", [account], block_hash=block_hash)
    if record is not None:
        return "v1", record
    if await substrate.constant("HashedAccounts", "MlDsaRegistrationDeposit") is not None:
        for kind, storage in (("mldsa", "MlDsaAccounts"), ("derived", "DerivedAuthorities")):
            record = await substrate.query(
                "HashedAccounts", storage, [account], block_hash=block_hash
            )
            if record is not None:
                return kind, record
    return None, None


async def is_pq_required(substrate, account: str, *, block_hash=None) -> bool:
    """Read the permanent direct or derived quantum-resistant authority floor."""
    if await substrate.constant("HashedAccounts", "MlDsaRegistrationDeposit") is None:
        return False
    kind, _ = await protected_account_record(substrate, account, block_hash=block_hash)
    return kind in ("mldsa", "derived")


async def compose_derivative(substrate, parent, index: int, call, *, account_mode=None):
    """Wrap exact call bytes using the actual parent's derivation mode.

    A fixed post-quantum key/wallet or safe composite view defaults to safe derivation.
    A bare SS58 needs explicit ``account_mode="safe"``; its key type cannot
    be inferred. Legacy mode retains registered rotating-account protection.
    """
    if not 0 <= index <= 65535:
        raise ValueError("derivative index must fit u16")
    from .account_mode import validate_account_mode, warn_legacy_mldsa
    from .signing import public_view

    if not isinstance(parent, str):
        if account_mode is None:
            account_mode = getattr(parent, "account_mode", None)
        public = public_view(parent, "coldkey")
        if account_mode is None and is_stateless_pq(public):
            account_mode = "safe"
        warn_legacy_mldsa(account_mode or "legacy", public, composite="derivative")
        parent = public.ss58_address
    if account_mode is not None:
        validate_account_mode(account_mode)
    parent = parse_recipient(parent).account
    if account_mode == "safe":
        return await substrate.compose(
            calls.Call("Utility", "as_safe_derivative", {"index": index, "call": call})
        )
    protected = await is_pq_required(substrate, parent)
    return await substrate.compose(
        calls.Call(
            "Utility",
            "as_protected_derivative" if protected else "as_derivative",
            {"index": index, "call": call},
        )
    )


async def multisig_protection_status(substrate, signatories: list[str], threshold: int) -> dict:
    """Describe finalized protection eligibility without submitting anything."""
    if await substrate.constant("HashedAccounts", "MlDsaRegistrationDeposit") is None:
        return {"eligible": False, "registered": False}
    account = substrate.multisig_account(
        [parse_recipient(s).account for s in signatories], threshold
    )
    block_hash = await substrate.block_hash(await substrate.finalized_block_number())
    if not block_hash:
        raise ValueError("could not verify finalized multisig protection")
    members = list(account.signatories)
    weak = 0
    for member in members:
        if not await is_pq_required(substrate, member, block_hash=block_hash):
            weak += 1
    kind, _ = await protected_account_record(substrate, account.ss58_address, block_hash=block_hash)
    return {"eligible": len(members) >= 2 and weak < threshold, "registered": kind == "derived"}


async def multisig_protection_call(
    substrate, signatories: list[str], threshold: int, *, deposit_payer: str
):
    """Compose explicit permanent protection for a multisig authority.

    Every registration requires the multisig's own quorum, including new
    unfunded identities. The final executing member explicitly pays the deposit.
    Approve this call separately so pending economic call bytes remain fixed.
    """
    from .sp_core import ss58_decode, ss58_encode

    if await substrate.constant("HashedAccounts", "MlDsaRegistrationDeposit") is None:
        raise ValueError("quantum-resistant derived accounts are not supported on this chain")
    if await hashed_accounts_enabled(substrate) is not True:
        raise ValueError("hashed accounts are not enabled on this chain")
    members = sorted({bytes(ss58_decode(parse_recipient(s).account)) for s in signatories})
    if not 2 <= len(members) <= 100 or not 1 <= threshold <= len(members):
        raise ValueError("invalid protected multisig threshold or signatory count")
    members = [ss58_encode(member, 42) for member in members]
    finalized = await substrate.block_hash(await substrate.finalized_block_number())
    if not finalized:
        raise ValueError("could not verify finalized multisig authority")
    weak = 0
    for member in members:
        if not await is_pq_required(substrate, member, block_hash=finalized):
            weak += 1
    if weak >= threshold:
        raise ValueError(
            "a complete multisig quorum must require a finalized quantum-resistant member"
        )
    account = substrate.multisig_account(members, threshold).ss58_address
    descriptor = {"Multisig": {"threshold": threshold, "signatories": members}}
    record = await substrate.query(
        "HashedAccounts", "DerivedAuthorities", [account], block_hash=finalized
    )
    if record is not None:
        if record["descriptor"] != descriptor:
            raise ValueError("registered derived authority does not match the multisig")
        function = "check_derived"
    else:
        function = "register_derived_sponsored"
    params = {"descriptor": descriptor}
    if function == "register_derived_sponsored":
        params["deposit_payer"] = parse_recipient(deposit_payer).account
    return await substrate.compose(calls.Call("HashedAccounts", function, params))


async def descriptor_registration_enabled(substrate, descriptor: bytes, *, block_hash=None):
    if descriptor_storage(descriptor) == "Accounts":
        return await hashed_accounts_enabled(substrate, block_hash=block_hash)
    if descriptor[1] in (6, 7, 8, 9):
        profiles = await substrate.constant("HashedAccounts", "FalconAccountProfiles")
        if not supports_fixed_profile(profiles, descriptor[1]):
            return None
    elif descriptor[1] == 5:
        profiles = await substrate.constant("HashedAccounts", "MldsaAccountProfiles")
        if not supports_fixed_profile(profiles):
            return None
    elif await substrate.constant("HashedAccounts", "MlDsaRegistrationDeposit") is None:
        return None
    return await hashed_accounts_enabled(substrate, block_hash=block_hash)


async def _require_receiving_support(substrate, descriptor: bytes):
    enabled = await descriptor_registration_enabled(substrate, descriptor)
    if enabled is not True:
        raise ValueError("hashed receiving addresses are not supported on this chain")


async def prepare_recipient_intent(substrate, intent):
    """Resolve typed recipients only at the call boundary, retaining their guards.

    Return a copy with chain AccountIds and a map of the complete recipients.
    Neither display/plan arguments nor stored contacts lose their descriptor.
    """
    from .signing import is_address_param

    changes, recipients = {}, {}

    async def resolve(item, name):
        if not is_receiving_address(item):
            return item
        recipient = parse_recipient(item)
        await _require_receiving_support(substrate, recipient.descriptor)
        recipients[name] = recipient
        return recipient.account

    for field in fields(intent):
        if intent.op == "set_children" and field.name == "children":
            children = []
            for index, (proportion, address) in enumerate(intent.children):
                address = await coerce_payment_address(substrate, address, "hotkey_ss58")
                children.append([proportion, await resolve(address, f"children[{index}][1]")])
            if children != intent.children:
                changes["children"] = children
            continue
        if not is_address_param(field.name):
            continue
        value = await coerce_payment_address(substrate, getattr(intent, field.name), field.name)
        values = value if isinstance(value, (list, tuple)) else [value]
        resolved = []
        for index, item in enumerate(values):
            name = field.name if not isinstance(value, (list, tuple)) else f"{field.name}[{index}]"
            resolved.append(await resolve(item, name))
        normalized = resolved if isinstance(value, (list, tuple)) else resolved[0]
        if normalized != getattr(intent, field.name):
            changes[field.name] = normalized
    return (replace(intent, **changes) if changes else intent), recipients


def raw_bytes(value: Any) -> bytes:
    return bytes.fromhex(value.removeprefix("0x")) if isinstance(value, str) else bytes(value)


def descriptor_bytes(value: dict) -> bytes:
    """Validate the versioned public descriptor decoded from chain storage."""
    if value.get("version") == 2:
        commitment = raw_bytes(value["initial_commitment"])
        if (
            value.get("algorithm") not in (4, 5, 6, 7, 8, 9)
            or len(commitment) != 64
            or not any(commitment)
        ):
            raise ValueError("unsupported post-quantum descriptor")
        return bytes((2, value["algorithm"])) + commitment
    scheme = value["scheme"]
    if isinstance(scheme, dict) and len(scheme) == 1:
        scheme = next(iter(scheme))
    schemes = {"Sr25519": 1, "Ed25519": 3}
    if value["version"] != 1 or not isinstance(scheme, str) or scheme not in schemes:
        raise ValueError("unsupported hashed descriptor")
    commitment = raw_bytes(value["initial_commitment"])
    if len(commitment) != 32 or not any(commitment):
        raise ValueError("hashed key commitments must contain 32 bytes")
    return bytes((value["version"], schemes[scheme])) + commitment


def descriptor_value(descriptor: bytes) -> dict:
    """Convert the native key's SCALE descriptor to its stored descriptor fields."""
    if (
        len(descriptor) == 66
        and descriptor[0] == 2
        and descriptor[1] in (4, 5, 6, 7, 8, 9)
        and any(descriptor[2:])
    ):
        return {
            "version": 2,
            "algorithm": descriptor[1],
            "initial_commitment": "0x" + descriptor[2:].hex(),
        }
    if (
        len(descriptor) != 34
        or (descriptor[0], descriptor[1])
        not in (
            (1, 1),
            (1, 3),
        )
        or not any(descriptor[2:])
    ):
        raise ValueError("unsupported hashed descriptor")
    return {
        "version": descriptor[0],
        "scheme": {1: "Sr25519", 3: "Ed25519"}[descriptor[1]],
        "initial_commitment": "0x" + descriptor[2:].hex(),
    }


def registration_descriptor_value(descriptor: bytes) -> dict:
    """Select the original register/check call's explicit authentication scheme."""
    value = descriptor_value(descriptor)
    variant = (
        "Falcon"
        if descriptor[0] == 2 and descriptor[1] in (6, 7, 8, 9)
        else ("MlDsa65" if descriptor[0] == 2 else "Classical")
    )
    return {variant: value}


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
            if key.ss58_address == address and requires_registration(key):
                return bytes(key.hashed_descriptor)
    return None


def has_local_hashed_recipient(wallet: Any, intent: Any) -> bool:
    """Detect public setup information before an imported-call fast path."""
    from .signing import public_view

    if intent.op == "set_children" and any(
        isinstance(address, str) and _local_descriptor(wallet, address) is not None
        for _, address in intent.children
    ):
        return True
    parameter = _DESCRIPTOR_TARGETS.get(intent.op)
    address = getattr(intent, parameter, None) if parameter else None
    if address is None and parameter == "hotkey_ss58":
        return requires_registration(public_view(wallet, "hotkey"))
    return isinstance(address, str) and _local_descriptor(wallet, address) is not None


async def has_receiving_setup_inputs(substrate, wallet: Any, intent: Any, depth: int = 0) -> bool:
    """Imported call bytes cannot be authenticated by unrelated display metadata."""
    if depth > 16:
        raise ValueError("intent nesting is too deep to verify recipient setup")
    semantic = intent.semantic_intent()
    if semantic.op == "fund_evm_key":
        if await substrate.constant("EVM", "NativeDepositSupported") is True:
            return True
        # Its recipient is resolved dynamically from the alias registry, not an
        # address field. Imported bytes may encode the obsolete mirror instead.
        enabled = await hashed_accounts_enabled(substrate)
        if enabled is not False:
            return enabled is True
        # Disabling registration does not remove existing alias bindings.
        from .evm.addresses import h160_to_ss58, resolve_evm_recipient

        recipient = await resolve_evm_recipient(substrate, semantic.evm_address)
        return recipient.descriptor is not None or recipient.account != h160_to_ss58(
            semantic.evm_address
        )
    _, recipients = await prepare_recipient_intent(substrate, semantic)
    if recipients or getattr(semantic, "hashed_descriptor", None) is not None:
        return True
    if has_local_hashed_recipient(wallet, semantic):
        return True
    nested = None
    if semantic.op == "execute_proxy_announced":
        nested = {**semantic.inner_args, "op": semantic.inner_op}
    elif semantic.op in ("multisig_execute", "multisig_threshold_1", "create_crowdloan"):
        nested = semantic.call
    if nested:
        from .intents.registry import build

        spec = dict(nested)
        child = build(spec.pop("op"), spec)
        if await has_receiving_setup_inputs(substrate, wallet, child, depth + 1):
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
    origin: Any = None,
):
    """Guard a known hashed recipient and its operation in one atomic batch.

    The caller adds origin wrappers after this step, keeping setup under the
    same sponsor authorization. No mutation or private-key access occurs here,
    so plan/dry-run has the same call and fee as submission.

    Coldkey operations with finalized destinations and fee-free PoW registration
    use direct calls. Initial announcements may atomically register a new recipient.
    Require the recipient's permanent registration in finalized state instead
    of adding a batch that the lock rejects or that loses the PoW fee exemption.
    Announced-proxy identities also use finalized checks: an outer batch would
    filter the guarded payment's inner batch and consume its announcement.
    """
    from .signing import public_view

    recipients = dict(recipients or {})
    if intent.op == "set_children":
        for index, (_, address) in enumerate(intent.children):
            name = f"children[{index}][1]"
            if name not in recipients and isinstance(address, str):
                descriptor = _local_descriptor(wallet, address)
                if descriptor is not None:
                    recipients[name] = Recipient(address, address, descriptor)
    parameter = _DESCRIPTOR_TARGETS.get(intent.op)
    if parameter is not None:
        address = getattr(intent, parameter, None)
        default_descriptor = None
        if address is None and parameter == "hotkey_ss58":
            public = public_view(wallet, "hotkey")
            address = public.ss58_address
            if requires_registration(public):
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
            if is_stateless_pq_descriptor(descriptor):
                await _require_receiving_support(substrate, descriptor)
                recipients.pop(parameter, None)
            else:
                recipients[parameter] = typed or Recipient(address, address, descriptor)
    for name, recipient in list(recipients.items()):
        if recipient.descriptor is not None and is_stateless_pq_descriptor(recipient.descriptor):
            public = Keypair.from_hashed_descriptor(recipient.descriptor)
            if public.ss58_address != recipient.account:
                raise ValueError("hashed descriptor does not match the destination address")
            del recipients[name]
    if not recipients:
        return (await atomic_calls(substrate, call) if as_calls else call), {}
    descriptors: dict[str, bytes] = {}
    for recipient in recipients.values():
        descriptor = recipient.descriptor
        if descriptor is None:
            raise ValueError("hashed recipient registration requires a descriptor")
        descriptors[recipient.account] = descriptor
        if descriptor[0] == 1:
            if await hashed_accounts_enabled(substrate) is not True:
                raise ValueError("hashed accounts are not enabled on this chain")
        else:
            await _require_receiving_support(substrate, descriptor)
    if intent.op == "associate_hotkey":
        owner = public_view(wallet if origin is None else origin, "coldkey").ss58_address
        kind, _ = await protected_account_record(substrate, owner)
        requires_pq = any(descriptor[0] == 2 for descriptor in descriptors.values())
        if kind is None or (requires_pq and kind not in ("mldsa", "derived")):
            raise ValueError("a hashed hotkey requires a registered hashed coldkey owner")
    finalized_hash = None
    direct = intent.op in ("swap_coldkey_announced", "pow_register", "execute_proxy_announced")
    if intent.op == "announce_coldkey_swap":
        # Existing announcements lock out Utility.batch_all, even when it only
        # checks registration. Keep first-use setup atomic, but require finality
        # and a direct call once the destination has a permanent registration.
        destination = recipients["new_coldkey_ss58"].account
        storage = descriptor_storage(descriptors[destination])
        direct = await substrate.query("HashedAccounts", storage, [destination]) is not None
    if direct:
        operation = {
            "pow_register": "PoW registration",
            "execute_proxy_announced": "announced proxy execution",
        }.get(intent.op, "coldkey swap")
        if as_calls:
            raise ValueError(f"submit the {operation} directly, not inside a batch")
        finalized_hash = await substrate.block_hash(await substrate.finalized_block_number())
        if not finalized_hash:
            raise ValueError("could not verify finalized hashed destination registration")
    unique = {recipient.account: recipient for recipient in recipients.values()}
    registrations, new_accounts = [], []
    total_deposit = 0
    for address in unique:
        descriptor = descriptors[address]
        record = await substrate.query(
            "HashedAccounts",
            descriptor_storage(descriptor),
            [address],
            block_hash=finalized_hash,
        )
        if record is not None:
            if descriptor_bytes(record["descriptor"]) != descriptor:
                raise ValueError("registered hashed descriptor does not match the destination")
        else:
            if await descriptor_registration_enabled(substrate, descriptor) is not True:
                raise ValueError("new account registrations are not enabled on this chain")
            if finalized_hash is not None:
                raise ValueError(
                    "hashed destination registration is not finalized; register the recipient "
                    f"and wait for finalization before executing the {operation}"
                )
            if (
                intent.op not in _REGISTRATION_TARGETS
                or parameter not in recipients
                or recipients[parameter].account != address
                or len(unique) != 1
            ):
                raise ValueError(
                    "recipient is not registered; send it a direct payment "
                    "before using this operation"
                )
            new_accounts.append(address)
            deposit_name = (
                "MlDsaRegistrationDeposit" if descriptor[0] == 2 else "RegistrationDeposit"
            )
            total_deposit += int(await substrate.constant("HashedAccounts", deposit_name))
        if finalized_hash is not None:
            continue
        name = "check_registered" if record is not None else "register"
        registrations.append(
            await substrate.compose(
                calls.Call(
                    "HashedAccounts",
                    name,
                    {"descriptor": registration_descriptor_value(descriptor)},
                )
            )
        )
    if finalized_hash is not None:
        # Registrations cannot be removed or have their descriptor changed.
        # Finality therefore supplies the safety normally provided by the guard.
        return call, {"hashed_registration_finalized_at": finalized_hash}
    # A check-only guard fails closed after a reorg without granting proxies the
    # authority to sponsor registration. Only first-use registration reserves.
    guarded = [*registrations, *await atomic_calls(substrate, call)]
    batch = guarded if as_calls else await substrate.compose(calls.Utility.batch_all(calls=guarded))
    extras = {
        "hashed_registration_guards": list(unique),
        "hashed_registration_max_deposit_rao": total_deposit,
    }
    if new_accounts:
        extras.update(
            hashed_registration=new_accounts[0],
            hashed_registration_deposit_rao=total_deposit,
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
