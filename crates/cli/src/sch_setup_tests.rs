//! Schematic Setup's pages over a real board directory (UI-LEFT S3): each page's OK is one `/api/cmd` verb, the settings reach the derived project as
//! KiCad's `SCHEMATIC_SETTINGS` and `NET_SETTINGS` write them, our reader reads them back as written, and Undo on the schematic tab takes each back.
//!
//! The slow tier (`EDA_SLOW_TESTS=1`) hands the derived files to kicad-cli: its ERC reads the project's bus aliases (wires labelled D0 and D1 entering a bus
//! labelled `{DATA}` are not its members until the alias says DATA is D0 and D1 -- read from the project alone too) and its net classes (a label's class is
//! undefined until Schematic Setup > Net Classes defines it), and its plot draws a wire in its class's colour.

use crate::board::{load, step, undo};
use crate::sch_hierarchy_tests::{edit_label_fields, erc_kinds, kicad_cli_ready, mcu30_board, netclass};
use eda_model::ir::{BusAlias, Nm};
use eda_model::sch_setup::{AnnotationSettings, NetClassSchematic, SchFormatting, TemplateFieldName};
use eda_model::units::mm;
use eda_ops::{Cmd, Domain};
use std::path::Path;

/// The project the studio derives for kicad-cli (`export_kicad_pro_for`).
fn derived_project(dir: &Path) -> serde_json::Value {
    let (design, model) = crate::kicad_engine::load_with_schematic(dir).unwrap();
    serde_json::from_str(&eda_kicad::export_kicad_pro_for(&design, &model)).unwrap()
}

fn alias(name: &str, members: &[&str]) -> BusAlias {
    BusAlias { name: name.into(), members: members.iter().map(|m| m.to_string()).collect() }
}

/// A class `name` with no sizes of its own and the schematic columns `look`, added to the board's classes (Schematic Setup > Net Classes' OK: `set_net_classes`).
fn define_class(dir: &Path, name: &str, look: NetClassSchematic) {
    let (_, _, model) = load(dir).unwrap();
    let mut settings = eda_model::rules::net_class_settings_of(&model.board);
    settings.classes.push(eda_model::NetClass { name: name.into(), nets: vec![], track_width: None, clearance: None, via_diameter: None, via_drill: None, microvia_diameter: None, microvia_drill: None, diff_pair_width: None, diff_pair_gap: None, diff_pair_via_gap: None, priority: 0 });
    settings.schematic.insert(name.into(), look);
    step(dir, Cmd::SetNetClasses { settings }, false, "test").unwrap();
}

/// Formatting, Annotation, Field Name Templates and Bus Alias Definitions: each OK stores the page (one step), the derived project's `schematic` object carries
/// it under KiCad's keys and units, `eda_kicad::project_schematic` reads that object back to the same settings, and one schematic-tab undo takes each page back.
#[test]
fn each_page_reaches_the_project_reads_back_and_undoes() {
    let dir = mcu30_board("setup_pages");
    let formatting = SchFormatting { junction_size_choice: 5, default_text_size_nm: Nm::new(60 * 25_400), default_line_width_nm: Nm::new(8 * 25_400), label_size_ratio: 0.5, dash_ratio: 10.0, opo_v_range: "mV".into(), ..SchFormatting::default() };
    step(&dir, Cmd::SetSchFormatting { settings: formatting.clone() }, false, "test").unwrap();
    let annotation = AnnotationSettings { sort_order: 1, method: 2, start_num: 100, reuse_designators: false };
    step(&dir, Cmd::SetAnnotationSettings { settings: annotation.clone(), subpart_first_id: '1' as u32, subpart_id_separator: '-' as u32 }, false, "test").unwrap();
    let templates = vec![TemplateFieldName { name: "MPN".into(), visible: false, url: false }, TemplateFieldName { name: "Link".into(), visible: true, url: true }];
    step(&dir, Cmd::SetFieldNameTemplates { fields: templates.clone() }, false, "test").unwrap();
    step(&dir, Cmd::SetBusAliases { aliases: vec![alias("DATA", &["D0", "D1"]), alias("ADDR", &["A[0..3]"])] }, false, "test").unwrap();

    let pro = derived_project(&dir);
    let sch = &pro["schematic"];
    assert_eq!(sch["drawing"]["default_text_size"], 60.0, "mils, as `PARAM_SCALED` writes them");
    assert_eq!(sch["drawing"]["default_line_thickness"], 8.0);
    assert_eq!(sch["drawing"]["junction_size_choice"], 5);
    assert_eq!(sch["drawing"]["label_size_ratio"], 0.5);
    assert_eq!(sch["drawing"]["dashed_lines_dash_length_ratio"], 10.0);
    assert_eq!(sch["drawing"]["operating_point_overlay_v_range"], "mV");
    assert_eq!(sch["meta"]["version"], 1, "at SCHEMATIC_SETTINGS' schema, so KiCad does not migrate the margin ratio over");
    assert_eq!((sch["annotation"]["sort_order"].as_u64(), sch["annotation"]["method"].as_u64(), sch["annotate_start_num"].as_i64(), sch["reuse_designators"].as_bool()), (Some(1), Some(2), Some(100), Some(false)));
    assert_eq!((sch["subpart_first_id"].as_u64(), sch["subpart_id_separator"].as_u64()), (Some('1' as u64), Some('-' as u64)), "the unit notation -1");
    assert_eq!(sch["drawing"]["field_names"], serde_json::json!([{ "name": "MPN", "visible": false, "url": false }, { "name": "Link", "visible": true, "url": true }]));
    assert_eq!(sch["bus_aliases"], serde_json::json!({ "ADDR": ["A[0..3]"], "DATA": ["D0", "D1"] }), "PROJECT_FILE's `schematic.bus_aliases`");

    // our reader takes the derived project's object back to what was set
    let text = serde_json::to_string(&pro).unwrap();
    let mut back = eda_model::sch_extras::SchExtras::default();
    eda_kicad::project_schematic::apply(&mut back, &text);
    assert_eq!(back.formatting.as_ref(), Some(&formatting));
    assert_eq!(back.annotation.as_ref(), Some(&annotation));
    assert_eq!((back.subpart_first_id, back.subpart_id_separator), (Some('1' as u32), Some('-' as u32)));
    assert_eq!(back.field_name_templates, templates);
    assert_eq!(eda_kicad::project_bus_aliases(&text), vec![alias("ADDR", &["A[0..3]"]), alias("DATA", &["D0", "D1"])]);

    // the schematic tab's undo takes the pages back one by one, the bus aliases (a top-level part of the design) with them
    undo(&dir, "test", Some(Domain::Schematic)).unwrap();
    assert!(load(&dir).unwrap().1.bus_aliases.is_empty(), "the bus aliases are the schematic's to undo");
    undo(&dir, "test", Some(Domain::Schematic)).unwrap();
    assert!(load(&dir).unwrap().1.schematic.unwrap().extras.field_name_templates.is_empty());
    undo(&dir, "test", Some(Domain::Schematic)).unwrap();
    let (_, d, _) = load(&dir).unwrap();
    assert!(d.schematic.as_ref().unwrap().extras.annotation.is_none() && d.schematic.as_ref().unwrap().extras.subpart_first_id.is_none());
    undo(&dir, "test", Some(Domain::Schematic)).unwrap();
    assert!(load(&dir).unwrap().1.schematic.unwrap().extras.formatting.is_none());
}

/// Schematic Setup > Net Classes: the schematic columns of a class and of the Default class are in the derived project's `net_settings.classes[]` as
/// `saveNetclass` writes them (widths in mils, the line style's number, the colour as `COLOR4D` writes it, `rgba(0, 0, 0, 0.000)` for none), the importer
/// reads them back, the sheet draws a wire of the class by them (`class_look`) and the junction dot follows the Default class's wire width; Undo from
/// either tab takes the table back (Board Setup and Schematic Setup edit one).
#[test]
fn net_class_schematic_columns_reach_the_project_and_the_sheet() {
    let dir = mcu30_board("setup_netclasses");
    let (_, design, model) = load(&dir).unwrap();
    let root = design.schematic.as_ref().unwrap();
    let wire = root.wires.iter().find(|w| !w.net.is_empty() && !w.bus).expect("mcu30 has a wire on a net").clone();
    let mut settings = eda_model::rules::net_class_settings_of(&model.board);
    let pattern = eda_kicad::exact_pattern(&wire.net);
    settings.classes.push(eda_model::NetClass { name: "Wide".into(), nets: vec![pattern], track_width: None, clearance: None, via_diameter: None, via_drill: None, microvia_diameter: None, microvia_drill: None, diff_pair_width: None, diff_pair_gap: None, diff_pair_via_gap: None, priority: 0 });
    let wide = NetClassSchematic { wire_width: Some(Nm::new(20 * 25_400)), bus_width: None, color: Some("rgb(255, 0, 0)".into()), line_style: Some(1) };
    settings.schematic.insert("Wide".into(), wide.clone());
    settings.schematic.insert("Default".into(), NetClassSchematic { wire_width: Some(Nm::new(10 * 25_400)), ..Default::default() });
    step(&dir, Cmd::SetNetClasses { settings: settings.clone() }, false, "test").unwrap();

    // the schematic's project: `export_kicad_pro_for` with the classes `project_net_classes` puts in it, as the schematic export writes it
    let (d, m) = crate::kicad_engine::load_with_schematic(&dir).unwrap();
    let mut pro: serde_json::Value = serde_json::from_str(&eda_kicad::export_kicad_pro_for(&d, &m)).unwrap();
    eda_kicad::appearance::put_classes(&mut pro, &eda_kicad_engine::project_net_classes(&m));
    let classes = pro["net_settings"]["classes"].as_array().unwrap();
    let of = |name: &str| classes.iter().find(|c| c["name"] == name).unwrap_or_else(|| panic!("{name} in {classes:?}"));
    assert_eq!((of("Wide")["wire_width"].as_f64(), of("Wide")["line_style"].as_i64(), of("Wide")["schematic_color"].as_str()), (Some(20.0), Some(1), Some("rgb(255, 0, 0)")));
    assert!(of("Wide").get("bus_width").is_none(), "an unset width is not written (`HasBusWidth`)");
    assert_eq!((of("Default")["wire_width"].as_f64(), of("Default")["bus_width"].as_f64(), of("Default")["line_style"].as_i64(), of("Default")["schematic_color"].as_str()), (Some(10.0), Some(12.0), Some(0), Some("rgba(0, 0, 0, 0.000)")), "the Default class with KiCad's defaults for what it leaves unset");

    // read back as a project with a schematic: the same columns
    let mut back = eda_model::ConstraintModel::default();
    eda_kicad::merge_project_net_classes_for_schematic(&mut back, &serde_json::to_string(&pro).unwrap());
    assert_eq!(back.board.net_class_schematic.get("Wide"), Some(&wide));
    assert_eq!(back.board.net_class_schematic.get("Default").and_then(|l| l.wire_width), Some(Nm::new(254_000)));

    // the sheet: the wire is drawn by its class, the junction dot at the Default class's 10 mils times 6
    let j = crate::studio::schematic_json(&dir, "", crate::text_vars_json::clock_of("/api/schematic")).unwrap();
    let drawn = j["wires"].as_array().unwrap().iter().find(|w| w["id"] == wire.id.as_str()).unwrap();
    assert_eq!(drawn["class_look"], serde_json::json!({ "width_nm": 508_000, "color": "rgb(255, 0, 0)", "style": 1 }));
    assert_eq!(j["setup"]["junction_diameter_nm"], 1_524_000, "60 mils: `GetJunctionSize` on the Default class's wire");

    // one undo from the schematic tab takes the table back
    undo(&dir, "test", Some(Domain::Schematic)).unwrap();
    let (_, _, model) = load(&dir).unwrap();
    assert!(!model.board.net_classes.iter().any(|c| c.name == "Wide") && model.board.net_class_schematic.is_empty());
}

/// One sheet with R1 and R2: each pin 1 on a wire labelled D0 / D1 that meets a bus through a bus entry, the bus labelled `{DATA}` -- a bus group whose
/// one member is the alias DATA when the project defines it, else the net DATA.
fn alias_board(name: &str) -> std::path::PathBuf {
    use eda_model::ir::{BusEntry, LabelKind, NetLabel, Point, Wire};
    let dir = crate::sch_hierarchy_tests::scratch(name);
    let intent = dir.join("intent.yaml");
    let part = |r: &str| format!("  - {{ reference: {r}, value: 10k, package: \"0603\", pins: [{{ number: \"1\", kind: passive }}, {{ number: \"2\", kind: passive }}] }}\n");
    std::fs::write(&intent, format!("parts:\n{}{}nets:\n  - {{ name: A, pins: [R1.2, R2.2] }}\n", part("R1"), part("R2"))).unwrap();
    let model: eda_model::ConstraintModel = serde_yaml::from_str(&std::fs::read_to_string(&intent).unwrap()).unwrap();
    let mut design = eda_engine::derive_schematic(&model, &eda_engine::EngineOptions::new(1, "t")).unwrap();
    design.placement = Some(eda_model::ir::PlacementSection { outline: vec![], footprints: vec![], modules: vec![] });
    let mut root = design.schematic.take().unwrap();
    root.sheets.clear();
    root.wires.clear();
    root.labels.clear();
    root.power_symbols.clear();
    root.no_connects.clear();
    root.junctions.clear();
    root.bus_entries.clear();
    for (r, y) in [("R1", 50.8), ("R2", 76.2)] {
        let s = root.symbols.iter_mut().find(|s| s.id == r).unwrap();
        s.at = Point { x: mm(101.6), y: mm(y) };
        s.rot = 0;
    }
    let pin1 = |sec: &eda_model::ir::SchematicSection, r: &str| -> Point {
        let sym = sec.symbols.iter().find(|s| s.id == r).unwrap();
        let part = model.part(r).unwrap();
        eda_engine::placed::pin_points_in(sec, sym, part, model.real_symbol_of_instance(sec, sym, part).as_ref()).into_iter().find(|(n, _)| n == "1").unwrap().1
    };
    let (p1, p2) = (pin1(&root, "R1"), pin1(&root, "R2"));
    let bx = Nm::new(p1.x.min(p2.x).get() / 2_540_000 * 2_540_000) - mm(10.16);
    for (p, net) in [(p1, "D0"), (p2, "D1")] {
        // a wire from the pin to the entry: a vertical drop when the pin is not on the entry's row, then across
        let tap = Point { x: bx + mm(2.54), y: p.y };
        root.wires.push(Wire { id: String::new(), net: String::new(), pins: vec![], pts: vec![p, tap], bus: false });
        root.labels.push(NetLabel { id: String::new(), net: net.into(), at: Point { x: tap.x + mm(2.54), y: tap.y }, kind: LabelKind::Local });
        root.bus_entries.push(BusEntry { id: String::new(), at: tap, size: Point { x: mm(-2.54), y: mm(2.54) } });
    }
    let top = Point { x: bx, y: p1.y.min(p2.y) - mm(2.54) };
    root.wires.push(Wire { id: String::new(), net: String::new(), pins: vec![], pts: vec![top, Point { x: bx, y: p1.y.max(p2.y) + mm(7.62) }], bus: true });
    root.labels.push(NetLabel { id: String::new(), net: "{DATA}".into(), at: top, kind: LabelKind::Local });
    root.assign_missing_ids();
    design.schematic = Some(root);
    crate::board::save(&dir, &design).unwrap();
    let meta = crate::board::Meta { intent: intent.display().to_string(), snap_nm: mm(0.1), spacing_nm: mm(0.3) };
    std::fs::write(dir.join("board.json"), serde_json::to_string_pretty(&meta).unwrap()).unwrap();
    dir
}

/// Bus Alias Definitions in kicad-cli's ERC: the wires labelled D0 and D1 meet the bus `{DATA}` through bus entries; with no alias `DATA` is a net and
/// neither wire's net is a member of the bus (`net_not_bus_member`, `ERCE_BUS_ENTRY_CONFLICT`: "Net %s is graphically connected to bus %s but is not a member of that bus"); the alias
/// DATA = D0 D1 (one `set_bus_aliases`) makes both members and the findings go. kicad-cli reads it from the project's `schematic.bus_aliases` alone (the
/// derived schematic's legacy blocks taken out); one undo brings the findings back.
#[test]
fn slow_kicad_cli_reads_the_projects_bus_aliases() {
    if !kicad_cli_ready() {
        return;
    }
    let dir = alias_board("slow_setup_alias");
    let conflicts = |dir: &Path| erc_kinds(dir).get("net_not_bus_member").copied().unwrap_or(0);
    assert_eq!(conflicts(&dir), 2, "without the alias neither D0 nor D1 is a member of {{DATA}}: {:?}", erc_kinds(&dir));

    step(&dir, Cmd::SetBusAliases { aliases: vec![alias("DATA", &["D0", "D1"])] }, false, "test").unwrap();
    assert_eq!(conflicts(&dir), 0, "the alias makes D0 and D1 members of {{DATA}}: {:?}", erc_kinds(&dir));
    let (_, design, _) = load(&dir).unwrap();
    let bus = design.schematic.as_ref().unwrap().wires.iter().find(|w| w.bus).unwrap().id.clone();
    let j = crate::studio::schematic_json(&dir, "", crate::text_vars_json::clock_of("/api/schematic")).unwrap();
    let drawn = j["wires"].as_array().unwrap().iter().find(|w| w["id"] == bus.as_str()).unwrap();
    assert_eq!(drawn["members"], serde_json::json!(["D0", "D1"]), "the bus unfolds to the alias's members");

    // the project alone: the same files without the schematic's `(bus_alias ..)` blocks
    let work = dir.join(".kicad");
    let sch_path = work.join("board.kicad_sch");
    let text = std::fs::read_to_string(&sch_path).unwrap();
    let mut kept = Vec::new();
    let mut skipping = false;
    for line in text.lines() {
        if !skipping && line.trim_start().starts_with("(bus_alias") {
            skipping = true;
        } else if skipping {
            if line.trim() == ")" {
                skipping = false;
            }
        } else {
            kept.push(line);
        }
    }
    assert!(!kept.iter().any(|l| l.contains("bus_alias")) && text.contains("(bus_alias"), "the legacy block was there and is gone");
    std::fs::write(&sch_path, kept.join("\n") + "\n").unwrap();
    let pro: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(work.join("board.kicad_pro")).unwrap()).unwrap();
    assert_eq!(pro["schematic"]["bus_aliases"], serde_json::json!({ "DATA": ["D0", "D1"] }));
    let cli = eda_kicad_engine::find_cli().unwrap();
    let kinds_of = |sch: &Path| -> Vec<String> {
        let report = work.join("alias_only.json");
        let _ = std::fs::remove_file(&report);
        let _ = eda_kicad_engine::cli_command(&cli, Some(&work.join("board.kicad_pro"))).args(["sch", "erc", "--format", "json", "--severity-all", "-o"]).arg(&report).arg(sch).status().unwrap();
        let raw: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&report).expect("kicad-cli wrote its report")).unwrap();
        raw["sheets"].as_array().into_iter().flatten().flat_map(|s| s["violations"].as_array().into_iter().flatten().map(|v| v["type"].as_str().unwrap_or("").to_string())).collect()
    };
    let kinds = kinds_of(&sch_path);
    assert!(!kinds.iter().any(|k| k == "net_not_bus_member"), "kicad-cli reads the alias from the project's schematic.bus_aliases: {kinds:?}");
    // and without it there either, the members are gone again (the probe is the alias, not the edit of the file)
    let mut bare = pro.clone();
    bare["schematic"].as_object_mut().unwrap().remove("bus_aliases");
    std::fs::write(work.join("board.kicad_pro"), serde_json::to_string_pretty(&bare).unwrap()).unwrap();
    assert_eq!(kinds_of(&sch_path).iter().filter(|k| *k == "net_not_bus_member").count(), 2, "with neither, the nets are not members");

    undo(&dir, "test", Some(Domain::Schematic)).unwrap();
    assert_eq!(conflicts(&dir), 2, "one undo takes the alias back");
}

/// Net Classes in kicad-cli: a label's class is undefined (`undefined_netclass`, `ERC_TESTER::TestMissingNetclasses`) until Schematic Setup > Net Classes
/// defines it, and kicad-cli's plot draws the label's wire in the class's colour (`SCH_LINE::GetLineColor` from `GetEffectiveNetClass()`) and the junction
/// dots at Formatting's size.
#[test]
fn slow_kicad_cli_reads_the_net_classes_schematic_setup_defines() {
    if !kicad_cli_ready() {
        return;
    }
    let dir = mcu30_board("slow_setup_classes");
    let (_, design, _) = load(&dir).unwrap();
    let root = design.schematic.as_ref().unwrap();
    let label = root.labels.iter().find(|l| l.kind == eda_model::ir::LabelKind::Local && root.wires.iter().any(|w| w.pts.contains(&l.at))).expect("a local label on a wire").clone();
    step(&dir, edit_label_fields(&label.id, vec![netclass("Signal")]), false, "test").unwrap();
    let undefined = |dir: &Path| erc_kinds(dir).get("undefined_netclass").copied().unwrap_or(0);
    assert_eq!(undefined(&dir), 1, "kicad-cli reads the label's class and the project has none of that name");

    let svg_before = plot_svg(&dir);
    define_class(&dir, "Signal", NetClassSchematic { wire_width: Some(Nm::new(20 * 25_400)), bus_width: None, color: Some("rgb(255, 0, 0)".into()), line_style: None });
    step(&dir, Cmd::SetSchFormatting { settings: SchFormatting { junction_size_choice: 5, ..SchFormatting::default() } }, false, "test").unwrap();
    assert_eq!(undefined(&dir), 0, "Schematic Setup defined the class: {:?}", erc_kinds(&dir));
    let svg = plot_svg(&dir);
    let red = |s: &str| s.to_ascii_uppercase().matches("#FF0000").count();
    assert!(red(&svg) > red(&svg_before), "kicad-cli's plot draws the class's wire red ({} red strokes before, {} after)", red(&svg_before), red(&svg));
    // the junction dots (`SCH_JUNCTION::Plot`, a filled circle of `GetEffectiveDiameter`): 36 mils across at "Default", 72 at "Largest" (12 times the 6 mil wire)
    let radii = |s: &str| -> std::collections::BTreeSet<String> { s.split("<circle ").skip(1).filter_map(|c| c.split("r=\"").nth(1).and_then(|r| r.split('"').next()).map(String::from)).collect() };
    assert!(radii(&svg_before).contains("0.4572"), "a Default dot is 0.4572 mm in radius: {:?}", radii(&svg_before));
    assert!(radii(&svg).contains("0.9144") && !radii(&svg).contains("0.4572"), "kicad-cli reads the junction size Schematic Setup set: {:?}", radii(&svg));
}

/// The root sheet as kicad-cli plots it to SVG (`sch export svg`, the derived project beside it).
fn plot_svg(dir: &Path) -> String {
    let done = crate::kicad_engine::export_sch(dir, "svg", &[]).expect("kicad-cli plots the schematic");
    let mut text = String::new();
    for f in done["files"].as_array().into_iter().flatten().filter_map(|f| f.as_str()) {
        text.push_str(&std::fs::read_to_string(dir.join(f)).unwrap_or_default());
    }
    text
}
