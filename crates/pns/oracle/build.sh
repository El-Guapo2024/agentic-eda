#!/bin/bash
# Builds the oracle the meander tests compare against: KiCad's own pcbnew/router/pns_meander.cpp, the tuning functions of
# pns_meander_placer_base.cpp (findAmplitudeBinarySearch, findAmplitudeForLength, tuneLineLength: extracted verbatim) and the real
# libs/kimath geometry they stand on (SHAPE_ARC, SHAPE_LINE_CHAIN, SEG, VECTOR2, EDA_ANGLE, trigo), into one program that answers the case
# lines tests/meander_oracle.rs writes. No downloads: clang++ and the checkout of KiCad this repo is ported from.
#
#   KICAD_SRC_DIR=/path/to/kicad-src-8303b2ad crates/pns/oracle/build.sh [build-dir]       (default target/pns-oracle)
#   EDA_PNS_ORACLE=<build-dir>/oracle cargo test -p eda-pns --test meander_oracle -- --nocapture
#
# macOS and clang (it links with -undefined dynamic_lookup, and gives the symbols the real objects name but never run an empty body).
# What stands in for KiCad's world is in standins/: wx and Clipper2 headers (only the names the kimath headers need), the PNS headers of
# pns_meander.cpp, and the placer that fits a shape against circles and against the shapes of its own line (MEANDER_PLACER::CheckFit's two halves).
# shape_line_chain.cpp is compiled without the Clipper2 and SHAPE_POLY_SET functions (the Path64 constructor, convertToClipper2, OffsetLine,
# TransformToPolygon), which the geometry of a meander does not use.
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
K="${KICAD_SRC_DIR:?set KICAD_SRC_DIR to the KiCad checkout (8303b2ad)}"
out="${1:-$here/../../../target/pns-oracle}"
mkdir -p "$out/obj"
out="$(cd "$out" && pwd)"
G="$K/libs/kimath/src/geometry"
INC="-I $here/standins -I $K/libs/kimath/include -I $K/libs/core/include -I $K/include -I $K/pcbnew/router -I /usr/local/include"
cxx() { nice clang++ -std=c++20 -O1 -w -DNDEBUG -c $INC "$1" -o "$2"; }

# shape_line_chain.cpp without the functions that need Clipper2 and SHAPE_POLY_SET
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

# the tuning functions of pns_meander_placer_base.cpp, verbatim
s = open(K + "/pcbnew/router/pns_meander_placer_base.cpp").read()
tol = "const int LENGTH_TARGET_TOLERANCE = 20;"
body = s[s.index("int findAmplitudeBinarySearch"):s.index("const MEANDER_SETTINGS& MEANDER_PLACER_BASE::MeanderSettings()")]
amp = s[s.index("void MEANDER_PLACER_BASE::AmplitudeStep"):s.index("int MEANDER_PLACER_BASE::Clearance()")]
d = open(K + "/pcbnew/router/pns_diff_pair.cpp").read()
cpp = d[d.index("bool commonParallelProjection( SEG p, SEG n, SEG &pClip, SEG& nClip )\n{"):d.index("double DIFF_PAIR::Skew() const")]
csp = d[d.index("void DIFF_PAIR::CoupledSegmentPairs( COUPLED_SEGMENTS_VEC& aPairs ) const"):d.index("int64_t DIFF_PAIR::CoupledLength( const SHAPE_LINE_CHAIN& aP")].replace("void DIFF_PAIR::CoupledSegmentPairs", "void DP_STAND_IN::CoupledSegmentPairs")
open(out + "/oracle_dp.cpp", "w").write('// Verbatim from pcbnew/router/pns_diff_pair.cpp: commonParallelProjection and DIFF_PAIR::CoupledSegmentPairs (of a stand-in pair).\n#include "oracle_dp.h"\nnamespace PNS {\n' + cpp + "\n" + csp + "\n}\n")
open(out + "/oracle_tune.cpp", "w").write('// Verbatim from pcbnew/router/pns_meander_placer_base.cpp: LENGTH_TARGET_TOLERANCE, findAmplitudeBinarySearch, findAmplitudeForLength,\n// MEANDER_PLACER_BASE::tuneLineLength, AmplitudeStep and SpacingStep.\n#include "pns_meander_placer_base.h"\nnamespace PNS {\n' + tol + "\n\n" + body + "\n" + amp + "\n}\n")
PY
cp "$K/pcbnew/router/pns_meander.cpp" "$out/pns_meander.cpp"

for f in seg eda_angle shape_arc geometry_utils shape circle shape_rect shape_compound shape_collisions shape_nearest_points shape_segment; do
  cxx "$G/$f.cpp" "$out/obj/$f.o"
done
cxx "$K/libs/kimath/src/trigo.cpp" "$out/obj/trigo.o"
cxx "$here/standins/util_shim.cpp" "$out/obj/util.o"
cxx "$here/standins/poly_stubs.cpp" "$out/obj/poly_stubs.o"
cxx "$out/shape_line_chain_patched.cpp" "$out/obj/slc.o"
cxx "$out/pns_meander.cpp" "$out/obj/pns_meander.o"
cxx "$out/oracle_tune.cpp" "$out/obj/oracle_tune.o"
cxx "$out/oracle_dp.cpp" "$out/obj/oracle_dp.o"
cxx "$here/oracle_main.cpp" "$out/obj/oracle_main.o"

# link; every symbol of KiCad's that the real objects name and the oracle never runs (the polygon conversions, circle collisions of shapes it
# does not build) gets an empty definition, data symbols a zeroed block
: > "$out/stubs.s"
for round in 1 2 3 4 5; do
  nice clang++ -std=c++20 -O1 -Wl,-undefined,dynamic_lookup -o "$out/oracle" "$out"/obj/*.o 2>/dev/null
  syms=$(nm -u "$out/oracle" | grep -E "^__Z(NK?[0-9]+(SHAPE|CIRCLE|SEG|EDA_ANGLE)|TV[0-9]+(SHAPE|CIRCLE)|TI[0-9]+(SHAPE|CIRCLE)|N?K?[0-9]+(SHAPE_POLY_SET|SHAPE_RECT|SHAPE_CIRCLE|SHAPE_SIMPLE|SHAPE_COMPOUND|SHAPE_SEGMENT|SHAPE_ARC|SHAPE_LINE_CHAIN))|^__Z[0-9]+Transform" | sort -u || true)
  [ -z "$syms" ] && break
  for s in $syms; do
    grep -q "^$s:" "$out/stubs.s" && continue
    case "$s" in
      __ZTV*|__ZTI*) printf '.data\n.globl %s\n.p2align 4\n%s:\n.zero 512\n' "$s" "$s" >> "$out/stubs.s" ;;
      *) printf '.text\n.globl %s\n%s:\nret\n' "$s" "$s" >> "$out/stubs.s" ;;
    esac
  done
  clang -c "$out/stubs.s" -o "$out/obj/zz_stubs.o"
done
nice clang++ -std=c++20 -O1 -Wl,-undefined,dynamic_lookup -o "$out/oracle" "$out"/obj/*.o
echo "$out/oracle"
