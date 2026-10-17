"""Capture finalized, live six-wallet transactions for independent codec checks.

Usage: E2E_WALLET_ENDPOINT=ws://127.0.0.1:19944 python SCRIPT OUTPUT
Requires an isolated localnet with registered-account authorization activated.
"""

import asyncio
import json
import os
import sys
from pathlib import Path

import bittensor as bt
from bittensor._transport.runtime import strip_option_opaque_metadata
from bittensor.receiving import receiving_address
from bittensor.sp_core import Keypair
from tests.harness.samples import dev_wallet


async def main():
    async with bt.Client(
        os.environ["E2E_WALLET_ENDPOINT"], fallback_endpoints=[], archive_endpoints=[]
    ) as client:
        assert await client.query(bt.storage.Sudo.Key) == dev_wallet().coldkey.ss58_address
        keys = [Keypair.create_from_seed(os.urandom(32), code) for code in (0, 1, 4, 6, 7, 8)]
        classical_deposit = int(
            await client.constant(bt.constants.HashedAccounts.RegistrationDeposit)
        )
        mldsa_deposit = int(
            await client.constant(bt.constants.HashedAccounts.MlDsaRegistrationDeposit)
        )
        for key in keys:
            deposit = (
                mldsa_deposit
                if key.crypto_type in (7, 8)
                else classical_deposit
                if key.crypto_type in (4, 6)
                else 0
            )
            result = await client.execute(
                bt.intents.Transfer(receiving_address(key), 10),
                dev_wallet("//Charlie"),
                max_registration_deposit=bt.Balance.from_rao(deposit),
                wait_for_finalization=True,
            )
            assert result.success, result.to_dict()
        raw = client._substrate.raw
        rpc = raw._session.request
        blocks, submitted = {}, {}
        for _attempt in range(3):
            call = await client.compose(
                bt.calls.System.remark(remark="0x" + b"live six wallets".hex())
            )
            signed = [await raw.create_signed_extrinsic(call, key, era="00") for key in keys]
            results = await asyncio.gather(
                *[raw.submit_extrinsic(tx, wait_for_finalization=True) for tx in signed]
            )
            for key, tx, result in zip(keys, signed, results):
                assert result.is_success and result.finalized, result
                submitted[tx.extrinsic_hash] = {
                    "crypto_type": key.crypto_type,
                    "hex": tx.data_hex,
                    "block_hash": result.block_hash,
                    "index": result.extrinsic_idx,
                }
                if result.block_hash not in blocks:
                    blocks[result.block_hash] = await rpc("chain_getBlock", [result.block_hash])
            mixed = [
                block_hash
                for block_hash in blocks
                if len(
                    {
                        value["crypto_type"]
                        for value in submitted.values()
                        if value["block_hash"] == block_hash
                    }
                )
                == 6
            ]
            if mixed:
                break
        assert mixed, "six wallet types did not land together in a finalized block"
        block_hash = mixed[0]
        metadata = {}
        for version in (14, 15, 16):
            value = await rpc(
                "state_call",
                [
                    "Metadata_metadata_at_version",
                    "0x" + version.to_bytes(4, "little").hex(),
                    block_hash,
                ],
            )
            inner = strip_option_opaque_metadata(bytes.fromhex(value[2:]))
            assert inner
            metadata[version] = "0x" + inner.hex()
        version = await rpc("state_getRuntimeVersion", [block_hash])
        output = {
            "genesis": await rpc("chain_getBlockHash", [0]),
            "finalized_head": await rpc("chain_getFinalizedHead", []),
            "runtime": version,
            "metadata": metadata,
            "blocks": blocks,
            "submitted": submitted,
            "six_wallet_block": block_hash,
        }
        Path(sys.argv[1]).write_text(json.dumps(output, indent=2) + "\n")
        print(json.dumps({"six_wallet_block": block_hash, "transactions": len(submitted)}))


if __name__ == "__main__":
    asyncio.run(main())
