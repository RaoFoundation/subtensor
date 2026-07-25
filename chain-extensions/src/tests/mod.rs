//! Unit tests for `subtensor-chain-extensions`, split by concept for discoverability.
#![allow(clippy::unwrap_used)]
// Shared imports are consumed by concept modules via `use super::*`.
#![allow(unused_imports)]

use super::{SubtensorChainExtension, SubtensorExtensionEnv, mock};
use crate::types::{ColdkeyLock, FunctionId, Output, StakeAvailability, SubnetRegistrationState};
use codec::{Decode, Encode};
use frame_support::pallet_prelude::Zero;
use frame_support::{assert_ok, weights::Weight};
use frame_system::RawOrigin;
use pallet_contracts::chain_extension::RetVal;
use pallet_subtensor::DefaultMinStake;
use pallet_subtensor::weights::WeightInfo as SubtensorWeightInfo;
use sp_core::Get;
use sp_core::U256;
use sp_runtime::DispatchError;
use substrate_fixed::types::U64F64;
use subtensor_runtime_common::{AlphaBalance, NetUid, TaoBalance, Token};
use subtensor_swap_interface::SwapHandler;

type AccountId = <mock::Test as frame_system::Config>::AccountId;

/// In-memory [`SubtensorExtensionEnv`] for dispatch unit tests (no `pallet-contracts` VM).
#[derive(Clone)]
struct MockEnv {
    func_id: u16,
    caller: AccountId,
    input: Vec<u8>,
    output: Vec<u8>,
    charged_weight: Option<Weight>,
    expected_weight: Option<Weight>,
}

#[allow(dead_code)]
pub fn add_balance_to_coldkey_account(coldkey: &U256, tao: TaoBalance) {
    let credit = pallet_subtensor::Pallet::<mock::Test>::mint_tao(tao);
    let _ = pallet_subtensor::Pallet::<mock::Test>::spend_tao(coldkey, credit, tao).unwrap();
}

impl MockEnv {
    fn new(func_id: FunctionId, caller: AccountId, input: Vec<u8>) -> Self {
        Self {
            func_id: func_id as u16,
            caller,
            input,
            output: Vec::new(),
            charged_weight: None,
            expected_weight: None,
        }
    }

    fn with_expected_weight(mut self, weight: Weight) -> Self {
        self.expected_weight = Some(weight);
        self
    }

    fn charged_weight(&self) -> Option<Weight> {
        self.charged_weight
    }

    fn output(&self) -> &[u8] {
        &self.output
    }
}

impl SubtensorExtensionEnv<mock::Test> for MockEnv {
    fn func_id(&self) -> u16 {
        self.func_id
    }

    fn charge_weight(&mut self, weight: Weight) -> Result<(), DispatchError> {
        let prev = self.charged_weight.unwrap_or_default();
        let cumulative = Weight::from_parts(
            prev.ref_time().checked_add(weight.ref_time()).unwrap(),
            prev.proof_size().checked_add(weight.proof_size()).unwrap(),
        );
        if let Some(expected) = self.expected_weight
            && (cumulative.ref_time() > expected.ref_time()
                || cumulative.proof_size() > expected.proof_size())
        {
            return Err(DispatchError::Other(
                "unexpected weight charged by mock env",
            ));
        }
        self.charged_weight = Some(cumulative);
        Ok(())
    }

    fn read_as<U: codec::Decode + codec::MaxEncodedLen>(&mut self) -> Result<U, DispatchError> {
        U::decode(&mut &self.input[..]).map_err(|_| DispatchError::Other("mock env decode failure"))
    }

    fn write_output(&mut self, data: &[u8]) -> Result<(), DispatchError> {
        self.output.clear();
        self.output.extend_from_slice(data);
        Ok(())
    }

    fn caller(&mut self) -> AccountId {
        self.caller
    }

    fn origin(&mut self) -> pallet_contracts::Origin<mock::Test> {
        pallet_contracts::Origin::Signed(self.caller)
    }
}

fn assert_extension_success(ret: RetVal) {
    match ret {
        RetVal::Converging(code) => {
            assert_eq!(code, Output::Success as u32, "expected success code")
        }
        _ => panic!("unexpected return value"),
    }
}

mod alpha_recycle_burn;
mod auto_stake_hotkey;
mod caller_dispatch;
mod extension_queries;
mod proxy_ops;
mod stake_limit;
mod stake_ops;
