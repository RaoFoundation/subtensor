//! Shared post-quantum and derived records; existing storage encodings are unchanged.
use super::*;
use alloc::vec::Vec;
use frame_support::{
    BoundedVec, ensure,
    traits::{ConstU32, Get},
    transactional,
    weights::Weight,
};
use sp_runtime::DispatchError;

pub const MAX_DERIVED_DEPTH: u8 = 16;
pub const MAX_DERIVED_NODES: u32 = 1024;
pub type DerivedSignatories = BoundedVec<AccountId32, ConstU32<100>>;
pub type EncodedProxyType = BoundedVec<u8, ConstU32<32>>;

#[derive(
    Clone, Debug, PartialEq, Eq, Encode, Decode, DecodeWithMemTracking, TypeInfo, MaxEncodedLen,
)]
pub struct MlDsaAccountRecord {
    pub descriptor: subtensor_hashed::mldsa::Descriptor,
    pub generation: u64,
    pub commitment: [u8; 64],
}

/// The runtime verifier authenticates the complete transcript before passing
/// this ticket through normal transaction validation and preparation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ValidatedMlDsaProof {
    account: AccountId32,
    profile: u8,
    generation: u64,
    commitment: [u8; 64],
    next_commitment: [u8; 64],
}

#[derive(
    Clone,
    Copy,
    Debug,
    PartialEq,
    Eq,
    Encode,
    Decode,
    DecodeWithMemTracking,
    TypeInfo,
    MaxEncodedLen,
)]
pub enum AuthenticationKind {
    Ordinary,
    HashedV1,
    MlDsa,
    DerivedPq,
}

#[derive(
    Clone,
    Copy,
    Debug,
    PartialEq,
    Eq,
    Encode,
    Decode,
    DecodeWithMemTracking,
    TypeInfo,
    MaxEncodedLen,
)]
pub enum SecurityFloor {
    Unrestricted,
    Committed,
    PqRequired,
}

/// Fixed to Subtensor's existing u32 block-height derivation and SCALE-encoded
/// proxy type. The runtime hook must decode/re-encode proxy_type canonically.
#[derive(
    Clone, Debug, PartialEq, Eq, Encode, Decode, DecodeWithMemTracking, TypeInfo, MaxEncodedLen,
)]
pub enum DerivedDescriptor {
    Multisig {
        threshold: u16,
        signatories: DerivedSignatories,
    },
    Derivative {
        parent: AccountId32,
        index: u16,
    },
    PureProxy {
        spawner: AccountId32,
        proxy_type: EncodedProxyType,
        index: u16,
        height: u32,
        extrinsic_index: u32,
    },
}

impl DerivedDescriptor {
    /// Existing FRAME account derivation bytes, not a new identity namespace.
    pub fn account_id(&self) -> AccountId32 {
        let hash = match self {
            Self::Multisig {
                threshold,
                signatories,
            } => (b"modlpy/utilisuba", signatories.as_slice(), threshold)
                .using_encoded(sp_io::hashing::blake2_256),
            Self::Derivative { parent, index } => {
                (b"modlpy/utilisuba", parent, index).using_encoded(sp_io::hashing::blake2_256)
            }
            Self::PureProxy {
                spawner,
                proxy_type,
                index,
                height,
                extrinsic_index,
            } => {
                let mut bytes = (b"modlpy/proxy____", spawner, height, extrinsic_index).encode();
                bytes.extend_from_slice(proxy_type);
                index.encode_to(&mut bytes);
                sp_io::hashing::blake2_256(&bytes)
            }
        };
        AccountId32::new(hash)
    }
}

#[derive(
    Clone, Debug, PartialEq, Eq, Encode, Decode, DecodeWithMemTracking, TypeInfo, MaxEncodedLen,
)]
pub struct DerivedAuthorityRecord {
    pub descriptor: DerivedDescriptor,
    /// Freeze the already-protected members sufficient to intersect every
    /// quorum. Later protection of a weak member cannot expand this proof.
    pub pq_dependencies: DerivedSignatories,
    /// Depth and expanded node count are cached only for immutable dependencies
    /// with permanent PqRequired floors. Shared nodes are counted repeatedly.
    pub depth: u8,
    pub nodes: u32,
}

impl<T: Config> Pallet<T> {
    /// V1 registration retains its historical meaning; security checks should
    /// use this unified query instead of querying Accounts directly.
    pub fn authentication_kind(account: &AccountId32) -> AuthenticationKind {
        if Accounts::<T>::contains_key(account) {
            AuthenticationKind::HashedV1
        } else if MlDsaAccounts::<T>::contains_key(account) {
            AuthenticationKind::MlDsa
        } else if DerivedAuthorities::<T>::contains_key(account) {
            AuthenticationKind::DerivedPq
        } else {
            AuthenticationKind::Ordinary
        }
    }

    pub fn is_protected(account: &AccountId32) -> bool {
        Self::authentication_kind(account) != AuthenticationKind::Ordinary
    }

    pub fn authority_meets(account: &AccountId32, floor: SecurityFloor) -> bool {
        match floor {
            SecurityFloor::Unrestricted => true,
            SecurityFloor::Committed => Self::is_protected(account),
            SecurityFloor::PqRequired => matches!(
                Self::authentication_kind(account),
                AuthenticationKind::MlDsa | AuthenticationKind::DerivedPq
            ),
        }
    }

    /// Worst-case cost of one unified identity query. Callers must also budget
    /// proof sizes of the relevant records in their measured WeightInfo.
    pub fn protected_query_weight() -> Weight {
        T::DbWeight::get().reads(3)
    }

    fn check_new_kind(account: &AccountId32) -> DispatchResult {
        ensure!(
            !Self::is_protected(account),
            Error::<T>::AuthenticationKindConflict
        );
        let alias = Self::evm_alias(account);
        ensure!(
            EvmAliases::<T>::get(alias).is_none_or(|existing| &existing == account),
            Error::<T>::AliasCollision
        );
        Ok(())
    }

    fn retain_account_and_alias(account: &AccountId32) {
        frame_system::Pallet::<T>::inc_providers(account);
        if frame_system::Pallet::<T>::account_nonce(account) == Default::default() {
            frame_system::Pallet::<T>::inc_account_nonce(account);
        }
        EvmAliases::<T>::insert(Self::evm_alias(account), account);
    }

    pub(crate) fn do_register_mldsa(
        sponsor: &AccountId32,
        descriptor: subtensor_hashed::mldsa::Descriptor,
    ) -> DispatchResult {
        ensure!(T::Enabled::get(), Error::<T>::Disabled);
        ensure!(
            descriptor.is_supported() && !descriptor.is_stateless(),
            Error::<T>::UnsupportedDescriptor
        );
        let account = AccountId32::new(subtensor_hashed::mldsa::account_id(&descriptor));
        if let Some(record) = MlDsaAccounts::<T>::get(&account) {
            ensure!(
                record.descriptor == descriptor,
                Error::<T>::DescriptorMismatch
            );
            return Ok(());
        }
        Self::check_new_kind(&account)?;
        T::OnRegister::on_register_mldsa(&account, sponsor, &descriptor)?;
        let deposit = T::MlDsaRegistrationDeposit::get();
        T::Currency::reserve(sponsor, deposit)?;
        Self::retain_account_and_alias(&account);
        MlDsaAccounts::<T>::insert(
            &account,
            MlDsaAccountRecord {
                descriptor,
                generation: 0,
                commitment: descriptor.initial_commitment,
            },
        );
        Self::deposit_event(Event::RegisteredMlDsa {
            account,
            sponsor: sponsor.clone(),
            deposit,
        });
        Ok(())
    }

    /// A dependency's cached policy cannot change: direct ML-DSA records may only
    /// rotate their ML-DSA commitment and protected derived records are immutable.
    fn dependency(account: &AccountId32) -> Result<Option<(u8, u32)>, DispatchError> {
        if MlDsaAccounts::<T>::contains_key(account) {
            return Ok(Some((0, 1)));
        }
        if let Some(record) = DerivedAuthorities::<T>::get(account) {
            ensure!(
                record.depth <= MAX_DERIVED_DEPTH && record.nodes <= MAX_DERIVED_NODES,
                Error::<T>::DerivedAuthorityTooComplex
            );
            return Ok(Some((record.depth, record.nodes)));
        }
        Ok(None)
    }

    fn derived_shape(
        descriptor: &DerivedDescriptor,
    ) -> Result<(u8, u32, DerivedSignatories), DispatchError> {
        let (members, threshold): (Vec<&AccountId32>, usize) = match descriptor {
            DerivedDescriptor::Multisig {
                threshold,
                signatories,
            } => {
                ensure!(
                    signatories.len() >= 2
                        && *threshold > 0
                        && usize::from(*threshold) <= signatories.len()
                        && signatories
                            .iter()
                            .zip(signatories.iter().skip(1))
                            .all(|(left, right)| left < right),
                    Error::<T>::InvalidDerivedAuthority
                );
                (signatories.iter().collect(), usize::from(*threshold))
            }
            DerivedDescriptor::Derivative { parent, .. } => (alloc::vec![parent], 1),
            DerivedDescriptor::PureProxy {
                spawner,
                proxy_type,
                ..
            } => {
                ensure!(!proxy_type.is_empty(), Error::<T>::InvalidDerivedAuthority);
                (alloc::vec![spawner], 1)
            }
        };
        let mut weak = 0usize;
        let mut depth = 0u8;
        let mut nodes = 1u32;
        let mut pq_dependencies = DerivedSignatories::default();
        for member in members {
            if let Some((child_depth, child_nodes)) = Self::dependency(member)? {
                pq_dependencies
                    .try_push(member.clone())
                    .map_err(|_| Error::<T>::DerivedAuthorityTooComplex)?;
                depth = depth.max(child_depth);
                nodes = nodes
                    .checked_add(child_nodes)
                    .ok_or(Error::<T>::DerivedAuthorityTooComplex)?;
            } else {
                weak = weak.saturating_add(1);
                nodes = nodes
                    .checked_add(1)
                    .ok_or(Error::<T>::DerivedAuthorityTooComplex)?;
            }
        }
        ensure!(threshold > weak, Error::<T>::InvalidDerivedAuthority);
        depth = depth
            .checked_add(1)
            .ok_or(Error::<T>::DerivedAuthorityTooComplex)?;
        ensure!(
            depth <= MAX_DERIVED_DEPTH && nodes <= MAX_DERIVED_NODES,
            Error::<T>::DerivedAuthorityTooComplex
        );
        Ok((depth, nodes, pq_dependencies))
    }

    /// Traverse only the frozen sufficient PQ proof. Optional membership edges
    /// may later gain protection, but cannot expand or establish this proof.
    fn ensure_acyclic(
        account: &AccountId32,
        pq_dependencies: &DerivedSignatories,
    ) -> DispatchResult {
        let mut pending: Vec<(AccountId32, u8)> = pq_dependencies
            .iter()
            .map(|member| (member.clone(), 1))
            .collect();
        let mut visited = alloc::collections::btree_set::BTreeSet::new();
        let mut traversed = 0u32;
        while let Some((member, depth)) = pending.pop() {
            ensure!(&member != account, Error::<T>::InvalidDerivedAuthority);
            ensure!(
                depth <= MAX_DERIVED_DEPTH,
                Error::<T>::DerivedAuthorityTooComplex
            );
            traversed = traversed
                .checked_add(1)
                .ok_or(Error::<T>::DerivedAuthorityTooComplex)?;
            ensure!(
                traversed <= MAX_DERIVED_NODES,
                Error::<T>::DerivedAuthorityTooComplex
            );
            if !visited.insert(member.clone()) {
                continue;
            }
            if let Some(record) = DerivedAuthorities::<T>::get(&member) {
                let next = depth
                    .checked_add(1)
                    .ok_or(Error::<T>::DerivedAuthorityTooComplex)?;
                pending.extend(
                    record
                        .pq_dependencies
                        .iter()
                        .map(|child| (child.clone(), next)),
                );
            }
        }
        Ok(())
    }

    /// Entry point for runtime creation hooks as well as the dispatchable. The
    /// runtime registration hook still validates provenance and current grants.
    #[transactional]
    pub fn register_derived_with_sponsor(
        sponsor: &AccountId32,
        descriptor: DerivedDescriptor,
    ) -> DispatchResult {
        Self::register_derived_with_authorization(sponsor, sponsor, descriptor)
    }

    /// Registration authority and deposit consent are independent. Runtime
    /// hooks must authenticate both; a sponsor never substitutes for the owner.
    #[transactional]
    pub fn register_derived_with_authorization(
        owner: &AccountId32,
        deposit_payer: &AccountId32,
        descriptor: DerivedDescriptor,
    ) -> DispatchResult {
        ensure!(T::Enabled::get(), Error::<T>::Disabled);
        let account = descriptor.account_id();
        if let Some(record) = DerivedAuthorities::<T>::get(&account) {
            ensure!(
                record.descriptor == descriptor,
                Error::<T>::DescriptorMismatch
            );
            return Ok(());
        }
        Self::check_new_kind(&account)?;
        let (depth, nodes, pq_dependencies) = Self::derived_shape(&descriptor)?;
        Self::ensure_acyclic(&account, &pq_dependencies)?;
        if deposit_payer != owner {
            T::OnRegister::on_sponsored_deposit(&account, deposit_payer, &descriptor)?;
        }
        T::OnRegister::on_register_derived(&account, owner, &descriptor)?;
        let deposit = T::DerivedRegistrationDeposit::get();
        T::Currency::reserve(deposit_payer, deposit)?;
        Self::retain_account_and_alias(&account);
        DerivedAuthorities::<T>::insert(
            &account,
            DerivedAuthorityRecord {
                descriptor,
                pq_dependencies,
                depth,
                nodes,
            },
        );
        Self::deposit_event(Event::DerivedProtected {
            account,
            sponsor: deposit_payer.clone(),
            deposit,
        });
        Ok(())
    }

    /// Check an authenticated ML-DSA or Falcon identity against current chain state.
    pub fn check_mldsa_authorization(
        account: &AccountId32,
        profile: u8,
        public_key: &[u8],
        generation: u64,
        next_commitment: [u8; 64],
    ) -> Result<ValidatedMlDsaProof, DispatchError> {
        ensure!(T::Enabled::get(), Error::<T>::Disabled);
        if profile == subtensor_hashed::mldsa::FIXED_ALGORITHM {
            let header = subtensor_hashed::mldsa::Header {
                account: account.clone().into(),
                version: subtensor_hashed::mldsa::VERSION,
                algorithm: profile,
                generation,
                public_key: public_key
                    .try_into()
                    .map_err(|_| Error::<T>::WrongCommitment)?,
                next_commitment,
            };
            ensure!(header.is_valid_stateless(), Error::<T>::WrongCommitment);
            return Ok(ValidatedMlDsaProof {
                account: account.clone(),
                profile,
                generation,
                commitment: next_commitment,
                next_commitment,
            });
        }
        if subtensor_hashed::falcon::is_fixed_profile(profile) {
            let header = subtensor_hashed::falcon::Header {
                account: account.clone().into(),
                version: subtensor_hashed::falcon::VERSION,
                algorithm: profile,
                generation,
                public_key: public_key.to_vec(),
                next_commitment,
            };
            ensure!(header.is_valid_stateless(), Error::<T>::WrongCommitment);
            return Ok(ValidatedMlDsaProof {
                account: account.clone(),
                profile,
                generation,
                commitment: next_commitment,
                next_commitment,
            });
        }
        let commitment = if subtensor_hashed::falcon::is_supported_profile(profile) {
            subtensor_hashed::falcon::commitment(profile, public_key)
                .map_err(|_| Error::<T>::WrongCommitment)?
        } else {
            ensure!(
                subtensor_hashed::mldsa::is_account_profile(profile),
                Error::<T>::UnsupportedDescriptor
            );
            let public_key = public_key
                .try_into()
                .map_err(|_| Error::<T>::WrongCommitment)?;
            subtensor_hashed::mldsa::key_commitment(public_key)
        };
        let record = MlDsaAccounts::<T>::get(account).ok_or(Error::<T>::NotRegistered)?;
        ensure!(
            record.descriptor.is_supported() && record.descriptor.algorithm == profile,
            Error::<T>::UnsupportedDescriptor
        );
        ensure!(
            record.generation != u64::MAX,
            Error::<T>::GenerationExhausted
        );
        ensure!(record.generation == generation, Error::<T>::WrongGeneration);
        ensure!(record.commitment == commitment, Error::<T>::WrongCommitment);
        ensure!(
            record
                .descriptor
                .accepts_next_commitment(&record.commitment, &next_commitment),
            Error::<T>::InvalidNextCommitment
        );
        Ok(ValidatedMlDsaProof {
            account: account.clone(),
            profile,
            generation,
            commitment: record.commitment,
            next_commitment,
        })
    }

    /// Consume in transaction-extension preparation, outside inner-call rollback.
    pub fn advance_mldsa(validated: &ValidatedMlDsaProof) -> DispatchResult {
        ensure!(T::Enabled::get(), Error::<T>::Disabled);
        if validated.profile == subtensor_hashed::mldsa::FIXED_ALGORITHM
            || subtensor_hashed::falcon::is_fixed_profile(validated.profile)
        {
            // Only a valid first signature can acquire this reference. Keep the
            // native nonce even after all funds are spent so old signatures
            // cannot become valid again on refund. No identity registry is needed:
            // every later signature proves the same immutable account binding.
            if frame_system::Pallet::<T>::account_nonce(&validated.account) == Default::default() {
                frame_system::Pallet::<T>::inc_sufficients(&validated.account);
            }
            return Ok(());
        }
        let generation = validated
            .generation
            .checked_add(1)
            .ok_or(Error::<T>::GenerationExhausted)?;
        MlDsaAccounts::<T>::try_mutate(&validated.account, |entry| -> DispatchResult {
            let record = entry.as_mut().ok_or(Error::<T>::NotRegistered)?;
            ensure!(
                record.generation == validated.generation,
                Error::<T>::WrongGeneration
            );
            ensure!(
                record.commitment == validated.commitment,
                Error::<T>::WrongCommitment
            );
            ensure!(
                record.descriptor.algorithm == validated.profile
                    && record
                        .descriptor
                        .accepts_next_commitment(&record.commitment, &validated.next_commitment),
                Error::<T>::InvalidNextCommitment
            );
            record.generation = generation;
            record.commitment = validated.next_commitment;
            Ok(())
        })?;
        Ok(())
    }
}
