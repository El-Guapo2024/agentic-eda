//! A text box of a library symbol (`(text_box "text" ..)` in a unit's sub-symbol, `SCH_TEXTBOX` on `LAYER_DEVICE`) on a placed symbol (UI-LEFT-2 SS5, GAPS item 19).
//!
//! The reader of a library keeps the box (`LibConnectivity::text_boxes`) beside the rectangle the engine draws its border and fill with; the `.kicad_sch` the studio
//! derives writes it back as KiCad's `(text_box ..)` in place of that rectangle, baked through the symbol's turn and mirror like the rest of the symbol, its text turned
//! and justified as `OrientAndMirrorSymbolItems` does it to a placed symbol's temp copy (`eda_model::sym_textbox`) -- so kicad-cli's plots have the text, which they
//! lost when the cache had a bare rectangle.
//!
//! The slow tests (`EDA_SLOW_TESTS=1`; each skips without kicad-cli) have kicad-cli plot the derived sheet to SVG and find each box's text there, with its variables
//! resolved by the placed symbol (`${REFERENCE}`), for a test symbol in six orientations and for KiCad's own `Analog_ADC:AD574A` (`$HOME/src/kicad-libs`, or
//! `KICAD_PATH`), whose text boxes are the reason the item exists.

use eda_kicad::{export_kicad_sch, list_symbols_in_file, parse_library_symbols, ExportMeta};
use eda_model::ir::{Design, LibrarySymbolGraphic, Point, Provenance, SchematicSection, SymbolInstance};
use eda_model::symbol::{LibSymbol, SymbolGraphic};
use eda_model::units::mm;
use eda_model::{ConstraintModel, Part, Pin, PinKind};
use std::path::{Path, PathBuf};
use std::process::Command;

const META: ExportMeta<'static> = ExportMeta { date: "2026-11-06", title: "library text boxes" };

/// A gate drawn as a box, with a text box naming the reference in its top left (left and top justified, the default margin) and a vertical one down its right
/// side, a third one private to the Symbol Editor, and three pins.
const LIBRARY: &str = r#"(kicad_symbol_lib (version 20241209) (generator "test")
  (symbol "TB" (pin_names (offset 0.508)) (exclude_from_sim no) (in_bom yes) (on_board yes)
    (property "Reference" "U" (at 0 7.62 0) (effects (font (size 1.27 1.27))))
    (property "Value" "TB" (at 0 -7.62 0) (effects (font (size 1.27 1.27))))
    (symbol "TB_0_1"
      (rectangle (start -5.08 5.08) (end 5.08 -5.08) (stroke (width 0.254) (type default)) (fill (type background)))
      (text_box "${REFERENCE}\nIN" (at -4.445 4.445 0) (size 5.08 -2.54) (margins 0.508 0.508 0.508 0.508) (stroke (width 0.254) (type default)) (fill (type none))
        (effects (font (size 1.27 1.27) bold) (justify left top)))
      (text_box "VERT" (at 2.54 3.81 90) (size 1.905 -7.62) (margins 0.381 0.381 0.381 0.381) (stroke (width -0.0001) (type default)) (fill (type none))
        (effects (font (size 1.016 1.016)) (justify left top)))
      (text_box private "HIDDEN" (at -2.54 -2.54 0) (size 2.54 -1.27) (stroke (width 0.254) (type default)) (fill (type none)) (effects (font (size 1 1))))
    )
    (symbol "TB_1_1"
      (pin input line (at -7.62 2.54 0) (length 2.54) (name "A" (effects (font (size 1.27 1.27)))) (number "1" (effects (font (size 1.27 1.27)))))
      (pin input line (at -7.62 -2.54 0) (length 2.54) (name "B" (effects (font (size 1.27 1.27)))) (number "2" (effects (font (size 1.27 1.27)))))
      (pin output line (at 7.62 0 180) (length 2.54) (name "Y" (effects (font (size 1.27 1.27)))) (number "3" (effects (font (size 1.27 1.27)))))
    )
  )
)
"#;

fn scratch(tag: &str) -> PathBuf {
    // one of its own for each call: the tests run side by side in one process
    static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("eda_sch_lib_text_box_{tag}_{}_{n}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// The test symbol as the installed-library reader gives it, under `test:TB`.
fn test_symbol() -> LibSymbol {
    let dir = scratch("lib");
    let path = dir.join("test.kicad_sym");
    std::fs::write(&path, LIBRARY).unwrap();
    let sym = list_symbols_in_file(&path, "test").into_iter().find(|s| s.lib_id == "test:TB").expect("the reader reads the symbol");
    let _ = std::fs::remove_dir_all(&dir);
    sym
}

fn part_of(reference: &str, lib: &LibSymbol) -> Part {
    Part {
        reference: reference.into(),
        mpn: None,
        lcsc: None,
        value: Some("TB".into()),
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

/// The six orientations of `sch_orientations.rs`: unturned, a quarter turn, mirrored left-right, mirrored top-bottom, a quarter turn and mirrored left-right, three
/// quarters and mirrored top-bottom.
const ORIENTATIONS: [(u32, bool, bool); 6] = [(0, false, false), (90_000, false, false), (0, true, false), (0, false, true), (90_000, true, false), (270_000, false, true)];

/// `U1` .. `U6` of `lib`, one in each of [`ORIENTATIONS`], 76.2 mm apart.
fn placed(lib: &LibSymbol) -> (ConstraintModel, Design) {
    let mut sch = SchematicSection::default();
    let mut parts = Vec::new();
    for (i, (rot, mirrored, mirror_y)) in ORIENTATIONS.into_iter().enumerate() {
        let reference = format!("U{}", i + 1);
        sch.symbols.push(SymbolInstance {
            id: reference.clone(),
            at: Point { x: mm(25.4) + mm(76.2) * (i as i64 % 3), y: mm(25.4) + mm(76.2) * (i as i64 / 3) },
            rot,
            mirrored,
            mirror_y,
            lib_id: lib.lib_id.clone(),
            unit: 1,
            value: "TB".into(),
            footprint: String::new(),
            datasheet: String::new(),
            dnp: false,
            exclude_from_bom: false,
            exclude_from_board: false,
            exclude_from_sim: false,
            no_footprint: false,
        });
        parts.push(part_of(&reference, lib));
    }
    (ConstraintModel { parts, symbols: vec![lib.clone()], ..Default::default() }, design_with(sch))
}

/// The cache entry (`(lib_symbols (symbol "<name>" ..))`) named `name`.
fn cache_entry<'a>(text: &'a str, name: &str) -> &'a str {
    let start = text.find(&format!("\t\t(symbol \"{name}\"\n")).unwrap_or_else(|| panic!("no cache entry {name}"));
    let rest = &text[start + 3..];
    let end = rest.find("\n\t\t(symbol \"").or_else(|| rest.find("\n\t)")).unwrap();
    &rest[..end]
}

/// The cache entry the instance `reference` draws from: its `(lib_name ..)`, else its `lib_id`.
fn entry_of<'a>(text: &'a str, reference: &str, lib_id: &str) -> &'a str {
    let at = text.find(&format!("(property \"Reference\" \"{reference}\"")).unwrap();
    let head = &text[text[..at].rfind("\t(symbol").unwrap()..at];
    let name = head.split("(lib_name \"").nth(1).and_then(|r| r.split('"').next()).unwrap_or(lib_id);
    cache_entry(text, name)
}

/// The `(text_box ..)` forms of a cache entry, each as its text, its `(at ..)` angle and its `(justify ..)` words.
fn text_boxes(entry: &str) -> Vec<(String, String, String)> {
    entry
        .lines()
        .filter(|l| l.trim_start().starts_with("(text_box "))
        .map(|l| {
            let text = l.split('"').nth(1).unwrap().to_string();
            let at = l.split("(at ").nth(1).unwrap().split(')').next().unwrap();
            let angle = at.split(' ').nth(2).unwrap().to_string();
            let justify = l.split("(justify ").nth(1).map(|j| j.split(')').next().unwrap().to_string()).unwrap_or_default();
            (text, angle, justify)
        })
        .collect()
}

#[test]
fn a_library_text_box_is_read_as_its_border_and_the_box_itself() {
    let sym = test_symbol();
    // the body and the bordered box's border: the borderless box has neither border nor fill, and the private one is not drawn on a sheet
    let rects: Vec<&SymbolGraphic> = sym.graphics.iter().filter(|g| matches!(g, SymbolGraphic::Rectangle { .. })).collect();
    assert_eq!(rects.len(), 2, "{:?}", sym.graphics);
    let texts: Vec<&str> = sym.connectivity.text_boxes.iter().map(|t| match t {
        LibrarySymbolGraphic::TextBox { text, .. } => text.as_str(),
        _ => panic!("only text boxes"),
    }).collect();
    assert_eq!(texts, ["${REFERENCE}\nIN", "VERT"]);
    let LibrarySymbolGraphic::TextBox { unit, start, end, angle_deg, margin_mm, h_align, .. } = &sym.connectivity.text_boxes[1] else { unreachable!() };
    assert_eq!((*unit, start.x, start.y, end.x, end.y, *angle_deg, *margin_mm, *h_align), (0, 2.54, 3.81, 4.445, -3.81, 90.0, 0.381, eda_model::sym_fields::FieldHAlign::Left));
    // the Symbol Editor's symbol of the same file has the same boxes when it is placed
    let parsed = parse_library_symbols(LIBRARY).expect("the library parses");
    assert_eq!(parsed.symbols[0].to_engine_symbol().connectivity.text_boxes, sym.connectivity.text_boxes);
}

#[test]
fn the_derived_sheet_writes_each_placed_text_box_as_kicads_text_box() {
    let lib = test_symbol();
    let (m, design) = placed(&lib);
    let text = export_kicad_sch(&design, &m, &META).expect("the sheet exports");
    // (reference, the bordered box's angle and justification, the vertical one's) -- `SCH_TEXTBOX::Plot` of each orientation: the other way after a quarter turn,
    // the justification the library's whatever the mirror
    let want = [
        ("U1", ("0", "left top"), ("90", "left top")),
        ("U2", ("90", "left top"), ("0", "left top")),
        ("U3", ("0", "left top"), ("90", "left top")),
        ("U4", ("0", "left top"), ("90", "left top")),
        ("U5", ("90", "left top"), ("0", "left top")),
        ("U6", ("90", "left top"), ("0", "left top")),
    ];
    for (reference, first, second) in want {
        let entry = entry_of(&text, reference, "test:TB");
        let boxes = text_boxes(entry);
        let got: Vec<(&str, &str, &str)> = boxes.iter().map(|(t, a, j)| (t.as_str(), a.as_str(), j.as_str())).collect();
        assert_eq!(got, [("${REFERENCE}\\nIN", first.0, first.1), ("VERT", second.0, second.1)], "{reference}: {entry}");
        // the box is written once: the rectangle of its border is not, only the body's
        assert_eq!(entry.matches("(rectangle").count(), 1, "{reference}: {entry}");
        assert!(!entry.contains("HIDDEN"), "a box private to the Symbol Editor stays out of the sheet");
    }
    // the unturned box: its corners baked as the body's (the body's top left at the corner the symbol is placed by, 0.635 mm in from it), the default border
    // and no fill, its margin
    let u1 = entry_of(&text, "U1", "test:TB");
    let line = u1.lines().find(|l| l.contains("(text_box \"${REFERENCE}")).unwrap();
    assert!(line.contains("(margins 0.508 0.508 0.508 0.508) (stroke (width 0.254) (type default)) (fill (type none)) (effects (font (size 1.27 1.27) bold) (justify left top))"), "{line}");
    let body = u1.lines().skip_while(|l| !l.contains("(rectangle")).nth(1).unwrap();
    let corner = |s: &str, key: &str| -> (f64, f64) {
        let v: Vec<f64> = s.split(&format!("({key} ")).nth(1).unwrap().split(')').next().unwrap().split(' ').map(|n| n.parse().unwrap()).collect();
        (v[0], v[1])
    };
    let (bx, by) = corner(body, "start");
    let (tx, ty) = corner(line, "at");
    assert!(((tx - bx) - 0.635).abs() < 1e-9 && ((by - ty) - 0.635).abs() < 1e-9, "box at ({tx}, {ty}), body from ({bx}, {by})");
    assert_eq!(corner(line, "size"), (5.08, -2.54), "{line}");
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

/// kicad-cli's SVG plot of `text` (a `.kicad_sch`), written to `dir` as `<name>.kicad_sch`: each stroked text of it (`SVG_PLOTTER::Text`, `<g class="stroked-text">
/// <desc>TEXT</desc><path d=".."/>..</g>`, a line of a text of several lines each) as its text and the points of its strokes, in mm.
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
            let numbers: Vec<f64> = body.split("d=\"").skip(1).flat_map(|d| d.split('"').next().unwrap().split(|c: char| c == 'M' || c == 'L' || c.is_whitespace()).filter(|n| !n.is_empty()).map(|n| n.parse::<f64>().unwrap()).collect::<Vec<_>>()).collect();
            (desc.to_string(), numbers.chunks(2).map(|p| (p[0], p[1])).collect())
        })
        .collect()
}

fn texts_of(plot: &[(String, Vec<(f64, f64)>)]) -> Vec<&str> {
    plot.iter().map(|(t, _)| t.as_str()).collect()
}

#[test]
fn kicad_cli_plots_the_text_of_each_placed_symbols_text_box() {
    let Some(cli) = slow() else { return };
    let lib = test_symbol();
    let (m, design) = placed(&lib);
    let text = export_kicad_sch(&design, &m, &META).expect("the sheet exports");
    let dir = scratch("plot");
    let plot = plotted(&cli, &dir, "sheet", &text);
    let texts = texts_of(&plot);
    let count = |want: &str| texts.iter().filter(|t| **t == want).count();
    // each line of a box's text is a stroked text of the plot (a cache with the box's border alone had none of them); `${REFERENCE}` is the symbol's, as kicad-cli's
    // painter resolves a library text box of a placed symbol (`expandLibItemTextVars`), so the reference is there twice, the field's and the box's
    for reference in ["U1", "U2", "U3", "U4", "U5", "U6"] {
        assert_eq!(count(reference), 2, "{reference}: {texts:?}");
    }
    assert_eq!((count("IN"), count("VERT")), (6, 6), "{texts:?}");
    assert!(!texts.iter().any(|t| t.contains("HIDDEN") || t.contains("${")), "{texts:?}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// The derived sheet of `lib` placed alone in `orientation`, and the same sheet as KiCad has it: the library symbol as the library draws it in the cache (`own_cache`,
/// a `(lib_symbols ..)` block) and the symbol placed by its own origin with KiCad's turn and mirror, `(at x y angle) (mirror y)`, where the derived one has it unturned
/// and baked. The origin is where the derived sheet puts the library's: the instance's corner and the baked offset of the origin from the box's corner.
fn derived_and_kicads(lib: &LibSymbol, own_cache: &str, orientation: (u32, bool, bool)) -> (String, String) {
    let (rot, mirrored, mirror_y) = orientation;
    let at = Point { x: mm(50.8), y: mm(50.8) };
    let mut sch = SchematicSection::default();
    sch.symbols.push(SymbolInstance { id: "U1".into(), at, rot, mirrored, mirror_y, lib_id: lib.lib_id.clone(), unit: 1, value: "TB".into(), footprint: String::new(), datasheet: String::new(), dnp: false, exclude_from_bom: false, exclude_from_board: false, exclude_from_sim: false, no_footprint: false });
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

/// The test symbol's own `(symbol ..)` form, named as the cache names it.
fn test_symbol_cache() -> String {
    let start = LIBRARY.find("(symbol \"TB\"").unwrap();
    let end = LIBRARY.rfind("\n)").unwrap();
    LIBRARY[start..end].replacen("(symbol \"TB\"", "(symbol \"test:TB\"", 1)
}

#[test]
fn kicad_cli_draws_each_baked_text_box_as_it_draws_the_library_symbol_turned_and_mirrored() {
    let Some(cli) = slow() else { return };
    let lib = test_symbol();
    let dir = scratch("parity");
    let close = |a: &[(f64, f64)], b: &[(f64, f64)]| a.len() == b.len() && a.iter().zip(b).all(|(p, q)| (p.0 - q.0).abs() < 0.002 && (p.1 - q.1).abs() < 0.002);
    for (i, orientation) in ORIENTATIONS.into_iter().enumerate() {
        let (derived, kicads) = derived_and_kicads(&lib, &test_symbol_cache(), orientation);
        let ours = plotted(&cli, &dir, &format!("ours{i}"), &derived);
        let theirs = plotted(&cli, &dir, &format!("kicad{i}"), &kicads);
        // the box's two lines and the vertical box: the same strokes as where kicad-cli turns and mirrors the box itself (`SCH_TEXTBOX::Plot`)
        for want in ["IN", "VERT"] {
            let a = &ours.iter().find(|(t, _)| t == want).unwrap_or_else(|| panic!("{want} is not in our plot: {:?}", texts_of(&ours))).1;
            let b = &theirs.iter().find(|(t, _)| t == want).unwrap_or_else(|| panic!("{want} is not in KiCad's plot: {:?}", texts_of(&theirs))).1;
            assert!(close(a, b), "{orientation:?} {want}: ours {a:?}\nKiCad's {b:?}");
        }
        let box_ref = |plot: &[(String, Vec<(f64, f64)>)]| plot.iter().filter(|(t, _)| t == "U1").map(|(_, p)| p.clone()).collect::<Vec<_>>();
        assert!(box_ref(&ours).iter().any(|a| box_ref(&theirs).iter().any(|b| close(a, b))), "{orientation:?}: the box's U1 is not where KiCad draws it");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// KiCad's own libraries (KiCad 10's folder form): `KICAD_PATH`'s, else `$HOME/src/kicad-libs`.
fn kicad_symbols() -> Option<PathBuf> {
    let root = std::env::var_os("KICAD_PATH").map(PathBuf::from).or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join("src/kicad-libs")))?;
    Some(root.join("symbols")).filter(|p| p.join("Analog_ADC.kicad_symdir").exists())
}

#[test]
fn kicad_cli_plots_the_text_boxes_of_kicads_ad574a_turned_and_unturned() {
    let Some(cli) = slow() else { return };
    let Some(root) = kicad_symbols() else {
        eprintln!("skipped: no KiCad symbol library with Analog_ADC (set KICAD_PATH)");
        return;
    };
    let lib = eda_kicad::resolve_symbol(&root, "Analog_ADC:AD574A").expect("AD574A resolves");
    let boxes: Vec<&str> = lib.connectivity.text_boxes.iter().filter_map(|t| match t {
        LibrarySymbolGraphic::TextBox { text, .. } => Some(text.as_str()),
        _ => None,
    }).collect();
    assert!(boxes.contains(&"10V\nREF") && boxes.contains(&"9k95") && boxes.contains(&"19k95"), "AD574A's text boxes: {boxes:?}");
    let (m, design) = placed(&lib);
    let text = export_kicad_sch(&design, &m, &META).expect("the sheet exports");
    let dir = scratch("ad574a");
    let plot = plotted(&cli, &dir, "ad574a", &text);
    let texts = texts_of(&plot);
    // one of each in each of the six symbols (a cache of the boxes' borders alone had none of them)
    for want in ["9k95", "19k95", "10V", "REF"] {
        assert_eq!(texts.iter().filter(|t| **t == want).count(), ORIENTATIONS.len(), "{want}: {texts:?}");
    }
    // and drawn as KiCad draws the library's own symbol turned a quarter and mirrored
    let own = std::fs::read_to_string(root.join("Analog_ADC.kicad_symdir/AD574A.kicad_sym")).unwrap();
    let start = own.find("(symbol \"AD574A\"").unwrap();
    let end = own.rfind(')').unwrap();
    let cache = own[start..end].replacen("(symbol \"AD574A\"", "(symbol \"Analog_ADC:AD574A\"", 1);
    let (derived, kicads) = derived_and_kicads(&lib, &cache, (90_000, true, false));
    let ours = plotted(&cli, &dir, "ad574a_ours", &derived);
    let theirs = plotted(&cli, &dir, "ad574a_kicad", &kicads);
    for want in ["9k95", "19k95", "10V", "REF"] {
        let a = &ours.iter().find(|(t, _)| t == want).unwrap().1;
        let b = &theirs.iter().find(|(t, _)| t == want).unwrap_or_else(|| panic!("{want} is not in KiCad's plot: {:?}", texts_of(&theirs))).1;
        assert!(a.len() == b.len() && a.iter().zip(b).all(|(p, q)| (p.0 - q.0).abs() < 0.002 && (p.1 - q.1).abs() < 0.002), "{want}: ours {a:?}\nKiCad's {b:?}");
    }
    let _ = std::fs::remove_dir_all(&dir);
}
