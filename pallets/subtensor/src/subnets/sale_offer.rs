//! Subnet sale offers and sale-time freezes.
//!
//! This module intentionally only owns the seller-side primitive: listing a subnet
//! for sale freezes the seller coldkey and owner hotkey until the offer is cancelled
//! or later consumed by a sale finalization path.

use super::*;
use frame_support::traits::fungible;
use frame_system::pallet_prelude::BlockNumberFor;
use subtensor_runtime_common::{NetUid, TaoBalance};

pub type CurrencyOf<T> = <T as Config>::Currency;

pub type BalanceOf<T> =
    <CurrencyOf<T> as fungible::Inspect<<T as frame_system::Config>::AccountId>>::Balance;

#[freeze_struct("d7b8c7328636281a")]
#[derive(Encode, Decode, Eq, PartialEq, Ord, PartialOrd, RuntimeDebug, TypeInfo, MaxEncodedLen)]
pub struct SubnetSaleOffer<AccountId, Balance, BlockNumber> {
    /// The subnet being sold.
    pub netuid: NetUid,
    /// The subnet owner coldkey that created the offer.
    pub seller: AccountId,
    /// The subnet owner hotkey frozen by this offer.
    ///
    /// Kept here (rather than re-read from `SubnetOwnerHotkey` at unfreeze time) so
    /// cancellation and subnet dissolution unfreeze exactly the hotkey that was frozen,
    /// even if the subnet owner hotkey changed while the offer was active.
    pub owner_hotkey: AccountId,
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
        seller: T::AccountId,
        netuid: NetUid,
        price: TaoBalance,
        authorized_buyer: Option<T::AccountId>,
    ) -> DispatchResult {
        ensure!(price > TaoBalance::from(0_u64), Error::<T>::AmountTooLow);
        ensure!(Self::if_subnet_exist(netuid), Error::<T>::SubnetNotExists);
        ensure!(
            SubnetOwner::<T>::get(netuid) == seller,
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
            !SubnetSaleFrozenColdkeys::<T>::contains_key(&seller),
            Error::<T>::ColdkeyLockedDuringSale
        );
        let owner_hotkey = SubnetOwnerHotkey::<T>::try_get(netuid)
            .map_err(|_| Error::<T>::HotKeyAccountNotExists)?;
        ensure!(
            owner_hotkey != seller,
            Error::<T>::SubnetOwnerHotkeyCannotBeOwnerColdkey
        );
        ensure!(
            !SubnetSaleFrozenHotkeys::<T>::contains_key(&owner_hotkey),
            Error::<T>::HotkeyLockedDuringSale
        );

        SubnetSaleOffers::<T>::insert(
            netuid,
            SubnetSaleOffer {
                netuid,
                seller: seller.clone(),
                owner_hotkey: owner_hotkey.clone(),
                authorized_buyer: authorized_buyer.clone(),
                price: price.into(),
                created_at: frame_system::Pallet::<T>::block_number(),
            },
        );
        SubnetSaleFrozenColdkeys::<T>::insert(&seller, ());
        SubnetSaleFrozenHotkeys::<T>::insert(&owner_hotkey, ());

        Self::deposit_event(Event::SubnetSaleOfferCreated {
            seller,
            netuid,
            price,
            authorized_buyer,
        });

        Ok(())
    }

    pub fn do_cancel_sale_offer(
        maybe_seller: Option<T::AccountId>,
        netuid: NetUid,
    ) -> DispatchResult {
        let offer = SubnetSaleOffers::<T>::get(netuid).ok_or(Error::<T>::SaleOfferNotFound)?;

        // If the caller is not the seller, they are root.
        if let Some(seller) = maybe_seller {
            ensure!(seller == offer.seller, Error::<T>::NotSubnetOwner);
        }

        let seller = offer.seller.clone();
        SubnetSaleOffers::<T>::remove(offer.netuid);
        SubnetSaleFrozenColdkeys::<T>::remove(&offer.seller);
        SubnetSaleFrozenHotkeys::<T>::remove(&offer.owner_hotkey);

        Self::deposit_event(Event::SubnetSaleOfferCancelled { seller, netuid });

        Ok(())
    }
}
