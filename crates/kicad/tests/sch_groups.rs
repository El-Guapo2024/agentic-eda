//! Schematic groups in the `.kicad_sch` the studio writes and reads (package P10b): `(group "name" (uuid ..) (members "uuid" ..))`, as `SCH_IO_KICAD_SEXPR::saveGroup`
//! writes it and `parseGroup` / `resolveGroups` read it -- a group names its members by the uuids of the items, and a group in a group by that group's uuid.
//!
//! The pure-Rust tests are our writer and reader (and the clipboard's). The slow one hands the file to kicad-cli (`EDA_SLOW_TESTS=1`, and it skips when there is none):
//! it loads a file with nested groups, runs its ERC, and its own writer (`sch upgrade`) keeps the groups only when it found every member by the uuid we gave.

use eda_model::units::mm;
use eda_kicad::{export_kicad_sch, export_kicad_sch_mapped, import_kicad_sch, parse_clipboard, write_clipboard, CopyInput, ExportMeta};
use eda_model::ir::{Design, Group, LabelKind, NetLabel, Point, Provenance, SchematicSection, SchematicText, SymbolInstance, Wire};
use eda_model::symbol::{LibPin, LibSymbol, SPoint, SymbolGraphic};
use eda_model::{ConstraintModel, Part, Pin, PinKind};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

const META: ExportMeta<'static> = ExportMeta { date: "2026-10-13", title: "groups" };

fn pin(number: &str, name: &str, etype: &str, x: f64, y: f64, angle: f64) -> LibPin {
    LibPin { number: number.into(), name: name.into(), electrical_type: etype.into(), shape: "line".into(), at: SPoint::new(x, y), angle_deg: angle, length_mm: 2.54, unit: 1, hidden: false }
}

/// A two-input gate: A and B on the left (one grid cell apart), Y on the right.
fn gate() -> LibSymbol {
    LibSymbol { connectivity: Default::default(),
        lib_id: "test:GATE".into(),
        graphics: vec![SymbolGraphic::Rectangle { unit: 1, start: SPoint::new(-2.54, 5.08), end: SPoint::new(2.54, -5.08), stroke_mm: 0.254, filled: false }],
        pins: vec![pin("1", "A", "input", -7.62, 2.54, 0.0), pin("2", "B", "input", -7.62, 0.0, 0.0), pin("3", "Y", "output", 7.62, 0.0, 180.0)],
        power: false,
        in_bom: true,
        on_board: true,
        datasheet: String::new(),
        description: String::new(),
        reference_prefix: "U".into(),
        unit_count: 1,
        pin_names_hidden: false,
        pin_numbers_hidden: false,
        pin_name_offset_mm: 0.508,
        alternate: None,
    }
}

fn model() -> ConstraintModel {
    let pins = ["1", "2", "3"].map(|n| Pin { number: n.into(), name: None, kind: PinKind::Signal }).to_vec();
    let part = Part { reference: "U1".into(), mpn: None, lcsc: None, value: Some("GATE".into()), package: None, footprint: Some("Foo:Bar".into()), symbol: Some("test:GATE".into()), datasheet: None, pins, body_nm: None, edge: None };
    ConstraintModel { parts: vec![part], symbols: vec![gate()], ..Default::default() }
}

fn design_with(sch: SchematicSection) -> Design {
    Design { schema: eda_model::ir::SCHEMA_VERSION, provenance: Provenance { engine_version: "0".into(), intent_hash: "x".into(), seed: 0, stage_hashes: vec![] }, schematic: Some(sch), nets: None, placement: None, routing: None, drawings: None, footprint_library: None, sheet_contents: None, bus_aliases: Default::default(), symbol_library: None }
}

/// The gate with a wire from each pin out to a label (`NET_A`, `NET_B`, `NET_Y`), a text, and these groups: `inner = {w_NET_A, l_NET_A}` named "net A",
/// `outer = {inner, U1}` named "gate" and `other = {l_NET_B, l_NET_Y, txt_note}` named "the rest".
fn grouped(m: &ConstraintModel) -> SchematicSection {
    let sym = SymbolInstance { id: "U1".into(), at: Point { x: mm(76.2), y: mm(76.2) }, rot: 0, mirrored: false, mirror_y: false, lib_id: "test:GATE".into(), unit: 1, value: "GATE".into(), footprint: String::new(), datasheet: String::new(), dnp: false, exclude_from_bom: false, exclude_from_board: false, exclude_from_sim: false };
    let mut sch = SchematicSection { symbols: vec![sym.clone()], ..Default::default() };
    let part = m.part("U1").unwrap();
    for (number, tip) in eda_engine::placed::pin_points(&sym, part, m.real_symbol_of(&sym.lib_id, part).as_ref()) {
        let left = number != "3";
        let end = Point { x: tip.x + if left { mm(-12.7) } else { mm(12.7) }, y: tip.y };
        let net = format!("NET_{}", ["A", "B", "Y"][number.parse::<usize>().unwrap() - 1]);
        // the wire of net A bends once: two segments in the file, one wire here
        let pts = if net == "NET_A" { vec![tip, Point { x: end.x + mm(5.08), y: tip.y }, end] } else { vec![tip, end] };
        sch.wires.push(Wire { id: format!("w_{net}"), net: net.clone(), pins: vec![], pts, bus: false });
        sch.labels.push(NetLabel { id: format!("l_{net}"), net, at: end, kind: LabelKind::Local });
    }
    sch.texts.push(SchematicText { id: "txt_note".into(), content: "a note".into(), at: Point { x: mm(30.0), y: mm(40.0) }, angle: 0, size_nm: mm(1.27) });
    let group = |id: &str, name: &str, members: &[&str]| Group { id: id.into(), name: name.into(), member_ids: members.iter().map(|s| s.to_string()).collect(), design_block: String::new() };
    sch.extras.groups = vec![group("grp_inner", "net A", &["w_NET_A", "l_NET_A"]), group("grp_outer", "gate", &["grp_inner", "U1"]), group("grp_other", "the rest", &["l_NET_B", "l_NET_Y", "txt_note"])];
    sch
}

/// The balanced `(...)` that starts at `at` in `text` (quotes respected).
fn balanced(text: &str, at: usize) -> &str {
    let (mut depth, mut quoted) = (0i32, false);
    for (i, c) in text[at..].char_indices() {
        match c {
            '"' => quoted = !quoted,
            '(' if !quoted => depth += 1,
            ')' if !quoted => {
                depth -= 1;
                if depth == 0 {
                    return &text[at..at + i + 1];
                }
            }
            _ => {}
        }
    }
    &text[at..]
}

/// The quoted strings of `form`, in order.
fn quoted(form: &str) -> Vec<String> {
    form.split('"').skip(1).step_by(2).map(String::from).collect()
}

/// The `(group ..)` forms of `file`, as our writer or KiCad's lays them out: (name, uuid, member uuids).
fn group_forms(file: &str) -> Vec<(String, String, Vec<String>)> {
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(i) = file[from..].find("(group ") {
        let at = from + i;
        let form = balanced(file, at);
        from = at + form.len().max(1);
        let name = quoted(form).into_iter().next().unwrap_or_default();
        let uuid = form.find("(uuid").map(|u| quoted(balanced(form, u)).remove(0)).expect("a group has a uuid");
        let members = form.find("(members").map(|m| quoted(balanced(form, m))).expect("a group names its members");
        out.push((name, uuid, members));
    }
    out
}

#[test]
fn groups_are_written_naming_their_members_by_the_uuids_of_the_items() {
    let m = model();
    let (file, uuids) = export_kicad_sch_mapped(&design_with(grouped(&m)), &m, &META).expect("the sheet exports");
    let forms = group_forms(&file);
    assert_eq!(forms.len(), 3, "{}", file.lines().filter(|l| l.contains("(group")).collect::<Vec<_>>().join("\n"));
    let by_name: BTreeMap<&str, &(String, String, Vec<String>)> = forms.iter().map(|f| (f.0.as_str(), f)).collect();
    // every member is what the file says it is: a uuid that maps back to the item we meant
    let ids = |name: &str| -> Vec<String> {
        let mut v: Vec<String> = by_name[name].2.iter().map(|u| uuids.get(u).cloned().unwrap_or_else(|| panic!("{name}: member {u} is no item of the file"))).collect();
        v.sort();
        v.dedup();
        v
    };
    // the wire that bends is two segments: both are members
    assert_eq!(by_name["net A"].2.len(), 3, "two segments of the wire and the label: {:?}", by_name["net A"]);
    assert_eq!(ids("net A"), vec!["l_NET_A".to_string(), "w_NET_A".to_string()]);
    assert_eq!(ids("the rest"), vec!["l_NET_B".to_string(), "l_NET_Y".to_string(), "txt_note".to_string()]);
    // a group in a group is named by its own uuid
    assert!(by_name["gate"].2.contains(&by_name["net A"].1), "the outer group holds the inner one by its uuid: {:?}", by_name["gate"]);
    assert!(by_name["gate"].2.iter().any(|u| uuids.get(u).map(String::as_str) == Some("U1")), "and the symbol by its uuid");
    // members are sorted, as `saveGroup` sorts them
    for f in &forms {
        let mut sorted = f.2.clone();
        sorted.sort();
        assert_eq!(f.2, sorted, "{}", f.0);
    }
}

#[test]
fn a_sheet_without_groups_writes_none_and_a_locked_group_says_so() {
    let m = model();
    let mut plain = grouped(&m);
    plain.extras.groups.clear();
    assert!(!export_kicad_sch(&design_with(plain), &m, &META).unwrap().contains("(group"));
    let mut locked = grouped(&m);
    locked.extras.set_locked("grp_other", true);
    let file = export_kicad_sch(&design_with(locked), &m, &META).unwrap();
    let line = file.lines().find(|l| l.contains("\"the rest\"")).expect("the group is written");
    assert!(line.contains("(locked yes)"), "{line}");
    assert!(!file.lines().find(|l| l.contains("\"gate\"")).unwrap().contains("locked"));
}

#[test]
fn a_group_none_of_whose_members_is_written_is_not_written() {
    let m = model();
    let mut sch = grouped(&m);
    sch.extras.groups.push(Group { id: "grp_ghost".into(), name: "ghost".into(), member_ids: vec!["no_such_item".into(), "nor_this".into()], design_block: String::new() });
    let file = export_kicad_sch(&design_with(sch), &m, &META).unwrap();
    assert!(!file.contains("\"ghost\""), "an empty group is not written (`if( aGroup->GetItems().empty() ) return;`)");
    assert_eq!(group_forms(&file).len(), 3);
}

#[test]
fn groups_read_back_with_the_same_names_and_members() {
    let m = model();
    let sch = grouped(&m);
    let file = export_kicad_sch(&design_with(sch.clone()), &m, &META).unwrap();
    let (design, _, _) = import_kicad_sch(&file).expect("the file reads back");
    let back = design.schematic.expect("a schematic");
    assert_eq!(back.extras.groups.len(), 3, "{:?}", back.extras.groups);
    let named = |name: &str| back.extras.groups.iter().find(|g| g.name == name).unwrap_or_else(|| panic!("no group {name}: {:?}", back.extras.groups));
    let (inner, outer, rest) = (named("net A"), named("gate"), named("the rest"));
    // the wire is two wires now (a file has one per segment): the group holds both, and the label
    assert_eq!(inner.member_ids.len(), 3, "{inner:?}");
    let label_of = |id: &str| back.labels.iter().find(|l| l.id == id).map(|l| l.net.clone());
    let wire_of = |id: &str| back.wires.iter().find(|w| w.id == id).map(|w| (w.net.clone(), w.pts.len()));
    let labels: Vec<String> = inner.member_ids.iter().filter_map(|id| label_of(id)).collect();
    assert_eq!(labels, vec!["NET_A".to_string()]);
    assert_eq!(inner.member_ids.iter().filter(|id| wire_of(id).is_some()).count(), 2, "both segments");
    // the symbol is U1 (a symbol's id is its reference), and the outer group holds the inner one
    assert!(outer.member_ids.contains(&"U1".to_string()), "{outer:?}");
    assert!(outer.member_ids.contains(&inner.id), "{outer:?}");
    assert_eq!(back.extras.parent_group(&inner.id).map(|g| g.id.clone()), Some(outer.id.clone()));
    // the rest: two labels and the text
    let mut nets: Vec<String> = rest.member_ids.iter().filter_map(|id| label_of(id)).collect();
    nets.sort();
    assert_eq!(nets, vec!["NET_B".to_string(), "NET_Y".to_string()]);
    assert!(rest.member_ids.iter().any(|id| back.texts.iter().any(|t| &t.id == id && t.content == "a note")));

    // and a second trip changes nothing: the structure is stable
    let again = export_kicad_sch(&design_with(back.clone()), &m, &META).unwrap();
    let names = |f: &str| {
        let mut v: Vec<(String, usize)> = group_forms(f).into_iter().map(|g| (g.0, g.2.len())).collect();
        v.sort();
        v
    };
    assert_eq!(names(&again), names(&file));
}

#[test]
fn a_member_the_file_does_not_have_leaves_the_group_and_a_group_of_one_stays() {
    let m = model();
    let file = export_kicad_sch(&design_with(grouped(&m)), &m, &META).unwrap();
    // a member uuid that no item has
    let forms = group_forms(&file);
    let rest = forms.iter().find(|f| f.0 == "the rest").unwrap();
    let broken = file.replacen(&rest.2[0], "00000000-0000-4000-8000-000000000000", 1);
    let (design, _, _) = import_kicad_sch(&broken).unwrap();
    let g = design.schematic.unwrap().extras.groups.into_iter().find(|g| g.name == "the rest").expect("two members are left");
    assert_eq!(g.member_ids.len(), 2);
    // ... two unknown ones leave one: still a group (`SCH_IO_KICAD_SEXPR_PARSER::resolveGroups` makes every group it reads; r2-pcb-edit A1)
    let broken = broken.replacen(&rest.2[1], "00000000-0000-4000-8000-000000000001", 1);
    let (design, _, _) = import_kicad_sch(&broken).unwrap();
    assert_eq!(design.schematic.unwrap().extras.groups.iter().find(|g| g.name == "the rest").map(|g| g.member_ids.len()), Some(1));
}

#[test]
fn a_loop_of_groups_in_a_file_made_by_hand_is_dropped_not_followed() {
    let m = model();
    let file = export_kicad_sch(&design_with(grouped(&m)), &m, &META).unwrap();
    let forms = group_forms(&file);
    let (inner, outer) = (forms.iter().find(|f| f.0 == "net A").unwrap(), forms.iter().find(|f| f.0 == "gate").unwrap());
    // the inner group also names the outer one: a cycle `GroupsSanityCheck` repairs by removing a group
    let looped = file.replacen(&format!("(members \"{}\"", inner.2[0]), &format!("(members \"{}\" \"{}\"", outer.1, inner.2[0]), 1);
    let (design, _, _) = import_kicad_sch(&looped).expect("a cycle is not a parse error");
    let back = design.schematic.unwrap();
    // whatever was kept has no group above itself
    for g in &back.extras.groups {
        let mut at = back.extras.parent_group(&g.id);
        let mut hops = 0;
        while let Some(p) = at {
            hops += 1;
            assert!(hops <= back.extras.groups.len() && p.id != g.id, "a group holds itself through {}", p.name);
            at = back.extras.parent_group(&p.id);
        }
    }
}

#[test]
fn a_group_linked_to_a_design_block_writes_its_lib_id_and_reads_back_with_the_link() {
    let m = model();
    let mut sch = grouped(&m);
    // `(lib_id "Lib:Name")` is the link (`SCH_IO_KICAD_SEXPR::saveGroup`: after the lock, before the members); a group of one item that is linked is written and kept
    sch.extras.groups[2].design_block = "Power:Buck".to_string();
    sch.extras.groups.push(Group { id: "grp_solo".into(), name: "alone".into(), member_ids: vec!["U1".into()], design_block: "Power:Single".into() });
    // U1 is held by "gate": a member is one group's at a time, so the one-item group takes the text instead
    sch.extras.groups[1].member_ids.retain(|m| m != "U1");
    let file = export_kicad_sch(&design_with(sch), &m, &META).unwrap();
    let line = file.lines().find(|l| l.contains("\"the rest\"")).expect("the group is written");
    assert!(line.contains("(lib_id \"Power:Buck\")") && line.find("(uuid").unwrap() < line.find("(lib_id").unwrap() && line.find("(lib_id").unwrap() < line.find("(members").unwrap(), "{line}");
    assert!(!file.lines().find(|l| l.contains("\"net A\"")).unwrap().contains("lib_id"), "an unlinked group writes none");
    let (design, _, _) = import_kicad_sch(&file).expect("the file reads back");
    let back = design.schematic.unwrap().extras.groups;
    let link_of = |name: &str| back.iter().find(|g| g.name == name).map(|g| (g.design_block.clone(), g.member_ids.len()));
    assert_eq!(link_of("the rest"), Some(("Power:Buck".to_string(), 3)));
    assert_eq!(link_of("alone"), Some(("Power:Single".to_string(), 1)), "a group of one that is linked is kept");
    assert_eq!(link_of("net A").map(|l| l.0), Some(String::new()));
    // a link that is no `Lib:Name` is read as none
    let broken = file.replace("(lib_id \"Power:Buck\")", "(lib_id \"nonsense\")");
    let (design, _, _) = import_kicad_sch(&broken).unwrap();
    assert!(design.schematic.unwrap().extras.groups.iter().all(|g| g.design_block != "nonsense"));
    // and the clipboard carries the link of a copied group
    let text = copy_of(&m, &grouped_with_link(&m), &["grp_other"]);
    assert!(text.contains("(lib_id \"Power:Buck\")"), "{text}");
    let fragment = parse_clipboard(&text).expect("reads back");
    assert_eq!(fragment.section.extras.groups.iter().find(|g| g.name == "the rest").map(|g| g.design_block.clone()), Some("Power:Buck".to_string()));
}

fn grouped_with_link(m: &ConstraintModel) -> SchematicSection {
    let mut sch = grouped(m);
    sch.extras.groups[2].design_block = "Power:Buck".to_string();
    sch
}

// ---------------------------------------------------------------------------------------------------------------------- the clipboard

fn copy_of(m: &ConstraintModel, sch: &SchematicSection, ids: &[&str]) -> String {
    let design = design_with(sch.clone());
    let input = CopyInput { design: &design, model: m, section: sch, project: "groups", sheet_path: &[] };
    write_clipboard(&input, &ids.iter().map(|s| s.to_string()).collect::<Vec<_>>()).expect("a copy").text
}

#[test]
fn copying_a_group_copies_everything_below_it_and_writes_the_groups() {
    let m = model();
    let sch = grouped(&m);
    // the outer group alone: the symbol, the wire's segments and the label are in the text, and so are the inner and the outer group
    let text = copy_of(&m, &sch, &["grp_outer"]);
    assert!(text.contains("(symbol\n\t\t(lib_id \"test:GATE\")"), "the symbol below the outer group");
    assert_eq!(text.matches("\t(wire\n").count(), 2, "the wire that bends is two segments");
    assert!(text.contains("(label \"NET_A\""), "the label of the inner group");
    assert!(!text.contains("NET_B"), "what is in another group stays");
    let forms = group_forms(&text);
    let mut names: Vec<&str> = forms.iter().map(|f| f.0.as_str()).collect();
    names.sort();
    assert_eq!(names, vec!["gate", "net A"]);

    // read back as a fragment: the same two groups, the inner one in the outer one
    let fragment = parse_clipboard(&text).expect("the clipboard reads back");
    let g = &fragment.section.extras.groups;
    assert_eq!(g.len(), 2, "{g:?}");
    let outer = g.iter().find(|x| x.name == "gate").unwrap();
    let inner = g.iter().find(|x| x.name == "net A").unwrap();
    assert!(outer.member_ids.contains(&inner.id) && outer.member_ids.contains(&"U1".to_string()), "{outer:?}");
    // the two segments of the wire were joined again into the one wire, and the inner group holds that wire and the label
    assert_eq!(fragment.section.wires.len(), 1);
    assert_eq!(inner.member_ids.len(), 2, "{inner:?}");
    assert!(inner.member_ids.contains(&fragment.section.wires[0].id) && inner.member_ids.contains(&fragment.section.labels[0].id));

    // an item copied alone is no group
    let alone = copy_of(&m, &sch, &["txt_note"]);
    assert!(group_forms(&alone).is_empty() && alone.contains("a note"));
    // a group copied with an item outside it keeps both
    let both = copy_of(&m, &sch, &["grp_other", "U1"]);
    assert_eq!(group_forms(&both).iter().map(|f| f.0.as_str()).collect::<Vec<_>>(), vec!["the rest"]);
}

// ---------------------------------------------------------------------------------------------------------------------- kicad-cli

fn find_kicad_cli() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("EDA_KICAD_CLI").map(PathBuf::from).filter(|p| p.exists()) {
        return Some(p);
    }
    if let Ok(out) = Command::new("which").arg("kicad-cli").output() {
        let p = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if out.status.success() && !p.is_empty() {
            return Some(PathBuf::from(p));
        }
    }
    Some(PathBuf::from("/Applications/KiCad/KiCad.app/Contents/MacOS/kicad-cli")).filter(|p| p.exists())
}

/// `kicad-cli sch erc` on `file`: the report, which exists only when the file loaded.
fn erc(cli: &Path, file: &Path) -> serde_json::Value {
    let report = file.with_extension("erc.json");
    let out = Command::new(cli).args(["sch", "erc", "--format", "json", "--output"]).arg(&report).arg(file).output().expect("run kicad-cli");
    assert!(report.exists(), "kicad-cli did not produce an ERC report for {}: {}{}", file.display(), String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    serde_json::from_str(&std::fs::read_to_string(&report).unwrap()).expect("the ERC report is JSON")
}

#[test]
fn kicad_loads_nested_groups_and_writes_them_back_with_every_member() {
    if std::env::var_os("EDA_SLOW_TESTS").is_none() {
        eprintln!("skipped: slow test; set EDA_SLOW_TESTS=1 to run it");
        return;
    }
    let Some(cli) = find_kicad_cli() else {
        eprintln!("skipped: no kicad-cli");
        return;
    };
    let m = model();
    let (file_text, uuids) = export_kicad_sch_mapped(&design_with(grouped(&m)), &m, &META).expect("the sheet exports");
    let dir = std::env::temp_dir().join(format!("eda_sch_groups_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("groups.kicad_sch");
    std::fs::write(&file, &file_text).unwrap();

    // 1. the ERC runs: the file, with its groups, loads
    let report = erc(&cli, &file);
    assert!(report.get("sheets").is_some(), "{report}");

    // 2. KiCad reads the file and writes it again (`sch upgrade --force`, its own writer): `saveGroup` writes a group only if it found members, and it writes
    //    the members it found -- so what is left in the file is what it understood of ours
    let run = Command::new(&cli).args(["sch", "upgrade", "--force"]).arg(&file).output().expect("run kicad-cli");
    assert!(run.status.success(), "kicad-cli sch upgrade: {}{}", String::from_utf8_lossy(&run.stdout), String::from_utf8_lossy(&run.stderr));
    let back = std::fs::read_to_string(&file).unwrap();
    let ours = group_forms(&file_text);
    let theirs = group_forms(&back);
    assert_eq!(theirs.len(), ours.len(), "KiCad kept every group:\n{}", back.lines().filter(|l| l.contains("(group")).collect::<Vec<_>>().join("\n"));
    for (name, uuid, members) in &ours {
        let kept = theirs.iter().find(|g| &g.0 == name).unwrap_or_else(|| panic!("KiCad lost the group {name}"));
        // the group keeps its uuid, and every member we named is still a member
        assert_eq!(&kept.1, uuid, "{name}");
        let (mut want, mut got) = (members.clone(), kept.2.clone());
        want.sort();
        got.sort();
        assert_eq!(got, want, "{name}: members {:?}", members.iter().map(|u| uuids.get(u).cloned().unwrap_or_else(|| "a group".into())).collect::<Vec<_>>());
    }
    let _ = std::fs::remove_dir_all(&dir);
}

// -------------------------------------------------------------------------------------------------------- a placed user field (P10b step 2)

/// The invisible `<text x=".." y=".." textLength=".." text-anchor="..">content</text>` element kicad-cli's SVG export writes beside the strokes of each text: where
/// the text is, in millimetres of the page: the x and y of its middle and the length of the text.
fn svg_text(svg: &str, content: &str) -> Option<(f64, f64, f64)> {
    let end = svg.find(&format!(">{content}</text>"))?;
    let start = svg[..end].rfind("<text ")?;
    let tag = &svg[start..end];
    let attr = |name: &str| -> Option<f64> {
        let from = tag.find(&format!("{name}=\""))? + name.len() + 2;
        tag[from..].split('"').next()?.parse().ok()
    };
    Some((attr("x")?, attr("y")?, attr("textLength")?))
}

#[test]
fn kicad_draws_a_placed_user_field_where_we_put_it_in_a_sheet_with_nested_groups() {
    if std::env::var_os("EDA_SLOW_TESTS").is_none() {
        eprintln!("skipped: slow test; set EDA_SLOW_TESTS=1 to run it");
        return;
    }
    let Some(cli) = find_kicad_cli() else {
        eprintln!("skipped: no kicad-cli");
        return;
    };
    // the sheet of the groups test, with two user fields of U1: `MPN`, shown 12.7 mm right of and 20.32 mm above the origin of the symbol (left justified, centred
    // on its line), and `Manufacturer`, hidden
    let m = model();
    let mut sch = grouped(&m);
    let user = sch.user_fields.entry("U1".into()).or_default();
    user.insert("MPN".into(), "74LVC1G08".into());
    user.insert("Manufacturer".into(), "TexasInstruments".into());
    sch.field_layout.insert("U1".into(), vec![eda_model::ir::FieldPlacement { dx: mm(12.7), dy: mm(-20.32), visible: true, h: eda_model::ir::TextJustify::Left, ..eda_model::ir::FieldPlacement::at_origin("MPN") }]);
    let file_text = export_kicad_sch(&design_with(sch), &m, &META).expect("the sheet exports");
    let (ax, ay) = (76.2 + 12.7, 76.2 - 20.32);
    assert!(file_text.contains(&format!("(property \"MPN\" \"74LVC1G08\" (at {ax} {ay} 0)")), "our file puts the field at {ax} {ay}:\n{}", file_text.lines().filter(|l| l.contains("MPN")).collect::<Vec<_>>().join("\n"));

    let dir = std::env::temp_dir().join(format!("eda_sch_user_field_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("field.kicad_sch");
    std::fs::write(&file, &file_text).unwrap();

    // 1. the ERC runs: a file with nested groups and a user field loads
    assert!(erc(&cli, &file).get("sheets").is_some());

    // 2. the SVG draws the shown field where we put it, and not the hidden one
    let svg_dir = dir.join("svg");
    let out = Command::new(&cli).args(["sch", "export", "svg", "--no-background-color", "--output"]).arg(&svg_dir).arg(&file).output().expect("run kicad-cli");
    assert!(out.status.success(), "kicad-cli sch export svg: {}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    let svg = std::fs::read_to_string(svg_dir.join("field.svg")).expect("the SVG of the sheet");
    let (x, y, len) = svg_text(&svg, "74LVC1G08").unwrap_or_else(|| panic!("the field is not in the SVG"));
    // text-anchor "middle": x is the middle of a text that starts at its anchor; y is the middle of the line, half the 1.27 mm text height below the anchor
    assert!((x - (ax + len / 2.0)).abs() < 0.05, "x {x}, wanted {}", ax + len / 2.0);
    assert!((y - (ay + 0.635)).abs() < 0.05, "y {y}, wanted {}", ay + 0.635);
    assert!(svg_text(&svg, "TexasInstruments").is_none(), "a hidden field is not drawn");

    // 3. KiCad's own writer keeps the field, where it is and hidden or not, and the groups
    let run = Command::new(&cli).args(["sch", "upgrade", "--force"]).arg(&file).output().expect("run kicad-cli");
    assert!(run.status.success(), "kicad-cli sch upgrade: {}{}", String::from_utf8_lossy(&run.stdout), String::from_utf8_lossy(&run.stderr));
    let back = std::fs::read_to_string(&file).unwrap();
    assert!(back.contains(&format!("(property \"MPN\" \"74LVC1G08\"\n\t\t\t(at {ax} {ay} 0)")) || back.contains(&format!("(property \"MPN\" \"74LVC1G08\" (at {ax} {ay} 0)")), "KiCad kept the place:\n{}", back.lines().filter(|l| l.contains("MPN")).collect::<Vec<_>>().join("\n"));
    let manufacturer = back.find("\"TexasInstruments\"").expect("KiCad kept the hidden field");
    assert!(back[manufacturer..].lines().take(6).any(|l| l.contains("(hide yes)")), "and kept it hidden");
    assert_eq!(group_forms(&back).len(), 3, "and the three groups");
    let _ = std::fs::remove_dir_all(&dir);
}
