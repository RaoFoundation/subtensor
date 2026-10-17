// SPDX-License-Identifier: GPL-3.0-or-later WITH Classpath-exception-2.0

use jsonrpsee::core::RpcResult;
use std::{
	sync::{
		atomic::{AtomicUsize, Ordering},
		Arc, Mutex,
	},
	time::{Duration, Instant},
};

/// An entire pending reconstruction, including any final simulation, has one
/// deadline. These host limits are deliberately independent of consensus limits.
pub const PENDING_TIMEOUT: Duration = Duration::from_secs(15);
pub const MAX_PENDING_EXTRINSICS: usize = 256;
pub const MAX_PENDING_SNAPSHOT_BYTES: usize = 8 * 1024 * 1024;
const MAX_PENDING_JOBS: usize = 2;
const MAX_PENDING_ATTEMPTS: usize = 1024;
const MAX_PENDING_ATTEMPT_BYTES: usize = 64 * 1024 * 1024;
pub const PENDING_LIMIT_ERROR: &str =
	"Pending execution deadline or work limit reached; retry later";

static PENDING_JOBS: AtomicUsize = AtomicUsize::new(0);

struct PendingWork {
	deadline: Instant,
	// Counts even rejected admission attempts, never reset by overlay rollback.
	work: Mutex<(usize, usize)>,
}

impl Drop for PendingWork {
	fn drop(&mut self) {
		PENDING_JOBS.fetch_sub(1, Ordering::AcqRel);
	}
}

/// Trusted, process-local capability attached only by the pending RPC builder.
/// It has no wire encoding or runtime host function; raw `state_call` callers
/// cannot create it. Clones retain the job lease until the final API is dropped.
#[derive(Clone)]
pub struct PendingExecution(Arc<PendingWork>);

impl PendingExecution {
	pub fn try_new(deadline: Instant) -> Result<Self, &'static str> {
		if deadline <= Instant::now() {
			return Err(PENDING_LIMIT_ERROR);
		}
		PENDING_JOBS
			.fetch_update(Ordering::AcqRel, Ordering::Acquire, |active| {
				(active < MAX_PENDING_JOBS).then(|| active + 1)
			})
			.map_err(|_| fp_rpc::LOCAL_EXECUTION_CAPACITY_ERROR)?;
		Ok(Self(Arc::new(PendingWork {
			deadline: deadline.min(Instant::now() + PENDING_TIMEOUT),
			work: Mutex::new((0, 0)),
		})))
	}

	pub fn deadline(&self) -> Instant {
		self.0.deadline
	}

	pub fn check(&self) -> Result<(), &'static str> {
		if Instant::now() >= self.0.deadline {
			Err(PENDING_LIMIT_ERROR)
		} else {
			Ok(())
		}
	}

	/// Called at the executor boundary, including on attempts rejected for rate
	/// or concurrency. Neither a clone nor a transaction rollback renews credit.
	pub fn charge(&self, bytes: usize) -> Result<(), &'static str> {
		self.check()?;
		let mut work = self.0.work.lock().map_err(|_| PENDING_LIMIT_ERROR)?;
		work.0 = work.0.saturating_add(1);
		work.1 = work.1.saturating_add(bytes);
		if work.0 > MAX_PENDING_ATTEMPTS || work.1 > MAX_PENDING_ATTEMPT_BYTES {
			Err(PENDING_LIMIT_ERROR)
		} else {
			Ok(())
		}
	}
}

impl sp_externalities::Extension for PendingExecution {
	fn as_mut_any(&mut self) -> &mut dyn std::any::Any {
		self
	}
	fn type_id(&self) -> std::any::TypeId {
		std::any::TypeId::of::<Self>()
	}
}

/// Retry only pre-execution admission failures. The caller rolls back each
/// failed attempt and retains its already committed pending overlay.
pub async fn retry_pending_execution<T, A>(
	state: &mut A,
	mut execute: impl FnMut(&mut A) -> Result<T, sp_api::ApiError>,
	budget: &PendingExecution,
) -> Result<T, sp_api::ApiError> {
	loop {
		budget.check().map_err(pending_limit_error)?;
		let result = execute(state);
		if !result.as_ref().err().is_some_and(|error| {
			error.to_string()
				== format!(
					"Execution failed: Other: {}",
					fp_rpc::LOCAL_EXECUTION_CAPACITY_ERROR
				)
		}) {
			budget.check().map_err(pending_limit_error)?;
			return result;
		}
		tokio::time::sleep_until(tokio::time::Instant::from_std(
			budget
				.deadline()
				.min(Instant::now() + Duration::from_millis(250)),
		))
		.await;
	}
}

pub fn pending_limit_error(message: &'static str) -> sp_api::ApiError {
	sp_api::ApiError::Application(std::io::Error::other(message).into())
}

/// Wait for executor capacity instead of presenting ordinary backpressure as an
/// EVM failure. Gas estimation shares one deadline across all its probes. Every
/// retry still passes executor admission, and cancellation drops the sleep
/// without launching another probe.
pub async fn retry_execution_capacity<T, F: std::future::Future<Output = RpcResult<T>>>(
	mut execute: impl FnMut() -> F,
	deadline: tokio::time::Instant,
) -> RpcResult<T> {
	loop {
		let result = execute().await;
		let retry = result.as_ref().err().is_some_and(|error| {
			error.code() == jsonrpsee::types::error::INTERNAL_ERROR_CODE
				&& error.message()
					== format!(
						"runtime error: Execution failed: Other: {}",
						fp_rpc::LOCAL_EXECUTION_CAPACITY_ERROR
					)
		});
		let now = tokio::time::Instant::now();
		if !retry || now >= deadline {
			return result;
		}
		tokio::time::sleep_until(deadline.min(now + std::time::Duration::from_millis(125))).await;
	}
}
