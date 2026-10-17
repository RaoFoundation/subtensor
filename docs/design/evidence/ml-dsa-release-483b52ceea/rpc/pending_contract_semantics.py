"""Focused disposable localnet pending call/create transaction semantics.

No runtime or feature flags are changed. Alice temporarily adds one random
creator to the deployment allowlist and restores the exact original list.
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
import rlp
from eth_account import Account
from eth_utils import keccak, to_checksum_address

from bittensor.evm.addresses import h160_to_ss58
from tests.harness.samples import dev_wallet

WEI_PER_RAO = 10**9
ZERO = "0x" + "00" * 32
CONSTANT_RUNTIME = bytes.fromhex("602a60005260206000f3")
CONSTANT_INIT = bytes.fromhex("600a600c600039600a6000f3") + CONSTANT_RUNTIME


def stateful_bytecode():
    code, labels, jumps = bytearray(), {}, []

    def emit(hexbytes):
        code.extend(bytes.fromhex(hexbytes))

    def jump(label):
        emit("60")
        jumps.append((len(code), label))
        emit("0057")

    def label(name):
        labels[name] = len(code)
        emit("5b")

    # Calldata words: 0=read, 1=increment once, 2=revert, 3=gasprice, 4=SLOAD cost.
    emit("600035")
    for selector, name in ((1, "write"), (2, "revert"), (3, "price"), (4, "sload")):
        emit(f"8060{selector:02x}14")
        jump(name)
    emit("5060005460005260206000f3")
    label("write")
    emit("5060005480602a14")
    jump("write_ok")
    emit("60006000fd")  # Any leaked prior probe makes this branch revert.
    label("write_ok")
    emit("6001018060005560005260206000f3")
    label("revert")
    emit("50606360005563deadbeef6000526004601cfd")
    label("price")
    emit("503a60005260206000f3")
    label("sload")
    # GAS; PUSH1 0; SLOAD; POP; GAS; SWAP1; SUB exposes cold/warm SLOAD gas.
    emit("505a600054505a900360005260206000f3")
    for position, name in jumps:
        assert labels[name] < 256
        code[position] = labels[name]
    runtime = bytes(code)
    assert len(runtime) < 256
    # Constructor initializes slot zero to 42, then returns runtime bytes.
    init = (
        bytes.fromhex(
            f"602a60005560{len(runtime):02x}601160003960{len(runtime):02x}6000f3"
        )
        + runtime
    )
    assert len(init) - len(runtime) == 17
    return init, runtime


async def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--endpoint", default="ws://127.0.0.1:21046")
    parser.add_argument("--timeout", type=float, default=240)
    parser.add_argument(
        "--output",
        type=Path,
        default=Path(
            "/tmp/ms-release-483b52ceea/rpc/pending-contract-results.json"
        ),
    )
    args = parser.parse_args()
    endpoint = urlparse(args.endpoint)
    assert endpoint.hostname in ("127.0.0.1", "localhost") and endpoint.port == 21046
    report = {
        "test": "typed pending simulation creation, EIP-1559/access-list and rollback semantics",
        "status": "running",
        "endpoint": args.endpoint,
        "started_at_utc": datetime.now(timezone.utc).isoformat(),
        "script_sha256": hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
        "boundaries": "One disposable real contract plus read-only pending simulations; does not claim legacy runtime API-version, EIP-7702, throughput or exhaustive opcode coverage.",
        "native_setup_receipts": [],
        "checks": {},
    }
    sender = Account.create()
    source = sender.address
    mirror = h160_to_ss58(source)
    report.update(sender=source, sender_mirror=mirror)
    init, runtime = stateful_bytecode()
    report.update(
        stateful_initcode="0x" + init.hex(), stateful_runtime="0x" + runtime.hex()
    )
    try:
        async with bt.Client(
            args.endpoint, fallback_endpoints=[], archive_endpoints=[]
        ) as client:
            substrate = client._substrate
            raw = substrate.raw
            rpc = raw._session.request
            restored = False
            original = None

            async def wait(predicate, description):
                deadline = time.monotonic() + args.timeout
                while True:
                    result = await predicate()
                    if result:
                        return result
                    assert time.monotonic() < deadline, description
                    await asyncio.sleep(1)

            async def native_submit(call, who, stage):
                composed = await client.compose(call)
                nonce = int(await rpc("system_accountNextIndex", [who.ss58_address]))
                tx = await raw.create_signed_extrinsic(
                    composed, who, nonce=nonce, era="00"
                )
                receipt = await asyncio.wait_for(
                    raw.submit_extrinsic(
                        tx, wait_for_inclusion=True, wait_for_finalization=True
                    ),
                    args.timeout,
                )
                report["native_setup_receipts"].append(
                    {
                        "stage": stage,
                        "extrinsic_hash": receipt.extrinsic_hash,
                        "block_hash": receipt.block_hash,
                        "block_number": receipt.block_number,
                        "extrinsic_index": receipt.extrinsic_idx,
                        "success": receipt.is_success,
                    }
                )
                assert receipt.is_success, str(receipt.error_message)

            async def whitelist(values, stage):
                inner = await client.compose(bt.calls.EVM.set_whitelist(new=values))
                await native_submit(
                    bt.calls.Sudo.sudo(call=inner), dev_wallet("//Alice").coldkey, stage
                )
                actual = await substrate.query("EVM", "WhitelistedCreators")
                assert [item.lower() for item in actual] == [
                    item.lower() for item in values
                ]

            async def expect_revert(request, method="eth_call"):
                try:
                    await rpc(method, [request, "pending"])
                except Exception as error:
                    message = str(error)
                    assert "revert" in message.lower(), message
                    return message
                raise AssertionError("reverting simulation unexpectedly succeeded")

            async def snap(contract):
                return {
                    "source_nonce": await rpc(
                        "eth_getTransactionCount", [source, "pending"]
                    ),
                    "source_balance": await rpc("eth_getBalance", [source, "pending"]),
                    "code": await rpc("eth_getCode", [contract, "pending"]),
                    "slot0": await rpc("eth_getStorageAt", [contract, ZERO, "pending"]),
                }

            report["genesis_hash"] = await rpc("chain_getBlockHash", [0])
            assert (
                report["genesis_hash"]
                != "0x2f0555cc76fc2840a25a6ea3b9637146806f1f44b090c175ffde2a7e5ab36c03"
            )
            assert await rpc("system_chainType", []) in ("Local", "Development")
            assert int(await substrate.constant("Timestamp", "MinimumPeriod")) == 6000
            assert (
                int(await substrate.constant("Balances", "ExistentialDeposit")) == 500
            )
            flags = {
                "master": await substrate.query("AdminUtils", "HashedAccountsEnabled"),
                "disable_deployment_whitelist": await substrate.query(
                    "EVM", "DisableWhitelistCheck"
                ),
            }
            assert (
                flags["master"] is False
            )
            report["initial_flags"] = flags
            code = await rpc("state_getStorage", ["0x3a636f6465"])
            report["runtime_code_sha256"] = hashlib.sha256(
                bytes.fromhex(code[2:])
            ).hexdigest()
            original = list(await substrate.query("EVM", "WhitelistedCreators"))
            report["original_creator_allowlist"] = original
            try:
                if not flags["disable_deployment_whitelist"]:
                    await whitelist([*original, source], "allow_disposable_creator")
                await native_submit(
                    bt.calls.Balances.transfer_keep_alive(
                        dest=mirror, value=100 * 10**9
                    ),
                    dev_wallet("//Bob").coldkey,
                    "fund_creator_mirror",
                )
                print(
                    json.dumps({"stage": "funded_and_allowed", "creator": source}),
                    flush=True,
                )

                async def funded():
                    return (
                        int(await rpc("eth_getBalance", [source, "latest"]), 16)
                        == (100 * 10**9 - 500) * WEI_PER_RAO
                    )

                await wait(funded, "finalized mirror funding not visible")
                price = (
                    (
                        max(int(await rpc("eth_gasPrice", []), 16) * 2, 10**10)
                        + WEI_PER_RAO
                        - 1
                    )
                    // WEI_PER_RAO
                ) * WEI_PER_RAO
                dynamic = {
                    "maxFeePerGas": hex(price),
                    "maxPriorityFeePerGas": hex(price),
                }
                contract = to_checksum_address(
                    keccak(rlp.encode([bytes.fromhex(source[2:]), 0]))[-20:]
                )
                report["predicted_contract"] = contract
                base = {"from": source, "gas": hex(500000), **dynamic}
                creation = {
                    **base,
                    "data": "0x" + CONSTANT_INIT.hex(),
                    "value": hex(1000 * WEI_PER_RAO),
                    "accessList": [{"address": source, "storageKeys": []}],
                }
                before = await snap(contract)
                assert (
                    int(before["source_nonce"], 16) == 0
                    and before["code"] == "0x"
                    and int(before["slot0"], 16) == 0
                )
                create_results = []
                for _ in range(2):
                    returned = await rpc("eth_call", [creation, "pending"])
                    assert returned == "0x" + CONSTANT_RUNTIME.hex(), returned
                    estimate = int(
                        await rpc("eth_estimateGas", [creation, "pending"]), 16
                    )
                    assert 53000 <= estimate < 500000, estimate
                    create_results.append(
                        {"returned_runtime": returned, "estimated_gas": estimate}
                    )
                create_error = await expect_revert({**base, "data": "0x60006000fd"})
                after = await snap(contract)
                assert before == after, (before, after)
                report["checks"]["creation_rollback"] = {
                    "before": before,
                    "after": after,
                    "runs": create_results,
                    "revert": create_error,
                }

                tx = sender.sign_transaction(
                    {
                        "type": 2,
                        "chainId": int(await rpc("eth_chainId", []), 16),
                        "nonce": 0,
                        "gas": 500000,
                        "maxFeePerGas": price,
                        "maxPriorityFeePerGas": price,
                        "value": 1000 * WEI_PER_RAO,
                        "data": init,
                        "accessList": [{"address": contract, "storageKeys": [ZERO]}],
                    }
                )
                tx_hash = await rpc(
                    "eth_sendRawTransaction", ["0x" + tx.raw_transaction.hex()]
                )
                assert tx_hash == "0x" + tx.hash.hex()

                async def get_receipt():
                    return await rpc("eth_getTransactionReceipt", [tx_hash])

                receipt = await wait(
                    get_receipt, "contract deployment was not included"
                )
                assert receipt["status"] == "0x1", receipt
                assert receipt["contractAddress"].lower() == contract.lower()
                deployment_height = int(receipt["blockNumber"], 16)

                async def final_deployment():
                    final_hash = await rpc("chain_getFinalizedHead", [])
                    header = await rpc("chain_getHeader", [final_hash])
                    return (
                        final_hash
                        if int(header["number"], 16) >= deployment_height
                        else None
                    )

                final_hash = await wait(final_deployment, "deployment did not finalize")
                canonical = await rpc(
                    "eth_getBlockByNumber", [hex(deployment_height), False]
                )
                assert canonical["hash"] == receipt["blockHash"]
                report["deployment"] = {
                    "receipt": receipt,
                    "finalized_substrate_hash": final_hash,
                }
                print(
                    json.dumps({"stage": "contract_finalized", "contract": contract}),
                    flush=True,
                )
                before = await snap(contract)
                assert (
                    before["code"] == "0x" + runtime.hex()
                    and int(before["slot0"], 16) == 42
                )
                assert int(before["source_nonce"], 16) == 1
                access = [{"address": contract, "storageKeys": [ZERO]}]
                request = {**base, "to": contract, "accessList": access}
                checks = report["checks"]
                checks["read"] = await rpc("eth_call", [request, "pending"])
                assert int(checks["read"], 16) == 42
                write = {**request, "data": "0x" + (1).to_bytes(32, "big").hex()}
                writes = []
                for _ in range(2):
                    output = await rpc("eth_call", [write, "pending"])
                    estimate = int(await rpc("eth_estimateGas", [write, "pending"]), 16)
                    assert int(output, 16) == 43 and 21000 < estimate < 500000
                    writes.append(
                        {"returned_value": int(output, 16), "estimated_gas": estimate}
                    )
                checks["write_simulations"] = writes
                reverting = {**request, "data": "0x" + (2).to_bytes(32, "big").hex()}
                checks["call_revert"] = await expect_revert(reverting)
                checks["estimate_revert"] = await expect_revert(
                    reverting, "eth_estimateGas"
                )
                gasprice = {**request, "data": "0x" + (3).to_bytes(32, "big").hex()}
                price_seen = int(await rpc("eth_call", [gasprice, "pending"]), 16)
                base_fee = int(await rpc("eth_gasPrice", []), 16)
                assert price_seen == base_fee and price > base_fee, (
                    price_seen,
                    base_fee,
                    price,
                )
                checks["eip1559_gasprice"] = {
                    "submitted_max_fee_and_priority_fee": price,
                    "expected_existing_no_tip_policy_base_fee": base_fee,
                    "observed": price_seen,
                    "boundary": "Type2 fee fields accepted; existing runtime intentionally exposes base fee as GASPRICE and ignores priority fee",
                }
                no_access = {k: v for k, v in request.items() if k != "accessList"}
                estimates = {}
                access_costs = {}
                for tag in ("latest", "pending"):
                    estimates[tag] = {
                        "plain": int(
                            await rpc("eth_estimateGas", [no_access, tag]), 16
                        ),
                        "access_list": int(
                            await rpc("eth_estimateGas", [request, tag]), 16
                        ),
                    }
                    for name, probe in (("plain", no_access), ("access_list", request)):
                        estimate = estimates[tag][name]
                        assert 21000 < estimate < 500000, estimates
                        output = await rpc(
                            "eth_call", [{**probe, "gas": hex(estimate)}, tag]
                        )
                        assert int(output, 16) == 42, (tag, name, estimate, output)
                    data = "0x" + (4).to_bytes(32, "big").hex()
                    access_costs[tag] = {
                        name: int(
                            await rpc("eth_call", [{**probe, "data": data}, tag]), 16
                        )
                        for name, probe in (("cold", no_access), ("warm", request))
                    }
                    assert access_costs[tag] == {"cold": 2107, "warm": 107}, (
                        access_costs
                    )
                # Effective estimates include proof/storage/external costs, so
                # exact gas parity/order is not an access-list semantic invariant.
                checks["access_list_estimates"] = estimates
                checks["access_list_opcode_costs"] = access_costs
                after = await snap(contract)
                assert before == after, (before, after)
                checks["call_rollback"] = {"before": before, "after": after}
                assert (
                    int(await rpc("eth_getStorageAt", [contract, ZERO, "latest"]), 16)
                    == 42
                )
                assert (
                    await rpc("eth_getCode", [contract, "latest"])
                    == "0x" + runtime.hex()
                )
            finally:
                current = list(await substrate.query("EVM", "WhitelistedCreators"))
                if current != original:
                    await whitelist(original, "restore_creator_allowlist")
                restored = (
                    list(await substrate.query("EVM", "WhitelistedCreators"))
                    == original
                )
                report["creator_allowlist_restored"] = restored
            assert restored
            final_flags = {
                "master": await substrate.query("AdminUtils", "HashedAccountsEnabled"),
                "disable_deployment_whitelist": await substrate.query(
                    "EVM", "DisableWhitelistCheck"
                ),
            }
            assert final_flags == flags
            report["final_flags"] = final_flags
            report["status"] = "passed"
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
