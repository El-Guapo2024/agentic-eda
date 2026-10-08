//! One module's sheet: the anchor (an IC, a connector) in the middle with every pin drawn out as a power symbol, a hierarchical
//! label or a local label; the passives that serve it as chains below; repeated channels as a tidy row of identical chains.
//!
//! The layout is built from blocks. A block is drawn on its own, from the origin, with every item's rectangle known; blocks are then
//! packed into rows on the smallest paper that holds them. Nothing is routed: a connection is either a power symbol, a short wire
//! between two parts that are neighbours in a chain, or a short wire out to a label of the same name -- so a net can never touch
//! another net by accident, and the tracer in `nets.rs` reads back exactly what was meant.

use eda_layout::Side;
use eda_model::ir::Point;
use eda_model::modules::{is_anchor_part, natural_cmp, FunctionalModule, ModuleKind};

use super::items::{Ctx, Items, NetClass, RailPin, View};
use super::kit::{side_dir, snap_down, snap_up, Paper, Placed, Rect, G, PAPERS};

/// A symbol ready to place, resolved the way `derive_schematic` resolves it (or as the existing schematic had it).
pub fn placed(ctx: &Ctx, reference: &str) -> Placed {
    let part = ctx.model.part(reference).unwrap_or_else(|| panic!("module part {reference} is in the model"));
    let kept = ctx.keep.symbols.get(reference);
    let lib_id = match kept.map(|k| k.lib_id.clone()).filter(|l| !l.is_empty()) {
        Some(kept_id) => ctx.model.fitting_lib_id(part, kept_id),
        None => ctx.model.lib_id_of(part),
    };
    let resolved = ctx.model.real_symbol_of(&lib_id, part);
    let value = kept.map(|k| k.value.clone()).unwrap_or_else(|| part.value.clone().unwrap_or_default());
    let footprint = kept.map(|k| k.footprint.clone()).filter(|f| !f.is_empty()).or_else(|| part.package.clone()).unwrap_or_default();
    Placed::new(part, &lib_id, resolved, &value, &footprint)
}

/// One part of a chain: which pin faces the previous part (or the outside, for the first) and which faces the next.
#[derive(Debug, Clone, PartialEq)]
pub struct ChainLink {
    pub r: String,
    pub entry: String,
    pub exit: String,
}

/// A thing to draw.
#[derive(Debug, Clone)]
pub enum BlockPlan {
    /// A part with every pin drawn out on its own (an IC, a connector, or any part that is not a simple two-pin).
    Single(String),
    Chain(Vec<ChainLink>),
}

impl BlockPlan {
    fn refs(&self) -> Vec<String> {
        match self {
            BlockPlan::Single(r) => vec![r.clone()],
            BlockPlan::Chain(l) => l.iter().map(|x| x.r.clone()).collect(),
        }
    }
}

/// A drawn block, from the origin.
pub struct Block {
    pub items: Items,
    pub w: i64,
    pub h: i64,
    /// The anchor's box, for centring.
    pub core: Option<Rect>,
}

fn finish(mut items: Items) -> Block {
    items.normalize();
    let e = items.extent().unwrap_or(Rect { x0: 0, y0: 0, x1: 0, y1: 0 });
    let core = items.core;
    Block { w: snap_up(e.x1), h: snap_up(e.y1), items, core }
}

/// The pins of `part` in the order they are drawn.
fn pins_of(ctx: &Ctx, r: &str) -> Vec<String> {
    ctx.model.part(r).map(|p| p.pins.iter().map(|x| format!("{r}.{}", x.number)).collect()).unwrap_or_default()
}

// ------------------------------------------------------------------------------------------------- plans

/// Satellites of a module joined through non-rail nets (inside the module).
fn components(ctx: &Ctx, view: &View, refs: &[String]) -> Vec<Vec<String>> {
    let idx: std::collections::BTreeMap<&str, usize> = refs.iter().enumerate().map(|(i, r)| (r.as_str(), i)).collect();
    let mut up: Vec<usize> = (0..refs.len()).collect();
    fn find(up: &mut Vec<usize>, mut x: usize) -> usize {
        while up[x] != x {
            up[x] = up[up[x]];
            x = up[x];
        }
        x
    }
    for (net, pins) in &ctx.net_pins {
        if view.class_of(net).is_none() || ctx.is_rail(net) {
            continue;
        }
        let on: Vec<usize> = pins.iter().filter_map(|p| idx.get(p.split('.').next().unwrap_or("")).copied()).collect();
        for w in on.windows(2) {
            let (a, b) = (find(&mut up, w[0]), find(&mut up, w[1]));
            if a != b {
                up[a] = b;
            }
        }
    }
    let mut groups: std::collections::BTreeMap<usize, Vec<String>> = std::collections::BTreeMap::new();
    for (i, r) in refs.iter().enumerate() {
        let root = find(&mut up, i);
        groups.entry(root).or_default().push(r.clone());
    }
    let mut out: Vec<Vec<String>> = groups.into_values().collect();
    for g in &mut out {
        g.sort_by(|a, b| natural_cmp(a, b));
    }
    out.sort_by(|a, b| natural_cmp(&a[0], &b[0]));
    out
}

/// The drawn side of the pin `number` of `r`, unturned.
fn side_of(ctx: &Ctx, r: &str, number: &str) -> Option<Side> {
    placed(ctx, r).tip(number).map(|(_, s)| s)
}

fn opposite(a: Side, b: Side) -> bool {
    matches!((a, b), (Side::Top, Side::Bottom) | (Side::Bottom, Side::Top) | (Side::Left, Side::Right) | (Side::Right, Side::Left))
}

/// A component drawn as a chain: two-pin parts in a simple path, each with its pins on opposite sides. `start` picks the end the
/// chain begins at when it can.
fn as_chain(ctx: &Ctx, comp: &[String], start: Option<&str>) -> Option<Vec<ChainLink>> {
    let two_pin = |r: &String| ctx.model.part(r).is_some_and(|p| p.pins.len() == 2);
    if !comp.iter().all(two_pin) {
        return None;
    }
    let pins = |r: &str| -> Vec<String> { ctx.model.part(r).unwrap().pins.iter().map(|p| p.number.clone()).collect() };
    let net_of = |r: &str, n: &str| ctx.net_of_pin.get(&format!("{r}.{n}")).cloned();
    let valid = |r: &str| -> bool {
        let p = pins(r);
        matches!((side_of(ctx, r, &p[0]), side_of(ctx, r, &p[1])), (Some(a), Some(b)) if opposite(a, b))
    };
    if !comp.iter().all(|r| valid(r)) {
        return None;
    }
    // links: nets with exactly two pins of the component, on different parts
    let mut links: Vec<((String, String), (String, String))> = Vec::new();
    for (net, npins) in &ctx.net_pins {
        if ctx.is_rail(net) {
            continue;
        }
        let inside: Vec<(String, String)> = npins.iter().filter_map(|p| p.split_once('.')).filter(|(r, _)| comp.iter().any(|c| c == r)).map(|(r, n)| (r.to_string(), n.to_string())).collect();
        if inside.len() >= 2 {
            // Only a net of exactly these two pins is a wire between neighbours; one that goes on elsewhere (to the IC, to
            // another sheet) is joined by name, so the parts are not a chain.
            if npins.len() == 2 && inside[0].0 != inside[1].0 {
                links.push((inside[0].clone(), inside[1].clone()));
            } else {
                return None;
            }
        }
    }
    if comp.len() == 1 {
        let r = &comp[0];
        let p = pins(r);
        let (a, b) = (net_of(r, &p[0]), net_of(r, &p[1]));
        let (entry, exit) = if ctx.rank(a.as_deref()) <= ctx.rank(b.as_deref()) { (p[0].clone(), p[1].clone()) } else { (p[1].clone(), p[0].clone()) };
        return Some(vec![ChainLink { r: r.clone(), entry, exit }]);
    }
    if links.len() != comp.len() - 1 {
        return None;
    }
    let degree = |r: &str| links.iter().filter(|(a, b)| a.0 == r || b.0 == r).count();
    if comp.iter().any(|r| degree(r) == 0 || degree(r) > 2) {
        return None;
    }
    let mut ends: Vec<&String> = comp.iter().filter(|r| degree(r) == 1).collect();
    if ends.len() != 2 {
        return None;
    }
    ends.sort_by(|a, b| natural_cmp(a, b));
    let first = start.and_then(|s| ends.iter().find(|e| e.as_str() == s).copied()).unwrap_or(ends[0]).clone();
    // walk from `first`
    let mut order: Vec<ChainLink> = Vec::new();
    let mut cur = first;
    let mut came_in: Option<String> = None; // the pin of `cur` that links back
    loop {
        let link = links.iter().find(|(a, b)| (a.0 == cur && Some(&a.1) != came_in.as_ref()) || (b.0 == cur && Some(&b.1) != came_in.as_ref()));
        let p = pins(&cur);
        match link {
            Some((a, b)) => {
                let (mine, theirs) = if a.0 == cur { (a, b) } else { (b, a) };
                let entry = came_in.clone().unwrap_or_else(|| p.iter().find(|x| **x != mine.1).cloned().unwrap());
                order.push(ChainLink { r: cur.clone(), entry, exit: mine.1.clone() });
                came_in = Some(theirs.1.clone());
                cur = theirs.0.clone();
            }
            None => {
                let entry = came_in.clone()?;
                let exit = p.iter().find(|x| **x != entry).cloned()?;
                order.push(ChainLink { r: cur.clone(), entry, exit });
                break;
            }
        }
        if order.len() > comp.len() {
            return None;
        }
    }
    if order.len() != comp.len() {
        return None;
    }
    // Supply at the top, ground at the bottom.
    let start_net = net_of(&order[0].r, &order[0].entry);
    let end_net = net_of(&order.last().unwrap().r, &order.last().unwrap().exit);
    if ctx.rank(start_net.as_deref()) > ctx.rank(end_net.as_deref()) {
        order.reverse();
        for l in &mut order {
            std::mem::swap(&mut l.entry, &mut l.exit);
        }
    }
    Some(order)
}

/// What to draw for a module, in drawing order: anchors first, then the rest.
pub fn plan(ctx: &Ctx, view: &View, module: &FunctionalModule) -> (Vec<BlockPlan>, Vec<BlockPlan>) {
    let mut anchors: Vec<BlockPlan> = Vec::new();
    let mut rest: Vec<BlockPlan> = Vec::new();
    let part_is_anchor = |r: &String| module.kind != ModuleKind::Channels && ctx.model.part(r).is_some_and(is_anchor_part);
    let mut anchor_refs: Vec<String> = module.refs.iter().filter(|r| part_is_anchor(r)).cloned().collect();
    // by reference, the lead first
    anchor_refs.sort_by(|a, b| natural_cmp(a, b));
    if let Some(lead) = module.lead.as_ref() {
        if let Some(i) = anchor_refs.iter().position(|r| r == lead) {
            let l = anchor_refs.remove(i);
            anchor_refs.insert(0, l);
        }
    }
    for r in &anchor_refs {
        anchors.push(BlockPlan::Single(r.clone()));
    }
    let sats: Vec<String> = module.refs.iter().filter(|r| !part_is_anchor(r)).cloned().collect();
    // Channels come already ordered; otherwise group satellites by the nets they share.
    let comps: Vec<(Vec<String>, Option<String>)> = if module.kind == ModuleKind::Channels && !module.channels.is_empty() {
        module.channels.iter().map(|c| (c.clone(), c.first().cloned())).collect()
    } else {
        components(ctx, view, &sats).into_iter().map(|c| (c, None)).collect()
    };
    let mut signal_chains: Vec<BlockPlan> = Vec::new();
    let mut rail_only: Vec<BlockPlan> = Vec::new();
    for (comp, start) in comps {
        let touches_signal = comp.iter().any(|r| pins_of(ctx, r).iter().any(|p| ctx.net_of_pin.get(p).is_some_and(|n| !ctx.is_rail(n))));
        match as_chain(ctx, &comp, start.as_deref()) {
            Some(chain) => {
                if touches_signal {
                    signal_chains.push(BlockPlan::Chain(chain));
                } else {
                    rail_only.push(BlockPlan::Chain(chain));
                }
            }
            None => {
                for r in comp {
                    signal_chains.push(BlockPlan::Single(r));
                }
            }
        }
    }
    rest.extend(signal_chains);
    rest.extend(rail_only);
    (anchors, rest)
}

// ------------------------------------------------------------------------------------------------- blocks

/// A part with every pin drawn out.
fn single_block(ctx: &Ctx, view: &View, r: &str) -> Block {
    let mut p = placed(ctx, r);
    p.x = 0;
    p.y = 0;
    let mut items = Items::default();
    items.symbol(ctx, &p);
    items.core = Some(p.box_rect());
    let mut rail: Vec<RailPin> = Vec::new();
    let mut others: Vec<(String, Point, Side)> = Vec::new();
    for pin in &p.part.pins {
        if let Some((tip, side)) = p.tip(&pin.number) {
            let pin_ref = format!("{r}.{}", pin.number);
            match ctx.net_of_pin.get(&pin_ref).filter(|n| view.class_of(n) == Some(NetClass::Rail)) {
                Some(net) => rail.push(RailPin { flag: ctx.flag_pins.contains(&pin_ref), pin_ref, tip, side, net: net.clone() }),
                None => others.push((pin_ref, tip, side)),
            }
        }
    }
    // the rails first: pins side by side on one rail share a symbol, out beyond the part's texts; the labels then keep clear of them
    items.rail_pins(rail);
    for (pin_ref, tip, side) in others {
        items.connect(ctx, view, &pin_ref, tip, side);
    }
    finish(items)
}

/// Candidate extra offsets for a part that does not fit where the path first puts it: the first fits, the rest nudge it along.
fn nudges(vertical_to_vertical: bool, horizontal_to_horizontal: bool) -> Vec<(i64, i64)> {
    let mut v = Vec::new();
    for k in 0..12 {
        if vertical_to_vertical {
            v.push((0, k * G));
        } else if horizontal_to_horizontal {
            v.push((k * G, 0));
        } else {
            for a in 0..=k {
                v.push((a * G, (k - a) * G));
            }
        }
    }
    v
}

fn chain_block(ctx: &Ctx, view: &View, links: &[ChainLink]) -> Block {
    let g = 2 * G;
    let mut items = Items::default();
    let mut prev: Option<(Point, bool, String)> = None; // (exit tip, previous was vertical, "REF.PIN" of the exit)
    let mut placed_all: Vec<Placed> = Vec::new();
    for link in links {
        let mut p = placed(ctx, &link.r);
        // Vertical when the entry pin is on the top or bottom of the box; turned so the entry faces the way the path comes from.
        let (_, entry_side) = p.tip(&link.entry).expect("a chain part's entry pin has a port");
        let vertical = matches!(entry_side, Side::Top | Side::Bottom);
        p.flip = if vertical { entry_side != Side::Top } else { entry_side != Side::Left };
        let base = match &prev {
            None => Point { x: 0, y: 0 },
            Some((e, pv, _)) => match (*pv, vertical) {
                (true, true) => Point { x: e.x, y: e.y + g },
                (false, false) => Point { x: e.x + g, y: e.y },
                _ => Point { x: e.x + g, y: e.y + g },
            },
        };
        let (vv, hh) = match &prev {
            Some((_, pv, _)) => (*pv && vertical, !*pv && !vertical),
            None => (false, false),
        };
        let mut target = base;
        for (ex, ey) in nudges(vv, hh) {
            target = Point { x: base.x + ex, y: base.y + ey };
            p.place_tip(&link.entry, target);
            let ko = p.keepout();
            if placed_all.iter().all(|q| !q.keepout().inflate(G / 2).overlaps(&ko)) {
                break;
            }
        }
        p.place_tip(&link.entry, target);
        if let Some((e, pv, exit_ref)) = &prev {
            let pts = match (*pv, vertical) {
                (true, true) | (false, false) => vec![*e, target],
                (true, false) => vec![*e, Point { x: e.x, y: target.y }, target],
                (false, true) => vec![*e, Point { x: target.x, y: e.y }, target],
            };
            let entry_ref = format!("{}.{}", link.r, link.entry);
            let net = ctx.net_of_pin.get(&entry_ref).cloned().unwrap_or_default();
            items.wire(&net, vec![exit_ref.clone(), entry_ref], pts);
        }
        items.symbol(ctx, &p);
        let (exit_tip, _) = p.tip(&link.exit).expect("a chain part's exit pin has a port");
        prev = Some((exit_tip, vertical, format!("{}.{}", link.r, link.exit)));
        placed_all.push(p);
    }
    // The two ends of the chain meet whatever their nets are.
    let first = links.first().unwrap();
    let last = links.last().unwrap();
    let pf = placed_all.first().unwrap();
    let pl = placed_all.last().unwrap();
    if let Some((tip, side)) = pf.tip(&first.entry) {
        items.connect(ctx, view, &format!("{}.{}", first.r, first.entry), tip, side);
    }
    if let Some((tip, side)) = pl.tip(&last.exit) {
        items.connect(ctx, view, &format!("{}.{}", last.r, last.exit), tip, side);
    }
    finish(items)
}

// ------------------------------------------------------------------------------------------- arrangement

fn shelf(sizes: &[(i64, i64)], idx: &[usize], max_w: i64, gap: i64) -> Vec<Vec<usize>> {
    let mut rows: Vec<Vec<usize>> = Vec::new();
    let mut width = 0;
    for &i in idx {
        let w = sizes[i].0;
        match rows.last_mut() {
            Some(row) if width + gap + w <= max_w => {
                row.push(i);
                width += gap + w;
            }
            _ => {
                rows.push(vec![i]);
                width = w;
            }
        }
    }
    rows
}

/// Block origins (top-left) for two groups of blocks, rows centred on one another; and the content's size.
fn arrange(sizes: &[(i64, i64)], groups: [&[usize]; 2], max_w: i64) -> (Vec<(usize, i64, i64)>, i64, i64) {
    let gap_x = 3 * G;
    let mut all_rows: Vec<(Vec<usize>, bool)> = Vec::new(); // (row, first row of the second group)
    for (gi, g) in groups.iter().enumerate() {
        for (ri, row) in shelf(sizes, g, max_w, gap_x).into_iter().enumerate() {
            all_rows.push((row, gi == 1 && ri == 0));
        }
    }
    let row_w = |row: &Vec<usize>| row.iter().map(|&i| sizes[i].0).sum::<i64>() + gap_x * (row.len() as i64 - 1);
    let content_w = all_rows.iter().map(|(r, _)| row_w(r)).max().unwrap_or(0);
    let mut out = Vec::new();
    let mut y = 0;
    for (n, (row, group_break)) in all_rows.iter().enumerate() {
        if n > 0 {
            y += if *group_break { 7 * G } else { 3 * G };
        }
        let h = row.iter().map(|&i| sizes[i].1).max().unwrap_or(0);
        let mut x = snap_down((content_w - row_w(row)) / 2);
        for &i in row {
            out.push((i, x, y));
            x += sizes[i].0 + gap_x;
        }
        y += h;
    }
    (out, content_w, y)
}

/// The finished sheet.
pub struct SheetOut {
    pub items: Items,
    pub paper: Paper,
}

/// Lay one module out.
pub fn layout_module(ctx: &Ctx, module: &FunctionalModule) -> SheetOut {
    let mut view = View::new(ctx, &module.refs);
    let (anchors, rest) = plan(ctx, &view, module);
    let plans: Vec<&BlockPlan> = anchors.iter().chain(rest.iter()).collect();
    // The label carriers, in drawing order.
    let pins_in_order: Vec<String> = plans.iter().flat_map(|p| p.refs()).flat_map(|r| pins_of(ctx, &r)).collect();
    view.assign_carriers(ctx, &pins_in_order);

    let blocks: Vec<Block> = plans
        .iter()
        .map(|p| match p {
            BlockPlan::Single(r) => single_block(ctx, &view, r),
            BlockPlan::Chain(l) => chain_block(ctx, &view, l),
        })
        .collect();
    let sizes: Vec<(i64, i64)> = blocks.iter().map(|b| (b.w, b.h)).collect();
    let a_idx: Vec<usize> = (0..anchors.len()).collect();
    let r_idx: Vec<usize> = (anchors.len()..blocks.len()).collect();

    // The smallest paper that holds the arranged blocks.
    let mut chosen = None;
    for paper in PAPERS {
        let u = paper.usable();
        let (pos, w, h) = arrange(&sizes, [&a_idx, &r_idx], u.w());
        if w <= u.w() && h <= u.h() {
            chosen = Some((paper, pos, w, h));
            break;
        }
    }
    let (paper, pos, w, h) = chosen.unwrap_or_else(|| {
        let p = PAPERS[PAPERS.len() - 1];
        let (pos, w, h) = arrange(&sizes, [&a_idx, &r_idx], p.usable().w());
        (p, pos, w, h)
    });
    let u = paper.usable();
    let mut dx = u.x0 + snap_down((u.w() - w) / 2);
    let dy = u.y0 + snap_down((u.h() - h) / 2);

    // One anchor: its centre on the page's centre line when the rest still fits.
    if anchors.len() == 1 {
        if let Some(&(i, bx, _)) = pos.iter().find(|(i, _, _)| *i == 0) {
            if let Some(core) = blocks[i].core {
                let want = snap_down((u.x0 + u.x1) / 2);
                let have = dx + bx + (core.x0 + core.x1) / 2;
                let shift = snap_down(want - have);
                let lo = u.x0 - dx;
                let hi = u.x1 - (dx + w);
                dx += shift.clamp(lo, hi.max(lo));
            }
        }
    }

    let mut items = Items::default();
    let mut blocks = blocks;
    for (i, bx, by) in pos {
        let mut it = std::mem::take(&mut blocks[i].items);
        it.translate(dx + bx, dy + by);
        items.append(it);
    }
    SheetOut { items, paper }
}

/// Used by the root layout and the tests: the direction a side points.
pub fn outward(side: Side) -> (i64, i64) {
    side_dir(side)
}

/// How a module's pins split: the nets that leave it, and which side of the module they should leave by is the root's concern.
pub fn crossing_nets(ctx: &Ctx, module: &FunctionalModule) -> Vec<String> {
    let mut view = View::new(ctx, &module.refs);
    let (anchors, rest) = plan(ctx, &view, module);
    let pins_in_order: Vec<String> = anchors.iter().chain(rest.iter()).flat_map(|p| p.refs()).flat_map(|r| pins_of(ctx, &r)).collect();
    view.assign_carriers(ctx, &pins_in_order);
    view.crossing.clone()
}
