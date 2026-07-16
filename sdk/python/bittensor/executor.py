"""The single choke point for executing intents.

Everything that mutates chain state flows through here: build the call once,
simulate the fee, gather predicted effects/warnings, enforce policy, and only
then sign and submit. ``plan`` does all of that except submitting; ``execute``
adds the submission (and refuses if policy is violated).
"""

from __future__ import annotations

import asyncio
from dataclasses import fields as dataclass_fields
from dataclasses import replace
from typing import Any, Optional

# Module import + attribute access (not `from bittensor_core import ...`):
# ty cannot see into the compiled extension, so named imports fail its check.
import bittensor_core as _core

from ._generated import calls as generated_calls
from ._substrate import Substrate
from ._transport.contract import UnsignedExtrinsic
from ._transport.utils.receipt import nested_dispatch_error
from .fee_filters import COLDKEY_FEE_WARNING, charges_coldkey_fee
from .intents import Intent, Plan, Policy, list_tools
from .intents import build as build_intent
from .intents.base import BuiltCall
from .intents.proxy import check_proxy_type
from .result import ChainError, ExtrinsicResult, PolicyError, chain_error_from_dispatch
from .settings import DEFAULT_ERA_PERIOD, MEV_SHIELD_ERA_PERIOD
from .signing import (
    WalletLike,
    as_wallet,
    coerce_address,
    is_address_param,
    public_view,
    resolve_signer,
)

# Transaction-pool rejections that resolve themselves within a block or so (a
# competing extrinsic at the same nonce, or a race against pool state). Worth
# resubmitting; a fresh nonce is fetched on every attempt.
_TRANSIENT_SUBSTRINGS = (
    "priority is too low",
    "transaction is outdated",
    "stale",
)


def _is_transient(result: ExtrinsicResult) -> bool:
    message = (result.message or "").lower()
    return any(needle in message for needle in _TRANSIENT_SUBSTRINGS)


def _is_sudo_call(call: Any) -> bool:
    """Whether ``call`` is already a composed ``Sudo.sudo`` wrapper."""
    module = getattr(call, "module", None) or getattr(call, "call_module", None)
    function = getattr(call, "function", None) or getattr(call, "call_function", None)
    return module == "Sudo" and function == "sudo"


async def _wrap_root_call(substrate: Substrate, intent: Intent, call: Any) -> Any:
    """Nest root-origin intents in ``Sudo.sudo`` so privilege matches ``execute``."""
    if intent.origin == "root" and not _is_sudo_call(call):
        return await substrate.compose(generated_calls.Sudo.sudo(call=call))
    return call


def _coerce_addresses(intent: Intent) -> Intent:
    """Normalize the intent's ``*_ss58`` / ``*_ss58s`` fields: a ``Wallet``,
    keypair, or signer passed where an address string is expected becomes its
    ss58 address (hotkey fields take the wallet's hotkey, others its coldkey).
    Returns a new intent only when something needed coercing."""
    changes = {}
    for field in dataclass_fields(intent):
        if not is_address_param(field.name):
            continue
        value = getattr(intent, field.name)
        if value is None or isinstance(value, str):
            continue
        coerced = coerce_address(value, field.name)
        if coerced is not value:
            changes[field.name] = coerced
    return replace(intent, **changes) if changes else intent


def _find_event(events: list, module_id: str, event_id: str) -> Optional[Any]:
    """The attributes of the first matching triggered event, or None."""
    for entry in events:
        record = entry.value if hasattr(entry, "value") else entry
        event = record.get("event", record) if isinstance(record, dict) else {}
        if event.get("module_id") == module_id and event.get("event_id") == event_id:
            return event.get("attributes")
    return None


def _pure_created_data(result: ExtrinsicResult) -> dict[str, Any]:
    """The spawned account and creation coordinates of a ``create_pure_proxy``.

    The chain only reports the derived pure address through the
    ``Proxy.PureCreated`` event, and ``kill_pure`` later demands the creation
    block height and extrinsic index — so all three are captured here, where
    the submission receipt still has them.
    """
    data: dict[str, Any] = {}
    attributes = _find_event(result.events, "Proxy", "PureCreated")
    if isinstance(attributes, dict):
        if attributes.get("pure") is not None:
            data["pure_proxy"] = str(attributes["pure"])
        if attributes.get("who") is not None:
            data["spawner"] = str(attributes["who"])
    if result.extrinsic_id:
        height, _, ext_index = result.extrinsic_id.partition("-")
        try:
            data["height"] = int(height)
            data["ext_index"] = int(ext_index)
        except ValueError:
            pass  # cosmetic identifier in an unexpected shape
    return data


class Executor:
    def __init__(self, substrate: Substrate, policy: Optional[Policy] = None):
        self.substrate = substrate
        self.policy = policy

    @staticmethod
    def _public_keypair(wallet: WalletLike, signer: str):
        """The signer's public keypair — enough to address and to estimate fees,
        without unlocking the private coldkey."""
        return public_view(wallet, signer)

    # Policy: the one enforcement point ---------------------------------------
    #
    # Every path that can reach the chain funnels its policy decision through
    # these helpers, so a new Policy rule is honored everywhere or nowhere.

    def _active_policy(self, policy: Optional[Policy]) -> Optional[Policy]:
        """The call-level override, else the client-wide policy."""
        return policy or self.policy

    def _violations(self, intent: Intent, fee: Any, policy: Optional[Policy]) -> list[str]:
        active = self._active_policy(policy)
        return active.check(intent, fee) if active else []

    def _enforce(self, intent: Intent, fee: Any, policy: Optional[Policy]) -> None:
        violations = self._violations(intent, fee, policy)
        if violations:
            raise PolicyError(violations)

    def _enforce_raw_call(self, policy: Optional[Policy]) -> None:
        active = self._active_policy(policy)
        violations = active.check_raw_call() if active else []
        if violations:
            raise PolicyError(violations)

    async def plan(
        self,
        intent: Intent,
        wallet: WalletLike,
        *,
        policy: Optional[Policy] = None,
        proxy_for: Optional[str] = None,
        proxy_type: Optional[str] = None,
    ) -> Plan:
        """Dry-run: build the call, simulate the fee, and check policy. No submit.

        ``proxy_for`` switches to proxy signing: the call is wrapped in
        ``Proxy.proxy(real=proxy_for)`` so it dispatches with that account's
        origin, while the *local* wallet key (which must be a registered proxy of
        ``proxy_for``) signs. ``proxy_type`` optionally forces the exact proxy
        type to match (``force_proxy_type``).
        """
        wallet = as_wallet(wallet)
        intent = _coerce_addresses(intent)
        built = await intent.build(self.substrate, wallet)
        if isinstance(built, BuiltCall):
            call, extras = built.call, built.extras
        else:
            call, extras = built, {}
        # Root intents declare privilege via ``origin``; wrap here so metadata
        # and execution cannot drift (an intent that forgets Sudo.sudo still
        # dispatches as root, and docs stay authoritative).
        call = await _wrap_root_call(self.substrate, intent, call)
        pub = self._public_keypair(wallet, intent.signer)
        signer_address = pub.ss58_address
        # The account whose state the call actually touches.
        origin = proxy_for or signer_address

        if proxy_for is not None:
            if proxy_type is not None:
                check_proxy_type(proxy_type)
            call = await self.substrate.compose(
                generated_calls.Proxy.proxy(real=proxy_for, force_proxy_type=proxy_type, call=call)
            )
            extras = {**extras, "proxy_for": proxy_for}

        warnings: list[str] = list(await intent.warnings(self.substrate, origin))
        if intent.signer == "hotkey" and proxy_for is None and charges_coldkey_fee(call):
            warnings.append(COLDKEY_FEE_WARNING)
        fee = None
        try:
            fee = await self.substrate.estimate_fee(call, pub)
        except Exception as error:  # fee estimation is best-effort
            warnings.append(f"could not estimate fee: {error}")

        effects = list(await intent.effects(self.substrate, origin))
        if proxy_for is not None:
            effects.append(f"dispatched via proxy as {proxy_for} (signed by {signer_address})")
        violations = self._violations(intent, fee, policy)

        return Plan(
            op=intent.op,
            summary=intent.summary(),
            signer=intent.signer,
            signer_address=signer_address,
            fee=fee,
            effects=effects,
            warnings=warnings,
            violations=violations,
            call=call,
            extras=extras,
        )

    async def execute(
        self,
        intent: Intent,
        wallet: WalletLike,
        *,
        policy: Optional[Policy] = None,
        proxy_for: Optional[str] = None,
        proxy_type: Optional[str] = None,
        period: Optional[int] = DEFAULT_ERA_PERIOD,
        wait_for_inclusion: bool = True,
        wait_for_finalization: bool = True,
        retries: int = 0,
    ) -> ExtrinsicResult:
        """Plan, then sign and submit. Raises ``PolicyError`` if the plan violates
        policy. (To preview without submitting, call ``plan`` instead.)

        With ``proxy_for``, the local wallet key signs a ``Proxy.proxy`` wrapper
        and the call dispatches as ``proxy_for`` — the real account's key never
        touches this machine (see ``plan``).

        ``retries`` resubmits (up to that many extra times, one block apart) when
        the transaction pool rejects with a transient error such as a nonce race
        or "priority is too low". Chain-side dispatch failures are never retried —
        they would fail identically.
        """
        plan = await self.plan(
            intent, wallet, policy=policy, proxy_for=proxy_for, proxy_type=proxy_type
        )
        if not plan.ok:
            raise PolicyError(plan.violations)

        keypair = resolve_signer(wallet, intent.signer)
        attempts = max(0, int(retries)) + 1
        for attempt in range(attempts):
            result = await self.substrate.submit(
                plan.call,
                keypair,
                period=period,
                wait_for_inclusion=wait_for_inclusion,
                wait_for_finalization=wait_for_finalization,
            )
            if result.success or attempt == attempts - 1 or not _is_transient(result):
                break
            # One block, as the chain measures it (0.25s on fast-blocks localnets).
            await asyncio.sleep(await self.substrate.block_time())
        # Defense for backends that mark ExtrinsicSuccess without decoding
        # nested Sudo/Proxy/Multisig Results (e.g. in-memory fakes). The RPC
        # path already fails these in resolve_outcome.
        if result.success:
            inner_error = nested_dispatch_error(result.events)
            if inner_error is not None:
                error = chain_error_from_dispatch(inner_error)
                result = replace(
                    result,
                    success=False,
                    message=f"nested call failed: {error.message}",
                    error=error,
                )
        if result.success:
            data = dict(result.data)
            if intent.op == "create_pure_proxy":
                data.update(_pure_created_data(result))
            data.update(plan.extras)
            if data != result.data:
                result = replace(result, data=data)
        return result

    async def execute_tool(
        self, op: str, args: dict, wallet: WalletLike, **kwargs
    ) -> ExtrinsicResult:
        """Build an intent by name from a dict of args, then execute it."""
        return await self.execute(build_intent(op, args), wallet, **kwargs)

    async def submit_shielded(
        self,
        intent: Intent,
        wallet: WalletLike,
        *,
        policy: Optional[Policy] = None,
        period: int = MEV_SHIELD_ERA_PERIOD,
        wait_for_inclusion: bool = True,
        wait_for_finalization: bool = False,
    ) -> ExtrinsicResult:
        """Submit an intent MEV-shielded via the MevShield pallet.

        The intent's call is signed as an inner extrinsic (at nonce+1), encrypted
        with the chain's rotating ML-KEM-768 key (``NextKey``), and carried inside
        ``MevShield.submit_encrypted`` (signed at nonce). It stays encrypted in the
        pool until the block author decrypts and executes it, so the mempool can't
        front-run it. Policy is enforced on the intent before anything is signed;
        ``max_fee_tao`` is checked against the inner call's estimated fee (the
        outer carrier extrinsic pays its own small fee on top).
        """
        wallet = as_wallet(wallet)
        intent = _coerce_addresses(intent)
        built = await intent.build(self.substrate, wallet)
        call = built.call if isinstance(built, BuiltCall) else built
        # Same root wrapping as ``plan``/``execute``: the decrypted inner
        # extrinsic must dispatch ``Sudo.sudo``, not the bare AdminUtils call.
        call = await _wrap_root_call(self.substrate, intent, call)
        fee = None
        active = self._active_policy(policy)
        if active is not None and active.max_fee_tao is not None:
            # A fee guardrail must not fail open: if the estimate is
            # unavailable the submission is blocked, unlike ``plan`` where a
            # failed estimate only warns.
            try:
                fee = await self.substrate.estimate_fee(
                    call, self._public_keypair(wallet, intent.signer)
                )
            except Exception as error:
                raise PolicyError(
                    [f"could not estimate fee to enforce max_fee_tao: {error}"]
                ) from error
        self._enforce(intent, fee, policy)

        # NextKey rotates every block and is legitimately absent for a beat
        # when an upcoming author hasn't announced its key yet (always the
        # case in a chain's first blocks). Wait it out for a couple of
        # blocks before concluding the pallet is inactive.
        pubkey = await self.substrate.mev_next_key()
        for _ in range(2):
            if pubkey:
                break
            await asyncio.sleep(await self.substrate.block_time())
            pubkey = await self.substrate.mev_next_key()
        if not pubkey:
            raise ChainError("MEV Shield NextKey not available; is the MevShield pallet active?")

        keypair = resolve_signer(wallet, intent.signer)
        nonce = await self.substrate.account_next_index(keypair.ss58_address)
        inner_bytes, inner_hash = await self.substrate.sign_extrinsic(
            call, keypair, nonce=nonce + 1, period=period
        )
        ciphertext = _core.encrypt_mlkem768(pubkey, inner_bytes, include_key_hash=True)
        outer = await self.substrate.compose(
            generated_calls.MevShield.submit_encrypted(ciphertext=ciphertext)
        )
        result = await self.substrate.submit(
            outer,
            keypair,
            nonce=nonce,
            period=period,
            wait_for_inclusion=wait_for_inclusion,
            wait_for_finalization=wait_for_finalization,
        )
        if result.success:
            result = replace(
                result,
                data={**result.data, "shielded": True, "inner_extrinsic_hash": inner_hash},
            )
        return result

    async def submit_call(
        self,
        call,
        wallet: WalletLike,
        *,
        signer: str = "coldkey",
        policy: Optional[Policy] = None,
        period: Optional[int] = DEFAULT_ERA_PERIOD,
        wait_for_inclusion: bool = True,
        wait_for_finalization: bool = True,
    ) -> ExtrinsicResult:
        """Escape hatch: sign and submit a generated raw call with no intent wrapper.

        ``call`` is any builder from ``bittensor.calls`` (every extrinsic the chain
        exposes, including ones no intent wraps). There is no plan/preview, so an
        active policy cannot bound the spend — raw calls are refused unless the
        policy sets ``allow_raw_calls=True``. No policy means no restriction,
        exactly as for intents.
        """
        self._enforce_raw_call(policy)
        composed = await self.substrate.compose(call)
        keypair = resolve_signer(wallet, signer)
        return await self.substrate.submit(
            composed,
            keypair,
            period=period,
            wait_for_inclusion=wait_for_inclusion,
            wait_for_finalization=wait_for_finalization,
        )

    async def prepare_call(
        self,
        call,
        *,
        address: str,
        crypto_type: int = 1,
        nonce: Optional[int] = None,
        period: Optional[int] = DEFAULT_ERA_PERIOD,
        tip: int = 0,
        metadata_hash: Optional[bytes] = None,
    ) -> UnsignedExtrinsic:
        """Build an unsigned extrinsic for signing somewhere else entirely.

        The first half of the offline flow (QR / air-gapped vault / hardware
        device on another machine): no key material is needed, only the
        signing account's ``address``. ``call`` is a generated call builder
        (``bittensor.calls``) or an already-composed call — a ``Plan.call``
        from :meth:`plan` works, so intents can be prepared too.

        Hand the resulting payload to the external signer, then submit its
        signature with :meth:`submit_signature`. Mortal eras are the deadline:
        with the default period the signature must come back within ~64
        blocks; pass a longer ``period`` (or ``None`` for an immortal
        extrinsic) when the round-trip is slower than that.
        """
        composed = call if hasattr(call, "data") else await self.substrate.compose(call)
        return await self.substrate.prepare(
            composed,
            address=address,
            crypto_type=crypto_type,
            nonce=nonce,
            period=period,
            tip=tip,
            metadata_hash=metadata_hash,
        )

    async def submit_signature(
        self,
        unsigned: UnsignedExtrinsic,
        signature: bytes | str,
        *,
        policy: Optional[Policy] = None,
        wait_for_inclusion: bool = True,
        wait_for_finalization: bool = True,
    ) -> ExtrinsicResult:
        """Submit an externally-produced signature for a prepared extrinsic.

        The second half of the offline flow. Policy-gated like
        :meth:`submit_call`: the prepared call is opaque bytes, so an active
        policy must allow raw calls for this to pass.
        """
        self._enforce_raw_call(policy)
        return await self.substrate.submit_signature(
            unsigned,
            signature,
            wait_for_inclusion=wait_for_inclusion,
            wait_for_finalization=wait_for_finalization,
        )

    def tools(self) -> list[dict]:
        """The machine-readable catalog of every executable operation."""
        return list_tools()
