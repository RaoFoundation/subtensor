"""What the rig's Python scripts share: the rig's settings, its keys, and the few calls they
make on the two chains.

The scripts run in the SDK's environment (``sdk/python``) and use the SDK for everything
on Bittensor. On the spoke they sign with anvil's development keys.
"""

from __future__ import annotations

import asyncio
import json
import os
import time
from pathlib import Path
from types import SimpleNamespace
from typing import Any, Awaitable, Callable, Optional

from eth_abi import decode as abi_decode
from eth_abi import encode as abi_encode
from eth_account import Account
from eth_utils import keccak

import bittensor as bt
from bittensor import rails
from bittensor.evm.addresses import h160_to_ss58, pubkey_to_ss58
from bittensor.evm.rpc import EvmRpc
from bittensor.evm.transactions import prepare_transaction, send_transaction
from bittensor.keyfiles import Keypair

REPO = Path(__file__).resolve().parents[2]
DEPLOYMENTS = REPO / "contracts" / "evm" / "deployments"


def setting(name: str) -> str:
    """A setting of the rig. scripts/rails/env.sh is the one place that defines them."""
    try:
        return os.environ[name]
    except KeyError:
        raise SystemExit(
            f"{name} is not set: run this through `just rails-up` or `just rails-e2e`, "
            "or source scripts/rails/env.sh first"
        ) from None


STATE = Path(setting("RAILS_STATE"))
MANIFEST = Path(setting("RAILS_MANIFEST"))
HUB_WS = setting("HUB_WS")
HUB_RPC = setting("HUB_RPC")
HUB_CHAIN_ID = int(setting("HUB_CHAIN_ID"))
SPOKE_NAME = setting("SPOKE_NAME")
SPOKE_RPC = setting("SPOKE_RPC")
SPOKE_BRIDGE = int(setting("SPOKE_BRIDGE"))
SPOKE_DOMAIN = int(setting("SPOKE_DOMAIN"))
SPOKE_CHAIN_ID = int(setting("SPOKE_CHAIN_ID"))

# Anvil's development accounts.
DEPLOYER_KEY = setting("DEPLOYER_KEY")
DEPLOYER = setting("DEPLOYER")
VALIDATOR = setting("VALIDATOR")
RELAYER = setting("RELAYER")
KEEPER_KEY = setting("KEEPER_KEY")
KEEPER = setting("KEEPER")
GUARDIAN_KEY = setting("GUARDIAN_KEY")
MULTISIG_KEY = setting("MULTISIG_KEY")
USER_KEY = setting("USER_KEY")
USER = setting("USER_ADDRESS")
MULTISIG = setting("MULTISIG")
# What an order pays on the spoke for its answer (wei), and an exit on the hub (rao).
ANSWER_FEE = int(setting("ANSWER_FEE"))
EXIT_FEE = int(setting("EXIT_FEE"))

UNIT = 10**9
USDC = 10**6
# The TAO price the rig's oracle feeds, in dollars.
TAO_PRICE = 400

# The subnets the rig creates, by the name each publishes on chain.
SUBNET_NAMES = ("Chutes", "lium.io")


def dev_wallet(cold: str = "//Alice", hot: str = "//Alice//hot") -> SimpleNamespace:
    """A wallet of development keys, as the SDK's tests use one."""
    return SimpleNamespace(
        coldkey=Keypair.create_from_uri(cold),
        coldkeypub=Keypair(ss58_address=Keypair.create_from_uri(cold).ss58_address),
        hotkey=Keypair.create_from_uri(hot),
    )


# The validators the rig puts on each of its subnets.
VALIDATORS = 4
# Where the rig records the subnets whose validators the keeper keeps active.
VALIDATED = STATE / "validators.json"


def miner_wallet(index: int) -> SimpleNamespace:
    """The miner of the rig's subnet number `index`."""
    return dev_wallet("//Alice", f"//Alice//rails//{index}//miner")


def validator_wallets(index: int) -> list[SimpleNamespace]:
    """The validators of the rig's subnet number `index`."""
    return [
        dev_wallet("//Alice", f"//Alice//rails//{index}//validator//{number}")
        for number in range(VALIDATORS)
    ]


async def score_miner(chain: bt.Client, index: int, netuid: int, miner_uid: int) -> None:
    """Each validator of the subnet gives all of its weight to the subnet's miner."""
    for wallet in validator_wallets(index):
        result = await chain.execute(
            bt.intents.SetWeights(netuid=netuid, weights={miner_uid: 1}), wallet
        )
        if not result.success:
            raise RuntimeError(f"weights on subnet {netuid}: {result.message}")


def oracle_wallet() -> SimpleNamespace:
    """The account that feeds the rig's TAO price."""
    return dev_wallet("//Rails//Oracle", "//Rails//Oracle//hot")


def client() -> bt.Client:
    return bt.Client(HUB_WS, fallback_endpoints=[], archive_endpoints=[])


async def submit(chain: bt.Client, call, wallet, *, root: bool = False):
    """Submits `call`, through sudo if `root`, and fails loudly if the chain refuses it."""
    if root:
        call = bt.calls.Sudo.sudo(call=await chain.compose(call))
    result = await chain.submit_call(call, wallet)
    if not result.success:
        raise RuntimeError(f"{call}: {result.message}")
    return result


async def wait_for(
    what: str,
    check: Callable[[], Awaitable[Any]],
    *,
    timeout: float = 180,
    every: float = 1.0,
) -> Any:
    """Waits until `check` returns something true, and returns it."""
    deadline = time.monotonic() + timeout
    while True:
        value = await check()
        if value:
            return value
        if time.monotonic() > deadline:
            raise TimeoutError(f"timed out waiting for {what}")
        await asyncio.sleep(every)


def mirror(address: str) -> str:
    """The account on Bittensor that an EVM address acts as."""
    return h160_to_ss58(address)


def sub_account(pallet_id: bytes, sub: bytes) -> str:
    """The keyless account a pallet derives for `sub` (`into_sub_account_truncating`)."""
    return pubkey_to_ss58((b"modl" + pallet_id + sub).ljust(32, b"\0")[:32])


def vault_account(netuid: int) -> str:
    """The hub's account that stakes on `netuid`."""
    return sub_account(b"bt/rails", b"vlt/" + netuid.to_bytes(2, "little"))


def index_hotkey(netuid: int) -> str:
    """The keyless hotkey through which the hub's alpha on `netuid` is staked."""
    return sub_account(b"subtensr", b"idx/" + netuid.to_bytes(2, "little"))


# The hub's account that holds the TAO behind all canonical TAO.
RESERVE_ACCOUNT = sub_account(b"bt/rails", b"tao/")


def load_json(path: Path) -> dict:
    return json.loads(path.read_text())


def manifest() -> dict:
    return load_json(MANIFEST)


# ---------------------------------------------------------------- EVM calls


def calldata(signature: str, types: list[str], values: list) -> str:
    """The data of a call to the function `signature`."""
    return "0x" + (keccak(text=signature)[:4] + abi_encode(types, values)).hex()


def send(rpc: EvmRpc, key: str, to: str, data: str, value: int = 0) -> dict:
    """Signs and sends a transaction, waits for it, and returns its receipt."""
    account = Account.from_key(key)
    preview = prepare_transaction(rpc, account.address, to, value_wei=value, data=data)
    result = send_transaction(rpc, account, preview)
    if not result.get("success"):
        raise RuntimeError(f"transaction to {to} reverted: {result}")
    return rpc.call("eth_getTransactionReceipt", [result["tx_hash"]])


# ---------------------------------------------------------------- the spoke


class Spoke:
    """Anvil playing Base: the rails' contracts there, and a key to sign with."""

    def __init__(self, addresses: dict):
        self.rpc = EvmRpc(SPOKE_RPC)
        self.vault = addresses["vault"]
        self.portal = addresses["portal"]
        self.stable = addresses["stable"]
        self.tao = addresses["tao"]

    def call(self, to: str, signature: str, types: list[str], values: list) -> str:
        return self.rpc.eth_call({"to": to, "data": calldata(signature, types, values)})

    def send(self, key: str, to: str, data: str, value: int = 0) -> dict:
        return send(self.rpc, key, to, data, value)

    def next_order(self) -> int:
        """The number the portal gives its next order."""
        return rails.decode_uint(self.call(self.portal, "nextOrder()", [], []))

    def order_fee(self) -> int:
        """What an order costs now on top of what it pays with, in wei."""
        return rails.decode_uint(
            self.rpc.eth_call({"to": self.portal, "data": rails.encode_order_fee()})
        )

    def native_balance(self, holder: str) -> int:
        return int(self.rpc.call("eth_getBalance", [holder, "latest"]), 16)

    def balance(self, token: str, holder: str) -> int:
        return rails.decode_uint(
            self.rpc.eth_call({"to": token, "data": rails.encode_balance_of(holder)})
        )

    def alpha_token(self, netuid: int) -> str:
        return rails.decode_address(
            self.rpc.eth_call({"to": self.portal, "data": rails.encode_alpha_token(netuid)})
        )

    def total_supply(self, token: str) -> int:
        return rails.decode_uint(self.call(token, "totalSupply()", [], []))

    def convert_to_assets(self, token: str, shares: int) -> int:
        """The alpha that `shares` of an alpha token are worth at the rate the token knows."""
        return rails.decode_uint(
            self.call(token, "convertToAssets(uint256)", ["uint256"], [shares])
        )

    def symbol(self, token: str) -> str:
        (symbol,) = abi_decode(["string"], bytes.fromhex(self.call(token, "symbol()", [], [])[2:]))
        return symbol

    def alpha_token_address(self, netuid: int) -> str:
        """Where the portal creates the alpha token of `netuid`."""
        return rails.decode_address(
            self.call(self.portal, "alphaTokenAddress(uint16)", ["uint16"], [netuid])
        )

    def mint_stable(self, key: str, to: str, amount: int) -> None:
        self.send(
            key,
            self.stable,
            calldata("mint(address,uint256)", ["address", "uint256"], [to, amount]),
        )

    def deposit(
        self,
        key: str,
        amount: int,
        destination: rails.Destination,
        *,
        min_out: int = 0,
        deadline: Optional[int] = None,
    ) -> int:
        """Deposits `amount` of the stablecoin for `destination`. Returns the order's nonce."""
        owner = Account.from_key(key).address
        allowance = rails.decode_uint(
            self.rpc.eth_call(
                {"to": self.stable, "data": rails.encode_allowance(owner, self.vault)}
            )
        )
        if allowance < amount:
            self.send(key, self.stable, rails.encode_approve(self.vault, 2**256 - 1))
        receipt = self.send(
            key,
            self.vault,
            rails.encode_deposit(
                amount, destination, min_out, deadline or int(time.time()) + 3_600
            ),
            value=self.order_fee(),
        )
        nonce = rails.order_nonce(receipt, self.portal)
        assert nonce is not None, "the deposit sent no order"
        return nonce

    def sell(
        self,
        key: str,
        token: str,
        amount: int,
        destination: rails.Destination,
        *,
        min_out: int = 0,
    ) -> int:
        """Sells `amount` of a canonical token for `destination`. Returns the order's nonce."""
        receipt = self.send(
            key,
            token,
            rails.encode_sell(amount, destination, min_out, int(time.time()) + 3_600),
            value=self.order_fee(),
        )
        nonce = rails.order_nonce(receipt, self.portal)
        assert nonce is not None, "the sale sent no order"
        return nonce

    def answers(self, order: int) -> list[dict]:
        """The portal's payments in answer to `order`."""
        logs = self.rpc.call("eth_getLogs", [rails.credited_filter(self.portal, order, 0)])
        return [answer for answer in map(rails.decode_credited, logs) if answer["answers_order"]]
