//! Ported from `pcbnew/drc/drc_test_provider_solder_mask.cpp`
//! (`DRC_TEST_PROVIDER_SOLDER_MASK`): mask apertures that bridge copper of
//! different nets (`DRCE_SOLDERMASK_BRIDGE`) and -- when the board sets a
//! minimum web width -- silkscreen clipped by the whole-board mask polygon
//! (`DRCE_SILK_MASK_CLEARANCE`, `testSilkToMaskClearance`). With a zero web
//! width that second test is delegated to the silk-clearance provider
//! (`silk_mask.rs`), exactly as the C++ does.
//!
//! Structure follows the C++ function for function:
//! - [`build_items`] stands in for `forEachGeometryItem` over
//!   `{F_Mask, B_Mask, F_Cu, B_Cu}` (pads, vias, tracks, mask-layer
//!   graphics); an item's mask-layer presence (`IsOnLayer(F_Mask)`) is
//!   what puts it in `m_itemTree`;
//! - [`Ctx::test_item_against_items`] is `testItemAgainstItems` (filter,
//!   `m_checkedPairs` de-dup, the web-width / copper-clearance +
//!   `GetSolderMaskExpansion` clearance, aperture vs. plain item routing);
//! - [`Ctx::check_mask_aperture`] / [`Ctx::check_item_mask`] are
//!   `checkMaskAperture` / `checkItemMask` (first-net-wins aperture map,
//!   footprint net-tie and `allow_soldermask_bridges` handling);
//! - [`Ctx::test_mask_item_against_zones`] is `testMaskItemAgainstZones`;
//! - the deferred `m_pendingCollisions` pass at the end of `testMaskBridges`
//!   is [`Ctx::report_pending`].
//!
//! Known gaps (see the task report): custom `bridged_mask` /
//! `solder_mask_expansion` DRC rules; mask-layer text; tracks/zones that
//! sit on a mask layer themselves (`hasSolderMask`); padstack per-layer
//! shapes (a pad has one shape on every layer here); custom-shaped pads are
//! the importer's rectangle approximation.

use crate::board::{DrcBoard, DrcGraphic};
use crate::fill::FillResults;
use crate::item::{DrcRefItem, DrcViolation, ErrorType};
use crate::kimath::Shape;
use crate::rtree::DrcRTree;
use eda_clipper2::Point64;
use eda_model::ir::{Point, Side, Um};
use eda_shape_poly_set::{CornerStrategy, ShapePolySet};
use eda_zone_filler::shape::{shape_to_polygon_outside, Shape as FillShape};
use eda_model::{BoardRules, PadKind};
use std::collections::{HashMap, HashSet};

pub(crate) const FRONT: usize = 0;
pub(crate) const BACK: usize = 1;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Kind {
    Pad,
    Via,
    Track,
    Graphic,
}

pub(crate) struct Item {
    pub(crate) kind: Kind,
    /// Index into `board.pads` / `vias` / `tracks` / `mask.graphics`.
    pub(crate) idx: usize,
    /// Index into `board.mask.footprints` (`GetParentFootprint()`).
    pub(crate) fp: Option<usize>,
    /// `GetNetCode()`: -1 for a non-connected item, 0 "no net".
    pub(crate) net: i32,
    /// `IsOnLayer( F_Cu / B_Cu )`.
    pub(crate) cu: [bool; 2],
    /// `IsOnLayer( F_Mask / B_Mask )`.
    pub(crate) mask: [bool; 2],
    pub(crate) shapes: Vec<Shape>,
    pub(crate) bbox: (Um, Um, Um, Um),
    /// `isMaskAperture`.
    pub(crate) aperture: bool,
    /// `PAD::IsNPTHWithNoCopper`.
    pub(crate) npth_no_cu: bool,
    /// Pad number (real pads and mask-only pads).
    pub(crate) pad_number: Option<String>,
    pub(crate) is_pad: bool,
}

pub(crate) struct FpInfo {
    allow_bridges: bool,
    is_net_tie: bool,
    /// `MapPadNumbersToNetTieGroups` restricted to grouped numbers.
    groups: HashMap<String, usize>,
    /// `GetNetTieCache`, keyed by item index.
    cache: HashMap<usize, HashSet<i32>>,
    /// Item indices of this footprint's pads.
    pads: Vec<usize>,
}

/// `FOOTPRINT::MapPadNumbersToNetTieGroups`' parsing of `m_netTiePadGroups`
/// (comma separated pad numbers per group string, `\` escapes).
fn parse_net_tie_groups(groups: &[String]) -> HashMap<String, usize> {
    let mut map = HashMap::new();
    for (ii, group) in groups.iter().enumerate() {
        let mut esc = false;
        let mut pad = String::new();
        let flush = |pad: &mut String, map: &mut HashMap<String, usize>| {
            let t = pad.trim().to_string();
            if !t.is_empty() {
                map.insert(t, ii);
            }
            pad.clear();
        };
        for ch in group.chars() {
            if esc {
                esc = false;
                pad.push(ch);
                continue;
            }
            match ch {
                '\\' => esc = true,
                ',' => flush(&mut pad, &mut map),
                c => pad.push(c),
            }
        }
        flush(&mut pad, &mut map);
    }
    map
}

fn layer_side(layer: &str) -> Option<usize> {
    match layer {
        "F.Cu" | "F.Mask" => Some(FRONT),
        "B.Cu" | "B.Mask" => Some(BACK),
        _ => None,
    }
}

pub(crate) fn shapes_bbox(shapes: &[Shape]) -> (Um, Um, Um, Um) {
    let (mut x0, mut y0, mut x1, mut y1) = (Um::MAX, Um::MAX, Um::MIN, Um::MIN);
    for s in shapes {
        let (a, b, c, d) = s.bbox(0);
        x0 = x0.min(a);
        y0 = y0.min(b);
        x1 = x1.max(c);
        y1 = y1.max(d);
    }
    (x0, y0, x1, y1)
}

/// Minimum-distance collision between two compound shapes (any pair of
/// children colliding is a hit; the reported `actual` is the smallest).
pub(crate) fn collide(a: &[Shape], b: &[Shape], clearance: Um) -> Option<(Um, Point)> {
    let mut best: Option<(Um, Point)> = None;
    for sa in a {
        for sb in b {
            if let Some((actual, pos)) = sa.collides(sb, clearance) {
                if best.map_or(true, |(ba, _)| actual < ba) {
                    best = Some((actual, pos));
                }
            }
        }
    }
    best
}

/// `PAD::IsNPTHWithNoCopper` -- an NPTH whose round/oval pad is no larger
/// than its own drill. Any other pad shape counts as copper.
fn npth_with_no_copper(p: &crate::board::DrcPad, shape: eda_model::PadShape, size: (Um, Um)) -> bool {
    if p.kind != PadKind::NonPlatedHole {
        return false;
    }
    let drill = match (p.drill_round, p.drill_slot) {
        (Some(d), _) => (d, d),
        (None, Some(s)) => s,
        _ => (0, 0),
    };
    match shape {
        eda_model::PadShape::Circle => size.0 <= drill.0,
        eda_model::PadShape::Oval => size.0 <= drill.0 && size.1 <= drill.1,
        _ => false,
    }
}

struct Ctx<'a> {
    board: &'a DrcBoard,
    items: Vec<Item>,
    fps: Vec<FpInfo>,
    web: Um,
    to_copper: Um,
    board_expansion: Um,
    /// Mask-layer R-trees (`m_itemTree`), by side.
    trees: [DrcRTree; 2],
    /// `m_checkedPairs`.
    checked: HashSet<(usize, usize, usize)>,
    /// `m_maskApertureNetMap`: aperture (item index, mask side) -> first (item, net).
    aperture_first: HashMap<(usize, usize), (usize, i32)>,
    /// `m_maskApertureNetMapAll`.
    aperture_all: HashMap<(usize, usize), Vec<(usize, i32)>>,
    pending: Vec<Pending>,
    out: Vec<DrcViolation>,
    bridges: usize,
    /// Per-zone fill fragments and overall bbox, computed lazily.
    zone_frags: Vec<(Vec<Shape>, (Um, Um, Um, Um))>,
    net_ids: HashMap<String, i32>,
    /// `m_largestClearance`.
    largest: Um,
}

struct Pending {
    aperture: usize,
    colliding: usize,
    colliding_net: i32,
    side: usize,
}

const ERROR_LIMIT: usize = 199;

impl<'a> Ctx<'a> {
    fn pad_mask(&self, idx: usize) -> Option<&crate::board::DrcPadMask> {
        self.board.mask.pads.get(idx)
    }

    /// `PAD::GetSolderMaskExpansion`.
    fn pad_expansion(&self, it: &Item) -> Um {
        // Pads with no copper layer at all use their own shape only.
        if !it.cu[FRONT] && !it.cu[BACK] && !self.pad_has_inner_cu(it) {
            return 0;
        }
        let pm = self.pad_mask(it.idx);
        let margin = pm
            .and_then(|m| m.margin)
            .or_else(|| it.fp.and_then(|f| self.board.mask.footprints[f].margin))
            .unwrap_or(self.board_expansion);
        let mut m = margin;
        if m < 0 {
            let size = pm.map(|m| m.size).unwrap_or((0, 0));
            let minsize = -(size.0.min(size.1)) / 2;
            if m < minsize {
                m = minsize;
            }
        }
        m
    }

    fn pad_has_inner_cu(&self, it: &Item) -> bool {
        // Only outer copper is tracked per item; an inner-layer-only pad
        // is not a case this model produces. Treat "no outer copper" as none.
        let _ = it;
        false
    }

    /// `PCB_SHAPE::GetSolderMaskExpansion` for a mask-layer graphic.
    fn graphic_expansion(&self, g: &DrcGraphic) -> Um {
        let mut margin = g.margin.unwrap_or(self.board_expansion);
        if margin < 0 && !g.filled {
            margin = margin.max(-g.width / 2);
        }
        margin
    }

    /// `PCB_VIA::IsTented`.
    fn via_tented(&self, via: usize, side: usize) -> bool {
        let vm = self.board.mask.vias.get(via);
        let over = vm.and_then(|m| if side == FRONT { m.tent_front } else { m.tent_back });
        over.unwrap_or(if side == FRONT { self.board.mask.rules.tent_vias_front } else { self.board.mask.rules.tent_vias_back })
    }

    /// The `if( pad ) ... else if( via && !via->IsTented ) ... else if( shape )`
    /// expansion ladder `testItemAgainstItems` applies to either participant.
    fn expansion_of(&self, it: &Item, side: usize) -> Um {
        match it.kind {
            Kind::Pad => self.pad_expansion(it),
            Kind::Via => {
                if self.via_tented(it.idx, side) {
                    0
                } else {
                    self.board_expansion
                }
            }
            Kind::Graphic => {
                let g = &self.board.mask.graphics[it.idx];
                // A mask-only pad is a `PCB_PAD_T` with no copper: 0.
                if g.pad.is_some() {
                    0
                } else {
                    self.graphic_expansion(g)
                }
            }
            Kind::Track => 0,
        }
    }

    fn ref_item(&self, it: &Item) -> DrcRefItem {
        item_ref(self.board, it)
    }

    fn same_logical_pad(&self, a: &Item, b: &Item) -> bool {
        a.is_pad && b.is_pad && a.fp == b.fp && a.fp.is_some() && a.pad_number.as_deref().is_some_and(|n| !n.is_empty()) && a.pad_number == b.pad_number
    }

    /// `PAD::SharesNetTieGroup`.
    fn shares_net_tie_group(&self, a: &Item, b: &Item) -> bool {
        let (Some(fa), Some(fb)) = (a.fp, b.fp) else { return false };
        if fa != fb || !a.is_pad || !b.is_pad || !self.fps[fa].is_net_tie {
            return false;
        }
        let g = &self.fps[fa].groups;
        match (g.get(a.pad_number.as_deref().unwrap_or("")), g.get(b.pad_number.as_deref().unwrap_or(""))) {
            (Some(x), Some(y)) => x == y,
            _ => false,
        }
    }

    /// `checkMaskAperture`.
    fn check_mask_aperture(&mut self, mask_item: usize, test_item: usize, test_is_front_mask: Option<usize>, test_layer_side: usize, test_net: i32) -> Option<usize> {
        // `if( aTestLayer == F_Mask && !aTestItem->IsOnLayer( F_Cu ) ) return false;`
        if let Some(side) = test_is_front_mask {
            if !self.items[test_item].cu[side] {
                return None;
            }
        }
        let mask_side = test_layer_side;
        let fp = self.items[mask_item].fp;
        // Mask apertures in footprints which allow soldermask bridges are ignored entirely.
        if let Some(f) = fp {
            if self.fps[f].allow_bridges {
                return None;
            }
        }
        let key = (mask_item, mask_side);
        let (already_item, already_net);
        match self.aperture_first.get(&key) {
            None => {
                self.aperture_first.insert(key, (test_item, test_net));
                self.aperture_all.entry(key).or_default().push((test_item, test_net));
                // First net; no bridge yet....
                return None;
            }
            Some(&(i, n)) => {
                already_item = i;
                already_net = n;
                self.aperture_all.entry(key).or_default().push((test_item, test_net));
                if already_net == test_net && test_net >= 0 {
                    // Same net; no bridge.
                    return None;
                }
            }
        }

        if let Some(f) = fp {
            if self.items[test_item].fp == Some(f) {
                let pad_a = self.items[already_item].is_pad;
                let pad_b = self.items[test_item].is_pad;
                let test_is_shape = self.items[test_item].kind == Kind::Graphic && !pad_b;
                let enc_is_shape = self.items[already_item].kind == Kind::Graphic && !pad_a;
                if pad_a && pad_b && (self.same_logical_pad(&self.items[already_item], &self.items[test_item]) || self.shares_net_tie_group(&self.items[already_item], &self.items[test_item])) {
                    return None;
                } else if pad_a && test_is_shape {
                    // `padToNetTieGroupMap.contains( padA->GetNumber() )`: the map holds every pad number.
                    return None;
                } else if pad_b && enc_is_shape {
                    return None;
                }
            }
        }
        Some(already_item)
    }

    /// `checkItemMask`.
    fn check_item_mask(&self, item: usize, test_net: i32) -> bool {
        let it = &self.items[item];
        if let Some(f) = it.fp {
            let info = &self.fps[f];
            // If we're allowing bridges then we're allowing bridges.  Nothing to check.
            if info.allow_bridges {
                return false;
            }
            // Items belonging to a net-tie may share the mask aperture of pads in the same group.
            if it.is_pad && info.is_net_tie {
                let num = it.pad_number.as_deref().unwrap_or("");
                if let Some(&grp) = info.groups.get(num) {
                    if test_net < 0 {
                        return false;
                    }
                    if it.net == test_net {
                        return false;
                    }
                    for &other in &info.pads {
                        let on = self.items[other].pad_number.as_deref().unwrap_or("");
                        if info.groups.get(on) == Some(&grp) && self.items[other].net == test_net {
                            return false;
                        }
                    }
                }
            }
        }
        true
    }

    /// `testItemAgainstItems`.
    fn test_item_against_items(&mut self, ai: usize, ref_is_mask: bool, side: usize) {
        if self.bridges >= ERROR_LIMIT {
            return;
        }
        let item_net = self.items[ai].net;
        let item_fp = self.items[ai].fp;
        let inflate = self.largest;
        let bb = self.items[ai].bbox;
        let cand = self.trees[side].query((bb.0 - inflate, bb.1 - inflate, bb.2 + inflate, bb.3 + inflate));
        let mut cand = cand;
        cand.sort_unstable();
        for oi in cand {
            if oi == ai {
                continue;
            }
            // ---- filter ----
            let other_net = self.items[oi].net;
            if other_net > 0 && other_net == item_net {
                continue;
            }
            if self.items[oi].npth_no_cu {
                continue;
            }
            let other_fp = self.items[oi].fp;
            if let (Some(a), Some(b)) = (item_fp, other_fp) {
                if a == b {
                    // Board-wide exclusion / footprint-specific exclusion.
                    if self.board.mask.rules.allow_bridges_in_footprints || self.fps[a].allow_bridges {
                        continue;
                    }
                }
            }
            if self.items[ai].is_pad && self.items[oi].is_pad && (self.same_logical_pad(&self.items[ai], &self.items[oi]) || self.shares_net_tie_group(&self.items[ai], &self.items[oi])) {
                continue;
            }
            if let Some(f) = item_fp {
                if self.fps[f].is_net_tie {
                    let nets = self.fps[f].cache.get(&ai);
                    if other_net < 0 || nets.is_some_and(|n| n.contains(&other_net)) {
                        continue;
                    }
                }
            }
            if let Some(f) = other_fp {
                if self.fps[f].is_net_tie {
                    let nets = self.fps[f].cache.get(&oi);
                    if item_net < 0 || nets.is_some_and(|n| n.contains(&item_net)) {
                        continue;
                    }
                }
            }
            // store canonical order so we don't collide in both directions (a:b and b:a)
            let (a, b) = if ai > oi { (oi, ai) } else { (ai, oi) };
            if !self.checked.insert((a, b, side)) {
                continue;
            }

            // ---- visitor ----
            // Aperture-to-aperture enforces web-min-width; copper-to-aperture
            // uses the solder-mask-to-copper clearance.
            let mut clearance = if ref_is_mask { self.web } else { self.to_copper };
            clearance += self.expansion_of(&self.items[ai], side);
            clearance += self.expansion_of(&self.items[oi], side);

            let hit = collide(&self.items[ai].shapes, &self.items[oi].shapes, clearance);
            let Some((_actual, _pos)) = hit else { continue };

            let a_aperture = self.items[ai].aperture;
            let o_aperture = self.items[oi].aperture;
            if a_aperture {
                // `checkMaskAperture( aItem, other, aRefLayer, otherNet, .. )`
                let test_front_mask = if ref_is_mask { Some(side) } else { None };
                if self.check_mask_aperture(ai, oi, test_front_mask, side, other_net).is_some() {
                    self.pending.push(Pending { aperture: ai, colliding: oi, colliding_net: other_net, side });
                }
            } else if o_aperture {
                let test_front_mask = if ref_is_mask { Some(side) } else { None };
                if self.check_mask_aperture(oi, ai, test_front_mask, side, item_net).is_some() {
                    self.pending.push(Pending { aperture: oi, colliding: ai, colliding_net: item_net, side });
                }
            } else if self.check_item_mask(oi, item_net) {
                let msg = bridge_message(side);
                let items = vec![self.ref_item(&self.items[ai]), self.ref_item(&self.items[oi])];
                self.push_bridge(msg, items);
            }
        }
    }

    fn push_bridge(&mut self, msg: &str, items: Vec<DrcRefItem>) {
        let mut v = DrcViolation::new(ErrorType::SolderMaskBridge, "", items);
        v.description = msg.to_string();
        self.out.push(v);
        self.bridges += 1;
    }

    /// `testMaskItemAgainstZones`.
    fn test_mask_item_against_zones(&mut self, ai: usize, side: usize) {
        let board = self.board;
        let target_layer = if side == FRONT { "F.Cu" } else { "B.Cu" };
        for zi in 0..board.zones.len() {
            if self.bridges >= ERROR_LIMIT {
                return;
            }
            let zone = &board.zones[zi];
            if zone.layer != target_layer {
                continue;
            }
            let zone_net = zone.net.as_deref().map(|n| self.net_id(n)).unwrap_or(0);
            let hit = {
                let it = &self.items[ai];
                let connected = it.kind != Kind::Graphic || it.is_pad;
                if connected && zone_net == it.net && zone_net > 0 {
                    false
                } else {
                    // `m_SolderMaskToCopperClearance` + the item's own expansion.
                    let mut clearance = self.to_copper;
                    match it.kind {
                        Kind::Pad => clearance += self.pad_expansion(it),
                        Kind::Via if !self.via_tented(it.idx, side) => clearance += self.board_expansion,
                        Kind::Graphic if !it.is_pad => clearance += self.expansion_of(it, side),
                        _ => {}
                    }
                    let (frags, zbb) = &self.zone_frags[zi];
                    let bb = it.bbox;
                    if bb.2 + clearance < zbb.0 || zbb.2 + clearance < bb.0 || bb.3 + clearance < zbb.1 || zbb.3 + clearance < bb.1 {
                        false
                    } else {
                        collide(&it.shapes, frags, clearance).is_some()
                    }
                }
            };
            if !hit {
                continue;
            }
            let msg = bridge_message(side);
            let zone_ref = DrcRefItem {
                description: format!("{} zone [{}] on {}", if zone.teardrop { "Teardrop" } else { "Copper" }, zone.net.as_deref().unwrap_or("<no net>"), zone.layer),
                pos: zone.outline.first().map(|p| (p.x, p.y)).unwrap_or((0, 0)),
                id: zone.id.clone(),
            };
            if self.items[ai].aperture && zone_net >= 0 {
                let zitem = self.zone_item_index(zi);
                if let Some(colliding) = self.check_mask_aperture_zone(ai, zitem, side, zone_net) {
                    let items = vec![self.ref_item(&self.items[ai]), self.ref_item(&self.items[colliding]), zone_ref];
                    self.push_bridge(msg, items);
                }
            } else {
                let items = vec![self.ref_item(&self.items[ai]), zone_ref];
                self.push_bridge(msg, items);
            }
        }
    }

    fn zone_item_index(&self, zi: usize) -> usize {
        // Zones live after the regular items in `aperture_*` bookkeeping
        // keyed by a synthetic index.
        usize::MAX - zi
    }

    /// `checkMaskAperture( aItem, zone, aTargetLayer, zoneNet, .. )`: the zone
    /// has no parent footprint, and `aTestLayer` is a copper layer so the
    /// "exposed copper" pre-check is skipped.
    fn check_mask_aperture_zone(&mut self, mask_item: usize, zone_item: usize, side: usize, zone_net: i32) -> Option<usize> {
        if let Some(f) = self.items[mask_item].fp {
            if self.fps[f].allow_bridges {
                return None;
            }
        }
        let key = (mask_item, side);
        match self.aperture_first.get(&key).copied() {
            None => {
                self.aperture_first.insert(key, (zone_item, zone_net));
                self.aperture_all.entry(key).or_default().push((zone_item, zone_net));
                None
            }
            Some((i, n)) => {
                self.aperture_all.entry(key).or_default().push((zone_item, zone_net));
                if n == zone_net && zone_net >= 0 {
                    return None;
                }
                // The colliding "item" the report names is the first thing the aperture exposed;
                // a zone can only be that when `i` is a synthetic zone index -- fall back to the aperture.
                if i >= self.items.len() {
                    Some(mask_item)
                } else {
                    Some(i)
                }
            }
        }
    }

    fn net_id(&self, name: &str) -> i32 {
        self.net_ids.get(name).copied().unwrap_or(0)
    }

    /// The deferred pass at the end of `testMaskBridges`.
    fn report_pending(&mut self) {
        let mut reported: HashSet<(usize, usize, usize)> = HashSet::new();
        let pending = std::mem::take(&mut self.pending);
        for c in pending {
            if self.bridges >= ERROR_LIMIT {
                break;
            }
            let key = (c.aperture, c.side);
            let in_aperture = self.aperture_all.get(&key).cloned().unwrap_or_default();
            let msg = bridge_message(c.side);
            let mut reported_any_track = false;
            for (first_item, first_net) in in_aperture {
                // Only report items from a different net than the colliding item.
                if first_net == c.colliding_net {
                    continue;
                }
                if reported.contains(&(c.aperture, first_item, c.colliding)) {
                    continue;
                }
                reported.insert((c.aperture, first_item, c.colliding));
                // Also insert the reverse to avoid reporting (A, B, C) and (A, C, B).
                reported.insert((c.aperture, c.colliding, first_item));
                if first_item >= self.items.len() {
                    continue;
                }
                let first_is_track = self.items[first_item].kind == Kind::Track;
                if first_is_track && reported_any_track {
                    // `GetReportAllTrackErrors()` is off: one track per collision.
                    continue;
                }
                let items = vec![self.ref_item(&self.items[c.aperture]), self.ref_item(&self.items[first_item]), self.ref_item(&self.items[c.colliding])];
                self.push_bridge(msg, items);
                if first_is_track {
                    reported_any_track = true;
                }
                if self.bridges >= ERROR_LIMIT {
                    break;
                }
            }
        }
    }
}

/// The `items[]` entry kicad-cli would list for `it`.
pub(crate) fn item_ref(b: &DrcBoard, it: &Item) -> DrcRefItem {
    match it.kind {
        Kind::Pad => pad_ref(b, it.idx),
        Kind::Via => {
            let v = &b.vias[it.idx];
            DrcRefItem { description: format!("Via [{}] on {}-{}", v.net.as_deref().unwrap_or("<no net>"), v.from_layer, v.to_layer), pos: (v.at.x, v.at.y), id: v.id.clone() }
        }
        Kind::Track => {
            let t = &b.tracks[it.idx];
            DrcRefItem { description: format!("Track [{}] on {}", t.net.as_deref().unwrap_or("<no net>"), t.layer), pos: (t.a.x, t.a.y), id: t.id.clone() }
        }
        Kind::Graphic => {
            let g = &b.mask.graphics[it.idx];
            DrcRefItem { description: g.desc.clone(), pos: (g.pos.x, g.pos.y), id: g.id.clone() }
        }
    }
}

pub(crate) fn pad_ref(b: &DrcBoard, idx: usize) -> DrcRefItem {
    let p = &b.pads[idx];
    DrcRefItem { description: format!("Pad {} [{}] of {}", p.number, p.net.as_deref().unwrap_or("<no net>"), p.footprint_ref), pos: (p.center.x, p.center.y), id: p.id.clone() }
}

fn bridge_message(side: usize) -> &'static str {
    if side == FRONT {
        "Front solder mask aperture bridges items with different nets"
    } else {
        "Rear solder mask aperture bridges items with different nets"
    }
}

/// `forEachGeometryItem( s_allBasicItemsButZones, {F_Mask,B_Mask,F_Cu,B_Cu} )`
/// plus the per-item layer/net/shape facts the test reads.
pub(crate) fn build_items(board: &DrcBoard, net_ids: &mut HashMap<String, i32>) -> (Vec<Item>, Vec<FpInfo>) {
    let mut id_of = |n: Option<&str>| -> i32 {
        match n {
            None | Some("") => 0,
            Some(name) => {
                let next = net_ids.len() as i32 + 1;
                *net_ids.entry(name.to_string()).or_insert(next)
            }
        }
    };
    let mut items: Vec<Item> = Vec::new();

    // Tracks (`PCB_TRACE_T`/`PCB_ARC_T`): copper only (`hasSolderMask` is not modelled).
    for (i, t) in board.tracks.iter().enumerate() {
        let Some(side) = layer_side(&t.layer) else { continue };
        let shapes = vec![t.shape()];
        let mut cu = [false; 2];
        cu[side] = t.layer.ends_with(".Cu");
        if !cu[side] {
            continue;
        }
        items.push(Item { kind: Kind::Track, idx: i, fp: None, net: id_of(t.net.as_deref()), cu, mask: [false, false], bbox: shapes_bbox(&shapes), shapes, aperture: false, npth_no_cu: false, pad_number: None, is_pad: false });
    }

    // Vias.
    let rules = &board.mask.rules;
    for (i, v) in board.vias.iter().enumerate() {
        let from = board.layers.iter().position(|l| *l == v.from_layer).unwrap_or(0);
        let to = board.layers.iter().position(|l| *l == v.to_layer).unwrap_or(board.layers.len().saturating_sub(1));
        let (lo, hi) = (from.min(to), from.max(to));
        let cu = [lo == 0, hi + 1 >= board.layers.len().max(1)];
        let vm = board.mask.vias.get(i);
        let tented = |side: usize| -> bool {
            let over = vm.and_then(|m| if side == FRONT { m.tent_front } else { m.tent_back });
            over.unwrap_or(if side == FRONT { rules.tent_vias_front } else { rules.tent_vias_back })
        };
        // `PCB_VIA::IsOnLayer( F_Mask )`: untented and on the outer copper layer.
        let mask = [cu[FRONT] && !tented(FRONT), cu[BACK] && !tented(BACK)];
        let shapes = vec![v.shape()];
        items.push(Item { kind: Kind::Via, idx: i, fp: None, net: id_of(v.net.as_deref()), cu, mask, bbox: shapes_bbox(&shapes), shapes, aperture: false, npth_no_cu: false, pad_number: None, is_pad: false });
    }

    // Pads.
    let mut fp_index: HashMap<&str, usize> = HashMap::new();
    for (i, f) in board.mask.footprints.iter().enumerate() {
        fp_index.insert(f.id.as_str(), i);
    }
    let mut fps: Vec<FpInfo> = board
        .mask
        .footprints
        .iter()
        .map(|f| {
            let groups = parse_net_tie_groups(&f.net_tie_groups);
            FpInfo { allow_bridges: f.allow_bridges, is_net_tie: f.net_tie_groups.iter().any(|g| !g.is_empty()), groups, cache: HashMap::new(), pads: Vec::new() }
        })
        .collect();
    for (i, p) in board.pads.iter().enumerate() {
        let pm = board.mask.pads.get(i);
        let (cu, mask) = match pm.filter(|m| !m.layers.is_empty()) {
            Some(m) => {
                let has = |n: &str| m.layers.iter().any(|l| l == n);
                ([has("F.Cu"), has("B.Cu")], [has("F.Mask"), has("B.Mask")])
            }
            None => {
                let has_cu = |n: &str| p.layers.iter().any(|l| l == n);
                let cu = [has_cu("F.Cu"), has_cu("B.Cu")];
                match p.kind {
                    PadKind::Smd => (cu, [cu[FRONT], cu[BACK]]),
                    _ => (cu, [true, true]),
                }
            }
        };
        let (shape_kind, size) = pm.map(|m| (m.shape, m.size)).unwrap_or((eda_model::PadShape::Rect, (0, 0)));
        let net = id_of(p.net.as_deref());
        let free = p.net.as_deref().is_some_and(|n| n.starts_with("unconnected-(")) && pm.is_some_and(|m| m.pin_type == "free");
        let mask_only = (mask[FRONT] || mask[BACK]) && !cu[FRONT] && !cu[BACK];
        let shapes = vec![p.copper.clone()];
        let fp = pm.and_then(|m| m.fp).or_else(|| fp_index.get(p.footprint_ref.as_str()).copied());
        let idx = items.len();
        if let Some(f) = fp {
            fps[f].pads.push(idx);
        }
        items.push(Item {
            kind: Kind::Pad,
            idx: i,
            fp,
            net,
            cu,
            mask,
            bbox: shapes_bbox(&shapes),
            shapes,
            aperture: free || mask_only,
            npth_no_cu: npth_with_no_copper(p, shape_kind, size),
            pad_number: Some(p.number.clone()),
            is_pad: true,
        });
    }

    // Mask-layer graphics and mask-only pads.
    for (i, g) in board.mask.graphics.iter().enumerate() {
        let Some(side) = layer_side(&g.layer) else { continue };
        if !g.layer.ends_with(".Mask") || g.shapes.is_empty() {
            continue;
        }
        let mut mask = [false; 2];
        mask[side] = true;
        let (is_pad, net, number) = match &g.pad {
            Some(p) => (true, id_of(p.net.as_deref()), Some(p.number.clone())),
            None => (false, -1, None),
        };
        let free = g.pad.as_ref().is_some_and(|p| p.net.as_deref().is_some_and(|n| n.starts_with("unconnected-(")) && p.pin_type == "free");
        let idx = items.len();
        if let (true, Some(f)) = (is_pad, g.fp) {
            fps[f].pads.push(idx);
        }
        items.push(Item { kind: Kind::Graphic, idx: i, fp: g.fp, net, cu: [false, false], mask, bbox: shapes_bbox(&g.shapes), shapes: g.shapes.clone(), aperture: true || free, npth_no_cu: false, pad_number: number, is_pad });
    }

    // `FOOTPRINT::BuildNetTieCache`: a pad in a group with at least one
    // other pad may touch the nets of every pad in that group.
    for f in fps.iter_mut() {
        if !f.is_net_tie {
            continue;
        }
        let pads = f.pads.clone();
        for &a in &pads {
            let Some(&ga) = f.groups.get(items[a].pad_number.as_deref().unwrap_or("")) else { continue };
            let mates: Vec<usize> = pads.iter().copied().filter(|&b| b != a && f.groups.get(items[b].pad_number.as_deref().unwrap_or("")) == Some(&ga)).collect();
            if mates.is_empty() {
                continue;
            }
            let e = f.cache.entry(a).or_default();
            e.insert(items[a].net);
            for b in mates {
                e.insert(items[b].net);
            }
        }
    }
    (items, fps)
}

// ---------------------------------------------------------------------------
// testSilkToMaskClearance (only when `m_SolderMaskMinWidth > 0`)
// ---------------------------------------------------------------------------

/// KiCad's `m_MaxError` (`ARC_HIGH_DEF`), in um.
const MASK_MAX_ERROR: i64 = 5;

/// A DRC shape as `eda_zone_filler` shapes (a stroked compound becomes one
/// stadium per segment).
fn fill_shapes(shapes: &[Shape]) -> Vec<FillShape> {
    let pt = |p: Point| Point64::new(p.x, p.y);
    let mut out = Vec::new();
    for s in shapes {
        match s {
            Shape::Circle { c, r } => out.push(FillShape::Circle { c: pt(*c), r: *r }),
            Shape::Stadium { a, b, r } => out.push(FillShape::Stadium { a: pt(*a), b: pt(*b), r: *r }),
            Shape::Rect { x0, y0, x1, y1 } => out.push(FillShape::Rect { x0: *x0, y0: *y0, x1: *x1, y1: *y1 }),
            Shape::RoundRect { x0, y0, x1, y1, r } => out.push(FillShape::RoundRect { x0: *x0, y0: *y0, x1: *x1, y1: *y1, r: *r }),
            Shape::Polygon { pts } => out.push(FillShape::Polygon { pts: pts.iter().map(|&p| pt(p)).collect() }),
            Shape::Strokes { segs, r } => {
                for sg in segs {
                    out.push(FillShape::Stadium { a: pt(sg.a), b: pt(sg.b), r: *r });
                }
            }
        }
    }
    out
}

/// Does `shapes` touch the filled region of `poly` (outline minus holes)?
/// A boundary contact is a collision (the region is closed); otherwise the
/// shape is wholly inside or outside every ring, decided by a probe point.
fn touches_region(shapes: &[Shape], poly: &[Vec<Point64>]) -> bool {
    let ring_pts = |r: &Vec<Point64>| -> Vec<Point> { r.iter().map(|p| Point { x: p.x, y: p.y }).collect() };
    let outline_pts = ring_pts(&poly[0]);
    if outline_pts.len() < 3 {
        return false;
    }
    let outline = Shape::Polygon { pts: outline_pts.clone() };
    let ring_edges = |pts: &[Point]| -> Shape {
        let n = pts.len();
        Shape::Strokes { segs: (0..n).map(|i| crate::kimath::Seg::new(pts[i], pts[(i + 1) % n])).collect(), r: 0 }
    };
    for s in shapes {
        // Touches the outline boundary or a hole boundary: collision.
        if s.collides(&ring_edges(&outline_pts), 0).is_some() {
            return true;
        }
        for h in &poly[1..] {
            let hp = ring_pts(h);
            if hp.len() >= 3 && s.collides(&ring_edges(&hp), 0).is_some() {
                return true;
            }
        }
        // No boundary contact: inside the outline (or enclosing it)?
        if s.collides(&outline, 0).is_none() {
            continue;
        }
        // Inside the outline: a collision unless it sits wholly inside a hole.
        let probe = shape_probe(s);
        let in_hole = poly[1..].iter().any(|h| {
            let hp = ring_pts(h);
            hp.len() >= 3 && Shape::Polygon { pts: hp }.collides(&Shape::Circle { c: probe, r: 0 }, 0).is_some()
        });
        if !in_hole {
            return true;
        }
    }
    false
}

fn shape_probe(s: &Shape) -> Point {
    let (x0, y0, x1, y1) = s.bbox(0);
    match s {
        Shape::Circle { c, .. } => *c,
        Shape::Stadium { a, .. } => *a,
        Shape::Strokes { segs, .. } => segs[0].a,
        Shape::Polygon { pts } => pts[0],
        _ => Point { x: (x0 + x1) / 2, y: (y0 + y1) / 2 },
    }
}

impl<'a> Ctx<'a> {
    /// `addItemToRTrees` + `buildRTrees`' whole-board mask polygon for one side:
    /// every item on the mask layer, grown by `web/2` plus its own mask
    /// expansion, unioned (`Simplify`), then deflated by `web/2`.
    fn mask_region(&self, side: usize) -> ShapePolySet {
        let mut set = ShapePolySet::new();
        for it in &self.items {
            if !it.mask[side] {
                continue;
            }
            let clearance = self.web / 2 + self.expansion_of(it, side);
            for sh in fill_shapes(&it.shapes) {
                let poly = shape_to_polygon_outside(&sh, clearance, MASK_MAX_ERROR);
                if poly.len() >= 3 {
                    set.add_outline(poly);
                }
            }
        }
        set.simplify();
        if self.web > 0 {
            set.deflate(self.web / 2, CornerStrategy::ChamferAllCorners, MASK_MAX_ERROR as i32);
        }
        set
    }

    /// `testSilkToMaskClearance`: silkscreen that touches the healed mask
    /// aperture polygon.
    fn test_silk_to_mask_clearance(&mut self, silk: &[Vec<crate::providers::silk_mask::Ent>; 2]) {
        let mut silk_hits = 0usize;
        for side in [FRONT, BACK] {
            if silk[side].is_empty() {
                continue;
            }
            let region = self.mask_region(side);
            let boxes: Vec<(Um, Um, Um, Um)> = region
                .polys
                .iter()
                .map(|p| {
                    let (mut x0, mut y0, mut x1, mut y1) = (Um::MAX, Um::MAX, Um::MIN, Um::MIN);
                    for pt in &p[0] {
                        x0 = x0.min(pt.x);
                        y0 = y0.min(pt.y);
                        x1 = x1.max(pt.x);
                        y1 = y1.max(pt.y);
                    }
                    (x0, y0, x1, y1)
                })
                .collect();
            for ent in &silk[side] {
                if silk_hits >= ERROR_LIMIT {
                    return;
                }
                for (pi, poly) in region.polys.iter().enumerate() {
                    let (b, e) = (boxes[pi], ent.bbox);
                    if e.2 < b.0 || b.2 < e.0 || e.3 < b.1 || b.3 < e.1 {
                        continue;
                    }
                    if touches_region(&ent.shapes, poly) {
                        self.out.push(DrcViolation::new(ErrorType::SilkOverCopper, "", vec![ent.refitem.clone()]));
                        silk_hits += 1;
                        break;
                    }
                }
            }
        }
    }
}

pub fn check(board: &DrcBoard, rules: &BoardRules, fills: &FillResults) -> Vec<DrcViolation> {
    let _ = rules;
    let mut net_ids: HashMap<String, i32> = HashMap::new();
    let (items, fps) = build_items(board, &mut net_ids);
    for z in &board.zones {
        if let Some(n) = z.net.as_deref() {
            let next = net_ids.len() as i32 + 1;
            net_ids.entry(n.to_string()).or_insert(next);
        }
    }

    let mask = &board.mask.rules;
    let mut trees = [DrcRTree::new(2000), DrcRTree::new(2000)];
    for (i, it) in items.iter().enumerate() {
        for side in [FRONT, BACK] {
            if it.mask[side] && !it.shapes.is_empty() {
                trees[side].insert(i, it.bbox);
            }
        }
    }
    let zone_frags: Vec<(Vec<Shape>, (Um, Um, Um, Um))> = board
        .zones
        .iter()
        .map(|z| {
            let frags = fills.fragments_or(&z.id, z.shape());
            let bb = if frags.is_empty() { (0, 0, -1, -1) } else { shapes_bbox(&frags) };
            (frags, bb)
        })
        .collect();

    let mut ctx = Ctx {
        board,
        items,
        fps,
        web: mask.min_width_um,
        to_copper: mask.to_copper_clearance_um,
        board_expansion: mask.expansion_um,
        trees,
        checked: HashSet::new(),
        aperture_first: HashMap::new(),
        aperture_all: HashMap::new(),
        pending: Vec::new(),
        out: Vec::new(),
        bridges: 0,
        zone_frags,
        net_ids,
        largest: 0,
    };
    ctx.largest = {
        // `m_largestClearance += m_largestClearance + m_webWidth`, then max
        // with the solder-mask-to-copper clearance.
        let mut m: Um = 0;
        for it in &ctx.items {
            let e = match it.kind {
                Kind::Pad | Kind::Graphic => ctx.expansion_of(it, FRONT).max(ctx.expansion_of(it, BACK)),
                Kind::Via | Kind::Track => ctx.board_expansion,
            };
            m = m.max(e);
        }
        (m + m + ctx.web).max(ctx.to_copper).max(0)
    };

    // `testMaskBridges`.
    for ai in 0..ctx.items.len() {
        if ctx.bridges >= ERROR_LIMIT {
            break;
        }
        let (m, cu, npth) = (ctx.items[ai].mask, ctx.items[ai].cu, ctx.items[ai].npth_no_cu);
        for side in [FRONT, BACK] {
            if m[side] && !npth {
                // aperture-to-aperture, then aperture-to-zone
                ctx.test_item_against_items(ai, true, side);
                ctx.test_mask_item_against_zones(ai, side);
            } else if cu[side] {
                // copper-item-to-aperture
                ctx.test_item_against_items(ai, false, side);
            }
        }
    }
    ctx.report_pending();
    // `testSilkToMaskClearance` returns immediately when the web width is 0 (the
    // silk-clearance provider tests individual mask items instead).
    if ctx.web > 0 {
        let silk = crate::providers::silk_mask::silk_ents(board);
        ctx.test_silk_to_mask_clearance(&silk);
    }
    let _ = Side::Top;
    ctx.out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::board::{DrcFpMask, DrcGraphic, DrcPad, DrcPadMask, DrcSilk, DrcTrackSeg};
    use eda_model::PadShape;

    fn pad(footprint: &str, number: &str, net: Option<&str>, x: Um) -> DrcPad {
        DrcPad {
            id: format!("{footprint}.{number}"),
            footprint_ref: footprint.into(),
            number: number.into(),
            net: net.map(String::from),
            center: Point { x, y: 0 },
            side: Side::Top,
            kind: PadKind::Smd,
            layers: vec!["F.Cu".into()],
            copper: Shape::Rect { x0: x - 250, y0: -250, x1: x + 250, y1: 250 },
            hole: None,
            drill_round: None,
            drill_slot: None,
        }
    }

    fn pad_mask(fp: Option<usize>) -> DrcPadMask {
        DrcPadMask { fp, layers: vec![], margin: None, tent_front: None, tent_back: None, pin_type: String::new(), shape: PadShape::Rect, size: (500, 500) }
    }

    fn board(pads: Vec<DrcPad>) -> DrcBoard {
        let n = pads.len();
        let mut b = DrcBoard { copper_graphics: vec![], layers: vec!["F.Cu".into(), "B.Cu".into()], outline: vec![], pads, tracks: vec![], vias: vec![], zones: vec![], keepouts: vec![], footprints: vec![], shapes: vec![], texts: vec![], silk_items: vec![], mask: Default::default() };
        b.mask.pads = (0..n).map(|_| pad_mask(None)).collect();
        b
    }

    fn run(b: &DrcBoard) -> Vec<DrcViolation> {
        check(b, &BoardRules::default(), &crate::fill::fill_all_zones(b, &BoardRules::default()))
    }

    fn bridges(v: &[DrcViolation]) -> usize {
        v.iter().filter(|v| v.error_type == "solder_mask_bridge").count()
    }

    #[test]
    fn touching_apertures_of_different_nets_bridge() {
        // 0 mm expansion: bare pads 100 um apart do not bridge, touching ones do.
        let apart = board(vec![pad("U1", "1", Some("A"), 0), pad("U1", "2", Some("B"), 600)]);
        assert_eq!(bridges(&run(&apart)), 0);
        let touching = board(vec![pad("U1", "1", Some("A"), 0), pad("U1", "2", Some("B"), 500)]);
        assert_eq!(bridges(&run(&touching)), 1);
    }

    #[test]
    fn mask_expansion_grows_both_apertures() {
        // Pads 100 um apart; a 60 um pad_to_mask_clearance makes the apertures (2 x 60) overlap.
        let mut b = board(vec![pad("U1", "1", Some("A"), 0), pad("U1", "2", Some("B"), 600)]);
        b.mask.rules.expansion_um = 60;
        assert_eq!(bridges(&run(&b)), 1);
        // A pad's own margin overrides the board's; zero on both removes it.
        b.mask.pads[0].margin = Some(0);
        b.mask.pads[1].margin = Some(0);
        assert_eq!(bridges(&run(&b)), 0);
    }

    #[test]
    fn min_web_width_is_enforced_between_apertures() {
        let mut b = board(vec![pad("U1", "1", Some("A"), 0), pad("U1", "2", Some("B"), 600)]);
        b.mask.rules.min_width_um = 150; // 100 um gap < 150 um web
        assert_eq!(bridges(&run(&b)), 1);
        b.mask.rules.min_width_um = 80;
        assert_eq!(bridges(&run(&b)), 0);
    }

    #[test]
    fn same_net_and_same_logical_pad_never_bridge() {
        let same_net = board(vec![pad("U1", "1", Some("A"), 0), pad("U2", "1", Some("A"), 500)]);
        assert_eq!(bridges(&run(&same_net)), 0);
        // Two physical pads sharing one pad number are one logical pad.
        let mut logical = board(vec![pad("U1", "1", Some("A"), 0), pad("U1", "1", Some("B"), 500)]);
        logical.mask.footprints = vec![DrcFpMask { id: "U1".into(), margin: None, allow_bridges: false, net_tie_groups: vec![] }];
        logical.mask.pads = vec![pad_mask(Some(0)), pad_mask(Some(0))];
        assert_eq!(bridges(&run(&logical)), 0);
    }

    #[test]
    fn a_footprint_that_allows_bridges_is_skipped() {
        let mut b = board(vec![pad("U1", "1", Some("A"), 0), pad("U1", "2", Some("B"), 500)]);
        b.mask.footprints = vec![DrcFpMask { id: "U1".into(), margin: None, allow_bridges: true, net_tie_groups: vec![] }];
        b.mask.pads = vec![pad_mask(Some(0)), pad_mask(Some(0))];
        assert_eq!(bridges(&run(&b)), 0);
        // ... and so is a board that allows them in every footprint.
        let mut c = board(vec![pad("U1", "1", Some("A"), 0), pad("U1", "2", Some("B"), 500)]);
        c.mask.footprints = vec![DrcFpMask { id: "U1".into(), margin: None, allow_bridges: false, net_tie_groups: vec![] }];
        c.mask.pads = vec![pad_mask(Some(0)), pad_mask(Some(0))];
        assert_eq!(bridges(&run(&c)), 1);
        c.mask.rules.allow_bridges_in_footprints = true;
        assert_eq!(bridges(&run(&c)), 0);
    }

    #[test]
    fn net_tie_pads_may_share_an_aperture() {
        let mut b = board(vec![pad("NT1", "1", Some("A"), 0), pad("NT1", "2", Some("B"), 500)]);
        b.mask.footprints = vec![DrcFpMask { id: "NT1".into(), margin: None, allow_bridges: false, net_tie_groups: vec!["1, 2".into()] }];
        b.mask.pads = vec![pad_mask(Some(0)), pad_mask(Some(0))];
        assert_eq!(bridges(&run(&b)), 0);
        b.mask.footprints[0].net_tie_groups.clear();
        assert_eq!(bridges(&run(&b)), 1);
    }

    #[test]
    fn a_track_touching_a_foreign_pad_aperture_bridges() {
        let mut b = board(vec![pad("U1", "1", Some("A"), 0)]);
        b.tracks = vec![DrcTrackSeg { id: "t".into(), net: Some("B".into()), layer: "F.Cu".into(), width: 200, a: Point { x: 340, y: 0 }, b: Point { x: 2000, y: 0 }, arc_mid: None }];
        assert_eq!(bridges(&run(&b)), 1);
        b.tracks[0].net = Some("A".into());
        assert_eq!(bridges(&run(&b)), 0);
    }

    #[test]
    fn a_mask_aperture_bridges_only_when_it_exposes_two_nets() {
        // One mask-only opening (a graphic on F.Mask) covering two pads.
        let mut b = board(vec![pad("U1", "1", Some("A"), 0), pad("U2", "1", Some("B"), 1000)]);
        let aperture = DrcGraphic {
            fp: None,
            id: "g".into(),
            desc: "Graphic on F.Mask".into(),
            pos: Point { x: 0, y: 0 },
            layer: "F.Mask".into(),
            shapes: vec![Shape::Polygon { pts: vec![Point { x: -300, y: -300 }, Point { x: 1300, y: -300 }, Point { x: 1300, y: 300 }, Point { x: -300, y: 300 }] }],
            margin: None,
            filled: true,
            width: 0,
            pad: None,
        };
        b.mask.graphics = vec![aperture.clone()];
        let v = run(&b);
        assert!(v.iter().any(|v| v.error_type == "solder_mask_bridge" && v.items.len() == 3), "{v:#?}");
        // Only one net inside it: no bridge.
        let mut one = board(vec![pad("U1", "1", Some("A"), 0), pad("U2", "1", Some("A"), 1000)]);
        one.mask.graphics = vec![aperture];
        assert!(run(&one).iter().all(|v| v.items.len() != 3));
    }

    #[test]
    fn a_tented_via_is_not_a_mask_item_but_its_copper_still_bridges_pad_apertures() {
        let mut b = board(vec![pad("U1", "1", Some("A"), 0)]);
        b.vias = vec![crate::board::DrcVia { id: "v".into(), net: Some("B".into()), at: Point { x: 600, y: 0 }, drill: 200, diameter: 500, from_layer: "F.Cu".into(), to_layer: "B.Cu".into() }];
        b.mask.vias = vec![Default::default()];
        // Tented by default: only the via's copper meets the pad's aperture (0 clearance); 100 um gap, no hit.
        assert_eq!(bridges(&run(&b)), 0);
        b.vias[0].at = Point { x: 500, y: 0 };
        assert_eq!(bridges(&run(&b)), 1);
    }

    #[test]
    fn silk_on_a_healed_mask_aperture_is_silk_over_copper() {
        let mut b = board(vec![pad("U1", "1", Some("A"), 0)]);
        b.mask.rules.min_width_um = 100;
        b.mask.silk = vec![DrcSilk { fp: None, id: "s".into(), desc: "silk".into(), pos: Point { x: 0, y: 0 }, layer: "F.SilkS".into(), shapes: vec![Shape::Stadium { a: Point { x: -1000, y: 0 }, b: Point { x: 1000, y: 0 }, r: 50 }], is_shape: true }];
        let v = run(&b);
        assert_eq!(v.iter().filter(|v| v.error_type == "silk_over_copper").count(), 1, "{v:#?}");
        // Far from every aperture: nothing.
        b.mask.silk[0].shapes = vec![Shape::Stadium { a: Point { x: -1000, y: 5000 }, b: Point { x: 1000, y: 5000 }, r: 50 }];
        assert!(run(&b).iter().all(|v| v.error_type != "silk_over_copper"));
    }
}
