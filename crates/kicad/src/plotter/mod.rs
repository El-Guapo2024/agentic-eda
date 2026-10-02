//! KiCad's `PLOTTER` class hierarchy, ported for the schematic plotter:
//! the abstract device (`include/plotters/plotter.h`,
//! `common/plotters/plotter.cpp`) plus its SVG and PDF back ends
//! ([`svg::SvgPlotter`] = `common/plotters/SVG_plotter.cpp`,
//! [`pdf::PdfPlotter`] = `common/plotters/PDF_plotter.cpp`).
//!
//! Units: like eeschema, *user* coordinates are internal units (IU) of
//! 0.1 um (`SCH_IU_PER_MM` = 10000, so 1 mil = 254 IU, 1 decimil = 25.4
//! IU). A plotter's *device* units differ per back end: SVG writes
//! millimetres (`m_iuPerDeviceUnit = 1 / iusPerMM`), PDF writes decimils
//! under a `0.0072 cm` scale (`m_iuPerDeviceUnit = 1 / IUsPerDecimil`).
//!
//! Text is always stroked here (`PLOT_TEXT_MODE::STROKE` -- the SVG
//! plotter's own default, see `SVG_PLOTTER::SVG_PLOTTER`): the Newstroke
//! font via [`eda_drc::stroke_font::glyph_strokes`], laid out by a port of
//! `FONT::getLinePositions` / `FONT::drawMarkup` /
//! `STROKE_FONT::GetTextAsGlyphs` ([`stroke_text_segments`]). The PDF
//! plotter's Type3-font text path (`PDF_PLOTTER::Text` with its
//! `PDF_STROKE_FONT_MANAGER`) is not ported: PDF text is plotted as the
//! same stroke segments, so it is vector art rather than selectable text.

pub mod pdf;
pub mod svg;

pub type Iu = i64;

/// Eeschema internal units per mil / decimil / mm (`schIUScale`).
pub const IU_PER_MILS: f64 = 254.0;
pub const IUS_PER_DECIMIL: f64 = IU_PER_MILS / 10.0;
pub const IU_PER_MM: f64 = 10_000.0;

/// `KiROUND`.
pub fn ki_round(v: f64) -> Iu {
    if v >= 0.0 {
        (v + 0.5).floor() as Iu
    } else {
        (v - 0.5).ceil() as Iu
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Pt {
    pub x: Iu,
    pub y: Iu,
}

impl Pt {
    pub fn new(x: Iu, y: Iu) -> Pt {
        Pt { x, y }
    }
}

/// `COLOR4D` -- components 0..1.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Color {
    pub r: f64,
    pub g: f64,
    pub b: f64,
    pub a: f64,
}

impl Color {
    pub const BLACK: Color = Color { r: 0.0, g: 0.0, b: 0.0, a: 1.0 };
    pub const WHITE: Color = Color { r: 1.0, g: 1.0, b: 1.0, a: 1.0 };

    /// `"#rrggbb"` or `"#rrggbbaa"`.
    pub fn from_hex(s: &str) -> Color {
        let h = s.trim_start_matches('#');
        let byte = |i: usize| u8::from_str_radix(h.get(i..i + 2).unwrap_or("00"), 16).unwrap_or(0) as f64 / 255.0;
        let a = if h.len() >= 8 { byte(6) } else { 1.0 };
        Color { r: byte(0), g: byte(2), b: byte(4), a }
    }
}

/// `FILL_T` (the hatch modes are unused by the schematic plot path).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FillT {
    NoFill,
    FilledShape,
    FilledWithBgBodyColor,
    FilledWithColor,
}

/// `LINE_STYLE`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LineStyle {
    Default,
    Solid,
    Dash,
    Dot,
    DashDot,
    DashDotDot,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HAlign {
    Left,
    Center,
    Right,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VAlign {
    Top,
    Center,
    Bottom,
}

/// The subset of `TEXT_ATTRIBUTES` the plotters use.
#[derive(Clone, Copy, Debug)]
pub struct TextAttrs {
    pub angle_deg: f64,
    pub size: (Iu, Iu),
    pub h: HAlign,
    pub v: VAlign,
    pub pen_width: Iu,
    pub italic: bool,
    pub bold: bool,
    pub mirrored: bool,
}

impl TextAttrs {
    pub fn new(size: Iu, h: HAlign, v: VAlign, pen_width: Iu) -> TextAttrs {
        TextAttrs { angle_deg: 0.0, size: (size, size), h, v, pen_width, italic: false, bold: false, mirrored: false }
    }
}

/// `PAGE_INFO`, in mils (`PAGE_INFO( MMsize( 297, 210 ), A4 )`).
#[derive(Clone, Copy, Debug)]
pub struct PageInfo {
    pub name: &'static str,
    pub width_mils: f64,
    pub height_mils: f64,
}

impl PageInfo {
    pub const A4: PageInfo = PageInfo { name: "A4", width_mils: 297.0 * 1000.0 / 25.4, height_mils: 210.0 * 1000.0 / 25.4 };
    /// `PAGE_INFO::GetWidthIU( IU_PER_MILS )`.
    pub fn width_iu(&self) -> Iu {
        (IU_PER_MILS * self.width_mils) as Iu
    }
    pub fn height_iu(&self) -> Iu {
        (IU_PER_MILS * self.height_mils) as Iu
    }
}

/// `RENDER_SETTINGS` dash metrics (`GetDashLength`/`GetGapLength`/
/// `GetDotLength`, ISO 128-2 ratios 12 and 3, `correction` = 1.0).
pub fn dash_len(w: Iu) -> f64 {
    (12.0f64 - 1.0).max(1.0) * w as f64
}
pub fn gap_len(w: Iu) -> f64 {
    (3.0f64 + 1.0).max(1.0) * w as f64
}
pub fn dot_len(w: Iu) -> f64 {
    (1.0f64 - 1.0).max(0.2) * w as f64
}

/// State every `PLOTTER` carries (`plotter.h`'s protected members).
#[derive(Clone, Debug)]
pub struct PlotterBase {
    pub color_mode: bool,
    pub page: PageInfo,
    /// `m_paperSize`, IU.
    pub paper_size: (f64, f64),
    pub plot_offset: Pt,
    pub plot_scale: f64,
    pub ius_per_decimil: f64,
    pub iu_per_device_unit: f64,
    pub plot_mirror: bool,
    pub yaxis_reversed: bool,
    pub current_pen_width: Iu,
    pub pen_state: char,
    pub pen_last_pos: Pt,
    pub creator: String,
    pub title: String,
    pub author: String,
    pub subject: String,
    pub filename: String,
}

impl PlotterBase {
    pub fn new() -> PlotterBase {
        PlotterBase {
            color_mode: false,
            page: PageInfo::A4,
            paper_size: (0.0, 0.0),
            plot_offset: Pt::default(),
            plot_scale: 1.0,
            ius_per_decimil: 1.0,
            iu_per_device_unit: 1.0,
            plot_mirror: false,
            yaxis_reversed: false,
            current_pen_width: -1,
            pen_state: 'Z',
            pen_last_pos: Pt::default(),
            creator: String::new(),
            title: String::new(),
            author: String::new(),
            subject: String::new(),
            filename: String::new(),
        }
    }

    /// `PLOTTER::userToDeviceCoordinates`.
    pub fn user_to_device(&self, c: Pt) -> (f64, f64) {
        let px = (c.x - self.plot_offset.x) as f64;
        let py = (c.y - self.plot_offset.y) as f64;
        let mut x = px * self.plot_scale;
        let mut y = self.paper_size.1 - py * self.plot_scale;
        if self.plot_mirror {
            x = self.paper_size.0 - px * self.plot_scale;
        }
        if self.yaxis_reversed {
            y = self.paper_size.1 - y;
        }
        (x * self.iu_per_device_unit, y * self.iu_per_device_unit)
    }

    /// `PLOTTER::userToDeviceSize( double )`.
    pub fn user_to_device_size(&self, size: f64) -> f64 {
        size * self.plot_scale * self.iu_per_device_unit
    }
}

impl Default for PlotterBase {
    fn default() -> Self {
        PlotterBase::new()
    }
}

/// EDA_ANGLE helper: degrees normalised to [0, 360).
fn norm360(a: f64) -> f64 {
    let mut r = a % 360.0;
    if r < 0.0 {
        r += 360.0;
    }
    r
}

/// EDA_ANGLE helper: degrees normalised to (-360, 0].
fn norm_neg(a: f64) -> f64 {
    let mut r = a % 360.0;
    if r > 0.0 {
        r -= 360.0;
    }
    r
}

/// `CalcArcCenter` (circumcentre of three points).
pub fn calc_arc_center(a: (f64, f64), b: (f64, f64), c: (f64, f64)) -> (f64, f64) {
    let d = 2.0 * (a.0 * (b.1 - c.1) + b.0 * (c.1 - a.1) + c.0 * (a.1 - b.1));
    if d.abs() < 1e-12 {
        return ((a.0 + c.0) / 2.0, (a.1 + c.1) / 2.0);
    }
    let a2 = a.0 * a.0 + a.1 * a.1;
    let b2 = b.0 * b.0 + b.1 * b.1;
    let c2 = c.0 * c.0 + c.1 * c.1;
    ((a2 * (b.1 - c.1) + b2 * (c.1 - a.1) + c2 * (a.1 - b.1)) / d, (a2 * (c.0 - b.0) + b2 * (a.0 - c.0) + c2 * (b.0 - a.0)) / d)
}

/// The abstract `PLOTTER`.
pub trait Plotter {
    fn base(&self) -> &PlotterBase;
    fn base_mut(&mut self) -> &mut PlotterBase;

    fn set_color_mode(&mut self, on: bool) {
        self.base_mut().color_mode = on;
    }
    fn color_mode(&self) -> bool {
        self.base().color_mode
    }
    fn current_line_width(&self) -> Iu {
        self.base().current_pen_width
    }

    fn set_current_line_width(&mut self, width: Iu);
    fn set_color(&mut self, color: Color);
    fn set_dash(&mut self, line_width: Iu, style: LineStyle);
    fn rect(&mut self, p1: Pt, p2: Pt, fill: FillT, width: Iu);
    fn circle(&mut self, pos: Pt, diameter: Iu, fill: FillT, width: Iu);
    /// `PLOTTER::Arc( center, startAngle, angle, radius, fill, width )`.
    fn arc(&mut self, center: (f64, f64), start_angle_deg: f64, angle_deg: f64, radius: f64, fill: FillT, width: Iu);
    fn plot_poly(&mut self, pts: &[Pt], fill: FillT, width: Iu);
    fn pen_to(&mut self, pos: Pt, plume: char);
    /// `PLOTTER::Text` -- the base-class stroke-font path; the SVG plotter
    /// overrides it to add its invisible `<text>` element first.
    fn text(&mut self, pos: Pt, color: Color, text: &str, attrs: &TextAttrs) {
        self.stroke_text(pos, color, text, attrs);
    }

    fn move_to(&mut self, p: Pt) {
        self.pen_to(p, 'U');
    }
    fn line_to(&mut self, p: Pt) {
        self.pen_to(p, 'D');
    }
    fn finish_to(&mut self, p: Pt) {
        self.pen_to(p, 'D');
        self.pen_to(p, 'Z');
    }
    fn pen_finish(&mut self) {
        self.pen_to(Pt::default(), 'Z');
    }

    /// `PLOTTER::Arc( start, mid, end, fill, width )`.
    fn arc_3pt(&mut self, start: (f64, f64), mid: (f64, f64), end: (f64, f64), fill: FillT, width: Iu) {
        let center = calc_arc_center(start, mid, end);
        let start_angle = (start.1 - center.1).atan2(start.0 - center.0).to_degrees();
        let end_angle = (end.1 - center.1).atan2(end.0 - center.0).to_degrees();
        // < 0: left, 0: on the line, > 0: right
        let det = (end.0 - start.0) * (mid.1 - start.1) - (end.1 - start.1) * (mid.0 - start.0);
        let cw = det <= 0.0;
        let raw = end_angle - start_angle;
        let angle = if cw { norm360(raw) } else { norm_neg(raw) };
        let radius = ((start.0 - center.0).powi(2) + (start.1 - center.1).powi(2)).sqrt();
        self.arc(center, start_angle, angle, radius, fill, width);
    }

    /// `PLOTTER::ThickSegment`.
    fn thick_segment(&mut self, start: Pt, end: Pt, width: Iu) {
        if start == end {
            self.circle(start, width, FillT::FilledShape, 0);
        } else {
            self.set_current_line_width(width);
            self.move_to(start);
            self.finish_to(end);
        }
    }
    fn thick_circle(&mut self, pos: Pt, diameter: Iu, width: Iu) {
        self.circle(pos, diameter, FillT::NoFill, width);
    }
    fn filled_circle(&mut self, pos: Pt, diameter: Iu) {
        self.circle(pos, diameter, FillT::FilledShape, 0);
    }

    /// `PLOTTER::Text`'s body: the stroke callback plots every glyph
    /// segment as `SetCurrentLineWidth; MoveTo; LineTo; PenFinish`.
    fn stroke_text(&mut self, pos: Pt, color: Color, text: &str, attrs: &TextAttrs) {
        self.set_color(color);
        let mut pen = attrs.pen_width;
        if pen == 0 && attrs.bold {
            pen = ki_round(attrs.size.0.min(attrs.size.1) as f64 / 5.0);
        }
        if pen < 0 {
            pen = -pen;
        }
        for (a, b) in stroke_text_segments(text, pos, attrs, pen) {
            self.set_current_line_width(pen);
            self.move_to(a);
            self.line_to(b);
            self.pen_finish();
        }
    }
}

// ---------------------------------------------------------------- stroke font text

const INTER_CHAR: f64 = 0.2;
const SUPER_SUB_SIZE_MULTIPLIER: f64 = 0.8;
const SUPER_HEIGHT_OFFSET: f64 = 0.35;
const SUB_HEIGHT_OFFSET: f64 = 0.15;
const ITALIC_TILT: f64 = 1.0 / 8.0;
/// `METRICS::m_OverbarHeight` (`include/font/font_metrics.h`).
const OVERBAR_HEIGHT: f64 = 1.23;

#[derive(Clone, Copy, Default)]
struct StyleFlags {
    sub: bool,
    sup: bool,
}

/// One run of text under one markup style (the flat equivalent of a
/// `MARKUP::NODE` with content).
struct Run {
    text: String,
    style: StyleFlags,
    /// Index of the overbar group this run belongs to (consecutive runs of
    /// one `~{...}` share a bar, as `drawMarkup` draws one bar per node).
    bar_group: Option<usize>,
}

/// `MARKUP_PARSER` for the three constructs it knows: `~{overbar}`,
/// `_{subscript}`, `^{superscript}` (nestable).
fn parse_markup(text: &str) -> Vec<Run> {
    let chars: Vec<char> = text.chars().collect();
    let mut runs: Vec<Run> = Vec::new();
    let mut stack: Vec<(char, Option<usize>)> = Vec::new();
    let mut bar_groups = 0usize;
    let mut cur = String::new();
    let style_of = |stack: &[(char, Option<usize>)]| StyleFlags {
        sub: stack.iter().any(|s| s.0 == '_'),
        sup: stack.iter().any(|s| s.0 == '^'),
    };
    let bar_of = |stack: &[(char, Option<usize>)]| stack.iter().rev().find_map(|s| s.1);
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if matches!(c, '~' | '_' | '^') && chars.get(i + 1) == Some(&'{') {
            if !cur.is_empty() {
                runs.push(Run { text: std::mem::take(&mut cur), style: style_of(&stack), bar_group: bar_of(&stack) });
            }
            let g = if c == '~' {
                bar_groups += 1;
                Some(bar_groups - 1)
            } else {
                None
            };
            stack.push((c, g));
            i += 2;
            continue;
        }
        if c == '}' && !stack.is_empty() {
            if !cur.is_empty() {
                runs.push(Run { text: std::mem::take(&mut cur), style: style_of(&stack), bar_group: bar_of(&stack) });
            }
            stack.pop();
            i += 1;
            continue;
        }
        cur.push(c);
        i += 1;
    }
    if !cur.is_empty() {
        runs.push(Run { text: cur, style: style_of(&stack), bar_group: bar_of(&stack) });
    }
    runs
}

/// A glyph point set in absolute (pre-rotation) user coordinates.
type Polylines = Vec<Vec<(f64, f64)>>;

/// `STROKE_FONT::GetTextAsGlyphs` for one run: appends the transformed
/// (size, tilt, offset) stroke polylines and returns the new cursor x.
fn glyphs_for_run(run_text: &str, style: StyleFlags, size: (Iu, Iu), position: (f64, f64), italic: bool, out: &mut Polylines) -> f64 {
    let mut cursor = (position.0.round() as Iu, position.1.round() as Iu);
    let mut glyph_size = (size.0 as f64, size.1 as f64);
    let tilt = if italic { ITALIC_TILT } else { 0.0 };
    let space_width = eda_drc::stroke_font::glyph_strokes(' ' as u32).width;
    if style.sub || style.sup {
        glyph_size = (glyph_size.0 * SUPER_SUB_SIZE_MULTIPLIER, glyph_size.1 * SUPER_SUB_SIZE_MULTIPLIER);
        if style.sub {
            cursor.1 += (glyph_size.1 * SUB_HEIGHT_OFFSET) as Iu;
        } else {
            cursor.1 -= (glyph_size.1 * SUPER_HEIGHT_OFFSET) as Iu;
        }
    }
    for c in run_text.chars() {
        if c == ' ' {
            cursor.0 += ki_round(glyph_size.0 * space_width);
        } else if c == '\t' {
            // `TAB_WIDTH` columns are locked to the next 4th base-width; the
            // schematic never carries tabs, treat as 4 spaces.
            cursor.0 += ki_round(glyph_size.0 * space_width * 4.0);
        } else {
            let g = eda_drc::stroke_font::glyph_strokes(c as u32);
            for stroke in &g.strokes {
                let mut line = Vec::with_capacity(stroke.len());
                for &(gx, gy) in stroke {
                    let mut x = gx * glyph_size.0;
                    let y = gy * glyph_size.1;
                    if tilt != 0.0 {
                        x -= y * tilt;
                    }
                    line.push((x + cursor.0 as f64, y + cursor.1 as f64));
                }
                out.push(line);
            }
            cursor.0 += ki_round(g.width * glyph_size.0);
        }
    }
    cursor.0 as f64
}

/// `FONT::drawMarkup` + `drawSingleLineText`: stroke polylines of one
/// line, positioned at `position` (before rotation about `origin`).
/// Returns the polylines and the cursor's end x.
fn single_line_polylines(text: &str, position: (f64, f64), size: (Iu, Iu), italic: bool) -> (Polylines, f64) {
    let runs = parse_markup(text);
    let mut out: Polylines = Vec::new();
    let mut next_x = position.0;
    let mut bars: Vec<(usize, f64, f64)> = Vec::new(); // (group, start x, end x)
    for run in &runs {
        let start_x = next_x;
        next_x = glyphs_for_run(&run.text, run.style, size, (next_x, position.1), italic, &mut out);
        if let Some(g) = run.bar_group {
            match bars.iter_mut().find(|b| b.0 == g) {
                Some(b) => b.2 = next_x,
                None => bars.push((g, start_x, next_x)),
            }
        }
    }
    // Overbars: `drawMarkup` trims the bar by 0.1 * size.x at both ends and
    // lifts it by `GetOverbarVerticalPosition( size.y )`.
    for (_, x0, x1) in bars {
        let trim = size.0 as f64 * 0.1;
        let off = size.1 as f64 * OVERBAR_HEIGHT;
        out.push(vec![(x0 + trim, position.1 - off), (x1 - trim, position.1 - off)]);
    }
    (out, next_x)
}

/// `FONT::getLinePositions` + `FONT::Draw` for the stroke font, returned
/// as one `(a, b)` segment per consecutive glyph-point pair --
/// `CALLBACK_GAL::DrawGlyph`'s own stroke-callback granularity.
pub fn stroke_text_segments(text: &str, anchor: Pt, attrs: &TextAttrs, pen_width: Iu) -> Vec<(Pt, Pt)> {
    if text.is_empty() {
        return Vec::new();
    }
    let size = (attrs.size.0.abs(), attrs.size.1.abs());
    let interline = ki_round(size.1 as f64 * 1.68 * 0.9583);
    let lines: Vec<&str> = text.split('\n').collect();

    // getLinePositions
    let mut extents: Vec<f64> = Vec::new();
    let mut height: Iu = 0;
    for (i, l) in lines.iter().enumerate() {
        let (_, end_x) = single_line_polylines(l, (0.0, 0.0), size, attrs.italic);
        extents.push(end_x);
        if i == 0 {
            height += (size.1 as f64 * 1.17) as Iu;
        } else {
            height += interline;
        }
    }
    let mut off = (0 as Iu, size.1);
    // Fudge factors to match 6.0 positioning (stroke fonts).
    off.0 += (pen_width as f64 / 1.52) as Iu;
    off.1 -= (pen_width as f64 * 0.052) as Iu;
    match attrs.v {
        VAlign::Top => {}
        VAlign::Center => off.1 -= height / 2,
        VAlign::Bottom => off.1 -= height,
    }

    let rad = attrs.angle_deg.to_radians();
    let (sin, cos) = rad.sin_cos();
    let mut segs: Vec<(Pt, Pt)> = Vec::new();
    for (i, l) in lines.iter().enumerate() {
        let line_size_x = extents[i] as Iu;
        let mut lo = (off.0, off.1 + i as Iu * interline);
        match attrs.h {
            HAlign::Left => {}
            HAlign::Center => lo.0 = -line_size_x / 2,
            HAlign::Right => lo.0 = -(line_size_x + off.0),
        }
        let pos = ((anchor.x + lo.0) as f64, (anchor.y + lo.1) as f64);
        let (polys, _) = single_line_polylines(l, pos, size, attrs.italic);
        for poly in polys {
            let pts: Vec<Pt> = poly
                .iter()
                .map(|&(mut x, y)| {
                    // `STROKE_GLYPH::Transform`: mirror about the anchor, then rotate about it.
                    if attrs.mirrored {
                        x = anchor.x as f64 - (x - anchor.x as f64);
                    }
                    let (dx, dy) = (x - anchor.x as f64, y - anchor.y as f64);
                    // `RotatePoint`: x' = x cos + y sin, y' = y cos - x sin.
                    let (rx, ry) = if attrs.angle_deg == 0.0 { (dx, dy) } else { (dx * cos + dy * sin, dy * cos - dx * sin) };
                    Pt::new(ki_round(anchor.x as f64 + rx), ki_round(anchor.y as f64 + ry))
                })
                .collect();
            for w in pts.windows(2) {
                segs.push((w[0], w[1]));
            }
        }
    }
    segs
}

/// `GRTextWidth` / `FONT::StringBoundaryLimits().x` for a stroke font:
/// the markup bounding box width, inflated by `1.5 * thickness` each side.
pub fn text_width(text: &str, size: (Iu, Iu), thickness: Iu, italic: bool) -> Iu {
    let size = (size.0.abs(), size.1.abs());
    let (_, end_x) = single_line_polylines(text, (0.0, 0.0), size, italic);
    // The bbox ends at `cursor.x - INTER_CHAR * size.x` (`GetTextAsGlyphs`).
    let w = end_x - ki_round(size.0 as f64 * INTER_CHAR) as f64;
    w.round() as Iu + 2 * ki_round(thickness as f64 * 1.5)
}

/// `{:g}` as `PDF_PLOTTER::encodeDoubleForPlotter` post-processes it:
/// six significant digits, never an exponent, no trailing zeros, no `-0`.
pub fn fmt_g(v: f64) -> String {
    let mut buf = if v == 0.0 {
        "0".to_string()
    } else {
        let exp10 = v.abs().log10().floor() as i32;
        if !(-4..6).contains(&exp10) {
            String::from("e")
        } else {
            let decimals = (5 - exp10).max(0) as usize;
            format!("{:.*}", decimals, v)
        }
    };
    if buf.contains('e') || buf.contains('E') {
        buf = format!("{:.10}", v);
    }
    if buf.contains('.') {
        while buf.len() > 1 && buf.ends_with('0') {
            buf.pop();
        }
        if buf.ends_with('.') {
            buf.pop();
        }
    }
    if buf == "-0" {
        buf = "0".to_string();
    }
    buf
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fmt_g_matches_pdf_plotter_rules() {
        assert_eq!(fmt_g(0.0072), "0.0072");
        assert_eq!(fmt_g(1.0), "1");
        assert_eq!(fmt_g(595.2756), "595.276");
        assert_eq!(fmt_g(-0.0), "0");
        assert_eq!(fmt_g(0.00001234), "0.0000123400".trim_end_matches('0'));
    }

    #[test]
    fn stroke_text_has_segments_and_respects_alignment() {
        let a = TextAttrs::new(12700, HAlign::Left, VAlign::Center, 1524);
        let left = stroke_text_segments("AB", Pt::new(0, 0), &a, 1524);
        assert!(!left.is_empty());
        let mut c = a;
        c.h = HAlign::Center;
        let centre = stroke_text_segments("AB", Pt::new(0, 0), &c, 1524);
        let min_x = |s: &[(Pt, Pt)]| s.iter().map(|(p, q)| p.x.min(q.x)).min().unwrap();
        // Centre-justified text starts left of the anchor; left-justified starts right of it.
        assert!(min_x(&centre) < min_x(&left));
    }

    #[test]
    fn overbar_markup_adds_a_bar_above_the_text() {
        let a = TextAttrs::new(12700, HAlign::Left, VAlign::Bottom, 1524);
        let plain = stroke_text_segments("RESET", Pt::new(0, 0), &a, 1524);
        let barred = stroke_text_segments("~{RESET}", Pt::new(0, 0), &a, 1524);
        assert_eq!(barred.len(), plain.len() + 1);
    }

    #[test]
    fn arc_three_point_centre() {
        let c = calc_arc_center((1.0, 0.0), (0.0, 1.0), (-1.0, 0.0));
        assert!(c.0.abs() < 1e-9 && c.1.abs() < 1e-9);
    }
}
