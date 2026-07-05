use super::{CallOf, DispatchableOriginOf};
use crate::{Call, Config, Error, SubnetSaleFrozenColdkeys, SubnetSaleFrozenHotkeys};
use frame_support::{
    dispatch::{DispatchErrorWithPostInfo, DispatchExtension, DispatchInfo, PostDispatchInfo},
    pallet_prelude::*,
    traits::{IsSubType, OriginTrait},
};
use sp_runtime::traits::Dispatchable;
use sp_std::marker::PhantomData;

/// Dispatch extension that blocks seller coldkey and owner hotkey calls during a subnet sale.
///
/// When a subnet sale offer is active:
/// - The frozen seller coldkey can only cancel the sale offer or submit MEV-protected calls.
/// - The frozen owner hotkey can only submit MEV-protected calls.
///
/// Root origin bypasses this extension entirely.
/// Non-signed origins pass through.
///
/// Because this is a `DispatchExtension` (not a `TransactionExtension`), it fires at every
/// `call.dispatch(origin)` site, including inside proxy dispatch with the resolved origin.
pub struct CheckSubnetSale<T: Config>(PhantomData<T>);

impl<T> CheckSubnetSale<T>
where
    T: Config + pallet_shield::Config,
    CallOf<T>: IsSubType<Call<T>> + IsSubType<pallet_shield::Call<T>>,
{
    pub fn check(who: &T::AccountId, call: &CallOf<T>) -> Result<(), Error<T>> {
        let is_sale_frozen_coldkey = SubnetSaleFrozenColdkeys::<T>::contains_key(who);
        let is_sale_frozen_owner_hotkey = SubnetSaleFrozenHotkeys::<T>::contains_key(who);

        if is_sale_frozen_coldkey && !Self::is_allowed_for_frozen_coldkey(call) {
            return Err(Error::<T>::ColdkeyLockedDuringSale);
        }

        if is_sale_frozen_owner_hotkey && !Self::is_allowed_for_frozen_owner_hotkey(call) {
            return Err(Error::<T>::HotkeyLockedDuringSale);
        }

        Ok(())
    }

    fn is_allowed_for_frozen_coldkey(call: &CallOf<T>) -> bool {
        matches!(call.is_sub_type(), Some(Call::cancel_sale_offer { .. }))
            || Self::is_mev_protected(call)
    }

    fn is_allowed_for_frozen_owner_hotkey(call: &CallOf<T>) -> bool {
        Self::is_mev_protected(call)
    }

    fn is_mev_protected(call: &CallOf<T>) -> bool {
        matches!(
            IsSubType::<pallet_shield::Call<T>>::is_sub_type(call),
            Some(pallet_shield::Call::submit_encrypted { .. })
        )
    }
}

impl<T> DispatchExtension<<T as frame_system::Config>::RuntimeCall> for CheckSubnetSale<T>
where
    T: Config + pallet_shield::Config,
    <T as frame_system::Config>::RuntimeCall: Dispatchable<Info = DispatchInfo, PostInfo = PostDispatchInfo>
        + IsSubType<Call<T>>
        + IsSubType<pallet_shield::Call<T>>,
    DispatchableOriginOf<T>: OriginTrait<AccountId = T::AccountId>,
{
    type Pre = ();

    fn weight(_call: &CallOf<T>) -> Weight {
        T::DbWeight::get().reads(2)
    }

    fn pre_dispatch(
        origin: &DispatchableOriginOf<T>,
        call: &CallOf<T>,
    ) -> Result<Self::Pre, DispatchErrorWithPostInfo> {
        let Some(who) = origin.as_signer() else {
            return Ok(());
        };

        Self::check(who, call).map_err(Into::into)
    }
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used)]
mod tests {
    use crate::{Error, SubnetSaleFrozenColdkeys, SubnetSaleFrozenHotkeys, tests::mock::*};
    use frame_support::{
        BoundedVec, assert_noop, assert_ok,
        dispatch::{DispatchErrorWithPostInfo, DispatchExtension},
    };
    use frame_system::Call as SystemCall;
    use pallet_subtensor_proxy::Call as ProxyCall;
    use sp_core::U256;
    use sp_runtime::traits::Dispatchable;
    use subtensor_runtime_common::{NetUid, ProxyType, TaoBalance};

    type SaleGuard = super::CheckSubnetSale<Test>;

    fn pre_dispatch(
        origin: RuntimeOrigin,
        call: &RuntimeCall,
    ) -> Result<(), DispatchErrorWithPostInfo> {
        <SaleGuard as DispatchExtension<RuntimeCall>>::pre_dispatch(&origin, call)
    }

    fn sale_netuid() -> NetUid {
        NetUid::from(1)
    }

    fn freeze_coldkey(who: U256) {
        SubnetSaleFrozenColdkeys::<Test>::insert(who, ());
    }

    fn freeze_owner_hotkey(who: U256) {
        SubnetSaleFrozenHotkeys::<Test>::insert(who, ());
    }

    fn remark_call() -> RuntimeCall {
        RuntimeCall::System(SystemCall::remark { remark: vec![] })
    }

    fn cancel_call() -> RuntimeCall {
        RuntimeCall::SubtensorModule(crate::Call::cancel_sale_offer {
            netuid: sale_netuid(),
        })
    }

    fn shielded_call() -> RuntimeCall {
        RuntimeCall::Shield(pallet_shield::Call::submit_encrypted {
            ciphertext: BoundedVec::truncate_from(vec![1, 2, 3, 4]),
        })
    }

    fn add_balance_to_coldkey_account(coldkey: &U256, tao: TaoBalance) {
        let credit = SubtensorModule::mint_tao(tao);
        let _ = SubtensorModule::spend_tao(coldkey, credit, tao).unwrap();
    }

    #[test]
    fn no_sale_freeze_allows_signed_calls() {
        new_test_ext(1).execute_with(|| {
            let who = U256::from(1);

            assert_ok!(pre_dispatch(RuntimeOrigin::signed(who), &remark_call()));
        });
    }

    #[test]
    fn none_and_root_bypass_sale_freezes() {
        new_test_ext(1).execute_with(|| {
            let who = U256::from(1);
            freeze_coldkey(who);
            freeze_owner_hotkey(who);

            assert_ok!(pre_dispatch(RuntimeOrigin::none(), &remark_call()));
            assert_ok!(pre_dispatch(RuntimeOrigin::root(), &remark_call()));
        });
    }

    #[test]
    fn freeze_coldkey_blocks_regular_signed_calls() {
        new_test_ext(1).execute_with(|| {
            let seller = U256::from(1);
            freeze_coldkey(seller);

            assert_noop!(
                pre_dispatch(RuntimeOrigin::signed(seller), &remark_call()),
                Error::<Test>::ColdkeyLockedDuringSale
            );
        });
    }

    #[test]
    fn freeze_owner_hotkey_blocks_regular_signed_calls() {
        new_test_ext(1).execute_with(|| {
            let owner_hotkey = U256::from(2);
            freeze_owner_hotkey(owner_hotkey);

            assert_noop!(
                pre_dispatch(RuntimeOrigin::signed(owner_hotkey), &remark_call()),
                Error::<Test>::HotkeyLockedDuringSale
            );
        });
    }

    #[test]
    fn freeze_coldkey_allows_sale_cancellation() {
        new_test_ext(1).execute_with(|| {
            let seller = U256::from(1);
            freeze_coldkey(seller);

            assert_ok!(pre_dispatch(RuntimeOrigin::signed(seller), &cancel_call()));
        });
    }

    #[test]
    fn freeze_owner_hotkey_does_not_allow_sale_cancellation() {
        new_test_ext(1).execute_with(|| {
            let owner_hotkey = U256::from(2);
            freeze_owner_hotkey(owner_hotkey);

            assert_noop!(
                pre_dispatch(RuntimeOrigin::signed(owner_hotkey), &cancel_call()),
                Error::<Test>::HotkeyLockedDuringSale
            );
        });
    }

    #[test]
    fn frozen_owner_hotkey_rejects_sale_cancellation_even_if_coldkey() {
        new_test_ext(1).execute_with(|| {
            let seller_and_owner_hotkey = U256::from(1);
            freeze_coldkey(seller_and_owner_hotkey);
            freeze_owner_hotkey(seller_and_owner_hotkey);

            assert_noop!(
                pre_dispatch(
                    RuntimeOrigin::signed(seller_and_owner_hotkey),
                    &cancel_call()
                ),
                Error::<Test>::HotkeyLockedDuringSale
            );
        });
    }

    #[test]
    fn mev_protected_calls_are_allowed_for_sale_frozen_accounts() {
        new_test_ext(1).execute_with(|| {
            let seller = U256::from(1);
            let owner_hotkey = U256::from(2);
            freeze_coldkey(seller);
            freeze_owner_hotkey(owner_hotkey);

            assert_ok!(pre_dispatch(
                RuntimeOrigin::signed(seller),
                &shielded_call()
            ));
            assert_ok!(pre_dispatch(
                RuntimeOrigin::signed(owner_hotkey),
                &shielded_call()
            ));
        });
    }

    #[test]
    fn proxied_call_from_sale_frozen_coldkey_is_blocked() {
        new_test_ext(1).execute_with(|| {
            let real = U256::from(1);
            let delegate = U256::from(2);
            freeze_coldkey(real);

            add_balance_to_coldkey_account(&real, 1_000_000_000.into());
            add_balance_to_coldkey_account(&delegate, 1_000_000_000.into());

            assert_ok!(Proxy::add_proxy(
                RuntimeOrigin::signed(real),
                delegate,
                ProxyType::Any,
                0
            ));

            let proxy_call = RuntimeCall::Proxy(ProxyCall::proxy {
                real,
                force_proxy_type: None,
                call: Box::new(remark_call()),
            });

            assert_ok!(proxy_call.dispatch(RuntimeOrigin::signed(delegate)));
            assert_eq!(
                pallet_subtensor_proxy::LastCallResult::<Test>::get(real),
                Some(Err(Error::<Test>::ColdkeyLockedDuringSale.into()))
            );
        });
    }

    #[test]
    fn nested_proxied_call_from_sale_frozen_owner_hotkey_is_blocked() {
        new_test_ext(1).execute_with(|| {
            let real = U256::from(1);
            let delegate1 = U256::from(2);
            let delegate2 = U256::from(3);
            freeze_owner_hotkey(real);

            add_balance_to_coldkey_account(&real, 1_000_000_000.into());
            add_balance_to_coldkey_account(&delegate1, 1_000_000_000.into());
            add_balance_to_coldkey_account(&delegate2, 1_000_000_000.into());

            assert_ok!(Proxy::add_proxy(
                RuntimeOrigin::signed(real),
                delegate1,
                ProxyType::Any,
                0
            ));
            assert_ok!(Proxy::add_proxy(
                RuntimeOrigin::signed(delegate1),
                delegate2,
                ProxyType::Any,
                0
            ));

            let inner_proxy = RuntimeCall::Proxy(ProxyCall::proxy {
                real,
                force_proxy_type: None,
                call: Box::new(remark_call()),
            });
            let outer_proxy = RuntimeCall::Proxy(ProxyCall::proxy {
                real: delegate1,
                force_proxy_type: None,
                call: Box::new(inner_proxy),
            });

            assert_ok!(outer_proxy.dispatch(RuntimeOrigin::signed(delegate2)));
            assert_eq!(
                pallet_subtensor_proxy::LastCallResult::<Test>::get(real),
                Some(Err(Error::<Test>::HotkeyLockedDuringSale.into()))
            );
        });
    }
}
