//! Block TAO emission schedule (logarithmic decay toward the hard supply cap).

use super::*;
use crate::coinbase::tao::TaoCreditOf;
use frame_support::traits::Imbalance;
use safe_math::*;
use substrate_fixed::{transcendental::log2, types::I96F32};

impl<T: Config> Pallet<T> {
    /// Mint this block's TAO emission as a currency credit (or zero credit if none).
    ///
    /// Uses [`Pallet::calculate_block_emission`] then [`Pallet::mint_tao`]. The credit is
    /// later spent by [`Pallet::run_coinbase`].
    pub fn get_block_emission() -> TaoCreditOf<T> {
        let maybe_tao_to_mint = Self::calculate_block_emission();
        if let Ok(tao_to_mint) = maybe_tao_to_mint
            && !tao_to_mint.is_zero()
        {
            return Self::mint_tao(tao_to_mint.into());
        }
        TaoCreditOf::<T>::zero()
    }

    /// Block emission in TAO for the current total issuance — no minting.
    pub fn calculate_block_emission() -> Result<TaoBalance, &'static str> {
        Self::get_block_emission_for_issuance(Self::get_total_issuance().into()).map(Into::into)
    }

    /// Block emission (rao) for a hypothetical `issuance` under the log₂ residual schedule.
    ///
    /// Returns `0` when issuance is at or above [`TotalSupply`]. The curve floors the log
    /// residual so emission steps down in powers of two relative to [`DefaultBlockEmission`].
    pub fn get_block_emission_for_issuance(issuance: u64) -> Result<u64, &'static str> {
        let total_issuance: I96F32 = I96F32::saturating_from_num(issuance);
        // Check to prevent division by zero when the total supply is reached
        // and creating an issuance greater than the total supply.
        if total_issuance >= I96F32::saturating_from_num(TotalSupply::<T>::get()) {
            return Ok(0);
        }
        // Calculate the logarithmic residual of the issuance against half the total supply.
        let residual: I96F32 = log2(
            I96F32::saturating_from_num(1.0)
                .checked_div(
                    I96F32::saturating_from_num(1.0)
                        .checked_sub(
                            total_issuance
                                .checked_div(I96F32::saturating_from_num(2.0).saturating_mul(
                                    I96F32::saturating_from_num(10_500_000_000_000_000.0),
                                ))
                                .ok_or("Logarithm calculation failed")?,
                        )
                        .ok_or("Logarithm calculation failed")?,
                )
                .ok_or("Logarithm calculation failed")?,
        )
        .map_err(|_| "Logarithm calculation failed")?;
        // Floor the residual to smooth out the emission rate.
        let floored_residual: I96F32 = residual.floor();
        // Calculate the final emission rate using the floored residual.
        // Convert floored_residual to an integer
        let floored_residual_int: u64 = floored_residual.saturating_to_num::<u64>();
        // Multiply 2.0 by itself floored_residual times to calculate the power of 2.
        let mut multiplier: I96F32 = I96F32::saturating_from_num(1.0);
        for _ in 0..floored_residual_int {
            multiplier = multiplier.saturating_mul(I96F32::saturating_from_num(2.0));
        }
        let block_emission_percentage: I96F32 =
            I96F32::saturating_from_num(1.0).safe_div(multiplier);
        // Calculate the actual emission based on the emission rate
        let block_emission: I96F32 = block_emission_percentage
            .saturating_mul(I96F32::saturating_from_num(DefaultBlockEmission::<T>::get()));
        // Convert to u64
        let block_emission_u64: u64 = block_emission.saturating_to_num::<u64>();
        Ok(block_emission_u64)
    }
}
