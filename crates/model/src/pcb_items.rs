//! The board items the IR did not have (GAPS.md item 24, package P16): the text box (`PCB_TEXTBOX`), the reference image (`PCB_REFERENCE_IMAGE`), the point (`PCB_POINT`) and a
//! shape's hatch fill. A board file that has them keeps them here (`DrawingsSection::board_items`), the importer reads them, the exporter writes them back as
//! `PCB_IO_KICAD_SEXPR::format` writes them, the canvas draws them and the verbs of `eda_ops` edit them. Before this, a text box or an image in a KiCad file was
//! carried as the text of its s-expression (`DrawingsSection::raw_items`) and nothing more.
//!
//! One field of `DrawingsSection` holds all of them, `board_items`, for the reason `rules` does: `DrawingsSection` is written out field by field in a good many places
//! and a new field there is a change at every one. Every collection is additive: an older `design.json` has none and reads as "no such items".

use crate::ir::{Millideg, Point, TextJustify, TextVAlign, Um};
use crate::sch_extras::SchLineStyle;
use serde::{Deserialize, Serialize};

/// The stroke type of a border or an outline (`LINE_STYLE`). The schematic's type is the same enumeration; `Default` follows the item's class.
pub type LineStyle = SchLineStyle;

fn d_true() -> bool {
    true
}
fn is_true(b: &bool) -> bool {
    *b
}
fn is_false(b: &bool) -> bool {
    !*b
}
fn is_zero_um(n: &Um) -> bool {
    *n == 0
}
fn is_zero_angle(a: &Millideg) -> bool {
    *a == 0
}
fn is_zero_i64(n: &i64) -> bool {
    *n == 0
}

/// `EDA_TEXT::GetPenSizeForNormal` / `GetPenSizeForBold` (`common/eda_text.cpp`) for a glyph `width_um` wide: an eighth and a fifth of it.
pub fn auto_pen_um(width_um: Um, bold: bool) -> Um {
    (width_um as f64 / if bold { 5.0 } else { 8.0 }).round() as Um
}

/// `PCB_TEXTBOX::GetLegacyTextMargin`: the margin of a box the file gave none (and a new box starts with), half the border width and three quarters of the text height.
pub fn legacy_text_margin_um(stroke_width_um: Um, text_height_um: Um) -> Um {
    (stroke_width_um as f64 / 2.0).round() as Um + (text_height_um as f64 * 0.75).round() as Um
}

/// A text box: wrapped text in a rectangle, with a border (`PCB_TEXTBOX`, `(gr_text_box ..)`).
///
/// The geometry is what the file keeps. A box turned by a multiple of 90 degrees is a rectangle, `start` and `end` its two corners as drawn (not ordered: `Move`, `Rotate`
/// and `Flip` carry them as they are, and the text's angle says which corner is the top left, `PCB_TEXTBOX::GetTopLeft`); a box turned by any other angle is a polygon of
/// four corners, `pts` (`SHAPE_T::POLY`), and `start` and `end` are the first and third of them. The text's own angle (`angle`) is the box's: the text runs along the box.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TextBox {
    /// Stable id (`tbx_xxxxxxxxxxxx`), made when the box is read or added, as every drawing's is.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub id: String,
    /// The layer the box and its text are on.
    pub layer: String,
    /// The text as stored: `${VARIABLE}`s unexpanded, lines separated by a line feed.
    pub text: String,
    pub start: Point,
    pub end: Point,
    /// The four corners of a box turned by an angle that is not a multiple of 90 degrees, in the file's order; empty for a rectangle.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pts: Vec<Point>,
    /// The text angle in the file's sense (counter-clockwise on the screen), millidegrees, 0 to 360000.
    #[serde(default, skip_serializing_if = "is_zero_angle")]
    pub angle: Millideg,
    /// `(margins L T R B)`: the gap between the border and the text, µm.
    pub margin_left_um: Um,
    pub margin_top_um: Um,
    pub margin_right_um: Um,
    pub margin_bottom_um: Um,
    /// `(border yes|no)`: the border is drawn.
    #[serde(default = "d_true", skip_serializing_if = "is_true")]
    pub border: bool,
    /// The border's width, µm (`(stroke (width ..))`).
    pub stroke_width_um: Um,
    #[serde(default)]
    pub line_style: LineStyle,
    /// The `r g b a` of a `(stroke .. (color r g b a))`, as the file wrote it; the canvas draws a layer in its own colour.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stroke_color: Option<String>,
    /// `(knockout yes)`: the box is filled and the text is cut out of it (`PCB_TEXTBOX::TransformTextToPolySet`).
    #[serde(default, skip_serializing_if = "is_false")]
    pub knockout: bool,
    /// The glyph height, µm (`(size HEIGHT WIDTH)`).
    pub size_um: Um,
    /// The glyph width, µm, when it is not the height.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub width_um: Option<Um>,
    /// The pen width, µm; 0 is KiCad's automatic width (`GetPenSizeForNormal`, `GetPenSizeForBold`) and the file has no `(thickness ..)`.
    #[serde(default, skip_serializing_if = "is_zero_um")]
    pub thickness_um: Um,
    /// Horizontal and vertical justification of the text in the box (`(justify ..)`): a new box is left and top, one the file says nothing of centred.
    #[serde(default)]
    pub halign: TextJustify,
    #[serde(default)]
    pub valign: TextVAlign,
    /// `(justify mirror)`: the glyphs are mirrored, as text on a back layer is.
    #[serde(default, skip_serializing_if = "is_false")]
    pub mirror: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub bold: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub italic: bool,
    /// `(line_spacing N)` in thousandths; 0 is KiCad's 1.0.
    #[serde(default, skip_serializing_if = "is_zero_i64")]
    pub line_spacing_permille: i64,
    /// `(face "..")`: the outline font the text is set in. Stroke text, the only kind drawn here, has none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub face: Option<String>,
}

impl TextBox {
    /// A box on `layer` between two corners, with the looks a text box the Draw Text Box tool makes starts with: the layer's text size and pen
    /// (`DRAWING_TOOL::m_textAttrs`), left and top, the default margins and a border of the layer's line width.
    pub fn new(layer: &str, start: Point, end: Point, size_um: Um, stroke_width_um: Um) -> TextBox {
        let margin = legacy_text_margin_um(stroke_width_um, size_um);
        TextBox {
            id: String::new(),
            layer: layer.to_string(),
            text: String::new(),
            start,
            end,
            pts: Vec::new(),
            angle: 0,
            margin_left_um: margin,
            margin_top_um: margin,
            margin_right_um: margin,
            margin_bottom_um: margin,
            border: true,
            stroke_width_um,
            line_style: LineStyle::Default,
            stroke_color: None,
            knockout: false,
            size_um,
            width_um: None,
            thickness_um: 0,
            halign: TextJustify::Left,
            valign: TextVAlign::Top,
            mirror: layer.starts_with("B."),
            bold: false,
            italic: false,
            line_spacing_permille: 0,
            face: None,
        }
    }

    /// The seed `assign_missing_ids` hashes.
    pub fn id_seed(&self) -> String {
        format!("{}|{},{}|{},{}|{}|{}", self.text, self.start.x, self.start.y, self.end.x, self.end.y, self.angle, self.layer)
    }

    /// The glyph width, µm (`EDA_TEXT::GetTextWidth`).
    pub fn text_width_um(&self) -> Um {
        self.width_um.unwrap_or(self.size_um)
    }

    /// The pen width the text is drawn with (`EDA_TEXT::GetEffectiveTextPenWidth`): the file's, else the automatic one for the smaller glyph side.
    pub fn effective_pen_um(&self) -> Um {
        if self.thickness_um > 0 {
            self.thickness_um
        } else {
            auto_pen_um(self.text_width_um().min(self.size_um), self.bold)
        }
    }

    /// Whether the box is a rectangle (turned by a multiple of 90 degrees) and not a polygon.
    pub fn is_rect(&self) -> bool {
        self.pts.is_empty()
    }

    /// Every corner the geometry is made of, in the file's order.
    pub fn points(&self) -> Vec<Point> {
        if self.pts.is_empty() {
            vec![self.start, self.end]
        } else {
            self.pts.clone()
        }
    }

    /// The box's extent `[x0, y0, x1, y1]`, µm, border width not included.
    pub fn bounds(&self) -> [Um; 4] {
        let pts = self.points();
        let (mut x0, mut y0, mut x1, mut y1) = (Um::MAX, Um::MAX, Um::MIN, Um::MIN);
        for p in &pts {
            x0 = x0.min(p.x);
            y0 = y0.min(p.y);
            x1 = x1.max(p.x);
            y1 = y1.max(p.y);
        }
        [x0, y0, x1, y1]
    }
}

/// The items this module adds to a board. See the module doc.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BoardItems {
    /// The board's text boxes (`gr_text_box`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub text_boxes: Vec<TextBox>,
}

impl BoardItems {
    pub fn is_empty(&self) -> bool {
        self.text_boxes.is_empty()
    }

    /// Every id of the items, for the id sets that name them.
    pub fn ids(&self) -> impl Iterator<Item = &str> {
        self.text_boxes.iter().map(|t| t.id.as_str())
    }

    /// Give each item without an id the deterministic one the others get (`next_item_id`), in an order that does not depend on how they were listed.
    pub fn assign_missing_ids(&mut self, existing: &mut std::collections::BTreeSet<String>) {
        let mut order: Vec<usize> = (0..self.text_boxes.len()).collect();
        order.sort_by(|&a, &b| (&self.text_boxes[a].text, self.text_boxes[a].start, self.text_boxes[a].end).cmp(&(&self.text_boxes[b].text, self.text_boxes[b].start, self.text_boxes[b].end)));
        for i in order {
            if self.text_boxes[i].id.is_empty() {
                let id = crate::ir::next_item_id("tbx", &self.text_boxes[i].id_seed(), existing);
                existing.insert(id.clone());
                self.text_boxes[i].id = id;
            }
        }
    }

    /// Sort the collections by id (the canonical form a design is hashed in).
    pub fn canonicalize(&mut self) {
        self.text_boxes.sort_by(|a, b| a.id.cmp(&b.id));
    }

    /// Put each item on the layer `map` names for its own, when it names one (Swap Layers, `PCB_EDIT_FRAME::SwapLayers`).
    pub fn swap_layers(&mut self, map: &dyn Fn(&str) -> Option<String>) {
        for t in &mut self.text_boxes {
            if let Some(dest) = map(&t.layer) {
                t.layer = dest;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_new_text_box_has_the_margin_of_its_layer_defaults() {
        // 0.12 mm border, 1 mm text: 60 + 750.
        let b = TextBox::new("F.SilkS", Point { x: 0, y: 0 }, Point { x: 10_000, y: 5_000 }, 1_000, 120);
        assert_eq!((b.margin_left_um, b.margin_top_um, b.margin_right_um, b.margin_bottom_um), (810, 810, 810, 810));
        assert_eq!((b.halign, b.valign, b.mirror, b.border), (TextJustify::Left, TextVAlign::Top, false, true));
        assert!(TextBox::new("B.SilkS", Point::default(), Point::default(), 1_000, 120).mirror, "text on a back layer is mirrored");
    }

    #[test]
    fn the_pen_is_the_files_or_an_eighth_of_the_smaller_glyph_side() {
        let mut b = TextBox::new("F.SilkS", Point::default(), Point::default(), 1_000, 120);
        assert_eq!(b.effective_pen_um(), 125);
        b.bold = true;
        assert_eq!(b.effective_pen_um(), 200);
        b.width_um = Some(800);
        assert_eq!(b.effective_pen_um(), 160);
        b.thickness_um = 150;
        assert_eq!(b.effective_pen_um(), 150);
    }

    #[test]
    fn items_serialize_to_nothing_when_there_are_none_and_ids_are_deterministic() {
        let mut items = BoardItems::default();
        assert_eq!(serde_json::to_string(&items).unwrap(), "{}");
        let mut t = TextBox::new("F.SilkS", Point { x: 1, y: 2 }, Point { x: 3, y: 4 }, 1_000, 120);
        t.text = "hello".into();
        items.text_boxes.push(t.clone());
        items.text_boxes.push(t);
        let mut existing = std::collections::BTreeSet::new();
        items.assign_missing_ids(&mut existing);
        assert!(items.text_boxes[0].id.starts_with("tbx_") && items.text_boxes[1].id.starts_with("tbx_"));
        assert_ne!(items.text_boxes[0].id, items.text_boxes[1].id, "two boxes alike still get two ids");
        let back: BoardItems = serde_json::from_str(&serde_json::to_string(&items).unwrap()).unwrap();
        assert_eq!(back, items);
    }
}
