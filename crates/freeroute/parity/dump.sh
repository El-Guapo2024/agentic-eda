#!/bin/sh
# Regenerate the parity dumps in ../tests/parity from FreeRouting v1.9 itself.
#
#   dump.sh                    regenerate every board already in tests/parity
#   dump.sh <board>...         dump these fixtures (names without .dsn)
#   dump.sh --steps <board>    also record each completion, to find where a
#                              board diverges (see RoomParity.java)
#
# Needs Java 25 (JAVA_HOME, else Homebrew's openjdk@25) and a FreeRouting
# checkout (FREEROUTING, default ~/ws/freerouting), whose src_v19 tree is the
# v1.9 source this crate ports. Each board is filled for its first net with
# two pins.
set -eu
# A failed run must not leave a truncated dump behind the grep below.
set -o pipefail
here=$(cd "$(dirname "$0")" && pwd)
fr=${FREEROUTING:-$HOME/ws/freerouting}
JAVA_HOME=${JAVA_HOME:-$(brew --prefix openjdk@25)/libexec/openjdk.jdk/Contents/Home}
export JAVA_HOME
out="$here/../tests/parity"
mkdir -p "$out"
steps=""
if [ "${1:-}" = "--steps" ]; then
  steps="--steps"
  shift
fi
if [ $# -eq 0 ]; then
  set -- $(cd "$out" && ls *.txt | sed 's/\.txt$//')
fi
(cd "$fr" && ./gradlew -q executableV19Jar)
jar="$fr/build/libs/freerouting-1.9.0-executable.jar"
# FreeRouting logs warnings, timestamped, to the same stream: keep only the
# dump's own records, so a regenerated file is byte-identical.
records='^(fixture|board|net|item|tree_class|cm|pad|area_section|area|start|step|grown|cand|made|room|door|obstacle_doors)( |$)'
# Run from the FreeRouting checkout: it writes a logs/ directory wherever it
# runs.
cd "$fr"
for board in "$@"; do
  "$JAVA_HOME/bin/java" -cp "$jar" "$here/RoomParity.java" "$fr/fixtures/$board.dsn" $steps | grep -E "$records" > "$out/$board.txt"
done
