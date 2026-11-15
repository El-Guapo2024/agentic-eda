// Golden data for web/studio/src/kicad-port/calc/resistorSubstitution.ts and eseries.ts: KiCad's own pcb_calculator/resistor_substitution_utils.cpp
// and eseries.cpp (KiCad 25ec2335), compiled unchanged with GCC (its std::sort decides which of equal-valued combinations is found), run as
// PANEL_R_CALCULATOR::OnCalculateESeries runs them: NewCalc( target ), Exclude( target ), Exclude( two more ), Calculate.
//
//   KICAD=/path/to/kicad-src-25ec2335; OUT=$(mktemp -d)
//   g++ -std=c++20 -O2 -I"$KICAD/pcb_calculator" web/studio/tools/calc-golden/rescalc_golden.cpp \
//       "$KICAD"/pcb_calculator/{resistor_substitution_utils,eseries}.cpp -o "$OUT/rescalc_golden"
//   "$OUT/rescalc_golden" > web/studio/src/kicad-port/calc/resistorSubstitution.golden.json
#include <cmath>
#include <cstdio>
#include <string>
#include <vector>
#include "resistor_substitution_utils.h"

static std::string num( double v )
{
    if( std::isnan( v ) ) return "\"NaN\"";
    char buf[64];
    snprintf( buf, sizeof buf, "%.17g", v );
    return buf;
}

int main()
{
    struct CASE { int series; double target; double ex1; double ex2; };
    std::vector<CASE> cases;
    const double targets[] = { 2.5, 3.3, 7, 12.34, 20, 47, 100, 123.4, 270, 333, 1000, 1234.5, 2200, 4700, 5555, 10000, 12345, 33000,
                               47000, 68000, 99999, 100000, 150000, 470000, 1e6, 1.5e6, 3.9e6, 4e6 };
    for( int s = 0; s <= 4; ++s )
        for( double t : targets )
            cases.push_back( { s, t, NAN, NAN } );
    cases.push_back( { 2, 4700, 2200, 1000 } );
    cases.push_back( { 4, 4700, 2200, 2400 } );
    cases.push_back( { 3, 10000, 4700, 5600 } );
    cases.push_back( { 4, 1234, 1200, 1300 } );

    RES_EQUIV_CALC calc;
    printf( "{\"kicad\":\"25ec2335\",\"cases\":[\n" );
    for( size_t i = 0; i < cases.size(); ++i )
    {
        const CASE& c = cases[i];
        calc.SetSeries( c.series );
        calc.NewCalc( c.target );
        calc.Exclude( c.target );
        calc.Exclude( c.ex1 );
        calc.Exclude( c.ex2 );
        calc.Calculate();
        printf( "{\"series\":%d,\"target\":%s,\"exclude\":[%s,%s],\"results\":[", c.series, num( c.target ).c_str(), num( c.ex1 ).c_str(),
                num( c.ex2 ).c_str() );
        const auto& r = calc.GetResults();
        for( size_t k = 0; k < r.size(); ++k )
        {
            if( k ) printf( "," );
            if( r[k] ) printf( "{\"value\":%s,\"name\":\"%s\"}", num( r[k]->value ).c_str(), r[k]->name.c_str() );
            else printf( "null" );
        }
        printf( "]}%s\n", i + 1 < cases.size() ? "," : "" );
    }
    printf( "]}\n" );
}
