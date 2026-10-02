"""The public CPU miner and registration intent preserve the runtime challenge."""

import hashlib

import pytest
from eth_utils import keccak

from bittensor._transport.codec import ss58_encode
from bittensor.intents import PowRegister
from bittensor.pow_registration import (
    DOMAIN,
    _mine_chunk,
    mine_registration,
    registration_prefix,
    registration_seal,
)
from tests.harness.fake_substrate import FakeSubstrate
from tests.harness.samples import ALICE, ALICE_HOT, dev_wallet


def test_challenge_encoding_binds_both_keys_subnet_block_and_nonce():
    hot = bytes([11]) + bytes(31)
    cold = bytes([22]) + bytes(31)
    block = bytes([1]) * 32
    prefix = registration_prefix(1, block.hex(), ss58_encode(hot.hex()), ss58_encode(cold.hex()))
    expected = DOMAIN + b"\x01\x00" + block + hot + cold
    assert prefix == expected
    assert registration_seal(prefix, 7).hex() == (
        "06eef1fff090c8a0c4fbbbd17093346c9a45b3208fb3ff62524474cf4ba2cbc1"
    )
    assert registration_seal(prefix, 7) == keccak(
        hashlib.sha256(expected + (7).to_bytes(8, "little")).digest()
    )
    assert registration_seal(prefix, 7) != registration_seal(prefix, 8)
    assert (
        registration_prefix(2, block.hex(), ss58_encode(hot.hex()), ss58_encode(cold.hex()))
        != prefix
    )
    assert _mine_chunk(prefix, 1, 7, 1) == (7, registration_seal(prefix, 7).hex())
    assert _mine_chunk(prefix, 1 << 256, 7, 1) is None


@pytest.mark.asyncio
async def test_public_miner_returns_fresh_intent_without_a_signing_key():
    substrate = FakeSubstrate()
    substrate.block = 2
    substrate.seed("SubtensorModule", "NetworkRegistrationAllowed", [1], True)
    substrate.seed("SubtensorModule", "NetworkPowRegistrationAllowed", [1], True)
    substrate.seed("SubtensorModule", "Difficulty", [1], 1)
    substrate.seed("SubtensorModule", "MaxRegistrationsPerBlock", [1], 1)
    substrate.seed("SubtensorModule", "Uids", [1, ALICE_HOT], None)
    intent = await mine_registration(substrate, 1, ALICE_HOT, ALICE, workers=1, max_seconds=10)
    assert isinstance(intent, PowRegister)
    assert intent.work_block == 2
    prefix = registration_prefix(1, await substrate.block_hash(2), ALICE_HOT, ALICE)
    assert bytes.fromhex(intent.work_hex) == registration_seal(prefix, intent.nonce)
    await intent.build(substrate, dev_wallet())


@pytest.mark.asyncio
async def test_miner_refuses_disabled_or_already_registered_subnets():
    substrate = FakeSubstrate()
    substrate.block = 2
    with pytest.raises(ValueError, match="enable"):
        await mine_registration(substrate, 1, ALICE_HOT, ALICE, workers=1)
    substrate.seed("SubtensorModule", "NetworkRegistrationAllowed", [1], True)
    substrate.seed("SubtensorModule", "NetworkPowRegistrationAllowed", [1], True)
    substrate.seed("SubtensorModule", "Uids", [1, ALICE_HOT], 0)
    with pytest.raises(ValueError, match="already registered"):
        await mine_registration(substrate, 1, ALICE_HOT, ALICE, workers=1)


@pytest.mark.asyncio
async def test_intent_rejects_bad_seals_and_root():
    with pytest.raises(ValueError, match="32-byte"):
        await PowRegister(netuid=1, work_block=1, nonce=0, work_hex="ab").build(
            FakeSubstrate(), dev_wallet()
        )
    with pytest.raises(ValueError, match="non-root"):
        await PowRegister(netuid=0, work_block=1, nonce=0, work_hex="ab" * 32).build(
            FakeSubstrate(), dev_wallet()
        )


def test_sync_sdk_exposes_the_same_public_miner():
    from bittensor import SyncClient

    substrate = FakeSubstrate()
    substrate.block = 2
    substrate.seed("SubtensorModule", "NetworkRegistrationAllowed", [1], True)
    substrate.seed("SubtensorModule", "NetworkPowRegistrationAllowed", [1], True)
    substrate.seed("SubtensorModule", "Difficulty", [1], 1)
    substrate.seed("SubtensorModule", "MaxRegistrationsPerBlock", [1], 1)
    substrate.seed("SubtensorModule", "Uids", [1, ALICE_HOT], None)
    with SyncClient("local", substrate=substrate) as client:
        intent = client.mine_pow_registration(1, ALICE_HOT, ALICE, workers=1, max_seconds=10)
    assert isinstance(intent, PowRegister)
    assert intent.work_block == 2


@pytest.mark.asyncio
async def test_miner_waits_for_next_block_when_registration_capacity_is_used(monkeypatch):
    import asyncio

    from bittensor import pow_registration

    substrate = FakeSubstrate()
    substrate.block = 2
    substrate.seed("SubtensorModule", "NetworkRegistrationAllowed", [1], True)
    substrate.seed("SubtensorModule", "NetworkPowRegistrationAllowed", [1], True)
    substrate.seed("SubtensorModule", "Difficulty", [1], 1)
    substrate.seed("SubtensorModule", "Uids", [1, ALICE_HOT], None)
    substrate.seed("SubtensorModule", "MaxRegistrationsPerBlock", [1], 1)
    substrate.seed("SubtensorModule", "RegistrationsThisBlock", [1], 1)
    sleep = asyncio.sleep
    waited = []

    async def next_block(delay):
        waited.append(delay)
        substrate.block += 1
        substrate.seed("SubtensorModule", "RegistrationsThisBlock", [1], 0)
        await sleep(0)

    monkeypatch.setattr(pow_registration.asyncio, "sleep", next_block)
    intent = await mine_registration(substrate, 1, ALICE_HOT, ALICE, workers=1, max_seconds=10)
    assert waited == [1]
    assert intent.work_block == 3


@pytest.mark.parametrize(
    "value,minimum,maximum,expected",
    [
        (1, 1000, 1, 1000),
        (2000, 1000, 1500, 1500),
        (None, None, None, 1),
        (0, 0, 0, 1),
    ],
)
def test_effective_difficulty_preserves_team_floor(value, minimum, maximum, expected):
    from bittensor.pow_registration import effective_difficulty

    assert effective_difficulty(value, minimum, maximum) == expected


def _pow_substrate():
    substrate = FakeSubstrate()
    substrate.block = 2
    # A PoW-only subnet must remain usable while burn registration is disabled.
    substrate.seed("SubtensorModule", "NetworkRegistrationAllowed", [1], False)
    substrate.seed("SubtensorModule", "NetworkPowRegistrationAllowed", [1], True)
    substrate.seed("SubtensorModule", "Difficulty", [1], 1)
    substrate.seed("SubtensorModule", "MaxRegistrationsPerBlock", [1], 1)
    substrate.seed("SubtensorModule", "Uids", [1, ALICE_HOT], None)
    return substrate


@pytest.mark.asyncio
async def test_gpu_miner_preserves_device_selection_and_pow_only_admission(monkeypatch):
    from bittensor import pow_registration

    calls = []

    class Gpu:
        def mine(self, prefix, difficulty, start, attempts):
            calls.append((prefix, difficulty, start, attempts))
            return start, registration_seal(prefix, start)

    selected = []

    def create(backend, ids):
        selected.append((backend, ids))
        return Gpu()

    monkeypatch.setattr(pow_registration, "_create_gpu_miner", create)
    intent = await mine_registration(
        _pow_substrate(), 1, ALICE_HOT, ALICE, backend="gpu", device_ids=(0, 1)
    )
    assert selected == [("gpu", (0, 1))]
    assert len(calls) == 1
    assert calls[0][1] == 1
    assert intent.mining_backend == "gpu"
    assert intent.mining_device_ids == [0, 1]
    assert bytes.fromhex(intent.work_hex) == registration_seal(calls[0][0], intent.nonce)


@pytest.mark.asyncio
async def test_gpu_solution_is_remined_when_difficulty_increases(monkeypatch):
    from bittensor import pow_registration

    substrate = _pow_substrate()
    calls = []

    class Gpu:
        def mine(self, prefix, difficulty, start, attempts):
            calls.append(difficulty)
            substrate.seed("SubtensorModule", "Difficulty", [1], 2)
            nonce = 0
            while True:
                work = registration_seal(prefix, nonce)
                high = int.from_bytes(work, "little") > ((1 << 256) - 1) // 2
                if high == (len(calls) == 1):
                    return nonce, work
                nonce += 1

    monkeypatch.setattr(pow_registration, "_create_gpu_miner", lambda *args: Gpu())
    intent = await mine_registration(substrate, 1, ALICE_HOT, ALICE, backend="gpu")
    assert calls == [1, 2]
    assert int.from_bytes(bytes.fromhex(intent.work_hex), "little") <= ((1 << 256) - 1) // 2


@pytest.mark.asyncio
async def test_gpu_driver_cannot_return_a_corrupt_proof(monkeypatch):
    from bittensor import pow_registration

    class Gpu:
        def mine(self, *args):
            return 0, bytes(32)

    monkeypatch.setattr(pow_registration, "_create_gpu_miner", lambda *args: Gpu())
    with pytest.raises(RuntimeError, match="invalid registration proof"):
        await mine_registration(_pow_substrate(), 1, ALICE_HOT, ALICE, backend="gpu")


def test_auto_discovers_all_gpus_and_explicit_selection_is_forwarded(monkeypatch):
    from bittensor import pow_registration

    created = []
    monkeypatch.setattr(
        pow_registration.bittensor_core,
        "pow_gpu_devices",
        lambda: [{"id": 0}, {"id": 1}],
        raising=False,
    )
    monkeypatch.setattr(
        pow_registration.bittensor_core,
        "PowGpuMiner",
        lambda ids: created.append(ids) or object(),
        raising=False,
    )
    pow_registration._create_gpu_miner("auto", None)
    pow_registration._create_gpu_miner("gpu", (1,))
    assert created == [None, [1]]


def test_explicit_gpu_requires_hardware_and_cpu_skips_discovery(monkeypatch):
    from bittensor import pow_registration

    monkeypatch.setattr(
        pow_registration.bittensor_core, "pow_gpu_devices", lambda: [], raising=False
    )
    monkeypatch.setattr(
        pow_registration.bittensor_core, "PowGpuMiner", lambda ids: object(), raising=False
    )
    assert pow_registration._create_gpu_miner("auto", None) is None
    assert pow_registration._create_gpu_miner("cpu", None) is None
    with pytest.raises(RuntimeError, match="No usable GPU"):
        pow_registration._create_gpu_miner("gpu", None)
    with pytest.raises(RuntimeError, match="No usable GPU"):
        pow_registration._create_gpu_miner("auto", (0,))


@pytest.mark.asyncio
async def test_miner_rejects_duplicate_or_cpu_device_selection():
    for backend, ids in [("cpu", (0,)), ("gpu", (0, 0)), ("gpu", (-1,))]:
        with pytest.raises(ValueError, match="unique nonnegative"):
            await mine_registration(
                _pow_substrate(), 1, ALICE_HOT, ALICE, backend=backend, device_ids=ids
            )


@pytest.mark.asyncio
async def test_long_gpu_search_refreshes_at_twelve_seconds_without_batch_head_polls(monkeypatch):
    from types import SimpleNamespace

    from bittensor import pow_registration

    substrate = _pow_substrate()
    clock = [0.0]
    heads = []
    batches = []
    block_number = substrate.block_number

    async def recorded_head():
        value = await block_number()
        heads.append(value)
        return value

    class Gpu:
        def mine(self, prefix, difficulty, start, attempts):
            batches.append((prefix, difficulty))
            if len(batches) <= 3:
                clock[0] += 4
                substrate.block += 1
                substrate.seed("SubtensorModule", "Difficulty", [1], 2)
                return None
            nonce = 0
            while (
                int.from_bytes(registration_seal(prefix, nonce), "little") > ((1 << 256) - 1) // 2
            ):
                nonce += 1
            return nonce, registration_seal(prefix, nonce)

    monkeypatch.setattr(pow_registration, "time", SimpleNamespace(monotonic=lambda: clock[0]))
    monkeypatch.setattr(pow_registration, "_create_gpu_miner", lambda *args: Gpu())
    monkeypatch.setattr(substrate, "block_number", recorded_head)
    intent = await mine_registration(substrate, 1, ALICE_HOT, ALICE, backend="gpu", max_seconds=60)
    # Three failed batches run locally on one challenge. At twelve seconds the
    # next batch uses the latest block hash and refreshed on-chain difficulty.
    assert heads == [2, 5, 5]
    assert batches[0] == batches[1] == batches[2]
    assert batches[0][1] == 1
    assert batches[3][1] == 2
    assert batches[3][0] != batches[0][0]
    assert intent.work_block == 5


@pytest.mark.asyncio
@pytest.mark.parametrize("head_advance,expected_batches", [(4, 1), (5, 2)])
async def test_miner_rechecks_solution_at_pool_age_boundary(
    monkeypatch, head_advance, expected_batches
):
    from bittensor import pow_registration

    substrate = _pow_substrate()
    batches = []

    class Gpu:
        def mine(self, prefix, difficulty, start, attempts):
            batches.append(prefix)
            if len(batches) == 1:
                substrate.block += head_advance
            return start, registration_seal(prefix, start)

    monkeypatch.setattr(pow_registration, "_create_gpu_miner", lambda *args: Gpu())
    intent = await mine_registration(substrate, 1, ALICE_HOT, ALICE, backend="gpu")
    assert len(batches) == expected_batches
    assert intent.work_block == (2 if head_advance == 4 else 7)


@pytest.mark.asyncio
@pytest.mark.parametrize("expires_in", ["batch", "validation"])
async def test_miner_does_not_return_solution_after_deadline(monkeypatch, expires_in):
    from types import SimpleNamespace

    from bittensor import pow_registration

    substrate = _pow_substrate()
    clock = [0.0]
    batches = []
    query = substrate.query
    validation_reads = []

    class Gpu:
        def mine(self, prefix, difficulty, start, attempts):
            batches.append(prefix)
            if expires_in == "batch":
                clock[0] = 61
            return start, registration_seal(prefix, start)

    async def delayed_query(module, storage, *args, **kwargs):
        value = await query(module, storage, *args, **kwargs)
        if batches:
            validation_reads.append(storage)
            if expires_in == "validation":
                clock[0] = 61
        return value

    monkeypatch.setattr(pow_registration, "time", SimpleNamespace(monotonic=lambda: clock[0]))
    monkeypatch.setattr(pow_registration, "_create_gpu_miner", lambda *args: Gpu())
    monkeypatch.setattr(substrate, "query", delayed_query)
    with pytest.raises(TimeoutError, match="mining time limit"):
        await mine_registration(substrate, 1, ALICE_HOT, ALICE, backend="gpu", max_seconds=60)
    assert len(batches) == 1
    assert bool(validation_reads) == (expires_in == "validation")
