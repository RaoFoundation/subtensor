"""Chain error descriptions declared (first) by the `Proxy` pallet."""

from __future__ import annotations

DESCRIPTIONS: dict[str, str] = {
    "AnnouncementDepositInvariantViolated": (
        "Internal invariant failure in `announce`: recomputing the announcement deposit "
        "returned nothing after the pending announcements were updated. Inspect the caller's "
        "`Announcements` entry and the announcement deposit constants; this indicates a pallet "
        "bug rather than bad input."
    ),
    "Duplicate": (
        "This delegate is already registered as a proxy for the delegator with the same proxy "
        "type and delay. Check the delegator's `Proxies` entry before calling `add_proxy` with "
        "the same (delegate, proxy type, delay) tuple."
    ),
    "InvalidDerivedAccountId": (
        "Deriving the pure proxy account id from the provided entropy failed to decode into a "
        "valid account. Check the spawner, `proxy_type`, and `index` arguments used with "
        "`create_pure` (or the equivalent lookup when destroying one)."
    ),
    "NoPermission": (
        "The proxy pallet refused the action: the proxied call could escalate privileges, or "
        "the caller lacks authority over the pure proxy (e.g. `kill_pure` by a non-spawner). "
        "Check the proxy type's call filter and the original `create_pure` arguments."
    ),
    "NoSelfProxy": (
        "An account attempted to register itself as its own proxy, which is not allowed. Check "
        "the `delegate` argument to `add_proxy` and ensure it differs from the calling "
        "(delegator) account."
    ),
    "NotProxy": (
        "The sender is not registered as a proxy for the account it tried to act for. Check the "
        "`Proxies` entry of the `real` account and confirm the sender appears there with a "
        "proxy type and delay compatible with the call."
    ),
    "Unannounced": (
        "The proxied call was executed before its announcement matured, or no matching "
        "announcement exists at all. Check the proxy's `Announcements` entry for the call hash "
        "and the `delay` on the proxy registration; enough blocks must elapse between "
        "`announce` and `proxy_announced`."
    ),
    "Unproxyable": (
        "The attempted call is not permitted by the registered proxy type's call filter. Check "
        "which `proxy_type` the sender holds in the real account's `Proxies` entry and whether "
        "that type's `InstanceFilter` allows this specific call."
    ),
}
