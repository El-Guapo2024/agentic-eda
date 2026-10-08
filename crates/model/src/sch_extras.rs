//! Additive schematic items and per-item annotations: the data behind the schematic editor's drawing
//! and edit tools that the core schematic items (`SymbolInstance`, `Wire`, `NetLabel`, ...) do not
//! carry. Everything here is optional (`SchematicSection::extras`, default empty and never written
//! when empty), so a design that never used one of these tools serializes exactly as before.
//!
//! Why one `extras` struct instead of a new field per item kind: `SchematicSection` and its items are
//! built with struct literals in many places (tests, importers, the derived-schematic generator), so
//! every new field there means touching every literal. Growing this struct touches none of them.
//!
//! What lives here, each with the KiCad class it mirrors:
//! - [`SchGraphic`]: rectangles, circles, arcs, beziers and polygons (`SCH_SHAPE`), text boxes
//!   (`SCH_TEXTBOX`), rule areas (`SCH_RULE_AREA`) and directive labels (`SCH_DIRECTIVE_LABEL`, the
//!   "netclass flag"). Decoration or annotation: none of them is part of any net.
//! - `locked`: the ids of locked items (`SCH_ITEM::IsLocked`, set by Lock / Unlock / Toggle Lock).
//! - `label_spins`: which way a label's text runs from its anchor (`SCH_LABEL_BASE::GetSpinStyle`), once Rotate or Mirror has set it.

use crate::ir::{Millideg, Point, Um};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// Items and annotations beyond the core schematic content -- see the module doc.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SchExtras {
    /// Drawn graphics and annotations, in drawing order (later ones paint over earlier ones).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub graphics: Vec<SchGraphic>,
    /// Ids of locked items, sorted and unique. A symbol is keyed by its reference (every unit of a
    /// multi-unit part locks together, the way the studio selects them together).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub locked: Vec<String>,
    /// `SCH_SCREEN::GetPageSettings()`: the sheet's paper (Page Settings, `common.Control.pageSettings`), written as `(paper ...)`.
    /// `None` is KiCad's default, A4 landscape.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub page: Option<crate::page::PageSettings>,
    /// A label's spin (`SCH_LABEL_BASE::GetSpinStyle`: which side of its anchor the text runs to), by label id. A label with no entry
    /// has its spin read off the wire that ends at it (`labelShape.ts::inferSpin`), as every label did before Rotate and Mirror could
    /// set one; an entry is written only when a turn or a mirror set it, so a design that never turned a label serializes as before.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub label_spins: BTreeMap<String, LabelSpin>,
}

/// `SPIN_STYLE`: which side of its anchor a label's text sits on and runs to. `Right` is the usual label (text to the right of a wire that
/// comes from the left); `Up` reads upward from the anchor, `Left` ends at it, `Bottom` hangs below it reading upward.
/// Turning a label counter-clockwise goes `Right`, `Up`, `Left`, `Bottom` (`SCH_TEXT::Rotate90`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LabelSpin {
    Right,
    Up,
    Left,
    Bottom,
}

impl LabelSpin {
    /// One quarter turn: counter-clockwise when `ccw` (`SCH_TEXT::Rotate90( !ccw )`: the horizontal/vertical angle toggles and the
    /// justification flips in two of the four cases, which is this cycle).
    pub fn rotated(self, ccw: bool) -> LabelSpin {
        use LabelSpin::*;
        match (self, ccw) {
            (Right, true) | (Left, false) => Up,
            (Up, true) | (Bottom, false) => Left,
            (Left, true) | (Right, false) => Bottom,
            (Bottom, true) | (Up, false) => Right,
        }
    }

    /// `SCH_LABEL_BASE::MirrorSpinStyle( aLeftRight )`: a left-right mirror swaps `Right` and `Left`, a top-bottom one `Up` and `Bottom`;
    /// the other axis leaves the spin as it is.
    pub fn mirrored(self, left_right: bool) -> LabelSpin {
        use LabelSpin::*;
        match (self, left_right) {
            (Right, true) => Left,
            (Left, true) => Right,
            (Up, false) => Bottom,
            (Bottom, false) => Up,
            (s, _) => s,
        }
    }
}

impl SchExtras {
    pub fn is_empty(&self) -> bool {
        self.graphics.is_empty() && self.locked.is_empty() && self.page.is_none() && self.label_spins.is_empty()
    }

    /// True when `id` is locked.
    pub fn is_locked(&self, id: &str) -> bool {
        self.locked.binary_search_by(|l| l.as_str().cmp(id)).is_ok()
    }

    /// Lock or unlock `id` (keeping `locked` sorted and unique). Returns whether anything changed.
    pub fn set_locked(&mut self, id: &str, locked: bool) -> bool {
        match self.locked.binary_search_by(|l| l.as_str().cmp(id)) {
            Ok(i) if !locked => {
                self.locked.remove(i);
                true
            }
            Err(i) if locked => {
                self.locked.insert(i, id.to_string());
                true
            }
            _ => false,
        }
    }

    /// Backfill `id` on every graphic that has none, deterministically from its content (same
    /// contract as `SchematicSection::assign_missing_ids`). `existing` is the set of ids already
    /// taken in the section; it grows as ids are handed out.
    pub fn assign_missing_ids(&mut self, existing: &mut BTreeSet<String>) {
        existing.extend(self.graphics.iter().map(|g| g.id.clone()).filter(|s| !s.is_empty()));
        for i in 0..self.graphics.len() {
            if self.graphics[i].id.is_empty() {
                let id = crate::ir::next_item_id(self.graphics[i].shape.id_prefix(), &self.graphics[i].id_seed(), existing);
                existing.insert(id.clone());
                self.graphics[i].id = id;
            }
        }
    }

    /// Canonical order for hashing: `locked` is kept sorted by its setter;
    /// `graphics` keeps its drawing order (it is meaningful), so there is nothing to sort.
    pub fn canonicalize(&mut self) {
        self.locked.sort();
        self.locked.dedup();
    }
}

/// Stroke style of a graphic (`LINE_STYLE`): `Default` follows the sheet's own default line style.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SchLineStyle {
    #[default]
    Default,
    Solid,
    Dash,
    Dot,
    DashDot,
    DashDotDot,
}

impl SchLineStyle {
    pub fn is_default(&self) -> bool {
        *self == SchLineStyle::Default
    }

    /// The `(stroke (type ...))` token KiCad writes.
    pub fn token(self) -> &'static str {
        match self {
            SchLineStyle::Default => "default",
            SchLineStyle::Solid => "solid",
            SchLineStyle::Dash => "dash",
            SchLineStyle::Dot => "dot",
            SchLineStyle::DashDot => "dash_dot",
            SchLineStyle::DashDotDot => "dash_dot_dot",
        }
    }

    pub fn from_token(s: &str) -> SchLineStyle {
        match s {
            "solid" => SchLineStyle::Solid,
            "dash" => SchLineStyle::Dash,
            "dot" => SchLineStyle::Dot,
            "dash_dot" => SchLineStyle::DashDot,
            "dash_dot_dot" => SchLineStyle::DashDotDot,
            _ => SchLineStyle::Default,
        }
    }
}

/// How a closed graphic is filled (`FILL_T`): `Outline` fills with the stroke colour, `Background`
/// with the symbol-body background colour, `Color` with [`SchGraphic::fill_color`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SchFill {
    #[default]
    None,
    Outline,
    Background,
    Color,
}

impl SchFill {
    pub fn is_none(&self) -> bool {
        *self == SchFill::None
    }

    /// The `(fill (type ...))` token KiCad writes.
    pub fn token(self) -> &'static str {
        match self {
            SchFill::None => "none",
            SchFill::Outline => "outline",
            SchFill::Background => "background",
            SchFill::Color => "color",
        }
    }

    pub fn from_token(s: &str) -> SchFill {
        match s {
            "outline" => SchFill::Outline,
            "background" => SchFill::Background,
            "color" => SchFill::Color,
            _ => SchFill::None,
        }
    }
}

/// An RGBA colour, each channel 0..=255.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SchColor {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

/// Horizontal text alignment (`GR_TEXT_H_ALIGN_T`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SchHAlign {
    #[default]
    Left,
    Center,
    Right,
}

/// Vertical text alignment (`GR_TEXT_V_ALIGN_T`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SchVAlign {
    #[default]
    Top,
    Center,
    Bottom,
}

/// The outline of a directive label's flag (`LABEL_FLAG_SHAPE`'s `F_*` values).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DirectiveShape {
    Dot,
    #[default]
    Round,
    Diamond,
    Rectangle,
}

impl DirectiveShape {
    pub fn token(self) -> &'static str {
        match self {
            DirectiveShape::Dot => "dot",
            DirectiveShape::Round => "round",
            DirectiveShape::Diamond => "diamond",
            DirectiveShape::Rectangle => "rectangle",
        }
    }

    pub fn from_token(s: &str) -> DirectiveShape {
        match s {
            "dot" => DirectiveShape::Dot,
            "diamond" => DirectiveShape::Diamond,
            "rectangle" => DirectiveShape::Rectangle,
            _ => DirectiveShape::Round,
        }
    }
}

/// What kind of graphic an [`SchGraphic`] is, with its own geometry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum SchGraphicKind {
    /// `SCH_SHAPE` rectangle: two opposite corners (not normalized -- the order they were drawn in).
    Rectangle {
        start: Point,
        end: Point,
        #[serde(default, skip_serializing_if = "is_zero_um")]
        corner_radius_um: Um,
    },
    /// `SCH_SHAPE` circle: centre and radius.
    Circle { center: Point, radius_um: Um },
    /// `SCH_SHAPE` arc: start, a point on the arc between them, and end (KiCad's own three-point form).
    Arc { start: Point, mid: Point, end: Point },
    /// `SCH_SHAPE` bezier: start, control 1, control 2, end.
    Bezier { start: Point, c1: Point, c2: Point, end: Point },
    /// `SCH_SHAPE` polygon: a closed outline of three or more points.
    Polygon { pts: Vec<Point> },
    /// `SCH_TEXTBOX`: a rectangle with text inside. `angle` is 0 or 90000 (KiCad keeps a text box's
    /// text horizontal or vertical only); `margin_um` is the gap between the border and the text.
    TextBox {
        start: Point,
        end: Point,
        text: String,
        #[serde(default, skip_serializing_if = "is_zero_angle")]
        angle: Millideg,
        size_um: Um,
        #[serde(default, skip_serializing_if = "is_false")]
        bold: bool,
        #[serde(default, skip_serializing_if = "is_false")]
        italic: bool,
        #[serde(default)]
        h_align: SchHAlign,
        #[serde(default)]
        v_align: SchVAlign,
        #[serde(default, skip_serializing_if = "is_zero_um")]
        margin_um: Um,
    },
    /// `SCH_RULE_AREA`: a polygon that applies its attributes to everything inside it and to the
    /// nets crossing it (a netclass directive label on its border names the class).
    RuleArea {
        pts: Vec<Point>,
        #[serde(default, skip_serializing_if = "is_false")]
        exclude_from_sim: bool,
        #[serde(default, skip_serializing_if = "is_false")]
        exclude_from_bom: bool,
        #[serde(default, skip_serializing_if = "is_false")]
        exclude_from_board: bool,
        #[serde(default, skip_serializing_if = "is_false")]
        dnp: bool,
    },
    /// `SCH_DIRECTIVE_LABEL` (the "netclass flag"): a flag on a pole that attaches its fields to the
    /// net it touches. `orientation` is the pole's direction in millidegrees (0 right, 90000 up,
    /// 180000 left, 270000 down).
    Directive {
        at: Point,
        #[serde(default, skip_serializing_if = "is_zero_angle")]
        orientation: Millideg,
        #[serde(default)]
        shape: DirectiveShape,
        pin_length_um: Um,
        #[serde(default, skip_serializing_if = "String::is_empty")]
        netclass: String,
        #[serde(default, skip_serializing_if = "String::is_empty")]
        component_class: String,
    },
}

impl SchGraphicKind {
    /// The id prefix for this kind (`next_item_id`'s first argument).
    pub fn id_prefix(&self) -> &'static str {
        match self {
            SchGraphicKind::TextBox { .. } => "tbox",
            SchGraphicKind::RuleArea { .. } => "rarea",
            SchGraphicKind::Directive { .. } => "dirl",
            _ => "shp",
        }
    }

    /// True for the kinds drawn with a stroked outline that may be filled.
    pub fn is_closed_shape(&self) -> bool {
        matches!(self, SchGraphicKind::Rectangle { .. } | SchGraphicKind::Circle { .. } | SchGraphicKind::Polygon { .. } | SchGraphicKind::TextBox { .. } | SchGraphicKind::RuleArea { .. } | SchGraphicKind::Bezier { .. })
    }
}

/// One drawn graphic or annotation -- see [`SchGraphicKind`]. Stroke and fill apply to every kind
/// except `Directive` (which draws with the directive-label colour).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SchGraphic {
    /// Stable id (`shp_`/`tbox_`/`rarea_`/`dirl_` + 12 hex) -- see `Wire::id`'s doc.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub id: String,
    pub shape: SchGraphicKind,
    /// Stroke width, um; 0 means the default line width.
    #[serde(default, skip_serializing_if = "is_zero_um")]
    pub width_um: Um,
    #[serde(default, skip_serializing_if = "SchLineStyle::is_default")]
    pub line_style: SchLineStyle,
    /// Stroke colour; absent means the layer's own colour.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<SchColor>,
    #[serde(default, skip_serializing_if = "SchFill::is_none")]
    pub fill: SchFill,
    /// The fill colour when `fill` is `Color`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fill_color: Option<SchColor>,
}

impl SchGraphic {
    /// A graphic of `shape` with every style at its default.
    pub fn new(shape: SchGraphicKind) -> SchGraphic {
        SchGraphic { id: String::new(), shape, width_um: 0, line_style: SchLineStyle::Default, color: None, fill: SchFill::None, fill_color: None }
    }

    /// The seed `assign_missing_ids` hashes: the kind's own geometry, so the same drawing always gets the same id.
    pub fn id_seed(&self) -> String {
        let p = |a: &Point| format!("{},{}", a.x, a.y);
        match &self.shape {
            SchGraphicKind::Rectangle { start, end, .. } => format!("rect|{}|{}", p(start), p(end)),
            SchGraphicKind::Circle { center, radius_um } => format!("circle|{}|{radius_um}", p(center)),
            SchGraphicKind::Arc { start, mid, end } => format!("arc|{}|{}|{}", p(start), p(mid), p(end)),
            SchGraphicKind::Bezier { start, c1, c2, end } => format!("bezier|{}|{}|{}|{}", p(start), p(c1), p(c2), p(end)),
            SchGraphicKind::Polygon { pts } => format!("poly|{}", pts.iter().map(p).collect::<Vec<_>>().join(";")),
            SchGraphicKind::TextBox { start, end, text, .. } => format!("tbox|{}|{}|{text}", p(start), p(end)),
            SchGraphicKind::RuleArea { pts, .. } => format!("rarea|{}", pts.iter().map(p).collect::<Vec<_>>().join(";")),
            SchGraphicKind::Directive { at, netclass, .. } => format!("dirl|{}|{netclass}", p(at)),
        }
    }
}

fn is_zero_um(v: &Um) -> bool {
    *v == 0
}

fn is_zero_angle(v: &Millideg) -> bool {
    *v == 0
}

fn is_false(v: &bool) -> bool {
    !*v
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(x: Um, y: Um) -> Point {
        Point { x, y }
    }

    #[test]
    fn empty_extras_serialize_to_nothing_and_back() {
        let e = SchExtras::default();
        assert!(e.is_empty());
        assert_eq!(serde_json::to_string(&e).unwrap(), "{}");
        let back: SchExtras = serde_json::from_str("{}").unwrap();
        assert!(back.is_empty());
    }

    #[test]
    fn locking_keeps_the_ids_sorted_and_unique() {
        let mut e = SchExtras::default();
        assert!(e.set_locked("U2", true));
        assert!(e.set_locked("R1", true));
        assert!(!e.set_locked("R1", true), "already locked: no change");
        assert_eq!(e.locked, vec!["R1".to_string(), "U2".to_string()]);
        assert!(e.is_locked("U2") && !e.is_locked("U1"));
        assert!(e.set_locked("R1", false));
        assert!(!e.set_locked("R1", false), "already unlocked: no change");
        assert_eq!(e.locked, vec!["U2".to_string()]);
    }

    #[test]
    fn graphics_round_trip_through_json_and_get_distinct_ids() {
        let mut e = SchExtras::default();
        let mut r = SchGraphic::new(SchGraphicKind::Rectangle { start: p(0, 0), end: p(10_000, 5_000), corner_radius_um: 0 });
        r.fill = SchFill::Background;
        e.graphics.push(r);
        e.graphics.push(SchGraphic::new(SchGraphicKind::Circle { center: p(1, 2), radius_um: 3_000 }));
        e.graphics.push(SchGraphic::new(SchGraphicKind::RuleArea { pts: vec![p(0, 0), p(10, 0), p(10, 10)], exclude_from_sim: false, exclude_from_bom: true, exclude_from_board: false, dnp: false }));
        e.graphics.push(SchGraphic::new(SchGraphicKind::Directive { at: p(5, 5), orientation: 90_000, shape: DirectiveShape::Round, pin_length_um: 2_540, netclass: "HV".into(), component_class: String::new() }));
        let mut taken = BTreeSet::new();
        e.assign_missing_ids(&mut taken);
        let ids: Vec<&str> = e.graphics.iter().map(|g| g.id.as_str()).collect();
        assert!(ids[0].starts_with("shp_") && ids[1].starts_with("shp_") && ids[2].starts_with("rarea_") && ids[3].starts_with("dirl_"), "{ids:?}");
        assert_eq!(taken.len(), 4);
        let json = serde_json::to_string(&e).unwrap();
        let back: SchExtras = serde_json::from_str(&json).unwrap();
        assert_eq!(back, e);
        // Assigning again changes nothing.
        let mut again = back.clone();
        again.assign_missing_ids(&mut BTreeSet::new());
        assert_eq!(again, back);
    }
}
