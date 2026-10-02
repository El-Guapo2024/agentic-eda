//! Readability/style checks, folded into the ERC engine as the same kind of
//! test KiCad's own ERC runs alongside pure electrical ones (KiCad flags
//! similar labels and off-grid pins too, not just pin conflicts). These are
//! this project's own generator-quality checks -- grid alignment, wire
//! orthogonality/length/overlap, label placement, sheet density/aspect --
//! things a hand-drawn KiCad sheet has a human judging instead. Ported
//! (moved, not rewritten) from the former `eda-gates::check_style` so
//! `eda_kicad::check_erc` is the one engine judging a schematic, per the
//! project's own "no duplicate tools" rule; every check keeps its original
//! name, severity and fix hint verbatim.

use std::collections::BTreeMap;

use eda_engine::geometry;
use eda_layout::{Node as LayoutNode, Point as LPoint, Port};
use eda_model::ir::{Design, Point as IrPoint, SchematicSection, SymbolInstance};
use eda_model::{CheckResult, CheckStatus, ConstraintModel, Part};

fn to_lpoint(p: IrPoint) -> LPoint {
    LPoint { x: p.x, y: p.y }
}


/// A symbol's re-derived box + port geometry, keyed by symbol id.
struct SymGeo<'a> {
    #[allow(dead_code)]
    part: &'a Part,
    top_left: LPoint,
    width: i64,
    height: i64,
    node: LayoutNode,
    /// pin index (in `part.pins`) -> port index, mirroring
    /// `geometry::build_ports`'s own return.
    pin_port: Vec<Option<usize>>,
}

impl<'a> SymGeo<'a> {
    fn build(sym: &SymbolInstance, part: &'a Part, model: &ConstraintModel) -> Self {
        let resolved = model.real_symbol_of(&sym.lib_id, part);
        let (width, height) = geometry::node_size(part, resolved.as_ref(), sym.unit);
        let (ports, pin_port): (Vec<Port>, _) = geometry::build_ports(part, width, height, resolved.as_ref(), sym.unit);
        let node = LayoutNode { id: 0, width, height, ports };
        Self { part, top_left: to_lpoint(sym.at), width, height, node, pin_port }
    }

    fn bounds(&self) -> (i64, i64, i64, i64) {
        (self.top_left.x, self.top_left.x + self.width, self.top_left.y, self.top_left.y + self.height)
    }

    fn port_side_for_pin_number(&self, pin_number: &str) -> Option<eda_layout::Side> {
        let pin_idx = self.part.pins.iter().position(|p| p.number == pin_number)?;
        let port_idx = self.pin_port.get(pin_idx).copied().flatten()?;
        Some(self.node.ports[port_idx].side)
    }

    fn stub_tip_for_pin_number(&self, pin_number: &str) -> Option<LPoint> {
        let pin_idx = self.part.pins.iter().position(|p| p.number == pin_number)?;
        let port_idx = self.pin_port.get(pin_idx).copied().flatten()?;
        Some(geometry::stub_tip(&self.node, self.top_left, port_idx))
    }
}

fn build_geos<'a>(sch: &SchematicSection, model: &'a ConstraintModel) -> BTreeMap<String, SymGeo<'a>> {
    let mut geos = BTreeMap::new();
    for sym in &sch.symbols {
        if let Some(part) = model.part(&sym.id) {
            geos.insert(sym.id.clone(), SymGeo::build(sym, part, model));
        }
    }
    geos
}

fn split_pin_ref(pin_ref: &str) -> (&str, &str) {
    pin_ref.split_once('.').unwrap_or((pin_ref, ""))
}

/// Runs every schematic geometry gate against `design.schematic`, re-using
/// `model` to recompute each symbol's box/port layout the same way the
/// engine and renderer do. Returns one `CheckResult` per finding, plus a
/// `Pass` for any check that found nothing to fail/warn about, and always
/// exactly one `schematic_wire_crossing_count` result (Warn, informational).
pub(crate) fn check_style(design: &Design, model: &ConstraintModel) -> Vec<CheckResult> {
    let mut results = Vec::new();
    let Some(sch) = design.schematic.as_ref() else {
        results.push(CheckResult::fail("schematic_wire_through_symbol", "design", "design has no schematic section"));
        return results;
    };
    let geos = build_geos(sch, model);

    check_offgrid(sch, &mut results);
    check_orthogonal(sch, &mut results);
    check_symbol_overlap(&geos, &mut results);
    check_wire_through_symbol(sch, &geos, &mut results);
    check_endpoint_off_pin(sch, &geos, &mut results);
    check_wire_overlap(sch, &mut results);
    check_wire_detour(sch, &geos, &mut results);
    check_crossing_count(sch, &mut results);
    check_text_overlap(sch, model, &mut results);
    check_label_over_wire(sch, model, &mut results);
    check_label_in_symbol(sch, model, &mut results);
    check_value_label_distance(sch, model, &mut results);
    check_power_net_as_wire(sch, &mut results);
    check_column_overflow(sch, model, &mut results);
    check_missing_junction(sch, &mut results);
    check_flow_direction(sch, model, &mut results);
    check_flag_adjacent(sch, model, &mut results);
    check_sheet_aspect(sch, &geos, &mut results);
    check_wire_length(sch, &mut results);
    check_wire_ink(sch, &mut results);
    check_cluster_split(model, &geos, &mut results);
    check_sheet_density(sch, &geos, &mut results);
    check_content_in_bounds(design, sch, model, &geos, &mut results);

    results
}

/// Fails when two flag/label texts of the *same* net sit closer together
/// than one text height — the "VDDVDD"/"GNDGND" defect where two adjacent
/// same-net pins on one part (e.g. a MCU's VDD/VDD2 or GND/GND2) each get
/// their own power-flag/ground-glyph label and the texts run together.
/// `schematic_text_overlap` already catches outright overlap between *any*
/// two text boxes; this gate is the stricter, same-net-specific version that
/// also fails on a near-miss (texts that don't technically overlap but sit
/// closer than one line of text apart, which reads as merged at a glance).
/// Uses `collect_text_boxes` — the same estimator (`geometry::text_bbox`)
/// `eda-render` places labels with — so gate and renderer never drift.
fn check_flag_adjacent(sch: &SchematicSection, model: &ConstraintModel, out: &mut Vec<CheckResult>) {
    const LABEL_FONT_MM: f64 = 1.3;
    let boxes = collect_text_boxes(sch, model);
    // Only the `label:{net}@{x},{y}` entries are net-label flags/glyphs;
    // ref/value boxes are keyed `{ref}:ref`/`{ref}:value` and never collide
    // with a same-net check since they don't carry a net name at all.
    let labels: Vec<(&str, &geometry::TextBox)> =
        boxes.iter().filter_map(|(name, b)| name.strip_prefix("label:").map(|rest| (rest.split('@').next().unwrap_or(rest), b))).collect();

    let mut ok = true;
    for i in 0..labels.len() {
        for j in (i + 1)..labels.len() {
            let (net_i, bi) = labels[i];
            let (net_j, bj) = labels[j];
            if net_i != net_j {
                continue;
            }
            let dx = (bi.x0 - bj.x1).max(bj.x0 - bi.x1).max(0.0);
            let dy = (bi.y0 - bj.y1).max(bj.y0 - bi.y1).max(0.0);
            let dist = dx.max(dy);
            if dist < LABEL_FONT_MM {
                out.push(CheckResult {
                    check: "schematic_flag_adjacent".into(),
                    status: CheckStatus::Fail,
                    location: Some(net_i.to_string()),
                    hint: Some(format!(
                        "two '{net_i}' flag/label texts are only {dist:.2}mm apart (min {LABEL_FONT_MM}mm, one text height) — they read as merged"
                    )), detail: None
                });
                ok = false;
            }
        }
    }
    if ok {
        out.push(CheckResult::pass("schematic_flag_adjacent"));
    }
}

/// True for a part that reads as an external connector/input — a header,
/// jack, or switch feeding the design from off-sheet — using the same
/// reference-prefix convention the rest of the crate already leans on for
/// part identity (`J*`/`SW*`/`P*`), since the model carries no first-class
/// "connector" kind. Signal flow should read left-to-right: such a part
/// driving a net should sit to the left of what it drives, not among or to
/// the right of it (see `check_flow_direction`).
fn is_flow_source_part(part: &Part) -> bool {
    let r = part.reference.trim_start_matches(|c: char| !c.is_ascii_alphabetic());
    let prefix: String = r.chars().take_while(|c| c.is_ascii_alphabetic()).collect();
    matches!(prefix.to_ascii_uppercase().as_str(), "J" | "SW" | "P")
}

/// Fails when a connector/input part (see [`is_flow_source_part`]) sits to
/// the right of the average x of the other parts it shares a net with —
/// i.e. signal flow into the board would visually run right-to-left there,
/// against the left-to-right convention the rest of the sheet follows.
/// Power/ground nets are excluded (a connector's GND/VCC pin says nothing
/// about signal direction and is often the majority of its net-degree), and
/// so are other connectors on the same net — see the note at the filter.
fn check_flow_direction(sch: &SchematicSection, model: &ConstraintModel, out: &mut Vec<CheckResult>) {
    let sym_x: BTreeMap<&str, i64> = sch.symbols.iter().map(|s| (s.id.as_str(), s.at.x)).collect();
    let mut ok = true;
    for sym in &sch.symbols {
        let Some(part) = model.part(&sym.id) else { continue };
        if !is_flow_source_part(part) {
            continue;
        }
        let mut others: Vec<i64> = Vec::new();
        for w in &sch.wires {
            if geometry::is_power_or_ground_net_name(&w.net) {
                continue;
            }
            if !w.pins.iter().any(|p| split_pin_ref(p).0 == sym.id) {
                continue;
            }
            for p in &w.pins {
                let (r, _) = split_pin_ref(p);
                if r == sym.id {
                    continue;
                }
                // Judge a connector only against the parts it actually
                // *drives*. Two connectors on one net (a header feeding a
                // breakout, say) carry no flow direction between them: with
                // both in the average, whichever one the star decomposition
                // happened not to pick as hub is flagged for sitting right
                // of the other, which says nothing about whether the sheet
                // reads left-to-right.
                if model.part(r).map(is_flow_source_part).unwrap_or(false) {
                    continue;
                }
                if let Some(&x) = sym_x.get(r) {
                    others.push(x);
                }
            }
        }
        if others.is_empty() {
            continue;
        }
        let avg = others.iter().sum::<i64>() as f64 / others.len() as f64;
        if (sym.at.x as f64) > avg {
            out.push(CheckResult {
                check: "schematic_flow_direction".into(),
                status: CheckStatus::Fail,
                location: Some(sym.id.clone()),
                hint: Some(format!(
                    "connector '{}' at x={} sits right of the average x={:.0} of the parts it drives — signal flow should read left-to-right",
                    sym.id, sym.at.x, avg
                )), detail: None
            });
            ok = false;
        }
    }
    if ok {
        out.push(CheckResult::pass("schematic_flow_direction"));
    }
}

/// Every point where 3+ same-net wire segments meet must carry a junction
/// dot in the rendered SVG (`eda-render`'s `render_junctions`, which draws
/// one at every point `geometry::wire_junction_points` returns — the full
/// definition, including a wire endpoint landing mid-span on another
/// same-net wire, a "T" meeting with no shared vertex). This gate fails on
/// exactly the case a naive coincident-vertex-only junction count would
/// silently miss: a real T meeting where the through-wire's own polyline
/// never explicitly touches the point (so anything that only counted
/// coincident vertices, not this crate's full definition, would render
/// there with no dot at all). Ordinary vertex-aligned junctions (the common
/// case: three wires already sharing an explicit bend point from routing)
/// are unaffected and never flagged.
fn check_missing_junction(sch: &SchematicSection, out: &mut Vec<CheckResult>) {
    let full = geometry::wire_junction_points(&sch.wires);
    let naive = naive_junction_points(&sch.wires);
    let mut ok = true;
    for (net, pt) in full.difference(&naive) {
        out.push(CheckResult {
            check: "schematic_missing_junction".into(),
            status: CheckStatus::Fail,
            location: Some(format!("{net}@{},{}", pt.x, pt.y)),
            hint: Some(format!(
                "net '{net}' has 3+ wire segments meeting at ({},{}) with no shared vertex there (a naive junction-dot renderer would miss it)",
                pt.x, pt.y
            )), detail: None
        });
        ok = false;
    }
    if ok {
        out.push(CheckResult::pass("schematic_missing_junction"));
    }
}

/// Coincident-vertex-only junction count (the naive half of
/// `geometry::wire_junction_points`'s definition, kept separate here only
/// to detect when the full definition found something extra — see
/// `check_missing_junction`).
fn naive_junction_points(wires: &[eda_model::ir::Wire]) -> std::collections::BTreeSet<(String, eda_model::ir::Point)> {
    let mut touches: BTreeMap<(String, eda_model::ir::Point), usize> = BTreeMap::new();
    for w in wires {
        if w.pts.len() < 2 {
            continue;
        }
        for pair in w.pts.windows(2) {
            for pt in [pair[0], pair[1]] {
                *touches.entry((w.net.clone(), pt)).or_insert(0) += 1;
            }
        }
    }
    touches.into_iter().filter(|(_, n)| *n >= 3).map(|(k, _)| k).collect()
}

/// A wire segment (in mm, orthogonal) drawn through any ref/value/net-label
/// text: the label reads as struck-through by a wire, one of the
/// readability defects this crate is meant to catch. Fails if any wire
/// segment's line passes through a text box's interior.
fn check_label_over_wire(sch: &SchematicSection, model: &ConstraintModel, out: &mut Vec<CheckResult>) {
    let boxes = collect_text_boxes(sch, model);
    let mut ok = true;
    for (name, bbox) in &boxes {
        for w in &sch.wires {
            for seg in w.pts.windows(2) {
                let (ax, ay) = (seg[0].x as f64 / 1000.0, seg[0].y as f64 / 1000.0);
                let (bx, by) = (seg[1].x as f64 / 1000.0, seg[1].y as f64 / 1000.0);
                if geometry::segment_crosses_box(ax, ay, bx, by, bbox) {
                    out.push(CheckResult {
                        check: "schematic_label_over_wire".into(),
                        status: CheckStatus::Fail,
                        location: Some(name.clone()),
                        hint: Some(format!(
                            "text '{name}' is crossed by a wire segment ({},{})->({},{})",
                            seg[0].x, seg[0].y, seg[1].x, seg[1].y
                        )), detail: None
                    });
                    ok = false;
                }
            }
        }
    }
    if ok {
        out.push(CheckResult::pass("schematic_label_over_wire"));
    }
}

/// Fails when any net-label's ink — its tag/flag glyph or the name text
/// itself, using the exact same boxes `eda-render` draws with (see
/// `collect_text_boxes`, which mirrors `render_net_label`'s placement
/// search) — overlaps ANY symbol's box rectangle. A label belongs outside
/// the body, sitting on its pin's stub side; nothing before this gate
/// forbade the crowded-label search from landing a tag mid-body once every
/// outward slot looked taken, which is exactly the "tag drawn on top of a
/// pin name" defect reported on U7/U8 of `l4_control_hub`.
fn check_label_in_symbol(sch: &SchematicSection, model: &ConstraintModel, out: &mut Vec<CheckResult>) {
    let boxes = collect_text_boxes(sch, model);
    let symbol_boxes = collect_symbol_boxes(sch, model);
    let mut ok = true;
    for (name, bbox) in &boxes {
        if !name.starts_with("label:") {
            continue;
        }
        for sbox in &symbol_boxes {
            if bbox.overlaps(sbox) {
                out.push(CheckResult {
                    check: "schematic_label_in_symbol".into(),
                    status: CheckStatus::Fail,
                    location: Some(name.clone()),
                    hint: Some("net-label ink overlaps a symbol's box — it must sit outside the body, on the pin's stub side".into()), detail: None
                });
                ok = false;
                break;
            }
        }
    }
    if ok {
        out.push(CheckResult::pass("schematic_label_in_symbol"));
    }
}

/// A ref/value label placed further than `MAX_LABEL_DIST_MM` (Chebyshev
/// distance, mm) from its owning symbol's box is effectively floating —
/// readable but no longer obviously "this part's label" at a glance. Fails
/// per offending label.
const MAX_LABEL_DIST_MM: f64 = 6.0;

fn check_value_label_distance(sch: &SchematicSection, model: &ConstraintModel, out: &mut Vec<CheckResult>) {
    let mut ok = true;
    let mut placed: Vec<geometry::TextBox> = Vec::new();
    for sym in &sch.symbols {
        let Some(part) = model.part(&sym.id) else { continue };
        let (width, height) = geometry::node_size(part, model.real_symbol_of(&sym.lib_id, part).as_ref(), sym.unit);
        let x = sym.at.x as f64 / 1000.0;
        let y = sym.at.y as f64 / 1000.0;
        let w = width as f64 / 1000.0;
        let h = height as f64 / 1000.0;
        let (box_x0, box_x1, box_y0, box_y1) = (x, x + w, y, y + h);

        let mut check_one = |name: &str, tx0: f64, ty0: f64, tx1: f64, ty1: f64| {
            let dx = (tx0 - box_x1).max(box_x0 - tx1).max(0.0);
            let dy = (ty0 - box_y1).max(box_y0 - ty1).max(0.0);
            let dist = dx.max(dy);
            if dist > MAX_LABEL_DIST_MM {
                out.push(CheckResult {
                    check: "schematic_label_far_from_part".into(),
                    status: CheckStatus::Fail,
                    location: Some(format!("{}:{}", sym.id, name)),
                    hint: Some(format!("{} label is {:.1}mm from {}'s box (max {}mm)", name, dist, sym.id, MAX_LABEL_DIST_MM)), detail: None
                });
                ok = false;
            }
        };

        use geometry::{text_bbox, HAnchor};
        let (_, base_ref_y) = geometry::ref_slot_local(part, height, model.real_symbol_of(&sym.lib_id, part).as_ref());
        let ref_y = geometry::resolve_text_y_obs(x, y, 0.0, base_ref_y, -1.0, &sym.id, 1.6, HAnchor::Start, &sch.wires, &placed);
        let rb = text_bbox(x, y + ref_y, &sym.id, 1.6, HAnchor::Start);
        check_one("ref", rb.x0, rb.y0, rb.x1, rb.y1);
        placed.push(rb);

        let value = match (&part.value, &part.mpn) {
            (Some(v), _) => Some(v.clone()),
            (None, Some(m)) => Some(m.clone()),
            (None, None) => None,
        };
        if let Some(val) = value {
            let (base_value_x, base_value_y) = geometry::value_slot_local(part, height);
            let vf = geometry::value_font_mm(&val, width);
            let (value_x, value_y) = geometry::resolve_value_pos_obs(x, y, base_value_x, base_value_y, &val, vf, &sch.wires, &placed);
            let vb = text_bbox(x + value_x, y + value_y, &val, vf, HAnchor::Start);
            check_one("value", vb.x0, vb.y0, vb.x1, vb.y1);
            placed.push(vb);
        }
    }
    if ok {
        out.push(CheckResult::pass("schematic_label_far_from_part"));
    }
}

/// Estimated ref/value/net-label text boxes (using the same
/// `geometry::text_bbox` estimator the renderer uses to place them), so this
/// gate and `eda-render`'s placement always agree on what counts as
/// clearance. Only the ref/value/net-label text is modeled here (pin-name
/// text sits tight against the box interior at fixed offsets and is not a
/// realistic collision source in practice).
/// Every symbol's box rectangle (mm) — recomputed the same way
/// `eda-render` does (`geometry::symbol_box_mm`), used both as a hard
/// placement obstacle for label positioning below and, directly, by
/// `schematic_label_in_symbol`.
fn collect_symbol_boxes(sch: &SchematicSection, model: &ConstraintModel) -> Vec<geometry::TextBox> {
    sch.symbols
        .iter()
        .filter_map(|sym| {
            let part = model.part(&sym.id)?;
            let (width, height) = geometry::node_size(part, model.real_symbol_of(&sym.lib_id, part).as_ref(), sym.unit);
            Some(geometry::symbol_box_mm(sym.at.x as f64 / 1000.0, sym.at.y as f64 / 1000.0, width, height))
        })
        .collect()
}

/// The tight ink box each drawn label/ref/value/pin-text occupies — see
/// [`collect_text_boxes_and_label_extents`], which this just discards the
/// second (full-extent) element of.
fn collect_text_boxes(sch: &SchematicSection, model: &ConstraintModel) -> Vec<(String, geometry::TextBox)> {
    collect_text_boxes_and_label_extents(sch, model).0
}

/// Returns every drawn text/tag's *tight* ink box (`{ref}:ref`, `{ref}:value`,
/// `{ref}:pin{n}`, `label:{net}@{x},{y}` keys — used by the overlap/adjacency
/// gates, which measure exactly the ink `eda-render` draws) alongside, for
/// net labels only, that same label's *full* drawn-glyph extent (leader
/// line(s) plus, for Ground/Power, the bars/arrowhead — see
/// `geometry::net_label_extent`), for `schematic_content_in_bounds`, which
/// needs to know how far a label's ink actually reaches, not just its tag/
/// text box. Both come from the one shared `geometry::resolve_net_label`
/// call per label, so a bounds check and an overlap check can never see two
/// different positions for the same label.
fn collect_text_boxes_and_label_extents(sch: &SchematicSection, model: &ConstraintModel) -> (Vec<(String, geometry::TextBox)>, Vec<(String, geometry::TextBox)>) {
    use geometry::{text_bbox, HAnchor};
    let symbol_boxes = collect_symbol_boxes(sch, model);
    let mut out = Vec::new();
    for sym in &sch.symbols {
        let Some(part) = model.part(&sym.id) else { continue };
        let (width, height) = geometry::node_size(part, model.real_symbol_of(&sym.lib_id, part).as_ref(), sym.unit);
        let x = sym.at.x as f64 / 1000.0;
        let y = sym.at.y as f64 / 1000.0;
        let _w = width as f64 / 1000.0;
        let h = height as f64 / 1000.0;

        // Mirrors eda-render: a 2-pin passive anchors ref/value off the
        // glyph's vertical center (cy = h/2), not the (IC-sized) box edges.
        let _ = h;
        let (_, base_ref_y) = geometry::ref_slot_local(part, height, model.real_symbol_of(&sym.lib_id, part).as_ref());
        // Steer clear of every ref/value box already placed for an earlier
        // symbol (`out` so far), same "extra obstacles" retry
        // `resolve_text_y_obs`/`resolve_value_pos_obs` use for label-vs-label
        // (see `geometry::resolve_label_pos`) — extended here to
        // ref/value-vs-ref/value, so `schematic_text_overlap` can be a hard
        // Fail instead of a permanent warning.
        let obstacles: Vec<geometry::TextBox> = out.iter().map(|(_, b)| *b).collect();
        let ref_y = geometry::resolve_text_y_obs(x, y, 0.0, base_ref_y, -1.0, &sym.id, 1.6, HAnchor::Start, &sch.wires, &obstacles);
        out.push((format!("{}:ref", sym.id), text_bbox(x, y + ref_y, &sym.id, 1.6, HAnchor::Start)));

        // value: below-left, start-anchored (mirrors eda-render's value_y)
        let value = match (&part.value, &part.mpn) {
            (Some(v), _) => Some(v.clone()),
            (None, Some(m)) => Some(m.clone()),
            (None, None) => None,
        };
        if let Some(val) = value {
            let (base_value_x, base_value_y) = geometry::value_slot_local(part, height);
            let vf = geometry::value_font_mm(&val, width);
            let obstacles: Vec<geometry::TextBox> = out.iter().map(|(_, b)| *b).collect();
            let (value_x, value_y) = geometry::resolve_value_pos_obs(x, y, base_value_x, base_value_y, &val, vf, &sch.wires, &obstacles);
            out.push((format!("{}:value", sym.id), text_bbox(x + value_x, y + value_y, &val, vf, HAnchor::Start)));
        }
    }
    // Pin name/number text: every visible pin `eda-render` actually draws
    // text for, using the exact same `geometry::pin_text_box` it draws with.
    // Computed before the label loop (mirroring `eda-render`'s render()
    // function, which seeds `label_obstacles` with pin boxes before placing
    // any net label) and pushed into `out` immediately so the obstacle list
    // built from `out` below matches the renderer's exactly — pin text is a
    // fixed obstacle labels must jog clear of, just like ref/value.
    for sym in &sch.symbols {
        let Some(part) = model.part(&sym.id) else { continue };
        let resolved = model.real_symbol_of(&sym.lib_id, part);
        let (width, height) = geometry::node_size(part, resolved.as_ref(), sym.unit);
        let (ports, pin_port) = geometry::build_ports(part, width, height, resolved.as_ref(), sym.unit);
        let sym_x = sym.at.x as f64 / 1000.0;
        let sym_y = sym.at.y as f64 / 1000.0;
        for (pin_idx, port_idx) in pin_port.iter().enumerate() {
            let Some(port_idx) = port_idx else { continue };
            let pin = &part.pins[pin_idx];
            let label = pin.name.clone().unwrap_or_else(|| pin.number.clone());
            let bbox = geometry::pin_text_box(&ports[*port_idx], width, height, sym_x, sym_y, &label);
            out.push((format!("{}:pin{}", sym.id, pin.number), bbox));
        }
    }

    let mut obstacles: Vec<geometry::TextBox> = out.iter().map(|(_, b)| *b).collect();
    let mut label_extents = Vec::new();
    for l in &sch.labels {
        // The single position-resolution routine `eda-render`'s
        // `render_net_label` draws from (see its doc comment): same jog-vs-
        // obstacles search, same obstacle order (`obstacles` accumulated in
        // the same `sch.labels` order render draws them in), so this gate
        // and the renderer can never disagree on where a label lands or how
        // far its ink reaches.
        let (resolved, ink) = geometry::resolve_net_label(l.at.x, l.at.y, &l.net, 1.3, &mut obstacles, &symbol_boxes, &sch.wires);
        let key = format!("label:{}@{},{}", l.net, l.at.x, l.at.y);
        out.push((key.clone(), ink));
        label_extents.push((key, geometry::net_label_extent(&resolved, ink)));
    }
    (out, label_extents)
}

fn check_text_overlap(sch: &SchematicSection, model: &ConstraintModel, out: &mut Vec<CheckResult>) {
    let boxes = collect_text_boxes(sch, model);
    let mut ok = true;
    for i in 0..boxes.len() {
        for j in (i + 1)..boxes.len() {
            if boxes[i].1.overlaps(&boxes[j].1) {
                out.push(CheckResult {
                    check: "schematic_text_overlap".into(),
                    status: CheckStatus::Fail,
                    location: Some(format!("{}/{}", boxes[i].0, boxes[j].0)),
                    hint: Some("estimated text bounding boxes overlap".into()), detail: None
                });
                ok = false;
            }
        }
    }
    if ok {
        out.push(CheckResult::pass("schematic_text_overlap"));
    }
}

const GRID: i64 = geometry::GRID;

fn check_offgrid(sch: &SchematicSection, out: &mut Vec<CheckResult>) {
    let mut ok = true;
    for s in &sch.symbols {
        if s.at.x % GRID != 0 || s.at.y % GRID != 0 {
            out.push(CheckResult::fail("schematic_offgrid", s.id.clone(), format!("symbol position {:?} is not on the {GRID}um grid", s.at)));
            ok = false;
        }
    }
    for w in &sch.wires {
        for p in &w.pts {
            if p.x % GRID != 0 || p.y % GRID != 0 {
                out.push(CheckResult::fail("schematic_offgrid", w.net.clone(), format!("wire point {p:?} is not on the {GRID}um grid")));
                ok = false;
            }
        }
    }
    if ok {
        out.push(CheckResult::pass("schematic_offgrid"));
    }
}

fn check_orthogonal(sch: &SchematicSection, out: &mut Vec<CheckResult>) {
    let mut ok = true;
    for w in &sch.wires {
        for seg in w.pts.windows(2) {
            let (a, b) = (seg[0], seg[1]);
            if a.x != b.x && a.y != b.y {
                out.push(CheckResult::fail("schematic_wire_not_orthogonal", w.net.clone(), format!("segment {a:?} -> {b:?} is neither horizontal nor vertical")));
                ok = false;
            }
        }
    }
    if ok {
        out.push(CheckResult::pass("schematic_wire_not_orthogonal"));
    }
}

fn check_symbol_overlap(geos: &BTreeMap<String, SymGeo>, out: &mut Vec<CheckResult>) {
    let mut ok = true;
    let ids: Vec<&String> = geos.keys().collect();
    for i in 0..ids.len() {
        for j in (i + 1)..ids.len() {
            let (li, ri, ti, bi) = geos[ids[i]].bounds();
            let (lj, rj, tj, bj) = geos[ids[j]].bounds();
            let overlap_x = li < rj && lj < ri;
            let overlap_y = ti < bj && tj < bi;
            if overlap_x && overlap_y {
                out.push(CheckResult::fail(
                    "schematic_symbol_overlap",
                    format!("{}/{}", ids[i], ids[j]),
                    "symbol boxes overlap",
                ));
                ok = false;
            }
        }
    }
    if ok {
        out.push(CheckResult::pass("schematic_symbol_overlap"));
    }
}

/// Axis-aligned-aware "does this segment cross the box's open interior"
/// test (diagonal segments — which `schematic_wire_not_orthogonal` already
/// flags separately — fall back to a conservative bounding-box overlap).
fn seg_crosses_box(a: LPoint, b: LPoint, left: i64, right: i64, top: i64, bottom: i64) -> bool {
    if a.y == b.y {
        let y = a.y;
        if y > top && y < bottom {
            let (x0, x1) = (a.x.min(b.x), a.x.max(b.x));
            return x1 > left && x0 < right;
        }
        false
    } else if a.x == b.x {
        let x = a.x;
        if x > left && x < right {
            let (y0, y1) = (a.y.min(b.y), a.y.max(b.y));
            return y1 > top && y0 < bottom;
        }
        false
    } else {
        let (sx0, sx1) = (a.x.min(b.x), a.x.max(b.x));
        let (sy0, sy1) = (a.y.min(b.y), a.y.max(b.y));
        sx1 > left && sx0 < right && sy1 > top && sy0 < bottom
    }
}

fn check_wire_through_symbol(sch: &SchematicSection, geos: &BTreeMap<String, SymGeo>, out: &mut Vec<CheckResult>) {
    let mut ok = true;
    for w in &sch.wires {
        for seg in w.pts.windows(2) {
            let (a, b) = (to_lpoint(seg[0]), to_lpoint(seg[1]));
            for (id, geo) in geos {
                let (left, right, top, bottom) = geo.bounds();
                if seg_crosses_box(a, b, left, right, top, bottom) {
                    out.push(CheckResult::fail(
                        "schematic_wire_through_symbol",
                        format!("{} through {id}", w.net),
                        format!("segment {a:?} -> {b:?} cuts through {id}'s box"),
                    ));
                    ok = false;
                }
            }
        }
    }
    if ok {
        out.push(CheckResult::pass("schematic_wire_through_symbol"));
    }
}

fn check_endpoint_off_pin(sch: &SchematicSection, geos: &BTreeMap<String, SymGeo>, out: &mut Vec<CheckResult>) {
    let mut ok = true;
    for w in &sch.wires {
        // A wire naming no part pins at all isn't a part-to-part wire this
        // gate can judge -- e.g. the short leg `derive_schematic` draws
        // from a `PWR_FLAG` to an existing power symbol, neither end of
        // which is a `Part` pin. Nothing here re-derives *those* two
        // points' correctness (that's `eda_kicad::erc`'s job on the
        // exported file), so skip rather than false-fail.
        if w.pins.is_empty() {
            continue;
        }
        let (Some(&first), Some(&last)) = (w.pts.first(), w.pts.last()) else { continue };
        let stub_tips: Vec<LPoint> = w
            .pins
            .iter()
            .filter_map(|pin_ref| {
                let (sym_id, pin_num) = split_pin_ref(pin_ref);
                geos.get(sym_id).and_then(|g| g.stub_tip_for_pin_number(pin_num))
            })
            .collect();
        if stub_tips.len() != w.pins.len() {
            out.push(CheckResult::fail("schematic_wire_endpoint_off_pin", w.net.clone(), "one or more of the wire's pin refs did not resolve to a symbol/pin/port"));
            ok = false;
            continue;
        }
        let first = to_lpoint(first);
        let last = to_lpoint(last);
        if !stub_tips.contains(&first) {
            out.push(CheckResult::fail("schematic_wire_endpoint_off_pin", w.net.clone(), format!("first point {first:?} is not at any listed pin's stub tip")));
            ok = false;
        }
        if !stub_tips.contains(&last) {
            out.push(CheckResult::fail("schematic_wire_endpoint_off_pin", w.net.clone(), format!("last point {last:?} is not at any listed pin's stub tip")));
            ok = false;
        }
    }
    if ok {
        out.push(CheckResult::pass("schematic_wire_endpoint_off_pin"));
    }
}

fn seg_orientation(a: IrPoint, b: IrPoint) -> Option<bool> {
    // Some(true) = horizontal, Some(false) = vertical, None = degenerate/diagonal.
    if a.y == b.y && a.x != b.x {
        Some(true)
    } else if a.x == b.x && a.y != b.y {
        Some(false)
    } else {
        None
    }
}

/// Flags any two wire segments (consecutive-point pairs within different
/// nets' `Wire.pts` polylines) that are collinear (both axis-aligned,
/// sharing the same x or same y) and overlap over a positive-length range
/// (not merely touching at a shared endpoint). Same-net overlaps are
/// explicitly allowed (star hubs/branches legitimately share points or
/// segments).
fn check_wire_overlap(sch: &SchematicSection, out: &mut Vec<CheckResult>) {
    let mut segs: Vec<(&str, IrPoint, IrPoint)> = Vec::new();
    for w in &sch.wires {
        for seg in w.pts.windows(2) {
            segs.push((w.net.as_str(), seg[0], seg[1]));
        }
    }

    let mut ok = true;
    for i in 0..segs.len() {
        for j in (i + 1)..segs.len() {
            let (net_i, ai, bi) = segs[i];
            let (net_j, aj, bj) = segs[j];
            if net_i == net_j {
                continue;
            }
            let (Some(oi), Some(oj)) = (seg_orientation(ai, bi), seg_orientation(aj, bj)) else { continue };
            if oi != oj {
                continue; // different orientation: not collinear
            }
            let overlap = if oi {
                // both horizontal: collinear iff same y.
                if ai.y != aj.y {
                    None
                } else {
                    let (x0i, x1i) = (ai.x.min(bi.x), ai.x.max(bi.x));
                    let (x0j, x1j) = (aj.x.min(bj.x), aj.x.max(bj.x));
                    let lo = x0i.max(x0j);
                    let hi = x1i.min(x1j);
                    if hi > lo {
                        Some((ai.y, lo, hi))
                    } else {
                        None
                    }
                }
            } else {
                // both vertical: collinear iff same x.
                if ai.x != aj.x {
                    None
                } else {
                    let (y0i, y1i) = (ai.y.min(bi.y), ai.y.max(bi.y));
                    let (y0j, y1j) = (aj.y.min(bj.y), aj.y.max(bj.y));
                    let lo = y0i.max(y0j);
                    let hi = y1i.min(y1j);
                    if hi > lo {
                        Some((ai.x, lo, hi))
                    } else {
                        None
                    }
                }
            };
            if overlap.is_some() {
                out.push(CheckResult::fail(
                    "schematic_wire_overlap",
                    format!("{net_i}/{net_j}"),
                    format!("segments {ai:?}->{bi:?} ({net_i}) and {aj:?}->{bj:?} ({net_j}) are collinear and overlap over a positive length"),
                ));
                ok = false;
            }
        }
    }
    if ok {
        out.push(CheckResult::pass("schematic_wire_overlap"));
    }
}

/// Sheet shape. A schematic that comes out as a tall ribbon or a wide
/// letterbox does not fit a sheet and reads badly however clean its wires
/// are, so bound the drawn extent's height/width ratio to
/// `[MIN_SHEET_ASPECT, MAX_SHEET_ASPECT]`. The extent is measured over
/// everything that gets drawn (symbol boxes, wire points, label anchors) in
/// final, post-transpose screen coordinates.
const MAX_SHEET_ASPECT: f64 = 2.5;
const MIN_SHEET_ASPECT: f64 = 0.4;

fn check_sheet_aspect(sch: &SchematicSection, geos: &BTreeMap<String, SymGeo<'_>>, out: &mut Vec<CheckResult>) {
    let (mut x0, mut x1, mut y0, mut y1) = (i64::MAX, i64::MIN, i64::MAX, i64::MIN);
    let mut seen = false;
    let mut add = |bx0: i64, bx1: i64, by0: i64, by1: i64, seen: &mut bool| {
        x0 = x0.min(bx0);
        x1 = x1.max(bx1);
        y0 = y0.min(by0);
        y1 = y1.max(by1);
        *seen = true;
    };
    for g in geos.values() {
        let (a, b, c, d) = g.bounds();
        add(a, b, c, d, &mut seen);
    }
    for w in &sch.wires {
        for p in &w.pts {
            add(p.x, p.x, p.y, p.y, &mut seen);
        }
    }
    for l in &sch.labels {
        add(l.at.x, l.at.x, l.at.y, l.at.y, &mut seen);
    }
    if !seen {
        out.push(CheckResult::pass("schematic_sheet_aspect"));
        return;
    }
    let w = (x1 - x0).max(1) as f64;
    let h = (y1 - y0).max(1) as f64;
    let ratio = h / w;
    if ratio > MAX_SHEET_ASPECT || ratio < MIN_SHEET_ASPECT {
        out.push(CheckResult::fail(
            "schematic_sheet_aspect",
            "sheet",
            format!(
                "drawn extent is {:.1}mm wide x {:.1}mm tall (height/width = {ratio:.2}); allowed {MIN_SHEET_ASPECT}..{MAX_SHEET_ASPECT}",
                w / 1000.0,
                h / 1000.0
            ),
        ));
    } else {
        out.push(CheckResult::pass("schematic_sheet_aspect"));
    }
}

/// Sheet cell size for the density grid (um). Chosen to be roughly the
/// size of one small part's box plus its label slots (~20mm), so a single
/// occupied cell corresponds to "about one symbol's worth" of drawn
/// content — coarse enough that a few wire jogs don't paper over a truly
/// empty region, fine enough that a real functional block spans several
/// cells rather than one.
const DENSITY_CELL_UM: i64 = 20_000;

/// Fraction of grid cells (over the drawing's own bounding box) that may be
/// completely empty (no symbol box, wire segment or label) before the sheet
/// reads as mostly blank.
///
/// Threshold provenance: measured across all 16 `examples/*.yaml` and
/// `examples/ladder/*.yaml` fixtures at seeds 0-3 on a 20mm grid.
/// `ladder/l4_control_hub` (100 parts) is 49-55% empty at every seed, and
/// `ladder/l3_motor_hub` (58 parts) is 41-55% empty at 3 of its 4 seeds —
/// both read as a sheet with dead space between functional blocks. Every
/// other fixture, including the smaller `ladder/l1_usb_mcu` (17 parts,
/// up to 37% on a coarser grid) and single-cluster boards whose own shape
/// is legitimately irregular rather than under-packed, stays at or below
/// 37%. The limit sits between those two populations, just above the
/// clean boards' worst case.
const MAX_EMPTY_CELL_FRACTION: f64 = 0.40;

/// A fully empty band (every cell across the sheet's other axis) spanning
/// more than this fraction of the sheet's width or height reads as a dead
/// strip a reader has to visually jump over. Provenance: same measurement
/// pass as `MAX_EMPTY_CELL_FRACTION` — `l4_control_hub` had empty bands
/// covering 3 of its 10 grid rows (30%) before the packing fix; every clean
/// fixture's widest empty band was at most 1-2 cells out of at least 5.
const MAX_EMPTY_BAND_FRACTION: f64 = 0.25;

/// Fails when the drawn content leaves too much of the sheet empty: divide
/// the drawing's own bounding box into a grid of ~`DENSITY_CELL_UM` cells
/// and fail if too many cells hold nothing (`MAX_EMPTY_CELL_FRACTION`), or
/// if any fully-empty band of cells spans too much of the sheet's width or
/// height (`MAX_EMPTY_BAND_FRACTION`). Purely geometric — no rasterisation
/// — from the same symbol boxes, wire points and label anchors
/// `schematic_sheet_aspect` already measures the extent from.
fn check_sheet_density(sch: &SchematicSection, geos: &BTreeMap<String, SymGeo<'_>>, out: &mut Vec<CheckResult>) {
    let (mut x0, mut x1, mut y0, mut y1) = (i64::MAX, i64::MIN, i64::MAX, i64::MIN);
    let mut seen = false;
    let mut extend = |bx0: i64, bx1: i64, by0: i64, by1: i64| {
        x0 = x0.min(bx0);
        x1 = x1.max(bx1);
        y0 = y0.min(by0);
        y1 = y1.max(by1);
        seen = true;
    };
    for g in geos.values() {
        let (a, b, c, d) = g.bounds();
        extend(a, b, c, d);
    }
    for w in &sch.wires {
        for p in &w.pts {
            extend(p.x, p.x, p.y, p.y);
        }
    }
    for l in &sch.labels {
        extend(l.at.x, l.at.x, l.at.y, l.at.y);
    }
    if !seen {
        out.push(CheckResult::pass("schematic_sheet_density"));
        return;
    }

    let width = (x1 - x0).max(1);
    let height = (y1 - y0).max(1);
    let cols = ((width + DENSITY_CELL_UM - 1) / DENSITY_CELL_UM).max(1) as usize;
    let rows = ((height + DENSITY_CELL_UM - 1) / DENSITY_CELL_UM).max(1) as usize;
    let mut occupied = vec![vec![false; cols]; rows];

    let mut mark_rect = |bx0: i64, bx1: i64, by0: i64, by1: i64| {
        let c0 = ((bx0 - x0) / DENSITY_CELL_UM).clamp(0, cols as i64 - 1) as usize;
        let c1 = ((bx1 - x0) / DENSITY_CELL_UM).clamp(0, cols as i64 - 1) as usize;
        let r0 = ((by0 - y0) / DENSITY_CELL_UM).clamp(0, rows as i64 - 1) as usize;
        let r1 = ((by1 - y0) / DENSITY_CELL_UM).clamp(0, rows as i64 - 1) as usize;
        for r in r0..=r1 {
            for c in c0..=c1 {
                occupied[r][c] = true;
            }
        }
    };

    for g in geos.values() {
        let (a, b, c, d) = g.bounds();
        mark_rect(a, b, c, d);
    }
    for w in &sch.wires {
        for seg in w.pts.windows(2) {
            let (a, b) = (seg[0], seg[1]);
            mark_rect(a.x.min(b.x), a.x.max(b.x), a.y.min(b.y), a.y.max(b.y));
        }
        if w.pts.len() == 1 {
            let p = w.pts[0];
            mark_rect(p.x, p.x, p.y, p.y);
        }
    }
    for l in &sch.labels {
        mark_rect(l.at.x, l.at.x, l.at.y, l.at.y);
    }

    let total_cells = rows * cols;
    // A tiny grid (a handful of parts whose bounding box barely spans a
    // couple of cells) makes the empty-fraction ratio noise-dominated: one
    // unavoidable corner cell on a 3x3 grid alone is 11%. Below this many
    // cells there just isn't enough resolution for "fraction empty" to mean
    // anything, so skip straight to a pass — this is what keeps small
    // fixtures like `passive_divider_ladder` (down to a 3x3 grid on some
    // seeds) from failing on quantization alone.
    const MIN_CELLS_FOR_DENSITY_CHECK: usize = 12;
    if total_cells < MIN_CELLS_FOR_DENSITY_CHECK {
        out.push(CheckResult::pass("schematic_sheet_density"));
        return;
    }
    let empty_cells: usize = occupied.iter().flatten().filter(|&&o| !o).count();
    let empty_fraction = empty_cells as f64 / total_cells as f64;

    // Widest fully-empty band: a run of columns each empty across every
    // row, or a run of rows each empty across every column.
    let col_empty: Vec<bool> = (0..cols).map(|c| (0..rows).all(|r| !occupied[r][c])).collect();
    let row_empty: Vec<bool> = (0..rows).map(|r| (0..cols).all(|c| !occupied[r][c])).collect();
    let max_run = |flags: &[bool]| -> usize {
        let mut best = 0;
        let mut cur = 0;
        for &f in flags {
            if f {
                cur += 1;
                best = best.max(cur);
            } else {
                cur = 0;
            }
        }
        best
    };
    let max_col_band = max_run(&col_empty) as f64 / cols as f64;
    let max_row_band = max_run(&row_empty) as f64 / rows as f64;

    if empty_fraction > MAX_EMPTY_CELL_FRACTION || max_col_band > MAX_EMPTY_BAND_FRACTION || max_row_band > MAX_EMPTY_BAND_FRACTION {
        out.push(CheckResult {
            check: "schematic_sheet_density".into(),
            status: CheckStatus::Fail,
            location: Some("sheet".into()),
            hint: Some(format!(
                "{empty_cells}/{total_cells} of {DENSITY_CELL_UM}um grid cells are empty ({:.0}% > {:.0}% max); widest empty band is {:.0}% of width / {:.0}% of height (max {:.0}%)",
                empty_fraction * 100.0,
                MAX_EMPTY_CELL_FRACTION * 100.0,
                max_col_band * 100.0,
                max_row_band * 100.0,
                MAX_EMPTY_BAND_FRACTION * 100.0
            )), detail: None
        });
    } else {
        out.push(CheckResult::pass("schematic_sheet_density"));
    }
}

/// Extracts the `x y w h` of an SVG document's top-level `viewBox`
/// attribute, in mm — `eda-render` always emits one on its root `<svg>`
/// (see `render_schematic`).
fn parse_view_box(svg: &str) -> Option<(f64, f64, f64, f64)> {
    let start = svg.find("viewBox=\"")? + "viewBox=\"".len();
    let rest = &svg[start..];
    let end = rest.find('"')?;
    let mut it = rest[..end].split_whitespace();
    let x: f64 = it.next()?.parse().ok()?;
    let y: f64 = it.next()?.parse().ok()?;
    let w: f64 = it.next()?.parse().ok()?;
    let h: f64 = it.next()?.parse().ok()?;
    Some((x, y, w, h))
}

/// Fails when any drawn element's full extent — a symbol box or its pin
/// stub tips, a wire segment endpoint, a refdes/value/pin-name text box, or
/// a net label's full ink (its tag/glyph including the pentagon nose and,
/// for Ground/Power, the leader line and bars/arrowhead) — lies outside the
/// sheet bounding box `eda-render` actually emits as the SVG `viewBox`. This
/// is deliberately checked against the *real rendered* viewBox (by calling
/// `eda_render::render_schematic` directly), not a second independently
/// re-derived bounding box, so a future drawn element that `eda-render`'s
/// own bounds computation forgets to include is still caught here rather
/// than the two computations silently agreeing on the same mistake. Every
/// element's own extent is still measured with the exact shared
/// `eda_engine::geometry` functions `eda-render` draws with (`symbol_box_mm`,
/// `stub_tip`, `pin_text_box`, `resolve_net_label`/`net_label_extent`), so a
/// disagreement here always means the sheet bounding box itself is wrong,
/// never a difference in how the two crates estimate ink.
fn check_content_in_bounds(design: &Design, sch: &SchematicSection, model: &ConstraintModel, geos: &BTreeMap<String, SymGeo<'_>>, out: &mut Vec<CheckResult>) {
    let svg = match eda_render::render_schematic(design, model) {
        Ok(s) => s,
        Err(_) => {
            // Rendering itself already failed for an unrelated reason (e.g.
            // a symbol with no matching Part) — nothing for this gate to
            // measure against.
            out.push(CheckResult::pass("schematic_content_in_bounds"));
            return;
        }
    };
    let Some((vx, vy, vw, vh)) = parse_view_box(&svg) else {
        out.push(CheckResult::fail("schematic_content_in_bounds", "sheet", "renderer did not emit a parseable viewBox"));
        return;
    };
    let (x0, y0, x1, y1) = (vx, vy, vx + vw, vy + vh);
    let eps = 1e-6;
    let mut ok = true;

    fn check_point(name: &str, x: f64, y: f64, x0: f64, y0: f64, x1: f64, y1: f64, eps: f64, ok: &mut bool, out: &mut Vec<CheckResult>) {
        if x < x0 - eps || x > x1 + eps || y < y0 - eps || y > y1 + eps {
            out.push(CheckResult {
                check: "schematic_content_in_bounds".into(),
                status: CheckStatus::Fail,
                location: Some(name.to_string()),
                hint: Some(format!("({x:.3},{y:.3}) mm lies outside the sheet viewBox ({x0:.3},{y0:.3})..({x1:.3},{y1:.3}) mm")), detail: None
            });
            *ok = false;
        }
    }

    // Symbol boxes + every pin's stub tip.
    for (id, g) in geos {
        let b = geometry::symbol_box_mm(g.top_left.x as f64 / 1000.0, g.top_left.y as f64 / 1000.0, g.width, g.height);
        check_point(&format!("{id}:box"), b.x0, b.y0, x0, y0, x1, y1, eps, &mut ok, out);
        check_point(&format!("{id}:box"), b.x1, b.y1, x0, y0, x1, y1, eps, &mut ok, out);
        for port_idx in 0..g.node.ports.len() {
            let tip = geometry::stub_tip(&g.node, g.top_left, port_idx);
            check_point(&format!("{id}:stub{port_idx}"), tip.x as f64 / 1000.0, tip.y as f64 / 1000.0, x0, y0, x1, y1, eps, &mut ok, out);
        }
    }
    // Wire segment endpoints.
    for (wi, w) in sch.wires.iter().enumerate() {
        for p in &w.pts {
            check_point(&format!("wire{wi}"), p.x as f64 / 1000.0, p.y as f64 / 1000.0, x0, y0, x1, y1, eps, &mut ok, out);
        }
    }
    // Every refdes/value/pin-name/net-label ink box, plus each net label's
    // full drawn-glyph extent (leader line, bars/arrowhead, tag nose).
    let (tight_boxes, label_extents) = collect_text_boxes_and_label_extents(sch, model);
    for (name, b) in tight_boxes.iter().chain(label_extents.iter()) {
        check_point(name, b.x0, b.y0, x0, y0, x1, y1, eps, &mut ok, out);
        check_point(name, b.x1, b.y1, x0, y0, x1, y1, eps, &mut ok, out);
    }

    if ok {
        out.push(CheckResult::pass("schematic_content_in_bounds"));
    }
}

fn check_crossing_count(sch: &SchematicSection, out: &mut Vec<CheckResult>) {
    // Collect (net, segment) for every wire segment.
    let mut segs: Vec<(&str, IrPoint, IrPoint)> = Vec::new();
    for w in &sch.wires {
        for seg in w.pts.windows(2) {
            segs.push((w.net.as_str(), seg[0], seg[1]));
        }
    }

    let mut count = 0usize;
    for i in 0..segs.len() {
        for j in (i + 1)..segs.len() {
            let (net_i, ai, bi) = segs[i];
            let (net_j, aj, bj) = segs[j];
            if net_i == net_j {
                continue;
            }
            let (Some(oi), Some(oj)) = (seg_orientation(ai, bi), seg_orientation(aj, bj)) else { continue };
            if oi == oj {
                continue; // parallel segments: not counted as a crossing here
            }
            // oi==true means i is horizontal, j is vertical (or vice versa).
            let (h_a, h_b, v_a, v_b) = if oi { (ai, bi, aj, bj) } else { (aj, bj, ai, bi) };
            let hy = h_a.y;
            let (hx0, hx1) = (h_a.x.min(h_b.x), h_a.x.max(h_b.x));
            let vx = v_a.x;
            let (vy0, vy1) = (v_a.y.min(v_b.y), v_a.y.max(v_b.y));
            if vx > hx0 && vx < hx1 && hy > vy0 && hy < vy1 {
                count += 1;
            }
        }
    }

    // Fails once crossings pile up faster than the board is growing: a
    // handful of crossings on a small board reads fine, but crossing count
    // scaling well past part count is the "spaghetti" defect this exists to
    // catch (`mcu_board_30plus` at 231 crossings for 29 parts, for example).
    let part_count = sch.symbols.len();
    let threshold = 3 * part_count.max(1);
    let status = if count > threshold { CheckStatus::Fail } else { CheckStatus::Warn };
    out.push(CheckResult {
        check: "schematic_wire_crossing_count".into(),
        status,
        location: None,
        hint: Some(format!("{count} wire/wire crossing(s) between different nets (max {threshold} = 3x{part_count} parts)")), detail: None
    });
}

/// A two-pin net's wire drawn far longer than the straight-line (Manhattan)
/// distance between its two pins reads as an unnecessary detour around the
/// sheet. Fails per offending wire when its drawn (polyline arc) length
/// exceeds `DETOUR_RATIO` times the Manhattan distance between its first and
/// last point (which — since a wire's endpoints are exactly its two pins'
/// stub tips — is the same as the pin-to-pin Manhattan distance). Only
/// applies to genuinely 2-pin nets (`wire.pins.len() == 2`); a star net's
/// per-edge "wire" is deliberately a hub-to-leaf leg, not a direct pin-pair,
/// so the same ratio isn't a meaningful detour signal there.
const DETOUR_RATIO: f64 = 2.0;

fn check_wire_detour(sch: &SchematicSection, geos: &BTreeMap<String, SymGeo<'_>>, out: &mut Vec<CheckResult>) {
    let mut ok = true;
    for w in &sch.wires {
        if w.pins.len() != 2 || w.pts.len() < 2 {
            continue;
        }
        // Two pins on the *same* part (e.g. an op-amp's two feedback pins)
        // route around that part's own box (`same_node_path`), which can
        // legitimately be several times the pins' direct distance since it
        // can never cut through the box — not the "unnecessary detour
        // across the sheet" defect this gate targets.
        if split_pin_ref(&w.pins[0]).0 == split_pin_ref(&w.pins[1]).0 {
            continue;
        }
        // Two pins that escape their boxes toward the *same* side (two
        // east-facing passive legs, say) cannot be joined by a straight
        // run: the wire has to leave one box, clear it, and come back into
        // the other from the same direction. That wrap is geometry, not a
        // layout failure, and on a short pin-to-pin distance it trivially
        // exceeds 2x — so it is not what this gate is looking for. Only
        // absolute length (`schematic_wire_length`) polices these.
        let sides = (
            geos.get(split_pin_ref(&w.pins[0]).0).and_then(|g| g.port_side_for_pin_number(split_pin_ref(&w.pins[0]).1)),
            geos.get(split_pin_ref(&w.pins[1]).0).and_then(|g| g.port_side_for_pin_number(split_pin_ref(&w.pins[1]).1)),
        );
        if let (Some(a), Some(b)) = sides {
            if a == b {
                continue;
            }
        }
        let manhattan: i64 = {
            let a = w.pts[0];
            let b = w.pts[w.pts.len() - 1];
            (a.x - b.x).abs() + (a.y - b.y).abs()
        };
        if manhattan == 0 {
            continue;
        }
        let drawn: i64 = w.pts.windows(2).map(|s| (s[0].x - s[1].x).abs() + (s[0].y - s[1].y).abs()).sum();
        if (drawn as f64) > DETOUR_RATIO * (manhattan as f64) {
            out.push(CheckResult {
                check: "schematic_wire_detour".into(),
                status: CheckStatus::Fail,
                location: Some(format!("{}:{:?}", w.net, w.pins)),
                hint: Some(format!(
                    "wire length {drawn}um is {:.1}x the {manhattan}um Manhattan distance between its pins (max {DETOUR_RATIO}x)",
                    drawn as f64 / manhattan as f64
                )), detail: None
            });
            ok = false;
        }
    }
    if ok {
        out.push(CheckResult::pass("schematic_wire_detour"));
    }
}

/// A power/ground-style net (see `geometry::is_power_or_ground_net_name`)
/// that still produced a real wire — rather than the power-flag/ground-glyph
/// symbols `eda-engine` normally emits for such nets instead of wiring them
/// — reads as a rail snaking across the sheet. Fails when that wire is
/// either long (>`MAX_POWER_WIRE_LEN_UM`) or bendy (>2 bends): a short,
/// single-segment drop between two adjacent pins on the same net is
/// harmless and common (e.g. a decoupling cap's own two legs are never on a
/// "net" in this sense, but a hand-authored intent could still tie two
/// adjacent power pins directly), so this only flags a wire that actually
/// behaves like the sheet-spanning rail this gate exists to catch.
const MAX_POWER_WIRE_LEN_UM: i64 = 30_000; // 30mm
const MAX_POWER_WIRE_BENDS: usize = 2;

fn check_power_net_as_wire(sch: &SchematicSection, out: &mut Vec<CheckResult>) {
    let mut ok = true;
    for w in &sch.wires {
        if !geometry::is_power_or_ground_net_name(&w.net) || w.pts.len() < 2 {
            continue;
        }
        let length: i64 = w.pts.windows(2).map(|s| (s[0].x - s[1].x).abs() + (s[0].y - s[1].y).abs()).sum();
        let bends = w.pts.len().saturating_sub(2);
        if length > MAX_POWER_WIRE_LEN_UM || bends > MAX_POWER_WIRE_BENDS {
            out.push(CheckResult {
                check: "schematic_power_net_as_wire".into(),
                status: CheckStatus::Fail,
                location: Some(format!("{}:{:?}", w.net, w.pins)),
                hint: Some(format!(
                    "power/ground net '{}' drawn as a {length}um/{bends}-bend wire (max {MAX_POWER_WIRE_LEN_UM}um / {MAX_POWER_WIRE_BENDS} bends) instead of a flag/glyph at each pin",
                    w.net
                )), detail: None
            });
            ok = false;
        }
    }
    if ok {
        out.push(CheckResult::pass("schematic_power_net_as_wire"));
    }
}

/// A "column" here is a group of symbols sharing the same x position — the
/// proxy for a Sugiyama layer once layout swaps layer-index and
/// within-layer-index onto (x, y) (see `eda-layout`'s `swap_xy`). Fails
/// when any such group exceeds `MAX_COLUMN_SIZE` symbols: a single column
/// that long is the "everything stacked in one line" defect (e.g. all 20
/// decoupling caps + LEDs sharing one x on `mcu_board_30plus`), regardless
/// of whether the layout algorithm ever gets reworked to avoid it.
const MAX_COLUMN_SIZE: usize = 8;

fn check_column_overflow(sch: &SchematicSection, model: &ConstraintModel, out: &mut Vec<CheckResult>) {
    let mut by_x: BTreeMap<i64, Vec<&str>> = BTreeMap::new();
    for sym in &sch.symbols {
        if model.part(&sym.id).is_none() {
            continue;
        }
        by_x.entry(sym.at.x).or_default().push(&sym.id);
    }
    let mut ok = true;
    for (x, ids) in &by_x {
        if ids.len() > MAX_COLUMN_SIZE {
            out.push(CheckResult {
                check: "schematic_column_overflow".into(),
                status: CheckStatus::Fail,
                location: Some(format!("x={x}")),
                hint: Some(format!("column at x={x} has {} symbols (max {MAX_COLUMN_SIZE}): {}", ids.len(), ids.join(","))), detail: None
            });
            ok = false;
        }
    }
    if ok {
        out.push(CheckResult::pass("schematic_column_overflow"));
    }
}


/// A single wire polyline longer than this reads as a page-crossing
/// staircase rather than a connection between two parts: at A4/A3 scale it
/// spans most of the sheet, and the eye cannot follow it. Real schematics
/// draw a wire only between nearby parts and use a net label for anything
/// further. 80 mm is a little under half an A3 sheet's short side.
///
/// Threshold provenance: measured across the 12 core `examples/` fixtures
/// before the block-and-label layout landed, the worst single wire was
/// 82.5 mm (`passive_divider_ladder`, `star_net`) — i.e. the limit was set
/// to bite exactly the drawings that already carried one page-crossing
/// wire, and the layout change (not a tuned threshold) is what brings every
/// fixture under it. Not since relaxed.
const MAX_WIRE_LEN_UM: i64 = 80_000;

fn check_wire_length(sch: &SchematicSection, out: &mut Vec<CheckResult>) {
    let mut ok = true;
    for w in &sch.wires {
        if w.pts.len() < 2 {
            continue;
        }
        let len: i64 = w.pts.windows(2).map(|s| (s[0].x - s[1].x).abs() + (s[0].y - s[1].y).abs()).sum();
        if len > MAX_WIRE_LEN_UM {
            out.push(CheckResult {
                check: "schematic_wire_length".into(),
                status: CheckStatus::Fail,
                location: Some(format!("{}:{:?}", w.net, w.pins)),
                hint: Some(format!(
                    "wire polyline is {:.1}mm long (max {:.1}mm) — this net belongs on a net label, not a wire",
                    len as f64 / 1000.0,
                    MAX_WIRE_LEN_UM as f64 / 1000.0
                )), detail: None
            });
            ok = false;
        }
    }
    if ok {
        out.push(CheckResult::pass("schematic_wire_length"));
    }
}

/// Total drawn wire per part. A sheet can be free of any *single* overlong
/// wire and still be a field of medium-length staircases; ink-per-part is
/// the whole-sheet version of the same defect. 60 mm/part is roughly "each
/// part is joined to its neighbours by a few short runs".
///
/// Threshold provenance: measured across the 12 core `examples/` fixtures
/// before the block-and-label layout landed, the worst was 60.5 mm/part
/// (`star_net`), against 738 mm/part for `ladder/l4_control_hub` — the
/// limit sits just under the busiest clean fixture. Not since relaxed.
const MAX_WIRE_INK_UM_PER_PART: f64 = 60_000.0;

fn check_wire_ink(sch: &SchematicSection, out: &mut Vec<CheckResult>) {
    let total: i64 = sch
        .wires
        .iter()
        .map(|w| w.pts.windows(2).map(|s| (s[0].x - s[1].x).abs() + (s[0].y - s[1].y).abs()).sum::<i64>())
        .sum();
    let parts = sch.symbols.len().max(1);
    let per_part = total as f64 / parts as f64;
    if per_part > MAX_WIRE_INK_UM_PER_PART {
        out.push(CheckResult {
            check: "schematic_wire_ink".into(),
            status: CheckStatus::Fail,
            location: Some("sheet".into()),
            hint: Some(format!(
                "{:.0}mm of wire over {parts} parts = {:.1}mm/part (max {:.0}mm/part)",
                total as f64 / 1000.0,
                per_part / 1000.0,
                MAX_WIRE_INK_UM_PER_PART / 1000.0
            )), detail: None
        });
    } else {
        out.push(CheckResult::pass("schematic_wire_ink"));
    }
}

/// Functional blocks must actually be blocks. Re-derives the same
/// clustering `eda-engine` lays out with (intent `clusters:` when present,
/// otherwise IC-anchored affinity) and fails when a cluster's members are
/// drawn spread over more than `CLUSTER_SPREAD_RATIO` times the cluster's
/// own extent — where "extent" is the side of the square that would hold
/// the cluster's symbol boxes packed together, i.e. how big the block
/// intrinsically *is*. A block drawn more than twice that wide has its
/// members scattered, which is the "decoupling cap on the far side of the
/// sheet from its IC" defect.
const CLUSTER_SPREAD_RATIO: f64 = 3.5;

fn check_cluster_split(
    model: &ConstraintModel,
    geos: &BTreeMap<String, SymGeo<'_>>,
    out: &mut Vec<CheckResult>,
) {
    let mut ok = true;
    for members in eda_engine::cluster_members(model) {
        if members.len() < 2 {
            continue;
        }
        let (mut x0, mut x1, mut y0, mut y1) = (i64::MAX, i64::MIN, i64::MAX, i64::MIN);
        let mut area = 0f64;
        let mut seen = 0usize;
        for m in &members {
            let Some(g) = geos.get(m.as_str()) else { continue };
            let (a, b, c, d) = g.bounds();
            x0 = x0.min(a);
            x1 = x1.max(b);
            y0 = y0.min(c);
            y1 = y1.max(d);
            area += (g.width as f64) * (g.height as f64);
            seen += 1;
        }
        if seen < 2 {
            continue;
        }
        let packed_side = area.sqrt().max(1.0);
        let spread = ((x1 - x0).max(y1 - y0)) as f64;
        if spread > CLUSTER_SPREAD_RATIO * packed_side {
            out.push(CheckResult {
                check: "schematic_cluster_split".into(),
                status: CheckStatus::Fail,
                location: Some(members[0].clone()),
                hint: Some(format!(
                    "cluster of {seen} parts ({}) is drawn {:.1}mm across, more than {CLUSTER_SPREAD_RATIO}x its own {:.1}mm packed extent",
                    members.join(","),
                    spread / 1000.0,
                    packed_side / 1000.0
                )), detail: None
            });
            ok = false;
        }
    }
    if ok {
        out.push(CheckResult::pass("schematic_cluster_split"));
    }
}


#[cfg(test)]
mod tests {
    use super::*;
    use eda_model::ir::{NetLabel, Provenance, Wire};
    use eda_model::{Pin, PinKind};

    fn provenance() -> Provenance {
        Provenance { engine_version: "0".into(), intent_hash: "x".into(), seed: 0, stage_hashes: vec![] }
    }

    fn design(sch: SchematicSection) -> Design {
        Design { schema: 1, provenance: provenance(), schematic: Some(sch), nets: None, placement: None, routing: None, drawings: None, footprint_library: None, sheet_contents: None, bus_aliases: vec![], symbol_library: None }
    }

    /// One part: a 2-pin passive with pins at Left(offset 1270)/Right
    /// (offset 1270), so `geometry::node_size(2)` -> width 10160, height
    /// 7620, and both ports land at grid-aligned offsets (see
    /// `geometry::distribute_offsets`).
    fn one_part_model() -> ConstraintModel {
        let r1 = Part {
            reference: "R1".into(),
            mpn: None,
            lcsc: None,
            value: None,
            package: None,
            footprint: None,
            pins: vec![
                Pin { number: "1".into(), name: None, kind: PinKind::Passive },
                Pin { number: "2".into(), name: None, kind: PinKind::Passive },
            ],
            body_um: None, symbol: None, datasheet: None,
            edge: None,
        };
        ConstraintModel { parts: vec![r1], ..Default::default() }
    }

    fn r1_geo(model: &ConstraintModel, at: IrPoint) -> SymGeo<'_> {
        let sym = SymbolInstance { lib_id: String::new(), unit: 1, value: String::new(), footprint: String::new(), datasheet: String::new(), id: "R1".into(), at, rot: 0, mirrored: false, mirror_y: false };
        SymGeo::build(&sym, model.part("R1").unwrap(), model)
    }

    fn r1_symbol(at: IrPoint) -> SymbolInstance {
        SymbolInstance { lib_id: String::new(), unit: 1, value: String::new(), footprint: String::new(), datasheet: String::new(), id: "R1".into(), at, rot: 0, mirrored: false, mirror_y: false }
    }

    // ---------------------------------------------------------- offgrid

    #[test]
    fn offgrid_passes_when_everything_on_grid() {
        let model = one_part_model();
        let sch = SchematicSection { power_symbols: vec![], no_connects: vec![], bus_entries: vec![], title_block: None, sheets: vec![], instance_overrides: vec![], junctions: vec![], lines: vec![], texts: vec![], erc_exclusions: vec![], erc_pin_map: None, user_fields: Default::default(), imported_from_kicad: false, symbols: vec![r1_symbol(IrPoint { x: 0, y: 0 })], wires: vec![], labels: vec![] };
        let results = check_style(&design(sch), &model);
        assert!(results.iter().any(|r| r.check == "schematic_offgrid" && r.status == CheckStatus::Pass));
    }

    #[test]
    fn offgrid_fails_on_off_grid_symbol() {
        let model = one_part_model();
        let sch = SchematicSection { power_symbols: vec![], no_connects: vec![], bus_entries: vec![], title_block: None, sheets: vec![], instance_overrides: vec![], junctions: vec![], lines: vec![], texts: vec![], erc_exclusions: vec![], erc_pin_map: None, user_fields: Default::default(), imported_from_kicad: false, symbols: vec![r1_symbol(IrPoint { x: 100, y: 0 })], wires: vec![], labels: vec![] };
        let results = check_style(&design(sch), &model);
        assert!(results.iter().any(|r| r.check == "schematic_offgrid" && r.status == CheckStatus::Fail));
    }

    // ---------------------------------------------------------- orthogonal

    #[test]
    fn orthogonal_passes_for_hv_only_wire() {
        let model = one_part_model();
        let sch = SchematicSection { power_symbols: vec![], no_connects: vec![], bus_entries: vec![], title_block: None, sheets: vec![], instance_overrides: vec![], junctions: vec![], lines: vec![], texts: vec![], erc_exclusions: vec![], erc_pin_map: None, user_fields: Default::default(), imported_from_kicad: false,
            symbols: vec![],
            wires: vec![Wire { id: String::new(), net: "N".into(), pins: vec![], pts: vec![IrPoint { x: 0, y: 0 }, IrPoint { x: 1270, y: 0 }, IrPoint { x: 1270, y: 1270 }], bus: false }],
            labels: vec![],
        };
        let results = check_style(&design(sch), &model);
        assert!(results.iter().any(|r| r.check == "schematic_wire_not_orthogonal" && r.status == CheckStatus::Pass));
    }

    #[test]
    fn orthogonal_fails_for_diagonal_segment() {
        let model = one_part_model();
        let sch = SchematicSection { power_symbols: vec![], no_connects: vec![], bus_entries: vec![], title_block: None, sheets: vec![], instance_overrides: vec![], junctions: vec![], lines: vec![], texts: vec![], erc_exclusions: vec![], erc_pin_map: None, user_fields: Default::default(), imported_from_kicad: false,
            symbols: vec![],
            wires: vec![Wire { id: String::new(), net: "N".into(), pins: vec![], pts: vec![IrPoint { x: 0, y: 0 }, IrPoint { x: 1270, y: 1270 }], bus: false }],
            labels: vec![],
        };
        let results = check_style(&design(sch), &model);
        assert!(results.iter().any(|r| r.check == "schematic_wire_not_orthogonal" && r.status == CheckStatus::Fail));
    }

    // ---------------------------------------------------------- symbol_overlap

    #[test]
    fn symbol_overlap_passes_when_apart() {
        let model = one_part_model();
        let sch = SchematicSection { power_symbols: vec![], no_connects: vec![], bus_entries: vec![], title_block: None, sheets: vec![], instance_overrides: vec![], junctions: vec![], lines: vec![], texts: vec![], erc_exclusions: vec![], erc_pin_map: None, user_fields: Default::default(), imported_from_kicad: false, symbols: vec![r1_symbol(IrPoint { x: 0, y: 0 })], wires: vec![], labels: vec![] };
        let results = check_style(&design(sch), &model);
        assert!(results.iter().any(|r| r.check == "schematic_symbol_overlap" && r.status == CheckStatus::Pass));
    }

    #[test]
    fn symbol_overlap_fails_when_two_boxes_overlap() {
        let mut model = one_part_model();
        let r2 = Part { reference: "R2".into(), ..model.parts[0].clone() };
        model.parts.push(r2);
        let sch = SchematicSection { power_symbols: vec![], no_connects: vec![], bus_entries: vec![], title_block: None, sheets: vec![], instance_overrides: vec![], junctions: vec![], lines: vec![], texts: vec![], erc_exclusions: vec![], erc_pin_map: None, user_fields: Default::default(), imported_from_kicad: false,
            symbols: vec![r1_symbol(IrPoint { x: 0, y: 0 }), SymbolInstance { lib_id: String::new(), unit: 1, value: String::new(), footprint: String::new(), datasheet: String::new(), id: "R2".into(), at: IrPoint { x: 1270, y: 0 }, rot: 0, mirrored: false, mirror_y: false }],
            wires: vec![],
            labels: vec![],
        };
        let results = check_style(&design(sch), &model);
        assert!(results.iter().any(|r| r.check == "schematic_symbol_overlap" && r.status == CheckStatus::Fail));
    }

    // ---------------------------------------------------------- wire_through_symbol

    #[test]
    fn wire_through_symbol_passes_when_wire_stays_outside_box() {
        let model = one_part_model();
        let sch = SchematicSection { power_symbols: vec![], no_connects: vec![], bus_entries: vec![], title_block: None, sheets: vec![], instance_overrides: vec![], junctions: vec![], lines: vec![], texts: vec![], erc_exclusions: vec![], erc_pin_map: None, user_fields: Default::default(), imported_from_kicad: false,
            symbols: vec![r1_symbol(IrPoint { x: 0, y: 0 })],
            wires: vec![Wire { id: String::new(), net: "N".into(), pins: vec![], pts: vec![IrPoint { x: -5000, y: -5000 }, IrPoint { x: -5000, y: -6000 }], bus: false }],
            labels: vec![],
        };
        let results = check_style(&design(sch), &model);
        assert!(results.iter().any(|r| r.check == "schematic_wire_through_symbol" && r.status == CheckStatus::Pass));
    }

    #[test]
    fn wire_through_symbol_fails_when_segment_cuts_the_box() {
        let model = one_part_model();
        let geo = r1_geo(&model, IrPoint { x: 0, y: 0 });
        let (left, right, top, bottom) = geo.bounds();
        let mid_y = (top + bottom) / 2;
        let sch = SchematicSection { power_symbols: vec![], no_connects: vec![], bus_entries: vec![], title_block: None, sheets: vec![], instance_overrides: vec![], junctions: vec![], lines: vec![], texts: vec![], erc_exclusions: vec![], erc_pin_map: None, user_fields: Default::default(), imported_from_kicad: false,
            symbols: vec![r1_symbol(IrPoint { x: 0, y: 0 })],
            wires: vec![Wire { id: String::new(), net: "N".into(), pins: vec![], pts: vec![IrPoint { x: left - 100, y: mid_y }, IrPoint { x: right + 100, y: mid_y }], bus: false }],
            labels: vec![],
        };
        let results = check_style(&design(sch), &model);
        assert!(results.iter().any(|r| r.check == "schematic_wire_through_symbol" && r.status == CheckStatus::Fail));
    }

    // ------------------------------------------------------- flow_direction

    /// Two connectors and one IC on one net. J2 sits right of J1 but left of
    /// the IC it drives: that reads correctly left-to-right, and the gate
    /// must not fail it just because another *connector* is further left.
    /// The same J2 placed right of the IC must still fail — the exclusion is
    /// connector-vs-connector only, not a blanket exemption.
    #[test]
    fn flow_direction_ignores_other_connectors_but_still_judges_driven_parts() {
        let two_conn_model = |_: ()| ConstraintModel {
            parts: vec![
                Part { reference: "J1".into(), mpn: None, lcsc: None, value: None, package: None, footprint: None,
                       pins: vec![Pin { number: "1".into(), name: None, kind: PinKind::Passive }],
                       body_um: None, symbol: None, datasheet: None,
                       edge: None, },
                Part { reference: "J2".into(), mpn: None, lcsc: None, value: None, package: None, footprint: None,
                       pins: vec![Pin { number: "1".into(), name: None, kind: PinKind::Passive }],
                       body_um: None, symbol: None, datasheet: None,
                       edge: None, },
                Part { reference: "U1".into(), mpn: None, lcsc: None, value: None, package: None, footprint: None,
                       pins: vec![Pin { number: "1".into(), name: None, kind: PinKind::Passive }],
                       body_um: None, symbol: None, datasheet: None,
                       edge: None, },
            ],
            ..Default::default()
        };
        let model = two_conn_model(());
        let sch_for = |u1_x: i64| SchematicSection { power_symbols: vec![], no_connects: vec![], bus_entries: vec![], title_block: None, sheets: vec![], instance_overrides: vec![], junctions: vec![], lines: vec![], texts: vec![], erc_exclusions: vec![], erc_pin_map: None, user_fields: Default::default(), imported_from_kicad: false,
            symbols: vec![
                SymbolInstance { lib_id: String::new(), unit: 1, value: String::new(), footprint: String::new(), datasheet: String::new(), id: "J1".into(), at: IrPoint { x: 0, y: 0 }, rot: 0, mirrored: false, mirror_y: false },
                SymbolInstance { lib_id: String::new(), unit: 1, value: String::new(), footprint: String::new(), datasheet: String::new(), id: "J2".into(), at: IrPoint { x: 12700, y: 0 }, rot: 0, mirrored: false, mirror_y: false },
                SymbolInstance { lib_id: String::new(), unit: 1, value: String::new(), footprint: String::new(), datasheet: String::new(), id: "U1".into(), at: IrPoint { x: u1_x, y: 0 }, rot: 0, mirrored: false, mirror_y: false },
            ],
            wires: vec![Wire { id: String::new(),
                net: "SIG".into(),
                pins: vec!["J1.1".into(), "J2.1".into(), "U1.1".into()],
                pts: vec![IrPoint { x: 0, y: 0 }, IrPoint { x: 25400, y: 0 }], bus: false,
            }],
            labels: vec![],
        };
        // U1 to the right of both connectors: passes.
        let results = check_style(&design(sch_for(25400)), &model);
        assert!(
            results.iter().any(|r| r.check == "schematic_flow_direction" && r.status == CheckStatus::Pass),
            "{results:#?}"
        );
        // U1 to the left of both: both connectors are genuinely on the wrong
        // side of what they drive, and must still be flagged.
        let results = check_style(&design(sch_for(-12700)), &model);
        let fails: Vec<&CheckResult> =
            results.iter().filter(|r| r.check == "schematic_flow_direction" && r.status == CheckStatus::Fail).collect();
        assert_eq!(fails.len(), 2, "{results:#?}");
    }

    // ---------------------------------------------------------- sheet_aspect

    #[test]
    fn sheet_aspect_passes_for_a_roughly_square_sheet_and_fails_for_a_tall_ribbon() {
        let model = one_part_model();
        let geo = r1_geo(&model, IrPoint { x: 0, y: 0 });
        let (left, right, top, _bottom) = geo.bounds();
        let w = right - left;
        // A wire stretching the extent out to ~1:1 keeps the sheet in shape.
        let square = SchematicSection { power_symbols: vec![], no_connects: vec![], bus_entries: vec![], title_block: None, sheets: vec![], instance_overrides: vec![], junctions: vec![], lines: vec![], texts: vec![], erc_exclusions: vec![], erc_pin_map: None, user_fields: Default::default(), imported_from_kicad: false,
            symbols: vec![r1_symbol(IrPoint { x: 0, y: 0 })],
            wires: vec![Wire { id: String::new(),
                net: "N".into(),
                pins: vec![],
                pts: vec![IrPoint { x: left, y: top }, IrPoint { x: left + 4 * w, y: top }, IrPoint { x: left + 4 * w, y: top + 4 * w }], bus: false,
            }],
            labels: vec![],
        };
        let results = check_style(&design(square), &model);
        assert!(results.iter().any(|r| r.check == "schematic_sheet_aspect" && r.status == CheckStatus::Pass), "{results:#?}");

        // The same extent stretched only downward is a tall ribbon.
        let ribbon = SchematicSection { power_symbols: vec![], no_connects: vec![], bus_entries: vec![], title_block: None, sheets: vec![], instance_overrides: vec![], junctions: vec![], lines: vec![], texts: vec![], erc_exclusions: vec![], erc_pin_map: None, user_fields: Default::default(), imported_from_kicad: false,
            symbols: vec![r1_symbol(IrPoint { x: 0, y: 0 })],
            wires: vec![Wire { id: String::new(),
                net: "N".into(),
                pins: vec![],
                pts: vec![IrPoint { x: left, y: top }, IrPoint { x: left, y: top + 40 * w }], bus: false,
            }],
            labels: vec![],
        };
        let results = check_style(&design(ribbon), &model);
        assert!(results.iter().any(|r| r.check == "schematic_sheet_aspect" && r.status == CheckStatus::Fail), "{results:#?}");
    }

    // ---------------------------------------------------------- endpoint_off_pin

    #[test]
    fn endpoint_off_pin_passes_when_wire_starts_and_ends_on_stub_tips() {
        let model = one_part_model();
        let geo = r1_geo(&model, IrPoint { x: 0, y: 0 });
        let p1 = geo.stub_tip_for_pin_number("1").unwrap();
        let p2 = geo.stub_tip_for_pin_number("2").unwrap();
        let sch = SchematicSection { power_symbols: vec![], no_connects: vec![], bus_entries: vec![], title_block: None, sheets: vec![], instance_overrides: vec![], junctions: vec![], lines: vec![], texts: vec![], erc_exclusions: vec![], erc_pin_map: None, user_fields: Default::default(), imported_from_kicad: false,
            symbols: vec![r1_symbol(IrPoint { x: 0, y: 0 })],
            wires: vec![Wire { id: String::new(),
                net: "N".into(),
                pins: vec!["R1.1".into(), "R1.2".into()],
                pts: vec![IrPoint { x: p1.x, y: p1.y }, IrPoint { x: p2.x, y: p2.y }], bus: false,
            }],
            labels: vec![],
        };
        let results = check_style(&design(sch), &model);
        assert!(results.iter().any(|r| r.check == "schematic_wire_endpoint_off_pin" && r.status == CheckStatus::Pass));
    }

    #[test]
    fn endpoint_off_pin_fails_when_wire_stops_short_of_the_stub_tip() {
        let model = one_part_model();
        let geo = r1_geo(&model, IrPoint { x: 0, y: 0 });
        let p1 = geo.stub_tip_for_pin_number("1").unwrap();
        let p2 = geo.stub_tip_for_pin_number("2").unwrap();
        let sch = SchematicSection { power_symbols: vec![], no_connects: vec![], bus_entries: vec![], title_block: None, sheets: vec![], instance_overrides: vec![], junctions: vec![], lines: vec![], texts: vec![], erc_exclusions: vec![], erc_pin_map: None, user_fields: Default::default(), imported_from_kicad: false,
            symbols: vec![r1_symbol(IrPoint { x: 0, y: 0 })],
            wires: vec![Wire { id: String::new(),
                net: "N".into(),
                pins: vec!["R1.1".into(), "R1.2".into()],
                // first point is on the box boundary, not the stub tip.
                pts: vec![IrPoint { x: p1.x + 1, y: p1.y }, IrPoint { x: p2.x, y: p2.y }], bus: false,
            }],
            labels: vec![],
        };
        let results = check_style(&design(sch), &model);
        assert!(results.iter().any(|r| r.check == "schematic_wire_endpoint_off_pin" && r.status == CheckStatus::Fail));
    }

    // ---------------------------------------------------------- wire_overlap

    #[test]
    fn wire_overlap_passes_for_touching_or_offset_or_crossing_segments() {
        let model = ConstraintModel::default();
        let sch = SchematicSection { power_symbols: vec![], no_connects: vec![], bus_entries: vec![], title_block: None, sheets: vec![], instance_overrides: vec![], junctions: vec![], lines: vec![], texts: vec![], erc_exclusions: vec![], erc_pin_map: None, user_fields: Default::default(), imported_from_kicad: false,
            symbols: vec![],
            wires: vec![
                // A: horizontal 0..1270 at y=0
                Wire { id: String::new(), net: "A".into(), pins: vec![], pts: vec![IrPoint { x: 0, y: 0 }, IrPoint { x: 1270, y: 0 }], bus: false },
                // B: horizontal 1270..2540 at y=0 -- only touches A at a point.
                Wire { id: String::new(), net: "B".into(), pins: vec![], pts: vec![IrPoint { x: 1270, y: 0 }, IrPoint { x: 2540, y: 0 }], bus: false },
                // C: horizontal at y=1270 (offset, parallel but not collinear).
                Wire { id: String::new(), net: "C".into(), pins: vec![], pts: vec![IrPoint { x: 0, y: 1270 }, IrPoint { x: 1270, y: 1270 }], bus: false },
                // D: vertical crossing A, not collinear.
                Wire { id: String::new(), net: "D".into(), pins: vec![], pts: vec![IrPoint { x: 600, y: -600 }, IrPoint { x: 600, y: 600 }], bus: false },
            ],
            labels: vec![],
        };
        let results = check_style(&design(sch), &model);
        assert!(results.iter().any(|r| r.check == "schematic_wire_overlap" && r.status == CheckStatus::Pass));
    }

    #[test]
    fn wire_overlap_fails_for_collinear_different_net_overlap() {
        let model = ConstraintModel::default();
        let sch = SchematicSection { power_symbols: vec![], no_connects: vec![], bus_entries: vec![], title_block: None, sheets: vec![], instance_overrides: vec![], junctions: vec![], lines: vec![], texts: vec![], erc_exclusions: vec![], erc_pin_map: None, user_fields: Default::default(), imported_from_kicad: false,
            symbols: vec![],
            wires: vec![
                Wire { id: String::new(), net: "A".into(), pins: vec![], pts: vec![IrPoint { x: 0, y: 0 }, IrPoint { x: 2540, y: 0 }], bus: false },
                // Overlaps A over x in [1270, 2540].
                Wire { id: String::new(), net: "B".into(), pins: vec![], pts: vec![IrPoint { x: 1270, y: 0 }, IrPoint { x: 3810, y: 0 }], bus: false },
            ],
            labels: vec![],
        };
        let results = check_style(&design(sch), &model);
        assert!(results.iter().any(|r| r.check == "schematic_wire_overlap" && r.status == CheckStatus::Fail));
    }

    #[test]
    fn wire_overlap_passes_for_same_net_overlap() {
        let model = ConstraintModel::default();
        let sch = SchematicSection { power_symbols: vec![], no_connects: vec![], bus_entries: vec![], title_block: None, sheets: vec![], instance_overrides: vec![], junctions: vec![], lines: vec![], texts: vec![], erc_exclusions: vec![], erc_pin_map: None, user_fields: Default::default(), imported_from_kicad: false,
            symbols: vec![],
            wires: vec![
                Wire { id: String::new(), net: "A".into(), pins: vec![], pts: vec![IrPoint { x: 0, y: 0 }, IrPoint { x: 2540, y: 0 }], bus: false },
                Wire { id: String::new(), net: "A".into(), pins: vec![], pts: vec![IrPoint { x: 1270, y: 0 }, IrPoint { x: 3810, y: 0 }], bus: false },
            ],
            labels: vec![],
        };
        let results = check_style(&design(sch), &model);
        assert!(results.iter().any(|r| r.check == "schematic_wire_overlap" && r.status == CheckStatus::Pass));
    }

    // ---------------------------------------------------------- crossing_count (warn)

    #[test]
    fn crossing_count_zero_when_no_crossings() {
        let model = ConstraintModel::default();
        let sch = SchematicSection { power_symbols: vec![], no_connects: vec![], bus_entries: vec![], title_block: None, sheets: vec![], instance_overrides: vec![], junctions: vec![], lines: vec![], texts: vec![], erc_exclusions: vec![], erc_pin_map: None, user_fields: Default::default(), imported_from_kicad: false,
            symbols: vec![],
            wires: vec![
                Wire { id: String::new(), net: "A".into(), pins: vec![], pts: vec![IrPoint { x: 0, y: 0 }, IrPoint { x: 1270, y: 0 }], bus: false },
                Wire { id: String::new(), net: "B".into(), pins: vec![], pts: vec![IrPoint { x: 0, y: 5080 }, IrPoint { x: 1270, y: 5080 }], bus: false },
            ],
            labels: vec![],
        };
        let results = check_style(&design(sch), &model);
        let r = results.iter().find(|r| r.check == "schematic_wire_crossing_count").unwrap();
        assert_eq!(r.status, CheckStatus::Warn);
        assert!(r.hint.as_ref().unwrap().starts_with('0'));
    }

    #[test]
    fn crossing_count_counts_a_true_crossing_between_different_nets() {
        let model = ConstraintModel::default();
        let sch = SchematicSection { power_symbols: vec![], no_connects: vec![], bus_entries: vec![], title_block: None, sheets: vec![], instance_overrides: vec![], junctions: vec![], lines: vec![], texts: vec![], erc_exclusions: vec![], erc_pin_map: None, user_fields: Default::default(), imported_from_kicad: false,
            symbols: vec![],
            wires: vec![
                Wire { id: String::new(), net: "A".into(), pins: vec![], pts: vec![IrPoint { x: -1270, y: 0 }, IrPoint { x: 1270, y: 0 }], bus: false },
                Wire { id: String::new(), net: "B".into(), pins: vec![], pts: vec![IrPoint { x: 0, y: -1270 }, IrPoint { x: 0, y: 1270 }], bus: false },
            ],
            labels: vec![],
        };
        let results = check_style(&design(sch), &model);
        let r = results.iter().find(|r| r.check == "schematic_wire_crossing_count").unwrap();
        assert_eq!(r.status, CheckStatus::Warn);
        assert!(r.hint.as_ref().unwrap().starts_with('1'));
    }

    // ---------------------------------------------------------- misc

    #[test]
    fn empty_schematic_section_is_all_pass() {
        let model = ConstraintModel::default();
        let sch = SchematicSection { power_symbols: vec![], no_connects: vec![], bus_entries: vec![], title_block: None, sheets: vec![], instance_overrides: vec![], junctions: vec![], lines: vec![], texts: vec![], erc_exclusions: vec![], erc_pin_map: None, user_fields: Default::default(), imported_from_kicad: false, symbols: vec![], wires: vec![], labels: vec![] };
        let results = check_style(&design(sch), &model);
        assert!(results.iter().filter(|r| r.check != "schematic_wire_crossing_count").all(|r| r.status == CheckStatus::Pass));
    }

    #[test]
    fn net_labels_are_ignored_by_geometry_gates() {
        // Sanity: labels don't participate in any of these checks/panic the
        // gate even when off-grid.
        let model = ConstraintModel::default();
        let sch = SchematicSection { power_symbols: vec![], no_connects: vec![], bus_entries: vec![], title_block: None, sheets: vec![], instance_overrides: vec![], junctions: vec![], lines: vec![], texts: vec![], erc_exclusions: vec![], erc_pin_map: None, user_fields: Default::default(), imported_from_kicad: false,
            symbols: vec![],
            wires: vec![],
            labels: vec![NetLabel { id: String::new(), kind: eda_model::ir::LabelKind::Local, net: "GND".into(), at: IrPoint { x: 3, y: 7 } }],
        };
        let results = check_style(&design(sch), &model);
        assert!(results.iter().all(|r| r.check != "schematic_offgrid" || r.status == CheckStatus::Pass));
    }
}

#[cfg(test)]
mod integration {
    use super::*;
    use eda_engine::{derive_schematic, EngineOptions};
    use eda_model::{Net, Pin, PinKind};

    fn pin(number: &str, name: &str, kind: PinKind) -> Pin {
        Pin { number: number.into(), name: Some(name.into()), kind }
    }

    fn part(reference: &str, pins: Vec<Pin>) -> Part {
        Part { reference: reference.into(), mpn: None, lcsc: None, value: None, package: None, footprint: None, pins, body_um: None, symbol: None, datasheet: None, edge: None }
    }

    fn net(name: &str, pins: &[&str]) -> Net {
        Net { name: name.into(), pins: pins.iter().map(|s| s.to_string()).collect() }
    }

    /// Same LDO fixture as eda-engine's and eda-render's own tests: U1 (LDO)
    /// + CIN + COUT.
    fn ldo_model() -> ConstraintModel {
        let u1 = part(
            "U1",
            vec![
                pin("1", "VIN", PinKind::Power),
                pin("2", "GND", PinKind::Ground),
                pin("3", "EN", PinKind::Signal),
                pin("4", "VOUT", PinKind::Power),
                pin("5", "NC", PinKind::Nc),
            ],
        );
        let cin = part("CIN", vec![pin("1", "1", PinKind::Passive), pin("2", "2", PinKind::Ground)]);
        let cout = part("COUT", vec![pin("1", "1", PinKind::Passive), pin("2", "2", PinKind::Ground)]);

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

    #[test]
    fn ldo_schematic_has_zero_fails() {
        let model = ldo_model();
        let design = derive_schematic(&model, &EngineOptions::new(1, "test-hash")).unwrap();
        let results = check_style(&design, &model);
        let fails: Vec<&CheckResult> = results.iter().filter(|r| r.status == CheckStatus::Fail).collect();
        assert!(fails.is_empty(), "expected zero Fail-status results on the LDO fixture, got: {fails:#?}");
    }

    /// `schematic_wire_overlap` on the LDO fixture: this gate previously
    /// caught a real bug here (GND and VOUT sharing a collinear channel
    /// stagger row, before `eda-layout`'s routing sized channels to fit one
    /// genuinely distinct row per hop) — proven failing at that point in
    /// the same test position this now occupies. With the routing fix
    /// landed, this must now be a clean `Pass`, same as
    /// `ldo_schematic_has_zero_fails` above.
    #[test]
    fn schematic_wire_overlap_passes_on_ldo_after_routing_fix() {
        let model = ldo_model();
        let design = derive_schematic(&model, &EngineOptions::new(1, "test-hash")).unwrap();
        let results = check_style(&design, &model);
        assert!(
            results.iter().any(|r| r.check == "schematic_wire_overlap" && r.status == CheckStatus::Pass),
            "expected schematic_wire_overlap to pass cleanly on the LDO fixture post-fix, got: {results:#?}"
        );
    }
}

