#![cfg(not(feature = "runtime-benchmarks"))]
#![allow(clippy::expect_used)]

use frame_support::{assert_noop, assert_ok, dispatch::GetDispatchInfo, traits::fungible::Mutate};
use node_subtensor_runtime::{
    AllowCommitments, Balances, BuildStorage, Commitments, Runtime, RuntimeCall,
    RuntimeGenesisConfig, RuntimeOrigin, System, TxExtension, check_mortality, check_nonce,
    check_nonzero_sender, sudo_wrapper,
    transaction_payment_wrapper::ChargeTransactionPaymentWrapper,
};
use pallet_commitments::{CanCommit, CommitmentInfo, Data};
use pallet_subtensor::{self as st, Pallet as Subtensor};
use sp_runtime::{
    generic::Era,
    traits::{DispatchTransaction, Dispatchable, TransactionExtension},
};
use subtensor_runtime_common::{AccountId, NetUid, TaoBalance};

fn hot() -> AccountId {
    AccountId::new([7; 32])
}
fn cold() -> AccountId {
    AccountId::new([8; 32])
}
fn net() -> NetUid {
    NetUid::from(1)
}
fn info() -> Box<CommitmentInfo<<Runtime as pallet_commitments::Config>::MaxFields>> {
    Box::new(CommitmentInfo {
        fields: vec![Data::Raw(vec![1, 2, 3].try_into().expect("small blob"))]
            .try_into()
            .expect("one field"),
    })
}
fn new_test_ext() -> sp_io::TestExternalities {
    let mut ext: sp_io::TestExternalities = RuntimeGenesisConfig::default()
        .build_storage()
        .expect("genesis")
        .into();
    ext.execute_with(|| {
        System::set_block_number(2);
        Subtensor::<Runtime>::init_new_network(net(), 5);
        assert_ok!(Subtensor::<Runtime>::do_set_null_consensus(net(), true));
        st::NetworkPowRegistrationAllowed::<Runtime>::insert(net(), true);
        Subtensor::<Runtime>::set_difficulty(net(), 1);
        let work = Subtensor::<Runtime>::create_null_seal_hash(net(), 1, 0, &hot(), &cold())
            .as_bytes()
            .to_vec();
        assert_ok!(Subtensor::<Runtime>::do_null_pow_register(
            RuntimeOrigin::signed(cold()),
            net(),
            1,
            0,
            work,
            hot(),
            cold()
        ));
    });
    ext
}
fn submit(nonce: u32) {
    let ext: TxExtension = (
        (
            check_nonzero_sender::CheckNonZeroSender::<Runtime>::new(),
            frame_system::CheckSpecVersion::<Runtime>::new(),
            frame_system::CheckTxVersion::<Runtime>::new(),
            frame_system::CheckGenesis::<Runtime>::new(),
            check_mortality::CheckMortality::<Runtime>::from(Era::Immortal),
            check_nonce::CheckNonce::<Runtime>::from(nonce),
            frame_system::CheckWeight::<Runtime>::new(),
        ),
        (
            ChargeTransactionPaymentWrapper::<Runtime>::new(TaoBalance::new(0)),
            sudo_wrapper::SudoTransactionExtension::<Runtime>::new(),
            pallet_shield::CheckShieldedTxValidity::<Runtime>::new(),
            st::SubtensorTransactionExtension::<Runtime>::new(),
            pallet_drand::drand_priority::DrandPriority::<Runtime>::new(),
        ),
        frame_metadata_hash_extension::CheckMetadataHash::<Runtime>::new(false),
    );
    let call = RuntimeCall::Commitments(pallet_commitments::Call::set_commitment {
        netuid: net(),
        info: info(),
    });
    let mut dispatch_info = call.get_dispatch_info();
    dispatch_info.extension_weight = ext.weight(&call);
    assert_ok!(
        ext.test_run(
            RuntimeOrigin::signed(hot()),
            &call,
            &dispatch_info,
            0,
            0,
            |origin| call.clone().dispatch(origin)
        )
        .expect("transaction accepted")
    );
}

#[test]
fn fresh_null_hotkey_can_commit_and_renew_quota_without_yuma_epochs() {
    new_test_ext().execute_with(|| {
        assert!(!st::Owner::<Runtime>::contains_key(hot()));
        assert!(!frame_system::Account::<Runtime>::contains_key(hot()));
        assert!(!Subtensor::<Runtime>::is_hotkey_registered_on_network(
            net(),
            &hot()
        ));
        pallet_commitments::MaxSpace::<Runtime>::put(100);
        let epoch = st::SubnetEpochIndex::<Runtime>::get(net());
        submit(0);
        assert!(!Commitments::get_commitments(net()).is_empty());
        assert_noop!(
            Commitments::set_commitment(RuntimeOrigin::signed(hot()), net(), info()),
            pallet_commitments::Error::<Runtime>::SpaceLimitExceeded
        );
        System::set_block_number(6);
        submit(1);
        assert_eq!(st::SubnetEpochIndex::<Runtime>::get(net()), epoch);
        assert_eq!(Balances::free_balance(hot()), TaoBalance::new(0));
        assert_eq!(Balances::free_balance(cold()), TaoBalance::new(0));
    });
}

#[test]
fn commitment_membership_follows_the_active_mode_and_hotkey_rotation() {
    new_test_ext().execute_with(|| {
        assert_ok!(AllowCommitments::validate(net(), &hot()));
        assert_ok!(Subtensor::<Runtime>::do_set_null_consensus(net(), false));
        assert_noop!(
            Commitments::set_commitment(RuntimeOrigin::signed(hot()), net(), info()),
            pallet_commitments::Error::<Runtime>::AccountNotAllowedCommit
        );
        assert_ok!(Subtensor::<Runtime>::do_set_null_consensus(net(), true));
        assert_ok!(Balances::mint_into(
            &cold(),
            TaoBalance::new(1_000_000_000_000)
        ));
        let new = AccountId::new([9; 32]);
        assert_ok!(Subtensor::<Runtime>::do_swap_hotkey(
            RuntimeOrigin::signed(cold()),
            &hot(),
            &new,
            Some(net()),
            false
        ));
        // A stale Yuma UID must not authorize the retired null hotkey.
        Subtensor::<Runtime>::append_neuron(net(), &hot(), 1);
        assert_noop!(
            Commitments::set_commitment(RuntimeOrigin::signed(hot()), net(), info()),
            pallet_commitments::Error::<Runtime>::AccountNotAllowedCommit
        );
        assert_ok!(Commitments::set_commitment(
            RuntimeOrigin::signed(new),
            net(),
            info()
        ));
        assert_ok!(Subtensor::<Runtime>::do_set_null_consensus(net(), false));
        assert_ok!(Commitments::set_commitment(
            RuntimeOrigin::signed(hot()),
            net(),
            info()
        ));
    });
}

#[test]
fn null_metadata_does_not_reset_paused_yuma_bonds() {
    new_test_ext().execute_with(|| {
        Subtensor::<Runtime>::append_neuron(net(), &hot(), 1);
        st::BondsResetOn::<Runtime>::insert(net(), true);
        let index = Subtensor::<Runtime>::get_mechanism_storage_index(net(), 0.into());
        let bonds = vec![(0u16, u16::MAX)];
        st::Bonds::<Runtime>::insert(index, 0, &bonds);
        let mut commitment = info();
        commitment
            .fields
            .try_push(Data::ResetBondsFlag)
            .expect("two fields");
        assert_ok!(Commitments::set_commitment(
            RuntimeOrigin::signed(hot()),
            net(),
            commitment
        ));
        assert_eq!(st::Bonds::<Runtime>::get(index, 0), bonds);
    });
}
