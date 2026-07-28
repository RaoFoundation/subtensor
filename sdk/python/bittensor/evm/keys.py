"""EVM key management: encrypted keystores next to the wallet's hotkeys.

EVM keys are secp256k1 — a key kind the sr25519/ed25519 wallet cannot hold — so
they live in their own directory inside the wallet:

    ~/.bittensor/wallets/<wallet>/evmkeys/<name>

Each file is a standard Ethereum keystore V3 JSON (scrypt + AES-128-CTR),
the format geth, MetaMask, ethers, and web3.py all read, so a key moves
between btcli and any Ethereum tool by copying one file. The address is
embedded in plaintext, so listing never needs a password.

These keys custody funds, so unlike hotkeys they are always encrypted; the
password resolves through the same chain as coldkeys (explicit argument,
``BT_WALLET_PASSWORD``, password file, macOS Keychain / dialog).
"""

from __future__ import annotations

import contextlib
import getpass
import json
import os
import stat
import sys
from dataclasses import dataclass
from pathlib import Path

from eth_account import Account
from eth_account.signers.local import LocalAccount

from ..wallets import DEFAULT_WALLET_PATH
from .addresses import h160_to_ss58, normalize_h160

_EVM_DIR = "evmkeys"

# BIP-44 Ethereum derivation path, for --from-mnemonic imports (account 0).
ETH_DERIVATION_PATH = "m/44'/60'/0'/0/0"


@dataclass
class EvmKeyInfo:
    """Public facts about one stored EVM key (readable without a password)."""

    name: str
    address: str  # 0x-prefixed h160
    ss58_mirror: str  # where its native-side balance lives
    path: str


def _evm_dir(wallet_name: str, wallet_path: str = DEFAULT_WALLET_PATH) -> Path:
    return Path(wallet_path).expanduser() / wallet_name / _EVM_DIR


def keyfile_path(name: str, wallet_name: str, wallet_path: str = DEFAULT_WALLET_PATH) -> Path:
    return _evm_dir(wallet_name, wallet_path) / name


def _info(path: Path) -> "EvmKeyInfo | None":
    try:
        data = json.loads(path.read_text())
        address = normalize_h160(str(data["address"]))
    except (OSError, ValueError, KeyError):
        return None
    return EvmKeyInfo(
        name=path.name,
        address=address,
        ss58_mirror=h160_to_ss58(address),
        path=str(path),
    )


def list_evm_keys(wallet_name: str, wallet_path: str = DEFAULT_WALLET_PATH) -> list[EvmKeyInfo]:
    """All EVM keys stored in a wallet, from keystore metadata only (no unlock)."""
    directory = _evm_dir(wallet_name, wallet_path)
    if not directory.is_dir():
        return []
    infos = (_info(p) for p in sorted(directory.iterdir()) if p.is_file())
    return [info for info in infos if info is not None]


def get_evm_key_info(
    name: str, wallet_name: str, wallet_path: str = DEFAULT_WALLET_PATH
) -> EvmKeyInfo:
    """Public info for one stored key; raises with the available names on a miss."""
    path = keyfile_path(name, wallet_name, wallet_path)
    info = _info(path)
    if info is None:
        available = [k.name for k in list_evm_keys(wallet_name, wallet_path)]
        listing = f" Available: {', '.join(available)}." if available else ""
        raise ValueError(f"no EVM key {name!r} in wallet {wallet_name!r}.{listing}")
    return info


def write_keystore_file(path: Path, keystore: dict) -> None:
    """Write keystore JSON with owner-only permissions from file creation."""
    path.parent.mkdir(parents=True, exist_ok=True)
    content = json.dumps(keystore, indent=2) + "\n"
    fd = os.open(path, os.O_CREAT | os.O_WRONLY | os.O_TRUNC, stat.S_IRUSR | stat.S_IWUSR)
    try:
        with os.fdopen(fd, "w") as handle:
            handle.write(content)
    except Exception:
        with contextlib.suppress(OSError):
            os.close(fd)
        raise


def _write_keystore(path: Path, keystore: dict) -> None:
    write_keystore_file(path, keystore)


def _resolve_password(password: "str | None", *, confirm: bool) -> str:
    """Password for encrypting/decrypting a keystore.

    Explicit argument wins, then ``BT_WALLET_PASSWORD`` (the same env the
    coldkey unlock chain uses), then an interactive prompt. Refuses to prompt
    in a non-interactive session.
    """
    if password:
        return password
    env = os.getenv("BT_WALLET_PASSWORD")
    if env:
        return env
    if not sys.stdin.isatty():
        raise ValueError("no password available: pass one explicitly or set BT_WALLET_PASSWORD")
    first = getpass.getpass("EVM key password: ")
    if not first:
        raise ValueError("empty password")
    if confirm:
        second = getpass.getpass("Retype password: ")
        if first != second:
            raise ValueError("passwords do not match")
    return first


def _store(
    account: "LocalAccount",
    name: str,
    wallet_name: str,
    wallet_path: str,
    password: "str | None",
    overwrite: bool,
) -> EvmKeyInfo:
    path = keyfile_path(name, wallet_name, wallet_path)
    if path.exists() and not overwrite:
        raise ValueError(
            f"EVM key {name!r} already exists in wallet {wallet_name!r} "
            "(pass overwrite to replace it)"
        )
    resolved = _resolve_password(password, confirm=True)
    _write_keystore(path, Account.encrypt(account.key, resolved))
    info = _info(path)
    assert info is not None
    return info


def create_evm_key(
    name: str = "default",
    wallet_name: str = "default",
    wallet_path: str = DEFAULT_WALLET_PATH,
    *,
    password: "str | None" = None,
    overwrite: bool = False,
) -> EvmKeyInfo:
    """Generate a fresh random EVM key and store it encrypted.

    Deliberately *not* derived from the coldkey mnemonic: an EVM key's
    compromise surface (browser wallets, dapp signing) should never chain
    back to the coldkey seed. Use ``import_evm_key(mnemonic=...)`` for BIP-44
    derivation from a seed you manage yourself.
    """
    return _store(Account.create(), name, wallet_name, wallet_path, password, overwrite)


def import_evm_key(
    name: str = "default",
    wallet_name: str = "default",
    wallet_path: str = DEFAULT_WALLET_PATH,
    *,
    private_key: "str | None" = None,
    keystore_json: "str | None" = None,
    keystore_password: "str | None" = None,
    mnemonic: "str | None" = None,
    derivation_path: str = ETH_DERIVATION_PATH,
    password: "str | None" = None,
    overwrite: bool = False,
) -> EvmKeyInfo:
    """Import an EVM key from a raw private key, keystore V3 JSON, or BIP-39 mnemonic.

    Exactly one source must be given. ``keystore_password`` unlocks an
    imported keystore (defaults to the storage password); ``password``
    encrypts the stored copy.
    """
    sources = [private_key, keystore_json, mnemonic]
    if sum(source is not None for source in sources) != 1:
        raise ValueError("provide exactly one of: private_key, keystore_json, mnemonic")
    if private_key is not None:
        account = Account.from_key(private_key.strip())
    elif keystore_json is not None:
        unlock = keystore_password or _resolve_password(password, confirm=False)
        account = Account.from_key(Account.decrypt(json.loads(keystore_json), unlock))
    else:
        Account.enable_unaudited_hdwallet_features()
        assert mnemonic is not None
        account = Account.from_mnemonic(mnemonic.strip(), account_path=derivation_path)
    return _store(account, name, wallet_name, wallet_path, password, overwrite)


def unlock_evm_key(
    name: str = "default",
    wallet_name: str = "default",
    wallet_path: str = DEFAULT_WALLET_PATH,
    *,
    password: "str | None" = None,
) -> "LocalAccount":
    """Decrypt a stored EVM key into a signing account (address + private key)."""
    path = keyfile_path(name, wallet_name, wallet_path)
    if not path.is_file():
        get_evm_key_info(name, wallet_name, wallet_path)  # raises with the listing
    keystore = json.loads(path.read_text())
    resolved = _resolve_password(password, confirm=False)
    try:
        return Account.from_key(Account.decrypt(keystore, resolved))
    except ValueError as error:
        raise ValueError(
            f"could not unlock EVM key {name!r} in wallet {wallet_name!r}: {error}"
        ) from error


def export_evm_key(
    name: str = "default",
    wallet_name: str = "default",
    wallet_path: str = DEFAULT_WALLET_PATH,
) -> dict:
    """The stored keystore V3 JSON, as-is — importable by MetaMask/geth/ethers.

    Still encrypted; the private key never leaves the file unencrypted. Use
    ``unlock_evm_key`` when the raw key itself is needed.
    """
    path = keyfile_path(name, wallet_name, wallet_path)
    if not path.is_file():
        get_evm_key_info(name, wallet_name, wallet_path)  # raises with the listing
    return json.loads(path.read_text())
