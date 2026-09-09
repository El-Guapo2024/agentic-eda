//! Bookshelf (UCLA) placement format — the interface to Cypress / DREAMPlace.
//!
//! Cypress (NVlabs, ISPD'25) is a GPU analytical placer built on DREAMPlace;
//! it reads the classic Bookshelf quintet (.aux/.nodes/.nets/.pl/.scl) and
//! writes a `.gp.pl` with the global placement. We emit Bookshelf straight
//! from our IR (no KiCad round-trip) and read the `.pl` back into a
//! `PlacementSection`, so Cypress is just another generator judged by our
//! own gates and HPWL — exactly like `eda-place`.
//!
//! Conventions (matching NVlabs' own `pcb-util/bookshelf_converter.py`):
//! * integer units; we use `unit_um` micrometres per unit (default 100),
//! * node size = courtyard, pin offsets relative to the node centre,
//! * every node movable (NumTerminals 0), orientation `N`,
//! * `.scl` rows of height 1 tiling the outline bounding box, so any
//!   integer position is a legal site,
//! * `.pl` positions are the node's lower-left corner (Bookshelf), while
//!   our IR stores centres — converted both ways.

use eda_model::footprint::{keepout_for, placed_courtyard, refdes_font_um, to_board};
use eda_model::ir::{Design, FootprintInstance, PlacementSection, Point, Side};
use eda_model::{CheckResult, ConstraintModel};
use std::collections::BTreeMap;
use std::fmt::Write as _;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bookshelf {
    pub aux: String,
    pub nodes: String,
    pub nets: String,
    pub pl: String,
    pub scl: String,
    /// UCLA `.wts` — per-net weight overrides (default weight is 1.0 for
    /// any net not listed here). Used to carry `placement_rules`
    /// (`Proximity`) into Cypress/DREAMPlace's wirelength objective: see
    /// `PROXIMITY_NET_PREFIX` below. Empty (header only) when there are no
    /// proximity rules.
    pub wts: String,
}

impl Bookshelf {
    /// `(file name, content)` for every file, given the design name.
    pub fn files(&self, name: &str) -> Vec<(String, &str)> {
        vec![
            (format!("{name}.aux"), self.aux.as_str()),
            (format!("{name}.nodes"), self.nodes.as_str()),
            (format!("{name}.nets"), self.nets.as_str()),
            (format!("{name}.pl"), self.pl.as_str()),
            (format!("{name}.scl"), self.scl.as_str()),
            (format!("{name}.wts"), self.wts.as_str()),
        ]
    }
}

/// Net-name prefix for the synthetic 2-pin nets we add per `Proximity`
/// rule, so `.wts` can target them without touching any real net's weight.
pub const PROXIMITY_NET_PREFIX: &str = "prox";

fn div_ceil(v: i64, unit: i64) -> i64 {
    if v >= 0 { (v + unit - 1) / unit } else { -((-v) / unit) }
}

fn div_round(v: i64, unit: i64) -> i64 {
    if v >= 0 {
        (v + unit / 2) / unit
    } else {
        -((-v + unit / 2) / unit)
    }
}

/// Default `.wts` weight for the synthetic proximity nets (see
/// [`to_bookshelf_weighted`]) when a caller doesn't need to tune it.
pub const DEFAULT_PROXIMITY_WEIGHT: f64 = 50.0;

/// Same as [`to_bookshelf_weighted`] with [`DEFAULT_PROXIMITY_WEIGHT`].
/// Kept so existing callers (e.g. `eda export`, which has no proximity
/// tuning knob) don't need to change.
pub fn to_bookshelf(design: &Design, model: &ConstraintModel, name: &str, unit_um: i64) -> Result<Bookshelf, Vec<CheckResult>> {
    to_bookshelf_weighted(design, model, name, unit_um, DEFAULT_PROXIMITY_WEIGHT)
}

/// Export the design's parts as a Bookshelf problem. Uses the existing
/// placement (or the model's outline / an auto square) for the board
/// extent and initial positions; parts without a placement start at the
/// board centre.
pub fn to_bookshelf_weighted(design: &Design, model: &ConstraintModel, name: &str, unit_um: i64, proximity_weight: f64) -> Result<Bookshelf, Vec<CheckResult>> {
    let unit = unit_um.max(1);
    // Board extent.
    let outline: Vec<Point> = match design.placement.as_ref().map(|p| p.outline.clone()).or_else(|| model.board.outline.clone()) {
        Some(o) if o.len() >= 3 => o,
        _ => return Err(vec![CheckResult::fail("bookshelf_outline", "design", "no board outline (place first or set board.outline)")]),
    };
    let min_x = outline.iter().map(|p| p.x).min().unwrap();
    let min_y = outline.iter().map(|p| p.y).min().unwrap();
    let max_x = outline.iter().map(|p| p.x).max().unwrap();
    let max_y = outline.iter().map(|p| p.y).max().unwrap();
    // Board rounds DOWN so every legal Cypress position lies inside the outline.
    let bw = ((max_x - min_x) / unit).max(1);
    let bh = ((max_y - min_y) / unit).max(1);
    let font = refdes_font_um(&outline);

    let placed: BTreeMap<&str, &FootprintInstance> =
        design.placement.as_ref().map(|p| p.footprints.iter().map(|f| (f.id.as_str(), f)).collect()).unwrap_or_default();

    let mut parts: Vec<&eda_model::Part> = model.parts.iter().collect();
    parts.sort_by(|a, b| a.reference.cmp(&b.reference));

    let mut nodes = String::new();
    let mut pl = String::new();
    let mut fails = Vec::new();
    // Per part: centre offset of each pad in units (rotation 0 frame).
    let mut pad_offsets: BTreeMap<String, BTreeMap<String, (i64, i64)>> = BTreeMap::new();
    // Per part: node centre in units, used only to pick the nearest pad
    // pair for a synthetic proximity net below (heuristic, ignores
    // rotation — Cypress' own optimisation corrects any slack).
    let mut centers_units: BTreeMap<String, (i64, i64)> = BTreeMap::new();
    let stamp = format!("generated by eda-interchange from design {}", design.provenance.intent_hash);

    for part in &parts {
        let Some(fp) = model.footprint_of(part) else {
            fails.push(CheckResult::fail("bookshelf_footprint", &part.reference, "no footprint geometry"));
            continue;
        };
        let (hw, hh) = fp.courtyard_half();
        // The node is the part's keep-out: courtyard plus the refdes label
        // above it (gate `placement_refdes_clear`), so Cypress reserves
        // label space and no neighbour's pad ends up under a label. The
        // node centre is offset (ox, oy) from the part centre.
        let ko = keepout_for((-hw, -hh, hw, hh), &part.reference, font, i64::MIN);
        let (ox, oy) = ((ko.0 + ko.2) / 2, (ko.1 + ko.3) / 2);
        // Sizes round UP: Cypress packs nodes flush on its integer grid, so a
        // rounded-down courtyard would overlap its neighbour at µm precision.
        let w = div_ceil(ko.2 - ko.0, unit).max(1);
        let h = div_ceil(ko.3 - ko.1, unit).max(1);
        writeln!(nodes, "{} {} {}", part.reference, w, h).unwrap();
        let offs: BTreeMap<String, (i64, i64)> =
            fp.pads.iter().map(|p| (p.number.clone(), (div_round(p.at.0 - ox, unit), div_round(p.at.1 - oy, unit)))).collect();
        pad_offsets.insert(part.reference.clone(), offs);
        // Initial position: lower-left corner in units.
        let (cx, cy) = match placed.get(part.reference.as_str()) {
            Some(f) => (f.at.x - min_x + ox, f.at.y - min_y + oy),
            None => ((max_x - min_x) / 2, (max_y - min_y) / 2),
        };
        let llx = div_round(cx, unit) - w / 2;
        let lly = div_round(cy, unit) - h / 2;
        centers_units.insert(part.reference.clone(), (llx + w / 2, lly + h / 2));
        writeln!(pl, "{} {} {} : N", part.reference, llx.max(0), lly.max(0)).unwrap();
    }
    if !fails.is_empty() {
        return Err(fails);
    }

    // The Bookshelf lexer treats a fixed word list as keywords (case-
    // insensitive), so a net called `SCL` or a part called `END` breaks the
    // parser. Nets never round-trip, so they are emitted by index; a refdes
    // does round-trip through the .pl, so a colliding one is a hard error.
    for part in &parts {
        if is_bookshelf_keyword(&part.reference) {
            fails.push(CheckResult::fail("bookshelf_reserved_name", part.reference.clone(),
                "reference designator collides with a Bookshelf keyword; rename the part"));
        }
    }
    if !fails.is_empty() {
        return Err(fails);
    }
    let mut nets = String::new();
    let mut num_nets = 0usize;
    let mut num_pins = 0usize;
    let mut net_body = String::new();
    for (net_idx, net) in model.nets.iter().enumerate() {
        let pins: Vec<(&str, (i64, i64))> = net
            .pins
            .iter()
            .filter_map(|p| {
                let (r, n) = p.split_once('.')?;
                let off = pad_offsets.get(r)?.get(n)?;
                Some((r, *off))
            })
            .collect();
        if pins.len() < 2 {
            continue;
        }
        num_nets += 1;
        num_pins += pins.len();
        writeln!(net_body, "NetDegree : {} n{net_idx}", pins.len()).unwrap();
        for (r, (ox, oy)) in pins {
            writeln!(net_body, "\t{r} I : {ox} {oy}").unwrap();
        }
    }
    // Carry `placement_rules: Proximity` into Cypress's wirelength
    // objective: a dedicated 2-pin net per rule, between the pad of `a`
    // nearest `b` and the pad of `b` nearest `a`, weighted (via `.wts`)
    // well above 1.0 so it actually pulls the pair together — a real net
    // shared with unrelated pins would drag those along too. The gate
    // (`placement_proximity`) still measures courtyard gap, not this net;
    // this only shapes what the optimiser is pulled toward.
    let mut wts = String::new();
    let mut wts_body = String::new();
    let mut num_wts = 0usize;
    for (rule_idx, rule) in model.placement_rules.iter().enumerate() {
        let eda_model::PlacementRule::Proximity { a, b, .. } = rule else { continue };
        let (Some(offs_a), Some(offs_b)) = (pad_offsets.get(a), pad_offsets.get(b)) else { continue };
        let (Some(&ca), Some(&cb)) = (centers_units.get(a), centers_units.get(b)) else { continue };
        let nearest = |offs: &BTreeMap<String, (i64, i64)>, center: (i64, i64), target: (i64, i64)| {
            offs.iter()
                .map(|(n, off)| {
                    let (px, py) = (center.0 + off.0, center.1 + off.1);
                    let d2 = (px - target.0).pow(2) + (py - target.1).pow(2);
                    (d2, n.clone(), *off)
                })
                .min_by_key(|(d2, n, _)| (*d2, n.clone()))
        };
        let (Some((_, _, off_a)), Some((_, _, off_b))) = (nearest(offs_a, ca, cb), nearest(offs_b, cb, ca)) else { continue };
        num_nets += 1;
        num_pins += 2;
        num_wts += 1;
        let net_name = format!("{PROXIMITY_NET_PREFIX}{rule_idx}");
        writeln!(net_body, "NetDegree : 2 {net_name}").unwrap();
        writeln!(net_body, "\t{a} I : {} {}", off_a.0, off_a.1).unwrap();
        writeln!(net_body, "\t{b} I : {} {}", off_b.0, off_b.1).unwrap();
        writeln!(wts_body, "{net_name} {proximity_weight}").unwrap();
    }
    writeln!(wts, "UCLA wts 1.0\n\n# {stamp}\n").unwrap();
    wts.push_str(&wts_body);
    let _ = num_wts;

    writeln!(nets, "UCLA nets 1.0\n\nNumNets : {num_nets}\nNumPins : {num_pins}\n\n# {stamp}\n").unwrap();
    nets.push_str(&net_body);

    let nodes_hdr = format!("UCLA nodes 1.0\n\nNumNodes : {}\nNumTerminals : 0\n\n# {stamp}\n\n", parts.len());
    let pl_hdr = format!("UCLA pl 1.0\n\n# {stamp}\n\n");

    let mut scl = String::new();
    writeln!(scl, "UCLA scl 1.0\n\n# {stamp}\n\nNumRows : {bh}\n").unwrap();
    for row in 0..bh {
        writeln!(
            scl,
            "CoreRow Horizontal\n    Coordinate   : {row}\n    Height       : 1\n    Sitewidth    : 1\n    Sitespacing  : 1\n    Siteorient   : {}\n    Sitesymmetry : 1\n    SubrowOrigin : 0  NumSites : {bw}\nEnd\n",
            if row % 2 == 0 { 1 } else { 0 }
        )
        .unwrap();
    }
    // Order in the `.aux` list doesn't matter to the parser (it sorts by
    // suffix), but `.wts` must be listed for Cypress to read it at all.
    let aux = format!("RowBasedPlacement : {name}.nodes {name}.nets {name}.wts {name}.pl {name}.scl\n");
    Ok(Bookshelf { aux, nodes: nodes_hdr + &nodes, nets, pl: pl_hdr + &pl, scl, wts })
}

/// Read a Bookshelf `.pl` (as written by DREAMPlace/Cypress) back into a
/// placement: lower-left corners in units → centres in µm, relative to the
/// outline origin used at export. Rotation from the orientation token
/// (`N`=0, `W`=90, `S`=180, `E`=270; `F*` flips to the bottom side).
/// Words the UCLA Bookshelf lexer reserves (matched case-insensitively).
pub fn is_bookshelf_keyword(name: &str) -> bool {
    const KW: &[&str] = &[
        "ucla", "numnodes", "numterminals", "numnets", "numpins", "netdegree", "numrows",
        "corerow", "horizontal", "vertical", "coordinate", "height", "sitewidth", "sitespacing",
        "siteorient", "sitesymmetry", "subroworigin", "numsites", "end", "terminal", "terminal_ni",
        "scl", "nodes", "nets", "pl", "wts", "aux", "rowbasedplacement", "n", "s", "e", "w",
        "fn", "fs", "fe", "fw", "i", "o", "b",
    ];
    let l = name.to_ascii_lowercase();
    KW.contains(&l.as_str())
}

pub fn from_bookshelf_pl(pl: &str, design: &Design, model: &ConstraintModel, unit_um: i64) -> Result<Design, Vec<CheckResult>> {
    let unit = unit_um.max(1);
    let outline: Vec<Point> = match design.placement.as_ref().map(|p| p.outline.clone()).or_else(|| model.board.outline.clone()) {
        Some(o) if o.len() >= 3 => o,
        _ => return Err(vec![CheckResult::fail("bookshelf_outline", "design", "no board outline")]),
    };
    let min_x = outline.iter().map(|p| p.x).min().unwrap();
    let min_y = outline.iter().map(|p| p.y).min().unwrap();
    let font = refdes_font_um(&outline);

    let mut footprints = Vec::new();
    let mut fails = Vec::new();
    for line in pl.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with("UCLA") {
            continue;
        }
        let mut it = line.split_whitespace();
        let (Some(name), Some(xs), Some(ys)) = (it.next(), it.next(), it.next()) else { continue };
        let (Ok(x), Ok(y)) = (xs.parse::<f64>(), ys.parse::<f64>()) else { continue };
        let orient = it.find(|t| *t != ":").unwrap_or("N");
        let Some(part) = model.part(name) else {
            fails.push(CheckResult::fail("bookshelf_unknown_node", name, "node is not a part in the model"));
            continue;
        };
        let Some(fp) = model.footprint_of(part) else { continue };
        let (hw, hh) = fp.courtyard_half();
        let (rot, side) = match orient.trim_start_matches('F') {
            "W" => (90_000u32, orient.starts_with('F')),
            "S" => (180_000, orient.starts_with('F')),
            "E" => (270_000, orient.starts_with('F')),
            _ => (0, orient.starts_with('F')),
        };
        let (w, h) = if rot == 90_000 || rot == 270_000 { (hh * 2, hw * 2) } else { (hw * 2, hh * 2) };
        // Node = keep-out (courtyard + label), see `to_bookshelf_weighted`;
        // undo the node-centre offset to recover the part centre.
        let ko = keepout_for((-w / 2, -h / 2, w / 2, h / 2), name, font, i64::MIN);
        let (ox, oy) = ((ko.0 + ko.2) / 2, (ko.1 + ko.3) / 2);
        let cx = min_x + (x * unit as f64).round() as i64 + (ko.2 - ko.0) / 2 - ox;
        let cy = min_y + (y * unit as f64).round() as i64 + (ko.3 - ko.1) / 2 - oy;
        footprints.push(FootprintInstance {
            id: name.to_string(),
            at: Point { x: cx, y: cy },
            rot,
            side: if side { Side::Bottom } else { Side::Top },
        });
    }
    if !fails.is_empty() {
        return Err(fails);
    }
    footprints.sort_by(|a, b| a.id.cmp(&b.id));
    let mut out = design.clone();
    out.placement = Some(PlacementSection { outline, footprints });
    out.routing = None;
    // Sanity: every part came back.
    let missing: Vec<&str> = model.parts.iter().map(|p| p.reference.as_str()).filter(|r| !out.placement.as_ref().unwrap().footprints.iter().any(|f| f.id == *r)).collect();
    if !missing.is_empty() {
        return Err(vec![CheckResult::fail("bookshelf_missing_nodes", "pl", format!("parts absent from .pl: {}", missing.join(", ")))]);
    }
    let _ = (placed_courtyard, to_board);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use eda_model::ir::Provenance;
    use eda_model::{Net, Part, Pin, PinKind};

    fn part(r: &str, pkg: &str, n: usize) -> Part {
        Part {
            reference: r.into(),
            mpn: None,
            value: None,
            package: Some(pkg.into()),
            footprint: None,
            pins: (1..=n).map(|i| Pin { number: i.to_string(), name: None, kind: PinKind::Signal }).collect(),
        }
    }

    fn fixture() -> (Design, ConstraintModel) {
        let model = ConstraintModel {
            parts: vec![part("U1", "SOIC-8", 8), part("C1", "0603", 2), part("R1", "0603", 2)],
            nets: vec![
                Net { name: "A".into(), pins: vec!["U1.1".into(), "C1.1".into(), "R1.1".into()] },
                Net { name: "B".into(), pins: vec!["U1.2".into(), "R1.2".into()] },
            ],
            ..Default::default()
        };
        let outline = vec![Point { x: 0, y: 0 }, Point { x: 20_000, y: 0 }, Point { x: 20_000, y: 15_000 }, Point { x: 0, y: 15_000 }];
        let design = Design {
            schema: 1,
            provenance: Provenance { engine_version: "t".into(), intent_hash: "h".into(), seed: 0, stage_hashes: vec![] },
            schematic: None,
            placement: Some(PlacementSection {
                outline,
                footprints: vec![
                    FootprintInstance { id: "U1".into(), at: Point { x: 10_000, y: 7_500 }, rot: 0, side: Side::Top },
                    FootprintInstance { id: "C1".into(), at: Point { x: 3_000, y: 3_000 }, rot: 0, side: Side::Top },
                    FootprintInstance { id: "R1".into(), at: Point { x: 17_000, y: 12_000 }, rot: 0, side: Side::Top },
                ],
            }),
            routing: None,
        };
        (design, model)
    }

    #[test]
    fn writes_all_six_files_with_headers() {
        let (d, m) = fixture();
        let bs = to_bookshelf(&d, &m, "t", 100).unwrap();
        assert!(bs.aux.starts_with("RowBasedPlacement : t.nodes t.nets t.wts t.pl t.scl"));
        assert!(bs.nodes.contains("NumNodes : 3"));
        assert!(bs.nets.contains("NumNets : 2") && bs.nets.contains("NumPins : 5"));
        assert!(bs.scl.contains("NumRows : 150"));
        assert!(bs.nets.contains("NetDegree : 3 n0"));
        assert!(bs.wts.starts_with("UCLA wts 1.0"));
        assert_eq!(bs.files("t").len(), 6);
    }

    #[test]
    fn proximity_rule_emits_weighted_synthetic_net() {
        let (d, m) = fixture();
        let mut m = m;
        m.placement_rules.push(eda_model::PlacementRule::Proximity { a: "U1".into(), b: "C1".into(), max_mm: 3.0 });
        let bs = to_bookshelf_weighted(&d, &m, "t", 100, 77.0).unwrap();
        assert!(bs.nets.contains("NetDegree : 2 prox0"));
        assert!(bs.nets.contains("U1 I :"));
        assert!(bs.wts.contains("prox0 77"));
    }

    #[test]
    fn pl_round_trip_recovers_centres() {
        let (d, m) = fixture();
        let bs = to_bookshelf(&d, &m, "t", 100).unwrap();
        let back = from_bookshelf_pl(&bs.pl, &d, &m, 100).unwrap();
        for f in &back.placement.as_ref().unwrap().footprints {
            let orig = d.placement.as_ref().unwrap().footprints.iter().find(|o| o.id == f.id).unwrap();
            assert!((f.at.x - orig.at.x).abs() <= 100 && (f.at.y - orig.at.y).abs() <= 100, "{}: {:?} vs {:?}", f.id, f.at, orig.at);
        }
    }

    #[test]
    fn orientation_tokens_map_to_rotation_and_side() {
        let (d, m) = fixture();
        let pl = "UCLA pl 1.0\nU1 10 10 : W\nC1 0 0 : FN\nR1 5 5 : S\n";
        let back = from_bookshelf_pl(pl, &d, &m, 100).unwrap();
        let p = back.placement.unwrap();
        let get = |id: &str| p.footprints.iter().find(|f| f.id == id).unwrap().clone();
        assert_eq!(get("U1").rot, 90_000);
        assert_eq!(get("C1").side, Side::Bottom);
        assert_eq!(get("R1").rot, 180_000);
    }

    #[test]
    fn missing_node_is_an_error() {
        let (d, m) = fixture();
        let err = from_bookshelf_pl("UCLA pl 1.0\nU1 0 0 : N\n", &d, &m, 100).unwrap_err();
        assert_eq!(err[0].check, "bookshelf_missing_nodes");
    }
}
