extern crate alloc;

use codec::Compact;
use frame_support::pallet_prelude::{Decode, Encode};
use sp_std::collections::btree_map::BTreeMap;
use subtensor_runtime_common::{AlphaBalance, NetUid, TaoBalance, Token};
use subtensor_swap_interface::SwapHandler;

use super::*;

#[freeze_struct("8cef3fae262a623e")]
#[derive(Decode, Encode, PartialEq, Eq, Clone, Debug, TypeInfo)]
pub struct StakeInfo<AccountId: TypeInfo + Encode + Decode> {
    hotkey: AccountId,
    coldkey: AccountId,
    netuid: Compact<NetUid>,
    stake: Compact<AlphaBalance>,
    locked: Compact<u64>,
    emission: Compact<AlphaBalance>,
    tao_emission: Compact<TaoBalance>,
    drain: Compact<u64>,
    is_registered: bool,
}

#[freeze_struct("2d52e2de04425fb6")]
#[derive(Decode, Encode, PartialEq, Eq, Clone, Debug, TypeInfo)]
pub struct StakeAvailability {
    total: Compact<AlphaBalance>,
    locked: Compact<AlphaBalance>,
    available: Compact<AlphaBalance>,
}

// Per-subnet stake breakdown: total alpha, locked mass, and what is free to unstake.
impl StakeAvailability {
    pub fn total(&self) -> AlphaBalance {
        self.total.into()
    }

    pub fn locked(&self) -> AlphaBalance {
        self.locked.into()
    }

    pub fn available(&self) -> AlphaBalance {
        self.available.into()
    }
}

impl<T: Config> Pallet<T> {
    fn _get_stake_info_for_coldkeys(
        coldkeys: Vec<T::AccountId>,
    ) -> Vec<(T::AccountId, Vec<StakeInfo<T::AccountId>>)> {
        if coldkeys.is_empty() {
            return Vec::new(); // No coldkeys to check
        }
        let netuids = Self::get_all_subnet_netuids();
        let mut stake_info: Vec<(T::AccountId, Vec<StakeInfo<T::AccountId>>)> = Vec::new();
        for coldkey_i in coldkeys.clone().iter() {
            // Get all hotkeys associated with this coldkey.
            let staking_hotkeys = StakingHotkeys::<T>::get(coldkey_i.clone());
            let mut stake_info_for_coldkey: Vec<StakeInfo<T::AccountId>> = Vec::new();
            for netuid_i in netuids.clone().iter() {
                for hotkey_i in staking_hotkeys.clone().iter() {
                    let alpha = Self::get_stake_for_hotkey_and_coldkey_on_subnet(
                        hotkey_i, coldkey_i, *netuid_i,
                    );
                    if alpha.is_zero() {
                        continue;
                    }
                    let emission = AlphaDividendsPerSubnet::<T>::get(*netuid_i, &hotkey_i);
                    // Tao dividends were removed
                    let tao_emission = TaoBalance::ZERO;
                    let is_registered: bool =
                        Self::is_hotkey_registered_on_network(*netuid_i, hotkey_i);
                    stake_info_for_coldkey.push(StakeInfo {
                        hotkey: hotkey_i.clone(),
                        coldkey: coldkey_i.clone(),
                        netuid: (*netuid_i).into(),
                        stake: alpha.into(),
                        locked: 0.into(),
                        emission: emission.into(),
                        tao_emission: tao_emission.into(),
                        drain: 0.into(),
                        is_registered,
                    });
                }
            }
            stake_info.push((coldkey_i.clone(), stake_info_for_coldkey));
        }
        stake_info
    }

    pub fn get_stake_info_for_coldkeys(
        coldkey_accounts: Vec<T::AccountId>,
    ) -> Vec<(T::AccountId, Vec<StakeInfo<T::AccountId>>)> {
        if coldkey_accounts.is_empty() {
            return Vec::new(); // Empty coldkeys
        }

        Self::_get_stake_info_for_coldkeys(coldkey_accounts)
    }

    pub fn get_stake_info_for_coldkey(
        coldkey_account: T::AccountId,
    ) -> Vec<StakeInfo<T::AccountId>> {
        let stake_info = Self::_get_stake_info_for_coldkeys(vec![coldkey_account]);

        if stake_info.is_empty() {
            Vec::new() // Invalid coldkey
        } else {
            let Some(first) = stake_info.first() else {
                return Vec::new();
            };

            first.1.clone()
        }
    }

    pub fn get_stake_info_for_hotkey_coldkey_netuid(
        hotkey_account: T::AccountId,
        coldkey_account: T::AccountId,
        netuid: NetUid,
    ) -> Option<StakeInfo<T::AccountId>> {
        let alpha = Self::get_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey_account,
            &coldkey_account,
            netuid,
        );
        let emission = AlphaDividendsPerSubnet::<T>::get(netuid, &hotkey_account);
        // Tao dividends were removed
        let tao_emission = TaoBalance::ZERO;
        let is_registered: bool = Self::is_hotkey_registered_on_network(netuid, &hotkey_account);

        Some(StakeInfo {
            hotkey: hotkey_account,
            coldkey: coldkey_account,
            netuid: (netuid).into(),
            stake: alpha.into(),
            locked: 0.into(),
            emission: emission.into(),
            tao_emission: tao_emission.into(),
            drain: 0.into(),
            is_registered,
        })
    }

    /// Batch query of unstakable stake per coldkey and subnet.
    ///
    /// `netuids: None` scans every subnet; `Some(vec)` limits the scan.
    /// Subnets with zero stake and zero lock are left out of the response.
    ///
    /// Invalid `Some(vec)` requests (empty or longer than the number of subnets on chain)
    /// return each coldkey with an empty inner map. Non-existent netuids are omitted.
    pub fn get_stake_availability_for_coldkeys(
        coldkey_accounts: Vec<T::AccountId>,
        netuids: Option<Vec<NetUid>>,
    ) -> BTreeMap<T::AccountId, BTreeMap<NetUid, StakeAvailability>> {
        if coldkey_accounts.is_empty() {
            return BTreeMap::new();
        }

        let existing_netuids = Self::get_all_subnet_netuids();

        let netuids = match netuids {
            None => existing_netuids,
            Some(mut requested) => {
                // Same netuid may appear more than once in the request — keep one row per subnet.
                requested.sort();
                requested.dedup();
                if requested.is_empty() || requested.len() > existing_netuids.len() {
                    return coldkey_accounts
                        .into_iter()
                        .map(|coldkey| (coldkey, BTreeMap::new()))
                        .collect();
                }
                requested.retain(|n| Self::if_subnet_exist(*n));
                requested
            }
        };

        coldkey_accounts
            .into_iter()
            .map(|coldkey| {
                let availability: BTreeMap<NetUid, StakeAvailability> = netuids
                    .iter()
                    .filter_map(|netuid| {
                        let (total, locked, available) =
                            Self::stake_availability(&coldkey, *netuid);
                        // Nothing staked and no active lock — skip this subnet.
                        if total.is_zero() && locked.is_zero() {
                            None
                        } else {
                            Some((
                                *netuid,
                                StakeAvailability {
                                    total: total.into(),
                                    locked: locked.into(),
                                    available: available.into(),
                                },
                            ))
                        }
                    })
                    .collect();

                (coldkey, availability)
            })
            .collect()
    }

    pub fn get_stake_fee(
        origin: Option<(T::AccountId, NetUid)>,
        _origin_coldkey_account: T::AccountId,
        destination: Option<(T::AccountId, NetUid)>,
        _destination_coldkey_account: T::AccountId,
        amount: u64,
    ) -> u64 {
        if destination == origin {
            0_u64
        } else {
            let netuid = destination.or(origin).map(|v| v.1).unwrap_or_default();
            T::SwapInterface::approx_fee_amount(netuid.into(), TaoBalance::from(amount)).to_u64()
        }
    }
}
