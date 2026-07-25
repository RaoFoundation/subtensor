//! Thread-local flag marking dispatch that originated from an EVM precompile.
//!
//! Precompiles wrap `dispatch` in [`with_evm_context`] so fee / origin logic can distinguish
//! EVM-driven calls from native extrinsics via [`is_in_evm`].

environmental::environmental!(IN_EVM: bool);

/// Returns `true` if the current dispatch originated from an EVM precompile.
pub fn is_in_evm() -> bool {
    IN_EVM::with(|v| *v).unwrap_or(false)
}

/// Executes `f` within an EVM context, making `is_in_evm()` return `true`
/// for the duration of the closure. Uses `using_once` so nested calls
/// reuse the already-set value instead of building a stack.
pub fn with_evm_context<R>(f: impl FnOnce() -> R) -> R {
    IN_EVM::using_once(&mut true, f)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_in_evm_returns_false_by_default() {
        assert!(!is_in_evm());
    }

    #[test]
    fn with_evm_context_sets_flag() {
        with_evm_context(|| {
            assert!(is_in_evm());
        });
    }

    #[test]
    fn flag_clears_after_evm_context() {
        with_evm_context(|| {});
        assert!(!is_in_evm());
    }

    #[test]
    fn nested_evm_context_stays_true() {
        with_evm_context(|| {
            with_evm_context(|| {
                assert!(is_in_evm());
            });
            // Still true after inner context exits thanks to using_once.
            assert!(is_in_evm());
        });
    }
}
