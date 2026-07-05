//! Subnet sale offers and sale-time freezes.
//!
//! This module intentionally only owns the seller-side primitive: listing a subnet
//! for sale freezes the seller coldkey and seller hotkey until the offer is cancelled
//! or later consumed by a sale finalization path.

use super::*;
use frame_support::traits::fungible;
use frame_system::pallet_prelude::BlockNumberFor;
use subtensor_runtime_common::{NetUid, TaoBalance};

pub type CurrencyOf<T> = <T as Config>::Currency;

pub type BalanceOf<T> =
    <CurrencyOf<T> as fungible::Inspect<<T as frame_system::Config>::AccountId>>::Balance;

/// Monotonically increasing identifier minted for every subnet sale offer.
///
/// A fresh id is assigned on each creation so that a consumer that binds to an
/// offer (e.g. a crowdloan raising funds to buy the subnet) can detect a
/// cancel-and-recreate: the recreated offer carries a new id and no longer
/// matches the one that was bound, so the stale binding fails instead of being
/// silently satisfied by a different offer with modified terms.
pub type SaleOfferId = u64;

#[freeze_struct("6adb80684dface3a")]
#[derive(Encode, Decode, Eq, PartialEq, Ord, PartialOrd, RuntimeDebug, TypeInfo, MaxEncodedLen)]
pub struct SubnetSaleOffer<AccountId, Balance, BlockNumber> {
    /// Unique identifier for this offer.
    pub id: SaleOfferId,
    /// The subnet being sold.
    pub netuid: NetUid,
    /// The subnet owner coldkey that created the offer.
    pub seller_coldkey: AccountId,
    /// The subnet owner hotkey frozen by this offer.
    ///
    /// Kept here (rather than re-read from `SubnetOwnerHotkey` at unfreeze time) so
    /// cancellation and subnet dissolution unfreeze exactly the hotkey that was frozen,
    /// even if the subnet owner hotkey changed while the offer was active.
    pub seller_hotkey: AccountId,
    /// Optional coldkey that is allowed to consume this offer.
    pub authorized_buyer: Option<AccountId>,
    /// Sale price expected by the seller.
    pub price: Balance,
    /// Block at which the sale offer was created.
    pub created_at: BlockNumber,
}

pub type SubnetSaleOfferOf<T> = SubnetSaleOffer<AccountIdOf<T>, BalanceOf<T>, BlockNumberFor<T>>;

impl<T: Config> Pallet<T> {
    pub fn do_create_sale_offer(
        seller_coldkey: T::AccountId,
        netuid: NetUid,
        price: TaoBalance,
        authorized_buyer: Option<T::AccountId>,
    ) -> DispatchResult {
        ensure!(price > TaoBalance::from(0_u64), Error::<T>::AmountTooLow);
        ensure!(Self::if_subnet_exist(netuid), Error::<T>::SubnetNotExists);
        ensure!(
            SubnetOwner::<T>::get(netuid) == seller_coldkey,
            Error::<T>::NotSubnetOwner
        );
        ensure!(
            !SubnetUidToLeaseId::<T>::contains_key(netuid),
            Error::<T>::SubnetIsLeased
        );
        ensure!(
            !SubnetSaleOffers::<T>::contains_key(netuid),
            Error::<T>::SaleOfferAlreadyExists
        );
        ensure!(
            !SubnetSaleFrozenColdkeys::<T>::contains_key(&seller_coldkey),
            Error::<T>::ColdkeyLockedDuringSale
        );
        let seller_hotkey = SubnetOwnerHotkey::<T>::try_get(netuid)
            .map_err(|_| Error::<T>::HotKeyAccountNotExists)?;
        ensure!(
            !SubnetSaleFrozenHotkeys::<T>::contains_key(&seller_hotkey),
            Error::<T>::HotkeyLockedDuringSale
        );

        let id = Self::get_next_sale_offer_id()?;

        SubnetSaleOffers::<T>::insert(
            netuid,
            SubnetSaleOffer {
                id,
                netuid,
                seller_coldkey: seller_coldkey.clone(),
                seller_hotkey: seller_hotkey.clone(),
                authorized_buyer: authorized_buyer.clone(),
                price: price.into(),
                created_at: frame_system::Pallet::<T>::block_number(),
            },
        );
        SubnetSaleFrozenColdkeys::<T>::insert(&seller_coldkey, ());
        // When the seller coldkey and seller hotkey are the same account, the coldkey freeze
        // already locks it (and still permits cancellation). Freezing it in the hotkey map as
        // well would block cancellation, permanently locking the seller out of their own
        // offer, so we skip it in that case.
        if seller_hotkey != seller_coldkey {
            SubnetSaleFrozenHotkeys::<T>::insert(&seller_hotkey, ());
        }

        Self::deposit_event(Event::SubnetSaleOfferCreated {
            id,
            seller_coldkey,
            netuid,
            price,
            authorized_buyer,
        });

        Ok(())
    }

    /// Mint the next unique sale offer id, incrementing the on-chain counter.
    fn get_next_sale_offer_id() -> Result<SaleOfferId, Error<T>> {
        let id = NextSubnetSaleOfferId::<T>::get();
        let next_id = id.checked_add(1).ok_or(Error::<T>::Overflow)?;
        NextSubnetSaleOfferId::<T>::put(next_id);
        Ok(id)
    }

    pub fn do_cancel_sale_offer(
        maybe_seller_coldkey: Option<T::AccountId>,
        netuid: NetUid,
    ) -> DispatchResult {
        let offer = SubnetSaleOffers::<T>::get(netuid).ok_or(Error::<T>::SaleOfferNotFound)?;

        // If the caller is not the seller, they are root.
        if let Some(seller_coldkey) = maybe_seller_coldkey {
            ensure!(
                seller_coldkey == offer.seller_coldkey,
                Error::<T>::NotSubnetOwner
            );
        }

        let seller_coldkey = offer.seller_coldkey.clone();
        SubnetSaleOffers::<T>::remove(offer.netuid);
        SubnetSaleFrozenColdkeys::<T>::remove(&offer.seller_coldkey);
        SubnetSaleFrozenHotkeys::<T>::remove(&offer.seller_hotkey);

        Self::deposit_event(Event::SubnetSaleOfferCancelled {
            id: offer.id,
            seller_coldkey,
            netuid,
        });

        Ok(())
    }
}
