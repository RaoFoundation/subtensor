"""Typed outcomes and errors.

Every write returns an ``ExtrinsicResult`` rather than a ``(bool, str)`` tuple.
Errors carry a machine-readable :class:`ErrorCode` and a short remediation hint so
an agent can branch on the failure and know what to try next, instead of parsing
a human sentence.
"""

from __future__ import annotations

import re
from dataclasses import dataclass, field
from typing import Any, Optional

from ._generated.errors import ERRORS
from .error_descriptions import DESCRIPTIONS as _DESCRIPTIONS
from .error_map import CUSTOM_TRANSACTION_ERRORS as _CUSTOM_TRANSACTION_ERRORS
from .error_map import DISPATCH_ERRORS as _DISPATCH_ERRORS
from .error_map import INVALID_TRANSACTION_ERRORS as _INVALID_TRANSACTION_ERRORS
from .error_map import NAME_TO_CODE as _NAME_TO_CODE
from .error_map import ErrorCode
from .settings import chain_error_docs_url, error_docs_url

# Matches classic ``Custom error: N``, newer
# ``Invalid transaction with custom error: N``, and structured
# ``{'Custom': N}`` RPC data embeddings.
_CUSTOM_ERROR_RE = re.compile(
    r"(?:custom error:\s*|['\"]Custom['\"]\s*:\s*)(\d+)",
    re.IGNORECASE,
)
_WATCH_STATUS_RE = re.compile(
    r"\bextrinsic\s+(usurped|retracted|finalitytimeout|dropped|invalid)\b",
    re.IGNORECASE,
)
_CATALOG_DOCS_BY_NAME = {info.name: info.docs for info in ERRORS.values() if info.docs}
_WATCH_STATUS_NAMES = {
    "usurped": "Usurped",
    "retracted": "Retracted",
    "finalitytimeout": "FinalityTimeout",
    "dropped": "Dropped",
    "invalid": "Invalid",
}


class BittensorError(Exception):
    """Base class for all SDK errors."""


class ConnectionNotReady(BittensorError):
    """Raised when the client is used before its connection is opened."""


class RpcConnectionError(BittensorError):
    """Raised when the public RPC transport cannot complete an operation."""


class RpcPolicyError(RpcConnectionError):
    """Raised when an RPC endpoint explicitly refuses traffic under a policy."""

    def __init__(
        self,
        message: str,
        *,
        status_code: int | None = None,
        retry_after: str | None = None,
        policy: str | None = None,
        reason: str | None = None,
    ):
        super().__init__(message)
        self.status_code = status_code
        self.retry_after = retry_after
        self.policy = policy
        self.reason = reason


# Rustc help conventions: lowercase, no trailing punctuation, states the fix.
# Commands are backticked so the CLI renders them in the command style.
REMEDIATION: dict[ErrorCode, str] = {
    ErrorCode.INSUFFICIENT_BALANCE: (
        "fund the signing account or reduce the amount; check with `btcli wallet balance`"
    ),
    ErrorCode.INSUFFICIENT_LIQUIDITY: (
        "the pool cannot absorb this trade; reduce the amount or split it into smaller steps"
    ),
    ErrorCode.RATE_LIMITED: "wait for the rate-limit window to pass, then retry",
    ErrorCode.NOT_REGISTERED: (
        "register the hotkey on this subnet first with `btcli subnets register`"
    ),
    ErrorCode.NOT_AUTHORIZED: (
        "sign with the key or origin that owns the target object, then retry"
    ),
    ErrorCode.ALREADY_EXISTS: (
        "the object or state already exists; treat the goal as met or pick a different target"
    ),
    ErrorCode.NOT_FOUND: "the referenced object is not on-chain; check the identifier",
    ErrorCode.SUBNET_NOT_EXISTS: ("use an existing netuid; `btcli subnets list` shows valid ones"),
    ErrorCode.SUBTOKEN_DISABLED: "the subnet is not active yet; wait for start_call",
    ErrorCode.DISABLED: "this call or feature is switched off on this network",
    ErrorCode.TOO_EARLY: "the required window has not opened yet; wait some blocks and retry",
    ErrorCode.EXPIRED: "the window has closed; restart the flow with fresh state",
    ErrorCode.LIMIT_EXCEEDED: (
        "a chain-side capacity limit was hit; reduce the size or count of the request"
    ),
    ErrorCode.UNIT_MISMATCH: "match the Balance netuid to the operation's currency",
    ErrorCode.INVALID_ARGUMENT: "check the argument values against the operation schema",
    ErrorCode.POLICY_VIOLATION: "the action exceeds a configured safety policy",
    ErrorCode.INTERNAL: (
        "a chain-side invariant failed; nothing to fix client-side — report it if it persists"
    ),
    ErrorCode.UNKNOWN: "inspect the message for details",
}

# Long-form explanations, one per code (the rustc --explain convention): the
# inline diagnostic stays terse; `btcli explain <code>` prints these.
# Same style rules as the short messages: lowercase, plain English.
EXPLANATIONS: dict[ErrorCode, str] = {
    ErrorCode.INSUFFICIENT_BALANCE: (
        "the signing account cannot cover the requested amount, fee, or stake requirement.\n"
        "\n"
        "this covers several chain-side variants: free TAO too low for a transfer or fee, "
        "an unstake/move larger than free stake (or locked by conviction/collateral), a "
        "withdrawal that would drop below the existential deposit, or a hotkey below the "
        "stake floor needed to set weights / children.\n"
        "\n"
        "check with `btcli wallet balance` and `btcli stake list`, then reduce the amount, "
        "fund the coldkey, unlock/wait out locks, or stake more to the hotkey — the exact "
        "error name in the diagnostic says which."
    ),
    ErrorCode.INSUFFICIENT_LIQUIDITY: (
        "the pool on the other side of the operation cannot absorb it: the swap "
        "reserves are too low or too imbalanced, the price would move past the "
        "configured limit, or the slippage exceeds the allowed tolerance.\n"
        "\n"
        "unlike insufficient_balance this is not about your account — it is about "
        "the subnet's liquidity. reduce the amount, split it into smaller steps "
        "across blocks, or trade on a subnet with deeper reserves.\n"
        "\n"
        "stake trades are slippage-protected by default with a 5% tolerance "
        "(SlippageTooHigh): raise it with `--rate-tolerance`, or disable the "
        "protection with `--no-slippage-protection` (slippage_protection=False "
        "in the SDK) to trade through the price move."
    ),
    ErrorCode.RATE_LIMITED: (
        "the chain enforces per-key rate limits on certain calls (registration, weight "
        "setting, stake movement, child hotkey changes) to keep the network stable.\n"
        "\n"
        "the limit is measured in blocks since the same key last performed the same kind "
        "of call, so retrying immediately will fail with the same error. wait for the "
        "window to pass (typically a few blocks to a few hundred blocks depending on the "
        "call), then retry."
    ),
    ErrorCode.NOT_REGISTERED: (
        "the hotkey is not registered where the call expects it to be: either not on the "
        "specific subnet, or not anywhere on the network.\n"
        "\n"
        "many calls (serving an axon, setting weights, some stake operations) require the "
        "hotkey to hold a UID on the target subnet. register it first with "
        "`btcli subnets register --netuid N`, or check where it is registered with "
        "`btcli query netuids-for-hotkey` / `btcli query uid --netuid N`."
    ),
    ErrorCode.NOT_AUTHORIZED: (
        "the signing key is not allowed to perform this call, or the account is "
        "blocked from acting: the coldkey is not associated with the hotkey, the "
        "caller is not the subnet owner, a proxy lacks permission, the call needs "
        "root/sudo, the hotkey lacks a validator permit, or a disputed coldkey "
        "swap has frozen the account.\n"
        "\n"
        "check the exact error name. ownership problems need the owning key; "
        "validator-permit failures need more stake weight (not a different signer); "
        "disputed swaps stay frozen until root resolves them."
    ),
    ErrorCode.ALREADY_EXISTS: (
        "the state the call would create already exists: the hotkey is already "
        "registered on the subnet, an announcement or commit is already pending, "
        "the identity/symbol is already taken, or the approval was already given.\n"
        "\n"
        "this is often good news — the goal may already be met, so check the "
        "current state before retrying. if you meant to create something new, "
        "pick a different identifier or wait for the pending object to clear."
    ),
    ErrorCode.NOT_FOUND: (
        "the call references an object that is not on-chain: a weights commit, "
        "lease, multisig operation, scheduled call, announcement, or contribution "
        "that was never created, was already consumed, or has been removed.\n"
        "\n"
        "check the identifier and the current chain state. if the flow has a "
        "create step (commit before reveal, announce before execute), make sure "
        "it completed on the network you are connected to (check `--network`)."
    ),
    ErrorCode.SUBNET_NOT_EXISTS: (
        "the netuid does not name an existing subnet on this network.\n"
        "\n"
        "netuids are not stable across networks: a subnet that exists on finney may not "
        "exist on testnet. `btcli subnets list` shows the valid netuids for the "
        "network you are connected to (check `--network`)."
    ),
    ErrorCode.SUBTOKEN_DISABLED: (
        "the subnet exists but its token operations are not enabled yet.\n"
        "\n"
        "a newly registered subnet starts inactive: staking, unstaking, and other alpha "
        "operations are rejected until the subnet owner activates it with "
        "`btcli sudo start`. there is nothing to fix on your side; wait for the "
        "subnet to become active (`btcli sudo check-start`)."
    ),
    ErrorCode.DISABLED: (
        "the call or feature is blocked by a chain/subnet switch or pending state: "
        "registration is closed, commit-reveal requires the other weights path, "
        "stake transfers are toggled off on a subnet, a coldkey has a pending swap "
        "announcement, the extrinsic is deprecated, or an admin disabled it.\n"
        "\n"
        "this is not a bad argument. use the supported alternative (the error name "
        "usually points at it), finish or clear a pending coldkey swap "
        "(`btcli wallet swap-check`), or wait for the feature to be enabled."
    ),
    ErrorCode.TOO_EARLY: (
        "the call arrived before its window opened: a reveal before the reveal "
        "period, a coldkey swap before its delay elapsed, `btcli sudo start` before "
        "enough blocks passed, or a finalization before the period ended.\n"
        "\n"
        "unlike rate_limited this is not about calling too often — the flow has a "
        "built-in waiting period that has not elapsed yet. wait the required "
        "blocks and retry; the chain state (announcement block, commit block) "
        "tells you how long is left."
    ),
    ErrorCode.EXPIRED: (
        "the call arrived after its window closed: a weights commit past its "
        "reveal period, an order past its expiry, or a contribution after the "
        "period ended.\n"
        "\n"
        "the expired object cannot be revived. restart the flow with fresh state — "
        "make a new commit, place a new order, or move on."
    ),
    ErrorCode.LIMIT_EXCEEDED: (
        "a chain-side capacity limit was hit: too many children, weights above the "
        "maximum, no free UID slots on the subnet, the subnet limit reached, or a "
        "batch/queue at its configured maximum.\n"
        "\n"
        "these are hard caps rather than timing windows, so retrying unchanged "
        "will fail again. reduce the size or count of the request, or free "
        "capacity first (e.g. remove a child before adding another)."
    ),
    ErrorCode.UNIT_MISMATCH: (
        "two amounts denominated in different currencies were combined.\n"
        "\n"
        "every Balance carries the netuid whose currency it denominates: netuid 0 is TAO, "
        "any other netuid is that subnet's alpha. arithmetic between different netuids "
        "raises instead of silently mixing currencies, because 1 alpha on one subnet is "
        "not worth 1 alpha on another.\n"
        "\n"
        "construct the amount in the operation's own currency: Balance.from_tao for TAO, "
        "Balance.from_alpha(amount, netuid) for a subnet's alpha."
    ),
    ErrorCode.INVALID_ARGUMENT: (
        "an argument was rejected before or at submission: a malformed address, an "
        "out-of-range value, a bad signature, or an extrinsic the node refused to accept "
        "into its pool.\n"
        "\n"
        "check the argument values against the operation's schema (`btcli tools` "
        "prints the machine-readable catalog; `--help` on any command shows its "
        "parameters)."
    ),
    ErrorCode.POLICY_VIOLATION: (
        "the action was blocked by a policy: either a local SDK safety limit "
        "(spend caps, allowed operations) or an on-chain account/subnet policy "
        "(e.g. the destination rejects locked alpha).\n"
        "\n"
        "for local policies, raise or remove the limit in your configuration. for "
        "on-chain policies, change the recipient/settings or ask the counterparty "
        "to opt in — the error name says which."
    ),
    ErrorCode.INTERNAL: (
        "the chain hit an internal invariant it could not uphold: an arithmetic "
        "overflow, an unreachable branch, a storage inconsistency, or a failure in "
        "an underlying pallet.\n"
        "\n"
        "there is usually nothing to fix client-side. retry once in case it was "
        "state-dependent; if it persists, report it upstream with the exact chain "
        "error name from the diagnostic."
    ),
    ErrorCode.UNKNOWN: (
        "the error did not match any known chain error name or message pattern.\n"
        "\n"
        "the original message is preserved in the diagnostic; read it for details. if it "
        "names a chain error that should be classified, the mapping lives in "
        "bittensor/error_map.py."
    ),
}

# Substring fallback, used only when there is no exact name (e.g. the transaction
# was rejected in the pool with a JSON-RPC message rather than a module error).
_SUBSTRING_FALLBACK: tuple[tuple[str, ErrorCode], ...] = (
    # "balance too low", not a bare "too low": pool rejections like
    # "Priority is too low" are not balance problems.
    ("balance too low", ErrorCode.INSUFFICIENT_BALANCE),
    ("inability to pay some fees", ErrorCode.INSUFFICIENT_BALANCE),
    ("insufficient", ErrorCode.INSUFFICIENT_BALANCE),
    ("not enough", ErrorCode.INSUFFICIENT_BALANCE),
    ("rate limit", ErrorCode.RATE_LIMITED),
    ("too fast", ErrorCode.RATE_LIMITED),
    ("priority is too low", ErrorCode.RATE_LIMITED),
    ("already imported", ErrorCode.ALREADY_EXISTS),
    ("bad signature", ErrorCode.INVALID_ARGUMENT),
    ("invalid transaction", ErrorCode.INVALID_ARGUMENT),
)

# Remediation overrides keyed by the exact chain error name; more specific
# than the per-code defaults above.
_NAME_HELP_OVERRIDES: dict[str, str] = {
    "SlippageTooHigh": (
        "the price moved past the slippage-protection limit (stake trades are "
        "protected by default with a 5% tolerance); retry, raise the tolerance "
        "(`--rate-tolerance 0.1` / rate_tolerance=0.1), or disable protection "
        "entirely (`--no-slippage-protection` / slippage_protection=False)"
    ),
    "ColdkeySwapAnnounced": (
        "this coldkey has a pending swap announcement that blocks transfers "
        "and most other calls; check with `btcli wallet swap-check`, then "
        "finish with `btcli wallet swap-coldkey` or clear via "
        "`btcli tx clear-coldkey-swap-announcement`"
    ),
    "ColdkeySwapDisputed": (
        "this coldkey's pending swap is under dispute, so all calls are frozen "
        "until root resolves it; check with `btcli wallet swap-check`"
    ),
    "ColdkeySwapTooEarly": (
        "the announcement delay has not elapsed yet; check remaining blocks with "
        "`btcli wallet swap-check` and retry `btcli wallet swap-coldkey` later"
    ),
    "ColdkeySwapAnnouncementNotFound": (
        "announce first with `btcli wallet announce-coldkey-swap`, then execute "
        "with `btcli wallet swap-coldkey` after the delay"
    ),
    "AnnouncedColdkeyHashDoesNotMatch": (
        "the new coldkey does not match the hash from `btcli wallet announce-coldkey-swap`; "
        "use the same new coldkey you announced"
    ),
    "Payment": (
        "fund the signing account with free TAO for fees (and tip); check with "
        "`btcli wallet balance`. MEV-shielded submissions cannot take the outer "
        "carrier fee in alpha — retry with `--no-mev-shield` if you only hold "
        "staked alpha"
    ),
    "Future": (
        "the nonce is too high; wait for pending extrinsics or resubmit with "
        "the current on-chain nonce"
    ),
    "Stale": "the nonce was already used; rebuild and resign with a fresh nonce",
    "AncientBirthBlock": "rebuild and resign the extrinsic against a recent block",
    "BadProof": (
        "the signature did not match the extrinsic; with `--signer extension`, retry "
        "the command — if it keeps failing, use a local wallet or file a bug"
    ),
    "ZeroMaxAmount": (
        "relax the price/slippage limit or wait for a better price so max_amount is non-zero"
    ),
    "AmountTooLow": (
        "raise the amount above the chain minimum stake (after fees/slippage); "
        "check with `btcli stake list` / subnet price"
    ),
    "StakeAmountTooLow": (
        "raise the stake amount above the minimum, or stake more to the hotkey if "
        "this was a weights call (`btcli stake list`)"
    ),
    "NotEnoughStakeToSetWeights": (
        "the hotkey's stake weight is below the weights floor — stake more to it "
        "(`btcli stake add` / `btcli stake list`); funding free TAO alone will not help"
    ),
    "NotEnoughStakeToWithdraw": (
        "lower the amount or use the hotkey that actually holds the stake "
        "(`btcli stake list`); lock target and stake hotkey can differ"
    ),
    "NotEnoughBalanceToStake": (
        "fund the coldkey or reduce the stake/registration amount; check with "
        "`btcli wallet balance`"
    ),
    "StakeUnavailable": (
        "only free stake can move — wait out conviction locks or drain miner "
        "collateral, then retry; check locked vs free with `btcli stake list`"
    ),
    "LockHotkeyMismatch": (
        "reuse the existing lock hotkey, or for transfers of locked alpha land "
        "on the receiver's lock hotkey via `btcli stake transfer "
        "--destination-hotkey <lock-hotkey>`; inspect with `btcli lock show` / "
        "`btcli stake list`"
    ),
    "NeuronNoValidatorPermit": (
        "this hotkey lacks a validator permit on the subnet; increase stake weight "
        "into the top-K or wait for the next epoch (`btcli subnets metagraph`) — "
        "resigning with another key will not help"
    ),
    "TransferDisallowed": (
        "stake transfers are disabled for this origin or destination subnet; ask "
        "the subnet owner to enable the transfer toggle, or use a different path"
    ),
    "RateLimitExceeded": (
        "wait for the rate-limit window (weights commit/set or network tx), then retry"
    ),
    "ExistentialDeposit": (
        "leave enough free TAO for the existential deposit, or use a full-balance "
        "transfer that allows reaping; check with `btcli wallet balance`"
    ),
    "Expendability": (
        "this move would reap the account below the existential deposit; leave dust "
        "or explicitly transfer-all"
    ),
    "ZeroBalanceAfterWithdrawn": (
        "the withdrawal would leave a zero/non-viable balance; leave the existential "
        "deposit or transfer the remainder intentionally"
    ),
    "CommitRevealEnabled": (
        "this subnet requires commit-reveal weights; use the commit/reveal path "
        "(btcli weights auto-selects it) instead of plain set_weights — check with "
        "`btcli sudo get --netuid N`"
    ),
    "CommitRevealDisabled": (
        "commit-reveal is off on this subnet; use plain set_weights — check with "
        "`btcli sudo get --netuid N`"
    ),
    "SubNetRegistrationDisabled": (
        "registration is closed on this subnet; check `network_registration_allowed` "
        "with `btcli sudo get --netuid N` or ask the owner"
    ),
    "SubtokenDisabled": (
        "the subnet is not started yet; wait for the owner to run `btcli sudo start` "
        "(`btcli sudo check-start`)"
    ),
    "RegistrationNotPermittedOnRootSubnet": (
        "do not register on netuid 0 with the normal register path; use the root "
        "register flow instead"
    ),
    "AccountRejectsLockedAlpha": (
        "the destination coldkey rejects locked alpha; they must opt in, or send "
        "unlocked stake only"
    ),
    "BadRequest": (
        "the node rejected the extrinsic before inclusion (unclassified validity "
        "failure); retry once, and if it persists inspect the exact call / report it"
    ),
    "Dropped": "the extrinsic was dropped from the pool; resign and resubmit",
    "Usurped": (
        "another extrinsic with the same nonce replaced this one; check pending "
        "submissions and resubmit with a fresh nonce if needed"
    ),
    "Invalid": (
        "the pool marked the extrinsic invalid after submission; resign and resubmit "
        "with current nonce/era"
    ),
    "Retracted": "the extrinsic was retracted; resign and resubmit",
    "FinalityTimeout": (
        "finality timed out while watching the extrinsic; check whether it landed "
        "on-chain before resubmitting"
    ),
    "ExtrinsicDecodeFailed": (
        "the shielded extrinsic could not be decoded on inclusion; resubmit with "
        "`--mev-protection` / a fresh shield encrypt"
    ),
    "ExtrinsicExpired": (
        "the shielded extrinsic expired in the queue; resubmit with MEV protection"
    ),
    "DecryptionFailed": (
        "the block author could not decrypt the shielded extrinsic; resubmit with "
        "a fresh MEV-protected submission"
    ),
}

_HELP_OVERRIDES: tuple[tuple[str, str], ...] = (
    (
        "invalid transaction",
        "the node rejected the extrinsic before inclusion; check the detail line above",
    ),
)


def classify_error(text: str, name: Optional[str] = None) -> ErrorCode:
    """Map a chain error to a semantic code: exact by name (rate-limit by suffix),
    falling back to substring matching only when no name is available."""
    if name:
        if name in _NAME_TO_CODE:
            return _NAME_TO_CODE[name]
        if name in _DISPATCH_ERRORS:
            return _DISPATCH_ERRORS[name][0]
        if name.endswith("RateLimitExceeded") or "RateLimit" in name:
            return ErrorCode.RATE_LIMITED
        return ErrorCode.UNKNOWN
    haystack = (text or "").lower()
    for needle, code in _SUBSTRING_FALLBACK:
        if needle in haystack:
            return code
    return ErrorCode.UNKNOWN


class ChainError(BittensorError):
    """A call was rejected or failed on-chain."""

    def __init__(
        self,
        message: str,
        name: Optional[str] = None,
        code: Optional[ErrorCode] = None,
    ):
        super().__init__(message)
        self.message = message
        self.name = name
        self.code = code or classify_error(message, name)

    @property
    def remediation(self) -> str:
        if self.name in _NAME_HELP_OVERRIDES:
            return _NAME_HELP_OVERRIDES[self.name]
        haystack = self.message.lower()
        for needle, help_text in _HELP_OVERRIDES:
            if needle in haystack:
                return help_text
        return REMEDIATION[self.code]

    @property
    def description(self) -> Optional[str]:
        """What triggered this exact chain error and where to check, from the
        per-name table in :mod:`bittensor.error_descriptions` (split by pallet),
        or the dispatch-level table for non-module errors (``BadOrigin``,
        ``TokenError`` variants). ``None`` when the failure carried no error
        name (e.g. a pool rejection)."""
        if not self.name:
            return None
        if self.name in _DESCRIPTIONS:
            return _DESCRIPTIONS[self.name]
        if self.name in _DISPATCH_ERRORS:
            return _DISPATCH_ERRORS[self.name][1]
        return None

    def to_dict(self) -> dict[str, Any]:
        payload = {
            "message": self.message,
            "name": self.name,
            "code": self.code.value,
            "remediation": self.remediation,
        }
        if self.description:
            payload["description"] = self.description
        if docs_url := self.docs_url:
            payload["docs_url"] = docs_url
        return payload

    @property
    def docs_url(self) -> Optional[str]:
        """The most specific docs page for this failure: the exact chain
        error's page when the name is classified, the semantic code's page
        otherwise, None when nothing more specific than unknown is known."""
        if self.name and self.name in _NAME_TO_CODE:
            return chain_error_docs_url(self.name)
        if self.code is not ErrorCode.UNKNOWN:
            return error_docs_url(self.code.value)
        return None


def _message_for_named_error(name: str) -> str:
    """Short diagnostic text for a known chain / custom-transaction error name."""
    if docs := _CATALOG_DOCS_BY_NAME.get(name):
        return docs
    if name in _DISPATCH_ERRORS:
        return _DISPATCH_ERRORS[name][1]
    return name


def _custom_code_from_payload(payload: Any) -> Optional[int]:
    """Pull ``InvalidTransaction::Custom(N)`` out of a JSON-RPC error payload."""
    if not isinstance(payload, dict):
        return None
    err = payload.get("error")
    if not isinstance(err, dict):
        return None
    data = err.get("data")
    if isinstance(data, dict) and "Custom" in data:
        try:
            return int(data["Custom"])
        except (TypeError, ValueError):
            return None
    if isinstance(data, int):
        return data
    if isinstance(data, str):
        match = _CUSTOM_ERROR_RE.search(data)
        if match:
            return int(match.group(1))
    return None


def _named_pool_rejection(message: str, *, payload: Any = None) -> Optional[str]:
    """Resolve a pool-rejection RPC string/payload to a known error name, if any."""
    code = _custom_code_from_payload(payload)
    if code is None:
        match = _CUSTOM_ERROR_RE.search(message)
        if match:
            code = int(match.group(1))
    if code is not None:
        name = _CUSTOM_TRANSACTION_ERRORS.get(code)
        if name:
            return name
    watch = _WATCH_STATUS_RE.search(message)
    if watch:
        return _WATCH_STATUS_NAMES[watch.group(1).lower()]
    haystack = message.lower()
    for needle, name in _INVALID_TRANSACTION_ERRORS:
        if needle in haystack:
            return name
    return None


def chain_error_from_substrate_request(error: Exception) -> ChainError:
    """Build a ChainError from a transport-layer request failure.

    Pool rejections often arrive as ``InvalidTransaction::Custom(N)`` with the
    opaque text ``Custom error: N`` / ``{'Custom': N}``, a standard
    ``InvalidTransaction`` Display string, or a watch-status fatal
    (``dropped`` / ``usurped`` / …). Resolve those against the SDK maps so the
    CLI can show the real name, description, and remediation.
    """
    message = str(error)
    payload = getattr(error, "payload", None)
    name = _named_pool_rejection(message, payload=payload)
    if name:
        return ChainError(_message_for_named_error(name), name)
    return ChainError(message, code=classify_error(message))


def chain_error_from_dispatch(err: Any) -> ChainError:
    """Build a ChainError from a decoded ``DispatchError`` payload.

    Used for errors that arrive inside event attributes (e.g. the inner result of
    a ``Sudo.Sudid``, ``Proxy.ProxyExecuted``, or ``Multisig.MultisigExecuted``
    event) rather than as a failed extrinsic. Module errors are resolved to
    their exact name/docs via the generated catalog; Token/BadOrigin/Arithmetic
    and friends use :data:`DISPATCH_ERRORS`.
    """
    if isinstance(err, dict) and "Module" in err:
        module = err["Module"] or {}
        pallet_index = int(module.get("index", -1))
        raw = module.get("error")
        if isinstance(raw, str):
            error_index = int(raw.removeprefix("0x")[:2] or "0", 16)
        elif isinstance(raw, (list, tuple)) and raw:
            error_index = int(raw[0])
        else:
            error_index = int(raw or 0)
        info = ERRORS.get((pallet_index, error_index))
        if info:
            return ChainError(info.docs or info.name, info.name)
        return ChainError(f"module error {pallet_index}/{error_index}")
    if isinstance(err, dict):
        for wrapper in (
            "BadOrigin",
            "CannotLookup",
            "Other",
            "Token",
            "Arithmetic",
            "Transactional",
            "Exhausted",
            "Corruption",
            "Unavailable",
            "RootNotAllowed",
        ):
            if wrapper not in err:
                continue
            variant = err[wrapper]
            if (
                wrapper in ("Token", "Arithmetic", "Transactional")
                and isinstance(variant, dict)
                and variant
            ):
                name = next(iter(variant))
            elif wrapper in ("BadOrigin", "CannotLookup", "Other"):
                name = wrapper
            else:
                name = wrapper
            return ChainError(_message_for_named_error(str(name)), str(name))
    return ChainError(str(err))


class PolicyError(BittensorError):
    """Raised when an intent violates the active safety policy at execute time."""

    def __init__(self, violations: list[str]):
        super().__init__("; ".join(violations))
        self.violations = violations
        self.code = ErrorCode.POLICY_VIOLATION


@dataclass(frozen=True)
class ExtrinsicResult:
    """Outcome of a submitted extrinsic.

    Immutable: derived outcomes (e.g. a proxied call whose inner dispatch
    failed) are built with ``dataclasses.replace``, never by editing a result
    in place — what a caller holds can't change under them.

    Attributes:
        success: Whether the extrinsic executed successfully on-chain.
        message: Human-readable status.
        block_hash: Hash of the including block, when available.
        extrinsic_id: On-chain identifier "block_number-index", when available.
        explorer_url: Public explorer page for the extrinsic, when the network
            is indexed by one (taomarketcap).
        fee: Fee actually paid, when available.
        events: Triggered events, when the call waited for inclusion.
        error: The underlying chain error, if it failed.
        data: Operation-specific extras (e.g. balances before/after).
    """

    success: bool
    message: str = ""
    block_hash: Optional[str] = None
    extrinsic_id: Optional[str] = None
    explorer_url: Optional[str] = None
    fee: Optional["Any"] = None
    events: list = field(default_factory=list)
    error: Optional[ChainError] = None
    data: dict[str, Any] = field(default_factory=dict)

    def raise_for_failure(self) -> "ExtrinsicResult":
        """Raise the underlying ChainError if the extrinsic did not succeed."""
        if not self.success:
            raise self.error or ChainError(self.message or "Extrinsic failed.")
        return self

    def to_dict(self) -> dict[str, Any]:
        payload: dict[str, Any] = {"success": self.success, "message": self.message}
        if self.block_hash:
            payload["block_hash"] = self.block_hash
        if self.extrinsic_id:
            payload["extrinsic_id"] = self.extrinsic_id
        if self.explorer_url:
            payload["explorer_url"] = self.explorer_url
        if self.fee is not None:
            payload["fee_tao"] = self.fee.tao
        if self.error is not None:
            payload["error"] = self.error.to_dict()
        if self.data:
            payload["data"] = self.data
        return payload

    def __bool__(self) -> bool:
        return self.success
