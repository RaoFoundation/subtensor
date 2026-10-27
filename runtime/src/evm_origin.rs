//! Native-to-EVM origin that keeps truncated matching but refuses address(0).
//!
//! `EnsureAddressTruncated` treats a 32-byte AccountId as the EVM address of
//! its first 20 bytes. A small-order Ed25519 encoding can have a zero prefix
//! and would otherwise be accepted as `address(0)` for `evm.call` / `evm.withdraw`.

use frame_system::RawOrigin;
use pallet_evm::EnsureAddressOrigin;
use sp_core::H160;
use sp_runtime::AccountId32;

/// Same prefix rule as [`pallet_evm::EnsureAddressTruncated`], plus a reject
/// when the requested EVM address is zero.
pub struct EnsureAddressTruncatedNonZero;

impl<OuterOrigin> EnsureAddressOrigin<OuterOrigin> for EnsureAddressTruncatedNonZero
where
    OuterOrigin: Into<Result<RawOrigin<AccountId32>, OuterOrigin>> + From<RawOrigin<AccountId32>>,
{
    type Success = AccountId32;

    fn try_address_origin(address: &H160, origin: OuterOrigin) -> Result<AccountId32, OuterOrigin> {
        if address.is_zero() {
            return Err(origin);
        }
        origin.into().and_then(|o| match o {
            RawOrigin::Signed(who) if AsRef::<[u8; 32]>::as_ref(&who)[0..20] == address[0..20] => {
                Ok(who)
            }
            r => Err(OuterOrigin::from(r)),
        })
    }
}

#[allow(clippy::unwrap_used)]
#[cfg(test)]
mod tests {
    use super::*;
    use crate::RuntimeOrigin;
    use pallet_evm::EnsureAddressOrigin;
    use subtensor_runtime_common::AccountId;

    fn signed(bytes: [u8; 32]) -> RuntimeOrigin {
        RuntimeOrigin::signed(AccountId::from(bytes))
    }

    #[test]
    fn ordinary_truncated_match_is_accepted() {
        let mut raw = [0u8; 32];
        raw[0] = 0xab;
        raw[19] = 0xcd;
        raw[31] = 0x11;
        let address = H160::from_slice(&raw[0..20]);
        assert!(EnsureAddressTruncatedNonZero::try_address_origin(&address, signed(raw)).is_ok());
    }

    #[test]
    fn zero_evm_address_is_rejected_even_when_prefix_matches() {
        let mut raw = [0u8; 32];
        raw[31] = 0x80;
        assert!(
            EnsureAddressTruncatedNonZero::try_address_origin(&H160::zero(), signed(raw)).is_err()
        );
    }

    #[test]
    fn mismatched_prefix_is_rejected() {
        let mut raw = [0u8; 32];
        raw[0] = 0xab;
        let other = H160::from_low_u64_be(1);
        assert!(EnsureAddressTruncatedNonZero::try_address_origin(&other, signed(raw)).is_err());
    }
}
