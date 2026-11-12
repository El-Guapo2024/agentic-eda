//! A placed symbol's library shapes and fields with their looks in the schematic's `lib_symbols` cache (UI-LEFT-2 YY2's leftover, PARITY-sch section 87): the
//! library's fill (`outline`, a hatch), line style, border and fill colours and a rectangle's corner radius read from a `.kicad_sym` (`symbol_lib.rs`), kept on
//! the engine symbol (`LibConnectivity::shape_looks`, `field_looks`) and written where `SCH_IO_KICAD_SEXPR_LIB_CACHE::SaveSymbol` writes them; a shape private
//! to the Symbol Editor is written with `private` (it was left out). The slow proof plots the derived file with kicad-cli: the colours and the dashes are
//! drawn, the private shape is not.

use eda_kicad::{export_kicad_sch, list_symbols_in_file, ExportMeta};
use eda_model::ir::{Design, Point, Provenance, SchematicSection, SymbolInstance};
use eda_model::units::mm;
use eda_model::{ConstraintModel, Part, Pin, PinKind};

const META: ExportMeta<'static> = ExportMeta { date: "2026-11-12", title: "looks" };

const LIB: &str = r#"(kicad_symbol_lib (version 20241209) (generator "test")
  (symbol "LOOK" (in_bom yes) (on_board yes)
    (property "Reference" "U" (at 0 7.62 0) (effects (font (face "KiCad Font") (size 1.27 1.27) (color 0 0 255 1))))
    (property "Value" "LOOK" (at 0 -7.62 0) (effects (font (size 1.27 1.27))))
    (symbol "LOOK_0_1"
      (rectangle (start -5.08 5.08) (end 5.08 -5.08) (radius 1.27) (stroke (width 0.254) (type dash) (color 255 0 0 1)) (fill (type background)))
      (polyline (pts (xy -2.54 2.54) (xy 2.54 0) (xy -2.54 -2.54) (xy -2.54 2.54)) (stroke (width 0.254) (type default)) (fill (type outline)))
      (circle (center 0 0) (radius 1.27) (stroke (width 0.254) (type default)) (fill (type cross_hatch) (color 0 160 0 1)))
      (rectangle private (start -3.81 -3.81) (end -1.27 -1.27) (stroke (width 0.254) (type default) (color 0 200 200 1)) (fill (type none)))
    )
    (symbol "LOOK_1_1"
      (pin input line (at -7.62 0 0) (length 2.54) (name "A" (effects (font (size 1.27 1.27)))) (number "1" (effects (font (size 1.27 1.27)))))
    )
  )
)"#;

fn model() -> ConstraintModel {
    let path = std::env::temp_dir().join(format!("sch_symbol_looks_{}_{:?}.kicad_sym", std::process::id(), std::thread::current().id()));
    std::fs::write(&path, LIB).unwrap();
    let mut sym = list_symbols_in_file(&path, "test").into_iter().find(|s| s.lib_id.ends_with("LOOK")).expect("the symbol");
    let _ = std::fs::remove_file(&path);
    sym.lib_id = "test:LOOK".into();
    let part = Part {
        reference: "U1".into(),
        mpn: None,
        lcsc: None,
        value: Some("LOOK".into()),
        package: None,
        footprint: None,
        symbol: Some("test:LOOK".into()),
        datasheet: None,
        pins: vec![Pin { number: "1".into(), name: None, kind: PinKind::Signal }],
        body_nm: None,
        edge: None,
    };
    ConstraintModel { parts: vec![part], symbols: vec![sym], ..Default::default() }
}

fn design() -> Design {
    let sym = SymbolInstance {
        id: "U1".into(),
        at: Point { x: mm(50.8), y: mm(50.8) },
        rot: 0,
        mirrored: false,
        mirror_y: false,
        lib_id: "test:LOOK".into(),
        unit: 1,
        value: "LOOK".into(),
        footprint: String::new(),
        datasheet: String::new(),
        dnp: false,
        exclude_from_bom: false,
        exclude_from_board: false,
        exclude_from_sim: false,
    };
    Design {
        schema: eda_model::ir::SCHEMA_VERSION,
        provenance: Provenance { engine_version: "0".into(), intent_hash: "x".into(), seed: 0, stage_hashes: vec![] },
        schematic: Some(SchematicSection { symbols: vec![sym], ..Default::default() }),
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

#[test]
fn the_library_reader_keeps_the_shapes_and_fields_looks() {
    let m = model();
    let c = &m.symbols[0].connectivity;
    assert_eq!(m.symbols[0].graphics.len(), 3, "the private rectangle is not drawn");
    assert_eq!(c.shape_looks.len(), 4, "{:?}", c.shape_looks);
    let rect = c.shape_looks.iter().find(|l| l.looks.radius_mm > 0.0).expect("the rounded rectangle");
    assert_eq!((rect.stroke_type.as_deref(), rect.looks.stroke_color.map(|c| (c.r, c.g, c.b)), rect.fill), (Some("dash"), Some((255, 0, 0)), eda_model::ir::LibraryFill::Background));
    assert!(c.shape_looks.iter().any(|l| l.fill == eda_model::ir::LibraryFill::Outline), "the outline fill");
    assert!(c.shape_looks.iter().any(|l| l.looks.hatch.as_deref() == Some("cross_hatch") && l.looks.fill_color.is_some_and(|c| c.g == 160)), "the hatch and its colour");
    assert_eq!(c.private_shapes(0, 1).count(), 1, "the private shape");
    assert_eq!(c.field_looks.get("Reference").and_then(|l| l.font.as_deref()), Some("KiCad Font"));
}

#[test]
fn the_cache_writes_the_looks_and_the_private_shape() {
    let out = export_kicad_sch(&design(), &model(), &META).unwrap();
    let cache = out.split("(lib_symbols").nth(1).unwrap().split("\n\t)\n").next().unwrap();
    for want in ["(radius 1.27)", "(type dash) (color 255 0 0 1)", "(fill (type outline))", "(fill (type cross_hatch) (color 0 160 0 1))", "(rectangle private\n", "(face \"KiCad Font\")"] {
        assert!(cache.contains(want), "{want} in {cache}");
    }
    if std::env::var_os("EDA_SLOW_TESTS").is_none() {
        eprintln!("skipped the kicad-cli plot: set EDA_SLOW_TESTS=1");
        return;
    }
    let cli = std::env::var_os("EDA_KICAD_CLI").map(std::path::PathBuf::from).unwrap_or_else(|| "kicad-cli".into());
    let dir = std::env::temp_dir().join(format!("sch_symbol_looks_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("looks.kicad_sch");
    std::fs::write(&file, &out).unwrap();
    let status = std::process::Command::new(cli).args(["sch", "export", "svg", "--exclude-drawing-sheet", "-o"]).arg(&dir).arg(&file).output().unwrap();
    let svg = std::fs::read_dir(&dir).unwrap().flatten().find(|e| e.path().extension().is_some_and(|x| x == "svg")).map(|e| std::fs::read_to_string(e.path()).unwrap());
    let svg = svg.unwrap_or_else(|| panic!("kicad-cli plots the derived file: {}", String::from_utf8_lossy(&status.stderr)));
    // kicad-cli reads and draws them: the red dashed border, the hatch (`SCH_SHAPE::Plot` hatches in the border's colour at 40 % alpha; the canvas's
    // `SCH_PAINTER` in the fill colour); the private shape's cyan is not on the sheet
    assert!(svg.contains("#FF0000"), "the border colour");
    assert!(svg.contains("stroke-dasharray:2.7940,1.0160"), "the dashed border");
    assert!(svg.contains("opacity:0.4000"), "the hatch: {}", dir.display());
    assert!(!svg.contains("#00C8C8"), "the private shape is not plotted");
    if std::env::var_os("KEEP_SVG").is_none() { let _ = std::fs::remove_dir_all(&dir); }
}
