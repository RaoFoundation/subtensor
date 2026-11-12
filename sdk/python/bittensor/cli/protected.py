"""Protected-account CLI helpers: setup, auto-announce, show/cancel/execute/rotate."""

from __future__ import annotations

import contextlib
from datetime import datetime, timedelta
from typing import Any, Optional

import typer

from .. import protected_account as pa
from .. import proxy_announcements as ann
from ..executor import _event_parts
from ..intents import (
    AddProxy,
    AnnounceColdkeySwap,
    AnnounceProxyCall,
    Batch,
    CreatePureProxy,
    ExecuteProxyAnnounced,
    RemoveProxies,
    RemoveProxy,
    RemoveProxyAnnouncement,
    Transfer,
)
from ..intents.proxy import compose_announceable_call
from ..receiving import is_receiving_address, parse_recipient
from ..settings import BLOCKTIME
from ..timelock import format_duration
from .context import AppContext

# Intents that must never be auto-routed through the protected pure proxy.
# Setup / announce plumbing / coldkey-swap-from-controller stay on the wallet.
PROTECTED_BYPASS_OPS = frozenset(
    {
        "announce_proxy_call",
        "remove_proxy_announcement",
        "reject_proxy_announcement",
        "execute_proxy_announced",
        "create_pure_proxy",
        "announce_coldkey_swap",
        "swap_coldkey_announced",
        "clear_coldkey_swap_announcement",
        "dispute_coldkey_swap",
        "release_own_coldkey_swap_destination_reservation",
    }
)

# These calls can orphan the keyless account or install immediate authority.
# Protected-account changes must go through protect/rotate, whose invariants
# reconcile the complete grant set and delayed queue.
PROTECTED_MANAGEMENT_OPS = frozenset(
    {"add_proxy", "remove_proxy", "remove_proxies", "kill_pure_proxy"}
)


def _account_identity(address: str) -> str:
    """Use account identities for authority checks, retaining descriptors for writes."""
    return parse_recipient(address).account if is_receiving_address(address) else address


def _normalize_record(record: dict[str, Any], *, persist: bool = True) -> dict[str, Any]:
    normalized = dict(record)
    if is_receiving_address(normalized.get("allow_key_ss58")):
        normalized.setdefault("allow_key_address", normalized["allow_key_ss58"])
    for field in ("controller_ss58", "allow_key_ss58", "protected_ss58"):
        if normalized.get(field):
            normalized[field] = _account_identity(normalized[field])
    if persist and normalized != record:
        pa.upsert_protected(normalized)
    return normalized


def _chain_genesis_hash(app_ctx: AppContext) -> str:
    """Identify the selected chain independently of endpoint aliases."""
    network = app_ctx.network
    cached = getattr(app_ctx, "_protected_chain", None)
    if cached is not None and cached[0] == network:
        return cached[1]
    genesis_hash = app_ctx.run(lambda client: client._substrate.block_hash(0)).lower()
    if len(genesis_hash) != 66 or not genesis_hash.startswith("0x"):
        raise ValueError("chain returned an invalid genesis hash")
    bytes.fromhex(genesis_hash[2:])
    app_ctx._protected_chain = (network, genesis_hash)
    return genesis_hash


def _get_protected_record(
    app_ctx: AppContext,
    wallet_name: Optional[str] = None,
    *,
    require_ready: bool = False,
) -> Optional[dict[str, Any]]:
    wallet_name = wallet_name or app_ctx.wallet_name
    # Ordinary wallets need no extra connection just to check automatic routing.
    if not any(e.get("controller_wallet") == wallet_name for e in pa.load_protected()):
        return None
    record = pa.get_by_controller(wallet_name, genesis_hash=_chain_genesis_hash(app_ctx))
    if record is not None:
        if require_ready and pa.protected_setup_pending(record):
            app_ctx.output.error(
                "protected-account setup is incomplete; resume it before sending transactions",
                help=f"run `btcli proxy protect --wallet {wallet_name}` on this network",
            )
            raise typer.Exit(2)
        return _normalize_record(record, persist=not getattr(app_ctx, "dry_run", False))
    if pa.get_by_controller(wallet_name) is not None:
        app_ctx.output.error(
            "this protected-account binding has no network identity",
            help="select its original network and run `btcli proxy protect "
            "--bind-network --wallet ...` before using it",
        )
        raise typer.Exit(2)
    return None


def require_wallet_name(app_ctx: AppContext) -> str:
    name = app_ctx.wallet_name
    if not name:
        app_ctx.output.error(
            "--wallet / -w is required for protected-account commands",
            help="pass -w <controller-wallet>",
        )
        raise typer.Exit(2)
    return name


def _eta_for_block(executable_at: int, current_block: int) -> str:
    remaining = executable_at - current_block
    if remaining <= 0:
        return "ready now"
    wait = timedelta(seconds=remaining * BLOCKTIME)
    eta = datetime.now().astimezone() + wait
    if wait >= timedelta(minutes=1):
        rounded = timedelta(minutes=round(wait.total_seconds() / 60))
    else:
        rounded = wait
    return f"~{format_duration(rounded)} ({eta:%Y-%m-%d %H:%M %Z})"


def print_queued(
    app_ctx: AppContext,
    *,
    record: dict[str, Any],
    row: dict[str, Any],
    current_block: int,
) -> None:
    name = record.get("name") or pa.default_protected_name(record["controller_wallet"])
    ss58 = record["protected_ss58"]
    executable_at = int(row["executable_at_block"])
    eta = _eta_for_block(executable_at, current_block)
    index = int(row["index"])
    wallet = record["controller_wallet"]
    app_ctx.output.message(
        f"Protected account: {name} ({ss58})\n"
        f"Queued #{index}, executes after block {executable_at} ({eta})\n"
        f"Cancel with: btcli proxy cancel --index {index} --wallet {wallet}"
    )


def maybe_route_protected(
    app_ctx: AppContext,
    intent,
    *,
    proxy_for: Optional[str],
    user_set_proxy: bool,
) -> tuple[Optional[str], Optional[str], bool]:
    """Decide auto-routing for a wallet that controls a protected account.

    Returns ``(proxy_for, force_proxy_type, announced)``. When ``announced`` is
    True the caller must stop — the announcement was already submitted.
    """
    if user_set_proxy or proxy_for is not None:
        return proxy_for, None, False
    if intent.signer == "hotkey" or intent.op in PROTECTED_BYPASS_OPS:
        return proxy_for, None, False
    wallet_name = app_ctx.wallet_name
    if not wallet_name:
        return proxy_for, None, False
    record = _get_protected_record(app_ctx, wallet_name, require_ready=True)
    if record is None:
        return proxy_for, None, False
    if intent.op in PROTECTED_MANAGEMENT_OPS:
        app_ctx.output.error(
            f"refusing to auto-route {intent.op} to keyless protected account "
            f"{record['protected_ss58']}",
            help="use `btcli proxy protect --allow/--disallow` or `btcli proxy rotate`; "
            "ordinary proxy management can orphan the account or install immediate authority",
        )
        raise typer.Exit(2)
    record = _refresh_live_allows(app_ctx, record)
    dispatcher = record.get("controller_ss58") or _controller_ss58(app_ctx)
    instant = pa.instant_proxy_type_for(
        intent.op, record, dispatcher_ss58=dispatcher, intent=intent
    )
    if intent.op == "pow_register" and instant != "Registration":
        app_ctx.output.error(
            "PoW registration cannot use a protected account's delayed controller: "
            "the proof expires after five blocks, long before the announcement delay",
            help="PoW requires an already-installed zero-delay Registration proxy; "
            "Registration is intentionally not offered by `btcli proxy protect --allow`",
        )
        raise typer.Exit(2)
    funded_intent = _offer_operation_refill(app_ctx, record, intent)
    if funded_intent is not intent:
        # A Staking allow cannot authorize a transfer. The user accepted an
        # explicitly delayed Any batch, not an instant spending permission.
        intent = funded_intent
        instant = None
    if instant is not None:
        return record["protected_ss58"], instant, False

    # Delayed path: announce instead of executing.
    _submit_protected_announcement(app_ctx, intent, record)
    return None, None, True


def protected_pow_origin(app_ctx: AppContext) -> Optional[str]:
    """Return the pure origin for a usable instant PoW route, or reject delay."""
    if not app_ctx.wallet_name:
        return None
    record = _get_protected_record(app_ctx, require_ready=True)
    if record is None:
        return None
    record = _refresh_live_allows(app_ctx, record)
    dispatcher = record.get("controller_ss58") or _controller_ss58(app_ctx)
    if (
        pa.instant_proxy_type_for("pow_register", record, dispatcher_ss58=dispatcher, intent=None)
        != "Registration"
    ):
        app_ctx.output.error(
            "PoW registration cannot wait behind this protected account's controller delay",
            help="the proof is valid for only five blocks; use burn registration or an "
            "explicitly installed zero-delay Registration proxy",
        )
        raise typer.Exit(2)
    return record["protected_ss58"]


def _controller_funding(app_ctx: AppContext, record: dict[str, Any]) -> dict[str, Any]:
    from ..protected_funding import controller_funding
    from ..sp_core import Keypair, ss58_decode

    try:
        # Inspection must work without opening a local signing wallet. Query
        # fees using an unsigned standard-signature envelope for this address.
        public = Keypair(public_key=bytes(ss58_decode(record["controller_ss58"])), crypto_type=1)
        state = app_ctx.run(lambda client: controller_funding(client, record, public))
        return state.to_dict()
    except Exception as error:
        # Funding is advisory. An unavailable estimate must be visible, but
        # must not disable cancellation or access to a previously saved queue.
        return {"controller": record["controller_ss58"], "available": False, "error": str(error)}


def _funding_warning(app_ctx: AppContext, state: dict[str, Any]) -> None:
    from ..balance import Balance

    if "available_rao" not in state:
        app_ctx.output.message(
            "[bold red]WARNING: controller funding could not be checked: "
            f"{state.get('error', 'unknown error')}[/bold red]"
        )
        return
    app_ctx.output.message(
        "[bold red]WARNING: controller funds are low. Available: "
        f"{Balance.from_rao(state['available_rao'])}; recommended: "
        f"{Balance.from_rao(state['target_rao'])}. "
        "Fees and refundable announcement deposits are paid outside the pure account."
        "[/bold red]"
    )


def _offer_operation_refill(app_ctx: AppContext, record: dict[str, Any], intent):
    from ..balance import Balance
    from .prompt import interactive

    operations = {
        "transfer",
        "transfer_all",
        "transfer_stake",
        "transfer_stake_and_hotkey",
        "move_stake_limit",
        "swap_stake_limit",
        "move_swap_stake",
        "batch",
        *pa.PROXY_TYPE_INTENT_OPS["Staking"],
    }
    if intent.op not in operations or app_ctx.dry_run:
        return intent
    if getattr(app_ctx, "no_controller_refill", False):
        return intent
    state = _controller_funding(app_ctx, record)
    if "available_rao" not in state:
        _funding_warning(app_ctx, state)
        return intent
    projected = (
        state["available_rao"]
        - state["next_announcement_deposit_rao"]
        - max(state["transfer_fee_rao"], state["staking_fee_rao"])
    )
    if not state["low"] and projected >= state["warning_rao"]:
        return intent
    _funding_warning(app_ctx, state)
    if not state["can_announce"]:
        app_ctx.output.message(
            "Top up the controller from another wallet first; it cannot fund a new announcement. "
            f"Use `btcli proxy fund --wallet {record['controller_wallet']} "
            "--from-wallet FUNDING_WALLET`."
        )
        return intent
    pending_refill = _pending_controller_refill_rao(app_ctx, record)
    if pending_refill:
        app_ctx.output.message(
            f"Controller refill already pending: {Balance.from_rao(pending_refill)}; "
            "not stacking another refill. Execute or cancel the existing announcement first."
        )
        return intent
    maximum = getattr(app_ctx, "max_controller_refill", None)
    if maximum is not None and state["top_up_rao"] > maximum.rao:
        app_ctx.output.error(
            f"automatic controller refill {Balance.from_rao(state['top_up_rao'])} exceeds "
            f"--max-controller-refill {maximum}",
            help="raise the explicit ceiling, fund the controller externally, or pass "
            "--no-refill-controller",
        )
        raise typer.Exit(2)
    amount = Balance.from_rao(state["top_up_rao"])
    app_ctx.output.message(
        f"Offer: include a {amount} refill to {record['controller_ss58']} in the same "
        "Utility.batch_all as this operation. The whole batch waits for the controller "
        "delay and executes with one signature; this also delays otherwise instant staking."
    )
    accepted = app_ctx.assume_yes or (
        interactive(app_ctx) and app_ctx.output.confirm("Include the delayed controller refill?")
    )
    if not accepted:
        app_ctx.output.message("Controller refill declined; continuing without replenishment.")
        return intent
    children = intent.intents if isinstance(intent, Batch) else [intent.to_dict()]
    refill = Transfer(dest_ss58=record["controller_ss58"], amount_tao=amount)
    unstaking = {"remove_stake", "remove_stake_limit", "unstake_all", "unstake_all_alpha"}
    release_ops = unstaking | {"claim_root", "claim_root_with_hotkey"}
    last_release = max(
        (index for index, child in enumerate(children) if child["op"] in release_ops),
        default=-1,
    )
    if last_release >= 0:
        # Preserve the user's child order while ensuring every requested
        # release that may fund the refill runs before the transfer.
        return Batch(
            intents=[
                *children[: last_release + 1],
                refill.to_dict(),
                *children[last_release + 1 :],
            ]
        )
    return Batch(intents=[refill.to_dict(), *children])


def _pending_controller_refill_rao(app_ctx: AppContext, record: dict[str, Any]) -> int:
    """Total TAO already committed from the pure account to its controller."""
    from ..balance import Balance

    controller = _account_identity(record["controller_ss58"])

    def visit(encoded: dict[str, Any]) -> int:
        if (
            encoded.get("op") == "transfer"
            and _account_identity(encoded.get("dest_ss58")) == controller
        ):
            amount = encoded.get("amount_tao")
            if amount == "all":
                return 0
            try:
                return Balance.from_tao(str(amount)).rao
            except (TypeError, ValueError):
                return 0
        if encoded.get("op") == "batch":
            return sum(visit(child) for child in encoded.get("intents") or [])
        return 0

    if all(hasattr(app_ctx, field) for field in ("network", "run")) and record.get("genesis_hash"):
        rows = _reconcile_pending(app_ctx, record)
    else:
        # Pure policy/unit callers can still inspect the locally committed
        # queue without manufacturing a network client.
        rows = ann.list_for_wallet(
            record["controller_wallet"], genesis_hash=record.get("genesis_hash")
        )
    return sum(
        visit({"op": row.get("inner_op"), **(row.get("inner_args") or {})})
        for row in rows
        if not row.get("plaintext_missing")
    )


def run_fund(app_ctx: AppContext, *, from_wallet: str) -> None:
    """Fund the controller using a separate wallet after its coldkey migration."""
    record = _get_protected_record(app_ctx, require_ready=True)
    if record is None:
        raise typer.Exit(2)
    _live_controller_delay(app_ctx, record, require_grant=True)
    controller = record["controller_ss58"]
    pending = app_ctx.run(lambda c: c.read("coldkey_swap_announcement", coldkey_ss58=controller))
    if pending is not None:
        app_ctx.output.error("complete the coldkey migration before topping up its controller")
        raise typer.Exit(2)
    state = _controller_funding(app_ctx, record)
    if "top_up_rao" not in state:
        _funding_warning(app_ctx, state)
        raise typer.Exit(1)
    if not state["top_up_rao"]:
        app_ctx.output.message("Controller already has the recommended funding.")
        return
    from ..balance import Balance

    with _preserve_controller_context(app_ctx, record):
        app_ctx.wallet_name = from_wallet
        app_ctx.multisig_wallet_name = None
        app_ctx.signatory_wallet = None
        app_ctx.signatory_wallets = []
        if _account_identity(_controller_ss58(app_ctx)) == record["controller_ss58"]:
            app_ctx.output.error("choose a different funding wallet")
            raise typer.Exit(2)
        result = app_ctx.submit(
            Transfer(
                dest_ss58=record["controller_ss58"],
                amount_tao=Balance.from_rao(state["top_up_rao"]),
            ),
            proxy_for="self",
            summary="fund protected-account controller",
        )
    if result is None or not result.success:
        raise typer.Exit(1)


def offer_post_migration_funding(
    app_ctx: AppContext, destination: str, *, controller_wallet: Optional[str] = None
) -> None:
    from .prompt import interactive

    record = _get_protected_record(app_ctx, controller_wallet)
    if record is None or _account_identity(destination) != record["protected_ss58"]:
        return
    # Multisig submission may have replaced wallet_name with the member that
    # supplied the last approval. Follow-up lookup and funding must continue
    # in the protected controller's namespace.
    app_ctx.wallet_name = record["controller_wallet"]
    app_ctx.multisig_wallet_name = record["controller_wallet"]
    controller = record["controller_ss58"]
    pending = app_ctx.run(lambda c: c.read("coldkey_swap_announcement", coldkey_ss58=controller))
    if pending is not None:
        return  # A successful multisig approval may not have executed the swap.
    state = _controller_funding(app_ctx, record)
    if not state.get("low", True):
        return
    _funding_warning(app_ctx, state)
    app_ctx.output.message(
        "Fund the controller now from a different wallet with "
        f"`btcli proxy fund --wallet {record['controller_wallet']} --from-wallet FUNDING_WALLET`."
    )
    if interactive(app_ctx) and app_ctx.output.confirm(
        "Fund the controller now from another wallet?"
    ):
        source = typer.prompt("Funding wallet")
        run_fund(app_ctx, from_wallet=source)


def _refresh_live_allows(app_ctx: AppContext, record: dict[str, Any]) -> dict[str, Any]:
    """Choose instant routes from current grants, never a cached permission list."""
    delegate = record.get("allow_key_ss58") or record.get("controller_ss58")
    delegate = delegate or _controller_ss58(app_ctx)
    data = app_ctx.run(lambda c: c.read("proxies", coldkey_ss58=record["protected_ss58"]))
    allows = [
        str(grant["proxy_type"])
        for grant in pa.allow_key_grants(data["proxies"], delegate)
        if grant["proxy_type"] in pa.PROXY_TYPE_INTENT_OPS
    ]
    latest = pa.get_by_controller(record["controller_wallet"], genesis_hash=record["genesis_hash"])
    if (
        latest is not None
        and latest["protected_ss58"] == record["protected_ss58"]
        and latest.get("allows") != allows
        and not app_ctx.dry_run
    ):
        pa.upsert_protected({**latest, "allows": allows})
    return {**record, "allows": allows}


def _controller_ss58(app_ctx: AppContext) -> str:
    controller = app_ctx.resolve_account("coldkey_ss58", None)
    if controller is None:
        raise ValueError("protected accounts require a controller address")
    return controller


@contextlib.contextmanager
def _preserve_controller_context(app_ctx: AppContext, record: dict[str, Any]):
    """Restore wallet routing after a multisig member submit mutates ``wallet_name``.

    ``wrap_intent_for_multisig_wallet`` may switch ``app_ctx.wallet_name`` to a
    local signatory for the duration of ``submit``. Announcement bookkeeping and
    follow-up protects must keep addressing the controller, not the member.
    """
    saved = (
        app_ctx.wallet_name,
        getattr(app_ctx, "multisig_wallet_name", None),
        getattr(app_ctx, "signatory_wallet", None),
        list(getattr(app_ctx, "signatory_wallets", [])),
    )
    controller_ss58 = _account_identity(record.get("controller_ss58") or _controller_ss58(app_ctx))
    try:
        yield controller_ss58
    finally:
        app_ctx.wallet_name = record.get("controller_wallet") or saved[0]
        app_ctx.multisig_wallet_name = saved[1]
        if hasattr(app_ctx, "signatory_wallet"):
            app_ctx.signatory_wallet = saved[2]
        if hasattr(app_ctx, "signatory_wallets"):
            app_ctx.signatory_wallets = saved[3]


def _call_data_hex(call: Any) -> Optional[str]:
    """0x-hex of composed call SCALE bytes, if available."""
    raw = getattr(call, "data", None)
    if raw is None:
        return None
    return "0x" + bytes(raw).hex()


def _review_protected_announcement(
    app_ctx: AppContext, intent, record: dict[str, Any], wallet: Any
) -> None:
    """Run the normal inner-call safety plan before committing only its hash.

    A delayed announcement is not permission to skip recipient, spend, balance,
    or policy checks: discovering an impossible or unsafe call after the delay
    would waste both the waiting period and the announcement deposit.
    """
    from .. import config as cfg

    if not hasattr(app_ctx.output, "plan"):
        # Minimal unit fixtures replace the output renderer. Production btcli
        # always has it; keeping the guard avoids making storage tests emulate
        # the complete transaction-review UI.
        return

    from ..executor import _OriginView
    from .context import _ProtectedPreviewIntent
    from .prompt import replay_command

    plan_wallet = wallet
    if cfg.get_multisig(record["controller_wallet"]) is not None:
        # The saved multisig has no keyfile-shaped wallet of its own. Its
        # public account is sufficient for inner-call policy and balance
        # checks; the outer announcement flow selects the actual member payer.
        plan_wallet = _OriginView(record["controller_ss58"])

    async def _plan(client):
        return await client.plan(
            _ProtectedPreviewIntent(intent, record["protected_ss58"]),
            plan_wallet,
            shielded=False,
        )

    plan = app_ctx.run(_plan)
    plan.effects.append(
        f"commit this call for protected account {record['protected_ss58']}; "
        f"execution remains delayed by {record.get('delay_blocks')} blocks"
    )
    app_ctx.output.plan(plan, command=replay_command())
    if not plan.ok:
        raise typer.Exit(1)


def _hash_call_data(call_data: str) -> str:
    """Blake2-256 of saved SCALE call bytes (same digest Proxy announces)."""
    from hashlib import blake2b

    raw = bytes.fromhex(str(call_data).removeprefix("0x"))
    return "0x" + blake2b(raw, digest_size=32).hexdigest()


def _coldkey_swap_matches_destination(
    announcement: dict[str, Any], destination: Optional[str]
) -> bool:
    """Match the runtime's legacy hash or authenticated V2 policy commitment."""
    from hashlib import blake2b

    from ..sp_core import ss58_decode

    if destination is None:
        return False
    account = bytes(ss58_decode(_account_identity(destination)))
    announced_hash = str(announcement.get("new_coldkey_hash") or "").lower()
    if announced_hash == "0x" + blake2b(account, digest_size=32).hexdigest():
        return True
    # A legacy caller can announce an arbitrary hash. V2 semantics require its
    # dedicated on-chain marker, including a stored False policy snapshot.
    policy = announcement.get("import_source_policies")
    if not isinstance(policy, bool):
        return False
    # SCALE encodes the runtime's fixed byte-array domain, AccountId32 and bool
    # directly: no length prefix or Option tag belongs in this preimage.
    preimage = b"coldkey-swap-v2-policy" + account + bytes([policy])
    return announced_hash == "0x" + blake2b(preimage, digest_size=32).hexdigest()


def _mark_announcement_from_submit(
    app_ctx: AppContext, row: dict[str, Any], result: Any
) -> Optional[dict[str, Any]]:
    """Tag a saved row as awaiting multisig approval when announce has not landed yet."""
    from . import multisig_helpers as ms_helpers

    events = getattr(result, "events", None) or []
    if not ms_helpers.multisig_approval_only(events):
        return row
    updates: dict[str, Any] = {"status": "awaiting_approval"}
    for event in events:
        identity = _multisig_announcement_event(event, row["delegate_ss58"])
        if identity:
            updates["multisig_announcement"] = identity
            break
    if "multisig_announcement" not in updates:
        call_hash = (getattr(result, "data", None) or {}).get("multisig_call_hash")
        if call_hash:
            updates["multisig_announcement"] = {
                "account_ss58": _account_identity(row["delegate_ss58"]),
                "call_hash": ms_helpers.hex_bytes(call_hash),
            }
    updated = ann.update_announcement(
        row["controller_wallet"],
        int(row["index"]),
        updates,
        genesis_hash=row.get("genesis_hash"),
    )
    app_ctx.output.message(
        f"#{row['index']}: announcement approved but awaiting further multisig "
        "signatures before it lands on chain — plaintext kept locally"
    )
    return updated or {**row, **updates}


def _multisig_announcement_event(entry: Any, delegate: str) -> Optional[dict[str, Any]]:
    """Read the underlying operation's identity, not the protected call's hash."""
    from . import multisig_helpers as ms_helpers

    module, event, attributes, _index = _event_parts(entry)
    if module != "Multisig" or event not in ("NewMultisig", "MultisigApproval"):
        return None
    if isinstance(attributes, (list, tuple)):
        names = (
            ("approving", "multisig", "call_hash")
            if event == "NewMultisig"
            else ("approving", "timepoint", "multisig", "call_hash")
        )
        attributes = dict(zip(names, attributes))
    if not isinstance(attributes, dict):
        return None
    account = attributes.get("multisig")
    call_hash = attributes.get("call_hash")
    if (
        not account
        or not call_hash
        or _account_identity(str(account)) != _account_identity(delegate)
    ):
        return None
    identity = {
        "account_ss58": _account_identity(delegate),
        "call_hash": ms_helpers.hex_bytes(call_hash),
    }
    if attributes.get("timepoint"):
        identity["timepoint"] = attributes["timepoint"]
    return identity


async def _recover_multisig_announcement(client, row: dict[str, Any]) -> Optional[dict[str, Any]]:
    """Recover older rows from their inclusion events and original call details.

    Rebuilding with current metadata cannot prove which call hash was approved
    before a runtime upgrade. Historical events plus the original cached or
    embedded call can, including when the operation has since been cancelled.
    """
    from .. import config as cfg
    from . import multisig_helpers as ms_helpers

    height = int(row.get("announced_at_block") or 0)
    if not height:
        return None
    block_hash = await client._substrate.block_hash(height)
    if not block_hash:
        return None
    identities = {}
    for entry in await client._substrate.events(block_hash):
        identity = _multisig_announcement_event(entry, row["delegate_ss58"])
        if identity is None:
            continue
        _, _, _, index = _event_parts(entry)
        timepoint = identity.get("timepoint") or {"height": height, "index": index}
        spec = cfg.get_multisig_cache(identity["call_hash"])
        opening_height = timepoint.get("height")
        opening_index = timepoint.get("index")
        if spec is None and opening_height is not None and opening_index is not None:
            spec = await ms_helpers.fetch_call_from_timepoint(
                client, height=int(opening_height), index=int(opening_index)
            )
        if (
            not spec
            or spec.get("target") != "Proxy.announce"
            or spec.get("proxy_for")
            or spec.get("sudo")
        ):
            continue
        params = spec.get("params") or {}
        real = ms_helpers._multiaddress_ss58(params.get("real"))
        if (
            real
            and _account_identity(real) == _account_identity(row["real_ss58"])
            and ms_helpers.hex_bytes(params.get("call_hash", "")).lower()
            == str(row["call_hash"]).lower()
        ):
            identities[identity["call_hash"]] = {**identity, "timepoint": timepoint}
    return next(iter(identities.values())) if len(identities) == 1 else None


def _pending_multisig_announcement(app_ctx: AppContext, row: dict[str, Any]) -> Optional[bool]:
    """True for a live commitment, False for confirmed absence, None if unknown."""

    async def _check(client):
        try:
            identity = row.get("multisig_announcement") or await _recover_multisig_announcement(
                client, row
            )
            if not identity or _account_identity(identity["account_ss58"]) != _account_identity(
                row["delegate_ss58"]
            ):
                return None, None
            pending = await client.read(
                "multisig", account_ss58=identity["account_ss58"], call_hash=identity["call_hash"]
            )
            if pending is not None:
                return True, identity
            # The threshold may have been met after the caller's initial Proxy
            # read. Recheck after the multisig read before retiring its plaintext.
            data = await client.read("proxy_announcements", delegate_ss58=row["delegate_ss58"])
            announced = any(
                str(item.get("call_hash") or "").lower() == str(row["call_hash"]).lower()
                and _account_identity(str(item.get("real") or ""))
                == _account_identity(row["real_ss58"])
                for item in data.get("announcements") or []
            )
            return announced, identity
        except Exception:
            # Archive pruning, missing identifiers and RPC failures are not
            # evidence that the operation was cancelled.
            return None, None

    pending, identity = app_ctx.run(_check)
    if identity and identity != row.get("multisig_announcement"):
        row["multisig_announcement"] = identity
        if not getattr(app_ctx, "dry_run", False):
            ann.update_announcement(
                row["controller_wallet"],
                int(row["index"]),
                {"multisig_announcement": identity},
                genesis_hash=row.get("genesis_hash"),
            )
    return pending


def _submit_protected_announcement(app_ctx: AppContext, intent, record: dict[str, Any]) -> None:
    from .prompt import confirm_wallet

    confirm_wallet(
        app_ctx,
        help_text="Wallet whose coldkey announces this protected-account call.",
        require_coldkey=True,
    )
    wallet = app_ctx.wallet()
    real = record["protected_ss58"]
    delay = _live_controller_delay(app_ctx, record, require_grant=True)
    if delay <= 0:
        app_ctx.output.error(
            "controller has a zero-delay Any grant; protection is not active",
            help="remove the zero-delay grant before relying on protected announcements",
        )
        raise typer.Exit(2)

    async def _prepare(client):
        composed, call_hash, _extras = await compose_announceable_call(
            client._substrate, intent, real, wallet
        )
        block = await client.block()
        spec_version = None
        with contextlib.suppress(Exception):
            spec_version = await client._substrate.spec_version()
        semantics = None
        with contextlib.suppress(Exception):
            semantics = await client.decode_scale("Call", bytes(composed.data))
        return composed, call_hash, block, intent.to_dict(), spec_version, semantics

    composed, call_hash, block, encoded, spec_version, semantics = app_ctx.run(_prepare)
    _review_protected_announcement(app_ctx, intent, record, wallet)
    inner_args = {k: v for k, v in encoded.items() if k != "op"}
    call_data = _call_data_hex(composed)
    recovered = _recover_existing_commitment(
        app_ctx,
        record,
        call_hash=call_hash,
        inner_op=intent.op,
        inner_args=inner_args,
        category=ann.category_for_op(intent.op),
        summary=intent.summary(),
        call_data=call_data,
        runtime_spec_version=spec_version,
        call_semantics=semantics,
        delay_blocks=delay,
    )
    if recovered is not None:
        app_ctx.output.message(
            f"identical announcement already exists on chain as #{recovered['index']}; "
            "recovered its receipt instead of reserving another deposit"
        )
        print_queued(app_ctx, record=record, row=recovered, current_block=block)
        return
    with _preserve_controller_context(app_ctx, record) as controller_ss58:
        result = app_ctx.submit(
            AnnounceProxyCall(real_ss58=real, call_hash=call_hash),
            proxy_for="self",
            summary=f"announce {intent.op} for protected account",
        )
    if result is None or not result.success:
        raise typer.Exit(1)
    # Prefer the inclusion block when available.
    announced_at = block
    if result.extrinsic_id:
        with contextlib.suppress(ValueError):
            announced_at = int(str(result.extrinsic_id).split("-")[0])
    row = ann.add_announcement(
        genesis_hash=record["genesis_hash"],
        controller_wallet=record["controller_wallet"],
        delegate_ss58=controller_ss58,
        real_ss58=real,
        call_hash=call_hash,
        inner_op=intent.op,
        inner_args=inner_args,
        category=ann.category_for_op(intent.op),
        delay_blocks=delay,
        announced_at_block=announced_at,
        force_proxy_type="Any",
        summary=intent.summary(),
        call_data=call_data,
        runtime_spec_version=spec_version,
        call_semantics=semantics,
    )
    row = _mark_announcement_from_submit(app_ctx, row, result) or row
    row = next(
        (live for live in _reconcile_pending(app_ctx, record) if live["index"] == row["index"]),
        row,
    )
    print_queued(app_ctx, record=record, row=row, current_block=announced_at)


def _recover_existing_commitment(
    app_ctx: AppContext,
    record: dict[str, Any],
    *,
    call_hash: str,
    inner_op: str,
    inner_args: dict[str, Any],
    category: str,
    summary: str,
    call_data: Optional[str],
    runtime_spec_version: Optional[int],
    call_semantics: Optional[dict[str, Any]],
    delay_blocks: int,
) -> Optional[dict[str, Any]]:
    """Recover a lost receipt without creating a duplicate on-chain commitment."""
    controller = _account_identity(record.get("controller_ss58") or _controller_ss58(app_ctx))
    matches = [
        item
        for item in _on_chain_announcements(
            app_ctx,
            controller_ss58=controller,
            protected_ss58=record["protected_ss58"],
            delay_blocks=delay_blocks,
        )
        if str(item["call_hash"]).lower() == str(call_hash).lower()
    ]
    recovered = []
    for item in matches:
        recovered.append(
            ann.add_announcement(
                controller_wallet=record["controller_wallet"],
                genesis_hash=record["genesis_hash"],
                delegate_ss58=controller,
                real_ss58=record["protected_ss58"],
                call_hash=call_hash,
                inner_op=inner_op,
                inner_args=inner_args,
                category=category,
                delay_blocks=delay_blocks,
                announced_at_block=int(item["announced_at_block"]),
                announcement_ordinal=int(item.get("announcement_ordinal") or 0),
                force_proxy_type="Any",
                summary=summary,
                call_data=call_data,
                runtime_spec_version=runtime_spec_version,
                call_semantics=call_semantics,
            )
        )
    return min(recovered, key=lambda row: int(row["announced_at_block"])) if recovered else None


def warn_defeating_proxies(app_ctx: AppContext, record: dict[str, Any]) -> None:
    controller = _account_identity(record.get("controller_ss58") or _controller_ss58(app_ctx))
    data = app_ctx.run(lambda c: c.read("proxies", coldkey_ss58=record["protected_ss58"]))
    hits = pa.defeating_zero_delay_proxies(data["proxies"], controller)
    if hits:
        types = ", ".join(hits)
        app_ctx.output.message(
            f"[bold red]WARNING[/bold red]: controller also has zero-delay "
            f"{types} proxy on the protected account — this defeats the "
            f"{record.get('delay_blocks')} block announcement delay. Remove it "
            "or the delay is meaningless."
        )


def _verify_pure_created_provenance(app_ctx: AppContext, book: dict[str, Any]) -> None:
    """Prove ``book['address']`` was spawned by the claimed create_pure coordinates.

    Field presence alone is not enough: a keyed account could hold a delayed Any
    grant with arbitrary book metadata. Recompute the legacy pure address when
    applicable, and always require a matching ``Proxy.PureCreated`` event at the
    claimed height / extrinsic index.
    """
    pure = str(book["address"])
    spawner = _account_identity(str(book["spawner"]))
    height = int(book["height"])
    ext_index = int(book["ext_index"])
    index = int(book.get("index") or 0)
    account_mode = str(book.get("account_mode") or "legacy")

    if account_mode == "legacy":
        try:
            expected = pa.derive_legacy_pure_ss58(
                spawner,
                height=height,
                ext_index=ext_index,
                proxy_type=str(book.get("proxy_type") or "Any"),
                index=index,
            )
        except ValueError as error:
            app_ctx.output.error(f"cannot recompute pure address: {error}")
            raise typer.Exit(2) from error
        if expected != pure:
            app_ctx.output.error(
                f"proxy-book address {pure} does not match legacy pure derivation "
                f"from spawner/height/ext-index ({expected}); refusing reuse — "
                "the entry is not a keyless pure with those coordinates"
            )
            raise typer.Exit(2)

    async def _check_event(client):
        block_hash = await client._substrate.block_hash(height)
        if not block_hash:
            return "missing-block"
        events = await client._substrate.events(block_hash)
        for entry in events:
            module_id, event_id, attributes, extrinsic_idx = _event_parts(entry)
            if module_id != "Proxy" or event_id != "PureCreated":
                continue
            if extrinsic_idx is not None and int(extrinsic_idx) != ext_index:
                continue
            if not isinstance(attributes, dict):
                continue
            event_pure = str(attributes.get("pure") or "")
            event_who = str(attributes.get("who") or "")
            if event_pure == pure and event_who == spawner:
                return "ok"
        return "missing-event"

    outcome = app_ctx.run(_check_event)
    if outcome == "missing-block":
        app_ctx.output.error(
            f"cannot read block {height} to verify PureCreated — need an archive "
            "endpoint (or create a fresh pure with `btcli proxy protect`)"
        )
        raise typer.Exit(2)
    if outcome != "ok":
        app_ctx.output.error(
            f"no Proxy.PureCreated for {pure} by {spawner} at block {height} "
            f"extrinsic {ext_index}; refusing to reuse a non-pure (or mismatched) address"
        )
        raise typer.Exit(2)


def _validate_reusable_pure(
    app_ctx: AppContext,
    *,
    book: dict[str, Any],
    controller_ss58: str,
    delay_blocks: int,
) -> None:
    """Reject a proxy-book resume target that is not a delayed-controller pure.

    A book entry alone does not prove the address is keyless or that this
    wallet is its sole delayed Any controller. Validate provenance fields,
    recompute/verify pure creation, and live on-chain proxy grants before
    announcing a coldkey swap into it.
    """
    controller_ss58 = _account_identity(controller_ss58)
    missing = [
        key
        for key in ("address", "spawner", "height", "ext_index", "proxy_type")
        if book.get(key) in (None, "")
    ]
    if missing:
        app_ctx.output.error(
            "proxy-book entry is missing pure-creation provenance "
            f"({', '.join(missing)}); refusing to reuse it as a protected account",
            help="create a fresh pure with `btcli proxy protect`, or `btcli proxy book "
            "add` with spawner/height/ext-index from PureCreated",
        )
        raise typer.Exit(2)
    if str(book.get("proxy_type")) != "Any":
        app_ctx.output.error(
            f"proxy-book entry has proxy_type {book.get('proxy_type')!r}; "
            "protected accounts require an Any controller relationship",
        )
        raise typer.Exit(2)

    _verify_pure_created_provenance(app_ctx, book)

    pure = str(book["address"])
    data = app_ctx.run(lambda c: c.read("proxies", coldkey_ss58=pure))
    delegations = data["proxies"]
    if not delegations:
        app_ctx.output.error(
            f"{pure} has no on-chain proxy delegations — not a usable protected account"
        )
        raise typer.Exit(2)

    any_delays = pa.controller_any_delays(delegations, controller_ss58)
    if not any_delays:
        app_ctx.output.error(
            f"{controller_ss58} is not an Any proxy of {pure}; refusing to reuse it"
        )
        raise typer.Exit(2)
    # find_proxy picks the first matching grant; a shorter sibling Any wins even
    # when a longer delay also exists. Every controller Any must be exactly the
    # requested delay, and no controller grant may execute earlier.
    if any(delay != delay_blocks for delay in any_delays):
        have = ", ".join(str(delay) for delay in any_delays)
        app_ctx.output.error(
            f"controller Any delays on {pure} are [{have}]; every Any grant must "
            f"be exactly --delay {delay_blocks} (shorter siblings defeat the window)"
        )
        raise typer.Exit(2)

    earlier = pa.controller_grants_before_delay(
        delegations, controller_ss58, delay_blocks=delay_blocks
    )
    if earlier:
        detail = ", ".join(f"{d.get('proxy_type')}@{d.get('delay')}" for d in earlier)
        app_ctx.output.error(
            f"controller holds grants that execute earlier than --delay {delay_blocks} "
            f"on {pure}: {detail}. Remove them before protect can resume."
        )
        raise typer.Exit(2)

    hits = pa.defeating_zero_delay_proxies(delegations, controller_ss58)
    if hits:
        types = ", ".join(hits)
        app_ctx.output.error(
            f"controller also holds zero-delay {types} on {pure}, which defeats the "
            "announcement delay; remove those grants before protect can resume"
        )
        raise typer.Exit(2)

    # Any foreign proxy grant is incompatible with a sole-controller protected
    # account. Spending types can drain after their delay; types like
    # NonFungible can immediately ``reject_announcement`` and veto the owner's
    # delayed withdrawals / grant removals. Refuse all of them.
    others = [d for d in delegations if d["delegate"] != controller_ss58]
    if others:
        detail = ", ".join(
            f"{d['delegate']} ({d['proxy_type']}, delay {d['delay']})" for d in others
        )
        app_ctx.output.error(
            f"{pure} has other proxy grants besides the controller: {detail}. "
            "Refuse to move funds onto an account with a competing delegate "
            "(spending path or announcement-veto capability)."
        )
        raise typer.Exit(2)


def _on_chain_announcements(
    app_ctx: AppContext, *, controller_ss58: str, protected_ss58: str, delay_blocks: int
) -> list[dict[str, Any]]:
    """Pending Proxy.Announcements by the controller targeting the protected pure."""

    async def _load(client):
        data = await client.read("proxy_announcements", delegate_ss58=controller_ss58)
        return data["announcements"]

    rows: list[dict[str, Any]] = []
    occurrences: dict[tuple[str, int], int] = {}
    for item in app_ctx.run(_load):
        if item["real"] != protected_ss58:
            continue
        height = int(item["height"])
        identity = (str(item["call_hash"]).lower(), height)
        ordinal = occurrences.get(identity, 0)
        occurrences[identity] = ordinal + 1
        rows.append(
            {
                "real_ss58": item["real"],
                "call_hash": item["call_hash"],
                "announced_at_block": height,
                "announcement_ordinal": ordinal,
                "delay_blocks": delay_blocks,
                "executable_at_block": height + delay_blocks,
                "delegate_ss58": controller_ss58,
            }
        )
    return sorted(
        rows,
        key=lambda row: (
            row["announced_at_block"],
            row["call_hash"],
            row["announcement_ordinal"],
        ),
    )


def _live_controller_delay(
    app_ctx: AppContext, record: dict[str, Any], *, require_grant: bool = False
) -> int:
    """Active on-chain Any delay for the controller, falling back to the local record.

    Pending delay-change announcements must not advertise a longer window than
    the live grant still permits.
    """
    controller = _account_identity(record.get("controller_ss58") or _controller_ss58(app_ctx))
    pure = record["protected_ss58"]
    data = app_ctx.run(lambda c: c.read("proxies", coldkey_ss58=pure))
    delays = [
        int(d["delay"])
        for d in data["proxies"]
        if d["delegate"] == controller and d["proxy_type"] == "Any"
    ]
    if delays:
        # Prefer the shortest live delay — that is the soonest execution window.
        return min(delays)
    if require_grant:
        app_ctx.output.error("controller has no live Any grant on the protected account")
        raise typer.Exit(2)
    return int(record.get("delay_blocks") or 0)


def _reconcile_pending(app_ctx: AppContext, record: dict[str, Any]) -> list[dict[str, Any]]:
    """Merge local plaintext rows with live on-chain announcements.

    On-chain entries without local plaintext stay visible and cancellable.
    Approval-only rows retire only after their actual multisig commitment and
    proxy announcement are both confirmed absent. Dry runs project these updates
    without changing the saved queue or protected-account settings.
    """
    wallet_name = record["controller_wallet"]
    genesis_hash = _chain_genesis_hash(app_ctx)
    if record.get("genesis_hash") != genesis_hash:
        raise ValueError("protected-account binding does not belong to the selected chain")
    controller = _account_identity(record.get("controller_ss58") or _controller_ss58(app_ctx))
    dry_run = getattr(app_ctx, "dry_run", False)
    delay = _live_controller_delay(app_ctx, record)
    if record.get("delay_blocks") != delay:
        record["delay_blocks"] = delay
        # The caller may retain a pre-signing snapshot. Refresh only the active
        # delay, preserving newer settings/announcements already saved locally.
        latest = pa.get_by_controller(wallet_name, genesis_hash=genesis_hash)
        if (
            not dry_run
            and latest is not None
            and latest["protected_ss58"] == record["protected_ss58"]
        ):
            pa.upsert_protected({**latest, "delay_blocks": delay})
    on_chain = _on_chain_announcements(
        app_ctx,
        controller_ss58=controller,
        protected_ss58=record["protected_ss58"],
        delay_blocks=delay,
    )
    # Persist discovered identities before showing an index. Reuse historical
    # plaintext when a previously disappeared commitment is seen again.
    by_identity = {}
    history = ann.list_for_wallet(wallet_name, include_done=True, genesis_hash=genesis_hash)
    for item in on_chain:
        fields: dict[str, Any] = dict(
            controller_wallet=wallet_name,
            genesis_hash=genesis_hash,
            delegate_ss58=controller,
            real_ss58=record["protected_ss58"],
            call_hash=item["call_hash"],
            delay_blocks=delay,
            announced_at_block=item["announced_at_block"],
            announcement_ordinal=item.get("announcement_ordinal", 0),
        )
        if dry_run:
            matches = [
                saved
                for saved in history
                if str(saved.get("call_hash") or "").lower() == str(item["call_hash"]).lower()
                and _account_identity(saved["delegate_ss58"]) == controller
                and _account_identity(saved["real_ss58"]) == record["protected_ss58"]
            ]
            exact = next(
                (
                    saved
                    for saved in matches
                    if int(saved.get("announced_at_block") or 0) == int(item["announced_at_block"])
                    and int(saved.get("announcement_ordinal") or 0)
                    == int(item.get("announcement_ordinal") or 0)
                ),
                None,
            )
            if exact is not None:
                saved = exact
            elif matches:
                saved = matches[0]
            else:
                saved = {
                    "index": None,
                    "category": "unknown",
                    "provisional": True,
                }
            plaintext = next((candidate for candidate in matches if candidate.get("inner_op")), {})
            row = {
                **saved,
                **plaintext,
                **fields,
                "index": saved["index"],
                "status": "pending",
                "on_chain": True,
                "executable_at_block": item["announced_at_block"] + delay,
                "plaintext_missing": not bool(plaintext.get("inner_op")),
            }
        else:
            row = ann.upsert_on_chain_announcement(**fields)
        identity = (
            str(item["call_hash"]).lower(),
            int(item["announced_at_block"]),
            int(item.get("announcement_ordinal") or 0),
        )
        by_identity[identity] = row
    local = ann.list_for_wallet(wallet_name, genesis_hash=genesis_hash)
    merged: list[dict[str, Any]] = list(by_identity.values())

    for row in local:
        identity = (
            str(row.get("call_hash") or "").lower(),
            int(row.get("announced_at_block") or 0),
            int(row.get("announcement_ordinal") or 0),
        )
        if (
            identity in by_identity
            and _account_identity(row["delegate_ss58"]) == controller
            and row["real_ss58"] == record["protected_ss58"]
        ):
            continue
        elif row.get("status") == "awaiting_approval":
            approval_pending = _pending_multisig_announcement(app_ctx, row)
            if approval_pending is not False:
                merged.append(
                    {
                        **row,
                        "on_chain": False,
                        "approval_state": "pending" if approval_pending else "unknown",
                        "approval_state_reason": (
                            None
                            if approval_pending
                            else "Could not verify the underlying multisig operation. "
                            "Retry with an archive RPC that can read the original approval "
                            "block; saved plaintext is retained."
                        ),
                    }
                )
                continue
            if not dry_run:
                ann.mark_status(wallet_name, int(row["index"]), "gone", genesis_hash=genesis_hash)
        elif row.get("status") == "pending" and not dry_run:
            # Gone from chain (executed, cancelled elsewhere, or never landed).
            ann.mark_status(wallet_name, int(row["index"]), "gone", genesis_hash=genesis_hash)

    record.update(_reconcile_pending_allow_flags(record, merged, persist=not dry_run))
    return sorted(merged, key=lambda r: int(r.get("index") or 0))


def _reconcile_pending_allow_flags(
    record: dict[str, Any], rows: list[dict[str, Any]], *, persist: bool = True
) -> dict[str, Any]:
    """Derive pending grants from surviving commitments, including nested batches."""
    allow_key = record.get("allow_key_ss58") or record.get("controller_ss58")
    pending = []
    for row in rows:
        for grant in pa.announcement_proxy_grants(row):
            target = grant["delegate_ss58"]
            proxy_type = grant["proxy_type"]
            if grant["delay"] != 0 or (
                allow_key and target and _account_identity(target) != _account_identity(allow_key)
            ):
                continue
            if proxy_type not in pending:
                pending.append(proxy_type)
    # An undisclosed on-chain call may still be one of the cached pending grants.
    if any(row.get("plaintext_missing") or row.get("category") == "unknown" for row in rows):
        pending = list(dict.fromkeys([*(record.get("pending_allows") or []), *pending]))
    updated = {**record, "pending_allows": pending}
    latest = pa.get_by_controller(
        record["controller_wallet"], genesis_hash=record.get("genesis_hash")
    )
    if (
        persist
        and latest is not None
        and latest["protected_ss58"] == record["protected_ss58"]
        and list(latest.get("pending_allows") or []) != pending
    ):
        pa.upsert_protected({**latest, "pending_allows": pending})
    return updated


def run_protect(
    app_ctx: AppContext,
    *,
    delay_raw: Optional[str],
    allow: list[str],
    disallow: list[str],
    allow_key: Optional[str],
    bind_network: bool = False,
) -> None:
    """Create or update a protected account for ``-w``."""
    wallet_name = require_wallet_name(app_ctx)
    if bind_network:
        _bind_legacy_network(app_ctx, wallet_name)
        return
    try:
        delay_blocks = pa.parse_delay_to_blocks(delay_raw) if delay_raw is not None else None
    except ValueError as error:
        app_ctx.output.error(str(error))
        raise typer.Exit(2) from error

    try:
        allow_types = pa.resolve_allow_items(allow) if allow else []
        disallow_types = pa.resolve_allow_items(disallow) if disallow else []
    except ValueError as error:
        app_ctx.output.error(str(error))
        raise typer.Exit(2) from error

    if allow_types:
        _warn_allow_authority(app_ctx, allow_types, separate_key=bool(allow_key))

    allow_key_ss58 = None
    if allow_key:
        allow_key_ss58 = app_ctx.resolve_address("coldkey_ss58", allow_key)

    existing = _get_protected_record(app_ctx, wallet_name)
    if existing is None:
        _protect_create(
            app_ctx,
            wallet_name=wallet_name,
            delay_blocks=delay_blocks,
            allow_types=allow_types,
            allow_key_ss58=allow_key_ss58,
        )
        return

    if delay_blocks is None:
        delay_blocks = _live_controller_delay(app_ctx, existing, require_grant=True)
    if pa.protected_setup_pending(existing):
        if disallow_types:
            app_ctx.output.error("finish the interrupted setup before changing allow grants")
            raise typer.Exit(2)
        _protect_create(
            app_ctx,
            wallet_name=wallet_name,
            delay_blocks=delay_blocks,
            allow_types=allow_types,
            allow_key_ss58=allow_key_ss58,
            existing=existing,
        )
        return

    _protect_update(
        app_ctx,
        existing,
        delay_blocks=delay_blocks,
        allow_types=allow_types,
        disallow_types=disallow_types,
        allow_key_ss58=allow_key_ss58,
    )


def _warn_allow_authority(
    app_ctx: AppContext, allow_types: list[str], *, separate_key: bool
) -> None:
    """Disclose the complete live ProxyType filters before granting delay-0 authority."""
    from .._generated.runtime_apis import ProxyFilterRuntimeApi

    def text_value(value: Any) -> str:
        if isinstance(value, bytes):
            return value.decode(errors="replace")
        if isinstance(value, list) and all(isinstance(part, int) for part in value):
            return bytes(value).decode(errors="replace")
        return str(value)

    filters: dict[str, list[str]] = {}
    try:
        raw = app_ctx.run(lambda c: c.runtime(ProxyFilterRuntimeApi.get_proxy_filters, [None]))
        for item in raw or []:
            name = text_value(item.get("name") or "")
            mode = item.get("filter_mode") or {}
            calls = mode.get("Allow", []) if isinstance(mode, dict) else []
            rendered = []
            for call in calls:
                if isinstance(call, dict):
                    pallet = text_value(
                        call.get("pallet_name") or call.get("pallet") or call.get("module") or ""
                    )
                    function = text_value(
                        call.get("call_name") or call.get("call") or call.get("function") or ""
                    )
                    rendered.append(f"{pallet}::{function}" if pallet and function else str(call))
                else:
                    rendered.append(str(call))
            if name:
                filters[name] = rendered
    except Exception:
        # Disclosure must not make setup depend on a new runtime API. Older
        # chains still get the SDK's complete known operation list.
        filters = {}

    app_ctx.output.message(
        "[bold red]WARNING: every --allow grant is zero-delay authority. A stolen "
        "allow key can repeatedly run every listed operation and destroy economic "
        "value even when it cannot transfer free TAO.[/bold red]"
    )
    for proxy_type in allow_types:
        calls = filters.get(proxy_type) or sorted(pa.PROXY_TYPE_INTENT_OPS.get(proxy_type, ()))
        rendered = ", ".join(calls) if calls else "all live filter calls"
        app_ctx.output.message(f"  {proxy_type}: {rendered}")
    if not separate_key:
        app_ctx.output.message(
            "[bold red]The controller would hold these instant grants too. For a "
            "high-value account, use a separate --allow-key so compromising the "
            "controller does not also compromise every allow.[/bold red]"
        )


def _bind_legacy_network(app_ctx: AppContext, wallet_name: str) -> None:
    """Explicitly attach a pre-scoping binding and its queue to a verified chain."""
    genesis_hash = _chain_genesis_hash(app_ctx)
    record = pa.get_by_controller(wallet_name)
    if record is None:
        if pa.get_by_controller(wallet_name, genesis_hash=genesis_hash) is not None:
            app_ctx.output.message("protected account is already bound to this network")
            return
        app_ctx.output.error("no legacy protected-account binding exists for this wallet")
        raise typer.Exit(2)
    if pa.get_by_controller(wallet_name, genesis_hash=genesis_hash) is not None:
        app_ctx.output.error("this network already has a binding; refusing to overwrite it")
        raise typer.Exit(2)
    controller = _account_identity(record.get("controller_ss58") or _controller_ss58(app_ctx))
    if controller != _controller_ss58(app_ctx):
        app_ctx.output.error("the wallet no longer matches the saved controller")
        raise typer.Exit(2)
    if any(record.get(key) is None for key in ("spawner", "height", "ext_index")):
        app_ctx.output.error("legacy binding lacks pure-creation provenance; cannot bind safely")
        raise typer.Exit(2)
    _verify_pure_created_provenance(
        app_ctx, {**record, "address": record["protected_ss58"], "proxy_type": "Any"}
    )
    data = app_ctx.run(
        lambda c, pure=record["protected_ss58"]: c.read("proxies", coldkey_ss58=pure)
    )
    if not any(d > 0 for d in pa.controller_any_delays(data["proxies"], controller)):
        app_ctx.output.error("the selected chain has no delayed Any grant for this controller")
        raise typer.Exit(2)
    summary = (
        f"Bind {wallet_name} → {record['protected_ss58']} and its saved announcements "
        f"to {app_ctx.network} (genesis {genesis_hash})"
    )
    if app_ctx.dry_run:
        app_ctx.output.message(summary)
        return
    app_ctx.confirm(summary + "?")
    # Move payloads first: a collision must leave the old binding available.
    ann.bind_legacy_announcements(wallet_name, genesis_hash, real_ss58=record["protected_ss58"])
    bound = pa.bind_legacy_controller(wallet_name, genesis_hash)
    if bound is not None:
        _normalize_record(bound)
    app_ctx.output.message("protected account and saved announcements bound to this network")


def _protect_create(
    app_ctx: AppContext,
    *,
    wallet_name: str,
    delay_blocks: Optional[int],
    allow_types: list[str],
    allow_key_ss58: Optional[str],
    existing: Optional[dict[str, Any]] = None,
) -> None:
    from .. import config as cfg
    from .multisig_helpers import multisig_approval_only

    name = existing["name"] if existing else pa.default_protected_name(wallet_name)
    genesis_hash = _chain_genesis_hash(app_ctx)
    controller_ss58 = _controller_ss58(app_ctx)
    controller_ctx = {
        "controller_wallet": wallet_name,
        "controller_ss58": controller_ss58,
    }
    book = cfg.get_proxy(name)
    if not existing and book and book.get("genesis_hash") not in (None, genesis_hash):
        name = f"{name}-{genesis_hash[2:10]}"
        book = cfg.get_proxy(name)
    pure_ss58 = existing["protected_ss58"] if existing else book.get("address") if book else None
    create_meta: dict[str, Any] = dict(book) if book else {}
    if existing:
        create_meta = {
            **existing,
            "address": pure_ss58,
            "proxy_type": "Any",
            "delay": existing["delay_blocks"],
        }
        if controller_ss58 != _account_identity(existing["controller_ss58"]):
            app_ctx.output.error("the wallet no longer matches the interrupted setup controller")
            raise typer.Exit(2)
        configured_key = existing.get("allow_key_ss58") or controller_ss58
        if allow_key_ss58 and _account_identity(allow_key_ss58) != configured_key:
            app_ctx.output.error("finish the interrupted setup before changing its allow key")
            raise typer.Exit(2)
        allow_key_ss58 = existing.get("allow_key_address") or existing.get("allow_key_ss58")
        prior = (existing.get("setup") or {}).get("allow_types")
        if prior is None:
            prior = [*(existing.get("allows") or []), *(existing.get("pending_allows") or [])]
        allow_types = list(dict.fromkeys([*prior, *allow_types]))
    if delay_blocks is None:
        delay_blocks = int(create_meta.get("delay") or pa.parse_delay_to_blocks("36h"))
    if existing and delay_blocks != _live_controller_delay(app_ctx, existing, require_grant=True):
        app_ctx.output.error("finish the interrupted setup before changing the controller delay")
        raise typer.Exit(2)

    if app_ctx.dry_run:
        action = (
            f"resume protected account {name} ({pure_ss58})"
            if pure_ss58
            else "create a protected pure account"
        )
        app_ctx.output.message(
            f"Dry run: {action} with controller delay {delay_blocks}; "
            f"queue missing allows {allow_types}, then announce the coldkey migration. "
            "No setup progress or account binding was saved."
        )
        return

    def _setup_record() -> dict[str, Any]:
        return {
            "genesis_hash": genesis_hash,
            "controller_wallet": wallet_name,
            "controller_ss58": controller_ss58,
            "protected_ss58": pure_ss58,
            "name": name,
            "delay_blocks": delay_blocks,
            "allows": [],
            "pending_allows": [],
            "allow_key_ss58": _account_identity(allow_key_ss58) if allow_key_ss58 else None,
            "allow_key_address": allow_key_ss58,
            "spawner": create_meta.get("spawner") or controller_ss58,
            "account_mode": create_meta.get("account_mode", "legacy"),
            "height": create_meta.get("height"),
            "ext_index": create_meta.get("ext_index"),
            "index": create_meta.get("index", 0),
            "coldkey_swap": {"status": "pending"},
            **(existing or {}),
            "setup": {"status": "pending", "allow_types": allow_types},
        }

    if not pure_ss58:

        def _save_creation_receipt(result):
            nonlocal pure_ss58, create_meta
            if not result.success or not result.data.get("pure_proxy"):
                return
            pure_ss58 = result.data["pure_proxy"]
            create_meta = {
                "genesis_hash": genesis_hash,
                "name": name,
                "address": pure_ss58,
                "spawner": result.data.get("spawner") or controller_ss58,
                "account_mode": result.data.get("pure_proxy_account_mode", "legacy"),
                "proxy_type": "Any",
                "delay": delay_blocks,
                "index": 0,
                "note": "protected account",
            }
            for key in ("height", "ext_index"):
                if result.data.get(key) is not None:
                    create_meta[key] = result.data[key]
            pa.upsert_protected(_setup_record())

        with _preserve_controller_context(app_ctx, controller_ctx):
            result = app_ctx.submit(
                CreatePureProxy(proxy_type="Any", delay=delay_blocks, index=0),
                proxy_for="self",
                summary=f"create protected pure proxy (Any, delay {delay_blocks})",
                on_result=_save_creation_receipt,
            )
        if result is None or not result.success:
            raise typer.Exit(1)
        _save_creation_receipt(result)
        if not pure_ss58:
            app_ctx.output.error(
                "could not read PureCreated address — recover the created account with "
                "`btcli proxy book add` and its PureCreated provenance before retrying protect"
            )
            raise typer.Exit(1)
        app_ctx.output.message(f"created protected account {name} ({pure_ss58})")
    elif existing is None:
        assert book is not None
        _validate_reusable_pure(
            app_ctx,
            book=book,
            controller_ss58=controller_ss58,
            delay_blocks=delay_blocks,
        )
        app_ctx.output.message(f"resuming protect: reusing proxy-book entry {name} ({pure_ss58})")
    else:
        _verify_pure_created_provenance(app_ctx, create_meta)

    record: dict[str, Any] = _setup_record()
    # Save provenance before the book or any later transaction: a partial run
    # must resume this pure account instead of creating another one.
    pa.upsert_protected(record)
    if not book or book.get("address") != pure_ss58:
        cfg.add_proxy(create_meta)

    delegate = allow_key_ss58 or controller_ss58
    delegate_identity = _account_identity(delegate)
    grants = app_ctx.run(lambda c: c.read("proxies", coldkey_ss58=pure_ss58))["proxies"]
    expected = {(controller_ss58, "Any", delay_blocks)} | {
        (delegate_identity, proxy_type, 0) for proxy_type in allow_types
    }
    actual = {
        (_account_identity(grant["delegate"]), str(grant["proxy_type"]), int(grant["delay"]))
        for grant in grants
    }
    if (controller_ss58, "Any", delay_blocks) not in actual or actual - expected:
        app_ctx.output.error(
            "interrupted setup has unexpected proxy authority; "
            "inspect its grants before migrating assets"
        )
        raise typer.Exit(2)
    warn_defeating_proxies(app_ctx, record)
    pending = _reconcile_pending(app_ctx, record)
    confirmed = []
    pending_allows = []
    awaiting_approval = False
    for proxy_type in allow_types:
        if (delegate_identity, proxy_type, 0) in actual:
            confirmed.append(proxy_type)
            continue
        intent = AddProxy(delegate_ss58=delegate, proxy_type=proxy_type, delay=0, from_hotkey=False)
        row = next(
            (
                item
                for item in pending
                if item.get("inner_op") == "add_proxy"
                and _account_identity((item.get("inner_args") or {}).get("delegate_ss58") or "")
                == delegate_identity
                and (item.get("inner_args") or {}).get("proxy_type") == proxy_type
                and int((item.get("inner_args") or {}).get("delay") or 0) == 0
            ),
            None,
        )
        if row is None and any(item.get("plaintext_missing") for item in pending):
            # Recover a successful announce whose receipt/local save was lost.
            # Matching the composed hash avoids a second commitment.
            async def _compose(client, intent=intent, pure=str(pure_ss58)):
                return await compose_announceable_call(
                    client._substrate, intent, pure, app_ctx.wallet()
                )

            composed, call_hash, _extras = app_ctx.run(_compose)
            live = next(
                (item for item in pending if str(item["call_hash"]).lower() == call_hash.lower()),
                None,
            )
            if live is not None:
                encoded = intent.to_dict()
                row = ann.add_announcement(
                    controller_wallet=wallet_name,
                    genesis_hash=genesis_hash,
                    delegate_ss58=controller_ss58,
                    real_ss58=pure_ss58,
                    call_hash=call_hash,
                    inner_op=intent.op,
                    inner_args={key: value for key, value in encoded.items() if key != "op"},
                    category=f"allow:{proxy_type}",
                    delay_blocks=delay_blocks,
                    announced_at_block=live["announced_at_block"],
                    call_data=_call_data_hex(composed),
                )
        if row is None:
            row = _announce_as_protected(app_ctx, record, intent, category=f"allow:{proxy_type}")
        if row is None:
            raise typer.Exit(1)
        awaiting_approval |= row.get("status") == "awaiting_approval"
        pending_allows.append(proxy_type)
        record = {**record, "allows": confirmed, "pending_allows": list(pending_allows)}
        pa.upsert_protected(record)
    record = {**record, "allows": confirmed, "pending_allows": pending_allows}
    pa.upsert_protected(record)
    if awaiting_approval:
        app_ctx.output.message(
            "Setup is waiting for multisig approval of its allow announcements. "
            f"Finish those approvals, then re-run `btcli proxy protect --wallet {wallet_name}`."
        )
        return

    async def _swap_status(client):
        announcement = await client.read("coldkey_swap_announcement", coldkey_ss58=controller_ss58)
        return announcement, await client.block()

    announcement, current = app_ctx.run(_swap_status)

    def _validate_swap(announcement):
        if announcement is None:
            return
        if not _coldkey_swap_matches_destination(announcement, pure_ss58):
            app_ctx.output.error(
                "the controller has a different or recovery-locked coldkey migration; "
                "resolve it before resuming setup"
            )
            raise typer.Exit(2)
        if announcement.get("disputed") or announcement.get("recovery_locked"):
            app_ctx.output.error(
                "the protected-account coldkey migration is disputed or recovery locked"
            )
            raise typer.Exit(2)

    _validate_swap(announcement)
    if announcement is None and record["coldkey_swap"].get("status") == "awaiting_approval":
        identity = record["coldkey_swap"].get("multisig_announcement")
        approval_pending = None
        if identity:
            try:
                approval_pending = app_ctx.run(lambda c: c.read("multisig", **identity)) is not None
                if not approval_pending:
                    # Approval may have executed after the first migration read.
                    announcement, current = app_ctx.run(_swap_status)
            except Exception:
                approval_pending = None
        _validate_swap(announcement)
        if announcement is None and approval_pending is not False:
            state = "waiting for approval" if approval_pending else "approval state is unknown"
            app_ctx.output.message(
                f"Coldkey migration {state}. Check its multisig operation, then re-run "
                f"`btcli proxy protect --wallet {wallet_name}`."
            )
            return
        if announcement is None:
            record = {**record, "coldkey_swap": {"status": "pending"}}
            pa.upsert_protected(record)

    if announcement is None and record["coldkey_swap"].get("status") != "announced":
        app_ctx.output.message(
            "[bold red]WARNING: the coldkey migration moves ALL available TAO to the pure "
            "account. Its controller will need a separate top-up AFTER migration to pay "
            "fees and refundable announcement deposits. Keep a funded second wallet ready."
            "[/bold red]"
        )

        def _save_swap_receipt(result):
            nonlocal record
            if not result.success:
                return
            events = getattr(result, "events", None) or []
            awaiting = multisig_approval_only(events)
            swap: dict[str, Any] = {
                "status": "awaiting_approval" if awaiting else "announced",
                "new_coldkey_ss58": pure_ss58,
            }
            if awaiting:
                for event in events:
                    identity = _multisig_announcement_event(event, controller_ss58)
                    if identity is not None:
                        swap["multisig_announcement"] = {
                            "account_ss58": identity["account_ss58"],
                            "call_hash": identity["call_hash"],
                        }
                        break
            record = {**record, "coldkey_swap": swap}
            pa.upsert_protected(record)

        with _preserve_controller_context(app_ctx, record):
            swap_result = app_ctx.submit(
                AnnounceColdkeySwap(new_coldkey_ss58=pure_ss58),
                proxy_for="self",
                summary=f"announce coldkey swap {wallet_name} → {name}",
                on_result=_save_swap_receipt,
            )
        if swap_result is None or not swap_result.success:
            app_ctx.output.message(
                "Protected setup is incomplete; re-run "
                f"`btcli proxy protect --wallet {wallet_name}` to resume the coldkey migration."
            )
            raise typer.Exit(1)
        # Persist success before querying timing so an RPC interruption cannot
        # charge another migration fee or reset the execution window on retry.
        _save_swap_receipt(swap_result)
        if record["coldkey_swap"]["status"] == "awaiting_approval":
            app_ctx.output.message(
                "Coldkey migration awaits multisig approval; finish approval and re-run "
                f"`btcli proxy protect --wallet {wallet_name}`."
            )
            return
        announcement, current = app_ctx.run(_swap_status)
        _validate_swap(announcement)

    execute_block = (
        announcement.get("execute_block")
        if announcement
        else record["coldkey_swap"].get("execute_block")
    )
    record = {
        **record,
        "coldkey_swap": {
            "status": "announced",
            "execute_block": execute_block,
            "new_coldkey_ss58": pure_ss58,
        },
        "setup": {"status": "ready", "allow_types": allow_types},
    }
    pa.upsert_protected(record)
    timing = (
        f"after block {execute_block} ({_eta_for_block(int(execute_block), current)})"
        if execute_block is not None
        else "when ready"
    )
    app_ctx.output.message(
        f"Coldkey swap announced → {name} ({pure_ss58}). Execute {timing} with:\n"
        f"  btcli wallet swap-coldkey --new-coldkey {pure_ss58} --wallet {wallet_name}\n"
        f"Check progress: btcli wallet swap-check --wallet {wallet_name}"
    )
    app_ctx.output.message(
        "After executing the migration, fund its controller immediately with:\n"
        f"  btcli proxy fund --wallet {wallet_name} --from-wallet FUNDING_WALLET\n"
        "`btcli proxy show` reports the recommended balance for 20 transfers or 10 staking "
        "operations, including refundable announcement deposits."
    )
    if pending_allows:
        app_ctx.output.message(
            "Zero-delay allow proxies were queued as delayed announcements — "
            f"run `btcli proxy execute --wallet {wallet_name}` after the delay to activate them."
        )


def _protect_update(
    app_ctx: AppContext,
    existing: dict[str, Any],
    *,
    delay_blocks: int,
    allow_types: list[str],
    disallow_types: list[str],
    allow_key_ss58: Optional[str],
) -> None:
    existing = _normalize_record(existing, persist=not app_ctx.dry_run)
    controller_ss58 = existing.get("controller_ss58") or _controller_ss58(app_ctx)
    genesis_hash = existing["genesis_hash"]
    current_delay = _live_controller_delay(app_ctx, existing, require_grant=True)
    existing = {**existing, "delay_blocks": current_delay}
    # Anyone may execute a matured announcement. Local pending flags cannot
    # establish whether authority was installed or removed by another client.
    data = app_ctx.run(lambda c: c.read("proxies", coldkey_ss58=existing["protected_ss58"]))
    old_allow_key = existing.get("allow_key_ss58") or controller_ss58
    current_allows = [
        str(grant["proxy_type"])
        for grant in pa.allow_key_grants(data["proxies"], old_allow_key)
        if grant["proxy_type"] in pa.PROXY_TYPE_INTENT_OPS
    ]
    pending_rows = _reconcile_pending(app_ctx, existing)
    pending_allows = list(
        dict.fromkeys(
            grant["proxy_type"]
            for row in pending_rows
            for grant in pa.announcement_proxy_grants(row)
            if grant["delay"] == 0
            and _account_identity(grant["delegate_ss58"]) == _account_identity(old_allow_key)
            and grant["proxy_type"] in pa.PROXY_TYPE_INTENT_OPS
        )
    )
    existing = {**existing, "allows": current_allows}
    if not app_ctx.dry_run:
        pa.upsert_protected(existing)
    new_allow_key = _account_identity(allow_key_ss58) if allow_key_ss58 else None
    # Keep the recorded allow-key unless the user explicitly retargets it.
    # Never flip the pointer while the old key still holds grants — otherwise
    # --disallow / rotation would miss the live authority.
    if new_allow_key is not None and new_allow_key != old_allow_key:
        outstanding = pa.pending_allow_grant_labels(
            pending_rows,
            allow_key_ss58=old_allow_key,
        )
        unknown = [f"#{row.get('index')}" for row in pending_rows if row.get("plaintext_missing")]
        if unknown:
            outstanding = [
                *outstanding,
                f"plaintext-missing on-chain announcement(s) {unknown}",
            ]
        blockers = pa.allow_key_change_blockers(
            old_allow_key_ss58=old_allow_key,
            new_allow_key_ss58=new_allow_key,
            allows=current_allows,
            pending_allows=pending_allows,
            delegations=data["proxies"],
            pending_allow_announcements=outstanding,
        )
        if blockers:
            app_ctx.output.error(
                f"cannot change --allow-key from {old_allow_key} to {allow_key_ss58} "
                "while the old key still holds (or is pending) zero-delay grants: "
                + "; ".join(blockers),
                help="cancel outstanding allow announcements (`btcli proxy cancel`) "
                "and/or `btcli proxy protect --disallow …` without changing "
                "--allow-key, execute those removals, then re-run with the new "
                "--allow-key and --allow …",
            )
            raise typer.Exit(2)
        existing = {
            **existing,
            "allow_key_ss58": new_allow_key,
            "allow_key_address": allow_key_ss58,
        }
        if not app_ctx.dry_run:
            pa.upsert_protected(existing)
    delegate = (
        allow_key_ss58
        if allow_key_ss58 is not None
        else (
            existing.get("allow_key_address") or existing.get("allow_key_ss58") or controller_ss58
        )
    )

    if app_ctx.dry_run:
        _preview_protect_update(
            app_ctx,
            existing,
            current_delay=current_delay,
            delay_blocks=delay_blocks,
            current_allows=current_allows,
            pending_rows=pending_rows,
            allow_types=allow_types,
            disallow_types=disallow_types,
            delegate=delegate,
        )
        return

    if delay_blocks != current_delay:
        # Rotate the delayed Any controller grant via announced batch.
        # Keep ``delay_blocks`` as the live value until the announcement
        # executes — otherwise show/cancel advertise a longer window than
        # the chain still enforces (and a cancelled update would stick).
        children = [
            RemoveProxy(
                delegate_ss58=controller_ss58, proxy_type="Any", delay=current_delay
            ).to_dict(),
            AddProxy(delegate_ss58=controller_ss58, proxy_type="Any", delay=delay_blocks).to_dict(),
        ]
        row = _announce_as_protected(
            app_ctx,
            existing,
            Batch(intents=children),
            category="delay-change",
            row_extra={"target_delay_blocks": delay_blocks},
        )
        if row is not None:
            existing = {
                **existing,
                "pending_delay_blocks": delay_blocks,
                "pending_delay_index": row["index"],
            }
            pa.upsert_protected(existing)

    outstanding_labels = pa.pending_allow_grant_labels(pending_rows, allow_key_ss58=delegate)
    for proxy_type in allow_types:
        if proxy_type in current_allows:
            continue
        already_queued = any(f"allow:{proxy_type} →" in label for label in outstanding_labels)
        if already_queued:
            if proxy_type not in pending_allows:
                pending_allows.append(proxy_type)
            continue
        # Stale pending_allows (announce never submitted) must be re-queued.
        row = _announce_as_protected(
            app_ctx,
            existing,
            AddProxy(delegate_ss58=delegate, proxy_type=proxy_type, delay=0),
            category=f"allow:{proxy_type}",
        )
        if row is None:
            raise typer.Exit(1)
        if proxy_type not in pending_allows:
            pending_allows.append(proxy_type)

    for proxy_type in disallow_types:
        # Pending AddProxy commits remain executable until cancelled. Queuing
        # RemoveProxy alone must not drop them from pending_allows / open an
        # allow-key retarget window.
        # Executing an older duplicate can clear the local pending flag while
        # a younger commitment survives. Inspect the reconciled queue even if
        # that flag is absent, including grants embedded in queued rotations.
        cancelled = _cancel_pending_allow_grants(app_ctx, existing, proxy_type)
        if not cancelled:
            app_ctx.output.error(
                f"could not cancel pending {proxy_type} grant announcement(s); "
                "refusing to queue a revocation that could be undone",
                help="run `btcli proxy show` then `btcli proxy cancel --index …`",
            )
            raise typer.Exit(2)
        existing = (
            pa.get_by_controller(existing["controller_wallet"], genesis_hash=genesis_hash)
            or existing
        )
        pending_allows = [t for t in pending_allows if t != proxy_type]
        # A permissionless executor may have installed the grant between
        # our first read and cancellation. Confirm live authority again.
        data = app_ctx.run(
            lambda c, pure=existing["protected_ss58"]: c.read("proxies", coldkey_ss58=pure)
        )
        current_allows = [
            str(grant["proxy_type"])
            for grant in pa.allow_key_grants(data["proxies"], old_allow_key)
            if grant["proxy_type"] in pa.PROXY_TYPE_INTENT_OPS
        ]
        if proxy_type not in current_allows:
            continue
        _announce_as_protected(
            app_ctx,
            existing,
            RemoveProxy(delegate_ss58=delegate, proxy_type=proxy_type, delay=0),
            category=f"disallow:{proxy_type}",
        )
        # Confirmed allows stay in ``allows`` until the disallow announcement
        # executes; do not drop pending grant tracking here.

    existing = {
        **existing,
        "allows": current_allows,
        "pending_allows": pending_allows,
        "allow_key_ss58": (
            new_allow_key if new_allow_key is not None else existing.get("allow_key_ss58")
        ),
        "allow_key_address": allow_key_ss58 or existing.get("allow_key_address"),
        "controller_ss58": controller_ss58,
    }
    pa.upsert_protected(existing)
    warn_defeating_proxies(app_ctx, existing)
    app_ctx.output.message(
        f"Protected account settings update queued for {existing.get('name')} "
        f"({existing['protected_ss58']}). "
        f"Run `btcli proxy execute --wallet {existing['controller_wallet']}` after the delay."
    )


def _pending_allow_grant_rows(
    pending: list[dict[str, Any]], record: dict[str, Any], proxy_type: str
) -> list[dict[str, Any]]:
    category = f"allow:{proxy_type}"
    delegate = record.get("allow_key_ss58") or record["controller_ss58"]
    return [
        row
        for row in pending
        if str(row.get("category") or "") == category
        or any(
            grant["delegate_ss58"]
            and _account_identity(grant["delegate_ss58"]) == _account_identity(delegate)
            and grant["proxy_type"] == proxy_type
            and grant["delay"] == 0
            for grant in pa.announcement_proxy_grants(row)
        )
        or (
            row.get("category") == "rotate-controller"
            and pa.controller_holds_allows(record, record["controller_ss58"])
            and proxy_type in (row.get("rotate_migrated_allows") or [])
        )
    ]


def _preview_protect_update(
    app_ctx: AppContext,
    record: dict[str, Any],
    *,
    current_delay: int,
    delay_blocks: int,
    current_allows: list[str],
    pending_rows: list[dict[str, Any]],
    allow_types: list[str],
    disallow_types: list[str],
    delegate: str,
) -> None:
    """Preview the same proposed settings without changing bindings or queues."""
    app_ctx.output.message(
        f"Dry run: {record['protected_ss58']}, allow key {delegate}, "
        f"active delay {current_delay} blocks; requested delay {delay_blocks} blocks."
    )
    if delay_blocks != current_delay:
        _announce_as_protected(
            app_ctx,
            record,
            Batch(
                intents=[
                    RemoveProxy(
                        delegate_ss58=record["controller_ss58"],
                        proxy_type="Any",
                        delay=current_delay,
                    ).to_dict(),
                    AddProxy(
                        delegate_ss58=record["controller_ss58"],
                        proxy_type="Any",
                        delay=delay_blocks,
                    ).to_dict(),
                ]
            ),
            category="delay-change",
        )
    labels = pa.pending_allow_grant_labels(pending_rows, allow_key_ss58=delegate)
    for proxy_type in allow_types:
        if proxy_type in current_allows or any(
            f"allow:{proxy_type} →" in label for label in labels
        ):
            continue
        _announce_as_protected(
            app_ctx,
            record,
            AddProxy(delegate_ss58=delegate, proxy_type=proxy_type, delay=0),
            category=f"allow:{proxy_type}",
        )
    for proxy_type in disallow_types:
        if any(row.get("plaintext_missing") for row in pending_rows):
            app_ctx.output.error(
                "cancel announcements with missing plaintext before revoking grants"
            )
            raise typer.Exit(2)
        for row in _pending_allow_grant_rows(pending_rows, record, proxy_type):
            app_ctx.output.message(f"Would cancel outstanding grant announcement #{row['index']}")
        if proxy_type in current_allows:
            _announce_as_protected(
                app_ctx,
                record,
                RemoveProxy(delegate_ss58=delegate, proxy_type=proxy_type, delay=0),
                category=f"disallow:{proxy_type}",
            )


def _cancel_pending_allow_grants(
    app_ctx: AppContext, record: dict[str, Any], proxy_type: str
) -> bool:
    """Cancel every known commitment that can restore this allow-key permission."""
    pending = _reconcile_pending(app_ctx, record)
    unknown = [str(row["index"]) for row in pending if row.get("plaintext_missing")]
    if unknown:
        app_ctx.output.error(
            "cannot verify grant revocation while announcements lack plaintext: #"
            + ", #".join(unknown),
            help="cancel these announcements first; they may restore the permission",
        )
        return False
    rows = _pending_allow_grant_rows(pending, record, proxy_type)
    if not rows:
        return True
    return all(_cancel_one(app_ctx, row) for row in rows)


def _announce_as_protected(
    app_ctx: AppContext,
    record: dict[str, Any],
    intent,
    *,
    category: str,
    row_extra: Optional[dict[str, Any]] = None,
) -> Optional[dict[str, Any]]:
    """Announce ``intent`` to run as the protected account (delayed Any path)."""
    from .prompt import confirm_wallet

    confirm_wallet(
        app_ctx,
        help_text="Wallet whose coldkey announces this protected-account call.",
        require_coldkey=True,
    )
    wallet = app_ctx.wallet()
    real = record["protected_ss58"]
    delay = _live_controller_delay(app_ctx, record, require_grant=True)
    if delay <= 0:
        app_ctx.output.error("controller has a zero-delay Any grant; protection is not active")
        raise typer.Exit(2)

    async def _prepare(client):
        composed, call_hash, _extras = await compose_announceable_call(
            client._substrate, intent, real, wallet
        )
        block = await client.block()
        spec_version = None
        with contextlib.suppress(Exception):
            spec_version = await client._substrate.spec_version()
        semantics = None
        with contextlib.suppress(Exception):
            semantics = await client.decode_scale("Call", bytes(composed.data))
        return composed, call_hash, block, spec_version, semantics

    composed, call_hash, block, spec_version, semantics = app_ctx.run(_prepare)
    _review_protected_announcement(app_ctx, intent, record, wallet)
    encoded = intent.to_dict()
    inner_args = {k: v for k, v in encoded.items() if k != "op"}
    recovered = _recover_existing_commitment(
        app_ctx,
        record,
        call_hash=call_hash,
        inner_op=intent.op,
        inner_args=inner_args,
        category=category,
        summary=intent.summary(),
        call_data=_call_data_hex(composed),
        runtime_spec_version=spec_version,
        call_semantics=semantics,
        delay_blocks=delay,
    )
    if recovered is not None:
        app_ctx.output.message(
            f"identical {category} announcement already exists on chain as "
            f"#{recovered['index']}; recovered its receipt instead of duplicating it"
        )
        return recovered
    with _preserve_controller_context(app_ctx, record) as controller_ss58:
        result = app_ctx.submit(
            AnnounceProxyCall(real_ss58=real, call_hash=call_hash),
            proxy_for="self",
            summary=f"announce {category}",
        )
    if result is None:
        return None
    if not result.success:
        raise typer.Exit(1)
    announced_at = block
    if result.extrinsic_id:
        with contextlib.suppress(ValueError):
            announced_at = int(str(result.extrinsic_id).split("-")[0])
    row = ann.add_announcement(
        controller_wallet=record["controller_wallet"],
        genesis_hash=record["genesis_hash"],
        delegate_ss58=controller_ss58,
        real_ss58=real,
        call_hash=call_hash,
        inner_op=intent.op,
        inner_args=inner_args,
        category=category,
        delay_blocks=delay,
        announced_at_block=announced_at,
        force_proxy_type="Any",
        summary=intent.summary(),
        call_data=_call_data_hex(composed),
        runtime_spec_version=spec_version,
        call_semantics=semantics,
    )
    row = _mark_announcement_from_submit(app_ctx, row, result) or row
    if row_extra:
        updated = ann.update_announcement(
            record["controller_wallet"],
            int(row["index"]),
            row_extra,
            genesis_hash=record["genesis_hash"],
        )
        if updated is not None:
            row = updated
    row = next(
        (live for live in _reconcile_pending(app_ctx, record) if live["index"] == row["index"]),
        row,
    )
    print_queued(app_ctx, record=record, row=row, current_block=announced_at)
    return row


def run_show(app_ctx: AppContext, *, index: Optional[int]) -> None:
    wallet_name = require_wallet_name(app_ctx)
    record = _get_protected_record(app_ctx, wallet_name)
    if record is None:
        app_ctx.output.error(
            f"wallet {wallet_name!r} does not control a local protected account",
            help="run `btcli proxy protect --wallet ...` first",
        )
        raise typer.Exit(1)

    current = app_ctx.run(lambda c: c.block())
    rows = _reconcile_pending(app_ctx, record)
    funding = _controller_funding(app_ctx, record)

    def _display_status(row):
        if row.get("status") == "awaiting_approval":
            status = (
                "approval_unknown"
                if row.get("approval_state") == "unknown"
                else "awaiting_approval"
            )
            return status, "not announced"
        executable_at = int(row["executable_at_block"])
        return ("ready" if current >= executable_at else "waiting"), _eta_for_block(
            executable_at, current
        )

    for row in rows:
        if row.get("approval_state_reason") and (index is None or row["index"] == index):
            app_ctx.output.message(f"#{row['index']}: {row['approval_state_reason']}")
    if any(row.get("provisional") for row in rows):
        app_ctx.output.message(
            "Dry-run discoveries have no saved index. Run `btcli proxy show` without "
            "`--dry-run` before cancelling a discovered announcement by index."
        )

    if index is not None:
        row = next((r for r in rows if int(r.get("index") or 0) == index), None)
        if row is None:
            app_ctx.output.error(
                f"no pending announcement #{index} for wallet {wallet_name!r} "
                "(checked local store and on-chain Proxy.Announcements)"
            )
            raise typer.Exit(1)
        call_hash = row["call_hash"]
        recomputed = None
        hash_match = False
        # Prefer hashing the exact SCALE bytes execution will replay. Rebuilding
        # from inner_args can drift for dynamic amounts (--all) after balance
        # changes and falsely report hash_match=false for a still-valid call.
        if "call_data" in row:
            from re import fullmatch

            saved = row["call_data"]
            if not isinstance(saved, str) or fullmatch(r"0x(?:[0-9a-fA-F]{2})+", saved) is None:
                app_ctx.output.message(
                    "[dim]invalid saved call data: expected nonempty 0x-prefixed "
                    "hexadecimal SCALE bytes; call hash was not verified[/dim]"
                )
            else:
                recomputed = _hash_call_data(saved)
                hash_match = recomputed.lower() == str(call_hash).lower()
        elif row.get("plaintext_missing") or not row.get("inner_op"):
            app_ctx.output.message(
                "[dim]on-chain announcement without local plaintext — "
                "cancellable, but call hash cannot be recomputed here[/dim]"
            )
        else:

            async def _verify(client):
                from ..intents.registry import build as build_intent

                _call, recomputed_hash, _ = await compose_announceable_call(
                    client._substrate,
                    build_intent(row["inner_op"], row["inner_args"]),
                    row["real_ss58"],
                    app_ctx.wallet(),
                )
                return recomputed_hash

            try:
                recomputed = app_ctx.run(_verify)
                hash_match = recomputed.lower() == str(call_hash).lower()
            except Exception as error:
                recomputed = None
                hash_match = False
                app_ctx.output.message(f"[dim]could not recompute call hash: {error}[/dim]")

        status, countdown = _display_status(row)
        detail = {
            **row,
            "status": status,
            "current_block": current,
            "countdown": countdown,
            "hash_match": hash_match,
            "recomputed_call_hash": recomputed,
            "protected": {
                "name": record.get("name"),
                "ss58": record["protected_ss58"],
                "delay_blocks": record.get("delay_blocks"),
            },
            "controller_funding": funding,
        }
        app_ctx.output.detail(f"announcement #{index}", detail)
        return

    out = app_ctx.output
    if out.json_mode:
        payload = []
        for row in rows:
            status, countdown = _display_status(row)
            payload.append(
                {
                    **row,
                    "status": status,
                    "countdown": countdown,
                }
            )
        out.value(
            {
                "protected": record,
                "current_block": current,
                "pending": payload,
                "controller_funding": funding,
            }
        )
        return

    name = record.get("name") or pa.default_protected_name(wallet_name)
    delay = record.get("delay_blocks")
    title = f"protected {name} ({record['protected_ss58']}) — delay {delay} blocks"
    if "available_rao" in funding:
        from ..balance import Balance

        out.message(
            f"Controller {record['controller_ss58']}: available "
            f"{Balance.from_rao(funding['available_rao'])}, reserved "
            f"{Balance.from_rao(funding['reserved_rao'])}; recommended "
            f"{Balance.from_rao(funding['target_rao'])}. "
            "Next announcement deposit: "
            f"{Balance.from_rao(funding['next_announcement_deposit_rao'])}; "
            f"can announce: {funding['can_announce']}. Fees are estimates."
        )
    if funding.get("low", True):
        _funding_warning(app_ctx, funding)
    table_rows = []
    for row in rows:
        status, countdown = _display_status(row)
        category = row.get("category") or row.get("inner_op") or "unknown"
        if row.get("plaintext_missing"):
            category = f"{category}*"
        table_rows.append(
            [
                "unsaved" if row.get("index") is None else str(row["index"]),
                category,
                "after approval"
                if row.get("status") == "awaiting_approval"
                else str(row["executable_at_block"]),
                countdown,
                status,
            ]
        )
    if not table_rows:
        out.message(f"{title}\n(no pending announcements)")
        return
    out.columns(
        title,
        ["index", "category", "executable at", "countdown", "status"],
        table_rows,
        rows,
    )


def run_cancel(app_ctx: AppContext, *, index: Optional[int], yes: bool) -> None:
    wallet_name = require_wallet_name(app_ctx)
    record = _get_protected_record(app_ctx, wallet_name)
    if record is None:
        app_ctx.output.error(f"wallet {wallet_name!r} does not control a local protected account")
        raise typer.Exit(1)

    pending = _reconcile_pending(app_ctx, record)
    if index is not None:
        row = next((r for r in pending if int(r.get("index") or 0) == index), None)
        if row is None:
            app_ctx.output.error(
                f"no pending announcement #{index} for wallet {wallet_name!r} "
                "(checked local store and on-chain Proxy.Announcements)"
            )
            raise typer.Exit(1)
        _cancel_one(app_ctx, row)
        return

    if not pending:
        app_ctx.output.message("no pending announcements to cancel")
        return
    if not yes:
        app_ctx.confirm(f"Cancel all {len(pending)} pending transactions?")
    for row in pending:
        _cancel_one(app_ctx, row)


def _announcement_still_on_chain(app_ctx: AppContext, row: dict[str, Any]) -> bool:
    """True when the row's call hash is still listed under the announcing delegate."""
    controller = row.get("delegate_ss58")
    call_hash = str(row.get("call_hash") or "").lower()
    real = row.get("real_ss58")
    if not controller or not call_hash:
        return True
    data = app_ctx.run(lambda c: c.read("proxy_announcements", delegate_ss58=controller))
    for item in data.get("announcements") or []:
        if str(item.get("call_hash") or "").lower() != call_hash:
            continue
        if real and str(item.get("real") or "") != str(real):
            continue
        return True
    return False


def _cancel_one(app_ctx: AppContext, row: dict[str, Any]) -> bool:
    # Multisig controllers Multisig-wrap this call (remove_announcement is NOT a
    # fee-payer-only exception). Single coldkeys submit as themselves.
    from . import multisig_helpers as ms_helpers

    if row.get("status") == "awaiting_approval":
        approval_pending = _pending_multisig_announcement(app_ctx, row)
        if approval_pending is False:
            if getattr(app_ctx, "dry_run", False):
                app_ctx.output.message(
                    f"#{row['index']}: would clear the abandoned multisig announcement"
                )
                return True
            ann.mark_status(
                row["controller_wallet"],
                int(row["index"]),
                "gone",
                genesis_hash=row.get("genesis_hash"),
            )
            _clear_pending_allow_after_cancel(row)
            app_ctx.output.message(
                f"#{row['index']}: underlying multisig operation is gone; plaintext retained"
            )
            return True
        if approval_pending is None:
            app_ctx.output.error(
                f"#{row['index']}: cannot verify whether the underlying multisig "
                "operation remains pending; "
                "retry with an archive RPC that can read the original approval block. "
                "The saved plaintext and pending grant have been retained."
            )
        else:
            app_ctx.output.error(
                f"#{row['index']} is still awaiting multisig approval to land on chain; "
                "complete or cancel the underlying multisig operation before cancelling "
                "with remove_announcement"
            )
        return False

    record = pa.get_by_controller(
        row["controller_wallet"], genesis_hash=row.get("genesis_hash")
    ) or {
        "controller_wallet": row["controller_wallet"],
        "controller_ss58": row.get("delegate_ss58"),
    }
    with _preserve_controller_context(app_ctx, record):
        result = app_ctx.submit(
            RemoveProxyAnnouncement(real_ss58=row["real_ss58"], call_hash=row["call_hash"]),
            proxy_for="self",
            summary=f"cancel announcement #{row['index']}",
        )
    if result is None or not result.success:
        return False
    still = _announcement_still_on_chain(app_ctx, row)
    if not ms_helpers.announcement_cancel_completed(result.events or [], still_on_chain=still):
        if ms_helpers.multisig_approval_only(result.events or []):
            app_ctx.output.message(
                f"#{row['index']}: cancel approved but awaiting further multisig "
                "signatures — announcement remains pending/executable"
            )
        else:
            app_ctx.output.message(
                f"#{row['index']}: cancel submitted but announcement still on chain; "
                "leaving local plaintext pending"
            )
        return False
    ann.mark_status(
        row["controller_wallet"],
        int(row["index"]),
        "cancelled",
        genesis_hash=row.get("genesis_hash"),
    )
    _clear_pending_allow_after_cancel(row)
    if row.get("category") == "rotate-controller":
        latest = pa.get_by_controller(
            row["controller_wallet"], genesis_hash=row.get("genesis_hash")
        )
        pending = (latest or {}).get("pending_rotate") or {}
        if latest is not None and pending.get("index") == row["index"]:
            latest.pop("pending_rotate", None)
            pa.upsert_protected(latest)
    app_ctx.output.message(f"cancelled #{row['index']}")
    return True


def _clear_pending_allow_after_cancel(row: dict[str, Any]) -> None:
    """Reconcile cached grants after any cancelled payload, including batches."""
    record = pa.get_by_controller(row["controller_wallet"], genesis_hash=row.get("genesis_hash"))
    if record is None:
        return
    remaining = ann.list_for_wallet(row["controller_wallet"], genesis_hash=row.get("genesis_hash"))
    cancelled_types = {
        grant["proxy_type"] for grant in pa.announcement_proxy_grants(row) if grant["delay"] == 0
    }
    surviving_types = {
        grant["proxy_type"]
        for pending in remaining
        for grant in pa.announcement_proxy_grants(pending)
        if grant["delay"] == 0
    }
    if any(
        pending.get("plaintext_missing") or pending.get("category") == "unknown"
        for pending in remaining
    ):
        return
    pending_allows = [
        proxy_type
        for proxy_type in record.get("pending_allows") or []
        if proxy_type not in cancelled_types or proxy_type in surviving_types
    ]
    if pending_allows != list(record.get("pending_allows") or []):
        pa.upsert_protected({**record, "pending_allows": pending_allows})


def run_execute_protected(
    app_ctx: AppContext, *, index: Optional[int], fee_payer: Optional[str] = None
) -> None:
    wallet_name = require_wallet_name(app_ctx)
    record = _get_protected_record(app_ctx, wallet_name)
    if record is None:
        app_ctx.output.error(
            f"wallet {wallet_name!r} does not control a local protected account",
            help="for low-level announced execute without a protected record, "
            "pass --delegate / --real / --inner-op",
        )
        raise typer.Exit(1)

    current = app_ctx.run(lambda c: c.block())
    pending = _reconcile_pending(app_ctx, record)
    if index is not None:
        row = next((r for r in pending if int(r.get("index") or 0) == index), None)
        if row is None:
            app_ctx.output.error(
                f"no pending announcement #{index} for wallet {wallet_name!r} "
                "(checked local store and on-chain Proxy.Announcements)"
            )
            raise typer.Exit(1)
        _execute_one(app_ctx, row, current, fee_payer=fee_payer)
        return

    def _idx(row: dict[str, Any]) -> Optional[int]:
        # Dry-run discoveries from _reconcile_pending keep index=None (unsaved).
        raw = row.get("index")
        return None if raw is None else int(raw)

    def _label(row: dict[str, Any]) -> str:
        idx = _idx(row)
        return f"#{idx}" if idx is not None else "unsaved"

    awaiting = [
        r for r in pending if r.get("status") == "awaiting_approval" or not r.get("on_chain", True)
    ]
    awaiting_idxs = {i for r in awaiting if (i := _idx(r)) is not None}
    ready = []
    waiting = []
    for row in pending:
        idx = _idx(row)
        if idx in awaiting_idxs:
            continue
        if current >= int(row["executable_at_block"]) and not row.get("plaintext_missing"):
            if idx is not None:
                ready.append(row)
        elif current < int(row["executable_at_block"]):
            waiting.append(row)
    unknown = [r for r in pending if r.get("plaintext_missing")]
    for row in awaiting:
        app_ctx.output.message(
            f"skipping {_label(row)} ({row.get('category')}): awaiting multisig "
            "approval before Proxy.announce lands on chain"
        )
    for row in waiting:
        app_ctx.output.message(
            f"skipping {_label(row)} ({row.get('category')}): not ready — "
            f"{_eta_for_block(int(row['executable_at_block']), current)}"
        )
    for row in unknown:
        app_ctx.output.message(
            f"skipping {_label(row)}: on-chain hash without local plaintext "
            "(cancel with `btcli proxy cancel --index …`, or re-announce from this device)"
        )
    if not ready:
        if not waiting and not unknown:
            app_ctx.output.message("no pending announcements")
        return
    for row in ready:
        _execute_one(app_ctx, row, current, fee_payer=fee_payer)


def _execute_one(
    app_ctx: AppContext,
    row: dict[str, Any],
    current_block: int,
    *,
    fee_payer: Optional[str] = None,
) -> None:
    if row.get("plaintext_missing") or not row.get("inner_op"):
        app_ctx.output.error(
            f"#{row['index']} has no local plaintext — cannot execute; cancel instead "
            "or re-announce the call from this device"
        )
        return
    if row.get("status") == "awaiting_approval" or not row.get("on_chain", True):
        app_ctx.output.error(
            f"#{row['index']} has not landed on chain yet "
            "(still awaiting multisig approval for Proxy.announce)"
        )
        return
    executable_at = int(row["executable_at_block"])
    if current_block < executable_at:
        app_ctx.output.message(
            f"#{row['index']} not ready — {_eta_for_block(executable_at, current_block)}"
        )
        return
    record = pa.get_by_controller(
        row["controller_wallet"], genesis_hash=row.get("genesis_hash")
    ) or {
        "controller_wallet": row["controller_wallet"],
        "controller_ss58": row.get("delegate_ss58"),
    }
    context = (
        _execution_fee_payer(app_ctx, fee_payer)
        if fee_payer
        else _preserve_controller_context(app_ctx, record)
    )
    with context:
        result = app_ctx.submit(
            ExecuteProxyAnnounced(
                delegate_ss58=row["delegate_ss58"],
                real_ss58=row["real_ss58"],
                inner_op=row["inner_op"],
                inner_args=row.get("inner_args") or {},
                force_proxy_type=row.get("force_proxy_type") or "Any",
                announced_call_data=row.get("call_data"),
                expected_call_hash=row.get("call_hash"),
                announced_spec_version=row.get("runtime_spec_version"),
                announced_call_semantics=row.get("call_semantics"),
            ),
            proxy_for="self",
            summary=(
                f"execute announcement #{row['index']}: {row.get('summary') or row['inner_op']}"
            ),
        )
    if result is not None and result.success:
        ann.mark_status(
            row["controller_wallet"],
            int(row["index"]),
            "executed",
            genesis_hash=row.get("genesis_hash"),
        )
        app_ctx.output.message(f"executed #{row['index']}")
        record = pa.get_by_controller(
            row["controller_wallet"], genesis_hash=row.get("genesis_hash")
        )
        category = str(row.get("category") or "")
        if record is not None and category.startswith("allow:"):
            proxy_type = category.split(":", 1)[1]
            allows = list(record.get("allows") or [])
            pending = list(record.get("pending_allows") or [])
            if proxy_type not in allows:
                allows.append(proxy_type)
            # proxy_announced consumes matured duplicates only; younger
            # commitments with the same hash still need cancellation tracking.
            outstanding = pa.pending_allow_grant_labels(_reconcile_pending(app_ctx, record))
            if any(f"allow:{proxy_type} →" in label for label in outstanding):
                if proxy_type not in pending:
                    pending.append(proxy_type)
            else:
                pending = [t for t in pending if t != proxy_type]
            record = {**record, "allows": allows, "pending_allows": pending}
            pa.upsert_protected(record)
        if record is not None and category.startswith("disallow:"):
            proxy_type = category.split(":", 1)[1]
            allows = [t for t in (record.get("allows") or []) if t != proxy_type]
            pending = [t for t in (record.get("pending_allows") or []) if t != proxy_type]
            record = {**record, "allows": allows, "pending_allows": pending}
            pa.upsert_protected(record)
        if record is not None and row.get("category") == "delay-change":
            # Each row commits its own change. A newer request may have replaced
            # the local pending pointer, but must never override the live delay.
            live = _live_controller_delay(app_ctx, record, require_grant=True)
            record = {**record, "delay_blocks": live}
            if record.get("pending_delay_index") == row["index"] or (
                record.get("pending_delay_index") is None
                and record.get("pending_delay_blocks") == row.get("target_delay_blocks")
            ):
                record.pop("pending_delay_blocks", None)
                record.pop("pending_delay_index", None)
            pa.upsert_protected(record)
            app_ctx.output.message(f"active delay is {live} blocks (on-chain confirmed)")
        if record is not None and row.get("category") == "rotate-controller":
            # Prefer destination committed on THIS announcement row — not the
            # record's latest pending_rotate (a later rotate would otherwise
            # rebind to the wrong controller after executing an earlier one).
            pending = record.get("pending_rotate") or {}
            to_name = row.get("rotate_to_wallet") or pending.get("to_wallet")
            to_ss58 = row.get("rotate_to_ss58") or pending.get("to_ss58")
            if to_name and to_ss58:
                to_ss58 = _account_identity(to_ss58)
                old_ss58 = row.get("delegate_ss58") or record.get("controller_ss58")
                old_ss58 = _account_identity(str(old_ss58)) if old_ss58 else None
                if not _rotation_complete_on_chain(
                    app_ctx,
                    record,
                    new_controller_ss58=to_ss58,
                    old_controller_ss58=old_ss58,
                ):
                    app_ctx.output.error(
                        f"rotate announcement #{row['index']} executed but on-chain "
                        f"grants do not show {to_ss58} as delayed Any with the old "
                        "controller fully revoked; leaving local binding unchanged"
                    )
                else:
                    moved = _move_protected_binding(
                        app_ctx,
                        row["controller_wallet"],
                        to_name,
                        controller_ss58=to_ss58,
                        genesis_hash=row.get("genesis_hash"),
                    )
                    if moved is not None:
                        # Drop pending_rotate only when it matches this row.
                        pending_now = moved.get("pending_rotate") or {}
                        if (
                            pending_now.get("to_ss58") == to_ss58
                            or pending_now.get("to_wallet") == to_name
                        ):
                            moved.pop("pending_rotate", None)
                        allow_key = moved.get("allow_key_ss58")
                        if allow_key and old_ss58 and str(allow_key) == str(old_ss58):
                            moved["allow_key_ss58"] = to_ss58
                            moved.pop("allow_key_address", None)
                        moved["pending_allows"] = []
                        pa.upsert_protected(moved)
                        _refresh_live_allows(app_ctx, moved)
                        ann.move_wallet_announcements(
                            row["controller_wallet"],
                            to_name,
                            genesis_hash=row.get("genesis_hash"),
                        )
                        app_ctx.output.message(
                            f"local protected-account record moved: "
                            f"{row['controller_wallet']} → {to_name} ({to_ss58})"
                        )


@contextlib.contextmanager
def _execution_fee_payer(app_ctx: AppContext, wallet_name: str):
    """Select a single local signer that pays only for permissionless execution."""
    saved = (
        app_ctx.wallet_name,
        app_ctx.wallet_given,
        app_ctx.multisig_wallet_name,
        app_ctx.signatory_wallet,
        list(app_ctx.signatory_wallets),
        app_ctx.signer_backend,
        app_ctx.signer_address,
    )
    try:
        app_ctx.wallet_name = wallet_name
        app_ctx.wallet_given = True
        app_ctx.multisig_wallet_name = None
        app_ctx.signatory_wallet = None
        app_ctx.signatory_wallets = []
        app_ctx.signer_backend = None
        app_ctx.signer_address = None
        yield
    finally:
        (
            app_ctx.wallet_name,
            app_ctx.wallet_given,
            app_ctx.multisig_wallet_name,
            app_ctx.signatory_wallet,
            app_ctx.signatory_wallets,
            app_ctx.signer_backend,
            app_ctx.signer_address,
        ) = saved


def _rotation_complete_on_chain(
    app_ctx: AppContext,
    record: dict[str, Any],
    *,
    new_controller_ss58: str,
    old_controller_ss58: Optional[str],
) -> bool:
    """True when new holds delayed Any and the old controller has zero grants left."""
    data = app_ctx.run(lambda c: c.read("proxies", coldkey_ss58=record["protected_ss58"]))
    return pa.rotation_complete(
        data["proxies"],
        new_controller_ss58=new_controller_ss58,
        old_controller_ss58=old_controller_ss58,
        delay_blocks=int(record.get("delay_blocks") or 0),
    )


def _move_protected_binding(app_ctx: AppContext, from_wallet: str, to_wallet: str, **kwargs):
    try:
        return pa.move_protected_controller(from_wallet, to_wallet, **kwargs)
    except ValueError as error:
        app_ctx.output.error(str(error))
        raise typer.Exit(2) from error


def _rotation_batch_children(
    app_ctx: AppContext,
    record: dict[str, Any],
    *,
    controller_ss58: str,
    new_ss58: str,
    delay: int,
) -> tuple[list[dict[str, Any]], list[str]]:
    """Build the atomic rotate batch and the allow types migrated to ``new_ss58``.

    Reset the grant set at execution, including grants created after announcement.
    Reinstall delayed Any for the new controller, live controller-owned allows,
    and the separate delegates reviewed at announcement time. The entire batch
    rolls back if any replacement fails.
    """
    data = app_ctx.run(lambda c: c.read("proxies", coldkey_ss58=record["protected_ss58"]))
    controller_ss58 = _account_identity(controller_ss58)
    live_allows = [
        str(grant["proxy_type"])
        for grant in pa.allow_key_grants(data["proxies"], controller_ss58)
        if grant["proxy_type"] in pa.PROXY_TYPE_INTENT_OPS
    ]
    migrate = pa.controller_owned_allow_types({**record, "allows": live_allows}, controller_ss58)
    children: list[dict[str, Any]] = [
        RemoveProxies().to_dict(),
        AddProxy(delegate_ss58=new_ss58, proxy_type="Any", delay=delay).to_dict(),
    ]
    # Preserve only grants represented by this protected record's explicit
    # allow-key policy. An arbitrary non-controller grant may have been planted
    # by an attacker and rotation is the recovery boundary that must clear it.
    allow_key = _account_identity(record.get("allow_key_ss58"))
    recorded_allows = set(record.get("allows") or [])
    replacements = [
        (grant["delegate"], str(grant["proxy_type"]), int(grant["delay"]))
        for grant in data["proxies"]
        if allow_key
        and _account_identity(grant["delegate"]) == allow_key
        and str(grant["proxy_type"]) in recorded_allows
        and int(grant["delay"]) == 0
    ]
    for proxy_type in migrate:
        replacements.append((new_ss58, proxy_type, 0))
    seen = {(_account_identity(new_ss58), "Any", delay)}
    for delegate, proxy_type, grant_delay in replacements:
        identity = (_account_identity(delegate), proxy_type, grant_delay)
        if identity in seen:
            continue
        seen.add(identity)
        children.append(
            AddProxy(
                delegate_ss58=delegate,
                proxy_type=proxy_type,
                delay=grant_delay,
                from_hotkey=False,
            ).to_dict()
        )
    return children, migrate


def _resolve_rotation_destination(app_ctx: AppContext, target: str) -> tuple[str, str, str]:
    """Bind raw destinations to the local name used for subsequent wallet routing.

    Reverse lookup reads only public metadata. A matching address-book label is
    not sufficient: automatic protected routing is keyed by a signing wallet or
    saved multisig name, and choosing between duplicate local names is a user's
    decision.
    """
    from .. import config as cfg
    from .. import wallets
    from ..keyfiles import KeyfileError
    from ..receiving import receiving_address
    from . import multisig_helpers as ms_helpers
    from .wallet_addresses import public_key_from_file

    address = app_ctx.resolve_address("coldkey_ss58", target)
    if address is None:
        raise ValueError("controller rotation requires a destination address")
    account = _account_identity(address)
    raw_target = is_receiving_address(target) or wallets.is_bittensor_address(target)

    def alias_address(name):
        address = cfg.get_address(name) or (cfg.get_proxy(name) or {}).get("address")
        return address if isinstance(address, str) else None

    if not raw_target and not alias_address(target):
        # Without an address/proxy-book alias, the ordinary resolver accepts a
        # name only through a local public wallet or a saved multisig preset.
        return target, address, account

    wallet_path = getattr(app_ctx, "wallet_path", None) or wallets.DEFAULT_WALLET_PATH
    preset_names = {
        entry["name"]
        for entry in cfg.load_multisigs()
        if isinstance(entry.get("name"), str) and entry["name"]
    }
    local_names = wallets.list_wallets(wallet_path)
    matches: dict[str, str] = {}
    for name in local_names:
        # -w selects a saved multisig before a same-named wallet directory.
        if name in preset_names:
            continue
        try:
            wallet = wallets.open_wallet(name=name, path=wallet_path)
            public = public_key_from_file(wallet.coldkeypub_file.path)
        except (KeyfileError, OSError, ValueError):
            continue
        if public.ss58_address == account:
            matches[name] = receiving_address(public)

    def has_public_signatories(name, seen):
        if name in seen:
            return False
        entry = cfg.get_multisig(name)
        if (
            entry is None
            or not isinstance(entry.get("signatories"), list)
            or not entry["signatories"]
            or not isinstance(entry.get("threshold"), int)
            or entry["threshold"] < 1
        ):
            return False
        for ref in entry.get("signatories") or []:
            if not isinstance(ref, str):
                return False
            if is_receiving_address(ref) or wallets.is_bittensor_address(ref) or alias_address(ref):
                continue
            if cfg.get_multisig(ref) is not None:
                if not has_public_signatories(ref, seen | {name}):
                    return False
            else:
                try:
                    wallet = wallets.open_wallet(name=ref, path=wallet_path)
                    public_key_from_file(wallet.coldkeypub_file.path)
                except (KeyfileError, OSError, ValueError):
                    return False
        return True

    for name in sorted(preset_names):
        if not has_public_signatories(name, set()):
            continue
        try:
            # Use the signing resolver, including proxy-book and nested preset
            # references. The bulk display helper intentionally has softer,
            # incomplete name resolution and cannot establish signing identity.
            multisig_address = ms_helpers.derive_saved_multisig_address(app_ctx, name)
        except (KeyfileError, OSError, ValueError, typer.Exit):
            continue
        if multisig_address and _account_identity(multisig_address) == account:
            matches[name] = multisig_address
    # A name used by an address-book alias must still resolve to this signing
    # account on later commands. Do not bind a matching key under a shadowed name.
    for name in list(matches):
        alias = alias_address(name)
        if alias and _account_identity(alias) != account:
            del matches[name]

    if not raw_target and target in matches:
        return target, matches[target], account

    shadowed_target = not raw_target and (target in local_names or target in preset_names)
    if shadowed_target or len(matches) != 1:
        if shadowed_target:
            explanation = f"does not match the signing account of local name {target!r}"
        else:
            explanation = (
                "matches multiple local wallets or saved multisigs: " + ", ".join(sorted(matches))
                if matches
                else "has no matching local wallet or saved multisig"
            )
        app_ctx.output.error(
            f"rotation destination {account} {explanation}",
            help="pass --to with an explicit local wallet or saved multisig name; "
            "create or import that local entry first if needed",
        )
        raise typer.Exit(2)
    name, local_address = next(iter(matches.items()))
    # A hashed local key can supply the complete descriptor even when the user
    # pasted its bare SS58 account. The persisted identity remains the account.
    return name, address if is_receiving_address(address) else local_address, account


def run_rotate(app_ctx: AppContext, *, to_wallet: str) -> None:
    wallet_name = require_wallet_name(app_ctx)
    record = _get_protected_record(app_ctx, wallet_name)
    if record is None:
        app_ctx.output.error(f"wallet {wallet_name!r} does not control a local protected account")
        raise typer.Exit(1)

    to_name, new_address, new_ss58 = _resolve_rotation_destination(app_ctx, to_wallet)

    destination = pa.get_by_controller(to_name, genesis_hash=record["genesis_hash"])
    if (
        destination is not None
        and _account_identity(destination["protected_ss58"]) != record["protected_ss58"]
    ):
        app_ctx.output.error(
            f"wallet {to_name!r} already controls protected account "
            f"{destination['protected_ss58']} on this network; choose another wallet"
        )
        raise typer.Exit(2)
    pending = _reconcile_pending(app_ctx, record)
    if pending:
        labels = ", ".join(
            f"#{row.get('index')} {row.get('category') or row.get('inner_op') or 'unknown'}"
            for row in pending
        )
        grants = pa.pending_allow_grant_labels(pending)
        reason = (
            "cannot rotate while allow grants are still pending: "
            if grants
            else "cannot rotate while the old controller has pending announcements: "
        )
        app_ctx.output.error(
            reason + labels,
            help="execute the operations you still want and cancel the rest, then rerun "
            "rotation; otherwise their deposits and plaintext can be stranded",
        )
        raise typer.Exit(2)
    record = _refresh_live_allows(app_ctx, record)
    delay = _live_controller_delay(app_ctx, record, require_grant=True)
    record = {**record, "delay_blocks": delay}
    controller_ss58 = str(record.get("controller_ss58") or _controller_ss58(app_ctx))
    if pa.controller_holds_allows(record, controller_ss58) and (record.get("pending_allows") or []):
        app_ctx.output.error(
            "cannot rotate while allow grants are still pending for this controller",
            help="run `btcli proxy execute` or `btcli proxy cancel` for pending "
            "allow:/disallow: announcements first, then rotate — otherwise a later "
            "execute would re-grant the old controller",
        )
        raise typer.Exit(2)

    children, migrated = _rotation_batch_children(
        app_ctx,
        record,
        controller_ss58=controller_ss58,
        new_ss58=new_address,
        delay=delay,
    )
    retained = [
        child
        for child in children
        if child.get("op") == "add_proxy"
        and _account_identity(child.get("delegate_ss58")) != _account_identity(new_ss58)
    ]
    app_ctx.output.message(
        "Rotation atomically clears every proxy grant, installs the new delayed "
        "controller, and restores only the protected record's explicit allow-key "
        f"grants: {retained or 'none'}. Grants added outside that policy, including "
        "during the waiting period, will be removed; proxy fee sponsorship will be reset."
    )
    row = _announce_as_protected(
        app_ctx,
        record,
        Batch(intents=children),
        category="rotate-controller",
        row_extra={
            "rotate_to_wallet": to_name,
            "rotate_to_ss58": new_ss58,
            "rotate_migrated_allows": migrated,
        },
    )
    if row is None:
        return
    # Stash the intended rebind; applied after the matching announcement executes.
    record = {
        **record,
        "pending_rotate": {
            "index": row["index"],
            "to_wallet": to_name,
            "to_ss58": new_ss58,
            "migrated_allows": migrated,
        },
    }
    pa.upsert_protected(record)
    migrate_note = f" Migrating allows {migrated} to the new controller." if migrated else ""
    app_ctx.output.message(
        f"Controller rotate queued: add {to_name} ({new_ss58}), remove {wallet_name}."
        f"{migrate_note}\n"
        f"After `btcli proxy execute --wallet {wallet_name}`, rebind local config with:\n"
        f"  btcli proxy rotate --wallet {wallet_name} --to {to_name} --finalize\n"
        "Or re-run rotate with --finalize once the announcement has executed."
    )


def run_rotate_finalize(app_ctx: AppContext, *, to_wallet: str) -> None:
    wallet_name = require_wallet_name(app_ctx)
    record = _get_protected_record(app_ctx, wallet_name)
    if record is None:
        app_ctx.output.error(f"wallet {wallet_name!r} does not control a local protected account")
        raise typer.Exit(1)
    # Several rotations can be announced before one executes. The explicit
    # destination identifies the outcome to verify; the latest local pointer
    # must never silently redirect this command to a different wallet.
    to_name, _new_address, to_ss58 = _resolve_rotation_destination(app_ctx, to_wallet)
    old_ss58 = record.get("controller_ss58")
    if not _rotation_complete_on_chain(
        app_ctx,
        record,
        new_controller_ss58=to_ss58,
        old_controller_ss58=old_ss58,
    ):
        app_ctx.output.error(
            f"on-chain rotation to {to_ss58} is not complete yet "
            "(new delayed Any missing, or old controller still has proxy grants); "
            "execute the rotate announcement first, then --finalize"
        )
        raise typer.Exit(2)
    if getattr(app_ctx, "dry_run", False):
        app_ctx.output.message(
            f"would move local protected-account record: {wallet_name} → {to_name} ({to_ss58})"
        )
        return
    moved = _move_protected_binding(
        app_ctx, wallet_name, to_name, controller_ss58=to_ss58, genesis_hash=record["genesis_hash"]
    )
    if moved is None:
        raise typer.Exit(1)
    moved.pop("pending_rotate", None)
    allow_key = moved.get("allow_key_ss58")
    if allow_key and old_ss58 and str(allow_key) == str(old_ss58):
        moved["allow_key_ss58"] = to_ss58
        moved.pop("allow_key_address", None)
    moved["pending_allows"] = []
    pa.upsert_protected(moved)
    _refresh_live_allows(app_ctx, moved)
    ann.move_wallet_announcements(wallet_name, to_name, genesis_hash=record["genesis_hash"])
    app_ctx.output.message(
        f"local protected-account record moved: {wallet_name} → {to_name} ({to_ss58})"
    )
