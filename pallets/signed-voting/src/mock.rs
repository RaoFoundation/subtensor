#![allow(
    clippy::arithmetic_side_effects,
    clippy::unwrap_used,
    clippy::expect_used
)]

use core::cell::RefCell;
use std::collections::BTreeMap;

use frame_support::{
    derive_impl,
    pallet_prelude::*,
    parameter_types,
    sp_runtime::{BuildStorage, traits::IdentityLookup},
    weights::constants::RocksDbWeight,
};
use sp_core::U256;
use subtensor_runtime_common::{OnPollCompleted, OnPollCreated, Polls, SetLike, VoteTally};

use crate::{self as pallet_signed_voting};

type Block = frame_system::mocking::MockBlock<Test>;

frame_support::construct_runtime!(
    pub enum Test {
        System: frame_system = 1,
        SignedVoting: pallet_signed_voting = 2,
    }
);

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
    /// Used to exercise the scheme-mismatch rejection in `vote` / `remove_vote`.
    Anonymous,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SimpleVoterSet(pub Vec<U256>);

impl SetLike<U256> for SimpleVoterSet {
    fn contains(&self, who: &U256) -> bool {
        self.0.contains(who)
    }
    fn len(&self) -> u32 {
        self.0.len() as u32
    }
    fn is_initialized(&self) -> bool {
        true
    }
    fn to_vec(&self) -> Vec<U256> {
        self.0.clone()
    }
}

#[derive(Clone)]
pub struct PollState {
    pub is_ongoing: bool,
    pub scheme: Option<VotingScheme>,
    pub voter_set: Vec<U256>,
}

thread_local! {
    static POLLS_STATE: RefCell<BTreeMap<u32, PollState>> =
        const { RefCell::new(BTreeMap::new()) };
    static TALLY_UPDATES: RefCell<Vec<(u32, VoteTally)>> =
        const { RefCell::new(Vec::new()) };
}

pub struct MockPolls;

impl Polls<U256> for MockPolls {
    type Index = u32;
    type VotingScheme = VotingScheme;
    type VoterSet = SimpleVoterSet;

    fn is_ongoing(index: Self::Index) -> bool {
        POLLS_STATE.with(|p| {
            p.borrow()
                .get(&index)
                .map(|s| s.is_ongoing)
                .unwrap_or(false)
        })
    }

    fn voting_scheme_of(index: Self::Index) -> Option<Self::VotingScheme> {
        POLLS_STATE.with(|p| p.borrow().get(&index).and_then(|s| s.scheme))
    }

    fn voter_set_of(index: Self::Index) -> Option<Self::VoterSet> {
        POLLS_STATE.with(|p| {
            p.borrow()
                .get(&index)
                .map(|s| SimpleVoterSet(s.voter_set.clone()))
        })
    }

    fn on_tally_updated(index: Self::Index, tally: &VoteTally) {
        TALLY_UPDATES.with(|t| t.borrow_mut().push((index, *tally)));
    }

    fn on_tally_updated_weight() -> Weight {
        Weight::zero()
    }
}

/// Register a poll and fire `on_poll_created` so `TallyOf` / `ActivePolls`
/// are populated. After this returns, the pallet sees the poll as ongoing.
pub fn start_poll(index: u32, scheme: VotingScheme, voter_set: Vec<U256>) {
    POLLS_STATE.with(|p| {
        p.borrow_mut().insert(
            index,
            PollState {
                is_ongoing: true,
                scheme: Some(scheme),
                voter_set,
            },
        );
    });
    <SignedVoting as OnPollCreated<u32>>::on_poll_created(index);
}

/// Mark the poll inactive and fire `on_poll_completed` to clean up storage.
pub fn complete_poll(index: u32) {
    POLLS_STATE.with(|p| {
        if let Some(s) = p.borrow_mut().get_mut(&index) {
            s.is_ongoing = false;
        }
    });
    <SignedVoting as OnPollCompleted<u32>>::on_poll_completed(index);
}

/// Simulate a membership rotation in the underlying collective by removing
/// `who` from the mock's `Polls::voter_set_of` view. Used to assert that
/// signed-voting is unaffected: the eligibility roster is whatever was
/// snapshotted into `VoterSetOf` at `on_poll_created`, regardless of later
/// changes here.
pub fn rotate_voter_out(index: u32, who: U256) {
    POLLS_STATE.with(|p| {
        if let Some(s) = p.borrow_mut().get_mut(&index) {
            s.voter_set.retain(|v| *v != who);
        }
    });
}

/// Simulate adding a member to the underlying collective after the poll
/// snapshot was taken. The new member must not gain voting rights on the
/// existing poll.
pub fn rotate_voter_in(index: u32, who: U256) {
    POLLS_STATE.with(|p| {
        if let Some(s) = p.borrow_mut().get_mut(&index)
            && !s.voter_set.contains(&who)
        {
            s.voter_set.push(who);
        }
    });
}

/// Simulate a producer that reports `is_ongoing = true` while
/// `voting_scheme_of` returns `None`. Used to reach the `PollNotFound`
/// branch in `ensure_valid_voting_scheme`.
pub fn force_scheme_none(index: u32) {
    POLLS_STATE.with(|p| {
        if let Some(s) = p.borrow_mut().get_mut(&index) {
            s.scheme = None;
        }
    });
}

pub fn take_tally_updates() -> Vec<(u32, VoteTally)> {
    TALLY_UPDATES.with(|t| t.borrow_mut().drain(..).collect())
}

pub fn signed_voting_events() -> Vec<crate::Event<Test>> {
    System::events()
        .into_iter()
        .filter_map(|r| match r.event {
            RuntimeEvent::SignedVoting(e) => Some(e),
            _ => None,
        })
        .collect()
}

#[derive_impl(frame_system::config_preludes::TestDefaultConfig)]
impl frame_system::Config for Test {
    type Block = Block;
    type AccountId = U256;
    type Lookup = IdentityLookup<Self::AccountId>;
    // Use the production weight table so `on_idle` weight assertions
    // catch regressions that the default `DbWeight = ()` would mask.
    type DbWeight = RocksDbWeight;
}

macro_rules! define_scoped_state {
    ($flag:ident, $guard:ident, $reader:ident, $ty:ty, $default:expr) => {
        thread_local! {
            static $flag: RefCell<$ty> = const { RefCell::new($default) };
        }

        #[must_use = "the guard restores the prior value on drop; bind it to a local"]
        pub struct $guard {
            previous: Option<$ty>,
        }

        impl $guard {
            pub fn new(value: $ty) -> Self {
                let previous =
                    Some($flag.with(|r| core::mem::replace(&mut *r.borrow_mut(), value)));
                Self { previous }
            }
        }

        impl Drop for $guard {
            fn drop(&mut self) {
                if let Some(prev) = self.previous.take() {
                    $flag.with(|r| *r.borrow_mut() = prev);
                }
            }
        }

        fn $reader() -> $ty {
            $flag.with(|r| *r.borrow())
        }
    };
}

define_scoped_state!(
    MAX_VOTER_SET_SIZE,
    MaxVoterSetSizeGuard,
    max_voter_set_size,
    u32,
    256
);
define_scoped_state!(
    MAX_PENDING_CLEANUP,
    MaxPendingCleanupGuard,
    max_pending_cleanup,
    u32,
    32
);
define_scoped_state!(
    CLEANUP_CHUNK_SIZE,
    CleanupChunkSizeGuard,
    cleanup_chunk_size,
    u32,
    4
);

parameter_types! {
    pub const TestScheme: VotingScheme = VotingScheme::Signed;
    pub const TestCleanupCursorMaxLen: u32 = 128;
    pub TestMaxVoterSetSize: u32 = max_voter_set_size();
    pub TestMaxPendingCleanup: u32 = max_pending_cleanup();
    pub TestCleanupChunkSize: u32 = cleanup_chunk_size();
}

impl pallet_signed_voting::Config for Test {
    type Scheme = TestScheme;
    type Polls = MockPolls;
    type MaxVoterSetSize = TestMaxVoterSetSize;
    type MaxPendingCleanup = TestMaxPendingCleanup;
    type CleanupChunkSize = TestCleanupChunkSize;
    type CleanupCursorMaxLen = TestCleanupCursorMaxLen;
    type WeightInfo = pallet_signed_voting::weights::SubstrateWeight<Test>;
    #[cfg(feature = "runtime-benchmarks")]
    type BenchmarkHelper = MockBenchmarkHelper;
}

/// Benchmark bootstrap for the mock. Registers a poll directly in
/// `POLLS_STATE` so `MockPolls::is_ongoing` and `voting_scheme_of`
/// return the values the benchmark expects.
#[cfg(feature = "runtime-benchmarks")]
pub struct MockBenchmarkHelper;

#[cfg(feature = "runtime-benchmarks")]
impl pallet_signed_voting::benchmarking::BenchmarkHelper<Test> for MockBenchmarkHelper {
    fn ongoing_poll() -> u32 {
        let index: u32 = 0;
        POLLS_STATE.with(|p| {
            p.borrow_mut().insert(
                index,
                PollState {
                    is_ongoing: true,
                    scheme: Some(VotingScheme::Signed),
                    // Voter set populated directly by the benchmark via
                    // `populate_snapshot`.
                    voter_set: alloc::vec::Vec::new(),
                },
            );
        });
        index
    }
}

pub fn new_test_ext() -> sp_io::TestExternalities {
    let mut ext: sp_io::TestExternalities = RuntimeGenesisConfig::default()
        .build_storage()
        .unwrap()
        .into();
    ext.execute_with(|| {
        System::set_block_number(1);
        POLLS_STATE.with(|p| p.borrow_mut().clear());
        let _ = take_tally_updates();
    });
    ext
}

pub struct TestState;

impl TestState {
    pub fn build_and_execute(test: impl FnOnce()) {
        new_test_ext().execute_with(test);
    }
}
