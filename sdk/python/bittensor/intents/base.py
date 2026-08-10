"""The ``Intent`` base: a mutation described as serializable data.

An intent is a small dataclass whose fields round-trip to and from a dict
without custom encoders: scalars are JSON-native, and money fields (declared
as :data:`Money`) normalize to an exact :class:`Balance` at construction and
serialize back as exact decimal strings — a float never carries an amount
internally. An intent knows how to build its chain call, summarize itself, and
expose a JSON schema. It does *not* know how to sign or submit — that is the
client's single execute choke point.
"""

from __future__ import annotations

import inspect
from abc import ABC, abstractmethod
from dataclasses import MISSING, asdict, dataclass, field, fields
from typing import TYPE_CHECKING, Any, ClassVar, Literal, Optional

from ..balance import Balance
from ..result import BittensorError
from ._money import Spend

if TYPE_CHECKING:
    from .._substrate import Substrate

Signer = Literal["coldkey", "hotkey"]

# Dispatch privilege the chain's call filter accepts before pallet logic runs.
# This is *not* a full application-role model: "signed" only means a signed
# extrinsic origin — the pallet may still require a registered hotkey, the
# stake beneficiary, the subnet owner, etc. "subnet_owner" / "root" are the
# cases the SDK elevates in docs and (for root) wraps with Sudo.sudo at
# execute time. Finer roles belong in each intent's docstring / field help.
Origin = Literal["signed", "subnet_owner", "root"]

# Field annotations are strings here (PEP 563 / `from __future__ import annotations`),
# so this maps by annotation name.
_JSON_TYPES: dict[str, str] = {
    "str": "string",
    "int": "integer",
    "float": "number",
    "bool": "boolean",
}

# A non-negative decimal number, e.g. "1" or "10000000.123456789".
_DECIMAL_PATTERN = r"^\d+(\.\d+)?$"


def _strip_optional(annotation: str) -> str:
    a = annotation.strip()
    if a.startswith("Optional[") and a.endswith("]"):
        a = a[len("Optional[") : -1].strip()
    if a.endswith("| None"):
        a = a[: -len("| None")].strip()
    return a


def is_money(annotation: str) -> bool:
    """Whether a (stringified) field annotation declares a money field."""
    return _strip_optional(annotation) == "Money"


def money_schema(*, allow_all: bool) -> dict[str, Any]:
    """JSON Schema for a money field: a number, or a decimal string for exact
    amounts (floats lose precision above ~9M TAO), plus ``"all"`` when the
    field can drain the whole position."""
    options: list[dict[str, Any]] = [
        {"type": "number"},
        {
            "type": "string",
            "pattern": _DECIMAL_PATTERN,
            "description": "decimal string, for amounts too large for an exact float",
        },
    ]
    if allow_all:
        options.append({"type": "string", "enum": ["all"]})
    return {"anyOf": options}


def _json_type(annotation: str) -> dict[str, Any]:
    """Map a (stringified) field annotation to a JSON Schema fragment.

    Handles ``Optional[X]`` / ``X | None`` and ``list``/``list[X]``. Raises on an
    unknown base type so a mistyped intent field fails loudly at registration
    rather than shipping a wrong schema to an agent.
    """
    a = annotation.strip()
    if a.startswith("Optional[") and a.endswith("]"):
        a = a[len("Optional[") : -1].strip()
    if a.endswith("| None"):
        a = a[: -len("| None")].strip()
    if a == "list" or a.startswith("list["):
        schema: dict[str, Any] = {"type": "array"}
        if a.startswith("list["):
            schema["items"] = _json_type(a[len("list[") : -1].strip())
        return schema
    if a == "dict" or a.startswith("dict["):
        return {"type": "object"}
    if a in _JSON_TYPES:
        return {"type": _JSON_TYPES[a]}
    if "|" in a:  # scalar union, e.g. `int | float | str`
        parts = [part.strip() for part in a.split("|") if part.strip() != "None"]
        if all(part in _JSON_TYPES for part in parts):
            return {"type": [_JSON_TYPES[part] for part in parts]}
    raise ValueError(f"Unsupported intent field annotation for JSON schema: {annotation!r}")


@dataclass
class Intent(ABC):
    """Base class for all state-changing operations.

    Subclasses set the class vars ``op`` (stable machine name) and ``signer``
    ("coldkey" or "hotkey"), declare their parameters as dataclass fields, and
    implement ``build`` and ``summary``.
    """

    op: ClassVar[str]
    signer: ClassVar[Signer] = "coldkey"
    # Extrinsic-origin privilege (not a full app-role model): "signed" (any
    # signed account — pallet checks may still require a hotkey, beneficiary,
    # owner, …), "subnet_owner" (owner coldkey of the touched netuid), or
    # "root" (chain sudo key / sudo multisig; Executor wraps Sudo.sudo).
    # Surfaced in the tool catalog, CLI help, pre-sign plan, and docs.
    origin: ClassVar[Origin] = "signed"
    # The registered read that confirms this intent's effect after inclusion
    # (e.g. "subnet_emission_enabled"). Optional, but every root intent should
    # declare one: a privileged write with no paired verify is a docs bug.
    verify: ClassVar[Optional[str]] = None
    # The chain call(s) this intent wraps, as (pallet, call_function) pairs. Used by
    # the codegen coverage gate to prove every chain call has a deliberate status.
    wraps: ClassVar[tuple[tuple[str, str], ...]] = ()
    # Money fields (annotated ``Money``) that also accept the sentinel string
    # "all" (the concrete amount is resolved from chain state at build time).
    # Drives the CLI's flag/prompt parsing and the JSON schema for agents.
    all_amount_fields: ClassVar[tuple[str, ...]] = ()
    # Whether the CLI submits this intent MEV-shielded by default (via
    # ``client.submit_shielded``). Set on intents that trade through subnet
    # pools, where a visible mempool entry can be front-run. Overridable per
    # invocation with --mev-shield/--no-mev-shield or the mev_shield config key
    # unless ``mev_shield_required`` is set.
    mev_shield_default: ClassVar[bool] = False
    # When True, unshielded submission is refused (CLI ``--no-mev-shield`` /
    # config false and SDK ``execute``). Use for collateral AMM buys where a
    # clear mempool entry can be sandwich-attacked into a worse fill.
    mev_shield_required: ClassVar[bool] = False

    @abstractmethod
    async def build(self, substrate: "Substrate", wallet: "Any"):
        """Compose and return the chain call for this intent.

        Returns the composed call, or a :class:`BuiltCall` when the intent needs to
        surface build-time data (e.g. a reveal round) into the execution result.

        ``wallet`` is provided because some operations need the signer's own keys
        as call parameters (e.g. the hotkey being registered). Intents that don't
        need it ignore it. It is never stored on the intent.
        """

    @abstractmethod
    def summary(self) -> str:
        """One-line human description of what this intent will do."""

    async def effects(self, substrate: "Substrate", signer_address: str) -> list[str]:
        """Predicted effects, shown by ``plan``. Defaults to the summary."""
        return [self.summary()]

    async def warnings(self, substrate: "Substrate", signer_address: str) -> list[str]:
        """Non-fatal cautions surfaced by ``plan`` (e.g. dust amounts)."""
        return []

    async def wrap_call(self, substrate: "Substrate", wallet: "Any", call: Any):
        """Apply an execution wrapper around an already composed semantic call.

        The executor calls this after root and proxy composition. Ordinary
        intents leave the call unchanged; execution adapters such as saved
        multisigs add their transport wrapper here.
        """
        return call

    # Key views ----------------------------------------------------------------
    #
    # Intents never touch private keys, but some need the *addresses* of the
    # wallet's keys as call parameters. These helpers are the only sanctioned
    # way to get them, because ``wallet`` may not be wallet-shaped at all: a
    # bare Signer (extension, hardware, remote) has no attached hotkey — it IS
    # one key. When this intent is signed by the role being asked for, the
    # signer's own identity is that key; otherwise the caller must say
    # explicitly, and the error explains that.

    def hotkey_address(self, wallet: Any, override: Optional[str] = None) -> str:
        """The hotkey ss58 address this intent references.

        ``override`` (the intent's ``hotkey_ss58`` field, where one exists)
        always wins; otherwise the wallet's own hotkey.
        """
        if override:
            return override
        hotkey = getattr(wallet, "hotkey", None)
        if hotkey is not None:
            return hotkey.ss58_address
        if self.signer == "hotkey" and hasattr(wallet, "ss58_address"):
            return wallet.ss58_address  # the signer IS the hotkey
        raise BittensorError(
            f"{self.op}: this signer carries no hotkey to default to; "
            "pass the hotkey address explicitly or sign with a Wallet"
        )

    def hotkey_public_key(self, wallet: Any) -> bytes:
        """The hotkey's raw public key (e.g. for timelock commit encryption)."""
        hotkey = getattr(wallet, "hotkey", None)
        if hotkey is not None:
            return bytes(hotkey.public_key)
        if self.signer == "hotkey" and getattr(wallet, "public_key", None) is not None:
            return bytes(wallet.public_key)
        raise BittensorError(
            f"{self.op}: this signer carries no hotkey public key; sign with a Wallet "
            "or a hotkey Signer"
        )

    def coldkey_address(self, wallet: Any) -> str:
        """The coldkey ss58 address, from the public view (never unlocks)."""
        coldkeypub = getattr(wallet, "coldkeypub", None)
        if coldkeypub is not None:
            return coldkeypub.ss58_address
        if self.signer == "coldkey" and hasattr(wallet, "ss58_address"):
            return wallet.ss58_address  # the signer IS the coldkey
        raise BittensorError(
            f"{self.op}: this signer carries no coldkey to reference; "
            "sign with a Wallet or a coldkey Signer"
        )

    def spend(self) -> Spend:
        """TAO this intent moves out of / destroys from the signer, for policy checks.

        Return a TAO :class:`Balance` when the spend is known exactly,
        ``UNBOUNDED`` for intents that move or burn an amount the SDK can't
        cheaply bound (so a spend cap blocks them until raised), or ``None``
        for intents that move no TAO. Default is ``None`` — override when
        value leaves.
        """
        return None

    def touches_netuids(self) -> list[int]:
        """Every netuid this intent acts on, for policy allowlists.

        Defaults to the intent's ``netuid`` field if present, else none. Intents
        that span origin+destination override to return both.
        """
        netuid = getattr(self, "netuid", None)
        return [netuid] if netuid is not None else []

    def affects_all_subnets(self) -> bool:
        """True if the intent acts across every subnet (so any allowlist must fail it)."""
        return False

    def semantic_intent(self) -> "Intent":
        """Intent whose safety contract governs this submission.

        Execution adapters may wrap a call without changing the spend, subnet,
        or MEV requirements of the operation being dispatched.
        """
        return self

    # Introspection ----------------------------------------------------------

    @classmethod
    def field_help(cls, name: str) -> Optional[str]:
        """The declared help text for one field (``metadata["help"]``), if any.

        Single source for the generated ``tx`` option help, the interactive
        prompt hints, and hand-written commands that wrap this intent.
        """
        for f in fields(cls):
            if f.name == name:
                return f.metadata.get("help")
        return None

    @classmethod
    def describe(cls) -> str:
        """The full docstring, dedented, for help screens and the tool manifest.

        RST-style double backticks (the docstring convention here) are collapsed
        to single backticks so --help and the JSON manifest read naturally.
        """
        return inspect.cleandoc(cls.__doc__ or "").replace("``", "`")

    # Serialization ----------------------------------------------------------

    def to_dict(self) -> dict[str, Any]:
        """A JSON-native dict that round-trips through ``from_args`` exactly.

        Money fields (held as ``Balance``) serialize as exact decimal strings —
        a float here would silently corrupt amounts above ~9M TAO.
        """
        return {"op": self.op, **{k: _encode(v) for k, v in asdict(self).items()}}

    @classmethod
    def from_args(cls, args: dict[str, Any]) -> "Intent":
        allowed = {f.name for f in fields(cls)}
        unknown = set(args) - allowed - {"op"}
        if unknown:
            raise ValueError(f"Unknown arguments for {cls.op}: {sorted(unknown)}")
        return cls(**{k: v for k, v in args.items() if k in allowed})

    @classmethod
    def json_schema(cls) -> dict[str, Any]:
        """JSON Schema for this intent's parameters (for tool/agent discovery).

        Field descriptions come from each dataclass field's ``metadata["help"]``
        — the same text the CLI shows as the option's ``--help`` — so the agent
        manifest and the human help can't say different things.
        """
        properties: dict[str, Any] = {}
        required: list[str] = []
        for f in fields(cls):
            if is_money(str(f.type)):
                schema: dict[str, Any] = money_schema(allow_all=f.name in cls.all_amount_fields)
            else:
                schema = _json_type(str(f.type))
            help_text = f.metadata.get("help")
            if help_text:
                schema["description"] = help_text
            properties[f.name] = schema
            if f.default is MISSING and f.default_factory is MISSING:
                required.append(f.name)
        return {
            "type": "object",
            "properties": properties,
            "required": required,
            "additionalProperties": False,
        }


def _encode(value: Any) -> Any:
    """JSON-native form of a field value: Balances become exact decimal strings."""
    if isinstance(value, Balance):
        return format(value.decimal, "f")
    if isinstance(value, list):
        return [_encode(item) for item in value]
    if isinstance(value, dict):
        return {key: _encode(item) for key, item in value.items()}
    return value


@dataclass
class BuiltCall:
    """A composed call plus structured extras to surface into the result.

    Returned from ``Intent.build`` when an intent computes data at build time that
    the caller should see in the execution result (e.g. a weights reveal round).
    """

    call: Any
    extras: dict[str, Any] = field(default_factory=dict)
