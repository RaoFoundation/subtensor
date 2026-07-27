//! Integration tests for `chain_spec` seed / authority-key helpers.

use sp_core::sr25519;
// use sp_consensus_aura::sr25519::AuthorityId as AuraId;
// use sp_consensus_grandpa::AuthorityId as GrandpaId;

use node_subtensor::chain_spec::*;

#[test]
fn get_from_seed_returns_expected_ss58() {
    let seed = "WoOt";
    let pare = get_from_seed::<sr25519::Public>(seed);
    let expected = "5Gj3QEiZaFJPFK1yN4Lkj6FLM4V7GEBCewVBVniuvZ75S2Fd";
    assert_eq!(pare.to_string(), expected);
}

#[test]
#[should_panic(expected = "static values are valid; qed: InvalidFormat")]
fn get_from_seed_panics_on_empty_seed() {
    let bad_seed = "";
    get_from_seed::<sr25519::Public>(bad_seed);
}

#[test]
fn get_account_id_from_seed_returns_expected_ss58() {
    let seed = "WoOt";
    let account_id = get_account_id_from_seed::<sr25519::Public>(seed);
    let expected = "5Gj3QEiZaFJPFK1yN4Lkj6FLM4V7GEBCewVBVniuvZ75S2Fd";
    assert_eq!(account_id.to_string(), expected);
}

#[test]
#[should_panic(expected = "static values are valid; qed: InvalidFormat")]
fn get_account_id_from_seed_panics_on_empty_seed() {
    let bad_seed = "";
    get_account_id_from_seed::<sr25519::Public>(bad_seed);
}

#[test]
fn authority_keys_from_seed_returns_aura_and_grandpa() {
    let seed = "WoOt";
    let (aura_id, grandpa_id) = authority_keys_from_seed(seed);

    let expected_aura_id = "5Gj3QEiZaFJPFK1yN4Lkj6FLM4V7GEBCewVBVniuvZ75S2Fd";
    let expected_grandpa_id = "5H7623Nvxq655p9xrLQPip1mwssFRMfL5fvT5LUSa4nWwLSm";

    assert_eq!(aura_id.to_string(), expected_aura_id);
    assert_eq!(grandpa_id.to_string(), expected_grandpa_id);
}

#[test]
#[should_panic(expected = "static values are valid; qed: InvalidFormat")]
fn authority_keys_from_seed_panics_on_empty_seed() {
    let bad_seed = "";
    authority_keys_from_seed(bad_seed);
}
