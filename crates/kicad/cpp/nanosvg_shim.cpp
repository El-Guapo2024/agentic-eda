// A C ABI over KiCad's nanosvg (third_party/nanosvg): parse an SVG text the
// way SVG_IMPORT_PLUGIN::LoadFromMemory does (nsvgParse(text, "mm", 96) on a
// NUL-terminated copy, since nsvgParse writes into its input), hand every
// shape and path to callbacks, and free the image. No C++ exception crosses
// the ABI.

#include "nanosvg.h"

#include <cstddef>
#include <vector>

extern "C" {

typedef void (*ksvg_shape_cb)( void* ud, int fill_type, unsigned int fill_color, int stroke_type, unsigned int stroke_color,
                               float stroke_width, const float* dashes, int ndashes, int fill_rule, int flags );
typedef void (*ksvg_path_cb)( void* ud, const float* pts, int npts, int closed );

// Returns 0, -1 when nanosvg returned no image, -2 on a C++ exception.
int ksvg_parse( const char* text, size_t len, ksvg_shape_cb on_shape, ksvg_path_cb on_path, void* ud )
{
    try
    {
        std::vector<char> buf( text, text + len );
        buf.push_back( '\0' );
        NSVGimage* image = nsvgParse( buf.data(), "mm", 96.0f );

        if( !image )
            return -1;

        for( NSVGshape* shape = image->shapes; shape != nullptr; shape = shape->next )
        {
            on_shape( ud, shape->fill.type, shape->fill.color, shape->stroke.type, shape->stroke.color, shape->strokeWidth,
                      shape->strokeDashArray, shape->strokeDashCount, shape->fillRule, shape->flags );

            for( NSVGpath* path = shape->paths; path != nullptr; path = path->next )
                on_path( ud, path->pts, path->npts, path->closed );
        }

        nsvgDelete( image );
        return 0;
    }
    catch( ... )
    {
        return -2;
    }
}

}
