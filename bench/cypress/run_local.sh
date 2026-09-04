#!/usr/bin/env bash
# Cypress (original placer, native CPU build) vs eda-place (port), judged by
# our own gates and HPWL, then routed by eda-router. One row per intent.
#
#   CYPRESS_INSTALL=~/ws/Cypress/install  bash bench/cypress/run_local.sh [seed]
# (activate the conda env that has torch first: `conda activate cypress`)
set -euo pipefail
cd "$(dirname "$0")/../.."
SEED="${1:-0}"
cargo build -q -p eda-cli
OUT="${CYPRESS_OUT:-$(mktemp -d /tmp/eda_bench_cypress.XXXXXX)}"
REPORT=bench/cypress/report_local.md
K=/Applications/KiCad/KiCad.app/Contents/MacOS/kicad-cli
{
  echo "# Cypress (native CPU) vs eda-place"
  echo
  echo "Generated: $(date -u +%Y-%m-%dT%H:%M:%SZ) · seed $SEED · Cypress CPU build, judged by eda-gates"
  echo
  echo "| intent | parts | eda-place hpwl (µm) | cypress hpwl (µm) | ratio | cypress placement gate | routed after cypress | kicad DRC |"
  echo "|---|---|---|---|---|---|---|---|"
} > "$REPORT"
for f in examples/*.yaml; do
  n=$(basename "$f" .yaml)
  case "$n" in unroutable_tiny_outline) continue;; esac
  d="$OUT/$n"; mkdir -p "$d"
  if ! ./target/debug/eda place "$f" -o "$d" --seed "$SEED" >"$d/place.log" 2>&1; then
    echo "| $n | - | place failed | - | - | - | - | - |" >> "$REPORT"; continue
  fi
  ./target/debug/eda export "$f" --design "$d/design.json" -o "$d" >/dev/null 2>&1 || true
  parts=$(grep -c "^  - reference:" "$f" || true)
  ours=$(python3 - "$d/runs.jsonl" <<'PY'
import json,sys
h=None
for l in open(sys.argv[1]):
    e=json.loads(l)
    if e.get("kind")=="candidate" and e.get("stage")=="placement": h=e.get("metrics",{}).get("hpwl_um")
print(h if h is not None else "-")
PY
)
  mkdir -p "$d/cypress"
  python3 bench/cypress/local_config.py "$d/bookshelf/$n.aux" "$d/cypress/results" "$((1000+SEED))" > "$d/cypress/config.json"
  if ! ( cd "${CYPRESS_INSTALL:-$HOME/ws/Cypress/install}" && python dreamplace/Placer.py "$d/cypress/config.json" ) >"$d/cypress/cypress.log" 2>&1; then
    echo "| $n | $parts | $ours | cypress failed (see $d/cypress/cypress.log) | - | - | - | - |" >> "$REPORT"; continue
  fi
  cp "$d/cypress/results/$n/$n.gp.pl" "$d/cypress/$n.gp.pl" 2>/dev/null || { echo "| $n | $parts | $ours | no .pl written | - | - | - | - |" >> "$REPORT"; continue; }
  mkdir -p "$d/cy"
  if ./target/debug/eda import-pl "$f" --design "$d/design.json" --pl "$d/cypress/$n.gp.pl" -o "$d/cy" >"$d/cy/import.log" 2>&1; then gate="clean"; else gate="FAIL"; fi
  cy=$(grep -m1 "^hpwl_um" "$d/cy/import.log" | awk '{print $2}')
  ratio=$(python3 -c "print(f'{$cy/$ours:.2f}') if '$cy'.isdigit() and '$ours'.isdigit() and $ours>0 else print('-')" 2>/dev/null || echo "-")
  routed="-"; drc="-"
  if [ "$gate" = "clean" ]; then
    if ./target/debug/eda route "$f" --design "$d/cy/design.json" -o "$d/cy" --seed "$SEED" >"$d/cy/route.log" 2>&1; then
      routed="yes"
      if [ -x "$K" ] && [ -f "$d/cy/$n.kicad_pcb" ]; then
        "$K" pcb drc --format json --severity-all -o "$d/cy/drc.json" "$d/cy/$n.kicad_pcb" >/dev/null 2>&1 || true
        drc=$(python3 -c "
import json;d=json.load(open('$d/cy/drc.json'));print(sum(1 for v in d['violations'] if v['type'] in ('clearance','shorting_items','track_width','unconnected_items'))+len(d.get('unconnected_items',[])))" 2>/dev/null || echo "?")
      fi
    else routed="no"; fi
  fi
  echo "| $n | $parts | $ours | $cy | $ratio | $gate | $routed | $drc |" >> "$REPORT"
done
echo "wrote $REPORT (work dir $OUT)"
