"""Typed surface of the read namespaces. GENERATED — DO NOT EDIT.

Generated from the read registry by ``python -m codegen.emit_namespaces``;
``python -m codegen.check --namespaces`` gates drift in CI. The runtime
implementation lives in ``namespaces.py`` (curated methods + a ``__getattr__``
that dispatches any registry read).
"""


from typing import Any, Optional
from .balance import Balance
from .metagraph import Metagraph, NeuronCommitment
from .reads.delegation import DelegateInfo, DelegatedStake
from .reads.identity import Commitment
from .reads.neurons import Neuron
from .reads.prices import SwapQuote
from .reads.staking import StakePosition, StakeValuation
from .reads.subnets import SubnetInfo

class _ReadNamespace:
    def __init__(self, view: Any) -> None: ...

class Balances(_ReadNamespace):
    """Free-TAO balances, plus every "Accounts & keys" read (multisig, proxies, ...)."""

    async def get(self, address: str, block: Optional[int] = None) -> Balance:
        """Free TAO balance of a coldkey address."""

    async def get_many(self, addresses: list[str], block: Optional[int] = None) -> dict[str, Balance]:
        """Free TAO balance for several coldkey addresses in one batched request."""

    async def existential_deposit(self) -> Balance:
        """Minimum balance an account must keep to stay alive."""

    async def balance(self, coldkey_ss58: str, *, block: Optional[int] = None) -> Balance:
        """Free TAO balance of a coldkey address."""

    async def balances(self, coldkey_ss58s: list[str], *, block: Optional[int] = None) -> dict[str, Balance]:
        """Free TAO balance for several coldkey addresses in one batched request."""

    async def coldkey_swap_announcement(self, coldkey_ss58: str, *, block: Optional[int] = None) -> Optional[dict]:
        """A coldkey's pending swap announcement (execute block, new-key hash, disputed), or None.

        This is the `swap-check` status: `ColdkeySwapAnnouncements` stores the block
        at which the swap becomes executable and the BlakeTwo256 hash committed to.
        """

    async def multisig(self, account_ss58: str, call_hash: str, *, block: Optional[int] = None) -> Optional[dict]:
        """A pending multisig operation (opening timepoint, approvals, depositor), or None."""

    async def proxies(self, coldkey_ss58: str, *, block: Optional[int] = None) -> dict:
        """Proxy delegations of an account: who may sign on its behalf, and the reserved deposit."""

class Chain(_ReadNamespace):
    """Chain-level reads: time, block metadata, global limits."""

    async def block_info(self, block: int) -> Optional[dict]:
        """A block's hash, timestamp, and extrinsics (as module.function summaries).

        Programmatic callers wanting the fully decoded extrinsics and raw header
        should use `client.block_info()` instead.
        """

    async def block_time(self, *, block: Optional[int] = None) -> float:
        """Seconds per block, detected from the chain (12.0 mainnet, 0.25 fast-blocks)."""

    async def is_fast_blocks(self, *, block: Optional[int] = None) -> bool:
        """Whether the chain runs fast blocks (0.25s slots, local/e2e testing mode)."""

    async def mev_shield_next_key(self, *, block: Optional[int] = None) -> Optional[str]:
        """The MEV Shield ML-KEM-768 public key (0x-hex) used to encrypt shielded txs, or None."""

    async def timestamp(self, *, block: Optional[int] = None) -> dict:
        """Current chain time (from the Timestamp pallet) and the block it was read at."""

    async def tx_rate_limit(self, netuid: Optional[int] = None, *, block: Optional[int] = None) -> int:
        """Global transaction rate limit in blocks."""

class Collateral(_ReadNamespace):
    """Miner registration collateral."""

    async def collateral_policy(self, netuid: int, *, block: Optional[int] = None) -> dict:
        """A subnet's collateral configuration.

        `lock_share` is the fraction of the registration price locked as
        collateral instead of burned (0 disables collateral); `drain_ratio` is
        the alpha released per alpha of miner incentive earned, snapshot per
        miner at registration.
        """

    async def miner_collateral(self, netuid: int, hotkey_ss58: str, coldkey_ss58: str | None = None, *, block: Optional[int] = None) -> Optional[dict]:
        """A `(hotkey, coldkey)` stake position's standing collateral, or None.

        Collateral is keyed by hotkey + coldkey so nominators on the same hotkey are
        never charged for the owner's bond. When `coldkey_ss58` is omitted, the
        hotkey owner is used.

        `locked_alpha` is non-withdrawable stake released through earned incentive
        at `drain_ratio` alpha per alpha earned; `min_locked_alpha` is the
        miner-set floor the lock self-maintains around (the drain stops at it and
        incentive fills any shortfall); `earned_alpha` is lifetime incentive since
        the collateral entry existed. Derived: `headroom_alpha` (draining portion
        above the floor), `shortfall_alpha` (capture in progress below the floor),
        and `releasable_work_alpha` (incentive still needed to release the
        headroom). The lock survives deregistration and is credited on
        re-registration.
        """

    async def subnet_collateral(self, netuid: int, *, block: Optional[int] = None) -> list[dict]:
        """Every `(hotkey, coldkey)` position with standing collateral on a subnet.

        The list validator code reads to enforce a per-machine collateral
        requirement: each record carries the locked amount, the miner's
        self-maintained floor, the drain-ratio snapshot, and the hotkey's current
        `uid` (None when the hotkey is deregistered but its collateral persists) —
        ready to join against the metagraph when scoring.
        """

class Delegation(_ReadNamespace):
    """Delegate info, delegated stake, and child/parent hotkey relations."""

    async def children(self, hotkey_ss58: str, netuid: int, *, block: Optional[int] = None) -> list[tuple[int, str]]:
        """Child hotkeys of a parent on a subnet, as (proportion, child_ss58) pairs.

        Proportions are u64-normalized fractions of the parent's stake, where
        u64::MAX means 100%.
        """

    async def delegate(self, hotkey_ss58: str, *, block: Optional[int] = None) -> Optional[DelegateInfo]:
        """Delegate info for one hotkey, or None if it is not a delegate."""

    async def delegate_take(self, hotkey_ss58: str, *, block: Optional[int] = None) -> dict:
        """A hotkey's delegate take (emission fraction it keeps) with the allowed min/max.

        `take`, `min`, and `max` are fractions in 0..1; `take_u16` is the raw
        on-chain u16 value.
        """

    async def delegated(self, coldkey_ss58: str, *, block: Optional[int] = None) -> list[DelegatedStake]:
        """Every nomination a coldkey holds: (delegate, netuid, stake) per position.

        Each `stake` is denominated in the subnet's own currency: subnet alpha, or
        TAO when netuid is 0.
        """

    async def delegates(self, *, block: Optional[int] = None) -> list[DelegateInfo]:
        """Every delegate hotkey on the network, with take and registrations."""

    async def is_delegate(self, hotkey_ss58: str, *, block: Optional[int] = None) -> bool:
        """Whether a hotkey is a delegate."""

    async def parents(self, hotkey_ss58: str, netuid: int, *, block: Optional[int] = None) -> list[tuple[int, str]]:
        """Parent hotkeys of a child on a subnet, as (proportion, parent_ss58) pairs.

        Proportions are u64-normalized fractions of the parent's stake, where
        u64::MAX means 100%.
        """

    async def pending_children(self, hotkey_ss58: str, netuid: int, *, block: Optional[int] = None) -> dict:
        """Proposed child hotkeys of a parent still in cooldown, and when they apply.

        `set_children` normally does not take effect immediately: the proposal is
        parked here until `cooldown_block`, then promoted to the finalized set
        that the `children` read returns. On subnets whose subtoken is not yet
        enabled the cooldown is skipped and children apply immediately, so
        nothing lingers here. `children` is (proportion, child_ss58) pairs with
        u64-normalized proportions, matching the `children` read.
        """

class Epochs(_ReadNamespace):
    """Epoch timing: blocks since/until steps, epoch status."""

    async def blocks_since_last_step(self, netuid: Optional[int] = None, *, block: Optional[int] = None) -> int:
        """Blocks since the subnet's last epoch step ran."""

    async def blocks_since_last_update(self, netuid: int, uid: int, *, block: Optional[int] = None) -> Optional[int]:
        """Blocks since a neuron last set or committed weights on a subnet
        (mechanism 0), or None.

        On commit-reveal subnets LastUpdate is bumped at commit time, not when
        the weights are revealed and applied. None means the subnet has no
        LastUpdate vector or the uid does not exist. The standard "is this
        validator still setting weights" health check.
        """

    async def blocks_until_next_epoch(self, netuid: int, *, block: Optional[int] = None) -> Optional[int]:
        """Blocks until the subnet's next epoch fires, or None if tempo is 0.

        Derived from the chain's own `next_epoch_start_block` at one pinned block.
        The actual fire can shift by a few blocks (per-block epoch cap defers,
        owner triggers pull earlier) — to act on the epoch itself, use
        `client.wait_for_epoch()`.
        """

    async def epoch_status(self, netuid: int, *, block: Optional[int] = None) -> dict:
        """Where a subnet is in its epoch cycle, in one block-pinned read.

        Bundles tempo, the last epoch run, the chain-computed next start block, and
        the remaining blocks/seconds (seconds use the chain's detected block time,
        so they are correct on fast-blocks chains too). `pending_epoch_at` is the
        block of an owner-triggered epoch, or None when none is pending.
        """

    async def next_epoch_start_block(self, netuid: int, *, block: Optional[int] = None) -> Optional[int]:
        """Block at which the subnet's next epoch is expected to fire, or None if tempo is 0.

        Chain-computed: `LastEpochBlock + tempo`, pulled earlier by any pending
        owner-triggered epoch. This is the source of truth — the epoch schedule is
        stateful, so the legacy client-side modular formula is wrong on subnets
        whose tempo changed or whose owner triggered an epoch.
        """

class Hyperparameters(_ReadNamespace):
    """Per-subnet hyperparameter scalars (difficulty, immunity period, ...)."""

    async def difficulty(self, netuid: Optional[int] = None, *, block: Optional[int] = None) -> int:
        """Current PoW registration difficulty for a subnet."""

    async def immunity_period(self, netuid: Optional[int] = None, *, block: Optional[int] = None) -> int:
        """Blocks a newly registered neuron is immune from deregistration."""

    async def max_weight_limit(self, netuid: Optional[int] = None, *, block: Optional[int] = None) -> int:
        """Normalized-fraction cap on any single weight: after the chain normalizes the submitted vector, no weight may exceed max_weight_limit/65535 of the total. Not a raw u16 ceiling per weight."""

    async def min_allowed_weights(self, netuid: Optional[int] = None, *, block: Optional[int] = None) -> int:
        """Minimum number of weights a validator must set on a subnet. A pure self-weight (a single entry pointing at the caller's own uid) bypasses the minimum, and the subnet owner bypasses validator-permit rules."""

    async def reveal_period(self, netuid: Optional[int] = None, *, block: Optional[int] = None) -> int:
        """Commit-reveal reveal window, in epochs, for a subnet."""

    async def subnet_emission_enabled(self, netuid: Optional[int] = None, *, block: Optional[int] = None) -> int:
        """Root-controlled pool-side TAO emission flag for a subnet (1 = enabled). When 0, the subnet earns no TAO emission share even while subnet_is_active is true; only root can flip it (see the set_subnet_emission_enabled intent)."""

    async def weights_rate_limit(self, netuid: Optional[int] = None, *, block: Optional[int] = None) -> int:
        """Blocks a hotkey must wait between weight sets on a subnet."""

class Identity(_ReadNamespace):
    """On-chain identities and commitments."""

    async def commitment(self, netuid: int, hotkey_ss58: str, *, block: Optional[int] = None) -> Optional[Commitment]:
        """The commitment a hotkey has published on a subnet, or None."""

    async def commitments(self, netuid: int, *, block: Optional[int] = None) -> list[dict]:
        """Every commitment on a subnet, newest first: hotkey, uid, content, block, age, reveal state.

        One row per hotkey that has (or had) a commitment — including sealed
        timelocked payloads waiting on drand (`is_revealed` false, `reveals_at`
        set) and fully-revealed ones whose live storage entry the chain already
        dropped. `commitment` is the currently visible content: the plaintext, or
        the latest chain-decrypted payload; null while still sealed.
        """

    async def hotkey_identities(self, hotkey_ss58s: list[str], *, block: Optional[int] = None) -> dict[str, dict]:
        """On-chain identities for hotkeys (via each hotkey's owner coldkey), keyed by hotkey."""

    async def identity(self, coldkey_ss58: str, *, block: Optional[int] = None) -> Optional[dict]:
        """The on-chain identity of a coldkey (name, links, description), or None."""

    async def revealed_commitment(self, netuid: int, hotkey_ss58: str, *, block: Optional[int] = None) -> Any:
        """The revealed (timelock-decrypted) commitment for a hotkey on a subnet, or None."""

class Leasing(_ReadNamespace):
    """Leases and crowdloans."""

    async def crowdloan(self, crowdloan_id: int, *, block: Optional[int] = None) -> Optional[dict]:
        """A crowdloan's state (creator, deposit, raised, cap, end, target/call), or None.

        Amounts are TAO; `end` is the block number at which contributions close.
        """

    async def crowdloan_contributors(self, crowdloan_id: int, *, block: Optional[int] = None) -> list[dict]:
        """Contributors and amounts for a crowdloan, with amounts in TAO."""

    async def crowdloans(self, *, block: Optional[int] = None) -> list[dict]:
        """All crowdloans on chain (id and summary fields)."""

    async def lease(self, lease_id: int, *, block: Optional[int] = None) -> Optional[dict]:
        """A subnet lease by id (beneficiary, emissions share, end block, netuid, cost), or None.

        `emissions_share` is a percentage from 0 to 100; `end_block` is None for a
        perpetual lease.
        """

    async def leases(self, *, block: Optional[int] = None) -> list[dict]:
        """Every subnet lease on the network."""

class Locks(_ReadNamespace):
    """Stake locks and conviction."""

    async def coldkey_lock(self, coldkey_ss58: str, netuid: int, *, block: Optional[int] = None) -> Optional[dict]:
        """Lock state for a coldkey on a subnet, or None if no lock exists.

        `locked_alpha` is denominated in the subnet's alpha and rolled forward
        (decayed) to the current block; `is_perpetual` locks do not decay.
        """

    async def hotkey_conviction(self, hotkey_ss58: str, netuid: int, *, block: Optional[int] = None) -> dict:
        """Conviction metrics for a hotkey on a subnet.

        Returns `conviction_alpha` denominated in the subnet's alpha, rolled
        forward to the current block.
        """

    async def locks_for_coldkey(self, coldkey_ss58: str, *, block: Optional[int] = None) -> list[dict]:
        """Every lock a coldkey holds (one per subnet), rolled forward to now."""

    async def most_convicted_hotkey(self, netuid: int, *, block: Optional[int] = None) -> Optional[str]:
        """Hotkey with the highest conviction on a subnet, if any."""

    async def subnet_convictions(self, netuid: int, *, block: Optional[int] = None) -> dict:
        """Every hotkey with locked stake on a subnet, rolled forward to now.

        Per hotkey: locked mass, conviction, and the estimated blocks until its
        conviction reaches 10% of the subnet's outstanding alpha. That per-hotkey
        figure is a projection heuristic, not a takeover trigger: the ownership
        takeover in `change_subnet_owner_if_needed` requires the subnet to be
        at least ~1 year old (2,629,800 blocks) and the total aggregate
        conviction across all lockers to reach 10% of `SubnetAlphaOut`, at
        which point the highest-conviction hotkey's coldkey becomes the subnet
        owner. Projections assume the lock rates and alpha out stay constant.
        """

class Neurons(_ReadNamespace):
    """Neuron listings and registration lookups, plus every "Neurons & registration" read."""

    async def all(self, netuid: int, block: Optional[int] = None, *, lite: bool = True) -> list[Neuron]:
        """Every neuron on a subnet in ONE runtime-API call (the metagraph fast path).

        ``lite=True`` (default) omits per-neuron weights/bonds, which is far
        smaller and what almost every caller wants.
        """

    async def associated_evm_key(self, netuid: int, uid: int, *, block: Optional[int] = None) -> Optional[dict]:
        """The EVM address associated with a neuron (by netuid + uid) and the block it was set."""

    async def hotkey_owner(self, hotkey_ss58: str, *, block: Optional[int] = None) -> Optional[str]:
        """The coldkey that owns a hotkey, or None if unowned."""

    async def netuids_for_hotkey(self, hotkey_ss58: str, *, block: Optional[int] = None) -> list[int]:
        """Every subnet a hotkey is registered on, as a sorted list of netuids."""

    async def neurons(self, netuid: int, lite: bool = True, *, block: Optional[int] = None) -> list[Neuron]:
        """Every neuron on a subnet in ONE runtime-API call (the metagraph fast path).

        `lite=true` (the default) omits per-neuron weights/bonds, which is far
        smaller and what almost every caller wants.
        """

    async def owned_hotkeys(self, coldkey_ss58: str, *, block: Optional[int] = None) -> list[str]:
        """Every hotkey owned by a coldkey (the reverse of `hotkey_owner`)."""

    async def uid(self, hotkey_ss58: str, netuid: int, *, block: Optional[int] = None) -> Optional[int]:
        """UID of a hotkey on a subnet, or None if not registered there."""

class Prices(_ReadNamespace):
    """Alpha prices and stake/unstake swap quotes."""

    async def alpha_price(self, netuid: int, *, block: Optional[int] = None) -> dict:
        """Current spot alpha price for a subnet, as TAO per alpha."""

    async def alpha_prices(self, *, block: Optional[int] = None) -> dict[int, float]:
        """Spot alpha price for every subnet, as TAO per alpha keyed by netuid."""

    async def quote_stake(self, netuid: int, amount_tao: float, *, block: Optional[int] = None) -> SwapQuote:
        """Simulate staking `amount_tao` TAO into a subnet: alpha out, fee, and slippage.

        Use this to compute a safe `limit_price_rao` for the `add_stake_limit` intent.
        """

    async def quote_unstake(self, netuid: int, amount_alpha: float, *, block: Optional[int] = None) -> SwapQuote:
        """Simulate unstaking `amount_alpha` alpha from a subnet: TAO out, fee, and slippage."""

class Staking(_ReadNamespace):
    """Stake amounts and positions, plus every "Staking" read."""

    async def get(self, coldkey_ss58: str, hotkey_ss58: str, netuid: int, block: Optional[int] = None) -> Balance:
        """Alpha staked by a coldkey to a hotkey on a subnet (TAO when netuid is 0)."""

    async def positions(self, coldkey_ss58: str, block: Optional[int] = None) -> list[StakePosition]:
        """Every stake position held by a coldkey, across all hotkeys and subnets."""

    async def auto_stake(self, coldkey_ss58: str, netuid: int, *, block: Optional[int] = None) -> Optional[str]:
        """The hotkey a coldkey auto-stakes its rewards to on a subnet, or None if unset."""

    async def auto_stake_all(self, coldkey_ss58: str, *, block: Optional[int] = None) -> list[dict]:
        """Every auto-stake destination configured for a coldkey.

        One entry per subnet where a destination hotkey is set; subnets with no
        entry have auto-stake unset.
        """

    async def root_claim_type(self, coldkey_ss58: str, *, block: Optional[int] = None) -> dict:
        """How a coldkey claims root alpha emission: Swap, Keep, or KeepSubnets(+subnets)."""

    async def stake(self, coldkey_ss58: str, hotkey_ss58: str, netuid: int, *, block: Optional[int] = None) -> Balance:
        """Alpha staked by a coldkey to a hotkey on a subnet (TAO when netuid is 0)."""

    async def stake_for_coldkey(self, coldkey_ss58: str, *, block: Optional[int] = None) -> list[StakePosition]:
        """Every stake position held by a coldkey, across all hotkeys and subnets.

        Each position's `stake` is denominated in the subnet's own currency: subnet
        alpha, or TAO when netuid is 0. Use `stake_value_for_coldkey` to mark alpha
        positions to TAO.
        """

    async def stake_for_coldkeys(self, coldkey_ss58s: list[str], *, block: Optional[int] = None) -> dict[str, list[StakePosition]]:
        """Every stake position for several coldkeys at once, in one runtime call.

        Each position's `stake` is denominated in the subnet's own currency: subnet
        alpha, or TAO when netuid is 0.
        """

    async def stake_value_for_coldkey(self, coldkey_ss58: str, *, block: Optional[int] = None) -> StakeValuation:
        """A coldkey's stake marked to TAO at spot prices (excludes slippage/fees), block-pinned."""

    async def stake_value_for_coldkeys(self, coldkey_ss58s: list[str], *, block: Optional[int] = None) -> dict[str, StakeValuation]:
        """Spot-price stake valuation for several coldkeys at once, at one block."""

    async def staking_hotkeys(self, coldkey_ss58: str, *, block: Optional[int] = None) -> list[str]:
        """Every hotkey a coldkey has stake with, owned or nominated.

        Unlike `owned_hotkeys` this includes delegates the coldkey has nominated.
        Use `stake_for_coldkey` for the per-subnet amounts behind each entry.
        """

class Subnets(_ReadNamespace):
    """Aggregating/decoding subnet reads, plus every "Subnets" read."""

    async def burn(self, netuid: int, block: Optional[int] = None) -> Balance:
        """Current burn (recycle) cost to register on a subnet."""

    async def commit_reveal_enabled(self, netuid: int, block: Optional[int] = None) -> bool:
        """Whether commit-reveal weights are enabled on a subnet."""

    async def metagraph(self, netuid: int, block: Optional[int] = None, *, commitments: bool = True) -> Optional[Metagraph]:
        """The typed metagraph for a subnet, or None when it does not exist.

        Every neuron with stakes, scores, axon endpoint, identity, and (unless
        ``commitments=False``) its on-chain commitment with timelock/decryption
        status. See :class:`Metagraph`. (This shadows the registry's raw
        ``metagraph`` read, which stays reachable via ``client.read("metagraph")``.)
        """

    async def commitments(self, netuid: int, block: Optional[int] = None) -> dict[str, NeuronCommitment]:
        """Every commitment on a subnet, keyed by hotkey, newest first.

        The bulk view a subnet owner polls: each entry carries the hotkey, its
        uid (None once deregistered), the visible content (``.value``), the
        commit block, how long it has been on chain (``.duration`` /
        ``.age_blocks``), and its reveal state (``.is_revealed`` / ``.status``
        / ``.reveals_at``), plus the chain-decrypted history (``.revealed``).
        Much cheaper than ``metagraph()`` when commitments are all you need.
        """

    async def info(self, netuid: int, block: Optional[int] = None) -> SubnetInfo:
        """Tempo, burn, and neuron count for one subnet (the three reads run concurrently)."""

    async def all(self, block: Optional[int] = None) -> list[SubnetInfo]:
        """Info for every subnet, fetched in four batched map queries rather than
        one-query-per-subnet. This is what listing should use.
        """

    async def mechanism_count(self, netuid: int, *, block: Optional[int] = None) -> int:
        """Current mechanism count configured for a subnet."""

    async def mechanism_emission_split(self, netuid: int, *, block: Optional[int] = None) -> list[int]:
        """Emission split between mechanisms on a subnet.

        Returns the raw on-chain per-mechanism share values, in mechanism order; an
        empty list means no explicit split is set.
        """

    async def subnet(self, netuid: int, *, block: Optional[int] = None) -> SubnetInfo:
        """Tempo, burn, and neuron count for one subnet (the three reads run concurrently)."""

    async def subnet_hyperparameters(self, netuid: int, *, block: Optional[int] = None) -> Optional[dict]:
        """All hyperparameters for a subnet, as a flat name -> raw value mapping.

        The set of names is version-dependent; None if the subnet does not exist.
        Uses the forward-compatible `get_subnet_hyperparams_v3` runtime API, so
        newly added chain hyperparameters (e.g. `burn_half_life`) show up
        without a client update.
        """

    async def subnet_identity(self, netuid: int, *, block: Optional[int] = None) -> Optional[dict]:
        """The identity metadata of a subnet, or None."""

    async def subnet_names(self, *, block: Optional[int] = None) -> dict[int, str]:
        """Registered name of every subnet that published an identity, keyed by netuid."""

    async def subnet_registration_cost(self, *, block: Optional[int] = None) -> Balance:
        """Current cost to register a new subnet.

        Denominated in TAO. The cost is dynamic: it rises when subnets register
        and decays back down over time.
        """

    async def subnet_start_schedule(self, netuid: int, *, block: Optional[int] = None) -> dict:
        """When a registered subnet can call `start_call`.

        All values are block numbers: the subnet can start once the current block
        reaches `earliest_start_block` (registration block plus the chain's delay).
        """

    async def subnets(self, *, block: Optional[int] = None) -> list[SubnetInfo]:
        """Info for every subnet, fetched in four batched map queries rather than
        one-query-per-subnet. This is what listing should use.
        """

    async def token_symbols(self, *, block: Optional[int] = None) -> dict[int, str]:
        """Chain-registered token symbol of every subnet, keyed by netuid.

        Connecting runs this automatically (through a disk cache) and
        installs the result as the connection's display symbols, so balances
        decoded by that client render with each subnet's real symbol.
        """

class Weights(_ReadNamespace):
    """Weights, bonds, and timelocked weight commits."""

    async def bonds(self, netuid: int, mechid: int = 0, *, block: Optional[int] = None) -> dict[int, dict[int, float]]:
        """Validator bond rows as `{validator_uid: {miner_uid: bond}}`, scaled to 0..1.

        Bonds are the slow EMA of a validator's stake-weighted weights that pays
        its dividends; unlike weights their magnitude is meaningful, so values are
        scaled by the u16 maximum (1.0 = the largest bond the chain can store)
        rather than row-normalized.
        """

    async def timelocked_weight_commits(self, netuid: int, mechid: int = 0, *, block: Optional[int] = None) -> dict[int, list[dict]]:
        """Pending (still-encrypted) commit-reveal weight commits, grouped by epoch.

        Each entry carries the committing `hotkey`, the `commit_block`, the drand
        `reveal_round` at which the chain auto-decrypts and applies it, `reveals_at`
        (that round's UTC time, computed locally), and the `ciphertext`. Entries
        disappear from this map once revealed — revealed weights show up in the
        `weights` read.
        """

    async def weights(self, netuid: int, mechid: int = 0, *, block: Optional[int] = None) -> dict[int, dict[int, float]]:
        """Validator weight rows as `{validator_uid: {miner_uid: fraction}}`, each row summing to 1.

        The chain stores max-upscaled u16 values whose absolute scale carries no
        meaning — consensus only uses within-row proportions — so rows are
        normalized to fractions here. On commit-reveal subnets a validator's row
        updates only when its timelocked commit reveals (see
        `timelocked_weight_commits` for what is pending).
        """

NAMESPACES: dict[str, type[_ReadNamespace]]

async def _scoped(view: Any, block: Optional[int]) -> Any: ...
def _accepts_pin(spec: Any) -> bool: ...
