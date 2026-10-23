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
#   // run-all-checks: board=<kind> [minutes=<m>] [ui=import-foreign]
#
# and the kinds are made here (see make_board): mcu30 (examples/mcu_board_30plus.yaml, unplaced), mcu30-few (U1, R1 and R2 placed), mcu30-placed (`eda place`),
# mcu30-routed (`eda place` then `eda route`), mcu30-zones (routed, with a GND pour on F.Cu and B.Cu), opamp-routed (`eda pipeline
# examples/opamp_filter.yaml`), usb-pair (diff-pairs.check.js's board), tune-board (length-tuning.check.js's), multichannel and
# simulator (web/studio/e2e/*-board.yaml). A check with no such line runs on mcu30 for 10 minutes. `ui=import-foreign` serves a
# copy of the UI with the QA files of KiCad's other formats in `e2e-import-foreign/` (needs $KICAD_SRC_DIR).
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
            "$eda" board new "$yaml_mcu" -o "$d" ;;
        mcu30-few) # three footprints placed, the rest not (board-setup-pages.check.js: `eda board place U1 --at 17.5,17.5` and a few more)
            "$eda" board new "$yaml_mcu" -o "$d" && "$eda" board place U1 --at 17.5,17.5 -C "$d" \
                && "$eda" board place R1 --at 8,8 -C "$d" && "$eda" board place R2 --at 8,12 -C "$d" ;;
        mcu30-placed)
            "$eda" place "$yaml_mcu" -o "$t/placed" && "$eda" board new "$yaml_mcu" -o "$d" && cp "$t/placed/design.json" "$d/" ;;
        mcu30-routed)
            "$eda" place "$yaml_mcu" -o "$t/placed" && "$eda" route "$yaml_mcu" --design "$t/placed/design.json" -o "$t/routed" \
                && "$eda" board new "$yaml_mcu" -o "$d" && cp "$t/routed/design.json" "$d/" ;;
        mcu30-zones)
            ensure_board mcu30-routed && cp -r "$scratch/boards/mcu30-routed" "$d" \
                && "$eda" board zone add --net GND --layer F.Cu --pts "2.9,5 28.3,5 28.3,29.2 2.9,29.2" -C "$d" \
                && "$eda" board zone add --net GND --layer B.Cu --pts "2.9,5 28.3,5 28.3,29.2 2.9,29.2" -C "$d" ;;
        opamp-routed)
            "$eda" pipeline "$repo/examples/opamp_filter.yaml" -o "$t/p" && "$eda" board new "$repo/examples/opamp_filter.yaml" -o "$d" \
                && cp "$t/p/design.json" "$d/" ;;
        usb-pair) # diff-pairs.check.js
            sed -e 's/SDA/USB_DP/' -e 's/SCL/USB_DN/' "$repo/examples/through_hole_headers.yaml" > "$t/usb_pair.yaml" \
                && "$eda" board new "$t/usb_pair.yaml" -o "$d" \
                && "$eda" board place J3 --at 11.5,3 -C "$d" && "$eda" board place J2 --at 6,20 -C "$d" && "$eda" board place J1 --at 7.5,11 -C "$d" \
                && "$eda" board place R1 --at 3,16 -C "$d" && "$eda" board place R2 --at 17,18 -C "$d" \
                && "$eda" board track add --net GND --layer F.Cu --width 0.2 --pts "4.4,7 4.4,12" -C "$d" ;;
        tune-board) # length-tuning.check.js
            perl -pe 's/\bPA2\b/USB_DP/g; s/\bPA3\b/USB_DN/g' "$yaml_mcu" > "$t/tune_board.yaml" \
                && "$eda" board new "$t/tune_board.yaml" -o "$d" \
                && "$eda" board track add --net PA0 --layer F.Cu --width 0.2 --pts "4,5 30,5" -C "$d" \
                && "$eda" board track add --net PA1 --layer F.Cu --width 0.2 --pts "4,8 30,8" -C "$d" \
                && "$eda" board track add --net USB_DP --layer F.Cu --width 0.2 --pts "4,20 30,20" -C "$d" \
                && "$eda" board track add --net USB_DN --layer F.Cu --width 0.2 --pts "4,20.5 30,20.5" -C "$d" ;;
        multichannel)
            "$eda" board new "$e2e/multichannel-board.yaml" -o "$d" ;;
        simulator)
            "$eda" board new "$e2e/simulator-board.yaml" -o "$d" ;;
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
    "$eda" board serve -C "$r/board" --port "$port" --ui "$uidir" > "$r/serve.log" 2>&1 &
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
if [ "$failedn" -eq 0 ] && [ "$keep" = 0 ]; then rm -rf "$scratch"; fi
[ "$failedn" -eq 0 ]
