"""CLI logging configuration.

The SDK emits diagnostics through stdlib loggers under ``bittensor.*`` and
never configures handlers itself (library convention). The CLI is the
application, so this module wires those loggers to a Rich handler on stderr —
exactly once, at entry — keeping stdout pure data for ``--json`` piping.

Verbosity ladder (the ``-v`` flag counts):

    --quiet   ERROR       only unrecoverable problems
    (default) WARNING     degraded-but-continuing conditions
    -v        INFO        connection lifecycle, endpoint fallback, retries
    -vv       DEBUG       full SDK diagnostics
    -vvv      DEBUG+      raw websocket frames and the ``websockets`` library

``BITTENSOR_LOG`` (error/warning/info/debug/trace) overrides the flags, which
is handy for CI runs and bug reports where editing the command line is
awkward.

User-facing results, prompts, and errors are NOT logging — they go through
``cli.output.Output``. Logging is diagnostics only.
"""

from __future__ import annotations

import logging
import os

from rich.console import Console
from rich.logging import RichHandler

ENV_VAR = "BITTENSOR_LOG"

# Env value -> (verbosity, quiet), mirroring the flag ladder.
_ENV_LEVELS: dict[str, tuple[int, bool]] = {
    "error": (0, True),
    "warning": (0, False),
    "info": (1, False),
    "debug": (2, False),
    "trace": (3, False),
}

_RAW_WEBSOCKET_LOGGER = "bittensor.transport.raw_websocket"
# Third-party loggers worth surfacing at trace level.
_TRACE_THIRD_PARTY = ("websockets",)


def setup_logging(verbosity: int = 0, quiet: bool = False) -> None:
    """Configure diagnostic logging for the CLI process. Safe to call again;
    the previous handler is replaced rather than stacked."""
    env = os.environ.get(ENV_VAR, "").strip().lower()
    if env in _ENV_LEVELS:
        verbosity, quiet = _ENV_LEVELS[env]

    if verbosity <= 0:
        level = logging.ERROR if quiet else logging.WARNING
    elif verbosity == 1:
        level = logging.INFO
    else:
        level = logging.DEBUG

    handler = RichHandler(
        console=Console(stderr=True),
        show_time=False,
        show_path=verbosity >= 2,
        rich_tracebacks=verbosity >= 2,
    )

    root = logging.getLogger("bittensor")
    root.setLevel(level)
    # Don't bubble up to the root logger: the host process is us, and
    # propagation would double-print if anything else configures root.
    root.propagate = False
    _replace_handler(root, handler)

    # Frame-level tracing is opt-in even at -vv: a plain DEBUG run shouldn't
    # dump every websocket payload. INFO here masks the inherited DEBUG.
    raw = logging.getLogger(_RAW_WEBSOCKET_LOGGER)
    raw.setLevel(logging.DEBUG if verbosity >= 3 else logging.INFO)

    for name in _TRACE_THIRD_PARTY:
        third_party = logging.getLogger(name)
        if verbosity >= 3:
            third_party.setLevel(logging.DEBUG)
            third_party.propagate = False
            _replace_handler(third_party, handler)
        else:
            # Undo a previous trace-level setup (repeat calls in one process).
            _remove_our_handlers(third_party)
            third_party.setLevel(logging.NOTSET)
            third_party.propagate = True


def _replace_handler(target: logging.Logger, handler: logging.Handler) -> None:
    _remove_our_handlers(target)
    handler.set_name("btcli")
    target.addHandler(handler)


def _remove_our_handlers(target: logging.Logger) -> None:
    for existing in list(target.handlers):
        if existing.get_name() == "btcli":
            target.removeHandler(existing)
