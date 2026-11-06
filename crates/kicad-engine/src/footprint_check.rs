//! The Footprint Checker (`DIALOG_FOOTPRINT_CHECKER`, pcbnew/dialogs/dialog_footprint_checker.cpp at KiCad 8303b2ad) as kicad-cli's DRC.
//!
//! KiCad's checker runs the footprint-level checks of `FOOTPRINT` (its courtyards, its attributes, its pads, the pads that touch, its net ties, the silkscreen the
//! mask clips) on the footprint held by the Footprint Editor and puts a marker where each fails. Those are the checks of the board DRC's footprint tests: here the
//! footprint being edited is put on a scratch board -- its `.kicad_mod` as the library writes it, placed at the origin on the front, inside an outline of its own --
//! and `kicad-cli pcb drc` runs on that, the one DRC engine; what the checker reports is what the board DRC says of those checks on that board, and nothing more is
//! made up here.
//!
//! What makes the board DRC the footprint checker's:
//!   * every pad number is a net of its own (pads that share a number share it: `PAD::SameLogicalPadAs`), so two pads of different numbers that touch are the
//!     "items shorting two nets" the checker's `CheckShortingPads` finds without knowing nets;
//!   * the severities are the Footprint Editor's own (`PCB_EDIT_FRAME`'s `Clear_Pcb` in initpcb.cpp: all errors, the holes, the padstack, the type mismatch and
//!     (`LoadFootprintFromBoard`) the missing courtyard warnings), and two checks the board leaves ignored -- the missing courtyard and the type mismatch -- run;
//!   * two drilled holes are a finding when they overlap (`holeA->Collide( holeB->GetSeg(), 0 )`), not when they are closer than the board's 0.25 mm (the minimum hole to
//!     hole is 0.1 um here: no smaller value keeps the hole test on).
//!
//! A report is the violations of those checks only: the board DRC's clearances, edge, mask and parity tests have nothing to say about a footprint alone.

use super::{cli_version, fail, need_cli, run_report, with_scratch, work_dir, write_file, Violation};
use eda_model::ir::{LibraryFootprint, Shape, Nm};
use eda_model::units::mm;
use eda_model::CheckResult;
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashMap};

/// The checks of the Footprint Checker, as the board DRC names them (`DRC_ITEM`'s settings keys), with the severity the Footprint Editor gives them.
pub const CHECKS: &[(&str, &str)] = &[
    ("malformed_courtyard", "error"),
    ("missing_courtyard", "warning"),
    ("footprint_type_mismatch", "warning"),
    ("through_hole_pad_without_hole", "error"),
    ("padstack", "warning"),
    ("padstack_invalid", "error"),
    ("holes_co_located", "warning"),
    ("hole_to_hole", "warning"),
    ("shorting_items", "error"),
    ("footprint", "error"),
    ("silk_over_copper", "error"),
];

/// Is `kind` (a violation's `type`) one of the Footprint Checker's?
pub fn is_checker_kind(kind: &str) -> bool {
    CHECKS.iter().any(|(k, _)| *k == kind)
}

/// The scratch board of a footprint and which of its items each uuid is.
#[derive(Debug, Clone)]
pub struct Scratch {
    /// The `.kicad_pcb`.
    pub pcb: String,
    /// The board's project: the Footprint Editor's severities.
    pub project: String,
    /// The footprint's items by the uuid the scratch board gives them: a pad's, a graphic's, a text's id in the editor.
    pub ids: HashMap<String, String>,
}

/// The layers of a board as KiCad writes a new one's (`PCB_IO_KICAD_SEXPR::formatBoardLayers` on two copper layers and the standard technical layers).
const LAYERS: &str = "\t(layers\n\t\t(0 \"F.Cu\" signal)\n\t\t(31 \"B.Cu\" signal)\n\t\t(32 \"B.Adhes\" user \"B.Adhesive\")\n\t\t(33 \"F.Adhes\" user \"F.Adhesive\")\n\t\t(34 \"B.Paste\" user)\n\t\t(35 \"F.Paste\" user)\n\t\t(36 \"B.SilkS\" user \"B.Silkscreen\")\n\t\t(37 \"F.SilkS\" user \"F.Silkscreen\")\n\t\t(38 \"B.Mask\" user)\n\t\t(39 \"F.Mask\" user)\n\t\t(40 \"Dwgs.User\" user \"User.Drawings\")\n\t\t(41 \"Cmts.User\" user \"User.Comments\")\n\t\t(42 \"Eco1.User\" user \"User.Eco1\")\n\t\t(43 \"Eco2.User\" user \"User.Eco2\")\n\t\t(44 \"Edge.Cuts\" user)\n\t\t(45 \"Margin\" user)\n\t\t(46 \"B.CrtYd\" user \"B.Courtyard\")\n\t\t(47 \"F.CrtYd\" user \"F.Courtyard\")\n\t\t(48 \"B.Fab\" user)\n\t\t(49 \"F.Fab\" user)\n\t)\n";

/// A uuid in the form KiCad writes, from a number.
fn uuid_of(n: usize) -> String {
    format!("00000000-0000-4000-8000-{n:012x}")
}

/// Millimetres as KiCad's files write them.
fn mm_text(nm: Nm) -> String {
    let s = format!("{:.6}", nm.mm());
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s.is_empty() || s == "-" || s == "-0" {
        "0".to_string()
    } else {
        s.to_string()
    }
}

/// One child of the root form of an s-expression text: an atom, or a form with the byte range of its text.
enum Child<'a> {
    Atom(&'a str),
    Form(&'a str),
}

/// The root `( ... )` of `text` and its children, in order. Strings and `#` comments are skipped over, nothing else is read.
fn root_children(text: &str) -> Option<Vec<Child<'_>>> {
    let b = text.as_bytes();
    let open = text.find('(')?;
    let mut i = open + 1;
    let mut out = Vec::new();
    while i < b.len() {
        match b[i] {
            b' ' | b'\t' | b'\r' | b'\n' => i += 1,
            b')' => return Some(out),
            b'(' => {
                let start = i;
                let (mut depth, mut j) = (0usize, i);
                while j < b.len() {
                    match b[j] {
                        b'"' => {
                            j += 1;
                            while j < b.len() && b[j] != b'"' {
                                if b[j] == b'\\' {
                                    j += 1;
                                }
                                j += 1;
                            }
                        }
                        b'(' => depth += 1,
                        b')' => {
                            depth -= 1;
                            if depth == 0 {
                                break;
                            }
                        }
                        _ => {}
                    }
                    j += 1;
                }
                out.push(Child::Form(&text[start..(j + 1).min(text.len())]));
                i = j + 1;
            }
            b'"' => {
                let start = i;
                i += 1;
                while i < b.len() && b[i] != b'"' {
                    if b[i] == b'\\' {
                        i += 1;
                    }
                    i += 1;
                }
                out.push(Child::Atom(&text[start..(i + 1).min(text.len())]));
                i += 1;
            }
            _ => {
                let start = i;
                while i < b.len() && !matches!(b[i], b' ' | b'\t' | b'\r' | b'\n' | b'(' | b')') {
                    i += 1;
                }
                out.push(Child::Atom(&text[start..i]));
            }
        }
    }
    None
}

/// The head of a form: `pad` for `(pad "1" ...)`.
fn head(form: &str) -> &str {
    let inner = &form[1..];
    let end = inner.find(|c: char| c.is_whitespace() || c == '(' || c == ')').unwrap_or(inner.len());
    &inner[..end]
}

/// `form` with its own `(uuid ..)` (a direct child, not one inside a nested form) replaced by `uuid`, and `extra` put before its closing parenthesis.
fn rewrite_form(form: &str, uuid: &str, extra: &str) -> String {
    let Some(children) = root_children(form) else { return form.to_string() };
    let mut out = String::from("(");
    out.push_str(head(form));
    let mut first = true;
    for c in children {
        // the head atom is already written
        if first {
            first = false;
            continue;
        }
        out.push(' ');
        match c {
            Child::Atom(a) => out.push_str(a),
            Child::Form(f) if head(f) == "uuid" => out.push_str(&format!("(uuid \"{uuid}\")")),
            Child::Form(f) => out.push_str(f),
        }
    }
    out.push_str(extra);
    out.push(')');
    out
}

fn shape_extent(s: &Shape) -> Nm {
    s.points().iter().map(|p| p.x.abs().max(p.y.abs())).max().unwrap_or(Nm::ZERO) + s.stroke_width()
}

/// How far from the origin anything of the footprint reaches, in nanometres: what the scratch board's outline has to keep clear of.
fn reach(fp: &LibraryFootprint) -> Nm {
    let pads = fp.pads.iter().map(|p| p.at.x.abs().max(p.at.y.abs()) + p.size.0.max(p.size.1) + p.offset.x.abs().max(p.offset.y.abs()));
    let graphics = fp.graphics.iter().map(shape_extent);
    let texts = fp.texts.iter().map(|t| t.at.x.abs().max(t.at.y.abs()) + t.size_nm * (t.content.chars().count() as i64 + 1));
    pads.chain(graphics).chain(texts).max().unwrap_or(Nm::ZERO)
}

/// The Footprint Editor's severities as a `.kicad_pro`: the checks of [`CHECKS`] at the severity the editor gives them, and the board's minimum hole to hole at 0.1 um: two
/// holes are a finding when they overlap or touch, as the checker's `Collide( .., 0 )` has it. (A minimum of 0 would switch the hole tests off: `DRC_TEST_PROVIDER_HOLE_TO_HOLE`
/// does not run without a constraint above zero.)
pub fn project_text() -> String {
    let severities: BTreeMap<&str, &str> = CHECKS.iter().copied().collect();
    let project = json!({
        "board": { "design_settings": { "rule_severities": severities, "rules": { "min_hole_to_hole": 0.0001 } } },
        "meta": { "filename": "footprint_check.kicad_pro", "version": 3 },
    });
    format!("{}\n", serde_json::to_string_pretty(&project).unwrap_or_default())
}

/// The scratch board of `fp`: the footprint at the origin on the front, a net per pad number, an outline 10 mm outside everything it has.
pub fn scratch_board(fp: &LibraryFootprint) -> Scratch {
    let text = eda_kicad::export_kicad_mod(fp);
    let children = root_children(&text).unwrap_or_default();
    let nets: BTreeMap<&str, usize> = {
        let mut numbers: Vec<&str> = fp.pads.iter().map(|p| p.number.as_str()).filter(|n| !n.is_empty()).collect();
        numbers.sort_unstable();
        numbers.dedup();
        numbers.into_iter().enumerate().map(|(i, n)| (n, i + 1)).collect()
    };
    let (mut graphic, mut text_i, mut pad_i) = (0usize, 0usize, 0usize);
    let mut ids: HashMap<String, String> = HashMap::new();
    let mut next = 1usize;
    let mut body = String::new();
    let mut name = String::from("\"footprint\"");
    // (the first atom is the head of the form, the second the footprint's name)
    let mut atoms = 0usize;
    for c in &children {
        match c {
            Child::Atom(a) => {
                atoms += 1;
                if atoms == 2 {
                    name = a.to_string();
                }
            }
            Child::Form(f) => match head(f) {
                // a footprint of a board has neither
                "version" | "generator" | "generator_version" | "layer" => {}
                // The footprint's fields (the Reference, the Value, the user fields) are not the checker's: `FOOTPRINT::CheckClippedSilk` tests the drawings
                // (`m_drawings`: the graphics and the texts) against each other and the pads, never `m_fields`, while the board DRC's silk-to-mask test takes
                // every visible field -- a new footprint's REF** (`CreateNewFootprint`'s, at (0, -0.5 mm) on the silkscreen) would be "clipped by solder mask"
                // over any pad near the origin. They are left off; the one Reference below is hidden.
                "property" => {}
                "fp_line" | "fp_arc" | "fp_rect" | "fp_circle" | "fp_poly" | "fp_curve" => {
                    let uuid = uuid_of(next);
                    next += 1;
                    if let Some(g) = fp.graphics.get(graphic) {
                        ids.insert(uuid.clone(), g.id().to_string());
                    }
                    graphic += 1;
                    body.push_str(&format!("\t\t{}\n", rewrite_form(f, &uuid, "")));
                }
                "fp_text" => {
                    let uuid = uuid_of(next);
                    next += 1;
                    if let Some(t) = fp.texts.get(text_i) {
                        ids.insert(uuid.clone(), t.id.clone());
                    }
                    text_i += 1;
                    body.push_str(&format!("\t\t{}\n", rewrite_form(f, &uuid, "")));
                }
                "pad" => {
                    let uuid = uuid_of(next);
                    next += 1;
                    let pad = fp.pads.get(pad_i);
                    if let Some(p) = pad {
                        ids.insert(uuid.clone(), p.id.clone());
                    }
                    pad_i += 1;
                    let net = pad.and_then(|p| nets.get(p.number.as_str()).map(|n| format!(" (net {n} \"{}\")", p.number.replace('\\', "\\\\").replace('"', "\\\""))));
                    body.push_str(&format!("\t\t{}\n", rewrite_form(f, &uuid, net.as_deref().unwrap_or(""))));
                }
                _ => body.push_str(&format!("\t\t{f}\n")),
            },
        }
    }
    // The footprint's own Reference and Value as the Footprint Editor holds them (`REF**` and the name of a library footprint, the part's reference and value of one loaded
    // from the board: UI-LEFT F9), so that what the checks say of the footprint names it as KiCad's checker does ("Footprint R1", "Pad 1 [1] of R1"). Both hidden: the
    // checker's tests do not look at the fields (`CheckClippedSilk` walks `m_drawings`, never `m_fields`), and a shown one on the silkscreen would be clipped by the
    // board DRC's silk test over a pad near the origin.
    let quote = |t: &str| t.replace('\\', "\\\\").replace('"', "\\\"");
    let reference = fp.reference.as_ref().and_then(|l| l.text.clone()).unwrap_or_else(|| eda_model::fp_edit::LIBRARY_REFERENCE_TEXT.to_string());
    let value = fp.value.as_ref().and_then(|l| l.text.clone()).unwrap_or_else(|| fp.name.rsplit(':').next().unwrap_or(&fp.name).to_string());
    body.insert_str(
        0,
        &format!(
            "\t\t(property \"Reference\" \"{}\" (at 0 0 0) (layer \"F.SilkS\") (hide yes) (uuid \"{}\") (effects (font (size 1 1) (thickness 0.15))))\n\t\t(property \"Value\" \"{}\" (at 0 0 0) (layer \"F.Fab\") (hide yes) (uuid \"{}\") (effects (font (size 1 1) (thickness 0.15))))\n",
            quote(&reference),
            uuid_of(next),
            quote(&value),
            uuid_of(next + 1)
        ),
    );
    next += 2;
    let r = reach(fp) + mm(10.0);
    let mut pcb = String::from("(kicad_pcb\n\t(version 20241229)\n\t(generator \"eda-kicad\")\n\t(generator_version \"9.0\")\n\t(general\n\t\t(thickness 1.6)\n\t\t(legacy_teardrops no)\n\t)\n\t(paper \"A3\")\n");
    pcb.push_str(LAYERS);
    pcb.push_str("\t(setup\n\t\t(pad_to_mask_clearance 0)\n\t)\n\t(net 0 \"\")\n");
    for (number, n) in &nets {
        pcb.push_str(&format!("\t(net {n} \"{}\")\n", number.replace('\\', "\\\\").replace('"', "\\\"")));
    }
    pcb.push_str(&format!("\t(footprint {name}\n\t\t(layer \"F.Cu\")\n\t\t(uuid \"{}\")\n\t\t(at 0 0)\n{body}\t)\n", uuid_of(0)));
    pcb.push_str(&format!(
        "\t(gr_rect (start {} {}) (end {} {}) (stroke (width 0.05) (type default)) (fill no) (layer \"Edge.Cuts\") (uuid \"{}\"))\n)\n",
        mm_text(-r),
        mm_text(-r),
        mm_text(r),
        mm_text(r),
        uuid_of(next)
    ));
    Scratch { pcb, project: project_text(), ids }
}

/// What the Footprint Checker found.
#[derive(Debug, Clone)]
pub struct FootprintCheck {
    /// "kicad-cli 10.99.0".
    pub engine: String,
    /// The violations of [`CHECKS`] on the scratch board, in the order kicad-cli reported them.
    pub violations: Vec<Violation>,
}

/// Where a violation's marker goes (nanometres, the footprint's own frame) when nothing more is known of it than kicad-cli's report says: its first item's position,
/// the middle of the first two when there are two, the origin for a violation that names the footprint alone. [`marker_in`] places the checker's markers as
/// `DIALOG_FOOTPRINT_CHECKER::runChecks` does.
pub fn marker_at(v: &Violation) -> (Nm, Nm) {
    match v.items.as_slice() {
        [] => (Nm::ZERO, Nm::ZERO),
        [a] => (a.pos.x, a.pos.y),
        [a, b, ..] => ((a.pos.x + b.pos.x) / 2, (a.pos.y + b.pos.y) / 2),
    }
}

/// Where `DIALOG_FOOTPRINT_CHECKER::runChecks` puts the marker of `v` on the footprint `fp` (UI-LEFT F9; kicad-cli's report gives the items' positions, not the marker's):
/// - the footprint's own findings (no courtyard, the type, a net tie's pad groups) at `{ 0, 0 }`;
/// - a pad's (`CheckPads`) at the pad (`aPad->GetPosition()`), and the two hole checks (`CheckShortingPads`: `pos = pad->GetPosition()`) at the first of the two pads
///   in the footprint's order;
/// - two pads that short (`padShape->Collide( otherShape, 0, nullptr, &pos )`) where they meet: for two pads whose shape is a polygon (a rectangle, a trapezoid) KiCad's
///   `Collide( SHAPE_LINE_CHAIN_BASE, SHAPE_LINE_CHAIN_BASE )` location ([`chain_contact`]); for other shapes the middle of the overlap of their boxes;
/// - silkscreen clipped by the mask (`CheckClippedSilk`'s `Collide` location) at the middle of the overlap of the drawing's and the pad's boxes;
/// - anything else as [`marker_at`].
pub fn marker_in(fp: &LibraryFootprint, v: &Violation) -> (Nm, Nm) {
    let pad_index = |id: &Option<String>| id.as_ref().and_then(|id| fp.pads.iter().position(|p| &p.id == id));
    let mut pads: Vec<usize> = v.items.iter().filter_map(|it| pad_index(&it.id)).collect();
    pads.sort_unstable();
    match v.kind.as_str() {
        "missing_courtyard" | "footprint_type_mismatch" | "footprint" => (Nm::ZERO, Nm::ZERO),
        "padstack" | "padstack_invalid" | "through_hole_pad_without_hole" | "holes_co_located" | "hole_to_hole" if !pads.is_empty() => {
            let p = &fp.pads[pads[0]];
            (p.at.x, p.at.y)
        }
        "shorting_items" if pads.len() >= 2 => {
            let (a, b) = (&fp.pads[pads[0]], &fp.pads[pads[1]]);
            match (pad_outline(a), pad_outline(b)) {
                (Some(pa), Some(pb)) => chain_contact(&pa, &pb).map(|(x, y)| (Nm::new(x), Nm::new(y))).unwrap_or_else(|| box_overlap_centre(pad_box(a), pad_box(b))),
                _ => box_overlap_centre(pad_box(a), pad_box(b)),
            }
        }
        "silk_over_copper" if pads.len() == 1 => {
            let graphic = v.items.iter().find_map(|it| it.id.as_ref().and_then(|id| fp.graphics.iter().find(|g| g.id() == id)));
            match graphic {
                Some(g) => box_overlap_centre(shape_box(g), pad_box(&fp.pads[pads[0]])),
                None => marker_at(v),
            }
        }
        _ => marker_at(v),
    }
}

type Box2 = (i64, i64, i64, i64);

/// The middle of the overlap of two boxes (of the nearer edges when they do not overlap).
fn box_overlap_centre(a: Box2, b: Box2) -> (Nm, Nm) {
    let (x0, y0, x1, y1) = (a.0.max(b.0), a.1.max(b.1), a.2.min(b.2), a.3.min(b.3));
    (Nm::new((x0 + x1) / 2), Nm::new((y0 + y1) / 2))
}

/// `RotatePoint( x, y, angle )`: KiCad's turn of a point, positive counter-clockwise on the screen (y down).
fn rotate(p: (f64, f64), deg: f64) -> (f64, f64) {
    let (s, c) = deg.to_radians().sin_cos();
    (p.0 * c + p.1 * s, p.1 * c - p.0 * s)
}

/// The pad's orientation as KiCad keeps it (`GetOrientation()`, counter-clockwise; the file's angle).
fn pad_orientation_deg(p: &eda_model::ir::LibraryPad) -> f64 {
    -(p.rot as f64) / 1000.0
}

/// `PAD::ShapePos`: the copper's centre, the pad's position with its offset turned with it.
fn shape_pos(p: &eda_model::ir::LibraryPad) -> (f64, f64) {
    let o = rotate((p.offset.x.get() as f64, p.offset.y.get() as f64), pad_orientation_deg(p));
    (p.at.x.get() as f64 + o.0, p.at.y.get() as f64 + o.1)
}

/// `PAD::buildEffectiveShape` of a rectangle or a trapezoid (no corner radius): the four corners in its order, turned and moved -- and, when they make a rectangle on the
/// axes, `SHAPE_RECT`'s outline instead (`TransformToPolygon`: the top-left corner, then clockwise on the screen). `None` for any other shape.
fn pad_outline(p: &eda_model::ir::LibraryPad) -> Option<Vec<(i64, i64)>> {
    use eda_model::ir::LibraryPadShape;
    let (hx, hy) = (p.size.0.get() / 2, p.size.1.get() / 2);
    let (dx, dy) = match p.shape {
        LibraryPadShape::Rect => (0, 0),
        LibraryPadShape::Trapezoid => p.trapezoid_delta.map(|(x, y)| (x.get() / 2, y.get() / 2)).unwrap_or((0, 0)),
        _ => return None,
    };
    let corners = [(-hx - dy, hy + dx), (hx + dy, hy - dx), (hx - dy, -hy + dx), (-hx + dy, -hy - dx)];
    let deg = pad_orientation_deg(p);
    let c = shape_pos(p);
    let pts: Vec<(i64, i64)> = corners
        .iter()
        .map(|&(x, y)| {
            let r = rotate((x as f64, y as f64), deg);
            ((r.0 + c.0).round() as i64, (r.1 + c.1).round() as i64)
        })
        .collect();
    let axis_rect = (pts[0].1 == pts[1].1 && pts[1].0 == pts[2].0 && pts[2].1 == pts[3].1 && pts[3].0 == pts[0].0) || (pts[0].0 == pts[1].0 && pts[1].1 == pts[2].1 && pts[2].0 == pts[3].0 && pts[3].1 == pts[0].1);
    if axis_rect {
        let (x0, y0) = (pts[0].0.min(pts[2].0), pts[0].1.min(pts[2].1));
        let (w, h) = ((pts[2].0 - pts[0].0).abs(), (pts[2].1 - pts[0].1).abs());
        return Some(vec![(x0, y0), (x0 + w, y0), (x0 + w, y0 + h), (x0, y0 + h)]);
    }
    Some(pts)
}

/// The box of a pad's copper (its corners for a polygon, else its size about its centre, turned).
fn pad_box(p: &eda_model::ir::LibraryPad) -> Box2 {
    let pts: Vec<(i64, i64)> = pad_outline(p).unwrap_or_else(|| {
        let (hx, hy) = (p.size.0.get() as f64 / 2.0, p.size.1.get() as f64 / 2.0);
        let c = shape_pos(p);
        [(-hx, -hy), (hx, -hy), (hx, hy), (-hx, hy)].iter().map(|&q| rotate(q, pad_orientation_deg(p))).map(|(x, y)| ((x + c.0).round() as i64, (y + c.1).round() as i64)).collect()
    });
    let xs = pts.iter().map(|q| q.0);
    let ys = pts.iter().map(|q| q.1);
    (xs.clone().min().unwrap_or(0), ys.clone().min().unwrap_or(0), xs.max().unwrap_or(0), ys.max().unwrap_or(0))
}

/// The box of a drawing, its stroke counted.
fn shape_box(s: &Shape) -> Box2 {
    let half = s.stroke_width().get() / 2;
    let pts = s.points();
    let xs = pts.iter().map(|q| q.x.get());
    let ys = pts.iter().map(|q| q.y.get());
    (xs.clone().min().unwrap_or(0) - half, ys.clone().min().unwrap_or(0) - half, xs.max().unwrap_or(0) + half, ys.max().unwrap_or(0) + half)
}

/// `SHAPE_LINE_CHAIN_BASE::PointInside( aPt, 0 )`: the ray to the right crosses an odd number of edges (`rescale( diff.x, aPt.y - p1.y, diff.y )`).
fn point_inside(poly: &[(i64, i64)], pt: (i64, i64)) -> bool {
    if poly.len() < 3 {
        return false;
    }
    let mut inside = false;
    for i in 0..poly.len() {
        let p1 = poly[i];
        let p2 = poly[(i + 1) % poly.len()];
        let diff = (p2.0 - p1.0, p2.1 - p1.1);
        if diff.1 == 0 {
            continue;
        }
        // `rescale( a, b, c )`: a * b / c rounded to the nearest
        let d = ((diff.0 as i128 * (pt.1 - p1.1) as i128) as f64 / diff.1 as f64).round() as i64;
        if (p1.1 >= pt.1) != (p2.1 >= pt.1) && pt.0 - p1.0 < d {
            inside = !inside;
        }
    }
    inside
}

/// `rescale( a, b, c )`: `a * b / c`, rounded.
fn rescale(a: i128, b: i128, c: i128) -> i128 {
    ((a * b) as f64 / c as f64).round() as i128
}

/// `SEG::Intersect( aSeg )` (`SEG::intersects`, libs/kimath/src/geometry/seg.cpp at 8303b2ad): where segment `a` meets segment `b`, their ends included; two collinear
/// segments that overlap meet at the middle of the overlap (`checkCollinearOverlap`, measured along the axis `a` runs most along).
fn seg_intersection(a: ((i64, i64), (i64, i64)), b: ((i64, i64), (i64, i64))) -> Option<(i64, i64)> {
    let ((ax, ay), (bx, by)) = a;
    let ((cx, cy), (dx_, dy_)) = b;
    if ax.max(bx) < cx.min(dx_) || cx.max(dx_) < ax.min(bx) || ay.max(by) < cy.min(dy_) || cy.max(dy_) < ay.min(by) {
        return None;
    }
    let cross = |u: (i128, i128), v: (i128, i128)| u.0 * v.1 - u.1 * v.0;
    let dir1 = ((bx - ax) as i128, (by - ay) as i128);
    let dir2 = ((dx_ - cx) as i128, (dy_ - cy) as i128);
    let offset = ((cx - ax) as i128, (cy - ay) as i128);
    let det = cross(dir2, dir1);
    if det == 0 {
        if cross(dir1, offset) != 0 {
            return None;
        }
        // collinear: the middle of the overlap on the axis `a` runs most along, the other coordinate on `a`'s line
        let use_x = dir1.0.abs() >= dir1.1.abs();
        let (s1, e1, s2, e2, o1, oe) = if use_x { (ax, bx, cx, dx_, ay, by) } else { (ay, by, cy, dy_, ax, bx) };
        let (min1, max1, min2, max2) = (s1.min(e1), s1.max(e1), s2.min(e2), s2.max(e2));
        if !(max1 >= min2 && max2 >= min1) {
            return None;
        }
        let proj = (min1.max(min2) + max1.min(max2)) / 2;
        let other = if e1 != s1 { o1 + rescale((proj - s1) as i128, (oe - o1) as i128, (e1 - s1) as i128) as i64 } else { o1 };
        return Some(if use_x { (proj, other) } else { (other, proj) });
    }
    let p2 = cross(dir2, offset);
    let p1 = cross(dir1, offset);
    let outside = if det > 0 { p1 < 0 || p1 > det || p2 < 0 || p2 > det } else { p1 > 0 || p1 < det || p2 > 0 || p2 < det };
    if outside {
        return None;
    }
    Some(((cx as i128 + rescale(p1, dir2.0, det)) as i64, (cy as i128 + rescale(p1, dir2.1, det)) as i64))
}

/// `Collide( const SHAPE_LINE_CHAIN_BASE& aA, const SHAPE_LINE_CHAIN_BASE& aB, 0, nullptr, &location )` (libs/kimath/src/geometry/shape_collisions.cpp): `aA`'s first point
/// when `aB` holds it, else `aB`'s first point when `aA` holds it, else where the first of `aA`'s edges (sorted by their start, x then y) crosses the first of `aB`'s that it
/// crosses. `None` when the outlines do not touch.
pub fn chain_contact(a: &[(i64, i64)], b: &[(i64, i64)]) -> Option<(i64, i64)> {
    if !a.is_empty() && point_inside(b, a[0]) {
        return Some(a[0]);
    }
    if !b.is_empty() && point_inside(a, b[0]) {
        return Some(b[0]);
    }
    let segs = |poly: &[(i64, i64)]| {
        let mut v: Vec<((i64, i64), (i64, i64))> = (0..poly.len()).map(|i| (poly[i], poly[(i + 1) % poly.len()])).collect();
        v.sort_by(|x, y| (x.0 .0, x.0 .1).cmp(&(y.0 .0, y.0 .1)));
        v
    };
    let (sa, sb) = (segs(a), segs(b));
    for x in &sa {
        for y in &sb {
            if let Some(p) = seg_intersection(*x, *y) {
                return Some(p);
            }
        }
    }
    None
}

impl FootprintCheck {
    /// `{ ok, engine, markers: [{ type, description, severity, at: [x, y], items: [{ description, pos: [x, y], id }] }], counts: { errors, warnings } }`, each marker where
    /// `runChecks` puts it on `fp` ([`marker_in`]).
    pub fn to_json_for(&self, fp: &LibraryFootprint) -> Value {
        self.json_with(|v| marker_in(fp, v))
    }

    /// [`Self::to_json_for`] without the footprint: the markers by [`marker_at`].
    pub fn to_json(&self) -> Value {
        self.json_with(marker_at)
    }

    fn json_with(&self, place: impl Fn(&Violation) -> (Nm, Nm)) -> Value {
        let markers: Vec<Value> = self
            .violations
            .iter()
            .map(|v| {
                let at = place(v);
                json!({
                    "type": v.kind,
                    "description": v.description,
                    "severity": v.severity,
                    "at": [at.0, at.1],
                    "items": v.items.iter().map(|i| json!({ "description": i.description, "pos": [i.pos.x, i.pos.y], "id": i.id })).collect::<Vec<_>>(),
                })
            })
            .collect();
        let count = |s: &str| self.violations.iter().filter(|v| v.severity == s).count();
        json!({ "ok": true, "engine": self.engine, "markers": markers, "counts": { "errors": count("error"), "warnings": count("warning") } })
    }
}

/// kicad-cli's DRC on `scratch`, the violations of the checker's checks.
pub fn run_scratch(scratch: &Scratch) -> Result<FootprintCheck, Vec<CheckResult>> {
    let cli = need_cli()?;
    with_scratch(|dir| {
        work_dir(dir)?;
        let pcb = dir.join("footprint_check.kicad_pcb");
        write_file(&pcb, &scratch.pcb)?;
        write_file(&dir.join("footprint_check.kicad_pro"), &scratch.project)?;
        let report = dir.join("footprint_check.drc.json");
        // The scratch project names only the checker's checks, so the run needs no library table (`cli_command`).
        let mut cmd = super::cli_command(&cli, None);
        cmd.args(["pcb", "drc", "--format", "json", "--severity-all", "--units", "mm", "-o"]).arg(&report).arg(&pcb);
        let raw = run_report(cmd, &report, "footprint_check")?;
        let units_mm = raw["coordinate_units"].as_str().unwrap_or("mm") == "mm";
        let violations: Vec<Violation> = raw["violations"]
            .as_array()
            .map(|vs| vs.iter().map(|v| Violation::from_json(v, &scratch.ids, units_mm)).filter(|v| is_checker_kind(&v.kind) && !v.excluded).collect())
            .unwrap_or_default();
        Ok(FootprintCheck { engine: format!("kicad-cli {}", raw["kicad_version"].as_str().map(str::to_string).unwrap_or_else(|| cli_version(&cli))), violations })
    })
}

/// `FOOTPRINT_CHECKER`: the footprint `fp` as the Footprint Editor holds it, checked.
pub fn footprint_check(fp: &LibraryFootprint) -> Result<FootprintCheck, Vec<CheckResult>> {
    if fp.name.is_empty() {
        return Err(fail("footprint_check", "footprint", "the footprint has no name"));
    }
    run_scratch(&scratch_board(fp))
}

#[cfg(test)]
mod tests {
    use super::*;
    use eda_model::ir::{FootprintAttributes, LibraryPad, LibraryPadShape, Point};
    use eda_model::PadKind;

    fn pad(id: &str, number: &str, x: Nm, y: Nm, kind: PadKind, drill: Option<Nm>) -> LibraryPad {
        LibraryPad {
            id: id.into(),
            number: number.into(),
            at: Point { x, y },
            size: (mm(1.0), mm(1.0)),
            offset: Point { x: Nm::ZERO, y: Nm::ZERO },
            shape: LibraryPadShape::Rect,
            kind,
            drill,
            drill_slot: None,
            rot: 0,
            roundrect_ratio: None,
            trapezoid_delta: None,
            chamfer_ratio: None,
            chamfer_corners: Default::default(),
            custom: None,
            layers: Vec::new(),
            clearance_override: None,
            thermal_gap_override: None,
            thermal_spoke_width_override: None,
            zone_connection: None,
            thermal_spoke_angle_mdeg: None,
            stack: Default::default(),
            kicad: Default::default(),
        }
    }

    fn footprint(pads: Vec<LibraryPad>) -> LibraryFootprint {
        LibraryFootprint { name: "TEST:Two".into(), pads, attributes: FootprintAttributes::default(), ..Default::default() }
    }

    fn line(id: &str, layer: &str, a: (Nm, Nm), b: (Nm, Nm)) -> Shape {
        Shape::Segment { id: id.into(), layer: layer.into(), stroke_width: mm(0.05), filled: false, start: Point { x: a.0, y: a.1 }, end: Point { x: b.0, y: b.1 } }
    }

    #[test]
    fn the_scratch_board_has_the_footprint_a_net_per_pad_number_and_an_outline() {
        let mut fp = footprint(vec![pad("p1", "1", mm(-1.0), Nm::ZERO, PadKind::Smd, None), pad("p2", "2", mm(1.0), Nm::ZERO, PadKind::Smd, None), pad("p3", "2", mm(3.0), Nm::ZERO, PadKind::Smd, None), pad("p4", "", mm(5.0), Nm::ZERO, PadKind::NonPlatedHole, Some(mm(0.8)))]);
        fp.graphics = vec![line("g1", "F.CrtYd", (mm(-2.0), mm(-1.0)), (mm(2.0), mm(-1.0)))];
        let s = scratch_board(&fp);
        assert!(s.pcb.starts_with("(kicad_pcb\n"), "{}", &s.pcb[..40]);
        assert!(s.pcb.contains("(net 1 \"1\")") && s.pcb.contains("(net 2 \"2\")") && !s.pcb.contains("(net 3 "), "one net per pad number");
        assert!(s.pcb.contains("(footprint \"TEST:Two\"\n\t\t(layer \"F.Cu\")") && s.pcb.contains("(at 0 0)"), "placed at the origin on the front");
        assert!(!s.pcb.contains("(generator \"eda\")") && !s.pcb.contains("(version 20240108)"), "a board's footprint has no version of its own");
        assert!(s.pcb.contains("(property \"Reference\" \"REF**\"") && s.pcb.contains("(hide yes)"), "a hidden Reference, as the editor's footprint has one");
        assert!(s.pcb.contains("(property \"Value\" \"Two\""), "and its Value, the footprint's name: {}", s.pcb);
        // the library file's own fields (a visible REF** on the silkscreen) are not on it: the checker's silk test never looks at fields; the two the board has are hidden
        assert_eq!(s.pcb.matches("(property ").count(), 2, "{}", s.pcb);
        assert_eq!(s.pcb.matches("(hide yes)").count(), 2, "{}", s.pcb);
        // a footprint loaded from the board names the part: "Footprint R1"
        let mut linked = fp.clone();
        linked.reference = Some(eda_model::fp_edit::FieldLayout { text: Some("R1".into()), ..eda_model::fp_edit::default_library_reference_layout() });
        assert!(scratch_board(&linked).pcb.contains("(property \"Reference\" \"R1\""));
        // pads carry their net; the two pads numbered 2 share one; the hole has none
        let pad_lines: Vec<&str> = s.pcb.lines().filter(|l| l.trim_start().starts_with("(pad ")).collect();
        assert_eq!(pad_lines.len(), 4);
        assert!(pad_lines[0].contains("(net 1 \"1\")") && pad_lines[1].contains("(net 2 \"2\")") && pad_lines[2].contains("(net 2 \"2\")") && !pad_lines[3].contains("(net "), "{pad_lines:?}");
        // the uuids are the checker's own and say which item each is
        assert_eq!(s.ids.get(&uuid_of(1)).map(String::as_str), Some("g1"));
        assert_eq!(s.ids.values().filter(|v| v.starts_with('p')).count(), 4);
        assert!(pad_lines[0].contains(&uuid_of(2)), "{}", pad_lines[0]);
        // the outline reaches 10 mm past the farthest pad (5 mm and its 1 mm size)
        assert!(s.pcb.contains("(gr_rect (start -16 -16) (end 16 16)"), "{}", s.pcb.lines().last().unwrap_or(""));
        // every parenthesis closes
        assert_eq!(s.pcb.matches('(').count(), s.pcb.matches(')').count());
    }

    #[test]
    fn the_project_gives_the_footprint_editors_severities() {
        let p: Value = serde_json::from_str(&project_text()).unwrap();
        let sev = &p["board"]["design_settings"]["rule_severities"];
        assert_eq!((sev["malformed_courtyard"].as_str(), sev["missing_courtyard"].as_str(), sev["holes_co_located"].as_str(), sev["padstack"].as_str(), sev["footprint_type_mismatch"].as_str()), (Some("error"), Some("warning"), Some("warning"), Some("warning"), Some("warning")));
        assert_eq!(p["board"]["design_settings"]["rules"]["min_hole_to_hole"], json!(0.0001));
        assert_eq!(sev.as_object().map(|o| o.len()), Some(CHECKS.len()));
    }

    #[test]
    fn rewriting_a_form_replaces_its_own_uuid_only() {
        let f = "(pad \"1\" smd rect (at 0 0) (size 1 1) (primitives (gr_poly (pts (xy 0 0)) (uuid \"inner\"))) (uuid \"old\"))";
        let out = rewrite_form(f, "NEW", " (net 1 \"1\")");
        assert!(out.contains("(uuid \"NEW\")") && !out.contains("\"old\"") && out.contains("(uuid \"inner\")"), "{out}");
        assert!(out.ends_with(" (net 1 \"1\"))"), "{out}");
        assert_eq!(out.matches('(').count(), out.matches(')').count());
    }

    #[test]
    fn marker_positions_follow_the_items() {
        let item = |x: Nm, y: Nm| super::super::Item { description: "d".into(), pos: eda_model::ir::Point { x, y }, id: None, uuid: String::new() };
        let mut v = Violation { kind: "shorting_items".into(), ..Default::default() };
        assert_eq!(marker_at(&v), (Nm::ZERO, Nm::ZERO));
        v.items = vec![item(mm(1.0), mm(2.0))];
        assert_eq!(marker_at(&v), (mm(1.0), mm(2.0)));
        v.items = vec![item(Nm::ZERO, Nm::ZERO), item(mm(1.0), mm(2.0))];
        assert_eq!(marker_at(&v), (mm(0.5), mm(1.0)));
    }

    /// kicad-cli on the scratch board: each defect the footprint checker finds is found, a clean footprint has none, and what the checker reports is what the
    /// board DRC says of its checks on that board.
    #[test]
    fn kicad_cli_finds_what_the_footprint_checker_finds() {
        if std::env::var_os("EDA_SLOW_TESTS").is_none() {
            eprintln!("skipped: slow test; set EDA_SLOW_TESTS=1 to run it");
            return;
        }
        // No other test's environment (a one-second time limit) while this one runs kicad-cli.
        let _env = crate::tests::Env::take();
        if super::super::find_cli().is_none() {
            eprintln!("skipped: kicad-cli not found");
            return;
        }
        let courtyard = |layer: &str| vec![line("c1", layer, (mm(-3.0), mm(-2.0)), (mm(3.0), mm(-2.0))), line("c2", layer, (mm(3.0), mm(-2.0)), (mm(3.0), mm(2.0))), line("c3", layer, (mm(3.0), mm(2.0)), (mm(-3.0), mm(2.0))), line("c4", layer, (mm(-3.0), mm(2.0)), (mm(-3.0), mm(-2.0)))];
        let kinds = |c: &FootprintCheck| -> Vec<String> {
            let mut k: Vec<String> = c.violations.iter().map(|v| v.kind.clone()).collect();
            k.sort();
            k.dedup();
            k
        };

        // a clean footprint: two SMD pads apart, a closed courtyard
        let mut clean = footprint(vec![pad("p1", "1", mm(-1.5), Nm::ZERO, PadKind::Smd, None), pad("p2", "2", mm(1.5), Nm::ZERO, PadKind::Smd, None)]);
        clean.graphics = courtyard("F.CrtYd");
        clean.attributes.smd = true;
        let c = footprint_check(&clean).expect("kicad-cli runs");
        assert!(c.violations.is_empty(), "{:?}", c.violations.iter().map(|v| (&v.kind, &v.description)).collect::<Vec<_>>());

        // silkscreen drawn over a pad: clipped by the mask, an error (`CheckClippedSilk`)
        let mut silk = clean.clone();
        silk.graphics.push(line("s1", "F.SilkS", (mm(-2.5), Nm::ZERO), (mm(-0.5), Nm::ZERO)));
        let c = footprint_check(&silk).unwrap();
        assert_eq!(kinds(&c), ["silk_over_copper"], "{:?}", c.violations.iter().map(|v| (&v.kind, &v.description)).collect::<Vec<_>>());
        assert!(c.violations.iter().all(|v| v.severity == "error"));

        // no courtyard at all: a warning
        let mut missing = clean.clone();
        missing.graphics.clear();
        let c = footprint_check(&missing).unwrap();
        assert_eq!(kinds(&c), ["missing_courtyard"], "{:?}", c.violations.iter().map(|v| (&v.kind, &v.description)).collect::<Vec<_>>());
        assert_eq!(c.violations[0].severity, "warning");
        missing.attributes.allow_missing_courtyard = true;
        assert!(footprint_check(&missing).unwrap().violations.is_empty(), "allowed by the footprint's own attribute");

        // an open courtyard: malformed, an error, naming the shapes
        let mut open = clean.clone();
        open.graphics.pop();
        let c = footprint_check(&open).unwrap();
        assert_eq!(kinds(&c), ["malformed_courtyard"], "{:?}", c.violations.iter().map(|v| (&v.kind, &v.description)).collect::<Vec<_>>());
        assert_eq!(c.violations[0].severity, "error");

        // two pads of different numbers that overlap are shorting items, with the editor's ids on the items
        let mut touching = clean.clone();
        touching.pads[1].at = Point { x: mm(-1.0), y: Nm::ZERO };
        let c = footprint_check(&touching).unwrap();
        assert_eq!(kinds(&c), ["shorting_items"], "{:?}", c.violations.iter().map(|v| (&v.kind, &v.description)).collect::<Vec<_>>());
        let mut ids: Vec<_> = c.violations[0].items.iter().filter_map(|i| i.id.clone()).collect();
        ids.sort();
        assert_eq!(ids, ["p1", "p2"], "the markers name the editor's pads");
        // the same pad numbers: one logical pad, not a short
        touching.pads[1].number = "1".into();
        assert!(footprint_check(&touching).unwrap().violations.is_empty());

        // a through-hole pad with no hole
        let mut no_hole = clean.clone();
        no_hole.pads[0].kind = PadKind::ThroughHole;
        no_hole.pads[0].drill = None;
        no_hole.attributes.smd = false;
        no_hole.attributes.through_hole = true;
        let c = footprint_check(&no_hole).unwrap();
        // (this kicad-cli says it as an invalid padstack: "PTH pad hole size must be larger than ..."; older ones with `through_hole_pad_without_hole`)
        assert!(kinds(&c).iter().any(|k| k == "through_hole_pad_without_hole" || k == "padstack_invalid"), "{:?}", c.violations.iter().map(|v| (&v.kind, &v.description)).collect::<Vec<_>>());
        assert!(c.violations.iter().all(|v| v.severity == "error"));

        // a footprint marked SMD that has through-hole pads: the type mismatch, a warning
        let mut mismatch = footprint(vec![pad("p1", "1", mm(-1.5), Nm::ZERO, PadKind::ThroughHole, Some(mm(0.6))), pad("p2", "2", mm(1.5), Nm::ZERO, PadKind::ThroughHole, Some(mm(0.6)))]);
        mismatch.graphics = courtyard("F.CrtYd");
        mismatch.attributes.smd = true;
        let c = footprint_check(&mismatch).unwrap();
        assert_eq!(kinds(&c), ["footprint_type_mismatch"], "{:?}", c.violations.iter().map(|v| (&v.kind, &v.description)).collect::<Vec<_>>());
        assert_eq!(c.violations[0].severity, "warning");

        // two drilled holes at the same place
        let mut colocated = footprint(vec![pad("p1", "1", Nm::ZERO, Nm::ZERO, PadKind::ThroughHole, Some(mm(0.6))), pad("p2", "1", Nm::ZERO, Nm::ZERO, PadKind::ThroughHole, Some(mm(0.6)))]);
        colocated.graphics = courtyard("F.CrtYd");
        colocated.attributes.through_hole = true;
        let c = footprint_check(&colocated).unwrap();
        assert!(kinds(&c).contains(&"holes_co_located".to_string()), "{:?}", c.violations.iter().map(|v| (&v.kind, &v.description)).collect::<Vec<_>>());
        // holes that are close but do not overlap are not a finding (the board's 0.25 mm does not apply)
        colocated.pads[1].at = Point { x: mm(1.0), y: Nm::ZERO };
        colocated.pads[1].number = "2".into();
        colocated.pads[1].size = (mm(0.4), mm(0.4));
        colocated.pads[0].size = (mm(0.4), mm(0.4));
        let c = footprint_check(&colocated).unwrap();
        assert!(!kinds(&c).contains(&"hole_to_hole".to_string()), "{:?}", c.violations.iter().map(|v| (&v.kind, &v.description)).collect::<Vec<_>>());

        // what the checker reports is the board DRC's, on that board, for its checks
        let board = scratch_board(&touching);
        let direct = with_scratch(|dir| {
            let pcb = dir.join("direct.kicad_pcb");
            work_dir(dir).unwrap();
            write_file(&pcb, &board.pcb).unwrap();
            write_file(&dir.join("direct.kicad_pro"), &board.project).unwrap();
            let report = dir.join("direct.json");
            let mut cmd = super::super::cli_command(&super::super::find_cli().unwrap(), None);
            cmd.args(["pcb", "drc", "--format", "json", "--severity-all", "--units", "mm", "-o"]).arg(&report).arg(&pcb);
            run_report(cmd, &report, "direct").unwrap()
        });
        let mut expected: Vec<(String, String)> = direct["violations"].as_array().unwrap().iter().filter(|v| is_checker_kind(v["type"].as_str().unwrap_or(""))).map(|v| (v["type"].as_str().unwrap().to_string(), v["severity"].as_str().unwrap().to_string())).collect();
        expected.sort();
        let mut got: Vec<(String, String)> = footprint_check(&touching).unwrap().violations.iter().map(|v| (v.kind.clone(), v.severity.clone())).collect();
        got.sort();
        assert_eq!(got, expected);
    }
    fn violation(kind: &str, items: &[(&str, Nm, Nm)]) -> Violation {
        Violation {
            kind: kind.into(),
            description: String::new(),
            severity: "error".into(),
            items: items.iter().map(|(id, x, y)| super::super::Item { description: String::new(), pos: Point { x: *x, y: *y }, id: Some(id.to_string()), uuid: String::new() }).collect(),
            excluded: false,
            comment: String::new(),
            markers_nm: vec![],
            sheet: String::new(),
            marker: None,
        }
    }

    /// UI-LEFT F9: the markers are where `DIALOG_FOOTPRINT_CHECKER::runChecks` puts them -- two rectangular pads that short at `Collide`'s location (the first point of one
    /// inside the other, else where the first edges cross), a hole check at the first pad, a footprint's finding at the origin.
    #[test]
    fn the_checkers_markers_are_where_kicads_are() {
        let fp = footprint(vec![pad("p1", "1", Nm::ZERO, Nm::ZERO, PadKind::Smd, None), pad("p2", "2", mm(0.6), Nm::ZERO, PadKind::Smd, None), pad("p3", "3", mm(5.0), mm(0.3), PadKind::Smd, None)]);
        // p1 [-0.5, 0.5]^2 and p2 [0.1, 1.1] x [-0.5, 0.5]: neither's first corner is inside the other (p2's top-left lies on p1's edge, where the ray test says outside);
        // the first edges that meet, in their sorted order, are the two top edges, collinear: they meet in the middle of their overlap, (0.3, -0.5)
        let short = violation("shorting_items", &[("p1", Nm::ZERO, Nm::ZERO), ("p2", mm(0.6), Nm::ZERO)]);
        assert_eq!(marker_in(&fp, &short), (mm(0.3), mm(-0.5)));
        // a pad down and to the side: its top-left corner is inside the first pad, and is the marker's place
        let cross = footprint(vec![pad("a", "1", Nm::ZERO, Nm::ZERO, PadKind::Smd, None), pad("b", "2", mm(0.6), mm(0.7), PadKind::Smd, None)]);
        assert_eq!(marker_in(&cross, &violation("shorting_items", &[("a", Nm::ZERO, Nm::ZERO), ("b", mm(0.6), mm(0.7))])), (mm(0.1), mm(0.2)));
        // a pad turned 45 degrees (a polygon, `SHAPE_SIMPLE`): its first corner, (-hx, +hy) turned, is its left one, inside the first pad
        let turned = footprint(vec![pad("a", "1", Nm::ZERO, Nm::ZERO, PadKind::Smd, None), LibraryPad { rot: 45_000, ..pad("b", "2", mm(1.1), Nm::ZERO, PadKind::Smd, None) }]);
        let at = marker_in(&turned, &violation("shorting_items", &[("a", Nm::ZERO, Nm::ZERO), ("b", mm(1.1), Nm::ZERO)]));
        assert!((at.0.get() - (1_100_000 - 707_107)).abs() <= 1 && at.1 == Nm::ZERO, "b's left corner: {at:?}");
        assert_eq!(marker_at(&short), (mm(0.3), Nm::ZERO), "kicad-cli's report alone puts it between the pads");
        // a pad inside the other: its first corner
        let inner = footprint(vec![pad("big", "1", Nm::ZERO, Nm::ZERO, PadKind::Smd, None), LibraryPad { size: (mm(0.2), mm(0.2)), ..pad("small", "2", mm(0.1), mm(0.1), PadKind::Smd, None) }]);
        assert_eq!(marker_in(&inner, &violation("shorting_items", &[("small", mm(0.1), mm(0.1)), ("big", Nm::ZERO, Nm::ZERO)])), (mm(0.0), mm(0.0)));
        // the hole checks: the first pad of the two in the footprint's order
        assert_eq!(marker_in(&fp, &violation("holes_co_located", &[("p3", mm(5.0), mm(0.3)), ("p1", Nm::ZERO, Nm::ZERO)])), (Nm::ZERO, Nm::ZERO));
        assert_eq!(marker_in(&fp, &violation("missing_courtyard", &[])), (Nm::ZERO, Nm::ZERO));
        assert_eq!(marker_in(&fp, &violation("padstack", &[("p3", mm(5.0), mm(0.3))])), (mm(5.0), mm(0.3)));
    }
}

