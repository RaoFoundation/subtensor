import asyncio
import hashlib
import json
import sys
from pathlib import Path

import bittensor as bt
from bittensor.hashed import registration_descriptor_value
from bittensor.sp_core import Keypair
from tests.harness.samples import dev_wallet

RUN = Path(__file__).parent

async def main():
    name, port = sys.argv[1], int(sys.argv[2])
    async with bt.Client(f"ws://127.0.0.1:{port}", fallback_endpoints=[], archive_endpoints=[]) as c:
        rpc = c._substrate.raw._session.request
        code = bytes.fromhex((await rpc("state_getStorage", ["0x3a636f6465"]))[2:])
        artifact = RUN / "runtime.wasm"
        assert code == artifact.read_bytes()
        assert await c.constant(("Timestamp", "MinimumPeriod")) == 6000
        assert await c.query(("AdminUtils", "HashedAccountsEnabled")) is False
        calls = [
            bt.calls.AdminUtils.sudo_set_hashed_accounts_enabled(enabled=True),
            bt.calls.AdminUtils.sudo_set_network_rate_limit(rate_limit=0),
            bt.calls.AdminUtils.sudo_set_owner_hparam_rate_limit(epochs=0),
            bt.calls.AdminUtils.sudo_set_admin_freeze_window(window=0),
            bt.calls.AdminUtils.sudo_set_tx_rate_limit(tx_rate_limit=0),
            bt.calls.AdminUtils.sudo_set_lock_reduction_interval(interval=1),
        ]
        batch = await c.compose(bt.calls.Utility.batch_all(calls=[await c.compose(call) for call in calls]))
        activation = await c.submit_call(
            bt.calls.Sudo.sudo_unchecked_weight(call=batch, weight={"ref_time": 100000000, "proof_size": 0}),
            dev_wallet(), wait_for_finalization=True,
        )
        assert activation.success, activation.to_dict()
        report = {"head": (RUN / "initial-manifest.json").read_text(), "genesis": await rpc("chain_getBlockHash", [0]),
                  "wasm_sha256": hashlib.sha256(code).hexdigest(), "minimum_period": 6000,
                  "activation": activation.to_dict(), "production_gate_cases": []}
        if name == "production":
            for crypto in (7, 8):
                key = Keypair.create_from_seed(bytes([crypto + 20]) * 32, crypto)
                try:
                    result = await c.submit_call(bt.calls.HashedAccounts.register(
                        descriptor=registration_descriptor_value(bytes(key.hashed_descriptor))), dev_wallet())
                    rejected = result.to_dict()
                    assert not result.success, rejected
                except Exception as error:
                    rejected = str(error)
                assert "resource" in str(rejected).lower() or "weight" in str(rejected).lower(), rejected
                assert await c.query(("HashedAccounts", "MlDsaAccounts"), [key.ss58_address]) is None
                report["production_gate_cases"].append({"crypto_type": crypto, "rejection": rejected})
        (RUN / name / "activation.json").write_text(json.dumps(report, indent=2, default=str) + "\n")
        print(name, "activated and verified", flush=True)

asyncio.run(main())
