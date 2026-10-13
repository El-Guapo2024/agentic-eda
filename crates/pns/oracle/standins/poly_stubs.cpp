// Stand-ins for what the oracle never calls but the vtables of the real shapes name: the polygon conversions (SHAPE_POLY_SET).
#include <geometry/shape_poly_set.h>
#include <geometry/shape_line_chain.h>
#include <geometry/shape_arc.h>
#include <convert_basic_shapes_to_polygon.h>
SHAPE_POLY_SET::SHAPE_POLY_SET() : SHAPE( SH_POLY_SET ) {}
SHAPE_POLY_SET::~SHAPE_POLY_SET() {}
int SHAPE_POLY_SET::AddOutline( const SHAPE_LINE_CHAIN& ) { return 0; }
void SHAPE_LINE_CHAIN::TransformToPolygon( SHAPE_POLY_SET&, int, ERROR_LOC ) const {}
void TransformArcToPolygon( SHAPE_POLY_SET&, const VECTOR2I&, const VECTOR2I&, const VECTOR2I&, int, int, ERROR_LOC ) {}
