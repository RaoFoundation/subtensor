"""A minimal Ethereum JSON-RPC client over HTTP (stdlib only).

Subtensor nodes serve the standard ``eth_*`` methods; the handful the CLI
needs (balances, gas, nonce, raw submission, receipts) doesn't justify a
web3 dependency. Synchronous by design — every caller is a CLI command.
"""

from __future__ import annotations

import json
import time
import urllib.error
import urllib.request
from typing import Any

from ..balance import Balance
from ..settings import RAO_PER_TAO

# 1 TAO is 10**18 in EVM transaction values but 10**9 rao natively.
WEI_PER_TAO = 10**18
_WEI_PER_RAO = WEI_PER_TAO // RAO_PER_TAO


class EvmRpcError(Exception):
    """A JSON-RPC level error from the EVM endpoint (code + message)."""

    def __init__(self, code: int, message: str, data: Any = None):
        self.code = code
        self.data = data
        super().__init__(message)


def wei_to_balance(wei: int) -> Balance:
    """An EVM-side amount (18 decimals) as a TAO Balance (truncates sub-rao dust)."""
    return Balance.from_rao(int(wei) // _WEI_PER_RAO)


def balance_to_wei(balance: Balance) -> int:
    """A TAO Balance as an EVM transaction value (18 decimals)."""
    if balance.netuid != 0:
        raise ValueError("EVM values are TAO; got an alpha balance")
    return balance.rao * _WEI_PER_RAO


class EvmRpc:
    """One EVM endpoint, spoken to via plain JSON-RPC POSTs."""

    def __init__(self, url: str, *, timeout: float = 30.0):
        self.url = url
        self.timeout = timeout
        self._id = 0

    def call(self, method: str, params: "list | None" = None) -> Any:
        self._id += 1
        body = json.dumps(
            {"jsonrpc": "2.0", "id": self._id, "method": method, "params": params or []}
        ).encode()
        request = urllib.request.Request(
            self.url, data=body, headers={"Content-Type": "application/json"}
        )
        try:
            with urllib.request.urlopen(request, timeout=self.timeout) as response:
                payload = json.loads(response.read())
        except urllib.error.URLError as error:
            raise ConnectionError(f"could not reach EVM RPC {self.url}: {error}") from error
        if "error" in payload:
            err = payload["error"]
            raise EvmRpcError(err.get("code", -1), err.get("message", "unknown"), err.get("data"))
        return payload["result"]

    # Typed conveniences over the raw methods the commands use -----------------

    def chain_id(self) -> int:
        return int(self.call("eth_chainId"), 16)

    def block_number(self) -> int:
        return int(self.call("eth_blockNumber"), 16)

    def get_balance_wei(self, address: str) -> int:
        return int(self.call("eth_getBalance", [address, "latest"]), 16)

    def get_nonce(self, address: str) -> int:
        return int(self.call("eth_getTransactionCount", [address, "latest"]), 16)

    def gas_price(self) -> int:
        return int(self.call("eth_gasPrice"), 16)

    def estimate_gas(self, tx: dict) -> int:
        return int(self.call("eth_estimateGas", [tx]), 16)

    def eth_call(self, tx: dict) -> str:
        return self.call("eth_call", [tx, "latest"])

    def send_raw_transaction(self, raw: "str | bytes") -> str:
        data = raw if isinstance(raw, str) else "0x" + bytes(raw).hex()
        return self.call("eth_sendRawTransaction", [data])

    def wait_for_receipt(self, tx_hash: str, *, timeout: float = 120.0) -> dict:
        """Poll for a transaction receipt (subtensor blocks are ~12s apart)."""
        deadline = time.monotonic() + timeout
        while True:
            receipt = self.call("eth_getTransactionReceipt", [tx_hash])
            if receipt is not None:
                return receipt
            if time.monotonic() > deadline:
                raise TimeoutError(f"no receipt for {tx_hash} after {timeout:.0f}s")
            time.sleep(3.0)
