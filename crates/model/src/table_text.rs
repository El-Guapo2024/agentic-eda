//! What a table cell shows and where it puts its lines: the text of a `PCB_TABLECELL` after its variables are resolved and it is broken to the width of its
//! column, and the place of each line as `PCB_TEXTBOX::GetDrawPos` and `EDA_TEXT::GetLinePositions` / `FONT::getLinePositions` put it.
//!
//! Everything here is worked out in nanometres, the unit KiCad's board works in, with `KiROUND` where KiCad rounds, and handed back in nanometres:
//! a table's cells are on the nanometre grid of the design, but the width of a word, the margin of a cell and the offset of a line are KiCad's.
//!
//! Ported from:
//!   pcbnew/pcb_tablecell.cpp       `PCB_TABLECELL::GetShownText` (`${ROW}`, `${COL}`, `${ADDR}`, `${LAYER}`, then the board's variables, then `LinebreakText`)
//!   pcbnew/pcb_textbox.cpp         `PCB_TEXTBOX::GetDrawPos`
//!   common/font/font.cpp           `FONT::LinebreakText`, `wordbreakMarkup`, `getLinePositions`, `Draw`
//!   common/font/stroke_font.cpp    `STROKE_FONT::GetTextAsGlyphs` (the advance of a run), `GetInterline` (the legacy factor)
//!   common/eda_text.cpp            `EDA_TEXT::GetLinePositions`, `GetEffectiveTextPenWidth`

use crate::ir::{Point, TextJustify, TextVAlign, Nm};
use crate::kicad_font::{advance, kiround};
use crate::table::{rotate_point, Table, TableCell};
use crate::text_vars::{has_text_vars, resolve_text_vars, shown_text, unescape_string, PcbBoard, Resolver, ESC_AT, ESC_DOLLAR};

/// One line of a cell's text, placed: the point its first glyph's origin is at once the cell's turn and mirror are applied, nm on the board (fractions kept:
/// KiCad's own are nanometres).
#[derive(Debug, Clone, PartialEq)]
pub struct PlacedLine {
    pub text: String,
    pub x_nm: f64,
    pub y_nm: f64,
}

/// A cell's text ready to draw.
#[derive(Debug, Clone, PartialEq)]
pub struct CellDraw {
    /// What the cell shows: the variables resolved and the lines broken to the column (`GetShownText`).
    pub shown: String,
    pub lines: Vec<PlacedLine>,
    /// The angle every line is drawn at, millidegrees, counter-clockwise on the screen.
    pub angle: u32,
    /// The glyphs are drawn mirrored (`m_Mirrored`).
    pub mirror: bool,
    /// The stroke width the glyphs are drawn with, nm (`GetEffectiveTextPenWidth`).
    pub pen_nm: f64,
}

/// The letters of a column, `A` for 0, `B` for 1 ... `Z` for 25 and `A` again after (`'A' + GetColumn() % 26`).
pub fn column_letter(col: usize) -> char {
    (b'A' + (col % 26) as u8) as char
}

/// `PCB_TABLECELL::GetAddr`: `B3` for the second column, third row (rows count from 1 on a board).
pub fn pcb_cell_addr(row: usize, col: usize) -> String {
    format!("{}{}", column_letter(col), row + 1)
}

/// The text of a cell of a board's table with its variables resolved, before it is broken into lines: `${ROW}` and `${COL}` count from 1, `${ADDR}` is the
/// spreadsheet name of the cell, `${LAYER}` the name of the table's layer, then the board's own variables (`PCB_TABLECELL::GetShownText`).
pub fn pcb_cell_resolved(raw: &str, layer: &str, row: usize, col: usize, board: &PcbBoard) -> String {
    shown_text(
        raw,
        &|t: &str| match t {
            "ROW" => Some((row + 1).to_string()),
            "COL" => Some((col + 1).to_string()),
            "ADDR" => Some(pcb_cell_addr(row, col)),
            "LAYER" => Some(board.layer_name(layer)),
            _ => board.resolve(t, 1),
        },
        0,
    )
}

// ------------------------------------------------------------------------------------------------------------------------- line breaking

/// `STROKE_FONT::GetTextAsGlyphs( .. ).x` of a run of plain text: the sum of the rounded advances of its glyphs at `size` (a space is the font's space; no run
/// here has a tab).
fn run_width(text: &str, size: f64) -> i64 {
    text.chars().map(|c| kiround(advance(c) * size)).sum()
}

/// One word of a line and its width: a run of plain text up to and including the space that ends it, or a markup node (`~{..}`, `_{..}`, `^{..}`) whole
/// (`wordbreakMarkup`).
fn words_of_line(line: &str, size: f64) -> Vec<(String, i64)> {
    let chars: Vec<char> = line.chars().collect();
    let mut out: Vec<(String, i64)> = Vec::new();
    let mut plain = String::new();
    let flush = |plain: &mut String, out: &mut Vec<(String, i64)>| {
        if plain.is_empty() {
            return;
        }
        // `wxStringTokenizer( textRun, " ", wxTOKEN_RET_DELIMS )`: every token keeps the space that ended it.
        let mut token = String::new();
        for c in plain.chars() {
            token.push(c);
            if c == ' ' {
                let trimmed = token.trim_end_matches(' ');
                let w = run_width(if trimmed.is_empty() { &token } else { trimmed }, size);
                out.push((std::mem::take(&mut token), w));
            }
        }
        if !token.is_empty() {
            let w = run_width(token.trim_end_matches(' '), size);
            out.push((token, w));
        }
        plain.clear();
    };
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if matches!(c, '~' | '_' | '^') && chars.get(i + 1) == Some(&'{') {
            // The node runs to its matching brace.
            let (mut depth, mut j) = (1, i + 2);
            while j < chars.len() && depth > 0 {
                match chars[j] {
                    '{' => depth += 1,
                    '}' => depth -= 1,
                    _ => {}
                }
                j += 1;
            }
            if depth == 0 {
                flush(&mut plain, &mut out);
                let word: String = chars[i..j].iter().collect();
                let inner: String = chars[i + 2..j - 1].iter().collect();
                let scale = if c == '~' { 1.0 } else { 0.8 };
                let (stripped, _) = crate::kicad_font::strip_markup(&inner);
                out.push((word, run_width(&stripped, size * scale)));
                i = j;
                continue;
            }
        }
        plain.push(c);
        i += 1;
    }
    flush(&mut plain, &mut out);
    out
}

/// `FONT::LinebreakText( aText, aColumnWidth, aSize, aThickness, aBold, aItalic )` for the stroke font: the text with a line feed where a line has to end for
/// its words to fit `column_width_nm` less the stroke. A word wider than the column stays whole on a line of its own.
pub fn linebreak_text(text: &str, column_width_nm: i64, size_w_nm: i64, thickness_nm: i64) -> String {
    let size = size_w_nm as f64;
    let space_width = kiround(advance(' ') * size);
    let lines: Vec<&str> = text.split('\n').collect();
    let mut out = String::new();
    for (ii, line) in lines.iter().enumerate() {
        let mut words: Vec<(String, i64)> = Vec::new();
        for (run, width) in words_of_line(line, size) {
            match words.last_mut() {
                Some(last) if !last.0.ends_with(' ') => {
                    last.0.push_str(&run);
                    last.1 += width;
                }
                _ => words.push((run, width)),
            }
        }
        let (mut bury, mut line_width, mut pending) = (false, 0i64, 0usize);
        for (word, word_width) in &words {
            let mut pending_width = pending as i64 * space_width;
            let overflow = line_width + pending_width + word_width > column_width_nm - thickness_nm;
            if overflow && pending > 0 {
                out.push('\n');
                line_width = 0;
                pending = 0;
                pending_width = 0;
                bury = true;
            }
            if word == " " {
                pending += 1;
            } else {
                if bury {
                    bury = false;
                } else {
                    out.push_str(&" ".repeat(pending));
                    line_width += pending_width;
                }
                if let Some(stem) = word.strip_suffix(' ') {
                    out.push_str(stem);
                    pending = 1;
                } else {
                    out.push_str(word);
                    pending = 0;
                }
                line_width += word_width;
            }
        }
        if ii != lines.len() - 1 {
            out.push('\n');
        }
    }
    out
}

// -------------------------------------------------------------------------------------------------------------------------------- placing

/// `PCB_TEXTBOX::GetDrawPos()`: where the text is anchored in a cell, nanometres: the corner or middle of the cell its justification names, taken from the
/// corners as the table reads them, and moved in by the margin of that side, turned by the cell's angle. A cell whose text is mirrored is anchored on the
/// other side (`IsMirrored() != aIsFlipped` swaps left and right).
pub fn draw_pos_nm(t: &Table, row: usize, col: usize) -> (i64, i64) {
    let cell = t.cell(row, col).expect("a cell of the table");
    let c = t.cell_corners(row, col).map(|p| (p.x, p.y));
    let mid = |a: (i64, i64), b: (i64, i64)| ((a.0 + b.0) / 2, (a.1 + b.1) / 2);
    let mid_top = mid(c[0], c[1]);
    let mid_bottom = mid(c[3], c[2]);
    let mid_left = mid(c[0], c[3]);
    let mid_right = mid(c[1], c[2]);
    let center = ((c[0].0 + c[1].0 + c[2].0 + c[3].0) / 4, (c[0].1 + c[1].1 + c[2].1 + c[3].1) / 4);
    let mut h = cell.style.h_align;
    let v = cell.style.v_align;
    if cell.style.mirror {
        h = match h {
            TextJustify::Left => TextJustify::Right,
            TextJustify::Right => TextJustify::Left,
            TextJustify::Center => TextJustify::Center,
        };
    }
    let anchor = match (h, v) {
        (TextJustify::Left, TextVAlign::Top) => c[0],
        (TextJustify::Center, TextVAlign::Top) => mid_top,
        (TextJustify::Right, TextVAlign::Top) => c[1],
        (TextJustify::Left, TextVAlign::Center) => mid_left,
        (TextJustify::Center, TextVAlign::Center) => center,
        (TextJustify::Right, TextVAlign::Center) => mid_right,
        (TextJustify::Left, TextVAlign::Bottom) => c[3],
        (TextJustify::Center, TextVAlign::Bottom) => mid_bottom,
        (TextJustify::Right, TextVAlign::Bottom) => c[2],
    };
    let mut off = (0i64, 0i64);
    match h {
        TextJustify::Left => off.0 = cell.margins[0],
        TextJustify::Right => off.0 = -cell.margins[2],
        TextJustify::Center => {}
    }
    match v {
        TextVAlign::Top => off.1 = cell.margins[1],
        TextVAlign::Bottom => off.1 = -cell.margins[3],
        TextVAlign::Center => {}
    }
    let (ox, oy) = rotate_point(off.0, off.1, t.angle);
    (anchor.0 + ox, anchor.1 + oy)
}

/// `GetShownText`'s second half: the resolved text broken to the width of the cell less its margins (`colWidth = ( corners[1] - corners[0] ).EuclideanNorm()`,
/// less the left and right margins for a text that lies along the sheet and the top and bottom ones for any other angle, as KiCad does).
pub fn wrap_to_cell(t: &Table, row: usize, col: usize, resolved: &str) -> String {
    let Some(cell) = t.cell(row, col) else { return resolved.to_string() };
    let c = t.cell_corners(row, col);
    let (dx, dy) = ((c[1].x - c[0].x) as f64, (c[1].y - c[0].y) as f64);
    let mut width = kiround((dx * dx + dy * dy).sqrt());
    let horizontal = t.angle % 180_000 == 0;
    width -= if horizontal { cell.margins[0] + cell.margins[2] } else { cell.margins[1] + cell.margins[3] };
    linebreak_text(resolved, width, cell.style.width_nm, cell.style.pen_nm())
}

/// The lines of `shown` (already broken) as `EDA_TEXT::GetLinePositions` places them for the plot and `FONT::Draw` for the canvas -- the two come to the same
/// place: the first line's baseline is `size.y - thickness * 0.052` under the top of the block, the block is as high as `1.17` lines plus the others, and a
/// line starts `thickness / 1.52` from its anchor, or is centred or ended on it. Each line is then turned about the cell's draw position and, when the text is
/// mirrored, mirrored about it.
pub fn place_lines(t: &Table, row: usize, col: usize, shown: &str) -> Vec<PlacedLine> {
    let Some(cell) = t.cell(row, col) else { return Vec::new() };
    place_lines_at(&cell.style, cell.style.pen_nm() as f64, draw_pos_nm(t, row, col), t.angle, shown)
}

/// [`place_lines`] for any text box: `style` is its text, `pen_nm` the stroke width the glyphs are drawn with, `origin` its `GetDrawPos()` (nanometres) and `angle` the
/// angle the lines are turned about it (millidegrees, counter-clockwise on the screen).
pub fn place_lines_at(style: &crate::table::TableText, pen_nm: f64, origin: (i64, i64), angle: u32, shown: &str) -> Vec<PlacedLine> {
    let (size_w, size_h) = (style.width_nm as f64, style.height_nm as f64);
    let pen = pen_nm;
    let lines: Vec<&str> = shown.split('\n').collect();
    // `GetInterline( size.y ) * m_LineSpacing`, with the stroke font's legacy factor, as an int.
    let interline = (size_h * 1.68 * 0.9583 * style.line_spacing()) as i64;
    let mut height = 0i64;
    for i in 0..lines.len() {
        height += if i == 0 { (size_h * 1.17) as i64 } else { interline };
    }
    // `offset.x += thickness / 1.52` and `offset.y -= thickness * 0.052` on ints: the double is cut to an int after the add.
    let mut off = (0i64, size_h as i64);
    off.0 = (off.0 as f64 + pen / 1.52) as i64;
    off.1 = (off.1 as f64 - pen * 0.052) as i64;
    match style.v_align {
        TextVAlign::Top => {}
        TextVAlign::Center => off.1 -= height / 2,
        TextVAlign::Bottom => off.1 -= height,
    }
    lines
        .iter()
        .enumerate()
        .map(|(i, line)| {
            // `boundingBoxSingleLine`: the advance of the whole line (markup drawn at its own size).
            let (stripped, _) = crate::kicad_font::strip_markup(line);
            let extent = run_width(&stripped, size_w);
            let line_x = match style.h_align {
                TextJustify::Left => off.0,
                TextJustify::Center => -extent / 2,
                TextJustify::Right => -(extent + off.0),
            };
            let line_y = off.1 + i as i64 * interline;
            // Mirror about the draw position's x, then turn about it (`STROKE_GLYPH::Transform`).
            let mx = if style.mirror { -line_x } else { line_x };
            let (rx, ry) = rotate_point(mx, line_y, angle);
            PlacedLine { text: (*line).to_string(), x_nm: (origin.0 + rx) as f64, y_nm: (origin.1 + ry) as f64 }
        })
        .collect()
}

/// A board cell ready to draw. `board` resolves the variables.
pub fn pcb_cell_draw(t: &Table, row: usize, col: usize, board: &PcbBoard) -> CellDraw {
    let cell: &TableCell = t.cell(row, col).expect("a cell of the table");
    let resolved = pcb_cell_resolved(&cell.text, &t.layer, row, col, board);
    let shown = wrap_to_cell(t, row, col, &resolved);
    let lines = if cell.is_covered() { Vec::new() } else { place_lines(t, row, col, &shown) };
    CellDraw { shown, lines, angle: t.angle, mirror: cell.style.mirror, pen_nm: cell.style.pen_nm() as f64 }
}

// ------------------------------------------------------------------------------------------------------------------------------------ a sheet's table

/// The stroke width a sheet's text is drawn with when it has none of its own: the sheet's default line width, 6 mils (`DEFAULT_LINE_WIDTH_MILS`), nanometres.
pub const SCH_DEFAULT_PEN_NM: i64 = 152_400;

/// `EDA_TEXT::GetEffectiveTextPenWidth( aDefaultPenWidth )`, nanometres: the thickness the text has, else the default pen (a fifth of the glyph width when bold), at most a
/// quarter of the smaller glyph dimension (`ClampTextPenSize`). A sheet draws its text with the default pen where a board's is an eighth of the glyph.
pub fn effective_pen_nm(style: &crate::table::TableText, default_pen_nm: i64) -> i64 {
    let mut pen = style.thickness_nm;
    if pen <= 1 {
        pen = default_pen_nm;
        if style.bold {
            pen = kiround(style.width_nm as f64 / 5.0);
        } else if pen <= 1 {
            pen = kiround(style.width_nm as f64 / 8.0);
        }
    }
    let size = style.width_nm.abs().min(style.height_nm.abs());
    pen.min(kiround(size as f64 * 0.25))
}

/// `SCH_TABLECELL::GetAddr`: the column's letter and the row, which a sheet counts from 0 (`A0`, `B3`).
pub fn sch_cell_addr(row: usize, col: usize) -> String {
    format!("{}{}", column_letter(col), row)
}

/// `parseCellAddress`: `A0`, `b12`, `AA3` as the row and the column, both from 0.
fn parse_cell_address(addr: &str) -> Option<(usize, usize)> {
    let letters: String = addr.chars().take_while(|c| c.is_ascii_alphabetic()).collect();
    if letters.is_empty() {
        return None;
    }
    let col = letters.to_uppercase().chars().fold(0usize, |n, c| n * 26 + (c as usize - 'A' as usize + 1)) - 1;
    let row: i64 = addr[letters.len()..].parse().ok()?;
    (row >= 0).then_some((row as usize, col))
}

/// `SCH_TABLECELL::GetShownText` of the cell at (`row`, `col`): `${ROW}` and `${COL}` (from 0), `${ADDR}`, `${CELL("A0")}` / `${CELL(row, col)}` (the shown text of another
/// cell, `<Unresolved: ..>` when it has none, `<Circular reference>` past [`crate::text_vars::MAX_DEPTH`]), then the sheet's own variables (`sheet`), the lines broken to the
/// width of the cell, and the markers of a literal `\${..}` turned back at the top level (`depth` 0) only. `@{..}` expressions are not evaluated (`text_vars.rs`).
pub fn sch_cell_shown(t: &Table, row: usize, col: usize, sheet: &Resolver, depth: usize) -> String {
    let Some(cell) = t.cell(row, col) else { return String::new() };
    let resolver = |token: &str| -> Option<String> {
        match token {
            "ROW" => return Some(row.to_string()),
            "COL" => return Some(col.to_string()),
            "ADDR" => return Some(sch_cell_addr(row, col)),
            _ => {}
        }
        if let Some(args) = token.strip_prefix("CELL(").and_then(|s| s.strip_suffix(')')) {
            let args = args.trim();
            let (target_row, target_col) = if args.len() >= 2 && args.starts_with('"') && args.ends_with('"') {
                let addr = &args[1..args.len() - 1];
                match parse_cell_address(addr) {
                    Some(rc) => rc,
                    None => return Some(format!("<Unresolved: Invalid cell address: {addr}>")),
                }
            } else if let Some((r, c)) = args.split_once(',') {
                match (r.trim().parse::<i64>(), c.trim().parse::<i64>()) {
                    (Ok(r), Ok(c)) if r >= 0 && c >= 0 => (r as usize, c as usize),
                    (Ok(r), Ok(c)) => {
                        return Some(format!("<Unresolved: Cell {} not found>", if r >= 0 && c >= 0 { sch_cell_addr(r as usize, c as usize) } else { args.to_string() }));
                    }
                    _ => return Some(format!("<Unresolved: Invalid cell coordinates: {args}>")),
                }
            } else {
                return Some(format!("<Unresolved: Invalid CELL() syntax: {args}>"));
            };
            if target_row >= t.row_count() || target_col >= t.col_count() {
                return Some(format!("<Unresolved: Cell {} not found>", sch_cell_addr(target_row, target_col)));
            }
            if depth >= crate::text_vars::MAX_DEPTH {
                return Some("<Circular reference>".to_string());
            }
            return Some(sch_cell_shown(t, target_row, target_col, sheet, depth + 1));
        }
        sheet(token)
    };
    let mut text = unescape_string(&cell.text);
    if has_text_vars(&text) {
        let mut passes = 0usize;
        text = resolve_text_vars(&text, &resolver, &mut passes);
    }
    let mut shown = sch_wrap_to_cell(t, row, col, &text);
    if depth == 0 {
        shown = shown.replace(ESC_DOLLAR, "${").replace(ESC_AT, "@{");
    }
    shown
}

/// The width the text of a sheet's cell is broken to: the cell's extent along the text (`abs( size.x )` less the left and right margins; `abs( size.y )` less the top and
/// bottom ones for a text that stands), the stroke being the one `GetEffectiveTextPenWidth()` gives (an eighth of the glyph: the breaking does not know the sheet's pen).
pub fn sch_wrap_to_cell(t: &Table, row: usize, col: usize, resolved: &str) -> String {
    let Some(cell) = t.cell(row, col) else { return resolved.to_string() };
    let c = t.cell_corners(row, col);
    let (sx, sy) = ((c[2].x - c[0].x).abs(), (c[2].y - c[0].y).abs());
    let vertical = t.angle % 180_000 == 90_000;
    let width = if vertical { sy - (cell.margins[1] + cell.margins[3]) } else { sx - (cell.margins[0] + cell.margins[2]) };
    linebreak_text(resolved, width, cell.style.width_nm, cell.style.pen_nm())
}

/// `SCH_TEXTBOX::GetDrawPos()`, nanometres: the box of the cell (`start` and `end`, normalised), and the corner or middle of it its justification names, moved in by the
/// margin of that side. A text that stands (a table turned a quarter) is anchored by its horizontal justification along the box's height and by its vertical one along
/// its width, with the margins of the sides those are.
pub fn sch_draw_pos_nm(t: &Table, row: usize, col: usize) -> (i64, i64) {
    let cell = t.cell(row, col).expect("a cell of the table");
    let c = t.cell_corners(row, col).map(|p| (p.x, p.y));
    let (left, right) = (c[0].0.min(c[2].0), c[0].0.max(c[2].0));
    let (top, bottom) = (c[0].1.min(c[2].1), c[0].1.max(c[2].1));
    let m = cell.margins;
    let (h, v) = (cell.style.h_align, cell.style.v_align);
    let mut pos = (left + m[0], bottom - m[3]);
    if t.angle % 180_000 == 90_000 {
        pos.1 = match h {
            TextJustify::Left => bottom - m[3],
            TextJustify::Center => (top + bottom) / 2,
            TextJustify::Right => top + m[1],
        };
        pos.0 = match v {
            TextVAlign::Top => left + m[0],
            TextVAlign::Center => (left + right) / 2,
            TextVAlign::Bottom => right - m[2],
        };
    } else {
        pos.0 = match h {
            TextJustify::Left => left + m[0],
            TextJustify::Center => (left + right) / 2,
            TextJustify::Right => right - m[2],
        };
        pos.1 = match v {
            TextVAlign::Top => top + m[1],
            TextVAlign::Center => (top + bottom) / 2,
            TextVAlign::Bottom => bottom - m[3],
        };
    }
    pos
}

/// A cell of a sheet's table ready to draw. `sheet` resolves the sheet's variables.
pub fn sch_cell_draw(t: &Table, row: usize, col: usize, sheet: &Resolver) -> CellDraw {
    let cell: &TableCell = t.cell(row, col).expect("a cell of the table");
    let shown = sch_cell_shown(t, row, col, sheet, 0);
    let pen_nm = effective_pen_nm(&cell.style, SCH_DEFAULT_PEN_NM);
    let lines = if cell.is_covered() { Vec::new() } else { place_lines_at(&cell.style, pen_nm as f64, sch_draw_pos_nm(t, row, col), t.angle, &shown) };
    CellDraw { shown, lines, angle: t.angle, mirror: false, pen_nm: pen_nm as f64 }
}

/// `Point` of a placed line, rounded to the nanometre (for a hit test).
pub fn rounded(l: &PlacedLine) -> Point {
    Point { x: l.x_nm.round() as Nm, y: l.y_nm.round() as Nm }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::table::{TableBorder, TableSeparators, TableStroke, TableText};

    fn one_cell(text: &str, w: Nm, h: Nm, style: TableText) -> Table {
        let mut cell = TableCell::pcb_new(false);
        cell.text = text.into();
        cell.style = style;
        cell.margins = [1_000_000; 4];
        Table {
            id: "tbl_t".into(),
            uuid: String::new(),
            layer: "F.SilkS".into(),
            at: Point { x: 10_000_000, y: 20_000_000 },
            angle: 0,
            col_widths: vec![w],
            row_heights: vec![h],
            cells: vec![cell],
            border: TableBorder { external: true, header: true, stroke: TableStroke::solid(100_000) },
            separators: TableSeparators { rows: true, cols: true, stroke: TableStroke::solid(100_000) },
        }
    }

    fn style(size_nm: Nm, thickness_nm: Nm) -> TableText {
        TableText { height_nm: size_nm, width_nm: size_nm, thickness_nm, ..TableText::pcb_default() }
    }

    #[test]
    fn cell_addresses_are_spreadsheet_names_from_one() {
        assert_eq!(pcb_cell_addr(0, 0), "A1");
        assert_eq!(pcb_cell_addr(2, 1), "B3");
        assert_eq!(column_letter(27), 'B');
    }

    #[test]
    fn variables_are_resolved_with_rows_and_columns_counted_from_one() {
        let board = PcbBoard::default();
        assert_eq!(pcb_cell_resolved("${ADDR} ${ROW}/${COL} on ${LAYER}", "F.SilkS", 2, 1, &board), "B3 3/2 on F.Silkscreen");
        // a variable nobody knows stays as written, and an escaped one is shown literally
        assert_eq!(pcb_cell_resolved("${NOPE} \\${ROW}", "F.SilkS", 0, 0, &board), "${NOPE} ${ROW}");
    }

    #[test]
    fn a_line_is_broken_where_its_next_word_no_longer_fits() {
        // 1.5 mm glyphs: "Board" is 5 glyphs of about 1.4 mm; a column of 12 mm less the pen holds one or two words
        let size = 1_500_000;
        let one = linebreak_text("alpha beta gamma", 100_000_000, size, 200_000);
        assert_eq!(one, "alpha beta gamma", "a wide column keeps one line");
        let narrow = linebreak_text("alpha beta gamma", 12_000_000, size, 200_000);
        assert!(narrow.contains('\n'), "{narrow}");
        // the words are all there, in order, with a line feed or a space between each
        assert_eq!(narrow.split_whitespace().collect::<Vec<_>>(), vec!["alpha", "beta", "gamma"]);
        // a word wider than the column stays whole
        let long = linebreak_text("supercalifragilistic x", 5_000_000, size, 200_000);
        assert!(long.starts_with("supercalifragilistic"), "{long}");
        // explicit line feeds are kept
        assert_eq!(linebreak_text("a\nb", 100_000_000, size, 0), "a\nb");
    }

    #[test]
    fn a_left_centred_cell_anchors_text_a_margin_in_from_the_middle_of_its_left_edge() {
        let t = one_cell("Layer", 20_000_000, 6_000_000, style(1_500_000, 200_000));
        // mid-left of a cell at (10, 20) of 20 x 6 mm is (10, 23); the margin is 1 mm in
        assert_eq!(draw_pos_nm(&t, 0, 0), (11_000_000, 23_000_000));
        // top and right: the corner, and the margin inward
        let mut t2 = t.clone();
        t2.cells[0].style.v_align = TextVAlign::Top;
        t2.cells[0].style.h_align = TextJustify::Right;
        assert_eq!(draw_pos_nm(&t2, 0, 0), (30_000_000 - 1_000_000, 20_000_000 + 1_000_000));
        // a mirrored left-justified cell is anchored at the right
        let mut t3 = t.clone();
        t3.cells[0].style.mirror = true;
        assert_eq!(draw_pos_nm(&t3, 0, 0), (30_000_000 - 1_000_000, 23_000_000));
    }

    #[test]
    fn the_first_baseline_of_a_one_line_cell_is_where_the_legacy_layout_puts_it() {
        // font.cpp getLinePositions: y = size - thickness * 0.052 - 1.17 * size / 2 about the centre, x = thickness / 1.52 in from the anchor.
        let t = one_cell("L", 20_000_000, 6_000_000, style(1_500_000, 200_000));
        let lines = place_lines(&t, 0, 0, "L");
        assert_eq!(lines.len(), 1);
        let (cx, cy) = draw_pos_nm(&t, 0, 0);
        let pen = 200_000.0f64;
        let want_y = cy as f64 + (1_500_000.0 - pen * 0.052).trunc() - ((1_500_000.0f64 * 1.17).trunc() / 2.0).trunc();
        assert_eq!(lines[0].y_nm, want_y);
        assert_eq!(lines[0].x_nm, (cx + (pen / 1.52) as i64) as f64);
    }

    #[test]
    fn a_turned_table_turns_its_lines_about_the_draw_position() {
        let mut t = one_cell("L", 20_000_000, 6_000_000, style(1_500_000, 200_000));
        let flat = place_lines(&t, 0, 0, "L");
        let flat_origin = draw_pos_nm(&t, 0, 0);
        t.angle = 90_000;
        let turned = place_lines(&t, 0, 0, "L");
        let origin = draw_pos_nm(&t, 0, 0);
        // the line's offset from the origin is the flat one turned a quarter: (x, y) -> (y, -x)
        let (fx, fy) = (flat[0].x_nm - flat_origin.0 as f64, flat[0].y_nm - flat_origin.1 as f64);
        let (tx, ty) = (turned[0].x_nm - origin.0 as f64, turned[0].y_nm - origin.1 as f64);
        assert_eq!((tx, ty), (fy, -fx));
    }

    #[test]
    fn a_cell_that_is_covered_draws_nothing() {
        let mut t = one_cell("x", 20_000_000, 6_000_000, style(1_500_000, 200_000));
        t.cells[0].col_span = 0;
        assert!(pcb_cell_draw(&t, 0, 0, &PcbBoard::default()).lines.is_empty());
    }

    // ------------------------------------------------------------------------------------------------------------------------------ a sheet's table

    /// A `rows` x `cols` sheet table of 20 x 5 mm cells at (100, 50) mm: text `A0`..., left and top, a 1 mm margin, 1.27 mm glyphs.
    fn sheet(rows: usize, cols: usize) -> Table {
        let mut t = Table::new_sch_grid(Point { x: 100_000_000, y: 50_000_000 }, cols, rows, 20_000_000, 5_000_000, 1_270_000);
        for (i, c) in t.cells.iter_mut().enumerate() {
            c.margins = [1_000_000; 4];
            c.text = format!("t{}", i);
        }
        t
    }

    fn no_vars(_: &str) -> Option<String> {
        None
    }

    #[test]
    fn a_sheets_cell_addresses_and_rows_and_columns_count_from_zero() {
        assert_eq!(sch_cell_addr(0, 0), "A0");
        assert_eq!(sch_cell_addr(3, 1), "B3");
        let mut t = sheet(2, 2);
        t.cells[3].text = "${ADDR} ${ROW},${COL}".into();
        assert_eq!(sch_cell_shown(&t, 1, 1, &no_vars, 0), "B1 1,1");
        // the sheet's own variables come after the cell's
        let sheet_vars = |tok: &str| (tok == "TITLE").then(|| "My sheet".to_string());
        t.cells[0].text = "${TITLE} ${NOPE}".into();
        assert_eq!(sch_cell_shown(&t, 0, 0, &sheet_vars, 0), "My sheet ${NOPE}");
    }

    #[test]
    fn a_cell_can_show_another_cells_text() {
        let mut t = sheet(2, 2);
        t.cells[1].text = "total".into();
        t.cells[2].text = "${CELL(\"B0\")}!".into();
        t.cells[3].text = "${CELL(0, 1)} and ${CELL( 1 , 0 )} and ${CELL(\"c9\")} ${CELL(7,7)} ${CELL(x)} ${CELL(\"??\")}".into();
        assert_eq!(sch_cell_shown(&t, 1, 0, &no_vars, 0), "total!");
        // (the answers are long and the column is 20 mm: the lines are broken, as KiCad breaks them)
        let text = sch_cell_shown(&t, 1, 1, &no_vars, 0).replace('\n', " ");
        assert!(text.contains("total and total!"), "{text}");
        assert!(text.contains("<Unresolved: Cell C9 not found>"), "{text}");
        assert!(text.contains("<Unresolved: Cell H7 not found>"), "{text}");
        assert!(text.contains("<Unresolved: Invalid CELL() syntax: x>"), "{text}");
        assert!(text.contains("<Unresolved: Invalid cell address: ??>"), "{text}");
        // a cell that shows itself ends at the depth limit
        let mut loopy = sheet(1, 2);
        loopy.cells[0].text = "${CELL(\"A0\")}".into();
        assert!(sch_cell_shown(&loopy, 0, 0, &no_vars, 0).replace('\n', " ").contains("<Circular reference>"));
    }

    #[test]
    fn a_sheet_cell_anchors_its_text_by_its_justification_with_the_margin_of_that_side() {
        let mut t = sheet(1, 1);
        // left, top: a margin in from the top left corner of the box (100, 50) mm
        assert_eq!(sch_draw_pos_nm(&t, 0, 0), (101_000_000, 51_000_000));
        t.cells[0].style.h_align = TextJustify::Right;
        t.cells[0].style.v_align = TextVAlign::Bottom;
        t.cells[0].margins = [1_000_000, 2_000_000, 3_000_000, 4_000_000];
        assert_eq!(sch_draw_pos_nm(&t, 0, 0), (120_000_000 - 3_000_000, 55_000_000 - 4_000_000));
        t.cells[0].style.h_align = TextJustify::Center;
        t.cells[0].style.v_align = TextVAlign::Center;
        assert_eq!(sch_draw_pos_nm(&t, 0, 0), (110_000_000, 52_500_000));
    }

    #[test]
    fn a_table_that_stands_anchors_along_its_height_and_breaks_to_its_width() {
        let mut t = sheet(1, 1);
        t.angle = 90_000;
        t.cells[0].margins = [1_000_000, 2_000_000, 3_000_000, 4_000_000];
        // the box of the cell is turned a quarter about the corner: 5 mm wide and 20 mm high now, above and to the right of the corner? (the grid runs up the sheet)
        let c = t.cell_corners(0, 0);
        let (x0, x1) = (c[0].x.min(c[2].x), c[0].x.max(c[2].x));
        let (y0, y1) = (c[0].y.min(c[2].y), c[0].y.max(c[2].y));
        assert_eq!((x1 - x0, y1 - y0), (5_000_000, 20_000_000));
        // left justified: along the text, which now runs up, from the bottom less the bottom margin; top justified: across, from the left plus the left margin
        let (px, py) = sch_draw_pos_nm(&t, 0, 0);
        assert_eq!((px, py), (x0 + 1_000_000, y1 - 4_000_000));
        // the column the text is broken to is the 20 mm of the cell less the top and bottom margins
        let long = "alpha beta gamma delta epsilon zeta eta theta";
        let wrapped = sch_wrap_to_cell(&t, 0, 0, long);
        assert_eq!(wrapped, linebreak_text(long, 20_000_000 - 6_000_000, 1_270_000, 159_000));
    }

    #[test]
    fn the_text_of_a_sheet_is_drawn_with_the_sheets_default_pen_where_a_boards_is_an_eighth_of_the_glyph() {
        let plain = TableText::sch_default(1_270_000);
        assert_eq!(effective_pen_nm(&plain, SCH_DEFAULT_PEN_NM), 152_400);
        let bold = TableText { bold: true, ..plain.clone() };
        assert_eq!(effective_pen_nm(&bold, SCH_DEFAULT_PEN_NM), 254_000, "a fifth of the glyph");
        let own = TableText { thickness_nm: 200_000, ..plain.clone() };
        assert_eq!(effective_pen_nm(&own, SCH_DEFAULT_PEN_NM), 200_000);
        let huge = TableText { thickness_nm: 900_000, ..plain.clone() };
        assert_eq!(effective_pen_nm(&huge, SCH_DEFAULT_PEN_NM), 317_500, "at most a quarter of the glyph");
        assert_eq!(effective_pen_nm(&plain, 0), 158_750, "with no default pen it is the board's eighth");
    }

    #[test]
    fn a_sheet_cell_is_placed_like_the_first_cell_of_the_qa_sheet() {
        // BusAndVectors.kicad_sch: 1.27 mm text, left and top, margin 0.9525, a cell at (122.555, 29.21) mm: kicad-cli draws the text a margin in from the corner.
        let mut t = Table::new_sch_grid(Point { x: 122_555_000, y: 29_210_000 }, 2, 1, 26_670_000, 2_540_000, 1_270_000);
        t.cells[0].text = "LED pin (by VectorIndex)".into();
        let draw = sch_cell_draw(&t, 0, 0, &no_vars);
        assert_eq!(draw.shown, "LED pin (by VectorIndex)", "the 26.67 mm column holds it whole");
        assert_eq!(draw.lines.len(), 1);
        // x: the anchor (the corner and the 0.9525 mm margin) plus thickness / 1.52 (a 152.4 um pen); y: the anchor plus the glyph height less 0.052 of
        // the pen, each cut to an integer as KiCad's `getLinePositions` does
        let l = &draw.lines[0];
        assert_eq!(l.x_nm, (123_507_500 + (152_400.0f64 / 1.52) as i64) as f64);
        assert_eq!(l.y_nm, (30_162_500 + (1_270_000.0f64 - 152_400.0 * 0.052) as i64) as f64);
        assert_eq!(draw.pen_nm, 152_400.0);
    }

    #[test]
    fn rotating_a_sheet_table_toggles_its_attitude_about_the_origin_the_turn_puts_cell_zero_at() {
        let mut t = sheet(2, 2);
        let centre = Point { x: 110_000_000, y: 50_000_000 };
        t.rotate_sch(centre, true);
        // (100, 50) turned a quarter counter-clockwise about (110, 50): 10 mm to the left becomes 10 mm down the screen? rotate_point(-10, 0, 90) = (0, 10)
        assert_eq!((t.at, t.angle), (Point { x: 110_000_000, y: 60_000_000 }, 90_000));
        t.rotate_sch(centre, true);
        // the second quarter does not continue the first: the attitude is lying again, from where cell zero went
        assert_eq!(t.angle, 0);
        assert_eq!(t.at, Point { x: 120_000_000, y: 50_000_000 });
        t.rotate_sch(centre, false);
        assert_eq!(t.angle, 90_000);
    }
}
