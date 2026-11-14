#!/usr/bin/env bash
# Writes src/kicad/color_preview.json: the preview board of Preferences > PCB Editor > Colors (`g_previewBoard`) and the preview sheet of Preferences > Schematic
# Editor > Colors (`createPreviewItems`) as the studio's own server reads them -- the board state (`GET /api/state`), its zone filled by kicad-cli (`POST
# /api/fill/kicad`, `GET /api/fill`), and the sheet (`GET /api/schematic`). components/ColorPreview.tsx paints them in the page's colours (UI-LEFT-3 C3-12).
#
#   KICAD_SRC_DIR=$HOME/src/kicad-src-8303b2ad web/studio/tools/make-color-preview.sh <eda binary> <free port>
set -euo pipefail
EDA="$1"; PORT="$2"
HERE="$(cd "$(dirname "$0")/.." && pwd)"
T="$(mktemp -d -p /tmp color-preview.XXXX)"
trap 'kill "$(cat "$T/pid" 2>/dev/null)" 2>/dev/null || true; rm -rf "$T"' EXIT
node "$HERE/tools/extract-color-preview.mjs" "$T/preview.kicad_pcb" "$T/preview.kicad_sch"
"$EDA" import-kicad "$T/preview.kicad_pcb" -o "$T/board" >/dev/null
"$EDA" import-kicad "$T/preview.kicad_sch" -o "$T/sheet" >/dev/null 2>&1
serve() {
  "$EDA" board serve -C "$1" --port "$PORT" >/dev/null 2>&1 &
  echo $! > "$T/pid"
  for _ in $(seq 1 60); do curl -sf "localhost:$PORT/api/version" >/dev/null && return; sleep 0.5; done
  echo "the server did not start" >&2; exit 1
}
stop() { kill "$(cat "$T/pid")"; wait "$(cat "$T/pid")" 2>/dev/null || true; }
serve "$T/board"
curl -sf -X POST "localhost:$PORT/api/fill/kicad" -H 'content-type: application/json' -d '{}' >/dev/null
curl -sf "localhost:$PORT/api/state" > "$T/state.json"
curl -sf "localhost:$PORT/api/fill" > "$T/fill.json"
stop
serve "$T/sheet"
curl -sf "localhost:$PORT/api/schematic" > "$T/sheet.json"
stop
python3 - "$T" "$HERE/src/kicad/color_preview.json" <<'PY'
import json, sys
t, out = sys.argv[1], sys.argv[2]
board = json.load(open(f"{t}/state.json"))
for k in ("dir", "activity", "job", "checks"):
    board.pop(k, None)
fill = json.load(open(f"{t}/fill.json"))
fill.get("source", {}).pop("revision", None)
fill.get("source", {}).pop("seconds", None)
sheet = json.load(open(f"{t}/sheet.json"))
json.dump({"meta": {"generated": True, "kicadCommit": "8303b2ad", "by": "tools/make-color-preview.sh"}, "board": board, "fill": fill, "sheet": sheet}, open(out, "w"), indent=1, sort_keys=True)
open(out, "a").write("\n")
print(f"wrote {out}")
PY
