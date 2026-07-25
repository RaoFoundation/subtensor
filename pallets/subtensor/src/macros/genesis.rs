use frame_support::pallet_macros::pallet_section;

/// [`pallet_section`] defining genesis build for the subtensor pallet (imported via [`import_section`]).
///
/// Seeds root (`NetUid::ROOT`) and a bootstrap dynamic subnet (netuid 1) used by local/dev chains.
/// Production vs fast-runtime owner keys are selected via `prod_or_fast!`.
#[pallet_section]
mod genesis {
    use sp_core::crypto::Pair;
    use sp_core::sr25519::Pair as Sr25519Pair;

    /// Applies [`GenesisConfig`] at chain start: issuance, optional `start_call` delay, root network, and netuid-1 pool.
    #[pallet::genesis_build]
    impl<T: Config> BuildGenesisConfig for GenesisConfig<T> {
        fn build(&self) {
            // Alice's public key
            let alice_bytes = sp_keyring::Sr25519Keyring::Alice.public();

            // Create Alice's hotkey from seed string
            let pair = Sr25519Pair::from_string("//Alice_hk", None)
                .expect("Alice hotkey pair should be valid");
            let alice_hk_bytes = pair.public().0;

            let alice_account =
                T::AccountId::decode(&mut &alice_bytes[..]).expect("Alice account should decode");
            let alice_hk_account = T::AccountId::decode(&mut &alice_hk_bytes[..])
                .expect("Alice hotkey account should decode");

            // Prod: `DefaultSubnetOwner`; fast/dev: Alice coldkey + `//Alice_hk` hotkey.
            let subnet_root_owner = prod_or_fast!(DefaultSubnetOwner::<T>::get(), alice_account);
            let subnet_root_owner_hotkey =
                prod_or_fast!(DefaultSubnetOwner::<T>::get(), alice_hk_account);

            // Align SubtensorModule issuance with the balances-pallet genesis figure (rao).
            TotalIssuance::<T>::put(self.balances_issuance);

            // Optional override for blocks before `start_call` may enable emissions.
            if let Some(delay) = self.start_call_delay {
                StartCallDelay::<T>::put(delay);
            }

            // --- Root network (netuid 0): senate-sized, open registration, no weight floor. ---
            NetworksAdded::<T>::insert(NetUid::ROOT, true);

            // Increment the number of total networks.
            TotalNetworks::<T>::mutate(|n| *n = n.saturating_add(1));

            // Set the root network owner.
            SubnetOwner::<T>::insert(NetUid::ROOT, subnet_root_owner);

            // Set the root network owner hotkey.
            SubnetOwnerHotkey::<T>::insert(NetUid::ROOT, subnet_root_owner_hotkey);

            // Set the number of validators to 1.
            SubnetworkN::<T>::insert(NetUid::ROOT, 0);

            // Set the maximum number to the number of senate members.
            MaxAllowedUids::<T>::insert(NetUid::ROOT, 64u16);

            // Set the maximum number to the number of validators to all members.
            MaxAllowedValidators::<T>::insert(NetUid::ROOT, 64u16);

            // Set the min allowed weights to zero, no weights restrictions.
            MinAllowedWeights::<T>::insert(NetUid::ROOT, 0);

            // Add default root tempo.
            Tempo::<T>::insert(NetUid::ROOT, 100);

            // Set the root network as open.
            NetworkRegistrationAllowed::<T>::insert(NetUid::ROOT, true);

            // Set target registrations for validators as 1 per block.
            TargetRegistrationsPerInterval::<T>::insert(NetUid::ROOT, 1);

            // Set token symbol for root
            TokenSymbol::<T>::insert(
                NetUid::ROOT,
                Pallet::<T>::get_symbol_for_subnet(NetUid::ROOT),
            );

            // --- Bootstrap subnet netuid 1: dynamic mechanism, seeded AMM reserves, uid 0 = DefaultAccount. ---
            let netuid = NetUid::from(1);
            let hotkey = DefaultAccount::<T>::get();
            SubnetMechanism::<T>::insert(netuid, 1); // Make dynamic.
            Owner::<T>::insert(hotkey.clone(), hotkey.clone());
            SubnetAlphaIn::<T>::insert(netuid, AlphaBalance::from(10_000_000_000_u64));
            SubnetTAO::<T>::insert(netuid, TaoBalance::from(10_000_000_000_u64));
            NetworksAdded::<T>::insert(netuid, true);
            TotalNetworks::<T>::mutate(|n| *n = n.saturating_add(1));
            SubnetworkN::<T>::insert(netuid, 0);
            MaxAllowedUids::<T>::insert(netuid, 256u16);
            MaxAllowedValidators::<T>::insert(netuid, 64u16);
            MinAllowedWeights::<T>::insert(netuid, 0);
            Tempo::<T>::insert(netuid, 100);
            NetworkRegistrationAllowed::<T>::insert(netuid, true);
            SubnetOwner::<T>::insert(netuid, hotkey.clone());
            SubnetLocked::<T>::insert(netuid, TaoBalance::from(1));
            LargestLocked::<T>::insert(netuid, 1);
            AlphaV2::<T>::insert(
                // Lock the initial funds making this key the owner.
                (hotkey.clone(), hotkey.clone(), netuid),
                SafeFloat::from(1_000_000_000),
            );
            TotalHotkeyAlpha::<T>::insert(
                hotkey.clone(),
                netuid,
                AlphaBalance::from(1_000_000_000),
            );
            TotalHotkeySharesV2::<T>::insert(
                hotkey.clone(),
                netuid,
                SafeFloat::from(1_000_000_000),
            );
            SubnetAlphaOut::<T>::insert(netuid, AlphaBalance::from(1_000_000_000));
            let mut staking_hotkeys = StakingHotkeys::<T>::get(hotkey.clone());
            if !staking_hotkeys.contains(&hotkey) {
                staking_hotkeys.push(hotkey.clone());
                StakingHotkeys::<T>::insert(hotkey.clone(), staking_hotkeys.clone());
            }

            let block_number = Pallet::<T>::get_current_block_as_u64();

            SubnetworkN::<T>::insert(netuid, 1);
            Active::<T>::mutate(netuid, |v| v.push(true));
            Emission::<T>::mutate(netuid, |v| v.push(0.into()));
            Consensus::<T>::mutate(netuid, |v| v.push(PerU16::zero()));
            Incentive::<T>::mutate(NetUidStorageIndex::from(netuid), |v| v.push(PerU16::zero()));
            Dividends::<T>::mutate(netuid, |v| v.push(PerU16::zero()));
            LastUpdate::<T>::mutate(NetUidStorageIndex::from(netuid), |v| v.push(block_number));
            ValidatorTrust::<T>::mutate(netuid, |v| v.push(PerU16::zero()));
            ValidatorPermit::<T>::mutate(netuid, |v| v.push(false));
            Keys::<T>::insert(netuid, 0, hotkey.clone()); // Make hotkey - uid association.
            Uids::<T>::insert(netuid, hotkey.clone(), 0); // Make uid - hotkey association.
            BlockAtRegistration::<T>::insert(netuid, 0, block_number); // Fill block at registration.
            IsNetworkMember::<T>::insert(hotkey.clone(), netuid, true); // Fill network is member.
            TokenSymbol::<T>::insert(netuid, Pallet::<T>::get_symbol_for_subnet(netuid));
        }
    }
}
