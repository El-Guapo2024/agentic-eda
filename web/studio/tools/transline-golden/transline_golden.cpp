// Golden data for web/studio/src/kicad-port/transline.ts and tuningCalculator.ts: KiCad's own transmission-line calculators
// (common/transline_calculations/, KiCad 25ec2335) compiled as they are, driven two ways --
//
//   "transline": each calculator analysed or synthesized from a full parameter set, with the result map KiCad's UI reads
//                (GetAnalysisResults / GetSynthesisResults) and Synthesize's return;
//   "calculator": the Calculator Tools' Transmission Lines panel (pcb_calculator, the nine line types), from the values typed to the calculator's
//                result maps; see the "calculator" section below.
//   "panel":     PANEL_SETUP_TUNING_PROFILE_INFO::calculateTrackParametersForCell over a small stackup, a grid row and a column. The panel itself
//                (pcbnew/dialogs/panel_setup_tuning_profile_info.cpp) needs wx and a BOARD, so its functions are transcribed below, statement for statement,
//                with the grid cells and the stackup items as plain values; each is named after the function it copies.
//
// The calculators need no wx; units.h only wants the HAVE_CMATH_* macros KiCad's config.h gives. To regenerate the fixture:
//
//   KICAD=/path/to/kicad-src-25ec2335; OUT=$(mktemp -d)
//   g++ -std=c++20 -O2 -DHAVE_CMATH_ASINH -DHAVE_CMATH_ACOSH -DHAVE_CMATH_ATANH -I"$KICAD/common" \
//       web/studio/tools/transline-golden/transline_golden.cpp \
//       "$KICAD"/common/transline_calculations/{transline_calculation_base,microstrip,stripline,coupled_microstrip,coupled_stripline,dielectric_djordjevic_sarkar,coax,coplanar,rectwaveguide,twistedpair}.cpp \
//       -o "$OUT/transline_golden"
//   "$OUT/transline_golden" > web/studio/src/kicad-port/transline.golden.json
//
// Doubles are written with 17 significant digits (they read back exactly); NaN and the infinities as the strings "NaN", "Infinity", "-Infinity".

#include <cmath>
#include <cstdio>
#include <map>
#include <optional>
#include <string>
#include <utility>
#include <vector>

#include <transline_calculations/coax.h>
#include <transline_calculations/coplanar.h>
#include <transline_calculations/coupled_microstrip.h>
#include <transline_calculations/coupled_stripline.h>
#include <transline_calculations/microstrip.h>
#include <transline_calculations/rectwaveguide.h>
#include <transline_calculations/stripline.h>
#include <transline_calculations/twistedpair.h>
#include <transline_calculations/units.h>

using TCP = TRANSLINE_PARAMETERS;

// ---------------------------------------------------------------------------------------------------- JSON output

static std::string num( double v )
{
    if( std::isnan( v ) )
        return "\"NaN\"";

    if( std::isinf( v ) )
        return v > 0 ? "\"Infinity\"" : "\"-Infinity\"";

    char buf[64];
    snprintf( buf, sizeof( buf ), "%.17g", v );
    return buf;
}

static std::string str( const std::string& s )
{
    std::string out = "\"";

    for( char c : s )
    {
        if( c == '"' || c == '\\' )
            out += '\\';

        out += c;
    }

    return out + "\"";
}

static const std::vector<std::pair<TCP, const char*>> PARAM_NAMES = {
    { TCP::EPSILONR, "EPSILONR" },
    { TCP::TAND, "TAND" },
    { TCP::H, "H" },
    { TCP::H_T, "H_T" },
    { TCP::T, "T" },
    { TCP::PHYS_WIDTH, "PHYS_WIDTH" },
    { TCP::PHYS_S, "PHYS_S" },
    { TCP::PHYS_LEN, "PHYS_LEN" },
    { TCP::ROUGH, "ROUGH" },
    { TCP::MUR, "MUR" },
    { TCP::MURC, "MURC" },
    { TCP::FREQUENCY, "FREQUENCY" },
    { TCP::STRIPLINE_A, "STRIPLINE_A" },
    { TCP::Z0, "Z0" },
    { TCP::Z0_E, "Z0_E" },
    { TCP::Z0_O, "Z0_O" },
    { TCP::ANG_L, "ANG_L" },
    { TCP::SIGMA, "SIGMA" },
    { TCP::SKIN_DEPTH, "SKIN_DEPTH" },
    { TCP::LOSS_DIELECTRIC, "LOSS_DIELECTRIC" },
    { TCP::LOSS_CONDUCTOR, "LOSS_CONDUCTOR" },
    { TCP::EPSILON_EFF, "EPSILON_EFF" },
    { TCP::EPSILON_EFF_EVEN, "EPSILON_EFF_EVEN" },
    { TCP::EPSILON_EFF_ODD, "EPSILON_EFF_ODD" },
    { TCP::UNIT_PROP_DELAY, "UNIT_PROP_DELAY" },
    { TCP::UNIT_PROP_DELAY_ODD, "UNIT_PROP_DELAY_ODD" },
    { TCP::UNIT_PROP_DELAY_EVEN, "UNIT_PROP_DELAY_EVEN" },
    { TCP::ATTEN_COND, "ATTEN_COND" },
    { TCP::ATTEN_COND_EVEN, "ATTEN_COND_EVEN" },
    { TCP::ATTEN_COND_ODD, "ATTEN_COND_ODD" },
    { TCP::ATTEN_DILECTRIC, "ATTEN_DILECTRIC" },
    { TCP::ATTEN_DILECTRIC_EVEN, "ATTEN_DILECTRIC_EVEN" },
    { TCP::ATTEN_DILECTRIC_ODD, "ATTEN_DILECTRIC_ODD" },
    { TCP::Z_DIFF, "Z_DIFF" },
    { TCP::Z_COMM, "Z_COMM" },
    { TCP::COUPLING_K, "COUPLING_K" },
    { TCP::DIELECTRIC_MODEL_SEL, "DIELECTRIC_MODEL_SEL" },
    { TCP::EPSILONR_SPEC_FREQ, "EPSILONR_SPEC_FREQ" },
    { TCP::SOLDERMASK_PRESENT, "SOLDERMASK_PRESENT" },
    { TCP::SOLDERMASK_THICKNESS, "SOLDERMASK_THICKNESS" },
    { TCP::SOLDERMASK_EPSILONR, "SOLDERMASK_EPSILONR" },
    { TCP::SOLDERMASK_TAND, "SOLDERMASK_TAND" },
    { TCP::PHYS_DIAM_IN, "PHYS_DIAM_IN" },
    { TCP::PHYS_DIAM_OUT, "PHYS_DIAM_OUT" },
    { TCP::CUTOFF_FREQUENCY, "CUTOFF_FREQUENCY" },
    { TCP::TWISTEDPAIR_TWIST, "TWISTEDPAIR_TWIST" },
    { TCP::TWISTEDPAIR_EPSILONR_ENV, "TWISTEDPAIR_EPSILONR_ENV" },
    { TCP::CPW_BACKMETAL, "CPW_BACKMETAL" },
    { TCP::SOLDERMASK_FILLS_GAPS, "SOLDERMASK_FILLS_GAPS" },
};

static const char* paramName( TCP p )
{
    for( const auto& [id, name] : PARAM_NAMES )
    {
        if( id == p )
            return name;
    }

    return "UNKNOWN";
}

static const char* statusName( TRANSLINE_STATUS s )
{
    switch( s )
    {
    case TRANSLINE_STATUS::OK: return "OK";
    case TRANSLINE_STATUS::WARNING: return "WARNING";
    default: return "TS_ERROR";
    }
}

static std::string resultsJson( const std::unordered_map<TCP, std::pair<double, TRANSLINE_STATUS>>& aResults )
{
    // In a fixed order, so the file diffs well
    std::string out = "{";
    bool        first = true;

    for( const auto& [id, name] : PARAM_NAMES )
    {
        auto it = aResults.find( id );

        if( it == aResults.end() )
            continue;

        out += ( first ? "" : ", " ) + str( name ) + ": [" + num( it->second.first ) + ", " + str( statusName( it->second.second ) ) + "]";
        first = false;
    }

    return out + "}";
}

// ---------------------------------------------------------------------------------------------------- "transline" cases

struct RAW_CASE
{
    std::string                         name;
    std::string                         calc; // microstrip, stripline, coupled_microstrip, coupled_stripline
    std::string                         op;   // analyse, synthesize
    SYNTHESIZE_OPTS                     opts;
    std::vector<std::pair<TCP, double>> inputs;
};

static const char* optsName( SYNTHESIZE_OPTS o )
{
    switch( o )
    {
    case SYNTHESIZE_OPTS::FIX_WIDTH: return "FIX_WIDTH";
    case SYNTHESIZE_OPTS::FIX_SPACING: return "FIX_SPACING";
    case SYNTHESIZE_OPTS::FROM_ZDIFF_ZCOMM: return "FROM_ZDIFF_ZCOMM";
    default: return "DEFAULT";
    }
}

static std::string runRaw( const RAW_CASE& c )
{
    MICROSTRIP         ms;
    STRIPLINE          sl;
    COUPLED_MICROSTRIP cms;
    COUPLED_STRIPLINE  csl;

    TRANSLINE_CALCULATION_BASE* calc = c.calc == "microstrip"           ? static_cast<TRANSLINE_CALCULATION_BASE*>( &ms )
                                       : c.calc == "stripline"          ? static_cast<TRANSLINE_CALCULATION_BASE*>( &sl )
                                       : c.calc == "coupled_microstrip" ? static_cast<TRANSLINE_CALCULATION_BASE*>( &cms )
                                                                        : static_cast<TRANSLINE_CALCULATION_BASE*>( &csl );

    for( const auto& [p, v] : c.inputs )
        calc->SetParameter( p, v );

    bool converged = false;

    if( c.op == "synthesize" )
        converged = calc->Synthesize( c.opts );
    else
        calc->Analyse();

    const auto& results = c.op == "synthesize" ? calc->GetSynthesisResults() : calc->GetAnalysisResults();

    std::string out = "    {\"name\": " + str( c.name ) + ", \"calc\": " + str( c.calc ) + ", \"op\": " + str( c.op ) + ", \"opts\": " + str( optsName( c.opts ) )
                      + ",\n     \"inputs\": {";

    for( size_t i = 0; i < c.inputs.size(); ++i )
        out += ( i ? ", " : "" ) + str( paramName( c.inputs[i].first ) ) + ": " + num( c.inputs[i].second );

    out += "},\n     \"converged\": " + std::string( converged ? "true" : "false" ) + ",\n     \"results\": " + resultsJson( results ) + "}";
    return out;
}

// The parameter sets the panel hands each calculator (calculateSingleMicrostrip & co.), with the geometry varied
static constexpr double RHO = 1.72e-8;

static double skinDepth( double aFreq, double aMurc, double aSigma )
{
    return 1.0 / sqrt( M_PI * aFreq * aMurc * TRANSLINE_CALCULATIONS::MU0 * aSigma );
}

static std::vector<std::pair<TCP, double>> microstripInputs( double er, double h, double t, double tand, double f, double z0, double w, bool mask = false,
                                                             double maskT = 0, double maskEr = 0, double maskTand = 0 )
{
    std::vector<std::pair<TCP, double>> v = { { TCP::PHYS_WIDTH, w },       { TCP::SIGMA, 1.0 / RHO },
                                              { TCP::EPSILON_EFF, 1.0 },    { TCP::SKIN_DEPTH, skinDepth( f, 1.0, 1.0 / RHO ) },
                                              { TCP::EPSILONR, er },        { TCP::H_T, 1e+20 },
                                              { TCP::H, h },                { TCP::T, t },
                                              { TCP::Z0, z0 },              { TCP::FREQUENCY, f },
                                              { TCP::ROUGH, 0 },            { TCP::TAND, tand },
                                              { TCP::PHYS_LEN, 10 },        { TCP::MUR, 1 },
                                              { TCP::MURC, 1 },             { TCP::ANG_L, 1 } };

    if( mask )
    {
        v.push_back( { TCP::SOLDERMASK_PRESENT, 1.0 } );
        v.push_back( { TCP::SOLDERMASK_THICKNESS, maskT } );
        v.push_back( { TCP::SOLDERMASK_EPSILONR, maskEr } );
        v.push_back( { TCP::SOLDERMASK_TAND, maskTand } );
    }

    return v;
}

static std::vector<std::pair<TCP, double>> striplineInputs( double er, double top, double bottom, double t, double tand, double f, double z0, double w )
{
    return { { TCP::PHYS_WIDTH, w },
             { TCP::SKIN_DEPTH, skinDepth( 1.0, 1.0, 1.0 / RHO ) },
             { TCP::EPSILONR, er },
             { TCP::T, t },
             { TCP::STRIPLINE_A, top },
             { TCP::H, top + t + bottom },
             { TCP::Z0, z0 },
             { TCP::PHYS_LEN, 1.0 },
             { TCP::FREQUENCY, f },
             { TCP::TAND, tand },
             { TCP::ANG_L, 1.0 },
             { TCP::SIGMA, 1.0 / RHO },
             { TCP::MURC, 1 } };
}

static std::vector<std::pair<TCP, double>> coupledMicrostripInputs( double er, double h, double t, double tand, double f, double zd, double w, double s,
                                                                    bool mask = false, double maskT = 0, double maskEr = 0, double maskTand = 0 )
{
    std::vector<std::pair<TCP, double>> v = { { TCP::Z0_E, zd / 2.0 },   { TCP::Z0_O, zd / 2.0 },
                                              { TCP::Z_DIFF, zd },        { TCP::PHYS_WIDTH, w },
                                              { TCP::PHYS_S, s },         { TCP::EPSILONR, er },
                                              { TCP::PHYS_LEN, 10 },      { TCP::H, h },
                                              { TCP::T, t },              { TCP::H_T, 1e+20 },
                                              { TCP::FREQUENCY, f },      { TCP::MURC, 1 },
                                              { TCP::SKIN_DEPTH, skinDepth( 1.0, 1.0, 1.0 / RHO ) },
                                              { TCP::SIGMA, 1.0 / RHO },  { TCP::ROUGH, 0 },
                                              { TCP::TAND, tand },        { TCP::ANG_L, 1 } };

    if( mask )
    {
        v.push_back( { TCP::SOLDERMASK_PRESENT, 1.0 } );
        v.push_back( { TCP::SOLDERMASK_THICKNESS, maskT } );
        v.push_back( { TCP::SOLDERMASK_EPSILONR, maskEr } );
        v.push_back( { TCP::SOLDERMASK_TAND, maskTand } );
    }

    return v;
}

static std::vector<std::pair<TCP, double>> coupledStriplineInputs( double er, double top, double bottom, double t, double tand, double f, double zd, double w,
                                                                   double s )
{
    const double totalH = top + t + bottom;
    const double a = top + 0.5 * t;
    return { { TCP::Z0_E, zd / 2.0 },
             { TCP::Z0_O, zd / 2.0 },
             { TCP::Z_DIFF, zd },
             { TCP::PHYS_WIDTH, w },
             { TCP::PHYS_S, s },
             { TCP::T, t },
             { TCP::H, totalH },
             { TCP::STRIPLINE_A, COUPLED_STRIPLINE::IsCenteredOffset( a, totalH ) ? 0.0 : a },
             { TCP::EPSILONR, er },
             { TCP::SKIN_DEPTH, skinDepth( 1.0, 1.0, 1.0 / RHO ) },
             { TCP::PHYS_LEN, 1.0 },
             { TCP::FREQUENCY, f },
             { TCP::ANG_L, 1.0 },
             { TCP::SIGMA, 1.0 / RHO },
             { TCP::MURC, 1 } };
}

static std::vector<RAW_CASE> rawCases()
{
    std::vector<RAW_CASE> cases;
    const auto            A = std::string( "analyse" );
    const auto            S = std::string( "synthesize" );

    // Single microstrip: width for Z0, and the delay of a width, over dielectrics, heights, copper and frequencies
    struct MS { double er, h, t, tand, f, z0, w; };
    const MS ms[] = { { 4.5, 0.2e-3, 35e-6, 0.02, 1e9, 50, 0.35e-3 },  { 4.5, 0.1e-3, 35e-6, 0.02, 1e9, 50, 0.18e-3 },
                      { 3.66, 0.508e-3, 17.5e-6, 0.004, 5e9, 50, 1.1e-3 }, { 4.2, 0.2e-3, 70e-6, 0.015, 1e8, 75, 0.15e-3 },
                      { 2.2, 0.787e-3, 35e-6, 0.0009, 10e9, 50, 2.4e-3 }, { 9.8, 0.635e-3, 5e-6, 0.0001, 2e9, 50, 0.6e-3 },
                      { 4.5, 0.2e-3, 35e-6, 0.02, 0.0, 50, 0.35e-3 },     { 4.5, 1.5e-3, 35e-6, 0.02, 1e9, 30, 5e-3 } };

    for( size_t i = 0; i < std::size( ms ); ++i )
    {
        const MS& m = ms[i];
        cases.push_back( { "microstrip width #" + std::to_string( i ), "microstrip", S, SYNTHESIZE_OPTS::DEFAULT,
                           microstripInputs( m.er, m.h, m.t, m.tand, m.f, m.z0, 0.001 ) } );
        cases.push_back( { "microstrip delay #" + std::to_string( i ), "microstrip", A, SYNTHESIZE_OPTS::DEFAULT,
                           microstripInputs( m.er, m.h, m.t, m.tand, m.f, m.z0, m.w ) } );
    }

    // With a solder mask (Wan-Hoorfar), wide and narrow strips
    cases.push_back( { "microstrip width, mask", "microstrip", S, SYNTHESIZE_OPTS::DEFAULT,
                       microstripInputs( 4.5, 0.2e-3, 35e-6, 0.02, 1e9, 50, 0.001, true, 10e-6, 3.3, 0.025 ) } );
    cases.push_back( { "microstrip delay, mask, wide", "microstrip", A, SYNTHESIZE_OPTS::DEFAULT,
                       microstripInputs( 4.5, 0.2e-3, 35e-6, 0.02, 1e9, 50, 0.4e-3, true, 25e-6, 3.8, 0.03 ) } );
    cases.push_back( { "microstrip delay, mask, narrow", "microstrip", A, SYNTHESIZE_OPTS::DEFAULT,
                       microstripInputs( 4.5, 0.2e-3, 35e-6, 0.02, 1e9, 50, 0.1e-3, true, 10e-6, 3.3, 0.0 ) } );

    // The Djordjevic-Sarkar overlay of the base class
    {
        auto in = microstripInputs( 4.3, 0.2e-3, 35e-6, 0.02, 10e9, 50, 0.35e-3 );
        in.push_back( { TCP::DIELECTRIC_MODEL_SEL, 1 } );
        in.push_back( { TCP::EPSILONR_SPEC_FREQ, 1e9 } );
        cases.push_back( { "microstrip delay, Djordjevic-Sarkar", "microstrip", A, SYNTHESIZE_OPTS::DEFAULT, in } );
        auto in2 = in;
        in2[0].second = 0.001;
        cases.push_back( { "microstrip width, Djordjevic-Sarkar", "microstrip", S, SYNTHESIZE_OPTS::DEFAULT, in2 } );
    }

    // Single stripline: centred and offset
    struct SL { double er, top, bottom, t, tand, f, z0, w; };
    const SL sl[] = { { 4.5, 0.2e-3, 0.2e-3, 35e-6, 0.02, 1e9, 50, 0.15e-3 },   { 4.5, 0.1e-3, 0.3e-3, 17.5e-6, 0.02, 1e9, 50, 0.12e-3 },
                      { 3.5, 0.15e-3, 0.15e-3, 35e-6, 0.005, 5e9, 40, 0.2e-3 }, { 4.2, 0.3e-3, 0.1e-3, 35e-6, 0.015, 1e8, 60, 0.1e-3 },
                      { 4.5, 0.5e-3, 0.5e-3, 35e-6, 0.02, 1e9, 50, 0.6e-3 } };

    for( size_t i = 0; i < std::size( sl ); ++i )
    {
        const SL& s = sl[i];
        cases.push_back( { "stripline width #" + std::to_string( i ), "stripline", S, SYNTHESIZE_OPTS::DEFAULT,
                           striplineInputs( s.er, s.top, s.bottom, s.t, s.tand, s.f, s.z0, 0.001 ) } );
        cases.push_back( { "stripline delay #" + std::to_string( i ), "stripline", A, SYNTHESIZE_OPTS::DEFAULT,
                           striplineInputs( s.er, s.top, s.bottom, s.t, s.tand, s.f, s.z0, s.w ) } );
    }

    // Coupled microstrip: width for a gap, gap for a width, delay of both
    struct CMS { double er, h, t, tand, f, zd, w, s; };
    const CMS cms[] = { { 4.5, 0.2e-3, 35e-6, 0.02, 1e9, 100, 0.2e-3, 0.2e-3 },  { 4.5, 0.1e-3, 35e-6, 0.02, 1e9, 90, 0.12e-3, 0.15e-3 },
                        { 3.66, 0.508e-3, 17.5e-6, 0.004, 5e9, 100, 0.8e-3, 0.3e-3 }, { 4.2, 0.2e-3, 35e-6, 0.015, 1e8, 85, 0.25e-3, 0.127e-3 },
                        { 9.8, 0.635e-3, 5e-6, 0.0001, 2e9, 100, 0.4e-3, 0.5e-3 } };

    for( size_t i = 0; i < std::size( cms ); ++i )
    {
        const CMS& c = cms[i];
        cases.push_back( { "coupled microstrip width #" + std::to_string( i ), "coupled_microstrip", S, SYNTHESIZE_OPTS::FIX_SPACING,
                           coupledMicrostripInputs( c.er, c.h, c.t, c.tand, c.f, c.zd, 0.0, c.s ) } );
        cases.push_back( { "coupled microstrip gap #" + std::to_string( i ), "coupled_microstrip", S, SYNTHESIZE_OPTS::FIX_WIDTH,
                           coupledMicrostripInputs( c.er, c.h, c.t, c.tand, c.f, c.zd, c.w, 0.0 ) } );
        cases.push_back( { "coupled microstrip delay #" + std::to_string( i ), "coupled_microstrip", A, SYNTHESIZE_OPTS::DEFAULT,
                           coupledMicrostripInputs( c.er, c.h, c.t, c.tand, c.f, c.zd, c.w, c.s ) } );
    }

    cases.push_back( { "coupled microstrip width, mask", "coupled_microstrip", S, SYNTHESIZE_OPTS::FIX_SPACING,
                       coupledMicrostripInputs( 4.5, 0.2e-3, 35e-6, 0.02, 1e9, 100, 0.0, 0.2e-3, true, 10e-6, 3.3, 0.025 ) } );
    cases.push_back( { "coupled microstrip delay, mask", "coupled_microstrip", A, SYNTHESIZE_OPTS::DEFAULT,
                       coupledMicrostripInputs( 4.5, 0.2e-3, 35e-6, 0.02, 1e9, 100, 0.2e-3, 0.2e-3, true, 20e-6, 3.6, 0.03 ) } );

    // Coupled stripline: centred (the fast path) and offset (the image method)
    struct CSL { double er, top, bottom, t, tand, f, zd, w, s; };
    const CSL csl[] = { { 4.5, 0.2e-3, 0.2e-3, 35e-6, 0.02, 1e9, 100, 0.1e-3, 0.2e-3 },   { 4.5, 0.1e-3, 0.3e-3, 17.5e-6, 0.02, 1e9, 100, 0.1e-3, 0.15e-3 },
                        { 3.5, 0.15e-3, 0.15e-3, 35e-6, 0.005, 5e9, 85, 0.12e-3, 0.12e-3 }, { 4.2, 0.3e-3, 0.12e-3, 35e-6, 0.015, 1e8, 90, 0.1e-3, 0.3e-3 },
                        { 4.5, 0.2e-3, 0.2e-3, 0.0, 0.02, 1e9, 100, 0.1e-3, 0.2e-3 } };

    for( size_t i = 0; i < std::size( csl ); ++i )
    {
        const CSL& c = csl[i];
        cases.push_back( { "coupled stripline width #" + std::to_string( i ), "coupled_stripline", S, SYNTHESIZE_OPTS::FIX_SPACING,
                           coupledStriplineInputs( c.er, c.top, c.bottom, c.t, c.tand, c.f, c.zd, 0.0, c.s ) } );
        cases.push_back( { "coupled stripline gap #" + std::to_string( i ), "coupled_stripline", S, SYNTHESIZE_OPTS::FIX_WIDTH,
                           coupledStriplineInputs( c.er, c.top, c.bottom, c.t, c.tand, c.f, c.zd, c.w, 0.0 ) } );
        cases.push_back( { "coupled stripline delay #" + std::to_string( i ), "coupled_stripline", A, SYNTHESIZE_OPTS::DEFAULT,
                           coupledStriplineInputs( c.er, c.top, c.bottom, c.t, c.tand, c.f, c.zd, c.w, c.s ) } );
    }

    return cases;
}

// ---------------------------------------------------------------------------------------------------- "panel" cases

// layer_ids.h: F_Cu 0, B_Cu 2, In1_Cu 4, In2_Cu 6, ..; F_Mask / B_Mask are not copper
enum
{
    UNDEFINED_LAYER = -1,
    F_Cu = 0,
    B_Cu = 2,
    F_Mask = 1001,
    B_Mask = 1002
};

static int layerIdOf( const std::string& aName )
{
    if( aName == "F.Cu" )
        return F_Cu;

    if( aName == "B.Cu" )
        return B_Cu;

    if( aName == "F.Mask" )
        return F_Mask;

    if( aName == "B.Mask" )
        return B_Mask;

    if( aName.size() > 5 && aName.rfind( "In", 0 ) == 0 && aName.substr( aName.size() - 3 ) == ".Cu" )
        return 2 + 2 * std::stoi( aName.substr( 2, aName.size() - 5 ) );

    return UNDEFINED_LAYER;
}

// layer_ids.h IsCopperLayerLowerThan
static bool IsCopperLayerLowerThan( int aLayerA, int aLayerB )
{
    if( aLayerA == aLayerB )
        return false;

    if( aLayerA == B_Cu )
        return true;

    if( aLayerB == B_Cu )
        return false;

    return aLayerA > aLayerB;
}

enum ITEM_TYPE
{
    BS_ITEM_TYPE_COPPER,
    BS_ITEM_TYPE_DIELECTRIC,
    BS_ITEM_TYPE_SOLDERMASK,
    BS_ITEM_TYPE_SILKSCREEN
};

struct SUBLAYER
{
    int    Thickness; // IU (nm)
    double EpsilonR;
    double LossTangent;
};

// The parts of a BOARD_STACKUP_ITEM the panel reads
struct STACKUP_ITEM
{
    std::string           Name;
    ITEM_TYPE             Type;
    std::vector<SUBLAYER> Subs;

    int    GetBrdLayerId() const { return Type == BS_ITEM_TYPE_DIELECTRIC ? UNDEFINED_LAYER : layerIdOf( Name ); }
    int    GetType() const { return Type; }
    bool   HasEpsilonRValue() const { return Type == BS_ITEM_TYPE_DIELECTRIC || Type == BS_ITEM_TYPE_SOLDERMASK; }
    int    GetSublayersCount() const { return static_cast<int>( Subs.size() ); }
    int    GetThickness( int i = 0 ) const { return Subs.at( i ).Thickness; }
    double GetEpsilonR( int i = 0 ) const { return Subs.at( i ).EpsilonR; }
    double GetLossTangent( int i = 0 ) const { return Subs.at( i ).LossTangent; }
};

static double IUTomm( int iu )
{
    return iu / 1e6; // EDA_IU_SCALE::IUTomm, pcbIUScale
}

struct DIELECTRIC_INFO
{
    double Height{ 0.0 };
    double E_r{ 0.0 };
    double Loss_Tangent{ 0.0 };
};

struct CALCULATION_RESULT
{
    bool        OK{ false };
    int         Width{ 0 };
    int         DiffPairGap{ 0 };
    int         Delay{ 0 };
    std::string ErrorMsg;

    static CALCULATION_RESULT err( const std::string& aMsg )
    {
        CALCULATION_RESULT r;
        r.ErrorMsg = aMsg;
        return r;
    }
};

struct CALCULATION_BOARD_PARAMETERS
{
    int    SignalLayer{ UNDEFINED_LAYER };
    double DielectricConstant{ 0 };
    double TopDielectricLayerThickness{ 0 };
    double BottomDielectricLayerThickness{ 0 };
    double SignalLayerThickness{ 0 };
    double LossTangent{ 0 };
    double SolderMaskDielectricConstant{ 0 };
    double SolderMaskThickness{ 0 };
    double SolderMaskLossTangent{ 0 };
};

enum class CalculationType
{
    WIDTH,
    GAP,
    DELAY
};

// One profile page with one grid row
struct PANEL
{
    std::vector<STACKUP_ITEM>  Stackup;
    std::vector<std::string>   LayerNames; // m_parentPanel->m_layerNames: the copper layers
    bool                       Differential{ false };
    std::string                TargetImpedance; // m_targetImpedance text
    double                     Frequency{ 1e9 };
    bool                       ModelSolderMask{ true };
    std::string                SignalLayer, TopReference, BottomReference;
    std::optional<int>         Width, Gap; // the cells (nullopt: empty)

    bool contains( const std::string& aName ) const
    {
        for( const auto& n : LayerNames )
        {
            if( n == aName )
                return true;
        }

        return false;
    }

    // PANEL_SETUP_TUNING_PROFILE_INFO::getTargetImpedance
    double getTargetImpedance() const
    {
        char*  end = nullptr;
        double z = strtod( TargetImpedance.c_str(), &end );

        if( TargetImpedance.empty() || *end != '\0' )
            z = -1;

        return z;
    }

    // getStackupLayerId
    static int getStackupLayerId( const std::vector<STACKUP_ITEM>& aLayerList, int aPcbLayerId )
    {
        bool layerFound = false;
        int  layerStackupId = 0;

        while( layerStackupId < static_cast<int>( aLayerList.size() ) && !layerFound )
        {
            if( aLayerList.at( layerStackupId ).GetBrdLayerId() != aPcbLayerId )
                ++layerStackupId;
            else
                layerFound = true;
        }

        if( !layerFound )
            return -1;

        return layerStackupId;
    }

    // calculateAverageDielectricConstants (the constant dielectric model: the studio's stackup carries no other)
    DIELECTRIC_INFO calculateAverageDielectricConstants( const std::vector<STACKUP_ITEM>& aStackupLayerList,
                                                         const std::vector<int>&          dielectricLayerStackupIds ) const
    {
        double totalHeight = 0.0;
        double e_r = 0.0;
        double lossTangent = 0.0;

        for( int i : dielectricLayerStackupIds )
        {
            const STACKUP_ITEM& layer = aStackupLayerList.at( i );

            for( int subLayerIdx = 0; subLayerIdx < layer.GetSublayersCount(); ++subLayerIdx )
            {
                totalHeight += IUTomm( layer.GetThickness( subLayerIdx ) );
                double e_r_layer = layer.GetEpsilonR( subLayerIdx );
                double l_t_layer = layer.GetLossTangent( subLayerIdx );
                e_r += e_r_layer * IUTomm( layer.GetThickness( subLayerIdx ) );
                lossTangent += l_t_layer * IUTomm( layer.GetThickness( subLayerIdx ) );
            }
        }

        if( totalHeight <= 0.0 )
            return { 0.0, 0.0, 0.0 };

        e_r = e_r / totalHeight;
        lossTangent = lossTangent / totalHeight;
        totalHeight /= 1000.0;

        return { totalHeight, e_r, lossTangent };
    }

    // getDielectricLayers
    static void getDielectricLayers( const std::vector<STACKUP_ITEM>& aStackupLayerList, const int aSignalLayerId, const int aReferenceLayerId,
                                     std::vector<int>& aDielectricLayerStackupIds )
    {
        for( int i = std::min( aSignalLayerId, aReferenceLayerId ) + 1; i < std::max( aSignalLayerId, aReferenceLayerId ); ++i )
        {
            const STACKUP_ITEM& layer = aStackupLayerList.at( i );

            if( layer.GetType() != BS_ITEM_TYPE_DIELECTRIC )
                continue;

            if( !layer.HasEpsilonRValue() )
                continue;

            aDielectricLayerStackupIds.push_back( i );
        }
    }

    // getSolderMaskParameters
    static DIELECTRIC_INFO getSolderMaskParameters( const std::vector<STACKUP_ITEM>& aStackupLayerList, int aSignalLayerId )
    {
        int maskLayerId = UNDEFINED_LAYER;

        if( aSignalLayerId == F_Cu )
            maskLayerId = F_Mask;
        else if( aSignalLayerId == B_Cu )
            maskLayerId = B_Mask;
        else
            return { 0.0, 0.0, 0.0 };

        const int layerIdx = getStackupLayerId( aStackupLayerList, maskLayerId );

        if( layerIdx < 0 )
            return { 0.0, 0.0, 0.0 };

        const STACKUP_ITEM& layer = aStackupLayerList.at( layerIdx );
        const double        thickness = IUTomm( layer.GetThickness() ) / 1000.0;

        return { thickness, layer.GetEpsilonR(), layer.GetLossTangent() };
    }

    // getMicrostripBoardParameters
    std::pair<CALCULATION_BOARD_PARAMETERS, CALCULATION_RESULT> getMicrostripBoardParameters() const
    {
        const std::vector<STACKUP_ITEM>& stackupLayerList = Stackup;

        if( !contains( SignalLayer ) )
            return { {}, CALCULATION_RESULT::err( "Signal layer not found in stackup" ) };

        const int signalLayer = layerIdOf( SignalLayer );

        if( signalLayer != F_Cu && signalLayer != B_Cu )
            return { {}, CALCULATION_RESULT::err( "Internal error: Microstrip can only be on an outer copper layer" ) };

        const int signalLayerStackupId = getStackupLayerId( stackupLayerList, signalLayer );

        if( signalLayerStackupId == -1 )
            return { {}, CALCULATION_RESULT::err( "Signal layer not found in stackup" ) };

        const double signalLayerThickness = IUTomm( stackupLayerList.at( signalLayerStackupId ).GetThickness() ) / 1000.0;

        if( signalLayerThickness <= 0 )
            return { {}, CALCULATION_RESULT::err( "Signal layer thickness must be greater than 0" ) };

        std::string referenceLayerName = signalLayer == F_Cu ? BottomReference : TopReference;

        if( !contains( referenceLayerName ) )
            return { {}, CALCULATION_RESULT::err( "Reference layer not found in stackup" ) };

        const int referenceLayer = layerIdOf( referenceLayerName );
        const int referenceLayerStackupId = getStackupLayerId( stackupLayerList, referenceLayer );

        if( signalLayerStackupId == referenceLayerStackupId )
            return { {}, CALCULATION_RESULT::err( "Reference layer must be different to signal layer" ) };

        std::vector<int> dielectricLayerStackupIds;
        getDielectricLayers( stackupLayerList, signalLayerStackupId, referenceLayerStackupId, dielectricLayerStackupIds );

        const DIELECTRIC_INFO dielectricInfo = calculateAverageDielectricConstants( stackupLayerList, dielectricLayerStackupIds );

        if( dielectricInfo.Height <= 0.0 )
            return { {}, CALCULATION_RESULT::err( "Dielectric height must be greater than 0" ) };

        const DIELECTRIC_INFO solderMaskInfo = getSolderMaskParameters( stackupLayerList, signalLayer );

        CALCULATION_BOARD_PARAMETERS boardParameters{ signalLayer,          dielectricInfo.E_r, dielectricInfo.Height,
                                                      0.0,                  signalLayerThickness, dielectricInfo.Loss_Tangent,
                                                      solderMaskInfo.E_r,   solderMaskInfo.Height, solderMaskInfo.Loss_Tangent };
        CALCULATION_RESULT           result;
        result.OK = true;

        return { boardParameters, result };
    }

    // getStriplineBoardParameters
    std::pair<CALCULATION_BOARD_PARAMETERS, CALCULATION_RESULT> getStriplineBoardParameters() const
    {
        const std::vector<STACKUP_ITEM>& stackupLayerList = Stackup;

        if( !contains( SignalLayer ) )
            return { {}, CALCULATION_RESULT::err( "Signal layer not found in stackup" ) };

        const int signalLayer = layerIdOf( SignalLayer );
        const int signalLayerStackupId = getStackupLayerId( stackupLayerList, signalLayer );

        if( signalLayerStackupId == -1 )
            return { {}, CALCULATION_RESULT::err( "Signal layer not found in stackup" ) };

        const double signalLayerThickness = IUTomm( stackupLayerList.at( signalLayerStackupId ).GetThickness() ) / 1000.0;

        if( signalLayerThickness <= 0 )
            return { {}, CALCULATION_RESULT::err( "Signal layer thickness must be greater than 0" ) };

        if( !contains( TopReference ) )
            return { {}, CALCULATION_RESULT::err( "Top reference layer not found in stackup" ) };

        const int topReferenceLayer = layerIdOf( TopReference );
        const int topReferenceLayerStackupId = getStackupLayerId( stackupLayerList, topReferenceLayer );

        if( !IsCopperLayerLowerThan( signalLayer, topReferenceLayer ) )
            return { {}, CALCULATION_RESULT::err( "Top reference layer must be above signal layer in board stackup" ) };

        if( !contains( BottomReference ) )
            return { {}, CALCULATION_RESULT::err( "Bottom reference layer not found in stackup" ) };

        const int bottomReferenceLayer = layerIdOf( BottomReference );
        const int bottomReferenceLayerStackupId = getStackupLayerId( stackupLayerList, bottomReferenceLayer );

        if( !IsCopperLayerLowerThan( bottomReferenceLayer, signalLayer ) )
            return { {}, CALCULATION_RESULT::err( "Bottom reference layer must be below signal layer in board stackup" ) };

        std::vector<int> topDielectricLayerStackupIds, bottomDielectricLayerStackupIds;

        getDielectricLayers( stackupLayerList, signalLayerStackupId, topReferenceLayerStackupId, topDielectricLayerStackupIds );
        getDielectricLayers( stackupLayerList, signalLayerStackupId, bottomReferenceLayerStackupId, bottomDielectricLayerStackupIds );

        std::vector<int> allDielectricLayerStackupIds( topDielectricLayerStackupIds );
        allDielectricLayerStackupIds.insert( allDielectricLayerStackupIds.end(), bottomDielectricLayerStackupIds.begin(),
                                             bottomDielectricLayerStackupIds.end() );

        const DIELECTRIC_INFO topDielectricInfo = calculateAverageDielectricConstants( stackupLayerList, topDielectricLayerStackupIds );
        const DIELECTRIC_INFO bottomDielectricInfo = calculateAverageDielectricConstants( stackupLayerList, bottomDielectricLayerStackupIds );
        const DIELECTRIC_INFO allDielectricInfo = calculateAverageDielectricConstants( stackupLayerList, allDielectricLayerStackupIds );

        if( topDielectricInfo.Height <= 0.0 && bottomDielectricInfo.Height <= 0.0 )
            return { {}, CALCULATION_RESULT::err( "Dielectric heights must be greater than 0" ) };

        CALCULATION_BOARD_PARAMETERS boardParameters{ signalLayer,          allDielectricInfo.E_r, topDielectricInfo.Height, bottomDielectricInfo.Height,
                                                      signalLayerThickness, allDielectricInfo.Loss_Tangent };
        CALCULATION_RESULT           result;
        result.OK = true;

        return { boardParameters, result };
    }

    // EDA_UNIT_UTILS::UI::FromUserUnit( pcbIUScale, MM / PS_PER_CM, .. ), cast to int as the panel does
    static int mmToIU( double aMM ) { return static_cast<int>( aMM * 1e6 ); }
    static int psPerCmToIU( double aPsPerCm ) { return static_cast<int>( aPsPerCm / 10 * 1e6 ); }

    // calculateSingleMicrostrip
    CALCULATION_RESULT calculateSingleMicrostrip( CalculationType aCalculationType ) const
    {
        MICROSTRIP m_microstripCalc;
        const double targetZ = getTargetImpedance();

        if( targetZ <= 0 )
            return CALCULATION_RESULT::err( "Target impedance must be greater than 0" );

        auto [boardParameters, result] = getMicrostripBoardParameters();

        if( !result.OK )
            return result;

        if( aCalculationType == CalculationType::WIDTH )
        {
            m_microstripCalc.SetParameter( TCP::PHYS_WIDTH, 0.001 );
        }
        else if( aCalculationType == CalculationType::DELAY )
        {
            const int    widthInt = Width.value_or( 0 );
            const double width = IUTomm( widthInt ) / 1000.0;
            m_microstripCalc.SetParameter( TCP::PHYS_WIDTH, width );
        }

        const double frequency = Frequency;

        m_microstripCalc.SetParameter( TCP::SIGMA, 1.0 / RHO );
        m_microstripCalc.SetParameter( TCP::EPSILON_EFF, 1.0 );
        m_microstripCalc.SetParameter( TCP::SKIN_DEPTH, skinDepth( frequency, 1.0, 1.0 / RHO ) );
        m_microstripCalc.SetParameter( TCP::EPSILONR, boardParameters.DielectricConstant );
        m_microstripCalc.SetParameter( TCP::H_T, 1e+20 );
        m_microstripCalc.SetParameter( TCP::H, boardParameters.TopDielectricLayerThickness );
        m_microstripCalc.SetParameter( TCP::T, boardParameters.SignalLayerThickness );
        m_microstripCalc.SetParameter( TCP::Z0, targetZ );
        m_microstripCalc.SetParameter( TCP::FREQUENCY, frequency );
        m_microstripCalc.SetParameter( TCP::ROUGH, 0 );
        m_microstripCalc.SetParameter( TCP::TAND, boardParameters.LossTangent );
        m_microstripCalc.SetParameter( TCP::PHYS_LEN, 10 );
        m_microstripCalc.SetParameter( TCP::MUR, 1 );
        m_microstripCalc.SetParameter( TCP::MURC, 1 );
        m_microstripCalc.SetParameter( TCP::ANG_L, 1 );

        if( ModelSolderMask )
        {
            m_microstripCalc.SetParameter( TCP::SOLDERMASK_PRESENT, 1.0 );
            m_microstripCalc.SetParameter( TCP::SOLDERMASK_THICKNESS, boardParameters.SolderMaskThickness );
            m_microstripCalc.SetParameter( TCP::SOLDERMASK_EPSILONR, boardParameters.SolderMaskDielectricConstant );
            m_microstripCalc.SetParameter( TCP::SOLDERMASK_TAND, boardParameters.SolderMaskLossTangent );
        }

        if( aCalculationType == CalculationType::WIDTH )
            m_microstripCalc.Synthesize( SYNTHESIZE_OPTS::DEFAULT );
        else
            m_microstripCalc.Analyse();

        auto& results = aCalculationType == CalculationType::WIDTH ? m_microstripCalc.GetSynthesisResults() : m_microstripCalc.GetAnalysisResults();

        if( results[TCP::PHYS_WIDTH].second != TRANSLINE_STATUS::OK )
            return CALCULATION_RESULT::err( "Width calculation failed" );

        if( results[TCP::UNIT_PROP_DELAY].second != TRANSLINE_STATUS::OK )
            return CALCULATION_RESULT::err( "Delay calculation failed" );

        CALCULATION_RESULT r;
        r.OK = true;
        r.Width = mmToIU( results[TCP::PHYS_WIDTH].first * 1000.0 );
        r.Delay = psPerCmToIU( results[TCP::UNIT_PROP_DELAY].first );
        return r;
    }

    // calculateSingleStripline
    CALCULATION_RESULT calculateSingleStripline( CalculationType aCalculationType ) const
    {
        STRIPLINE    m_striplineCalc;
        const double targetZ = getTargetImpedance();

        if( targetZ <= 0 )
            return CALCULATION_RESULT::err( "Target impedance must be greater than 0" );

        auto [boardParameters, result] = getStriplineBoardParameters();

        if( !result.OK )
            return result;

        if( aCalculationType == CalculationType::WIDTH )
        {
            m_striplineCalc.SetParameter( TCP::PHYS_WIDTH, 0.001 );
        }
        else if( aCalculationType == CalculationType::DELAY )
        {
            const int    widthInt = Width.value_or( 0 );
            const double width = IUTomm( widthInt ) / 1000.0;
            m_striplineCalc.SetParameter( TCP::PHYS_WIDTH, width );
        }

        m_striplineCalc.SetParameter( TCP::SKIN_DEPTH, skinDepth( 1.0, 1.0, 1.0 / RHO ) );
        m_striplineCalc.SetParameter( TCP::EPSILONR, boardParameters.DielectricConstant );
        m_striplineCalc.SetParameter( TCP::T, boardParameters.SignalLayerThickness );
        m_striplineCalc.SetParameter( TCP::STRIPLINE_A, boardParameters.TopDielectricLayerThickness );
        m_striplineCalc.SetParameter( TCP::H, boardParameters.TopDielectricLayerThickness + boardParameters.SignalLayerThickness
                                                      + boardParameters.BottomDielectricLayerThickness );
        m_striplineCalc.SetParameter( TCP::Z0, targetZ );
        m_striplineCalc.SetParameter( TCP::PHYS_LEN, 1.0 );
        m_striplineCalc.SetParameter( TCP::FREQUENCY, Frequency );
        m_striplineCalc.SetParameter( TCP::TAND, boardParameters.LossTangent );
        m_striplineCalc.SetParameter( TCP::ANG_L, 1.0 );
        m_striplineCalc.SetParameter( TCP::SIGMA, 1.0 / RHO );
        m_striplineCalc.SetParameter( TCP::MURC, 1 );

        if( aCalculationType == CalculationType::WIDTH )
            m_striplineCalc.Synthesize( SYNTHESIZE_OPTS::DEFAULT );
        else
            m_striplineCalc.Analyse();

        auto& results = aCalculationType == CalculationType::WIDTH ? m_striplineCalc.GetSynthesisResults() : m_striplineCalc.GetAnalysisResults();

        if( results[TCP::PHYS_WIDTH].second != TRANSLINE_STATUS::OK )
            return CALCULATION_RESULT::err( "Width calculation failed" );

        if( results[TCP::UNIT_PROP_DELAY].second != TRANSLINE_STATUS::OK )
            return CALCULATION_RESULT::err( "Delay calculation failed" );

        CALCULATION_RESULT r;
        r.OK = true;
        r.Width = mmToIU( results[TCP::PHYS_WIDTH].first * 1000.0 );
        r.Delay = psPerCmToIU( results[TCP::UNIT_PROP_DELAY].first );
        return r;
    }

    // The width / gap checks shared by calculateDifferentialMicrostrip and calculateDifferentialStripline
    std::optional<CALCULATION_RESULT> diffGeometry( CalculationType aCalculationType, double& width, double& gap ) const
    {
        const std::optional<int> widthOpt = Width;
        const std::optional<int> gapOpt = Gap;

        if( aCalculationType == CalculationType::WIDTH )
        {
            if( !gapOpt || *gapOpt <= 0 )
                return CALCULATION_RESULT::err( "Diff pair gap must be greater than 0 to calculate width" );

            gap = IUTomm( gapOpt.value() ) / 1000.0;
        }
        else if( aCalculationType == CalculationType::GAP )
        {
            if( !widthOpt || *widthOpt <= 0 )
                return CALCULATION_RESULT::err( "Width must be greater than 0 to calculate diff pair gap" );

            width = IUTomm( widthOpt.value() ) / 1000.0;
        }
        else if( aCalculationType == CalculationType::DELAY )
        {
            if( !widthOpt || !gapOpt || *widthOpt <= 0 || *gapOpt <= 0 )
                return CALCULATION_RESULT::err( "Width and diff pair gap must be greater than 0 to calculate delay" );

            width = IUTomm( widthOpt.value() ) / 1000.0;
            gap = IUTomm( gapOpt.value() ) / 1000.0;
        }

        return std::nullopt;
    }

    static CALCULATION_RESULT diffResults( std::unordered_map<TCP, std::pair<double, TRANSLINE_STATUS>>& results )
    {
        if( results[TCP::PHYS_WIDTH].second != TRANSLINE_STATUS::OK )
            return CALCULATION_RESULT::err( "Width calculation failed" );

        if( results[TCP::PHYS_S].second != TRANSLINE_STATUS::OK )
            return CALCULATION_RESULT::err( "Diff pair gap calculation failed" );

        if( results[TCP::UNIT_PROP_DELAY_ODD].second != TRANSLINE_STATUS::OK )
            return CALCULATION_RESULT::err( "Delay calculation failed" );

        CALCULATION_RESULT r;
        r.OK = true;
        r.Width = mmToIU( results[TCP::PHYS_WIDTH].first * 1000.0 );
        r.DiffPairGap = mmToIU( results[TCP::PHYS_S].first * 1000.0 );
        r.Delay = psPerCmToIU( results[TCP::UNIT_PROP_DELAY_ODD].first );
        return r;
    }

    // calculateDifferentialMicrostrip
    CALCULATION_RESULT calculateDifferentialMicrostrip( CalculationType aCalculationType ) const
    {
        COUPLED_MICROSTRIP m_coupledMicrostripCalc;
        const double       targetZ = getTargetImpedance();

        if( targetZ <= 0 )
            return CALCULATION_RESULT::err( "Target impedance must be greater than 0" );

        auto [boardParameters, result] = getMicrostripBoardParameters();

        if( !result.OK )
            return result;

        double width = 0.0;
        double gap = 0.0;

        if( auto bad = diffGeometry( aCalculationType, width, gap ) )
            return *bad;

        m_coupledMicrostripCalc.SetParameter( TCP::Z0_E, targetZ / 2.0 );
        m_coupledMicrostripCalc.SetParameter( TCP::Z0_O, targetZ / 2.0 );
        m_coupledMicrostripCalc.SetParameter( TCP::Z_DIFF, targetZ );
        m_coupledMicrostripCalc.SetParameter( TCP::PHYS_WIDTH, width );
        m_coupledMicrostripCalc.SetParameter( TCP::PHYS_S, gap );
        m_coupledMicrostripCalc.SetParameter( TCP::EPSILONR, boardParameters.DielectricConstant );
        m_coupledMicrostripCalc.SetParameter( TCP::PHYS_LEN, 10 );
        m_coupledMicrostripCalc.SetParameter( TCP::H, boardParameters.TopDielectricLayerThickness );
        m_coupledMicrostripCalc.SetParameter( TCP::T, boardParameters.SignalLayerThickness );
        m_coupledMicrostripCalc.SetParameter( TCP::H_T, 1e+20 );
        m_coupledMicrostripCalc.SetParameter( TCP::FREQUENCY, Frequency );
        m_coupledMicrostripCalc.SetParameter( TCP::MURC, 1 );
        m_coupledMicrostripCalc.SetParameter( TCP::SKIN_DEPTH, skinDepth( 1.0, 1.0, 1.0 / RHO ) );
        m_coupledMicrostripCalc.SetParameter( TCP::SIGMA, 1.0 / RHO );
        m_coupledMicrostripCalc.SetParameter( TCP::ROUGH, 0 );
        m_coupledMicrostripCalc.SetParameter( TCP::TAND, boardParameters.LossTangent );
        m_coupledMicrostripCalc.SetParameter( TCP::ANG_L, 1 );

        if( ModelSolderMask )
        {
            m_coupledMicrostripCalc.SetParameter( TCP::SOLDERMASK_PRESENT, 1.0 );
            m_coupledMicrostripCalc.SetParameter( TCP::SOLDERMASK_THICKNESS, boardParameters.SolderMaskThickness );
            m_coupledMicrostripCalc.SetParameter( TCP::SOLDERMASK_EPSILONR, boardParameters.SolderMaskDielectricConstant );
            m_coupledMicrostripCalc.SetParameter( TCP::SOLDERMASK_TAND, boardParameters.SolderMaskLossTangent );
        }

        switch( aCalculationType )
        {
        case CalculationType::WIDTH: m_coupledMicrostripCalc.Synthesize( SYNTHESIZE_OPTS::FIX_SPACING ); break;
        case CalculationType::GAP: m_coupledMicrostripCalc.Synthesize( SYNTHESIZE_OPTS::FIX_WIDTH ); break;
        case CalculationType::DELAY: m_coupledMicrostripCalc.Analyse(); break;
        }

        auto& results = aCalculationType == CalculationType::DELAY ? m_coupledMicrostripCalc.GetAnalysisResults() : m_coupledMicrostripCalc.GetSynthesisResults();
        return diffResults( results );
    }

    // calculateDifferentialStripline
    CALCULATION_RESULT calculateDifferentialStripline( CalculationType aCalculationType ) const
    {
        COUPLED_STRIPLINE m_coupledStriplineCalc;
        const double      targetZ = getTargetImpedance();

        if( targetZ <= 0 )
            return CALCULATION_RESULT::err( "Target impedance must be greater than 0" );

        auto [boardParameters, result] = getStriplineBoardParameters();

        if( !result.OK )
            return result;

        double width = 0.0;
        double gap = 0.0;

        if( auto bad = diffGeometry( aCalculationType, width, gap ) )
            return *bad;

        m_coupledStriplineCalc.SetParameter( TCP::Z0_E, targetZ / 2.0 );
        m_coupledStriplineCalc.SetParameter( TCP::Z0_O, targetZ / 2.0 );
        m_coupledStriplineCalc.SetParameter( TCP::Z_DIFF, targetZ );
        m_coupledStriplineCalc.SetParameter( TCP::PHYS_WIDTH, width );
        m_coupledStriplineCalc.SetParameter( TCP::PHYS_S, gap );
        m_coupledStriplineCalc.SetParameter( TCP::T, boardParameters.SignalLayerThickness );

        const double totalH = boardParameters.TopDielectricLayerThickness + boardParameters.SignalLayerThickness
                              + boardParameters.BottomDielectricLayerThickness;
        const double striplineA = boardParameters.TopDielectricLayerThickness + 0.5 * boardParameters.SignalLayerThickness;

        m_coupledStriplineCalc.SetParameter( TCP::H, totalH );

        if( COUPLED_STRIPLINE::IsCenteredOffset( striplineA, totalH ) )
            m_coupledStriplineCalc.SetParameter( TCP::STRIPLINE_A, 0.0 );
        else
            m_coupledStriplineCalc.SetParameter( TCP::STRIPLINE_A, striplineA );

        m_coupledStriplineCalc.SetParameter( TCP::EPSILONR, boardParameters.DielectricConstant );
        m_coupledStriplineCalc.SetParameter( TCP::SKIN_DEPTH, skinDepth( 1.0, 1.0, 1.0 / RHO ) );
        m_coupledStriplineCalc.SetParameter( TCP::PHYS_LEN, 1.0 );
        m_coupledStriplineCalc.SetParameter( TCP::FREQUENCY, Frequency );
        m_coupledStriplineCalc.SetParameter( TCP::ANG_L, 1.0 );
        m_coupledStriplineCalc.SetParameter( TCP::SIGMA, 1.0 / RHO );
        m_coupledStriplineCalc.SetParameter( TCP::MURC, 1 );

        switch( aCalculationType )
        {
        case CalculationType::WIDTH: m_coupledStriplineCalc.Synthesize( SYNTHESIZE_OPTS::FIX_SPACING ); break;
        case CalculationType::GAP: m_coupledStriplineCalc.Synthesize( SYNTHESIZE_OPTS::FIX_WIDTH ); break;
        case CalculationType::DELAY: m_coupledStriplineCalc.Analyse(); break;
        }

        auto& results = aCalculationType == CalculationType::DELAY ? m_coupledStriplineCalc.GetAnalysisResults() : m_coupledStriplineCalc.GetSynthesisResults();
        return diffResults( results );
    }

    // calculateTrackParametersForCell: "none" (KiCad does nothing), "error" (DisplayErrorMessage's text) or the cells it writes
    std::string calculateTrackParametersForCell( CalculationType calculationType ) const
    {
        if( !contains( SignalLayer ) )
            return "{\"kind\": \"none\"}";

        const int  signalLayer = layerIdOf( SignalLayer );
        const bool isMicrostrip = signalLayer == F_Cu || signalLayer == B_Cu; // IsFrontLayer || IsBackLayer of a copper layer
        auto       error = []( const std::string& aMsg ) { return "{\"kind\": \"error\", \"message\": " + str( aMsg ) + "}"; };

        CALCULATION_RESULT result;

        if( Differential )
            result = isMicrostrip ? calculateDifferentialMicrostrip( calculationType ) : calculateDifferentialStripline( calculationType );
        else
            result = isMicrostrip ? calculateSingleMicrostrip( calculationType ) : calculateSingleStripline( calculationType );

        if( !result.OK )
            return error( "Error: " + result.ErrorMsg );

        if( result.Width <= 0 )
            return error( "Could not compute track width" );

        if( Differential && result.DiffPairGap <= 0 )
            return error( "Could not compute differential pair gap" );

        if( result.Delay <= 0 )
            return error( "Could not compute track propagation delay" );

        std::string out = "{\"kind\": \"ok\"";

        if( calculationType == CalculationType::WIDTH )
            out += ", \"width_nm\": " + std::to_string( result.Width );
        else if( Differential && calculationType == CalculationType::GAP )
            out += ", \"diff_pair_gap_nm\": " + std::to_string( result.DiffPairGap );

        return out + ", \"delay_as_per_mm\": " + std::to_string( result.Delay ) + "}";
    }
};

struct STACKUP_CASE
{
    std::string               Name;
    std::vector<STACKUP_ITEM> Items;
};

static STACKUP_ITEM copper( const std::string& n, int t ) { return { n, BS_ITEM_TYPE_COPPER, { { t, 1.0, 0.0 } } }; }
static STACKUP_ITEM mask( const std::string& n, int t, double er, double tand ) { return { n, BS_ITEM_TYPE_SOLDERMASK, { { t, er, tand } } }; }
static STACKUP_ITEM silk( const std::string& n ) { return { n, BS_ITEM_TYPE_SILKSCREEN, { { 0, 1.0, 0.0 } } }; }
static STACKUP_ITEM diel( const std::string& n, std::vector<SUBLAYER> subs ) { return { n, BS_ITEM_TYPE_DIELECTRIC, std::move( subs ) }; }

static std::vector<STACKUP_CASE> stackups()
{
    return {
        { "4-layer", { silk( "F.SilkS" ), mask( "F.Mask", 10000, 3.3, 0.0 ), copper( "F.Cu", 35000 ), diel( "dielectric 1", { { 200000, 4.5, 0.02 } } ),
                       copper( "In1.Cu", 17500 ), diel( "dielectric 2", { { 1065000, 4.5, 0.02 } } ), copper( "In2.Cu", 17500 ),
                       diel( "dielectric 3", { { 200000, 4.5, 0.02 } } ), copper( "B.Cu", 35000 ), mask( "B.Mask", 10000, 3.3, 0.0 ), silk( "B.SilkS" ) } },
        // Sublayers of mixed materials, an offset inner layer, a mask of its own
        { "6-layer mixed", { mask( "F.Mask", 20000, 3.8, 0.025 ), copper( "F.Cu", 35000 ),
                             diel( "dielectric 1", { { 100000, 3.66, 0.004 }, { 50000, 4.2, 0.015 } } ), copper( "In1.Cu", 17500 ),
                             diel( "dielectric 2", { { 300000, 4.4, 0.018 } } ), copper( "In2.Cu", 17500 ), diel( "dielectric 3", { { 500000, 4.5, 0.02 } } ),
                             copper( "In3.Cu", 17500 ), diel( "dielectric 4", { { 150000, 3.5, 0.005 } } ), copper( "In4.Cu", 17500 ),
                             diel( "dielectric 5", { { 120000, 3.66, 0.004 }, { 80000, 3.9, 0.01 } } ), copper( "B.Cu", 70000 ),
                             mask( "B.Mask", 15000, 3.3, 0.02 ) } },
        // A bare copper layer and a zero-thickness dielectric, for the error paths
        { "broken", { copper( "F.Cu", 0 ), diel( "dielectric 1", { { 0, 4.5, 0.02 } } ), copper( "In1.Cu", 35000 ), diel( "dielectric 2", { { 0, 4.5, 0.02 } } ),
                      copper( "In2.Cu", 35000 ), diel( "dielectric 3", { { 200000, 4.5, 0.02 } } ), copper( "In3.Cu", 35000 ),
                      diel( "dielectric 4", { { 0, 4.5, 0.02 } } ), copper( "B.Cu", 35000 ) } },
    };
}

struct PANEL_CASE
{
    std::string        Name;
    int                Stackup;
    std::vector<std::string> Copper;
    bool               Differential;
    std::string        TargetZ;
    double             Frequency;
    bool               Mask;
    std::string        Signal, Top, Bottom;
    std::optional<int> Width, Gap;
    CalculationType    Column;
};

static std::vector<PANEL_CASE> panelCases()
{
    const std::vector<std::string> cu4 = { "F.Cu", "In1.Cu", "In2.Cu", "B.Cu" };
    const std::vector<std::string> cu5 = { "F.Cu", "In1.Cu", "In2.Cu", "In3.Cu", "B.Cu" };
    const std::vector<std::string> cu6 = { "F.Cu", "In1.Cu", "In2.Cu", "In3.Cu", "In4.Cu", "B.Cu" };
    const auto                     W = CalculationType::WIDTH, G = CalculationType::GAP, D = CalculationType::DELAY;
    const std::optional<int>       none;

    return {
        // single, microstrip
        { "single F.Cu width", 0, cu4, false, "50", 1e9, true, "F.Cu", "", "In1.Cu", none, none, W },
        { "single F.Cu width, no mask model", 0, cu4, false, "50", 1e9, false, "F.Cu", "", "In1.Cu", none, none, W },
        { "single F.Cu delay", 0, cu4, false, "50", 1e9, true, "F.Cu", "", "In1.Cu", 350000, none, D },
        { "single B.Cu width 5 GHz", 0, cu4, false, "45", 5e9, true, "B.Cu", "In2.Cu", "", none, none, W },
        { "single F.Cu width to In2", 0, cu4, false, "60", 1e8, true, "F.Cu", "", "In2.Cu", none, none, W },
        { "single F.Cu width, sublayers", 1, cu6, false, "50", 1e9, true, "F.Cu", "", "In1.Cu", none, none, W },
        { "single B.Cu delay, 70 um", 1, cu6, false, "50", 2e9, true, "B.Cu", "In4.Cu", "", 300000, none, D },
        // single, stripline
        { "single In1 width", 0, cu4, false, "50", 1e9, true, "In1.Cu", "F.Cu", "In2.Cu", none, none, W },
        { "single In1 delay", 0, cu4, false, "50", 1e9, true, "In1.Cu", "F.Cu", "In2.Cu", 120000, none, D },
        { "single In2 width, offset", 1, cu6, false, "50", 1e9, true, "In2.Cu", "In1.Cu", "In3.Cu", none, none, W },
        { "single In4 width, sublayers", 1, cu6, false, "40", 5e9, true, "In4.Cu", "In3.Cu", "B.Cu", none, none, W },
        { "single In2 width across a layer", 1, cu6, false, "55", 1e9, true, "In2.Cu", "F.Cu", "In4.Cu", none, none, W },
        // differential, microstrip
        { "diff F.Cu width", 0, cu4, true, "100", 1e9, true, "F.Cu", "", "In1.Cu", none, 200000, W },
        { "diff F.Cu gap", 0, cu4, true, "100", 1e9, true, "F.Cu", "", "In1.Cu", 150000, none, G },
        { "diff F.Cu delay", 0, cu4, true, "100", 1e9, true, "F.Cu", "", "In1.Cu", 150000, 200000, D },
        { "diff B.Cu width, sublayers", 1, cu6, true, "90", 2e9, true, "B.Cu", "In4.Cu", "", none, 127000, W },
        { "diff F.Cu width, no mask model", 1, cu6, true, "100", 1e9, false, "F.Cu", "", "In1.Cu", none, 150000, W },
        // differential, stripline
        { "diff In1 width", 0, cu4, true, "100", 1e9, true, "In1.Cu", "F.Cu", "In2.Cu", none, 200000, W },
        { "diff In1 gap", 0, cu4, true, "100", 1e9, true, "In1.Cu", "F.Cu", "In2.Cu", 100000, none, G },
        { "diff In1 delay", 0, cu4, true, "100", 1e9, true, "In1.Cu", "F.Cu", "In2.Cu", 100000, 200000, D },
        { "diff In2 width, offset", 1, cu6, true, "100", 1e9, true, "In2.Cu", "In1.Cu", "In3.Cu", none, 150000, W },
        { "diff In4 gap, sublayers", 1, cu6, true, "85", 5e9, true, "In4.Cu", "In3.Cu", "B.Cu", 120000, none, G },
        // the messages
        { "no impedance", 0, cu4, false, "0", 1e9, true, "F.Cu", "", "In1.Cu", none, none, W },
        { "impedance not a number", 0, cu4, false, "fifty", 1e9, true, "F.Cu", "", "In1.Cu", none, none, W },
        { "signal layer not a board layer", 0, cu4, false, "50", 1e9, true, "In5.Cu", "", "In1.Cu", none, none, W },
        { "microstrip reference none", 0, cu4, false, "50", 1e9, true, "F.Cu", "", "", none, none, W },
        { "microstrip reference is the signal", 0, cu4, false, "50", 1e9, true, "F.Cu", "", "F.Cu", none, none, W },
        { "B.Cu microstrip reads the top reference", 0, cu4, false, "50", 1e9, true, "B.Cu", "", "In2.Cu", none, none, W },
        { "stripline no top reference", 0, cu4, false, "50", 1e9, true, "In1.Cu", "", "In2.Cu", none, none, W },
        { "stripline top reference below", 0, cu4, false, "50", 1e9, true, "In1.Cu", "In2.Cu", "B.Cu", none, none, W },
        { "stripline no bottom reference", 0, cu4, false, "50", 1e9, true, "In1.Cu", "F.Cu", "", none, none, W },
        { "stripline bottom reference above", 0, cu4, false, "50", 1e9, true, "In2.Cu", "In1.Cu", "F.Cu", none, none, W },
        { "signal copper of no thickness", 2, cu5, false, "50", 1e9, true, "F.Cu", "", "In1.Cu", none, none, W },
        { "stripline dielectrics of no thickness", 2, cu5, false, "50", 1e9, true, "In1.Cu", "F.Cu", "In2.Cu", none, none, W },
        { "stripline dielectric of no thickness above", 2, cu5, false, "50", 1e9, true, "In2.Cu", "In1.Cu", "In3.Cu", none, none, W },
        { "microstrip dielectric of no thickness", 2, cu5, false, "50", 1e9, true, "B.Cu", "In3.Cu", "", none, none, W },
        { "stripline top reference is the signal", 0, cu4, false, "50", 1e9, true, "In2.Cu", "In2.Cu", "B.Cu", none, none, W },
        { "single delay of no width", 0, cu4, false, "50", 1e9, true, "F.Cu", "", "In1.Cu", none, none, D },
        { "diff width with no gap", 0, cu4, true, "100", 1e9, true, "F.Cu", "", "In1.Cu", none, none, W },
        { "diff gap with no width", 0, cu4, true, "100", 1e9, true, "In1.Cu", "F.Cu", "In2.Cu", none, 100000, G },
        { "diff delay with no gap", 0, cu4, true, "100", 1e9, true, "F.Cu", "", "In1.Cu", 100000, 0, D },
        { "diff impedance out of reach", 0, cu4, true, "400", 1e9, true, "In1.Cu", "F.Cu", "In2.Cu", none, 200000, W },
        { "single impedance out of reach", 0, cu4, false, "5", 1e9, true, "In1.Cu", "F.Cu", "In2.Cu", none, none, W },
    };
}

static const char* columnName( CalculationType c )
{
    return c == CalculationType::WIDTH ? "width" : c == CalculationType::GAP ? "gap" : "delay";
}

// ---------------------------------------------------------------------------------------------------- "calculator" cases
//
// The Calculator Tools' Transmission Lines panel (pcb_calculator/calculator_panels/panel_transline.cpp, transline_dlg_funct.cpp, transline_ident.cpp and
// the TRANSLINE wrappers in pcb_calculator/transline/*.cpp). Those need wx, so what they do between the text boxes and the calculator is transcribed:
// the parameters of each line type as transline_ident.cpp lists them (keyword, default in the user unit, scale of the default unit), read as
// TransfDlgDataToTranslineParams does (text * GetUnitScale()), TRANSLINE::getProperties (a parameter the type does not list reads 1.0, SIGMA = 1 / Rho,
// EPSILON_EFF = 1, the skin depth), the wrapper's getProperties (which parameters reach the calculator, the synthesis target from the radio buttons), and
// Analyse or Synthesize( DEFAULT ). The mask thickness is typed in micrometres, as on the panel. Each case gives the values typed (user units), the calculator's analysis and synthesis maps after the operation
// (the wrappers read either), the coax / waveguide mode strings and Synthesize's return.

namespace CALC
{
constexpr double MM = 1e-3, GHZ = 1e9;

struct PRM
{
    const char* Id;      // PRMS_ID without _PRM
    const char* Key;     // m_KeyWord
    double      Default; // m_DefaultValue, in the default unit
    double      Scale;   // GetUnitScale() of the default unit; 1 without a unit selector
};

// transline_ident.cpp, in its order (the four common parameters first)
static std::vector<PRM> prmsOf( int aType )
{
    std::vector<PRM> v = { { "EPSILONR", "Er", 4.5, 1 }, { "TAND", "TanD", 2e-2, 1 }, { "RHO", "Rho", 1.72e-8, 1 }, { "FREQUENCY", "Frequency", 1.0, GHZ } };
    auto add = [&]( std::initializer_list<PRM> l ) { v.insert( v.end(), l ); };

    switch( aType )
    {
    case 0: add( { { "H", "H", 0.2, MM }, { "H_T", "H_t", 1e20, MM }, { "T", "T", 0.035, MM }, { "ROUGH", "Rough", 0.0, MM }, { "MUR", "mu Rel S", 1, 1 },
                   { "MURC", "mu Rel C", 1, 1 }, { "PHYS_WIDTH", "W", 0.2, MM }, { "PHYS_LEN", "L", 50.0, MM }, { "Z0", "Z0", 50.0, 1 }, { "DUMMY", "", 0, 1 },
                   { "ANG_L", "Ang_l", 0.0, 1 } } ); break;
    case 1: add( { { "H", "H", 0.2, MM }, { "H_T", "H_t", 1e20, MM }, { "T", "T", 0.035, MM }, { "ROUGH", "Rough", 0.0, MM }, { "MURC", "mu rel C", 1, 1 },
                   { "PHYS_WIDTH", "W", 0.2, MM }, { "PHYS_S", "S", 0.2, MM }, { "PHYS_LEN", "L", 50.0, MM }, { "Z0_E", "Zeven", 50.0, 1 },
                   { "Z0_O", "Zodd", 50.0, 1 }, { "ANG_L", "Ang_l", 0.0, 1 } } ); break;
    case 2: add( { { "H", "H", 0.2, MM }, { "STRIPLINE_A", "a", 0.2, MM }, { "T", "T", 0.035, MM }, { "MURC", "mu Rel C", 1, 1 }, { "PHYS_WIDTH", "W", 0.2, MM },
                   { "PHYS_LEN", "L", 50.0, MM }, { "Z0", "Z0", 50, 1 }, { "DUMMY", "", 0, 1 }, { "ANG_L", "Ang_l", 0, 1 } } ); break;
    case 3: add( { { "H", "H", 0.2, MM }, { "STRIPLINE_A", "a", 0.0, MM }, { "T", "T", 0.035, MM }, { "MURC", "mu rel C", 1, 1 }, { "PHYS_WIDTH", "W", 0.2, MM },
                   { "PHYS_S", "S", 0.2, MM }, { "PHYS_LEN", "L", 50.0, MM }, { "Z0_E", "Zeven", 50.0, 1 }, { "Z0_O", "Zodd", 50.0, 1 },
                   { "ANG_L", "Ang_l", 0.0, 1 } } ); break;
    case 4:
    case 5: add( { { "H", "H", 0.2, MM }, { "T", "T", 0.035, MM }, { "MURC", "mu Rel C", 1, 1 }, { "PHYS_WIDTH", "W", 0.2, MM }, { "PHYS_S", "S", 0.2, MM },
                   { "PHYS_LEN", "L", 50.0, MM }, { "Z0", "Z0", 50.0, 1 }, { "DUMMY", "", 0, 1 }, { "ANG_L", "Ang_l", 0.0, 1 } } ); break;
    case 6: add( { { "MUR", "mu Rel I", 1, 1 }, { "MURC", "mu Rel C", 1, 1 }, { "PHYS_WIDTH", "a", 10.0, MM }, { "PHYS_S", "b", 5.0, MM },
                   { "PHYS_LEN", "L", 50.0, MM }, { "Z0", "Z0", 50.0, 1 }, { "DUMMY", "", 0, 1 }, { "ANG_L", "Ang_l", 0, 1 } } ); break;
    case 7: add( { { "MUR", "mu Rel I", 1, 1 }, { "MURC", "mu Rel C", 1, 1 }, { "PHYS_DIAM_IN", "Din", 1.0, MM }, { "PHYS_DIAM_OUT", "Dout", 8.0, MM },
                   { "PHYS_LEN", "L", 50.0, MM }, { "Z0", "Z0", 50.0, 1 }, { "DUMMY", "", 0, 1 }, { "ANG_L", "Ang_l", 0.0, 1 } } ); break;
    case 8: add( { { "TWISTEDPAIR_TWIST", "Twists", 0.0, 1 }, { "MURC", "mu Rel C", 1, 1 }, { "TWISTEDPAIR_EPSILONR_ENV", "ErEnv", 1, 1 },
                   { "PHYS_DIAM_IN", "Din", 1.0, MM }, { "PHYS_DIAM_OUT", "Dout", 8.0, MM }, { "PHYS_LEN", "L", 50.0, MM }, { "Z0", "Z0", 50.0, 1 },
                   { "DUMMY", "", 0, 1 }, { "ANG_L", "Ang_l", 0.0, 1 } } ); break;
    }

    return v;
}

static const char* const TYPE_NAMES[] = { "MicroStrip", "Coupled_MicroStrip", "StripLine", "Coupled_Stripline", "CoPlanar", "GrCoPlanar", "RectWaveGuide", "Coax", "TwistedPair" };

struct CASE
{
    std::string                   Name;
    int                           Type;
    bool                          Synthesize;
    int                           Radio = 1; // m_radioBtnPrm1 (default) or 2
    std::map<std::string, double> Typed;     // keyword -> value typed (user unit); the default otherwise
    double                        Model = 0, SpecFreq = 1e9, MaskPresent = 0, MaskTUm = 20, MaskEr = 3.5, MaskTand = 0.025, Fills = 1;
};

static std::string run( const CASE& c )
{
    std::map<std::string, double> P; // TRANSLINE::m_parameters by PRMS_ID name
    const char* const ids[] = { "EPSILONR", "TAND", "RHO", "H", "TWISTEDPAIR_TWIST", "H_T", "STRIPLINE_A", "T", "ROUGH", "MUR", "TWISTEDPAIR_EPSILONR_ENV",
                                "MURC", "FREQUENCY", "Z0", "Z0_E", "Z0_O", "ANG_L", "PHYS_WIDTH", "PHYS_DIAM_IN", "PHYS_S", "PHYS_DIAM_OUT", "PHYS_LEN" };

    std::map<std::string, double> normalized;

    for( const PRM& p : prmsOf( c.Type ) )
    {
        const auto   it = c.Typed.find( p.Key );
        const double typed = it != c.Typed.end() ? it->second : p.Default;
        normalized[p.Id] = typed * p.Scale;
    }

    for( const char* id : ids )
        P[id] = normalized.count( id ) ? normalized[id] : 1.0; // PANEL_TRANSLINE::GetPrmValue

    const double SIGMA = 1.0 / P["RHO"];
    const double SKIN = 1.0 / sqrt( M_PI * P["FREQUENCY"] * P["MURC"] * TRANSLINE_CALCULATIONS::MU0 * SIGMA );

    MICROSTRIP ms; COUPLED_MICROSTRIP cms; STRIPLINE sl; COUPLED_STRIPLINE csl; COPLANAR cpw; RECTWAVEGUIDE rw; COAX cx; TWISTEDPAIR tp;
    TRANSLINE_CALCULATION_BASE* calcs[] = { &ms, &cms, &sl, &csl, &cpw, &cpw, &rw, &cx, &tp };
    TRANSLINE_CALCULATION_BASE& k = *calcs[c.Type];
    auto set = [&]( TCP t, double v ) { k.SetParameter( t, v ); };
    auto mask = [&]( bool fills ) {
        set( TCP::SOLDERMASK_PRESENT, c.MaskPresent ); set( TCP::SOLDERMASK_THICKNESS, c.MaskTUm * 1.0e-6 ); set( TCP::SOLDERMASK_EPSILONR, c.MaskEr );
        set( TCP::SOLDERMASK_TAND, c.MaskTand );
        if( fills ) set( TCP::SOLDERMASK_FILLS_GAPS, c.Fills );
    };

    switch( c.Type )
    {
    case 0: // MICROSTRIP_UI::getProperties
        set( TCP::SIGMA, SIGMA ); set( TCP::EPSILON_EFF, 1.0 ); set( TCP::SKIN_DEPTH, SKIN ); set( TCP::EPSILONR, P["EPSILONR"] );
        set( TCP::DIELECTRIC_MODEL_SEL, c.Model ); set( TCP::EPSILONR_SPEC_FREQ, c.SpecFreq ); mask( false );
        set( TCP::H_T, P["H_T"] ); set( TCP::H, P["H"] ); set( TCP::PHYS_WIDTH, P["PHYS_WIDTH"] ); set( TCP::T, P["T"] ); set( TCP::Z0, P["Z0"] );
        set( TCP::FREQUENCY, P["FREQUENCY"] ); set( TCP::ROUGH, P["ROUGH"] ); set( TCP::TAND, P["TAND"] ); set( TCP::PHYS_LEN, P["PHYS_LEN"] );
        set( TCP::MUR, P["MUR"] ); set( TCP::MURC, P["MURC"] ); set( TCP::ANG_L, P["ANG_L"] );
        break;
    case 1: // C_MICROSTRIP::getProperties
        set( TCP::Z0_E, P["Z0_E"] ); set( TCP::Z0_O, P["Z0_O"] ); set( TCP::EPSILONR, P["EPSILONR"] ); set( TCP::DIELECTRIC_MODEL_SEL, c.Model );
        set( TCP::EPSILONR_SPEC_FREQ, c.SpecFreq ); mask( false ); set( TCP::PHYS_WIDTH, P["PHYS_WIDTH"] ); set( TCP::PHYS_LEN, P["PHYS_LEN"] );
        set( TCP::PHYS_S, P["PHYS_S"] ); set( TCP::H, P["H"] ); set( TCP::T, P["T"] ); set( TCP::H_T, P["H_T"] ); set( TCP::FREQUENCY, P["FREQUENCY"] );
        set( TCP::MURC, P["MURC"] ); set( TCP::SKIN_DEPTH, SKIN ); set( TCP::SIGMA, SIGMA ); set( TCP::ROUGH, P["ROUGH"] ); set( TCP::TAND, P["TAND"] );
        set( TCP::ANG_L, P["ANG_L"] );
        break;
    case 2: // STRIPLINE_UI::getProperties
        set( TCP::SKIN_DEPTH, SKIN ); set( TCP::EPSILONR, P["EPSILONR"] ); set( TCP::DIELECTRIC_MODEL_SEL, c.Model ); set( TCP::EPSILONR_SPEC_FREQ, c.SpecFreq );
        set( TCP::T, P["T"] ); set( TCP::STRIPLINE_A, P["STRIPLINE_A"] ); set( TCP::H, P["H"] ); set( TCP::Z0, P["Z0"] ); set( TCP::PHYS_LEN, P["PHYS_LEN"] );
        set( TCP::FREQUENCY, P["FREQUENCY"] ); set( TCP::TAND, P["TAND"] ); set( TCP::PHYS_WIDTH, P["PHYS_WIDTH"] ); set( TCP::ANG_L, P["ANG_L"] );
        set( TCP::SIGMA, SIGMA ); set( TCP::MURC, P["MURC"] );
        break;
    case 3: // C_STRIPLINE::getProperties
        set( TCP::Z0_E, P["Z0_E"] ); set( TCP::Z0_O, P["Z0_O"] ); set( TCP::EPSILONR, P["EPSILONR"] ); set( TCP::DIELECTRIC_MODEL_SEL, c.Model );
        set( TCP::EPSILONR_SPEC_FREQ, c.SpecFreq ); set( TCP::PHYS_WIDTH, P["PHYS_WIDTH"] ); set( TCP::PHYS_LEN, P["PHYS_LEN"] ); set( TCP::PHYS_S, P["PHYS_S"] );
        set( TCP::H, P["H"] ); set( TCP::STRIPLINE_A, P["STRIPLINE_A"] ); set( TCP::T, P["T"] ); set( TCP::FREQUENCY, P["FREQUENCY"] ); set( TCP::MUR, P["MUR"] );
        set( TCP::MURC, P["MURC"] ); set( TCP::SKIN_DEPTH, SKIN ); set( TCP::SIGMA, SIGMA ); set( TCP::TAND, P["TAND"] ); set( TCP::ROUGH, P["ROUGH"] );
        set( TCP::ANG_L, P["ANG_L"] );
        break;
    case 4:
    case 5: // COPLANAR_UI / GROUNDEDCOPLANAR_UI::getProperties
        set( TCP::EPSILONR, P["EPSILONR"] ); set( TCP::DIELECTRIC_MODEL_SEL, c.Model ); set( TCP::EPSILONR_SPEC_FREQ, c.SpecFreq ); mask( true );
        set( TCP::TAND, P["TAND"] ); set( TCP::SIGMA, SIGMA ); set( TCP::MURC, P["MURC"] ); set( TCP::PHYS_WIDTH, P["PHYS_WIDTH"] ); set( TCP::PHYS_S, P["PHYS_S"] );
        set( TCP::H, P["H"] ); set( TCP::T, P["T"] ); set( TCP::PHYS_LEN, P["PHYS_LEN"] ); set( TCP::FREQUENCY, P["FREQUENCY"] ); set( TCP::Z0, P["Z0"] );
        set( TCP::ANG_L, P["ANG_L"] ); set( TCP::SKIN_DEPTH, SKIN ); set( TCP::CPW_BACKMETAL, c.Type == 5 ? 1.0 : 0.0 );
        k.SetSynthesizeTarget( c.Radio == 1 ? TCP::PHYS_WIDTH : TCP::PHYS_S );
        break;
    case 6: // RECTWAVEGUIDE_UI::getProperties
        set( TCP::EPSILONR, P["EPSILONR"] ); set( TCP::DIELECTRIC_MODEL_SEL, c.Model ); set( TCP::EPSILONR_SPEC_FREQ, c.SpecFreq ); set( TCP::TAND, P["TAND"] );
        set( TCP::SIGMA, SIGMA ); set( TCP::MUR, P["MUR"] ); set( TCP::MURC, P["MURC"] ); set( TCP::PHYS_WIDTH, P["PHYS_WIDTH"] ); set( TCP::PHYS_S, P["PHYS_S"] );
        set( TCP::PHYS_LEN, P["PHYS_LEN"] ); set( TCP::FREQUENCY, P["FREQUENCY"] ); set( TCP::Z0, P["Z0"] ); set( TCP::ANG_L, P["ANG_L"] );
        if( c.Radio == 1 ) k.SetSynthesizeTarget( TCP::PHYS_WIDTH );
        break;
    case 7: // COAX_UI::getProperties
        set( TCP::SIGMA, SIGMA ); set( TCP::SKIN_DEPTH, SKIN ); set( TCP::EPSILONR, P["EPSILONR"] ); set( TCP::DIELECTRIC_MODEL_SEL, c.Model );
        set( TCP::EPSILONR_SPEC_FREQ, c.SpecFreq ); set( TCP::TAND, P["TAND"] ); set( TCP::PHYS_DIAM_IN, P["PHYS_DIAM_IN"] ); set( TCP::PHYS_DIAM_OUT, P["PHYS_DIAM_OUT"] );
        set( TCP::PHYS_LEN, P["PHYS_LEN"] ); set( TCP::MUR, P["MUR"] ); set( TCP::MURC, P["MURC"] ); set( TCP::FREQUENCY, P["FREQUENCY"] ); set( TCP::Z0, P["Z0"] );
        set( TCP::ANG_L, P["ANG_L"] );
        k.SetSynthesizeTarget( c.Radio == 1 ? TCP::PHYS_DIAM_IN : TCP::PHYS_DIAM_OUT );
        break;
    case 8: // TWISTEDPAIR_UI::getProperties
        set( TCP::EPSILONR, P["EPSILONR"] ); set( TCP::DIELECTRIC_MODEL_SEL, c.Model ); set( TCP::EPSILONR_SPEC_FREQ, c.SpecFreq ); set( TCP::TAND, P["TAND"] );
        set( TCP::SIGMA, SIGMA ); set( TCP::MURC, P["MURC"] ); set( TCP::PHYS_DIAM_IN, P["PHYS_DIAM_IN"] ); set( TCP::PHYS_DIAM_OUT, P["PHYS_DIAM_OUT"] );
        set( TCP::PHYS_LEN, P["PHYS_LEN"] ); set( TCP::FREQUENCY, P["FREQUENCY"] ); set( TCP::Z0, P["Z0"] ); set( TCP::ANG_L, P["ANG_L"] );
        set( TCP::TWISTEDPAIR_TWIST, P["TWISTEDPAIR_TWIST"] ); set( TCP::TWISTEDPAIR_EPSILONR_ENV, P["TWISTEDPAIR_EPSILONR_ENV"] );
        k.SetSynthesizeTarget( c.Radio == 1 ? TCP::PHYS_DIAM_IN : TCP::PHYS_DIAM_OUT );
        break;
    }

    bool converged = false;

    if( c.Synthesize )
        converged = k.Synthesize( SYNTHESIZE_OPTS::DEFAULT );
    else
        k.Analyse();

    const std::string synthesis = resultsJson( k.GetSynthesisResults() );
    const std::string analysis = resultsJson( k.GetAnalysisResults() );
    std::string te, tm;

    if( c.Type == 6 ) { te = rw.GetTEModes(); tm = rw.GetTMModes(); }
    if( c.Type == 7 ) { te = cx.GetTEModes(); tm = cx.GetTMModes(); }

    std::string typed;

    for( const auto& [key, v] : c.Typed )
        typed += ( typed.empty() ? "" : ", " ) + str( key ) + ": " + num( v );

    return "    {\"name\": " + str( c.Name ) + ", \"type\": " + std::to_string( c.Type ) + ", \"tline\": " + str( TYPE_NAMES[c.Type] ) + ", \"op\": "
           + str( c.Synthesize ? "synthesize" : "analyse" ) + ", \"radio\": " + std::to_string( c.Radio ) + ", \"typed\": {" + typed + "}"
           + ",\n     \"model\": " + num( c.Model ) + ", \"spec_freq\": " + num( c.SpecFreq ) + ", \"mask\": [" + num( c.MaskPresent ) + ", " + num( c.MaskTUm ) + ", "
           + num( c.MaskEr ) + ", " + num( c.MaskTand ) + ", " + num( c.Fills ) + "], \"converged\": " + ( converged ? "true" : "false" )
           + ", \"te\": " + str( te ) + ", \"tm\": " + str( tm ) + ",\n     \"analysis\": " + analysis + ",\n     \"synthesis\": " + synthesis + "}";
}

static std::vector<CASE> cases()
{
    std::vector<CASE> v;

    // Every type at the panel's defaults, analysed and synthesized
    for( int t = 0; t < 9; ++t )
    {
        v.push_back( { std::string( TYPE_NAMES[t] ) + " defaults, analyse", t, false } );
        v.push_back( { std::string( TYPE_NAMES[t] ) + " defaults, synthesize", t, true } );
    }

    // The other radio button
    for( int t : { 4, 5, 6, 7, 8 } )
        v.push_back( { std::string( TYPE_NAMES[t] ) + " defaults, synthesize the second parameter", t, true, 2 } );

    // Coupled lines with a real even / odd target (DEFAULT 2-D synthesis), an electrical length, other geometry
    v.push_back( { "Coupled_MicroStrip 60/40 ohm, synthesize", 1, true, 1, { { "Zeven", 60 }, { "Zodd", 40 }, { "Ang_l", 1.5 } } } );
    v.push_back( { "Coupled_MicroStrip 70/35 ohm on RO4350B, synthesize", 1, true, 1, { { "Er", 3.66 }, { "TanD", 0.0037 }, { "H", 0.508 }, { "Zeven", 70 },
                                                                                       { "Zodd", 35 }, { "Ang_l", 0.7 } } } );
    v.push_back( { "Coupled_Stripline 60/40 ohm, synthesize", 3, true, 1, { { "Zeven", 60 }, { "Zodd", 40 } } } );
    v.push_back( { "Coupled_Stripline offset, analyse", 3, false, 1, { { "a", 0.05 }, { "W", 0.15 }, { "S", 0.1 } } } );
    v.push_back( { "Coupled_Stripline offset 55/45 ohm, synthesize", 3, true, 1, { { "a", 0.07 }, { "Zeven", 55 }, { "Zodd", 45 } } } );

    // Geometry and frequency variations of the four new calculators
    v.push_back( { "Coax RG58-like at 10 GHz, analyse", 7, false, 1, { { "Er", 2.25 }, { "TanD", 4e-4 }, { "Din", 0.9 }, { "Dout", 2.95 }, { "Frequency", 10 } } } );
    v.push_back( { "Coax 75 ohm outer diameter, synthesize", 7, true, 2, { { "Er", 2.1 }, { "Z0", 75 }, { "Ang_l", 3.14159 } } } );
    v.push_back( { "Coax Dout <= Din, analyse", 7, false, 1, { { "Din", 3 }, { "Dout", 2 } } } );
    v.push_back( { "RectWaveGuide WR-90 at 10 GHz, analyse", 6, false, 1, { { "Er", 1 }, { "TanD", 0 }, { "a", 22.86 }, { "b", 10.16 }, { "Frequency", 10 } } } );
    v.push_back( { "RectWaveGuide WR-90 at 30 GHz, analyse", 6, false, 1, { { "Er", 1 }, { "TanD", 0 }, { "a", 22.86 }, { "b", 10.16 }, { "Frequency", 30 } } } );
    v.push_back( { "RectWaveGuide WR-90 at 80 GHz (truncated modes), analyse", 6, false, 1, { { "Er", 1 }, { "a", 22.86 }, { "b", 10.16 }, { "Frequency", 80 } } } );
    v.push_back( { "RectWaveGuide 500 ohm at 10 GHz, synthesize", 6, true, 1, { { "Er", 1 }, { "Z0", 500 }, { "Ang_l", 2 }, { "Frequency", 10 } } } );
    v.push_back( { "CoPlanar 1.6 mm FR4 at 5 GHz, analyse", 4, false, 1, { { "H", 1.6 }, { "W", 1.0 }, { "S", 0.15 }, { "Frequency", 5 } } } );
    v.push_back( { "GrCoPlanar 1.6 mm FR4 at 5 GHz, analyse", 5, false, 1, { { "H", 1.6 }, { "W", 1.0 }, { "S", 0.15 }, { "Frequency", 5 } } } );
    v.push_back( { "GrCoPlanar 50 ohm gap, synthesize", 5, true, 2, { { "H", 0.4 }, { "W", 0.6 }, { "Ang_l", 1 } } } );
    v.push_back( { "CoPlanar thin copper, analyse", 4, false, 1, { { "T", 0 }, { "Frequency", 0.1 } } } );
    v.push_back( { "TwistedPair 20 twists/m, analyse", 8, false, 1, { { "Twists", 20 }, { "Er", 3 }, { "Din", 0.5 }, { "Dout", 1.0 }, { "Frequency", 0.01 } } } );
    v.push_back( { "TwistedPair 100 ohm outer diameter, synthesize", 8, true, 2, { { "Twists", 30 }, { "Din", 0.5 }, { "Z0", 100 }, { "Ang_l", 1 } } } );

    // The panel-level controls: Djordjevic-Sarkar dispersion and the soldermask overlay
    CASE ds{ "MicroStrip Djordjevic-Sarkar at 5 GHz spec 1 GHz, analyse", 0, false, 1, { { "Frequency", 5 } } };
    ds.Model = 1;
    v.push_back( ds );
    CASE dsc{ "Coax Djordjevic-Sarkar, synthesize", 7, true, 1, { { "Frequency", 3 } } };
    dsc.Model = 1; dsc.SpecFreq = 2e9;
    v.push_back( dsc );
    CASE mms{ "MicroStrip soldermask, analyse", 0, false };
    mms.MaskPresent = 1;
    v.push_back( mms );
    CASE mcp{ "CoPlanar soldermask filling gaps, analyse", 4, false };
    mcp.MaskPresent = 1;
    v.push_back( mcp );
    CASE mcp2{ "GrCoPlanar soldermask on the traces only, analyse", 5, false };
    mcp2.MaskPresent = 1; mcp2.Fills = 0; mcp2.MaskTUm = 30; mcp2.MaskEr = 3.8;
    v.push_back( mcp2 );
    CASE mcms{ "Coupled_MicroStrip soldermask, analyse", 1, false };
    mcms.MaskPresent = 1;
    v.push_back( mcms );

    return v;
}
} // namespace CALC

int main()
{
    std::string out = "{\n  \"kicad\": \"25ec2335\",\n  \"transline\": [\n";
    const auto  raw = rawCases();

    for( size_t i = 0; i < raw.size(); ++i )
        out += runRaw( raw[i] ) + ( i + 1 < raw.size() ? ",\n" : "\n" );

    out += "  ],\n  \"stackups\": [\n";
    const auto stacks = stackups();

    for( size_t i = 0; i < stacks.size(); ++i )
    {
        out += "    {\"name\": " + str( stacks[i].Name ) + ", \"layers\": [";

        for( size_t j = 0; j < stacks[i].Items.size(); ++j )
        {
            const STACKUP_ITEM& it = stacks[i].Items[j];
            const char*         type = it.Type == BS_ITEM_TYPE_COPPER ? "copper" : it.Type == BS_ITEM_TYPE_DIELECTRIC ? "dielectric"
                                                                          : it.Type == BS_ITEM_TYPE_SOLDERMASK ? "soldermask" : "silkscreen";
            out += std::string( j ? ",\n      " : "\n      " ) + "{\"name\": " + str( it.Name ) + ", \"type\": " + str( type ) + ", \"sublayers\": [";

            for( size_t k = 0; k < it.Subs.size(); ++k )
            {
                out += std::string( k ? ", " : "" ) + "{\"thickness_nm\": " + std::to_string( it.Subs[k].Thickness ) + ", \"epsilon_r\": " + num( it.Subs[k].EpsilonR )
                       + ", \"loss_tangent\": " + num( it.Subs[k].LossTangent ) + "}";
            }

            out += "]}";
        }

        out += "]}" + std::string( i + 1 < stacks.size() ? ",\n" : "\n" );
    }

    out += "  ],\n  \"panel\": [\n";
    const auto cases = panelCases();

    for( size_t i = 0; i < cases.size(); ++i )
    {
        const PANEL_CASE& c = cases[i];
        PANEL             p;
        p.Stackup = stacks[c.Stackup].Items;
        p.LayerNames = c.Copper;
        p.Differential = c.Differential;
        p.TargetImpedance = c.TargetZ;
        p.Frequency = c.Frequency;
        p.ModelSolderMask = c.Mask;
        p.SignalLayer = c.Signal;
        p.TopReference = c.Top;
        p.BottomReference = c.Bottom;
        p.Width = c.Width;
        p.Gap = c.Gap;

        std::string copperList;

        for( size_t k = 0; k < c.Copper.size(); ++k )
            copperList += ( k ? ", " : "" ) + str( c.Copper[k] );

        auto optInt = []( const std::optional<int>& v ) { return v ? std::to_string( *v ) : std::string( "null" ); };

        out += "    {\"name\": " + str( c.Name ) + ", \"stackup\": " + std::to_string( c.Stackup ) + ", \"copper\": [" + copperList + "], \"differential\": "
               + ( c.Differential ? "true" : "false" ) + ", \"target_impedance\": " + str( c.TargetZ ) + ", \"frequency\": " + num( c.Frequency )
               + ", \"model_solder_mask\": " + ( c.Mask ? "true" : "false" ) + ",\n     \"signal\": " + str( c.Signal ) + ", \"top\": " + str( c.Top )
               + ", \"bottom\": " + str( c.Bottom ) + ", \"width_nm\": " + optInt( c.Width ) + ", \"gap_nm\": " + optInt( c.Gap ) + ", \"column\": "
               + str( columnName( c.Column ) ) + ",\n     \"result\": " + p.calculateTrackParametersForCell( c.Column ) + "}"
               + ( i + 1 < cases.size() ? ",\n" : "\n" );
    }

    out += "  ],\n  \"calculator\": [\n";
    const auto calc = CALC::cases();

    for( size_t i = 0; i < calc.size(); ++i )
        out += CALC::run( calc[i] ) + ( i + 1 < calc.size() ? ",\n" : "\n" );

    out += "  ]\n}\n";
    fputs( out.c_str(), stdout );
    return 0;
}
