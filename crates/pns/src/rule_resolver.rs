//! Port of `PNS_PCBNEW_RULE_RESOLVER` (`pcbnew/router/pns_kicad_iface.cpp`): the clearance the router asks of two of its items, answered by
//! the board's DRC rule engine (`m_board->GetDesignSettings().m_DRCEngine`, here [`eda_drc::constraints::RuleEngine`] over the items of
//! [`eda_drc::rule_board::RuleBoard`]), and the router's view of keepout rule areas, net ties and holes.
//!
//! `Clearance( aA, aB, aUseClearanceEpsilon )` takes, on every layer the two share, the largest of `CT_HOLE_TO_HOLE` (two drilled holes),
//! `CT_HOLE_CLEARANCE` (a hole and anything), `CT_CLEARANCE` (two copper items), `CT_EDGE_CLEARANCE` (an Edge.Cuts graphic or a non-plated
//! slot) and `CT_PHYSICAL_CLEARANCE`, each `QueryConstraint`'s `DRC_ENGINE::EvalRules` of the two board items -- so the net classes, a pad's or
//! a footprint's local clearance, the net-tie rule ("Net tie on %s; clearance: 0."), the custom rules of the `.kicad_dru` and the board
//! minimum all count, as they do in KiCad -- less the DRC epsilon when asked. An item the router made (the head, a shoved line) has no
//! board item: `getBoardItem` gives it a dummy track or via of its net on the layer, with its anchors (and no width of its own: a
//! `PCB_TRACK`'s default 0.2 mm), and so it is here. A track or via the board has is asked about as such a dummy too (**differs**: KiCad
//! asks the `PCB_TRACK` itself, which has its own width; a rule that reads `A.Width` of an existing track sees 0.2 mm here).
//!
//! The answers are cached per pair of items as KiCad's `m_clearanceCache` / `m_tempClearanceCache` are; the key of a dummy is its net and
//! layer, and its anchors only when a rule's condition looks at shapes (`HasGeometryDependentRules`). KiCad evaluates a dummy line of several
//! segments segment by segment then and takes the smallest (`evaluateLineSegments`); here every query is of one segment already, except the
//! hulls of `NearestObstacle`, which ask of the line's first segment (**differs**, only with geometry-dependent rules).

use crate::item::{Item, Net, Solid};
use crate::layer::LayerRange;
use eda_drc::constraints::{CType, ImplicitSource, RuleEngine, Severity};
use eda_drc::kimath::Shape;
use eda_drc::rule_item::{Class, Prop, RuleItem};
use eda_model::ir::{Design, Nm, Point};
use eda_model::{BoardRules, ConstraintModel};
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

/// `GetDRCEpsilon()` (`ADVANCED_CFG::m_DRCEpsilon`, 0.0005 mm): what `Clearance( .., aUseClearanceEpsilon )` takes off.
pub const DRC_EPSILON: Nm = Nm::new(500);

/// One side of a clearance query: what `PNS::ITEM` the rule resolver is asked about.
#[derive(Clone, Copy, Debug)]
pub enum Side<'a> {
    /// A track (`SEGMENT_T` / `ARC_T` / `LINE_T`) of `net` on `layer`: the head, a line of the shove, a track of the board. `seg` is the
    /// segment it is, for a rule that looks at shapes.
    Track { net: &'a Net, layer: i32, seg: Option<(Point, Point)> },
    /// A via's copper (`VIA_T`).
    Via { net: &'a Net, layers: LayerRange, pos: Point },
    /// A via's hole (`HOLE_T` whose `ParentPadVia` is a via): drilled, plated.
    ViaHole { net: &'a Net, layers: LayerRange, pos: Point },
    /// An item of the world: its copper.
    Item(&'a Item),
    /// The hole of a pad of the world (`HOLE_T` of a `SOLID`).
    PadHole(&'a Solid),
}

impl<'a> Side<'a> {
    pub fn track(net: &'a Net, layer: i32) -> Side<'a> {
        Side::Track { net, layer, seg: None }
    }

    pub fn via(v: &'a crate::item::Via) -> Side<'a> {
        Side::Via { net: &v.net, layers: v.layers, pos: v.pos }
    }

    pub fn via_hole(v: &'a crate::item::Via) -> Side<'a> {
        Side::ViaHole { net: &v.net, layers: v.layers, pos: v.pos }
    }

    /// The side a probe shape of the router is (`QueryColliding( aItem )` of a line's segment or a via): a circle is a via, anything else a
    /// track on the first of `layers`.
    pub fn of_shape(shape: &Shape, net: &'a Net, layers: LayerRange) -> Side<'a> {
        match shape {
            Shape::Circle { c, .. } => Side::Via { net, layers, pos: *c },
            Shape::Stadium { a, b, .. } => Side::Track { net, layer: layers.start(), seg: Some((*a, *b)) },
            _ => Side::Track { net, layer: layers.start(), seg: None },
        }
    }

    pub fn net(&self) -> &'a Net {
        match *self {
            Side::Track { net, .. } | Side::Via { net, .. } | Side::ViaHole { net, .. } => net,
            Side::Item(it) => it.net(),
            Side::PadHole(s) => &s.net,
        }
    }

    pub fn layers(&self) -> LayerRange {
        match *self {
            Side::Track { layer, .. } => LayerRange::single(layer),
            Side::Via { layers, .. } | Side::ViaHole { layers, .. } => layers,
            Side::Item(it) => it.layers(),
            Side::PadHole(s) => s.layers,
        }
    }

    /// `isEdge( aItem )`: a graphic on Edge.Cuts.
    pub fn is_edge(&self) -> bool {
        matches!(self, Side::Item(Item::Solid(s)) if s.edge)
    }

    /// `isHole( aItem )`.
    fn is_hole(&self) -> bool {
        matches!(self, Side::ViaHole { .. } | Side::PadHole(_))
    }

    /// `ITEM::Kind()` as `getBoardItem` and `checkKeepout` see it: is this a via (a via or its hole), a track, or a pad?
    pub fn kind(&self) -> SideKind {
        match self {
            Side::Track { .. } | Side::Item(Item::Segment(_)) | Side::Item(Item::Arc(_)) => SideKind::Track,
            Side::Via { .. } | Side::ViaHole { .. } | Side::Item(Item::Via(_)) => SideKind::Via,
            Side::Item(Item::Solid(_)) | Side::PadHole(_) => SideKind::Pad,
        }
    }
}

/// What kind of board item a side stands for, for a keepout's per-kind flags (`checkKeepout`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SideKind {
    Track,
    Via,
    Pad,
}

/// A keepout rule area (`ZONE::GetIsRuleArea() && HasKeepoutParametersSet()`) as the router sees it.
#[derive(Clone, Debug)]
pub struct Keepout {
    pub zone_id: String,
    pub tracks: bool,
    pub vias: bool,
    pub pads: bool,
    pub footprints: bool,
    /// `GetParentFootprint()` of a footprint's own rule area.
    pub parent_footprint: Option<String>,
}

impl Keepout {
    /// `PNS_PCBNEW_RULE_RESOLVER::IsKeepout`'s `checkKeepout( aKeepout, getBoardItem( aItem, .. ) )`: does the area keep this kind of item out?
    /// `GetDoNotAllowTracks() && aOther->IsType( { PCB_ARC_T, PCB_TRACE_T } )`, `GetDoNotAllowVias() && aOther->Type() == PCB_VIA_T`. A pad is
    /// never kept out by the router: `getBoardItem` gives a `SOLID` no board item (`default: return nullptr`), and `checkKeepout( .., nullptr )`
    /// is false (`GetDoNotAllowPads()` and the footprint test are for a pad that has one, which the router's items never are).
    pub fn keeps_out(&self, kind: SideKind) -> bool {
        match kind {
            SideKind::Track => self.tracks,
            SideKind::Via => self.vias,
            SideKind::Pad => false,
        }
    }
}

/// A pad of a net-tie footprint that is in one of its groups (`padToNetTieGroupMap[ pad->GetNumber() ] >= 0`), for `IsNetTieExclusion`.
#[derive(Clone, Debug)]
struct NetTiePad {
    net: Net,
    shape: Shape,
    layers: LayerRange,
}

/// A net in a cache key, by its handle (`NET_HANDLE` is a pointer in KiCad, and so is it here): `from_ir` gives every item of a net the one
/// `Rc`, so asking by the pointer is asking by the net. Two handles of one name are two keys of one answer; the key holds its `Rc`, so a
/// freed one is never mistaken for another.
#[derive(Clone, Debug)]
struct NetKey(Net);

impl PartialEq for NetKey {
    fn eq(&self, other: &Self) -> bool {
        match (&self.0, &other.0) {
            (Some(a), Some(b)) => Rc::ptr_eq(a, b),
            (None, None) => true,
            _ => false,
        }
    }
}

impl Eq for NetKey {}

impl std::hash::Hash for NetKey {
    fn hash<H: std::hash::Hasher>(&self, h: &mut H) {
        h.write_usize(self.0.as_ref().map_or(0, |r| Rc::as_ptr(r) as *const u8 as usize));
    }
}

#[derive(Clone, PartialEq, Eq, Hash, Debug)]
enum Key {
    Track(NetKey, i32, Option<(i64, i64, i64, i64)>),
    Via(NetKey, LayerRange, Option<(i64, i64)>),
    ViaHole(NetKey, LayerRange, Option<(i64, i64)>),
    /// A solid by the number its rule item has ([`RuleResolver::solid_id`]).
    Solid(u32, LayerRange),
    PadHole(u32, LayerRange),
}

/// FNV-1a: the clearance cache is asked on every collision test, its keys are small and not adversarial.
#[derive(Default)]
struct Fnv(u64);

impl std::hash::Hasher for Fnv {
    fn finish(&self) -> u64 {
        self.0
    }

    fn write(&mut self, bytes: &[u8]) {
        let mut h = if self.0 == 0 { 0xcbf2_9ce4_8422_2325 } else { self.0 };
        for b in bytes {
            h ^= *b as u64;
            h = h.wrapping_mul(0x0100_0000_01b3);
        }
        self.0 = h;
    }
}

type FnvMap<K, V> = HashMap<K, V, std::hash::BuildHasherDefault<Fnv>>;

/// The number of the board edge's rule item ([`RuleResolver::solid_id`]).
const EDGE_ID: u32 = 0;

/// What the router asks of a solid besides its rules.
#[derive(Debug, Clone, Default)]
struct SolidInfo {
    /// The drilled hole of a pad (`GetEffectiveHoleShape`), whether it is drilled (`HasDrilledHole`) and whether it is a non-plated slot.
    hole: Option<(Shape, bool, bool)>,
    /// How far inside the pad's copper its hole is at the least (0 when it is not known to be inside): see [`RuleResolver::hole_reach`].
    annulus: Nm,
    /// The rules ask more of it than of its net: a local clearance (`GetClearanceOverrides`), or it is a rule area or a text.
    special: bool,
    /// The footprint a pad is of.
    footprint: Option<String>,
    keepout: Option<Keepout>,
}

/// `PNS_PCBNEW_RULE_RESOLVER`, built once for a world ([`crate::from_ir`]).
pub struct RuleResolver {
    engine: RuleEngine,
    /// The router's layer index to the board's layer name.
    layer_names: Vec<String>,
    /// The board items of the world's solids, by number (`Solid::rule`): the board edge (0), the pads, the rule areas, the copper texts.
    items: Vec<Rc<RuleItem>>,
    /// What else the router asks of each: a pad's hole and footprint, a rule area's keepout.
    info: Vec<SolidInfo>,
    /// The number of a solid's rule item by `Solid::source` (a pad, a copper text) or `keepout:<zone id>` (a rule area).
    ids: FnvMap<String, u32>,
    /// Items made when a solid of no number is asked about (one the board does not know): an item of no net on its layer.
    extra: RefCell<FnvMap<String, (u32, Rc<RuleItem>)>>,
    /// For each net-tie footprint (`IsNetTie()`), its pads in a group.
    net_ties: HashMap<String, Vec<NetTiePad>>,
    /// `HasGeometryDependentRules()`.
    geometry: bool,
    /// The board's clearance-type rules are its board setup and its net classes' plain clearances alone (no custom rule, no "(diff pair)"
    /// clearance of a class whose gap is under its clearance): [`RuleResolver::is_plain`].
    plain: bool,
    /// What the plain answers are made of.
    net_rules: BoardRules,
    /// `NODE::SetMaxClearance( worstClearance + ClearanceEpsilon() )`'s `worstClearance` (`SyncWorld`).
    max_clearance: Nm,
    /// The worst clearance a hole can ask beyond its copper's: the board's hole clearance and hole to hole, the local clearances (which are
    /// hole clearances too), and every hole, physical or edge rule.
    max_hole_clearance: Nm,
    cache: RefCell<FnvMap<(Key, Key, bool), Nm>>,
    /// What building the world had to tell the user ([`Self::warnings`]).
    warnings: Vec<String>,
}

impl std::fmt::Debug for RuleResolver {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "RuleResolver({} items)", self.items.len())
    }
}

/// How far inside `copper` the hole `hole` is at the least: the radial margin of a round hole in a round pad, the margin to the sides of a
/// (rounded, then less the rounding) rectangle; 0 when it is not one of those, so that the hole is always tested.
fn hole_depth(copper: &Shape, hole: &Shape) -> Nm {
    let (hx0, hy0, hx1, hy1) = hole.bbox(Nm::ZERO);
    match (copper, hole) {
        (Shape::Circle { c, r }, Shape::Circle { c: hc, r: hr }) => {
            let off = Nm::new((((c.x - hc.x).as_f64()).hypot((c.y - hc.y).as_f64())).ceil() as i64);
            (*r - *hr - off).max(Nm::ZERO)
        }
        (Shape::Rect { x0, y0, x1, y1 }, _) => (hx0 - *x0).min(*x1 - hx1).min(hy0 - *y0).min(*y1 - hy1).max(Nm::ZERO),
        (Shape::RoundRect { x0, y0, x1, y1, r }, _) => ((hx0 - *x0).min(*x1 - hx1).min(hy0 - *y0).min(*y1 - hy1) - *r).max(Nm::ZERO),
        _ => Nm::ZERO,
    }
}

/// A net's name as the rules know it: `None` for no net and for [`crate::item::not_a_net`].
fn net_name(n: &Net) -> Option<&str> {
    n.as_deref().filter(|s| *s != "\u{1}" && !s.is_empty())
}

/// A cache entry count after which the cache starts over (KiCad clears its temporary cache between algorithm runs; the router's mouse moves
/// here make new heads on every request).
const CACHE_LIMIT: usize = 200_000;

impl RuleResolver {
    /// The resolver of a board: its rule engine (the implicit rules and the `.kicad_dru`, `RuleBoard`), the board items of its pads and rule
    /// areas, its net ties, and the worst clearance anything can ask for.
    pub fn new(design: &Design, model: &ConstraintModel, board: &eda_drc::board::DrcBoard, layer_names: Vec<String>) -> RuleResolver {
        let rules = &model.board;
        let rb = eda_drc::rule_board::RuleBoard::new(design, model, board, eda_drc::constraints::Units::Mm, rules.custom_rules_text.as_deref());
        // the board edge is item 0: a `PCB_SHAPE` on Edge.Cuts
        let mut edge = RuleItem::new("edge", Class::Shape);
        edge.layers = vec!["Edge.Cuts".to_string()];
        edge.layer = Some("Edge.Cuts".to_string());
        edge.set("Layer", Prop::Layer("Edge.Cuts".to_string()));
        let mut items: Vec<Rc<RuleItem>> = vec![Rc::new(edge)];
        let mut info: Vec<SolidInfo> = vec![SolidInfo::default()];
        let mut ids: FnvMap<String, u32> = FnvMap::default();
        let mut max_clearance = eda_drc::constraints::worst_case_clearance(rules).max(eda_drc::constraints::edge_clearance_min(rules)).max(rules.hole_to_hole_min_nm);
        let mut max_hole_clearance = rules.hole_clearance_nm.max(rules.hole_to_hole_min_nm);
        for (i, pad) in board.pads.iter().enumerate() {
            let Some(it) = rb.pad_item_at(i) else { continue };
            // `pad->GetClearanceOverrides( nullptr )` counts towards the world's worst clearance
            if let Some((c, _)) = &it.overrides.clearance {
                max_clearance = max_clearance.max(*c);
                max_hole_clearance = max_hole_clearance.max(*c);
            }
            if ids.contains_key(&pad.id) {
                continue;
            }
            let hole = pad.hole.as_ref().map(|h| {
                let drilled = pad.drill_round.is_some() || pad.drill_slot.is_none();
                let npth_slot = pad.kind == eda_model::PadKind::NonPlatedHole && pad.drill_slot.is_some_and(|(w, h)| w != h);
                (h.clone(), drilled, npth_slot)
            });
            let annulus = hole.as_ref().map_or(Nm::ZERO, |(h, _, _)| hole_depth(&pad.copper, h));
            ids.insert(pad.id.clone(), items.len() as u32);
            let special = it.overrides.clearance.is_some();
            items.push(it);
            info.push(SolidInfo { hole, annulus, special, footprint: Some(pad.footprint_ref.clone()), keepout: None });
        }
        let layer_index = |name: &str| layer_names.iter().position(|l| l == name).map(|i| i as i32);
        if let Some(rt) = &design.routing {
            for z in rt.zones.iter().filter(|z| z.is_rule_area && (z.keepout_tracks || z.keepout_vias || z.keepout_pads || z.keepout_copper_pour || z.keepout_footprints)) {
                ids.insert(format!("{}{}", Solid::KEEPOUT_SOURCE, z.id), items.len() as u32);
                items.push(rb.zone_item_of(z));
                let keepout = Keepout { zone_id: z.id.clone(), tracks: z.keepout_tracks, vias: z.keepout_vias, pads: z.keepout_pads, footprints: z.keepout_footprints, parent_footprint: z.parent_footprint.clone() };
                info.push(SolidInfo { keepout: Some(keepout), special: true, ..Default::default() });
            }
        }
        // the net ties: `FOOTPRINT::IsNetTie()` and its pads of a group
        let mut net_ties: HashMap<String, Vec<NetTiePad>> = HashMap::new();
        if let Some(d) = &design.drawings {
            for e in d.footprint_extras.iter().filter(|e| !e.net_tie_pad_groups.is_empty()) {
                let grouped: Vec<&str> = e.net_tie_pad_groups.iter().flat_map(|g| g.split(',').map(str::trim)).filter(|n| !n.is_empty()).collect();
                let pads = board
                    .pads
                    .iter()
                    .filter(|p| p.footprint_ref == e.id && grouped.contains(&p.number.as_str()))
                    .map(|p| {
                        let idx: Vec<i32> = p.layers.iter().filter_map(|l| layer_index(l)).collect();
                        let layers = match (idx.iter().min(), idx.iter().max()) {
                            (Some(&a), Some(&b)) => LayerRange::new(a, b),
                            _ => LayerRange::default(),
                        };
                        NetTiePad { net: p.net.as_deref().map(crate::item::net_of).unwrap_or(None), shape: p.copper.clone(), layers }
                    })
                    .collect();
                net_ties.insert(e.id.clone(), pads);
            }
        }
        let engine = rb.engine;
        // `GetBiggestClearanceValue`: the worst of every clearance-type rule (`QueryWorstConstraint`)
        // (the barcodes' implicit 1 mm physical clearance, `A.Type == 'Barcode'`, is left out: it never holds for the router's items, and the
        // margin is only how far the search looks -- KiCad's `QueryWorstConstraint` counts it and looks 1 mm out round everything)
        for r in engine.rules.iter().filter(|r| r.implicit != ImplicitSource::BarcodeDefaults) {
            for c in &r.constraints {
                if matches!(c.ctype, CType::Clearance | CType::PhysicalClearance | CType::HoleClearance | CType::EdgeClearance | CType::HoleToHole) {
                    let v = Nm::new(c.value.min().min(eda_drc::constraints::MAXIMUM_CLEARANCE));
                    max_clearance = max_clearance.max(v);
                    if c.ctype != CType::Clearance {
                        max_hole_clearance = max_hole_clearance.max(v);
                    }
                }
            }
        }
        let geometry = engine.rules.iter().any(|r| !r.is_implicit() && r.condition.as_ref().is_some_and(|c| c.has_geometry_dependent_functions()));
        let clearance_type = |t: CType| matches!(t, CType::Clearance | CType::PhysicalClearance | CType::HoleClearance | CType::EdgeClearance | CType::HoleToHole);
        let plain = engine.rules.iter().filter(|r| r.constraints.iter().any(|c| clearance_type(c.ctype))).all(|r| {
            r.is_implicit()
                && match r.implicit {
                    ImplicitSource::BoardSetup => r.condition.is_none(),
                    // `A.Type == 'Barcode'`: never one of the router's items
                    ImplicitSource::BarcodeDefaults => true,
                    ImplicitSource::NetClass => r.condition.as_ref().is_some_and(|c| c.expression().starts_with("A.hasExactNetclass(") && !c.expression().contains("&&")),
                    _ => false,
                }
        });
        RuleResolver { engine, layer_names, items, info, ids, extra: RefCell::new(FnvMap::default()), net_ties, geometry, plain, net_rules: rules.clone(), max_clearance, max_hole_clearance, cache: RefCell::new(FnvMap::default()), warnings: Vec::new() }
    }

    /// The worst clearance anything of this board can ask for (`NODE::GetMaxClearance`, without the epsilon): how far a collision search
    /// has to look.
    pub fn max_clearance(&self) -> Nm {
        self.max_clearance
    }

    /// What `SyncWorld` told the user while it built the world: a rule area the router cannot handle (`syncZone`'s `KIDIALOG`: "%s is malformed.").
    pub fn warnings(&self) -> &[String] {
        &self.warnings
    }

    /// Adds a warning to [`Self::warnings`].
    pub fn warn(&mut self, message: String) {
        self.warnings.push(message);
    }

    /// The number `from_ir` gives the solid of `source` (`Solid::rule`): a pad's id, `keepout:<zone id>`, a copper text's source; `0` when the
    /// board has no item of it.
    pub fn rule_of(&self, source: &str) -> u32 {
        self.ids.get(source).copied().unwrap_or(0)
    }

    /// A copper text's board item (`syncTextItem`'s `PCB_TEXT`, of no net on its layer), numbered for its solid.
    pub fn add_text(&mut self, source: &str, layer: i32) -> u32 {
        if let Some(&id) = self.ids.get(source) {
            return id;
        }
        let layer = self.layer_name(layer).unwrap_or("F.Cu").to_string();
        let mut it = RuleItem::new(source.to_string(), Class::Text);
        it.layers = vec![layer.clone()];
        it.layer = Some(layer.clone());
        it.set("Layer", Prop::Layer(layer));
        let id = self.items.len() as u32;
        self.items.push(Rc::new(it));
        self.info.push(SolidInfo { special: true, ..Default::default() });
        self.ids.insert(source.to_string(), id);
        id
    }

    /// A graphic's board item (`syncGraphicalItem`'s `PCB_SHAPE` / `PCB_TEXTBOX`, `syncDimension`'s and `syncBarcode`'s items, `syncTextItem`'s table): an item
    /// of `net` (none: no net) on its copper layer, of the footprint `footprint` when it is one's (`GetParentFootprint()`, which a net tie's bridge is
    /// [`Self::is_in_net_tie`] by), numbered for its solids. `class` is what the rules see it as (`A.Type`).
    pub fn add_graphic(&mut self, source: &str, layer: i32, net: Option<&str>, footprint: Option<&str>, class: Class) -> u32 {
        if let Some(&id) = self.ids.get(source) {
            return id;
        }
        let layer = self.layer_name(layer).unwrap_or("F.Cu").to_string();
        let mut it = RuleItem::new(source.to_string(), class);
        it.layers = vec![layer.clone()];
        it.layer = Some(layer.clone());
        it.net = net.unwrap_or("").to_string();
        it.set("Net", Prop::Enum(it.net.clone()));
        it.set("Layer", Prop::Layer(layer));
        let id = self.items.len() as u32;
        self.items.push(Rc::new(it));
        self.info.push(SolidInfo { special: true, footprint: footprint.map(str::to_string), ..Default::default() });
        self.ids.insert(source.to_string(), id);
        id
    }

    fn info_of(&self, s: &Solid) -> Option<&SolidInfo> {
        if s.edge {
            return None;
        }
        let id = if s.rule != 0 { s.rule } else { *self.ids.get(s.source.as_str())? };
        self.info.get(id as usize)
    }

    /// Can a hole at `depth` inside copper that keeps `copper_clearance` from the probe still be within its own clearance of it? Not when the
    /// worst hole clearance is no more than the copper's plus the depth (the hole is that much further than the copper is): then the hole's
    /// test is skipped, whatever its rules (`CT_CLEARANCE`, which a hole asks too, is the copper's).
    pub fn hole_reach(&self, copper_clearance: Nm, depth: Nm) -> bool {
        self.max_hole_clearance > copper_clearance + depth
    }

    /// How far inside its pad's copper a pad's hole is at the least ([`hole_depth`]).
    pub fn hole_annulus(&self, s: &Solid) -> Nm {
        self.info_of(s).map_or(Nm::ZERO, |i| i.annulus)
    }

    /// The keepout area a solid of the world is a triangle of ([`crate::from_ir`]'s `syncZone`).
    pub fn keepout(&self, s: &Solid) -> Option<&Keepout> {
        if s.rule == 0 && !s.is_keepout() {
            return None;
        }
        if s.rule == 0 {
            let zone = s.source.strip_prefix(Solid::KEEPOUT_SOURCE)?;
            let zone = zone.rsplit_once('#').map_or(zone, |(z, _)| z);
            return self.ids.get(&format!("{}{zone}", Solid::KEEPOUT_SOURCE)).and_then(|&id| self.info.get(id as usize)?.keepout.as_ref());
        }
        self.info_of(s)?.keepout.as_ref()
    }

    /// The drilled hole of a pad of the world (`SOLID::Hole()`), if it has one.
    pub fn pad_hole(&self, s: &Solid) -> Option<&Shape> {
        self.info_of(s)?.hole.as_ref().map(|(h, _, _)| h)
    }

    /// The footprint a pad of the world belongs to.
    pub fn footprint_of(&self, s: &Solid) -> Option<&str> {
        self.info_of(s)?.footprint.as_deref()
    }

    /// `PNS_PCBNEW_RULE_RESOLVER::IsInNetTie( aA )`: the item is of a net-tie footprint.
    pub fn is_in_net_tie(&self, item: &Item) -> bool {
        matches!(item, Item::Solid(s) if self.footprint_of(s).is_some_and(|fp| self.net_ties.contains_key(fp)))
    }

    /// `PNS_PCBNEW_RULE_RESOLVER::IsNetTieExclusion( aItem, aCollisionPos, aCollidingItem )` -> `DRC_ENGINE::IsNetTieExclusion( aTrackNetCode,
    /// aTrackLayer, aCollisionPos, aCollidingItem )`: the collision with a pad of a net-tie footprint at `pos` is allowed when `pos` is on a pad of
    /// that footprint's groups of the colliding item's own net (`pad->GetEffectiveShape( aTrackLayer )->Collide( aCollisionPos, epsilon )`). (The
    /// first test, "Two items colliding from the same net tie footprint are not checked", is for two pads; the router's heads are never one.)
    pub fn is_net_tie_exclusion(&self, net: &Net, layer: i32, pos: Point, colliding: &Item) -> bool {
        let Item::Solid(s) = colliding else { return false };
        let Some(pads) = self.footprint_of(s).and_then(|fp| self.net_ties.get(fp)) else { return false };
        let probe = Shape::Circle { c: pos, r: Nm::ZERO };
        pads.iter().any(|p| p.net.is_some() && p.net == *net && p.layers.overlaps_layer(layer) && p.shape.collides(&probe, DRC_EPSILON).is_some())
    }

    fn layer_name(&self, layer: i32) -> Option<&str> {
        self.layer_names.get(usize::try_from(layer).ok()?).map(String::as_str)
    }

    fn key(&self, side: &Side) -> Key {
        let g = self.geometry;
        match *side {
            Side::Track { net, layer, seg } => Key::Track(NetKey(net.clone()), layer, seg.filter(|_| g).map(|(a, b)| (a.x.get(), a.y.get(), b.x.get(), b.y.get()))),
            Side::Via { net, layers, pos } => Key::Via(NetKey(net.clone()), layers, g.then_some((pos.x.get(), pos.y.get()))),
            Side::ViaHole { net, layers, pos } => Key::ViaHole(NetKey(net.clone()), layers, g.then_some((pos.x.get(), pos.y.get()))),
            Side::Item(Item::Segment(s)) => Key::Track(NetKey(s.net.clone()), s.layer, g.then_some((s.a.x.get(), s.a.y.get(), s.b.x.get(), s.b.y.get()))),
            Side::Item(Item::Arc(a)) => Key::Track(NetKey(a.net.clone()), a.layer, g.then_some((a.arc.start.x.get(), a.arc.start.y.get(), a.arc.end.x.get(), a.arc.end.y.get()))),
            Side::Item(Item::Via(v)) => Key::Via(NetKey(v.net.clone()), v.layers, g.then_some((v.pos.x.get(), v.pos.y.get()))),
            Side::Item(Item::Solid(s)) => Key::Solid(self.solid_id(s), s.layers),
            Side::PadHole(s) => Key::PadHole(self.solid_id(s), s.layers),
        }
    }

    /// The number of the rule item a solid is (`Solid::rule`): the board edge's pieces are one item to the rules (0), a keepout area's
    /// triangles one area. A solid with no number is looked up by its source, and one the board does not know is made an item of no net on
    /// its layer the first time it is asked about.
    fn solid_id(&self, s: &Solid) -> u32 {
        if s.edge {
            return EDGE_ID;
        }
        if s.rule != 0 {
            return s.rule;
        }
        if let Some(&id) = self.ids.get(s.source.as_str()) {
            return id;
        }
        if let Some(k) = self.keepout(s) {
            if let Some(&id) = self.ids.get(&format!("{}{}", Solid::KEEPOUT_SOURCE, k.zone_id)) {
                return id;
            }
        }
        self.extra_item(s).0
    }

    fn extra_item(&self, s: &Solid) -> (u32, Rc<RuleItem>) {
        if let Some(e) = self.extra.borrow().get(&s.source) {
            return e.clone();
        }
        let layer = self.layer_name(s.layers.start()).unwrap_or("F.Cu").to_string();
        let mut it = RuleItem::new(s.source.clone(), Class::Text);
        it.layers = vec![layer.clone()];
        it.layer = Some(layer.clone());
        it.set("Layer", Prop::Layer(layer));
        let mut extra = self.extra.borrow_mut();
        let entry = (self.items.len() as u32 + extra.len() as u32, Rc::new(it));
        extra.insert(s.source.clone(), entry.clone());
        entry
    }

    /// `getBoardItem` / `BoardItem()`: the board item a side is asked about as, on `layer`.
    fn board_item(&self, side: &Side, layer: &str) -> Rc<RuleItem> {
        let net_name = |n: &Net| n.as_deref().filter(|s| *s != "\u{1}").unwrap_or("").to_string();
        let copper = || self.layer_names.clone();
        let dummy_track = |net: &Net, a: Option<(Point, Point)>| {
            // `m_dummyTracks[aIdx]`: `SetLayer( aBoardLayer )`, `SetNet`, `SetStart( Anchor( 0 ) )`, `SetEnd( Anchor( 1 ) )`; the width is a
            // `PCB_TRACK`'s default, 0.2 mm
            let mut it = RuleItem::new("router_track", Class::Track);
            it.net = net_name(net);
            it.layers = vec![layer.to_string()];
            it.layer = Some(layer.to_string());
            it.set("Net", Prop::Enum(it.net.clone()));
            it.set("Layer", Prop::Layer(layer.to_string()));
            it.set("Width", Prop::Int(200_000));
            if let Some((a, b)) = a {
                it.position = a;
                it.shapes = vec![Shape::Stadium { a, b, r: Nm::new(100_000) }];
                it.set("Start X", Prop::Int(a.x.get()));
                it.set("Start Y", Prop::Int(a.y.get()));
                it.set("End X", Prop::Int(b.x.get()));
                it.set("End Y", Prop::Int(b.y.get()));
            }
            Rc::new(it)
        };
        let dummy_via = |net: &Net, pos: Point| {
            // `m_dummyVias[aIdx]`: `SetLayer( aBoardLayer )`, `SetNet`, `SetStart( Anchor( 0 ) )`: a through via
            let mut it = RuleItem::new("router_via", Class::Via);
            it.net = net_name(net);
            it.layers = copper();
            it.layer = it.layers.first().cloned();
            it.position = pos;
            it.via_type = Some("Through");
            it.has_hole = true;
            it.has_drilled_hole = true;
            it.plated = true;
            it.flashed = it.layers.clone();
            it.set("Net", Prop::Enum(it.net.clone()));
            it.set("Via Type", Prop::Enum("Through".to_string()));
            Rc::new(it)
        };
        match *side {
            Side::Track { net, seg, .. } => dummy_track(net, seg),
            Side::Via { net, pos, .. } | Side::ViaHole { net, pos, .. } => dummy_via(net, pos),
            Side::Item(Item::Segment(s)) => dummy_track(&s.net, Some((s.a, s.b))),
            Side::Item(Item::Arc(a)) => dummy_track(&a.net, Some((a.arc.start, a.arc.end))),
            Side::Item(Item::Via(v)) => dummy_via(&v.net, v.pos),
            Side::Item(Item::Solid(s)) | Side::PadHole(s) => self.solid_item(s),
        }
    }

    fn solid_item(&self, s: &Solid) -> Rc<RuleItem> {
        let id = self.solid_id(s) as usize;
        match self.items.get(id) {
            Some(it) => it.clone(),
            None => self.extra_item(s).1,
        }
    }

    /// `isCopper( aItem )`: `!parent || parent->IsOnCopperLayer()`. An item the router made, a hole (whose `Parent()` is null) and a keepout
    /// area on a copper layer are; the board edge is not.
    fn is_copper(&self, side: &Side, item: &RuleItem) -> bool {
        match side {
            Side::Item(Item::Solid(s)) => !s.edge && (item.on_copper() || self.keepout(s).is_some() || s.is_text() || s.is_graphic() || s.is_obstacle()),
            _ => true,
        }
    }

    /// `IsDrilledHole( aItem )`.
    fn is_drilled_hole(&self, side: &Side) -> bool {
        match side {
            Side::ViaHole { .. } => true,
            Side::PadHole(s) => self.info_of(s).and_then(|i| i.hole.as_ref()).is_some_and(|h| h.1),
            _ => false,
        }
    }

    /// `IsNonPlatedSlot( aItem )`.
    fn is_non_plated_slot(&self, side: &Side) -> bool {
        matches!(side, Side::PadHole(s) if self.info_of(s).and_then(|i| i.hole.as_ref()).is_some_and(|h| h.2))
    }

    /// `QueryConstraint( aType, aItemA, aItemB, aLayer )`: the minimum the rules give, or `None` when there is no constraint or it is
    /// ignored (a rule of severity ignore, other than a tuning profile's, makes the minimum -1).
    fn query(&self, ctype: CType, a: &Rc<RuleItem>, b: &Rc<RuleItem>, layer: &str) -> Option<i64> {
        let c = self.engine.eval_rules(ctype, Some(a), Some(b), Some(layer), None);
        if c.is_null() {
            return None;
        }
        if let Some(rule) = c.parent.map(|i| &self.engine.rules[i]) {
            if rule.severity == Severity::Ignore && !(rule.is_implicit() && rule.implicit == ImplicitSource::TuningProfile) {
                return Some(-1);
            }
        }
        Some(c.value.min())
    }

    /// `QueryConstraint( CT_CLEARANCE, aItem, nullptr, aLayer )` of a track of `net` on `layer`: the clearance the rules ask of it alone
    /// (`MEANDER_PLACER_BASE::Clearance()`, the meanders' spacing). `None` when the rules give none.
    pub fn track_clearance(&self, net: &Net, layer: i32) -> Option<Nm> {
        let name = self.layer_name(layer)?;
        let item = self.board_item(&Side::track(net, layer), name);
        let c = self.engine.eval_rules(CType::Clearance, Some(&item), None, Some(name), None);
        (!c.is_null()).then(|| Nm::new(c.value.min()))
    }

    /// The board's clearance rules are its board setup's and its net classes' plain clearances alone, which [`Self::clearance`] answers
    /// without the engine for the items that have no rules of their own.
    pub fn is_plain(&self) -> bool {
        self.plain
    }

    /// Is the side's answer its net's alone? A track or via the router asks about, a pad with no local clearance, the board edge; not a rule
    /// area, a copper text or a pad with a clearance of its own.
    fn plain_side(&self, side: &Side) -> bool {
        match side {
            Side::Item(Item::Solid(s)) => s.edge || self.info_of(s).is_some_and(|i| !i.special),
            Side::PadHole(s) => self.info_of(s).is_some_and(|i| !i.special),
            _ => true,
        }
    }

    /// `Clearance` for two plain sides on a plain board ([`Self::plain_side`], `plain`), without the engine: what `EvalRules` gives there is
    /// the board setup's hole to hole, hole clearance and copper-to-edge clearance and the net classes' clearance (the larger of the two
    /// nets', the board minimum under it: `eda_drc::constraints::clearance`), and nothing for `CT_PHYSICAL_CLEARANCE`. (A test holds this to
    /// the engine on KiCad's QA boards.)
    fn plain_clearance(&self, a: &Side, b: &Side) -> i64 {
        let common = a.is_edge() || b.is_edge() || a.layers().overlaps(&b.layers());
        if !common {
            return 0;
        }
        let r = &self.net_rules;
        let mut rv: i64 = 0;
        if self.is_drilled_hole(a) && self.is_drilled_hole(b) {
            rv = rv.max(r.hole_to_hole_min_nm.get());
        } else if a.is_hole() || b.is_hole() {
            rv = rv.max(r.hole_clearance_nm.get());
        }
        if !a.is_edge() && !b.is_edge() {
            rv = rv.max(eda_drc::constraints::clearance(r, net_name(a.net()), net_name(b.net())).get());
        }
        if a.is_edge() || self.is_non_plated_slot(a) || b.is_edge() || self.is_non_plated_slot(b) {
            rv = rv.max(eda_drc::constraints::edge_clearance_min(r).get());
        }
        rv
    }

    /// `PNS_PCBNEW_RULE_RESOLVER::Clearance( aA, aB, aUseClearanceEpsilon )`.
    pub fn clearance(&self, a: &Side, b: &Side, use_epsilon: bool) -> Nm {
        if self.plain {
            // two tracks or vias (the most of what is asked): the net classes' clearance on a layer they share
            let track_or_via = |s: &Side| matches!(s, Side::Track { .. } | Side::Via { .. } | Side::Item(Item::Segment(_) | Item::Arc(_) | Item::Via(_)));
            let rv = if track_or_via(a) && track_or_via(b) {
                if a.layers().overlaps(&b.layers()) {
                    eda_drc::constraints::clearance(&self.net_rules, net_name(a.net()), net_name(b.net())).get()
                } else {
                    0
                }
            } else if self.plain_side(a) && self.plain_side(b) {
                self.plain_clearance(a, b)
            } else {
                return self.engine_clearance(a, b, use_epsilon);
            };
            return Nm::new(if use_epsilon && rv > 0 { (rv - DRC_EPSILON.get()).max(0) } else { rv });
        }
        self.engine_clearance(a, b, use_epsilon)
    }

    /// [`Self::clearance`] asked of the engine whatever the sides, cached per pair (`m_clearanceCache`).
    pub fn engine_clearance(&self, a: &Side, b: &Side, use_epsilon: bool) -> Nm {
        let key = (self.key(a), self.key(b), use_epsilon);
        if let Some(&v) = self.cache.borrow().get(&key) {
            return v;
        }
        let layers = if a.is_edge() {
            b.layers()
        } else if b.is_edge() {
            a.layers()
        } else {
            let (la, lb) = (a.layers(), b.layers());
            if la.overlaps(&lb) {
                LayerRange::new(la.start().max(lb.start()), la.end().min(lb.end()))
            } else {
                LayerRange::default()
            }
        };
        let mut rv: i64 = 0;
        if layers.start() >= 0 {
            for layer in layers.start()..=layers.end() {
                let Some(name) = self.layer_name(layer) else { continue };
                let (ia, ib) = (self.board_item(a, name), self.board_item(b, name));
                let mut take = |t: CType| {
                    if let Some(v) = self.query(t, &ia, &ib, name) {
                        rv = rv.max(v);
                    }
                };
                if self.is_drilled_hole(a) && self.is_drilled_hole(b) {
                    take(CType::HoleToHole);
                } else if a.is_hole() || b.is_hole() {
                    take(CType::HoleClearance);
                }
                // "No 'else'; plated holes get both HOLE_CLEARANCE and CLEARANCE"
                if self.is_copper(a, &ia) && self.is_copper(b, &ib) {
                    take(CType::Clearance);
                }
                // "No 'else'; non-plated milled holes get both HOLE_CLEARANCE and EDGE_CLEARANCE"
                if a.is_edge() || self.is_non_plated_slot(a) || b.is_edge() || self.is_non_plated_slot(b) {
                    take(CType::EdgeClearance);
                }
                take(CType::PhysicalClearance);
            }
        }
        if use_epsilon && rv > 0 {
            rv = (rv - DRC_EPSILON.get()).max(0);
        }
        let mut cache = self.cache.borrow_mut();
        if cache.len() >= CACHE_LIMIT {
            cache.clear();
        }
        cache.insert(key, Nm::new(rv));
        Nm::new(rv)
    }
}
