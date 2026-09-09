#!/usr/bin/env bash
# One loop round over a set of intents: pipeline (+judge when a key is set),
# stage renders, and an HTML page. Usage:
#   bash bench/loop/run.sh <out_dir> [seed] [intent files...]   (default: examples/*.yaml examples/ladder/*.yaml)
# EDA_PLACER=cypress|anneal selects the placer (default anneal).
# EDA_BIN=<path> runs that binary instead of building target/release/eda
# (so a concurrent cargo build/test cannot relink it mid-run).
set -uo pipefail
cd "$(dirname "$0")/../.."
OUT="${1:?out dir}"; SEED="${2:-0}"; shift 2 || true
PL=(--placer "${EDA_PLACER:-anneal}")
FILES=("$@"); [ ${#FILES[@]} -eq 0 ] && FILES=(examples/*.yaml examples/ladder/*.yaml)
if [ -n "${EDA_BIN:-}" ]; then BIN="$EDA_BIN"; else cargo build -q --release -p eda-cli || exit 1; BIN=./target/release/eda; fi
mkdir -p "$OUT"
for f in "${FILES[@]}"; do
  [ -f "$f" ] || continue
  n=$(basename "$f" .yaml); d="$OUT/$n"; mkdir -p "$d"
  s=$(date +%s.%N)
  if [ -n "${ANTHROPIC_API_KEY:-}" ]; then "$BIN" pipeline "$f" -o "$d" --seed "$SEED" "${PL[@]}" --judge > "$d/log.txt" 2>&1
  else "$BIN" pipeline "$f" -o "$d" --seed "$SEED" "${PL[@]}" > "$d/log.txt" 2>&1; fi
  echo "exit=$?" >> "$d/log.txt"
  # Stages are independent: a failing schematic must not hide placement/routing
  # results. Drive them separately (still gated) so every stage gets measured.
  if ! grep -q "placement gates:" "$d/log.txt"; then
    { echo "--- placement (schematic failed, run separately)"; "$BIN" place "$f" --design "$d/design.json" -o "$d" --seed "$SEED" "${PL[@]}"; } >> "$d/log.txt" 2>&1
    if grep -q "placement gates: .* 0 fail" "$d/log.txt"; then
      { echo "--- routing"; "$BIN" route "$f" --design "$d/design.json" -o "$d" --seed "$SEED"; } >> "$d/log.txt" 2>&1
    fi
  fi
  # renders for every stage present (judge writes them before calling the API; without a key it fails after)
  "$BIN" judge "$f" --design "$d/design.json" -o "$d" > "$d/judge.txt" 2>&1
  cp "$f" "$d/intent.yaml"
  echo "wall_s=$(echo "$(date +%s.%N) - $s" | bc)" >> "$d/log.txt"
  echo "$n: $(grep -E 'gates:' "$d/log.txt" | tr '\n' ' ')"
done
python3 bench/loop/page.py "$OUT" > "$OUT/index.html" && echo "page: $OUT/index.html"
