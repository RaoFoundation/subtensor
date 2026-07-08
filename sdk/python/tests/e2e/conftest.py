"""E2E fixtures: a writable localnet chain, one Client session, dev wallets.

Chain resolution, in order:

1. ``E2E_ENDPOINT`` set -> attach to that node (the dev inner loop: run a
   localnet yourself and iterate without docker churn).
2. Otherwise start a disposable docker localnet (``LOCALNET_IMAGE``, default
   the fork's image) and tear it down at session end. ``SKIP_PULL=1`` skips
   the pull when CI pre-loads the image.

Every test in this directory is automatically marked ``e2e`` (deselected by
the default pytest run; select with ``-m e2e``). Each module must declare

    pytestmark = pytest.mark.asyncio(loop_scope="session")

so its tests run on the same session event loop that owns the session-scoped
websocket Client — an async fixture used from a different loop hangs forever
(a collection hook is too late for pytest-asyncio to see the marker, which is
why this lives in the modules).

Tests must stay re-runnable against a long-lived dev node: prove writes by
state *delta* (or accept the chain's rate-limit answer where a rerun can be
throttled), and never assume a fresh chain.
"""

from __future__ import annotations

import os
import subprocess
import time

import pytest
import pytest_asyncio

import bittensor as sub
from tests.harness.samples import dev_wallet

DEFAULT_IMAGE = "ghcr.io/raofoundation/subtensor-localnet:monorepo-sdk"
CONTAINER_NAME = f"bittensor-e2e-{os.getpid()}"


def pytest_collection_modifyitems(config, items):
    this_dir = os.path.dirname(__file__)
    for item in items:
        if str(item.fspath).startswith(this_dir):
            item.add_marker(pytest.mark.e2e)


@pytest.fixture(scope="session", autouse=True)
def _isolated_local_state(tmp_path_factory):
    """Redirect every ~/.bittensor cache/config file to a temp dir so e2e runs
    never touch (or depend on) the developer's real local state."""
    mp = pytest.MonkeyPatch()
    tmp = tmp_path_factory.mktemp("btcli-state")
    for var, filename in [
        ("BTCLI_CONFIG", "btcli.json"),
        ("BTCLI_PROXIES_PATH", "proxies.json"),
        ("BTCLI_ADDRESSES_PATH", "addresses.json"),
        ("BTCLI_MULTISIGS_PATH", "multisigs.json"),
        ("BTCLI_MULTISIG_CACHE", "multisig_cache.json"),
        ("BTCLI_SUBNET_NAMES_CACHE", "subnet_names.json"),
        ("BTCLI_TOKEN_SYMBOLS_CACHE", "token_symbols.json"),
    ]:
        mp.setenv(var, str(tmp / filename))
    yield
    mp.undo()


def _wait_for_import(name: str, timeout: float = 180.0) -> None:
    """Block until the node logs its first imported block (chain is live)."""
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        logs = subprocess.run(["docker", "logs", name], capture_output=True, text=True)
        if "Imported #1" in logs.stdout or "Imported #1" in logs.stderr:
            return
        time.sleep(1)
    raise RuntimeError(f"localnet container {name} did not import a block in {timeout}s")


@pytest.fixture(scope="session")
def chain_endpoint() -> str:
    endpoint = os.getenv("E2E_ENDPOINT")
    if endpoint:
        yield endpoint
        return

    image = os.getenv("LOCALNET_IMAGE", DEFAULT_IMAGE)
    if not os.getenv("SKIP_PULL"):
        subprocess.run(["docker", "pull", image], check=True)
    subprocess.run(
        [
            "docker",
            "run",
            "--rm",
            "-d",
            "--name",
            CONTAINER_NAME,
            "-p",
            "9944:9944",
            "-p",
            "9945:9945",
            image,
        ],
        check=True,
    )
    try:
        _wait_for_import(CONTAINER_NAME)
        yield "ws://127.0.0.1:9944"
    finally:
        subprocess.run(["docker", "rm", "-f", CONTAINER_NAME], capture_output=True)


@pytest_asyncio.fixture(scope="session", loop_scope="session")
async def client(chain_endpoint: str):
    async with sub.Client(chain_endpoint) as c:
        yield c


@pytest.fixture(scope="session")
def alice():
    return dev_wallet("//Alice", "//Alice//hot")


@pytest.fixture(scope="session")
def bob():
    return dev_wallet("//Bob", "//Bob//hot")


@pytest_asyncio.fixture(scope="session", loop_scope="session")
async def owned_subnet(client, alice) -> int:
    """A subnet registered (and started) by Alice for this session.

    Registered once and shared by every test that needs an Alice-owned subnet
    with her hotkey at uid 0. Tests that mutate owner-scoped state they can't
    share should register their own via ``register_subnet``.
    """
    return await register_subnet(client, alice)


async def register_subnet(client, wallet) -> int:
    """Register a fresh subnet for ``wallet`` and return its netuid."""
    result = await client.execute_tool("register_subnet", {}, wallet)
    assert result.success, f"register_subnet failed: {result.message}"
    netuid = max(s.netuid for s in await client.subnets.all())
    # Schedule permitting; a start-call delay refusal doesn't block staking on
    # localnet, so the result is deliberately not asserted.
    await client.execute_tool("start_call", {"netuid": netuid}, wallet)
    return netuid
