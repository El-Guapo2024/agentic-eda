//! `eda import-kicad <file> -o <dir>` -- read a board or a schematic into a fresh `eda board` directory (`design.json` + `intent.yaml` + `board.json`, the same shape
//! `eda board new` writes -- see `crate::board::Meta`), so `eda board status/serve/check` can open it straight away.
//!
//! The file is a KiCad board (`.kicad_pcb`), a KiCad schematic (`.kicad_sch`, with the sheets it places), or a file of another EDA format -- Eagle, Altium, CADSTAR, PADS,
//! P-CAD, Fabmaster, EasyEDA and the rest of what `kicad-cli pcb import` and `kicad-cli sch import` take. KiCad's own importers do the converting
//! (`eda_kicad_engine::foreign`: kicad-cli is the only reader of those formats there is here); our readers turn the KiCad file it makes into the design, and what kicad-cli said
//! about the import (the format it recognised, its counts, its errors and warnings, where each layer went) is printed and, with `--report`, written.
//!
//! ```text
//! eda import-kicad templates/Arduino/Arduino_Uno_R3.kicad_pcb -o work/uno          # a KiCad board
//! eda import-kicad "Adafruit AHT20.brd" -o work/aht20                              # Eagle: the board, and the schematic of the same name beside it
//! eda import-kicad Power.SchDoc -o work/power                                      # Altium: a schematic
//! eda import-kicad Project.PrjPcb -o work/project                                  # Altium: the project's board and its sheets
//! eda import-kicad board.PcbDoc -o work/b --layer-map layers.json --report r.json  # source layer -> KiCad layer; kicad-cli's report as JSON
//! eda board status -C work/uno
//! eda board serve  -C work/uno
//! ```
//!
//! Options: `--format NAME` (kicad-cli's importer: `altium`, `eagle`, `cadstar`, `pads`, ...; default `auto`), `--kind board|schematic` (for a file both could be: EasyEDA's
//! `.json`, a `.zip`, PADS' and LTspice's `.asc`), `--layer-map FILE` (a JSON object, source layer name to KiCad layer name, for the layers the importer's guess should not
//! decide; a board only), `--keep-kicad-layer-names` (a board from another format shows its layers by their standard names, not the names that application gave them),
//! `--report FILE|-`, `--no-pair`.
//!
//! # A board with a schematic
//!
//! KiCad's project import pairs them (see `crate::import_foreign`): an Eagle `.brd` and the `.sch` of the same name, a CADSTAR `.cpa` and its `.csa`, an Altium `.PrjPcb`
//! and the `.PcbDoc` and `.SchDoc`s it lists. Either file of a pair brings the other, unless `--no-pair`. The board is the design's board; the schematic is put in with the same
//! command the studio's File > Import > Non-KiCad Schematic uses (`Cmd::ReplaceSchematic`), so the nets the drawing says are the design's netlist.

use crate::import_foreign as foreign;
use eda_kicad_engine::foreign::{ForeignImport, ForeignOptions, Kind};
use eda_model::sheet_file::SheetFile;
use eda_model::CheckResult;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

fn flag(rest: &[String], name: &str) -> Option<String> {
    rest.iter().position(|a| a == name).and_then(|i| rest.get(i + 1)).cloned()
}

fn fail(check: &str, what: &str, msg: impl Into<String>) -> Vec<CheckResult> {
    vec![CheckResult::fail(check, what, msg)]
}

/// Same shape as `board::Meta` (kept private there), duplicated rather
/// than exposed across the module boundary just for this one writer.
#[derive(serde::Serialize)]
struct Meta {
    intent: String,
    snap_nm: i64,
    spacing_nm: i64,
}

const USAGE: &str = "usage: eda import-kicad <board.kicad_pcb | schematic.kicad_sch | file of another EDA format> -o <dir> [--format NAME] [--kind board|schematic] [--layer-map FILE] [--keep-kicad-layer-names] [--report FILE|-] [--no-pair]";

#[derive(Debug)]
struct Args {
    input: String,
    out: PathBuf,
    options: ForeignOptions,
    kind: Option<Kind>,
    report: Option<String>,
    pair: bool,
    /// The dialog's "Keep KiCad layer names" (`m_ImportKeepKiCadLayerNames`): a board from another format shows its layers by their standard names.
    keep_names: bool,
}

fn parse(rest: &[String]) -> Result<Args, Vec<CheckResult>> {
    let input = rest.first().filter(|a| !a.starts_with('-')).cloned().ok_or_else(|| fail("kicad_import_usage", "import-kicad", USAGE))?;
    let out: PathBuf = flag(rest, "-o").or_else(|| flag(rest, "--out")).map(PathBuf::from).ok_or_else(|| fail("kicad_import_usage", "import-kicad", "eda import-kicad needs -o <dir>"))?;
    let kind = match flag(rest, "--kind").as_deref() {
        None => None,
        Some("board" | "pcb") => Some(Kind::Board),
        Some("schematic" | "sch") => Some(Kind::Schematic),
        Some(other) => return Err(fail("kicad_import_usage", "--kind", format!("--kind is board or schematic, not '{other}'"))),
    };
    // The project kicad-cli makes of a board has the design rules and net classes its importer set (Eagle's, Altium's); the board file alone has none.
    let mut options = ForeignOptions { format: flag(rest, "--format"), project: true, ..Default::default() };
    if let Some(map) = flag(rest, "--layer-map") {
        let text = std::fs::read_to_string(&map).map_err(|e| fail("io", &map, format!("could not read the layer map {map}: {e}")))?;
        let parsed: Value = serde_json::from_str(&text).map_err(|e| fail("kicad_import_usage", &map, format!("the layer map is not JSON: {e}")))?;
        let object = parsed.as_object().ok_or_else(|| fail("kicad_import_usage", &map, "the layer map is a JSON object of source layer name to KiCad layer name"))?;
        for (from, to) in object {
            let to = to.as_str().ok_or_else(|| fail("kicad_import_usage", &map, format!("the layer map names '{from}' by something that is not a KiCad layer name")))?;
            options.layer_map.insert(from.clone(), to.to_string());
        }
    }
    Ok(Args { input, out, options, kind, report: flag(rest, "--report"), pair: !rest.iter().any(|a| a == "--no-pair"), keep_names: rest.iter().any(|a| a == "--keep-kicad-layer-names") })
}

/// What was converted, and for the report what kicad-cli said.
struct Sources {
    /// The board's file name (for the messages) and the `.kicad_pcb` text, and the folder a KiCad project's own files (`.kicad_pro`, `.kicad_dru`, library tables) are in.
    board: Option<(String, String, Option<PathBuf>)>,
    /// The `.kicad_pro` kicad-cli wrote for a board it converted: its design rules and net classes.
    project: Option<String>,
    /// A schematic's sheets: the name each is placed under and what it holds.
    sheets: Vec<(String, SheetFile)>,
    /// kicad-cli's runs, in the order they were made.
    imports: Vec<ForeignImport>,
}

fn stem_of(path: &Path) -> String {
    path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "sheet".to_string())
}

fn name_of(path: &Path) -> String {
    path.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default()
}

pub fn run(rest: &[String]) -> Result<(), Vec<CheckResult>> {
    let args = parse(rest)?;
    eda_kicad_engine::with_scratch(|work| {
        let sources = gather(&args, work)?;
        write(&args, sources)
    })
}

/// Read, or have kicad-cli convert, everything the input stands for.
fn gather(args: &Args, work: &Path) -> Result<Sources, Vec<CheckResult>> {
    let input = PathBuf::from(&args.input);
    let ext = input.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
    let mut s = Sources { board: None, project: None, sheets: Vec::new(), imports: Vec::new() };
    let mut runs = 0;
    let mut convert_one = |s: &mut Sources, path: &Path, kind: Option<Kind>| -> Result<ForeignImport, Vec<CheckResult>> {
        runs += 1;
        let import = foreign::convert(&work.join(format!("run{runs}")), path, kind, &args.options)?;
        report(&import, path);
        s.imports.push(import.clone());
        Ok(import)
    };
    match ext.as_str() {
        "kicad_pcb" => {
            let text = std::fs::read_to_string(&input).map_err(|e| fail("io", &args.input, format!("could not read {}: {e}", args.input)))?;
            let dir = std::fs::canonicalize(&input).ok().and_then(|p| p.parent().map(Path::to_path_buf));
            s.board = Some((name_of(&input), text, dir));
        }
        "kicad_sch" => {
            let file = eda_kicad::load_sheet_file(&input)?;
            s.sheets.push((stem_of(&input), file));
        }
        "prjpcb" => {
            let project = foreign::altium_project(&input)?;
            for gone in &project.missing {
                eprintln!("  the project lists {gone}, which is not there");
            }
            if project.boards.is_empty() && project.sheets.is_empty() {
                return Err(fail("altium_project_empty", &args.input, "the project lists no .PcbDoc and no .SchDoc that exists"));
            }
            if project.boards.len() > 1 {
                eprintln!("  the project lists {} boards; the first, {}, is imported", project.boards.len(), name_of(&project.boards[0]));
            }
            if let Some(board) = project.boards.first() {
                let import = convert_one(&mut s, board, Some(Kind::Board))?;
                s.board = Some((name_of(board), import.text.clone(), None));
                s.project = import.project.clone();
            }
            let mut parts = Vec::new();
            for sheet in &project.sheets {
                let import = convert_one(&mut s, sheet, Some(Kind::Schematic))?;
                parts.push((stem_of(sheet), foreign::sheet_file_of(&import)?));
            }
            s.sheets = parts;
        }
        _ => {
            let import = convert_one(&mut s, &input, args.kind)?;
            let kind = import.kind;
            put(&mut s, kind, &input, &import)?;
            if args.pair {
                if let Some((partner_kind, partner)) = foreign::partner_of(&input) {
                    if partner_kind != kind {
                        eprintln!("  {} is the {} of the same project (KiCad pairs them by name)", name_of(&partner), partner_kind.name());
                        let import = convert_one(&mut s, &partner, Some(partner_kind))?;
                        put(&mut s, partner_kind, &partner, &import)?;
                    }
                }
            }
        }
    }
    Ok(s)
}

fn put(s: &mut Sources, kind: Kind, path: &Path, import: &ForeignImport) -> Result<(), Vec<CheckResult>> {
    match kind {
        Kind::Board => {
            s.board = Some((name_of(path), import.text.clone(), None));
            s.project = import.project.clone();
        }
        Kind::Schematic => s.sheets.push((stem_of(path), foreign::sheet_file_of(import)?)),
    }
    Ok(())
}

/// What kicad-cli made of one file, to the terminal: the format and the counts, then the errors and the warnings.
fn report(import: &ForeignImport, path: &Path) {
    let counts: Vec<String> = import.report.statistics.iter().map(|(k, v)| format!("{v} {}", k.replace('_', " "))).collect();
    eprintln!("import-kicad: {} {} '{}' by {}: {}", import.report.source_format, import.kind.name(), name_of(path), import.engine, if counts.is_empty() { "no count in its report".to_string() } else { counts.join(", ") });
    let findings = import.findings();
    for f in findings.iter().take(20) {
        eprintln!("  {}: {}{}", f.severity.name(), f.message, if f.count > 1 { format!(" (x{})", f.count) } else { String::new() });
    }
    if findings.len() > 20 {
        eprintln!("  ... and {} more", findings.len() - 20);
    }
}

/// The board directory: the board (or an empty one), then the schematic over it.
fn write(args: &Args, sources: Sources) -> Result<(), Vec<CheckResult>> {
    let out = &args.out;
    std::fs::create_dir_all(out).map_err(|e| fail("io", &out.display().to_string(), e.to_string()))?;
    let intent_path = out.join("intent.yaml");

    let (design, model, notes, setup) = match &sources.board {
        Some((name, text, project_dir)) => {
            let (design, mut model, notes) = eda::import_kicad_pcb(text)?;
            if args.keep_names && project_dir.is_none() {
                model.board.layer_setup.names.clear();
            }
            let mut setup = None;
            // A real project's net classes live in the sidecar `.kicad_pro`, not the `.kicad_pcb` itself (see `eda_kicad::merge_project_net_classes`'s doc comment) -- merge
            // it in when the board came from a real project directory rather than a bare, single-file board. (A board kicad-cli made from another format has no project.)
            if project_dir.is_some() {
                let board_file = PathBuf::from(&args.input);
                let pro_path = board_file.with_extension("kicad_pro");
                if let Ok(pro_text) = std::fs::read_to_string(&pro_path) {
                    eda::merge_project_net_classes(&mut model, &pro_text);
                    // Same sidecar project, its per-DRC-type severity table (task item 2) -- see `eda_kicad::merge_project_rule_severities`'s doc comment for why this is a
                    // wholesale replacement, not a merge.
                    eda::merge_project_rule_severities(&mut model, &pro_text);
                    // Board-wide minimum constraints (`rules.min_*`, task item 1) -- distinct from the net classes merged just above, which only ever carry nominal/default
                    // values, never a DRC minimum. See `eda_kicad::merge_project_design_rules`'s doc comment (GAPS.md #10).
                    eda::merge_project_design_rules(&mut model, &pro_text);
                    // Board Setup's smaller pages: Formatting's footprint boxes, the Length-tuning Patterns, the Tuning Profiles and the profile each net class names.
                    eda::merge_project_setup_pages(&mut model, &pro_text);
                }
                // A `.kicad_dru` custom-rule file (task item 4) is its own sibling file, not a section inside `.kicad_pro`.
                if let Ok(dru_text) = std::fs::read_to_string(board_file.with_extension("kicad_dru")) {
                    eda::merge_custom_rules(&mut model, &dru_text);
                }
                // The project's own library tables and variables come along, so its libraries still resolve from the new folder (`library_table::import_project_setup`).
                setup = project_dir.as_ref().map(|dir| eda_kicad::library_table::import_project_setup(dir, out, std::fs::read_to_string(&pro_path).ok().as_deref()));
            }
            // A board kicad-cli converted has no project beside it, but kicad-cli made one with it: the rules and the net classes of the application the board came from.
            if let Some(pro) = &sources.project {
                crate::import_foreign::merge_project(&mut model, pro);
                if args.keep_names {
                    model.board.layer_setup.names.clear();
                }
            }
            let _ = name;
            (design, model, Some(notes), setup)
        }
        None => (empty_board(), eda_model::ConstraintModel::default(), None, None),
    };

    let intent_yaml = serde_yaml::to_string(&model).map_err(|e| fail("kicad_import_encode", "intent.yaml", e.to_string()))?;
    std::fs::write(&intent_path, intent_yaml).map_err(|e| fail("io", "intent.yaml", e.to_string()))?;
    let design_bytes = design.canonical_bytes().map_err(|e| fail("kicad_import_encode", "design.json", e.to_string()))?;
    std::fs::write(out.join("design.json"), design_bytes).map_err(|e| fail("io", "design.json", e.to_string()))?;
    // The board shows the fills the file was saved with (KiCad's own), as it does in KiCad: they are kept beside the design, not in it (`crate::fill_api`).
    if let Some(notes) = &notes {
        crate::fill_api::keep_imported(out, &design, notes.zone_fills.clone());
    }

    // `board::load` reads `meta.intent` as a plain path (no relativizing to the board dir), so it has to survive the board directory moving relative to the cwd it was
    // created from -- same as `eda board new`.
    let abs_intent = std::fs::canonicalize(&intent_path).unwrap_or(intent_path);
    let meta = Meta { intent: abs_intent.display().to_string(), snap_nm: model.solver.place_snap_nm, spacing_nm: model.solver.place_spacing_nm };
    std::fs::write(out.join("board.json"), serde_json::to_string_pretty(&meta).expect("meta encodes")).map_err(|e| fail("io", "board.json", e.to_string()))?;

    // The libraries kicad-cli made of the footprints and symbols it read, and the tables that name them, so "Update from Library" finds them from this folder.
    let mut libraries = 0;
    for import in &sources.imports {
        libraries += copy_libraries(import, out);
    }

    if let Some(notes) = &notes {
        let pl = design.placement.as_ref();
        let fp_count = pl.map_or(0, |p| p.footprints.len());
        let outline_pts = pl.map_or(0, |p| p.outline.len());
        let (tracks, vias) = design.routing.as_ref().map_or((0, 0), |r| (r.tracks.len(), r.vias.len()));
        eprintln!("import-kicad: {fp_count} footprint(s) ({} explicit footprint def(s)), {} net(s), {tracks} track(s), {vias} via(s) -> {}", model.footprints.len(), model.nets.len(), out.display());
        eprintln!("  outline: {} ({outline_pts} point(s){})", notes.outline_source, if notes.outline_open { ", did not close into a loop" } else { "" });
        if notes.outline_shapes > 0 {
            eprintln!("  {} Edge.Cuts item(s) kept as shapes (arcs, circles, rectangles, curves and cutouts stay what they are)", notes.outline_shapes);
        }
        for e in &notes.outline_errors {
            eprintln!("  board outline is malformed: {e}");
        }
        if !notes.unread_items.is_empty() {
            let kinds: Vec<String> = notes.unread_items.iter().map(|(kind, n)| format!("{kind} x{n}")).collect();
            eprintln!("  not read (no reader for these kinds of board item yet; they are not in the design nor in what is exported from it): {}", kinds.join(", "));
        }
        if notes.zones_skipped > 0 {
            eprintln!("  {} zone(s)/pour(s) skipped -- not imported (see report for a proposed model shape)", notes.zones_skipped);
        }
        if notes.non_rect_pad_shapes_approximated > 0 {
            eprintln!("  {} pad(s) with an unsupported shape (trapezoid/custom) approximated as rect", notes.non_rect_pad_shapes_approximated);
        }
    }
    if let Some(setup) = setup {
        if !setup.tables.is_empty() || setup.variables > 0 {
            eprintln!("  library setup: {} table(s) ({}), {} text variable(s) from the project; its libraries are found where they are", setup.tables.len(), setup.tables.join(", "), setup.variables);
        }
        for note in &setup.notes {
            eprintln!("  library setup: {note}");
        }
    }
    if libraries > 0 {
        eprintln!("  library setup: {libraries} library file(s) kicad-cli made of the footprints and symbols it read, with the tables that name them");
    }

    // The schematic, over the board: the command the studio's File > Import > Non-KiCad Schematic runs, so the nets are traced from the drawing by the same code.
    if !sources.sheets.is_empty() {
        let (name, content) = match sources.sheets.len() {
            1 => sources.sheets.into_iter().next().expect("one sheet"),
            n => {
                eprintln!("import-kicad: {n} sheets, placed on a root sheet as KiCad's Altium project import places them");
                ("project".to_string(), foreign::combine_sheets(sources.sheets))
            }
        };
        for note in &content.notes {
            eprintln!("  schematic: {note}");
        }
        let summary = crate::board::step(out, eda_ops::Cmd::ReplaceSchematic { content, name }, false, "import-kicad")?;
        // A fresh board has no history: what the import did is the board, not an edit of it.
        let _ = std::fs::remove_dir_all(out.join(".history"));
        eprintln!("import-kicad: schematic: {summary}");
    }

    if let Some(report) = &args.report {
        let doc = json!({
            "input": args.input,
            "imports": sources.imports.iter().map(ForeignImport::to_json).collect::<Vec<_>>(),
        });
        let text = serde_json::to_string_pretty(&doc).expect("report encodes");
        if report == "-" {
            println!("{text}");
        } else {
            std::fs::write(report, text).map_err(|e| fail("io", report, e.to_string()))?;
            eprintln!("  report: {report}");
        }
    }
    eprintln!("wrote {}", out.join("design.json").display());
    Ok(())
}

/// A board with an outline and nothing on it, for a schematic that has no board: the 100 by 80 mm a new project starts with.
fn empty_board() -> eda_model::ir::Design {
    use eda_model::ir::{Design, PlacementSection, Point, Provenance};
    let pt = |x, y| Point { x, y };
    Design {
        schema: eda_model::ir::SCHEMA_VERSION,
        provenance: Provenance { engine_version: env!("CARGO_PKG_VERSION").into(), intent_hash: String::new(), seed: 0, stage_hashes: vec![] },
        schematic: None,
        nets: None,
        placement: Some(PlacementSection { outline: vec![pt(0, 0), pt(100_000, 0), pt(100_000, 80_000), pt(0, 80_000)], footprints: vec![], modules: vec![] }),
        routing: None,
        drawings: None,
        footprint_library: None,
        sheet_contents: None,
        bus_aliases: vec![],
        symbol_library: None,
    }
}

/// Copy the libraries kicad-cli wrote beside what it converted (`<name>-import-fps.pretty`, `fp-lib-table`; `<name>-eagle-import.kicad_sym`, `sym-lib-table`) into the board's
/// folder, where the library tables are looked for. A file the folder has already is kept (a project with several sheets has one table per sheet; the first wins). How many
/// files were copied.
fn copy_libraries(import: &ForeignImport, out: &Path) -> usize {
    let Some(dir) = import.output.parent() else { return 0 };
    let mut n = 0;
    for rel in &import.files {
        let is_library = rel == "fp-lib-table" || rel == "sym-lib-table" || rel.ends_with(".kicad_sym") || rel.split('/').next().is_some_and(|top| top.ends_with(".pretty"));
        if !is_library {
            continue;
        }
        let to = out.join(rel);
        if to.exists() {
            continue;
        }
        if let Some(parent) = to.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if std::fs::copy(dir.join(rel), &to).is_ok() {
            n += 1;
        }
    }
    n
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A project that has library tables and variables of its own is imported with them: the libraries still resolve from the new board folder.
    #[test]
    fn the_projects_library_tables_and_variables_come_along() {
        let root = std::env::temp_dir().join(format!("eda-import-setup-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let (project, board) = (root.join("project"), root.join("board"));
        std::fs::create_dir_all(&project).unwrap();
        std::fs::write(project.join("b.kicad_pcb"), "(kicad_pcb (version 20240108) (generator \"test\") (general (thickness 1.6)))\n").unwrap();
        std::fs::write(project.join("fp-lib-table"), "(fp_lib_table\n\t(version 7)\n\t(lib (name \"Mine\") (type \"KiCad\") (uri \"${KIPRJMOD}/Mine.pretty\") (options \"\") (descr \"\"))\n)\n").unwrap();
        std::fs::write(project.join("b.kicad_pro"), r#"{ "text_variables": { "MYLIBS": "${KIPRJMOD}/libs" } }"#).unwrap();
        let args = [project.join("b.kicad_pcb").display().to_string(), "-o".to_string(), board.display().to_string()];
        if let Err(e) = run(&args) {
            panic!("import failed: {}", e.iter().map(|c| format!("{c:?}")).collect::<String>());
        }
        let folder = std::fs::canonicalize(&project).unwrap().display().to_string();
        let table = std::fs::read_to_string(board.join("fp-lib-table")).unwrap();
        assert!(table.contains(&format!("(uri \"{folder}/Mine.pretty\")")), "{table}");
        assert!(!board.join("sym-lib-table").exists(), "a project without a table brings none");
        let vars = eda_kicad::library_table::read_project_variables(&board);
        assert_eq!(vars.get("MYLIBS"), Some(&format!("{folder}/libs")));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn the_command_line_names_a_file_a_folder_and_what_kicad_cli_is_told() {
        let a = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        let got = parse(&a(&["b.brd", "-o", "out", "--format", "eagle", "--kind", "pcb", "--report", "-", "--no-pair", "--keep-kicad-layer-names"])).unwrap();
        assert_eq!((got.input.as_str(), got.out.to_str(), got.options.format.as_deref(), got.kind, got.report.as_deref(), got.pair, got.keep_names), ("b.brd", Some("out"), Some("eagle"), Some(Kind::Board), Some("-"), false, true));
        let plain = parse(&a(&["b.brd", "--out", "out"])).unwrap();
        assert!(plain.pair && plain.kind.is_none() && plain.options.format.is_none() && !plain.keep_names);
        assert_eq!(parse(&a(&["-o", "out"])).unwrap_err()[0].check, "kicad_import_usage", "a file first");
        assert_eq!(parse(&a(&["b.brd"])).unwrap_err()[0].check, "kicad_import_usage", "and a folder");
        assert_eq!(parse(&a(&["b.brd", "-o", "o", "--kind", "both"])).unwrap_err()[0].check, "kicad_import_usage");
        let dir = std::env::temp_dir().join(format!("eda_import_args_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("map.json"), r#"{ "tNames": "F.Fab", "Top": "F.Cu" }"#).unwrap();
        let mapped = parse(&a(&["b.brd", "-o", "o", "--layer-map", &dir.join("map.json").display().to_string()])).unwrap();
        assert_eq!(mapped.options.layer_map.get("tNames").map(String::as_str), Some("F.Fab"));
        std::fs::write(dir.join("bad.json"), r#"["tNames"]"#).unwrap();
        assert_eq!(parse(&a(&["b.brd", "-o", "o", "--layer-map", &dir.join("bad.json").display().to_string()])).unwrap_err()[0].check, "kicad_import_usage");
        std::fs::write(dir.join("bad2.json"), r#"{ "tNames": 3 }"#).unwrap();
        assert_eq!(parse(&a(&["b.brd", "-o", "o", "--layer-map", &dir.join("bad2.json").display().to_string()])).unwrap_err()[0].check, "kicad_import_usage");
        let _ = std::fs::remove_dir_all(&dir);
    }

    // ---------------------------------------------------------------- the slow tier: the real kicad-cli on KiCad's QA files

    fn qa(rel: &str) -> Option<PathBuf> {
        if std::env::var_os("EDA_SLOW_TESTS").is_none() {
            eprintln!("skipped: slow test; set EDA_SLOW_TESTS=1 to run it");
            return None;
        }
        if eda_kicad_engine::find_cli().is_none() {
            eprintln!("kicad-cli not found; skipping");
            return None;
        }
        let root = std::env::var_os("KICAD_QA_DATA").map(PathBuf::from).filter(|p| p.exists()).unwrap_or_else(|| PathBuf::from("/Users/juanantonioluera/ws/kicad-src-8303b2ad/qa/data"));
        let path = root.join(rel);
        assert!(path.is_file(), "no QA file {}", path.display());
        Some(path)
    }

    fn import(args: &[String]) {
        if let Err(e) = run(args) {
            panic!("import failed: {}", e.iter().map(|c| format!("{c:?}")).collect::<String>());
        }
    }

    fn args(input: &Path, out: &Path, more: &[&str]) -> Vec<String> {
        let mut a = vec![input.display().to_string(), "-o".to_string(), out.display().to_string()];
        a.extend(more.iter().map(|s| s.to_string()));
        a
    }

    fn scratch(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("eda_import_foreign_cli_{name}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    /// The symbols a design places, over the root and every screen.
    fn placed_symbols(d: &eda_model::ir::Design) -> usize {
        d.schematic.iter().chain(d.sheet_contents.iter().flat_map(|c| c.values())).map(|s| s.symbols.len() + s.power_symbols.len()).sum()
    }

    /// Eagle: the board and the schematic of one name are one project -- the board is the design's board, the schematic is drawn over it and its nets are the netlist --
    /// and the libraries kicad-cli made of the footprints and the symbols are in the folder with the tables that name them.
    #[test]
    fn an_eagle_board_brings_the_schematic_of_the_same_name_and_the_libraries_kicad_cli_made() {
        let Some(brd) = qa("pcbnew/plugins/eagle/Adafruit-AHT20-PCB/Adafruit AHT20 Temperature & Humidity.brd") else { return };
        let out = scratch("eagle");
        let report_file = scratch("eagle_report").with_extension("json");
        import(&args(&brd, &out, &["--report", &report_file.display().to_string()]));
        let (_, design, model) = crate::board::load(&out).expect("a board the studio opens");
        assert_eq!(design.placement.as_ref().unwrap().footprints.len(), 19, "the board's footprints, as kicad-cli's report counts them");
        assert_eq!(placed_symbols(&design), 36, "the schematic of the same name beside it: 36 symbols");
        assert!(design.nets.as_ref().is_some_and(|n| !n.is_empty()), "the drawing's nets are the netlist now");
        // The nets of the schematic reach the board's pads: the part U1 is on the board and on the sheet.
        assert!(model.part("U1").is_some() && model.nets.iter().any(|n| n.pins.iter().any(|p| p.starts_with("U1."))));
        // The design rules and the net classes Eagle's board has are in the project kicad-cli makes of it, not in the board file: the board is judged by them.
        assert!(model.board.net_classes.iter().any(|c| c.name == "highpower" && c.track_width == Some(304_800)), "{:?}", model.board.net_classes.iter().map(|c| &c.name).collect::<Vec<_>>());
        assert_eq!(model.board.min_clearance_nm, 203_200, "Eagle's 8 mil minimum clearance");
        assert!(out.join("fp-lib-table").is_file() && out.join("sym-lib-table").is_file(), "the tables that name the libraries");
        assert!(std::fs::read_dir(&out).unwrap().flatten().any(|e| e.file_name().to_string_lossy().ends_with("-import-fps.pretty")));
        assert!(std::fs::read_dir(&out).unwrap().flatten().any(|e| e.file_name().to_string_lossy().ends_with(".kicad_sym")));
        assert!(!out.join(".history").exists(), "a fresh board has no history: the import is the board, not an edit of it");
        // What kicad-cli said, as JSON: both files, the formats it recognised.
        let report: Value = serde_json::from_str(&std::fs::read_to_string(&report_file).unwrap()).unwrap();
        let formats: Vec<&str> = report["imports"].as_array().unwrap().iter().map(|i| i["format"].as_str().unwrap()).collect();
        assert_eq!(formats, vec!["Eagle", "EAGLE"], "the board, then its schematic: {report}");
        assert_eq!(report["imports"][0]["statistics"]["footprints"], 19);
        assert_eq!(report["imports"][1]["statistics"]["symbols"], 36);
        let _ = std::fs::remove_file(&report_file);

        // The schematic alone, from the other file of the pair, brings the board; --no-pair keeps to the one file.
        let sch = brd.with_extension("sch");
        let from_sch = scratch("eagle_from_sch");
        import(&args(&sch, &from_sch, &[]));
        let (_, d2, _) = crate::board::load(&from_sch).unwrap();
        assert_eq!((d2.placement.as_ref().unwrap().footprints.len(), placed_symbols(&d2)), (19, 36));
        let alone = scratch("eagle_alone");
        import(&args(&brd, &alone, &["--no-pair"]));
        let (_, d3, _) = crate::board::load(&alone).unwrap();
        assert_eq!((d3.placement.as_ref().unwrap().footprints.len(), d3.schematic.is_none()), (19, true), "board only");
        for d in [&out, &from_sch, &alone] {
            let _ = std::fs::remove_dir_all(d);
        }
    }

    /// Altium: the project lists a board and two sheets; the sheets are placed on a root as the Altium loader of KiCad places them, and the board is the design's board.
    #[test]
    fn an_altium_project_brings_its_board_and_places_its_sheets_on_a_root() {
        let Some(prj) = qa("pcbnew/plugins/altium/eDP_adapter_dvt1_source/eDP_adapter_dvt1.PrjPcb") else { return };
        let out = scratch("altium");
        import(&args(&prj, &out, &[]));
        let (_, design, model) = crate::board::load(&out).expect("a board the studio opens");
        assert_eq!(design.placement.as_ref().unwrap().footprints.len(), 115);
        assert_eq!(model.board.min_clearance_nm, 101_600, "Altium's clearance rule (4 mil), from the project kicad-cli made with the board");
        let root = design.schematic.as_ref().expect("a schematic");
        let names: Vec<(&str, &str)> = root.sheets.iter().map(|s| (s.name.as_str(), s.page.as_str())).collect();
        assert_eq!(names, vec![("it6251_core", "2"), ("power", "3")], "in the order the project lists them, from page 2");
        assert_eq!(placed_symbols(&design), 124 + 49, "every symbol of both sheets");
        assert_eq!(design.sheet_contents.as_ref().map(|c| c.len()), Some(2));
        let _ = std::fs::remove_dir_all(&out);
    }

    /// A schematic alone is a board with nothing on it and the drawing; a layer map moves a layer of a board, and the report is written.
    #[test]
    fn a_schematic_alone_and_a_board_with_a_layer_map() {
        let Some(sch) = qa("pcbnew/plugins/altium/eDP_adapter_dvt1_source/power.SchDoc") else { return };
        let out = scratch("sch_only");
        import(&args(&sch, &out, &[]));
        let (_, design, _) = crate::board::load(&out).unwrap();
        assert_eq!((design.placement.as_ref().unwrap().footprints.len(), placed_symbols(&design)), (0, 49));
        let _ = std::fs::remove_dir_all(&out);

        let brd = qa("pcbnew/plugins/eagle/test_eagle_23016/test_eagle.brd").unwrap();
        let dir = scratch("layer_map");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("map.json"), r#"{ "tNames": "F.Fab" }"#).unwrap();
        let report = dir.join("report.json");
        import(&args(&brd, &dir.join("board"), &["--layer-map", &dir.join("map.json").display().to_string(), "--report", &report.display().to_string()]));
        let doc: Value = serde_json::from_str(&std::fs::read_to_string(&report).unwrap()).unwrap();
        let layers = doc["imports"][0]["layers"].as_array().unwrap();
        assert!(layers.contains(&json!({ "source": "tNames", "kicad": "F.Fab", "explicit": true })), "{layers:?}");
        assert!(layers.contains(&json!({ "source": "Top", "kicad": "F.Cu", "explicit": false })));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A `.json` is EasyEDA's board or its schematic: tried as a board, and, when no importer takes it so, as a schematic.
    #[test]
    fn a_file_both_kinds_could_be_is_tried_as_a_board_and_then_as_a_schematic() {
        let Some(sch) = qa("pcbnew/plugins/easyeda/SCH_ESP32-PICO-D4 smart watch_2023-09-02.json") else { return };
        let out = scratch("easyeda_sch");
        import(&args(&sch, &out, &[]));
        let (_, design, _) = crate::board::load(&out).unwrap();
        assert_eq!((design.placement.as_ref().unwrap().footprints.len(), placed_symbols(&design)), (0, 135), "the schematic json is a schematic");
        let pcb = qa("pcbnew/plugins/easyeda/PCB_PCB_ESP32-PICO-D4 smart watch_2023-09-02.json").unwrap();
        let out2 = scratch("easyeda_pcb");
        import(&args(&pcb, &out2, &[]));
        let (_, d2, _) = crate::board::load(&out2).unwrap();
        assert_eq!((d2.placement.as_ref().unwrap().footprints.len(), d2.schematic.is_none()), (70, true), "and the pcb json is a board");
        // Said the other way round, the kind is forced.
        let e = run(&args(&sch, &scratch("easyeda_forced"), &["--kind", "board"])).unwrap_err();
        assert!(eda_kicad_engine::foreign::is_unrecognized(&e), "{e:?}");
        for d in [&out, &out2] {
            let _ = std::fs::remove_dir_all(d);
        }
    }

    /// An Eagle schematic of two sheets that none places: kicad-cli writes a file each and reaches only the first; both are read, and placed on a root as an Altium project's sheets are.
    /// The board of the same name beside it comes with it (KiCad's project import pairs them by name).
    #[test]
    fn a_schematic_of_several_top_level_sheets_is_read_whole_and_its_board_comes_with_it() {
        let Some(sch) = qa("eeschema/io/eagle/eagle-import-testfile.sch") else { return };
        let out = scratch("eagle_two_sheets");
        import(&args(&sch, &out, &[]));
        let (_, design, _) = crate::board::load(&out).expect("a board the studio opens");
        assert_eq!(placed_symbols(&design), 24, "20 on the first sheet and 4 on the second: kicad-cli's report counts 24");
        let root = design.schematic.as_ref().unwrap();
        assert_eq!(root.sheets.len(), 2, "two sheets placed on a root: {:?}", root.sheets.iter().map(|s| &s.name).collect::<Vec<_>>());
        assert_eq!(design.sheet_contents.as_ref().map(|c| c.len()), Some(2));
        assert!(design.placement.as_ref().unwrap().footprints.len() > 3, "the board of the same name: {}", design.placement.as_ref().unwrap().footprints.len());
        let _ = std::fs::remove_dir_all(&out);
    }
}
