//! Proxy pallet helpers: deposits, announcements, pure accounts, and dispatch.

use super::*;

impl<T: Config> Pallet<T> {
    /// Read [`Proxies`] for `account`: `(delegates, reserved_deposit)`.
    pub fn proxies(
        account: T::AccountId,
    ) -> (
        BoundedVec<ProxyDefinition<T::AccountId, T::ProxyType, BlockNumberFor<T>>, T::MaxProxies>,
        BalanceOf<T>,
    ) {
        Proxies::<T>::get(account)
    }

    /// Read [`Announcements`] for `account`: `(pending, reserved_deposit)`.
    pub fn announcements(
        account: T::AccountId,
    ) -> (
        BoundedVec<Announcement<T::AccountId, CallHashOf<T>, BlockNumberFor<T>>, T::MaxPending>,
        BalanceOf<T>,
    ) {
        Announcements::<T>::get(account)
    }

    /// Calculate the address of an pure account.
    ///
    /// - `who`: The spawner account.
    /// - `proxy_type`: The type of the proxy that the sender will be registered as over the
    ///   new account. This will almost always be the most permissive `ProxyType` possible to
    ///   allow for maximum flexibility.
    /// - `index`: A disambiguation index, in case this is called multiple times in the same
    ///   transaction (e.g. with `utility::batch`). Unless you're using `batch` you probably just
    ///   want to use `0`.
    /// - `maybe_when`: The block height and extrinsic index of when the pure account was
    ///   created. None to use current block height and extrinsic index.
    pub fn pure_account(
        who: &T::AccountId,
        proxy_type: &T::ProxyType,
        index: u16,
        maybe_when: Option<(BlockNumberFor<T>, u32)>,
    ) -> Result<T::AccountId, DispatchError> {
        let (height, ext_index) = maybe_when.unwrap_or_else(|| {
            (
                T::BlockNumberProvider::current_block_number(),
                frame_system::Pallet::<T>::extrinsic_index().unwrap_or_default(),
            )
        });
        let entropy = (
            b"modlpy/proxy____",
            who,
            height,
            ext_index,
            proxy_type,
            index,
        )
            .using_encoded(blake2_256);

        T::AccountId::decode(&mut TrailingZeroInput::new(entropy.as_ref()))
            .map_err(|_| Error::<T>::InvalidDerivedAccountId.into())
    }

    /// Register a proxy account for the delegator that is able to make calls on its behalf.
    ///
    /// Parameters:
    /// - `delegator`: The delegator account.
    /// - `delegatee`: The account that the `delegator` would like to make a proxy.
    /// - `proxy_type`: The permissions allowed for this proxy account.
    /// - `delay`: The announcement period required of the initial proxy. Will generally be
    ///   zero.
    pub fn add_proxy_delegate(
        delegator: &T::AccountId,
        delegatee: T::AccountId,
        proxy_type: T::ProxyType,
        delay: BlockNumberFor<T>,
    ) -> DispatchResult {
        ensure!(delegator != &delegatee, Error::<T>::NoSelfProxy);
        Proxies::<T>::try_mutate(delegator, |(proxies, deposit)| {
            let proxy_def = ProxyDefinition {
                delegate: delegatee.clone(),
                proxy_type: proxy_type.clone(),
                delay,
            };
            let i = proxies
                .binary_search(&proxy_def)
                .err()
                .ok_or(Error::<T>::Duplicate)?;
            proxies
                .try_insert(i, proxy_def)
                .map_err(|_| Error::<T>::TooMany)?;
            let new_deposit = Self::deposit(proxies.len() as u32);
            if new_deposit > *deposit {
                T::Currency::reserve(delegator, new_deposit.saturating_sub(*deposit))?;
            } else if new_deposit < *deposit {
                T::Currency::unreserve(delegator, (*deposit).saturating_sub(new_deposit));
            }
            *deposit = new_deposit;
            Self::deposit_event(Event::<T>::ProxyAdded {
                delegator: delegator.clone(),
                delegatee,
                proxy_type,
                delay,
            });
            Ok(())
        })
    }

    /// Unregister a proxy account for the delegator.
    ///
    /// Parameters:
    /// - `delegator`: The delegator account.
    /// - `delegatee`: The account that the `delegator` would like to make a proxy.
    /// - `proxy_type`: The permissions allowed for this proxy account.
    /// - `delay`: The announcement period required of the initial proxy. Will generally be
    ///   zero.
    pub fn remove_proxy_delegate(
        delegator: &T::AccountId,
        delegatee: T::AccountId,
        proxy_type: T::ProxyType,
        delay: BlockNumberFor<T>,
    ) -> DispatchResult {
        Proxies::<T>::try_mutate_exists(delegator, |x| {
            let (mut proxies, old_deposit) = x.take().ok_or(Error::<T>::NotFound)?;
            let proxy_def = ProxyDefinition {
                delegate: delegatee.clone(),
                proxy_type: proxy_type.clone(),
                delay,
            };
            let i = proxies
                .binary_search(&proxy_def)
                .ok()
                .ok_or(Error::<T>::NotFound)?;
            proxies.remove(i);
            let new_deposit = Self::deposit(proxies.len() as u32);
            if new_deposit > old_deposit {
                T::Currency::reserve(delegator, new_deposit.saturating_sub(old_deposit))?;
            } else if new_deposit < old_deposit {
                T::Currency::unreserve(delegator, old_deposit.saturating_sub(new_deposit));
            }
            if !proxies.is_empty() {
                *x = Some((proxies, new_deposit))
            }
            // Clean up real-pays-fee flag for this specific proxy relationship
            RealPaysFee::<T>::remove(delegator, &delegatee);

            Self::deposit_event(Event::<T>::ProxyRemoved {
                delegator: delegator.clone(),
                delegatee,
                proxy_type,
                delay,
            });
            Ok(())
        })
    }

    /// Required reserve for `num_proxies` entries: `base + factor * n` (zero when `n == 0`).
    pub fn deposit(num_proxies: u32) -> BalanceOf<T> {
        if num_proxies == 0 {
            Zero::zero()
        } else {
            T::ProxyDepositBase::get()
                .saturating_add(T::ProxyDepositFactor::get().saturating_mul(num_proxies.into()))
        }
    }

    /// Top up or release reserved funds so the lock matches `base + factor * len`.
    ///
    /// Returns `None` when `len == 0` (caller should clear the storage entry).
    pub(crate) fn recompute_reserved_deposit(
        who: &T::AccountId,
        old_deposit: BalanceOf<T>,
        base: BalanceOf<T>,
        factor: BalanceOf<T>,
        len: usize,
    ) -> Result<Option<BalanceOf<T>>, DispatchError> {
        let new_deposit = if len == 0 {
            BalanceOf::<T>::zero()
        } else {
            base.saturating_add(factor.saturating_mul((len as u32).into()))
        };
        if new_deposit > old_deposit {
            T::Currency::reserve(who, new_deposit.saturating_sub(old_deposit))?;
        } else if new_deposit < old_deposit {
            let excess = old_deposit.saturating_sub(new_deposit);
            let remaining_unreserved = T::Currency::unreserve(who, excess);
            if !remaining_unreserved.is_zero() {
                defensive!(
                    "Failed to unreserve full amount. (Requested, Actual)",
                    (excess, excess.saturating_sub(remaining_unreserved))
                );
            }
        }
        Ok(if len == 0 { None } else { Some(new_deposit) })
    }

    /// Keep announcements for which `f` returns true; fails with [`Error::NotFound`] if none removed.
    pub(crate) fn retain_proxy_announcements<
        F: FnMut(&Announcement<T::AccountId, CallHashOf<T>, BlockNumberFor<T>>) -> bool,
    >(
        delegate: &T::AccountId,
        f: F,
    ) -> DispatchResult {
        Announcements::<T>::try_mutate_exists(delegate, |x| {
            let (mut pending, old_deposit) = x.take().ok_or(Error::<T>::NotFound)?;
            let orig_pending_len = pending.len();
            pending.retain(f);
            ensure!(orig_pending_len > pending.len(), Error::<T>::NotFound);
            *x = Self::recompute_reserved_deposit(
                delegate,
                old_deposit,
                T::AnnouncementDepositBase::get(),
                T::AnnouncementDepositFactor::get(),
                pending.len(),
            )?
            .map(|deposit| (pending, deposit));
            Ok(())
        })
    }

    /// Locate the proxy definition for `delegate` acting on `real`, optionally matching `force_proxy_type`.
    pub fn find_proxy(
        real: &T::AccountId,
        delegate: &T::AccountId,
        force_proxy_type: Option<T::ProxyType>,
    ) -> Result<ProxyDefinition<T::AccountId, T::ProxyType, BlockNumberFor<T>>, DispatchError> {
        let f = |x: &ProxyDefinition<T::AccountId, T::ProxyType, BlockNumberFor<T>>| -> bool {
            &x.delegate == delegate && force_proxy_type.as_ref().is_none_or(|y| &x.proxy_type == y)
        };
        Ok(Proxies::<T>::get(real)
            .0
            .into_iter()
            .find(f)
            .ok_or(Error::<T>::NotProxy)?)
    }

    /// Dispatch `call` as `real` under `def.proxy_type` filters (privilege escalation guards included).
    pub(crate) fn do_proxy(
        def: ProxyDefinition<T::AccountId, T::ProxyType, BlockNumberFor<T>>,
        real: T::AccountId,
        call: <T as Config>::RuntimeCall,
    ) {
        use frame::traits::{InstanceFilter as _, OriginTrait as _};
        // This is a freshly authenticated new account, the origin restrictions doesn't apply.
        let mut origin: T::RuntimeOrigin = frame_system::RawOrigin::Signed(real.clone()).into();
        origin.add_filter(move |c: &<T as frame_system::Config>::RuntimeCall| {
            let c = <T as Config>::RuntimeCall::from_ref(c);
            // We make sure the proxy call does access this pallet to change modify proxies.
            match c.is_sub_type() {
                // Proxy call cannot add or remove a proxy with more permissions than it already
                // has.
                Some(Call::add_proxy { proxy_type, .. })
                | Some(Call::remove_proxy { proxy_type, .. })
                    if !def.proxy_type.is_superset(proxy_type) =>
                {
                    false
                }
                // Proxy call cannot remove all proxies or kill pure proxies unless it has full
                // permissions.
                Some(Call::remove_proxies { .. }) | Some(Call::kill_pure { .. })
                    if def.proxy_type != T::ProxyType::default() =>
                {
                    false
                }
                _ => def.proxy_type.filter(c),
            }
        });
        let e = call.dispatch(origin);

        LastCallResult::<T>::insert(real, e.map(|_| ()).map_err(|e| e.error));

        Self::deposit_event(Event::ProxyExecuted {
            result: e.map(|_| ()).map_err(|e| e.error),
        });
    }

    /// Removes all proxy delegates for a given delegator.
    ///
    /// Parameters:
    /// - `delegator`: The delegator account.
    pub fn remove_all_proxy_delegates(delegator: &T::AccountId) {
        let (_, old_deposit) = Proxies::<T>::take(delegator);
        T::Currency::unreserve(delegator, old_deposit);
        // Clean up all real-pays-fee flags for this delegator
        let _ = RealPaysFee::<T>::clear_prefix(delegator, u32::MAX, None);
    }

    /// Check if the real account has opted in to paying fees for a specific delegate.
    pub fn is_real_pays_fee(real: &T::AccountId, delegate: &T::AccountId) -> bool {
        RealPaysFee::<T>::contains_key(real, delegate)
    }
}
