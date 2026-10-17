"""Disposable localnet EVM nonce/value overlay and finalized-receipt regression.

Only public addresses, hashes and receipts are persisted. Generated private keys
remain in this process. No flags, weights, runtime code or chain configuration
are modified. The caller owns localnet launch/shutdown.
"""

from __future__ import annotations

import argparse
import asyncio
import hashlib
import json
import time
from datetime import datetime, timezone
from pathlib import Path
from urllib.parse import urlparse

import bittensor as bt
from eth_account import Account

from bittensor.evm.addresses import h160_to_ss58
from tests.harness.samples import dev_wallet

WEI_PER_RAO = 10**9
VALUE_RAO = 10**6
ED_RAO = 500


def arguments():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--endpoint", default="ws://127.0.0.1:21045")
    parser.add_argument("--count", type=int, default=32, choices=(24, 32))
    parser.add_argument("--funder", default="Bob", choices=("Alice", "Bob"))
    parser.add_argument("--timeout", type=float, default=240)
    parser.add_argument(
        "--output",
        type=Path,
        default=Path(
            "/tmp/ms-release-483b52ceea/rpc/ethereum-pending-results.json"
        ),
    )
    result = parser.parse_args()
    endpoint = urlparse(result.endpoint)
    if endpoint.hostname not in ("127.0.0.1", "localhost") or endpoint.port not in (
        21044,
        21045,
        21046,
    ):
        parser.error(
            "this fixture only permits task-owned loopback localnet ports 21044..21046"
        )
    return result


async def run(args, report):
    async with bt.Client(
        args.endpoint, fallback_endpoints=[], archive_endpoints=[]
    ) as client:
        substrate = client._substrate
        rpc = substrate.raw._session.request

        async def query_account(address, block_hash):
            value = await substrate.query(
                "System", "Account", [address], block_hash=block_hash
            )
            return {
                "nonce": int(value["nonce"]),
                "free_rao": int(value["data"]["free"]),
            }

        async def finalized():
            block_hash = await rpc("chain_getFinalizedHead", [])
            header = await rpc("chain_getHeader", [block_hash])
            return block_hash, int(header["number"], 16)

        async def wait_for(predicate, reason, interval=1.0):
            deadline = time.monotonic() + args.timeout
            while True:
                result = await predicate()
                if result:
                    return result
                if time.monotonic() >= deadline:
                    raise AssertionError(reason)
                await asyncio.sleep(interval)

        report["genesis_hash"] = await rpc("chain_getBlockHash", [0])
        assert report["genesis_hash"] != (
            "0x2f0555cc76fc2840a25a6ea3b9637146806f1f44b090c175ffde2a7e5ab36c03"
        ), "fixture refuses mainnet genesis"
        report["chain_type"] = await rpc("system_chainType", [])
        assert report["chain_type"] in ("Local", "Development"), report["chain_type"]
        report["minimum_period_ms"] = int(
            await substrate.constant("Timestamp", "MinimumPeriod")
        )
        assert report["minimum_period_ms"] == 6000, "requires normal 12-second slots"
        flags = await substrate.query("AdminUtils", "HashedAccountsEnabled")
        assert flags is False, (
            "this ordinary EVM regression requires the master flag off"
        )
        report["hashed_accounts_enabled"] = flags
        report["runtime"] = await rpc("state_getRuntimeVersion", [])
        code = await rpc("state_getStorage", ["0x3a636f6465"])
        report["runtime_code_sha256"] = hashlib.sha256(
            bytes.fromhex(code[2:])
        ).hexdigest()

        sender = Account.create()
        receiver = Account.create()
        source, destination = sender.address, receiver.address
        source_mirror, destination_mirror = map(h160_to_ss58, (source, destination))
        report.update(
            sender=source,
            recipient=destination,
            sender_mirror=source_mirror,
            recipient_mirror=destination_mirror,
            value_per_transaction_rao=VALUE_RAO,
            existential_deposit_rao=ED_RAO,
            wei_per_rao=WEI_PER_RAO,
        )
        ed = int(await substrate.constant("Balances", "ExistentialDeposit"))
        assert ed == ED_RAO, ed
        chain_id = int(await rpc("eth_chainId", []), 16)
        quoted_gas_price = int(await rpc("eth_gasPrice", []), 16)
        gas_price = (
            (max(quoted_gas_price * 2, 10**10) + WEI_PER_RAO - 1) // WEI_PER_RAO
        ) * WEI_PER_RAO
        gas_limit = 100_000
        fund_rao = max(
            100 * 10**9,
            args.count * (VALUE_RAO + gas_limit * gas_price // WEI_PER_RAO)
            + 10 * 10**9
            + ED_RAO,
        )
        assert fund_rao <= 10**15, (
            "unexpected local gas price exceeds disposable funding bound"
        )
        report.update(
            chain_id=chain_id,
            gas_price_wei=gas_price,
            gas_limit=gas_limit,
            funding_rao=fund_rao,
        )
        before_finalized, _ = await finalized()
        assert (await query_account(source_mirror, before_finalized))["free_rao"] == 0
        assert (await query_account(destination_mirror, before_finalized))[
            "free_rao"
        ] == 0

        funder = dev_wallet("//" + args.funder).coldkey

        async def funder_idle():
            final_hash, _ = await finalized()
            current = await query_account(funder.ss58_address, final_hash)
            return (
                int(await rpc("system_accountNextIndex", [funder.ss58_address]))
                == current["nonce"]
            )

        await wait_for(funder_idle, "dev funder did not reach finalized idle nonce")
        print(
            json.dumps({"stage": "funding", "funder": args.funder, "sender": source}),
            flush=True,
        )
        funding = await asyncio.wait_for(
            client.submit_call(
                bt.calls.Balances.transfer_keep_alive(
                    dest=source_mirror, value=fund_rao
                ),
                funder,
                shielded=False,
                wait_for_inclusion=True,
                wait_for_finalization=True,
            ),
            timeout=args.timeout,
        )
        assert funding.success, str(funding.error)
        report["funding"] = {
            "block_hash": funding.block_hash,
            "extrinsic_id": funding.extrinsic_id,
            "success": funding.success,
        }
        initial_finalized, initial_number = await finalized()
        initial_sender = await query_account(source_mirror, initial_finalized)
        assert initial_sender["free_rao"] == fund_rao, initial_sender
        initial_nonce = initial_sender["nonce"]
        assert initial_nonce == 0, initial_nonce

        async def funding_visible():
            balance = int(await rpc("eth_getBalance", [source, "latest"]), 16)
            return balance == (fund_rao - ED_RAO) * WEI_PER_RAO

        await wait_for(
            funding_visible, "finalized native funding not visible in Ethereum mapping"
        )
        report["funding_finalized_block"] = initial_number
        report["initial_sender_native"] = initial_sender
        assert (
            int(await rpc("eth_getTransactionCount", [source, "latest"]), 16)
            == initial_nonce
        )
        assert int(await rpc("eth_getBalance", [destination, "latest"]), 16) == 0

        signed = [
            sender.sign_transaction(
                {
                    "chainId": chain_id,
                    "nonce": initial_nonce + i,
                    "gasPrice": gas_price,
                    "gas": gas_limit,
                    "to": destination,
                    "value": VALUE_RAO * WEI_PER_RAO,
                    "data": b"",
                }
            )
            for i in range(args.count)
        ]
        previous = int((await rpc("chain_getHeader", []))["number"], 16)

        async def new_block():
            header = await rpc("chain_getHeader", [])
            return header if int(header["number"], 16) > previous else None

        header = await wait_for(
            new_block, "normal localnet did not author a new block", interval=0.05
        )
        head_before = int(header["number"], 16)
        started = time.monotonic()
        hashes = []
        report["submitted_hashes"] = hashes
        for tx in signed:
            returned = await rpc(
                "eth_sendRawTransaction", ["0x" + tx.raw_transaction.hex()]
            )
            assert returned.lower() == ("0x" + tx.hash.hex()).lower(), (
                "submitted transaction hash mismatch"
            )
            hashes.append(returned)
        report["submission_seconds"] = time.monotonic() - started
        report["submission_head"] = head_before
        assert len(hashes) == args.count
        print(
            json.dumps(
                {
                    "stage": "submitted",
                    "count": len(hashes),
                    "seconds": report["submission_seconds"],
                }
            ),
            flush=True,
        )

        pending_error = None
        try:
            latest_nonce = int(
                await rpc("eth_getTransactionCount", [source, "latest"]), 16
            )
            latest_balance = int(
                await rpc("eth_getBalance", [destination, "latest"]), 16
            )
            pool = await rpc("author_pendingExtrinsics", [])
            pending_nonce = int(
                await rpc("eth_getTransactionCount", [source, "pending"]), 16
            )
            pending_balance = int(
                await rpc("eth_getBalance", [destination, "pending"]), 16
            )
            head_after = int((await rpc("chain_getHeader", []))["number"], 16)
            expected_nonce = initial_nonce + args.count
            expected_balance = (args.count * VALUE_RAO - ED_RAO) * WEI_PER_RAO
            report["pending"] = {
                "head_before": head_before,
                "head_after": head_after,
                "pool_entries": len(pool),
                "latest_sender_nonce": latest_nonce,
                "latest_recipient_balance_wei": str(latest_balance),
                "sender_nonce": pending_nonce,
                "recipient_balance_wei": str(pending_balance),
                "expected_sender_nonce": expected_nonce,
                "expected_recipient_balance_wei": str(expected_balance),
                "observed_seconds_after_block": time.monotonic() - started,
            }
            assert head_after == head_before, (
                "pending observation crossed a block; rerun with fresh keys"
            )
            assert latest_nonce == initial_nonce and latest_balance == 0, (
                "transactions included before pending observation"
            )
            assert len(pool) >= args.count, (
                "not all submitted transactions are represented in ready pool"
            )
            assert pending_nonce == expected_nonce, (pending_nonce, expected_nonce)
            assert pending_balance == expected_balance, (
                pending_balance,
                expected_balance,
            )
        except Exception as error:
            pending_error = f"{type(error).__name__}: {error}"
            report["pending_error"] = pending_error

        async def all_receipts():
            receipts = await asyncio.gather(
                *[rpc("eth_getTransactionReceipt", [h]) for h in hashes]
            )
            return (
                receipts if all(receipt is not None for receipt in receipts) else None
            )

        receipts = await wait_for(
            all_receipts,
            "accepted Ethereum transfers did not obtain receipts",
            interval=2,
        )
        # Persist raw receipt evidence even if a later accounting assertion fails.
        report["included_receipts"] = receipts
        for tx_hash, receipt in zip(hashes, receipts, strict=True):
            assert receipt["transactionHash"].lower() == tx_hash.lower()
            assert receipt["status"] == "0x1", receipt
            assert receipt["from"].lower() == source.lower()
            assert receipt["to"].lower() == destination.lower()
            assert int(receipt["effectiveGasPrice"], 16) == gas_price
        highest = max(int(receipt["blockNumber"], 16) for receipt in receipts)

        async def all_finalized():
            final_hash, height = await finalized()
            return (final_hash, height) if height >= highest else None

        final_hash, final_number = await wait_for(
            all_finalized, "Ethereum inclusion did not finalize", interval=2
        )
        canonical = {}
        base_fees = {}
        for height in sorted({int(receipt["blockNumber"], 16) for receipt in receipts}):
            block = await rpc("eth_getBlockByNumber", [hex(height), False])
            canonical[height] = block["hash"]
            base_fees[height] = int(block["baseFeePerGas"], 16)
        for receipt in receipts:
            assert receipt["blockHash"] == canonical[int(receipt["blockNumber"], 16)]
        sender_native = await query_account(source_mirror, final_hash)
        recipient_native = await query_account(destination_mirror, final_hash)
        receipt_fee_wei = sum(
            int(r["gasUsed"], 16) * int(r["effectiveGasPrice"], 16) for r in receipts
        )
        # Subtensor's established runner policy disables priority fees, charging
        # block base fee. The stock receipt field still reports submitted price.
        total_fee_wei = sum(
            int(r["gasUsed"], 16) * base_fees[int(r["blockNumber"], 16)]
            for r in receipts
        )
        actual_fee_rao = fund_rao - args.count * VALUE_RAO - sender_native["free_rao"]
        report["fee_policy"] = {
            "policy": "Existing Subtensor runner ignores priority fee and charges base fee only",
            "block_base_fees_wei": base_fees,
            "actual_native_debit_rao": actual_fee_rao,
            "base_fee_accounting_wei": str(total_fee_wei),
            "receipt_reported_fee_wei": str(receipt_fee_wei),
            "receipt_matches_actual_debit": receipt_fee_wei
            == actual_fee_rao * WEI_PER_RAO,
            "finding": "PRE-EXISTING: effectiveGasPrice reports submitted legacy gas price rather than charged base fee; confirmed receipt/accounting mismatch",
        }
        assert receipt_fee_wei > total_fee_wei, report["fee_policy"]
        assert actual_fee_rao * WEI_PER_RAO == total_fee_wei, report["fee_policy"]
        assert total_fee_wei % WEI_PER_RAO == 0
        expected_recipient_rao = args.count * VALUE_RAO
        expected_sender_rao = (
            fund_rao - expected_recipient_rao - total_fee_wei // WEI_PER_RAO
        )
        assert recipient_native["free_rao"] == expected_recipient_rao, recipient_native
        assert sender_native["free_rao"] == expected_sender_rao, (
            sender_native,
            expected_sender_rao,
        )
        assert sender_native["nonce"] == initial_nonce + args.count, sender_native
        tag = hex(final_number)
        final_nonce = int(await rpc("eth_getTransactionCount", [source, tag]), 16)
        final_source_balance = int(await rpc("eth_getBalance", [source, tag]), 16)
        final_destination_balance = int(
            await rpc("eth_getBalance", [destination, tag]), 16
        )
        assert final_nonce == initial_nonce + args.count
        assert final_source_balance == (expected_sender_rao - ED_RAO) * WEI_PER_RAO
        assert (
            final_destination_balance == (expected_recipient_rao - ED_RAO) * WEI_PER_RAO
        )
        report["finalized"] = {
            "substrate_block_hash": final_hash,
            "block_number": final_number,
            "sender_native": sender_native,
            "recipient_native": recipient_native,
            "sender_nonce": final_nonce,
            "sender_balance_wei": str(final_source_balance),
            "recipient_balance_wei": str(final_destination_balance),
            "total_fee_wei": str(total_fee_wei),
            "receipt_reported_fee_wei": str(receipt_fee_wei),
            "canonical_ethereum_blocks": canonical,
            "receipts": receipts,
        }
        assert await substrate.query("AdminUtils", "HashedAccountsEnabled") is False
        assert pending_error is None, pending_error
        report["status"] = "passed"


async def main():
    args = arguments()
    report = {
        "test": "sequential Ethereum pending overlay and finalized value/nonce accounting",
        "started_at_utc": datetime.now(timezone.utc).isoformat(),
        "endpoint": args.endpoint,
        "requested_count": args.count,
        "status": "running",
        "limitations": "Disposable ordinary ECDSA transfers with MS master flag off; not production throughput or weights evidence.",
        "script_sha256": hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
    }
    try:
        await run(args, report)
    except Exception as error:
        report["status"] = "failed"
        report["error"] = f"{type(error).__name__}: {error}"
        raise
    finally:
        report["finished_at_utc"] = datetime.now(timezone.utc).isoformat()
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(json.dumps(report, indent=2) + "\n")
        print(
            json.dumps({"status": report["status"], "output": str(args.output)}),
            flush=True,
        )


if __name__ == "__main__":
    asyncio.run(main())
