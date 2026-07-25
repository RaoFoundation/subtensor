#![allow(
    clippy::arithmetic_side_effects,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::unwrap_used
)]
//! Epoch term outputs for minimal registered-stake topologies.

use approx::assert_abs_diff_eq;
use sp_core::U256;
use subtensor_runtime_common::AlphaBalance;

use super::super::mock::*;
use crate::*;

#[test]
fn test_epoch_outputs_single_staker_registered_no_weights() {
    new_test_ext(1).execute_with(|| {
        let netuid = NetUid::from(1);
        let high_tempo: u16 = u16::MAX - 1; // Don't run automatically.
        add_network(netuid, high_tempo, 0);

        let hotkey = U256::from(1);
        let coldkey = U256::from(2);
        register_ok_neuron(netuid, hotkey, coldkey, 0);
        // Give non-zero alpha
        SubtensorModule::increase_stake_for_hotkey_and_coldkey_on_subnet(
            &hotkey,
            &coldkey,
            netuid,
            1.into(),
        );

        let pending_alpha = AlphaBalance::from(1_000_000_000);
        let hotkey_emission = SubtensorModule::epoch(netuid, pending_alpha);

        let sum_incentives = hotkey_emission
            .iter()
            .map(|(_, incentive, _)| incentive)
            .copied()
            .fold(AlphaBalance::ZERO, |acc, x| acc + x);
        let sum_dividends: AlphaBalance = hotkey_emission
            .iter()
            .map(|(_, _, dividend)| dividend)
            .copied()
            .fold(AlphaBalance::ZERO, |acc, x| acc + x);

        assert_abs_diff_eq!(
            sum_incentives.saturating_add(sum_dividends),
            pending_alpha,
            epsilon = 1_000.into()
        );
    });
}

// Map the retention graph for consensus guarantees with an single epoch on a graph with 512 nodes,
// of which the first 64 are validators, the graph is split into a major and minor set, each setting
// specific weight on itself and the complement on the other.
//
// ```import torch
// import matplotlib.pyplot as plt
// from matplotlib.pyplot import cm
// %matplotlib inline
//
// with open('finney_consensus_0.4.txt') as f:  # test output saved to finney_consensus.txt
//     retention_map = eval(f.read())
//
// major_ratios = {}
// avg_weight_devs = {}
// for major_stake, major_weight, minor_weight, avg_weight_dev, major_ratio in retention_map:
//     major_stake = f'{major_stake:.2f}'
//     maj, min = int(round(50 * major_weight)), int(round(50 * minor_weight))
//     avg_weight_devs.setdefault(major_stake, torch.zeros((51, 51)))
//     avg_weight_devs[major_stake][maj][min] = avg_weight_dev
//     major_ratios.setdefault(major_stake, torch.zeros((51, 51)))
//     major_ratios[major_stake][maj][min] = major_ratio
//
// _x = torch.linspace(0, 1, 51); _y = torch.linspace(0, 1, 51)
// x, y = torch.meshgrid(_x, _y, indexing='ij')
//
// fig = plt.figure(figsize=(6, 6), dpi=70); ax = fig.gca()
// ax.set_xticks(torch.arange(0, 1, 0.05)); ax.set_yticks(torch.arange(0, 1., 0.05))
// ax.set_xticklabels([f'{_:.2f}'[1:] for _ in torch.arange(0, 1., 0.05)])
// plt.grid(); plt.rc('grid', linestyle="dotted", color=[0.85, 0.85, 0.85])
//
// isolate = ['0.60']; stakes = [0.51, 0.55, 0.6, 0.65, 0.7, 0.75, 0.8, 0.85, 0.9, 0.95, 0.99]
// colors = cm.viridis(torch.linspace(0, 1, len(stakes) + 1))
// for i, stake in enumerate(stakes):
//     contours = plt.contour(x, y, major_ratios[f'{stake:.2f}'], levels=[0., stake], colors=[colors[i + 1]])
//     if f'{stake:.2f}' in isolate:
//         contours.collections[1].set_linewidth(3)
//     plt.clabel(contours, inline=True, fontsize=10)
//
// plt.title(f'Major emission [$stake_{{maj}}=emission_{{maj}}$ retention lines]')
// plt.ylabel('Minor self-weight'); plt.xlabel('Major self-weight'); plt.show()
// ```
// #[test]
// fn _map_consensus_guarantees() {
//     let netuid = NetUid::from(1);
//     let network_n: u16 = 512;
//     let validators_n: u16 = 64;
//     let epochs: u16 = 1;
//     let interleave = 0;
//     let weight_stddev: I32F32 = fixed(0.4);
//     let bonds_penalty: u16 = u16::MAX;
//     println!("[");
//     for _major_stake in vec![0.51, 0.55, 0.6, 0.65, 0.7, 0.75, 0.8, 0.85, 0.9, 0.95, 0.99] {
//         let major_stake: I32F32 = I32F32::from_num(_major_stake);
//         for _major_weight in 0..51 {
//             let major_weight: I32F32 = I32F32::from_num(50 - _major_weight) / I32F32::from_num(50);
//             for _minor_weight in 0..51 {
//                 let minor_weight: I32F32 =
//                     I32F32::from_num(50 - _minor_weight) / I32F32::from_num(50);
//                 let (
//                     validators,
//                     servers,
//                     major_validators,
//                     minor_validators,
//                     major_servers,
//                     minor_servers,
//                     stake,
//                     weights,
//                     avg_weight_dev,
//                 ) = split_graph(
//                     major_stake,
//                     major_weight,
//                     minor_weight,
//                     weight_stddev,
//                     validators_n as usize,
//                     network_n as usize,
//                     interleave as usize,
//                 );
//
//                 new_test_ext(1).execute_with(|| {
// 					init_run_epochs(netuid, network_n, &validators, &servers, epochs, 1, true, &stake, true, &weights, true, false, 0, true, bonds_penalty);
//
// 					let mut major_emission: I64F64 = I64F64::from_num(0);
// 					let mut minor_emission: I64F64 = I64F64::from_num(0);
// 					for set in vec![major_validators, major_servers] {
// 						for uid in set {
// 							major_emission += I64F64::from_num(SubtensorModule::get_emission_for_uid( netuid, uid ));
// 						}
// 					}
// 					for set in vec![minor_validators, minor_servers] {
// 						for uid in set {
// 							minor_emission += I64F64::from_num(SubtensorModule::get_emission_for_uid( netuid, uid ));
// 						}
// 					}
// 					let major_ratio: I32F32 = I32F32::from_num(major_emission / (major_emission + minor_emission));
// 					println!("[{major_stake}, {major_weight:.2}, {minor_weight:.2}, {avg_weight_dev:.3}, {major_ratio:.3}], ");
// 				});
//             }
//         }
//     }
//     println!("]");
// }

// Helpers
