"""Coldkey-paid channel commitments and their owner-controlled burn prices."""

from __future__ import annotations

from dataclasses import dataclass, field
from decimal import Decimal, InvalidOperation
from typing import Any, Optional, cast

import bittensor_core as _core

from .._generated import calls
from ..balance import Balance
from ..hyperparams import to_raw
from ..result import BittensorError
from ._money import Money, Spend, tao_amount
from .base import BuiltCall, Intent
from .registry import register

CHANNEL_HELP = "Independent commitment channel; separate from the legacy commitment."
MECHID_HELP = "Mechanism whose commitment channel to use."
COMMIT_BURN_PARAMETERS = ("enabled", "min_burn", "max_burn", "half_life", "increase_mult")
COMMIT_BURN_METHODS = {
    "enabled": "sudo_set_commit_burn_enabled",
    "min_burn": "sudo_set_commit_burn_min",
    "max_burn": "sudo_set_commit_burn_max",
    "half_life": "sudo_set_commit_burn_half_life",
    "increase_mult": "sudo_set_commit_burn_increase_mult",
}


def _payload(data: str, encoding: str) -> bytes:
    if encoding == "hex":
        try:
            return bytes.fromhex(data.removeprefix("0x"))
        except ValueError:
            raise BittensorError(
                "Commitment data must be valid hexadecimal with encoding='hex'."
            ) from None
    if encoding != "utf8":
        raise BittensorError("Commitment encoding must be 'utf8' or 'hex'.")
    return data.encode()


def commitment_info(
    data: str, *, encoding: str = "utf8", reveal_round: Optional[int] = None
) -> tuple[dict, Optional[int]]:
    """Build the same raw/timelocked ``CommitmentInfo`` accepted by set_commitment.

    Returns ``(info, actual_reveal_round)``. An empty plaintext clears the
    commitment. Raw fields carry at most 512 bytes; timelocked data uses the
    pallet's inner TLE ciphertext, not the SDK's portable envelope.
    """
    payload = _payload(data, encoding)
    if reveal_round is not None:
        if not payload:
            raise BittensorError("A timelocked commitment cannot have an empty payload.")
        encrypted, round_number = _core.encrypt_at_round(payload, int(reveal_round))
        return {
            "fields": [
                {"TimelockEncrypted": {"encrypted": bytes(encrypted), "reveal_round": round_number}}
            ]
        }, int(round_number)
    if not payload:
        return {"fields": []}, None
    if len(payload) > 512:
        raise BittensorError(
            "A raw channel commitment is limited to 512 bytes; use timelock "
            "encryption or a content hash."
        )
    variant = f"Raw{len(payload)}" if len(payload) <= 128 else "BigRaw"
    return {"fields": [{variant: payload}]}, None


def _burn_rao(config: Any) -> int:
    burn = (config or {}).get("burn") or 0
    if isinstance(burn, dict):
        burn = burn.get("amount") or burn.get("value") or 0
    return int(burn)


def _enabled_value(value: Any) -> bool:
    if isinstance(value, bool):
        return value
    if isinstance(value, int) and value in (0, 1):
        return bool(value)
    if isinstance(value, str) and value.strip().lower() in ("true", "yes", "1"):
        return True
    if isinstance(value, str) and value.strip().lower() in ("false", "no", "0"):
        return False
    raise BittensorError("Commit-burn enabled must be true/false or 1/0.")


def _fixed_bits(value: Any) -> int:
    """Round decimals down to U64F64 precision; plain integers remain raw bits."""
    if isinstance(value, int):
        return to_raw("burn_increase_mult", value)
    try:
        decimal = Decimal(str(value))
        if not decimal.is_finite() or not 1 <= decimal <= 3:
            raise ValueError
        numerator, denominator = decimal.as_integer_ratio()
        return to_raw("burn_increase_mult", (numerator << 64) // denominator)
    except (InvalidOperation, ValueError):
        raise BittensorError(
            "increase_mult must be a decimal in 1..3 or an exact raw U64F64 integer."
        ) from None


@register
@dataclass
class SetChannelCommitment(Intent):
    """Publish a hotkey's independent channel commitment, paid by its coldkey.

    ``data`` is UTF-8 by default or hexadecimal with ``encoding='hex'``. An
    empty plaintext clears this channel's commitment. With ``reveal_round``,
    the payload is drand-timelock-encrypted and the chain reveals it at that
    round. The target is independent of the legacy commitment and other
    mechanisms/channels. The signing coldkey must own the registered hotkey.
    When this channel's burn is enabled, the current decayed TAO price is paid
    from the coldkey; the call fails rather than exceeding ``max_burn_tao``.
    That maximum bounds policy spending; fees are additional. With no enabled
    burn, the commitment is free apart from the normal transaction fee.
    """

    op = "set_channel_commitment"
    signer = "coldkey"
    verify = "channel_commitment"
    wraps = (("Commitments", "set_commitment_to"),)

    netuid: int = field(metadata={"help": "Subnet containing the target mechanism and hotkey."})
    mechid: int = field(default=0, metadata={"help": MECHID_HELP})
    channel: int = field(default=0, metadata={"help": CHANNEL_HELP})
    data: str = field(
        default="", metadata={"help": "Commitment bytes as UTF-8 or hex; empty plaintext clears."}
    )
    max_burn_tao: Money = field(
        default=0, metadata={"help": "Maximum current TAO burn accepted; never a tip or reserve."}
    )
    hotkey_ss58: Optional[str] = field(
        default=None,
        metadata={"help": "Hotkey publishing the commitment; defaults to the wallet's hotkey."},
    )
    encoding: str = field(default="utf8", metadata={"help": "Payload encoding: 'utf8' or 'hex'."})
    reveal_round: Optional[int] = field(
        default=None,
        metadata={"help": "Future drand round for a timelocked commitment; omit for plaintext."},
    )

    def __post_init__(self):
        self.max_burn_tao = tao_amount(self.max_burn_tao)
        if not 0 <= self.channel <= 255:
            raise BittensorError("channel must be in 0..255.")
        # Validate eagerly, before opening a wallet or contacting the chain.
        _payload(self.data, self.encoding)

    async def build(self, substrate, wallet: Any):
        hotkey = self.hotkey_address(wallet, self.hotkey_ss58)
        info, reveal_round = commitment_info(
            self.data, encoding=self.encoding, reveal_round=self.reveal_round
        )
        call = await substrate.compose(
            calls.Call(
                "Commitments",
                "set_commitment_to",
                {
                    "netuid": self.netuid,
                    "mecid": self.mechid,
                    "channel": self.channel,
                    "hotkey": hotkey,
                    "info": info,
                    "max_burn": cast(Balance, self.max_burn_tao).rao,
                },
            )
        )
        extras = {"reveal_round": reveal_round} if reveal_round is not None else {}
        return BuiltCall(call, extras)

    async def effects(self, substrate, signer_address: str) -> list[str]:
        config = await substrate.runtime_call(
            "SubnetRegistrationRuntimeApi",
            "get_commit_burn",
            [self.netuid, self.mechid, self.channel],
        )
        price = Balance.from_rao(_burn_rao(config)) if config else Balance.from_rao(0)
        lines = [self.summary(), f"maximum accepted burn: {self.max_burn_tao}"]
        if config is None:
            lines.append(
                "commitment burn is disabled or unconfigured; only the transaction fee applies"
            )
        else:
            lines.insert(
                1, f"current commitment burn: {price} (the execution-time price may differ)"
            )
        return lines

    async def warnings(self, substrate, signer_address: str) -> list[str]:
        return [
            "max_burn is only a price limit, not a tip: the runtime charges its current "
            "decayed price and never retains the unused difference"
        ]

    def spend(self) -> Spend:
        return cast(Balance, self.max_burn_tao)

    def summary(self) -> str:
        mode = "timelocked" if self.reveal_round is not None else "plaintext"
        return (
            f"publish {mode} commitment on netuid {self.netuid}, "
            f"mechanism {self.mechid}, channel {self.channel}"
        )


@register
@dataclass
class SetCommitBurn(Intent):
    """Set one subnet owner's commitment-channel burn parameter.

    ``name`` is one of ``enabled``, ``min_burn``, ``max_burn``, ``half_life``,
    or ``increase_mult``. Min/max values are TAO amounts, half-life is blocks,
    and increase multiplier is a decimal in 1..3 (or its raw U64F64 bits).
    Changes require the subnet-owner coldkey or root, respect the owner's
    rate-limit window, and are independent for each mechanism and channel.
    Disabling burn makes subsequent channel commitments free apart from fees;
    it does not refund earlier burns. Read the live decayed price and stored
    settings with the ``commit_burn`` read.
    """

    op = "set_commit_burn"
    signer = "coldkey"
    origin = "subnet_owner"
    verify = "commit_burn"
    wraps = tuple(("AdminUtils", method) for method in COMMIT_BURN_METHODS.values())

    netuid: int = field(metadata={"help": "Subnet to configure; the signer must be its owner."})
    name: str = field(
        metadata={
            "help": "Burn parameter: enabled, min_burn, max_burn, half_life, or increase_mult."
        }
    )
    value: bool | int | float | str = field(
        metadata={
            "help": (
                "Enabled flag, TAO min/max, block half-life, or 1..3 multiplier "
                "/ raw U64F64 integer."
            )
        }
    )
    mechid: int = field(default=0, metadata={"help": MECHID_HELP})
    channel: int = field(default=0, metadata={"help": CHANNEL_HELP})

    def __post_init__(self):
        if self.name not in COMMIT_BURN_METHODS:
            raise BittensorError(
                f"Unknown commit-burn parameter {self.name!r}; "
                f"choose from {COMMIT_BURN_PARAMETERS}."
            )
        if not 0 <= self.channel <= 255:
            raise BittensorError("channel must be in 0..255.")
        if self.name == "enabled":
            self.value = _enabled_value(self.value)
        elif self.name in ("min_burn", "max_burn"):
            self.value = cast(Balance, tao_amount(self.value)).rao
        elif self.name == "half_life":
            try:
                value = int(self.value)
            except (TypeError, ValueError):
                raise BittensorError("half_life must be a positive block count (u16).") from None
            if not 1 <= value <= 65535 or str(self.value).strip() != str(value):
                raise BittensorError("half_life must be an integer in 1..65535 blocks.")
            self.value = value
        else:
            self.value = _fixed_bits(self.value)

    async def build(self, substrate, wallet: Any):
        params = {"netuid": self.netuid, "mecid": self.mechid, "channel": self.channel}
        if self.name == "enabled":
            params["enabled"] = self.value
        else:
            params["value"] = {"bits": self.value} if self.name == "increase_mult" else self.value
        return await substrate.compose(
            calls.Call("AdminUtils", COMMIT_BURN_METHODS[self.name], params)
        )

    def summary(self) -> str:
        return (
            f"set commit burn {self.name}={self.value} on netuid {self.netuid}, "
            f"mechanism {self.mechid}, channel {self.channel}"
        )
