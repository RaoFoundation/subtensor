use super::*;
use codec::MaxEncodedLen;
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
    /// The shared work limit bounds all reads and cleanup before any mutation.
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

        let indexed_hotkeys = OwnedHotkeys::<T>::decode_len(coldkey)
            .unwrap_or(0)
            .saturating_add(StakingHotkeys::<T>::decode_len(coldkey).unwrap_or(0));
        let mut remaining = max_items;
        Self::consume_disassociation_items(&mut remaining, indexed_hotkeys)?;

        // Check keys rather than amounts: even zero-valued legacy rows must be
        // settled by their owning subsystem before this account can be reused.
        ensure!(
            IsNetworkMember::<T>::iter_key_prefix(hotkey)
                .next()
                .is_none(),
            Error::<T>::HotkeyIsStillRegistered
        );
        ensure!(
            Alpha::<T>::iter_key_prefix((hotkey,)).next().is_none()
                && AlphaV2::<T>::iter_key_prefix((hotkey,)).next().is_none(),
            Error::<T>::HotkeyHasOutstandingStake
        );
        ensure!(
            BasketShares::<T>::get(hotkey) == 0
                && !BasketRate::<T>::contains_key(hotkey)
                && BasketClaimed::<T>::iter_key_prefix(hotkey).next().is_none()
                && PendingBasketDeposits::<T>::iter_key_prefix(hotkey)
                    .next()
                    .is_none()
                && !RootClaimable::<T>::contains_key(hotkey),
            Error::<T>::HotkeyHasOutstandingRewards
        );
        ensure!(
            ChildKeys::<T>::iter_key_prefix(hotkey).next().is_none()
                && ParentKeys::<T>::iter_key_prefix(hotkey).next().is_none(),
            Error::<T>::HotkeyHasActiveRelationships
        );

        // Count distinct netuid buckets, not account rows. A raw trie seek
        // skips every other account in a bucket, so unrelated nominators cannot
        // make validation unbounded. The work limit covers all maps together.
        for (_, subnet_owner) in SubnetOwnerHotkey::<T>::iter() {
            Self::consume_disassociation_items(&mut remaining, 1)?;
            ensure!(
                &subnet_owner != hotkey,
                Error::<T>::HotkeyHasActiveRelationships
            );
        }
        Self::ensure_disassociation_subnets(
            &PendingChildKeys::<T>::final_prefix(),
            &mut remaining,
            Error::<T>::HotkeyHasActiveRelationships,
            |netuid| !PendingChildKeys::<T>::contains_key(netuid, hotkey),
        )?;
        Self::ensure_disassociation_subnets(
            &Uids::<T>::final_prefix(),
            &mut remaining,
            Error::<T>::HotkeyIsStillRegistered,
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
                AutoStakeDestinationColdkeys::<T>::decode_len(hotkey, netuid).unwrap_or(0),
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
