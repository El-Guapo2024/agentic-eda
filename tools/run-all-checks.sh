#!/usr/bin/env bash
# Runs the studio's browser checks (web/studio/e2e/*.check.js) headless, each on a fresh scratch copy of the board it needs.
#
#   tools/run-all-checks.sh [-j N] [--ports A-B] [--no-build] [--keep] [name ...]
#
#   name        a check to run (`zones` or `zones.check.js`); none runs every check
#   -j N        how many checks run at once, each with its own server and port (default 2, at most the number of ports)
#   --ports A-B the ports the servers may take (default 8810-8814); a busy one is skipped, and a server that falls back to another
#               port is followed to the port it prints
#   --no-build  use the web/studio/dist and target/debug/eda that are there
#   --keep      keep the scratch directory (boards, logs) when every check passed (it is always kept when one failed)
#
# It builds once (`npm run build`, `cargo build -p eda-cli`), makes each kind of board once, serves a copy of it per check
# (`eda board serve -C <copy> --port P --ui <ui>`), runs `web/studio/e2e/run-check.mjs` against it, kills the server, and prints one
# line per check and a summary. It exits non-zero when any check failed.
#
# Which board a check needs is written in its header, on a line of its own:
#
#   // run-all-checks: board=<kind> [minutes=<m>] [ui=import-foreign|import-files|legacy]
#
# and the kinds are made here (see make_board): mcu30 (examples/mcu_board_30plus.yaml, unplaced), mcu30-nofp (the same with no package on R1..R8 and D1..D8), mcu30-new (the same where `eda board new` puts a new board,
# in the middle of its page; every other kind is made with `--origin 0,0`, the page's corner, which the checks' coordinates were written for), mcu30-few (U1, R1 and R2 placed), mcu30-placed (`eda place`),
# mcu30-routed (`eda place` then `eda route`), mcu30-zones (routed, with a GND pour on F.Cu and B.Cu), mcu30-sheet (mcu30 with KiCad's QA
# issue10926_1_subsheet_1 schematics under fixtures/; $KICAD_SRC_DIR), opamp-routed (`eda pipeline
# examples/opamp_filter.yaml`), kicad10-stitch (KiCad 10's stitch_fp_rulearea.kicad_pcb through replace_board; $KICAD10_SRC_DIR), padstacks-3d (KiCad's padstacks_normal.kicad_pcb with an
# outline, through replace_board; $KICAD_SRC_DIR), usb-pair (diff-pairs.check.js's board), tune-board (length-tuning.check.js's), tuner-arcs (tuner-arcs.check.js's), cutout-board (autoroute-cutout.check.js's), multichannel and
# simulator, sim-model and sim-left (web/studio/e2e/*-board.yaml), board-io (`eda import-kicad web/studio/e2e/board-io.kicad_pcb`), ldo (examples/ldo.yaml, nothing placed), ldo-j1 (ldo where `eda board new` puts it, J1 placed). A check with no such line runs on mcu30 for 10 minutes. `ui=legacy` serves a copy with KiCad's QA legacy projects in `e2e-legacy/` (legacy-import.check.js). `ui=import-foreign` serves a
# copy of the UI with the QA files of KiCad's other formats in `e2e-import-foreign/` (needs $KICAD_SRC_DIR). `kicad_config=fresh` serves the check with a KiCad
# configuration folder of its own, empty (`EDA_KICAD_CONFIG_DIR`): a check that writes the global library tables or KiCad's path variables never touches the
# machine's. `fixtures=library-formats` puts two of KiCad's QA libraries in other formats in the board's folder, `legacy.lib` (a legacy symbol library) and
# `gps.lbr` (an Eagle library), for Migrate Libraries and the libraries kicad-cli converts (needs $KICAD_SRC_DIR); `fixtures=broken-symdir` a folder symbol library
# `Broken.kicad_symdir` with a good file and a cut-off one (a library KiCad 10 loads with errors); `fixtures=cvpcb` e2e/cvpcb.equ as `cvpcb-check.equ` (a footprint
# association file for cvpcb.check.js). Several are joined by commas. `fixtures=cvpcb` puts
# web/studio/e2e/cvpcb.equ (a footprint association file) in the board's folder as `cvpcb-check.equ`, for cvpcb.check.js.
#
# Needs: Playwright with Chromium ($PLAYWRIGHT_MODULE, default ~/tools/pw/node_modules/playwright/index.mjs), kicad-cli on the PATH
# (or $EDA_KICAD_CLI), ngspice for the simulator check.
set -uo pipefail
cd "$(dirname "$0")/.."
repo="$PWD"

jobs=2
ports="8810-8814"
build=1
keep=0
names=()
while [ $# -gt 0 ]; do
    case "$1" in
        -j) jobs="$2"; shift 2 ;;
        --ports) ports="$2"; shift 2 ;;
        --no-build) build=0; shift ;;
        --keep) keep=1; shift ;;
        -h|--help) sed -n '2,30p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
        -*) echo "unknown option $1" >&2; exit 2 ;;
        *) names+=("${1%.check.js}"); shift ;;
    esac
done
port_lo="${ports%-*}"
port_hi="${ports#*-}"
export PLAYWRIGHT_MODULE="${PLAYWRIGHT_MODULE:-$HOME/tools/pw/node_modules/playwright/index.mjs}"
export CARGO_BUILD_JOBS="${CARGO_BUILD_JOBS:-4}" CARGO_INCREMENTAL="${CARGO_INCREMENTAL:-0}"
eda="$repo/target/debug/eda"
e2e="$repo/web/studio/e2e"

scratch="$(mktemp -d -p /tmp run-all-checks.XXXX)"
echo "run-all-checks: scratch $scratch"

if [ "$build" = 1 ]; then
    echo "run-all-checks: building the studio and eda-cli"
    (cd web/studio && npm run build) > "$scratch/build-web.log" 2>&1 || { echo "npm run build failed: $scratch/build-web.log"; exit 1; }
    cargo build -p eda-cli > "$scratch/build-cli.log" 2>&1 || { echo "cargo build -p eda-cli failed: $scratch/build-cli.log"; exit 1; }
fi
[ -x "$eda" ] && [ -f web/studio/dist/index.html ] || { echo "no $eda or web/studio/dist: run without --no-build"; exit 1; }

# ------------------------------------------------------------------------------------------------------------------ the checks to run
checks=()
if [ ${#names[@]} -eq 0 ]; then
    for f in "$e2e"/*.check.js; do checks+=("$(basename "$f" .check.js)"); done
else
    for n in "${names[@]}"; do
        [ -f "$e2e/$n.check.js" ] || { echo "no check $e2e/$n.check.js"; exit 2; }
        checks+=("$n")
    done
fi
# The header line of a check: its board, its minutes, its ui.
meta() { # meta <check> <key> <default>
    local line v
    line="$(grep -m1 -E '^// run-all-checks:' "$e2e/$1.check.js" || true)"
    v="$(sed -nE "s/.*[[:space:]]$2=([^[:space:]]+).*/\1/p" <<< "$line")"
    echo "${v:-$3}"
}

# ------------------------------------------------------------------------------------------------------------------ the boards
# Each kind is made once, under $scratch/boards/<kind> (the design, the board, its files), and copied for every check that needs it.
yaml_mcu="$repo/examples/mcu_board_30plus.yaml"
make_board() { # make_board <kind> <dir>; the log goes to <dir>.log
    local kind="$1" d="$2" t="$scratch/work-$1"
    mkdir -p "$t"
    case "$kind" in
        mcu30)
            "$eda" board new "$yaml_mcu" -o "$d" --origin 0,0 ;;
        mcu30-nofp) # mcu30 whose LED channels (R1..R8, D1..D8) have no package, so no footprint: the symbols CvPcb assigns to (cvpcb.check.js). A part with a
                    # package has the footprint it stands for in the derived .kicad_sch, and CvPcb, like KiCad's AutomaticFootprintMatching, leaves it alone
            awk '/^  - reference: /{ led = ($3 ~ /^[RD][1-8]$/) } !(led && $1 == "package:")' "$yaml_mcu" > "$t/mcu_nofp.yaml" \
                && "$eda" board new "$t/mcu_nofp.yaml" -o "$d" --origin 0,0 ;;
        mcu30-new) # a new board where `eda board new` puts it, in the middle of its page (UI-LEFT-2 CC4; new-board-page.check.js)
            "$eda" board new "$yaml_mcu" -o "$d" ;;
        mcu30-few) # three footprints placed, the rest not (board-setup-pages.check.js: `eda board place U1 --at 17.5,17.5` and a few more)
            "$eda" board new "$yaml_mcu" -o "$d" --origin 0,0 && "$eda" board place U1 --at 17.5,17.5 -C "$d" \
                && "$eda" board place R1 --at 8,8 -C "$d" && "$eda" board place R2 --at 8,12 -C "$d" ;;
        mcu30-placed)
            "$eda" place "$yaml_mcu" -o "$t/placed" && "$eda" board new "$yaml_mcu" -o "$d" --origin 0,0 && cp "$t/placed/design.json" "$d/" ;;
        mcu30-routed)
            "$eda" place "$yaml_mcu" -o "$t/placed" && "$eda" route "$yaml_mcu" --design "$t/placed/design.json" -o "$t/routed" \
                && "$eda" board new "$yaml_mcu" -o "$d" --origin 0,0 && cp "$t/routed/design.json" "$d/" ;;
        mcu30-zones)
            ensure_board mcu30-routed && cp -r "$scratch/boards/mcu30-routed" "$d" \
                && "$eda" board zone add --net GND --layer F.Cu --pts "2.9,5 28.3,5 28.3,29.2 2.9,29.2" -C "$d" \
                && "$eda" board zone add --net GND --layer B.Cu --pts "2.9,5 28.3,5 28.3,29.2 2.9,29.2" -C "$d" ;;
        mcu30-sheet) # import-sheet.check.js and hierarchy-navigator.check.js: KiCad's QA issue10926_1_subsheet_1 schematics under fixtures/
            local q="${KICAD_SRC_DIR:-$HOME/src/kicad-src-8303b2ad}/qa/data/eeschema"
            [ -f "$q/issue10926_1_subsheet_1.kicad_sch" ] && [ -f "$q/issue10926_1_subsheet_1_1.kicad_sch" ] \
                || { echo "no $q/issue10926_1_subsheet_1*.kicad_sch (set KICAD_SRC_DIR to a KiCad source)"; return 1; }
            "$eda" board new "$yaml_mcu" -o "$d" --origin 0,0 && mkdir -p "$d/fixtures" \
                && cp "$q/issue10926_1_subsheet_1.kicad_sch" "$q/issue10926_1_subsheet_1_1.kicad_sch" "$d/fixtures/" ;;
        opamp-routed)
            "$eda" pipeline "$repo/examples/opamp_filter.yaml" -o "$t/p" && "$eda" board new "$repo/examples/opamp_filter.yaml" -o "$d" --origin 0,0 \
                && cp "$t/p/design.json" "$d/" ;;
        usb-pair) # diff-pairs.check.js
            sed -e 's/SDA/USB_DP/' -e 's/SCL/USB_DN/' "$repo/examples/through_hole_headers.yaml" > "$t/usb_pair.yaml" \
                && "$eda" board new "$t/usb_pair.yaml" -o "$d" --origin 0,0 \
                && "$eda" board place J3 --at 11.5,3 -C "$d" && "$eda" board place J2 --at 6,20 -C "$d" && "$eda" board place J1 --at 7.5,11 -C "$d" \
                && "$eda" board place R1 --at 3,16 -C "$d" && "$eda" board place R2 --at 17,18 -C "$d" \
                && "$eda" board track add --net GND --layer F.Cu --width 0.2 --pts "4.4,7 4.4,12" -C "$d" ;;
        cutout-board) # autoroute-cutout.check.js: through_hole_headers placed so SDA and SCL run across a window cut out of the board
            "$eda" board new "$repo/examples/through_hole_headers.yaml" -o "$d" --origin 0,0 \
                && "$eda" board place J3 --at 11.5,3 -C "$d" && "$eda" board place J2 --at 7,20 -C "$d" && "$eda" board place J1 --at 17,20 -C "$d" \
                && "$eda" board place R1 --at 20,8 -C "$d" && "$eda" board place R2 --at 20,14 -C "$d" \
                && "$eda" board shape add --kind rect --layer Edge.Cuts --pts "4,8 11,15" --width 0.05 -C "$d" ;;
        tuner-arcs) # tuner-arcs.check.js: tune-board's nets on an empty board, with the pair of concentric arcs tuner-arcs.board.mjs adds
            perl -pe 's/\bPA2\b/USB_DP/g; s/\bPA3\b/USB_DN/g' "$yaml_mcu" > "$t/tuner_arcs.yaml" \
                && "$eda" board new "$t/tuner_arcs.yaml" -o "$d" --origin 0,0 && node "$e2e/tuner-arcs.board.mjs" "$d" ;;
        tune-board) # length-tuning.check.js (the nets under KiCad's names: the hierarchical labels of the LED sheet)
            perl -pe 's/\bPA2\b/USB_DP/g; s/\bPA3\b/USB_DN/g' "$yaml_mcu" > "$t/tune_board.yaml" \
                && "$eda" board new "$t/tune_board.yaml" -o "$d" --origin 0,0 \
                && "$eda" board track add --net "/LED channels (D1–D8)/PA0" --layer F.Cu --width 0.2 --pts "4,5 30,5" -C "$d" \
                && "$eda" board track add --net "/LED channels (D1–D8)/PA1" --layer F.Cu --width 0.2 --pts "4,8 30,8" -C "$d" \
                && "$eda" board track add --net "/LED channels (D1–D8)/USB_DP" --layer F.Cu --width 0.2 --pts "4,20 30,20" -C "$d" \
                && "$eda" board track add --net "/LED channels (D1–D8)/USB_DN" --layer F.Cu --width 0.2 --pts "4,20.5 30,20.5" -C "$d" ;;
        kicad10-stitch) # kicad10-items.check.js: KiCad 25ec2335's qa/data/pcbnew/stitch_fp_rulearea.kicad_pcb brought in with `replace_board` (which keeps its zone fills)
            local f="${KICAD10_SRC_DIR:-$HOME/src/kicad-src-25ec2335}/qa/data/pcbnew/stitch_fp_rulearea.kicad_pcb" sp
            [ -f "$f" ] || { echo "no $f (set KICAD10_SRC_DIR to a KiCad 10 source)"; return 1; }
            "$eda" board new "$yaml_mcu" -o "$d" --origin 0,0 || return 1
            # the board is brought in through the studio's own verb: a server on this worker's port (free until its check is served), one POST, stopped
            "$eda" board serve -C "$d" --port "$port" --ui "$repo/web/studio/dist" > "$t/serve.log" 2>&1 &
            sp=$!
            local url=""
            for _ in $(seq 1 100); do url="$(grep -oE 'http://[0-9.]+:[0-9]+/' "$t/serve.log" | head -1)"; [ -n "$url" ] && break; sleep 0.2; done
            python3 -c 'import json,sys; print(json.dumps({"cmd": {"op": "replace_board", "text": open(sys.argv[1]).read()}, "strict": False}))' "$f" > "$t/replace.json"
            local reply
            reply="$(curl -s -H 'Content-Type: application/json' --data-binary @"$t/replace.json" "${url}api/cmd")"
            kill "$sp" 2>/dev/null; wait "$sp" 2>/dev/null
            echo "replace_board: $reply" | cut -c1-300
            grep -q '"ok":true' <<< "$reply" ;;
        padstacks-3d) # viewer3d-controls.check.js: KiCad 8303b2ad's qa/data/pcbnew/padstacks_normal.kicad_pcb (trapezoid, chamfered and rounded pads, non-plated holes) with an
            # Edge.Cuts rectangle round its two footprints, brought in with `replace_board`
            local f="${KICAD_SRC_DIR:-$HOME/src/kicad-src-8303b2ad}/qa/data/pcbnew/padstacks_normal.kicad_pcb" sp
            [ -f "$f" ] || { echo "no $f (set KICAD_SRC_DIR to a KiCad source)"; return 1; }
            "$eda" board new "$yaml_mcu" -o "$d" --origin 0,0 || return 1
            "$eda" board serve -C "$d" --port "$port" --ui "$repo/web/studio/dist" > "$t/serve.log" 2>&1 &
            sp=$!
            local url=""
            for _ in $(seq 1 100); do url="$(grep -oE 'http://[0-9.]+:[0-9]+/' "$t/serve.log" | head -1)"; [ -n "$url" ] && break; sleep 0.2; done
            python3 -c 'import json,sys
t=open(sys.argv[1]).read().rstrip()
edge="\t(gr_rect\n\t\t(start 128 68)\n\t\t(end 178 108)\n\t\t(stroke\n\t\t\t(width 0.05)\n\t\t\t(type default)\n\t\t)\n\t\t(fill no)\n\t\t(layer \"Edge.Cuts\")\n\t\t(uuid \"aaaaaaaa-1111-2222-3333-444444444444\")\n\t)\n"
print(json.dumps({"cmd": {"op": "replace_board", "text": t[:-1] + edge + ")\n"}, "strict": False}))' "$f" > "$t/replace.json"
            local reply
            reply="$(curl -s -H 'Content-Type: application/json' --data-binary @"$t/replace.json" "${url}api/cmd")"
            kill "$sp" 2>/dev/null; wait "$sp" 2>/dev/null
            echo "replace_board: $reply" | cut -c1-300
            grep -q '"ok":true' <<< "$reply" ;;
        ldo) # lines-dims.check.js: a 26 mm square board with nothing placed, room to draw in
            "$eda" board new "$repo/examples/ldo.yaml" -o "$d" --origin 0,0 ;;
        ldo-j1) # viewer3d-prefs.check.js: examples/ldo.yaml where `eda board new` puts it (the middle of its page), J1 placed at 145, 105 mm (`place_at`)
            local sp url="" reply
            "$eda" board new "$repo/examples/ldo.yaml" -o "$d" || return 1
            "$eda" board serve -C "$d" --port "$port" --ui "$repo/web/studio/dist" > "$t/serve.log" 2>&1 &
            sp=$!
            for _ in $(seq 1 100); do url="$(grep -oE 'http://[0-9.]+:[0-9]+/' "$t/serve.log" | head -1)"; [ -n "$url" ] && break; sleep 0.2; done
            reply="$(curl -s -H 'Content-Type: application/json' -d '{"cmd":{"op":"place_at","part":"J1","x":145000000,"y":105000000},"strict":false}' "${url}api/cmd")"
            kill "$sp" 2>/dev/null; wait "$sp" 2>/dev/null
            echo "place_at J1: $reply" | cut -c1-300
            grep -q '"ok":true' <<< "$reply" ;;
        board-io) # board-io.check.js: a hand-written KiCad board read by the importer
            "$eda" import-kicad "$e2e/board-io.kicad_pcb" -o "$d" ;;
        multichannel)
            "$eda" board new "$e2e/multichannel-board.yaml" -o "$d" --origin 0,0 ;;
        simulator)
            "$eda" board new "$e2e/simulator-board.yaml" -o "$d" --origin 0,0 ;;
        sim-left) # sim-left-u5.check.js (UI-LEFT M3): a source with an S-parameter port, a potentiometer, a tuned R
            "$eda" board new "$e2e/sim-left-board.yaml" -o "$d" --origin 0,0 ;;
        sim-model) # sim-model.check.js: the built-in models, and an IBIS file in the board folder
            "$eda" board new "$e2e/sim-model-board.yaml" -o "$d" --origin 0,0 && mkdir -p "$d/models" && cp "$e2e/sim-model-io.ibs" "$d/models/io.ibs" && cp "$e2e/sim-model-diode.lib" "$d/models/diode.lib" ;;
        sim-v10) # sim-v10.check.js (UI-LEFT-2 MM1-MM4): inductors for a K, a library diode, a sheet pin to probe; runs ngspice
            "$eda" board new "$e2e/sim-v10-board.yaml" -o "$d" --origin 0,0 && mkdir -p "$d/models" && cp "$e2e/sim-model-diode.lib" "$d/models/diode.lib" ;;
        *) echo "unknown board kind $kind"; return 1 ;;
    esac
}
ensure_board() { # ensure_board <kind>: makes it once (under a lock, the workers share it)
    local kind="$1" d="$scratch/boards/$1"
    mkdir -p "$scratch/boards"
    (
        flock 9
        [ -f "$d.ok" ] && exit 0
        [ -f "$d.failed" ] && exit 1
        rm -rf "$d"
        # The steps that made it (a design.json copied in, the setup commands) are not the check's to undo: the board starts with no history, as a board opened
        # for the first time does (`sync_external_edits` takes the design as it finds it).
        if make_board "$kind" "$d" > "$d.log" 2>&1 && rm -rf "$d/.history" "$d/activity.jsonl"; then touch "$d.ok"; else touch "$d.failed"; exit 1; fi
    ) 9> "$scratch/boards/$kind.lock"
}
ensure_ui() { # ensure_ui <kind>: the directory to serve as --ui
    local kind="$1" u="$scratch/ui-$1"
    case "$kind" in
        default) echo "$repo/web/studio/dist"; return 0 ;;
        import-foreign)
            (
                flock 9
                [ -d "$u" ] && exit 0
                local q="${KICAD_SRC_DIR:?KICAD_SRC_DIR is needed for ui=import-foreign}/qa/data/pcbnew/plugins"
                cp -r "$repo/web/studio/dist" "$u.tmp" && mkdir -p "$u.tmp/e2e-import-foreign" \
                    && cp "$q/eagle/test_eagle_23016/test_eagle.brd" "$u.tmp/e2e-import-foreign/test_eagle.brd" \
                    && cp "$q/eagle/Adafruit-AHT20-PCB/Adafruit AHT20 Temperature & Humidity.brd" "$u.tmp/e2e-import-foreign/adafruit_aht20.brd" \
                    && cp "$q/cadstar/route_offset/minimal_route_offset_curved_track.cpa" "$u.tmp/e2e-import-foreign/" \
                    && cp "$q/altium/eDP_adapter_dvt1_source/power.SchDoc" "$u.tmp/e2e-import-foreign/" \
                    && cp "$e2e/import-foreign.check.js" "$u.tmp/e2e-import-foreign/" && mv "$u.tmp" "$u"
            ) 9> "$scratch/ui-$kind.lock" && echo "$u" ;;
        import-files)
            # import-files.check.js (task-u4-import): KiCad's QA Eagle project as aht20.brd / aht20.sch under e2e-import-files/.
            (
                flock 9
                [ -d "$u" ] && exit 0
                local q="${KICAD_SRC_DIR:?KICAD_SRC_DIR is needed for ui=import-files}/qa/data/pcbnew/plugins/eagle/Adafruit-AHT20-PCB"
                cp -r "$repo/web/studio/dist" "$u.tmp" && mkdir -p "$u.tmp/e2e-import-files" \
                    && cp "$q/Adafruit AHT20 Temperature & Humidity.brd" "$u.tmp/e2e-import-files/aht20.brd" \
                    && cp "$q/Adafruit AHT20 Temperature & Humidity.sch" "$u.tmp/e2e-import-files/aht20.sch" && mv "$u.tmp" "$u"
            ) 9> "$scratch/ui-$kind.lock" && echo "$u" ;;
        legacy)
            # legacy-import.check.js (task-v10-legacy): KiCad's QA legacy projects (`.sch`, cache libraries, `.lib`s, `.pro`) under e2e-legacy/<project>/.
            (
                flock 9
                [ -d "$u" ] && exit 0
                local q="${KICAD_SRC_DIR:?KICAD_SRC_DIR is needed for ui=legacy}/qa/data"
                local n="$q/eeschema/netlists" l="$u.tmp/e2e-legacy"
                cp -r "$repo/web/studio/dist" "$u.tmp" && mkdir -p "$l/complex_hierarchy" "$l/legacy_power4" "$l/pic_programmer" \
                    && cp "$n/complex_hierarchy/"{complex_hierarchy.sch,ampli_ht.sch,complex_hierarchy-cache.lib,complex_hierarchy_schlib.lib,complex_hierarchy.pro,sym-lib-table} "$l/complex_hierarchy/" \
                    && cp "$n/legacy_power4/"{legacy_power4.sch,legacy_power4-cache.lib,legacy_power4-cache.dcm,symbols.lib,symbols.dcm,legacy_power4.pro} "$l/legacy_power4/" \
                    && cp "$q/pcbnew/plugins/legacy_demos/pic_programmer/"{pic_programmer.sch,pic_sockets.sch,pic_programmer.cache.lib,pic_programmer.pro} "$l/pic_programmer/" \
                    && mv "$u.tmp" "$u"
            ) 9> "$scratch/ui-$kind.lock" && echo "$u" ;;
        *) echo "unknown ui $kind" >&2; return 1 ;;
    esac
}

# ------------------------------------------------------------------------------------------------------------------ one check
port_free() { ! ss -ltn "( sport = :$1 )" 2>/dev/null | grep -q ":$1 "; }
run_one() { # run_one <check> <port>: prints the check's line; its logs are in $scratch/runs/<check>
    local c="$1" port="$2" r="$scratch/runs/$1" kind minutes ui uidir pid url t0 rc line
    kind="$(meta "$c" board mcu30)"; minutes="$(meta "$c" minutes 10)"; ui="$(meta "$c" ui default)"
    mkdir -p "$r"
    t0=$SECONDS
    if ! ensure_board "$kind"; then printf 'FAIL %-22s board %s could not be made: %s\n' "$c" "$kind" "$scratch/boards/$kind.log"; return 1; fi
    if ! uidir="$(ensure_ui "$ui")"; then printf 'FAIL %-22s ui %s could not be made\n' "$c" "$ui"; return 1; fi
    cp -r "$scratch/boards/$kind" "$r/board"
    local served=()
    if [ "$(meta "$c" kicad_config default)" = fresh ]; then mkdir -p "$r/kicad-config"; served+=("EDA_KICAD_CONFIG_DIR=$r/kicad-config" "EDA_LIBRARY_CACHE=$r/library-cache"); fi
    local fx
    for fx in $(meta "$c" fixtures none | tr ',' ' '); do
        case "$fx" in
            library-formats)
                local q="${KICAD_SRC_DIR:-$HOME/src/kicad-src-8303b2ad}/qa/data"
                cp "$q/eeschema/netlists/legacy_power4/symbols.lib" "$r/board/legacy.lib" && cp "$q/pcbnew/plugins/eagle/lbr/SparkFun-GPS.lbr" "$r/board/gps.lbr" \
                    || { printf 'FAIL %-22s the library fixtures are not in %s (set KICAD_SRC_DIR)\n' "$c" "$q"; return 1; } ;;
            broken-symdir)
                # a folder symbol library with one good file and one cut off: KiCad 10 loads it "with errors"
                mkdir -p "$r/board/Broken.kicad_symdir"
                printf '(kicad_symbol_lib (version 20241209) (generator "eda")\n  (symbol "GoodPart" (property "Reference" "U" (at 0 0 0)) (property "Value" "GoodPart" (at 0 0 0)))\n)\n' > "$r/board/Broken.kicad_symdir/GoodPart.kicad_sym"
                printf '(kicad_symbol_lib (version 20241209) (generator "eda")\n  (symbol "BadPart" (property "Reference" "U" (at 0 0 0)\n' > "$r/board/Broken.kicad_symdir/BadPart.kicad_sym" ;;
            cvpcb)
                # a footprint association file for cvpcb.check.js
                cp "$e2e/cvpcb.equ" "$r/board/cvpcb-check.equ" || { printf 'FAIL %-22s %s is missing\n' "$c" "$e2e/cvpcb.equ"; return 1; } ;;
            none) ;;
        esac
    done
    if [ "$(meta "$c" fixtures none)" = cvpcb ]; then
        cp "$e2e/cvpcb.equ" "$r/board/cvpcb-check.equ" || { printf 'FAIL %-22s %s is missing\n' "$c" "$e2e/cvpcb.equ"; return 1; }
    fi
    env "${served[@]}" "$eda" board serve -C "$r/board" --port "$port" --ui "$uidir" > "$r/serve.log" 2>&1 &
    pid=$!
    url=""
    for _ in $(seq 1 100); do
        url="$(grep -oE 'http://[0-9.]+:[0-9]+/' "$r/serve.log" | head -1)"
        [ -n "$url" ] && break
        kill -0 "$pid" 2>/dev/null || break
        sleep 0.2
    done
    if [ -z "$url" ]; then
        kill "$pid" 2>/dev/null; wait "$pid" 2>/dev/null
        printf 'FAIL %-22s the server did not start: %s\n' "$c" "$r/serve.log"; return 1
    fi
    RUN_CHECK_SHOT="$r/start.png" timeout $(( minutes * 60 + 180 )) node "$e2e/run-check.mjs" "$url" "$e2e/$c.check.js" "$minutes" > "$r/check.log" 2>&1
    rc=$?
    kill "$pid" 2>/dev/null; wait "$pid" 2>/dev/null
    line="$(grep -E ': done=(true|false) ok=' "$r/check.log" | tail -1)"
    local n failed
    n="$(sed -nE 's/.* scenarios=([0-9]+).*/\1/p' <<< "$line")"
    failed="$(sed -nE 's/.* failed=(.*)$/\1/p' <<< "$line")"
    if [ "$rc" = 0 ]; then
        printf 'ok   %-22s %3s scenarios  %4ss  board=%s\n' "$c" "${n:-?}" "$(( SECONDS - t0 ))" "$kind"
    else
        printf 'FAIL %-22s %3s scenarios  %4ss  board=%s  failed=%s  log=%s\n' "$c" "${n:-?}" "$(( SECONDS - t0 ))" "$kind" "${failed:-?}" "$r/check.log"
    fi
    return "$rc"
}

# ------------------------------------------------------------------------------------------------------------------ the workers
# Each worker owns one free port and takes the next check from the queue.
free_ports=()
for p in $(seq "$port_lo" "$port_hi"); do port_free "$p" && free_ports+=("$p"); done
[ ${#free_ports[@]} -gt 0 ] || { echo "no free port in $ports"; exit 1; }
[ "$jobs" -gt ${#free_ports[@]} ] && jobs=${#free_ports[@]}
printf '%s\n' "${checks[@]}" > "$scratch/queue"
echo 0 > "$scratch/next"
mkdir -p "$scratch/runs"
worker() {
    local port="$1" i c
    while :; do
        i="$( (flock 9; i=$(cat "$scratch/next"); echo $(( i + 1 )) > "$scratch/next"; echo "$i") 9> "$scratch/queue.lock")"
        c="$(sed -n "$(( i + 1 ))p" "$scratch/queue")"
        [ -n "$c" ] || break
        run_one "$c" "$port" | tee -a "$scratch/results"
    done
}
echo "run-all-checks: ${#checks[@]} check(s), $jobs at a time on port(s) ${free_ports[*]:0:$jobs}"
start=$SECONDS
for k in $(seq 0 $(( jobs - 1 ))); do worker "${free_ports[$k]}" & done
wait

total=${#checks[@]}
passed=$(grep -c '^ok ' "$scratch/results" 2>/dev/null || true)
failedn=$(( total - passed ))
echo
echo "run-all-checks: $passed passed, $failedn failed of $total in $(( SECONDS - start ))s (logs: $scratch/runs)"
grep '^FAIL' "$scratch/results" 2>/dev/null | sed 's/^/  /'
# the libraries kicad-cli converted are kept read only (`library_convert`): writable again first, or rm cannot empty their folders
if [ "$failedn" -eq 0 ] && [ "$keep" = 0 ]; then chmod -R u+w "$scratch" 2>/dev/null; rm -rf "$scratch"; fi
[ "$failedn" -eq 0 ]
