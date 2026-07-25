use super::*;
use crate::HasMigrationRun;
use frame_support::{traits::Get, weights::Weight};
use scale_info::prelude::string::String;
use sp_std::collections::btree_set::BTreeSet;

/// Drops `Axons`, `NeuronCertificates`, and `Prometheus` rows whose hotkey is not a current subnet member.
///
/// Idempotency key (frozen): `migrate_remove_neuron_axon_cert_prom`.
pub fn migrate_remove_unknown_neuron_axon_cert_prom<T: Config>() -> Weight {
    let migration_name = b"migrate_remove_neuron_axon_cert_prom".to_vec();
    let mut weight: Weight = T::DbWeight::get().reads(1);

    // Skip if already executed
    if HasMigrationRun::<T>::get(&migration_name) {
        log::info!(
            target: "runtime",
            "Migration '{}' already run - skipping.",
            String::from_utf8_lossy(&migration_name)
        );
        return weight;
    }
    log::info!(
        "Running migration '{}'",
        String::from_utf8_lossy(&migration_name)
    );

    for network in NetworksAdded::<T>::iter_keys() {
        weight.saturating_accrue(T::DbWeight::get().reads(1));

        let hotkeys = BTreeSet::from_iter(Uids::<T>::iter_key_prefix(network));
        weight.saturating_accrue(T::DbWeight::get().reads(hotkeys.len() as u64));

        // Axons
        let axons = Axons::<T>::iter_key_prefix(network).collect::<Vec<_>>();
        weight.saturating_accrue(T::DbWeight::get().reads(axons.len() as u64));
        let mut cleaned_axons: u32 = 0;
        for axon_hotkey in axons {
            if !hotkeys.contains(&axon_hotkey) {
                Axons::<T>::remove(network, &axon_hotkey);
                cleaned_axons = cleaned_axons.saturating_add(1);
            }
        }
        weight.saturating_accrue(T::DbWeight::get().writes(cleaned_axons as u64));

        // Neuron Certificates
        let certificates = NeuronCertificates::<T>::iter_key_prefix(network).collect::<Vec<_>>();
        weight.saturating_accrue(T::DbWeight::get().reads(certificates.len() as u64));
        let mut cleaned_certificates: u32 = 0;
        for certificate_hotkey in certificates {
            if !hotkeys.contains(&certificate_hotkey) {
                NeuronCertificates::<T>::remove(network, &certificate_hotkey);
                cleaned_certificates = cleaned_certificates.saturating_add(1);
            }
        }
        weight.saturating_accrue(T::DbWeight::get().writes(cleaned_certificates as u64));

        // Prometheus
        let prometheus = Prometheus::<T>::iter_key_prefix(network).collect::<Vec<_>>();
        weight.saturating_accrue(T::DbWeight::get().reads(prometheus.len() as u64));
        let mut cleaned_prometheus: u32 = 0;
        for prometheus_hotkey in prometheus {
            if !hotkeys.contains(&prometheus_hotkey) {
                Prometheus::<T>::remove(network, &prometheus_hotkey);
                cleaned_prometheus = cleaned_prometheus.saturating_add(1);
            }
        }
        weight.saturating_accrue(T::DbWeight::get().writes(cleaned_prometheus as u64));

        log::info!(
            "Cleaned {cleaned_axons} axons, {cleaned_certificates} neuron certificates, {cleaned_prometheus} prometheus for network {network}"
        );
    }

    HasMigrationRun::<T>::insert(&migration_name, true);
    weight = weight.saturating_add(T::DbWeight::get().writes(1));

    log::info!(
        "Migration '{:?}' completed successfully.",
        String::from_utf8_lossy(&migration_name)
    );

    log::info!("{weight:#?} weight");

    weight
}
