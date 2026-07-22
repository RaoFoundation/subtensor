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

    btcli addresses triumph-a 5OtherA...
    btcli addresses triumph-b 5OtherB...
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

import json
from typing import Any, Optional

import typer

from .. import calls
from ..intents.proxy import ProxyTypeChoice
from . import multisig_helpers as ms_helpers
from .context import ctx_of
from .globals import with_tx_globals

_MAX_SHOWN = 80  # truncate long param values (e.g. a wasm blob) in dry-run output


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
        help="Full signer set: ss58, address-book names, or wallet names (include yourself).",
    ),
    other_signatories: Optional[str] = typer.Option(
        None,
        "--other-signatories",
        help="Other signers only (book names or ss58); your -w wallet coldkey is added.",
    ),
    signer: str = typer.Option(
        "coldkey", "--signer", help="Which wallet key signs: 'coldkey' or 'hotkey'."
    ),
    proxy_for: Optional[str] = typer.Option(
        None,
        "--proxy-for",
        help="Dispatch as this account (ss58, proxy-book/address-book name, or local "
        "wallet name) via Proxy.proxy; the signing key — or the multisig, when "
        "combined with --multisig — must be its registered proxy.",
    ),
    force_proxy_type: Optional[ProxyTypeChoice] = typer.Option(
        None,
        "--force-proxy-type",
        help="Require this exact proxy type to be used (with --proxy-for).",
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
    if signer not in ("coldkey", "hotkey"):
        raise typer.BadParameter("must be 'coldkey' or 'hotkey'", param_hint="--signer")
    if force_proxy_type is not None and proxy_for is None:
        raise typer.BadParameter("requires --proxy-for", param_hint="--force-proxy-type")
    threshold, sigs, preset, _signatory_refs = _resolve_multisig(
        app_ctx,
        multisig_name=multisig,
        threshold=multisig_threshold,
        signatories=signatories,
        other_signatories=other_signatories,
        signer=signer,
    )
    builder = _resolve_builder(target)
    params = _load_params(args, args_file)
    if proxy_for is not None:
        proxy_for = app_ctx.resolve_address("proxy_for", proxy_for)
    proxy_type_value = force_proxy_type.value if force_proxy_type else None
    signing = app_ctx.signer(signer)
    label = target + (" via Sudo.sudo" if sudo else "")
    if proxy_for:
        label += f" as {proxy_for} via proxy"
    via_multisig = threshold is not None
    if via_multisig and len(sigs) < threshold:
        raise typer.BadParameter(
            f"need at least {threshold} signatories, got {len(sigs)}",
            param_hint="--signatories",
        )

    async def prepare(client):
        """Build the call against live metadata, nesting the wrappers inside-out:
        the raw call, then Sudo.sudo, then Proxy.proxy (outermost, so the sudo
        origin can itself be the proxied account)."""
        built = builder(**params)
        if sudo:
            built = calls.Sudo.sudo(call=await client.compose(built))
        if proxy_for:
            built = calls.Proxy.proxy(
                real=proxy_for,
                force_proxy_type=proxy_type_value,
                call=await client.compose(built),
            )
        return built

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
        else:
            app_ctx.run(lambda client: _compose_only(client, prepare))
        app_ctx.output.detail("dry run: raw call", fields)
        return

    if via_multisig:
        who = preset or f"{threshold}-of-{len(sigs)}"
        prompt = f"multisig-approve {label} ({who}, signed by {signer})?"
        success_msg = f"multisig approved {label}"
    else:
        prompt = f"submit raw call {label} (signed by {signer})?"
        success_msg = f"submitted {label}"

    app_ctx.confirm(prompt)
    if via_multisig:
        signatory_refs = _signatory_refs

        async def _submit_multisig(client):
            ms = await client.multisig(sigs, threshold)
            call = await prepare(client)
            composed = await client.compose(call)
            result = await ms.approve(call, signing, signer=signer)
            followup = await ms_helpers.multisig_followup_from_composed(
                client,
                app_ctx,
                call_hash=ms_helpers.hex_bytes(composed.call_hash),
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
        result = app_ctx.run(lambda client: _submit(client, prepare, signing, signer))
    if not app_ctx.output.result(result, success_msg):
        raise typer.Exit(1)


async def _compose_only(client, prepare):
    """Build and compose the call to validate its params against chain metadata."""
    return await client.compose(await prepare(client))


async def _multisig_address(client, signatories, threshold):
    """Derive the on-chain multisig account address for a signer set."""
    ms = await client.multisig(signatories, threshold)
    return ms.address


async def _submit(client, prepare, wallet, signer):
    return await client.submit_call(await prepare(client), wallet, signer=signer)
