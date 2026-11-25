#!/usr/bin/env python3
"""Verify a data-only mainnet proposal and use the standard multisig flow.

Run this checked-in file from a separately reviewed repository checkout. Never
run a verifier supplied by a CI artifact. No keys or seeds are accepted here;
btcli handles the operator's local wallet. The call itself is always composed
with the SDK, so the downloaded artifact is data rather than executable code.
"""

import argparse
import shutil
import asyncio
import json
import os
import re
import subprocess
import sys
import tempfile
from pathlib import Path


def arguments(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("proposal", type=Path, help="Downloaded data-only CI proposal")
    parser.add_argument(
        "reproduced_proposal",
        type=Path,
        help="Proposal independently reproduced from the reviewed source and baseline",
    )
    parser.add_argument(
        "--expected-call-hash",
        required=True,
        help="Hash independently reproduced from the reviewed source and baseline",
    )
    parser.add_argument(
        "--operation", required=True,
        help="open for the designated initiator; HEIGHT:INDEX from the initiator for every later approval",
    )
    options = parser.parse_args(argv)
    if options.operation != "open" and not re.fullmatch(r"[0-9]{1,10}:[0-9]{1,10}", options.operation):
        parser.error("--operation must be open or HEIGHT:INDEX")
    return options


def sdk_call(value):
    """Convert a pallet/function variant call to the SDK's Call type."""
    import bittensor as bt

    if isinstance(value, list):
        return [sdk_call(item) for item in value]
    if isinstance(value, dict) and len(value) == 1:
        pallet, function_map = next(iter(value.items()))
        if isinstance(function_map, dict) and len(function_map) == 1:
            function, arguments = next(iter(function_map.items()))
            if callable(getattr(getattr(bt.calls, pallet, None), function, None)):
                return bt.calls.Call(
                    pallet,
                    function,
                    {key: sdk_call(item) for key, item in arguments.items()},
                )
    return normalize(value)


def normalize(value):
    if isinstance(value, dict) and set(value) == {"none"} and value["none"] is True:
        return None
    if isinstance(value, dict) and set(value) == {"bits"}:
        bits = value["bits"]
        return {"bits": int(bits, 0) if isinstance(bits, str) else bits}
    if isinstance(value, dict):
        return {key: normalize(item) for key, item in value.items()}
    if isinstance(value, list):
        return [normalize(item) for item in value]
    if isinstance(value, str) and re.fullmatch(r"-?[0-9]+", value):
        return int(value)
    return value


def load_proposal(path, reproduced_path, expected):
    if not re.fullmatch(r"0x[0-9a-fA-F]{64}", expected):
        raise ValueError("Expected call hash must be 32 bytes of hex")
    if path.resolve() == reproduced_path.resolve():
        raise ValueError("The independently reproduced proposal must be a separate file")
    proposal = json.loads(path.read_text())
    reproduced = json.loads(reproduced_path.read_text())
    if proposal.get("network") != "mainnet" or reproduced.get("network") != "mainnet":
        raise ValueError("Countersigning is only available for a mainnet proposal")
    for candidate in (proposal, reproduced):
        if not candidate.get("provenance", {}).get("baseCommit"):
            raise ValueError("Proposal has no exact reviewed baseline")
        if candidate.get("call_hash") != expected:
            raise ValueError(
                "Artifact or reproduced call hash differs from the independently reproduced hash"
            )
        if not candidate.get("call_data"):
            raise ValueError("Proposal contains no call data")
    if proposal["btcli_args"] != reproduced["btcli_args"]:
        raise ValueError("Artifact calls differ from the independently reproduced calls")
    if proposal["provenance"] != reproduced["provenance"]:
        raise ValueError("Artifact provenance differs from the reproduced proposal")
    if proposal.get("signers") != reproduced.get("signers"):
        raise ValueError("Artifact signers differ from the reproduced proposal")
    if proposal.get("sudoKey") != reproduced.get("sudoKey"):
        raise ValueError("Artifact sudo key differs from the reproduced proposal")
    if proposal["call_data"] != reproduced["call_data"]:
        raise ValueError("Artifact call data differs from the reproduced proposal")
    if proposal["decoded_call"] != reproduced["decoded_call"]:
        raise ValueError("Artifact decoded call differs from the reproduced proposal")
    affected = reproduced["diff"]["proposed"]
    return proposal, reproduced, affected


def map_affected_storage(call_args):
    """Derive storage and new values from authenticated calls and the allowlist."""
    try:
        result = subprocess.run(
            ["node", str(Path(__file__).with_name("verify-proposal.js"))],
            input=json.dumps(call_args),
            check=True,
            capture_output=True,
            text=True,
        )
    except subprocess.CalledProcessError as error:
        raise ValueError(error.stderr.strip() or "Reviewed batch mapping failed") from error
    return json.loads(result.stdout)


def verify_preconditions(proposal, affected):
    """Authenticate artifact preconditions against reproduced calls and mapping."""
    if not affected:
        raise ValueError("Proposal contains no reviewed changes")
    keys = [entry["key"] for entry in affected]
    if len(keys) != len(set(keys)):
        raise ValueError("Duplicate affected storage entries")
    allowed = map_affected_storage(proposal["btcli_args"])
    expected = [{key: value for key, value in entry.items() if key != "live"} for entry in affected]
    if allowed != expected:
        raise ValueError(
            "Affected storage differs from the reviewed setter mapping or reproduced proposal"
        )
    if proposal["diff"]["proposed"] != affected:
        raise ValueError("Artifact old values differ from the reproduced proposal")


async def verify(proposal, affected, call_args, signatories, threshold):
    import bittensor as bt

    async with bt.Subtensor("finney", fallback_endpoints=[], archive_endpoints=[]) as client:
        batch = call_args["call"]["Utility"]["batch_all"]["calls"]
        inner_calls = [await client.compose(sdk_call(value)) for value in batch]
        batch_call = await client.compose(bt.calls.Utility.batch_all(calls=inner_calls))
        snapshot = await client.at(await client.finalized_block())
        sudo_key = str(await snapshot.query(bt.storage.Sudo.Key))
        multisig = await client.multisig(signatories, threshold, account_mode="legacy")
        if multisig.address != sudo_key:
            raise ValueError("Signer set does not derive the live sudo key")
        if str(proposal.get("sudoKey")) != sudo_key:
            raise ValueError("Proposal sudo key differs from the live sudo key")
        call = await client.compose(bt.calls.Sudo.sudo(call=batch_call))
        actual = "0x" + bytes(call.call_hash).hex()
        if actual != proposal["call_hash"]:
            raise ValueError(f"Recomposed call hash {actual} differs from the reviewed hash")
        # Recheck old values only after reproducing the call and authenticating
        # every storage precondition in main(). No artifact field supplies the
        # storage-to-setter mapping.
        for entry in affected:
            live = await snapshot.query(tuple(entry["storage"]))
            if normalize(live) != normalize(entry["live"]):
                raise ValueError(f"Live value changed for {entry['key']}; regenerate the proposal")
        print(
            "Verified live sudo key, independently supplied call hash, "
            "and every affected old value at this read."
        )
        print("Call hash:", actual)
        return call


def btcli_command(call_args_path, signatories, threshold, wallet, expected, operation):
    return [
        shutil.which("btcli") or "btcli",
        "call",
        "Sudo.sudo",
        "--args-file",
        str(call_args_path),
        "--multisig-account-mode",
        "legacy",
        "--multisig-threshold",
        str(threshold),
        "--signatories",
        ",".join(signatories),
        "-w",
        wallet,
        "--network",
        "finney",
        "--expected-call-hash",
        expected,
        "--multisig-operation",
        operation,
        "--no-mev-shield",
        "--yes",
    ]


def main(argv=None):
    args = arguments(argv)
    wallet = os.environ.get("SUDO_WALLET")
    if not wallet or wallet.startswith("<"):
        raise ValueError("Set SUDO_WALLET to your own local triumvirate member wallet")
    proposal, reproduced, affected = load_proposal(
        args.proposal.resolve(),
        args.reproduced_proposal.resolve(),
        args.expected_call_hash,
    )
    verify_preconditions(proposal, affected)
    config_path = (
        Path(__file__).resolve().parent.parent / ".github/scripts/deploy/sudo-signatories.json"
    )
    config = json.loads(config_path.read_text())
    signatories = config["signatories"]
    threshold = config["threshold"]
    if proposal.get("signers") is None:
        raise ValueError("Proposal has no signer configuration")
    if proposal["signers"]["threshold"] != threshold or set(
        proposal["signers"]["signatories"]
    ) != set(signatories):
        raise ValueError("Proposal signers differ from the reviewed repository configuration")
    call_args = reproduced["btcli_args"]
    for entry in affected:
        print(f"{entry['key']}: {entry['live']} -> {entry['proposed']}")
    print(f"Reviewed call hash: {args.expected_call_hash}")
    if input(f"Confirm one Finney approval ({args.operation})? [y/N] ").strip().lower() != "y":
        return 1
    with tempfile.TemporaryDirectory(prefix="sudo-mainnet-verification-") as directory:
        args_path = Path(directory) / "verified-mainnet-args.json"
        args_path.write_text(json.dumps(normalize(call_args), indent=2) + "\n")
        asyncio.run(verify(proposal, affected, call_args, signatories, threshold))
        return subprocess.run(btcli_command(
            args_path, signatories, threshold, wallet, args.expected_call_hash, args.operation,
        )).returncode


if __name__ == "__main__":
    try:
        sys.exit(main())
    except (KeyError, TypeError, ValueError, json.JSONDecodeError) as error:
        raise SystemExit(f"Refusing approval: {error}") from error
