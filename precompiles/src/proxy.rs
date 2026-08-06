use core::marker::PhantomData;

use crate::{PrecompileExt, PrecompileHandleExt};

use alloc::format;
use fp_evm::{ExitError, PrecompileFailure};
use frame_support::dispatch::{DispatchInfo, GetDispatchInfo, PostDispatchInfo};
use frame_support::traits::IsSubType;
use frame_system::RawOrigin;
use pallet_evm::{AddressMapping, PrecompileHandle};
use pallet_subtensor_proxy as pallet_proxy;
use precompile_utils::EvmResult;
use sp_core::{H256, U256};
use sp_runtime::{
    DispatchError,
    codec::DecodeLimit,
    traits::{
        AsSystemOriginSigner, Dispatchable, SaturatedConversion, StaticLookup, UniqueSaturatedInto,
    },
};
use sp_std::boxed::Box;
use sp_std::convert::{TryFrom, TryInto};
use sp_std::vec;
use sp_std::vec::Vec;
use subtensor_runtime_common::ProxyType;
pub struct ProxyPrecompile<R>(PhantomData<R>);
const MAX_DECODE_DEPTH: u32 = 8;

impl<R> PrecompileExt<R::AccountId> for ProxyPrecompile<R>
where
    R: frame_system::Config
        + pallet_balances::Config
        + pallet_evm::Config
        + pallet_subtensor::Config
        + pallet_proxy::Config<ProxyType = ProxyType>
        + pallet_shield::Config
        + pallet_subtensor_proxy::Config
        + Send
        + Sync
        + scale_info::TypeInfo,
    R::AccountId: From<[u8; 32]> + Into<[u8; 32]>,
    <R as frame_system::Config>::RuntimeOrigin: AsSystemOriginSigner<R::AccountId> + Clone,
    <R as pallet_evm::Config>::AddressMapping: AddressMapping<R::AccountId>,
    <R as frame_system::Config>::RuntimeCall: From<pallet_subtensor::Call<R>>
        + From<pallet_proxy::Call<R>>
        + GetDispatchInfo
        + Dispatchable<Info = DispatchInfo, PostInfo = PostDispatchInfo>
        + IsSubType<pallet_balances::Call<R>>
        + IsSubType<pallet_subtensor::Call<R>>
        + IsSubType<pallet_shield::Call<R>>
        + IsSubType<pallet_subtensor_proxy::Call<R>>,
    <R as pallet_evm::Config>::AddressMapping: AddressMapping<R::AccountId>,
    <<R as frame_system::Config>::Lookup as StaticLookup>::Source: From<R::AccountId>,
{
    const INDEX: u64 = 2059;
}

#[precompile_utils::precompile]
impl<R> ProxyPrecompile<R>
where
    R: frame_system::Config
        + pallet_balances::Config
        + pallet_evm::Config
        + pallet_subtensor::Config
        + pallet_proxy::Config<ProxyType = ProxyType>
        + pallet_shield::Config
        + pallet_subtensor_proxy::Config
        + Send
        + Sync
        + scale_info::TypeInfo,
    R::AccountId: From<[u8; 32]> + Into<[u8; 32]>,
    <R as frame_system::Config>::RuntimeOrigin: AsSystemOriginSigner<R::AccountId> + Clone,
    <R as pallet_evm::Config>::AddressMapping: AddressMapping<R::AccountId>,
    <R as frame_system::Config>::RuntimeCall: From<pallet_subtensor::Call<R>>
        + From<pallet_proxy::Call<R>>
        + GetDispatchInfo
        + Dispatchable<Info = DispatchInfo, PostInfo = PostDispatchInfo>
        + IsSubType<pallet_balances::Call<R>>
        + IsSubType<pallet_subtensor::Call<R>>
        + IsSubType<pallet_shield::Call<R>>
        + IsSubType<pallet_subtensor_proxy::Call<R>>,
    <<R as frame_system::Config>::Lookup as StaticLookup>::Source: From<R::AccountId>,
{
    #[precompile::public("createPureProxy(uint8,uint32,uint16)")]
    pub fn create_pure_proxy(
        handle: &mut impl PrecompileHandle,
        proxy_type_: u8,
        delay: u32,
        index: u16,
    ) -> EvmResult<H256> {
        let account_id = handle.caller_account_id::<R>();
        let proxy_type =
            ProxyType::try_from(proxy_type_).map_err(|_| PrecompileFailure::Error {
                exit_status: ExitError::Other("Invalid proxy type".into()),
            })?;

        let call = pallet_proxy::Call::<R>::create_pure {
            proxy_type,
            delay: delay.into(),
            index,
        };

        handle.try_dispatch_runtime_call::<R, _>(call, RawOrigin::Signed(account_id.clone()))?;

        // Success!
        // Try to get proxy address
        let proxy_address: [u8; 32] =
            pallet_proxy::pallet::Pallet::<R>::pure_account(&account_id, &proxy_type, index, None)
                .map_err(|_| PrecompileFailure::Error {
                    exit_status: ExitError::Other("Proxy not found".into()),
                })?
                .into();

        // Check if in the proxies map
        let proxy_entry = pallet_proxy::pallet::Pallet::<R>::proxies(proxy_address.into());
        if proxy_entry
            .0
            .iter()
            .any(|p| account_id == p.delegate && proxy_type == p.proxy_type)
        {
            return Ok(proxy_address.into());
        }

        Err(PrecompileFailure::Error {
            exit_status: ExitError::Other("Proxy not found".into()),
        })
    }

    #[precompile::public("killPureProxy(bytes32,uint8,uint16,uint32,uint32)")]
    pub fn kill_pure_proxy(
        handle: &mut impl PrecompileHandle,
        spawner: H256,
        proxy_type: u8,
        index: u16,
        height: u32,
        ext_index: u32,
    ) -> EvmResult<()> {
        let account_id = handle.caller_account_id::<R>();
        let proxy_type = ProxyType::try_from(proxy_type).map_err(|_| PrecompileFailure::Error {
            exit_status: ExitError::Other("Invalid proxy type".into()),
        })?;

        let call = pallet_proxy::Call::<R>::kill_pure {
            spawner: <<R as frame_system::Config>::Lookup as StaticLookup>::Source::from(
                spawner.0.into(),
            ),
            proxy_type,
            index,
            height: height.into(),
            ext_index: ext_index.into(),
        };

        handle.try_dispatch_runtime_call::<R, _>(call, RawOrigin::Signed(account_id))
    }

    #[precompile::public("proxyCall(bytes32,uint8[],uint8[])")]
    pub fn proxy_call(
        handle: &mut impl PrecompileHandle,
        real: H256,
        force_proxy_type: Vec<u8>,
        call: Vec<u8>,
    ) -> EvmResult<()> {
        let account_id = handle.caller_account_id::<R>();

        let call = <R as pallet_proxy::Config>::RuntimeCall::decode_with_depth_limit(
            MAX_DECODE_DEPTH,
            &mut &call[..],
        )
        .map_err(|_| PrecompileFailure::Error {
            exit_status: ExitError::Other("The raw call data not correctly encoded".into()),
        })?;

        let mut proxy_type: Option<ProxyType> = None;
        if let Some(p) = force_proxy_type.first() {
            let proxy_type_ = ProxyType::try_from(*p).map_err(|_| PrecompileFailure::Error {
                exit_status: ExitError::Other("Invalid proxy type".into()),
            })?;
            proxy_type = Some(proxy_type_);
        };

        let call = pallet_proxy::Call::<R>::proxy {
            real: <<R as frame_system::Config>::Lookup as StaticLookup>::Source::from(
                real.0.into(),
            ),
            force_proxy_type: proxy_type,
            call: Box::new(call),
        };

        handle.try_dispatch_runtime_call::<R, _>(call, RawOrigin::Signed(account_id))?;

        let real_account_id = R::AccountId::from(real.0.into());

        let last_call_result = pallet_proxy::LastCallResult::<R>::get(real_account_id);
        match last_call_result {
            Some(last_call_result) => match last_call_result {
                Ok(()) => Ok(()),
                Err(e) => Err(PrecompileFailure::Error {
                    exit_status: ExitError::Other(format!("{e:?}").into()),
                }),
            },
            None => Err(PrecompileFailure::Error {
                exit_status: ExitError::Other("Proxy execution failed".into()),
            }),
        }
    }

    #[precompile::public("addProxy(bytes32,uint8,uint32)")]
    pub fn add_proxy(
        handle: &mut impl PrecompileHandle,
        delegate: H256,
        proxy_type: u8,
        delay: u32,
    ) -> EvmResult<()> {
        let account_id = handle.caller_account_id::<R>();
        let proxy_type = ProxyType::try_from(proxy_type).map_err(|_| PrecompileFailure::Error {
            exit_status: ExitError::Other("Invalid proxy type".into()),
        })?;

        let call = pallet_proxy::Call::<R>::add_proxy {
            delegate: <<R as frame_system::Config>::Lookup as StaticLookup>::Source::from(
                delegate.0.into(),
            ),
            proxy_type,
            delay: delay.into(),
        };

        handle.try_dispatch_runtime_call::<R, _>(call, RawOrigin::Signed(account_id))
    }

    #[precompile::public("removeProxy(bytes32,uint8,uint32)")]
    pub fn remove_proxy(
        handle: &mut impl PrecompileHandle,
        delegate: H256,
        proxy_type: u8,
        delay: u32,
    ) -> EvmResult<()> {
        let account_id = handle.caller_account_id::<R>();
        let proxy_type = ProxyType::try_from(proxy_type).map_err(|_| PrecompileFailure::Error {
            exit_status: ExitError::Other("Invalid proxy type".into()),
        })?;

        let call = pallet_proxy::Call::<R>::remove_proxy {
            delegate: <<R as frame_system::Config>::Lookup as StaticLookup>::Source::from(
                delegate.0.into(),
            ),
            proxy_type,
            delay: delay.into(),
        };

        handle.try_dispatch_runtime_call::<R, _>(call, RawOrigin::Signed(account_id))
    }

    #[precompile::public("removeProxies()")]
    pub fn remove_proxies(handle: &mut impl PrecompileHandle) -> EvmResult<()> {
        let account_id = handle.caller_account_id::<R>();

        let call = pallet_proxy::Call::<R>::remove_proxies {};

        handle.try_dispatch_runtime_call::<R, _>(call, RawOrigin::Signed(account_id))
    }

    #[precompile::public("pokeDeposit()")]
    pub fn poke_deposit(handle: &mut impl PrecompileHandle) -> EvmResult<()> {
        let account_id = handle.caller_account_id::<R>();

        let call = pallet_proxy::Call::<R>::poke_deposit {};

        handle.try_dispatch_runtime_call::<R, _>(call, RawOrigin::Signed(account_id))
    }

    #[precompile::public("getProxies(bytes32)")]
    #[precompile::view]
    pub fn get_proxies(
        handle: &mut impl PrecompileHandle,
        account_id: H256,
    ) -> EvmResult<Vec<(H256, U256, U256)>> {
        handle.record_db_reads::<R>(1)?;
        let account_id = R::AccountId::from(account_id.0.into());

        let proxies = pallet_proxy::pallet::Pallet::<R>::proxies(account_id);
        let mut result: Vec<(H256, U256, U256)> = vec![];
        for proxy in proxies.0 {
            let delegate: [u8; 32] = proxy.delegate.into();
            let proxy_type: u8 = proxy.proxy_type.into();
            let delay: u32 = proxy
                .delay
                .try_into()
                .map_err(|_| PrecompileFailure::Error {
                    exit_status: ExitError::Other("Invalid delay".into()),
                })?;

            result.push((delegate.into(), proxy_type.into(), delay.into()));
        }

        Ok(result)
    }

    #[precompile::public("getProxyDeposit(bytes32)")]
    #[precompile::view]
    pub fn get_proxy_deposit(
        handle: &mut impl PrecompileHandle,
        account_id: H256,
    ) -> EvmResult<U256> {
        handle.record_db_reads::<R>(1)?;
        let (_, deposit) = pallet_proxy::Proxies::<R>::get(R::AccountId::from(account_id.0));
        Ok(U256::from(deposit.saturated_into::<u128>()))
    }

    #[precompile::public("getAnnouncements(bytes32)")]
    #[precompile::view]
    pub fn get_announcements(
        handle: &mut impl PrecompileHandle,
        account_id: H256,
    ) -> EvmResult<(Vec<(H256, H256, u64)>, U256)> {
        handle.record_db_reads::<R>(1)?;
        let (announcements, deposit) =
            pallet_proxy::Announcements::<R>::get(R::AccountId::from(account_id.0));
        let announcements = announcements
            .into_iter()
            .map(|announcement| {
                (
                    H256::from(<R::AccountId as Into<[u8; 32]>>::into(
                        announcement.real().clone(),
                    )),
                    H256::from_slice(announcement.call_hash().as_ref()),
                    (*announcement.height()).unique_saturated_into(),
                )
            })
            .collect();
        Ok((announcements, U256::from(deposit.saturated_into::<u128>())))
    }

    #[precompile::public("getLastCallResult(bytes32)")]
    #[precompile::view]
    pub fn get_last_call_result(
        handle: &mut impl PrecompileHandle,
        account_id: H256,
    ) -> EvmResult<(bool, bool, u8, u8, H256)> {
        handle.record_db_reads::<R>(1)?;
        let Some(result) = pallet_proxy::LastCallResult::<R>::get(R::AccountId::from(account_id.0))
        else {
            return Ok((false, false, 0, 0, H256::zero()));
        };
        match result {
            Ok(()) => Ok((true, true, 0, 0, H256::zero())),
            Err(error) => {
                let (kind, pallet_index, error_data) = dispatch_error_metadata(error);
                Ok((true, false, kind, pallet_index, error_data))
            }
        }
    }

    #[precompile::public("isRealPaysFee(bytes32,bytes32)")]
    #[precompile::view]
    pub fn is_real_pays_fee(
        handle: &mut impl PrecompileHandle,
        real: H256,
        delegate: H256,
    ) -> EvmResult<bool> {
        handle.record_db_reads::<R>(1)?;
        Ok(pallet_proxy::RealPaysFee::<R>::contains_key(
            R::AccountId::from(real.0),
            R::AccountId::from(delegate.0),
        ))
    }

    #[precompile::public("announce(bytes32,bytes32)")]
    pub fn announce(
        handle: &mut impl PrecompileHandle,
        real: H256,
        call_hash: H256,
    ) -> EvmResult<()> {
        let account_id = handle.caller_account_id::<R>();
        let call_hash = DecodeLimit::decode_all_with_depth_limit(1, &mut &call_hash.as_bytes()[..])
            .map_err(|_| PrecompileFailure::Error {
                exit_status: ExitError::Other(
                    "runtime call hash is not compatible with bytes32".into(),
                ),
            })?;
        let call = pallet_proxy::Call::<R>::announce {
            real: <<R as frame_system::Config>::Lookup as StaticLookup>::Source::from(
                real.0.into(),
            ),
            call_hash,
        };
        handle.try_dispatch_runtime_call::<R, _>(call, RawOrigin::Signed(account_id))
    }

    #[precompile::public("removeAnnouncement(bytes32,bytes32)")]
    pub fn remove_announcement(
        handle: &mut impl PrecompileHandle,
        real: H256,
        call_hash: H256,
    ) -> EvmResult<()> {
        let account_id = handle.caller_account_id::<R>();
        let call_hash = DecodeLimit::decode_all_with_depth_limit(1, &mut &call_hash.as_bytes()[..])
            .map_err(|_| PrecompileFailure::Error {
                exit_status: ExitError::Other(
                    "runtime call hash is not compatible with bytes32".into(),
                ),
            })?;
        let call = pallet_proxy::Call::<R>::remove_announcement {
            real: <<R as frame_system::Config>::Lookup as StaticLookup>::Source::from(
                real.0.into(),
            ),
            call_hash,
        };
        handle.try_dispatch_runtime_call::<R, _>(call, RawOrigin::Signed(account_id))
    }

    #[precompile::public("rejectAnnouncement(bytes32,bytes32)")]
    pub fn reject_announcement(
        handle: &mut impl PrecompileHandle,
        delegate: H256,
        call_hash: H256,
    ) -> EvmResult<()> {
        let account_id = handle.caller_account_id::<R>();
        let call_hash = DecodeLimit::decode_all_with_depth_limit(1, &mut &call_hash.as_bytes()[..])
            .map_err(|_| PrecompileFailure::Error {
                exit_status: ExitError::Other(
                    "runtime call hash is not compatible with bytes32".into(),
                ),
            })?;
        let call = pallet_proxy::Call::<R>::reject_announcement {
            delegate: <<R as frame_system::Config>::Lookup as StaticLookup>::Source::from(
                delegate.0.into(),
            ),
            call_hash,
        };
        handle.try_dispatch_runtime_call::<R, _>(call, RawOrigin::Signed(account_id))
    }

    #[precompile::public("setRealPaysFee(bytes32,bool)")]
    pub fn set_real_pays_fee(
        handle: &mut impl PrecompileHandle,
        delegate: H256,
        pays_fee: bool,
    ) -> EvmResult<()> {
        let account_id = handle.caller_account_id::<R>();
        let call = pallet_proxy::Call::<R>::set_real_pays_fee {
            delegate: <<R as frame_system::Config>::Lookup as StaticLookup>::Source::from(
                delegate.0.into(),
            ),
            pays_fee,
        };
        handle.try_dispatch_runtime_call::<R, _>(call, RawOrigin::Signed(account_id))
    }
}

fn dispatch_error_metadata(error: DispatchError) -> (u8, u8, H256) {
    let mut data = [0u8; 32];
    match error {
        DispatchError::Other(_) => (1, 0, H256::zero()),
        DispatchError::CannotLookup => (2, 0, H256::zero()),
        DispatchError::BadOrigin => (3, 0, H256::zero()),
        DispatchError::Module(module) => {
            data[..4].copy_from_slice(&module.error);
            (4, module.index, H256::from(data))
        }
        DispatchError::ConsumerRemaining => (5, 0, H256::zero()),
        DispatchError::NoProviders => (6, 0, H256::zero()),
        DispatchError::TooManyConsumers => (7, 0, H256::zero()),
        DispatchError::Token(_) => (8, 0, H256::zero()),
        DispatchError::Arithmetic(_) => (9, 0, H256::zero()),
        DispatchError::Transactional(_) => (10, 0, H256::zero()),
        DispatchError::Exhausted => (11, 0, H256::zero()),
        DispatchError::Corruption => (12, 0, H256::zero()),
        DispatchError::Unavailable => (13, 0, H256::zero()),
        DispatchError::RootNotAllowed => (14, 0, H256::zero()),
        DispatchError::Trie(_) => (15, 0, H256::zero()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::PrecompileExt;
    use crate::mock::{
        AccountId, Runtime, addr_from_index, new_test_ext, precompiles, selector_u32,
    };
    use precompile_utils::solidity::encode_with_selector;
    use precompile_utils::testing::PrecompileTesterExt;

    #[test]
    fn proxy_state_views_return_typed_values_and_missing_state() {
        new_test_ext().execute_with(|| {
            let caller = addr_from_index(0x80b1);
            let address = addr_from_index(ProxyPrecompile::<Runtime>::INDEX);
            let real = AccountId::from([0x31; 32]);
            let delegate = AccountId::from([0x32; 32]);
            let real_word = H256::from_slice(real.as_ref());
            let delegate_word = H256::from_slice(delegate.as_ref());
            let precompiles = precompiles::<ProxyPrecompile<Runtime>>();

            precompiles
                .prepare_test(
                    caller,
                    address,
                    encode_with_selector(selector_u32("getProxyDeposit(bytes32)"), (real_word,)),
                )
                .with_static_call(true)
                .execute_returns(U256::zero());

            precompiles
                .prepare_test(
                    caller,
                    address,
                    encode_with_selector(
                        selector_u32("getAnnouncements(bytes32)"),
                        (delegate_word,),
                    ),
                )
                .with_static_call(true)
                .execute_returns((Vec::<(H256, H256, u64)>::new(), U256::zero()));

            precompiles
                .prepare_test(
                    caller,
                    address,
                    encode_with_selector(selector_u32("getLastCallResult(bytes32)"), (real_word,)),
                )
                .with_static_call(true)
                .execute_returns((false, false, 0_u8, 0_u8, H256::zero()));

            pallet_proxy::LastCallResult::<Runtime>::insert(
                &real,
                Err::<(), _>(DispatchError::BadOrigin),
            );
            precompiles
                .prepare_test(
                    caller,
                    address,
                    encode_with_selector(selector_u32("getLastCallResult(bytes32)"), (real_word,)),
                )
                .with_static_call(true)
                .execute_returns((true, false, 3_u8, 0_u8, H256::zero()));

            pallet_proxy::RealPaysFee::<Runtime>::insert(&real, &delegate, ());
            precompiles
                .prepare_test(
                    caller,
                    address,
                    encode_with_selector(
                        selector_u32("isRealPaysFee(bytes32,bytes32)"),
                        (real_word, delegate_word),
                    ),
                )
                .with_static_call(true)
                .execute_returns(true);
        });
    }
}
