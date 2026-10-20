//! Other EDA formats in -- the parts `eda import-kicad` and the studio's File > Import > Non-KiCad Board / Schematic share.
//!
//! kicad-cli does the converting (`eda_kicad_engine::foreign`); this module is what stands between it and a design: which kind of file a name suggests, which file pairs with
//! it, how an Altium project lists its documents, the model a board brings, and how several schematics become one.
//!
//! # How a board and a schematic pair
//!
//! KiCad pairs them in the project manager's "Import Non-KiCad Project" (`KICAD_MANAGER_FRAME::ImportNonKiCadProject`; the file filters it offers are in
//! `common/wildcards_and_files_ext.cpp`): **Eagle** (`*.sch;*.brd`) and **CADSTAR Archive** (`*.csa;*.cpa`) are asked for as either file of the pair, and the other is the file of
//! the same name in the same folder with the other extension; **Altium** (`*.PrjPcb`) is a project file that lists its documents (`[DocumentN]` / `DocumentPath=`: the
//! `.SchDoc` sheets and the `.PcbDoc`), and its schematic plugin (`SCH_IO_ALTIUM::LoadSchematicProject`) makes one sheet symbol of each `.SchDoc` that no other sheet places,
//! laid out on a grid of 1000 mil (two apart, ten across), the sheets numbered from page 2, their hierarchical labels turned into global ones (a top-level sheet has no
//! parent to take them to). kicad-cli imports one file at a time and has no project import, so that is what is done here round it.
//!
//! This checkout of KiCad's sources has the file filters, the Altium plugin's project loader and the importers; the project manager's own `import_project.cpp` is not in it, so
//! the same-name pairing is the manager's as the filters and the QA data imply (`qa/data/pcbnew/plugins/eagle/Adafruit-AHT20-PCB/` and `qa/data/eeschema/io/eagle/` hold a
//! `.brd` and a `.sch` of one name), not read from its code.

use eda_kicad_engine::foreign::{self, ForeignImport, ForeignOptions, Kind};
use eda_model::ir::{LabelKind, Point, SchematicSection, SheetInstance};
use eda_model::sheet_file::SheetFile;
use eda_model::CheckResult;
use eda_ops::Cmd;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

fn fail(check: &str, what: &str, msg: impl Into<String>) -> Vec<CheckResult> {
    vec![CheckResult::fail(check, what, msg.into())]
}

fn ext_of(name: &str) -> String {
    Path::new(name).extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default()
}

/// What a file of this name can be, in the order kicad-cli is asked: the extensions only a board or only a schematic has say so; the ones both have (`.json` of EasyEDA,
/// `.zip`, `.asc` of PADS and of LTspice, `.txt`) are tried as a board and, when no importer takes them so, as a schematic.
pub(crate) fn kinds_for(name: &str) -> Vec<Kind> {
    match ext_of(name).as_str() {
        "brd" | "pcbdoc" | "cpa" | "swpcbdoc" | "cmpcbdoc" | "cspcbdoc" | "pcb" => vec![Kind::Board],
        "sch" | "schdoc" | "csa" | "dsn" | "dch" => vec![Kind::Schematic],
        _ => vec![Kind::Board, Kind::Schematic],
    }
}

/// Convert `input` as `kind`, or as what its name suggests: the first kind kicad-cli has an importer for. An error other than "no importer recognises the file" is final.
pub(crate) fn convert(work: &Path, input: &Path, kind: Option<Kind>, opts: &ForeignOptions) -> Result<ForeignImport, Vec<CheckResult>> {
    let order = match kind {
        Some(k) => vec![k],
        None => kinds_for(&input.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()),
    };
    let mut first: Option<Vec<CheckResult>> = None;
    for k in order {
        let run = match k {
            Kind::Board => foreign::import_foreign_pcb(work, input, opts),
            Kind::Schematic => foreign::import_foreign_sch(work, input, opts),
        };
        match run {
            Ok(done) => return Ok(done),
            Err(e) if foreign::is_unrecognized(&e) => {
                first.get_or_insert(e);
            }
            Err(e) => return Err(e),
        }
    }
    Err(first.unwrap_or_else(|| fail("kicad_cli_import_format", &input.display().to_string(), "no importer takes this file")))
}

/// The file KiCad's project import pairs `path` with: Eagle's `.brd` and `.sch`, CADSTAR Archive's `.cpa` and `.csa`, the same name in the same folder. Which kind it is.
pub(crate) fn partner_of(path: &Path) -> Option<(Kind, PathBuf)> {
    let (partner_ext, kind) = match ext_of(&path.to_string_lossy()).as_str() {
        "brd" => ("sch", Kind::Schematic),
        "sch" => ("brd", Kind::Board),
        "cpa" => ("csa", Kind::Schematic),
        "csa" => ("cpa", Kind::Board),
        _ => return None,
    };
    let stem = path.file_stem()?.to_string_lossy().into_owned();
    let dir = path.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or_else(|| Path::new("."));
    // The file of the same name with the other extension, in any case of the extension (`.SCH`, `.Brd`) ...
    let by_scan = std::fs::read_dir(dir).ok().and_then(|entries| {
        entries.flatten().map(|e| e.path()).filter(|p| p.is_file()).find(|p| p.file_stem().is_some_and(|s| s.to_string_lossy() == stem) && ext_of(&p.to_string_lossy()) == partner_ext)
    });
    // ... or the name as `wxFileName::SetExt( .. )` then `FileExists()` would find it: on a file system that does not tell `Plan.csa` from `plan.CSA`, either is the file.
    let by_name = || [partner_ext.to_string(), partner_ext.to_ascii_uppercase()].into_iter().map(|e| dir.join(format!("{stem}.{e}"))).find(|p| p.is_file());
    by_scan.or_else(by_name).map(|p| (kind, p))
}

/// What an Altium project (`.PrjPcb`, an INI file) lists: the `.PcbDoc` boards and the `.SchDoc` sheets, as paths that exist, in the order the project lists them.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct AltiumProject {
    pub boards: Vec<PathBuf>,
    pub sheets: Vec<PathBuf>,
    /// What the project names that is not there.
    pub missing: Vec<String>,
}

pub(crate) fn altium_project(path: &Path) -> Result<AltiumProject, Vec<CheckResult>> {
    let text = std::fs::read(path).map_err(|e| fail("altium_project_read", &path.display().to_string(), format!("cannot read {}: {e}", path.display())))?;
    // A project file is Windows text, usually with a byte-order mark.
    let text = String::from_utf8_lossy(&text);
    let text = text.trim_start_matches('\u{feff}');
    let dir = path.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or_else(|| Path::new("."));
    let mut project = AltiumProject::default();
    let mut in_document = false;
    for line in text.lines().map(str::trim) {
        if let Some(section) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            in_document = section.to_ascii_lowercase().starts_with("document");
            continue;
        }
        let Some((key, value)) = line.split_once('=') else { continue };
        if !in_document || !key.trim().eq_ignore_ascii_case("DocumentPath") {
            continue;
        }
        let relative = value.trim().replace('\\', "/");
        let file = dir.join(&relative);
        let list = match ext_of(&relative).as_str() {
            "pcbdoc" => &mut project.boards,
            "schdoc" => &mut project.sheets,
            _ => continue,
        };
        // Altium is not particular about the case of a file name; a disk that is will have `Power.SchDoc` for `power.schdoc`.
        match find_ignoring_case(&file) {
            Some(found) => list.push(found),
            None => project.missing.push(relative),
        }
    }
    Ok(project)
}

fn find_ignoring_case(path: &Path) -> Option<PathBuf> {
    if path.is_file() {
        return Some(path.to_path_buf());
    }
    let (dir, name) = (path.parent()?, path.file_name()?.to_string_lossy().to_lowercase());
    std::fs::read_dir(dir).ok()?.flatten().map(|e| e.path()).find(|p| p.is_file() && p.file_name().is_some_and(|n| n.to_string_lossy().to_lowercase() == name))
}

/// The converted schematic as a [`SheetFile`]: the file kicad-cli wrote, its sheets and its library symbols, read as `Import Sheet` reads a file.
///
/// A schematic of several sheets that are not placed one on another (an Eagle file of several sheets, KiCad 10's "top-level sheets") is written as one file each --
/// `<name>.kicad_sch`, `<name>_2.kicad_sch`, ... -- and only the first is reachable from the output: the others are recorded in a project file, which `kicad-cli sch import` does not
/// write ("2 sheets were written, but only 'x.kicad_sch' is reachable"). Their files are beside the output, so they are read too, and the top-level sheets are placed on a root as
/// [`combine_sheets`] places an Altium project's.
pub(crate) fn sheet_file_of(import: &ForeignImport) -> Result<SheetFile, Vec<CheckResult>> {
    let first = eda_kicad::load_sheet_file(&import.output)?;
    let extra = top_level_siblings(import, &first);
    if extra.is_empty() {
        return Ok(first);
    }
    let stem = |p: &Path| p.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "sheet".to_string());
    let mut parts = vec![(stem(&import.output), first)];
    for path in extra {
        parts.push((stem(&path), eda_kicad::load_sheet_file(&path)?));
    }
    let mut all = combine_sheets(parts);
    all.notes.push(format!("the file has {} sheets that no sheet places: placed on a root sheet, as KiCad 10 keeps them in its project", all.root.sheets.len()));
    Ok(all)
}

/// The `.kicad_sch` files kicad-cli wrote beside the output that the output does not reach (`first` is what it reaches): the other top-level sheets, in the order of their numbers.
pub(crate) fn top_level_siblings(import: &ForeignImport, first: &SheetFile) -> Vec<PathBuf> {
    let Some(dir) = import.output.parent() else { return Vec::new() };
    let reached: std::collections::BTreeSet<&str> = first.screens.keys().map(String::as_str).collect();
    let own = import.output.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let mut extra: Vec<String> = import.files.iter().filter(|f| f.ends_with(".kicad_sch") && !f.contains('/') && **f != own && !reached.contains(f.as_str())).cloned().collect();
    // `x_2`, `x_3` .. `x_10`: by length, then by name, so that ten follows nine
    extra.sort_by(|a, b| a.len().cmp(&b.len()).then_with(|| a.cmp(b)));
    extra.into_iter().map(|f| dir.join(f)).collect()
}

/// Several schematics as one, the way `SCH_IO_ALTIUM::LoadSchematicProject` makes a schematic of a project's `.SchDoc`s: a root that places each of them as a sheet (a grid of
/// 1000 mil, two apart and ten across; a `SCH_SHEET`'s default size, 500 by 150 mil; pages from 2), their hierarchical labels turned into global ones, since a top-level sheet
/// has no parent to take them to. `parts` are the sheets' names and what each was converted to. A sheet that another sheet of the project places is not placed again (the loader
/// looks for "an existing screen" of the same name), and the screens inside each file are filed under names that cannot meet another file's.
pub(crate) fn combine_sheets(parts: Vec<(String, SheetFile)>) -> SheetFile {
    // 1000 mil, in micrometres, and the default size of a sheet symbol (`MIN_SHEET_WIDTH`, `MIN_SHEET_HEIGHT`: 500 and 150 mil)
    const GRID: i64 = 25_400;
    const SHEET: (i64, i64) = (12_700, 3_810);
    let mut root = SchematicSection { imported_from_kicad: true, ..Default::default() };
    let mut screens: BTreeMap<String, SchematicSection> = BTreeMap::new();
    let mut lib_symbols: Vec<eda_model::ir::LibrarySymbol> = Vec::new();
    let mut notes: Vec<String> = Vec::new();
    // The screens each file places by their own name: a project sheet with one of these names is one of them, not a top-level sheet of its own.
    let inner: Vec<String> = parts.iter().flat_map(|(_, f)| f.screens.keys().map(|k| k.trim_end_matches(".kicad_sch").to_ascii_lowercase())).collect();
    let (mut x, mut y, mut page) = (1_i64, 1_i64, 2);
    for (name, file) in parts {
        if inner.contains(&name.to_ascii_lowercase()) {
            notes.push(format!("{name} is placed by another sheet of the project, so it is not a sheet of its own"));
            continue;
        }
        let mut taken = (1..).map(|n| if n == 1 { name.clone() } else { format!("{name}_{}", n - 1) });
        let sheet_name = taken.find(|n| !root.sheets.iter().any(|s| &s.name == n)).unwrap_or_else(|| name.clone());
        let key = format!("{}.kicad_sch", file_name_of(&sheet_name));
        // The file's own screens, under a prefix, with the placements that name them following.
        let prefix = format!("{}__", file_name_of(&sheet_name));
        let renamed = |f: &str| format!("{prefix}{f}");
        let mut top = file.root.clone();
        for s in &mut top.sheets {
            s.file = renamed(&s.file);
        }
        for l in &mut top.labels {
            // `m_rootSheet` takes hierarchical labels of a top-level sheet for global ones
            if let LabelKind::Hierarchical { shape } = l.kind.clone() {
                l.kind = LabelKind::Global { shape };
            }
        }
        for (f, mut sec) in file.screens {
            for s in &mut sec.sheets {
                s.file = renamed(&s.file);
            }
            screens.insert(renamed(&f), sec);
        }
        screens.insert(key.clone(), top);
        root.sheets.push(SheetInstance { id: String::new(), name: sheet_name, file: key, at: Point { x: x * GRID, y: y * GRID }, size: SHEET, pins: vec![], page: page.to_string() });
        page += 1;
        x += 2;
        if x > 10 {
            x = 1;
            y += 2;
        }
        for lib in file.lib_symbols {
            if !lib_symbols.iter().any(|l| l.lib_id == lib.lib_id) {
                lib_symbols.push(lib);
            }
        }
        notes.extend(file.notes);
    }
    root.assign_missing_ids();
    SheetFile { root, screens, lib_symbols, notes }
}

/// A name as a file name stem: the characters every file system takes.
pub(crate) fn file_name_of(name: &str) -> String {
    eda_kicad_engine::file_stem(name)
}

/// The settings a `.kicad_pro` holds that the model has: net classes, the severity of each DRC check, the board-wide minimums, Board Setup's smaller pages. The one list `eda import-kicad`
/// applies to the project beside a `.kicad_pcb`.
pub(crate) fn merge_project(model: &mut eda_model::ConstraintModel, pro_text: &str) {
    eda::merge_project_net_classes(model, pro_text);
    // The per-DRC-type severity table is a wholesale replacement, not a merge (`eda_kicad::merge_project_rule_severities`).
    eda::merge_project_rule_severities(model, pro_text);
    // Board-wide minimum constraints (`rules.min_*`), distinct from the net classes merged above, which only ever carry nominal values (`eda_kicad::merge_project_design_rules`).
    eda::merge_project_design_rules(model, pro_text);
    // Board Setup's smaller pages: Formatting's footprint boxes, the Length-tuning Patterns, the Tuning Profiles and the profile each net class names.
    eda::merge_project_setup_pages(model, pro_text);
}

/// What a board brings that `Cmd::ReplaceBoard` cannot carry in its text: the model of the board (its parts, nets, footprints and rules, as an intent) as a file of its
/// own under the board's folder, where `DrawingsSection::intent_file` finds it; and the fills the file was saved with, which are kept beside the design. Run by
/// `board::step` before the command, from the same text the command carries, so the file is there for whatever loads the board after it -- an undo or a redo included
/// (the file is named by the hash of the text, and left where it is).
pub(crate) fn prepare_board(cmd: &Cmd, dir: &Path) -> Result<(), Vec<CheckResult>> {
    let Cmd::ReplaceBoard { text, name, keep_kicad_layer_names, project } = cmd else { return Ok(()) };
    let (design, mut model, notes) = eda::import_kicad_pcb(text).map_err(|e| fail("ops_bad_board", name, format!("'{name}' did not convert to a board this reader takes: {}", e.iter().filter_map(|c| c.hint.clone()).collect::<Vec<_>>().join("; "))))?;
    let file = eda_ops::imported_intent_file(&design.provenance.intent_hash, *keep_kicad_layer_names, project.as_deref());
    let path = dir.join(&file);
    if !path.is_file() {
        // The rules and the net classes the importer set are in the project kicad-cli wrote with the board, as a project's own are in its `.kicad_pro` (`eda import-kicad` of a project).
        if let Some(pro) = project {
            merge_project(&mut model, pro);
        }
        // `OpenProjectFiles`: with "Keep KiCad layer names" every layer of the loaded board gets its standard name back (`SetLayerName( layer, wxEmptyString )`).
        if *keep_kicad_layer_names {
            model.board.layer_setup.names.clear();
        }
        let yaml = serde_yaml::to_string(&model).map_err(|e| fail("import_encode", &file, e.to_string()))?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| fail("io", &parent.display().to_string(), e.to_string()))?;
        }
        // Beside the target and renamed into place, so a reader that comes in the meantime never sees half a model.
        let tmp = path.with_extension(format!("tmp.{}", std::process::id()));
        std::fs::write(&tmp, yaml).and_then(|()| std::fs::rename(&tmp, &path)).map_err(|e| fail("io", &file, e.to_string()))?;
    }
    // The fills of the old board are the old board's; the new one starts with the fills it was saved with, if it was saved with any.
    crate::fill_api::clear(dir, None);
    crate::fill_api::keep_imported(dir, &design, notes.zone_fills);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use eda_model::ir::{LabelShape, NetLabel};

    fn scratch(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("eda_cli_import_foreign_{name}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn the_extensions_only_one_kind_has_say_so_and_the_ones_both_have_are_tried_both_ways() {
        assert_eq!(kinds_for("x.brd"), vec![Kind::Board]);
        assert_eq!(kinds_for("X.PcbDoc"), vec![Kind::Board]);
        assert_eq!(kinds_for("x.CPA"), vec![Kind::Board]);
        assert_eq!(kinds_for("x.sch"), vec![Kind::Schematic]);
        assert_eq!(kinds_for("Power.SchDoc"), vec![Kind::Schematic]);
        assert_eq!(kinds_for("x.csa"), vec![Kind::Schematic]);
        for both in ["PCB_x.json", "p.zip", "x.asc", "x.txt", "noext"] {
            assert_eq!(kinds_for(both), vec![Kind::Board, Kind::Schematic], "{both}");
        }
    }

    #[test]
    fn eagle_and_cadstar_files_pair_by_name_in_their_folder_and_nothing_else_does() {
        let d = scratch("pair");
        for f in ["Board A.brd", "Board A.sch", "other.sch", "lone.brd", "Plan.cpa", "x.SchDoc", "x.PcbDoc"] {
            std::fs::write(d.join(f), "x").unwrap();
        }
        assert_eq!(partner_of(&d.join("Board A.brd")), Some((Kind::Schematic, d.join("Board A.sch"))));
        assert_eq!(partner_of(&d.join("Board A.sch")), Some((Kind::Board, d.join("Board A.brd"))));
        assert_eq!(partner_of(&d.join("lone.brd")), None);
        assert_eq!(partner_of(&d.join("other.sch")), None);
        // CADSTAR: the other extension, in any case of it (`Plan.CSA` is `Plan`'s schematic)
        std::fs::write(d.join("Plan.CSA"), "x").unwrap();
        assert_eq!(partner_of(&d.join("Plan.cpa")), Some((Kind::Schematic, d.join("Plan.CSA"))));
        assert_eq!(partner_of(&d.join("Plan.CSA")), Some((Kind::Board, d.join("Plan.cpa"))));
        // Altium's documents are not paired by name: its project file lists them.
        assert_eq!(partner_of(&d.join("x.PcbDoc")), None);
        assert_eq!(partner_of(&d.join("x.SchDoc")), None);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn an_altium_project_lists_its_sheets_and_its_board() {
        let d = scratch("prj");
        std::fs::create_dir_all(d.join("sub")).unwrap();
        for f in ["Top.SchDoc", "sub/Power.SchDoc", "B.PcbDoc", "notes.txt"] {
            std::fs::write(d.join(f), "x").unwrap();
        }
        std::fs::write(
            d.join("P.PrjPcb"),
            "\u{feff}[Design]\r\nVersion=1.0\r\nDocumentPath=ignored.SchDoc\r\n\r\n[Document1]\r\nDocumentPath=Top.SchDoc\r\nAnnotationEnabled=1\r\n[Document2]\r\nDocumentPath=sub\\power.schdoc\r\n[Document3]\r\nDocumentPath=B.PcbDoc\r\n[Document4]\r\nDocumentPath=notes.txt\r\n[Document5]\r\nDocumentPath=Gone.SchDoc\r\n[Preferences]\r\nPrefsVaultGUID=\r\n",
        )
        .unwrap();
        let p = altium_project(&d.join("P.PrjPcb")).unwrap();
        // (a file system that does not tell `power.schdoc` from `Power.SchDoc` answers with the name the project spells)
        let lower = |v: &[PathBuf]| v.iter().map(|p| p.display().to_string().to_lowercase()).collect::<Vec<_>>();
        assert_eq!(lower(&p.sheets), lower(&[d.join("Top.SchDoc"), d.join("sub/Power.SchDoc")]), "in the order listed, a file name found whatever its case, [Design] not a document");
        assert_eq!(p.boards, vec![d.join("B.PcbDoc")]);
        assert_eq!(p.missing, vec!["Gone.SchDoc".to_string()]);
        let _ = std::fs::remove_dir_all(&d);
    }

    fn sheet(label_kind: LabelKind, inner: Option<&str>) -> SheetFile {
        let mut root = SchematicSection { imported_from_kicad: true, ..Default::default() };
        root.labels.push(NetLabel { id: String::new(), net: "VCC".into(), at: Point { x: 0, y: 0 }, kind: label_kind });
        let mut screens = BTreeMap::new();
        if let Some(inner) = inner {
            root.sheets.push(SheetInstance { id: String::new(), name: "Inner".into(), file: inner.into(), at: Point { x: 1, y: 1 }, size: (1, 1), pins: vec![], page: String::new() });
            screens.insert(inner.to_string(), SchematicSection { imported_from_kicad: true, ..Default::default() });
        }
        SheetFile { root, screens, lib_symbols: vec![], notes: vec![] }
    }

    #[test]
    fn several_schematics_become_a_root_that_places_each_on_the_grid_of_the_altium_loader() {
        let hier = LabelKind::Hierarchical { shape: LabelShape::default() };
        let all = combine_sheets(vec![("Top".into(), sheet(hier.clone(), Some("inner.kicad_sch"))), ("Power".into(), sheet(hier, None)), ("Top".into(), sheet(LabelKind::Local, None))]);
        let placed: Vec<(&str, &str, Point, &str)> = all.root.sheets.iter().map(|s| (s.name.as_str(), s.file.as_str(), s.at, s.page.as_str())).collect();
        assert_eq!(
            placed,
            vec![
                ("Top", "Top.kicad_sch", Point { x: 25_400, y: 25_400 }, "2"),
                ("Power", "Power.kicad_sch", Point { x: 76_200, y: 25_400 }, "3"),
                ("Top_1", "Top_1.kicad_sch", Point { x: 127_000, y: 25_400 }, "4"),
            ],
            "1000 mil, two apart, pages from 2, a name taken gets _1"
        );
        assert!(all.root.sheets.iter().all(|s| s.size == (12_700, 3_810)), "500 by 150 mil");
        let top = &all.screens["Top.kicad_sch"];
        assert!(matches!(top.labels[0].kind, LabelKind::Global { .. }), "a top-level sheet has no parent: its hierarchical labels are global");
        assert!(matches!(all.screens["Top_1.kicad_sch"].labels[0].kind, LabelKind::Local), "a local label stays");
        // The screen a file places is filed under a name of its own, and the placement follows it.
        assert_eq!(top.sheets[0].file, "Top__inner.kicad_sch");
        assert!(all.screens.contains_key("Top__inner.kicad_sch"));
        assert!(all.root.imported_from_kicad);

        // A sheet another sheet places is not a top-level sheet.
        let inner_sheet = combine_sheets(vec![("Top".into(), sheet(LabelKind::Local, Some("Power.kicad_sch"))), ("Power".into(), sheet(LabelKind::Local, None))]);
        assert_eq!(inner_sheet.root.sheets.len(), 1);
        assert!(inner_sheet.notes.iter().any(|n| n.contains("Power is placed by another sheet")), "{:?}", inner_sheet.notes);
    }

    #[test]
    fn a_board_brings_its_model_as_a_file_under_the_boards_folder_named_by_the_hash_of_its_text() {
        let d = scratch("prepare");
        let text = "(kicad_pcb (version 20260901) (generator \"pcbnew\") (layers (0 \"F.Cu\" signal) (2 \"B.Cu\" signal)) (footprint \"L:A\" (layer \"F.Cu\") (uuid \"f\") (at 1 1) (property \"Reference\" \"R1\" (at 0 0 0) (layer \"F.SilkS\") (uuid \"p\")) (pad \"1\" smd rect (at 0 0) (size 1 1) (layers \"F.Cu\") (net \"SIG\") (uuid \"a\"))))";
        let cmd = Cmd::ReplaceBoard { text: text.into(), name: "x.brd".into(), keep_kicad_layer_names: false, project: None };
        prepare_board(&cmd, &d).unwrap();
        let (design, _, _) = eda::import_kicad_pcb(text).unwrap();
        let file = d.join(eda_ops::imported_intent_file(&design.provenance.intent_hash, false, None));
        assert!(file.is_file(), "{}", file.display());
        let model: eda_model::ConstraintModel = serde_yaml::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
        assert_eq!(model.parts.iter().map(|p| p.reference.as_str()).collect::<Vec<_>>(), vec!["R1"]);
        assert_eq!(model.nets.iter().map(|n| n.name.as_str()).collect::<Vec<_>>(), vec!["SIG"]);
        // Again: the file stays as it is (a redo finds it); another command is not its business; a text that is no board is refused before anything is written.
        let first = std::fs::metadata(&file).unwrap().modified().unwrap();
        prepare_board(&cmd, &d).unwrap();
        assert_eq!(std::fs::metadata(&file).unwrap().modified().unwrap(), first);
        prepare_board(&Cmd::Rip { part: "R1".into() }, &d).unwrap();
        let bad = Cmd::ReplaceBoard { text: "hello".into(), name: "y.brd".into(), keep_kicad_layer_names: false, project: None };
        assert_eq!(prepare_board(&bad, &d).unwrap_err()[0].check, "ops_bad_board");
        assert_eq!(std::fs::read_dir(d.join("imports")).unwrap().count(), 1);
        let _ = std::fs::remove_dir_all(&d);
    }

    // -------------------------------------------------------------------- the commands, through the board directory: one undo step each

    /// A board folder of its own: the part J9 on GND, placed, on a two-layer board.
    fn board_dir(tag: &str) -> PathBuf {
        use eda_model::ir::{FootprintInstance, PlacementSection, Provenance, Side};
        use eda_model::{ConstraintModel, Footprint, Net, Pad, PadKind, PadShape, Part, Pin, PinKind};
        let dir = scratch(&format!("board_{tag}"));
        let pad = |n: &str, x: i64| Pad { opposite_side: false, ext: Default::default(), number: n.into(), at: (x, 0), size: (800, 800), shape: PadShape::Rect, kind: PadKind::Smd, drill: None, drill_slot: None, rot: 0, roundrect_ratio: None };
        let fp = Footprint { name: "HDR".into(), pads: vec![pad("1", -800), pad("2", 800)], courtyard: Some((3000, 1500)), model: None, courtyard_outlines: vec![], models3d: vec![], art: Default::default() };
        let part = Part {
            reference: "J9".into(),
            mpn: None,
            lcsc: None,
            value: None,
            package: Some("HDR".into()),
            footprint: Some("HDR".into()),
            pins: vec![Pin { number: "1".into(), name: None, kind: PinKind::Passive }, Pin { number: "2".into(), name: None, kind: PinKind::Passive }],
            body_um: None,
            symbol: None,
            datasheet: None,
            edge: None,
        };
        let mut model = ConstraintModel { parts: vec![part], nets: vec![Net { name: "OLD".into(), pins: vec!["J9.1".into(), "J9.2".into()] }], footprints: vec![fp], ..Default::default() };
        model.board.layers = vec!["F.Cu".into(), "B.Cu".into()];
        let intent = dir.join("intent.yaml");
        std::fs::write(&intent, serde_yaml::to_string(&model).unwrap()).unwrap();
        let p = |x, y| Point { x, y };
        let design = eda_model::ir::Design {
            footprint_library: None,
            sheet_contents: None,
            bus_aliases: vec![],
            symbol_library: None,
            schema: 1,
            provenance: Provenance { engine_version: "t".into(), intent_hash: "x".into(), seed: 0, stage_hashes: vec![] },
            schematic: None,
            nets: None,
            placement: Some(PlacementSection { outline: vec![p(0, 0), p(100_000, 0), p(100_000, 100_000), p(0, 100_000)], footprints: vec![FootprintInstance { id: "J9".into(), at: p(20_000, 20_000), rot: 0, side: Side::Top, label: Default::default() }], modules: vec![] }),
            routing: None,
            drawings: None,
        };
        crate::board::save(&dir, &design).unwrap();
        std::fs::write(dir.join("board.json"), serde_json::json!({ "intent": intent.display().to_string(), "snap_um": 100, "spacing_um": 300 }).to_string()).unwrap();
        dir
    }

    fn parts(dir: &Path) -> Vec<String> {
        let (_, _, model) = crate::board::load(dir).unwrap();
        let mut refs: Vec<String> = model.parts.iter().map(|p| p.reference.clone()).collect();
        refs.sort();
        refs
    }

    /// A board as kicad-cli 10.99 writes one: the nets named where they are used.
    const FOREIGN_BOARD: &str = r#"(kicad_pcb (version 20260901) (generator "pcbnew") (generator_version "10.99")
        (general (thickness 1.6) (legacy_teardrops no)) (paper "A4")
        (layers (0 "F.Cu" signal) (2 "B.Cu" signal) (25 "Edge.Cuts" user))
        (footprint "eagle-import-fps:0603" (layer "F.Cu") (uuid "11111111-1111-1111-1111-111111111111") (at 20 20)
          (property "Reference" "R1" (at 0 -1 0) (layer "F.SilkS") (uuid "11111111-1111-1111-1111-111111111112") (effects (font (size 1 1) (thickness 0.15))))
          (property "Value" "10k" (at 0 1 0) (layer "F.Fab") (uuid "11111111-1111-1111-1111-111111111113") (effects (font (size 1 1) (thickness 0.15))))
          (pad "1" smd rect (at -0.8 0) (size 0.8 0.9) (layers "F.Cu" "F.Mask" "F.Paste") (net "VCC") (uuid "11111111-1111-1111-1111-111111111114"))
          (pad "2" smd rect (at 0.8 0) (size 0.8 0.9) (layers "F.Cu" "F.Mask" "F.Paste") (net "GND") (uuid "11111111-1111-1111-1111-111111111115")))
        (segment (start 19.2 20) (end 12 20) (width 0.25) (layer "F.Cu") (net "VCC") (uuid "22222222-2222-2222-2222-222222222221"))
        (gr_line (start 0 0) (end 40 0) (stroke (width 0.1) (type solid)) (layer "Edge.Cuts") (uuid "55555555-5555-5555-5555-555555555551"))
        (gr_line (start 40 0) (end 40 40) (stroke (width 0.1) (type solid)) (layer "Edge.Cuts") (uuid "55555555-5555-5555-5555-555555555552"))
        (gr_line (start 40 40) (end 0 40) (stroke (width 0.1) (type solid)) (layer "Edge.Cuts") (uuid "55555555-5555-5555-5555-555555555553"))
        (gr_line (start 0 40) (end 0 0) (stroke (width 0.1) (type solid)) (layer "Edge.Cuts") (uuid "55555555-5555-5555-5555-555555555554"))
    )"#;

    /// "Keep KiCad layer names": the names the other application gave the layers are the board's, unless the person keeps the standard ones -- another model of the same text.
    #[test]
    fn a_board_keeps_the_names_its_layers_came_with_unless_asked_to_use_kicads() {
        let d = scratch("names");
        let text = FOREIGN_BOARD.replace("(0 \"F.Cu\" signal)", "(0 \"F.Cu\" signal \"Top Layer\")");
        let model_of = |keep: bool| {
            let cmd = Cmd::ReplaceBoard { text: text.clone(), name: "x.brd".into(), keep_kicad_layer_names: keep, project: None };
            prepare_board(&cmd, &d).unwrap();
            let (design, _, _) = eda::import_kicad_pcb(&text).unwrap();
            let yaml = std::fs::read_to_string(d.join(eda_ops::imported_intent_file(&design.provenance.intent_hash, keep, None))).unwrap();
            serde_yaml::from_str::<eda_model::ConstraintModel>(&yaml).unwrap()
        };
        assert_eq!(model_of(false).board.layer_setup.names.get("F.Cu").map(String::as_str), Some("Top Layer"));
        assert!(model_of(true).board.layer_setup.names.is_empty(), "every layer shows its standard name");
        assert_eq!(std::fs::read_dir(d.join("imports")).unwrap().count(), 2, "two models of one text, two files");
        let _ = std::fs::remove_dir_all(&d);
    }

    /// The rules and the net classes an importer set are in the project kicad-cli made with the board and in no other file: the board that brings it has them in its model (another
    /// model of the same text, so another file), the one that does not has the defaults, and an undo takes them away with the rest of the imported board.
    #[test]
    fn a_board_brings_the_rules_and_the_net_classes_of_the_project_kicad_cli_made_with_it() {
        use eda_ops::Domain;
        let pro = r#"{ "board": { "design_settings": { "rules": { "min_clearance": 0.2032, "min_track_width": 0.2 } } },
            "net_settings": { "classes": [ { "name": "Default", "clearance": 0.2, "track_width": 0.2 }, { "name": "highpower", "track_width": 0.3048, "clearance": 0.2 } ],
                              "netclass_patterns": [ { "netclass": "highpower", "pattern": "VCC" } ] } }"#;
        let dir = board_dir("project");
        let import = |project: Option<&str>| Cmd::ReplaceBoard { text: FOREIGN_BOARD.into(), name: "x.brd".into(), keep_kicad_layer_names: false, project: project.map(String::from) };

        crate::board::step(&dir, import(None), false, "test").unwrap();
        let (_, _, plain) = crate::board::load(&dir).unwrap();
        assert!(plain.board.net_classes.iter().all(|c| c.name != "highpower") && plain.board.min_clearance_um != 203, "the board file alone says nothing of them");

        crate::board::step(&dir, import(Some(pro)), false, "test").unwrap();
        let (_, design, model) = crate::board::load(&dir).unwrap();
        let class = model.board.net_classes.iter().find(|c| c.name == "highpower").expect("the importer's net class");
        assert_eq!((class.nets.clone(), class.track_width), (vec!["VCC".to_string()], Some(305)));
        assert_eq!(model.board.min_clearance_um, 203, "and its design rules");
        assert!(model.board.constraints_explicit, "stated by the project: kept as stated");
        assert_eq!(model.parts.iter().map(|p| p.reference.as_str()).collect::<Vec<_>>(), vec!["R1"], "the same board");
        let file = design.drawings.as_ref().unwrap().intent_file.clone().unwrap();
        assert!(dir.join(&file).is_file());
        assert_eq!(std::fs::read_dir(dir.join("imports")).unwrap().count(), 2, "the same text with a project is another model, so another file");

        crate::board::undo(&dir, "test", Some(Domain::Pcb)).unwrap();
        let (_, _, back) = crate::board::load(&dir).unwrap();
        assert!(back.board.net_classes.iter().all(|c| c.name != "highpower"), "the import without the project is the step before");
        crate::board::redo(&dir, "test", Some(Domain::Pcb)).unwrap();
        let (_, _, again) = crate::board::load(&dir).unwrap();
        assert_eq!(again.board.min_clearance_um, 203, "a redo finds the file the first run left");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// File > Import > Non-KiCad Board is one step of the history: the parts, nets and footprints of the file replace the board's (not the intent's file, which is
    /// the old board's and stays), the gates do not refuse it, and an undo brings the old board back with its model; a redo, the imported one.
    #[test]
    fn importing_a_board_replaces_the_model_in_one_step_and_an_undo_takes_the_old_board_back_whole() {
        use eda_ops::Domain;
        let dir = board_dir("replace");
        assert_eq!(parts(&dir), vec!["J9"]);
        let cmd = Cmd::ReplaceBoard { text: FOREIGN_BOARD.into(), name: "x.brd".into(), keep_kicad_layer_names: false, project: None };
        // `strict`: the gates would refuse a move that makes the board worse; an import is not one.
        crate::board::step(&dir, cmd, true, "test").expect("the import");
        assert_eq!(parts(&dir), vec!["R1"], "the old board's J9 is gone, the file's R1 is the board");
        let (meta, design, model) = crate::board::load(&dir).unwrap();
        assert_eq!(design.placement.as_ref().unwrap().footprints.iter().map(|f| f.id.as_str()).collect::<Vec<_>>(), vec!["R1"]);
        assert_eq!(model.nets.iter().map(|n| n.name.as_str()).collect::<Vec<_>>(), vec!["GND", "VCC"], "the nets of the pads of the file");
        let file = design.drawings.as_ref().unwrap().intent_file.clone().expect("the model is a file of its own");
        assert!(dir.join(&file).is_file() && file.starts_with("imports/"), "{file}");
        assert!(std::fs::read_to_string(&meta.intent).unwrap().contains("J9"), "board.json's intent is the old board's, untouched");
        assert_eq!(design.routing.as_ref().unwrap().tracks.len(), 1, "the copper the file brings is kept: an import does not clear the routing");

        crate::board::undo(&dir, "test", Some(Domain::Pcb)).expect("one undo");
        assert_eq!(parts(&dir), vec!["J9"], "the old board's model is back, with the board");
        let (_, back, model) = crate::board::load(&dir).unwrap();
        assert_eq!(back.placement.as_ref().unwrap().footprints[0].id, "J9");
        assert!(back.drawings.as_ref().and_then(|d| d.intent_file.clone()).is_none() && model.nets.iter().any(|n| n.name == "OLD"));
        assert!(crate::board::undo(&dir, "test", Some(Domain::Pcb)).is_err(), "and that was the one step");

        crate::board::redo(&dir, "test", Some(Domain::Pcb)).expect("a redo");
        assert_eq!(parts(&dir), vec!["R1"], "the imported board again, from the file the first run left");
        // A second import replaces the first, and undoes back to it.
        let other = FOREIGN_BOARD.replace("\"R1\"", "\"C7\"");
        crate::board::step(&dir, Cmd::ReplaceBoard { text: other, name: "y.brd".into(), keep_kicad_layer_names: false, project: None }, false, "test").unwrap();
        assert_eq!(parts(&dir), vec!["C7"]);
        crate::board::undo(&dir, "test", Some(Domain::Pcb)).unwrap();
        assert_eq!(parts(&dir), vec!["R1"]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A text that is no board is refused before anything is written: the folder is as it was.
    #[test]
    fn a_file_that_is_no_board_changes_nothing() {
        let dir = board_dir("refused");
        let before = std::fs::read_to_string(dir.join("design.json")).unwrap();
        let e = crate::board::step(&dir, Cmd::ReplaceBoard { text: "not a board".into(), name: "x.brd".into(), keep_kicad_layer_names: false, project: None }, false, "test").unwrap_err();
        assert_eq!(e[0].check, "ops_bad_board");
        assert_eq!(std::fs::read_to_string(dir.join("design.json")).unwrap(), before);
        assert!(!dir.join("imports").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The mcu30 example drawn as module sheets, written as a KiCad project and read as a file: a root that places three sheets, a screen each.
    fn a_file() -> SheetFile {
        let dir = scratch("mcu30_sheets");
        std::fs::create_dir_all(&dir).unwrap();
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/mcu_board_30plus.yaml");
        let model: eda_model::ConstraintModel = serde_yaml::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        let design = eda_engine::derive_schematic_modules(&model, &eda_engine::EngineOptions::new(1, "t")).unwrap();
        let tree = eda_kicad::export_kicad_sch_tree(&design, &model, &eda_kicad::ExportMeta { date: "2026-10-10", title: "t" }, "whole.kicad_sch").unwrap();
        for (file, text) in &tree {
            std::fs::write(dir.join(file), text).unwrap();
        }
        let file = eda_kicad::load_sheet_file(&dir.join("whole.kicad_sch")).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
        file
    }

    /// File > Import > Non-KiCad Schematic: the schematic is the file's, its nets are traced from the drawing and are the board's netlist (the symbols of the file are parts the board
    /// has not placed), and one undo from the schematic's tab takes the old drawing and the old nets back.
    #[test]
    fn importing_a_schematic_traces_its_nets_into_the_netlist_and_an_undo_takes_the_old_drawing_back() {
        use eda_ops::Domain;
        let dir = board_dir("replace_sch");
        let file = a_file();
        // the parts of the file: one per reference of a symbol (a power symbol is not a part)
        let want = file.all_screens().flat_map(|(_, s)| s.symbols.iter().map(|x| x.id.clone())).collect::<std::collections::BTreeSet<_>>().len();
        let cmd = Cmd::ReplaceSchematic { content: file, name: "mcu.SchDoc".into() };
        crate::board::step(&dir, cmd, true, "test").expect("the import");
        let (_, design, model) = crate::board::load(&dir).unwrap();
        assert_eq!(design.schematic.as_ref().unwrap().sheets.len(), 3);
        let nets = design.nets.as_ref().expect("the drawing's nets are the netlist now");
        assert!(nets.len() > 10 && nets.iter().any(|n| n.pins.iter().any(|p| p.starts_with("U1."))), "{} nets", nets.len());
        assert!(model.part("U1").is_some() && model.part("J9").is_some(), "the symbols of the file are parts; the board's J9 is still a part of the board");
        assert_eq!(model.parts.len(), want + 1, "the file's parts and the board's J9");
        assert!(design.symbol_library.as_ref().is_some_and(|l| l.symbols.iter().any(|s| s.published)), "the library symbols of the file are the project's");

        crate::board::undo(&dir, "test", Some(Domain::Schematic)).expect("one undo");
        let (_, back, model) = crate::board::load(&dir).unwrap();
        assert!(back.schematic.is_none() && back.nets.is_none() && back.sheet_contents.is_none(), "the old drawing: none stored");
        assert_eq!(parts(&dir), vec!["J9"]);
        assert!(model.nets.iter().any(|n| n.name == "OLD"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The sheets kicad-cli wrote that the output does not reach are the other top-level sheets, in the order of their numbers (ten after nine), and the screens a sheet places are not.
    #[test]
    fn the_files_the_output_does_not_reach_are_the_other_top_level_sheets_in_the_order_of_their_numbers() {
        use eda_kicad_engine::foreign::{ImportReport, Kind};
        let import = ForeignImport {
            engine: "kicad-cli test".into(),
            kind: Kind::Schematic,
            output: PathBuf::from("/work/sch-out/x.kicad_sch"),
            text: String::new(),
            report: ImportReport::default(),
            log: vec![],
            files: ["x-eagle-import.kicad_sym", "x_10.kicad_sch", "x_2.kicad_sch", "child.kicad_sch", "x_3.kicad_sch", "sym-lib-table", "sub/inner.kicad_sch", "x.kicad_sch"].map(String::from).to_vec(),
            project: None,
        };
        let mut first = SheetFile::default();
        first.screens.insert("child.kicad_sch".into(), SchematicSection::default());
        let extra: Vec<String> = top_level_siblings(&import, &first).iter().map(|p| p.file_name().unwrap().to_string_lossy().into_owned()).collect();
        assert_eq!(extra, vec!["x_2.kicad_sch", "x_3.kicad_sch", "x_10.kicad_sch"], "the output itself, a sheet it places, a library and a file in a folder are not top-level sheets");
        assert!(top_level_siblings(&import, &SheetFile { screens: ["child.kicad_sch", "x_2.kicad_sch", "x_3.kicad_sch", "x_10.kicad_sch"].map(|f| (f.to_string(), SchematicSection::default())).into_iter().collect(), ..Default::default() }).is_empty(), "when the output places them they are its own");
    }
}
