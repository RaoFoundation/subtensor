"""Background bridge process started automatically by ``--signer extension``."""

from __future__ import annotations

import argparse
import asyncio
import os
import signal
from pathlib import Path

from .bridge import DEFAULT_BRIDGE_HOST, DEFAULT_BRIDGE_PORT, run_bridge
from .tokens import clear_bridge_token, write_bridge_token


async def _serve(host: str, port: int, pid_file: Path) -> None:
    server = await run_bridge(host=host, port=port, open_browser=False)
    write_bridge_token(server.state.client_token)
    pid_file.parent.mkdir(parents=True, exist_ok=True)
    pid_file.write_text(str(os.getpid()))
    stop = asyncio.Event()

    def _shutdown(*_args: object) -> None:
        stop.set()

    loop = asyncio.get_running_loop()
    for sig in (signal.SIGTERM, signal.SIGINT):
        try:
            loop.add_signal_handler(sig, _shutdown)
        except NotImplementedError:
            signal.signal(sig, lambda *_: _shutdown())

    try:
        await stop.wait()
    finally:
        pid_file.unlink(missing_ok=True)
        clear_bridge_token()
        await server.stop()


def main() -> None:
    parser = argparse.ArgumentParser(description="Bittensor extension bridge daemon")
    parser.add_argument("--host", default=DEFAULT_BRIDGE_HOST)
    parser.add_argument("--port", type=int, default=DEFAULT_BRIDGE_PORT)
    parser.add_argument("--pid-file", type=Path, required=True)
    args = parser.parse_args()
    asyncio.run(_serve(args.host, args.port, args.pid_file))


if __name__ == "__main__":
    main()
