//! The root sheet: one sheet symbol per module, the nets that cross modules as sheet pins, wired between the symbols where the two
//! ends face each other and joined by label where they do not.
//!
//! Modules go in columns by what they are: connectors on the left, ICs in the middle, channels and the rest on the right, so the
//! signal reads left to right. A net between two modules in neighbouring columns gets a pin on each facing side and, when it can,
//! one straight horizontal wire (the pins are ordered the same way on both sides and the symbols are shifted so they line up).
//! Anything else -- a net between three modules, two in one column, an order that will not line up -- is a short wire to a local
//! label at each pin: the same name joins them. A wire is only drawn if nothing else in its way would be touched.

use std::collections::{BTreeMap, BTreeSet};

use eda_layout::Side;
use eda_model::ir::{LabelKind, LabelShape, NetLabel, Point, SheetInstance, SheetPin, Wire};
use eda_model::modules::{natural_cmp, FunctionalModule, ModuleKind};

use super::items::Items;
use super::kit::{cap, label_rect, sheet_pin_rect, smallest_paper, snap_down, snap_up, text_w, Paper, Rect, G, SHEET_FILE_FONT, SHEET_NAME_FONT, SHEET_PIN_FONT};

#[derive(Debug, Clone)]
struct PinSpec {
    net: String,
    side: Side,
    /// Sort key: where the partner sits, so both ends list a pair of modules' nets in the same order.
    key: (usize, usize, String),
}

struct SheetBox {
    w: i64,
    h: i64,
    left: Vec<PinSpec>,
    right: Vec<PinSpec>,
}

impl SheetBox {
    fn pin_y(i: usize) -> i64 {
        3 * G + 2 * G * i as i64
    }
}

pub struct RootOut {
    pub sheets: Vec<SheetInstance>,
    pub items: Items,
    pub paper: Paper,
}

fn role(m: &FunctionalModule) -> usize {
    match m.kind {
        ModuleKind::Connector => 0,
        ModuleKind::Ic | ModuleKind::Declared => 1,
        ModuleKind::Channels => 2,
        ModuleKind::Misc => 3,
    }
}

/// Two segments (axis-aligned) on one line that share more than a point.
fn collinear_overlap(a: (Point, Point), b: (Point, Point)) -> bool {
    let horizontal = |s: (Point, Point)| s.0.y == s.1.y;
    if horizontal(a) && horizontal(b) && a.0.y == b.0.y {
        return a.0.x.min(a.1.x).max(b.0.x.min(b.1.x)) < a.0.x.max(a.1.x).min(b.0.x.max(b.1.x));
    }
    let vertical = |s: (Point, Point)| s.0.x == s.1.x;
    if vertical(a) && vertical(b) && a.0.x == b.0.x {
        return a.0.y.min(a.1.y).max(b.0.y.min(b.1.y)) < a.0.y.max(a.1.y).min(b.0.y.max(b.1.y));
    }
    false
}

fn point_on_segment(p: Point, s: (Point, Point)) -> bool {
    p.x >= s.0.x.min(s.1.x) && p.x <= s.0.x.max(s.1.x) && p.y >= s.0.y.min(s.1.y) && p.y <= s.0.y.max(s.1.y) && (s.0.x == s.1.x || s.0.y == s.1.y)
}

/// Lay the root out.
pub fn layout_root(modules: &[FunctionalModule], files: &[String], crossing: &[Vec<String>]) -> RootOut {
    let n = modules.len();
    // which modules touch each crossing net
    let mut mods_of: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    for (m, nets) in crossing.iter().enumerate() {
        for net in nets {
            mods_of.entry(net.clone()).or_default().push(m);
        }
    }

    // ---- columns: by role, compacted ----
    let roles: BTreeSet<usize> = modules.iter().map(role).collect();
    let col: Vec<usize> = modules.iter().map(|m| roles.iter().position(|r| *r == role(m)).unwrap()).collect();
    let ncols = roles.len();

    // ---- which nets are wired: exactly two modules, in neighbouring columns ----
    let mut wired: BTreeSet<String> = BTreeSet::new();
    for (net, ms) in &mods_of {
        if ms.len() == 2 && col[ms[0]].abs_diff(col[ms[1]]) == 1 {
            wired.insert(net.clone());
        }
    }

    // ---- the pins of each sheet ----
    let mut boxes: Vec<SheetBox> = Vec::new();
    for m in 0..n {
        let mut pins: Vec<PinSpec> = Vec::new();
        for net in &crossing[m] {
            let partners: Vec<usize> = mods_of[net].iter().copied().filter(|&p| p != m).collect();
            let side = if wired.contains(net) {
                if col[partners[0]] > col[m] {
                    Side::Right
                } else {
                    Side::Left
                }
            } else {
                // label-joined: toward the bulk of its partners
                let to_right = partners.iter().filter(|&&p| col[p] >= col[m]).count();
                if to_right * 2 >= partners.len() {
                    Side::Right
                } else {
                    Side::Left
                }
            };
            let first = partners.iter().copied().min().unwrap_or(0);
            pins.push(PinSpec { net: net.clone(), side, key: (col[first], first, net.clone()) });
        }
        let order = |a: &PinSpec, b: &PinSpec| (a.key.0, a.key.1).cmp(&(b.key.0, b.key.1)).then_with(|| natural_cmp(&a.net, &b.net));
        let mut left: Vec<PinSpec> = pins.iter().filter(|p| p.side == Side::Left).cloned().collect();
        let mut right: Vec<PinSpec> = pins.iter().filter(|p| p.side == Side::Right).cloned().collect();
        left.sort_by(order);
        right.sort_by(order);
        let lmax = left.iter().map(|p| text_w(SHEET_PIN_FONT, &p.net)).max().unwrap_or(0);
        let rmax = right.iter().map(|p| text_w(SHEET_PIN_FONT, &p.net)).max().unwrap_or(0);
        let rows = left.len().max(right.len());
        let h = if rows == 0 { 8 * G } else { snap_up(SheetBox::pin_y(rows - 1) + 3 * G).max(8 * G) };
        let name_w = text_w(SHEET_NAME_FONT, &modules[m].name).max(text_w(SHEET_FILE_FONT, &files[m]));
        // each side's names start past the flag (1.77 mm) and the two columns of names keep a gap between them
        let w = snap_up((lmax + rmax + 2 * (1_770) + 4 * G).max(name_w + 2 * G)).max(14 * G);
        boxes.push(SheetBox { w, h, left, right });
    }

    // ---- horizontal: columns left to right, the gap wide enough for the labels of both sides ----
    let label_len = |net: &str| 2 * G + label_rect(Point { x: 0, y: 0 }, (1, 0), net, false).w();
    let mut col_w = vec![0i64; ncols];
    let mut right_ext = vec![0i64; ncols];
    let mut left_ext = vec![0i64; ncols];
    for m in 0..n {
        col_w[col[m]] = col_w[col[m]].max(boxes[m].w);
        for p in &boxes[m].right {
            right_ext[col[m]] = right_ext[col[m]].max(label_len(&p.net));
        }
        for p in &boxes[m].left {
            left_ext[col[m]] = left_ext[col[m]].max(label_len(&p.net));
        }
    }
    let mut col_x = vec![0i64; ncols];
    let mut x = 0;
    for c in 0..ncols {
        x += if c == 0 { left_ext[0] } else { 0 };
        col_x[c] = snap_up(x);
        let gap = if c + 1 < ncols { (right_ext[c] + left_ext[c + 1] + 4 * G).max(14 * G) } else { 0 };
        x = col_x[c] + col_w[c] + snap_up(gap);
    }

    // ---- vertical: stack each column, pulling a sheet level with the partner its first wired net goes to ----
    let mut y_of = vec![0i64; n];
    let mut pin_abs_y: BTreeMap<(usize, String), i64> = BTreeMap::new();
    for c in 0..ncols {
        let mut members: Vec<usize> = (0..n).filter(|&m| col[m] == c).collect();
        members.sort();
        let mut next_free = 0i64;
        for m in members {
            let pin_index = |side: &Vec<PinSpec>, net: &str| side.iter().position(|p| p.net == net);
            // the first wired pin whose partner is already placed
            let mut want: Option<i64> = None;
            for (side, list) in [(Side::Left, &boxes[m].left), (Side::Right, &boxes[m].right)] {
                let _ = side;
                for p in list.iter() {
                    if !wired.contains(&p.net) {
                        continue;
                    }
                    let partner = mods_of[&p.net].iter().copied().find(|&q| q != m).unwrap();
                    if let Some(&py) = pin_abs_y.get(&(partner, p.net.clone())) {
                        let i = pin_index(list, &p.net).unwrap();
                        want = Some(py - SheetBox::pin_y(i));
                        break;
                    }
                }
                if want.is_some() {
                    break;
                }
            }
            let y = snap_up(want.unwrap_or(next_free).max(next_free));
            y_of[m] = y;
            next_free = y + boxes[m].h + 7 * G;
            for list in [&boxes[m].left, &boxes[m].right] {
                for (i, p) in list.iter().enumerate() {
                    pin_abs_y.insert((m, p.net.clone()), y + SheetBox::pin_y(i));
                }
            }
        }
    }

    // ---- items ----
    let mut items = Items::default();
    let mut sheets: Vec<SheetInstance> = Vec::new();
    let mut pin_at: BTreeMap<(usize, String), Point> = BTreeMap::new();
    for m in 0..n {
        let at = Point { x: col_x[col[m]], y: y_of[m] };
        let b = &boxes[m];
        let mut pins: Vec<SheetPin> = Vec::new();
        for (i, p) in b.left.iter().enumerate() {
            let pt = Point { x: at.x, y: at.y + SheetBox::pin_y(i) };
            pin_at.insert((m, p.net.clone()), pt);
            pins.push(SheetPin { id: String::new(), name: p.net.clone(), shape: LabelShape::Bidirectional, at: pt });
            items.rects.push(sheet_pin_rect(pt, true, &p.net));
        }
        for (i, p) in b.right.iter().enumerate() {
            let pt = Point { x: at.x + b.w, y: at.y + SheetBox::pin_y(i) };
            pin_at.insert((m, p.net.clone()), pt);
            pins.push(SheetPin { id: String::new(), name: p.net.clone(), shape: LabelShape::Bidirectional, at: pt });
            items.rects.push(sheet_pin_rect(pt, false, &p.net));
        }
        sheets.push(SheetInstance { id: String::new(), name: modules[m].name.clone(), file: files[m].clone(), at, size: (b.w, b.h), pins, page: String::new() });
        // the symbol, its name above and its file below
        let name_w = text_w(SHEET_NAME_FONT, &modules[m].name);
        let file_base = at.y + b.h + 400 + SHEET_FILE_FONT;
        let r = Rect::new(at.x, at.y - 400 - cap(SHEET_NAME_FONT), at.x + b.w.max(name_w), at.y + b.h).union(Rect::new(at.x, file_base - cap(SHEET_FILE_FONT), at.x + text_w(SHEET_FILE_FONT, &files[m]), file_base));
        items.rects.push(r);
        items.keepouts.push((modules[m].name.clone(), r));
    }

    // ---- wires between facing pins, then labels for what is left ----
    let mut segments: Vec<(String, (Point, Point))> = Vec::new();
    let mut routes: BTreeMap<String, Vec<Point>> = BTreeMap::new();
    let mut lane_of_gap: BTreeMap<usize, i64> = BTreeMap::new();
    let wired_nets: Vec<String> = wired.iter().cloned().collect();
    for net in &wired_nets {
        let ms = &mods_of[net];
        let (a, b) = if col[ms[0]] < col[ms[1]] { (ms[0], ms[1]) } else { (ms[1], ms[0]) };
        let (pa, pb) = (pin_at[&(a, net.clone())], pin_at[&(b, net.clone())]);
        let pts = if pa.y == pb.y {
            vec![pa, pb]
        } else {
            let gap_start = col_x[col[a]] + col_w[col[a]];
            let k = lane_of_gap.entry(col[a]).or_insert(0);
            let lane = gap_start + 4 * G + *k;
            *k += 2 * G;
            vec![pa, Point { x: lane, y: pa.y }, Point { x: lane, y: pb.y }, pb]
        };
        routes.insert(net.clone(), pts);
    }
    // A wire survives only if no other net's wire runs along it and it touches no other net's pin.
    let all_pins: Vec<(String, Point)> = pin_at.iter().map(|((_, net), p)| (net.clone(), *p)).collect();
    let mut demoted: BTreeSet<String> = BTreeSet::new();
    for (net, pts) in &routes {
        let mine: Vec<(Point, Point)> = pts.windows(2).map(|w| (w[0], w[1])).collect();
        let mut bad = false;
        for (other, opts) in &routes {
            if other == net {
                continue;
            }
            for s in &mine {
                for t in opts.windows(2).map(|w| (w[0], w[1])) {
                    if collinear_overlap(*s, t) {
                        bad = true;
                    }
                }
            }
        }
        for (pnet, p) in &all_pins {
            if pnet != net && mine.iter().any(|s| point_on_segment(*p, *s)) {
                bad = true;
            }
        }
        if bad {
            demoted.insert(net.clone());
        }
    }
    for (net, pts) in &routes {
        if demoted.contains(net) {
            continue;
        }
        items.wires.push(Wire { id: String::new(), net: net.clone(), pins: Vec::new(), pts: pts.clone(), bus: false });
        for w in pts.windows(2) {
            segments.push((net.clone(), (w[0], w[1])));
        }
    }
    // Everything not wired: a short wire out of the pin and a local label of the net's name.
    for m in 0..n {
        for (list, side) in [(&boxes[m].left, Side::Left), (&boxes[m].right, Side::Right)] {
            for p in list {
                if wired.contains(&p.net) && !demoted.contains(&p.net) {
                    continue;
                }
                let pt = pin_at[&(m, p.net.clone())];
                let dir = if side == Side::Left { (-1, 0) } else { (1, 0) };
                let end = Point { x: pt.x + dir.0 * 2 * G, y: pt.y };
                items.wires.push(Wire { id: String::new(), net: p.net.clone(), pins: Vec::new(), pts: vec![pt, end], bus: false });
                items.labels.push(NetLabel { id: String::new(), net: p.net.clone(), at: end, kind: LabelKind::Local });
                items.rects.push(label_rect(end, dir, &p.net, false));
            }
        }
    }

    // ---- the page ----
    let e = items.extent().unwrap_or(Rect::new(0, 0, 0, 0));
    let mut ext = e;
    for s in &sheets {
        ext = ext.union(Rect::new(s.at.x, s.at.y, s.at.x + s.size.0, s.at.y + s.size.1));
    }
    let (w, h) = (snap_up(ext.x1) - snap_down(ext.x0), snap_up(ext.y1) - snap_down(ext.y0));
    let paper = smallest_paper(w, h);
    let u = paper.usable();
    let dx = u.x0 + snap_down((u.w() - w) / 2) - snap_down(ext.x0);
    let dy = u.y0 + snap_down((u.h() - h) / 2) - snap_down(ext.y0);
    items.translate(dx, dy);
    for s in &mut sheets {
        s.at.x += dx;
        s.at.y += dy;
        for p in &mut s.pins {
            p.at.x += dx;
            p.at.y += dy;
        }
    }
    let _ = segments;
    RootOut { sheets, items, paper }
}
