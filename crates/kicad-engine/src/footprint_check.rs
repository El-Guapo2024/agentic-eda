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
    // The Footprint Editor's footprint carries the Reference `REF**`; here it is hidden, so that what the checks say of the footprint names it and no text of its own lands on a pad.
    body.insert_str(0, &format!("\t\t(property \"Reference\" \"REF**\" (at 0 0 0) (layer \"F.SilkS\") (hide yes) (uuid \"{}\") (effects (font (size 1 1) (thickness 0.15))))\n", uuid_of(next)));
    next += 1;
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

/// Where a violation's marker goes (nanometres, the footprint's own frame): its first item's position, the middle of the first two when there are two (a short is
/// marked where the pads meet), the origin for a violation that names the footprint alone.
pub fn marker_at(v: &Violation) -> (Nm, Nm) {
    match v.items.as_slice() {
        [] => (Nm::ZERO, Nm::ZERO),
        [a] => (Nm::new(a.pos.0), Nm::new(a.pos.1)),
        [a, b, ..] => (Nm::new((a.pos.0 + b.pos.0) / 2), Nm::new((a.pos.1 + b.pos.1) / 2)),
    }
}

impl FootprintCheck {
    /// `{ ok, engine, markers: [{ type, description, severity, at: [x, y], items: [{ description, pos: [x, y], id }] }], counts: { errors, warnings } }`.
    pub fn to_json(&self) -> Value {
        let markers: Vec<Value> = self
            .violations
            .iter()
            .map(|v| {
                let at = marker_at(v);
                json!({
                    "type": v.kind,
                    "description": v.description,
                    "severity": v.severity,
                    "at": [at.0, at.1],
                    "items": v.items.iter().map(|i| json!({ "description": i.description, "pos": [i.pos.0, i.pos.1], "id": i.id })).collect::<Vec<_>>(),
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
        let item = |x: Nm, y: Nm| super::super::Item { description: "d".into(), pos: (x.get(), y.get()), id: None, uuid: String::new() };
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
}
