//! KiCad 10's via stitching (file version 20260816, "Via stitching and guarding"; `PCB_VIA_STITCH`, `pcbnew/generators/pcb_via_stitch.h` of KiCad 25ec2335 -- the 8303b2ad
//! source the rest of the port reads predates it): a generator (`PCB_GENERATOR`, `(generated (type via_stitch) ..)`) that fills an outline with vias of one net -- on a plain or a
//! staggered grid, or in a tiled Poisson pattern -- or guards the tracks of another net with a row of them. The vias it made are ordinary vias of the board, listed as its members.
//!
//! The board keeps them in [`crate::pcb_items::BoardItems::via_stitches`]. What the file has (`PCB_VIA_STITCH::GetProperties`, written in the order of a `std::map`): `layout`,
//! `mode`, `net_name`, `guarded_net_name`, `excluded_grid_cells`, `excluded_positions`, `origin` (the generator's), `origin_offset_x`/`_y` (where the grid is anchored, in
//! `[0, pitch)`), `outline`, `pitch` and `seed`; then the via template (`(templates (template (name "via") (via ..)))`) and the members.

use serde::{Deserialize, Serialize};

use crate::ir::{Point, Nm};

/// `PCB_VIA_STITCH_LAYOUT`: the file's `(layout 0|1|2)`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StitchLayout {
    /// A regular grid of rows and columns.
    #[default]
    Plain,
    /// Odd rows shifted by half the pitch.
    Staggered,
    /// A tiled Poisson distribution.
    Poisson,
}

impl StitchLayout {
    pub fn index(self) -> i64 {
        match self {
            StitchLayout::Plain => 0,
            StitchLayout::Staggered => 1,
            StitchLayout::Poisson => 2,
        }
    }
    pub fn from_index(i: i64) -> StitchLayout {
        match i {
            1 => StitchLayout::Staggered,
            2 => StitchLayout::Poisson,
            _ => StitchLayout::Plain,
        }
    }
}

/// `PCB_VIA_STITCH_MODE`: the file's `(mode 0|1)`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StitchMode {
    /// Fill the outline with vias in the layout's pattern.
    #[default]
    Stitch,
    /// Place vias to guard a net inside the outline.
    Guard,
}

impl StitchMode {
    pub fn index(self) -> i64 {
        match self {
            StitchMode::Stitch => 0,
            StitchMode::Guard => 1,
        }
    }
    pub fn from_index(i: i64) -> StitchMode {
        if i == 1 {
            StitchMode::Guard
        } else {
            StitchMode::Stitch
        }
    }
}

fn is_zero_nm(v: &Nm) -> bool {
    *v == Nm::ZERO
}
fn is_zero_point(p: &Point) -> bool {
    p.x == Nm::ZERO && p.y == Nm::ZERO
}
fn is_zero_u32(v: &u32) -> bool {
    *v == 0
}

/// The via a stitching makes copies of (`m_viaTemplate`, the generator's `(template (name "via") (via ..))`): its size and drill and the copper layers it spans, and the file's
/// text of it, which is written back as it was (`tokens`; its size and drill replaced when they were edited). A new stitching's template is the one `PCB_VIA_STITCH`'s
/// constructor makes, written as `format( PCB_TRACK )` writes a free via.
///
/// What Via Stitching Properties' Configure (`DIALOG_TRACK_VIA_PROPERTIES` on the template, `OnViaConfigureClicked`) set beyond the size, drill and layers -- the via type and
/// what becomes of the copper on the layers it does not connect -- is `kind` and `annular`; left `None`, the template's text says (and a template read from a file has them
/// `None`). Every via the stitching makes is a copy of the whole template (`m_viaTemplate->Clone()`): its type and every other token of its text. The other rows of the
/// dialog (UI-LEFT-2 PP22) are `props`, `teardrops`, `net` and `locked`, likewise `None` where the text says.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ViaTemplate {
    #[serde(alias = "size_um", deserialize_with = "crate::units::de")]
    pub size_nm: Nm,
    #[serde(alias = "drill_um", deserialize_with = "crate::units::de")]
    pub drill_nm: Nm,
    pub from_layer: String,
    pub to_layer: String,
    /// The `(via ..)` form as the file wrote it, on one line; empty for one made here.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub text: String,
    /// `PCB_VIA::SetViaType` of the template (the dialog's Via type), when it was set here.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<crate::via_extra::ViaKind>,
    /// `PADSTACK::SetUnconnectedLayerMode` of the template (the dialog's Annular rings), when it was set here.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub annular: Option<crate::padstack::UnconnectedLayers>,
    /// The dialog's Padstack mode, Protection features, Backdrill and Hole Post-machining rows (`PCB_VIA::SetPadstack`, `setViaConfiguration`,
    /// `SetBackdrillMode` ..), when they were set here: the template's whole `ViaProps`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub props: Option<crate::via_extra::ViaProps>,
    /// The dialog's Teardrops part (`PCB_VIA::GetTeardropParams()`), when it was set here.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub teardrops: Option<crate::teardrop_params::TeardropParams>,
    /// The dialog's Net (`PCB_VIA::SetNetCode`), when it was set here: the net a via the stitching makes is on when the stitching names none (`if( m_netCode )
    /// via->SetNetCode( m_netCode )` after the clone).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub net: Option<String>,
    /// The dialog's Locked (`SetLocked`), when it was set here: the clone of a locked template is locked.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub locked: Option<bool>,
}

impl Default for ViaTemplate {
    fn default() -> Self {
        ViaTemplate { size_nm: Nm::ZERO, drill_nm: Nm::ZERO, from_layer: "F.Cu".into(), to_layer: "B.Cu".into(), text: String::new(), kind: None, annular: None, props: None, teardrops: None, net: None, locked: None }
    }
}

impl ViaTemplate {
    /// Whether the dialog set anything the template's text does not say.
    pub fn is_configured(&self) -> bool {
        self.kind.is_some() || self.annular.is_some() || self.props.is_some() || self.teardrops.is_some() || self.net.is_some() || self.locked.is_some()
    }
}

/// One via stitching generator (`PCB_VIA_STITCH`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ViaStitch {
    /// `vst_xxxxxxxxxxxx`, assigned by the board.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub id: String,
    /// `GetLayer()`: the generator's layer (`F_Cu` for a new one).
    #[serde(default = "d_layer")]
    pub layer: String,
    /// `GetName()`: "Via Stitching".
    #[serde(default = "d_name")]
    pub name: String,
    /// The area the vias are placed in (`m_outline`'s one outline, a closed ring).
    pub outline: Vec<Point>,
    /// `m_pitch`: the centre-to-centre spacing; 0 is the default (`defaultPitch`: twice the via, and at least 2 mm).
    #[serde(default, skip_serializing_if = "is_zero_nm")]
    #[serde(alias = "pitch_um", deserialize_with = "crate::units::de")]
    pub pitch_nm: Nm,
    #[serde(default)]
    pub layout: StitchLayout,
    #[serde(default)]
    pub mode: StitchMode,
    /// `m_seed`: the random shift of the Poisson tiling.
    #[serde(default, skip_serializing_if = "is_zero_u32")]
    pub seed: u32,
    /// `m_originOffset`: where the grid is anchored relative to (0, 0), in `[0, pitch)` each way.
    #[serde(default, skip_serializing_if = "is_zero_point")]
    pub origin_offset: Point,
    /// `PCB_GENERATOR::m_origin`.
    #[serde(default, skip_serializing_if = "is_zero_point")]
    pub origin: Point,
    /// `m_lastNetName`: the net of the vias.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub net: String,
    /// `m_lastGuardedNetName`: the net a guard protects.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub guarded_net: String,
    /// `m_excludedCells`: the grid cells (column, row) the user took vias out of (Exclude From Stitching), for the grid layouts.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub excluded_cells: Vec<(i64, i64)>,
    /// `m_excludedPositions`: the board positions taken out, for the Poisson layout and a guard.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub excluded_positions: Vec<Point>,
    #[serde(default)]
    pub via: ViaTemplate,
    /// The ids of the board's vias this stitching made (`GetItems()`, the file's `(members ..)`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub member_ids: Vec<String>,
    /// KiCad 10's `(custom_property "key" "value")` forms, kept as the file has them.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub custom_properties: Vec<(String, String)>,
}

fn d_layer() -> String {
    "F.Cu".into()
}
fn d_name() -> String {
    "Via Stitching".into()
}

impl ViaStitch {
    /// `PCB_VIA_STITCH( parent )`: an outline and nothing else set; the defaults are filled in when it is first edited (`InitializeDefaults`).
    pub fn new(outline: Vec<Point>) -> ViaStitch {
        ViaStitch {
            id: String::new(),
            layer: d_layer(),
            name: d_name(),
            outline,
            pitch_nm: Nm::ZERO,
            layout: StitchLayout::Plain,
            mode: StitchMode::Stitch,
            seed: 0,
            origin_offset: Point { x: Nm::ZERO, y: Nm::ZERO },
            origin: Point { x: Nm::ZERO, y: Nm::ZERO },
            net: String::new(),
            guarded_net: String::new(),
            excluded_cells: Vec::new(),
            excluded_positions: Vec::new(),
            via: ViaTemplate::default(),
            member_ids: Vec::new(),
            custom_properties: Vec::new(),
        }
    }

    pub fn id_seed(&self) -> String {
        format!("via_stitch:{}:{:?}", self.net, self.outline.first())
    }

    /// `usesGridCells`: the plain and the staggered grid key exclusions by cell; the Poisson layout and a guard place at positions of their own.
    pub fn uses_grid_cells(&self) -> bool {
        self.layout != StitchLayout::Poisson && self.mode != StitchMode::Guard
    }

    /// The outline's box `[x0, y0, x1, y1]` (`GetBoundingBox`: `m_outline.BBox()`).
    pub fn bounds(&self) -> [Nm; 4] {
        let mut b = [Nm::MAX, Nm::MAX, Nm::MIN, Nm::MIN];
        for p in &self.outline {
            b = [b[0].min(p.x), b[1].min(p.y), b[2].max(p.x), b[3].max(p.y)];
        }
        b
    }
}

/// `defaultViaSize`: the board's current via size, 0.6 mm when it has none.
pub fn default_via_size(current: Nm) -> Nm {
    if current > Nm::ZERO {
        current
    } else {
        Nm::new(600_000)
    }
}

/// `defaultViaDrill`: the board's current via drill, 0.3 mm when it has none.
pub fn default_via_drill(current: Nm) -> Nm {
    if current > Nm::ZERO {
        current
    } else {
        Nm::new(300_000)
    }
}

/// `defaultPitch`: twice the via, and at least 2 mm.
pub fn default_pitch(via_size: Nm) -> Nm {
    (via_size * 2).max(Nm::new(2_000_000))
}
