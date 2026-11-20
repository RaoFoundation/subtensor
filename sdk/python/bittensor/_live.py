"""Pause a live CLI spinner so an interactive prompt can own the terminal.

``Output.activity`` registers a suspend callback on a context var while the
Rich status line is animating. Password reads and Rich ``Console.input``
call :func:`pause_live_display` so the spinner cannot fight the prompt for
the same line (each typed ``*`` otherwise lands on its own row).
"""

from __future__ import annotations

import contextlib
from collections.abc import Callable, Iterator
from contextvars import ContextVar, Token
from typing import Optional, TextIO

from rich.console import Console
from rich.text import TextType

_Pause = Callable[[], contextlib.AbstractContextManager[None]]

_pause: ContextVar[Optional[_Pause]] = ContextVar("bt_live_pause", default=None)


def set_live_pause(pause: Optional[_Pause]) -> Token[Optional[_Pause]]:
    """Install the current activity's suspend callback (or clear it)."""
    return _pause.set(pause)


def reset_live_pause(token: Token[Optional[_Pause]]) -> None:
    """Restore the previous suspend callback after an activity ends."""
    _pause.reset(token)


@contextlib.contextmanager
def pause_live_display() -> Iterator[None]:
    """Stop the live spinner for the duration of an interactive read.

    No-op when no activity is running. Re-entrant: nested prompts keep the
    spinner stopped until the outermost pause exits.
    """
    pause = _pause.get()
    if pause is None:
        yield
        return
    with pause():
        yield


@contextlib.contextmanager
def guard_rich_input() -> Iterator[None]:
    """Route every Rich ``Console.input`` through :func:`pause_live_display`.

    Covers ``Confirm.ask``, ``Prompt.ask``, and ad-hoc ``console.input``
    calls, including consoles the CLI builds outside ``Output``.
    """
    original = Console.input

    def guarded(
        self: Console,
        prompt: TextType = "",
        *,
        markup: bool = True,
        emoji: bool = True,
        password: bool = False,
        stream: Optional[TextIO] = None,
    ) -> str:
        with pause_live_display():
            return original(
                self,
                prompt,
                markup=markup,
                emoji=emoji,
                password=password,
                stream=stream,
            )

    Console.input = guarded  # type: ignore[method-assign]
    try:
        yield
    finally:
        Console.input = original
