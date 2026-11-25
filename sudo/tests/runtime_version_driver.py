"""Exercise the verifier-to-CLI runtime pin with no network or key material."""

import asyncio
import importlib.util
import sys
from pathlib import Path
from types import SimpleNamespace
from unittest.mock import AsyncMock

spec = importlib.util.spec_from_file_location("checked_in_countersign", sys.argv[1])
verifier = importlib.util.module_from_spec(spec)
spec.loader.exec_module(verifier)

REVIEWED = 496
CALL_HASH = bytes(32)
PROPOSAL = {"specVersion": REVIEWED, "sudoKey": "test-key", "call_hash": "0x" + CALL_HASH.hex()}
ARGS = {"call": {"Utility": {"batch_all": {"calls": []}}}}


class Client:
    def __init__(self, finalized_version, composed_version, current_version):
        self.snapshot = SimpleNamespace(
            spec_version=AsyncMock(return_value=finalized_version),
            query=AsyncMock(return_value="test-key"),
        )
        self.compose = AsyncMock(
            return_value=SimpleNamespace(spec_version=composed_version, call_hash=CALL_HASH),
        )
        self.spec_version = AsyncMock(return_value=current_version)

    async def __aenter__(self):
        return self

    async def __aexit__(self, *_exc):
        pass

    async def finalized_block(self):
        return 123

    async def at(self, block):
        assert block == 123
        return self.snapshot

    async def multisig(self, *_args, **_kwargs):
        return SimpleNamespace(address="test-key")


async def check(finalized, composed, current, expected_error=None):
    client = Client(finalized, composed, current)
    sys.modules["bittensor"] = SimpleNamespace(
        Subtensor=lambda *_args, **_kwargs: client,
        calls=SimpleNamespace(
            Utility=SimpleNamespace(batch_all=lambda **kwargs: kwargs),
            Sudo=SimpleNamespace(sudo=lambda **kwargs: kwargs),
        ),
        storage=SimpleNamespace(Sudo=SimpleNamespace(Key=("Sudo", "Key"))),
    )
    try:
        await verifier.verify(PROPOSAL, [], ARGS, [], 2)
    except ValueError as error:
        assert expected_error and expected_error in str(error), str(error)
        if expected_error.startswith("Finalized"):
            client.compose.assert_not_awaited()
            client.snapshot.query.assert_not_awaited()
    else:
        assert not expected_error, "mismatched spec allowed"


asyncio.run(check(497, 496, 496, "Finalized runtime specVersion 497"))
asyncio.run(check(496, 497, 497, "Composed call runtime specVersion 497"))
asyncio.run(check(496, 496, 497, "Current runtime specVersion 497"))
asyncio.run(check(496, 496, 496))

command = verifier.btcli_command(Path("test.args"), [], 2, "unused", PROPOSAL["call_hash"], "open", REVIEWED)
assert command[command.index("--expected-spec-version") + 1] == str(REVIEWED)
