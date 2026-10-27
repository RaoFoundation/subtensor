//! Exercise pre-read charging through the actual pinned EVM executor.
#![allow(clippy::unwrap_used, clippy::arithmetic_side_effects)]
use evm::{
    ExitError, Opcode,
    executor::stack::{StackExecutor, StackSubstateMetadata},
};
use fp_evm::{Vicinity, WeightInfo};
use frame_support::{derive_impl, parameter_types, weights::Weight};
use pallet_evm::{AddressMapping, runner::stack::SubstrateStackState};
use sp_core::{H160, H256, U256};

frame_support::construct_runtime! {
    pub enum Test {
        System: frame_system,
        Balances: pallet_balances,
        EVM: pallet_evm,
    }
}
#[derive_impl(frame_system::config_preludes::SolochainDefaultConfig)]
impl frame_system::Config for Test {
    type Block = frame_system::mocking::MockBlock<Self>;
    type AccountData = pallet_balances::AccountData<u64>;
}
#[derive_impl(pallet_balances::config_preludes::TestDefaultConfig)]
impl pallet_balances::Config for Test {
    type AccountStore = System;
}
parameter_types! {
    pub static MappingReadWeight: Weight = Weight::zero();
    pub static MappingReadCount: u64 = 0;
}
pub struct CountingAddressMapping;
pub struct TestTime;
impl frame_support::traits::Time for TestTime {
    type Moment = u64;
    fn now() -> Self::Moment {
        0
    }
}
impl AddressMapping<sp_runtime::AccountId32> for CountingAddressMapping {
    fn into_account_id(address: H160) -> sp_runtime::AccountId32 {
        MappingReadCount::set(MappingReadCount::get().saturating_add(1));
        pallet_evm::HashedAddressMapping::<sp_runtime::traits::BlakeTwo256>::into_account_id(
            address,
        )
    }
    fn extra_read_weight() -> Weight {
        MappingReadWeight::get()
    }
}
#[derive_impl(pallet_evm::config_preludes::TestDefaultConfig)]
impl pallet_evm::Config for Test {
    type AccountProvider = pallet_evm::FrameSystemAccountProvider<Self>;
    type AddressMapping = CountingAddressMapping;
    type BlockHashMapping = pallet_evm::SubstrateBlockHashMapping<Self>;
    type Currency = Balances;
    type Runner = pallet_evm::runner::stack::Runner<Self>;
    type Timestamp = TestTime;
    type BalanceConverter = crate::SubtensorEvmBalanceConverter;
}

#[cfg(test)]
mod mapping_precharge_tests {

    use super::*;
    use evm::{Context, Handler, Stack};

    fn check(
        opcode: Opcode,
        gas: u64,
        limit: Weight,
    ) -> (Result<(), ExitError>, u64, Option<WeightInfo>) {
        let vicinity = Vicinity::default();
        let config = <Test as pallet_evm::Config>::config();
        let metadata = StackSubstateMetadata::new(gas, config);
        let weight_info = WeightInfo::new_from_weight_limit(Some(limit), Some(0)).unwrap();
        let state = SubstrateStackState::<Test>::new(&vicinity, metadata, weight_info, None);
        let precompiles = ();
        let mut executor = StackExecutor::new_with_precompiles(state, config, &precompiles);
        let mut stack = Stack::new(1024);
        // Seven zero operands suffice for every CALL variant; SELFDESTRUCT uses the top.
        for _ in 0..7 {
            stack.push(H256::zero()).unwrap();
        }
        MappingReadCount::set(0);
        let result = executor.pre_validate(
            &Context {
                address: H160::repeat_byte(9),
                caller: H160::repeat_byte(8),
                apparent_value: U256::zero(),
            },
            opcode,
            &stack,
        );
        let info = executor.state().weight_info();
        (result, MappingReadCount::get(), info)
    }

    #[test]
    fn failing_dynamic_opcodes_charge_mapping_before_reading_storage() {
        sp_io::TestExternalities::default().execute_with(|| {
            let read = Weight::from_parts(1_000_000, 32);
            MappingReadWeight::set(read);
            let cost = pallet_evm::account_cost::gas::<Test>(read);
            assert!(cost > 0);
            for opcode in [
                Opcode::CALL,
                Opcode::CALLCODE,
                Opcode::DELEGATECALL,
                Opcode::STATICCALL,
                Opcode::SUICIDE,
            ] {
                // Neither gas, ref-time nor proof-size exhaustion may expose the read.
                for (gas, limit) in [
                    (cost - 1, Weight::MAX),
                    (u64::MAX, Weight::from_parts(read.ref_time() - 1, u64::MAX)),
                    (
                        u64::MAX,
                        Weight::from_parts(u64::MAX, read.proof_size() - 1),
                    ),
                    (
                        u64::MAX,
                        Weight::from_parts(
                            u64::MAX,
                            read.proof_size() + fp_evm::ACCOUNT_BASIC_PROOF_SIZE - 1,
                        ),
                    ),
                ] {
                    let (result, reads, _) = check(opcode, gas, limit);
                    assert_eq!(result, Err(ExitError::OutOfGas), "{opcode:?}");
                    assert_eq!(reads, 0, "{opcode:?}");
                }
            }
            // Source balance may be read only after its own charge, even if the
            // second (target existence) lookup cannot be afforded.
            let (result, reads, info) = check(Opcode::SUICIDE, cost, Weight::MAX);
            assert_eq!(result, Err(ExitError::OutOfGas));
            assert_eq!(reads, 1);
            assert_eq!(info.unwrap().ref_time_usage, Some(read.ref_time()));
            // Both early reads remain paid when ordinary opcode gas then fails.
            let (result, reads, info) = check(Opcode::SUICIDE, cost * 2, Weight::MAX);
            assert_eq!(result, Err(ExitError::OutOfGas));
            assert_eq!(reads, 2);
            assert_eq!(info.unwrap().ref_time_usage, Some(read.ref_time() * 2));
            MappingReadWeight::set(Weight::zero());
        });
    }
    fn execute_parent(
        code: Vec<u8>,
        child_code: Vec<u8>,
        funded: bool,
    ) -> (evm::ExitReason, Vec<u8>, u64) {
        use frame_support::traits::Currency;
        use std::rc::Rc;
        let parent = H160::repeat_byte(9);
        let child = H160::repeat_byte(7);
        pallet_evm::AccountCodes::<Test>::insert(child, child_code);
        if funded {
            let who = CountingAddressMapping::into_account_id(parent);
            let _ = Balances::make_free_balance_be(&who, 1_000_000);
        }
        let vicinity = Vicinity::default();
        let config = <Test as pallet_evm::Config>::config();
        let metadata = StackSubstateMetadata::new(1_000_000, config);
        let state = SubstrateStackState::<Test>::new(&vicinity, metadata, None, None);
        let precompiles = ();
        let mut executor = StackExecutor::new_with_precompiles(state, config, &precompiles);
        let mut runtime = evm::Runtime::new(
            Rc::new(code),
            Rc::new(Vec::new()),
            Context {
                address: parent,
                caller: H160::repeat_byte(8),
                apparent_value: U256::zero(),
            },
            config.stack_limit,
            config.memory_limit,
        );
        MappingReadCount::set(0);
        let result = executor.execute(&mut runtime);
        (
            result,
            runtime.machine().return_value(),
            MappingReadCount::get(),
        )
    }

    fn push_call(code: &mut Vec<u8>, gas: u16, value: u32) {
        // out size/offset, input size/offset, value, target, gas.
        code.extend_from_slice(&[0x60, 0, 0x60, 0, 0x60, 0, 0x60, 0, 0x63]);
        code.extend_from_slice(&value.to_be_bytes());
        code.push(0x73);
        code.extend_from_slice(H160::repeat_byte(7).as_bytes());
        code.push(0x61);
        code.extend_from_slice(&gas.to_be_bytes());
        code.push(0xf1);
    }

    #[test]
    fn caught_low_gas_children_cannot_repeat_unpaid_selfdestruct_reads() {
        for attempts in [1, 8, 64] {
            sp_io::TestExternalities::default().execute_with(|| {
                let read = Weight::from_parts(1_000_000, 32);
                MappingReadWeight::set(read);
                let cost = u16::try_from(pallet_evm::account_cost::gas::<Test>(read)).unwrap();
                let mut code = Vec::new();
                for _ in 0..attempts {
                    push_call(&mut code, cost - 1, 0);
                    code.push(0x50); // caller catches child failure
                }
                code.push(0);
                let (result, _, reads) = execute_parent(code, vec![0x60, 0, 0xff], false);
                assert!(result.is_succeed(), "{result:?}");
                // Each caller pays/reads target existence and the two transfer
                // mappings. Exhausted children must perform zero alias reads.
                assert_eq!(reads, attempts * 3);
                MappingReadWeight::set(Weight::zero());
            });
        }
    }

    #[test]
    fn value_transfer_with_only_the_2300_gas_stipend_still_succeeds() {
        sp_io::TestExternalities::default().execute_with(|| {
            MappingReadWeight::set(Weight::from_parts(1_000_000, 32));
            let mut code = Vec::new();
            push_call(&mut code, 0, 1_000_000_000); // one rao; child gets only stipend
            code.extend_from_slice(&[0x60, 0, 0x52, 0x60, 32, 0x60, 0, 0xf3]);
            let (result, output, _) = execute_parent(code, vec![0], true);
            assert!(result.is_succeed(), "{result:?}");
            assert_eq!(U256::from_big_endian(&output), U256::one());
            MappingReadWeight::set(Weight::zero());
        });
    }
}

#[cfg(test)]
mod external_oog_atomicity_tests {
    use crate::{Balances, Runtime, RuntimeEvent, System};
    use evm::ExitError;
    use frame_support::{traits::Currency, weights::Weight};
    use pallet_evm::{AddressMapping, FeeCalculator, GasWeightMapping, runner::Runner};
    use sp_core::{H160, H256, U256};

    type Mapping = <Runtime as pallet_evm::Config>::AddressMapping;
    type EvmRunner = pallet_evm::runner::stack::Runner<Runtime>;

    const GAS_LIMIT: u64 = 1_000_000;
    const INITIAL_BALANCE: u64 = 1_000_000_000_000;
    const VALUE: u64 = 1_000;

    #[derive(Clone, Copy, Debug)]
    enum Operation {
        Call,
        Create,
        Create2,
    }

    fn source() -> H160 {
        H160::repeat_byte(8)
    }

    fn child() -> H160 {
        H160::repeat_byte(7)
    }

    fn target(operation: Operation, code: &[u8]) -> H160 {
        use evm::{
            CreateScheme,
            executor::stack::{StackExecutor, StackSubstateMetadata},
        };
        use pallet_evm::runner::stack::SubstrateStackState;
        let vicinity = fp_evm::Vicinity::default();
        let config = <Runtime as pallet_evm::Config>::config();
        let state = SubstrateStackState::<Runtime>::new(
            &vicinity,
            StackSubstateMetadata::new(GAS_LIMIT, config),
            None,
            None,
        );
        let executor = StackExecutor::new_with_precompiles(state, config, &());
        match operation {
            Operation::Call => H160::repeat_byte(9),
            Operation::Create => executor.create_address(CreateScheme::Legacy { caller: source() }),
            Operation::Create2 => executor.create_address(CreateScheme::Create2 {
                caller: source(),
                code_hash: sp_io::hashing::keccak_256(code).into(),
                salt: H256::zero(),
            }),
        }
    }

    fn fund(address: H160, amount: u64) {
        let _ = Balances::make_free_balance_be(&Mapping::into_account_id(address), amount.into());
    }

    fn balance(address: H160) -> u64 {
        Balances::free_balance(Mapping::into_account_id(address)).into()
    }

    fn execute(
        operation: Operation,
        code: Vec<u8>,
        limit: Weight,
    ) -> (evm::ExitReason, fp_evm::UsedGas, Vec<fp_evm::Log>) {
        let (price, _) = <Runtime as pallet_evm::Config>::FeeCalculator::min_gas_price();
        let value = U256::from(VALUE) * U256::from(crate::EVM_TO_SUBSTRATE_DECIMALS);
        let config = <Runtime as pallet_evm::Config>::config();
        match operation {
            Operation::Call => {
                let target = target(operation, &code);
                pallet_evm::AccountCodes::<Runtime>::insert(target, code);
                let info = EvmRunner::call(
                    source(),
                    target,
                    Vec::new(),
                    value,
                    GAS_LIMIT,
                    Some(price),
                    None,
                    None,
                    Vec::new(),
                    Vec::new(),
                    true,
                    false,
                    Some(limit),
                    None,
                    config,
                )
                .unwrap();
                (info.exit_reason, info.used_gas, info.logs)
            }
            Operation::Create | Operation::Create2 => {
                let info = if matches!(operation, Operation::Create) {
                    EvmRunner::create(
                        source(),
                        code,
                        value,
                        GAS_LIMIT,
                        Some(price),
                        None,
                        None,
                        Vec::new(),
                        Vec::new(),
                        true,
                        Vec::new(),
                        true,
                        false,
                        Some(limit),
                        None,
                        config,
                    )
                } else {
                    EvmRunner::create2(
                        source(),
                        code,
                        H256::zero(),
                        value,
                        GAS_LIMIT,
                        Some(price),
                        None,
                        None,
                        Vec::new(),
                        Vec::new(),
                        true,
                        Vec::new(),
                        true,
                        false,
                        Some(limit),
                        None,
                        config,
                    )
                }
                .unwrap();
                (info.exit_reason, info.used_gas, info.logs)
            }
        }
    }

    fn assert_fee_and_nonce(gas: U256, transferred: u64) {
        let (price, _) = <Runtime as pallet_evm::Config>::FeeCalculator::min_gas_price();
        let fee = (gas * price / U256::from(crate::EVM_TO_SUBSTRATE_DECIMALS)).as_u64();
        assert!(fee > 0);
        assert_eq!(balance(source()), INITIAL_BALANCE - fee - transferred);
        assert_eq!(
            pallet_evm::Pallet::<Runtime>::account_basic(&source())
                .0
                .nonce,
            U256::one()
        );
        assert_eq!(
            pallet_evm::AccountNonces::<Runtime>::get(Mapping::into_account_id(source())),
            1
        );
    }

    fn call_empty(code: &mut Vec<u8>, value: u32) {
        code.extend_from_slice(&[0x60, 0, 0x60, 0, 0x60, 0, 0x60, 0, 0x63]);
        code.extend_from_slice(&value.to_be_bytes());
        code.push(0x73);
        code.extend_from_slice(H160::repeat_byte(7).as_bytes());
        code.extend_from_slice(&[0x60, 0, 0xf1, 0x50]);
    }

    #[test]
    fn caught_external_oog_cannot_commit_parent_storage() {
        sp_io::TestExternalities::default().execute_with(|| {
            let source = H160::repeat_byte(8);
            let target = H160::repeat_byte(9);
            let _ = Balances::make_free_balance_be(
                &Mapping::into_account_id(source),
                1_000_000_000_000u64.into(),
            );
            System::set_block_number(1);
            let mut code = vec![0x60, 1, 0x60, 0, 0x55];
            for _ in 0..24 {
                call_empty(&mut code, 0);
            }
            code.push(0);
            pallet_evm::AccountCodes::<Runtime>::insert(target, code);
            let (price, _) = <Runtime as pallet_evm::Config>::FeeCalculator::min_gas_price();
            let result = EvmRunner::call(
                source,
                target,
                Vec::new(),
                U256::zero(),
                100_000,
                Some(price),
                None,
                None,
                Vec::new(),
                Vec::new(),
                true,
                false,
                None,
                None,
                <Runtime as pallet_evm::Config>::config(),
            )
            .unwrap();
            assert_eq!(result.exit_reason, ExitError::OutOfGas.into());
            assert_eq!(
                pallet_evm::AccountStorages::<Runtime>::get(target, H256::zero()),
                H256::zero()
            );
        });
    }

    #[test]
    fn global_exhaustion_reverts_call_create_and_create2_effects() {
        for operation in [Operation::Call, Operation::Create, Operation::Create2] {
            for succeeds in [false, true] {
                let mut ext = sp_io::TestExternalities::default();
                ext.execute_with(|| {
                    System::set_block_number(1);
                    fund(source(), INITIAL_BALANCE);
                    fund(child(), VALUE);
                    let mut code = vec![0x60, 1, 0x60, 0, 0x55]; // SSTORE(0, 1)
                    code.extend_from_slice(&[0x60, 0, 0x60, 0, 0xa0]); // LOG0
                    call_empty(&mut code, 1_000_000_000); // transfer one rao
                    for _ in 0..24 {
                        call_empty(&mut code, 0);
                    }
                    // CREATE must return nonempty code: its cleanup tries to
                    // install this even when the execution-frame commit fails.
                    code.extend_from_slice(&[0x60, 1, 0x60, 0, 0xf3]);
                    let target = target(operation, &code);
                    fund(target, VALUE);
                    System::reset_events();
                    let limit = <Runtime as pallet_evm::Config>::GasWeightMapping::gas_to_weight(
                        if succeeds { GAS_LIMIT } else { 100_000 },
                        false,
                    );
                    let (reason, gas, logs) = execute(operation, code, limit);
                    assert_eq!(reason.is_succeed(), succeeds, "{operation:?}: {reason:?}");
                    assert_eq!(
                        pallet_evm::AccountStorages::<Runtime>::get(target, H256::zero()),
                        if succeeds {
                            H256::from_low_u64_be(1)
                        } else {
                            H256::zero()
                        }
                    );
                    assert_eq!(balance(child()), VALUE + u64::from(succeeds));
                    assert_eq!(
                        balance(target),
                        VALUE + if succeeds { VALUE - 1 } else { 0 }
                    );
                    assert_eq!(logs.len(), usize::from(succeeds));
                    let events = System::events();
                    assert_eq!(
                        events
                            .iter()
                            .filter(|event| matches!(
                                event.event,
                                RuntimeEvent::EVM(pallet_evm::Event::Log { .. })
                            ))
                            .count(),
                        usize::from(succeeds)
                    );
                    assert_eq!(
                        events
                            .iter()
                            .filter(|event| matches!(
                                event.event,
                                RuntimeEvent::Balances(pallet_balances::Event::Transfer { .. })
                            ))
                            .count(),
                        if succeeds { 2 } else { 0 }
                    );
                    assert_fee_and_nonce(gas.effective, if succeeds { VALUE } else { 0 });
                    if !succeeds {
                        assert_eq!(reason, ExitError::OutOfGas.into());
                        assert_eq!(gas.effective, U256::from(GAS_LIMIT));
                    }
                    if !matches!(operation, Operation::Call) {
                        assert_eq!(
                            pallet_evm::AccountCodes::<Runtime>::get(target),
                            if succeeds { vec![0] } else { Vec::new() }
                        );
                        assert_eq!(
                            pallet_evm::AccountCodesMetadata::<Runtime>::contains_key(target),
                            succeeds
                        );
                        assert_eq!(
                            pallet_evm::AccountNonces::<Runtime>::get(Mapping::into_account_id(
                                target
                            )),
                            u32::from(succeeds)
                        );
                    }
                });
                assert_eq!(
                    ext.overlayed_changes().transaction_depth(),
                    0,
                    "{operation:?}"
                );
            }
        }
    }

    #[test]
    fn created_nonce_exhaustion_closes_its_frame_before_fee_settlement() {
        for operation in [Operation::Create, Operation::Create2] {
            let mut ext = sp_io::TestExternalities::default();
            ext.execute_with(|| {
                System::set_block_number(1);
                fund(source(), INITIAL_BALANCE);
                let code = vec![0];
                let target = target(operation, &code);
                fund(target, VALUE);
                // Pay the root nonce and the three retained-nonce reads, but
                // exhaust the budget at the created account's nonce update.
                let limit = pallet_evm::account_cost::transaction_weight::<Runtime>(true, 0)
                    .saturating_add(<Runtime as frame_system::Config>::DbWeight::get().reads(3))
                    .saturating_add(
                        <Runtime as frame_system::Config>::DbWeight::get().reads_writes(1, 1),
                    );
                let (reason, gas, logs) = execute(operation, code, limit);
                assert_eq!(reason, ExitError::OutOfGas.into());
                assert_eq!(gas.effective, U256::from(GAS_LIMIT));
                assert_eq!(balance(target), VALUE);
                assert!(logs.is_empty());
                assert!(!pallet_evm::AccountCodes::<Runtime>::contains_key(target));
                assert_eq!(
                    pallet_evm::AccountNonces::<Runtime>::get(Mapping::into_account_id(target)),
                    0
                );
                assert_fee_and_nonce(gas.effective, 0);
            });
            assert_eq!(
                ext.overlayed_changes().transaction_depth(),
                0,
                "{operation:?}"
            );
        }
    }

    #[test]
    fn create_cleanup_cannot_install_code_after_a_rejected_commit() {
        use evm::executor::stack::{StackExecutor, StackSubstateMetadata};
        use pallet_evm::runner::stack::SubstrateStackState;

        for operation in [Operation::Create, Operation::Create2] {
            let mut ext = sp_io::TestExternalities::default();
            ext.execute_with(|| {
                fund(source(), INITIAL_BALANCE);
                // Store a value, then return one byte of runtime code.
                let init = vec![0x60, 1, 0x60, 0, 0x55, 0x60, 1, 0x60, 0, 0xf3];
                let target = target(operation, &init);
                let vicinity = fp_evm::Vicinity::default();
                let config = <Runtime as pallet_evm::Config>::config();
                let state = SubstrateStackState::<Runtime>::new(
                    &vicinity,
                    StackSubstateMetadata::new(GAS_LIMIT, config),
                    None,
                    None,
                );
                let mut executor = StackExecutor::new_with_precompiles(state, config, &());
                let (reason, _, _, _) = fp_evm::handle_storage_oog(GAS_LIMIT, || {
                    // A nested runner can exhaust the shared budget while this
                    // executor still has local gas/weight. Force that condition
                    // so cleanup reaches set_code after the rejected commit.
                    let mut other_budget =
                        fp_evm::WeightInfo::new_from_weight_limit(Some(Weight::zero()), None)
                            .unwrap()
                            .unwrap();
                    assert_eq!(
                        other_budget.try_record_ref_time_or_fail(1),
                        Err(ExitError::OutOfGas)
                    );
                    let (reason, _) = if matches!(operation, Operation::Create) {
                        executor.transact_create(
                            source(),
                            U256::zero(),
                            init,
                            GAS_LIMIT,
                            Vec::new(),
                            Vec::new(),
                        )
                    } else {
                        executor.transact_create2(
                            source(),
                            U256::zero(),
                            init,
                            H256::zero(),
                            GAS_LIMIT,
                            Vec::new(),
                            Vec::new(),
                        )
                    };
                    (
                        reason,
                        (),
                        executor.used_gas(),
                        U256::from(executor.used_gas()),
                    )
                });
                assert_eq!(reason, ExitError::OutOfGas.into());
                assert!(!pallet_evm::AccountCodes::<Runtime>::contains_key(target));
                assert!(!pallet_evm::AccountCodesMetadata::<Runtime>::contains_key(
                    target
                ));
                assert_eq!(
                    pallet_evm::AccountStorages::<Runtime>::get(target, H256::zero()),
                    H256::zero()
                );
                assert_eq!(
                    pallet_evm::Pallet::<Runtime>::account_basic(&source())
                        .0
                        .nonce,
                    U256::one()
                );
            });
            assert_eq!(ext.overlayed_changes().transaction_depth(), 0);
        }
    }

    #[test]
    fn ordinary_child_gas_failure_remains_catchable() {
        sp_io::TestExternalities::default().execute_with(|| {
            System::set_block_number(1);
            fund(source(), INITIAL_BALANCE);
            // The child cannot SSTORE with zero forwarded gas. Its parent must
            // still be able to complete and commit its own storage and transfer.
            pallet_evm::AccountCodes::<Runtime>::insert(child(), vec![0x60, 1, 0x60, 0, 0x55, 0]);
            let mut code = Vec::new();
            call_empty(&mut code, 0);
            code.extend_from_slice(&[0x60, 1, 0x60, 0, 0x55, 0]);
            let target = target(Operation::Call, &code);
            fund(target, VALUE);
            let (reason, gas, _) = execute(Operation::Call, code, Weight::MAX);
            assert!(reason.is_succeed());
            assert_eq!(
                pallet_evm::AccountStorages::<Runtime>::get(target, H256::zero()),
                H256::from_low_u64_be(1)
            );
            assert_eq!(
                pallet_evm::AccountStorages::<Runtime>::get(child(), H256::zero()),
                H256::zero()
            );
            assert_eq!(balance(target), VALUE * 2);
            assert_fee_and_nonce(gas.effective, VALUE);
        });
    }
}
