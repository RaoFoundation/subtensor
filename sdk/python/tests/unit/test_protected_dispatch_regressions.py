"""Protected accounts preserve fee-payer selection and registered hotkey dispatch."""

from __future__ import annotations

import pytest
import typer

from bittensor import config
from bittensor.cli import context, multisig_helpers, protected
from bittensor.cli.output import Output
from bittensor.client import Client
from bittensor.intents import ExecuteProxyAnnounced, SetWeights
from tests.harness.fake_substrate import FakeSubstrate
from tests.harness.samples import ALICE, ALICE_HOT, BOB, BOB_HOT, dev_wallet


@pytest.fixture
def dispatch_context(tmp_path, monkeypatch):
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
    for account in (ALICE, ALICE_HOT, BOB, BOB_HOT):
        fake.seed("System", "Account", [account], {"data": {"free": 100 * 10**9}})
    wallets = {"alice": dev_wallet(), "bob": dev_wallet("//Bob", "//Bob//hot")}
    monkeypatch.setattr(context, "Client", lambda network, **kw: Client(network, substrate=fake))
    monkeypatch.setattr(context.AppContext, "wallet", lambda self: wallets[self.wallet_name])
    monkeypatch.setattr(
        multisig_helpers,
        "local_signatory_wallets",
        lambda app_ctx, signatories: [
            (name, wallet.coldkeypub.ss58_address)
            for name, wallet in wallets.items()
            if wallet.coldkeypub.ss58_address in signatories
        ],
    )
    monkeypatch.setattr(multisig_helpers, "_has_coldkey_secret", lambda *_: True)
    config.add_multisig({"name": "team", "threshold": 2, "signatories": [ALICE, BOB]})
    app_ctx = context.AppContext(
        "local",
        "team",
        "default",
        str(tmp_path),
        True,
        False,
        Output(json_mode=True),
        mev_shield=False,
        wallet_given=True,
    )
    return app_ctx, fake


# PR #3305: multisig protected withdrawals must dispatch once.
# PROTECTED_REGRESSION: changes require explicit human approval.
@pytest.mark.parametrize("selection", ["explicit", "default", "repeated"])
def test_multisig_announced_withdrawal_uses_one_member_fee_payer(dispatch_context, selection):
    app_ctx, fake = dispatch_context
    if selection != "default":
        app_ctx.signatory_wallet = "alice"
        app_ctx.signatory_wallets = ["alice", "bob"] if selection == "repeated" else ["alice"]
    delegate = app_ctx._saved_multisig_address("team")
    intent = ExecuteProxyAnnounced(
        delegate_ss58=delegate,
        real_ss58=BOB_HOT,
        inner_op="transfer",
        inner_args={"dest_ss58": BOB, "amount_tao": 5},
        force_proxy_type="Any",
    )

    result = app_ctx.submit(intent, proxy_for="self")

    assert result is not None and result.success
    assert len(fake.submissions) == 1
    call, signer, _options = fake.submissions[0]
    assert (call.module, call.function) == ("Proxy", "proxy_announced")
    assert signer == ALICE
    assert call.params["delegate"] == delegate
    assert call.params["real"] == BOB_HOT
    transfer = call.params["call"]
    assert (transfer.module, transfer.function) == ("Balances", "transfer_keep_alive")
    assert transfer.params == {"dest": BOB, "value": 5 * 10**9}
    assert app_ctx.multisig_wallet_name is None
    assert app_ctx._multisig_rounds_active is False


def test_announced_execution_rejects_signatory_for_non_multisig(dispatch_context):
    app_ctx, fake = dispatch_context
    app_ctx.wallet_name = "alice"
    app_ctx.signatory_wallet = "bob"
    with pytest.raises(typer.Exit) as error:
        app_ctx.submit(
            ExecuteProxyAnnounced(
                delegate_ss58=ALICE,
                real_ss58=BOB_HOT,
                inner_op="transfer",
                inner_args={"dest_ss58": BOB, "amount_tao": 5},
            ),
            proxy_for="self",
        )
    assert error.value.exit_code == 2
    assert fake.submissions == []


# PR #3305: coldkey protection must preserve validator hotkey weight submission.
# PROTECTED_REGRESSION: changes require explicit human approval.
@pytest.mark.parametrize("dry_run", [False, True])
def test_protected_coldkey_keeps_weights_on_registered_hotkey(
    dispatch_context, monkeypatch, dry_run
):
    app_ctx, fake = dispatch_context
    app_ctx.wallet_name = "alice"
    app_ctx.dry_run = dry_run
    fake.seed_default("SubtensorModule", "Uids", None)
    fake.seed("SubtensorModule", "Uids", [1, ALICE_HOT], 0)
    monkeypatch.setattr(
        protected.pa,
        "get_by_controller",
        lambda *_args, **_kwargs: {
            "controller_wallet": "alice",
            "controller_ss58": ALICE,
            "protected_ss58": BOB_HOT,
            "delay_blocks": 10800,
            "allows": ["Weights"],
        },
    )
    # A protected coldkey must not even perform protected-origin discovery for
    # a hotkey operation: the registered hotkey remains the dispatch identity.
    monkeypatch.setattr(
        protected,
        "_get_protected_record",
        lambda *_args, **_kwargs: pytest.fail("hotkey routed through coldkey protection"),
        raising=False,
    )
    plans = []
    monkeypatch.setattr(app_ctx.output, "plan", lambda plan, **kw: plans.append(plan))

    result = app_ctx.submit(SetWeights(netuid=1, weights={0: 1.0}))

    if dry_run:
        assert result is None
        assert fake.submissions == []
        assert len(plans) == 1 and plans[0].ok
        call = plans[0].call
        assert plans[0].signer_address == ALICE_HOT
    else:
        assert result is not None and result.success
        assert len(fake.submissions) == 1
        call, signer, _options = fake.submissions[0]
        assert signer == ALICE_HOT
    assert (call.module, call.function) == ("SubtensorModule", "set_mechanism_weights")
    assert call.params["netuid"] == 1
    assert call.params["dests"] == [0]
    assert call.params["weights"] == [65535]
