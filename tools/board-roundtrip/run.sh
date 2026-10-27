#!/usr/bin/env bash
# Round trip of KiCad's QA boards through our reader and writer, judged by kicad-cli's own rewrite (the method of the round-2 review r2-board-io):
# for each board, `eda import-kicad` + `eda board export --kicad pos`, then `kicad-cli pcb upgrade --force` of the original and of ours, and a structural diff of
# the two (`norm.py`: uuids, tstamps and zone fills dropped, one line per footprint and zone child). The original's rewrite is cached under <out>/orig.
#
#   tools/board-roundtrip/run.sh <eda binary> <out dir> [jobs]
#   python3 tools/board-roundtrip/cmp.py <before>/summary.txt <after>/summary.txt
#
# Boards: every .kicad_pcb of $KICAD_SRC_DIR/qa/data/pcbnew (185 on 8303b2ad), and the top-level ones of $KICAD10_SRC_DIR (default ~/src/kicad-src-25ec2335) that the first
# has not (29): 214.
# <out>/<board>/diff.txt is the diff; <out>/summary.txt has one line per board: OK <board> <lines that differ>, or IMPORTFAIL / EXPORTFAIL.
set -uo pipefail
EDA="$(realpath "$1")"; OUT="$(realpath -m "$2")"; JOBS="${3:-3}"
HERE="$(cd "$(dirname "$0")" && pwd)"
K="${KICAD_SRC_DIR:?set KICAD_SRC_DIR}"; K10="${KICAD10_SRC_DIR:-$HOME/src/kicad-src-25ec2335}"
mkdir -p "$OUT"
{
  find "$K/qa/data/pcbnew" -name '*.kicad_pcb' -not -name '._*' | sort
  if [ -d "$K10/qa/data/pcbnew" ]; then
    for b in "$K10"/qa/data/pcbnew/*.kicad_pcb; do [ -e "$K/qa/data/pcbnew/$(basename "$b")" ] || echo "$b"; done
  fi
} > "$OUT/boards.txt"
one() {
  b="$1"; n=$(basename "$b" .kicad_pcb); k=$(basename "$(dirname "$b")")__$n; W="$OUT/$k"; rm -rf "$W"; mkdir -p "$W/src"
  cp "$(dirname "$b")/$n".kicad_p* "$W/src/" 2>/dev/null; cp "$(dirname "$b")/$n".kicad_dru "$W/src/" 2>/dev/null
  O="$OUT/orig/$k"
  if [ ! -f "$O/o.n" ]; then mkdir -p "$O"; cp "$W/src/$n.kicad_pcb" "$O/orig.kicad_pcb"; kicad-cli pcb upgrade --force "$O/orig.kicad_pcb" >/dev/null 2>&1; python3 -I "$HERE/norm.py" "$O/orig.kicad_pcb" > "$O/o.n"; fi
  (cd "$W" && "$EDA" import-kicad "$W/src/$n.kicad_pcb" -o imp > import.log 2>&1) || { echo "IMPORTFAIL $k"; return; }
  (cd "$W/imp" && "$EDA" board export -C . --kicad pos > ../export.log 2>&1)
  [ -f "$W/imp/.kicad/intent.kicad_pcb" ] || { echo "EXPORTFAIL $k"; return; }
  cp "$W/imp/.kicad/intent.kicad_pcb" "$W/ours.kicad_pcb"
  kicad-cli pcb upgrade --force "$W/ours.kicad_pcb" >/dev/null 2>&1
  python3 -I "$HERE/norm.py" "$W/ours.kicad_pcb" > "$W/u.n"
  diff "$O/o.n" "$W/u.n" > "$W/diff.txt"
  echo "OK $k $(grep -c '^[<>]' "$W/diff.txt")"
}
export -f one; export EDA OUT HERE
xargs -P "$JOBS" -I{} bash -c 'one "$@"' _ {} < "$OUT/boards.txt" | sort -k2 > "$OUT/summary.txt"
awk '{s+=$3; if($3>0)nz++} END{print "boards",NR,"difflines",s,"nonzero",nz}' "$OUT/summary.txt"
