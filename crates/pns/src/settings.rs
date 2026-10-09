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
/// constructor (`pns_routing_settings.cpp`) for every field this port has.
///
/// Every field is read by the router: `mode`, `optimizer_effort`,
/// `shove_vias`, `jump_over_obstacles`, `shove_iteration_limit` and
/// `via_force_prop_iteration_limit` by the shove and the via placement,
/// `walkaround_iteration_limit` and `walkaround_hug_length_threshold` by the
/// walkaround, `smart_pads` and `optimizer_effort` by the optimizer,
/// `remove_loops`, `fix_all_segments`, `free_angle_mode` and `can_violate_drc`
/// by the line placer and the dragger. The studio's HTTP API reads all of them
/// from the `settings` object of a start request
/// (`crates/cli/src/route_api.rs`), and Interactive Router Settings sets them.
///
/// KiCad's other settings are not here because nothing here could read them:
/// `m_autoPosture` (no mouse trail: the posture is the last segment's, or the
/// `/` key's), `m_smoothDraggedSegments` and `m_optimizeEntireDraggedTrack`
/// (the corner drag does not snap or optimize what it drags), `m_suggestFinish`
/// (KiCad hides it: "not implemented"), `m_followMouse`, `m_snapTo*`,
/// `m_cornerMode` (only the mitered 45-degree corner is built) and the time
/// limits (every call branches fresh from the world, there is no springback).
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
    /// cleaner entry/exit from a pad (`OPTIMIZER::SMART_PADS`).
    pub smart_pads: bool,
    /// `JumpOverObstacles()`: a track a shove pushed into a solid goes behind
    /// it (rank - 1) instead of being "reflected" back at the pusher.
    pub jump_over_obstacles: bool,
    pub shove_iteration_limit: i32,
    pub walkaround_iteration_limit: i32,
    /// `ViaForcePropIterationLimit()`: how many steps `VIA::PushoutForce`
    /// takes to get a via off what it sits on.
    pub via_force_prop_iteration_limit: i32,
    /// `GetFixAllSegments()`: a plain click (as opposed to the final,
    /// route-ending click) fixes the *entire* current head, not all but
    /// its last segment, which stays free and follows the cursor. KiCad's
    /// default is `true`.
    pub fix_all_segments: bool,
    /// `WalkaroundHugLengthThreshold()`: a walkaround within this multiple
    /// (twice, for the whole walk) of the direct path's length is taken as it
    /// is; a longer one hugs the obstacle to the point nearest the cursor.
    pub walkaround_hug_length_threshold: f64,
    /// `GetAllowDRCViolationsSetting()` (`CanViolateDRC`, `can_violate_drc` in
    /// the settings file): only with this set does `AllowDRCViolations()` let
    /// `MarkObstacles` mode commit a colliding head. Default `false`.
    pub can_violate_drc: bool,
    /// `GetFreeAngleMode()`: in `MarkObstacles` mode the head is a straight
    /// line from the last fixed point to the cursor, at any angle
    /// (`buildInitialLine`). In the other modes it does nothing, like KiCad's.
    pub free_angle_mode: bool,
}

impl RoutingSettings {
    /// `ROUTING_SETTINGS::AllowDRCViolations()`.
    pub fn allow_drc_violations(&self) -> bool {
        self.mode == Mode::MarkObstacles && self.can_violate_drc
    }

    /// `GetFreeAngleMode() && Mode() == RM_MarkObstacles`: the only place KiCad's router builds a head at any angle.
    pub fn free_angle(&self) -> bool {
        self.free_angle_mode && self.mode == Mode::MarkObstacles
    }
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
            can_violate_drc: false,
            free_angle_mode: false,
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
