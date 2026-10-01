//! RS-274X (Gerber X2) writer.
//!
//! A port of KiCad's `GERBER_PLOTTER` (`common/plotters/GERBER_plotter.cpp`),
//! its X2 attribute helpers (`common/gbr_metadata.cpp`), and the per-item
//! plotting dispatch in `pcbnew/plot_board_layers.cpp` /
//! `plot_brditems_plotter.cpp`. Scoped to what this workspace's own model
//! can express:
//!
//! - Pads are rect / round-rect / circle / oval only (no custom/trapezoid/
//!   chamfered shapes -- this model has none). `PlacedPad::size` is already
//!   the board-space axis-aligned extent (see `eda_model::footprint::
//!   rotated_extent_by`'s own doc comment: exact at 0/90/180/270 degrees,
//!   which is the only case this model's placer/router ever produces), so
//!   a Gerber aperture never needs the rotated-corners macro form KiCad
//!   falls back to for an arbitrary-angle rect/oval/round-rect pad --
//!   every aperture this writer emits is axis-aligned, exactly as KiCad's
//!   own writer would emit for the same 0/90/180/270 case.
//! - Tracks are straight segments (no arcs on copper).
//! - Zone fills come in already "fractured" (`eda_zone_filler`/
//!   `eda_shape_poly_set::fracture`), so each fragment is a single closed
//!   contour -- exactly the per-outline loop `BRDITEMS_PLOTTER::PlotZone`
//!   runs over `SHAPE_POLY_SET::Outline(idx)`, with no separate hole list
//!   to plot.
//! - Footprint reference/value silkscreen text is not plotted (see
//!   `eda_drc::stroke_font`'s own doc comment: the footprint-relative
//!   anchor/rotation convention has an open, uninvestigated bug). Free-
//!   standing `Text`/`Shape` items (`DrawingsSection`) on a silkscreen
//!   layer are plotted in full, since their anchor is already an absolute
//!   board point with no such ambiguity.
//!
//! ## Object/aperture attribute bookkeeping
//!
//! KiCad's object-attribute dictionary (`%TO.P`/`%TO.N`/`%TO.C`, cleared by
//! a bare `%TD*%`) is diffed against the *previous* object: a key is only
//! re-printed when its value changes, and the whole dictionary is cleared
//! first when a key that was present stops being used (`GERBER_PLOTTER::
//! formatNetAttribute` / `FormatNetAttribute` in `gbr_metadata.cpp`).
//! [`AttrState`] below is that same diff, specialised to the three
//! attribute combinations this writer ever asks for (pad: P+N, mask/paste
//! pad: C only, track/via/zone: N only). `GERBER_PLOTTER::StartBlock`/
//! `EndBlock` (called once per footprint's pad group, once for all vias,
//! once for all tracks, once per zone fragment) are [`AttrState::clear`]:
//! unconditional in source order, but a no-op on an already-empty
//! dictionary, which is what makes a plain `%TD*%` appear exactly once per
//! footprint and not once per pad.
//!
//! The aperture-attribute dictionary (`%TA.AperFunction`) is a *different*
//! dictionary with its own reset rule (`GERBER_PLOTTER::writeApertureList`):
//! it is set and then immediately cleared after every single aperture
//! definition that has one, so two consecutive apertures sharing the same
//! function each still get their own `%TA...%`/`%TD*%` pair -- see
//! [`write_aperture_list`].

use eda_model::footprint::to_board;
use eda_model::ir::{Design, FootprintInstance, Point, Shape, Side, Um};
use eda_model::{CheckResult, ConstraintModel, PadKind, PadShape, Part};
use std::collections::BTreeMap;
use std::fmt::Write as _;

/// Per-package metadata a caller supplies once (the file-writer itself does
/// not know the project name or when "now" is -- see `ExportMeta` elsewhere
/// in this workspace for the same split).
pub struct FabMeta {
    pub title: String,
    /// RFC3339, e.g. `2026-10-01T12:00:00-07:00`.
    pub date: String,
    pub rev: String,
    pub generator_version: String,
}

impl FabMeta {
    pub fn project_guid(&self) -> String {
        project_guid(&self.title)
    }
}

/// `GbrMakeProjectGUIDfromString` (`common/gbr_metadata.cpp`): a
/// deterministic, RFC4122-*shaped* (not a real v4 UUID -- KiCad's own isn't
/// either) GUID derived from the project name, padded/truncated to 16
/// bytes first.
fn project_guid(name: &str) -> String {
    let mut b: Vec<u8> = name.bytes().collect();
    while b.len() < 16 {
        b.push(b'X');
    }
    let h2 = |v: u32, w: usize| format!("{:0width$x}", v, width = w);
    let mut out = String::new();
    for &byte in &b[0..4] {
        out.push_str(&h2(byte as u32, 2));
    }
    out.push('-');
    for &byte in &b[4..6] {
        out.push_str(&h2(byte as u32, 2));
    }
    out.push_str("-4");
    {
        let cc = ((b[6] as u32) << 4 & 0xFF0) + ((b[7] as u32) >> 4 & 0x0F);
        out.push_str(&h2(cc, 3));
    }
    out.push_str("-9");
    {
        let cc = (((b[8] as u32) & 0x0F) << 8) + (b[9] as u32 & 0xFF);
        out.push_str(&h2(cc, 3));
    }
    out.push('-');
    for &byte in &b[10..16] {
        out.push_str(&h2(byte as u32, 2));
    }
    out
}

// ---------------------------------------------------------------- numbers

/// Gerber X coordinate: `%FSLAX46Y46%` (4.6 format, leading zero omitted) is
/// raw nanometres -- this model's µm scaled by 1000. See the module doc.
fn gx(um: Um) -> i64 {
    um * 1000
}

/// Gerber Y: KiCad's plot coordinate system is Y-up; this model (like
/// every internal PCB coordinate) is Y-down, so the plotted value is
/// negated. Checked directly against a `kicad-cli`-plotted board: a pad at
/// internal Y = 17.2mm plots as `Y-17200000`.
fn gy(um: Um) -> i64 {
    -(um * 1000)
}

/// An aperture/macro parameter in millimetres, 6 decimals, always with a
/// decimal point (`fmt::format`'s `{:#f}` in the original -- never
/// exponential, per the Gerber spec's "mass parameter" rule).
fn mm6(um: Um) -> String {
    format!("{:.6}", um as f64 / 1000.0)
}

fn flash(out: &mut String, p: Point) {
    writeln!(out, "X{}Y{}D03*", gx(p.x), gy(p.y)).unwrap();
}
fn move_to(out: &mut String, p: Point) {
    writeln!(out, "X{}Y{}D02*", gx(p.x), gy(p.y)).unwrap();
}
fn line_to(out: &mut String, p: Point) {
    writeln!(out, "X{}Y{}D01*", gx(p.x), gy(p.y)).unwrap();
}

// ------------------------------------------------------------ attributes

/// The object-attribute dictionary diff -- see the module doc. `p`/`n`/`c`
/// are `None` when that key is not wanted *at all* by the current object
/// kind (e.g. a track never wants `P`); `Some(String::new())` is a wanted-
/// but-empty value (KiCad's "not in net" pad: the key is present with no
/// value, not absent).
#[derive(Default)]
struct AttrState {
    p: Option<String>,
    n: Option<String>,
    c: Option<String>,
}

impl AttrState {
    fn clear(&mut self, out: &mut String) {
        if self.p.is_some() || self.n.is_some() || self.c.is_some() {
            out.push_str("%TD*%\n");
        }
        *self = AttrState::default();
    }

    fn apply(&mut self, out: &mut String, p: Option<String>, n: Option<String>, c: Option<String>) {
        let losing_a_key = (self.p.is_some() && p.is_none()) || (self.n.is_some() && n.is_none()) || (self.c.is_some() && c.is_none());
        if losing_a_key {
            self.clear(out);
        }
        if p != self.p {
            if let Some(v) = &p {
                writeln!(out, "%TO.P,{v}*%").unwrap();
            }
            self.p = p;
        }
        if n != self.n {
            if let Some(v) = &n {
                writeln!(out, "%TO.N,{v}*%").unwrap();
            }
            self.n = n;
        }
        if c != self.c {
            if let Some(v) = &c {
                writeln!(out, "%TO.C,{v}*%").unwrap();
            }
            self.c = c;
        }
    }
}

// -------------------------------------------------------------- apertures

#[derive(Clone, Copy, PartialEq)]
enum Ap {
    Circle(Um),
    Rect(Um, Um),
    Oval(Um, Um),
    /// width, height, corner radius.
    RoundRect(Um, Um, Um),
}

/// One file's aperture table, in first-use order (KiCad's `m_apertures`,
/// `D` codes `10..`). Identity includes the aperture *function* (see
/// `GERBER_PLOTTER::GetOrCreateAperture`): a pad and an unrelated via that
/// happen to share a diameter never share a D-code, because their function
/// tags differ.
#[derive(Default)]
struct ApertureList {
    entries: Vec<(Ap, Option<&'static str>)>,
}

impl ApertureList {
    fn get(&mut self, shape: Ap, func: Option<&'static str>) -> u32 {
        if let Some(i) = self.entries.iter().position(|(s, f)| shape_eq(*s, shape) && *f == func) {
            return 10 + i as u32;
        }
        self.entries.push((shape, func));
        10 + (self.entries.len() - 1) as u32
    }
}

fn shape_eq(a: Ap, b: Ap) -> bool {
    match (a, b) {
        (Ap::Circle(x), Ap::Circle(y)) => x == y,
        (Ap::Rect(w1, h1), Ap::Rect(w2, h2)) => w1 == w2 && h1 == h2,
        (Ap::Oval(w1, h1), Ap::Oval(w2, h2)) => w1 == w2 && h1 == h2,
        (Ap::RoundRect(w1, h1, r1), Ap::RoundRect(w2, h2, r2)) => w1 == w2 && h1 == h2 && r1 == r2,
        _ => false,
    }
}

/// `%AMRoundRect*...*%` -- verbatim, checked against a real `kicad-cli`
/// Gerber file byte for byte. Written once per file, only if the file uses
/// at least one round-rect aperture.
const ROUNDRECT_MACRO: &str = "%AMRoundRect*\n\
0 Rectangle with rounded corners*\n\
0 $1 Rounding radius*\n\
0 $2 $3 $4 $5 $6 $7 $8 $9 X,Y pos of 4 corners*\n\
0 Add a 4 corners polygon primitive as box body*\n\
4,1,4,$2,$3,$4,$5,$6,$7,$8,$9,$2,$3,0*\n\
0 Add four circle primitives for the rounded corners*\n\
1,1,$1+$1,$2,$3*\n\
1,1,$1+$1,$4,$5*\n\
1,1,$1+$1,$6,$7*\n\
1,1,$1+$1,$8,$9*\n\
0 Add four rect primitives between the rounded corners*\n\
20,1,$1+$1,$2,$3,$4,$5,0*\n\
20,1,$1+$1,$4,$5,$6,$7,0*\n\
20,1,$1+$1,$6,$7,$8,$9,0*\n\
20,1,$1+$1,$8,$9,$2,$3,0*%\n";

/// `GERBER_PLOTTER::writeApertureList`: the `G04 APERTURE LIST*` comment
/// block, the round-rect macro (if used), then one `%ADD<n><shape>...*%`
/// per aperture in allocation order, each wrapped in its own
/// `%TA.AperFunction,x*%` / `%TD*%` when it has a function -- see the
/// module doc for why that wrap repeats even for two consecutive apertures
/// sharing a function.
fn write_aperture_list(out: &mut String, apertures: &ApertureList) {
    out.push_str("G04 APERTURE LIST*\n");
    if apertures.entries.iter().any(|(a, _)| matches!(a, Ap::RoundRect(..))) {
        out.push_str("G04 Aperture macros list*\n");
        out.push_str(ROUNDRECT_MACRO);
        out.push_str("G04 Aperture macros list end*\n");
    }
    for (i, (shape, func)) in apertures.entries.iter().enumerate() {
        let d = 10 + i as u32;
        if let Some(f) = func {
            writeln!(out, "%TA.AperFunction,{f}*%").unwrap();
        }
        match shape {
            Ap::Circle(dia) => writeln!(out, "%ADD{d}C,{}*%", mm6(*dia)).unwrap(),
            Ap::Rect(w, h) => writeln!(out, "%ADD{d}R,{}X{}*%", mm6(*w), mm6(*h)).unwrap(),
            Ap::Oval(w, h) => writeln!(out, "%ADD{d}O,{}X{}*%", mm6(*w), mm6(*h)).unwrap(),
            Ap::RoundRect(w, h, r) => {
                // `GERBER_PLOTTER::writeApertureList`'s `AM_ROUND_RECT`
                // case: half_size = size/2 - radius (clamped off zero),
                // four corners in order (-x,-y)(x,-y)(x,y)(-x,y), a
                // trailing "0" (the macro's own rotation -- always 0 here,
                // since the corners are already axis-aligned; see the
                // module doc on why this writer never needs a rotated
                // aperture).
                let hx = (w / 2 - r).max(1);
                let hy = (h / 2 - r).max(1);
                write!(out, "%ADD{d}RoundRect,{}X", mm6(*r)).unwrap();
                for (cx, cy) in [(-hx, -hy), (hx, -hy), (hx, hy), (-hx, hy)] {
                    write!(out, "{}X{}X", mm6(cx), mm6(cy)).unwrap();
                }
                writeln!(out, "0*%").unwrap();
            }
        }
        if func.is_some() {
            out.push_str("%TD*%\n");
        }
    }
    out.push_str("G04 APERTURE END LIST*\n");
}

// ------------------------------------------------------------------ pads

/// A footprint's pad, already placed in board space -- this module's own
/// minimal re-derivation of `eda_model::footprint::placed_pads`'s geometry
/// (same formula, see `rotated_extent` below), kept local rather than
/// extending that shared struct, which other work in this tree is editing
/// concurrently and which does not carry `PadKind`/drill fields a Gerber
/// (and, in `drill.rs`, Excellon) writer needs.
pub(crate) struct PlacedPadFull {
    pub number: String,
    pub center: Point,
    pub size: (Um, Um),
    pub shape: PadShape,
    pub roundrect_ratio: Option<f64>,
    pub kind: PadKind,
    pub drill: Option<Um>,
    pub drill_slot: Option<(Um, Um)>,
}

/// `rotated_extent_by` (`eda_model::footprint`, private to that crate):
/// axis-aligned board-space extent of a local (w,h) box under a rotation.
/// Exact at 0/90/180/270 degrees, which is the only case this model's
/// placer/router ever produces -- see that function's own doc comment.
pub(crate) fn rotated_extent(rot_millideg: i64, size: (Um, Um)) -> (Um, Um) {
    let rad = (rot_millideg as f64) / 1000.0 * std::f64::consts::PI / 180.0;
    let (sin, cos) = rad.sin_cos();
    let (w, h) = (size.0 as f64, size.1 as f64);
    ((w * cos.abs() + h * sin.abs()).round() as Um, (w * sin.abs() + h * cos.abs()).round() as Um)
}

/// Every pad of `fp`, sorted by pad number -- the same order
/// `eda_kicad::pcb::write_footprint` writes them in, so a D-code's
/// first-use order here matches the order `kicad-cli` (reading that same
/// file) assigns them in too.
pub(crate) fn placed_pads_full(model: &ConstraintModel, part: &Part, fp: &FootprintInstance) -> Option<Vec<PlacedPadFull>> {
    let footprint = model.footprint_of(part)?;
    let mut pads: Vec<PlacedPadFull> = footprint
        .pads
        .iter()
        .map(|p| PlacedPadFull {
            number: p.number.clone(),
            center: to_board(fp, p.at),
            size: rotated_extent(fp.rot as i64 + p.rot as i64, p.size),
            shape: p.shape,
            roundrect_ratio: p.roundrect_ratio,
            kind: p.kind,
            drill: p.drill,
            drill_slot: p.drill_slot.map(|sz| rotated_extent(fp.rot as i64 + p.rot as i64, sz)),
        })
        .collect();
    pads.sort_by(|a, b| a.number.cmp(&b.number));
    Some(pads)
}

impl PlacedPadFull {
    fn corner_radius(&self) -> Um {
        match self.shape {
            PadShape::RoundRect => ((self.roundrect_ratio.unwrap_or(0.25)) * self.size.0.min(self.size.1) as f64).round() as Um,
            _ => 0,
        }
    }
    fn aperture(&self) -> Ap {
        match self.shape {
            PadShape::Circle => Ap::Circle(self.size.0),
            PadShape::Oval => Ap::Oval(self.size.0, self.size.1),
            PadShape::Rect => Ap::Rect(self.size.0, self.size.1),
            PadShape::RoundRect => Ap::RoundRect(self.size.0, self.size.1, self.corner_radius()),
        }
    }
    /// `GBR_APERTURE_ATTRIB_{SMDPAD_CUDEF,COMPONENTPAD,WASHERPAD}`
    /// (`gbr_metadata.cpp`): this model has no per-pad solder-mask-defined
    /// override, so every SMD pad is "copper defined" (`CuDef`).
    fn copper_aperture_function(&self) -> &'static str {
        match self.kind {
            PadKind::Smd => "SMDPad,CuDef",
            PadKind::ThroughHole => "ComponentPad",
            PadKind::NonPlatedHole => "WasherPad",
        }
    }
}

/// A pad is on a given side's copper/mask layer if it is through-hole
/// (plated or not: both get `("*.Cu" "*.Mask")` in `eda_kicad::pcb`'s
/// writer, i.e. both sides) or an SMD pad on that side.
fn pad_on_side(kind: PadKind, fp_side: Side, want: Side) -> bool {
    kind != PadKind::Smd || fp_side == want
}

// --------------------------------------------------------------- layers

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GerberLayer {
    /// Index into `model.board.layers` (0 = top/outer).
    Copper(usize),
    Mask(Side),
    Paste(Side),
    Silk(Side),
    EdgeCuts,
}

impl GerberLayer {
    pub fn kicad_name(&self, layers: &[String]) -> String {
        match self {
            GerberLayer::Copper(i) => layers[*i].clone(),
            GerberLayer::Mask(Side::Top) => "F.Mask".into(),
            GerberLayer::Mask(Side::Bottom) => "B.Mask".into(),
            GerberLayer::Paste(Side::Top) => "F.Paste".into(),
            GerberLayer::Paste(Side::Bottom) => "B.Paste".into(),
            GerberLayer::Silk(Side::Top) => "F.SilkS".into(),
            GerberLayer::Silk(Side::Bottom) => "B.SilkS".into(),
            GerberLayer::EdgeCuts => "Edge.Cuts".into(),
        }
    }

    /// Filename suffix (`l1_usb_mcu-F_Cu.gtl`'s `F_Cu`). KiCad's layer
    /// name with `.` replaced by `_` -- *except* silkscreen, whose
    /// internal/file-format token is still `F.SilkS`/`B.SilkS` (what
    /// `kicad_name` returns, and what this model's own `Shape`/`Text`
    /// `layer` field uses) but whose exported *filename* spells out
    /// `F_Silkscreen`/`B_Silkscreen` in KiCad 10 -- checked directly
    /// against a real `kicad-cli`-plotted board.
    fn filename_suffix(&self, layers: &[String]) -> String {
        match self {
            GerberLayer::Silk(Side::Top) => "F_Silkscreen".into(),
            GerberLayer::Silk(Side::Bottom) => "B_Silkscreen".into(),
            _ => self.kicad_name(layers).replace('.', "_"),
        }
    }

    /// `GetGerberProtelExtension` (`pcbnew/pcbplot.cpp`).
    fn extension(&self, n_copper: usize) -> String {
        match self {
            GerberLayer::Copper(0) => "gtl".into(),
            GerberLayer::Copper(i) if *i + 1 == n_copper => "gbl".into(),
            GerberLayer::Copper(i) => format!("g{}", i + 1),
            GerberLayer::Mask(Side::Top) => "gts".into(),
            GerberLayer::Mask(Side::Bottom) => "gbs".into(),
            GerberLayer::Paste(Side::Top) => "gtp".into(),
            GerberLayer::Paste(Side::Bottom) => "gbp".into(),
            GerberLayer::Silk(Side::Top) => "gto".into(),
            GerberLayer::Silk(Side::Bottom) => "gbo".into(),
            GerberLayer::EdgeCuts => "gm1".into(),
        }
    }

    /// `GetGerberFileFunctionAttribute` (`pcbnew/pcbplot.cpp`). `header`
    /// selects the form written into the Gerber file's own
    /// `%TF.FileFunction%` line (Edge.Cuts carries a `,NP` plating
    /// qualifier there); the job file's own `FilesAttributes` entry omits
    /// it -- checked against both a real Gerber file and a real
    /// `.gbrjob` from the same `kicad-cli` run.
    fn file_function(&self, n_copper: usize, header: bool) -> String {
        match self {
            GerberLayer::Copper(0) => "Copper,L1,Top".into(),
            GerberLayer::Copper(i) if *i + 1 == n_copper => format!("Copper,L{n_copper},Bot"),
            GerberLayer::Copper(i) => format!("Copper,L{},Inr", i + 1),
            GerberLayer::Mask(Side::Top) => "Soldermask,Top".into(),
            GerberLayer::Mask(Side::Bottom) => "Soldermask,Bot".into(),
            GerberLayer::Paste(Side::Top) => "Paste,Top".into(),
            GerberLayer::Paste(Side::Bottom) => "Paste,Bot".into(),
            GerberLayer::Silk(Side::Top) => "Legend,Top".into(),
            GerberLayer::Silk(Side::Bottom) => "Legend,Bot".into(),
            GerberLayer::EdgeCuts if header => "Profile,NP".into(),
            GerberLayer::EdgeCuts => "Profile".into(),
        }
    }

    /// `GetGerberFilePolarityAttribute`: `None` for Edge.Cuts (no
    /// `%TF.FilePolarity%` line at all -- checked against a real file).
    fn polarity(&self) -> Option<&'static str> {
        match self {
            GerberLayer::Mask(_) => Some("Negative"),
            GerberLayer::EdgeCuts => None,
            _ => Some("Positive"),
        }
    }
}

/// The JLCPCB-ready default set `dialog_plot.cpp`'s own "Plot" default
/// selection covers for a board this model can describe: every copper
/// layer, both mask layers, both paste layers, both silkscreen layers and
/// the board outline. (KiCad's own full default also includes
/// Adhesive/Fab/Courtyard/Margin -- assembly and fab-house review aids a
/// factory does not read off a Gerber -- which JLCPCB's own upload
/// instructions do not ask for either.)
pub fn default_jlc_layers(n_copper: usize) -> Vec<GerberLayer> {
    let mut v: Vec<GerberLayer> = (0..n_copper).map(GerberLayer::Copper).collect();
    v.push(GerberLayer::Mask(Side::Top));
    v.push(GerberLayer::Mask(Side::Bottom));
    v.push(GerberLayer::Paste(Side::Top));
    v.push(GerberLayer::Paste(Side::Bottom));
    v.push(GerberLayer::Silk(Side::Top));
    v.push(GerberLayer::Silk(Side::Bottom));
    v.push(GerberLayer::EdgeCuts);
    v
}

pub struct PlottedFile {
    pub layer: GerberLayer,
    pub filename: String,
    pub file_function: String,
    pub polarity: Option<&'static str>,
    pub content: String,
}

/// Plot every layer in `layers` for `design`/`model`, in the order given.
pub fn plot_all(design: &Design, model: &ConstraintModel, meta: &FabMeta, layers: &[GerberLayer]) -> Result<Vec<PlottedFile>, Vec<CheckResult>> {
    let Some(pl) = &design.placement else {
        return Err(vec![CheckResult::fail("fab.gerber", "design", "design has no placement: nothing to plot")]);
    };
    let parts_by_ref: BTreeMap<&str, &Part> = model.parts.iter().map(|p| (p.reference.as_str(), p)).collect();
    let mut footprints: Vec<&FootprintInstance> = pl.footprints.iter().collect();
    footprints.sort_by(|a, b| a.id.cmp(&b.id));

    let mut fp_pads: Vec<(&FootprintInstance, &Part, Vec<PlacedPadFull>)> = Vec::new();
    for fp in &footprints {
        let Some(part) = parts_by_ref.get(fp.id.as_str()) else {
            return Err(vec![CheckResult::fail("fab.gerber", fp.id.clone(), "footprint has no matching part")]);
        };
        let Some(pads) = placed_pads_full(model, part, fp) else {
            return Err(vec![CheckResult::fail("fab.gerber", fp.id.clone(), "part has no resolvable footprint geometry")]);
        };
        fp_pads.push((fp, part, pads));
    }

    let n_copper = model.board.layers.len().max(2);
    let drc_board = design.routing.as_ref().map(|_| eda_drc::board::build(design, model));
    let fills = drc_board.as_ref().map(|b| eda_drc::fill::fill_all_zones(b, &model.board));

    let mut out = Vec::new();
    for layer in layers {
        let mut apertures = ApertureList::default();
        let body = match layer {
            GerberLayer::Copper(idx) => plot_copper(design, model, &fp_pads, *idx, &mut apertures, fills.as_ref()),
            GerberLayer::Mask(side) => plot_pad_only_layer(&fp_pads, &mut apertures, *side, PadLayerKind::Mask),
            GerberLayer::Paste(side) => plot_pad_only_layer(&fp_pads, &mut apertures, *side, PadLayerKind::Paste),
            GerberLayer::Silk(side) => plot_silk(design, *side, &mut apertures),
            GerberLayer::EdgeCuts => plot_edge_cuts(design, pl, &mut apertures),
        };

        let mut content = String::new();
        write_header(&mut content, meta, *layer, n_copper);
        content.push_str("G01*\n");
        write_aperture_list(&mut content, &apertures);
        content.push_str(&body);
        content.push_str("M02*\n");

        out.push(PlottedFile {
            layer: *layer,
            filename: format!("{}-{}.{}", meta.title, layer.filename_suffix(&model.board.layers), layer.extension(n_copper)),
            file_function: layer.file_function(n_copper, false),
            polarity: layer.polarity(),
            content,
        });
    }
    Ok(out)
}

fn write_header(out: &mut String, meta: &FabMeta, layer: GerberLayer, n_copper: usize) {
    writeln!(out, "%TF.GenerationSoftware,EdaFab,eda-fab,{}*%", meta.generator_version).unwrap();
    writeln!(out, "%TF.CreationDate,{}*%", meta.date).unwrap();
    writeln!(out, "%TF.ProjectId,{},{},{}*%", meta.title, meta.project_guid(), meta.rev).unwrap();
    out.push_str("%TF.SameCoordinates,Original*%\n");
    writeln!(out, "%TF.FileFunction,{}*%", layer.file_function(n_copper, true)).unwrap();
    if let Some(p) = layer.polarity() {
        writeln!(out, "%TF.FilePolarity,{p}*%").unwrap();
    }
    out.push_str("%FSLAX46Y46*%\n");
    out.push_str("G04 Gerber Fmt 4.6, Leading zero omitted, Abs format (unit mm)*\n");
    writeln!(out, "G04 Created by eda-fab ({}) date {}*", meta.generator_version, meta.date).unwrap();
    out.push_str("%MOMM*%\n");
    out.push_str("%LPD*%\n");
}

enum PadLayerKind {
    Mask,
    Paste,
}

/// Mask/Paste: one footprint-level `%TO.C,ref*%` per footprint (never
/// `%TO.P`/`%TO.N` -- these layers carry no per-pin/per-net meaning), no
/// aperture function at all (checked against a real `kicad-cli` mask/paste
/// file: its aperture list has no `%TA.AperFunction%` lines whatsoever).
fn plot_pad_only_layer(fp_pads: &[(&FootprintInstance, &Part, Vec<PlacedPadFull>)], apertures: &mut ApertureList, side: Side, kind: PadLayerKind) -> String {
    let mut out = String::new();
    let mut cur_dcode: Option<u32> = None;
    let mut attrs = AttrState::default();
    for (fp, part, pads) in fp_pads {
        let included: Vec<&PlacedPadFull> = pads
            .iter()
            .filter(|p| match kind {
                PadLayerKind::Mask => pad_on_side(p.kind, fp.side, side),
                PadLayerKind::Paste => p.kind == PadKind::Smd && fp.side == side,
            })
            .collect();
        if included.is_empty() {
            continue;
        }
        for pad in &included {
            let d = apertures.get(pad.aperture(), None);
            if cur_dcode != Some(d) {
                writeln!(out, "D{d}*").unwrap();
                cur_dcode = Some(d);
            }
            attrs.apply(&mut out, None, None, Some(part.reference.clone()));
            flash(&mut out, pad.center);
        }
    }
    attrs.clear(&mut out);
    out
}

/// Copper: footprint pads (one `StartBlock`/`EndBlock` per footprint),
/// then vias, then tracks, then zone fills -- the same order and the same
/// attribute-dictionary boundaries as `PlotStandardLayer`
/// (`pcbnew/plot_board_layers.cpp`); see the module doc.
/// "REF.PIN" -> net name, same lookup `eda_kicad::pcb` uses.
fn pin_net<'m>(model: &'m ConstraintModel, fp_id: &str, number: &str) -> Option<&'m str> {
    let key = format!("{fp_id}.{number}");
    model.nets.iter().find(|n| n.pins.iter().any(|p| p == &key)).map(|n| n.name.as_str())
}

#[allow(clippy::too_many_arguments)]
fn plot_copper(
    design: &Design,
    model: &ConstraintModel,
    fp_pads: &[(&FootprintInstance, &Part, Vec<PlacedPadFull>)],
    layer_idx: usize,
    apertures: &mut ApertureList,
    fills: Option<&eda_drc::fill::FillResults>,
) -> String {
    let layers = &model.board.layers;
    let side = if layer_idx == 0 { Side::Top } else if layer_idx + 1 == layers.len() { Side::Bottom } else { Side::Top /* inner: both-ish, treated as through-only below */ };
    let is_outer = layer_idx == 0 || layer_idx + 1 == layers.len();
    let mut out = String::new();
    let mut cur_dcode: Option<u32> = None;

    // ---- pads ----
    for (fp, _part, pads) in fp_pads {
        let included: Vec<&PlacedPadFull> = pads.iter().filter(|p| !is_outer || pad_on_side(p.kind, fp.side, side)).collect();
        if included.is_empty() {
            continue;
        }
        let mut attrs = AttrState::default();
        for pad in &included {
            let func = pad.copper_aperture_function();
            let d = apertures.get(pad.aperture(), Some(func));
            if cur_dcode != Some(d) {
                writeln!(out, "D{d}*").unwrap();
                cur_dcode = Some(d);
            }
            let p_val = Some(format!("{},{}", fp.id, pad.number));
            let n_val = Some(if pad.kind == PadKind::NonPlatedHole { String::new() } else { pin_net(model, &fp.id, &pad.number).map(str::to_string).unwrap_or_else(|| "N/C".into()) });
            attrs.apply(&mut out, p_val, n_val, None);
            flash(&mut out, pad.center);
        }
        attrs.clear(&mut out);
    }

    let Some(routing) = &design.routing else { return out };

    // ---- vias ----
    let mut attrs = AttrState::default();
    let layer_name = &layers[layer_idx];
    for v in &routing.vias {
        let (Some(fi), Some(ti)) = (layers.iter().position(|l| l == &v.from_layer), layers.iter().position(|l| l == &v.to_layer)) else { continue };
        let (lo, hi) = (fi.min(ti), fi.max(ti));
        if !(lo..=hi).contains(&layer_idx) {
            continue;
        }
        let d = apertures.get(Ap::Circle(v.diameter), Some("ViaPad"));
        if cur_dcode != Some(d) {
            writeln!(out, "D{d}*").unwrap();
            cur_dcode = Some(d);
        }
        attrs.apply(&mut out, None, Some(v.net.clone()), None);
        flash(&mut out, v.at);
    }
    attrs.clear(&mut out);

    // ---- tracks ----
    let mut attrs = AttrState::default();
    for t in &routing.tracks {
        if &t.layer != layer_name {
            continue;
        }
        let d = apertures.get(Ap::Circle(t.width), Some("Conductor"));
        if cur_dcode != Some(d) {
            writeln!(out, "D{d}*").unwrap();
            cur_dcode = Some(d);
        }
        attrs.apply(&mut out, None, Some(t.net.clone()), None);
        for pair in t.pts.windows(2) {
            move_to(&mut out, pair[0]);
            line_to(&mut out, pair[1]);
        }
    }
    attrs.clear(&mut out);

    // ---- zone fills ----
    // `PlotZone` wraps *every* fragment in its own StartBlock/EndBlock;
    // because `EndBlock` is a no-op on an empty dictionary (see the module
    // doc), this collapses to one `%TD*%` per fragment whose net actually
    // got printed -- exactly what the outer `StartBlock`/`EndBlock` pair
    // around the whole zone loop in `plot_board_layers.cpp` plus that
    // per-zone inner pair together produce.
    let mut attrs = AttrState::default();
    for z in &routing.zones {
        if &z.layer != layer_name {
            continue;
        }
        let Some(fill) = fills.and_then(|f| f.get(&z.id)) else { continue };
        let func = if z.net.is_empty() { "NonConductor" } else { "Conductor" };
        for poly in &fill.polys {
            let Some(outline) = poly.first() else { continue };
            if outline.len() < 3 {
                continue;
            }
            writeln!(out, "%TA.AperFunction,{func}*%").unwrap();
            attrs.apply(&mut out, None, Some(z.net.clone()), None);
            out.push_str("G36*\n");
            let first = Point { x: outline[0].x, y: outline[0].y };
            move_to(&mut out, first);
            out.push_str("G01*\n");
            for p in &outline[1..] {
                line_to(&mut out, Point { x: p.x, y: p.y });
            }
            let last = outline.last().unwrap();
            if (last.x, last.y) != (outline[0].x, outline[0].y) {
                line_to(&mut out, first);
            }
            out.push_str("G37*\n");
            out.push_str("%TD.AperFunction*%\n");
        }
    }
    attrs.clear(&mut out);

    out
}

/// Silkscreen: free-standing `Shape`/`Text` items on this side's silk
/// layer only -- see the module doc for why footprint reference/value
/// text is not plotted here.
fn plot_silk(design: &Design, side: Side, apertures: &mut ApertureList) -> String {
    let layer_name = if side == Side::Top { "F.SilkS" } else { "B.SilkS" };
    let mut out = String::new();
    let mut cur_dcode: Option<u32> = None;
    let Some(drawings) = &design.drawings else { return out };

    let select = |out: &mut String, cur: &mut Option<u32>, apertures: &mut ApertureList, width: Um| {
        let d = apertures.get(Ap::Circle(width.max(1)), None);
        if *cur != Some(d) {
            writeln!(out, "D{d}*").unwrap();
            *cur = Some(d);
        }
    };

    for s in &drawings.shapes {
        if s.layer() != layer_name {
            continue;
        }
        match s {
            Shape::Segment { stroke_width, start, end, .. } => {
                select(&mut out, &mut cur_dcode, apertures, *stroke_width);
                move_to(&mut out, *start);
                line_to(&mut out, *end);
            }
            Shape::Arc { stroke_width, start, mid, end, .. } => {
                select(&mut out, &mut cur_dcode, apertures, *stroke_width);
                move_to(&mut out, *start);
                for p in flatten_arc(*start, *mid, *end) {
                    line_to(&mut out, p);
                }
            }
            Shape::Rect { stroke_width, filled, start, end, .. } => {
                let pts = [*start, Point { x: end.x, y: start.y }, *end, Point { x: start.x, y: end.y }];
                plot_closed_poly(&mut out, &pts, *filled, *stroke_width, &mut cur_dcode, apertures);
            }
            Shape::Circle { stroke_width, filled, center, end, .. } => {
                let r = (((end.x - center.x).pow(2) + (end.y - center.y).pow(2)) as f64).sqrt();
                let pts: Vec<Point> = (0..64)
                    .map(|i| {
                        let a = i as f64 / 64.0 * std::f64::consts::TAU;
                        Point { x: center.x + (r * a.cos()).round() as Um, y: center.y + (r * a.sin()).round() as Um }
                    })
                    .collect();
                plot_closed_poly(&mut out, &pts, *filled, *stroke_width, &mut cur_dcode, apertures);
            }
            Shape::Polygon { stroke_width, filled, pts, .. } => {
                plot_closed_poly(&mut out, pts, *filled, *stroke_width, &mut cur_dcode, apertures);
            }
        }
    }

    for t in &drawings.texts {
        if t.layer != layer_name {
            continue;
        }
        let Some(strokes) = eda_drc::stroke_font::layout_strokes(&t.content, t.at, t.size_um, t.angle as i64, t.mirror, t.justify) else { continue };
        select(&mut out, &mut cur_dcode, apertures, t.stroke_width);
        for seg in strokes {
            move_to(&mut out, seg.a);
            line_to(&mut out, seg.b);
        }
    }

    out
}

/// Flatten a 3-point (start/mid/end) arc to short line segments, for a
/// Gerber writer that only ever draws straight `D01` moves on a
/// silkscreen shape. Returns the points from just after `start` through
/// `end` inclusive -- the caller has already moved its pen to `start`.
/// Degrades to a single segment to `end` if the three points are
/// (near-)collinear.
fn flatten_arc(start: Point, mid: Point, end: Point) -> Vec<Point> {
    let (ax, ay, bx, by, cx, cy) = (start.x as f64, start.y as f64, mid.x as f64, mid.y as f64, end.x as f64, end.y as f64);
    let d = 2.0 * (ax * (by - cy) + bx * (cy - ay) + cx * (ay - by));
    if d.abs() < 1e-6 {
        return vec![end];
    }
    let ux = ((ax * ax + ay * ay) * (by - cy) + (bx * bx + by * by) * (cy - ay) + (cx * cx + cy * cy) * (ay - by)) / d;
    let uy = ((ax * ax + ay * ay) * (cx - bx) + (bx * bx + by * by) * (ax - cx) + (cx * cx + cy * cy) * (bx - ax)) / d;
    let r = ((ax - ux).powi(2) + (ay - uy).powi(2)).sqrt();
    const TAU: f64 = std::f64::consts::TAU;
    let norm = |a: f64| a.rem_euclid(TAU);
    let ang = |x: f64, y: f64| norm((y - uy).atan2(x - ux));
    let (a0, a1, a2) = (ang(ax, ay), ang(bx, by), ang(cx, cy));
    // Sweep counter-clockwise from a0; pick whichever of the two possible
    // sweeps (positive or the "long way round", i.e. negative) actually
    // passes through the mid angle a1.
    let ccw_to_1 = norm(a1 - a0);
    let ccw_to_2 = norm(a2 - a0);
    let sweep = if ccw_to_1 <= ccw_to_2 { ccw_to_2 } else { ccw_to_2 - TAU };
    let steps = (sweep.abs() / (std::f64::consts::PI / 24.0)).ceil().max(1.0) as usize;
    (1..=steps)
        .map(|i| {
            let a = a0 + sweep * (i as f64 / steps as f64);
            Point { x: (ux + r * a.cos()).round() as Um, y: (uy + r * a.sin()).round() as Um }
        })
        .collect()
}

fn plot_closed_poly(out: &mut String, pts: &[Point], filled: bool, stroke_width: Um, cur_dcode: &mut Option<u32>, apertures: &mut ApertureList) {
    if pts.len() < 2 {
        return;
    }
    if filled && pts.len() >= 3 {
        out.push_str("G36*\n");
        move_to(out, pts[0]);
        out.push_str("G01*\n");
        for p in &pts[1..] {
            line_to(out, *p);
        }
        if pts.last() != Some(&pts[0]) {
            line_to(out, pts[0]);
        }
        out.push_str("G37*\n");
    } else {
        let d = apertures.get(Ap::Circle(stroke_width.max(1)), None);
        if *cur_dcode != Some(d) {
            writeln!(out, "D{d}*").unwrap();
            *cur_dcode = Some(d);
        }
        move_to(out, pts[0]);
        for p in &pts[1..] {
            line_to(out, *p);
        }
        if pts.len() > 2 && pts.last() != Some(&pts[0]) {
            line_to(out, pts[0]);
        }
    }
}

/// Edge.Cuts: the board outline, plus any free `Shape` items a caller put
/// directly on that layer. Line width: this model's outline carries no
/// stroke of its own (`eda_kicad::pcb`'s writer emits a bare `gr_line`
/// with none), matching KiCad's own fallback for a width-less edge
/// segment -- checked against a real `kicad-cli`-plotted `.gm1`, whose
/// sole aperture is `C,0.100000`.
const EDGE_CUTS_WIDTH_UM: Um = 100;

fn plot_edge_cuts(design: &Design, pl: &eda_model::ir::PlacementSection, apertures: &mut ApertureList) -> String {
    let mut out = String::new();
    let d = apertures.get(Ap::Circle(EDGE_CUTS_WIDTH_UM), Some("Profile"));
    writeln!(out, "D{d}*").unwrap();
    if pl.outline.len() >= 2 {
        let n = pl.outline.len();
        move_to(&mut out, pl.outline[0]);
        for i in 1..=n {
            line_to(&mut out, pl.outline[i % n]);
        }
    }
    if let Some(drawings) = &design.drawings {
        for s in &drawings.shapes {
            if s.layer() != "Edge.Cuts" {
                continue;
            }
            let pts = s.points();
            if pts.len() >= 2 {
                move_to(&mut out, pts[0]);
                for p in &pts[1..] {
                    line_to(&mut out, *p);
                }
            }
        }
    }
    out
}
