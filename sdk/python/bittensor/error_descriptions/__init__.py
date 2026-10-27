"""Per-chain-error descriptions: what the error means and where to check.

Every name classified in :mod:`bittensor.error_map` has a description here that
tells an agent what condition triggered the error and which state, argument, or
query to inspect. ``python -m codegen.check --names`` enforces that this table
and ``NAME_TO_CODE`` cover exactly the same names, so a newly classified error
must be described before it can ship.

Descriptions are split by the pallet that (first) declares the error. Import
:data:`DESCRIPTIONS` from this package — the public API is unchanged.
"""

from __future__ import annotations

from .admin_utils import DESCRIPTIONS as _ADMIN_UTILS
from .balances import DESCRIPTIONS as _BALANCES
from .commitments import DESCRIPTIONS as _COMMITMENTS
from .contracts import DESCRIPTIONS as _CONTRACTS
from .crowdloan import DESCRIPTIONS as _CROWDLOAN
from .drand import DESCRIPTIONS as _DRAND
from .ethereum import DESCRIPTIONS as _ETHEREUM
from .evm import DESCRIPTIONS as _EVM
from .grandpa import DESCRIPTIONS as _GRANDPA
from .limit_orders import DESCRIPTIONS as _LIMIT_ORDERS
from .mev_shield import DESCRIPTIONS as _MEV_SHIELD
from .multisig import DESCRIPTIONS as _MULTISIG
from .preimage import DESCRIPTIONS as _PREIMAGE
from .proxy import DESCRIPTIONS as _PROXY
from .safe_mode import DESCRIPTIONS as _SAFE_MODE
from .scheduler import DESCRIPTIONS as _SCHEDULER
from .subtensor import DESCRIPTIONS as _SUBTENSOR
from .sudo import DESCRIPTIONS as _SUDO
from .swap import DESCRIPTIONS as _SWAP
from .system import DESCRIPTIONS as _SYSTEM
from .utility import DESCRIPTIONS as _UTILITY

DESCRIPTIONS: dict[str, str] = {}
for _part in (
    _ADMIN_UTILS,
    _BALANCES,
    _COMMITMENTS,
    _CONTRACTS,
    _CROWDLOAN,
    _DRAND,
    _EVM,
    _ETHEREUM,
    _GRANDPA,
    _LIMIT_ORDERS,
    _MEV_SHIELD,
    _MULTISIG,
    _PREIMAGE,
    _PROXY,
    _SAFE_MODE,
    _SCHEDULER,
    _SUBTENSOR,
    _SUDO,
    _SWAP,
    _SYSTEM,
    _UTILITY,
):
    overlap = DESCRIPTIONS.keys() & _part.keys()
    if overlap:
        raise RuntimeError(f"duplicate error descriptions: {sorted(overlap)}")
    DESCRIPTIONS.update(_part)

__all__ = ["DESCRIPTIONS"]
