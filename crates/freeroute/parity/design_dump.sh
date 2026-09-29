#!/bin/sh
# Dump the boards FreeRouting v1.9 reads from the DSN eda-freeroute writes
# for placed designs, into ../tests/design/<name>/board.txt, next to the
# design and its intent:
#
#   design_dump.sh                          redo every board in tests/design
#   design_dump.sh <name> <design.json> <intent.yaml>
#                                           add a board
#
# The dump carries FreeRouting's routing of the board too, pass by pass, up
# to MAZE_PASSES (default 20) passes, as maze_dump.sh's do; MAZE_PASS=0 dumps
# the board alone. FreeRouting runs with automatic neckdown off, as
# eda-freeroute routes designs.
# Needs Java 25 and a FreeRouting checkout, as maze_dump.sh does.
set -eu
here=$(cd "$(dirname "$0")" && pwd)
fr=${FREEROUTING:-$HOME/ws/freerouting}
JAVA_HOME=${JAVA_HOME:-$(brew --prefix openjdk@25)/libexec/openjdk.jdk/Contents/Home}
export JAVA_HOME
out="$here/../tests/design"
root="$here/../../.."
records=$(sed -n "s/^records='\(.*\)'$/\1/p" "$here/maze_dump.sh")
(cd "$root" && cargo build -q --release -p eda-freeroute --example design_dsn)
(cd "$fr" && ./gradlew -q executableV19Jar)
jar="$fr/build/libs/freerouting-1.9.0-executable.jar"
if [ $# -eq 3 ]; then
  mkdir -p "$out/$1"
  cp "$2" "$out/$1/design.json"
  cp "$3" "$out/$1/intent.yaml"
  set -- "$1"
elif [ $# -eq 0 ]; then
  set -- $(cd "$out" && ls)
else
  echo "usage: design_dump.sh [<name> <design.json> <intent.yaml>]" >&2
  exit 2
fi
tmp=$(mktemp -d)
for name in "$@"; do
  "$root/target/release/examples/design_dsn" "$out/$name/design.json" "$out/$name/intent.yaml" > "$tmp/$name.dsn"
  (cd "$fr" && AUTOMATIC_NECKDOWN=0 MAZE_PASS=${MAZE_PASS:-1000000} MAZE_PASSES=${MAZE_PASSES:-20} "$JAVA_HOME/bin/java" -cp "$jar" "$here/MazeParity.java" "$tmp/$name.dsn" 2>/dev/null) | grep -E "$records" > "$out/$name/board.txt"
  echo "$name: $(grep -c '^it ' "$out/$name/board.txt") items"
done
rm -r "$tmp"
