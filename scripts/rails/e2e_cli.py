"""btcli, end to end, against the local rig.

Every command of `btcli bridge` and `btcli pool` runs once, with keys in a wallet
directory that this script makes and deletes: Alice's coldkey for the calls on Bittensor,
and the rig's test account as the wallet's EVM key for the transactions on the spoke.
"""

from __future__ import annotations

import asyncio
import json
import os
import sys
import tempfile
import time
from pathlib import Path

from rig import (
    HUB_RPC,
    HUB_WS,
    MANIFEST,
    MESSAGE_CEILING,
    MESSAGE_RESERVE,
    SPOKE_BRIDGE,
    SPOKE_NAME,
    TRANSPORT_FEE,
    USDC,
    USER,
    USER_KEY,
    Spoke,
    client,
    dev_wallet,
    manifest,
    submit,
)
from typer.testing import CliRunner

import bittensor as bt
from bittensor.cli.main import app
from bittensor.evm.keys import import_evm_key
from bittensor.wallet import Wallet

runner = CliRunner()
PASSWORD = "rails"
# btcli sends a swap through the MEV shield by default: encrypted to the key of the next
# block's author. The rig's blocks come four a second, faster than a client can follow
# that key, so here the swaps go in the clear.
CLEAR = ("--no-mev-shield",)


def show(step: str, detail: str = "") -> None:
    print(f"[e2e] btcli {step}" + (f": {detail}" if detail else ""), flush=True)


def btcli(*arguments: str) -> dict | list:
    """Runs one btcli command and returns the last JSON document it printed.

    A command that sends several transactions prints a document for each one.
    """
    result = runner.invoke(app, ["--json", *arguments])
    assert result.exit_code == 0, f"btcli {' '.join(arguments)}\n{result.output}"
    decoder, text, document = json.JSONDecoder(), result.output.strip(), None
    while text:
        try:
            document, end = decoder.raw_decode(text)
        except json.JSONDecodeError as error:
            raise AssertionError(f"btcli {' '.join(arguments)} printed:\n{text}") from error
        text = text[end:].lstrip()
    assert document is not None, f"btcli {' '.join(arguments)} printed nothing"
    return document


def make_wallet(directory: Path) -> None:
    """A wallet called alice: Alice's keys, and the rig's test account as its EVM key."""
    alice = dev_wallet()
    wallet = Wallet(name="alice", hotkey="default", path=str(directory))
    wallet.coldkey_file.set_keypair(alice.coldkey, encrypt=False, overwrite=True)
    wallet.coldkeypub_file.set_keypair(alice.coldkey, encrypt=False, overwrite=True)
    wallet.hotkey_file.set_keypair(alice.hotkey, encrypt=False, overwrite=True)
    import_evm_key("default", "alice", str(directory), private_key=USER_KEY, password=PASSWORD)
    (directory / "password").write_text(PASSWORD)
    for variable, value in {
        "BT_NETWORK": HUB_WS,
        "BT_FALLBACK_ENDPOINTS": "none",
        "BT_ARCHIVE_ENDPOINTS": "none",
        "BT_WALLET": "alice",
        "BT_WALLET_HOTKEY": "default",
        "BT_WALLET_PATH": str(directory),
        "BT_WALLET_PASSWORD_FILE": str(directory / "password"),
        "BT_RAILS_MANIFEST": str(MANIFEST),
        "BTCLI_CONFIG": str(directory / "btcli.json"),
        "BTCLI_PROXIES_PATH": str(directory / "proxies.json"),
        "BTCLI_ADDRESSES_PATH": str(directory / "addresses.json"),
        "BTCLI_MULTISIGS_PATH": str(directory / "multisigs.json"),
        "BTCLI_MULTISIG_CACHE": str(directory / "multisig_cache.json"),
        "BTCLI_SUBNET_NAMES_CACHE": str(directory / "subnet_names.json"),
        "BTCLI_TOKEN_SYMBOLS_CACHE": str(directory / "token_symbols.json"),
        # A program reads the JSON here, not a person: no terminal styling in it, whatever
        # the terminal that runs this is.
        "TERM": "dumb",
    }.items():
        os.environ[variable] = value
    os.environ.pop("FORCE_COLOR", None)


def wait_for_order(order: int) -> dict:
    """Follows an order with `btcli bridge status` until the spoke has paid."""
    deadline = time.monotonic() + 180
    while True:
        record = btcli("bridge", "status", "--bridge", str(SPOKE_BRIDGE), "--order", str(order))
        if record["delivered"]:
            return record
        assert time.monotonic() < deadline, f"order {order} was not paid: {record}"
        time.sleep(1)


def listing() -> dict:
    bridges = btcli("bridge", "list")
    (bridge,) = [row for row in bridges if row["bridge"] == SPOKE_BRIDGE]
    assert bridge["enabled"], bridge
    tokens = btcli("bridge", "tokens")
    symbols = {(row["asset"], row["netuid"]): row["symbol"] for row in tokens}
    assert symbols[("tao", None)] == "TAO"
    market = manifest()["markets"][0]
    assert symbols[("alpha", market["netuid"])] == market["symbol"]
    show("bridge list, bridge tokens", ", ".join(sorted(symbols.values())))
    return market


def deposit_and_sell(spoke: Spoke, market: dict) -> None:
    netuid, token = market["netuid"], market["token"]
    before = spoke.balance(token, USER)
    sent = btcli(
        "bridge", "deposit", "--amount", "20", "--receive", "alpha",
        "--netuid", str(netuid), "--spoke", SPOKE_NAME, "--yes",
    )  # fmt: skip
    record = wait_for_order(sent["order"])
    assert record["status"] == "settled", record
    bought = spoke.balance(token, USER) - before
    assert bought > 0
    show("bridge deposit, bridge status", f"20 USDC -> {bought / 1e9:.6f} {market['symbol']}")

    stable = spoke.balance(spoke.stable, USER)
    sent = btcli(
        "bridge", "sell", "--token", "alpha", "--netuid", str(netuid),
        "--amount", f"{bought / 1e9:.9f}", "--spoke", SPOKE_NAME, "--yes",
    )  # fmt: skip
    record = wait_for_order(sent["order"])
    assert record["status"] == "settled", record
    returned = spoke.balance(spoke.stable, USER) - stable
    assert returned > 19 * USDC
    show("bridge sell", f"{bought / 1e9:.6f} {market['symbol']} -> {returned / USDC:.6f} USDC")


def pool() -> None:
    alice = dev_wallet().coldkey.ss58_address
    quote = btcli("pool", "quote", "--sell", "tao", "--amount", "2")
    result = btcli("pool", "swap", "--sell", "tao", "--amount", "2", "--yes", *CLEAR)
    assert result["success"], result
    held = float(btcli("bridge", "balance", "--coldkey", alice)["tusd"])
    assert held > 0
    show("pool quote, pool swap", f"2 TAO -> {quote['receive']} tUSD quoted, {held} held")

    shares = btcli("pool", "show", "--coldkey", alice)["shares"]["shares"]
    result = btcli("pool", "add", "--amount-tao", "1", "--amount-tusd", "400", "--yes")
    assert result["success"], result
    added = btcli("pool", "show", "--coldkey", alice)["shares"]["shares"] - shares
    assert added > 0
    result = btcli("pool", "remove", "--shares", str(added), "--yes")
    assert result["success"], result
    assert btcli("pool", "show", "--coldkey", alice)["shares"]["shares"] == shares
    show("pool add, pool remove", f"{added} shares added and removed")


def exit_and_transfer(spoke: Spoke) -> None:
    bob = dev_wallet("//Bob", "//Bob//hot").coldkey.ss58_address
    result = btcli("bridge", "transfer", "--dest", bob, "--amount-tusd", "10", "--yes")
    assert result["success"], result
    assert float(btcli("bridge", "balance", "--coldkey", bob)["tusd"]) >= 10

    stable = spoke.balance(spoke.stable, USER)
    result = btcli(
        "bridge", "exit", "--bridge", str(SPOKE_BRIDGE), "--pay-with", "tusd",
        "--amount", "100", "--receive", "stable", "--to-address", USER, "--yes", *CLEAR,
    )  # fmt: skip
    assert result["success"], result
    # The exit waits in the queue of its bridge. Anyone may send the queue: here, the
    # wallet's EVM key. (The rig's keeper does the same, so this may find the queue empty.)
    # What it offers towards a charge above the bridge's reserve is not needed here, and
    # comes back.
    btcli(
        "bridge", "flush", "--bridge", str(SPOKE_BRIDGE), "--pay-up-to", "0.001",
        "--rpc-url", HUB_RPC, "--yes",
    )  # fmt: skip
    deadline = time.monotonic() + 180
    while spoke.balance(spoke.stable, USER) == stable:
        assert time.monotonic() < deadline, "the exit did not arrive on the spoke"
        time.sleep(1)
    arrived = spoke.balance(spoke.stable, USER) - stable
    assert arrived == 100 * USDC
    # The vault's allowance covered the payment, so the vault owes nothing more.
    owed = btcli("bridge", "claim", "--to", USER, "--yes")
    assert owed["owed"] == 0 and owed["tx_hash"] is None, owed
    show(
        "bridge transfer, bridge exit, bridge flush, bridge claim",
        f"100 tUSD -> {arrived / USDC:.2f} USDC on the spoke, and nothing left owed",
    )


def set_message_terms(reserve: int, ceiling: int) -> None:
    """Root sets what the hub pays the transport for one message to the spoke."""

    async def run() -> None:
        call = bt.calls.Rails.set_message_reserve(
            bridge=SPOKE_BRIDGE, reserve=reserve, ceiling=ceiling
        )
        async with client() as chain:
            await submit(chain, call, dev_wallet(), root=True)

    asyncio.run(run())


def deferred_messages() -> list[int]:
    (bridge,) = [b for b in btcli("bridge", "list") if b["bridge"] == SPOKE_BRIDGE]
    return bridge["messages_deferred"]


def send_a_deferred_message(spoke: Spoke) -> None:
    """The hub pays half of what the transport charges, so the message of an exit is
    deferred. The wallet's EVM key sends it and pays the difference."""
    stable = spoke.balance(spoke.stable, USER)
    set_message_terms(TRANSPORT_FEE // 2, TRANSPORT_FEE // 2)
    try:
        result = btcli(
            "bridge", "exit", "--bridge", str(SPOKE_BRIDGE), "--pay-with", "tusd",
            "--amount", "10", "--receive", "stable", "--to-address", USER, "--yes", *CLEAR,
        )  # fmt: skip
        assert result["success"], result
        # The rig's keeper asks the gateway to send the message. It is not paid for.
        deadline = time.monotonic() + 180
        while not (waiting := deferred_messages()):
            assert time.monotonic() < deadline, "the exit's message was not deferred"
            time.sleep(1)
        (message,) = waiting
        btcli(
            "bridge", "flush", "--bridge", str(SPOKE_BRIDGE), "--message", str(message),
            "--pay-up-to", "0.001", "--rpc-url", HUB_RPC, "--yes",
        )  # fmt: skip
        while spoke.balance(spoke.stable, USER) == stable:
            assert time.monotonic() < deadline, "the deferred message did not arrive"
            time.sleep(1)
        assert spoke.balance(spoke.stable, USER) - stable == 10 * USDC
        assert not deferred_messages()
    finally:
        set_message_terms(MESSAGE_RESERVE, MESSAGE_CEILING)
    show(
        "bridge flush --message",
        f"message {message} of the hub was deferred, then sent with the difference paid",
    )


def main() -> None:
    (addresses,) = manifest()["spokes"].values()
    spoke = Spoke(addresses)
    spoke.mint_stable(USER_KEY, USER, 100 * USDC)
    with tempfile.TemporaryDirectory() as directory:
        make_wallet(Path(directory))
        market = listing()
        deposit_and_sell(spoke, market)
        pool()
        exit_and_transfer(spoke)
        send_a_deferred_message(spoke)
    print("[e2e] btcli passed", flush=True)


if __name__ == "__main__":
    try:
        main()
    except AssertionError as error:
        sys.exit(f"[e2e] btcli FAILED: {error}")
