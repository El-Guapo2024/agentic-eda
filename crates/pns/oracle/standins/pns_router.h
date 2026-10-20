// Stand-in for pcbnew/router/pns_router.h. pns_meander.cpp wants nothing of it; pns_mouse_trail_tracer.cpp only asks the router for the
// debug decorator (`ROUTER::GetInstance()->GetInterface()->GetDebugDecorator()`), which here is none.
#pragma once

namespace PNS
{
class DEBUG_DECORATOR;

class ROUTER_IFACE_STAND_IN
{
public:
    DEBUG_DECORATOR* GetDebugDecorator() { return nullptr; }
};

class ROUTER
{
public:
    static ROUTER* GetInstance()
    {
        static ROUTER router;
        return &router;
    }

    ROUTER_IFACE_STAND_IN* GetInterface()
    {
        static ROUTER_IFACE_STAND_IN iface;
        return &iface;
    }
};
} // namespace PNS
