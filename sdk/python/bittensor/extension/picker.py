"""Interactive extension account selection."""

from __future__ import annotations

import sys
from typing import Callable, Optional

from .client import BridgeError, ExtensionAccount


def pick_extension_account(
    accounts: list[ExtensionAccount],
    *,
    address: Optional[str] = None,
    source: Optional[str] = None,
    name: Optional[str] = None,
    interactive: bool = True,
    default_address: Optional[str] = None,
    on_picked: Optional[Callable[[ExtensionAccount], None]] = None,
) -> ExtensionAccount:
    """Choose one extension account, prompting when needed."""
    if address is not None:
        selected = _select_account(accounts, address=address, source=source, name=name)
        if on_picked is not None:
            on_picked(selected)
        return selected

    filtered = _filter_accounts(accounts, source=source, name=name)
    if not filtered:
        hint = _format_account_hint(accounts)
        raise BridgeError(f"no extension account matched the given filters; available: {hint}")

    if len(filtered) == 1:
        selected = filtered[0]
        if on_picked is not None:
            on_picked(selected)
        return selected

    if default_address is not None:
        saved = [account for account in filtered if account.address == default_address]
        if len(saved) == 1:
            filtered_with_default = saved + [
                account for account in filtered if account.address != default_address
            ]
        else:
            filtered_with_default = filtered
    else:
        filtered_with_default = filtered

    if not interactive or not sys.stdin.isatty():
        hint = _format_account_hint(filtered)
        raise BridgeError(
            "multiple extension accounts available; pass --signer-address or run in a terminal: "
            f"{hint}"
        )

    selected = _prompt_account(filtered_with_default, default_address=default_address)
    if on_picked is not None:
        on_picked(selected)
    return selected


def _prompt_account(
    accounts: list[ExtensionAccount],
    *,
    default_address: Optional[str],
) -> ExtensionAccount:
    from rich.console import Console
    from rich.prompt import Prompt

    console = Console(stderr=True)
    default_index = 0
    if default_address is not None:
        for index, account in enumerate(accounts):
            if account.address == default_address:
                default_index = index
                break

    console.print("\n[bold]Select an extension account[/bold]")
    for index, account in enumerate(accounts, start=1):
        marker = " (last used)" if account.address == default_address else ""
        console.print(
            f"  [cyan]{index}[/cyan]. {account.name}  {account.address}  "
            f"[dim]({account.source})[/dim]{marker}"
        )

    default_choice = str(default_index + 1)
    while True:
        raw = Prompt.ask(
            "Account number",
            default=default_choice,
            console=console,
        )
        try:
            choice = int(raw) - 1
        except ValueError:
            console.print("[red]Enter a number from the list[/red]")
            continue
        if 0 <= choice < len(accounts):
            return accounts[choice]
        console.print("[red]Enter a number from the list[/red]")


def _filter_accounts(
    accounts: list[ExtensionAccount],
    *,
    source: Optional[str],
    name: Optional[str],
) -> list[ExtensionAccount]:
    filtered = accounts
    if source is not None:
        filtered = [account for account in filtered if account.source == source]
    if name is not None:
        filtered = [account for account in filtered if account.name == name]
    return filtered


def _select_account(
    accounts: list[ExtensionAccount],
    *,
    address: Optional[str],
    source: Optional[str],
    name: Optional[str],
) -> ExtensionAccount:
    if address is not None:
        matches = [account for account in accounts if account.address == address]
        if not matches:
            raise BridgeError(f"extension account {address!r} not found")
        return matches[0]

    filtered = _filter_accounts(accounts, source=source, name=name)
    if not filtered:
        hint = _format_account_hint(accounts)
        raise BridgeError(f"no extension account matched the given filters; available: {hint}")
    if len(filtered) > 1:
        hint = _format_account_hint(filtered)
        raise BridgeError(
            f"multiple extension accounts matched; pass --signer-address to choose one: {hint}"
        )
    return filtered[0]


def _format_account_hint(accounts: list[ExtensionAccount]) -> str:
    return ", ".join(
        f"{account.name} ({account.address}, {account.source})" for account in accounts
    )
