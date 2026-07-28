"""`btcli upgrade`: runtime-upgrade proposals — discover, verify, sign.

The release train proposes a mainnet runtime upgrade as the CI half of a
2-of-2 deployment multisig and publishes a proposal pre-release whose URL is
the one thing signers need:

    btcli upgrade pending                       # what is waiting, from chain state
    btcli upgrade check --url <release-url>     # verify the call data end to end
    btcli upgrade sign  --url <release-url> -w <wallet>   # approve it

`sign` re-runs every check first and refuses on any mismatch, then picks the
right approval (first / interior / final) from chain state — no signer
numbers, timepoints, or PolkadotJS required.
"""

from __future__ import annotations

import asyncio
from typing import Any, Optional

import typer

from ... import calls
from .. import multisig_helpers as ms_helpers
from .. import upgrade_helpers as uh
from ..context import AppContext, ctx_of
from ..globals import with_globals, with_tx_globals
from ..prompt import confirm_wallet

app = typer.Typer(
    no_args_is_help=True,
    help="Discover, verify, and sign runtime-upgrade proposals.",
)


# --- shared rendering --------------------------------------------------------------------


def _approval_summary(approvals: list[str], threshold: int) -> str:
    return f"{len(approvals)} of {threshold} approvals"


def upgrade_record_fields(record: dict[str, Any]) -> dict[str, Any]:
    """Human key/value view of one discovered pending upgrade."""
    fields: dict[str, Any] = {}
    release = record.get("release")
    if release:
        if release.get("spec_version") is not None:
            fields["spec_version"] = release["spec_version"]
        fields["release"] = release.get("url")
        if release.get("tag"):
            fields["source"] = f"{release.get('repo')}@{release['tag']} ({release.get('commit')})"
    fields["call_hash"] = record["call_hash"]
    fields["proposed at"] = record["timepoint_display"]
    fields["CI key"] = record["ci_address"]
    fields["deployment multisig"] = record["deployment_multisig"]
    fields["sudo key"] = record["sudo_key"]
    fields["deployment layer"] = _approval_summary(record["deployment_approvals"], 2) + " (CI)"
    sudo_layer = record.get("sudo_layer")
    threshold = record.get("sudo_threshold")
    if sudo_layer:
        summary = _approval_summary(sudo_layer["approvals"], threshold or 0)
        if not threshold:
            summary = f"{len(sudo_layer['approvals'])} approvals so far"
        fields["sudo multisig layer"] = (
            f"{summary} — opened at "
            f"{sudo_layer['timepoint']['height']}:{sudo_layer['timepoint']['index']}"
        )
    elif record.get("release"):
        fields["sudo multisig layer"] = "not opened yet — the next signer opens it"
    if record.get("sign_command"):
        fields["sign with"] = record["sign_command"]
    elif not release:
        fields["note"] = (
            "no matching release manifest found — pass the proposal's release URL "
            "to `btcli upgrade check/sign` directly"
        )
    return fields


def render_upgrade_records(app_ctx: AppContext, records: list[dict[str, Any]]) -> None:
    out = app_ctx.output
    if out.json_mode:
        out.value({"pending_upgrades": records, "count": len(records)})
        return
    if not records:
        out.detail("pending runtime upgrades", {})
        return
    for index, record in enumerate(records):
        title = "pending runtime upgrade" if index == 0 else None
        out.detail(title, upgrade_record_fields(record))


async def _enrich_upgrades(client, app_ctx: AppContext, records: list[dict], repo: str) -> None:
    """Attach release/manifest info and sudo-layer status to discovered upgrades.

    Enrichment is advisory: GitHub being unreachable degrades the output but
    never fails the command. Fetches run in a worker thread so the websocket
    stays serviced.
    """
    manifests = await asyncio.to_thread(uh.fetch_release_manifests, repo)
    for record in records:
        manifest = uh.find_manifest_for_call_hash(manifests, record["call_hash"])
        if not manifest:
            continue
        record["release"] = {
            "url": manifest.get("release_url"),
            "repo": manifest.get("repo"),
            "tag": manifest.get("tag"),
            "commit": manifest.get("commit"),
            "spec_version": manifest.get("spec_version"),
            "prerelease": manifest.get("prerelease"),
        }
        if manifest.get("release_url"):
            record["sign_command"] = uh.sign_command(app_ctx.network, manifest["release_url"])
        sudo = manifest.get("sudo") or {}
        if sudo.get("threshold"):
            record["sudo_threshold"] = int(sudo["threshold"])
        call_data_url = (manifest.get("assets") or {}).get("call_data")
        if not call_data_url:
            continue
        try:
            blob = uh.parse_hex_blob(await asyncio.to_thread(uh.fetch_bytes, call_data_url))
        except ValueError:
            continue
        if uh.call_hash_hex(blob) != record["call_hash"]:
            continue  # release asset does not match this on-chain op; don't trust it
        finalizing = await uh.compose_finalizing_call(
            client,
            blob=blob,
            ci_address=record["ci_address"],
            deploy_timepoint=record["timepoint"],
        )
        record["sudo_layer"] = await uh.sudo_layer_status(
            client,
            sudo_key=record["sudo_key"],
            finalizing_call_hash=ms_helpers.hex_bytes(finalizing.call_hash),
        )


async def load_pending_upgrades(client, app_ctx: AppContext, repo: str) -> list[dict]:
    records = await uh.discover_pending_upgrades(client)
    if records:
        await _enrich_upgrades(client, app_ctx, records, repo)
    return records


# --- commands ----------------------------------------------------------------------------


@app.command("pending")
@with_globals
def upgrade_pending(
    ctx: typer.Context,
    repo: str = typer.Option(
        uh.DEFAULT_UPGRADE_REPO,
        "--repo",
        help="GitHub repo whose releases carry the upgrade manifests.",
    ),
):
    """List pending runtime-upgrade proposals, discovered from chain state.

    Walks sudo.key() -> its SudoUncheckedSetCode proxy -> pending multisig
    operations, then matches each against the repo's release manifests to show
    the spec version, source commit, both approval layers, and the exact sign
    command.
    """
    app_ctx = ctx_of(ctx)
    records = app_ctx.run(lambda client: load_pending_upgrades(client, app_ctx, repo))
    render_upgrade_records(app_ctx, records)


def _bundle_or_exit(
    app_ctx: AppContext,
    *,
    url: Optional[str],
    hex_url: Optional[str],
    hex_file: Optional[str],
    wasm: Optional[str],
) -> uh.ProposalBundle:
    app_ctx.output.message("[dim]fetching proposal artifacts…[/dim]")
    try:
        bundle = uh.fetch_proposal_bundle(
            release_url=url, hex_url=hex_url, hex_file=hex_file, wasm_path=wasm
        )
    except (ValueError, OSError) as error:
        app_ctx.output.error(str(error))
        raise typer.Exit(1)
    for note in bundle.notes:
        app_ctx.output.message(f"[dim]note: {note}[/dim]")
    return bundle


def _render_checks(app_ctx: AppContext, outcome: uh.CheckOutcome) -> None:
    out = app_ctx.output
    if out.json_mode:
        out.value(
            {
                "ok": outcome.ok,
                "checks": [
                    {"name": c.name, "ok": c.ok, "detail": c.detail} for c in outcome.checks
                ],
                "data": outcome.data,
            }
        )
        return
    glyphs = {True: "✓", False: "✗ FAIL:", None: "– skipped:"}
    fields = {c.name: f"{glyphs[c.ok]} {c.detail}" for c in outcome.checks}
    out.detail("upgrade check", fields)


def _print_reproduce_recipe(app_ctx: AppContext, bundle: uh.ProposalBundle) -> None:
    """When the wasm came from the release (or nowhere), point at self-building."""
    if bundle.wasm_source == "local" or app_ctx.output.json_mode:
        return
    out = app_ctx.output
    out.message("")
    out.message(
        "to verify against code you compiled yourself, build the runtime with "
        "srtool and re-run with --wasm:"
    )
    for line in uh.srtool_recipe(bundle.manifest):
        out.message(f"  [dim]{line}[/dim]")


@app.command(
    "check",
    epilog=(
        "Example: btcli upgrade check "
        "--url https://github.com/RaoFoundation/subtensor/releases/tag/v426 "
        "--wasm ./my-srtool-build.wasm"
    ),
)
@with_globals
def upgrade_check(
    ctx: typer.Context,
    url: Optional[str] = typer.Option(
        None,
        "--url",
        help="Proposal release page (https://github.com/O/R/releases/tag/TAG or O/R@TAG).",
    ),
    hex_url: Optional[str] = typer.Option(
        None, "--hex-url", help="URL of the raw call-data hex (proxy_proxy_blob.hex)."
    ),
    hex_file: Optional[str] = typer.Option(
        None, "--hex-file", help="Local path to the call-data hex file."
    ),
    wasm: Optional[str] = typer.Option(
        None,
        "--wasm",
        help="Runtime wasm you built yourself (srtool); pins the call data to your build.",
    ),
):
    """Verify a proposed runtime upgrade end to end without signing anything.

    Checks that the call data is exactly proxy.proxy(sudo_key, None,
    sudoUncheckedWeight(setCode(wasm), <pinned weight>)), that re-encoding it
    against live chain metadata reproduces it byte-for-byte, that the embedded
    runtime matches the wasm and srtool digest, that the proxied account is
    the chain's sudo key, and that a pending on-chain proposal carries its
    exact call hash.
    """
    app_ctx = ctx_of(ctx)
    bundle = _bundle_or_exit(app_ctx, url=url, hex_url=hex_url, hex_file=hex_file, wasm=wasm)
    outcome = app_ctx.run(lambda client: uh.run_proposal_checks(client, bundle))
    _render_checks(app_ctx, outcome)
    _print_reproduce_recipe(app_ctx, bundle)
    if not outcome.ok:
        app_ctx.output.error(
            "verification failed: " + "; ".join(c.name for c in outcome.failed()),
            help="do not sign this proposal until every check passes",
        )
        raise typer.Exit(1)


def _resolve_sudo_signer_set(
    app_ctx: AppContext,
    *,
    multisig: Optional[str],
    multisig_threshold: Optional[int],
    signatories: Optional[str],
    other_signatories: Optional[str],
    manifest: Optional[dict],
) -> tuple[int, list[str]]:
    """The sudo multisig's (threshold, resolved signatories) for signing.

    Explicit flags win; otherwise the release manifest's published signer set
    is used. Either way the derived address is verified against the live sudo
    key before anything is submitted.
    """
    threshold, sigs, _preset, _refs = ms_helpers.resolve_multisig(
        app_ctx,
        multisig_name=multisig,
        threshold=multisig_threshold,
        signatories=signatories,
        other_signatories=other_signatories,
        signer="coldkey",
        wallet_default=None,
    )
    if threshold is not None:
        return threshold, sigs
    sudo = (manifest or {}).get("sudo") or {}
    manifest_sigs = [str(s) for s in sudo.get("signatories") or []]
    manifest_threshold = sudo.get("threshold")
    if manifest_sigs and manifest_threshold:
        return int(manifest_threshold), list(dict.fromkeys(manifest_sigs))
    raise ValueError(
        "cannot determine the sudo multisig signer set: the release manifest has "
        "no sudo block — pass `--multisig NAME` (a saved multisig) or "
        "`--multisig-threshold N --signatories a,b,c`"
    )


@app.command(
    "sign",
    epilog=(
        "Example: btcli upgrade sign "
        "--url https://github.com/RaoFoundation/subtensor/releases/tag/v426 -w trium-a"
    ),
)
@with_tx_globals
def upgrade_sign(
    ctx: typer.Context,
    url: Optional[str] = typer.Option(
        None,
        "--url",
        help="Proposal release page (https://github.com/O/R/releases/tag/TAG or O/R@TAG).",
    ),
    hex_url: Optional[str] = typer.Option(
        None, "--hex-url", help="URL of the raw call-data hex (proxy_proxy_blob.hex)."
    ),
    hex_file: Optional[str] = typer.Option(
        None, "--hex-file", help="Local path to the call-data hex file."
    ),
    wasm: Optional[str] = typer.Option(
        None,
        "--wasm",
        help="Runtime wasm you built yourself (srtool); pins the call data to your build.",
    ),
    multisig: Optional[str] = typer.Option(
        None, "--multisig", help="Saved sudo-multisig name (see `btcli multisig list`)."
    ),
    multisig_threshold: Optional[int] = typer.Option(
        None, "--multisig-threshold", help="Sudo multisig threshold (with --signatories)."
    ),
    signatories: Optional[str] = typer.Option(
        None,
        "--signatories",
        help="Full sudo signer set: ss58, address-book names, or wallet names.",
    ),
    other_signatories: Optional[str] = typer.Option(
        None,
        "--other-signatories",
        help="Other sudo signers only; your -w wallet coldkey is added.",
    ),
):
    """Verify a proposed runtime upgrade, then submit your multisig approval.

    Runs every `upgrade check` verification first and refuses to sign on any
    mismatch. Your position is read from chain state: if the sudo multisig has
    not acted yet you open the operation (first approval), if it is underway
    you approve, and if yours is the final approval the upgrade executes in
    the same extrinsic.
    """
    app_ctx = ctx_of(ctx)
    bundle = _bundle_or_exit(app_ctx, url=url, hex_url=hex_url, hex_file=hex_file, wasm=wasm)

    confirm_wallet(
        app_ctx,
        help_text="Wallet whose coldkey is a sudo multisig signatory.",
        require_coldkey=True,
    )
    try:
        sudo_threshold, sudo_signatories = _resolve_sudo_signer_set(
            app_ctx,
            multisig=multisig,
            multisig_threshold=multisig_threshold,
            signatories=signatories,
            other_signatories=other_signatories,
            manifest=bundle.manifest,
        )
    except ValueError as error:
        app_ctx.output.error(str(error))
        raise typer.Exit(1)

    signer_address = app_ctx.wallet().coldkeypub.ss58_address
    if signer_address not in sudo_signatories:
        app_ctx.output.error(
            f"wallet {app_ctx.wallet_name!r} coldkey {signer_address} is not in the "
            "sudo multisig signer set",
            note="signatories: " + ", ".join(sudo_signatories),
        )
        raise typer.Exit(1)

    # Phase 1: verify everything and read the current signing position.
    async def _verify(client):
        outcome = await uh.run_proposal_checks(client, bundle)
        plan: dict[str, Any] = {}
        if outcome.ok:
            derived = await client.multisig(sudo_signatories, sudo_threshold)
            plan["multisig_address"] = derived.address
            plan["matches_sudo"] = derived.address == outcome.data["sudo_key"]
            if plan["matches_sudo"]:
                pending = outcome.data["pending"]
                finalizing = await uh.compose_finalizing_call(
                    client,
                    blob=bundle.blob,
                    ci_address=pending["ci_address"],
                    deploy_timepoint=pending["timepoint"],
                )
                plan["finalizing_call_hash"] = ms_helpers.hex_bytes(finalizing.call_hash)
                plan["sudo_layer"] = await uh.sudo_layer_status(
                    client,
                    sudo_key=outcome.data["sudo_key"],
                    finalizing_call_hash=plan["finalizing_call_hash"],
                )
        return outcome, plan

    outcome, plan = app_ctx.run(_verify)
    _render_checks(app_ctx, outcome)
    if not outcome.ok:
        app_ctx.output.error(
            "verification failed: " + "; ".join(c.name for c in outcome.failed()),
            help="refusing to sign; resolve every failed check first",
        )
        raise typer.Exit(1)
    if not plan["matches_sudo"]:
        app_ctx.output.error(
            f"the resolved signer set derives {plan['multisig_address']}, which is not "
            f"the chain's sudo key {outcome.data['sudo_key']}",
            help="check the threshold and signatory list (or the saved multisig entry)",
        )
        raise typer.Exit(1)

    position, prompt = _signing_position(
        plan.get("sudo_layer"), sudo_threshold, signer_address, outcome.data
    )
    if position == "signed":
        app_ctx.output.message(prompt)
        _print_share_hint(app_ctx, url, plan.get("sudo_layer"), sudo_signatories)
        return

    if bundle.wasm_source != "local":
        app_ctx.output.message(
            "[yellow]note:[/yellow] the runtime was taken from the release, not a local "
            "srtool build — anyone reproducing the build should pass --wasm"
        )

    if app_ctx.dry_run:
        app_ctx.output.detail(
            "dry run: upgrade sign",
            {
                "position": position,
                "action": prompt,
                "sudo_multisig": plan["multisig_address"],
                "finalizing_call_hash": plan["finalizing_call_hash"],
            },
        )
        return

    app_ctx.confirm(prompt + "?")
    signing = app_ctx.signer("coldkey")

    # Phase 2: re-read the position from live state and submit the matching
    # approval (state may have advanced since the prompt — later is only ever
    # *more* approvals, and the submission adapts).
    async def _submit(client):
        pending = await uh.find_pending_upgrade(client, bundle.call_hash)
        if pending is None:
            raise ValueError(
                "the deployment-multisig proposal disappeared from chain state "
                "(executed or cancelled since verification)"
            )
        finalizing = await uh.compose_finalizing_call(
            client,
            blob=bundle.blob,
            ci_address=pending["ci_address"],
            deploy_timepoint=pending["timepoint"],
        )
        sudo_layer = await uh.sudo_layer_status(
            client,
            sudo_key=pending["sudo_key"],
            finalizing_call_hash=ms_helpers.hex_bytes(finalizing.call_hash),
        )
        approvals = (sudo_layer or {}).get("approvals") or []
        if signer_address in approvals:
            return None, sudo_layer
        others = uh.sorted_other_signatories(sudo_signatories, signer_address)
        # The outer max_weight must cover the finalizing call's *declared*
        # dispatch weight, which already includes the pinned FINALIZE_WEIGHT
        # it carries as its own as_multi argument plus the pallet's base
        # overhead — so the pinned constant itself is always too low here.
        # Only the finalizing call's encoding must be byte-identical across
        # signers; this outer argument is free to be estimated per submission.
        max_weight = await uh.finalizing_max_weight(client, finalizing, signer_address)
        if sudo_layer is None:
            call = calls.Multisig.approve_as_multi(
                threshold=sudo_threshold,
                other_signatories=others,
                maybe_timepoint=None,
                call_hash=finalizing.call_hash,
                max_weight=max_weight,
            )
        elif len(approvals) < sudo_threshold - 1:
            call = calls.Multisig.approve_as_multi(
                threshold=sudo_threshold,
                other_signatories=others,
                maybe_timepoint=sudo_layer["timepoint"],
                call_hash=finalizing.call_hash,
                max_weight=max_weight,
            )
        else:
            call = calls.Multisig.as_multi(
                threshold=sudo_threshold,
                other_signatories=others,
                maybe_timepoint=sudo_layer["timepoint"],
                call=finalizing,
                max_weight=max_weight,
            )
        result = await client.submit_call(
            call, signing, signer="coldkey", wait_for_finalization=False
        )
        updated = await uh.sudo_layer_status(
            client,
            sudo_key=pending["sudo_key"],
            finalizing_call_hash=ms_helpers.hex_bytes(finalizing.call_hash),
        )
        return result, updated

    result, sudo_layer = app_ctx.run(_submit)
    if result is None:
        app_ctx.output.message("your approval is already recorded on-chain; nothing to submit")
        _print_share_hint(app_ctx, url, sudo_layer, sudo_signatories)
        return
    if not app_ctx.output.result(result, "runtime-upgrade approval submitted"):
        raise typer.Exit(1)
    _report_after_sign(app_ctx, url, sudo_layer, sudo_signatories, sudo_threshold)


def _signing_position(
    sudo_layer: Optional[dict],
    threshold: int,
    signer_address: str,
    check_data: dict[str, Any],
) -> tuple[str, str]:
    """(position, human action line) for the confirmation prompt."""
    spec = ((check_data.get("manifest") or {}).get("spec_version")) or "?"
    if sudo_layer is None:
        return (
            "first",
            f"open the sudo-multisig approval for runtime upgrade {spec} "
            f"(approval 1 of {threshold})",
        )
    approvals = sudo_layer.get("approvals") or []
    if signer_address in approvals:
        return (
            "signed",
            f"your approval is already recorded ({len(approvals)} of {threshold} so far)",
        )
    if len(approvals) >= threshold - 1:
        return (
            "final",
            f"submit the FINAL approval for runtime upgrade {spec} — "
            "the upgrade executes in this extrinsic",
        )
    return (
        "interior",
        f"add approval {len(approvals) + 1} of {threshold} for runtime upgrade {spec}",
    )


def _print_share_hint(
    app_ctx: AppContext,
    url: Optional[str],
    sudo_layer: Optional[dict],
    sudo_signatories: list[str],
) -> None:
    if app_ctx.output.json_mode or not url:
        return
    approvals = (sudo_layer or {}).get("approvals") or []
    remaining = [s for s in sudo_signatories if s not in approvals]
    if remaining:
        app_ctx.output.message("waiting on: " + ", ".join(remaining))
        app_ctx.output.message(f"they can run: {uh.sign_command(app_ctx.network, url)}")


def _report_after_sign(
    app_ctx: AppContext,
    url: Optional[str],
    sudo_layer: Optional[dict],
    sudo_signatories: list[str],
    threshold: int,
) -> None:
    if sudo_layer is None:
        # The op is gone from pending state: the threshold was reached and the
        # finalizing call executed — the upgrade is done (or the pending read
        # lagged; `upgrade pending` will confirm).
        app_ctx.output.message(
            "the sudo multisig operation is complete — the upgrade should now "
            "execute; verify with `btcli upgrade pending` and the chain's spec version"
        )
        return
    approvals = sudo_layer.get("approvals") or []
    fields: dict[str, Any] = {
        "approvals": f"{len(approvals)} of {threshold}",
        "opened at": (f"{sudo_layer['timepoint']['height']}:{sudo_layer['timepoint']['index']}"),
        "call_hash": sudo_layer["call_hash"],
    }
    remaining = [s for s in sudo_signatories if s not in approvals]
    if remaining:
        fields["waiting on"] = ", ".join(remaining)
        if url:
            fields["they run"] = uh.sign_command(app_ctx.network, url)
    app_ctx.output.detail("sudo multisig status", fields)


__all__ = ["app", "load_pending_upgrades", "render_upgrade_records", "upgrade_record_fields"]
