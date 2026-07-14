//! Runtime governance wiring.
//!
//! This module connects Subtensor's concrete governance model to three
//! generic pallets:
//!
//! - `pallet_multi_collective`: stores named membership sets.
//! - `pallet_referenda`: owns proposal lifecycle, scheduling, and root dispatch.
//! - `pallet_signed_voting`: records per-account aye/nay votes over referendum
//!   voter-set snapshots.
//!
//! The runtime governance path is intentionally two-stage:
//!
//! 1. Track 0 (`triumvirate`) is the only directly-submittable track. Members
//!    of the `Proposers` collective may submit root calls, and the
//!    `Triumvirate` collective decides by 2-of-3 signed vote.
//! 2. Approval on track 0 delegates the call to track 1 (`review`). Track 1 has
//!    `proposer_set: None`, so it cannot be submitted to directly. Its voters
//!    are the deduplicated union of the `Economic` and `Building` collectives.
//!
//! Collective selection is split by stakeholder role:
//!
//! - `Economic` rotates to the top root-registered coldkeys by governance
//!   stake-value EMA.
//! - `Building` rotates to the top subnet-owner coldkeys by each owner's best
//!   mature subnet moving price.
//! - `EconomicEligible` is a non-voting staging set synchronized from root
//!   registration and used as the candidate pool for `Economic`.
//!
//! Keep the safety invariants close to the code:
//!
//! - `CollectiveId` codec indices are consensus-facing.
//! - Track 1 must remain non-submittable; otherwise proposers could bypass
//!   Triumvirate approval and schedule root calls straight into review.
//! - Signed-voting snapshots voter sets at poll creation, so rotations do not
//!   change eligibility for already-open referenda.
//!
//! See `runtime/src/governance/README.md` for the full operator-facing
//! explanation and selection details.

mod collectives;
mod ema_provider;
mod member_set;
mod term_management;
mod tracks;
mod weights;

#[cfg(feature = "runtime-benchmarks")]
pub mod benchmarking;

pub use self::collectives::*;
pub use self::ema_provider::*;
pub use self::member_set::*;
pub use self::term_management::*;
pub use self::tracks::*;

use codec::{Decode, DecodeWithMemTracking, Encode, MaxEncodedLen};
use frame_support::parameter_types;
use frame_support::traits::AsEnsureOriginWithArg;
use frame_system::EnsureRoot;
use scale_info::TypeInfo;

use crate::{
    AccountId, Preimage, Referenda, Runtime, RuntimeCall, Scheduler, SignedVoting, System,
};

parameter_types! {
    /// Storage cap shared by all collectives; sized for the widest one
    /// (`EconomicEligible`). Per-collective `info.max_members` are the
    /// logical caps; this is just the `BoundedVec` capacity.
    pub const MaxMembers: u32 = collectives::ECONOMIC_ELIGIBLE_SIZE;
}

impl pallet_multi_collective::Config for Runtime {
    type CollectiveId = CollectiveId;
    type Collectives = Collectives;
    type AddOrigin = AsEnsureOriginWithArg<EnsureRoot<AccountId>>;
    type RemoveOrigin = AsEnsureOriginWithArg<EnsureRoot<AccountId>>;
    type SwapOrigin = AsEnsureOriginWithArg<EnsureRoot<AccountId>>;
    type SetOrigin = AsEnsureOriginWithArg<EnsureRoot<AccountId>>;
    type RotateOrigin = AsEnsureOriginWithArg<EnsureRoot<AccountId>>;
    type OnMembersChanged = ();
    type OnNewTerm = TermManagement;
    type MaxMembers = MaxMembers;
    type WeightInfo = pallet_multi_collective::weights::SubstrateWeight<Runtime>;
    #[cfg(feature = "runtime-benchmarks")]
    type BenchmarkHelper = MultiCollectiveBenchmarkHelper;
}

#[cfg(feature = "runtime-benchmarks")]
pub struct MultiCollectiveBenchmarkHelper;

#[cfg(feature = "runtime-benchmarks")]
impl pallet_multi_collective::BenchmarkHelper<Runtime> for MultiCollectiveBenchmarkHelper {
    fn collective() -> CollectiveId {
        CollectiveId::EconomicEligible
    }

    fn rotatable_collective() -> CollectiveId {
        CollectiveId::Economic
    }
}

/// Voting scheme for each referenda track.
#[derive(
    Copy,
    Clone,
    PartialEq,
    Eq,
    Debug,
    Encode,
    Decode,
    DecodeWithMemTracking,
    MaxEncodedLen,
    TypeInfo,
)]
pub enum VotingScheme {
    Signed,
}

parameter_types! {
    pub const Scheme: VotingScheme = VotingScheme::Signed;
    /// Headroom over the widest track's voter set (see guard below).
    pub const MaxVoterSetSize: u32 = 64;
    /// 2x `MaxQueued` for headroom; queue overflow leaks `VotingFor` storage.
    pub const MaxPendingCleanup: u32 = 40;
    /// `VotingFor` entries drained per `on_idle` step. A full poll drains
    /// in `MaxVoterSetSize / CleanupChunkSize` idle blocks.
    pub const CleanupChunkSize: u32 = 16;
    /// Resume cursor for chunked cleanup; 128 bytes covers any FRAME
    /// double-map partial trie key.
    pub const CleanupCursorMaxLen: u32 = 128;
}

impl pallet_signed_voting::Config for Runtime {
    type Scheme = Scheme;
    type Polls = Referenda;
    type MaxVoterSetSize = MaxVoterSetSize;
    type MaxPendingCleanup = MaxPendingCleanup;
    type CleanupChunkSize = CleanupChunkSize;
    type CleanupCursorMaxLen = CleanupCursorMaxLen;
    type WeightInfo = pallet_signed_voting::weights::SubstrateWeight<Runtime>;
    #[cfg(feature = "runtime-benchmarks")]
    type BenchmarkHelper = SignedVotingBenchmarkHelper;
}

#[cfg(feature = "runtime-benchmarks")]
pub struct SignedVotingBenchmarkHelper;

#[cfg(feature = "runtime-benchmarks")]
impl pallet_signed_voting::benchmarking::BenchmarkHelper<Runtime> for SignedVotingBenchmarkHelper {
    #[allow(clippy::expect_used)]
    fn ongoing_poll() -> u32 {
        use self::ReferendaBenchmarkHelper as RBH;
        use pallet_referenda::{
            BenchmarkHelper as BH, ReferendumCount, ReferendumStatus, ReferendumStatusFor,
        };
        use sp_runtime::Perbill;
        use subtensor_runtime_common::VoteTally;

        let proposer = <RBH as BH<u8, AccountId, RuntimeCall>>::proposer();
        <RBH as BH<u8, AccountId, RuntimeCall>>::seed_collective_members();
        let track = <RBH as BH<u8, AccountId, RuntimeCall>>::track_passorfail();
        let call = <RBH as BH<u8, AccountId, RuntimeCall>>::call();
        let parent = ReferendumCount::<Runtime>::get();

        Referenda::submit(
            frame_system::RawOrigin::Signed(proposer).into(),
            track,
            sp_std::boxed::Box::new(call),
        )
        .expect("submit must succeed in benchmark setup");

        let child = ReferendumCount::<Runtime>::get();
        let mut info = match ReferendumStatusFor::<Runtime>::get(parent) {
            Some(ReferendumStatus::Ongoing(info)) => info,
            _ => panic!("expected ongoing referendum"),
        };
        info.tally = VoteTally {
            approval: Perbill::one(),
            rejection: Perbill::zero(),
            abstention: Perbill::zero(),
        };
        ReferendumStatusFor::<Runtime>::insert(parent, ReferendumStatus::Ongoing(info));

        Referenda::advance_referendum(frame_system::RawOrigin::Root.into(), parent)
            .expect("advance must create review poll in benchmark setup");
        assert!(matches!(
            ReferendumStatusFor::<Runtime>::get(child),
            Some(ReferendumStatus::Ongoing(_))
        ));
        child
    }
}

parameter_types! {
    pub const MaxQueued: u32 = 20;
    pub const MaxActivePerProposer: u32 = 5;
}

impl pallet_referenda::Config for Runtime {
    type RuntimeCall = RuntimeCall;
    type Scheduler = Scheduler;
    type Preimages = Preimage;
    type MaxQueued = MaxQueued;
    type MaxActivePerProposer = MaxActivePerProposer;
    type KillOrigin = EnsureRoot<AccountId>;
    type Tracks = tracks::Tracks;
    type AdjustmentCurve = tracks::EaseOutAdjustmentCurve;
    type BlockNumberProvider = System;
    type OnPollCreated = SignedVoting;
    type OnPollCompleted = SignedVoting;
    type WeightInfo = pallet_referenda::weights::SubstrateWeight<Runtime>;
    #[cfg(feature = "runtime-benchmarks")]
    type BenchmarkHelper = ReferendaBenchmarkHelper;
}

#[cfg(feature = "runtime-benchmarks")]
pub struct ReferendaBenchmarkHelper;

#[cfg(feature = "runtime-benchmarks")]
#[allow(clippy::expect_used)]
impl pallet_referenda::BenchmarkHelper<u8, AccountId, RuntimeCall> for ReferendaBenchmarkHelper {
    fn track_passorfail() -> u8 {
        0
    }

    fn track_adjustable() -> u8 {
        1
    }

    fn proposer() -> AccountId {
        use frame_system::RawOrigin;
        use pallet_multi_collective::Pallet as MultiCollective;
        use sp_core::crypto::AccountId32;

        let proposer: AccountId = AccountId32::new([1u8; 32]).into();
        MultiCollective::<Runtime>::add_member(
            RawOrigin::Root.into(),
            CollectiveId::Proposers,
            proposer.clone(),
        )
        .expect("add proposer must succeed in benchmark setup");

        proposer
    }

    fn seed_collective_members() {
        use frame_system::RawOrigin;
        use pallet_multi_collective::Pallet as MultiCollective;
        use sp_core::crypto::AccountId32;

        MultiCollective::<Runtime>::add_member(
            RawOrigin::Root.into(),
            CollectiveId::Triumvirate,
            AccountId32::new([2u8; 32]).into(),
        )
        .expect("add triumvirate member must succeed in benchmark setup");
        MultiCollective::<Runtime>::add_member(
            RawOrigin::Root.into(),
            CollectiveId::Economic,
            AccountId32::new([3u8; 32]).into(),
        )
        .expect("add economic member must succeed in benchmark setup");
        MultiCollective::<Runtime>::add_member(
            RawOrigin::Root.into(),
            CollectiveId::Building,
            AccountId32::new([4u8; 32]).into(),
        )
        .expect("add building member must succeed in benchmark setup");
    }

    fn call() -> RuntimeCall {
        RuntimeCall::System(frame_system::Call::remark {
            remark: alloc::vec![],
        })
    }
}

// Compile-time guards on the relationships between the constants above.
// A misconfiguration here would degrade signed-voting silently (oversized
// voter set collapses to an empty snapshot, queue overflow leaks state),
// so catch the obvious foot-guns at build time.
const _: () = {
    // The widest track today is `Union(Economic, Building)`. Union members
    // can overlap (a coldkey may sit in both), so this sum is an upper
    // bound on the voter set's true cardinality before `MemberSet::Union`'s
    // dedup runs.
    let widest_union = (collectives::ECONOMIC_SIZE as u64) + (collectives::BUILDING_SIZE as u64);
    assert!(
        MaxVoterSetSize::get() as u64 >= widest_union,
        "MaxVoterSetSize must fit the widest track's voter set",
    );
    assert!(
        MaxVoterSetSize::get() >= MaxMembers::get(),
        "MaxVoterSetSize must fit any single-collective track",
    );
    assert!(
        MaxPendingCleanup::get() >= MaxQueued::get(),
        "MaxPendingCleanup must absorb at least one full simultaneous-completion event from `pallet-referenda`",
    );
};
