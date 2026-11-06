"""Explicit, encrypted registration of locally persisted rotating wallet keys."""

from __future__ import annotations

import inspect
from collections.abc import Awaitable, Callable, Iterable
from dataclasses import dataclass
from typing import Any

from ._generated import calls
from .balance import Balance
from .hashed import (
    descriptor_bytes,
    descriptor_registration_enabled,
    descriptor_storage,
    hashed_registration_deposit,
    quote_hashed_registration_deposit,
    registration_descriptor_value,
)
from .registration_sponsorship import validate_registration_deposit_limit
from .result import BittensorError, ExtrinsicResult
from .settings import MEV_SHIELD_ERA_PERIOD
from .signing import public_view
from .sp_core import (
    CLASSICAL_HASHED_CRYPTO_TYPES,
    HASHED_CRYPTO_TYPES,
    Keypair,
    requires_registration,
)


@dataclass(frozen=True)
class WalletRegistrationQuote:
    """Missing registrations and their total permanent reserve and estimated fees."""

    payer_account: str
    accounts: tuple[str, ...]
    roles: dict[str, str]
    deposit: Balance
    fee: Balance


WalletRegistrationCallback = Callable[[WalletRegistrationQuote], bool | Awaitable[bool]]


@dataclass(frozen=True)
class WalletRegistrationResult:
    """Finalized identities, last receipt, and the quoted new registration reserve."""

    roles: dict[str, str]
    accounts: tuple[str, ...]
    finalized_block_hash: str | None
    result: ExtrinsicResult | None
    deposit: Balance


class WalletRegistrationError(BittensorError):
    """Registration did not complete; the existing wallet keys remain available."""

    def __init__(
        self,
        message: str,
        *,
        wallet: Any,
        result: ExtrinsicResult | None = None,
        quote: WalletRegistrationQuote | None = None,
    ):
        super().__init__(message)
        self.wallet = wallet
        self.result = result
        self.receipt = result
        self.quote = quote


def registration_targets(wallet, roles: Iterable[str]) -> tuple[dict[str, str], dict[str, bytes]]:
    """Read only public keys, validating each descriptor before any chain request."""
    if isinstance(roles, str):
        raise ValueError("roles must contain coldkey and/or hotkey")
    roles = tuple(roles)
    if any(role not in ("coldkey", "hotkey") for role in roles):
        raise ValueError("roles must contain coldkey and/or hotkey")
    selected: dict[str, str] = {}
    descriptors: dict[str, bytes] = {}
    for role in dict.fromkeys(roles):
        public = public_view(wallet, role)
        if not requires_registration(public):
            continue
        descriptor = bytes(public.hashed_descriptor)
        if Keypair.from_hashed_descriptor(descriptor).ss58_address != public.ss58_address:
            raise ValueError(f"{role} descriptor does not match its account")
        if public.ss58_address in descriptors and descriptors[public.ss58_address] != descriptor:
            raise ValueError("wallet roles have conflicting account descriptors")
        selected[role] = public.ss58_address
        descriptors[public.ss58_address] = descriptor
    return selected, descriptors


def empty_registration_result() -> WalletRegistrationResult:
    return WalletRegistrationResult({}, (), None, None, Balance.from_rao(0))


async def registration_deposit_for_types(client, crypto_types: Iterable[int]) -> Balance:
    """Quote new hashed keys before generating them; fixed-key wallets need no RPC."""
    crypto_types = tuple(kind for kind in crypto_types if kind in HASHED_CRYPTO_TYPES)
    if not crypto_types:
        return Balance.from_rao(0)
    substrate = client._substrate
    head = await substrate.block_hash()
    if not head:
        raise ValueError("could not pin wallet registration reserve")
    total = 0
    for kind in crypto_types:
        legacy_constant = (
            "RegistrationDeposit"
            if kind in CLASSICAL_HASHED_CRYPTO_TYPES
            else "MlDsaRegistrationDeposit"
        )
        reserve = await hashed_registration_deposit(substrate, legacy_constant, block_hash=head)
        if reserve < 0:
            raise ValueError("wallet registration reserve cannot be negative")
        total += reserve
    return Balance.from_rao(total)


async def _finalized_hash(substrate) -> str:
    block_hash = await substrate.block_hash(await substrate.finalized_block_number())
    if not block_hash:
        raise ValueError("could not verify finalized wallet registration")
    return block_hash


async def _registered(substrate, account: str, descriptor: bytes, block_hash: str) -> bool:
    record = await substrate.query(
        "HashedAccounts", descriptor_storage(descriptor), [account], block_hash=block_hash
    )
    if record is None:
        return False
    if descriptor_bytes(record["descriptor"]) != descriptor:
        raise ValueError("registered wallet descriptor does not match the local key")
    return True


async def register_wallet(
    client,
    wallet,
    *,
    payer,
    roles: Iterable[str] = ("coldkey", "hotkey"),
    max_registration_deposit: Balance | None = None,
    on_registration_sponsorship: WalletRegistrationCallback | None = None,
) -> WalletRegistrationResult:
    """Register persisted hashed keys with a funded payer, strict Shield and finality.

    A cap authorizes the aggregate permanent reserve. An optional callback sees
    the complete quote; if both are supplied, both must approve. No public
    submission fallback is permitted. Errors preserve the wallet and any receipt
    so callers can inspect finalized state and retry without generating new keys.
    Classical registrations require separate transactions; each is verified in
    finalized state before the next is submitted. Retries skip completed keys.
    """
    result = None
    quote = None
    try:
        validate_registration_deposit_limit(max_registration_deposit)
        selected, descriptors = registration_targets(wallet, roles)
        if not descriptors:
            return empty_registration_result()
        if payer is None:
            raise ValueError("hashed wallet registration requires an existing funded payer")
        substrate = client._substrate
        finalized = await _finalized_hash(substrate)
        missing = {}
        for account, descriptor in descriptors.items():
            if not await _registered(substrate, account, descriptor, finalized):
                missing[account] = descriptor
        if not missing:
            return WalletRegistrationResult(
                selected, tuple(descriptors), finalized, None, Balance.from_rao(0)
            )

        payer_public = public_view(payer, "coldkey")
        payer_account = payer_public.ss58_address
        if payer_account in missing:
            raise ValueError("an unregistered wallet cannot pay for its own registration")
        if requires_registration(payer_public) and not await _registered(
            substrate, payer_account, bytes(payer_public.hashed_descriptor), finalized
        ):
            raise ValueError("registration payer must already be registered in finalized state")
        for descriptor in missing.values():
            if await descriptor_registration_enabled(substrate, descriptor) is not True:
                raise ValueError("hashed wallet registration is not enabled for this scheme")
        if await substrate.mev_next_key() is None:
            raise ValueError("MEV Shield is unavailable; wallet registration was not submitted")

        head = await substrate.block_hash()
        if not head:
            raise ValueError("could not pin wallet registration reserve")
        # The classical runtime hook accepts one registration, optionally guarded,
        # but does not accept another registration as its batch follow-up.
        groups = (
            [{account: descriptor} for account, descriptor in missing.items()]
            if any(descriptor_storage(descriptor) == "Accounts" for descriptor in missing.values())
            else [missing]
        )
        total = 0
        fee = Balance.from_rao(0)
        transactions = []
        for group in groups:
            registrations = []
            guard_ceiling = None
            for descriptor in group.values():
                deposit_quote = await quote_hashed_registration_deposit(
                    substrate,
                    "MlDsaRegistrationDeposit" if descriptor[0] == 2 else "RegistrationDeposit",
                    block_hash=head,
                )
                reserve = deposit_quote.amount_rao
                if reserve < 0:
                    raise ValueError("wallet registration reserve cannot be negative")
                total += reserve
                if deposit_quote.configurable:
                    guard_ceiling = (
                        reserve if guard_ceiling is None else min(guard_ceiling, reserve)
                    )
                registrations.append(
                    await substrate.compose(
                        calls.HashedAccounts.register(
                            descriptor=registration_descriptor_value(descriptor)
                        )
                    )
                )
            if guard_ceiling is not None:
                # Each transaction binds the approved per-account reserve, even
                # if sudo raises the price between the sequential registrations.
                registrations.insert(
                    0,
                    await substrate.compose(
                        calls.Call(
                            "HashedAccounts",
                            "check_registration_deposit",
                            {"max_deposit": guard_ceiling},
                        )
                    ),
                )
            composed = (
                registrations[0]
                if len(registrations) == 1
                else await substrate.compose(calls.Utility.batch_all(calls=registrations))
            )
            fee += await substrate.estimate_fee(
                composed, payer_public, period=MEV_SHIELD_ERA_PERIOD
            )
            fee += await substrate.estimate_shielded_carrier_fee(
                composed, payer_public, period=MEV_SHIELD_ERA_PERIOD
            )
            transactions.append((composed, group))
        quote = WalletRegistrationQuote(
            payer_account,
            tuple(missing),
            {role: account for role, account in selected.items() if account in missing},
            Balance.from_rao(total),
            fee,
        )
        if max_registration_deposit is not None and total > max_registration_deposit.rao:
            raise ValueError("wallet registration reserve exceeds the approved maximum")
        if max_registration_deposit is None and on_registration_sponsorship is None:
            raise ValueError(
                "wallet registration requires max_registration_deposit or "
                "on_registration_sponsorship approval"
            )
        account = await substrate.query("System", "Account", [payer_account])
        data = (account or {}).get("data") or {}
        minimum = int(await substrate.constant("Balances", "ExistentialDeposit") or 0)
        required = total + fee.rao + max(int(data.get("frozen", 0)), minimum)
        if int(data.get("free", 0)) < required:
            raise ValueError(
                "registration payer has insufficient spendable balance for the permanent "
                "reserve, inner transaction fee and MEV Shield carrier fee"
            )
        if on_registration_sponsorship is not None:
            approved = on_registration_sponsorship(quote)
            if inspect.isawaitable(approved):
                approved = await approved
            if approved is not True:
                raise ValueError("wallet registration sponsorship was declined")

        for composed, group in transactions:
            result = None
            result = await client.submit_call(
                composed,
                payer,
                shielded=True,
                wait_for_inclusion=True,
                wait_for_finalization=True,
            )
            result.raise_for_failure()
            finalized = await _finalized_hash(substrate)
            for account, descriptor in group.items():
                if not await _registered(substrate, account, descriptor, finalized):
                    raise ValueError("wallet registration is not present in finalized state")
        for account, descriptor in descriptors.items():
            if not await _registered(substrate, account, descriptor, finalized):
                raise ValueError("wallet registration is not present in finalized state")
        return WalletRegistrationResult(
            selected, tuple(descriptors), finalized, result, quote.deposit
        )
    except WalletRegistrationError:
        raise
    except Exception as error:
        raise WalletRegistrationError(
            f"Wallet keys are preserved; registration did not complete: {error}",
            wallet=wallet,
            result=result,
            quote=quote,
        ) from error
