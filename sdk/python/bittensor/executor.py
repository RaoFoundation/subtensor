"""The single choke point for executing intents.

Everything that mutates chain state flows through here: build the call once,
simulate the fee, gather predicted effects/warnings, enforce policy, and only
then sign and submit. ``plan`` does all of that except submitting; ``execute``
adds the submission (and refuses if policy is violated).
"""

from __future__ import annotations

import asyncio
import inspect
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
from .result import (
    BittensorError,
    ChainError,
    ExtrinsicResult,
    PolicyError,
    chain_error_from_dispatch,
)
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


async def _compose_intent_call(
    substrate: Substrate,
    intent: Intent,
    wallet: WalletLike,
    *,
    proxy_for: Optional[str] = None,
    proxy_type: Optional[str] = None,
) -> tuple[Any, dict]:
    """Compose semantic call -> sudo -> proxy -> execution adapter."""
    semantic = _coerce_addresses(intent.semantic_intent())
    built = await semantic.build(substrate, wallet)
    if isinstance(built, BuiltCall):
        call, extras = built.call, built.extras
    else:
        call, extras = built, {}

    call = await _wrap_root_call(substrate, semantic, call)
    if proxy_for is not None:
        if proxy_type is not None:
            check_proxy_type(proxy_type)
        call = await substrate.compose(
            generated_calls.Proxy.proxy(real=proxy_for, force_proxy_type=proxy_type, call=call)
        )
        extras = {**extras, "proxy_for": proxy_for}

    wrapped = await intent.wrap_call(substrate, wallet, call)
    if isinstance(wrapped, BuiltCall):
        return wrapped.call, {**extras, **wrapped.extras}
    return wrapped, extras


def _find_event(events: list, module_id: str, event_id: str) -> Optional[Any]:
    """The attributes of the first matching triggered event, or None."""
    for entry in events:
        record = entry.value if hasattr(entry, "value") else entry
        event = record.get("event", record) if isinstance(record, dict) else {}
        if event.get("module_id") == module_id and event.get("event_id") == event_id:
            return event.get("attributes")
    return None


def _event_parts(entry: Any) -> tuple[Optional[str], Optional[str], Any, Optional[int]]:
    """Normalize one decoded event record across transport and test shapes."""
    record = entry.value if hasattr(entry, "value") else entry
    if not isinstance(record, dict):
        return None, None, None, None
    event = record.get("event", record)
    if not isinstance(event, dict):
        return None, None, None, None
    index = record.get("extrinsic_idx")
    try:
        index = int(index) if index is not None else None
    except (TypeError, ValueError):
        index = None
    return event.get("module_id"), event.get("event_id"), event.get("attributes"), index


def _event_netuid(attributes: Any) -> Optional[int]:
    """Read the netuid from named or tuple-style Subtensor events."""
    value = attributes.get("netuid") if isinstance(attributes, dict) else attributes
    if isinstance(value, (list, tuple)):
        value = value[0] if value else None
    try:
        return int(value) if value is not None else None
    except (TypeError, ValueError):
        return None


def _result_block(result: ExtrinsicResult) -> Optional[int]:
    if not result.extrinsic_id:
        return None
    height, _, _ = result.extrinsic_id.partition("-")
    try:
        return int(height)
    except ValueError:
        return None


async def _notify_progress(callback: Any, **progress: Any) -> None:
    if callback is None:
        return
    outcome = callback(progress)
    if inspect.isawaitable(outcome):
        await outcome


async def _cleanup_netuid(substrate: Substrate, block_hash: str) -> Optional[int]:
    """The subnet currently being cleaned, or the next queued for cleanup."""
    status = await substrate.query(
        "SubtensorModule", "CurrentDissolveCleanupStatus", block_hash=block_hash
    )
    if isinstance(status, dict) and status.get("netuid") is not None:
        return int(status["netuid"])
    queue = await substrate.query("SubtensorModule", "DissolveCleanupQueue", block_hash=block_hash)
    if isinstance(queue, (list, tuple)) and queue:
        return int(queue[0])
    return None


async def _complete_subnet_registration(
    substrate: Substrate,
    result: ExtrinsicResult,
    *,
    owner: str,
    hotkey: str,
    on_progress: Any = None,
    timeout: Optional[float] = None,
    deferred_dispatch: bool = False,
) -> ExtrinsicResult:
    """Resolve immediate or queued ``register_network`` to its ``NetworkAdded``.

    A successful register extrinsic can now mean either "created" or merely
    "queued behind subnet dissolution". The receipt decides the fast path. On
    the queued path we follow blocks and only accept a ``NetworkAdded`` whose
    owner and owner-hotkey match this request at that exact block.
    """
    included_at = _result_block(result)
    queued = False
    queued_at: Optional[int] = None
    registration_block: Optional[int] = None
    registration_price_rao: Optional[int] = None
    deregistered_netuid: Optional[int] = None
    cleanup_netuid: Optional[int] = None

    for entry in result.events:
        module, event, attributes, _ = _event_parts(entry)
        if module != "SubtensorModule":
            continue
        if event == "NetworkAdded":
            netuid = _event_netuid(attributes)
            if netuid is not None:
                event_hash = result.block_hash
                if event_hash is None and included_at is not None:
                    event_hash = await substrate.block_hash(included_at)
                locked = await substrate.query(
                    "SubtensorModule", "SubnetLocked", [netuid], block_hash=event_hash
                )
                data = {
                    **result.data,
                    "netuid": netuid,
                    "registration_mode": "immediate",
                    "registered_at_block": included_at,
                }
                if locked is not None:
                    data["registration_price_rao"] = int(locked)
                await _notify_progress(
                    on_progress,
                    stage="registered",
                    mode="immediate",
                    netuid=netuid,
                    block=included_at,
                )
                return replace(result, message=f"Subnet {netuid} registered.", data=data)
        elif event == "NetworkRegistrationQueued":
            queued = True
            queued_at = included_at
            if isinstance(attributes, dict) and attributes.get("registration_block") is not None:
                registration_block = int(attributes["registration_block"])
            if isinstance(attributes, dict) and attributes.get("lock_amount") is not None:
                registration_price_rao = int(attributes["lock_amount"])
        elif event == "NetworkRemoved":
            deregistered_netuid = _event_netuid(attributes)

    # Without the queued event, a normal included register call has no deferred
    # work to follow (e.g. a multisig approval that did not execute the call).
    if not queued and not deferred_dispatch:
        return result

    async def wait() -> ExtrinsicResult:
        nonlocal cleanup_netuid, deregistered_netuid, queued, queued_at
        nonlocal registration_block, registration_price_rao

        # Scan the inclusion block too: on_idle events are not part of the
        # extrinsic receipt, and may follow the register call in that same block.
        first = included_at if included_at is not None else await substrate.block_number()
        last_scanned = first - 1

        async def scan(block: int) -> Optional[ExtrinsicResult]:
            nonlocal cleanup_netuid, deregistered_netuid, queued, queued_at
            nonlocal registration_block, registration_price_rao
            block_hash = await substrate.block_hash(block)
            events = await substrate.events(block_hash)
            target_queue_indices: set[Optional[int]] = set()
            removed_by_index: dict[Optional[int], int] = {}

            for entry in events:
                module, event, attributes, index = _event_parts(entry)
                if module != "SubtensorModule":
                    continue
                if event == "NetworkRegistrationQueued" and isinstance(attributes, dict):
                    if (
                        str(attributes.get("coldkey")) == owner
                        and str(attributes.get("hotkey")) == hotkey
                    ):
                        target_queue_indices.add(index)
                        queued_at = queued_at or block
                        if attributes.get("registration_block") is not None:
                            registration_block = int(attributes["registration_block"])
                        if attributes.get("lock_amount") is not None:
                            registration_price_rao = int(attributes["lock_amount"])
                elif event == "NetworkRemoved":
                    netuid = _event_netuid(attributes)
                    if netuid is not None:
                        removed_by_index[index] = netuid

            if deregistered_netuid is None:
                for index in target_queue_indices:
                    if index in removed_by_index:
                        deregistered_netuid = removed_by_index[index]
                        break

            if target_queue_indices:
                newly_queued = not queued
                queued = True
                if cleanup_netuid is None:
                    cleanup_netuid = deregistered_netuid or await _cleanup_netuid(
                        substrate, block_hash
                    )
                if newly_queued:
                    await _notify_progress(
                        on_progress,
                        stage="queued",
                        block=queued_at,
                        cleanup_netuid=cleanup_netuid,
                        deregistered_netuid=deregistered_netuid,
                    )

            for entry in events:
                module, event, attributes, _ = _event_parts(entry)
                if module != "SubtensorModule" or event != "NetworkAdded":
                    continue
                netuid = _event_netuid(attributes)
                if netuid is None:
                    continue
                actual_owner, actual_hotkey, pending_registrations, locked = await asyncio.gather(
                    substrate.query(
                        "SubtensorModule", "SubnetOwner", [netuid], block_hash=block_hash
                    ),
                    substrate.query(
                        "SubtensorModule", "SubnetOwnerHotkey", [netuid], block_hash=block_hash
                    ),
                    substrate.query(
                        "SubtensorModule", "NetworkRegistrationQueue", block_hash=block_hash
                    ),
                    substrate.query(
                        "SubtensorModule", "SubnetLocked", [netuid], block_hash=block_hash
                    ),
                )
                if str(actual_owner) != owner or str(actual_hotkey) != hotkey:
                    continue
                # Queue processing may skip a failed entry and register a later
                # request. Do not mistake that later subnet for this one merely
                # because it has the same owner/hotkey pair.
                if (
                    queued
                    and registration_block is not None
                    and isinstance(pending_registrations, (list, tuple))
                ):
                    still_pending = any(
                        isinstance(entry, dict)
                        and str(entry.get("coldkey")) == owner
                        and str(entry.get("hotkey")) == hotkey
                        and int(entry.get("registration_block", -1)) == registration_block
                        for entry in pending_registrations
                    )
                    if still_pending:
                        continue
                mode = "after_deregistration" if queued else "immediate"
                data = {
                    **result.data,
                    "netuid": netuid,
                    "registration_mode": mode,
                    "registered_at_block": block,
                }
                if queued_at is not None:
                    data["queued_at_block"] = queued_at
                if queued and cleanup_netuid is not None:
                    data["cleanup_netuid"] = cleanup_netuid
                if queued and deregistered_netuid is not None:
                    data["deregistered_netuid"] = deregistered_netuid
                exact_price = locked if locked is not None else registration_price_rao
                if exact_price is not None:
                    data["registration_price_rao"] = int(exact_price)
                await _notify_progress(
                    on_progress,
                    stage="registered",
                    mode=mode,
                    netuid=netuid,
                    block=block,
                    cleanup_netuid=cleanup_netuid,
                    deregistered_netuid=deregistered_netuid,
                )
                return replace(result, message=f"Subnet {netuid} registered.", data=data)
            return None

        if queued:
            inclusion_hash = await substrate.block_hash(first)
            cleanup_netuid = deregistered_netuid or await _cleanup_netuid(substrate, inclusion_hash)
            await _notify_progress(
                on_progress,
                stage="queued",
                block=queued_at,
                cleanup_netuid=cleanup_netuid,
                deregistered_netuid=deregistered_netuid,
            )

        # Cover blocks produced while the receipt was finalizing before opening
        # the live subscription. Each live head then closes any subscription race
        # by scanning every height since the last one observed.
        head = await substrate.block_number()
        for block in range(first, head + 1):
            completed = await scan(block)
            last_scanned = block
            if completed is not None:
                return completed

        async for update in substrate.blocks():
            header = update.get("header") or {}
            block = int(header["number"])
            for height in range(last_scanned + 1, block + 1):
                completed = await scan(height)
                last_scanned = height
                if completed is not None:
                    return completed
            await _notify_progress(
                on_progress,
                stage="waiting",
                block=block,
                blocks_since_call=max(0, block - (queued_at if queued_at is not None else first)),
                cleanup_netuid=cleanup_netuid,
                deregistered_netuid=deregistered_netuid,
            )
        raise ChainError("block subscription ended before the queued subnet was registered")

    return await asyncio.wait_for(wait(), timeout) if timeout else await wait()


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
        origin. The effective dispatch account (the direct signer or saved
        multisig) must be a registered proxy of ``proxy_for``. ``proxy_type``
        optionally forces the exact proxy type to match (``force_proxy_type``).
        """
        wallet = as_wallet(wallet)
        intent = _coerce_addresses(intent)
        call, extras = await _compose_intent_call(
            self.substrate,
            intent,
            wallet,
            proxy_for=proxy_for,
            proxy_type=proxy_type,
        )
        pub = self._public_keypair(wallet, intent.signer)
        signer_address = pub.ss58_address
        # The account whose state the call actually touches.
        origin = proxy_for or signer_address

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
            spend=intent.semantic_intent().spend(),
            args={k: v for k, v in intent.to_dict().items() if k != "op"},
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
        wait_for_registration: bool = True,
        registration_timeout: Optional[float] = None,
        on_progress: Any = None,
    ) -> ExtrinsicResult:
        """Plan, then sign and submit. Raises ``PolicyError`` if the plan violates
        policy. (To preview without submitting, call ``plan`` instead.)

        With ``proxy_for``, the direct signer or saved multisig dispatches a
        ``Proxy.proxy`` wrapper as ``proxy_for`` — the real account's key never
        touches this machine (see ``plan``).

        ``retries`` resubmits (up to that many extra times, one block apart) when
        the transaction pool rejects with a transient error such as a nonce race
        or "priority is too low". Chain-side dispatch failures are never retried —
        they would fail identically.

        Intents with ``mev_shield_required`` (collateral AMM buys) are redirected
        to :meth:`submit_shielded`; they cannot be submitted in the clear.

        ``register_subnet`` has a second completion boundary: when subnet
        capacity is full the successful extrinsic queues behind multi-block
        dissolution. By default execute waits for the matching ``NetworkAdded``
        event and returns its netuid. Set ``wait_for_registration=False`` to
        return the queue receipt instead. ``registration_timeout`` and the
        optional ``on_progress(dict)`` callback apply only to that wait.
        """
        if intent.semantic_intent().mev_shield_required:
            if proxy_for is not None:
                raise BittensorError(
                    f"{intent.op} must be submitted MEV-shielded and cannot "
                    "wrap a proxied call; sign directly or use submit_shielded"
                )
            return await self.submit_shielded(
                intent,
                wallet,
                policy=policy,
                wait_for_inclusion=wait_for_inclusion,
                wait_for_finalization=wait_for_finalization,
            )
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
        if (
            result.success
            and intent.op == "register_subnet"
            and wait_for_registration
            and (wait_for_inclusion or wait_for_finalization)
        ):
            resolved_intent = _coerce_addresses(intent)
            resolved_wallet = as_wallet(wallet)
            owner = proxy_for or plan.signer_address
            if owner is None:
                raise ChainError("subnet registration signer address is unavailable")
            hotkey = resolved_intent.hotkey_address(
                resolved_wallet, getattr(resolved_intent, "hotkey_ss58", None)
            )
            result = await _complete_subnet_registration(
                self.substrate,
                result,
                owner=owner,
                hotkey=hotkey,
                on_progress=on_progress,
                timeout=registration_timeout,
            )
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
        wait_for_registration: bool = True,
        registration_timeout: Optional[float] = None,
        on_progress: Any = None,
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
        call, extras = await _compose_intent_call(self.substrate, intent, wallet)
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
                data={
                    **result.data,
                    **extras,
                    "shielded": True,
                    "inner_extrinsic_hash": inner_hash,
                },
            )
        if result.success and (wait_for_inclusion or wait_for_finalization):
            # The carrier's success only proves the ciphertext was accepted;
            # the decrypted inner extrinsic executes separately and can still
            # fail. Follow it so success, block, extrinsic id, and explorer
            # link all describe the actual call, not the carrier.
            result = await self._resolve_shielded_inner(result, inner_hash, period)
        if (
            result.success
            and intent.op == "register_subnet"
            and wait_for_registration
            and (wait_for_inclusion or wait_for_finalization)
        ):
            owner = self._public_keypair(wallet, intent.signer).ss58_address
            hotkey = intent.hotkey_address(wallet, getattr(intent, "hotkey_ss58", None))
            result = await _complete_subnet_registration(
                self.substrate,
                result,
                owner=owner,
                hotkey=hotkey,
                on_progress=on_progress,
                timeout=registration_timeout,
                deferred_dispatch=True,
            )
        return result

    async def _resolve_shielded_inner(
        self, outer: ExtrinsicResult, inner_hash: str, period: int
    ) -> ExtrinsicResult:
        """Follow a shielded carrier to the decrypted inner extrinsic's receipt.

        The block author decrypts ``submit_encrypted`` and includes the inner
        extrinsic as a regular extrinsic — normally in the carrier's own block.
        Scan from the carrier's block until the inner hash is found and return
        its receipt (success flag, block, extrinsic id, error) merged with the
        carrier's shield metadata. If the inner extrinsic has not appeared by
        the end of its mortal era, report the submission failed rather than
        letting the carrier's success stand in for it.
        """
        included_at = _result_block(outer)
        if included_at is None:
            return outer  # inclusion was not awaited; nothing to follow
        # The inner extrinsic's era opened at signing, one or two blocks before
        # the carrier's inclusion; past included_at + period it cannot land.
        deadline = included_at + max(1, int(period))
        block = included_at
        # Bounded so a wedged node (block_hash never resolving) cannot hang the
        # follow-up forever: ~4 block-times of slack per remaining block.
        waits_left = 4 * (deadline - included_at + 1)
        while block <= deadline:
            try:
                block_hash = await self.substrate.block_hash(block)
            except Exception:
                block_hash = None
            if not block_hash:
                # Chain head has not reached this height yet.
                waits_left -= 1
                if waits_left <= 0:
                    break
                await asyncio.sleep(await self.substrate.block_time())
                continue
            inner = await self.substrate.find_extrinsic(inner_hash, block_hash)
            if inner is not None:
                return replace(inner, data={**inner.data, **outer.data})
            block += 1
        message = (
            "the MEV shield accepted the encrypted submission, but the decrypted "
            f"extrinsic ({inner_hash}) was not included before its era expired"
        )
        return replace(
            outer,
            success=False,
            message=message,
            error=ChainError(message),
        )

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
