//! Coldkey ([`IdentitiesV2`]) and subnet ([`SubnetIdentitiesV3`]) identity writes.
//!
//! Extrinsic bodies live in `lib.rs`; these `do_*` helpers perform ownership checks,
//! field-length validation, and storage + event emission.

use super::*;
use frame_support::ensure;
use frame_system::ensure_signed;
use sp_std::vec::Vec;
use subtensor_runtime_common::NetUid;

impl<T: Config> Pallet<T> {
    /// Set or replace the caller's coldkey identity in [`IdentitiesV2`].
    ///
    /// Requires at least one owned hotkey registered on any subnet. Field bytes are
    /// validated by [`Self::is_valid_identity`] before insert; emits [`Event::ChainIdentitySet`].
    pub fn do_set_identity(
        origin: OriginFor<T>,
        name: Vec<u8>,
        url: Vec<u8>,
        github_repo: Vec<u8>,
        image: Vec<u8>,
        discord: Vec<u8>,
        description: Vec<u8>,
        additional: Vec<u8>,
    ) -> dispatch::DispatchResult {
        // Ensure the call is signed and get the signer's (coldkey) account
        let coldkey = ensure_signed(origin)?;

        // Retrieve all hotkeys associated with this coldkey
        let hotkeys: Vec<T::AccountId> = OwnedHotkeys::<T>::get(coldkey.clone());

        // Ensure that at least one of the associated hotkeys is registered on any network
        ensure!(
            hotkeys
                .iter()
                .any(|hotkey| Self::is_hotkey_registered_on_any_network(hotkey)),
            Error::<T>::HotKeyNotRegisteredInNetwork
        );

        // Create the identity struct with the provided information
        let identity = ChainIdentityOfV2 {
            name,
            url,
            github_repo,
            image,
            discord,
            description,
            additional,
        };

        // Validate the created identity
        ensure!(
            Self::is_valid_identity(&identity),
            Error::<T>::InvalidIdentity
        );

        // Store the validated identity in the blockchain state
        IdentitiesV2::<T>::insert(coldkey.clone(), identity.clone());

        // Log the identity set event
        log::debug!("ChainIdentitySet( coldkey:{:?} ) ", coldkey.clone());

        // Emit an event to notify that an identity has been set
        Self::deposit_event(Event::ChainIdentitySet(coldkey.clone()));

        // Return Ok to indicate successful execution
        Ok(())
    }

    /// Set or replace a subnet's identity in [`SubnetIdentitiesV3`].
    ///
    /// Caller must be [`SubnetOwner`] for `netuid`. Validated by
    /// [`Self::is_valid_subnet_identity`]; emits [`Event::SubnetIdentitySet`].
    pub fn do_set_subnet_identity(
        origin: OriginFor<T>,
        netuid: NetUid,
        subnet_name: Vec<u8>,
        github_repo: Vec<u8>,
        subnet_contact: Vec<u8>,
        subnet_url: Vec<u8>,
        discord: Vec<u8>,
        description: Vec<u8>,
        logo_url: Vec<u8>,
        additional: Vec<u8>,
    ) -> dispatch::DispatchResult {
        // Ensure the call is signed and get the signer's (coldkey) account
        let coldkey = ensure_signed(origin)?;
        ensure!(Self::subnet_exists(netuid), Error::<T>::SubnetNotExists);

        // Ensure that the coldkey owns the subnet
        ensure!(
            Self::get_subnet_owner(netuid) == coldkey,
            Error::<T>::NotSubnetOwner
        );

        // Create the identity struct with the provided information
        let identity: SubnetIdentityOfV3 = SubnetIdentityOfV3 {
            subnet_name,
            github_repo,
            subnet_contact,
            subnet_url,
            discord,
            description,
            logo_url,
            additional,
        };

        // Validate the created identity
        ensure!(
            Self::is_valid_subnet_identity(&identity),
            Error::<T>::InvalidIdentity
        );

        // Store the validated identity in the blockchain state
        SubnetIdentitiesV3::<T>::insert(netuid, identity.clone());

        // Log the identity set event
        log::debug!("SubnetIdentitySet( netuid:{netuid:?} ) ");

        // Emit an event to notify that an identity has been set
        Self::deposit_event(Event::SubnetIdentitySet(netuid));

        // Return Ok to indicate successful execution
        Ok(())
    }

    /// Per-field and aggregate byte limits for [`ChainIdentityOfV2`].
    ///
    /// Individual caps: name/url/github_repo/discord ≤ 256; image/description/additional ≤ 1024.
    /// The aggregate check sums name+url+image+discord+description+additional against 4096 and
    /// deliberately omits `github_repo` from that sum (still enforced by its per-field cap).
    pub fn is_valid_identity(identity: &ChainIdentityOfV2) -> bool {
        let total_length = identity
            .name
            .len()
            .saturating_add(identity.url.len())
            .saturating_add(identity.image.len())
            .saturating_add(identity.discord.len())
            .saturating_add(identity.description.len())
            .saturating_add(identity.additional.len());

        let max_length: usize = 256_usize
            .saturating_add(256)
            .saturating_add(256)
            .saturating_add(1024)
            .saturating_add(256)
            .saturating_add(1024)
            .saturating_add(1024);

        total_length <= max_length
            && identity.name.len() <= 256
            && identity.url.len() <= 256
            && identity.github_repo.len() <= 256
            && identity.image.len() <= 1024
            && identity.discord.len() <= 256
            && identity.description.len() <= 1024
            && identity.additional.len() <= 1024
    }

    /// Per-field and aggregate byte limits for [`SubnetIdentityOfV3`].
    ///
    /// Individual caps: subnet_name/discord ≤ 256; remaining string fields ≤ 1024.
    /// Aggregate check sums only subnet_name+github_repo+subnet_contact against 5632;
    /// other fields are enforced solely by their per-field caps.
    pub fn is_valid_subnet_identity(identity: &SubnetIdentityOfV3) -> bool {
        let total_length = identity
            .subnet_name
            .len()
            .saturating_add(identity.github_repo.len())
            .saturating_add(identity.subnet_contact.len());

        let max_length: usize = 256_usize
            .saturating_add(1024)
            .saturating_add(1024)
            .saturating_add(1024)
            .saturating_add(256)
            .saturating_add(1024)
            .saturating_add(1024)
            .saturating_add(1024);

        total_length <= max_length
            && identity.subnet_name.len() <= 256
            && identity.github_repo.len() <= 1024
            && identity.subnet_contact.len() <= 1024
            && identity.subnet_url.len() <= 1024
            && identity.discord.len() <= 256
            && identity.description.len() <= 1024
            && identity.logo_url.len() <= 1024
            && identity.additional.len() <= 1024
    }
}
