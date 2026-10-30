"""Explicit account derivation modes; existing account identities remain stable."""

from ._generated.calls import Call
from .sp_core import is_stateless_pq


def validate_account_mode(account_mode: str) -> str:
    if account_mode not in ("legacy", "safe"):
        raise ValueError("account_mode must be 'legacy' or 'safe'")
    return account_mode


def multisig_call(function: str, *, account_mode: str = "legacy", **params) -> Call:
    suffix = "_safe" if validate_account_mode(account_mode) == "safe" else ""
    return Call("Multisig", function + suffix, params)


def warn_legacy_mldsa(account_mode: str, public_key, *, composite="multisig") -> None:
    if account_mode == "legacy" and is_stateless_pq(public_key):
        import warnings

        warnings.warn(
            f"Legacy {composite} derivation does not guarantee an off-curve account; "
            "use account_mode='safe' when creating a new composite account.",
            stacklevel=3,
        )
