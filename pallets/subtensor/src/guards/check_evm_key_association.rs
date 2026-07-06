use super::{CallOf, DispatchableOriginOf, applicable_call};
use crate::weights::WeightInfo;
use crate::{Call, Config, Error, Pallet};
use frame_support::{
    dispatch::{DispatchErrorWithPostInfo, DispatchExtension, DispatchInfo, PostDispatchInfo},
    pallet_prelude::*,
    traits::{IsSubType, OriginTrait},
};
use sp_runtime::traits::Dispatchable;
use sp_std::marker::PhantomData;

/// Dispatch extension for EVM-key association preconditions.
///
/// Signed EVM-key association calls are checked for subnet registration and
/// cooldown before dispatch; unrelated calls and non-signed origins pass through.
pub struct CheckEvmKeyAssociation<T: Config>(PhantomData<T>);

impl<T: Config> CheckEvmKeyAssociation<T> {
    pub(crate) fn applies_to(call: &Call<T>) -> bool {
        matches!(call, Call::associate_evm_key { .. })
    }

    pub fn check(who: &T::AccountId, call: &Call<T>) -> Result<(), Error<T>> {
        match call {
            Call::associate_evm_key { netuid, .. } => {
                let uid = Pallet::<T>::get_uid_for_net_and_hotkey(*netuid, who)
                    .map_err(|_| Error::<T>::HotKeyNotRegisteredInSubNet)?;
                Pallet::<T>::ensure_evm_key_associate_rate_limit(*netuid, uid)
                    .map_err(|_| Error::<T>::EvmKeyAssociateRateLimitExceeded)?;
                Ok(())
            }
            _ => Ok(()),
        }
    }
}

impl<T> DispatchExtension<CallOf<T>> for CheckEvmKeyAssociation<T>
where
    T: Config,
    CallOf<T>: Dispatchable<Info = DispatchInfo, PostInfo = PostDispatchInfo> + IsSubType<Call<T>>,
    DispatchableOriginOf<T>: OriginTrait<AccountId = T::AccountId>,
{
    type Pre = ();

    fn weight(call: &CallOf<T>) -> Weight {
        applicable_call(call, Self::applies_to)
            .map(|_| <T as Config>::WeightInfo::check_evm_key_association_extension())
            .unwrap_or(Weight::zero())
    }

    fn pre_dispatch(
        origin: &DispatchableOriginOf<T>,
        call: &CallOf<T>,
    ) -> Result<Self::Pre, DispatchErrorWithPostInfo> {
        let Some(who) = origin.as_signer() else {
            return Ok(());
        };

        let Some(call) = applicable_call(call, Self::applies_to) else {
            return Ok(());
        };

        Self::check(who, call).map_err(Into::into)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::arithmetic_side_effects)]
mod tests {
    use super::CheckEvmKeyAssociation;
    use crate::{AssociatedEvmAddress, Error, tests::mock::*, weights::WeightInfo as _};
    use codec::Encode;
    use frame_support::{
        assert_ok,
        dispatch::{DispatchExtension, DispatchResultWithPostInfo},
        traits::ExtendedDispatchable,
        weights::Weight,
    };
    use frame_system::Call as SystemCall;
    use sp_core::{H160, Pair, U256, ecdsa, keccak_256};
    use sp_runtime::DispatchError;
    use subtensor_runtime_common::NetUid;

    fn dispatch_with_ext(call: RuntimeCall, origin: RuntimeOrigin) -> DispatchResultWithPostInfo {
        <CheckEvmKeyAssociation<Test> as ExtendedDispatchable<RuntimeCall>>::dispatch_with_extension(
            origin, call,
        )
    }

    fn err(result: DispatchResultWithPostInfo) -> DispatchError {
        result.err().unwrap().error
    }

    fn public_to_evm_key(pubkey: &ecdsa::Public) -> H160 {
        let secp_pub = libsecp256k1::PublicKey::parse_compressed(&pubkey.0).unwrap();
        let uncompressed = secp_pub.serialize();
        let hash = keccak_256(&uncompressed[1..]);
        H160::from_slice(&hash[12..])
    }

    fn sign_evm_message<M: AsRef<[u8]>>(pair: &ecdsa::Pair, message: M) -> ecdsa::Signature {
        let hash = SubtensorModule::hash_message_eip191(message);
        let mut signature = pair.sign_prehashed(&hash);
        signature.0[64] += 27;
        signature
    }

    fn associate_call(
        netuid: NetUid,
        evm_key: H160,
        block_number: u64,
        signature: ecdsa::Signature,
    ) -> RuntimeCall {
        RuntimeCall::SubtensorModule(SubtensorCall::associate_evm_key {
            netuid,
            evm_key,
            block_number,
            signature,
        })
    }

    fn dummy_associate_call(netuid: NetUid) -> RuntimeCall {
        associate_call(
            netuid,
            H160::zero(),
            0,
            ecdsa::Signature::from_raw([0_u8; 65]),
        )
    }

    fn valid_associate_call(netuid: NetUid, hotkey: U256) -> (RuntimeCall, H160) {
        let pair = ecdsa::Pair::generate().0;
        let evm_key = public_to_evm_key(&pair.public());
        let block_number = System::block_number();
        let block_hash = keccak_256(block_number.encode().as_ref());
        let message = [
            hotkey.encode().as_ref(),
            <[u8; 32] as AsRef<[u8]>>::as_ref(&block_hash),
        ]
        .concat();
        let signature = sign_evm_message(&pair, message);

        (
            associate_call(netuid, evm_key, block_number, signature),
            evm_key,
        )
    }

    fn add_stake_call() -> RuntimeCall {
        RuntimeCall::SubtensorModule(SubtensorCall::add_stake {
            hotkey: U256::from(1),
            netuid: 1u16.into(),
            amount_staked: 1_000u64.into(),
        })
    }

    #[test]
    fn weight_only_charges_evm_key_association_calls() {
        let netuid = NetUid::from(1);
        let expected = <Test as crate::Config>::WeightInfo::check_evm_key_association_extension();

        for call in [
            RuntimeCall::System(SystemCall::remark { remark: vec![] }),
            add_stake_call(),
        ] {
            assert_eq!(
                <CheckEvmKeyAssociation<Test> as DispatchExtension<RuntimeCall>>::weight(&call),
                Weight::zero()
            );
        }

        assert_eq!(
            <CheckEvmKeyAssociation<Test> as DispatchExtension<RuntimeCall>>::weight(
                &dummy_associate_call(netuid)
            ),
            expected
        );
    }

    #[test]
    fn unrelated_calls_pass_through() {
        new_test_ext(0).execute_with(|| {
            let hotkey = U256::from(1);
            let call = RuntimeCall::System(SystemCall::remark { remark: vec![] });

            assert_ok!(dispatch_with_ext(call, RuntimeOrigin::signed(hotkey)));
        });
    }

    #[test]
    fn registered_hotkey_allows_call() {
        new_test_ext(0).execute_with(|| {
            let netuid = NetUid::from(1);
            let hotkey = U256::from(1);
            let coldkey = U256::from(2);
            add_network(netuid, 1, 0);
            register_ok_neuron(netuid, hotkey, coldkey, 0);
            System::set_block_number(EvmKeyAssociateRateLimit::get());

            let (call, evm_key) = valid_associate_call(netuid, hotkey);
            assert_ok!(dispatch_with_ext(call, RuntimeOrigin::signed(hotkey)));

            let uid = SubtensorModule::get_uid_for_net_and_hotkey(netuid, &hotkey).unwrap();
            assert_eq!(
                AssociatedEvmAddress::<Test>::get(netuid, uid),
                Some((evm_key, SubtensorModule::get_current_block_as_u64()))
            );
        });
    }

    #[test]
    fn unregistered_hotkey_blocks_call() {
        new_test_ext(0).execute_with(|| {
            let netuid = NetUid::from(1);
            let hotkey = U256::from(1);
            add_network(netuid, 1, 0);

            assert_eq!(
                err(dispatch_with_ext(
                    dummy_associate_call(netuid),
                    RuntimeOrigin::signed(hotkey)
                )),
                Error::<Test>::HotKeyNotRegisteredInSubNet.into()
            );
        });
    }

    #[test]
    fn recent_association_blocks_call() {
        new_test_ext(0).execute_with(|| {
            let netuid = NetUid::from(1);
            let hotkey = U256::from(1);
            let coldkey = U256::from(2);
            add_network(netuid, 1, 0);
            register_ok_neuron(netuid, hotkey, coldkey, 0);
            let uid = SubtensorModule::get_uid_for_net_and_hotkey(netuid, &hotkey).unwrap();
            System::set_block_number(300_u64);
            AssociatedEvmAddress::<Test>::insert(
                netuid,
                uid,
                (H160::zero(), SubtensorModule::get_current_block_as_u64()),
            );

            assert_eq!(
                err(dispatch_with_ext(
                    dummy_associate_call(netuid),
                    RuntimeOrigin::signed(hotkey)
                )),
                Error::<Test>::EvmKeyAssociateRateLimitExceeded.into()
            );
        });
    }
}
