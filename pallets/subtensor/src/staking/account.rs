use super::*;
use codec::{Compact, MaxEncodedLen};
use frame_support::storage::StoragePrefixedMap;

impl<T: Config> Pallet<T> {
    pub fn do_try_associate_hotkey(
        coldkey: &T::AccountId,
        hotkey: &T::AccountId,
    ) -> DispatchResult {
        // Ensure the hotkey is not already associated with a coldkey
        Self::create_account_if_non_existent(coldkey, hotkey)?;

        Ok(())
    }

    /// Release an idle hotkey without transferring stake or financial entitlements.
    /// The work limit bounds subnet probes, decoded index entries and cleanup.
    pub fn do_disassociate_hotkey(
        coldkey: &T::AccountId,
        hotkey: &T::AccountId,
        max_items: u32,
    ) -> DispatchResult {
        let owner = Owner::<T>::try_get(hotkey).map_err(|_| Error::<T>::HotKeyAccountNotExists)?;
        ensure!(&owner == coldkey, Error::<T>::NonAssociatedColdKey);
        ensure!(
            Self::is_subnet_account_id(hotkey).is_none(),
            Error::<T>::CannotUseSystemAccount
        );
        Self::ensure_beta_basket_seed_idle()?;

        let indexed_hotkeys =
            Self::disassociation_index_length(&OwnedHotkeys::<T>::hashed_key_for(coldkey))?
                .saturating_add(Self::disassociation_index_length(
                    &StakingHotkeys::<T>::hashed_key_for(coldkey),
                )?);
        let mut remaining = max_items;
        Self::consume_disassociation_items(&mut remaining, indexed_hotkeys)?;

        // Check keys rather than amounts: even zero-valued legacy rows must be
        // settled by their owning subsystem before this account can be reused.
        ensure!(
            IsNetworkMember::<T>::iter_key_prefix(hotkey)
                .next()
                .is_none(),
            Error::<T>::HotKeyAlreadyRegisteredInSubNet
        );
        ensure!(
            Alpha::<T>::iter_key_prefix((hotkey,)).next().is_none()
                && AlphaV2::<T>::iter_key_prefix((hotkey,)).next().is_none(),
            Error::<T>::HotkeyHasOutstandingStake
        );
        ensure!(
            BasketShares::<T>::get(hotkey) == 0
                && PendingBasketDeposits::<T>::iter_key_prefix(hotkey)
                    .next()
                    .is_none()
                && !RootClaimable::<T>::contains_key(hotkey),
            Error::<T>::HotkeyHasOutstandingRewards
        );
        for staker in BasketClaimed::<T>::iter_key_prefix(hotkey) {
            Self::consume_disassociation_items(&mut remaining, 1)?;
            ensure!(
                BasketClaimed::<T>::get(hotkey, staker) == 0,
                Error::<T>::HotkeyHasOutstandingRewards
            );
        }
        ensure!(
            ChildKeys::<T>::iter_key_prefix(hotkey).next().is_none()
                && ParentKeys::<T>::iter_key_prefix(hotkey).next().is_none(),
            Error::<T>::HotkeyHasActiveRelationships
        );

        // Pending relations have no inverse index until they are applied.
        // Check both directions, charging each parent row before reading its
        // value. Scheduling caps each parent's vector at five children.
        for (netuid, parent) in PendingChildKeys::<T>::iter_keys() {
            Self::consume_disassociation_items(&mut remaining, 1)?;
            ensure!(&parent != hotkey, Error::<T>::HotkeyHasActiveRelationships);
            let (children, _) = PendingChildKeys::<T>::try_get(netuid, &parent)
                .map_err(|_| Error::<T>::InvalidDisassociationWitness)?;
            ensure!(
                children.iter().all(|(_, child)| child != hotkey),
                Error::<T>::HotkeyHasActiveRelationships
            );
        }

        // For the remaining maps, count distinct netuid buckets, not account
        // rows. Raw trie seeks skip unrelated accounts within each bucket.
        for (_, subnet_owner) in SubnetOwnerHotkey::<T>::iter() {
            Self::consume_disassociation_items(&mut remaining, 1)?;
            ensure!(
                &subnet_owner != hotkey,
                Error::<T>::HotkeyHasActiveRelationships
            );
        }
        Self::ensure_disassociation_subnets(
            &Uids::<T>::final_prefix(),
            &mut remaining,
            Error::<T>::HotKeyAlreadyRegisteredInSubNet,
            |netuid| !Uids::<T>::contains_key(netuid, hotkey),
        )?;
        Self::ensure_disassociation_subnets(
            &LockingColdkeys::<T>::final_prefix(),
            &mut remaining,
            Error::<T>::HotkeyHasOutstandingStake,
            |netuid| {
                LockingColdkeys::<T>::iter_key_prefix((netuid, hotkey))
                    .next()
                    .is_none()
            },
        )?;
        Self::ensure_disassociation_subnets(
            &MinerCollateral::<T>::final_prefix(),
            &mut remaining,
            Error::<T>::HotkeyHasOutstandingStake,
            |netuid| {
                MinerCollateral::<T>::iter_key_prefix((netuid, hotkey))
                    .next()
                    .is_none()
            },
        )?;
        Self::ensure_disassociation_subnets(
            &RootClaimed::<T>::final_prefix(),
            &mut remaining,
            Error::<T>::HotkeyHasOutstandingRewards,
            |netuid| {
                RootClaimed::<T>::iter_key_prefix((netuid, hotkey))
                    .next()
                    .is_none()
            },
        )?;
        for netuid in AutoStakeDestinationColdkeys::<T>::iter_key_prefix(hotkey) {
            Self::consume_disassociation_items(&mut remaining, 1)?;
            Self::consume_disassociation_items(
                &mut remaining,
                Self::disassociation_index_length(
                    &AutoStakeDestinationColdkeys::<T>::hashed_key_for(hotkey, netuid),
                )?,
            )?;
        }

        // All fallible checks precede mutation, including checks for references
        // on later subnets. Never let a stale inverse index erase a new target.
        for (netuid, stakers) in AutoStakeDestinationColdkeys::<T>::drain_prefix(hotkey) {
            for staker in stakers {
                if AutoStakeDestination::<T>::get(&staker, netuid).as_ref() == Some(hotkey) {
                    AutoStakeDestination::<T>::remove(&staker, netuid);
                }
            }
        }
        OwnedHotkeys::<T>::mutate_exists(coldkey, |maybe_hotkeys| {
            if let Some(hotkeys) = maybe_hotkeys {
                hotkeys.retain(|key| key != hotkey);
                if hotkeys.is_empty() {
                    *maybe_hotkeys = None;
                }
            }
        });
        Self::maybe_remove_staking_hotkey(hotkey, coldkey);
        for _ in BasketClaimed::<T>::drain_prefix(hotkey) {}
        BasketRate::<T>::remove(hotkey);
        Owner::<T>::remove(hotkey);
        Delegates::<T>::remove(hotkey);
        AutoParentDelegationEnabled::<T>::remove(hotkey);
        // Cooldowns, swap lineage and the hotkey's own EVM association survive
        // ownership changes, so disassociation cannot reset those protections.
        Self::deposit_event(Event::HotkeyDisassociated {
            coldkey: coldkey.clone(),
            hotkey: hotkey.clone(),
        });
        Ok(())
    }

    /// Copy only the SCALE length prefix into the runtime before decoding an
    /// index. The host still reads the full existing Vec value and includes it
    /// in storage proofs; max_items does not bound that underlying storage read.
    fn disassociation_index_length(key: &[u8]) -> Result<usize, DispatchError> {
        let mut prefix = [0u8; 5];
        let Some(encoded_len) = sp_io::storage::read(key, &mut prefix, 0) else {
            return Ok(0);
        };
        let prefix_len = (encoded_len as usize).min(prefix.len());
        let Compact(len) = Compact::<u32>::decode(&mut &prefix[..prefix_len])
            .map_err(|_| Error::<T>::InvalidDisassociationWitness)?;
        Ok(len as usize)
    }

    fn consume_disassociation_items(remaining: &mut u32, count: usize) -> DispatchResult {
        let count = u32::try_from(count).map_err(|_| Error::<T>::InvalidDisassociationWitness)?;
        *remaining = remaining
            .checked_sub(count)
            .ok_or(Error::<T>::InvalidDisassociationWitness)?;
        Ok(())
    }

    /// Seek across Identity-hashed u16 netuid buckets without decoding values
    /// or visiting the account rows within each bucket. All supplied maps have
    /// one or two Blake2_128Concat account keys after the leading netuid.
    fn ensure_disassociation_subnets(
        prefix: &[u8],
        remaining: &mut u32,
        error: Error<T>,
        is_clear: impl Fn(NetUid) -> bool,
    ) -> DispatchResult {
        let mut cursor = prefix.to_vec();
        let suffix_len = T::AccountId::max_encoded_len()
            .saturating_add(16)
            .saturating_mul(2);
        let bucket_end = prefix.len().saturating_add(2);
        while let Some(mut key) = sp_io::storage::next_key(&cursor) {
            if !key.starts_with(prefix) {
                break;
            }
            Self::consume_disassociation_items(remaining, 1)?;
            let bytes = key
                .get(prefix.len()..bucket_end)
                .ok_or(Error::<T>::InvalidDisassociationWitness)?;
            let raw_netuid = u16::decode(&mut &bytes[..])
                .map_err(|_| Error::<T>::InvalidDisassociationWitness)?;
            ensure!(is_clear(NetUid::from(raw_netuid)), error);
            key.truncate(bucket_end);
            key.resize(bucket_end.saturating_add(suffix_len), u8::MAX);
            cursor = key;
        }
        Ok(())
    }
}
