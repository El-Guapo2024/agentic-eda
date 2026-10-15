//! `POST /api/append_board/read`: what Append Board would bring in from a `.kicad_pcb`, and where the picture is held by.
//!
//! Ports the part of `PCB_CONTROL::AppendBoard` / `placeBoardItems` (`pcbnew/tools/pcb_control.cpp` at KiCad 8303b2ad) that the page cannot do for itself. A browser
//! gives the page a file's text and not its path, so the file is read here as the paste reads KiCad's clipboard: the text of a board file *is* the clipboard form
//! `Cmd::PasteClipboard` takes (a `(kicad_pcb ..)` of items -- `CLIPBOARD_IO` writes and parses the same format as a board file), and its items keep the file's own
//! coordinates. What Append Board adds to a paste is the point the items are held by while they follow the cursor: `placeBoardItems( .., aAnchorAtOrigin = false, ..)`
//! sets the selection's reference point to the position of its top left item (`PCB_SELECTION::GetTopLeftItem`: the smallest x, then the smallest y, by each item's
//! `GetPosition()`), so the page pastes with `at = cursor - reference` and that item is where the cursor is.
//!
//! Request: `{ "text": "<the .kicad_pcb>", "name": "board.kicad_pcb" }` (`name` optional). Reply: `{ "ok": true, "reference": { "x", "y" } (micrometres),
//! "counts": { "footprints", "tracks", ... }, "notes": ["..."] }` or `{ "ok": false, "message": "..." }`. Nothing on the board changes; the append itself is the paste.
//!
//! # Where this differs from KiCad
//!
//! * The paste's rules apply to what comes in, where `LoadBoard` appends everything: copper on a layer this board does not have is dropped (KiCad adds the layer to the
//!   board), and an item on a net this board does not have and no appended footprint brings is added with no net. A footprint whose reference is taken keeps the reference
//!   it shows in the file, as KiCad's append leaves it ("Don't reannotate dupes on Append Board": `PASTE_MODE::KEEP_ANNOTATIONS`), but is filed under the next free reference,
//!   a part existing once here (`pcb_paste_special.rs`). The notes say how many.
//! * A group's position is the middle of the box of its members' positions (`PCB_GROUP::GetPosition` is the middle of the group's own box); the items of a group are not
//!   candidates for the top left one, as the selection holds the group and not them.

use crate::board;
use eda_kicad::{ClipRef, Clipboard};
use eda_model::ir::{Point, Shape};
use serde_json::{json, Value};
use std::collections::BTreeSet;
use std::path::Path;

pub fn read(dir: &Path, body: &[u8]) -> Value {
    let req: Value = match serde_json::from_slice(body) {
        Ok(v) => v,
        Err(e) => return json!({ "ok": false, "message": format!("not an append request: {e}") }),
    };
    let Some(text) = req.get("text").and_then(Value::as_str) else { return json!({ "ok": false, "message": "no file to append" }) };
    if let Some(name) = req.get("name").and_then(Value::as_str) {
        // `AskLoadBoardFileName( .., KICTL_KICAD_ONLY )`: a KiCad board. (The drop of a legacy or Eagle board runs a plugin this studio does not have.)
        if !name.to_ascii_lowercase().ends_with(".kicad_pcb") {
            return json!({ "ok": false, "message": "Only KiCad boards (.kicad_pcb files) can be appended." });
        }
    }
    let clip = match eda_kicad::parse_pcb_clipboard(text) {
        Ok(c) => c,
        Err(e) => return json!({ "ok": false, "message": format!("Error loading board. {}", board::reasons(&e)) }),
    };
    if clip.bare_footprint {
        return json!({ "ok": false, "message": "Error loading board. The file holds a footprint, not a board." });
    }
    let reference = reference_point(&clip);
    let mut notes = Vec::new();
    // What the paste will leave out or change, against the board as it is now.
    if let Ok((_, _, model)) = board::load(dir) {
        let layers: BTreeSet<&str> = model.board.layers.iter().map(String::as_str).collect();
        let mut missing: BTreeSet<&str> = BTreeSet::new();
        let copper = |l: &'_ str| l.ends_with(".Cu") && !layers.contains(l);
        for t in &clip.tracks {
            if copper(&t.layer) {
                missing.insert(&t.layer);
            }
        }
        for v in &clip.vias {
            for l in [&v.from_layer, &v.to_layer] {
                if copper(l) {
                    missing.insert(l);
                }
            }
        }
        for z in &clip.zones {
            for l in z.layer_set() {
                if copper(l) {
                    missing.insert(l);
                }
            }
        }
        if !missing.is_empty() {
            notes.push(format!("Copper on {} is left out: this board has no such layer.", missing.into_iter().collect::<Vec<_>>().join(", ")));
        }
        let brought: BTreeSet<&str> = clip.footprints.iter().flat_map(|f| f.pad_nets.iter().map(|(_, n)| n.as_str())).collect();
        let known = |n: &str| n.is_empty() || brought.contains(n) || model.nets.iter().any(|b| b.name == n);
        let netless = clip.tracks.iter().filter(|t| !known(&t.net)).count() + clip.vias.iter().filter(|v| !known(&v.net)).count() + clip.zones.iter().filter(|z| !z.is_rule_area && !known(&z.net)).count();
        if netless > 0 {
            let (items, be, nets) = if netless == 1 { ("item is", "is", "a net") } else { ("items are", "are", "nets") };
            notes.push(format!("{netless} copper {items} on {nets} this board does not have and {be} added with no net."));
        }
        let repeated = clip.footprints.iter().filter(|f| model.part(&f.reference).is_some()).count();
        if repeated > 0 {
            let (s, has, is) = if repeated == 1 { ("", "has", "is") } else { ("s", "have", "are") };
            notes.push(format!("{repeated} footprint{s} {has} a reference this board already uses: {} the reference the file gives (as KiCad's Append Board leaves it) and {is} filed under the next free one here.", if repeated == 1 { "it shows" } else { "they show" }));
        }
    }
    json!({
        "ok": true,
        "reference": { "x": reference.x, "y": reference.y },
        "counts": {
            "footprints": clip.footprints.len(), "tracks": clip.tracks.len(), "vias": clip.vias.len(), "zones": clip.zones.len(),
            "shapes": clip.shapes.len(), "texts": clip.texts.len(), "dimensions": clip.dimensions.len(), "groups": clip.groups.len(),
        },
        "notes": notes,
    })
}

/// `PCB_SELECTION::GetTopLeftItem( false )->GetPosition()` over the items `placeBoardItems` selects: the items of the file that are not inside a group in it, and the groups
/// that are not inside another. The leftmost position wins and, among equals, the highest. An empty clipboard has none (the origin).
pub fn reference_point(clip: &Clipboard) -> Point {
    // Every item a group holds, however deep, and the groups held by a group.
    let key = |r: ClipRef| -> Option<(u8, usize)> {
        Some(match r {
            ClipRef::Track(i) => (0, i),
            ClipRef::Via(i) => (1, i),
            ClipRef::Zone(i) => (2, i),
            ClipRef::Shape(i) => (3, i),
            ClipRef::Text(i) => (4, i),
            ClipRef::Dimension(i) => (5, i),
            ClipRef::Footprint(i) => (6, i),
            ClipRef::Group(_) => return None,
        })
    };
    let mut grouped: BTreeSet<(u8, usize)> = BTreeSet::new();
    let mut held_groups: BTreeSet<usize> = BTreeSet::new();
    for g in &clip.groups {
        for m in &g.members {
            match m {
                ClipRef::Group(i) => {
                    held_groups.insert(*i);
                }
                other => {
                    grouped.extend(key(*other));
                }
            }
        }
    }
    let mut best: Option<Point> = None;
    let mut consider = |p: Point| {
        if best.is_none_or(|b| p.x < b.x || (p.x == b.x && p.y < b.y)) {
            best = Some(p);
        }
    };
    let free = |kind: u8, i: usize| !grouped.contains(&(kind, i));
    for (i, t) in clip.tracks.iter().enumerate() {
        if free(0, i) {
            consider(track_position(t));
        }
    }
    for (i, v) in clip.vias.iter().enumerate() {
        if free(1, i) {
            consider(v.at);
        }
    }
    for (i, z) in clip.zones.iter().enumerate() {
        if free(2, i) {
            // `ZONE::GetPosition`: the first corner of the outline.
            consider(z.outline.first().copied().unwrap_or(Point { x: 0, y: 0 }));
        }
    }
    for (i, s) in clip.shapes.iter().enumerate() {
        if free(3, i) {
            consider(shape_position(s));
        }
    }
    for (i, t) in clip.texts.iter().enumerate() {
        if free(4, i) {
            consider(t.at);
        }
    }
    for (i, d) in clip.dimensions.iter().enumerate() {
        if free(5, i) {
            consider(d.start);
        }
    }
    for (i, f) in clip.footprints.iter().enumerate() {
        if free(6, i) {
            consider(f.at);
        }
    }
    for gi in (0..clip.groups.len()).filter(|gi| !held_groups.contains(gi)) {
        if let Some(p) = group_position(clip, gi) {
            consider(p);
        }
    }
    best.unwrap_or(Point { x: 0, y: 0 })
}

/// `PCB_TRACK::GetPosition` is the start; `PCB_ARC::GetPosition` the centre of the arc.
fn track_position(t: &eda_model::ir::Track) -> Point {
    if let Some((s, m, e)) = t.arc() {
        if let Some(c) = arc_center(s, m, e) {
            return c;
        }
    }
    t.pts.first().copied().unwrap_or(Point { x: 0, y: 0 })
}

/// `EDA_SHAPE::getPosition`: an arc's centre, a polygon's first vertex, the start of anything else (a circle's is its centre).
fn shape_position(s: &Shape) -> Point {
    match s {
        Shape::Segment { start, .. } | Shape::Rect { start, .. } | Shape::Bezier { start, .. } => *start,
        Shape::Circle { center, .. } => *center,
        Shape::Polygon { pts, .. } => pts.first().copied().unwrap_or(Point { x: 0, y: 0 }),
        Shape::Arc { start, mid, end, .. } => arc_center(*start, *mid, *end).unwrap_or(*start),
    }
}

/// `CalcArcCenter`: the centre of the circle through three points (the middle of a whole circle's diameter when the ends meet); `None` when they are collinear.
fn arc_center(start: Point, mid: Point, end: Point) -> Option<Point> {
    let (sx, sy, mx, my, ex, ey) = (start.x as f64, start.y as f64, mid.x as f64, mid.y as f64, end.x as f64, end.y as f64);
    if start == end {
        return Some(Point { x: ((sx + mx) / 2.0).round() as i64, y: ((sy + my) / 2.0).round() as i64 });
    }
    let d = 2.0 * (sx * (my - ey) + mx * (ey - sy) + ex * (sy - my));
    if d == 0.0 {
        return None;
    }
    let ux = ((sx * sx + sy * sy) * (my - ey) + (mx * mx + my * my) * (ey - sy) + (ex * ex + ey * ey) * (sy - my)) / d;
    let uy = ((sx * sx + sy * sy) * (ex - mx) + (mx * mx + my * my) * (sx - ex) + (ex * ex + ey * ey) * (mx - sx)) / d;
    Some(Point { x: ux.round() as i64, y: uy.round() as i64 })
}

/// The middle of the box of the positions of everything a group holds (the groups in it included): `PCB_GROUP::GetPosition` stands at the middle of the group's own box,
/// which these positions approximate.
fn group_position(clip: &Clipboard, group: usize) -> Option<Point> {
    let mut pts: Vec<Point> = Vec::new();
    collect_group(clip, group, &mut pts, &mut BTreeSet::new());
    if pts.is_empty() {
        return None;
    }
    let (x0, x1) = (pts.iter().map(|p| p.x).min()?, pts.iter().map(|p| p.x).max()?);
    let (y0, y1) = (pts.iter().map(|p| p.y).min()?, pts.iter().map(|p| p.y).max()?);
    Some(Point { x: (x0 + x1) / 2, y: (y0 + y1) / 2 })
}

fn collect_group(clip: &Clipboard, group: usize, out: &mut Vec<Point>, seen: &mut BTreeSet<usize>) {
    if !seen.insert(group) {
        return;
    }
    let Some(g) = clip.groups.get(group) else { return };
    for m in &g.members {
        match *m {
            ClipRef::Track(i) => out.extend(clip.tracks.get(i).map(track_position)),
            ClipRef::Via(i) => out.extend(clip.vias.get(i).map(|v| v.at)),
            ClipRef::Zone(i) => out.extend(clip.zones.get(i).and_then(|z| z.outline.first().copied())),
            ClipRef::Shape(i) => out.extend(clip.shapes.get(i).map(shape_position)),
            ClipRef::Text(i) => out.extend(clip.texts.get(i).map(|t| t.at)),
            ClipRef::Dimension(i) => out.extend(clip.dimensions.get(i).map(|d| d.start)),
            ClipRef::Footprint(i) => out.extend(clip.footprints.get(i).map(|f| f.at)),
            ClipRef::Group(i) => collect_group(clip, i, out, seen),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(x: i64, y: i64) -> Point {
        Point { x, y }
    }

    /// A board file with a footprint, two tracks, a via, a zone, a line on Edge.Cuts and a text, each at a position of its own.
    fn board_file() -> String {
        r#"(kicad_pcb (version 20241229) (generator "pcbnew") (generator_version "9.0")
  (general (thickness 1.6) (legacy_teardrops no))
  (paper "A4")
  (layers (0 "F.Cu" signal) (2 "B.Cu" signal) (25 "Edge.Cuts" user) (5 "F.SilkS" user))
  (net 0 "")
  (net 1 "GND")
  (footprint "Lib:R0603" (layer "F.Cu") (uuid "11111111-1111-1111-1111-111111111111") (at 50 40)
    (property "Reference" "R1" (at 0 -1 0) (layer "F.SilkS") (uuid "11111111-1111-1111-1111-111111111112") (effects (font (size 1 1) (thickness 0.15))))
    (property "Value" "10k" (at 0 1 0) (layer "F.Fab") (uuid "11111111-1111-1111-1111-111111111113") (effects (font (size 1 1) (thickness 0.15))))
    (pad "1" smd rect (at -0.8 0) (size 0.8 0.9) (layers "F.Cu" "F.Mask" "F.Paste") (net 1 "GND") (uuid "11111111-1111-1111-1111-111111111114"))
    (pad "2" smd rect (at 0.8 0) (size 0.8 0.9) (layers "F.Cu" "F.Mask" "F.Paste") (uuid "11111111-1111-1111-1111-111111111115")))
  (segment (start 30 60) (end 60 60) (width 0.25) (layer "F.Cu") (net 1) (uuid "22222222-2222-2222-2222-222222222221"))
  (segment (start 30 20) (end 30 60) (width 0.25) (layer "F.Cu") (net 1) (uuid "22222222-2222-2222-2222-222222222222"))
  (via (at 70 25) (size 0.8) (drill 0.4) (layers "F.Cu" "B.Cu") (net 1) (uuid "33333333-3333-3333-3333-333333333331"))
  (gr_line (start 10 10) (end 90 10) (stroke (width 0.1) (type solid)) (layer "Edge.Cuts") (uuid "44444444-4444-4444-4444-444444444441"))
  (gr_text "hello" (at 20 80 0) (layer "F.SilkS") (uuid "55555555-5555-5555-5555-555555555551") (effects (font (size 1 1) (thickness 0.15))))
)"#
        .to_string()
    }

    #[test]
    fn the_item_with_the_smallest_x_then_the_smallest_y_is_the_one_the_items_are_held_by() {
        let clip = eda_kicad::parse_pcb_clipboard(&board_file()).unwrap();
        // The gr_line at (10, 10) is the leftmost of every position: the footprint (50, 40), the segments (30, 60) and (30, 20), the via (70, 25), the text (20, 80).
        assert_eq!(reference_point(&clip), p(10_000, 10_000));
        // Two items at the same x: the higher (smaller y) wins.
        let text = board_file().replace("(gr_line (start 10 10) (end 90 10)", "(gr_line (start 30 5) (end 90 10)").replace("(at 20 80 0)", "(at 31 80 0)");
        let clip = eda_kicad::parse_pcb_clipboard(&text).unwrap();
        assert_eq!(reference_point(&clip), p(30_000, 5_000));
    }

    #[test]
    fn an_arcs_position_is_its_centre_and_a_circles_is_its_centre() {
        assert_eq!(arc_center(p(10_000, 0), p(0, 10_000), p(-10_000, 0)), Some(p(0, 0)));
        assert_eq!(arc_center(p(0, 0), p(10, 10), p(20, 20)), None, "collinear points have no centre");
        let c = Shape::Circle { id: String::new(), layer: "Dwgs.User".into(), stroke_width: 100, filled: false, center: p(5, 6), end: p(9, 6) };
        assert_eq!(shape_position(&c), p(5, 6));
        let a = Shape::Arc { id: String::new(), layer: "Edge.Cuts".into(), stroke_width: 100, filled: false, start: p(10_000, 0), mid: p(0, 10_000), end: p(-10_000, 0) };
        assert_eq!(shape_position(&a), p(0, 0));
    }

    #[test]
    fn the_route_reads_the_file_counts_what_is_in_it_and_says_what_the_paste_will_change() {
        let dir = std::env::temp_dir().join(format!("eda_append_api_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        // No board in the folder: the file is still read and its reference given, with no notes about a board that is not there.
        let r = read(&dir, json!({ "text": board_file(), "name": "other.kicad_pcb" }).to_string().as_bytes());
        assert_eq!(r["ok"], true, "{r}");
        assert_eq!(r["reference"], json!({ "x": 10_000, "y": 10_000 }));
        assert_eq!(r["counts"], json!({ "footprints": 1, "tracks": 2, "vias": 1, "zones": 0, "shapes": 1, "texts": 1, "dimensions": 0, "groups": 0 }));
        assert_eq!(r["notes"], json!([]));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A board folder of its own: a part R1 on GND, placed, on a two-layer board.
    fn board_dir(tag: &str) -> std::path::PathBuf {
        use eda_model::ir::{Design, FootprintInstance, PlacementSection, Provenance, Side};
        use eda_model::{ConstraintModel, Footprint, Net, Part, Pad, PadKind, PadShape, Pin, PinKind};
        let dir = std::env::temp_dir().join(format!("eda_append_api_board_{}_{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let pad = |n: &str, x: i64| Pad { opposite_side: false, ext: Default::default(), number: n.into(), at: (x, 0), size: (800, 800), shape: PadShape::Rect, kind: PadKind::Smd, drill: None, drill_slot: None, rot: 0, roundrect_ratio: None };
        let fp = Footprint { name: "R0603".into(), pads: vec![pad("1", -800), pad("2", 800)], courtyard: Some((3000, 1500)), model: None, courtyard_outlines: vec![], models3d: vec![], art: Default::default() };
        let part = Part {
            reference: "R1".into(),
            mpn: None,
            lcsc: None,
            value: None,
            package: Some("R0603".into()),
            footprint: Some("R0603".into()),
            pins: vec![Pin { number: "1".into(), name: None, kind: PinKind::Passive }, Pin { number: "2".into(), name: None, kind: PinKind::Passive }],
            body_um: None,
            symbol: None,
            datasheet: None,
            edge: None,
        };
        let mut model = ConstraintModel { parts: vec![part], nets: vec![Net { name: "GND".into(), pins: vec!["R1.1".into()] }], footprints: vec![fp], ..Default::default() };
        model.board.layers = vec!["F.Cu".into(), "B.Cu".into()];
        let intent = dir.join("intent.yaml");
        std::fs::write(&intent, serde_yaml::to_string(&model).unwrap()).unwrap();
        let design = Design {
            footprint_library: None,
            sheet_contents: None,
            bus_aliases: vec![],
            symbol_library: None,
            schema: 1,
            provenance: Provenance { engine_version: "t".into(), intent_hash: "x".into(), seed: 0, stage_hashes: vec![] },
            schematic: None,
            nets: None,
            placement: Some(PlacementSection {
                outline: vec![p(0, 0), p(100_000, 0), p(100_000, 100_000), p(0, 100_000)],
                footprints: vec![FootprintInstance { id: "R1".into(), at: p(20_000, 20_000), rot: 0, side: Side::Top, label: Default::default() }],
                modules: vec![],
            }),
            routing: None,
            drawings: None,
        };
        board::save(&dir, &design).unwrap();
        std::fs::write(dir.join("board.json"), json!({ "intent": intent.display().to_string(), "snap_um": 100, "spacing_um": 300 }).to_string()).unwrap();
        dir
    }

    #[test]
    fn the_notes_say_what_the_paste_will_leave_out_or_change_on_this_board() {
        let dir = board_dir("notes");
        // The file's R1 is a reference this board uses; its track on In1.Cu has no layer here; and a segment on a net nobody brings.
        let text = board_file().replace("(layers (0 \"F.Cu\" signal)", "(layers (0 \"F.Cu\" signal) (4 \"In1.Cu\" signal)").replace(
            "(gr_line",
            "(segment (start 11 11) (end 12 11) (width 0.25) (layer \"In1.Cu\") (net 1) (uuid \"66666666-6666-6666-6666-666666666661\"))\n  (segment (start 13 11) (end 14 11) (width 0.25) (layer \"F.Cu\") (net 2) (uuid \"66666666-6666-6666-6666-666666666662\"))\n  (gr_line",
        ).replace("(net 1 \"GND\")", "(net 1 \"GND\")\n  (net 2 \"SIG\")");
        let r = read(&dir, json!({ "text": text, "name": "other.kicad_pcb" }).to_string().as_bytes());
        assert_eq!(r["ok"], true, "{r}");
        let notes: Vec<String> = r["notes"].as_array().unwrap().iter().map(|n| n.as_str().unwrap().to_string()).collect();
        assert!(notes.iter().any(|n| n == "Copper on In1.Cu is left out: this board has no such layer."), "{notes:?}");
        assert!(notes.iter().any(|n| n == "1 copper item is on a net this board does not have and is added with no net."), "{notes:?}");
        assert!(notes.iter().any(|n| n == "1 footprint has a reference this board already uses: it shows the reference the file gives (as KiCad's Append Board leaves it) and is filed under the next free one here."), "{notes:?}");
        // The reference is still the leftmost item's position: the new segments at x = 11 sit right of the line at 10.
        assert_eq!(r["reference"], json!({ "x": 10_000, "y": 10_000 }));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Append Board is the paste of the file's own text with its reference point on the cursor: `at = cursor - reference`.
    #[test]
    fn pasting_the_file_at_the_cursor_less_the_reference_puts_the_top_left_item_at_the_cursor_as_one_undo_step() {
        let dir = board_dir("paste");
        let text = board_file();
        let r = read(&dir, json!({ "text": text, "name": "other.kicad_pcb" }).to_string().as_bytes());
        let (rx, ry) = (r["reference"]["x"].as_i64().unwrap(), r["reference"]["y"].as_i64().unwrap());
        let cursor = p(60_000, 70_000);
        board::step(&dir, eda_ops::Cmd::PasteClipboard { text, at: p(cursor.x - rx, cursor.y - ry) }, false, "test").unwrap();
        let (_, d, model) = board::load(&dir).unwrap();
        let dr = d.drawings.as_ref().unwrap();
        let line = dr.shapes.iter().find(|s| s.layer() == "Edge.Cuts").expect("the file's Edge.Cuts line came in");
        assert_eq!(line.points()[0], cursor, "the top left item of the file is where the cursor was");
        assert_eq!(line.points()[1], p(cursor.x + 80_000, cursor.y), "and the rest keeps its place relative to it");
        // The file's footprint R1 met this board's R1: it is a part of its own now, filed under the next free reference.
        assert_eq!(d.placement.as_ref().unwrap().footprints.len(), 2);
        assert_eq!(model.parts.len(), 2, "{:?}", model.parts.iter().map(|p| p.reference.clone()).collect::<Vec<_>>());
        board::undo(&dir, "test", Some(eda_ops::Domain::Pcb)).unwrap();
        let (_, back, model) = board::load(&dir).unwrap();
        assert!(back.drawings.as_ref().map_or(true, |d| d.shapes.is_empty()) && back.placement.as_ref().unwrap().footprints.len() == 1 && model.parts.len() == 1, "one undo takes the whole append back");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A footprint's lines and texts are its own (`Footprint::art`): an appended board keeps the silkscreen and the fab drawings of its footprints, in the model
    /// and in the `.kicad_pcb` written from it, which kicad-cli plots.
    #[test]
    fn an_appended_footprint_keeps_its_silkscreen_and_fab_art() {
        let dir = board_dir("art");
        let art = "(fp_line (start -1.5 -0.9) (end 1.5 -0.9) (stroke (width 0.12) (type solid)) (layer \"F.SilkS\") (uuid \"b0000000-0000-0000-0000-000000000001\"))\n    \
                   (fp_rect (start -1 -0.5) (end 1 0.5) (stroke (width 0.1) (type solid)) (fill no) (layer \"F.Fab\") (uuid \"b0000000-0000-0000-0000-000000000002\"))\n    \
                   (fp_text user \"${REFERENCE}\" (at 0 0 0) (layer \"F.Fab\") (uuid \"b0000000-0000-0000-0000-000000000003\") (effects (font (size 0.5 0.5) (thickness 0.08))))\n    \
                   (pad \"1\" smd rect (at -0.8 0)";
        // A footprint this board's model does not know by its name keeps the definition the file gives (the art with it); one it knows is the model's own.
        let text = board_file().replacen("(pad \"1\" smd rect (at -0.8 0)", art, 1).replace("Lib:R0603", "Custom:Widget_9x9");
        let r = read(&dir, json!({ "text": text, "name": "other.kicad_pcb" }).to_string().as_bytes());
        assert_eq!(r["ok"], true, "{r}");
        let at = p(60_000, 70_000);
        let (rx, ry) = (r["reference"]["x"].as_i64().unwrap(), r["reference"]["y"].as_i64().unwrap());
        board::step(&dir, eda_ops::Cmd::PasteClipboard { text, at: p(at.x - rx, at.y - ry) }, false, "test").unwrap();
        let (_, d, model) = board::load(&dir).unwrap();
        let new_part = model.parts.iter().find(|p| p.reference != "R1").expect("the appended footprint is a part of its own");
        let fp = model.footprint_of(new_part).expect("and has its own definition");
        assert_eq!(fp.art.shapes.iter().filter(|s| s.on_layer("F.SilkS")).count(), 1, "{:?}", fp.art);
        assert_eq!(fp.art.shapes.iter().filter(|s| s.on_layer("F.Fab")).count(), 1, "{:?}", fp.art);
        assert_eq!(fp.art.texts.len(), 1, "{:?}", fp.art);
        let pcb = eda_kicad::export_kicad_pcb(&d, &model, &eda_kicad::ExportMeta { date: "2026-01-01", title: "t" }).unwrap();
        assert!(pcb.contains("(fp_line") && pcb.contains("(fp_rect") && pcb.contains("${REFERENCE}"), "the written board has the art");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The same for a footprint of a common package, which the built-in table resolves by its name: the file's drawings are still the appended footprint's.
    #[test]
    fn an_appended_footprint_of_a_common_package_keeps_its_art_too() {
        let dir = board_dir("art_builtin");
        let art = "(fp_line (start -1.5 -0.9) (end 1.5 -0.9) (stroke (width 0.12) (type solid)) (layer \"F.SilkS\") (uuid \"b0000000-0000-0000-0000-000000000011\"))\n    \
                   (fp_rect (start -1 -0.5) (end 1 0.5) (stroke (width 0.1) (type solid)) (fill no) (layer \"F.Fab\") (uuid \"b0000000-0000-0000-0000-000000000012\"))\n    \
                   (pad \"1\" smd rect (at -0.8 0)";
        let text = board_file().replacen("(pad \"1\" smd rect (at -0.8 0)", art, 1).replace("Lib:R0603", "Resistor_SMD:R_0603_1608Metric");
        let r = read(&dir, json!({ "text": text, "name": "other.kicad_pcb" }).to_string().as_bytes());
        assert_eq!(r["ok"], true, "{r}");
        let (rx, ry) = (r["reference"]["x"].as_i64().unwrap(), r["reference"]["y"].as_i64().unwrap());
        board::step(&dir, eda_ops::Cmd::PasteClipboard { text, at: p(60_000 - rx, 70_000 - ry) }, false, "test").unwrap();
        let (_, d, model) = board::load(&dir).unwrap();
        let new_part = model.parts.iter().find(|p| p.reference != "R1").expect("the appended footprint is a part of its own");
        let fp = model.footprint_of(new_part).expect("and resolves");
        assert!(fp.art.shapes.iter().any(|s| s.on_layer("F.SilkS")) && fp.art.shapes.iter().any(|s| s.on_layer("F.Fab")), "the file's silkscreen and fab drawings: {:?}", fp.art);
        let pcb = eda_kicad::export_kicad_pcb(&d, &model, &eda_kicad::ExportMeta { date: "2026-01-01", title: "t" }).unwrap();
        assert!(pcb.contains("(fp_line") && pcb.contains("(fp_rect"), "the written board has them");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn what_is_not_a_kicad_board_is_refused_in_the_words_of_the_dialog() {
        let dir = std::env::temp_dir();
        let r = read(&dir, json!({ "text": "hello", "name": "x.kicad_pcb" }).to_string().as_bytes());
        assert_eq!(r["ok"], false);
        assert!(r["message"].as_str().unwrap().starts_with("Error loading board."), "{r}");
        let r = read(&dir, json!({ "text": board_file(), "name": "old.brd" }).to_string().as_bytes());
        assert_eq!(r["ok"], false);
        assert_eq!(r["message"], "Only KiCad boards (.kicad_pcb files) can be appended.");
        let r = read(&dir, json!({ "name": "x.kicad_pcb" }).to_string().as_bytes());
        assert_eq!(r["ok"], false);
        // A single footprint is not a board.
        let fp = r#"(footprint "Lib:R0603" (layer "F.Cu") (at 0 0) (property "Reference" "R1" (at 0 0 0) (layer "F.SilkS") (effects (font (size 1 1) (thickness 0.15)))) (pad "1" smd rect (at 0 0) (size 1 1) (layers "F.Cu")))"#;
        let r = read(&dir, json!({ "text": fp, "name": "x.kicad_pcb" }).to_string().as_bytes());
        assert_eq!(r["ok"], false, "{r}");
    }
}
