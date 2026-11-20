"""Safe composite identities survive creation, signing stages and local restore."""

from types import SimpleNamespace
from unittest.mock import AsyncMock, Mock

import pytest
from typer.testing import CliRunner

from bittensor import config
from bittensor._transport.codec import multisig_account
from bittensor.account_mode import multisig_call
from bittensor.cli import context as cli_context
from bittensor.cli import multisig_helpers as helpers
from bittensor.cli.commands.upgrade import _resolve_sudo_signer_set
from bittensor.cli.main import app
from bittensor.cli.output import Output
from bittensor.client import Client
from bittensor.hashed import compose_derivative
from bittensor.intents.base import BuiltCall
from bittensor.intents.crowdloan import CreateCrowdloan
from bittensor.intents.multisig import (
    MultisigApprove,
    MultisigCancel,
    MultisigExecute,
    MultisigIntentAdapter,
    MultisigThreshold1,
)
from bittensor.intents.proxy import CreatePureProxy, KillPureProxy
from bittensor.sp_core import Keypair, ss58_encode
from tests.harness.fake_substrate import FakeSubstrate
from tests.harness.samples import ALICE, BOB, dev_wallet


@pytest.fixture
def local_state(tmp_path, monkeypatch):
    for name in ("CONFIG", "MULTISIGS_PATH", "MULTISIG_CACHE", "ADDRESSES_PATH", "PROXIES_PATH"):
        monkeypatch.setenv("BTCLI_" + name, str(tmp_path / (name + ".json")))
    monkeypatch.setenv("BT_WALLET_PATH", str(tmp_path / "wallets"))
    substrate = FakeSubstrate()
    monkeypatch.setattr(
        cli_context, "Client", lambda network, **kw: Client(network, substrate=substrate)
    )
    return substrate


def test_legacy_derivation_golden_and_explicit_safe_identity():
    from hashlib import blake2b

    from bittensor.sp_core import ss58_decode

    legacy = multisig_account([BOB, ALICE], 2)
    keys = sorted(bytes(ss58_decode(s)) for s in [ALICE, BOB])
    # Existing Substrate pallet formula: fixed prefix, compact Vec length, u16 threshold.
    assert (
        legacy.public_key
        == blake2b(
            b"modlpy/utilisuba" + bytes([8]) + b"".join(keys) + b"\x02\x00", digest_size=32
        ).digest()
    )
    safe = multisig_account([ALICE, BOB], 2, account_mode="safe")
    assert safe.account_mode == "safe"
    assert safe.public_key != legacy.public_key
    assert safe == multisig_account([BOB, ALICE], 2, account_mode="safe")
    assert safe.public_key != multisig_account([BOB, ALICE], 1, account_mode="safe").public_key


@pytest.mark.parametrize("code", [0, 1, 4, 6, 7, 8])
def test_safe_identity_accepts_any_member_key_type(code):
    member = Keypair.create_from_seed(bytes([code + 12]) * 32, code)
    assert (
        multisig_account([ALICE, member.ss58_address], 2, account_mode="safe").account_mode
        == "safe"
    )


@pytest.mark.parametrize(
    "members,threshold", [([], 1), ([ALICE, ALICE], 2), ([ALICE], 0), ([ALICE], 2)]
)
def test_safe_identity_rejects_invalid_manifests(members, threshold):
    with pytest.raises(ValueError):
        multisig_account(members, threshold, account_mode="safe")


def _member_accounts(count):
    return [ss58_encode(n.to_bytes(32, "little")) for n in range(1, count + 1)]


@pytest.mark.parametrize("count", [0, 1, 101])
def test_safe_address_rejects_runtime_inaccessible_member_counts(count):
    import bittensor_core

    keys = [n.to_bytes(32, "little") for n in range(1, count + 1)]
    with pytest.raises(ValueError):
        bittensor_core.safe_multisig_account_id(keys, 1)
    with pytest.raises(ValueError):
        multisig_account(_member_accounts(count), 1, account_mode="safe")


@pytest.mark.parametrize("count", [2, 100])
@pytest.mark.parametrize("unanimous", [False, True])
@pytest.mark.asyncio
async def test_safe_address_accepts_runtime_boundaries_and_can_build_funding(count, unanimous):
    import bittensor_core

    threshold = count if unanimous else 1
    keys = [n.to_bytes(32, "little") for n in range(1, count + 1)]
    account, sorted_keys = bittensor_core.safe_multisig_account_id(keys[::-1], threshold)
    derived = multisig_account(_member_accounts(count)[::-1], threshold, account_mode="safe")
    assert bytes(account) == derived.public_key
    assert [bytes(key) for key in sorted_keys] == sorted(keys)
    assert len(derived.signatories) == count
    substrate = FakeSubstrate()
    substrate.query = AsyncMock(side_effect=AssertionError("unexpected authority lookup"))
    client = Client("local", substrate=substrate)
    handle = await client.multisig(_member_accounts(count), threshold, account_mode="safe")
    funding = await handle.funding_call(123)
    assert (funding.module, funding.function) == ("Balances", "transfer_keep_alive")
    assert funding.params == {"dest": derived.ss58_address, "value": 123}
    substrate.query.assert_not_awaited()


@pytest.mark.parametrize("count", [0, 1, 101])
@pytest.mark.asyncio
async def test_invalid_safe_members_cannot_produce_a_funding_handle(count):
    substrate = FakeSubstrate()
    substrate.compose = AsyncMock()
    substrate.query = AsyncMock()
    client = Client("local", substrate=substrate)
    with pytest.raises(ValueError):
        await client.multisig(_member_accounts(count), 1, account_mode="safe")
    substrate.compose.assert_not_awaited()
    substrate.query.assert_not_awaited()
    assert substrate.submissions == []


def test_safe_members_require_distinct_accounts_not_distinct_ss58_encodings():
    from bittensor.sp_core import ss58_decode

    alternate = ss58_encode(bytes(ss58_decode(ALICE)), 0)
    assert alternate != ALICE
    with pytest.raises(ValueError, match="duplicate"):
        multisig_account([ALICE, alternate], 1, account_mode="safe")


@pytest.mark.asyncio
async def test_safe_handle_never_registers_or_queries_authority():
    substrate = FakeSubstrate()
    substrate.query = AsyncMock(side_effect=AssertionError("unexpected authority lookup"))
    client = Client("local", substrate=substrate)
    safe = await client.multisig([ALICE, BOB], 2, account_mode="safe")
    assert (await safe.funding_call(123)).function == "transfer_keep_alive"
    assert await safe.protection_status() == {
        "eligible": False,
        "registered": False,
        "safe_derivation": True,
    }
    with pytest.raises(ValueError, match="do not use permanent"):
        await safe.protection_call(deposit_payer=ALICE)
    assert (await client.multisig([ALICE, BOB], 2)).account_mode == "legacy"


@pytest.mark.parametrize(
    "kind,timepoint,function",
    [
        (MultisigThreshold1, None, "as_multi_threshold_1_safe"),
        (MultisigExecute, None, "as_multi_safe"),
        (MultisigExecute, {"height": 12, "index": 3}, "as_multi_safe"),
        (MultisigApprove, None, "as_multi_safe"),
        (MultisigApprove, {"height": 12, "index": 3}, "approve_as_multi_safe"),
        (MultisigCancel, {"height": 12, "index": 3}, "cancel_as_multi_safe"),
    ],
)
@pytest.mark.asyncio
async def test_safe_mode_reaches_every_approval_stage(kind, timepoint, function):
    substrate = FakeSubstrate()
    substrate.seed("System", "Account", [ALICE], {"data": {"free": 10**15}})
    kwargs = {
        "other_signatories": [BOB],
        "call": {"op": "transfer", "dest_ss58": BOB, "amount_tao": 1},
        "account_mode": "safe",
    }
    if kind is not MultisigThreshold1:
        kwargs.update(threshold=2, timepoint=timepoint)
    built = await kind(**kwargs).build(substrate, dev_wallet())
    call = built.call if isinstance(built, BuiltCall) else built
    assert call.function == function
    if kind is not MultisigThreshold1:
        assert call.params.get("maybe_timepoint", call.params.get("timepoint")) == timepoint
    assert call.params["other_signatories"] == [BOB]


@pytest.mark.asyncio
async def test_pending_lookup_and_nested_origin_keep_safe_mode():
    substrate = FakeSubstrate()
    semantic = CreatePureProxy()
    dispatch = MultisigExecute(
        threshold=2, other_signatories=[BOB], call=semantic.to_dict(), account_mode="safe"
    )
    adapter = MultisigIntentAdapter(dispatch, semantic)
    view = adapter.origin_view(substrate, dev_wallet())
    expected = multisig_account([ALICE, BOB], 2, account_mode="safe")
    assert view.ss58_address == expected.ss58_address
    built = await semantic.build(substrate, view)
    assert built.call.function == "create_safe_pure"
    assert (
        await compose_derivative(substrate, view, 3, built.call)
    ).function == "as_safe_derivative"
    substrate.seed(
        "Multisig",
        "Multisigs",
        [expected.ss58_address, built.call.call_hash],
        {"when": {"height": 88, "index": 3}, "approvals": [BOB]},
    )
    wrapped = await adapter.wrap_call(substrate, dev_wallet(), built.call)
    assert wrapped.call.function == "as_multi_safe"
    assert wrapped.call.params["maybe_timepoint"] == {"height": 88, "index": 3}


@pytest.mark.parametrize("mode", ["safe", "legacy"])
def test_cli_creation_persists_mode_and_offline_restore(local_state, mode):
    options = [] if mode == "safe" else ["--account-mode", "legacy"]
    result = CliRunner().invoke(
        app,
        [
            "multisig",
            "add",
            "treasury",
            "--threshold",
            "2",
            "--signatories",
            f"{ALICE},{BOB}",
            *options,
            "--json",
        ],
    )
    assert result.exit_code == 0, result.output or str(result.exception)
    entry = config.get_multisig("treasury")
    assert entry["account_mode"] == mode
    context = SimpleNamespace(resolve_account=lambda kind, ref: ref)
    assert (
        helpers.derive_saved_multisig_address(context, "treasury")
        == multisig_account([ALICE, BOB], 2, account_mode=mode).ss58_address
    )


@pytest.mark.parametrize("count", [1, 101])
@pytest.mark.parametrize("overwrite", [False, True])
def test_cli_invalid_safe_creation_preserves_saved_config(local_state, count, overwrite):
    existing = {
        "name": "treasury" if overwrite else "untouched",
        "threshold": 2,
        "signatories": [ALICE, BOB],
        "account_mode": "safe",
        "note": "Preserve this entry",
    }
    config.save_multisigs([existing])
    before = config.multisigs_path().read_bytes()
    result = CliRunner().invoke(
        app,
        [
            "multisig",
            "add",
            "treasury",
            "--threshold",
            "1",
            "--signatories",
            ",".join(_member_accounts(count)),
            "--account-mode",
            "safe",
            *(["--overwrite"] if overwrite else []),
            "--json",
        ],
    )
    assert result.exit_code != 0, result.output
    assert "signator" in result.output.lower()
    assert config.multisigs_path().read_bytes() == before
    assert local_state.submissions == []


@pytest.mark.parametrize("count", [0, 1, 101])
def test_imported_invalid_safe_preset_cannot_resolve_or_receive_funding(
    local_state, monkeypatch, count
):
    invalid = {
        "name": "broken",
        "threshold": 1,
        "signatories": _member_accounts(count),
        "account_mode": "safe",
    }
    valid = {
        "name": "usable",
        "threshold": 2,
        "signatories": [ALICE, BOB],
        "account_mode": "safe",
    }
    # Restore/import bypasses add-time validation, so each use must still validate.
    config.save_multisigs([invalid, valid])
    before = config.multisigs_path().read_bytes()
    context = cli_context.AppContext(
        network="local",
        wallet_name="default",
        hotkey_name="default",
        wallet_path=str(config.multisigs_path().parent / "wallets"),
        assume_yes=True,
        dry_run=False,
        output=Output(json_mode=True),
    )
    with pytest.raises(ValueError):
        helpers.derive_saved_multisig_address(context, "broken")
    submit = Mock(side_effect=AssertionError("invalid preset reached transaction submission"))
    monkeypatch.setattr(cli_context.AppContext, "submit", submit)
    result = CliRunner().invoke(
        app,
        ["--json", "--yes", "wallet", "transfer", "--dest", "broken", "--amount", "1"],
    )
    assert result.exit_code != 0, result.output
    assert "cannot resolve multisig" in result.output
    submit.assert_not_called()
    assert local_state.submissions == []
    expected = multisig_account([ALICE, BOB], 2, account_mode="safe").ss58_address
    assert helpers.saved_multisig_accounts(context) == [("usable", expected)]
    rows = {row["name"]: row for row in helpers.multisig_list_records(context)}
    assert rows["broken"]["ss58"] is None
    assert rows["usable"]["ss58"] == expected
    assert config.multisigs_path().read_bytes() == before


def test_old_config_and_manifest_keep_legacy_identity(local_state):
    config.add_multisig({"name": "old", "threshold": 2, "signatories": [ALICE, BOB]})
    context = SimpleNamespace(resolve_account=lambda kind, ref: ref)
    assert helpers.saved_account_mode("old") == "legacy"
    assert (
        helpers.derive_saved_multisig_address(context, "old")
        == multisig_account([ALICE, BOB], 2).ss58_address
    )
    _resolve_sudo_signer_set(
        context,
        multisig=None,
        multisig_threshold=None,
        signatories=None,
        other_signatories=None,
        manifest={"sudo": {"threshold": 2, "signatories": [ALICE, BOB], "account_mode": "safe"}},
    )
    assert context.multisig_account_mode == "safe"


@pytest.mark.parametrize(
    "stored_mode,stored_address,accepted",
    [("safe", ALICE, True), ("legacy", ALICE, False), ("safe", BOB, False)],
)
@pytest.mark.asyncio
async def test_recovery_cache_requires_matching_mode_and_account(
    local_state, stored_mode, stored_address, accepted
):
    config.save_multisig_cache_entry(
        "0x" + "01" * 32,
        {
            "target": "System.remark",
            "multisig_address": stored_address,
            "account_mode": stored_mode,
        },
    )
    result = await helpers.resolve_call_spec(
        Mock(), call_hash="0x" + "01" * 32, multisig_address=ALICE, account_mode="safe"
    )
    assert bool(result) is accepted


def test_replay_and_deposit_refresh_preserve_safe_mode():
    command = helpers.build_replay_command(
        SimpleNamespace(network="local"),
        target="System.remark",
        params={},
        args_file=None,
        sudo=False,
        threshold=2,
        signatories=[ALICE, BOB],
        wallet_label="bob",
        account_mode="safe",
    )
    assert "--multisig-account-mode safe" in command
    assert (
        multisig_call(
            "poke_deposit",
            account_mode="safe",
            threshold=2,
            other_signatories=[BOB],
            call_hash=b"\0" * 32,
        ).function
        == "poke_deposit_safe"
    )


@pytest.mark.asyncio
@pytest.mark.parametrize("crypto_type", [8, 9, 11])
async def test_fixed_wallet_defaults_safe_pure_and_derivative_with_explicit_legacy(crypto_type):
    substrate = FakeSubstrate()
    key = Keypair.create_from_seed(b"\x77" * 32, crypto_type)
    wallet = SimpleNamespace(coldkey=key, coldkeypub=key, hotkey=key)
    built = await CreatePureProxy().build(substrate, wallet)
    assert built.call.function == "create_safe_pure"
    assert built.extras["pure_proxy_account_mode"] == "safe"
    with pytest.warns(UserWarning, match="Legacy pure proxy"):
        assert (
            await CreatePureProxy(account_mode="legacy").build(substrate, wallet)
        ).function == "create_pure"
    assert (
        await compose_derivative(substrate, key, 3, built.call)
    ).function == "as_safe_derivative"
    with pytest.warns(UserWarning, match="Legacy derivative"):
        assert (
            await compose_derivative(substrate, key, 3, built.call, account_mode="legacy")
        ).function == "as_derivative"
    kill = await KillPureProxy(
        spawner_ss58=key.ss58_address, index=3, height=88, ext_index=2, account_mode="safe"
    ).build(substrate, wallet)
    assert kill.function == "kill_safe_pure"
    assert kill.params["height"] == 88 and kill.params["ext_index"] == 2


def _crowdloan_with_pure_proxy(**proxy_args):
    return CreateCrowdloan(
        1,
        1,
        10,
        1000,
        call={
            "op": "batch",
            "intents": [
                {"op": "create_pure_proxy", **proxy_args},
                {"op": "register_leased_network", "emissions_share": 50},
            ],
        },
    )


@pytest.mark.parametrize("creator", ["fixed_key", "fixed_wallet", "safe_1", "safe_2", "sr"])
@pytest.mark.parametrize("crypto_type", [8, 9, 11])
@pytest.mark.asyncio
async def test_crowdloan_stored_batch_preserves_creator_derivation(
    creator, crypto_type, tmp_path, monkeypatch
):
    from bittensor.wallet import Wallet

    substrate = FakeSubstrate()
    substrate.seed("System", "Account", [ALICE], {"data": {"free": 10**15}})
    intent = _crowdloan_with_pure_proxy()
    wallet = dev_wallet()
    if creator.startswith("fixed"):
        key = Keypair.create_from_seed(b"\x77" * 32, crypto_type)
        wallet = key
        if creator == "fixed_wallet":
            wallet = Wallet("creator", path=str(tmp_path))
            wallet.coldkeypub_file.set_keypair(key.public_only(), encrypt=False)
            monkeypatch.setattr(
                Wallet, "coldkey", property(lambda _: pytest.fail("must not unlock creator"))
            )
    elif creator.startswith("safe"):
        kwargs = dict(other_signatories=[BOB], call=intent.to_dict(), account_mode="safe")
        intent = (
            MultisigThreshold1(**kwargs)
            if creator == "safe_1"
            else MultisigExecute(threshold=2, **kwargs)
        )
    built = await intent.build(substrate, wallet)
    call = built.call if isinstance(built, BuiltCall) else built
    if creator.startswith("safe"):
        assert call.module == "Multisig" and call.function.endswith("_safe")
        call = call.params["call"]
    assert (call.module, call.function) == ("Crowdloan", "create")
    stored = call.params["call"]
    assert (stored.module, stored.function) == ("Utility", "batch_all")
    pure, lease = stored.params["calls"]
    assert pure.module == "Proxy"
    assert pure.function == ("create_pure" if creator == "sr" else "create_safe_pure")
    assert (lease.module, lease.function) == ("SubtensorModule", "register_leased_network")
    assert lease.params == {"emissions_share": 50, "end_block": None}


@pytest.mark.asyncio
async def test_crowdloan_fixed_ms_explicit_legacy_pure_warns():
    key = Keypair.create_from_seed(b"\x77" * 32, 8)
    with pytest.warns(UserWarning, match="Legacy pure proxy"):
        call = await _crowdloan_with_pure_proxy(account_mode="legacy").build(FakeSubstrate(), key)
    assert call.params["call"].params["calls"][0].function == "create_pure"


def test_native_safe_derivation_shared_runtime_golden():
    import bittensor_core

    account, keys = bittensor_core.safe_multisig_account_id([bytes([n]) * 32 for n in [3, 1, 2]], 2)
    assert (
        bytes(account).hex() == "ac41577a3fbc8615cae86a48a256f1878be1ea6b4c59301dbe1ceeb90324602e"
    )
    assert [bytes(k) for k in keys] == [bytes([n]) * 32 for n in [1, 2, 3]]


@pytest.mark.parametrize("threshold", [1, 2])
@pytest.mark.asyncio
async def test_transport_builds_safe_call_and_excludes_signing_member(threshold):
    from bittensor._transport.interface import SubstrateConnection

    key = dev_wallet().coldkey
    account = multisig_account([ALICE, BOB], threshold, account_mode="safe")
    codec = Mock()
    codec.call_hash.return_value = b"\x01" * 32
    connection = SimpleNamespace(
        _runtimes=SimpleNamespace(codec_at=AsyncMock(return_value=codec)),
        get_payment_info=AsyncMock(return_value={"weight": {"ref_time": 10, "proof_size": 0}}),
        query=AsyncMock(return_value={"when": {"height": 88, "index": 3}}),
        create_signed_extrinsic=AsyncMock(),
        ss58_format=42,
    )
    await SubstrateConnection.create_multisig_extrinsic(connection, b"inner", key, account)
    module, function, params = codec.compose_call.call_args.args
    assert module == "Multisig"
    assert function == ("as_multi_safe" if threshold == 2 else "as_multi_threshold_1_safe")
    assert params["other_signatories"] == [BOB]
    if threshold == 2:
        assert params["maybe_timepoint"] == {"height": 88, "index": 3}
    else:
        assert set(params) == {"other_signatories", "call"}


@pytest.mark.asyncio
async def test_opening_extrinsic_recovery_keeps_safe_mode():
    inner = {
        "call_module": "System",
        "call_function": "remark",
        "call_args": [{"name": "remark", "value": "0x00"}],
    }
    client = SimpleNamespace(
        block_info=AsyncMock(
            return_value=SimpleNamespace(
                extrinsics=[
                    {
                        "call": {
                            "call_module": "Multisig",
                            "call_function": "as_multi_safe",
                            "call_args": [{"name": "call", "value": inner}],
                        }
                    }
                ]
            )
        )
    )
    spec = await helpers.fetch_call_from_timepoint(client, height=9, index=0)
    assert spec["account_mode"] == "safe"
    assert spec["target"] == "System.remark"


def test_safe_pure_book_restore_keeps_mode(local_state):
    result = CliRunner().invoke(
        app,
        [
            "proxy",
            "book",
            "add",
            "--name",
            "restored",
            "--address",
            BOB,
            "--spawner",
            ALICE,
            "--height",
            "88",
            "--ext-index",
            "3",
            "--account-mode",
            "safe",
            "--json",
        ],
    )
    assert result.exit_code == 0, result.output or str(result.exception)
    assert config.get_proxy("restored")["account_mode"] == "safe"


@pytest.mark.asyncio
async def test_shielded_safe_pure_receipt_preserves_creation_coordinates():
    from bittensor.executor import Executor
    from bittensor.result import ExtrinsicResult

    substrate = FakeSubstrate()
    substrate.seed("System", "Account", [ALICE], {"data": {"free": 10**15}})
    executor = Executor(substrate)
    receipt = ExtrinsicResult(
        success=True,
        extrinsic_id="88-0003",
        data={"pure_proxy_account_mode": "safe"},
        events=[
            {
                "event": {
                    "module_id": "Proxy",
                    "event_id": "PureCreated",
                    "attributes": {"pure": BOB, "who": ALICE},
                }
            }
        ],
    )
    executor._submit_encrypted_call = AsyncMock(return_value=receipt)
    result = await executor.execute(
        CreatePureProxy(account_mode="safe"), dev_wallet(), shielded=True
    )
    assert result.data == {
        "pure_proxy_account_mode": "safe",
        "pure_proxy": BOB,
        "spawner": ALICE,
        "height": 88,
        "ext_index": 3,
    }


def test_safe_pure_creation_hint_preserves_restore_parameters(local_state, monkeypatch):
    from bittensor.result import ExtrinsicResult

    monkeypatch.setattr(
        cli_context.AppContext,
        "submit",
        lambda self, intent: ExtrinsicResult(
            success=True,
            data={
                "pure_proxy": BOB,
                "spawner": ALICE,
                "pure_proxy_account_mode": "safe",
                "height": 88,
                "ext_index": 3,
            },
        ),
    )
    result = CliRunner().invoke(app, ["proxy", "create", "--account-mode", "safe", "--index", "7"])
    assert result.exit_code == 0, result.output or str(result.exception)
    text = " ".join(result.output.split())
    for argument in ("--account-mode safe", "--height 88", "--ext-index 3", "--index 7"):
        assert argument in text


@pytest.mark.parametrize("stored_mode", [None, "legacy", "safe"])
def test_cli_overwrite_preserves_existing_group_identity(local_state, stored_mode):
    entry = {"name": "treasury", "threshold": 2, "signatories": [ALICE, BOB]}
    if stored_mode is not None:
        entry["account_mode"] = stored_mode
    config.add_multisig(entry)
    result = CliRunner().invoke(
        app,
        [
            "multisig",
            "add",
            "treasury",
            "--threshold",
            "2",
            "--signatories",
            f"{ALICE},{BOB}",
            "--overwrite",
            "--note",
            "Updated label",
            "--json",
        ],
    )
    assert result.exit_code == 0, result.output or str(result.exception)
    assert config.get_multisig("treasury")["account_mode"] == (stored_mode or "legacy")
