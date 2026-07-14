use alloc::vec::Vec;

use pallet_multi_collective::CollectiveInspect;
use sp_runtime::traits::UniqueSaturatedInto;
use subtensor_runtime_common::SetLike;

use crate::{AccountId, MultiCollective};

use super::collectives::CollectiveId;

/// A voter or proposer set composed of one or more collectives.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MemberSet {
    Single(CollectiveId),
    Union(Vec<CollectiveId>),
}

impl MemberSet {
    fn contains_with<A, F>(&self, who: &A, lookup: F) -> bool
    where
        F: Fn(CollectiveId, &A) -> bool,
    {
        match self {
            Self::Single(id) => lookup(*id, who),
            Self::Union(ids) => ids.iter().any(|id| lookup(*id, who)),
        }
    }

    // Union members can overlap across collectives; dedup so the count
    // signed-voting captures as `total` reflects true cardinality and
    // does not bias thresholds upward.
    fn to_vec_with<A, F>(&self, lookup: F) -> Vec<A>
    where
        A: Ord,
        F: Fn(CollectiveId) -> Vec<A>,
    {
        match self {
            Self::Single(id) => lookup(*id),
            Self::Union(ids) => {
                let mut accounts: Vec<A> = Vec::new();
                for id in ids {
                    accounts.extend(lookup(*id));
                }
                accounts.sort();
                accounts.dedup();
                accounts
            }
        }
    }

    fn is_initialized_with<F>(&self, lookup: F) -> bool
    where
        F: Fn(CollectiveId) -> bool,
    {
        match self {
            Self::Single(id) => lookup(*id),
            Self::Union(ids) if ids.is_empty() => true,
            Self::Union(ids) => ids.iter().any(|id| lookup(*id)),
        }
    }
}

impl SetLike<AccountId> for MemberSet {
    fn contains(&self, who: &AccountId) -> bool {
        use CollectiveInspect as CI;
        use MultiCollective as MC;

        self.contains_with(who, |id, who| {
            <MC as CI<AccountId, CollectiveId>>::is_member(id, who)
        })
    }

    fn len(&self) -> u32 {
        self.to_vec().len().unique_saturated_into()
    }

    fn is_initialized(&self) -> bool {
        use CollectiveInspect as CI;
        use MultiCollective as MC;

        self.is_initialized_with(<MC as CI<AccountId, CollectiveId>>::is_initialized)
    }

    fn to_vec(&self) -> Vec<AccountId> {
        use CollectiveInspect as CI;
        use MultiCollective as MC;

        self.to_vec_with(<MC as CI<AccountId, CollectiveId>>::members_of)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make(ids: &[u32]) -> Vec<u32> {
        ids.to_vec()
    }

    #[test]
    fn single_delegates_to_lookup() {
        let set = MemberSet::Single(CollectiveId::Triumvirate);
        let out = set.to_vec_with::<u32, _>(|id| match id {
            CollectiveId::Triumvirate => make(&[1, 2, 3]),
            _ => make(&[]),
        });
        assert_eq!(out, vec![1, 2, 3]);
    }

    #[test]
    fn union_concatenates_and_dedups() {
        let set = MemberSet::Union(alloc::vec![CollectiveId::Economic, CollectiveId::Building,]);
        let out = set.to_vec_with::<u32, _>(|id| match id {
            CollectiveId::Economic => make(&[1, 2, 3]),
            CollectiveId::Building => make(&[3, 4, 5]),
            _ => make(&[]),
        });
        assert_eq!(out, vec![1, 2, 3, 4, 5]);
    }

    #[test]
    fn union_with_no_ids_is_empty() {
        let set = MemberSet::Union(alloc::vec![]);
        let out = set.to_vec_with::<u32, _>(|_| make(&[1, 2]));
        assert!(out.is_empty());
    }

    #[test]
    fn single_contains_uses_only_named_collective() {
        let set = MemberSet::Single(CollectiveId::Proposers);
        let lookup = |id: CollectiveId, who: &u32| -> bool {
            matches!(id, CollectiveId::Proposers) && *who == 7
        };
        assert!(set.contains_with(&7, lookup));
        assert!(!set.contains_with(&8, lookup));
    }

    #[test]
    fn union_contains_short_circuits_on_first_match() {
        let set = MemberSet::Union(alloc::vec![CollectiveId::Economic, CollectiveId::Building,]);
        let lookup = |id: CollectiveId, who: &u32| -> bool {
            matches!(id, CollectiveId::Building) && *who == 42
        };
        assert!(set.contains_with(&42, lookup));
        assert!(!set.contains_with(&1, lookup));
    }
}
