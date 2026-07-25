"""Declarative intents: mutations as serializable, plannable, policy-gated data.

Importing this package registers every intent. Use the concrete intent classes
directly, or discover/build them by name via the registry helpers.
"""

from ._money import ALL, UNBOUNDED, Money, Spend
from .association import AssociateEvmKey, AssociateHotkey
from .base import Intent
from .batch import Batch
from .children import DecreaseTake, IncreaseTake, SetChildkeyTake, SetChildren, SetTake
from .coldkey import (
    AnnounceColdkeySwap,
    ClearColdkeySwapAnnouncement,
    DisputeColdkeySwap,
    SwapColdkeyAnnounced,
)
from .collateral import AddCollateral, SetMinCollateral
from .crowdloan import (
    ContributeCrowdloan,
    CreateCrowdloan,
    DissolveCrowdloan,
    FinalizeCrowdloan,
    RefundCrowdloan,
    SetCrowdloanMaxContribution,
    UpdateCrowdloanCap,
    UpdateCrowdloanEnd,
    UpdateCrowdloanMinContribution,
    WithdrawCrowdloan,
)
from .evm import EvmWithdraw, FundEvmKey
from .governance import (
    SetMechanismCount,
    StakeBurn,
    TrimSubnet,
    UpdateSymbol,
)
from .hyperparameters import OWNER_HYPERPARAMETERS, SetHyperparameter
from .identity import SetIdentity, SetSubnetIdentity
from .leasing import RegisterLeasedNetwork, TerminateLease
from .lock import LockStake, MoveLock, SetPerpetualLock
from .multisig import (
    MultisigApprove,
    MultisigCancel,
    MultisigExecute,
    MultisigThreshold1,
)
from .plan import Plan, Policy
from .proxy import (
    PROXY_TYPES,
    AddProxy,
    CreatePureProxy,
    ExecuteProxyAnnounced,
    KillPureProxy,
    RemoveProxies,
    RemoveProxy,
)
from .registration import (
    BurnedRegister,
    ClaimRoot,
    RegisterSubnet,
    RootRegister,
    SetRootClaimType,
    StartCall,
    SwapHotkey,
)
from .registry import REGISTRY, build, list_tools, register
from .root import SetSubnetEmissionEnabled
from .serving import ResetAxon, ServeAxon, ServeAxonTls, ServePrometheus
from .staking import (
    AddStake,
    AddStakeLimit,
    MoveStake,
    RemoveStake,
    RemoveStakeLimit,
    SetAutoStake,
    SwapStake,
    TransferStake,
    UnstakeAll,
    UnstakeAllAlpha,
)
from .transfer import Transfer, TransferAll
from .weights import CommitWeights, RevealWeights, SetWeights, normalize

__all__ = [
    "ALL",
    "OWNER_HYPERPARAMETERS",
    "PROXY_TYPES",
    "REGISTRY",
    "UNBOUNDED",
    "AddCollateral",
    "AddProxy",
    "AddStake",
    "AddStakeLimit",
    "AnnounceColdkeySwap",
    "AssociateEvmKey",
    "AssociateHotkey",
    "Batch",
    "BurnedRegister",
    "ClaimRoot",
    "ClearColdkeySwapAnnouncement",
    "CommitWeights",
    "ContributeCrowdloan",
    "CreateCrowdloan",
    "CreatePureProxy",
    "DecreaseTake",
    "DisputeColdkeySwap",
    "DissolveCrowdloan",
    "EvmWithdraw",
    "ExecuteProxyAnnounced",
    "FinalizeCrowdloan",
    "FundEvmKey",
    "IncreaseTake",
    "Intent",
    "KillPureProxy",
    "LockStake",
    # Money vocabulary: what money fields accept (Money), the drain sentinel
    # (ALL), and the spend contract for policy checks (Spend / UNBOUNDED).
    "Money",
    "MoveLock",
    "MoveStake",
    "MultisigApprove",
    "MultisigCancel",
    "MultisigExecute",
    "MultisigThreshold1",
    "Plan",
    "Policy",
    "RefundCrowdloan",
    "RegisterLeasedNetwork",
    "RegisterSubnet",
    "RemoveProxies",
    "RemoveProxy",
    "RemoveStake",
    "RemoveStakeLimit",
    "ResetAxon",
    "RevealWeights",
    "RootRegister",
    "ServeAxon",
    "ServeAxonTls",
    "ServePrometheus",
    "SetAutoStake",
    "SetChildkeyTake",
    "SetChildren",
    "SetCrowdloanMaxContribution",
    "SetHyperparameter",
    "SetIdentity",
    "SetMechanismCount",
    "SetMinCollateral",
    "SetPerpetualLock",
    "SetRootClaimType",
    "SetSubnetEmissionEnabled",
    "SetSubnetIdentity",
    "SetTake",
    "SetWeights",
    "Spend",
    "StakeBurn",
    "StartCall",
    "SwapColdkeyAnnounced",
    "SwapHotkey",
    "SwapStake",
    "TerminateLease",
    "Transfer",
    "TransferAll",
    "TransferStake",
    "TrimSubnet",
    "UnstakeAll",
    "UnstakeAllAlpha",
    "UpdateCrowdloanCap",
    "UpdateCrowdloanEnd",
    "UpdateCrowdloanMinContribution",
    "UpdateSymbol",
    "WithdrawCrowdloan",
    "build",
    "list_tools",
    "normalize",
    "register",
]
