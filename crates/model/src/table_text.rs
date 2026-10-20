//! What a table cell shows and where it puts its lines: the text of a `PCB_TABLECELL` after its variables are resolved and it is broken to the width of its
//! column, and the place of each line as `PCB_TEXTBOX::GetDrawPos` and `EDA_TEXT::GetLinePositions` / `FONT::getLinePositions` put it.
//!
//! Everything here is worked out in nanometres, the unit KiCad's board works in, with `KiROUND` where KiCad rounds, and handed back in micrometres:
//! a table's cells are on the micrometre grid of the design, but the width of a word, the margin of a cell and the offset of a line are KiCad's.
//!
//! Ported from:
//!   pcbnew/pcb_tablecell.cpp       `PCB_TABLECELL::GetShownText` (`${ROW}`, `${COL}`, `${ADDR}`, `${LAYER}`, then the board's variables, then `LinebreakText`)
//!   pcbnew/pcb_textbox.cpp         `PCB_TEXTBOX::GetDrawPos`
//!   common/font/font.cpp           `FONT::LinebreakText`, `wordbreakMarkup`, `getLinePositions`, `Draw`
//!   common/font/stroke_font.cpp    `STROKE_FONT::GetTextAsGlyphs` (the advance of a run), `GetInterline` (the legacy factor)
//!   common/eda_text.cpp            `EDA_TEXT::GetLinePositions`, `GetEffectiveTextPenWidth`

use crate::ir::{Point, TextJustify, TextVAlign, Um};
use crate::kicad_font::{advance, kiround};
use crate::table::{rotate_point, Table, TableCell};
use crate::text_vars::{shown_text, PcbBoard};

/// One line of a cell's text, placed: the point its first glyph's origin is at once the cell's turn and mirror are applied, µm on the board (fractions kept:
/// KiCad's own are nanometres).
#[derive(Debug, Clone, PartialEq)]
pub struct PlacedLine {
    pub text: String,
    pub x_um: f64,
    pub y_um: f64,
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
    /// The stroke width the glyphs are drawn with, µm (`GetEffectiveTextPenWidth`).
    pub pen_um: f64,
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

fn nm(um: Um) -> i64 {
    um * 1000
}

/// `PCB_TEXTBOX::GetDrawPos()`: where the text is anchored in a cell, nanometres: the corner or middle of the cell its justification names, taken from the
/// corners as the table reads them, and moved in by the margin of that side, turned by the cell's angle. A cell whose text is mirrored is anchored on the
/// other side (`IsMirrored() != aIsFlipped` swaps left and right).
pub fn draw_pos_nm(t: &Table, row: usize, col: usize) -> (i64, i64) {
    let cell = t.cell(row, col).expect("a cell of the table");
    let c = t.cell_corners(row, col).map(|p| (nm(p.x), nm(p.y)));
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
        TextJustify::Left => off.0 = nm(cell.margins[0]),
        TextJustify::Right => off.0 = -nm(cell.margins[2]),
        TextJustify::Center => {}
    }
    match v {
        TextVAlign::Top => off.1 = nm(cell.margins[1]),
        TextVAlign::Bottom => off.1 = -nm(cell.margins[3]),
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
    let mut width = kiround((dx * dx + dy * dy).sqrt() * 1000.0);
    let horizontal = t.angle % 180_000 == 0;
    width -= if horizontal { nm(cell.margins[0] + cell.margins[2]) } else { nm(cell.margins[1] + cell.margins[3]) };
    linebreak_text(resolved, width, nm(cell.style.width_um), nm(cell.style.pen_um()))
}

/// The lines of `shown` (already broken) as `EDA_TEXT::GetLinePositions` places them for the plot and `FONT::Draw` for the canvas -- the two come to the same
/// place: the first line's baseline is `size.y - thickness * 0.052` under the top of the block, the block is as high as `1.17` lines plus the others, and a
/// line starts `thickness / 1.52` from its anchor, or is centred or ended on it. Each line is then turned about the cell's draw position and, when the text is
/// mirrored, mirrored about it.
pub fn place_lines(t: &Table, row: usize, col: usize, shown: &str) -> Vec<PlacedLine> {
    let Some(cell) = t.cell(row, col) else { return Vec::new() };
    let style = &cell.style;
    let (size_w, size_h) = (nm(style.width_um) as f64, nm(style.height_um) as f64);
    let pen = nm(style.pen_um()) as f64;
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
    let origin = draw_pos_nm(t, row, col);
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
            let (rx, ry) = rotate_point(mx, line_y, t.angle);
            PlacedLine { text: (*line).to_string(), x_um: (origin.0 + rx) as f64 / 1000.0, y_um: (origin.1 + ry) as f64 / 1000.0 }
        })
        .collect()
}

/// A board cell ready to draw. `board` resolves the variables.
pub fn pcb_cell_draw(t: &Table, row: usize, col: usize, board: &PcbBoard) -> CellDraw {
    let cell: &TableCell = t.cell(row, col).expect("a cell of the table");
    let resolved = pcb_cell_resolved(&cell.text, &t.layer, row, col, board);
    let shown = wrap_to_cell(t, row, col, &resolved);
    let lines = if cell.is_covered() { Vec::new() } else { place_lines(t, row, col, &shown) };
    CellDraw { shown, lines, angle: t.angle, mirror: cell.style.mirror, pen_um: cell.style.pen_um() as f64 }
}

/// `Point` of a placed line, rounded to the micrometre (for a hit test).
pub fn rounded(l: &PlacedLine) -> Point {
    Point { x: l.x_um.round() as Um, y: l.y_um.round() as Um }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::table::{TableBorder, TableSeparators, TableStroke, TableText};

    fn one_cell(text: &str, w: Um, h: Um, style: TableText) -> Table {
        let mut cell = TableCell::pcb_new(false);
        cell.text = text.into();
        cell.style = style;
        cell.margins = [1_000; 4];
        Table {
            id: "tbl_t".into(),
            uuid: String::new(),
            layer: "F.SilkS".into(),
            at: Point { x: 10_000, y: 20_000 },
            angle: 0,
            col_widths: vec![w],
            row_heights: vec![h],
            cells: vec![cell],
            border: TableBorder { external: true, header: true, stroke: TableStroke::solid(100) },
            separators: TableSeparators { rows: true, cols: true, stroke: TableStroke::solid(100) },
        }
    }

    fn style(size_um: Um, thickness_um: Um) -> TableText {
        TableText { height_um: size_um, width_um: size_um, thickness_um, ..TableText::pcb_default() }
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
        let t = one_cell("Layer", 20_000, 6_000, style(1_500, 200));
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
        let t = one_cell("L", 20_000, 6_000, style(1_500, 200));
        let lines = place_lines(&t, 0, 0, "L");
        assert_eq!(lines.len(), 1);
        let (cx, cy) = draw_pos_nm(&t, 0, 0);
        let pen = 200_000.0f64;
        let want_y = cy as f64 + (1_500_000.0 - pen * 0.052).trunc() - ((1_500_000.0f64 * 1.17).trunc() / 2.0).trunc();
        assert_eq!(lines[0].y_um * 1000.0, want_y);
        assert_eq!(lines[0].x_um * 1000.0, (cx + (pen / 1.52) as i64) as f64);
    }

    #[test]
    fn a_turned_table_turns_its_lines_about_the_draw_position() {
        let mut t = one_cell("L", 20_000, 6_000, style(1_500, 200));
        let flat = place_lines(&t, 0, 0, "L");
        let flat_origin = draw_pos_nm(&t, 0, 0);
        t.angle = 90_000;
        let turned = place_lines(&t, 0, 0, "L");
        let origin = draw_pos_nm(&t, 0, 0);
        // the line's offset from the origin is the flat one turned a quarter: (x, y) -> (y, -x)
        let (fx, fy) = (flat[0].x_um * 1000.0 - flat_origin.0 as f64, flat[0].y_um * 1000.0 - flat_origin.1 as f64);
        let (tx, ty) = (turned[0].x_um * 1000.0 - origin.0 as f64, turned[0].y_um * 1000.0 - origin.1 as f64);
        assert_eq!((tx, ty), (fy, -fx));
    }

    #[test]
    fn a_cell_that_is_covered_draws_nothing() {
        let mut t = one_cell("x", 20_000, 6_000, style(1_500, 200));
        t.cells[0].col_span = 0;
        assert!(pcb_cell_draw(&t, 0, 0, &PcbBoard::default()).lines.is_empty());
    }
}
