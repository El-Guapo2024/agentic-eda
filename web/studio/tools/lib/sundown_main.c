/* KiCad's `ConvertMarkdown2Html` (thirdparty/markdown2html/markdown2html.cpp) as a filter: markdown on stdin, the HTML on stdout.
 * Built by tools/sundown-golden.mjs against KiCad's own sundown sources; the options are KiCad's: MKDEXT_TABLES, a nesting of 16,
 * render flags 0. Not part of the studio. */
#include <stdio.h>
#include "markdown.h"
#include "html.h"
#include "buffer.h"

int main( void )
{
    struct buf* ib = bufnew( 1024 );
    char        chunk[4096];
    size_t      n;

    while( ( n = fread( chunk, 1, sizeof chunk, stdin ) ) > 0 )
        bufput( ib, chunk, n );

    struct sd_callbacks    callbacks;
    struct html_renderopt  options;
    struct buf*            ob = bufnew( 64 );

    sdhtml_renderer( &callbacks, &options, 0 );
    struct sd_markdown* markdown = sd_markdown_new( MKDEXT_TABLES, 16, &callbacks, &options );

    sd_markdown_render( ob, ib->data, ib->size, markdown );
    sd_markdown_free( markdown );

    fwrite( ob->data, 1, ob->size, stdout );
    bufrelease( ob );
    bufrelease( ib );
    return 0;
}
