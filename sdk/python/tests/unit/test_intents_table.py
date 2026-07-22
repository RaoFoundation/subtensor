"""Registry-driven tests over every intent.

Iterates ``bittensor.intents.REGISTRY`` so a new intent is automatically
tested (and fails loudly if it lacks a sample in ``tests/harness/samples.py``).
Each intent must: have a valid JSON schema, build from its sample args,
serialize round-trip exactly, and compose+plan against the in-memory
``FakeSubstrate`` with no chain.
"""

from __future__ import annotations

import pytest

from bittensor import Policy, PolicyError
from bittensor.balance import Balance
from bittensor.client import Client
from bittensor.intents import REGISTRY, build
from bittensor.intents._money import UNBOUNDED, _Unbounded
from bittensor.intents.base import BuiltCall
from tests.harness.fake_substrate import FakeSubstrate
from tests.harness.samples import BOB, BOB_HOT, INTENT_SAMPLES, dev_wallet


@pytest.fixture()
def substrate() -> FakeSubstrate:
    return FakeSubstrate()


@pytest.fixture()
def client(substrate: FakeSubstrate) -> Client:
    return Client("local", substrate=substrate)


@pytest.fixture(scope="module")
def wallet():
    return dev_wallet()


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
async def test_composes_and_plans_offline(op: str, client: Client, wallet):
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


def test_tools_catalog_matches_registry():
    client = Client("local", substrate=FakeSubstrate())
    tools = client.tools()
    assert {t["name"] for t in tools} == set(REGISTRY)
    for tool in tools:
        assert tool["summary"]
        assert tool["input_schema"]["type"] == "object"
        assert tool["signer"] in ("coldkey", "hotkey")
