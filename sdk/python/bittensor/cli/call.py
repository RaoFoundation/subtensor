"""The `btcli call` command — the raw-call escape hatch, as a CLI command.

Every generated call builder (``bittensor.calls``) is reachable here by its
``Pallet.function`` name, so any extrinsic the chain exposes — including ones no
intent wraps, like ``Sudo.sudo`` or ``System.set_code`` — can be submitted from
the command line. This is the CLI projection of ``client.submit_call``: ``tx`` is
for the safe, previewable intents; ``call`` is the deliberate escape hatch.

A runtime upgrade is the canonical example (the sudo key signs)::

    btcli call System.set_code --args-file runtime.json --sudo --yes

where ``runtime.json`` is ``{"code": "0x<compact-wasm-hex>"}``. ``--sudo`` wraps
the call in ``Sudo.sudo(...)``; drop it to submit the call directly.

When finney's sudo key is a multisig, save contacts once::

    btcli addr triumph-a 5OtherA...
    btcli addr triumph-b 5OtherB...
    btcli multisig add finney-sudo --threshold 2 \\
      --signatories suro,triumph-a,triumph-b

Then each signatory approves with the short form::

    btcli call System.set_code --args-file runtime.json --sudo \\
      --multisig finney-sudo -w suro --yes

After the first approval, the CLI prints ``call_hash``, ``call_data``, ``timepoint``,
and copy-paste commands for each remaining co-signer.

If you only know the *other* signers' addresses, pass them and your wallet is
added automatically::

    btcli call System.set_code --args-file runtime.json --sudo \\
      --multisig-threshold 2 --other-signatories 5OtherA...,5OtherB... \\
      -w suro --yes

``--proxy-for`` wraps the call in ``Proxy.proxy`` so it dispatches as another
account (e.g. a pure proxy) whose registered delegate is the signing key — or
the multisig, combining both wrappers. Each signatory approves the same
``Proxy.proxy`` call::

    btcli call SubtensorModule.set_sn_owner_hotkey --args '{...}' \\
      --proxy-for sn7-owner --multisig sn7-team -w suro --yes
"""

from __future__ import annotations

import contextlib
import json
from typing import Any, Optional

import typer

from .. import calls
from ..executor import nest_origin_wrappers
from . import multisig_helpers as ms_helpers
from .call_names import resolve_builder_params
from .context import ctx_of
from .globals import with_tx_globals
from .prompt import replay_command

_MAX_SHOWN = 80  # truncate long param values (e.g. a wasm blob) in dry-run output


def _shield_policy_for_target(target: str) -> tuple[bool, bool]:
    """``(default, required)`` from the intent that wraps this Pallet.function.

    Stake add/remove/move/transfer (and other pool-trading wraps) default
    shielded, same as ``btcli stake add``. Imports the intents package so
    the registry is fully populated.
    """
    pallet, sep, function = target.partition(".")
    if not sep:
        return False, False
    from ..intents import REGISTRY

    pair = (pallet, function)
    default = False
    for cls in REGISTRY.values():
        if pair not in cls.wraps:
            continue
        if cls.mev_shield_required:
            return True, True
        if cls.mev_shield_default:
            default = True
    return default, False


def _resolve_builder(target: str):
    """Resolve a ``Pallet.function`` name to its generated call builder."""
    pallet_name, sep, function = target.partition(".")
    if not sep or not function:
        raise typer.BadParameter(f"expected Pallet.function, got {target!r}", param_hint="TARGET")
    pallet = getattr(calls, pallet_name, None)
    if pallet is None or not isinstance(pallet, type):
        raise typer.BadParameter(f"unknown pallet {pallet_name!r}", param_hint="TARGET")
    builder = getattr(pallet, function, None)
    if not callable(builder):
        raise typer.BadParameter(
            f"unknown call {function!r} in pallet {pallet_name!r}", param_hint="TARGET"
        )
    return builder


def _load_params(args: Optional[str], args_file: Optional[str]) -> dict:
    """Load call parameters from a JSON string or file into a dict."""
    if args and args_file:
        raise typer.BadParameter("pass either --args or --args-file, not both")
    raw = None
    if args_file:
        try:
            with open(args_file) as handle:
                raw = handle.read()
        except OSError as error:
            raise typer.BadParameter(
                f"cannot read {args_file!r}: {error}", param_hint="--args-file"
            )
    elif args:
        raw = args
    if not raw or not raw.strip():
        return {}
    try:
        parsed = json.loads(raw)
    except json.JSONDecodeError as error:
        raise typer.BadParameter(f"invalid JSON: {error}")
    if not isinstance(parsed, dict):
        raise typer.BadParameter("call parameters must be a JSON object (name -> value)")
    return parsed


def _for_display(params: dict) -> dict[str, Any]:
    """Truncate long string values so a dry-run of e.g. set_code stays readable."""
    shown: dict[str, Any] = {}
    for key, value in params.items():
        if isinstance(value, str) and len(value) > _MAX_SHOWN:
            shown[key] = f"{value[:_MAX_SHOWN]}… (+{len(value) - _MAX_SHOWN} chars)"
        else:
            shown[key] = value
    return shown


def _resolve_multisig(app_ctx, **kwargs):
    """Resolve multisig settings from a preset name or inline flags."""
    try:
        return ms_helpers.resolve_multisig(app_ctx, **kwargs)
    except ValueError as error:
        raise typer.BadParameter(str(error), param_hint="--multisig")


@with_tx_globals
def call(
    ctx: typer.Context,
    target: str = typer.Argument(
        ..., metavar="TARGET", help="The call as Pallet.function, e.g. System.set_code."
    ),
    args: Optional[str] = typer.Option(
        None, "--args", help='Call parameters as a JSON object, e.g. \'{"code": "0x.."}\'.'
    ),
    args_file: Optional[str] = typer.Option(
        None, "--args-file", help="Read the JSON parameters from a file (for large payloads)."
    ),
    sudo: bool = typer.Option(
        False, "--sudo", help="Wrap the call in Sudo.sudo (the signing key must be sudo)."
    ),
    multisig: Optional[str] = typer.Option(
        None,
        "--multisig",
        help="Named multisig wallet (see `btcli multisig list`).",
    ),
    multisig_threshold: Optional[int] = typer.Option(
        None,
        "--multisig-threshold",
        help="Dispatch via a multisig: approvals needed before the call executes.",
    ),
    signatories: Optional[str] = typer.Option(
        None,
        "--signatories",
        help="Full signer set: wallet names, address-book names, or ss58 "
        "(comma/space-separated; include yourself).",
    ),
    other_signatories: Optional[str] = typer.Option(
        None,
        "--other-signatories",
        help="Other signers only: wallet names, address-book names, or ss58 "
        "(comma/space-separated); your -w wallet coldkey is added.",
    ),
    signer: str = typer.Option(
        "coldkey",
        "--signer",
        "--key",
        help="Which key signs the raw call: 'coldkey' or 'hotkey'. "
        "Also accepts a signing backend (vault, ledger, extension) because "
        "this command owns --signer; the tx-global --signer is not added.",
    ),
):
    """Submit any raw chain call (escape hatch; use `tx` for wrapped intents).

    Reaches every Pallet.function the chain exposes, including ones no intent
    wraps; the call can be wrapped in Sudo.sudo or Proxy.proxy, or dispatched
    through a multisig. Prefer `tx` for day-to-day operations: intents preview
    effects and validate arguments, while `call` submits exactly what you pass.

    Example: btcli call System.set_code --args-file runtime.json --sudo --yes
    """
    app_ctx = ctx_of(ctx)
    if signer in ms_helpers.SIGNATORY_BACKENDS:
        # This command owns --signer (role), so the tx-global backend flag is
        # not added. Accept the backend names here so `btcli call --signer vault`
        # still selects Vault instead of dying as an unknown role.
        if signer != "wallet":
            app_ctx.signer_backend = signer
        signer = "coldkey"
    elif signer not in ("coldkey", "hotkey"):
        raise typer.BadParameter(
            "must be 'coldkey', 'hotkey', or a signing backend (wallet, vault, ledger, extension)",
            param_hint="--signer",
        )
    shield_default, shield_required = _shield_policy_for_target(target)
    shield, shield_forced = app_ctx.resolve_mev_shield(
        default=shield_default, required=shield_required, op=target
    )
    proxy_for = app_ctx.resolve_dispatch_proxy()
    force_proxy_type = app_ctx.force_proxy_type
    if force_proxy_type is not None and proxy_for is None:
        raise typer.BadParameter("requires --proxy-for", param_hint="--force-proxy-type")
    threshold, sigs, preset, _signatory_refs = _resolve_multisig(
        app_ctx,
        multisig_name=multisig,
        threshold=multisig_threshold,
        signatories=signatories,
        other_signatories=other_signatories,
        signer=signer,
        wallet_default=app_ctx.wallet_name,
    )
    builder = _resolve_builder(target)
    params = resolve_builder_params(app_ctx, target, _load_params(args, args_file))
    proxy_type_value = (
        str(getattr(force_proxy_type, "value", force_proxy_type)) if force_proxy_type else None
    )
    via_multisig = threshold is not None
    if app_ctx.signatory_wallet and not via_multisig:
        # A typo'd or missing multisig name must not degrade into a plain raw
        # call: the user explicitly asked for a multisig member to sign.
        raise typer.BadParameter(
            "--signatory only applies when dispatching through a multisig, but "
            f"-w {app_ctx.wallet_name!r} is not in this environment's multisig book "
            "(`btcli multisig list`) and no --multisig/--signatories were given",
            param_hint="--signatory",
        )
    if via_multisig and len(sigs) < threshold:
        raise typer.BadParameter(
            f"need at least {threshold} signatories, got {len(sigs)}",
            param_hint="--signatories",
        )
    # Repeated ``--signatory``: chain one approval per member, in order, in
    # this single invocation (same semantics as the intent commands). With no
    # ``--signatory`` at all, the rounds are derived from what this device
    # can sign (local wallets, then address-book signer tags).
    rounds = None
    if len(app_ctx.signatory_wallets) > 1:
        if signer != "coldkey":
            raise typer.BadParameter(
                "chained approvals sign with member coldkeys; with --signer "
                "hotkey pass one --signatory per invocation",
                param_hint="--signatory",
            )
        try:
            rounds = ms_helpers.plan_signatory_rounds(
                app_ctx,
                app_ctx.signatory_wallets,
                signatories=sigs,
                threshold=threshold,
                preset=preset or app_ctx.wallet_name,
            )
        except ValueError as error:
            raise typer.BadParameter(str(error), param_hint="--signatory") from error
    elif (
        via_multisig
        and signer == "coldkey"
        and not app_ctx.signatory_wallet
        and not app_ctx.signer_backend
        and not app_ctx.signer_address
    ):
        rounds = ms_helpers.plan_default_rounds(app_ctx, signatories=sigs, threshold=threshold)
        if rounds:
            app_ctx.output.message(
                "[dim]no --signatory given — approvals planned from local wallets "
                "and address-book signer tags[/dim]"
            )
    # ``-w <multisig>`` (no separate signatory wallet): the signing member is
    # the external signer's account when one is configured (vault/ledger/
    # extension — no local wallet files needed), else a local member wallet.
    if via_multisig and rounds is None:
        try:
            ms_helpers.infer_external_signer_from_signatory(app_ctx, sigs)
            external_member = ms_helpers.external_signer_member(
                app_ctx, preset=preset or app_ctx.wallet_name, signatories=sigs
            )
        except ValueError as error:
            hint = "--signatory" if app_ctx.signatory_wallet else "--signer-address"
            raise typer.BadParameter(str(error), param_hint=hint) from error
        if external_member is not None:
            app_ctx.multisig_wallet_name = preset or app_ctx.wallet_name
        elif not app_ctx.uses_external_signer():
            signer_ss58 = None
            try:
                signer_ss58 = app_ctx.wallet().coldkeypub.ss58_address
            except Exception:
                signer_ss58 = None
            if signer_ss58 not in sigs:
                try:
                    member_name, _ss58 = ms_helpers.pick_local_signatory(
                        app_ctx,
                        preset=preset or app_ctx.wallet_name,
                        signatories=sigs,
                    )
                except ValueError as error:
                    raise typer.BadParameter(str(error), param_hint="--wallet") from error
                app_ctx.multisig_wallet_name = preset or app_ctx.wallet_name
                app_ctx.wallet_name = member_name
                app_ctx.wallet_given = True
    label = target + (" via Sudo.sudo" if sudo else "")
    if proxy_for:
        label += f" as {proxy_for} via proxy"
    if shield:
        label += " [MEV-shielded]"

    async def prepare(client):
        """Build the call against live metadata, nesting wrappers inside-out.

        Same order as ``Executor``: the raw call, then Sudo.sudo, then
        Proxy.proxy (outermost, so the sudo origin can itself be the proxied
        account).
        """
        return await nest_origin_wrappers(
            client._substrate,
            builder(**params),
            sudo=sudo,
            proxy_for=proxy_for,
            proxy_type=proxy_type_value,
        )

    if app_ctx.dry_run:
        fields: dict[str, Any] = {
            "target": target,
            "sudo": sudo,
            "signer": signer,
            "params": _for_display(params),
        }
        if proxy_for:
            fields["proxy_for"] = proxy_for
            if proxy_type_value:
                fields["force_proxy_type"] = proxy_type_value
        if shield:
            fields["mev_shield"] = True
        if via_multisig:

            async def _dry_run_multisig(client):
                await _compose_only(client, prepare)
                return await _multisig_address(client, sigs, threshold)

            ms_addr = app_ctx.run(_dry_run_multisig)
            fields["multisig_threshold"] = threshold
            fields["signatories"] = sigs
            fields["multisig_address"] = ms_addr
            if preset:
                fields["multisig_preset"] = preset
            if rounds:
                fields["approvals_this_run"] = " → ".join(
                    f"{name} ({backend})" for name, _ss58, backend in rounds
                )
        else:
            app_ctx.run(lambda client: _compose_only(client, prepare))
        fields["command"] = replay_command()
        app_ctx.output.detail("dry run: raw call", fields)
        return

    if via_multisig:
        who = preset or f"{threshold}-of-{len(sigs)}"
        if rounds:
            sequence = " → ".join(f"{name} ({backend})" for name, _ss58, backend in rounds)
            app_ctx.output.message(
                f"[dim]{len(rounds)} approvals in this run for multisig {who}: {sequence}[/dim]"
            )
            prompt = f"multisig-approve {label} ({who}, {len(rounds)} approvals)?"
        else:
            prompt = f"multisig-approve {label} ({who}, signed by {signer})?"
        success_msg = f"multisig approved {label}"
    else:
        prompt = f"submit raw call {label} (signed by {signer})?"
        success_msg = f"submitted {label}"

    context_rows: list[tuple] = [("target", target)]
    if sudo:
        context_rows.append(("sudo", "yes"))
    if proxy_for:
        context_rows.append(("proxy for", proxy_for))
    context_rows.append(("summary", label))
    signer_rows: list[tuple] = [("signer", app_ctx.wallet_name)]
    if via_multisig:
        signer_rows.append(("multisig", who))
        signer_rows.append(("threshold", f"{threshold} of {len(sigs)}"))
    if proxy_for:
        signer_rows.append(("proxy for", proxy_for))
    signer_rows.append(
        (
            "mev shield",
            "on — encrypted in the mempool until executed"
            if shield
            else "off — no protection (call is visible in the mempool)",
        )
    )
    tx_rows: list[tuple] = [("operation", target)]
    for key, value in _for_display(params).items():
        tx_rows.append((str(key).replace("_", " "), value))
    confirm_facts: list[tuple[str, str]] = []
    if via_multisig:
        confirm_facts.append(("via", f"{who} · {threshold}-of-{len(sigs)}"))
    confirm_facts.append(("signing as", app_ctx.wallet_name))
    confirm_facts.append(("mev shield", "on" if shield else "off — no protection"))
    app_ctx.show_review(
        [
            ("Call", context_rows),
            ("Fees", [("estimated fee", "unavailable")]),
            ("Signer", signer_rows),
            ("Transaction", tx_rows),
        ],
        question=prompt,
        confirm_facts=confirm_facts,
    )
    if app_ctx.uses_vault_signer():
        # Display-only context for the vault page ("what am I signing?").
        app_ctx.vault_signer().summary = label
    if via_multisig:
        signatory_refs = _signatory_refs

        async def _approve_once(client, ms, call) -> Any:
            signing = await app_ctx.resolve_signing_wallet(signer, pick_account=True)
            result = None
            try:
                use_shield = await _honor_shield(
                    client, app_ctx, shield, shield_forced, shield_required, target
                )
                if use_shield:
                    await app_ctx._prepare_two_stage_signer(signing)
                result = await ms.approve(call, signing, signer=signer, shielded=use_shield)
                return result
            finally:
                await _release_signer(signing, result)

        async def _approve_rounds(client, ms, call, call_hash) -> Any:
            """One approval per planned member, stopping once the call executes.

            The composed call is reused verbatim in every round, so the hashes
            match by construction. The stop check matters when members approved
            outside this run: another as_multi after execution would open a
            fresh operation and risk running the call twice.
            """
            result = None
            for index, (name, ss58, backend) in enumerate(rounds):
                if index:
                    if ms_helpers.multisig_executed(result.events):
                        app_ctx.output.message(
                            f"[green]the call executed after {index} approval(s) — "
                            "skipping the remaining signatories[/green]"
                        )
                        return result
                    await ms_helpers.await_pending_visible(client, sigs, threshold, call_hash)
                app_ctx.signatory_wallet = ss58
                app_ctx.signer_backend = None if backend == "wallet" else backend
                app_ctx.signer_address = None if backend == "wallet" else ss58
                app_ctx._vault_signer = None
                app_ctx._ledger_signer = None
                app_ctx.reset_extension_session()
                if backend == "wallet":
                    app_ctx.wallet_name = name
                    app_ctx.wallet_given = True
                elif app_ctx.uses_vault_signer():
                    app_ctx.vault_signer().summary = label
                app_ctx.output.step(
                    f"approval {index + 1} of {threshold}",
                    f"{name} via {backend}",
                    state="active",
                )
                result = await _approve_once(client, ms, call)
                if not result.success:
                    return result
            return result

        async def _submit_multisig(client):
            ms = await client.multisig(sigs, threshold)
            call = await prepare(client)
            composed = await client.compose(call)
            call_hash = ms_helpers.hex_bytes(composed.call_hash)
            if rounds:
                result = await _approve_rounds(client, ms, call, call_hash)
            else:
                result = await _approve_once(client, ms, call)
            followup = await ms_helpers.multisig_followup_from_composed(
                client,
                app_ctx,
                call_hash=call_hash,
                call_data=ms_helpers.hex_bytes(composed.data),
                raw_call_hash=composed.call_hash,
                ms=ms,
                signatories=sigs,
                threshold=threshold,
                signatory_refs=signatory_refs,
                target=target,
                params=params,
                args_file=args_file,
                sudo=sudo,
                proxy_for=proxy_for,
                force_proxy_type=proxy_type_value,
                preset=preset,
                signer_role=signer,
                result=result,
            )
            result.data["multisig_followup"] = followup
            return result

        result = app_ctx.run(_submit_multisig)
    else:

        async def _submit_direct(client):
            signing = await app_ctx.resolve_signing_wallet(signer, pick_account=True)
            result = None
            try:
                use_shield = await _honor_shield(
                    client, app_ctx, shield, shield_forced, shield_required, target
                )
                if use_shield:
                    await app_ctx._prepare_two_stage_signer(signing)
                result = await _submit(client, prepare, signing, signer, shielded=use_shield)
                return result
            finally:
                await _release_signer(signing, result)

        result = app_ctx.run(_submit_direct)
    if not app_ctx.output.result(result, success_msg):
        raise typer.Exit(1)


async def _honor_shield(
    client, app_ctx, shield: bool, forced: bool, required: bool, op: str
) -> bool:
    """Keep shielding unless the pallet is off and the default (not a force) can degrade."""
    if not shield:
        return False
    if await client.read("mev_shield_next_key") is not None:
        return True
    if forced or required:
        app_ctx.output.error(
            "MEV shield is not active on this network (MevShield.NextKey is unset)",
            help=(
                f"{op} cannot submit unshielded"
                if required
                else "pass --no-mev-shield to submit unshielded"
            ),
        )
        raise typer.Exit(2)
    app_ctx.output.message(
        "[dim]MEV shield is not active on this network — submitting unshielded[/dim]"
    )
    return False


async def _compose_only(client, prepare):
    """Build and compose the call to validate its params against chain metadata."""
    return await client.compose(await prepare(client))


async def _multisig_address(client, signatories, threshold):
    """Derive the on-chain multisig account address for a signer set."""
    ms = await client.multisig(signatories, threshold)
    return ms.address


async def _submit(client, prepare, wallet, signer, shielded: bool = False):
    return await client.submit_call(
        await prepare(client),
        wallet,
        signer=signer,
        shielded=shielded,
    )


async def _release_signer(signing, result) -> None:
    """Report the outcome to and close an external signer (vault page,
    extension bridge); local wallet signers have neither hook."""
    if hasattr(signing, "report_transaction_result"):
        with contextlib.suppress(Exception):
            await signing.report_transaction_result(bool(result is not None and result.success))
    if hasattr(signing, "close"):
        with contextlib.suppress(Exception):
            await signing.close()
