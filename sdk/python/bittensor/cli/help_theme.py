"""Restyle typer's rich help screens to match the CLI's palette.

Typer renders --help through module-level style constants in
``typer.rich_utils`` (bold-cyan commands, yellow usage, green/magenta
switches). Overriding them here keeps every help screen on the same
Stripe-muted scheme as cli/output.py: monochrome attributes carry the
structure (bold for names, dim for metadata) and pastel red is the only
hue, reserved for error state. The constants are read at render time, so
assigning them once at import is enough.
"""

from __future__ import annotations

import typer.rich_utils as _help

try:  # typer >= 0.17 vendors click as typer._click
    from typer._click.exceptions import NoArgsIsHelpError
except ImportError:  # older typer uses the external click package
    from click.exceptions import (  # type: ignore[no-redef]  # ty: ignore[unresolved-import]
        NoArgsIsHelpError,
    )

from .output import PASTEL_RED

# Running a command group bare (e.g. `btcli addresses`) prints help via
# NoArgsIsHelpError, which inherits UsageError's exit code 2. That makes
# terminals flag the block as a failed command; showing help on request is
# informational, so exit 0. Genuine usage errors still exit 2.
NoArgsIsHelpError.exit_code = 0

# Option and command names: emphasis without hue.
_help.STYLE_OPTION = "bold"
_help.STYLE_SWITCH = "bold"
_help.STYLE_NEGATIVE_OPTION = "bold"
_help.STYLE_NEGATIVE_SWITCH = "bold"
_help.STYLE_COMMANDS_TABLE_FIRST_COLUMN = "bold"

# Metadata (argument types, the "Usage:" label, env vars) recedes.
_help.STYLE_TYPES = "dim"
_help.STYLE_USAGE = "dim"
_help.STYLE_OPTION_ENVVAR = "dim"

# Error state is the only color, in the shared pastel shade.
_help.STYLE_REQUIRED_SHORT = PASTEL_RED
_help.STYLE_REQUIRED_LONG = f"dim {PASTEL_RED}"
_help.STYLE_DEPRECATED = PASTEL_RED
_help.STYLE_ERRORS_PANEL_BORDER = PASTEL_RED
_help.STYLE_ABORTED = PASTEL_RED
