"""Interactive secret input that echoes ``*`` for each typed character.

:func:`getpass.getpass` shows nothing while the user types. That is
disorienting for wallet passwords, so the CLI uses this helper instead: it
reads one keystroke at a time and prints a ``*`` per character, supporting
backspace and Ctrl-U. When stdin is not an interactive terminal (piped
input, CI) it falls back to :func:`getpass.getpass`, which reads a plain
line in that case.
"""

from __future__ import annotations

import codecs
import getpass
import os
import sys

if sys.platform == "win32":
    import msvcrt
else:
    import select
    import termios
    import tty

__all__ = ["masked_input"]

_BACKSPACE = ("\x7f", "\x08")


def masked_input(prompt: str) -> str:
    """Read a secret from the terminal, echoing ``*`` per character.

    The prompt and echo go to stderr so redirected stdout stays clean,
    matching how the CLI prompts elsewhere.
    """
    if not sys.stdin.isatty():
        return getpass.getpass(prompt)
    if sys.platform == "win32":
        return _masked_input_windows(prompt)
    try:
        return _masked_input_posix(prompt)
    except (termios.error, OSError):
        return getpass.getpass(prompt)


def _erase_stars(count: int) -> None:
    sys.stderr.write("\b \b" * count)
    sys.stderr.flush()


def _echo(text: str) -> None:
    sys.stderr.write(text)
    sys.stderr.flush()


def _masked_input_posix(prompt: str) -> str:
    fd = sys.stdin.fileno()
    _echo(prompt)
    # Read raw bytes and decode incrementally so multi-byte (UTF-8)
    # characters get exactly one star each.
    decoder = codecs.getincrementaldecoder(sys.stdin.encoding or "utf-8")(errors="replace")
    old_attrs = termios.tcgetattr(fd)
    chars: list[str] = []
    try:
        # cbreak (not raw) keeps ISIG on, so Ctrl-C still raises
        # KeyboardInterrupt and the finally block restores the terminal.
        tty.setcbreak(fd, termios.TCSADRAIN)
        while True:
            byte = os.read(fd, 1)
            if not byte or byte in (b"\r", b"\n"):
                break
            if byte in (b"\x7f", b"\x08"):
                if chars:
                    chars.pop()
                    _erase_stars(1)
                continue
            if byte == b"\x15":  # Ctrl-U clears the whole entry
                _erase_stars(len(chars))
                chars.clear()
                continue
            if byte == b"\x04":  # Ctrl-D
                if not chars:
                    raise EOFError
                continue
            if byte == b"\x1b":
                _discard_escape_sequence(fd)
                continue
            char = decoder.decode(byte)
            if not char or char < " ":
                continue
            chars.append(char)
            _echo("*")
    finally:
        termios.tcsetattr(fd, termios.TCSADRAIN, old_attrs)
        _echo("\n")
    return "".join(chars)


def _discard_escape_sequence(fd: int) -> None:
    """Swallow the rest of an ANSI escape sequence (e.g. an arrow key).

    Only the sequence itself is consumed — a CSI sequence ends at its final
    byte (``@`` through ``~``) — so keystrokes typed right after it, like
    Enter, are left for the main loop.
    """
    if not select.select([fd], [], [], 0.05)[0]:
        return  # a lone Esc keypress, nothing follows
    lead = os.read(fd, 1)
    if lead not in (b"[", b"O"):
        return
    while select.select([fd], [], [], 0.05)[0]:
        byte = os.read(fd, 1)
        if byte != b"[" and b"@" <= byte <= b"~":
            return


def _masked_input_windows(prompt: str) -> str:
    _echo(prompt)
    chars: list[str] = []
    while True:
        char = msvcrt.getwch()
        if char in ("\r", "\n"):
            break
        if char == "\x03":
            raise KeyboardInterrupt
        if char in _BACKSPACE:
            if chars:
                chars.pop()
                _erase_stars(1)
            continue
        if char == "\x15":
            _erase_stars(len(chars))
            chars.clear()
            continue
        if char in ("\x00", "\xe0"):  # prefix for function/arrow keys
            msvcrt.getwch()
            continue
        if char < " ":
            continue
        chars.append(char)
        _echo("*")
    _echo("\n")
    return "".join(chars)
