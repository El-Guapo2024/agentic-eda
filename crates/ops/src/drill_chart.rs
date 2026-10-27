//! KiCad 10's drill model: the board's holes as drill operations, grouped and marked by the board's drill symbol profile, and the drill charts that report them (KiCad 25ec2335:
//! the 8303b2ad source the rest of the port reads predates them).
//!
//!   pcbnew/drill/drill_enumerator.cpp        EnumerateDrillOperations, EnumerateDrillSpans, IsDrillSlot     [`operations`], [`spans`]
//!   pcbnew/drill/drill_symbol_profile.cpp    DRILL_SYMBOL_PROFILE::GroupKeyString                            [`group_key`]
//!   pcbnew/drill/drill_chart_model.cpp       DRILL_CHART_MODEL::Build, matchesFilter                         [`groups`]
//!   pcbnew/drill/drill_symbol_assigner.cpp   AssignDrillSymbols, ResolveDrillSymbols, ResolveDrillSymbolsByItem   [`assign`], [`resolve`], [`marks`]
//!   pcbnew/pcb_drill_chart.cpp               PCB_DRILL_CHART::generate, onRebuilt                            [`content`]
//!   pcbnew/pcb_generated_table.cpp           PCB_GENERATED_TABLE::RebuildCells, migrateRows, IsStale         [`rebuild`], [`is_stale`]
//!   pcbnew/board_tables/generated_table_refresh.cpp   RefreshGeneratedTables                                 [`Board::refresh_drill_charts`]
//!
//! A via's backdrills (`(backdrill ..)`, `(tertiary_drill ..)`) and post-machining (`(front_post_machining ..)`, `(back_post_machining ..)`) are read from the tokens its
//! [`eda_model::via_extra::ViaExtra`] kept (r6-editors-exports D6-1): a backdrill span and its rows, the operation kind and the post-machining in the key, the stub length. A pad's
//! are not needed: the chart's queries leave `m_PadMachining` off (`DRILL_QUERY`), so KiCad's chart has neither. Positions, sizes, keys and sites are nanometres.

use std::collections::{BTreeMap, BTreeSet};

use eda_model::drill_chart::{DrillChart, DrillChartColumnId, DrillChartFilter, DrillGroupKey, DrillMarkMode, DrillMarkPolicy, DrillSpan, DrillSymbolAssignment, DrillSymbolProfile, TableAlign, TableUnits, CURATED_SHAPES};
use eda_model::footprint::{to_board, Pad, PadKind};
use eda_model::ir::Side;
use eda_model::ir::{Design, FootprintInstance, Point, TextJustify, Nm};
use eda_model::table::{Table, TableCell};
use eda_model::via_extra::{PostMachining, ViaBackdrill};
use eda_model::{ConstraintModel, Stackup};

// ------------------------------------------------------------------------------------------------------------------------------------------------ the operations

/// `HOLE_ATTRIBUTE`, its value as the key writes it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HoleAttribute {
    ViaThrough = 1,
    ViaBuried = 2,
    ViaBackdrill = 3,
    Pad = 4,
    PadCastellated = 5,
    PadPressfit = 6,
    Mechanical = 7,
}

/// `DRILL_OPERATION`: one machining action, a hole's primary drill or a via's backdrill.
#[derive(Debug, Clone, PartialEq)]
pub struct DrillOp {
    /// The via's id, or `REF.PAD`.
    pub source: String,
    pub at: Point,
    /// `m_SizeXY`, µm, the source's own axes.
    pub size: (Nm, Nm),
    /// `m_Orientation`, degrees, KiCad's sense.
    pub orientation_deg: f64,
    pub diameter: Nm,
    pub is_slot: bool,
    pub top: String,
    pub bottom: String,
    pub not_plated: bool,
    pub attribute: HoleAttribute,
    /// filled, capped, top/bottom covered, top/bottom plugged, top/bottom tented.
    pub protection: [bool; 8],
    /// `DRILL_OP_KIND`: 0 the primary drill, 1 the secondary (the backdrill), 2 the tertiary.
    pub kind: i32,
    /// `m_FrontPostMachining`, `m_BackPostMachining`.
    pub post_machining: [PostMachining; 2],
    /// `m_StubLength`: a backdrill's `BOARD_STACKUP::GetDrillDepth` of its start and end layers.
    pub stub_length: Option<Nm>,
}

/// `DRILL_SPAN` of two copper layers of the board, ordered as the board stacks them.
fn copper_index(layers: &[String], name: &str) -> usize {
    layers.iter().position(|l| l == name).unwrap_or(0)
}

/// `CopperLayerToOrdinal`: F.Cu 0, InN.Cu N, B.Cu 31.
fn ordinal(name: &str) -> u32 {
    match name {
        "F.Cu" => 0,
        "B.Cu" => 31,
        n => n.strip_prefix("In").and_then(|r| r.strip_suffix(".Cu")).and_then(|d| d.parse().ok()).unwrap_or(0),
    }
}

/// `TopLayer()` / `BottomLayer()` of a span from `a` to `b`.
fn top_bottom(layers: &[String], a: &str, b: &str) -> (String, String) {
    if copper_index(layers, a) <= copper_index(layers, b) {
        (a.to_string(), b.to_string())
    } else {
        (b.to_string(), a.to_string())
    }
}

fn copper_layers(model: &ConstraintModel) -> Vec<String> {
    let l: Vec<String> = model.board.layers.iter().filter(|l| l.ends_with(".Cu")).cloned().collect();
    if l.is_empty() {
        vec!["F.Cu".into(), "B.Cu".into()]
    } else {
        l
    }
}

/// `isBackdrillSet`: a drill with both layers and a size.
fn backdrill_set(d: &ViaBackdrill) -> bool {
    !d.start.is_empty() && !d.end.is_empty() && d.size > Nm::ZERO
}

/// A via's secondary and tertiary drills, with their `DRILL_OP_KIND`.
fn via_backdrills(design: &Design, via_id: &str) -> Vec<(i32, ViaBackdrill)> {
    let Some(x) = design.drawings.as_ref().and_then(|d| d.via_extra(via_id)) else { return Vec::new() };
    [(1, x.secondary_drill()), (2, x.tertiary_drill())].into_iter().filter_map(|(k, d)| d.filter(backdrill_set).map(|d| (k, d))).collect()
}

/// `EnumerateDrillSpans` (without pad backdrills, `aPadBackdrills` false as the chart and the map ask): the through span first, then every other pair the vias span and every
/// span a via's backdrill drills, in `DRILL_SPAN::operator<` order (the layer ids of the top and the bottom, a backdrill before a plain span, then the stored layers).
pub fn spans(design: &Design, model: &ConstraintModel) -> Vec<DrillSpan> {
    let layers = copper_layers(model);
    let id = |n: &str| eda_model::layers::pcb_layer_id(n).unwrap_or(0);
    let mut unique: BTreeSet<(u32, u32, bool, u32, u32, DrillSpanKey)> = BTreeSet::new();
    let mut add = |start: &str, end: &str, backdrill: bool| {
        let (top, bottom) = top_bottom(&layers, start, end);
        unique.insert((id(&top), id(&bottom), !backdrill, id(start), id(end), DrillSpanKey(start.to_string(), end.to_string(), backdrill)));
    };
    for v in design.routing.iter().flat_map(|r| r.vias.iter()) {
        let (top, bottom) = top_bottom(&layers, &v.from_layer, &v.to_layer);
        if !(top == "F.Cu" && bottom == "B.Cu") {
            add(&top, &bottom, false);
        }
        for (_, d) in via_backdrills(design, &v.id) {
            add(&d.start, &d.end, true);
        }
    }
    let mut out = vec![DrillSpan { start: "F.Cu".into(), end: "B.Cu".into(), backdrill: false, npth: false }];
    out.extend(unique.into_iter().filter(|u| u.5 .2 || !(u.0 == id("F.Cu") && u.1 == id("B.Cu"))).map(|u| DrillSpan { start: u.5 .0, end: u.5 .1, backdrill: u.5 .2, npth: false }));
    out
}

/// A span's stored layers and its backdrill flag, the tail of the set's order.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct DrillSpanKey(String, String, bool);

/// A via's or the board's protection word: `(name yes)` among the tokens, `(name (front yes) ..)` for a side.
fn token_flag(tokens: &[String], name: &str, side: Option<&str>) -> Option<bool> {
    let t = tokens.iter().find(|t| t.starts_with(&format!("({name} ")) || t.starts_with(&format!("({name})")))?;
    match side {
        None => Some(!t.contains(" no)")),
        Some(s) => {
            let at = t.find(&format!("({s} "))?;
            Some(t[at..].starts_with(&format!("({s} yes)")))
        }
    }
}

/// `EnumerateDrillOperations` for one span: the vias of the span, then -- for the through span -- the pads of every footprint (and the footprint's holes on no copper). A
/// `non_plated` query gives the non-plated holes only, vias none; a plated one every other.
pub fn operations(design: &Design, model: &ConstraintModel, span: &DrillSpan, non_plated: bool, merged: bool) -> Vec<DrillOp> {
    operations_in(design, model, None, span, non_plated, merged)
}

/// [`operations`] with the board's stackup, which gives a backdrill its stub length (`stubLength`, `GetStackupDescriptor().GetDrillDepth`).
pub fn operations_in(design: &Design, model: &ConstraintModel, stackup: Option<&Stackup>, span: &DrillSpan, non_plated: bool, merged: bool) -> Vec<DrillOp> {
    let layers = copper_layers(model);
    let mut out = Vec::new();
    let dr = design.drawings.as_ref();
    let board_tokens: Vec<String> = dr.map(|d| d.setup_tokens.clone()).unwrap_or_default();
    let sm = &model.board.solder_mask;
    let span_pair = top_bottom(&layers, &span.start, &span.end);
    let through = span_pair.0 == "F.Cu" && span_pair.1 == "B.Cu";
    if span.backdrill {
        // `enumerateVia`'s `tryBackdrill`: each of the via's drills that runs from the span's start layer to its end layer, not plated, with its stub; "Backdrills remove
        // the via barrel, so via surface protection is not inherited" (the drill's own filling and capping, which the file format does not write: false).
        if non_plated && !merged {
            return out;
        }
        for v in design.routing.iter().flat_map(|r| r.vias.iter()) {
            for (kind, d) in via_backdrills(design, &v.id) {
                if d.start != span.start || d.end != span.end {
                    continue;
                }
                out.push(DrillOp {
                    source: v.id.clone(),
                    at: v.at,
                    size: (d.size, d.size),
                    orientation_deg: 0.0,
                    diameter: d.size,
                    is_slot: false,
                    top: span_pair.0.clone(),
                    bottom: span_pair.1.clone(),
                    not_plated: true,
                    attribute: HoleAttribute::ViaBackdrill,
                    protection: [false; 8],
                    kind,
                    post_machining: Default::default(),
                    stub_length: stackup.map(|s| drill_depth_nm(s, &d.start, &d.end)),
                });
            }
        }
        return out;
    }
    if !non_plated || merged {
        for v in design.routing.iter().flat_map(|r| r.vias.iter()) {
            if v.drill == Nm::ZERO {
                continue;
            }
            let pair = top_bottom(&layers, &v.from_layer, &v.to_layer);
            if pair != span_pair {
                continue;
            }
            let tokens: Vec<String> = dr.and_then(|d| d.via_extras.iter().find(|x| x.id == v.id)).map(|x| x.tokens.clone()).unwrap_or_default();
            // `setProtection`: the via's own, else the board's (`m_FillVias`, `m_CapVias`, the outer sides' covering, plugging and tenting).
            let side = |l: &str| if l == "F.Cu" { Some("front") } else if l == "B.Cu" { Some("back") } else { None };
            let flag = |name: &str, layer: &str, board_default: bool| -> bool {
                // An inner layer is no board side: nothing falls back for it, and the IR keeps no inner-layer protection.
                let Some(s) = side(layer) else { return false };
                token_flag(&tokens, name, Some(s)).or_else(|| token_flag(&board_tokens, name, Some(s))).unwrap_or(board_default)
            };
            let tent = |layer: &str| match side(layer) {
                Some("front") => token_flag(&tokens, "tenting", Some("front")).unwrap_or(sm.tent_vias_front),
                Some(_) => token_flag(&tokens, "tenting", Some("back")).unwrap_or(sm.tent_vias_back),
                None => false,
            };
            let protection = [
                token_flag(&tokens, "filling", None).or_else(|| token_flag(&board_tokens, "filling", None)).unwrap_or(false),
                token_flag(&tokens, "capping", None).or_else(|| token_flag(&board_tokens, "capping", None)).unwrap_or(false),
                flag("covering", &pair.0, false),
                flag("covering", &pair.1, false),
                flag("plugging", &pair.0, false),
                flag("plugging", &pair.1, false),
                tent(&pair.0),
                tent(&pair.1),
            ];
            out.push(DrillOp {
                source: v.id.clone(),
                at: v.at,
                size: (v.drill, v.drill),
                orientation_deg: 0.0,
                diameter: v.drill,
                is_slot: false,
                top: pair.0.clone(),
                bottom: pair.1.clone(),
                not_plated: false,
                attribute: if through { HoleAttribute::ViaThrough } else { HoleAttribute::ViaBuried },
                protection,
                kind: 0,
                post_machining: dr.and_then(|d| d.via_extra(&v.id)).map(|x| [x.post_machining(true), x.post_machining(false)]).unwrap_or_default(),
                stub_length: None,
            });
        }
    }
    if !through || span.backdrill {
        return out;
    }
    let Some(pl) = &design.placement else { return out };
    for fp in &pl.footprints {
        let Some(part) = model.part(&fp.id) else { continue };
        let Some(footprint) = model.footprint_of(part) else { continue };
        let extra = dr.and_then(|d| d.footprint_extras.iter().find(|e| e.id == fp.id));
        for (i, pad) in footprint.pads.iter().enumerate() {
            let tokens: &[String] = extra.filter(|e| e.pads.len() == footprint.pads.len()).and_then(|e| e.pads.get(i)).map(|m| m.tokens.as_slice()).unwrap_or(&[]);
            if let Some(op) = pad_op(fp, pad, tokens, non_plated, merged) {
                out.push(op);
            }
        }
        for hole in extra.iter().flat_map(|e| e.holes.iter()) {
            if let Some(op) = pad_op(fp, &hole.pad, &[], non_plated, merged) {
                out.push(op);
            }
        }
    }
    out
}

/// `enumeratePad`: a pad with a hole, plated or not as the query asks (both when merged).
fn pad_op(fp: &FootprintInstance, pad: &Pad, tokens: &[String], non_plated: bool, merged: bool) -> Option<DrillOp> {
    let npth = pad.kind == PadKind::NonPlatedHole;
    if pad.kind == PadKind::Smd {
        return None;
    }
    if !merged && (npth != non_plated) {
        return None;
    }
    let size = match (pad.drill_slot, pad.drill) {
        (Some(s), _) => s,
        (None, Some(d)) => (d, d),
        _ => return None,
    };
    if size.0 == Nm::ZERO {
        return None;
    }
    let has = |p: &str| tokens.iter().any(|t| t == &format!("(property {p})"));
    let attribute = if npth {
        HoleAttribute::Mechanical
    } else if has("pad_prop_castellated") {
        HoleAttribute::PadCastellated
    } else if has("pad_prop_pressfit") {
        HoleAttribute::PadPressfit
    } else {
        HoleAttribute::Pad
    };
    let board_rot = fp.rot as i64;
    // `pad_file_angle`: the footprint's rotation plus the pad's own (whose sense flips on the bottom), negated -- `PAD::GetOrientation`.
    let orientation_mdeg = (-(board_rot + if fp.side == Side::Bottom { -(pad.rot as i64) } else { pad.rot as i64 })).rem_euclid(360_000);
    Some(DrillOp {
        source: format!("{}.{}", fp.id, pad.number),
        at: to_board(fp, pad.at),
        size,
        orientation_deg: orientation_mdeg as f64 / 1000.0,
        diameter: size.0.min(size.1),
        // `IsDrillSlot`: an oblong drill whose sides differ.
        is_slot: pad.drill_slot.is_some() && size.0 != size.1,
        top: "F.Cu".into(),
        bottom: "B.Cu".into(),
        not_plated: npth,
        attribute,
        protection: [false; 8],
        kind: 0,
        // `m_PadMachining` is off in every query the chart and the map make.
        post_machining: Default::default(),
        stub_length: None,
    })
}

/// `DRILL_SYMBOL_PROFILE::GroupKeyString`.
pub fn group_key(p: &DrillSymbolProfile, op: &DrillOp) -> String {
    let mut key = String::from("v1");
    if p.is_grouped_by(DrillGroupKey::Size) {
        key += &format!("|d{}", op.diameter);
    }
    if p.is_grouped_by(DrillGroupKey::Slot) {
        key += &if op.is_slot { format!("|s{}x{}", op.size.0, op.size.1) } else { "|s-".to_string() };
    }
    if p.is_grouped_by(DrillGroupKey::Plating) {
        key += if op.not_plated { "|p0" } else { "|p1" };
    }
    if p.is_grouped_by(DrillGroupKey::Span) {
        key += &format!("|L{}-{}", ordinal(&op.top), ordinal(&op.bottom));
    }
    if p.is_grouped_by(DrillGroupKey::Operation) {
        key += &format!("|k{}", op.kind);
    }
    if p.is_grouped_by(DrillGroupKey::HoleFunction) {
        key += &format!("|a{}", op.attribute as i32);
    }
    if p.is_grouped_by(DrillGroupKey::Protection) {
        key += "|r";
        for b in op.protection {
            key += if b { "1" } else { "0" };
        }
    }
    if p.is_grouped_by(DrillGroupKey::PostMachining) {
        let [f, b] = op.post_machining;
        key += &format!("|m{}:{}:{}:{}/{}:{}:{}:{}", f.mode, f.size, f.depth, f.angle, b.mode, b.size, b.depth, b.angle);
    }
    key
}

// ---------------------------------------------------------------------------------------------------------------------------------------------------- the groups

/// `DRILL_CHART_GROUP`: one row of a chart.
#[derive(Debug, Clone, PartialEq)]
pub struct DrillGroup {
    pub key: String,
    /// The first operation of the group: what its row prints.
    pub first: DrillOp,
    pub operations: usize,
    /// Distinct positions, nm.
    pub sites: BTreeSet<(Nm, Nm)>,
    pub symbol: DrillSymbolAssignment,
}

/// `matchesFilter`.
fn matches_filter(op: &DrillOp, f: &DrillChartFilter) -> bool {
    if op.not_plated && !f.non_plated || !op.not_plated && !f.plated || op.is_slot && !f.slots {
        return false;
    }
    if op.kind != 0 && !f.backdrills {
        return false;
    }
    if op.attribute == HoleAttribute::PadCastellated && !f.castellated {
        return false;
    }
    let via = matches!(op.attribute, HoleAttribute::ViaThrough | HoleAttribute::ViaBuried);
    !(via && !f.vias)
}

/// `DRILL_CHART_MODEL::Build`: every span's plated then non-plated operations that pass `filter`, grouped by the profile's key, sorted plated first, then by diameter, then by key.
pub fn groups(design: &Design, model: &ConstraintModel, profile: &DrillSymbolProfile, filter: &DrillChartFilter) -> Vec<DrillGroup> {
    groups_in(design, model, None, profile, filter)
}

/// [`groups`] with the board's stackup (the backdrills' stub lengths).
pub fn groups_in(design: &Design, model: &ConstraintModel, stackup: Option<&Stackup>, profile: &DrillSymbolProfile, filter: &DrillChartFilter) -> Vec<DrillGroup> {
    let mut by_key: BTreeMap<String, DrillGroup> = BTreeMap::new();
    for span in spans(design, model) {
        for non_plated in [false, true] {
            for op in operations_in(design, model, stackup, &span, non_plated, false) {
                if !matches_filter(&op, filter) {
                    continue;
                }
                let key = group_key(profile, &op);
                let site = (op.at.x, op.at.y);
                let g = by_key.entry(key.clone()).or_insert_with(|| DrillGroup { key, first: op.clone(), operations: 0, sites: BTreeSet::new(), symbol: Default::default() });
                g.operations += 1;
                g.sites.insert(site);
            }
        }
    }
    let mut out: Vec<DrillGroup> = by_key.into_values().collect();
    out.sort_by(|a, b| a.first.not_plated.cmp(&b.first.not_plated).then(a.first.diameter.cmp(&b.first.diameter)).then(a.key.cmp(&b.key)));
    out
}

/// `AssignDrillSymbols`: with assignments frozen, a group the profile has a mark for keeps it; every other group takes the lowest curated shape nobody uses (the policy
/// permitting), then letters, A to ZZ; the profile gains the new marks.
pub fn assign(groups: &mut [DrillGroup], profile: &mut DrillSymbolProfile) {
    let shape_count = CURATED_SHAPES.len() as i32;
    let mut used_shapes: BTreeSet<i32> = BTreeSet::new();
    let mut used_letters: BTreeSet<String> = BTreeSet::new();
    if profile.freeze_assignments {
        for g in groups.iter_mut() {
            if let Some(a) = profile.assignments.get(&g.key) {
                g.symbol = a.clone();
                match a.mode {
                    DrillMarkMode::Shape => {
                        used_shapes.insert(a.shape_index);
                    }
                    DrillMarkMode::Letter => {
                        used_letters.insert(a.letter.clone());
                    }
                    DrillMarkMode::SizeText => {}
                }
            }
        }
    }
    let next_letter = |used: &BTreeSet<String>| -> String {
        for i in 0..26 * 27 {
            let c = if i < 26 { ((b'A' + i as u8) as char).to_string() } else { format!("{}{}", (b'A' + (i / 26) as u8 - 1) as char, (b'A' + (i % 26) as u8) as char) };
            if !used.contains(&c) {
                return c;
            }
        }
        "?".to_string()
    };
    for g in groups.iter_mut() {
        if profile.freeze_assignments && profile.assignments.contains_key(&g.key) {
            continue;
        }
        let mut a = DrillSymbolAssignment::default();
        match profile.mark_policy {
            DrillMarkPolicy::SizeText => a.mode = DrillMarkMode::SizeText,
            DrillMarkPolicy::Letters => {
                a.mode = DrillMarkMode::Letter;
                a.letter = next_letter(&used_letters);
                used_letters.insert(a.letter.clone());
            }
            DrillMarkPolicy::Shapes | DrillMarkPolicy::ShapesThenLetters => match (0..shape_count).find(|i| !used_shapes.contains(i)) {
                Some(shape) => {
                    a.mode = DrillMarkMode::Shape;
                    a.shape_index = shape;
                    used_shapes.insert(shape);
                }
                None if profile.mark_policy == DrillMarkPolicy::ShapesThenLetters => {
                    a.mode = DrillMarkMode::Letter;
                    a.letter = next_letter(&used_letters);
                    used_letters.insert(a.letter.clone());
                }
                // "Out of distinguishable shapes, so reuse from the start rather than draw nothing at all"
                None => {
                    a.mode = DrillMarkMode::Shape;
                    a.shape_index = 0;
                }
            },
        }
        g.symbol = a.clone();
        profile.assignments.insert(g.key.clone(), a);
    }
}

/// The board's profile (`GetDrillSymbolProfile`).
pub fn profile_of(design: &Design) -> DrillSymbolProfile {
    design.drawings.as_ref().and_then(|d| d.board_items.drill_profile.clone()).unwrap_or_default()
}

/// `ResolveDrillSymbols`: the mark every group of the whole board has, assigned on a copy of the profile.
pub fn resolve(design: &Design, model: &ConstraintModel) -> BTreeMap<String, DrillSymbolAssignment> {
    let mut profile = profile_of(design);
    let mut gs = groups(design, model, &profile, &DrillChartFilter::default());
    assign(&mut gs, &mut profile);
    gs.into_iter().map(|g| (g.key, g.symbol)).collect()
}

/// `DRILL_SYMBOL_ENTRY`: one mark a drill map draws, at its hole.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct DrillMark {
    pub at: Point,
    pub span: DrillSpan,
    pub mode: DrillMarkMode,
    pub shape_index: i32,
    pub letter: String,
    pub diameter_nm: Nm,
    pub size_nm: (Nm, Nm),
    pub orientation_deg: f64,
    pub is_slot: bool,
}

/// `ResolveDrillSymbolsByItem`, flattened: every hole's mark, span by span.
pub fn marks(design: &Design, model: &ConstraintModel) -> Vec<DrillMark> {
    let resolved = resolve(design, model);
    let profile = profile_of(design);
    let mut out = Vec::new();
    for span in spans(design, model) {
        for op in operations(design, model, &span, false, true) {
            let Some(a) = resolved.get(&group_key(&profile, &op)) else { continue };
            out.push(DrillMark { at: op.at, span: span.clone(), mode: a.mode, shape_index: a.shape_index, letter: a.letter.clone(), diameter_nm: op.diameter, size_nm: op.size, orientation_deg: op.orientation_deg, is_slot: op.is_slot });
        }
    }
    out
}

// ------------------------------------------------------------------------------------------------------------------------------------------------- the content

/// `FormatGeneratedTableLength`.
pub fn format_length(nm: Nm, units: TableUnits, precision: i32) -> String {
    let mm = eda_model::units::to_mm(nm);
    let p = precision.max(0) as usize;
    match units {
        TableUnits::Inch => format!("{:.*}\u{2033}", p + 1, mm / 25.4),
        TableUnits::Mils => format!("{:.*} mil", precision.saturating_sub(2).max(0) as usize, mm / 25.4 * 1000.0),
        TableUnits::Mm => format!("{mm:.p$}"),
    }
}

const NO_VALUE: &str = "\u{2014}";

/// `BOARD_STACKUP::GetLayerDistance`: the thickness between two copper layers, half of an inner one at either end, nm.
fn layer_distance_nm(stackup: &Stackup, copper: &[String], first: &str, second: &str) -> Nm {
    if first == second {
        return Nm::ZERO;
    }
    let pos = |n: &str| copper.iter().position(|c| c == n).unwrap_or(0);
    let (a, b) = if pos(second) < pos(first) { (second, first) } else { (first, second) };
    let is_copper = |l: &eda_model::StackupLayer| l.kind.as_deref() == Some("copper") || (l.kind.is_none() && l.name.ends_with(".Cu"));
    let is_dielectric = |l: &eda_model::StackupLayer| matches!(l.kind.as_deref(), Some("core") | Some("prepreg") | Some("dielectric"));
    let inner = |n: &str| n != "F.Cu" && n != "B.Cu";
    let (mut total, mut start, mut half) = (Nm::ZERO, false, false);
    for l in &stackup.layers {
        let layer = is_copper(l).then_some(l.name.as_str());
        if layer.is_none() && !is_dielectric(l) {
            continue;
        }
        if !start && layer == Some(a) {
            start = true;
            half = inner(a);
        } else if !start {
            continue;
        }
        if layer == Some(b) && inner(b) {
            half = true;
        }
        let t = Nm::kiround(l.thickness_mm.unwrap_or(0.0) * 1_000_000.0);
        total += if half { t / 2 } else { t };
        half = false;
        if layer == Some(b) {
            break;
        }
    }
    total
}

/// `BOARD_STACKUP::GetDrillDepth( aFirstLayer, aSecondLayer )`: the thickness of every copper layer and dielectric from the board's top (from its bottom when the second layer's id
/// is below the first's) down to the second layer, and half of the item after it -- the first layer only picks the direction, as KiCad's loop has it.
pub fn drill_depth_nm(stackup: &Stackup, first: &str, second: &str) -> Nm {
    let id = |n: &str| eda_model::layers::pcb_layer_id(n).unwrap_or(0);
    let is_copper = |l: &eda_model::StackupLayer| l.kind.as_deref() == Some("copper") || (l.kind.is_none() && l.name.ends_with(".Cu"));
    let is_dielectric = |l: &eda_model::StackupLayer| matches!(l.kind.as_deref(), Some("core") | Some("prepreg") | Some("dielectric"));
    let (mut total, mut half) = (Nm::ZERO, false);
    let items: Vec<&eda_model::StackupLayer> = if id(second) > id(first) { stackup.layers.iter().collect() } else { stackup.layers.iter().rev().collect() };
    for l in items {
        // "Will be UNDEFINED_LAYER for dielectrics"; a silk or mask layer is skipped.
        if !is_copper(l) && !is_dielectric(l) {
            continue;
        }
        let t = Nm::kiround(l.thickness_mm.unwrap_or(0.0) * 1_000_000.0);
        total += if half { t / 2 } else { t };
        if half {
            return total;
        }
        if is_copper(l) && l.name == second {
            half = true;
        }
    }
    total
}

/// `DRILL_CHART_CONTENT`: the rows (key and text of each cell), the totals, the shape of each row's mark and the symbol column.
#[derive(Debug, Clone, PartialEq)]
pub struct ChartContent {
    pub rows: Vec<(String, Vec<String>)>,
    pub trailer: Vec<Vec<String>>,
    pub row_shapes: Vec<Option<i32>>,
    pub symbol_column: i32,
}

/// `PCB_DRILL_CHART::generate`: the chart's groups under the profile `profile` (assigned into it when `refresh`, else resolved over the whole board on a copy), and the text of
/// each of the chart's columns for each.
pub fn content(design: &Design, model: &ConstraintModel, stackup: &Stackup, chart: &DrillChart, profile: &mut DrillSymbolProfile, refresh: bool) -> ChartContent {
    let mut gs = groups_in(design, model, Some(stackup), profile, &chart.filter);
    if refresh {
        assign(&mut gs, profile);
    } else {
        // "Whole board, not this chart's filtered subset, or a filtered chart prints a different symbol from the map for the same hole"
        let resolved = resolve(design, model);
        for g in gs.iter_mut() {
            if let Some(a) = resolved.get(&g.key) {
                g.symbol = a.clone();
            }
        }
    }
    let copper = copper_layers(model);
    let (u, p) = (chart.units, chart.precision);
    let symbol_column = chart.columns.iter().rposition(|c| c.id == DrillChartColumnId::Symbol).map(|i| i as i32).unwrap_or(-1);
    let mut rows = Vec::new();
    let mut row_shapes = Vec::new();
    for g in &gs {
        let f = &g.first;
        row_shapes.push((g.symbol.mode == DrillMarkMode::Shape).then_some(g.symbol.shape_index));
        let cells = chart
            .columns
            .iter()
            .map(|c| match c.id {
                DrillChartColumnId::Symbol => match g.symbol.mode {
                    DrillMarkMode::Letter => g.symbol.letter.clone(),
                    DrillMarkMode::SizeText => format_length(f.diameter, u, p),
                    // "The shape itself is drawn by the painter. The cell carries no text"
                    DrillMarkMode::Shape => String::new(),
                },
                DrillChartColumnId::DrillDiameter => format_length(f.diameter, u, p),
                DrillChartColumnId::SlotSize => {
                    if f.is_slot {
                        format!("{} x {}", format_length(f.size.0, u, p), format_length(f.size.1, u, p))
                    } else {
                        NO_VALUE.into()
                    }
                }
                DrillChartColumnId::Plating => (if f.not_plated { "No" } else { "Yes" }).into(),
                DrillChartColumnId::OperationCount => g.operations.to_string(),
                DrillChartColumnId::SiteCount => g.sites.len().to_string(),
                // `spanText`: "The board's own layer names, so a chart matches the stackup the fabricator was given" (`BOARD::GetLayerName`).
                DrillChartColumnId::LayerSpan => format!("{} / {}", model.board.layer_setup.display_name(&f.top), model.board.layer_setup.display_name(&f.bottom)),
                // `operationText`.
                DrillChartColumnId::Operation => (if f.kind != 0 {
                    "Backdrill"
                } else if f.is_slot {
                    "Slot"
                } else {
                    "Drill"
                })
                .into(),
                DrillChartColumnId::Protection => {
                    let pr = f.protection;
                    let parts: Vec<&str> = [(pr[0], "filled"), (pr[1], "capped"), (pr[4] || pr[5], "plugged"), (pr[2] || pr[3], "covered"), (pr[6] || pr[7], "tented")].into_iter().filter(|(on, _)| *on).map(|(_, w)| w).collect();
                    if parts.is_empty() {
                        NO_VALUE.into()
                    } else {
                        parts.join(",")
                    }
                }
                DrillChartColumnId::BackdrillStub => f.stub_length.map(|s| format_length(s, u, p)).unwrap_or_else(|| NO_VALUE.into()),
                DrillChartColumnId::AspectRatio => {
                    let depth = layer_distance_nm(stackup, &copper, &f.top, &f.bottom);
                    let d = f.diameter;
                    if depth > Nm::ZERO && d > Nm::ZERO {
                        format!("{:.1}:1", depth.ratio(d))
                    } else {
                        NO_VALUE.into()
                    }
                }
                DrillChartColumnId::Description => g.symbol.description.clone(),
            })
            .collect();
        rows.push((g.key.clone(), cells));
    }
    let mut trailer = Vec::new();
    if chart.show_totals {
        let ops: usize = gs.iter().map(|g| g.operations).sum();
        let sites: BTreeSet<(Nm, Nm)> = gs.iter().flat_map(|g| g.sites.iter().copied()).collect();
        trailer.push(vec![format!("{ops} OPS / {} SITES", sites.len())]);
    }
    ChartContent { rows, trailer, row_shapes, symbol_column }
}

fn text_at(cells: &[String], col: usize) -> &str {
    cells.get(col).map(String::as_str).unwrap_or("")
}

fn justify_of(a: TableAlign) -> TextJustify {
    match a {
        TableAlign::Left => TextJustify::Left,
        TableAlign::Center => TextJustify::Center,
        TableAlign::Right => TextJustify::Right,
    }
}

/// `BOARD::GetDrillModelGeneration`, as a digest (FNV-1a over the JSON) of what bumps it: every via (`PCB_VIA_T`), every footprint and its pads (`PCB_FOOTPRINT_T`,
/// `PCB_PAD_T`: the placement, the footprint the part has, the extras and edits of its pads and holes), the copper layers (`SetCopperLayerCount`, `SetLayerName`,
/// `SetEnabledLayers`) and the drill maps (`noteDrillModelChange` of a `PCB_DRILL_MAP_T`). KiCad counts commits; the digest changes on the same edits, except one that
/// gives an item back exactly what it had (KiCad bumps, the digest does not).
pub fn drill_generation(design: &Design, model: &ConstraintModel) -> String {
    fn eat<T: serde::Serialize + ?Sized>(h: &mut u64, x: &T) {
        for b in serde_json::to_string(x).unwrap_or_default().bytes().chain([0xff]) {
            *h ^= u64::from(b);
            *h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    eat(&mut h, &model.board.layers);
    eat(&mut h, &model.board.layer_setup.names);
    if let Some(r) = &design.routing {
        eat(&mut h, &r.vias);
    }
    if let Some(pl) = &design.placement {
        eat(&mut h, &pl.footprints);
        for fp in &pl.footprints {
            eat(&mut h, &model.part(&fp.id).and_then(|p| model.footprint_of(p)).map(|f| f.pads));
        }
    }
    if let Some(dr) = design.drawings.as_ref() {
        eat(&mut h, &dr.via_extras);
        eat(&mut h, &dr.footprint_extras);
        eat(&mut h, &dr.footprint_edits);
        eat(&mut h, &dr.board_items.drill_maps.iter().map(|m| (&m.id, &m.layer)).collect::<Vec<_>>());
    }
    format!("{h:016x}")
}

/// `PCB_GENERATED_TABLE::IsStale` (the base class's test): the cells no longer say what the board says (a column, a heading, a row's key or text, a totals line). A drill chart
/// overrides it with the drill generation (`PCB_DRILL_CHART::IsStale`, `DrillChart::built_generation`); `refresh_drill_charts` asks that.
pub fn is_stale(t: &Table, chart: &DrillChart, c: &ChartContent) -> bool {
    let cols = chart.columns.len();
    let rows = 1 + c.rows.len() + c.trailer.len();
    if t.col_count() != cols || t.row_count() != rows {
        return true;
    }
    let cell = |r: usize, col: usize| t.cells.get(r * cols + col).map(|x| x.text.as_str()).unwrap_or("\u{0}");
    if chart.columns.iter().enumerate().any(|(i, col)| cell(0, i) != col.heading) {
        return true;
    }
    for (i, (key, texts)) in c.rows.iter().enumerate() {
        if chart.row_keys.get(&(1 + i as u32)) != Some(key) || (0..cols).any(|col| cell(1 + i, col) != text_at(texts, col)) {
            return true;
        }
    }
    c.trailer.iter().enumerate().any(|(i, texts)| (0..cols).any(|col| cell(1 + c.rows.len() + i, col) != text_at(texts, col)))
}

/// A cell a rebuild makes (`new PCB_TABLECELL( this )` with the table's layer): KiCad 25ec2335 no longer mirrors a board cell for a back layer.
fn new_cell() -> TableCell {
    TableCell::pcb_new(false)
}

/// `migrateRows`: each row's cells to the row that reports the same key (the heading matched from the end of its run, the trailer after the last data row), a cell for every row
/// nothing carries; `None` when the old table has no keys to match by.
fn migrate_rows(t: &Table, row_keys: &BTreeMap<u32, String>, rows: usize, cols: usize, new_keys: &[String]) -> Option<Vec<TableCell>> {
    let old_cols = t.col_count();
    if old_cols == 0 || cols == 0 || t.cells.is_empty() || row_keys.is_empty() {
        return None;
    }
    let old_rows = t.cells.len() / old_cols;
    let mut by_key: BTreeMap<&str, usize> = BTreeMap::new();
    let (mut first, mut last) = (old_rows as i64, -1i64);
    for (&r, k) in row_keys {
        if (r as usize) < old_rows {
            by_key.insert(k.as_str(), r as usize);
            first = first.min(r as i64);
            last = last.max(r as i64);
        }
    }
    if by_key.is_empty() {
        return None;
    }
    let data = new_keys.iter().filter(|k| !k.is_empty()).count();
    let mut source: Vec<Option<usize>> = vec![None; rows];
    // The heading: the row before the first data row.
    if first - 1 >= 0 {
        source[0] = Some((first - 1) as usize);
    }
    for (i, slot) in source.iter_mut().enumerate().skip(1) {
        if !new_keys[i].is_empty() {
            *slot = by_key.get(new_keys[i].as_str()).copied();
        }
    }
    let trailing = 1 + data;
    for (i, slot) in source.iter_mut().enumerate().skip(trailing) {
        let old = last + 1 + (i - trailing) as i64;
        if (old as usize) < old_rows {
            *slot = Some(old as usize);
        }
    }
    let mut out = Vec::with_capacity(rows * cols);
    for src in source.iter().take(rows) {
        for col in 0..cols {
            match src {
                Some(r) if col < old_cols.min(cols) => out.push(t.cells[r * old_cols + col].clone()),
                _ => out.push(new_cell()),
            }
        }
    }
    Some(out)
}

/// `PCB_GENERATED_TABLE::RebuildCells` with `PCB_DRILL_CHART::onRebuilt`: the cells laid out for `c` -- a cell kept where its row still reports the same thing (so its looks and
/// uuid survive), a new one elsewhere -- their text and the column's alignment set, the table sized to its text (`Autosize`, no column narrower than its set width) and left
/// where its first corner was; the row keys, row shapes and symbol column of the chart follow.
///
/// `RebuildCells` first widens a column to its widest unwrapped text so that `Autosize`, which measures the text as wrapped to its cell, does not see it wrapped; the port's
/// `autosize` measures the text unwrapped, so the step has nothing to do here.
pub fn rebuild(t: &mut Table, chart: &mut DrillChart, c: &ChartContent) {
    let cols = chart.columns.len().max(1);
    let data = c.rows.len();
    let rows = 1 + data + c.trailer.len();
    let anchor = if t.cells.is_empty() { Point { x: Nm::ZERO, y: Nm::ZERO } } else { t.at };
    let mut new_keys = vec![String::new(); rows];
    for (i, (k, _)) in c.rows.iter().enumerate() {
        new_keys[1 + i] = k.clone();
    }
    let mut cells = match migrate_rows(t, &chart.row_keys, rows, cols, &new_keys) {
        Some(cells) => cells,
        // `ResizeCells`: the cells kept by their place in the list, new ones added or the last taken away.
        None => {
            let mut cells = t.cells.clone();
            cells.truncate(rows * cols);
            while cells.len() < rows * cols {
                cells.push(new_cell());
            }
            cells
        }
    };
    let mut set = |r: usize, col: usize, text: &str| {
        let cell = &mut cells[r * cols + col];
        cell.text = text.to_string();
        cell.style.h_align = justify_of(chart.columns[col].align);
    };
    for col in 0..cols.min(chart.columns.len()) {
        let heading = chart.columns[col].heading.clone();
        set(0, col, &heading);
    }
    for (i, (_, texts)) in c.rows.iter().enumerate() {
        for col in 0..chart.columns.len() {
            set(1 + i, col, text_at(texts, col));
        }
    }
    for (i, texts) in c.trailer.iter().enumerate() {
        for col in 0..chart.columns.len() {
            set(1 + data + i, col, text_at(texts, col));
        }
    }
    t.cells = cells;
    t.col_widths = vec![Nm::ZERO; cols];
    t.row_heights = vec![Nm::ZERO; rows];
    crate::pcb_table_builders::autosize(t);
    for (col, def) in chart.columns.iter().enumerate() {
        if def.width_nm > t.col_widths[col] {
            t.col_widths[col] = def.width_nm;
        }
    }
    t.at = anchor;
    chart.row_keys = new_keys.iter().enumerate().filter(|(_, k)| !k.is_empty()).map(|(i, k)| (i as u32, k.clone())).collect();
    chart.row_shapes = c.row_shapes.iter().enumerate().filter_map(|(i, s)| s.map(|s| (1 + i as u32, s))).collect();
    chart.symbol_column = c.symbol_column;
}

/// A new chart's table (`PCB_DRILL_CHART( board )`: no cells yet, lines 0.1 mm wide), on `layer` with its first corner at `at`.
pub fn new_chart_table(layer: &str, at: Point) -> Table {
    let mut t = Table::new_grid(layer, at, 1, 1, Nm::ZERO, Nm::ZERO, Nm::new(100_000));
    t.cells.clear();
    t.col_widths.clear();
    t.row_heights.clear();
    t
}

impl super::Board<'_> {
    /// `RefreshGeneratedTables`: every drill chart that is stale rebuilt against one pending copy of the board's profile, which becomes the board's once they all have. True
    /// when a chart was rebuilt.
    pub fn refresh_drill_charts(&mut self) -> bool {
        let Some(dr) = self.design.drawings.as_ref() else { return false };
        if dr.board_items.drill_charts.is_empty() {
            return false;
        }
        let (_, stackup) = self.effective_stackup();
        let mut profile = profile_of(&self.design);
        let charts = dr.board_items.drill_charts.clone();
        let generation = drill_generation(&self.design, self.model);
        let mut rebuilt: Vec<(Table, DrillChart)> = Vec::new();
        for chart in &charts {
            let Some(t) = dr.tables.iter().find(|t| t.id == chart.table_id) else { continue };
            // `PCB_DRILL_CHART::IsStale`: `m_builtGeneration != aBoard.GetDrillModelGeneration()`. A chart whose cells were edited by hand keeps them until the holes change.
            if chart.built_generation.as_deref() == Some(generation.as_str()) {
                continue;
            }
            let c = content(&self.design, self.model, &stackup, chart, &mut profile, true);
            let (mut t, mut chart) = (t.clone(), chart.clone());
            rebuild(&mut t, &mut chart, &c);
            // `onRebuilt`: `m_builtGeneration = aBoard.GetDrillModelGeneration()`.
            chart.built_generation = Some(generation.clone());
            rebuilt.push((t, chart));
        }
        if rebuilt.is_empty() {
            return false;
        }
        let dr = self.drawings_mut();
        for (t, chart) in rebuilt {
            if let Some(slot) = dr.tables.iter_mut().find(|x| x.id == t.id) {
                *slot = t;
            }
            if let Some(slot) = dr.board_items.drill_charts.iter_mut().find(|x| x.table_id == chart.table_id) {
                *slot = chart;
            }
        }
        dr.board_items.drill_profile = (profile != DrillSymbolProfile::default()).then_some(profile);
        true
    }

    /// `PCB_CONTROL::PlaceDrillChart` (`placeGeneratedTable`): a new chart of the default template on `layer`, built from the board and put with its first corner at `at`;
    /// the marks its rows were given become the board's profile.
    pub(crate) fn place_drill_chart(&mut self, layer: &str, at: Point) -> Result<(), Vec<eda_model::CheckResult>> {
        if !eda_model::drill_chart::is_documentation_layer(layer) {
            return Err(vec![eda_model::CheckResult::fail("ops_bad_drill_chart", layer, "No documentation layer is enabled to hold a drill chart.".to_string())]);
        }
        let (_, stackup) = self.effective_stackup();
        let mut profile = profile_of(&self.design);
        let mut t = new_chart_table(layer, at);
        let mut chart = DrillChart::new("");
        let c = content(&self.design, self.model, &stackup, &chart, &mut profile, true);
        rebuild(&mut t, &mut chart, &c);
        chart.built_generation = Some(drill_generation(&self.design, self.model));
        t.at = at;
        let dr = self.drawings_mut();
        let before: BTreeSet<String> = dr.tables.iter().map(|t| t.id.clone()).collect();
        dr.tables.push(t);
        dr.assign_missing_ids();
        let id = dr.tables.iter().find(|t| !before.contains(&t.id)).map(|t| t.id.clone()).unwrap_or_default();
        chart.table_id = id;
        dr.board_items.drill_charts.push(chart);
        dr.board_items.drill_profile = (profile != DrillSymbolProfile::default()).then_some(profile);
        Ok(())
    }

    /// The Properties panel's edits of a chart (`Units`, `Decimal Places`, `Show Totals`, the filter): the settings, then the chart rebuilt from the board.
    pub(crate) fn set_drill_chart(&mut self, table_id: &str, units: TableUnits, precision: i32, show_totals: bool, filter: DrillChartFilter) -> Result<(), Vec<eda_model::CheckResult>> {
        let dr = self.drawings_mut();
        let chart = dr.board_items.drill_charts.iter_mut().find(|c| c.table_id == table_id).ok_or_else(|| vec![eda_model::CheckResult::fail("ops_unknown_drill_chart", table_id, "no drill chart with this id".to_string())])?;
        chart.units = units;
        chart.precision = precision.clamp(0, 6);
        chart.show_totals = show_totals;
        chart.filter = filter;
        // `PCB_PROPERTIES_PANEL::valueChanged`: a generated table's edit is followed by `RebuildCells` of that table, whatever its generation.
        chart.built_generation = None;
        self.refresh_drill_charts();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use eda_model::units::mm;
    use super::*;

    #[test]
    fn lengths_are_written_as_the_generated_tables_write_them() {
        assert_eq!(format_length(mm(0.3), TableUnits::Mm, 3), "0.300");
        assert_eq!(format_length(mm(1.143), TableUnits::Mm, 2), "1.14");
        assert_eq!(format_length(mm(1.524), TableUnits::Inch, 3), "0.0600\u{2033}");
        assert_eq!(format_length(mm(1.524), TableUnits::Mils, 3), "60.0 mil");
        assert_eq!(format_length(mm(1.524), TableUnits::Mils, 1), "60 mil");
    }

    #[test]
    fn marks_are_assigned_from_the_lowest_free_shape_and_then_letters() {
        let op = |d: Nm| DrillOp { source: String::new(), at: Point { x: Nm::ZERO, y: Nm::ZERO }, size: (d, d), orientation_deg: 0.0, diameter: d, is_slot: false, top: "F.Cu".into(), bottom: "B.Cu".into(), not_plated: false, attribute: HoleAttribute::Pad, protection: [false; 8], kind: 0, post_machining: Default::default(), stub_length: None };
        let p = DrillSymbolProfile::default();
        assert_eq!(group_key(&p, &op(mm(0.3))), "v1|d300000|s-|p1|L0-31|k0|m0:0:0:0/0:0:0:0");
        let mut profile = DrillSymbolProfile::default();
        profile.assignments.insert(group_key(&p, &op(mm(0.4))), DrillSymbolAssignment { mode: DrillMarkMode::Shape, shape_index: 0, ..Default::default() });
        let mut gs: Vec<DrillGroup> = (0..14).map(|i| DrillGroup { key: group_key(&p, &op(mm(0.3) + i * mm(0.1))), first: op(mm(0.3) + i * mm(0.1)), operations: 1, sites: BTreeSet::new(), symbol: Default::default() }).collect();
        assign(&mut gs, &mut profile);
        assert_eq!((gs[0].symbol.mode, gs[0].symbol.shape_index), (DrillMarkMode::Shape, 1), "shape 0 is the frozen 0.4 mm group's");
        assert_eq!(gs[1].symbol.shape_index, 0);
        assert_eq!((gs[12].symbol.mode, gs[12].symbol.letter.as_str()), (DrillMarkMode::Letter, "A"));
        assert_eq!(gs[13].symbol.letter, "B");
        assert_eq!(profile.assignments.len(), 14);
    }
}
