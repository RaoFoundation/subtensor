"""Open the extension bridge page in a chosen browser."""

from __future__ import annotations

import os
import shutil
import subprocess
import sys
import webbrowser
from typing import Optional

_KNOWN_BROWSERS = {
    "firefox": "Firefox",
    "ff": "Firefox",
    "chrome": "Google Chrome",
    "google-chrome": "Google Chrome",
    "chromium": "Chromium",
    "brave": "Brave Browser",
    "edge": "Microsoft Edge",
    "safari": "Safari",
}


def resolve_extension_browser(explicit: Optional[str] = None) -> Optional[str]:
    """CLI flag > env > config is applied by the caller; here flag/env only."""
    return explicit or os.getenv("BT_EXTENSION_BROWSER")


def open_bridge_page(url: str, browser: Optional[str] = None) -> None:
    """Open ``url`` in the requested browser, or the system default."""
    choice = resolve_extension_browser(browser)
    if not choice or choice.strip().lower() in ("default", "system"):
        webbrowser.open(url)
        return

    normalized = choice.strip().lower()
    if sys.platform == "darwin":
        app_name = _KNOWN_BROWSERS.get(normalized, choice)
        subprocess.run(["open", "-a", app_name, url], check=False)
        return

    for command in _linux_commands(normalized, choice):
        path = shutil.which(command)
        if path:
            subprocess.Popen(
                [path, url],
                start_new_session=True,
                stdout=subprocess.DEVNULL,
                stderr=subprocess.DEVNULL,
            )
            return

    webbrowser.open(url)


def _linux_commands(normalized: str, raw: str) -> list[str]:
    if normalized in ("firefox", "ff"):
        return ["firefox", "firefox-esr"]
    if normalized in ("chrome", "google-chrome"):
        return ["google-chrome", "google-chrome-stable", "chromium", "chromium-browser"]
    if normalized == "brave":
        return ["brave-browser", "brave"]
    if normalized == "edge":
        return ["microsoft-edge", "microsoft-edge-stable"]
    return [raw]
