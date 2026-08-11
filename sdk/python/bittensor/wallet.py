"""Native Bittensor wallet — owns keyfile layout and creation/regeneration flows."""

from __future__ import annotations

from pathlib import Path
from typing import Any

from .keyfiles import Keyfile, Keypair
from .sp_core import CRYPTO_SR25519

DEFAULT_WALLET_PATH = str(Path.home() / ".bittensor" / "wallets")


def _seed_bytes(seed: str | bytes) -> bytes:
    if isinstance(seed, bytes):
        return seed
    return bytes.fromhex(seed.removeprefix("0x"))


def _public_only(keypair: Keypair) -> Keypair:
    return Keypair(
        ss58_address=keypair.ss58_address,
        public_key=bytes(keypair.public_key),
        crypto_type=keypair.crypto_type,
        ss58_format=keypair.ss58_format,
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

    def _set_coldkey(
        self,
        keypair: Keypair,
        encrypt: bool,
        overwrite: bool,
        save_coldkey_to_env: bool,
        coldkey_password: str | None,
    ) -> None:
        self._coldkey = keypair
        self._coldkey_file.should_save_to_env = save_coldkey_to_env
        self._coldkey_file.set_keypair(
            keypair,
            encrypt=encrypt,
            overwrite=overwrite,
            password=coldkey_password,
        )

    def _set_coldkeypub(self, keypair: Keypair, overwrite: bool) -> None:
        public = _public_only(keypair)
        self._coldkeypub = public
        self._coldkeypub_file.set_keypair(public, encrypt=False, overwrite=overwrite)

    def _set_hotkey(
        self,
        keypair: Keypair,
        encrypt: bool,
        overwrite: bool,
        save_hotkey_to_env: bool,
        hotkey_password: str | None,
    ) -> None:
        self._hotkey = keypair
        self._hotkey_file.should_save_to_env = save_hotkey_to_env
        self._hotkey_file.set_keypair(
            keypair,
            encrypt=encrypt,
            overwrite=overwrite,
            password=hotkey_password,
        )

    def _set_hotkeypub(self, keypair: Keypair, overwrite: bool) -> None:
        public = _public_only(keypair)
        self._hotkeypub = public
        self._hotkeypub_file.set_keypair(public, encrypt=False, overwrite=overwrite)

    def create_new_coldkey(
        self,
        n_words: int = 12,
        use_password: bool = True,
        overwrite: bool = False,
        suppress: bool = False,
        save_coldkey_to_env: bool = False,
        coldkey_password: str | None = None,
        crypto_type: int = CRYPTO_SR25519,
    ) -> Wallet:
        mnemonic = Keypair.generate_mnemonic(n_words)
        keypair = Keypair.create_from_mnemonic(mnemonic, crypto_type)
        if not suppress:
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
        self._set_coldkeypub(keypair, overwrite)
        return self

    def create_new_hotkey(
        self,
        n_words: int = 12,
        use_password: bool = False,
        overwrite: bool = False,
        suppress: bool = False,
        save_hotkey_to_env: bool = False,
        hotkey_password: str | None = None,
        crypto_type: int = CRYPTO_SR25519,
    ) -> Wallet:
        mnemonic = Keypair.generate_mnemonic(n_words)
        keypair = Keypair.create_from_mnemonic(mnemonic, crypto_type)
        if not suppress:
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
        self._set_hotkeypub(keypair, overwrite)
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
        **_: Any,
    ) -> Wallet:
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
        self._set_coldkeypub(keypair, overwrite)
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
        **_: Any,
    ) -> Wallet:
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
        self._set_hotkeypub(keypair, overwrite)
        return self

    def regenerate_coldkeypub(
        self,
        ss58_address: str | None = None,
        public_key: str | None = None,
        overwrite: bool = False,
        suppress: bool = True,
        crypto_type: int = CRYPTO_SR25519,
        **_: Any,
    ) -> Wallet:
        del suppress
        if ss58_address is None and public_key is None:
            raise ValueError("either ss58_address or public_key must be passed")
        keypair = Keypair(
            ss58_address=ss58_address,
            public_key=bytes.fromhex(public_key.removeprefix("0x")) if public_key else None,
            crypto_type=crypto_type,
        )
        self._set_coldkeypub(keypair, overwrite)
        return self

    def regenerate_hotkeypub(
        self,
        ss58_address: str | None = None,
        public_key: str | None = None,
        overwrite: bool = False,
        suppress: bool = True,
        crypto_type: int = CRYPTO_SR25519,
        **_: Any,
    ) -> Wallet:
        del suppress
        if ss58_address is None and public_key is None:
            raise ValueError("either ss58_address or public_key must be passed")
        keypair = Keypair(
            ss58_address=ss58_address,
            public_key=bytes.fromhex(public_key.removeprefix("0x")) if public_key else None,
            crypto_type=crypto_type,
        )
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
