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
//!   * `PCB_TEXT::StyleFromSettings`: the text size and the text thickness are the layer class's (the model draws a text square, so the size is the
//!     class's text height; italic and upright have nothing to land on in a text of this model);
//!   * `PCB_DIMENSION_BASE::StyleFromSettings`: the text as above, `SetLineThickness( GetLineThickness( layer ) )`, and the units, units format,
//!     precision, zero suppression, text position and "keep text aligned" of the board's dimension settings.
//!
//! The edges and courtyard classes have a line thickness in Board Setup but no text cells (`ROW_EDGES` and `ROW_COURTYARD` leave them disabled), so a
//! text on `Edge.Cuts` or a courtyard layer keeps `BOARD_DESIGN_SETTINGS`' own constants (`DEFAULT_TEXT_SIZE` 1 mm, `DEFAULT_TEXT_WIDTH` 0.15 mm).

use super::Board;
use eda_model::ir::Nm;
use eda_model::rules::TextGraphicsDefaults;
use eda_model::CheckResult;
use std::collections::BTreeSet;

#[cfg(test)]
pub use eda_model::rules::{layer_class, LayerClass};

/// `BOARD_DESIGN_SETTINGS::GetLineThickness( aLayer )` (`TextGraphicsDefaults::line_thickness_nm`).
pub fn line_thickness(d: &TextGraphicsDefaults, layer: &str) -> Nm {
    d.line_thickness_nm(layer)
}

/// `BOARD_DESIGN_SETTINGS::GetTextSize( aLayer )`, as the one size a text of this model has (its height).
pub fn text_size(d: &TextGraphicsDefaults, layer: &str) -> Nm {
    d.text_size_nm(layer).1
}

/// `BOARD_DESIGN_SETTINGS::GetTextThickness( aLayer )`.
pub fn text_thickness(d: &TextGraphicsDefaults, layer: &str) -> Nm {
    d.text_thickness_nm(layer)
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
}

fn refuse(check: &str, message: &str) -> Result<(), Vec<CheckResult>> {
    Err(vec![CheckResult::fail(check, "edit_text_and_graphics", message)])
}

impl Board<'_> {
    /// `Cmd::EditTextAndGraphics`. Unknown ids among the lists are tolerated (they match nothing), the same convention `CommitRoute` set for a list the
    /// caller computed; refused when all three lists are empty (nothing named at all, almost certainly a caller bug) or an explicit value is out of
    /// range.
    pub(crate) fn edit_text_and_graphics(&mut self, e: &GlobalTextEdit<'_>) -> Result<(), Vec<CheckResult>> {
        if e.shape_ids.is_empty() && e.text_ids.is_empty() && e.dimension_ids.is_empty() {
            return refuse("ops_bad_global_edit", "no items given");
        }
        if !e.layer_defaults {
            // Not `known_layer` -- that validates against the board's *copper* layer list only, and silkscreen/fab/edge layers (where a shape or text
            // normally lives) are deliberately not on it. Same "just non-empty" convention `edit_shape`/`edit_text` already use.
            if matches!(e.layer, Some(l) if l.is_empty()) {
                return refuse("ops_bad_shape", "a layer, if given, cannot be empty");
            }
            if matches!(e.line_width, Some(w) if w <= 0) {
                return refuse("ops_bad_shape", "line width must be positive");
            }
            if matches!(e.text_size, Some(s) if s <= 0) {
                return refuse("ops_bad_text", "text size must be positive");
            }
            if matches!(e.text_thickness, Some(t) if t <= 0) {
                return refuse("ops_bad_text", "text thickness must be positive");
            }
        }

        let shapes: BTreeSet<&str> = e.shape_ids.iter().map(String::as_str).collect();
        let texts: BTreeSet<&str> = e.text_ids.iter().map(String::as_str).collect();
        let dims: BTreeSet<&str> = e.dimension_ids.iter().map(String::as_str).collect();
        let defaults = self.model.board.text_graphics;
        let Some(dr) = self.design.drawings.as_mut() else { return Ok(()) };
        let dim_style = dr.dimension_settings;

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
        for t in dr.texts.iter_mut().filter(|t| texts.contains(t.id.as_str())) {
            if e.layer_defaults {
                t.size_nm = text_size(&defaults, &t.layer);
                t.stroke_width = text_thickness(&defaults, &t.layer);
                continue;
            }
            if let Some(l) = e.layer {
                t.layer = l.to_string();
            }
            if let Some(s) = e.text_size {
                t.size_nm = s;
            }
            if let Some(th) = e.text_thickness {
                t.stroke_width = th;
            }
        }
        // A size or a stroke width set here replaces the glyph width and the automatic stroke width an imported text kept (`DrawingsSection::text_extras`).
        let (new_size, new_stroke) = (e.layer_defaults || e.text_size.is_some(), e.layer_defaults || e.text_thickness.is_some());
        for x in dr.text_extras.iter_mut().filter(|x| texts.contains(x.id.as_str())) {
            if new_size {
                x.width_nm = None;
            }
            if new_stroke {
                x.auto_thickness = false;
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
            if let Some(s) = e.text_size {
                d.text_size_nm = s;
            }
            if let Some(th) = e.text_thickness {
                d.text_thickness_nm = Some(th);
            }
        }
        Ok(())
    }
}
