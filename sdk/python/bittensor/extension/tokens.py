"""Bridge client auth token persisted for the local daemon process."""

from __future__ import annotations

import os
from pathlib import Path
from typing import Optional

DEFAULT_BRIDGE_TOKEN_PATH = Path.home() / ".bittensor" / "extension_bridge.token"


def bridge_token_path() -> Path:
    return Path(os.getenv("BITTENSOR_EXTENSION_BRIDGE_TOKEN") or DEFAULT_BRIDGE_TOKEN_PATH)


def write_bridge_token(token: str) -> None:
    path = bridge_token_path()
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(token, encoding="utf-8")
    os.chmod(path, 0o600)


def read_bridge_token() -> Optional[str]:
    path = bridge_token_path()
    if not path.is_file():
        return None
    token = path.read_text(encoding="utf-8").strip()
    return token or None


def clear_bridge_token() -> None:
    bridge_token_path().unlink(missing_ok=True)
