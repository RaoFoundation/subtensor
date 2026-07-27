//! Phased dissolve cleanup for a single netuid.
//!
//! Advances [`DissolveCleanupStatus::phase`] until all subnet storage for that
//! netuid is gone or the weight meter is exhausted.

use super::*;
use frame_support::weights::WeightMeter;
use subtensor_swap_interface::SwapHandler;

impl<T: Config> Pallet<T> {
    /// Run dissolve cleanup phases for `status.netuid` until done or weight exhausted.
    pub fn clean_up_data_for_one_dissolved_network(
        weight_meter: &mut WeightMeter,
        status: &mut DissolveCleanupStatus,
    ) -> (bool, Weight) {
        let r = T::DbWeight::get().reads(1);

        let netuid = status.netuid;

        if !weight_meter.can_consume(r) {
            return (false, weight_meter.consumed());
        }

        // if one phase is done or exit because of weight limit
        let mut phase_done = true;
        let mut cleanup_completed = false;
        // only reason for phase_done to be false is if the weight limit is reached
        while phase_done {
            // let phase = status.phase.clone();
            log::debug!(
                "dissolved_networks phase: {:?} for netuid: {:?}",
                &status.phase,
                netuid
            );

            let done = match &status.phase {
                DissolveCleanupPhase::SubnetRootDividendsRootClaimable => {
                    let (done, new_key) = Self::clean_up_root_claimable_for_subnet(
                        netuid,
                        weight_meter,
                        status.last_key.clone(),
                    );

                    if done {
                        status.set_phase(DissolveCleanupPhase::SubnetRootDividendsRootClaimed);
                        status.last_key = None;
                    } else {
                        status.last_key = new_key;
                    }
                    done
                }

                DissolveCleanupPhase::SubnetRootDividendsRootClaimed => {
                    let done = Self::clean_up_root_claimed_for_subnet(netuid, weight_meter);

                    if done {
                        status.set_phase(DissolveCleanupPhase::AlphaInOutStakesGetTotalAlphaValue);
                        status.last_key = None;
                    }
                    done
                }

                DissolveCleanupPhase::AlphaInOutStakesGetTotalAlphaValue => {
                    let (done, new_key) = Self::destroy_alpha_in_out_stakes_get_total_alpha_value(
                        netuid,
                        weight_meter,
                        status.last_key.clone(),
                        status,
                    );
                    if done {
                        status.subnet_distributed_tao = Some(0);
                        status.set_phase(DissolveCleanupPhase::AlphaInOutStakesSettleStakes);
                        status.last_key = None;
                        weight_meter.consume(T::DbWeight::get().writes(2));
                    } else {
                        status.last_key = new_key;
                    }
                    done
                }

                DissolveCleanupPhase::AlphaInOutStakesSettleStakes => {
                    let (done, new_key) = Self::destroy_alpha_in_out_stakes_settle_stakes(
                        netuid,
                        weight_meter,
                        status.last_key.clone(),
                        status,
                    );
                    if done {
                        status.set_phase(DissolveCleanupPhase::AlphaInOutStakesAlpha);
                        status.last_key = None;
                    } else {
                        status.last_key = new_key;
                    }
                    done
                }

                DissolveCleanupPhase::AlphaInOutStakesAlpha => {
                    let (done, new_key) = Self::destroy_alpha_in_out_stakes_clean_alpha(
                        netuid,
                        weight_meter,
                        status.last_key.clone(),
                    );
                    if done {
                        status.set_phase(DissolveCleanupPhase::AlphaInOutStakesHotkeyTotals);
                        status.last_key = None;
                    } else {
                        status.last_key = new_key;
                    }
                    done
                }

                DissolveCleanupPhase::AlphaInOutStakesHotkeyTotals => {
                    let (done, new_key) = Self::destroy_alpha_in_out_stakes_clear_hotkey_totals(
                        netuid,
                        weight_meter,
                        status.last_key.clone(),
                    );

                    if done {
                        status.set_phase(DissolveCleanupPhase::AlphaInOutStakesLocks);
                        status.last_key = None;
                    } else {
                        status.last_key = new_key;
                    }
                    done
                }

                DissolveCleanupPhase::AlphaInOutStakesLocks => {
                    let (done, new_key) = Self::destroy_alpha_in_out_stakes_clear_locks(
                        netuid,
                        weight_meter,
                        status.last_key.clone(),
                    );
                    if done {
                        status.set_phase(DissolveCleanupPhase::AlphaInOutStakesDecayingLocks);
                        status.last_key = None;
                    } else {
                        status.last_key = new_key;
                    }
                    done
                }
                DissolveCleanupPhase::AlphaInOutStakesDecayingLocks => {
                    let (done, new_key) = Self::destroy_alpha_in_out_stakes_clear_decaying_locks(
                        netuid,
                        weight_meter,
                        status.last_key.clone(),
                    );
                    if done {
                        status.set_phase(DissolveCleanupPhase::AlphaInOutStakes);
                        status.last_key = None;
                    } else {
                        status.last_key = new_key;
                    }
                    done
                }

                DissolveCleanupPhase::AlphaInOutStakes => {
                    let done = Self::destroy_alpha_in_out_stakes(netuid, weight_meter, status);
                    if done {
                        status.set_phase(DissolveCleanupPhase::ProtocolLiquidity);
                        status.last_key = None;
                    }
                    done
                }

                DissolveCleanupPhase::ProtocolLiquidity => {
                    let done = T::SwapInterface::clear_protocol_liquidity(netuid, weight_meter);

                    if done {
                        status.set_phase(DissolveCleanupPhase::PurgeNetuid);
                        status.last_key = None;
                    }
                    done
                }

                DissolveCleanupPhase::PurgeNetuid => {
                    let done = T::CommitmentsInterface::purge_netuid(netuid, weight_meter);

                    if done {
                        status.set_phase(DissolveCleanupPhase::NetworkIsNetworkMember);
                        status.last_key = None;
                    }
                    done
                }
                DissolveCleanupPhase::NetworkIsNetworkMember => {
                    let (done, new_key) = Self::remove_network_is_network_member(
                        netuid,
                        weight_meter,
                        status.last_key.clone(),
                    );

                    if done {
                        status.set_phase(DissolveCleanupPhase::NetworkParameters);
                        status.last_key = None;
                    } else {
                        status.last_key = new_key;
                    }
                    done
                }
                DissolveCleanupPhase::NetworkParameters => {
                    let done = Self::remove_network_parameters(netuid, weight_meter);

                    if done {
                        status.set_phase(DissolveCleanupPhase::NetworkMapParameters);
                        status.last_key = None;
                    }
                    done
                }
                DissolveCleanupPhase::NetworkMapParameters => {
                    let done = Self::remove_network_map_parameters(netuid, weight_meter);

                    if done {
                        status.set_phase(DissolveCleanupPhase::NetworkUpdateWeightsOnRoot);
                        status.last_key = None;
                    }
                    done
                }
                DissolveCleanupPhase::NetworkUpdateWeightsOnRoot => {
                    let (done, new_key) = Self::remove_network_update_weights_on_root(
                        netuid,
                        weight_meter,
                        status.last_key.clone(),
                    );

                    if done {
                        status.set_phase(DissolveCleanupPhase::NetworkChildkeyTake);
                        status.last_key = None;
                    } else {
                        status.last_key = new_key;
                    }
                    done
                }
                DissolveCleanupPhase::NetworkChildkeyTake => {
                    let (done, new_key) = Self::remove_network_childkey_take(
                        netuid,
                        weight_meter,
                        status.last_key.clone(),
                    );

                    if done {
                        status.set_phase(DissolveCleanupPhase::NetworkChildkeys);
                        status.last_key = None;
                    } else {
                        status.last_key = new_key;
                    }
                    done
                }
                DissolveCleanupPhase::NetworkChildkeys => {
                    let (done, new_key) = Self::remove_network_childkeys(
                        netuid,
                        weight_meter,
                        status.last_key.clone(),
                    );

                    if done {
                        status.set_phase(DissolveCleanupPhase::NetworkParentkeys);
                        status.last_key = None;
                    } else {
                        status.last_key = new_key;
                    }
                    done
                }
                DissolveCleanupPhase::NetworkParentkeys => {
                    let (done, new_key) = Self::remove_network_parentkeys(
                        netuid,
                        weight_meter,
                        status.last_key.clone(),
                    );

                    if done {
                        status.set_phase(DissolveCleanupPhase::NetworkLastHotkeyEmissionOnNetuid);
                        status.last_key = None;
                    } else {
                        status.last_key = new_key;
                    }
                    done
                }
                DissolveCleanupPhase::NetworkLastHotkeyEmissionOnNetuid => {
                    let (done, new_key) = Self::remove_network_last_hotkey_emission_on_netuid(
                        netuid,
                        weight_meter,
                        status.last_key.clone(),
                    );

                    if done {
                        status.set_phase(DissolveCleanupPhase::NetworkTotalHotkeyAlphaLastEpoch);
                        status.last_key = None;
                    } else {
                        status.last_key = new_key;
                    }
                    done
                }
                DissolveCleanupPhase::NetworkTotalHotkeyAlphaLastEpoch => {
                    let (done, new_key) = Self::remove_network_total_hotkey_alpha_last_epoch(
                        netuid,
                        weight_meter,
                        status.last_key.clone(),
                    );

                    if done {
                        status.set_phase(DissolveCleanupPhase::NetworkTransactionKeyLastBlock);
                        status.last_key = None;
                    } else {
                        status.last_key = new_key;
                    }
                    done
                }
                DissolveCleanupPhase::NetworkTransactionKeyLastBlock => {
                    let (done, new_key) = Self::remove_network_transaction_key_last_block(
                        netuid,
                        weight_meter,
                        status.last_key.clone(),
                    );
                    if done {
                        status.set_phase(DissolveCleanupPhase::NetworkLock);
                        status.last_key = None;
                    } else {
                        status.last_key = new_key;
                    }
                    done
                }
                DissolveCleanupPhase::NetworkLock => {
                    let (done, new_key) =
                        Self::remove_network_lock(netuid, weight_meter, status.last_key.clone());

                    if done {
                        status.set_phase(DissolveCleanupPhase::NetworkDecayingLock);
                        status.last_key = None;
                    } else {
                        status.last_key = new_key;
                    }
                    done
                }
                DissolveCleanupPhase::NetworkDecayingLock => {
                    let (done, new_key) = Self::remove_network_decaying_lock(
                        netuid,
                        weight_meter,
                        status.last_key.clone(),
                    );

                    // if all phases are done, remove the network from the dissolved networks list and emit the event
                    if done {
                        cleanup_completed = true;
                    } else {
                        status.last_key = new_key;
                    }
                    done
                }
            };

            phase_done = done;

            if cleanup_completed {
                Self::deposit_event(Event::NetworkDissolveCleanupCompleted { netuid });
                break;
            }

            CurrentDissolveCleanupStatus::<T>::set(Some(status.clone()));
        }

        (cleanup_completed, weight_meter.consumed())
    }
}
