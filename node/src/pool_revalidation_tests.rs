//! Exercise the vendored pool's maintenance policy through its public node-facing APIs.

#![allow(
    clippy::arithmetic_side_effects,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::unwrap_used
)]

use futures::{
    StreamExt,
    future::{Ready, ready},
};
use sc_transaction_pool::{BasicPool, ChainApi, ForkAwareTxPool, ValidateTransactionPriority};
use sc_transaction_pool_api::{
    ChainEvent, MaintainedTransactionPool, TransactionPool, TransactionStatus,
    error::{Error as PoolError, IntoPoolError},
};
use sp_blockchain::{HashAndNumber, TreeRoute};
use sp_core::{Encode, H256};
use sp_runtime::{
    OpaqueExtrinsic,
    generic::{Block as GenericBlock, BlockId, Header as GenericHeader},
    traits::{BlakeTwo256, Hash as HashT, Header as HeaderT},
    transaction_validity::{
        InvalidTransaction, TransactionSource, TransactionValidity, ValidTransaction,
    },
};
use std::{
    collections::HashMap,
    fmt,
    future::Future,
    sync::{Arc, Mutex},
    time::Duration,
};

type Header = GenericHeader<u64, BlakeTwo256>;
type Block = GenericBlock<Header, OpaqueExtrinsic>;
type Basic = BasicPool<TestApi, Block>;
type ForkAware = ForkAwareTxPool<TestApi, Block>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Validation {
    Valid,
    Overloaded,
    Invalid,
}

#[derive(Debug)]
enum TestError {
    Overloaded,
    Pool(PoolError),
}

impl fmt::Display for TestError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Overloaded => formatter.write_str("local validation capacity exhausted"),
            Self::Pool(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for TestError {}

impl From<PoolError> for TestError {
    fn from(error: PoolError) -> Self {
        Self::Pool(error)
    }
}

impl IntoPoolError for TestError {
    fn into_pool_error(self) -> Result<PoolError, Self> {
        match self {
            Self::Pool(error) => Ok(error),
            error => Err(error),
        }
    }
}

/// A chain with optional forks/block bodies and independently controllable runtime validation.
struct TestApi {
    headers: Vec<Header>,
    hashes: Vec<H256>,
    fork_headers: Mutex<HashMap<H256, Header>>,
    bodies: Mutex<HashMap<H256, Vec<OpaqueExtrinsic>>>,
    validation: Mutex<Validation>,
    transaction_validation: Mutex<HashMap<H256, Validation>>,
    calls: Mutex<Vec<(H256, H256, Validation)>>,
    longevity: u64,
}

impl TestApi {
    fn new(longevity: u64) -> Arc<Self> {
        let mut headers = Vec::new();
        let mut hashes = Vec::new();
        let mut parent_hash = H256::zero();
        for number in 0..=32 {
            let header = Header::new(
                number,
                H256::zero(),
                H256::zero(),
                parent_hash,
                Default::default(),
            );
            parent_hash = header.hash();
            hashes.push(parent_hash);
            headers.push(header);
        }
        Arc::new(Self {
            headers,
            hashes,
            fork_headers: Mutex::new(HashMap::new()),
            bodies: Mutex::new(HashMap::new()),
            validation: Mutex::new(Validation::Valid),
            transaction_validation: Mutex::new(HashMap::new()),
            calls: Mutex::new(Vec::new()),
            longevity,
        })
    }

    fn hash(&self, number: u64) -> H256 {
        self.hashes[number as usize]
    }

    fn number(&self, hash: H256) -> Option<u64> {
        self.header(hash).map(|header| *header.number())
    }

    fn header(&self, hash: H256) -> Option<Header> {
        self.hashes
            .iter()
            .position(|candidate| *candidate == hash)
            .map(|index| self.headers[index].clone())
            .or_else(|| self.fork_headers.lock().unwrap().get(&hash).cloned())
    }

    fn fork_block(&self, parent: H256, discriminator: u8) -> H256 {
        let header = Header::new(
            self.number(parent).expect("known fork parent") + 1,
            H256::zero(),
            H256::repeat_byte(discriminator),
            parent,
            Default::default(),
        );
        let hash = header.hash();
        self.fork_headers.lock().unwrap().insert(hash, header);
        hash
    }

    fn set_body(&self, hash: H256, body: Vec<OpaqueExtrinsic>) {
        self.bodies.lock().unwrap().insert(hash, body);
    }

    fn set_validation(&self, validation: Validation) {
        *self.validation.lock().unwrap() = validation;
    }

    fn was_validated(&self, number: u64, validation: Validation) -> bool {
        self.calls
            .lock()
            .unwrap()
            .iter()
            .any(|(at, _, result)| *at == self.hash(number) && *result == validation)
    }

    fn set_transaction_validation(&self, hash: H256, validation: Validation) {
        self.transaction_validation
            .lock()
            .unwrap()
            .insert(hash, validation);
    }

    fn was_transaction_validated(&self, number: u64, hash: H256, validation: Validation) -> bool {
        self.was_transaction_validated_at(self.hash(number), hash, validation)
    }

    fn was_transaction_validated_at(&self, at: H256, hash: H256, validation: Validation) -> bool {
        self.calls.lock().unwrap().contains(&(at, hash, validation))
    }

    fn validation_count(&self, number: u64, validation: Validation) -> usize {
        self.calls
            .lock()
            .unwrap()
            .iter()
            .filter(|(at, _, result)| *at == self.hash(number) && *result == validation)
            .count()
    }

    fn validate(
        &self,
        at: H256,
        extrinsic: &OpaqueExtrinsic,
    ) -> Result<TransactionValidity, TestError> {
        let hash = BlakeTwo256::hash_of(extrinsic);
        let validation = self
            .transaction_validation
            .lock()
            .unwrap()
            .get(&hash)
            .copied()
            .unwrap_or_else(|| *self.validation.lock().unwrap());
        self.calls.lock().unwrap().push((at, hash, validation));
        match validation {
            Validation::Valid => Ok(Ok(ValidTransaction {
                priority: 7,
                provides: vec![hash.as_bytes().to_vec()],
                longevity: self.longevity,
                ..Default::default()
            })),
            Validation::Overloaded => Err(TestError::Overloaded),
            Validation::Invalid => Ok(Err(InvalidTransaction::BadProof.into())),
        }
    }

    fn new_best(&self, number: u64) -> ChainEvent<Block> {
        ChainEvent::NewBestBlock {
            hash: self.hash(number),
            tree_route: None,
        }
    }

    fn finalized(&self, previous: u64, number: u64) -> ChainEvent<Block> {
        ChainEvent::Finalized {
            hash: self.hash(number),
            tree_route: ((previous + 1)..number)
                .map(|n| self.hash(n))
                .collect::<Vec<_>>()
                .into(),
        }
    }
}

impl ChainApi for TestApi {
    type Block = Block;
    type Error = TestError;
    type ValidationFuture = Ready<Result<TransactionValidity, TestError>>;
    type BodyFuture = Ready<Result<Option<Vec<OpaqueExtrinsic>>, TestError>>;

    fn is_retriable_validation_error(&self, error: &Self::Error) -> bool {
        matches!(error, TestError::Overloaded)
    }

    fn validate_transaction(
        &self,
        at: H256,
        _source: TransactionSource,
        extrinsic: Arc<OpaqueExtrinsic>,
        _priority: ValidateTransactionPriority,
    ) -> Self::ValidationFuture {
        ready(self.validate(at, &extrinsic))
    }

    fn validate_transaction_blocking(
        &self,
        at: H256,
        _source: TransactionSource,
        extrinsic: Arc<OpaqueExtrinsic>,
    ) -> Result<TransactionValidity, TestError> {
        self.validate(at, &extrinsic)
    }

    fn block_id_to_number(&self, at: &BlockId<Block>) -> Result<Option<u64>, TestError> {
        Ok(match at {
            BlockId::Hash(hash) => self.number(*hash),
            BlockId::Number(number) => self.hashes.get(*number as usize).map(|_| *number),
        })
    }

    fn block_id_to_hash(&self, at: &BlockId<Block>) -> Result<Option<H256>, TestError> {
        Ok(match at {
            BlockId::Hash(hash) => self.number(*hash).map(|_| *hash),
            BlockId::Number(number) => self.hashes.get(*number as usize).copied(),
        })
    }

    fn hash_and_length(&self, extrinsic: &OpaqueExtrinsic) -> (H256, usize) {
        (BlakeTwo256::hash_of(extrinsic), extrinsic.encoded_size())
    }

    fn block_body(&self, at: H256) -> Self::BodyFuture {
        ready(Ok(self.number(at).map(|_| {
            self.bodies
                .lock()
                .unwrap()
                .get(&at)
                .cloned()
                .unwrap_or_default()
        })))
    }

    fn block_header(&self, at: H256) -> Result<Option<Header>, TestError> {
        Ok(self.header(at))
    }

    fn tree_route(&self, from: H256, to: H256) -> Result<TreeRoute<Block>, TestError> {
        let block = |hash| HashAndNumber {
            hash,
            number: self.number(hash).expect("known tree-route block"),
        };
        let parent = |hash| {
            block(
                *self
                    .header(hash)
                    .expect("known tree-route header")
                    .parent_hash(),
            )
        };
        let mut left = block(from);
        let mut right = block(to);
        let mut retracted = Vec::new();
        let mut enacted = Vec::new();
        while left.number > right.number {
            let previous = parent(left.hash);
            retracted.push(left);
            left = previous;
        }
        while right.number > left.number {
            let previous = parent(right.hash);
            enacted.push(right);
            right = previous;
        }
        while left.hash != right.hash {
            let previous_left = parent(left.hash);
            let previous_right = parent(right.hash);
            retracted.push(left);
            enacted.push(right);
            left = previous_left;
            right = previous_right;
        }
        let common_index = retracted.len();
        retracted.push(left);
        retracted.extend(enacted.into_iter().rev());
        Ok(TreeRoute::new(retracted, common_index).expect("connected route is valid"))
    }
}

fn extrinsic() -> OpaqueExtrinsic {
    OpaqueExtrinsic::from_bytes(&[4, 42]).expect("one-byte SCALE opaque extrinsic")
}

fn numbered_extrinsic(number: u32) -> OpaqueExtrinsic {
    let mut encoded = vec![16]; // Compact length of the four-byte payload.
    encoded.extend_from_slice(&number.to_le_bytes());
    OpaqueExtrinsic::from_bytes(&encoded).expect("four-byte SCALE opaque extrinsic")
}

fn run_test(test: impl Future<Output = ()>) {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(async {
            tokio::time::timeout(Duration::from_secs(10), test)
                .await
                .expect("transaction pool regression timed out");
        });
}

fn basic_pool(api: Arc<TestApi>) -> Basic {
    let (pool, worker) = Basic::new_test(api.clone(), api.hash(0), api.hash(0), Default::default());
    tokio::spawn(worker);
    pool
}

fn fork_aware_pool(api: Arc<TestApi>) -> ForkAware {
    fork_aware_pool_at(api, 0)
}

fn fork_aware_pool_at(api: Arc<TestApi>, best_number: u64) -> ForkAware {
    let (pool, [notifications, sync_bridge]) =
        ForkAware::new_test(api.clone(), api.hash(best_number), api.hash(0), None);
    tokio::spawn(notifications);
    // The sync bridge blocks on std::sync::mpsc::Receiver even though its API returns
    // a Future. Poll it on a dedicated thread; dropping the pool closes its channel.
    std::thread::spawn(move || futures::executor::block_on(sync_bridge));
    pool
}

async fn wait_for_validation(api: &TestApi, number: u64, validation: Validation) {
    while !api.was_validated(number, validation) {
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    // The ready validation future and its batch results complete in the same worker poll.
    tokio::task::yield_now().await;
}

async fn maintain_view(pool: &ForkAware, api: &TestApi, number: u64) {
    pool.maintain(api.new_best(number)).await;
    // new_test revalidates synchronously; the next maintain applies those results before
    // deciding that this duplicate best-block notification needs no further processing.
    pool.maintain(api.new_best(number)).await;
}

#[test]
fn basic_pool_retries_local_overload_but_removes_explicitly_invalid_transactions() {
    run_test(async {
        let api = TestApi::new(64);
        let pool = basic_pool(api.clone());
        let hash = pool
            .submit_one(api.hash(0), TransactionSource::External, extrinsic())
            .await
            .unwrap();

        api.set_validation(Validation::Overloaded);
        pool.maintain(api.new_best(1)).await;
        wait_for_validation(&api, 1, Validation::Overloaded).await;
        assert_eq!(
            pool.ready().map(|tx| tx.hash).collect::<Vec<_>>(),
            vec![hash]
        );

        api.set_validation(Validation::Invalid);
        pool.maintain(api.new_best(2)).await;
        wait_for_validation(&api, 2, Validation::Invalid).await;
        assert_eq!(pool.status().ready, 0);
    });
}

#[test]
fn basic_pool_overload_does_not_extend_cached_longevity() {
    run_test(async {
        let api = TestApi::new(2);
        let pool = basic_pool(api.clone());
        pool.submit_one(api.hash(0), TransactionSource::External, extrinsic())
            .await
            .unwrap();

        api.set_validation(Validation::Overloaded);
        pool.maintain(api.new_best(1)).await;
        wait_for_validation(&api, 1, Validation::Overloaded).await;
        assert_eq!(pool.ready().next().unwrap().valid_till, 2);

        pool.maintain(api.new_best(2)).await;
        assert_eq!(
            pool.status().ready,
            0,
            "the original validity expires at block two"
        );
    });
}

#[test]
fn basic_pool_rejects_initial_submission_on_local_overload() {
    run_test(async {
        let api = TestApi::new(64);
        let pool = basic_pool(api.clone());
        api.set_validation(Validation::Overloaded);

        assert!(
            pool.submit_one(api.hash(0), TransactionSource::External, extrinsic())
                .await
                .is_err()
        );
        assert!(api.was_validated(0, Validation::Overloaded));
        assert_eq!(pool.status().ready, 0);
        assert_eq!(pool.status().future, 0);

        api.set_validation(Validation::Valid);
        pool.submit_one(api.hash(0), TransactionSource::External, extrinsic())
            .await
            .expect("an overloaded submission must not ban the transaction hash");
        assert_eq!(pool.status().ready, 1);
    });
}

#[test]
fn fork_aware_view_retries_local_overload_but_removes_explicitly_invalid_transactions() {
    run_test(async {
        let api = TestApi::new(64);
        let pool = fork_aware_pool(api.clone());
        let hash = pool
            .submit_one(api.hash(0), TransactionSource::External, extrinsic())
            .await
            .unwrap();

        api.set_validation(Validation::Overloaded);
        maintain_view(&pool, &api, 1).await;
        assert!(api.was_validated(1, Validation::Overloaded));
        assert_eq!(
            pool.ready_at(api.hash(1))
                .await
                .map(|tx| tx.hash)
                .collect::<Vec<_>>(),
            vec![hash]
        );

        api.set_validation(Validation::Invalid);
        maintain_view(&pool, &api, 2).await;
        assert!(api.was_validated(2, Validation::Invalid));
        assert_eq!(pool.ready_at(api.hash(2)).await.count(), 0);
    });
}

#[test]
fn fork_aware_view_overload_does_not_extend_cached_longevity() {
    run_test(async {
        let api = TestApi::new(2);
        let pool = fork_aware_pool(api.clone());
        pool.submit_one(api.hash(0), TransactionSource::External, extrinsic())
            .await
            .unwrap();

        api.set_validation(Validation::Overloaded);
        maintain_view(&pool, &api, 1).await;
        assert!(api.was_validated(1, Validation::Overloaded));
        assert_eq!(
            pool.ready_at(api.hash(1)).await.next().unwrap().valid_till,
            2
        );

        maintain_view(&pool, &api, 2).await;
        assert_eq!(pool.ready_at(api.hash(2)).await.count(), 0);
    });
}

#[test]
fn fork_aware_mempool_overload_retains_transaction_without_delaying_next_retry() {
    run_test(async {
        let api = TestApi::new(64);
        let pool = fork_aware_pool(api.clone());
        pool.submit_one(api.hash(0), TransactionSource::External, extrinsic())
            .await
            .unwrap();
        maintain_view(&pool, &api, 20).await;

        // Keeping the best view ahead of finality isolates TxMemPool revalidation from
        // view creation. Its initial validated_at=0 becomes eligible at finalized block 11.
        api.set_validation(Validation::Overloaded);
        pool.maintain(api.finalized(0, 11)).await;
        assert!(api.was_validated(11, Validation::Overloaded));
        assert_eq!(pool.mempool_len().await, (1, 0));
        assert_eq!(pool.ready_at(api.hash(20)).await.count(), 1);

        // A transient failure must not reset validated_at to 11 and postpone this retry
        // until block 22. The immediately following finalized block discovers BadProof.
        api.set_validation(Validation::Invalid);
        pool.maintain(api.finalized(11, 12)).await;
        assert!(api.was_validated(12, Validation::Invalid));
        assert_eq!(pool.mempool_len().await, (0, 0));
        assert_eq!(pool.ready_at(api.hash(20)).await.count(), 0);
    });
}

#[test]
fn fork_aware_mempool_overload_does_not_starve_later_eligible_transactions() {
    run_test(async {
        let api = TestApi::new(64);
        // One already-built best view keeps finalization focused on the memory pool.
        let pool = fork_aware_pool_at(api.clone(), 20);
        for number in 0..1000 {
            pool.submit_one(
                api.hash(0),
                TransactionSource::External,
                numbered_extrinsic(number),
            )
            .await
            .unwrap();
            tokio::task::yield_now().await;
        }
        let later_hash = pool
            .submit_one(
                api.hash(1),
                TransactionSource::External,
                numbered_extrinsic(1000),
            )
            .await
            .unwrap();
        assert_eq!(pool.mempool_len().await, (1001, 0));
        assert_eq!(pool.ready_at(api.hash(20)).await.count(), 1001);

        api.set_validation(Validation::Overloaded);
        api.set_transaction_validation(later_hash, Validation::Invalid);
        pool.maintain(api.finalized(0, 11)).await;
        // Only the 1000 older entries are eligible for this first full batch.
        assert_eq!(api.validation_count(11, Validation::Overloaded), 1000);
        assert!(!api.was_transaction_validated(11, later_hash, Validation::Invalid));
        assert_eq!(pool.mempool_len().await, (1001, 0));

        pool.maintain(api.finalized(11, 12)).await;
        // Sorting by last successful validation would select the same 1000 entries
        // forever. Sorting by last attempt gives the now-eligible later entry a turn.
        assert!(api.was_transaction_validated(12, later_hash, Validation::Invalid));
        assert_eq!(api.validation_count(12, Validation::Overloaded), 999);
        assert_eq!(pool.mempool_len().await, (1000, 0));
        let retained = pool.ready_at(api.hash(20)).await.collect::<Vec<_>>();
        assert_eq!(retained.len(), 1000);
        assert!(
            retained
                .iter()
                .all(|tx| tx.hash != later_hash && tx.valid_till == 84)
        );

        pool.maintain(api.finalized(12, 13)).await;
        // Attempts rotate ordering without renewing eligibility or cached validity:
        // every remaining entry is still due immediately, not ten blocks later.
        assert_eq!(api.validation_count(13, Validation::Overloaded), 1000);
        assert_eq!(pool.mempool_len().await, (1000, 0));
        assert!(
            pool.ready_at(api.hash(20))
                .await
                .all(|tx| tx.valid_till == 84)
        );
    });
}

#[test]
fn fork_aware_pool_rejects_initial_submission_on_local_overload() {
    run_test(async {
        let api = TestApi::new(64);
        let pool = fork_aware_pool(api.clone());
        api.set_validation(Validation::Overloaded);

        assert!(
            pool.submit_one(api.hash(0), TransactionSource::External, extrinsic())
                .await
                .is_err()
        );
        assert!(api.was_validated(0, Validation::Overloaded));
        assert_eq!(pool.mempool_len().await, (0, 0));
        assert_eq!(pool.status().ready, 0);
        assert_eq!(pool.status().future, 0);

        api.set_validation(Validation::Valid);
        pool.submit_one(api.hash(0), TransactionSource::External, extrinsic())
            .await
            .expect("an overloaded submission must not ban the transaction hash");
        assert_eq!(pool.mempool_len().await, (1, 0));
        assert_eq!(pool.status().ready, 1);
    });
}

#[test]
fn basic_pool_rejects_overloaded_watched_submission_without_banning_retry() {
    run_test(async {
        let api = TestApi::new(64);
        let pool = basic_pool(api.clone());
        api.set_validation(Validation::Overloaded);

        assert!(
            pool.submit_and_watch(api.hash(0), TransactionSource::External, extrinsic())
                .await
                .is_err()
        );
        assert!(api.was_validated(0, Validation::Overloaded));
        assert_eq!(pool.status().ready, 0);
        assert_eq!(pool.status().future, 0);

        api.set_validation(Validation::Valid);
        let _watcher = pool
            .submit_and_watch(api.hash(0), TransactionSource::External, extrinsic())
            .await
            .expect("an overloaded watched submission must allow immediate retry");
        assert_eq!(pool.status().ready, 1);
    });
}

#[test]
fn fork_aware_pool_rejects_overloaded_watched_submission_without_banning_retry() {
    run_test(async {
        let api = TestApi::new(64);
        let pool = fork_aware_pool(api.clone());
        api.set_validation(Validation::Overloaded);

        assert!(
            pool.submit_and_watch(api.hash(0), TransactionSource::External, extrinsic())
                .await
                .is_err()
        );
        assert!(api.was_validated(0, Validation::Overloaded));
        assert_eq!(pool.mempool_len().await, (0, 0));
        assert_eq!(pool.status().ready, 0);
        assert_eq!(pool.status().future, 0);

        api.set_validation(Validation::Valid);
        let _watcher = pool
            .submit_and_watch(api.hash(0), TransactionSource::External, extrinsic())
            .await
            .expect("an overloaded watched submission must allow immediate retry");
        assert_eq!(pool.mempool_len().await, (0, 1));
        assert_eq!(pool.status().ready, 1);
    });
}

#[test]
fn fork_aware_pool_duplicate_watched_submission_preserves_existing_watcher() {
    run_test(async {
        let api = TestApi::new(64);
        let pool = fork_aware_pool(api.clone());
        let mut watcher = pool
            .submit_and_watch(api.hash(0), TransactionSource::External, extrinsic())
            .await
            .unwrap();
        assert_eq!(watcher.next().await, Some(TransactionStatus::Ready));

        assert!(
            pool.submit_and_watch(api.hash(0), TransactionSource::External, extrinsic())
                .await
                .is_err()
        );
        assert_eq!(pool.mempool_len().await, (0, 1));

        api.set_body(api.hash(1), vec![extrinsic()]);
        maintain_view(&pool, &api, 1).await;
        loop {
            match watcher.next().await {
                Some(TransactionStatus::Ready) => continue,
                Some(TransactionStatus::InBlock((hash, index))) => {
                    assert_eq!((hash, index), (api.hash(1), 0));
                    break;
                }
                status => panic!("original watcher lost its inclusion notification: {status:?}"),
            }
        }
    });
}

#[test]
fn basic_pool_still_bans_initial_explicitly_invalid_submission() {
    run_test(async {
        let api = TestApi::new(64);
        let pool = basic_pool(api.clone());
        api.set_validation(Validation::Invalid);

        assert!(matches!(
            pool.submit_one(api.hash(0), TransactionSource::External, extrinsic())
                .await,
            Err(TestError::Pool(PoolError::InvalidTransaction(
                InvalidTransaction::BadProof
            )))
        ));
        api.set_validation(Validation::Valid);
        assert!(matches!(
            pool.submit_one(api.hash(0), TransactionSource::External, extrinsic())
                .await,
            Err(TestError::Pool(PoolError::TemporarilyBanned))
        ));
        assert!(!api.was_validated(0, Validation::Valid));
        assert_eq!(pool.status().ready, 0);
    });
}

#[test]
fn basic_pool_retries_overloaded_reorg_reinsertion_without_early_admission() {
    run_test(async {
        let api = TestApi::new(64);
        let pool = basic_pool(api.clone());
        let hash = pool
            .submit_one(api.hash(0), TransactionSource::External, extrinsic())
            .await
            .unwrap();
        api.set_body(api.hash(1), vec![extrinsic()]);
        pool.maintain(api.new_best(1)).await;
        assert_eq!(pool.status().ready, 0, "branch A included the transaction");

        let branch_b = api.fork_block(api.hash(0), 1);
        api.set_validation(Validation::Overloaded);
        pool.maintain(ChainEvent::NewBestBlock {
            hash: branch_b,
            tree_route: None,
        })
        .await;
        assert!(api.was_transaction_validated_at(branch_b, hash, Validation::Overloaded));
        assert_eq!(
            pool.status().ready,
            0,
            "retracted transactions require fresh validation"
        );
        assert_eq!(pool.status().future, 0);

        let successor = api.fork_block(branch_b, 1);
        api.set_validation(Validation::Valid);
        pool.maintain(ChainEvent::NewBestBlock {
            hash: successor,
            tree_route: None,
        })
        .await;
        assert!(api.was_transaction_validated_at(successor, hash, Validation::Valid));
        assert_eq!(
            pool.ready().map(|tx| tx.hash).collect::<Vec<_>>(),
            vec![hash]
        );
    });
}

#[test]
fn basic_pool_drops_explicitly_invalid_reorg_reinsertion_without_retry() {
    run_test(async {
        let api = TestApi::new(64);
        let pool = basic_pool(api.clone());
        let hash = pool
            .submit_one(api.hash(0), TransactionSource::External, extrinsic())
            .await
            .unwrap();
        api.set_body(api.hash(1), vec![extrinsic()]);
        pool.maintain(api.new_best(1)).await;
        assert_eq!(pool.status().ready, 0);

        let branch_b = api.fork_block(api.hash(0), 1);
        api.set_validation(Validation::Invalid);
        pool.maintain(ChainEvent::NewBestBlock {
            hash: branch_b,
            tree_route: None,
        })
        .await;
        assert!(api.was_transaction_validated_at(branch_b, hash, Validation::Invalid));
        assert_eq!(pool.status().ready, 0);
        assert_eq!(pool.status().future, 0);

        let successor = api.fork_block(branch_b, 1);
        api.set_validation(Validation::Valid);
        pool.maintain(ChainEvent::NewBestBlock {
            hash: successor,
            tree_route: None,
        })
        .await;
        assert!(!api.was_transaction_validated_at(successor, hash, Validation::Valid));
        assert_eq!(
            pool.status().ready,
            0,
            "explicitly invalid transactions are not deferred"
        );
        assert_eq!(pool.status().future, 0);
    });
}

#[test]
fn basic_pool_bounds_deferred_reorg_transactions_by_count_and_bytes() {
    // Each fixture extrinsic occupies five bytes. Test each independent limit while
    // giving the other limit enough room for all three retracted transactions.
    for (count, total_bytes) in [(1, 100), (10, 5)] {
        run_test(async {
            let api = TestApi::new(64);
            let options = sc_transaction_pool::Options {
                ready: sc_transaction_pool::PoolLimit { count, total_bytes },
                future: sc_transaction_pool::PoolLimit {
                    count: 0,
                    total_bytes: 0,
                },
                ..Default::default()
            };
            let (pool, worker) = Basic::new_test(api.clone(), api.hash(0), api.hash(0), options);
            tokio::spawn(worker);
            api.set_body(api.hash(1), (0..3).map(numbered_extrinsic).collect());
            pool.maintain(api.new_best(1)).await;

            let branch_b = api.fork_block(api.hash(0), 1);
            api.set_validation(Validation::Overloaded);
            pool.maintain(ChainEvent::NewBestBlock {
                hash: branch_b,
                tree_route: None,
            })
            .await;
            assert_eq!(pool.status().ready, 0);
            assert_eq!(pool.status().future, 0);

            let successor = api.fork_block(branch_b, 1);
            api.set_validation(Validation::Valid);
            pool.maintain(ChainEvent::NewBestBlock {
                hash: successor,
                tree_route: None,
            })
            .await;
            let validated = api
                .calls
                .lock()
                .unwrap()
                .iter()
                .filter(|(at, _, result)| *at == successor && *result == Validation::Valid)
                .count();
            assert_eq!(
                validated, 1,
                "the overflow must not survive in deferred storage"
            );
            assert_eq!(pool.status().ready, 1);
        });
    }
}

#[test]
fn basic_pool_reorg_retry_failures_do_not_extend_original_block_expiry() {
    run_test(async {
        let api = TestApi::new(128);
        let pool = basic_pool(api.clone());
        let hash = pool
            .submit_one(api.hash(0), TransactionSource::External, extrinsic())
            .await
            .unwrap();
        api.set_body(api.hash(1), vec![extrinsic()]);
        pool.maintain(api.new_best(1)).await;

        let mut branch_b = api.fork_block(api.hash(0), 1);
        api.set_validation(Validation::Overloaded);
        pool.maintain(ChainEvent::NewBestBlock {
            hash: branch_b,
            tree_route: None,
        })
        .await;
        assert!(api.was_transaction_validated_at(branch_b, hash, Validation::Overloaded));
        // Pass the fixed 64-block retry horizon while every actual attempt overloads.
        // Resetting the birth block on any failed retry would incorrectly keep it alive.
        for _ in 0..65 {
            branch_b = api.fork_block(branch_b, 1);
            pool.maintain(ChainEvent::NewBestBlock {
                hash: branch_b,
                tree_route: None,
            })
            .await;
        }
        assert_eq!(pool.status().ready, 0);
        assert_eq!(pool.status().future, 0);

        let successor = api.fork_block(branch_b, 1);
        api.set_validation(Validation::Valid);
        pool.maintain(ChainEvent::NewBestBlock {
            hash: successor,
            tree_route: None,
        })
        .await;
        assert!(!api.was_transaction_validated_at(successor, hash, Validation::Valid));
        assert_eq!(
            pool.status().ready,
            0,
            "expired deferred candidates cannot be resurrected"
        );
        assert_eq!(pool.status().future, 0);
    });
}
