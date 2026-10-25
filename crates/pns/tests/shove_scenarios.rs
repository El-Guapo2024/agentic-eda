//! Shove scenarios on hand-built boards: what the router does when the track
//! it is laying runs into pads, vias and other tracks (gap #7, D5/D6/D10 of
//! `docs/parity/CODE-COMPARE-router.md`).
//!
//! Every test checks the same invariant first -- the shove introduced no
//! clearance violation the board did not already have (`assert_no_new_violations`,
//! measured with the router's own collision test) -- and then the property that
//! scenario is about.

use eda_drc::kimath::Shape;
use eda_model::ir::{Point, Nm};
use eda_model::BoardRules;
use eda_pns::item::{net_of, Item, ItemId, Segment, Solid, Via};
use eda_pns::layer::LayerRange;
use eda_pns::line::Line;
use eda_pns::node::Node;
use eda_pns::settings::RoutingSettings;
use eda_pns::shove::{shove_line, ShoveOutcome};
use std::collections::HashSet;

fn rules() -> BoardRules {
    serde_yaml::from_str("track_width: 200000\nclearance: 200000\nvia_drill: 300000\nvia_diameter: 600000\n").unwrap()
}

fn p(x: Nm, y: Nm) -> Point {
    Point { x, y }
}

fn seg(net: &str, track: &str, a: Point, b: Point, width: Nm) -> Item {
    Item::Segment(Segment { net: net_of(net), layer: 0, a, b, width, source_track: Some((track.into(), 0)), locked: false })
}

/// A chain of segments of one IR track.
fn track(node: &mut Node, net: &str, id: &str, pts: &[Point], width: Nm) {
    for (i, w) in pts.windows(2).enumerate() {
        node.add(Item::Segment(Segment { net: net_of(net), layer: 0, a: w[0], b: w[1], width, source_track: Some((id.into(), i)), locked: false }));
    }
}

fn pad(net: &str, name: &str, c: Point, r: Nm) -> Item {
    Item::Solid(Solid { net: net_of(net), layers: LayerRange::new(0, 1), pos: c, shape: Shape::Circle { c, r }, source: name.into(), edge: false })
}

fn via(net: &str, id: &str, c: Point) -> Item {
    Item::Via(Via { net: net_of(net), layers: LayerRange::new(0, 1), pos: c, diameter: Nm::from_raw_shim(600_000), drill: Nm::from_raw_shim(300_000), source_via: Some(id.into()), locked: false, is_virtual: false })
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
fn with_head(out: &ShoveOutcome, net: &str, width: Nm) -> Node {
    let mut w = out.world.branch();
    w.add_line(&Line::from_points(net_of(net), 0, width, out.head.clone()), None, false);
    w
}

fn assert_no_new_violations(before: &Node, out: &ShoveOutcome, head_net: &str, head_width: Nm, rules: &BoardRules) {
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
    track(&mut node, "GND", "t1", &[p(Nm::ZERO, Nm::from_raw_shim(3_000_000)), p(Nm::from_raw_shim(6_000_000), Nm::from_raw_shim(3_000_000))], Nm::from_raw_shim(200_000));
    let raw = vec![p(Nm::ZERO, Nm::ZERO), p(Nm::from_raw_shim(6_000_000), Nm::ZERO)];
    let out = shove_line(&node, &raw, &net_of("SIG"), 0, Nm::from_raw_shim(200_000), &rules(), &RoutingSettings::default()).expect("free path");
    assert_eq!(out.head, raw);
    assert!(out.displaced_lines.is_empty() && out.displaced_vias.is_empty());
}

#[test]
fn a_parallel_track_is_pushed_aside_and_keeps_its_width_and_ends() {
    let mut node = Node::new();
    // a 300-wide track alongside the head: needs 100 + 200 + 150 = 450 of room
    track(&mut node, "GND", "t1", &[p(Nm::from_raw_shim(-2_000_000), Nm::from_raw_shim(300_000)), p(Nm::from_raw_shim(8_000_000), Nm::from_raw_shim(300_000))], Nm::from_raw_shim(300_000));
    let raw = vec![p(Nm::ZERO, Nm::ZERO), p(Nm::from_raw_shim(6_000_000), Nm::ZERO)];
    let out = shove_line(&node, &raw, &net_of("SIG"), 0, Nm::from_raw_shim(200_000), &rules(), &RoutingSettings::default()).expect("pushed");
    assert_no_new_violations(&node, &out, "SIG", Nm::from_raw_shim(200_000), &rules());
    assert_eq!(out.displaced_lines.len(), 1);
    let d = &out.displaced_lines[0];
    assert_eq!(d.source_track.as_deref(), Some("t1"));
    assert_eq!(d.line.width, Nm::from_raw_shim(300_000), "the shoved track keeps its width");
    assert_eq!(d.line.first(), Some(p(Nm::from_raw_shim(-2_000_000), Nm::from_raw_shim(300_000))));
    assert_eq!(d.line.last(), Some(p(Nm::from_raw_shim(8_000_000), Nm::from_raw_shim(300_000))));
    assert!(d.line.pts.iter().any(|q| q.y >= Nm::from_raw_shim(450_000)), "it moved away from the head: {:?}", d.line.pts);
}

/// D10: a track that changes width keeps both widths. The two widths are two
/// lines, each pinned at the joint they share.
#[test]
fn a_track_with_two_widths_keeps_both() {
    let mut node = Node::new();
    track(&mut node, "GND", "narrow", &[p(Nm::from_raw_shim(3_500_000), Nm::from_raw_shim(-2_000_000)), p(Nm::from_raw_shim(3_500_000), Nm::from_raw_shim(-1_000_000))], Nm::from_raw_shim(200_000));
    track(&mut node, "GND", "wide", &[p(Nm::from_raw_shim(3_500_000), Nm::from_raw_shim(-1_000_000)), p(Nm::from_raw_shim(3_500_000), Nm::from_raw_shim(2_000_000))], Nm::from_raw_shim(500_000));
    let raw = vec![p(Nm::ZERO, Nm::ZERO), p(Nm::from_raw_shim(5_000_000), Nm::ZERO)];
    let out = shove_line(&node, &raw, &net_of("SIG"), 0, Nm::from_raw_shim(200_000), &rules(), &RoutingSettings::default()).expect("pushed");
    assert_no_new_violations(&node, &out, "SIG", Nm::from_raw_shim(200_000), &rules());
    for d in &out.displaced_lines {
        assert!(d.line.width == Nm::from_raw_shim(200_000) || d.line.width == Nm::from_raw_shim(500_000) || d.line.pts.len() < 2, "{:?}", d);
    }
    let wide: Vec<_> = out.displaced_lines.iter().filter(|d| d.source_track.as_deref() == Some("wide") && d.line.pts.len() >= 2).collect();
    assert_eq!(wide.len(), 1);
    assert_eq!(wide[0].line.width, Nm::from_raw_shim(500_000), "the 500-wide part stays 500 wide");
    assert!(out.displaced_lines.iter().all(|d| d.source_track.as_deref() != Some("narrow") || d.line.pts.len() < 2 || d.line.width == Nm::from_raw_shim(200_000)), "the 200-wide part is never widened");
}

#[test]
fn a_line_spanning_two_ir_tracks_names_both() {
    let mut node = Node::new();
    // an imported board: one IR track per segment
    node.add(seg("GND", "ta", p(Nm::from_raw_shim(2_500_000), Nm::from_raw_shim(-2_000_000)), p(Nm::from_raw_shim(2_500_000), Nm::ZERO), Nm::from_raw_shim(200_000)));
    node.add(seg("GND", "tb", p(Nm::from_raw_shim(2_500_000), Nm::ZERO), p(Nm::from_raw_shim(2_500_000), Nm::from_raw_shim(2_000_000)), Nm::from_raw_shim(200_000)));
    let raw = vec![p(Nm::ZERO, Nm::ZERO), p(Nm::from_raw_shim(5_000_000), Nm::ZERO)];
    let out = shove_line(&node, &raw, &net_of("SIG"), 0, Nm::from_raw_shim(200_000), &rules(), &RoutingSettings::default()).expect("pushed");
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
    node.add(pad("VCC", "U1.1", p(Nm::from_raw_shim(3_000_000), Nm::ZERO), Nm::from_raw_shim(400_000)));
    let raw = vec![p(Nm::ZERO, Nm::ZERO), p(Nm::from_raw_shim(6_000_000), Nm::ZERO)];
    let out = shove_line(&node, &raw, &net_of("SIG"), 0, Nm::from_raw_shim(200_000), &rules(), &RoutingSettings::default()).expect("walks around the pad");
    assert_ne!(out.head, raw);
    assert_eq!((out.head.first(), out.head.last()), (Some(&raw[0]), Some(&raw[1])));
    assert_no_new_violations(&node, &out, "SIG", Nm::from_raw_shim(200_000), &rules());
}

/// D5: a pad stops the track a shove pushes. The track is walked along the pad's clearance
/// (`onCollidingSolid`), ranked above the head, and the head gives way in turn -- KiCad's rule.
#[test]
fn a_track_shoved_past_a_pad_row_is_walked_around_the_pads() {
    let mut node = Node::new();
    track(&mut node, "GND", "t1", &[p(Nm::ZERO, Nm::from_raw_shim(500_000)), p(Nm::from_raw_shim(6_000_000), Nm::from_raw_shim(500_000))], Nm::from_raw_shim(200_000));
    for (i, x) in [1_000_000, 1_800_000, 2_600_000, 3_400_000, 4_200_000, 5_000_000].iter().enumerate() {
        node.add(pad("VCC", &format!("U1.{i}"), p(Nm::from_raw_shim(*x), Nm::from_raw_shim(1_300_000)), Nm::from_raw_shim(300_000)));
    }
    // 150 above the track's centre line: pushing the track up 250 would put it 147 from the pads
    let raw = vec![p(Nm::ZERO, Nm::from_raw_shim(350_000)), p(Nm::from_raw_shim(6_000_000), Nm::from_raw_shim(350_000))];
    let out = shove_line(&node, &raw, &net_of("SIG"), 0, Nm::from_raw_shim(200_000), &rules(), &RoutingSettings::default()).expect("pushed past the pad row");
    assert_no_new_violations(&node, &out, "SIG", Nm::from_raw_shim(200_000), &rules());
    assert_eq!(out.displaced_lines.len(), 1);
    let t = &out.displaced_lines[0].line;
    assert_eq!(t.width, Nm::from_raw_shim(200_000));
    assert!(t.pts.iter().all(|q| q.y > Nm::from_raw_shim(600_000) && q.y < Nm::from_raw_shim(800_000)), "the track runs between the head and the pads: {:?}", t.pts);
    assert_ne!(out.head, raw, "the head had to give way for it");
    assert!(out.head.iter().all(|q| q.y <= Nm::from_raw_shim(350_000)), "the head moved away from the pads, never toward them: {:?}", out.head);
}

/// D6: a via is moved by the minimum translation vector -- exactly far enough to clear.
#[test]
fn a_via_is_pushed_just_far_enough() {
    let (rules, settings) = (rules(), RoutingSettings::default());
    for (name, c, expected) in [("off the line", p(Nm::from_raw_shim(2_500_000), Nm::from_raw_shim(100_000)), p(Nm::from_raw_shim(2_500_000), Nm::from_raw_shim(600_000))), ("on the line", p(Nm::from_raw_shim(2_500_000), Nm::ZERO), p(Nm::from_raw_shim(2_500_000), Nm::from_raw_shim(601_000))), ("on the other side", p(Nm::from_raw_shim(2_500_000), Nm::from_raw_shim(-400_000)), p(Nm::from_raw_shim(2_500_000), Nm::from_raw_shim(-600_000)))] {
        let mut node = Node::new();
        node.add(via("GND", "v1", c));
        let out = shove_line(&node, &[p(Nm::ZERO, Nm::ZERO), p(Nm::from_raw_shim(5_000_000), Nm::ZERO)], &net_of("SIG"), 0, Nm::from_raw_shim(200_000), &rules, &settings).expect(name);
        assert_eq!(out.displaced_vias.len(), 1, "{name}");
        let at = out.displaced_vias[0].pos;
        // edge to edge: clearance 200 + head half 100 + via radius 300 from the head's centre line
        assert!((at.y.abs() - Nm::from_raw_shim(600_000)).abs() <= Nm::from_raw_shim(1) && at.x == Nm::from_raw_shim(2_500_000), "{name}: via went to {at:?}, expected about {expected:?}");
        assert!(at.y.abs() >= Nm::from_raw_shim(600_000), "{name}: never closer than the clearance, got {at:?}");
    }
}

/// D6: the tracks attached to a pushed via follow it and stay on 45-degree headings.
#[test]
fn a_pushed_vias_tracks_follow_it_at_45_degrees() {
    let mut node = Node::new();
    node.add(via("GND", "v1", p(Nm::from_raw_shim(2_500_000), Nm::from_raw_shim(100_000))));
    track(&mut node, "GND", "f1", &[p(Nm::from_raw_shim(2_500_000), Nm::from_raw_shim(100_000)), p(Nm::from_raw_shim(2_500_000), Nm::from_raw_shim(3_000_000))], Nm::from_raw_shim(200_000));
    track(&mut node, "GND", "f2", &[p(Nm::from_raw_shim(2_500_000), Nm::from_raw_shim(100_000)), p(Nm::from_raw_shim(4_500_000), Nm::from_raw_shim(2_100_000))], Nm::from_raw_shim(200_000));
    let out = shove_line(&node, &[p(Nm::ZERO, Nm::ZERO), p(Nm::from_raw_shim(5_000_000), Nm::ZERO)], &net_of("SIG"), 0, Nm::from_raw_shim(200_000), &rules(), &RoutingSettings::default()).expect("pushed");
    assert_eq!(out.displaced_vias[0].pos, p(Nm::from_raw_shim(2_500_000), Nm::from_raw_shim(600_000)));
    assert_eq!(out.displaced_lines.len(), 2, "both attached tracks are re-shaped");
    for d in &out.displaced_lines {
        assert!(d.line.pts.contains(&p(Nm::from_raw_shim(2_500_000), Nm::from_raw_shim(600_000))), "{:?} ends on the via's new place", d.line.pts);
        for (a, b) in d.line.segs() {
            let (dx, dy) = ((b.x - a.x).abs(), (b.y - a.y).abs());
            assert!(dx == Nm::ZERO || dy == Nm::ZERO || dx == dy, "leg {a:?}-{b:?} of {:?} is not on a 45-degree heading", d.source_track);
        }
    }
    assert_no_new_violations(&node, &out, "SIG", Nm::from_raw_shim(200_000), &rules());
}

/// D6: a via pushed between two tracks pushes one of them in turn (a via with no track of its own is
/// a lone-via line on the stack: `shoveLineFromLoneVia`).
#[test]
fn a_via_pushed_between_two_tracks_pushes_the_one_it_runs_into() {
    let mut node = Node::new();
    track(&mut node, "NETA", "ta", &[p(Nm::from_raw_shim(300_000), Nm::from_raw_shim(-900_000)), p(Nm::from_raw_shim(5_000_000), Nm::from_raw_shim(-900_000))], Nm::from_raw_shim(200_000));
    track(&mut node, "NETB", "tb", &[p(Nm::from_raw_shim(300_000), Nm::from_raw_shim(900_000)), p(Nm::from_raw_shim(5_000_000), Nm::from_raw_shim(900_000))], Nm::from_raw_shim(200_000));
    node.add(via("GND", "v1", p(Nm::from_raw_shim(1_000_000), Nm::from_raw_shim(300_000))));
    // the head ends 500 from the via's centre, up and to the left of it: the via goes up and to the right
    let raw = vec![p(Nm::from_raw_shim(-2_000_000), Nm::from_raw_shim(-300_000)), p(Nm::from_raw_shim(600_000), Nm::ZERO)];
    let out = shove_line(&node, &raw, &net_of("SIG"), 0, Nm::from_raw_shim(200_000), &rules(), &RoutingSettings::default()).expect("pushed");
    assert_no_new_violations(&node, &out, "SIG", Nm::from_raw_shim(200_000), &rules());
    let v = out.displaced_vias.first().expect("the via moved").pos;
    assert!(v.x > Nm::from_raw_shim(1_000_000) && v.y > Nm::from_raw_shim(300_000), "up and to the right: {v:?}");
    let tb = out.displaced_lines.iter().find(|d| d.source_track.as_deref() == Some("tb")).expect("the upper track gave way to the via");
    assert_eq!(tb.line.width, Nm::from_raw_shim(200_000));
    assert!(tb.line.pts.iter().any(|q| q.y > Nm::from_raw_shim(900_000)), "pushed away from the via: {:?}", tb.line.pts);
    assert!(out.displaced_lines.iter().all(|d| d.source_track.as_deref() != Some("ta")), "the lower track is out of reach");
    // the via stayed between the two tracks
    let (ta, tb) = (Nm::from_raw_shim(-900_000), tb.line.pts.iter().map(|q| q.y).fold(Nm::from_raw_shim(900_000), Ord::max));
    assert!(v.y > ta && v.y < tb);
}

/// `ShoveIterationLimit` (and its off-by-one: the loop ends when `m_iter >= limit`): a limit of 1 fails a
/// shove that needs the second iteration to see the stack empty.
#[test]
fn the_iteration_limit_makes_a_shove_fail() {
    let mut node = Node::new();
    track(&mut node, "GND", "t1", &[p(Nm::from_raw_shim(2_500_000), Nm::from_raw_shim(-2_000_000)), p(Nm::from_raw_shim(2_500_000), Nm::from_raw_shim(2_000_000))], Nm::from_raw_shim(200_000));
    let raw = vec![p(Nm::ZERO, Nm::ZERO), p(Nm::from_raw_shim(5_000_000), Nm::ZERO)];
    let tight = RoutingSettings { shove_iteration_limit: 1, ..RoutingSettings::default() };
    assert!(shove_line(&node, &raw, &net_of("SIG"), 0, Nm::from_raw_shim(200_000), &rules(), &tight).is_none());
    assert!(shove_line(&node, &raw, &net_of("SIG"), 0, Nm::from_raw_shim(200_000), &rules(), &RoutingSettings::default()).is_some());
}

/// `ShoveVias()` off: a via is no longer pushed (`SH_TRY_WALK`): the head walks around it instead.
#[test]
fn with_shove_vias_off_the_head_walks_around_the_via() {
    let mut node = Node::new();
    node.add(via("GND", "v1", p(Nm::from_raw_shim(2_500_000), Nm::ZERO)));
    let raw = vec![p(Nm::ZERO, Nm::ZERO), p(Nm::from_raw_shim(5_000_000), Nm::ZERO)];
    let settings = RoutingSettings { shove_vias: false, ..RoutingSettings::default() };
    let out = shove_line(&node, &raw, &net_of("SIG"), 0, Nm::from_raw_shim(200_000), &rules(), &settings).expect("walks around");
    assert!(out.displaced_vias.is_empty());
    assert_ne!(out.head, raw);
    assert_no_new_violations(&node, &out, "SIG", Nm::from_raw_shim(200_000), &rules());
}
