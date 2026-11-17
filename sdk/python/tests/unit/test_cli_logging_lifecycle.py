"""Embedded CLI invocations must leave SDK and host diagnostics usable."""

import importlib
import io
import logging

import pytest
from typer.testing import CliRunner

from bittensor.cli import logs

main = importlib.import_module("bittensor.cli.main")
NAMES = ("bittensor", "bittensor.transport.raw_websocket", "websockets")


def snapshot():
    return [
        (logger.level, logger.propagate, tuple(logger.handlers))
        for logger in map(logging.getLogger, NAMES)
    ]


@pytest.fixture
def host_logging(monkeypatch):
    monkeypatch.delenv("BITTENSOR_LOG", raising=False)
    original = snapshot()
    get_named = getattr(logging, "getHandlerByName", None)
    original_names = {name: get_named(name) for name in ("btcli", "host")} if get_named else {}
    owned = []
    for index, name in enumerate(NAMES):
        logger = logging.getLogger(name)
        handler = logging.StreamHandler(io.StringIO())
        # Even a pre-existing handler with the CLI name belongs to the host.
        handler.set_name("btcli" if index == 0 else "host")
        owned.append(handler)
        logger.handlers = [handler]
        logger.setLevel(logging.INFO + index)
        logger.propagate = index != 1
    expected = snapshot()
    try:
        yield expected
    finally:
        for name, (level, propagate, handlers) in zip(NAMES, original):
            logger = logging.getLogger(name)
            logger.handlers = list(handlers)
            logger.setLevel(level)
            logger.propagate = propagate
        for handler in owned:
            handler.close()
        for name, handler in original_names.items():
            if handler is not None:
                handler.set_name(name)


@pytest.mark.parametrize(
    "arguments,exit_code",
    [
        (["explain", "insufficient_balance"], 0),
        (["-vvv", "explain", "not_an_error_code"], 1),
        (["--max-controller-refill", "invalid", "tools"], 2),
    ],
)
def test_cli_restores_host_logging_after_success_and_errors(
    host_logging, caplog, arguments, exit_code
):
    before = snapshot()
    get_named = getattr(logging, "getHandlerByName", None)
    named_before = get_named("btcli") if get_named else None
    for _ in range(2):
        result = CliRunner().invoke(main.app, arguments)
        assert result.exit_code == exit_code, result.output
        assert snapshot() == before
        if get_named:
            assert get_named("btcli") is named_before
        assert all(not handler._closed for _, _, handlers in host_logging for handler in handlers)
    logging.getLogger("bittensor.wallets").warning("SDK warning remains visible after CLI")
    assert "SDK warning remains visible after CLI" in caplog.text


def test_nested_cli_restores_outer_verbosity_and_handler(host_logging, monkeypatch):
    before = snapshot()
    observed = []

    def catalog():
        outer = snapshot()
        get_named = getattr(logging, "getHandlerByName", None)
        outer_named = get_named("btcli") if get_named else None
        assert outer[0][0:2] == (logging.DEBUG, False)
        assert outer[1][0] == logging.DEBUG
        assert outer[2][0:2] == (logging.DEBUG, False)
        result = CliRunner().invoke(main.app, ["--quiet", "explain", "insufficient_balance"])
        assert result.exit_code == 0, result.output
        assert snapshot() == outer
        if get_named:
            assert get_named("btcli") is outer_named
        assert all(not handler._closed for _, _, handlers in outer for handler in handlers)
        observed.append(True)
        return []

    monkeypatch.setattr(main, "list_tools", catalog)
    result = CliRunner().invoke(main.app, ["-vvv", "tools"])
    assert result.exit_code == 0, result.exception
    assert observed == [True]
    assert snapshot() == before


def test_reconfiguration_closes_only_invocation_owned_handlers(host_logging, monkeypatch):
    before = snapshot()
    created = []
    handler_type = logs.RichHandler

    def make_handler(**kwargs):
        handler = handler_type(**kwargs)
        created.append(handler)
        return handler

    monkeypatch.setattr(logs, "RichHandler", make_handler)
    with pytest.raises(RuntimeError, match="command failed"), logs.cli_logging(verbosity=1):
        logs.setup_logging(verbosity=3)
        outer = snapshot()
        with logs.cli_logging(quiet=True):
            assert logging.getLogger("bittensor").level == logging.ERROR
            logs.setup_logging(verbosity=2)
        assert snapshot() == outer
        assert not created[1]._closed
        assert all(handler._closed for handler in created[2:])
        raise RuntimeError("command failed")
    assert snapshot() == before
    assert len(created) == 4
    assert all(handler._closed for handler in created)
    assert all(not handler._closed for _, _, handlers in host_logging for handler in handlers)
