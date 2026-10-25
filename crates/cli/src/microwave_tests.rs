//! The microwave tools (GAPS.md item 9, package P19) through a board directory: each footprint is a part of the board that loads with its own pads, one undo step, and the
//! `.kicad_pcb` the studio derives writes it as `createBaseFootprint` leaves it (no library link, copper only, no courtyard). The slow tier (`EDA_SLOW_TESTS=1`) hands the board to
//! kicad-cli: it loads the five footprints, writes them back as it writes its own (the same pads, graphics and texts, entry for entry), and plots them where the tools'
//! arithmetic puts them (`microwave_footprint.cpp`, `microwave_inductor.cpp`, `microwave_polygon.cpp` at KiCad 8303b2ad).

use eda_model::units::{mm, Nm};
use super::tests::{scratch, setup};
use super::{load, redo, step, undo};
use eda_model::ir::Point;
use eda_ops::microwave::{Hand, MicrowaveCmd, ShapeKind};
use eda_ops::{Cmd, Domain, FlipDirection};
use std::path::Path;

fn slow() -> bool {
    let off = std::env::var_os("EDA_SLOW_TESTS").is_none();
    if off {
        eprintln!("skipped: slow test; set EDA_SLOW_TESTS=1 to run it");
    }
    !off
}

fn p(x: i64, y: i64) -> Point {
    Point { x: Nm::from_raw_shim(x), y: Nm::from_raw_shim(y) }
}

fn gap(at: Point, size_nm: i64) -> Cmd {
    Cmd::Microwave(MicrowaveCmd::Gap { at, size_nm: Nm::from_raw_shim(size_nm), track_width_nm: mm(0.25), hand: Hand::default() })
}

fn derived_board(dir: &Path) -> String {
    let (_, design, model) = load(dir).unwrap();
    eda_kicad::export_kicad_pcb(&design, &model, &eda_kicad::ExportMeta { date: "2026-10-21", title: "microwave" }).unwrap()
}

/// The `(footprint ..)` block of the board file that holds `(property "Reference" "<reference>"`.
fn footprint_block<'a>(pcb: &'a str, reference: &str) -> &'a str {
    let at = pcb.find(&format!("(property \"Reference\" \"{reference}\"")).unwrap_or_else(|| panic!("{reference} is written:\n{pcb}"));
    let start = pcb[..at].rfind("\n\t(footprint ").unwrap() + 1;
    let end = pcb[at..].find("\n\t(footprint ").map_or(pcb.len(), |i| at + i);
    &pcb[start..end]
}

// ------------------------------------------------------------------------------------------------------------------------------ the fast tier

#[test]
fn a_gap_is_a_part_of_the_board_with_its_own_pads_and_one_undo_step() {
    let dir = scratch("mw_gap_part");
    setup(&dir);
    let before = serde_json::to_value(load(&dir).unwrap().1).unwrap();
    step(&dir, gap(p(10_000_000, 12_000_000), 500_000), false, "test").unwrap();

    let (_, design, model) = load(&dir).unwrap();
    let part = model.part("REF1").expect("the gap is a part of the model now");
    assert_eq!((part.footprint.as_deref(), part.value.as_deref()), (Some("muwave_gap"), Some("muwave_gap")));
    let fp = model.footprint_of(part).expect("its pads come from its own definition");
    assert_eq!(fp.pads.iter().map(|p| (p.number.as_str(), p.at, p.size)).collect::<Vec<_>>(), [("1", (Nm::new(-375_000), Nm::ZERO), (mm(0.25), mm(0.25))), ("2", (Nm::new(375_000), Nm::ZERO), (mm(0.25), mm(0.25)))]);
    assert_eq!(part.pins.iter().map(|p| p.number.as_str()).collect::<Vec<_>>(), ["1", "2"], "a pin for each pad");
    let pose = design.placement.as_ref().unwrap().footprints.iter().find(|f| f.id == "REF1").unwrap();
    assert_eq!((pose.at, pose.rot), (p(10_000_000, 12_000_000), 0));

    // one undo step takes the whole tool back, the part, its fields' edit and its facts; redo brings it back
    undo(&dir, "test", Some(Domain::Pcb)).unwrap();
    assert_eq!(serde_json::to_value(load(&dir).unwrap().1).unwrap(), before, "the board is as it was");
    assert!(load(&dir).unwrap().2.part("REF1").is_none());
    redo(&dir, "test", Some(Domain::Pcb)).unwrap();
    assert!(load(&dir).unwrap().2.part("REF1").is_some());
}

/// The model finds a footprint by its name, so two gaps of different sizes are two footprints; both come back from the project file with their own pads.
#[test]
fn gaps_of_different_sizes_keep_their_own_pads_in_the_model() {
    let dir = scratch("mw_two_gaps");
    setup(&dir);
    step(&dir, gap(p(10_000_000, 12_000_000), 500_000), false, "test").unwrap();
    step(&dir, gap(p(14_000_000, 12_000_000), 900_000), false, "test").unwrap();
    let (_, _, model) = load(&dir).unwrap();
    let x2 = |r: &str| model.footprint_of(model.part(r).unwrap()).unwrap().pads[1].at.0;
    assert_eq!((x2("REF1"), x2("REF2")), (Nm::new(375_000), Nm::new(575_000)));
    assert_eq!((model.part("REF1").unwrap().footprint.as_deref(), model.part("REF2").unwrap().footprint.as_deref()), (Some("muwave_gap"), Some("muwave_gap_1")));
}

/// KiCad's tools make no courtyard, so the studio's placement gates have no keep-out of the pads' box to hold these footprints to: a gap and a stub on top of each other, or a gap at
/// the very edge of the board, are placed with Strict on. (The copper is another matter: the routing and DRC checks judge it. The one gate that still looks is the label's: a footprint
/// whose origin is under another's Reference text, within about a text height and a half, is refused as before.)
#[test]
fn microwave_footprints_are_not_held_to_a_courtyard_of_their_pads_with_strict_on() {
    let dir = scratch("mw_strict");
    setup(&dir);
    step(&dir, gap(p(10_000_000, 12_000_000), 500_000), true, "test").unwrap();
    step(&dir, Cmd::Microwave(MicrowaveCmd::Stub { at: p(10_000_000, 14_500_000), size_nm: mm(1.0), track_width_nm: mm(0.25), hand: Hand::default() }), true, "test").expect("a stub 2.5 mm under the gap, its pad reaching up to the gap's");
    step(&dir, gap(p(13_000_000, 12_000_000), 500_000), true, "test").expect("a gap 3 mm to its side");
    step(&dir, gap(p(500_000, 500_000), 500_000), true, "test").expect("a gap at the corner of the board");
    // a pad of the stub overlapping the gap's is not a placement matter: the copper is judged by the routing checks
    step(&dir, Cmd::Microwave(MicrowaveCmd::Stub { at: p(10_375_000, 13_000_000), size_nm: mm(1.0), track_width_nm: mm(0.25), hand: Hand { rotate_millideg: 0, flip: None } }), true, "test").expect("a stub whose pad lies over the gap's");
}

/// What `createBaseFootprint` leaves in the file: the footprint has no library (`LIB_ID( wxEmptyString, name )`), the attributes, the fields at the size of the track, the
/// `${REFERENCE}` text, pads on the front copper only, and no courtyard.
#[test]
fn the_derived_board_writes_a_gap_as_kicads_tool_leaves_it() {
    let dir = scratch("mw_gap_file");
    setup(&dir);
    step(&dir, gap(p(10_000_000, 12_000_000), 500_000), false, "test").unwrap();
    let pcb = derived_board(&dir);
    let block = footprint_block(&pcb, "REF1");
    assert!(block.starts_with("\t(footprint \"muwave_gap\"\n"), "no library: {}", block.lines().next().unwrap());
    assert!(block.contains("(at 10 12 0)"), "{block}");
    assert!(block.contains("(property \"Reference\" \"REF1\" (at 0 -0.5 0) (layer \"F.SilkS\")") && block.contains("(size 0.25 0.25) (thickness 0.05)"), "{block}");
    assert!(block.contains("(property \"Value\" \"muwave_gap\" (at 0 1 0) (layer \"F.Fab\")"), "{block}");
    assert!(block.contains("(attr exclude_from_pos_files exclude_from_bom)"), "neither smd nor through_hole: {block}");
    assert!(block.contains("(fp_text user \"${REFERENCE}\" (at 0 2.5 0) (layer \"F.Fab\")"), "{block}");
    assert!(block.contains("(pad \"1\" smd rect (at -0.375 0 0) (size 0.25 0.25) (layers \"F.Cu\")"), "copper only: {block}");
    assert!(block.contains("(pad \"2\" smd rect (at 0.375 0 0) (size 0.25 0.25) (layers \"F.Cu\")"), "{block}");
    assert!(!block.contains("CrtYd") && !block.contains("F.Paste") && !block.contains("F.Mask"), "no courtyard, no paste, no mask opening: {block}");
}

/// A copy of a microwave footprint (Duplicate, Create Array) is one: the same pads on the same name, copper on the front layer only and no courtyard, under its own reference.
#[test]
fn a_duplicate_of_a_microwave_footprint_is_copper_only_with_no_courtyard_too() {
    let dir = scratch("mw_duplicate");
    setup(&dir);
    step(&dir, gap(p(10_000_000, 12_000_000), 500_000), false, "test").unwrap();
    step(&dir, Cmd::Duplicate { ids: vec!["REF1".into()] }, false, "test").unwrap();
    let (_, design, model) = load(&dir).unwrap();
    assert_eq!(model.part("REF2").unwrap().footprint.as_deref(), Some("muwave_gap"), "the same footprint, by its name");
    let extras = &design.drawings.as_ref().unwrap().footprint_extras;
    let (a, b) = (extras.iter().find(|e| e.id == "REF1").unwrap(), extras.iter().find(|e| e.id == "REF2").expect("the copy has the facts too"));
    assert_eq!((b.no_courtyard, b.pads.clone(), b.unlinked_name.clone(), b.uuid.clone()), (a.no_courtyard, a.pads.clone(), a.unlinked_name.clone(), None));
    let pcb = derived_board(&dir);
    let copy = footprint_block(&pcb, "REF2");
    assert!(copy.starts_with("\t(footprint \"muwave_gap\"\n") && copy.contains("(pad \"1\" smd rect (at -0.375 0 0) (size 0.25 0.25) (layers \"F.Cu\")"), "{copy}");
    assert!(!copy.contains("CrtYd") && !copy.contains("F.Paste") && !copy.contains("F.Mask"), "{copy}");
}

#[test]
fn the_derived_board_writes_the_arc_stub_the_polygon_and_the_inductor() {
    let dir = scratch("mw_file_others");
    setup(&dir);
    step(&dir, Cmd::Microwave(MicrowaveCmd::ArcStub { at: p(10_000_000, 12_000_000), radius_nm: mm(1.5), angle_millideg: 90_000, track_width_nm: mm(0.25), hand: Hand::default() }), false, "test").unwrap();
    step(&dir, Cmd::Microwave(MicrowaveCmd::FunctionShape { at: p(6_000_000, 16_000_000), file: SHAPE.into(), kind: ShapeKind::Normal, size_x_nm: None, size_y_nm: None, track_width_nm: mm(0.25), hand: Hand::default() }), false, "test").unwrap();
    step(&dir, Cmd::Microwave(MicrowaveCmd::Inductor { start: p(12_000_000, 16_000_000), end: p(17_000_000, 16_000_000), length_nm: mm(12.0), name: "L7".into(), track_width_nm: mm(0.25) }), false, "test").unwrap();
    let pcb = derived_board(&dir);
    let arc = footprint_block(&pcb, "REF1");
    assert!(arc.contains("(pad \"1\" smd custom (at 0 0 0) (size 0.25 0.25) (layers \"F.Cu\") (options (clearance outline) (anchor rect)) (primitives (gr_poly (pts (xy 0 0) (xy 1.06066 -1.06066)"), "{arc}");
    assert!(arc.contains("(width 0) (fill yes)"), "{arc}");
    let poly = footprint_block(&pcb, "REF2");
    assert!(poly.starts_with("\t(footprint \"muwave_polygon\"\n"), "{poly}");
    assert!(poly.contains("(fp_poly (pts (xy -3 0) (xy -3 -0.75)") && poly.contains("(stroke (width 0) (type solid)) (fill yes) (layer \"F.Cu\")"), "{poly}");
    assert!(poly.contains("(pad \"1\" smd rect (at -3 0 0)") && poly.contains("(pad \"2\" smd rect (at 3 0 0)"), "{poly}");
    let coil = footprint_block(&pcb, "REF3");
    assert!(coil.starts_with("\t(footprint \"mw_inductor\"\n") && coil.contains("(property \"Value\" \"L7\""), "{coil}");
    assert!(coil.contains("(at 17 16 0)"), "anchored at the end point: {coil}");
    assert!(coil.matches("(fp_line ").count() > 40 && coil.contains("(stroke (width 0.25) (type solid)) (layer \"F.Cu\")"), "{coil}");
    assert!(coil.contains("(pad \"1\" smd circle (at 0 0 0) (size 0.25 0.25) (layers \"F.Cu\")") && coil.contains("(pad \"2\" smd circle (at -5 0 0)"), "{coil}");
}

#[test]
fn an_inductor_that_cannot_be_made_says_why_in_kicads_words_and_changes_nothing() {
    let dir = scratch("mw_refused");
    setup(&dir);
    let before = serde_json::to_value(load(&dir).unwrap().1).unwrap();
    let e = step(&dir, Cmd::Microwave(MicrowaveCmd::Inductor { start: p(5_000_000, 10_000_000), end: p(5_000_000, 20_000_000), length_nm: mm(100.0), name: "L".into(), track_width_nm: mm(0.25) }), false, "test").unwrap_err();
    assert_eq!(super::reasons(&e), "Requested length too large");
    assert_eq!(serde_json::to_value(load(&dir).unwrap().1).unwrap(), before);
}

// ------------------------------------------------------------------------------------------------------------------------------ the slow tier

const SHAPE: &str = "Unit=MM\nXScale=6\nYScale=1.5\n\n$COORD\n0 0.5\n0.25 1\n0.75 1\n1 0.5\n$ENDCOORD\n";
/// A shape that is not the same seen from the right as from the left, so a flip shows.
const LEAN: &str = "Unit=MM\nXScale=4\nYScale=1\n\n$COORD\n0 0.2\n0.2 1\n1 0.6\n$ENDCOORD\n";

/// A tiny s-expression reader, to compare what kicad-cli writes with what this studio wrote without caring about layout.
#[derive(Debug, Clone, PartialEq)]
enum Node {
    Atom(String),
    Str(String),
    List(Vec<Node>),
}

fn parse_sexpr(text: &str) -> Node {
    let b: Vec<char> = text.chars().collect();
    let mut i = 0;
    fn go(b: &[char], i: &mut usize) -> Node {
        while b[*i].is_whitespace() {
            *i += 1;
        }
        if b[*i] == '(' {
            *i += 1;
            let mut out = Vec::new();
            loop {
                while b[*i].is_whitespace() {
                    *i += 1;
                }
                if b[*i] == ')' {
                    *i += 1;
                    return Node::List(out);
                }
                out.push(go(b, i));
            }
        } else if b[*i] == '"' {
            *i += 1;
            let mut s = String::new();
            while b[*i] != '"' {
                if b[*i] == '\\' {
                    *i += 1;
                }
                s.push(b[*i]);
                *i += 1;
            }
            *i += 1;
            Node::Str(s)
        } else {
            let from = *i;
            while !b[*i].is_whitespace() && b[*i] != '(' && b[*i] != ')' {
                *i += 1;
            }
            Node::Atom(b[from..*i].iter().collect())
        }
    }
    go(&b, &mut i)
}

impl Node {
    fn head(&self) -> Option<&str> {
        match self {
            Node::List(l) => match l.first() {
                Some(Node::Atom(a)) => Some(a),
                _ => None,
            },
            _ => None,
        }
    }
    fn children(&self) -> &[Node] {
        match self {
            Node::List(l) => l,
            _ => &[],
        }
    }
    /// The same tree without the parts that say nothing about the footprint's shape: ids, the transform KiCad writes, the properties it adds, and the layout words of a board file.
    fn normalized(&self) -> Node {
        match self {
            Node::Atom(a) => match a.parse::<f64>() {
                Ok(v) => Node::Atom(format!("{:.4}", if v == 0.0 { 0.0 } else { v })),
                Err(_) => Node::Atom(a.clone()),
            },
            Node::Str(s) => Node::Str(s.clone()),
            Node::List(l) => {
                let mut out: Vec<Node> = Vec::new();
                for c in l {
                    if matches!(c.head(), Some("uuid" | "tstamp" | "transform" | "embedded_fonts" | "duplicate_pad_numbers_are_jumpers" | "locked")) {
                        continue;
                    }
                    // the two properties KiCad adds to every footprint it writes
                    if c.head() == Some("property") && matches!(c.children().get(1), Some(Node::Str(n)) if n == "Datasheet" || n == "Description") {
                        continue;
                    }
                    out.push(c.normalized());
                }
                // `(at x y 0)` and `(at x y)` are one position, and -90 and 270 one angle
                if out.first() == Some(&Node::Atom("at".into())) && out.len() == 4 {
                    if let Node::Atom(a) = &out[3] {
                        let angle = a.parse::<f64>().unwrap_or(0.0).rem_euclid(360.0);
                        out[3] = Node::Atom(format!("{angle:.4}"));
                    }
                    if out[3] == Node::Atom("0.0000".into()) {
                        out.pop();
                    }
                }
                Node::List(out)
            }
        }
    }
}

/// The footprints of a board file keyed by their Reference, each as the normalized children that describe its shape (everything but the footprint's own place).
fn footprints_of(pcb: &str) -> std::collections::BTreeMap<String, Vec<Node>> {
    let root = parse_sexpr(pcb);
    let mut out = std::collections::BTreeMap::new();
    for fp in root.children().iter().filter(|c| c.head() == Some("footprint")) {
        let n = fp.normalized();
        let kids = n.children();
        let reference = kids
            .iter()
            .find_map(|c| match (c.head(), c.children().get(1), c.children().get(2)) {
                (Some("property"), Some(Node::Str(k)), Some(Node::Str(v))) if k == "Reference" => Some(v.clone()),
                _ => None,
            })
            .unwrap_or_default();
        // (KiCad writes a footprint's drawings in an order of its own, `FOOTPRINT::cmp_drawings`: the entries are compared as a set.)
        let mut shape: Vec<Node> = kids.iter().skip(1).filter(|c| c.head() != Some("at")).cloned().collect();
        shape.sort_by_key(|n| format!("{n:?}"));
        out.insert(reference, shape);
    }
    out
}

/// A kicad-cli SVG plot, in millimetres: the filled polygons (`Z`-closed paths), the stroked segments and the circles.
#[derive(Default)]
struct Plot {
    polygons: Vec<Vec<(f64, f64)>>,
    segments: Vec<((f64, f64), (f64, f64))>,
    circles: Vec<(f64, f64, f64)>,
    stroke_width: Option<f64>,
}

fn parse_plot(svg: &str) -> Plot {
    let mut plot = Plot::default();
    let mut rest = svg;
    while let Some(i) = rest.find("<path") {
        let tail = &rest[i..];
        let end = tail.find("/>").expect("a path ends");
        let tag = &tail[..end];
        rest = &tail[end + 2..];
        let Some(d_at) = tag.find("d=\"") else { continue };
        let d = &tag[d_at + 3..];
        let d = &d[..d.find('"').unwrap()];
        let closed = d.contains('Z');
        let nums: Vec<f64> = d.replace(['M', 'L', 'Z'], " ").replace(',', " ").split_whitespace().filter_map(|t| t.parse().ok()).collect();
        let pts: Vec<(f64, f64)> = nums.chunks(2).filter(|c| c.len() == 2).map(|c| (c[0], c[1])).collect();
        if closed {
            plot.polygons.push(pts);
        } else if pts.len() == 2 {
            plot.segments.push((pts[0], pts[1]));
        }
    }
    let mut rest = svg;
    while let Some(i) = rest.find("<circle") {
        let tail = &rest[i..];
        let end = tail.find("/>").unwrap();
        let tag = &tail[..end];
        rest = &tail[end + 2..];
        let attr = |name: &str| -> f64 {
            let at = tag.find(&format!("{name}=\"")).unwrap();
            let v = &tag[at + name.len() + 2..];
            v[..v.find('"').unwrap()].parse().unwrap()
        };
        plot.circles.push((attr("cx"), attr("cy"), attr("r")));
    }
    if let Some(i) = svg.find("stroke-width:") {
        plot.stroke_width = svg[i + 13..].split(';').next().and_then(|v| v.trim().parse().ok());
    }
    plot
}

const TOL: f64 = 0.0011;

fn near(a: (f64, f64), b: (f64, f64)) -> bool {
    (a.0 - b.0).abs() <= TOL && (a.1 - b.1).abs() <= TOL
}

/// Does `plot` hold a filled polygon whose corners are `want`, in any order?
fn has_polygon(plot: &Plot, want: &[(f64, f64)]) -> bool {
    plot.polygons.iter().any(|poly| poly.len() == want.len() && want.iter().all(|w| poly.iter().any(|q| near(*q, *w))))
}

fn rect(cx: f64, cy: f64, w: f64, h: f64) -> Vec<(f64, f64)> {
    vec![(cx - w / 2.0, cy - h / 2.0), (cx + w / 2.0, cy - h / 2.0), (cx + w / 2.0, cy + h / 2.0), (cx - w / 2.0, cy + h / 2.0)]
}

fn kicad_cli(work: &Path, args: &[&std::ffi::OsStr]) -> Result<String, String> {
    let cli = eda_kicad_engine::find_cli().ok_or("no kicad-cli")?;
    let out = eda_kicad_engine::cli_command(&cli, None).args(args).current_dir(work).output().map_err(|e| e.to_string())?;
    let said = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    if out.status.success() {
        Ok(said)
    } else {
        Err(said)
    }
}

fn plot_of(work: &Path, board: &Path, layer: &str, name: &str) -> Plot {
    let out = work.join(format!("{name}.svg"));
    kicad_cli(
        work,
        &["pcb".as_ref(), "export".as_ref(), "svg".as_ref(), "--mode-single".as_ref(), "-l".as_ref(), layer.as_ref(), "--page-size-mode".as_ref(), "2".as_ref(), "--exclude-drawing-sheet".as_ref(), "--black-and-white".as_ref(), "-o".as_ref(), out.as_os_str(), board.as_os_str()],
    )
    .unwrap_or_else(|e| panic!("kicad-cli export svg {name}: {e}"));
    parse_plot(&std::fs::read_to_string(&out).unwrap())
}

/// Slow tier (kicad-cli, `EDA_SLOW_TESTS=1`). The five tools on one board -- a gap, a stub turned a quarter, an arc stub, a polygonal shape, an inductor, and a lopsided polygon
/// turned over -- are loaded by kicad-cli and written back by it as it writes its own boards: every footprint has the same pads, graphics, texts and attributes, entry for entry.
/// And the plots of the copper layers put each where the tools' arithmetic does, worked out here from KiCad's formulas and not from the studio's code.
#[test]
fn kicad_cli_loads_and_plots_each_microwave_footprint_as_its_tool_makes_it() {
    if !slow() || eda_kicad_engine::find_cli().is_none() {
        return;
    }
    let dir = scratch("mw_kicad_cli_proof");
    setup(&dir);
    let mw = |c: MicrowaveCmd| step(&dir, Cmd::Microwave(c), false, "test").unwrap();
    mw(MicrowaveCmd::Gap { at: p(3_000_000, 11_000_000), size_nm: mm(0.5), track_width_nm: mm(0.25), hand: Hand::default() }); // REF1
    mw(MicrowaveCmd::Stub { at: p(17_000_000, 11_000_000), size_nm: mm(0.8), track_width_nm: mm(0.25), hand: Hand { rotate_millideg: 90_000, flip: None } }); // REF2
    mw(MicrowaveCmd::ArcStub { at: p(10_000_000, 11_000_000), radius_nm: mm(1.5), angle_millideg: 90_000, track_width_nm: mm(0.25), hand: Hand::default() }); // REF3
    mw(MicrowaveCmd::FunctionShape { at: p(6_000_000, 15_000_000), file: SHAPE.into(), kind: ShapeKind::Normal, size_x_nm: None, size_y_nm: None, track_width_nm: mm(0.25), hand: Hand::default() }); // REF4
    mw(MicrowaveCmd::Inductor { start: p(12_000_000, 17_000_000), end: p(17_000_000, 17_000_000), length_nm: mm(12.0), name: "L".into(), track_width_nm: mm(0.25) }); // REF5
    mw(MicrowaveCmd::FunctionShape { at: p(5_000_000, 18_500_000), file: LEAN.into(), kind: ShapeKind::Normal, size_x_nm: None, size_y_nm: None, track_width_nm: mm(0.25), hand: Hand { rotate_millideg: 0, flip: Some(FlipDirection::LeftRight) } }); // REF6, on the back

    let ours = derived_board(&dir);
    let work = dir.join("kicad");
    std::fs::create_dir_all(&work).unwrap();
    let (ours_path, theirs_path) = (work.join("ours.kicad_pcb"), work.join("theirs.kicad_pcb"));
    std::fs::write(&ours_path, &ours).unwrap();
    std::fs::write(&theirs_path, &ours).unwrap();
    kicad_cli(&work, &["pcb".as_ref(), "upgrade".as_ref(), theirs_path.as_os_str()]).expect("kicad-cli loads and writes the board");
    let theirs = std::fs::read_to_string(&theirs_path).unwrap();

    // 1. KiCad's own text of each footprint is ours, entry for entry
    let (a, b) = (footprints_of(&ours), footprints_of(&theirs));
    for r in ["REF1", "REF2", "REF3", "REF4", "REF5", "REF6"] {
        let (x, y) = (a.get(r).unwrap_or_else(|| panic!("{r} in ours")), b.get(r).unwrap_or_else(|| panic!("{r} in KiCad's rewrite")));
        assert_eq!(x.len(), y.len(), "{r}: the same entries\nours:   {x:?}\ntheirs: {y:?}");
        for (i, (xe, ye)) in x.iter().zip(y).enumerate() {
            assert_eq!(xe, ye, "{r}: entry {i}");
        }
    }

    // 2. the plot of the front copper
    let front = plot_of(&work, &ours_path, "F.Cu", "front");
    // the gap at (3, 11): two 0.25 mm squares, 0.5 mm apart
    assert!(has_polygon(&front, &rect(3.0 - 0.375, 11.0, 0.25, 0.25)) && has_polygon(&front, &rect(3.0 + 0.375, 11.0, 0.25, 0.25)), "the gap's two pads");
    // the stub at (17, 11) turned a quarter clockwise: the square pad, and the stub of 0.25 by 0.8 that was above it is now to its right: (0, -(0.8 + 0.25) / 2) -> (+0.525, 0)
    assert!(has_polygon(&front, &rect(17.0, 11.0, 0.25, 0.25)), "the stub's square pad");
    assert!(has_polygon(&front, &rect(17.0 + 0.525, 11.0, 0.8, 0.25)), "the stub lies along x after the turn: {:?}", front.polygons);
    // the arc stub at (10, 11): the points of the pie are on the outline of the pad's copper (the union with its 0.25 mm anchor)
    let arc_pad = front.polygons.iter().find(|poly| poly.iter().any(|q| near(*q, (10.0, 11.0 - 1.5))) || poly.iter().any(|q| near(*q, (10.0 + 1.061, 11.0 - 1.061)))).expect("the arc stub's pad is plotted");
    for (k, deg) in (0..19).map(|k| (k, -45.0 + 5.0 * k as f64)) {
        // `RotatePoint( (0, -r), theta )`: x = -r sin(theta), y = -r cos(theta), to the micrometre
        let r = 1.5_f64;
        let want = (10.0 - r * (deg as f64).to_radians().sin(), 11.0 - r * (deg as f64).to_radians().cos());
        assert!(arc_pad.iter().any(|q| (q.0 - want.0).abs() <= 0.0011 && (q.1 - want.1).abs() <= 0.0011), "arc point {k} ({deg} degrees) at {want:?} in {arc_pad:?}");
    }
    // the polygonal shape at (6, 15): XScale 6 mm, YScale 1.5 mm; the pads 6 mm apart at y = 15 and the polygon (0,.5) (.25,1) (.75,1) (1,.5) -> x = -3 + 6 u, y = -1.5 v
    assert!(has_polygon(&front, &rect(3.0, 15.0, 0.25, 0.25)) && has_polygon(&front, &rect(9.0, 15.0, 0.25, 0.25)), "the two pads");
    let shape: Vec<(f64, f64)> = vec![(3.0, 15.0), (3.0, 15.0 - 0.75), (4.5, 15.0 - 1.5), (7.5, 15.0 - 1.5), (9.0, 15.0 - 0.75), (9.0, 15.0)];
    assert!(shape.iter().all(|w| front.polygons.iter().any(|poly| poly.iter().any(|q| near(*q, *w)) && poly.len() >= 5)), "the filled polygon: {:?}", front.polygons);
    // the inductor from (12, 17) to (17, 17): pad 1 at the end, pad 2 at the start; its strokes are 0.25 wide and run from one to the other
    assert!(front.circles.iter().any(|c| near((c.0, c.1), (17.0, 17.0)) && (c.2 - 0.125).abs() < TOL), "pad 1: {:?}", front.circles);
    assert!(front.circles.iter().any(|c| near((c.0, c.1), (12.0, 17.0)) && (c.2 - 0.125).abs() < TOL), "pad 2");
    let coil: Vec<&((f64, f64), (f64, f64))> = front.segments.iter().filter(|s| s.0 .0 >= 11.0 && s.1 .0 <= 18.0 && s.0 .1 >= 14.0 && s.0 .1 <= 20.0).collect();
    assert!(coil.len() > 40, "a coil of chords: {}", coil.len());
    assert!(coil.iter().any(|s| near(s.0, (12.0, 17.0))) && coil.iter().any(|s| near(s.1, (17.0, 17.0))), "from the start to the end");
    assert_eq!(front.stroke_width, Some(0.25), "the strokes are the track's width");
    let length: f64 = coil.iter().map(|s| ((s.1 .0 - s.0 .0).powi(2) + (s.1 .1 - s.0 .1).powi(2)).sqrt()).sum();
    assert!((length - 12.0).abs() < 0.12, "the coil is {length} mm long for 12 mm asked");

    // 3. the lopsided polygon, turned over: on the back copper, its shape mirrored about the vertical line through its anchor
    let back = plot_of(&work, &ours_path, "B.Cu", "back");
    // LEAN at (5, 18.5): XScale 4, YScale 1: pads at 5 -/+ 2; polygon (0,.2) (.2,1) (1,.6) -> x = -2 + 4 u, y = -v, then mirrored: x -> -x
    let mirrored: Vec<(f64, f64)> = vec![(5.0 - -2.0, 18.5), (5.0 - -2.0, 18.5 - 0.2), (5.0 - -1.2, 18.5 - 1.0), (5.0 - 2.0, 18.5 - 0.6), (5.0 - 2.0, 18.5)];
    assert!(has_polygon(&back, &mirrored), "the flipped polygon on the back: {:?}", back.polygons);
    assert!(!front.polygons.iter().any(|poly| poly.iter().any(|q| near(*q, (7.0, 18.5)))), "and not on the front");
}

/// Copy and Paste (KiCad's clipboard text) bring a microwave footprint back as one: the text says copper only and no library, and the paste keeps it.
#[test]
fn a_pasted_microwave_footprint_is_copper_only_with_no_library_too() {
    let dir = scratch("mw_copy_paste");
    setup(&dir);
    step(&dir, gap(p(10_000_000, 12_000_000), 500_000), false, "test").unwrap();
    let (_, design, model) = load(&dir).unwrap();
    let text = eda_kicad::export_pcb_clipboard(&design, &model, &["REF1".to_string()], p(10_000_000, 12_000_000)).unwrap();
    assert!(text.contains("(footprint \"muwave_gap\"") && text.contains("(layers \"F.Cu\")"), "{text}");
    step(&dir, Cmd::PasteClipboard { text, at: p(14_000_000, 12_000_000) }, false, "test").unwrap();
    let (_, design, _) = load(&dir).unwrap();
    let extras = &design.drawings.as_ref().unwrap().footprint_extras;
    assert_eq!(extras.iter().map(|e| e.id.as_str()).collect::<Vec<_>>(), ["REF1", "REF2"], "the paste carries the facts under the copy's reference");
    let pcb = derived_board(&dir);
    let pasted = footprint_block(&pcb, "REF1");
    // (the paste's default leaves the reference text the clipboard had, `KEEP_ANNOTATIONS`: the block is found by that text)
    let copy = pcb.match_indices("\n\t(footprint \"muwave_gap\"").count();
    assert_eq!(copy, 2, "both are unlinked footprints: {pcb}");
    assert!(pasted.contains("(layers \"F.Cu\")") && !pasted.contains("F.Paste") && !pasted.contains("CrtYd"), "{pasted}");
}
