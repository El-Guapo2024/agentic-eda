#!/usr/bin/env bash
# The repo's two test tiers.
#
#   tools/check.sh fast [-p crate ...]
#       While working. Debug build; the slow tier is skipped (tests guarded by
#       `EDA_SLOW_TESTS`: whole-corpus placement, kicad-cli round trips, the
#       freerouting replays), so `cargo test -p eda-cli` no longer spends
#       many debug-mode minutes placing every example. Web typecheck and unit
#       tests run alongside. With -p, only those crates' Rust tests run.
#
#   tools/check.sh full
#       Before a merge lands on main. Release build and every test, the slow
#       tier included, with an incremental release profile so a merge that
#       touches a few crates does not rebuild the world. The web typecheck,
#       unit tests and build run in parallel with the Rust side.
#
# Both print one summary line per side and exit non-zero when anything fails.
# The two wall-clock budget tests (random30_layout_is_fast,
# fixture_4p6n_routes_under_1s_release) run in neither tier: set
# EDA_TIMING_TESTS=1 and run them on an idle machine.
# Respects CARGO_BUILD_JOBS (agents share the machine; use 2).
set -uo pipefail
cd "$(dirname "$0")/.."

mode="${1:-fast}"
shift || true
crates=()
while [ $# -gt 0 ]; do
    case "$1" in
        -p) crates+=("-p" "$2"); shift 2 ;;
        *) echo "usage: tools/check.sh fast|full [-p crate ...]" >&2; exit 2 ;;
    esac
done

logs="$(mktemp -d)"
trap 'rm -rf "$logs"' EXIT
start=$SECONDS

web() {
    local build="$1"
    (
        cd web/studio
        [ -e node_modules ] || { echo "web: node_modules missing"; exit 1; }
        nice npm run typecheck > "$logs/web-typecheck.log" 2>&1 || { echo "web: typecheck FAILED"; tail -20 "$logs/web-typecheck.log"; exit 1; }
        nice npm run test:unit > "$logs/web-unit.log" 2>&1
        local rc=$?
        local pass fail
        pass=$(grep -E '^# pass' "$logs/web-unit.log" | grep -oE '[0-9]+' | tail -1)
        fail=$(grep -E '^# fail' "$logs/web-unit.log" | grep -oE '[0-9]+' | tail -1)
        if [ "$rc" -ne 0 ]; then
            echo "web: unit tests FAILED (${pass:-?} passed, ${fail:-?} failed)"
            grep -E '^not ok' "$logs/web-unit.log" | head -10
            exit 1
        fi
        local note=""
        if [ "$build" = build ]; then
            nice npm run build > "$logs/web-build.log" 2>&1 || { echo "web: build FAILED"; tail -20 "$logs/web-build.log"; exit 1; }
            note=", build ok"
        fi
        echo "web: typecheck ok, ${pass:-?} unit tests passed$note"
    )
}

rust() {
    local profile=() what="debug"
    if [ "$mode" = full ]; then
        profile=(--release)
        what="release"
        nice cargo build --release > "$logs/rust-build.log" 2>&1 || { echo "rust: release build FAILED"; grep -E '^error' -A5 "$logs/rust-build.log" | head -30; return 1; }
    fi
    local scope=(--workspace)
    [ ${#crates[@]} -gt 0 ] && scope=("${crates[@]}")
    # `${profile[@]+...}`: macOS's bash 3.2 reports an empty array as unbound under `set -u`.
    nice cargo test ${profile[@]+"${profile[@]}"} "${scope[@]}" --no-fail-fast > "$logs/rust-test.log" 2>&1
    local rc=$?
    local totals
    totals=$(grep -oE '[0-9]+ passed; [0-9]+ failed; [0-9]+ ignored' "$logs/rust-test.log" | awk '{p+=$1; f+=$3; i+=$5} END {print p " passed, " f " failed, " i " ignored"}')
    if [ "$rc" -ne 0 ]; then
        echo "rust ($what): FAILED: ${totals:-did not build}"
        grep -E '^error(\[|:)|FAILED|panicked' "$logs/rust-test.log" | head -20
        return 1
    fi
    echo "rust ($what): $totals"
}

case "$mode" in
    fast)
        unset EDA_SLOW_TESTS
        web nobuild > "$logs/web.out" 2>&1 &
        web_pid=$!
        rust; rust_rc=$?
        wait "$web_pid"; web_rc=$?
        ;;
    full)
        export EDA_SLOW_TESTS=1 CARGO_PROFILE_RELEASE_INCREMENTAL=true
        web build > "$logs/web.out" 2>&1 &
        web_pid=$!
        rust; rust_rc=$?
        wait "$web_pid"; web_rc=$?
        ;;
    *)
        echo "usage: tools/check.sh fast|full [-p crate ...]" >&2
        exit 2
        ;;
esac

cat "$logs/web.out"
echo "check $mode: $(( (SECONDS - start) / 60 ))m$(( (SECONDS - start) % 60 ))s"
[ "$rust_rc" -eq 0 ] && [ "$web_rc" -eq 0 ]
