//! Admission for offchain post-quantum authentication, including raw RPC calls.
//!
//! Only encoded ML-DSA/Falcon extrinsics consume this budget. Classical requests,
//! queries, initialization and simulations retain the base executor behavior.
//! Clones share reservations, and a permit lives until synchronous Wasm execution
//! returns, even if the requesting RPC future is cancelled. Pool revalidation has
//! separate capacity; block import and authoring use the unrestricted Onchain path.

use crate::pq_execution::{self, Work};
use sc_executor::{RuntimeVersion, RuntimeVersionOf, error};
use sp_core::traits::{
    CallContext, CodeExecutor, Externalities, ExternalitiesExt, ReadRuntimeVersion, RuntimeCode,
};
use std::{
    sync::{Arc, Condvar, Mutex},
    time::{Duration, Instant},
};

const MAX_ACTIVE: usize = 2;
const MAX_ACTIVE_BYTES: usize = 16 * 1024 * 1024;
const BURST: u32 = 16;
const REFILL: Duration = Duration::from_millis(125);

/// Only the local benchmark command may construct an unrestricted executor.
/// RPC input and runtime feature flags cannot select this mode.
#[derive(Clone, Copy)]
pub enum ExecutionMode {
    Node,
    #[cfg(any(test, feature = "runtime-benchmarks"))]
    Benchmark,
}

fn busy() -> error::Error {
    error::Error::Other(fp_rpc::LOCAL_EXECUTION_CAPACITY_ERROR.into())
}

struct State {
    active: usize,
    bytes: usize,
    tokens: u32,
    refilled: Instant,
}

impl State {
    fn new(now: Instant) -> Self {
        Self {
            active: 0,
            bytes: 0,
            tokens: BURST,
            refilled: now,
        }
    }

    fn acquire(&mut self, work: Work, now: Instant, charge_rate: bool) -> Result<(), error::Error> {
        let next_bytes = self.bytes.checked_add(work.bytes).ok_or_else(busy)?;
        if self.active >= MAX_ACTIVE || next_bytes > MAX_ACTIVE_BYTES {
            return Err(busy());
        }
        if charge_rate {
            if let Some(elapsed) = now.checked_duration_since(self.refilled) {
                let added = elapsed
                    .as_nanos()
                    .checked_div(REFILL.as_nanos())
                    .unwrap_or(0);
                if added != 0 {
                    self.tokens = self
                        .tokens
                        .saturating_add(added.min(u128::from(BURST)) as u32)
                        .min(BURST);
                    // Discard fractional credit conservatively; never move backwards.
                    self.refilled = now;
                }
            }
            // A batch larger than the burst requires a full bucket, rather than
            // becoming permanently inadmissible. Its bytes and concurrency are
            // still bounded. Count PQ proofs only, not classical neighbours.
            let cost = work.proofs.min(BURST as usize) as u32;
            if self.tokens < cost {
                return Err(busy());
            }
            self.tokens = self.tokens.saturating_sub(cost);
        }
        self.active = self.active.saturating_add(1);
        self.bytes = next_bytes;
        Ok(())
    }
}

struct Permit {
    state: Arc<Mutex<State>>,
    bytes: usize,
    wake: Option<Arc<Condvar>>,
}

impl Drop for Permit {
    fn drop(&mut self) {
        // A poisoned admission lock fails closed on subsequent acquisition.
        if let Ok(mut state) = self.state.lock() {
            state.active = state.active.saturating_sub(1);
            state.bytes = state.bytes.saturating_sub(self.bytes);
        }
        if let Some(wake) = &self.wake {
            wake.notify_all();
        }
    }
}

/// Shared PQ execution capacity, with a separate trusted pool reservation.
#[derive(Clone)]
pub struct AdmissionExecutor<E> {
    inner: E,
    mode: ExecutionMode,
    raw: Arc<Mutex<State>>,
    pool: Arc<Mutex<State>>,
    pool_available: Arc<Condvar>,
}

impl<E> AdmissionExecutor<E> {
    pub fn new(inner: E, mode: ExecutionMode) -> Self {
        Self {
            inner,
            mode,
            raw: Arc::new(Mutex::new(State::new(Instant::now()))),
            pool: Arc::new(Mutex::new(State::new(Instant::now()))),
            pool_available: Arc::new(Condvar::new()),
        }
    }

    fn acquire_raw(&self, work: Work) -> Result<Permit, error::Error> {
        self.raw
            .lock()
            .map_err(|_| busy())?
            .acquire(work, Instant::now(), true)?;
        Ok(Permit {
            state: self.raw.clone(),
            bytes: work.bytes,
            wake: None,
        })
    }

    fn acquire_pool(&self, work: Work) -> Result<Permit, error::Error> {
        if work.bytes > MAX_ACTIVE_BYTES {
            return Err(busy());
        }
        let mut budget = self.pool.lock().map_err(|_| busy())?;
        // Only bounded pool workers and admitted local submissions can wait here.
        // Raw RPC has no PoolValidation extension. Ingress already charges the
        // attempt rate; maintenance/fork revalidation must not charge it again.
        while budget.acquire(work, Instant::now(), false).is_err() {
            budget = self.pool_available.wait(budget).map_err(|_| busy())?;
        }
        Ok(Permit {
            state: self.pool.clone(),
            bytes: work.bytes,
            wake: Some(self.pool_available.clone()),
        })
    }
}

impl<E: CodeExecutor<Error = error::Error>> CodeExecutor for AdmissionExecutor<E> {
    type Error = error::Error;

    fn call(
        &self,
        mut ext: &mut dyn Externalities,
        runtime_code: &RuntimeCode,
        method: &str,
        data: &[u8],
        context: CallContext,
    ) -> (Result<Vec<u8>, Self::Error>, bool) {
        if !matches!(self.mode, ExecutionMode::Node) || context == CallContext::Onchain {
            return self.inner.call(ext, runtime_code, method, data, context);
        }
        let work = match pq_execution::work(method, data) {
            Ok(work) => work,
            Err(_) => {
                return (
                    Err(error::Error::Other(
                        "Wallet benchmarks require the local benchmark command".into(),
                    )),
                    false,
                );
            }
        };
        let _permit = if work.proofs == 0 {
            None
        } else {
            let permit = if method == "TaggedTransactionQueue_validate_transaction"
                && ext
                    .extension::<sc_transaction_pool::PoolValidation>()
                    .is_some()
            {
                self.acquire_pool(work)
            } else {
                self.acquire_raw(work)
            };
            match permit {
                Ok(permit) => Some(permit),
                // Capacity is an executor error, never a signature/validity result.
                Err(error) => return (Err(error), false),
            }
        };
        self.inner.call(ext, runtime_code, method, data, context)
    }
}

impl<E: ReadRuntimeVersion> ReadRuntimeVersion for AdmissionExecutor<E> {
    fn read_runtime_version(
        &self,
        wasm_code: &[u8],
        ext: &mut dyn Externalities,
    ) -> Result<Vec<u8>, String> {
        self.inner.read_runtime_version(wasm_code, ext)
    }
}

impl<E: RuntimeVersionOf> RuntimeVersionOf for AdmissionExecutor<E> {
    fn runtime_version(
        &self,
        ext: &mut dyn Externalities,
        runtime_code: &RuntimeCode,
    ) -> error::Result<RuntimeVersion> {
        self.inner.runtime_version(ext, runtime_code)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::arithmetic_side_effects)]
mod tests {
    use super::*;
    use sp_runtime::codec::{Compact, Encode};
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        mpsc,
    };

    const VALIDATE: &str = "TaggedTransactionQueue_validate_transaction";
    const APPLY: &str = "BlockBuilder_apply_extrinsic";
    const PENDING: &str = "EthereumRuntimeRPCApi_pending_block";

    #[derive(Clone)]
    struct MockExecutor {
        calls: Arc<AtomicUsize>,
        behavior: Arc<AtomicUsize>,
        entered: mpsc::Sender<()>,
        resume: Arc<(Mutex<bool>, Condvar)>,
    }

    impl ReadRuntimeVersion for MockExecutor {
        fn read_runtime_version(
            &self,
            _: &[u8],
            _: &mut dyn Externalities,
        ) -> Result<Vec<u8>, String> {
            Ok(vec![1])
        }
    }

    impl CodeExecutor for MockExecutor {
        type Error = error::Error;
        fn call(
            &self,
            _: &mut dyn Externalities,
            _: &RuntimeCode,
            _: &str,
            _: &[u8],
            _: CallContext,
        ) -> (Result<Vec<u8>, Self::Error>, bool) {
            self.calls.fetch_add(1, Ordering::SeqCst);
            match self.behavior.load(Ordering::SeqCst) {
                1 => return (Err(error::Error::Other("mock runtime error".into())), false),
                2 => panic!("mock runtime panic"),
                3 => {
                    self.entered.send(()).unwrap();
                    let (lock, wake) = &*self.resume;
                    let mut resumed = lock.lock().unwrap();
                    while !*resumed {
                        resumed = wake.wait(resumed).unwrap();
                    }
                }
                _ => {}
            }
            (Ok(vec![]), false)
        }
    }

    fn executor() -> (AdmissionExecutor<MockExecutor>, mpsc::Receiver<()>) {
        let (entered, receiver) = mpsc::channel();
        (
            AdmissionExecutor::new(
                MockExecutor {
                    calls: Arc::new(AtomicUsize::new(0)),
                    behavior: Arc::new(AtomicUsize::new(0)),
                    entered,
                    resume: Arc::new((Mutex::new(false), Condvar::new())),
                },
                ExecutionMode::Node,
            ),
            receiver,
        )
    }

    fn execute(
        executor: &AdmissionExecutor<MockExecutor>,
        method: &str,
        data: &[u8],
        context: CallContext,
        pool: bool,
    ) -> Result<Vec<u8>, error::Error> {
        let mut ext = sp_io::TestExternalities::default();
        if pool {
            ext.register_extension(sc_transaction_pool::PoolValidation);
        }
        executor
            .call(&mut ext.ext(), &RuntimeCode::empty(), method, data, context)
            .0
    }

    // Syntactic envelopes exercise admission only; MockExecutor does not verify them.
    fn extrinsic(proof: Option<usize>, call_bytes: usize) -> Vec<u8> {
        let mut body = vec![0x84];
        if let Some(size) = proof {
            body.push(2);
            vec![0u8; size].encode_to(&mut body);
        } else {
            body.push(0);
            body.extend_from_slice(&[0; 32]);
        }
        body.resize(body.len() + call_bytes, 0);
        body.encode()
    }

    fn validation(proof: Option<usize>) -> Vec<u8> {
        let mut data = vec![2];
        data.extend(extrinsic(proof, 66));
        data.extend_from_slice(&[0; 32]);
        data
    }

    fn list(items: &[Vec<u8>]) -> Vec<u8> {
        let mut data = Compact(items.len() as u32).encode();
        for item in items {
            data.extend(item);
        }
        data
    }

    fn block(items: &[Vec<u8>]) -> Vec<u8> {
        let mut data = vec![0; 32];
        Compact(1u32).encode_to(&mut data);
        data.extend_from_slice(&[0; 64]);
        data.push(0); // Empty digest.
        data.extend(list(items));
        data
    }

    fn freeze_rate(state: &mut State) {
        state.refilled = Instant::now() + Duration::from_secs(60);
    }

    fn saturate(executor: &AdmissionExecutor<MockExecutor>) {
        for state in [&executor.raw, &executor.pool] {
            let mut state = state.lock().unwrap();
            state.active = MAX_ACTIVE;
            state.bytes = MAX_ACTIVE_BYTES;
            state.tokens = 0;
            freeze_rate(&mut state);
        }
    }

    #[test]
    fn ordinary_exports_ignore_saturated_pq_budgets() {
        let (executor, _) = executor();
        saturate(&executor);
        for method in [
            "SubnetInfoRuntimeApi_get_selective_metagraph",
            "SubnetInfoRuntimeApi_get_selective_mechagraph",
            "FutureApi_unknown",
            "Core_initialize_block",
            "BlockBuilder_finalize_block",
            "EthereumRuntimeRPCApi_initialize_pending_block",
            "EthereumRuntimeRPCApi_call",
            "EthereumRuntimeRPCApi_create",
            "ContractsApi_call",
            "ContractsApi_instantiate",
            "ContractsApi_upload_code",
            "BlockBuilder_check_inherents",
            "OffchainWorkerApi_offchain_worker",
            "ShieldApi_try_decode_shielded_tx",
            "ShieldApi_try_unshield_tx",
            "GrandpaApi_submit_report_equivocation_unsigned_extrinsic",
            "BabeApi_submit_report_equivocation_unsigned_extrinsic",
        ] {
            execute(
                &executor,
                method,
                &vec![0; 2048],
                CallContext::Offchain,
                false,
            )
            .unwrap();
        }
    }

    #[test]
    fn classical_execution_has_no_new_count_byte_or_rate_limit() {
        let (executor, _) = executor();
        saturate(&executor);
        for proof in [None, Some(169)] {
            execute(
                &executor,
                VALIDATE,
                &validation(proof),
                CallContext::Offchain,
                false,
            )
            .unwrap();
            execute(
                &executor,
                APPLY,
                &extrinsic(proof, 66),
                CallContext::Offchain,
                true,
            )
            .unwrap();
        }
        let items = vec![extrinsic(None, 1); 1025];
        execute(
            &executor,
            PENDING,
            &list(&items),
            CallContext::Offchain,
            false,
        )
        .unwrap();
        execute(
            &executor,
            "Core_execute_block",
            &block(&items),
            CallContext::Offchain,
            false,
        )
        .unwrap();
        execute(
            &executor,
            APPLY,
            &extrinsic(None, MAX_ACTIVE_BYTES + 1),
            CallContext::Offchain,
            false,
        )
        .unwrap();
    }

    #[test]
    fn pq_authentication_rejects_before_entering_runtime_when_saturated() {
        let (executor, _) = executor();
        saturate(&executor);
        for size in [5367, 1669, 3179] {
            let tx = extrinsic(Some(size), 66);
            for (method, data) in [
                (VALIDATE, validation(Some(size))),
                (APPLY, tx.clone()),
                (PENDING, list(std::slice::from_ref(&tx))),
                ("Core_execute_block", block(std::slice::from_ref(&tx))),
                ("TryRuntime_execute_block", {
                    let mut data = block(&[tx]);
                    data.extend_from_slice(&[0, 0, 0]);
                    data
                }),
            ] {
                let err =
                    execute(&executor, method, &data, CallContext::Offchain, false).unwrap_err();
                assert!(
                    err.to_string()
                        .contains(fp_rpc::LOCAL_EXECUTION_CAPACITY_ERROR)
                );
            }
        }
        assert_eq!(executor.inner.calls.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn mixed_requests_charge_only_pq_envelopes() {
        let (executor, _) = executor();
        freeze_rate(&mut executor.raw.lock().unwrap());
        let pq = extrinsic(Some(1669), 66);
        let data = list(&[extrinsic(None, MAX_ACTIVE_BYTES + 1), pq.clone(), pq]);
        execute(&executor, PENDING, &data, CallContext::Offchain, false).unwrap();
        let state = executor.raw.lock().unwrap();
        assert_eq!((state.active, state.bytes, state.tokens), (0, 0, BURST - 2));
    }

    #[test]
    fn large_pq_batches_can_run_with_a_full_bucket() {
        let (executor, _) = executor();
        freeze_rate(&mut executor.raw.lock().unwrap());
        let data = list(&vec![extrinsic(Some(1669), 66); 32]);
        execute(&executor, PENDING, &data, CallContext::Offchain, false).unwrap();
        assert_eq!(executor.raw.lock().unwrap().tokens, 0);
        assert!(execute(&executor, PENDING, &data, CallContext::Offchain, false).is_err());
        assert_eq!(executor.inner.calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn trusted_pool_reservation_is_separate_and_only_for_validation() {
        let (executor, _) = executor();
        {
            let mut raw = executor.raw.lock().unwrap();
            raw.tokens = 0;
            freeze_rate(&mut raw);
        }
        for _ in 0..32 {
            execute(
                &executor,
                VALIDATE,
                &validation(Some(5367)),
                CallContext::Offchain,
                true,
            )
            .unwrap();
        }
        assert!(
            execute(
                &executor,
                VALIDATE,
                &validation(Some(5367)),
                CallContext::Offchain,
                false
            )
            .is_err()
        );
        assert!(
            execute(
                &executor,
                APPLY,
                &extrinsic(Some(5367), 66),
                CallContext::Offchain,
                true
            )
            .is_err()
        );
        assert_eq!(executor.pool.lock().unwrap().tokens, BURST);
    }

    #[test]
    fn onchain_and_local_benchmark_execution_bypass_admission() {
        let (mut executor, _) = executor();
        saturate(&executor);
        execute(
            &executor,
            VALIDATE,
            &validation(Some(5367)),
            CallContext::Onchain,
            false,
        )
        .unwrap();
        executor.mode = ExecutionMode::Benchmark;
        execute(
            &executor,
            VALIDATE,
            &validation(Some(5367)),
            CallContext::Offchain,
            false,
        )
        .unwrap();
    }

    #[test]
    fn wallet_benchmarks_require_explicit_local_mode() {
        let (mut executor, _) = executor();
        let mut config = frame_benchmarking::BenchmarkConfig {
            pallet: b"pallet_hashed_accounts".to_vec(),
            instance: b"HashedAccounts".to_vec(),
            benchmark: b"register_derived".to_vec(),
            ..Default::default()
        };
        let method = "Benchmark_dispatch_benchmark";
        assert!(
            execute(
                &executor,
                method,
                &config.encode(),
                CallContext::Offchain,
                false
            )
            .is_err()
        );
        assert_eq!(executor.inner.calls.load(Ordering::SeqCst), 0);
        executor.mode = ExecutionMode::Benchmark;
        execute(
            &executor,
            method,
            &config.encode(),
            CallContext::Offchain,
            false,
        )
        .unwrap();
        executor.mode = ExecutionMode::Node;
        saturate(&executor);
        config.pallet = b"pallet_balances".to_vec();
        config.instance = b"Balances".to_vec();
        config.benchmark = b"transfer_keep_alive".to_vec();
        execute(
            &executor,
            method,
            &config.encode(),
            CallContext::Offchain,
            false,
        )
        .unwrap();
    }

    #[test]
    fn failed_and_panicking_runtime_calls_release_capacity_but_not_rate() {
        let (executor, _) = executor();
        freeze_rate(&mut executor.raw.lock().unwrap());
        executor.inner.behavior.store(1, Ordering::SeqCst);
        assert!(
            execute(
                &executor,
                VALIDATE,
                &validation(Some(5367)),
                CallContext::Offchain,
                false
            )
            .is_err()
        );
        executor.inner.behavior.store(2, Ordering::SeqCst);
        assert!(
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                execute(
                    &executor,
                    VALIDATE,
                    &validation(Some(5367)),
                    CallContext::Offchain,
                    false,
                )
            }))
            .is_err()
        );
        let state = executor.raw.lock().unwrap();
        assert_eq!((state.active, state.bytes, state.tokens), (0, 0, BURST - 2));
    }

    #[test]
    fn detached_request_retains_permit_until_runtime_finishes() {
        let (executor, entered) = executor();
        executor.inner.behavior.store(3, Ordering::SeqCst);
        let worker = executor.clone();
        let (done, finished) = mpsc::channel();
        let handle = std::thread::spawn(move || {
            execute(
                &worker,
                VALIDATE,
                &validation(Some(5367)),
                CallContext::Offchain,
                false,
            )
            .unwrap();
            done.send(()).unwrap();
        });
        entered.recv_timeout(Duration::from_secs(5)).unwrap();
        drop(handle); // Dropping a request does not stop synchronous execution.
        assert_eq!(executor.raw.lock().unwrap().active, 1);
        let (lock, wake) = &*executor.inner.resume;
        *lock.lock().unwrap() = true;
        wake.notify_all();
        finished.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(executor.raw.lock().unwrap().active, 0);
    }

    #[test]
    fn pool_waits_for_shared_capacity_and_wakes_when_released() {
        let (executor, _) = executor();
        let permit = executor
            .acquire_pool(Work {
                proofs: 1,
                bytes: MAX_ACTIVE_BYTES,
            })
            .unwrap();
        let worker = executor.clone();
        let (ready, started) = mpsc::channel();
        let (done, finished) = mpsc::channel();
        let handle = std::thread::spawn(move || {
            ready.send(()).unwrap();
            let _permit = worker
                .acquire_pool(Work {
                    proofs: 1,
                    bytes: 1,
                })
                .unwrap();
            done.send(()).unwrap();
        });
        started.recv_timeout(Duration::from_secs(5)).unwrap();
        assert!(finished.recv_timeout(Duration::from_millis(20)).is_err());
        drop(permit);
        finished.recv_timeout(Duration::from_secs(5)).unwrap();
        handle.join().unwrap();
        assert_eq!(executor.pool.lock().unwrap().active, 0);
        assert!(
            executor
                .acquire_pool(Work {
                    proofs: 1,
                    bytes: MAX_ACTIVE_BYTES + 1
                })
                .is_err()
        );
    }

    #[test]
    fn state_bounds_bytes_concurrency_and_refill_without_refunding_attempts() {
        let now = Instant::now();
        let work = Work {
            proofs: 1,
            bytes: 1,
        };
        let mut state = State::new(now);
        assert!(
            state
                .acquire(
                    Work {
                        proofs: 1,
                        bytes: MAX_ACTIVE_BYTES + 1
                    },
                    now,
                    true
                )
                .is_err()
        );
        state.acquire(work, now, true).unwrap();
        state.acquire(work, now, true).unwrap();
        assert!(state.acquire(work, now, true).is_err());
        assert_eq!(state.tokens, BURST - 2);
        state.active = 0;
        state.bytes = usize::MAX;
        assert!(state.acquire(work, now, true).is_err());
        state.bytes = 0;
        state.tokens = 0;
        assert!(state.acquire(work, now - REFILL, true).is_err());
        assert_eq!(state.refilled, now);
        assert!(state.acquire(work, now + REFILL / 2, true).is_err());
        state.acquire(work, now + REFILL, true).unwrap();
        assert_eq!(state.tokens, 0);
    }
}
