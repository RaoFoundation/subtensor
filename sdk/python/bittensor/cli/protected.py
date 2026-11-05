"""Protected-account CLI helpers: setup, auto-announce, show/cancel/execute/rotate."""

from __future__ import annotations

import contextlib
from datetime import datetime, timedelta
from typing import Any, Optional

import typer

from .. import protected_account as pa
from .. import proxy_announcements as ann
from ..intents import (
    AddProxy,
    AnnounceColdkeySwap,
    AnnounceProxyCall,
    Batch,
    CreatePureProxy,
    ExecuteProxyAnnounced,
    RemoveProxy,
    RemoveProxyAnnouncement,
)
from ..intents.proxy import compose_announceable_call
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
    wallet_name = app_ctx.wallet_name
    if not wallet_name:
        return proxy_for, None, False
    record = pa.get_by_controller(wallet_name)
    if record is None:
        return proxy_for, None, False
    if intent.op in PROTECTED_BYPASS_OPS:
        return proxy_for, None, False

    instant = pa.instant_proxy_type_for(intent.op, record)
    if instant is not None:
        return record["protected_ss58"], instant, False

    # Delayed path: announce instead of executing.
    _submit_protected_announcement(app_ctx, intent, record)
    return None, None, True


def _controller_ss58(app_ctx: AppContext) -> str:
    return app_ctx.resolve_address("coldkey_ss58", None)


def _submit_protected_announcement(app_ctx: AppContext, intent, record: dict[str, Any]) -> None:
    from .prompt import confirm_wallet

    confirm_wallet(
        app_ctx,
        help_text="Wallet whose coldkey announces this protected-account call.",
        require_coldkey=True,
    )
    wallet = app_ctx.wallet()
    real = record["protected_ss58"]
    delay = int(record.get("delay_blocks") or 0)
    if delay <= 0:
        app_ctx.output.error(
            "protected account record has delay_blocks <= 0; refusing to announce",
            help="re-run `btcli proxy protect` with a positive --delay",
        )
        raise typer.Exit(2)

    async def _prepare(client):
        _call, call_hash, _extras = await compose_announceable_call(
            client._substrate, intent, real, wallet
        )
        block = await client.block()
        return call_hash, block, intent.to_dict()

    call_hash, block, encoded = app_ctx.run(_prepare)
    inner_args = {k: v for k, v in encoded.items() if k != "op"}
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
        controller_wallet=record["controller_wallet"],
        delegate_ss58=_controller_ss58(app_ctx),
        real_ss58=real,
        call_hash=call_hash,
        inner_op=intent.op,
        inner_args=inner_args,
        category=ann.category_for_op(intent.op),
        delay_blocks=delay,
        announced_at_block=announced_at,
        force_proxy_type="Any",
        summary=intent.summary(),
    )
    print_queued(app_ctx, record=record, row=row, current_block=announced_at)


def warn_defeating_proxies(app_ctx: AppContext, record: dict[str, Any]) -> None:
    controller = record.get("controller_ss58") or _controller_ss58(app_ctx)
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


def run_protect(
    app_ctx: AppContext,
    *,
    delay_raw: str,
    allow: list[str],
    disallow: list[str],
    allow_key: Optional[str],
) -> None:
    """Create or update a protected account for ``-w``."""
    wallet_name = require_wallet_name(app_ctx)
    try:
        delay_blocks = pa.parse_delay_to_blocks(delay_raw)
    except ValueError as error:
        app_ctx.output.error(str(error))
        raise typer.Exit(2) from error

    try:
        allow_types = pa.resolve_allow_items(allow) if allow else []
        disallow_types = pa.resolve_allow_items(disallow) if disallow else []
    except ValueError as error:
        app_ctx.output.error(str(error))
        raise typer.Exit(2) from error

    allow_key_ss58 = None
    if allow_key:
        allow_key_ss58 = app_ctx.resolve_address("coldkey_ss58", allow_key)

    existing = pa.get_by_controller(wallet_name)
    if existing is None:
        _protect_create(
            app_ctx,
            wallet_name=wallet_name,
            delay_blocks=delay_blocks,
            allow_types=allow_types,
            allow_key_ss58=allow_key_ss58,
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


def _protect_create(
    app_ctx: AppContext,
    *,
    wallet_name: str,
    delay_blocks: int,
    allow_types: list[str],
    allow_key_ss58: Optional[str],
) -> None:
    name = pa.default_protected_name(wallet_name)
    controller_ss58 = _controller_ss58(app_ctx)

    # Resume: if a proxy-book entry already exists from a prior partial run, reuse it.
    from .. import config as cfg

    book = cfg.get_proxy(name)
    pure_ss58 = book.get("address") if book else None
    create_meta: dict[str, Any] = dict(book) if book else {}

    if not pure_ss58:
        result = app_ctx.submit(
            CreatePureProxy(proxy_type="Any", delay=delay_blocks, index=0),
            proxy_for="self",
            summary=f"create protected pure proxy (Any, delay {delay_blocks})",
        )
        if result is None or not result.success:
            raise typer.Exit(1)
        pure_ss58 = result.data.get("pure_proxy")
        if not pure_ss58:
            app_ctx.output.error(
                "could not read PureCreated address — re-run protect after "
                "`btcli proxy book add` with the spawned address"
            )
            raise typer.Exit(1)
        create_meta = {
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
        cfg.add_proxy(create_meta)
        app_ctx.output.message(f"created protected account {name} ({pure_ss58})")
    else:
        app_ctx.output.message(f"resuming protect: reusing proxy-book entry {name} ({pure_ss58})")

    record = {
        "controller_wallet": wallet_name,
        "controller_ss58": controller_ss58,
        "protected_ss58": pure_ss58,
        "name": name,
        "delay_blocks": delay_blocks,
        "allows": list(allow_types),
        "allow_key_ss58": allow_key_ss58,
        "spawner": create_meta.get("spawner") or controller_ss58,
        "account_mode": create_meta.get("account_mode", "legacy"),
        "height": create_meta.get("height"),
        "ext_index": create_meta.get("ext_index"),
        "index": create_meta.get("index", 0),
        "coldkey_swap": {"status": "pending"},
    }
    pa.upsert_protected(record)
    warn_defeating_proxies(app_ctx, record)

    # Queue zero-delay allow grants (announced via delayed Any controller).
    delegate = allow_key_ss58 or controller_ss58
    for proxy_type in allow_types:
        _announce_as_protected(
            app_ctx,
            record,
            AddProxy(delegate_ss58=delegate, proxy_type=proxy_type, delay=0),
            category=f"allow:{proxy_type}",
        )

    # Schedule coldkey swap: moves stake / subnets / identity onto the pure.
    swap_result = app_ctx.submit(
        AnnounceColdkeySwap(new_coldkey_ss58=pure_ss58),
        proxy_for="self",
        summary=f"announce coldkey swap {wallet_name} → {name}",
    )
    if swap_result is None or not swap_result.success:
        app_ctx.output.message(
            "protected account created, but coldkey-swap announce failed — "
            f"re-run `btcli wallet announce-coldkey-swap --new-coldkey {pure_ss58} "
            f"--wallet {wallet_name}` then execute when ready"
        )
        raise typer.Exit(1)

    async def _swap_status(client):
        announcement = await client.read("coldkey_swap_announcement", coldkey_ss58=controller_ss58)
        block = await client.block()
        return announcement, block

    announcement, current = app_ctx.run(_swap_status)
    execute_block = announcement["execute_block"] if announcement else None
    record["coldkey_swap"] = {
        "status": "announced",
        "execute_block": execute_block,
        "new_coldkey_ss58": pure_ss58,
    }
    pa.upsert_protected(record)

    if execute_block is not None:
        eta = _eta_for_block(int(execute_block), current)
        app_ctx.output.message(
            f"Coldkey swap announced → {name} ({pure_ss58})\n"
            f"Execute after block {execute_block} ({eta}) with:\n"
            f"  btcli wallet swap-coldkey --new-coldkey {pure_ss58} --wallet {wallet_name}\n"
            f"Check progress: btcli wallet swap-check --wallet {wallet_name}"
        )
    else:
        app_ctx.output.message(
            f"Coldkey swap announced → {name} ({pure_ss58}). "
            f"Run `btcli wallet swap-check --wallet {wallet_name}` for the execute block, "
            f"then `btcli wallet swap-coldkey --new-coldkey {pure_ss58} --wallet {wallet_name}`."
        )

    if allow_types:
        app_ctx.output.message(
            "Zero-delay allow proxies were queued as delayed announcements — "
            f"run `btcli proxy execute --wallet {wallet_name}` after the delay "
            "to activate them."
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
    controller_ss58 = existing.get("controller_ss58") or _controller_ss58(app_ctx)
    current_delay = int(existing.get("delay_blocks") or 0)
    current_allows = list(existing.get("allows") or [])
    delegate = allow_key_ss58 or existing.get("allow_key_ss58") or controller_ss58

    if delay_blocks != current_delay:
        # Rotate the delayed Any controller grant via announced batch.
        children = [
            RemoveProxy(
                delegate_ss58=controller_ss58, proxy_type="Any", delay=current_delay
            ).to_dict(),
            AddProxy(delegate_ss58=controller_ss58, proxy_type="Any", delay=delay_blocks).to_dict(),
        ]
        _announce_as_protected(
            app_ctx,
            existing,
            Batch(intents=children),
            category="delay-change",
        )
        existing = {**existing, "delay_blocks": delay_blocks}
        pa.upsert_protected(existing)

    for proxy_type in allow_types:
        if proxy_type in current_allows:
            continue
        _announce_as_protected(
            app_ctx,
            existing,
            AddProxy(delegate_ss58=delegate, proxy_type=proxy_type, delay=0),
            category=f"allow:{proxy_type}",
        )
        current_allows.append(proxy_type)

    for proxy_type in disallow_types:
        if proxy_type not in current_allows and proxy_type not in (existing.get("allows") or []):
            # Still try remove in case on-chain state differs from local.
            pass
        _announce_as_protected(
            app_ctx,
            existing,
            RemoveProxy(delegate_ss58=delegate, proxy_type=proxy_type, delay=0),
            category=f"disallow:{proxy_type}",
        )
        current_allows = [t for t in current_allows if t != proxy_type]

    existing = {
        **existing,
        "allows": current_allows,
        "allow_key_ss58": allow_key_ss58 or existing.get("allow_key_ss58"),
        "controller_ss58": controller_ss58,
    }
    pa.upsert_protected(existing)
    warn_defeating_proxies(app_ctx, existing)
    app_ctx.output.message(
        f"Protected account settings update queued for {existing.get('name')} "
        f"({existing['protected_ss58']}). "
        f"Run `btcli proxy execute --wallet {existing['controller_wallet']}` after the delay."
    )


def _announce_as_protected(
    app_ctx: AppContext, record: dict[str, Any], intent, *, category: str
) -> None:
    """Announce ``intent`` to run as the protected account (delayed Any path)."""
    from .prompt import confirm_wallet

    confirm_wallet(
        app_ctx,
        help_text="Wallet whose coldkey announces this protected-account call.",
        require_coldkey=True,
    )
    wallet = app_ctx.wallet()
    real = record["protected_ss58"]
    delay = int(record.get("delay_blocks") or 0)

    async def _prepare(client):
        _call, call_hash, _extras = await compose_announceable_call(
            client._substrate, intent, real, wallet
        )
        block = await client.block()
        return call_hash, block

    call_hash, block = app_ctx.run(_prepare)
    result = app_ctx.submit(
        AnnounceProxyCall(real_ss58=real, call_hash=call_hash),
        proxy_for="self",
        summary=f"announce {category}",
    )
    if result is None:
        return
    if not result.success:
        raise typer.Exit(1)
    announced_at = block
    if result.extrinsic_id:
        with contextlib.suppress(ValueError):
            announced_at = int(str(result.extrinsic_id).split("-")[0])
    encoded = intent.to_dict()
    inner_args = {k: v for k, v in encoded.items() if k != "op"}
    row = ann.add_announcement(
        controller_wallet=record["controller_wallet"],
        delegate_ss58=record.get("controller_ss58") or _controller_ss58(app_ctx),
        real_ss58=real,
        call_hash=call_hash,
        inner_op=intent.op,
        inner_args=inner_args,
        category=category,
        delay_blocks=delay,
        announced_at_block=announced_at,
        force_proxy_type="Any",
        summary=intent.summary(),
    )
    print_queued(app_ctx, record=record, row=row, current_block=announced_at)


def run_show(app_ctx: AppContext, *, index: Optional[int]) -> None:
    wallet_name = require_wallet_name(app_ctx)
    record = pa.get_by_controller(wallet_name)
    if record is None:
        app_ctx.output.error(
            f"wallet {wallet_name!r} does not control a local protected account",
            help="run `btcli proxy protect --wallet ...` first",
        )
        raise typer.Exit(1)

    current = app_ctx.run(lambda c: c.block())

    if index is not None:
        row = ann.get_by_index(wallet_name, index)
        if row is None:
            app_ctx.output.error(f"no local announcement #{index} for wallet {wallet_name!r}")
            raise typer.Exit(1)
        call_hash = row["call_hash"]

        async def _verify(client):
            from ..intents.registry import build as build_intent

            _call, recomputed, _ = await compose_announceable_call(
                client._substrate,
                build_intent(row["inner_op"], row["inner_args"]),
                row["real_ss58"],
                app_ctx.wallet(),
            )
            return recomputed

        try:
            recomputed = app_ctx.run(_verify)
            hash_match = recomputed.lower() == str(call_hash).lower()
        except Exception as error:
            recomputed = None
            hash_match = False
            app_ctx.output.message(f"[dim]could not recompute call hash: {error}[/dim]")

        executable_at = int(row["executable_at_block"])
        status = row.get("status", "pending")
        if status == "pending":
            status = "ready" if current >= executable_at else "waiting"
        detail = {
            **row,
            "status": status,
            "current_block": current,
            "countdown": _eta_for_block(executable_at, current),
            "hash_match": hash_match,
            "recomputed_call_hash": recomputed,
            "protected": {
                "name": record.get("name"),
                "ss58": record["protected_ss58"],
                "delay_blocks": record.get("delay_blocks"),
            },
        }
        app_ctx.output.detail(f"announcement #{index}", detail)
        return

    rows = ann.list_for_wallet(wallet_name)
    out = app_ctx.output
    if out.json_mode:
        payload = []
        for row in rows:
            executable_at = int(row["executable_at_block"])
            status = "ready" if current >= executable_at else "waiting"
            payload.append(
                {
                    **row,
                    "status": status,
                    "countdown": _eta_for_block(executable_at, current),
                }
            )
        out.value(
            {
                "protected": record,
                "current_block": current,
                "pending": payload,
            }
        )
        return

    name = record.get("name") or pa.default_protected_name(wallet_name)
    delay = record.get("delay_blocks")
    title = f"protected {name} ({record['protected_ss58']}) — delay {delay} blocks"
    table_rows = []
    for row in rows:
        executable_at = int(row["executable_at_block"])
        status = "ready" if current >= executable_at else "waiting"
        table_rows.append(
            [
                str(row["index"]),
                row.get("category") or row.get("inner_op"),
                str(executable_at),
                _eta_for_block(executable_at, current),
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
    record = pa.get_by_controller(wallet_name)
    if record is None:
        app_ctx.output.error(f"wallet {wallet_name!r} does not control a local protected account")
        raise typer.Exit(1)

    if index is not None:
        row = ann.get_by_index(wallet_name, index)
        if row is None:
            app_ctx.output.error(f"no local announcement #{index} for wallet {wallet_name!r}")
            raise typer.Exit(1)
        _cancel_one(app_ctx, row)
        return

    pending = ann.list_for_wallet(wallet_name)
    if not pending:
        app_ctx.output.message("no pending announcements to cancel")
        return
    if not yes:
        app_ctx.confirm(f"Cancel all {len(pending)} pending transactions?")
    for row in pending:
        _cancel_one(app_ctx, row)


def _cancel_one(app_ctx: AppContext, row: dict[str, Any]) -> None:
    result = app_ctx.submit(
        RemoveProxyAnnouncement(real_ss58=row["real_ss58"], call_hash=row["call_hash"]),
        proxy_for="self",
        summary=f"cancel announcement #{row['index']}",
    )
    if result is not None and result.success:
        ann.mark_status(row["controller_wallet"], int(row["index"]), "cancelled")
        app_ctx.output.message(f"cancelled #{row['index']}")


def run_execute_protected(app_ctx: AppContext, *, index: Optional[int]) -> None:
    wallet_name = require_wallet_name(app_ctx)
    record = pa.get_by_controller(wallet_name)
    if record is None:
        app_ctx.output.error(
            f"wallet {wallet_name!r} does not control a local protected account",
            help="for low-level announced execute without a protected record, "
            "pass --delegate / --real / --inner-op",
        )
        raise typer.Exit(1)

    current = app_ctx.run(lambda c: c.block())
    if index is not None:
        row = ann.get_by_index(wallet_name, index)
        if row is None:
            app_ctx.output.error(f"no local announcement #{index} for wallet {wallet_name!r}")
            raise typer.Exit(1)
        _execute_one(app_ctx, row, current)
        return

    pending = ann.list_for_wallet(wallet_name)
    ready = [r for r in pending if current >= int(r["executable_at_block"])]
    waiting = [r for r in pending if current < int(r["executable_at_block"])]
    for row in waiting:
        app_ctx.output.message(
            f"skipping #{row['index']} ({row.get('category')}): not ready — "
            f"{_eta_for_block(int(row['executable_at_block']), current)}"
        )
    if not ready:
        if not waiting:
            app_ctx.output.message("no pending announcements")
        return
    for row in ready:
        _execute_one(app_ctx, row, current)


def _execute_one(app_ctx: AppContext, row: dict[str, Any], current_block: int) -> None:
    executable_at = int(row["executable_at_block"])
    if current_block < executable_at:
        app_ctx.output.message(
            f"#{row['index']} not ready — {_eta_for_block(executable_at, current_block)}"
        )
        return
    result = app_ctx.submit(
        ExecuteProxyAnnounced(
            delegate_ss58=row["delegate_ss58"],
            real_ss58=row["real_ss58"],
            inner_op=row["inner_op"],
            inner_args=row["inner_args"],
            force_proxy_type=row.get("force_proxy_type") or "Any",
        ),
        proxy_for="self",
        summary=f"execute announcement #{row['index']}: {row.get('summary') or row['inner_op']}",
    )
    if result is not None and result.success:
        ann.mark_status(row["controller_wallet"], int(row["index"]), "executed")
        app_ctx.output.message(f"executed #{row['index']}")
        if row.get("category") == "rotate-controller":
            record = pa.get_by_controller(row["controller_wallet"])
            pending = (record or {}).get("pending_rotate") or {}
            to_name = pending.get("to_wallet")
            if record is not None and to_name:
                moved = pa.move_protected_controller(row["controller_wallet"], to_name)
                if moved is not None:
                    moved.pop("pending_rotate", None)
                    pa.upsert_protected(moved)
                    ann.move_wallet_announcements(row["controller_wallet"], to_name)
                    app_ctx.output.message(
                        f"local protected-account record moved: "
                        f"{row['controller_wallet']} → {to_name}"
                    )


def run_rotate(app_ctx: AppContext, *, to_wallet: str) -> None:
    wallet_name = require_wallet_name(app_ctx)
    record = pa.get_by_controller(wallet_name)
    if record is None:
        app_ctx.output.error(f"wallet {wallet_name!r} does not control a local protected account")
        raise typer.Exit(1)

    new_ss58 = app_ctx.resolve_address("coldkey_ss58", to_wallet)
    # Resolve the destination wallet name even when the user passed an ss58.
    to_name = to_wallet
    from .. import config as cfg

    if cfg.get_multisig(to_wallet) is None:
        # Prefer a local wallet directory name when the arg is a name.
        to_name = to_wallet

    delay = int(record.get("delay_blocks") or 0)
    controller_ss58 = record.get("controller_ss58") or _controller_ss58(app_ctx)
    children = [
        AddProxy(delegate_ss58=new_ss58, proxy_type="Any", delay=delay).to_dict(),
        RemoveProxy(delegate_ss58=controller_ss58, proxy_type="Any", delay=delay).to_dict(),
    ]
    _announce_as_protected(
        app_ctx,
        record,
        Batch(intents=children),
        category="rotate-controller",
    )
    # Stash the intended rebind; applied after the user executes the announcement.
    record = {
        **record,
        "pending_rotate": {"to_wallet": to_name, "to_ss58": new_ss58},
    }
    pa.upsert_protected(record)
    app_ctx.output.message(
        f"Controller rotate queued: add {to_name} ({new_ss58}), remove {wallet_name}.\n"
        f"After `btcli proxy execute --wallet {wallet_name}`, rebind local config with:\n"
        f"  btcli proxy rotate --wallet {wallet_name} --to {to_name} --finalize\n"
        "Or re-run rotate with --finalize once the announcement has executed."
    )


def run_rotate_finalize(app_ctx: AppContext, *, to_wallet: str) -> None:
    wallet_name = require_wallet_name(app_ctx)
    record = pa.get_by_controller(wallet_name)
    if record is None:
        app_ctx.output.error(f"wallet {wallet_name!r} does not control a local protected account")
        raise typer.Exit(1)
    pending = record.get("pending_rotate") or {}
    to_name = pending.get("to_wallet") or to_wallet
    moved = pa.move_protected_controller(wallet_name, to_name)
    if moved is None:
        raise typer.Exit(1)
    moved.pop("pending_rotate", None)
    pa.upsert_protected(moved)
    ann.move_wallet_announcements(wallet_name, to_name)
    app_ctx.output.message(f"local protected-account record moved: {wallet_name} → {to_name}")
