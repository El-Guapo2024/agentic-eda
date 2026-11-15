// Expected values for web/studio/src/kicad-port/calc/{trackWidth,attenuators,viaSize,wavelength}.test.ts (KiCad 25ec2335).
// The track width fit is KiCad's own common/track_width_calculations.cpp, compiled as it is; the attenuators (attenuator_classes.cpp), the via
// calculator (panel_via_size.cpp OnViaCalculate) and the wavelength update (panel_wavelength.cpp update) need wx and are transcribed below,
// statement for statement. Every value is printed with %.17g and with the panels' "%g".
//
//   KICAD=/path/to/kicad-src-25ec2335; OUT=$(mktemp -d)
//   g++ -std=c++20 -O2 -I"$KICAD/include" web/studio/tools/calc-golden/rf_power_golden.cpp "$KICAD/common/track_width_calculations.cpp" -o "$OUT/g"
//   "$OUT/g"
#include <cmath>
#include <cstdio>
#include <track_width_calculations.h>

#define UNIT_MIL ( 1e-5 * 2.54 )
#define UNIT_MICRON 1e-6
#define UNIT_MM 1e-3
#define UNIT_OZSQFT ( 34.40 * UNIT_MICRON )

static void out( const char* name, double v ) { printf( "%s %.17g %g\n", name, v, v ); }

struct ATT { double zin, zout, att, r1, r2, r3, minAtt; bool err; };

static bool base( ATT& a, double& L, double& A )
{
    double Lmin;
    L = pow( 10, a.att / 10 );
    A = ( L + 1 ) / ( L - 1 );
    if( a.zin > a.zout )
        Lmin = ( 2 * a.zin / a.zout ) - 1 + 2 * sqrt( a.zin / a.zout * ( a.zin / a.zout - 1 ) );
    else
        Lmin = ( 2 * a.zout / a.zin ) - 1 + 2 * sqrt( a.zout / a.zin * ( a.zout / a.zin - 1 ) );
    a.minAtt = 10 * log10( Lmin );
    if( a.minAtt > a.att ) { a.err = true; return false; }
    a.err = false;
    return true;
}

int main()
{
    // Track width: the panel's defaults (1 A, 10 C, 35 um both, 20 mm, rho 1.72e-8).
    double ext = 35 * UNIT_MICRON, in = 35 * UNIT_MICRON, dT = 10, I = 1, len = 20 * UNIT_MM, rho = 1.72e-8;
    double extW = TRACK_WIDTH_CALCULATIONS::WidthFromCurrent( I, ext / UNIT_MIL, dT, false ) * UNIT_MIL;
    double intW = TRACK_WIDTH_CALCULATIONS::WidthFromCurrent( I, in / UNIT_MIL, dT, true ) * UNIT_MIL;
    out( "tw.extW_mm", extW / UNIT_MM );
    out( "tw.intW_mm", intW / UNIT_MM );
    out( "tw.extArea_mm2", ( extW * ext ) / ( UNIT_MM * UNIT_MM ) );
    double extR = ( rho * len ) / ( extW * ext ), intR = ( rho * len ) / ( intW * in );
    out( "tw.extR", extR ); out( "tw.intR", intR );
    out( "tw.extV", extR * I ); out( "tw.extLoss", extR * I * I );
    // From an external width of 0.5 mm.
    double I2 = TRACK_WIDTH_CALCULATIONS::CurrentFromWidth( 0.5 * UNIT_MM / UNIT_MIL, ext / UNIT_MIL, dT, false );
    out( "tw.fromExt0.5.current", I2 );
    out( "tw.fromExt0.5.intW_mm", TRACK_WIDTH_CALCULATIONS::WidthFromCurrent( I2, in / UNIT_MIL, dT, true ) * UNIT_MIL / UNIT_MM );
    // Internal, 2 oz and 3 oz, half oz.
    out( "tw.int2oz_mils", TRACK_WIDTH_CALCULATIONS::WidthFromCurrent( 3, 2 * UNIT_OZSQFT / UNIT_MIL, 20, true ) );
    out( "tw.int3oz_mils", TRACK_WIDTH_CALCULATIONS::WidthFromCurrent( 3, 3 * UNIT_OZSQFT / UNIT_MIL, 20, true ) );
    out( "tw.inthalf_A", TRACK_WIDTH_CALCULATIONS::CurrentFromWidth( 10, 0.6, 10, true ) );

    // Attenuators.
    {
        ATT a{ 50, 50, 6 }; double L, A; base( a, L, A );
        a.r2 = ( ( L - 1 ) / 2 ) * sqrt( a.zin * a.zout / L ); a.r1 = 1 / ( ( ( A / a.zin ) ) - ( 1 / a.r2 ) ); a.r3 = 1 / ( ( ( A / a.zout ) ) - ( 1 / a.r2 ) );
        out( "pi.r1", a.r1 ); out( "pi.r2", a.r2 ); out( "pi.r3", a.r3 );
    }
    {
        ATT a{ 75, 50, 10 }; double L, A; base( a, L, A );
        a.r2 = ( ( L - 1 ) / 2 ) * sqrt( a.zin * a.zout / L ); a.r1 = 1 / ( ( ( A / a.zin ) ) - ( 1 / a.r2 ) ); a.r3 = 1 / ( ( ( A / a.zout ) ) - ( 1 / a.r2 ) );
        out( "pi75.r1", a.r1 ); out( "pi75.r2", a.r2 ); out( "pi75.r3", a.r3 ); out( "pi75.minAtt", a.minAtt );
    }
    {
        ATT a{ 50, 50, 6 }; double L, A; base( a, L, A );
        a.r2 = ( 2 * sqrt( L * a.zin * a.zout ) ) / ( L - 1 ); a.r1 = a.zin * A - a.r2; a.r3 = a.zout * A - a.r2;
        out( "tee.r1", a.r1 ); out( "tee.r2", a.r2 ); out( "tee.r3", a.r3 );
    }
    {
        ATT a{ 50, 50, 6 }; double L, A; a.zin = a.zout; base( a, L, A );
        L = pow( 10, a.att / 20 ); a.r1 = a.zin * ( L - 1 ); a.r2 = a.zin / ( L - 1 );
        out( "bridge.r1", a.r1 ); out( "bridge.r2", a.r2 );
    }
    {
        ATT a{ 300, 50, 3 }; double L, A; bool ok = base( a, L, A );
        printf( "err.ok %d\n", ok ); out( "err.minAtt", a.minAtt ); printf( "err.msg %f\n", a.minAtt );
    }

    // Via: the panel's defaults.
    {
        double finishedHoleDia = 0.4 * UNIT_MM, platingThickness = 0.035 * UNIT_MM, viaLength = 1.6 * UNIT_MM, padDia = 0.6 * UNIT_MM;
        double clearanceDia = 1.0 * UNIT_MM, charImpedance = 50, appliedCurrent = 1, platingResistivity = 1.72e-8, relativePermitivity = 4.5;
        double temperatureDiff = 10, pulseRiseTime = 1;
        double area = M_PI * ( finishedHoleDia + platingThickness ) * platingThickness;
        double viaResistance = platingResistivity * viaLength / area;
        const double thermalResistivity = 2.49e-3;
        double thermalResistance = thermalResistivity * viaLength / area;
        double voltageDrop = appliedCurrent * viaResistance;
        double powerLoss = appliedCurrent * voltageDrop;
        area /= pow( UNIT_MIL, 2 );
        double estimatedAmpacity = 0.048 * pow( temperatureDiff, 0.44 ) * pow( area, 0.725 );
        double capacitance = 55.51 * relativePermitivity * viaLength * padDia;
        capacitance /= clearanceDia - padDia;
        double timeDegradation = 2.2 * capacitance * charImpedance / 2;
        double inductance = 200 * viaLength;
        inductance *= log( 4 * viaLength / finishedHoleDia ) + 1;
        double reactance = M_PI * inductance / pulseRiseTime;
        out( "via.R", viaResistance ); out( "via.V", voltageDrop ); out( "via.P", powerLoss ); out( "via.amp", estimatedAmpacity );
        out( "via.thermal", thermalResistance ); out( "via.C", capacitance ); out( "via.rise", timeDegradation ); out( "via.L", inductance );
        out( "via.X", reactance );
    }

    // Wavelength: 1 GHz, er 4.5, mur 1, units GHz / ns / cm / cm / m/s.
    {
        double f = 1e9, er = 4.5, mur = 1;
        out( "wl.period_ns", 1 / f / 1e-9 );
        out( "wl.vac_cm", 299792458 / f / 1e-2 );
        out( "wl.med_cm", 299792458 / f / sqrt( er * mur ) / 1e-2 );
        out( "wl.speed", 299792458 / sqrt( er * mur ) / 1.0 );
        out( "wl.speed_kmh", 299792458 / sqrt( er * mur ) / ( 1 / 3.6 ) );
    }
    return 0;
}
