// Replaces KiCad's nanosvg_text.h (third_party/nanosvg/text/nanosvg_text.h)
// in the build without Fontconfig, FreeType and HarfBuzz: `text` and `tspan`
// elements draw nothing (no attribute is pushed, so none is popped), and
// their characters are dropped. nanosvg.cpp includes this file by name; it is
// found here because the real one is kept outside nanosvg.cpp's directory.
// Licensed under the zlib licence, as nanosvg.

struct NSVGtext
{
};

static void nsvg__deleteText( NSVGtext* text )
{
    delete text;
}

static void nsvg__startText( NSVGparser*, const char*, const char** )
{
}

static void nsvg__endText( NSVGparser* )
{
}

static void nsvg__textContent( NSVGparser*, const char*, bool )
{
}
