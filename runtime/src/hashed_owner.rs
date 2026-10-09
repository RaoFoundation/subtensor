//! Ownership must preserve a protected hotkey's signature scheme.
use crate::{AccountId, Runtime, hashed_auth::compatible_authority};
use frame_support::weights::Weight;

pub struct HashedHotkeyOwnerPolicy;

impl pallet_subtensor::HotkeyOwnerPolicy<AccountId> for HashedHotkeyOwnerPolicy {
    fn allows_owner(coldkey: &AccountId, hotkey: &AccountId) -> bool {
        compatible_authority(hotkey, coldkey)
    }

    fn allows_coldkey_swap(old: &AccountId, new: &AccountId) -> bool {
        // This preserves all existing owner relationships without enumerating
        // their hotkeys. Registration also rejects a weaker preexisting owner.
        compatible_authority(old, new)
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
    fn ownership_swaps_and_proxies_share_the_scheme_compatibility_matrix() {
        use frame_support::traits::Contains;
        use subtensor_hashed::Scheme;
        sp_io::TestExternalities::default().execute_with(|| {
            let accounts = [
                AccountId::new([1; 32]),
                AccountId::new([2; 32]),
                AccountId::new([3; 32]),
            ];
            for (account, scheme) in accounts[1..].iter().zip([Scheme::Sr25519, Scheme::MlDsa65]) {
                pallet_hashed_accounts::Accounts::<Runtime>::insert(
                    account,
                    pallet_hashed_accounts::AccountRecord {
                        descriptor: subtensor_hashed::Descriptor {
                            version: 1,
                            scheme,
                            initial_commitment: [7; 32],
                        },
                        generation: 0,
                        commitment: [7; 32],
                    },
                );
            }
            for (real, allowed) in accounts.iter().zip([
                [true, true, true],
                [false, true, true],
                [false, false, true],
            ]) {
                for (authority, expected) in accounts.iter().zip(allowed) {
                    assert_eq!(
                        HashedHotkeyOwnerPolicy::allows_owner(authority, real),
                        expected
                    );
                    assert_eq!(
                        HashedHotkeyOwnerPolicy::allows_coldkey_swap(real, authority),
                        expected
                    );
                    assert_eq!(
                        crate::hashed_auth::HashedProxyPolicy::contains(&(
                            real.clone(),
                            authority.clone()
                        )),
                        expected
                    );
                }
            }
        });
    }

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
