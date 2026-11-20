"""Raw rotation targets retain the local name needed for protected-wallet routing."""

from __future__ import annotations

import asyncio
import json
from copy import deepcopy
from hashlib import blake2b
from pathlib import Path
from types import SimpleNamespace

import pytest
import typer

from bittensor import config
from bittensor import protected_account as pa
from bittensor import proxy_announcements as ann
from bittensor.cli import multisig_helpers, prompt, protected
from bittensor.cli.context import AppContext
from bittensor.intents import AnnounceProxyCall, ExecuteProxyAnnounced, Transfer
from bittensor.receiving import parse_recipient, receiving_address
from bittensor.sp_core import Keypair, serialized_keypair_to_keyfile_data
from bittensor.wallet import Wallet

GENESIS = "0x" + "a8" * 32
OTHER_GENESIS = "0x" + "b8" * 32


class _Chain:
    def __init__(self, controller, pure):
        self._substrate = self
        self.pure = pure
        self.height = 100
        self.grants = [{"delegate": controller, "proxy_type": "Any", "delay": 10}]
        self.announcements = []
        self.submitted = []

    async def block_hash(self, block=0):
        assert block == 0
        return GENESIS

    async def block(self):
        return self.height

    async def read(self, operation, **_kwargs):
        if operation == "proxies":
            return {"proxies": deepcopy(self.grants)}
        assert operation == "proxy_announcements"
        return {"announcements": deepcopy(self.announcements)}

    def _apply(self, encoded):
        if encoded["op"] == "batch":
            for child in encoded["intents"]:
                self._apply(child)
        elif encoded["op"] == "remove_proxies":
            self.grants = []
        else:
            assert encoded["op"] == "add_proxy"
            self.grants.append(
                {
                    "delegate": parse_recipient(encoded["delegate_ss58"]).account,
                    "proxy_type": encoded["proxy_type"],
                    "delay": encoded["delay"],
                }
            )

    def submit(self, intent, **_kwargs):
        self.submitted.append(intent)
        if isinstance(intent, AnnounceProxyCall):
            self.announcements.append(
                {
                    "real": intent.real_ss58,
                    "call_hash": intent.call_hash,
                    "height": self.height,
                }
            )
        else:
            assert isinstance(intent, ExecuteProxyAnnounced)
            payload = bytes.fromhex(intent.announced_call_data.removeprefix("0x"))
            call_hash = "0x" + blake2b(payload, digest_size=32).hexdigest()
            matched = [row for row in self.announcements if row["call_hash"] == call_hash]
            assert matched and self.height >= matched[0]["height"] + 10
            self.announcements = [row for row in self.announcements if row not in matched]
            self._apply(json.loads(payload))
        return SimpleNamespace(success=True, events=[], data={}, extrinsic_id=f"{self.height}-0")


def _write_public(root, name, key):
    directory = Path(root) / name
    directory.mkdir(parents=True, exist_ok=True)
    (directory / "coldkeypub.txt").write_bytes(
        bytes(serialized_keypair_to_keyfile_data(key.public_only()))
    )


@pytest.fixture
def rotation(tmp_path, monkeypatch):
    for variable, filename in [
        ("BTCLI_PROTECTED_PATH", "protected.json"),
        ("BTCLI_PROXY_ANNOUNCEMENTS_PATH", "announcements.json"),
        ("BTCLI_MULTISIGS_PATH", "multisigs.json"),
        ("BTCLI_ADDRESSES_PATH", "addresses.json"),
        ("BTCLI_PROXIES_PATH", "proxies.json"),
        ("BTCLI_CONFIG", "config.json"),
    ]:
        monkeypatch.setenv(variable, str(tmp_path / filename))
    old = Keypair.create_from_seed(bytes([121]) * 32, 1)
    pure = Keypair.create_from_seed(bytes([122]) * 32, 1)
    root = tmp_path / "wallets"
    _write_public(root, "old", old)
    chain = _Chain(old.ss58_address, pure.ss58_address)
    messages, errors = [], []
    output = SimpleNamespace(
        message=messages.append,
        error=lambda message, **_kwargs: errors.append(message),
        name_address=lambda *_a: None,
        classify_address=lambda *_a: None,
    )
    ctx = AppContext(
        network="test",
        wallet_name="old",
        hotkey_name="default",
        wallet_path=str(root),
        assume_yes=True,
        dry_run=False,
        output=output,
    )
    monkeypatch.setattr(ctx, "run", lambda callback: asyncio.run(callback(chain)))
    monkeypatch.setattr(ctx, "submit", chain.submit)
    monkeypatch.setattr(prompt, "confirm_wallet", lambda *_a, **_kw: ctx.wallet_name)
    # No destination private file exists. Fail explicitly if reverse lookup or
    # transaction preparation attempts to unlock one anyway.
    monkeypatch.setattr(
        Wallet, "coldkey", property(lambda _self: pytest.fail("private key access"))
    )

    async def compose(_substrate, intent, _real, _wallet):
        payload = json.dumps(intent.to_dict(), sort_keys=True, default=str).encode()
        digest = blake2b(payload, digest_size=32).digest()
        return SimpleNamespace(data=payload, call_hash=digest), "0x" + digest.hex(), {}

    monkeypatch.setattr(protected, "compose_announceable_call", compose)
    record = {
        "controller_wallet": "old",
        "controller_ss58": old.ss58_address,
        "protected_ss58": pure.ss58_address,
        "delay_blocks": 10,
        "genesis_hash": GENESIS,
        "allows": [],
        "pending_allows": [],
    }
    pa.upsert_protected(record)
    return ctx, chain, record, root, old, pure, errors


def _execute_rotation(ctx, chain):
    row = ann.list_for_wallet("old", genesis_hash=GENESIS)[0]
    chain.height = 110
    protected.run_execute_protected(ctx, index=row["index"])
    return row


def _assert_subsequent_wallet_routing(ctx, name, pure):
    ctx.wallet_name = name
    route = protected.maybe_route_protected(
        ctx,
        Transfer(dest_ss58=pure, amount_tao=1),
        proxy_for=None,
        user_set_proxy=False,
    )
    assert route == (None, None, True)
    pending = ann.list_for_wallet(name, genesis_hash=GENESIS)
    assert pending[-1]["real_ss58"] == pure
    assert pending[-1]["controller_wallet"] == name
    assert pending[-1]["inner_op"] == "transfer"


# PR #3305: a rotation reviewed against an empty queue must stop if the queue changes.
# PROTECTED_REGRESSION: changes require explicit human approval.
def test_rotation_execute_refuses_a_later_announcement(rotation):
    ctx, chain, _record, root, _old, _pure, errors = rotation
    new = Keypair.create_from_seed(bytes([123]) * 32, 1)
    _write_public(root, "new", new)
    protected.run_rotate(ctx, to_wallet="new")
    rotation_row = ann.list_for_wallet("old", genesis_hash=GENESIS)[0]
    chain.announcements.append(
        {
            "real": chain.pure,
            "call_hash": "0x" + "44" * 32,
            "height": chain.height + 1,
        }
    )
    chain.height = 111

    with pytest.raises(typer.Exit) as rejected:
        protected.run_execute_protected(ctx, index=rotation_row["index"])

    assert rejected.value.exit_code == 2
    assert len(chain.submitted) == 1
    assert isinstance(chain.submitted[0], AnnounceProxyCall)
    assert pa.get_by_controller("old", genesis_hash=GENESIS) is not None
    assert errors and "queue changed" in errors[-1]


@pytest.mark.parametrize("crypto_type,full_address", [(1, False), (6, False), (6, True)])
def test_raw_rotation_executes_and_routes_again_by_actual_wallet_name(
    rotation, crypto_type, full_address
):
    ctx, chain, _record, root, _old, pure, _errors = rotation
    new = Keypair.create_from_seed(bytes([123]) * 32, crypto_type)
    _write_public(root, "new", new)
    target = receiving_address(new) if full_address else new.ss58_address

    protected.run_rotate(ctx, to_wallet=target)
    queued = ann.list_for_wallet("old", genesis_hash=GENESIS)[0]
    assert queued["rotate_to_wallet"] == "new"
    assert queued["rotate_to_ss58"] == new.ss58_address
    if crypto_type == 6:
        assert queued["inner_args"]["intents"][1]["delegate_ss58"] == receiving_address(new)
    _execute_rotation(ctx, chain)

    moved = pa.get_by_controller("new", genesis_hash=GENESIS)
    assert moved is not None and moved["controller_ss58"] == new.ss58_address
    assert pa.get_by_controller(target, genesis_hash=GENESIS) is None
    assert pa.get_by_controller("old", genesis_hash=GENESIS) is None
    _assert_subsequent_wallet_routing(ctx, "new", pure.ss58_address)


@pytest.mark.parametrize("account_mode", ["legacy", "safe"])
def test_raw_multisig_rotation_binds_saved_preset_and_routes_again(rotation, account_mode):
    ctx, chain, _record, root, old, pure, _errors = rotation
    member = Keypair.create_from_seed(bytes([124]) * 32, 1)
    _write_public(root, "member", member)
    config.add_multisig(
        {
            "name": "committee",
            "threshold": 2,
            "signatories": [old.ss58_address, member.ss58_address],
            "account_mode": account_mode,
        }
    )
    destination = multisig_helpers.derive_saved_multisig_address(ctx, "committee")

    protected.run_rotate(ctx, to_wallet=destination)
    assert ann.list_for_wallet("old", genesis_hash=GENESIS)[0]["rotate_to_wallet"] == "committee"
    _execute_rotation(ctx, chain)

    assert pa.get_by_controller("committee", genesis_hash=GENESIS)["controller_ss58"] == destination
    assert pa.get_by_controller(destination, genesis_hash=GENESIS) is None
    _assert_subsequent_wallet_routing(ctx, "committee", pure.ss58_address)


@pytest.mark.parametrize("full_address", [False, True])
def test_raw_finalize_uses_actual_local_name_after_external_execution(rotation, full_address):
    ctx, chain, record, root, _old, pure, _errors = rotation
    new = Keypair.create_from_seed(bytes([125]) * 32, 6)
    _write_public(root, "new", new)
    pa.upsert_protected(
        {**record, "pending_rotate": {"to_wallet": "another", "to_ss58": pure.ss58_address}}
    )
    chain.grants = [{"delegate": new.ss58_address, "proxy_type": "Any", "delay": 10}]
    target = receiving_address(new) if full_address else new.ss58_address

    protected.run_rotate_finalize(ctx, to_wallet=target)

    assert pa.get_by_controller("new", genesis_hash=GENESIS)["controller_ss58"] == new.ss58_address
    assert pa.get_by_controller(target, genesis_hash=GENESIS) is None
    assert pa.get_by_controller("another", genesis_hash=GENESIS) is None
    _assert_subsequent_wallet_routing(ctx, "new", pure.ss58_address)


@pytest.mark.parametrize("finalize", [False, True])
@pytest.mark.parametrize("problem", ["missing", "ambiguous", "shadowed"])
def test_raw_rotation_refuses_unusable_local_binding_without_mutation(rotation, finalize, problem):
    ctx, chain, record, root, old, pure, errors = rotation
    new = Keypair.create_from_seed(bytes([126]) * 32, 1)
    if problem == "ambiguous":
        _write_public(root, "first", new)
        _write_public(root, "second", new)
    elif problem == "shadowed":
        _write_public(root, "committee", new)
        config.add_multisig(
            {
                "name": "committee",
                "threshold": 2,
                "signatories": [old.ss58_address, pure.ss58_address],
            }
        )
    before = pa.protected_path().read_bytes()
    chain.grants = [{"delegate": new.ss58_address, "proxy_type": "Any", "delay": 10}]

    with pytest.raises(typer.Exit) as error:
        if finalize:
            protected.run_rotate_finalize(ctx, to_wallet=new.ss58_address)
        else:
            protected.run_rotate(ctx, to_wallet=new.ss58_address)

    assert error.value.exit_code == 2
    assert errors and "rotation destination" in errors[-1]
    assert pa.protected_path().read_bytes() == before
    assert ann.load_announcements() == []
    assert chain.submitted == []
    assert pa.get_by_controller("old", genesis_hash=GENESIS) == record


@pytest.mark.parametrize("finalize", [False, True])
def test_raw_destination_respects_existing_binding_conflict(rotation, finalize):
    ctx, chain, record, root, _old, pure, errors = rotation
    new = Keypair.create_from_seed(bytes([127]) * 32, 1)
    _write_public(root, "new", new)
    pa.upsert_protected(
        {
            **record,
            "controller_wallet": "new",
            "controller_ss58": new.ss58_address,
            "protected_ss58": new.ss58_address,
        }
    )
    before = pa.protected_path().read_bytes()
    if finalize:
        chain.grants = [{"delegate": new.ss58_address, "proxy_type": "Any", "delay": 10}]

    with pytest.raises(typer.Exit):
        if finalize:
            protected.run_rotate_finalize(ctx, to_wallet=new.ss58_address)
        else:
            protected.run_rotate(ctx, to_wallet=new.ss58_address)

    assert errors
    assert pa.protected_path().read_bytes() == before
    assert ann.load_announcements() == []
    assert chain.submitted == []
    assert pa.get_by_controller("old", genesis_hash=GENESIS)["protected_ss58"] == pure.ss58_address


def test_same_local_destination_on_another_network_does_not_conflict(rotation):
    ctx, chain, record, root, _old, pure, _errors = rotation
    new = Keypair.create_from_seed(bytes([128]) * 32, 1)
    _write_public(root, "new", new)
    other = {
        **record,
        "controller_wallet": "new",
        "controller_ss58": new.ss58_address,
        "protected_ss58": new.ss58_address,
        "genesis_hash": OTHER_GENESIS,
    }
    pa.upsert_protected(other)

    protected.run_rotate(ctx, to_wallet=new.ss58_address)
    _execute_rotation(ctx, chain)

    assert pa.get_by_controller("new", genesis_hash=GENESIS)["protected_ss58"] == pure.ss58_address
    assert pa.get_by_controller("new", genesis_hash=OTHER_GENESIS) == other


def test_explicit_name_disambiguates_duplicate_public_keys(rotation):
    ctx, chain, _record, root, _old, _pure, _errors = rotation
    new = Keypair.create_from_seed(bytes([129]) * 32, 1)
    _write_public(root, "first", new)
    _write_public(root, "second", new)

    protected.run_rotate(ctx, to_wallet="second")
    _execute_rotation(ctx, chain)

    assert (
        pa.get_by_controller("second", genesis_hash=GENESIS)["controller_ss58"] == new.ss58_address
    )
    assert pa.get_by_controller("first", genesis_hash=GENESIS) is None


@pytest.mark.parametrize("book", ["address", "proxy"])
def test_alias_only_destination_maps_to_actual_local_wallet(rotation, book):
    ctx, chain, _record, root, _old, pure, _errors = rotation
    new = Keypair.create_from_seed(bytes([130]) * 32, 1)
    _write_public(root, "new", new)
    add_entry = config.add_address if book == "address" else config.add_proxy
    add_entry({"name": "destination", "address": new.ss58_address})

    protected.run_rotate(ctx, to_wallet="destination")
    _execute_rotation(ctx, chain)

    assert pa.get_by_controller("new", genesis_hash=GENESIS)["protected_ss58"] == pure.ss58_address
    assert pa.get_by_controller("destination", genesis_hash=GENESIS) is None
    _assert_subsequent_wallet_routing(ctx, "new", pure.ss58_address)


@pytest.mark.parametrize("book", ["address", "proxy"])
@pytest.mark.parametrize("raw_target", [False, True])
def test_conflicting_alias_cannot_bind_a_different_signing_wallet(rotation, book, raw_target):
    ctx, chain, _record, root, _old, pure, errors = rotation
    new = Keypair.create_from_seed(bytes([131]) * 32, 1)
    _write_public(root, "new", new)
    add_entry = config.add_address if book == "address" else config.add_proxy
    add_entry({"name": "new", "address": pure.ss58_address})
    before = pa.protected_path().read_bytes()

    with pytest.raises(typer.Exit):
        protected.run_rotate(ctx, to_wallet=new.ss58_address if raw_target else "new")

    assert errors
    assert pa.protected_path().read_bytes() == before
    assert ann.load_announcements() == []
    assert chain.submitted == []


def test_named_matching_alias_still_disambiguates_duplicate_wallets(rotation):
    ctx, chain, _record, root, _old, _pure, _errors = rotation
    new = Keypair.create_from_seed(bytes([132]) * 32, 1)
    _write_public(root, "first", new)
    _write_public(root, "second", new)
    config.add_address({"name": "second", "address": new.ss58_address})

    protected.run_rotate(ctx, to_wallet="second")
    _execute_rotation(ctx, chain)

    assert (
        pa.get_by_controller("second", genesis_hash=GENESIS)["controller_ss58"] == new.ss58_address
    )
    assert pa.get_by_controller("first", genesis_hash=GENESIS) is None


def test_raw_reverse_lookup_rejects_forged_public_file_identity(rotation):
    ctx, chain, _record, root, _old, pure, errors = rotation
    new = Keypair.create_from_seed(bytes([133]) * 32, 6)
    _write_public(root, "new", new)
    public_path = root / "new" / "coldkeypub.txt"
    encoded = json.loads(public_path.read_text())
    encoded["ss58Address"] = pure.ss58_address
    public_path.write_text(json.dumps(encoded))
    before = pa.protected_path().read_bytes()

    with pytest.raises(typer.Exit):
        protected.run_rotate(ctx, to_wallet=pure.ss58_address)

    assert errors and "no matching local wallet" in errors[-1]
    assert pa.protected_path().read_bytes() == before
    assert chain.submitted == []


def test_duplicate_multisig_presets_require_explicit_name(rotation):
    ctx, chain, _record, _root, old, pure, errors = rotation
    for name in ("first", "second"):
        config.add_multisig(
            {
                "name": name,
                "threshold": 2,
                "signatories": [old.ss58_address, pure.ss58_address],
            }
        )
    destination = multisig_helpers.derive_saved_multisig_address(ctx, "first")

    with pytest.raises(typer.Exit):
        protected.run_rotate(ctx, to_wallet=destination)

    assert errors and "multiple local wallets or saved multisigs" in errors[-1]
    assert chain.submitted == []


def test_multisig_reverse_binding_matches_actual_proxy_book_signatory_resolution(rotation):
    ctx, chain, _record, root, old, pure, errors = rotation
    member = Keypair.create_from_seed(bytes([134]) * 32, 1)
    _write_public(root, "member", member)
    config.add_proxy({"name": "member", "address": pure.ss58_address})
    config.add_multisig(
        {
            "name": "committee",
            "threshold": 2,
            "signatories": [old.ss58_address, "member"],
        }
    )
    stale_address = dict(multisig_helpers.saved_multisig_accounts(ctx))["committee"]
    actual_address = multisig_helpers.derive_saved_multisig_address(ctx, "committee")
    assert stale_address != actual_address
    before = pa.protected_path().read_bytes()

    with pytest.raises(typer.Exit):
        protected.run_rotate(ctx, to_wallet=stale_address)

    assert errors and "no matching local wallet" in errors[-1]
    assert pa.protected_path().read_bytes() == before
    assert chain.submitted == []
    protected.run_rotate(ctx, to_wallet=actual_address)
    _execute_rotation(ctx, chain)
    assert (
        pa.get_by_controller("committee", genesis_hash=GENESIS)["controller_ss58"] == actual_address
    )
    _assert_subsequent_wallet_routing(ctx, "committee", pure.ss58_address)


def test_nested_multisig_destination_uses_actual_preset_derivation(rotation):
    ctx, chain, _record, _root, old, pure, _errors = rotation
    config.add_multisig(
        {
            "name": "inner",
            "threshold": 2,
            "signatories": [old.ss58_address, pure.ss58_address],
        }
    )
    config.add_multisig(
        {
            "name": "outer",
            "threshold": 2,
            "signatories": [old.ss58_address, "inner"],
        }
    )
    destination = multisig_helpers.derive_saved_multisig_address(ctx, "outer")

    protected.run_rotate(ctx, to_wallet=destination)
    _execute_rotation(ctx, chain)

    assert pa.get_by_controller("outer", genesis_hash=GENESIS)["controller_ss58"] == destination


@pytest.mark.parametrize("problem", ["unreadable", "encrypted", "cyclic", "malformed"])
def test_unusable_preset_is_skipped_without_keyfile_unlock(rotation, monkeypatch, problem):
    ctx, chain, _record, root, old, pure, _errors = rotation
    _write_public(root, "new", pure)
    unreadable = root / "unreadable"
    unreadable.mkdir()
    (unreadable / "coldkeypub.txt").write_bytes(
        b"$NACL" + b"0" * 80 if problem == "encrypted" else b"not public metadata"
    )
    config.save_multisigs(
        [
            {
                "name": "broken",
                "threshold": 2,
                "signatories": [
                    old.ss58_address,
                    "broken" if problem == "cyclic" else "unreadable",
                ],
            }
        ]
        if problem != "malformed"
        else [{"name": "broken"}]
    )
    from bittensor.keyfiles import Keyfile

    monkeypatch.setattr(Keyfile, "get_keypair", lambda *_a, **_kw: pytest.fail("keyfile unlock"))

    name, _address, account = protected._resolve_rotation_destination(ctx, pure.ss58_address)

    assert (name, account) == ("new", pure.ss58_address)
    assert chain.submitted == []
