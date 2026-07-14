use super::*;
use alloc::string::ToString;
use frame_support::ensure;
use frame_system::ensure_signed;

use sp_core::{H160, ecdsa::Signature, hashing::keccak_256};
use sp_std::collections::btree_map::BTreeMap;
use sp_std::vec::Vec;
use subtensor_runtime_common::NetUid;

const MESSAGE_PREFIX: &str = "\x19Ethereum Signed Message:\n";

impl<T: Config> Pallet<T> {
    pub(crate) fn hash_message_eip191<M: AsRef<[u8]>>(message: M) -> [u8; 32] {
        let msg_len = message.as_ref().len().to_string();
        keccak_256(
            &[
                MESSAGE_PREFIX.as_bytes(),
                msg_len.as_bytes(),
                message.as_ref(),
            ]
            .concat(),
        )
    }

    /// Associate an EVM key with a hotkey.
    ///
    /// This function accepts a Signature, which is a signed message containing the hotkey
    /// concatenated with the hashed block number. It will then attempt to recover the EVM key from
    /// the signature and compare it with the `evm_key` parameter, and ensures that they match.
    ///
    /// The EVM key is expected to sign the message according to this formula to produce the
    /// signature:
    /// ```text
    /// keccak_256(hotkey ++ keccak_256(block_number))
    /// ```
    ///
    /// # Arguments
    ///
    /// * `origin`: The origin of the call, which should be the hotkey.
    /// * `netuid`: The unique identifier for the subnet that the hotkey belongs to.
    /// * `evm_key`: The EVM address to associate with the `hotkey`.
    /// * `block_number`: The block number used in the `signature`.
    /// * `signature`: A signed message by the `evm_key` containing the `hotkey` and the hashed
    ///   `block_number`.
    pub fn do_associate_evm_key(
        origin: OriginFor<T>,
        netuid: NetUid,
        evm_key: H160,
        block_number: u64,
        mut signature: Signature,
    ) -> dispatch::DispatchResult {
        let hotkey = ensure_signed(origin)?;
        ensure!(Self::if_subnet_exist(netuid), Error::<T>::SubnetNotExists);
        ensure!(
            Self::get_owning_coldkey_for_hotkey(&hotkey) != DefaultAccount::<T>::get(),
            Error::<T>::NonAssociatedColdKey
        );

        // Normalize the v value to 0 or 1
        if signature.0[64] >= 27 {
            signature.0[64] = signature.0[64].saturating_sub(27);
        }

        let uid = Self::get_uid_for_net_and_hotkey(netuid, &hotkey)?;
        Self::ensure_evm_key_associate_rate_limit(netuid, uid)?;

        let block_hash = keccak_256(block_number.encode().as_ref());
        let message = [
            hotkey.encode().as_ref(),
            <[u8; 32] as AsRef<[u8]>>::as_ref(&block_hash),
        ]
        .concat();
        let public = signature
            .recover_prehashed(&Self::hash_message_eip191(message))
            .ok_or(Error::<T>::InvalidIdentity)?;
        let secp_pubkey = libsecp256k1::PublicKey::parse_compressed(&public.0)
            .map_err(|_| Error::<T>::UnableToRecoverPublicKey)?;
        let uncompressed = secp_pubkey.serialize();
        let hashed_evm_key = H160::from_slice(&keccak_256(&uncompressed[1..])[12..]);

        ensure!(
            evm_key == hashed_evm_key,
            Error::<T>::InvalidRecoveredPublicKey
        );

        Self::ensure_evm_address_index_capacity(netuid, uid, evm_key)?;

        let current_block_number = Self::get_current_block_as_u64();

        Self::set_associated_evm_address(netuid, uid, evm_key, current_block_number);

        Self::deposit_event(Event::EvmKeyAssociated {
            netuid,
            hotkey,
            evm_key,
            block_associated: current_block_number,
        });

        Ok(())
    }

    /// Ensure `evm_key` can accept an association for `uid` on `netuid` without exceeding
    /// [`MAX_ASSOCIATED_UIDS_PER_EVM_ADDRESS`].
    ///
    /// Re-associating a UID that is already tracked for this address (e.g. refreshing its
    /// block) never consumes a new slot, so it is always permitted. This is the only path
    /// that grows a bucket, so enforcing the cap here keeps the forward map
    /// (`AssociatedEvmAddress`) and reverse index (`AssociatedUidsByEvmAddress`) consistent.
    pub fn ensure_evm_address_index_capacity(
        netuid: NetUid,
        uid: u16,
        evm_key: H160,
    ) -> DispatchResult {
        let bucket = AssociatedUidsByEvmAddress::<T>::get(netuid, evm_key);
        let already_tracked = bucket.iter().any(|(stored_uid, _)| *stored_uid == uid);
        ensure!(
            already_tracked || (bucket.len() as u32) < crate::MAX_ASSOCIATED_UIDS_PER_EVM_ADDRESS,
            Error::<T>::EvmKeyAssociationLimitExceeded
        );
        Ok(())
    }

    pub fn set_associated_evm_address(
        netuid: NetUid,
        uid: u16,
        evm_key: H160,
        block_associated: u64,
    ) {
        if let Some((old_evm_key, _)) = AssociatedEvmAddress::<T>::get(netuid, uid)
            && old_evm_key != evm_key
        {
            Self::remove_uid_from_evm_address_index(netuid, old_evm_key, uid);
        }

        Self::upsert_uid_in_evm_address_index(netuid, evm_key, uid, block_associated);
        AssociatedEvmAddress::<T>::insert(netuid, uid, (evm_key, block_associated));
    }

    pub fn remove_associated_evm_address(netuid: NetUid, uid: u16) {
        if let Some((evm_key, _)) = AssociatedEvmAddress::<T>::take(netuid, uid) {
            Self::remove_uid_from_evm_address_index(netuid, evm_key, uid);
        }
    }

    pub fn clear_associated_evm_addresses(netuid: NetUid) {
        let _ = AssociatedEvmAddress::<T>::clear_prefix(netuid, u32::MAX, None);
        let _ = AssociatedUidsByEvmAddress::<T>::clear_prefix(netuid, u32::MAX, None);
    }

    /// Remap the UIDs stored in the EVM-address reverse index after a subnet UID compaction.
    ///
    /// [`Self::trim_to_max_allowed_uids`] compacts kept UIDs into new, lower positions described
    /// by `old_to_new_uid`; trimmed UIDs have already been dropped from both the forward map and
    /// this reverse index. Only the UID *positions* of kept associations change (their EVM key
    /// and block are untouched), so we rewrite each bucket's UIDs through the mapping rather than
    /// clearing the whole index and rebuilding it from the forward map.
    ///
    /// Work is bounded by the number of distinct EVM addresses associated on the subnet: each
    /// reverse-index entry is read once and written at most once, and the forward map is never
    /// re-scanned.
    pub fn remap_associated_evm_address_index(
        netuid: NetUid,
        old_to_new_uid: &BTreeMap<usize, usize>,
    ) {
        // Collect first: rewriting entries while iterating the same storage prefix is unsafe.
        let updates: Vec<_> = AssociatedUidsByEvmAddress::<T>::iter_prefix(netuid)
            .filter_map(|(evm_key, mut bucket)| {
                let mut changed = false;
                for (uid, _) in bucket.iter_mut() {
                    if let Some(&new_uid) = old_to_new_uid.get(&(*uid as usize))
                        && *uid != new_uid as u16
                    {
                        *uid = new_uid as u16;
                        changed = true;
                    }
                }
                changed.then_some((evm_key, bucket))
            })
            .collect();

        for (evm_key, bucket) in updates {
            AssociatedUidsByEvmAddress::<T>::insert(netuid, evm_key, bucket);
        }
    }

    fn upsert_uid_in_evm_address_index(
        netuid: NetUid,
        evm_key: H160,
        uid: u16,
        block_associated: u64,
    ) {
        AssociatedUidsByEvmAddress::<T>::mutate(netuid, evm_key, |uids| {
            if let Some((_, stored_block)) =
                uids.iter_mut().find(|(stored_uid, _)| *stored_uid == uid)
            {
                *stored_block = block_associated;
                return;
            }

            if uids.try_push((uid, block_associated)).is_err() {
                log::error!(
                    "AssociatedUidsByEvmAddress overflow for netuid {netuid:?}, evm_key {evm_key:?}"
                );
            }
        });
    }

    fn remove_uid_from_evm_address_index(netuid: NetUid, evm_key: H160, uid: u16) {
        AssociatedUidsByEvmAddress::<T>::mutate_exists(netuid, evm_key, |maybe_uids| {
            let Some(uids) = maybe_uids else {
                return;
            };

            uids.retain(|(stored_uid, _)| *stored_uid != uid);

            if uids.is_empty() {
                *maybe_uids = None;
            }
        });
    }

    pub fn uid_lookup(netuid: NetUid, evm_key: H160, limit: u16) -> Vec<(u16, u64)> {
        let mut ret_val = AssociatedUidsByEvmAddress::<T>::get(netuid, evm_key)
            .into_iter()
            .collect::<Vec<(u16, u64)>>();
        ret_val.sort_by(|(_, block1), (_, block2)| block1.cmp(block2));
        ret_val.truncate(limit as usize);
        ret_val
    }

    pub fn ensure_evm_key_associate_rate_limit(netuid: NetUid, uid: u16) -> DispatchResult {
        let now = Self::get_current_block_as_u64();
        let block_associated = match AssociatedEvmAddress::<T>::get(netuid, uid) {
            Some((_, block_associated)) => block_associated,
            None => 0,
        };
        let block_diff = now.saturating_sub(block_associated);
        if block_diff < T::EvmKeyAssociateRateLimit::get() {
            return Err(Error::<T>::EvmKeyAssociateRateLimitExceeded.into());
        }
        Ok(())
    }
}
