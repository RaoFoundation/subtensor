use super::*;
use subtensor_runtime_common::NetUid;

impl<T: Config> Pallet<T> {
    /// ---- The implementation for the extrinsic serve_axon which sets the ip endpoint information for a uid on a network.
    ///
    /// # Args:
    /// * 'origin': (<T as frame_system::Config>RuntimeOrigin):
    ///     - The signature of the caller.
    ///
    /// * 'netuid' (u16):
    ///     - The u16 network identifier.
    ///
    /// * 'version' (u64):
    ///     - The bittensor version identifier.
    ///
    /// * 'ip' (u64):
    ///     - The endpoint ip information as a u128 encoded integer.
    ///
    /// * 'port' (u16):
    ///     - The endpoint port information as a u16 encoded integer.
    ///
    /// * 'ip_type' (u8):
    ///     - The endpoint ip version as a u8, 4 or 6.
    ///
    /// * 'protocol' (u8):
    ///     - UDP:1 or TCP:0
    ///
    /// * 'placeholder1' (u8):
    ///     - Placeholder for further extra params.
    ///
    /// * 'placeholder2' (u8):
    ///     - Placeholder for further extra params.
    ///
    /// * 'certificate' (Option<Vec<u8>>):
    ///     - Certificate for mutual Tls connection between neurons
    ///
    /// # Event:
    /// * AxonServed;
    ///     - On successfully serving the axon info.
    ///
    /// # Raises:
    /// * 'MechanismDoesNotExist':
    ///     - Attempting to set weights on a non-existent network.
    ///
    /// * 'NotRegistered':
    ///     - Attempting to set weights from a non registered account.
    ///
    /// * 'InvalidIpType':
    ///     - The ip type is not 4 or 6.
    ///
    /// * 'InvalidIpAddress':
    ///     - The numerically encoded ip address does not resolve to a proper ip.
    ///
    /// * 'ServingRateLimitExceeded':
    ///     - Attempting to set prometheus information withing the rate limit min.
    ///
    pub fn do_serve_axon(
        origin: OriginFor<T>,
        netuid: NetUid,
        version: u32,
        ip: u128,
        port: u16,
        ip_type: u8,
        protocol: u8,
        placeholder1: u8,
        placeholder2: u8,
        certificate: Option<Vec<u8>>,
    ) -> dispatch::DispatchResult {
        // We check the callers (hotkey) signature.
        let hotkey_id = ensure_signed(origin)?;

        // Validate user input
        Self::validate_serve_axon(
            &hotkey_id,
            netuid,
            version,
            ip,
            port,
            ip_type,
            protocol,
            placeholder1,
            placeholder2,
        )?;

        // Check+insert certificate
        if let Some(certificate) = certificate
            && let Ok(certificate) = NeuronCertificateOf::try_from(certificate)
        {
            NeuronCertificates::<T>::insert(netuid, hotkey_id.clone(), certificate)
        }

        // We insert the axon meta.
        let mut prev_axon = Self::get_axon_info(netuid, &hotkey_id);
        prev_axon.block = Self::get_current_block_as_u64();
        prev_axon.version = version;
        prev_axon.ip = ip;
        prev_axon.port = port;
        prev_axon.ip_type = ip_type;
        prev_axon.protocol = protocol;
        prev_axon.placeholder1 = placeholder1;
        prev_axon.placeholder2 = placeholder2;

        // Validate axon data with delegate func
        let axon_validated = Self::validate_axon_data(&prev_axon);
        ensure!(
            axon_validated.is_ok(),
            axon_validated.err().unwrap_or(Error::<T>::InvalidPort)
        );

        Axons::<T>::insert(netuid, hotkey_id.clone(), prev_axon);

        // We deposit axon served event.
        log::debug!("AxonServed( hotkey:{:?} ) ", hotkey_id.clone());
        Self::deposit_event(Event::AxonServed(netuid, hotkey_id));

        // Return is successful dispatch.
        Ok(())
    }

    /// ---- The implementation for the extrinsic serve_prometheus.
    ///
    /// # Args:
    /// * 'origin': (<T as frame_system::Config>RuntimeOrigin):
    ///     - The signature of the caller.
    ///
    /// * 'netuid' (u16):
    ///     - The u16 network identifier.
    ///
    /// * 'version' (u64):
    ///     - The bittensor version identifier.
    ///
    /// * 'ip' (u64):
    ///     - The prometheus ip information as a u128 encoded integer.
    ///
    /// * 'port' (u16):
    ///     - The prometheus port information as a u16 encoded integer.
    ///
    /// * 'ip_type' (u8):
    ///     - The prometheus ip version as a u8, 4 or 6.
    ///
    /// # Event:
    /// * PrometheusServed;
    ///     - On successfully serving the axon info.
    ///
    /// # Raises:
    /// * 'MechanismDoesNotExist':
    ///     - Attempting to set weights on a non-existent network.
    ///
    /// * 'NotRegistered':
    ///     - Attempting to set weights from a non registered account.
    ///
    /// * 'InvalidIpType':
    ///     - The ip type is not 4 or 6.
    ///
    /// * 'InvalidIpAddress':
    ///     - The numerically encoded ip address does not resolve to a proper ip.
    ///
    /// * 'ServingRateLimitExceeded':
    ///     - Attempting to set prometheus information withing the rate limit min.
    ///
    pub fn do_serve_prometheus(
        origin: OriginFor<T>,
        netuid: NetUid,
        version: u32,
        ip: u128,
        port: u16,
        ip_type: u8,
    ) -> dispatch::DispatchResult {
        // We check the callers (hotkey) signature.
        let hotkey_id = ensure_signed(origin)?;

        let updated_prometheus =
            Self::validate_serve_prometheus(&hotkey_id, netuid, version, ip, port, ip_type)?;

        // Insert new prometheus data
        Prometheus::<T>::insert(netuid, hotkey_id.clone(), updated_prometheus);

        // We deposit prometheus served event.
        log::debug!("PrometheusServed( hotkey:{:?} ) ", hotkey_id.clone());
        Self::deposit_event(Event::PrometheusServed(netuid, hotkey_id));

        // Return is successful dispatch.
        Ok(())
    }

    /********************************
     --==[[  Helper functions   ]]==--
    *********************************/

    pub fn axon_passes_rate_limit(
        netuid: NetUid,
        prev_axon_info: &AxonInfoOf,
        current_block: u64,
    ) -> bool {
        let rate_limit: u64 = Self::get_serving_rate_limit(netuid);
        let last_serve = prev_axon_info.block;
        rate_limit == 0 || last_serve == 0 || current_block.saturating_sub(last_serve) >= rate_limit
    }

    pub fn prometheus_passes_rate_limit(
        netuid: NetUid,
        prev_prometheus_info: &PrometheusInfoOf,
        current_block: u64,
    ) -> bool {
        let rate_limit: u64 = Self::get_serving_rate_limit(netuid);
        let last_serve = prev_prometheus_info.block;
        rate_limit == 0 || last_serve == 0 || current_block.saturating_sub(last_serve) >= rate_limit
    }

    pub fn get_axon_info(netuid: NetUid, hotkey: &T::AccountId) -> AxonInfoOf {
        if let Some(axons) = Axons::<T>::get(netuid, hotkey) {
            axons
        } else {
            AxonInfo {
                block: 0,
                version: 0,
                ip: 0,
                port: 0,
                ip_type: 0,
                protocol: 0,
                placeholder1: 0,
                placeholder2: 0,
            }
        }
    }

    pub fn get_prometheus_info(netuid: NetUid, hotkey: &T::AccountId) -> PrometheusInfoOf {
        if let Some(prometheus) = Prometheus::<T>::get(netuid, hotkey) {
            prometheus
        } else {
            PrometheusInfo {
                block: 0,
                version: 0,
                ip: 0,
                port: 0,
                ip_type: 0,
            }
        }
    }

    pub fn is_valid_ip_type(ip_type: u8) -> bool {
        let allowed_values = [4, 6];
        allowed_values.contains(&ip_type)
    }

    // @todo (Parallax 2-1-2021) : Implement exclusion of private IP ranges
    pub fn is_valid_ip_address(ip_type: u8, addr: u128, allow_zero: bool) -> bool {
        if !allow_zero && addr == 0 {
            return false;
        }
        if ip_type == 4 {
            if addr >= u32::MAX as u128 {
                return false;
            }
            if addr == 0x7f000001 {
                return false;
            } // Localhost
        }
        if ip_type == 6 {
            if addr == u128::MAX {
                return false;
            }
            if addr == 1 {
                return false;
            } // IPv6 localhost
        }
        true
    }

    pub fn validate_axon_data(axon_info: &AxonInfoOf) -> Result<bool, pallet::Error<T>> {
        if axon_info.port.clamp(0, u16::MAX) == 0 {
            return Err(Error::<T>::InvalidPort);
        }

        Ok(true)
    }

    pub fn validate_prometheus_data(
        prom_info: &PrometheusInfoOf,
    ) -> Result<bool, pallet::Error<T>> {
        if prom_info.port.clamp(0, u16::MAX) == 0 {
            return Err(Error::<T>::InvalidPort);
        }

        Ok(true)
    }

    pub fn validate_serve_axon(
        hotkey_id: &T::AccountId,
        netuid: NetUid,
        version: u32,
        ip: u128,
        port: u16,
        ip_type: u8,
        protocol: u8,
        placeholder1: u8,
        placeholder2: u8,
    ) -> Result<(), Error<T>> {
        // Ensure the hotkey is registered on the subnet it is serving.
        ensure!(
            Self::is_hotkey_registered_on_network(netuid, hotkey_id),
            Error::<T>::HotKeyNotRegisteredInNetwork
        );

        // Check the ip signature validity.
        ensure!(Self::is_valid_ip_type(ip_type), Error::<T>::InvalidIpType);
        ensure!(
            // allow axon to be served with a zero ip address for testing purposes
            Self::is_valid_ip_address(ip_type, ip, true),
            Error::<T>::InvalidIpAddress
        );

        // Get the previous axon information.
        let mut prev_axon = Self::get_axon_info(netuid, hotkey_id);
        let current_block: u64 = Self::get_current_block_as_u64();
        ensure!(
            Self::axon_passes_rate_limit(netuid, &prev_axon, current_block),
            Error::<T>::ServingRateLimitExceeded
        );

        // Validate axon data with delegate func
        prev_axon.block = Self::get_current_block_as_u64();
        prev_axon.version = version;
        prev_axon.ip = ip;
        prev_axon.port = port;
        prev_axon.ip_type = ip_type;
        prev_axon.protocol = protocol;
        prev_axon.placeholder1 = placeholder1;
        prev_axon.placeholder2 = placeholder2;

        let axon_validated = Self::validate_axon_data(&prev_axon);
        ensure!(
            axon_validated.is_ok(),
            axon_validated.err().unwrap_or(Error::<T>::InvalidPort)
        );

        Ok(())
    }

    /// Same checks as [`Self::do_serve_prometheus`] before storage writes (for transaction extension).
    pub fn validate_serve_prometheus(
        hotkey_id: &T::AccountId,
        netuid: NetUid,
        version: u32,
        ip: u128,
        port: u16,
        ip_type: u8,
    ) -> Result<PrometheusInfoOf, Error<T>> {
        ensure!(Self::is_valid_ip_type(ip_type), Error::<T>::InvalidIpType);
        ensure!(
            Self::is_valid_ip_address(ip_type, ip, false),
            Error::<T>::InvalidIpAddress
        );

        ensure!(
            Self::is_hotkey_registered_on_network(netuid, hotkey_id),
            Error::<T>::HotKeyNotRegisteredInNetwork
        );

        let mut prev_prometheus = Self::get_prometheus_info(netuid, hotkey_id);
        let current_block: u64 = Self::get_current_block_as_u64();
        ensure!(
            Self::prometheus_passes_rate_limit(netuid, &prev_prometheus, current_block),
            Error::<T>::ServingRateLimitExceeded
        );

        prev_prometheus.block = Self::get_current_block_as_u64();
        prev_prometheus.version = version;
        prev_prometheus.ip = ip;
        prev_prometheus.port = port;
        prev_prometheus.ip_type = ip_type;

        let prom_validated = Self::validate_prometheus_data(&prev_prometheus);
        ensure!(
            prom_validated.is_ok(),
            prom_validated.err().unwrap_or(Error::<T>::InvalidPort)
        );

        Ok(prev_prometheus)
    }
}
