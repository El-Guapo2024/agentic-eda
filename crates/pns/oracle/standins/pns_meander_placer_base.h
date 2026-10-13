#pragma once
// Oracle stand-in for pcbnew/router/pns_meander_placer_base.h: the interface the shapes read off their placer, and the
// functions of pns_meander_placer_base.cpp that tune a meandered line (build.sh extracts those into oracle_tune.cpp, verbatim).
#include "pns_meander.h"

namespace PNS {

class MEANDER_PLACER_BASE
{
public:
    virtual ~MEANDER_PLACER_BASE() {}
    virtual const MEANDER_SETTINGS& MeanderSettings() const { return m_settings; }
    virtual void UpdateSettings( const MEANDER_SETTINGS& aSettings ) { m_settings = aSettings; }
    virtual int Clearance() { return m_clearance; }
    // MEANDER_PLACER::CheckFit: the world (here a list of circles, `m_fit`), then the shape against the meanders of the line so far.
    virtual bool CheckFit( MEANDER_SHAPE* aShape )
    {
        if( m_fit && !m_fit( aShape ) )
            return false;

        if( !m_result )
            return true;

        int w = aShape->Width();
        int clearance = w + m_settings.m_spacing;
        return m_result->CheckSelfIntersections( aShape, clearance );
    }
    void tuneLineLength( MEANDERED_LINE& aTuned, long long int aElongation );
    virtual void AmplitudeStep( int aSign );
    virtual void SpacingStep( int aSign );
    int m_currentWidth = 0;

    MEANDER_SETTINGS m_settings;
    int m_clearance = 0;
    bool ( *m_fit )( MEANDER_SHAPE* ) = nullptr;
    MEANDERED_LINE* m_result = nullptr;
};

}
