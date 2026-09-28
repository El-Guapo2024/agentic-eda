#!/bin/sh
# Regenerate the maze parity dumps in ../tests/maze from FreeRouting v1.9.
#
#   maze_dump.sh                 regenerate every board already in tests/maze
#   maze_dump.sh <board>...      dump these fixtures (names without .dsn): from
#                                parity/fixtures if there, else FreeRouting's
#   maze_dump.sh --all <dir>     dump every fixture FreeRouting can read into
#                                <dir>, skipping the rest; then, from the
#                                workspace root, check them with
#                                FREEROUTE_MAZE_DIR=<dir> cargo test --release \
#                                  -p eda-freeroute --test maze
#
# Needs Java 25 and a FreeRouting checkout, as dump.sh does.
set -eu
set -o pipefail
here=$(cd "$(dirname "$0")" && pwd)
fr=${FREEROUTING:-$HOME/ws/freerouting}
JAVA_HOME=${JAVA_HOME:-$(brew --prefix openjdk@25)/libexec/openjdk.jdk/Contents/Home}
export JAVA_HOME
out="$here/../tests/maze"
mkdir -p "$out"
all=""
if [ "${1:-}" = "--all" ]; then
  mkdir -p "${2:?usage: maze_dump.sh --all <dir>}"
  all=$(cd "$2" && pwd)
  set --
fi
if [ $# -eq 0 ] && [ -z "$all" ]; then
  set -- $(cd "$out" && ls *.txt | sed 's/\.txt$//')
fi
(cd "$fr" && ./gradlew -q executableV19Jar)
jar="$fr/build/libs/freerouting-1.9.0-executable.jar"
# Keep only the dump's own records: FreeRouting logs to the same stream.
records='^(fixture|board|layer|host_cad|area_section|min_trace_half_width|trace_half_widths|pull_tight_accuracy|id_generator|default_via_diameter|pin_edge_to_turn_dist|classes|cm|cmax|padstack|padstack_shape|viainfo|viarule|netclass|net|settings|layer_costs|it|center|via_padstack|pin_neckdown|pin_exit|conduction|pad|trace|area|area_piece|outline|outline_shape|tree_class|item|exact|ditem|route|start_item|dest_item|ctrl|step|result|path|maze|located|located_trace|id_max|inserted|ins_trace|ins_via|optimized|opt_trace|opt_via|pass|conn|conn_del|conn_add)( |$)'
cd "$fr"
dump() {
  "$JAVA_HOME/bin/java" -cp "$jar" "$here/MazeParity.java" "$1" 2>/dev/null | grep -E "$records"
}
if [ -n "$all" ]; then
  for f in fixtures/*.dsn; do
    b=$(basename "$f" .dsn)
    if dump "$f" > "$all/$b.txt.tmp"; then
      mv "$all/$b.txt.tmp" "$all/$b.txt"
    else
      rm -f "$all/$b.txt.tmp"
      echo "skipped $b" >&2
    fi
  done
  exit 0
fi
for board in "$@"; do
  dsn="$here/fixtures/$board.dsn"
  [ -f "$dsn" ] || dsn="$fr/fixtures/$board.dsn"
  dump "$dsn" > "$out/$board.txt"
done
