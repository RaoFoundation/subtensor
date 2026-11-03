// SPDX-License-Identifier: GPL-3.0-or-later WITH Classpath-exception-2.0

use std::time::Duration;

/// Retry only PQ capacity errors on the same retained pending overlay. Successful
/// classical work has no deadline. A deadline starts only after contention and
/// bounds this individual retry, allowing a large mixed pool to make progress.
pub async fn retry_pending_execution<T, A>(
	state: &mut A,
	mut execute: impl FnMut(&mut A) -> Result<T, sp_api::ApiError>,
) -> Result<T, sp_api::ApiError> {
	let mut deadline = None;
	loop {
		let result = execute(state);
		if !result.as_ref().err().is_some_and(|error| {
			error.to_string()
				== format!(
					"Execution failed: Other: {}",
					fp_rpc::LOCAL_EXECUTION_CAPACITY_ERROR
				)
		}) {
			return result;
		}
		let now = tokio::time::Instant::now();
		let deadline = *deadline.get_or_insert(now + Duration::from_secs(15));
		if now >= deadline {
			return result;
		}
		tokio::time::sleep_until(deadline.min(now + Duration::from_millis(125))).await;
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	fn error(message: &str) -> sp_api::ApiError {
		sp_api::ApiError::Application(std::io::Error::other(message.to_owned()).into())
	}

	#[test]
	fn successful_legacy_work_needs_no_timer_or_job_permit() {
		let mut applied = 0;
		for _ in 0..300 {
			futures::executor::block_on(retry_pending_execution(&mut applied, |applied| {
				*applied += 1;
				Ok(())
			}))
			.unwrap();
		}
		assert_eq!(applied, 300);
	}

	#[tokio::test]
	async fn pending_replay_progresses_beyond_a_pq_burst_without_restarting_state() {
		let mut applied = 0;
		for index in 0..40 {
			let mut attempts = 0;
			retry_pending_execution(&mut applied, |applied| {
				assert_eq!(*applied, index);
				attempts += 1;
				if index >= 16 && attempts == 1 {
					return Err(error(&format!(
						"Execution failed: Other: {}",
						fp_rpc::LOCAL_EXECUTION_CAPACITY_ERROR
					)));
				}
				*applied += 1;
				Ok(())
			})
			.await
			.unwrap();
		}
		assert_eq!(applied, 40);
	}

	#[tokio::test]
	async fn genuine_runtime_errors_and_similar_messages_are_not_retried() {
		for message in [
			"runtime execution failed",
			fp_rpc::LOCAL_EXECUTION_CAPACITY_ERROR,
			"Execution failed: Other: Local runtime execution capacity reached; retry later suffix",
		] {
			let mut attempts = 0;
			let result = retry_pending_execution(&mut attempts, |attempts| {
				*attempts += 1;
				Err::<(), _>(error(message))
			})
			.await;
			assert_eq!(result.unwrap_err().to_string(), message);
			assert_eq!(attempts, 1);
		}
	}

	#[tokio::test]
	async fn cancellation_stops_pq_retry_without_replaying_state() {
		let mut attempts = 0;
		let mut pending = Box::pin(retry_pending_execution(&mut attempts, |attempts| {
			*attempts += 1;
			Err::<(), _>(error(&format!(
				"Execution failed: Other: {}",
				fp_rpc::LOCAL_EXECUTION_CAPACITY_ERROR
			)))
		}));
		assert!(futures::poll!(&mut pending).is_pending());
		drop(pending);
		assert_eq!(attempts, 1);
	}
}
