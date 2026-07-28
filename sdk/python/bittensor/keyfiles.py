"""Bittensor wallet keyfiles — on-disk keyfile format and encryption."""

from __future__ import annotations

import getpass
import os
import sys
from pathlib import Path

from .sp_core import (
    KeyfileError,
    Keypair,
    WrongPasswordError,
    decrypt_keyfile_data,
    deserialize_keypair_from_keyfile_data,
    encrypt_keyfile_data,
    get_password_from_environment,
    keyfile_data_is_encrypted,
    save_password_to_environment,
    serialized_keypair_to_keyfile_data,
)

__all__ = [
    "Keyfile",
    "KeyfileError",
    "Keypair",
    "WrongPasswordError",
    "resolve_key_password",
]


def __getattr__(name: str):
    """Backward-compatible re-exports moved to :mod:`bittensor.wallet`."""
    if name in {"DEFAULT_WALLET_PATH", "Wallet"}:
        from . import wallet as wallet_module

        return getattr(wallet_module, name)
    raise AttributeError(f"module {__name__!r} has no attribute {name!r}")


def _prompt_password(
    prompt: str = "Enter password to unlock key: ",
    *,
    confirm: bool = False,
) -> str:
    """Prompt for a password; re-ask on empty (and mismatch) when stdin is a TTY."""
    while True:
        password = getpass.getpass(prompt)
        if not password:
            if not sys.stdin.isatty():
                raise ValueError("password cannot be empty")
            print("password cannot be empty", file=sys.stderr)
            continue
        if confirm:
            again = getpass.getpass("Retype password: ")
            if again != password:
                if not sys.stdin.isatty():
                    raise ValueError("passwords do not match")
                print("passwords do not match", file=sys.stderr)
                continue
        return password


def resolve_key_password(
    password: str | None = None,
    *,
    env_var_name: str | None = None,
    prompt: str = "Enter password to unlock key: ",
    allow_prompt: bool = True,
    confirm: bool = False,
) -> str | None:
    """Resolve a keyfile password from explicit input, env, or an interactive prompt."""
    if password:
        return password
    if env_var_name:
        env_password = get_password_from_environment(env_var_name)
        if env_password:
            return env_password
    if allow_prompt:
        return _prompt_password(prompt, confirm=confirm)
    return None


class Keyfile:
    """A single keyfile on disk (coldkey, coldkeypub, hotkey, or hotkeypub)."""

    def __init__(
        self,
        path: str | os.PathLike[str],
        name: str | None = None,
        *,
        should_save_to_env: bool = False,
    ) -> None:
        self.path = str(path)
        self.name = name or Path(self.path).name
        self.should_save_to_env = should_save_to_env

    def __repr__(self) -> str:
        return f"Keyfile(path={self.path!r}, name={self.name!r})"

    def env_var_name(self) -> str:
        normalized = self.path.replace(os.sep, "_").replace(".", "_")
        return f"BT_PW_{normalized.upper()}"

    def exists_on_device(self) -> bool:
        return Path(self.path).exists()

    def is_readable(self) -> bool:
        path = Path(self.path)
        return path.exists() and os.access(path, os.R_OK)

    def is_writable(self) -> bool:
        path = Path(self.path)
        if not path.exists():
            parent = path.parent
            return parent.exists() and os.access(parent, os.W_OK)
        return os.access(path, os.W_OK)

    def _read_data(self) -> bytes:
        path = Path(self.path)
        if not path.exists():
            raise FileNotFoundError(f"keyfile at {self.path!r} does not exist")
        if not os.access(path, os.R_OK):
            raise PermissionError(f"keyfile at {self.path!r} is not readable")
        return path.read_bytes()

    def _write_data(self, data: bytes, *, overwrite: bool) -> None:
        path = Path(self.path)
        if path.exists() and not overwrite:
            # Only prompt when a human can answer; non-interactive callers
            # get the documented refusal instead of an input() hang/EOFError.
            if not sys.stdin.isatty():
                raise FileExistsError(f"refusing to overwrite existing keyfile {self.path!r}")
            answer = input(f"File {self.path} already exists. Overwrite? (y/N) ").strip().lower()
            if answer not in {"y", "yes"}:
                raise FileExistsError(f"refusing to overwrite existing keyfile {self.path!r}")
        self.make_dirs()
        # Key material (including intentionally-plaintext hotkeys) must be
        # owner-only regardless of the caller's umask. Written to a sibling
        # temp file and atomically renamed into place so a crash mid-write
        # can never destroy an existing keyfile.
        tmp = path.with_suffix(path.suffix + ".tmp")
        try:
            fd = os.open(tmp, os.O_WRONLY | os.O_CREAT | os.O_TRUNC, 0o600)
            try:
                # The create mode only applies to new files; fchmod also
                # clamps a leftover temp file being reused.
                os.fchmod(fd, 0o600)
                os.write(fd, data)
                os.fsync(fd)
            finally:
                os.close(fd)
            os.replace(tmp, path)
        except BaseException:
            tmp.unlink(missing_ok=True)
            raise

    def make_dirs(self) -> None:
        Path(self.path).parent.mkdir(parents=True, exist_ok=True, mode=0o700)

    def is_encrypted(self) -> bool:
        if not self.exists_on_device() or not self.is_readable():
            return False
        return bool(keyfile_data_is_encrypted(self._read_data()))

    def get_keypair(self, password: str | None = None) -> Keypair:
        data = self._read_data()
        if keyfile_data_is_encrypted(data):
            resolved = resolve_key_password(
                password,
                env_var_name=self.env_var_name(),
                prompt=f"Enter password to unlock key {self.path}: ",
            )
            if resolved is None:
                raise KeyfileError("password required to decrypt keyfile")
            data = bytes(decrypt_keyfile_data(data, resolved))
        return deserialize_keypair_from_keyfile_data(data)

    def set_keypair(
        self,
        keypair: Keypair,
        encrypt: bool = True,
        overwrite: bool = False,
        password: str | None = None,
    ) -> None:
        plaintext = bytes(serialized_keypair_to_keyfile_data(keypair))
        if encrypt:
            # Confirm only when prompting interactively; an explicit/env
            # password is already chosen by the caller.
            resolved = resolve_key_password(
                password,
                prompt=f"Enter password to encrypt key {self.path}: ",
                confirm=password is None,
            )
            if resolved is None:
                raise KeyfileError("password required to encrypt keyfile")
            payload = bytes(encrypt_keyfile_data(plaintext, resolved))
        else:
            payload = plaintext
        self._write_data(payload, overwrite=overwrite)
        # Only after the keyfile is actually on disk: a failed or declined
        # write must not leave the password behind in the environment.
        if encrypt and self.should_save_to_env:
            self.save_password_to_env(resolved)

    def save_password_to_env(self, password: str | None = None) -> str:
        resolved = password or _prompt_password(
            f"Enter password to store in environment for {self.path}: "
        )
        return save_password_to_environment(self.env_var_name(), resolved)

    def remove_password_from_env(self) -> None:
        os.environ.pop(self.env_var_name(), None)
