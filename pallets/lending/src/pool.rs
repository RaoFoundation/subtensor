//! The pool adapter the pallet reads through (DESIGN-v2 section 2).
//!
//! `pallet_lending` does not depend on `pallet_subtensor`; subtensor implements this trait.
//! It currently carries only the reads behind the price rings and their seeds; the tasks that
//! add positions extend it with quotes and the balance-moving legs.

use alloc::vec::Vec;
use subtensor_runtime_common::NetUid;

use crate::math::fixed::Price;

/// Read access to subnet pools. `AccountId` is unused by the reads below and is fixed here
/// because the custody legs added later are keyed by it.
pub trait LendingPoolInterface<AccountId> {
    /// Every live non-root subnet: the set that must hold price rings (SPEC I25).
    fn live_subnets() -> Vec<NetUid>;
    /// Upper bound on `live_subnets().len()` (the registration `SubnetLimit`), used to charge
    /// `set_enabled` for its re-seed (SPEC section 9 item 3).
    fn subnet_limit() -> u32;
    /// Live pool spot, rao TAO per rao alpha, with no add-back of minted alpha (SPEC P14).
    fn spot_price(netuid: NetUid) -> Price;
    /// Live pool TAO reserve (`SubnetTAO`) in rao; lending custody is not in it.
    fn pool_tao(netuid: NetUid) -> u64;
    /// The emission EMA (`SubnetMovingPrice`). The only permitted reads are the rank-ring
    /// seeds at the runtime upgrade and on enable (SPEC 8.7, section 9 items 1 and 3).
    fn moving_alpha_price(netuid: NetUid) -> Price;
}
