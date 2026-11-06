//! "Edit Text & Graphics Properties..." (`pcbnew.GlobalEdit.editTextAndGraphics`): what `DIALOG_GLOBAL_EDIT_TEXT_AND_GRAPHICS::processItem`
//! (`pcbnew/dialogs/dialog_global_edit_text_and_graphics.cpp`) does to each item the dialog's scope and filters picked -- the caller picks them, this
//! only changes them.
//!
//! * "Set to specified values:" -- the layer, the line thickness of a graphic item or a dimension, the size and thickness of a text or a dimension's
//!   label. A field left `None` is the dialog's `INDETERMINATE_ACTION` and leaves the property alone.
//! * "Set to layer default values:" -- `aItem->StyleFromSettings( *m_brdSettings, false )`, with the settings Board Setup > Text & Graphics >
//!   Defaults holds (`BOARD_DESIGN_SETTINGS::m_LineThickness`, `m_TextSize`, `m_TextThickness`, read by the layer class of the item's layer,
//!   `GetLayerClass`):
//!   * `PCB_SHAPE::StyleFromSettings`: the stroke width is the line thickness of the layer class;
//!   * `PCB_TEXT::StyleFromSettings`: the text size (width and height), the text thickness and italic are the layer class's (the width and italic of a
//!     board text land on its `TextExtra`; keep-upright is only for a footprint's text, which this dialog does not reach here). A text box and a table
//!     cell take the same; a dimension's label has one size in this model (the class's height) and no italic;
//!   * `PCB_DIMENSION_BASE::StyleFromSettings`: the text as above, `SetLineThickness( GetLineThickness( layer ) )`, and the units, units format,
//!     precision, zero suppression, text position and "keep text aligned" of the board's dimension settings.
//!
//! The edges and courtyard classes have a line thickness in Board Setup but no text cells (`ROW_EDGES` and `ROW_COURTYARD` leave them disabled), so a
//! text on `Edge.Cuts` or a courtyard layer keeps `BOARD_DESIGN_SETTINGS`' own constants (`DEFAULT_TEXT_SIZE` 1 mm, `DEFAULT_TEXT_WIDTH` 0.15 mm).
//!
//! # The footprints' items
//!
//! `TransferDataFromWindow` walks every footprint before the board's drawings: its Reference and Value (`fp->Reference()`, `fp->Value()`), its other
//! fields, and its graphical items (`fp->GraphicalItems()`: texts, text boxes, table cells, dimensions, shapes and barcodes). Here a footprint is a pose of
//! a part, so they are where the design keeps them ([`FootprintItems`]):
//!
//! * a field (`REF:Reference`, `REF:Value`, `REF:<name>`) is its layout in the footprint's edit (`DrawingsSection::footprint_edits`, [`fp_edit::FieldLayout`]), the
//!   default layout when the field was never edited;
//! * a footprint's own text (`REF~t<k>`) and graphic (`REF~g<k>`) are the k-th of its art (`Footprint::art`'s `texts` and `shapes`), which a library footprint
//!   shares with every instance: the edit gives the instance its own copy of the footprint, as Change Footprint does
//!   (`FootprintAssignment::definition`, the name unchanged), with the pad edits still laid over it. Art keeps its layers as for a top-side footprint, so a
//!   layer set on a bottom-side footprint's item is turned over first (`layer_on_side`).
//!
//! `processItem`'s rows for them: a text's width and height, its thickness or "auto" (`SetAutoThickness`: a field's and a footprint text's thickness 0),
//! bold, italic, the font (`m_fontCtrl`: "KiCad Font" is the stroke font, no face; any other is the face), keep upright and "Center on footprint" (only for an
//! item that has a parent footprint: `SetKeepUpright`, `SetTextPos( aItem->GetParent()->GetCenter() )`, the centre of `FOOTPRINT::GetBoundingBox( false )`),
//! and visible (a field only, `PCB_FIELD::SetVisible`). "Set to layer default values:" is `PCB_TEXT::StyleFromSettings` (keep upright too, a parent
//! footprint's text) and `PCB_SHAPE::StyleFromSettings`.
//!
//! The board's own texts take the same rows: a width that is not the height, bold, italic and the face land on the text's `TextExtra`; "auto" is
//! `TextExtra::auto_thickness` with the pen `GetEffectiveTextPenWidth` gives. A dimension's label has one size here (the height) and no bold, italic or
//! face; its "auto" thickness is written as that pen.
//!
//! # Track & Via Properties' via rows
//!
//! [`Board::edit_via_padstacks`] is the "Via annular rings:" and "Via protection features:" rows of `DIALOG_GLOBAL_EDIT_TRACKS_AND_VIAS::processItem`:
//! `Padstack().SetUnconnectedLayerMode` ([`ViaExtra::set_unconnected_layers`]) and an IPC-4761 preset (`VIA_PROTECTION_UI_MIXIN::setViaConfiguration`,
//! [`Ipc4761Preset`]) on every via named.

use super::Board;
use eda_model::fp_art::layer_on_side;
use eda_model::fp_assign::FootprintAssignment;
use eda_model::fp_edit::{self, FieldLayout, REFERENCE, VALUE};
use eda_model::ir::{FootprintInstance, Nm, Point};
use eda_model::padstack::UnconnectedLayers;
use eda_model::pcb_items::auto_pen_nm;
use eda_model::rules::TextGraphicsDefaults;
use eda_model::via_extra::{ViaExtra, ViaProps};
use eda_model::CheckResult;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[cfg(test)]
pub use eda_model::rules::{layer_class, LayerClass};

/// `BOARD_DESIGN_SETTINGS::GetLineThickness( aLayer )` (`TextGraphicsDefaults::line_thickness_nm`).
pub fn line_thickness(d: &TextGraphicsDefaults, layer: &str) -> Nm {
    d.line_thickness_nm(layer)
}

/// `BOARD_DESIGN_SETTINGS::GetTextSize( aLayer )`'s height, the one size a dimension's label has in this model.
pub fn text_size(d: &TextGraphicsDefaults, layer: &str) -> Nm {
    d.text_size_nm(layer).1
}

/// `BOARD_DESIGN_SETTINGS::GetTextSize( aLayer )` as a glyph height and a width that is `None` when it is the height (how a text, a text box keeps it).
fn text_height_width(d: &TextGraphicsDefaults, layer: &str) -> (Nm, Option<Nm>) {
    let (w, h) = d.text_size_nm(layer);
    (h, (w != h).then_some(w))
}

/// `BOARD_DESIGN_SETTINGS::GetTextThickness( aLayer )`.
pub fn text_thickness(d: &TextGraphicsDefaults, layer: &str) -> Nm {
    d.text_thickness_nm(layer)
}

/// `KICAD_FONT_NAME`: the stroke font's name in a Font list (`FONT_CHOICE`).
pub const KICAD_FONT: &str = "KiCad Font";

/// The footprints' items `Cmd::EditTextAndGraphics` changes (see the module doc): the ids the studio gives them.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FootprintItems {
    /// Fields: `REF:Reference`, `REF:Value`, `REF:<name>` ([`fp_edit::field_id`]).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub field_ids: Vec<String>,
    /// A footprint's own texts, `REF~t<k>` ([`fp_text_id`]).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub text_ids: Vec<String>,
    /// A footprint's graphics, `REF~g<k>` ([`fp_shape_id`]).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub shape_ids: Vec<String>,
}

impl FootprintItems {
    pub fn is_empty(&self) -> bool {
        self.field_ids.is_empty() && self.text_ids.is_empty() && self.shape_ids.is_empty()
    }
}

/// The id of the k-th text of a footprint's art (`crates/cli/src/fp_art_json.rs` gives its texts these ids).
pub fn fp_text_id(reference: &str, k: usize) -> String {
    format!("{reference}~t{k}")
}

/// The id of the k-th graphic of a footprint's art (the order of `parts[].graphics`).
pub fn fp_shape_id(reference: &str, k: usize) -> String {
    format!("{reference}~g{k}")
}

/// `REF~t<k>` / `REF~g<k>` as the reference and k, for the kind `tag` (`'t'` or `'g'`).
fn parse_fp_item_id(id: &str, tag: char) -> Option<(&str, usize)> {
    let (reference, rest) = id.rsplit_once('~')?;
    let k = rest.strip_prefix(tag)?.parse().ok()?;
    (!reference.is_empty()).then_some((reference, k))
}

/// "Set to specified values:"'s rows beyond the layer, the line thickness and the one text size (see the module doc). `None` / `false` is
/// `INDETERMINATE_ACTION` ("-- leave unchanged --") or a box left unticked.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TextRows {
    /// "Text width:" (`SetTextSize( VECTOR2I( width, GetTextSize().y ) )`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub width: Option<Nm>,
    /// "Text height:".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub height: Option<Nm>,
    /// The auto-thickness button (`m_autoTextThickness`): `SetAutoThickness( true )`, which wins over a thickness.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub auto_thickness: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bold: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub italic: Option<bool>,
    /// "Font:": [`KICAD_FONT`] or a family name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub font: Option<String>,
    /// "Keep upright" (a footprint's texts and fields).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub keep_upright: Option<bool>,
    /// "Visible  (fields only)".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub visible: Option<bool>,
    /// "Center on footprint" (a footprint's texts and fields).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub center_on_footprint: bool,
}

impl TextRows {
    /// The face the Font row gives: `Some(None)` for the stroke font, `Some(Some(family))` for another, `None` to leave the font alone.
    fn face(&self) -> Option<Option<String>> {
        self.font.as_ref().map(|f| (f != KICAD_FONT && !f.is_empty()).then(|| f.clone()))
    }
}

/// What `Cmd::EditTextAndGraphics` asks for.
pub(crate) struct GlobalTextEdit<'a> {
    pub shape_ids: &'a [String],
    pub text_ids: &'a [String],
    pub dimension_ids: &'a [String],
    /// "Set to layer default values:" instead of "Set to specified values:".
    pub layer_defaults: bool,
    pub layer: Option<&'a str>,
    pub line_width: Option<Nm>,
    pub text_size: Option<Nm>,
    pub text_thickness: Option<Nm>,
    pub footprint: &'a FootprintItems,
    pub rows: &'a TextRows,
}

impl GlobalTextEdit<'_> {
    /// The glyph width asked for: "Text width:", else the older one-size field.
    fn width(&self) -> Option<Nm> {
        self.rows.width.or(self.text_size)
    }

    /// The glyph height asked for.
    fn height(&self) -> Option<Nm> {
        self.rows.height.or(self.text_size)
    }
}

/// `processItem`'s text rows on a footprint field's or a footprint text's layout (`layer` already set by the caller): the width, the height, the thickness
/// or auto, bold, italic, keep upright; the font and the visibility are the caller's (a field keeps its face in the layout, a text beside it).
fn apply_text_rows(l: &mut FieldLayout, e: &GlobalTextEdit<'_>) {
    if let Some(w) = e.width() {
        l.size.0 = w;
    }
    if let Some(h) = e.height() {
        l.size.1 = h;
    }
    if e.rows.auto_thickness {
        l.thickness = Nm::ZERO;
    } else if let Some(t) = e.text_thickness {
        l.thickness = t;
    }
    if let Some(b) = e.rows.bold {
        l.bold = b;
    }
    if let Some(i) = e.rows.italic {
        l.italic = i;
    }
    if let Some(u) = e.rows.keep_upright {
        l.keep_upright = u;
    }
}

/// `PCB_TEXT::StyleFromSettings( settings, false )` on a footprint's text or field: the size, the thickness, italic and, for a text with a parent footprint,
/// keep upright, all of the layer class of `board_layer`.
fn style_text_from_settings(l: &mut FieldLayout, d: &TextGraphicsDefaults, board_layer: &str) {
    l.size = d.text_size_nm(board_layer);
    l.thickness = d.text_thickness_nm(board_layer);
    let (italic, upright) = d.text_italic_upright(board_layer);
    l.italic = italic;
    l.keep_upright = upright;
}

/// The centre of `FOOTPRINT::GetBoundingBox( false )` (`FOOTPRINT::GetCenter`).
fn box_center(b: eda_model::multichannel::Box4) -> Point {
    Point { x: (b.0 + b.2) / 2, y: (b.1 + b.3) / 2 }
}

fn refuse(check: &str, message: &str) -> Result<(), Vec<CheckResult>> {
    Err(vec![CheckResult::fail(check, "edit_text_and_graphics", message)])
}

impl Board<'_> {
    /// `Cmd::EditTextAndGraphics`. Unknown ids among the lists are tolerated (they match nothing), the same convention `CommitRoute` set for a list the
    /// caller computed; refused when every list is empty (nothing named at all, almost certainly a caller bug) or an explicit value is out of range.
    pub(crate) fn edit_text_and_graphics(&mut self, e: &GlobalTextEdit<'_>) -> Result<(), Vec<CheckResult>> {
        if e.shape_ids.is_empty() && e.text_ids.is_empty() && e.dimension_ids.is_empty() && e.footprint.is_empty() {
            return refuse("ops_bad_global_edit", "no items given");
        }
        if !e.layer_defaults {
            // Not `known_layer` -- that validates against the board's *copper* layer list only, and silkscreen/fab/edge layers (where a shape or text
            // normally lives) are deliberately not on it. Same "just non-empty" convention `edit_shape`/`edit_text` already use.
            if matches!(e.layer, Some(l) if l.is_empty()) {
                return refuse("ops_bad_shape", "a layer, if given, cannot be empty");
            }
            if matches!(e.line_width, Some(w) if w <= Nm::ZERO) {
                return refuse("ops_bad_shape", "line width must be positive");
            }
            if [e.text_size, e.rows.width, e.rows.height].into_iter().flatten().any(|s| s <= Nm::ZERO) {
                return refuse("ops_bad_text", "text size must be positive");
            }
            if matches!(e.text_thickness, Some(t) if t <= Nm::ZERO) {
                return refuse("ops_bad_text", "text thickness must be positive");
            }
        }
        self.edit_footprint_items(e);

        let shapes: BTreeSet<&str> = e.shape_ids.iter().map(String::as_str).collect();
        let texts: BTreeSet<&str> = e.text_ids.iter().map(String::as_str).collect();
        let dims: BTreeSet<&str> = e.dimension_ids.iter().map(String::as_str).collect();
        let defaults = self.model.board.text_graphics;
        let Some(dr) = self.design.drawings.as_mut() else { return Ok(()) };
        let dim_style = dr.dimension_settings;
        let face = e.rows.face();

        for s in dr.shapes.iter_mut().filter(|s| shapes.contains(s.id())) {
            if e.layer_defaults {
                let w = line_thickness(&defaults, s.layer());
                s.set_stroke_width(w);
                continue;
            }
            if let Some(l) = e.layer {
                s.set_layer(l.to_string());
            }
            if let Some(w) = e.line_width {
                s.set_stroke_width(w);
            }
        }
        // A board text: `Text` holds the layer, the height and the pen; what else the rows set is its `TextExtra` (the glyph width when it is not the height,
        // bold, italic, the face, the automatic pen). Each text's extra is worked out whole and put back (or dropped when it says nothing).
        let ids: Vec<String> = dr.texts.iter().filter(|t| texts.contains(t.id.as_str())).map(|t| t.id.clone()).collect();
        for id in ids {
            let mut extra = dr.text_extra(&id).cloned().unwrap_or(eda_model::board_text::TextExtra { id: id.clone(), ..Default::default() });
            let Some(t) = dr.texts.iter_mut().find(|t| t.id == id) else { continue };
            if e.layer_defaults {
                // `PCB_TEXT::StyleFromSettings`: `SetTextSize( GetTextSize( layer ) )`, `SetTextThickness( .. )`, `SetItalic( GetTextItalic( layer ) )`.
                let (h, w) = text_height_width(&defaults, &t.layer);
                t.size_nm = h;
                t.stroke_width = text_thickness(&defaults, &t.layer);
                extra.width_nm = w;
                extra.italic = defaults.text_italic_upright(&t.layer).0;
                extra.auto_thickness = false;
            } else {
                if let Some(l) = e.layer {
                    t.layer = l.to_string();
                }
                // `SetTextSize( VECTOR2I( width, y ) )`, then `( x, height )`: the glyph width the text had unless one is given.
                let width = e.width().unwrap_or(extra.width_nm.unwrap_or(t.size_nm));
                if let Some(h) = e.height() {
                    t.size_nm = h;
                }
                extra.width_nm = (width != t.size_nm).then_some(width);
                if let Some(b) = e.rows.bold {
                    extra.bold = b;
                }
                if let Some(i) = e.rows.italic {
                    extra.italic = i;
                }
                if let Some(f) = &face {
                    extra.face = f.clone();
                }
                if e.rows.auto_thickness {
                    // `EDA_TEXT::GetEffectiveTextPenWidth` with no thickness: the pen of the smaller of the glyph's two sizes (after the size and bold).
                    extra.auto_thickness = true;
                    t.stroke_width = auto_pen_nm(width.min(t.size_nm), extra.bold);
                } else if let Some(th) = e.text_thickness {
                    t.stroke_width = th;
                    extra.auto_thickness = false;
                }
            }
            match (dr.text_extras.iter().position(|x| x.id == id), extra.is_default()) {
                (Some(i), true) => {
                    dr.text_extras.remove(i);
                }
                (Some(i), false) => dr.text_extras[i] = extra,
                (None, false) => dr.text_extras.push(extra),
                (None, true) => {}
            }
        }
        // A text box is a text and a shape (`PCB_TEXTBOX` is an `EDA_TEXT` and a `PCB_SHAPE`): its glyphs, its pen and its border width.
        for t in dr.board_items.text_boxes.iter_mut().filter(|t| texts.contains(t.id.as_str())) {
            if e.layer_defaults {
                // `PCB_TEXTBOX::StyleFromSettings`: `PCB_SHAPE::StyleFromSettings`, then the text
                t.stroke_width_nm = line_thickness(&defaults, &t.layer);
                (t.size_nm, t.width_nm) = text_height_width(&defaults, &t.layer);
                t.thickness_nm = text_thickness(&defaults, &t.layer);
                t.italic = defaults.text_italic_upright(&t.layer).0;
                continue;
            }
            if let Some(l) = e.layer {
                t.layer = l.to_string();
            }
            let width = e.width().unwrap_or(t.width_nm.unwrap_or(t.size_nm));
            if let Some(h) = e.height() {
                t.size_nm = h;
            }
            t.width_nm = (width != t.size_nm).then_some(width);
            if e.rows.auto_thickness {
                t.thickness_nm = Nm::ZERO;
            } else if let Some(th) = e.text_thickness {
                t.thickness_nm = th;
            }
            if let Some(b) = e.rows.bold {
                t.bold = b;
            }
            if let Some(i) = e.rows.italic {
                t.italic = i;
            }
            if let Some(f) = &face {
                t.face = f.clone();
            }
            if let Some(w) = e.line_width {
                t.stroke_width_nm = w;
            }
        }
        // A table's cells (`RunOnChildren`, `PCB_TABLECELL_T`), named `<table>:<row>:<col>`: a `PCB_TEXTBOX` each, on the table's layer. A cell has no layer and no
        // border of its own here (the table's grid draws it), so only its text changes.
        for table in dr.tables.iter_mut() {
            let layer = table.layer.clone();
            let cols = table.col_widths.len().max(1);
            for (i, cell) in table.cells.iter_mut().enumerate() {
                if !texts.contains(eda_model::table::cell_item_id(&table.id, i / cols, i % cols).as_str()) {
                    continue;
                }
                if e.layer_defaults {
                    // `PCB_TEXTBOX::StyleFromSettings` (a cell is one): the class's (width, height), thickness and italic.
                    (cell.style.width_nm, cell.style.height_nm) = defaults.text_size_nm(&layer);
                    cell.style.thickness_nm = text_thickness(&defaults, &layer);
                    cell.style.italic = defaults.text_italic_upright(&layer).0;
                    continue;
                }
                if let Some(w) = e.width() {
                    cell.style.width_nm = w;
                }
                if let Some(h) = e.height() {
                    cell.style.height_nm = h;
                }
                if e.rows.auto_thickness {
                    cell.style.thickness_nm = Nm::ZERO;
                } else if let Some(th) = e.text_thickness {
                    cell.style.thickness_nm = th;
                }
                if let Some(b) = e.rows.bold {
                    cell.style.bold = b;
                }
                if let Some(i) = e.rows.italic {
                    cell.style.italic = i;
                }
                if let Some(f) = &face {
                    cell.style.face = f.clone();
                }
            }
        }
        // `PCB_BARCODE_T` with the graphics: its layer and its text's size (`SetTextSize( m_textHeight )`, else the width; `PCB_BARCODE::StyleFromSettings`: the
        // layer's text height).
        for b in dr.barcodes.iter_mut().filter(|b| shapes.contains(b.id.as_str())) {
            if e.layer_defaults {
                b.text_height_nm = text_size(&defaults, &b.layer);
                continue;
            }
            if let Some(l) = e.layer {
                b.layer = l.to_string();
            }
            if let Some(s) = e.height().or(e.width()) {
                b.text_height_nm = s;
            }
        }
        for d in dr.dimensions.iter_mut().filter(|d| dims.contains(d.id.as_str())) {
            if e.layer_defaults {
                // `PCB_TEXT::StyleFromSettings`, then `PCB_DIMENSION_BASE::StyleFromSettings`.
                d.text_size_nm = text_size(&defaults, &d.layer);
                d.text_thickness_nm = Some(text_thickness(&defaults, &d.layer));
                d.stroke_width = line_thickness(&defaults, &d.layer);
                d.units = dim_style.units;
                d.units_format = dim_style.units_format;
                d.precision = dim_style.precision;
                d.suppress_trailing_zeros = dim_style.suppress_trailing_zeros;
                d.text_position = dim_style.text_position;
                d.keep_text_aligned = dim_style.keep_text_aligned;
                continue;
            }
            if let Some(l) = e.layer {
                d.layer = l.to_string();
            }
            if let Some(w) = e.line_width {
                d.stroke_width = w;
            }
            // The label has one size here: the height (or the width when only that is given).
            if let Some(s) = e.height().or(e.width()) {
                d.text_size_nm = s;
            }
            if e.rows.auto_thickness {
                d.text_thickness_nm = Some(auto_pen_nm(d.text_size_nm, false));
            } else if let Some(th) = e.text_thickness {
                d.text_thickness_nm = Some(th);
            }
        }
        Ok(())
    }

    /// The footprints' part of `Cmd::EditTextAndGraphics` (see the module doc): the fields' layouts in their footprint's edit, the texts and graphics of
    /// the art in the instance's own copy of its footprint.
    fn edit_footprint_items(&mut self, e: &GlobalTextEdit<'_>) {
        let items = e.footprint;
        if items.is_empty() {
            return;
        }
        let defaults = self.model.board.text_graphics;
        let face = e.rows.face();
        // `FOOTPRINT::GetCenter` of each footprint a centred text belongs to, worked out on the board as it is.
        let mut centers: BTreeMap<String, Point> = BTreeMap::new();
        let mut center_of = |board: &Board<'_>, reference: &str, pose: &FootprintInstance| -> Option<Point> {
            if let Some(c) = centers.get(reference) {
                return Some(*c);
            }
            let part = board.model.part(reference)?;
            let footprint = board.model.footprint_of(part);
            let extra = board.design.drawings.as_ref().and_then(|d| d.footprint_extras.iter().find(|x| x.id == reference));
            let c = box_center(board.mc_footprint_box(pose, part, footprint.as_ref(), extra));
            centers.insert(reference.to_string(), c);
            Some(c)
        };

        // ---- fields
        for id in &items.field_ids {
            let Some((reference, name)) = fp_edit::parse_field_id(id) else { continue };
            let Some(pose) = self.pose_of(reference).cloned() else { continue };
            let Some(part) = self.model.part(reference) else { continue };
            let Some(footprint) = self.model.footprint_of(part) else { continue };
            let current = match name {
                REFERENCE => Some(fp_edit::effective_reference(&self.design, &pose, &footprint)),
                VALUE => Some(fp_edit::effective_value(&self.design, &pose)),
                other => self.design.footprint_edit(reference).and_then(|x| x.fields.iter().find(|f| f.name == other)).map(|f| f.layout.clone()),
            };
            let Some(mut l) = current else { continue };
            let before = l.clone();
            if e.layer_defaults {
                let layer = l.layer.clone();
                style_text_from_settings(&mut l, &defaults, &layer);
            } else {
                if let Some(layer) = e.layer {
                    l.layer = layer.to_string();
                }
                apply_text_rows(&mut l, e);
                if let Some(f) = &face {
                    l.face = f.clone();
                }
                if e.rows.center_on_footprint {
                    if let Some(c) = center_of(self, reference, &pose) {
                        let angle = l.board_angle(&pose);
                        l.set_from_board(&pose, c, angle);
                    }
                }
                // `PCB_FIELD::SetVisible`
                if let Some(v) = e.rows.visible {
                    l.visible = v;
                }
            }
            if l == before {
                continue;
            }
            let slot = self.edit_slot(reference);
            match name {
                REFERENCE => slot.reference = Some(l),
                VALUE => slot.value = Some(l),
                other => {
                    if let Some(f) = slot.fields.iter_mut().find(|f| f.name == other) {
                        f.layout = l;
                    }
                }
            }
        }

        // ---- the art: the texts and graphics of each footprint named, in its own copy
        let mut by_footprint: BTreeMap<&str, (BTreeSet<usize>, BTreeSet<usize>)> = BTreeMap::new();
        for (r, k) in items.text_ids.iter().filter_map(|id| parse_fp_item_id(id, 't')) {
            by_footprint.entry(r).or_default().0.insert(k);
        }
        for (r, k) in items.shape_ids.iter().filter_map(|id| parse_fp_item_id(id, 'g')) {
            by_footprint.entry(r).or_default().1.insert(k);
        }
        for (reference, (text_ks, shape_ks)) in by_footprint {
            let Some(pose) = self.pose_of(reference).cloned() else { continue };
            let Some(part) = self.model.part(reference) else { continue };
            let Some(before) = self.unedited_footprint(part) else { continue };
            let name = self.design.footprint_assignment(reference).map(|a| a.footprint.clone()).unwrap_or_else(|| crate::fp_exchange::footprint_name_of(part));
            let center = if e.rows.center_on_footprint && !e.layer_defaults && !text_ks.is_empty() { center_of(self, reference, &pose) } else { None };
            let mut fp = before.clone();
            for k in text_ks {
                let Some(t) = fp.art.texts.get_mut(k) else { continue };
                let board_layer = layer_on_side(&t.layout.layer, pose.side);
                if e.layer_defaults {
                    style_text_from_settings(&mut t.layout, &defaults, &board_layer);
                    continue;
                }
                if let Some(layer) = e.layer {
                    t.layout.layer = layer_on_side(layer, pose.side);
                }
                apply_text_rows(&mut t.layout, e);
                if let Some(f) = &face {
                    t.face = f.clone();
                }
                if let Some(c) = center {
                    let angle = t.layout.board_angle(&pose);
                    t.layout.set_from_board(&pose, c, angle);
                }
            }
            for k in shape_ks {
                let Some(s) = fp.art.shapes.get_mut(k) else { continue };
                if e.layer_defaults {
                    // `PCB_SHAPE::StyleFromSettings`: the stroke of the layer class of the layer it is on, on the board.
                    s.shape.set_stroke_width(line_thickness(&defaults, &layer_on_side(s.layer(), pose.side)));
                    continue;
                }
                if let Some(layer) = e.layer {
                    // `PCB_SHAPE::SetLayer`: one layer, with the mask opening of a copper graphic that had one (`GetLayerSet`).
                    let own = layer_on_side(layer, pose.side);
                    let had_mask = s.layers.iter().any(|l| l.ends_with(".Mask"));
                    s.layers = match own.as_str() {
                        "F.Cu" if had_mask => vec![own.clone(), "F.Mask".into()],
                        "B.Cu" if had_mask => vec![own.clone(), "B.Mask".into()],
                        _ => Vec::new(),
                    };
                    s.shape.set_layer(own);
                }
                if let Some(w) = e.line_width {
                    s.shape.set_stroke_width(w);
                }
            }
            if fp != before {
                self.drawings_mut().set_footprint_assignment(FootprintAssignment { reference: reference.to_string(), footprint: name, definition: Some(fp) });
            }
        }
    }

    /// The via rows of `Cmd::EditTracksAndVias` that are not sizes: "Via annular rings:" (`v->Padstack().SetUnconnectedLayerMode( .. )`, [`ViaExtra::set_unconnected_layers`]) and
    /// "Via protection features:" (`setViaConfiguration( v, preset )`), on every via among `ids`; a via left with KiCad's defaults keeps no extra. The Rust DRC's
    /// tenting table follows the tenting, as `set_via_props` keeps it.
    pub(crate) fn edit_via_padstacks(&mut self, ids: &[String], unconnected: Option<UnconnectedLayers>, protection: Option<Ipc4761Preset>) -> Result<(), Vec<CheckResult>> {
        if unconnected.is_none() && protection.is_none() {
            return Ok(());
        }
        let id_set: BTreeSet<&str> = ids.iter().map(String::as_str).collect();
        let vias: Vec<(String, Point, String)> = self.design.routing.as_ref().map(|rt| rt.vias.iter().filter(|v| id_set.contains(v.id.as_str())).map(|v| (v.id.clone(), v.at, v.net.clone())).collect()).unwrap_or_default();
        let dr = self.drawings_mut();
        for (id, at, net) in vias {
            let mut extra = dr.via_extra(&id).cloned().unwrap_or(ViaExtra { id: id.clone(), ..Default::default() });
            if let Some(mode) = unconnected {
                extra.set_unconnected_layers(mode);
            }
            if let Some(preset) = protection {
                let mut props = extra.props();
                preset.apply(&mut props);
                extra.set_props(&props);
                dr.via_tenting.retain(|t| !(t.at == at && t.net == net));
                if props.front_tenting.is_some() || props.back_tenting.is_some() {
                    dr.via_tenting.push(eda_model::ir::ViaTenting { at, net: net.clone(), front: props.front_tenting, back: props.back_tenting });
                }
            }
            match (dr.via_extras.iter().position(|x| x.id == id), extra.is_default()) {
                (Some(i), true) => {
                    dr.via_extras.remove(i);
                }
                (Some(i), false) => dr.via_extras[i] = extra,
                (None, false) => dr.via_extras.push(extra),
                (None, true) => {}
            }
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------------------------------------------------------------------- IPC-4761

/// `VIA_PROTECTION_UI_MIXIN::IPC4761_SURFACE`: what a feature is on the two sides.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Ipc4761Surface {
    FromBoard,
    None,
    Front,
    Back,
    Both,
}

/// `VIA_PROTECTION_UI_MIXIN::IPC4761_DRILL`: a feature of the hole.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Ipc4761Drill {
    FromBoard,
    NotSet,
    Set,
}

/// `VIA_PROTECTION_UI_MIXIN::IPC4761_PRESET` but `CUSTOM` and `END` (the global dialog lists the presets before `CUSTOM`): the via protection features of IPC-4761.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Ipc4761Preset {
    FromBoard,
    None,
    Ia,
    Ib,
    IaInverted,
    Iia,
    Iib,
    IiaInverted,
    Iiia,
    Iiib,
    IiiaInverted,
    Iva,
    Ivb,
    IvaInverted,
    V,
    Via,
    Vib,
    ViaInverted,
    Vii,
}

impl Ipc4761Preset {
    /// Every preset, in `magic_enum::enum_values<IPC4761_PRESET>()`'s order (the dialog's list).
    pub const ALL: [Ipc4761Preset; 19] = [
        Ipc4761Preset::FromBoard,
        Ipc4761Preset::None,
        Ipc4761Preset::Ia,
        Ipc4761Preset::Ib,
        Ipc4761Preset::IaInverted,
        Ipc4761Preset::Iia,
        Ipc4761Preset::Iib,
        Ipc4761Preset::IiaInverted,
        Ipc4761Preset::Iiia,
        Ipc4761Preset::Iiib,
        Ipc4761Preset::IiiaInverted,
        Ipc4761Preset::Iva,
        Ipc4761Preset::Ivb,
        Ipc4761Preset::IvaInverted,
        Ipc4761Preset::V,
        Ipc4761Preset::Via,
        Ipc4761Preset::Vib,
        Ipc4761Preset::ViaInverted,
        Ipc4761Preset::Vii,
    ];

    /// `m_IPC4761Names`.
    pub fn name(self) -> &'static str {
        match self {
            Ipc4761Preset::FromBoard => "From rules",
            Ipc4761Preset::None => "None",
            Ipc4761Preset::Ia => "Type I-a (tented top)",
            Ipc4761Preset::Ib => "Type I-b (tented both sides)",
            Ipc4761Preset::IaInverted => "Type I-a (tented bottom)",
            Ipc4761Preset::Iia => "Type II-a (covered and tented top)",
            Ipc4761Preset::Iib => "Type II-b (covered and tented both sides)",
            Ipc4761Preset::IiaInverted => "Type II-a (covered and tented bottom)",
            Ipc4761Preset::Iiia => "Type III-a (plugged top)",
            Ipc4761Preset::Iiib => "Type III-b (plugged both sides)",
            Ipc4761Preset::IiiaInverted => "Type III-a (plugged bottom)",
            Ipc4761Preset::Iva => "Type IV-a (plugged and tented top)",
            Ipc4761Preset::Ivb => "Type IV-b (plugged and tented both sides)",
            Ipc4761Preset::IvaInverted => "Type IV-a (plugged and tented bottom)",
            Ipc4761Preset::V => "Type V (filled )",
            Ipc4761Preset::Via => "Type VI-a (filled and tented top)",
            Ipc4761Preset::Vib => "Type VI-b (filled and tented both sides)",
            Ipc4761Preset::ViaInverted => "Type VI-a (filled and tented bottom)",
            Ipc4761Preset::Vii => "Type VII (filled and capped)",
        }
    }

    /// `m_IPC4761Presets`: (tent, cover, plug, fill, cap); a field the table leaves out is `NONE` / `NOT_SET` (`IPC4761_CONFIGURATION`'s defaults).
    fn configuration(self) -> (Ipc4761Surface, Ipc4761Surface, Ipc4761Surface, Ipc4761Drill, Ipc4761Drill) {
        use Ipc4761Drill as D;
        use Ipc4761Surface as S;
        let n = (S::None, S::None, S::None, D::NotSet, D::NotSet);
        match self {
            Ipc4761Preset::FromBoard => (S::FromBoard, S::FromBoard, S::FromBoard, D::FromBoard, D::FromBoard),
            Ipc4761Preset::None => n,
            Ipc4761Preset::Ia => (S::Front, n.1, n.2, n.3, n.4),
            Ipc4761Preset::Ib => (S::Both, n.1, n.2, n.3, n.4),
            Ipc4761Preset::IaInverted => (S::Back, n.1, n.2, n.3, n.4),
            Ipc4761Preset::Iia => (S::Front, S::Front, n.2, n.3, n.4),
            Ipc4761Preset::Iib => (S::Both, S::Both, n.2, n.3, n.4),
            Ipc4761Preset::IiaInverted => (S::Back, S::Back, n.2, n.3, n.4),
            Ipc4761Preset::Iiia => (n.0, n.1, S::Front, n.3, n.4),
            Ipc4761Preset::Iiib => (n.0, n.1, S::Both, n.3, n.4),
            Ipc4761Preset::IiiaInverted => (n.0, n.1, S::Back, n.3, n.4),
            Ipc4761Preset::Iva => (S::Front, n.1, S::Front, n.3, n.4),
            Ipc4761Preset::Ivb => (S::Both, n.1, S::Both, n.3, n.4),
            Ipc4761Preset::IvaInverted => (S::Back, n.1, S::Back, n.3, n.4),
            Ipc4761Preset::V => (n.0, n.1, n.2, D::Set, n.4),
            Ipc4761Preset::Via => (S::Front, n.1, n.2, D::Set, n.4),
            Ipc4761Preset::Vib => (S::Both, n.1, n.2, D::Set, n.4),
            Ipc4761Preset::ViaInverted => (S::Back, n.1, n.2, D::Set, n.4),
            Ipc4761Preset::Vii => (n.0, n.1, n.2, D::Set, D::Set),
        }
    }

    /// `setViaConfiguration`: the tenting (`has_solder_mask`), plugging and covering of the two outer layers, then the filling and the capping of the hole.
    pub fn apply(self, p: &mut ViaProps) {
        fn surface(front: &mut Option<bool>, back: &mut Option<bool>, s: Ipc4761Surface) {
            (*front, *back) = match s {
                Ipc4761Surface::FromBoard => (None, None),
                Ipc4761Surface::None => (Some(false), Some(false)),
                Ipc4761Surface::Front => (Some(true), Some(false)),
                Ipc4761Surface::Back => (Some(false), Some(true)),
                Ipc4761Surface::Both => (Some(true), Some(true)),
            };
        }
        fn drill(d: &mut Option<bool>, v: Ipc4761Drill) {
            *d = match v {
                Ipc4761Drill::FromBoard => None,
                Ipc4761Drill::NotSet => Some(false),
                Ipc4761Drill::Set => Some(true),
            };
        }
        let (tent, cover, plug, fill, cap) = self.configuration();
        surface(&mut p.front_tenting, &mut p.back_tenting, tent);
        surface(&mut p.front_plugging, &mut p.back_plugging, plug);
        surface(&mut p.front_covering, &mut p.back_covering, cover);
        drill(&mut p.filling, fill);
        drill(&mut p.capping, cap);
    }
}
