// Stand-in for pcbnew/router/pns_debug_decorator.h: no debug decorator, so PNS_DBG draws nothing (and does not evaluate its arguments).
#pragma once

#define PNS_DBG( dbg, method, ... ) do { (void) (dbg); } while( 0 )
#define PNS_DBGN( dbg, method ) do { (void) (dbg); } while( 0 )
