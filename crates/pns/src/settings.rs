//! Port of `PNS::ROUTING_SETTINGS`/`PNS::SIZES_SETTINGS`
//! (`pcbnew/router/pns_routing_settings.h`, `pns_sizes_settings.h`).
//!
//! KiCad's `SIZES_SETTINGS` is filled in by `PNS_KICAD_IFACE` from the
//! board's net classes right before a routing operation starts -- the
//! numbers themselves (not the defaults baked into `SIZES_SETTINGS`'s own
//! constructor, which only matter before a board is loaded) come from
//! `BOARD_DESIGN_SETTINGS`. This port's equivalent source is
//! `eda_model::BoardRules`, which already has exactly that per-net-class
//! resolution (`width_of`/`clearance_of`/`via_diameter_of`/`via_drill_of`)
//! -- see [`SizesSettings::for_net`].

use eda_model::ir::Um;
use eda_model::BoardRules;

/// `PNS_MODE` (`RM_MarkObstacles`/`RM_Shove`/`RM_Walkaround`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Mode {
    /// Collisions are reported but never resolved -- the line stops (or is
    /// marked violating) at the obstacle. Still routable; just not helpful.
    MarkObstacles,
    /// Push colliding tracks/vias out of the way (`crate::shove`).
    Shove,
    /// Hug around whatever is in the way instead of pushing it
    /// (`crate::walkaround`). KiCad's own default (`ROUTING_SETTINGS`'s
    /// constructor: `m_routingMode = RM_Walkaround`, confirmed by the
    /// router-orchestration research spec against `pns_routing_settings.
    /// cpp` -- not `RM_Shove`, which would be the more obvious guess).
    #[default]
    Walkaround,
}

/// `PNS_OPTIMIZATION_EFFORT`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub enum OptEffort {
    Low,
    #[default]
    Medium,
    Full,
}

/// `PNS::ROUTING_SETTINGS`. Defaults mirror `ROUTING_SETTINGS`'s own
/// constructor (`pns_routing_settings.cpp`) for every field KiCad persists;
/// `free_angle_mode` is pinned permanently `false` (this port only
/// implements KiCad's default 45-degree `DIRECTION_45` posture model --
/// free-angle routing is out of scope, see `docs/parity/GAPS.md` #7's
/// task scope).
#[derive(Debug, Clone)]
pub struct RoutingSettings {
    pub mode: Mode,
    pub optimizer_effort: OptEffort,
    /// `ShoveVias()`: when shoving, also push vias out of the way (`true`)
    /// rather than treating them as fixed obstacles to walk around.
    pub shove_vias: bool,
    /// `RemoveLoops()`: retiring a segment that makes the line double back
    /// over itself.
    pub remove_loops: bool,
    /// `SmartPads()`: shorten/angle the line's own first/last segment for a
    /// cleaner entry/exit from a pad. Not yet implemented by this port's
    /// optimizer (see `crates/pns/PARITY.md`); the setting exists so
    /// callers/tests can see it is deliberately inert for now.
    pub smart_pads: bool,
    /// `JumpOverObstacles()`.
    pub jump_over_obstacles: bool,
    pub shove_iteration_limit: i32,
    pub walkaround_iteration_limit: i32,
    pub via_force_prop_iteration_limit: i32,
    /// `GetFixAllSegments()`: a plain click (as opposed to the final,
    /// route-ending click) fixes the *entire* current head, not all but
    /// its last segment. KiCad's own default is `true`; this port only
    /// implements that default (see `crates/pns/PARITY.md`).
    pub fix_all_segments: bool,
    /// `WalkaroundHugLengthThreshold()`: a walkaround candidate within this
    /// multiple of the direct path's length is accepted outright, instead
    /// of falling back to the cursor-proximity heuristic.
    pub walkaround_hug_length_threshold: f64,
}

impl Default for RoutingSettings {
    fn default() -> Self {
        RoutingSettings {
            mode: Mode::default(),
            optimizer_effort: OptEffort::default(),
            shove_vias: true,
            remove_loops: true,
            smart_pads: true,
            jump_over_obstacles: false,
            shove_iteration_limit: 250,
            walkaround_iteration_limit: 40,
            via_force_prop_iteration_limit: 40,
            fix_all_segments: true,
            walkaround_hug_length_threshold: 1.5,
        }
    }
}

/// `PNS::SIZES_SETTINGS`, resolved for one net.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SizesSettings {
    pub clearance: Um,
    pub track_width: Um,
    pub via_diameter: Um,
    pub via_drill: Um,
}

impl SizesSettings {
    /// `PNS_KICAD_IFACE::ImportSizes` -- this port's version pulls straight
    /// from the board's own resolved net-class rules instead of a wx
    /// dialog's current values.
    pub fn for_net(rules: &BoardRules, net: &str) -> Self {
        SizesSettings { clearance: rules.clearance_of(net), track_width: rules.width_of(net), via_diameter: rules.via_diameter_of(net), via_drill: rules.via_drill_of(net) }
    }
}
