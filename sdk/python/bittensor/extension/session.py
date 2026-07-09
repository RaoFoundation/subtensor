"""Ensure the extension bridge is running and ready before signing."""

from __future__ import annotations

import asyncio
import os
import signal
import subprocess
import sys
from pathlib import Path
from typing import Callable, Optional
from urllib.parse import urlparse

from .bridge import DEFAULT_BRIDGE_HOST, DEFAULT_BRIDGE_PORT
from .browser import open_bridge_page
from .client import BridgeClient, BridgeError
from .tokens import clear_bridge_token, read_bridge_token

DEFAULT_BRIDGE_PID_PATH = Path.home() / ".bittensor" / "extension_bridge.pid"


def bridge_pid_path() -> Path:
    return Path(os.getenv("BITTENSOR_EXTENSION_BRIDGE_PID") or DEFAULT_BRIDGE_PID_PATH)


def bridge_http_url(host: str = DEFAULT_BRIDGE_HOST, port: int = DEFAULT_BRIDGE_PORT) -> str:
    return f"http://{host}:{port}/"


def bridge_page_url(
    host: str = DEFAULT_BRIDGE_HOST,
    port: int = DEFAULT_BRIDGE_PORT,
    *,
    session_id: str,
) -> str:
    """Bridge page URL scoped to one daemon session (stale tabs cannot reconnect)."""
    return f"{bridge_http_url(host, port)}?session={session_id}"


def bridge_ws_url(
    host: str = DEFAULT_BRIDGE_HOST,
    port: int = DEFAULT_BRIDGE_PORT,
    *,
    bridge_url: Optional[str] = None,
) -> str:
    if bridge_url:
        parsed = urlparse(bridge_url)
        if parsed.scheme in ("ws", "wss"):
            return bridge_url
        if parsed.scheme in ("http", "https"):
            host = parsed.hostname or host
            port = parsed.port or port
    return f"ws://{host}:{port}/ws"


def _parse_bridge_target(bridge_url: Optional[str]) -> tuple[str, int]:
    if not bridge_url:
        return DEFAULT_BRIDGE_HOST, DEFAULT_BRIDGE_PORT
    parsed = urlparse(bridge_url)
    host = parsed.hostname or DEFAULT_BRIDGE_HOST
    port = parsed.port or DEFAULT_BRIDGE_PORT
    return host, port


def _process_alive(pid: int) -> bool:
    try:
        os.kill(pid, 0)
    except OSError:
        return False
    return True


def _read_daemon_pid() -> Optional[int]:
    path = bridge_pid_path()
    if not path.is_file():
        return None
    try:
        pid = int(path.read_text().strip())
    except ValueError:
        return None
    if not _process_alive(pid):
        path.unlink(missing_ok=True)
        clear_bridge_token()
        return None
    return pid


def start_bridge_daemon(
    *,
    host: str = DEFAULT_BRIDGE_HOST,
    port: int = DEFAULT_BRIDGE_PORT,
) -> None:
    """Spawn a detached bridge process if one is not already running."""
    if _read_daemon_pid() is not None:
        return

    pid_path = bridge_pid_path()
    pid_path.parent.mkdir(parents=True, exist_ok=True)

    command = [
        sys.executable,
        "-m",
        "bittensor.extension.daemon",
        "--host",
        host,
        "--port",
        str(port),
        "--pid-file",
        str(pid_path),
    ]
    subprocess.Popen(
        command,
        stdin=subprocess.DEVNULL,
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
        start_new_session=True,
    )


async def bridge_status(url: str, *, token: Optional[str] = None) -> dict:
    async with BridgeClient(url, token=token) as client:
        result = await client.request("bridge.status")
        return result if isinstance(result, dict) else {}


async def bridge_is_reachable(url: str) -> bool:
    token = read_bridge_token()
    if not token:
        return False
    try:
        await bridge_status(url, token=token)
        return True
    except Exception:
        return False


async def wait_for_bridge(url: str, *, timeout: float = 15.0) -> None:
    deadline = asyncio.get_running_loop().time() + timeout
    while asyncio.get_running_loop().time() < deadline:
        if await bridge_is_reachable(url):
            return
        await asyncio.sleep(0.15)
    raise BridgeError("extension bridge did not start; try again in a moment")


async def wait_for_extension(
    url: str,
    *,
    page_url: str,
    timeout: float = 180.0,
    open_browser: bool = True,
    browser: Optional[str] = None,
    on_waiting: Optional[Callable[[str, dict], None]] = None,
) -> None:
    """Wait until a browser tab is connected and an extension is authorized."""
    opened = False
    deadline = asyncio.get_running_loop().time() + timeout
    last_notice = 0.0

    while asyncio.get_running_loop().time() < deadline:
        try:
            status = await bridge_status(url)
        except Exception:
            status = {}

        account_count = status.get("account_count")
        if status.get("browser_connected") and isinstance(account_count, int) and account_count > 0:
            return

        now = asyncio.get_running_loop().time()
        if on_waiting is not None and now - last_notice >= 5.0:
            on_waiting(page_url, status)
            last_notice = now

        if open_browser and not opened:
            open_bridge_page(page_url, browser=browser)
            opened = True
        await asyncio.sleep(0.25)

    raise BridgeError(
        "extension not ready — in the browser tab, allow “Bittensor SDK” to access your "
        f"extension, then run the command again: {page_url}"
    )


async def restart_bridge_daemon(
    *,
    host: str = DEFAULT_BRIDGE_HOST,
    port: int = DEFAULT_BRIDGE_PORT,
    url: Optional[str] = None,
) -> dict:
    """Stop any running bridge and start a fresh daemon with a new session id."""
    ws_url = url or bridge_ws_url(host, port)
    stop_bridge_daemon()
    deadline = asyncio.get_running_loop().time() + 5.0
    while asyncio.get_running_loop().time() < deadline:
        if not await bridge_is_reachable(ws_url):
            break
        await asyncio.sleep(0.1)

    start_bridge_daemon(host=host, port=port)
    await wait_for_bridge(ws_url)
    return await bridge_status(ws_url)


async def ensure_bridge(
    *,
    bridge_url: Optional[str] = None,
    host: str = DEFAULT_BRIDGE_HOST,
    port: int = DEFAULT_BRIDGE_PORT,
    open_browser: bool = True,
    browser: Optional[str] = None,
    fresh: bool = False,
    on_waiting: Optional[Callable[[str, dict], None]] = None,
) -> str:
    """Return a bridge WebSocket URL, starting the daemon and browser flow if needed.

    With ``fresh=True`` the daemon is restarted so each command gets an isolated
    session; stale bridge tabs in other browsers cannot hijack signing.
    """
    if bridge_url is not None:
        host, port = _parse_bridge_target(bridge_url)
    url = bridge_ws_url(host, port)

    if fresh:
        status = await restart_bridge_daemon(host=host, port=port, url=url)
    elif not await bridge_is_reachable(url):
        start_bridge_daemon(host=host, port=port)
        await wait_for_bridge(url)
        status = await bridge_status(url)
    else:
        status = await bridge_status(url)

    session_id = status.get("session_id")
    if not isinstance(session_id, str) or not session_id:
        raise BridgeError("extension bridge returned no session id")

    page_url = bridge_page_url(host, port, session_id=session_id)
    account_count = status.get("account_count")
    browser_connected = bool(status.get("browser_connected"))

    if fresh or not browser_connected or not (isinstance(account_count, int) and account_count > 0):
        await wait_for_extension(
            url,
            page_url=page_url,
            open_browser=open_browser,
            browser=browser,
            on_waiting=on_waiting,
        )

    return url


def stop_bridge_daemon() -> bool:
    pid = _read_daemon_pid()
    if pid is None:
        clear_bridge_token()
        return False
    try:
        os.kill(pid, signal.SIGTERM)
    except OSError:
        bridge_pid_path().unlink(missing_ok=True)
        clear_bridge_token()
        return False
    bridge_pid_path().unlink(missing_ok=True)
    clear_bridge_token()
    return True
