"""Bridge client auth token persisted for the local daemon process."""

from __future__ import annotations

import os
import stat
from pathlib import Path
from typing import Optional

DEFAULT_BRIDGE_TOKEN_PATH = Path.home() / ".bittensor" / "extension_bridge.token"


def bridge_token_path() -> Path:
    return Path(os.getenv("BITTENSOR_EXTENSION_BRIDGE_TOKEN") or DEFAULT_BRIDGE_TOKEN_PATH)


def write_bridge_token(token: str) -> None:
    path = bridge_token_path()
    path.parent.mkdir(parents=True, exist_ok=True)
    fd = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_TRUNC, stat.S_IRUSR | stat.S_IWUSR)
    with os.fdopen(fd, "w", encoding="utf-8") as handle:
        handle.write(token)


def read_bridge_token() -> Optional[str]:
    path = bridge_token_path()
    if not path.is_file():
        return None
    token = path.read_text(encoding="utf-8").strip()
    return token or None


def clear_bridge_token() -> None:
    bridge_token_path().unlink(missing_ok=True)
