"""Public wallet presentation, separate from descriptor-preserving payments."""

from __future__ import annotations

import asyncio
import contextlib
import json
import subprocess
import sys
from pathlib import Path
from typing import Any

from .._transport import SubstrateConnection
from ..hashed import descriptor_bytes, descriptor_storage
from ..receiving import parse_recipient, receiving_address
from ..settings import resolve_endpoint
from ..sp_core import (
    REGISTERED_CRYPTO_TYPES,
    deserialize_keypair_from_keyfile_data,
    keyfile_data_is_encrypted,
)

REGISTRATION_TIMEOUT = 5.0


def public_key_from_file(path):
    """Read only public metadata, including legacy unencrypted hotkey files."""
    data = Path(path).read_bytes()
    if keyfile_data_is_encrypted(data):
        raise ValueError("encrypted key; public address unavailable")
    value = json.loads(data)
    if not isinstance(value, dict):
        raise ValueError("invalid public keyfile")
    if "ss58Address" not in value and " ss58Address" in value:
        value["ss58Address"] = value[" ss58Address"]
    # Never derive or unlock a signing key for display, even if an old public
    # companion accidentally contains secrets. Keep all identity validation fields.
    public = {
        key: value[key]
        for key in (
            "ss58Address",
            "accountId",
            "publicKey",
            "cryptoType",
            "walletVersion",
            "hashedDescriptor",
            "mldsaDescriptor",
            "accountType",
            "signingScheme",
            "encoded",
            "encoding",
        )
        if key in value
    }
    return deserialize_keypair_from_keyfile_data(json.dumps(public).encode())


def address_fields(public, *, network: str, role: str = "") -> dict[str, Any]:
    """Both public forms, with the complete receiving address always primary."""
    fields: dict[str, Any] = {"ss58": public.ss58_address}
    if public.crypto_type in REGISTERED_CRYPTO_TYPES:
        address = receiving_address(public)
        recipient = parse_recipient(address)
        if recipient.account != public.ss58_address:
            raise ValueError("wallet descriptor does not match its SS58 account")
        fields.update(
            address=address,  # Existing JSON consumers retain the complete descriptor.
            receiving_address=address,
            primary_address=address,
            registration="not_checked",
            registration_network=network,
            registration_block_hash=None,
        )
    return {f"{role}_{key}" if role else key: value for key, value in fields.items()}


async def query_registration(substrate, records: list[dict[str, Any]]) -> None:
    """Match permanent descriptors at one finalized snapshot, without caching."""
    candidates = [record for record in records if record.get("receiving_address")]
    for record in candidates:
        record.update(
            registration="unavailable",
            primary_address=record["receiving_address"],
            registration_block_hash=None,
        )
    if not candidates:
        return
    try:
        finalized = await substrate.get_block_hash(await substrate.get_finalized_block_number())
        if not finalized:
            return
    except Exception:
        return

    # Bound concurrent reads even when a directory contains many wallets. Duplicate
    # public companions share a read, but nothing survives this chain connection.
    groups: dict[str, list[dict[str, Any]]] = {}
    for record in candidates:
        groups.setdefault(record["receiving_address"], []).append(record)
    limit = asyncio.Semaphore(8)

    async def check(address, group):
        try:
            recipient = parse_recipient(address)
            if recipient.descriptor is None:
                return
            async with limit:
                stored = await substrate.query(
                    "HashedAccounts",
                    descriptor_storage(recipient.descriptor),
                    [recipient.account],
                    block_hash=finalized,
                )
            status = (
                "unregistered"
                if stored is None
                else "registered"
                if descriptor_bytes(stored["descriptor"]) == recipient.descriptor
                else "mismatch"
            )
            for record in group:
                if record["ss58"] != recipient.account:
                    continue
                record.update(
                    registration=status,
                    registration_block_hash=finalized,
                    primary_address=address,
                )
        except Exception:
            # A missing registry, malformed response or failed RPC is unknown,
            # never evidence that the account is registered (or unregistered).
            return

    await asyncio.gather(*(check(address, group) for address, group in groups.items()))


async def lookup_registration(network: str, records: list[dict[str, Any]]) -> None:
    """Own and close a connection for a single optional registration snapshot."""
    for record in records:
        record.update(
            registration="unavailable",
            registration_network=network,
            registration_block_hash=None,
            primary_address=record["receiving_address"],
        )
    _, endpoint = resolve_endpoint(network)
    substrate = SubstrateConnection(
        endpoint,
        fallback_urls=[],
        max_retries=1,
        response_timeout=REGISTRATION_TIMEOUT,
    )

    async def read():
        await substrate.initialize()
        await query_registration(substrate, records)

    try:
        await asyncio.wait_for(read(), timeout=REGISTRATION_TIMEOUT)
    finally:
        with contextlib.suppress(Exception):
            await asyncio.wait_for(substrate.close(), timeout=1.0)


def check_registration(app_ctx, records: list[dict[str, Any]], *, offline: bool = False) -> None:
    """Best-effort display lookup, with a hard deadline even for stalled DNS."""
    candidates = [record for record in records if record.get("receiving_address")]
    for record in candidates:
        record.update(
            registration="not_checked" if offline else "unavailable",
            registration_network=app_ctx.network,
            registration_block_hash=None,
            primary_address=record["receiving_address"],
        )
    if offline or not candidates:
        return

    # OS DNS resolution runs in Python's executor and cannot always be cancelled.
    # asyncio.run waits for those threads at exit, beyond the coroutine timeout.
    # Isolate this cosmetic lookup so timeout kills/reaps all of its work. Send
    # only public identities on stdin; no keyfiles, secrets, or signing operations.
    identities = [
        {"ss58": record["ss58"], "receiving_address": record["receiving_address"]}
        for record in candidates
    ]
    try:
        with app_ctx.output.activity(f"checking wallet registration on {app_ctx.network}…"):
            result = subprocess.run(
                [sys.executable, "-m", "bittensor.cli.wallet_addresses"],
                input=json.dumps({"network": app_ctx.network, "records": identities}),
                text=True,
                stdout=subprocess.PIPE,
                stderr=subprocess.DEVNULL,
                timeout=REGISTRATION_TIMEOUT + 1.0,
                check=True,
            )
        checked = json.loads(result.stdout)
        if len(checked) != len(candidates):
            return
        for record, identity, status in zip(candidates, identities, checked):
            if any(status.get(key) != value for key, value in identity.items()):
                continue
            # Registration is informational; it must never change the address to share.
            record.update(
                {
                    key: status[key]
                    for key in (
                        "registration",
                        "registration_network",
                        "registration_block_hash",
                    )
                }
            )
    except Exception:
        # Listing and inspecting local public keys must still work offline.
        pass


if __name__ == "__main__":
    request = json.load(sys.stdin)
    with contextlib.suppress(Exception):
        asyncio.run(lookup_registration(request["network"], request["records"]))
    json.dump(request["records"], sys.stdout)
