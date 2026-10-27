//! Reference measurements include the subsequently removed ML-DSA subgate reads.
//! Their cost remains conservatively charged; the AdminUtils switch is the only gate.
//! ML-DSA additions: reference run 39060946210, commit 437053f80d5ebebce950602b168901bfe6591483.
//! Measured 2026-10-17, compiled Wasm, AMD EPYC 9V45, 50 steps, 20 repeats.
//! Reference CI weights (50 steps, 20 repeats).
//! Registration: run 38318619464, commit 5fd041a246253c6387ee2db84a5e7555c5dbad06.
//! Authorization: run 38429545696, commit 3f6afe107706d995a9af0c0949d5b354f062a94f.
//! Authorization measured with compiled Wasm on an AMD EPYC 9V74, 2026-10-13.
//! Falcon: reference run 39343562831, commit 6efef86d21a8eea84963373882afdd8747742f68.
//! Measured 2026-10-20 UTC, compiled Wasm, AMD EPYC 9V45, 50 steps, 20 repeats.
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
    fn register_mldsa65() -> Weight;
    fn register_falcon() -> Weight;
    fn check_registered_falcon() -> Weight;
    fn check_registered_mldsa65() -> Weight;
    fn authorize_mldsa65(n: u32) -> Weight;
    fn authorize_mldsa65_invalid(n: u32) -> Weight;
    // Valid and invalid Falcon proofs are measured independently; authorization
    // charges their component-wise maximum.
    fn authorize_falcon512(n: u32) -> Weight;
    fn authorize_falcon512_invalid(n: u32) -> Weight;
    fn authorize_falcon1024(n: u32) -> Weight;
    fn authorize_falcon1024_invalid(n: u32) -> Weight;
    fn authorize_fixed_falcon512(n: u32) -> Weight;
    fn authorize_fixed_falcon512_invalid(n: u32) -> Weight;
    fn authorize_fixed_falcon1024(n: u32) -> Weight;
    fn authorize_fixed_falcon1024_invalid(n: u32) -> Weight;
    fn register_derived() -> Weight;
    fn register_derived_sponsored() -> Weight;
    fn check_derived() -> Weight;

    fn install_capability() -> Weight;
    fn revoke_capability() -> Weight;
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

    /// Storage: `AdminUtils::HashedAccountsEnabled` (r:1 w:0)
    /// Proof: `AdminUtils::HashedAccountsEnabled` (`max_values`: Some(1), `max_size`: None, mode: `Measured`)
    /// Storage: `HashedAccounts::MlDsaAuthorizationEnabled` (r:1 w:0)
    /// Proof: `HashedAccounts::MlDsaAuthorizationEnabled` (`max_values`: Some(1), `max_size`: Some(1), added: 496, mode: `MaxEncodedLen`)
    /// Storage: `HashedAccounts::Accounts` (r:1 w:0)
    /// Proof: `HashedAccounts::Accounts` (`max_values`: None, `max_size`: Some(122), added: 2597, mode: `MaxEncodedLen`)
    /// Storage: `HashedAccounts::MlDsaAccounts` (r:1 w:0)
    /// Proof: `HashedAccounts::MlDsaAccounts` (`max_values`: None, `max_size`: Some(186), added: 2661, mode: `MaxEncodedLen`)
    /// Storage: `HashedAccounts::CapabilityEpochs` (r:1 w:1)
    /// Proof: `HashedAccounts::CapabilityEpochs` (`max_values`: None, `max_size`: Some(65), added: 2540, mode: `MaxEncodedLen`)
    /// Storage: `HashedAccounts::ActiveCapabilities` (r:0 w:1)
    /// Proof: `HashedAccounts::ActiveCapabilities` (`max_values`: None, `max_size`: Some(300), added: 2775, mode: `MaxEncodedLen`)
    fn install_capability() -> Weight {
        // Proof Size summary in bytes:
        //  Measured:  `873`
        //  Estimated: `3651`
        // Minimum execution time: 29_505_000 picoseconds.
        Weight::from_parts(34_592_000, 3651)
            .saturating_add(T::DbWeight::get().reads(5_u64))
            .saturating_add(T::DbWeight::get().writes(2_u64))
    }
    /// Storage: `HashedAccounts::Accounts` (r:1 w:0)
    /// Proof: `HashedAccounts::Accounts` (`max_values`: None, `max_size`: Some(122), added: 2597, mode: `MaxEncodedLen`)
    /// Storage: `HashedAccounts::MlDsaAccounts` (r:1 w:0)
    /// Proof: `HashedAccounts::MlDsaAccounts` (`max_values`: None, `max_size`: Some(186), added: 2661, mode: `MaxEncodedLen`)
    /// Storage: `HashedAccounts::CapabilityEpochs` (r:1 w:1)
    /// Proof: `HashedAccounts::CapabilityEpochs` (`max_values`: None, `max_size`: Some(65), added: 2540, mode: `MaxEncodedLen`)
    /// Storage: `HashedAccounts::ActiveCapabilities` (r:0 w:1)
    /// Proof: `HashedAccounts::ActiveCapabilities` (`max_values`: None, `max_size`: Some(300), added: 2775, mode: `MaxEncodedLen`)
    fn revoke_capability() -> Weight {
        // Proof Size summary in bytes:
        //  Measured:  `415`
        //  Estimated: `3651`
        // Minimum execution time: 21_362_000 picoseconds.
        Weight::from_parts(22_364_000, 3651)
            .saturating_add(T::DbWeight::get().reads(3_u64))
            .saturating_add(T::DbWeight::get().writes(2_u64))
    }
    /// Storage: `AdminUtils::HashedAccountsEnabled` (r:1 w:0)
    /// Proof: `AdminUtils::HashedAccountsEnabled` (`max_values`: Some(1), `max_size`: None, mode: `Measured`)
    /// Storage: `HashedAccounts::MlDsaAccounts` (r:22 w:1)
    /// Proof: `HashedAccounts::MlDsaAccounts` (`max_values`: None, `max_size`: Some(186), added: 2661, mode: `MaxEncodedLen`)
    /// Storage: `HashedAccounts::Accounts` (r:22 w:0)
    /// Proof: `HashedAccounts::Accounts` (`max_values`: None, `max_size`: Some(122), added: 2597, mode: `MaxEncodedLen`)
    /// Storage: `HashedAccounts::DerivedAuthorities` (r:22 w:0)
    /// Proof: `HashedAccounts::DerivedAuthorities` (`max_values`: None, `max_size`: Some(6460), added: 8935, mode: `MaxEncodedLen`)
    /// Storage: `HashedAccounts::EvmAliases` (r:1 w:1)
    /// Proof: `HashedAccounts::EvmAliases` (`max_values`: None, `max_size`: Some(68), added: 2543, mode: `MaxEncodedLen`)
    /// Storage: `SubtensorModule::Owner` (r:1 w:0)
    /// Proof: `SubtensorModule::Owner` (`max_values`: None, `max_size`: None, mode: `Measured`)
    /// Storage: `Proxy::Proxies` (r:1 w:0)
    /// Proof: `Proxy::Proxies` (`max_values`: None, `max_size`: Some(789), added: 3264, mode: `MaxEncodedLen`)
    /// Storage: `SubtensorModule::ColdkeySwapAnnouncements` (r:1 w:0)
    /// Proof: `SubtensorModule::ColdkeySwapAnnouncements` (`max_values`: None, `max_size`: None, mode: `Measured`)
    /// Storage: `SubtensorModule::ColdkeySwapDisputes` (r:1 w:0)
    /// Proof: `SubtensorModule::ColdkeySwapDisputes` (`max_values`: None, `max_size`: None, mode: `Measured`)
    /// Storage: `Multisig::Multisigs` (r:1 w:0)
    /// Proof: `Multisig::Multisigs` (`max_values`: None, `max_size`: Some(3338), added: 5813, mode: `MaxEncodedLen`)
    /// Storage: `Proxy::Announcements` (r:20 w:0)
    /// Proof: `Proxy::Announcements` (`max_values`: None, `max_size`: Some(5150), added: 7625, mode: `MaxEncodedLen`)
    /// Storage: `System::Account` (r:2 w:2)
    /// Proof: `System::Account` (`max_values`: None, `max_size`: Some(104), added: 2579, mode: `MaxEncodedLen`)
    /// Storage: `Balances::Locks` (r:1 w:0)
    /// Proof: `Balances::Locks` (`max_values`: None, `max_size`: Some(899), added: 3374, mode: `MaxEncodedLen`)
    /// Storage: `Balances::Reserves` (r:1 w:0)
    /// Proof: `Balances::Reserves` (`max_values`: None, `max_size`: Some(849), added: 3324, mode: `MaxEncodedLen`)
    /// Storage: `Balances::Holds` (r:1 w:0)
    /// Proof: `Balances::Holds` (`max_values`: None, `max_size`: Some(89), added: 2564, mode: `MaxEncodedLen`)
    /// Storage: `Balances::Freezes` (r:1 w:0)
    /// Proof: `Balances::Freezes` (`max_values`: None, `max_size`: Some(499), added: 2974, mode: `MaxEncodedLen`)
    /// Storage: `EVM::AccountCodes` (r:1 w:0)
    /// Proof: `EVM::AccountCodes` (`max_values`: None, `max_size`: None, mode: `Measured`)
    /// Storage: `EVM::AccountCodesMetadata` (r:1 w:0)
    /// Proof: `EVM::AccountCodesMetadata` (`max_values`: None, `max_size`: None, mode: `Measured`)
    /// Storage: `EVM::AccountStorages` (r:1 w:0)
    /// Proof: `EVM::AccountStorages` (`max_values`: None, `max_size`: None, mode: `Measured`)
    fn register_falcon() -> Weight {
        // Proof Size summary in bytes:
        //  Measured:  `116616`
        //  Estimated: `197560`
        // Minimum execution time: 427_118_000 picoseconds.
        Weight::from_parts(450_464_000, 197560)
            .saturating_add(T::DbWeight::get().reads(102_u64))
            .saturating_add(T::DbWeight::get().writes(4_u64))
    }
    /// Storage: `AdminUtils::HashedAccountsEnabled` (r:1 w:0)
    /// Proof: `AdminUtils::HashedAccountsEnabled` (`max_values`: Some(1), `max_size`: None, mode: `Measured`)
    /// Storage: `HashedAccounts::MlDsaAccounts` (r:1 w:0)
    /// Proof: `HashedAccounts::MlDsaAccounts` (`max_values`: None, `max_size`: Some(186), added: 2661, mode: `MaxEncodedLen`)
    fn check_registered_falcon() -> Weight {
        // Proof Size summary in bytes:
        //  Measured:  `425`
        //  Estimated: `3651`
        // Minimum execution time: 20_481_000 picoseconds.
        Weight::from_parts(25_588_000, 3651).saturating_add(T::DbWeight::get().reads(2_u64))
    }
    /// Storage: `AdminUtils::HashedAccountsEnabled` (r:1 w:0)
    /// Proof: `AdminUtils::HashedAccountsEnabled` (`max_values`: Some(1), `max_size`: None, mode: `Measured`)
    /// Storage: `HashedAccounts::MlDsaAccounts` (r:22 w:1)
    /// Proof: `HashedAccounts::MlDsaAccounts` (`max_values`: None, `max_size`: Some(186), added: 2661, mode: `MaxEncodedLen`)
    /// Storage: `HashedAccounts::MlDsaRegistrationEnabled` (r:1 w:0)
    /// Proof: `HashedAccounts::MlDsaRegistrationEnabled` (`max_values`: Some(1), `max_size`: Some(1), added: 496, mode: `MaxEncodedLen`)
    /// Storage: `HashedAccounts::Accounts` (r:22 w:0)
    /// Proof: `HashedAccounts::Accounts` (`max_values`: None, `max_size`: Some(122), added: 2597, mode: `MaxEncodedLen`)
    /// Storage: `HashedAccounts::DerivedAuthorities` (r:22 w:0)
    /// Proof: `HashedAccounts::DerivedAuthorities` (`max_values`: None, `max_size`: Some(6460), added: 8935, mode: `MaxEncodedLen`)
    /// Storage: `HashedAccounts::EvmAliases` (r:1 w:1)
    /// Proof: `HashedAccounts::EvmAliases` (`max_values`: None, `max_size`: Some(68), added: 2543, mode: `MaxEncodedLen`)
    /// Storage: `SubtensorModule::Owner` (r:1 w:0)
    /// Proof: `SubtensorModule::Owner` (`max_values`: None, `max_size`: None, mode: `Measured`)
    /// Storage: `Proxy::Proxies` (r:1 w:0)
    /// Proof: `Proxy::Proxies` (`max_values`: None, `max_size`: Some(789), added: 3264, mode: `MaxEncodedLen`)
    /// Storage: `SubtensorModule::ColdkeySwapAnnouncements` (r:1 w:0)
    /// Proof: `SubtensorModule::ColdkeySwapAnnouncements` (`max_values`: None, `max_size`: None, mode: `Measured`)
    /// Storage: `SubtensorModule::ColdkeySwapDisputes` (r:1 w:0)
    /// Proof: `SubtensorModule::ColdkeySwapDisputes` (`max_values`: None, `max_size`: None, mode: `Measured`)
    /// Storage: `Multisig::Multisigs` (r:1 w:0)
    /// Proof: `Multisig::Multisigs` (`max_values`: None, `max_size`: Some(3338), added: 5813, mode: `MaxEncodedLen`)
    /// Storage: `Proxy::Announcements` (r:20 w:0)
    /// Proof: `Proxy::Announcements` (`max_values`: None, `max_size`: Some(5150), added: 7625, mode: `MaxEncodedLen`)
    /// Storage: `System::Account` (r:2 w:2)
    /// Proof: `System::Account` (`max_values`: None, `max_size`: Some(104), added: 2579, mode: `MaxEncodedLen`)
    /// Storage: `Balances::Locks` (r:1 w:0)
    /// Proof: `Balances::Locks` (`max_values`: None, `max_size`: Some(899), added: 3374, mode: `MaxEncodedLen`)
    /// Storage: `Balances::Reserves` (r:1 w:0)
    /// Proof: `Balances::Reserves` (`max_values`: None, `max_size`: Some(849), added: 3324, mode: `MaxEncodedLen`)
    /// Storage: `Balances::Holds` (r:1 w:0)
    /// Proof: `Balances::Holds` (`max_values`: None, `max_size`: Some(89), added: 2564, mode: `MaxEncodedLen`)
    /// Storage: `Balances::Freezes` (r:1 w:0)
    /// Proof: `Balances::Freezes` (`max_values`: None, `max_size`: Some(499), added: 2974, mode: `MaxEncodedLen`)
    /// Storage: `EVM::AccountCodes` (r:1 w:0)
    /// Proof: `EVM::AccountCodes` (`max_values`: None, `max_size`: None, mode: `Measured`)
    /// Storage: `EVM::AccountCodesMetadata` (r:1 w:0)
    /// Proof: `EVM::AccountCodesMetadata` (`max_values`: None, `max_size`: None, mode: `Measured`)
    /// Storage: `EVM::AccountStorages` (r:1 w:0)
    /// Proof: `EVM::AccountStorages` (`max_values`: None, `max_size`: None, mode: `Measured`)
    fn register_mldsa65() -> Weight {
        // Proof Size summary in bytes:
        //  Measured:  `115690`
        //  Estimated: `197560`
        // Minimum execution time: 339_453_000 picoseconds.
        Weight::from_parts(360_816_000, 197560)
            .saturating_add(T::DbWeight::get().reads(103_u64))
            .saturating_add(T::DbWeight::get().writes(4_u64))
    }
    /// Storage: `AdminUtils::HashedAccountsEnabled` (r:1 w:0)
    /// Proof: `AdminUtils::HashedAccountsEnabled` (`max_values`: Some(1), `max_size`: None, mode: `Measured`)
    /// Storage: `HashedAccounts::MlDsaAccounts` (r:1 w:0)
    /// Proof: `HashedAccounts::MlDsaAccounts` (`max_values`: None, `max_size`: Some(186), added: 2661, mode: `MaxEncodedLen`)
    fn check_registered_mldsa65() -> Weight {
        // Proof Size summary in bytes:
        //  Measured:  `465`
        //  Estimated: `3651`
        // Minimum execution time: 13_500_000 picoseconds.
        Weight::from_parts(15_153_000, 3651).saturating_add(T::DbWeight::get().reads(2_u64))
    }
    /// Storage: `AdminUtils::HashedAccountsEnabled` (r:1 w:0)
    /// Proof: `AdminUtils::HashedAccountsEnabled` (`max_values`: Some(1), `max_size`: None, mode: `Measured`)
    /// Storage: `HashedAccounts::DerivedAuthorities` (r:946 w:1)
    /// Proof: `HashedAccounts::DerivedAuthorities` (`max_values`: None, `max_size`: Some(6460), added: 8935, mode: `MaxEncodedLen`)
    /// Storage: `HashedAccounts::MlDsaRegistrationEnabled` (r:1 w:0)
    /// Proof: `HashedAccounts::MlDsaRegistrationEnabled` (`max_values`: Some(1), `max_size`: Some(1), added: 496, mode: `MaxEncodedLen`)
    /// Storage: `HashedAccounts::Accounts` (r:22 w:0)
    /// Proof: `HashedAccounts::Accounts` (`max_values`: None, `max_size`: Some(122), added: 2597, mode: `MaxEncodedLen`)
    /// Storage: `HashedAccounts::MlDsaAccounts` (r:122 w:0)
    /// Proof: `HashedAccounts::MlDsaAccounts` (`max_values`: None, `max_size`: Some(186), added: 2661, mode: `MaxEncodedLen`)
    /// Storage: `HashedAccounts::EvmAliases` (r:1 w:1)
    /// Proof: `HashedAccounts::EvmAliases` (`max_values`: None, `max_size`: Some(68), added: 2543, mode: `MaxEncodedLen`)
    /// Storage: `SubtensorModule::Owner` (r:1 w:0)
    /// Proof: `SubtensorModule::Owner` (`max_values`: None, `max_size`: None, mode: `Measured`)
    /// Storage: `Proxy::Proxies` (r:1 w:0)
    /// Proof: `Proxy::Proxies` (`max_values`: None, `max_size`: Some(789), added: 3264, mode: `MaxEncodedLen`)
    /// Storage: `SubtensorModule::ColdkeySwapAnnouncements` (r:1 w:0)
    /// Proof: `SubtensorModule::ColdkeySwapAnnouncements` (`max_values`: None, `max_size`: None, mode: `Measured`)
    /// Storage: `SubtensorModule::ColdkeySwapDisputes` (r:1 w:0)
    /// Proof: `SubtensorModule::ColdkeySwapDisputes` (`max_values`: None, `max_size`: None, mode: `Measured`)
    /// Storage: `Multisig::Multisigs` (r:1 w:0)
    /// Proof: `Multisig::Multisigs` (`max_values`: None, `max_size`: Some(3338), added: 5813, mode: `MaxEncodedLen`)
    /// Storage: `Proxy::Announcements` (r:20 w:0)
    /// Proof: `Proxy::Announcements` (`max_values`: None, `max_size`: Some(5150), added: 7625, mode: `MaxEncodedLen`)
    /// Storage: `System::Account` (r:2 w:2)
    /// Proof: `System::Account` (`max_values`: None, `max_size`: Some(104), added: 2579, mode: `MaxEncodedLen`)
    /// Storage: `Balances::Locks` (r:1 w:0)
    /// Proof: `Balances::Locks` (`max_values`: None, `max_size`: Some(899), added: 3374, mode: `MaxEncodedLen`)
    /// Storage: `Balances::Reserves` (r:1 w:0)
    /// Proof: `Balances::Reserves` (`max_values`: None, `max_size`: Some(849), added: 3324, mode: `MaxEncodedLen`)
    /// Storage: `Balances::Holds` (r:1 w:0)
    /// Proof: `Balances::Holds` (`max_values`: None, `max_size`: Some(89), added: 2564, mode: `MaxEncodedLen`)
    /// Storage: `Balances::Freezes` (r:1 w:0)
    /// Proof: `Balances::Freezes` (`max_values`: None, `max_size`: Some(499), added: 2974, mode: `MaxEncodedLen`)
    /// Storage: `EVM::AccountCodes` (r:1 w:0)
    /// Proof: `EVM::AccountCodes` (`max_values`: None, `max_size`: None, mode: `Measured`)
    /// Storage: `EVM::AccountCodesMetadata` (r:1 w:0)
    /// Proof: `EVM::AccountCodesMetadata` (`max_values`: None, `max_size`: None, mode: `Measured`)
    /// Storage: `EVM::AccountStorages` (r:1 w:0)
    /// Proof: `EVM::AccountStorages` (`max_values`: None, `max_size`: None, mode: `Measured`)
    fn register_derived() -> Weight {
        // Proof Size summary in bytes:
        //  Measured:  `281775`
        //  Estimated: `8453500`
        // Minimum execution time: 4_101_194_000 picoseconds.
        Weight::from_parts(4_313_926_000, 8453500)
            .saturating_add(T::DbWeight::get().reads(1127_u64))
            .saturating_add(T::DbWeight::get().writes(4_u64))
    }
    /// Storage: `AdminUtils::HashedAccountsEnabled` (r:1 w:0)
    /// Proof: `AdminUtils::HashedAccountsEnabled` (`max_values`: Some(1), `max_size`: None, mode: `Measured`)
    /// Storage: `HashedAccounts::DerivedAuthorities` (r:946 w:1)
    /// Proof: `HashedAccounts::DerivedAuthorities` (`max_values`: None, `max_size`: Some(6460), added: 8935, mode: `MaxEncodedLen`)
    /// Storage: `HashedAccounts::MlDsaRegistrationEnabled` (r:1 w:0)
    /// Proof: `HashedAccounts::MlDsaRegistrationEnabled` (`max_values`: Some(1), `max_size`: Some(1), added: 496, mode: `MaxEncodedLen`)
    /// Storage: `HashedAccounts::Accounts` (r:22 w:0)
    /// Proof: `HashedAccounts::Accounts` (`max_values`: None, `max_size`: Some(122), added: 2597, mode: `MaxEncodedLen`)
    /// Storage: `HashedAccounts::MlDsaAccounts` (r:122 w:0)
    /// Proof: `HashedAccounts::MlDsaAccounts` (`max_values`: None, `max_size`: Some(186), added: 2661, mode: `MaxEncodedLen`)
    /// Storage: `HashedAccounts::EvmAliases` (r:1 w:1)
    /// Proof: `HashedAccounts::EvmAliases` (`max_values`: None, `max_size`: Some(68), added: 2543, mode: `MaxEncodedLen`)
    /// Storage: `System::ExtrinsicData` (r:1 w:0)
    /// Proof: `System::ExtrinsicData` (`max_values`: None, `max_size`: None, mode: `Measured`)
    /// Storage: `SubtensorModule::Owner` (r:1 w:0)
    /// Proof: `SubtensorModule::Owner` (`max_values`: None, `max_size`: None, mode: `Measured`)
    /// Storage: `Proxy::Proxies` (r:1 w:0)
    /// Proof: `Proxy::Proxies` (`max_values`: None, `max_size`: Some(789), added: 3264, mode: `MaxEncodedLen`)
    /// Storage: `SubtensorModule::ColdkeySwapAnnouncements` (r:1 w:0)
    /// Proof: `SubtensorModule::ColdkeySwapAnnouncements` (`max_values`: None, `max_size`: None, mode: `Measured`)
    /// Storage: `SubtensorModule::ColdkeySwapDisputes` (r:1 w:0)
    /// Proof: `SubtensorModule::ColdkeySwapDisputes` (`max_values`: None, `max_size`: None, mode: `Measured`)
    /// Storage: `Multisig::Multisigs` (r:1 w:0)
    /// Proof: `Multisig::Multisigs` (`max_values`: None, `max_size`: Some(3338), added: 5813, mode: `MaxEncodedLen`)
    /// Storage: `Proxy::Announcements` (r:20 w:0)
    /// Proof: `Proxy::Announcements` (`max_values`: None, `max_size`: Some(5150), added: 7625, mode: `MaxEncodedLen`)
    /// Storage: `System::Account` (r:2 w:2)
    /// Proof: `System::Account` (`max_values`: None, `max_size`: Some(104), added: 2579, mode: `MaxEncodedLen`)
    /// Storage: `Balances::Locks` (r:1 w:0)
    /// Proof: `Balances::Locks` (`max_values`: None, `max_size`: Some(899), added: 3374, mode: `MaxEncodedLen`)
    /// Storage: `Balances::Reserves` (r:1 w:0)
    /// Proof: `Balances::Reserves` (`max_values`: None, `max_size`: Some(849), added: 3324, mode: `MaxEncodedLen`)
    /// Storage: `Balances::Holds` (r:1 w:0)
    /// Proof: `Balances::Holds` (`max_values`: None, `max_size`: Some(89), added: 2564, mode: `MaxEncodedLen`)
    /// Storage: `Balances::Freezes` (r:1 w:0)
    /// Proof: `Balances::Freezes` (`max_values`: None, `max_size`: Some(499), added: 2974, mode: `MaxEncodedLen`)
    /// Storage: `EVM::AccountCodes` (r:1 w:0)
    /// Proof: `EVM::AccountCodes` (`max_values`: None, `max_size`: None, mode: `Measured`)
    /// Storage: `EVM::AccountCodesMetadata` (r:1 w:0)
    /// Proof: `EVM::AccountCodesMetadata` (`max_values`: None, `max_size`: None, mode: `Measured`)
    /// Storage: `EVM::AccountStorages` (r:1 w:0)
    /// Proof: `EVM::AccountStorages` (`max_values`: None, `max_size`: None, mode: `Measured`)
    fn register_derived_sponsored() -> Weight {
        // Proof Size summary in bytes:
        //  Measured:  `347319`
        //  Estimated: `8453500`
        // Minimum execution time: 5_234_469_000 picoseconds.
        Weight::from_parts(5_530_897_000, 8453500)
            .saturating_add(T::DbWeight::get().reads(1128_u64))
            .saturating_add(T::DbWeight::get().writes(4_u64))
    }
    /// Storage: `AdminUtils::HashedAccountsEnabled` (r:1 w:0)
    /// Proof: `AdminUtils::HashedAccountsEnabled` (`max_values`: Some(1), `max_size`: None, mode: `Measured`)
    /// Storage: `HashedAccounts::DerivedAuthorities` (r:1 w:0)
    /// Proof: `HashedAccounts::DerivedAuthorities` (`max_values`: None, `max_size`: Some(6460), added: 8935, mode: `MaxEncodedLen`)
    fn check_derived() -> Weight {
        // Proof Size summary in bytes:
        //  Measured:  `7972`
        //  Estimated: `9925`
        // Minimum execution time: 24_217_000 picoseconds.
        Weight::from_parts(28_023_000, 9925).saturating_add(T::DbWeight::get().reads(2_u64))
    }
    /// Storage: `AdminUtils::HashedAccountsEnabled` (r:1 w:0)
    /// Proof: `AdminUtils::HashedAccountsEnabled` (`max_values`: Some(1), `max_size`: None, mode: `Measured`)
    /// Storage: `HashedAccounts::MlDsaAuthorizationEnabled` (r:1 w:0)
    /// Proof: `HashedAccounts::MlDsaAuthorizationEnabled` (`max_values`: Some(1), `max_size`: Some(1), added: 496, mode: `MaxEncodedLen`)
    /// Storage: `HashedAccounts::MlDsaAccounts` (r:1 w:1)
    /// Proof: `HashedAccounts::MlDsaAccounts` (`max_values`: None, `max_size`: Some(186), added: 2661, mode: `MaxEncodedLen`)
    /// The range of component `n` is `[0, 10485760]`.
    fn authorize_mldsa65(n: u32) -> Weight {
        // Proof Size summary in bytes:
        //  Measured:  `465`
        //  Estimated: `3651`
        // Minimum execution time: 224_590_000 picoseconds.
        Weight::from_parts(185_991_410, 3651)
            // Standard Error: 4
            .saturating_add(Weight::from_parts(2_246, 0).saturating_mul(n.into()))
            .saturating_add(T::DbWeight::get().reads(3_u64))
            .saturating_add(T::DbWeight::get().writes(1_u64))
    }
    /// Storage: `AdminUtils::HashedAccountsEnabled` (r:1 w:0)
    /// Proof: `AdminUtils::HashedAccountsEnabled` (`max_values`: Some(1), `max_size`: None, mode: `Measured`)
    /// Storage: `HashedAccounts::MlDsaAuthorizationEnabled` (r:1 w:0)
    /// Proof: `HashedAccounts::MlDsaAuthorizationEnabled` (`max_values`: Some(1), `max_size`: Some(1), added: 496, mode: `MaxEncodedLen`)
    /// Storage: `HashedAccounts::MlDsaAccounts` (r:1 w:0)
    /// Proof: `HashedAccounts::MlDsaAccounts` (`max_values`: None, `max_size`: Some(186), added: 2661, mode: `MaxEncodedLen`)
    /// The range of component `n` is `[0, 10485760]`.
    fn authorize_mldsa65_invalid(n: u32) -> Weight {
        // Proof Size summary in bytes:
        //  Measured:  `465`
        //  Estimated: `3651`
        // Minimum execution time: 237_099_000 picoseconds.
        Weight::from_parts(349_427_694, 3651)
            // Standard Error: 1
            .saturating_add(Weight::from_parts(2_271, 0).saturating_mul(n.into()))
            .saturating_add(T::DbWeight::get().reads(3_u64))
    }
    /// Storage: `AdminUtils::HashedAccountsEnabled` (r:1 w:0)
    /// Proof: `AdminUtils::HashedAccountsEnabled` (`max_values`: Some(1), `max_size`: None, mode: `Measured`)
    /// Storage: `HashedAccounts::MlDsaAccounts` (r:1 w:1)
    /// Proof: `HashedAccounts::MlDsaAccounts` (`max_values`: None, `max_size`: Some(186), added: 2661, mode: `MaxEncodedLen`)
    /// The range of component `n` is `[0, 10485760]`.
    fn authorize_falcon512(n: u32) -> Weight {
        // Proof Size summary in bytes:
        //  Measured:  `425`
        //  Estimated: `3651`
        // Minimum execution time: 78_208_000 picoseconds.
        Weight::from_parts(206_650_078, 3651)
            // Standard Error: 2
            .saturating_add(Weight::from_parts(2_260, 0).saturating_mul(n.into()))
            .saturating_add(T::DbWeight::get().reads(2_u64))
            .saturating_add(T::DbWeight::get().writes(1_u64))
    }
    /// Storage: `AdminUtils::HashedAccountsEnabled` (r:1 w:0)
    /// Proof: `AdminUtils::HashedAccountsEnabled` (`max_values`: Some(1), `max_size`: None, mode: `Measured`)
    /// Storage: `HashedAccounts::MlDsaAccounts` (r:1 w:0)
    /// Proof: `HashedAccounts::MlDsaAccounts` (`max_values`: None, `max_size`: Some(186), added: 2661, mode: `MaxEncodedLen`)
    /// The range of component `n` is `[0, 10485760]`.
    fn authorize_falcon512_invalid(n: u32) -> Weight {
        // Proof Size summary in bytes:
        //  Measured:  `425`
        //  Estimated: `3651`
        // Minimum execution time: 64_967_000 picoseconds.
        Weight::from_parts(70_732_205, 3651)
            // Standard Error: 3
            .saturating_add(Weight::from_parts(2_274, 0).saturating_mul(n.into()))
            .saturating_add(T::DbWeight::get().reads(2_u64))
    }
    /// Storage: `AdminUtils::HashedAccountsEnabled` (r:1 w:0)
    /// Proof: `AdminUtils::HashedAccountsEnabled` (`max_values`: Some(1), `max_size`: None, mode: `Measured`)
    /// Storage: `HashedAccounts::MlDsaAccounts` (r:1 w:1)
    /// Proof: `HashedAccounts::MlDsaAccounts` (`max_values`: None, `max_size`: Some(186), added: 2661, mode: `MaxEncodedLen`)
    /// The range of component `n` is `[0, 10485760]`.
    fn authorize_falcon1024(n: u32) -> Weight {
        // Proof Size summary in bytes:
        //  Measured:  `425`
        //  Estimated: `3651`
        // Minimum execution time: 123_475_000 picoseconds.
        Weight::from_parts(215_567_566, 3651)
            // Standard Error: 3
            .saturating_add(Weight::from_parts(2_280, 0).saturating_mul(n.into()))
            .saturating_add(T::DbWeight::get().reads(2_u64))
            .saturating_add(T::DbWeight::get().writes(1_u64))
    }
    /// Storage: `AdminUtils::HashedAccountsEnabled` (r:1 w:0)
    /// Proof: `AdminUtils::HashedAccountsEnabled` (`max_values`: Some(1), `max_size`: None, mode: `Measured`)
    /// Storage: `HashedAccounts::MlDsaAccounts` (r:1 w:0)
    /// Proof: `HashedAccounts::MlDsaAccounts` (`max_values`: None, `max_size`: Some(186), added: 2661, mode: `MaxEncodedLen`)
    /// The range of component `n` is `[0, 10485760]`.
    fn authorize_falcon1024_invalid(n: u32) -> Weight {
        // Proof Size summary in bytes:
        //  Measured:  `425`
        //  Estimated: `3651`
        // Minimum execution time: 109_874_000 picoseconds.
        Weight::from_parts(93_923_526, 3651)
            // Standard Error: 2
            .saturating_add(Weight::from_parts(2_288, 0).saturating_mul(n.into()))
            .saturating_add(T::DbWeight::get().reads(2_u64))
    }
    /// Storage: `AdminUtils::HashedAccountsEnabled` (r:1 w:0)
    /// Proof: `AdminUtils::HashedAccountsEnabled` (`max_values`: Some(1), `max_size`: None, mode: `Measured`)
    /// Storage: `System::Account` (r:1 w:1)
    /// Proof: `System::Account` (`max_values`: None, `max_size`: Some(104), added: 2579, mode: `MaxEncodedLen`)
    /// The range of component `n` is `[0, 10485760]`.
    fn authorize_fixed_falcon512(n: u32) -> Weight {
        // Proof Size summary in bytes:
        //  Measured:  `247`
        //  Estimated: `3569`
        // Minimum execution time: 100_149_000 picoseconds.
        Weight::from_parts(64_919_788, 3569)
            // Standard Error: 1
            .saturating_add(Weight::from_parts(2_297, 0).saturating_mul(n.into()))
            .saturating_add(T::DbWeight::get().reads(2_u64))
            .saturating_add(T::DbWeight::get().writes(1_u64))
    }
    /// Storage: `AdminUtils::HashedAccountsEnabled` (r:1 w:0)
    /// Proof: `AdminUtils::HashedAccountsEnabled` (`max_values`: Some(1), `max_size`: None, mode: `Measured`)
    /// The range of component `n` is `[0, 10485760]`.
    fn authorize_fixed_falcon512_invalid(n: u32) -> Weight {
        // Proof Size summary in bytes:
        //  Measured:  `65`
        //  Estimated: `1550`
        // Minimum execution time: 78_568_000 picoseconds.
        Weight::from_parts(63_619_533, 1550)
            // Standard Error: 2
            .saturating_add(Weight::from_parts(2_298, 0).saturating_mul(n.into()))
            .saturating_add(T::DbWeight::get().reads(1_u64))
    }
    /// Storage: `AdminUtils::HashedAccountsEnabled` (r:1 w:0)
    /// Proof: `AdminUtils::HashedAccountsEnabled` (`max_values`: Some(1), `max_size`: None, mode: `Measured`)
    /// Storage: `System::Account` (r:1 w:1)
    /// Proof: `System::Account` (`max_values`: None, `max_size`: Some(104), added: 2579, mode: `MaxEncodedLen`)
    /// The range of component `n` is `[0, 10485760]`.
    fn authorize_fixed_falcon1024(n: u32) -> Weight {
        // Proof Size summary in bytes:
        //  Measured:  `180`
        //  Estimated: `3569`
        // Minimum execution time: 138_047_000 picoseconds.
        Weight::from_parts(233_764_723, 3569)
            // Standard Error: 2
            .saturating_add(Weight::from_parts(2_249, 0).saturating_mul(n.into()))
            .saturating_add(T::DbWeight::get().reads(2_u64))
            .saturating_add(T::DbWeight::get().writes(1_u64))
    }
    /// Storage: `AdminUtils::HashedAccountsEnabled` (r:1 w:0)
    /// Proof: `AdminUtils::HashedAccountsEnabled` (`max_values`: Some(1), `max_size`: None, mode: `Measured`)
    /// The range of component `n` is `[0, 10485760]`.
    fn authorize_fixed_falcon1024_invalid(n: u32) -> Weight {
        // Proof Size summary in bytes:
        //  Measured:  `65`
        //  Estimated: `1550`
        // Minimum execution time: 115_593_000 picoseconds.
        Weight::from_parts(107_584_423, 1550)
            // Standard Error: 2
            .saturating_add(Weight::from_parts(2_329, 0).saturating_mul(n.into()))
            .saturating_add(T::DbWeight::get().reads(1_u64))
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

    /// Storage: `AdminUtils::HashedAccountsEnabled` (r:1 w:0)
    /// Proof: `AdminUtils::HashedAccountsEnabled` (`max_values`: Some(1), `max_size`: None, mode: `Measured`)
    /// Storage: `HashedAccounts::MlDsaAuthorizationEnabled` (r:1 w:0)
    /// Proof: `HashedAccounts::MlDsaAuthorizationEnabled` (`max_values`: Some(1), `max_size`: Some(1), added: 496, mode: `MaxEncodedLen`)
    /// Storage: `HashedAccounts::Accounts` (r:1 w:0)
    /// Proof: `HashedAccounts::Accounts` (`max_values`: None, `max_size`: Some(122), added: 2597, mode: `MaxEncodedLen`)
    /// Storage: `HashedAccounts::MlDsaAccounts` (r:1 w:0)
    /// Proof: `HashedAccounts::MlDsaAccounts` (`max_values`: None, `max_size`: Some(186), added: 2661, mode: `MaxEncodedLen`)
    /// Storage: `HashedAccounts::CapabilityEpochs` (r:1 w:1)
    /// Proof: `HashedAccounts::CapabilityEpochs` (`max_values`: None, `max_size`: Some(65), added: 2540, mode: `MaxEncodedLen`)
    /// Storage: `HashedAccounts::ActiveCapabilities` (r:0 w:1)
    /// Proof: `HashedAccounts::ActiveCapabilities` (`max_values`: None, `max_size`: Some(300), added: 2775, mode: `MaxEncodedLen`)
    fn install_capability() -> Weight {
        // Proof Size summary in bytes:
        //  Measured:  `873`
        //  Estimated: `3651`
        // Minimum execution time: 29_505_000 picoseconds.
        Weight::from_parts(34_592_000, 3651)
            .saturating_add(RocksDbWeight::get().reads(5_u64))
            .saturating_add(RocksDbWeight::get().writes(2_u64))
    }
    /// Storage: `HashedAccounts::Accounts` (r:1 w:0)
    /// Proof: `HashedAccounts::Accounts` (`max_values`: None, `max_size`: Some(122), added: 2597, mode: `MaxEncodedLen`)
    /// Storage: `HashedAccounts::MlDsaAccounts` (r:1 w:0)
    /// Proof: `HashedAccounts::MlDsaAccounts` (`max_values`: None, `max_size`: Some(186), added: 2661, mode: `MaxEncodedLen`)
    /// Storage: `HashedAccounts::CapabilityEpochs` (r:1 w:1)
    /// Proof: `HashedAccounts::CapabilityEpochs` (`max_values`: None, `max_size`: Some(65), added: 2540, mode: `MaxEncodedLen`)
    /// Storage: `HashedAccounts::ActiveCapabilities` (r:0 w:1)
    /// Proof: `HashedAccounts::ActiveCapabilities` (`max_values`: None, `max_size`: Some(300), added: 2775, mode: `MaxEncodedLen`)
    fn revoke_capability() -> Weight {
        // Proof Size summary in bytes:
        //  Measured:  `415`
        //  Estimated: `3651`
        // Minimum execution time: 21_362_000 picoseconds.
        Weight::from_parts(22_364_000, 3651)
            .saturating_add(RocksDbWeight::get().reads(3_u64))
            .saturating_add(RocksDbWeight::get().writes(2_u64))
    }
    /// Storage: `AdminUtils::HashedAccountsEnabled` (r:1 w:0)
    /// Proof: `AdminUtils::HashedAccountsEnabled` (`max_values`: Some(1), `max_size`: None, mode: `Measured`)
    /// Storage: `HashedAccounts::MlDsaAccounts` (r:22 w:1)
    /// Proof: `HashedAccounts::MlDsaAccounts` (`max_values`: None, `max_size`: Some(186), added: 2661, mode: `MaxEncodedLen`)
    /// Storage: `HashedAccounts::Accounts` (r:22 w:0)
    /// Proof: `HashedAccounts::Accounts` (`max_values`: None, `max_size`: Some(122), added: 2597, mode: `MaxEncodedLen`)
    /// Storage: `HashedAccounts::DerivedAuthorities` (r:22 w:0)
    /// Proof: `HashedAccounts::DerivedAuthorities` (`max_values`: None, `max_size`: Some(6460), added: 8935, mode: `MaxEncodedLen`)
    /// Storage: `HashedAccounts::EvmAliases` (r:1 w:1)
    /// Proof: `HashedAccounts::EvmAliases` (`max_values`: None, `max_size`: Some(68), added: 2543, mode: `MaxEncodedLen`)
    /// Storage: `SubtensorModule::Owner` (r:1 w:0)
    /// Proof: `SubtensorModule::Owner` (`max_values`: None, `max_size`: None, mode: `Measured`)
    /// Storage: `Proxy::Proxies` (r:1 w:0)
    /// Proof: `Proxy::Proxies` (`max_values`: None, `max_size`: Some(789), added: 3264, mode: `MaxEncodedLen`)
    /// Storage: `SubtensorModule::ColdkeySwapAnnouncements` (r:1 w:0)
    /// Proof: `SubtensorModule::ColdkeySwapAnnouncements` (`max_values`: None, `max_size`: None, mode: `Measured`)
    /// Storage: `SubtensorModule::ColdkeySwapDisputes` (r:1 w:0)
    /// Proof: `SubtensorModule::ColdkeySwapDisputes` (`max_values`: None, `max_size`: None, mode: `Measured`)
    /// Storage: `Multisig::Multisigs` (r:1 w:0)
    /// Proof: `Multisig::Multisigs` (`max_values`: None, `max_size`: Some(3338), added: 5813, mode: `MaxEncodedLen`)
    /// Storage: `Proxy::Announcements` (r:20 w:0)
    /// Proof: `Proxy::Announcements` (`max_values`: None, `max_size`: Some(5150), added: 7625, mode: `MaxEncodedLen`)
    /// Storage: `System::Account` (r:2 w:2)
    /// Proof: `System::Account` (`max_values`: None, `max_size`: Some(104), added: 2579, mode: `MaxEncodedLen`)
    /// Storage: `Balances::Locks` (r:1 w:0)
    /// Proof: `Balances::Locks` (`max_values`: None, `max_size`: Some(899), added: 3374, mode: `MaxEncodedLen`)
    /// Storage: `Balances::Reserves` (r:1 w:0)
    /// Proof: `Balances::Reserves` (`max_values`: None, `max_size`: Some(849), added: 3324, mode: `MaxEncodedLen`)
    /// Storage: `Balances::Holds` (r:1 w:0)
    /// Proof: `Balances::Holds` (`max_values`: None, `max_size`: Some(89), added: 2564, mode: `MaxEncodedLen`)
    /// Storage: `Balances::Freezes` (r:1 w:0)
    /// Proof: `Balances::Freezes` (`max_values`: None, `max_size`: Some(499), added: 2974, mode: `MaxEncodedLen`)
    /// Storage: `EVM::AccountCodes` (r:1 w:0)
    /// Proof: `EVM::AccountCodes` (`max_values`: None, `max_size`: None, mode: `Measured`)
    /// Storage: `EVM::AccountCodesMetadata` (r:1 w:0)
    /// Proof: `EVM::AccountCodesMetadata` (`max_values`: None, `max_size`: None, mode: `Measured`)
    /// Storage: `EVM::AccountStorages` (r:1 w:0)
    /// Proof: `EVM::AccountStorages` (`max_values`: None, `max_size`: None, mode: `Measured`)
    fn register_falcon() -> Weight {
        // Proof Size summary in bytes:
        //  Measured:  `116616`
        //  Estimated: `197560`
        // Minimum execution time: 427_118_000 picoseconds.
        Weight::from_parts(450_464_000, 197560)
            .saturating_add(RocksDbWeight::get().reads(102_u64))
            .saturating_add(RocksDbWeight::get().writes(4_u64))
    }
    /// Storage: `AdminUtils::HashedAccountsEnabled` (r:1 w:0)
    /// Proof: `AdminUtils::HashedAccountsEnabled` (`max_values`: Some(1), `max_size`: None, mode: `Measured`)
    /// Storage: `HashedAccounts::MlDsaAccounts` (r:1 w:0)
    /// Proof: `HashedAccounts::MlDsaAccounts` (`max_values`: None, `max_size`: Some(186), added: 2661, mode: `MaxEncodedLen`)
    fn check_registered_falcon() -> Weight {
        // Proof Size summary in bytes:
        //  Measured:  `425`
        //  Estimated: `3651`
        // Minimum execution time: 20_481_000 picoseconds.
        Weight::from_parts(25_588_000, 3651).saturating_add(RocksDbWeight::get().reads(2_u64))
    }
    /// Storage: `AdminUtils::HashedAccountsEnabled` (r:1 w:0)
    /// Proof: `AdminUtils::HashedAccountsEnabled` (`max_values`: Some(1), `max_size`: None, mode: `Measured`)
    /// Storage: `HashedAccounts::MlDsaAccounts` (r:22 w:1)
    /// Proof: `HashedAccounts::MlDsaAccounts` (`max_values`: None, `max_size`: Some(186), added: 2661, mode: `MaxEncodedLen`)
    /// Storage: `HashedAccounts::MlDsaRegistrationEnabled` (r:1 w:0)
    /// Proof: `HashedAccounts::MlDsaRegistrationEnabled` (`max_values`: Some(1), `max_size`: Some(1), added: 496, mode: `MaxEncodedLen`)
    /// Storage: `HashedAccounts::Accounts` (r:22 w:0)
    /// Proof: `HashedAccounts::Accounts` (`max_values`: None, `max_size`: Some(122), added: 2597, mode: `MaxEncodedLen`)
    /// Storage: `HashedAccounts::DerivedAuthorities` (r:22 w:0)
    /// Proof: `HashedAccounts::DerivedAuthorities` (`max_values`: None, `max_size`: Some(6460), added: 8935, mode: `MaxEncodedLen`)
    /// Storage: `HashedAccounts::EvmAliases` (r:1 w:1)
    /// Proof: `HashedAccounts::EvmAliases` (`max_values`: None, `max_size`: Some(68), added: 2543, mode: `MaxEncodedLen`)
    /// Storage: `SubtensorModule::Owner` (r:1 w:0)
    /// Proof: `SubtensorModule::Owner` (`max_values`: None, `max_size`: None, mode: `Measured`)
    /// Storage: `Proxy::Proxies` (r:1 w:0)
    /// Proof: `Proxy::Proxies` (`max_values`: None, `max_size`: Some(789), added: 3264, mode: `MaxEncodedLen`)
    /// Storage: `SubtensorModule::ColdkeySwapAnnouncements` (r:1 w:0)
    /// Proof: `SubtensorModule::ColdkeySwapAnnouncements` (`max_values`: None, `max_size`: None, mode: `Measured`)
    /// Storage: `SubtensorModule::ColdkeySwapDisputes` (r:1 w:0)
    /// Proof: `SubtensorModule::ColdkeySwapDisputes` (`max_values`: None, `max_size`: None, mode: `Measured`)
    /// Storage: `Multisig::Multisigs` (r:1 w:0)
    /// Proof: `Multisig::Multisigs` (`max_values`: None, `max_size`: Some(3338), added: 5813, mode: `MaxEncodedLen`)
    /// Storage: `Proxy::Announcements` (r:20 w:0)
    /// Proof: `Proxy::Announcements` (`max_values`: None, `max_size`: Some(5150), added: 7625, mode: `MaxEncodedLen`)
    /// Storage: `System::Account` (r:2 w:2)
    /// Proof: `System::Account` (`max_values`: None, `max_size`: Some(104), added: 2579, mode: `MaxEncodedLen`)
    /// Storage: `Balances::Locks` (r:1 w:0)
    /// Proof: `Balances::Locks` (`max_values`: None, `max_size`: Some(899), added: 3374, mode: `MaxEncodedLen`)
    /// Storage: `Balances::Reserves` (r:1 w:0)
    /// Proof: `Balances::Reserves` (`max_values`: None, `max_size`: Some(849), added: 3324, mode: `MaxEncodedLen`)
    /// Storage: `Balances::Holds` (r:1 w:0)
    /// Proof: `Balances::Holds` (`max_values`: None, `max_size`: Some(89), added: 2564, mode: `MaxEncodedLen`)
    /// Storage: `Balances::Freezes` (r:1 w:0)
    /// Proof: `Balances::Freezes` (`max_values`: None, `max_size`: Some(499), added: 2974, mode: `MaxEncodedLen`)
    /// Storage: `EVM::AccountCodes` (r:1 w:0)
    /// Proof: `EVM::AccountCodes` (`max_values`: None, `max_size`: None, mode: `Measured`)
    /// Storage: `EVM::AccountCodesMetadata` (r:1 w:0)
    /// Proof: `EVM::AccountCodesMetadata` (`max_values`: None, `max_size`: None, mode: `Measured`)
    /// Storage: `EVM::AccountStorages` (r:1 w:0)
    /// Proof: `EVM::AccountStorages` (`max_values`: None, `max_size`: None, mode: `Measured`)
    fn register_mldsa65() -> Weight {
        // Proof Size summary in bytes:
        //  Measured:  `115690`
        //  Estimated: `197560`
        // Minimum execution time: 339_453_000 picoseconds.
        Weight::from_parts(360_816_000, 197560)
            .saturating_add(RocksDbWeight::get().reads(103_u64))
            .saturating_add(RocksDbWeight::get().writes(4_u64))
    }
    /// Storage: `AdminUtils::HashedAccountsEnabled` (r:1 w:0)
    /// Proof: `AdminUtils::HashedAccountsEnabled` (`max_values`: Some(1), `max_size`: None, mode: `Measured`)
    /// Storage: `HashedAccounts::MlDsaAccounts` (r:1 w:0)
    /// Proof: `HashedAccounts::MlDsaAccounts` (`max_values`: None, `max_size`: Some(186), added: 2661, mode: `MaxEncodedLen`)
    fn check_registered_mldsa65() -> Weight {
        // Proof Size summary in bytes:
        //  Measured:  `465`
        //  Estimated: `3651`
        // Minimum execution time: 13_500_000 picoseconds.
        Weight::from_parts(15_153_000, 3651).saturating_add(RocksDbWeight::get().reads(2_u64))
    }
    /// Storage: `AdminUtils::HashedAccountsEnabled` (r:1 w:0)
    /// Proof: `AdminUtils::HashedAccountsEnabled` (`max_values`: Some(1), `max_size`: None, mode: `Measured`)
    /// Storage: `HashedAccounts::DerivedAuthorities` (r:946 w:1)
    /// Proof: `HashedAccounts::DerivedAuthorities` (`max_values`: None, `max_size`: Some(6460), added: 8935, mode: `MaxEncodedLen`)
    /// Storage: `HashedAccounts::MlDsaRegistrationEnabled` (r:1 w:0)
    /// Proof: `HashedAccounts::MlDsaRegistrationEnabled` (`max_values`: Some(1), `max_size`: Some(1), added: 496, mode: `MaxEncodedLen`)
    /// Storage: `HashedAccounts::Accounts` (r:22 w:0)
    /// Proof: `HashedAccounts::Accounts` (`max_values`: None, `max_size`: Some(122), added: 2597, mode: `MaxEncodedLen`)
    /// Storage: `HashedAccounts::MlDsaAccounts` (r:122 w:0)
    /// Proof: `HashedAccounts::MlDsaAccounts` (`max_values`: None, `max_size`: Some(186), added: 2661, mode: `MaxEncodedLen`)
    /// Storage: `HashedAccounts::EvmAliases` (r:1 w:1)
    /// Proof: `HashedAccounts::EvmAliases` (`max_values`: None, `max_size`: Some(68), added: 2543, mode: `MaxEncodedLen`)
    /// Storage: `SubtensorModule::Owner` (r:1 w:0)
    /// Proof: `SubtensorModule::Owner` (`max_values`: None, `max_size`: None, mode: `Measured`)
    /// Storage: `Proxy::Proxies` (r:1 w:0)
    /// Proof: `Proxy::Proxies` (`max_values`: None, `max_size`: Some(789), added: 3264, mode: `MaxEncodedLen`)
    /// Storage: `SubtensorModule::ColdkeySwapAnnouncements` (r:1 w:0)
    /// Proof: `SubtensorModule::ColdkeySwapAnnouncements` (`max_values`: None, `max_size`: None, mode: `Measured`)
    /// Storage: `SubtensorModule::ColdkeySwapDisputes` (r:1 w:0)
    /// Proof: `SubtensorModule::ColdkeySwapDisputes` (`max_values`: None, `max_size`: None, mode: `Measured`)
    /// Storage: `Multisig::Multisigs` (r:1 w:0)
    /// Proof: `Multisig::Multisigs` (`max_values`: None, `max_size`: Some(3338), added: 5813, mode: `MaxEncodedLen`)
    /// Storage: `Proxy::Announcements` (r:20 w:0)
    /// Proof: `Proxy::Announcements` (`max_values`: None, `max_size`: Some(5150), added: 7625, mode: `MaxEncodedLen`)
    /// Storage: `System::Account` (r:2 w:2)
    /// Proof: `System::Account` (`max_values`: None, `max_size`: Some(104), added: 2579, mode: `MaxEncodedLen`)
    /// Storage: `Balances::Locks` (r:1 w:0)
    /// Proof: `Balances::Locks` (`max_values`: None, `max_size`: Some(899), added: 3374, mode: `MaxEncodedLen`)
    /// Storage: `Balances::Reserves` (r:1 w:0)
    /// Proof: `Balances::Reserves` (`max_values`: None, `max_size`: Some(849), added: 3324, mode: `MaxEncodedLen`)
    /// Storage: `Balances::Holds` (r:1 w:0)
    /// Proof: `Balances::Holds` (`max_values`: None, `max_size`: Some(89), added: 2564, mode: `MaxEncodedLen`)
    /// Storage: `Balances::Freezes` (r:1 w:0)
    /// Proof: `Balances::Freezes` (`max_values`: None, `max_size`: Some(499), added: 2974, mode: `MaxEncodedLen`)
    /// Storage: `EVM::AccountCodes` (r:1 w:0)
    /// Proof: `EVM::AccountCodes` (`max_values`: None, `max_size`: None, mode: `Measured`)
    /// Storage: `EVM::AccountCodesMetadata` (r:1 w:0)
    /// Proof: `EVM::AccountCodesMetadata` (`max_values`: None, `max_size`: None, mode: `Measured`)
    /// Storage: `EVM::AccountStorages` (r:1 w:0)
    /// Proof: `EVM::AccountStorages` (`max_values`: None, `max_size`: None, mode: `Measured`)
    fn register_derived() -> Weight {
        // Proof Size summary in bytes:
        //  Measured:  `281775`
        //  Estimated: `8453500`
        // Minimum execution time: 4_101_194_000 picoseconds.
        Weight::from_parts(4_313_926_000, 8453500)
            .saturating_add(RocksDbWeight::get().reads(1127_u64))
            .saturating_add(RocksDbWeight::get().writes(4_u64))
    }
    /// Storage: `AdminUtils::HashedAccountsEnabled` (r:1 w:0)
    /// Proof: `AdminUtils::HashedAccountsEnabled` (`max_values`: Some(1), `max_size`: None, mode: `Measured`)
    /// Storage: `HashedAccounts::DerivedAuthorities` (r:946 w:1)
    /// Proof: `HashedAccounts::DerivedAuthorities` (`max_values`: None, `max_size`: Some(6460), added: 8935, mode: `MaxEncodedLen`)
    /// Storage: `HashedAccounts::MlDsaRegistrationEnabled` (r:1 w:0)
    /// Proof: `HashedAccounts::MlDsaRegistrationEnabled` (`max_values`: Some(1), `max_size`: Some(1), added: 496, mode: `MaxEncodedLen`)
    /// Storage: `HashedAccounts::Accounts` (r:22 w:0)
    /// Proof: `HashedAccounts::Accounts` (`max_values`: None, `max_size`: Some(122), added: 2597, mode: `MaxEncodedLen`)
    /// Storage: `HashedAccounts::MlDsaAccounts` (r:122 w:0)
    /// Proof: `HashedAccounts::MlDsaAccounts` (`max_values`: None, `max_size`: Some(186), added: 2661, mode: `MaxEncodedLen`)
    /// Storage: `HashedAccounts::EvmAliases` (r:1 w:1)
    /// Proof: `HashedAccounts::EvmAliases` (`max_values`: None, `max_size`: Some(68), added: 2543, mode: `MaxEncodedLen`)
    /// Storage: `System::ExtrinsicData` (r:1 w:0)
    /// Proof: `System::ExtrinsicData` (`max_values`: None, `max_size`: None, mode: `Measured`)
    /// Storage: `SubtensorModule::Owner` (r:1 w:0)
    /// Proof: `SubtensorModule::Owner` (`max_values`: None, `max_size`: None, mode: `Measured`)
    /// Storage: `Proxy::Proxies` (r:1 w:0)
    /// Proof: `Proxy::Proxies` (`max_values`: None, `max_size`: Some(789), added: 3264, mode: `MaxEncodedLen`)
    /// Storage: `SubtensorModule::ColdkeySwapAnnouncements` (r:1 w:0)
    /// Proof: `SubtensorModule::ColdkeySwapAnnouncements` (`max_values`: None, `max_size`: None, mode: `Measured`)
    /// Storage: `SubtensorModule::ColdkeySwapDisputes` (r:1 w:0)
    /// Proof: `SubtensorModule::ColdkeySwapDisputes` (`max_values`: None, `max_size`: None, mode: `Measured`)
    /// Storage: `Multisig::Multisigs` (r:1 w:0)
    /// Proof: `Multisig::Multisigs` (`max_values`: None, `max_size`: Some(3338), added: 5813, mode: `MaxEncodedLen`)
    /// Storage: `Proxy::Announcements` (r:20 w:0)
    /// Proof: `Proxy::Announcements` (`max_values`: None, `max_size`: Some(5150), added: 7625, mode: `MaxEncodedLen`)
    /// Storage: `System::Account` (r:2 w:2)
    /// Proof: `System::Account` (`max_values`: None, `max_size`: Some(104), added: 2579, mode: `MaxEncodedLen`)
    /// Storage: `Balances::Locks` (r:1 w:0)
    /// Proof: `Balances::Locks` (`max_values`: None, `max_size`: Some(899), added: 3374, mode: `MaxEncodedLen`)
    /// Storage: `Balances::Reserves` (r:1 w:0)
    /// Proof: `Balances::Reserves` (`max_values`: None, `max_size`: Some(849), added: 3324, mode: `MaxEncodedLen`)
    /// Storage: `Balances::Holds` (r:1 w:0)
    /// Proof: `Balances::Holds` (`max_values`: None, `max_size`: Some(89), added: 2564, mode: `MaxEncodedLen`)
    /// Storage: `Balances::Freezes` (r:1 w:0)
    /// Proof: `Balances::Freezes` (`max_values`: None, `max_size`: Some(499), added: 2974, mode: `MaxEncodedLen`)
    /// Storage: `EVM::AccountCodes` (r:1 w:0)
    /// Proof: `EVM::AccountCodes` (`max_values`: None, `max_size`: None, mode: `Measured`)
    /// Storage: `EVM::AccountCodesMetadata` (r:1 w:0)
    /// Proof: `EVM::AccountCodesMetadata` (`max_values`: None, `max_size`: None, mode: `Measured`)
    /// Storage: `EVM::AccountStorages` (r:1 w:0)
    /// Proof: `EVM::AccountStorages` (`max_values`: None, `max_size`: None, mode: `Measured`)
    fn register_derived_sponsored() -> Weight {
        // Proof Size summary in bytes:
        //  Measured:  `347319`
        //  Estimated: `8453500`
        // Minimum execution time: 5_234_469_000 picoseconds.
        Weight::from_parts(5_530_897_000, 8453500)
            .saturating_add(RocksDbWeight::get().reads(1128_u64))
            .saturating_add(RocksDbWeight::get().writes(4_u64))
    }
    /// Storage: `AdminUtils::HashedAccountsEnabled` (r:1 w:0)
    /// Proof: `AdminUtils::HashedAccountsEnabled` (`max_values`: Some(1), `max_size`: None, mode: `Measured`)
    /// Storage: `HashedAccounts::DerivedAuthorities` (r:1 w:0)
    /// Proof: `HashedAccounts::DerivedAuthorities` (`max_values`: None, `max_size`: Some(6460), added: 8935, mode: `MaxEncodedLen`)
    fn check_derived() -> Weight {
        // Proof Size summary in bytes:
        //  Measured:  `7972`
        //  Estimated: `9925`
        // Minimum execution time: 24_217_000 picoseconds.
        Weight::from_parts(28_023_000, 9925).saturating_add(RocksDbWeight::get().reads(2_u64))
    }
    /// Storage: `AdminUtils::HashedAccountsEnabled` (r:1 w:0)
    /// Proof: `AdminUtils::HashedAccountsEnabled` (`max_values`: Some(1), `max_size`: None, mode: `Measured`)
    /// Storage: `HashedAccounts::MlDsaAuthorizationEnabled` (r:1 w:0)
    /// Proof: `HashedAccounts::MlDsaAuthorizationEnabled` (`max_values`: Some(1), `max_size`: Some(1), added: 496, mode: `MaxEncodedLen`)
    /// Storage: `HashedAccounts::MlDsaAccounts` (r:1 w:1)
    /// Proof: `HashedAccounts::MlDsaAccounts` (`max_values`: None, `max_size`: Some(186), added: 2661, mode: `MaxEncodedLen`)
    /// The range of component `n` is `[0, 10485760]`.
    fn authorize_mldsa65(n: u32) -> Weight {
        // Proof Size summary in bytes:
        //  Measured:  `465`
        //  Estimated: `3651`
        // Minimum execution time: 224_590_000 picoseconds.
        Weight::from_parts(185_991_410, 3651)
            // Standard Error: 4
            .saturating_add(Weight::from_parts(2_246, 0).saturating_mul(n.into()))
            .saturating_add(RocksDbWeight::get().reads(3_u64))
            .saturating_add(RocksDbWeight::get().writes(1_u64))
    }
    /// Storage: `AdminUtils::HashedAccountsEnabled` (r:1 w:0)
    /// Proof: `AdminUtils::HashedAccountsEnabled` (`max_values`: Some(1), `max_size`: None, mode: `Measured`)
    /// Storage: `HashedAccounts::MlDsaAuthorizationEnabled` (r:1 w:0)
    /// Proof: `HashedAccounts::MlDsaAuthorizationEnabled` (`max_values`: Some(1), `max_size`: Some(1), added: 496, mode: `MaxEncodedLen`)
    /// Storage: `HashedAccounts::MlDsaAccounts` (r:1 w:0)
    /// Proof: `HashedAccounts::MlDsaAccounts` (`max_values`: None, `max_size`: Some(186), added: 2661, mode: `MaxEncodedLen`)
    /// The range of component `n` is `[0, 10485760]`.
    fn authorize_mldsa65_invalid(n: u32) -> Weight {
        // Proof Size summary in bytes:
        //  Measured:  `465`
        //  Estimated: `3651`
        // Minimum execution time: 237_099_000 picoseconds.
        Weight::from_parts(349_427_694, 3651)
            // Standard Error: 1
            .saturating_add(Weight::from_parts(2_271, 0).saturating_mul(n.into()))
            .saturating_add(RocksDbWeight::get().reads(3_u64))
    }
    /// Storage: `AdminUtils::HashedAccountsEnabled` (r:1 w:0)
    /// Proof: `AdminUtils::HashedAccountsEnabled` (`max_values`: Some(1), `max_size`: None, mode: `Measured`)
    /// Storage: `HashedAccounts::MlDsaAccounts` (r:1 w:1)
    /// Proof: `HashedAccounts::MlDsaAccounts` (`max_values`: None, `max_size`: Some(186), added: 2661, mode: `MaxEncodedLen`)
    /// The range of component `n` is `[0, 10485760]`.
    fn authorize_falcon512(n: u32) -> Weight {
        // Proof Size summary in bytes:
        //  Measured:  `425`
        //  Estimated: `3651`
        // Minimum execution time: 78_208_000 picoseconds.
        Weight::from_parts(206_650_078, 3651)
            // Standard Error: 2
            .saturating_add(Weight::from_parts(2_260, 0).saturating_mul(n.into()))
            .saturating_add(RocksDbWeight::get().reads(2_u64))
            .saturating_add(RocksDbWeight::get().writes(1_u64))
    }
    /// Storage: `AdminUtils::HashedAccountsEnabled` (r:1 w:0)
    /// Proof: `AdminUtils::HashedAccountsEnabled` (`max_values`: Some(1), `max_size`: None, mode: `Measured`)
    /// Storage: `HashedAccounts::MlDsaAccounts` (r:1 w:0)
    /// Proof: `HashedAccounts::MlDsaAccounts` (`max_values`: None, `max_size`: Some(186), added: 2661, mode: `MaxEncodedLen`)
    /// The range of component `n` is `[0, 10485760]`.
    fn authorize_falcon512_invalid(n: u32) -> Weight {
        // Proof Size summary in bytes:
        //  Measured:  `425`
        //  Estimated: `3651`
        // Minimum execution time: 64_967_000 picoseconds.
        Weight::from_parts(70_732_205, 3651)
            // Standard Error: 3
            .saturating_add(Weight::from_parts(2_274, 0).saturating_mul(n.into()))
            .saturating_add(RocksDbWeight::get().reads(2_u64))
    }
    /// Storage: `AdminUtils::HashedAccountsEnabled` (r:1 w:0)
    /// Proof: `AdminUtils::HashedAccountsEnabled` (`max_values`: Some(1), `max_size`: None, mode: `Measured`)
    /// Storage: `HashedAccounts::MlDsaAccounts` (r:1 w:1)
    /// Proof: `HashedAccounts::MlDsaAccounts` (`max_values`: None, `max_size`: Some(186), added: 2661, mode: `MaxEncodedLen`)
    /// The range of component `n` is `[0, 10485760]`.
    fn authorize_falcon1024(n: u32) -> Weight {
        // Proof Size summary in bytes:
        //  Measured:  `425`
        //  Estimated: `3651`
        // Minimum execution time: 123_475_000 picoseconds.
        Weight::from_parts(215_567_566, 3651)
            // Standard Error: 3
            .saturating_add(Weight::from_parts(2_280, 0).saturating_mul(n.into()))
            .saturating_add(RocksDbWeight::get().reads(2_u64))
            .saturating_add(RocksDbWeight::get().writes(1_u64))
    }
    /// Storage: `AdminUtils::HashedAccountsEnabled` (r:1 w:0)
    /// Proof: `AdminUtils::HashedAccountsEnabled` (`max_values`: Some(1), `max_size`: None, mode: `Measured`)
    /// Storage: `HashedAccounts::MlDsaAccounts` (r:1 w:0)
    /// Proof: `HashedAccounts::MlDsaAccounts` (`max_values`: None, `max_size`: Some(186), added: 2661, mode: `MaxEncodedLen`)
    /// The range of component `n` is `[0, 10485760]`.
    fn authorize_falcon1024_invalid(n: u32) -> Weight {
        // Proof Size summary in bytes:
        //  Measured:  `425`
        //  Estimated: `3651`
        // Minimum execution time: 109_874_000 picoseconds.
        Weight::from_parts(93_923_526, 3651)
            // Standard Error: 2
            .saturating_add(Weight::from_parts(2_288, 0).saturating_mul(n.into()))
            .saturating_add(RocksDbWeight::get().reads(2_u64))
    }
    /// Storage: `AdminUtils::HashedAccountsEnabled` (r:1 w:0)
    /// Proof: `AdminUtils::HashedAccountsEnabled` (`max_values`: Some(1), `max_size`: None, mode: `Measured`)
    /// Storage: `System::Account` (r:1 w:1)
    /// Proof: `System::Account` (`max_values`: None, `max_size`: Some(104), added: 2579, mode: `MaxEncodedLen`)
    /// The range of component `n` is `[0, 10485760]`.
    fn authorize_fixed_falcon512(n: u32) -> Weight {
        // Proof Size summary in bytes:
        //  Measured:  `247`
        //  Estimated: `3569`
        // Minimum execution time: 100_149_000 picoseconds.
        Weight::from_parts(64_919_788, 3569)
            // Standard Error: 1
            .saturating_add(Weight::from_parts(2_297, 0).saturating_mul(n.into()))
            .saturating_add(RocksDbWeight::get().reads(2_u64))
            .saturating_add(RocksDbWeight::get().writes(1_u64))
    }
    /// Storage: `AdminUtils::HashedAccountsEnabled` (r:1 w:0)
    /// Proof: `AdminUtils::HashedAccountsEnabled` (`max_values`: Some(1), `max_size`: None, mode: `Measured`)
    /// The range of component `n` is `[0, 10485760]`.
    fn authorize_fixed_falcon512_invalid(n: u32) -> Weight {
        // Proof Size summary in bytes:
        //  Measured:  `65`
        //  Estimated: `1550`
        // Minimum execution time: 78_568_000 picoseconds.
        Weight::from_parts(63_619_533, 1550)
            // Standard Error: 2
            .saturating_add(Weight::from_parts(2_298, 0).saturating_mul(n.into()))
            .saturating_add(RocksDbWeight::get().reads(1_u64))
    }
    /// Storage: `AdminUtils::HashedAccountsEnabled` (r:1 w:0)
    /// Proof: `AdminUtils::HashedAccountsEnabled` (`max_values`: Some(1), `max_size`: None, mode: `Measured`)
    /// Storage: `System::Account` (r:1 w:1)
    /// Proof: `System::Account` (`max_values`: None, `max_size`: Some(104), added: 2579, mode: `MaxEncodedLen`)
    /// The range of component `n` is `[0, 10485760]`.
    fn authorize_fixed_falcon1024(n: u32) -> Weight {
        // Proof Size summary in bytes:
        //  Measured:  `180`
        //  Estimated: `3569`
        // Minimum execution time: 138_047_000 picoseconds.
        Weight::from_parts(233_764_723, 3569)
            // Standard Error: 2
            .saturating_add(Weight::from_parts(2_249, 0).saturating_mul(n.into()))
            .saturating_add(RocksDbWeight::get().reads(2_u64))
            .saturating_add(RocksDbWeight::get().writes(1_u64))
    }
    /// Storage: `AdminUtils::HashedAccountsEnabled` (r:1 w:0)
    /// Proof: `AdminUtils::HashedAccountsEnabled` (`max_values`: Some(1), `max_size`: None, mode: `Measured`)
    /// The range of component `n` is `[0, 10485760]`.
    fn authorize_fixed_falcon1024_invalid(n: u32) -> Weight {
        // Proof Size summary in bytes:
        //  Measured:  `65`
        //  Estimated: `1550`
        // Minimum execution time: 115_593_000 picoseconds.
        Weight::from_parts(107_584_423, 1550)
            // Standard Error: 2
            .saturating_add(Weight::from_parts(2_329, 0).saturating_mul(n.into()))
            .saturating_add(RocksDbWeight::get().reads(1_u64))
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
