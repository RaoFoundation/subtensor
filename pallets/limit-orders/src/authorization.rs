//! Additional order authorization without changing legacy signed-order bytes.

use crate::*;
use frame_support::{dispatch::DispatchResult, ensure, traits::Get, transactional};
use sp_runtime::traits::Zero;

pub const ORDER_ID_DOMAIN: &[u8] = b"bittensor/ms/v2/order-id";
/// Bound account-approved entries independently of legacy batching.
pub type MaxAuthorizedOrders = ConstU32<8>;

#[derive(
    Clone, Debug, PartialEq, Eq, Encode, Decode, DecodeWithMemTracking, TypeInfo, MaxEncodedLen,
)]
pub enum OrderAuthorization {
    #[codec(index = 0)]
    AccountApproved,
}

#[derive(
    Clone, Debug, PartialEq, Eq, Encode, Decode, DecodeWithMemTracking, TypeInfo, MaxEncodedLen,
)]
pub struct AuthorizedOrder {
    pub nonce: u64,
    pub order: VersionedOrder<AccountId32>,
    pub authorization: OrderAuthorization,
    pub partial_fill: Option<u64>,
}

/// Local order proof encoding. The first three variants are byte-for-byte
/// identical to MultiSignature. This never changes the outer extrinsic signature.
#[derive(
    Clone, Debug, PartialEq, Eq, Encode, Decode, DecodeWithMemTracking, TypeInfo, MaxEncodedLen,
)]
pub enum OrderSignature {
    #[codec(index = 0)]
    Ed25519(sp_core::ed25519::Signature),
    #[codec(index = 1)]
    Sr25519(sp_core::sr25519::Signature),
    #[codec(index = 2)]
    Ecdsa(sp_core::ecdsa::Signature),
    #[codec(index = 3)]
    AccountApproved,
}

impl OrderSignature {
    pub fn is_authenticated(&self) -> bool {
        matches!(self, Self::AccountApproved)
    }

    pub fn verify(&self, message: &[u8], signer: &AccountId32) -> bool {
        use sp_runtime::{MultiSignature, traits::Verify};
        let signature = match self {
            Self::Ed25519(signature) => MultiSignature::Ed25519(*signature),
            Self::Sr25519(signature) => MultiSignature::Sr25519(*signature),
            Self::Ecdsa(signature) => MultiSignature::Ecdsa(*signature),
            Self::AccountApproved => return false,
        };
        signature.verify(message, signer)
    }
}

impl From<sp_runtime::MultiSignature> for OrderSignature {
    fn from(signature: sp_runtime::MultiSignature) -> Self {
        match signature {
            sp_runtime::MultiSignature::Ed25519(signature) => Self::Ed25519(signature),
            sp_runtime::MultiSignature::Sr25519(signature) => Self::Sr25519(signature),
            sp_runtime::MultiSignature::Ecdsa(signature) => Self::Ecdsa(signature),
        }
    }
}

impl TryFrom<AuthorizedOrder> for SignedOrder<AccountId32> {
    type Error = &'static str;
    fn try_from(signed: AuthorizedOrder) -> Result<Self, Self::Error> {
        Ok(Self {
            order: signed.order.with_nonce(signed.nonce)?,
            signature: match signed.authorization {
                OrderAuthorization::AccountApproved => OrderSignature::AccountApproved,
            },
            partial_fill: signed.partial_fill,
        })
    }
}

impl<T: Config> Pallet<T> {
    pub(crate) fn authenticated_order(
        signed: &SignedOrder<AccountId32>,
    ) -> Result<Option<AuthorizedOrder>, sp_runtime::DispatchError> {
        let Some((nonce, order)) = signed.order.authorized_terms() else {
            ensure!(
                !signed.signature.is_authenticated(),
                Error::<T>::InvalidAuthorizationMode
            );
            return Ok(None);
        };
        let authorization = match &signed.signature {
            OrderSignature::AccountApproved => OrderAuthorization::AccountApproved,
            _ => return Err(Error::<T>::InvalidAuthorizationMode.into()),
        };
        Ok(Some(AuthorizedOrder {
            nonce,
            order,
            authorization,
            partial_fill: signed.partial_fill,
        }))
    }

    pub(crate) fn check_authorized_batch_bound(
        orders: &[SignedOrder<AccountId32>],
    ) -> DispatchResult {
        // Bound attempted account approvals, including malformed combinations,
        // before any signature verification or mutable order processing.
        let count = orders
            .iter()
            .filter(|signed| signed.order.is_authorized() || signed.signature.is_authenticated())
            .count();
        ensure!(
            count <= <MaxAuthorizedOrders as Get<u32>>::get() as usize,
            Error::<T>::OrderBatchTooLarge
        );
        Ok(())
    }

    pub fn execute_orders_weight(
        orders: &[SignedOrder<AccountId32>],
    ) -> frame_support::weights::Weight {
        use crate::weights::WeightInfo;
        let count = orders
            .iter()
            .filter(|signed| signed.order.is_authorized() || signed.signature.is_authenticated())
            .count() as u32;
        if count == 0 {
            T::WeightInfo::execute_orders(orders.len() as u32)
        } else {
            // The two-dimensional reference benchmark covers total entries 8..max
            // and authenticated entries 1..8. Small batches charge its minimum.
            let n = (orders.len() as u32).max(8);
            T::WeightInfo::execute_authenticated_orders(n, count)
        }
    }

    pub fn execute_batched_orders_weight(
        orders: &[SignedOrder<AccountId32>],
    ) -> frame_support::weights::Weight {
        use crate::weights::WeightInfo;
        let count = orders
            .iter()
            .filter(|signed| signed.order.is_authorized() || signed.signature.is_authenticated())
            .count() as u32;
        if count == 0 {
            T::WeightInfo::execute_batched_orders(orders.len() as u32)
        } else {
            let n = (orders.len() as u32).max(8);
            T::WeightInfo::execute_authenticated_batched_orders(n, count)
        }
    }

    #[transactional]
    pub(crate) fn cancel_authenticated_order(
        who: AccountId32,
        nonce: u64,
        order: VersionedOrder<AccountId32>,
    ) -> DispatchResult {
        let order_id = Self::derive_authorized_order_id(nonce, &order);
        Self::check_order_nonce(&who, nonce, order_id)?;
        ensure!(
            !matches!(
                Orders::<T>::get(order_id),
                Some(OrderStatus::Fulfilled | OrderStatus::Cancelled)
            ),
            Error::<T>::OrderAlreadyProcessed
        );
        OrderNonces::<T>::insert(&who, nonce, order_id);
        Orders::<T>::insert(order_id, OrderStatus::Cancelled);
        ApprovedOrders::<T>::remove(order_id);
        Self::deposit_event(Event::OrderCancelled {
            order_id,
            signer: who,
        });
        Ok(())
    }

    pub fn derive_authorized_order_id(nonce: u64, order: &VersionedOrder<AccountId32>) -> H256 {
        let mut bytes = ORDER_ID_DOMAIN.to_vec();
        (
            frame_system::Pallet::<T>::block_hash(
                frame_system::pallet_prelude::BlockNumberFor::<T>::zero(),
            ),
            order.signer(),
            nonce,
            order,
        )
            .encode_to(&mut bytes);
        H256(sp_core::hashing::blake2_256(&bytes))
    }

    /// Full-strength owner approval; H256 remains only the shared bookkeeping ID.
    pub fn order_approval_commitment(nonce: u64, order: &VersionedOrder<AccountId32>) -> [u8; 64] {
        let terms = (
            frame_system::Pallet::<T>::block_hash(
                frame_system::pallet_prelude::BlockNumberFor::<T>::zero(),
            ),
            order.signer(),
            nonce,
            order,
        )
            .encode();
        subtensor_hashed::mldsa::shake256_64(&[b"bittensor/ms/v2/order-approval", &terms])
    }

    pub(crate) fn check_order_nonce(owner: &AccountId32, nonce: u64, id: H256) -> DispatchResult {
        if let Some(existing) = OrderNonces::<T>::get(owner, nonce) {
            ensure!(existing == id, Error::<T>::OrderNonceUsed);
        }
        Ok(())
    }

    pub(crate) fn validate_authorized_order(
        signed: &AuthorizedOrder,
        order: &OrderView<AccountId32>,
        id: H256,
        now_ms: u64,
        price: substrate_fixed::types::U64F64,
        relayer: &AccountId32,
    ) -> Result<(u64, Option<H256>), sp_runtime::DispatchError> {
        use frame_support::traits::{Contains, Get};
        ensure!(!order.netuid.is_root(), Error::<T>::RootNetUidNotAllowed);
        ensure!(
            T::AuthenticatedOrderSignerFilter::contains(&order.signer),
            Error::<T>::OrderSignerFrozen
        );
        ensure!(
            order.chain_id == T::ChainId::get(),
            Error::<T>::ChainIdMismatch
        );
        Self::check_order_nonce(&order.signer, signed.nonce, id)?;
        let (amount, provider) =
            Self::validate_order_execution(order, signed.partial_fill, id, now_ms, price, relayer)?;
        match &signed.authorization {
            OrderAuthorization::AccountApproved => {
                ensure!(
                    ApprovedOrders::<T>::get(id)
                        == Some(Self::order_approval_commitment(signed.nonce, &signed.order)),
                    Error::<T>::OrderNotApproved
                );
            }
        }
        Ok((amount, provider))
    }

    #[transactional]
    pub(crate) fn try_execute_authorized_order(
        signed: AuthorizedOrder,
        id: H256,
        relayer: &AccountId32,
    ) -> DispatchResult {
        use frame_support::traits::UnixTime;
        let order = signed.order.view();
        let now_ms = T::TimeProvider::now().as_millis() as u64;
        let price = T::SwapInterface::current_alpha_price(order.netuid);
        let (amount, provider) =
            Self::validate_authorized_order(&signed, &order, id, now_ms, price, relayer)?;
        OrderNonces::<T>::insert(&order.signer, signed.nonce, id);
        Self::execute_validated_order(&order, signed.partial_fill, id, amount, provider)?;
        Ok(())
    }
    #[transactional]
    pub(crate) fn do_execute_authenticated_batch(
        netuid: NetUid,
        orders: BoundedVec<SignedOrder<AccountId32>, T::MaxOrdersPerBatch>,
        relayer: &AccountId32,
    ) -> DispatchResult {
        use alloc::collections::BTreeSet;
        use frame_support::traits::{Get, UnixTime};
        ensure!(!netuid.is_root(), Error::<T>::RootNetUidNotAllowed);
        ensure!(
            orders.len() <= T::MaxOrdersPerBatch::get() as usize,
            Error::<T>::OrderBatchTooLarge
        );
        let now_ms = T::TimeProvider::now().as_millis() as u64;
        let price = T::SwapInterface::current_alpha_price(netuid);
        let mut buys: BoundedVec<OrderEntry<AccountId32>, T::MaxOrdersPerBatch> = BoundedVec::new();
        let mut sells: BoundedVec<OrderEntry<AccountId32>, T::MaxOrdersPerBatch> =
            BoundedVec::new();
        let mut seen_ids = BTreeSet::new();
        let mut seen_providers = BTreeSet::new();
        for signed in orders {
            let order = signed.order.view();
            let id = Self::derive_order_id(&signed.order);
            ensure!(seen_ids.insert(id), Error::<T>::DuplicateOrderInBatch);
            ensure!(order.netuid == netuid, Error::<T>::OrderNetUidMismatch);
            let (amount, provider) = Self::is_order_valid(&signed, id, now_ms, price, relayer)?;
            if let Some(provider) = provider {
                ensure!(seen_providers.insert(provider), Error::<T>::NoLinkedOutput);
            }
            // This reservation is inside the same storage transaction as all
            // fills. It also catches conflicting nonces within a single batch.
            if let Some((nonce, _)) = signed.order.authorized_terms() {
                OrderNonces::<T>::insert(&order.signer, nonce, id);
            }
            let entry = Self::classify_order(&order, signed.partial_fill, id, amount, provider);
            let side = if entry.side.is_buy() {
                &mut buys
            } else {
                &mut sells
            };
            side.try_push(entry)
                .map_err(|_| Error::<T>::OrderBatchTooLarge)?;
        }
        Self::execute_classified_batch(netuid, price, buys, sells)?;
        Ok(())
    }
}
