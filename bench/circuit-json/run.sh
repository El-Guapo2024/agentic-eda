#!/usr/bin/env bash
# Differential validation of our Rust circuit-json exporter against the
# official tscircuit `circuit-json` zod schema.
#
# 1. builds the CLI
# 2. runs the pipeline (schematic -> place -> route) over a set of example
#    intents across several seeds, plus schematic-only and placement-only
#    (no routing) engine runs, to cover partial designs
# 3. validates every circuit.json produced against the official schema and
#    renders schematic + PCB SVGs with the official circuit-to-svg
# 4. writes bench/circuit-json/report.md summarizing the results
set -euo pipefail

BENCH_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$BENCH_DIR/../.." && pwd)"
OUT_DIR="$BENCH_DIR/out"
BIN="$REPO_ROOT/target/debug/eda"
REPORT="$BENCH_DIR/report.md"

# Example intents that fully route on their own (no pre-existing gate
# failures unrelated to this bench); ldo.yaml is included only for its
# schematic-only / placement-only (no-routing) partial-design coverage.
FULL_EXAMPLES=(dense_small_outline nc_pins opamp_filter passive_divider_ladder star_net two_pin_nets)
SEEDS=(0 1 2 3 4 5)

echo "==> building eda-cli"
( cd "$REPO_ROOT" && cargo build -p eda-cli )

echo "==> building eda-interchange test suite"
( cd "$REPO_ROOT" && cargo test -p eda-interchange )

echo "==> installing bench node dependencies"
( cd "$BENCH_DIR" && npm install --no-audit --no-fund >/dev/null )

rm -rf "$OUT_DIR"
mkdir -p "$OUT_DIR"

echo "==> generating corpus"
for ex in "${FULL_EXAMPLES[@]}"; do
  for seed in "${SEEDS[@]}"; do
    d="$OUT_DIR/${ex}_seed${seed}"
    "$BIN" pipeline "$REPO_ROOT/examples/${ex}.yaml" -o "$d" --seed "$seed" > "$d.log" 2>&1 || true
  done
done

# Partial-design coverage: schematic only, and placement without routing.
"$BIN" schematic "$REPO_ROOT/examples/ldo.yaml" -o "$OUT_DIR/schematic_only" --seed 0 > "$OUT_DIR/schematic_only.log" 2>&1 || true
"$BIN" place "$REPO_ROOT/examples/ldo.yaml" -o "$OUT_DIR/place_only" --seed 0 > "$OUT_DIR/place_only.log" 2>&1 || true

echo "==> validating circuit.json files against the official schema"
{
  echo "# circuit-json differential validation report"
  echo
  echo "Generated: $(date -u +%Y-%m-%dT%H:%M:%SZ)"
  echo
} > "$REPORT"

total_files=0
total_errors=0
counts_file="$(mktemp)"

for f in $(find "$OUT_DIR" -name circuit.json | sort); do
  total_files=$((total_files + 1))
  rel="${f#"$REPO_ROOT"/}"
  echo "--- $rel"
  set +e
  out=$(node "$BENCH_DIR/validate.mjs" "$f" 2>&1)
  status=$?
  set -e
  echo "$out"
  err_count=$(echo "$out" | grep -m1 "^validation errors:" | grep -o '[0-9]*' || echo "?")
  total_errors=$((total_errors + err_count))
  {
    echo "## \`$rel\`"
    echo
    echo '```'
    echo "$out"
    echo '```'
    echo
  } >> "$REPORT"
  if [ "$status" -ne 0 ]; then
    echo "FAILED: $rel (errors or SVG render failure)" >&2
  fi
  echo "$out" | grep -E '^  [a-z_]+: [0-9]+$' >> "$counts_file" || true
done

{
  echo "## Summary"
  echo
  echo "- files validated: $total_files"
  echo "- total validation errors: $total_errors"
  echo
  echo "### Element counts across the whole corpus"
  echo
  awk '{ n[$1]+=$2 } END { for (t in n) printf "- %s %d\n", t, n[t] }' "$counts_file" | sort
} >> "$REPORT"
rm -f "$counts_file"

echo
echo "==> report written to $REPORT"
echo "files validated: $total_files, total validation errors: $total_errors"

if [ "$total_errors" -ne 0 ]; then
  exit 1
fi
