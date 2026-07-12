"""Persistent CLI configuration, address book, and local proxy book."""

from __future__ import annotations

import contextlib
import json
import os
import time
from pathlib import Path
from typing import Any, Optional

DEFAULT_CONFIG_PATH = Path.home() / ".bittensor" / "btcli.json"
DEFAULT_PROXIES_PATH = Path.home() / ".bittensor" / "btcli_proxies.json"
DEFAULT_ADDRESSES_PATH = Path.home() / ".bittensor" / "btcli_addresses.json"
DEFAULT_MULTISIGS_PATH = Path.home() / ".bittensor" / "btcli_multisigs.json"
DEFAULT_MULTISIG_CACHE_PATH = Path.home() / ".bittensor" / "btcli_multisig_cache.json"
DEFAULT_SUBNET_NAMES_PATH = Path.home() / ".bittensor" / "btcli_subnet_names.json"
DEFAULT_TOKEN_SYMBOLS_PATH = Path.home() / ".bittensor" / "btcli_token_symbols.json"

# Cached subnet names older than this are refreshed on the next connected
# command. Names change rarely; a day of staleness is invisible in practice.
SUBNET_NAMES_TTL_SECONDS = 24 * 3600

# Token symbols are registered once per subnet and essentially never change,
# so the cache saves a full storage-map scan on every connect and only
# refreshes weekly (a new subnet's symbol shows up on the next refresh).
TOKEN_SYMBOLS_TTL_SECONDS = 7 * 24 * 3600

# Settable keys and the type each value coerces to.
SETTABLE: dict[str, type] = {
    "network": str,
    "wallet": str,
    "wallet_hotkey": str,
    "wallet_path": str,
    "json": bool,
    "quiet": bool,
    "signer_address": str,
    "extension_browser": str,
    # Comma-separated ws(s):// endpoint pools. Unset -> the network's public
    # defaults (settings.FALLBACK_ENDPOINTS / ARCHIVE_ENDPOINTS); "none" ->
    # pinned to the primary endpoint only.
    "fallback_endpoints": str,
    "archive_endpoints": str,
    # Never give up on connection failures; keep cycling the endpoint pool.
    "retry_forever": bool,
    # Default account for --proxy-for on every transaction command (ss58 or a
    # proxy-book / address-book / wallet name). Pass `--proxy-for self` to
    # bypass it for one command.
    "proxy_for": str,
    # MEV shielding for every transaction command: true shields all mutations,
    # false disables the built-in stake-command default. Unset -> each command's
    # own default (stake-trading ops shield, everything else doesn't). The
    # --mev-shield/--no-mev-shield flags override per invocation.
    "mev_shield": bool,
}


def config_path() -> Path:
    return Path(os.getenv("BTCLI_CONFIG") or DEFAULT_CONFIG_PATH)


def load() -> dict[str, Any]:
    """Return the stored config, ignoring unknown keys. Missing/corrupt file -> {}."""
    path = config_path()
    if not path.is_file():
        return {}
    try:
        data = json.loads(path.read_text())
    except json.JSONDecodeError:
        # Preserve the corrupt file instead of letting a later save() silently
        # overwrite whatever was in it.
        backup = path.with_name(f"{path.name}.corrupt.{int(time.time())}")
        with contextlib.suppress(OSError):
            path.rename(backup)
        return {}
    except OSError:
        return {}
    if not isinstance(data, dict):
        return {}
    return {k: v for k, v in data.items() if k in SETTABLE}


def save(data: dict[str, Any]) -> Path:
    # Atomic temp-write + rename so a crash mid-write can't truncate the
    # config. (Cross-process locking is out of scope; last writer wins.)
    path = config_path()
    path.parent.mkdir(parents=True, exist_ok=True)
    tmp = path.with_suffix(path.suffix + ".tmp")
    try:
        tmp.write_text(json.dumps(dict(sorted(data.items())), indent=2) + "\n")
        os.replace(tmp, path)
    except BaseException:
        tmp.unlink(missing_ok=True)
        raise
    return path


def _coerce(key: str, value: str) -> Any:
    typ = SETTABLE[key]
    if typ is bool:
        low = value.strip().lower()
        if low in ("true", "1", "yes", "on"):
            return True
        if low in ("false", "0", "no", "off"):
            return False
        raise ValueError(f"{key} expects a boolean (true/false), got {value!r}")
    return typ(value)


def _require_key(key: str) -> None:
    if key not in SETTABLE:
        raise ValueError(f"unknown config key {key!r}; settable keys: {sorted(SETTABLE)}")


def set_value(key: str, value: str) -> Any:
    """Set and persist one config value; returns the coerced value."""
    _require_key(key)
    data = load()
    data[key] = _coerce(key, value)
    save(data)
    return data[key]


def unset_value(key: str) -> bool:
    """Remove one config value; returns whether it had been set."""
    _require_key(key)
    data = load()
    existed = data.pop(key, None) is not None
    save(data)
    return existed


def get(key: str, default: Any = None) -> Any:
    """The stored value for ``key``, or ``default`` — used to seed CLI option defaults."""
    return load().get(key, default)


def proxies_path() -> Path:
    return Path(os.getenv("BTCLI_PROXIES_PATH") or DEFAULT_PROXIES_PATH)


def load_proxies() -> list[dict[str, Any]]:
    path = proxies_path()
    if not path.is_file():
        return []
    try:
        data = json.loads(path.read_text())
    except (json.JSONDecodeError, OSError):
        return []
    return data if isinstance(data, list) else []


def save_proxies(entries: list[dict[str, Any]]) -> Path:
    path = proxies_path()
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(entries, indent=2) + "\n")
    return path


def add_proxy(entry: dict[str, Any]) -> dict[str, Any]:
    entries = load_proxies()
    name = entry.get("name")
    if not name:
        raise ValueError("proxy entry requires a 'name' field")
    entries = [e for e in entries if e.get("name") != name]
    entries.append(entry)
    save_proxies(entries)
    return entry


def get_proxy(name: str) -> Optional[dict[str, Any]]:
    """The proxy-book entry named ``name``, or None."""
    for entry in load_proxies():
        if entry.get("name") == name:
            return entry
    return None


def remove_proxy(name: str) -> bool:
    entries = load_proxies()
    filtered = [e for e in entries if e.get("name") != name]
    existed = len(filtered) != len(entries)
    save_proxies(filtered)
    return existed


def update_proxy(name: str, updates: dict[str, Any]) -> Optional[dict[str, Any]]:
    entries = load_proxies()
    updated = None
    for index, entry in enumerate(entries):
        if entry.get("name") == name:
            entry = {**entry, **updates, "name": name}
            entries[index] = entry
            updated = entry
            break
    if updated is None:
        return None
    save_proxies(entries)
    return updated


def clear_proxies() -> int:
    count = len(load_proxies())
    save_proxies([])
    return count


# --- Named ss58 address book (reusable contacts for multisig, transfers, etc.) ---


def addresses_path() -> Path:
    return Path(os.getenv("BTCLI_ADDRESSES_PATH") or DEFAULT_ADDRESSES_PATH)


def load_addresses() -> list[dict[str, Any]]:
    path = addresses_path()
    if not path.is_file():
        return []
    try:
        data = json.loads(path.read_text())
    except (json.JSONDecodeError, OSError):
        return []
    return data if isinstance(data, list) else []


def save_addresses(entries: list[dict[str, Any]]) -> Path:
    path = addresses_path()
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(entries, indent=2) + "\n")
    return path


def get_address(name: str) -> Optional[str]:
    for entry in load_addresses():
        if entry.get("name") == name:
            address = entry.get("address")
            return address if isinstance(address, str) else None
    return None


def add_address(entry: dict[str, Any]) -> dict[str, Any]:
    name = entry.get("name")
    address = entry.get("address")
    if not name:
        raise ValueError("address entry requires a 'name' field")
    if not isinstance(address, str) or not address:
        raise ValueError("address entry requires an 'address' field")
    entries = [e for e in load_addresses() if e.get("name") != name]
    entries.append(entry)
    save_addresses(entries)
    return entry


def remove_address(name: str) -> bool:
    entries = load_addresses()
    filtered = [e for e in entries if e.get("name") != name]
    existed = len(filtered) != len(entries)
    save_addresses(filtered)
    return existed


# --- Multisig address book (named signer sets for repeated governance ops) ---


def multisigs_path() -> Path:
    return Path(os.getenv("BTCLI_MULTISIGS_PATH") or DEFAULT_MULTISIGS_PATH)


def load_multisigs() -> list[dict[str, Any]]:
    path = multisigs_path()
    if not path.is_file():
        return []
    try:
        data = json.loads(path.read_text())
    except (json.JSONDecodeError, OSError):
        return []
    return data if isinstance(data, list) else []


def save_multisigs(entries: list[dict[str, Any]]) -> Path:
    path = multisigs_path()
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(entries, indent=2) + "\n")
    return path


def get_multisig(name: str) -> Optional[dict[str, Any]]:
    for entry in load_multisigs():
        if entry.get("name") == name:
            return entry
    return None


def add_multisig(entry: dict[str, Any]) -> dict[str, Any]:
    name = entry.get("name")
    if not name:
        raise ValueError("multisig entry requires a 'name' field")
    threshold = entry.get("threshold")
    signatories = entry.get("signatories")
    if not isinstance(threshold, int) or threshold < 1:
        raise ValueError("multisig entry requires threshold >= 1")
    if not isinstance(signatories, list) or not signatories:
        raise ValueError("multisig entry requires a non-empty signatories list")
    entries = [e for e in load_multisigs() if e.get("name") != name]
    entries.append(entry)
    save_multisigs(entries)
    return entry


def remove_multisig(name: str) -> bool:
    entries = load_multisigs()
    filtered = [e for e in entries if e.get("name") != name]
    existed = len(filtered) != len(entries)
    save_multisigs(filtered)
    return existed


def multisig_cache_path() -> Path:
    return Path(os.getenv("BTCLI_MULTISIG_CACHE") or DEFAULT_MULTISIG_CACHE_PATH)


def load_multisig_cache() -> dict[str, Any]:
    path = multisig_cache_path()
    if not path.is_file():
        return {}
    try:
        data = json.loads(path.read_text())
    except (json.JSONDecodeError, OSError):
        return {}
    return data if isinstance(data, dict) else {}


def save_multisig_cache(data: dict[str, Any]) -> Path:
    path = multisig_cache_path()
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(dict(sorted(data.items())), indent=2) + "\n")
    return path


# --- Subnet-name cache (netuid -> registered name, per network) --------------
#
# Display-only: lets prompts and summaries label netuids ("4 (Targon)") without
# a chain round-trip. Refreshed opportunistically whenever a command connects.


def subnet_names_path() -> Path:
    return Path(os.getenv("BTCLI_SUBNET_NAMES_CACHE") or DEFAULT_SUBNET_NAMES_PATH)


def _load_subnet_names_file() -> dict[str, Any]:
    path = subnet_names_path()
    if not path.is_file():
        return {}
    try:
        data = json.loads(path.read_text())
    except (json.JSONDecodeError, OSError):
        return {}
    return data if isinstance(data, dict) else {}


def load_subnet_names(network: str) -> dict[int, str]:
    """Cached netuid -> subnet-name map for ``network`` ({} when never fetched)."""
    entry = _load_subnet_names_file().get(network)
    if not isinstance(entry, dict):
        return {}
    names = entry.get("names")
    if not isinstance(names, dict):
        return {}
    out: dict[int, str] = {}
    for key, value in names.items():
        try:
            out[int(key)] = str(value)
        except (TypeError, ValueError):
            continue
    return out


def subnet_names_fresh(network: str) -> bool:
    """Whether the cached names for ``network`` are within the refresh TTL."""
    entry = _load_subnet_names_file().get(network)
    if not isinstance(entry, dict):
        return False
    fetched_at = entry.get("fetched_at")
    return isinstance(fetched_at, (int, float)) and (
        time.time() - fetched_at < SUBNET_NAMES_TTL_SECONDS
    )


def save_subnet_names(network: str, names: dict[int, str]) -> Path:
    data = _load_subnet_names_file()
    data[network] = {
        "fetched_at": int(time.time()),
        "names": {str(netuid): name for netuid, name in sorted(names.items())},
    }
    path = subnet_names_path()
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(data, indent=2) + "\n")
    return path


# --- Token-symbol cache (netuid -> chain-registered symbol, per network) -----
#
# Display-only, like the subnet-name cache: lets Balance render each subnet's
# real symbol without a full storage-map scan on every connect.


def token_symbols_path() -> Path:
    return Path(os.getenv("BTCLI_TOKEN_SYMBOLS_CACHE") or DEFAULT_TOKEN_SYMBOLS_PATH)


def _load_token_symbols_file() -> dict[str, Any]:
    path = token_symbols_path()
    if not path.is_file():
        return {}
    try:
        data = json.loads(path.read_text())
    except (json.JSONDecodeError, OSError):
        return {}
    return data if isinstance(data, dict) else {}


def load_token_symbols(network: str) -> dict[int, str]:
    """Cached netuid -> token-symbol map for ``network`` ({} when never fetched)."""
    entry = _load_token_symbols_file().get(network)
    if not isinstance(entry, dict):
        return {}
    symbols = entry.get("symbols")
    if not isinstance(symbols, dict):
        return {}
    out: dict[int, str] = {}
    for key, value in symbols.items():
        try:
            out[int(key)] = str(value)
        except (TypeError, ValueError):
            continue
    return out


def token_symbols_fresh(network: str) -> bool:
    """Whether the cached symbols for ``network`` are within the refresh TTL."""
    entry = _load_token_symbols_file().get(network)
    if not isinstance(entry, dict):
        return False
    fetched_at = entry.get("fetched_at")
    return isinstance(fetched_at, (int, float)) and (
        time.time() - fetched_at < TOKEN_SYMBOLS_TTL_SECONDS
    )


def save_token_symbols(network: str, symbols: dict[int, str]) -> Path:
    data = _load_token_symbols_file()
    data[network] = {
        "fetched_at": int(time.time()),
        "symbols": {str(netuid): symbol for netuid, symbol in sorted(symbols.items())},
    }
    path = token_symbols_path()
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(data, indent=2) + "\n")
    return path


def get_multisig_cache(call_hash: str) -> Optional[dict[str, Any]]:
    key = call_hash if call_hash.startswith("0x") else "0x" + call_hash
    entry = load_multisig_cache().get(key)
    return entry if isinstance(entry, dict) else None


def save_multisig_cache_entry(call_hash: str, entry: dict[str, Any]) -> None:
    key = call_hash if call_hash.startswith("0x") else "0x" + call_hash
    data = load_multisig_cache()
    data[key] = entry
    save_multisig_cache(data)
