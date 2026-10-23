//! The verbs of the board items `eda_model::pcb_items` adds (GAPS.md item 24, package P16): text boxes, reference images, points and the hatch fill of shapes.
//!
//! They are one variant of [`crate::Cmd`], `{"op": "pcb_item", "verb": "add_text_box", ...}`, the way the schematic's `sch_edit` and `sch_move` are, so the hub file holds
//! one variant and one arm per match. Move, Rotate, Flip, Delete-by-group, Lock and Duplicate go through the verbs that take any item (`move_items`, `rotate_items`,
//! `flip_items`, `set_locked`, `duplicate`): [`transform_board_items`] is what those call for the ids of these kinds.
//!
//! Ported from `PCB_TEXTBOX::Rotate` / `::Flip` / `::Move` (`pcbnew/pcb_textbox.cpp`), `EDA_SHAPE::rotate` / `::flip` (`common/eda_shape.cpp`),
//! `DIALOG_TEXTBOX_PROPERTIES::TransferDataFromWindow` (`pcbnew/dialogs/dialog_textbox_properties.cpp`).

use super::pcb_transform::{flip_layer, side_specific, FlipDirection, Xform};
use super::Board;
use eda_model::barcode::{Barcode, BarcodeKind, EccLevel};
use eda_model::ir::{DrawingsSection, Millideg, Point, Shape, Um};
use eda_model::grid_item::GridItem;
use eda_model::via_stitch::ViaStitch;
use eda_model::line_ending::LineEnding;
use eda_model::pcb_items::{PcbPoint, RefImage, ShapeFill, TextBox};
use eda_model::CheckResult;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

/// `TEXT_MIN_SIZE_MM` and `TEXT_MAX_SIZE_MM` (`include/eda_text.h`): the glyph sizes the Text Box Properties dialog accepts.
const TEXT_MIN_SIZE_UM: Um = 1;
const TEXT_MAX_SIZE_UM: Um = 250_000;

fn fail(check: &str, what: &str, msg: impl Into<String>) -> Vec<CheckResult> {
    vec![CheckResult::fail(check, what, msg.into())]
}

/// One edit of the board's text boxes, images, points or shape fills. On the wire `{"op": "pcb_item", "verb": "<verb>", ...}`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "verb", rename_all = "snake_case")]
pub enum PcbItemCmd {
    /// `pcbnew.InteractiveDrawing.textbox` (`DRAWING_TOOL::DrawRectangle` with `PCB_ACTIONS::drawTextBox`) after `DIALOG_TEXTBOX_PROPERTIES`'s OK: the box on the board. Its `id`, if
    /// the caller sent one, is ignored: ids are assigned here.
    AddTextBox { text_box: TextBox },
    /// `DIALOG_TEXTBOX_PROPERTIES::TransferDataFromWindow` and the Properties panel: the box with every field replaced, in place -- its id, its place in the drawing order,
    /// its lock and its groups stay.
    EditTextBox { id: String, text_box: TextBox },
    /// Remove a text box (`EDIT_TOOL::DeleteItems`).
    DeleteTextBox { id: String },
    /// `pcbnew.InteractiveDrawing.placeReferenceImage` (`DRAWING_TOOL::PlaceReferenceImage`): the image, its centre and the layer it is on. `image.id` is ignored; `image.data` is the
    /// PNG, JPEG, GIF or BMP file in base64.
    AddRefImage { image: RefImage },
    /// `DIALOG_REFERENCE_IMAGE_PROPERTIES::TransferDataFromWindow`: the image's centre, layer and scale; its pixels, id, lock and groups stay.
    EditRefImage { id: String, layer: String, at: Point, scale_ppm: i64 },
    /// The pixels of an image replaced (`BITMAP_BASE::ConvertToGreyscale` and what turns them): the centre, layer and scale stay.
    SetRefImageData { id: String, data: String },
    /// Remove a reference image.
    DeleteRefImage { id: String },
    /// `pcbnew.InteractiveDrawing.placePoint` (`DRAWING_TOOL::PlacePoint`): a point on the board. `point.id` is ignored.
    AddPoint { point: PcbPoint },
    /// Point Properties / the Properties panel: the point's layer, place and size, in place.
    EditPoint { id: String, layer: String, at: Point, size_um: Um },
    /// Remove a point.
    DeletePoint { id: String },
    /// `pcbnew.InteractiveDrawing.barcode` (`DRAWING_TOOL::DrawBarcode`) after `DIALOG_BARCODE_PROPERTIES`'s OK: the barcode on the board. `barcode.id` is ignored; a Micro QR code takes
    /// level Q for High (`PCB_BARCODE::SetErrorCorrection`).
    AddBarcode { barcode: Barcode },
    /// `DIALOG_BARCODE_PROPERTIES::TransferDataFromWindow` and the Properties panel: the barcode with every field replaced, in place -- its id, its place in the drawing order, its lock and its
    /// groups stay.
    EditBarcode { id: String, barcode: Barcode },
    /// Remove a barcode (`EDIT_TOOL::DeleteItems`).
    DeleteBarcode { id: String },
    /// The fill of rectangles, circles and polygons (`EDA_SHAPE::SetFillModeProp`: the Fill row of the Properties panel and the shape dialog's choice): none, solid, hatch, reverse hatch or
    /// cross-hatch, for all of `ids` as one undo step. The hatch lines are worked out when the board is described (`eda_drc::hatch`), as KiCad's `rehatchShapes` does after every commit,
    /// so there is nothing for a verb to refresh. Only fills: a fill of `null` changes nothing.
    ///
    /// And the line endings of lines, arcs and curves (`EDA_SHAPE::SetStartEnding` / `SetEndEnding`: the shape dialog's Start and End rows, and the Properties panel's Start Shape,
    /// Start Length, Start Width, Start Stroke Width and their End rows), KiCad 10's `(start_shape ..)` and `(end_shape ..)`: an ending left out changes nothing, one of style
    /// `none` takes it away. A closed shape has no endings (`isOpenShape`).
    SetShapeStyle {
        ids: Vec<String>,
        #[serde(default)]
        fill: Option<ShapeFill>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        start_ending: Option<LineEnding>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        end_ending: Option<LineEnding>,
    },
    /// `pcbnew.GridItem.place` (`DRAWING_TOOL::PlaceSubGrid`, "Place Sub-grid"): KiCad 10's grid item, centred on the first click and as big and as turned as the second makes it.
    /// `grid_item.id` is ignored. See [`crate::kicad10_items`].
    AddGridItem { grid_item: GridItem },
    /// `DIALOG_GRID_ITEM_PROPERTIES::TransferDataFromWindow` and the Properties panel ("Edit Grid Item Properties"): the grid with every field replaced, in place.
    EditGridItem { id: String, grid_item: GridItem },
    /// Remove a grid item.
    DeleteGridItem { id: String },
    /// KiCad 10's via stitching (`PCB_VIA_STITCH`): the generator with every setting replaced -- or a new one, from Draw Via Stitch Area, when `id` is none -- and its vias
    /// made again (`EditStart`, `Update`, `EditFinish`: "Edit Via Stitching", "Exclude Via From Stitching", "Clear Via Stitching Exclusions"). `vias` is where they go
    /// (`eda_drc::via_stitch::plan`), which the board directory works out from the zone fills when the command leaves it out. See [`crate::kicad10_items`].
    SetViaStitch {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        id: Option<String>,
        stitch: ViaStitch,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        vias: Option<Vec<PlannedVia>>,
    },
    /// `PCB_VIA_STITCH::Remove`: the generator and every via it made.
    DeleteViaStitch { id: String },
    /// `PCB_CONTROL::PlaceDrillChart` ("Place Drill Chart"): KiCad 10's drill chart of the default template on `layer` (a documentation layer), built from the board, its first
    /// corner at `at`. See [`crate::drill_chart`].
    PlaceDrillChart { layer: String, at: Point },
    /// The Properties panel's Units, Decimal Places and Show Totals of a drill chart (and its filter): the chart rebuilt from the board. `id` is the chart's table's.
    SetDrillChart {
        id: String,
        units: eda_model::drill_chart::TableUnits,
        precision: i32,
        show_totals: bool,
        #[serde(default)]
        filter: eda_model::drill_chart::DrillChartFilter,
    },
    /// `PCB_CONTROL::PlaceDrillMap` ("Place Drill Map"): a drill map on `layer`, no offset, the profile's mark size.
    PlaceDrillMap { layer: String },
    /// The Properties panel's edits of a drill map (Offset X / Y, Symbol Size, Hole Span, Outline Slots, Guide Cross): every field replaced, in place.
    EditDrillMap { id: String, map: eda_model::drill_chart::DrillMap },
    /// Remove a drill map.
    DeleteDrillMap { id: String },
}

/// Where a via stitching puts one via: its grid cell (or its position, where the layout has no grid: `cellForPosition`) and its position.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlannedVia {
    pub cell: (i64, i64),
    pub at: Point,
}

impl PcbItemCmd {
    /// The ids the command names, for the journal and the activity feed.
    pub fn ids(&self) -> Vec<&str> {
        match self {
            PcbItemCmd::AddTextBox { text_box } => vec![text_box.text.as_str()],
            PcbItemCmd::AddRefImage { image } => vec![image.layer.as_str()],
            PcbItemCmd::AddPoint { point } => vec![point.layer.as_str()],
            PcbItemCmd::SetShapeStyle { ids, .. } => ids.iter().map(String::as_str).collect(),
            PcbItemCmd::AddBarcode { barcode } => vec![barcode.layer.as_str()],
            PcbItemCmd::AddGridItem { .. } => vec!["grid_item"],
            PcbItemCmd::EditGridItem { id, .. } | PcbItemCmd::DeleteGridItem { id } => vec![id.as_str()],
            PcbItemCmd::SetViaStitch { id, stitch, .. } => vec![id.as_deref().unwrap_or(stitch.net.as_str())],
            PcbItemCmd::DeleteViaStitch { id } => vec![id.as_str()],
            PcbItemCmd::PlaceDrillChart { layer, .. } | PcbItemCmd::PlaceDrillMap { layer } => vec![layer.as_str()],
            PcbItemCmd::SetDrillChart { id, .. } | PcbItemCmd::EditDrillMap { id, .. } | PcbItemCmd::DeleteDrillMap { id } => vec![id.as_str()],
            PcbItemCmd::EditBarcode { id, .. } | PcbItemCmd::DeleteBarcode { id } => vec![id.as_str()],
            PcbItemCmd::EditTextBox { id, .. }
            | PcbItemCmd::DeleteTextBox { id }
            | PcbItemCmd::EditRefImage { id, .. }
            | PcbItemCmd::SetRefImageData { id, .. }
            | PcbItemCmd::DeleteRefImage { id }
            | PcbItemCmd::EditPoint { id, .. }
            | PcbItemCmd::DeletePoint { id } => vec![id.as_str()],
        }
    }

    /// The one-line description the CLI journal and the activity feed show.
    pub fn describe(&self) -> String {
        let mm = |v: Um| format!("{:.3}", v as f64 / 1000.0);
        match self {
            PcbItemCmd::AddTextBox { text_box: t } => format!("text-box add --layer {} --from {},{} --to {},{} --text {:?}", t.layer, mm(t.start.x), mm(t.start.y), mm(t.end.x), mm(t.end.y), t.text),
            PcbItemCmd::EditTextBox { id, text_box: t } => format!("text-box edit {id} --layer {} --text {:?}", t.layer, t.text),
            PcbItemCmd::DeleteTextBox { id } => format!("text-box delete {id}"),
            PcbItemCmd::AddRefImage { image: i } => format!("ref-image add --layer {} --at {},{} --scale {} ({} bytes of data)", i.layer, mm(i.at.x), mm(i.at.y), i.scale_ppm as f64 / 1e6, i.data.len() / 4 * 3),
            PcbItemCmd::EditRefImage { id, layer, at, scale_ppm } => format!("ref-image edit {id} --layer {layer} --at {},{} --scale {}", mm(at.x), mm(at.y), *scale_ppm as f64 / 1e6),
            PcbItemCmd::SetRefImageData { id, data } => format!("ref-image pixels {id} ({} bytes of data)", data.len() / 4 * 3),
            PcbItemCmd::DeleteRefImage { id } => format!("ref-image delete {id}"),
            PcbItemCmd::AddPoint { point: p } => format!("point add --layer {} --at {},{} --size {}", p.layer, mm(p.at.x), mm(p.at.y), mm(p.size_um)),
            PcbItemCmd::EditPoint { id, layer, at, size_um } => format!("point edit {id} --layer {layer} --at {},{} --size {}", mm(at.x), mm(at.y), mm(*size_um)),
            PcbItemCmd::DeletePoint { id } => format!("point delete {id}"),
            PcbItemCmd::AddBarcode { barcode: b } => format!("barcode add --layer {} --at {},{} --size {}x{} --kind {} --text {:?}", b.layer, mm(b.at.x), mm(b.at.y), mm(b.width_um), mm(b.height_um), b.kind.file_name(), b.text),
            PcbItemCmd::EditBarcode { id, barcode: b } => format!("barcode edit {id} --layer {} --at {},{} --size {}x{} --kind {} --text {:?}", b.layer, mm(b.at.x), mm(b.at.y), mm(b.width_um), mm(b.height_um), b.kind.file_name(), b.text),
            PcbItemCmd::DeleteBarcode { id } => format!("barcode delete {id}"),
            PcbItemCmd::AddGridItem { grid_item: g } => format!("grid-item add --{} --at {},{} --extent {}x{} --spacing {}x{}", g.kind.token(), mm(g.at.x), mm(g.at.y), mm(g.extent.x), mm(g.extent.y), mm(g.spacing.x), mm(g.spacing.y)),
            PcbItemCmd::EditGridItem { id, grid_item: g } => format!("grid-item edit {id} --{} --at {},{} --extent {}x{} --spacing {}x{} --angle {}", g.kind.token(), mm(g.at.x), mm(g.at.y), mm(g.extent.x), mm(g.extent.y), mm(g.spacing.x), mm(g.spacing.y), g.angle),
            PcbItemCmd::DeleteGridItem { id } => format!("grid-item delete {id}"),
            PcbItemCmd::SetViaStitch { id, stitch: v, vias } => format!(
                "via-stitch {} --net {} --pitch {} --layout {:?} --mode {:?} --via {}x{} ({} vias)",
                id.as_deref().unwrap_or("add"),
                v.net,
                mm(v.pitch_um),
                v.layout,
                v.mode,
                mm(v.via.size_um),
                mm(v.via.drill_um),
                vias.as_ref().map_or("planned".to_string(), |p| p.len().to_string())
            )
            .to_lowercase(),
            PcbItemCmd::DeleteViaStitch { id } => format!("via-stitch delete {id}"),
            PcbItemCmd::PlaceDrillChart { layer, at } => format!("drill-chart place --layer {layer} --at {},{}", mm(at.x), mm(at.y)),
            PcbItemCmd::SetDrillChart { id, units, precision, show_totals, .. } => format!("drill-chart edit {id} --units {} --precision {precision} --totals {show_totals}", units.token()),
            PcbItemCmd::PlaceDrillMap { layer } => format!("drill-map place --layer {layer}"),
            PcbItemCmd::EditDrillMap { id, map: m } => format!("drill-map edit {id} --layer {} --offset {},{} --size {}", m.layer, mm(m.offset.x), mm(m.offset.y), mm(m.size_um)),
            PcbItemCmd::DeleteDrillMap { id } => format!("drill-map delete {id}"),
            PcbItemCmd::SetShapeStyle { ids, fill, start_ending, end_ending } => {
                let mut out = format!("shape style {} --fill {}", ids.join(","), fill.map_or("unchanged".to_string(), |f| format!("{f:?}").to_lowercase()));
                for (flag, e) in [("start", start_ending), ("end", end_ending)] {
                    if let Some(e) = e {
                        out.push_str(&format!(" --{flag}-shape {} --{flag}-size {}x{} --{flag}-stroke {}", e.style.token(), mm(e.length_um), mm(e.width_um), mm(e.stroke_width_um)));
                    }
                }
                out
            }
        }
    }

    /// The kind of step it is, for the journal's summary line.
    pub fn kind(&self) -> &'static str {
        match self {
            PcbItemCmd::AddTextBox { .. } | PcbItemCmd::EditTextBox { .. } | PcbItemCmd::DeleteTextBox { .. } => "text-box",
            PcbItemCmd::AddRefImage { .. } | PcbItemCmd::EditRefImage { .. } | PcbItemCmd::SetRefImageData { .. } | PcbItemCmd::DeleteRefImage { .. } => "ref-image",
            PcbItemCmd::AddPoint { .. } | PcbItemCmd::EditPoint { .. } | PcbItemCmd::DeletePoint { .. } => "point",
            PcbItemCmd::SetShapeStyle { .. } => "shape-style",
            PcbItemCmd::AddBarcode { .. } | PcbItemCmd::EditBarcode { .. } | PcbItemCmd::DeleteBarcode { .. } => "barcode",
            PcbItemCmd::AddGridItem { .. } | PcbItemCmd::EditGridItem { .. } | PcbItemCmd::DeleteGridItem { .. } => "grid-item",
            PcbItemCmd::SetViaStitch { .. } | PcbItemCmd::DeleteViaStitch { .. } => "via-stitch",
            PcbItemCmd::PlaceDrillChart { .. } | PcbItemCmd::SetDrillChart { .. } => "drill-chart",
            PcbItemCmd::PlaceDrillMap { .. } | PcbItemCmd::EditDrillMap { .. } | PcbItemCmd::DeleteDrillMap { .. } => "drill-map",
        }
    }
}

impl<'a> Board<'a> {
    /// Apply one [`PcbItemCmd`].
    pub(crate) fn apply_pcb_item(&mut self, cmd: &PcbItemCmd) -> Result<(), Vec<CheckResult>> {
        match cmd {
            PcbItemCmd::AddTextBox { text_box } => self.add_text_box(text_box.clone()),
            PcbItemCmd::EditTextBox { id, text_box } => self.edit_text_box(id, text_box.clone()),
            PcbItemCmd::DeleteTextBox { id } => self.delete_text_box(id),
            PcbItemCmd::AddRefImage { image } => self.add_ref_image(image.clone()),
            PcbItemCmd::EditRefImage { id, layer, at, scale_ppm } => self.edit_ref_image(id, layer, *at, *scale_ppm),
            PcbItemCmd::SetRefImageData { id, data } => self.set_ref_image_data(id, data),
            PcbItemCmd::DeleteRefImage { id } => self.delete_ref_image(id),
            PcbItemCmd::AddPoint { point } => self.add_point(point.clone()),
            PcbItemCmd::EditPoint { id, layer, at, size_um } => self.edit_point(id, layer, *at, *size_um),
            PcbItemCmd::DeletePoint { id } => self.delete_point(id),
            PcbItemCmd::SetShapeStyle { ids, fill, start_ending, end_ending } => self.set_shape_style(ids, *fill, *start_ending, *end_ending),
            PcbItemCmd::AddBarcode { barcode } => self.add_barcode(barcode.clone()),
            PcbItemCmd::EditBarcode { id, barcode } => self.edit_barcode(id, barcode.clone()),
            PcbItemCmd::DeleteBarcode { id } => self.delete_barcode(id),
            PcbItemCmd::AddGridItem { grid_item } => self.add_grid_item(grid_item.clone()),
            PcbItemCmd::EditGridItem { id, grid_item } => self.edit_grid_item(id, grid_item.clone()),
            PcbItemCmd::DeleteGridItem { id } => self.delete_grid_item(id),
            PcbItemCmd::SetViaStitch { id, stitch, vias } => self.set_via_stitch(id.as_deref(), stitch.clone(), vias.as_deref()),
            PcbItemCmd::DeleteViaStitch { id } => self.delete_via_stitch(id),
            PcbItemCmd::PlaceDrillChart { layer, at } => self.place_drill_chart(layer, *at),
            PcbItemCmd::SetDrillChart { id, units, precision, show_totals, filter } => self.set_drill_chart(id, *units, *precision, *show_totals, *filter),
            PcbItemCmd::PlaceDrillMap { layer } => self.place_drill_map(layer),
            PcbItemCmd::EditDrillMap { id, map } => self.edit_drill_map(id, map.clone()),
            PcbItemCmd::DeleteDrillMap { id } => self.delete_drill_map(id),
        }
    }

    /// What a barcode must be to be drawn: a layer the board has on, a symbol and a text of a size, an angle under a turn and margins that are not negative (`DIALOG_BARCODE_PROPERTIES`
    /// takes any size above zero; `PCB_BARCODE::SetRect` keeps one at 0.01 mm at least).
    fn check_barcode(&self, what: &str, b: &Barcode) -> Result<(), Vec<CheckResult>> {
        if b.layer.is_empty() {
            return Err(fail("ops_bad_barcode", what, "a barcode needs a layer"));
        }
        self.layer_is_on(&b.layer, what)?;
        if b.width_um <= 0 || b.height_um <= 0 {
            return Err(fail("ops_bad_barcode", what, "the size of a barcode must be greater than zero"));
        }
        if b.text_height_um <= 0 {
            return Err(fail("ops_bad_barcode", what, "the text size of a barcode must be greater than zero"));
        }
        if b.angle >= 360_000 {
            return Err(fail("ops_bad_barcode", what, "the angle of a barcode is below 360 degrees"));
        }
        if b.margin_x_um < 0 || b.margin_y_um < 0 {
            return Err(fail("ops_bad_barcode", what, "the margins of a barcode cannot be negative"));
        }
        Ok(())
    }

    fn add_barcode(&mut self, mut b: Barcode) -> Result<(), Vec<CheckResult>> {
        self.check_barcode("barcode", &b)?;
        b.id = String::new();
        normalize_barcode(&mut b);
        let dr = self.drawings_mut();
        dr.barcodes.push(b);
        dr.assign_missing_ids();
        Ok(())
    }

    fn edit_barcode(&mut self, id: &str, mut b: Barcode) -> Result<(), Vec<CheckResult>> {
        self.check_barcode(id, &b)?;
        let dr = self.design.drawings.as_mut().ok_or_else(|| fail("ops_unknown_barcode", id, "the board has no drawings yet"))?;
        let slot = dr.barcodes.iter_mut().find(|x| x.id == id).ok_or_else(|| fail("ops_unknown_barcode", id, "no barcode with this id"))?;
        b.id = id.to_string();
        normalize_barcode(&mut b);
        *slot = b;
        Ok(())
    }

    fn delete_barcode(&mut self, id: &str) -> Result<(), Vec<CheckResult>> {
        let dr = self.design.drawings.as_mut().ok_or_else(|| fail("ops_unknown_barcode", id, "the board has no drawings yet"))?;
        let before = dr.barcodes.len();
        dr.barcodes.retain(|b| b.id != id);
        if dr.barcodes.len() == before {
            return Err(fail("ops_unknown_barcode", id, "no barcode with this id"));
        }
        Ok(())
    }

    /// `SetShapeStyle`: a rectangle, a circle or a polygon (the kinds `fillAvailable` lets have a fill) is solid (`Shape::filled`), hatched (the board's `shape_styles`) or neither. Every shape
    /// is checked before any is changed.
    fn set_shape_style(&mut self, ids: &[String], fill: Option<ShapeFill>, start: Option<LineEnding>, end: Option<LineEnding>) -> Result<(), Vec<CheckResult>> {
        if ids.is_empty() {
            return Err(fail("ops_bad_shape_style", "ids", "name at least one shape"));
        }
        if fill.is_none() && start.is_none() && end.is_none() {
            return Ok(());
        }
        let dr = self.design.drawings.as_mut().ok_or_else(|| fail("ops_unknown_shape", &ids[0], "the board has no drawings yet"))?;
        for id in ids {
            let shape = dr.shapes.iter().find(|s| s.id() == id).ok_or_else(|| fail("ops_unknown_shape", id, "no shape with this id"))?;
            let closed = matches!(shape, Shape::Rect { .. } | Shape::Circle { .. } | Shape::Polygon { .. });
            if fill.is_some() && !closed {
                return Err(fail("ops_bad_shape_style", id, "only a rectangle, a circle or a polygon has a fill"));
            }
            if (start.is_some() || end.is_some()) && closed {
                return Err(fail("ops_bad_shape_style", id, "only a line, an arc or a curve has line endings"));
            }
        }
        for id in ids {
            if let Some(fill) = fill {
                if let Some(shape) = dr.shapes.iter_mut().find(|s| s.id() == id) {
                    shape.set_filled(fill == ShapeFill::Solid);
                }
                dr.board_items.set_hatch(id, fill.hatch());
            }
            if start.is_some() || end.is_some() {
                // `DIALOG_SHAPE_PROPERTIES::TransferDataFromWindow`: a size below 0 is 0 (`std::max( 0, .. )`).
                // The outline's line type is the file's and no control sets it (`SetStrokeWidth` keeps it): a command that names none keeps the one there is.
                let mut endings = dr.board_items.endings_of(id);
                let keep_type = |new: LineEnding, old: LineEnding| if new.stroke_style.is_default() { LineEnding { stroke_style: old.stroke_style, ..new } } else { new };
                if let Some(e) = start {
                    endings.start = keep_type(e.clamped(), endings.start);
                }
                if let Some(e) = end {
                    endings.end = keep_type(e.clamped(), endings.end);
                }
                dr.board_items.set_endings(id, endings);
            }
        }
        Ok(())
    }

    /// `DIALOG_TEXTBOX_PROPERTIES`'s checks, and what a box must be to be drawn: a layer the board has on, a glyph size KiCad accepts, a pen that fits the glyphs, margins
    /// and a border width that are not negative, and a rectangle or a polygon of four corners.
    fn check_text_box(&self, what: &str, t: &TextBox) -> Result<(), Vec<CheckResult>> {
        if t.layer.is_empty() {
            return Err(fail("ops_bad_text_box", what, "a text box needs a layer"));
        }
        self.layer_is_on(&t.layer, what)?;
        let width = t.text_width_um();
        if !(TEXT_MIN_SIZE_UM..=TEXT_MAX_SIZE_UM).contains(&t.size_um) || !(TEXT_MIN_SIZE_UM..=TEXT_MAX_SIZE_UM).contains(&width) {
            return Err(fail("ops_bad_text_box", what, "the text size must be between 0.001 mm and 250 mm"));
        }
        if t.thickness_um < 0 || t.stroke_width_um < 0 || t.margin_left_um < 0 || t.margin_top_um < 0 || t.margin_right_um < 0 || t.margin_bottom_um < 0 {
            return Err(fail("ops_bad_text_box", what, "the pen, the border and the margins cannot be negative"));
        }
        if !t.pts.is_empty() && t.pts.len() != 4 {
            return Err(fail("ops_bad_text_box", what, "a box turned by an angle that is not a multiple of 90 degrees has four corners"));
        }
        if t.angle >= 360_000 {
            return Err(fail("ops_bad_text_box", what, "the text angle is below 360 degrees"));
        }
        Ok(())
    }

    fn add_text_box(&mut self, mut t: TextBox) -> Result<(), Vec<CheckResult>> {
        self.check_text_box("text_box", &t)?;
        t.id = String::new();
        let dr = self.drawings_mut();
        dr.board_items.text_boxes.push(t);
        dr.assign_missing_ids();
        Ok(())
    }

    fn edit_text_box(&mut self, id: &str, mut t: TextBox) -> Result<(), Vec<CheckResult>> {
        self.check_text_box(id, &t)?;
        let dr = self.design.drawings.as_mut().ok_or_else(|| fail("ops_unknown_text_box", id, "the board has no drawings yet"))?;
        let slot = dr.board_items.text_boxes.iter_mut().find(|b| b.id == id).ok_or_else(|| fail("ops_unknown_text_box", id, "no text box with this id"))?;
        t.id = id.to_string();
        *slot = t;
        Ok(())
    }

    fn delete_text_box(&mut self, id: &str) -> Result<(), Vec<CheckResult>> {
        let dr = self.design.drawings.as_mut().ok_or_else(|| fail("ops_unknown_text_box", id, "the board has no drawings yet"))?;
        let before = dr.board_items.text_boxes.len();
        dr.board_items.text_boxes.retain(|b| b.id != id);
        if dr.board_items.text_boxes.len() == before {
            return Err(fail("ops_unknown_text_box", id, "no text box with this id"));
        }
        Ok(())
    }
}

// ----------------------------------------------------------------------------------------------------------------------------- reference images

impl<'a> Board<'a> {
    /// What a reference image must be: on a layer the board has on, with a positive scale, and the file of an image of a kind known (`REFERENCE_IMAGE::ReadImageFile`).
    fn check_ref_image(&self, what: &str, layer: &str, scale_ppm: i64, data: Option<&str>) -> Result<(), Vec<CheckResult>> {
        if layer.is_empty() {
            return Err(fail("ops_bad_ref_image", what, "a reference image needs a layer"));
        }
        self.layer_is_on(layer, what)?;
        if scale_ppm <= 0 {
            return Err(fail("ops_bad_ref_image", what, "the scale must be a positive number"));
        }
        if let Some(data) = data {
            if eda_model::image_info::header_of_base64(data).is_none() {
                return Err(fail("ops_bad_ref_image", what, "the data is not a PNG, JPEG, GIF or BMP file"));
            }
        }
        Ok(())
    }

    fn add_ref_image(&mut self, mut image: RefImage) -> Result<(), Vec<CheckResult>> {
        image.data = eda_model::image_info::compact(&image.data);
        self.check_ref_image("image", &image.layer, image.scale_ppm, Some(&image.data))?;
        image.id = String::new();
        let dr = self.drawings_mut();
        dr.board_items.images.push(image);
        dr.assign_missing_ids();
        Ok(())
    }

    fn ref_image_mut(&mut self, id: &str) -> Result<&mut RefImage, Vec<CheckResult>> {
        let dr = self.design.drawings.as_mut().ok_or_else(|| fail("ops_unknown_ref_image", id, "the board has no drawings yet"))?;
        dr.board_items.images.iter_mut().find(|i| i.id == id).ok_or_else(|| fail("ops_unknown_ref_image", id, "no reference image with this id"))
    }

    fn edit_ref_image(&mut self, id: &str, layer: &str, at: Point, scale_ppm: i64) -> Result<(), Vec<CheckResult>> {
        self.check_ref_image(id, layer, scale_ppm, None)?;
        let image = self.ref_image_mut(id)?;
        image.layer = layer.to_string();
        image.at = at;
        image.scale_ppm = scale_ppm;
        Ok(())
    }

    fn set_ref_image_data(&mut self, id: &str, data: &str) -> Result<(), Vec<CheckResult>> {
        let data = eda_model::image_info::compact(data);
        if eda_model::image_info::header_of_base64(&data).is_none() {
            return Err(fail("ops_bad_ref_image", id, "the data is not a PNG, JPEG, GIF or BMP file"));
        }
        self.ref_image_mut(id)?.data = data;
        Ok(())
    }

    fn delete_ref_image(&mut self, id: &str) -> Result<(), Vec<CheckResult>> {
        let dr = self.design.drawings.as_mut().ok_or_else(|| fail("ops_unknown_ref_image", id, "the board has no drawings yet"))?;
        let before = dr.board_items.images.len();
        dr.board_items.images.retain(|i| i.id != id);
        if dr.board_items.images.len() == before {
            return Err(fail("ops_unknown_ref_image", id, "no reference image with this id"));
        }
        Ok(())
    }
}

// ----------------------------------------------------------------------------------------------------------------------------- points

impl<'a> Board<'a> {
    /// What a point must be to be drawn: on a layer the board has on, and with a size that is a length (`PCB_POINT::SetSize` takes any `int`; a mark of no size is nothing to see).
    fn check_point(&self, what: &str, layer: &str, size_um: Um) -> Result<(), Vec<CheckResult>> {
        if layer.is_empty() {
            return Err(fail("ops_bad_point", what, "a point needs a layer"));
        }
        self.layer_is_on(layer, what)?;
        if size_um <= 0 {
            return Err(fail("ops_bad_point", what, "the size of a point must be greater than zero"));
        }
        Ok(())
    }

    fn add_point(&mut self, mut point: PcbPoint) -> Result<(), Vec<CheckResult>> {
        self.check_point("point", &point.layer, point.size_um)?;
        point.id = String::new();
        let dr = self.drawings_mut();
        dr.board_items.points.push(point);
        dr.assign_missing_ids();
        Ok(())
    }

    fn edit_point(&mut self, id: &str, layer: &str, at: Point, size_um: Um) -> Result<(), Vec<CheckResult>> {
        self.check_point(id, layer, size_um)?;
        let dr = self.design.drawings.as_mut().ok_or_else(|| fail("ops_unknown_point", id, "the board has no drawings yet"))?;
        let p = dr.board_items.points.iter_mut().find(|p| p.id == id).ok_or_else(|| fail("ops_unknown_point", id, "no point with this id"))?;
        p.layer = layer.to_string();
        p.at = at;
        p.size_um = size_um;
        Ok(())
    }

    fn delete_point(&mut self, id: &str) -> Result<(), Vec<CheckResult>> {
        let dr = self.design.drawings.as_mut().ok_or_else(|| fail("ops_unknown_point", id, "the board has no drawings yet"))?;
        let before = dr.board_items.points.len();
        dr.board_items.points.retain(|p| p.id != id);
        if dr.board_items.points.len() == before {
            return Err(fail("ops_unknown_point", id, "no point with this id"));
        }
        Ok(())
    }
}

/// `PCB_POINT::Rotate` / `::Flip`: the position goes where the transform puts it (`RotatePoint`, `MIRROR`) and a flip puts the point on the other side's layer.
pub(crate) fn transform_point(p: &mut PcbPoint, x: Xform, copper: usize) {
    p.at = x.point(p.at);
    if let Xform::Flip { .. } = x {
        p.layer = flip_layer(&p.layer, copper);
    }
}

/// `PCB_REFERENCE_IMAGE::Move` / `::Rotate` / `::Flip` as far as the position and the layer go: the centre goes where the transform puts it, and a flip puts the image on
/// the other side's layer. The pixels are not turned or mirrored (that takes a decoder: `BITMAP_BASE::Rotate` / `::Mirror`), so a quarter turn of a single image about its own
/// centre changes nothing -- GAPS.md item 24 lists it.
pub(crate) fn transform_ref_image(i: &mut RefImage, x: Xform, copper: usize) {
    i.at = x.point(i.at);
    if let Xform::Flip { .. } = x {
        i.layer = flip_layer(&i.layer, copper);
    }
}

/// `PCB_BARCODE::SetErrorCorrection`: a Micro QR code has no High level, so a High one is Quartile.
pub(crate) fn normalize_barcode(b: &mut Barcode) {
    if b.kind == BarcodeKind::MicroQr && b.ecc == EccLevel::H {
        b.ecc = EccLevel::Q;
    }
}

/// `PCB_BARCODE::Move` / `::Rotate` / `::Flip`: the centre goes where the transform puts it, a turn adds to the barcode's own angle (`Xform::Rotate`'s is clockwise, the file's
/// counter-clockwise, so it is taken off), a flip puts the barcode on the other side's layer and a top-bottom one turns it half a turn; the symbol is mirrored on a back layer when it is drawn.
pub(crate) fn transform_barcode(b: &mut Barcode, x: Xform, copper: usize) {
    b.at = x.point(b.at);
    match x {
        Xform::Move { .. } => {}
        Xform::Rotate { angle, .. } => b.angle = norm360(b.angle as i64 - angle) as Millideg,
        Xform::Flip { dir, .. } => {
            if matches!(dir, FlipDirection::TopBottom) {
                b.angle = norm360(b.angle as i64 + 180_000) as Millideg;
            }
            b.layer = flip_layer(&b.layer, copper);
        }
    }
}

// ----------------------------------------------------------------------------------------------------------------------------- the transform verbs

fn norm360(a: i64) -> i64 {
    a.rem_euclid(360_000)
}

/// Whether a text angle in millidegrees is a multiple of 90 degrees (`EDA_ANGLE::IsCardinal`).
fn is_cardinal(angle: Millideg) -> bool {
    angle % 90_000 == 0
}

/// `EDA_SHAPE::GetCornersInSequence( angle )` of a text box: the four corners in the order the text's frame has them (top left, top right, bottom right, bottom left of the
/// text as it reads), for a rectangle (its bounding box turned to the angle's quadrant) and for a polygon (the corners that sit furthest left, up, right and down).
pub fn corners_in_sequence(t: &TextBox, angle: Millideg) -> [Point; 4] {
    let angle = angle % 360_000;
    if t.pts.is_empty() || is_cardinal(angle) {
        let [x0, y0, x1, y1] = t.bounds();
        let (lt, rt, rb, lb) = (Point { x: x0, y: y0 }, Point { x: x1, y: y0 }, Point { x: x1, y: y1 }, Point { x: x0, y: y1 });
        return match angle {
            90_000 => [lb, lt, rt, rb],
            180_000 => [rb, lb, lt, rt],
            270_000 => [rt, rb, lb, lt],
            _ => [lt, rt, rb, lb],
        };
    }
    let mut corners = t.pts.clone();
    while corners.len() < 4 {
        let last = *corners.last().unwrap_or(&Point { x: 0, y: 0 });
        corners.push(Point { x: last.x + 10, y: last.y + 10 });
    }
    let (mut min_x, mut max_x, mut min_y, mut max_y) = (corners[0], corners[0], corners[0], corners[0]);
    for c in &corners {
        if c.x < min_x.x {
            min_x = *c;
        }
        if c.x > max_x.x {
            max_x = *c;
        }
        if c.y < min_y.y {
            min_y = *c;
        }
        if c.y > max_y.y {
            max_y = *c;
        }
    }
    if angle < 90_000 {
        [min_x, min_y, max_x, max_y]
    } else if angle < 180_000 {
        [max_y, min_x, min_y, max_x]
    } else if angle < 270_000 {
        [max_x, max_y, min_x, min_y]
    } else {
        [min_y, max_x, max_y, min_x]
    }
}

/// `PCB_TEXTBOX::Move` / `::Rotate` / `::Flip` (with `EDA_SHAPE::rotate` / `::flip`). `Xform::Rotate`'s angle is clockwise on the screen, so the text's own angle (KiCad's,
/// counter-clockwise) loses it. A rectangle turned by anything but a quarter turn becomes the polygon of its four corners; a polygon whose text angle is again a multiple
/// of 90 degrees becomes a rectangle again, started at the first corner of the text's frame. A flip puts the box on the other side's layer, negates (left-right) or
/// reflects (top-bottom) the text angle, and mirrors the glyphs of a box on a layer that belongs to a side.
pub(crate) fn transform_text_box(t: &mut TextBox, x: Xform, copper: usize) {
    match x {
        Xform::Move { .. } => {
            t.start = x.point(t.start);
            t.end = x.point(t.end);
            for p in t.pts.iter_mut() {
                *p = x.point(*p);
            }
        }
        Xform::Rotate { angle, .. } => {
            if t.pts.is_empty() {
                if angle.rem_euclid(90_000) == 0 {
                    t.start = x.point(t.start);
                    t.end = x.point(t.end);
                } else {
                    // "Convert non-cardinally-rotated rect to a diamond"
                    let (s, e) = (t.start, t.end);
                    let corners = [s, Point { x: e.x, y: s.y }, e, Point { x: s.x, y: e.y }];
                    t.pts = corners.iter().map(|c| x.point(*c)).collect();
                    t.start = t.pts[0];
                    t.end = t.pts[2];
                }
            } else {
                for p in t.pts.iter_mut() {
                    *p = x.point(*p);
                }
                t.start = t.pts[0];
                t.end = t.pts[2];
            }
            t.angle = norm360(t.angle as i64 - angle) as Millideg;
            if is_cardinal(t.angle) && !t.pts.is_empty() {
                let corners = corners_in_sequence(t, t.angle);
                let diag = Point { x: corners[2].x - corners[0].x, y: corners[2].y - corners[0].y };
                let (dx, dy) = (diag.x.abs(), diag.y.abs());
                let c0 = corners[0];
                t.start = c0;
                t.end = match t.angle {
                    90_000 => Point { x: c0.x + dx, y: c0.y - dy },
                    180_000 => Point { x: c0.x - dx, y: c0.y - dy },
                    270_000 => Point { x: c0.x - dx, y: c0.y + dy },
                    _ => Point { x: c0.x + dx, y: c0.y + dy },
                };
                t.pts.clear();
            }
        }
        Xform::Flip { dir, .. } => {
            t.start = x.point(t.start);
            t.end = x.point(t.end);
            for p in t.pts.iter_mut() {
                *p = x.point(*p);
            }
            t.angle = match dir {
                FlipDirection::LeftRight => norm360(-(t.angle as i64)),
                FlipDirection::TopBottom => norm360(180_000 - t.angle as i64),
            } as Millideg;
            t.layer = flip_layer(&t.layer, copper);
            if side_specific(&t.layer) {
                t.mirror = !t.mirror;
            }
        }
    }
}

/// Move, turn or flip the items of the kinds this module adds that `ids` names (`Cmd::MoveItems`, `RotateItems`, `FlipItems`).
pub(crate) fn transform_board_items(dr: &mut DrawingsSection, ids: &BTreeSet<String>, x: Xform, copper: usize) {
    for t in dr.board_items.text_boxes.iter_mut().filter(|t| ids.contains(&t.id)) {
        transform_text_box(t, x, copper);
    }
    for i in dr.board_items.images.iter_mut().filter(|i| ids.contains(&i.id)) {
        transform_ref_image(i, x, copper);
    }
    for p in dr.board_items.points.iter_mut().filter(|p| ids.contains(&p.id)) {
        transform_point(p, x, copper);
    }
    for b in dr.barcodes.iter_mut().filter(|b| ids.contains(&b.id)) {
        transform_barcode(b, x, copper);
    }
    for g in dr.board_items.grid_items.iter_mut().filter(|g| ids.contains(&g.id)) {
        crate::kicad10_items::transform_grid_item(g, x);
    }
    for v in dr.board_items.via_stitches.iter_mut().filter(|v| ids.contains(&v.id)) {
        crate::kicad10_items::transform_via_stitch(v, x);
    }
    for m in dr.board_items.drill_maps.iter_mut().filter(|m| ids.contains(&m.id)) {
        crate::kicad10_items::transform_drill_map(m, x);
        if matches!(x, Xform::Flip { .. }) {
            m.layer = flip_layer(&m.layer, copper);
        }
    }
}

/// Is `id` the id of an item of the kinds this module adds?
pub(crate) fn has_board_item(dr: &DrawingsSection, id: &str) -> bool {
    dr.board_items.ids().any(|i| i == id) || dr.barcodes.iter().any(|b| b.id == id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use eda_model::pcb_items::TextBox;

    fn bx() -> TextBox {
        TextBox::new("F.SilkS", Point { x: 10_000, y: 10_000 }, Point { x: 30_000, y: 20_000 }, 1_000, 120)
    }

    fn rot(angle_cw: i64) -> Xform {
        Xform::Rotate { pivot: Point { x: 0, y: 0 }, angle: angle_cw }
    }

    #[test]
    fn a_quarter_turn_keeps_a_rectangle_and_turns_the_text() {
        let mut t = bx();
        transform_text_box(&mut t, rot(90_000), 2);
        assert!(t.is_rect());
        // clockwise on the screen: (x, y) -> (-y, x)
        assert_eq!((t.start, t.end), (Point { x: -10_000, y: 10_000 }, Point { x: -20_000, y: 30_000 }));
        assert_eq!(t.angle, 270_000, "the text angle is KiCad's, counter-clockwise: a clockwise quarter turn takes 90 from 0");
        // ... and the next brings the frame round to 180
        transform_text_box(&mut t, rot(90_000), 2);
        assert_eq!(t.angle, 180_000);
    }

    #[test]
    fn four_quarter_turns_bring_the_box_back() {
        let mut t = bx();
        for _ in 0..4 {
            transform_text_box(&mut t, rot(90_000), 2);
        }
        assert_eq!((t.start, t.end, t.angle), (bx().start, bx().end, 0));
        assert!(t.is_rect());
    }

    #[test]
    fn a_turn_that_is_not_a_quarter_makes_a_polygon_and_a_turn_back_makes_the_rectangle_again() {
        let mut t = bx();
        transform_text_box(&mut t, rot(45_000), 2);
        assert_eq!(t.pts.len(), 4);
        assert_eq!(t.angle, 315_000);
        assert_eq!((t.start, t.end), (t.pts[0], t.pts[2]));
        transform_text_box(&mut t, rot(-45_000), 2);
        assert!(t.is_rect(), "the polygon is a rectangle again at a multiple of 90 degrees: {t:?}");
        let [x0, y0, x1, y1] = t.bounds();
        // The rounding of two 45 degree turns is a micrometre at most.
        assert!((x0 - 10_000).abs() <= 1 && (y0 - 10_000).abs() <= 1 && (x1 - 30_000).abs() <= 1 && (y1 - 20_000).abs() <= 1, "{:?}", t.bounds());
        assert_eq!(t.angle, 0);
    }

    #[test]
    fn a_flip_moves_the_box_to_the_other_side_and_mirrors_its_text() {
        let mut t = bx();
        transform_text_box(&mut t, Xform::Flip { pivot: Point { x: 0, y: 0 }, dir: FlipDirection::LeftRight }, 2);
        assert_eq!(t.layer, "B.SilkS");
        assert!(t.mirror, "text on a back layer reads mirrored");
        assert_eq!((t.start, t.end), (Point { x: -10_000, y: 10_000 }, Point { x: -30_000, y: 20_000 }));
        assert_eq!(t.angle, 0);
        let mut u = bx();
        u.angle = 90_000;
        transform_text_box(&mut u, Xform::Flip { pivot: Point { x: 0, y: 0 }, dir: FlipDirection::TopBottom }, 2);
        assert_eq!(u.angle, 90_000, "180 - 90");
        assert_eq!((u.start.y, u.end.y), (-10_000, -20_000));
    }

    #[test]
    fn corners_follow_the_text_frame() {
        let mut t = bx();
        let at = |a| corners_in_sequence(&t, a);
        assert_eq!(at(0)[0], Point { x: 10_000, y: 10_000 });
        assert_eq!(at(90_000)[0], Point { x: 10_000, y: 20_000 });
        assert_eq!(at(180_000)[0], Point { x: 30_000, y: 20_000 });
        assert_eq!(at(270_000)[0], Point { x: 30_000, y: 10_000 });
        t.pts = vec![Point { x: 0, y: 0 }, Point { x: 5_000, y: 5_000 }, Point { x: 0, y: 10_000 }, Point { x: -5_000, y: 5_000 }];
        // 45 degrees: minX, minY, maxX, maxY
        assert_eq!(corners_in_sequence(&t, 45_000), [Point { x: -5_000, y: 5_000 }, Point { x: 0, y: 0 }, Point { x: 5_000, y: 5_000 }, Point { x: 0, y: 10_000 }]);
    }
}
