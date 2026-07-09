"""Thin conveniences over the native Bittensor wallet implementation.

Key management lives in :mod:`bittensor.wallet` and :mod:`bittensor.keyfiles`.
This module adds helpers for the common create / regenerate / list flows.
"""

from __future__ import annotations

import json
import os
from dataclasses import dataclass, field
from pathlib import Path

from ._transport.codec import is_valid_ss58_address
from .keyfiles import Keypair, WrongPasswordError, resolve_key_password
from .settings import SS58_FORMAT
from .sp_core import CRYPTO_ED25519, CRYPTO_SR25519
from .wallet import DEFAULT_WALLET_PATH, Wallet

CRYPTO_TYPE_NAMES: dict[int, str] = {
    CRYPTO_ED25519: "ed25519",
    CRYPTO_SR25519: "sr25519",
}
_NAME_TO_CRYPTO_TYPE: dict[str, int] = {name: code for code, name in CRYPTO_TYPE_NAMES.items()}
DEFAULT_CRYPTO_TYPE = CRYPTO_SR25519


def is_bittensor_address(value: str) -> bool:
    """True when ``value`` is a valid ss58 address in Bittensor's format.

    The canonical address check for user-supplied input: pinned to the
    network's ss58 format so an address pasted from another Substrate chain
    (same key, different checksum prefix) is rejected instead of silently
    accepted.
    """
    return is_valid_ss58_address(value, SS58_FORMAT)


def parse_crypto_type(value: str) -> int:
    """Parse ``ed25519`` / ``sr25519`` (or ``0`` / ``1``) to a wallet crypto type."""
    normalized = value.strip().lower()
    if normalized in _NAME_TO_CRYPTO_TYPE:
        return _NAME_TO_CRYPTO_TYPE[normalized]
    if normalized.isdigit():
        code = int(normalized)
        if code in CRYPTO_TYPE_NAMES:
            return code
    raise ValueError(
        f"unknown crypto type {value!r}; expected one of: {', '.join(CRYPTO_TYPE_NAMES.values())}"
    )


def format_crypto_type(crypto_type: int | None) -> str:
    """Human-readable crypto scheme name; legacy keyfiles without a type default to sr25519."""
    if crypto_type is None:
        return CRYPTO_TYPE_NAMES[DEFAULT_CRYPTO_TYPE]
    return CRYPTO_TYPE_NAMES.get(crypto_type, f"unknown({crypto_type})")


def resolve_wallet_password(
    wallet: Wallet,
    *,
    password: str | None = None,
    password_file: str | None = None,
    macos_prompt: bool = False,
    keychain: bool = False,
) -> str | None:
    """Resolve a coldkey password for non-interactive unlock.

    Precedence (first match wins):
    1. explicit ``password`` argument (SDK / tests)
    2. ``BT_WALLET_PASSWORD`` environment variable (Doppler-friendly)
    3. per-wallet env from ``BT_PW__...`` (see :func:`keyfiles.resolve_key_password`)
    4. ``password_file`` argument or ``BT_WALLET_PASSWORD_FILE`` env (one line)
    5. macOS Keychain (``--keychain-password``)
    6. native macOS password dialog (``--macos-password``)
    7. ``None`` — prompts interactively if the key is encrypted
    """
    if password:
        return password
    env_password = os.getenv("BT_WALLET_PASSWORD")
    if env_password:
        return env_password
    native = resolve_key_password(
        None,
        env_var_name=wallet.coldkey_file.env_var_name(),
        allow_prompt=False,
    )
    if native:
        return native
    path = password_file or os.getenv("BT_WALLET_PASSWORD_FILE")
    if path:
        try:
            return Path(path).expanduser().read_text().strip()
        except OSError as error:
            raise ValueError(f"cannot read wallet password file {path!r}: {error}") from error
    if keychain:
        from .macos_password import keychain_load

        stored = keychain_load(wallet.name)
        if stored:
            return stored
        if not macos_prompt:
            raise ValueError(
                f"no Keychain password for wallet {wallet.name!r}; "
                "run `btcli wallet keychain save -w "
                f"{wallet.name}`"
            )
    if macos_prompt:
        from .macos_password import prompt_password

        return prompt_password(
            title=f"Unlock {wallet.name} coldkey",
            message=f"Enter the password for wallet {wallet.name!r}.",
        )
    return None


def signing_keypair(
    wallet: Wallet,
    signer: str,
    *,
    password: str | None = None,
    password_file: str | None = None,
    macos_prompt: bool = False,
    keychain: bool = False,
):
    """Return the keypair that signs extrinsics, unlocking an encrypted coldkey if needed."""
    if signer not in ("coldkey", "hotkey"):
        raise ValueError(f"signer must be 'coldkey' or 'hotkey', got {signer!r}")
    if signer == "hotkey":
        return wallet.get_hotkey()
    pwd = resolve_wallet_password(
        wallet,
        password=password,
        password_file=password_file,
        macos_prompt=macos_prompt,
        keychain=keychain,
    )
    try:
        return wallet.get_coldkey(password=pwd)
    except WrongPasswordError as error:
        if macos_prompt and not keychain:
            hint = (
                f" The password entered for wallet {wallet.name!r} did not decrypt its "
                "coldkey. `--macos-password` only prompts — it does not set or change the "
                "wallet password. Use the password from when the coldkey was created. After "
                f"a successful unlock, run `btcli wallet keychain save -w {wallet.name}` "
                "and use `--keychain-password`."
            )
        elif keychain:
            hint = (
                f" Keychain entry for wallet {wallet.name!r} did not decrypt its coldkey. "
                f"Re-save with `btcli wallet keychain save -w {wallet.name}`."
            )
        else:
            hint = (
                " Set BT_WALLET_PASSWORD, use --keychain-password / --macos-password, "
                "or run via `doppler run -- btcli ...`."
            )
        raise ValueError(f"Wrong password for decryption.{hint}") from error
    except OSError as error:
        raise ValueError(f"could not unlock coldkey: {error}") from error


def sign_message(
    message: str,
    *,
    name: str = "default",
    hotkey: str = "default",
    path: str = DEFAULT_WALLET_PATH,
    use: str = "coldkey",
    password: str | None = None,
    password_file: str | None = None,
    macos_prompt: bool = False,
    keychain: bool = False,
) -> dict[str, str]:
    """Sign a message with a wallet key; returns the signer address and 0x-hex signature.

    ``use`` selects the key: "coldkey" (unlocks if encrypted) or "hotkey".
    Signs the exact utf-8 bytes of ``message``.
    """
    wallet = Wallet(name=name, hotkey=hotkey, path=path)
    keypair = signing_keypair(
        wallet,
        use,
        password=password,
        password_file=password_file,
        macos_prompt=macos_prompt,
        keychain=keychain,
    )
    signature = keypair.sign(message.encode())
    return {"ss58": keypair.ss58_address, "signature": "0x" + bytes(signature).hex()}


def verify_message(message: str, signature: str, ss58_address: str) -> bool:
    """Check a 0x-hex signature over ``message`` against an address's public key."""
    try:
        keypair = Keypair(ss58_address=ss58_address)
        return keypair.verify(message.encode(), bytes.fromhex(signature.removeprefix("0x")))
    except ValueError:
        # Malformed hex or address: not verified, per the boolean contract.
        return False


def open_wallet(
    name: str = "default",
    hotkey: str = "default",
    path: str = DEFAULT_WALLET_PATH,
) -> Wallet:
    """Return a Wallet handle (does not create keys)."""
    return Wallet(name=name, hotkey=hotkey, path=path)


def create(
    name: str = "default",
    hotkey: str = "default",
    path: str = DEFAULT_WALLET_PATH,
    *,
    n_words: int = 12,
    use_password: bool = True,
    overwrite: bool = False,
    coldkey_crypto_type: int = DEFAULT_CRYPTO_TYPE,
    hotkey_crypto_type: int = DEFAULT_CRYPTO_TYPE,
) -> Wallet:
    """Create a new coldkey and hotkey for ``name``/``hotkey``."""
    wallet = Wallet(name=name, hotkey=hotkey, path=path)
    wallet.create_new_coldkey(
        n_words=n_words,
        use_password=use_password,
        overwrite=overwrite,
        crypto_type=coldkey_crypto_type,
    )
    wallet.create_new_hotkey(
        n_words=n_words,
        use_password=False,
        overwrite=overwrite,
        crypto_type=hotkey_crypto_type,
    )
    return wallet


def regen_coldkey(
    mnemonic: str | None = None,
    name: str = "default",
    path: str = DEFAULT_WALLET_PATH,
    *,
    seed: str | None = None,
    json: tuple[str, str] | None = None,
    use_password: bool = True,
    overwrite: bool = False,
    crypto_type: int = DEFAULT_CRYPTO_TYPE,
) -> Wallet:
    """Regenerate a coldkey from a mnemonic, 32-byte hex seed, or encrypted JSON."""
    wallet = Wallet(name=name, path=path)
    # suppress=True stops the wallet lib from echoing the mnemonic back to stdout;
    # the caller already supplied it, so reprinting only widens secret exposure.
    wallet.regenerate_coldkey(
        mnemonic=mnemonic,
        seed=seed,
        json=json,
        use_password=use_password,
        overwrite=overwrite,
        suppress=True,
        crypto_type=crypto_type,
    )
    return wallet


def regen_hotkey(
    mnemonic: str | None = None,
    name: str = "default",
    hotkey: str = "default",
    path: str = DEFAULT_WALLET_PATH,
    *,
    seed: str | None = None,
    overwrite: bool = False,
    crypto_type: int = DEFAULT_CRYPTO_TYPE,
) -> Wallet:
    """Regenerate a hotkey from a mnemonic or a 32-byte hex seed (exactly one)."""
    wallet = Wallet(name=name, hotkey=hotkey, path=path)
    wallet.regenerate_hotkey(
        mnemonic=mnemonic,
        seed=seed,
        use_password=False,
        overwrite=overwrite,
        suppress=True,
        crypto_type=crypto_type,
    )
    return wallet


def new_coldkey(
    name: str = "default",
    path: str = DEFAULT_WALLET_PATH,
    *,
    n_words: int = 12,
    use_password: bool = True,
    overwrite: bool = False,
    crypto_type: int = DEFAULT_CRYPTO_TYPE,
) -> Wallet:
    """Create a new coldkey only (does not create a hotkey)."""
    wallet = Wallet(name=name, path=path)
    wallet.create_new_coldkey(
        n_words=n_words,
        use_password=use_password,
        overwrite=overwrite,
        crypto_type=crypto_type,
    )
    return wallet


def new_hotkey(
    name: str = "default",
    hotkey: str = "default",
    path: str = DEFAULT_WALLET_PATH,
    *,
    n_words: int = 12,
    overwrite: bool = False,
    crypto_type: int = DEFAULT_CRYPTO_TYPE,
) -> Wallet:
    """Create a new hotkey in an existing wallet."""
    wallet = Wallet(name=name, hotkey=hotkey, path=path)
    wallet.create_new_hotkey(
        n_words=n_words,
        use_password=False,
        overwrite=overwrite,
        crypto_type=crypto_type,
    )
    return wallet


def regen_coldkey_pub(
    ss58: str,
    public_key_hex: str,
    name: str = "default",
    path: str = DEFAULT_WALLET_PATH,
    *,
    overwrite: bool = False,
    crypto_type: int = DEFAULT_CRYPTO_TYPE,
) -> Wallet:
    wallet = Wallet(name=name, path=path)
    wallet.regenerate_coldkeypub(
        ss58_address=ss58,
        public_key=public_key_hex,
        overwrite=overwrite,
        suppress=True,
        crypto_type=crypto_type,
    )
    return wallet


def regen_hotkey_pub(
    ss58: str,
    public_key_hex: str,
    name: str = "default",
    hotkey: str = "default",
    path: str = DEFAULT_WALLET_PATH,
    *,
    overwrite: bool = False,
    crypto_type: int = DEFAULT_CRYPTO_TYPE,
) -> Wallet:
    wallet = Wallet(name=name, hotkey=hotkey, path=path)
    wallet.regenerate_hotkeypub(
        ss58_address=ss58,
        public_key=public_key_hex,
        overwrite=overwrite,
        suppress=True,
        crypto_type=crypto_type,
    )
    return wallet


def encrypt_message(
    message: str,
    recipient_ss58: str,
) -> dict[str, str]:
    """Encrypt a message for a recipient's ED25519 public key.

    Only ed25519 recipient keys work: encryption converts the public key to
    X25519, and sr25519 keys (the wallet default) are not valid ed25519 points.
    """
    try:
        ciphertext = Keypair.encrypt_for(recipient_ss58, message.encode("utf-8"))
    except ValueError as error:
        raise ValueError(
            f"cannot encrypt for {recipient_ss58}: the recipient public key is not "
            f"a valid ed25519 key ({error}); it is most likely sr25519, the wallet "
            "default scheme"
        ) from error
    return {"ciphertext": "0x" + ciphertext.hex(), "recipient": recipient_ss58}


def decrypt_message(
    ciphertext_hex: str,
    *,
    name: str = "default",
    hotkey: str = "default",
    path: str = DEFAULT_WALLET_PATH,
    use_hotkey: bool = False,
    password: str | None = None,
    password_file: str | None = None,
    macos_prompt: bool = False,
    keychain: bool = False,
) -> str:
    """Decrypt a 0x-hex ciphertext with the wallet's coldkey or hotkey."""
    wallet = Wallet(name=name, hotkey=hotkey, path=path)
    signer = "hotkey" if use_hotkey else "coldkey"
    keypair = signing_keypair(
        wallet,
        signer,
        password=password,
        password_file=password_file,
        macos_prompt=macos_prompt,
        keychain=keychain,
    )
    raw = bytes.fromhex(ciphertext_hex.removeprefix("0x"))
    return keypair.decrypt(raw).decode()


def _is_hotkey_companion_file(name: str) -> bool:
    """Lock artifacts and public companion files that live beside hotkeys but
    are not hotkeys themselves (``default.lock``, ``defaultpub.txt``)."""
    return name.endswith(".lock") or name.endswith("pub.txt")


def list_wallets(path: str = DEFAULT_WALLET_PATH) -> dict[str, list[str]]:
    """Map coldkey wallet name -> list of its hotkey names on disk."""
    root = Path(path).expanduser()
    if not root.is_dir():
        return {}
    out: dict[str, list[str]] = {}
    for coldkey_dir in sorted(root.iterdir()):
        if not coldkey_dir.is_dir():
            continue
        hotkeys_dir = coldkey_dir / "hotkeys"
        hotkeys = (
            sorted(
                hk.name
                for hk in hotkeys_dir.iterdir()
                if hk.is_file() and not _is_hotkey_companion_file(hk.name)
            )
            if hotkeys_dir.is_dir()
            else []
        )
        out[coldkey_dir.name] = hotkeys
    return out


@dataclass
class HotkeyInfo:
    name: str
    ss58: str | None = None
    crypto_type: int | None = None


@dataclass
class ColdkeyInfo:
    name: str
    ss58: str | None = None
    crypto_type: int | None = None
    hotkeys: list[HotkeyInfo] = field(default_factory=list)


def _read_keyfile(key_file: Path) -> tuple[str | None, int | None]:
    """Read ss58 address and crypto type from a keyfile without unlocking it."""
    try:
        data = json.loads(key_file.read_text())
    except (OSError, ValueError):
        return None, None
    address = data.get("ss58Address")
    ss58 = address if isinstance(address, str) else None
    raw_type = data.get("cryptoType", data.get("crypto_type"))
    crypto_type = raw_type if isinstance(raw_type, int) else None
    return ss58, crypto_type


def _read_ss58(key_file: Path) -> str | None:
    """Read the ss58 address from a keyfile without unlocking it."""
    ss58, _ = _read_keyfile(key_file)
    return ss58


def list_wallets_detailed(path: str = DEFAULT_WALLET_PATH) -> list[ColdkeyInfo]:
    """List coldkeys with their public ss58 address and hotkeys (name + ss58)."""
    root = Path(path).expanduser()
    if not root.is_dir():
        return []
    coldkeys: list[ColdkeyInfo] = []
    for coldkey_dir in sorted(root.iterdir()):
        if not coldkey_dir.is_dir():
            continue
        ss58, crypto_type = _read_keyfile(coldkey_dir / "coldkeypub.txt")
        info = ColdkeyInfo(name=coldkey_dir.name, ss58=ss58, crypto_type=crypto_type)
        hotkeys_dir = coldkey_dir / "hotkeys"
        if hotkeys_dir.is_dir():
            for hk in sorted(hotkeys_dir.iterdir(), key=lambda p: p.name):
                if hk.is_file() and not _is_hotkey_companion_file(hk.name):
                    hk_ss58, hk_crypto = _read_keyfile(hk)
                    info.hotkeys.append(
                        HotkeyInfo(name=hk.name, ss58=hk_ss58, crypto_type=hk_crypto)
                    )
        coldkeys.append(info)
    return coldkeys
