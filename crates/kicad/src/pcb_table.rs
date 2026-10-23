//! A board's tables (`(table ..)`, KiCad's `PCB_TABLE` with its `PCB_TABLECELL`s) read and written: `PCB_IO_KICAD_SEXPR_PARSER::parsePCB_TABLE`,
//! `parsePCB_TABLECELL` and `parseTextBoxContent`, and `PCB_IO_KICAD_SEXPR::format( const PCB_TABLE* )` with `format( const PCB_TEXTBOX* )`, for
//! [`eda_model::table::Table`].
//!
//! The file keeps every cell's corners (`(start ..) (end ..)`, or four points for a table turned by an angle that is not a quarter turn); the model keeps the
//! grid they are laid out on (`PCB_TABLE::Normalize`) and works the corners out. A table whose cells are not where its grid puts them (a hand-edited file) is
//! not a table this model can hold without moving something: [`read`] gives `None` for it and the caller keeps its text as it was.

use std::fmt::Write as _;

use eda_model::ir::{Point, TextJustify, TextVAlign, Nm};
use eda_model::sch_extras::{SchColor, SchFill};
use eda_model::table::{Table, TableBorder, TableCell, TableLineStyle, TableSeparators, TableStroke, TableText};

use crate::sexpr::{self, Sexpr};
use crate::{duid_for, fmt_mm_f, mm, mm_to_nm, note_uuid, sexpr_str};

/// A flag as the file writes it: `(name yes)`, `(name no)`, `(name)` (yes) or the bare word in the item itself.
fn flag(item: &[Sexpr], name: &str) -> Option<bool> {
    if let Some(l) = sexpr::find(item, name) {
        return Some(sexpr::txt(l, 1).is_none_or(|v| v != "no"));
    }
    item.iter().skip(1).any(|c| matches!(c, Sexpr::Atom(a) if a == name)).then_some(true)
}

fn um(v: f64) -> Nm {
    mm_to_nm(v)
}

/// A colour as `(color r g b a)` writes it: the channels 0..255 and an alpha of 0..1.
pub(crate) fn read_color(list: &[Sexpr]) -> Option<SchColor> {
    let ch = |i: usize| sexpr::num(list, i).map(|v| v.clamp(0.0, 255.0).round() as u8);
    let a = sexpr::num(list, 4).unwrap_or(1.0).clamp(0.0, 1.0);
    Some(SchColor { r: ch(1)?, g: ch(2)?, b: ch(3)?, a: (a * 255.0).round() as u8 })
}

/// `FormatDouble2Str` of an alpha: shortest form, four places at most.
fn alpha_str(a: u8) -> String {
    fmt_mm_f(a as f64 / 255.0)
}

/// `(stroke (width w) (type t) [(color r g b a)])` as `STROKE_PARAMS::Format` writes it.
pub(crate) fn stroke_str(s: &TableStroke) -> String {
    let mut out = format!("(stroke (width {}) (type {})", mm(s.width_nm), s.style.token());
    if let Some(c) = s.color {
        let _ = write!(out, " (color {} {} {} {})", c.r, c.g, c.b, alpha_str(c.a));
    }
    out.push(')');
    out
}

/// `STROKE_PARAMS_PARSER::ParseStroke` on top of `base`: the tokens present change the stroke, the others stay.
pub(crate) fn read_stroke(node: &[Sexpr], base: TableStroke) -> TableStroke {
    let mut s = base;
    if let Some(w) = sexpr::find(node, "width").and_then(|w| sexpr::num(w, 1)) {
        s.width_nm = um(w);
    }
    if let Some(t) = sexpr::find(node, "type").and_then(|t| sexpr::txt(t, 1)) {
        s.style = TableLineStyle::from_token(t);
    }
    if let Some(c) = sexpr::find(node, "color") {
        s.color = read_color(c);
    }
    s
}

/// The `TableText` of an `(effects ..)` (`parseEDA_TEXT`): the size is height first; with no `(justify ..)` the text is centred both ways, whatever the item
/// started with.
pub(crate) fn read_effects(item: &[Sexpr]) -> TableText {
    let mut t = TableText::pcb_default();
    t.h_align = TextJustify::Center;
    t.v_align = TextVAlign::Center;
    t.mirror = false;
    let Some(e) = sexpr::find(item, "effects") else { return t };
    let mut found_size = false;
    if let Some(font) = sexpr::find(e, "font") {
        if let Some(s) = sexpr::find(font, "size") {
            // `(size HEIGHT WIDTH)`
            t.height_nm = um(sexpr::num(s, 1).unwrap_or(1.27));
            t.width_nm = um(sexpr::num(s, 2).unwrap_or(1.27));
            found_size = true;
        }
        t.thickness_nm = sexpr::find(font, "thickness").and_then(|x| sexpr::num(x, 1)).map(um).unwrap_or(0);
        t.bold = flag(font, "bold").unwrap_or(false);
        t.italic = flag(font, "italic").unwrap_or(false);
        t.face = sexpr::find(font, "face").and_then(|f| sexpr::txt(f, 1)).filter(|f| !f.is_empty()).map(str::to_string);
        t.line_spacing_permille = sexpr::find(font, "line_spacing").and_then(|x| sexpr::num(x, 1)).map(|v| (v * 1000.0).round() as i64).filter(|v| *v != 1000).unwrap_or(0);
        t.color = sexpr::find(font, "color").and_then(read_color);
    }
    if !found_size {
        // "Text size was not specified in file, force legacy default units: 60 mils is 1.524 mm".
        t.height_nm = 1_524_000;
        t.width_nm = 1_524_000;
    }
    if let Some(j) = sexpr::find(e, "justify") {
        for tok in j.iter().skip(1).filter_map(Sexpr::text) {
            match tok {
                "left" => t.h_align = TextJustify::Left,
                "right" => t.h_align = TextJustify::Right,
                "top" => t.v_align = TextVAlign::Top,
                "bottom" => t.v_align = TextVAlign::Bottom,
                "mirror" => t.mirror = true,
                _ => {}
            }
        }
    }
    t
}

/// `EDA_TEXT::Format`: `(effects (font [(face ..)] (size H W) [(line_spacing ..)] [(thickness ..)] [bold] [italic] [(color ..)]) [(justify ..)])`.
pub(crate) fn effects_str(t: &TableText) -> String {
    let mut s = String::from("(effects (font");
    if let Some(f) = t.face.as_deref().filter(|f| !f.is_empty()) {
        let _ = write!(s, " (face {})", sexpr_str(f));
    }
    let _ = write!(s, " (size {} {})", mm(t.height_nm), mm(t.width_nm));
    if t.line_spacing_permille != 0 && t.line_spacing_permille != 1000 {
        let _ = write!(s, " (line_spacing {})", fmt_mm_f(t.line_spacing_permille as f64 / 1000.0));
    }
    if t.thickness_nm != 0 {
        let _ = write!(s, " (thickness {})", mm(t.thickness_nm));
    }
    if t.bold {
        s.push_str(" bold");
    }
    if t.italic {
        s.push_str(" italic");
    }
    if let Some(c) = t.color {
        let _ = write!(s, " (color {} {} {} {})", c.r, c.g, c.b, alpha_str(c.a));
    }
    s.push(')');
    if t.mirror || t.h_align != TextJustify::Center || t.v_align != TextVAlign::Center {
        s.push_str(" (justify");
        match t.h_align {
            TextJustify::Left => s.push_str(" left"),
            TextJustify::Right => s.push_str(" right"),
            TextJustify::Center => {}
        }
        match t.v_align {
            TextVAlign::Top => s.push_str(" top"),
            TextVAlign::Bottom => s.push_str(" bottom"),
            TextVAlign::Center => {}
        }
        if t.mirror {
            s.push_str(" mirror");
        }
        s.push(')');
    }
    s.push(')');
    s
}

/// The corners a cell of the file has, top-left first as the table reads them (`EDA_SHAPE::GetCornersInSequence`), and its text angle in millidegrees.
struct FileCell {
    corners: [Point; 4],
    angle: u32,
}

/// One `(table_cell ..)`: the cell, and where the file put it.
fn read_cell(item: &[Sexpr]) -> Option<(TableCell, FileCell)> {
    let text = item.get(1).and_then(Sexpr::text)?.to_string();
    let angle_deg = sexpr::find(item, "angle").and_then(|a| sexpr::num(a, 1)).unwrap_or(0.0);
    let angle = ((angle_deg * 1000.0).round() as i64).rem_euclid(360_000) as u32;
    let corners = if let Some(pts) = sexpr::find(item, "pts") {
        // A cell of a table turned by an angle that is not a quarter turn is a polygon: its four corners in the order the table reads them.
        let p: Vec<Point> = pts.iter().skip(1).filter_map(Sexpr::as_list).filter(|l| sexpr::tag(l) == Some("xy")).filter_map(|l| Some(Point { x: um(sexpr::num(l, 1)?), y: um(sexpr::num(l, 2)?) })).collect();
        if p.len() < 4 {
            return None;
        }
        [p[0], p[1], p[2], p[3]]
    } else {
        let (s, e) = (sexpr::find(item, "start")?, sexpr::find(item, "end")?);
        let (a, b) = (Point { x: um(sexpr::num(s, 1)?), y: um(sexpr::num(s, 2)?) }, Point { x: um(sexpr::num(e, 1)?), y: um(sexpr::num(e, 2)?) });
        // `GetCornersInSequence( angle )` of the box the two points make, for a quarter turn.
        let (l, r, t, bt) = (a.x.min(b.x), a.x.max(b.x), a.y.min(b.y), a.y.max(b.y));
        match angle {
            90_000 => [Point { x: l, y: bt }, Point { x: l, y: t }, Point { x: r, y: t }, Point { x: r, y: bt }],
            180_000 => [Point { x: r, y: bt }, Point { x: l, y: bt }, Point { x: l, y: t }, Point { x: r, y: t }],
            270_000 => [Point { x: r, y: t }, Point { x: r, y: bt }, Point { x: l, y: bt }, Point { x: l, y: t }],
            _ => [Point { x: l, y: t }, Point { x: r, y: t }, Point { x: r, y: bt }, Point { x: l, y: bt }],
        }
    };
    let style = read_effects(item);
    let margins = match sexpr::find(item, "margins") {
        Some(m) => [um(sexpr::num(m, 1).unwrap_or(0.0)), um(sexpr::num(m, 2).unwrap_or(0.0)), um(sexpr::num(m, 3).unwrap_or(0.0)), um(sexpr::num(m, 4).unwrap_or(0.0))],
        // `GetLegacyTextMargin` with the stroke of a cell read from a file (width -1 nm, which is no micrometre at all): three quarters of the height.
        None => [TableCell::legacy_margin_nm(&style, 0); 4],
    };
    let (col_span, row_span) = match sexpr::find(item, "span") {
        Some(s) => (sexpr::num(s, 1).unwrap_or(1.0).max(0.0) as u32, sexpr::num(s, 2).unwrap_or(1.0).max(0.0) as u32),
        None => (1, 1),
    };
    let fill = sexpr::find(item, "fill");
    let cell = TableCell {
        uuid: crate::import_items::item_uuid(item).unwrap_or_default(),
        text,
        col_span,
        row_span,
        margins,
        style,
        fill: fill.and_then(|f| sexpr::find(f, "type")).and_then(|t| sexpr::txt(t, 1)).map(SchFill::from_token).unwrap_or(SchFill::None),
        fill_color: fill.and_then(|f| sexpr::find(f, "color")).and_then(read_color),
        exclude_from_sim: flag(item, "exclude_from_sim").unwrap_or(false),
    };
    Some((cell, FileCell { corners, angle }))
}

/// One `(table ..)` of a board; `None` when it is not a table this model can hold as it is (no cell, a cell the grid does not put where the file does).
pub(crate) fn read(item: &[Sexpr]) -> Option<Table> {
    let cols = sexpr::find(item, "column_count").and_then(|c| sexpr::num(c, 1)).map(|c| c as i64).unwrap_or(0);
    let cells_node = sexpr::find(item, "cells")?;
    let mut cells = Vec::new();
    let mut placed = Vec::new();
    for c in cells_node.iter().skip(1).filter_map(Sexpr::as_list) {
        if sexpr::tag(c) != Some("table_cell") {
            return None;
        }
        let (cell, file) = read_cell(c)?;
        cells.push(cell);
        placed.push(file);
    }
    if cols < 1 || cells.is_empty() || cells.len() % cols as usize != 0 {
        return None;
    }
    let cols = cols as usize;
    let rows = cells.len() / cols;
    let widths = |name: &str, n: usize| -> Vec<Nm> {
        let mut v: Vec<Nm> = sexpr::find(item, name).map(|l| l.iter().skip(1).filter_map(|x| x.text()?.parse::<f64>().ok()).map(um).collect()).unwrap_or_default();
        v.resize(n, 0);
        v
    };
    let mut border = TableBorder::default();
    if let Some(b) = sexpr::find(item, "border") {
        border.external = flag(b, "external").unwrap_or(border.external);
        border.header = flag(b, "header").unwrap_or(border.header);
        if let Some(s) = sexpr::find(b, "stroke") {
            border.stroke = read_stroke(s, TableStroke::solid(-1));
        }
    }
    let mut separators = TableSeparators::default();
    if let Some(s) = sexpr::find(item, "separators") {
        separators.rows = flag(s, "rows").unwrap_or(separators.rows);
        separators.cols = flag(s, "cols").unwrap_or(separators.cols);
        if let Some(st) = sexpr::find(s, "stroke") {
            separators.stroke = read_stroke(st, TableStroke::solid(-1));
        }
    }
    let angle = placed[0].angle;
    let table = Table {
        id: String::new(),
        uuid: crate::import_items::item_uuid(item).unwrap_or_default(),
        layer: sexpr::find(item, "layer").and_then(|l| sexpr::txt(l, 1)).unwrap_or("F.SilkS").to_string(),
        at: placed[0].corners[0],
        angle,
        col_widths: widths("column_widths", cols),
        row_heights: widths("row_heights", rows),
        cells,
        border,
        separators,
    };
    // Every cell must be where the grid puts it (to the micrometre the file is held in, give or take the rounding of a turned corner), and turned as the first.
    for (i, file) in placed.iter().enumerate() {
        let (row, col) = table.row_col(i);
        if file.angle != angle {
            return None;
        }
        let want = table.cell_corners(row, col);
        if want.iter().zip(file.corners.iter()).any(|(a, b)| (a.x - b.x).abs() > 2 || (a.y - b.y).abs() > 2) {
            return None;
        }
    }
    Some(table)
}

/// Every `(table ..)` of a board, in file order: the tables, and the ones this model cannot hold (as text, to be kept the way the file had them).
pub(crate) fn read_all(root: &[Sexpr], refs: &mut Vec<crate::import_items::ItemRef>) -> (Vec<Table>, Vec<String>) {
    let mut out = Vec::new();
    let mut raw = Vec::new();
    for item in sexpr::find_all(root, "table") {
        match read(item) {
            Some(t) => {
                out.push(t);
                refs.push(crate::import_items::ItemRef::of(item));
            }
            None => raw.push(sexpr::to_text(&Sexpr::List(item.to_vec()))),
        }
    }
    (out, raw)
}

/// The uuid of cell `i` of `t`.
fn cell_uuid(t: &Table, i: usize) -> String {
    let c = &t.cells[i];
    if c.uuid.is_empty() {
        duid_for(&format!("table:{}:cell:{i}", t.id), &t.id)
    } else {
        note_uuid(&c.uuid, &t.id);
        c.uuid.clone()
    }
}

/// One table as `format( const PCB_TABLE* )` writes it; returns its uuid. A board's table is on its layer, so are its cells.
pub(crate) fn write(out: &mut String, t: &Table, locked: bool) -> String {
    let uuid = table_uuid(t);
    let layer = sexpr_str(&t.layer);
    let _ = write!(out, "\t(table (column_count {}) (uuid \"{uuid}\")", t.col_count());
    if locked {
        out.push_str(" (locked yes)");
    }
    let _ = write!(out, " (layer {layer})");
    write_body(out, t, false);
    uuid
}

/// The uuid `t` is written with: the file's, else one made from its id.
pub(crate) fn table_uuid(t: &Table) -> String {
    if t.uuid.is_empty() {
        duid_for(&format!("table:{}", t.id), &t.id)
    } else {
        note_uuid(&t.uuid, &t.id);
        t.uuid.clone()
    }
}

/// `formatTableData` from the border on: the border, the separators, the column widths and row heights and the cells, and the parenthesis that closes the item. `knockout`
/// writes each cell's `(knockout no)`, as KiCad 10's `format( const PCB_TEXTBOX* )` does (a drill chart, which only a KiCad 10 file has).
pub(crate) fn write_body(out: &mut String, t: &Table, knockout: bool) {
    let layer = sexpr_str(&t.layer);
    let _ = write!(out, " (border (external {}) (header {})", yes(t.border.external), yes(t.border.header));
    if t.border.external || t.border.header {
        let _ = write!(out, " {}", stroke_str(&t.border.stroke));
    }
    out.push(')');
    let _ = write!(out, " (separators (rows {}) (cols {})", yes(t.separators.rows), yes(t.separators.cols));
    if t.separators.rows || t.separators.cols {
        let _ = write!(out, " {}", stroke_str(&t.separators.stroke));
    }
    out.push(')');
    let _ = write!(out, " (column_widths {})", t.col_widths.iter().map(|w| mm(*w)).collect::<Vec<_>>().join(" "));
    let _ = write!(out, " (row_heights {})", t.row_heights.iter().map(|h| mm(*h)).collect::<Vec<_>>().join(" "));
    out.push_str("\n\t\t(cells");
    for i in 0..t.cells.len() {
        let (row, col) = t.row_col(i);
        let c = &t.cells[i];
        let corners = t.cell_corners(row, col);
        let _ = write!(out, "\n\t\t\t(table_cell {}", sexpr_str(&c.text));
        if t.angle % 90_000 == 0 {
            // `(start ..) (end ..)`: the first and the third corner as the table reads them.
            let _ = write!(out, " (start {} {}) (end {} {})", mm(corners[0].x), mm(corners[0].y), mm(corners[2].x), mm(corners[2].y));
        } else {
            let _ = write!(out, " (pts {})", corners.iter().map(|p| format!("(xy {} {})", mm(p.x), mm(p.y))).collect::<Vec<_>>().join(" "));
        }
        // A new cell's legacy margin as KiCad writes it (1.0025, `TableCell::margin_nm`), any other to the micrometre.
        let m = |i: usize| mm(c.margin_nm(i));
        let _ = write!(out, " (margins {} {} {} {}) (span {} {})", m(0), m(1), m(2), m(3), c.col_span, c.row_span);
        if t.angle != 0 {
            let _ = write!(out, " (angle {})", fmt_mm_f(t.angle as f64 / 1000.0));
        }
        let _ = write!(out, " (layer {layer}) (uuid \"{}\") {}{})", cell_uuid(t, i), effects_str(&c.style), if knockout { " (knockout no)" } else { "" });
    }
    out.push_str("))\n");
}

fn yes(b: bool) -> &'static str {
    if b {
        "yes"
    } else {
        "no"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A two-by-two table as `PCB_IO_KICAD_SEXPR::format( PCB_TABLE )` writes it, at (10, 20) on the front silkscreen: a header row, 2 mm cells.
    fn table_text(angle: &str, cells: &str) -> String {
        format!(
            r#"(table (column_count 2) (uuid "11111111-1111-1111-1111-111111111111") (layer "F.SilkS") (border (external yes) (header yes) (stroke (width 0.1) (type solid))) (separators (rows yes) (cols no) (stroke (width 0.05) (type dash) (color 255 0 0 1))) (column_widths 10 20) (row_heights 5 6)
  (cells {cells}))"#,
        )
        .replace("ANGLE", angle)
    }

    fn flat_cells() -> String {
        let cell = |text: &str, x: f64, y: f64, w: f64, h: f64, extra: &str| {
            format!(
                r#"(table_cell "{text}" (start {x} {y}) (end {} {}) (margins 1 1 1 1) (span 1 1) {extra}(layer "F.SilkS") (uuid "22222222-2222-2222-2222-22222222222{}") (effects (font (size 1.5 1.5) (thickness 0.2) bold) (justify left top)))"#,
                x + w,
                y + h,
                text.len() % 10
            )
        };
        [cell("A", 10.0, 20.0, 10.0, 5.0, ""), cell("B", 20.0, 20.0, 20.0, 5.0, ""), cell("C", 10.0, 25.0, 10.0, 6.0, ""), cell("D", 20.0, 25.0, 20.0, 6.0, "")].join("\n")
    }

    fn read_text(text: &str) -> Option<Table> {
        let tree = sexpr::parse(text).unwrap();
        read(tree.as_list().unwrap())
    }

    #[test]
    fn a_table_as_kicad_writes_it_is_read_with_its_grid_its_strokes_and_its_cells_looks() {
        let t = read_text(&table_text("", &flat_cells())).expect("a table");
        assert_eq!((t.col_count(), t.row_count()), (2, 2));
        assert_eq!(t.at, Point { x: 10_000, y: 20_000 });
        assert_eq!((t.col_widths.clone(), t.row_heights.clone()), (vec![10_000, 20_000], vec![5_000, 6_000]));
        assert_eq!(t.layer, "F.SilkS");
        assert_eq!(t.uuid, "11111111-1111-1111-1111-111111111111");
        assert!(t.border.external && t.border.header && !t.separators.cols && t.separators.rows);
        assert_eq!(t.border.stroke, TableStroke::solid(100));
        assert_eq!(t.separators.stroke.width_nm, 50);
        assert_eq!(t.separators.stroke.style, TableLineStyle::Dash);
        assert_eq!(t.separators.stroke.color, Some(SchColor { r: 255, g: 0, b: 0, a: 255 }));
        let c = t.cell(1, 1).unwrap();
        assert_eq!(c.text, "D");
        assert_eq!(c.margins, [1_000; 4]);
        assert_eq!((c.style.height_nm, c.style.width_nm, c.style.thickness_nm, c.style.bold, c.style.h_align, c.style.v_align), (1_500, 1_500, 200, true, TextJustify::Left, TextVAlign::Top));
        assert_eq!(t.cell_corners(1, 1)[2], Point { x: 40_000, y: 31_000 });
    }

    #[test]
    fn a_written_table_reads_back_the_same_and_writes_the_same_text() {
        let t = read_text(&table_text("", &flat_cells())).unwrap();
        let mut t = t;
        t.id = "tbl_test".into();
        let mut text = String::new();
        let uuid = write(&mut text, &t, true);
        assert_eq!(uuid, "11111111-1111-1111-1111-111111111111");
        assert!(text.starts_with("\t(table (column_count 2) (uuid \"11111111-1111-1111-1111-111111111111\") (locked yes) (layer \"F.SilkS\") (border (external yes) (header yes) (stroke (width 0.1) (type solid))) (separators (rows yes) (cols no) (stroke (width 0.05) (type dash) (color 255 0 0 1))) (column_widths 10 20) (row_heights 5 6)"), "{text}");
        assert!(text.contains("(table_cell \"A\" (start 10 20) (end 20 25) (margins 1 1 1 1) (span 1 1) (layer \"F.SilkS\") (uuid \"22222222-2222-2222-2222-222222222221\") (effects (font (size 1.5 1.5) (thickness 0.2) bold) (justify left top)))"), "{text}");
        let mut again = read_text(text.trim()).expect("reads again");
        again.id = t.id.clone();
        assert_eq!(again, t);
        let mut text2 = String::new();
        write(&mut text2, &again, true);
        assert_eq!(text2, text);
    }

    #[test]
    fn a_table_turned_a_quarter_keeps_its_first_corner_and_writes_start_and_end_as_kicad_does() {
        // The grid of `flat_cells` turned 90 degrees about (10, 20): cell (0,0) is 5 mm wide on the screen and 10 mm tall, going up from (10, 20).
        let mut t = read_text(&table_text("", &flat_cells())).unwrap();
        t.rotate(Point { x: 10_000, y: 20_000 }, 90_000);
        t.id = "tbl_q".into();
        let mut text = String::new();
        write(&mut text, &t, false);
        assert!(text.contains("(angle 90)"), "{text}");
        let again = read_text(text.trim()).expect("reads again");
        assert_eq!(again.angle, 90_000);
        assert_eq!(again.at, t.at);
        for r in 0..2 {
            for c in 0..2 {
                assert_eq!(again.cell_corners(r, c), t.cell_corners(r, c));
            }
        }
        // `Rotate` sets the start to the corner the table reads first and the end to the third one
        let a = t.cell_corners(0, 0);
        assert!(text.contains(&format!("(start {} {}) (end {} {})", mm(a[0].x), mm(a[0].y), mm(a[2].x), mm(a[2].y))), "{text}");
    }

    #[test]
    fn a_table_turned_by_any_other_angle_is_a_polygon_per_cell() {
        let mut t = read_text(&table_text("", &flat_cells())).unwrap();
        t.rotate(Point { x: 10_000, y: 20_000 }, 30_000);
        t.id = "tbl_r".into();
        let mut text = String::new();
        write(&mut text, &t, false);
        assert_eq!(text.matches("(pts (xy").count(), 4, "{text}");
        assert!(!text.contains("(start "), "{text}");
        let again = read_text(text.trim()).expect("reads again");
        assert_eq!(again.angle, 30_000);
        for r in 0..2 {
            for c in 0..2 {
                for (a, b) in again.cell_corners(r, c).iter().zip(t.cell_corners(r, c).iter()) {
                    assert!((a.x - b.x).abs() <= 2 && (a.y - b.y).abs() <= 2, "{a:?} {b:?}");
                }
            }
        }
    }

    #[test]
    fn a_cell_with_no_justify_is_centred_and_one_with_no_margins_gets_the_legacy_ones() {
        let cells = r#"(table_cell "x" (start 0 0) (end 10 5) (span 1 1) (layer "F.SilkS") (uuid "33333333-3333-3333-3333-333333333333") (effects (font (size 2 2) (thickness 0.3))))"#;
        let text = format!(r#"(table (column_count 1) (layer "F.SilkS") (column_widths 10) (row_heights 5) (cells {cells}))"#);
        let t = read_text(&text).unwrap();
        let c = t.cell(0, 0).unwrap();
        assert_eq!((c.style.h_align, c.style.v_align), (TextJustify::Center, TextVAlign::Center));
        // the border and separators the file does not mention: both on, the stroke of a table that has none
        assert!(t.border.external && t.border.header && t.separators.rows && t.separators.cols);
        assert_eq!(t.border.stroke.width_nm, -1);
        // a cell read from a file has the stroke -1 of `parseTextBoxContent`: -1 nm and three quarters of the 2 mm height
        assert_eq!(c.margins, [1_500; 4]);
    }

    #[test]
    fn a_table_whose_cells_are_not_on_its_grid_is_left_as_text() {
        let mut cells = flat_cells();
        cells = cells.replacen("(start 20 20) (end 40 25)", "(start 20 20) (end 41 25)", 1);
        assert!(read_text(&table_text("", &cells)).is_none());
        // a count that does not divide the cells
        let odd = table_text("", &flat_cells()).replace("(column_count 2)", "(column_count 3)");
        assert!(read_text(&odd).is_none());
    }

    const HEADER: &str = r#"(kicad_pcb (version 20241229) (generator "pcbnew") (generator_version "9.0")
  (general (thickness 1.6) (legacy_teardrops no))
  (paper "A4")
  (layers (0 "F.Cu" signal) (2 "B.Cu" signal) (5 "F.SilkS" user "F.Silkscreen") (25 "Dwgs.User" user "User.Drawings") (44 "Edge.Cuts" user))
  (setup (pad_to_mask_clearance 0))
  (net 0 "")
"#;

    /// A board with a table is read into `DrawingsSection::tables`, not kept as text, and exported again as the same table, locked and in its group.
    #[test]
    fn a_board_with_a_table_keeps_it_as_a_table_through_the_importer_and_the_writer() {
        let table = table_text("", &flat_cells()).replace("(layer \"F.SilkS\") (border", "(locked yes) (layer \"F.SilkS\") (border");
        let line = |l: &str| l.trim().to_string();
        let board = format!("{HEADER}  (gr_line (start 0 0) (end 50 0) (layer \"Edge.Cuts\") (stroke (width 0.1) (type solid)))\n  {}\n  (group \"tabs\" (uuid \"55555555-5555-5555-5555-555555555555\") (members \"11111111-1111-1111-1111-111111111111\" \"66666666-6666-6666-6666-666666666666\"))\n  (gr_text \"x\" (at 5 5) (layer \"F.SilkS\") (uuid \"66666666-6666-6666-6666-666666666666\") (effects (font (size 1 1))))\n)\n", line(&table));
        let (design, model, notes) = crate::import_kicad_pcb(&board).expect("imports");
        assert!(notes.unread_items.is_empty(), "{:?}", notes.unread_items);
        let dr = design.drawings.as_ref().unwrap();
        assert!(dr.raw_items.is_empty(), "{:?}", dr.raw_items);
        assert_eq!(dr.tables.len(), 1);
        let t = &dr.tables[0];
        assert!(t.id.starts_with("tbl_"), "{}", t.id);
        assert!(dr.locked_ids.contains(&t.id), "a locked table comes back locked");
        assert_eq!(dr.groups.len(), 1);
        assert!(dr.groups[0].member_ids.contains(&t.id), "the group names the table by its uuid: {:?}", dr.groups);
        let out = crate::export_kicad_pcb(&design, &model, &crate::ExportMeta { date: "2026-01-01", title: "t" }).expect("exports");
        assert_eq!(out.matches("(table (column_count 2)").count(), 1, "{out}");
        assert!(out.contains("(locked yes) (layer \"F.SilkS\") (border (external yes)"), "{out}");
        assert!(out.contains("(members \"11111111-1111-1111-1111-111111111111\""), "the table's own uuid is the member: {out}");
        let (back, _, back_notes) = crate::import_kicad_pcb(&out).expect("re-imports");
        assert!(back_notes.unread_items.is_empty());
        let back_dr = back.drawings.as_ref().unwrap();
        assert_eq!(back_dr.tables, dr.tables, "read again, the same table");
        assert_eq!(back_dr.groups.len(), 1);
        let again = crate::export_kicad_pcb(&back, &model, &crate::ExportMeta { date: "2026-01-01", title: "t" }).unwrap();
        assert_eq!(again, out, "and written again, the same text");
    }

    /// A new cell's legacy margin is written as KiCad holds it, `KiROUND( 0.1 mm / 2 ) + KiROUND( 1.27 mm * 0.75 )` = 1.0025 mm (the IR's 1.003 rounds it), and read back
    /// as the same cell; a margin set to the micrometre stays what it is.
    #[test]
    fn a_new_cells_legacy_margin_is_written_to_the_nanometre() {
        let mut t = Table::new_grid("F.SilkS", eda_model::ir::Point { x: 0, y: 0 }, 2, 1, 10_000, 5_000, 100);
        t.id = "tbl_l".into();
        t.cells = vec![TableCell::pcb_new(false), TableCell { margins: [500; 4], ..TableCell::pcb_new(false) }];
        assert_eq!(t.cells[0].margins, [1_003; 4]);
        let mut out = String::new();
        write(&mut out, &t, false);
        assert!(out.contains("(margins 1.0025 1.0025 1.0025 1.0025)"), "{out}");
        assert!(out.contains("(margins 0.5 0.5 0.5 0.5)"), "{out}");
        let back = read_text(&out).expect("a table");
        assert_eq!((back.cells[0].margins, back.cells[0].margin_nm(0)), ([1_003; 4], 1_002_500));
    }

    #[test]
    fn merged_cells_keep_their_spans_and_the_cells_they_cover() {
        let cell = |text: &str, x: f64, y: f64, w: f64, h: f64, span: &str| {
            format!(r#"(table_cell "{text}" (start {x} {y}) (end {} {}) (margins 1 1 1 1) (span {span}) (layer "F.SilkS") (uuid "44444444-4444-4444-4444-44444444444{}") (effects (font (size 1.5 1.5)) (justify left)))"#, x + w, y + h, text.len() + x as usize % 7)
        };
        let cells = [cell("hdr", 0.0, 0.0, 30.0, 5.0, "2 1"), cell("", 10.0, 0.0, 20.0, 5.0, "0 1"), cell("a", 0.0, 5.0, 10.0, 6.0, "1 1"), cell("b", 10.0, 5.0, 20.0, 6.0, "1 1")].join(" ");
        let text = format!(r#"(table (column_count 2) (layer "F.SilkS") (column_widths 10 20) (row_heights 5 6) (cells {cells}))"#);
        let t = read_text(&text).expect("a table");
        assert_eq!((t.cell(0, 0).unwrap().col_span, t.cell(0, 1).unwrap().col_span), (2, 0));
        assert_eq!(t.cell_local_rect(0, 0), (0, 0, 30_000, 5_000));
        let mut out = String::new();
        let mut t = t;
        t.id = "tbl_m".into();
        write(&mut out, &t, false);
        assert!(out.contains("(table_cell \"hdr\" (start 0 0) (end 30 5) (margins 1 1 1 1) (span 2 1)"), "{out}");
        assert!(out.contains("(span 0 1)"), "{out}");
    }
}
