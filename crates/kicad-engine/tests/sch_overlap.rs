//! Nothing on a derived sheet overlaps anything else, as KiCad draws it: every example and `mcu_board_30plus` (the board
//! the studio shows), derived flat and as module sheets, written as `.kicad_sch` files and measured with `eda_kicad::sch_overlap`
//! (the bounding-box rules of eeschema, ported). No kicad-cli is needed.

use eda_engine::{derive_schematic, derive_schematic_modules, EngineOptions};
use eda_kicad::sch_overlap::{check_tree, SheetReport};
use eda_kicad::{export_kicad_sch_tree, ExportMeta};
use eda_model::ir::Design;
use eda_model::ConstraintModel;
use std::path::{Path, PathBuf};

fn examples_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples")
}

/// Every intent under `examples/` (and `examples/ladder/`), sorted.
fn example_files() -> Vec<PathBuf> {
    let dir = examples_dir();
    let mut files: Vec<PathBuf> = std::fs::read_dir(&dir).unwrap().flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|e| e == "yaml")).collect();
    files.extend(std::fs::read_dir(dir.join("ladder")).into_iter().flatten().flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|e| e == "yaml")));
    files.sort();
    files
}

/// The model of an intent, with the symbols of the installed KiCad libraries resolved when KiCad is installed (as the studio does),
/// else the built-in ones.
fn model_of(path: &Path) -> Option<ConstraintModel> {
    let mut model: ConstraintModel = serde_yaml::from_str(&std::fs::read_to_string(path).ok()?).ok()?;
    let root = eda_kicad::default_symbol_library_root();
    if root.is_dir() {
        let _ = eda_kicad::resolve_library_symbols(&mut model, &root);
    }
    Some(model)
}

/// With `EDA_SCH_DUMP=<dir>` the exported sheets are left in `<dir>/<example>/<how>/`, to be rendered with kicad-cli.
fn dump(name: &str, how: &str, files: &[(String, String)]) {
    let Some(dir) = std::env::var_os("EDA_SCH_DUMP") else { return };
    let dir = Path::new(&dir).join(name.trim_end_matches(".yaml")).join(how.replace(' ', "_"));
    std::fs::create_dir_all(&dir).expect("the dump directory");
    for (file, text) in files {
        std::fs::write(dir.join(file), text).expect("a sheet is written");
    }
}

fn reports_of(name: &str, how: &str, design: &Design, model: &ConstraintModel) -> Vec<SheetReport> {
    let files = export_kicad_sch_tree(design, model, &ExportMeta { date: "2026-10-08", title: "t" }, "root.kicad_sch").expect("the design exports");
    dump(name, how, &files);
    check_tree(&files).expect("the exported sheets read back")
}

fn summary(reports: &[SheetReport]) -> usize {
    reports.iter().map(SheetReport::count).sum()
}

fn print(name: &str, how: &str, reports: &[SheetReport]) {
    println!("{name} ({how}): {} finding(s) on {} sheet(s)", summary(reports), reports.len());
    for r in reports.iter().filter(|r| r.count() > 0) {
        print!("{}", r.render());
    }
}

/// `examples/<file>` -> (flat, module sheets), findings counted.
fn measure(path: &Path) -> Option<(usize, usize)> {
    let model = model_of(path)?;
    let name = path.file_name()?.to_string_lossy().to_string();
    let opts = EngineOptions::new(1, "t");
    let flat = derive_schematic(&model, &opts).ok().map(|d| reports_of(&name, "flat", &d, &model));
    let modules = derive_schematic_modules(&model, &opts).ok().map(|d| reports_of(&name, "module sheets", &d, &model));
    if let Some(r) = &flat {
        print(&name, "flat", r);
    }
    if let Some(r) = &modules {
        print(&name, "module sheets", r);
    }
    Some((flat.as_deref().map(summary).unwrap_or(0), modules.as_deref().map(summary).unwrap_or(0)))
}

/// Before the cleanup began (fields of every symbol stacked at the page's corner, labels reading over pin names, generated symbols with
/// their pins one grid apart) the findings, flat / module sheets, were: all_power_ground_net 46 / 49, dense_small_outline 36 / 30,
/// l1_usb_mcu 2147 / 957, l2_sensor_hub 5298 / 1414, l3_motor_hub 15262 / 2238, l4_control_hub 37401 / 3617, ldo 142 / 140,
/// ldo_proximity_heavy 217 / 167, mcu_board_30plus 3100 / 1536, mixed_track_widths 142 / 140, nc_pins 67 / 68, opamp_filter 153 / 168,
/// passive_divider_ladder 91 / 86, star_net 51 / 42, through_hole_headers 104 / 94, two_pin_nets 14 / 12, unroutable_tiny_outline 56 / 56.
/// Now there are none.
#[test]
fn no_overlap_on_any_example_flat_or_as_module_sheets() {
    let mut off = Vec::new();
    for f in example_files() {
        let name = f.file_name().unwrap().to_string_lossy().to_string();
        let Some((flat, modules)) = measure(&f) else { continue };
        if flat != 0 || modules != 0 {
            off.push(format!("{name}: {flat} finding(s) flat, {modules} as module sheets"));
        }
    }
    assert!(off.is_empty(), "something on a derived sheet overlaps something else (run with --nocapture for the list, EDA_SCH_DUMP=<dir> to keep the sheets and render them with kicad-cli):\n{}", off.join("\n"));
}
