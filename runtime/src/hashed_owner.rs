//! Ownership must not provide a classical route around a hashed hotkey.
use crate::{AccountId, HashedAccounts, Runtime};
use frame_support::weights::Weight;

pub struct HashedHotkeyOwnerPolicy;

impl pallet_subtensor::HotkeyOwnerPolicy<AccountId> for HashedHotkeyOwnerPolicy {
    fn allows_owner(coldkey: &AccountId, hotkey: &AccountId) -> bool {
        !HashedAccounts::is_registered(hotkey) || HashedAccounts::is_registered(coldkey)
    }

    fn allows_coldkey_swap(old: &AccountId, new: &AccountId) -> bool {
        // This preserves all existing owner relationships without enumerating
        // their hotkeys. Registration also rejects a preexisting classical owner.
        !HashedAccounts::is_registered(old) || HashedAccounts::is_registered(new)
    }

    fn weight() -> Weight {
        <Runtime as frame_system::Config>::DbWeight::get().reads(2)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pallet_subtensor::HotkeyOwnerPolicy;

    #[test]
    fn protected_hotkeys_and_coldkeys_cannot_fall_back_to_classical_control() {
        sp_io::TestExternalities::default().execute_with(|| {
            let classical = AccountId::new([3; 32]);
            let protected = AccountId::new([4; 32]);
            let other_protected = AccountId::new([5; 32]);
            for account in [&protected, &other_protected] {
                pallet_hashed_accounts::Accounts::<Runtime>::insert(
                    account,
                    pallet_hashed_accounts::AccountRecord {
                        descriptor: subtensor_hashed::Descriptor {
                            version: 1,
                            scheme: subtensor_hashed::Scheme::Sr25519,
                            initial_commitment: [7; 32],
                        },
                        generation: 0,
                        commitment: [7; 32],
                    },
                );
            }
            assert!(!HashedHotkeyOwnerPolicy::allows_owner(
                &classical, &protected
            ));
            assert!(HashedHotkeyOwnerPolicy::allows_owner(
                &protected,
                &other_protected
            ));
            assert!(HashedHotkeyOwnerPolicy::allows_owner(
                &classical, &classical
            ));
            assert!(!HashedHotkeyOwnerPolicy::allows_coldkey_swap(
                &protected, &classical
            ));
            assert!(HashedHotkeyOwnerPolicy::allows_coldkey_swap(
                &classical, &protected
            ));
            assert!(HashedHotkeyOwnerPolicy::allows_coldkey_swap(
                &protected,
                &other_protected
            ));
        });
    }
}
