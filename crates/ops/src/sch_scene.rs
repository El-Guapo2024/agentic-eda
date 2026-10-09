//! The working copy a schematic move, drag, turn or mirror edits (`SCH_MOVE_TOOL`, `SCH_EDIT_TOOL::Rotate` / `Mirror`, eeschema at 8303b2ad).
//!
//! KiCad edits `SCH_ITEM`s in place and keeps a handful of per-item flags (`STARTPOINT`, `ENDPOINT`, `SELECTED_BY_DRAG`, `IS_NEW`, ...)
//! that the move and drag paths read. This is the same state for the IR: every wire is split into its segments (`SCH_LINE` is one
//! segment; a [`Wire`] here is a polyline), the other items are edited where they stand, and the flags live next to them. The pieces
//! that ask "what is connected where" are ports of KiCad's own: [`Scene::analyze_point`] is `JUNCTION_HELPERS::AnalyzePoint`,
//! [`Scene::clean_up`] is `SCHEMATIC::CleanUp`, [`merge_overlap`] is `SCH_LINE::MergeOverlap`.
//!
//! Nothing here knows about verbs or commits: `sch_move.rs` and `sch_drag.rs` drive it.

use eda_model::ir::{Junction, Millideg, Point, SchematicSection, SymbolInstance, Um, Wire};
use eda_model::sch_extras::{LabelSpin, SchGraphic, SchGraphicKind};
use eda_model::symbol::{LibSymbol, SymbolGraphic};
use eda_model::ConstraintModel;
use std::collections::{BTreeMap, BTreeSet};

// ---------------------------------------------------------------------------------------------------------------------------------
// flags: the subset of `eda_item_flags.h` the move and drag paths read

/// `SELECTED`: the user's selection (not what a drag added).
pub(crate) const F_SELECTED: u8 = 1;
/// `STARTPOINT`: the first end of a selected line moves.
pub(crate) const F_START: u8 = 2;
/// `ENDPOINT`: the second end of a selected line moves.
pub(crate) const F_END: u8 = 4;
/// `SELECTED_BY_DRAG`: added to the selection because it is attached to something that is dragged.
pub(crate) const F_BY_DRAG: u8 = 8;
/// `IS_NEW`: made by this edit.
pub(crate) const F_NEW: u8 = 16;

/// One item of the sheet, by position in its vector. A [`Seg`] is one segment of a wire (`SCH_LINE_T` on the wire or bus layer).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) enum Item {
    Junction(usize),
    NoConnect(usize),
    BusEntry(usize),
    Seg(usize),
    /// A graphic polyline on the notes layer (`SCH_LINE_T` on `LAYER_NOTES`): not connectable.
    NoteLine(usize),
    /// A drawn shape, text box, rule area or directive label (`SchGraphic`).
    Graphic(usize),
    Text(usize),
    Label(usize),
    Power(usize),
    Symbol(usize),
    SheetPin(usize, usize),
    Sheet(usize),
}

impl Item {
    /// `KICAD_T` order, which `SELECTION::GetItemsSortedByTypeAndXY` sorts by first.
    pub(crate) fn rank(self) -> u8 {
        match self {
            Item::Graphic(_) | Item::Text(_) => 0,
            Item::Junction(_) => 1,
            Item::NoConnect(_) => 2,
            Item::BusEntry(_) => 3,
            Item::Seg(_) | Item::NoteLine(_) => 4,
            Item::Label(_) => 5,
            Item::Power(_) | Item::Symbol(_) => 6,
            Item::SheetPin(..) => 7,
            Item::Sheet(_) => 8,
        }
    }
}

/// One segment of a wire or bus: `SCH_LINE` on `LAYER_WIRE` / `LAYER_BUS`.
#[derive(Clone, Debug)]
pub(crate) struct Seg {
    pub a: Point,
    pub b: Point,
    pub bus: bool,
    /// The wire it came from (index into [`Scene::meta`]) and its place in that wire; `None` for one the edit made.
    pub from: Option<(usize, usize)>,
    /// The net the wire carried, kept on every piece it splits into (`cloneWireConnection`).
    pub net: String,
    pub flags: u8,
    /// `SCH_LINE::StoreAngle`: the direction the line had when the drag started, for a line that has shrunk to nothing.
    pub stored: (i64, i64),
    pub dead: bool,
}

impl Seg {
    pub(crate) fn new(a: Point, b: Point, bus: bool) -> Seg {
        Seg { a, b, bus, from: None, net: String::new(), flags: 0, stored: (1, 0), dead: false }
    }
    pub(crate) fn has(&self, f: u8) -> bool {
        self.flags & f == f
    }
    pub(crate) fn is_null(&self) -> bool {
        self.a == self.b
    }
    pub(crate) fn length(&self) -> f64 {
        ((self.b.x - self.a.x) as f64).hypot((self.b.y - self.a.y) as f64)
    }
    /// `SCH_LINE::Angle` as a direction (a null line points along +x, which is what `EDA_ANGLE( 0 vector )` is).
    pub(crate) fn dir(&self) -> (i64, i64) {
        if self.is_null() {
            (1, 0)
        } else {
            (self.b.x - self.a.x, self.b.y - self.a.y)
        }
    }
    pub(crate) fn midpoint(&self) -> Point {
        Point { x: (self.a.x + self.b.x) / 2, y: (self.a.y + self.b.y) / 2 }
    }
    pub(crate) fn is_endpoint(&self, p: Point) -> bool {
        self.a == p || self.b == p
    }
    /// `SCH_LINE::IsOrthogonal`: horizontal or vertical (a null line counts, as its angle is 0).
    pub(crate) fn is_orthogonal(&self) -> bool {
        self.a.x == self.b.x || self.a.y == self.b.y
    }
}

/// What a polyline [`Wire`] was before it was split into segments.
#[derive(Clone, Debug)]
pub(crate) struct WireMeta {
    pub id: String,
    pub net: String,
    pub pins: Vec<String>,
    pub bus: bool,
}

/// `EDA_ANGLE::IsParallelTo` for two directions (a zero vector is the angle 0).
pub(crate) fn parallel(a: (i64, i64), b: (i64, i64)) -> bool {
    let a = if a == (0, 0) { (1, 0) } else { a };
    let b = if b == (0, 0) { (1, 0) } else { b };
    a.0 as i128 * b.1 as i128 - a.1 as i128 * b.0 as i128 == 0
}

/// `IsPointOnSegment`: `p` lies on the segment `a`-`b` (ends included).
pub(crate) fn on_segment(a: Point, b: Point, p: Point) -> bool {
    if a == b {
        return p == a;
    }
    let cross = (b.x - a.x) as i128 * (p.y - a.y) as i128 - (b.y - a.y) as i128 * (p.x - a.x) as i128;
    cross == 0 && p.x >= a.x.min(b.x) && p.x <= a.x.max(b.x) && p.y >= a.y.min(b.y) && p.y <= a.y.max(b.y)
}

/// `RotatePoint( point, center, ANGLE_90 or ANGLE_270 )` (`libs/kimath/src/trigo.cpp`): a quarter turn about `c`, counter-clockwise on
/// the screen when `ccw` (the y axis points down, so +90 maps (x, y) to (y, -x)).
pub(crate) fn rotate_point(p: Point, c: Point, ccw: bool) -> Point {
    let (dx, dy) = (p.x - c.x, p.y - c.y);
    if ccw {
        Point { x: c.x + dy, y: c.y - dx }
    } else {
        Point { x: c.x - dy, y: c.y + dx }
    }
}

/// `RotatePoint( &size.x, &size.y, angle )`: a vector, not a point.
pub(crate) fn rotate_vec(v: Point, ccw: bool) -> Point {
    if ccw {
        Point { x: v.y, y: -v.x }
    } else {
        Point { x: -v.y, y: v.x }
    }
}

/// `MIRROR( a, axis )`.
pub(crate) fn mirror_coord(a: Um, axis: Um) -> Um {
    2 * axis - a
}

// ---------------------------------------------------------------------------------------------------------------------------------
// symbol orientation (`TRANSFORM`, `SCH_SYMBOL::SetOrientation`)

/// `TRANSFORM`: `x' = x1 x + y1 y`, `y' = x2 x + y2 y` on a library point already flipped to the sheet's y axis.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Matrix {
    pub x1: i64,
    pub y1: i64,
    pub x2: i64,
    pub y2: i64,
}

impl Matrix {
    pub(crate) const IDENTITY: Matrix = Matrix { x1: 1, y1: 0, x2: 0, y2: 1 };
    /// `SetOrientation( SYM_ROTATE_COUNTERCLOCKWISE )`.
    pub(crate) const ROT_CCW: Matrix = Matrix { x1: 0, y1: 1, x2: -1, y2: 0 };
    /// `SetOrientation( SYM_ROTATE_CLOCKWISE )`.
    pub(crate) const ROT_CW: Matrix = Matrix { x1: 0, y1: -1, x2: 1, y2: 0 };
    /// `SYM_MIRROR_Y`: negates x ("Mirror Horizontally"; this IR's `SymbolInstance::mirrored`).
    pub(crate) const MIRROR_Y: Matrix = Matrix { x1: -1, y1: 0, x2: 0, y2: 1 };
    /// `SYM_MIRROR_X`: negates y ("Mirror Vertically"; this IR's `SymbolInstance::mirror_y`).
    pub(crate) const MIRROR_X: Matrix = Matrix { x1: 1, y1: 0, x2: 0, y2: -1 };

    pub(crate) fn apply(self, x: i64, y: i64) -> (i64, i64) {
        (self.x1 * x + self.y1 * y, self.x2 * x + self.y2 * y)
    }

    /// The matrix of a file orientation: the rotation first, then the mirror (`parseSymbol`: `SetOrientation( angle )`, then
    /// `SetOrientation( SYM_MIRROR_x )`), each composed after what is there (`SetOrientation`'s `newTransform`).
    pub(crate) fn of(rot: Millideg, mirrored: bool, mirror_y: bool) -> Matrix {
        let mut m = match (rot / 1000 % 360 + 45) / 90 % 4 {
            0 => Matrix::IDENTITY,
            1 => Matrix::IDENTITY.then(Matrix::ROT_CCW),
            2 => Matrix::IDENTITY.then(Matrix::ROT_CCW).then(Matrix::ROT_CCW),
            _ => Matrix::IDENTITY.then(Matrix::ROT_CW),
        };
        if mirror_y {
            m = m.then(Matrix::MIRROR_X);
        }
        if mirrored {
            m = m.then(Matrix::MIRROR_Y);
        }
        m
    }

    /// `SCH_SYMBOL::SetOrientation( incremental )`: the new matrix is `temp * old`.
    pub(crate) fn then(self, temp: Matrix) -> Matrix {
        Matrix {
            x1: self.x1 * temp.x1 + self.x2 * temp.y1,
            y1: self.y1 * temp.x1 + self.y2 * temp.y1,
            x2: self.x1 * temp.x2 + self.x2 * temp.y2,
            y2: self.y1 * temp.x2 + self.y2 * temp.y2,
        }
    }

    /// `SCH_SYMBOL::GetOrientation`: the first of KiCad's twelve orientations that gives this matrix, as this IR stores it
    /// (`rot`, `mirrored` = `SYM_MIRROR_Y`, `mirror_y` = `SYM_MIRROR_X`).
    pub(crate) fn orientation(self) -> (Millideg, bool, bool) {
        const LIST: [(Millideg, bool, bool); 12] = [
            (0, false, false),
            (90_000, false, false),
            (180_000, false, false),
            (270_000, false, false),
            (0, false, true),
            (90_000, false, true),
            (270_000, false, true),
            (0, true, false),
            (0, true, false),
            (90_000, true, false),
            (180_000, true, false),
            (270_000, true, false),
        ];
        for (rot, mirrored, mirror_y) in LIST {
            if Matrix::of(rot, mirrored, mirror_y) == self {
                return (rot, mirrored, mirror_y);
            }
        }
        (0, false, false)
    }
}

// ---------------------------------------------------------------------------------------------------------------------------------
// the scene

/// `JUNCTION_HELPERS::POINT_INFO`.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct PointInfo {
    pub has_bus_entry: bool,
    pub has_bus_entry_to_multiple_wires: bool,
    pub has_explicit_junction_dot: bool,
    pub is_junction: bool,
}

/// Where one symbol's pins are, in its own frame (micrometres on the sheet's y axis, before the orientation), and the box its graphics and
/// pins fill.
#[derive(Clone, Debug, Default)]
pub(crate) struct SymbolShape {
    pub pins: Vec<(String, (i64, i64))>,
    pub bbox: Option<(i64, i64, i64, i64)>,
}

pub(crate) struct Scene<'m> {
    pub sch: SchematicSection,
    pub segs: Vec<Seg>,
    pub meta: Vec<WireMeta>,
    pub shapes: Vec<SymbolShape>,
    pub power_shapes: Vec<SymbolShape>,
    /// The user's selection (`SELECTED`); a [`Seg`] keeps its own flags.
    pub selected: BTreeSet<Item>,
    /// `SELECTED_BY_DRAG` on an item that is not a segment.
    pub by_drag: BTreeSet<Item>,
    /// The model the scene was made against: the parts and library symbols the fields of its symbols are measured by.
    pub model: &'m ConstraintModel,
    /// The fields the user picked on their own (`SCH_FIELD`), see `sch_fields.rs`.
    pub fields: Vec<crate::sch_fields::FieldSel>,
}

impl<'m> Scene<'m> {
    pub(crate) fn new(sch: &SchematicSection, model: &'m ConstraintModel) -> Scene<'m> {
        let mut sch = sch.clone();
        let mut segs = Vec::new();
        let mut meta = Vec::new();
        for (wi, w) in std::mem::take(&mut sch.wires).into_iter().enumerate() {
            meta.push(WireMeta { id: w.id.clone(), net: w.net.clone(), pins: w.pins.clone(), bus: w.bus });
            for (k, pair) in w.pts.windows(2).enumerate() {
                let mut s = Seg::new(pair[0], pair[1], w.bus);
                s.from = Some((wi, k));
                s.net = w.net.clone();
                segs.push(s);
            }
        }
        let shapes = sch.symbols.iter().map(|s| symbol_shape(model, &sch, s)).collect();
        let power_shapes = sch.power_symbols.iter().map(|p| power_shape(model, p)).collect();
        Scene { sch, segs, meta, shapes, power_shapes, selected: BTreeSet::new(), by_drag: BTreeSet::new(), model, fields: Vec::new() }
    }

    // -------------------------------------------------------------------------------------------------------------- items

    /// Every live item of the sheet.
    pub(crate) fn items(&self) -> Vec<Item> {
        let mut out = Vec::new();
        out.extend((0..self.sch.junctions.len()).map(Item::Junction));
        out.extend((0..self.sch.no_connects.len()).map(Item::NoConnect));
        out.extend((0..self.sch.bus_entries.len()).map(Item::BusEntry));
        out.extend(self.segs.iter().enumerate().filter(|(_, s)| !s.dead).map(|(i, _)| Item::Seg(i)));
        out.extend((0..self.sch.lines.len()).map(Item::NoteLine));
        out.extend((0..self.sch.extras.graphics.len()).map(Item::Graphic));
        out.extend((0..self.sch.texts.len()).map(Item::Text));
        out.extend((0..self.sch.labels.len()).map(Item::Label));
        out.extend((0..self.sch.power_symbols.len()).map(Item::Power));
        out.extend((0..self.sch.symbols.len()).map(Item::Symbol));
        for (si, s) in self.sch.sheets.iter().enumerate() {
            out.extend((0..s.pins.len()).map(|pi| Item::SheetPin(si, pi)));
            out.push(Item::Sheet(si));
        }
        out
    }

    pub(crate) fn graphic(&self, i: usize) -> &SchGraphic {
        &self.sch.extras.graphics[i]
    }

    /// Is this item a label of any kind (`SCH_LABEL_BASE`): a local, global or hierarchical label, a directive label, or a sheet pin.
    pub(crate) fn is_label_like(&self, it: Item) -> bool {
        match it {
            Item::Label(_) | Item::SheetPin(..) => true,
            Item::Graphic(i) => matches!(self.graphic(i).shape, SchGraphicKind::Directive { .. }),
            _ => false,
        }
    }

    /// `SCH_ITEM::IsConnectable`.
    pub(crate) fn is_connectable(&self, it: Item) -> bool {
        match it {
            Item::Seg(_) | Item::Junction(_) | Item::NoConnect(_) | Item::BusEntry(_) | Item::Label(_) | Item::Power(_) | Item::Symbol(_) | Item::SheetPin(..) | Item::Sheet(_) => true,
            Item::Graphic(i) => matches!(self.graphic(i).shape, SchGraphicKind::Directive { .. }),
            Item::NoteLine(_) | Item::Text(_) => false,
        }
    }

    /// `SCH_ITEM::GetPosition`.
    pub(crate) fn position(&self, it: Item) -> Point {
        match it {
            Item::Junction(i) => self.sch.junctions[i].at,
            Item::NoConnect(i) => self.sch.no_connects[i].at,
            Item::BusEntry(i) => self.sch.bus_entries[i].at,
            Item::Seg(i) => self.segs[i].a,
            Item::NoteLine(i) => self.sch.lines[i].pts.first().copied().unwrap_or_default(),
            Item::Graphic(i) => graphic_position(&self.sch.extras.graphics[i]),
            Item::Text(i) => self.sch.texts[i].at,
            Item::Label(i) => self.sch.labels[i].at,
            Item::Power(i) => self.sch.power_symbols[i].at,
            Item::Symbol(i) => self.sch.symbols[i].at,
            Item::SheetPin(s, p) => self.sch.sheets[s].pins[p].at,
            Item::Sheet(i) => self.sch.sheets[i].at,
        }
    }

    /// `SCH_ITEM::GetSortPosition` (a line sorts by its midpoint).
    pub(crate) fn sort_position(&self, it: Item) -> Point {
        match it {
            Item::Seg(i) => self.segs[i].midpoint(),
            other => self.position(other),
        }
    }

    /// Where the pins of a placed symbol are right now.
    pub(crate) fn pin_tips(&self, i: usize) -> Vec<Point> {
        let s = &self.sch.symbols[i];
        let m = Matrix::of(s.rot, s.mirrored, s.mirror_y);
        self.shapes[i].pins.iter().map(|(_, (x, y))| {
            let (px, py) = m.apply(*x, *y);
            Point { x: s.at.x + px, y: s.at.y + py }
        }).collect()
    }

    /// The pins of a placed symbol with their numbers.
    #[cfg(test)]
    pub(crate) fn pins_of(&self, i: usize) -> Vec<(String, Point)> {
        let s = &self.sch.symbols[i];
        let m = Matrix::of(s.rot, s.mirrored, s.mirror_y);
        self.shapes[i].pins.iter().map(|(n, (x, y))| {
            let (px, py) = m.apply(*x, *y);
            (n.clone(), Point { x: s.at.x + px, y: s.at.y + py })
        }).collect()
    }

    /// The box a symbol fills (graphics and pins), as KiCad's `SCH_SYMBOL::GetBodyBoundingBox` (without fields).
    pub(crate) fn symbol_box(&self, i: usize) -> Option<(Point, Point)> {
        let s = &self.sch.symbols[i];
        let m = Matrix::of(s.rot, s.mirrored, s.mirror_y);
        shape_box(&self.shapes[i], m, s.at)
    }

    pub(crate) fn power_box(&self, i: usize) -> Option<(Point, Point)> {
        let p = &self.sch.power_symbols[i];
        shape_box(&self.power_shapes[i], Matrix::of(p.rot, false, false), p.at)
    }

    /// `GetConnectionPoints`.
    pub(crate) fn connection_points(&self, it: Item) -> Vec<Point> {
        match it {
            Item::Seg(i) => vec![self.segs[i].a, self.segs[i].b],
            Item::Junction(_) | Item::NoConnect(_) | Item::Label(_) | Item::Power(_) | Item::SheetPin(..) => vec![self.position(it)],
            Item::Graphic(i) => match self.graphic(i).shape {
                SchGraphicKind::Directive { at, .. } => vec![at],
                _ => Vec::new(),
            },
            Item::BusEntry(i) => {
                let b = &self.sch.bus_entries[i];
                vec![b.at, Point { x: b.at.x + b.size.x, y: b.at.y + b.size.y }]
            }
            Item::Symbol(i) => self.pin_tips(i),
            Item::Sheet(i) => self.sch.sheets[i].pins.iter().map(|p| p.at).collect(),
            Item::NoteLine(_) | Item::Text(_) => Vec::new(),
        }
    }

    /// `SCH_ITEM::IsConnected( point )`.
    pub(crate) fn is_connected(&self, it: Item, p: Point) -> bool {
        self.connection_points(it).contains(&p)
    }

    /// `test->CanConnect( other )`.
    pub(crate) fn can_connect(&self, test: Item, other: Item) -> bool {
        let is_wire = |it: Item| matches!(it, Item::Seg(i) if !self.segs[i].bus);
        let is_bus = |it: Item| matches!(it, Item::Seg(i) if self.segs[i].bus);
        let is_label = |it: Item| matches!(it, Item::Label(_)) || self.is_label_like(it);
        match test {
            Item::Seg(i) => {
                let wire = !self.segs[i].bus;
                match other {
                    Item::NoConnect(_) | Item::Symbol(_) | Item::Power(_) => wire,
                    Item::Junction(_) | Item::BusEntry(_) | Item::Sheet(_) | Item::SheetPin(..) | Item::Label(_) => true,
                    o if self.is_label_like(o) => true,
                    // `m_layer == aItem->GetLayer()`
                    o => (is_wire(o) && wire) || (is_bus(o) && !wire),
                }
            }
            Item::Symbol(_) | Item::Power(_) => match other {
                o if is_wire(o) => true,
                Item::NoConnect(_) | Item::Junction(_) | Item::Symbol(_) | Item::Power(_) => true,
                o => is_label(o) && !matches!(o, Item::SheetPin(..)),
            },
            Item::Junction(_) => matches!(other, Item::Seg(_) | Item::Symbol(_) | Item::Power(_)) || is_label(other) && !matches!(other, Item::SheetPin(..)),
            Item::NoConnect(_) => is_wire(other) || matches!(other, Item::Symbol(_) | Item::Power(_) | Item::Sheet(_)),
            Item::BusEntry(i) => {
                let _ = i;
                matches!(other, Item::Seg(_))
            }
            Item::Sheet(_) => matches!(other, Item::Seg(_) | Item::NoConnect(_) | Item::Symbol(_) | Item::Power(_)),
            t if is_label(t) => match other {
                Item::Seg(_) | Item::BusEntry(_) | Item::Symbol(_) | Item::Power(_) | Item::Label(_) | Item::SheetPin(..) => true,
                o => self.is_label_like(o),
            },
            _ => false,
        }
    }

    // ------------------------------------------------------------------------------------------------------------ geometry

    /// Does `p` lie on this item's line (`SCH_LINE::HitTest( p, 1 )`; exact on this integer grid)?
    pub(crate) fn seg_hit(&self, i: usize, p: Point) -> bool {
        let s = &self.segs[i];
        on_segment(s.a, s.b, p)
    }

    /// The bounding box of an item, for the centre of a selection (`SELECTION::GetCenter` leaves out text and labels).
    pub(crate) fn item_box(&self, it: Item) -> Option<(Point, Point)> {
        let pt = |p: Point| Some((p, p));
        let around = |p: Point, r: Um| Some((Point { x: p.x - r, y: p.y - r }, Point { x: p.x + r, y: p.y + r }));
        match it {
            Item::Seg(i) => Some((Point { x: self.segs[i].a.x.min(self.segs[i].b.x), y: self.segs[i].a.y.min(self.segs[i].b.y) }, Point { x: self.segs[i].a.x.max(self.segs[i].b.x), y: self.segs[i].a.y.max(self.segs[i].b.y) })),
            Item::NoteLine(i) => points_box(&self.sch.lines[i].pts),
            // `DEFAULT_JUNCTION_DIAMETER` 36 mil, `DEFAULT_NOCONNECT_SIZE` 48 mil: the same boxes the studio draws and picks by.
            Item::Junction(i) => around(self.sch.junctions[i].at, 457),
            Item::NoConnect(i) => around(self.sch.no_connects[i].at, 610),
            Item::BusEntry(i) => {
                let b = &self.sch.bus_entries[i];
                points_box(&[b.at, Point { x: b.at.x + b.size.x, y: b.at.y + b.size.y }])
            }
            Item::Graphic(i) => graphic_box(self.graphic(i)),
            Item::Text(i) => pt(self.sch.texts[i].at),
            Item::Label(i) => pt(self.sch.labels[i].at),
            Item::Power(i) => self.power_box(i),
            Item::Symbol(i) => self.symbol_box(i),
            Item::SheetPin(s, p) => pt(self.sch.sheets[s].pins[p].at),
            Item::Sheet(i) => {
                let s = &self.sch.sheets[i];
                Some((s.at, Point { x: s.at.x + s.size.0, y: s.at.y + s.size.1 }))
            }
        }
    }

    // ------------------------------------------------------------------------------------------------------- junctions

    /// `JUNCTION_HELPERS::AnalyzePoint( items, p, aBreakCrossings )`: what meets at `p`. Items in `skip` are ignored (the ones being moved,
    /// which KiCad flags `STRUCT_DELETED` while it asks).
    pub(crate) fn analyze_point(&self, p: Point, break_crossings: bool, skip: &BTreeSet<Item>) -> PointInfo {
        const WIRES: usize = 0;
        const BUSES: usize = 1;
        let mut info = PointInfo::default();
        let mut lines: Vec<(Point, Point, usize)> = Vec::new();
        for i in 0..self.segs.len() {
            let s = &self.segs[i];
            if s.dead || skip.contains(&Item::Seg(i)) || !on_segment(s.a, s.b, p) {
                continue;
            }
            lines.push((s.a, s.b, if s.bus { BUSES } else { WIRES }));
        }
        for (i, j) in self.sch.junctions.iter().enumerate() {
            if j.at == p && !skip.contains(&Item::Junction(i)) {
                info.has_explicit_junction_dot = true;
            }
        }
        for (i, b) in self.sch.bus_entries.iter().enumerate() {
            if (b.at == p || entry_end(b) == p) && !skip.contains(&Item::BusEntry(i)) {
                info.has_bus_entry = true;
            }
        }
        // temporarily merge collinear lines, unless a dot is there or crossings break lines anyway
        if !(info.has_explicit_junction_dot || break_crossings) {
            let mut merged = true;
            while merged {
                merged = false;
                'outer: for i in 0..lines.len() {
                    for j in i + 1..lines.len() {
                        if lines[i].2 != lines[j].2 {
                            continue;
                        }
                        if let Some((a, b)) = merge_ends(lines[i].0, lines[i].1, lines[j].0, lines[j].1) {
                            lines[i] = (a, b, lines[i].2);
                            lines.remove(j);
                            merged = true;
                            break 'outer;
                        }
                    }
                }
            }
        }

        let mut break_lines = [false; 2];
        let mut exit_angles: [BTreeSet<i32>; 2] = [BTreeSet::new(), BTreeSet::new()];
        let mut mid_lines: [Vec<(Point, Point)>; 2] = [Vec::new(), Vec::new()];
        let mut unique_angle = 10_000i32;
        for &(a, b, layer) in &lines {
            if a == b {
                continue;
            }
            if a == p || b == p {
                break_lines[layer] = true;
                let other = if a == p { b } else { a };
                exit_angles[layer].insert(angle_from(p, other));
            } else {
                if break_crossings {
                    break_lines[layer] = true;
                }
                mid_lines[layer].push((a, b));
            }
        }
        for (i, b) in self.sch.bus_entries.iter().enumerate() {
            if skip.contains(&Item::BusEntry(i)) {
                continue;
            }
            if b.at == p || entry_end(b) == p {
                break_lines[BUSES] = true;
                exit_angles[BUSES].insert(unique_angle);
                unique_angle += 1;
                break_lines[WIRES] = true;
                exit_angles[WIRES].insert(unique_angle);
                unique_angle += 1;
            }
        }
        // symbol pins, power symbols and sheet pins
        for i in 0..self.sch.symbols.len() {
            if !skip.contains(&Item::Symbol(i)) && self.pin_tips(i).contains(&p) {
                break_lines[WIRES] = true;
                exit_angles[WIRES].insert(unique_angle);
                unique_angle += 1;
            }
        }
        for (i, ps) in self.sch.power_symbols.iter().enumerate() {
            if !skip.contains(&Item::Power(i)) && ps.at == p {
                break_lines[WIRES] = true;
                exit_angles[WIRES].insert(unique_angle);
                unique_angle += 1;
            }
        }
        for (si, s) in self.sch.sheets.iter().enumerate() {
            if skip.contains(&Item::Sheet(si)) {
                continue;
            }
            for (pi, pin) in s.pins.iter().enumerate() {
                if pin.at == p && !skip.contains(&Item::SheetPin(si, pi)) {
                    break_lines[WIRES] = true;
                    exit_angles[WIRES].insert(unique_angle);
                    unique_angle += 1;
                }
            }
        }
        // a label at the point only marks it as one where lines break
        for (i, l) in self.sch.labels.iter().enumerate() {
            if l.at == p && !skip.contains(&Item::Label(i)) {
                let bus_label = is_bus_label(&l.net);
                break_lines[if bus_label { BUSES } else { WIRES }] = true;
            }
        }
        for (i, g) in self.sch.extras.graphics.iter().enumerate() {
            if let SchGraphicKind::Directive { at, .. } = g.shape {
                if at == p && !skip.contains(&Item::Graphic(i)) {
                    break_lines[WIRES] = true;
                }
            }
        }
        for layer in [WIRES, BUSES] {
            if break_lines[layer] {
                for &(a, b) in &mid_lines[layer] {
                    exit_angles[layer].insert(angle_from(p, b));
                    exit_angles[layer].insert(angle_from(p, a));
                }
            }
        }
        if info.has_bus_entry {
            info.has_bus_entry_to_multiple_wires = exit_angles[WIRES].len() > 2 && exit_angles[BUSES].len() == 1;
        }
        info.is_junction = exit_angles[WIRES].len() >= 3 || exit_angles[BUSES].len() >= 3;
        info
    }

    /// `SCH_SCREEN::IsJunction`.
    pub(crate) fn is_junction(&self, p: Point) -> bool {
        self.analyze_point(p, false, &BTreeSet::new()).is_junction
    }

    /// `SCH_SCREEN::IsExplicitJunction`.
    pub(crate) fn is_explicit_junction(&self, p: Point) -> bool {
        let info = self.analyze_point(p, false, &BTreeSet::new());
        info.is_junction && (!info.has_bus_entry || info.has_bus_entry_to_multiple_wires)
    }

    /// `SCH_SCREEN::IsExplicitJunctionNeeded`.
    pub(crate) fn is_explicit_junction_needed(&self, p: Point) -> bool {
        let info = self.analyze_point(p, false, &BTreeSet::new());
        info.is_junction && (!info.has_bus_entry || info.has_bus_entry_to_multiple_wires) && !info.has_explicit_junction_dot
    }

    /// `SCH_LINE_WIRE_BUS_TOOL::AddJunction`: a junction dot at `p`, and the wires that pass through it are broken there
    /// (`BreakSegments( aPos )`, which skips the ones that already end at it).
    pub(crate) fn add_junction(&mut self, p: Point) {
        if self.sch.junctions.iter().any(|j| j.at == p) {
            return;
        }
        self.sch.junctions.push(Junction { id: String::new(), at: p });
        self.break_segments_at(p);
    }

    /// `SCH_SCREEN::GetBusesAndWires( p, true )` and `BreakSegment`: split every segment that passes through `p` without ending there.
    pub(crate) fn break_segments_at(&mut self, p: Point) {
        for i in 0..self.segs.len() {
            let s = &self.segs[i];
            if s.dead || s.is_endpoint(p) || !on_segment(s.a, s.b, p) {
                continue;
            }
            self.break_seg(i, p);
        }
    }

    /// `SCH_LINE::BreakAt`: the segment keeps its start and ends at `p`; the new one runs from `p` to the old end and copies the rest.
    pub(crate) fn break_seg(&mut self, i: usize, p: Point) -> usize {
        let mut tail = self.segs[i].clone();
        tail.a = p;
        tail.flags &= !(F_START | F_END);
        tail.flags |= F_NEW;
        self.segs[i].b = p;
        // the tail follows its head in the wire it came from
        if let Some((w, k)) = self.segs[i].from {
            tail.from = Some((w, k));
        }
        self.segs.push(tail);
        self.segs.len() - 1
    }

    // ------------------------------------------------------------------------------------------------------------ cleanup

    /// `SCHEMATIC::CleanUp` for one screen: junctions that no longer join anything go (a selected one stays), two junctions or
    /// no-connects at one point become one, a line of no length goes, and lines that overlap or meet end to end on one line with no
    /// junction between them merge into one.
    pub(crate) fn clean_up(&mut self) {
        // junctions
        let mut keep_j: Vec<bool> = Vec::new();
        for i in 0..self.sch.junctions.len() {
            let at = self.sch.junctions[i].at;
            let keep = self.is_explicit_junction(at) || self.selected.contains(&Item::Junction(i)) || self.by_drag.contains(&Item::Junction(i));
            keep_j.push(keep);
        }
        // two at one point: the later one goes
        for i in 0..self.sch.junctions.len() {
            if !keep_j[i] {
                continue;
            }
            for j in i + 1..self.sch.junctions.len() {
                if keep_j[j] && self.sch.junctions[j].at == self.sch.junctions[i].at {
                    keep_j[j] = false;
                }
            }
        }
        let mut keep_n: Vec<bool> = vec![true; self.sch.no_connects.len()];
        for i in 0..keep_n.len() {
            for j in i + 1..keep_n.len() {
                if keep_n[i] && keep_n[j] && self.sch.no_connects[j].at == self.sch.no_connects[i].at {
                    keep_n[j] = false;
                }
            }
        }
        self.retain_junctions(&keep_j);
        self.retain_no_connects(&keep_n);

        // lines
        let mut changed = true;
        while changed {
            changed = false;
            let mut order: Vec<usize> = (0..self.segs.len()).filter(|&i| !self.segs[i].dead).collect();
            order.sort_by_key(|&i| (self.segs[i].a.x.min(self.segs[i].b.x), i));
            'scan: for (oi, &i1) in order.iter().enumerate() {
                if self.segs[i1].dead {
                    continue;
                }
                if self.segs[i1].is_null() {
                    self.segs[i1].dead = true;
                    changed = true;
                    continue;
                }
                let first_right = self.segs[i1].a.x.max(self.segs[i1].b.x);
                for &i2 in &order[oi + 1..] {
                    if self.segs[i2].dead {
                        continue;
                    }
                    let (f, s) = (&self.segs[i1], &self.segs[i2]);
                    if s.a.x.min(s.b.x) > first_right {
                        break;
                    }
                    if f.a.y.min(f.b.y).max(s.a.y.min(s.b.y)) > f.a.y.max(f.b.y).min(s.a.y.max(s.b.y)) {
                        continue;
                    }
                    if !parallel(s.dir(), f.dir()) || s.bus != f.bus {
                        continue;
                    }
                    // identical lines
                    if f.is_endpoint(s.a) && f.is_endpoint(s.b) {
                        self.segs[i2].dead = true;
                        changed = true;
                        continue;
                    }
                    if let Some((a, b)) = merge_ends(f.a, f.b, s.a, s.b) {
                        // two lines that only touch merge when no junction sits where they meet (`aCheckJunctions`)
                        let touching = {
                            let (fl, fr) = ordered(f.a, f.b);
                            let (sl, sr) = ordered(s.a, s.b);
                            let (left, right) = if sl < fl { ((sl, sr), (fl, fr)) } else { ((fl, fr), (sl, sr)) };
                            left.1 == right.0
                        };
                        if touching && self.is_junction(if self.segs[i1].a == a || self.segs[i1].b == a { self.meeting_point(i1, i2) } else { self.meeting_point(i1, i2) }) {
                            continue;
                        }
                        let selected = self.segs[i1].has(F_SELECTED) || self.segs[i2].has(F_SELECTED);
                        self.segs[i1].a = a;
                        self.segs[i1].b = b;
                        if selected {
                            self.segs[i1].flags |= F_SELECTED;
                        }
                        self.segs[i2].dead = true;
                        changed = true;
                        continue 'scan;
                    }
                }
            }
        }
    }

    fn meeting_point(&self, i1: usize, i2: usize) -> Point {
        let (f, s) = (&self.segs[i1], &self.segs[i2]);
        for p in [f.a, f.b] {
            if s.is_endpoint(p) {
                return p;
            }
        }
        f.a
    }

    pub(crate) fn retain_junctions(&mut self, keep: &[bool]) {
        let remap = remap_indices(keep);
        let mut k = 0;
        self.sch.junctions.retain(|_| {
            let r = keep[k];
            k += 1;
            r
        });
        self.selected = self.selected.iter().filter_map(|it| match it {
            Item::Junction(i) => remap[*i].map(Item::Junction),
            o => Some(*o),
        }).collect();
        self.by_drag = self.by_drag.iter().filter_map(|it| match it {
            Item::Junction(i) => remap[*i].map(Item::Junction),
            o => Some(*o),
        }).collect();
    }

    pub(crate) fn retain_no_connects(&mut self, keep: &[bool]) {
        let remap = remap_indices(keep);
        let mut k = 0;
        self.sch.no_connects.retain(|_| {
            let r = keep[k];
            k += 1;
            r
        });
        self.selected = self.selected.iter().filter_map(|it| match it {
            Item::NoConnect(i) => remap[*i].map(Item::NoConnect),
            o => Some(*o),
        }).collect();
        self.by_drag = self.by_drag.iter().filter_map(|it| match it {
            Item::NoConnect(i) => remap[*i].map(Item::NoConnect),
            o => Some(*o),
        }).collect();
    }

    // ---------------------------------------------------------------------------------------------------------------- end

    /// The section back, with the segments joined into wires again where they still form a chain.
    pub(crate) fn finish(mut self) -> SchematicSection {
        let mut wires: Vec<Wire> = Vec::new();
        // wires the edit did not touch keep their points and ids exactly
        let mut by_wire: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
        let mut fresh: Vec<usize> = Vec::new();
        for (i, s) in self.segs.iter().enumerate() {
            if s.dead {
                continue;
            }
            match s.from {
                Some((w, _)) => by_wire.entry(w).or_default().push(i),
                None => fresh.push(i),
            }
        }
        for (wi, m) in self.meta.iter().enumerate() {
            let Some(list) = by_wire.get_mut(&wi) else { continue };
            list.sort_by_key(|&i| (self.segs[i].from.map(|f| f.1).unwrap_or(0), i));
            let mut runs: Vec<Vec<Point>> = Vec::new();
            for &i in list.iter() {
                let s = &self.segs[i];
                match runs.last_mut() {
                    Some(run) if run.last() == Some(&s.a) => run.push(s.b),
                    _ => runs.push(vec![s.a, s.b]),
                }
            }
            for (k, pts) in runs.into_iter().enumerate() {
                let seg_net = list.first().map(|&i| self.segs[i].net.clone()).unwrap_or_else(|| m.net.clone());
                wires.push(Wire { id: if k == 0 { m.id.clone() } else { String::new() }, net: seg_net, pins: if k == 0 { m.pins.clone() } else { Vec::new() }, pts, bus: m.bus });
            }
        }
        for i in fresh {
            let s = &self.segs[i];
            wires.push(Wire { id: String::new(), net: s.net.clone(), pins: Vec::new(), pts: vec![s.a, s.b], bus: s.bus });
        }
        self.sch.wires = wires;
        self.sch.assign_missing_ids();
        self.sch
    }
}

// ---------------------------------------------------------------------------------------------------------------------------------
// free helpers

fn entry_end(b: &eda_model::ir::BusEntry) -> Point {
    Point { x: b.at.x + b.size.x, y: b.at.y + b.size.y }
}

fn remap_indices(keep: &[bool]) -> Vec<Option<usize>> {
    let mut n = 0;
    keep.iter().map(|&k| {
        if k {
            n += 1;
            Some(n - 1)
        } else {
            None
        }
    }).collect()
}

fn ordered(a: Point, b: Point) -> (Point, Point) {
    if (a.x, a.y) <= (b.x, b.y) {
        (a, b)
    } else {
        (b, a)
    }
}

/// `SCH_CONNECTION::IsBusLabel`: a label with a bus name (a vector `A[0..3]` or a group `{A B}`).
pub(crate) fn is_bus_label(text: &str) -> bool {
    (text.contains('[') && text.contains(']') && text.contains("..")) || (text.contains('{') && text.contains('}'))
}

/// `SCH_LINE::GetAngleFrom`: the direction from `p` towards `other`, in whole degrees.
fn angle_from(p: Point, other: Point) -> i32 {
    let (dx, dy) = ((other.x - p.x) as f64, (other.y - p.y) as f64);
    dy.atan2(dx).to_degrees().round() as i32
}

/// The far ends of two collinear lines that overlap, or only touch end to end: the line that covers both (`SCH_LINE::MergeOverlap` without
/// the junction check, which is the caller's). `None` for lines that are not on one line or do not reach each other.
pub(crate) fn merge_ends(a1: Point, b1: Point, a2: Point, b2: Point) -> Option<(Point, Point)> {
    let key = |p: Point| (p.x, p.y);
    let (l1, r1) = ordered(a1, b1);
    let (l2, r2) = ordered(a2, b2);
    // `leftmost` is the line that starts first, `other` the other
    let ((ls, le), (os, oe)) = if key(l2) < key(l1) { ((l2, r2), (l1, r1)) } else { ((l1, r1), (l2, r2)) };
    // one ends before the other begins: no overlap is possible
    if key(le) < key(os) {
        return None;
    }
    // collinear?
    let colinear = if ls.y == le.y && os.y == oe.y {
        ls.y == os.y
    } else if ls.x == le.x && os.x == oe.x {
        ls.x == os.x
    } else {
        let dx = (le.x - ls.x) as i128;
        let dy = (le.y - ls.y) as i128;
        (os.y - ls.y) as i128 * dx == (os.x - ls.x) as i128 * dy && (oe.y - ls.y) as i128 * dx == (oe.x - ls.x) as i128 * dy
    };
    if !colinear {
        return None;
    }
    let end = if key(oe) > key(le) { oe } else { le };
    Some((ls, end))
}

fn points_box(pts: &[Point]) -> Option<(Point, Point)> {
    let first = pts.first()?;
    let mut lo = *first;
    let mut hi = *first;
    for p in pts {
        lo.x = lo.x.min(p.x);
        lo.y = lo.y.min(p.y);
        hi.x = hi.x.max(p.x);
        hi.y = hi.y.max(p.y);
    }
    Some((lo, hi))
}

/// `GetPosition` of a drawn graphic.
pub(crate) fn graphic_position(g: &SchGraphic) -> Point {
    match &g.shape {
        SchGraphicKind::Rectangle { start, .. } | SchGraphicKind::Arc { start, .. } | SchGraphicKind::Bezier { start, .. } | SchGraphicKind::TextBox { start, .. } => *start,
        SchGraphicKind::Circle { center, .. } => *center,
        SchGraphicKind::Polygon { pts } | SchGraphicKind::RuleArea { pts, .. } => pts.first().copied().unwrap_or_default(),
        SchGraphicKind::Directive { at, .. } => *at,
    }
}

fn graphic_box(g: &SchGraphic) -> Option<(Point, Point)> {
    match &g.shape {
        SchGraphicKind::Rectangle { start, end, .. } | SchGraphicKind::TextBox { start, end, .. } => points_box(&[*start, *end]),
        SchGraphicKind::Circle { center, radius_um } => Some((Point { x: center.x - radius_um, y: center.y - radius_um }, Point { x: center.x + radius_um, y: center.y + radius_um })),
        SchGraphicKind::Arc { start, mid, end } => points_box(&[*start, *mid, *end]),
        SchGraphicKind::Bezier { start, c1, c2, end } => points_box(&[*start, *c1, *c2, *end]),
        SchGraphicKind::Polygon { pts } | SchGraphicKind::RuleArea { pts, .. } => points_box(pts),
        SchGraphicKind::Directive { at, .. } => Some((*at, *at)),
    }
}

/// The box a shape fills once placed: its four corners through the orientation and onto `at`.
fn shape_box(shape: &SymbolShape, m: Matrix, at: Point) -> Option<(Point, Point)> {
    let (x0, y0, x1, y1) = shape.bbox?;
    let pts: Vec<Point> = [(x0, y0), (x1, y0), (x1, y1), (x0, y1)].iter().map(|&(x, y)| {
        let (px, py) = m.apply(x, y);
        Point { x: at.x + px, y: at.y + py }
    }).collect();
    points_box(&pts)
}

/// Where a placed symbol's pins sit in its own frame: what the studio draws (`lib_symbols` in `GET /api/schematic`) and what KiCad
/// reads from the file the writer bakes. A sheet this project drew places a symbol by the corner of its box (`eda_engine::placed`), a
/// sheet read from KiCad by the symbol's own origin.
fn symbol_shape(model: &ConstraintModel, sch: &SchematicSection, s: &SymbolInstance) -> SymbolShape {
    let part = model.part(&s.id);
    let effective_id = if s.lib_id.is_empty() { format!("eda:{}", s.id) } else { s.lib_id.clone() };
    let lib: Option<LibSymbol> = match (sch.imported_from_kicad, part) {
        (false, Some(p)) => {
            let resolved = model.real_symbol_of(&s.lib_id, p);
            Some(eda_engine::placed::corner_symbol(&effective_id, p, resolved.as_ref(), s.unit))
        }
        _ => {
            if eda_model::is_synthetic_lib_id(&effective_id) {
                None
            } else {
                model.symbol_of(&effective_id)
            }
        }
    };
    match lib {
        Some(lib) => lib_shape(&lib, s.unit),
        None => SymbolShape::default(),
    }
}

fn power_shape(model: &ConstraintModel, p: &eda_model::ir::PowerSymbol) -> SymbolShape {
    match model.symbol_of(&p.lib_id) {
        Some(lib) => lib_shape(&lib, 1),
        None => SymbolShape { pins: vec![(String::new(), (0, 0))], bbox: Some((-300, -300, 300, 300)) },
    }
}

fn lib_shape(lib: &LibSymbol, unit: u32) -> SymbolShape {
    let on_unit = |u: u32| u == 0 || u == unit;
    // a library point (mm, y up) to the sheet's frame (micrometres, y down)
    let um = |x: f64, y: f64| ((x * 1000.0).round() as i64, (-y * 1000.0).round() as i64);
    let mut pins = Vec::new();
    let mut pts: Vec<(i64, i64)> = Vec::new();
    for p in lib.pins.iter().filter(|p| on_unit(p.unit)) {
        let tip = um(p.at.x, p.at.y);
        pins.push((p.number.clone(), tip));
        pts.push(tip);
    }
    for g in lib.graphics.iter().filter(|g| on_unit(g.unit())) {
        match g {
            SymbolGraphic::Rectangle { start, end, .. } => {
                pts.push(um(start.x, start.y));
                pts.push(um(end.x, end.y));
            }
            SymbolGraphic::Polyline { pts: ps, .. } => pts.extend(ps.iter().map(|p| um(p.x, p.y))),
            SymbolGraphic::Circle { center, radius_mm, .. } => {
                pts.push(um(center.x - radius_mm, center.y - radius_mm));
                pts.push(um(center.x + radius_mm, center.y + radius_mm));
            }
            SymbolGraphic::Arc { start, mid, end, .. } => {
                pts.push(um(start.x, start.y));
                pts.push(um(mid.x, mid.y));
                pts.push(um(end.x, end.y));
            }
            SymbolGraphic::Text { .. } => {}
        }
    }
    let bbox = pts.iter().fold(None, |acc: Option<(i64, i64, i64, i64)>, &(x, y)| match acc {
        None => Some((x, y, x, y)),
        Some((x0, y0, x1, y1)) => Some((x0.min(x), y0.min(y), x1.max(x), y1.max(y))),
    });
    SymbolShape { pins, bbox }
}

/// The spin a label has when nothing set one: read off the wire that ends at its anchor (`labelShape.ts::inferSpin`, which draws labels
/// that way). A label no wire ends at reads to the right.
pub(crate) fn inferred_spin(segs: &[Seg], at: Point) -> LabelSpin {
    let tol = 50;
    let near = |p: Point| (p.x - at.x).abs() <= tol && (p.y - at.y).abs() <= tol;
    for s in segs.iter().filter(|s| !s.dead) {
        let hit = if near(s.b) {
            Some((s.b.x - s.a.x, s.b.y - s.a.y))
        } else if near(s.a) {
            Some((s.a.x - s.b.x, s.a.y - s.b.y))
        } else {
            None
        };
        if let Some((dx, dy)) = hit {
            return if dx.abs() >= dy.abs() {
                if dx >= 0 {
                    LabelSpin::Right
                } else {
                    LabelSpin::Left
                }
            } else if dy >= 0 {
                LabelSpin::Bottom
            } else {
                LabelSpin::Up
            };
        }
    }
    LabelSpin::Right
}

