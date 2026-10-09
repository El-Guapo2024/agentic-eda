//! Where the fields of a placed symbol, a power symbol and a sheet are drawn.
//!
//! KiCad keeps a position, an angle, a justification and a visibility for every field of every instance (`SCH_FIELD`). The IR keeps
//! them in [`FieldPlacement`]s (`SchematicSection::field_layout`), in the frame of the unturned item, so a move, a turn or a mirror
//! carries them along as it carries the pins. This module places them the way Autoplace Fields does
//! (`eeschema/autoplace_fields.cpp`, `AUTOPLACER::DoAutoplace` in its "auto" mode: no collision search, fields to the side of the
//! symbol that has no pins, else the side with the fewest, always horizontal), and turns a placement into where the text is on the
//! sheet (`SCH_FIELD::GetPosition`, `GetBoundingBox`: the text box carried through the symbol's transform).
//!
//! The arithmetic is KiCad's, in schematic internal units (0.1 um) with C++'s truncating division, so a field lands where KiCad
//! would put it.

use eda_model::ir::{field_key, FieldPlacement, Point, PowerSymbol, SchematicSection, SheetInstance, SymbolInstance, TextJustify, TextVAlign};
use eda_model::kicad_font::{default_pen_iu, kiround, text_box_iu, HJustify, VJustify, DEFAULT_TEXT_SIZE_IU};
use eda_model::kicad_geom::{pin_bbox, PinOrient, Rect, TextStyle};
use eda_model::symbol::LibSymbol;
use eda_model::Part;

use crate::symgeom::{bake, unbake, SymbolGeom};

/// One field of an item: its name, the text it shows, whether it is drawn, and how its text is set (what Autoplace Fields measures it by).
#[derive(Debug, Clone, PartialEq)]
pub struct FieldSpec {
    pub name: String,
    pub text: String,
    pub visible: bool,
    /// The text's size, micrometres; 0 is the default, 50 mil.
    pub size_um: i64,
    pub bold: bool,
    pub italic: bool,
    pub name_shown: bool,
    /// `SCH_FIELD::CanAutoplace`: Autoplace Fields leaves a field that does not allow it where it is.
    pub can_autoplace: bool,
}

impl FieldSpec {
    pub fn new(name: &str, text: String, visible: bool) -> FieldSpec {
        FieldSpec { name: name.to_string(), text, visible, size_um: 0, bold: false, italic: false, name_shown: false, can_autoplace: true }
    }

    /// The same field as a stored placement sets it: visibility, size, bold, italic, shown name and whether it may be autoplaced.
    pub fn styled_by(mut self, p: &FieldPlacement) -> FieldSpec {
        self.visible = p.visible;
        self.size_um = p.size_um;
        self.bold = p.bold;
        self.italic = p.italic;
        self.name_shown = p.name_shown;
        self.can_autoplace = !p.no_autoplace;
        self
    }

    /// `SCH_FIELD::GetShownText`.
    pub fn shown_text(&self) -> String {
        if self.name_shown {
            format!("{}: {}", self.name, self.text)
        } else {
            self.text.clone()
        }
    }
}

/// A field on the sheet: what the `.kicad_sch` writer emits and the painter draws. The anchor is in micrometres; `vertical` is the
/// text's angle on the sheet (90 degrees); `h` and `v` justify the text against the anchor in the text's own axes, as the file does.
#[derive(Debug, Clone, PartialEq)]
pub struct PageField {
    pub name: String,
    pub text: String,
    pub at: (f64, f64),
    pub vertical: bool,
    pub h: HJustify,
    pub v: VJustify,
    pub visible: bool,
    /// The text's size (height and width), micrometres.
    pub size_um: f64,
    pub bold: bool,
    pub italic: bool,
    /// `SCH_FIELD::IsNameShown`: [`shown_text`](Self::shown_text) leads with the field's name.
    pub name_shown: bool,
    /// `!SCH_FIELD::CanAutoplace`.
    pub no_autoplace: bool,
}

impl PageField {
    /// The text as it is drawn (`SCH_FIELD::GetShownText`): `Name: value` when the name is shown.
    pub fn shown_text(&self) -> String {
        if self.name_shown {
            format!("{}: {}", self.name, self.text)
        } else {
            self.text.clone()
        }
    }

    /// How the text is set: its size, the pen (a fifth of the size when bold, `GetPenSizeForBold`) and its justification.
    pub fn style(&self) -> TextStyle {
        field_style(self.size_um, self.bold, self.h, self.v)
    }
}

/// The style a text of `size_um` (bold or not) is measured in.
pub fn field_style(size_um: f64, bold: bool, h: HJustify, v: VJustify) -> TextStyle {
    let size_iu = kiround(size_um * 10.0);
    TextStyle { size_iu, thickness_iu: if bold { kiround(size_iu as f64 / 5.0) } else { 0 }, h, v }
}

// ---------------------------------------------------------------------------------------------------------- frames

/// A 2 x 2 matrix of a rotation or mirror (entries -1, 0, 1).
#[derive(Debug, Clone, Copy, PartialEq)]
struct Lin([[i32; 2]; 2]);

impl Lin {
    fn apply(self, v: (i32, i32)) -> (i32, i32) {
        (self.0[0][0] * v.0 + self.0[0][1] * v.1, self.0[1][0] * v.0 + self.0[1][1] * v.1)
    }
    fn mul(self, o: Lin) -> Lin {
        let a = self.0;
        let b = o.0;
        Lin([[a[0][0] * b[0][0] + a[0][1] * b[1][0], a[0][0] * b[0][1] + a[0][1] * b[1][1]], [a[1][0] * b[0][0] + a[1][1] * b[1][0], a[1][0] * b[0][1] + a[1][1] * b[1][1]]])
    }
    /// A rotation or mirror's inverse is its transpose.
    fn transpose(self) -> Lin {
        Lin([[self.0[0][0], self.0[1][0]], [self.0[0][1], self.0[1][1]]])
    }
    /// The turn and mirror of a placed symbol (`placed::bake_offset` without the translation): the mirror first, about the box's vertical
    /// axis, then the turn.
    fn of_symbol(rot_millideg: u32, mirrored: bool) -> Lin {
        let m = if mirrored { Lin([[-1, 0], [0, 1]]) } else { Lin([[1, 0], [0, 1]]) };
        let r = match rot_millideg % 360_000 {
            90_000 => Lin([[0, -1], [1, 0]]),
            180_000 => Lin([[-1, 0], [0, -1]]),
            270_000 => Lin([[0, 1], [-1, 0]]),
            _ => Lin([[1, 0], [0, 1]]),
        };
        r.mul(m)
    }
    /// A text turned a quarter counter-clockwise on the sheet (`RotatePoint`: (x, y) goes to (y, -x)).
    fn text_turn(vertical: bool) -> Lin {
        if vertical {
            Lin([[0, 1], [-1, 0]])
        } else {
            Lin([[1, 0], [0, 1]])
        }
    }
    fn swaps_axes(self) -> bool {
        self.0[0][0] == 0
    }
}

/// Which way a justified text's box lies from its anchor: left-justified runs to +x, top-justified to +y.
fn just_dir(h: TextJustify, v: TextVAlign) -> (i32, i32) {
    (
        match h {
            TextJustify::Left => 1,
            TextJustify::Center => 0,
            TextJustify::Right => -1,
        },
        match v {
            TextVAlign::Top => 1,
            TextVAlign::Center => 0,
            TextVAlign::Bottom => -1,
        },
    )
}

fn dir_just(d: (i32, i32)) -> (TextJustify, TextVAlign) {
    (
        match d.0.signum() {
            1 => TextJustify::Left,
            -1 => TextJustify::Right,
            _ => TextJustify::Center,
        },
        match d.1.signum() {
            1 => TextVAlign::Top,
            -1 => TextVAlign::Bottom,
            _ => TextVAlign::Center,
        },
    )
}

pub fn to_hj(h: TextJustify) -> HJustify {
    match h {
        TextJustify::Left => HJustify::Left,
        TextJustify::Center => HJustify::Center,
        TextJustify::Right => HJustify::Right,
    }
}

pub fn to_vj(v: TextVAlign) -> VJustify {
    match v {
        TextVAlign::Top => VJustify::Top,
        TextVAlign::Center => VJustify::Center,
        TextVAlign::Bottom => VJustify::Bottom,
    }
}

/// A placement in the item's own frame on the sheet: `at` is the item's origin, `rot`/`mirrored` how it is turned, `width` the box a
/// mirror is about (0 for an item with its origin at its anchor).
pub fn page_field(at: Point, rot: u32, mirrored: bool, width: f64, p: &FieldPlacement, text: &str) -> PageField {
    let (ox, oy) = bake(rot, mirrored, width, p.dx as f64, p.dy as f64);
    let m = Lin::of_symbol(rot, mirrored).mul(Lin::text_turn(p.angle == 90_000));
    let vertical = m.apply((1, 0)).0 == 0;
    let d_page = m.apply(just_dir(p.h, p.v));
    // a vertical text reads along the page's y axis: its own axes are the page's turned back a quarter
    let d_text = if vertical { (-d_page.1, d_page.0) } else { d_page };
    let (h, v) = dir_just(d_text);
    PageField {
        name: p.name.clone(),
        text: text.to_string(),
        at: (at.x as f64 + ox, at.y as f64 + oy),
        vertical,
        h: to_hj(h),
        v: to_vj(v),
        visible: p.visible,
        size_um: p.text_size_um() as f64,
        bold: p.bold,
        italic: p.italic,
        name_shown: p.name_shown,
        no_autoplace: p.no_autoplace,
    }
}

/// The reverse: a field placed horizontally on the sheet (what autoplace produces) as a placement in the item's own frame.
pub fn local_placement(at: Point, rot: u32, mirrored: bool, width: f64, name: &str, anchor: (f64, f64), h: TextJustify, v: TextVAlign, visible: bool) -> FieldPlacement {
    local_placement_on_page(at, rot, mirrored, width, name, anchor, false, h, v, visible)
}

/// The reverse of [`page_field`] for a text of either orientation: the field named `name` with its anchor at `anchor` on the sheet, running
/// along the sheet's y axis when `page_vertical`, justified `h` and `v` in the text's own axes (what the file says and `SCH_FIELD::
/// GetEffectiveHorizJustify` reads), as a placement in the item's own frame. This is `SCH_FIELD::SetPosition` (the inverse of the symbol's
/// transform) and the angle and justification that, once the symbol's transform has turned them, read as asked. Size and the flags of the
/// placement are the caller's to fill.
#[allow(clippy::too_many_arguments)]
pub fn local_placement_on_page(at: Point, rot: u32, mirrored: bool, width: f64, name: &str, anchor: (f64, f64), page_vertical: bool, h: TextJustify, v: TextVAlign, visible: bool) -> FieldPlacement {
    let (lx, ly) = unbake(rot, mirrored, width, anchor.0 - at.x as f64, anchor.1 - at.y as f64);
    let l = Lin::of_symbol(rot, mirrored);
    // the stored angle that, with the symbol's own turn, shows the text as asked (KiCad's autoplace stores the one that shows it horizontally)
    let stored_vertical = page_vertical != l.swaps_axes();
    let m = l.mul(Lin::text_turn(stored_vertical));
    // the direction the text's box lies from its anchor, on the sheet: a vertical text's own axes are the sheet's turned back a quarter
    let d_text = just_dir(h, v);
    let d_page = if page_vertical { (d_text.1, -d_text.0) } else { d_text };
    let d_local = m.transpose().apply(d_page);
    let (lh, lv) = dir_just(d_local);
    FieldPlacement { name: name.to_string(), dx: lx.round() as i64, dy: ly.round() as i64, angle: if stored_vertical { 90_000 } else { 0 }, h: lh, v: lv, visible, ..FieldPlacement::at_origin(name) }
}

// ------------------------------------------------------------------------------------------------------ specs

/// The four fields of a placed symbol, in KiCad's order, with the text each shows.
pub fn symbol_specs(sym: &SymbolInstance, part: Option<&Part>, datasheet_of_lib: &str) -> Vec<FieldSpec> {
    let value = if !sym.value.is_empty() { sym.value.clone() } else { part.and_then(|p| p.value.clone()).unwrap_or_else(|| sym.id.clone()) };
    let footprint = if !sym.footprint.is_empty() { sym.footprint.clone() } else { part.and_then(|p| p.footprint.clone()).unwrap_or_default() };
    let datasheet = if !sym.datasheet.is_empty() { sym.datasheet.clone() } else { datasheet_of_lib.to_string() };
    vec![
        FieldSpec::new("Reference", sym.id.clone(), true),
        FieldSpec::new("Value", value, true),
        FieldSpec::new("Footprint", footprint, false),
        FieldSpec::new("Datasheet", datasheet, false),
    ]
}

/// `specs` with the visibility and text setting the placements `stored` give each field (a field `stored` says nothing of keeps its default).
pub fn specs_styled(specs: Vec<FieldSpec>, stored: Option<&[FieldPlacement]>) -> Vec<FieldSpec> {
    match stored {
        Some(st) => specs.into_iter().map(|sp| match st.iter().find(|p| p.name == sp.name) {
            Some(p) => sp.styled_by(p),
            None => sp,
        }).collect(),
        None => specs,
    }
}

// ------------------------------------------------------------------------------------------------------ autoplace

const MIL_IU: i64 = 254;
const HPADDING: i64 = 25 * MIL_IU;
const VPADDING: i64 = 15 * MIL_IU;
const GRID_IU: i64 = 50 * MIL_IU;

/// `round_n`: round up or down to a multiple of `n`, with C++'s truncating division.
fn round_n(value: i64, n: i64, up: bool) -> i64 {
    if value % n != 0 {
        n * (value / n + if up { 1 } else { 0 })
    } else {
        value
    }
}

fn iu(um: f64) -> i64 {
    (um * 10.0).round() as i64
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Side {
    Right,
    Top,
    Left,
    Bottom,
}

impl Side {
    fn vec(self) -> (i64, i64) {
        match self {
            Side::Right => (1, 0),
            Side::Top => (0, -1),
            Side::Left => (-1, 0),
            Side::Bottom => (0, 1),
        }
    }
}

/// `getPinSide`: a pin that runs right from its tip is on the left of the symbol.
fn pin_side(o: PinOrient) -> Side {
    match o {
        PinOrient::Right => Side::Left,
        PinOrient::Left => Side::Right,
        PinOrient::Up => Side::Bottom,
        PinOrient::Down => Side::Top,
    }
}

/// What an Autoplace Fields run in manual mode knows of the sheet around the symbol (`AUTOPLACER::m_colliders`, `getDrawableArea`).
#[derive(Debug, Clone, Default)]
pub struct Surroundings {
    /// Boxes of what is drawn around the symbol, micrometres: every item but the symbol itself (a symbol by its body and pins, wires, labels,
    /// text, sheets, shapes) and the shown fields of the other symbols.
    pub colliders: Vec<Collider>,
    /// The sheet inside its border, micrometres; `None` for no limit.
    pub drawable: Option<crate::hier::kit::Rect>,
}

/// One item the fields must keep clear of.
#[derive(Debug, Clone)]
pub struct Collider {
    pub rect: crate::hier::kit::Rect,
    /// Set for a wire or a bus: the y of its two ends, micrometres (`SCH_LINE::GetStartPoint().y`, `GetEndPoint().y`).
    pub wire: Option<(i64, i64)>,
}

/// `AUTOPLACER::COLLISION`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Collision {
    Objects,
    HWires,
}

const WIRE_V_SPACING: i64 = 100 * MIL_IU;

/// A box in internal units: x0, y0, x1, y1.
type IuBox = [i64; 4];

fn iu_box(r: crate::hier::kit::Rect) -> IuBox {
    [r.x0 * 10, r.y0 * 10, r.x1 * 10, r.y1 * 10]
}

/// `BOX2I::Intersects`: the boxes share some area.
fn boxes_intersect(a: IuBox, b: IuBox) -> bool {
    a[0] < b[2] && b[0] < a[2] && a[1] < b[3] && b[1] < a[3]
}

/// `chooseSideFiltered`: the sides that collide the way `which` says leave the list, and the one of them with the fewest pins (the later of equals)
/// is kept as the fall-back.
fn choose_side_filtered(sides: &mut Vec<(Side, usize)>, colliding: &[(Side, Collision)], which: Collision, last: (Side, u64)) -> (Side, u64) {
    let mut sel = last;
    let mut i = 0;
    while i < sides.len() {
        let collide = colliding.iter().any(|(s, c)| *s == sides[i].0 && *c == which);
        if !collide {
            i += 1;
            continue;
        }
        if (sides[i].1 as u64) <= sel.1 {
            sel = (sides[i].0, sides[i].1 as u64);
        }
        sides.remove(i);
    }
    sel
}

/// A field of the batch being placed, measured: the text as shown and the pen it is drawn with.
struct Measured {
    text: String,
    size_iu: i64,
    pen_iu: i64,
}

fn measure(f: &FieldSpec) -> Measured {
    let size_iu = if f.size_um > 0 { f.size_um * 10 } else { DEFAULT_TEXT_SIZE_IU };
    let pen_iu = if f.bold { kiround(size_iu as f64 / 5.0) } else { default_pen_iu(size_iu) };
    Measured { text: f.shown_text(), size_iu, pen_iu }
}

/// Autoplace Fields for one symbol (`AUTOPLACER::DoAutoplace`): the anchor and the justification of each of `fields` (the visible ones that
/// allow it) on the sheet, horizontal text. Without `manual` this is the automatic mode (the side with no pins, else the one with the fewest);
/// with it the manual one: the sides where the fields would run over something drawn are left for last (`getCollidingSides`), and above or below
/// a symbol between horizontal wires the fields are spaced to fit between them (`fitFieldsBetweenWires`). `force` puts them on a side of the
/// symbol KiCad would not have chosen, `push` grid steps (50 mil) further from it: where the schematic needs the room the symbol's own side has
/// not got.
fn autoplace_page(geom: &SymbolGeom, at: Point, mirrored: bool, fields: &[&FieldSpec], force: Option<(Side, i64)>, manual: Option<&Surroundings>) -> Vec<((f64, f64), TextJustify, TextVAlign)> {
    let metrics: Vec<Measured> = fields.iter().map(|f| measure(f)).collect();
    let sizes: Vec<(i64, i64)> = metrics
        .iter()
        .map(|m| {
            let (x0, y0, x1, y1) = text_box_iu(&m.text, m.size_iu, m.pen_iu, HJustify::Left, VJustify::Center);
            (x1 - x0, y1 - y0)
        })
        .collect();
    let fbox_w = sizes.iter().map(|s| s.0).max().unwrap_or(0);
    let fbox_h: i64 = sizes.iter().map(|s| round_n(s.1, GRID_IU, true)).sum();

    let body = geom.body.unwrap_or(Rect::point((at.x as f64, at.y as f64)));
    let (bx, by, bw, bh) = (iu(body.x0), iu(body.y0), iu(body.w()), iu(body.h()));
    let center = (bx + bw / 2, by + bh / 2);

    // the pins on each side, and the box of each side's pins
    let st = SymbolGeom::pin_style();
    let count = |s: Side| geom.pins.iter().filter(|p| pin_side(p.orient) == s).count();
    let mut order = [(Side::Right, count(Side::Right)), (Side::Top, count(Side::Top)), (Side::Left, count(Side::Left)), (Side::Bottom, count(Side::Bottom))];
    // `getPreferredSides`: a symbol mirrored left to right swaps right and left, and a long thin one prefers the top and bottom
    if mirrored {
        order.swap(0, 2);
    }
    if bh > 0 && (bw as f64) / (bh as f64) > 3.0 {
        order.swap(0, 1);
        order.swap(1, 3);
    }
    // `fieldBoxPlacement`: where the box the fields fill goes when they are on `side`, `push` grid steps further out
    let field_box_origin = |side: Side, npins: usize, push: i64| -> (i64, i64) {
        let sv = side.vec();
        let mut offs_x = (bw + fbox_w) / 2;
        let mut offs_y = (bh + fbox_h) / 2;
        if sv.0 != 0 {
            offs_x += HPADDING;
        } else if sv.1 != 0 {
            offs_y += VPADDING;
        }
        let fc = (center.0 + sv.0 * (offs_x + push * GRID_IU), center.1 + sv.1 * (offs_y + push * GRID_IU));
        let (mut fx, mut fy) = (fc.0 - fbox_w / 2, fc.1 - fbox_h / 2);
        if npins > 0 {
            let mut pins_box: Option<Rect> = None;
            for p in geom.pins.iter().filter(|p| pin_side(p.orient) == side) {
                let b = pin_bbox(p.tip, p.orient, p.length, &p.name, &p.number, &st, &st, &geom.texts);
                pins_box = Some(pins_box.map_or(b, |acc| acc.merge(b)));
            }
            if let Some(pb) = pins_box {
                if matches!(side, Side::Top | Side::Bottom) {
                    fx = iu(pb.x1) + HPADDING * 2 + push * GRID_IU;
                } else {
                    fy = iu(pb.y0) - (fbox_h + VPADDING * 2) - push * GRID_IU;
                }
            }
        }
        (fx, fy)
    };
    let fbox_at = |side: Side, npins: usize| -> IuBox {
        let (x, y) = field_box_origin(side, npins, 0);
        [x, y, x + fbox_w, y + fbox_h]
    };

    // `chooseSideForFields`: the most preferred side with no pins, else the most preferred of those with the fewest. In manual mode the sides that
    // collide with something are the last resort.
    let chosen = match force {
        Some((s, _)) => (s, count(s)),
        None => {
            let mut sides: Vec<(Side, usize)> = order.iter().rev().copied().collect();
            let mut fallback: (Side, u64) = (Side::Right, u64::MAX);
            if let Some(env) = manual {
                // `getCollidingSides`
                let mut colliding: Vec<(Side, Collision)> = Vec::new();
                for side in [Side::Right, Side::Top, Side::Left, Side::Bottom] {
                    let b = fbox_at(side, count(side));
                    let mut collision: Option<Collision> = None;
                    if let Some(area) = env.drawable {
                        let a = iu_box(area);
                        if !(a[0] <= b[0] && a[1] <= b[1] && b[2] <= a[2] && b[3] <= a[3]) {
                            collision = Some(Collision::Objects);
                        }
                    }
                    for c in env.colliders.iter().filter(|c| boxes_intersect(iu_box(c.rect), b)) {
                        collision = match c.wire {
                            Some((y0, y1)) if side.vec().0 == 0 => {
                                if y0 == y1 && collision != Some(Collision::Objects) {
                                    Some(Collision::HWires)
                                } else {
                                    Some(Collision::Objects)
                                }
                            }
                            _ => Some(Collision::Objects),
                        };
                    }
                    if let Some(c) = collision {
                        colliding.push((side, c));
                    }
                }
                fallback = choose_side_filtered(&mut sides, &colliding, Collision::Objects, fallback);
                fallback = choose_side_filtered(&mut sides, &colliding, Collision::HWires, fallback);
            }
            if let Some(&(s, _)) = sides.iter().rev().find(|(_, n)| *n == 0) {
                (s, 0)
            } else {
                for &(s, n) in sides.iter() {
                    if (n as u64) <= fallback.1 {
                        fallback = (s, n as u64);
                    }
                }
                (fallback.0, fallback.1.min(usize::MAX as u64) as usize)
            }
        }
    };
    let (side, npins) = chosen;
    let sv = side.vec();
    let push = force.map(|(_, p)| p).unwrap_or(0);

    let (fx, mut fy) = field_box_origin(side, npins, push);

    // `fitFieldsBetweenWires` (manual mode): above or below the symbol, between horizontal wires on the 100 mil grid
    let mut dynamic = true;
    if let Some(env) = manual {
        if matches!(side, Side::Top | Side::Bottom) {
            let b = [fx, fy, fx + fbox_w, fy + fbox_h];
            let hits: Vec<&Collider> = env.colliders.iter().filter(|c| boxes_intersect(iu_box(c.rect), b)).collect();
            if !hits.is_empty() {
                let mut offset = 0i64;
                let mut fits = true;
                for c in &hits {
                    let Some((y0, y1)) = c.wire else {
                        fits = false;
                        break;
                    };
                    if y0 != y1 {
                        fits = false;
                        break;
                    }
                    let this = (3 * WIRE_V_SPACING / 2) - ((y0 * 10) % WIRE_V_SPACING);
                    if offset == 0 {
                        offset = this;
                    } else if offset != this {
                        fits = false;
                        break;
                    }
                }
                if fits {
                    fy = round_n(fy, WIRE_V_SPACING, side == Side::Bottom);
                    dynamic = false;
                }
            }
        }
    }

    // move the fields
    let h_just = if npins > 0 {
        if matches!(side, Side::Top | Side::Bottom) {
            TextJustify::Left
        } else {
            TextJustify::Center
        }
    } else {
        match sv.0 {
            1 => TextJustify::Left,
            -1 => TextJustify::Right,
            _ => TextJustify::Center,
        }
    };
    let mut acc = fy;
    let mut out = Vec::new();
    for &(_, h) in sizes.iter() {
        let mut x = match h_just {
            TextJustify::Left => fx,
            TextJustify::Center => fx + fbox_w / 2,
            TextJustify::Right => fx + fbox_w,
        };
        let (padding, height) = if dynamic { (round_n(h, GRID_IU, true) - h, h) } else { (WIRE_V_SPACING / 2, WIRE_V_SPACING / 2) };
        let mut y = acc + padding / 2 + height / 2;
        acc += padding + height;
        if sv.0 != 0 {
            x = round_n(x, GRID_IU, sv.0 >= 0);
        }
        if sv.1 != 0 {
            y = round_n(y, GRID_IU, sv.1 >= 0);
        }
        out.push(((x as f64 / 10.0, y as f64 / 10.0), h_just, TextVAlign::Center));
    }
    out
}

/// The placements Autoplace Fields gives a symbol's fields, in the symbol's own frame: one per spec, in order. A hidden field takes the
/// reference's place.
pub fn autoplace_symbol(sym: &SymbolInstance, geom: &SymbolGeom, specs: &[FieldSpec]) -> Vec<FieldPlacement> {
    place_fields(sym, geom, specs, &[], None, None)
}

/// [`autoplace_symbol`] in the manual mode the Autoplace Fields command runs (`AUTOPLACE_MANUAL`): the fields keep clear of `around`. A field
/// that does not allow autoplacement keeps the placement `kept` has for it.
pub fn autoplace_symbol_manual(sym: &SymbolInstance, geom: &SymbolGeom, specs: &[FieldSpec], kept: &[FieldPlacement], around: &Surroundings) -> Vec<FieldPlacement> {
    place_fields(sym, geom, specs, kept, None, Some(around))
}

/// The placements of the fields named by `specs`: the fields that are shown and allow it are placed, a field that does not allow it keeps its
/// placement in `kept`, a hidden one takes the first placed one's place.
fn place_fields(sym: &SymbolInstance, geom: &SymbolGeom, specs: &[FieldSpec], kept: &[FieldPlacement], force: Option<(Side, i64)>, manual: Option<&Surroundings>) -> Vec<FieldPlacement> {
    let movable: Vec<&FieldSpec> = specs.iter().filter(|s| s.visible && s.can_autoplace).collect();
    let placed = autoplace_page(geom, sym.at, sym.mirrored, &movable, force, manual);
    let style = |mut p: FieldPlacement, spec: &FieldSpec| {
        p.size_um = spec.size_um;
        p.bold = spec.bold;
        p.italic = spec.italic;
        p.name_shown = spec.name_shown;
        p.no_autoplace = !spec.can_autoplace;
        p
    };
    let mut by_name = std::collections::BTreeMap::new();
    for (spec, (anchor, h, v)) in movable.iter().zip(placed) {
        by_name.insert(spec.name.clone(), style(local_placement(sym.at, sym.rot, sym.mirrored, geom.width, &spec.name, anchor, h, v, true), spec));
    }
    let first = specs.iter().find(|s| s.visible && s.can_autoplace).and_then(|s| by_name.get(&s.name)).cloned();
    specs
        .iter()
        .map(|s| {
            if let Some(p) = by_name.get(&s.name) {
                return p.clone();
            }
            if s.visible && !s.can_autoplace {
                if let Some(p) = kept.iter().find(|p| p.name == s.name) {
                    return style(p.clone(), s);
                }
            }
            let mut p = first.clone().or_else(|| kept.iter().find(|p| p.name == s.name).cloned()).unwrap_or(FieldPlacement { h: TextJustify::Left, visible: false, ..FieldPlacement::at_origin(&s.name) });
            p.name = s.name.clone();
            p.visible = s.visible;
            style(p, s)
        })
        .collect()
}

/// How far (in 50 mil steps) the fields are pushed out from a side they are tried on when the symbol's own sides have no room.
const MAX_FIELD_PUSH: i64 = 4;

/// Every way the fields of a symbol can be placed, the one Autoplace Fields gives first, then each side in KiCad's order of preference,
/// then each pushed further out.
pub fn placement_candidates(sym: &SymbolInstance, geom: &SymbolGeom, specs: &[FieldSpec]) -> Vec<Vec<FieldPlacement>> {
    let mut out = vec![place_fields(sym, geom, specs, &[], None, None)];
    for push in 0..=MAX_FIELD_PUSH {
        for side in [Side::Right, Side::Top, Side::Left, Side::Bottom] {
            let c = place_fields(sym, geom, specs, &[], Some((side, push)), None);
            if !out.contains(&c) {
                out.push(c);
            }
        }
    }
    out
}

/// The placements of a symbol's four fields, in the order of `specs`: the ones the section keeps for it, else Autoplace Fields'.
pub fn symbol_placements(sch: &SchematicSection, sym: &SymbolInstance, geom: &SymbolGeom, specs: &[FieldSpec]) -> Vec<FieldPlacement> {
    let stored = sch.field_layout.get(&field_key(&sym.id, sym.unit));
    let complete = stored.is_some_and(|s| specs.iter().all(|sp| s.iter().any(|p| p.name == sp.name)));
    let auto = if complete { Vec::new() } else { autoplace_symbol(sym, geom, &specs_styled(specs.to_vec(), stored.map(|s| s.as_slice()))) };
    specs
        .iter()
        .map(|spec| stored.and_then(|s| s.iter().find(|p| p.name == spec.name)).or_else(|| auto.iter().find(|p| p.name == spec.name)).cloned().expect("every spec has a placement"))
        .collect()
}

/// The fields of a placed symbol on the sheet: the placements the section keeps for it, else Autoplace Fields', the specs' texts, in order.
pub fn symbol_fields(sch: &SchematicSection, sym: &SymbolInstance, part: Option<&Part>, resolved: Option<&LibSymbol>, geom: &SymbolGeom) -> Vec<PageField> {
    let specs = symbol_specs(sym, part, resolved.map(|r| r.datasheet.as_str()).unwrap_or(""));
    let placements = symbol_placements(sch, sym, geom, &specs);
    specs.iter().zip(placements.iter()).map(|(spec, p)| page_field(sym.at, sym.rot, sym.mirrored, geom.width, p, &spec.text)).collect()
}

/// The Reference and Value of a symbol (the fields that are shown) where the section keeps their placements; `None` for a symbol it keeps none
/// for (a drawing from before fields had places of their own).
pub fn stored_visible_fields(sch: &SchematicSection, sym: &SymbolInstance, part: &Part, resolved: Option<&LibSymbol>) -> Option<Vec<PageField>> {
    if !sch.field_layout.contains_key(&field_key(&sym.id, sym.unit)) {
        return None;
    }
    let mut sym = sym.clone();
    if sym.lib_id.is_empty() {
        sym.lib_id = format!("eda:{}", sym.id);
    }
    let geom = SymbolGeom::of(&sym, part, resolved);
    Some(symbol_fields(sch, &sym, Some(part), resolved, &geom).into_iter().filter(|f| f.visible && !f.text.is_empty()).collect())
}

/// The box a shown field's text takes on the sheet, in millimetres.
pub fn field_box_mm(f: &PageField) -> crate::geometry::TextBox {
    let r = field_rect(f);
    crate::geometry::TextBox { x0: r.x0 as f64 / 1000.0, y0: r.y0 as f64 / 1000.0, x1: r.x1 as f64 / 1000.0, y1: r.y1 as f64 / 1000.0 }
}

/// The box a field's text takes on the sheet as eeschema measures it (`SCH_FIELD::GetBoundingBox`), whole micrometres: the text as it is shown, in its own
/// size and pen, justified against its anchor, turned a quarter when it runs up the sheet.
pub fn field_rect(f: &PageField) -> crate::hier::kit::Rect {
    crate::hier::kit::text_rect_styled(&f.shown_text(), f.at, &f.style(), f.vertical)
}

// -------------------------------------------------------------------------------------------------- power symbols

/// How far a power symbol's glyph reaches from its pin along its axis, pen included (micrometres).
fn glyph_reach(lib_id: &str) -> f64 {
    let Some(lib) = eda_model::symbol::builtin(lib_id) else { return 2_540.0 };
    SymbolGeom::of_origin_symbol(Point { x: 0, y: 0 }, 0, &lib).body.map(|b| b.y0.abs().max(b.y1.abs())).unwrap_or(2_540.0)
}

/// Where a power symbol's Value is. KiCad's power library puts it 3.81 mm below a ground symbol and 3.556 mm above a supply one, centred. A
/// symbol turned a quarter, to run out along a row of pins, would show that text turned too, and the names of two such symbols a pitch apart
/// would run into one another: its Value is read along the row instead, just past the glyph's tip. A flag shows none.
pub fn power_value_placement(ps: &PowerSymbol, lib_id: &str) -> FieldPlacement {
    let ground_like = lib_id == "power:GND";
    let flag = lib_id == "power:PWR_FLAG";
    // the way the glyph points on the sheet
    let glyph = Lin::of_symbol(ps.rot, false).apply((0, if ground_like { 1 } else { -1 }));
    if glyph.0 != 0 && !flag {
        let reach = glyph_reach(lib_id) + 640.0;
        let anchor = (ps.at.x as f64 + glyph.0 as f64 * reach, ps.at.y as f64);
        let h = if glyph.0 > 0 { TextJustify::Left } else { TextJustify::Right };
        return local_placement(ps.at, ps.rot, false, 0.0, "Value", anchor, h, TextVAlign::Center, true);
    }
    let dy = if ground_like {
        3_810
    } else if flag {
        -3_810
    } else {
        -3_556
    };
    // a flag's own name says nothing about the net, and it is wider than the room beside the symbol it stands on
    FieldPlacement { dy, visible: !flag, ..FieldPlacement::at_origin("Value") }
}

/// The placements of a power symbol's fields in its own frame (origin at its pin), with the text each shows: the Reference, hidden, and
/// the Value, the net it asserts. What the `.kicad_sch` file keeps for a power symbol it writes turned.
pub fn power_placements(sch: &SchematicSection, ps: &PowerSymbol) -> Vec<(FieldPlacement, String)> {
    let stored = sch.field_layout.get(&ps.id);
    let value = stored.and_then(|s| s.iter().find(|p| p.name == "Value")).cloned().unwrap_or_else(|| power_value_placement(ps, &ps.lib_id));
    let reference = stored.and_then(|s| s.iter().find(|p| p.name == "Reference")).cloned().unwrap_or(FieldPlacement { visible: false, ..FieldPlacement::at_origin("Reference") });
    vec![(reference, ps.id.clone()), (value, ps.net.clone())]
}

/// The fields of a power symbol on the sheet: its Value (the net it asserts) where the placements put it, its Reference hidden.
pub fn power_fields(sch: &SchematicSection, ps: &PowerSymbol) -> Vec<PageField> {
    power_placements(sch, ps).iter().map(|(p, text)| page_field(ps.at, ps.rot, false, 0.0, p, text)).collect()
}

// ------------------------------------------------------------------------------------------------------ sheets

/// `SCH_SHEET::AutoplaceFields`: the name above the sheet's top-left corner, the file below its bottom-left, both left-justified, a
/// margin of half the text from the border (the name) or two fifths (the file).
pub fn sheet_placements(sheet: &SheetInstance) -> Vec<FieldPlacement> {
    let border = 762 + 4; // half the 6 mil pen, plus 4 internal units
    let name_margin = (border + (DEFAULT_TEXT_SIZE_IU as f64 * 0.5).round() as i64) as f64 / 10.0;
    let file_margin = (border + (DEFAULT_TEXT_SIZE_IU as f64 * 0.4).round() as i64) as f64 / 10.0;
    vec![
        FieldPlacement { dy: -(name_margin.round() as i64), h: TextJustify::Left, v: TextVAlign::Bottom, ..FieldPlacement::at_origin("Sheetname") },
        FieldPlacement { dy: sheet.size.1 + file_margin.round() as i64, h: TextJustify::Left, v: TextVAlign::Top, ..FieldPlacement::at_origin("Sheetfile") },
    ]
}

pub fn sheet_fields(sch: &SchematicSection, sheet: &SheetInstance) -> Vec<PageField> {
    let stored = sch.field_layout.get(&sheet.id);
    let auto = sheet_placements(sheet);
    [("Sheetname", sheet.name.as_str()), ("Sheetfile", sheet.file.as_str())]
        .iter()
        .map(|(name, text)| {
            let p = stored.and_then(|s| s.iter().find(|p| p.name == *name)).or_else(|| auto.iter().find(|p| p.name == *name)).cloned().expect("a placement for each sheet field");
            page_field(sheet.at, 0, false, 0.0, &p, text)
        })
        .collect()
}

/// How much of `a` lies inside `b` (square micrometres), the edges trimmed by a hair so boxes that only touch are not counted.
fn overlap_area(a: crate::hier::kit::Rect, b: crate::hier::kit::Rect) -> i64 {
    const HAIR: i64 = 30;
    let w = (a.x1 - HAIR).min(b.x1 - HAIR) - (a.x0 + HAIR).max(b.x0 + HAIR);
    let h = (a.y1 - HAIR).min(b.y1 - HAIR) - (a.y0 + HAIR).max(b.y0 + HAIR);
    if w > 0 && h > 0 {
        w * h
    } else {
        0
    }
}

/// A symbol whose fields are waiting to be placed: each way of placing them, and the boxes the way it is at now has.
struct Pending {
    key: String,
    sym: SymbolInstance,
    geom: SymbolGeom,
    specs: Vec<FieldSpec>,
    candidates: Vec<Vec<FieldPlacement>>,
    pick: usize,
}

impl Pending {
    /// The boxes of the visible fields placed as candidate `c` has them.
    fn boxes(&self, c: usize) -> Vec<crate::hier::kit::Rect> {
        self.specs
            .iter()
            .zip(self.candidates[c].iter())
            .filter(|(spec, p)| spec.visible && p.visible && !spec.text.is_empty())
            .map(|(spec, p)| {
                let f = page_field(self.sym.at, self.sym.rot, self.sym.mirrored, self.geom.width, p, &spec.text);
                field_rect(&f)
            })
            .collect()
    }
}

/// Give every symbol, power symbol and sheet of a section that has no placements the ones KiCad's Autoplace Fields would: what a
/// derivation stores, so the drawing carries its own field geometry. Items that already have an entry keep it. Where those fields would
/// run over something drawn on the sheet (a wire that passes the symbol, a label, the next symbol), the fields take the nearest other
/// place that is free: another side of the symbol, further out.
pub fn fill_layout(sch: &mut SchematicSection, model: &eda_model::ConstraintModel) {
    let mut add: Vec<(String, Vec<FieldPlacement>)> = Vec::new();
    let mut pending: Vec<Pending> = Vec::new();
    // the fields of the symbols that keep theirs are drawn where they are
    let mut fixed: Vec<crate::hier::kit::Rect> = Vec::new();
    for sym in &sch.symbols {
        let key = field_key(&sym.id, sym.unit);
        let Some(part) = model.part(&sym.id) else { continue };
        let lib_id = if sym.lib_id.is_empty() { format!("eda:{}", sym.id) } else { sym.lib_id.clone() };
        let resolved = model.real_symbol_in_style(&lib_id, part, sch.body_style_of(sym));
        let mut sym = sym.clone();
        sym.lib_id = lib_id;
        let geom = SymbolGeom::of(&sym, part, resolved.as_ref());
        if sch.field_layout.contains_key(&key) {
            for f in symbol_fields(sch, &sym, Some(part), resolved.as_ref(), &geom) {
                if f.visible && !f.text.is_empty() {
                    fixed.push(field_rect(&f));
                }
            }
            continue;
        }
        let specs = symbol_specs(&sym, Some(part), resolved.as_ref().map(|r| r.datasheet.as_str()).unwrap_or(""));
        let candidates = placement_candidates(&sym, &geom, &specs);
        pending.push(Pending { key, sym, geom, specs, candidates, pick: 0 });
    }
    if !pending.is_empty() {
        let obstacles = crate::obstacles::of_section(sch, model);
        let cost = |boxes: &[crate::hier::kit::Rect], others: &[crate::hier::kit::Rect]| -> i64 {
            boxes.iter().map(|b| obstacles.iter().map(|o| overlap_area(*b, o.rect)).sum::<i64>() + others.iter().map(|o| overlap_area(*b, *o)).sum::<i64>()).sum()
        };
        for i in 0..pending.len() {
            // the boxes of every other symbol's fields, where they are for now
            let others: Vec<crate::hier::kit::Rect> = fixed.iter().copied().chain(pending.iter().enumerate().filter(|(j, _)| *j != i).flat_map(|(_, p)| p.boxes(p.pick))).collect();
            let mut best = (cost(&pending[i].boxes(0), &others), 0usize);
            if best.0 > 0 {
                for c in 1..pending[i].candidates.len() {
                    let k = cost(&pending[i].boxes(c), &others);
                    if k < best.0 {
                        best = (k, c);
                        if k == 0 {
                            break;
                        }
                    }
                }
            }
            pending[i].pick = best.1;
        }
    }
    for p in pending {
        add.push((p.key, p.candidates[p.pick].clone()));
    }
    for ps in &sch.power_symbols {
        if !sch.field_layout.contains_key(&ps.id) {
            add.push((ps.id.clone(), power_placements(sch, ps).into_iter().map(|(p, _)| p).collect()));
        }
    }
    for sheet in &sch.sheets {
        if !sheet.id.is_empty() && !sch.field_layout.contains_key(&sheet.id) {
            add.push((sheet.id.clone(), sheet_placements(sheet)));
        }
    }
    // the fields a derivation places are where Autoplace Fields put them (`SCH_SYMBOL::AutoplaceFields( AUTOPLACE_AUTO )` when a symbol is placed): a
    // later turn of the symbol, or an edit of a field's text, places them again
    for (key, _) in &add {
        sch.extras.fields_autoplaced.entry(key.clone()).or_insert(eda_model::sch_extras::AutoplaceAlgo::Auto);
    }
    sch.field_layout.extend(add);
}

#[allow(dead_code)]
fn text_style_for(f: &PageField) -> TextStyle {
    TextStyle::new(f.h, f.v)
}

#[cfg(test)]
mod tests;
