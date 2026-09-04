"""Registry-driven tests over every intent.

Iterates ``bittensor.intents.REGISTRY`` so a new intent is automatically
tested (and fails loudly if it lacks a sample in ``tests/harness/samples.py``).
Each intent must: have a valid JSON schema, build from its sample args,
serialize round-trip exactly, and compose+plan against the in-memory
``FakeSubstrate`` with no chain.
"""

from __future__ import annotations

import asyncio

import pytest

from bittensor import Policy, PolicyError
from bittensor.balance import Balance
from bittensor.client import Client
from bittensor.intents import REGISTRY, build
from bittensor.intents._money import UNBOUNDED, _Unbounded
from bittensor.intents.base import BuiltCall
from bittensor.result import BittensorError, ChainError
from tests.harness.fake_substrate import FakeSubstrate
from tests.harness.samples import ALICE, ALICE_HOT, BOB, BOB_HOT, INTENT_SAMPLES, dev_wallet


@pytest.fixture()
def substrate() -> FakeSubstrate:
    fake = FakeSubstrate()
    fake.seed_runtime("StakeInfoRuntimeApi", "get_stake_info_for_coldkey", [])
    return fake


@pytest.fixture()
def client(substrate: FakeSubstrate) -> Client:
    return Client("local", substrate=substrate)


@pytest.fixture(scope="module")
def wallet():
    return dev_wallet()


def _seed_root_claim_shortfall(substrate: FakeSubstrate) -> None:
    substrate.seed("SubtensorModule", "StakingHotkeys", [ALICE], [BOB_HOT])
    substrate.seed("SubtensorModule", "RootClaimableThreshold", [0], {"bits": 500_000 << 32})
    substrate.seed("System", "Account", [ALICE], {"data": {"free": 0}})
    substrate.seed_map("SubtensorModule", "NetworksAdded", [(0, True)])
    substrate.seed_runtime(
        "StakeInfoRuntimeApi",
        "get_stake_info_for_coldkey",
        [{"hotkey": BOB_HOT, "coldkey": ALICE, "netuid": 0, "stake": 1}],
    )
    substrate.seed_runtime("BetaBasketRuntimeApi", "get_root_basket_owed", 1_000_000)
    substrate.seed_runtime(
        "BetaBasketRuntimeApi", "get_root_basket_positions", [(BOB_HOT, 1, 1_000_000)]
    )
    substrate.seed_runtime("BetaBasketRuntimeApi", "get_basket_payout", 1_000_000)
    substrate.seed_runtime("BetaBasketRuntimeApi", "get_validator_basket", [(0, 1)])


def test_every_intent_has_a_sample():
    missing = sorted(set(REGISTRY) - set(INTENT_SAMPLES))
    stale = sorted(set(INTENT_SAMPLES) - set(REGISTRY))
    assert not missing, f"intents without sample args: {missing}"
    assert not stale, f"samples for unregistered intents: {stale}"


@pytest.mark.parametrize("op", sorted(REGISTRY))
def test_json_schema_is_wellformed(op: str):
    schema = REGISTRY[op].json_schema()
    assert schema["type"] == "object"
    assert set(schema["required"]) <= set(schema["properties"])
    assert schema["additionalProperties"] is False
    # The sample satisfies the schema's required set.
    assert set(schema["required"]) <= set(INTENT_SAMPLES[op])


@pytest.mark.parametrize("op", sorted(REGISTRY))
def test_serialization_roundtrip(op: str):
    intent = build(op, INTENT_SAMPLES[op])
    encoded = intent.to_dict()
    assert encoded["op"] == op
    rebuilt = build(op, {k: v for k, v in encoded.items() if k != "op"})
    assert rebuilt.to_dict() == encoded


@pytest.mark.parametrize("op", sorted(REGISTRY))
def test_rejects_unknown_argument(op: str):
    with pytest.raises(ValueError, match="Unknown arguments"):
        build(op, {**INTENT_SAMPLES[op], "definitely_not_a_field": 1})


@pytest.mark.parametrize("op", sorted(REGISTRY))
@pytest.mark.asyncio
async def test_composes_and_plans_offline(
    op: str, client: Client, substrate: FakeSubstrate, wallet
):
    if op in ("claim_root", "claim_root_with_hotkey"):
        substrate.seed(
            "System",
            "Account",
            [wallet.coldkey.ss58_address],
            {"data": {"free": 10**9}},
        )
    plan = await client.plan(build(op, INTENT_SAMPLES[op]), wallet)
    assert plan.op == op
    assert plan.summary
    assert plan.signer in ("coldkey", "hotkey")
    assert plan.fee == Balance.from_rao(124_414)
    assert plan.ok, plan.violations


@pytest.mark.parametrize("op", sorted(REGISTRY))
@pytest.mark.asyncio
async def test_build_returns_composed_call(op: str, substrate: FakeSubstrate, wallet):
    built = await build(op, INTENT_SAMPLES[op]).build(substrate, wallet)
    call = built.call if isinstance(built, BuiltCall) else built
    module, function, params = call
    assert isinstance(module, str) and isinstance(function, str)
    assert isinstance(params, dict)
    # The composed target is one of the calls the intent declares it wraps
    # (module-level; some intents pick between calls at build time).
    wraps = REGISTRY[op].wraps
    if wraps:
        assert (module, function) in wraps or module in {m for m, _ in wraps}


@pytest.mark.parametrize("op", sorted(REGISTRY))
def test_spend_shape(op: str):
    spend = build(op, INTENT_SAMPLES[op]).spend()
    assert spend is None or spend is UNBOUNDED or isinstance(spend, Balance)
    if isinstance(spend, Balance):
        assert spend.netuid == 0, f"{op}.spend() must be TAO-denominated"
    assert isinstance(spend, (Balance, _Unbounded)) or spend is None


class TestPolicyEnforcement:
    """Policy is the one enforcement point; prove it holds for every intent
    that declares a spend, not just hand-picked ones."""

    @pytest.mark.asyncio
    async def test_spend_cap_blocks_every_spending_intent(self, client: Client, wallet):
        cap = Policy(max_spend_tao=0.000000001)  # 1 rao
        blocked, leaked = [], []
        for op in sorted(REGISTRY):
            intent = build(op, INTENT_SAMPLES[op])
            spend = intent.spend()
            if spend is None:
                continue
            plan = await client.plan(intent, wallet, policy=cap)
            (blocked if not plan.ok else leaked).append(op)
        assert not leaked, f"spend cap did not block: {leaked}"
        assert blocked, "no spending intents found — spend() wiring broken?"

    @pytest.mark.asyncio
    async def test_execute_raises_on_violation(self, client: Client, wallet):
        from bittensor.intents.transfer import Transfer

        with pytest.raises(PolicyError):
            await client.execute(
                Transfer(dest_ss58=BOB, amount_tao=5.0),
                wallet,
                policy=Policy(max_spend_tao=1.0),
            )

    @pytest.mark.asyncio
    async def test_netuid_allowlist(self, client: Client, wallet):
        allow = Policy(allowed_netuids=[1])
        blocked_ops = []
        for op in sorted(REGISTRY):
            intent = build(op, INTENT_SAMPLES[op])
            if not (set(intent.touches_netuids()) - {0, 1}) and not intent.affects_all_subnets():
                continue
            plan = await client.plan(intent, wallet, policy=allow)
            if not plan.ok:
                blocked_ops.append(op)
        # Every intent touching a netuid outside the allowlist must be blocked.
        for op in sorted(REGISTRY):
            intent = build(op, INTENT_SAMPLES[op])
            outside = set(intent.touches_netuids()) - {0, 1}
            if outside or intent.affects_all_subnets():
                assert op in blocked_ops, f"allowlist did not block {op}"

    @pytest.mark.asyncio
    async def test_raw_calls_refused_unless_allowed(self, client: Client, wallet):
        from bittensor._generated import calls

        call = calls.System.remark(remark="0x00")
        with pytest.raises(PolicyError):
            await client.submit_call(call, wallet, policy=Policy(max_spend_tao=100.0))
        result = await client.submit_call(
            call, wallet, policy=Policy(max_spend_tao=100.0, allow_raw_calls=True)
        )
        assert result.success


class TestExecuteFlow:
    @pytest.mark.asyncio
    async def test_execute_records_submission(
        self, client: Client, substrate: FakeSubstrate, wallet
    ):
        from bittensor.intents.transfer import Transfer

        result = await client.execute(Transfer(dest_ss58=BOB, amount_tao=1.0), wallet)
        assert result.success
        call, signer, _ = substrate.submissions[-1]
        assert signer == wallet.coldkey.ss58_address
        assert call.module == "Balances"
        assert call.function == "transfer_keep_alive"

    @pytest.mark.asyncio
    async def test_shielded_submission_enforces_intent_hard_stops(
        self, client: Client, substrate: FakeSubstrate, wallet
    ):
        from bittensor.intents.registration import ClaimRoot

        _seed_root_claim_shortfall(substrate)

        with pytest.raises(PolicyError, match="below the reserved claim fee"):
            await client.submit_shielded(ClaimRoot(), wallet)

        assert not substrate.submissions

    @pytest.mark.asyncio
    async def test_shielded_batch_enforces_child_hard_stops(
        self, client: Client, substrate: FakeSubstrate, wallet
    ):
        from bittensor.intents.batch import Batch
        from bittensor.intents.registration import ClaimRoot
        from bittensor.intents.transfer import Transfer

        _seed_root_claim_shortfall(substrate)
        intent = Batch(
            intents=[
                ClaimRoot(),
                Transfer(dest_ss58=BOB, amount_tao=0.1),
            ]
        )

        with pytest.raises(PolicyError, match=r"\[0\].*below the reserved claim fee"):
            await client.submit_shielded(intent, wallet)

        assert not substrate.submissions

    @pytest.mark.asyncio
    async def test_shielded_raw_call_propagates_finalization_request(self, client: Client, wallet):
        from unittest.mock import AsyncMock

        from bittensor._generated import calls
        from tests.harness.fake_substrate import success_result

        expected = success_result()
        client._executor._submit_encrypted_call = AsyncMock(return_value=expected)

        result = await client.submit_call(
            calls.System.remark(remark="0x00"),
            wallet,
            shielded=True,
            wait_for_inclusion=True,
            wait_for_finalization=True,
        )

        assert result is expected
        assert (
            client._executor._submit_encrypted_call.await_args.kwargs["wait_for_finalization"]
            is True
        )

    @pytest.mark.asyncio
    async def test_shielded_inner_waits_for_its_own_finalization(
        self, client: Client, substrate: FakeSubstrate, monkeypatch
    ):
        from unittest.mock import AsyncMock

        from tests.harness.fake_substrate import success_result

        outer = success_result(10)
        inner = success_result(11)
        monkeypatch.setattr(substrate, "block_time", AsyncMock(return_value=0))
        finalized = AsyncMock(side_effect=[10, 11])
        monkeypatch.setattr(substrate, "finalized_block_number", finalized)

        async def find_inner(_extrinsic_hash, block_hash):
            return inner if int(block_hash, 16) == 11 else None

        monkeypatch.setattr(substrate, "find_extrinsic", AsyncMock(side_effect=find_inner))

        result = await client._executor._resolve_shielded_inner(
            outer,
            "0xinner",
            period=8,
            wait_for_finalization=True,
        )

        assert result.extrinsic_id == inner.extrinsic_id
        assert finalized.await_count == 2

    @pytest.mark.asyncio
    async def test_shielded_inner_finalization_recovers_from_transient_rpc_failure(
        self, client: Client, substrate: FakeSubstrate, monkeypatch
    ):
        from unittest.mock import AsyncMock

        from tests.harness.fake_substrate import success_result

        outer = success_result(10)
        inner = success_result(10)
        monkeypatch.setattr(substrate, "block_time", AsyncMock(return_value=0))
        finalized = AsyncMock(side_effect=[ConnectionError("finality unavailable"), 10])
        monkeypatch.setattr(substrate, "finalized_block_number", finalized)
        monkeypatch.setattr(substrate, "find_extrinsic", AsyncMock(return_value=inner))

        result = await client._executor._resolve_shielded_inner(
            outer,
            "0xinner",
            period=1,
            wait_for_finalization=True,
        )

        assert result.extrinsic_id == inner.extrinsic_id
        assert finalized.await_count == 2

    @pytest.mark.asyncio
    async def test_shielded_inner_finalization_fails_after_bounded_rpc_retries(
        self, client: Client, substrate: FakeSubstrate, monkeypatch
    ):
        from unittest.mock import AsyncMock

        from tests.harness.fake_substrate import success_result

        outer = success_result(10)
        inner = success_result(10)
        monkeypatch.setattr(substrate, "block_time", AsyncMock(return_value=0))
        finalized = AsyncMock(side_effect=ConnectionError("finality unavailable"))
        monkeypatch.setattr(substrate, "finalized_block_number", finalized)
        monkeypatch.setattr(substrate, "find_extrinsic", AsyncMock(return_value=inner))

        with pytest.raises(ChainError, match=r"could not verify.*after 4 consecutive"):
            await asyncio.wait_for(
                client._executor._resolve_shielded_inner(
                    outer,
                    "0xinner",
                    period=1,
                    wait_for_finalization=True,
                ),
                timeout=1,
            )

        assert finalized.await_count == 4

    @pytest.mark.asyncio
    async def test_shielded_inner_finalization_fails_when_finality_stalls(
        self, client: Client, substrate: FakeSubstrate, monkeypatch
    ):
        from unittest.mock import AsyncMock

        from tests.harness.fake_substrate import success_result

        outer = success_result(10)
        inner = success_result(10)
        monkeypatch.setattr(substrate, "block_time", AsyncMock(return_value=0))
        finalized = AsyncMock(return_value=9)
        monkeypatch.setattr(substrate, "finalized_block_number", finalized)
        monkeypatch.setattr(substrate, "find_extrinsic", AsyncMock(return_value=inner))

        with pytest.raises(ChainError, match=r"did not finalize after 8 polls"):
            await client._executor._resolve_shielded_inner(
                outer,
                "0xinner",
                period=1,
                wait_for_finalization=True,
            )

        assert finalized.await_count == 8

    @pytest.mark.asyncio
    async def test_shielded_inner_finalization_recovers_from_canonical_hash_rpc_failure(
        self, client: Client, substrate: FakeSubstrate, monkeypatch
    ):
        from unittest.mock import AsyncMock

        from tests.harness.fake_substrate import success_result

        outer = success_result(10)
        inner = success_result(10)
        block_hash = await substrate.block_hash(10)
        monkeypatch.setattr(substrate, "block_time", AsyncMock(return_value=0))
        monkeypatch.setattr(substrate, "finalized_block_number", AsyncMock(return_value=10))
        hashes = AsyncMock(
            side_effect=[block_hash, ConnectionError("block hash unavailable"), block_hash]
        )
        monkeypatch.setattr(substrate, "block_hash", hashes)
        monkeypatch.setattr(substrate, "find_extrinsic", AsyncMock(return_value=inner))

        result = await client._executor._resolve_shielded_inner(
            outer,
            "0xinner",
            period=1,
            wait_for_finalization=True,
        )

        assert result.extrinsic_id == inner.extrinsic_id
        assert hashes.await_count == 3

    @pytest.mark.asyncio
    async def test_shielded_inner_finalization_bounds_hung_canonical_hash_rpc(
        self, client: Client, substrate: FakeSubstrate, monkeypatch
    ):
        from unittest.mock import AsyncMock

        from tests.harness.fake_substrate import success_result

        outer = success_result(10)
        inner = success_result(10)
        block_hash = await substrate.block_hash(10)
        calls = 0
        never_returns = asyncio.Event()

        async def hanging_canonical_hash(_block):
            nonlocal calls
            calls += 1
            if calls == 1:
                return block_hash
            return await never_returns.wait()

        monkeypatch.setattr(substrate, "block_time", AsyncMock(return_value=0))
        monkeypatch.setattr(substrate, "finalized_block_number", AsyncMock(return_value=10))
        monkeypatch.setattr(substrate, "block_hash", hanging_canonical_hash)
        monkeypatch.setattr(substrate, "find_extrinsic", AsyncMock(return_value=inner))

        with pytest.raises(ChainError, match=r"after 4 canonical block-hash RPC attempts"):
            await asyncio.wait_for(
                client._executor._resolve_shielded_inner(
                    outer,
                    "0xinner",
                    period=1,
                    wait_for_finalization=True,
                ),
                timeout=1,
            )

        assert calls == 5

    @pytest.mark.asyncio
    async def test_shielded_inner_finalization_recovers_from_canonical_receipt_rpc_failure(
        self, client: Client, substrate: FakeSubstrate, monkeypatch
    ):
        from unittest.mock import AsyncMock

        from tests.harness.fake_substrate import success_result

        outer = success_result(10)
        inner = success_result(10)
        monkeypatch.setattr(substrate, "block_time", AsyncMock(return_value=0))
        monkeypatch.setattr(substrate, "finalized_block_number", AsyncMock(return_value=10))
        hashes = AsyncMock(side_effect=["0xold", "0xcanonical"])
        monkeypatch.setattr(substrate, "block_hash", hashes)
        receipts = AsyncMock(side_effect=[inner, ConnectionError("receipt unavailable"), inner])
        monkeypatch.setattr(substrate, "find_extrinsic", receipts)

        result = await client._executor._resolve_shielded_inner(
            outer,
            "0xinner",
            period=1,
            wait_for_finalization=True,
        )

        assert result.extrinsic_id == inner.extrinsic_id
        assert receipts.await_count == 3

    @pytest.mark.asyncio
    async def test_shielded_inner_finalization_bounds_hung_canonical_receipt_rpc(
        self, client: Client, substrate: FakeSubstrate, monkeypatch
    ):
        from unittest.mock import AsyncMock

        from tests.harness.fake_substrate import success_result

        outer = success_result(10)
        inner = success_result(10)
        calls = 0
        never_returns = asyncio.Event()

        async def hanging_canonical_receipt(_extrinsic_hash, _block_hash):
            nonlocal calls
            calls += 1
            if calls == 1:
                return inner
            return await never_returns.wait()

        monkeypatch.setattr(substrate, "block_time", AsyncMock(return_value=0))
        monkeypatch.setattr(substrate, "finalized_block_number", AsyncMock(return_value=10))
        monkeypatch.setattr(
            substrate, "block_hash", AsyncMock(side_effect=["0xold", "0xcanonical"])
        )
        monkeypatch.setattr(substrate, "find_extrinsic", hanging_canonical_receipt)

        with pytest.raises(ChainError, match=r"after 4 canonical receipt RPC attempts"):
            await asyncio.wait_for(
                client._executor._resolve_shielded_inner(
                    outer,
                    "0xinner",
                    period=1,
                    wait_for_finalization=True,
                ),
                timeout=1,
            )

        assert calls == 5

    @pytest.mark.asyncio
    async def test_register_subnet_returns_immediate_network_added(
        self, client: Client, substrate: FakeSubstrate, wallet
    ):
        from dataclasses import replace

        from bittensor.intents.registration import RegisterSubnet
        from tests.harness.fake_substrate import success_result

        added = {
            "event": {
                "module_id": "SubtensorModule",
                "event_id": "NetworkAdded",
                "attributes": [12, 1],
            }
        }
        substrate.queue_result(replace(success_result(), events=[added]))

        result = await client.execute(RegisterSubnet(), wallet)

        assert result.success
        assert result.data["netuid"] == 12
        assert result.data["registration_mode"] == "immediate"
        assert result.data["registered_at_block"] == 100

    @pytest.mark.asyncio
    async def test_register_subnet_waits_for_its_network_added_after_cleanup(
        self, client: Client, substrate: FakeSubstrate, wallet
    ):
        from dataclasses import replace

        from bittensor.intents.registration import RegisterSubnet
        from tests.harness.fake_substrate import success_result

        coldkey = wallet.coldkeypub.ss58_address
        hotkey = wallet.hotkey.ss58_address
        queued = {
            "extrinsic_idx": 1,
            "event": {
                "module_id": "SubtensorModule",
                "event_id": "NetworkRegistrationQueued",
                "attributes": {
                    "coldkey": coldkey,
                    "hotkey": hotkey,
                    "registration_block": 100,
                },
            },
        }
        removed = {
            "extrinsic_idx": 1,
            "event": {
                "module_id": "SubtensorModule",
                "event_id": "NetworkRemoved",
                "attributes": 4,
            },
        }
        substrate.queue_result(replace(success_result(), events=[removed, queued]))
        substrate.seed("SubtensorModule", "SubnetOwner", [4], coldkey)
        substrate.seed("SubtensorModule", "SubnetOwnerHotkey", [4], hotkey)
        substrate.seed_events(
            102,
            [
                {
                    "phase": "Finalization",
                    "extrinsic_idx": None,
                    "event": {
                        "module_id": "SubtensorModule",
                        "event_id": "NetworkAdded",
                        "attributes": [4, 1],
                    },
                }
            ],
        )
        progress = []

        result = await client.execute(RegisterSubnet(), wallet, on_progress=progress.append)

        assert result.success
        assert result.data == {
            "netuid": 4,
            "registration_mode": "after_deregistration",
            "queued_at_block": 100,
            "registered_at_block": 102,
            "cleanup_netuid": 4,
            "deregistered_netuid": 4,
            "registration_price_rao": 1_000_000_000,
        }
        assert progress[0]["stage"] == "queued"
        assert progress[1] == {
            "stage": "waiting",
            "block": 101,
            "blocks_since_call": 1,
            "cleanup_netuid": 4,
            "deregistered_netuid": 4,
        }
        assert progress[-1] == {
            "stage": "registered",
            "mode": "after_deregistration",
            "netuid": 4,
            "block": 102,
            "cleanup_netuid": 4,
            "deregistered_netuid": 4,
        }

    @pytest.mark.asyncio
    async def test_hotkey_intents_sign_with_hotkey(
        self, client: Client, substrate: FakeSubstrate, wallet
    ):
        from bittensor.intents.weights import SetWeights

        result = await client.execute(SetWeights(netuid=1, uids=[0], weights=[1.0]), wallet)
        assert result.success
        _, signer, _ = substrate.submissions[-1]
        assert signer == wallet.hotkey.ss58_address

    @pytest.mark.asyncio
    async def test_proxy_wraps_call_and_detects_inner_failure(
        self, client: Client, substrate: FakeSubstrate, wallet
    ):
        from dataclasses import replace

        from bittensor.intents.transfer import Transfer
        from tests.harness.fake_substrate import success_result

        intent = Transfer(dest_ss58=BOB, amount_tao=1.0)
        result = await client.execute(intent, wallet, proxy_for=BOB)
        assert result.success
        call, _, _ = substrate.submissions[-1]
        assert (call.module, call.function) == ("Proxy", "proxy")
        assert call.params["real"] == BOB

        # A proxied extrinsic succeeds even when the wrapped call fails; the
        # executor must surface the inner ProxyExecuted error.
        inner_err = {
            "event": {
                "module_id": "Proxy",
                "event_id": "ProxyExecuted",
                "attributes": {"result": {"Err": {"Module": {"index": 1, "error": "0x00"}}}},
            }
        }
        substrate.queue_result(replace(success_result(), events=[inner_err]))
        result = await client.execute(intent, wallet, proxy_for=BOB)
        assert not result.success
        assert "nested call failed" in result.message

    @pytest.mark.asyncio
    async def test_transient_pool_rejection_is_retried(
        self, client: Client, substrate: FakeSubstrate, wallet
    ):
        from bittensor.intents.transfer import Transfer
        from bittensor.result import ExtrinsicResult

        substrate.seed_constant("Aura", "SlotDuration", 1)  # negligible retry sleep
        substrate.queue_result(
            ExtrinsicResult(success=False, message="Priority is too low: (1 vs 2)")
        )
        result = await client.execute(Transfer(dest_ss58=BOB, amount_tao=1.0), wallet, retries=1)
        assert result.success
        assert len(substrate.submissions) == 2

    @pytest.mark.asyncio
    async def test_execute_tool_builds_by_name(
        self, client: Client, substrate: FakeSubstrate, wallet
    ):
        result = await client.execute_tool(
            "transfer", {"dest_ss58": BOB, "amount_tao": 1.0}, wallet
        )
        assert result.success
        assert substrate.last_call.module == "Balances"

    @pytest.mark.asyncio
    async def test_submit_shielded_wraps_root_in_sudo(
        self, client: Client, substrate: FakeSubstrate, wallet, monkeypatch
    ):
        """Root intents must encrypt ``Sudo.sudo(...)``, not the bare inner call.

        Without the wrap, MevShield decrypts and dispatches e.g.
        ``AdminUtils.sudo_set_subnet_emission_enabled`` under a signed origin
        and the chain rejects with ``BadOrigin``.
        """
        from bittensor.intents.root import SetSubnetEmissionEnabled

        substrate.mev_key = b"\x01" * 32
        monkeypatch.setattr(
            "bittensor.executor._core.encrypt_mlkem768",
            lambda pubkey, plaintext, include_key_hash=True: b"ciphertext",
        )
        signed: list = []
        original_sign = substrate.sign_extrinsic

        async def capture_sign(call, keypair, *, nonce, period):
            signed.append(call)
            return await original_sign(call, keypair, nonce=nonce, period=period)

        monkeypatch.setattr(substrate, "sign_extrinsic", capture_sign)

        result = await client.submit_shielded(
            SetSubnetEmissionEnabled(netuids=[1], enabled=True), wallet
        )
        assert result.success
        assert result.data.get("shielded") is True
        assert len(signed) == 1
        inner = signed[0]
        assert (inner.module, inner.function) == ("Sudo", "sudo")
        nested = inner.params["call"]
        assert (nested.module, nested.function) == (
            "AdminUtils",
            "sudo_set_subnet_emission_enabled",
        )
        outer = substrate.last_call
        assert (outer.module, outer.function) == ("MevShield", "submit_encrypted")

    @pytest.mark.asyncio
    async def test_submit_shielded_wraps_proxy(
        self, client: Client, substrate: FakeSubstrate, wallet, monkeypatch
    ):
        """Shield encrypts the already-proxied call, not the bare semantic call."""
        from bittensor.intents.transfer import Transfer

        substrate.mev_key = b"\x01" * 32
        monkeypatch.setattr(
            "bittensor.executor._core.encrypt_mlkem768",
            lambda pubkey, plaintext, include_key_hash=True: b"ciphertext",
        )
        signed: list = []
        original_sign = substrate.sign_extrinsic

        async def capture_sign(call, keypair, *, nonce, period):
            signed.append(call)
            return await original_sign(call, keypair, nonce=nonce, period=period)

        monkeypatch.setattr(substrate, "sign_extrinsic", capture_sign)

        result = await client.submit_shielded(
            Transfer(dest_ss58=BOB, amount_tao=1.0), wallet, proxy_for=BOB
        )
        assert result.success
        assert result.data.get("shielded") is True
        assert result.data.get("proxy_for") == BOB
        assert len(signed) == 1
        inner = signed[0]
        assert (inner.module, inner.function) == ("Proxy", "proxy")
        assert inner.params["real"] == BOB
        nested = inner.params["call"]
        assert nested.module == "Balances"
        outer = substrate.last_call
        assert (outer.module, outer.function) == ("MevShield", "submit_encrypted")

    @pytest.mark.asyncio
    async def test_proxy_all_reads_proxied_account_balance(
        self, client: Client, substrate: FakeSubstrate, wallet
    ):
        """``amount=all`` must drain the proxied coldkey, not the delegate."""
        from bittensor.intents.staking import _ALL_STAKE_FEE_HEADROOM_RAO, AddStake

        treasury_free = 10**10
        substrate.seed("System", "Account", [BOB], {"data": {"free": treasury_free}})
        substrate.seed(
            "System",
            "Account",
            [wallet.coldkey.ss58_address],
            {"data": {"free": 0}},
        )
        plan = await client.plan(
            AddStake(hotkey_ss58=BOB_HOT, netuid=1, amount_tao="all"),
            wallet,
            proxy_for=BOB,
        )
        assert plan.call.module == "Proxy"
        inner = plan.call.params["call"]
        expected = treasury_free - 500 - _ALL_STAKE_FEE_HEADROOM_RAO
        assert inner.params["amount_staked"] == expected


class TestStakingMoneyUnits:
    """Unit-tagged amounts at the staking intent boundary: a correctly tagged
    Balance behaves exactly like the plain number; the wrong unit (alpha into
    a TAO parameter or vice versa) raises a ValueError naming the extrinsic,
    the parameter, and both units."""

    @pytest.mark.parametrize(
        ("op", "amount_field", "tagged", "extra"),
        [
            ("add_stake", "amount_tao", Balance.from_tao("1.5"), {}),
            ("remove_stake", "amount_alpha", Balance.from_alpha("1.5", 1), {}),
            (
                "add_stake_limit",
                "amount_tao",
                Balance.from_tao("1.5"),
                {"limit_price_rao": 10**9},
            ),
            (
                "remove_stake_limit",
                "amount_alpha",
                Balance.from_alpha("1.5", 1),
                {"limit_price_rao": 10**9},
            ),
        ],
    )
    @pytest.mark.asyncio
    async def test_tagged_correct_builds_same_call_as_untagged(
        self, substrate: FakeSubstrate, wallet, op, amount_field, tagged, extra
    ):
        base = {"hotkey_ss58": BOB, "netuid": 1, **extra}
        untagged = build(op, {**base, amount_field: 1.5})
        with_balance = build(op, {**base, amount_field: tagged})
        assert getattr(with_balance, amount_field) == getattr(untagged, amount_field)
        built_a = await untagged.build(substrate, wallet)
        built_b = await with_balance.build(substrate, wallet)
        assert built_b.params == built_a.params

    @pytest.mark.parametrize(
        ("op", "args", "match"),
        [
            (
                "add_stake",
                {"hotkey_ss58": BOB, "netuid": 1, "amount_tao": Balance.from_alpha(1.0, 1)},
                r"add_stake.*'amount_staked' takes TAO.*subnet-1 ALPHA",
            ),
            (
                "remove_stake",
                {"hotkey_ss58": BOB, "netuid": 1, "amount_alpha": Balance.from_tao(1.0)},
                r"remove_stake.*'amount_unstaked' takes subnet-1 ALPHA.*tagged TAO",
            ),
            (
                "add_stake_limit",
                {
                    "hotkey_ss58": BOB,
                    "netuid": 2,
                    "amount_tao": Balance.from_alpha(1.0, 2),
                    "limit_price_rao": 10**9,
                },
                r"add_stake_limit.*'amount_staked' takes TAO.*subnet-2 ALPHA",
            ),
            (
                "remove_stake_limit",
                {
                    "hotkey_ss58": BOB,
                    "netuid": 2,
                    "amount_alpha": Balance.from_tao(1.0),
                    "limit_price_rao": 10**9,
                },
                r"remove_stake_limit.*'amount_unstaked' takes subnet-2 ALPHA.*tagged TAO",
            ),
            (
                "move_stake",
                {
                    "origin_hotkey_ss58": BOB,
                    "origin_netuid": 1,
                    "dest_hotkey_ss58": BOB,
                    "dest_netuid": 2,
                    "amount_alpha": Balance.from_tao(1.0),
                },
                r"move_stake.*'alpha_amount' takes subnet-1 ALPHA.*tagged TAO",
            ),
            (
                "move_swap_stake",
                {
                    "origin_hotkey_ss58": BOB,
                    "origin_netuid": 1,
                    "dest_hotkey_ss58": BOB_HOT,
                    "dest_netuid": 2,
                    "amount_alpha": Balance.from_tao(1.0),
                },
                r"move_stake.*'alpha_amount' takes subnet-1 ALPHA.*tagged TAO",
            ),
            (
                "swap_stake",
                {
                    "hotkey_ss58": BOB,
                    "origin_netuid": 1,
                    "dest_netuid": 2,
                    "amount_alpha": Balance.from_alpha(1.0, 2),
                },
                r"swap_stake.*'alpha_amount' takes subnet-1 ALPHA.*subnet-2 ALPHA",
            ),
            (
                "transfer_stake",
                {
                    "dest_coldkey_ss58": BOB,
                    "hotkey_ss58": BOB,
                    "origin_netuid": 1,
                    "dest_netuid": 2,
                    "amount_alpha": Balance.from_tao(1.0),
                },
                r"transfer_stake.*'alpha_amount' takes subnet-1 ALPHA.*tagged TAO",
            ),
        ],
    )
    def test_wrong_unit_raises_valueerror(self, op, args, match):
        with pytest.raises(ValueError, match=match):
            build(op, args)

    def test_root_alpha_is_tao(self):
        # netuid 0 stake is TAO-denominated, so a TAO Balance is the correct
        # tag for remove_stake on root.
        intent = build(
            "remove_stake", {"hotkey_ss58": BOB, "netuid": 0, "amount_alpha": Balance.from_tao(1.0)}
        )
        assert intent.amount_alpha == Balance.from_tao(1.0)


class TestProportionInputs:
    """Normalized-proportion ergonomics on takes and child proportions: a
    value with a decimal point is the human 0..1 form (converted to the raw
    fixed-point integer at construction), a plain integer is the raw wire
    value (bound-checked), matching the hyperparameter value rules."""

    U16_MAX = 65535
    U64_MAX = 2**64 - 1

    @pytest.mark.parametrize("op", ["set_take", "increase_take", "decrease_take"])
    def test_take_accepts_fraction_and_raw(self, op):
        assert build(op, {"take": 0.18}).take == round(0.18 * self.U16_MAX)
        assert build(op, {"take": "0.18"}).take == round(0.18 * self.U16_MAX)
        assert build(op, {"take": 11796}).take == 11796

    def test_childkey_take_accepts_fraction(self):
        intent = build("set_childkey_take", {"netuid": 1, "take": 0.09})
        assert intent.take == round(0.09 * self.U16_MAX)

    @pytest.mark.parametrize("bad", [70000, 1.5, -1, "abc"])
    def test_take_rejects_out_of_range(self, bad):
        with pytest.raises(ValueError):
            build("set_take", {"take": bad})

    def test_children_accept_fraction_proportions(self):
        intent = build("set_children", {"netuid": 1, "children": [[0.5, BOB]]})
        assert intent.children == [[round(0.5 * self.U64_MAX), BOB]]

    def test_children_accept_raw_shares(self):
        intent = build("set_children", {"netuid": 1, "children": [[2**63, BOB]]})
        assert intent.children == [[2**63, BOB]]

    def test_children_reject_oversum(self):
        with pytest.raises(ValueError, match=r"must not exceed 1\.0"):
            build("set_children", {"netuid": 1, "children": [[0.7, BOB], [0.7, BOB_HOT]]})

    def test_children_roundtrip_after_normalization(self):
        intent = build("set_children", {"netuid": 1, "children": [[0.25, BOB]]})
        encoded = intent.to_dict()
        rebuilt = build("set_children", {k: v for k, v in encoded.items() if k != "op"})
        assert rebuilt.to_dict() == encoded


class TestRootClaimOnUnstake:
    @staticmethod
    def _claiming_unstake_intents():
        return [
            build(
                "remove_stake",
                {
                    "hotkey_ss58": BOB_HOT,
                    "netuid": 0,
                    "amount_alpha": 1.0,
                    "slippage_protection": False,
                    "claim": True,
                },
            ),
            build(
                "remove_stake_limit",
                {
                    "hotkey_ss58": BOB_HOT,
                    "netuid": 0,
                    "amount_alpha": 1.0,
                    "limit_price_rao": 1_000_000_000,
                    "claim": True,
                },
            ),
            build("unstake_all", {"hotkey_ss58": BOB_HOT, "claim": True}),
            build(
                "move_stake",
                {
                    "origin_hotkey_ss58": BOB_HOT,
                    "origin_netuid": 0,
                    "dest_hotkey_ss58": ALICE_HOT,
                    "dest_netuid": 0,
                    "amount_alpha": 1.0,
                    "claim": True,
                },
            ),
        ]

    @pytest.mark.asyncio
    @pytest.mark.parametrize("intent", _claiming_unstake_intents())
    async def test_embedded_claim_enforces_admission_in_plan_and_shielded_submit(
        self, substrate: FakeSubstrate, wallet, intent
    ):
        substrate.seed("System", "Account", [ALICE], {"data": {"free": 10**12}})
        substrate.seed_map("SubtensorModule", "NetworksAdded", [(0, True)])
        substrate.seed_runtime("BetaBasketRuntimeApi", "get_basket_payout", 1_000_000)
        substrate.seed_runtime(
            "BetaBasketRuntimeApi",
            "get_validator_basket",
            [(netuid, 1) for netuid in range(257)],
        )

        client = Client("local", substrate=substrate)
        plan = await client.plan(intent, wallet)
        assert any("257 basket holdings" in block for block in plan.violations)

        with pytest.raises(PolicyError, match="256-unit admission limit"):
            await client.submit_shielded(intent, wallet)
        assert not substrate.submissions

    @pytest.mark.asyncio
    @pytest.mark.parametrize("intent", _claiming_unstake_intents())
    async def test_claim_then_unstake_refuses_active_root_hold(
        self, substrate: FakeSubstrate, wallet, intent
    ):
        substrate.seed("SubtensorModule", "RootStakeUnlockInterval", [], 100)

        with pytest.raises(BittensorError, match="cannot execute atomically"):
            await intent.build(substrate, wallet)

    @pytest.mark.asyncio
    async def test_embedded_claim_proxy_uses_delegate_for_reserve(
        self, substrate: FakeSubstrate, wallet
    ):
        substrate.seed("System", "Account", [ALICE], {"data": {"free": 0}})
        substrate.seed("System", "Account", [BOB], {"data": {"free": 10**12}})
        substrate.seed_map("SubtensorModule", "NetworksAdded", [(0, True)])
        substrate.seed_runtime("BetaBasketRuntimeApi", "get_basket_payout", 1_000_000)
        substrate.seed_runtime("BetaBasketRuntimeApi", "get_validator_basket", [(0, 1)])
        intent = build(
            "remove_stake",
            {
                "hotkey_ss58": BOB_HOT,
                "netuid": 0,
                "amount_alpha": 1.0,
                "slippage_protection": False,
                "claim": True,
            },
        )

        plan = await Client("local", substrate=substrate).plan(intent, wallet, proxy_for=BOB)

        assert any("below the reserved claim fee" in block for block in plan.violations)

    """Client-side half of #3008: claim then unstake, whole entitlement only."""

    @pytest.mark.asyncio
    async def test_claim_batches_claim_then_unstake(self, substrate: FakeSubstrate, wallet):
        intent = build(
            "remove_stake",
            {
                "hotkey_ss58": BOB_HOT,
                "netuid": 0,
                "amount_alpha": 1.0,
                "slippage_protection": False,
                "claim": True,
            },
        )
        built = await intent.build(substrate, wallet)
        module, function, params = built
        assert (module, function) == ("Utility", "batch_all")
        claim, unstake = params["calls"]
        assert (claim.module, claim.function) == ("SubtensorModule", "claim_root_with_hotkey")
        assert claim.params["hotkey"] == BOB_HOT
        assert (unstake.module, unstake.function) == ("SubtensorModule", "remove_stake")

    @pytest.mark.asyncio
    async def test_claim_all_uses_runtime_capped_max_after_claim(
        self, substrate: FakeSubstrate, wallet
    ):
        substrate.seed_runtime(
            "StakeInfoRuntimeApi",
            "get_stake_info_for_hotkey_coldkey_netuid",
            {"stake": 123},
        )
        intent = build(
            "remove_stake",
            {
                "hotkey_ss58": BOB_HOT,
                "netuid": 0,
                "amount_alpha": "all",
                "claim": True,
            },
        )

        built = await intent.build(substrate, wallet)
        claim, unstake = built.params["calls"]

        assert (claim.module, claim.function) == ("SubtensorModule", "claim_root_with_hotkey")
        assert (unstake.module, unstake.function) == ("SubtensorModule", "remove_stake")
        assert unstake.params["amount_unstaked"] == (1 << 64) - 1

    @pytest.mark.asyncio
    async def test_move_claim_all_uses_runtime_capped_max_after_claim(
        self, substrate: FakeSubstrate, wallet
    ):
        substrate.seed_runtime(
            "StakeInfoRuntimeApi",
            "get_stake_info_for_hotkey_coldkey_netuid",
            {"stake": 123},
        )
        substrate.seed_runtime("BetaBasketRuntimeApi", "get_basket_payout", 0)
        intent = build(
            "move_stake",
            {
                "origin_hotkey_ss58": BOB_HOT,
                "origin_netuid": 0,
                "dest_hotkey_ss58": ALICE_HOT,
                "dest_netuid": 0,
                "amount_alpha": "all",
                "claim": True,
            },
        )

        built = await intent.build(substrate, wallet)
        claim, move = built.params["calls"]

        assert (claim.module, claim.function) == ("SubtensorModule", "claim_root_with_hotkey")
        assert claim.params["hotkey"] == BOB_HOT
        assert (move.module, move.function) == ("SubtensorModule", "move_stake")
        assert move.params["alpha_amount"] == (1 << 64) - 1

    @pytest.mark.asyncio
    async def test_move_claim_true_is_not_dropped_when_payout_quote_is_zero(
        self, substrate: FakeSubstrate, wallet
    ):
        substrate.seed_runtime(
            "StakeInfoRuntimeApi",
            "get_stake_info_for_hotkey_coldkey_netuid",
            {"stake": 123},
        )
        substrate.seed_runtime("BetaBasketRuntimeApi", "get_basket_payout", 0)
        intent = build(
            "move_stake",
            {
                "origin_hotkey_ss58": BOB_HOT,
                "origin_netuid": 0,
                "dest_hotkey_ss58": ALICE_HOT,
                "dest_netuid": 0,
                "amount_alpha": 1.0,
                "claim": True,
            },
        )

        built = await intent.build(substrate, wallet)
        claim, move = built.params["calls"]

        assert (claim.module, claim.function) == ("SubtensorModule", "claim_root_with_hotkey")
        assert (move.module, move.function) == ("SubtensorModule", "move_stake")

    @pytest.mark.asyncio
    async def test_limit_claim_all_uses_runtime_capped_plain_unstake(
        self, substrate: FakeSubstrate, wallet
    ):
        substrate.seed_runtime(
            "StakeInfoRuntimeApi",
            "get_stake_info_for_hotkey_coldkey_netuid",
            {"stake": 123},
        )
        intent = build(
            "remove_stake_limit",
            {
                "hotkey_ss58": BOB_HOT,
                "netuid": 0,
                "amount_alpha": "all",
                "limit_price_rao": 1_000_000_000,
                "claim": True,
            },
        )

        built = await intent.build(substrate, wallet)
        _claim, unstake = built.params["calls"]

        assert (unstake.module, unstake.function) == ("SubtensorModule", "remove_stake")
        assert unstake.params["amount_unstaked"] == (1 << 64) - 1

    @pytest.mark.asyncio
    async def test_limit_claim_all_preserves_invalid_root_limit_rejection(
        self, substrate: FakeSubstrate, wallet
    ):
        substrate.seed_runtime(
            "StakeInfoRuntimeApi",
            "get_stake_info_for_hotkey_coldkey_netuid",
            {"stake": 123},
        )
        intent = build(
            "remove_stake_limit",
            {
                "hotkey_ss58": BOB_HOT,
                "netuid": 0,
                "amount_alpha": "all",
                "limit_price_rao": 1_000_000_001,
                "claim": True,
            },
        )

        with pytest.raises(BittensorError, match=r"\[0, 1000000000\]"):
            await intent.build(substrate, wallet)

    @pytest.mark.asyncio
    async def test_limit_claim_all_rejects_negative_root_limit(
        self, substrate: FakeSubstrate, wallet
    ):
        substrate.seed_runtime(
            "StakeInfoRuntimeApi",
            "get_stake_info_for_hotkey_coldkey_netuid",
            {"stake": 123},
        )
        intent = build(
            "remove_stake_limit",
            {
                "hotkey_ss58": BOB_HOT,
                "netuid": 0,
                "amount_alpha": "all",
                "limit_price_rao": -1,
                "claim": True,
            },
        )

        with pytest.raises(BittensorError, match=r"\[0, 1000000000\]"):
            await intent.build(substrate, wallet)

    @pytest.mark.asyncio
    async def test_limit_claim_all_accepts_zero_root_limit(self, substrate: FakeSubstrate, wallet):
        substrate.seed_runtime(
            "StakeInfoRuntimeApi",
            "get_stake_info_for_hotkey_coldkey_netuid",
            {"stake": 123},
        )
        intent = build(
            "remove_stake_limit",
            {
                "hotkey_ss58": BOB_HOT,
                "netuid": 0,
                "amount_alpha": "all",
                "limit_price_rao": 0,
                "claim": True,
            },
        )

        built = await intent.build(substrate, wallet)
        _claim, unstake = built.params["calls"]
        assert (unstake.module, unstake.function) == ("SubtensorModule", "remove_stake")

    @pytest.mark.asyncio
    async def test_claim_refused_off_root(self, substrate: FakeSubstrate, wallet):
        intent = build(
            "remove_stake",
            {
                "hotkey_ss58": BOB_HOT,
                "netuid": 1,
                "amount_alpha": 1.0,
                "claim": True,
            },
        )
        with pytest.raises(BittensorError, match="netuid 0"):
            await intent.build(substrate, wallet)

    @pytest.mark.asyncio
    async def test_claim_warning_is_not_proportional(self, substrate: FakeSubstrate, wallet):
        intent = build(
            "remove_stake",
            {
                "hotkey_ss58": BOB_HOT,
                "netuid": 0,
                "amount_alpha": 1.0,
                "claim": True,
            },
        )
        warnings = await intent.warnings(substrate, ALICE)
        assert any("not a proportional" in warning for warning in warnings)

    @pytest.mark.asyncio
    async def test_unstake_all_claim_batches(self, substrate: FakeSubstrate, wallet):
        intent = build("unstake_all", {"hotkey_ss58": BOB_HOT, "claim": True})
        built = await intent.build(substrate, wallet)
        module, function, params = built
        assert (module, function) == ("Utility", "batch_all")
        claim, unstake = params["calls"]
        assert (claim.module, claim.function) == ("SubtensorModule", "claim_root_with_hotkey")
        assert (unstake.module, unstake.function) == ("SubtensorModule", "unstake_all")


def test_tools_catalog_matches_registry():
    client = Client("local", substrate=FakeSubstrate())
    tools = client.tools()
    assert {t["name"] for t in tools} == set(REGISTRY)
    for tool in tools:
        assert tool["summary"]
        assert tool["input_schema"]["type"] == "object"
        assert tool["signer"] in ("coldkey", "hotkey")
