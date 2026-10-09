//! Footprints on the board as editable objects: the text of their fields, their attributes and the edits of their pads.
//!
//! KiCad keeps all of this on the `FOOTPRINT` itself -- the Reference and Value `PCB_FIELD`s, any user fields, the attribute
//! bits (`FP_SMD`, `FP_DNP` ...), the `PAD`s' own shape, hole and margins. Here a placed footprint is a pose (`FootprintInstance`)
//! of a part whose pads come from a library definition shared by every instance, so what one instance changes is an *edit*
//! laid over it: [`FootprintEdit`], one per footprint, in `DrawingsSection::footprint_edits` (the same "design.json wins over
//! the frozen intent" arrangement as `board_parts` and `rules`). An edit that was never made is absent, and the footprint is
//! written the way it always was.
//!
//! # Field geometry
//!
//! A field's position and angle are stored **in the footprint's own frame**, the frame its pads are in (`Pad::at`): the
//! footprint as seen from the top, un-rotated, so moving, turning and flipping a footprint need no change here (KiCad moves
//! the fields along with it; this model never has to). Only the layer is absolute (`F.SilkS`, `B.Fab` ...) and the mirror flag
//! belongs to the side, so a flip turns the layer over and toggles the flag, as `PCB_TEXT::Flip` does. The `.kicad_pcb` stores a
//! field's position relative to its footprint but un-rotated by the footprint's orientation, with its angle absolute
//! (`PCB_IO_KICAD_SEXPR::format( PCB_TEXT )`); [`FieldLayout::file_position`] and [`FieldLayout::file_angle_deg`] are the way
//! across, and `eda_kicad`'s importer takes the same way back.
//!
//! # Pad edits
//!
//! A [`PadEdit`] changes one pad of one footprint (the k-th pad that carries a number, as the studio names it `REF.NUM#k`):
//! the shape, size, hole, offset, corner radius and the clearance and mask/paste margins of `dialog_pad_properties.cpp`.
//! [`patched_footprint`] lays the edits on a library footprint; `ConstraintModel::instance_footprints` carries the result so
//! every reader of `footprint_of` (the gates, the router, the exports) sees the pad as edited. What the engine's [`Pad`] has no
//! field for (the offset, the margins) is read from the edit by the `.kicad_pcb` writer.

use crate::footprint::{to_board, Footprint, Pad, PadKind, PadShape};
use crate::ir::{Design, FootprintInstance, LabelSide, Millideg, Point, Side, Um};
use serde::{Deserialize, Serialize};

/// The mandatory fields' names, as `PCB_FIELD::GetCanonicalName` gives them.
pub const REFERENCE: &str = "Reference";
pub const VALUE: &str = "Value";

fn d_true() -> bool {
    true
}
fn d_one() -> u32 {
    1
}
fn is_one(n: &u32) -> bool {
    *n == 1
}
fn is_zero(n: &i64) -> bool {
    *n == 0
}
fn is_zero_i8(n: &i8) -> bool {
    *n == 0
}
fn is_false(b: &bool) -> bool {
    !*b
}
fn is_true(b: &bool) -> bool {
    *b
}

/// One field of a footprint as laid out on the board: a `PCB_FIELD`'s `EDA_TEXT` attributes (`PCB_FIELDS_GRID_TABLE`'s columns
/// and the text rows of the Properties panel). See the module doc for the frame `at` and `angle` are in.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FieldLayout {
    /// The text's anchor, in the footprint's own frame (the frame of `Pad::at`), µm.
    pub at: Point,
    /// The text's angle in that frame, millidegrees, counter-clockwise on the screen as KiCad's text angles are. On a bottom-side
    /// footprint the frame is mirrored, and so is the sense of the angle: see [`FieldLayout::file_angle_deg`].
    #[serde(default, skip_serializing_if = "is_zero")]
    pub angle: i64,
    /// `(width, height)` of a glyph, µm (`GetTextWidth`, `GetTextHeight`).
    pub size: (Um, Um),
    /// Stroke width, µm; 0 is KiCad's auto thickness (no `(thickness ..)` written).
    #[serde(default)]
    pub thickness: Um,
    /// The layer the text is drawn on: `F.SilkS`, `B.Fab`, `Cmts.User` ... absolute, not relative to the footprint's side.
    pub layer: String,
    /// "Show" (`PCB_FIELD::IsVisible`); a hidden field is written `(hide yes)`.
    #[serde(default = "d_true", skip_serializing_if = "is_true")]
    pub visible: bool,
    /// Horizontal justification: `-1` left, `0` centre, `1` right.
    #[serde(default, skip_serializing_if = "is_zero_i8")]
    pub halign: i8,
    /// Vertical justification: `-1` top, `0` centre, `1` bottom.
    #[serde(default, skip_serializing_if = "is_zero_i8")]
    pub valign: i8,
    /// "Mirrored" (`EDA_TEXT::IsMirrored`): the glyphs are drawn mirrored, as text on a back layer is.
    #[serde(default, skip_serializing_if = "is_false")]
    pub mirror: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub bold: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub italic: bool,
    /// "Keep upright" (`PCB_TEXT::IsKeepUpright`): written as `(unlocked yes)` when off.
    #[serde(default = "d_true", skip_serializing_if = "is_true")]
    pub keep_upright: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub knockout: bool,
    /// The text shown, when it is not the footprint's reference (Reference field) or the part's value (Value field). Set on a
    /// copy that keeps the original's reference (Create Array's "Keep original reference designators"): its id is unique,
    /// the text it shows is not.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
}

impl FieldLayout {
    /// A field with KiCad's text defaults (`BOARD_DESIGN_SETTINGS` for silkscreen: 1 mm, 0.15 mm), at `at` on `layer`.
    pub fn new(at: Point, layer: &str) -> Self {
        FieldLayout {
            at,
            angle: 0,
            size: (1000, 1000),
            thickness: 150,
            layer: layer.to_string(),
            visible: true,
            halign: 0,
            valign: 0,
            mirror: layer.starts_with("B."),
            bold: false,
            italic: false,
            keep_upright: true,
            knockout: false,
            text: None,
        }
    }

    /// `+1` for a top-side footprint, `-1` for a bottom-side one: the sense of angles in its frame.
    fn sense(side: Side) -> i64 {
        if side == Side::Bottom {
            -1
        } else {
            1
        }
    }

    /// The anchor as the `.kicad_pcb` stores it: relative to the footprint, un-rotated, in the file's frame, which does not mirror
    /// a bottom-side footprint (so x is negated, as a pad's is).
    pub fn file_position(&self, side: Side) -> (Um, Um) {
        (if side == Side::Bottom { -self.at.x } else { self.at.x }, self.at.y)
    }

    /// The angle as the `.kicad_pcb` stores it: absolute, in degrees, counter-clockwise. `fp_rot` is the footprint's `rot`
    /// (clockwise millidegrees); its file angle is `-fp_rot`. A bottom-side footprint's frame is mirrored, so the field's own
    /// angle turns the other way (the same sense flip `pad_file_angle` applies to a pad's own rotation).
    pub fn file_angle_deg(&self, side: Side, fp_rot: Millideg) -> f64 {
        norm_deg((Self::sense(side) * self.angle - fp_rot as i64) as f64 / 1000.0)
    }

    /// Inverse of [`FieldLayout::file_position`] and [`FieldLayout::file_angle_deg`]: the layout's `at` and `angle` for a field
    /// the file puts at `(x, y)` with absolute `angle_deg`.
    pub fn set_from_file(&mut self, side: Side, fp_rot: Millideg, x: Um, y: Um, angle_deg: f64) {
        self.at = Point { x: if side == Side::Bottom { -x } else { x }, y };
        let abs = (angle_deg * 1000.0).round() as i64;
        self.angle = (Self::sense(side) * (abs + fp_rot as i64)).rem_euclid(360_000);
    }

    /// Where the anchor is on the board.
    pub fn board_position(&self, fp: &FootprintInstance) -> Point {
        to_board(fp, (self.at.x, self.at.y))
    }

    /// The text's absolute angle on the board, millidegrees counter-clockwise in `[0, 360000)` (`PCB_TEXT::GetTextAngle`).
    pub fn board_angle(&self, fp: &FootprintInstance) -> i64 {
        (Self::sense(fp.side) * self.angle - fp.rot as i64).rem_euclid(360_000)
    }

    /// Put the anchor at `pos` on the board and turn the text to the absolute angle `abs_angle` (millidegrees, counter-clockwise):
    /// what moving or turning the field on its own (`EDIT_TOOL` over a selected `PCB_FIELD`) comes to.
    pub fn set_from_board(&mut self, fp: &FootprintInstance, pos: Point, abs_angle: i64) {
        self.at = local_of(fp, pos);
        self.angle = (Self::sense(fp.side) * (abs_angle + fp.rot as i64)).rem_euclid(360_000);
    }

    /// The shown text, `own` unless this layout carries a text of its own.
    pub fn shown<'a>(&'a self, own: &'a str) -> &'a str {
        self.text.as_deref().unwrap_or(own)
    }
}

fn norm_deg(d: f64) -> f64 {
    let m = d.rem_euclid(360.0);
    if (m - 360.0).abs() < 1e-9 {
        0.0
    } else {
        m
    }
}

/// The inverse of [`to_board`]: the footprint-frame point that lands on `p` on the board.
pub fn local_of(fp: &FootprintInstance, p: Point) -> Point {
    let rad = (fp.rot as f64) / 1000.0 * std::f64::consts::PI / 180.0;
    let (sin, cos) = rad.sin_cos();
    let (dx, dy) = ((p.x - fp.at.x) as f64, (p.y - fp.at.y) as f64);
    let lx = dx * cos + dy * sin;
    let ly = -dx * sin + dy * cos;
    let mirror = if fp.side == Side::Bottom { -1.0 } else { 1.0 };
    Point { x: (lx * mirror).round() as Um, y: ly.round() as Um }
}

/// A user field (`FIELD_T::USER`): a name, a value and the layout of the text. Their order is their `m_ordinal`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UserField {
    pub name: String,
    pub text: String,
    pub layout: FieldLayout,
}

/// The "Component Type" choice of the Footprint Properties dialog (`m_componentType`): through-hole, SMD, or unspecified (a
/// footprint with neither `FP_THROUGH_HOLE` nor `FP_SMD`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FootprintKind {
    #[default]
    Unspecified,
    Smd,
    ThroughHole,
}

/// `FOOTPRINT::m_attributes` as the Footprint Properties dialog edits it, plus `AllowMissingCourtyard`. Written as `(attr ..)`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FootprintAttrs {
    #[serde(default)]
    pub kind: FootprintKind,
    /// `FP_BOARD_ONLY`: "Not in schematic" -- a footprint with no symbol (a mounting hole, a fiducial, a logo).
    #[serde(default)]
    pub board_only: bool,
    /// `FP_EXCLUDE_FROM_POS_FILES`: left out of the position (pick-and-place) files.
    #[serde(default)]
    pub exclude_from_pos_files: bool,
    /// `FP_EXCLUDE_FROM_BOM`: left out of the bill of materials.
    #[serde(default)]
    pub exclude_from_bom: bool,
    /// `FP_DNP`: "Do not populate".
    #[serde(default)]
    pub dnp: bool,
    /// `AllowMissingCourtyard` ("Exempt from courtyard requirement").
    #[serde(default)]
    pub allow_missing_courtyard: bool,
}

impl FootprintAttrs {
    /// The attributes of a footprint nothing was edited on: its type follows its pads, as the footprints of KiCad's libraries
    /// carry them (`GetLikelyAttribute`: any hole makes it through-hole, else any pad makes it SMD); the BOM and DNP flags follow
    /// the schematic symbol (`BOARD_NETLIST_UPDATER`), which is where the board gets them until they are edited here.
    pub fn derived(footprint: &Footprint, dnp: bool, exclude_from_bom: bool) -> Self {
        let kind = if footprint.pads.iter().any(|p| p.kind == PadKind::ThroughHole) {
            FootprintKind::ThroughHole
        } else if footprint.pads.iter().any(|p| p.kind == PadKind::Smd) {
            FootprintKind::Smd
        } else {
            FootprintKind::Unspecified
        };
        FootprintAttrs { kind, board_only: false, exclude_from_pos_files: false, exclude_from_bom, dnp, allow_missing_courtyard: false }
    }
}

/// How a pad's hole is drilled after an edit: a round hole, or an oblong one (`PAD_DRILL_SHAPE`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HoleShape {
    Round,
    Oblong,
}

/// Changes to one pad of one footprint. A field left `None` is the pad as the library has it. See the module doc.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PadEdit {
    /// The pad's number, and which of the pads that share it this is (1-based): the studio's `REF.NUM#k`.
    pub number: String,
    #[serde(default = "d_one", skip_serializing_if = "is_one")]
    pub nth: u32,
    /// Pad type: surface mount, through-hole (plated) or non-plated hole (`PAD_ATTRIB`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<PadKind>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shape: Option<PadShape>,
    /// `(width, height)`, un-rotated, µm.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<(Um, Um)>,
    /// A round hole's diameter, µm. Mutually exclusive with `drill_slot`; either one given replaces the library pad's hole.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub drill: Option<Um>,
    /// An oblong hole's `(width, height)`, µm.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub drill_slot: Option<(Um, Um)>,
    /// `PAD::SetOffset`: how far the copper shape sits from the pad's position (where the hole is), in the pad's own frame, µm.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub offset: Option<Point>,
    /// The pad's own rotation relative to its footprint, millidegrees clockwise (`Pad::rot`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rot: Option<Millideg>,
    /// `PAD::SetRoundRectRadiusRatio`: the corner radius as a fraction of the shorter side, `0..=0.5`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub roundrect_ratio: Option<f64>,
    /// `PAD::SetLocalClearance`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub clearance: Option<Um>,
    /// `PAD::SetLocalSolderMaskMargin`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub solder_mask_margin: Option<Um>,
    /// `PAD::SetLocalSolderPasteMargin`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub solder_paste_margin: Option<Um>,
    /// `PAD::SetLocalSolderPasteMarginRatio`: a fraction of the pad's size (-0.5 is 50% smaller).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub solder_paste_margin_ratio: Option<f64>,
}

impl PadEdit {
    /// An edit that changes nothing.
    pub fn none(number: &str, nth: u32) -> Self {
        PadEdit { number: number.to_string(), nth, ..Default::default() }
    }

    /// Whether this edit changes anything (an empty one is dropped, not stored).
    pub fn is_empty(&self) -> bool {
        self.kind.is_none()
            && self.shape.is_none()
            && self.size.is_none()
            && self.drill.is_none()
            && self.drill_slot.is_none()
            && self.offset.is_none()
            && self.rot.is_none()
            && self.roundrect_ratio.is_none()
            && self.clearance.is_none()
            && self.solder_mask_margin.is_none()
            && self.solder_paste_margin.is_none()
            && self.solder_paste_margin_ratio.is_none()
    }

    /// Whether this edit changes the pad's geometry the engine knows (shape, size, hole, type, rotation, corner radius), as
    /// opposed to what only the `.kicad_pcb` carries (offset, margins).
    pub fn changes_engine_pad(&self) -> bool {
        self.kind.is_some() || self.shape.is_some() || self.size.is_some() || self.drill.is_some() || self.drill_slot.is_some() || self.rot.is_some() || self.roundrect_ratio.is_some()
    }

    /// The same pad, identified: `REF.NUM` for the first pad with the number, `REF.NUM#k` for the k-th (k from 2).
    pub fn pad_id(&self, reference: &str) -> String {
        pad_id(reference, &self.number, self.nth)
    }
}

/// The id of a footprint's field: `REF:Reference`, `REF:Value`, `REF:<user field name>`. A reference holds no `:`, so the first one
/// splits the id.
pub fn field_id(reference: &str, name: &str) -> String {
    format!("{reference}:{name}")
}

/// Split a field id into the footprint's reference and the field's name.
pub fn parse_field_id(id: &str) -> Option<(&str, &str)> {
    let (reference, name) = id.split_once(':')?;
    (!reference.is_empty() && !name.is_empty()).then_some((reference, name))
}

/// The id the studio gives the `nth` pad numbered `number` of footprint `reference`: `REF.NUM`, `REF.NUM#2` ...
pub fn pad_id(reference: &str, number: &str, nth: u32) -> String {
    if nth <= 1 {
        format!("{reference}.{number}")
    } else {
        format!("{reference}.{number}#{nth}")
    }
}

/// Split a pad id into its footprint reference, pad number and `nth`. A reference holds no `.`; the number is whatever follows
/// the first one up to a trailing `#k`.
pub fn parse_pad_id(id: &str) -> Option<(String, String, u32)> {
    let dot = id.find('.')?;
    if dot == 0 {
        return None;
    }
    let (reference, rest) = (&id[..dot], &id[dot + 1..]);
    if let Some(hash) = rest.rfind('#') {
        if let Ok(n) = rest[hash + 1..].parse::<u32>() {
            if n >= 2 {
                return Some((reference.to_string(), rest[..hash].to_string(), n));
            }
        }
    }
    Some((reference.to_string(), rest.to_string(), 1))
}

/// Everything edited on one footprint of the board. See the module doc.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FootprintEdit {
    /// The footprint's reference (`FootprintInstance::id`).
    pub id: String,
    /// The Reference field, when its layout was set or imported; `None` is where the writer has always put it (beside the
    /// courtyard, on the side `FootprintInstance::label` names).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reference: Option<FieldLayout>,
    /// The Value field, likewise (`None`: 1 mm text on the fabrication layer at the footprint's origin).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<FieldLayout>,
    /// The user fields, in order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fields: Vec<UserField>,
    /// The attributes, when they were set or imported; `None` derives them ([`FootprintAttrs::derived`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attrs: Option<FootprintAttrs>,
    /// Pad edits, sorted by `(number, nth)`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pads: Vec<PadEdit>,
}

impl FootprintEdit {
    pub fn new(id: &str) -> Self {
        FootprintEdit { id: id.to_string(), ..Default::default() }
    }

    /// Whether nothing is edited (the entry is then dropped).
    pub fn is_empty(&self) -> bool {
        self.reference.is_none() && self.value.is_none() && self.fields.is_empty() && self.attrs.is_none() && self.pads.is_empty()
    }

    /// The edit of the `nth` pad numbered `number`.
    pub fn pad(&self, number: &str, nth: u32) -> Option<&PadEdit> {
        self.pads.iter().find(|p| p.number == number && p.nth == nth)
    }

    /// Replace (or, when `edit` changes nothing, remove) the edit of one pad, keeping the list sorted.
    pub fn set_pad(&mut self, edit: PadEdit) {
        self.pads.retain(|p| !(p.number == edit.number && p.nth == edit.nth));
        if !edit.is_empty() {
            self.pads.push(edit);
            self.pads.sort_by(|a, b| (&a.number, a.nth).cmp(&(&b.number, b.nth)));
        }
    }
}

impl Design {
    /// The edit of footprint `id`, if it has one.
    pub fn footprint_edit(&self, id: &str) -> Option<&FootprintEdit> {
        self.drawings.as_ref()?.footprint_edits.iter().find(|e| e.id == id)
    }

    /// The `DNP` and `Exclude from BOM` flags of the schematic symbol(s) of `id`: where the board's footprint takes them from
    /// until they are edited on the footprint.
    pub fn symbol_bom_flags(&self, id: &str) -> (bool, bool) {
        let mut dnp = false;
        let mut no_bom = false;
        let root = self.schematic.as_ref().map(|s| s.symbols.iter()).into_iter().flatten();
        let sheets = self.sheet_contents.iter().flat_map(|c| c.values()).flat_map(|s| s.symbols.iter());
        for sym in root.chain(sheets).filter(|s| s.id == id) {
            dnp |= sym.dnp;
            no_bom |= sym.exclude_from_bom;
        }
        (dnp, no_bom)
    }

    /// The attributes a footprint is written with: the edit's, else the derived ones.
    pub fn effective_attrs(&self, id: &str, footprint: &Footprint) -> FootprintAttrs {
        if let Some(a) = self.footprint_edit(id).and_then(|e| e.attrs) {
            return a;
        }
        let (dnp, no_bom) = self.symbol_bom_flags(id);
        FootprintAttrs::derived(footprint, dnp, no_bom)
    }
}

impl Design {
    /// Put the `DNP` and `Exclude from BOM` flags the footprint edits state on the schematic symbols of the same references, on every
    /// screen. The board's footprint and the schematic's symbol are two copies of one fact (KiCad keeps them in step with "Update PCB
    /// from Schematic"); an edit on the footprint reaches the schematic here, so the BOM kicad-cli writes from the schematic honours it.
    pub fn sync_symbol_bom_flags(&mut self, edits: &[FootprintEdit]) {
        let flags: Vec<(&str, FootprintAttrs)> = edits.iter().filter_map(|e| e.attrs.map(|a| (e.id.as_str(), a))).collect();
        if flags.is_empty() {
            return;
        }
        let screens = self.schematic.iter_mut().chain(self.sheet_contents.iter_mut().flat_map(|c| c.values_mut()));
        for screen in screens {
            for sym in screen.symbols.iter_mut() {
                if let Some((_, a)) = flags.iter().find(|(id, _)| *id == sym.id) {
                    sym.dnp = a.dnp;
                    sym.exclude_from_bom = a.exclude_from_bom;
                }
            }
        }
    }
}

// ------------------------------------------------------------------------------------------------------- default layouts

/// Where a footprint's Reference has always been written: beside the courtyard on the side `fp.label` names, 1 mm text on the
/// silkscreen of the footprint's side, absolute angle 0 (so it stays horizontal whatever the footprint's orientation). The
/// numbers are `eda_kicad`'s writer's own, kept so an unedited footprint is written exactly as before.
pub fn default_reference_layout(fp: &FootprintInstance, footprint: &Footprint) -> FieldLayout {
    let (hw, hh) = footprint.courtyard_half();
    let half_w = 300 * fp.id.chars().count() as Um;
    let (file_x, file_y) = match fp.label {
        LabelSide::Above => (0, -(hh + 700)),
        LabelSide::Below => (0, hh + 700),
        LabelSide::Left => (-(hw + 200 + half_w), 0),
        LabelSide::Right => (hw + 200 + half_w, 0),
    };
    let layer = if fp.side == Side::Bottom { "B.SilkS" } else { "F.SilkS" };
    let mut l = FieldLayout::new(Point { x: if fp.side == Side::Bottom { -file_x } else { file_x }, y: file_y }, layer);
    l.angle = (FieldLayout::sense(fp.side) * fp.rot as i64).rem_euclid(360_000);
    l
}

/// The Value as the writer has always put it: 1 mm text at the footprint's origin, one text-height below it, on the
/// fabrication layer of the footprint's side.
pub fn default_value_layout(fp: &FootprintInstance) -> FieldLayout {
    let layer = if fp.side == Side::Bottom { "B.Fab" } else { "F.Fab" };
    let mut l = FieldLayout::new(Point { x: 0, y: 1000 }, layer);
    l.angle = (FieldLayout::sense(fp.side) * fp.rot as i64).rem_euclid(360_000);
    l
}

/// The layout a new user field starts from (`DIALOG_FOOTPRINT_PROPERTIES::OnAddField`): hidden, at the footprint's origin, on
/// the fabrication layer of its side.
pub fn new_field_layout(fp: &FootprintInstance) -> FieldLayout {
    let layer = if fp.side == Side::Bottom { "B.Fab" } else { "F.Fab" };
    let mut l = FieldLayout::new(Point { x: 0, y: 0 }, layer);
    l.visible = false;
    l.angle = (FieldLayout::sense(fp.side) * fp.rot as i64).rem_euclid(360_000);
    l
}

/// The Reference layout in effect: the edit's, else the default.
pub fn effective_reference(design: &Design, fp: &FootprintInstance, footprint: &Footprint) -> FieldLayout {
    design.footprint_edit(&fp.id).and_then(|e| e.reference.clone()).unwrap_or_else(|| default_reference_layout(fp, footprint))
}

/// The Value layout in effect: the edit's, else the default.
pub fn effective_value(design: &Design, fp: &FootprintInstance) -> FieldLayout {
    design.footprint_edit(&fp.id).and_then(|e| e.value.clone()).unwrap_or_else(|| default_value_layout(fp))
}

// ------------------------------------------------------------------------------------------------------------ pad edits

/// The `nth` (1-based) pad of `footprint` that carries `number`, as an index into `footprint.pads`.
pub fn pad_index(footprint: &Footprint, number: &str, nth: u32) -> Option<usize> {
    footprint.pads.iter().enumerate().filter(|(_, p)| p.number == number).map(|(i, _)| i).nth(nth.max(1) as usize - 1)
}

/// What editing the pad does to the engine's pad: shape, size, hole, type, rotation and corner radius. The offset and the
/// margins have no place in a [`Pad`]; the writer reads them from the [`PadEdit`].
pub fn apply_pad_edit(pad: &mut Pad, e: &PadEdit) {
    if let Some(k) = e.kind {
        pad.kind = k;
        if k == PadKind::Smd {
            pad.drill = None;
            pad.drill_slot = None;
        }
    }
    if let Some(s) = e.shape {
        pad.shape = s;
    }
    if let Some(sz) = e.size {
        pad.size = sz;
    }
    // `PAD::SetShape( CIRCLE )` keeps the pad square: KiCad has one size for a circle.
    if pad.shape == PadShape::Circle {
        pad.size.1 = pad.size.0;
    }
    if e.drill.is_some() || e.drill_slot.is_some() {
        pad.drill = e.drill;
        pad.drill_slot = if e.drill.is_some() { None } else { e.drill_slot };
    }
    if let Some(r) = e.rot {
        pad.rot = r;
    }
    if let Some(r) = e.roundrect_ratio {
        pad.roundrect_ratio = Some(r);
    }
    // A pad turned into a rounded rectangle without a ratio takes KiCad's own default.
    if pad.shape == PadShape::RoundRect && pad.roundrect_ratio.is_none() {
        pad.roundrect_ratio = Some(0.25);
    }
    if pad.shape != PadShape::RoundRect {
        pad.roundrect_ratio = None;
    }
}

/// `footprint` with the edits laid on its pads. An edit that names a pad the footprint does not have is skipped.
pub fn patched_footprint(footprint: &Footprint, edits: &[PadEdit]) -> Footprint {
    let mut out = footprint.clone();
    for e in edits {
        if let Some(i) = pad_index(footprint, &e.number, e.nth) {
            apply_pad_edit(&mut out.pads[i], e);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fp(side: Side, rot: Millideg) -> FootprintInstance {
        FootprintInstance { id: "R1".into(), at: Point { x: 10_000, y: 20_000 }, rot, side, label: LabelSide::Above }
    }

    fn pad(number: &str, at: (Um, Um)) -> Pad {
        Pad { number: number.into(), at, size: (600, 600), shape: PadShape::Rect, kind: PadKind::Smd, drill: None, drill_slot: None, rot: 0, roundrect_ratio: None, opposite_side: false }
    }

    #[test]
    fn a_field_goes_across_to_the_file_and_back_on_either_side() {
        for (side, rot) in [(Side::Top, 0), (Side::Top, 90_000), (Side::Bottom, 0), (Side::Bottom, 90_000), (Side::Bottom, 270_000), (Side::Top, 45_000)] {
            let mut l = FieldLayout::new(Point { x: 1_500, y: -2_000 }, "F.SilkS");
            l.angle = 90_000;
            let (x, y) = l.file_position(side);
            let a = l.file_angle_deg(side, rot);
            let mut back = FieldLayout::new(Point { x: 0, y: 0 }, "F.SilkS");
            back.set_from_file(side, rot, x, y, a);
            assert_eq!((back.at, back.angle), (l.at, l.angle), "{side:?} {rot}");
        }
    }

    #[test]
    fn the_file_position_is_unmirrored_for_the_top_side_and_negated_for_the_bottom() {
        let l = FieldLayout::new(Point { x: 1_500, y: -2_000 }, "F.SilkS");
        assert_eq!(l.file_position(Side::Top), (1_500, -2_000));
        assert_eq!(l.file_position(Side::Bottom), (-1_500, -2_000));
    }

    #[test]
    fn field_ids_split_at_the_first_colon() {
        assert_eq!(field_id("R1", "Reference"), "R1:Reference");
        assert_eq!(parse_field_id("R1:Reference"), Some(("R1", "Reference")));
        assert_eq!(parse_field_id("R1#2:Vendor:code"), Some(("R1#2", "Vendor:code")));
        assert_eq!(parse_field_id("R1"), None);
        assert_eq!(parse_field_id(":x"), None);
        assert_eq!(parse_field_id("R1:"), None);
    }

    #[test]
    fn a_text_turns_with_its_footprint_and_the_board_position_follows_to_board() {
        let mut l = FieldLayout::new(Point { x: 1_000, y: 0 }, "F.SilkS");
        l.angle = 0;
        // a footprint turned 90 degrees clockwise on the screen: the text 1 mm to its right is now 1 mm below it
        let f = fp(Side::Top, 90_000);
        assert_eq!(l.board_position(&f), Point { x: 10_000, y: 21_000 });
        // and its angle (counter-clockwise) follows: 0 - 90 degrees
        assert_eq!(l.board_angle(&f), 270_000);
    }

    #[test]
    fn local_of_is_the_inverse_of_to_board() {
        for (side, rot) in [(Side::Top, 0), (Side::Top, 90_000), (Side::Bottom, 0), (Side::Bottom, 270_000), (Side::Bottom, 180_000)] {
            let f = fp(side, rot);
            let local = Point { x: 1_234, y: -5_678 };
            let on_board = to_board(&f, (local.x, local.y));
            assert_eq!(local_of(&f, on_board), local, "{side:?} {rot}");
        }
    }

    #[test]
    fn set_from_board_puts_the_anchor_and_the_angle_where_asked() {
        for (side, rot) in [(Side::Top, 0), (Side::Bottom, 90_000), (Side::Top, 180_000)] {
            let f = fp(side, rot);
            let mut l = FieldLayout::new(Point { x: 0, y: 0 }, "F.SilkS");
            l.set_from_board(&f, Point { x: 12_000, y: 19_000 }, 45_000);
            assert_eq!(l.board_position(&f), Point { x: 12_000, y: 19_000 });
            assert_eq!(l.board_angle(&f), 45_000);
        }
    }

    #[test]
    fn the_default_reference_is_where_the_writer_has_always_put_it_and_stays_horizontal() {
        let footprint = Footprint { name: "X".into(), pads: vec![pad("1", (-1_000, 0)), pad("2", (1_000, 0))], courtyard: Some((2_000, 1_000)), courtyard_outlines: vec![], model: None };
        for (side, rot) in [(Side::Top, 0), (Side::Top, 90_000), (Side::Bottom, 0), (Side::Bottom, 270_000)] {
            let f = fp(side, rot);
            let l = default_reference_layout(&f, &footprint);
            // above the courtyard: 700 um over the 1000 um half height, in the file's frame
            assert_eq!(l.file_position(side), (0, -1_700));
            assert_eq!(l.file_angle_deg(side, rot), 0.0, "the file angle stays 0, {side:?} {rot}");
            assert_eq!(l.layer, if side == Side::Bottom { "B.SilkS" } else { "F.SilkS" });
            assert_eq!(l.mirror, side == Side::Bottom);
        }
        let mut left = fp(Side::Bottom, 0);
        left.label = LabelSide::Left;
        let l = default_reference_layout(&left, &footprint);
        let half_w = 600;
        assert_eq!(l.file_position(Side::Bottom), (-(2_000 + 200 + half_w), 0));
    }

    #[test]
    fn the_default_value_is_a_millimetre_below_the_origin_on_the_fab_layer() {
        let l = default_value_layout(&fp(Side::Top, 0));
        assert_eq!((l.file_position(Side::Top), l.layer.as_str()), ((0, 1_000), "F.Fab"));
        assert_eq!(default_value_layout(&fp(Side::Bottom, 0)).layer, "B.Fab");
    }

    #[test]
    fn pad_ids_name_the_kth_pad_of_a_number_and_split_back() {
        assert_eq!(pad_id("J1", "3", 1), "J1.3");
        assert_eq!(pad_id("J1", "3", 2), "J1.3#2");
        assert_eq!(parse_pad_id("J1.3"), Some(("J1".into(), "3".into(), 1)));
        assert_eq!(parse_pad_id("J1.3#2"), Some(("J1".into(), "3".into(), 2)));
        assert_eq!(parse_pad_id("J1.A.1#3"), Some(("J1".into(), "A.1".into(), 3)));
        assert_eq!(parse_pad_id("J1"), None);
        assert_eq!(parse_pad_id(".3"), None);
    }

    #[test]
    fn an_edit_finds_the_kth_pad_with_a_number_and_changes_what_it_names() {
        let footprint = Footprint { name: "X".into(), pads: vec![pad("1", (0, 0)), pad("2", (1_000, 0)), pad("2", (2_000, 0))], courtyard: None, courtyard_outlines: vec![], model: None };
        assert_eq!(pad_index(&footprint, "2", 2), Some(2));
        assert_eq!(pad_index(&footprint, "2", 3), None);
        let mut e = PadEdit::none("2", 2);
        e.size = Some((900, 400));
        e.shape = Some(PadShape::Oval);
        let patched = patched_footprint(&footprint, &[e]);
        assert_eq!(patched.pads[2].size, (900, 400));
        assert_eq!(patched.pads[2].shape, PadShape::Oval);
        assert_eq!(patched.pads[1].size, (600, 600), "the other pad with that number is untouched");
        assert_eq!(footprint.pads[2].size, (600, 600), "the library footprint is not changed");
    }

    #[test]
    fn a_hole_edit_replaces_the_hole_and_an_smd_pad_has_none() {
        let mut p = pad("1", (0, 0));
        p.kind = PadKind::ThroughHole;
        p.drill = Some(800);
        p.size = (1_600, 1_600);
        p.shape = PadShape::Circle;
        let mut e = PadEdit::none("1", 1);
        e.drill_slot = Some((1_000, 600));
        apply_pad_edit(&mut p, &e);
        assert_eq!((p.drill, p.drill_slot), (None, Some((1_000, 600))), "an oblong hole replaces the round one");
        let mut e = PadEdit::none("1", 1);
        e.drill = Some(900);
        apply_pad_edit(&mut p, &e);
        assert_eq!((p.drill, p.drill_slot), (Some(900), None));
        let mut e = PadEdit::none("1", 1);
        e.kind = Some(PadKind::Smd);
        apply_pad_edit(&mut p, &e);
        assert_eq!((p.kind, p.drill, p.drill_slot), (PadKind::Smd, None, None));
    }

    #[test]
    fn a_circle_stays_square_and_only_a_rounded_rectangle_keeps_a_ratio() {
        let mut p = pad("1", (0, 0));
        let mut e = PadEdit::none("1", 1);
        e.shape = Some(PadShape::Circle);
        e.size = Some((900, 500));
        apply_pad_edit(&mut p, &e);
        assert_eq!(p.size, (900, 900));
        let mut e = PadEdit::none("1", 1);
        e.shape = Some(PadShape::RoundRect);
        apply_pad_edit(&mut p, &e);
        assert_eq!(p.roundrect_ratio, Some(0.25), "KiCad's default ratio");
        let mut e = PadEdit::none("1", 1);
        e.roundrect_ratio = Some(0.4);
        apply_pad_edit(&mut p, &e);
        assert_eq!(p.roundrect_ratio, Some(0.4));
        let mut e = PadEdit::none("1", 1);
        e.shape = Some(PadShape::Rect);
        apply_pad_edit(&mut p, &e);
        assert_eq!(p.roundrect_ratio, None);
    }

    #[test]
    fn the_derived_attributes_follow_the_pads_and_the_symbol() {
        let smd = Footprint { name: "X".into(), pads: vec![pad("1", (0, 0))], courtyard: None, courtyard_outlines: vec![], model: None };
        let mut th_pad = pad("1", (0, 0));
        th_pad.kind = PadKind::ThroughHole;
        th_pad.drill = Some(800);
        let th = Footprint { name: "Y".into(), pads: vec![pad("2", (0, 0)), th_pad], courtyard: None, courtyard_outlines: vec![], model: None };
        let hole = Footprint { name: "Z".into(), pads: vec![Pad { kind: PadKind::NonPlatedHole, drill: Some(2_000), ..pad("", (0, 0)) }], courtyard: None, courtyard_outlines: vec![], model: None };
        assert_eq!(FootprintAttrs::derived(&smd, false, false).kind, FootprintKind::Smd);
        assert_eq!(FootprintAttrs::derived(&th, false, false).kind, FootprintKind::ThroughHole, "any hole makes it through-hole");
        assert_eq!(FootprintAttrs::derived(&hole, false, false).kind, FootprintKind::Unspecified);
        let a = FootprintAttrs::derived(&smd, true, true);
        assert!(a.dnp && a.exclude_from_bom && !a.board_only && !a.exclude_from_pos_files);
    }

    #[test]
    fn an_edit_with_nothing_in_it_is_empty_and_a_pad_edit_replaces_its_predecessor() {
        let mut e = FootprintEdit::new("R1");
        assert!(e.is_empty());
        let mut p = PadEdit::none("1", 1);
        p.size = Some((700, 700));
        e.set_pad(p.clone());
        assert!(!e.is_empty() && e.pad("1", 1).is_some());
        p.size = Some((800, 800));
        e.set_pad(p);
        assert_eq!(e.pads.len(), 1);
        assert_eq!(e.pad("1", 1).unwrap().size, Some((800, 800)));
        e.set_pad(PadEdit::none("1", 1));
        assert!(e.is_empty(), "an edit that changes nothing is dropped");
    }

    #[test]
    fn the_edit_reads_and_writes_json_without_the_defaults() {
        let mut e = FootprintEdit::new("R1");
        e.reference = Some(FieldLayout::new(Point { x: 0, y: -1_700 }, "F.SilkS"));
        let text = serde_json::to_string(&e).unwrap();
        assert!(!text.contains("visible") && !text.contains("mirror") && !text.contains("keep_upright"), "{text}");
        let back: FootprintEdit = serde_json::from_str(&text).unwrap();
        assert_eq!(back, e);
        let mut hidden = FieldLayout::new(Point { x: 0, y: 0 }, "B.Fab");
        hidden.visible = false;
        hidden.keep_upright = false;
        let v: FieldLayout = serde_json::from_str(&serde_json::to_string(&hidden).unwrap()).unwrap();
        assert_eq!(v, hidden);
    }
}
