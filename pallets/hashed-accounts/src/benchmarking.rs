//! Reference benchmark definitions. No locally measured weights are committed.
#![allow(clippy::unwrap_used)]

use super::*;
use frame_benchmarking::v2::*;
use frame_system::RawOrigin;
use sp_runtime::traits::Bounded;

fn setup<T: Config>() -> Result<
    (
        AccountId32,
        Descriptor,
        AccountId32,
        sp_core::sr25519::Public,
    ),
    BenchmarkError,
> {
    let sponsor: AccountId32 = whitelisted_caller();
    let _ = T::Currency::make_free_balance_be(&sponsor, BalanceOf::<T>::max_value());
    let key = sp_io::crypto::sr25519_generate(
        sp_core::crypto::key_types::ACCOUNT,
        Some(b"//HashedBenchmark".to_vec()),
    );
    let descriptor = Descriptor {
        version: 1,
        scheme: Scheme::Sr25519,
        initial_commitment: subtensor_hashed::key_commitment(Scheme::Sr25519, &key.0),
    };
    let account = AccountId32::new(subtensor_hashed::account_id(&descriptor));
    T::OnRegister::setup_benchmark(&account, &sponsor, &descriptor)?;
    Ok((sponsor, descriptor, account, key))
}

fn setup_mldsa<T: Config>() -> Result<
    (
        AccountId32,
        subtensor_hashed::mldsa::Descriptor,
        AccountId32,
        [u8; 32],
    ),
    BenchmarkError,
> {
    let sponsor: AccountId32 = whitelisted_caller();
    let _ = T::Currency::make_free_balance_be(&sponsor, BalanceOf::<T>::max_value());
    let seed = [57; 32];
    let key = subtensor_hashed::mldsa::public_key_from_seed(&seed).map_err(BenchmarkError::Stop)?;
    let descriptor = subtensor_hashed::mldsa::Descriptor {
        version: 2,
        algorithm: 4,
        initial_commitment: subtensor_hashed::mldsa::key_commitment(&key),
    };
    let account = AccountId32::new(subtensor_hashed::mldsa::account_id(&descriptor));
    T::OnRegister::setup_benchmark_mldsa(&account, &sponsor, &descriptor)?;
    Ok((sponsor, descriptor, account, seed))
}

fn setup_derived<T: Config>() -> Result<(AccountId32, DerivedDescriptor), BenchmarkError> {
    let (sponsor, descriptor, leaf, _) = setup_mldsa::<T>()?;
    Pallet::<T>::register(RawOrigin::Signed(sponsor.clone()).into(), descriptor.into())?;
    // Largest membership with a valid near-limit expanded dependency graph.
    // Each cached dependency is constructed from a real immutable parent.
    let mut members = alloc::vec::Vec::new();
    for member in 0u16..100 {
        let mut parent = leaf.clone();
        let chain_length: u8 = if member == 99 {
            15
        } else if member < 17 {
            10
        } else {
            9
        };
        for depth in 1u8..=chain_length {
            let pq_dependencies = alloc::vec![parent.clone()]
                .try_into()
                .map_err(|_| BenchmarkError::Stop("dependency bound"))?;
            let child = DerivedDescriptor::Derivative {
                parent,
                index: member.saturating_mul(16).saturating_add(u16::from(depth)),
            };
            parent = child.account_id();
            DerivedAuthorities::<T>::insert(
                &parent,
                DerivedAuthorityRecord {
                    descriptor: child,
                    pq_dependencies,
                    depth,
                    nodes: u32::from(depth).saturating_add(1),
                },
            );
        }
        members.push(parent);
    }
    members.sort();
    let descriptor = DerivedDescriptor::Multisig {
        threshold: 100,
        signatories: members
            .try_into()
            .map_err(|_| BenchmarkError::Stop("signatory bound"))?,
    };
    let owner = descriptor.account_id();
    let _ = T::Currency::make_free_balance_be(&owner, BalanceOf::<T>::max_value());
    T::OnRegister::setup_benchmark_derived(&owner, &owner, &descriptor)?;
    Ok((owner, descriptor))
}

#[benchmarks]
mod benchmarks {
    use super::*;

    #[benchmark]
    fn install_capability() -> Result<(), BenchmarkError> {
        let (sponsor, descriptor, account, _) = setup_mldsa::<T>()?;
        Pallet::<T>::register(RawOrigin::Signed(sponsor).into(), descriptor.into())?;
        let scope = CapabilityScope::Message {
            audience: alloc::vec![42; 128]
                .try_into()
                .map_err(|_| BenchmarkError::Stop("audience bound"))?,
        };
        #[extrinsic_call]
        _(
            RawOrigin::Signed(account.clone()),
            1,
            [9; 64],
            u64::MAX,
            scope,
        );
        assert_eq!(
            CapabilityEpochs::<T>::get(account, CapabilityRole::Message),
            1
        );
        Ok(())
    }

    #[benchmark]
    fn revoke_capability() -> Result<(), BenchmarkError> {
        let (sponsor, descriptor, account, _) = setup_mldsa::<T>()?;
        Pallet::<T>::register(RawOrigin::Signed(sponsor).into(), descriptor.into())?;
        Pallet::<T>::install_capability(
            RawOrigin::Signed(account.clone()).into(),
            1,
            [9; 64],
            u64::MAX,
            CapabilityScope::Message {
                audience: alloc::vec![42;128]
                    .try_into()
                    .map_err(|_| BenchmarkError::Stop("audience bound"))?,
            },
        )?;
        #[extrinsic_call]
        _(RawOrigin::Signed(account.clone()), CapabilityRole::Message);
        assert!(ActiveCapabilities::<T>::get(account.clone(), CapabilityRole::Message).is_none());
        assert_eq!(
            CapabilityEpochs::<T>::get(account, CapabilityRole::Message),
            2
        );
        Ok(())
    }

    #[benchmark]
    fn register_mldsa65() -> Result<(), BenchmarkError> {
        let (sponsor, descriptor, account, _) = setup_mldsa::<T>()?;
        #[extrinsic_call]
        register(
            RawOrigin::Signed(sponsor),
            RegistrationDescriptor::MlDsa65(descriptor),
        );
        assert!(MlDsaAccounts::<T>::contains_key(account));
        Ok(())
    }

    #[benchmark]
    fn check_registered_mldsa65() -> Result<(), BenchmarkError> {
        let (sponsor, descriptor, account, _) = setup_mldsa::<T>()?;
        Pallet::<T>::register(RawOrigin::Signed(sponsor.clone()).into(), descriptor.into())?;
        #[extrinsic_call]
        check_registered(
            RawOrigin::Signed(sponsor),
            RegistrationDescriptor::MlDsa65(descriptor),
        );
        assert_eq!(MlDsaAccounts::<T>::get(account).unwrap().generation, 0);
        Ok(())
    }

    #[benchmark]
    fn register_derived() -> Result<(), BenchmarkError> {
        let (sponsor, descriptor) = setup_derived::<T>()?;
        let account = descriptor.account_id();
        #[extrinsic_call]
        _(RawOrigin::Signed(sponsor), descriptor);
        assert!(DerivedAuthorities::<T>::contains_key(account));
        Ok(())
    }

    #[benchmark]
    fn register_derived_sponsored() -> Result<(), BenchmarkError> {
        let (owner, descriptor) = setup_derived::<T>()?;
        let payer: AccountId32 = whitelisted_caller();
        let _ = T::Currency::make_free_balance_be(&payer, BalanceOf::<T>::max_value());
        T::OnRegister::setup_benchmark_sponsored(&owner, &payer, &descriptor)?;
        #[extrinsic_call]
        _(RawOrigin::Signed(owner.clone()), descriptor, payer);
        assert!(DerivedAuthorities::<T>::contains_key(owner));
        Ok(())
    }

    #[benchmark]
    fn check_derived() -> Result<(), BenchmarkError> {
        let (sponsor, descriptor) = setup_derived::<T>()?;
        Pallet::<T>::register_derived(
            RawOrigin::Signed(sponsor.clone()).into(),
            descriptor.clone(),
        )?;
        #[extrinsic_call]
        _(RawOrigin::Signed(sponsor), descriptor.clone());
        assert!(DerivedAuthorities::<T>::contains_key(
            descriptor.account_id()
        ));
        Ok(())
    }

    #[benchmark]
    fn authorize_mldsa65(n: Linear<0, 10485760>) -> Result<(), BenchmarkError> {
        let (sponsor, descriptor, account, seed) = setup_mldsa::<T>()?;
        Pallet::<T>::register(RawOrigin::Signed(sponsor).into(), descriptor.into())?;
        let implication = alloc::vec![0u8; n.saturating_add(4096) as usize];
        let header = subtensor_hashed::mldsa::Header {
            account: *account.as_ref(),
            version: 2,
            algorithm: 4,
            generation: 0,
            public_key: subtensor_hashed::mldsa::public_key_from_seed(&seed)
                .map_err(BenchmarkError::Stop)?,
            next_commitment: [9; 64],
        };
        let proof = header
            .sign(&seed, &implication, &[0; 32])
            .map_err(BenchmarkError::Stop)?;
        #[block]
        {
            // Include payload materialization, key parsing/expansion, hashing,
            // verification, current-state reads and the preparation state write.
            let payload = implication.clone();
            assert!(proof.verify(&payload));
            let ticket = Pallet::<T>::check_mldsa_authorization(
                &account,
                proof.algorithm,
                &proof.public_key,
                proof.generation,
                proof.next_commitment,
            )
            .unwrap();
            Pallet::<T>::advance_mldsa(&ticket).unwrap();
        }
        assert_eq!(MlDsaAccounts::<T>::get(account).unwrap().generation, 1);
        Ok(())
    }

    #[benchmark]
    fn authorize_mldsa65_invalid(n: Linear<0, 10485760>) -> Result<(), BenchmarkError> {
        let (sponsor, descriptor, account, seed) = setup_mldsa::<T>()?;
        Pallet::<T>::register(RawOrigin::Signed(sponsor).into(), descriptor.into())?;
        let implication = alloc::vec![0u8; n.saturating_add(4096) as usize];
        let header = subtensor_hashed::mldsa::Header {
            account: *account.as_ref(),
            version: 2,
            algorithm: 4,
            generation: 0,
            public_key: subtensor_hashed::mldsa::public_key_from_seed(&seed)
                .map_err(BenchmarkError::Stop)?,
            next_commitment: [9; 64],
        };
        let mut proof = header
            .sign(&seed, &implication, &[0; 32])
            .map_err(BenchmarkError::Stop)?;
        // Change the challenge, preserving the encoded polynomial/hint bounds
        // so rejection requires the actual cryptographic verification work.
        *proof.signature.first_mut().unwrap() ^= 1;
        #[block]
        {
            let payload = implication.clone();
            assert!(
                Pallet::<T>::check_mldsa_authorization(
                    &account,
                    proof.algorithm,
                    &proof.public_key,
                    proof.generation,
                    proof.next_commitment,
                )
                .is_ok()
            );
            assert!(!proof.verify(&payload));
        }
        assert_eq!(MlDsaAccounts::<T>::get(account).unwrap().generation, 0);
        Ok(())
    }

    #[benchmark]
    fn register() -> Result<(), BenchmarkError> {
        let (sponsor, descriptor, account, _) = setup::<T>()?;
        #[extrinsic_call]
        register(
            RawOrigin::Signed(sponsor),
            RegistrationDescriptor::Classical(descriptor),
        );
        assert!(Accounts::<T>::contains_key(account));
        Ok(())
    }

    #[benchmark]
    fn check_registered() -> Result<(), BenchmarkError> {
        let (sponsor, descriptor, account, _) = setup::<T>()?;
        Pallet::<T>::register(RawOrigin::Signed(sponsor.clone()).into(), descriptor.into())?;
        #[extrinsic_call]
        check_registered(
            RawOrigin::Signed(sponsor),
            RegistrationDescriptor::Classical(descriptor),
        );
        assert_eq!(Accounts::<T>::get(account).unwrap().generation, 0);
        Ok(())
    }

    #[benchmark]
    fn authorize(n: Linear<0, 10485760>) -> Result<(), BenchmarkError> {
        let (sponsor, descriptor, account, key) = setup::<T>()?;
        Pallet::<T>::register(RawOrigin::Signed(sponsor).into(), descriptor.into())?;
        // Include room for signed extensions and chain context in the constant
        // term. Runtime supplies call length; implication includes more than it.
        let implication =
            sp_runtime::Vec::from_iter(core::iter::repeat_n(0u8, n.saturating_add(4096) as usize));
        let address = subtensor_hashed::SignerAddress {
            version: 1,
            scheme: descriptor.scheme,
            account: *account.as_ref(),
            generation: 0,
            public_key: key.0,
            next_commitment: [9; 32],
        };
        let payload = address.signing_hash(&implication);
        let signature =
            sp_io::crypto::sr25519_sign(sp_core::crypto::key_types::ACCOUNT, &key, &payload)
                .ok_or(BenchmarkError::Stop("missing benchmark key"))?;
        let proof = Proof {
            generation: 0,
            public_key: key.0,
            next_commitment: [9; 32],
            signature: signature.0,
        };
        #[block]
        {
            // Materialize and domain-bind the full payload and raw signer address.
            let encoded_implication = implication.clone();
            let validated = {
                assert!(sp_io::crypto::sr25519_verify(
                    &sp_core::sr25519::Signature::from_raw(proof.signature),
                    &address.signing_hash(&encoded_implication),
                    &sp_core::sr25519::Public::from_raw(proof.public_key)
                ));
                Pallet::<T>::check_authorization(
                    &account,
                    descriptor.scheme,
                    &proof.public_key,
                    proof.generation,
                    proof.next_commitment,
                )
                .unwrap()
            };
            Pallet::<T>::advance(&validated).unwrap();
        }
        assert_eq!(Accounts::<T>::get(account).unwrap().generation, 1);
        Ok(())
    }

    #[benchmark]
    fn authorize_ed25519(n: Linear<0, 10485760>) -> Result<(), BenchmarkError> {
        let sponsor: AccountId32 = whitelisted_caller();
        let _ = T::Currency::make_free_balance_be(&sponsor, BalanceOf::<T>::max_value());
        let key = sp_io::crypto::ed25519_generate(
            sp_core::crypto::key_types::ACCOUNT,
            Some(b"//HashedEdBenchmark".to_vec()),
        );
        let descriptor = Descriptor {
            version: 1,
            scheme: Scheme::Ed25519,
            initial_commitment: subtensor_hashed::key_commitment(Scheme::Ed25519, &key.0),
        };
        let account = AccountId32::new(subtensor_hashed::account_id(&descriptor));
        T::OnRegister::setup_benchmark(&account, &sponsor, &descriptor)?;
        Pallet::<T>::register(RawOrigin::Signed(sponsor).into(), descriptor.into())?;
        // Include room for signed extensions and chain context in the constant
        // term. Runtime supplies call length; implication includes more than it.
        let implication =
            sp_runtime::Vec::from_iter(core::iter::repeat_n(0u8, n.saturating_add(4096) as usize));
        let address = subtensor_hashed::SignerAddress {
            version: 1,
            scheme: descriptor.scheme,
            account: *account.as_ref(),
            generation: 0,
            public_key: key.0,
            next_commitment: [9; 32],
        };
        let payload = address.signing_hash(&implication);
        let signature =
            sp_io::crypto::ed25519_sign(sp_core::crypto::key_types::ACCOUNT, &key, &payload)
                .ok_or(BenchmarkError::Stop("missing benchmark key"))?;
        let proof = Proof {
            generation: 0,
            public_key: key.0,
            next_commitment: [9; 32],
            signature: signature.0,
        };
        #[block]
        {
            // Materialize and domain-bind the full payload and raw signer address.
            let encoded_implication = implication.clone();
            let validated = {
                assert!(sp_io::crypto::ed25519_verify(
                    &sp_core::ed25519::Signature::from_raw(proof.signature),
                    &address.signing_hash(&encoded_implication),
                    &sp_core::ed25519::Public::from_raw(proof.public_key)
                ));
                Pallet::<T>::check_authorization(
                    &account,
                    descriptor.scheme,
                    &proof.public_key,
                    proof.generation,
                    proof.next_commitment,
                )
                .unwrap()
            };
            Pallet::<T>::advance(&validated).unwrap();
        }
        assert_eq!(Accounts::<T>::get(account).unwrap().generation, 1);
        Ok(())
    }

    impl_benchmark_test_suite!(Pallet, crate::tests::new_test_ext(), crate::tests::Test);
}
