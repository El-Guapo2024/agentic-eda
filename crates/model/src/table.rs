//! Tables: KiCad's `PCB_TABLE` and `SCH_TABLE` with their `PCB_TABLECELL` / `SCH_TABLECELL`s, one type for the board and for the sheet.
//!
//! A table is a grid. Its columns have widths, its rows heights, and its cells are text boxes laid out row by row on that grid; a cell can be merged with
//! the ones to its right and below (`span`), and then the cells it covers are kept in the list with a span of 0 and no text. KiCad stores every cell's
//! corners as well, but those are always the grid's (`PCB_TABLE::Normalize`, `SCH_TABLE::Normalize` lay them out after every edit), so they are not kept
//! here: a table is its origin, its angle, its widths and heights and its cells' looks, and the corners are worked out from them ([`Table::cell_corners`]).
//!
//! * `at` is where cell (0, 0)'s top-left corner is, in the table's own frame (`PCB_TABLE::GetPosition`); `angle` turns the whole grid about it
//!   (`EDA_TEXT::GetTextAngle` of the cells, which all share it), counter-clockwise on the screen as every angle in a KiCad file.
//! * Lengths are nanometres. A board's `layer` is a KiCad layer name; a sheet's table has none.
//! * A stroke width of -1 is KiCad's "not drawn" (`STROKE_PARAMS` of a border or separator the table does not have).
//! * The edit operations that `EDIT_TABLE_TOOL_BASE` (`include/tool/edit_table_tool_base.h`) shares between the two editors are methods of [`Table`]:
//!   [`Table::add_row`], [`Table::add_column`], [`Table::delete_rows`], [`Table::delete_columns`], [`Table::merge`] and [`Table::unmerge`]. Each one is
//!   a port of the `doXxx` function of that header and works on any table, whichever editor it is in.
//!
//! The data is additive: `DrawingsSection::tables` (a board's) and `SchExtras::tables` (a sheet's) are empty in a design that has none and are never
//! written when empty.

use crate::ir::{Millideg, Point, TextJustify, TextVAlign, Nm};
use crate::sch_extras::{SchColor, SchFill, SchLineStyle};
use serde::{Deserialize, Serialize};

/// `STROKE_PARAMS::GetColor` of a table's border or separators.
pub type TableColor = SchColor;
/// `LINE_STYLE`.
pub type TableLineStyle = SchLineStyle;

fn is_false(b: &bool) -> bool {
    !*b
}

fn is_zero(n: &Nm) -> bool {
    *n == 0
}

fn d_true() -> bool {
    true
}

fn is_true(b: &bool) -> bool {
    *b
}

fn d_one() -> u32 {
    1
}

fn is_one(n: &u32) -> bool {
    *n == 1
}

fn d_none_width() -> Nm {
    -1
}

/// The id of cell (`row`, `col`) of the table `table` as an item of the editor (a click on a cell selects the cell, not the table: see `PCB_SELECTION_TOOL`
/// `hitTestDistance`): `<table id>:<row>:<col>`, rows and columns counted from 0.
pub fn cell_item_id(table: &str, row: usize, col: usize) -> String {
    format!("{table}:{row}:{col}")
}

/// The table id, row and column of a [`cell_item_id`]; `None` for any other id.
pub fn parse_cell_item_id(id: &str) -> Option<(&str, usize, usize)> {
    let (rest, col) = id.rsplit_once(':')?;
    let (table, row) = rest.rsplit_once(':')?;
    if table.is_empty() {
        return None;
    }
    Some((table, row.parse().ok()?, col.parse().ok()?))
}

/// `STROKE_PARAMS` of a table's border or of its separators.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TableStroke {
    /// The line width; -1 is "no stroke" (the width a table has when the file says none, `PCB_TABLE( aParent, -1 )`), 0 the default width of the editor.
    #[serde(default = "d_none_width")]
    #[serde(alias = "width_um", deserialize_with = "crate::units::de_width")]
    pub width_nm: Nm,
    #[serde(default, skip_serializing_if = "TableLineStyle::is_default")]
    pub style: TableLineStyle,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<TableColor>,
}

impl Default for TableStroke {
    /// What a table the file gave no stroke has: width -1, `LINE_STYLE::DEFAULT`, no colour.
    fn default() -> Self {
        TableStroke { width_nm: -1, style: TableLineStyle::Default, color: None }
    }
}

impl TableStroke {
    /// A stroke of `width_nm`, solid and uncoloured.
    pub fn solid(width_nm: Nm) -> Self {
        TableStroke { width_nm, style: TableLineStyle::Solid, color: None }
    }

    /// `PCB_TABLE::GetBorderStyle`: the style a line is drawn in (`DEFAULT` is solid).
    pub fn drawn_style(&self) -> TableLineStyle {
        if self.style == TableLineStyle::Default {
            TableLineStyle::Solid
        } else {
            self.style
        }
    }
}

/// `(border (external yes|no) (header yes|no) [(stroke ..)])`: the table's outline and the line under its first row.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TableBorder {
    /// `PCB_TABLE::StrokeExternal`: the four sides of the table are drawn.
    #[serde(default = "d_true", skip_serializing_if = "is_true")]
    pub external: bool,
    /// `PCB_TABLE::StrokeHeaderSeparator`: the line under the first row is drawn (with the border's stroke).
    #[serde(default = "d_true", skip_serializing_if = "is_true")]
    pub header: bool,
    #[serde(default)]
    pub stroke: TableStroke,
}

impl Default for TableBorder {
    fn default() -> Self {
        TableBorder { external: true, header: true, stroke: TableStroke::default() }
    }
}

/// `(separators (rows yes|no) (cols yes|no) [(stroke ..)])`: the lines between the rows and between the columns.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TableSeparators {
    #[serde(default = "d_true", skip_serializing_if = "is_true")]
    pub rows: bool,
    #[serde(default = "d_true", skip_serializing_if = "is_true")]
    pub cols: bool,
    #[serde(default)]
    pub stroke: TableStroke,
}

impl Default for TableSeparators {
    fn default() -> Self {
        TableSeparators { rows: true, cols: true, stroke: TableStroke::default() }
    }
}

/// The looks of a cell's text, `EDA_TEXT`'s attributes: `(effects (font (face ..) (size H W) (line_spacing ..) (thickness ..) bold italic (color ..))
/// (justify ..))`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TableText {
    /// The glyph height, nm (`GetTextHeight`).
    #[serde(alias = "height_um", deserialize_with = "crate::units::de")]
    pub height_nm: Nm,
    /// The glyph width, nm (`GetTextWidth`).
    #[serde(alias = "width_um", deserialize_with = "crate::units::de")]
    pub width_nm: Nm,
    /// The stroke width of the glyphs; 0 is KiCad's automatic one (`GetAutoThickness`: no `(thickness ..)` in the file).
    #[serde(default, skip_serializing_if = "is_zero")]
    #[serde(alias = "thickness_um", deserialize_with = "crate::units::de")]
    pub thickness_nm: Nm,
    #[serde(default, skip_serializing_if = "is_false")]
    pub bold: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub italic: bool,
    #[serde(default)]
    pub h_align: TextJustify,
    #[serde(default)]
    pub v_align: TextVAlign,
    /// `(justify .. mirror)`: the glyphs are mirrored, as text on a back layer is.
    #[serde(default, skip_serializing_if = "is_false")]
    pub mirror: bool,
    /// `(face "..")`: the outline font the text is set in. Stroke text, the only kind drawn here, has none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub face: Option<String>,
    /// `(line_spacing N)` in thousandths; 0 is KiCad's 1.0, and 1000 is the same.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub line_spacing_permille: i64,
    /// `(color r g b a)` of the font.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<TableColor>,
}

impl TableText {
    /// A sheet's text box until something sets it (`SCH_TEXTBOX`): glyphs `size_nm` high (the sheet's default text size, 50 mil), automatic thickness, left and top.
    pub fn sch_default(size_nm: Nm) -> Self {
        TableText { height_nm: size_nm, width_nm: size_nm, v_align: TextVAlign::Top, ..TableText::pcb_default() }
    }

    /// What an `EDA_TEXT` is until something sets it: 50 mil glyphs (`DEFAULT_SIZE_TEXT`), automatic thickness, and the justification a text box starts with
    /// (`PCB_TEXTBOX`: left, centre).
    pub fn pcb_default() -> Self {
        TableText { height_nm: 1_270_000, width_nm: 1_270_000, thickness_nm: 0, bold: false, italic: false, h_align: TextJustify::Left, v_align: TextVAlign::Center, mirror: false, face: None, line_spacing_permille: 0, color: None }
    }

    /// `EDA_TEXT::GetEffectiveTextPenWidth`: the stroke width the glyphs are drawn with, which is the automatic one (an eighth of the glyph width, a fifth if
    /// bold) when none is set, never more than a quarter of the smaller glyph dimension (`ClampTextPenSize`).
    pub fn pen_nm(&self) -> Nm {
        let mut pen = self.thickness_nm;
        if pen <= 1 {
            pen = (self.width_nm as f64 / if self.bold { 5.0 } else { 8.0 }).round() as Nm;
        }
        let max = (self.width_nm.abs().min(self.height_nm.abs()) as f64 * 0.25).round() as Nm;
        pen.min(max)
    }

    /// `EDA_TEXT::GetLineSpacing`: 1.0 unless the file says otherwise.
    pub fn line_spacing(&self) -> f64 {
        if self.line_spacing_permille == 0 {
            1.0
        } else {
            self.line_spacing_permille as f64 / 1000.0
        }
    }
}

impl Default for TableText {
    fn default() -> Self {
        TableText::pcb_default()
    }
}

/// One cell: the text box `PCB_TABLECELL` / `SCH_TABLECELL` is, without the corners the grid decides.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TableCell {
    /// The uuid the cell had in the file it was read from; empty for a cell made here (the writer makes one).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub uuid: String,
    /// The text as typed, `${VAR}` references and all (`EDA_TEXT::GetText`); what is drawn is [`crate::table_text`]'s business.
    #[serde(default)]
    pub text: String,
    /// `GetColSpan`: how many columns the cell takes; 0 for a cell another one covers.
    #[serde(default = "d_one", skip_serializing_if = "is_one")]
    pub col_span: u32,
    /// `GetRowSpan`: how many rows; 0 for a cell another one covers.
    #[serde(default = "d_one", skip_serializing_if = "is_one")]
    pub row_span: u32,
    /// `(margins left top right bottom)`, nm.
    #[serde(deserialize_with = "crate::units::de")]
    pub margins: [Nm; 4],
    #[serde(default)]
    pub style: TableText,
    /// A sheet's cell (`SCH_TABLECELL` is an `SCH_SHAPE`): the background. A board's cell has none.
    #[serde(default, skip_serializing_if = "SchFill::is_none")]
    pub fill: SchFill,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fill_color: Option<TableColor>,
    /// A sheet's cell: `(exclude_from_sim yes)`.
    #[serde(default, skip_serializing_if = "is_false")]
    pub exclude_from_sim: bool,
}

impl TableCell {
    /// `PCB_TEXTBOX::GetLegacyTextMargin`: `KiROUND( GetStroke().GetWidth() / 2.0 ) + KiROUND( GetTextSize().y * 0.75 )`, which is what a cell has until a
    /// margin is set -- 1.0025 mm for a new cell's 1.27 mm text on a 0.1 mm stroke (the shape default).
    pub fn legacy_margin_nm(text: &TableText, stroke_nm: Nm) -> Nm {
        crate::units::kiround(stroke_nm as f64 / 2.0) + crate::units::kiround(text.height_nm as f64 * 0.75)
    }

    /// Margin `i` (left, top, right, bottom): what `Autosize` lays a table out with and the writer writes.
    pub fn margin_nm(&self, i: usize) -> Nm {
        self.margins[i]
    }

    /// A new board cell (`new PCB_TABLECELL( table )`): an empty text box of KiCad's defaults, mirrored when the table is on a back layer
    /// (`SetMirrored( board->IsBackLayer( aParent->GetLayer() ) )`).
    pub fn pcb_new(back: bool) -> Self {
        let mut style = TableText::pcb_default();
        style.mirror = back;
        let margin = Self::legacy_margin_nm(&style, 100_000);
        TableCell { uuid: String::new(), text: String::new(), col_span: 1, row_span: 1, margins: [margin; 4], style, fill: SchFill::None, fill_color: None, exclude_from_sim: false }
    }

    /// A new sheet cell (`new SCH_TABLECELL()`, then `SetTextSize` to the sheet's default text size, as Draw Table does): empty, no fill, left and top, and the margin the
    /// constructor gave it -- the legacy margin of 50 mil text on a stroke 0 wide.
    pub fn sch_new(text_size_nm: Nm) -> Self {
        let margin = Self::legacy_margin_nm(&TableText::sch_default(1_270_000), 0);
        TableCell { uuid: String::new(), text: String::new(), col_span: 1, row_span: 1, margins: [margin; 4], style: TableText::sch_default(text_size_nm), fill: SchFill::None, fill_color: None, exclude_from_sim: false }
    }

    /// Is this cell covered by a merged one?
    pub fn is_covered(&self) -> bool {
        self.col_span == 0 || self.row_span == 0
    }
}

/// One table. See the module doc.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Table {
    /// Stable id (`tbl_xxxxxxxxxxxx`), made when the table is read, as every drawing's is.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub id: String,
    /// The uuid the table had in its file; empty for one made here.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub uuid: String,
    /// A board table's KiCad layer (`F.SilkS`, `Dwgs.User` ..); empty on a sheet.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub layer: String,
    /// The top-left corner of cell (0, 0), nm (`PCB_TABLE::GetPosition`).
    pub at: Point,
    /// The grid's angle about [`Table::at`], millidegrees, counter-clockwise on the screen.
    #[serde(default)]
    pub angle: Millideg,
    /// The column widths, left to right, nm. The column count is its length.
    #[serde(deserialize_with = "crate::units::de")]
    pub col_widths: Vec<Nm>,
    /// The row heights, top to bottom, nm.
    #[serde(deserialize_with = "crate::units::de")]
    pub row_heights: Vec<Nm>,
    /// The cells, row by row: `row_heights.len() * col_widths.len()` of them.
    pub cells: Vec<TableCell>,
    #[serde(default)]
    pub border: TableBorder,
    #[serde(default)]
    pub separators: TableSeparators,
}

/// `RotatePoint( VECTOR2I&, EDA_ANGLE )` (`libs/kimath/src/trigo.cpp`): the turn is counter-clockwise on a screen whose y runs down, exact for the quarter
/// turns and rounded (`KiROUND`) for any other angle.
pub fn rotate_point(x: Nm, y: Nm, angle: Millideg) -> (Nm, Nm) {
    match angle % 360_000 {
        0 => (x, y),
        90_000 => (y, -x),
        180_000 => (-x, -y),
        270_000 => (-y, x),
        a => {
            let (s, c) = (a as f64 / 1000.0).to_radians().sin_cos();
            (((y as f64 * s) + (x as f64 * c)).round() as Nm, ((y as f64 * c) - (x as f64 * s)).round() as Nm)
        }
    }
}

/// An axis-aligned box, nm.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TableBox {
    pub x0: Nm,
    pub y0: Nm,
    pub x1: Nm,
    pub y1: Nm,
}

impl TableBox {
    fn of(points: &[Point]) -> TableBox {
        let mut b = TableBox { x0: Nm::MAX, y0: Nm::MAX, x1: Nm::MIN, y1: Nm::MIN };
        for p in points {
            b.x0 = b.x0.min(p.x);
            b.y0 = b.y0.min(p.y);
            b.x1 = b.x1.max(p.x);
            b.y1 = b.y1.max(p.y);
        }
        b
    }

    pub fn contains(&self, p: Point, slack: Nm) -> bool {
        p.x >= self.x0 - slack && p.x <= self.x1 + slack && p.y >= self.y0 - slack && p.y <= self.y1 + slack
    }

    pub fn intersects(&self, o: &TableBox) -> bool {
        self.x0 <= o.x1 && o.x0 <= self.x1 && self.y0 <= o.y1 && o.y0 <= self.y1
    }

    pub fn contains_box(&self, o: &TableBox) -> bool {
        self.x0 <= o.x0 && self.y0 <= o.y0 && self.x1 >= o.x1 && self.y1 >= o.y1
    }

    pub fn center(&self) -> Point {
        Point { x: (self.x0 + self.x1) / 2, y: (self.y0 + self.y1) / 2 }
    }
}

/// One line of a table's borders: where it goes and the stroke it is drawn in (`PCB_TABLE::DrawBorders`'s callback).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BorderLine {
    pub a: Point,
    pub b: Point,
    pub stroke: TableStroke,
}

/// `IsBackLayer`: the layers KiCad mirrors a text on.
pub fn is_back_layer(layer: &str) -> bool {
    matches!(layer, "B.Cu" | "B.Adhes" | "B.Paste" | "B.SilkS" | "B.Mask" | "B.CrtYd" | "B.Fab")
}

impl Table {
    /// The table `DRAWING_TOOL::DrawTable` makes: `cols` x `rows` empty cells of `cell_w` x `cell_h` with KiCad's cell defaults (`PCB_TABLECELL`), its first corner at
    /// `at`, drawn with a border and separators of `line_width` (`PCB_TABLE( aParent, bds.GetLineThickness( layer ) )`: the style is the default one, which draws solid).
    pub fn new_grid(layer: &str, at: Point, cols: usize, rows: usize, cell_w: Nm, cell_h: Nm, line_width: Nm) -> Table {
        let (cols, rows) = (cols.max(1), rows.max(1));
        let back = is_back_layer(layer);
        let stroke = TableStroke { width_nm: line_width, style: TableLineStyle::Default, color: None };
        Table {
            id: String::new(),
            uuid: String::new(),
            layer: layer.to_string(),
            at,
            angle: 0,
            col_widths: vec![cell_w; cols],
            row_heights: vec![cell_h; rows],
            cells: (0..cols * rows).map(|_| TableCell::pcb_new(back)).collect(),
            border: TableBorder { external: true, header: true, stroke },
            separators: TableSeparators { rows: true, cols: true, stroke },
        }
    }

    /// The table `SCH_DRAWING_TOOLS::DrawTable` makes: `cols` x `rows` empty sheet cells of `cell_w` x `cell_h`, the first corner at `at`, drawn with a border and separators
    /// of the default width (`SCH_TABLE( 0 )`: width 0, the style the default one).
    pub fn new_sch_grid(at: Point, cols: usize, rows: usize, cell_w: Nm, cell_h: Nm, text_size_nm: Nm) -> Table {
        let (cols, rows) = (cols.max(1), rows.max(1));
        let stroke = TableStroke { width_nm: 0, style: TableLineStyle::Default, color: None };
        Table {
            id: String::new(),
            uuid: String::new(),
            layer: String::new(),
            at,
            angle: 0,
            col_widths: vec![cell_w; cols],
            row_heights: vec![cell_h; rows],
            cells: (0..cols * rows).map(|_| TableCell::sch_new(text_size_nm)).collect(),
            border: TableBorder { external: true, header: true, stroke },
            separators: TableSeparators { rows: true, cols: true, stroke },
        }
    }

    pub fn col_count(&self) -> usize {
        self.col_widths.len()
    }

    pub fn row_count(&self) -> usize {
        self.row_heights.len()
    }

    /// `PCB_TABLE::GetCell`.
    pub fn cell(&self, row: usize, col: usize) -> Option<&TableCell> {
        if col >= self.col_count() {
            return None;
        }
        self.cells.get(row * self.col_count() + col)
    }

    pub fn cell_mut(&mut self, row: usize, col: usize) -> Option<&mut TableCell> {
        let cols = self.col_count();
        if col >= cols {
            return None;
        }
        self.cells.get_mut(row * cols + col)
    }

    /// The row and column of the cell at index `i` of [`Table::cells`] (`PCB_TABLECELL::GetRow` / `GetColumn`).
    pub fn row_col(&self, i: usize) -> (usize, usize) {
        let cols = self.col_count().max(1);
        (i / cols, i % cols)
    }

    /// Does the table hold what it must: at least one column and one row, and as many cells as rows times columns? A reader or an editor that breaks this has
    /// made something no other function here accepts.
    pub fn is_well_formed(&self) -> bool {
        !self.col_widths.is_empty() && !self.row_heights.is_empty() && self.cells.len() == self.col_widths.len() * self.row_heights.len()
    }

    /// The offset of cell (`row`, `col`)'s top-left corner from the table's origin, in the table's own (unturned) frame, and its extent: a merged cell takes the
    /// widths and heights of the columns and rows it spans (`PCB_TABLE::Normalize`; a cell another one covers has its own column's width and its own row's
    /// height).
    pub fn cell_local_rect(&self, row: usize, col: usize) -> (Nm, Nm, Nm, Nm) {
        let x: Nm = self.col_widths.iter().take(col).sum();
        let y: Nm = self.row_heights.iter().take(row).sum();
        let mut w = self.col_widths.get(col).copied().unwrap_or(0);
        let mut h = self.row_heights.get(row).copied().unwrap_or(0);
        if let Some(cell) = self.cell(row, col) {
            if cell.col_span > 1 || cell.row_span > 1 {
                for c in col + 1..col + cell.col_span as usize {
                    w += self.col_widths.get(c).copied().unwrap_or(0);
                }
                for r in row + 1..row + cell.row_span as usize {
                    h += self.row_heights.get(r).copied().unwrap_or(0);
                }
            }
        }
        (x, y, w, h)
    }

    /// A point of the table's own frame, on the board or the sheet.
    pub fn to_world(&self, local: (Nm, Nm)) -> Point {
        let (x, y) = rotate_point(local.0, local.1, self.angle);
        Point { x: self.at.x + x, y: self.at.y + y }
    }

    /// The corners of cell (`row`, `col`) as `GetCornersInSequence( drawAngle )` gives them: top-left, top-right, bottom-right, bottom-left of the cell as
    /// the table reads, so a table turned a quarter has its first corner at the bottom left of the screen.
    pub fn cell_corners(&self, row: usize, col: usize) -> [Point; 4] {
        let (x, y, w, h) = self.cell_local_rect(row, col);
        [self.to_world((x, y)), self.to_world((x + w, y)), self.to_world((x + w, y + h)), self.to_world((x, y + h))]
    }

    /// The box round a cell's corners.
    pub fn cell_box(&self, row: usize, col: usize) -> TableBox {
        TableBox::of(&self.cell_corners(row, col))
    }

    /// `PCB_TABLE::GetBoundingBox`: the first cell's box and the last cell's.
    pub fn bounding_box(&self) -> TableBox {
        let (rows, cols) = (self.row_count().max(1), self.col_count().max(1));
        let mut pts = self.cell_corners(0, 0).to_vec();
        pts.extend(self.cell_corners(rows - 1, cols - 1));
        TableBox::of(&pts)
    }

    /// The table's width and height in its own frame (`PCB_TABLE::GetEnd() - GetPosition()`).
    pub fn local_size(&self) -> (Nm, Nm) {
        (self.col_widths.iter().sum(), self.row_heights.iter().sum())
    }

    /// The centre of cell (0, 0) as `PCB_TABLE::Normalize` keeps it fixed.
    fn first_cell_center(&self) -> Point {
        let (x, y, w, h) = self.cell_local_rect(0, 0);
        self.to_world((x + w / 2, y + h / 2))
    }

    /// `PCB_TABLE::DrawBorders` (and `SCH_TABLE::DrawBorders`, the same code): every line the table draws, with its stroke.
    pub fn borders(&self) -> Vec<BorderLine> {
        let mut out = Vec::new();
        let (rows, cols) = (self.row_count(), self.col_count());
        if rows == 0 || cols == 0 || !self.is_well_formed() {
            return out;
        }
        let top_left = self.cell_corners(0, 0);
        let bottom_left = self.cell_corners(rows - 1, 0);
        let top_right = self.cell_corners(0, cols - 1);
        let bottom_right = self.cell_corners(rows - 1, cols - 1);
        let mut line = |a: Point, b: Point, stroke: TableStroke| out.push(BorderLine { a, b, stroke });

        for col in 0..cols - 1 {
            for row in 0..rows {
                let stroke = if row == 0 && self.border.header {
                    self.border.stroke
                } else if self.separators.cols {
                    self.separators.stroke
                } else {
                    continue;
                };
                let Some(cell) = self.cell(row, col) else { continue };
                if cell.col_span == 0 || col + cell.col_span as usize == cols {
                    continue;
                }
                let corners = self.cell_corners(row, col);
                line(corners[1], corners[2], stroke);
            }
        }
        for row in 0..rows - 1 {
            let stroke = if row == 0 && self.border.header {
                self.border.stroke
            } else if self.separators.rows {
                self.separators.stroke
            } else {
                continue;
            };
            for col in 0..cols {
                let Some(cell) = self.cell(row, col) else { continue };
                if cell.row_span == 0 || row + cell.row_span as usize == rows {
                    continue;
                }
                let corners = self.cell_corners(row, col);
                line(corners[2], corners[3], stroke);
            }
        }
        if self.border.external && self.border.stroke.width_nm >= 0 {
            let s = self.border.stroke;
            line(top_left[0], top_right[1], s);
            line(top_right[1], bottom_right[2], s);
            line(bottom_right[2], bottom_left[3], s);
            line(bottom_left[3], top_left[0], s);
        }
        out
    }

    /// The cell a point is in, the one `hitTestDistance` of the selection tool finds under it: a cell is a filled box, a cell another one covers is not there.
    /// `slack` widens every box (the click's tolerance). With cells that overlap (a merged cell and the cells it covers have the same corners), the merged one
    /// wins: covered cells are skipped.
    pub fn cell_at(&self, p: Point, slack: Nm) -> Option<(usize, usize)> {
        if !self.is_well_formed() {
            return None;
        }
        let mut best: Option<((usize, usize), Nm)> = None;
        for row in 0..self.row_count() {
            for col in 0..self.col_count() {
                if self.cell(row, col).is_none_or(TableCell::is_covered) {
                    continue;
                }
                let corners = self.cell_corners(row, col);
                let d = polygon_distance(&corners, p);
                if d <= slack && best.is_none_or(|(_, b)| d < b) {
                    best = Some(((row, col), d));
                }
            }
        }
        best.map(|(rc, _)| rc)
    }

    /// `PCB_TABLE::HitTest( aPosition, aAccuracy )`: the point is in the box round the table.
    pub fn hit_test(&self, p: Point, slack: Nm) -> bool {
        self.is_well_formed() && self.bounding_box().contains(p, slack)
    }

    /// `PCB_TABLE::HitTest( aRect, aContained, aAccuracy )`.
    pub fn hit_test_box(&self, r: &TableBox, contained: bool, slack: Nm) -> bool {
        if !self.is_well_formed() {
            return false;
        }
        let r = TableBox { x0: r.x0 - slack, y0: r.y0 - slack, x1: r.x1 + slack, y1: r.y1 + slack };
        let b = self.bounding_box();
        if contained {
            r.contains_box(&b)
        } else {
            r.intersects(&b)
        }
    }

    // ------------------------------------------------------------------------------------------------------------------------------ moves

    /// `PCB_TABLE::Move`.
    pub fn translate(&mut self, dx: Nm, dy: Nm) {
        self.at.x += dx;
        self.at.y += dy;
    }

    /// `PCB_TABLE::Rotate( aRotCentre, aAngle )` then `Normalize()`: the grid turns by `angle` about `centre` and cell (0, 0) keeps its centre where the turn
    /// puts it.
    pub fn rotate(&mut self, centre: Point, angle: Millideg) {
        let c0 = self.first_cell_center();
        let (rx, ry) = rotate_point(c0.x - centre.x, c0.y - centre.y, angle);
        let c0 = Point { x: centre.x + rx, y: centre.y + ry };
        self.angle = (self.angle + angle) % 360_000;
        let (_, _, w, h) = self.cell_local_rect(0, 0);
        let (ox, oy) = rotate_point(w / 2, h / 2, self.angle);
        self.at = Point { x: c0.x - ox, y: c0.y - oy };
    }

    /// `SCH_TABLE::Rotate( aCenter, aRotateCCW )`: every cell's rectangle is turned a quarter about `centre` and its text angle toggles between lying and standing
    /// (`SCH_TEXTBOX::Rotate`), then `Normalize()` lays the grid out again from where cell (0, 0) went, turned by that angle: a sheet's table has the two attitudes, and a
    /// second quarter does not continue the first.
    pub fn rotate_sch(&mut self, centre: Point, ccw: bool) {
        let (rx, ry) = rotate_point(self.at.x - centre.x, self.at.y - centre.y, if ccw { 90_000 } else { 270_000 });
        self.at = Point { x: centre.x + rx, y: centre.y + ry };
        self.angle = if self.angle == 90_000 { 0 } else { 90_000 };
    }

    /// `PCB_TABLE::Flip( aCentre, aFlipDirection )`: the table is turned over to `layer`. Its columns (`left_right`) or rows run the other way, the text of its cells
    /// is mirrored when the layers are side specific (`PCB_TEXTBOX::Flip`), and it lies where the mirror image of its box about the axis through `centre` is, with the
    /// angle it had. (KiCad finishes with a move by the table's width or height whose reason cannot be measured -- kicad-cli does not flip -- so the place is the
    /// mirror image's.)
    pub fn flip(&mut self, centre: Point, left_right: bool, layer: String, mirror_text: bool) {
        if !self.is_well_formed() {
            return;
        }
        let orig = self.bounding_box();
        let target = if left_right { Point { x: 2 * centre.x - orig.x1, y: orig.y0 } } else { Point { x: orig.x0, y: 2 * centre.y - orig.y1 } };
        let (rows, cols) = (self.row_count(), self.col_count());
        if left_right {
            for r in 0..rows {
                self.cells[r * cols..(r + 1) * cols].reverse();
            }
            self.col_widths.reverse();
        } else {
            let old = std::mem::take(&mut self.cells);
            let mut lines: Vec<Vec<TableCell>> = old.chunks(cols).map(<[TableCell]>::to_vec).collect();
            lines.reverse();
            self.cells = lines.into_iter().flatten().collect();
            self.row_heights.reverse();
        }
        if mirror_text {
            for c in &mut self.cells {
                c.style.mirror = !c.style.mirror;
            }
        }
        self.layer = layer;
        let now = self.bounding_box();
        self.translate(target.x - now.x0, target.y - now.y0);
    }

    // ------------------------------------------------------------------------------------------------------------------- structure edits

    /// A copy of `src` as `PCB_EDIT_TABLE_TOOL::copyCell` makes one: every look, no text, no uuid.
    fn copy_cell(src: &TableCell) -> TableCell {
        let mut c = src.clone();
        c.uuid.clear();
        c.text.clear();
        c
    }

    /// `doAddRowAbove` (`above`) and `doAddRowBelow`: a row of copies of the cells of row `row`, with its height, put above or below it. KiCad inserts every
    /// copy at the same index, so the new row's cells come out in the opposite order to the source row's; that is kept.
    pub fn add_row(&mut self, row: usize, above: bool) -> bool {
        let cols = self.col_count();
        if row >= self.row_count() || cols == 0 || !self.is_well_formed() {
            return false;
        }
        let sources: Vec<TableCell> = (0..cols).filter_map(|c| self.cell(row, c).cloned()).collect();
        let at = if above { row } else { row + 1 };
        for src in &sources {
            self.cells.insert(at * cols, Self::copy_cell(src));
        }
        let height = self.row_heights[row];
        self.row_heights.insert(at, height);
        true
    }

    /// `doAddColumnBefore` (`before`) and `doAddColumnAfter`: a column of copies of the cells of column `col`, with its width.
    pub fn add_column(&mut self, col: usize, before: bool) -> bool {
        let (rows, cols) = (self.row_count(), self.col_count());
        if col >= cols || rows == 0 || !self.is_well_formed() {
            return false;
        }
        let sources: Vec<TableCell> = (0..rows).filter_map(|r| self.cell(r, col).cloned()).collect();
        let at = if before { col } else { col + 1 };
        let new_cols = cols + 1;
        // Row by row from the top: the rows above `row` have gained their cell, so row `row` starts at `row * new_cols` (`InsertCell( row * GetColCount() + col )`
        // after `SetColCount( GetColCount() + 1 )`).
        for (row, src) in sources.iter().enumerate() {
            self.cells.insert(row * new_cols + at, Self::copy_cell(src));
        }
        let width = self.col_widths[col];
        self.col_widths.insert(at, width);
        true
    }

    /// `doDeleteRows`: every row that has one of `rows` is taken out, with its cells and height. Returns `false` when that would be every row (KiCad removes
    /// the table instead: the caller decides).
    pub fn delete_rows(&mut self, rows: &[usize]) -> bool {
        let mut del: Vec<usize> = rows.iter().copied().filter(|r| *r < self.row_count()).collect();
        del.sort_unstable();
        del.dedup();
        if del.is_empty() || del.len() >= self.row_count() || !self.is_well_formed() {
            return false;
        }
        let cols = self.col_count();
        let mut i = 0;
        self.cells.retain(|_| {
            let row = i / cols;
            i += 1;
            del.binary_search(&row).is_err()
        });
        let mut r = 0;
        self.row_heights.retain(|_| {
            let keep = del.binary_search(&r).is_err();
            r += 1;
            keep
        });
        true
    }

    /// `doDeleteColumns`: the same for columns.
    pub fn delete_columns(&mut self, cols: &[usize]) -> bool {
        let mut del: Vec<usize> = cols.iter().copied().filter(|c| *c < self.col_count()).collect();
        del.sort_unstable();
        del.dedup();
        if del.is_empty() || del.len() >= self.col_count() || !self.is_well_formed() {
            return false;
        }
        let cols_now = self.col_count();
        let mut i = 0;
        self.cells.retain(|_| {
            let col = i % cols_now;
            i += 1;
            del.binary_search(&col).is_err()
        });
        let mut c = 0;
        self.col_widths.retain(|_| {
            let keep = del.binary_search(&c).is_err();
            c += 1;
            keep
        });
        true
    }

    /// The columns and rows a block of selected cells takes: `(col_min, col_max, row_min, row_max)`, the maxima exclusive, and whether the cells fill that
    /// rectangle exactly (`getCellBlockBounds` / the menu's `cellBlockSelection`).
    pub fn block_bounds(&self, cells: &[(usize, usize)]) -> Option<(usize, usize, usize, usize, bool)> {
        if cells.is_empty() {
            return None;
        }
        let (mut c0, mut c1, mut r0, mut r1, mut area) = (usize::MAX, 0usize, usize::MAX, 0usize, 0usize);
        for &(row, col) in cells {
            let cell = self.cell(row, col)?;
            c0 = c0.min(col);
            c1 = c1.max(col + cell.col_span as usize);
            r0 = r0.min(row);
            r1 = r1.max(row + cell.row_span as usize);
            area += cell.col_span as usize * cell.row_span as usize;
        }
        let exact = cells.len() == 1 || area == (c1 - c0) * (r1 - r0);
        Some((c0, c1, r0, r1, exact))
    }

    /// `doMergeCells`: the block the cells span becomes one cell, the top-left one, with the non-empty texts of the block joined by line feeds; the others are
    /// covered (span 0, no text). Returns `false` when there is nothing to merge.
    pub fn merge(&mut self, cells: &[(usize, usize)]) -> bool {
        let Some((c0, c1, r0, r1, _)) = self.block_bounds(cells) else { return false };
        if r1 > self.row_count() || c1 > self.col_count() {
            return false;
        }
        let mut content = String::new();
        for row in r0..r1 {
            for col in c0..c1 {
                let Some(cell) = self.cell_mut(row, col) else { return false };
                if !cell.text.is_empty() {
                    if !content.is_empty() {
                        content.push('\n');
                    }
                    content.push_str(&cell.text);
                }
                cell.col_span = 0;
                cell.row_span = 0;
                cell.text.clear();
            }
        }
        if let Some(top_left) = self.cell_mut(r0, c0) {
            top_left.col_span = (c1 - c0) as u32;
            top_left.row_span = (r1 - r0) as u32;
            top_left.text = content;
        }
        true
    }

    /// `doUnmergeCells`: every cell of `cells` that is merged gives its block back, one cell each again.
    pub fn unmerge(&mut self, cells: &[(usize, usize)]) -> bool {
        let mut any = false;
        for &(row, col) in cells {
            let Some(cell) = self.cell(row, col) else { continue };
            let (row_span, col_span) = (cell.row_span as usize, cell.col_span as usize);
            for r in row..row + row_span {
                for c in col..col + col_span {
                    if let Some(target) = self.cell_mut(r, c) {
                        target.col_span = 1;
                        target.row_span = 1;
                        any = true;
                    }
                }
            }
        }
        any
    }

    /// The text of every cell, rows of columns, as the Table Properties grid holds it.
    pub fn texts(&self) -> Vec<Vec<String>> {
        (0..self.row_count()).map(|r| (0..self.col_count()).map(|c| self.cell(r, c).map(|x| x.text.clone()).unwrap_or_default()).collect()).collect()
    }

    /// What the id of a table is derived from.
    pub fn id_seed(&self) -> String {
        let first = self.cells.first().map(|c| c.text.as_str()).unwrap_or("");
        format!("{}|{},{}|{}|{}x{}|{}", self.layer, self.at.x, self.at.y, self.angle, self.col_count(), self.row_count(), first)
    }
}

/// How far a point is outside a convex polygon, nm: 0 or less when it is in it.
fn polygon_distance(poly: &[Point; 4], p: Point) -> Nm {
    // The polygon is a (turned) rectangle: a point is inside when it is on the same side of all four edges.
    let mut sign = 0i32;
    let mut inside = true;
    let mut dist = f64::MAX;
    for i in 0..4 {
        let (a, b) = (poly[i], poly[(i + 1) % 4]);
        let (abx, aby) = ((b.x - a.x) as f64, (b.y - a.y) as f64);
        let cross = abx * (p.y - a.y) as f64 - aby * (p.x - a.x) as f64;
        let s = if cross > 0.0 {
            1
        } else if cross < 0.0 {
            -1
        } else {
            0
        };
        if s != 0 {
            if sign == 0 {
                sign = s;
            } else if sign != s {
                inside = false;
            }
        }
        let len2 = abx * abx + aby * aby;
        let t = if len2 == 0.0 { 0.0 } else { (((p.x - a.x) as f64 * abx + (p.y - a.y) as f64 * aby) / len2).clamp(0.0, 1.0) };
        let (qx, qy) = (a.x as f64 + t * abx, a.y as f64 + t * aby);
        dist = dist.min(((p.x as f64 - qx).powi(2) + (p.y as f64 - qy).powi(2)).sqrt());
    }
    if inside {
        0
    } else {
        dist.ceil() as Nm
    }
}

impl crate::ir::DrawingsSection {
    /// The table `id`, if the board has it.
    pub fn table(&self, id: &str) -> Option<&Table> {
        self.tables.iter().find(|t| t.id == id)
    }

    /// Give every table that has no id one, from its place and its first text (the same contract as [`crate::ir::DrawingsSection::assign_missing_ids`]).
    pub fn assign_table_ids(&mut self) {
        let mut existing: std::collections::BTreeSet<String> = self.tables.iter().map(|t| t.id.clone()).filter(|s| !s.is_empty()).collect();
        let mut order: Vec<usize> = (0..self.tables.len()).collect();
        order.sort_by(|&a, &b| (&self.tables[a].layer, self.tables[a].at, &self.tables[a].id_seed()).cmp(&(&self.tables[b].layer, self.tables[b].at, &self.tables[b].id_seed())));
        for i in order {
            if self.tables[i].id.is_empty() {
                let id = crate::ir::next_item_id("tbl", &self.tables[i].id_seed(), &existing);
                existing.insert(id.clone());
                self.tables[i].id = id;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A `rows` x `cols` table of 10 mm wide, 5 mm high cells at the origin.
    fn grid(rows: usize, cols: usize) -> Table {
        let mut t = Table {
            id: "tbl_test".into(),
            uuid: String::new(),
            layer: "F.SilkS".into(),
            at: Point { x: 0, y: 0 },
            angle: 0,
            col_widths: vec![10_000_000; cols],
            row_heights: vec![5_000_000; rows],
            cells: (0..rows * cols).map(|_| TableCell::pcb_new(false)).collect(),
            border: TableBorder { external: true, header: true, stroke: TableStroke::solid(100_000) },
            separators: TableSeparators { rows: true, cols: true, stroke: TableStroke::solid(100_000) },
        };
        for (i, c) in t.cells.iter_mut().enumerate() {
            c.text = format!("c{i}");
        }
        t
    }

    #[test]
    fn a_cells_corners_are_the_grids_and_a_quarter_turn_puts_the_first_corner_at_the_bottom_left() {
        let mut t = grid(2, 2);
        t.at = Point { x: 1_000_000, y: 2_000_000 };
        assert_eq!(t.cell_corners(1, 1), [Point { x: 11_000_000, y: 7_000_000 }, Point { x: 21_000_000, y: 7_000_000 }, Point { x: 21_000_000, y: 12_000_000 }, Point { x: 11_000_000, y: 12_000_000 }]);
        // `GetCornersInSequence( ANGLE_90 )`: bbox left-bottom, left-top, right-top, right-bottom.
        t.angle = 90_000;
        let b = t.cell_corners(0, 0);
        assert_eq!(b[0], Point { x: 1_000_000, y: 2_000_000 });
        assert_eq!(b[1], Point { x: 1_000_000, y: 2_000_000 - 10_000_000 });
        assert_eq!(b[2], Point { x: 1_000_000 + 5_000_000, y: 2_000_000 - 10_000_000 });
        assert_eq!(b[3], Point { x: 1_000_000 + 5_000_000, y: 2_000_000 });
    }

    #[test]
    fn a_merged_cell_takes_the_widths_it_spans_and_a_covered_one_is_not_drawn_around() {
        let mut t = grid(3, 3);
        assert!(t.merge(&[(0, 0), (0, 1), (1, 0), (1, 1)]));
        assert_eq!(t.cell_local_rect(0, 0), (0, 0, 20_000_000, 10_000_000));
        assert_eq!((t.cell(0, 0).unwrap().col_span, t.cell(0, 0).unwrap().row_span), (2, 2));
        assert!(t.cell(1, 1).unwrap().is_covered());
        assert_eq!(t.cell(0, 0).unwrap().text, "c0\nc1\nc3\nc4", "the texts of the block, joined by line feeds");
        assert!(t.cell(0, 1).unwrap().text.is_empty());
        // a click in the merged block finds the merged cell
        assert_eq!(t.cell_at(Point { x: 15_000_000, y: 7_000_000 }, 0), Some((0, 0)));
        // and the borders skip the lines inside it: the vertical line at x = 10 mm is drawn on row 2 only (rows 0 and 1 are inside the merge)
        let inside: Vec<_> = t.borders().into_iter().filter(|l| l.a.x == 10_000_000 && l.b.x == 10_000_000).collect();
        assert!(inside.iter().all(|l| l.a.y >= 10_000_000 && l.b.y >= 10_000_000), "{inside:?}");
        assert!(t.unmerge(&[(0, 0)]));
        assert!(t.cells.iter().all(|c| c.col_span == 1 && c.row_span == 1));
    }

    #[test]
    fn the_borders_follow_draw_borders() {
        let t = grid(2, 3);
        let lines = t.borders();
        // 2 vertical separator columns x 2 rows + 1 horizontal row separator x 3 cols + the 4 sides
        assert_eq!(lines.len(), 2 * 2 + 3 + 4, "{lines:?}");
        // the line under the header and the verticals of the header row use the border stroke; the others the separators'
        let mut t2 = t.clone();
        t2.separators.stroke = TableStroke { width_nm: 50_000, style: TableLineStyle::Dash, color: None };
        let header_vertical = t2.borders().into_iter().find(|l| l.a.x == 10_000_000 && l.a.y == 0).unwrap();
        assert_eq!(header_vertical.stroke.width_nm, 100_000);
        let row2_vertical = t2.borders().into_iter().find(|l| l.a.x == 10_000_000 && l.a.y == 5_000_000).unwrap();
        assert_eq!(row2_vertical.stroke.width_nm, 50_000);
        // no border, no separators: only the header lines remain
        t2.border.external = false;
        t2.separators.rows = false;
        t2.separators.cols = false;
        assert_eq!(t2.borders().len(), 2 /* header verticals */ + 3 /* line under the header */);
        // a border with no stroke (width -1) draws no sides
        t2.border.external = true;
        t2.border.stroke.width_nm = -1;
        assert_eq!(t2.borders().len(), 5);
    }

    #[test]
    fn rows_and_columns_are_added_and_deleted_as_the_tool_does() {
        let mut t = grid(2, 2);
        t.row_heights = vec![5_000_000, 8_000_000];
        for (i, c) in t.cells.iter_mut().enumerate() {
            c.style.bold = i % 2 == 0;
        }
        // Add Row Below the first row: copies of its cells, its height, no text; KiCad inserts them at one index so they come out reversed.
        assert!(t.add_row(0, false));
        assert_eq!((t.row_count(), t.col_count()), (3, 2));
        assert_eq!(t.row_heights, vec![5_000_000, 5_000_000, 8_000_000]);
        assert_eq!(t.texts(), vec![vec!["c0", "c1"], vec!["", ""], vec!["c2", "c3"]]);
        assert_eq!((t.cell(1, 0).unwrap().style.bold, t.cell(1, 1).unwrap().style.bold), (false, true), "reversed, as KiCad inserts them");
        // Add Row Above the last row, Add Column After the first and Before the last
        assert!(t.add_row(2, true));
        assert_eq!(t.row_heights, vec![5_000_000, 5_000_000, 8_000_000, 8_000_000]);
        assert!(t.add_column(0, false));
        assert_eq!((t.row_count(), t.col_count()), (4, 3));
        assert_eq!(t.col_widths, vec![10_000_000; 3]);
        assert_eq!(t.texts()[0], vec!["c0", "", "c1"]);
        assert!(t.is_well_formed());
        // Delete rows 1 and 2, then column 0
        assert!(t.delete_rows(&[1, 2, 2]));
        assert_eq!((t.row_count(), t.row_heights.clone()), (2, vec![5_000_000, 8_000_000]));
        assert_eq!(t.texts(), vec![vec!["c0", "", "c1"], vec!["c2", "", "c3"]]);
        assert!(t.delete_columns(&[0]));
        assert_eq!(t.col_count(), 2);
        assert!(t.is_well_formed());
        // Deleting every row or column is refused here (KiCad removes the table)
        assert!(!t.delete_rows(&[0, 1]));
        assert!(!t.delete_columns(&[0, 1]));
    }

    #[test]
    fn a_table_turned_about_a_point_keeps_the_centre_of_its_first_cell_where_the_turn_puts_it() {
        let mut t = grid(2, 2);
        t.rotate(Point { x: 0, y: 0 }, 90_000);
        // cell (0,0) centre was (5000, 2500); a quarter turn about the origin takes it to (2500, -5000)
        assert_eq!(t.angle, 90_000);
        assert_eq!(t.cell_box(0, 0).center(), Point { x: 2_500_000, y: -5_000_000 });
        // and four quarter turns come back
        for _ in 0..3 {
            t.rotate(Point { x: 0, y: 0 }, 90_000);
        }
        assert_eq!((t.angle, t.at), (0, Point { x: 0, y: 0 }));
    }

    #[test]
    fn a_flipped_table_is_the_mirror_image_with_its_columns_reversed_and_its_text_mirrored() {
        let mut t = grid(2, 3);
        t.col_widths = vec![10_000_000, 20_000_000, 30_000_000];
        t.at = Point { x: 10_000_000, y: 5_000_000 };
        // the table spans x 10..70 mm; mirrored about x = 0 it spans -70..-10
        t.flip(Point { x: 0, y: 0 }, true, "B.SilkS".into(), true);
        assert_eq!(t.layer, "B.SilkS");
        assert_eq!(t.col_widths, vec![30_000_000, 20_000_000, 10_000_000]);
        assert_eq!(t.texts(), vec![vec!["c2", "c1", "c0"], vec!["c5", "c4", "c3"]]);
        assert!(t.cells.iter().all(|c| c.style.mirror));
        let b = t.bounding_box();
        assert_eq!((b.x0, b.x1, b.y0, b.y1), (-70_000_000, -10_000_000, 5_000_000, 15_000_000));
        // top-bottom reverses the rows instead
        let mut u = grid(2, 2);
        u.row_heights = vec![5_000_000, 8_000_000];
        u.flip(Point { x: 0, y: 0 }, false, "B.SilkS".into(), false);
        assert_eq!(u.row_heights, vec![8_000_000, 5_000_000]);
        assert_eq!(u.texts(), vec![vec!["c2", "c3"], vec!["c0", "c1"]]);
        let b = u.bounding_box();
        assert_eq!((b.y0, b.y1), (-13_000_000, 0));
    }

    #[test]
    fn item_ids_of_cells_round_trip() {
        assert_eq!(cell_item_id("tbl_ab12", 3, 1), "tbl_ab12:3:1");
        assert_eq!(parse_cell_item_id("tbl_ab12:3:1"), Some(("tbl_ab12", 3, 1)));
        assert_eq!(parse_cell_item_id("tbl_ab12"), None);
        assert_eq!(parse_cell_item_id("pin:U1:3"), None, "a pin id is not a cell's: its row would have to be a number");
    }
}
