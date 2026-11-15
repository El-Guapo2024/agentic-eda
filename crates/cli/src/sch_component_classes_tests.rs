//! The component classes the schematic gives each footprint (`crate::sch_component_classes`), through a board directory: KiCad's own QA netlists say which class
//! each component of their schematics has, and the studio's links say the same; a symbol's `Component Class` field and a rule area's directive label give the
//! footprint its class, one undo step each; and -- slow -- kicad-cli's netlist of the schematic the studio writes agrees with the links.

use crate::board::{load, step, undo};
use crate::sch_hierarchy_tests::{kicad_cli_ready, mcu30_board};
use eda_model::sch_extras::{DirectiveShape, SchGraphic, SchGraphicKind};
use eda_model::units::Nm;
use eda_ops::{Cmd, Domain};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

fn mm(v: f64) -> Nm {
    Nm::from_mm(v)
}

/// The classes the board's links give each reference (the ones with none left out).
fn link_classes(dir: &Path) -> BTreeMap<String, Vec<String>> {
    let (_, _, model) = load(dir).unwrap();
    model.symbol_links.iter().filter_map(|(r, l)| l.component_classes.clone().filter(|c| !c.is_empty()).map(|c| (r.clone(), c))).collect()
}

/// The classes a KiCad netlist gives each reference (`(component_classes (class ..))`).
fn netlist_classes(text: &str, name: &str) -> BTreeMap<String, Vec<String>> {
    let netlist = eda_kicad::netlist_reader::read_netlist(text, name).unwrap();
    netlist.components.iter().filter(|c| !c.component_classes.is_empty()).map(|c| (c.reference.clone(), c.component_classes.iter().cloned().collect())).collect()
}

fn qa_data() -> Option<PathBuf> {
    let dir = PathBuf::from(std::env::var_os("KICAD_QA_DATA")?);
    dir.is_dir().then_some(dir)
}

/// KiCad's QA schematics with component classes (`eeschema/netlists/component_classes`: fields on symbols and on directive labels of rule areas, a symbol of
/// several units; `hierarchical_component_classes`: rule areas around sheets, a sheet placed twice), imported: every component has the classes KiCad's own
/// netlist of it lists (`test_netlist_exporter_kicad.cpp`'s reference files), no more.
#[test]
fn the_qa_schematics_give_each_component_the_classes_kicads_netlist_lists() {
    let Some(qa) = qa_data() else {
        eprintln!("skipped: KICAD_QA_DATA is not set");
        return;
    };
    for name in ["component_classes", "hierarchical_component_classes"] {
        let src = qa.join("eeschema/netlists").join(name);
        let net = std::fs::read_to_string(src.join(format!("{name}.net"))).unwrap();
        let want = netlist_classes(&net, name);
        assert!(!want.is_empty(), "{name}: the reference netlist lists classes");
        let dir = crate::board::tests::scratch(&format!("qa_compclass_{name}"));
        let out = dir.join("board");
        crate::import_kicad::run(&[src.join(format!("{name}.kicad_sch")).display().to_string(), "-o".into(), out.display().to_string()]).unwrap();
        assert_eq!(link_classes(&out), want, "{name}: the studio's classes are KiCad's netlist's");
    }
}

/// KiCad's QA projects with component classes on the board (`pcbnew/component_classes`, `component_classes_drc`): the classes the schematic gives each
/// footprint are the ones the board file has (the project was updated from its schematic), so a board linked to its schematic keeps them.
#[test]
fn the_qa_projects_schematics_give_the_classes_their_boards_have() {
    let Some(qa) = qa_data() else {
        eprintln!("skipped: KICAD_QA_DATA is not set");
        return;
    };
    for name in ["component_classes", "component_classes_drc"] {
        let src = qa.join("pcbnew");
        let dir = crate::board::tests::scratch(&format!("qa_compclass_pcb_{name}"));
        let (sch, pcb) = (dir.join("sch"), dir.join("pcb"));
        crate::import_kicad::run(&[src.join(format!("{name}.kicad_sch")).display().to_string(), "-o".into(), sch.display().to_string()]).unwrap();
        crate::import_kicad::run(&[src.join(format!("{name}.kicad_pcb")).display().to_string(), "-o".into(), pcb.display().to_string()]).unwrap();
        let (_, design, _) = load(&pcb).unwrap();
        let board: BTreeMap<String, Vec<String>> =
            design.drawings.iter().flat_map(|d| d.footprint_extras.iter()).filter(|e| !e.component_classes.is_empty()).map(|e| (e.id.clone(), eda_model::component_classes::static_classes(Some(e), None))).collect();
        assert!(!board.is_empty(), "{name}: the board file has classes");
        assert_eq!(link_classes(&sch), board, "{name}: the schematic's classes are the board file's");
    }
}

/// The first wire-free symbol of mcu30's root sheet with its box, for a rule area to go around.
fn first_symbol(dir: &Path) -> (String, eda_model::ir::Point) {
    let (_, design, _) = load(dir).unwrap();
    let root = design.schematic.as_ref().unwrap();
    let s = root.symbols.iter().find(|s| s.unit <= 1).unwrap();
    (s.id.clone(), s.at)
}

/// A symbol's `Component Class` field is its footprint's class (`SCH_SYMBOL::GetComponentClassNames`; Update PCB from Schematic sets it,
/// `updateComponentClass`): the link says so, the board's footprint is in the class for the rule engine and is written with it, the field itself is no
/// footprint field (`compFields.erase( "Component Class" )`), and one undo takes it back. A rule area around a symbol with a directive label on its border that
/// carries a `Component Class` gives that class too.
#[test]
fn a_component_class_field_and_a_rule_areas_directive_give_the_footprint_its_class() {
    let dir = mcu30_board("compclass_field");
    let (sym, at) = first_symbol(&dir);
    assert!(link_classes(&dir).is_empty());
    let add = Cmd::SetSymbolFields {
        edits: vec![eda_ops::fields_table::FieldEdit { id: sym.clone(), field: "Component Class".into(), value: "POWER".into() }],
        add_fields: vec!["Component Class".into()],
        rename_fields: vec![],
        remove_fields: vec![],
    };
    step(&dir, add, false, "test").unwrap();
    assert_eq!(link_classes(&dir), BTreeMap::from([(sym.clone(), vec!["POWER".to_string()])]));
    let (_, design, model) = load(&dir).unwrap();
    assert!(!model.symbol_links[&sym].fields.iter().any(|(n, _)| n == "Component Class"), "the class is no footprint field");
    assert_eq!(crate::sch_component_classes::effective_classes(&design, &model).get(&sym), Some(&vec!["POWER".to_string()]));
    undo(&dir, "test", Some(Domain::Schematic)).unwrap();
    assert!(link_classes(&dir).is_empty(), "one undo takes the class back");

    // a rule area around the symbol, and a directive with a class on its border
    let r = mm(30.0).get();
    let corner = |dx: i64, dy: i64| eda_model::ir::Point { x: Nm::new(at.x.get() + dx), y: Nm::new(at.y.get() + dy) };
    let area = SchGraphic::new(SchGraphicKind::RuleArea { pts: vec![corner(-r, -r), corner(r, -r), corner(r, r), corner(-r, r)], exclude_from_sim: false, exclude_from_bom: false, exclude_from_board: false, dnp: false });
    step(&dir, Cmd::SchEdit(eda_ops::sch_edit::SchCmd::AddGraphic { graphic: area }), false, "test").unwrap();
    assert!(link_classes(&dir).is_empty(), "an area with no directive gives no class");
    let flag = SchGraphic::new(SchGraphicKind::Directive { at: corner(-r, -r), orientation: 0, shape: DirectiveShape::Round, pin_length_nm: mm(2.54), netclass: String::new(), component_class: "ANALOG".into(), text_size_nm: Nm::ZERO });
    step(&dir, Cmd::SchEdit(eda_ops::sch_edit::SchCmd::AddGraphic { graphic: flag }), false, "test").unwrap();
    let classes = link_classes(&dir);
    assert_eq!(classes.get(&sym), Some(&vec!["ANALOG".to_string()]), "{classes:?}");
    // a symbol far outside the area has none
    let (_, design, _) = load(&dir).unwrap();
    let far = design.schematic.as_ref().unwrap().symbols.iter().find(|s| (s.at.x.get() - at.x.get()).abs() > 2 * r + mm(40.0).get() || (s.at.y.get() - at.y.get()).abs() > 2 * r + mm(40.0).get());
    if let Some(far) = far {
        assert!(!classes.contains_key(&far.id), "{} is outside the area", far.id);
    }
    undo(&dir, "test", Some(Domain::Schematic)).unwrap();
    assert!(link_classes(&dir).is_empty(), "one undo takes the directive and the class back");
}

/// kicad-cli's netlist of the schematic the studio writes gives each component the classes the studio's links give it (slow): a field, and a rule area's
/// directive.
#[test]
fn slow_kicad_clis_netlist_gives_the_classes_the_links_give() {
    if !kicad_cli_ready() {
        return;
    }
    let dir = mcu30_board("slow_compclass_netlist");
    let (sym, at) = first_symbol(&dir);
    let add = Cmd::SetSymbolFields {
        edits: vec![eda_ops::fields_table::FieldEdit { id: sym.clone(), field: "Component Class".into(), value: "POWER".into() }],
        add_fields: vec!["Component Class".into()],
        rename_fields: vec![],
        remove_fields: vec![],
    };
    step(&dir, add, false, "test").unwrap();
    let r = mm(30.0).get();
    let corner = |dx: i64, dy: i64| eda_model::ir::Point { x: Nm::new(at.x.get() + dx), y: Nm::new(at.y.get() + dy) };
    let area = SchGraphic::new(SchGraphicKind::RuleArea { pts: vec![corner(-r, -r), corner(r, -r), corner(r, r), corner(-r, r)], exclude_from_sim: false, exclude_from_bom: false, exclude_from_board: false, dnp: false });
    step(&dir, Cmd::SchEdit(eda_ops::sch_edit::SchCmd::AddGraphic { graphic: area }), false, "test").unwrap();
    let flag = SchGraphic::new(SchGraphicKind::Directive { at: corner(-r, -r), orientation: 0, shape: DirectiveShape::Round, pin_length_nm: mm(2.54), netclass: String::new(), component_class: "ANALOG".into(), text_size_nm: Nm::ZERO });
    step(&dir, Cmd::SchEdit(eda_ops::sch_edit::SchCmd::AddGraphic { graphic: flag }), false, "test").unwrap();
    let done = crate::kicad_engine::export_sch(&dir, "netlist", &["--format".to_string(), "kicadsexpr".to_string()]).expect("kicad-cli exports the netlist");
    let text = std::fs::read_to_string(dir.join(done["files"][0].as_str().unwrap())).unwrap();
    let kicad = netlist_classes(&text, "kicad-cli");
    let ours = link_classes(&dir);
    assert!(ours.get(&sym).is_some_and(|c| c.contains(&"POWER".to_string()) && c.contains(&"ANALOG".to_string())), "{ours:?}");
    assert_eq!(ours, kicad, "the studio's classes are kicad-cli's netlist's");
}
