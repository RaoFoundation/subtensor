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

# Running a command group bare (e.g. `btcli addr`) prints help via
# NoArgsIsHelpError, which inherits UsageError's exit code 2. That makes
# terminals flag the block as a failed command; showing help on request is
# informational, so exit 0. Genuine usage errors still exit 2.
NoArgsIsHelpError.exit_code = 0

# These are Typer's runtime configuration attributes. Its inferred literal
# defaults describe the shipped palette, not the values accepted by Rich.
_STYLES = {
    # Option and command names: emphasis without hue.
    "STYLE_OPTION": "bold",
    "STYLE_SWITCH": "bold",
    "STYLE_NEGATIVE_OPTION": "bold",
    "STYLE_NEGATIVE_SWITCH": "bold",
    "STYLE_COMMANDS_TABLE_FIRST_COLUMN": "bold",
    # Metadata recedes.
    "STYLE_TYPES": "dim",
    "STYLE_USAGE": "dim",
    "STYLE_OPTION_ENVVAR": "dim",
    # Error state is the only color, in the shared pastel shade.
    "STYLE_REQUIRED_SHORT": PASTEL_RED,
    "STYLE_REQUIRED_LONG": f"dim {PASTEL_RED}",
    "STYLE_DEPRECATED": PASTEL_RED,
    "STYLE_ERRORS_PANEL_BORDER": PASTEL_RED,
    "STYLE_ABORTED": PASTEL_RED,
}
for _name, _style in _STYLES.items():
    # Catch renamed/removed hooks on a Typer upgrade instead of silently
    # creating an attribute the renderer never reads.
    if not isinstance(getattr(_help, _name), str):
        raise TypeError(f"Typer help style {_name} must be a string")
    setattr(_help, _name, _style)
