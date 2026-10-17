//! Local admission limits before transaction-pool runtime verification.
//!
//! The pinned SDK has two validation workers, but callers waiting to enqueue work
//! are not bounded and `submit_local` bypasses those workers. This shared wrapper
//! bounds ML-DSA RPC, network and offchain submissions, including invalid proofs.
//! Legacy-only requests retain the existing pool's behavior. Limits are local
//! operator policy, not measured verification throughput or a consensus rule.
//! Existing ready/future pool limits still govern retained data.

use crate::client::FullClient;
use async_trait::async_trait;
use node_subtensor_runtime::opaque::Block;
use sc_transaction_pool::{TransactionPoolHandle, TransactionPoolWrapper};
use sc_transaction_pool_api::{
    ChainEvent, ImportNotificationStream, LocalTransactionPool, MaintainedTransactionPool,
    PoolStatus, ReadyTransactions, TransactionPool, TransactionSource, TransactionStatusStreamFor,
    TxInvalidityReportMap,
};
use sp_core::H256;
use sp_runtime::{
    OpaqueExtrinsic,
    codec::{Compact, Decode, Encode, Output},
};
use std::{
    collections::{HashMap, HashSet},
    future::Future,
    pin::Pin,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

// Enough for the runtime's complete 10 MiB block budget, including large calls.
const MAX_TRANSACTION_BYTES: usize = 10 * 1024 * 1024;
const MAX_PENDING_BYTES: usize = 64 * 1024 * 1024;
const MAX_PENDING: usize = 64;
const MAX_MLDSA_PENDING: usize = 2;
const MLDSA_BURST: u32 = 16;
const MLDSA_REFILL: Duration = Duration::from_millis(125);
const MLDSA_ADDRESS_BYTES: u32 = 5367;

type Pool = TransactionPoolHandle<Block, FullClient>;
type PoolError = <Pool as TransactionPool>::Error;
type PoolTransaction = <Pool as TransactionPool>::InPoolTransaction;
type Ready = Box<dyn ReadyTransactions<Item = Arc<PoolTransaction>> + Send>;

fn busy() -> PoolError {
    // The pinned network adapter penalizes peers for *every* pool error. Local
    // overload is not evidence of an invalid transaction, so keep this outside
    // IntoPoolError's consensus-invalid branch and let the sender retry later.
    PoolError::RuntimeApi("Local transaction admission capacity reached; retry later".into())
}

/// Keeps only the beginning of an encoded opaque extrinsic, never allocating a
/// second transaction-sized buffer before checking the admission byte budget.
struct Prefix {
    bytes: [u8; 64],
    len: usize,
}

impl Default for Prefix {
    fn default() -> Self {
        Self {
            bytes: [0; 64],
            len: 0,
        }
    }
}

impl Output for Prefix {
    fn write(&mut self, bytes: &[u8]) {
        for (target, byte) in self.bytes.iter_mut().skip(self.len).zip(bytes) {
            *target = *byte;
            self.len = self.len.saturating_add(1);
        }
    }
}

fn mldsa_account(xt: &OpaqueExtrinsic) -> Option<[u8; 32]> {
    let mut prefix = Prefix::default();
    xt.encode_to(&mut prefix);
    let input = &mut prefix.bytes.get(..prefix.len)?;
    Compact::<u32>::decode(input).ok()?;
    // Signed v4 followed by MultiAddress::Raw. Treat every matching length as
    // expensive, even an unsupported profile or a malformed signature carrier.
    if u8::decode(input).ok()? != 0x84 || u8::decode(input).ok()? != 2 {
        return None;
    }
    if Compact::<u32>::decode(input).ok()?.0 != MLDSA_ADDRESS_BYTES {
        return None;
    }
    <[u8; 32]>::decode(input).ok()
}

#[derive(Default)]
struct Request {
    count: usize,
    bytes: usize,
    accounts: Vec<[u8; 32]>,
}

impl Request {
    fn from_transactions(xts: &[OpaqueExtrinsic]) -> Result<Option<Self>, PoolError> {
        // Preserve legacy submission behavior, including batches larger than the
        // new ML-DSA admission budget. This scan retains only a bounded prefix.
        if !xts.iter().any(|xt| mldsa_account(xt).is_some()) {
            return Ok(None);
        }
        if xts.len() > MAX_PENDING {
            return Err(busy());
        }
        let mut request = Self {
            count: xts.len(),
            ..Self::default()
        };
        for xt in xts {
            let bytes = xt.encoded_size();
            if bytes > MAX_TRANSACTION_BYTES {
                return Err(busy());
            }
            request.bytes = request.bytes.checked_add(bytes).ok_or_else(busy)?;
            if request.bytes > MAX_PENDING_BYTES {
                return Err(busy());
            }
            if let Some(account) = mldsa_account(xt) {
                if request.accounts.len() == MAX_MLDSA_PENDING {
                    return Err(busy());
                }
                request.accounts.push(account);
            }
        }
        Ok(Some(request))
    }
}

struct State {
    count: usize,
    bytes: usize,
    accounts: HashSet<[u8; 32]>,
    tokens: u32,
    refilled_at: Instant,
}

impl State {
    fn new(now: Instant) -> Self {
        Self {
            count: 0,
            bytes: 0,
            accounts: HashSet::new(),
            tokens: MLDSA_BURST,
            refilled_at: now,
        }
    }

    fn admit(&mut self, request: &Request, now: Instant) -> Result<(), PoolError> {
        let count = self.count.checked_add(request.count).ok_or_else(busy)?;
        let bytes = self.bytes.checked_add(request.bytes).ok_or_else(busy)?;
        if count > MAX_PENDING
            || bytes > MAX_PENDING_BYTES
            || self.accounts.len().saturating_add(request.accounts.len()) > MAX_MLDSA_PENDING
        {
            return Err(busy());
        }
        // No eviction: an active identity remains present until its actual
        // verification finishes. Different payloads/generations cannot bypass it.
        let mut unique = HashSet::new();
        for account in &request.accounts {
            if self.accounts.contains(account) || !unique.insert(*account) {
                return Err(busy());
            }
        }
        if let Some(elapsed) = now.checked_duration_since(self.refilled_at) {
            let refill = elapsed
                .as_nanos()
                .checked_div(MLDSA_REFILL.as_nanos())
                .unwrap_or(0);
            if refill > 0 {
                self.tokens = self
                    .tokens
                    .saturating_add(refill.min(u128::from(MLDSA_BURST)) as u32)
                    .min(MLDSA_BURST);
                // Discarding the fractional interval is deliberately conservative.
                self.refilled_at = now;
            }
        }
        let needed = request.accounts.len() as u32;
        if self.tokens < needed {
            return Err(busy());
        }
        self.tokens = self.tokens.saturating_sub(needed);
        self.count = count;
        self.bytes = bytes;
        self.accounts.extend(unique);
        Ok(())
    }

    fn release(&mut self, request: &Request) {
        self.count = self.count.saturating_sub(request.count);
        self.bytes = self.bytes.saturating_sub(request.bytes);
        for account in &request.accounts {
            self.accounts.remove(account);
        }
        // Invalid proofs and caller cancellation never refund attempt tokens.
    }
}

struct Permit {
    state: Arc<Mutex<State>>,
    request: Request,
}

impl Drop for Permit {
    fn drop(&mut self) {
        if let Ok(mut state) = self.state.lock() {
            state.release(&self.request);
        }
        // Poisoned state stays fail-closed; admission also refuses its lock.
    }
}

/// Dropping a waiting RPC future must not drop the permit while SDK workers
/// still hold its verification job. Tokio tasks run to completion independently
/// of the requesting task; the permit belongs to that completion task.
async fn finish_admitted<R: Send + 'static>(
    permit: Permit,
    work: impl Future<Output = Result<R, PoolError>> + Send + 'static,
) -> Result<R, PoolError> {
    tokio::spawn(async move {
        let _permit = permit;
        work.await
    })
    .await
    .map_err(|_| busy())?
}

struct AdmissionPool {
    inner: Arc<Pool>,
    state: Arc<Mutex<State>>,
}

impl AdmissionPool {
    fn admit(&self, xts: &[OpaqueExtrinsic]) -> Result<Option<Permit>, PoolError> {
        let Some(request) = Request::from_transactions(xts)? else {
            return Ok(None);
        };
        self.state
            .lock()
            .map_err(|_| busy())?
            .admit(&request, Instant::now())?;
        Ok(Some(Permit {
            state: self.state.clone(),
            request,
        }))
    }
}

pub(super) fn wrap(inner: Pool) -> Pool {
    TransactionPoolWrapper(Box::new(AdmissionPool {
        inner: Arc::new(inner),
        state: Arc::new(Mutex::new(State::new(Instant::now()))),
    }))
}

#[async_trait]
impl TransactionPool for AdmissionPool {
    type Block = Block;
    type Hash = H256;
    type InPoolTransaction = PoolTransaction;
    type Error = PoolError;

    async fn submit_at(
        &self,
        at: H256,
        source: TransactionSource,
        xts: Vec<OpaqueExtrinsic>,
    ) -> Result<Vec<Result<H256, PoolError>>, PoolError> {
        let Some(permit) = self.admit(&xts)? else {
            return self.inner.submit_at(at, source, xts).await;
        };
        let inner = self.inner.clone();
        finish_admitted(
            permit,
            async move { inner.submit_at(at, source, xts).await },
        )
        .await
    }

    async fn submit_one(
        &self,
        at: H256,
        source: TransactionSource,
        xt: OpaqueExtrinsic,
    ) -> Result<H256, PoolError> {
        let Some(permit) = self.admit(std::slice::from_ref(&xt))? else {
            return self.inner.submit_one(at, source, xt).await;
        };
        let inner = self.inner.clone();
        finish_admitted(
            permit,
            async move { inner.submit_one(at, source, xt).await },
        )
        .await
    }

    async fn submit_and_watch(
        &self,
        at: H256,
        source: TransactionSource,
        xt: OpaqueExtrinsic,
    ) -> Result<Pin<Box<TransactionStatusStreamFor<Self>>>, PoolError> {
        let Some(permit) = self.admit(std::slice::from_ref(&xt))? else {
            return self.inner.submit_and_watch(at, source, xt).await;
        };
        let inner = self.inner.clone();
        finish_admitted(permit, async move {
            inner.submit_and_watch(at, source, xt).await
        })
        .await
    }

    async fn ready_at(&self, at: H256) -> Ready {
        self.inner.ready_at(at).await
    }
    fn ready(&self) -> Ready {
        self.inner.ready()
    }
    fn report_invalid(
        &self,
        at: Option<H256>,
        errors: TxInvalidityReportMap<H256>,
    ) -> Vec<Arc<PoolTransaction>> {
        self.inner.report_invalid(at, errors)
    }
    fn futures(&self) -> Vec<PoolTransaction> {
        self.inner.futures()
    }
    fn status(&self) -> PoolStatus {
        self.inner.status()
    }
    fn import_notification_stream(&self) -> ImportNotificationStream<H256> {
        self.inner.import_notification_stream()
    }
    fn on_broadcasted(&self, propagations: HashMap<H256, Vec<String>>) {
        self.inner.on_broadcasted(propagations)
    }
    fn hash_of(&self, xt: &OpaqueExtrinsic) -> H256 {
        self.inner.hash_of(xt)
    }
    fn ready_transaction(&self, hash: &H256) -> Option<Arc<PoolTransaction>> {
        self.inner.ready_transaction(hash)
    }
    async fn ready_at_with_timeout(&self, at: H256, timeout: Duration) -> Ready {
        self.inner.ready_at_with_timeout(at, timeout).await
    }
}

#[async_trait]
impl MaintainedTransactionPool for AdmissionPool {
    async fn maintain(&self, event: ChainEvent<Block>) {
        self.inner.maintain(event).await;
    }
}

impl LocalTransactionPool for AdmissionPool {
    type Block = Block;
    type Hash = H256;
    type Error = PoolError;

    fn submit_local(&self, at: H256, xt: OpaqueExtrinsic) -> Result<H256, PoolError> {
        let _permit = self.admit(std::slice::from_ref(&xt))?;
        self.inner.submit_local(at, xt)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::arithmetic_side_effects)]
mod tests {
    use super::*;

    #[test]
    fn local_overload_is_not_reported_as_an_invalid_peer_transaction() {
        use sc_transaction_pool_api::error::IntoPoolError;

        assert!(busy().into_pool_error().is_err());
    }

    fn request(account: u8) -> Request {
        Request {
            count: 1,
            bytes: 6000,
            accounts: vec![[account; 32]],
        }
    }

    #[test]
    fn concurrent_identities_and_byte_limits_cannot_be_evicted_or_overflowed() {
        let now = Instant::now();
        let mut state = State::new(now);
        state.admit(&request(1), now).unwrap();
        assert!(state.admit(&request(1), now).is_err());
        state.admit(&request(2), now).unwrap();
        assert!(state.admit(&request(3), now).is_err());
        assert!(state.accounts.contains(&[1; 32]));
        state.release(&request(1));
        state.admit(&request(3), now).unwrap();
        assert!(
            state
                .admit(
                    &Request {
                        count: usize::MAX,
                        ..Request::default()
                    },
                    now
                )
                .is_err()
        );
        assert!(
            state
                .admit(
                    &Request {
                        bytes: usize::MAX,
                        ..Request::default()
                    },
                    now
                )
                .is_err()
        );
        assert_eq!(state.count, 2);
        assert_eq!(state.bytes, 12_000);
    }

    #[test]
    fn failed_proof_completion_does_not_refund_tokens_or_allow_clock_rollback() {
        let now = Instant::now();
        let mut state = State::new(now);
        for _ in 0..MLDSA_BURST {
            state.admit(&request(1), now).unwrap();
            state.release(&request(1));
        }
        assert!(state.admit(&request(2), now).is_err());
        let later = now + MLDSA_REFILL;
        state.admit(&request(2), later).unwrap();
        state.release(&request(2));
        assert!(state.admit(&request(2), now).is_err());
        assert!(state.admit(&request(2), later).is_err());
    }

    #[test]
    fn header_classification_and_batch_limits_do_not_require_full_decoding() {
        let mut body = vec![0x84, 2];
        body.extend(Compact(MLDSA_ADDRESS_BYTES).encode());
        body.extend([7; 32]);
        // Deliberately malformed remainder still receives the expensive budget.
        let xt = OpaqueExtrinsic::from_bytes(&body.encode()).unwrap();
        assert_eq!(mldsa_account(&xt), Some([7; 32]));
        assert!(Request::from_transactions(&vec![xt.clone(); MAX_PENDING + 1]).is_err());
        assert!(Request::from_transactions(&vec![xt; MAX_MLDSA_PENDING + 1]).is_err());
        let ordinary = OpaqueExtrinsic::from_bytes(&vec![4, 1, 2, 3].encode()).unwrap();
        assert_eq!(mldsa_account(&ordinary), None);
        assert!(Request::from_transactions(&[ordinary]).unwrap().is_none());
    }

    #[test]
    fn legacy_only_requests_bypass_new_count_and_byte_budgets() {
        let ordinary = OpaqueExtrinsic::from_bytes(&vec![4, 1, 2, 3].encode()).unwrap();
        assert!(
            Request::from_transactions(&vec![ordinary; MAX_PENDING + 1])
                .unwrap()
                .is_none()
        );
        let large =
            OpaqueExtrinsic::from_bytes(&vec![4; MAX_TRANSACTION_BYTES + 1].encode()).unwrap();
        assert!(Request::from_transactions(&[large]).unwrap().is_none());
    }

    #[test]
    fn cancelled_request_holds_admission_until_validation_finishes() {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(async {
                let state = Arc::new(Mutex::new(State::new(Instant::now())));
                let admitted = request(1);
                state
                    .lock()
                    .unwrap()
                    .admit(&admitted, Instant::now())
                    .unwrap();
                let permit = Permit {
                    state: state.clone(),
                    request: admitted,
                };
                let (started_tx, started_rx) = futures::channel::oneshot::channel();
                let (finish_tx, finish_rx) = futures::channel::oneshot::channel();
                let waiting = tokio::spawn(finish_admitted(permit, async move {
                    started_tx.send(()).unwrap();
                    finish_rx.await.unwrap();
                    Ok(())
                }));
                started_rx.await.unwrap();
                waiting.abort();
                assert!(
                    state
                        .lock()
                        .unwrap()
                        .admit(&request(1), Instant::now())
                        .is_err()
                );
                finish_tx.send(()).unwrap();
                tokio::task::yield_now().await;
                assert_eq!(state.lock().unwrap().count, 0);
                state
                    .lock()
                    .unwrap()
                    .admit(&request(1), Instant::now())
                    .unwrap();
            });
    }
}
