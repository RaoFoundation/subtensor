use codec::{Decode, DecodeWithMemTracking, Encode, MaxEncodedLen};
use frame_support::{traits::fungible::Inspect, weights::Weight};
use pallet_subtensor::{
    Pallet as Subtensor,
    root_registered::{EmaValueProvider, SampleStep},
    *,
};
use scale_info::TypeInfo;
use sp_runtime::traits::UniqueSaturatedInto;
use substrate_fixed::types::U64F64;
use subtensor_runtime_common::NetUid;
use subtensor_swap_interface::{Order, SwapHandler};

use super::weights::WeightInfo;
use crate::{AccountId, Runtime};

/// Number of subnets folded into the stake-value accumulator per tick.
pub(crate) const STAKE_CHUNK_SUBNETS: u32 = 8;

/// Maximum owned hotkeys valued for one governance stake EMA sample.
pub(crate) const STAKE_VALUE_HOTKEYS: u32 = 256;

/// Provider-owned progress for the governance stake-value EMA.
#[subtensor_macros::freeze_struct("1a8d9e6e7d73e9d3")]
#[derive(
    Clone,
    Copy,
    Default,
    PartialEq,
    Eq,
    Debug,
    Encode,
    Decode,
    DecodeWithMemTracking,
    MaxEncodedLen,
    TypeInfo,
)]
pub struct StakeValueProgress {
    /// Subnet offset processed so far.
    pub subnet_offset: u32,
    /// Running TAO accumulator for processed subnet chunks.
    pub accumulated_tao: u128,
}

/// Governance stake-value provider: each root-registered coldkey's sample
/// is the TAO value of its liquid balance plus the alpha held across all
/// owned hotkeys on every subnet.
pub struct StakeValueProvider;

impl StakeValueProvider {
    fn subnet_chunk(netuids: &[NetUid], offset: u32) -> &[NetUid] {
        let start: usize = offset.unique_saturated_into();
        let start = start.min(netuids.len());
        let netuids_len: u32 = netuids.len().unique_saturated_into();
        let end: usize = offset
            .saturating_add(STAKE_CHUNK_SUBNETS)
            .min(netuids_len)
            .unique_saturated_into();
        netuids.get(start..end).unwrap_or_default()
    }

    fn accumulate_subnet_values(
        hotkeys: &[AccountId],
        netuids: &[NetUid],
        accumulated_tao: u128,
    ) -> u128 {
        netuids.iter().fold(accumulated_tao, |total, netuid| {
            total.saturating_add(Self::tao_for_subnet_hotkeys(hotkeys, *netuid))
        })
    }

    fn tao_for_subnet_hotkeys(hotkeys: &[AccountId], netuid: NetUid) -> u128 {
        let hotkey_limit: usize = STAKE_VALUE_HOTKEYS.unique_saturated_into();
        let total_alpha = hotkeys
            .iter()
            .take(hotkey_limit)
            .fold(0_u128, |total, hotkey| {
                let alpha = Subtensor::<Runtime>::get_stake_for_hotkey_on_subnet(hotkey, netuid);
                total.saturating_add(u128::from(u64::from(alpha)))
            });

        if total_alpha == 0 {
            return 0;
        }

        let aggregated: u64 = total_alpha
            .min(u128::from(u64::MAX))
            .unique_saturated_into();
        let order = GetTaoForAlpha::<Runtime>::with_amount(aggregated);
        <Runtime as Config>::SwapInterface::sim_swap(netuid.into(), order)
            .map(|r| u128::from(u64::from(r.amount_paid_out)))
            .unwrap_or_default()
    }
}

impl EmaValueProvider<AccountId> for StakeValueProvider {
    type Progress = StakeValueProgress;

    /// Advances one chunk of subnet valuation for `coldkey`, carrying the
    /// accumulated TAO value in `Progress` until all subnets are sampled.
    fn step(coldkey: &AccountId, progress: Self::Progress) -> (SampleStep<Self::Progress>, Weight) {
        let netuids = Subtensor::<Runtime>::get_all_subnet_netuids();
        let total: u32 = netuids.len().unique_saturated_into();
        let hotkeys = OwnedHotkeys::<Runtime>::get(coldkey);

        let mut next = progress;
        if next.subnet_offset < total {
            let chunk = Self::subnet_chunk(&netuids, next.subnet_offset);
            next.accumulated_tao =
                Self::accumulate_subnet_values(&hotkeys, chunk, next.accumulated_tao);
            let chunk_len: u32 = chunk.len().unique_saturated_into();
            next.subnet_offset = next.subnet_offset.saturating_add(chunk_len).min(total);
        }

        let step = if next.subnet_offset >= total {
            let liquid = u128::from(u64::from(<Runtime as Config>::Currency::balance(coldkey)));
            let sample = U64F64::saturating_from_num(next.accumulated_tao.saturating_add(liquid));
            SampleStep::Complete { sample }
        } else {
            SampleStep::Continue { progress: next }
        };

        (step, Self::step_weight())
    }

    fn step_weight() -> Weight {
        super::weights::SubstrateWeight::<Runtime>::stake_ema_provider_step()
    }
}

#[cfg(test)]
#[allow(clippy::indexing_slicing)]
mod tests {
    use super::*;

    use frame_support::traits::fungible::Mutate;
    use sp_runtime::BuildStorage;
    use subtensor_runtime_common::{AlphaBalance, TaoBalance};

    fn new_test_ext() -> sp_io::TestExternalities {
        let storage = match (crate::RuntimeGenesisConfig {
            sudo: pallet_sudo::GenesisConfig { key: None },
            ..Default::default()
        })
        .build_storage()
        {
            Ok(storage) => storage,
            Err(err) => panic!("failed to build test storage: {err:?}"),
        };
        let mut ext: sp_io::TestExternalities = storage.into();
        ext.execute_with(|| crate::System::set_block_number(1));
        ext
    }

    fn account(seed: u8) -> AccountId {
        AccountId::from([seed; 32])
    }

    fn indexed_account(index: u32) -> AccountId {
        let mut bytes = [0; 32];
        bytes[..4].copy_from_slice(&index.to_le_bytes());
        AccountId::from(bytes)
    }

    fn add_balance(coldkey: &AccountId, amount: u64) {
        assert!(
            <Runtime as Config>::Currency::mint_into(coldkey, TaoBalance::from(amount)).is_ok()
        );
    }

    fn seed_subnet(netuid: NetUid) {
        Subtensor::<Runtime>::init_new_network(netuid, 1);
        SubtokenEnabled::<Runtime>::insert(netuid, true);
        SubnetTAO::<Runtime>::insert(netuid, TaoBalance::from(1_000_000_000_u64));
        SubnetAlphaIn::<Runtime>::insert(netuid, AlphaBalance::from(1_000_000_000_u64));
    }

    fn progress_at(netuid: NetUid, accumulated_tao: u128) -> StakeValueProgress {
        let netuids = Subtensor::<Runtime>::get_all_subnet_netuids();
        let Some(offset) = netuids.iter().position(|candidate| *candidate == netuid) else {
            panic!("seeded subnet {netuid:?} is not in the subnet list");
        };
        StakeValueProgress {
            subnet_offset: offset as u32,
            accumulated_tao,
        }
    }

    fn complete_sample(step: SampleStep<StakeValueProgress>) -> U64F64 {
        match step {
            SampleStep::Complete { sample } => sample,
            SampleStep::Continue { progress } => {
                panic!("expected complete sample, got progress {progress:?}")
            }
        }
    }

    fn continued_progress(step: SampleStep<StakeValueProgress>) -> StakeValueProgress {
        match step {
            SampleStep::Continue { progress } => progress,
            SampleStep::Complete { sample } => {
                panic!("expected continued sample, got complete sample {sample:?}")
            }
        }
    }

    #[test]
    fn step_completes_with_liquid_balance_when_there_are_no_subnets() {
        new_test_ext().execute_with(|| {
            let coldkey = account(1);
            add_balance(&coldkey, 1_000);

            let (step, weight) = StakeValueProvider::step(&coldkey, StakeValueProgress::default());

            assert_eq!(complete_sample(step), U64F64::from_num(1_000));
            assert_eq!(weight, StakeValueProvider::step_weight());
        });
    }

    #[test]
    fn step_continues_after_one_subnet_chunk_when_more_subnets_remain() {
        new_test_ext().execute_with(|| {
            let coldkey = account(1);
            for index in 0..=STAKE_CHUNK_SUBNETS {
                seed_subnet(NetUid::from(1_000_u16 + index as u16));
            }

            let (step, weight) = StakeValueProvider::step(&coldkey, StakeValueProgress::default());
            let progress = continued_progress(step);

            assert_eq!(progress.subnet_offset, STAKE_CHUNK_SUBNETS);
            assert_eq!(progress.accumulated_tao, 0);
            assert_eq!(weight, StakeValueProvider::step_weight());
        });
    }

    #[test]
    fn step_accumulates_multiple_chunks_with_many_hotkeys_until_complete() {
        new_test_ext().execute_with(|| {
            let coldkey = account(1);
            let hotkeys = vec![account(2), account(3), account(4), account(5)];
            let unowned_hotkey = account(6);
            let liquid = 1_000_u128;
            add_balance(&coldkey, liquid as u64);
            OwnedHotkeys::<Runtime>::insert(&coldkey, hotkeys.clone());

            let subnet_count = STAKE_CHUNK_SUBNETS * 2 + 1;
            for index in 0..subnet_count {
                seed_subnet(NetUid::from(1_000_u16 + index as u16));
            }

            let netuids = Subtensor::<Runtime>::get_all_subnet_netuids();
            assert!(netuids.len() > (STAKE_CHUNK_SUBNETS * 2) as usize);
            assert!(netuids.len() <= (STAKE_CHUNK_SUBNETS * 3) as usize);

            let expected_by_subnet = netuids
                .iter()
                .enumerate()
                .map(|(subnet_index, netuid)| {
                    let total_owned_alpha =
                        hotkeys
                            .iter()
                            .enumerate()
                            .fold(0_u64, |total, (hotkey_index, hotkey)| {
                                let alpha =
                                    ((subnet_index as u64) + 1) * ((hotkey_index as u64) + 1) * 10;
                                TotalHotkeyAlpha::<Runtime>::insert(
                                    hotkey.clone(),
                                    *netuid,
                                    AlphaBalance::from(alpha),
                                );
                                total + alpha
                            });
                    TotalHotkeyAlpha::<Runtime>::insert(
                        unowned_hotkey.clone(),
                        *netuid,
                        AlphaBalance::from(1_000_000_u64),
                    );
                    assert!(total_owned_alpha > 0);
                    StakeValueProvider::tao_for_subnet_hotkeys(&hotkeys, *netuid)
                })
                .collect::<Vec<_>>();

            let first_chunk_end = STAKE_CHUNK_SUBNETS as usize;
            let second_chunk_end = (STAKE_CHUNK_SUBNETS * 2) as usize;
            let expected_first_chunk = expected_by_subnet[..first_chunk_end]
                .iter()
                .copied()
                .sum::<u128>();
            let expected_second_chunk = expected_by_subnet[first_chunk_end..second_chunk_end]
                .iter()
                .copied()
                .sum::<u128>();
            let expected_final_chunk = expected_by_subnet[second_chunk_end..]
                .iter()
                .copied()
                .sum::<u128>();

            let (step, weight) = StakeValueProvider::step(&coldkey, StakeValueProgress::default());
            let progress = continued_progress(step);
            assert_eq!(weight, StakeValueProvider::step_weight());
            assert_eq!(progress.subnet_offset, STAKE_CHUNK_SUBNETS);
            assert_eq!(progress.accumulated_tao, expected_first_chunk);

            let (step, weight) = StakeValueProvider::step(&coldkey, progress);
            let progress = continued_progress(step);
            assert_eq!(weight, StakeValueProvider::step_weight());
            assert_eq!(progress.subnet_offset, STAKE_CHUNK_SUBNETS * 2);
            assert_eq!(
                progress.accumulated_tao,
                expected_first_chunk + expected_second_chunk
            );

            let (step, weight) = StakeValueProvider::step(&coldkey, progress);
            assert_eq!(weight, StakeValueProvider::step_weight());
            assert_eq!(
                complete_sample(step),
                U64F64::from_num(
                    expected_first_chunk + expected_second_chunk + expected_final_chunk + liquid,
                )
            );
        });
    }

    #[test]
    fn step_completes_from_resumed_progress_and_adds_liquid_balance() {
        new_test_ext().execute_with(|| {
            let coldkey = account(1);
            add_balance(&coldkey, 1_000);

            let progress = StakeValueProgress {
                subnet_offset: u32::MAX,
                accumulated_tao: 12,
            };
            let (step, _) = StakeValueProvider::step(&coldkey, progress);

            assert_eq!(complete_sample(step), U64F64::from_num(1_012));
        });
    }

    #[test]
    fn step_aggregates_owned_hotkey_alpha_for_the_current_subnet() {
        new_test_ext().execute_with(|| {
            let coldkey = account(1);
            let hotkey_a = account(2);
            let hotkey_b = account(3);
            let hotkeys = vec![hotkey_a.clone(), hotkey_b.clone()];
            let unowned_hotkey = account(4);
            let netuid = NetUid::from(1_000);
            seed_subnet(netuid);

            OwnedHotkeys::<Runtime>::insert(&coldkey, hotkeys.clone());
            TotalHotkeyAlpha::<Runtime>::insert(hotkey_a, netuid, AlphaBalance::from(100_u64));
            TotalHotkeyAlpha::<Runtime>::insert(hotkey_b, netuid, AlphaBalance::from(200_u64));
            TotalHotkeyAlpha::<Runtime>::insert(
                unowned_hotkey,
                netuid,
                AlphaBalance::from(900_u64),
            );

            let expected = StakeValueProvider::tao_for_subnet_hotkeys(&hotkeys, netuid);
            let (step, _) = StakeValueProvider::step(&coldkey, progress_at(netuid, 0));

            assert_eq!(complete_sample(step), U64F64::from_num(expected));
        });
    }

    #[test]
    fn step_values_only_the_governance_hotkey_limit() {
        new_test_ext().execute_with(|| {
            let coldkey = account(1);
            let netuid = NetUid::from(1_000);
            seed_subnet(netuid);

            let hotkeys = (0..=STAKE_VALUE_HOTKEYS)
                .map(|index| indexed_account(index + 10))
                .collect::<Vec<_>>();
            OwnedHotkeys::<Runtime>::insert(&coldkey, hotkeys.clone());

            for (index, hotkey) in hotkeys.iter().enumerate() {
                let alpha = if index < STAKE_VALUE_HOTKEYS as usize {
                    1_u64
                } else {
                    1_000_000_000_u64
                };
                TotalHotkeyAlpha::<Runtime>::insert(
                    hotkey.clone(),
                    netuid,
                    AlphaBalance::from(alpha),
                );
            }

            let expected = StakeValueProvider::tao_for_subnet_hotkeys(
                &hotkeys[..STAKE_VALUE_HOTKEYS as usize],
                netuid,
            );
            let (step, _) = StakeValueProvider::step(&coldkey, progress_at(netuid, 0));

            assert_eq!(complete_sample(step), U64F64::from_num(expected));
        });
    }

    #[test]
    fn step_carries_existing_accumulator_through_zero_alpha_subnets() {
        new_test_ext().execute_with(|| {
            let coldkey = account(1);
            let netuid = NetUid::from(1_000);
            seed_subnet(netuid);

            let (step, _) = StakeValueProvider::step(&coldkey, progress_at(netuid, 77));

            assert_eq!(complete_sample(step), U64F64::from_num(77));
        });
    }
}
