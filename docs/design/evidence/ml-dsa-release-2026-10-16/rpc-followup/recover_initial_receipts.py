"""Read-only recovery of first finalized 32-transfer fixture evidence."""

import argparse
import asyncio
import hashlib
import json
from pathlib import Path
from urllib.parse import urlparse

import bittensor as bt


async def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--endpoint", default="ws://127.0.0.1:20545")
    parser.add_argument(
        "--output",
        type=Path,
        default=Path(
            "/tmp/ms-release-validation/rpc-followup/ethereum-initial-recovered.json"
        ),
    )
    args = parser.parse_args()
    endpoint = urlparse(args.endpoint)
    assert endpoint.hostname in ("localhost", "127.0.0.1") and endpoint.port in (
        20544,
        20545,
        20546,
    )
    source = Path(
        "/tmp/ms-release-validation/rpc-followup/ethereum-pending-results.initial.json"
    )
    prior = json.loads(source.read_text())
    report = {
        "status": "running",
        "source_result_sha256": hashlib.sha256(source.read_bytes()).hexdigest(),
        "source_result": str(source),
        "script_sha256": hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
    }
    try:
        async with bt.Client(
            args.endpoint, fallback_endpoints=[], archive_endpoints=[]
        ) as client:
            rpc = client._substrate.raw._session.request
            assert await rpc("chain_getBlockHash", [0]) == prior["genesis_hash"]
            assert await rpc("system_chainType", []) in ("Local", "Development")
            receipts = [
                await rpc("eth_getTransactionReceipt", [h])
                for h in prior["submitted_hashes"]
            ]
            report["receipts"] = receipts
            assert all(r and r["status"] == "0x1" for r in receipts)
            blocks = {
                str(n): await rpc("eth_getBlockByNumber", [hex(n), False])
                for n in sorted({int(r["blockNumber"], 16) for r in receipts})
            }
            report["ethereum_blocks"] = blocks
            assert all(
                r["blockHash"] == blocks[str(int(r["blockNumber"], 16))]["hash"]
                for r in receipts
            )
            final_hash = await rpc("chain_getFinalizedHead", [])
            header = await rpc("chain_getHeader", [final_hash])
            height = int(header["number"], 16)
            assert height >= max(map(int, blocks))
            report["finalized_substrate_hash"] = final_hash
            report["finalized_height"] = height
            report["sender_native"] = await client._substrate.query(
                "System", "Account", [prior["sender_mirror"]], block_hash=final_hash
            )
            report["recipient_native"] = await client._substrate.query(
                "System", "Account", [prior["recipient_mirror"]], block_hash=final_hash
            )
            report["sender_eth_nonce"] = int(
                await rpc("eth_getTransactionCount", [prior["sender"], hex(height)]), 16
            )
            report["recipient_eth_balance_wei"] = int(
                await rpc("eth_getBalance", [prior["recipient"], hex(height)]), 16
            )
            receipt_fee = sum(
                int(r["gasUsed"], 16) * int(r["effectiveGasPrice"], 16)
                for r in receipts
            )
            base_fee = sum(
                int(r["gasUsed"], 16)
                * int(blocks[str(int(r["blockNumber"], 16))]["baseFeePerGas"], 16)
                for r in receipts
            )
            actual_fee = (
                prior["funding_rao"]
                - prior["requested_count"] * prior["value_per_transaction_rao"]
                - int(report["sender_native"]["data"]["free"])
            )
            report["fee_accounting"] = {
                "receipt_reported_fee_wei": receipt_fee,
                "block_base_fee_charge_wei": base_fee,
                "actual_debit_rao": actual_fee,
                "confirmed_preexisting_receipt_mismatch": receipt_fee != base_fee,
                "policy": "Subtensor runner explicitly ignores priority fee and charges only base fee",
            }
            assert actual_fee * 10**9 == base_fee and receipt_fee > base_fee
            assert report["sender_eth_nonce"] == prior["requested_count"]
            assert (
                report["recipient_eth_balance_wei"]
                == (
                    prior["requested_count"] * prior["value_per_transaction_rao"]
                    - prior["existential_deposit_rao"]
                )
                * 10**9
            )
            report["status"] = "passed_with_confirmed_preexisting_receipt_mismatch"
    except Exception as error:
        report["status"] = "failed"
        report["error"] = f"{type(error).__name__}: {error}"
        raise
    finally:
        args.output.write_text(json.dumps(report, indent=2) + "\n")
        print(
            json.dumps({"status": report["status"], "output": str(args.output)}),
            flush=True,
        )


if __name__ == "__main__":
    asyncio.run(main())
