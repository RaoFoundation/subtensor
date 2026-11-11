"""Protected origins and narrow proxy filters stay correct before submission."""

from __future__ import annotations

import asyncio
import io
import re
from pathlib import Path

import pytest
from rich.console import Console

from bittensor import config as cfg
from bittensor import protected_account as pa
from bittensor.balance import Balance
from bittensor.cli import protected as protected_cli
from bittensor.cli import root_helpers, stake_exit, stake_picker
from bittensor.cli.context import AppContext
from bittensor.cli.output import Output
from bittensor.intents import UnstakeAll, UnstakeAllAlpha
from bittensor.intents.registry import REGISTRY, build
from bittensor.reads import StakePosition, StakeValuation
from tests.harness.samples import ALICE, ALICE_HOT, BOB, BOB_HOT, INTENT_SAMPLES, dev_wallet

GENESIS = "0x" + "65" * 32
PURE = BOB


def _runtime_allowed_calls(proxy_type):
    """Read the actual Rust filter groups instead of duplicating their allowlists."""
    runtime = Path(__file__).resolve().parents[4] / "runtime/src/proxy_filters"
    groups = (runtime / "call_groups.rs").read_text()
    filters = (runtime / "mod.rs").read_text()

    def calls_for(group):
        group_body = re.search(rf"call_filter_group!\(\s*{group},\s*\[(.*?)\]\s*\);", groups, re.S)
        if group_body:
            return set(re.findall(r"RuntimeCall::(\w+)\(\s*\w+::(\w+)", group_body[1]))
        alias = re.search(rf"type\s+{group}\s*=\s*(.*?);", filters, re.S)
        assert alias is not None, group
        return set().union(*(calls_for(child) for child in re.findall(r"\w+", alias[1])))

    dispatch = re.search(rf"ProxyType::{proxy_type} => (\w+)::contains\(call\)", filters)
    assert dispatch is not None, proxy_type
    return calls_for(dispatch[1])


@pytest.mark.parametrize("proxy_type", pa.PROXY_TYPE_INTENT_OPS)
def test_every_instant_operation_matches_its_runtime_filter(proxy_type):
    allowed = _runtime_allowed_calls(proxy_type)
    for op in pa.PROXY_TYPE_INTENT_OPS[proxy_type]:
        assert op in REGISTRY
        calls = set(REGISTRY[op].wraps)
        if op in {"remove_stake", "remove_stake_limit", "move_stake", "unstake_all"}:
            # The normal variant is a single allowed call; claim=True is
            # covered separately and must take the delayed path.
            calls.discard(("Utility", "batch_all"))
        if op == "set_hyperparameter":
            calls.discard(("AdminUtils", "sudo_set_network_registration_allowed"))
        assert calls <= allowed, (proxy_type, op, calls - allowed)


@pytest.mark.parametrize(
    "proxy_type,op",
    [
        ("GammaSpend", "set_gamma_deposit_spender"),
        ("GammaSpend", "set_gamma_deposit_daily_spend_cap"),
        ("ChildKeys", "set_take"),
        ("ChildKeys", "increase_take"),
        ("ChildKeys", "decrease_take"),
        ("BasketTrading", "stake_into_basket"),
        ("Staking", "move_swap_stake"),
        ("Staking", "set_auto_stake"),
        ("Staking", "set_reject_staking_hotkey_growth"),
        ("Lock", "set_reject_locked_alpha"),
        ("Validate", "set_service"),
        ("Owner", "start_call"),
        ("SubnetLeaseBeneficiary", "register_leased_network"),
        ("SubnetLeaseBeneficiary", "terminate_lease"),
    ],
)
def test_filter_denied_operations_fall_back_to_delayed_controller(proxy_type, op):
    intent = build(op, INTENT_SAMPLES[op])
    assert pa.instant_proxy_type_for(op, {"allows": [proxy_type]}, intent=intent) is None


@pytest.mark.parametrize("op", ["remove_stake", "remove_stake_limit", "move_stake", "unstake_all"])
def test_claim_batches_do_not_take_single_call_staking_grant(op):
    regular = build(op, {**INTENT_SAMPLES[op], "claim": False})
    claim = build(op, {**INTENT_SAMPLES[op], "claim": True})
    record = {"allows": ["Staking"]}
    assert pa.instant_proxy_type_for(op, record, intent=regular) == "Staking"
    assert pa.instant_proxy_type_for(op, record, intent=claim) is None


@pytest.mark.parametrize("proxy_type", ["Owner", "SubnetLeaseBeneficiary"])
def test_root_only_hyperparameter_does_not_use_owner_grant(proxy_type):
    intent = build(
        "set_hyperparameter", {"netuid": 1, "name": "registration_allowed", "value": True}
    )
    assert pa.instant_proxy_type_for(intent.op, {"allows": [proxy_type]}, intent=intent) is None


@pytest.mark.parametrize(
    "proxy_type,op",
    [
        ("Staking", "stake_into_basket"),
        ("GammaSpend", "spend_gamma"),
        ("Validate", "reset_axon"),
        ("SubnetLeaseBeneficiary", "start_call"),
        ("SubnetLeaseBeneficiary", "set_service"),
    ],
)
def test_supported_operation_aliases_retain_correct_instant_grant(proxy_type, op):
    assert pa.instant_proxy_type_for(op, {"allows": [proxy_type]}) == proxy_type


class _Chain:
    def __init__(self):
        self._substrate = self
        self.reads = []
        self.positions = [
            StakePosition(BOB_HOT, PURE, 1, Balance.from_rao(3_000_000_000, netuid=1), True),
            StakePosition(ALICE_HOT, PURE, 0, Balance.from_tao(2), True),
        ]

    async def block_hash(self, height):
        assert height == 0
        return GENESIS

    async def read(self, name, **kwargs):
        self.reads.append((name, kwargs))
        owner = kwargs["coldkey_ss58"]
        positions = self.positions if owner == PURE else []
        if name == "stake_for_coldkey":
            return positions
        if name == "stake_value_for_coldkey":
            return StakeValuation(owner, 100, positions, Balance.from_tao(5), {1: 1.0})
        if name == "proxies":
            return {"proxies": [{"delegate": ALICE, "proxy_type": "Any", "delay": 10}]}
        raise AssertionError((name, kwargs))


@pytest.fixture
def prepared(monkeypatch, tmp_path):
    monkeypatch.setenv("BTCLI_PROTECTED_PATH", str(tmp_path / "protected.json"))
    monkeypatch.setenv("BTCLI_CONFIG", str(tmp_path / "config.json"))
    ctx = AppContext(
        "test", "controller", "hot", str(tmp_path), True, False, Output(json_mode=True)
    )
    chain = _Chain()
    monkeypatch.setattr(ctx, "run", lambda function: asyncio.run(function(chain)))
    monkeypatch.setattr(
        ctx,
        "resolve_address",
        lambda _param, value, **_kw: ALICE if value in (None, "controller") else value,
    )
    monkeypatch.setattr(ctx, "wallet", dev_wallet)
    pa.upsert_protected(
        {
            "genesis_hash": GENESIS,
            "controller_wallet": "controller",
            "controller_ss58": ALICE,
            "protected_ss58": PURE,
            "delay_blocks": 10,
            "allows": [],
        }
    )
    return ctx, chain


@pytest.mark.parametrize(
    "context_proxy,explicit,configured,signer,expected",
    [
        (None, None, None, "coldkey", PURE),
        (None, "self", ALICE, "coldkey", None),
        ("self", None, ALICE, "coldkey", None),
        (None, None, "self", "coldkey", None),
        (None, None, ALICE, "coldkey", ALICE),
        (ALICE, PURE, None, "coldkey", PURE),
        (None, None, None, "hotkey", None),
        (ALICE, None, None, "hotkey", ALICE),
    ],
)
def test_preparation_respects_proxy_precedence_without_mutating_submission(
    prepared, monkeypatch, context_proxy, explicit, configured, signer, expected
):
    ctx, _chain = prepared
    ctx.proxy_for = context_proxy
    monkeypatch.setattr(cfg, "get", lambda name, *_a: configured if name == "proxy_for" else None)
    assert ctx.resolve_preparation_proxy(explicit, signer=signer) == expected
    assert ctx.proxy_for == context_proxy


@pytest.mark.parametrize("intent_cls,include_root", [(UnstakeAll, True), (UnstakeAllAlpha, False)])
def test_bulk_exit_discovers_protected_positions_and_still_announces(
    prepared, monkeypatch, intent_cls, include_root
):
    ctx, chain = prepared
    announced = []
    monkeypatch.setattr(
        protected_cli,
        "_submit_protected_announcement",
        lambda _ctx, intent, record: announced.append((intent, record)),
    )

    stake_exit._submit(ctx, intent_cls, include_root, None, True, None, None)

    assert ("stake_for_coldkey", {"coldkey_ss58": PURE}) in chain.reads
    assert len(announced) == 1
    intent, record = announced[0]
    selected = intent.to_dict()
    children = selected["intents"] if selected["op"] == "batch" else [selected]
    expected = {BOB_HOT, ALICE_HOT} if include_root else {BOB_HOT}
    assert {child["hotkey_ss58"] for child in children} == expected
    assert record["protected_ss58"] == PURE
    assert ctx.proxy_for is None


def test_interactive_stake_picker_reads_protected_owner_before_selection(prepared, monkeypatch):
    ctx, chain = prepared
    monkeypatch.setattr(
        stake_picker, "local_address_names", lambda _path: {BOB_HOT: "b", ALICE_HOT: "a"}
    )
    console = Console(file=io.StringIO())
    monkeypatch.setattr(console, "input", lambda _prompt: "1")
    values = {"hotkey_ss58": None, "proxy_for": None}

    stake_picker._pick(console, ctx, values, "hotkey_ss58", None)

    assert ("stake_value_for_coldkey", {"coldkey_ss58": PURE}) in chain.reads
    assert values["hotkey_ss58"] == BOB_HOT
    assert values["proxy_for"] is None
    assert ctx.proxy_for is None


def test_root_position_preparation_uses_pure_without_rebinding_wallet(prepared):
    ctx, _chain = prepared
    assert root_helpers.resolve_position_wallet(ctx, None) == ("controller", PURE)
    assert ctx.wallet_name == "controller"
    assert ctx.proxy_for is None


def test_protected_multisig_preparation_does_not_open_a_member_wallet(prepared, monkeypatch):
    ctx, _chain = prepared
    monkeypatch.setattr(ctx, "_saved_multisig_address", lambda _name: ALICE)

    def no_wallet():
        pytest.fail("preparing protected multisig positions must not load a signatory wallet")

    monkeypatch.setattr(ctx, "wallet", no_wallet)
    assert stake_picker._stake_owner(ctx, {})[0] == PURE
    assert stake_exit._stake_owner(ctx, None) == PURE
    assert ctx.multisig_wallet_name is None
