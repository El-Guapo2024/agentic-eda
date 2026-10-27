//! The board items the IR did not have (GAPS.md item 24, package P16): the text box (`PCB_TEXTBOX`), the reference image (`PCB_REFERENCE_IMAGE`), the point (`PCB_POINT`) and a
//! shape's hatch fill. A board file that has them keeps them here (`DrawingsSection::board_items`), the importer reads them, the exporter writes them back as
//! `PCB_IO_KICAD_SEXPR::format` writes them, the canvas draws them and the verbs of `eda_ops` edit them. Before this, a text box or an image in a KiCad file was
//! carried as the text of its s-expression (`DrawingsSection::raw_items`) and nothing more.
//!
//! One field of `DrawingsSection` holds all of them, `board_items`, for the reason `rules` does: `DrawingsSection` is written out field by field in a good many places
//! and a new field there is a change at every one. Every collection is additive: an older `design.json` has none and reads as "no such items".

use crate::ir::{Millideg, Point, TextJustify, TextVAlign, Nm};
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
fn is_zero_nm(n: &Nm) -> bool {
    *n == Nm::ZERO
}
fn is_zero_angle(a: &Millideg) -> bool {
    *a == 0
}
fn is_zero_i64(n: &i64) -> bool {
    *n == 0
}

/// `EDA_TEXT::GetPenSizeForNormal` / `GetPenSizeForBold` (`common/eda_text.cpp`) for a glyph `width_nm` wide: an eighth and a fifth of it.
pub fn auto_pen_nm(width_nm: Nm, bold: bool) -> Nm {
    Nm::kiround(width_nm.as_f64() / if bold { 5.0 } else { 8.0 })
}

/// `PCB_TEXTBOX::GetLegacyTextMargin`: the margin of a box the file gave none (and a new box starts with), half the border width and three quarters of the text height.
pub fn legacy_text_margin_nm(stroke_width_nm: Nm, text_height_nm: Nm) -> Nm {
    Nm::kiround(stroke_width_nm.as_f64() / 2.0) + Nm::kiround(text_height_nm.as_f64() * 0.75)
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
    /// `(margins L T R B)`: the gap between the border and the text, nm.
    #[serde(alias = "margin_left_um", deserialize_with = "crate::units::de")]
    pub margin_left_nm: Nm,
    #[serde(alias = "margin_top_um", deserialize_with = "crate::units::de")]
    pub margin_top_nm: Nm,
    #[serde(alias = "margin_right_um", deserialize_with = "crate::units::de")]
    pub margin_right_nm: Nm,
    #[serde(alias = "margin_bottom_um", deserialize_with = "crate::units::de")]
    pub margin_bottom_nm: Nm,
    /// `(border yes|no)`: the border is drawn.
    #[serde(default = "d_true", skip_serializing_if = "is_true")]
    pub border: bool,
    /// The border's width, nm (`(stroke (width ..))`).
    #[serde(alias = "stroke_width_um", deserialize_with = "crate::units::de")]
    pub stroke_width_nm: Nm,
    #[serde(default)]
    pub line_style: LineStyle,
    /// The `r g b a` of a `(stroke .. (color r g b a))`, as the file wrote it; the canvas draws a layer in its own colour.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stroke_color: Option<String>,
    /// `(knockout yes)`: the box is filled and the text is cut out of it (`PCB_TEXTBOX::TransformTextToPolySet`).
    #[serde(default, skip_serializing_if = "is_false")]
    pub knockout: bool,
    /// The glyph height, nm (`(size HEIGHT WIDTH)`).
    #[serde(alias = "size_um", deserialize_with = "crate::units::de")]
    pub size_nm: Nm,
    /// The glyph width, nm, when it is not the height.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(alias = "width_um", deserialize_with = "crate::units::de")]
    pub width_nm: Option<Nm>,
    /// The pen width, nm; 0 is KiCad's automatic width (`GetPenSizeForNormal`, `GetPenSizeForBold`) and the file has no `(thickness ..)`.
    #[serde(default, skip_serializing_if = "is_zero_nm")]
    #[serde(alias = "thickness_um", deserialize_with = "crate::units::de")]
    pub thickness_nm: Nm,
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
    pub fn new(layer: &str, start: Point, end: Point, size_nm: Nm, stroke_width_nm: Nm) -> TextBox {
        let margin = legacy_text_margin_nm(stroke_width_nm, size_nm);
        TextBox {
            id: String::new(),
            layer: layer.to_string(),
            text: String::new(),
            start,
            end,
            pts: Vec::new(),
            angle: 0,
            margin_left_nm: margin,
            margin_top_nm: margin,
            margin_right_nm: margin,
            margin_bottom_nm: margin,
            border: true,
            stroke_width_nm,
            line_style: LineStyle::Default,
            stroke_color: None,
            knockout: false,
            size_nm,
            width_nm: None,
            thickness_nm: Nm::ZERO,
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

    /// The glyph width, nm (`EDA_TEXT::GetTextWidth`).
    pub fn text_width_nm(&self) -> Nm {
        self.width_nm.unwrap_or(self.size_nm)
    }

    /// The pen width the text is drawn with (`EDA_TEXT::GetEffectiveTextPenWidth`): the file's, else the automatic one for the smaller glyph side.
    pub fn effective_pen_nm(&self) -> Nm {
        if self.thickness_nm > Nm::ZERO {
            self.thickness_nm
        } else {
            auto_pen_nm(self.text_width_nm().min(self.size_nm), self.bold)
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

    /// The box's extent `[x0, y0, x1, y1]`, nm, border width not included.
    pub fn bounds(&self) -> [Nm; 4] {
        let pts = self.points();
        let (mut x0, mut y0, mut x1, mut y1) = (Nm::MAX, Nm::MAX, Nm::MIN, Nm::MIN);
        for p in &pts {
            x0 = x0.min(p.x);
            y0 = y0.min(p.y);
            x1 = x1.max(p.x);
            y1 = y1.max(p.y);
        }
        [x0, y0, x1, y1]
    }
}

/// A reference image: a bitmap on a graphic layer, kept out of every plot and every 3D view (`PCB_REFERENCE_IMAGE`, `(image ..)`).
///
/// The pixels are the file the board embeds, held as the base64 text its `(data ..)` token carries, with the line breaks taken out, so a file written back has the bytes
/// it was read with. The size, in pixels and in pixels per inch, is read from that file's header ([`crate::image_info`]); the size on the board is the pixels at their
/// definition times `scale_ppm`. `at` is the image's centre (`REFERENCE_IMAGE::GetPosition`), the box it covers is [`RefImage::bounds`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RefImage {
    /// Stable id (`img_xxxxxxxxxxxx`), made when the image is read or added.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub id: String,
    /// The layer the image is on; its visibility is the layer's, and the Images object's (`LAYER_DRAW_BITMAPS`).
    pub layer: String,
    /// The image's centre, nm.
    pub at: Point,
    /// `(scale s)` in millionths (1.0 is 1000000); KiCad's `SetImageScale` refuses a scale that is not a positive number.
    #[serde(default = "d_scale", skip_serializing_if = "is_unit_scale")]
    pub scale_ppm: i64,
    /// The image file, base64, no white space.
    pub data: String,
}

fn d_scale() -> i64 {
    crate::image_info::SCALE_PPM
}
fn is_unit_scale(s: &i64) -> bool {
    *s == crate::image_info::SCALE_PPM
}

impl RefImage {
    /// An image on `layer` centred at `at`, shown at its own definition (scale 1.0).
    pub fn new(layer: &str, at: Point, data: &str) -> RefImage {
        RefImage { id: String::new(), layer: layer.to_string(), at, scale_ppm: crate::image_info::SCALE_PPM, data: crate::image_info::compact(data) }
    }

    /// The seed `assign_missing_ids` hashes: the place and the start of the pixels.
    pub fn id_seed(&self) -> String {
        format!("img|{},{}|{}|{}|{}", self.at.x, self.at.y, self.layer, self.scale_ppm, &self.data[..self.data.len().min(96)])
    }

    /// What the file says of itself, `None` when the data is not an image of a kind known.
    pub fn header(&self) -> Option<crate::image_info::ImageHeader> {
        crate::image_info::header_of_base64(&self.data)
    }

    /// The size the image has on the board, nm (`BITMAP_BASE::GetSize`); zero for data that is not an image.
    pub fn size_nm(&self) -> (Nm, Nm) {
        self.header().map_or((Nm::ZERO, Nm::ZERO), |h| crate::image_info::size_nm(&h, self.scale_ppm))
    }

    /// The box the image covers, `[x0, y0, x1, y1]` (`REFERENCE_IMAGE::GetBoundingBox`: `BOX2I::ByCenter( m_pos, size )`).
    pub fn bounds(&self) -> [Nm; 4] {
        let (w, h) = self.size_nm();
        [self.at.x - w / 2, self.at.y - h / 2, self.at.x - w / 2 + w, self.at.y - h / 2 + h]
    }
}

/// `DEFAULT_PT_SIZE_MM` of `pcbnew/pcb_point.cpp`: the size of a point the Place Point tool makes.
pub const DEFAULT_POINT_SIZE_NM: Nm = Nm::new(1_000_000);

fn d_point_size() -> Nm {
    DEFAULT_POINT_SIZE_NM
}

/// A point: a mark on a layer that the grid helper snaps to (`PCB_POINT`, `(point (at x y) (size s) (layer ..) (uuid ..))`). Its only geometry is its position and the size of the mark
/// it is drawn as -- a cross whose bars reach `size / 2` from the centre, with a ring of a quarter of the size round it -- which says nothing about the board: no plot, no 3D view, no DRC.
/// The file has no lock for it, so it has none here either.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PcbPoint {
    /// Stable id (`pnt_xxxxxxxxxxxx`), made when the point is read or added.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub id: String,
    /// The layer the point is on: its ring is drawn in the layer's colour, and it is shown while the layer is.
    pub layer: String,
    pub at: Point,
    /// The size of the mark, nm.
    #[serde(default = "d_point_size")]
    #[serde(alias = "size_um", deserialize_with = "crate::units::de")]
    pub size_nm: Nm,
}

impl PcbPoint {
    /// A point of the default size on `layer`.
    pub fn new(layer: &str, at: Point) -> PcbPoint {
        PcbPoint { id: String::new(), layer: layer.to_string(), at, size_nm: DEFAULT_POINT_SIZE_NM }
    }

    /// The seed `assign_missing_ids` hashes.
    pub fn id_seed(&self) -> String {
        format!("pnt|{},{}|{}|{}", self.at.x, self.at.y, self.layer, self.size_nm)
    }

    /// `PCB_POINT::GetBoundingBox`: `BOX2I::ByCenter( m_pos, { m_size, m_size } )`, `[x0, y0, x1, y1]`.
    pub fn bounds(&self) -> [Nm; 4] {
        let (x0, y0) = (self.at.x - self.size_nm / 2, self.at.y - self.size_nm / 2);
        [x0, y0, x0 + self.size_nm, y0 + self.size_nm]
    }
}

/// How a rectangle, a circle or a polygon is hatched (`FILL_T::HATCH`, `REVERSE_HATCH`, `CROSS_HATCH`; the file's `(fill hatch)`, `(fill reverse_hatch)`, `(fill cross_hatch)`): the enum a
/// footprint's graphic has ([`crate::fp_art::Hatch`]), which the board's shapes share.
pub use crate::fp_art::Hatch as HatchKind;

/// The five fills of a shape (`UI_FILL_MODE`: None, Solid, Hatch, Reverse Hatch, Cross-hatch) as the Properties panel and the shape dialog offer them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ShapeFill {
    None,
    Solid,
    Hatch,
    ReverseHatch,
    CrossHatch,
}

impl ShapeFill {
    /// The hatch this fill is, if it is one.
    pub fn hatch(self) -> Option<HatchKind> {
        match self {
            ShapeFill::Hatch => Some(HatchKind::Hatch),
            ShapeFill::ReverseHatch => Some(HatchKind::ReverseHatch),
            ShapeFill::CrossHatch => Some(HatchKind::CrossHatch),
            ShapeFill::None | ShapeFill::Solid => None,
        }
    }
}

/// What a board shape has beyond what [`crate::ir::Shape`] holds, by the shape's id: its hatch, the line endings of a line, an arc or a curve, and the net of a shape on
/// copper. A shape without an entry is as `Shape::filled` says (solid or not filled), has no endings and is on no net. The entry is kept with the board, not with the shape,
/// because `Shape` is built field by field in a good many places and a new field there is a change at every one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ShapeStyle {
    /// The id of the shape (`shp_xxxxxxxxxxxx`).
    pub id: String,
    /// The hatch of a rectangle, a circle or a polygon that is not filled solid.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hatch: Option<HatchKind>,
    /// KiCad 10's `(start_shape ..)` and `(end_shape ..)` of an open shape (`EDA_SHAPE::m_startEnding`, `m_endEnding`) -- see [`crate::line_ending`]. Additive.
    #[serde(default, skip_serializing_if = "crate::line_ending::LineEndings::is_none")]
    pub endings: crate::line_ending::LineEndings,
    /// The net of a shape on a copper layer (`(net "name")`, `BOARD_CONNECTED_ITEM::GetNetname`): kicad-cli's DRC takes it as copper of that net. `None` is no net. Additive.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub net: Option<String>,
    /// The corner radius of a rectangle (`(gr_rect .. (radius r))`, `EDA_SHAPE::GetCornerRadius`), nm: the board outline, the plot, the hatch and the file see the
    /// rounded rectangle (`ROUNDRECT( SHAPE_RECT( .. ), GetCornerRadius() )`). 0 is a sharp one. Additive.
    #[serde(default, skip_serializing_if = "is_zero", deserialize_with = "crate::units::de")]
    pub corner_radius: crate::ir::Nm,
    /// A shape on an outer copper layer that has a solder-mask opening (`PCB_SHAPE::HasSolderMask`, the file's `(layers "F.Cu" "F.Mask")`). Additive.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub solder_mask: bool,
    /// Its own `(solder_mask_margin ..)` (`PCB_SHAPE::GetLocalSolderMaskMargin`), nm; `None` takes the board's. Additive.
    #[serde(default, skip_serializing_if = "Option::is_none", deserialize_with = "crate::units::de")]
    pub solder_mask_margin: Option<crate::ir::Nm>,
}

fn is_zero(v: &crate::ir::Nm) -> bool {
    *v == Nm::ZERO
}

impl ShapeStyle {
    /// The style of `id` that says nothing.
    pub fn empty(id: &str) -> ShapeStyle {
        ShapeStyle { id: id.to_string(), hatch: None, endings: Default::default(), net: None, corner_radius: Nm::ZERO, solder_mask: false, solder_mask_margin: None }
    }

    /// Says nothing beyond the shape itself: not kept.
    pub fn is_empty(&self) -> bool {
        self.hatch.is_none() && self.endings.is_none() && self.net.is_none() && self.corner_radius == Nm::ZERO && !self.solder_mask
    }
}

/// The items this module adds to a board. See the module doc.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BoardItems {
    /// The board's text boxes (`gr_text_box`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub text_boxes: Vec<TextBox>,
    /// The board's reference images (`image`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub images: Vec<RefImage>,
    /// The board's points (`point`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub points: Vec<PcbPoint>,
    /// The hatch, the line endings and the net of the board's shapes, by the id of the shape ([`ShapeStyle`]). Not items of their own: see [`BoardItems::ids`].
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub shape_styles: Vec<ShapeStyle>,
    /// KiCad 10's grid items (`grid_item`, `PCB_GRID_ITEM`) -- see [`crate::grid_item`]. Additive.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub grid_items: Vec<crate::grid_item::GridItem>,
    /// KiCad 10's via stitching generators (`(generated (type via_stitch) ..)`, `PCB_VIA_STITCH`) -- see [`crate::via_stitch`]. Additive.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub via_stitches: Vec<crate::via_stitch::ViaStitch>,
    /// KiCad 10's drill charts: what makes a table of the board one (`drill_chart`, `PCB_DRILL_CHART`), keyed by the table's id -- see [`crate::drill_chart`]. Not items of
    /// their own: the table is. Additive.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub drill_charts: Vec<crate::drill_chart::DrillChart>,
    /// KiCad 10's drill maps (`drill_map`, `PCB_DRILL_MAP`). Additive.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub drill_maps: Vec<crate::drill_chart::DrillMap>,
    /// The board's drill symbol profile (`(setup (drill_symbol_profile ..))`); `None` is KiCad's default one. Additive.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub drill_profile: Option<crate::drill_chart::DrillSymbolProfile>,
}

impl BoardItems {
    pub fn is_empty(&self) -> bool {
        self.text_boxes.is_empty() && self.images.is_empty() && self.points.is_empty() && self.shape_styles.is_empty() && self.grid_items.is_empty() && self.via_stitches.is_empty()
            && self.drill_charts.is_empty()
            && self.drill_maps.is_empty()
            && self.drill_profile.is_none()
    }

    /// Every id of the items, for the id sets that name them (a shape's style is the shape's, not an item).
    pub fn ids(&self) -> impl Iterator<Item = &str> {
        self.text_boxes.iter().map(|t| t.id.as_str()).chain(self.images.iter().map(|i| i.id.as_str())).chain(self.points.iter().map(|p| p.id.as_str())).chain(self.grid_items.iter().map(|g| g.id.as_str())).chain(self.via_stitches.iter().map(|v| v.id.as_str())).chain(self.drill_maps.iter().map(|m| m.id.as_str()))
    }

    /// The ids of the items the lock (`(locked yes)`) applies to: every kind but the point, whose file form has no lock.
    pub fn lockable_ids(&self) -> impl Iterator<Item = &str> {
        self.text_boxes.iter().map(|t| t.id.as_str()).chain(self.images.iter().map(|i| i.id.as_str())).chain(self.grid_items.iter().map(|g| g.id.as_str())).chain(self.via_stitches.iter().map(|v| v.id.as_str())).chain(self.drill_maps.iter().map(|m| m.id.as_str()))
    }

    /// Give each item without an id the deterministic one the others get (`next_item_id`), in an order that does not depend on how they were listed.
    pub fn assign_missing_ids(&mut self, existing: &mut std::collections::BTreeSet<String>) {
        assign_ids(&mut self.text_boxes, "tbx", existing, |t| &mut t.id, TextBox::id_seed, |t| (t.text.clone(), t.start, t.end));
        assign_ids(&mut self.images, "img", existing, |i| &mut i.id, RefImage::id_seed, |i| (i.layer.clone(), i.at, i.data.len() as i64));
        assign_ids(&mut self.points, "pnt", existing, |p| &mut p.id, PcbPoint::id_seed, |p| (p.layer.clone(), p.at, p.size_nm));
        assign_ids(&mut self.grid_items, "grd", existing, |g| &mut g.id, crate::grid_item::GridItem::id_seed, |g| (g.at, g.extent, g.spacing));
        assign_ids(&mut self.via_stitches, "vst", existing, |v| &mut v.id, crate::via_stitch::ViaStitch::id_seed, |v| (v.outline.clone(), v.net.clone()));
        assign_ids(&mut self.drill_maps, "dmap", existing, |m| &mut m.id, crate::drill_chart::DrillMap::id_seed, |m| (m.layer.clone(), m.offset));
    }

    /// Sort the collections by id (the canonical form a design is hashed in).
    pub fn canonicalize(&mut self) {
        self.text_boxes.sort_by(|a, b| a.id.cmp(&b.id));
        self.images.sort_by(|a, b| a.id.cmp(&b.id));
        self.points.sort_by(|a, b| a.id.cmp(&b.id));
        self.shape_styles.sort_by(|a, b| a.id.cmp(&b.id));
        self.grid_items.sort_by(|a, b| a.id.cmp(&b.id));
        self.via_stitches.sort_by(|a, b| a.id.cmp(&b.id));
        self.drill_charts.sort_by(|a, b| a.table_id.cmp(&b.table_id));
        self.drill_maps.sort_by(|a, b| a.id.cmp(&b.id));
    }

    /// The hatch of the shape `shape_id`, if it has one.
    pub fn hatch_of(&self, shape_id: &str) -> Option<HatchKind> {
        self.shape_styles.iter().find(|st| st.id == shape_id).and_then(|st| st.hatch)
    }

    /// Hatch the shape `shape_id` as `hatch` says, or take its hatch away (`None`); an entry that says nothing is not kept.
    pub fn set_hatch(&mut self, shape_id: &str, hatch: Option<HatchKind>) {
        self.edit_style(shape_id, |st| st.hatch = hatch);
    }

    /// The style of the shape `shape_id`, if it has one.
    pub fn style_of(&self, shape_id: &str) -> Option<&ShapeStyle> {
        self.shape_styles.iter().find(|st| st.id == shape_id)
    }

    /// The line endings of the shape `shape_id` (none when it has no style).
    pub fn endings_of(&self, shape_id: &str) -> crate::line_ending::LineEndings {
        self.style_of(shape_id).map(|st| st.endings).unwrap_or_default()
    }

    /// The corner radius of the rectangle `shape_id` (0: sharp, or not a rectangle).
    pub fn corner_radius_of(&self, shape_id: &str) -> crate::ir::Nm {
        self.style_of(shape_id).map(|st| st.corner_radius).unwrap_or(Nm::ZERO)
    }

    /// Round the corners of the rectangle `shape_id` by `radius` (0: sharp).
    pub fn set_corner_radius(&mut self, shape_id: &str, radius: crate::ir::Nm) {
        self.edit_style(shape_id, |st| st.corner_radius = radius.max(Nm::ZERO));
    }

    /// The net of the shape `shape_id`, if it is on one.
    pub fn net_of(&self, shape_id: &str) -> Option<&str> {
        self.style_of(shape_id).and_then(|st| st.net.as_deref())
    }

    /// Give the shape `shape_id` the endings `endings` (none takes them away).
    pub fn set_endings(&mut self, shape_id: &str, endings: crate::line_ending::LineEndings) {
        self.edit_style(shape_id, |st| st.endings = endings);
    }

    /// Put the shape `shape_id` on the net `net` (`None`: on no net).
    pub fn set_net(&mut self, shape_id: &str, net: Option<String>) {
        self.edit_style(shape_id, |st| st.net = net);
    }

    /// Change the style of `shape_id` by `edit`, making the entry when it has none and dropping one that ends up saying nothing.
    fn edit_style(&mut self, shape_id: &str, edit: impl FnOnce(&mut ShapeStyle)) {
        let i = match self.shape_styles.iter().position(|st| st.id == shape_id) {
            Some(i) => i,
            None => {
                self.shape_styles.push(ShapeStyle::empty(shape_id));
                self.shape_styles.len() - 1
            }
        };
        edit(&mut self.shape_styles[i]);
        if self.shape_styles[i].is_empty() {
            self.shape_styles.remove(i);
        }
    }

    /// Give the shape `shape_id` the style `style` (a copy's, from another shape): its hatch, endings and net, under this shape's id.
    pub fn set_style(&mut self, shape_id: &str, style: Option<&ShapeStyle>) {
        self.forget_shape(shape_id);
        if let Some(st) = style.filter(|st| !st.is_empty()) {
            self.shape_styles.push(ShapeStyle { id: shape_id.to_string(), ..st.clone() });
        }
    }

    /// Forget everything the board kept of the shape `shape_id` beyond `Shape` (its hatch, endings and net): the shape is gone.
    pub fn forget_shape(&mut self, shape_id: &str) {
        self.shape_styles.retain(|st| st.id != shape_id);
    }

    /// Forget the styles of shapes that are gone (`live` says which ids there are), so a shape made later under the same id does not take a hatch that was not its own.
    pub fn prune_shape_styles(&mut self, live: &std::collections::BTreeSet<String>) {
        self.shape_styles.retain(|st| live.contains(&st.id));
    }

    /// Put each item on the layer `map` names for its own, when it names one (Swap Layers, `PCB_EDIT_FRAME::SwapLayers`).
    pub fn swap_layers(&mut self, map: &dyn Fn(&str) -> Option<String>) {
        for t in &mut self.text_boxes {
            if let Some(dest) = map(&t.layer) {
                t.layer = dest;
            }
        }
        for i in &mut self.images {
            if let Some(dest) = map(&i.layer) {
                i.layer = dest;
            }
        }
        for p in &mut self.points {
            if let Some(dest) = map(&p.layer) {
                p.layer = dest;
            }
        }
    }
}

/// Give the items of one kind without an id the deterministic one, taking them in the order `key` gives (not the order they were listed in).
fn assign_ids<T, K: Ord>(items: &mut [T], prefix: &str, existing: &mut std::collections::BTreeSet<String>, id: impl Fn(&mut T) -> &mut String, seed: impl Fn(&T) -> String, key: impl Fn(&T) -> K) {
    let mut order: Vec<usize> = (0..items.len()).collect();
    order.sort_by_key(|&i| key(&items[i]));
    for i in order {
        if id(&mut items[i]).is_empty() {
            let new = crate::ir::next_item_id(prefix, &seed(&items[i]), existing);
            existing.insert(new.clone());
            *id(&mut items[i]) = new;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_new_text_box_has_the_margin_of_its_layer_defaults() {
        // 0.12 mm border, 1 mm text: 60 + 750.
        let b = TextBox::new("F.SilkS", Point { x: Nm::ZERO, y: Nm::ZERO }, Point { x: Nm::new(10_000_000), y: Nm::new(5_000_000) }, Nm::new(1_000_000), Nm::new(120_000));
        assert_eq!((b.margin_left_nm, b.margin_top_nm, b.margin_right_nm, b.margin_bottom_nm), (Nm::new(810_000), Nm::new(810_000), Nm::new(810_000), Nm::new(810_000)));
        assert_eq!((b.halign, b.valign, b.mirror, b.border), (TextJustify::Left, TextVAlign::Top, false, true));
        assert!(TextBox::new("B.SilkS", Point::default(), Point::default(), Nm::new(1_000_000), Nm::new(120_000)).mirror, "text on a back layer is mirrored");
    }

    #[test]
    fn the_pen_is_the_files_or_an_eighth_of_the_smaller_glyph_side() {
        let mut b = TextBox::new("F.SilkS", Point::default(), Point::default(), Nm::new(1_000_000), Nm::new(120_000));
        assert_eq!(b.effective_pen_nm(), Nm::new(125_000));
        b.bold = true;
        assert_eq!(b.effective_pen_nm(), Nm::new(200_000));
        b.width_nm = Some(Nm::new(800_000));
        assert_eq!(b.effective_pen_nm(), Nm::new(160_000));
        b.thickness_nm = Nm::new(150_000);
        assert_eq!(b.effective_pen_nm(), Nm::new(150_000));
    }

    #[test]
    fn a_reference_image_covers_its_pixels_at_their_definition_times_the_scale_around_its_centre() {
        let png = crate::image_info::base64_encode(&crate::image_info::solid_png(60, 40, [9, 9, 9], 254));
        let mut img = RefImage::new("Cmts.User", Point { x: Nm::new(10_000_000), y: Nm::new(20_000_000) }, &png);
        // 60 x 40 px at 254 ppi: 6 mm x 4 mm
        assert_eq!(img.size_nm(), (Nm::new(6_000_000), Nm::new(4_000_000)));
        assert_eq!(img.bounds(), [Nm::new(7_000_000), Nm::new(18_000_000), Nm::new(13_000_000), Nm::new(22_000_000)]);
        img.scale_ppm = 2_500_000;
        assert_eq!(img.bounds(), [Nm::new(2_500_000), Nm::new(15_000_000), Nm::new(17_500_000), Nm::new(25_000_000)]);
        // data that is no image has no size
        img.data = "AAAA".into();
        assert_eq!(img.size_nm(), (Nm::ZERO, Nm::ZERO));
        assert!(RefImage::new("F.Cu", Point::default(), "AB\nCD ").data == "ABCD", "white space is taken out of the data");
    }

    #[test]
    fn images_get_ids_of_their_own_kind_and_the_scale_one_is_not_written() {
        let png = crate::image_info::base64_encode(&crate::image_info::solid_png(20, 20, [1, 1, 1], 300));
        let mut items = BoardItems::default();
        items.images.push(RefImage::new("Cmts.User", Point { x: Nm::new(1), y: Nm::new(2) }, &png));
        items.images.push(RefImage::new("Cmts.User", Point { x: Nm::new(1), y: Nm::new(2) }, &png));
        assert!(!items.is_empty());
        let mut existing = std::collections::BTreeSet::new();
        items.assign_missing_ids(&mut existing);
        assert!(items.images.iter().all(|i| i.id.starts_with("img_")));
        assert_ne!(items.images[0].id, items.images[1].id);
        assert_eq!(items.ids().count(), 2);
        let json = serde_json::to_value(&items).unwrap();
        assert!(json["images"][0].get("scale_ppm").is_none(), "{json}");
        items.images[0].scale_ppm = 500_000;
        let back: BoardItems = serde_json::from_str(&serde_json::to_string(&items).unwrap()).unwrap();
        assert_eq!(back, items);
        let swapped = {
            let mut s = items.clone();
            s.swap_layers(&|l| (l == "Cmts.User").then(|| "Dwgs.User".to_string()));
            s
        };
        assert!(swapped.images.iter().all(|i| i.layer == "Dwgs.User"));
    }

    #[test]
    fn a_point_is_a_mark_of_a_size_on_a_layer_with_no_lock_of_its_own() {
        let p = PcbPoint::new("F.SilkS", Point { x: Nm::new(10_000_000), y: Nm::new(20_000_000) });
        assert_eq!((p.size_nm, p.bounds()), (Nm::new(1_000_000), [Nm::new(9_500_000), Nm::new(19_500_000), Nm::new(10_500_000), Nm::new(20_500_000)]), "the default is 1 mm and the box is centred on the point");
        let mut items = BoardItems::default();
        items.points.push(p.clone());
        items.points.push(p);
        items.images.push(RefImage::new("Cmts.User", Point::default(), &crate::image_info::base64_encode(&crate::image_info::solid_png(20, 20, [0, 0, 0], 300))));
        let mut existing = std::collections::BTreeSet::new();
        items.assign_missing_ids(&mut existing);
        assert!(items.points.iter().all(|p| p.id.starts_with("pnt_")) && items.points[0].id != items.points[1].id);
        assert_eq!(items.ids().count(), 3);
        assert_eq!(items.lockable_ids().count(), 1, "a point is not lockable: only the image is");
        let json = serde_json::to_string(&items).unwrap();
        let back: BoardItems = serde_json::from_str(&json).unwrap();
        assert_eq!(back, items);
        // a size left out is the default
        let p: PcbPoint = serde_json::from_str(r#"{"layer": "F.Cu", "at": {"x": 1, "y": 2}}"#).unwrap();
        assert_eq!(p.size_nm, Nm::new(1_000_000));
    }

    #[test]
    fn items_serialize_to_nothing_when_there_are_none_and_ids_are_deterministic() {
        let mut items = BoardItems::default();
        assert_eq!(serde_json::to_string(&items).unwrap(), "{}");
        let mut t = TextBox::new("F.SilkS", Point { x: Nm::new(1000), y: Nm::new(2000) }, Point { x: Nm::new(3000), y: Nm::new(4000) }, Nm::new(1_000_000), Nm::new(120_000));
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
