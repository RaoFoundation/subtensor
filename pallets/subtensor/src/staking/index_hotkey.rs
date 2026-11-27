//! Index hotkeys: one keyless hotkey per subnet that spreads its stake weight over the
//! subnet's leading validators.
//!
//! Staking to a subnet's index hotkey is ordinary staking: `add_stake`, `remove_stake`, the
//! same share pool. What differs is where the stake weight goes. An index hotkey has no key
//! and no UID, so it cannot validate. It lends all of its stake weight to children that a
//! fixed rule picks from the subnet's last epoch ([`Pallet::index_children`]), and what those
//! children earn on it flows back into its pool as the dividend of any parent does. Nobody
//! chooses the validators, and no stake moves when they change.
//!
//! An index hotkey differs from an ordinary hotkey in exactly these ways:
//!
//! - it may have [`MAX_INDEX_CHILDREN`] children rather than five;
//! - its delegate take is zero and cannot change, so its pool keeps all it is paid;
//! - the protocol sets its children: no owner signs, and no stake threshold applies;
//! - nobody can register it, own it by registration, swap to it or make it a child.

use super::*;
use frame_support::{PalletId, weights::Weight};
use sp_runtime::{PerU16, traits::AccountIdConversion};
use subtensor_runtime_common::NetUid;

/// Marks an account of the pallet as an index hotkey.
pub const INDEX_HOTKEY_TAG: [u8; 4] = *b"idx/";

/// The most children an index hotkey lends its stake weight to.
pub const MAX_INDEX_CHILDREN: usize = 8;

/// The largest part of its stake weight an index hotkey lends to one child: a quarter.
pub const MAX_INDEX_CHILD_PROPORTION: u64 = u64::MAX / 4;

/// How many of a subnet's validators the rule looks at, best first, before it stops.
pub const MAX_INDEX_CANDIDATES: usize = 32;

/// The most subnets that can have an index hotkey.
pub const MAX_INDEX_SUBNETS: u32 = 1_024;

impl<T: Config> Pallet<T> {
    /// The index hotkey of `netuid`: an account of this pallet that has no key.
    pub fn index_hotkey(netuid: NetUid) -> T::AccountId {
        T::SubtensorPalletId::get().into_sub_account_truncating((INDEX_HOTKEY_TAG, netuid))
    }

    /// The subnet `account` is the index hotkey of, if it is one. Reads no storage.
    pub fn index_hotkey_netuid(account: &T::AccountId) -> Option<NetUid> {
        match PalletId::try_from_sub_account::<([u8; 4], NetUid)>(account) {
            Some((pallet_id, (tag, netuid)))
                if pallet_id == T::SubtensorPalletId::get() && tag == INDEX_HOTKEY_TAG =>
            {
                Some(netuid)
            }
            _ => None,
        }
    }

    /// Whether `account` is an index hotkey. Reads no storage.
    pub fn is_index_hotkey(account: &T::AccountId) -> bool {
        Self::index_hotkey_netuid(account).is_some()
    }

    /// Whether `hotkey` is an account of the protocol that no coldkey may register or swap to.
    pub fn is_system_hotkey(hotkey: &T::AccountId) -> bool {
        Self::is_subnet_account_id(hotkey).is_some() || Self::is_index_hotkey(hotkey)
    }

    /// The most children `hotkey` may have on a subnet.
    pub fn max_children(hotkey: &T::AccountId) -> usize {
        if Self::is_index_hotkey(hotkey) {
            MAX_INDEX_CHILDREN
        } else {
            crate::MAX_CHILDREN
        }
    }

    /// Opens the index hotkey of `netuid` for staking, owned by `owner`, and sets its first
    /// children. Opening it again for the same owner changes nothing.
    ///
    /// The owner earns nothing by owning it: the take is zero. Ownership only exempts the
    /// owner's position from the sweep of small nominations, which matters to an owner that
    /// holds stake for others. An index hotkey has one owner for ever.
    pub fn do_open_index(netuid: NetUid, owner: &T::AccountId) -> DispatchResult {
        ensure!(
            !netuid.is_root() && Self::if_subnet_exist(netuid),
            Error::<T>::SubnetNotExists
        );
        let hotkey = Self::index_hotkey(netuid);
        match Owner::<T>::try_get(&hotkey) {
            Ok(current) => ensure!(current == *owner, Error::<T>::NonAssociatedColdKey),
            Err(_) => {
                Self::ensure_hotkey_owner_policy(owner, &hotkey)?;
                Owner::<T>::insert(&hotkey, owner);
                OwnedHotkeys::<T>::mutate(owner, |hotkeys| {
                    if !hotkeys.contains(&hotkey) {
                        hotkeys.push(hotkey.clone());
                    }
                });
            }
        }
        Delegates::<T>::insert(&hotkey, PerU16::zero());
        IndexSubnets::<T>::try_mutate(|subnets| {
            if !subnets.contains(&netuid) {
                subnets
                    .try_push(netuid)
                    .map_err(|_| Error::<T>::TooManyIndexSubnets)?;
            }
            Ok::<(), Error<T>>(())
        })?;
        Self::refresh_index(netuid);
        Ok(())
    }

    /// The children the index hotkey of `netuid` should have now, with their proportions.
    ///
    /// The rule, on the outputs of the subnet's last epoch:
    ///
    /// 1. take the UIDs that hold a validator permit and earned a dividend;
    /// 2. order them by dividend, highest first, then by UID;
    /// 3. walk at most [`MAX_INDEX_CANDIDATES`] of them and keep those whose childkey take
    ///    is no higher than the highest delegate take, and that can accept another parent;
    /// 4. stop at [`MAX_INDEX_CHILDREN`];
    /// 5. lend each the same proportion, at most [`MAX_INDEX_CHILD_PROPORTION`].
    ///
    /// With four children or more all of the stake weight is lent. With fewer, a quarter
    /// goes to each and the rest earns nothing until more validators qualify.
    pub fn index_children(netuid: NetUid) -> Vec<(u64, T::AccountId)> {
        let index = Self::index_hotkey(netuid);
        let dividends = Dividends::<T>::get(netuid);
        let mut candidates: Vec<(PerU16, u16)> = ValidatorPermit::<T>::get(netuid)
            .into_iter()
            .zip(dividends)
            .enumerate()
            .filter(|(_, (permit, dividend))| *permit && !dividend.is_zero())
            .filter_map(|(uid, (_, dividend))| Some((dividend, u16::try_from(uid).ok()?)))
            .collect();
        candidates.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));

        let max_take = MaxDelegateTake::<T>::get().deconstruct();
        let mut children: Vec<T::AccountId> = Vec::new();
        for (_, uid) in candidates.into_iter().take(MAX_INDEX_CANDIDATES) {
            let Ok(hotkey) = Keys::<T>::try_get(netuid, uid) else {
                continue;
            };
            let parents = ParentKeys::<T>::get(&hotkey, netuid);
            let has_room = parents.len() < crate::staking::set_children::MAX_PARENTS_PER_CHILD
                || parents.iter().any(|(_, parent)| *parent == index);
            if has_room
                && !Self::is_index_hotkey(&hotkey)
                && !children.contains(&hotkey)
                && Self::get_childkey_take(&hotkey, netuid) <= max_take
            {
                children.push(hotkey);
            }
            if children.len() == MAX_INDEX_CHILDREN {
                break;
            }
        }

        let proportion = u64::MAX
            .checked_div(children.len() as u64)
            .unwrap_or_default()
            .min(MAX_INDEX_CHILD_PROPORTION);
        children
            .into_iter()
            .map(|child| (proportion, child))
            .collect()
    }

    /// Sets the children of the index hotkey of `netuid` to what the rule gives now. They
    /// take effect at once: the rule reads consensus outputs, so there is no owner's choice
    /// for nominators to wait out.
    pub fn refresh_index(netuid: NetUid) {
        let index = Self::index_hotkey(netuid);
        let children = Self::index_children(netuid);
        let mut current = ChildKeys::<T>::get(&index, netuid);
        let mut wanted = children.clone();
        current.sort();
        wanted.sort();
        if current != wanted {
            Self::persist_pending_chidren_ok(netuid, &index, &children);
        }
        IndexRefreshedAt::<T>::insert(netuid, Self::get_current_block_as_u64());
    }

    /// The weight of one refresh: the two epoch vectors, five reads for each candidate, the
    /// child and parent lists rewritten on both sides, and the bookkeeping.
    fn index_refresh_weight() -> Weight {
        let candidates = MAX_INDEX_CANDIDATES as u64;
        let children = MAX_INDEX_CHILDREN as u64;
        T::DbWeight::get()
            .reads(
                8u64.saturating_add(candidates.saturating_mul(5))
                    .saturating_add(children.saturating_mul(4)),
            )
            .saturating_add(
                T::DbWeight::get().writes(3u64.saturating_add(children.saturating_mul(2))),
            )
    }

    /// Refreshes at most one index hotkey, within `limit`. Each subnet with an index has
    /// its turn every so many blocks as there are such subnets, and is refreshed on its
    /// turn if [`PendingChildKeyCooldown`] has passed since its last refresh: the same time
    /// an ordinary parent waits for a change of children. Returns the weight used.
    pub fn process_index_refresh(limit: Weight) -> Weight {
        let scan = T::DbWeight::get().reads(3);
        if !scan
            .saturating_add(Self::index_refresh_weight())
            .all_lte(limit)
        {
            return Weight::zero();
        }
        let subnets = IndexSubnets::<T>::get();
        let now = Self::get_current_block_as_u64();
        let Some(netuid) = now
            .checked_rem(subnets.len() as u64)
            .and_then(|turn| subnets.get(usize::try_from(turn).ok()?))
            .copied()
        else {
            return T::DbWeight::get().reads(1);
        };
        if !Self::if_subnet_exist(netuid) {
            // The subnet was dissolved: its index is closed with it.
            IndexSubnets::<T>::mutate(|subnets| subnets.retain(|listed| *listed != netuid));
            IndexRefreshedAt::<T>::remove(netuid);
            return scan.saturating_add(T::DbWeight::get().writes(2));
        }
        let due =
            IndexRefreshedAt::<T>::get(netuid).saturating_add(PendingChildKeyCooldown::<T>::get());
        if now < due {
            return scan;
        }
        Self::refresh_index(netuid);
        scan.saturating_add(Self::index_refresh_weight())
    }
}
