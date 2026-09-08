use super::*;
use codec::DecodeLength;
use sp_runtime::traits::{Header, Saturating};

pub(crate) struct IndexProof<T: Config> {
    root: T::Hash,
    db: sp_trie::MemoryDB<T::Hashing>,
}

impl<T: Config> Pallet<T> {
    /// Record a bound without loading the vector again. Writers already have its
    /// new length; readers must check this fixed-size record before loading it.
    pub(crate) fn note_hotkey_index_length(key: &[u8], len: usize) {
        HotkeyIndexLengths::<T>::insert(
            sp_io::hashing::blake2_256(key),
            u32::try_from(len).unwrap_or(u32::MAX),
        );
    }

    /// Activate tracking without scanning or decoding any legacy vector.
    pub(crate) fn start_hotkey_index_tracking() -> Weight {
        if HotkeyIndexTrackingSince::<T>::exists() {
            return T::DbWeight::get().reads(1);
        }
        // Runtime upgrades can run before System advances the block number.
        // Require a later header so no accepted snapshot predates tracking.
        HotkeyIndexTrackingSince::<T>::put(
            frame_system::Pallet::<T>::block_number().saturating_add(1u32.into()),
        );
        T::DbWeight::get().reads_writes(1, 1)
    }

    /// Offline invariant check; never scan these unbounded maps during execution.
    #[cfg(any(test, feature = "try-runtime"))]
    pub(crate) fn check_hotkey_index_lengths() -> Result<(), sp_runtime::TryRuntimeError> {
        let indexes =
            OwnedHotkeys::<T>::iter()
                .map(|(coldkey, keys)| (OwnedHotkeys::<T>::hashed_key_for(coldkey), keys.len()))
                .chain(StakingHotkeys::<T>::iter().map(|(coldkey, keys)| {
                    (StakingHotkeys::<T>::hashed_key_for(coldkey), keys.len())
                }))
                .chain(
                    AutoStakeDestinationColdkeys::<T>::iter().map(|(hotkey, netuid, keys)| {
                        (
                            AutoStakeDestinationColdkeys::<T>::hashed_key_for(hotkey, netuid),
                            keys.len(),
                        )
                    }),
                );
        for (key, len) in indexes {
            if let Some(bound) = HotkeyIndexLengths::<T>::get(sp_io::hashing::blake2_256(&key)) {
                ensure!(
                    len <= bound as usize,
                    "Hotkey index length exceeds its recorded bound"
                );
            }
        }
        Ok(())
    }

    pub(crate) fn disassociation_index_proof(
        proof: Option<&DisassociationProof<T>>,
    ) -> Result<Option<IndexProof<T>>, DispatchError> {
        let Some((header, nodes)) = proof else {
            return Ok(None);
        };
        let since =
            HotkeyIndexTrackingSince::<T>::get().ok_or(Error::<T>::InvalidDisassociationWitness)?;
        ensure!(
            *header.number() >= since
                && *header.number() < frame_system::Pallet::<T>::block_number()
                && frame_system::BlockHash::<T>::get(header.number()) == header.hash(),
            Error::<T>::InvalidDisassociationWitness
        );
        Ok(Some(IndexProof {
            root: *header.state_root(),
            db: sp_trie::StorageProof::new(nodes.clone()).into_memory_db::<T::Hashing>(),
        }))
    }

    pub(crate) fn disassociation_index_length(
        key: &[u8],
        proof: Option<&IndexProof<T>>,
    ) -> Result<usize, DispatchError> {
        if let Some(len) = HotkeyIndexLengths::<T>::get(sp_io::hashing::blake2_256(key)) {
            return Ok(len as usize);
        }
        // Missing metadata means no writer has grown this legacy vector since
        // tracking started. A post-activation historical length therefore bounds
        // its current length, even if cleanup has since shortened it. The proof
        // comes from the call, so its entire byte size is charged before execution.
        let proof = proof.ok_or(Error::<T>::InvalidDisassociationWitness)?;
        let value = sp_trie::read_trie_value::<sp_trie::LayoutV1<T::Hashing>, _>(
            &proof.db,
            &proof.root,
            key,
            None,
            None,
        )
        .map_err(|_| Error::<T>::InvalidDisassociationWitness)?;
        value.map_or(Ok(0), |value| {
            <Vec<T::AccountId> as DecodeLength>::len(&value)
                .map_err(|_| Error::<T>::InvalidDisassociationWitness.into())
        })
    }
}
