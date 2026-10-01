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

use std::fmt::Write as _;

use eda_engine::geometry;
use eda_layout::{Port, Side};
use eda_model::ir::{Design, Point, SchematicSection, SymbolInstance, Wire};
use eda_model::{CheckResult, ConstraintModel, Part};

/// Pin stub length, in um, drawn from the box edge outward.
const STUB: i64 = 1_270; // 1.27mm, one GRID
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
            Some(part) => boxes.push(SymbolBox::build(sym, part, model)),
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

    let connected = connected_pins(sch);
    // Ref/value text boxes for every symbol, computed the same way
    // `render`'s own ref/value placement does — used below as fixed
    // obstacles that net-label glyphs must jog clear of (see
    // `geometry::resolve_net_label`); `eda-gates`' `collect_text_boxes`
    // mirrors this exact computation so the two crates always agree. Each
    // symbol's own ref/value placement also steers clear of every earlier
    // symbol's boxes (`ref_value_boxes` pushes onto `label_obstacles` as it
    // goes), so this single pass gives ref/value-vs-ref/value clearance too.
    // Every symbol's box rectangle, in mm — a hard placement obstacle for
    // net labels (`schematic_label_in_symbol`): a tag must never overlap
    // ANY symbol's body, not just steer clear of its own pin's box.
    let symbol_boxes: Vec<geometry::TextBox> =
        boxes.iter().map(|b| geometry::symbol_box_mm(b.sym.at.x as f64 / 1000.0, b.sym.at.y as f64 / 1000.0, b.width, b.height)).collect();

    let mut label_obstacles: Vec<geometry::TextBox> = Vec::new();
    for b in &boxes {
        b.ref_value_boxes(&sch.wires, &mut label_obstacles);
    }
    // Pin-name text is a fixed obstacle too (never repositioned, unlike
    // ref/value/labels), so it's simply appended rather than threaded
    // through `ref_value_boxes`'s own obstacle-avoidance search.
    for b in &boxes {
        label_obstacles.extend(b.pin_text_boxes());
    }

    // The sheet's own bounding box (and, from it, the SVG viewBox) must be
    // the union of the FULL drawn extent of every element this function is
    // about to draw — symbol boxes + pin stub tips, wire points, ref/value/
    // pin text, and each net label's actual ink (its glyph/tag, not just
    // its pin anchor) — or a label that legitimately grows outward past the
    // sheet's other content (e.g. a Right-side pin's tag, which must point
    // away from its box) gets silently clipped by the viewBox. Net-label
    // ink can only be known by resolving each label's jog/obstacle search,
    // so this runs that search once here, purely to measure — against a
    // *clone* of `label_obstacles` — before the real drawing pass below
    // runs the identical, deterministic search again from the pristine
    // list to actually place and draw each label. `eda-gates`'
    // `schematic_content_in_bounds` re-derives this same box from the same
    // shared `geometry` functions, so the two can never disagree about
    // what "in bounds" means.
    let label_extents: Vec<geometry::TextBox> = {
        let mut probe_obstacles = label_obstacles.clone();
        sch.labels
            .iter()
            .map(|l| {
                let (resolved, ink) = geometry::resolve_net_label(l.at.x, l.at.y, &l.net, 1.3, &mut probe_obstacles, &symbol_boxes, &sch.wires);
                geometry::net_label_extent(&resolved, ink)
            })
            .collect()
    };
    let mut extra_bounds: Vec<geometry::TextBox> = label_obstacles.clone();
    extra_bounds.extend(symbol_boxes.iter().copied());
    extra_bounds.extend(label_extents);
    extra_bounds.extend(sch.power_symbols.iter().map(power_symbol_extent));

    let (min_x, min_y, max_x, max_y) = compute_bounds(&boxes, sch, &extra_bounds);
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

    // `render()` recomputes the same positions independently (it needs to
    // draw them, not just measure them); start its own obstacle
    // accumulation fresh so it reproduces `label_obstacles` step for step.
    let mut render_obstacles: Vec<geometry::TextBox> = Vec::new();
    for b in &boxes {
        b.render(&mut svg, &connected, &sch.wires, &mut render_obstacles);
    }
    for w in &sch.wires {
        render_wire(w, &mut svg);
    }
    render_junctions(sch, &mut svg);
    for l in &sch.labels {
        render_net_label(l, &mut svg, &mut label_obstacles, &symbol_boxes, &sch.wires);
    }
    for p in &sch.power_symbols {
        render_power_symbol(p, &mut svg);
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
.flag-label text{fill:#1a3a6b;text-anchor:middle;}
.label line{stroke:#1a3a6b;stroke-width:0.15;}
.flag-label polygon.tag{fill:none;stroke:#1a3a6b;stroke-width:0.08;}
.gnd-sym{stroke:#1a3a6b;stroke-width:0.25;fill:none;}
.pwr-sym{stroke:#a6321a;stroke-width:0.25;fill:none;}
</style>
"#;

/// Resolves `l`'s on-sheet position (via the shared `geometry::resolve_net_label`,
/// so `eda-gates` can never disagree on where this label lands), draws it
/// into `svg`, and returns the label's full drawn-ink extent (via
/// `geometry::net_label_extent`) so the caller can grow the sheet's own
/// bounding box to include it — see `render_schematic`'s bounds pass.
fn render_net_label(l: &eda_model::ir::NetLabel, svg: &mut String, obstacles: &mut Vec<geometry::TextBox>, boxes: &[geometry::TextBox], wires: &[Wire]) -> geometry::TextBox {
    let font = 1.3;
    let (resolved, ink) = geometry::resolve_net_label(l.at.x, l.at.y, &l.net, font, obstacles, boxes, wires);
    match resolved {
        geometry::ResolvedNetLabel::Ground { x, y, drop, jog_x, ty } => {
            // Uniform short vertical drop from the pin, then three shrinking
            // horizontal bars below it (the standard ground glyph), legible
            // scale (top bar 1.5mm wide) with the net name printed
            // immediately below the bars — compact enough to fit the
            // vertical channel between two vertically-stacked symbols
            // without reaching into the next one's ref text. The whole
            // glyph+text jogs sideways (see `geometry::resolve_net_label`)
            // when it would otherwise land on a nearby ref/value/label box;
            // the drop segment from the pin stays put so the connection is
            // still visually obvious, with a horizontal dogleg to the jog.
            let [(bx1, _), (bx2, _), (mx1, d2), (mx2, _), (tx1, d3), (tx2, _)] = geometry::ground_glyph_points(jog_x, drop);
            let dogleg = if (jog_x - x).abs() > 1e-6 {
                format!(r#"<line class="gnd-sym" x1="{x}" y1="{drop}" x2="{jog_x}" y2="{drop}"/>"#, x = fmt_f(x), drop = fmt_f(drop), jog_x = fmt_f(jog_x))
            } else {
                String::new()
            };
            let _ = writeln!(
                svg,
                r#"<g class="label gnd-label"><line class="gnd-sym" x1="{x}" y1="{y}" x2="{x}" y2="{drop}"/>{dogleg}
<line class="gnd-sym" x1="{bx1}" y1="{drop}" x2="{bx2}" y2="{drop}"/>
<line class="gnd-sym" x1="{mx1}" y1="{d2}" x2="{mx2}" y2="{d2}"/>
<line class="gnd-sym" x1="{tx1}" y1="{d3}" x2="{tx2}" y2="{d3}"/>
<text x="{jx}" y="{ty}">{net}</text></g>"#,
                x = fmt_f(x),
                y = fmt_f(y),
                drop = fmt_f(drop),
                bx1 = fmt_f(bx1),
                bx2 = fmt_f(bx2),
                d2 = fmt_f(d2),
                mx1 = fmt_f(mx1),
                mx2 = fmt_f(mx2),
                d3 = fmt_f(d3),
                tx1 = fmt_f(tx1),
                tx2 = fmt_f(tx2),
                jx = fmt_f(jog_x),
                ty = fmt_f(ty),
                net = xml_escape(&l.net),
            );
        }
        geometry::ResolvedNetLabel::Power { x, y, stem_top, jog_x, ty } => {
            // Small upward flag: a short stem then an arrowhead, net name
            // printed above it. Same jog-clear-of-obstacles scheme as
            // Ground, with a horizontal dogleg at the stem top.
            let [(ax1, ay), (ax2, _)] = geometry::power_glyph_points(jog_x, stem_top);
            let dogleg = if (jog_x - x).abs() > 1e-6 {
                format!(
                    r#"<line class="pwr-sym" x1="{x}" y1="{stem_top}" x2="{jog_x}" y2="{stem_top}"/>"#,
                    x = fmt_f(x),
                    stem_top = fmt_f(stem_top),
                    jog_x = fmt_f(jog_x)
                )
            } else {
                String::new()
            };
            let _ = writeln!(
                svg,
                r#"<g class="label pwr-label"><line class="pwr-sym" x1="{x}" y1="{y}" x2="{x}" y2="{stem_top}"/>{dogleg}
<path class="pwr-sym" d="M {ax1} {ay} L {jx} {atip} L {ax2} {ay}"/>
<text x="{jx}" y="{ty}">{net}</text></g>"#,
                x = fmt_f(x),
                y = fmt_f(y),
                stem_top = fmt_f(stem_top),
                ax1 = fmt_f(ax1),
                ax2 = fmt_f(ax2),
                ay = fmt_f(ay),
                jx = fmt_f(jog_x),
                atip = fmt_f(stem_top),
                ty = fmt_f(ty),
                net = xml_escape(&l.net),
            );
        }
        geometry::ResolvedNetLabel::Flag { bend_x, bend_y, dog_x, dog_y, jog_x, ty, side, .. } => {
            // A signal net label is a *tag*: the same italic-name visual
            // class as the power/ground flags, but outlined by a
            // pentagon-with-a-point so a reader can tell "this name
            // continues elsewhere on the sheet" from "this pin goes to a
            // rail" at a glance. The outline hugs the text box the gates
            // already reserve, so it never introduces clearance the gates
            // do not know about.
            let anchor = match side {
                Side::Top | Side::Bottom => geometry::HAnchor::Middle,
                Side::Left => geometry::HAnchor::End,
                Side::Right => geometry::HAnchor::Start,
            };
            let bbox = geometry::text_bbox(jog_x, ty, &l.net, font, anchor);
            let (tx0, ty0, tx1, ty1) = (bbox.x0 - 0.25, bbox.y0 - 0.15, bbox.x1 + 0.25, bbox.y1 + 0.15);
            let nose = 0.45;
            // The pentagon's point always faces the side the leader departs
            // toward: right (default) for a Top/Bottom/Left-side tag, whose
            // pin sits at or to the right of the tag; mirrored — pointing
            // left — for a Right-side tag, whose pin sits to its left.
            let tag = if side == Side::Right {
                format!(
                    r#"<polygon class="tag" points="{a2},{py} {px2},{py} {b},{my} {px2},{qy} {a2},{qy}"/>"#,
                    a2 = fmt_f(tx1 + nose),
                    px2 = fmt_f(tx0),
                    py = fmt_f(ty0),
                    qy = fmt_f(ty1),
                    b = fmt_f(tx0 - nose),
                    my = fmt_f((ty0 + ty1) / 2.0),
                )
            } else {
                format!(
                    r#"<polygon class="tag" points="{px},{py} {a},{py} {b},{my} {a},{qy} {px},{qy}"/>"#,
                    px = fmt_f(tx0 - nose),
                    py = fmt_f(ty0),
                    qy = fmt_f(ty1),
                    a = fmt_f(tx1),
                    b = fmt_f(tx1 + nose),
                    my = fmt_f((ty0 + ty1) / 2.0),
                )
            };
            // Leader: pin -> bend point (straight along the pin's outward
            // normal — see `geometry::resolve_net_label`), then, if the tag
            // had to slide sideways to dodge a neighbour, a dogleg along the
            // side to the tag's actual position.
            let dogleg = if (dog_x - bend_x).abs() > 1e-6 || (dog_y - bend_y).abs() > 1e-6 {
                format!(r#"<line x1="{x}" y1="{y}" x2="{dx}" y2="{dy}"/>"#, x = fmt_f(bend_x), y = fmt_f(bend_y), dx = fmt_f(dog_x), dy = fmt_f(dog_y))
            } else {
                String::new()
            };
            // The CSS class default (`text-anchor:middle`) only matches a
            // Top/Bottom-side tag; a Left/Right-side tag's text was placed
            // to grow away from the box, and needs the matching SVG anchor
            // or the renderer's own default centers it back on `jog_x` —
            // reaching half the text's width back over the very box this
            // whole gate/fix exists to stay clear of.
            let anchor_attr = match side {
                Side::Top | Side::Bottom => "",
                Side::Left => r#" style="text-anchor:end""#,
                Side::Right => r#" style="text-anchor:start""#,
            };
            let _ = writeln!(
                svg,
                r#"<g class="label flag-label"><line x1="{x}" y1="{y1}" x2="{bx}" y2="{by}"/>{dogleg}{tag}<text x="{tx}" y="{ty}"{anchor_attr}>{net}</text></g>"#,
                x = fmt_mm(l.at.x),
                y1 = fmt_mm(l.at.y),
                bx = fmt_f(bend_x),
                by = fmt_f(bend_y),
                tx = fmt_f(jog_x),
                ty = fmt_f(ty),
                net = xml_escape(&l.net),
            );
        }
    }
    geometry::net_label_extent(&resolved, ink)
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
    resolved: Option<eda_model::symbol::LibSymbol>,
}

impl<'a> SymbolBox<'a> {
    fn build(sym: &'a SymbolInstance, part: &'a Part, model: &ConstraintModel) -> Self {
        let resolved = model.real_symbol_of(&sym.lib_id, part);
        let (width, height) = geometry::node_size(part, resolved.as_ref());
        let (ports, pin_port) = geometry::build_ports(part, width, height, resolved.as_ref());
        let mut pin_of_port = vec![None; ports.len()];
        for (pin_idx, port_idx) in pin_port.iter().enumerate() {
            if let Some(pi) = port_idx {
                pin_of_port[*pi] = Some(pin_idx);
            }
        }
        Self { sym, part, width, height, ports, pin_of_port, resolved }
    }

    /// Local-space (box top-left = 0,0) corners, before the symbol's own
    /// translate/rotate/mirror transform.
    /// Box corners in local millimetres.
    ///
    /// `width`/`height` are µm, like every other stored dimension; the
    /// drawing divides them at emit time. Bounds have to divide too, or
    /// the sheet measures a thousand times bigger than it draws.
    fn local_corners(&self) -> [(f64, f64); 4] {
        let (w, h) = (self.width as f64 / 1000.0, self.height as f64 / 1000.0);
        [(0.0, 0.0), (w, 0.0), (w, h), (0.0, h)]
    }

    /// Absolute-**mm** bounding corners (box corners + pin stub tips),
    /// accounting for rotation/mirroring, for viewBox computation.
    ///
    /// Was µm, while `compute_bounds` fed it alongside wire points and
    /// text boxes that are both mm. The µm numbers dominated, so the
    /// viewBox spanned 133359 units for 133mm of content and the sheet
    /// rendered as a speck in the corner of an empty canvas.
    fn abs_bounds_points(&self) -> Vec<(f64, f64)> {
        let mut pts: Vec<(f64, f64)> = self.local_corners().to_vec();
        for p in &self.ports {
            let (lx, ly) = local_port_point(p, self.width, self.height);
            let (sx, sy) = stub_tip(p, lx, ly);
            pts.push((sx / 1000.0, sy / 1000.0));
        }
        pts.into_iter().map(|(x, y)| self.to_abs(x, y)).collect()
    }

    fn to_abs(&self, lx: f64, ly: f64) -> (f64, f64) {
        let lx = if self.sym.mirrored { self.width as f64 - lx } else { lx };
        let theta = (self.sym.rot as f64 / 1000.0) * std::f64::consts::PI / 180.0;
        let rx = lx * theta.cos() - ly * theta.sin();
        let ry = lx * theta.sin() + ly * theta.cos();
        // Millimetres, like every other coordinate here. Adding the
        // symbol's µm origin to a mm offset inflated the sheet bounds
        // ~1000x, so the viewBox spanned 133359 units for 133mm of
        // content and the whole drawing rendered as a speck in the
        // corner. Every other use of `sym.at` on this type divides.
        (self.sym.at.x as f64 / 1000.0 + rx, self.sym.at.y as f64 / 1000.0 + ry)
    }

    /// Ref (and, if present, value) text bounding boxes in absolute mm,
    /// computed with the exact same offsets `render` uses to place them —
    /// kept as a separate method so net-label placement can treat them as
    /// fixed obstacles (see `resolve_label_pos`) without duplicating the
    /// ref/value placement math a third time (it's already duplicated once,
    /// deliberately, in `eda-gates`' `collect_text_boxes`).
    /// Also steers clear of `obstacles` (other symbols' already-placed
    /// ref/value boxes; see `geometry::resolve_text_y_obs`/
    /// `resolve_value_pos_obs`) and pushes its own boxes onto `obstacles`
    /// before returning, so a caller looping over symbols in a fixed order
    /// gets label-vs-ref/value AND ref/value-vs-ref/value clearance for
    /// free.
    fn ref_value_boxes(&self, wires: &[eda_model::ir::Wire], obstacles: &mut Vec<geometry::TextBox>) -> Vec<geometry::TextBox> {
        let is_passive = geometry::is_two_pin_passive(self.part);
        let h_mm = self.height as f64 / 1000.0;
        let cy = h_mm / 2.0;
        let _ = (is_passive, cy, h_mm);
        let sym_x = self.sym.at.x as f64 / 1000.0;
        let sym_y = self.sym.at.y as f64 / 1000.0;
        let (_, base_ref_y) = geometry::ref_slot_local(self.part, self.height, self.resolved.as_ref());
        let ref_y = geometry::resolve_text_y_obs(sym_x, sym_y, 0.0, base_ref_y, -1.0, &self.sym.id, 1.6, geometry::HAnchor::Start, wires, obstacles);
        let ref_box = geometry::text_bbox(sym_x, sym_y + ref_y, &self.sym.id, 1.6, geometry::HAnchor::Start);
        obstacles.push(ref_box);
        let mut out = vec![ref_box];
        if let Some(val) = symbol_value(self.part) {
            let (base_value_x, base_value_y) = geometry::value_slot_local(self.part, self.height);
            let vf = geometry::value_font_mm(&val, self.width);
            let (value_x, value_y) = geometry::resolve_value_pos_obs(sym_x, sym_y, base_value_x, base_value_y, &val, vf, wires, obstacles);
            let value_box = geometry::text_bbox(sym_x + value_x, sym_y + value_y, &val, vf, geometry::HAnchor::Start);
            obstacles.push(value_box);
            out.push(value_box);
        }
        out
    }

    /// Every pin-name/number text box this symbol draws, in absolute mm —
    /// computed with the exact same `geometry::pin_text_box` the actual
    /// draw loop below uses. Fed into net-label placement's fixed obstacle
    /// list (alongside ref/value boxes) so a label jogs clear of pin text
    /// too, the same way it already jogs clear of ref/value and other
    /// labels; `eda-gates`' `collect_text_boxes` computes this identically.
    fn pin_text_boxes(&self) -> Vec<geometry::TextBox> {
        let sym_x = self.sym.at.x as f64 / 1000.0;
        let sym_y = self.sym.at.y as f64 / 1000.0;
        self.ports
            .iter()
            .enumerate()
            .filter_map(|(port_idx, port)| {
                let pin_idx = self.pin_of_port[port_idx]?;
                let pin = &self.part.pins[pin_idx];
                let label = pin.name.clone().unwrap_or_else(|| pin.number.clone());
                Some(geometry::pin_text_box(port, self.width, self.height, sym_x, sym_y, &label))
            })
            .collect()
    }

    fn render(&self, svg: &mut String, connected: &Connected, wires: &[eda_model::ir::Wire], obstacles: &mut Vec<geometry::TextBox>) {
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
        //
        // For a recognized 2-pin passive the box is sized to fit up-to-4-pin
        // ICs (see `geometry::node_size`), but the glyph itself is only ever
        // drawn on the box's vertical center line (`cy = h/2`) — so anchoring
        // ref/value off the box's top/bottom edge (as for a genuinely tall
        // IC box) leaves them floating several mm from the part a reader
        // actually sees. Anchor to `cy` instead for passives.
        let (_, base_ref_y) = geometry::ref_slot_local(self.part, self.height, self.resolved.as_ref());
        let sym_x = self.sym.at.x as f64 / 1000.0;
        let sym_y = self.sym.at.y as f64 / 1000.0;
        let ref_y = geometry::resolve_text_y_obs(sym_x, sym_y, 0.0, base_ref_y, -1.0, &self.sym.id, 1.6, geometry::HAnchor::Start, wires, obstacles);
        obstacles.push(geometry::text_bbox(sym_x, sym_y + ref_y, &self.sym.id, 1.6, geometry::HAnchor::Start));
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
            // text either. Passives anchor off `cy` (see above) instead of
            // the box's bottom edge, so the value hugs the drawn glyph.
            let (base_value_x, base_value_y) = geometry::value_slot_local(self.part, self.height);
            let vf = geometry::value_font_mm(&val, self.width);
            let sym_x = self.sym.at.x as f64 / 1000.0;
            let sym_y = self.sym.at.y as f64 / 1000.0;
            let (value_x, value_y) = geometry::resolve_value_pos_obs(sym_x, sym_y, base_value_x, base_value_y, &val, vf, wires, obstacles);
            obstacles.push(geometry::text_bbox(sym_x + value_x, sym_y + value_y, &val, vf, geometry::HAnchor::Start));
            let _ = writeln!(
                svg,
                r#"<text class="value" x="{x}" y="{y}" style="font-size:{f}px">{t}</text>"#,
                f = fmt_f(vf),
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
    let _ = width_mm;
    let (dx, dy, anchor) = geometry::pin_text_offset(port.side);
    let class = match anchor {
        geometry::HAnchor::Middle => "pin-vert",
        geometry::HAnchor::Start => "pin-left",
        geometry::HAnchor::End => "pin-right",
    };
    (lx_um / 1000.0 + dx, ly_um / 1000.0 + dy, class)
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
    // `geometry::wire_junction_points` is the single source of truth for
    // what counts as a junction — shared with `eda-gates`'
    // `schematic_missing_junction` so the two can never disagree.
    for (_, pt) in geometry::wire_junction_points(&sch.wires) {
        let _ = writeln!(
            svg,
            r#"<circle class="junction" cx="{x}" cy="{y}" r="0.25"/>"#,
            x = fmt_mm(pt.x),
            y = fmt_mm(pt.y),
        );
    }
}

/// A `PowerSymbol` (a real `power:GND`/`power:<RAIL>`/`power:PWR_FLAG`
/// instance the engine drops at a pin instead of wiring or labeling it —
/// see `eda_engine`'s own doc on why power/ground nets get this treatment)
/// as a small fixed glyph at its own point: the same ground-bars/
/// power-arrow ink `render_net_label` draws for a *label*, reusing
/// [`geometry::ground_glyph_points`]/[`geometry::power_glyph_points`]
/// directly rather than through `geometry::resolve_net_label`'s
/// obstacle-jogging search — a placed symbol instance doesn't jog to dodge
/// other ink the way a label does, it sits exactly where the engine (or an
/// imported file) put it, like any other symbol. `"power:GND"` is the
/// engine's own exact spelling for a ground symbol (`power_symbol_lib_id`);
/// everything else (a named rail, or `power:PWR_FLAG`) draws as the
/// upward power arrow.
fn render_power_symbol(ps: &eda_model::ir::PowerSymbol, svg: &mut String) {
    let x = ps.at.x as f64 / 1000.0;
    let y = ps.at.y as f64 / 1000.0;
    if ps.lib_id == "power:GND" {
        let drop = y + 0.5;
        let [(bx1, _), (bx2, _), (mx1, d2), (mx2, _), (tx1, d3), (tx2, _)] = geometry::ground_glyph_points(x, drop);
        let ty = d3 + 1.1;
        let _ = writeln!(
            svg,
            r#"<g class="power-symbol gnd-label"><line class="gnd-sym" x1="{x}" y1="{y}" x2="{x}" y2="{drop}"/>
<line class="gnd-sym" x1="{bx1}" y1="{drop}" x2="{bx2}" y2="{drop}"/>
<line class="gnd-sym" x1="{mx1}" y1="{d2}" x2="{mx2}" y2="{d2}"/>
<line class="gnd-sym" x1="{tx1}" y1="{d3}" x2="{tx2}" y2="{d3}"/>
<text x="{x}" y="{ty}">{net}</text></g>"#,
            x = fmt_f(x),
            y = fmt_f(y),
            drop = fmt_f(drop),
            bx1 = fmt_f(bx1),
            bx2 = fmt_f(bx2),
            d2 = fmt_f(d2),
            mx1 = fmt_f(mx1),
            mx2 = fmt_f(mx2),
            d3 = fmt_f(d3),
            tx1 = fmt_f(tx1),
            tx2 = fmt_f(tx2),
            ty = fmt_f(ty),
            net = xml_escape(&ps.net),
        );
    } else {
        let stem_top = y - 0.5;
        let [(ax1, ay), (ax2, _)] = geometry::power_glyph_points(x, stem_top);
        let ty = stem_top - 0.9;
        let _ = writeln!(
            svg,
            r#"<g class="power-symbol pwr-label"><line class="pwr-sym" x1="{x}" y1="{y}" x2="{x}" y2="{stem_top}"/>
<path class="pwr-sym" d="M {ax1} {ay} L {x} {stem_top} L {ax2} {ay}"/>
<text x="{x}" y="{ty}">{net}</text></g>"#,
            x = fmt_f(x),
            y = fmt_f(y),
            stem_top = fmt_f(stem_top),
            ax1 = fmt_f(ax1),
            ax2 = fmt_f(ax2),
            ay = fmt_f(ay),
            ty = fmt_f(ty),
            net = xml_escape(&ps.net),
        );
    }
}

/// Full ink extent (mm) of [`render_power_symbol`]'s own fixed glyph, the
/// same role [`geometry::net_label_extent`] plays for a label — folded into
/// `render_schematic`'s own bounds union so a power symbol's bars/arrow (and
/// its net-name text) can never poke past the sheet's `viewBox` and get
/// clipped, the exact defect that union exists to prevent for labels.
/// Deliberately approximate (a fixed half-width margin around the text
/// rather than measuring it): this ink is small and always sits right next
/// to its own symbol's already-counted box, so a generous fixed pad costs
/// nothing and keeps this in lock-step with the fixed (non-jogging) glyph
/// `render_power_symbol` actually draws.
fn power_symbol_extent(ps: &eda_model::ir::PowerSymbol) -> geometry::TextBox {
    let x = ps.at.x as f64 / 1000.0;
    let y = ps.at.y as f64 / 1000.0;
    let half_w = (ps.net.len() as f64 * 0.7 / 2.0).max(0.9);
    if ps.lib_id == "power:GND" {
        geometry::TextBox { x0: x - half_w, y0: y, x1: x + half_w, y1: y + 2.1 }
    } else {
        geometry::TextBox { x0: x - half_w, y0: y - 2.1, x1: x + half_w, y1: y }
    }
}

/// What counts as "connected", for the NC dead-end marker: a set of
/// `"REF.PIN"` refs carried by some wire, plus a set of net-label anchor
/// points (`NetLabel::at`, which the engine places at the port's on-box
/// attachment point — *not* the stub tip, so this must be compared against
/// the same on-box point, not the stub tip, when matching a label to a pin)
/// or a power symbol's own point (its pin sits exactly there, the same
/// coincident-point convention `eda_kicad`'s writer and `check_erc` both
/// use — see `eda_engine::derive_schematic`'s power-symbol placement).
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
    for p in &sch.power_symbols {
        label_points.insert(p.at);
    }
    Connected { wired_pins, label_points }
}

/// Unions the symbol boxes' (+ pin stub tips') absolute extent, every wire
/// point, and every box in `extra` (ref/value/pin text, symbol box
/// rectangles, and each net label's full drawn-ink extent — see
/// `render_schematic`) into one bounding box, in mm. This — not symbol
/// boxes/wire points/label *anchors* alone — is the sheet's true drawn
/// extent: an anchor-only union misses a right/east-growing label's ink
/// entirely, which is exactly how content used to escape the viewBox and
/// get clipped.
fn compute_bounds(boxes: &[SymbolBox], sch: &SchematicSection, extra: &[geometry::TextBox]) -> (f64, f64, f64, f64) {
    let mut min_x = f64::INFINITY;
    let mut min_y = f64::INFINITY;
    let mut max_x = f64::NEG_INFINITY;
    let mut max_y = f64::NEG_INFINITY;
    let mut feed = |x: f64, y: f64| {
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
            feed(p.x as f64 / 1000.0, p.y as f64 / 1000.0);
        }
    }
    for b in extra {
        feed(b.x0, b.y0);
        feed(b.x1, b.y1);
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
        Part { reference: reference.into(), mpn: None, lcsc: None, value: value.map(String::from), package: None, footprint: None, pins, body_um: None, symbol: None, datasheet: None, edge: None }
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
        // Only GND is a true rail name on this fixture (see
        // `geometry::is_power_or_ground_net_name`); VIN/VOUT stay ordinary
        // wires. GND draws no wire and no label — every pin gets its own
        // real `power:GND` symbol instead (see `eda_engine::derive_schematic`).
        // Check that each one's glyph is anchored exactly at its own point.
        let model = ldo_model();
        let design = ldo_design();
        let svg = render_schematic(&design, &model).unwrap();
        let sch = design.schematic.unwrap();
        assert!(sch.wires.iter().all(|w| w.net != "GND"), "GND is power-style: no wire expected");
        assert!(sch.labels.iter().all(|l| l.net != "GND"), "GND is power-style: no label expected");
        let gnd_power = sch.power_symbols.iter().find(|p| p.net == "GND" && p.lib_id == "power:GND").unwrap();
        let expected_anchor = format!(r#"x1="{}" y1="{}""#, fmt_mm(gnd_power.at.x), fmt_mm(gnd_power.at.y));
        assert!(svg.contains(&expected_anchor), "expected power symbol anchor {expected_anchor} in SVG:\n{svg}");
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
        // Every power symbol's own glyph ink (not just its anchor point —
        // the ground bars/power arrow reach beyond it) must fit too, or
        // `power_symbol_extent` isn't actually keeping the promise
        // `render_power_symbol`'s own doc comment makes.
        for ps in &sch.power_symbols {
            let b = power_symbol_extent(ps);
            assert!(b.x0 >= vx && b.x1 <= vx + vw, "power symbol {} x extent out of viewBox", ps.id);
            assert!(b.y0 >= vy && b.y1 <= vy + vh, "power symbol {} y extent out of viewBox", ps.id);
        }
    }

    #[test]
    fn net_labels_rendered() {
        // Force a dense GND net (>4 pins, all ground) so the engine emits
        // a real `power:GND` symbol at each pin (not a label or a wire —
        // see `eda_engine::derive_schematic`'s power/ground handling).
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
        let sch = design.schematic.as_ref().unwrap();
        assert!(sch.labels.iter().all(|l| l.net != "GND"), "GND is power-style: no label expected");
        let svg = render_schematic(&design, &model).unwrap();
        // Dense GND net -> a ground-glyph power symbol per pin, not a label.
        assert!(svg.contains("class=\"power-symbol gnd-label\""));
        assert!(svg.matches("class=\"power-symbol gnd-label\"").count() >= 5);
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
            schematic: None, nets: None,
            placement: None,
            routing: None,
            drawings: None,
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
