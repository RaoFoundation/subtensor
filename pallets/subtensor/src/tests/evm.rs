#![allow(
    clippy::arithmetic_side_effects,
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::indexing_slicing
)]

use super::mock::*;
use crate::*;
use frame_support::testing_prelude::*;
use sp_core::{H160, Pair, U256, ecdsa, keccak_256};
use sp_io::hashing::blake2_256;
use std::convert::AsRef;

fn public_to_evm_key(pubkey: &ecdsa::Public) -> H160 {
    use libsecp256k1::PublicKey;
    use sp_core::keccak_256;

    let secp_pub = PublicKey::parse_compressed(&pubkey.0).expect("Invalid pubkey");
    let uncompressed = secp_pub.serialize(); // 65 bytes: 0x04 + X + Y
    let hash = keccak_256(&uncompressed[1..]); // drop 0x04
    let mut address = [0u8; 20];
    address.copy_from_slice(&hash[12..]);
    H160::from(address)
}

fn sign_evm_message<M: AsRef<[u8]>>(pair: &ecdsa::Pair, message: M) -> ecdsa::Signature {
    let hash = SubtensorModule::hash_message_eip191(message);
    let mut sig = pair.sign_prehashed(&hash);
    // Adjust the v value to either 27 or 28
    sig.0[64] += 27;
    sig
}

#[test]
fn test_weight_usage() {
    new_test_ext(1).execute_with(|| {
        let write = <Test as frame_system::Config>::DbWeight::get().writes(1);
        assert_eq!(write.ref_time(), 100_000_000);
        assert_eq!(write.proof_size(), 0);
    });
}

#[test]
fn test_associate_evm_key_success() {
    new_test_ext(1).execute_with(|| {
        let netuid = NetUid::from(1);

        let tempo: u16 = 2;
        let modality: u16 = 2;

        add_network(netuid, tempo, modality);
        System::set_block_number(EvmKeyAssociateRateLimit::get());

        let coldkey = U256::from(1);
        let hotkey = U256::from(2);
        let _ = SubtensorModule::create_account_if_non_existent(&coldkey, &hotkey);

        register_ok_neuron(netuid, hotkey, coldkey, 0);

        let pair = ecdsa::Pair::generate().0;
        let public = pair.public();
        let evm_key = public_to_evm_key(&public);
        let block_number = frame_system::Pallet::<Test>::block_number();
        let hashed_block_number = keccak_256(block_number.encode().as_ref());
        let hotkey_bytes = hotkey.encode();

        let mut message = [0u8; 64];
        message[..32].copy_from_slice(hotkey_bytes.as_ref());
        message[32..].copy_from_slice(hashed_block_number.as_ref());
        let signature = sign_evm_message(&pair, message);

        assert_ok!(SubtensorModule::associate_evm_key(
            RuntimeOrigin::signed(hotkey),
            netuid,
            evm_key,
            block_number,
            signature,
        ));

        System::assert_last_event(
            Event::EvmKeyAssociated {
                netuid,
                hotkey,
                evm_key,
                block_associated: block_number,
            }
            .into(),
        );
    });
}

#[test]
fn test_associate_evm_key_different_block_number_success() {
    new_test_ext(100).execute_with(|| {
        let netuid = NetUid::from(1);

        let tempo: u16 = 2;
        let modality: u16 = 2;

        add_network(netuid, tempo, modality);
        System::set_block_number(EvmKeyAssociateRateLimit::get());

        let coldkey = U256::from(1);
        let hotkey = U256::from(2);
        let _ = SubtensorModule::create_account_if_non_existent(&coldkey, &hotkey);

        register_ok_neuron(netuid, hotkey, coldkey, 0);

        let pair = ecdsa::Pair::generate().0;
        let public = pair.public();
        let evm_key = public_to_evm_key(&public);
        let block_number = 99u64;
        let hashed_block_number = keccak_256(block_number.encode().as_ref());
        let hotkey_bytes = hotkey.encode();

        let message = [
            hotkey_bytes.as_ref(),
            <[u8; 32] as AsRef<[u8]>>::as_ref(&hashed_block_number),
        ]
        .concat();
        let signature = sign_evm_message(&pair, message);

        assert_ok!(SubtensorModule::associate_evm_key(
            RuntimeOrigin::signed(hotkey),
            netuid,
            evm_key,
            block_number,
            signature,
        ));

        System::assert_last_event(
            Event::EvmKeyAssociated {
                netuid,
                hotkey,
                evm_key,
                block_associated: frame_system::Pallet::<Test>::block_number(),
            }
            .into(),
        );
    });
}

#[test]
fn test_associate_evm_key_coldkey_does_not_own_hotkey() {
    new_test_ext(1).execute_with(|| {
        let netuid = NetUid::from(1);

        let tempo: u16 = 2;
        let modality: u16 = 2;

        add_network(netuid, tempo, modality);

        let hotkey = U256::from(2);

        let pair = ecdsa::Pair::generate().0;
        let public = pair.public();
        let evm_key = public_to_evm_key(&public);
        let block_number = frame_system::Pallet::<Test>::block_number();
        let hashed_block_number = keccak_256(block_number.encode().as_ref());
        let hotkey_bytes = hotkey.encode();

        let message = [
            hotkey_bytes.as_ref(),
            <[u8; 32] as AsRef<[u8]>>::as_ref(&hashed_block_number),
        ]
        .concat();
        let signature = sign_evm_message(&pair, message);

        assert_err!(
            SubtensorModule::associate_evm_key(
                RuntimeOrigin::signed(hotkey),
                netuid,
                evm_key,
                block_number,
                signature,
            ),
            Error::<Test>::NonAssociatedColdKey
        );
    });
}

#[test]
fn test_associate_evm_key_hotkey_not_registered_in_subnet() {
    new_test_ext(1).execute_with(|| {
        let netuid = NetUid::from(1);

        let tempo: u16 = 2;
        let modality: u16 = 2;

        add_network(netuid, tempo, modality);

        let coldkey = U256::from(1);
        let hotkey = U256::from(2);
        let _ = SubtensorModule::create_account_if_non_existent(&coldkey, &hotkey);

        let pair = ecdsa::Pair::generate().0;
        let public = pair.public();
        let evm_key = public_to_evm_key(&public);
        let block_number = frame_system::Pallet::<Test>::block_number();
        let hashed_block_number = keccak_256(block_number.encode().as_ref());
        let hotkey_bytes = hotkey.encode();

        let message = [
            hotkey_bytes.as_ref(),
            <[u8; 32] as AsRef<[u8]>>::as_ref(&hashed_block_number),
        ]
        .concat();
        let signature = sign_evm_message(&pair, message);

        assert_err!(
            SubtensorModule::associate_evm_key(
                RuntimeOrigin::signed(hotkey),
                netuid,
                evm_key,
                block_number,
                signature,
            ),
            Error::<Test>::HotKeyNotRegisteredInSubNet
        );
    });
}

#[test]
fn test_associate_evm_key_using_wrong_hash_function() {
    new_test_ext(1).execute_with(|| {
        let netuid = NetUid::from(1);

        let tempo: u16 = 2;
        let modality: u16 = 2;

        add_network(netuid, tempo, modality);
        System::set_block_number(EvmKeyAssociateRateLimit::get());

        let coldkey = U256::from(1);
        let hotkey = U256::from(2);
        let _ = SubtensorModule::create_account_if_non_existent(&coldkey, &hotkey);

        register_ok_neuron(netuid, hotkey, coldkey, 0);

        let pair = ecdsa::Pair::generate().0;
        let public = pair.public();
        let evm_key = public_to_evm_key(&public);
        let block_number = frame_system::Pallet::<Test>::block_number();
        let hashed_block_number = keccak_256(block_number.encode().as_ref());
        let hotkey_bytes = hotkey.encode();

        let message = [
            hotkey_bytes.as_ref(),
            <[u8; 32] as AsRef<[u8]>>::as_ref(&hashed_block_number),
        ]
        .concat();
        let hashed_message = blake2_256(message.as_ref());
        let signature = pair.sign_prehashed(&hashed_message);

        assert_err!(
            SubtensorModule::associate_evm_key(
                RuntimeOrigin::signed(hotkey),
                netuid,
                evm_key,
                block_number,
                signature,
            ),
            Error::<Test>::InvalidRecoveredPublicKey
        );
    });
}

#[test]
fn test_associate_evm_key_rate_limit_exceeded() {
    new_test_ext(1).execute_with(|| {
        let netuid = NetUid::from(1);

        let tempo: u16 = 2;
        let modality: u16 = 2;
        add_network(netuid, tempo, modality);
        System::set_block_number(EvmKeyAssociateRateLimit::get());

        let coldkey = U256::from(1);
        let hotkey = U256::from(2);
        let _ = SubtensorModule::create_account_if_non_existent(&coldkey, &hotkey);

        register_ok_neuron(netuid, hotkey, coldkey, 0);

        let pair = ecdsa::Pair::generate().0;
        let public = pair.public();
        let evm_key = public_to_evm_key(&public);
        let block_number = frame_system::Pallet::<Test>::block_number();
        let hashed_block_number = keccak_256(block_number.encode().as_ref());
        let hotkey_bytes = hotkey.encode();

        let message = [
            hotkey_bytes.as_ref(),
            <[u8; 32] as AsRef<[u8]>>::as_ref(&hashed_block_number),
        ]
        .concat();
        let signature = sign_evm_message(&pair, message);

        // First association should succeed
        assert_ok!(SubtensorModule::associate_evm_key(
            RuntimeOrigin::signed(hotkey),
            netuid,
            evm_key,
            block_number,
            signature,
        ));

        System::set_block_number(System::block_number() + 1);
        let block_number = frame_system::Pallet::<Test>::block_number();
        let hashed_block_number = keccak_256(block_number.encode().as_ref());
        let hotkey_bytes = hotkey.encode();
        let message = [
            hotkey_bytes.as_ref(),
            <[u8; 32] as AsRef<[u8]>>::as_ref(&hashed_block_number),
        ]
        .concat();
        let signature = sign_evm_message(&pair, message);

        // Second association should fail due to rate limit
        assert_noop!(
            SubtensorModule::associate_evm_key(
                RuntimeOrigin::signed(hotkey),
                netuid,
                evm_key,
                block_number,
                signature,
            ),
            Error::<Test>::EvmKeyAssociateRateLimitExceeded
        );

        System::set_block_number(System::block_number() + EvmKeyAssociateRateLimit::get());
        let block_number = frame_system::Pallet::<Test>::block_number();
        let hashed_block_number = keccak_256(block_number.encode().as_ref());
        let hotkey_bytes = hotkey.encode();
        let message = [
            hotkey_bytes.as_ref(),
            <[u8; 32] as AsRef<[u8]>>::as_ref(&hashed_block_number),
        ]
        .concat();
        let signature = sign_evm_message(&pair, message);

        assert_ok!(SubtensorModule::associate_evm_key(
            RuntimeOrigin::signed(hotkey),
            netuid,
            evm_key,
            block_number,
            signature,
        ));
    });
}

#[test]
fn test_associate_evm_key_cap_exceeded() {
    new_test_ext(1).execute_with(|| {
        let netuid = NetUid::from(1);

        let tempo: u16 = 2;
        let modality: u16 = 2;
        add_network(netuid, tempo, modality);
        System::set_block_number(EvmKeyAssociateRateLimit::get());

        let coldkey = U256::from(1);
        let hotkey = U256::from(2);
        let _ = SubtensorModule::create_account_if_non_existent(&coldkey, &hotkey);
        register_ok_neuron(netuid, hotkey, coldkey, 0);
        let uid = SubtensorModule::get_uid_for_net_and_hotkey(netuid, &hotkey).unwrap();

        let pair = ecdsa::Pair::generate().0;
        let evm_key = public_to_evm_key(&pair.public());

        // Fill the reverse-index bucket for `evm_key` to capacity with other UIDs.
        for i in 0..MAX_ASSOCIATED_UIDS_PER_EVM_ADDRESS as u16 {
            SubtensorModule::set_associated_evm_address(netuid, uid + 1 + i, evm_key, 1);
        }
        assert_eq!(
            AssociatedUidsByEvmAddress::<Test>::get(netuid, evm_key).len(),
            MAX_ASSOCIATED_UIDS_PER_EVM_ADDRESS as usize
        );

        // A valid association for the real neuron's UID must be rejected: it would need a
        // brand-new slot in an already-full bucket.
        let block_number = frame_system::Pallet::<Test>::block_number();
        let hashed_block_number = keccak_256(block_number.encode().as_ref());
        let hotkey_bytes = hotkey.encode();
        let message = [
            hotkey_bytes.as_ref(),
            <[u8; 32] as AsRef<[u8]>>::as_ref(&hashed_block_number),
        ]
        .concat();
        let signature = sign_evm_message(&pair, message);

        assert_noop!(
            SubtensorModule::associate_evm_key(
                RuntimeOrigin::signed(hotkey),
                netuid,
                evm_key,
                block_number,
                signature,
            ),
            Error::<Test>::EvmKeyAssociationLimitExceeded
        );
    });
}

#[test]
fn test_evm_address_index_capacity_allows_refresh_when_full() {
    new_test_ext(1).execute_with(|| {
        let netuid = NetUid::from(1);
        let evm_key = H160::from_slice(&[7u8; 20]);

        // Fill the bucket to capacity with distinct UIDs 0..MAX.
        for uid in 0..MAX_ASSOCIATED_UIDS_PER_EVM_ADDRESS as u16 {
            SubtensorModule::set_associated_evm_address(netuid, uid, evm_key, 1);
        }

        // A UID already tracked by the full bucket may be re-associated (e.g. block refresh):
        // it consumes no new slot.
        let tracked_uid = 0u16;
        assert_ok!(SubtensorModule::ensure_evm_address_index_capacity(
            netuid,
            tracked_uid,
            evm_key
        ));

        // The refresh updates the stored block in place without growing the bucket.
        SubtensorModule::set_associated_evm_address(netuid, tracked_uid, evm_key, 42);
        let bucket = AssociatedUidsByEvmAddress::<Test>::get(netuid, evm_key);
        assert_eq!(bucket.len(), MAX_ASSOCIATED_UIDS_PER_EVM_ADDRESS as usize);
        assert_eq!(
            bucket.iter().find(|(u, _)| *u == tracked_uid).unwrap().1,
            42
        );

        // A brand-new UID is rejected once the bucket is full.
        assert_err!(
            SubtensorModule::ensure_evm_address_index_capacity(
                netuid,
                MAX_ASSOCIATED_UIDS_PER_EVM_ADDRESS as u16,
                evm_key
            ),
            Error::<Test>::EvmKeyAssociationLimitExceeded
        );
    });
}

#[test]
fn test_evm_address_index_capacity_rejects_switch_onto_full_address() {
    new_test_ext(1).execute_with(|| {
        let netuid = NetUid::from(1);
        let addr_a = H160::from_slice(&[0xaa; 20]);
        let addr_b = H160::from_slice(&[0xbb; 20]);

        // UID 100 currently associated to addr_a.
        let uid = 100u16;
        SubtensorModule::set_associated_evm_address(netuid, uid, addr_a, 1);

        // addr_b is filled to capacity with other UIDs.
        for u in 0..MAX_ASSOCIATED_UIDS_PER_EVM_ADDRESS as u16 {
            SubtensorModule::set_associated_evm_address(netuid, u, addr_b, 1);
        }

        // Moving UID 100 onto the full addr_b must be rejected...
        assert_err!(
            SubtensorModule::ensure_evm_address_index_capacity(netuid, uid, addr_b),
            Error::<Test>::EvmKeyAssociationLimitExceeded
        );

        // ...leaving UID 100 still associated to addr_a.
        assert_eq!(
            AssociatedEvmAddress::<Test>::get(netuid, uid).map(|(k, _)| k),
            Some(addr_a)
        );
    });
}

#[test]
fn test_associate_evm_key_uid_not_found() {
    new_test_ext(1).execute_with(|| {
        let netuid = NetUid::from(1);

        let tempo: u16 = 2;
        let modality: u16 = 2;

        add_network(netuid, tempo, modality);

        let hotkey = U256::from(2);

        let pair = ecdsa::Pair::generate().0;
        let public = pair.public();
        let evm_key = public_to_evm_key(&public);
        let block_number = frame_system::Pallet::<Test>::block_number();
        let hashed_block_number = keccak_256(block_number.encode().as_ref());
        let hotkey_bytes = hotkey.encode();

        let message = [
            hotkey_bytes.as_ref(),
            <[u8; 32] as AsRef<[u8]>>::as_ref(&hashed_block_number),
        ]
        .concat();
        let signature = sign_evm_message(&pair, message);

        assert_noop!(
            SubtensorModule::associate_evm_key(
                RuntimeOrigin::signed(hotkey),
                netuid,
                evm_key,
                block_number,
                signature,
            ),
            Error::<Test>::NonAssociatedColdKey
        );
    });
}
