#!/usr/bin/env bash
# Two `eda` binaries write the same design files, byte for byte.
#
#   tools/same_design.sh <old eda> <new eda> [kicad qa data dir]
#
# A refactor that must not change behaviour (the length type of docs/NANOMETRES.md, "The length type") is checked with it: each binary runs
# `eda pipeline` on every `examples/*.yaml` at seed 0 (schematic, placement and routing: design.json and its derived files) and
# `eda import-kicad` on every `.kicad_pcb` at the top of KiCad's `qa/data/pcbnew` (the board reader), and every file the two write is compared
# with `cmp`, except the run log (`runs.jsonl`, `activity.jsonl`: timestamps). Prints one line per case and a summary; exits non-zero on a
# difference. Scratch output goes in a fresh `mktemp -d`.
set -uo pipefail
cd "$(dirname "$0")/.."
old="$(realpath "$1")"
new="$(realpath "$2")"
qa="${3:-${KICAD_QA_DATA:-${KICAD_SRC_DIR:-}/qa/data}}"
work="$(mktemp -d -p "${TMPDIR:-/tmp}" same-design.XXXX)"
same=0
diff=0
failed=0
volatile='s/"(wall_s|revision)": *[^,}]*/"\1": X/g'

compare() {
    local name="$1" a="$2" b="$3"
    local out
    out="$(cd "$a" && find . -type f ! -name runs.jsonl ! -name activity.jsonl | sort)"
    if [ "$out" != "$(cd "$b" && find . -type f ! -name runs.jsonl ! -name activity.jsonl | sort)" ]; then
        echo "DIFF  $name: not the same files"
        diff=$((diff + 1))
        return
    fi
    local f bad=""
    while IFS= read -r f; do
        [ -n "$f" ] || continue
        # what a run stamps with the clock: `result.json`'s `wall_s`, a fill cache's `revision`
        cmp -s "$a/$f" "$b/$f" || cmp -s <(sed -E "$volatile" "$a/$f") <(sed -E "$volatile" "$b/$f") || bad="$bad $f"
    done <<< "$out"
    if [ -n "$bad" ]; then
        echo "DIFF  $name:$bad"
        diff=$((diff + 1))
    else
        echo "same  $name ($(echo "$out" | grep -c .) files)"
        same=$((same + 1))
    fi
}

run_both() {
    local name="$1"
    shift
    mkdir -p "$work/old" "$work/new"
    local ra rb
    # both run in the same directory, so the absolute paths they write (board.json's `intent`) are the same
    mkdir -p "$work/run/$name"
    (cd "$work/run/$name" && "$old" "$@" > "$work/old/$name".log 2>&1)
    ra=$?
    mv "$work/run/$name" "$work/old/$name"
    mkdir -p "$work/run/$name"
    (cd "$work/run/$name" && "$new" "$@" > "$work/new/$name".log 2>&1)
    rb=$?
    mv "$work/run/$name" "$work/new/$name"
    if [ "$ra" != "$rb" ]; then
        echo "DIFF  $name: exit $ra against $rb"
        diff=$((diff + 1))
        return
    fi
    if ! cmp -s "$work/old/$name.log" "$work/new/$name.log"; then
        # the logs carry run ids and times only where the run log does; anything else is a difference
        local norm='s#'"$work"'/(old|new)#WORK#g; /run_id|elapsed|[0-9]+(\.[0-9]+)? ?m?s\b/d'
        if ! diff <(sed -E "$norm" "$work/old/$name.log") <(sed -E "$norm" "$work/new/$name.log") > /dev/null; then
            echo "DIFF  $name: the output differs (see $work/{old,new}/$name.log)"
            diff=$((diff + 1))
            return
        fi
    fi
    [ "$ra" = 0 ] || failed=$((failed + 1))
    compare "$name" "$work/old/$name" "$work/new/$name"
}

for y in examples/*.yaml; do
    n="pipeline-$(basename "$y" .yaml)"
    run_both "$n" pipeline "$(realpath "$y")" -o out --seed 0
done
if [ -d "$qa/pcbnew" ]; then
    for b in "$qa"/pcbnew/*.kicad_pcb; do
        n="import-$(basename "$b" .kicad_pcb)"
        run_both "$n" import-kicad "$b" -o out
    done
else
    echo "no KiCad qa data at $qa: the import cases are skipped"
fi
echo "same_design: $same same, $diff different ($failed of the same runs failed in both); scratch in $work"
[ "$diff" = 0 ]
