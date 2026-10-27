//! Unit-test weight overrides exercise admission boundaries independently of reference costs.
use crate::Runtime;
use crate::{pallet_proxy, pallet_utility};
use frame_support::weights::Weight;
pub struct Proxy;
impl pallet_proxy::weights::WeightInfo for Proxy {
    fn create_protected_pure() -> Weight {
        crate::hashed_auth::TestVerificationWeight::get()
    }
    fn proxy(p: u32) -> Weight {
        <pallet_proxy::weights::SubstrateWeight<Runtime> as pallet_proxy::weights::WeightInfo>::proxy(p)
    }
    fn proxy_announced(a: u32, p: u32) -> Weight {
        <pallet_proxy::weights::SubstrateWeight<Runtime> as pallet_proxy::weights::WeightInfo>::proxy_announced(a, p)
    }
    fn remove_announcement(a: u32, p: u32) -> Weight {
        <pallet_proxy::weights::SubstrateWeight<Runtime> as pallet_proxy::weights::WeightInfo>::remove_announcement(a, p)
    }
    fn reject_announcement(a: u32, p: u32) -> Weight {
        <pallet_proxy::weights::SubstrateWeight<Runtime> as pallet_proxy::weights::WeightInfo>::reject_announcement(a, p)
    }
    fn announce(a: u32, p: u32) -> Weight {
        <pallet_proxy::weights::SubstrateWeight<Runtime> as pallet_proxy::weights::WeightInfo>::announce(a, p)
    }
    fn add_proxy(p: u32) -> Weight {
        <pallet_proxy::weights::SubstrateWeight<Runtime> as pallet_proxy::weights::WeightInfo>::add_proxy(p)
    }
    fn remove_proxy(p: u32) -> Weight {
        <pallet_proxy::weights::SubstrateWeight<Runtime> as pallet_proxy::weights::WeightInfo>::remove_proxy(p)
    }
    fn remove_proxies(p: u32) -> Weight {
        <pallet_proxy::weights::SubstrateWeight<Runtime> as pallet_proxy::weights::WeightInfo>::remove_proxies(p)
    }
    fn create_pure(p: u32) -> Weight {
        <pallet_proxy::weights::SubstrateWeight<Runtime> as pallet_proxy::weights::WeightInfo>::create_pure(p)
    }
    fn kill_pure(p: u32) -> Weight {
        <pallet_proxy::weights::SubstrateWeight<Runtime> as pallet_proxy::weights::WeightInfo>::kill_pure(p)
    }
    fn poke_deposit() -> Weight {
        <pallet_proxy::weights::SubstrateWeight<Runtime> as pallet_proxy::weights::WeightInfo>::poke_deposit()
    }
    fn set_real_pays_fee(p: u32) -> Weight {
        <pallet_proxy::weights::SubstrateWeight<Runtime> as pallet_proxy::weights::WeightInfo>::set_real_pays_fee(p)
    }
}
pub struct Utility;
impl pallet_utility::weights::WeightInfo for Utility {
    fn as_protected_derivative() -> Weight {
        crate::hashed_auth::TestVerificationWeight::get()
    }
    fn batch(c: u32) -> Weight {
        <pallet_utility::weights::SubstrateWeight<Runtime> as pallet_utility::weights::WeightInfo>::batch(c)
    }
    fn as_derivative() -> Weight {
        <pallet_utility::weights::SubstrateWeight<Runtime> as pallet_utility::weights::WeightInfo>::as_derivative()
    }
    fn batch_all(c: u32) -> Weight {
        <pallet_utility::weights::SubstrateWeight<Runtime> as pallet_utility::weights::WeightInfo>::batch_all(c)
    }
    fn dispatch_as() -> Weight {
        <pallet_utility::weights::SubstrateWeight<Runtime> as pallet_utility::weights::WeightInfo>::dispatch_as()
    }
    fn force_batch(c: u32) -> Weight {
        <pallet_utility::weights::SubstrateWeight<Runtime> as pallet_utility::weights::WeightInfo>::force_batch(c)
    }
    fn dispatch_as_fallible() -> Weight {
        <pallet_utility::weights::SubstrateWeight<Runtime> as pallet_utility::weights::WeightInfo>::dispatch_as_fallible()
    }
    fn if_else() -> Weight {
        <pallet_utility::weights::SubstrateWeight<Runtime> as pallet_utility::weights::WeightInfo>::if_else()
    }
    fn with_weight() -> Weight {
        <pallet_utility::weights::SubstrateWeight<Runtime> as pallet_utility::weights::WeightInfo>::with_weight()
    }
}
pub struct Shield;
impl pallet_shield::weights::WeightInfo for Shield {
    fn submit_encrypted_large() -> Weight {
        crate::hashed_auth::TestVerificationWeight::get()
    }
    fn announce_next_key() -> Weight {
        <pallet_shield::weights::SubstrateWeight<Runtime> as pallet_shield::weights::WeightInfo>::announce_next_key()
    }
    fn submit_encrypted() -> Weight {
        <pallet_shield::weights::SubstrateWeight<Runtime> as pallet_shield::weights::WeightInfo>::submit_encrypted()
    }
    fn set_max_pending_extrinsics_number() -> Weight {
        <pallet_shield::weights::SubstrateWeight<Runtime> as pallet_shield::weights::WeightInfo>::set_max_pending_extrinsics_number()
    }
    fn set_on_initialize_weight() -> Weight {
        <pallet_shield::weights::SubstrateWeight<Runtime> as pallet_shield::weights::WeightInfo>::set_on_initialize_weight()
    }
    fn set_stored_extrinsic_lifetime() -> Weight {
        <pallet_shield::weights::SubstrateWeight<Runtime> as pallet_shield::weights::WeightInfo>::set_stored_extrinsic_lifetime()
    }
    fn set_max_extrinsic_weight() -> Weight {
        <pallet_shield::weights::SubstrateWeight<Runtime> as pallet_shield::weights::WeightInfo>::set_max_extrinsic_weight()
    }
}
pub struct Orders;
impl pallet_limit_orders::weights::WeightInfo for Orders {
    fn execute_falcon512_orders(_: u32, _: u32) -> Weight {
        crate::hashed_auth::TestVerificationWeight::get()
    }
    fn execute_falcon1024_orders(_: u32, _: u32) -> Weight {
        crate::hashed_auth::TestVerificationWeight::get()
    }
    fn execute_falcon512_batched_orders(_: u32, _: u32) -> Weight {
        crate::hashed_auth::TestVerificationWeight::get()
    }
    fn execute_falcon1024_batched_orders(_: u32, _: u32) -> Weight {
        crate::hashed_auth::TestVerificationWeight::get()
    }
    fn authorize_order() -> Weight {
        crate::hashed_auth::TestVerificationWeight::get()
    }
    fn execute_authenticated_orders(_n: u32, _proofs: u32) -> Weight {
        crate::hashed_auth::TestVerificationWeight::get()
    }
    fn execute_authenticated_batched_orders(_n: u32, _proofs: u32) -> Weight {
        crate::hashed_auth::TestVerificationWeight::get()
    }
    fn cancel_authenticated_order() -> Weight {
        crate::hashed_auth::TestVerificationWeight::get()
    }
    fn cancel_order() -> Weight {
        <pallet_limit_orders::weights::SubstrateWeight<Runtime> as pallet_limit_orders::weights::WeightInfo>::cancel_order()
    }
    fn set_pallet_status() -> Weight {
        <pallet_limit_orders::weights::SubstrateWeight<Runtime> as pallet_limit_orders::weights::WeightInfo>::set_pallet_status()
    }
    fn execute_orders(n: u32) -> Weight {
        <pallet_limit_orders::weights::SubstrateWeight<Runtime> as pallet_limit_orders::weights::WeightInfo>::execute_orders(n)
    }
    fn execute_batched_orders(n: u32) -> Weight {
        <pallet_limit_orders::weights::SubstrateWeight<Runtime> as pallet_limit_orders::weights::WeightInfo>::execute_batched_orders(n)
    }
    fn prune_linked_output() -> Weight {
        <pallet_limit_orders::weights::SubstrateWeight<Runtime> as pallet_limit_orders::weights::WeightInfo>::prune_linked_output()
    }
}
