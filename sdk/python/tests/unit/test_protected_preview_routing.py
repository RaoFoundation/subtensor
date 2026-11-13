"""Protected routing uses the selected wallet and previews the pure's balances."""

from __future__ import annotations

import asyncio
from types import SimpleNamespace

import pytest
import typer

from bittensor import config
from bittensor import protected_account as pa
from bittensor import proxy_announcements as ann
from bittensor.cli import context, multisig_helpers, prompt, protected
from bittensor.cli.output import Output
from bittensor.client import Client
from bittensor.intents import AddCollateral, AddStake, SetWeights, Transfer
from bittensor.intents.proxy import compose_announceable_call
from tests.harness.fake_substrate import GENESIS_HASH, FakeSubstrate
from tests.harness.samples import ALICE, ALICE_HOT, BOB, BOB_HOT, dev_wallet

PURE = BOB_HOT


@pytest.fixture
def preview_context(tmp_path, monkeypatch):
    for variable in (
        "BTCLI_CONFIG",
        "BTCLI_PROXIES_PATH",
        "BTCLI_ADDRESSES_PATH",
        "BTCLI_MULTISIGS_PATH",
        "BTCLI_MULTISIG_CACHE",
        "BTCLI_SUBNET_NAMES_CACHE",
        "BTCLI_TOKEN_SYMBOLS_CACHE",
        "BTCLI_PROTECTED_PATH",
        "BTCLI_PROXY_ANNOUNCEMENTS_PATH",
    ):
        monkeypatch.setenv(variable, str(tmp_path / f"{variable}.json"))
    fake = FakeSubstrate()
    fake.mev_key = bytes(1184)
    fake.seed("MevShield", "NextKey", [], fake.mev_key)
    for account, tao in ((ALICE, 1), (ALICE_HOT, 1), (BOB, 10), (PURE, 100)):
        fake.seed("System", "Account", [account], {"data": {"free": tao * 10**9}})
    fake.seed(
        "Proxy", "Proxies", [PURE], ([{"delegate": ALICE, "proxy_type": "Any", "delay": 10}], 0)
    )
    wallets = {"alice": dev_wallet(), "bob": dev_wallet("//Bob", "//Bob//hot")}
    monkeypatch.setattr(context, "Client", lambda network, **kw: Client(network, substrate=fake))
    monkeypatch.setattr(
        context.AppContext,
        "wallet",
        lambda self: wallets.get(self.wallet_name, SimpleNamespace(name=self.wallet_name)),
    )
    monkeypatch.setattr(
        context.wallets, "list_wallets", lambda *_: {name: ["default"] for name in wallets}
    )
    monkeypatch.setattr(context.wallets, "open_wallet", lambda name, *_: wallets[name])
    monkeypatch.setattr(
        multisig_helpers,
        "local_signatory_wallets",
        lambda _ctx, signatories: [
            (name, wallet.coldkeypub.ss58_address)
            for name, wallet in wallets.items()
            if wallet.coldkeypub.ss58_address in signatories
        ],
    )
    monkeypatch.setattr(multisig_helpers, "_has_coldkey_secret", lambda *_: True)
    record = {
        "genesis_hash": GENESIS_HASH,
        "controller_wallet": "alice",
        "controller_ss58": ALICE,
        "protected_ss58": PURE,
        "delay_blocks": 10,
        "allows": [],
    }
    pa.upsert_protected(record)
    ctx = context.AppContext(
        "local",
        "alice",
        "default",
        str(tmp_path),
        True,
        True,
        Output(json_mode=True),
        wallet_given=True,
    )
    plans = []
    monkeypatch.setattr(ctx.output, "plan", lambda plan, **_: plans.append(plan))
    return ctx, fake, plans, record


@pytest.mark.parametrize("shield", [None, False, True])
def test_delayed_dry_run_resolves_all_from_pure_and_keeps_controller_fee_payer(
    preview_context, shield
):
    ctx, fake, plans, _record = preview_context
    ctx.mev_shield = shield
    fee_payers = []
    estimate = fake.estimate_fee

    async def track_fee(call, keypair, **kwargs):
        fee_payers.append(keypair.ss58_address)
        return await estimate(call, keypair, **kwargs)

    fake.estimate_fee = track_fee
    intent = AddStake(ALICE_HOT, 1, "all")
    expected, _, _ = asyncio.run(compose_announceable_call(fake, intent, PURE, dev_wallet()))

    assert ctx.submit(intent) is None

    assert len(plans) == 1 and plans[0].ok
    plan = plans[0]
    assert plan.call == expected
    assert plan.call.params["amount_staked"] > 99 * 10**9
    assert (plan.call.module, plan.call.function) == ("SubtensorModule", "add_stake_limit")
    assert plan.signer_address == ALICE
    assert plan.shielded is (shield is not False)
    assert any(PURE in effect and "announcement required" in effect for effect in plan.effects)
    assert fee_payers and set(fee_payers) == {ALICE}
    assert fake.submissions == []
    assert ann.load_announcements() == []


@pytest.mark.parametrize("source", ["argument", "context", "config"])
def test_explicit_self_dry_run_still_previews_controller(preview_context, source):
    ctx, fake, plans, _record = preview_context
    kwargs = {}
    if source == "argument":
        kwargs["proxy_for"] = "self"
    elif source == "context":
        ctx.proxy_for = "self"
    else:
        config.set_value("proxy_for", "self")

    ctx.submit(AddStake(ALICE_HOT, 1, "all"), **kwargs)

    assert 0 < plans[0].call.params["amount_staked"] < 10**9
    assert plans[0].signer_address == ALICE
    assert fake.submissions == []


def test_delayed_dry_run_keeps_mandatory_shield_policy(preview_context):
    ctx, fake, plans, _record = preview_context
    ctx.mev_shield = False
    with pytest.raises(typer.Exit) as error:
        ctx.submit(AddCollateral(netuid=1, hotkey_ss58=ALICE_HOT, amount_alpha=1))
    assert error.value.exit_code == 2
    assert plans == []
    assert fake.submissions == []


@pytest.mark.parametrize("multisig", [False, True])
def test_delayed_transfer_preflight_uses_pure_balance(preview_context, multisig):
    ctx, fake, plans, record = preview_context
    if multisig:
        config.add_multisig({"name": "team", "threshold": 2, "signatories": [ALICE, BOB]})
        ctx.wallet_name = "team"
        ctx.signatory_wallet = "alice"
        delegate = ctx._saved_multisig_address("team")
        pa.upsert_protected({**record, "controller_wallet": "team", "controller_ss58": delegate})
        fake.seed(
            "Proxy",
            "Proxies",
            [PURE],
            ([{"delegate": delegate, "proxy_type": "Any", "delay": 10}], 0),
        )

    ctx.submit(Transfer(BOB, 5))

    assert len(plans) == 1 and plans[0].ok
    assert plans[0].signer_address == ALICE
    call = plans[0].call
    if multisig:
        assert (call.module, call.function) == ("Multisig", "as_multi")
        call = call.params["call"]
    assert (call.module, call.function) == ("Balances", "transfer_keep_alive")
    assert call.params["value"] == 5 * 10**9
    assert fake.submissions == []


@pytest.mark.parametrize("source", ["argument", "context", "config", "instant"])
def test_dry_run_preserves_explicit_and_instant_proxy_routes(preview_context, source):
    ctx, fake, plans, _record = preview_context
    target = BOB
    kwargs = {}
    if source == "argument":
        kwargs["proxy_for"] = target
    elif source == "context":
        ctx.proxy_for = target
    elif source == "config":
        config.set_value("proxy_for", target)
    else:
        target = PURE
        fake.seed(
            "Proxy",
            "Proxies",
            [PURE],
            (
                [
                    {"delegate": ALICE, "proxy_type": "Any", "delay": 10},
                    {"delegate": ALICE, "proxy_type": "Staking", "delay": 0},
                ],
                0,
            ),
        )

    ctx.submit(AddStake(ALICE_HOT, 1, "all"), **kwargs)

    call = plans[0].call
    assert (call.module, call.function) == ("Proxy", "proxy")
    assert call.params["real"] == target
    assert call.params["call"].params["amount_staked"] > (99 if source == "instant" else 9) * 10**9
    assert plans[0].signer_address == ALICE
    assert fake.submissions == []


# PR #3305: choosing a protected wallet must not send funds directly from its controller.
# PROTECTED_REGRESSION: changes require explicit human approval.
@pytest.mark.parametrize("saved_multisig", [False, True])
def test_prompt_selected_protected_wallet_announces_instead_of_transferring(
    preview_context, monkeypatch, saved_multisig
):
    ctx, fake, _plans, record = preview_context
    selected = "alice"
    if saved_multisig:
        selected = "team"
        config.add_multisig({"name": selected, "threshold": 2, "signatories": [ALICE, BOB]})
        delegate = ctx._saved_multisig_address(selected)
        record = {**record, "controller_wallet": selected, "controller_ss58": delegate}
        pa.upsert_protected(record)
        fake.seed(
            "Proxy",
            "Proxies",
            [PURE],
            ([{"delegate": delegate, "proxy_type": "Any", "delay": 10}], 0),
        )
        ctx.signatory_wallet = "alice"
    ctx.wallet_name = "bob"
    ctx.wallet_given = False
    ctx.assume_yes = False
    ctx.dry_run = False
    ctx.mev_shield = False
    prompts = []
    monkeypatch.setattr(prompt, "interactive", lambda _ctx: True)

    def select_wallet(_console, app_ctx, spec):
        prompts.append(spec.field)
        assert spec.field == "wallet"
        return spec.parse(app_ctx, selected), selected

    monkeypatch.setattr(prompt, "ask", select_wallet)
    monkeypatch.setattr(ctx, "confirm", lambda *_a, **_kw: None)
    # The fake records actual Client/Executor submissions without changing storage.
    # Reconciliation is immaterial to whether the signed call transfers or announces.
    monkeypatch.setattr(protected, "_reconcile_pending", lambda *_: [])

    assert ctx.submit(Transfer(BOB, 0.5)) is None

    assert prompts == ["wallet"]
    assert len(fake.submissions) == 1
    call, signer, _ = fake.submissions[0]
    assert signer == ALICE
    if saved_multisig:
        assert (call.module, call.function) == ("Multisig", "as_multi")
        call = call.params["call"]
    assert (call.module, call.function) == ("Proxy", "announce")
    assert call.params["real"] == PURE
    rows = ann.list_for_wallet(selected, genesis_hash=GENESIS_HASH)
    assert len(rows) == 1
    assert rows[0]["inner_op"] == "transfer"
    assert rows[0]["inner_args"]["dest_ss58"] == BOB


def test_hotkey_dry_run_ignores_coldkey_protection(preview_context):
    ctx, fake, plans, _record = preview_context
    ctx.submit(SetWeights(netuid=1, weights={0: 1.0}))
    assert plans[0].signer_address == ALICE_HOT
    assert (plans[0].call.module, plans[0].call.function) == (
        "SubtensorModule",
        "set_mechanism_weights",
    )
    assert fake.submissions == []
