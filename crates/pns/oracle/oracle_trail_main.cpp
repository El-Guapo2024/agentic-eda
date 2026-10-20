// Oracle driver for the mouse trail: KiCad's own PNS::MOUSE_TRAIL_TRACER (pcbnew/router/pns_mouse_trail_tracer.cpp, verbatim) and DIRECTION_45
// (libs/kimath/src/geometry/direction_45.cpp) over the real libs/kimath geometry, run on the cases read from stdin; one result line per case.
// The Rust side (crates/pns/tests/mouse_trail_oracle.rs) generates the same cases and compares token for token.
//
// A case, one per line, nanometres:
//   TRAIL tolerance init last mouse_disabled nops op...
//     init, last: a direction, 0 = N, 1 = NE, 2 = E, 3 = SE, 4 = S, 5 = SW, 6 = W, 7 = NW, -1 = undefined (SetDefaultDirections( init, last ))
//     op:  M x y   LINE_PLACER::Move: GetPosture( x, y ), then AddTrailPoint( x, y )
//          F       FlipPosture
//          R x y init last    Clear(); SetTolerance( tolerance ); AddTrailPoint( x, y ); SetDefaultDirections( init, last )   (FixRoute / UnfixRoute)
//          D flag  SetMouseDisabled( flag )
// The tracer starts as LINE_PLACER::Start leaves it: Clear(), AddTrailPoint( 0, 0 ), SetTolerance, SetDefaultDirections, SetMouseDisabled.
// The answer: one "d<direction> m<lead x>,<lead y> f<manually forced>" per M or F.
#include <cstdio>
#include <iostream>
#include <sstream>
#include <string>

#include <geometry/direction45.h>
#include <geometry/shape_line_chain.h>
#include <math/vector2d.h>
#include "pns_mouse_trail_tracer.h"

using namespace PNS;

static DIRECTION_45 dir( int d )
{
    return d < 0 ? DIRECTION_45( DIRECTION_45::UNDEFINED ) : DIRECTION_45( static_cast<DIRECTION_45::Directions>( d ) );
}

// DIRECTION_45 hides its value: find it by the vector of ToVector and the diagonal flag.
static int number( const DIRECTION_45& d )
{
    for( int i = 0; i < 8; i++ )
    {
        if( dir( i ) == d )
            return i;
    }
    return -1;
}

int main()
{
    std::string line;
    while( std::getline( std::cin, line ) )
    {
        std::istringstream in( line );
        std::string tag;
        in >> tag;
        if( tag != "TRAIL" )
        {
            std::cout << "?\n";
            continue;
        }
        long long tolerance;
        int init, last, disabled, nops;
        in >> tolerance >> init >> last >> disabled >> nops;

        MOUSE_TRAIL_TRACER t;
        t.Clear();
        t.AddTrailPoint( VECTOR2I( 0, 0 ) );
        t.SetTolerance( (int) tolerance );
        t.SetDefaultDirections( dir( init ), dir( last ) );
        t.SetMouseDisabled( disabled != 0 );

        std::string out;
        for( int i = 0; i < nops; i++ )
        {
            std::string op;
            in >> op;
            if( op == "M" )
            {
                long long x, y;
                in >> x >> y;
                DIRECTION_45 d = t.GetPosture( VECTOR2I( (int) x, (int) y ) );
                t.AddTrailPoint( VECTOR2I( (int) x, (int) y ) );
                VECTOR2I lead = t.GetTrailLeadVector();
                out += " d" + std::to_string( number( d ) ) + " m" + std::to_string( lead.x ) + "," + std::to_string( lead.y ) + " f" + std::to_string( t.IsManuallyForced() ? 1 : 0 );
            }
            else if( op == "F" )
            {
                t.FlipPosture();
                VECTOR2I lead = t.GetTrailLeadVector();
                out += " F m" + std::to_string( lead.x ) + "," + std::to_string( lead.y ) + " f" + std::to_string( t.IsManuallyForced() ? 1 : 0 );
            }
            else if( op == "R" )
            {
                long long x, y;
                int ri, rl;
                in >> x >> y >> ri >> rl;
                t.Clear();
                t.SetTolerance( (int) tolerance );
                t.AddTrailPoint( VECTOR2I( (int) x, (int) y ) );
                t.SetDefaultDirections( dir( ri ), dir( rl ) );
                out += " R";
            }
            else if( op == "D" )
            {
                int flag;
                in >> flag;
                t.SetMouseDisabled( flag != 0 );
                out += " D";
            }
        }
        std::cout << "ok" << out << "\n";
    }
    return 0;
}
