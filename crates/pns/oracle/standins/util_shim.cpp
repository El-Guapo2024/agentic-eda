// The two integer `rescale` specialisations of libs/kimath/src/math/util.cpp (the __int128 branch), without its wx logging.
#include <cstdint>
#include <math/util.h>
void kimathLogDebug( const char* aFormatString, ... ) {}
void kimathLogOverflow( double v, const char* aTypeName ) {}
template<>
int rescale( int aNumerator, int aValue, int aDenominator )
{
    int64_t numerator = (int64_t) aNumerator * (int64_t) aValue;
    if( ( numerator < 0 ) ^ ( aDenominator < 0 ) )
        return ( numerator - aDenominator / 2 ) / aDenominator;
    else
        return ( numerator + aDenominator / 2 ) / aDenominator;
}
template<>
int64_t rescale( int64_t aNumerator, int64_t aValue, int64_t aDenominator )
{
    __int128_t numerator = (__int128_t) aNumerator * (__int128_t) aValue;
    if( ( numerator < 0 ) ^ ( aDenominator < 0 ) )
        return ( numerator - aDenominator / 2 ) / aDenominator;
    else
        return ( numerator + aDenominator / 2 ) / aDenominator;
}
