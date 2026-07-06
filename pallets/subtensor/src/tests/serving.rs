#![allow(clippy::expect_used, clippy::unwrap_used)]
use super::mock::*;

use crate::Error;
use crate::*;
use frame_support::assert_noop;
use frame_support::{
    assert_ok,
    dispatch::{DispatchClass, DispatchInfo, GetDispatchInfo, Pays},
};
use frame_system::Config;
use sp_core::U256;

mod test {
    use std::net::{Ipv4Addr, Ipv6Addr};

    // Generates an ipv6 address based on 8 ipv6 words and returns it as u128
    #[allow(clippy::too_many_arguments)]
    pub fn ipv6(a: u16, b: u16, c: u16, d: u16, e: u16, f: u16, g: u16, h: u16) -> u128 {
        Ipv6Addr::new(a, b, c, d, e, f, g, h).into()
    }

    // Generate an ipv4 address based on 4 bytes and returns the corresponding u128, so it can be fed
    // to the module::subscribe() function
    pub fn ipv4(a: u8, b: u8, c: u8, d: u8) -> u128 {
        let ipv4: Ipv4Addr = Ipv4Addr::new(a, b, c, d);
        let integer: u32 = ipv4.into();
        u128::from(integer)
    }
}

#[test]
fn test_serving_subscribe_ok_dispatch_info_ok() {
    new_test_ext(1).execute_with(|| {
        let netuid = NetUid::from(1);
        let version: u32 = 2;
        let ip: u128 = 1676056785;
        let port: u16 = 128;
        let ip_type: u8 = 4;
        let protocol: u8 = 0;
        let placeholder1: u8 = 0;
        let placeholder2: u8 = 0;
        let call = RuntimeCall::SubtensorModule(SubtensorCall::serve_axon {
            netuid,
            version,
            ip,
            port,
            ip_type,
            protocol,
            placeholder1,
            placeholder2,
        });
        let di = call.get_dispatch_info();
        assert!(di.total_weight().ref_time() > 0);
        assert_eq!(di.class, DispatchClass::Normal);
        assert_eq!(di.pays_fee, Pays::No);
    });
}

#[test]
fn test_serving_ok() {
    new_test_ext(1).execute_with(|| {
        let hotkey_account_id = U256::from(1);
        let netuid = NetUid::from(1);
        let tempo: u16 = 13;
        let version: u32 = 2;
        let ip: u128 = 1676056785;
        let port: u16 = 128;
        let ip_type: u8 = 4;
        let modality: u16 = 0;
        let protocol: u8 = 0;
        let placeholder1: u8 = 0;
        let placeholder2: u8 = 0;
        add_network(netuid, tempo, modality);
        register_ok_neuron(netuid, hotkey_account_id, U256::from(66), 0);
        assert_ok!(SubtensorModule::serve_axon(
            <<Test as Config>::RuntimeOrigin>::signed(hotkey_account_id),
            netuid,
            version,
            ip,
            port,
            ip_type,
            protocol,
            placeholder1,
            placeholder2
        ));
        let neuron = SubtensorModule::get_axon_info(netuid, &hotkey_account_id);
        assert_eq!(neuron.ip, ip);
        assert_eq!(neuron.version, version);
        assert_eq!(neuron.port, port);
        assert_eq!(neuron.ip_type, ip_type);
        assert_eq!(neuron.protocol, protocol);
        assert_eq!(neuron.placeholder1, placeholder1);
        assert_eq!(neuron.placeholder2, placeholder2);
    });
}

#[test]
fn test_serving_tls_ok() {
    new_test_ext(1).execute_with(|| {
        let hotkey_account_id = U256::from(1);
        let netuid = NetUid::from(1);
        let tempo: u16 = 13;
        let version: u32 = 2;
        let ip: u128 = 1676056785;
        let port: u16 = 128;
        let ip_type: u8 = 4;
        let modality: u16 = 0;
        let protocol: u8 = 0;
        let placeholder1: u8 = 0;
        let placeholder2: u8 = 0;
        let certificate: Vec<u8> = "CERT".as_bytes().to_vec();
        add_network(netuid, tempo, modality);
        register_ok_neuron(netuid, hotkey_account_id, U256::from(66), 0);
        assert_ok!(SubtensorModule::serve_axon_tls(
            <<Test as Config>::RuntimeOrigin>::signed(hotkey_account_id),
            netuid,
            version,
            ip,
            port,
            ip_type,
            protocol,
            placeholder1,
            placeholder2,
            certificate.clone()
        ));

        let stored_certificate = NeuronCertificates::<Test>::get(netuid, hotkey_account_id)
            .expect("Certificate should exist");
        assert_eq!(
            stored_certificate.public_key.clone().into_inner(),
            certificate.get(1..).expect("Certificate should exist")
        );
        let new_certificate = "UPDATED_CERT".as_bytes().to_vec();
        assert_ok!(SubtensorModule::serve_axon_tls(
            <<Test as Config>::RuntimeOrigin>::signed(hotkey_account_id),
            netuid,
            version,
            ip,
            port,
            ip_type,
            protocol,
            placeholder1,
            placeholder2,
            new_certificate.clone()
        ));
        let stored_certificate = NeuronCertificates::<Test>::get(netuid, hotkey_account_id)
            .expect("Certificate should exist");
        assert_eq!(
            stored_certificate.public_key.clone().into_inner(),
            new_certificate.get(1..).expect("Certificate should exist")
        );
    });
}

#[test]
fn test_serving_set_metadata_update() {
    new_test_ext(1).execute_with(|| {
        let hotkey_account_id = U256::from(1);
        let netuid = NetUid::from(1);
        let tempo: u16 = 13;
        let version: u32 = 2;
        let ip: u128 = 1676056785;
        let port: u16 = 128;
        let ip_type: u8 = 4;
        let modality: u16 = 0;
        let protocol: u8 = 0;
        let placeholder1: u8 = 0;
        let placeholder2: u8 = 0;
        add_network(netuid, tempo, modality);
        register_ok_neuron(netuid, hotkey_account_id, U256::from(66), 0);
        assert_ok!(SubtensorModule::serve_axon(
            <<Test as Config>::RuntimeOrigin>::signed(hotkey_account_id),
            netuid,
            version,
            ip,
            port,
            ip_type,
            protocol,
            placeholder1,
            placeholder2
        ));
        let neuron = SubtensorModule::get_axon_info(netuid, &hotkey_account_id);
        assert_eq!(neuron.ip, ip);
        assert_eq!(neuron.version, version);
        assert_eq!(neuron.port, port);
        assert_eq!(neuron.ip_type, ip_type);
        assert_eq!(neuron.protocol, protocol);
        assert_eq!(neuron.placeholder1, placeholder1);
        assert_eq!(neuron.placeholder2, placeholder2);
        let version2: u32 = version + 1;
        let ip2: u128 = ip + 1;
        let port2: u16 = port + 1;
        let ip_type2: u8 = 6;
        let protocol2: u8 = protocol + 1;
        let placeholder12: u8 = placeholder1 + 1;
        let placeholder22: u8 = placeholder2 + 1;
        assert_ok!(SubtensorModule::serve_axon(
            <<Test as Config>::RuntimeOrigin>::signed(hotkey_account_id),
            netuid,
            version2,
            ip2,
            port2,
            ip_type2,
            protocol2,
            placeholder12,
            placeholder22
        ));
        let neuron = SubtensorModule::get_axon_info(netuid, &hotkey_account_id);
        assert_eq!(neuron.ip, ip2);
        assert_eq!(neuron.version, version2);
        assert_eq!(neuron.port, port2);
        assert_eq!(neuron.ip_type, ip_type2);
        assert_eq!(neuron.protocol, protocol2);
        assert_eq!(neuron.placeholder1, placeholder12);
        assert_eq!(neuron.placeholder2, placeholder22);
    });
}

#[test]
fn test_axon_serving_rate_limit_exceeded() {
    new_test_ext(1).execute_with(|| {
        let hotkey_account_id = U256::from(1);
        let netuid = NetUid::from(1);
        let tempo: u16 = 13;
        let version: u32 = 2;
        let ip: u128 = 1676056785;
        let port: u16 = 128;
        let ip_type: u8 = 4;
        let modality: u16 = 0;
        let protocol: u8 = 0;
        let placeholder1: u8 = 0;
        let placeholder2: u8 = 0;
        add_network(netuid, tempo, modality);
        register_ok_neuron(netuid, hotkey_account_id, U256::from(66), 0);
        run_to_block(1); // Go to block 1
        // No issue on multiple
        assert_ok!(SubtensorModule::serve_axon(
            <<Test as Config>::RuntimeOrigin>::signed(hotkey_account_id),
            netuid,
            version,
            ip,
            port,
            ip_type,
            protocol,
            placeholder1,
            placeholder2
        ));
        assert_ok!(SubtensorModule::serve_axon(
            <<Test as Config>::RuntimeOrigin>::signed(hotkey_account_id),
            netuid,
            version,
            ip,
            port,
            ip_type,
            protocol,
            placeholder1,
            placeholder2
        ));
        assert_ok!(SubtensorModule::serve_axon(
            <<Test as Config>::RuntimeOrigin>::signed(hotkey_account_id),
            netuid,
            version,
            ip,
            port,
            ip_type,
            protocol,
            placeholder1,
            placeholder2
        ));
        assert_ok!(SubtensorModule::serve_axon(
            <<Test as Config>::RuntimeOrigin>::signed(hotkey_account_id),
            netuid,
            version,
            ip,
            port,
            ip_type,
            protocol,
            placeholder1,
            placeholder2
        ));
        SubtensorModule::set_serving_rate_limit(netuid, 2);
        run_to_block(2); // Go to block 2
        // Needs to be 2 blocks apart, we are only 1 block apart
        assert_eq!(
            SubtensorModule::serve_axon(
                <<Test as Config>::RuntimeOrigin>::signed(hotkey_account_id),
                netuid,
                version,
                ip,
                port,
                ip_type,
                protocol,
                placeholder1,
                placeholder2
            ),
            Err(Error::<Test>::ServingRateLimitExceeded.into())
        );
    });
}

#[test]
fn test_axon_invalid_port() {
    new_test_ext(1).execute_with(|| {
        let hotkey_account_id = U256::from(1);
        let netuid = NetUid::from(1);
        let tempo: u16 = 13;
        let version: u32 = 2;
        let ip: u128 = 1676056785;
        let port: u16 = 0;
        let ip_type: u8 = 4;
        let modality: u16 = 0;
        let protocol: u8 = 0;
        let placeholder1: u8 = 0;
        let placeholder2: u8 = 0;
        add_network(netuid, tempo, modality);
        register_ok_neuron(netuid, hotkey_account_id, U256::from(66), 0);
        run_to_block(1); // Go to block 1
        assert_eq!(
            SubtensorModule::serve_axon(
                <<Test as Config>::RuntimeOrigin>::signed(hotkey_account_id),
                netuid,
                version,
                ip,
                port,
                ip_type,
                protocol,
                placeholder1,
                placeholder2
            ),
            Err(Error::<Test>::InvalidPort.into())
        );
    });
}

#[test]
fn test_prometheus_serving_subscribe_ok_dispatch_info_ok() {
    new_test_ext(1).execute_with(|| {
        let netuid = NetUid::from(1);
        let version: u32 = 2;
        let ip: u128 = 1676056785;
        let port: u16 = 128;
        let ip_type: u8 = 4;
        let call = RuntimeCall::SubtensorModule(SubtensorCall::serve_prometheus {
            netuid,
            version,
            ip,
            port,
            ip_type,
        });
        let di = call.get_dispatch_info();
        assert!(di.total_weight().ref_time() > 0);
        assert_eq!(di.class, DispatchClass::Normal);
        assert_eq!(di.pays_fee, Pays::No);
    });
}

#[test]
fn test_prometheus_serving_ok() {
    new_test_ext(1).execute_with(|| {
        let hotkey_account_id = U256::from(1);
        let netuid = NetUid::from(1);
        let tempo: u16 = 13;
        let version: u32 = 2;
        let ip: u128 = 1676056785;
        let port: u16 = 128;
        let ip_type: u8 = 4;
        let modality: u16 = 0;
        add_network(netuid, tempo, modality);
        register_ok_neuron(netuid, hotkey_account_id, U256::from(66), 0);
        assert_ok!(SubtensorModule::serve_prometheus(
            <<Test as Config>::RuntimeOrigin>::signed(hotkey_account_id),
            netuid,
            version,
            ip,
            port,
            ip_type
        ));
        let neuron = SubtensorModule::get_prometheus_info(netuid, &hotkey_account_id);
        assert_eq!(neuron.ip, ip);
        assert_eq!(neuron.version, version);
        assert_eq!(neuron.port, port);
        assert_eq!(neuron.ip_type, ip_type);
    });
}

#[test]
fn test_prometheus_serving_set_metadata_update() {
    new_test_ext(1).execute_with(|| {
        let hotkey_account_id = U256::from(1);
        let netuid = NetUid::from(1);
        let tempo: u16 = 13;
        let version: u32 = 2;
        let ip: u128 = 1676056785;
        let port: u16 = 128;
        let ip_type: u8 = 4;
        let modality: u16 = 0;
        add_network(netuid, tempo, modality);
        register_ok_neuron(netuid, hotkey_account_id, U256::from(66), 0);
        assert_ok!(SubtensorModule::serve_prometheus(
            <<Test as Config>::RuntimeOrigin>::signed(hotkey_account_id),
            netuid,
            version,
            ip,
            port,
            ip_type
        ));
        let neuron = SubtensorModule::get_prometheus_info(netuid, &hotkey_account_id);
        assert_eq!(neuron.ip, ip);
        assert_eq!(neuron.version, version);
        assert_eq!(neuron.port, port);
        assert_eq!(neuron.ip_type, ip_type);
        let version2: u32 = version + 1;
        let ip2: u128 = ip + 1;
        let port2: u16 = port + 1;
        let ip_type2: u8 = 6;
        assert_ok!(SubtensorModule::serve_prometheus(
            <<Test as Config>::RuntimeOrigin>::signed(hotkey_account_id),
            netuid,
            version2,
            ip2,
            port2,
            ip_type2
        ));
        let neuron = SubtensorModule::get_prometheus_info(netuid, &hotkey_account_id);
        assert_eq!(neuron.ip, ip2);
        assert_eq!(neuron.version, version2);
        assert_eq!(neuron.port, port2);
        assert_eq!(neuron.ip_type, ip_type2);
    });
}

#[test]
fn test_prometheus_serving_rate_limit_exceeded() {
    new_test_ext(1).execute_with(|| {
        let hotkey_account_id = U256::from(1);
        let netuid = NetUid::from(1);
        let tempo: u16 = 13;
        let version: u32 = 2;
        let ip: u128 = 1676056785;
        let port: u16 = 128;
        let ip_type: u8 = 4;
        let modality: u16 = 0;
        add_network(netuid, tempo, modality);
        register_ok_neuron(netuid, hotkey_account_id, U256::from(66), 0);
        run_to_block(1); // Go to block 1
        // No issue on multiple
        assert_ok!(SubtensorModule::serve_prometheus(
            <<Test as Config>::RuntimeOrigin>::signed(hotkey_account_id),
            netuid,
            version,
            ip,
            port,
            ip_type
        ));
        assert_ok!(SubtensorModule::serve_prometheus(
            <<Test as Config>::RuntimeOrigin>::signed(hotkey_account_id),
            netuid,
            version,
            ip,
            port,
            ip_type
        ));
        assert_ok!(SubtensorModule::serve_prometheus(
            <<Test as Config>::RuntimeOrigin>::signed(hotkey_account_id),
            netuid,
            version,
            ip,
            port,
            ip_type
        ));
        assert_ok!(SubtensorModule::serve_prometheus(
            <<Test as Config>::RuntimeOrigin>::signed(hotkey_account_id),
            netuid,
            version,
            ip,
            port,
            ip_type
        ));
        SubtensorModule::set_serving_rate_limit(netuid, 1);
        // Same block, need 1 block to pass
        assert_eq!(
            SubtensorModule::serve_prometheus(
                <<Test as Config>::RuntimeOrigin>::signed(hotkey_account_id),
                netuid,
                version,
                ip,
                port,
                ip_type
            ),
            Err(Error::<Test>::ServingRateLimitExceeded.into())
        );
    });
}

#[test]
fn test_prometheus_invalid_port() {
    new_test_ext(1).execute_with(|| {
        let hotkey_account_id = U256::from(1);
        let netuid = NetUid::from(1);
        let tempo: u16 = 13;
        let version: u32 = 2;
        let ip: u128 = 1676056785;
        let port: u16 = 0;
        let ip_type: u8 = 4;
        let modality: u16 = 0;
        add_network(netuid, tempo, modality);
        register_ok_neuron(netuid, hotkey_account_id, U256::from(66), 0);
        run_to_block(1); // Go to block 1
        assert_eq!(
            SubtensorModule::serve_prometheus(
                <<Test as Config>::RuntimeOrigin>::signed(hotkey_account_id),
                netuid,
                version,
                ip,
                port,
                ip_type
            ),
            Err(Error::<Test>::InvalidPort.into())
        );
    });
}

#[test]
fn test_serving_is_valid_ip_type_ok_ipv4() {
    new_test_ext(1).execute_with(|| {
        assert!(SubtensorModule::is_valid_ip_type(4));
    });
}

#[test]
fn test_serving_is_valid_ip_type_ok_ipv6() {
    new_test_ext(1).execute_with(|| {
        assert!(SubtensorModule::is_valid_ip_type(6));
    });
}

#[test]
fn test_serving_is_valid_ip_type_nok() {
    new_test_ext(1).execute_with(|| {
        assert!(!SubtensorModule::is_valid_ip_type(10));
    });
}

#[test]
fn test_serving_is_valid_ip_address_ipv4() {
    new_test_ext(1).execute_with(|| {
        assert!(SubtensorModule::is_valid_ip_address(
            4,
            test::ipv4(8, 8, 8, 8),
            false
        ));

        assert!(SubtensorModule::is_valid_ip_address(
            4,
            test::ipv4(0, 0, 0, 0),
            true
        ));
    });
}

#[test]
fn test_serving_is_valid_ip_address_ipv6() {
    new_test_ext(1).execute_with(|| {
        assert!(SubtensorModule::is_valid_ip_address(
            6,
            test::ipv6(1, 2, 3, 4, 5, 6, 7, 8),
            false
        ));
        assert!(SubtensorModule::is_valid_ip_address(
            6,
            test::ipv6(1, 2, 3, 4, 5, 6, 7, 8),
            false
        ));
        assert!(SubtensorModule::is_valid_ip_address(
            6,
            test::ipv6(0, 0, 0, 0, 0, 0, 0, 0),
            true
        ));
    });
}

#[test]
fn test_serving_is_invalid_ipv4_address() {
    new_test_ext(1).execute_with(|| {
        assert!(!SubtensorModule::is_valid_ip_address(
            4,
            test::ipv4(0, 0, 0, 0),
            false
        ));
        assert!(!SubtensorModule::is_valid_ip_address(
            4,
            test::ipv4(255, 255, 255, 255),
            false
        ));
        assert!(!SubtensorModule::is_valid_ip_address(
            4,
            test::ipv4(127, 0, 0, 1),
            false
        ));
        assert!(!SubtensorModule::is_valid_ip_address(
            4,
            test::ipv6(0xffff, 2, 3, 4, 5, 6, 7, 8),
            false
        ));
        assert!(!SubtensorModule::is_valid_ip_address(
            4,
            test::ipv4(255, 255, 255, 255),
            true
        ));
        assert!(!SubtensorModule::is_valid_ip_address(
            4,
            test::ipv4(127, 0, 0, 1),
            true
        ));
        assert!(!SubtensorModule::is_valid_ip_address(
            4,
            test::ipv6(0xffff, 2, 3, 4, 5, 6, 7, 8),
            true
        ));
    });
}

#[test]
fn test_serving_is_invalid_ipv6_address() {
    new_test_ext(1).execute_with(|| {
        assert!(!SubtensorModule::is_valid_ip_address(
            6,
            test::ipv6(0, 0, 0, 0, 0, 0, 0, 0),
            false
        ));
        assert!(!SubtensorModule::is_valid_ip_address(
            4,
            test::ipv6(
                0xffff, 0xffff, 0xffff, 0xffff, 0xffff, 0xffff, 0xffff, 0xffff
            ),
            false
        ));
        assert!(!SubtensorModule::is_valid_ip_address(
            4,
            test::ipv6(
                0xffff, 0xffff, 0xffff, 0xffff, 0xffff, 0xffff, 0xffff, 0xffff
            ),
            true
        ));
    });
}

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --test serving -- test_do_set_identity --exact --nocapture
#[test]
fn test_do_set_identity() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1);
        let hotkey = U256::from(2);
        let netuid = NetUid::from(1);

        // Register a hotkey for the coldkey
        add_network(netuid, 13, 0);
        register_ok_neuron(netuid, hotkey, coldkey, 0);

        // Prepare identity data
        let name = b"Alice".to_vec();
        let url = b"https://alice.com".to_vec();
        let git = b"https://github.com".to_vec();
        let image = b"alice.jpg".to_vec();
        let discord = b"alice#1234".to_vec();
        let description = b"Alice's identity".to_vec();
        let additional = b"Additional info".to_vec();

        // Set identity
        assert_ok!(SubtensorModule::do_set_identity(
            <<Test as Config>::RuntimeOrigin>::signed(coldkey),
            name.clone(),
            url.clone(),
            git.clone(),
            image.clone(),
            discord.clone(),
            description.clone(),
            additional.clone()
        ));

        // Check if identity is set correctly
        let stored_identity = IdentitiesV2::<Test>::get(coldkey).expect("Identity should be set");
        assert_eq!(stored_identity.name, name);
        assert_eq!(stored_identity.url, url);
        assert_eq!(stored_identity.image, image);
        assert_eq!(stored_identity.discord, discord);
        assert_eq!(stored_identity.description, description);
        assert_eq!(stored_identity.additional, additional);

        // Test setting identity with no registered hotkey
        let coldkey_without_hotkey = U256::from(3);
        assert_noop!(
            SubtensorModule::do_set_identity(
                <<Test as Config>::RuntimeOrigin>::signed(coldkey_without_hotkey),
                name.clone(),
                url.clone(),
                git.clone(),
                image.clone(),
                discord.clone(),
                description.clone(),
                additional.clone()
            ),
            Error::<Test>::HotKeyNotRegisteredInNetwork
        );

        // Test updating an existing identity
        let new_name = b"Alice Updated".to_vec();
        let new_url = b"https://alice-updated.com".to_vec();
        assert_ok!(SubtensorModule::do_set_identity(
            <<Test as Config>::RuntimeOrigin>::signed(coldkey),
            new_name.clone(),
            new_url.clone(),
            git.clone(),
            image.clone(),
            discord.clone(),
            description.clone(),
            additional.clone()
        ));

        let updated_identity =
            IdentitiesV2::<Test>::get(coldkey).expect("Updated identity should be set");
        assert_eq!(updated_identity.name, new_name);
        assert_eq!(updated_identity.url, new_url);

        // Test setting identity with invalid data (exceeding 512 bytes total)
        let long_data = vec![0; 513];
        assert_noop!(
            SubtensorModule::do_set_identity(
                <<Test as Config>::RuntimeOrigin>::signed(coldkey),
                long_data.clone(),
                long_data.clone(),
                long_data.clone(),
                long_data.clone(),
                long_data.clone(),
                long_data.clone(),
                long_data.clone()
            ),
            Error::<Test>::InvalidIdentity
        );
    });
}

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --test serving -- test_is_valid_identity --exact --nocapture
#[test]
fn test_is_valid_identity() {
    new_test_ext(1).execute_with(|| {
        // Test valid identity
        let valid_identity = ChainIdentityV2 {
            name: vec![0; 256],
            url: vec![0; 256],
            github_repo: vec![0; 256],
            image: vec![0; 1024],
            discord: vec![0; 256],
            description: vec![0; 1024],
            additional: vec![0; 1024],
        };
        assert!(SubtensorModule::is_valid_identity(&valid_identity));

        // Test identity with total length exactly at the maximum
        let max_length_identity = ChainIdentityV2 {
            name: vec![0; 256],
            url: vec![0; 256],
            github_repo: vec![0; 256],
            image: vec![0; 1024],
            discord: vec![0; 256],
            description: vec![0; 1024],
            additional: vec![0; 1024],
        };
        assert!(SubtensorModule::is_valid_identity(&max_length_identity));

        // Test identity with total length exceeding the maximum
        let invalid_length_identity = ChainIdentityV2 {
            name: vec![0; 257],
            url: vec![0; 256],
            github_repo: vec![0; 256],
            image: vec![0; 1024],
            discord: vec![0; 256],
            description: vec![0; 1024],
            additional: vec![0; 1024],
        };
        assert!(!SubtensorModule::is_valid_identity(
            &invalid_length_identity
        ));

        // Test identity with one field exceeding its maximum
        let invalid_field_identity = ChainIdentityV2 {
            name: vec![0; 257],
            url: vec![0; 256],
            github_repo: vec![0; 256],
            image: vec![0; 1024],
            discord: vec![0; 256],
            description: vec![0; 1024],
            additional: vec![0; 1024],
        };
        assert!(!SubtensorModule::is_valid_identity(&invalid_field_identity));

        // Test identity with empty fields
        let empty_identity = ChainIdentityV2 {
            name: vec![],
            url: vec![],
            github_repo: vec![],
            image: vec![],
            discord: vec![],
            description: vec![],
            additional: vec![],
        };
        assert!(SubtensorModule::is_valid_identity(&empty_identity));

        // Test identity with some empty and some filled fields
        let mixed_identity = ChainIdentityV2 {
            name: b"Alice".to_vec(),
            url: b"https://alice.com".to_vec(),
            github_repo: vec![],
            image: vec![],
            discord: b"alice#1234".to_vec(),
            description: vec![],
            additional: b"Additional info".to_vec(),
        };
        assert!(SubtensorModule::is_valid_identity(&mixed_identity));

        // Test identity with all fields at maximum allowed length
        let max_field_identity = ChainIdentityV2 {
            name: vec![0; 256],
            url: vec![0; 256],
            github_repo: vec![0; 256],
            image: vec![0; 1024],
            discord: vec![0; 256],
            description: vec![0; 1024],
            additional: vec![0; 1024],
        };
        assert!(SubtensorModule::is_valid_identity(&max_field_identity));
    });
}

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --test serving -- test_set_and_get_identity --exact --nocapture
#[test]
fn test_set_and_get_identity() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1);
        let hotkey = U256::from(2);
        let netuid = NetUid::from(1);

        // Register a hotkey for the coldkey
        add_network(netuid, 13, 0);
        register_ok_neuron(netuid, hotkey, coldkey, 0);

        // Prepare identity data
        let name = b"Bob".to_vec();
        let url = b"https://bob.com".to_vec();
        let git = b"https://github.com".to_vec();
        let image = b"bob.jpg".to_vec();
        let discord = b"bob#5678".to_vec();
        let description = b"Bob's identity".to_vec();
        let additional = b"More about Bob".to_vec();

        // Set identity
        assert_ok!(SubtensorModule::do_set_identity(
            <<Test as Config>::RuntimeOrigin>::signed(coldkey),
            name.clone(),
            url.clone(),
            git.clone(),
            image.clone(),
            discord.clone(),
            description.clone(),
            additional.clone()
        ));

        // Get and verify identity
        let stored_identity = IdentitiesV2::<Test>::get(coldkey).expect("Identity should be set");
        assert_eq!(stored_identity.name, name);
        assert_eq!(stored_identity.url, url);
        assert_eq!(stored_identity.image, image);
        assert_eq!(stored_identity.discord, discord);
        assert_eq!(stored_identity.description, description);
        assert_eq!(stored_identity.additional, additional);

        // Update identity
        let new_name = b"Bobby".to_vec();
        let new_url = b"https://bobby.com".to_vec();
        assert_ok!(SubtensorModule::do_set_identity(
            <<Test as Config>::RuntimeOrigin>::signed(coldkey),
            new_name.clone(),
            new_url.clone(),
            git.clone(),
            image.clone(),
            discord.clone(),
            description.clone(),
            additional.clone()
        ));

        // Get and verify updated identity
        let updated_identity =
            IdentitiesV2::<Test>::get(coldkey).expect("Updated identity should be set");
        assert_eq!(updated_identity.name, new_name);
        assert_eq!(updated_identity.url, new_url);
        assert_eq!(updated_identity.image, image);
        assert_eq!(updated_identity.discord, discord);
        assert_eq!(updated_identity.description, description);
        assert_eq!(updated_identity.additional, additional);

        // Verify non-existent identity
        let non_existent_coldkey = U256::from(999);
        assert!(IdentitiesV2::<Test>::get(non_existent_coldkey).is_none());
    });
}

// SKIP_WASM_BUILD=1 RUST_LOG=DEBUG cargo test --release -p pallet-subtensor test_do_set_subnet_identity -- --nocapture
#[test]
fn test_do_set_subnet_identity() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1);
        let hotkey = U256::from(2);
        let netuid = NetUid::from(1);

        // Register a hotkey for the coldkey
        add_network(netuid, 13, 0);
        register_ok_neuron(netuid, hotkey, coldkey, 0);

        // Set coldkey as the owner of the subnet
        SubnetOwner::<Test>::insert(netuid, coldkey);

        // Prepare subnet identity data
        let subnet_name = b"Test Subnet".to_vec();
        let github_repo = b"https://github.com/test/subnet".to_vec();
        let subnet_contact = b"contact@testsubnet.com".to_vec();
        let subnet_url = b"subnet.com".to_vec();
        let discord = b"discord.com".to_vec();
        let description = b"I am the describer".to_vec();
        let logo_url = b"https://testsubnet.com/logo.png".to_vec();
        let additional = b"tao foreva".to_vec();

        // Set subnet identity
        assert_ok!(SubtensorModule::do_set_subnet_identity(
            <<Test as Config>::RuntimeOrigin>::signed(coldkey),
            netuid,
            subnet_name.clone(),
            github_repo.clone(),
            subnet_contact.clone(),
            subnet_url.clone(),
            discord.clone(),
            description.clone(),
            logo_url.clone(),
            additional.clone(),
        ));

        // Check if subnet identity is set correctly
        let stored_identity =
            SubnetIdentitiesV3::<Test>::get(netuid).expect("Subnet identity should be set");
        assert_eq!(stored_identity.subnet_name, subnet_name);
        assert_eq!(stored_identity.github_repo, github_repo);
        assert_eq!(stored_identity.subnet_contact, subnet_contact);
        assert_eq!(stored_identity.logo_url, logo_url);

        // Test setting subnet identity by non-owner
        let non_owner_coldkey = U256::from(2);
        assert_noop!(
            SubtensorModule::do_set_subnet_identity(
                <<Test as Config>::RuntimeOrigin>::signed(non_owner_coldkey),
                netuid,
                subnet_name.clone(),
                github_repo.clone(),
                subnet_contact.clone(),
                subnet_url.clone(),
                discord.clone(),
                description.clone(),
                logo_url.clone(),
                additional.clone(),
            ),
            Error::<Test>::NotSubnetOwner
        );

        // Test updating an existing subnet identity
        let new_subnet_name = b"Updated Subnet".to_vec();
        let new_github_repo = b"https://github.com/test/subnet-updated".to_vec();
        assert_ok!(SubtensorModule::do_set_subnet_identity(
            <<Test as Config>::RuntimeOrigin>::signed(coldkey),
            netuid,
            new_subnet_name.clone(),
            new_github_repo.clone(),
            subnet_contact.clone(),
            subnet_url.clone(),
            discord.clone(),
            description.clone(),
            logo_url.clone(),
            additional.clone(),
        ));

        let updated_identity =
            SubnetIdentitiesV3::<Test>::get(netuid).expect("Updated subnet identity should be set");
        assert_eq!(updated_identity.subnet_name, new_subnet_name);
        assert_eq!(updated_identity.github_repo, new_github_repo);
        assert_eq!(updated_identity.logo_url, logo_url);

        // Test setting subnet identity with invalid data (exceeding 1024 bytes total)
        let long_data = vec![0; 1025];
        assert_noop!(
            SubtensorModule::do_set_subnet_identity(
                <<Test as Config>::RuntimeOrigin>::signed(coldkey),
                netuid,
                long_data.clone(),
                long_data.clone(),
                long_data.clone(),
                long_data.clone(),
                long_data.clone(),
                long_data.clone(),
                long_data.clone(),
                long_data.clone(),
            ),
            Error::<Test>::InvalidIdentity
        );
    });
}

// SKIP_WASM_BUILD=1 RUST_LOG=debug cargo test --test serving -- test_is_valid_subnet_identity --exact --nocapture
#[test]
fn test_is_valid_subnet_identity() {
    new_test_ext(1).execute_with(|| {
        // Test valid subnet identity
        let valid_identity = SubnetIdentityV3 {
            subnet_name: vec![0; 256],
            github_repo: vec![0; 1024],
            subnet_contact: vec![0; 1024],
            subnet_url: vec![0; 1024],
            discord: vec![0; 256],
            description: vec![0; 1024],
            logo_url: vec![0; 1024],
            additional: vec![0; 1024],
        };
        assert!(SubtensorModule::is_valid_subnet_identity(&valid_identity));

        // Test subnet identity with total length exactly at the maximum
        let max_length_identity = SubnetIdentityV3 {
            subnet_name: vec![0; 256],
            github_repo: vec![0; 1024],
            subnet_contact: vec![0; 1024],
            subnet_url: vec![0; 1024],
            discord: vec![0; 256],
            description: vec![0; 1024],
            logo_url: vec![0; 1024],
            additional: vec![0; 1024],
        };
        assert!(SubtensorModule::is_valid_subnet_identity(
            &max_length_identity
        ));

        // Test subnet identity with total length exceeding the maximum
        let invalid_length_identity = SubnetIdentityV3 {
            subnet_name: vec![0; 257],
            github_repo: vec![0; 1024],
            subnet_contact: vec![0; 1024],
            subnet_url: vec![0; 1024],
            discord: vec![0; 256],
            description: vec![0; 1024],
            logo_url: vec![0; 1024],
            additional: vec![0; 1024],
        };
        assert!(!SubtensorModule::is_valid_subnet_identity(
            &invalid_length_identity
        ));

        // Test subnet identity with one field exceeding its maximum
        let invalid_field_identity = SubnetIdentityV3 {
            subnet_name: vec![0; 257],
            github_repo: vec![0; 1024],
            subnet_contact: vec![0; 1024],
            subnet_url: vec![0; 1024],
            discord: vec![0; 256],
            description: vec![0; 1024],
            logo_url: vec![0; 1024],
            additional: vec![0; 1024],
        };
        assert!(!SubtensorModule::is_valid_subnet_identity(
            &invalid_field_identity
        ));

        // Test subnet identity with empty fields
        let empty_identity = SubnetIdentityV3 {
            subnet_name: vec![],
            github_repo: vec![],
            subnet_contact: vec![],
            subnet_url: vec![],
            discord: vec![],
            description: vec![],
            logo_url: vec![],
            additional: vec![],
        };
        assert!(SubtensorModule::is_valid_subnet_identity(&empty_identity));

        // Test subnet identity with some empty and some filled fields
        let mixed_identity = SubnetIdentityV3 {
            subnet_name: b"Test Subnet".to_vec(),
            github_repo: vec![],
            subnet_contact: b"contact@testsubnet.com".to_vec(),
            subnet_url: b"https://testsubnet.com".to_vec(),
            discord: vec![],
            description: b"A description".to_vec(),
            logo_url: vec![],
            additional: vec![],
        };
        assert!(SubtensorModule::is_valid_subnet_identity(&mixed_identity));
    });
}

#[test]
fn test_set_identity_for_non_existent_subnet() {
    new_test_ext(1).execute_with(|| {
        let coldkey = U256::from(1);
        let netuid = NetUid::from(999); // Non-existent subnet ID

        // Subnet identity data
        let subnet_name = b"Non-existent Subnet".to_vec();
        let github_repo = b"https://github.com/test/nonexistent".to_vec();
        let subnet_contact = b"contact@nonexistent.com".to_vec();
        let subnet_url = b"subnet.com".to_vec();
        let discord = b"discord.com".to_vec();
        let description = b"I am the describer".to_vec();
        let logo_url = b"https://testsubnet.com/logo.png".to_vec();
        let additional = b"tao foreva".to_vec();

        // Attempt to set identity for a non-existent subnet
        assert_noop!(
            SubtensorModule::do_set_subnet_identity(
                <<Test as Config>::RuntimeOrigin>::signed(coldkey),
                netuid,
                subnet_name.clone(),
                github_repo.clone(),
                subnet_contact.clone(),
                subnet_url.clone(),
                discord.clone(),
                description.clone(),
                logo_url.clone(),
                additional.clone(),
            ),
            Error::<Test>::NotSubnetOwner // Since there's no owner, it should fail
        );
    });
}

#[test]
fn test_set_subnet_identity_dispatch_info_ok() {
    new_test_ext(1).execute_with(|| {
        let netuid = NetUid::from(1);
        let subnet_name: Vec<u8> = b"JesusSubnet".to_vec();
        let github_repo: Vec<u8> = b"bible.com".to_vec();
        let subnet_contact: Vec<u8> = b"https://www.vatican.va".to_vec();
        let subnet_url = b"subnet.com".to_vec();
        let discord = b"discord.com".to_vec();
        let description = b"I am the describer".to_vec();
        let logo_url = b"https://testsubnet.com/logo.png".to_vec();
        let additional = b"tao foreva".to_vec();

        let call: RuntimeCall = RuntimeCall::SubtensorModule(SubtensorCall::set_subnet_identity {
            netuid,
            subnet_name,
            github_repo,
            subnet_contact,
            subnet_url,
            discord,
            description,
            logo_url,
            additional,
        });

        let dispatch_info: DispatchInfo = call.get_dispatch_info();

        assert_eq!(dispatch_info.class, DispatchClass::Normal);
        assert_eq!(dispatch_info.pays_fee, Pays::Yes);
    });
}
