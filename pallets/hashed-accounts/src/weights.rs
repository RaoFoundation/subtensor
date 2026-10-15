//! Reference CI weights (50 steps, 20 repeats).
//! Registration: run 38318619464, commit 5fd041a246253c6387ee2db84a5e7555c5dbad06.
//! Authorization: run 38429545696, commit 3f6afe107706d995a9af0c0949d5b354f062a94f.
//! Authorization measured with compiled Wasm on an AMD EPYC 9V74, 2026-10-13.
use core::marker::PhantomData;
use frame_support::{
    traits::Get,
    weights::{Weight, constants::RocksDbWeight},
};

pub trait WeightInfo {
    fn register() -> Weight;
    fn check_registered() -> Weight;
    fn authorize(n: u32) -> Weight;
    fn authorize_ed25519(n: u32) -> Weight;
}

pub struct SubstrateWeight<T>(PhantomData<T>);

impl<T: frame_system::Config> WeightInfo for SubstrateWeight<T> {
    /// Storage: `AdminUtils::HashedAccountsEnabled` (r:1 w:0)
    /// Proof: `AdminUtils::HashedAccountsEnabled` (`max_values`: Some(1), `max_size`: None, mode: `Measured`)
    /// Storage: `HashedAccounts::Accounts` (r:1 w:1)
    /// Proof: `HashedAccounts::Accounts` (`max_values`: None, `max_size`: Some(122), added: 2597, mode: `MaxEncodedLen`)
    /// The range of component `n` is `[0, 10485760]`.
    fn authorize_ed25519(n: u32) -> Weight {
        // Proof Size summary in bytes:
        //  Measured:  `291`
        //  Estimated: `3587`
        // Minimum execution time: 76_893_000 picoseconds.
        Weight::from_parts(108_873_266, 3587)
            // Standard Error: 0
            .saturating_add(Weight::from_parts(2_556, 0).saturating_mul(n.into()))
            .saturating_add(T::DbWeight::get().reads(2_u64))
            .saturating_add(T::DbWeight::get().writes(1_u64))
    }
    fn check_registered() -> Weight {
        Weight::from_parts(16_291_000, 3587).saturating_add(T::DbWeight::get().reads(2_u64))
    }
    fn register() -> Weight {
        Weight::from_parts(154_470_000, 21449)
            .saturating_add(T::DbWeight::get().reads(15_u64))
            .saturating_add(T::DbWeight::get().writes(7_u64))
    }
    /// Storage: `AdminUtils::HashedAccountsEnabled` (r:1 w:0)
    /// Proof: `AdminUtils::HashedAccountsEnabled` (`max_values`: Some(1), `max_size`: None, mode: `Measured`)
    /// Storage: `HashedAccounts::Accounts` (r:1 w:1)
    /// Proof: `HashedAccounts::Accounts` (`max_values`: None, `max_size`: Some(122), added: 2597, mode: `MaxEncodedLen`)
    /// The range of component `n` is `[0, 10485760]`.
    fn authorize(n: u32) -> Weight {
        // Proof Size summary in bytes:
        //  Measured:  `291`
        //  Estimated: `3587`
        // Minimum execution time: 79_418_000 picoseconds.
        Weight::from_parts(107_693_584, 3587)
            // Standard Error: 0
            .saturating_add(Weight::from_parts(2_555, 0).saturating_mul(n.into()))
            .saturating_add(T::DbWeight::get().reads(2_u64))
            .saturating_add(T::DbWeight::get().writes(1_u64))
    }
}

impl WeightInfo for () {
    /// Storage: `AdminUtils::HashedAccountsEnabled` (r:1 w:0)
    /// Proof: `AdminUtils::HashedAccountsEnabled` (`max_values`: Some(1), `max_size`: None, mode: `Measured`)
    /// Storage: `HashedAccounts::Accounts` (r:1 w:1)
    /// Proof: `HashedAccounts::Accounts` (`max_values`: None, `max_size`: Some(122), added: 2597, mode: `MaxEncodedLen`)
    /// The range of component `n` is `[0, 10485760]`.
    fn authorize_ed25519(n: u32) -> Weight {
        // Proof Size summary in bytes:
        //  Measured:  `291`
        //  Estimated: `3587`
        // Minimum execution time: 76_893_000 picoseconds.
        Weight::from_parts(108_873_266, 3587)
            // Standard Error: 0
            .saturating_add(Weight::from_parts(2_556, 0).saturating_mul(n.into()))
            .saturating_add(RocksDbWeight::get().reads(2_u64))
            .saturating_add(RocksDbWeight::get().writes(1_u64))
    }
    fn check_registered() -> Weight {
        Weight::from_parts(16_291_000, 3587).saturating_add(RocksDbWeight::get().reads(2_u64))
    }
    fn register() -> Weight {
        Weight::from_parts(154_470_000, 21449)
            .saturating_add(RocksDbWeight::get().reads(15_u64))
            .saturating_add(RocksDbWeight::get().writes(7_u64))
    }
    /// Storage: `AdminUtils::HashedAccountsEnabled` (r:1 w:0)
    /// Proof: `AdminUtils::HashedAccountsEnabled` (`max_values`: Some(1), `max_size`: None, mode: `Measured`)
    /// Storage: `HashedAccounts::Accounts` (r:1 w:1)
    /// Proof: `HashedAccounts::Accounts` (`max_values`: None, `max_size`: Some(122), added: 2597, mode: `MaxEncodedLen`)
    /// The range of component `n` is `[0, 10485760]`.
    fn authorize(n: u32) -> Weight {
        // Proof Size summary in bytes:
        //  Measured:  `291`
        //  Estimated: `3587`
        // Minimum execution time: 79_418_000 picoseconds.
        Weight::from_parts(107_693_584, 3587)
            // Standard Error: 0
            .saturating_add(Weight::from_parts(2_555, 0).saturating_mul(n.into()))
            .saturating_add(RocksDbWeight::get().reads(2_u64))
            .saturating_add(RocksDbWeight::get().writes(1_u64))
    }
}
