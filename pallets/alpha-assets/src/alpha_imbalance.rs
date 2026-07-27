//! Netuid-scoped alpha mint/burn imbalances for FRAME `Imbalance` accounting.
//!
//! A non-zero drop does **not** auto-credit or debit subnet pools; coinbase / staking
//! code must resolve a [`PositiveAlphaImbalance`] (e.g. into alpha-in or alpha-out).

use codec::{Decode, Encode, MaxEncodedLen};
use frame_support::traits::{Imbalance, SameOrOther, TryDrop, tokens::imbalance::TryMerge};
use scale_info::TypeInfo;
use sp_runtime::traits::Zero;
use subtensor_macros::freeze_struct;
use subtensor_runtime_common::{AlphaBalance, NetUid, Token};

/// Pending alpha mint for one subnet; callers resolve it into alpha-in or alpha-out.
///
/// Amounts are in rao of alpha. Merge / offset across different `netuid`s is rejected
/// (logs and keeps the left-hand side) so subnet ledgers never mix.
#[freeze_struct("10d20e374f3d3dc0")]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Encode, Decode, MaxEncodedLen, TypeInfo)]
pub struct PositiveAlphaImbalance {
    netuid: NetUid,
    amount: AlphaBalance,
}

/// Opposite of [`PositiveAlphaImbalance`]: pending alpha debit for one subnet.
///
/// Produced by [`Imbalance::offset`] when a negative imbalance exceeds a positive one.
/// Same netuid-isolation rules as the positive side.
#[freeze_struct("ff6feb7c6031d9d6")]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Encode, Decode, MaxEncodedLen, TypeInfo)]
pub struct NegativeAlphaImbalance {
    netuid: NetUid,
    amount: AlphaBalance,
}

impl PositiveAlphaImbalance {
    /// Builds a mint imbalance for `netuid` with the given alpha `amount` (rao).
    pub fn new(netuid: NetUid, amount: AlphaBalance) -> Self {
        Self { netuid, amount }
    }

    /// Subnet this mint is scoped to.
    pub fn netuid(&self) -> NetUid {
        self.netuid
    }

    /// Alpha amount (rao) still carried by this imbalance.
    pub fn amount(&self) -> AlphaBalance {
        self.amount
    }
}

impl NegativeAlphaImbalance {
    /// Builds a debit imbalance for `netuid` with the given alpha `amount` (rao).
    pub fn new(netuid: NetUid, amount: AlphaBalance) -> Self {
        Self { netuid, amount }
    }
}

/// Logs when imbalance ops attempt to combine different subnet ledgers.
fn log_cross_netuid_alpha_imbalance(context: &'static str, left: NetUid, right: NetUid) {
    log::error!(
        target: "runtime::alpha-assets",
        "{context}: attempted to combine alpha imbalances from different netuids: left={left}, right={right}"
    );
}

impl TryDrop for PositiveAlphaImbalance {
    fn try_drop(self) -> Result<(), Self> {
        if self.amount.is_zero() {
            Ok(())
        } else {
            Err(self)
        }
    }
}

impl TryDrop for NegativeAlphaImbalance {
    fn try_drop(self) -> Result<(), Self> {
        if self.amount.is_zero() {
            Ok(())
        } else {
            Err(self)
        }
    }
}

impl TryMerge for PositiveAlphaImbalance {
    fn try_merge(self, other: Self) -> Result<Self, (Self, Self)> {
        if self.netuid == other.netuid {
            Ok(Self::new(
                self.netuid,
                self.amount.saturating_add(other.amount),
            ))
        } else {
            Err((self, other))
        }
    }
}

impl TryMerge for NegativeAlphaImbalance {
    fn try_merge(self, other: Self) -> Result<Self, (Self, Self)> {
        if self.netuid == other.netuid {
            Ok(Self::new(
                self.netuid,
                self.amount.saturating_add(other.amount),
            ))
        } else {
            Err((self, other))
        }
    }
}

impl Imbalance<AlphaBalance> for PositiveAlphaImbalance {
    type Opposite = NegativeAlphaImbalance;

    fn zero() -> Self {
        Self::default()
    }

    fn drop_zero(self) -> Result<(), Self> {
        self.try_drop()
    }

    fn split(self, amount: AlphaBalance) -> (Self, Self) {
        let first = self.amount.min(amount);
        let second = self.amount.saturating_sub(first);
        (
            Self::new(self.netuid, first),
            Self::new(self.netuid, second),
        )
    }

    fn extract(&mut self, amount: AlphaBalance) -> Self {
        let extracted = self.amount.min(amount);
        self.amount = self.amount.saturating_sub(extracted);
        Self::new(self.netuid, extracted)
    }

    fn merge(self, other: Self) -> Self {
        match self.try_merge(other) {
            Ok(merged) => merged,
            Err((left, right)) => {
                log_cross_netuid_alpha_imbalance("merge(positive)", left.netuid, right.netuid);
                left
            }
        }
    }

    fn subsume(&mut self, other: Self) {
        if self.netuid != other.netuid {
            log_cross_netuid_alpha_imbalance("subsume(positive)", self.netuid, other.netuid);
            return;
        }
        self.amount = self.amount.saturating_add(other.amount);
    }

    fn offset(self, other: Self::Opposite) -> SameOrOther<Self, Self::Opposite> {
        if self.netuid != other.netuid {
            log_cross_netuid_alpha_imbalance("offset(positive)", self.netuid, other.netuid);
            return SameOrOther::Same(self);
        }
        if self.amount > other.amount {
            SameOrOther::Same(Self::new(
                self.netuid,
                self.amount.saturating_sub(other.amount),
            ))
        } else if other.amount > self.amount {
            SameOrOther::Other(NegativeAlphaImbalance::new(
                self.netuid,
                other.amount.saturating_sub(self.amount),
            ))
        } else {
            SameOrOther::None
        }
    }

    fn peek(&self) -> AlphaBalance {
        self.amount
    }
}

impl Imbalance<AlphaBalance> for NegativeAlphaImbalance {
    type Opposite = PositiveAlphaImbalance;

    fn zero() -> Self {
        Self::default()
    }

    fn drop_zero(self) -> Result<(), Self> {
        self.try_drop()
    }

    fn split(self, amount: AlphaBalance) -> (Self, Self) {
        let first = self.amount.min(amount);
        let second = self.amount.saturating_sub(first);
        (
            Self::new(self.netuid, first),
            Self::new(self.netuid, second),
        )
    }

    fn extract(&mut self, amount: AlphaBalance) -> Self {
        let extracted = self.amount.min(amount);
        self.amount = self.amount.saturating_sub(extracted);
        Self::new(self.netuid, extracted)
    }

    fn merge(self, other: Self) -> Self {
        match self.try_merge(other) {
            Ok(merged) => merged,
            Err((left, right)) => {
                log_cross_netuid_alpha_imbalance("merge(negative)", left.netuid, right.netuid);
                left
            }
        }
    }

    fn subsume(&mut self, other: Self) {
        if self.netuid != other.netuid {
            log_cross_netuid_alpha_imbalance("subsume(negative)", self.netuid, other.netuid);
            return;
        }
        self.amount = self.amount.saturating_add(other.amount);
    }

    fn offset(self, other: Self::Opposite) -> SameOrOther<Self, Self::Opposite> {
        if self.netuid != other.netuid {
            log_cross_netuid_alpha_imbalance("offset(negative)", self.netuid, other.netuid);
            return SameOrOther::Same(self);
        }
        if self.amount > other.amount {
            SameOrOther::Same(Self::new(
                self.netuid,
                self.amount.saturating_sub(other.amount),
            ))
        } else if other.amount > self.amount {
            SameOrOther::Other(PositiveAlphaImbalance::new(
                self.netuid,
                other.amount.saturating_sub(self.amount),
            ))
        } else {
            SameOrOther::None
        }
    }

    fn peek(&self) -> AlphaBalance {
        self.amount
    }
}
