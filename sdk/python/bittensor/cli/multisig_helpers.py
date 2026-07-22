"""Shared multisig helpers for call submission and pending-op inspection."""

from __future__ import annotations

import asyncio
import json
import shlex
from hashlib import blake2b
from typing import Any, Optional

from .. import config as cfg
from .. import wallets
from .._generated import storage as st
from ..result import ChainError, ExtrinsicResult
from ..wallets import is_bittensor_address


def hex_bytes(value: bytes | str) -> str:
    if isinstance(value, bytes):
        return "0x" + value.hex()
    if hasattr(value, "hex") and not isinstance(value, str):
        return "0x" + bytes(value).hex()
    text = str(value)
    return text if text.startswith("0x") else "0x" + text


def _soft_resolve_coldkey(app_ctx, ref: str) -> Optional[str]:
    """Resolve a coldkey ref without exiting the CLI."""
    if is_bittensor_address(ref):
        return ref
    booked = cfg.get_address(ref)
    if booked:
        return booked
    try:
        return wallets.open_wallet(name=ref, path=app_ctx.wallet_path).coldkeypub.ss58_address
    except Exception:
        return None


def _address_book_name(ss58: str) -> Optional[str]:
    for entry in cfg.load_addresses():
        if str(entry.get("address")) == ss58:
            name = entry.get("name")
            return str(name) if name else None
    return None


def _wallet_name_for_ss58(app_ctx, ss58: str) -> Optional[str]:
    try:
        for coldkey in wallets.list_wallets_detailed(app_ctx.wallet_path):
            if coldkey.ss58 == ss58:
                return coldkey.name
    except Exception:
        return None


def _portable_signatory_ref(ref: str) -> bool:
    """True when ``ref`` is an explicit alias/wallet name, not a bare ss58 address."""
    return not is_bittensor_address(ref)


def resolve_signatory_name(
    app_ctx,
    ss58: str,
    *,
    preset_ref: Optional[str] = None,
) -> str:
    """Best short name for an ss58: preset alias, local wallet, address book, then ss58."""
    if preset_ref and _portable_signatory_ref(preset_ref):
        return preset_ref
    wallet = _wallet_name_for_ss58(app_ctx, ss58)
    if wallet:
        return wallet
    booked = _address_book_name(ss58)
    if booked:
        return booked
    return ss58


def format_signatory_display(ss58: str, name: Optional[str] = None) -> str:
    """Human-readable signer label, e.g. ``vune (5Fev...)``."""
    label = name or ss58
    if label != ss58:
        return f"{label} ({ss58})"
    return ss58


def format_multisig_display(address: Optional[str], preset: Optional[str] = None) -> Optional[str]:
    """Human-readable multisig label, e.g. ``finney-trium (5DcS...)``."""
    if preset and address:
        return f"{preset} ({address})"
    return address or preset


def signatory_labels(
    app_ctx,
    refs: list[str],
    *,
    signatories: Optional[list[str]] = None,
) -> dict[str, str]:
    """Map ss58 addresses to the best local short name for each signatory."""
    preset_by_addr: dict[str, str] = {}
    for ref in refs:
        address = _soft_resolve_coldkey(app_ctx, ref)
        if address:
            preset_by_addr[address] = ref
    targets = signatories if signatories is not None else list(preset_by_addr.keys())
    return {
        ss58: resolve_signatory_name(app_ctx, ss58, preset_ref=preset_by_addr.get(ss58))
        for ss58 in targets
    }


def _replay_wallet_label(addr_to_ref: dict[str, str], ss58: str) -> str:
    ref = addr_to_ref.get(ss58, ss58)
    return ref if _portable_signatory_ref(ref) else ss58


def _replay_other_signatory_refs(
    signatories: list[str],
    addr_to_ref: dict[str, str],
    co_signer_ss58: str,
) -> Optional[list[str]]:
    """Other signers for ``--other-signatories`` when we know explicit non-ss58 refs."""
    others = [addr_to_ref[addr] for addr in signatories if addr != co_signer_ss58]
    if not others:
        return None
    if not all(_portable_signatory_ref(ref) for ref in others):
        return None
    return others


def build_replay_command(
    app_ctx,
    *,
    target: str,
    params: dict,
    args_file: Optional[str],
    sudo: bool,
    threshold: int,
    signatories: list[str],
    wallet_label: str,
    signer_role: str = "coldkey",
    preset: Optional[str] = None,
    other_signatory_labels: Optional[list[str]] = None,
    proxy_for: Optional[str] = None,
    force_proxy_type: Optional[str] = None,
) -> str:
    """Build a copy-paste ``btcli call`` command for a co-signer."""
    parts = ["btcli"]
    if app_ctx.network != "finney":
        parts.append(f"-n {shlex.quote(app_ctx.network)}")
    parts.append(f"call {target}")
    if args_file:
        parts.append(f"--args-file {shlex.quote(args_file)}")
    elif params:
        parts.append(f"--args {shlex.quote(json.dumps(params, separators=(',', ':')))}")
    if sudo:
        parts.append("--sudo")
    if proxy_for:
        # The raw ss58, not a book name: co-signers approve by call hash, so
        # their command must rebuild the byte-identical Proxy.proxy wrapper
        # even without this machine's proxy book.
        parts.append(f"--proxy-for {shlex.quote(proxy_for)}")
        if force_proxy_type:
            parts.append(f"--force-proxy-type {shlex.quote(force_proxy_type)}")
    if preset:
        parts.append(f"--multisig {shlex.quote(preset)}")
    elif other_signatory_labels:
        parts.append(f"--multisig-threshold {threshold}")
        parts.append(f"--other-signatories {shlex.quote(','.join(other_signatory_labels))}")
    else:
        parts.append(f"--multisig-threshold {threshold}")
        parts.append(f"--signatories {shlex.quote(','.join(signatories))}")
    parts.append(f"-w {shlex.quote(wallet_label)}")
    if signer_role != "coldkey":
        parts.append(f"--signer {signer_role}")
    return " ".join(parts)


def _resolve_stored_signatories(app_ctx, refs: list[str]) -> list[str]:
    """Resolve a saved multisig signer list (ss58, book names, or wallets)."""
    resolved: list[str] = []
    for ref in refs:
        address = app_ctx.resolve_address("coldkey_ss58", ref)
        if not address:
            raise ValueError(f"cannot resolve signatory {ref!r} in multisig preset")
        resolved.append(address)
    return list(dict.fromkeys(resolved))


def resolve_multisig(
    app_ctx,
    *,
    multisig_name: Optional[str] = None,
    threshold: Optional[int] = None,
    signatories: Optional[str] = None,
    other_signatories: Optional[str] = None,
    signer: str = "coldkey",
    wallet_default: Optional[str] = None,
) -> tuple[Optional[int], list[str], Optional[str], list[str]]:
    """Resolve multisig settings from a preset name, inline flags, or -w wallet name."""
    inline = threshold is not None or signatories or other_signatories
    if not multisig_name and not inline and wallet_default and cfg.get_multisig(wallet_default):
        multisig_name = wallet_default
    if multisig_name and inline:
        raise ValueError("use either --multisig NAME or inline multisig flags, not both")
    if multisig_name:
        entry = cfg.get_multisig(multisig_name)
        if entry is None:
            raise ValueError(
                f"unknown multisig {multisig_name!r}; run `btcli multisig add NAME` "
                "or `btcli multisig list`"
            )
        refs = list(entry["signatories"])
        return (
            int(entry["threshold"]),
            _resolve_stored_signatories(app_ctx, refs),
            multisig_name,
            refs,
        )
    if threshold is None:
        return None, [], None, []
    if threshold < 1:
        raise ValueError("threshold must be >= 1")
    if signatories and other_signatories:
        raise ValueError("pass either --signatories or --other-signatories, not both")
    if signatories:
        refs = [part.strip() for part in signatories.split(",") if part.strip()]
        sigs = app_ctx.resolve_signatory_list(signatories)
    elif other_signatories:
        refs = [part.strip() for part in other_signatories.split(",") if part.strip()]
        sigs = app_ctx.resolve_signatory_list(other_signatories)
        wallet = app_ctx.wallet()
        self_addr = (
            wallet.coldkeypub.ss58_address if signer == "coldkey" else wallet.hotkey.ss58_address
        )
        sigs = list(dict.fromkeys([*sigs, self_addr]))
        refs = [*refs, app_ctx.wallet_name]
    else:
        raise ValueError("with --multisig-threshold, pass --signatories or --other-signatories")
    return threshold, sigs, None, refs


def resolve_multisig_preset(app_ctx, name: str) -> tuple[int, list[str], list[str]]:
    """Return threshold, resolved ss58 signatories, and preset refs."""
    threshold, signatories, _, refs = resolve_multisig(app_ctx, multisig_name=name)
    if threshold is None:
        raise ValueError(f"unknown multisig {name!r}")
    return threshold, signatories, refs


async def multisig_list_records(
    client,
    app_ctx,
    entries: Optional[list[dict[str, Any]]] = None,
) -> list[dict[str, Any]]:
    """Build wallet-list rows for saved multisig wallets."""
    rows: list[dict[str, Any]] = []
    for entry in entries if entries is not None else cfg.load_multisigs():
        name = entry.get("name")
        if not name:
            continue
        threshold = int(entry.get("threshold") or 0)
        refs = list(entry.get("signatories") or [])
        signatories = [
            address for ref in refs if (address := _soft_resolve_coldkey(app_ctx, ref)) is not None
        ]
        signatories = list(dict.fromkeys(signatories))
        multisig_address = None
        if signatories and threshold >= 1:
            try:
                ms = await client.multisig(signatories, threshold)
                multisig_address = ms.address
            except Exception:
                pass
        signatory_rows = [
            {"name": ref, "ss58": _soft_resolve_coldkey(app_ctx, ref)} for ref in refs
        ]
        rows.append(
            {
                "name": name,
                "kind": "multisig",
                "threshold": threshold,
                "signatory_count": len(refs),
                "ss58": multisig_address,
                "signatories": signatory_rows,
                "note": entry.get("note", ""),
            }
        )
    rows.sort(key=lambda row: str(row["name"]).lower())
    return rows


def _json_friendly(value: Any) -> Any:
    if hasattr(value, "value"):
        value = value.value
    if isinstance(value, bytes):
        return "0x" + value.hex()
    if isinstance(value, dict):
        return {str(k): _json_friendly(v) for k, v in value.items()}
    if isinstance(value, (list, tuple)):
        return [_json_friendly(v) for v in value]
    return value


def _multiaddress_ss58(value: Any) -> Optional[str]:
    """The ss58 inside a decoded MultiAddress (a plain string or {"Id": ss58})."""
    if isinstance(value, str):
        return value
    if isinstance(value, dict) and len(value) == 1:
        inner = next(iter(value.values()))
        if isinstance(inner, str):
            return inner
    return None


def _proxy_type_name(value: Any) -> Optional[str]:
    """The variant name of a decoded Option<ProxyType> (string or {name: None})."""
    if isinstance(value, str):
        return value
    if isinstance(value, dict) and len(value) == 1:
        return str(next(iter(value.keys())))
    return None


def _call_spec_from_decoded(raw_call: Any) -> Optional[dict[str, Any]]:
    value = raw_call.value if hasattr(raw_call, "value") else raw_call
    if not isinstance(value, dict):
        return None
    module = value.get("call_module")
    function = value.get("call_function")
    args = {
        str(arg.get("name")): _json_friendly(arg.get("value"))
        for arg in (value.get("call_args") or [])
        if arg.get("name") is not None
    }
    if module == "Proxy" and function == "proxy":
        # Proxied dispatch: recurse into the inner call (which may itself be
        # Sudo.sudo) and record the real account so co-signer commands can
        # rebuild the identical wrapper.
        inner = args.pop("call", None)
        real = _multiaddress_ss58(args.get("real"))
        if inner is None or real is None:
            return None
        inner_spec = _call_spec_from_decoded(inner)
        if inner_spec is None:
            return None
        inner_spec["proxy_for"] = real
        forced = _proxy_type_name(args.get("force_proxy_type"))
        if forced is not None:
            inner_spec["force_proxy_type"] = forced
        return inner_spec
    if module == "Sudo" and function == "sudo":
        inner = args.pop("call", None)
        if inner is None:
            return None
        inner_spec = _call_spec_from_decoded(inner)
        if inner_spec is None:
            return None
        inner_spec["sudo"] = True
        return inner_spec
    if not module or not function:
        return None
    return {
        "target": f"{module}.{function}",
        "params": args,
        "sudo": False,
    }


async def decode_call_data(client, call_data: str) -> Optional[dict[str, Any]]:
    """Decode scale-encoded call bytes into target, params, and sudo flag."""
    try:
        raw = await client.decode_scale("Call", call_data)
    except Exception:
        return None
    spec = _call_spec_from_decoded(raw)
    if spec is None:
        return None
    spec["call_data"] = hex_bytes(call_data)
    return spec


async def resolve_call_spec(
    client,
    *,
    call_hash: str,
    call_data: Optional[str] = None,
    timepoint: Optional[dict[str, int]] = None,
) -> Optional[dict[str, Any]]:
    """Resolve call target/params from cache, call_data hex, or opening extrinsic."""
    cached = cfg.get_multisig_cache(hex_bytes(call_hash))
    if cached:
        return cached
    if call_data:
        data_hex = hex_bytes(call_data)
        computed = "0x" + blake2b(bytes.fromhex(data_hex[2:]), digest_size=32).hexdigest()
        if computed.lower() != hex_bytes(call_hash).lower():
            raise ValueError(
                f"--call-data does not match call hash {hex_bytes(call_hash)} "
                f"(blake2_256 of the data is {computed})"
            )
        spec = await decode_call_data(client, call_data)
        if spec:
            spec["call_hash"] = hex_bytes(call_hash)
        return spec
    if timepoint:
        spec = await fetch_call_from_timepoint(
            client,
            height=int(timepoint["height"]),
            index=int(timepoint["index"]),
        )
        if spec is None:
            return None
        if spec.get("call_hash") and hex_bytes(spec["call_hash"]) != hex_bytes(call_hash):
            return None
        spec["call_hash"] = hex_bytes(call_hash)
        return spec
    return None


async def fetch_call_from_timepoint(
    client,
    *,
    height: int,
    index: int,
) -> Optional[dict[str, Any]]:
    """Recover a multisig inner call from the opening ``as_multi`` extrinsic."""
    try:
        info = await client.block_info(height)
    except ChainError:
        # Pruned or unreachable block (block_info already retried the archive
        # pool); the call spec is simply unrecoverable from this timepoint.
        return None
    if info is None:
        return None
    if index < 0 or index >= len(info.extrinsics):
        return None
    value = info.extrinsics[index]
    if not isinstance(value, dict):
        return None  # the opening extrinsic failed to decode
    outer = value.get("call") or {}
    if outer.get("call_module") != "Multisig":
        return None
    args = {arg.get("name"): arg.get("value") for arg in (outer.get("call_args") or [])}
    if outer.get("call_function") == "as_multi":
        inner = args.get("call")
        if inner is None:
            return None
        spec = _call_spec_from_decoded(inner)
        if spec is None:
            return None
        spec["call_data"] = hex_bytes(getattr(inner, "data", b""))
        if hasattr(inner, "call_hash"):
            spec["call_hash"] = hex_bytes(inner.call_hash)
        return spec
    if outer.get("call_function") == "approve_as_multi":
        return None
    return None


async def list_pending_multisig_ops(client, multisig_address: str) -> list[dict[str, Any]]:
    """All open multisig operations for ``multisig_address``."""
    ops: list[dict[str, Any]] = []
    for key, value in await client.query_map(st.Multisig.Multisigs):
        account = str(key[0]) if isinstance(key, (list, tuple)) else str(key)
        if account != multisig_address:
            continue
        call_hash = key[1] if isinstance(key, (list, tuple)) and len(key) > 1 else None
        if value is None:
            continue
        when = value.get("when") or {}
        approvals = [str(a) for a in value.get("approvals") or []]
        ops.append(
            {
                "call_hash": hex_bytes(call_hash),
                "timepoint": {
                    "height": int(when.get("height", 0)),
                    "index": int(when.get("index", 0)),
                },
                "timepoint_display": f"{int(when.get('height', 0))}:{int(when.get('index', 0))}",
                "depositor": str(value.get("depositor")),
                "deposit_rao": int(value.get("deposit") or 0),
                "approvals": approvals,
            }
        )
    return sorted(ops, key=lambda op: op["timepoint"]["height"], reverse=True)


def _event_payload(entry: Any) -> dict[str, Any]:
    record = entry.value if hasattr(entry, "value") else entry
    if not isinstance(record, dict):
        return {}
    event = record.get("event", record)
    return event if isinstance(event, dict) else {}


def _multisig_event_ids(events: list) -> set[str]:
    ids: set[str] = set()
    for entry in events or []:
        event = _event_payload(entry)
        if event.get("module_id") != "Multisig":
            continue
        event_id = event.get("event_id")
        if event_id:
            ids.add(str(event_id))
    return ids


def _multisig_executed(events: list) -> bool:
    return "MultisigExecuted" in _multisig_event_ids(events)


async def _query_pending_multisig(
    client,
    multisig_address: str,
    call_hash: str,
    *,
    raw_call_hash: Any = None,
    attempts: int = 6,
    delay_seconds: float = 0.5,
) -> Optional[dict[str, Any]]:
    """Read pending multisig state, retrying briefly after inclusion."""
    key_candidates: list[Any] = []
    if raw_call_hash is not None:
        key_candidates.append(raw_call_hash)
    key_candidates.append(hex_bytes(call_hash))

    for attempt in range(attempts):
        for key in key_candidates:
            pending = await client.query(st.Multisig.Multisigs, [multisig_address, key])
            if pending:
                return pending
        if attempt + 1 < attempts:
            await asyncio.sleep(delay_seconds)
    return None


async def build_pending_followup(
    client,
    app_ctx,
    *,
    ms,
    threshold: int,
    signatories: list[str],
    signatory_refs: list[str],
    call_hash: str,
    pending: dict[str, Any],
    preset: Optional[str] = None,
    call_spec: Optional[dict[str, Any]] = None,
    signer_role: str = "coldkey",
) -> dict[str, Any]:
    """Build a pending multisig record with optional co-signer commands."""
    labels = signatory_labels(app_ctx, signatory_refs, signatories=signatories)
    approvals = list(pending.get("approvals") or [])
    remaining = [s for s in signatories if s not in approvals]
    call_data = None
    target = None
    params: dict[str, Any] = {}
    args_file = None
    sudo = False
    proxy_for = None
    force_proxy_type = None
    if call_spec:
        target = call_spec.get("target")
        params = call_spec.get("params") or {}
        args_file = call_spec.get("args_file")
        sudo = bool(call_spec.get("sudo"))
        proxy_for = call_spec.get("proxy_for")
        force_proxy_type = call_spec.get("force_proxy_type")
        call_data = call_spec.get("call_data")

    co_signer_commands = []
    if target:
        addr_to_ref = {addr: labels.get(addr, addr) for addr in signatories}
        for ss58 in remaining:
            name = labels.get(ss58, ss58)
            label = format_signatory_display(ss58, name)
            wallet_label = _replay_wallet_label(addr_to_ref, ss58)
            other_labels = _replay_other_signatory_refs(signatories, addr_to_ref, ss58)
            if other_labels is not None and not _portable_signatory_ref(wallet_label):
                other_labels = None
            co_signer_commands.append(
                {
                    "ss58": ss58,
                    "label": label,
                    "name": name,
                    "command": build_replay_command(
                        app_ctx,
                        target=target,
                        params=params,
                        args_file=args_file,
                        sudo=sudo,
                        threshold=threshold,
                        signatories=signatories,
                        wallet_label=wallet_label,
                        signer_role=signer_role,
                        preset=preset,
                        other_signatory_labels=other_labels,
                        proxy_for=proxy_for,
                        force_proxy_type=force_proxy_type,
                    ),
                }
            )

    return {
        "status": "pending",
        "target": target,
        "sudo": sudo,
        "proxy_for": proxy_for,
        "force_proxy_type": force_proxy_type,
        "params": params,
        "approvals": len(approvals),
        "threshold": threshold,
        "call_hash": hex_bytes(call_hash),
        "call_data": call_data,
        "timepoint": pending.get("timepoint"),
        "timepoint_display": pending.get("timepoint_display"),
        "multisig_address": ms.address,
        "multisig_preset": preset,
        "depositor": pending.get("depositor"),
        "depositor_label": (
            format_signatory_display(
                str(pending.get("depositor")),
                labels.get(str(pending.get("depositor"))),
            )
            if pending.get("depositor")
            else None
        ),
        "approvals_so_far": approvals,
        "approval_labels": [format_signatory_display(a, labels.get(a)) for a in approvals],
        "remaining_signatories": remaining,
        "remaining_labels": [format_signatory_display(a, labels.get(a)) for a in remaining],
        "co_signer_commands": co_signer_commands,
        "commands_available": bool(co_signer_commands),
        # Rustc-style split: `note:` states the situation, `hint:` the fix.
        "decode_note": (
            None
            if co_signer_commands
            else (
                "call details unknown — the opening block may be pruned on this RPC node, "
                "the op may have used approve_as_multi (hash only), or local cache may be missing"
            )
        ),
        "decode_hint": (
            None
            if co_signer_commands
            else "pass `--call-data 0x..` once, or re-open the op with a current SDK build"
        ),
    }


async def multisig_followup_from_composed(
    client,
    app_ctx,
    *,
    call_hash: str,
    call_data: str,
    raw_call_hash: Any = None,
    ms,
    signatories: list[str],
    threshold: int,
    signatory_refs: list[str],
    target: Optional[str],
    params: dict,
    args_file: Optional[str],
    sudo: bool,
    preset: Optional[str],
    signer_role: str,
    result: ExtrinsicResult,
    proxy_for: Optional[str] = None,
    force_proxy_type: Optional[str] = None,
) -> dict[str, Any]:
    """Build co-signer instructions after submitting a multisig approval."""
    call_hash = hex_bytes(call_hash)
    call_data = hex_bytes(call_data)
    call_spec = {
        "target": target,
        "params": params,
        "args_file": args_file,
        "sudo": sudo,
        "proxy_for": proxy_for,
        "force_proxy_type": force_proxy_type,
        "call_data": call_data,
        "call_hash": call_hash,
        "threshold": threshold,
        "signatories": signatories,
        "multisig_address": ms.address,
        "multisig_preset": preset,
        "network": app_ctx.network,
    }
    if target:
        # A spec without a target can't produce co-signer commands; caching it
        # would shadow a later, successful decode (cache hits return early).
        cfg.save_multisig_cache_entry(call_hash, call_spec)

    if _multisig_executed(result.events):
        return {
            "status": "executed",
            "target": target,
            "sudo": sudo,
            "proxy_for": proxy_for,
            "call_hash": call_hash,
            "call_data": call_data,
            "multisig_address": ms.address,
            "multisig_preset": preset,
        }

    pending = await _query_pending_multisig(
        client,
        ms.address,
        call_hash,
        raw_call_hash=raw_call_hash,
    )
    if not pending:
        for row in await list_pending_multisig_ops(client, ms.address):
            if row["call_hash"] == call_hash:
                pending = {
                    "when": row["timepoint"],
                    "approvals": row["approvals"],
                    "depositor": row["depositor"],
                }
                break

    if not pending:
        if _multisig_event_ids(result.events):
            pending_row = {
                "approvals": [],
                "depositor": None,
                "timepoint": {"height": 0, "index": 0},
                "timepoint_display": "?",
            }
            followup = await build_pending_followup(
                client,
                app_ctx,
                ms=ms,
                threshold=threshold,
                signatories=signatories,
                signatory_refs=signatory_refs,
                call_hash=call_hash,
                pending=pending_row,
                preset=preset,
                call_spec=call_spec,
                signer_role=signer_role,
            )
            followup["decode_note"] = (
                "approval recorded on-chain but pending state is not visible yet"
            )
            followup["decode_hint"] = "run `btcli multisig pending` to inspect co-signer commands"
            return followup
        return {
            "status": "submitted",
            "target": target,
            "sudo": sudo,
            "proxy_for": proxy_for,
            "call_hash": call_hash,
            "call_data": call_data,
            "multisig_address": ms.address,
            "multisig_preset": preset,
            "decode_note": "approval submitted but pending state is not visible yet",
            "decode_hint": "run `btcli multisig pending` shortly to inspect co-signer commands",
        }

    when = pending.get("when") or {}
    pending_row = {
        "approvals": [str(a) for a in pending.get("approvals") or []],
        "depositor": str(pending.get("depositor")),
        "timepoint": {
            "height": int(when.get("height", 0)),
            "index": int(when.get("index", 0)),
        },
        "timepoint_display": f"{int(when.get('height', 0))}:{int(when.get('index', 0))}",
    }
    return await build_pending_followup(
        client,
        app_ctx,
        ms=ms,
        threshold=threshold,
        signatories=signatories,
        signatory_refs=signatory_refs,
        call_hash=call_hash,
        pending=pending_row,
        preset=preset,
        call_spec=call_spec,
        signer_role=signer_role,
    )


def _match_multisig_book(
    app_ctx, signatories: list[str], threshold: int
) -> tuple[Optional[str], Optional[list[str]]]:
    """The saved multisig entry (name, refs) matching a signer set, if any."""
    wanted = set(signatories)
    for entry in cfg.load_multisigs():
        if int(entry.get("threshold") or 0) != threshold:
            continue
        refs = list(entry.get("signatories") or [])
        resolved = {
            address for ref in refs if (address := _soft_resolve_coldkey(app_ctx, ref)) is not None
        }
        if resolved == wanted:
            return str(entry.get("name")), refs
    return None, None


async def multisig_followup_for_intent(
    client,
    app_ctx,
    *,
    intent,
    result: ExtrinsicResult,
    signer_address: str,
) -> Optional[dict[str, Any]]:
    """Co-signer followup after a `tx multisig-approve` / `tx multisig-execute`.

    The intent's build surfaced the inner call's hash and SCALE bytes into
    ``result.data``; from those the call spec is decoded, cached locally, and
    turned into the same pending record (timepoint, approvals, ready-to-run
    co-signer commands) the `call --multisig` path produces.
    """
    call_hash = result.data.get("multisig_call_hash")
    call_data = result.data.get("multisig_call_data")
    threshold = int(getattr(intent, "threshold", 1) or 1)
    if not call_hash or not call_data or threshold < 2:
        return None
    signatories = list(dict.fromkeys([*intent.other_signatories, signer_address]))
    ms = await client.multisig(signatories, threshold)
    preset, refs = _match_multisig_book(app_ctx, signatories, threshold)
    spec = await decode_call_data(client, call_data)
    return await multisig_followup_from_composed(
        client,
        app_ctx,
        call_hash=call_hash,
        call_data=call_data,
        ms=ms,
        signatories=signatories,
        threshold=threshold,
        signatory_refs=refs if refs is not None else signatories,
        target=(spec or {}).get("target"),
        params=(spec or {}).get("params") or {},
        args_file=None,
        sudo=bool((spec or {}).get("sudo")),
        preset=preset,
        signer_role=intent.signer,
        result=result,
        proxy_for=(spec or {}).get("proxy_for"),
        force_proxy_type=(spec or {}).get("force_proxy_type"),
    )


async def list_pending_with_commands(
    client,
    app_ctx,
    *,
    ms,
    threshold: int,
    signatories: list[str],
    signatory_refs: list[str],
    preset: Optional[str] = None,
    call_hash_filter: Optional[str] = None,
    call_data: Optional[str] = None,
) -> list[dict[str, Any]]:
    """List pending multisig ops with co-signer commands when call details are known."""
    rows = await list_pending_multisig_ops(client, ms.address)
    if call_hash_filter:
        wanted = hex_bytes(call_hash_filter)
        rows = [row for row in rows if row["call_hash"] == wanted]

    results: list[dict[str, Any]] = []
    for row in rows:
        data_override = None
        if call_data and call_hash_filter and row["call_hash"] == hex_bytes(call_hash_filter):
            data_override = call_data
        spec = await resolve_call_spec(
            client,
            call_hash=row["call_hash"],
            call_data=data_override,
            timepoint=row.get("timepoint"),
        )
        if spec and data_override:
            cfg.save_multisig_cache_entry(
                row["call_hash"],
                {
                    **spec,
                    "call_hash": row["call_hash"],
                    "threshold": threshold,
                    "signatories": signatories,
                    "multisig_address": ms.address,
                    "multisig_preset": preset,
                    "network": app_ctx.network,
                },
            )
        results.append(
            await build_pending_followup(
                client,
                app_ctx,
                ms=ms,
                threshold=threshold,
                signatories=signatories,
                signatory_refs=signatory_refs,
                call_hash=row["call_hash"],
                pending=row,
                preset=preset,
                call_spec=spec,
            )
        )
    return results
