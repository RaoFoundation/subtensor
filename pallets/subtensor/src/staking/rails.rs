//! What the hub of the canonical rails does in staking: [`RailsStaking`] over this pallet.
//!
//! The hub's vault for a subnet is an ordinary coldkey that stakes at the subnet's index
//! hotkey. Every method here runs the code of the extrinsic an account would sign, with the
//! vault as the signer: the same validation, the same AMM, the same accounting.
//!
//! Staking keeps the existential deposit in the coldkey that pays. The first stake or spend
//! of a vault therefore uses that much less than it was given, once, and the hub measures
//! what a call did by the change of the vault's stake or balance rather than by what it
//! asked for.

use frame_support::weights::Weight;
use frame_system::RawOrigin;
use subtensor_runtime_common::{
    AlphaBalance, GammaBalance, NetUid, TaoBalance,
    rails::{BridgedPayment, RailsStaking, SubnetStatus},
};

use super::*;
use crate::weights::WeightInfo;

impl<T: Config> RailsStaking<T::AccountId> for Pallet<T> {
    fn subnet_status(netuid: NetUid) -> SubnetStatus {
        // Root has no alpha: staking there is holding TAO.
        if netuid.is_root() {
            SubnetStatus::Absent
        } else if Self::if_subnet_exist(netuid) {
            SubnetStatus::Live {
                generation: RegisteredSubnetCounter::<T>::get(netuid),
            }
        } else if DissolveCleanupQueue::<T>::get().contains(&netuid) {
            // The netuid cannot be registered again before its cleanup, which pays the
            // stake out, has finished.
            SubnetStatus::Dissolving
        } else {
            SubnetStatus::Absent
        }
    }

    fn subnet_name(netuid: NetUid) -> Option<Vec<u8>> {
        SubnetIdentitiesV3::<T>::get(netuid).map(|identity| identity.subnet_name)
    }

    fn open_index(netuid: NetUid, vault: &T::AccountId) -> DispatchResult {
        Self::do_open_index(netuid, vault)
    }

    fn staked(vault: &T::AccountId, netuid: NetUid) -> AlphaBalance {
        Self::get_stake_for_hotkey_and_coldkey_on_subnet(&Self::index_hotkey(netuid), vault, netuid)
    }

    fn stake(
        vault: &T::AccountId,
        netuid: NetUid,
        tao: TaoBalance,
    ) -> Result<AlphaBalance, DispatchError> {
        let before = Self::staked(vault, netuid);
        Self::do_add_stake(
            RawOrigin::Signed(vault.clone()).into(),
            Self::index_hotkey(netuid),
            netuid,
            tao,
        )?;
        Ok(Self::staked(vault, netuid).saturating_sub(before))
    }

    fn unstake(
        vault: &T::AccountId,
        netuid: NetUid,
        alpha: AlphaBalance,
    ) -> Result<TaoBalance, DispatchError> {
        // `do_remove_stake` lowers a request to what is staked. The hub asked for an exact
        // amount and must not be paid for less.
        ensure!(
            alpha <= Self::staked(vault, netuid),
            Error::<T>::NotEnoughStakeToWithdraw
        );
        let before = Self::get_coldkey_balance(vault);
        Self::do_remove_stake(
            RawOrigin::Signed(vault.clone()).into(),
            Self::index_hotkey(netuid),
            netuid,
            alpha,
        )?;
        Ok(Self::get_coldkey_balance(vault).saturating_sub(before))
    }

    fn spend_at_service(
        funder: &T::AccountId,
        netuid: NetUid,
        service: &T::AccountId,
        tao: TaoBalance,
        tao_usd_price_cap: u64,
        payment: BridgedPayment<T::AccountId>,
    ) -> Result<(AlphaBalance, GammaBalance), DispatchError> {
        Self::do_spend_tao_at_service(funder, netuid, service, tao, tao_usd_price_cap, payment)
            .map(|(_, alpha, gamma)| (alpha, gamma))
    }

    fn weight() -> Weight {
        <T as Config>::WeightInfo::remove_stake()
            .saturating_add(
                <T as Config>::WeightInfo::add_stake().max(<T as Config>::WeightInfo::mint_gamma()),
            )
            .max(Self::index_refresh_weight())
    }

    #[cfg(feature = "runtime-benchmarks")]
    fn benchmark_subnet() -> NetUid {
        let netuid = NetUid::from(1);
        if !Self::if_subnet_exist(netuid) {
            Self::init_new_network(netuid, 360);
        }
        netuid
    }
}
