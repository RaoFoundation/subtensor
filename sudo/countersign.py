#!/usr/bin/env python3
"""Verify a data-only mainnet proposal and use the standard multisig flow.

Run this checked-in file from a separately reviewed repository checkout. Never
run a verifier supplied by a CI artifact. No keys or seeds are accepted here;
btcli handles the operator's local wallet. The call itself is always composed
with the SDK, so the downloaded artifact is data rather than executable code.
"""

import argparse
import asyncio
import json
import os
import re
from pathlib import Path
import subprocess
import sys


def arguments(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("proposal", type=Path)
    parser.add_argument(
        "--expected-call-hash",
        required=True,
        help="Hash independently reproduced from the reviewed source and baseline",
    )
    return parser.parse_args(argv)


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
    return value


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
    if isinstance(value, str) and value.isdecimal():
        return int(value)
    return value


def load_proposal(path, expected):
    if not re.fullmatch(r"0x[0-9a-fA-F]{64}", expected):
        raise ValueError("Expected call hash must be 32 bytes of hex")
    proposal = json.loads(path.read_text())
    if proposal.get("network") != "mainnet":
        raise ValueError("Countersigning is only available for a mainnet proposal")
    if not proposal.get("provenance", {}).get("baseCommit"):
        raise ValueError("Proposal has no exact reviewed baseline")
    if proposal.get("call_hash") != expected:
        raise ValueError(
            "Artifact call hash differs from the independently reproduced hash"
        )
    if not proposal.get("call_data"):
        raise ValueError("Proposal contains no call data")
    affected = proposal["diff"]["proposed"]
    if not affected:
        raise ValueError("Proposal contains no reviewed changes")
    keys = [entry["key"] for entry in affected]
    if len(keys) != len(set(keys)):
        raise ValueError("Duplicate affected storage entries")
    return proposal, affected


async def verify(proposal, affected, call_args, signatories, threshold):
    import bittensor as bt

    async with bt.Subtensor(
        "finney", fallback_endpoints=[], archive_endpoints=[]
    ) as client:
        batch = call_args["call"]["Utility"]["batch_all"]["calls"]
        inner_calls = [await client.compose(sdk_call(value)) for value in batch]
        batch_call = await client.compose(bt.calls.Utility.batch_all(calls=inner_calls))
        sudo_key = str(await client.query(bt.storage.Sudo.Key))
        multisig = await client.multisig(signatories, threshold, account_mode="legacy")
        if multisig.address != sudo_key:
            raise ValueError("Signer set does not derive the live sudo key")
        if str(proposal.get("sudo_key")) != sudo_key:
            raise ValueError("Proposal sudo key differs from the live sudo key")
        call = await client.compose(bt.calls.Sudo.sudo(call=batch_call))
        actual = "0x" + bytes(call.call_hash).hex()
        if actual != proposal["call_hash"]:
            raise ValueError(
                f"Recomposed call hash {actual} differs from the reviewed hash"
            )
        for entry in affected:
            live = await client.query(tuple(entry["storage"]))
            if normalize(live) != normalize(entry["live"]):
                raise ValueError(
                    f"Live value changed for {entry['key']}; regenerate the proposal"
                )
        print(
            "Verified live sudo key, independently supplied call hash, and every affected old value."
        )
        print("Call hash:", actual)
        return call


def btcli_command(call_args_path, signatories, threshold, wallet):
    return [
        "btcli",
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
    ]


def main(argv=None):
    args = arguments(argv)
    wallet = os.environ.get("SUDO_WALLET")
    if not wallet or wallet.startswith("<"):
        raise ValueError("Set SUDO_WALLET to your own local triumvirate member wallet")
    proposal, affected = load_proposal(args.proposal.resolve(), args.expected_call_hash)
    config_path = (
        Path(__file__).resolve().parent.parent
        / ".github/scripts/deploy/sudo-signatories.json"
    )
    config = json.loads(config_path.read_text())
    signatories = config["signatories"]
    threshold = config["threshold"]
    if proposal.get("signers") is None:
        raise ValueError("Proposal has no signer configuration")
    if proposal["signers"]["threshold"] != threshold or set(
        proposal["signers"]["signatories"]
    ) != set(signatories):
        raise ValueError(
            "Proposal signers differ from the reviewed repository configuration"
        )
    call_args = proposal["btcli_args"]
    args_path = args.proposal.resolve().parent / "verified-mainnet-args.json"
    args_path.write_text(json.dumps(call_args, indent=2) + "\n")
    asyncio.run(verify(proposal, affected, call_args, signatories, threshold))
    print("Review the displayed batch, then approve in the standard btcli flow.")
    return subprocess.run(
        btcli_command(args_path, signatories, threshold, wallet)
    ).returncode


if __name__ == "__main__":
    try:
        sys.exit(main())
    except (KeyError, TypeError, ValueError, json.JSONDecodeError) as error:
        raise SystemExit(f"Refusing approval: {error}") from error
