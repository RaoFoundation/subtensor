"""Rendering: one object that speaks either to humans or to scripts.

Every command routes all output through an ``Output`` instance instead of printing
directly, so ``--json`` and ``--quiet`` work uniformly everywhere and a machine
consumer never has to scrape a rich table.
"""

from __future__ import annotations

import contextlib
import json as _json
import re
from datetime import datetime
from pathlib import Path
from typing import Any, Optional

from rich.console import Console
from rich.markup import escape
from rich.padding import Padding
from rich.prompt import Confirm
from rich.table import Table
from rich.text import Text
from rich.theme import Theme
from rich.tree import Tree

from .. import config as cfg
from ..balance import Balance
from ..error_map import DISPATCH_ERRORS, NAME_TO_CODE
from ..intents import Plan
from ..result import ChainError, ErrorCode, ExtrinsicResult
from ..settings import (
    error_docs_url,
    explorer_account_url,
    explorer_extrinsic_url,
    explorer_subnet_url,
    tx_docs_url,
)
from . import multisig_helpers as ms_helpers

# Stripe-muted palette: almost monochrome — dim for structure (titles, keys,
# metadata), plain for data, and quiet pastel tints where a hue earns its
# place: state (red/yellow/green) and names (blue-gray). No bold anywhere in
# data views; only the error/warning labels keep weight. Rich downgrades
# truecolor to the nearest ANSI color on terminals without truecolor support.
PASTEL_RED = "#cc6666"  # brick, not fire-engine
PASTEL_YELLOW = "#f0c674"  # sand, not neon
PASTEL_GREEN = "#b5bd68"  # olive, not lime
PASTEL_BLUE = "#81a2be"  # slate, not electric

STYLE_ADDRESS = ""  # identifiers print plain, like Stripe request IDs
STYLE_NAME = PASTEL_BLUE  # local names get the one quiet accent, never bold
STYLE_MULTISIG = PASTEL_BLUE
STYLE_CRYPTO = "dim"  # metadata is faint, like Stripe timestamps
STYLE_KEY = "dim"
STYLE_INCIDENTAL = "dim"
STYLE_COMMAND = "bold"  # inline `commands`: emphasis without hue
STYLE_URL = "underline"
STYLE_TITLE = "dim"  # section titles recede; the data carries the weight
STYLE_ERROR = f"bold {PASTEL_RED}"
STYLE_WARNING = f"bold {PASTEL_YELLOW}"
STYLE_HINT = "dim"  # note:/help:/see: lines recede; the error carries the weight
STYLE_SUCCESS = PASTEL_GREEN
STYLE_MESSAGE = ""  # error message text prints plain (color lives in the label)

# Success is marked with a glyph as well as color, so the state survives
# NO_COLOR, monochrome terminals, and colorblind users.
GLYPH_OK = "✓"
GLYPH_FAIL = "✗"

# print_json highlights through the console theme's json.* styles; rich's
# defaults are loud (bold blue keys, green strings, red/green booleans).
# Restyle them like a kv block: dim keys and punctuation, plain values.
_THEME = Theme(
    {
        "json.key": STYLE_KEY,
        "json.brace": "dim",
        "json.str": "",
        "json.number": "",
        "json.bool_true": "",
        "json.bool_false": "",
        "json.null": "dim",
    }
)

_BACKTICK_RE = re.compile(r"`([^`]+)`")
_URL_RE = re.compile(r"https?://[^\s)\]]+")

# An ss58 address embedded in a larger string (generic substrate addresses
# start with '5'; base58 alphabet, no 0/O/I/l).
_SS58_RE = re.compile(r"\b5[1-9A-HJ-NP-Za-km-z]{45,49}\b")

# A netuid reference in prose ("netuid 4", "subnet 4"), optionally already
# carrying its name ("netuid 4 (Targon)"). The id + name span gets hyperlinked
# to the subnet's explorer page.
_NETUID_REF_RE = re.compile(r"\b(?:netuid|subnet)\s+(?P<id>\d+)(?:\s+\((?P<name>[^()]{1,64})\))?")

# A bare netuid reference eligible for a name rewrite: not already carrying a
# parenthetical and not itself parenthesized ("alpha (netuid 4)"), so rewriting
# is idempotent and never nests parens.
_NETUID_BARE_RE = re.compile(r"\b(netuid|subnet)\s+(\d+)\b(?!\s*[()])")


def _diagnostic(text: str) -> str:
    """Apply the rustc message style: lowercase start, no trailing period.

    The first character is lowered only when it starts an ordinary word (next
    character lowercase), so acronyms and identifiers keep their casing.
    """
    text = text.strip()
    if text.endswith(".") and not text.endswith(".."):
        text = text[:-1]
    if len(text) > 1 and text[0].isupper() and text[1].islower():
        text = text[0].lower() + text[1:]
    return text


def _linkify_urls(text: Text) -> Text:
    """Underline every URL in ``text`` and attach it as an OSC-8 hyperlink so
    terminals render it clickable even when wrapping breaks plain detection."""
    for match in _URL_RE.finditer(text.plain):
        url = match.group(0)
        text.stylize(f"{STYLE_URL} link {url}", match.start(), match.end())
    return text


def _prose(text: str) -> Text:
    """Style a sentence: `backticked commands` emphasized, URLs underlined
    and hyperlinked."""
    out = Text()
    pos = 0
    for match in _BACKTICK_RE.finditer(text):
        out.append(text[pos : match.start()])
        out.append(match.group(1), style=STYLE_COMMAND)
        pos = match.end()
    out.append(text[pos:])
    return _linkify_urls(out)


_ADDRESS_KEYS = {"address", "ss58", "multisig", "signer", "coldkeypub", "hotkeypub"}
_NAME_KEYS = {"name", "wallet", "coldkey", "hotkey"}
_INCIDENTAL_KEYS = {"path", "timepoint", "block"}


def _looks_like_ss58(value: Any) -> bool:
    """Cheap ss58 shape check (substrate generic addresses start with '5')."""
    return (
        isinstance(value, str)
        and 46 <= len(value) <= 50
        and value.startswith("5")
        and value.isalnum()
    )


def _address_kind_for_key(key: str) -> Optional[str]:
    """Explorer link kind implied by a field name, or None when the name alone
    can't tell (the caller falls back to the per-address registry)."""
    key = key.lower()
    if "hotkey" in key:
        return "hotkey"
    if (
        "coldkey" in key
        or key.endswith("ss58")
        or key in _ADDRESS_KEYS | {"dest", "wallet", "owner"}
    ):
        return "coldkey"
    return None


def _value_style(key: str, value: Any = None) -> str:
    """Map a field to its semantic style role ('' means unstyled).

    Content wins over key name for addresses: keys like ``coldkey`` hold a
    wallet name in some views and an ss58 address in others.
    """
    if isinstance(value, str) and value.strip() == "—":
        return "dim"
    if isinstance(value, str) and value.startswith(GLYPH_OK):
        return STYLE_SUCCESS
    if isinstance(value, str) and value.startswith(GLYPH_FAIL):
        return PASTEL_RED
    if isinstance(value, str) and value.startswith(("http://", "https://")):
        return STYLE_URL
    key = key.lower()
    if key.endswith("ss58") or key in _ADDRESS_KEYS or _looks_like_ss58(value):
        return STYLE_ADDRESS
    if key.endswith("crypto_type"):
        return STYLE_CRYPTO
    if key in _NAME_KEYS or key.endswith("_name"):
        return STYLE_NAME
    if key in _INCIDENTAL_KEYS:
        return STYLE_INCIDENTAL
    return ""


class Output:
    def __init__(self, json_mode: bool = False, quiet: bool = False, network: Optional[str] = None):
        self.json_mode = json_mode
        self.quiet = quiet
        self.network = network
        # highlight=False: rich's default highlighter colors digits, parens,
        # and quoted strings on its own; every color here should be deliberate.
        self._out = Console(highlight=False, theme=_THEME)
        self._err = Console(stderr=True, highlight=False, theme=_THEME)
        # Local names for ss58 addresses seen this invocation (wallet names,
        # address-book contacts). Used to render "name (ss58)" in human output;
        # JSON output always carries the raw addresses.
        self.address_names: dict[str, str] = {}
        # Address -> "hotkey" | "coldkey", for explorer hyperlinks: hotkeys
        # link to the validator page, coldkeys to the account page.
        self.address_kinds: dict[str, str] = {}
        # netuid -> registered subnet name, for the canonical "4 (Targon)"
        # rendering. Seeded from the on-disk cache so prompts that run before
        # any connection still carry names; refreshed via update_subnet_names
        # whenever a command connects (AppContext.run).
        self.subnet_names: dict[int, str] = cfg.load_subnet_names(network) if network else {}
        self.subnet_names.setdefault(0, "root")
        # netuid -> chain-registered token symbol, for rendering amounts and
        # the canonical netuid text. Seeded from the on-disk cache (same
        # pattern as subnet names); refreshed from the client's connection-
        # scoped map whenever a command connects (AppContext.run).
        self.token_symbols: dict[int, str] = cfg.load_token_symbols(network) if network else {}

    def name_address(self, ss58: Optional[str], name: Optional[str]) -> None:
        """Remember a local name for an address, for human-readable rendering."""
        if ss58 and name and name != ss58:
            self.address_names.setdefault(ss58, name)

    def classify_address(self, ss58: Optional[str], kind: str) -> None:
        """Remember whether an address is a hotkey or coldkey (for explorer links)."""
        if ss58:
            self.address_kinds.setdefault(ss58, kind)

    def account_url(self, ss58: str, kind: Optional[str] = None) -> Optional[str]:
        """Explorer page for an account, or None (unknown kind / unindexed network)."""
        kind = kind or self.address_kinds.get(ss58)
        if not self.network or not kind:
            return None
        return explorer_account_url(self.network, kind, ss58)

    def subnet_url(self, netuid: int) -> Optional[str]:
        """Explorer page for a subnet, or None on unindexed networks."""
        if not self.network:
            return None
        return explorer_subnet_url(self.network, netuid)

    def update_subnet_names(self, names: dict[int, str]) -> None:
        """Install freshly fetched subnet names and persist them to the cache."""
        self.subnet_names.update(names)
        self.subnet_names.setdefault(0, "root")
        if self.network:
            # the cache is cosmetic; never fail a command over it
            with contextlib.suppress(OSError):
                cfg.save_subnet_names(self.network, names)

    def update_token_symbols(self, symbols: dict[int, str]) -> None:
        """Install the connected chain's token symbols (disk persistence is the
        client's job — it caches them at connect)."""
        self.token_symbols.update(symbols)

    def balance(self, rao: int, netuid: int = 0) -> Balance:
        """A display Balance tagged with the known token symbol for its subnet."""
        symbol = self.token_symbols.get(int(netuid)) if netuid else None
        return Balance(int(rao), int(netuid), symbol)

    def unit(self, netuid: int) -> str:
        """Currency symbol for a netuid: ``τ``, the chain symbol, or ``α`` + netuid."""
        return self.balance(0, netuid).unit

    def subnet_text(self, netuid: Any, *, symbol: bool = True) -> Text:
        """Canonical netuid rendering: ``4 (Targon) γ``, hyperlinked to the
        subnet's explorer page. The name renders dim italic (on-chain identity:
        informative but unverified); the token symbol is dim metadata."""
        netuid = int(netuid)
        url = self.subnet_url(netuid)
        out = Text()
        out.append(str(netuid), style=f"link {url}" if url else "")
        name = self.subnet_names.get(netuid)
        if name:
            name_style = "dim italic"
            out.append(f" ({name})", style=f"{name_style} link {url}" if url else name_style)
        if symbol:
            out.append(f" {self.unit(netuid)}", style="dim")
        return out

    def _linked_text(self, value: str, style: str, kind: Optional[str] = None) -> Text:
        """Text for ``value`` with any embedded ss58 spans, netuid references,
        and URLs hyperlinked. ``kind`` forces hotkey/coldkey; otherwise each
        address's registered kind is used (unregistered addresses stay plain)."""
        spans: list[tuple[int, int, str]] = []
        for match in _SS58_RE.finditer(value):
            url = self.account_url(match.group(0), kind)
            if url:
                spans.append((match.start(), match.end(), url))
        for match in _NETUID_REF_RE.finditer(value):
            url = self.subnet_url(int(match.group("id")))
            if url:
                # Link the "4 (Targon)" span, not the "netuid"/"subnet" word.
                spans.append((match.start("id"), match.end(), url))
        for match in _URL_RE.finditer(value):
            # A literal URL links to itself (OSC-8), so it stays clickable
            # even when column padding or wrapping breaks plain detection.
            spans.append((match.start(), match.end(), match.group(0)))
        spans.sort()
        out = Text()
        pos = 0
        for start, end, url in spans:
            if start < pos:
                continue
            out.append(value[pos:start], style=style)
            link_style = f"{style} link {url}" if style else f"link {url}"
            out.append(value[start:end], style=link_style)
            pos = end
        out.append(value[pos:], style=style)
        return out

    def with_names(self, text: str) -> str:
        """Rewrite known bare addresses in ``text`` as ``name (ss58)``."""
        for ss58, name in self.address_names.items():
            if ss58 in text and f"({ss58})" not in text:
                text = text.replace(ss58, f"{name} ({ss58})")
        return text

    def with_subnets(self, text: str) -> str:
        """Rewrite bare netuid references in ``text`` as ``netuid 4 (Targon)``."""

        def _named(match: "re.Match[str]") -> str:
            name = self.subnet_names.get(int(match.group(2)))
            if not name:
                return match.group(0)
            return f"{match.group(1)} {match.group(2)} ({name})"

        return _NETUID_BARE_RE.sub(_named, text)

    def linked_prose(self, text: str, style: str = "") -> Text:
        """Prose with local names substituted in and every address / netuid
        reference hyperlinked — the one rendering for summaries and prompts."""
        return self._linked_text(self.with_subnets(self.with_names(text)), style)

    def confirm(self, prompt: str) -> bool:
        """Ask a y/n question, rendering the prompt as linked prose so subnet
        and account references stay hyperlinked inside the prompt itself."""
        return Confirm.ask(self.linked_prose(prompt), console=self._out, default=False)

    def _json(self, payload: Any) -> None:
        self._out.print_json(_json.dumps(payload, default=str))

    def message(self, text: str) -> None:
        """Informational chatter. Suppressed by --quiet and in --json mode.

        Goes to stderr so piped stdout stays pure data (commands render data
        through ``detail``/``table``/``value``, never through ``message``).
        """
        if not self.quiet and not self.json_mode:
            self._err.print(text)

    @contextlib.contextmanager
    def activity(self, initial: str):
        """Transient stderr spinner with a quiet, non-TTY fallback.

        The yielded updater accepts ``(text, announce=False)``. Interactive
        terminals animate in place; redirected human output only prints
        meaningful announced phase changes. JSON and quiet modes stay silent.
        """
        if self.quiet or self.json_mode:
            yield lambda _text, announce=False: None
            return
        if self._err.is_terminal:
            with self._err.status(
                self.linked_prose(initial, STYLE_HINT),
                spinner="dots",
                spinner_style=STYLE_HINT,
            ) as status:
                yield lambda text, announce=False: status.update(
                    self.linked_prose(text, STYLE_HINT)
                )
            return

        announced: set[str] = set()

        def update(text: str, announce: bool = False) -> None:
            if announce and text not in announced:
                announced.add(text)
                self.message(text)

        yield update

    def value(self, payload: Any) -> None:
        """Emit an arbitrary already-JSON-friendly value (used by generic query).

        Soft-wrapped: this is data output, and hard-wrapping (rich's default at
        the console width, 80 when piped) would corrupt values like hex blobs.
        """
        if self.json_mode:
            self._json(payload)
        else:
            self._out.print(payload, soft_wrap=True)

    def error(
        self,
        text: str,
        *,
        help: Optional[str] = None,
        note: Optional[str] = None,
        see: Optional[str] = None,
    ) -> None:
        """Rustc diagnostic anatomy: ``error:`` states what is wrong (never how
        to fix it), ``note:`` adds context, ``help:`` carries the fix, ``see:``
        points at a reference. Messages are lowercase with no trailing period."""
        if self.json_mode:
            payload: dict[str, Any] = {"error": text}
            if note:
                payload["note"] = note
            if help:
                payload["help"] = help
            if see:
                payload["see"] = see
            self._err.print_json(_json.dumps(payload))
            return
        line = Text()
        line.append("error:", style=STYLE_ERROR)
        line.append(" ")
        message = _prose(_diagnostic(self.with_subnets(text)))
        message.style = STYLE_MESSAGE
        line.append_text(message)
        self._err.print(line)
        if note:
            self._sub_diag("note", note)
        if help:
            self._sub_diag("help", help)
        if see:
            self._sub_diag("see", see)

    def _sub_diag(self, label: str, text: str, *, console: Optional[Console] = None) -> None:
        """Indented sub-diagnostic line (``note:``, ``help:``, ``see:``).

        Defaults to stderr (error context); pass ``console=self._out`` when the
        diagnostic annotates ordinary data output.
        """
        line = Text()
        line.append(f"{label}:".rjust(7))
        line.append(" ")
        line.append_text(_prose(_diagnostic(text)))
        line.style = STYLE_HINT
        (console or self._err).print(line)

    def _kv_line(self, key: str, width: int, content: Text) -> None:
        """One aligned key/value line, never wrapped (hashes and addresses are
        copy targets; a mid-value wrap breaks copy-paste and the alignment)."""
        line = Text("  ", overflow="ignore", no_wrap=True)
        line.append(key.rjust(width), style=STYLE_KEY)
        line.append("  ")
        line.append_text(content)
        self._out.print(line, soft_wrap=True)

    def _print_title(self, title: str) -> None:
        """Section title with netuid references named and hyperlinked."""
        self._out.print(self._linked_text(self.with_subnets(title), STYLE_TITLE))

    def detail(
        self,
        title: Optional[str],
        fields: dict[str, Any],
        json_fields: Optional[dict[str, Any]] = None,
    ) -> None:
        """A single record as key/value pairs (human) or an object (json).

        ``json_fields`` supplies the JSON shape when the human view is a trimmed
        or reformatted rendering of a richer record.
        """
        if self.json_mode:
            self._json(json_fields if json_fields is not None else fields)
            return
        if title:
            self._print_title(title)
        if not fields:
            self._out.print("  [dim]none[/dim]")
            return
        self._print_fields(fields)

    def _print_fields(self, fields: dict[str, Any], indent: int = 2) -> None:
        """Aligned key/value block: dim keys, values colored by semantic role.

        Structured values stay legible instead of printing as raw reprs: dict
        values recurse as indented sub-blocks, lists of records render one
        compact line per record, and lists of scalars join with commas.
        """
        # Reads like subnet_names / weights key by int; coerce for display only.
        labels = [(str(key), value) for key, value in fields.items()]
        width = max((len(label) for label, _ in labels), default=0)
        for label, value in labels:
            # Built with Text (not markup) so values containing "[" render
            # as-is; never wrapped (addresses and hashes are copy targets).
            line = Text(" " * indent, overflow="ignore", no_wrap=True)
            line.append(label.rjust(width), style=STYLE_KEY)
            if isinstance(value, (dict, list)) and not value:
                line.append("  ")
                line.append("none", style="dim")
                self._out.print(line)
                continue
            if isinstance(value, dict):
                self._out.print(line)
                self._print_fields(value, indent + 2)
                continue
            if isinstance(value, list) and all(isinstance(item, dict) for item in value):
                self._out.print(line)
                for item in value:
                    self._out.print(self._record_line(item, indent + 2), soft_wrap=True)
                continue
            line.append("  ")
            if isinstance(value, list):
                value = ", ".join(str(item) for item in value)
            if label.endswith("netuid") and str(value).isdigit():
                line.append_text(self.subnet_text(value))
            else:
                line.append_text(
                    self._linked_text(
                        str(value),
                        _value_style(label, value),
                        _address_kind_for_key(label),
                    )
                )
            self._out.print(line, soft_wrap=True)

    def _record_line(self, record: dict[str, Any], indent: int) -> Text:
        """One record as a compact ``key value  key value`` line (list items
        inside a detail block, e.g. proxies or stake positions)."""
        line = Text(" " * indent, overflow="ignore", no_wrap=True)
        for index, (key, value) in enumerate(record.items()):
            label = str(key)
            if index:
                line.append("  ")
            line.append(f"{label} ", style=STYLE_KEY)
            line.append_text(
                self._linked_text(
                    str(value), _value_style(label, value), _address_kind_for_key(label)
                )
            )
        return line

    def hyperparameters(
        self,
        title: str,
        rows: list[tuple[str, str, Optional[str], Optional[str]]],
        json_fields: dict[str, Any],
        hint: Optional[str] = None,
        docs: Optional[str] = None,
    ) -> None:
        """Aligned hyperparameter listing: raw value, its dim human reading, and
        a one-line description (``kappa  32767  ≈ 0.5  consensus majority-stake
        threshold``). ``docs`` links each name (OSC-8) to its explainer page
        under that URL and prints as a ``see:`` footer. JSON carries the raw
        record untouched."""
        if self.json_mode:
            self._json(json_fields)
            return
        self._print_title(title)
        if not rows:
            self._out.print("  [dim]none[/dim]")
            return
        name_width = max(len(name) for name, _, _, _ in rows)
        value_width = max(len(value) for _, value, _, _ in rows)
        note_width = max((len(note) for _, _, note, _ in rows if note), default=0)
        for name, value, note, short in rows:
            line = Text("  ", overflow="ignore", no_wrap=True)
            name_style = f"{STYLE_KEY} link {docs}/{name.replace('_', '-')}" if docs else STYLE_KEY
            line.append(name.rjust(name_width), style=name_style)
            line.append("  ")
            line.append(value.rjust(value_width))
            if short:
                # The reading column is padded so descriptions align.
                line.append(f"  {(note or '').ljust(note_width)}", style=STYLE_HINT)
                line.append(f"  {short}", style=STYLE_INCIDENTAL)
            elif note:
                line.append(f"  {note}", style=STYLE_HINT)
            self._out.print(line, soft_wrap=True)
        if hint or docs:
            self._out.print()
        if hint:
            self._sub_diag("help", hint, console=self._out)
        if docs:
            self._sub_diag("see", docs, console=self._out)

    def hyperparameter(
        self,
        title: str,
        fields: dict[str, Any],
        doc: Optional[str],
        json_fields: dict[str, Any],
        *,
        help: Optional[str] = None,
        note: Optional[str] = None,
        see: Optional[str] = None,
    ) -> None:
        """One hyperparameter in full: value fields, the explanation paragraph,
        and the how-to-set diagnostics."""
        if self.json_mode:
            self._json(json_fields)
            return
        self._print_title(title)
        self._print_fields(fields)
        if doc:
            self._out.print()
            self._out.print(Padding(_prose(doc), (0, 0, 0, 2)))
        if help or note or see:
            self._out.print()
        if help:
            self._sub_diag("help", help, console=self._out)
        if note:
            self._sub_diag("note", note, console=self._out)
        if see:
            self._sub_diag("see", see, console=self._out)

    def table(
        self,
        title: str,
        columns: list[str],
        rows: list[list[Any]],
        records: Optional[list[dict]] = None,
    ) -> None:
        """A collection as a table (human) or a list of objects (json).

        ``records`` supplies the JSON shape; when omitted it is derived by zipping
        ``columns`` with each row.
        """
        if self.json_mode:
            self._json(
                records if records is not None else [dict(zip(columns, row)) for row in rows]
            )
            return
        if not rows:
            self._print_title(title)
            self._out.print("  [dim]none[/dim]")
            return
        table = Table(title=title, title_style=STYLE_TITLE, header_style=STYLE_KEY)
        for column in columns:
            table.add_column(column, style=_value_style(column) or None)
        for row in rows:
            table.add_row(
                *(
                    self.subnet_text(cell)
                    if columns[i].endswith("netuid") and str(cell).isdigit()
                    else (
                        _linkify_urls(Text(str(cell))) if _URL_RE.search(str(cell)) else str(cell)
                    )
                    for i, cell in enumerate(row)
                )
            )
        self._out.print(table)

    def columns(
        self,
        title: str,
        columns: list[str],
        rows: list[list[Any]],
        records: Optional[list[dict]] = None,
        *,
        right_align: Optional[set[int]] = None,
        footer: Optional[str] = None,
    ) -> None:
        """Borderless aligned columns (gh-style list) — same JSON contract as
        ``table``. ``right_align`` holds column indices (amounts and counts);
        ``footer`` is a human-only summary line (rich markup allowed).
        Lines never wrap; put the most clippable column last.
        """
        if self.json_mode:
            self._json(
                records if records is not None else [dict(zip(columns, row)) for row in rows]
            )
            return
        self._print_title(title)
        if not rows:
            self._out.print("  [dim]none[/dim]")
            return
        right_align = right_align or set()
        cells = [[str(cell) for cell in row] for row in rows]
        widths = [
            max(len(columns[i]), max(len(row[i]) for row in cells)) for i in range(len(columns))
        ]
        self._out.print()
        header = Text("  ", overflow="ignore", no_wrap=True)
        for i, name in enumerate(columns):
            header.append(
                name.rjust(widths[i]) if i in right_align else name.ljust(widths[i]),
                style=STYLE_KEY,
            )
            header.append("  ")
        self._out.print(header, soft_wrap=True)
        for row in cells:
            line = Text("  ", overflow="ignore", no_wrap=True)
            for i, cell in enumerate(row):
                padded = cell.rjust(widths[i]) if i in right_align else cell.ljust(widths[i])
                line.append_text(
                    self._linked_text(
                        padded, _value_style(columns[i], cell), _address_kind_for_key(columns[i])
                    )
                )
                line.append("  ")
            self._out.print(line, soft_wrap=True)
        if footer:
            self._out.print()
            self._out.print(footer)

    def extrinsic_url(self, extrinsic_id: str) -> Optional[str]:
        """Explorer page for an extrinsic, or None on unindexed networks."""
        if not self.network:
            return None
        return explorer_extrinsic_url(self.network, extrinsic_id)

    def transfer_history(self, title: str, records: list[dict[str, Any]]) -> None:
        """Recent transfers for a coldkey, newest first: dim timestamp, the
        block hyperlinked to its extrinsic's explorer page, a signed amount
        (incoming green), and the counterparty carrying its local name when
        one is known. Failed transfers keep their row but are flagged and
        left out of the totals. JSON emits the raw records.

        Each record carries ``block_number``, ``extrinsic_idx``, ``timestamp``,
        ``amount_rao``, ``netuid`` (None for TAO), ``from``, ``to``,
        ``direction`` ("in" / "out" / "self"), and ``success``.
        """
        if self.json_mode:
            self._json(records)
            return
        self._print_title(title)
        if not records:
            self._out.print("  [dim]none[/dim]")
            return

        def _when(ts: str) -> str:
            try:
                return datetime.fromisoformat(ts.replace("Z", "+00:00")).strftime("%Y-%m-%d %H:%M")
            except ValueError:
                return ts

        rows = [
            (
                record,
                f"{'−' if record['direction'] == 'out' else '+'}"
                f"{self.balance(record['amount_rao'], record.get('netuid') or 0)}",
            )
            for record in records
        ]
        when_width = max(len("when"), max(len(_when(r["timestamp"])) for r, _ in rows))
        block_width = max(len("block"), max(len(str(r["block_number"])) for r, _ in rows))
        amount_width = max(len("amount"), max(len(a) for _, a in rows))

        self._out.print()
        header = Text("  ", overflow="ignore", no_wrap=True)
        header.append("when".ljust(when_width), style=STYLE_KEY)
        header.append("  ")
        header.append("block".rjust(block_width), style=STYLE_KEY)
        header.append("  ")
        header.append("amount".rjust(amount_width), style=STYLE_KEY)
        header.append("  ")
        header.append("counterparty", style=STYLE_KEY)
        self._out.print(header, soft_wrap=True)

        for record, amount_text in rows:
            line = Text("  ", overflow="ignore", no_wrap=True)
            line.append(_when(record["timestamp"]).ljust(when_width), style="dim")
            line.append("  ")
            url = self.extrinsic_url(f"{record['block_number']}-{record['extrinsic_idx']:04}")
            line.append(
                str(record["block_number"]).rjust(block_width),
                style=f"dim link {url}" if url else "dim",
            )
            line.append("  ")
            if not record["success"]:
                amount_style = "dim"
            elif record["direction"] == "in":
                amount_style = STYLE_SUCCESS
            else:
                amount_style = ""
            line.append(amount_text.rjust(amount_width), style=amount_style)
            line.append("  ")
            if record["direction"] == "self":
                line.append("self".ljust(4), style=STYLE_KEY)
            else:
                word = "to" if record["direction"] == "out" else "from"
                line.append(word.rjust(4), style=STYLE_KEY)
            line.append(" ")
            counterparty = record["to"] if record["direction"] == "out" else record["from"]
            account_url = self.account_url(counterparty, "coldkey")
            name = self.address_names.get(counterparty)
            if name:
                line.append(name, style=STYLE_NAME)
                line.append(" (", style=STYLE_KEY)
                line.append(
                    counterparty,
                    style=f"dim link {account_url}" if account_url else "dim",
                )
                line.append(")", style=STYLE_KEY)
            else:
                line.append(counterparty, style=f"link {account_url}" if account_url else "")
            if not record["success"]:
                line.append(f"  {GLYPH_FAIL} failed", style=PASTEL_RED)
            self._out.print(line, soft_wrap=True)

        tao_in = sum(
            r["amount_rao"]
            for r in records
            if r["success"] and not r.get("netuid") and r["direction"] == "in"
        )
        tao_out = sum(
            r["amount_rao"]
            for r in records
            if r["success"] and not r.get("netuid") and r["direction"] == "out"
        )
        failed = sum(1 for r in records if not r["success"])
        suffix = "transfer" if len(records) == 1 else "transfers"
        parts = [f"{len(records)} {suffix}"]
        if tao_in:
            parts.append(f"in {Balance(tao_in)}")
        if tao_out:
            parts.append(f"out {Balance(tao_out)}")
        if failed:
            parts.append(f"{failed} failed excluded")
        self._out.print()
        self._out.print("[dim]" + "  ·  ".join(parts) + "[/dim]")

    def subnet_list(
        self,
        title: str,
        rows: list[dict[str, Any]],
        records: list[dict],
        *,
        footer: Optional[str] = None,
    ) -> None:
        """Subnet listing: the canonical netuid rendering (name + token symbol,
        hyperlinked) beside right-aligned numeric columns — same JSON contract
        as ``table``. ``footer`` is a human-only summary line."""
        if self.json_mode:
            self._json(records)
            return
        self._out.print(f"[{STYLE_TITLE}]{escape(title)}[/{STYLE_TITLE}]")
        if not rows:
            self._out.print("  [dim]none[/dim]")
            return
        columns = ["price (τ)", "tempo", "burn", "neurons"]
        keys = ["price", "tempo", "burn", "neurons"]
        subnet_cells = [self.subnet_text(row["netuid"]) for row in rows]
        subnet_width = max([len("netuid")] + [cell.cell_len for cell in subnet_cells])
        widths = [
            max(len(columns[i]), max(len(str(row[keys[i]])) for row in rows))
            for i in range(len(columns))
        ]
        self._out.print()
        header = Text("  ", overflow="ignore", no_wrap=True)
        header.append("netuid".ljust(subnet_width), style=STYLE_KEY)
        header.append("  ")
        for i, name in enumerate(columns):
            header.append(name.rjust(widths[i]), style=STYLE_KEY)
            header.append("  ")
        self._out.print(header, soft_wrap=True)
        for row, cell in zip(rows, subnet_cells):
            line = Text("  ", overflow="ignore", no_wrap=True)
            line.append_text(cell)
            line.append(" " * (subnet_width - cell.cell_len))
            line.append("  ")
            for i, key in enumerate(keys):
                value = str(row[key])
                line.append(value.rjust(widths[i]), style="dim" if value == "—" else "")
                line.append("  ")
            self._out.print(line, soft_wrap=True)
        if footer:
            self._out.print()
            self._out.print(footer)

    def stake_list(
        self,
        title: str,
        groups: list[dict[str, Any]],
        records: list[dict],
        total: Any,
    ) -> None:
        """Grouped stake view: one block per netuid with the subnet total on
        top and the per-hotkey breakdown dimmed beneath it.

        ``records`` supplies the JSON shape (flat per-position records).
        """
        if self.json_mode:
            self._json(records)
            return
        # Title carries a parenthetical note ("stake (per-subnet currency: …)");
        # the note becomes the tree root, playing the same context role the
        # file path plays in the wallet/address trees.
        head, _, note = title.partition(" (")
        self._out.print(f"[{STYLE_TITLE}]{escape(head)}[/{STYLE_TITLE}]")
        if not groups:
            self._out.print("  [dim]none[/dim]")
            return
        width = max(
            [len(str(g["stake"])) for g in groups]
            + [len(str(p["stake"])) for g in groups for p in g.get("positions", [])]
        )
        self._out.print()
        root_label = (
            f"[dim italic]{escape(note.rstrip(')'))}[/dim italic]" if note else "[dim]stake[/dim]"
        )
        root = Tree(root_label, guide_style="bright_black")
        for group in groups:
            label = Text(overflow="ignore", no_wrap=True)
            if group.get("wallet"):
                label.append(str(group["wallet"]), style=STYLE_NAME)
                label.append("  ")
            label.append("netuid ", style=STYLE_KEY)
            label.append_text(self.subnet_text(group["netuid"]))
            if group.get("note"):
                label.append(f"  {group['note']}", style="dim italic")
            label.append("\n")
            label.append(str(group["stake"]).rjust(width))
            label.append("  ")
            label.append(str(group["value"]))
            if group.get("availability_note"):
                # Own line so long locked/free figures are not clipped off the
                # stake/value row on narrow terminals.
                label.append("\n")
                label.append(str(group["availability_note"]), style="dim italic")
            branch = root.add(label)
            for position in group.get("positions", []):
                leaf = Text(overflow="ignore", no_wrap=True)
                leaf.append(str(position["stake"]).rjust(width), style="dim")
                leaf.append("  ")
                if position.get("named"):
                    label_style = STYLE_NAME
                elif position.get("identity"):
                    # On-chain identity name: informative but unverified, so it
                    # never gets the local-name accent.
                    label_style = "dim italic"
                else:
                    label_style = "dim"
                hotkey = position.get("hotkey")
                url = self.account_url(hotkey, "hotkey") if hotkey else None
                if url:
                    label_style = f"{label_style} link {url}"
                leaf.append(str(position["label"]), style=label_style)
                if position.get("take") is not None:
                    leaf.append(f"  take {position['take']:.1%}", style="dim")
                if position.get("note"):
                    leaf.append(f"  {position['note']}", style="dim italic")
                branch.add(leaf)
        self._out.print(root)
        self._out.print()
        self._out.print(f"[dim]total[/dim] {total}  [dim](spot, excl. slippage/fees)[/dim]")

    def metagraph(
        self,
        title: str,
        sections: list[tuple[Optional[str], list[tuple[str, str, Optional[str]]]]],
        tree_label: str,
        neurons: list[dict[str, Any]],
        records: Any,
        summary: str,
        hint: Optional[str] = None,
    ) -> None:
        """Metagraph view: aligned key/value sections for the subnet-level data
        (raw value first, dim human reading beside it — the hyperparameters
        convention), then one tree branch per neuron in the stake-list style:
        an identity line on top and the dimmed per-uid numbers beneath it.

        JSON emits the raw metagraph record untouched.
        """
        if self.json_mode:
            self._json(records)
            return
        self._print_title(title)
        for header, rows in sections:
            if not rows:
                continue
            self._out.print()
            if header:
                self._out.print(Text(header, style=STYLE_KEY))
            key_width = max(len(key) for key, _, _ in rows)
            for key, value, note in rows:
                line = Text("  ", overflow="ignore", no_wrap=True)
                line.append(key.rjust(key_width), style=STYLE_KEY)
                line.append("  ")
                line.append_text(
                    self._linked_text(value, _value_style(key, value), _address_kind_for_key(key))
                )
                if note:
                    line.append(f"  {note}", style=STYLE_HINT)
                self._out.print(line, soft_wrap=True)
        self._out.print()
        if not neurons:
            self._out.print("[dim]no neurons[/dim]")
        else:
            stake_width = max(len(str(n["stake"])) for n in neurons)
            uid_width = max(len(f"uid {n['uid']}") for n in neurons)
            root = Tree(f"[dim]{escape(tree_label)}[/dim]", guide_style="bright_black")
            for neuron in neurons:
                label = Text(overflow="ignore", no_wrap=True)
                label.append(f"uid {neuron['uid']}".ljust(uid_width), style=STYLE_KEY)
                label.append("  ")
                if neuron.get("named"):
                    name_style = STYLE_NAME
                elif neuron.get("identity"):
                    # On-chain identity name: informative but unverified.
                    name_style = "dim italic"
                else:
                    name_style = ""
                url = self.account_url(neuron["hotkey"], "hotkey")
                if url:
                    name_style = f"{name_style} link {url}".strip()
                label.append(str(neuron["label"]), style=name_style)
                if neuron.get("validator"):
                    label.append("  ✓ validator", style="dim")
                if not neuron.get("active", True):
                    label.append("  inactive", style="dim italic")
                label.append("\n")
                label.append(str(neuron["stake"]).rjust(stake_width))
                stats = [f"emission {neuron['emission']}"]
                stats.append(f"incentive {neuron['incentive']:.3f}")
                stats.append(f"dividends {neuron['dividends']:.3f}")
                if neuron.get("updated") is not None:
                    stats.append(f"updated {neuron['updated']}b ago")
                if neuron.get("axon"):
                    stats.append(str(neuron["axon"]))
                label.append("  " + " · ".join(stats), style="dim")
                root.add(label)
            self._out.print(root)
        self._out.print()
        self._out.print(f"[dim]{escape(summary)}[/dim]")
        if hint:
            self._sub_diag("help", hint, console=self._out)

    def conviction_list(
        self,
        title: str,
        total: dict[str, Any],
        positions: list[dict[str, Any]],
        parameters: list[tuple[str, str, Optional[str]]],
        explanation: list[str],
        json_payload: dict[str, Any],
    ) -> None:
        """Subnet conviction view: total locked at the root, one branch per
        locking hotkey with its conviction nested beneath it, then the
        takeover parameters and a how-it-works primer.

        ``total`` carries ``locked``/``conviction``/``summary`` strings;
        each position carries ``hotkey``, ``label``, ``named``/``identity``,
        ``locked``, ``conviction``, and optional ``note``/``detail`` strings.
        ``json_payload`` is the complete machine shape.
        """
        if self.json_mode:
            self._json(json_payload)
            return
        self._print_title(title)
        self._out.print()

        root_label = Text(overflow="ignore", no_wrap=True)
        root_label.append("total locked  ", style=STYLE_KEY)
        root_label.append(str(total["locked"]))
        root = Tree(root_label, guide_style="bright_black")
        for position in positions:
            if position.get("named"):
                label_style = STYLE_NAME
            elif position.get("identity"):
                label_style = "dim italic"
            else:
                label_style = ""
            hotkey = position.get("hotkey")
            url = self.account_url(hotkey, "hotkey") if hotkey else None
            if url:
                label_style = f"{label_style} link {url}".strip()
            label = Text(overflow="ignore", no_wrap=True)
            label.append(str(position["label"]), style=label_style)
            if position.get("note"):
                label.append(f"  {position['note']}", style="dim italic")
            label.append("\n")
            label.append("locked  ", style=STYLE_KEY)
            label.append(str(position["locked"]))
            branch = root.add(label)
            leaf = Text(overflow="ignore", no_wrap=True)
            leaf.append("conviction  ", style=STYLE_KEY)
            leaf.append(str(position["conviction"]))
            if position.get("detail"):
                leaf.append(f"  {position['detail']}", style=STYLE_HINT)
            branch.add(leaf)
        self._out.print(root)

        self._out.print()
        summary = Text(overflow="ignore", no_wrap=True)
        summary.append("total conviction ", style=STYLE_KEY)
        summary.append(str(total["conviction"]))
        if total.get("summary"):
            summary.append(f"  {total['summary']}", style=STYLE_HINT)
        self._out.print(summary, soft_wrap=True)

        if parameters:
            self._out.print()
            self._out.print(Text("takeover parameters", style=STYLE_TITLE))
            name_width = max(len(name) for name, _, _ in parameters)
            for name, value, note in parameters:
                line = Text("  ", overflow="ignore", no_wrap=True)
                line.append(name.rjust(name_width), style=STYLE_KEY)
                line.append("  ")
                line.append_text(self._linked_text(str(value), ""))
                if note:
                    line.append(f"  {note}", style=STYLE_HINT)
                self._out.print(line, soft_wrap=True)

        if explanation:
            self._out.print()
            self._out.print(Text("how conviction works", style=STYLE_TITLE))
            for paragraph in explanation:
                self._out.print()
                self._out.print(Padding(_prose(paragraph), (0, 0, 0, 2)))

    def tree(
        self,
        title: str,
        nodes: list[tuple[str, list[str]]],
        records: Optional[list[dict]] = None,
    ) -> None:
        """Render a two-level grouping as a tree (human) or list of objects (json).

        ``nodes`` is a list of ``(branch_label, [leaf_label, ...])`` pairs; rich
        markup in the labels is honoured. ``records`` supplies the JSON shape.
        """
        if self.json_mode:
            self._json(records if records is not None else [])
            return
        root = Tree(f"[{STYLE_TITLE}]{escape(title)}[/{STYLE_TITLE}]")
        for branch_label, leaves in nodes:
            branch = root.add(branch_label)
            for leaf in leaves:
                branch.add(leaf)
        self._out.print(root)

    def _wallet_node(
        self,
        name: str,
        ss58: str | None,
        *,
        name_style: str,
        count: int | None = None,
        crypto_type: str | None = None,
        kind: str | None = None,
    ) -> Text:
        node = Text()
        node.append(name, style=name_style)
        if crypto_type:
            node.append(f"  ({crypto_type})", style=STYLE_CRYPTO)
        if count:
            suffix = "hotkey" if count == 1 else "hotkeys"
            node.append(f"  ({count} {suffix})", style="dim")
        if ss58 and ss58 != name:
            node.append("\n")
            url = self.account_url(ss58, kind) if kind else None
            node.append(ss58, style=f"dim link {url}" if url else "dim")
        elif ss58 is None:
            node.append("\n")
            node.append("—", style=f"dim {PASTEL_RED}")
        return node

    @staticmethod
    def _named_label_text(text: str, *, name_style: str = STYLE_NAME) -> Text:
        """Render ``name (ss58)`` with the name emphasized and dim parens."""
        if " (" in text:
            name, rest = text.split(" (", 1)
            address = rest[:-1] if rest.endswith(")") else rest
            out = Text()
            out.append(name, style=name_style)
            out.append(" (", style=STYLE_KEY)
            out.append(address, style=STYLE_ADDRESS)
            out.append(")", style=STYLE_KEY)
            return out
        return Text(text, style=name_style)

    def _multisig_node(self, name: str, ss58: str | None, *, threshold: int, count: int) -> Text:
        node = Text()
        node.append(name, style=STYLE_MULTISIG)
        node.append(f"  (multisig · {threshold}-of-{count})", style="dim")
        node.append("\n")
        if ss58:
            url = self.account_url(ss58, "coldkey")
            node.append(ss58, style=f"dim link {url}" if url else "dim")
        else:
            node.append("—", style=f"dim {PASTEL_RED}")
        return node

    def wallet_list(
        self,
        path: str,
        records: list[dict[str, Any]],
        *,
        multisigs: Optional[list[dict[str, Any]]] = None,
        addresses: Optional[list[dict[str, Any]]] = None,
        proxies: Optional[list[dict[str, Any]]] = None,
    ) -> None:
        """Render coldkeys, hotkeys, saved multisigs, the address book, and the proxy book."""
        multisigs = multisigs or []
        addresses = addresses or []
        proxies = proxies or []
        if self.json_mode:
            self._json(
                {
                    "path": path,
                    "coldkeys": records,
                    "multisigs": multisigs,
                    "addresses": addresses,
                    "proxies": proxies,
                }
            )
            return

        display_path = str(Path(path).expanduser())
        home = str(Path.home())
        if display_path.startswith(home):
            display_path = "~" + display_path[len(home) :]

        self._out.print("[dim]Wallets[/dim]")
        self._out.print()

        root = Tree(f"[dim italic]{display_path}[/dim italic]", guide_style="bright_black")
        for ck in records:
            hotkeys = ck.get("hotkeys", [])
            branch = root.add(
                self._wallet_node(
                    ck["coldkey"],
                    ck.get("ss58"),
                    name_style=STYLE_NAME,
                    count=len(hotkeys) or None,
                    crypto_type=ck.get("crypto_type"),
                    kind="coldkey",
                )
            )
            if not hotkeys:
                branch.add(Text("no hotkeys", style="dim italic"))
                continue
            for hk in hotkeys:
                branch.add(
                    self._wallet_node(
                        hk["name"],
                        hk.get("ss58"),
                        name_style=STYLE_NAME,
                        crypto_type=hk.get("crypto_type"),
                        kind="hotkey",
                    )
                )

        self._out.print(root)

        total_coldkeys = len(records)
        total_hotkeys = sum(len(ck.get("hotkeys", [])) for ck in records)
        summary = f"[dim]{total_coldkeys} coldkeys  ·  {total_hotkeys} hotkeys"
        if multisigs:
            multisig_path = str(cfg.multisigs_path().expanduser())
            if multisig_path.startswith(home):
                multisig_path = "~" + multisig_path[len(home) :]
            self._out.print()
            self._out.print("[dim]Multisig wallets[/dim]")
            self._out.print()
            multi_root = Tree(
                f"[dim italic]{multisig_path}[/dim italic]", guide_style="bright_black"
            )
            for entry in multisigs:
                branch = multi_root.add(
                    self._multisig_node(
                        entry["name"],
                        entry.get("ss58"),
                        threshold=int(entry["threshold"]),
                        count=int(entry["signatory_count"]),
                    )
                )
                for signer in entry.get("signatories", []):
                    branch.add(
                        self._wallet_node(
                            signer["name"],
                            signer.get("ss58"),
                            name_style=STYLE_NAME,
                            kind="coldkey",
                        )
                    )
                note = entry.get("note")
                if note:
                    branch.add(Text(note, style="dim italic"))
            self._out.print(multi_root)
            summary += f"  ·  {len(multisigs)} multisigs"
        if addresses:
            addresses_path = str(cfg.addresses_path().expanduser())
            if addresses_path.startswith(home):
                addresses_path = "~" + addresses_path[len(home) :]
            self._out.print()
            self._out.print("[dim]Addresses[/dim]")
            self._out.print()
            addr_root = Tree(
                f"[dim italic]{addresses_path}[/dim italic]", guide_style="bright_black"
            )
            for entry in addresses:
                branch = addr_root.add(
                    self._wallet_node(
                        entry.get("name", ""),
                        entry.get("address"),
                        name_style=STYLE_NAME,
                        kind="coldkey",
                    )
                )
                note = entry.get("note")
                if note:
                    branch.add(Text(note, style="dim italic"))
            self._out.print(addr_root)
            summary += f"  ·  {len(addresses)} addresses"
        if proxies:
            proxies_path = str(cfg.proxies_path().expanduser())
            if proxies_path.startswith(home):
                proxies_path = "~" + proxies_path[len(home) :]
            # Wallet names for spawner labels ("pure, spawned by dev-wallet").
            coldkey_names = {ck.get("ss58"): ck["coldkey"] for ck in records if ck.get("ss58")}
            self._out.print()
            self._out.print("[dim]Proxies[/dim]")
            self._out.print()
            proxy_root = Tree(
                f"[dim italic]{proxies_path}[/dim italic]", guide_style="bright_black"
            )
            for entry in proxies:
                branch = proxy_root.add(
                    self._wallet_node(
                        entry.get("name", ""),
                        entry.get("address"),
                        name_style=STYLE_NAME,
                        kind="coldkey",
                    )
                )
                meta = [str(entry.get("proxy_type") or "Staking")]
                if entry.get("delay"):
                    meta.append(f"delay {entry['delay']}")
                spawner = entry.get("spawner")
                if spawner:
                    label = self.address_names.get(spawner) or coldkey_names.get(spawner)
                    meta.append(f"pure, spawned by {label or spawner}")
                branch.add(Text(" · ".join(meta), style="dim"))
                note = entry.get("note")
                if note:
                    branch.add(Text(note, style="dim italic"))
            self._out.print(proxy_root)
            suffix = "proxy" if len(proxies) == 1 else "proxies"
            summary += f"  ·  {len(proxies)} {suffix}"
        self._out.print()
        self._out.print(summary + "[/dim]")

    def address_list(self, path: Path, entries: list[dict[str, Any]]) -> None:
        """Render saved ss58 address-book contacts."""
        if self.json_mode:
            self._json({"path": str(path), "addresses": entries})
            return

        display_path = str(path.expanduser())
        home = str(Path.home())
        if display_path.startswith(home):
            display_path = "~" + display_path[len(home) :]

        self._out.print("[dim]Addresses[/dim]")
        self._out.print()

        root = Tree(f"[dim italic]{display_path}[/dim italic]", guide_style="bright_black")
        for entry in entries:
            name = entry.get("name", "")
            ss58 = entry.get("address") or "—"
            note = entry.get("note") or "—"
            branch = root.add(Text(name, style=STYLE_NAME))
            branch.add(Text(ss58, style="dim", overflow="ignore", no_wrap=True))
            note_line = Text()
            note_line.append("note", style="dim")
            note_line.append(" · ", style="dim")
            note_line.append(note, style="dim italic" if note != "—" else "dim")
            branch.add(note_line)

        self._out.print(root)
        count = len(entries)
        suffix = "entry" if count == 1 else "entries"
        self._out.print()
        self._out.print(f"[dim]{count} {suffix}[/dim]")

    def _print_copyable(self, text: str, *, prefix: str = "    ") -> None:
        """Print a single line without terminal wrapping so it can be copied as-is."""
        line = Text(prefix, overflow="ignore", no_wrap=True)
        line.append(text, style=STYLE_COMMAND)
        self._out.print(line, soft_wrap=True)

    def plan(self, plan: Plan) -> None:
        """Render a dry-run plan (fee, effects, warnings, policy)."""
        if self.json_mode:
            self._json(plan.to_dict())
            return
        summary = Text()
        summary.append("dry run:", style="dim")
        summary.append(" ")
        summary.append_text(self.linked_prose(plan.summary))
        self._out.print(summary)
        signer_label = self.address_names.get(plan.signer_address or "", plan.signer)
        line = Text("  ")
        line.append("signer", style=STYLE_KEY)
        line.append("  ")
        line.append(str(signer_label))
        if plan.signer_address:
            line.append(" (")
            line.append_text(self._linked_text(plan.signer_address, STYLE_ADDRESS))
            line.append(")")
        self._out.print(line)
        if plan.fee is not None:
            self._out.print(f"  [dim]est. fee[/dim]  {plan.fee}")
        for effect in plan.effects:
            line = Text("  ")
            line.append("effect", style="dim")
            line.append("  ")
            line.append_text(self.linked_prose(effect))
            self._out.print(line)
        for warning in plan.warnings:
            self._out.print(
                f"  [{STYLE_WARNING}]warning:[/{STYLE_WARNING}] "
                f"{escape(self.with_subnets(self.with_names(warning)))}"
            )
        for violation in plan.violations:
            self._out.print(
                f"  [{STYLE_ERROR}]policy:[/{STYLE_ERROR}] {escape(self.with_subnets(violation))}"
            )
        if not plan.ok:
            self._out.print(f"  [{STYLE_ERROR}]blocked by policy[/{STYLE_ERROR}]")
        # The docs page carries parameters, verify reads, and the on-chain
        # implementation with source links.
        self._sub_diag("see", tx_docs_url(plan.op), console=self._out)

    def multisig_followup(self, followup: dict[str, Any], *, suppress_decode: bool = False) -> None:
        """Render co-signer instructions after a multisig approval.

        ``suppress_decode`` skips the per-record decode note/hint so a listing
        can print it once for all records instead of after each one.
        """
        if self.quiet:
            return
        if self.json_mode:
            self._json({"multisig_followup": followup})
            return

        status = followup.get("status")
        if status == "executed":
            self._out.print(
                f"[{STYLE_SUCCESS}]{GLYPH_OK} multisig executed[/{STYLE_SUCCESS}] "
                "— no further approvals needed."
            )
            return
        if status == "submitted":
            self._out.print(
                f"multisig approval [{STYLE_WARNING}]submitted[/{STYLE_WARNING}] "
                "— co-signer details not ready yet."
            )
            hint = followup.get("decode_hint")
            if hint:
                self._sub_diag("help", str(hint), console=self._out)
            return

        threshold = followup.get("threshold")
        approvals = followup.get("approvals")
        commands = followup.get("co_signer_commands") or []
        # Only promise co-signer commands when there are commands to share.
        suffix = " — share with co-signers" if commands else ""
        self._out.print()
        # Stripe colors the status token itself (ColorizeStatus), nothing else:
        # the state word is yellow, the surrounding phrase stays plain.
        self._out.print(
            f"multisig [{STYLE_WARNING}]pending[/{STYLE_WARNING}] "
            f"({approvals}/{threshold} approvals{suffix})"
        )
        fields: dict[str, Any] = {
            "call_hash": followup.get("call_hash"),
            "call_data": followup.get("call_data"),
            "timepoint": followup.get("timepoint_display"),
            "multisig": ms_helpers.format_multisig_display(
                followup.get("multisig_address"),
                followup.get("multisig_preset"),
            ),
        }
        if followup.get("target"):
            target = followup.get("target")
            if followup.get("sudo"):
                target += " via Sudo.sudo"
            if followup.get("proxy_for"):
                target += f" as {followup['proxy_for']} via proxy"
            fields["target"] = target
        approval_labels = followup.get("approval_labels") or followup.get("approvals_so_far") or []
        if approval_labels:
            fields["approved_by"] = approval_labels
        remaining_labels = followup.get("remaining_labels") or []
        if remaining_labels:
            fields["needs"] = remaining_labels
        width = max((len(k) for k in fields), default=0)
        for key, value in fields.items():
            if value is None:
                continue
            if key == "multisig":
                self._kv_line(
                    key, width, self._named_label_text(str(value), name_style=STYLE_MULTISIG)
                )
                continue
            if key in ("approved_by", "needs"):
                # One signer per line: a comma-joined list wraps mid-address.
                for index, item in enumerate(value):
                    self._kv_line(
                        key if index == 0 else "", width, self._named_label_text(str(item))
                    )
                continue
            self._kv_line(key, width, Text(str(value)))

        if not commands:
            if not suppress_decode:
                self._print_decode_diag(followup)
            return
        self._out.print()
        self._out.print("[dim]Co-signer commands (one line each — copy and run):[/dim]")
        for entry in commands:
            label = entry.get("label") or entry.get("ss58")
            self._out.print("\n  ", end="")
            self._out.print(self._named_label_text(str(label), name_style=STYLE_NAME), end="")
            self._out.print(":")
            self._print_copyable(str(entry.get("command", "")))
        self._out.print()
        tail = _prose("add `--macos-password` or `--keychain-password` if the co-signer uses them")
        tail.style = "dim"
        self._out.print(tail)

    def _print_decode_diag(self, followup: dict[str, Any]) -> None:
        """Note/hint block explaining why co-signer commands are unavailable."""
        note = followup.get("decode_note")
        hint = followup.get("decode_hint")
        if not note and not hint:
            return
        self._out.print()
        if note:
            self._sub_diag("note", str(note), console=self._out)
        if hint:
            self._sub_diag("help", str(hint), console=self._out)

    def pending_multisigs(self, records: list[dict[str, Any]], *, title: str) -> None:
        """Render one or more pending multisig operations."""
        if self.quiet:
            return
        if self.json_mode:
            self._json({"pending_multisigs": records, "count": len(records)})
            return
        if not records:
            self._out.print("[dim]No pending multisig operations.[/dim]")
            return
        count = len(records)
        suffix = "op" if count == 1 else "ops"
        self._out.print(f"[dim]{title}  {count} {suffix}[/dim]")
        for index, followup in enumerate(records, start=1):
            if index > 1:
                self._out.print()
                self._out.print("[dim]" + ("─" * 60) + "[/dim]")
            self.multisig_followup(followup, suppress_decode=True)
        # The decode note usually applies to every record; print each distinct
        # note/hint pair once instead of repeating it per record.
        seen = dict.fromkeys(
            (record.get("decode_note"), record.get("decode_hint"))
            for record in records
            if not (record.get("co_signer_commands"))
        )
        for note, hint in seen:
            self._print_decode_diag({"decode_note": note, "decode_hint": hint})

    def result(self, result: ExtrinsicResult, success_message: str) -> bool:
        """Render the outcome of a submitted extrinsic. Returns ``result.success``.

        JSON mode emits the canonical ``ExtrinsicResult.to_dict()`` (including
        ``data`` and the structured, coded ``error``) so machine consumers get the
        full shape rather than a re-implemented subset.
        """
        followup = result.data.get("multisig_followup")
        if self.json_mode:
            self._json(result.to_dict())
            return result.success
        if result.success:
            line = Text()
            line.append(f"{GLYPH_OK} ", style=STYLE_SUCCESS)
            line.append_text(self.linked_prose(success_message, STYLE_SUCCESS))
            self._out.print(line)
            fields: dict[str, Any] = {}
            if result.fee is not None:
                fields["fee"] = result.fee
            if result.block_hash:
                fields["block"] = result.block_hash
            if result.extrinsic_id:
                fields["extrinsic"] = result.extrinsic_id
            if result.explorer_url:
                fields["explorer"] = result.explorer_url
            fields.update(
                (key, value) for key, value in result.data.items() if key != "multisig_followup"
            )
            if fields:
                self._print_fields(fields)
            if followup:
                self.multisig_followup(followup)
        else:
            self._print_failure(result)
        return result.success

    def registration_result(self, result: ExtrinsicResult) -> bool:
        """Render the two-phase subnet-registration result compactly."""
        if self.json_mode or not result.success or "netuid" not in result.data:
            return self.result(result, "register a new subnet")

        netuid = int(result.data["netuid"])
        line = Text()
        line.append(f"{GLYPH_OK} ", style=STYLE_SUCCESS)
        line.append_text(self.linked_prose(f"subnet {netuid} registered", STYLE_SUCCESS))
        self._out.print(line)

        mode = result.data.get("registration_mode")
        if mode == "after_deregistration":
            prior = result.data.get("deregistered_netuid") or result.data.get("cleanup_netuid")
            registration = "queued · registered after deregistration"
            if prior is not None:
                registration += f" of subnet {prior}"
        else:
            registration = "immediate · no deregistration needed"

        fields: dict[str, Any] = {
            "netuid": netuid,
            "registration": registration,
        }
        if result.data.get("registration_price_rao") is not None:
            fields["price"] = self.balance(int(result.data["registration_price_rao"]))
        if result.data.get("queued_at_block") is not None:
            fields["queued block"] = result.data["queued_at_block"]
        if result.data.get("registered_at_block") is not None:
            fields["registered block"] = result.data["registered_at_block"]
        if result.fee is not None:
            fields["fee"] = result.fee
        if result.extrinsic_id:
            fields["registration tx"] = result.extrinsic_id
        if result.explorer_url:
            fields["explorer"] = result.explorer_url
        self._print_fields(fields)
        return True

    def chain_error(
        self,
        error: ChainError,
        *,
        explorer_url: Optional[str] = None,
    ) -> None:
        """Render a :class:`ChainError` with the same rustc diagnostic anatomy
        used for failed ``ExtrinsicResult``s (code bracket, name, description,
        help, docs, explain tip)."""
        message = error.message
        if self.json_mode:
            payload = error.to_dict()
            if explorer_url:
                payload["explorer_url"] = explorer_url
            self._err.print_json(_json.dumps(payload))
            return
        header = Text()
        header.append("error", style=STYLE_ERROR)
        header.append(f"[{error.code.value}]", style=STYLE_ERROR)
        header.append(":", style=STYLE_ERROR)
        header.append(" ")
        body = _prose(_diagnostic(self.with_subnets(self.with_names(message))))
        body.style = STYLE_MESSAGE
        header.append_text(body)
        self._err.print(header)
        if error.name:
            self._sub_diag("note", f"the chain rejected the call with `{error.name}`")
        if error.description and error.description != message:
            self._sub_diag("note", error.description)
        self._sub_diag("help", error.remediation)
        if error.docs_url:
            self._sub_diag("see", error.docs_url)
        if explorer_url:
            self._sub_diag("see", explorer_url)
        if error.name and (error.name in NAME_TO_CODE or error.name in DISPATCH_ERRORS):
            explain_target = error.name
        elif error.code is not ErrorCode.UNKNOWN:
            explain_target = error.code.value
        else:
            explain_target = None
        if explain_target:
            tail = _prose(
                f"for more information about this error, run `btcli explain {explain_target}`"
            )
            tail.style = STYLE_HINT
            self._err.print(tail)

    def _print_failure(self, result: ExtrinsicResult) -> None:
        """Rustc diagnostic anatomy: ``error[code]:`` states what went wrong,
        ``note:`` adds the exact chain error, ``help:`` carries the fix, and the
        long-form explanation stays behind `btcli explain <code>`."""
        error = result.error
        if error is not None:
            self.chain_error(error, explorer_url=result.explorer_url)
            return
        message = result.message
        header = Text()
        header.append("error", style=STYLE_ERROR)
        header.append(":", style=STYLE_ERROR)
        header.append(" ")
        body = _prose(_diagnostic(self.with_subnets(self.with_names(message))))
        body.style = STYLE_MESSAGE
        header.append_text(body)
        self._err.print(header)
        if result.explorer_url:
            self._sub_diag("see", result.explorer_url)

    def explain(self, code: str, explanation: str, help_text: str) -> None:
        """Long-form explanation of one error code (`rustc --explain` convention)."""
        if self.json_mode:
            self._json(
                {
                    "code": code,
                    "explanation": explanation,
                    "help": help_text,
                    "see": error_docs_url(code),
                }
            )
            return
        self._out.print(Text(f"error[{code}]", style=STYLE_ERROR))
        self._out.print()
        self._out.print(_prose(explanation))
        self._out.print()
        self._sub_diag("help", help_text, console=self._out)
        self._sub_diag("see", error_docs_url(code), console=self._out)

    def error_catalog(self, title: str, records: list[dict[str, str]]) -> None:
        """The chain error catalog as a gh-style listing: one dim pallet header
        per group, an aligned name/code line per error, and the on-chain
        description dimmed beneath it. JSON emits the flat records."""
        if self.json_mode:
            self._json(records)
            return
        self._out.print(f"[{STYLE_TITLE}]{escape(title)}[/{STYLE_TITLE}]")
        if not records:
            self._out.print("  [dim]none[/dim]")
            return
        width = max(len(record["name"]) for record in records)
        pallet = None
        for record in records:
            if record["pallet"] != pallet:
                pallet = record["pallet"]
                self._out.print()
                self._out.print(Text(pallet, style=STYLE_KEY))
            line = Text("  ", overflow="ignore", no_wrap=True)
            url = record.get("docs_url")
            line.append(record["name"].ljust(width), style=f"link {url}" if url else "")
            line.append("  ")
            line.append(record["code"], style=STYLE_KEY)
            self._out.print(line, soft_wrap=True)
            description = _prose(_diagnostic(record["description"] or record["name"]))
            description.style = STYLE_HINT
            # Padding (not a leading indent) so wrapped lines keep the indent.
            self._out.print(Padding(description, (0, 0, 0, 4)))
        self._out.print()
        suffix = "error" if len(records) == 1 else "errors"
        self._out.print(f"[dim]{len(records)} {suffix}[/dim]")

    def explain_chain(self, matches: list[dict[str, str]]) -> None:
        """Explain exact chain error names: the researched description (trigger
        plus where to check) with the on-chain docs as fallback, and the
        semantic code each classifies to. One block per pallet when a name
        collides."""
        if self.json_mode:
            self._json(matches[0] if len(matches) == 1 else matches)
            return
        for index, match in enumerate(matches):
            if index:
                self._out.print()
            header = Text()
            header.append(f"error[{match['code']}]", style=STYLE_ERROR)
            header.append(": ")
            header.append(f"{match['pallet']}.{match['name']}")
            self._out.print(header)
            self._out.print()
            body = match.get("description") or match["docs"] or match["name"]
            self._out.print(_prose(_diagnostic(body)))
            self._out.print()
            self._sub_diag("help", match["help"], console=self._out)
            see = match.get("docs_url") or error_docs_url(match["code"])
            self._sub_diag("see", see, console=self._out)
        for code in dict.fromkeys(match["code"] for match in matches):
            tail = _prose(f"for more information about this code, run `btcli explain {code}`")
            tail.style = STYLE_HINT
            self._out.print(tail)
