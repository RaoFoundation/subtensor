"""Native Bittensor wallet — owns keyfile layout and creation/regeneration flows."""

from __future__ import annotations

import logging
import os
import tempfile
import threading
from contextlib import contextmanager
from pathlib import Path
from typing import Any, Callable

try:
    import fcntl
except ImportError:  # pragma: no cover - Windows uses the in-process lock.
    fcntl = None

from .keyfiles import Keyfile, Keypair
from .sp_core import (
    CRYPTO_SR25519,
    FIXED_PQ_CRYPTO_TYPES,
    PQ_CRYPTO_TYPES,
    REGISTERED_CRYPTO_TYPES,
    account_crypto_type,
)

DEFAULT_WALLET_PATH = str(Path.home() / ".bittensor" / "wallets")
_DIRECTORY_LOCKS: dict[Path, threading.RLock] = {}
_DIRECTORY_LOCKS_GUARD = threading.Lock()
_DIRECTORY_LOCK_DEPTH = threading.local()


@contextmanager
def _wallet_directory_lock(directory: Path):
    """Serialize cooperating writers without adding another wallet file."""
    directory.mkdir(parents=True, exist_ok=True, mode=0o700)
    directory = directory.resolve()
    with _DIRECTORY_LOCKS_GUARD:
        lock = _DIRECTORY_LOCKS.setdefault(directory, threading.RLock())
    with lock:
        depths = getattr(_DIRECTORY_LOCK_DEPTH, "depths", None)
        if depths is None:
            depths = {}
            _DIRECTORY_LOCK_DEPTH.depths = depths
        depth = depths.get(directory, 0)
        fd = None
        if depth == 0 and fcntl is not None:
            fd = os.open(directory, os.O_RDONLY | getattr(os, "O_DIRECTORY", 0))
            try:
                fcntl.flock(fd, fcntl.LOCK_EX)
            except BaseException:
                os.close(fd)
                raise
        depths[directory] = depth + 1
        try:
            yield
        finally:
            if depth == 0:
                depths.pop(directory)
                if fd is not None:
                    try:
                        fcntl.flock(fd, fcntl.LOCK_UN)
                    finally:
                        os.close(fd)
            else:
                depths[directory] = depth


def _cleanup_backups(backup_dir: Path, backups: dict[Path, Path]) -> None:
    """Unlink old key inodes before removing the now-empty backup directory."""
    logger = logging.getLogger(__name__)
    for backup in backups.values():
        try:
            backup.unlink(missing_ok=True)
        except OSError:
            logger.error(
                "old wallet keyfile backup could not be removed: %s", backup, exc_info=True
            )
            return
    try:
        backup_dir.rmdir()
    except OSError:
        logger.warning(
            "empty wallet backup directory could not be removed: %s", backup_dir, exc_info=True
        )


def _keyfile_inode(path: Path) -> tuple[int, int] | None:
    try:
        current = path.lstat()
    except FileNotFoundError:
        return None
    return current.st_dev, current.st_ino


def _seed_bytes(seed: str | bytes) -> bytes:
    if isinstance(seed, bytes):
        return seed
    return bytes.fromhex(seed.removeprefix("0x"))


def _public_only(keypair: Keypair) -> Keypair:
    if keypair.crypto_type in REGISTERED_CRYPTO_TYPES:
        return keypair.public_only()
    return Keypair(
        ss58_address=keypair.ss58_address,
        public_key=bytes(keypair.public_key),
        crypto_type=keypair.crypto_type,
        ss58_format=keypair.ss58_format,
    )


def _restore_public_key(address: str | None, public_key: str | None, crypto_type: int) -> Keypair:
    from .receiving import is_receiving_address, parse_recipient

    if address is not None and is_receiving_address(address):
        recipient = parse_recipient(address)
        keypair = Keypair.from_hashed_descriptor(recipient.descriptor)
        if public_key is not None:
            encoded = bytes.fromhex(public_key.removeprefix("0x"))
            if encoded != bytes(keypair.public_key):
                raise ValueError("public key does not match the receiving address")
        return keypair
    if crypto_type in REGISTERED_CRYPTO_TYPES and crypto_type not in FIXED_PQ_CRYPTO_TYPES:
        raise ValueError(
            "hashed watch-only recovery requires the complete receiving address; "
            "copy it from `btcli wallet show`"
        )
    return Keypair(
        ss58_address=address,
        public_key=bytes.fromhex(public_key.removeprefix("0x")) if public_key else None,
        crypto_type=crypto_type,
    )


class Wallet:
    """A named wallet directory under ``~/.bittensor/wallets/<name>/``.

    ``Wallet("cold/hot")`` is shorthand for ``Wallet("cold", "hot")`` — the
    same ``WALLET/HOTKEY`` form the CLI accepts.
    """

    def __init__(
        self,
        name: str = "default",
        hotkey: str = "default",
        path: str = DEFAULT_WALLET_PATH,
    ) -> None:
        if "/" in name:
            if hotkey != "default":
                raise ValueError(
                    "give the hotkey either in the name ('cold/hot') or as the "
                    "hotkey= argument, not both"
                )
            compound = name
            name, _, hotkey = name.partition("/")
            if not name or not hotkey or "/" in hotkey:
                raise ValueError(
                    f"invalid wallet name {compound!r}: expected 'wallet/hotkey' "
                    "with exactly one slash and both parts non-empty"
                )
        self.name = name
        self.path = path
        self.hotkey_str = hotkey
        self._coldkey: Keypair | None = None
        self._coldkeypub: Keypair | None = None
        self._hotkey: Keypair | None = None
        self._hotkeypub: Keypair | None = None
        self._preapproved_overwrites: dict[Path, tuple[tuple[int, int] | None, bool]] | None = None
        self._last_cold_published: tuple[tuple[Path, tuple[int, int]], ...] = ()

        wallet_dir = Path(self.path).expanduser() / self.name
        self._coldkey_file = Keyfile(wallet_dir / "coldkey", "coldkey")
        self._coldkeypub_file = Keyfile(wallet_dir / "coldkeypub.txt", "coldkeypub.txt")
        self._hotkey_file = Keyfile(wallet_dir / "hotkeys" / self.hotkey_str, self.hotkey_str)
        self._hotkeypub_file = Keyfile(
            wallet_dir / "hotkeys" / f"{self.hotkey_str}pub.txt",
            f"{self.hotkey_str}pub.txt",
        )

    def __repr__(self) -> str:
        return f"Wallet(name={self.name!r}, hotkey={self.hotkey_str!r}, path={self.path!r})"

    @property
    def hotkey(self) -> Keypair:
        if self._hotkey is None:
            self._hotkey = self._hotkey_file.get_keypair()
        return self._hotkey

    @property
    def coldkey(self) -> Keypair:
        if self._coldkey is None:
            self._coldkey = self._coldkey_file.get_keypair()
        return self._coldkey

    @property
    def coldkeypub(self) -> Keypair:
        if self._coldkeypub is None:
            self._coldkeypub = self._coldkeypub_file.get_keypair()
        return self._coldkeypub

    @property
    def hotkeypub(self) -> Keypair:
        if self._hotkeypub is None:
            self._hotkeypub = self._hotkeypub_file.get_keypair()
        return self._hotkeypub

    @property
    def coldkey_file(self) -> Keyfile:
        return self._coldkey_file

    @property
    def coldkeypub_file(self) -> Keyfile:
        return self._coldkeypub_file

    @property
    def hotkey_file(self) -> Keyfile:
        return self._hotkey_file

    @property
    def hotkeypub_file(self) -> Keyfile:
        return self._hotkeypub_file

    def get_coldkey(self, password: str | None = None) -> Keypair:
        keypair = self._coldkey_file.get_keypair(password)
        self._coldkey = keypair
        return keypair

    def get_hotkey(self, password: str | None = None) -> Keypair:
        keypair = self._hotkey_file.get_keypair(password)
        self._hotkey = keypair
        return keypair

    def _set_keypair_with_public(
        self,
        keypair: Keypair,
        *,
        private_file: Keyfile,
        public_file: Keyfile,
        encrypt: bool,
        overwrite: bool,
        save_to_env: bool,
        password: str | None,
    ) -> tuple[tuple[Path, tuple[int, int]], ...]:
        with _wallet_directory_lock(Path(self._coldkey_file.path).parent):
            return self._set_keypair_with_public_locked(
                keypair,
                private_file=private_file,
                public_file=public_file,
                encrypt=encrypt,
                overwrite=overwrite,
                save_to_env=save_to_env,
                password=password,
            )

    def _set_keypair_with_public_locked(
        self,
        keypair: Keypair,
        *,
        private_file: Keyfile,
        public_file: Keyfile,
        encrypt: bool,
        overwrite: bool,
        save_to_env: bool,
        password: str | None,
    ) -> tuple[tuple[Path, tuple[int, int]], ...]:
        """Publish the public and private files as a recoverable pair."""
        files = (public_file, private_file)
        paths = tuple(Path(file.path) for file in files)
        if any(path.is_symlink() for path in paths):
            raise ValueError("paired wallet keyfiles cannot be symlinks")
        if self._preapproved_overwrites is None:
            original = tuple(_keyfile_inode(path) for path in paths)
            # Keep the private-then-public prompt order of the original API.
            private_approved = private_file._confirm_overwrite(overwrite)
            public_approved = public_file._confirm_overwrite(overwrite)
            approved = (public_approved, private_approved)
        else:
            entries = tuple(self._preapproved_overwrites[path] for path in paths)
            original = tuple(entry[0] for entry in entries)
            approved = tuple(entry[1] for entry in entries)
        existed = tuple(inode is not None for inode in original)
        if tuple(_keyfile_inode(path) for path in paths) != original:
            raise FileExistsError("wallet keyfiles changed during overwrite approval")
        for file in files:
            file.make_dirs()

        backup_dir = Path(tempfile.mkdtemp(prefix=".wallet-backup.", dir=paths[0].parent))
        backups: dict[Path, Path] = {}
        staged: list[Path] = []
        published: list[tuple[Path, tuple[int, int]]] = []
        rollback_failed = False
        old_save_to_env = private_file.should_save_to_env
        env_name = private_file.env_var_name()
        old_env_value = os.environ.get(env_name)

        def record_publish(path: Path, inode: tuple[int, int]) -> None:
            published.append((path, inode))

        try:
            public_payload, _ = public_file._prepare_keypair_payload(
                _public_only(keypair), False, None
            )
            private_payload, resolved = private_file._prepare_keypair_payload(
                keypair, encrypt, password
            )
            staged.append(public_file._stage_data(public_payload))
            staged.append(private_file._stage_data(private_payload))
            for index, (path, was_present) in enumerate(zip(paths, existed)):
                if was_present:
                    if _keyfile_inode(path) != original[index]:
                        raise FileExistsError(f"wallet keyfile changed during staging: {path}")
                    backup = backup_dir / path.name
                    os.link(path, backup, follow_symlinks=False)
                    backups[path] = backup
                    if _keyfile_inode(backup) != original[index]:
                        raise FileExistsError(f"wallet keyfile changed during backup: {path}")

            if tuple(_keyfile_inode(path) for path in paths) != original:
                raise FileExistsError("wallet keyfiles changed before publication")

            public_file._publish_staged(
                staged[0],
                overwrite=approved[0] and existed[0],
                _on_publish=record_publish,
            )
            if _keyfile_inode(paths[1]) != original[1]:
                raise FileExistsError("private keyfile changed before publication")
            private_file._publish_staged(
                staged[1],
                overwrite=approved[1] and existed[1],
                _on_publish=record_publish,
            )
            private_file.should_save_to_env = save_to_env
            if encrypt and save_to_env:
                private_file.save_password_to_env(resolved)
        except BaseException:
            # Restore only our published inodes. A concurrent replacement must
            # never be overwritten by rollback.
            for path, inode in reversed(published):
                try:
                    current = path.lstat()
                    if (current.st_dev, current.st_ino) != inode:
                        raise RuntimeError(f"keyfile changed concurrently: {path}")
                    backup = backups.get(path)
                    if backup is None:
                        path.unlink()
                    else:
                        os.replace(backup, path)
                except OSError as rollback_exc:
                    rollback_failed = True
                    raise RuntimeError(
                        f"could not restore wallet keyfile {path}; backup remains in {backup_dir}"
                    ) from rollback_exc
                except RuntimeError:
                    rollback_failed = True
                    raise
            private_file.should_save_to_env = old_save_to_env
            if old_env_value is None:
                os.environ.pop(env_name, None)
            else:
                os.environ[env_name] = old_env_value
            raise
        finally:
            for temporary in staged:
                temporary.unlink(missing_ok=True)
            if not rollback_failed:
                _cleanup_backups(backup_dir, backups)
        return tuple(published)

    def _set_coldkey(
        self,
        keypair: Keypair,
        encrypt: bool,
        overwrite: bool,
        save_coldkey_to_env: bool,
        coldkey_password: str | None,
    ) -> None:
        self._last_cold_published = ()
        self._last_cold_published = self._set_keypair_with_public(
            keypair,
            private_file=self._coldkey_file,
            public_file=self._coldkeypub_file,
            encrypt=encrypt,
            overwrite=overwrite,
            save_to_env=save_coldkey_to_env,
            password=coldkey_password,
        )
        self._coldkey = keypair
        self._coldkeypub = _public_only(keypair)

    def _set_coldkeypub(self, keypair: Keypair, overwrite: bool) -> None:
        public = _public_only(keypair)
        with _wallet_directory_lock(Path(self._coldkey_file.path).parent):
            self._coldkeypub_file.set_keypair(public, encrypt=False, overwrite=overwrite)
            self._coldkeypub = public

    def _set_hotkey(
        self,
        keypair: Keypair,
        encrypt: bool,
        overwrite: bool,
        save_hotkey_to_env: bool,
        hotkey_password: str | None,
    ) -> None:
        self._set_keypair_with_public(
            keypair,
            private_file=self._hotkey_file,
            public_file=self._hotkeypub_file,
            encrypt=encrypt,
            overwrite=overwrite,
            save_to_env=save_hotkey_to_env,
            password=hotkey_password,
        )
        self._hotkey = keypair
        self._hotkeypub = _public_only(keypair)

    def _set_hotkeypub(self, keypair: Keypair, overwrite: bool) -> None:
        public = _public_only(keypair)
        with _wallet_directory_lock(Path(self._coldkey_file.path).parent):
            self._hotkeypub_file.set_keypair(public, encrypt=False, overwrite=overwrite)
            self._hotkeypub = public

    def create_new_coldkey(
        self,
        n_words: int | None = None,
        use_password: bool = True,
        overwrite: bool = False,
        suppress: bool = False,
        save_coldkey_to_env: bool = False,
        coldkey_password: str | None = None,
        crypto_type: int = CRYPTO_SR25519,
        on_mnemonic: Callable[[str], None] | None = None,
        account_type: str | None = None,
    ) -> Wallet:
        crypto_type = account_crypto_type(crypto_type, account_type)
        words = (24 if crypto_type in PQ_CRYPTO_TYPES else 12) if n_words is None else n_words
        if crypto_type in PQ_CRYPTO_TYPES and words != 24:
            raise ValueError("post-quantum wallets require a fresh 24-word mnemonic")
        mnemonic = Keypair.generate_mnemonic(words)
        keypair = Keypair.create_from_mnemonic(mnemonic, crypto_type)
        # on_mnemonic lets a caller (the CLI) render the mnemonic itself
        # instead of this library printing it raw to stdout.
        if on_mnemonic is not None:
            on_mnemonic(mnemonic)
        elif not suppress:
            print(f"Generating new coldkey\nMnemonic: {mnemonic}")
        if coldkey_password is not None:
            use_password = True
        self._set_coldkey(
            keypair,
            use_password,
            overwrite,
            save_coldkey_to_env,
            coldkey_password,
        )
        return self

    def create_new_hotkey(
        self,
        n_words: int | None = None,
        use_password: bool = False,
        overwrite: bool = False,
        suppress: bool = False,
        save_hotkey_to_env: bool = False,
        hotkey_password: str | None = None,
        crypto_type: int = CRYPTO_SR25519,
        on_mnemonic: Callable[[str], None] | None = None,
        account_type: str | None = None,
    ) -> Wallet:
        crypto_type = account_crypto_type(crypto_type, account_type)
        words = (24 if crypto_type in PQ_CRYPTO_TYPES else 12) if n_words is None else n_words
        if crypto_type in PQ_CRYPTO_TYPES and words != 24:
            raise ValueError("post-quantum wallets require a fresh 24-word mnemonic")
        mnemonic = Keypair.generate_mnemonic(words)
        keypair = Keypair.create_from_mnemonic(mnemonic, crypto_type)
        if on_mnemonic is not None:
            on_mnemonic(mnemonic)
        elif not suppress:
            print(f"Generating new hotkey\nMnemonic: {mnemonic}")
        if hotkey_password is not None:
            use_password = True
        self._set_hotkey(
            keypair,
            use_password,
            overwrite,
            save_hotkey_to_env,
            hotkey_password,
        )
        return self

    def regenerate_coldkey(
        self,
        mnemonic: str | None = None,
        seed: str | bytes | None = None,
        private_key: str | None = None,
        json: tuple[str, str] | None = None,
        use_password: bool = True,
        overwrite: bool = False,
        suppress: bool = False,
        save_coldkey_to_env: bool = False,
        coldkey_password: str | None = None,
        crypto_type: int = CRYPTO_SR25519,
        account_type: str | None = None,
        **_: Any,
    ) -> Wallet:
        crypto_type = account_crypto_type(crypto_type, account_type)
        if mnemonic is not None:
            if not suppress:
                print(f"Regenerating coldkey from mnemonic\nMnemonic: {mnemonic}")
            keypair = Keypair.create_from_mnemonic(mnemonic, crypto_type)
        elif private_key is not None:
            if not suppress:
                print("Regenerating coldkey from private key")
            keypair = Keypair.create_from_private_key(private_key, crypto_type)
        elif seed is not None:
            keypair = Keypair.create_from_seed(_seed_bytes(seed), crypto_type)
        elif json is not None:
            json_data, passphrase = json
            if not json_data:
                raise ValueError("encrypted JSON coldkey data is empty")
            if not passphrase:
                raise ValueError("passphrase required for encrypted JSON import")
            if not suppress:
                print("Regenerating coldkey from encrypted JSON keystore")
            keypair = Keypair.create_from_encrypted_json(json_data, passphrase)
        else:
            raise ValueError("either mnemonic, seed, private_key, or json must be provided")

        if coldkey_password is not None:
            use_password = True
        self._set_coldkey(
            keypair,
            use_password,
            overwrite,
            save_coldkey_to_env,
            coldkey_password,
        )
        return self

    def regenerate_hotkey(
        self,
        mnemonic: str | None = None,
        seed: str | bytes | None = None,
        private_key: str | None = None,
        use_password: bool = False,
        overwrite: bool = False,
        suppress: bool = False,
        save_hotkey_to_env: bool = False,
        hotkey_password: str | None = None,
        crypto_type: int = CRYPTO_SR25519,
        account_type: str | None = None,
        **_: Any,
    ) -> Wallet:
        crypto_type = account_crypto_type(crypto_type, account_type)
        if mnemonic is not None:
            if not suppress:
                print(f"Regenerating hotkey from mnemonic\nMnemonic: {mnemonic}")
            keypair = Keypair.create_from_mnemonic(mnemonic, crypto_type)
        elif private_key is not None:
            if not suppress:
                print("Regenerating hotkey from private key")
            keypair = Keypair.create_from_private_key(private_key, crypto_type)
        elif seed is not None:
            keypair = Keypair.create_from_seed(_seed_bytes(seed), crypto_type)
        else:
            raise ValueError("either mnemonic, seed, or private_key must be provided")

        if hotkey_password is not None:
            use_password = True
        self._set_hotkey(
            keypair,
            use_password,
            overwrite,
            save_hotkey_to_env,
            hotkey_password,
        )
        return self

    def regenerate_coldkeypub(
        self,
        ss58_address: str | None = None,
        public_key: str | None = None,
        overwrite: bool = False,
        suppress: bool = True,
        crypto_type: int = CRYPTO_SR25519,
        account_type: str | None = None,
        **_: Any,
    ) -> Wallet:
        crypto_type = account_crypto_type(crypto_type, account_type)
        del suppress
        if ss58_address is None and public_key is None:
            raise ValueError("either ss58_address or public_key must be passed")
        keypair = _restore_public_key(ss58_address, public_key, crypto_type)
        self._set_coldkeypub(keypair, overwrite)
        return self

    def regenerate_hotkeypub(
        self,
        ss58_address: str | None = None,
        public_key: str | None = None,
        overwrite: bool = False,
        suppress: bool = True,
        crypto_type: int = CRYPTO_SR25519,
        account_type: str | None = None,
        **_: Any,
    ) -> Wallet:
        crypto_type = account_crypto_type(crypto_type, account_type)
        del suppress
        if ss58_address is None and public_key is None:
            raise ValueError("either ss58_address or public_key must be passed")
        keypair = _restore_public_key(ss58_address, public_key, crypto_type)
        self._set_hotkeypub(keypair, overwrite)
        return self

    def unlock_coldkey(self) -> Keypair:
        if self._coldkey is None:
            self._coldkey = self._coldkey_file.get_keypair()
        return self._coldkey

    def unlock_hotkey(self) -> Keypair:
        if self._hotkey is None:
            self._hotkey = self._hotkey_file.get_keypair()
        return self._hotkey


__all__ = ["DEFAULT_WALLET_PATH", "Wallet"]
