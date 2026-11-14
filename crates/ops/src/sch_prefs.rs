//! The person's schematic preferences a verb reads where KiCad reads `EESCHEMA_SETTINGS` (eeschema.json): the colours a new sheet is drawn with,
//! Preferences > Schematic Editor > Editing Options > Defaults for New Objects ("Sheet border:", "Sheet background:", `drawing.default_sheet_border_color`,
//! `drawing.default_sheet_background_color`, `COLOR4D::UNSPECIFIED` by default). `SCH_DRAWING_TOOLS::DrawSheet` gives every sheet it draws -- an empty one, one from a
//! file, one from a design block -- `SetBorderColor( cfg->m_Drawing.default_sheet_border_color )` and `SetBackgroundColor( .. default_sheet_background_color )`.
//!
//! The studio keeps the preferences in the browser; the request that runs a command carries them (`/api/cmd`'s `sheet_colors`), and they are in force on its
//! thread while it runs ([`scoped_new_sheet_colors`]), as `eda_engine::fields::AutoplacePrefs` are for Symbol Field Automatic Placement. A command run with none
//! (the CLI, a test) draws a sheet with KiCad's default, no colour of its own.

use eda_model::sch_extras::SchColor;
use std::cell::Cell;

/// What a new sheet's border and background are: `None` is `COLOR4D::UNSPECIFIED` (the layer's colour, `LAYER_SHEET` / `LAYER_SHEET_BACKGROUND`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct NewSheetColors {
    pub border: Option<SchColor>,
    pub background: Option<SchColor>,
}

impl NewSheetColors {
    /// The request's `sheet_colors` (`{ "border": { r, g, b, a } | null, "background": .. }`); anything else is no colour.
    pub fn from_json(v: Option<&serde_json::Value>) -> NewSheetColors {
        let color = |k: &str| v.and_then(|o| o.get(k)).cloned().and_then(|c| serde_json::from_value::<SchColor>(c).ok());
        NewSheetColors { border: color("border"), background: color("background") }
    }
}

thread_local! {
    static NEW_SHEET_COLORS: Cell<NewSheetColors> = const { Cell::new(NewSheetColors { border: None, background: None }) };
}

/// The colours a sheet drawn on this thread now gets.
pub fn new_sheet_colors() -> NewSheetColors {
    NEW_SHEET_COLORS.with(|c| c.get())
}

/// `colors` in force on this thread until the scope returned is dropped, which puts back what was in force before.
#[must_use = "the colours are in force only while the scope lives"]
pub fn scoped_new_sheet_colors(colors: NewSheetColors) -> NewSheetColorsScope {
    NewSheetColorsScope(NEW_SHEET_COLORS.with(|c| c.replace(colors)))
}

/// See [`scoped_new_sheet_colors`].
pub struct NewSheetColorsScope(NewSheetColors);

impl Drop for NewSheetColorsScope {
    fn drop(&mut self) {
        let before = self.0;
        NEW_SHEET_COLORS.with(|c| c.set(before));
    }
}

/// Give the sheet `id` of `sch` the colours in force (`DrawSheet`'s `SetBorderColor` / `SetBackgroundColor`): with none set, `COLOR4D::UNSPECIFIED` -- no colour
/// of its own, the layers' (KiCad's default theme draws no background) -- where a sheet with no entry has the writer's old look (`SheetProps::legacy`, a
/// pale yellow background).
pub(crate) fn color_new_sheet(sch: &mut eda_model::ir::SchematicSection, id: &str) {
    let colors = new_sheet_colors();
    let mut p = sch.extras.sheet_props_of(id);
    p.border_color = colors.border;
    p.fill_color = colors.background;
    sch.extras.set_sheet_props(id, p);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_request_names_the_colours_and_none_is_the_layers() {
        let c = NewSheetColors::from_json(Some(&serde_json::json!({ "border": { "r": 200, "g": 0, "b": 0, "a": 255 }, "background": null })));
        assert_eq!(c, NewSheetColors { border: Some(SchColor { r: 200, g: 0, b: 0, a: 255 }), background: None });
        assert_eq!(NewSheetColors::from_json(None), NewSheetColors::default());
        {
            let _scope = scoped_new_sheet_colors(c);
            assert_eq!(new_sheet_colors(), c);
        }
        assert_eq!(new_sheet_colors(), NewSheetColors::default(), "put back");
    }
}
