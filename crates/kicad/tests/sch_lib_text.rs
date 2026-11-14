//! A plain text of a library symbol (`(text "text" ..)` in a unit's sub-symbol, `SCH_TEXT` on `LAYER_DEVICE`) and a text box's border on a placed symbol (UI-LEFT-3
//! S3-5, S3-8).
//!
//! The reader of a library keeps each text's look -- its justification, bold, italic, face, colour, pen and hidden flag (`LibConnectivity::text_looks`) -- and the line
//! style and colours of a text box's border (the look of the rectangle the engine draws it with, `LibConnectivity::shape_looks`). The `.kicad_sch` the studio derives
//! writes them back: a text turned and justified as `SCH_TEXT::Plot` draws it through the symbol's transform (`eda_model::sym_textbox::plotted_text_attrs`), as the
//! symbol is written unturned and baked; a text box with its line style.
//!
//! The slow test (`EDA_SLOW_TESTS=1`; skipped without kicad-cli) has kicad-cli plot the derived sheet and the sheet as KiCad has it (the library symbol in the cache, the
//! symbol turned and mirrored by its own `(at ..)` and `(mirror ..)`) in six orientations, and finds each text's strokes in the same place.

use eda_kicad::{export_kicad_sch, list_symbols_in_file, parse_library_symbols, ExportMeta};
use eda_model::ir::{Design, Point, Provenance, SchematicSection, SymbolInstance};
use eda_model::sym_fields::{FieldHAlign, FieldVAlign};
use eda_model::symbol::{LibSymbol, SymbolGraphic};
use eda_model::units::mm;
use eda_model::{ConstraintModel, Part, Pin, PinKind};
use std::path::{Path, PathBuf};
use std::process::Command;

const META: ExportMeta<'static> = ExportMeta { date: "2026-11-14", title: "library texts" };

/// A box with a left-top text naming the reference, a right-bottom text running up its right side, a centred two-line text, a bold italic text in a face of its own
/// with a colour and a pen, a hidden one, a private one, and a text box with a dashed border.
const LIBRARY: &str = r#"(kicad_symbol_lib (version 20241209) (generator "test")
  (symbol "TX" (pin_names (offset 0.508)) (exclude_from_sim no) (in_bom yes) (on_board yes)
    (property "Reference" "U" (at 0 10.16 0) (effects (font (size 1.27 1.27))))
    (property "Value" "TX" (at 0 -10.16 0) (effects (font (size 1.27 1.27))))
    (symbol "TX_0_1"
      (rectangle (start -7.62 7.62) (end 7.62 -7.62) (stroke (width 0.254) (type default)) (fill (type background)))
      (text "${REFERENCE}" (at -6.35 6.35 0) (effects (font (size 1.27 1.27)) (justify left top)))
      (text "UP" (at 5.08 -5.08 90) (effects (font (size 1.27 1.27)) (justify right bottom)))
      (text "ONE\nTWO" (at 0 0 0) (effects (font (size 1.016 1.016))))
      (text "FACE" (at 0 -3.81 0) (effects (font (face "DejaVu Sans") (size 1.27 1.27) (thickness 0.3) bold italic (color 200 0 0 1)) (justify left)))
      (text "GONE" (at 0 3.81 0) (effects (font (size 1.27 1.27)) (hide yes)))
      (text private "MINE" (at 0 -6.35 0) (effects (font (size 1.27 1.27))))
      (text_box "BOX" (at -6.35 -2.54 0) (size 3.81 -2.54) (margins 0.381 0.381 0.381 0.381) (stroke (width 0.254) (type dash) (color 0 0 200 1)) (fill (type color) (color 0 200 0 0.5))
        (effects (font (size 1.016 1.016)) (justify left top)))
    )
    (symbol "TX_1_1"
      (pin input line (at -10.16 0 0) (length 2.54) (name "A" (effects (font (size 1.27 1.27)))) (number "1" (effects (font (size 1.27 1.27)))))
      (pin output line (at 10.16 0 180) (length 2.54) (name "Y" (effects (font (size 1.27 1.27)))) (number "2" (effects (font (size 1.27 1.27)))))
    )
  )
)
"#;

fn scratch(tag: &str) -> PathBuf {
    static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("eda_sch_lib_text_{tag}_{}_{n}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// The test symbol as the installed-library reader gives it, under `test:TX`.
fn test_symbol() -> LibSymbol {
    let dir = scratch("lib");
    let path = dir.join("test.kicad_sym");
    std::fs::write(&path, LIBRARY).unwrap();
    let sym = list_symbols_in_file(&path, "test").into_iter().find(|s| s.lib_id == "test:TX").expect("the reader reads the symbol");
    let _ = std::fs::remove_dir_all(&dir);
    sym
}

fn part_of(reference: &str, lib: &LibSymbol) -> Part {
    Part {
        reference: reference.into(),
        mpn: None,
        lcsc: None,
        value: Some("TX".into()),
        package: None,
        footprint: Some("Foo:Bar".into()),
        symbol: Some(lib.lib_id.clone()),
        datasheet: None,
        pins: lib.pins.iter().map(|p| Pin { number: p.number.clone(), name: None, kind: PinKind::Signal }).collect(),
        body_nm: None,
        edge: None,
    }
}

fn design_with(sch: SchematicSection) -> Design {
    Design {
        schema: eda_model::ir::SCHEMA_VERSION,
        provenance: Provenance { engine_version: "0".into(), intent_hash: "x".into(), seed: 0, stage_hashes: vec![] },
        schematic: Some(sch),
        nets: None,
        placement: None,
        routing: None,
        drawings: None,
        footprint_library: None,
        sheet_contents: None,
        bus_aliases: vec![],
        symbol_library: None,
    }
}

/// Unturned, a quarter turn, mirrored left-right, mirrored top-bottom, a quarter turn and mirrored left-right, three quarters and mirrored top-bottom.
const ORIENTATIONS: [(u32, bool, bool); 6] = [(0, false, false), (90_000, false, false), (0, true, false), (0, false, true), (90_000, true, false), (270_000, false, true)];

fn instance(reference: &str, lib: &LibSymbol, at: Point, (rot, mirrored, mirror_y): (u32, bool, bool)) -> SymbolInstance {
    SymbolInstance { id: reference.into(), at, rot, mirrored, mirror_y, lib_id: lib.lib_id.clone(), unit: 1, value: "TX".into(), footprint: String::new(), datasheet: String::new(), dnp: false, exclude_from_bom: false, exclude_from_board: false, exclude_from_sim: false, no_footprint: false }
}

/// `U1` .. `U6` of `lib`, one in each of [`ORIENTATIONS`].
fn placed(lib: &LibSymbol) -> (ConstraintModel, Design) {
    let mut sch = SchematicSection::default();
    let mut parts = Vec::new();
    for (i, o) in ORIENTATIONS.into_iter().enumerate() {
        let reference = format!("U{}", i + 1);
        sch.symbols.push(instance(&reference, lib, Point { x: mm(25.4) + mm(76.2) * (i as i64 % 3), y: mm(25.4) + mm(76.2) * (i as i64 / 3) }, o));
        parts.push(part_of(&reference, lib));
    }
    (ConstraintModel { parts, symbols: vec![lib.clone()], ..Default::default() }, design_with(sch))
}

/// The cache entry the instance `reference` draws from.
fn entry_of<'a>(text: &'a str, reference: &str, lib_id: &str) -> &'a str {
    let at = text.find(&format!("(property \"Reference\" \"{reference}\"")).unwrap();
    let head = &text[text[..at].rfind("\t(symbol").unwrap()..at];
    let name = head.split("(lib_name \"").nth(1).and_then(|r| r.split('"').next()).unwrap_or(lib_id);
    let start = text.find(&format!("\t\t(symbol \"{name}\"\n")).unwrap_or_else(|| panic!("no cache entry {name}"));
    let rest = &text[start + 3..];
    let end = rest.find("\n\t\t(symbol \"").or_else(|| rest.find("\n\t)")).unwrap();
    &rest[..end]
}

/// The `(text "<text>" ..)` of a cache entry: its `(at ..)` angle and its `(effects ..)`.
fn text_form(entry: &str, text: &str) -> (String, String) {
    let at = entry.find(&format!("(text \"{text}\" (at ")).unwrap_or_else(|| panic!("no text {text}: {entry}"));
    let rest = &entry[at..];
    let angle = rest.split("(at ").nth(1).unwrap().split(')').next().unwrap().split(' ').nth(2).unwrap().to_string();
    let effects = rest.split("(effects ").nth(1).unwrap().split('\n').next().unwrap().to_string();
    (angle, effects)
}

#[test]
fn a_library_texts_looks_and_a_text_boxs_border_are_read_with_the_symbol() {
    let sym = test_symbol();
    let look = |text: &str| {
        let g = sym.graphics.iter().find(|g| matches!(g, SymbolGraphic::Text { text: t, .. } if t == text)).unwrap_or_else(|| panic!("no text {text}"));
        sym.connectivity.text_look(g).cloned()
    };
    let r = look("${REFERENCE}").expect("a justified text has a look");
    assert_eq!((r.h_align, r.v_align, r.bold, r.hidden), (FieldHAlign::Left, FieldVAlign::Top, false, false));
    let up = look("UP").unwrap();
    assert_eq!((up.h_align, up.v_align), (FieldHAlign::Right, FieldVAlign::Bottom));
    assert!(look("ONE\nTWO").is_none(), "a centred plain text has none");
    let face = look("FACE").unwrap();
    assert_eq!((face.font.as_deref(), face.bold, face.italic, face.thickness_mm, face.text_color.map(|c| (c.r, c.g, c.b))), (Some("DejaVu Sans"), true, true, Some(0.3), Some((200, 0, 0))));
    assert!(look("GONE").unwrap().hidden);
    assert!(!sym.graphics.iter().any(|g| matches!(g, SymbolGraphic::Text { text, .. } if text == "MINE")), "a private text is not drawn on a sheet");
    // the text box's border: dashed, blue, filled green
    let border = sym.connectivity.text_boxes[0].to_engine_graphics()[0].clone();
    let l = sym.connectivity.shape_look(&border).expect("the border has a look");
    assert_eq!((l.stroke_type.as_deref(), l.looks.stroke_color.map(|c| c.b), l.fill, l.looks.fill_color.map(|c| c.g)), (Some("dash"), Some(200), eda_model::ir::LibraryFill::Color, Some(200)));
    // the Symbol Editor's symbol of the same file has the same looks when it is placed
    let parsed = parse_library_symbols(LIBRARY).expect("the library parses");
    let engine = parsed.symbols[0].to_engine_symbol();
    assert_eq!(engine.connectivity.text_looks, sym.connectivity.text_looks);
    assert_eq!(engine.connectivity.shape_look(&border).map(|l| (l.stroke_type.clone(), l.fill)), Some((Some("dash".to_string()), eda_model::ir::LibraryFill::Color)));
}

#[test]
fn the_derived_sheet_writes_each_text_justified_as_sch_text_plot_draws_it_and_the_text_box_with_its_line_style() {
    let lib = test_symbol();
    let (m, design) = placed(&lib);
    let text = export_kicad_sch(&design, &m, &META).expect("the sheet exports");
    // (reference, `${REFERENCE}`'s angle and justification, `UP`'s) -- `SCH_TEXT::Plot` through each orientation's transform
    let want = [
        ("U1", ("0", "(justify left top)"), ("90", "(justify right bottom)")),
        ("U2", ("90", "(justify left top)"), ("0", "(justify left bottom)")),
        ("U3", ("0", "(justify right top)"), ("90", "(justify right top)")),
        ("U4", ("0", "(justify left bottom)"), ("90", "(justify left bottom)")),
        ("U5", ("90", "(justify left top)"), ("0", "(justify right top)")),
        ("U6", ("90", "(justify left top)"), ("0", "(justify right top)")),
    ];
    for (reference, first, second) in want {
        let entry = entry_of(&text, reference, "test:TX");
        let (a, e) = text_form(entry, "${REFERENCE}");
        assert!(a == first.0 && e.contains(first.1), "{reference} ${{REFERENCE}}: {a} {e}");
        let (a, e) = text_form(entry, "UP");
        assert!(a == second.0 && e.contains(second.1), "{reference} UP: {a} {e}");
        // the text box with its dashed border, written once
        let tb = entry.lines().find(|l| l.contains("(text_box \"BOX\"")).unwrap_or_else(|| panic!("{reference}: {entry}"));
        assert!(tb.contains("(stroke (width 0.254) (type dash) (color 0 0 200 1))"), "{reference}: {tb}");
        assert_eq!(entry.matches("(rectangle").count(), 1, "{reference}: only the body is a rectangle: {entry}");
        assert!(!entry.contains("MINE"), "a private text stays out of the sheet");
    }
    let u1 = entry_of(&text, "U1", "test:TX");
    let (_, face) = text_form(u1, "FACE");
    assert!(face.starts_with("(font (face \"DejaVu Sans\") (size 1.27 1.27) (thickness 0.3) bold italic (color 200 0 0 1)) (justify left)"), "{face}");
    let (_, gone) = text_form(u1, "GONE");
    assert!(gone.contains("(hide yes)"), "{gone}");
}

fn find_kicad_cli() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("KICAD_CLI") {
        return Some(PathBuf::from(p));
    }
    [PathBuf::from("/usr/local/bin/kicad-cli"), PathBuf::from("/usr/bin/kicad-cli")].into_iter().find(|p| p.exists())
}

fn slow() -> Option<PathBuf> {
    if std::env::var_os("EDA_SLOW_TESTS").is_none() {
        eprintln!("skipped: slow test; set EDA_SLOW_TESTS=1 to run it");
        return None;
    }
    let cli = find_kicad_cli();
    if cli.is_none() {
        eprintln!("skipped: no kicad-cli");
    }
    cli
}

/// kicad-cli's SVG plot of `text`: each stroked text (`<g class="stroked-text"><desc>TEXT</desc>..`) as its text and the points of its strokes, mm.
fn plotted(cli: &Path, dir: &Path, name: &str, text: &str) -> Vec<(String, Vec<(f64, f64)>)> {
    let path = dir.join(format!("{name}.kicad_sch"));
    std::fs::write(&path, text).unwrap();
    let out = dir.join(format!("{name}_svg"));
    let run = Command::new(cli).args(["sch", "export", "svg", "--exclude-drawing-sheet", "-o"]).arg(&out).arg(&path).output().expect("run kicad-cli");
    assert!(run.status.success(), "kicad-cli could not plot {name}: {}{}", String::from_utf8_lossy(&run.stdout), String::from_utf8_lossy(&run.stderr));
    let svg = std::fs::read_dir(&out).unwrap().filter_map(Result::ok).map(|e| e.path()).find(|p| p.extension().is_some_and(|x| x == "svg")).expect("an SVG");
    let svg = std::fs::read_to_string(svg).unwrap();
    svg.split("<g class=\"stroked-text\"><desc>")
        .skip(1)
        .map(|g| {
            let (desc, rest) = g.split_once("</desc>").unwrap();
            let body = rest.split("</g>").next().unwrap();
            let numbers: Vec<f64> = body.split("d=\"").skip(1).flat_map(|d| d.split('"').next().unwrap().split(|c: char| c == 'M' || c == 'L' || c.is_whitespace()).filter_map(|n| n.parse::<f64>().ok()).collect::<Vec<_>>()).collect();
            (desc.to_string(), numbers.chunks(2).map(|p| (p[0], p[1])).collect())
        })
        .collect()
}

/// The derived sheet of `lib` placed alone in `orientation`, and the same sheet as KiCad has it: the library symbol in the cache and the symbol placed by its own
/// origin with KiCad's turn and mirror (as `sch_lib_text_box.rs` builds it).
fn derived_and_kicads(lib: &LibSymbol, own_cache: &str, orientation: (u32, bool, bool)) -> (String, String) {
    let (rot, mirrored, mirror_y) = orientation;
    let at = Point { x: mm(50.8), y: mm(50.8) };
    let mut sch = SchematicSection::default();
    sch.symbols.push(instance("U1", lib, at, orientation));
    let m = ConstraintModel { parts: vec![part_of("U1", lib)], symbols: vec![lib.clone()], ..Default::default() };
    let derived = export_kicad_sch(&design_with(sch), &m, &META).expect("the sheet exports");
    let (x0, _, _, y1) = eda_engine::geometry::real_symbol_bbox(lib, 1);
    let (turn, mirror) = eda_engine::symgeom::kicad_turn(rot, mirrored, mirror_y);
    let (dx, dy) = eda_engine::symgeom::bake(turn, mirror, 0.0, -x0 * 1e6, y1 * 1e6);
    let origin = (at.x.as_f64() + dx) / 1e6;
    let origin_y = (at.y.as_f64() + dy) / 1e6;
    let start = derived.find("\t(lib_symbols\n").unwrap();
    let end = start + derived[start..].find("\n\t)\n").unwrap() + 3;
    let line = format!("(lib_id \"{}\") (at 50.8 50.8 0)", lib.lib_id);
    assert!(derived.contains(&line), "{derived}");
    let kicads = format!("{}\t(lib_symbols\n{own_cache}\n\t){}", &derived[..start], &derived[end..]).replace(
        &line,
        &format!("(lib_id \"{}\") (at {origin:.4} {origin_y:.4} {}){}", lib.lib_id, turn / 1000, if mirror { " (mirror y)" } else { "" }),
    );
    (derived, kicads)
}

#[test]
fn kicad_cli_plots_each_baked_text_as_it_plots_the_library_symbol_turned_and_mirrored() {
    let Some(cli) = slow() else { return };
    let lib = test_symbol();
    let start = LIBRARY.find("(symbol \"TX\"").unwrap();
    let end = LIBRARY.rfind("\n)").unwrap();
    let cache = LIBRARY[start..end].replacen("(symbol \"TX\"", "(symbol \"test:TX\"", 1);
    let dir = scratch("parity");
    let close = |a: &[(f64, f64)], b: &[(f64, f64)]| a.len() == b.len() && a.iter().zip(b).all(|(p, q)| (p.0 - q.0).abs() < 0.002 && (p.1 - q.1).abs() < 0.002);
    for (i, orientation) in ORIENTATIONS.into_iter().enumerate() {
        let (derived, kicads) = derived_and_kicads(&lib, &cache, orientation);
        let ours = plotted(&cli, &dir, &format!("ours{i}"), &derived);
        let theirs = plotted(&cli, &dir, &format!("kicad{i}"), &kicads);
        for want in ["UP", "ONE", "TWO", "BOX"] {
            let a = &ours.iter().find(|(t, _)| t == want).unwrap_or_else(|| panic!("{orientation:?}: {want} is not in our plot")).1;
            let b = &theirs.iter().find(|(t, _)| t == want).unwrap_or_else(|| panic!("{orientation:?}: {want} is not in KiCad's plot")).1;
            assert!(close(a, b), "{orientation:?} {want}: ours {a:?}\nKiCad's {b:?}");
        }
        // `${REFERENCE}` is the symbol's in both (`SCH_FIELD::GetShownText` of a dummy field), drawn where KiCad draws the library's text
        let refs = |plot: &[(String, Vec<(f64, f64)>)]| plot.iter().filter(|(t, _)| t == "U1").map(|(_, p)| p.clone()).collect::<Vec<_>>();
        assert!(refs(&ours).iter().any(|a| refs(&theirs).iter().any(|b| close(a, b))), "{orientation:?}: the text's U1 is not where KiCad draws it");
        assert!(!ours.iter().any(|(t, _)| t == "MINE"), "a private text is not plotted");
    }
    let _ = std::fs::remove_dir_all(&dir);
}
