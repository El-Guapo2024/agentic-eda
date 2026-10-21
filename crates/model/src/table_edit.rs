//! What the Table Properties and Table Cell Properties dialogs and the Properties panel do to a table, in one place for both editors: a board's
//! `DIALOG_TABLE_PROPERTIES` / `DIALOG_TABLECELL_PROPERTIES` (`pcbnew/dialogs/dialog_table{,cell}_properties.cpp`) and a sheet's (`eeschema/dialogs/..`) set the
//! same fields on the same kind of table, and the `PROPERTY_MANAGER` rows of `PCB_TABLE_DESC` / `PCB_TABLECELL_DESC` / `SCH_TABLE_DESC` /
//! `SCH_TABLECELL_DESC` are the same two lists.
//!
//! An edit names only what it changes (every field is an `Option`): a dialog that did not touch a field leaves it as it was, which is also how the
//! dialog handles a field several selected cells disagree on (`INDETERMINATE_STATE`).

use crate::ir::{Point, TextJustify, TextVAlign, Um};
use crate::table::{Table, TableColor, TableLineStyle, TableText};
use serde::{Deserialize, Serialize};

/// `GetPenSizeForNormal`: an eighth of the glyph, `KiROUND`ed.
pub fn pen_for_normal(size: Um) -> Um {
    (size as f64 / 8.0).round() as Um
}

/// `GetPenSizeForBold`: a fifth of the glyph.
pub fn pen_for_bold(size: Um) -> Um {
    (size as f64 / 5.0).round() as Um
}

impl TableText {
    /// `EDA_TEXT::SetBold` for a stroke font: boldness is the pen, so turning it on gives the bold pen of the smaller glyph dimension and turning it off the
    /// normal one (KiCad restores the width it had before the text was made bold when it remembers one; this model does not keep it).
    pub fn set_bold(&mut self, bold: bool) {
        if self.bold != bold {
            let size = self.width_um.min(self.height_um);
            self.thickness_um = if bold { pen_for_bold(size) } else { pen_for_normal(size) };
        }
        self.bold = bold;
    }
}

/// The fields of `DIALOG_TABLECELL_PROPERTIES` and the cell rows of the Properties panel that a cell has. A field left out is left alone.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CellEdit {
    #[serde(default)]
    pub text: Option<String>,
    #[serde(default)]
    pub bold: Option<bool>,
    #[serde(default)]
    pub italic: Option<bool>,
    /// The font's name; an empty one is the default font.
    #[serde(default)]
    pub face: Option<String>,
    #[serde(default)]
    pub width_um: Option<Um>,
    #[serde(default)]
    pub height_um: Option<Um>,
    /// 0 is KiCad's automatic stroke width (`SetAutoThickness( true )`).
    #[serde(default)]
    pub thickness_um: Option<Um>,
    #[serde(default)]
    pub h_align: Option<TextJustify>,
    #[serde(default)]
    pub v_align: Option<TextVAlign>,
    /// left, top, right, bottom.
    #[serde(default)]
    pub margins: [Option<Um>; 4],
    /// `PCB_TABLECELL::SetColumnWidth` (the Properties panel's "Column Width"): the width of the cell's column.
    #[serde(default)]
    pub col_width_um: Option<Um>,
    /// `SetRowHeight` ("Row Height").
    #[serde(default)]
    pub row_height_um: Option<Um>,
}

impl CellEdit {
    /// Does this change nothing?
    pub fn is_empty(&self) -> bool {
        *self == CellEdit::default()
    }
}

/// The border half of `DIALOG_TABLE_PROPERTIES`: the two switches, the width and the style. With neither switch on the stroke is not drawn at all (width -1), as the
/// dialog sets it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BorderEdit {
    pub external: bool,
    pub header: bool,
    pub width_um: Um,
    #[serde(default)]
    pub style: TableLineStyle,
}

/// The separators half: rows, columns, width and style.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SeparatorsEdit {
    pub rows: bool,
    pub cols: bool,
    pub width_um: Um,
    #[serde(default)]
    pub style: TableLineStyle,
}

/// The rows of the Properties panel's "Table Properties" group (`PCB_TABLE_DESC`, `SCH_TABLE_DESC`): each setter (`SetStrokeExternal`, `SetBorderWidth`,
/// `SetSeparatorsStyle` ...) changes one field of the border or of the separators and nothing else -- unlike the dialog's `border` and `separators`, which set a whole
/// half at once and give a stroke nothing draws the width -1. A colour with no alpha is `COLOR4D::UNSPECIFIED`, which clears the colour.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StrokeProps {
    /// "External Border".
    #[serde(default)]
    pub external: Option<bool>,
    /// "Header Border".
    #[serde(default)]
    pub header: Option<bool>,
    /// "Border Width".
    #[serde(default)]
    pub border_width_um: Option<Um>,
    /// "Border Style".
    #[serde(default)]
    pub border_style: Option<TableLineStyle>,
    /// "Border Color".
    #[serde(default)]
    pub border_color: Option<TableColor>,
    /// "Row Separators".
    #[serde(default)]
    pub rows: Option<bool>,
    /// "Cell Separators".
    #[serde(default)]
    pub cols: Option<bool>,
    /// "Separators Width".
    #[serde(default)]
    pub separators_width_um: Option<Um>,
    /// "Separators Style".
    #[serde(default)]
    pub separators_style: Option<TableLineStyle>,
    /// "Separators Color".
    #[serde(default)]
    pub separators_color: Option<TableColor>,
}

impl StrokeProps {
    fn apply(&self, t: &mut Table) {
        if let Some(v) = self.external {
            t.border.external = v;
        }
        if let Some(v) = self.header {
            t.border.header = v;
        }
        if let Some(v) = self.border_width_um {
            t.border.stroke.width_um = v;
        }
        if let Some(v) = self.border_style {
            t.border.stroke.style = v;
        }
        if let Some(c) = self.border_color {
            t.border.stroke.color = (c.a != 0).then_some(c);
        }
        if let Some(v) = self.rows {
            t.separators.rows = v;
        }
        if let Some(v) = self.cols {
            t.separators.cols = v;
        }
        if let Some(v) = self.separators_width_um {
            t.separators.stroke.width_um = v;
        }
        if let Some(v) = self.separators_style {
            t.separators.stroke.style = v;
        }
        if let Some(c) = self.separators_color {
            t.separators.stroke.color = (c.a != 0).then_some(c);
        }
    }
}

/// What a table dialog or the Properties panel changes in a table itself (not its layer or lock, which belong to the editor).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TablePatch {
    /// "Start X" / "Start Y": where the first cell's top-left corner is.
    #[serde(default)]
    pub at: Option<Point>,
    #[serde(default)]
    pub border: Option<BorderEdit>,
    #[serde(default)]
    pub separators: Option<SeparatorsEdit>,
    /// The rows of the Properties panel: single fields of the border and of the separators, applied after `border` and `separators`.
    #[serde(default)]
    pub props: Option<StrokeProps>,
    /// The grid of the dialog: the text of every cell, row by row. A cell another one covers has no text whatever the grid says.
    #[serde(default)]
    pub texts: Option<Vec<Vec<String>>>,
}

fn style_or_solid(s: TableLineStyle) -> TableLineStyle {
    // The dialog's combo has the five styles; a `default` stroke is shown as the first, solid.
    if s == TableLineStyle::Default {
        TableLineStyle::Solid
    } else {
        s
    }
}

/// `DIALOG_TABLE_PROPERTIES::TransferDataFromWindow` for the parts that are the table's: the cell text, the border and the separators.
pub fn patch_table(t: &mut Table, p: &TablePatch) -> Result<(), String> {
    if let Some(at) = p.at {
        t.at = at;
    }
    if let Some(texts) = &p.texts {
        if texts.len() != t.row_count() || texts.iter().any(|r| r.len() != t.col_count()) {
            return Err(format!("the grid has {} rows of {} cells and the table {} of {}", texts.len(), texts.first().map_or(0, Vec::len), t.row_count(), t.col_count()));
        }
        for (row, line) in texts.iter().enumerate() {
            for (col, text) in line.iter().enumerate() {
                if let Some(cell) = t.cell_mut(row, col) {
                    cell.text = if cell.is_covered() { String::new() } else { text.clone() };
                }
            }
        }
    }
    if let Some(b) = p.border {
        t.border.external = b.external;
        t.border.header = b.header;
        t.border.stroke.width_um = if b.external || b.header { b.width_um.max(0) } else { -1 };
        t.border.stroke.style = style_or_solid(b.style);
    }
    if let Some(s) = p.separators {
        t.separators.rows = s.rows;
        t.separators.cols = s.cols;
        t.separators.stroke.width_um = if s.rows || s.cols { s.width_um.max(0) } else { -1 };
        t.separators.stroke.style = style_or_solid(s.style);
    }
    if let Some(props) = &p.props {
        props.apply(t);
    }
    Ok(())
}

/// `DIALOG_TABLECELL_PROPERTIES::TransferDataFromWindow` (and the Properties panel's setters) on each of `cells`.
pub fn edit_cells(t: &mut Table, cells: &[(usize, usize)], e: &CellEdit) -> Result<(), String> {
    if cells.is_empty() {
        return Err("no cells given".into());
    }
    for &(row, col) in cells {
        if t.cell(row, col).is_none() {
            return Err(format!("the table has no cell at row {row}, column {col}"));
        }
    }
    for &(row, col) in cells {
        {
            let cell = t.cell_mut(row, col).expect("checked above");
            if let Some(text) = &e.text {
                cell.text = text.replace('\r', "");
            }
            let st = &mut cell.style;
            if let Some(b) = e.bold {
                st.set_bold(b);
            }
            if let Some(i) = e.italic {
                st.italic = i;
            }
            if let Some(f) = &e.face {
                st.face = (!f.is_empty()).then(|| f.clone());
            }
            if let Some(w) = e.width_um {
                st.width_um = w;
            }
            if let Some(h) = e.height_um {
                st.height_um = h;
            }
            if let Some(th) = e.thickness_um {
                st.thickness_um = th.max(0);
            }
            if let Some(h) = e.h_align {
                st.h_align = h;
            }
            if let Some(v) = e.v_align {
                st.v_align = v;
            }
            for (i, m) in e.margins.iter().enumerate() {
                if let Some(m) = m {
                    cell.margins[i] = *m;
                }
            }
        }
        if let Some(w) = e.col_width_um {
            if let Some(slot) = t.col_widths.get_mut(col) {
                *slot = w.max(0);
            }
        }
        if let Some(h) = e.row_height_um {
            if let Some(slot) = t.row_heights.get_mut(row) {
                *slot = h.max(0);
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::table::{TableBorder, TableCell, TableSeparators, TableStroke};

    fn table() -> Table {
        let mut cells: Vec<TableCell> = (0..4).map(|_| TableCell::pcb_new(false)).collect();
        for (i, c) in cells.iter_mut().enumerate() {
            c.text = format!("c{i}");
        }
        Table {
            id: "tbl_t".into(),
            uuid: String::new(),
            layer: "F.SilkS".into(),
            at: Point { x: 0, y: 0 },
            angle: 0,
            col_widths: vec![10_000, 20_000],
            row_heights: vec![5_000, 6_000],
            cells,
            border: TableBorder { external: true, header: true, stroke: TableStroke::solid(100) },
            separators: TableSeparators { rows: true, cols: true, stroke: TableStroke::solid(100) },
        }
    }

    #[test]
    fn the_borders_dialog_sets_a_width_only_while_a_border_is_on() {
        let mut t = table();
        patch_table(&mut t, &TablePatch { border: Some(BorderEdit { external: false, header: false, width_um: 300, style: TableLineStyle::Dash }), ..TablePatch::default() }).unwrap();
        assert_eq!((t.border.external, t.border.header, t.border.stroke.width_um, t.border.stroke.style), (false, false, -1, TableLineStyle::Dash));
        patch_table(&mut t, &TablePatch { border: Some(BorderEdit { external: true, header: false, width_um: -5, style: TableLineStyle::Default }), ..TablePatch::default() }).unwrap();
        assert_eq!((t.border.stroke.width_um, t.border.stroke.style), (0, TableLineStyle::Solid), "a negative width is 0 and the default style shows as solid");
        patch_table(&mut t, &TablePatch { separators: Some(SeparatorsEdit { rows: false, cols: true, width_um: 150, style: TableLineStyle::Dot }), ..TablePatch::default() }).unwrap();
        assert_eq!((t.separators.rows, t.separators.cols, t.separators.stroke.width_um), (false, true, 150));
    }

    #[test]
    fn a_properties_row_changes_one_field_of_a_stroke_and_leaves_the_rest() {
        let mut t = table();
        t.border.stroke.width_um = -1;
        // `SetStrokeExternal` alone: the width stays -1 (nothing draws until the width is set too), the dialog's rule does not apply.
        patch_table(&mut t, &TablePatch { props: Some(StrokeProps { external: Some(false), ..StrokeProps::default() }), ..TablePatch::default() }).unwrap();
        assert_eq!((t.border.external, t.border.header, t.border.stroke.width_um), (false, true, -1));
        let red = TableColor { r: 255, g: 0, b: 0, a: 255 };
        let props = StrokeProps { border_width_um: Some(250), border_style: Some(TableLineStyle::Dot), border_color: Some(red), cols: Some(false), separators_width_um: Some(80), ..StrokeProps::default() };
        patch_table(&mut t, &TablePatch { props: Some(props), ..TablePatch::default() }).unwrap();
        assert_eq!((t.border.stroke.width_um, t.border.stroke.style, t.border.stroke.color), (250, TableLineStyle::Dot, Some(red)));
        assert_eq!((t.separators.rows, t.separators.cols, t.separators.stroke.width_um, t.separators.stroke.style), (true, false, 80, TableLineStyle::Solid));
        // An unspecified colour (no alpha) clears it.
        let clear = StrokeProps { border_color: Some(TableColor { r: 0, g: 0, b: 0, a: 0 }), ..StrokeProps::default() };
        patch_table(&mut t, &TablePatch { props: Some(clear), ..TablePatch::default() }).unwrap();
        assert_eq!(t.border.stroke.color, None);
    }

    #[test]
    fn the_grid_of_texts_sets_the_cells_and_a_covered_cell_keeps_none() {
        let mut t = table();
        t.cells[1].col_span = 0;
        t.cells[1].row_span = 0;
        patch_table(&mut t, &TablePatch { texts: Some(vec![vec!["a".into(), "b".into()], vec!["c".into(), "d".into()]]), ..TablePatch::default() }).unwrap();
        assert_eq!(t.texts(), vec![vec!["a", ""], vec!["c", "d"]]);
        assert!(patch_table(&mut t, &TablePatch { texts: Some(vec![vec!["a".into()]]), ..TablePatch::default() }).is_err(), "a grid of another shape is refused");
    }

    #[test]
    fn a_cell_edit_changes_what_it_names_and_the_column_and_row_of_each_cell() {
        let mut t = table();
        let e = CellEdit { text: Some("x\r\ny".into()), width_um: Some(2_000), height_um: Some(2_000), thickness_um: Some(0), h_align: Some(TextJustify::Right), v_align: Some(TextVAlign::Top), margins: [Some(500), None, None, Some(700)], col_width_um: Some(12_000), row_height_um: Some(4_000), ..CellEdit::default() };
        edit_cells(&mut t, &[(1, 1)], &e).unwrap();
        let c = t.cell(1, 1).unwrap();
        assert_eq!(c.text, "x\ny");
        assert_eq!((c.style.width_um, c.style.height_um, c.style.thickness_um, c.style.h_align, c.style.v_align), (2_000, 2_000, 0, TextJustify::Right, TextVAlign::Top));
        assert_eq!(c.margins, [500, 1_003, 1_003, 700], "margins not named stay");
        assert_eq!((t.col_widths[1], t.row_heights[1]), (12_000, 4_000));
        // the other cells are as they were
        assert_eq!(t.cell(0, 0).unwrap().style, TableText::pcb_default());
        assert!(edit_cells(&mut t, &[(5, 5)], &e).is_err());
        assert!(edit_cells(&mut t, &[], &e).is_err());
    }

    #[test]
    fn making_a_cells_text_bold_takes_the_bold_pen_and_unmaking_it_the_normal_one() {
        let mut st = TableText::pcb_default();
        st.set_bold(true);
        assert_eq!((st.bold, st.thickness_um), (true, 254), "a fifth of 1.27 mm");
        st.set_bold(false);
        assert_eq!((st.bold, st.thickness_um), (false, 159), "an eighth");
        st.set_bold(false);
        assert_eq!(st.thickness_um, 159, "no change when it already was not bold");
    }
}
