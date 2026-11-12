//! `fromTo( 'from', 'to' )` of the expression language (`pcbexpr_functions.cpp::fromToFunc`): whether an item lies on the copper path between a pad matching
//! `from` and one matching `to`, by KiCad's `FROM_TO_CACHE` (pcbnew/connectivity/from_to_cache.cpp at 8303b2ad: `buildEndpointList`, `cacheFromToPaths`,
//! `uniquePathBetweenNodes`, `IsOnFromToPath`) over the board's copper items.
//!
//! The items are the connectivity's (`CN_ITEM`): every pad, track segment and via of the board, as the rule board has them ([`RuleBoard::copper_items`]); two
//! are connected as `CN_VISITOR::operator()` connects them -- they share a copper layer and their shapes on it collide, unless both are pads of two nets
//! (`!CanChangeNet()` of both). The endpoints are each pad by `REF-NUMBER` and by `REF`; a path starts at a pad matching `from` (`WildCompareString`, case
//! ignored), ends at the one pad connected to it (zones left out, `EXCLUDE_ZONES`) whose endpoint matches `to` -- two such endpoints and there is no path,
//! as KiCad's `count >= 2` drops it -- and holds the items of the first path the breadth-first search finds (`uniquePathBetweenNodes`, whose search skips a
//! node any queued path holds). The paths of a pair of wildcards are worked out the first time they are asked for and kept.
//!
//! The zones are items too when their fill is known (KiCad 25ec2335 `uniquePathBetweenNodes` walks `ConnectedItems()`, which hold the `CN_ZONE_LAYER`s; the
//! endpoints' island is the zones' too, `EXCLUDE_ZONES` leaving them out of the answer only): each outline of a zone's fill on a layer is one item, joined
//! to an item on that layer whose shape meets it (`checkZoneItemConnection`'s `Collide`) and is not wholly in one of its holes, unless both are of two nets.
//!
//! Differs: a pad's shape is its front one on every layer; two zones are not joined to each other (`checkZoneZoneConnection`).

use crate::pcbexpr::wild_compare;
use crate::rtree::DrcRTree;
use crate::rule_board::RuleBoard;
use crate::rule_item::{Class, RuleItem};
use eda_model::ir::Nm;
use std::cell::RefCell;
use std::collections::{HashSet, VecDeque};
use std::rc::Rc;

/// One path of `cacheFromToPaths` (`FT_PATH`): its wildcards and the items on it.
struct FtPath {
    from: String,
    to: String,
    items: HashSet<usize>,
}

/// `FROM_TO_CACHE` of one board.
pub struct FromToCache {
    /// The studio's id of each item: a pad's `REF.NUMBER`, a track segment's track, a via's.
    ids: Vec<String>,
    /// The rule board's own id of each item (`trk_1#2` for a segment).
    own_ids: Vec<String>,
    adjacency: Vec<Vec<usize>>,
    /// `m_ftEndpoints`: a name and its pad's item.
    endpoints: Vec<(String, usize)>,
    paths: RefCell<Vec<FtPath>>,
}

fn layers_meet(a: &RuleItem, b: &RuleItem) -> bool {
    a.layers.iter().any(|l| l.ends_with(".Cu") && b.layers.contains(l))
}

fn shapes_collide(a: &RuleItem, b: &RuleItem) -> bool {
    a.shapes.iter().any(|sa| b.shapes.iter().any(|sb| sa.gap_to(sb) == 0.0))
}

fn bbox_of(it: &RuleItem) -> (Nm, Nm, Nm, Nm) {
    let mut out = (Nm::MAX, Nm::MAX, Nm::MIN, Nm::MIN);
    for s in &it.shapes {
        let (x0, y0, x1, y1) = s.bbox(Nm::ZERO);
        out = (out.0.min(x0), out.1.min(y0), out.2.max(x1), out.3.max(y1));
    }
    if it.shapes.is_empty() {
        let p = it.position;
        out = (p.x, p.y, p.x, p.y);
    }
    out
}

impl FromToCache {
    /// `FROM_TO_CACHE::Rebuild`: the items, their connections and the endpoints.
    pub fn new(rb: &RuleBoard) -> FromToCache {
        Self::new_with(rb, None)
    }

    /// [`FromToCache::new`] with the zones' fills (`eda_kicad::zone_fills`'s copper): each outline of a fill is an item of its zone.
    pub fn new_with(rb: &RuleBoard, zones: Option<&crate::zone_copper::ZoneCopper>) -> FromToCache {
        let mut items = rb.copper_items();
        let copper_count = items.len();
        // each zone fill outline, with its holes
        let mut holes: Vec<Vec<Vec<eda_model::ir::Point>>> = vec![Vec::new(); items.len()];
        if let (Some(zc), Some(rt)) = (zones, rb.design_routing()) {
            for z in &rt.zones {
                let mut layers: Vec<(&str, &eda_shape_poly_set::ShapePolySet)> = zc.layers_of(&z.id).collect();
                layers.sort_by(|a, b| a.0.cmp(b.0));
                for (layer, fill) in layers {
                    for poly in &fill.polys {
                        let pts = |c: &eda_shape_poly_set::LineChain| -> Vec<eda_model::ir::Point> { c.iter().map(|p| eda_model::ir::Point { x: Nm::new(p.x), y: Nm::new(p.y) }).collect() };
                        let Some(outline) = poly.first() else { continue };
                        if outline.len() < 3 {
                            continue;
                        }
                        let mut it = RuleItem::new(z.id.clone(), Class::Zone);
                        it.net = z.net.clone();
                        it.layers = vec![layer.to_string()];
                        it.shapes = vec![crate::kimath::Shape::Polygon { pts: pts(outline) }];
                        items.push((z.id.clone(), Rc::new(it)));
                        holes.push(poly.iter().skip(1).map(pts).collect());
                    }
                }
            }
        }
        let in_a_hole = |hs: &[Vec<eda_model::ir::Point>], it: &RuleItem| {
            hs.iter().any(|h| {
                let poly = crate::kimath::Shape::Polygon { pts: h.clone() };
                it.shapes.iter().all(|s| {
                    let (x0, y0, x1, y1) = s.bbox(Nm::ZERO);
                    [(x0, y0), (x1, y0), (x1, y1), (x0, y1)].iter().all(|&(x, y)| poly.collides(&crate::kimath::Shape::Circle { c: eda_model::ir::Point { x, y }, r: Nm::ZERO }, Nm::ZERO).is_some()) && poly.gap_to(s) == 0.0 && !poly.boundary_segs().iter().any(|b| s.collides(&crate::kimath::Shape::Stadium { a: b.a, b: b.b, r: Nm::ZERO }, Nm::ZERO).is_some())
                })
            })
        };
        let ids: Vec<String> = items.iter().map(|(id, _)| id.clone()).collect();
        let own_ids: Vec<String> = items.iter().map(|(_, it)| it.id.clone()).collect();
        let mut tree = DrcRTree::new(Nm::new(1_000_000));
        for (i, (_, it)) in items.iter().enumerate() {
            tree.insert(i, bbox_of(it));
        }
        let mut adjacency: Vec<Vec<usize>> = vec![Vec::new(); items.len()];
        for (i, (_, a)) in items.iter().enumerate() {
            for j in tree.query(bbox_of(a)) {
                if j <= i {
                    continue;
                }
                let b = &items[j].1;
                // "Don't connect items in different nets that can't be changed" (a pad and a zone, `CN_ITEM::CanChangeNet` false, as two pads)
                let fixed = |c: Class| c == Class::Pad || c == Class::Zone;
                if fixed(a.class) && fixed(b.class) && a.net != b.net {
                    continue;
                }
                // two zones are not joined here; a zone and an item wholly in one of its holes are not joined
                if i >= copper_count && j >= copper_count {
                    continue;
                }
                if (j >= copper_count && in_a_hole(&holes[j], a)) || (i >= copper_count && in_a_hole(&holes[i], b)) {
                    continue;
                }
                if layers_meet(a, b) && shapes_collide(a, b) {
                    adjacency[i].push(j);
                    adjacency[j].push(i);
                }
            }
        }
        for adj in adjacency.iter_mut() {
            adj.sort();
        }
        // `buildEndpointList`: each pad by `REF-NUMBER` and by `REF`, in the board's order
        let mut endpoints = Vec::new();
        for (i, (_, it)) in items.iter().enumerate() {
            if it.class != Class::Pad {
                continue;
            }
            let reference = it.footprint.as_ref().map(|f| f.reference.clone()).unwrap_or_default();
            let number = match it.prop("Pad Number") {
                Some(crate::rule_item::Prop::Str(n)) => n.clone(),
                _ => String::new(),
            };
            endpoints.push((format!("{reference}-{number}"), i));
            endpoints.push((reference, i));
        }
        FromToCache { ids, own_ids, adjacency, endpoints, paths: RefCell::new(Vec::new()) }
    }

    /// `CONNECTIVITY_DATA::GetConnectedItems( from, EXCLUDE_ZONES )`: the items of `from`'s island.
    fn island(&self, from: usize) -> Vec<usize> {
        let mut seen = vec![false; self.ids.len()];
        let mut queue = VecDeque::from([from]);
        seen[from] = true;
        let mut out = Vec::new();
        while let Some(n) = queue.pop_front() {
            out.push(n);
            for &m in &self.adjacency[n] {
                if !seen[m] {
                    seen[m] = true;
                    queue.push_back(m);
                }
            }
        }
        out
    }

    /// `uniquePathBetweenNodes`: the first path from `u` to `v` of the breadth-first search, `None` when there is none.
    fn path_between(&self, u: usize, v: usize) -> Option<Vec<usize>> {
        let mut queue: VecDeque<Vec<usize>> = VecDeque::from([vec![u]]);
        let mut found: Option<Vec<usize>> = None;
        while let Some(path) = queue.pop_front() {
            let last = *path.last().unwrap_or(&u);
            if last == v {
                if found.is_some() {
                    // PS_MULTIPLE_PATHS: `outPath` is this second path
                    return Some(path);
                }
                found = Some(path.clone());
            }
            for &c in &self.adjacency[last] {
                let visited = path.contains(&c) || queue.iter().any(|p| p.contains(&c));
                if !visited {
                    let mut next = path.clone();
                    next.push(c);
                    queue.push_back(next);
                }
            }
        }
        found
    }

    /// `cacheFromToPaths`.
    fn cache_paths(&self, from: &str, to: &str) {
        let mut new_paths = Vec::new();
        let starts: Vec<usize> = self.endpoints.iter().filter(|(name, _)| wild_compare(from, name, false)).map(|(_, i)| *i).collect();
        for start in starts {
            let mut count = 0;
            let mut end: Option<usize> = None;
            for candidate in self.island(start) {
                if candidate == start {
                    continue;
                }
                for (name, pad) in &self.endpoints {
                    if *pad == candidate && wild_compare(to, name, false) {
                        count += 1;
                        end = if count >= 2 { None } else { Some(candidate) };
                    }
                }
            }
            let Some(end) = end else { continue };
            if let Some(path) = self.path_between(start, end) {
                new_paths.push(FtPath { from: from.to_string(), to: to.to_string(), items: path.into_iter().collect() });
            }
        }
        self.paths.borrow_mut().extend(new_paths);
    }

    /// `IsOnFromToPath`: whether the item (by the rule board's id or the studio's) is on a path of `from` and `to`.
    pub fn is_on_path(&self, item_id: &str, from: &str, to: &str) -> bool {
        for attempt in 0..2 {
            let mut found = 0;
            for p in self.paths.borrow().iter().filter(|p| p.from == from && p.to == to) {
                found += 1;
                if p.items.iter().any(|&i| self.own_ids[i] == item_id || self.ids[i] == item_id) {
                    return true;
                }
            }
            if found > 0 || attempt == 1 {
                return false;
            }
            self.cache_paths(from, to);
        }
        false
    }
}

/// The `Env::from_to` of a board: [`FromToCache::is_on_path`] over its copper (the cache is made here; its paths when first asked for).
pub fn of_board(rb: &RuleBoard) -> Rc<dyn Fn(&str, &str, &str) -> bool> {
    of_board_with(rb, None)
}

/// [`of_board`] with the zones' fills, so a path through a zone is found.
pub fn of_board_with(rb: &RuleBoard, zones: Option<&crate::zone_copper::ZoneCopper>) -> Rc<dyn Fn(&str, &str, &str) -> bool> {
    let cache = Rc::new(FromToCache::new_with(rb, zones));
    Rc::new(move |id: &str, from: &str, to: &str| cache.is_on_path(id, from, to))
}
