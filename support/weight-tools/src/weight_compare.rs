//! Compare two weights.rs files and report benchmark-level drift.
//!
//! Parses both files with `syn`, extracts per-function weight data including
//! base values, proof sizes, and parameterized slopes, then compares the whole
//! generated weight/proof across the benchmarked component ranges with a
//! configurable percentage threshold.
//!
//! Exit codes:
//! 0 — all within threshold
//! 1 — error
//! 2 — drift exceeds threshold

use anyhow::{Context, Result};
use clap::Parser;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::PathBuf;
use syn::{Attribute, Expr, File, ImplItem, Item, Lit, ReturnType, Stmt};

#[derive(Parser)]
#[command(about = "Compare two weights.rs files and report drift")]
struct Cli {
    #[arg(long)]
    old: PathBuf,

    #[arg(long)]
    new: PathBuf,

    #[arg(long, default_value = "40")]
    threshold: f64,
}

/// All weight data for a single function.
#[derive(Debug, Clone, Default)]
struct WeightValues {
    base_weight: u64,
    proof_size: u64,
    base_reads: u64,
    base_writes: u64,
    /// Per-component ref-time slopes, keyed by benchmark component name.
    component_weights: BTreeMap<String, u64>,
    component_reads: BTreeMap<String, u64>,
    component_writes: BTreeMap<String, u64>,
    /// Per-component proof-size slopes, keyed by benchmark component name.
    component_proof: BTreeMap<String, u64>,
    /// Component ranges from generated docs, keyed by benchmark component name.
    component_ranges: BTreeMap<String, (u64, u64)>,
}

type ComponentRange = (String, u64, u64);

#[derive(Debug, Clone, Copy)]
struct Drift {
    signed_pct: f64,
    abs_pct: f64,
}

impl Drift {
    fn new(signed_pct: f64) -> Self {
        Self {
            signed_pct,
            abs_pct: signed_pct.abs(),
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum DriftMetric {
    RefTime,
    ProofSize,
}

impl DriftMetric {
    fn total_at(
        self,
        values: &WeightValues,
        ranges: &[ComponentRange],
        component_values: &[u64],
    ) -> u128 {
        match self {
            Self::RefTime => total_ref_time_at(values, ranges, component_values),
            Self::ProofSize => total_proof_size_at(values, ranges, component_values),
        }
    }
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let old_weights = parse_file(&cli.old)?;
    let new_weights = parse_file(&cli.new)?;

    let mut any_drift = false;

    for (name, nw) in &new_weights {
        let ow = old_weights.get(name).cloned().unwrap_or_default();
        let mut reasons: Vec<String> = Vec::new();

        let weight_drift = max_ref_time_drift(&ow, nw);
        if weight_drift.abs_pct > cli.threshold {
            reasons.push(format!("max_weight {:+.1}%", weight_drift.signed_pct));
        }

        let proof_drift = max_proof_size_drift(&ow, nw);
        if proof_drift.abs_pct > cli.threshold {
            reasons.push(format!("max_proof_size {:+.1}%", proof_drift.signed_pct));
        }

        let old_components: Vec<_> = ow.component_ranges.keys().cloned().collect();
        let new_components: Vec<_> = nw.component_ranges.keys().cloned().collect();
        if !old_components.is_empty()
            && !new_components.is_empty()
            && old_components != new_components
        {
            reasons.push(format!(
                "components {:?} -> {:?}",
                old_components, new_components
            ));
        }

        // Storage I/O changes are structural, not percent drift, so keep them exact.
        if nw.base_reads != ow.base_reads {
            reasons.push(format!("reads {} -> {}", ow.base_reads, nw.base_reads));
        }
        if nw.base_writes != ow.base_writes {
            reasons.push(format!("writes {} -> {}", ow.base_writes, nw.base_writes));
        }
        check_map_exact(
            &ow.component_reads,
            &nw.component_reads,
            "component_reads",
            &mut reasons,
        );
        check_map_exact(
            &ow.component_writes,
            &nw.component_writes,
            "component_writes",
            &mut reasons,
        );

        let drifted = !reasons.is_empty();
        if drifted {
            any_drift = true;
        }

        let base_pct = signed_pct(ow.base_weight as u128, nw.base_weight as u128);
        let proof_pct = signed_pct(ow.proof_size as u128, nw.proof_size as u128);
        let icon = if drifted { "\u{274c}" } else { "\u{2705}" };
        let mut detail_parts: Vec<String> = Vec::new();
        if ow.proof_size != nw.proof_size || proof_drift.abs_pct > cli.threshold {
            detail_parts.push(format!(
                "proof {} -> {} ({:+.1}%; max {:+.1}%)",
                ow.proof_size, nw.proof_size, proof_pct, proof_drift.signed_pct,
            ));
        }
        if ow.base_reads != nw.base_reads || ow.base_writes != nw.base_writes {
            detail_parts.push(format!(
                "io reads {} -> {} writes {} -> {}",
                ow.base_reads, nw.base_reads, ow.base_writes, nw.base_writes,
            ));
        }
        if !reasons.is_empty() {
            detail_parts.push(format!("reasons: {}", reasons.join(", ")));
        }
        let detail_suffix = if detail_parts.is_empty() {
            String::new()
        } else {
            format!(" {}", detail_parts.join(" "))
        };

        println!(
            " {} {:<40} {:>12} -> {:<12} ({:>+.1}%; max {:>+.1}%){}",
            icon,
            name,
            ow.base_weight,
            nw.base_weight,
            base_pct,
            weight_drift.signed_pct,
            detail_suffix,
        );
    }

    for name in old_weights.keys() {
        if !new_weights.contains_key(name) {
            println!(" \u{274c} {:<40} REMOVED", name);
            any_drift = true;
        }
    }

    if any_drift {
        std::process::exit(2);
    }

    Ok(())
}

fn check_map_exact(
    old: &BTreeMap<String, u64>,
    new: &BTreeMap<String, u64>,
    label: &str,
    reasons: &mut Vec<String>,
) {
    if old != new {
        reasons.push(format!("{label} {:?} -> {:?}", old, new));
    }
}

fn signed_pct(old: u128, new: u128) -> f64 {
    if old > 0 {
        (new as f64 - old as f64) / old as f64 * 100.0
    } else if new > 0 {
        100.0
    } else {
        0.0
    }
}

fn max_ref_time_drift(old: &WeightValues, new: &WeightValues) -> Drift {
    max_benchmark_drift(old, new, DriftMetric::RefTime)
}

fn max_proof_size_drift(old: &WeightValues, new: &WeightValues) -> Drift {
    max_benchmark_drift(old, new, DriftMetric::ProofSize)
}

fn max_benchmark_drift(old: &WeightValues, new: &WeightValues, metric: DriftMetric) -> Drift {
    let ranges = comparison_ranges(old, new);
    if ranges.is_empty() {
        return Drift::new(signed_pct(
            metric.total_at(old, &[], &[]),
            metric.total_at(new, &[], &[]),
        ));
    }

    let mut values = Vec::with_capacity(ranges.len());
    let mut max_drift = Drift::new(0.0);
    visit_range_corners(&ranges, 0, &mut values, old, new, metric, &mut max_drift);
    max_drift
}

fn visit_range_corners(
    ranges: &[ComponentRange],
    idx: usize,
    values: &mut Vec<u64>,
    old: &WeightValues,
    new: &WeightValues,
    metric: DriftMetric,
    max_drift: &mut Drift,
) {
    if idx == ranges.len() {
        let old_total = metric.total_at(old, ranges, values);
        let new_total = metric.total_at(new, ranges, values);
        let drift = Drift::new(signed_pct(old_total, new_total));
        if drift.abs_pct > max_drift.abs_pct {
            *max_drift = drift;
        }
        return;
    }

    let (_, min, max) = &ranges[idx];
    values.push(*min);
    visit_range_corners(ranges, idx + 1, values, old, new, metric, max_drift);
    values.pop();

    if max != min {
        values.push(*max);
        visit_range_corners(ranges, idx + 1, values, old, new, metric, max_drift);
        values.pop();
    }
}

fn comparison_ranges(old: &WeightValues, new: &WeightValues) -> Vec<ComponentRange> {
    let mut names = BTreeSet::new();
    names.extend(old.component_ranges.keys().cloned());
    names.extend(new.component_ranges.keys().cloned());
    names.extend(old.component_weights.keys().cloned());
    names.extend(new.component_weights.keys().cloned());
    names.extend(old.component_proof.keys().cloned());
    names.extend(new.component_proof.keys().cloned());

    names
        .into_iter()
        .map(|name| {
            let old_range = range_for_component(old, &name);
            let new_range = range_for_component(new, &name);
            let min = old_range.0.min(new_range.0);
            let max = old_range.1.max(new_range.1).max(min);
            (name, min, max)
        })
        .filter(|(_, min, max)| *min != 0 || *max != 0)
        .collect()
}

fn range_for_component(values: &WeightValues, name: &str) -> (u64, u64) {
    values
        .component_ranges
        .get(name)
        .copied()
        .unwrap_or_else(|| {
            if values.component_weights.contains_key(name)
                || values.component_proof.contains_key(name)
            {
                // Generated files should include ranges. Fall back to a one-unit
                // multiplier if a legacy file does not, so slope-only changes are
                // still represented in the benchmark-level total.
                (0, 1)
            } else {
                (0, 0)
            }
        })
}

fn total_ref_time_at(
    values: &WeightValues,
    ranges: &[ComponentRange],
    component_values: &[u64],
) -> u128 {
    total_at(
        values.base_weight,
        &values.component_weights,
        ranges,
        component_values,
    )
}

fn total_proof_size_at(
    values: &WeightValues,
    ranges: &[ComponentRange],
    component_values: &[u64],
) -> u128 {
    total_at(
        values.proof_size,
        &values.component_proof,
        ranges,
        component_values,
    )
}

fn total_at(
    base: u64,
    components: &BTreeMap<String, u64>,
    ranges: &[ComponentRange],
    component_values: &[u64],
) -> u128 {
    let mut total = base as u128;

    for (name, slope) in components {
        let multiplier = ranges
            .iter()
            .position(|(range_name, _, _)| range_name == name)
            .and_then(|idx| component_values.get(idx))
            .copied()
            .unwrap_or(0);

        total = total.saturating_add((*slope as u128).saturating_mul(multiplier as u128));
    }

    total
}

// ── Parsing ─────────────────────────────────────────────────────────────────

fn parse_file(path: &PathBuf) -> Result<BTreeMap<String, WeightValues>> {
    let src = fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    let file: File = syn::parse_str(&src).with_context(|| format!("parsing {}", path.display()))?;

    let mut result = BTreeMap::new();

    for item in &file.items {
        let Item::Impl(impl_block) = item else {
            continue;
        };
        if !is_substrate_weight_impl(impl_block) {
            continue;
        }

        for impl_item in &impl_block.items {
            let ImplItem::Fn(method) = impl_item else {
                continue;
            };
            if !matches!(&method.sig.output, ReturnType::Type(..)) {
                continue;
            }

            let mut wv = WeightValues {
                component_ranges: parse_component_ranges(&method.attrs),
                ..WeightValues::default()
            };

            for stmt in &method.block.stmts {
                if let Stmt::Expr(expr, _) = stmt {
                    walk(expr, &mut wv, None);
                }
            }

            result.insert(method.sig.ident.to_string(), wv);
        }
        break;
    }

    Ok(result)
}

fn is_substrate_weight_impl(impl_block: &syn::ItemImpl) -> bool {
    let self_ty = match impl_block.self_ty.as_ref() {
        syn::Type::Path(tp) => tp
            .path
            .segments
            .iter()
            .map(|s| s.ident.to_string())
            .collect::<String>(),
        _ => return false,
    };

    if !self_ty.contains("SubstrateWeight") {
        return false;
    }

    impl_block
        .trait_
        .as_ref()
        .and_then(|(_, path, _)| path.segments.last())
        .map(|s| s.ident == "WeightInfo")
        .unwrap_or(false)
}

/// Walk a method-call chain. `component` is set while walking the receiver of
/// `.saturating_mul(component.into())`, which means the extracted values are
/// per-component slopes rather than base values.
fn walk(expr: &Expr, wv: &mut WeightValues, component: Option<&str>) {
    match expr {
        Expr::Call(call) => {
            if is_from_parts(&call.func) && call.args.len() >= 2 {
                let v = expr_to_u64(&call.args[0]).unwrap_or(0);
                let p = expr_to_u64(&call.args[1]).unwrap_or(0);

                if let Some(name) = component {
                    add_component(&mut wv.component_weights, Some(name), v);
                    add_component(&mut wv.component_proof, Some(name), p);
                } else {
                    wv.base_weight = v;
                    wv.proof_size = p;
                }
            }
        }
        Expr::MethodCall(mc) => match mc.method.to_string().as_str() {
            "saturating_add" => {
                walk(&mc.receiver, wv, component);
                if let Some(arg) = mc.args.first() {
                    walk(arg, wv, component);
                }
            }
            "saturating_mul" => {
                // Receiver is the component slope value; the first argument is
                // usually the benchmark component, e.g. `a.into()`.
                let name = mc.args.first().and_then(component_name);
                walk(&mc.receiver, wv, name.as_deref());
            }
            "reads" => {
                extract_rw(
                    mc.args.first(),
                    component,
                    &mut wv.base_reads,
                    &mut wv.component_reads,
                );
                walk(&mc.receiver, wv, component);
            }
            "writes" => {
                extract_rw(
                    mc.args.first(),
                    component,
                    &mut wv.base_writes,
                    &mut wv.component_writes,
                );
                walk(&mc.receiver, wv, component);
            }
            _ => walk(&mc.receiver, wv, component),
        },
        Expr::Paren(p) => walk(&p.expr, wv, component),
        _ => {}
    }
}

/// Extract a reads/writes value from the argument to `.reads()` / `.writes()`.
/// Handles both plain literals and `(N_u64).saturating_mul(k.into())` patterns.
fn extract_rw(
    arg: Option<&Expr>,
    component: Option<&str>,
    base: &mut u64,
    components: &mut BTreeMap<String, u64>,
) {
    let Some(arg) = arg else { return };

    // Direct literal: .reads(2_u64)
    if let Some(n) = expr_to_u64(arg) {
        if let Some(name) = component {
            add_component(components, Some(name), n);
        } else {
            *base += n;
        }
        return;
    }

    // Component pattern: .reads((2_u64).saturating_mul(k.into()))
    if let Expr::MethodCall(inner_mc) = arg
        && inner_mc.method == "saturating_mul"
        && let Some(n) = expr_to_u64(&inner_mc.receiver)
    {
        let name = inner_mc.args.first().and_then(component_name);
        add_component(components, name.as_deref(), n);
    }
}

fn add_component(map: &mut BTreeMap<String, u64>, component: Option<&str>, value: u64) {
    if value == 0 {
        return;
    }

    let key = component
        .map(str::to_owned)
        .unwrap_or_else(|| format!("__unknown_{}", map.len()));
    *map.entry(key).or_default() += value;
}

fn parse_component_ranges(attrs: &[Attribute]) -> BTreeMap<String, (u64, u64)> {
    attrs
        .iter()
        .filter_map(doc_attr_value)
        .filter_map(|doc| parse_component_range(&doc))
        .collect()
}

fn doc_attr_value(attr: &Attribute) -> Option<String> {
    if !attr.path().is_ident("doc") {
        return None;
    }

    let syn::Meta::NameValue(meta) = &attr.meta else {
        return None;
    };
    let Expr::Lit(expr_lit) = &meta.value else {
        return None;
    };
    let Lit::Str(lit) = &expr_lit.lit else {
        return None;
    };

    Some(lit.value())
}

fn parse_component_range(doc: &str) -> Option<(String, (u64, u64))> {
    let doc = doc.trim();
    let rest = doc.strip_prefix("The range of component `")?;
    let (name, rest) = rest.split_once("` is `[")?;
    let (range, _) = rest.split_once(']')?;
    let (min, max) = range.split_once(',')?;

    Some((
        name.to_owned(),
        (parse_doc_u64(min.trim())?, parse_doc_u64(max.trim())?),
    ))
}

fn parse_doc_u64(raw: &str) -> Option<u64> {
    raw.replace('_', "").parse().ok()
}

fn component_name(expr: &Expr) -> Option<String> {
    match expr {
        Expr::Path(path) => path.path.segments.last().map(|s| s.ident.to_string()),
        Expr::MethodCall(mc) if mc.method == "into" => component_name(&mc.receiver),
        Expr::Paren(p) => component_name(&p.expr),
        Expr::Reference(r) => component_name(&r.expr),
        Expr::Cast(c) => component_name(&c.expr),
        _ => None,
    }
}

fn is_from_parts(func: &Expr) -> bool {
    matches!(func, Expr::Path(p) if p.path.segments.last().map(|s| s.ident == "from_parts").unwrap_or(false))
}

fn expr_to_u64(expr: &Expr) -> Option<u64> {
    match expr {
        Expr::Lit(lit) => match &lit.lit {
            Lit::Int(i) => i.base10_parse::<u64>().ok(),
            _ => None,
        },
        Expr::Cast(c) => expr_to_u64(&c.expr),
        Expr::MethodCall(mc) if mc.method == "into" => expr_to_u64(&mc.receiver),
        Expr::Paren(p) => expr_to_u64(&p.expr),
        _ => None,
    }
}
