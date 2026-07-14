"""An in-memory :class:`bittensor.Substrate` implementation.

The SDK's chain-access seam is the ``Substrate`` protocol
(``bittensor/_substrate.py``): the executor, intents, reads, and namespaces
are all written against it, never against a websocket. ``FakeSubstrate``
implements the whole protocol over plain dicts, so ``Client``, plan/execute,
policy enforcement, and every registered read can be exercised with no
network and no chain.

Storage model:

- ``seed(module, item, params, value)`` pins one storage entry;
- ``seed_default(module, item, value)`` answers any key of that item;
- unseeded items fall back to ``DEFAULT_STORAGE`` (permissive values chosen so
  intent pre-flight checks pass: the hotkey is registered, no rate limits,
  commit-reveal off), then to ``None`` — exactly what a miss decodes to.

Writes are recorded, never applied: ``submit`` appends to ``submissions`` and
returns the next queued :class:`ExtrinsicResult` (default: success). Chain
state does not change on submit; tests assert on the composed call instead.
"""

from __future__ import annotations

from collections import defaultdict
from hashlib import blake2b
from typing import Any, AsyncIterator, Optional

from bittensor._transport import codec as codec_mod
from bittensor._transport.contract import MultisigAccount
from bittensor.balance import Balance
from bittensor.result import ExtrinsicResult

# Value returned for storage items not seeded by the test. Keyed by
# (module, item); chosen so intent build/warnings pre-flight checks pass.
DEFAULT_STORAGE: dict[tuple[str, str], Any] = {
    ("SubtensorModule", "Uids"): 0,  # the hotkey is registered (uid 0)
    ("SubtensorModule", "CommitRevealWeightsEnabled"): False,
    ("SubtensorModule", "WeightsSetRateLimit"): 0,  # no rate limit
    ("SubtensorModule", "LastUpdate"): 0,
    ("SubtensorModule", "MinAllowedWeights"): 0,
    ("SubtensorModule", "MaxWeightsLimit"): 65535,
    ("SubtensorModule", "Tempo"): 360,
    ("SubtensorModule", "RevealPeriodEpochs"): 1,
    ("SubtensorModule", "Delegates"): 0,
    ("SubtensorModule", "TxRateLimit"): 1000,
    ("SubtensorModule", "NetworksAdded"): True,
    ("SubtensorModule", "BlocksSinceLastStep"): 0,
    ("SubtensorModule", "Burn"): 10**9,
    ("SubtensorModule", "Difficulty"): 10**7,
    ("SubtensorModule", "ImmunityPeriod"): 4096,
    ("SubtensorModule", "SubnetworkN"): 0,
    ("SubtensorModule", "NetworkRegisteredAt"): 0,
    ("SubtensorModule", "SubnetEmissionEnabled"): True,
    ("System", "Account"): {"data": {"free": 0, "reserved": 0, "frozen": 0}},
    ("Timestamp", "Now"): 1_700_000_000_000,
}

DEFAULT_CONSTANTS: dict[tuple[str, str], Any] = {
    ("Aura", "SlotDuration"): 12_000,
    ("Balances", "ExistentialDeposit"): 500,
    ("SubtensorModule", "InitialStartCallDelay"): 100,
}

# Runtime-API results answered when the test seeds nothing. Chosen so intent
# builds that consult chain state (e.g. the alpha price backing the default
# slippage-protection limit) work offline.
DEFAULT_RUNTIME: dict[tuple[str, str], Any] = {
    ("SwapRuntimeApi", "current_alpha_price"): 10**9,  # 1 TAO per alpha
}

GENESIS_HASH = "0x" + "00" * 32


def _key(params: Optional[list]) -> tuple:
    return tuple(params or [])


class ComposedCall:
    """What ``FakeSubstrate.compose`` returns: the generated ``Call``'s fields
    plus the attributes intents read off a production composed call (a
    scalecodec ``GenericCall``): ``call_hash`` and ``data``. Unpacks like
    the 3-tuple so structural assertions stay simple."""

    def __init__(self, module: str, function: str, params: dict):
        self.module = module
        self.function = function
        self.params = params

    def __iter__(self):
        return iter((self.module, self.function, self.params))

    @property
    def call_hash(self) -> bytes:
        return blake2b(
            repr((self.module, self.function, self.params)).encode(), digest_size=32
        ).digest()

    @property
    def data(self) -> bytes:
        # Stand-in for the SCALE-encoded call bytes; deterministic in the call.
        return repr((self.module, self.function, self.params)).encode()

    def __repr__(self) -> str:
        return f"ComposedCall({self.module}.{self.function}, {self.params})"

    def __eq__(self, other) -> bool:
        return isinstance(other, ComposedCall) and tuple(self) == tuple(other)


def success_result(block: int = 100) -> ExtrinsicResult:
    return ExtrinsicResult(
        success=True,
        message="Success",
        block_hash=f"0x{block:064x}",
        extrinsic_id=f"{block}-0001",
        fee=Balance.from_rao(124_414),
        events=[],
    )


class FakeSubstrate:
    """See module docstring. Satisfies the ``bittensor.Substrate`` protocol."""

    def __init__(self) -> None:
        self.token_symbols: dict[int, str] = {}
        self.connected = False
        self.closed = False
        self.block = 100

        # (module, item) -> {params-tuple: value}
        self._storage: dict[tuple[str, str], dict[tuple, Any]] = defaultdict(dict)
        # (module, item) -> value answering any key
        self._storage_defaults: dict[tuple[str, str], Any] = {}
        # (module, item) -> [(key, value)] for query_map
        self._maps: dict[tuple[str, str], list[tuple[Any, Any]]] = {}
        self._constants: dict[tuple[str, str], Any] = dict(DEFAULT_CONSTANTS)
        # (api, method) -> value, or callable(params) -> value
        self._runtime: dict[tuple[str, str], Any] = dict(DEFAULT_RUNTIME)

        self.fee = Balance.from_rao(124_414)
        self.weight = {"ref_time": 1_000_000, "proof_size": 3_593}
        self.mev_key: Optional[bytes] = None
        self._nonces: dict[str, int] = defaultdict(int)

        # Every submitted (composed_call, signer_ss58, options) in order.
        self.submissions: list[tuple[Any, str, dict]] = []
        # Results handed out by submit/submit_signed/submit_multisig, FIFO;
        # empty -> a fresh success result.
        self.pending_results: list[ExtrinsicResult] = []

    # -- seeding -------------------------------------------------------------

    def seed(self, module: str, item: str, params: Optional[list], value: Any) -> None:
        self._storage[(module, item)][_key(params)] = value

    def seed_default(self, module: str, item: str, value: Any) -> None:
        self._storage_defaults[(module, item)] = value

    def seed_map(self, module: str, item: str, pairs: list[tuple[Any, Any]]) -> None:
        self._maps[(module, item)] = pairs

    def seed_constant(self, module: str, name: str, value: Any) -> None:
        self._constants[(module, name)] = value

    def seed_runtime(self, api: str, method: str, value: Any) -> None:
        """``value`` may be a plain result or a callable of the params list."""
        self._runtime[(api, method)] = value

    def queue_result(self, result: ExtrinsicResult) -> None:
        self.pending_results.append(result)

    @property
    def last_call(self) -> Any:
        return self.submissions[-1][0]

    # -- display metadata ------------------------------------------------------

    def balance(self, rao: int, netuid: int = 0) -> Balance:
        symbol = self.token_symbols.get(netuid) if netuid else None
        return Balance(int(rao), netuid, symbol)

    # -- lifecycle -------------------------------------------------------------

    async def connect(self) -> None:
        self.connected = True

    async def close(self) -> None:
        self.closed = True

    # -- reads -----------------------------------------------------------------

    async def block_hash(self, block: Optional[int] = None) -> str:
        return f"0x{(self.block if block is None else block):064x}"

    async def block_number(self) -> int:
        return self.block

    async def block_time(self) -> float:
        # Same derivation as production: the chain's Aura.SlotDuration constant.
        return int(await self.constant("Aura", "SlotDuration")) / 1000.0

    async def get_block(
        self, block_number: Optional[int] = None, block_hash: Optional[str] = None
    ) -> Optional[dict]:
        number = self.block if block_number is None else block_number
        ms = await self.query("Timestamp", "Now")
        return {
            "header": {"number": number, "hash": await self.block_hash(number)},
            "extrinsics": [{"call": {"call_module": "Timestamp", "call_args": [{"value": ms}]}}],
        }

    async def query(
        self,
        module: str,
        storage_function: str,
        params: Optional[list] = None,
        block_hash: Optional[str] = None,
    ) -> Any:
        item = (module, storage_function)
        entries = self._storage.get(item, {})
        key = _key(params)
        if key in entries:
            return entries[key]
        if item in self._storage_defaults:
            return self._storage_defaults[item]
        return DEFAULT_STORAGE.get(item)

    async def query_map(
        self,
        module: str,
        storage_function: str,
        params: Optional[list] = None,
        block_hash: Optional[str] = None,
    ) -> list[tuple[Any, Any]]:
        item = (module, storage_function)
        if item in self._maps:
            return list(self._maps[item])
        # Seeded point entries double as map contents (single-key maps only).
        return [
            (key[0] if len(key) == 1 else key, value)
            for key, value in self._storage.get(item, {}).items()
        ]

    async def query_batch(
        self,
        module: str,
        storage_function: str,
        param_sets: list[list],
        block_hash: Optional[str] = None,
    ) -> list[Any]:
        return [await self.query(module, storage_function, list(p)) for p in param_sets]

    async def runtime_call(
        self, api: str, method: str, params: list, block_hash: Optional[str] = None
    ) -> Any:
        value = self._runtime.get((api, method))
        return value(params) if callable(value) else value

    async def constant(self, module: str, name: str) -> Any:
        return self._constants.get((module, name))

    async def decode_scale(self, type_string: str, data: Any) -> Any:
        raise NotImplementedError(
            "FakeSubstrate has no runtime metadata; seed decode results explicitly "
            "or test decoding against the golden codec fixtures."
        )

    async def blocks(self, *, finalized: bool = False) -> AsyncIterator[dict]:
        # A short, terminating stream: three consecutive heads.
        for offset in range(3):
            number = self.block + offset
            yield {"header": {"number": number, "parentHash": f"0x{number - 1:064x}"}}

    # -- calls and fees ----------------------------------------------------------

    async def compose(self, call) -> ComposedCall:
        if isinstance(call, ComposedCall):
            return call
        module, function, params = call
        return ComposedCall(module, function, params)

    async def estimate_fee(self, call, keypair) -> Balance:
        return self.fee

    async def estimate_weight(self, call, keypair) -> dict:
        return dict(self.weight)

    async def account_next_index(self, address: str) -> int:
        return self._nonces[address]

    # -- writes -------------------------------------------------------------------

    async def mev_next_key(self) -> Optional[bytes]:
        return self.mev_key

    async def sign_extrinsic(self, call, keypair, *, nonce: int, period: int) -> tuple[bytes, str]:
        payload = repr((call, keypair.ss58_address, nonce)).encode()
        return payload, "0x" + payload.hex()[:64].ljust(64, "0")

    def _next_result(self) -> ExtrinsicResult:
        if self.pending_results:
            return self.pending_results.pop(0)
        return success_result(self.block)

    async def submit(
        self,
        call,
        keypair,
        *,
        nonce: Optional[int] = None,
        period: Optional[int] = None,
        wait_for_inclusion: bool = True,
        wait_for_finalization: bool = True,
    ) -> ExtrinsicResult:
        self.submissions.append((call, keypair.ss58_address, {"nonce": nonce, "period": period}))
        self._nonces[keypair.ss58_address] += 1
        self.block += 1
        return self._next_result()

    async def submit_signed(
        self,
        extrinsic,
        keypair,
        *,
        wait_for_inclusion: bool = True,
        wait_for_finalization: bool = True,
    ) -> ExtrinsicResult:
        self.submissions.append((extrinsic, keypair.ss58_address, {"signed": True}))
        self.block += 1
        return self._next_result()

    # -- multisig -------------------------------------------------------------------

    def multisig_account(self, signatories: list[str], threshold: int) -> MultisigAccount:
        # Real derivation — pure local crypto, no chain involved.
        return codec_mod.multisig_account(signatories, threshold)

    async def submit_multisig(
        self,
        call,
        keypair,
        multisig_account: MultisigAccount,
        *,
        wait_for_inclusion: bool = True,
        wait_for_finalization: bool = True,
    ) -> ExtrinsicResult:
        self.submissions.append(
            (call, keypair.ss58_address, {"multisig": multisig_account.ss58_address})
        )
        self.block += 1
        return self._next_result()
