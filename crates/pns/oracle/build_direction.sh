#!/bin/bash
# Builds the DIRECTION_45::BuildInitialTrace oracle (libs/kimath/src/geometry/direction_45.cpp and the kimath objects it stands on, verbatim)
# with g++ on Linux -- the corner modes' test compares against it (tests/direction45_oracle.rs):
#
#   KICAD_SRC_DIR=/path/to/kicad-src-8303b2ad crates/pns/oracle/build_direction.sh [build-dir]     (default target/pns-oracle-dir)
#   EDA_PNS_ORACLE_DIR=<build-dir>/oracle_direction cargo test -p eda-pns --test direction45_oracle -- --nocapture
#
# The stand-ins are build.sh's (standins/); shape_line_chain.cpp is compiled without its Clipper2 and SHAPE_POLY_SET functions, as there.
# Symbols the objects name and the oracle never runs are left unresolved (--unresolved-symbols=ignore-all).
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
K="${KICAD_SRC_DIR:?set KICAD_SRC_DIR to the KiCad checkout (8303b2ad)}"
out="${1:-$here/../../../target/pns-oracle-dir}"
mkdir -p "$out/obj"
out="$(cd "$out" && pwd)"
G="$K/libs/kimath/src/geometry"
INC="-include cstring -I $here/standins -I $K/libs/kimath/include -I $K/libs/core/include -I $K/include"
python3 - "$K" "$out" <<'PY'
import sys
K, out = sys.argv[1], sys.argv[2]
lines = open(K + "/libs/kimath/src/geometry/shape_line_chain.cpp").read().split("\n")
def remove(sig):
    i = next(n for n, l in enumerate(lines) if l.startswith(sig))
    j = i
    while lines[j] != "}":
        j += 1
    for k in range(i, j + 1):
        lines[k] = ""
for sig in ["SHAPE_LINE_CHAIN::SHAPE_LINE_CHAIN( const Clipper2Lib::Path64&", "Clipper2Lib::Path64 SHAPE_LINE_CHAIN::convertToClipper2", "bool SHAPE_LINE_CHAIN::OffsetLine", "void SHAPE_LINE_CHAIN::TransformToPolygon"]:
    remove(sig)
text = "\n".join(lines).replace("#include <geometry/shape_poly_set.h>", "").replace("#include <limits>", "#include <limits>\n#include <set>", 1)
open(out + "/shape_line_chain_patched.cpp", "w").write(text)
PY
for f in "$G/seg.cpp" "$G/eda_angle.cpp" "$G/shape_arc.cpp" "$G/geometry_utils.cpp" "$G/shape.cpp" "$G/direction_45.cpp" "$K/libs/kimath/src/trigo.cpp" \
         "$here/standins/util_shim.cpp" "$here/standins/poly_stubs.cpp" "$out/shape_line_chain_patched.cpp" "$here/oracle_direction_main.cpp"; do
  nice g++ -std=c++20 -O1 -w -DNDEBUG -c $INC "$f" -o "$out/obj/$(basename "$f" .cpp).o"
done
nice g++ -o "$out/oracle_direction" "$out"/obj/*.o -Wl,--unresolved-symbols=ignore-all 2>/dev/null
echo "$out/oracle_direction"
