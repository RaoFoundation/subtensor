use super::*;
extern crate alloc;
use codec::Compact;
use frame_support::pallet_prelude::{Decode, Encode};
use substrate_fixed::types::I96F32;
use subtensor_macros::freeze_struct;
use subtensor_runtime_common::{AlphaBalance, NetUid, TaoBalance};

#[freeze_struct("cf677afa654c96a6")]
#[derive(Decode, Encode, PartialEq, Eq, Clone, Debug, TypeInfo)]
pub struct DynamicInfo<AccountId: TypeInfo + Encode + Decode> {
    netuid: Compact<NetUid>,
    owner_hotkey: AccountId,
    owner_coldkey: AccountId,
    subnet_name: Vec<Compact<u8>>,
    token_symbol: Vec<Compact<u8>>,
    tempo: Compact<u16>,
    last_step: Compact<u64>,
    blocks_since_last_step: Compact<u64>,
    emission: Compact<u64>,
    alpha_in: Compact<AlphaBalance>,
    alpha_out: Compact<AlphaBalance>,
    tao_in: Compact<TaoBalance>,
    alpha_out_emission: Compact<AlphaBalance>,
    alpha_in_emission: Compact<AlphaBalance>,
    tao_in_emission: Compact<TaoBalance>,
    pending_alpha_emission: Compact<AlphaBalance>,
    pending_root_emission: Compact<TaoBalance>,
    subnet_volume: Compact<u128>,
    network_registered_at: Compact<u64>,
    subnet_identity: Option<SubnetIdentityV3>,
    moving_price: I96F32,
}

impl<T: Config> Pallet<T> {
    pub fn get_dynamic_info(netuid: NetUid) -> Option<DynamicInfo<T::AccountId>> {
        if !Self::if_subnet_exist(netuid) {
            return None;
        }
        let last_step: u64 = LastMechansimStepBlock::<T>::get(netuid);
        let current_block: u64 = Pallet::<T>::get_current_block_as_u64();
        let blocks_since_last_step: u64 = current_block.saturating_sub(last_step);
        Some(DynamicInfo {
            netuid: netuid.into(),
            owner_hotkey: SubnetOwnerHotkey::<T>::get(netuid),
            owner_coldkey: SubnetOwner::<T>::get(netuid),
            subnet_name: Self::get_name_for_subnet(netuid)
                .into_iter()
                .map(Compact)
                .collect(),
            token_symbol: TokenSymbol::<T>::get(netuid)
                .into_iter()
                .map(Compact)
                .collect(),
            tempo: Tempo::<T>::get(netuid).into(),
            last_step: last_step.into(),
            blocks_since_last_step: blocks_since_last_step.into(),
            emission: u64::from(SubnetTaoEmission::<T>::get(netuid)).into(),
            alpha_in: SubnetAlphaIn::<T>::get(netuid).into(),
            alpha_out: SubnetAlphaOut::<T>::get(netuid).into(),
            tao_in: SubnetTAO::<T>::get(netuid).into(),
            alpha_out_emission: SubnetAlphaOutEmission::<T>::get(netuid).into(),
            alpha_in_emission: SubnetAlphaInEmission::<T>::get(netuid).into(),
            tao_in_emission: SubnetTaoInEmission::<T>::get(netuid).into(),
            pending_alpha_emission: PendingValidatorEmission::<T>::get(netuid)
                .saturating_add(PendingServerEmission::<T>::get(netuid))
                .into(),
            pending_root_emission: TaoBalance::ZERO.into(),
            subnet_volume: SubnetVolume::<T>::get(netuid).into(),
            network_registered_at: NetworkRegisteredAt::<T>::get(netuid).into(),
            subnet_identity: SubnetIdentitiesV3::<T>::get(netuid),
            moving_price: SubnetMovingPrice::<T>::get(netuid),
        })
    }
    pub fn get_all_dynamic_info() -> Vec<Option<DynamicInfo<T::AccountId>>> {
        let netuids = Self::get_all_subnet_netuids();
        let mut dynamic_info = Vec::<Option<DynamicInfo<T::AccountId>>>::new();
        for netuid in netuids.clone().iter() {
            dynamic_info.push(Self::get_dynamic_info(*netuid));
        }
        dynamic_info
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::mock::{SubtensorModule, Test, add_dynamic_network, new_test_ext};
    use sp_core::U256;

    #[test]
    fn dynamic_info_reports_final_tao_emission() {
        new_test_ext(1).execute_with(|| {
            let netuid = add_dynamic_network(&U256::from(1), &U256::from(2));
            let final_emission = TaoBalance::from(123_u64);
            SubnetTaoEmission::<Test>::insert(netuid, final_emission);

            let info = SubtensorModule::get_dynamic_info(netuid).unwrap();

            assert_eq!(info.emission.0, u64::from(final_emission));
        });
    }
}
