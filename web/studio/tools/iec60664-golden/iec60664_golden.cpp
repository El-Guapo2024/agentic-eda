// Golden data for web/studio/src/kicad-port/calc/iec60664.ts: KiCad's own IEC 60664-1 tables (pcb_calculator/calculator_panels/iec60664.cpp,
// KiCad 25ec2335) compiled unchanged (it needs no wx), driven over a grid of inputs the way PANEL_ELECTRICAL_SPACING_IEC60664 drives it:
//   "impulse": SetOvervoltageCategory + SetRatedVoltage -> GetRatedImpulseWithstandVoltage (CalculateTransientImpulse)
//   "spacing": the setters of CalculateClearanceCreepage -> Compute -> groove, creepage, clearance
// Built by hand (g++ and KiCad's source only); to regenerate the fixture:
//
//   KICAD=/path/to/kicad-src-25ec2335; OUT=$(mktemp -d)
//   g++ -std=c++17 -O2 -I"$KICAD/pcb_calculator/calculator_panels" web/studio/tools/iec60664-golden/iec60664_golden.cpp \
//       "$KICAD/pcb_calculator/calculator_panels/iec60664.cpp" -o "$OUT/iec60664_golden"
//   "$OUT/iec60664_golden" > web/studio/src/kicad-port/calc/iec60664.golden.json
//
// Doubles are written with 17 significant digits.
#include <cstdio>
#include "iec60664.h"

int main()
{
    const double rated[] = { 0, 30, 50, 51, 100, 120, 150, 230, 300, 400, 600, 800, 1000, 1200, 1250, 1500, 1501, 2000 };
    std::printf( "{\n  \"impulse\": [\n" );
    bool first = true;
    for( int ovc = 1; ovc <= 4; ovc++ )
        for( double v : rated )
        {
            IEC60664 cal;
            cal.SetOvervoltageCategory( (IEC60664::OV_CATEGORY) ovc );
            cal.SetRatedVoltage( v );
            std::printf( "%s    [%d, %.17g, %.17g]", first ? "" : ",\n", ovc, v, cal.GetRatedImpulseWithstandVoltage() );
            first = false;
        }
    std::printf( "\n  ],\n  \"spacing\": [\n" );
    const double transient[] = { 0.33, 0.5, 0.8, 1, 1.5, 2.5, 4, 6, 8, 12, 50, 70 };
    const double peak[] = { 0.03, 0.5, 1.3, 7, 60 };
    const double rms[] = { 10, 50, 120, 230, 400, 900, 1100, 5000, 20000 };
    const double alt[] = { 0, 2000, 2500, 5000, 25000 };
    first = true;
    long n = 0;
    for( int ins = 1; ins <= 3; ins++ )
        for( int pd = 1; pd <= 4; pd++ )
            for( int mg = 1; mg <= 4; mg++ )
                for( int pcb = 0; pcb <= 1; pcb++ )
                    for( double t : transient )
                        for( double p : peak )
                            for( double r : rms )
                                for( double a : alt )
                                {
                                    if( n++ % 227 != 0 )
                                        continue;   // one point in 227 of the grid is plenty
                                    IEC60664 cal;
                                    cal.SetPollutionDegree( (IEC60664::POLLUTION_DEGREE) pd );
                                    cal.SetMaterialGroup( (IEC60664::MATERIAL_GROUP) mg );
                                    cal.SetInsulationType( (IEC60664::INSULATION_TYPE) ins );
                                    cal.SetAltitude( a );
                                    cal.SetTransientVoltage( t );
                                    cal.SetPeakVoltage( p );
                                    cal.SetRMSVoltage( r );
                                    cal.SetPCBMaterial( pcb );
                                    cal.Compute();
                                    std::printf( "%s    [%d, %d, %d, %d, %.17g, %.17g, %.17g, %.17g, %.17g, %.17g, %.17g]", first ? "" : ",\n",
                                                 ins, pd, mg, pcb, t, p, r, a, cal.GetMinGrooveWidth(), cal.GetCreepageDistance(),
                                                 cal.GetClearanceDistance() );
                                    first = false;
                                }
    std::printf( "\n  ]\n}\n" );
    return 0;
}
