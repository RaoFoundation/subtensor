//! Guards the vendored Solidity artifacts against the Rust source of truth.
//!
//! `precompiles/src/solidity/*.abi` is what EVM integrators consume as the interface of record. It is
//! maintained alongside `precompiles/src/*.rs` rather than generated from it, so a face can be added
//! to the Rust and missed in the ABI. When that happens the omission is silent: the precompile
//! dispatches correctly on chain, but tooling that enumerates from the vendored ABI cannot see the
//! face at all.
//!
//! This test diffs the two sets in BOTH directions and fails naming the offending signature.
//!
//! Run: `cargo test -p subtensor-precompiles --test abi_drift`

#![allow(clippy::expect_used, clippy::arithmetic_side_effects, clippy::indexing_slicing)]

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

/// Functions declared in `.abi` artifacts that are deliberately NOT `#[precompile::public]`.
///
/// `0x402`/`0x403` (ed25519 / sr25519) consume raw calldata and dispatch no selector — the `.sol`
/// and `.abi` artifacts describe the calling convention for integrators, but there is no annotated
/// face behind them. Anything added here needs the same kind of justification.
const ABI_ONLY_ALLOWLIST: &[&str] = &["verify(bytes32,bytes32,bytes32,bytes32)"];

fn crate_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// Every signature inside a `#[precompile::public("...")]` attribute across `precompiles/src/*.rs`.
///
/// The attribute is matched across newlines on purpose. `rustfmt` wraps long attributes, so a
/// line-oriented scan silently misses exactly the faces with the most parameters — today that is
/// `serveAxonTls` and both long `registerNetwork` variants, three of the most privileged faces in
/// the tree. This scanner is whitespace- and newline-agnostic by construction.
fn rust_faces(src: &Path) -> BTreeSet<String> {
    const OPEN: &str = "#[precompile::public";
    let mut out = BTreeSet::new();

    let dir = fs::read_dir(src).expect("read precompiles/src");
    for entry in dir {
        let path = entry.expect("dir entry").path();
        if path.extension().and_then(|e| e.to_str()) != Some("rs") {
            continue;
        }
        let body = fs::read_to_string(&path).expect("read .rs source");
        let mut rest: &str = &body;

        while let Some(open_at) = rest.find(OPEN) {
            let Some(after) = rest.get(open_at.saturating_add(OPEN.len())..) else {
                break;
            };
            // The first quoted string after the attribute name is the signature, however it wraps.
            let Some(q1) = after.find('"') else { break };
            let Some(tail) = after.get(q1.saturating_add(1)..) else {
                break;
            };
            let Some(q2) = tail.find('"') else { break };
            let Some(sig) = tail.get(..q2) else { break };

            out.insert(sig.trim().to_string());

            rest = tail.get(q2..).unwrap_or_default();
        }
    }
    out
}

/// Every `type: "function"` entry across `precompiles/src/solidity/*.abi`, as a canonical signature.
///
/// Deliberately module-agnostic: it unions every `.abi` file rather than pairing each to a Rust
/// module. Rust modules are `snake_case` and the artifacts are `camelCase`, and `staking.rs` alone
/// declares both `staking` and `stakingV2` — a per-module mapping is a second thing that can be
/// wrong, and it fails in the direction that manufactures findings.
fn abi_faces(solidity: &Path) -> BTreeSet<String> {
    let mut out = BTreeSet::new();

    let dir = fs::read_dir(solidity).expect("read precompiles/src/solidity");
    for entry in dir {
        let path = entry.expect("dir entry").path();
        if path.extension().and_then(|e| e.to_str()) != Some("abi") {
            continue;
        }
        let raw = fs::read_to_string(&path).expect("read .abi artifact");
        let json: serde_json::Value = serde_json::from_str(&raw)
            .unwrap_or_else(|e| panic!("{}: invalid JSON: {e}", path.display()));
        let entries = json
            .as_array()
            .unwrap_or_else(|| panic!("{}: expected a JSON array", path.display()));

        for entry in entries {
            if entry.get("type").and_then(|t| t.as_str()) != Some("function") {
                continue;
            }
            let name = entry.get("name").and_then(|n| n.as_str()).unwrap_or_default();
            let args = entry
                .get("inputs")
                .and_then(|i| i.as_array())
                .map(|inputs| {
                    inputs
                        .iter()
                        .map(|i| i.get("type").and_then(|t| t.as_str()).unwrap_or_default())
                        .collect::<Vec<_>>()
                        .join(",")
                })
                .unwrap_or_default();
            out.insert(format!("{name}({args})"));
        }
    }
    out
}

#[test]
fn vendored_abi_matches_precompile_public_faces() {
    let root = crate_root();
    let rust = rust_faces(&root.join("src"));
    let abi = abi_faces(&root.join("src").join("solidity"));

    // Guard the scanners themselves — an empty side would make either diff pass vacuously.
    assert!(
        rust.len() > 100,
        "rust face scan returned {}; the scanner is broken, not the tree",
        rust.len()
    );
    assert!(
        abi.len() > 100,
        "abi function scan returned {}; the scanner is broken, not the tree",
        abi.len()
    );

    let allow: BTreeSet<String> = ABI_ONLY_ALLOWLIST.iter().map(|s| (*s).to_string()).collect();

    let missing_from_abi: Vec<&String> = rust.difference(&abi).collect();
    let missing_from_rust: Vec<&String> = abi
        .difference(&rust)
        .filter(|sig| !allow.contains(*sig))
        .collect();

    let mut problems = String::new();

    if !missing_from_abi.is_empty() {
        problems.push_str(&format!(
            "\n{} face(s) declared #[precompile::public] but absent from every .abi in \
             precompiles/src/solidity/ — integrators enumerating the vendored ABI cannot see these:\n",
            missing_from_abi.len()
        ));
        for sig in &missing_from_abi {
            problems.push_str(&format!("    {sig}\n"));
        }
    }

    if !missing_from_rust.is_empty() {
        problems.push_str(&format!(
            "\n{} function(s) in the vendored .abi with no #[precompile::public] face — either the \
             face was removed and the artifact not updated, or the entry belongs in \
             ABI_ONLY_ALLOWLIST with a comment explaining why:\n",
            missing_from_rust.len()
        ));
        for sig in &missing_from_rust {
            problems.push_str(&format!("    {sig}\n"));
        }
    }

    assert!(
        problems.is_empty(),
        "vendored Solidity artifacts have drifted from the Rust source of truth:\n{problems}\n\
         rust faces: {}, abi functions: {}",
        rust.len(),
        abi.len()
    );
}
