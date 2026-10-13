// Oracle driver: KiCad's own SHAPE_ARC, SHAPE_LINE_CHAIN and PNS::MEANDER_* (compiled from pcbnew/router/pns_meander.cpp and the
// tuning functions of pns_meander_placer_base.cpp, over the real libs/kimath) run on a list of cases read from stdin; one result line
// per case on stdout. The Rust side (crates/pns/tests/meander_oracle.rs) generates the same cases and compares point for point.
//
// Cases (one per line, integers except where noted; nanometres):
//   ARC sx sy ex ey angle_millideg
//   CHAIN n x y x y ...                      -> Simplify, Length, Split(a, b) with the last four numbers
//   SHAPE type ax ay bx by px py side width clearance dual offset min max spacing step style radius single tbl amp nobs [cx cy r]...
//   LINE ax ay bx by side width clearance min max spacing step style radius single initial_side elong nobs [cx cy r]...
//   COUPLED np x y .. nn x y .. width gap    -> DIFF_PAIR::CoupledSegmentPairs
//   DLINE <the same> offset                  (a pair: MEANDERED_LINE( .., true ) with the baseline offset, the corners left to the caller)
#include <cstdio>
#include <cstdlib>
#include <iostream>
#include <sstream>
#include <string>
#include <vector>

#include <geometry/shape_arc.h>
#include <geometry/shape_line_chain.h>
#include <math/vector2d.h>
#include "pns_meander.h"
#include "pns_meander_placer_base.h"
#include "oracle_dp.h"

using namespace PNS;

struct Circle { long long cx, cy, r; };
static std::vector<Circle> g_obstacles;

static bool chainHits( const SHAPE_LINE_CHAIN& c )
{
    for( int i = 0; i < c.PointCount(); i++ )
    {
        for( const Circle& o : g_obstacles )
        {
            long long dx = c.CPoint( i ).x - o.cx, dy = c.CPoint( i ).y - o.cy;
            if( dx * dx + dy * dy < o.r * o.r )
                return true;
        }
    }
    return false;
}

// The stand-in for CheckFit the Rust side repeats: no vertex of either line inside an obstacle circle.
static bool fitRule( MEANDER_SHAPE* aShape )
{
    if( chainHits( aShape->CLine( 0 ) ) )
        return false;
    if( aShape->IsDual() && chainHits( aShape->CLine( 1 ) ) )
        return false;
    return true;
}

static void printChain( const char* tag, const SHAPE_LINE_CHAIN& c )
{
    printf( " %s %d", tag, c.PointCount() );
    for( int i = 0; i < c.PointCount(); i++ )
        printf( " %d,%d,%d,%d", c.CPoint( i ).x, c.CPoint( i ).y, (int) c.CShapes()[i].first, (int) c.CShapes()[i].second );
    printf( " arcs %d", (int) c.ArcCount() );
    for( size_t i = 0; i < c.ArcCount(); i++ )
    {
        const SHAPE_ARC& a = c.CArcs()[i];
        printf( " %d,%d,%d,%d,%d,%d", a.GetP0().x, a.GetP0().y, a.GetArcMid().x, a.GetArcMid().y, a.GetP1().x, a.GetP1().y );
    }
    printf( " len %lld", (long long) c.Length() );
}

static MEANDER_TYPE typeOf( int t )
{
    switch( t )
    {
    case 0: return MT_SINGLE;
    case 1: return MT_START;
    case 2: return MT_FINISH;
    case 3: return MT_TURN;
    case 4: return MT_CHECK_START;
    case 5: return MT_CHECK_FINISH;
    default: return MT_EMPTY;
    }
}

static void readObstacles( std::istringstream& in )
{
    g_obstacles.clear();
    int n = 0;
    in >> n;
    for( int i = 0; i < n; i++ )
    {
        Circle o;
        in >> o.cx >> o.cy >> o.r;
        g_obstacles.push_back( o );
    }
}

int main()
{
    std::string line;

    while( std::getline( std::cin, line ) )
    {
        std::istringstream in( line );
        std::string        cmd;
        in >> cmd;

        if( cmd == "ARC" )
        {
            long long sx, sy, ex, ey, mdeg;
            in >> sx >> sy >> ex >> ey >> mdeg;
            SHAPE_ARC arc;
            arc.ConstructFromStartEndAngle( VECTOR2I( sx, sy ), VECTOR2I( ex, ey ), EDA_ANGLE( mdeg / 1000.0, DEGREES_T ) );
            SHAPE_LINE_CHAIN pl = arc.ConvertToPolyline( 1000 );
            printf( "ARC mid %d,%d center %d,%d r %.6f len %.6f ccw %d", arc.GetArcMid().x, arc.GetArcMid().y, arc.GetCenter().x, arc.GetCenter().y,
                    arc.GetRadius(), arc.GetLength(), (int) arc.IsCCW() );
            printChain( "poly", pl );
            printf( "\n" );
        }
        else if( cmd == "CHAIN" )
        {
            int n;
            in >> n;
            std::vector<VECTOR2I> pts;
            for( int i = 0; i < n; i++ )
            {
                long long x, y;
                in >> x >> y;
                pts.push_back( VECTOR2I( x, y ) );
            }
            long long ax, ay, bx, by;
            in >> ax >> ay >> bx >> by;
            SHAPE_LINE_CHAIN c( pts, false );
            SHAPE_LINE_CHAIN s = c;
            s.Simplify();
            printf( "CHAIN len %lld", (long long) c.Length() );
            printChain( "simp", s );
            SHAPE_LINE_CHAIN pre, mid, post;
            c.Split( VECTOR2I( ax, ay ), VECTOR2I( bx, by ), pre, mid, post );
            printChain( "pre", pre );
            printChain( "mid", mid );
            printChain( "post", post );
            VECTOR2I np = c.NearestPoint( VECTOR2I( ax, ay ), false );
            printf( " nearest %d,%d\n", np.x, np.y );
        }
        else if( cmd == "COUPLED" )
        {
            DP_STAND_IN dp;
            int         counts[2];
            for( int k = 0; k < 2; k++ )
            {
                in >> counts[k];
                SHAPE_LINE_CHAIN& c = k == 0 ? dp.m_p : dp.m_n;
                for( int i = 0; i < counts[k]; i++ )
                {
                    long long x, y;
                    in >> x >> y;
                    c.Append( VECTOR2I( x, y ) );
                }
            }
            int gap;
            in >> dp.m_width >> gap;
            dp.m_gapConstraint = RANGED_NUM<int>( gap, 10000, 10000 );
            COUPLED_SEGMENTS_VEC pairs;
            dp.CoupledSegmentPairs( pairs );
            printf( "COUPLED n %d", (int) pairs.size() );
            for( const COUPLED_SEGMENTS& c : pairs )
                printf( " p %d,%d,%d,%d n %d,%d,%d,%d i %d,%d", c.coupledP.A.x, c.coupledP.A.y, c.coupledP.B.x, c.coupledP.B.y, c.coupledN.A.x, c.coupledN.A.y,
                        c.coupledN.B.x, c.coupledN.B.y, c.indexP, c.indexN );
            printf( "\n" );
        }
        else if( cmd == "SHAPE" || cmd == "LINE" || cmd == "DLINE" )
        {
            MEANDER_PLACER_BASE placer;
            placer.m_fit = fitRule;
            long long ax, ay, bx, by, px = 0, py = 0;
            int       type = 0, side, width, clearance, dual = 0, offset = 0, mn, mx, spacing, step, style, radius, single, tbl = 0, amp = 0;
            int       initialSide = 0;
            long long elong = 0;
            if( cmd == "SHAPE" )
            {
                in >> type >> ax >> ay >> bx >> by >> px >> py >> side >> width >> clearance >> dual >> offset >> mn >> mx >> spacing >> step >> style >> radius
                   >> single >> tbl >> amp;
            }
            else
            {
                in >> ax >> ay >> bx >> by >> side >> width >> clearance >> mn >> mx >> spacing >> step >> style >> radius >> single >> initialSide >> elong;
                if( cmd == "DLINE" )
                    in >> offset;
            }
            readObstacles( in );

            placer.m_clearance = clearance;
            placer.m_currentWidth = width;
            placer.m_settings.m_minAmplitude = mn;
            placer.m_settings.m_maxAmplitude = mx;
            placer.m_settings.m_spacing = spacing;
            placer.m_settings.m_step = step;
            placer.m_settings.m_cornerStyle = style == 0 ? MEANDER_STYLE_CHAMFER : MEANDER_STYLE_ROUND;
            placer.m_settings.m_cornerRadiusPercentage = radius;
            placer.m_settings.m_singleSided = single != 0;
            placer.m_settings.m_initialSide = (MEANDER_SIDE) initialSide;

            SEG base( VECTOR2I( ax, ay ), VECTOR2I( bx, by ) );

            if( cmd == "SHAPE" )
            {
                MEANDER_SHAPE m( &placer, width, dual != 0 );
                m.SetBaselineOffset( offset );
                m.SetTargetBaselineLength( tbl );
                bool ok = m.Fit( typeOf( type ), base, VECTOR2I( px, py ), side != 0 );
                if( ok && amp > 0 )
                    m.Resize( amp );
                printf( "SHAPE ok %d amp %d end %d,%d base %lld mincr %d minamp %d tunable %lld", (int) ok, m.Amplitude(), m.End().x, m.End().y,
                        (long long) m.BaselineLength(), 0, m.MinAmplitude(), (long long) ( ok ? m.MinTunableLength() : 0 ) );
                if( ok )
                {
                    printChain( "l0", m.CLine( 0 ) );
                    if( dual )
                        printChain( "l1", m.CLine( 1 ) );
                }
                printf( "\n" );
            }
            else if( cmd == "DLINE" )
            {
                MEANDERED_LINE result( &placer, true );
                placer.m_result = &result;
                placer.m_dualFit = true;
                result.SetWidth( width );
                result.SetBaselineOffset( offset );
                result.AddCorner( base.A, base.A );
                result.MeanderSegment( base, side != 0 );
                result.AddCorner( base.B, base.B );
                placer.tuneLineLength( result, elong );
                int n = 0;
                SHAPE_LINE_CHAIN tunedP, tunedN;
                std::string kinds, amps;
                for( MEANDER_SHAPE* m : result.Meanders() )
                {
                    n++;
                    kinds += std::to_string( (int) m->Type() ) + ",";
                    amps += std::to_string( m->Amplitude() ) + ",";
                    if( m->Type() != MT_EMPTY )
                    {
                        tunedP.Append( m->CLine( 0 ) );
                        tunedN.Append( m->CLine( 1 ) );
                    }
                }
                printf( "DLINE n %d kinds %s amps %s side %d", n, kinds.c_str(), amps.c_str(), (int) placer.m_settings.m_initialSide );
                printChain( "p", tunedP );
                printChain( "n", tunedN );
                printf( "\n" );
            }
            else
            {
                MEANDERED_LINE result( &placer, false );
                placer.m_result = &result;
                result.SetWidth( width );
                result.SetBaselineOffset( 0 );
                result.AddCorner( base.A );
                result.MeanderSegment( base, side != 0 );
                result.AddCorner( base.B );
                int n = 0;
                SHAPE_LINE_CHAIN tuned;
                std::string kinds, amps;
                // the elongation is asked for after the fitting, as MEANDER_PLACER::doMove does
                placer.tuneLineLength( result, elong );
                for( MEANDER_SHAPE* m : result.Meanders() )
                {
                    n++;
                    kinds += std::to_string( (int) m->Type() ) + ",";
                    amps += std::to_string( m->Amplitude() ) + ",";
                    if( m->Type() != MT_EMPTY )
                        tuned.Append( m->CLine( 0 ) );
                }
                printf( "LINE n %d kinds %s amps %s side %d", n, kinds.c_str(), amps.c_str(), (int) placer.m_settings.m_initialSide );
                printChain( "tuned", tuned );
                SHAPE_LINE_CHAIN simple = tuned;
                simple.Simplify();
                printChain( "simp", simple );
                printf( "\n" );
            }
        }
        else if( !cmd.empty() )
        {
            printf( "UNKNOWN %s\n", cmd.c_str() );
        }
    }

    return 0;
}
