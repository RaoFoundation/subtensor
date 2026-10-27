"""Chain error descriptions declared (first) by the `Sudo` pallet."""

from __future__ import annotations

DESCRIPTIONS: dict[str, str] = {
    "RequireSudo": (
        "The call requires the sudo key but was signed by a different account. Compare the "
        "sender against the account stored in the Sudo pallet's `Key` storage item."
    ),
}
