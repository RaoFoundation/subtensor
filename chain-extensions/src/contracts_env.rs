//! Contracts VM adapter: bridges `pallet-contracts` [`Environment`] to [`SubtensorExtensionEnv`].

use codec::{Decode, MaxEncodedLen};
use frame_system::RawOrigin;
use pallet_contracts::chain_extension::{BufInBufOutState, Environment, Ext, InitState};
use sp_runtime::{DispatchError, Weight};
use sp_std::marker::PhantomData;

/// Environment surface used by [`crate::SubtensorChainExtension`] dispatch helpers.
///
/// Production code uses [`ContractsEnvAdapter`]; unit tests supply an in-memory mock.
pub(crate) trait SubtensorExtensionEnv<T>
where
    T: pallet_contracts::Config,
{
    fn func_id(&self) -> u16;
    fn charge_weight(&mut self, weight: Weight) -> Result<(), DispatchError>;
    fn read_as<U: Decode + MaxEncodedLen>(&mut self) -> Result<U, DispatchError>;
    fn write_output(&mut self, data: &[u8]) -> Result<(), DispatchError>;
    /// Contract address (`ext.address()`), used by non-`Caller*` function ids as the signed origin.
    fn caller(&mut self) -> T::AccountId;
    /// Transaction origin (`ext.caller()`), used by `Caller*` function ids.
    #[allow(dead_code)]
    fn origin(&mut self) -> pallet_contracts::Origin<T>;
}

/// Map a `pallet-contracts` origin into a FRAME [`RawOrigin`] for pallet dispatch.
pub(crate) fn contracts_origin_as_raw<T>(
    origin: pallet_contracts::Origin<T>,
) -> RawOrigin<T::AccountId>
where
    T: pallet_contracts::Config,
{
    match origin {
        pallet_contracts::Origin::Signed(caller) => RawOrigin::Signed(caller),
        pallet_contracts::Origin::Root => RawOrigin::Root,
    }
}

/// Buf-in/buf-out wrapper around the contracts chain-extension [`Environment`].
pub(crate) struct ContractsEnvAdapter<'a, 'b, T, E>
where
    T: pallet_subtensor::Config + pallet_contracts::Config,
    E: Ext<T = T>,
{
    env: Environment<'a, 'b, E, BufInBufOutState>,
    _marker: PhantomData<T>,
}

impl<'a, 'b, T, E> ContractsEnvAdapter<'a, 'b, T, E>
where
    T: pallet_subtensor::Config + pallet_contracts::Config,
    T::AccountId: Clone,
    E: Ext<T = T>,
{
    pub(crate) fn new(env: Environment<'a, 'b, E, InitState>) -> Self {
        Self {
            env: env.buf_in_buf_out(),
            _marker: PhantomData,
        }
    }
}

impl<'a, 'b, T, E> SubtensorExtensionEnv<T> for ContractsEnvAdapter<'a, 'b, T, E>
where
    T: pallet_subtensor::Config + pallet_contracts::Config,
    T::AccountId: Clone,
    E: Ext<T = T>,
{
    fn func_id(&self) -> u16 {
        self.env.func_id()
    }

    fn charge_weight(&mut self, weight: Weight) -> Result<(), DispatchError> {
        self.env.charge_weight(weight).map(|_| ())
    }

    fn read_as<U: Decode + MaxEncodedLen>(&mut self) -> Result<U, DispatchError> {
        self.env.read_as()
    }

    fn write_output(&mut self, data: &[u8]) -> Result<(), DispatchError> {
        self.env.write(data, false, None)
    }

    fn caller(&mut self) -> T::AccountId {
        self.env.ext().address().clone()
    }

    fn origin(&mut self) -> pallet_contracts::Origin<T> {
        self.env.ext().caller()
    }
}
