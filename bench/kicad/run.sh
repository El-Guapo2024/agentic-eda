#!/usr/bin/env bash
# bench/kicad/run.sh — runs every examples/*.yaml through the pipeline at
# seeds 0..3, exports KiCad files, runs kicad-cli ERC/DRC on the result, and
# appends a summary to bench/kicad/report.md. Tolerant of any one case
# failing (logs and continues) — never aborts the whole sweep, and never
# swallows an error silently: stderr from a failing step is always printed.
set -u
cd "$(dirname "${BASH_SOURCE[0]}")/../.."
REPO_ROOT="$(pwd)"
BUILD_PROFILE="debug"
EDA_BIN="target/${BUILD_PROFILE}/eda"

find_kicad_cli() {
  if command -v kicad-cli >/dev/null 2>&1; then
    command -v kicad-cli
    return 0
  fi
  local mac="/Applications/KiCad/KiCad.app/Contents/MacOS/kicad-cli"
  if [ -x "$mac" ]; then
    echo "$mac"
    return 0
  fi
  return 1
}

echo "==> building eda-cli (${BUILD_PROFILE})"
cargo build -p eda-cli 2>&1 | tail -20
if [ ! -x "$EDA_BIN" ]; then
  echo "eda-cli binary not found at $EDA_BIN after build" >&2
  exit 1
fi

KICAD_CLI="$(find_kicad_cli || true)"
if [ -z "${KICAD_CLI:-}" ]; then
  echo "kicad-cli not found (checked PATH and /Applications/KiCad/KiCad.app); ERC/DRC columns will be marked n/a" >&2
fi

REPORT="bench/kicad/report.md"
WORKDIR="$(mktemp -d "${TMPDIR:-/tmp}/eda_bench_kicad.XXXXXX")"

{
  echo "# KiCad export bench report"
  echo
  echo "Generated $(date -u +%Y-%m-%dT%H:%M:%SZ) by \`bench/kicad/run.sh\` (cargo build profile: ${BUILD_PROFILE})."
  echo
  echo "kicad-cli: ${KICAD_CLI:-not found}"
  echo
  echo "| case | seed | pipeline | ERC violations | DRC violations (in-scope: clearance/track_width/shorting_items/unconnected_items) | DRC violations (other, informational) | notes |"
  echo "|---|---|---|---|---|---|---|"
} > "$REPORT"

count_json_array() {
  # $1 = json file, $2 = jq-ish path via python (no jq dependency assumed)
  python3 - "$1" "$2" <<'PY'
import json, sys
path, key = sys.argv[1], sys.argv[2]
try:
    d = json.load(open(path))
except Exception:
    print(0)
    sys.exit(0)
v = d.get(key, [])
print(len(v) if isinstance(v, list) else 0)
PY
}

count_in_scope_drc() {
  python3 - "$1" <<'PY'
import json, sys
path = sys.argv[1]
in_scope = {"clearance", "track_width", "shorting_items", "unconnected_items"}
try:
    d = json.load(open(path))
except Exception:
    print("0")
    sys.exit(0)
n = 0
for v in d.get("violations", []):
    if v.get("type") in in_scope:
        n += 1
n += len(d.get("unconnected_items", []))
print(n)
PY
}

count_other_drc() {
  python3 - "$1" <<'PY'
import json, sys
path = sys.argv[1]
in_scope = {"clearance", "track_width", "shorting_items", "unconnected_items"}
try:
    d = json.load(open(path))
except Exception:
    print("0")
    sys.exit(0)
n = sum(1 for v in d.get("violations", []) if v.get("type") not in in_scope)
print(n)
PY
}

shopt -s nullglob
for intent in examples/*.yaml; do
  case_name="$(basename "$intent" .yaml)"
  for seed in 0 1 2 3; do
    out_dir="${WORKDIR}/${case_name}_${seed}"
    mkdir -p "$out_dir"
    echo "==> ${case_name} seed=${seed}"

    pipeline_log="${out_dir}/pipeline.log"
    if ! "$EDA_BIN" pipeline "$intent" -o "$out_dir" --seed "$seed" > "$pipeline_log" 2>&1; then
      echo "    pipeline FAILED (see $pipeline_log)"
      tail -5 "$pipeline_log" | sed 's/^/    | /' >&2
      echo "| $case_name | $seed | FAIL | - | - | - | pipeline failed, see \`$pipeline_log\` |" >> "$REPORT"
      continue
    fi

    export_log="${out_dir}/export.log"
    if ! "$EDA_BIN" export "$intent" --design "${out_dir}/design.json" -o "$out_dir" > "$export_log" 2>&1; then
      echo "    export FAILED (see $export_log)"
      tail -5 "$export_log" | sed 's/^/    | /' >&2
      echo "| $case_name | $seed | export-fail | - | - | - | kicad export failed, see \`$export_log\` |" >> "$REPORT"
      continue
    fi

    erc_count="n/a"
    drc_in="n/a"
    drc_other="n/a"
    notes=""

    if [ -n "${KICAD_CLI:-}" ]; then
      sch_path="${out_dir}/${case_name}.kicad_sch"
      pcb_path="${out_dir}/${case_name}.kicad_pcb"

      if [ -f "$sch_path" ]; then
        erc_json="${out_dir}/erc.json"
        if "$KICAD_CLI" sch erc --format json --output "$erc_json" "$sch_path" > "${out_dir}/erc.log" 2>&1; then
          erc_count="$(count_json_array "$erc_json" violations)"
        else
          erc_count="cli-error"
          echo "    kicad-cli sch erc FAILED:" >&2
          tail -5 "${out_dir}/erc.log" | sed 's/^/    | /' >&2
        fi
      else
        erc_count="no-sch"
      fi

      if [ -f "$pcb_path" ]; then
        drc_json="${out_dir}/drc.json"
        "$KICAD_CLI" pcb drc --format json --severity-all --exit-code-violations --output "$drc_json" "$pcb_path" > "${out_dir}/drc.log" 2>&1
        if [ -f "$drc_json" ]; then
          drc_in="$(count_in_scope_drc "$drc_json")"
          drc_other="$(count_other_drc "$drc_json")"
        else
          drc_in="cli-error"
          drc_other="cli-error"
          echo "    kicad-cli pcb drc FAILED to produce a report:" >&2
          tail -5 "${out_dir}/drc.log" | sed 's/^/    | /' >&2
        fi
      else
        drc_in="no-pcb"
        drc_other="no-pcb"
      fi
    fi

    echo "| $case_name | $seed | ok | $erc_count | $drc_in | $drc_other | $notes |" >> "$REPORT"
  done
done

echo
echo "==> wrote $REPORT"
cat "$REPORT"
