"""Public Python facades and portable core share exact protected-account decisions."""

from bittensor._policy import evaluate_policy
from bittensor.protected_account import parse_delay_to_blocks, resolve_allow_item
from bittensor.protected_funding import funding_budget


def test_protected_funding_matches_portable_core_without_float_rounding():
    inputs = {
        "controller": "controller",
        "account": {"data": {"free": 9007199254740993, "reserved": 0}},
        "announcement_count": 0,
        "announcement_deposit": 20,
        "deposit_base": 10,
        "deposit_factor": 2,
        "existential_deposit": 1,
        "announce_fee": 3,
        "transfer_execution_fee": 4,
        "staking_execution_fee": 8,
        "fee_payer": "controller",
    }
    actual = funding_budget(**inputs).to_dict()
    core = evaluate_policy(
        "protected.funding", {**inputs, "signature_estimate": "standard sr25519 envelope"}
    )
    assert actual == {
        name: int(value) if name.endswith("_rao") else value for name, value in core.items()
    }
    assert actual["free_rao"] == 9007199254740993
    assert actual["target_rao"] == 171


def test_protected_delay_and_allow_mapping_share_core_rules():
    for raw in ("30s", "42s", "36h", "2d", 17):
        assert parse_delay_to_blocks(raw) == evaluate_policy(
            "protected.delay", {"raw": raw, "block_time": 12}
        )
    assert (
        resolve_allow_item("gamma")
        == evaluate_policy("protected.allow_resolution", {"item": "gamma"})["proxy_type"]
    )


def test_upgrade_policy_preserves_optional_checks_and_exact_weight_padding():
    from bittensor.cli.upgrade_helpers import approval_state

    assert evaluate_policy(
        "upgrade.manifest_checks",
        {
            "manifest": {"call_hash": "0xAB", "ci_address": "ci"},
            "call_hash": "0xab",
            "pending": True,
            "ci_address": "other",
            "python_optional": True,
        },
    ) == {"manifest call hash": True, "manifest CI key": False}
    assert (
        approval_state(pending={"approvals": ["a", "b"]}, threshold=3, signer="c")["position"]
        == "final"
    )
    assert evaluate_policy(
        "upgrade.weight_pad", {"ref_time": 18446744073709551615, "proof_size": 10}
    ) == {"ref_time": "20291418481080506776", "proof_size": "11"}
    assert not evaluate_policy("upgrade.digest_matches", {"expected": "bad", "actual": "bad"})


async def test_upgrade_python_runner_retains_optional_manifest_contract(monkeypatch):
    from hashlib import sha256
    from types import SimpleNamespace
    from unittest.mock import AsyncMock

    from bittensor.cli import upgrade_helpers as upgrade

    code = b"independently reproduced runtime"
    bundle = upgrade.ProposalBundle(
        blob=b"proposal", wasm=code, digest={"sha256": sha256(code).hexdigest()}
    )
    bundle.manifest = {"call_hash": bundle.call_hash.upper(), "ci_address": "ci"}
    monkeypatch.setattr(upgrade, "parse_proposal_blob", lambda _: upgrade.ParsedBlob("sudo", code))
    monkeypatch.setattr(
        upgrade, "reconstruct_proxy_call", AsyncMock(return_value=SimpleNamespace(data=bundle.blob))
    )
    monkeypatch.setattr(
        upgrade,
        "find_pending_upgrade",
        AsyncMock(return_value={"ci_address": "ci", "timepoint_display": "1:0"}),
    )
    client = SimpleNamespace(query=AsyncMock(return_value="sudo"))
    result = await upgrade.run_proposal_checks(client, bundle)
    assert result.ok
    assert [check.name for check in result.checks][-2:] == ["manifest call hash", "manifest CI key"]
    # Python's optional manifest has no TS-only mandatory spec/commit/asset fields.
    bundle.manifest["ci_address"] = "different"
    assert [
        check.name for check in (await upgrade.run_proposal_checks(client, bundle)).failed()
    ] == ["manifest CI key"]
    bundle.manifest.pop("ci_address")
    bundle.digest = {"sha256": "bad"}
    assert [
        check.name for check in (await upgrade.run_proposal_checks(client, bundle)).failed()
    ] == ["srtool digest"]


async def test_upgrade_signer_and_weight_facades_use_shared_decisions():
    from types import SimpleNamespace
    from unittest.mock import AsyncMock

    from bittensor.cli.upgrade_helpers import finalizing_max_weight, sorted_other_signatories
    from bittensor.sp_core import ss58_encode

    first, second, signer = [ss58_encode(bytes([value]) * 32) for value in (1, 2, 3)]
    assert sorted_other_signatories([second, signer, first, second], signer) == [first, second]
    client = SimpleNamespace(
        estimate_weight=AsyncMock(return_value={"ref_time": 18446744073709551615, "proof_size": 10})
    )
    assert await finalizing_max_weight(client, object(), signer) == {
        "ref_time": 20291418481080506776,
        "proof_size": 11,
    }
