"""Saved announced payloads retain the runtime's nested-batch safety boundary."""

from __future__ import annotations

from copy import deepcopy
from dataclasses import replace

import pytest

from bittensor import calls
from bittensor.executor import Executor, _compose_intent_call
from bittensor.fee_filters import _arg_value, _call_dict
from bittensor.hashed import descriptor_value
from bittensor.intents import Batch, Transfer
from bittensor.intents.crowdloan import FinalizeCrowdloan
from bittensor.intents.multisig import MultisigExecute, MultisigIntentAdapter, MultisigThreshold1
from bittensor.intents.proxy import ExecuteProxyAnnounced, compose_announceable_call
from bittensor.intents.staking import AddStake
from bittensor.receiving import receiving_address
from bittensor.sp_core import CRYPTO_HASHED, Keypair
from tests.conftest import golden_codec
from tests.harness.fake_substrate import FakeSubstrate
from tests.harness.samples import ALICE, BOB, BOB_HOT, dev_wallet

PURE = BOB_HOT


def _decoded(value):
    decoded = _call_dict(value)
    if decoded is not None:
        return {
            **decoded,
            "call_args": [
                {**argument, "value": _decoded(argument["value"])}
                for argument in decoded["call_args"]
            ],
        }
    if isinstance(value, dict):
        return {key: _decoded(item) for key, item in value.items()}
    if isinstance(value, list):
        return [_decoded(item) for item in value]
    return value


class _ReplayChain(FakeSubstrate):
    """Model batch_all's inherited filter and Proxy's non-propagated inner failure.

    The proxy consumes its announcement before dispatch, then reports any inner
    failure in ProxyExecuted while returning outer success. Thus the outer batch
    can commit its later transfer unless the SDK rejects it before submission.
    """

    def __init__(self):
        super().__init__()
        self.saved = {}
        self.pending = set()
        self.balances = {ALICE: 100 * 10**9, PURE: 100 * 10**9, BOB: 0}
        self.proxy_events = []
        self.decode_error = False
        self.guard_valid = True
        self.crowdloan_calls = {}
        self.finalized = set()

    def remember(self, call):
        self.saved[bytes(call.data)] = _decoded(call)
        self.pending.add(bytes(call.call_hash))
        return "0x" + bytes(call.data).hex()

    async def decode_scale(self, type_string, data):
        assert type_string == "Call"
        if self.decode_error:
            raise ValueError("metadata cannot decode the saved call")
        raw = bytes.fromhex(data.removeprefix("0x")) if isinstance(data, str) else bytes(data)
        return deepcopy(self.saved[raw])

    def _dispatch(self, call, origin, *, batch_filtered=False):
        decoded = _call_dict(call)
        if decoded is None:
            decoded = self.saved[bytes(call.data)]
        pair = (decoded["call_module"], decoded["call_function"])
        if pair == ("Utility", "batch_all"):
            if batch_filtered:
                return False
            balances, pending = dict(self.balances), set(self.pending)
            for child in _arg_value(decoded, "calls"):
                if not self._dispatch(child, origin, batch_filtered=True):
                    self.balances, self.pending = balances, pending
                    return False
            return True
        if pair == ("Proxy", "proxy_announced"):
            inner = _arg_value(decoded, "call")
            commitment = bytes(inner.call_hash)
            assert commitment in self.pending
            self.pending.remove(commitment)
            succeeded = self._dispatch(
                inner, _arg_value(decoded, "real"), batch_filtered=batch_filtered
            )
            self.proxy_events.append(
                {
                    "event": {
                        "module_id": "Proxy",
                        "event_id": "ProxyExecuted",
                        "attributes": {
                            "result": {"Ok": None} if succeeded else {"Err": "CallFiltered"}
                        },
                    }
                }
            )
            return True
        if pair[0] == "Multisig":
            # The current runtime creates a fresh signed origin for multisig.
            # ProxyExecuted still captures its inner failure without bubbling it.
            self._dispatch(_arg_value(decoded, "call"), origin, batch_filtered=False)
            return True
        if pair == ("Crowdloan", "finalize"):
            crowdloan_id = _arg_value(decoded, "crowdloan_id")
            succeeded = self._dispatch(
                self.crowdloan_calls[crowdloan_id], origin, batch_filtered=batch_filtered
            )
            if succeeded:
                self.finalized.add(crowdloan_id)
            return succeeded
        if pair == ("Balances", "transfer_keep_alive"):
            amount = _arg_value(decoded, "value")
            destination = _arg_value(decoded, "dest")
            self.balances[origin] -= amount
            self.balances[destination] = self.balances.get(destination, 0) + amount
            return True
        assert pair == ("HashedAccounts", "check_registered")
        return self.guard_valid

    async def submit(self, call, keypair, **kwargs):
        result = await super().submit(call, keypair, **kwargs)
        assert self._dispatch(call, keypair.ss58_address)
        return replace(result, events=list(self.proxy_events))


@pytest.fixture
def replay_chain():
    chain = _ReplayChain()
    chain.seed_default("System", "Account", {"data": {"free": 100 * 10**9, "frozen": 0}})
    chain.seed("AdminUtils", "HashedAccountsEnabled", [], True)
    chain.seed_constant("HashedAccounts", "RegistrationDeposit", 200_000_000)
    key = Keypair.create_from_seed(bytes([91]) * 32, CRYPTO_HASHED)
    chain.seed(
        "HashedAccounts",
        "Accounts",
        [key.ss58_address],
        {
            "descriptor": descriptor_value(bytes(key.hashed_descriptor)),
            "generation": 0,
            "commitment": key.hashed_current_commitment,
        },
    )
    return chain, key


async def _announced(chain, inner, *, saved, stale_metadata=False):
    call, _hash, _extras = await compose_announceable_call(chain, inner, PURE, dev_wallet())
    payload = chain.remember(call)
    metadata = Transfer(BOB, 99) if stale_metadata else inner
    encoded = metadata.to_dict()
    return ExecuteProxyAnnounced(
        delegate_ss58=ALICE,
        real_ss58=PURE,
        inner_op=encoded.pop("op"),
        inner_args=encoded,
        announced_call_data=payload if saved else None,
    ), call


# PR #3305: a failed nested guarded payment must not consume its announcement
# while later transfers commit.
# PROTECTED_REGRESSION: changes require explicit human approval.
@pytest.mark.parametrize("saved", [False, True])
@pytest.mark.parametrize("wrapper", ["direct", "threshold_1", "multisig"])
async def test_outer_batch_preserves_announcement_and_funds_when_payment_contains_guard(
    replay_chain, saved, wrapper
):
    chain, key = replay_chain
    announced, call = await _announced(chain, Transfer(receiving_address(key), 5), saved=saved)
    child = announced
    if wrapper == "threshold_1":
        child = MultisigThreshold1(other_signatories=[BOB], call=announced.to_dict())
    elif wrapper == "multisig":
        child = MultisigExecute(
            threshold=2,
            other_signatories=[BOB],
            call=announced.to_dict(),
            timepoint={"height": 10, "index": 0},
        )
    before = dict(chain.balances)
    # Model the check failing at dispatch (for example after a reorg), also
    # covering multisig wrappers whose fresh origin drops the outer filter.
    chain.guard_valid = False
    rejected = None
    try:
        await Executor(chain).execute(
            Batch([child, Transfer(BOB, 7)]), dev_wallet(), shielded=False
        )
    except ValueError as error:
        rejected = error

    assert bytes(call.call_hash) in chain.pending, "the payment's announcement was consumed"
    assert chain.balances == before, "a later transfer committed without the announced payment"
    assert rejected is not None
    assert chain.submissions == []


# PR #3305: a pinned multisig must still inspect saved announced bytes before batching payments.
# PROTECTED_REGRESSION: changes require explicit human approval.
async def test_pinned_multisig_cannot_skip_saved_announcement_batch_safety(replay_chain):
    chain, key = replay_chain
    child, inner = await _announced(
        chain, Transfer(receiving_address(key), 5), saved=True, stale_metadata=True
    )
    semantic = Batch([child, Transfer(BOB, 7)])
    proxy = await chain.compose(
        calls.Proxy.proxy_announced(delegate=ALICE, real=PURE, force_proxy_type=None, call=inner)
    )
    transfer = await chain.compose(calls.Balances.transfer_keep_alive(dest=BOB, value=7 * 10**9))
    pinned = await chain.compose(calls.Utility.batch_all(calls=[proxy, transfer]))
    adapter = MultisigIntentAdapter(
        dispatch=MultisigThreshold1(other_signatories=[BOB], call=semantic.to_dict()),
        semantic=semantic,
        inner_call_data="0x" + bytes(pinned.data).hex(),
    )
    with pytest.raises(ValueError, match=r"direct|batch"):
        await _compose_intent_call(chain, adapter, dev_wallet())
    assert bytes(inner.call_hash) in chain.pending
    assert not chain.submissions


# PR #3305: legacy rebuilt announcements must not bypass nested-batch safety
# when a multisig approval pins the outer batch bytes.
# PROTECTED_REGRESSION: changes require explicit human approval.
async def test_pinned_multisig_cannot_skip_rebuilt_announcement_batch_safety(replay_chain):
    chain, _key = replay_chain
    child, inner = await _announced(
        chain, Batch([Transfer(BOB, 1), Transfer(ALICE, 2)]), saved=False
    )
    semantic = Batch([child, Transfer(BOB, 7)])
    proxy = await chain.compose(
        calls.Proxy.proxy_announced(delegate=ALICE, real=PURE, force_proxy_type=None, call=inner)
    )
    transfer = await chain.compose(calls.Balances.transfer_keep_alive(dest=BOB, value=7 * 10**9))
    pinned = await chain.compose(calls.Utility.batch_all(calls=[proxy, transfer]))
    adapter = MultisigIntentAdapter(
        dispatch=MultisigThreshold1(other_signatories=[BOB], call=semantic.to_dict()),
        semantic=semantic,
        inner_call_data="0x" + bytes(pinned.data).hex(),
    )
    with pytest.raises(ValueError, match=r"direct|batch"):
        await _compose_intent_call(chain, adapter, dev_wallet())
    assert bytes(inner.call_hash) in chain.pending
    assert not chain.submissions


async def test_pinned_rebuilt_announcement_retains_approved_dynamic_staking_inputs(replay_chain):
    chain, _key = replay_chain
    child, original = await _announced(chain, AddStake(BOB, 1, "all"), saved=False)
    pinned, _extras = await _compose_intent_call(chain, child, dev_wallet())
    approved_amount = original.params["amount_staked"]
    approved_price = original.params["limit_price"]
    payload = chain.remember(pinned)
    chain.seed("System", "Account", [PURE], {"data": {"free": 2 * 10**9, "frozen": 0}})
    chain.seed_runtime("SwapRuntimeApi", "current_alpha_price", 2 * 10**9)
    rebuilt, _hash, _extras = await compose_announceable_call(
        chain, AddStake(BOB, 1, "all"), PURE, dev_wallet()
    )
    assert rebuilt.params["amount_staked"] != approved_amount
    assert rebuilt.params["limit_price"] != approved_price
    adapter = MultisigIntentAdapter(
        dispatch=MultisigThreshold1(other_signatories=[BOB], call=child.to_dict()),
        semantic=child,
        inner_call_data=payload,
    )

    outer, _extras = await _compose_intent_call(chain, adapter, dev_wallet())

    assert bytes(outer.params["call"].data) == bytes(pinned.data)
    assert not chain.submissions


@pytest.mark.parametrize("saved", [False, True])
async def test_plain_inner_atomic_batch_also_requires_direct_announced_execution(
    replay_chain, saved
):
    chain, _key = replay_chain
    child, _call = await _announced(
        chain, Batch([Transfer(BOB, 1), Transfer(ALICE, 2)]), saved=saved
    )
    with pytest.raises(ValueError, match=r"direct|batch"):
        await _compose_intent_call(chain, Batch([child, Transfer(BOB, 7)]), dev_wallet())
    assert not chain.submissions


@pytest.mark.parametrize("stale_metadata", [False, True])
async def test_saved_guard_is_detected_from_payload_despite_missing_or_stale_metadata(
    replay_chain, stale_metadata
):
    chain, key = replay_chain
    child, _call = await _announced(
        chain, Transfer(receiving_address(key), 5), saved=True, stale_metadata=stale_metadata
    )
    with pytest.raises(ValueError, match=r"direct|batch"):
        await _compose_intent_call(chain, Batch([child, Transfer(BOB, 7)]), dev_wallet())


@pytest.mark.parametrize("guarded", [False, True])
async def test_saved_direct_execution_keeps_exact_bytes_and_pays_original_amount(
    replay_chain, guarded
):
    chain, key = replay_chain
    recipient = receiving_address(key) if guarded else BOB
    child, original = await _announced(
        chain, Transfer(recipient, 5), saved=True, stale_metadata=True
    )

    result = await Executor(chain).execute(child, dev_wallet(), shielded=False)

    assert result.success
    assert bytes(chain.submissions[0][0].params["call"].data) == bytes(original.data)
    assert chain.balances[PURE] == 95 * 10**9
    assert chain.balances[key.ss58_address if guarded else BOB] == 5 * 10**9
    assert bytes(original.call_hash) not in chain.pending


async def test_decoded_plain_saved_transfer_remains_safe_to_batch(replay_chain):
    chain, _key = replay_chain
    child, _call = await _announced(chain, Transfer(BOB, 5), saved=True)
    result = await Executor(chain).execute(
        Batch([child, Transfer(BOB, 7)]), dev_wallet(), shielded=False
    )
    assert result.success
    assert chain.balances[PURE] == 95 * 10**9
    assert chain.balances[ALICE] == 93 * 10**9
    assert chain.balances[BOB] == 12 * 10**9


@pytest.mark.parametrize("failure", ["unavailable", "malformed", "no_decoder"])
async def test_opaque_legacy_saved_payload_can_replay_directly_but_cannot_bypass_batch_guard(
    replay_chain, failure
):
    chain, _key = replay_chain
    child, original = await _announced(chain, Transfer(BOB, 5), saved=True)
    if failure == "unavailable":
        chain.decode_error = True
    elif failure == "malformed":

        async def malformed(_type, _data):
            return {}

        chain.decode_scale = malformed
    else:
        chain.decode_scale = None
    direct, _extras = await _compose_intent_call(chain, child, dev_wallet())
    assert bytes(direct.params["call"].data) == bytes(original.data)
    with pytest.raises(ValueError, match=r"direct|batch|inspect"):
        await _compose_intent_call(chain, Batch([child, Transfer(BOB, 7)]), dev_wallet())


@pytest.mark.parametrize(
    "wrapper", ["batch_all", "proxy", "multisig", "batch", "force_batch", "if_else"]
)
async def test_real_scale_nested_atomic_payload_requires_direct_submission(replay_chain, wrapper):
    chain, _key = replay_chain
    codec = golden_codec()
    leaf = codec.compose_call(*calls.Balances.transfer_keep_alive(dest=BOB, value=5))
    call = codec.compose_call(*calls.Utility.batch_all(calls=[leaf]))
    if wrapper == "proxy":
        call = codec.compose_call(*calls.Proxy.proxy(real=PURE, force_proxy_type=None, call=call))
    elif wrapper == "multisig":
        call = codec.compose_call(
            *calls.Multisig.as_multi_threshold_1(other_signatories=[BOB], call=call)
        )
    elif wrapper in {"batch", "force_batch"}:
        call = codec.compose_call("Utility", wrapper, {"calls": [leaf, call]})
    elif wrapper == "if_else":
        call = codec.compose_call(*calls.Utility.if_else(main=leaf, fallback=call))

    async def decode(_type, data):
        return codec.decode_call(data)

    chain.decode_scale = decode
    child = ExecuteProxyAnnounced(
        delegate_ss58=ALICE,
        real_ss58=PURE,
        inner_op="transfer",
        inner_args=Transfer(BOB, 1).to_dict(),
        announced_call_data="0x" + bytes(call.data).hex(),
    )
    with pytest.raises(ValueError, match=r"direct|batch"):
        await _compose_intent_call(chain, Batch([child, Transfer(BOB, 7)]), dev_wallet())


async def test_real_scale_plain_saved_transfer_remains_batchable(replay_chain):
    chain, _key = replay_chain
    codec = golden_codec()
    saved = codec.compose_call(*calls.Balances.transfer_keep_alive(dest=BOB, value=123))

    async def decode(_type, data):
        return codec.decode_call(data)

    chain.decode_scale = decode
    child = ExecuteProxyAnnounced(
        delegate_ss58=ALICE,
        real_ss58=PURE,
        inner_op="transfer",
        inner_args=Transfer(BOB, 99).to_dict(),
        announced_call_data="0x" + bytes(saved.data).hex(),
    )
    outer, _ = await _compose_intent_call(chain, Batch([child, Transfer(BOB, 7)]), dev_wallet())
    assert outer.function == "batch_all"
    assert bytes(outer.params["calls"][0].params["call"].data) == bytes(saved.data)


# PR #3305: a stored crowdloan batch must not consume an announcement
# and let later transfers commit.
# PROTECTED_REGRESSION: changes require explicit human approval.
@pytest.mark.parametrize("saved", [False, True])
async def test_announced_stateful_finalization_is_direct_only(replay_chain, saved):
    chain, _key = replay_chain
    child, original = await _announced(chain, FinalizeCrowdloan(crowdloan_id=7), saved=saved)
    payment = await chain.compose(calls.Balances.transfer_keep_alive(dest=BOB, value=3 * 10**9))
    chain.crowdloan_calls[7] = await chain.compose(calls.Utility.batch_all(calls=[payment]))
    before = dict(chain.balances)
    # The stored RuntimeCall is absent from the announced bytes; it may be an
    # atomic batch that inherits an enclosing batch's rejection filter.
    rejected = None
    try:
        await Executor(chain).execute(
            Batch([child, Transfer(BOB, 7)]), dev_wallet(), shielded=False
        )
    except ValueError as error:
        rejected = error
    assert bytes(original.call_hash) in chain.pending
    assert chain.balances == before
    assert not chain.finalized
    assert rejected is not None
    assert not chain.submissions

    result = await Executor(chain).execute(child, dev_wallet(), shielded=False)
    assert result.success
    assert bytes(chain.submissions[0][0].params["call"].data) == bytes(original.data)
    assert chain.finalized == {7}
    assert chain.balances[PURE] == 97 * 10**9
    assert chain.balances[BOB] == 3 * 10**9
