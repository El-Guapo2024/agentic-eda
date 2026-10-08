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

/// What each example has today, (flat, module sheets). Every step of the cleanup lowers these; they end at zero.
///
/// Before the cleanup began (fields of every symbol stacked at the page's corner, labels reading over pin names, generated symbols with
/// their pins one grid apart): all_power_ground_net 46 / 49, dense_small_outline 36 / 30, l1_usb_mcu 2147 / 957, l2_sensor_hub 5298 / 1414,
/// l3_motor_hub 15262 / 2238, l4_control_hub 37401 / 3617, ldo 142 / 140, ldo_proximity_heavy 217 / 167, mcu_board_30plus 3100 / 1536,
/// mixed_track_widths 142 / 140, nc_pins 67 / 68, opamp_filter 153 / 168, passive_divider_ladder 91 / 86, star_net 51 / 42,
/// through_hole_headers 104 / 94, two_pin_nets 14 / 12, unroutable_tiny_outline 56 / 56.
const KNOWN: &[(&str, usize, usize)] = &[
    ("all_power_ground_net.yaml", 2, 0),
    ("dense_small_outline.yaml", 0, 0),
    ("l1_usb_mcu.yaml", 58, 0),
    ("l2_sensor_hub.yaml", 163, 0),
    ("l3_motor_hub.yaml", 256, 0),
    ("l4_control_hub.yaml", 441, 0),
    ("ldo.yaml", 16, 0),
    ("ldo_proximity_heavy.yaml", 10, 0),
    ("mcu_board_30plus.yaml", 90, 0),
    ("mixed_track_widths.yaml", 16, 0),
    ("nc_pins.yaml", 7, 0),
    ("opamp_filter.yaml", 10, 0),
    ("passive_divider_ladder.yaml", 1, 0),
    ("star_net.yaml", 0, 0),
    ("through_hole_headers.yaml", 18, 0),
    ("two_pin_nets.yaml", 0, 0),
    ("unroutable_tiny_outline.yaml", 4, 0),
];

#[test]
fn no_overlap_on_any_example_flat_or_as_module_sheets() {
    let mut off = Vec::new();
    for f in example_files() {
        let name = f.file_name().unwrap().to_string_lossy().to_string();
        let Some((flat, modules)) = measure(&f) else { continue };
        let (kflat, kmod) = KNOWN.iter().find(|(n, _, _)| *n == name).map(|(_, a, b)| (*a, *b)).unwrap_or((0, 0));
        if flat != kflat || modules != kmod {
            off.push(format!("{name}: flat {flat} (KNOWN says {kflat}), module sheets {modules} (KNOWN says {kmod})"));
        }
    }
    assert!(off.is_empty(), "the findings changed; a lower number is progress, so write it into KNOWN:\n{}", off.join("\n"));
}
