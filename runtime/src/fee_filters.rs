//! Extrinsics whose transaction fee is charged to the signing hotkey's owning
//! coldkey instead of the hotkey itself. Every entry resolves its signed origin as
//! a hotkey; the coldkey is charged only when that hotkey has an owner, otherwise
//! the signer pays (see [`ColdkeyFeeCallFilter`]). Extend one line per extrinsic as
//! more hotkey-origin calls are opted in.

use crate::transaction_payment_wrapper::ColdkeyFeeCallFilter;
use crate::{Runtime, RuntimeCall};
use frame_support::traits::Contains;
use pallet_commitments::Call as CommitmentsCall;
use pallet_subtensor::Call as SubtensorCall;
use subtensor_macros::call_filter_group;

call_filter_group!(
    ColdkeyPaysFeeCalls,
    [
        RuntimeCall::SubtensorModule(SubtensorCall::set_weights),
        RuntimeCall::SubtensorModule(SubtensorCall::set_mechanism_weights),
        RuntimeCall::SubtensorModule(SubtensorCall::batch_set_weights),
        RuntimeCall::SubtensorModule(SubtensorCall::commit_weights),
        RuntimeCall::SubtensorModule(SubtensorCall::commit_mechanism_weights),
        RuntimeCall::SubtensorModule(SubtensorCall::batch_commit_weights),
        RuntimeCall::SubtensorModule(SubtensorCall::commit_crv3_mechanism_weights),
        RuntimeCall::SubtensorModule(SubtensorCall::commit_timelocked_weights),
        RuntimeCall::SubtensorModule(SubtensorCall::commit_timelocked_mechanism_weights),
        RuntimeCall::SubtensorModule(SubtensorCall::reveal_weights),
        RuntimeCall::SubtensorModule(SubtensorCall::reveal_mechanism_weights),
        RuntimeCall::SubtensorModule(SubtensorCall::batch_reveal_weights),
        RuntimeCall::SubtensorModule(SubtensorCall::serve_axon),
        RuntimeCall::SubtensorModule(SubtensorCall::serve_axon_tls),
        RuntimeCall::SubtensorModule(SubtensorCall::serve_prometheus),
        RuntimeCall::SubtensorModule(SubtensorCall::associate_evm_key),
        RuntimeCall::Commitments(CommitmentsCall::set_commitment),
    ]
);

impl ColdkeyFeeCallFilter<RuntimeCall> for Runtime {
    fn charges_coldkey(call: &RuntimeCall) -> bool {
        ColdkeyPaysFeeCalls::contains(call)
    }
}
