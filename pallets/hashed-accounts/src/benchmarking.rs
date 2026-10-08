//! Reference benchmark definitions. No locally measured weights are committed.
#![allow(clippy::unwrap_used)]

use super::*;
use frame_benchmarking::v2::*;
use frame_support::traits::Get;
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
    if !T::Enabled::get() {
        return Err(BenchmarkError::Skip);
    }
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

#[benchmarks]
mod benchmarks {
    use super::*;

    #[benchmark]
    fn register() -> Result<(), BenchmarkError> {
        let (sponsor, descriptor, account, _) = setup::<T>()?;
        #[extrinsic_call]
        _(RawOrigin::Signed(sponsor), descriptor);
        assert!(Accounts::<T>::contains_key(account));
        Ok(())
    }

    #[benchmark]
    fn authorize(n: Linear<0, 10485760>) -> Result<(), BenchmarkError> {
        let (sponsor, descriptor, account, key) = setup::<T>()?;
        Pallet::<T>::register(RawOrigin::Signed(sponsor).into(), descriptor)?;
        // Include room for signed extensions and chain context in the constant
        // term. Runtime supplies call length; implication includes more than it.
        let implication =
            sp_runtime::Vec::from_iter(core::iter::repeat_n(0u8, n.saturating_add(4096) as usize));
        let payload = subtensor_hashed::transaction_payload(
            account.as_ref(),
            descriptor.scheme,
            0,
            &[9; 32],
            &implication,
        );
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
            // The runtime materializes implication.encode() before verification.
            let encoded_implication = implication.clone();
            let validated =
                Pallet::<T>::check_proof(&account, &proof, &encoded_implication).unwrap();
            Pallet::<T>::advance(&validated).unwrap();
        }
        assert_eq!(Accounts::<T>::get(account).unwrap().generation, 1);
        Ok(())
    }

    impl_benchmark_test_suite!(Pallet, crate::tests::new_test_ext(), crate::tests::Test);
}
