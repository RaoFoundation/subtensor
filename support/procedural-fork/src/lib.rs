//! Fork of `frame-support-procedural` that re-exports FRAME parse/expand APIs as a normal crate.
//!
//! Upstream `frame-support-procedural` is a `proc-macro` crate, so its parsers (e.g.
//! `construct_runtime::parse::RuntimeDeclaration`) cannot be depended on by Subtensor's
//! workspace lints such as `RequireExplicitPalletIndex`. This fork mirrors the polkadot-sdk
//! version Subtensor pins and exposes those internals under `procedural_fork::exports`.
//!
//! Prefer searching here for `construct_runtime` / `#[pallet]` expansion behavior when the
//! lint or runtime tooling needs to understand FRAME macro input. Do not rename Substrate
//! symbols lightly — they are compared against upstream when rebasing the fork.
//!
//! Tags are created ad-hoc per major `polkadot-sdk` bump Subtensor consumes.
#![recursion_limit = "512"]
#![allow(warnings)]
#![allow(clippy::all)]

extern crate proc_macro;

#[cfg(not(test))]
mod benchmark;
#[cfg(not(test))]
mod construct_runtime;
#[cfg(not(test))]
mod crate_version;
#[cfg(not(test))]
mod derive_impl;
#[cfg(not(test))]
mod dummy_part_checker;
#[cfg(not(test))]
mod dynamic_params;
#[cfg(not(test))]
mod key_prefix;
#[cfg(not(test))]
mod match_and_insert;
#[cfg(not(test))]
mod no_bound;
#[cfg(not(test))]
mod pallet;
#[cfg(not(test))]
mod pallet_error;
#[cfg(not(test))]
mod runtime;
#[cfg(not(test))]
mod storage_alias;
#[cfg(not(test))]
mod transactional;
#[cfg(not(test))]
mod tt_macro;
#[cfg(not(test))]
use std::{cell::RefCell, str::FromStr};

#[cfg(not(test))]
pub(crate) const INHERENT_INSTANCE_NAME: &str = "__InherentHiddenInstance";

/// The number of module instances supported by the runtime, starting at index 1,
/// and up to `NUMBER_OF_INSTANCE`.
#[cfg(not(test))]
pub(crate) const NUMBER_OF_INSTANCE: u8 = 16;

thread_local! {
    /// A global counter, can be used to generate a relatively unique identifier.
    #[cfg(not(test))]
    static COUNTER: RefCell<Counter> = const { RefCell::new(Counter(0)) };
}

/// Counter to generate a relatively unique identifier for macros. This is necessary because
/// declarative macros gets hoisted to the crate root, which shares the namespace with other pallets
/// containing the very same macros.
#[cfg(not(test))]
struct Counter(u64);

#[cfg(not(test))]
impl Counter {
    fn inc(&mut self) -> u64 {
        let ret = self.0;
        self.0 += 1;
        ret
    }
}

/// Get the value from the given environment variable set by cargo.
///
/// The value is parsed into the requested destination type.
#[cfg(not(test))]
fn get_cargo_env_var<T: FromStr>(version_env: &str) -> std::result::Result<T, ()> {
    let version = std::env::var(version_env)
        .unwrap_or_else(|_| panic!("`{}` is always set by cargo; qed", version_env));

    T::from_str(&version).map_err(drop)
}

/// Generate the counter_prefix related to the storage.
/// counter_prefix is used by counted storage map.
#[cfg(not(test))]
fn counter_prefix(prefix: &str) -> String {
    format!("CounterFor{}", prefix)
}

#[cfg(not(test))]
pub mod exports {
    pub mod benchmark {
        pub use crate::benchmark::*;
    }

    pub mod crate_version {
        pub use crate::crate_version::*;
    }

    pub mod derive_impl {
        pub use crate::derive_impl::*;
    }

    pub mod dummy_part_checker {
        pub use crate::dummy_part_checker::*;
    }

    pub mod dynamic_params {
        pub use crate::dynamic_params::*;
    }

    pub mod key_prefix {
        pub use crate::key_prefix::*;
    }

    pub mod match_and_insert {
        pub use crate::match_and_insert::*;
    }

    pub mod pallet_error {
        pub use crate::pallet_error::*;
    }

    pub mod storage_alias {
        pub use crate::storage_alias::*;
    }

    pub mod transactional {
        pub use crate::transactional::*;
    }

    pub mod tt_macro {
        pub use crate::tt_macro::*;
    }

    pub mod construct_runtime {
        pub use crate::construct_runtime::*;

        pub mod parse {
            pub use crate::construct_runtime::parse::*;
        }

        pub mod expand {
            pub use crate::construct_runtime::expand::*;
        }
    }

    pub mod no_bound {
        pub mod clone {
            pub use crate::no_bound::clone::*;
        }

        pub mod debug {
            pub use crate::no_bound::debug::*;
        }

        pub mod default {
            pub use crate::no_bound::default::*;
        }

        pub mod ord {
            pub use crate::no_bound::ord::*;
        }

        pub mod partial_eq {
            pub use crate::no_bound::partial_eq::*;
        }

        pub mod partial_ord {
            pub use crate::no_bound::partial_ord::*;
        }
    }

    pub mod pallet {
        pub use crate::pallet::*;

        pub mod parse {
            pub use crate::pallet::parse::*;
        }
    }
}
