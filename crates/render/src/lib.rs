//! eda-render — deterministic SVG rendering of a `Design`'s schematic
//! section.
//!
//! v1 draws generic box symbols (real KiCad-style glyphs are future work):
//! a rectangle per `SymbolInstance` sized/ported exactly the way
//! `eda_engine::geometry` computed them when the schematic was derived
//! (that module is shared by both crates so the two never drift), pin
//! stubs + names, orthogonal wire polylines with T-junction dots, and
//! italic net-label flags for power/ground.
//!
//! Determinism: all iteration is over the `Design`'s own (already sorted)
//! vectors, and every coordinate is formatted through [`fmt_mm`] — fixed
//! 3-decimal, trailing-zero-trimmed text — so the same `Design` + model
//! always produces byte-identical output.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use eda_engine::geometry;
use eda_layout::{Port, Side};
use eda_model::ir::{Design, Point, SchematicSection, SymbolInstance, Wire};
use eda_model::{CheckResult, ConstraintModel, Part};

/// Pin stub length, in um, drawn from the box edge outward.
const STUB: i64 = 1_270; // 1.27mm, one GRID
/// How far inside the box a pin's name text sits from the box edge, in mm.
const TEXT_MARGIN_MM: f64 = 0.8;
/// SVG-space margin around all content, in mm.
const MARGIN_MM: f64 = 5.0;

/// Renders `design`'s schematic section (looked up against `model` for
/// part/pin metadata) as a complete standalone SVG document.
pub fn render_schematic(design: &Design, model: &ConstraintModel) -> Result<String, Vec<CheckResult>> {
    let sch = design
        .schematic
        .as_ref()
        .ok_or_else(|| vec![CheckResult::fail("render.no_schematic", "design", "Design has no schematic section to render")])?;

    let mut errors = Vec::new();
    let mut boxes = Vec::with_capacity(sch.symbols.len());
    for sym in &sch.symbols {
        match model.part(&sym.id) {
            Some(part) => boxes.push(SymbolBox::build(sym, part)),
            None => errors.push(CheckResult::fail(
                "render.missing_part",
                sym.id.clone(),
                "symbol has no matching Part in the ConstraintModel",
            )),
        }
    }
    if !errors.is_empty() {
        return Err(errors);
    }

    let bounds = compute_bounds(&boxes, sch);
    let (min_x, min_y, max_x, max_y) = bounds;
    let vb_x = min_x - MARGIN_MM;
    let vb_y = min_y - MARGIN_MM;
    let vb_w = (max_x - min_x) + 2.0 * MARGIN_MM;
    let vb_h = (max_y - min_y) + 2.0 * MARGIN_MM;

    let mut svg = String::new();
    let _ = writeln!(
        svg,
        r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="{} {} {} {}">"#,
        fmt_f(vb_x),
        fmt_f(vb_y),
        fmt_f(vb_w),
        fmt_f(vb_h)
    );
    svg.push_str(STYLE);

    let connected = connected_pins(sch);
    for b in &boxes {
        b.render(&mut svg, &connected);
    }
    for w in &sch.wires {
        render_wire(w, &mut svg);
    }
    render_junctions(sch, &mut svg);
    for l in &sch.labels {
        render_net_label(l, &mut svg);
    }

    svg.push_str("</svg>\n");
    Ok(svg)
}

const STYLE: &str = r#"<style>
.box{fill:#ffffff;stroke:#000000;stroke-width:0.3;}
.box-ic{fill:#ffffff;stroke:#000000;stroke-width:0.3;}
.pinline{stroke:#000000;stroke-width:0.15;}
.glyph{fill:none;stroke:#000000;stroke-width:0.25;}
.glyph-fill{fill:#ffffff;stroke:#000000;stroke-width:0.25;}
.wire{stroke:#1a3a6b;stroke-width:0.15;fill:none;}
.junction{fill:#1a3a6b;stroke:none;}
.nc-mark{stroke:#666666;stroke-width:0.2;}
text{font-family:sans-serif;}
.ref{font-size:1.6px;text-anchor:start;font-weight:bold;}
.value{font-size:1.4px;text-anchor:start;}
.pin-left{font-size:1.1px;text-anchor:start;}
.pin-right{font-size:1.1px;text-anchor:end;}
.pin-vert{font-size:1.1px;text-anchor:middle;}
.label text{font-size:1.3px;font-style:italic;}
.gnd-label text{fill:#1a3a6b;text-anchor:middle;}
.pwr-label text{fill:#a6321a;text-anchor:middle;}
.flag-label text{fill:#1a3a6b;}
.label line{stroke:#1a3a6b;stroke-width:0.15;}
.gnd-sym{stroke:#1a3a6b;stroke-width:0.25;fill:none;}
.pwr-sym{stroke:#a6321a;stroke-width:0.25;fill:none;}
</style>
"#;

/// Which glyph a `NetLabel` gets, decided purely from the net name (no
/// model access needed): `GND*`/`AGND*`/`VSS*` (case-insensitive) draw a
/// ground symbol, `V*`/`+*`/`VCC`/`VDD` draw a power symbol, anything else
/// keeps the plain italic flag.
enum LabelKind {
    Ground,
    Power,
    Flag,
}

fn label_kind(net: &str) -> LabelKind {
    let upper = net.to_uppercase();
    if upper.starts_with("GND") || upper.starts_with("AGND") || upper.starts_with("VSS") {
        LabelKind::Ground
    } else if upper.starts_with('V') || upper.starts_with('+') || upper == "VCC" || upper == "VDD" {
        LabelKind::Power
    } else {
        LabelKind::Flag
    }
}

fn render_net_label(l: &eda_model::ir::NetLabel, svg: &mut String) {
    let x = l.at.x as f64 / 1000.0;
    let y = l.at.y as f64 / 1000.0;
    match label_kind(&l.net) {
        LabelKind::Ground => {
            // Uniform short vertical drop from the pin, then three shrinking
            // horizontal bars below it (the standard ground glyph), legible
            // scale (top bar 1.5mm wide) with the net name printed
            // immediately below the bars — compact enough to fit the
            // vertical channel between two vertically-stacked symbols
            // without reaching into the next one's ref text.
            let drop = y + 0.5;
            let d2 = drop + 0.25;
            let d3 = drop + 0.5;
            let ty = d3 + 0.8;
            let _ = writeln!(
                svg,
                r#"<g class="label gnd-label"><line class="gnd-sym" x1="{x}" y1="{y}" x2="{x}" y2="{drop}"/>
<line class="gnd-sym" x1="{bx1}" y1="{drop}" x2="{bx2}" y2="{drop}"/>
<line class="gnd-sym" x1="{mx1}" y1="{d2}" x2="{mx2}" y2="{d2}"/>
<line class="gnd-sym" x1="{tx1}" y1="{d3}" x2="{tx2}" y2="{d3}"/>
<text x="{x}" y="{ty}">{net}</text></g>"#,
                x = fmt_f(x),
                y = fmt_f(y),
                drop = fmt_f(drop),
                bx1 = fmt_f(x - 0.75),
                bx2 = fmt_f(x + 0.75),
                d2 = fmt_f(d2),
                mx1 = fmt_f(x - 0.45),
                mx2 = fmt_f(x + 0.45),
                d3 = fmt_f(d3),
                tx1 = fmt_f(x - 0.15),
                tx2 = fmt_f(x + 0.15),
                ty = fmt_f(ty),
                net = xml_escape(&l.net),
            );
        }
        LabelKind::Power => {
            // Small upward flag: a short stem then an arrowhead, net name
            // printed above it.
            let stem_top = y - 1.6;
            let _ = writeln!(
                svg,
                r#"<g class="label pwr-label"><line class="pwr-sym" x1="{x}" y1="{y}" x2="{x}" y2="{stem_top}"/>
<path class="pwr-sym" d="M {ax1} {ay} L {x} {atip} L {ax2} {ay}"/>
<text x="{x}" y="{ty}">{net}</text></g>"#,
                x = fmt_f(x),
                y = fmt_f(y),
                stem_top = fmt_f(stem_top),
                ax1 = fmt_f(x - 0.5),
                ax2 = fmt_f(x + 0.5),
                ay = fmt_f(stem_top + 0.6),
                atip = fmt_f(stem_top),
                ty = fmt_f(stem_top - 0.4),
                net = xml_escape(&l.net),
            );
        }
        LabelKind::Flag => {
            let _ = writeln!(
                svg,
                r#"<g class="label flag-label"><line x1="{x}" y1="{y1}" x2="{x}" y2="{y2}"/><text x="{tx}" y="{ty}">{net}</text></g>"#,
                x = fmt_mm(l.at.x),
                y1 = fmt_mm(l.at.y),
                y2 = fmt_f(y - 1.5),
                tx = fmt_mm(l.at.x),
                ty = fmt_f(y - 1.8),
                net = xml_escape(&l.net),
            );
        }
    }
}

/// A resolved symbol box: absolute position/size in um plus the pin layout
/// computed the same way the engine computed it (see `eda_engine::geometry`).
struct SymbolBox<'a> {
    sym: &'a SymbolInstance,
    part: &'a Part,
    width: i64,
    height: i64,
    ports: Vec<Port>,
    pin_of_port: Vec<Option<usize>>, // port idx -> pin idx in part.pins
}

impl<'a> SymbolBox<'a> {
    fn build(sym: &'a SymbolInstance, part: &'a Part) -> Self {
        let (width, height) = geometry::node_size(part.pins.len());
        let (ports, pin_port) = geometry::build_ports(part, width, height);
        let mut pin_of_port = vec![None; ports.len()];
        for (pin_idx, port_idx) in pin_port.iter().enumerate() {
            if let Some(pi) = port_idx {
                pin_of_port[*pi] = Some(pin_idx);
            }
        }
        Self { sym, part, width, height, ports, pin_of_port }
    }

    /// Local-space (box top-left = 0,0) corners, before the symbol's own
    /// translate/rotate/mirror transform.
    fn local_corners(&self) -> [(f64, f64); 4] {
        let (w, h) = (self.width as f64, self.height as f64);
        [(0.0, 0.0), (w, 0.0), (w, h), (0.0, h)]
    }

    /// Absolute-um bounding corners (box corners + pin stub tips),
    /// accounting for rotation/mirroring, for viewBox computation.
    fn abs_bounds_points(&self) -> Vec<(f64, f64)> {
        let mut pts: Vec<(f64, f64)> = self.local_corners().to_vec();
        for p in &self.ports {
            let (lx, ly) = local_port_point(p, self.width, self.height);
            let (sx, sy) = stub_tip(p, lx, ly);
            pts.push((sx, sy));
        }
        pts.into_iter().map(|(x, y)| self.to_abs(x, y)).collect()
    }

    fn to_abs(&self, lx: f64, ly: f64) -> (f64, f64) {
        let lx = if self.sym.mirrored { self.width as f64 - lx } else { lx };
        let theta = (self.sym.rot as f64 / 1000.0) * std::f64::consts::PI / 180.0;
        let rx = lx * theta.cos() - ly * theta.sin();
        let ry = lx * theta.sin() + ly * theta.cos();
        (self.sym.at.x as f64 + rx, self.sym.at.y as f64 + ry)
    }

    fn render(&self, svg: &mut String, connected: &Connected) {
        let transform = symbol_transform(self.sym, self.width);
        let _ = writeln!(svg, r#"<g{transform}>"#);

        match passive_kind(self.part) {
            Some(kind) => render_passive_glyph(kind, self.width, self.height, svg),
            None => {
                // Not a recognized 2-pin passive (an IC, or anything else):
                // keep the generic box, but with a thicker border so it
                // reads as a distinct "chip" outline next to the passives'
                // finer glyph strokes.
                let _ = writeln!(
                    svg,
                    r#"<rect class="box-ic" x="0" y="0" width="{w}" height="{h}"/>"#,
                    w = fmt_f(self.width as f64 / 1000.0),
                    h = fmt_f(self.height as f64 / 1000.0),
                );
            }
        }

        // Ref sits above-left of the box; value sits below-left. Both are
        // left-anchored (rather than centered) so they don't collide with a
        // south-side net-label glyph, which is centered under the box.
        let ref_y = -0.3;
        let _ = writeln!(
            svg,
            r#"<text class="ref" x="{x}" y="{y}">{t}</text>"#,
            x = fmt_f(0.0),
            y = fmt_f(ref_y),
            t = xml_escape(&self.sym.id),
        );
        if let Some(val) = symbol_value(self.part) {
            // Value sits at the box's bottom-right, clear of both the east
            // pin row (so it doesn't collide with that pin's number/wire)
            // and the south net-label column (centered under the box, so a
            // right-shifted value clears it horizontally) — and, being
            // right-shifted rather than spanning the box's full width below
            // it, it can't reach up into a vertically-stacked neighbor's ref
            // text either.
            let value_x = self.width as f64 / 1000.0 + 1.6;
            let value_y = self.height as f64 / 1000.0 + 1.8;
            let _ = writeln!(
                svg,
                r#"<text class="value" x="{x}" y="{y}">{t}</text>"#,
                x = fmt_f(value_x),
                y = fmt_f(value_y),
                t = xml_escape(&val),
            );
        }

        for (port_idx, port) in self.ports.iter().enumerate() {
            let (lx, ly) = local_port_point(port, self.width, self.height);
            let (sx, sy) = stub_tip(port, lx, ly);
            let _ = writeln!(
                svg,
                r#"<line class="pinline" x1="{x1}" y1="{y1}" x2="{x2}" y2="{y2}"/>"#,
                x1 = fmt_f(lx / 1000.0),
                y1 = fmt_f(ly / 1000.0),
                x2 = fmt_f(sx / 1000.0),
                y2 = fmt_f(sy / 1000.0),
            );

            if let Some(pin_idx) = self.pin_of_port[port_idx] {
                let pin = &self.part.pins[pin_idx];
                let label = pin.name.clone().unwrap_or_else(|| pin.number.clone());
                let (tx, ty, class) = pin_text_pos(port, lx, ly, self.width as f64 / 1000.0);
                let _ = writeln!(
                    svg,
                    r#"<text class="{class}" x="{x}" y="{y}">{t}</text>"#,
                    x = fmt_f(tx),
                    y = fmt_f(ty),
                    t = xml_escape(&label),
                );

                // Unconnected pin stub (no wire carries this pin ref, no
                // net label sits at this port's on-box point): mark the
                // dead end with a small "x" so it reads as deliberately
                // floating, not a missed connection.
                let pin_ref = format!("{}.{}", self.sym.id, pin.number);
                let (attach_x, attach_y) = self.to_abs(lx, ly); // um-scale, on-box attach point
                let attach_pt = Point { x: attach_x.round() as i64, y: attach_y.round() as i64 };
                let is_connected = connected.wired_pins.contains(&pin_ref) || connected.label_points.contains(&attach_pt);
                if !is_connected {
                    let (lx_m, ly_m) = (sx / 1000.0, sy / 1000.0);
                    let r = 0.5;
                    let _ = writeln!(
                        svg,
                        r#"<g class="nc-mark"><line x1="{x0}" y1="{y0}" x2="{x1}" y2="{y1}"/><line x1="{x2}" y1="{y0}" x2="{x3}" y2="{y1}"/></g>"#,
                        x0 = fmt_f(lx_m - r),
                        y0 = fmt_f(ly_m - r),
                        x1 = fmt_f(lx_m + r),
                        y1 = fmt_f(ly_m + r),
                        x2 = fmt_f(lx_m + r),
                        x3 = fmt_f(lx_m - r),
                    );
                }
            }
        }
        svg.push_str("</g>\n");
    }
}

/// Which 2-pin passive glyph a part gets, purely from `Part`
/// package/value/reference heuristics (no footprint DB needed): the
/// reference-designator prefix (R/C/L/D — the universal EDA convention) if
/// present, else the same letter checked against `package`/`value`. Parts
/// with any pin count other than exactly 2, or that match none of the four
/// letters, keep the generic IC rectangle.
#[derive(Clone, Copy, PartialEq, Eq)]
enum PassiveKind {
    Resistor,
    Capacitor,
    Inductor,
    Diode,
}

fn passive_kind(part: &Part) -> Option<PassiveKind> {
    if part.pins.len() != 2 {
        return None;
    }
    let candidates = [Some(part.reference.as_str()), part.package.as_deref(), part.value.as_deref()];
    for c in candidates.into_iter().flatten() {
        let first = c.trim_start_matches(['+', '-']).chars().next()?;
        match first.to_ascii_uppercase() {
            'R' => return Some(PassiveKind::Resistor),
            'C' => return Some(PassiveKind::Capacitor),
            'L' => return Some(PassiveKind::Inductor),
            'D' => return Some(PassiveKind::Diode),
            _ => continue,
        }
    }
    None
}

/// Draws a 2-pin passive glyph centered on the box's midline, in local
/// (box-space) coordinates, entirely inside the same `width`x`height`
/// bounding box `geometry::node_size` allocated — the box itself is never
/// drawn, and ports/stubs (drawn separately by the caller) are untouched,
/// so gates' box-overlap math stays valid against the same footprint.
/// Assumes a horizontal (Left/Right-port) 2-pin part, which is what
/// `geometry::build_ports` always produces for a 2-pin `Passive`/`Signal`
/// part (one west pin, one east pin) — the only shape this heuristic ever
/// matches in practice.
fn render_passive_glyph(kind: PassiveKind, width: i64, height: i64, svg: &mut String) {
    let w = width as f64 / 1000.0;
    let h = height as f64 / 1000.0;
    let cy = h / 2.0;
    let lead = w * 0.32; // lead runs from the box edge to this x
    let far = w - lead;
    match kind {
        PassiveKind::Capacitor => {
            let gap = w * 0.06;
            let x1 = w / 2.0 - gap;
            let x2 = w / 2.0 + gap;
            let plate_half = h * 0.28;
            let _ = writeln!(
                svg,
                r#"<g class="glyph"><line x1="0" y1="{cy}" x2="{x1}" y2="{cy}"/><line x1="{x1}" y1="{y0}" x2="{x1}" y2="{y1}"/><line x1="{x2}" y1="{y0}" x2="{x2}" y2="{y1}"/><line x1="{x2}" y1="{cy}" x2="{w}" y2="{cy}"/></g>"#,
                cy = fmt_f(cy),
                x1 = fmt_f(x1),
                x2 = fmt_f(x2),
                y0 = fmt_f(cy - plate_half),
                y1 = fmt_f(cy + plate_half),
                w = fmt_f(w),
            );
        }
        PassiveKind::Resistor => {
            let body_half = h * 0.18;
            let _ = writeln!(
                svg,
                r#"<g><line class="glyph" x1="0" y1="{cy}" x2="{lead}" y2="{cy}"/><rect class="glyph-fill" x="{lead}" y="{y0}" width="{bw}" height="{bh}"/><line class="glyph" x1="{far}" y1="{cy}" x2="{w}" y2="{cy}"/></g>"#,
                cy = fmt_f(cy),
                lead = fmt_f(lead),
                y0 = fmt_f(cy - body_half),
                bw = fmt_f(far - lead),
                bh = fmt_f(body_half * 2.0),
                far = fmt_f(far),
                w = fmt_f(w),
            );
        }
        PassiveKind::Inductor => {
            let bump_r = (far - lead) / 6.0;
            let mut path = format!("M {} {}", fmt_f(lead), fmt_f(cy));
            for i in 0..3 {
                let bump_end = lead + bump_r * (2.0 * i as f64 + 2.0);
                let _ = write!(path, " A {r} {r} 0 0 1 {x} {y}", r = fmt_f(bump_r), x = fmt_f(bump_end), y = fmt_f(cy));
            }
            let _ = writeln!(
                svg,
                r#"<g class="glyph"><line x1="0" y1="{cy}" x2="{lead}" y2="{cy}"/><path d="{path}"/><line x1="{far}" y1="{cy}" x2="{w}" y2="{cy}"/></g>"#,
                cy = fmt_f(cy),
                lead = fmt_f(lead),
                far = fmt_f(far),
                w = fmt_f(w),
            );
        }
        PassiveKind::Diode => {
            let tri_half = h * 0.22;
            let bar_x = far;
            let _ = writeln!(
                svg,
                r#"<g><line class="glyph" x1="0" y1="{cy}" x2="{lead}" y2="{cy}"/><path class="glyph-fill" d="M {lead} {y0} L {lead} {y1} L {bar_x} {cy} Z"/><line class="glyph" x1="{bar_x}" y1="{y0}" x2="{bar_x}" y2="{y1}"/><line class="glyph" x1="{far}" y1="{cy}" x2="{w}" y2="{cy}"/></g>"#,
                cy = fmt_f(cy),
                lead = fmt_f(lead),
                y0 = fmt_f(cy - tri_half),
                y1 = fmt_f(cy + tri_half),
                bar_x = fmt_f(bar_x),
                far = fmt_f(far),
                w = fmt_f(w),
            );
        }
    }
}

fn symbol_value(part: &Part) -> Option<String> {
    match (&part.value, &part.mpn) {
        (Some(v), _) => Some(v.clone()),
        (None, Some(m)) => Some(m.clone()),
        (None, None) => None,
    }
}

/// Local (box-space, um) point for a port.
fn local_port_point(port: &Port, width: i64, height: i64) -> (f64, f64) {
    match port.side {
        Side::Top => (port.offset as f64, 0.0),
        Side::Bottom => (port.offset as f64, height as f64),
        Side::Left => (0.0, port.offset as f64),
        Side::Right => (width as f64, port.offset as f64),
    }
}

/// Local (box-space, um) tip of the pin stub, extending outward from the box.
fn stub_tip(port: &Port, lx: f64, ly: f64) -> (f64, f64) {
    match port.side {
        Side::Top => (lx, ly - STUB as f64),
        Side::Bottom => (lx, ly + STUB as f64),
        Side::Left => (lx - STUB as f64, ly),
        Side::Right => (lx + STUB as f64, ly),
    }
}

/// Local (mm) pin-name text anchor position + CSS class, placed just inside
/// the box from the port point.
fn pin_text_pos(port: &Port, lx_um: f64, ly_um: f64, width_mm: f64) -> (f64, f64, &'static str) {
    let lx = lx_um / 1000.0;
    let ly = ly_um / 1000.0;
    match port.side {
        Side::Top => (lx, ly + TEXT_MARGIN_MM + 1.0, "pin-vert"),
        Side::Bottom => (lx, ly - TEXT_MARGIN_MM, "pin-vert"),
        Side::Left => (lx + TEXT_MARGIN_MM, ly + 0.4, "pin-left"),
        Side::Right => {
            let _ = width_mm;
            (lx - TEXT_MARGIN_MM, ly + 0.4, "pin-right")
        }
    }
}

/// SVG `transform` attribute for a symbol instance (empty when the symbol
/// needs no transform, i.e. rot=0 and not mirrored — the common case).
fn symbol_transform(sym: &SymbolInstance, width: i64) -> String {
    let x = fmt_f(sym.at.x as f64 / 1000.0);
    let y = fmt_f(sym.at.y as f64 / 1000.0);
    if sym.rot == 0 && !sym.mirrored {
        return format!(r#" transform="translate({x},{y})""#);
    }
    let deg = fmt_f(sym.rot as f64 / 1000.0);
    let w = fmt_f(width as f64 / 1000.0);
    if sym.mirrored {
        format!(r#" transform="translate({x},{y}) rotate({deg}) translate({w},0) scale(-1,1)""#)
    } else {
        format!(r#" transform="translate({x},{y}) rotate({deg})""#)
    }
}

fn render_wire(w: &Wire, svg: &mut String) {
    let mut points = String::new();
    for (i, p) in w.pts.iter().enumerate() {
        if i > 0 {
            points.push(' ');
        }
        let _ = write!(points, "{},{}", fmt_mm(p.x), fmt_mm(p.y));
    }
    let _ = writeln!(svg, r#"<polyline class="wire" points="{points}"/>"#);
}

/// T-junction dots: a point where >=3 same-net wire segment endpoints meet.
fn render_junctions(sch: &SchematicSection, svg: &mut String) {
    // Count, per net, how many wire segments touch each point (a segment
    // contributes to both of its endpoints). A pass-through vertex inside a
    // single polyline touches exactly 2 segments; a real junction — either
    // several wires sharing an endpoint, or a >2-way star hub — touches >=3.
    let mut touches: BTreeMap<(&str, Point), usize> = BTreeMap::new();
    for w in &sch.wires {
        if w.pts.len() < 2 {
            continue;
        }
        for pair in w.pts.windows(2) {
            for pt in [pair[0], pair[1]] {
                *touches.entry((w.net.as_str(), pt)).or_insert(0) += 1;
            }
        }
    }
    let keys: Vec<_> = touches.iter().filter(|(_, &n)| n >= 3).map(|(k, _)| *k).collect();
    for (_, pt) in keys {
        let _ = writeln!(
            svg,
            r#"<circle class="junction" cx="{x}" cy="{y}" r="0.25"/>"#,
            x = fmt_mm(pt.x),
            y = fmt_mm(pt.y),
        );
    }
}

/// What counts as "connected", for the NC dead-end marker: a set of
/// `"REF.PIN"` refs carried by some wire, plus a set of net-label anchor
/// points (`NetLabel::at`, which the engine places at the port's on-box
/// attachment point — *not* the stub tip, so this must be compared against
/// the same on-box point, not the stub tip, when matching a label to a pin).
struct Connected {
    wired_pins: std::collections::HashSet<String>,
    label_points: std::collections::BTreeSet<Point>,
}

fn connected_pins(sch: &SchematicSection) -> Connected {
    let mut wired_pins = std::collections::HashSet::new();
    for w in &sch.wires {
        for p in &w.pins {
            wired_pins.insert(p.clone());
        }
    }
    let mut label_points = std::collections::BTreeSet::new();
    for l in &sch.labels {
        label_points.insert(l.at);
    }
    Connected { wired_pins, label_points }
}

fn compute_bounds(boxes: &[SymbolBox], sch: &SchematicSection) -> (f64, f64, f64, f64) {
    let mut min_x = f64::INFINITY;
    let mut min_y = f64::INFINITY;
    let mut max_x = f64::NEG_INFINITY;
    let mut max_y = f64::NEG_INFINITY;
    let mut feed = |x_um: f64, y_um: f64| {
        let (x, y) = (x_um / 1000.0, y_um / 1000.0);
        min_x = min_x.min(x);
        min_y = min_y.min(y);
        max_x = max_x.max(x);
        max_y = max_y.max(y);
    };
    for b in boxes {
        for (x, y) in b.abs_bounds_points() {
            feed(x, y);
        }
    }
    for w in &sch.wires {
        for p in &w.pts {
            feed(p.x as f64, p.y as f64);
        }
    }
    for l in &sch.labels {
        feed(l.at.x as f64, l.at.y as f64);
    }
    if !min_x.is_finite() {
        // Empty schematic: fall back to a trivial origin box.
        return (0.0, 0.0, 0.0, 0.0);
    }
    (min_x, min_y, max_x, max_y)
}

/// um -> mm, 3-decimal max, no trailing float noise, deterministic.
fn fmt_mm(um: i64) -> String {
    fmt_f(um as f64 / 1000.0)
}

fn fmt_f(v: f64) -> String {
    let s = format!("{v:.3}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    match s {
        "" | "-0" => "0".to_string(),
        _ => s.to_string(),
    }
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

#[cfg(test)]
mod tests {
    use super::*;
    use eda_engine::{derive_schematic, EngineOptions};
    use eda_model::{Net, Pin, PinKind};

    fn pin(number: &str, name: &str, kind: PinKind) -> Pin {
        Pin { number: number.into(), name: Some(name.into()), kind }
    }

    fn part(reference: &str, value: Option<&str>, pins: Vec<Pin>) -> Part {
        Part { reference: reference.into(), mpn: None, value: value.map(String::from), package: None, footprint: None, pins }
    }

    fn net(name: &str, pins: &[&str]) -> Net {
        Net { name: name.into(), pins: pins.iter().map(|s| s.to_string()).collect() }
    }

    /// LDO + input/output caps, matching eda-engine's own fixture pattern.
    fn ldo_model() -> ConstraintModel {
        let u1 = part(
            "U1",
            Some("LDO"),
            vec![
                pin("1", "VIN", PinKind::Power),
                pin("2", "GND", PinKind::Ground),
                pin("3", "EN", PinKind::Signal),
                pin("4", "VOUT", PinKind::Power),
                pin("5", "NC", PinKind::Nc),
            ],
        );
        let cin = part("CIN", Some("1uF"), vec![pin("1", "1", PinKind::Passive), pin("2", "2", PinKind::Ground)]);
        let cout = part("COUT", Some("10uF"), vec![pin("1", "1", PinKind::Passive), pin("2", "2", PinKind::Ground)]);

        ConstraintModel {
            parts: vec![u1, cin, cout],
            nets: vec![
                net("VIN", &["U1.1", "CIN.1", "U1.3"]),
                net("VOUT", &["U1.4", "COUT.1"]),
                net("GND", &["U1.2", "CIN.2", "COUT.2"]),
            ],
            ..Default::default()
        }
    }

    fn ldo_design() -> Design {
        let model = ldo_model();
        derive_schematic(&model, &EngineOptions::new(1, "test-hash")).unwrap()
    }

    #[test]
    fn cli_smoke_writes_sample_svg() {
        let model = ldo_model();
        let design = ldo_design();
        let svg = render_schematic(&design, &model).unwrap();

        let doc = roxmltree::Document::parse(&svg).expect("rendered SVG must parse as XML");
        assert_eq!(doc.root_element().tag_name().name(), "svg");

        let out_dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../../target/test-output");
        std::fs::create_dir_all(out_dir).unwrap();
        std::fs::write(format!("{out_dir}/ldo.svg"), &svg).unwrap();
    }

    #[test]
    fn contains_rect_and_ref_text_per_symbol() {
        let model = ldo_model();
        let design = ldo_design();
        let svg = render_schematic(&design, &model).unwrap();
        // U1 (5 pins, an IC) keeps the thicker-border generic rectangle;
        // CIN/COUT (2-pin, ref starting with 'C') get the capacitor glyph
        // instead of any box rect at all.
        assert_eq!(svg.matches("<rect class=\"box-ic\"").count(), 1, "one IC rect (U1)");
        assert_eq!(svg.matches("class=\"glyph\"").count() + svg.matches("class=\"glyph-fill\"").count(), 2, "one capacitor glyph each for CIN/COUT");
        assert!(svg.contains(">U1<"));
        assert!(svg.contains(">CIN<"));
        assert!(svg.contains(">COUT<"));
    }

    #[test]
    fn wire_polyline_matches_design_points() {
        let model = ldo_model();
        let design = ldo_design();
        let svg = render_schematic(&design, &model).unwrap();
        let sch = design.schematic.unwrap();
        let vin_wire = sch.wires.iter().find(|w| w.net == "VIN").unwrap();
        let expected: Vec<String> = vin_wire.pts.iter().map(|p| format!("{},{}", fmt_mm(p.x), fmt_mm(p.y))).collect();
        let expected_points = expected.join(" ");
        assert!(svg.contains(&expected_points), "expected wire points {expected_points} in SVG:\n{svg}");
    }

    #[test]
    fn byte_determinism_across_two_renders() {
        let model = ldo_model();
        let design = ldo_design();
        let svg1 = render_schematic(&design, &model).unwrap();
        let svg2 = render_schematic(&design, &model).unwrap();
        assert_eq!(svg1, svg2);
    }

    #[test]
    fn viewbox_contains_all_geometry() {
        let model = ldo_model();
        let design = ldo_design();
        let svg = render_schematic(&design, &model).unwrap();
        let sch = design.schematic.as_ref().unwrap();

        let vb_line = svg.lines().next().unwrap();
        let vb_str = vb_line.split("viewBox=\"").nth(1).unwrap().split('"').next().unwrap();
        let nums: Vec<f64> = vb_str.split_whitespace().map(|s| s.parse().unwrap()).collect();
        let (vx, vy, vw, vh) = (nums[0], nums[1], nums[2], nums[3]);

        for sym in &sch.symbols {
            let x = sym.at.x as f64 / 1000.0;
            let y = sym.at.y as f64 / 1000.0;
            assert!(x >= vx && x <= vx + vw, "symbol {} x out of viewBox", sym.id);
            assert!(y >= vy && y <= vy + vh, "symbol {} y out of viewBox", sym.id);
        }
        for w in &sch.wires {
            for p in &w.pts {
                let x = p.x as f64 / 1000.0;
                let y = p.y as f64 / 1000.0;
                assert!(x >= vx && x <= vx + vw);
                assert!(y >= vy && y <= vy + vh);
            }
        }
    }

    #[test]
    fn net_labels_rendered() {
        // Force a dense GND net (>4 pins, all ground) so the engine emits
        // labels instead of wires.
        let mut parts = vec![];
        let mut net_pins = vec![];
        for i in 0..5 {
            let r = format!("R{i}");
            parts.push(part(&r, None, vec![pin("1", "GND", PinKind::Ground)]));
            net_pins.push(format!("{r}.1"));
        }
        let model = ConstraintModel {
            parts,
            nets: vec![net("GND", &net_pins.iter().map(|s| s.as_str()).collect::<Vec<_>>())],
            ..Default::default()
        };
        let design = derive_schematic(&model, &EngineOptions::new(1, "h")).unwrap();
        let svg = render_schematic(&design, &model).unwrap();
        // Dense GND net -> ground-glyph labels, not the plain flag.
        assert!(svg.contains("class=\"label gnd-label\""));
        assert!(svg.matches("class=\"label gnd-label\"").count() >= 5);
    }

    #[test]
    fn malformed_design_no_schematic_returns_check_result_not_panic() {
        let design = Design {
            schema: 1,
            provenance: eda_model::ir::Provenance {
                engine_version: "0".into(),
                intent_hash: "x".into(),
                seed: 0,
                stage_hashes: vec![],
            },
            schematic: None,
            placement: None,
            routing: None,
        };
        let model = ConstraintModel::default();
        let err = render_schematic(&design, &model).unwrap_err();
        assert!(err.iter().any(|e| e.check == "render.no_schematic"));
    }

    #[test]
    fn symbol_missing_from_model_is_a_check_result_not_panic() {
        let model = ConstraintModel::default(); // no parts at all
        let design = ldo_design(); // has symbols U1/CIN/COUT
        let err = render_schematic(&design, &model).unwrap_err();
        assert!(err.iter().any(|e| e.check == "render.missing_part"));
    }
}
