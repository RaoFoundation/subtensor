use super::{CallOf, DispatchableOriginOf};
use crate::weights::WeightInfo;
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
/// - The frozen seller coldkey can only cancel the sale offer.
/// - The frozen owner hotkey is fully locked and can submit no calls.
///
/// Root origin bypasses this extension entirely.
/// Non-signed origins pass through.
///
/// Because this is a `DispatchExtension` (not a `TransactionExtension`), it fires at every
/// `call.dispatch(origin)` site, including inside proxy dispatch with the resolved origin.
/// Any indirectly dispatched call that resolves to a frozen signer is therefore re-checked
/// here, so the freeze cannot be bypassed by wrapping a call in another dispatch layer.
pub struct CheckSubnetSale<T: Config>(PhantomData<T>);

impl<T> CheckSubnetSale<T>
where
    T: Config,
    CallOf<T>: IsSubType<Call<T>>,
{
    pub fn check(who: &T::AccountId, call: &CallOf<T>) -> Result<(), Error<T>> {
        // A frozen seller coldkey may only cancel the sale offer.
        if SubnetSaleFrozenColdkeys::<T>::contains_key(who)
            && !matches!(call.is_sub_type(), Some(Call::cancel_sale_offer { .. }))
        {
            return Err(Error::<T>::ColdkeyLockedDuringSale);
        }

        // A frozen seller hotkey is fully locked while the sale is active. Cancellation is a
        // coldkey action, so the seller cancels through the (also frozen) seller coldkey,
        // which is why a same-account seller hotkey is never frozen here (see do_create_sale_offer).
        if SubnetSaleFrozenHotkeys::<T>::contains_key(who) {
            return Err(Error::<T>::HotkeyLockedDuringSale);
        }

        Ok(())
    }
}

impl<T> DispatchExtension<<T as frame_system::Config>::RuntimeCall> for CheckSubnetSale<T>
where
    T: Config,
    <T as frame_system::Config>::RuntimeCall:
        Dispatchable<Info = DispatchInfo, PostInfo = PostDispatchInfo> + IsSubType<Call<T>>,
    DispatchableOriginOf<T>: OriginTrait<AccountId = T::AccountId>,
{
    type Pre = ();

    fn weight(_call: &CallOf<T>) -> Weight {
        <T as Config>::WeightInfo::check_subnet_sale_extension()
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
    use crate::{
        Error, SubnetSaleFrozenColdkeys, SubnetSaleFrozenHotkeys, tests::mock::*,
        weights::WeightInfo as _,
    };
    use frame_support::{
        assert_noop, assert_ok,
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
    fn weight_is_constant_across_calls_because_freeze_can_block_any_signed_call() {
        let expected = <Test as crate::Config>::WeightInfo::check_subnet_sale_extension();

        for call in [remark_call(), cancel_call()] {
            assert_eq!(
                <SaleGuard as DispatchExtension<RuntimeCall>>::weight(&call),
                expected
            );
        }
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
