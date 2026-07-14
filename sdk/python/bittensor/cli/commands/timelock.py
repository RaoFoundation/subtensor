"""`btcli timelock`: seal data that anyone can open at a known future time.

Drand timelock encryption as a standalone tool: encrypt to a moment (duration,
absolute time, or beacon round), publish the ciphertext anywhere, and anyone
can decrypt once the moment passes -- nobody can before, including the author.
The same mechanism the chain uses for commit-reveal weights, usable for
challenge answers, embargoed announcements, and sealed bids.
"""

from __future__ import annotations

import string
from datetime import timezone
from pathlib import Path
from typing import Optional

import typer

from ...timelock import (
    Timelocked,
    TimelockError,
    TimelockNotReady,
    format_duration,
    parse_duration,
)
from ...timelock import (
    decrypt as timelock_decrypt,
)
from ...timelock import (
    encrypt as timelock_encrypt,
)
from ..context import AppContext, ctx_of
from ..globals import with_globals
from ..prompt import PromptSpec, fill_missing

app = typer.Typer(
    no_args_is_help=True,
    help="Timelock encryption: seal data anyone can open at a set future time.",
)


def _is_file(candidate: str) -> bool:
    """Whether the argument names an existing file. Never raises: arguments
    like long hex blobs overflow the OS path-length limit in ``stat``."""
    try:
        return Path(candidate).is_file()
    except (OSError, ValueError):
        return False


def _parse_ciphertext_answer(_app_ctx: AppContext, raw: str) -> str:
    """Validate a prompted ciphertext answer: an existing file or parseable hex."""
    if _is_file(raw):
        return raw
    try:
        Timelocked.parse(raw)
    except TimelockError as error:
        raise ValueError(str(error)) from None
    return raw


def _parse_duration_answer(_app_ctx: AppContext, raw: str) -> str:
    """Validate a prompted duration; the raw text is re-parsed by encrypt."""
    try:
        parse_duration(raw)
    except TimelockError as error:
        raise ValueError(str(error)) from None
    return raw


def _read_ciphertext(app_ctx: AppContext, data: Optional[str], file: Optional[str]) -> Timelocked:
    """Resolve the ciphertext from the positional argument or --file.

    The positional argument may be the hex itself or a path to a file; files
    may hold raw ciphertext bytes or their hex text. When neither is given,
    a terminal session is prompted (scripts get a missing-option error).
    """
    if data is not None and file is not None:
        app_ctx.output.error(
            "give the ciphertext exactly one way",
            help="pass the hex or a file path as an argument, or use --file",
        )
        raise typer.Exit(1)
    if data is None and file is None:
        answers: dict[str, str] = {}
        fill_missing(
            app_ctx,
            [
                PromptSpec(
                    field="ciphertext",
                    flag="ciphertext",
                    help="Ciphertext hex (from encrypt), or a path to a ciphertext file.",
                    parse=_parse_ciphertext_answer,
                    positional=True,
                )
            ],
            answers,
        )
        data = answers["ciphertext"]
    if file is None and data is not None and _is_file(data):
        file = data
        data = None
    if file is not None:
        raw = Path(file).read_bytes()
        try:
            # Whitespace-tolerant: hex may arrive line-wrapped from a pipe.
            text = "".join(raw.decode("ascii").split())
        except UnicodeDecodeError:
            text = ""
        if text and all(c in string.hexdigits for c in text.removeprefix("0x")):
            return Timelocked.parse(text)
        return Timelocked.parse(raw)
    return Timelocked.parse(data)  # type: ignore[arg-type]


def _reveal_note(sealed: Timelocked) -> str:
    when = sealed.reveal_at.astimezone(timezone.utc).isoformat(timespec="seconds")
    if sealed.revealed:
        return f"revealed since {when} (round {sealed.reveal_round})"
    return (
        f"reveals at {when} (round {sealed.reveal_round}, in {format_duration(sealed.remaining)})"
    )


@app.command("encrypt")
@with_globals
def encrypt_cmd(
    ctx: typer.Context,
    data: Optional[str] = typer.Argument(
        None,
        help="Text to seal, or a path to a file whose contents to seal.",
    ),
    reveal_in: Optional[str] = typer.Option(
        None,
        "--in",
        help="Duration until the reveal: '30s', '15m', '1h30m', '2d', or plain seconds.",
    ),
    reveal_at: Optional[str] = typer.Option(
        None,
        "--at",
        help="Absolute reveal moment, ISO-8601: '2026-07-08T12:00Z' (no offset = UTC).",
    ),
    reveal_round: Optional[int] = typer.Option(
        None, "--round", help="Explicit drand quicknet round number to encrypt to."
    ),
    file: Optional[str] = typer.Option(
        None, "--file", help="Read the payload from this file instead of the argument."
    ),
    out: Optional[str] = typer.Option(
        None, "--out", help="Write raw ciphertext bytes here instead of printing hex."
    ),
):
    """Seal text or a file until a future moment; prints the ciphertext hex.

    The reveal round is embedded in the ciphertext, so the hex is all anyone
    needs to decrypt later -- no key, no metadata, no account.
    """
    app_ctx: AppContext = ctx_of(ctx)
    if data is not None and file is not None:
        app_ctx.output.error(
            "give the payload exactly one way",
            help="pass text or a file path as an argument, or use --file",
        )
        raise typer.Exit(1)

    # A bare `btcli timelock encrypt` asks for what's missing (terminal
    # sessions only; scripts get the usual missing-option error).
    specs = []
    if data is None and file is None:
        specs.append(
            PromptSpec(
                field="data",
                flag="data",
                help="Text to seal, or a path to a file whose contents to seal.",
                parse=lambda _ctx, raw: raw,
                positional=True,
            )
        )
    if reveal_in is None and reveal_at is None and reveal_round is None:
        specs.append(
            PromptSpec(
                field="reveal_in",
                flag="--in",
                help="Duration until the reveal: '30s', '15m', '1h30m', '2d', or plain seconds.",
                parse=_parse_duration_answer,
            )
        )
    if specs:
        answers: dict[str, str] = {}
        fill_missing(app_ctx, specs, answers)
        data = answers.get("data", data)
        reveal_in = answers.get("reveal_in", reveal_in)

    if file is None and data is not None and _is_file(data):
        file = data
        app_ctx.output.message(f"sealing file {file}")
    payload: str | bytes = Path(file).read_bytes() if file else data  # type: ignore[assignment]

    try:
        sealed = timelock_encrypt(
            payload, reveal_in, reveal_at=reveal_at, reveal_round=reveal_round
        )
    except TimelockError as error:
        app_ctx.output.error(str(error))
        raise typer.Exit(1)

    record = {
        "reveal_round": sealed.reveal_round,
        "reveal_at": sealed.reveal_at.isoformat(timespec="seconds"),
        "reveal_in_seconds": int(sealed.remaining.total_seconds()),
        "bytes": len(sealed.ciphertext),
    }
    if out:
        Path(out).write_bytes(sealed.ciphertext)
        if app_ctx.output.json_mode:
            app_ctx.output.value({**record, "path": out})
        else:
            app_ctx.output.message(f"wrote {len(sealed.ciphertext)} bytes to {out}")
            app_ctx.output.message(_reveal_note(sealed))
        return
    if app_ctx.output.json_mode:
        app_ctx.output.value({"ciphertext": sealed.hex(), **record})
    else:
        # Hex on stdout (pipeable), metadata on stderr.
        app_ctx.output.value(sealed.hex())
        app_ctx.output.message(_reveal_note(sealed))


@app.command("decrypt")
@with_globals
def decrypt_cmd(
    ctx: typer.Context,
    data: Optional[str] = typer.Argument(
        None, help="Ciphertext hex (from encrypt), or a path to a ciphertext file."
    ),
    file: Optional[str] = typer.Option(
        None, "--file", help="Read the ciphertext from this file (raw bytes or hex)."
    ),
    wait: bool = typer.Option(
        False, "--wait", help="Sleep until the reveal moment instead of failing early."
    ),
    timeout: Optional[float] = typer.Option(
        None, "--timeout", help="Give up after this many seconds (with --wait)."
    ),
    out: Optional[str] = typer.Option(
        None, "--out", help="Write the decrypted bytes here instead of printing."
    ),
):
    """Open a timelocked ciphertext once its reveal moment has passed."""
    app_ctx: AppContext = ctx_of(ctx)
    try:
        sealed = _read_ciphertext(app_ctx, data, file)
    except TimelockError as error:
        app_ctx.output.error(str(error))
        raise typer.Exit(1)

    if wait and not sealed.revealed and not app_ctx.output.json_mode:
        app_ctx.output.message(f"waiting: {_reveal_note(sealed)}")
    try:
        plaintext = timelock_decrypt(sealed, wait=wait, timeout=timeout)
    except TimelockNotReady as error:
        app_ctx.output.error(
            str(error),
            help="re-run with --wait, or come back after "
            f"{error.reveal_at.isoformat(timespec='seconds')}",
        )
        raise typer.Exit(1)
    except TimelockError as error:
        app_ctx.output.error(str(error))
        raise typer.Exit(1)

    if out:
        Path(out).write_bytes(plaintext)
        if app_ctx.output.json_mode:
            app_ctx.output.value(
                {"path": out, "bytes": len(plaintext), "reveal_round": sealed.reveal_round}
            )
        else:
            app_ctx.output.message(f"wrote {len(plaintext)} bytes to {out}")
        return
    try:
        text: Optional[str] = plaintext.decode()
    except UnicodeDecodeError:
        text = None
    if app_ctx.output.json_mode:
        app_ctx.output.value(
            {
                "plaintext": text,
                "plaintext_hex": plaintext.hex(),
                "reveal_round": sealed.reveal_round,
            }
        )
    elif text is not None:
        app_ctx.output.value(text)
    else:
        app_ctx.output.value(plaintext.hex())
        app_ctx.output.message("payload is binary; shown as hex (use --out to write bytes)")


@app.command("show")
@with_globals
def show_cmd(
    ctx: typer.Context,
    data: Optional[str] = typer.Argument(
        None, help="Ciphertext hex (from encrypt), or a path to a ciphertext file."
    ),
    file: Optional[str] = typer.Option(
        None, "--file", help="Read the ciphertext from this file (raw bytes or hex)."
    ),
):
    """Show when a ciphertext unlocks, without decrypting it."""
    app_ctx: AppContext = ctx_of(ctx)
    try:
        sealed = _read_ciphertext(app_ctx, data, file)
    except TimelockError as error:
        app_ctx.output.error(str(error))
        raise typer.Exit(1)
    app_ctx.output.detail(
        "timelock",
        {
            "reveal_round": sealed.reveal_round,
            "reveal_at": sealed.reveal_at.isoformat(timespec="seconds"),
            "revealed": sealed.revealed,
            "remaining": format_duration(sealed.remaining),
            "bytes": len(sealed.ciphertext),
        },
        json_fields={
            "reveal_round": sealed.reveal_round,
            "reveal_at": sealed.reveal_at.isoformat(timespec="seconds"),
            "revealed": sealed.revealed,
            "remaining_seconds": int(sealed.remaining.total_seconds()),
            "bytes": len(sealed.ciphertext),
        },
    )
