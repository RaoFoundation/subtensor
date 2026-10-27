"""Helpers for handling secret material at the CLI boundary.

Secrets passed as command-line flags leak into shell history and ``ps``
output; secrets printed to the terminal land in scrollback. These helpers
warn about the former and route the latter to the system clipboard.
"""

from __future__ import annotations

import shutil
import subprocess

from .output import STYLE_WARNING, Output

# Tried in order; the first tool on PATH wins. pbcopy is macOS, wl-copy is
# Wayland, xclip/xsel are X11.
_CLIPBOARD_COMMANDS: tuple[tuple[str, ...], ...] = (
    ("pbcopy",),
    ("wl-copy",),
    ("xclip", "-selection", "clipboard"),
    ("xsel", "--clipboard", "--input"),
)


def warn_argv_secrets(output: Output, provided: dict[str, object]) -> None:
    """Warn when secret-bearing flags were given on the command line.

    ``provided`` maps flag spelling (``--mnemonic``) to the value the command
    received. Only flags with a non-empty value warn: these options have no
    env or config source, so a value at command entry can only have come from
    argv (secure-prompt fallbacks fill in *after* this check).
    """
    flags = [flag for flag, value in provided.items() if value]
    if not flags:
        return
    names = ", ".join(f"`{flag}`" for flag in flags)
    output.message(
        f"[{STYLE_WARNING}]warning:[/{STYLE_WARNING}] secrets passed as flags ({names}) "
        "land in shell history and `ps` output — omit the flag to be prompted without echo"
    )


def copy_to_clipboard(text: str) -> bool:
    """Copy ``text`` to the system clipboard; True when a tool succeeded."""
    for command in _CLIPBOARD_COMMANDS:
        if shutil.which(command[0]) is None:
            continue
        try:
            subprocess.run(
                command,
                input=text.encode(),
                check=True,
                capture_output=True,
                timeout=10,
            )
        except (OSError, subprocess.SubprocessError):
            continue
        return True
    return False


def copy_secret_to_clipboard(output: Output, text: str, label: str) -> bool:
    """Copy a secret to the clipboard, confirming without echoing it.

    Returns False (after a warning) when no clipboard tool is available, in
    which case the caller should fall back to printing.
    """
    if copy_to_clipboard(text):
        output.message(f"{label} copied to clipboard (not printed)")
        return True
    output.message(
        f"[{STYLE_WARNING}]warning:[/{STYLE_WARNING}] no clipboard tool found "
        "(pbcopy, wl-copy, xclip, or xsel) — printing instead"
    )
    return False
