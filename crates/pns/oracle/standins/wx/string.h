// Just enough of wxString for libs/kimath/include/geometry/shape.h.
#pragma once
#include <string>
#include <cstdarg>
#include <cstdio>
class wxString : public std::string
{
public:
    wxString() {}
    wxString( const char* s ) : std::string( s ) {}
    wxString( const std::string& s ) : std::string( s ) {}
    static wxString FromUTF8( const char* s ) { return wxString( s ); }
    static wxString Format( const char* fmt, ... ) { char b[1024]; va_list a; va_start( a, fmt ); vsnprintf( b, sizeof b, fmt, a ); va_end( a ); return wxString( b ); }
    std::string utf8_string() const { return *this; }
    const char* c_str() const { return std::string::c_str(); }
    const char* ToStdString() const { return std::string::c_str(); }
};
#define wxEmptyString ""
