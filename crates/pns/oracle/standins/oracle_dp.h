// Stand-in for the part of pns_diff_pair.h that CoupledSegmentPairs reads: a pair's two lines, its width and its gap constraint. The
// function texts (commonParallelProjection and CoupledSegmentPairs) are KiCad's, extracted verbatim by build.sh into oracle_dp.cpp.
#pragma once
#include <vector>
#include <geometry/seg.h>
#include <geometry/shape_line_chain.h>
#include "ranged_num.h"

namespace PNS {

struct COUPLED_SEGMENTS
{
    COUPLED_SEGMENTS( const SEG& aCoupledP, const SEG& aParentP, int aIndexP, const SEG& aCoupledN, const SEG& aParentN, int aIndexN ) :
            coupledP( aCoupledP ), coupledN( aCoupledN ), parentP( aParentP ), parentN( aParentN ), indexP( aIndexP ), indexN( aIndexN )
    {
    }

    SEG coupledP;
    SEG coupledN;
    SEG parentP;
    SEG parentN;
    int indexP;
    int indexN;
};

typedef std::vector<COUPLED_SEGMENTS> COUPLED_SEGMENTS_VEC;

class DP_STAND_IN
{
public:
    void CoupledSegmentPairs( COUPLED_SEGMENTS_VEC& aPairs ) const;

    SHAPE_LINE_CHAIN m_p, m_n;
    int              m_width = 0;
    RANGED_NUM<int>  m_gapConstraint;
};

bool commonParallelProjection( SEG p, SEG n, SEG& pClip, SEG& nClip );

}
