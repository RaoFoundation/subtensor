#![allow(clippy::expect_used)]

use frame_support::{
    dispatch::{DispatchClass, GetDispatchInfo},
    traits::Hooks,
};
use node_subtensor_runtime::{BuildStorage, Runtime, RuntimeCall, RuntimeGenesisConfig, System};
use subtensor_runtime_common::{AccountId, TaoBalance};

#[test]
fn initialized_block_admits_shield_key_and_staking_test_setup_calls() {
    let mut ext: sp_io::TestExternalities = RuntimeGenesisConfig::default()
        .build_storage()
        .expect("runtime genesis builds")
        .into();
    ext.execute_with(|| {
        System::set_block_number(1);
        let hook = <pallet_subtensor::Pallet<Runtime> as Hooks<u32>>::on_initialize(1);
        System::register_extra_weight_unchecked(hook, DispatchClass::Mandatory);

        let calls = [
            RuntimeCall::MevShield(pallet_shield::Call::announce_next_key { enc_key: None }),
            RuntimeCall::Sudo(pallet_sudo::Call::sudo {
                call: Box::new(RuntimeCall::AdminUtils(
                    pallet_admin_utils::Call::sudo_set_lock_reduction_interval { interval: 0 },
                )),
            }),
            RuntimeCall::Sudo(pallet_sudo::Call::sudo {
                call: Box::new(RuntimeCall::Balances(
                    pallet_balances::Call::force_set_balance {
                        who: AccountId::new([1; 32]).into(),
                        new_free: TaoBalance::new(1_000_000_000),
                    },
                )),
            }),
            RuntimeCall::Balances(pallet_balances::Call::transfer_keep_alive {
                dest: AccountId::new([2; 32]).into(),
                value: TaoBalance::new(1_000_000),
            }),
        ];
        for call in calls {
            let info = call.get_dispatch_info();
            let (_, next_len) = frame_system::CheckWeight::<Runtime>::do_validate(&info, 256)
                .expect("call fits individual admission limit");
            frame_system::CheckWeight::<Runtime>::do_prepare(&info, 256, next_len)
                .expect("hook leaves capacity for Shield and user transactions");
        }
    });
}
