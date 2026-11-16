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
from collections.abc import Iterator
from contextlib import contextmanager
from contextvars import ContextVar

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
_session_handlers: ContextVar[list[logging.Handler] | None] = ContextVar(
    "btcli_logging_handlers", default=None
)


@contextmanager
def cli_logging(verbosity: int = 0, quiet: bool = False) -> Iterator[None]:
    """Configure one invocation, then restore its embedding application's loggers.

    A nested invocation temporarily replaces the outer CLI handler. Keep that
    handler alive for restoration; only close handlers created in this scope,
    including handlers replaced by subcommand verbosity overrides.
    """
    targets = [
        logging.getLogger(name)
        for name in ("bittensor", _RAW_WEBSOCKET_LOGGER, *_TRACE_THIRD_PARTY)
    ]
    previous = [
        (target, target.level, target.propagate, tuple(target.handlers)) for target in targets
    ]
    # Python 3.12 exposes the named-handler registry publicly. Older versions
    # still allow restoring the named handler attached to an affected logger.
    get_named = getattr(logging, "getHandlerByName", None)
    previous_named = (
        get_named("btcli")
        if get_named
        else next(
            (
                handler
                for _, _, _, handlers in previous
                for handler in handlers
                if handler.get_name() == "btcli"
            ),
            None,
        )
    )
    created: list[logging.Handler] = []
    token = _session_handlers.set(created)
    try:
        setup_logging(verbosity=verbosity, quiet=quiet)
        yield
    finally:
        _session_handlers.reset(token)
        for target, level, propagate, handlers in previous:
            for handler in tuple(target.handlers):
                target.removeHandler(handler)
            for handler in handlers:
                target.addHandler(handler)
            target.setLevel(level)
            target.propagate = propagate
        for handler in created:
            handler.close()
        if previous_named is not None:
            previous_named.set_name("btcli")


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
    created = _session_handlers.get()
    if created is not None:
        created.append(handler)

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
