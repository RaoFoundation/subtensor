"""Public descriptors and automatic setup for rotating hashed accounts.

Registration publishes commitments only. It must precede funding: an unknown
32-byte address cannot tell a sender whether its owner intended a hashed wallet.
"""

from __future__ import annotations

from dataclasses import dataclass, fields, replace
from pathlib import Path
from typing import Any

from ._generated import calls
from ._policy import evaluate_policy
from ._transport.errors import StorageFunctionNotFound
from .keyfiles import Keyfile, KeyfileError
from .mldsa import supports_fixed_profile
from .receiving import (
    Recipient,
    coerce_payment_address,
    is_receiving_address,
    parse_recipient,
)
from .sp_core import Keypair, is_stateless_pq_descriptor, requires_registration


def _descriptor_target(op):
    return evaluate_policy("transaction.receiving_target", {"op": op})


async def hashed_accounts_enabled(substrate, *, block_hash=None):
    """Read the live switch; None means this runtime has no hashed support.

    Never cache the storage value: sudo can change it without changing runtime
    metadata. RegistrationDeposit identifies runtimes that support the pallet.
    """
    if await substrate.constant("HashedAccounts", "RegistrationDeposit") is None:
        return None
    return await substrate.query("AdminUtils", "HashedAccountsEnabled", block_hash=block_hash)


@dataclass(frozen=True)
class HashedRegistrationDepositQuote:
    amount_rao: int
    configurable: bool


async def quote_hashed_registration_deposit(substrate, legacy_constant, *, block_hash=None):
    """Quote the live shared reserve, including zero, without caching sudo updates.

    Only runtimes whose metadata lacks the storage entry use the old per-kind
    constants. Read failures must not turn a stale constant into an approved cost.
    """
    if block_hash is None:
        block_hash = await substrate.block_hash()
        if not block_hash:
            raise ValueError("could not pin hashed account registration reserve")
    configurable = True
    try:
        value = await substrate.query(
            "HashedAccounts", "HashedAccountRegistrationDeposit", block_hash=block_hash
        )
    except StorageFunctionNotFound:
        configurable = False
        value = await substrate.constant("HashedAccounts", legacy_constant, block_hash=block_hash)
    if value is None:
        raise ValueError("could not quote hashed account registration reserve")
    return HashedRegistrationDepositQuote(
        int(evaluate_policy("financial.registration.deposit", {"value": value})), configurable
    )


async def hashed_registration_deposit(substrate, legacy_constant, *, block_hash=None):
    """Return the reserve amount for callers that do not compose registration."""
    quote = await quote_hashed_registration_deposit(
        substrate, legacy_constant, block_hash=block_hash
    )
    return quote.amount_rao


def descriptor_storage(descriptor: bytes) -> str:
    """Return the distinct permanent registry, validating the descriptor first."""
    return _descriptor_policy("transaction.descriptor", {"descriptor": bytes(descriptor)})[
        "storage"
    ]


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
    from ._policy import evaluate_policy
    from .account_mode import warn_legacy_mldsa
    from .signing import public_view

    crypto_type = None
    if not isinstance(parent, str):
        if account_mode is None:
            account_mode = getattr(parent, "account_mode", None)
        public = public_view(parent, "coldkey")
        crypto_type = public.crypto_type
        mode = evaluate_policy(
            "transaction.derivative",
            {"index": index, "mode": account_mode, "crypto_type": crypto_type},
        )["mode"]
        warn_legacy_mldsa(mode, public, composite="derivative")
        parent = public.ss58_address
    selected = evaluate_policy(
        "transaction.derivative", {"index": index, "mode": account_mode, "crypto_type": crypto_type}
    )
    parent = parse_recipient(parent).account
    protected = selected["mode"] != "safe" and await is_pq_required(substrate, parent)
    selected = evaluate_policy(
        "transaction.derivative",
        {"index": index, "mode": selected["mode"], "pq_required": protected},
    )
    return await substrate.compose(
        calls.Call("Utility", selected["call"], {"index": index, "call": call})
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
    return {
        "eligible": evaluate_policy(
            "transaction.multisig_eligible",
            {"count": len(members), "threshold": threshold, "weak": weak},
        ),
        "registered": kind == "derived",
    }


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


def _descriptor_policy(operation: str, value: dict):
    from ._policy import evaluate_policy

    try:
        return evaluate_policy(operation, value)
    except TypeError as error:
        raise ValueError(str(error)) from error


def descriptor_bytes(value: dict) -> bytes:
    """Validate a versioned public descriptor with the portable core."""
    normalized = {**value, "initial_commitment": raw_bytes(value["initial_commitment"])}
    return bytes.fromhex(
        _descriptor_policy("transaction.descriptor_encode", {"value": normalized})[2:]
    )


def descriptor_value(descriptor: bytes) -> dict:
    """Convert the public descriptor using the canonical portable validator."""
    return _descriptor_policy("transaction.descriptor", {"descriptor": bytes(descriptor)})["value"]


def registration_descriptor_value(descriptor: bytes) -> dict:
    result = _descriptor_policy("transaction.descriptor", {"descriptor": bytes(descriptor)})
    return {result["variant"]: result["value"]}


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
    parameter = _descriptor_target(intent.op)
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
    parameter = _descriptor_target(intent.op)
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
        if not evaluate_policy(
            "transaction.protected_owner",
            {"kind": "classical" if kind == "v1" else kind, "requires_pq": requires_pq},
        ):
            raise ValueError("a hashed hotkey requires a registered hashed coldkey owner")
    finalized_hash = None
    direct = evaluate_policy("transaction.receiving_direct", {"op": intent.op})
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
            if not evaluate_policy(
                "transaction.receiving_registration",
                {
                    "op": intent.op,
                    "field": parameter
                    if parameter in recipients and recipients[parameter].account == address
                    else None,
                    "unique_count": len(unique),
                    "direct": finalized_hash is not None,
                },
            )["may_register"]:
                raise ValueError(
                    "recipient is not registered; send it a direct payment "
                    "before using this operation"
                )
            new_accounts.append(address)
            deposit_name = (
                "MlDsaRegistrationDeposit" if descriptor[0] == 2 else "RegistrationDeposit"
            )
            quote = await quote_hashed_registration_deposit(
                substrate, deposit_name, block_hash=finalized_hash
            )
            total_deposit += quote.amount_rao
            if quote.configurable:
                registrations.append(
                    await substrate.compose(
                        calls.Call(
                            "HashedAccounts",
                            "check_registration_deposit",
                            {"max_deposit": quote.amount_rao},
                        )
                    )
                )
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
    from ._policy import evaluate_policy, evaluate_policy_preserving_values
    from .fee_filters import _call_dict

    decoded = _call_dict(call)
    if decoded is None:
        decoded = await substrate.decode_scale("Call", call.data)

    async def hydrate(value, depth=0):
        if depth > 128:
            raise ValueError("RuntimeCall nesting exceeds 128")
        if not isinstance(value, (str, bytes, bytearray, dict)):
            call_value = _call_dict(value)
            if call_value is not None:
                return await hydrate(call_value, depth + 1)
            if hasattr(value, "data"):
                return await hydrate(await substrate.decode_scale("Call", value.data), depth + 1)
        if isinstance(value, dict):
            return {key: await hydrate(child, depth + 1) for key, child in value.items()}
        if isinstance(value, (tuple, list)):
            return [await hydrate(child, depth + 1) for child in value]
        return value

    decoded = await hydrate(decoded)
    paths = evaluate_policy("protected.flatten_batch_paths", {"calls": [decoded]})
    if paths == [[0]] and not isinstance(call, dict):
        return [call]
    leaves = evaluate_policy_preserving_values("protected.flatten_batch", {"calls": [decoded]})
    return [
        await substrate.compose(
            calls.Call(
                leaf["pallet"],
                leaf["call"],
                {name: _call_variants(value) for name, value in leaf["params"].items()},
            )
        )
        for leaf in leaves
    ]


def _call_variants(value: Any) -> Any:
    """Restore nested RuntimeCall enum values without changing their wrappers."""
    if isinstance(value, dict):
        if (
            isinstance(value.get("pallet"), str)
            and isinstance(value.get("call"), str)
            and isinstance(value.get("params"), dict)
        ):
            return {
                value["pallet"]: {
                    value["call"]: {
                        key: _call_variants(child) for key, child in value["params"].items()
                    }
                }
            }
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
