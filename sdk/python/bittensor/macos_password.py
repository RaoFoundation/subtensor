"""macOS password prompts and Keychain storage for encrypted coldkeys."""

from __future__ import annotations

import subprocess
import sys

KEYCHAIN_SERVICE = "bittensor.coldkey"


def is_macos() -> bool:
    return sys.platform == "darwin"


def _require_macos(feature: str) -> None:
    if not is_macos():
        raise OSError(f"{feature} is only available on macOS")


def _escape_applescript(text: str) -> str:
    return text.replace("\\", "\\\\").replace('"', '\\"')


def _parse_dialog_password(stdout: str) -> str:
    """Extract the password from an osascript ``display dialog`` result."""
    prefix = "text returned:"
    if prefix in stdout:
        return stdout.split(prefix, 1)[1].strip()
    return stdout.strip()


def prompt_password(*, title: str, message: str) -> str:
    """Show a native macOS password dialog and return the entered secret."""
    _require_macos("macOS password dialog")
    script = (
        f'set dlg to display dialog "{_escape_applescript(message)}" '
        f'with title "{_escape_applescript(title)}" default answer "" '
        f'with hidden answer buttons {{"Cancel", "OK"}} default button "OK"\n'
        f"return text returned of dlg"
    )
    result = subprocess.run(
        ["osascript", "-e", script],
        capture_output=True,
        text=True,
        check=False,
    )
    if result.returncode != 0:
        detail = (result.stderr or result.stdout or "").strip()
        if "User canceled" in detail or "-128" in detail:
            raise ValueError("password entry cancelled")
        raise ValueError(f"macOS password prompt failed: {detail or 'unknown error'}")
    password = _parse_dialog_password(result.stdout)
    if not password:
        raise ValueError("empty password entered")
    return password


def keychain_account(wallet_name: str) -> str:
    return wallet_name


def keychain_load(wallet_name: str) -> str | None:
    """Load a coldkey password from the macOS Keychain; may show a system prompt."""
    _require_macos("macOS Keychain")
    result = subprocess.run(
        [
            "security",
            "find-generic-password",
            "-s",
            KEYCHAIN_SERVICE,
            "-a",
            keychain_account(wallet_name),
            "-w",
        ],
        capture_output=True,
        text=True,
        check=False,
    )
    if result.returncode != 0:
        return None
    password = result.stdout.strip()
    return password or None


def keychain_save(wallet_name: str, password: str) -> None:
    """Store a coldkey password in the macOS Keychain (replaces any existing entry)."""
    _require_macos("macOS Keychain")
    # A bare ``-w`` makes `security` prompt (enter + retype) and read both
    # lines from stdin, keeping the password out of argv where any local
    # process could read it via `ps`. A newline would desync the two prompts.
    if "\n" in password or "\r" in password:
        raise ValueError("password may not contain newline characters")
    subprocess.run(
        [
            "security",
            "delete-generic-password",
            "-s",
            KEYCHAIN_SERVICE,
            "-a",
            keychain_account(wallet_name),
        ],
        capture_output=True,
        text=True,
        check=False,
    )
    result = subprocess.run(
        [
            "security",
            "add-generic-password",
            "-s",
            KEYCHAIN_SERVICE,
            "-a",
            keychain_account(wallet_name),
            "-U",
            "-w",
        ],
        input=f"{password}\n{password}\n",
        capture_output=True,
        text=True,
        check=False,
    )
    if result.returncode != 0:
        detail = (result.stderr or result.stdout or "").strip()
        raise ValueError(f"could not save password to Keychain: {detail or 'unknown error'}")


def keychain_delete(wallet_name: str) -> bool:
    """Remove a stored coldkey password from the macOS Keychain."""
    _require_macos("macOS Keychain")
    result = subprocess.run(
        [
            "security",
            "delete-generic-password",
            "-s",
            KEYCHAIN_SERVICE,
            "-a",
            keychain_account(wallet_name),
        ],
        capture_output=True,
        text=True,
        check=False,
    )
    return result.returncode == 0
