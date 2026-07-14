#!/usr/bin/env bash
set -euo pipefail

# Auto-discover benchmarked runtime benchmark targets.
#
# Finds all pallets under pallets/ that have both:
# - src/benchmarking.rs (or src/benchmarks.rs)
# - src/weights.rs
#
# Then filters that list to pallets actually registered in runtime/src/lib.rs
# define_benchmarks!(...). A pallet having benchmark files is not enough for:
#
#   node-subtensor benchmark pallet --pallet=<name>
#
# The pallet must also be present in the runtime benchmark registry.
#
# Also includes runtime-owned benchmark targets that are registered in
# runtime/src/lib.rs via define_benchmarks!.
#
# Outputs one line per target: "benchmark_name path/to/weights.rs"
# The pallet name is derived from the Cargo.toml `name` field with dashes -> underscores.

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
RUNTIME_FILE="$ROOT_DIR/runtime/src/lib.rs"

RUNTIME_BENCHMARKS="$(
    perl -0ne '
        if (/define_benchmarks!\s*\((.*?)\)\s*;/s) {
            my $body = $1;
            while ($body =~ /\[\s*([A-Za-z0-9_:]+)\s*,/g) {
                my $name = $1;
                $name =~ s/::.*$//;
                print "$name\n";
            }
        }
    ' "$RUNTIME_FILE" | sort -u
)"

for dir in "$ROOT_DIR"/pallets/*/; do
    [ -f "$dir/src/weights.rs" ] || continue
    [ -f "$dir/src/benchmarking.rs" ] || [ -f "$dir/src/benchmarks.rs" ] || continue

    name="$(
        awk -F '"' '/^name[[:space:]]*=/ { print $2; exit }' "$dir/Cargo.toml" \
            | tr '-' '_'
    )"

    [ -n "$name" ] || continue

    if ! printf '%s\n' "$RUNTIME_BENCHMARKS" | grep -qxF "$name"; then
        continue
    fi

    relpath="pallets/$(basename "$dir")/src/weights.rs"
    echo "$name $relpath"
done

if printf '%s\n' "$RUNTIME_BENCHMARKS" | grep -qxF "governance" && \
   [ -f "$ROOT_DIR/runtime/src/governance/benchmarking.rs" ] && \
   [ -f "$ROOT_DIR/runtime/src/governance/weights.rs" ]; then
    echo "governance runtime/src/governance/weights.rs"
fi
