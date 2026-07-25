//! Integration and unit tests for `pallet-subtensor`.
//!
//! Flat files cover single concepts; split directories (`mod foo;` → `foo/mod.rs`)
//! hold the larger suites that were broken out by domain.
//!
//! ## Search anchors — split directories
//!
//! | Module | Owns |
//! |--------|------|
//! | [`math`] | Fixed-point helpers mirroring `epoch/math/` |
//! | [`weights`] | `set_weights`, commit–reveal, timelocked CRv3 |
//! | [`staking`] | Add/remove/move stake, take, share pools |
//! | [`migration`] | Storage / share-pool / coldkey migrations |
//! | [`locks`] | Stake locks, transfer, unlock schedules |
//! | [`children`] | Parent/child key maps and pending children |
//! | [`coinbase`] | Block-step emission, root / subnet coinbase |
//! | [`epoch`] | Yuma epoch / bonds / consensus timing |
//! | [`networks`] | Register / dissolve / prune / registration queue |
//! | [`swap_hotkey_with_subnet`] | Subnet-scoped hotkey swap |
//!
//! ## Search anchors — flat modules
//!
//! | Module | Owns |
//! |--------|------|
//! | [`mock`] / [`mock_high_ed`] | Test runtime + fixtures (`new_test_ext`, networks, stake) |
//! | [`auto_stake_hotkey`] | `set_coldkey_auto_stake_hotkey` |
//! | [`batch_tx`] | Utility `batch` nesting / allow-list |
//! | [`claim_root`] | Root alpha claim / thresholds |
//! | [`cleanup_tests`] | `remove_storage_entries_for_netuid` weight budgeting |
//! | [`coldkey_lineage`] / [`hotkey_lineage`] | Swap lineage recording |
//! | [`consensus`] | Synthetic consensus / map-consensus stress |
//! | [`delegate_info`] | RPC delegate info / return-per-1000 |
//! | [`destroy_alpha_tests`] | Dissolve-path destroy alpha in/out |
//! | [`dissolution`] | Subnet dissolve cleanup / netuid reuse |
//! | [`emission`] | `blocks_until_next_auto_epoch` |
//! | [`ensure`] | Subnet-owner / admin-window origin guards |
//! | [`epoch_logs`] | Epoch trace-log assertions |
//! | [`evm`] | `associate_evm_key` |
//! | [`leasing`] | Subnet leasing |
//! | [`mechanism`] | Multi-mechanism subnet state |
//! | [`move_stake`] | Cross-subnet / hotkey move stake |
//! | [`neuron_info`] | RPC neuron info getters |
//! | [`recycle_alpha`] | Alpha recycle into subnet |
//! | [`registration`] | Neuron / burned registration |
//! | [`remove_data_tests`] | Hotkey/coldkey data purge |
//! | [`serving`] | Axon / prometheus serve + identity |
//! | [`staking2`] | Dynamic-mechanism stake / swap paths |
//! | [`subnet`] | `do_start_call`, symbols, subnet lifecycle |
//! | [`subnet_emissions`] | Subnet emission share math |
//! | [`subnet_info`] | RPC hyperparams V3 |
//! | [`swap_coldkey`] / [`swap_hotkey`] | Full-account key swaps |
//! | [`tao`] | TAO issuance / high existential-deposit edge cases |
//! | [`tempo_control`] | Tempo / trigger-epoch / activity cutoff |
//! | [`uids`] | `replace_neuron` and uid maps |
//! | [`voting_power`] | Voting-power EMA tracking |

mod auto_stake_hotkey;
mod batch_tx;
mod children;
mod claim_root;
mod cleanup_tests;
mod coinbase;
mod coldkey_lineage;
mod consensus;
mod delegate_info;
mod destroy_alpha_tests;
mod dissolution;
mod emission;
mod ensure;
mod epoch;
mod epoch_logs;
mod evm;
mod hotkey_lineage;
mod leasing;
mod locks;
mod math;
mod mechanism;
mod migration;
pub(crate) mod mock;
pub(crate) mod mock_high_ed;
mod move_stake;
mod networks;
mod neuron_info;
mod recycle_alpha;
mod registration;
mod remove_data_tests;
mod serving;
mod staking;
mod staking2;
mod subnet;
mod subnet_emissions;
mod subnet_info;
mod swap_coldkey;
mod swap_hotkey;
mod swap_hotkey_with_subnet;
mod tao;
mod tempo_control;
mod uids;
mod voting_power;
mod weights;
