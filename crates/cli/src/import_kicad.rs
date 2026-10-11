//! `eda import-kicad <board.kicad_pcb> -o <dir>` -- read a real KiCad board
//! into a fresh `eda board` directory (`design.json` + `intent.yaml` +
//! `board.json`, the same shape `eda board new` writes -- see
//! `crate::board::Meta`), so `eda board status/serve/check` can open it
//! straight away.
//!
//! ```text
//! eda import-kicad templates/Arduino/Arduino_Uno_R3.kicad_pcb -o work/uno
//! eda board status -C work/uno
//! eda board serve  -C work/uno
//! ```

use eda_model::CheckResult;
use std::path::PathBuf;

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
    snap_um: i64,
    spacing_um: i64,
}

pub fn run(rest: &[String]) -> Result<(), Vec<CheckResult>> {
    let input = rest
        .first()
        .cloned()
        .ok_or_else(|| fail("kicad_import_usage", "import-kicad", "usage: eda import-kicad <board.kicad_pcb> -o <dir>"))?;
    let out: PathBuf = flag(rest, "-o")
        .or_else(|| flag(rest, "--out"))
        .map(PathBuf::from)
        .ok_or_else(|| fail("kicad_import_usage", "import-kicad", "eda import-kicad needs -o <dir>"))?;

    let text = std::fs::read_to_string(&input).map_err(|e| fail("io", &input, format!("could not read {input}: {e}")))?;
    let (design, mut model, notes) = eda::import_kicad_pcb(&text)?;
    // A real project's net classes live in the sidecar `.kicad_pro`, not
    // the `.kicad_pcb` itself (see `eda_kicad::merge_project_net_classes`'s
    // doc comment) -- merge it in when the board came from a real project
    // directory rather than a bare, single-file board.
    let pro_path = PathBuf::from(&input).with_extension("kicad_pro");
    if let Ok(pro_text) = std::fs::read_to_string(&pro_path) {
        eda::merge_project_net_classes(&mut model, &pro_text);
        // Same sidecar project, its per-DRC-type severity table (task item
        // 2) -- see `eda_kicad::merge_project_rule_severities`'s doc
        // comment for why this is a wholesale replacement, not a merge.
        eda::merge_project_rule_severities(&mut model, &pro_text);
        // Board-wide minimum constraints (`rules.min_*`, task item 1) --
        // distinct from the net classes merged just above, which only ever
        // carry nominal/default values, never a DRC minimum. See
        // `eda_kicad::merge_project_design_rules`'s doc comment (GAPS.md #10).
        eda::merge_project_design_rules(&mut model, &pro_text);
    }
    // A `.kicad_dru` custom-rule file (task item 4) is its own sibling
    // file, not a section inside `.kicad_pro`.
    let dru_path = PathBuf::from(&input).with_extension("kicad_dru");
    if let Ok(dru_text) = std::fs::read_to_string(&dru_path) {
        eda::merge_custom_rules(&mut model, &dru_text);
    }

    std::fs::create_dir_all(&out).map_err(|e| fail("io", &out.display().to_string(), e.to_string()))?;

    // The project's own library tables and variables come along, so its libraries still resolve from the new folder (`library_table::import_project_setup`).
    let source_dir = std::fs::canonicalize(&input).ok().and_then(|p| p.parent().map(std::path::Path::to_path_buf));
    let setup = source_dir.map(|dir| eda_kicad::library_table::import_project_setup(&dir, &out, std::fs::read_to_string(&pro_path).ok().as_deref()));

    let intent_path = out.join("intent.yaml");
    let intent_yaml = serde_yaml::to_string(&model).map_err(|e| fail("kicad_import_encode", "intent.yaml", e.to_string()))?;
    std::fs::write(&intent_path, intent_yaml).map_err(|e| fail("io", "intent.yaml", e.to_string()))?;

    let design_bytes = design.canonical_bytes().map_err(|e| fail("kicad_import_encode", "design.json", e.to_string()))?;
    std::fs::write(out.join("design.json"), design_bytes).map_err(|e| fail("io", "design.json", e.to_string()))?;

    // `board::load` reads `meta.intent` as a plain path (no relativizing to
    // the board dir), so it has to survive the board directory moving
    // relative to the cwd it was created from -- same as `eda board new`.
    let abs_intent = std::fs::canonicalize(&intent_path).unwrap_or(intent_path);
    let meta = Meta { intent: abs_intent.display().to_string(), snap_um: model.solver.place_snap_um, spacing_um: model.solver.place_spacing_um };
    std::fs::write(out.join("board.json"), serde_json::to_string_pretty(&meta).expect("meta encodes")).map_err(|e| fail("io", "board.json", e.to_string()))?;

    let pl = design.placement.as_ref();
    let fp_count = pl.map_or(0, |p| p.footprints.len());
    let outline_pts = pl.map_or(0, |p| p.outline.len());
    let (tracks, vias) = design.routing.as_ref().map_or((0, 0), |r| (r.tracks.len(), r.vias.len()));
    eprintln!(
        "import-kicad: {fp_count} footprint(s) ({} explicit footprint def(s)), {} net(s), {tracks} track(s), {vias} via(s) -> {}",
        model.footprints.len(),
        model.nets.len(),
        out.display()
    );
    eprintln!(
        "  outline: {} ({outline_pts} point(s){})",
        notes.outline_source,
        if notes.outline_open { ", did not close into a loop" } else { "" }
    );
    if notes.outline_shapes > 0 {
        eprintln!("  {} Edge.Cuts item(s) kept as shapes (arcs, circles, rectangles, curves and cutouts stay what they are)", notes.outline_shapes);
    }
    for e in &notes.outline_errors {
        eprintln!("  board outline is malformed: {e}");
    }
    if notes.zones_skipped > 0 {
        eprintln!("  {} zone(s)/pour(s) skipped -- not imported (see report for a proposed model shape)", notes.zones_skipped);
    }
    if notes.non_rect_pad_shapes_approximated > 0 {
        eprintln!("  {} pad(s) with an unsupported shape (trapezoid/custom) approximated as rect", notes.non_rect_pad_shapes_approximated);
    }
    if let Some(setup) = setup {
        if !setup.tables.is_empty() || setup.variables > 0 {
            eprintln!("  library setup: {} table(s) ({}), {} text variable(s) from the project; its libraries are found where they are", setup.tables.len(), setup.tables.join(", "), setup.variables);
        }
        for note in &setup.notes {
            eprintln!("  library setup: {note}");
        }
    }
    eprintln!("wrote {}", out.join("design.json").display());
    Ok(())
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
}
