//! `SVG_PLOTTER` -- port of `common/plotters/SVG_plotter.cpp` (constructor,
//! `SetViewport`, `setSVGPlotStyle`, `SetCurrentLineWidth`, `emitSetRGBColor`
//! (via `PSLIKE_PLOTTER::SetColor`), `SetDash`, `Rect`, `Circle`, `Arc`,
//! `PlotPoly`, `PenTo`, `StartPlot`, `EndPlot`, `Text`).
//!
//! SVG user units are millimetres, written with a fixed four-digit mantissa
//! (`m_precision`, "0.1 micron resolution"). Like KiCad, graphics-state
//! changes (colour / line width / fill / dash) close the current `<g>` and
//! open a new one lazily, the next time something is drawn.

use super::*;
use std::fmt::Write as _;

pub struct SvgPlotter {
    base: PlotterBase,
    out: String,
    graphics_changed: bool,
    fill_mode: FillT,
    pen_rgb: u32,
    brush_rgb: u32,
    brush_alpha: f64,
    dashed: LineStyle,
    precision: usize,
    /// `GetISO8601CurrentDateTime()` -- passed in so output is deterministic.
    date: String,
}

/// `XmlEsc` (`SVG_plotter.cpp`).
pub fn xml_esc(s: &str, is_attribute: bool) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '&' => out.push_str("&amp;"),
            '\r' => out.push_str("&#xD;"),
            '"' if is_attribute => out.push_str("&quot;"),
            '\t' if is_attribute => out.push_str("&#x9;"),
            '\n' if is_attribute => out.push_str("&#xA;"),
            _ => out.push(c),
        }
    }
    out
}

impl SvgPlotter {
    /// `SVG_PLOTTER::SVG_PLOTTER` -- `PLOT_TEXT_MODE::STROKE`, black pen.
    pub fn new(date: &str) -> SvgPlotter {
        SvgPlotter {
            base: PlotterBase::new(),
            out: String::new(),
            graphics_changed: true,
            fill_mode: FillT::NoFill,
            pen_rgb: 0,
            brush_rgb: 0,
            brush_alpha: 1.0,
            dashed: LineStyle::Solid,
            precision: 4,
            date: date.to_string(),
        }
    }

    pub fn set_creator(&mut self, s: &str) {
        self.base.creator = s.to_string();
    }
    pub fn set_page_settings(&mut self, page: PageInfo) {
        self.base.page = page;
    }
    /// The file name `<title>` mentions (`wxFileName( m_filename ).GetFullName()`).
    pub fn set_file_name(&mut self, name: &str) {
        self.base.filename = name.to_string();
    }

    /// `SVG_PLOTTER::SetViewport`.
    pub fn set_viewport(&mut self, offset: Pt, ius_per_decimil: f64, scale: f64, mirror: bool) {
        let b = &mut self.base;
        b.plot_mirror = mirror;
        b.yaxis_reversed = true; // unlike other plotters, SVG has Y axis reversed
        b.plot_offset = offset;
        b.plot_scale = scale;
        b.ius_per_decimil = ius_per_decimil;
        // paper size in IUs (the page size is in mils)
        b.paper_size = (b.page.width_mils * 10.0 * ius_per_decimil, b.page.height_mils * 10.0 * ius_per_decimil);
        let ius_per_mm = ius_per_decimil / 2.54 * 1000.0;
        b.iu_per_device_unit = 1.0 / ius_per_mm;
        self.precision = 4;
    }

    fn p(&self, v: f64) -> String {
        format!("{:.*}", self.precision, v)
    }

    fn set_fill_mode(&mut self, fill: FillT) {
        if self.fill_mode != fill {
            self.graphics_changed = true;
            self.fill_mode = fill;
        }
    }

    /// `SVG_PLOTTER::setSVGPlotStyle`.
    fn set_svg_plot_style(&mut self, line_width: Iu, is_group: bool, extra_style: &str) {
        if is_group {
            self.out.push_str("</g>\n<g ");
        }
        self.out.push_str("style=\"");
        if self.fill_mode == FillT::NoFill {
            self.out.push_str("fill:none; ");
        } else {
            let _ = write!(self.out, "fill:#{:06X}; ", self.brush_rgb);
            let _ = write!(self.out, "fill-opacity:{}; ", self.p(self.brush_alpha));
        }
        let pen_w = self.base.user_to_device_size(line_width as f64);
        if pen_w <= 0.0 {
            self.out.push_str("stroke:none;");
        } else {
            let _ = write!(self.out, "\nstroke:#{:06X}; stroke-width:{}; stroke-opacity:1; \n", self.pen_rgb, self.p(pen_w));
            self.out.push_str("stroke-linecap:round; stroke-linejoin:round;");
            let dash = |s: &Self, v: f64| s.base.user_to_device_size(v);
            match self.dashed {
                LineStyle::Dash => {
                    let _ = write!(self.out, "stroke-dasharray:{},{};", self.p(dash(self, dash_len(line_width))), self.p(dash(self, gap_len(line_width))));
                }
                LineStyle::Dot => {
                    let _ = write!(self.out, "stroke-dasharray:{:.6},{:.6};", dash(self, dot_len(line_width)), dash(self, gap_len(line_width)));
                }
                LineStyle::DashDot => {
                    let _ = write!(self.out, "stroke-dasharray:{:.6},{:.6},{:.6},{:.6};", dash(self, dash_len(line_width)), dash(self, gap_len(line_width)), dash(self, dot_len(line_width)), dash(self, gap_len(line_width)));
                }
                LineStyle::DashDotDot => {
                    let _ = write!(
                        self.out,
                        "stroke-dasharray:{:.6},{:.6},{:.6},{:.6},{:.6},{:.6};",
                        dash(self, dash_len(line_width)),
                        dash(self, gap_len(line_width)),
                        dash(self, dot_len(line_width)),
                        dash(self, gap_len(line_width)),
                        dash(self, dot_len(line_width)),
                        dash(self, gap_len(line_width))
                    );
                }
                LineStyle::Default | LineStyle::Solid => {}
            }
        }
        if !extra_style.is_empty() {
            self.out.push_str(extra_style);
        }
        self.out.push('"');
        if is_group {
            self.out.push('>');
            self.graphics_changed = false;
        }
        self.out.push('\n');
    }

    fn style_if_changed(&mut self) {
        if self.graphics_changed {
            let w = self.current_line_width();
            self.set_svg_plot_style(w, true, "");
        }
    }

    /// `SVG_PLOTTER::StartPlot`.
    pub fn start_plot(&mut self, _page_number: &str) {
        self.out.push_str(
            "<?xml version=\"1.0\" standalone=\"no\"?>\n <!DOCTYPE svg PUBLIC \"-//W3C//DTD SVG 1.1//EN\" \n \"http://www.w3.org/Graphics/SVG/1.1/DTD/svg11.dtd\"> \n<svg\n  xmlns:svg=\"http://www.w3.org/2000/svg\"\n  xmlns=\"http://www.w3.org/2000/svg\"\n  xmlns:xlink=\"http://www.w3.org/1999/xlink\"\n  xmlns:inkscape=\"http://www.inkscape.org/namespaces/inkscape\"\n  version=\"1.1\"\n",
        );
        let ius_per_decimil = self.base.ius_per_decimil;
        let (pw, ph) = self.base.paper_size;
        let div = self.base.iu_per_device_unit;
        let _ = writeln!(
            self.out,
            "  width=\"{}mm\" height=\"{}mm\" viewBox=\"{} {} {} {}\">",
            self.p(pw / ius_per_decimil * 2.54 / 1000.0),
            self.p(ph / ius_per_decimil * 2.54 / 1000.0),
            self.p(0.0),
            self.p(0.0),
            self.p(pw * div),
            self.p(ph * div)
        );
        let _ = writeln!(self.out, "<title>SVG Image created as {} date {} </title>", xml_esc(&self.base.filename, false), self.date);
        let _ = writeln!(self.out, "  <desc>Image generated by {} </desc>", xml_esc(&self.base.creator, false));
        let _ = write!(
            self.out,
            "<g style=\"fill:#{:06X}; fill-opacity:{};stroke:#{:06X}; stroke-opacity:{};\n",
            self.brush_rgb,
            self.p(self.brush_alpha),
            self.pen_rgb,
            self.p(1.0)
        );
        self.out.push_str("stroke-linecap:round; stroke-linejoin:round;\"\n");
        self.out.push_str(" transform=\"translate(0 0) scale(1 1)\">\n");
    }

    /// `SVG_PLOTTER::EndPlot` -- returns the finished document.
    pub fn end_plot(&mut self) -> String {
        self.out.push_str("</g> \n</svg>\n");
        std::mem::take(&mut self.out)
    }
}

impl Plotter for SvgPlotter {
    fn base(&self) -> &PlotterBase {
        &self.base
    }
    fn base_mut(&mut self) -> &mut PlotterBase {
        &mut self.base
    }

    fn set_current_line_width(&mut self, width: Iu) {
        // Note: width == 0 is fine: used for filled shapes with no outline thickness
        if width != self.base.current_pen_width {
            self.graphics_changed = true;
            self.base.current_pen_width = width;
        }
    }

    /// `PSLIKE_PLOTTER::SetColor` + `SVG_PLOTTER::emitSetRGBColor`.
    fn set_color(&mut self, color: Color) {
        let (r, g, b, a) = if self.base.color_mode {
            (color.r, color.g, color.b, color.a)
        } else {
            // B/W mode: BLACK or WHITE only.
            let k = if color == Color::WHITE { 1.0 } else { 0.0 };
            (k, k, k, 1.0)
        };
        let rgb = (((255.0 * r) as u32) << 16) | (((255.0 * g) as u32) << 8) | ((255.0 * b) as u32);
        if self.pen_rgb != rgb || self.brush_alpha != a {
            self.graphics_changed = true;
            self.pen_rgb = rgb;
            // Currently, use the same color for brush and pen.
            self.brush_rgb = rgb;
            self.brush_alpha = a;
        }
    }

    fn set_dash(&mut self, _line_width: Iu, style: LineStyle) {
        if self.dashed != style {
            self.graphics_changed = true;
            self.dashed = style;
        }
    }

    fn rect(&mut self, p1: Pt, p2: Pt, fill: FillT, width: Iu) {
        let (x0, x1) = (p1.x.min(p2.x), p1.x.max(p2.x));
        let (y0, y1) = (p1.y.min(p2.y), p1.y.max(p2.y));
        let org = self.base.user_to_device(Pt::new(x0, y0));
        let end = self.base.user_to_device(Pt::new(x1, y1));
        // BOX2D::Normalize
        let (ox, oy) = (org.0.min(end.0), org.1.min(end.1));
        let (sx, sy) = ((end.0 - org.0).abs(), (end.1 - org.1).abs());

        self.set_fill_mode(fill);
        self.set_current_line_width(width);
        self.style_if_changed();

        // Rectangles having a 0 size value for height or width are just not
        // drawn on Inkscape, so use a line when happens.
        if sx == 0.0 || sy == 0.0 {
            let _ = writeln!(self.out, "<line x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\" />", self.p(ox), self.p(oy), self.p(ox + sx), self.p(oy + sy));
        } else {
            let _ = writeln!(self.out, "<rect x=\"{:.6}\" y=\"{:.6}\" width=\"{:.6}\" height=\"{:.6}\" rx=\"{:.6}\" />", ox, oy, sx, sy, 0.0);
        }
    }

    fn circle(&mut self, pos: Pt, diameter: Iu, fill: FillT, width: Iu) {
        let pos_dev = self.base.user_to_device(pos);
        let mut radius = self.base.user_to_device_size(diameter as f64 / 2.0);
        self.set_fill_mode(fill);
        self.set_current_line_width(width);
        self.style_if_changed();

        // If diameter is less than width, switch to filled mode
        if fill == FillT::NoFill && diameter < self.current_line_width() {
            self.set_fill_mode(FillT::FilledShape);
            let w = self.current_line_width();
            self.set_current_line_width(0);
            radius = self.base.user_to_device_size(diameter as f64 / 2.0 + w as f64 / 2.0);
        }
        let _ = writeln!(self.out, "<circle cx=\"{}\" cy=\"{}\" r=\"{}\" /> ", self.p(pos_dev.0), self.p(pos_dev.1), self.p(radius));
    }

    fn arc(&mut self, center: (f64, f64), start_angle_deg: f64, angle_deg: f64, radius: f64, fill: FillT, width: Iu) {
        if radius <= 0.0 {
            self.circle(Pt::new(ki_round(center.0), ki_round(center.1)), width, FillT::FilledShape, 0);
            return;
        }
        let mut start_angle = -start_angle_deg;
        let mut end_angle = start_angle - angle_deg;
        if end_angle < start_angle {
            std::mem::swap(&mut start_angle, &mut end_angle);
        }
        let centre_dev = self.base.user_to_device(Pt::new(ki_round(center.0), ki_round(center.1)));
        let radius_dev = self.base.user_to_device_size(radius);
        if self.base.plot_mirror {
            // mirrorIsHorizontal is always true for the schematic plotter.
            std::mem::swap(&mut start_angle, &mut end_angle);
            start_angle = 180.0 - start_angle;
            end_angle = 180.0 - end_angle;
        }
        // RotatePoint( (r, 0), angle ): ( r cos a, -r sin a )
        let rot = |a: f64| (radius_dev * a.to_radians().cos(), -radius_dev * a.to_radians().sin());
        let s = rot(start_angle);
        let e = rot(end_angle);
        let start = (s.0 + centre_dev.0, s.1 + centre_dev.1);
        let end = (e.0 + centre_dev.0, e.1 + centre_dev.1);

        let tau = std::f64::consts::PI * 2.0;
        let mut theta1 = start_angle.to_radians();
        if theta1 < 0.0 {
            theta1 += tau;
        }
        let mut theta2 = end_angle.to_radians();
        if theta2 < 0.0 {
            theta2 += tau;
        }
        if theta2 < theta1 {
            theta2 += tau;
        }
        let flg_arc = if (theta2 - theta1).abs() > std::f64::consts::PI { 1 } else { 0 };
        let flg_sweep = 0;

        if fill != FillT::NoFill {
            // Filled arcs (in Eeschema) consist of the pie wedge and a stroke only on the arc.
            self.set_fill_mode(fill);
            self.set_current_line_width(0);
            self.style_if_changed();
            let _ = writeln!(
                self.out,
                "<path d=\"M{} {} A{} {} 0.0 {} {} {} {} L {} {} Z\" />",
                self.p(start.0),
                self.p(start.1),
                self.p(radius_dev),
                self.p(radius_dev),
                flg_arc,
                flg_sweep,
                self.p(end.0),
                self.p(end.1),
                self.p(centre_dev.0),
                self.p(centre_dev.1)
            );
        }
        self.set_fill_mode(FillT::NoFill);
        self.set_current_line_width(width);
        self.style_if_changed();
        let _ = writeln!(
            self.out,
            "<path d=\"M{} {} A{} {} 0.0 {} {} {} {}\" />",
            self.p(start.0),
            self.p(start.1),
            self.p(radius_dev),
            self.p(radius_dev),
            flg_arc,
            flg_sweep,
            self.p(end.0),
            self.p(end.1)
        );
    }

    fn plot_poly(&mut self, pts: &[Pt], fill: FillT, width: Iu) {
        if pts.len() <= 1 {
            return;
        }
        self.set_fill_mode(fill);
        self.set_current_line_width(width);
        self.out.push_str("<path ");
        match fill {
            FillT::NoFill => self.set_svg_plot_style(width, false, "fill:none"),
            _ => self.set_svg_plot_style(width, false, "fill-rule:evenodd;"),
        }
        let pos = self.base.user_to_device(pts[0]);
        let _ = write!(self.out, "d=\"M {},{}\n", self.p(pos.0), self.p(pos.1));
        for p in &pts[1..pts.len() - 1] {
            let d = self.base.user_to_device(*p);
            let _ = writeln!(self.out, "{},{}", self.p(d.0), self.p(d.1));
        }
        // If the corner list ends where it begins, then close the poly
        if pts.first() == pts.last() {
            self.out.push_str("Z\" /> \n");
        } else {
            let d = self.base.user_to_device(*pts.last().unwrap());
            let _ = write!(self.out, "{},{}\n\" /> \n", self.p(d.0), self.p(d.1));
        }
    }

    fn pen_to(&mut self, pos: Pt, plume: char) {
        if plume == 'Z' {
            if self.base.pen_state != 'Z' {
                self.out.push_str("\" />\n");
                self.base.pen_state = 'Z';
                self.base.pen_last_pos = Pt::new(-1, -1);
            }
            return;
        }
        if self.base.pen_state == 'Z' {
            // here plume = 'D' or 'U'
            let pos_dev = self.base.user_to_device(pos);
            // Ensure we do not use a fill mode when moving the pen.
            if self.fill_mode != FillT::NoFill {
                self.set_fill_mode(FillT::NoFill);
            }
            self.style_if_changed();
            let _ = writeln!(self.out, "<path d=\"M{} {}", self.p(pos_dev.0), self.p(pos_dev.1));
        } else if self.base.pen_state != plume || pos != self.base.pen_last_pos {
            self.style_if_changed();
            let pos_dev = self.base.user_to_device(pos);
            let _ = writeln!(self.out, "L{} {}", self.p(pos_dev.0), self.p(pos_dev.1));
        }
        self.base.pen_state = plume;
        self.base.pen_last_pos = pos;
    }

    /// `SVG_PLOTTER::Text`: an invisible `<text>` element (for search and
    /// accessibility), then the glyph strokes inside `<g class="stroked-text">`.
    fn text(&mut self, pos: Pt, color: Color, text: &str, attrs: &TextAttrs) {
        self.set_fill_mode(FillT::NoFill);
        self.set_color(color);
        self.set_current_line_width(attrs.pen_width);
        self.style_if_changed();

        let mut text_pos = pos;
        let hjust = match attrs.h {
            HAlign::Center => "middle",
            HAlign::Right => "end",
            HAlign::Left => "start",
        };
        match attrs.v {
            VAlign::Center => text_pos.y += attrs.size.1 / 2,
            VAlign::Top => text_pos.y += attrs.size.1,
            VAlign::Bottom => {}
        }
        // aSize.x < 0 flags a mirrored text.
        let size_x = if attrs.mirrored { -attrs.size.0 } else { attrs.size.0 };
        let text_w = text_width(text, attrs.size, self.current_line_width(), attrs.italic).abs();
        let text_h = (attrs.size.0 * 4 / 3).abs();
        let anchor_dev = self.base.user_to_device(pos);
        let text_dev = self.base.user_to_device(text_pos);
        let sz_w = self.base.user_to_device_size(text_w as f64);
        let sz_h = self.base.user_to_device_size(text_h as f64);

        if attrs.angle_deg != 0.0 {
            let a = if self.base.plot_mirror { attrs.angle_deg } else { -attrs.angle_deg };
            let _ = writeln!(self.out, "<g transform=\"rotate({:.6} {} {})\">", a, self.p(anchor_dev.0), self.p(anchor_dev.1));
        }
        let _ = writeln!(self.out, "<text x=\"{}\" y=\"{}\"", self.p(text_dev.0), self.p(text_dev.1));
        // If the text is mirrored, we should also mirror the hidden text to match
        if self.base.plot_mirror != (size_x < 0) {
            let _ = writeln!(self.out, "transform=\"scale(-1 1) translate({:.6} 0)\"", -2.0 * text_dev.0);
        }
        let _ = writeln!(
            self.out,
            "textLength=\"{}\" font-size=\"{}\" lengthAdjust=\"spacingAndGlyphs\"\ntext-anchor=\"{}\" opacity=\"0\" stroke-opacity=\"0\">{}</text>",
            self.p(sz_w),
            self.p(sz_h),
            hjust,
            xml_esc(text, false)
        );
        if attrs.angle_deg != 0.0 {
            self.out.push_str("</g>\n");
        }

        // The same text again as graphics with a <desc> tag.
        let _ = writeln!(self.out, "<g class=\"stroked-text\"><desc>{}</desc>", xml_esc(text, false));
        let mut a = *attrs;
        a.pen_width = self.current_line_width();
        self.stroke_text(pos, color, text, &a);
        self.out.push_str("</g>");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plotter() -> SvgPlotter {
        let mut p = SvgPlotter::new("2026-01-01T00:00:00");
        p.set_color_mode(true);
        p.set_page_settings(PageInfo::A4);
        p.set_viewport(Pt::default(), IUS_PER_DECIMIL, 1.0, false);
        p.set_creator("Eeschema-SVG");
        p.set_file_name("x.svg");
        p.start_plot("1");
        p
    }

    #[test]
    fn header_has_mm_page_size_and_viewbox() {
        let mut p = plotter();
        let s = p.end_plot();
        assert!(s.contains("width=\"297.0000mm\" height=\"210.0000mm\" viewBox=\"0.0000 0.0000 297.0000 210.0000\""), "{s}");
        assert!(s.ends_with("</g> \n</svg>\n"));
    }

    #[test]
    fn line_is_a_path_in_mm_with_group_style() {
        let mut p = plotter();
        p.set_color(Color::from_hex("#009600"));
        p.set_current_line_width(1524);
        p.move_to(Pt::new(0, 0));
        p.finish_to(Pt::new(100_000, 50_000));
        let s = p.end_plot();
        assert!(s.contains("stroke:#009600; stroke-width:0.1524;"), "{s}");
        assert!(s.contains("<path d=\"M0.0000 0.0000\nL10.0000 5.0000\n\" />"), "{s}");
    }

    #[test]
    fn xml_escaping() {
        assert_eq!(xml_esc("a<b&\"c\"", false), "a&lt;b&amp;\"c\"");
        assert_eq!(xml_esc("\"", true), "&quot;");
    }
}
