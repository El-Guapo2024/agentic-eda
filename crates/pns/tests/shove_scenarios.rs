//! Shove scenarios on hand-built boards: what the router does when the track
//! it is laying runs into pads, vias and other tracks (gap #7, D5/D6/D10 of
//! `docs/parity/CODE-COMPARE-router.md`).
//!
//! Every test checks the same invariant first -- the shove introduced no
//! clearance violation the board did not already have (`assert_no_new_violations`,
//! measured with the router's own collision test) -- and then the property that
//! scenario is about.

use eda_drc::kimath::Shape;
use eda_model::ir::{Point, Um};
use eda_model::BoardRules;
use eda_pns::item::{net_of, Item, ItemId, Segment, Solid, Via};
use eda_pns::layer::LayerRange;
use eda_pns::line::Line;
use eda_pns::node::Node;
use eda_pns::settings::RoutingSettings;
use eda_pns::shove::{shove_line, ShoveOutcome};
use std::collections::HashSet;

fn rules() -> BoardRules {
    serde_yaml::from_str("track_width: 200\nclearance: 200\nvia_drill: 300\nvia_diameter: 600\n").unwrap()
}

fn p(x: Um, y: Um) -> Point {
    Point { x, y }
}

fn seg(net: &str, track: &str, a: Point, b: Point, width: Um) -> Item {
    Item::Segment(Segment { net: net_of(net), layer: 0, a, b, width, source_track: Some((track.into(), 0)), locked: false })
}

/// A chain of segments of one IR track.
fn track(node: &mut Node, net: &str, id: &str, pts: &[Point], width: Um) {
    for (i, w) in pts.windows(2).enumerate() {
        node.add(Item::Segment(Segment { net: net_of(net), layer: 0, a: w[0], b: w[1], width, source_track: Some((id.into(), i)), locked: false }));
    }
}

fn pad(net: &str, name: &str, c: Point, r: Um) -> Item {
    Item::Solid(Solid { net: net_of(net), layers: LayerRange::new(0, 1), pos: c, shape: Shape::Circle { c, r }, source: name.into() })
}

fn via(net: &str, id: &str, c: Point) -> Item {
    Item::Via(Via { net: net_of(net), layers: LayerRange::new(0, 1), pos: c, diameter: 600, drill: 300, source_via: Some(id.into()), locked: false })
}

/// Every pair of items the router's own collision test calls a violation.
fn violations(node: &Node, rules: &BoardRules) -> HashSet<(ItemId, ItemId)> {
    let mut out = HashSet::new();
    for (id, item) in node.iter() {
        for o in node.all_colliding(&item.shape(item.layers().start()), item.net(), item.layers(), rules, &[id]) {
            out.insert((id.min(o.id), id.max(o.id)));
        }
    }
    out
}

/// The world after a shove with the head put back in.
fn with_head(out: &ShoveOutcome, net: &str, width: Um) -> Node {
    let mut w = out.world.branch();
    w.add_line(&Line::from_points(net_of(net), 0, width, out.head.clone()), None, false);
    w
}

fn assert_no_new_violations(before: &Node, out: &ShoveOutcome, head_net: &str, head_width: Um, rules: &BoardRules) {
    let mut start = before.branch();
    let head_pts = out.head.clone();
    // the head as it was asked for is not part of "before": judge only the shoved world
    let _ = &mut start;
    let after = with_head(out, head_net, head_width);
    let old = violations(before, rules);
    // ids are shared by `before` and `out.world` for everything the shove left alone;
    // anything it replaced has new ids, so compare counts of violations by kind instead of ids
    let new = violations(&after, rules);
    let new_ones: Vec<_> = new.iter().filter(|pair| !old.contains(pair)).collect();
    assert!(new_ones.is_empty(), "the shove left {} new clearance violations {:?}, head {:?}", new_ones.len(), new_ones, head_pts);
}

#[test]
fn nothing_in_the_way_changes_nothing() {
    let mut node = Node::new();
    track(&mut node, "GND", "t1", &[p(0, 3000), p(6000, 3000)], 200);
    let raw = vec![p(0, 0), p(6000, 0)];
    let out = shove_line(&node, &raw, &net_of("SIG"), 0, 200, &rules(), &RoutingSettings::default()).expect("free path");
    assert_eq!(out.head, raw);
    assert!(out.displaced_lines.is_empty() && out.displaced_vias.is_empty());
}

#[test]
fn a_parallel_track_is_pushed_aside_and_keeps_its_width_and_ends() {
    let mut node = Node::new();
    // a 300-wide track alongside the head: needs 100 + 200 + 150 = 450 of room
    track(&mut node, "GND", "t1", &[p(-2000, 300), p(8000, 300)], 300);
    let raw = vec![p(0, 0), p(6000, 0)];
    let out = shove_line(&node, &raw, &net_of("SIG"), 0, 200, &rules(), &RoutingSettings::default()).expect("pushed");
    assert_no_new_violations(&node, &out, "SIG", 200, &rules());
    assert_eq!(out.displaced_lines.len(), 1);
    let d = &out.displaced_lines[0];
    assert_eq!(d.source_track.as_deref(), Some("t1"));
    assert_eq!(d.line.width, 300, "the shoved track keeps its width");
    assert_eq!(d.line.first(), Some(p(-2000, 300)));
    assert_eq!(d.line.last(), Some(p(8000, 300)));
    assert!(d.line.pts.iter().any(|q| q.y >= 450), "it moved away from the head: {:?}", d.line.pts);
}

/// D10: a track that changes width keeps both widths. The two widths are two
/// lines, each pinned at the joint they share.
#[test]
fn a_track_with_two_widths_keeps_both() {
    let mut node = Node::new();
    track(&mut node, "GND", "narrow", &[p(3500, -2000), p(3500, -1000)], 200);
    track(&mut node, "GND", "wide", &[p(3500, -1000), p(3500, 2000)], 500);
    let raw = vec![p(0, 0), p(5000, 0)];
    let out = shove_line(&node, &raw, &net_of("SIG"), 0, 200, &rules(), &RoutingSettings::default()).expect("pushed");
    assert_no_new_violations(&node, &out, "SIG", 200, &rules());
    for d in &out.displaced_lines {
        assert!(d.line.width == 200 || d.line.width == 500 || d.line.pts.len() < 2, "{:?}", d);
    }
    let wide: Vec<_> = out.displaced_lines.iter().filter(|d| d.source_track.as_deref() == Some("wide") && d.line.pts.len() >= 2).collect();
    assert_eq!(wide.len(), 1);
    assert_eq!(wide[0].line.width, 500, "the 500-wide part stays 500 wide");
    assert!(out.displaced_lines.iter().all(|d| d.source_track.as_deref() != Some("narrow") || d.line.pts.len() < 2 || d.line.width == 200), "the 200-wide part is never widened");
}

#[test]
fn a_line_spanning_two_ir_tracks_names_both() {
    let mut node = Node::new();
    // an imported board: one IR track per segment
    node.add(seg("GND", "ta", p(2500, -2000), p(2500, 0), 200));
    node.add(seg("GND", "tb", p(2500, 0), p(2500, 2000), 200));
    let raw = vec![p(0, 0), p(5000, 0)];
    let out = shove_line(&node, &raw, &net_of("SIG"), 0, 200, &rules(), &RoutingSettings::default()).expect("pushed");
    let names: Vec<_> = out.displaced_lines.iter().map(|d| d.source_track.clone().unwrap()).collect();
    assert!(names.contains(&"ta".to_string()) && names.contains(&"tb".to_string()), "both tracks are replaced: {names:?}");
    let with_geometry = out.displaced_lines.iter().filter(|d| d.line.pts.len() >= 2).count();
    assert_eq!(with_geometry, 1, "one new polyline for the one line");
}

/// D5: the pusher itself runs into a pad (the placer's solids pre-pass normally walks it first; the
/// shove still has to cope if one is left): it is walked around the pad, not given up on.
#[test]
fn a_head_that_runs_into_a_pad_is_walked_around_it() {
    let mut node = Node::new();
    node.add(pad("VCC", "U1.1", p(3000, 0), 400));
    let raw = vec![p(0, 0), p(6000, 0)];
    let out = shove_line(&node, &raw, &net_of("SIG"), 0, 200, &rules(), &RoutingSettings::default()).expect("walks around the pad");
    assert_ne!(out.head, raw);
    assert_eq!((out.head.first(), out.head.last()), (Some(&raw[0]), Some(&raw[1])));
    assert_no_new_violations(&node, &out, "SIG", 200, &rules());
}

/// D5: a pad stops the track a shove pushes. The track is walked along the pad's clearance
/// (`onCollidingSolid`), ranked above the head, and the head gives way in turn -- KiCad's rule.
#[test]
fn a_track_shoved_past_a_pad_row_is_walked_around_the_pads() {
    let mut node = Node::new();
    track(&mut node, "GND", "t1", &[p(0, 500), p(6000, 500)], 200);
    for (i, x) in [1000, 1800, 2600, 3400, 4200, 5000].iter().enumerate() {
        node.add(pad("VCC", &format!("U1.{i}"), p(*x, 1300), 300));
    }
    // 150 above the track's centre line: pushing the track up 250 would put it 147 from the pads
    let raw = vec![p(0, 350), p(6000, 350)];
    let out = shove_line(&node, &raw, &net_of("SIG"), 0, 200, &rules(), &RoutingSettings::default()).expect("pushed past the pad row");
    assert_no_new_violations(&node, &out, "SIG", 200, &rules());
    assert_eq!(out.displaced_lines.len(), 1);
    let t = &out.displaced_lines[0].line;
    assert_eq!(t.width, 200);
    assert!(t.pts.iter().all(|q| q.y > 600 && q.y < 800), "the track runs between the head and the pads: {:?}", t.pts);
    assert_ne!(out.head, raw, "the head had to give way for it");
    assert!(out.head.iter().all(|q| q.y <= 350), "the head moved away from the pads, never toward them: {:?}", out.head);
}

/// D6: a via is moved by the minimum translation vector -- exactly far enough to clear.
#[test]
fn a_via_is_pushed_just_far_enough() {
    let (rules, settings) = (rules(), RoutingSettings::default());
    for (name, c, expected) in [("off the line", p(2500, 100), p(2500, 600)), ("on the line", p(2500, 0), p(2500, 601)), ("on the other side", p(2500, -400), p(2500, -600))] {
        let mut node = Node::new();
        node.add(via("GND", "v1", c));
        let out = shove_line(&node, &[p(0, 0), p(5000, 0)], &net_of("SIG"), 0, 200, &rules, &settings).expect(name);
        assert_eq!(out.displaced_vias.len(), 1, "{name}");
        let at = out.displaced_vias[0].pos;
        // edge to edge: clearance 200 + head half 100 + via radius 300 from the head's centre line
        assert!((at.y.abs() - 600).abs() <= 1 && at.x == 2500, "{name}: via went to {at:?}, expected about {expected:?}");
        assert!(at.y.abs() >= 600, "{name}: never closer than the clearance, got {at:?}");
    }
}

/// D6: the tracks attached to a pushed via follow it and stay on 45-degree headings.
#[test]
fn a_pushed_vias_tracks_follow_it_at_45_degrees() {
    let mut node = Node::new();
    node.add(via("GND", "v1", p(2500, 100)));
    track(&mut node, "GND", "f1", &[p(2500, 100), p(2500, 3000)], 200);
    track(&mut node, "GND", "f2", &[p(2500, 100), p(4500, 2100)], 200);
    let out = shove_line(&node, &[p(0, 0), p(5000, 0)], &net_of("SIG"), 0, 200, &rules(), &RoutingSettings::default()).expect("pushed");
    assert_eq!(out.displaced_vias[0].pos, p(2500, 600));
    assert_eq!(out.displaced_lines.len(), 2, "both attached tracks are re-shaped");
    for d in &out.displaced_lines {
        assert!(d.line.pts.contains(&p(2500, 600)), "{:?} ends on the via's new place", d.line.pts);
        for (a, b) in d.line.segs() {
            let (dx, dy) = ((b.x - a.x).abs(), (b.y - a.y).abs());
            assert!(dx == 0 || dy == 0 || dx == dy, "leg {a:?}-{b:?} of {:?} is not on a 45-degree heading", d.source_track);
        }
    }
    assert_no_new_violations(&node, &out, "SIG", 200, &rules());
}

/// D6: a via pushed between two tracks pushes one of them in turn (a via with no track of its own is
/// a lone-via line on the stack: `shoveLineFromLoneVia`).
#[test]
fn a_via_pushed_between_two_tracks_pushes_the_one_it_runs_into() {
    let mut node = Node::new();
    track(&mut node, "NETA", "ta", &[p(300, -900), p(5000, -900)], 200);
    track(&mut node, "NETB", "tb", &[p(300, 900), p(5000, 900)], 200);
    node.add(via("GND", "v1", p(1000, 300)));
    // the head ends 500 from the via's centre, up and to the left of it: the via goes up and to the right
    let raw = vec![p(-2000, -300), p(600, 0)];
    let out = shove_line(&node, &raw, &net_of("SIG"), 0, 200, &rules(), &RoutingSettings::default()).expect("pushed");
    assert_no_new_violations(&node, &out, "SIG", 200, &rules());
    let v = out.displaced_vias.first().expect("the via moved").pos;
    assert!(v.x > 1000 && v.y > 300, "up and to the right: {v:?}");
    let tb = out.displaced_lines.iter().find(|d| d.source_track.as_deref() == Some("tb")).expect("the upper track gave way to the via");
    assert_eq!(tb.line.width, 200);
    assert!(tb.line.pts.iter().any(|q| q.y > 900), "pushed away from the via: {:?}", tb.line.pts);
    assert!(out.displaced_lines.iter().all(|d| d.source_track.as_deref() != Some("ta")), "the lower track is out of reach");
    // the via stayed between the two tracks
    let (ta, tb) = (-900, tb.line.pts.iter().map(|q| q.y).fold(900, Ord::max));
    assert!(v.y > ta && v.y < tb);
}

/// `ShoveIterationLimit` (and its off-by-one: the loop ends when `m_iter >= limit`): a limit of 1 fails a
/// shove that needs the second iteration to see the stack empty.
#[test]
fn the_iteration_limit_makes_a_shove_fail() {
    let mut node = Node::new();
    track(&mut node, "GND", "t1", &[p(2500, -2000), p(2500, 2000)], 200);
    let raw = vec![p(0, 0), p(5000, 0)];
    let tight = RoutingSettings { shove_iteration_limit: 1, ..RoutingSettings::default() };
    assert!(shove_line(&node, &raw, &net_of("SIG"), 0, 200, &rules(), &tight).is_none());
    assert!(shove_line(&node, &raw, &net_of("SIG"), 0, 200, &rules(), &RoutingSettings::default()).is_some());
}

/// `ShoveVias()` off: a via is no longer pushed (`SH_TRY_WALK`): the head walks around it instead.
#[test]
fn with_shove_vias_off_the_head_walks_around_the_via() {
    let mut node = Node::new();
    node.add(via("GND", "v1", p(2500, 0)));
    let raw = vec![p(0, 0), p(5000, 0)];
    let settings = RoutingSettings { shove_vias: false, ..RoutingSettings::default() };
    let out = shove_line(&node, &raw, &net_of("SIG"), 0, 200, &rules(), &settings).expect("walks around");
    assert!(out.displaced_vias.is_empty());
    assert_ne!(out.head, raw);
    assert_no_new_violations(&node, &out, "SIG", 200, &rules());
}
