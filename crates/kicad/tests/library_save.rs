//! Saving the library editors' symbols and footprints into the libraries of the tables (`eda_kicad::library_save`): a `.kicad_sym` file library, a
//! KiCad 10 `.kicad_symdir` folder library and a `.pretty` folder, each made from KiCad's own installed libraries.
//!
//! Each test saves an edited symbol or footprint, a new one and (for symbols) a new derived one, and reads the library back with this workspace's
//! readers: what was saved is what reads back (the writer's text of the two is the same), and the symbols that were not saved keep their text. The
//! slow tier (`EDA_SLOW_TESTS=1`, kicad-cli) then has kicad-cli load the saved library and plot each symbol or footprint (`sym export svg`,
//! `fp export svg`), and plot a symbol that was kept exactly as it plots from KiCad's own library.
//!
//! The installed libraries are found as the studio finds them (`KICAD_PATH`, `eda_kicad::default_symbol_library_root`); without them a test says so and
//! passes.

use eda_kicad::library_save::{copy_footprint_library, delete_footprints, save_footprint, save_symbol_library_as, save_symbols, save_symbols_deleting, SymbolSave};
use eda_model::embedded_files::EmbeddedFiles;
use eda_model::ir::{LibraryFootprint, LibrarySymbol};
use std::path::{Path, PathBuf};
use std::process::Command;

fn scratch(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("eda-library-save-it-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn installed_device() -> Option<PathBuf> {
    let dir = eda_kicad::default_symbol_library_root().join("Device.kicad_symdir");
    if dir.is_dir() {
        Some(dir)
    } else {
        eprintln!("skipped: no Device.kicad_symdir under {} (set KICAD_PATH)", eda_kicad::default_symbol_library_root().display());
        None
    }
}

fn installed_pretty(name: &str) -> Option<PathBuf> {
    let dir = eda_kicad::default_footprint_library_root().join(format!("{name}.pretty"));
    if dir.is_dir() {
        Some(dir)
    } else {
        eprintln!("skipped: no {name}.pretty under {} (set KICAD_PATH)", eda_kicad::default_footprint_library_root().display());
        None
    }
}

fn slow_tests_on() -> bool {
    let on = std::env::var_os("EDA_SLOW_TESTS").is_some();
    if !on {
        eprintln!("kicad-cli part skipped: set EDA_SLOW_TESTS=1 to run it");
    }
    on
}

fn find_kicad_cli() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("EDA_KICAD_CLI").map(PathBuf::from).filter(|p| p.exists()) {
        return Some(p);
    }
    let out = Command::new("which").arg("kicad-cli").output().ok()?;
    let path = String::from_utf8_lossy(&out.stdout).trim().to_string();
    (out.status.success() && !path.is_empty()).then(|| PathBuf::from(path))
}

/// The library's symbol `name` as the editor opens it (`parse_symbol`: a derived one flattened), named `lib:name`.
fn symbol_of(text: &str, lib: &str, name: &str) -> LibrarySymbol {
    let summaries = eda_kicad::scan_symbols(text);
    let mut s = eda_kicad::parse_symbol(text, &summaries, name).unwrap_or_else(|| panic!("{name} reads"));
    s.lib_id = format!("{lib}:{name}");
    s.assign_missing_ids();
    s
}

/// The one-symbol `.kicad_sym` text this workspace's writer makes of `sym` under its bare name: two symbols that write the same are the same symbol.
fn written(sym: &LibrarySymbol) -> String {
    let mut s = sym.clone();
    s.lib_id = eda_model::sym_fields::bare_name(&sym.lib_id).to_string();
    s.published = false;
    eda_kicad::export_kicad_sym(&s)
}

/// The symbol's form in a library text, cut out as it stands.
fn form_of(text: &str, name: &str) -> String {
    let s = eda_kicad::scan_symbols(text).into_iter().find(|s| s.name == name).unwrap_or_else(|| panic!("{name} is in the library"));
    eda_kicad::symbol_text(text, &s).to_string()
}

/// A new derived symbol of `parent`: its own fields, everything else its parent's (`(extends ..)`).
fn derived(lib: &str, name: &str, parent: &str) -> LibrarySymbol {
    let mut kid = LibrarySymbol::new_empty(&format!("{lib}:{name}"));
    kid.extends = Some(format!("{lib}:{parent}"));
    kid.fields = vec![eda_model::sym_fields::LibrarySymbolField::named("Reference", "R"), eda_model::sym_fields::LibrarySymbolField::named("Value", name)];
    kid.description = format!("{name}, derived from {parent}");
    kid.assign_missing_ids();
    kid
}

/// What `sym export svg` draws of `symbol` in `library`, with what names the file (its title) taken out.
fn plot_symbol(cli: &Path, library: &Path, symbol: &str, out: &Path) -> String {
    let _ = std::fs::remove_dir_all(out);
    std::fs::create_dir_all(out).unwrap();
    let run = Command::new(cli).args(["sym", "export", "svg", "--symbol", symbol, "-o"]).arg(out).arg(library).output().expect("runs kicad-cli");
    assert!(run.status.success(), "kicad-cli sym export svg {symbol} of {}: {}{}", library.display(), String::from_utf8_lossy(&run.stdout), String::from_utf8_lossy(&run.stderr));
    let svg = std::fs::read_dir(out).unwrap().filter_map(|e| e.ok()).map(|e| e.path()).find(|p| p.extension().is_some_and(|x| x == "svg")).unwrap_or_else(|| panic!("kicad-cli drew {symbol}"));
    std::fs::read_to_string(svg).unwrap().lines().filter(|l| !l.contains("<title>") && !l.contains("<desc>")).collect::<Vec<_>>().join("\n")
}

fn plot_footprint(cli: &Path, library: &Path, footprint: &str, out: &Path) {
    let _ = std::fs::remove_dir_all(out);
    std::fs::create_dir_all(out).unwrap();
    let run = Command::new(cli).args(["fp", "export", "svg", "--layers", "F.Cu,F.SilkS,F.Fab", "--footprint", footprint, "-o"]).arg(out).arg(library).output().expect("runs kicad-cli");
    assert!(run.status.success(), "kicad-cli fp export svg {footprint} of {}: {}{}", library.display(), String::from_utf8_lossy(&run.stdout), String::from_utf8_lossy(&run.stderr));
    assert!(out.join(format!("{footprint}.svg")).is_file(), "kicad-cli drew {footprint}");
}

/// Edits the way the Symbol Editor makes them: a description, and a pin more.
fn edit(sym: &mut LibrarySymbol) {
    sym.description = "Edited in the studio".into();
    let mut pin = sym.pins[0].clone();
    pin.id.clear();
    pin.number = "99".into();
    pin.name = "EXTRA".into();
    pin.at.x += 5.08;
    sym.pins.push(pin);
    sym.assign_missing_ids();
}

#[test]
fn symbols_saved_into_a_file_library_read_back_and_kicad_cli_loads_it() {
    let Some(device) = installed_device() else { return };
    let dir = scratch("file");
    // a file library of four of KiCad's symbols, one derived from another of them, as `read_symbol_library_text` joins them
    let pick = dir.join("pick.kicad_symdir");
    std::fs::create_dir(&pick).unwrap();
    for f in ["R", "C", "C_Feedthrough", "Filter_EMI_C"] {
        std::fs::copy(device.join(format!("{f}.kicad_sym")), pick.join(format!("{f}.kicad_sym"))).unwrap();
    }
    let lib = dir.join("Mine.kicad_sym");
    let original = eda_kicad::read_symbol_library_text(&pick).unwrap();
    std::fs::write(&lib, &original).unwrap();

    let mut c = symbol_of(&original, "Mine", "C");
    edit(&mut c);
    let mut beta = symbol_of(&original, "Mine", "R");
    beta.lib_id = "Mine:Beta".into();
    let kid = derived("Mine", "Kid", "R");
    save_symbols(&lib, &[SymbolSave { symbol: &c, embedded: None }, SymbolSave { symbol: &beta, embedded: None }, SymbolSave { symbol: &kid, embedded: None }]).unwrap();

    let saved = std::fs::read_to_string(&lib).unwrap();
    for kept in ["R", "C_Feedthrough", "Filter_EMI_C"] {
        assert_eq!(form_of(&saved, kept), form_of(&original, kept), "{kept} is kept as the file had it");
    }
    assert_eq!(written(&symbol_of(&saved, "Mine", "C")), written(&c), "the edited C reads back as saved");
    assert_eq!(written(&symbol_of(&saved, "Mine", "Beta")), written(&beta), "the new symbol reads back as saved");
    assert!(form_of(&saved, "Kid").contains("(extends \"R\")"), "the new symbol stays derived");
    let back = eda_kicad::parse_library_symbols(&saved).unwrap();
    let kid_back = back.symbols.iter().find(|s| s.lib_id == "Kid").expect("Kid reads back");
    assert_eq!(kid_back.description, kid.description);
    // the library's order: roots by name, then what derives from them (`GetInheritanceDepth`)
    let order: Vec<String> = eda_kicad::scan_symbols(&saved).into_iter().map(|s| s.name).collect();
    assert_eq!(order, ["Beta", "C", "C_Feedthrough", "R", "Filter_EMI_C", "Kid"]);

    if !slow_tests_on() {
        return;
    }
    let Some(cli) = find_kicad_cli() else { return eprintln!("skipped: no kicad-cli") };
    for name in ["R", "C", "Beta", "Kid", "Filter_EMI_C"] {
        plot_symbol(&cli, &lib, name, &dir.join(format!("svg-{name}")));
    }
    assert_eq!(plot_symbol(&cli, &lib, "R", &dir.join("svg-r-ours")), plot_symbol(&cli, &device.join("R.kicad_sym"), "R", &dir.join("svg-r-kicad")), "a symbol kept plots as it does from KiCad's library");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn symbols_saved_into_a_folder_library_read_back_and_kicad_cli_loads_it() {
    let Some(device) = installed_device() else { return };
    let dir = scratch("folder");
    let lib = dir.join("Device.kicad_symdir");
    std::fs::create_dir(&lib).unwrap();
    for e in std::fs::read_dir(&device).unwrap().filter_map(|e| e.ok()) {
        std::fs::copy(e.path(), lib.join(e.file_name())).unwrap();
    }
    let before: Vec<(PathBuf, String)> = eda_kicad::symbol_library_files(&lib).into_iter().map(|f| (f.clone(), std::fs::read_to_string(&f).unwrap())).collect();
    let original = eda_kicad::read_symbol_library_text(&lib).unwrap();

    let mut r = symbol_of(&original, "Device", "R");
    edit(&mut r);
    let mut beta = symbol_of(&original, "Device", "C");
    beta.lib_id = "Device:Beta/2".into();
    let kid = derived("Device", "Kid", "C_Feedthrough");
    let w = save_symbols(&lib, &[SymbolSave { symbol: &r, embedded: None }, SymbolSave { symbol: &beta, embedded: None }, SymbolSave { symbol: &kid, embedded: None }]).unwrap();
    // the symbol's own file, a file of its own for each new one (`EscapeString( name, CTX_FILENAME )`), and no other
    assert_eq!(w.written, [lib.join("Beta{slash}2.kicad_sym"), lib.join("Kid.kicad_sym"), lib.join("R.kicad_sym")]);
    for (file, text) in &before {
        if !file.ends_with("R.kicad_sym") {
            assert_eq!(&std::fs::read_to_string(file).unwrap(), text, "{} is not written", file.display());
        }
    }
    let saved = eda_kicad::read_symbol_library_text(&lib).unwrap();
    assert_eq!(written(&symbol_of(&saved, "Device", "R")), written(&r), "the edited R reads back as saved");
    assert_eq!(written(&symbol_of(&saved, "Device", "Beta/2")), written(&beta), "the new symbol reads back as saved");
    assert!(form_of(&saved, "Kid").contains("(extends \"C_Feedthrough\")"));
    assert_eq!(eda_kicad::scan_symbols(&saved).len(), eda_kicad::scan_symbols(&original).len() + 2);

    // Save Library As: the folder, with an edit not yet saved, as one file
    let mut c = symbol_of(&saved, "Device", "C");
    edit(&mut c);
    let packed = dir.join("Packed.kicad_sym");
    save_symbol_library_as(Some(&lib), &packed, &[SymbolSave { symbol: &c, embedded: None }]).unwrap();
    let packed_text = std::fs::read_to_string(&packed).unwrap();
    assert_eq!(eda_kicad::scan_symbols(&packed_text).len(), eda_kicad::scan_symbols(&saved).len());
    assert_eq!(written(&symbol_of(&packed_text, "Packed", "C")), written(&c));
    assert_eq!(form_of(&packed_text, "L"), form_of(&saved, "L"));

    if !slow_tests_on() {
        return;
    }
    let Some(cli) = find_kicad_cli() else { return eprintln!("skipped: no kicad-cli") };
    for name in ["R", "Beta/2", "Kid"] {
        plot_symbol(&cli, &lib, name, &dir.join("svg"));
    }
    for name in ["C", "Kid", "Filter_EMI_C", "R"] {
        plot_symbol(&cli, &packed, name, &dir.join("svg-packed"));
    }
    assert_eq!(plot_symbol(&cli, &packed, "L", &dir.join("svg-l-ours")), plot_symbol(&cli, &device, "L", &dir.join("svg-l-kicad")), "a symbol kept plots as it does from KiCad's library");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn footprints_saved_into_a_pretty_folder_read_back_and_kicad_cli_loads_it() {
    let Some(installed) = installed_pretty("Resistor_SMD") else { return };
    let dir = scratch("pretty");
    let lib = dir.join("Mine.pretty");
    std::fs::create_dir(&lib).unwrap();
    for f in ["R_0603_1608Metric", "R_0805_2012Metric"] {
        std::fs::copy(installed.join(format!("{f}.kicad_mod")), lib.join(format!("{f}.kicad_mod"))).unwrap();
    }
    let kept = std::fs::read_to_string(lib.join("R_0805_2012Metric.kicad_mod")).unwrap();
    let open = |name: &str| -> LibraryFootprint {
        let text = std::fs::read_to_string(lib.join(format!("{name}.kicad_mod"))).unwrap();
        let mut fp = eda_kicad::parse_library_footprint(&text).unwrap().footprint;
        fp.name = format!("Mine:{name}");
        fp.assign_missing_ids();
        fp
    };
    let mut r = open("R_0603_1608Metric");
    r.description = "Edited in the studio".into();
    r.pads[0].size.0 = r.pads[0].size.0 + eda_model::units::Nm::from_mm(0.1);
    save_footprint(&lib, &r, &EmbeddedFiles::default()).unwrap();
    let mut beta = open("R_0805_2012Metric");
    beta.name = "Mine:Beta".into();
    save_footprint(&lib, &beta, &EmbeddedFiles::default()).unwrap();

    assert_eq!(std::fs::read_to_string(lib.join("R_0805_2012Metric.kicad_mod")).unwrap(), kept, "a footprint not saved is not written");
    let back = open("R_0603_1608Metric");
    let bare = |fp: &LibraryFootprint| {
        let mut f = fp.clone();
        f.name = f.name.rsplit(':').next().unwrap().to_string();
        eda_kicad::export_kicad_mod(&f)
    };
    assert_eq!(bare(&back), bare(&r), "the edited footprint reads back as saved");
    assert_eq!(bare(&open("Beta")), bare(&beta), "the new footprint reads back as saved");

    // Save Library As copies the library's files
    let copy = dir.join("Copy.pretty");
    std::fs::create_dir(&copy).unwrap();
    assert_eq!(copy_footprint_library(&lib, &copy).unwrap(), ["Beta", "R_0603_1608Metric", "R_0805_2012Metric"]);

    if !slow_tests_on() {
        return;
    }
    let Some(cli) = find_kicad_cli() else { return eprintln!("skipped: no kicad-cli") };
    for name in ["R_0603_1608Metric", "Beta", "R_0805_2012Metric"] {
        plot_footprint(&cli, &lib, name, &dir.join("svg"));
        plot_footprint(&cli, &copy, name, &dir.join("svg-copy"));
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// The symbols kicad-cli finds in `library` (a file or a folder): `sym export svg` of the whole library plots each, "Plotting symbol '%s' unit %d".
fn kicad_cli_symbols(cli: &Path, library: &Path, out: &Path) -> Vec<String> {
    let _ = std::fs::remove_dir_all(out);
    std::fs::create_dir_all(out).unwrap();
    let run = Command::new(cli).args(["sym", "export", "svg", "-o"]).arg(out).arg(library).output().expect("runs kicad-cli");
    let said = format!("{}{}", String::from_utf8_lossy(&run.stdout), String::from_utf8_lossy(&run.stderr));
    assert!(run.status.success(), "kicad-cli sym export svg of {}: {said}", library.display());
    let mut names: Vec<String> = said.lines().filter_map(|l| l.strip_prefix("Plotting symbol '")).filter_map(|l| l.split_once("' unit").map(|(n, _)| n.to_string())).collect();
    names.sort();
    names.dedup();
    names
}

/// The footprints kicad-cli finds in a `.pretty` folder: `fp export svg` of the whole library plots each, "Plotting footprint '%s' to '%s'".
fn kicad_cli_footprints(cli: &Path, library: &Path, out: &Path) -> Vec<String> {
    let _ = std::fs::remove_dir_all(out);
    std::fs::create_dir_all(out).unwrap();
    let run = Command::new(cli).args(["fp", "export", "svg", "--layers", "F.Cu", "-o"]).arg(out).arg(library).output().expect("runs kicad-cli");
    let said = format!("{}{}", String::from_utf8_lossy(&run.stdout), String::from_utf8_lossy(&run.stderr));
    assert!(run.status.success(), "kicad-cli fp export svg of {}: {said}", library.display());
    let mut names: Vec<String> = said.lines().filter_map(|l| l.strip_prefix("Plotting footprint '")).filter_map(|l| l.split_once('\'').map(|(n, _)| n.to_string())).collect();
    names.sort();
    names
}

/// UI-LEFT-2 LL1: Delete and Rename reach the library when it is saved -- a symbol deleted (with the symbol derived from it) and one renamed in a file library
/// and in a folder library (whose deleted symbol's file is removed and renamed symbol's file renamed, KiCad 25ec2335), a footprint deleted and one renamed in a
/// `.pretty` folder -- and kicad-cli loads each library with exactly the items left.
#[test]
fn deleted_and_renamed_items_leave_their_library_and_kicad_cli_loads_what_is_left() {
    let (Some(device), Some(installed)) = (installed_device(), installed_pretty("Resistor_SMD")) else { return };
    let dir = scratch("delete-rename");
    let pick = dir.join("pick.kicad_symdir");
    std::fs::create_dir(&pick).unwrap();
    for f in ["R", "C", "C_Feedthrough", "Filter_EMI_C"] {
        std::fs::copy(device.join(format!("{f}.kicad_sym")), pick.join(format!("{f}.kicad_sym"))).unwrap();
    }
    let original = eda_kicad::read_symbol_library_text(&pick).unwrap();
    // a file library: C_Feedthrough deleted takes Filter_EMI_C (derived from it) with it; C renamed Cap (the old name deleted, the symbol written anew)
    let file = dir.join("Mine.kicad_sym");
    std::fs::write(&file, &original).unwrap();
    let mut cap = symbol_of(&original, "Mine", "C");
    cap.lib_id = "Mine:Cap".into();
    save_symbols_deleting(&file, &[SymbolSave { symbol: &cap, embedded: None }], &["C_Feedthrough", "C"]).unwrap();
    let saved = std::fs::read_to_string(&file).unwrap();
    let mut names: Vec<String> = eda_kicad::scan_symbols(&saved).into_iter().map(|s| s.name).collect();
    names.sort();
    assert_eq!(names, ["Cap", "R"]);
    assert_eq!(form_of(&saved, "R"), form_of(&original, "R"), "a symbol neither deleted nor renamed keeps its text");
    // the folder library: the same, file by file
    let folder = dir.join("Folder.kicad_symdir");
    std::fs::create_dir(&folder).unwrap();
    for e in std::fs::read_dir(&pick).unwrap().filter_map(|e| e.ok()) {
        std::fs::copy(e.path(), folder.join(e.file_name())).unwrap();
    }
    let w = save_symbols_deleting(&folder, &[SymbolSave { symbol: &cap, embedded: None }], &["C_Feedthrough", "C"]).unwrap();
    assert_eq!(w.removed, [folder.join("C.kicad_sym"), folder.join("C_Feedthrough.kicad_sym"), folder.join("Filter_EMI_C.kicad_sym")]);
    let mut files: Vec<String> = std::fs::read_dir(&folder).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
    files.sort();
    assert_eq!(files, ["Cap.kicad_sym", "R.kicad_sym"]);
    // a .pretty folder: R_0805 deleted, R_0603 renamed (saved under the new name, the old file deleted, as `SaveFootprint` does after a rename)
    let pretty = dir.join("Mine.pretty");
    std::fs::create_dir(&pretty).unwrap();
    for f in ["R_0603_1608Metric", "R_0805_2012Metric", "R_1206_3216Metric"] {
        std::fs::copy(installed.join(format!("{f}.kicad_mod")), pretty.join(format!("{f}.kicad_mod"))).unwrap();
    }
    let mut renamed = eda_kicad::parse_library_footprint(&std::fs::read_to_string(pretty.join("R_0603_1608Metric.kicad_mod")).unwrap()).unwrap().footprint;
    renamed.name = "Mine:R_Small".into();
    renamed.assign_missing_ids();
    assert_eq!(delete_footprints(&pretty, &["Mine:R_0805_2012Metric", "Mine:R_0603_1608Metric"]).unwrap().len(), 2);
    save_footprint(&pretty, &renamed, &EmbeddedFiles::default()).unwrap();

    if !slow_tests_on() {
        return;
    }
    let Some(cli) = find_kicad_cli() else { return eprintln!("skipped: no kicad-cli") };
    assert_eq!(kicad_cli_symbols(&cli, &file, &dir.join("svg-file")), ["Cap", "R"]);
    assert_eq!(kicad_cli_symbols(&cli, &folder, &dir.join("svg-folder")), ["Cap", "R"]);
    assert_eq!(kicad_cli_footprints(&cli, &pretty, &dir.join("svg-pretty")), ["R_1206_3216Metric", "R_Small"]);
    let _ = std::fs::remove_dir_all(&dir);
}
