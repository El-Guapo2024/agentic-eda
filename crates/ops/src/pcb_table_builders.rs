//! The two tables KiCad builds from the board itself: Place Board Stackup Table and Place Board Characteristics.
//!
//! Ported from `pcbnew/board_tables/board_stackup_table.cpp` (`Build_Board_Stackup_Table`) and `board_characteristics_table.cpp`
//! (`Build_Board_Characteristics_Table`): the cells, in the order and with the text sizes and justifications KiCad gives them, the text each one holds from Board
//! Setup's stackup and the board's numbers, and `PCB_TABLE::Autosize`, which sizes the columns and the rows from the ink of the text.
//!
//! The numbers of the characteristics table that KiCad measures with `ComputeBoardStatistics` (the thickness, the overall dimensions, the smallest track, clearance
//! and hole, the pads with a property) come in as [`BoardNumbers`]: they are kicad-cli's (`pcb export stats`, the Board Statistics dialog's), and nothing here measures a
//! board.
//!
//! The text is `UNITS_PROVIDER::StringFromValue` / `MessageTextFromValue` of the user's units, so the same board gives a table in millimetres, mils or inches.

use eda_drc::constraints::{string_from_value, Units};
use eda_drc::stroke_font::{effective_pen_width, kicad_text_segments, TextLayout};
use eda_model::ir::{Point, TextJustify, Nm};
use eda_model::rules::default_stackup;
use eda_model::{Stackup, StackupLayer};
use eda_model::table::{Table, TableCell};
use serde::{Deserialize, Serialize};

/// What `ComputeBoardStatistics` and the board itself say that the characteristics table prints, in nanometres (the unit of the board file): the whole numbers of the
/// kicad-cli statistics reply, rounded.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BoardNumbers {
    /// `brd_stat_data.boardThickness`.
    pub thickness_nm: i64,
    /// The width and height of `GetBoardPolygonOutlines`' bounding box ("Board overall dimensions").
    pub width_nm: i64,
    pub height_nm: i64,
    /// `minTrackWidth`, `minClearanceTrackToTrack`, `minDrillSize`: an unset minimum is KiCad's own `INT_MAX` nanometres.
    pub min_track_nm: i64,
    pub min_clearance_nm: i64,
    pub min_drill_nm: i64,
    /// `GetPadWithCastellatedAttrCount()` and `GetPadWithPressFitAttrCount()`.
    #[serde(default)]
    pub castellated_pads: u32,
    #[serde(default)]
    pub press_fit_pads: u32,
}

// ----------------------------------------------------------------------------------------------------------------------------------------- the text of the numbers

/// C's `%.3e`.
fn sci3(v: f64) -> String {
    let s = format!("{v:.3e}");
    let (mantissa, exp) = s.split_once('e').unwrap_or((&s, "0"));
    let e: i32 = exp.parse().unwrap_or(0);
    format!("{mantissa}e{}{:02}", if e < 0 { '-' } else { '+' }, e.abs())
}

/// `EDA_UNIT_UTILS::UI::MessageTextFromValue` for a length of the board (`pcbIUScale`): the lower-precision text the info dialogs show -- four decimals in millimetres and
/// inches, two in mils -- in scientific notation when that would show only zeros, with the unit's label (`mm`, `mils`, `in`) when asked.
pub fn message_text_from_value(value_nm: i64, units: Units, add_label: bool) -> String {
    let (iu_per_unit, decimals, label) = match units {
        Units::Mm => (1_000_000.0, 4usize, " mm"),
        Units::Mils => (25_400.0, 2, " mils"),
        Units::Inch => (25_400_000.0, 4, " in"),
    };
    let value = value_nm as f64 / iu_per_unit;
    let mut text = format!("{value:.decimals$}");
    if value != 0.0 && !text.chars().any(|c| ('1'..='9').contains(&c)) {
        text = sci3(value);
    }
    if add_label {
        text.push_str(label);
    }
    text
}

/// `EDA_UNIT_UTILS::UI::StringFromValue( unityScale, EDA_UNITS::UNSCALED, value )`: a plain number (an epsilon, a loss tangent, a layer count), `%.10f` without the
/// trailing zeros.
pub fn unitless_text(value: f64) -> String {
    let text = format!("{value:.10}");
    let trimmed = text.trim_end_matches('0');
    let trimmed = trimmed.strip_suffix('.').unwrap_or(trimmed);
    if trimmed.is_empty() || trimmed == "-" {
        "0".to_string()
    } else {
        trimmed.to_string()
    }
}

/// `InitialCaps`: the first word with a capital, the others in lower case.
pub fn initial_caps(s: &str) -> String {
    let mut out = String::new();
    for (i, word) in s.split(' ').enumerate() {
        let mut chars = word.chars();
        if i == 0 {
            if let Some(first) = chars.next() {
                out.extend(first.to_uppercase());
                out.push_str(&chars.as_str().to_lowercase());
            }
        } else {
            out.push(' ');
            out.push_str(&word.to_lowercase());
        }
    }
    out
}

// ------------------------------------------------------------------------------------------------------------------------------------------------ Autosize

/// `ARC_LOW_DEF` (0.02 mm), the error `PCB_TABLE::Autosize` allows the polygon of a stroke.
const ARC_LOW_DEF_NM: f64 = 20_000.0;

/// The bounding box of `TransformOvalToPolygon( a, b, pen, ARC_LOW_DEF, ERROR_INSIDE )`: the stroke from `a` to `b` as the capsule KiCad polygonises, whose rounded ends
/// are cut into a multiple of eight sides (so a flat lies across the axis of the stroke, half a side from it) and lie inside the true outline.
fn oval_extents(a: (f64, f64), b: (f64, f64), pen_nm: f64) -> (f64, f64, f64, f64) {
    let radius = (pen_nm / 2.0).floor();
    // `GetArcToSegmentCount( radius, aError, FULL_CIRCLE )`, with its floor of eight sides to the circle, then "round up to 8".
    let rel_error = ARC_LOW_DEF_NM / radius.max(1.0);
    let increment = (180.0 / std::f64::consts::PI * (1.0 - rel_error).acos() * 2.0).min(45.0);
    let increment = if increment.is_finite() { increment } else { 45.0 };
    let count = ((360.0 / increment).round() as i64).max(2);
    let sides = (count + 7) / 8 * 8;
    let delta = 360.0 / sides as f64;

    // The stroke is built along +x from the origin ("normalize the position in order to have endp.x >= 0") and then turned and moved to where it is.
    let (start, end) = if b.0 - a.0 < 0.0 { (b, a) } else { (a, b) };
    let (dx, dy) = (end.0 - start.0, end.1 - start.1);
    let len = (dx * dx + dy * dy).sqrt();
    let theta = dy.atan2(dx);
    let (s, c) = theta.sin_cos();

    let mut local: Vec<(f64, f64)> = vec![(len, radius)];
    let mut angle = delta / 2.0;
    while angle < 180.0 {
        let (sa, ca) = angle.to_radians().sin_cos();
        local.push((len + radius * sa, radius * ca));
        angle += delta;
    }
    local.push((len, -radius));
    local.push((0.0, -radius));
    angle = delta / 2.0;
    while angle < 180.0 {
        let (sa, ca) = angle.to_radians().sin_cos();
        local.push((-radius * sa, -radius * ca));
        angle += delta;
    }
    local.push((0.0, radius));

    let (mut x0, mut y0, mut x1, mut y1) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
    for (lx, ly) in local {
        let (gx, gy) = (start.0 + lx * c - ly * s, start.1 + lx * s + ly * c);
        x0 = x0.min(gx);
        y0 = y0.min(gy);
        x1 = x1.max(gx);
        y1 = y1.max(gy);
    }
    (x0, y0, x1, y1)
}

/// The width and height, nm, of the box round the ink of a cell's text: `TransformTextToPolySet( .., 0, ARC_LOW_DEF, ERROR_INSIDE )` of the stroke font and its
/// `BBox()`. Text with no ink (nothing, or spaces) has none.
///
/// The glyphs are laid out in nanometres, as KiCad lays them out (each glyph's advance is rounded to the nanometre, not to the micrometre the IR holds a size in): a line
/// of fifteen glyphs laid out in micrometres comes out a few micrometres wider than KiCad's.
fn ink_size(text: &str, size: (Nm, Nm), thickness: Nm, bold: bool) -> (f64, f64) {
    let (size_nm, thickness_nm) = (size, thickness);
    let pen = effective_pen_width(thickness_nm, size_nm, bold).as_f64();
    let segments = kicad_text_segments(&TextLayout { text, pos: Point { x: Nm::ZERO, y: Nm::ZERO }, size: size_nm, thickness: thickness_nm, angle_deg: 0.0, mirror: false, halign: -1, valign: 0 });
    let (mut x0, mut y0, mut x1, mut y1) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
    for seg in &segments {
        let (ax0, ay0, ax1, ay1) = oval_extents((seg.a.x.as_f64(), seg.a.y.as_f64()), (seg.b.x.as_f64(), seg.b.y.as_f64()), pen);
        x0 = x0.min(ax0);
        y0 = y0.min(ay0);
        x1 = x1.max(ax1);
        y1 = y1.max(ay1);
    }
    if segments.is_empty() {
        (0.0, 0.0)
    } else {
        (x1 - x0, y1 - y0)
    }
}

/// `PCB_TABLE::Autosize`: every column as wide as its widest cell's ink plus one and a half times the margins (left and right) of that cell, every row as high as its
/// tallest cell's ink plus the same two margins (KiCad adds the left and right margins to a height too). A cell that another covers counts as any other.
pub fn autosize(t: &mut Table) {
    let (cols, rows) = (t.col_count(), t.row_count());
    let mut extents: Vec<Vec<(f64, f64)>> = Vec::with_capacity(rows);
    for row in 0..rows {
        let mut line = Vec::with_capacity(cols);
        for col in 0..cols {
            let cell = &t.cells[row * cols + col];
            let st = &cell.style;
            line.push(ink_size(&cell.text, (st.width_nm, st.height_nm), st.thickness_nm, st.bold));
        }
        extents.push(line);
    }
    for col in 0..cols {
        let mut width = Nm::ZERO;
        for row in 0..rows {
            let cell = &t.cells[row * cols + col];
            // `GetMarginLeft() + GetMarginRight()`, nanometres (a new cell's 1.0025 mm, not the IR's 1.003).
            let margins_nm = (cell.margin_nm(0) + cell.margin_nm(2)).as_f64();
            // `std::max<int>( colWidth, width + margins * 1.5 )`: the sum is cut to an integer, in nanometres.
            width = width.max(Nm::new((extents[row][col].0 + margins_nm * 1.5).trunc() as i64));
        }
        t.col_widths[col] = width;
    }
    for row in 0..rows {
        let mut height = Nm::ZERO;
        for col in 0..cols {
            let cell = &t.cells[row * cols + col];
            let margins_nm = cell.margin_nm(0) + cell.margin_nm(2);
            height = height.max(Nm::new(extents[row][col].1.trunc() as i64) + margins_nm);
        }
        t.row_heights[row] = height;
    }
}

// ------------------------------------------------------------------------------------------------------------------------------------------------- the cells

/// A cell of the builders: `new PCB_TABLECELL( table )` (a table that is still on the default layer, so its text is not mirrored whatever layer the table is put on after:
/// `SetLayer` does not turn the text over), then `SetTextSize`, `SetTextThickness` and the justification.
fn cell(text: &str, size_mm: f64, thickness_mm: f64, right: bool) -> TableCell {
    let mut c = TableCell::pcb_new(false);
    let size = eda_model::units::from_mm(size_mm);
    c.style.width_nm = size;
    c.style.height_nm = size;
    c.style.thickness_nm = eda_model::units::from_mm(thickness_mm);
    if right {
        c.style.h_align = TextJustify::Right;
    }
    c.text = text.to_string();
    c
}

/// `new PCB_TABLE( aBoard, DEFAULT_LINE_WIDTH )` with the cells given, row by row, on `layer`: the border and the separators are 0.1 mm wide and drawn.
fn table_of(layer: &str, cols: usize, cells: Vec<TableCell>) -> Table {
    let rows = cells.len() / cols.max(1);
    let mut t = Table::new_grid("F.Cu", Point { x: Nm::ZERO, y: Nm::ZERO }, cols, rows, Nm::ZERO, Nm::ZERO, Nm::new(100_000));
    t.cells = cells;
    t.layer = layer.to_string();
    t
}

// ------------------------------------------------------------------------------------------------------------------------------------------- the stackup table

/// `BOARD_STACKUP_ITEM_TYPE`, from the type a stackup layer is written with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ItemType {
    Copper,
    Dielectric,
    SolderPaste,
    SolderMask,
    SilkScreen,
}

fn is_copper_name(name: &str) -> bool {
    name.ends_with(".Cu")
}

/// The type name a layer is written with (`GetTypeName`): the one the file gave it, else what its name says it is.
fn type_name(l: &StackupLayer) -> String {
    if let Some(k) = &l.kind {
        return k.clone();
    }
    let side = if l.name.starts_with("B.") { "Bottom" } else { "Top" };
    match l.name.as_str() {
        n if is_copper_name(n) => "copper".into(),
        n if n.ends_with("SilkS") => format!("{side} Silk Screen"),
        n if n.ends_with("Paste") => format!("{side} Solder Paste"),
        n if n.ends_with("Mask") => format!("{side} Solder Mask"),
        _ => "core".into(),
    }
}

fn item_type(l: &StackupLayer) -> ItemType {
    let name = type_name(l);
    match name.to_lowercase().as_str() {
        "copper" => ItemType::Copper,
        "core" | "prepreg" => ItemType::Dielectric,
        n if n.ends_with("silk screen") => ItemType::SilkScreen,
        n if n.ends_with("solder paste") => ItemType::SolderPaste,
        n if n.ends_with("solder mask") => ItemType::SolderMask,
        _ if is_copper_name(&l.name) => ItemType::Copper,
        _ => ItemType::Dielectric,
    }
}

const NOT_SPECIFIED: &str = "Not specified";

/// The stackup item as `BOARD_STACKUP_ITEM( type )` starts and the file then fills in: what a layer that says nothing of a parameter has.
struct Item {
    material: String,
    thickness_nm: i64,
    color: String,
    epsilon_r: f64,
    loss_tangent: f64,
}

fn item_of(l: &StackupLayer, kind: ItemType) -> Item {
    // The defaults of the constructor of each type (`DIELECTRIC_PRMS`: epsilon 1, nothing else; then what the type sets).
    let (material, thickness_mm, color, epsilon, loss) = match kind {
        ItemType::Copper => ("", 0.035, "", 1.0, 0.0),
        ItemType::Dielectric => ("FR4", 0.0, NOT_SPECIFIED, 4.5, 0.02),
        ItemType::SolderPaste => ("", 0.0, "", 1.0, 0.0),
        ItemType::SolderMask => (NOT_SPECIFIED, 0.01, NOT_SPECIFIED, 3.3, 0.0),
        ItemType::SilkScreen => (NOT_SPECIFIED, 0.0, NOT_SPECIFIED, 1.0, 0.0),
    };
    Item {
        material: l.material.clone().unwrap_or_else(|| material.to_string()),
        thickness_nm: (l.thickness_mm.unwrap_or(thickness_mm) * 1_000_000.0).round() as i64,
        color: if matches!(kind, ItemType::Copper | ItemType::SolderPaste) { String::new() } else { l.color.clone().unwrap_or_else(|| color.to_string()) },
        epsilon_r: l.epsilon_r.unwrap_or(epsilon),
        loss_tangent: l.loss_tangent.unwrap_or(loss),
    }
}

/// `Build_Board_Stackup_Table`: seven columns -- layer name, type, material, thickness, colour, epsilon R and loss tangent -- a header row and a row for each layer of
/// the stackup, top to bottom. A dielectric is named "Dielectric N"; the thickness of a silkscreen or a paste layer, which has none, is "Not specified". The table is
/// sized by [`autosize`] and its first corner is the origin; `layer` is the layer it is put on.
pub fn stackup_table(layer: &str, stackup: &Stackup, units: Units) -> Table {
    let mut cells: Vec<TableCell> = Vec::new();
    for title in ["Layer Name", "Type", "Material", "Thickness", "Color", "Epsilon R", "Loss Tangent"] {
        cells.push(cell(title, 1.5, 0.3, false));
    }
    let mut dielectrics = 0;
    for l in &stackup.layers {
        let kind = item_type(l);
        let item = item_of(l, kind);
        let name = if kind == ItemType::Dielectric {
            dielectrics += 1;
            // "Dielectric N": the N of the file's "dielectric N" name, else the count so far (`GetDielectricLayerId`).
            let n = l.name.strip_prefix("dielectric").and_then(|s| s.trim().parse::<u32>().ok()).unwrap_or(dielectrics);
            format!("Dielectric {n}")
        } else {
            l.name.clone()
        };
        // `IsThicknessEditable`: copper, dielectric and solder mask; a silkscreen or a paste layer has no thickness to give.
        let thickness = if matches!(kind, ItemType::Copper | ItemType::Dielectric | ItemType::SolderMask) { string_from_value(item.thickness_nm as f64, units, true) } else { NOT_SPECIFIED.to_string() };
        cells.push(cell(&name, 1.5, 0.2, false));
        cells.push(cell(&initial_caps(&type_name(l)), 1.5, 0.2, false));
        cells.push(cell(&item.material, 1.5, 0.2, false));
        cells.push(cell(&thickness, 1.5, 0.2, true));
        cells.push(cell(&item.color, 1.5, 0.2, false));
        cells.push(cell(&unitless_text(item.epsilon_r), 1.5, 0.2, true));
        cells.push(cell(&unitless_text(item.loss_tangent), 1.5, 0.2, true));
    }
    let mut t = table_of(layer, 7, cells);
    autosize(&mut t);
    t.at = Point { x: Nm::ZERO, y: Nm::ZERO };
    t
}

// ----------------------------------------------------------------------------------------------------------------------------- the board characteristics table

/// `Build_Board_Characteristics_Table`: four columns, a title across all of them (2 mm text) and the board's numbers as label and value pairs, two to a row, in 1.5 mm
/// text; no border and no separators. The copper layer count, the finish, the impedance control, the plated edge and the edge connectors are the stackup's and Board
/// Setup's (a stackup that says no finish has KiCad's "None"); the rest are [`BoardNumbers`].
pub fn characteristics_table(layer: &str, copper_layers: usize, stackup: &Stackup, n: &BoardNumbers, units: Units) -> Table {
    let mut header = cell("BOARD CHARACTERISTICS", 2.0, 0.4, false);
    header.col_span = 4;
    let mut cells: Vec<TableCell> = vec![header];
    // The three cells the title covers: `SetColSpan( 0 )` and nothing else, so each keeps a row span of 1.
    for _ in 1..4 {
        let mut covered = cell("", 2.0, 0.4, false);
        covered.col_span = 0;
        cells.push(covered);
    }
    let mut pair = |label: &str, value: &str| {
        cells.push(cell(label, 1.5, 0.2, false));
        cells.push(cell(value, 1.5, 0.2, false));
    };
    let yes_no = |b: bool| if b { "Yes" } else { "No" };
    let msg = |v: i64| message_text_from_value(v, units, true);

    pair("Copper layer count: ", &unitless_text(copper_layers as f64));
    pair("Board thickness: ", &msg(n.thickness_nm));
    pair("Board overall dimensions: ", &format!("{} x {}", msg(n.width_nm), msg(n.height_nm)));
    pair("", "");
    pair("Min track/spacing: ", &format!("{} / {}", msg(n.min_track_nm), msg(n.min_clearance_nm)));
    pair("Min hole diameter: ", &msg(n.min_drill_nm));
    pair("Copper finish: ", stackup.copper_finish.as_deref().unwrap_or("None"));
    pair("Impedance control: ", yes_no(stackup.dielectric_constraints));
    pair("Castellated pads: ", yes_no(n.castellated_pads > 0));
    pair("Press-fit pads: ", yes_no(n.press_fit_pads > 0));
    pair("Plated board edge: ", yes_no(stackup.edge_plating));
    pair(
        "Edge card connectors: ",
        match stackup.edge_connector {
            0 => "No",
            1 => "Yes",
            _ => "Yes, Bevelled",
        },
    );
    // "We are building a table having 4 columns, so a cell count that is a multiple of 4": the last row is filled up.
    while cells.len() % 4 != 0 {
        cells.push(cell("", 1.5, 0.2, false));
    }
    let mut t = table_of(layer, 4, cells);
    t.border.external = false;
    t.border.header = false;
    t.separators.cols = false;
    t.separators.rows = false;
    autosize(&mut t);
    t.at = Point { x: Nm::ZERO, y: Nm::ZERO };
    t
}

/// The stackup the board has: the one Board Setup holds (`stackup`, with its copper layer count) or, for a board that has none, KiCad's default for its copper layers
/// and thickness (`BOARD_STACKUP::SynchronizeWithBoard` builds it).
pub fn effective_stackup(settings: Option<(usize, Stackup)>, copper_layers: usize, thickness_nm: Nm) -> (usize, Stackup) {
    settings.unwrap_or_else(|| (copper_layers, default_stackup(copper_layers, thickness_nm)))
}

#[cfg(test)]
mod tests {
    use eda_model::units::mm;
    use super::*;

    #[test]
    fn numbers_are_written_as_the_units_providers_write_them() {
        assert_eq!(message_text_from_value(1_600_000, Units::Mm, true), "1.6000 mm");
        assert_eq!(message_text_from_value(1_600_000, Units::Mils, true), "62.99 mils");
        assert_eq!(message_text_from_value(1_600_000, Units::Inch, false), "0.0630");
        assert_eq!(message_text_from_value(4, Units::Mm, true), "4.000e-06 mm", "four decimals of 4 nm are all zeros");
        assert_eq!(unitless_text(4.5), "4.5");
        assert_eq!(unitless_text(0.0), "0");
        assert_eq!(unitless_text(0.02), "0.02");
        assert_eq!(unitless_text(4.0), "4");
    }

    #[test]
    fn initial_caps_capitalises_the_first_word_and_lowers_the_rest() {
        assert_eq!(initial_caps("copper"), "Copper");
        assert_eq!(initial_caps("Top Silk Screen"), "Top silk screen");
        assert_eq!(initial_caps("prepreg"), "Prepreg");
        assert_eq!(initial_caps(""), "");
    }

    #[test]
    fn the_polygon_of_a_stroke_is_a_capsule_with_flats_across_its_axis() {
        // A horizontal stroke of 0.2 mm from (0,0) to (1 mm,0): eight sides to a circle, so its ends reach r * cos(22.5 deg) = 92.4 um along the axis and r across it.
        let (x0, y0, x1, y1) = oval_extents((0.0, 0.0), (1_000_000.0, 0.0), 200_000.0);
        assert!((x0 + 92_388.0).abs() < 1.0 && (x1 - 1_092_388.0).abs() < 1.0, "{x0} {x1}");
        assert!((y0 + 100_000.0).abs() < 1e-9 && (y1 - 100_000.0).abs() < 1e-9, "{y0} {y1}");
        // The same stroke drawn the other way round is the same capsule.
        assert_eq!(oval_extents((1_000_000.0, 0.0), (0.0, 0.0), 200_000.0), (x0, y0, x1, y1));
        // A vertical one: its length lies along y.
        let (vx0, vy0, vx1, vy1) = oval_extents((0.0, 0.0), (0.0, 1_000_000.0), 200_000.0);
        assert!((vy0 + 92_388.0).abs() < 1.0 && (vy1 - 1_092_388.0).abs() < 1.0 && (vx0 + 100_000.0).abs() < 1.0 && (vx1 - 100_000.0).abs() < 1.0, "{vx0} {vy0} {vx1} {vy1}");
    }

    #[test]
    fn the_ink_of_a_text_is_wider_for_more_text_and_has_none_for_a_space() {
        let one = ink_size("H", (mm(1.5), mm(1.5)), mm(0.2), false);
        let two = ink_size("HH", (mm(1.5), mm(1.5)), mm(0.2), false);
        assert!(one.0 > 800_000.0 && one.0 < 1_500_000.0, "{one:?}");
        assert!(two.0 > one.0 * 1.8 && two.1 == one.1, "{two:?} {one:?}");
        assert_eq!(ink_size("  ", (mm(1.5), mm(1.5)), mm(0.2), false), (0.0, 0.0));
        assert_eq!(ink_size("", (mm(1.5), mm(1.5)), mm(0.2), false), (0.0, 0.0));
        // A thicker pen makes the ink bigger.
        assert!(ink_size("H", (mm(1.5), mm(1.5)), mm(0.3), false).0 > one.0);
    }

    #[test]
    fn a_four_layer_stackup_table_has_a_row_for_every_layer_and_sizes_itself_from_its_text() {
        let stackup = default_stackup(4, mm(1.6));
        let t = stackup_table("F.SilkS", &stackup, Units::Mm);
        assert_eq!((t.col_count(), t.row_count()), (7, 1 + stackup.layers.len()));
        let text = |row: usize, col: usize| t.cell(row, col).unwrap().text.clone();
        assert_eq!((0..7).map(|c| text(0, c)).collect::<Vec<_>>(), ["Layer Name", "Type", "Material", "Thickness", "Color", "Epsilon R", "Loss Tangent"]);
        // F.SilkS: no thickness, no material.
        assert_eq!((0..7).map(|c| text(1, c)).collect::<Vec<_>>(), ["F.SilkS", "Top silk screen", "Not specified", "Not specified", "Not specified", "1", "0"]);
        assert_eq!((0..7).map(|c| text(2, c)).collect::<Vec<_>>(), ["F.Paste", "Top solder paste", "", "Not specified", "", "1", "0"]);
        assert_eq!((0..7).map(|c| text(3, c)).collect::<Vec<_>>(), ["F.Mask", "Top solder mask", "Not specified", "0.01 mm", "Not specified", "3.3", "0"]);
        assert_eq!((0..7).map(|c| text(4, c)).collect::<Vec<_>>(), ["F.Cu", "Copper", "", "0.035 mm", "", "1", "0"]);
        assert_eq!(text(5, 0), "Dielectric 1");
        assert_eq!((1..7).map(|c| text(5, c)).collect::<Vec<_>>(), ["Core", "FR4", "0.48 mm", "Not specified", "4.5", "0.02"].map(String::from).to_vec());
        // The sizes of the text and its justification.
        assert_eq!(t.cell(0, 0).unwrap().style.thickness_nm, mm(0.3));
        assert_eq!(t.cell(1, 0).unwrap().style.thickness_nm, mm(0.2));
        assert_eq!(t.cell(1, 3).unwrap().style.h_align, TextJustify::Right);
        assert_eq!(t.cell(1, 0).unwrap().style.h_align, TextJustify::Left);
        // Autosize: every column is as wide as its widest text and the margins ask, every row as high; the corner is the origin.
        assert_eq!(t.at, Point { x: Nm::ZERO, y: Nm::ZERO });
        assert!(t.col_widths.iter().all(|w| *w > mm(3.009)), "{:?}", t.col_widths);
        let widest_name = t.col_widths[0];
        assert!(widest_name > Nm::new(ink_size("Dielectric 1", (mm(1.5), mm(1.5)), mm(0.2), false).0 as i64), "{widest_name}");
        assert!(t.row_heights.iter().all(|h| *h > mm(2.006)), "{:?}", t.row_heights);
        assert_eq!(t.layer, "F.SilkS");
        assert!(t.border.external && t.border.header && t.separators.rows && t.separators.cols);
        assert_eq!(t.border.stroke.width_nm, mm(0.1));
    }

    #[test]
    fn the_stackup_table_follows_the_units() {
        let stackup = default_stackup(2, mm(1.6));
        let mils = stackup_table("F.SilkS", &stackup, Units::Mils);
        // Row 4 is F.Cu: 0.035 mm = 1.37795 mils.
        assert_eq!(mils.cell(4, 3).unwrap().text, "1.37795 mils");
        let inches = stackup_table("F.SilkS", &stackup, Units::Inch);
        assert_eq!(inches.cell(4, 3).unwrap().text, "0.00137795 in");
    }

    #[test]
    fn a_layer_with_no_type_is_what_its_name_says() {
        let mut stackup = default_stackup(2, mm(1.6));
        for l in stackup.layers.iter_mut() {
            l.kind = None;
        }
        let t = stackup_table("F.SilkS", &stackup, Units::Mm);
        let types: Vec<String> = (1..t.row_count()).map(|r| t.cell(r, 1).unwrap().text.clone()).collect();
        assert_eq!(types, ["Top silk screen", "Top solder paste", "Top solder mask", "Copper", "Core", "Copper", "Bottom solder mask", "Bottom solder paste", "Bottom silk screen"]);
    }

    fn numbers() -> BoardNumbers {
        BoardNumbers { thickness_nm: 1_600_000, width_nm: 26_385_000, height_nm: 25_181_000, min_track_nm: 200_000, min_clearance_nm: 203_000, min_drill_nm: 300_000, castellated_pads: 0, press_fit_pads: 3 }
    }

    #[test]
    fn the_characteristics_table_is_a_title_and_label_value_pairs_two_to_a_row() {
        let mut stackup = default_stackup(4, mm(1.6));
        stackup.copper_finish = Some("ENIG".into());
        stackup.dielectric_constraints = true;
        stackup.edge_connector = 2;
        let t = characteristics_table("Dwgs.User", 4, &stackup, &numbers(), Units::Mm);
        assert_eq!((t.col_count(), t.row_count()), (4, 7));
        let row = |r: usize| (0..4).map(|c| t.cell(r, c).unwrap().text.clone()).collect::<Vec<_>>();
        assert_eq!(row(0), ["BOARD CHARACTERISTICS", "", "", ""]);
        assert_eq!((t.cell(0, 0).unwrap().col_span, t.cell(0, 1).unwrap().col_span, t.cell(0, 1).unwrap().row_span), (4, 0, 1));
        assert_eq!(row(1), ["Copper layer count: ", "4", "Board thickness: ", "1.6000 mm"]);
        assert_eq!(row(2), ["Board overall dimensions: ", "26.3850 mm x 25.1810 mm", "", ""]);
        assert_eq!(row(3), ["Min track/spacing: ", "0.2000 mm / 0.2030 mm", "Min hole diameter: ", "0.3000 mm"]);
        assert_eq!(row(4), ["Copper finish: ", "ENIG", "Impedance control: ", "Yes"]);
        assert_eq!(row(5), ["Castellated pads: ", "No", "Press-fit pads: ", "Yes"]);
        assert_eq!(row(6), ["Plated board edge: ", "No", "Edge card connectors: ", "Yes, Bevelled"]);
        // No lines at all, 2 mm / 0.4 mm title, 1.5 mm / 0.2 mm data.
        assert!(!t.border.external && !t.border.header && !t.separators.rows && !t.separators.cols);
        assert_eq!((t.cell(0, 0).unwrap().style.height_nm, t.cell(0, 0).unwrap().style.thickness_nm), (mm(2.0), mm(0.4)));
        assert_eq!((t.cell(1, 0).unwrap().style.height_nm, t.cell(1, 0).unwrap().style.thickness_nm), (mm(1.5), mm(0.2)));
        // Autosize looks at the title's ink for the first column, as KiCad does (it does not know the title spans four).
        assert!(t.col_widths[0] > Nm::new(ink_size("BOARD CHARACTERISTICS", (mm(2.0), mm(2.0)), mm(0.4), false).0 as i64));
    }

    /// The point of sizing a table from its ink: KiCad's own line breaking (`GetShownText` breaks to the column less the margins) does not break any cell of it.
    fn assert_no_cell_wraps(t: &Table) {
        use eda_model::table_text::pcb_cell_draw;
        use eda_model::text_vars::PcbBoard;
        let board = PcbBoard::default();
        for row in 0..t.row_count() {
            for col in 0..t.col_count() {
                let c = t.cell(row, col).unwrap();
                if c.is_covered() {
                    continue;
                }
                let draw = pcb_cell_draw(t, row, col, &board);
                // (A trailing space is dropped at the end of a line, as `LinebreakText` does; a break would add a line.)
                assert_eq!(draw.shown.trim_end(), c.text.trim_end(), "row {row}, column {col} broke its text to a width of {} um", t.col_widths[col]);
                assert!(draw.lines.len() <= 1, "row {row}, column {col}: {:?}", draw.lines);
            }
        }
    }

    #[test]
    fn no_cell_of_a_built_table_is_broken_into_lines_whatever_the_units() {
        let mut stackup = default_stackup(6, mm(1.6));
        stackup.copper_finish = Some("ENIG".into());
        for units in [Units::Mm, Units::Mils, Units::Inch] {
            assert_no_cell_wraps(&stackup_table("F.SilkS", &stackup, units));
            assert_no_cell_wraps(&characteristics_table("Dwgs.User", 6, &stackup, &numbers(), units));
        }
        let mut odd = default_stackup(2, mm(1.6));
        odd.layers[3].material = Some("a long material name for the Material column".into());
        odd.layers[3].color = Some("#336699FF".into());
        assert_no_cell_wraps(&stackup_table("B.SilkS", &odd, Units::Mm));
    }

    #[test]
    fn a_board_without_a_stackup_gets_the_default_one_for_its_layers() {
        let (n, s) = effective_stackup(None, 4, mm(1.6));
        assert_eq!((n, s.layers.len()), (4, default_stackup(4, mm(1.6)).layers.len()));
        let own = default_stackup(2, mm(1.0));
        assert_eq!(effective_stackup(Some((2, own.clone())), 4, mm(1.6)), (2, own));
    }
}
