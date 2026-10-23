// Oracle driver (build_direction.sh): DIRECTION_45::BuildInitialTrace (libs/kimath/src/geometry/direction_45.cpp) over the real kimath.
// In, per line (nanometres): dir p0x p0y p1x p1y startDiagonal mode   (dir: 0..7 = N..NW, -1 undefined; mode: CORNER_MODE's order)
// Out, per line: the chain's pieces in order: "P x y" for a point not on an arc, "A sx sy mx my ex ey" once per arc.
#include <cstdio>
#include <iostream>
#include <sstream>
#include <string>
#include <geometry/direction45.h>
#include <geometry/shape_line_chain.h>
int main()
{
    std::string line;
    while( std::getline( std::cin, line ) )
    {
        std::istringstream in( line );
        long long d, x0, y0, x1, y1, sd, m;
        in >> d >> x0 >> y0 >> x1 >> y1 >> sd >> m;
        DIRECTION_45 dir = d < 0 ? DIRECTION_45() : DIRECTION_45( (DIRECTION_45::Directions) d );
        SHAPE_LINE_CHAIN c = dir.BuildInitialTrace( VECTOR2I( x0, y0 ), VECTOR2I( x1, y1 ), sd != 0, (DIRECTION_45::CORNER_MODE) m );
        ssize_t lastArc = -1;
        for( int i = 0; i < c.PointCount(); i++ )
        {
            ssize_t a = c.ArcIndex( i );
            if( a < 0 || !c.IsPtOnArc( i ) )
            {
                printf( "P %d %d ", c.CPoint( i ).x, c.CPoint( i ).y );
            }
            else if( a != lastArc )
            {
                const SHAPE_ARC& arc = c.Arc( a );
                printf( "A %d %d %d %d %d %d ", arc.GetP0().x, arc.GetP0().y, arc.GetArcMid().x, arc.GetArcMid().y, arc.GetP1().x, arc.GetP1().y );
                lastArc = a;
            }
        }
        printf( "\n" );
        fflush( stdout );
    }
}
