//! `Cmd::PasteSch`: the schematic clipboard's Paste, Paste Special and Duplicate.
use super::*;
use eda_model::ir::Provenance;
use eda_model::sch_clipboard::{PasteMode, SchFragment};
use eda_model::units::mm;

fn p(x: Nm, y: Nm) -> Point {
    Point { x, y }
}

fn empty_design() -> Design {
    Design {
        footprint_library: None,
        sheet_contents: None,
        bus_aliases: vec![],
        symbol_library: None,
        schema: eda_model::ir::SCHEMA_VERSION,
        provenance: Provenance { engine_version: "0".into(), intent_hash: "x".into(), seed: 0, stage_hashes: vec![] },
        schematic: None,
        nets: None,
        routing: None,
        placement: None,
        drawings: None,
    }
}

fn symbol(id: &str, lib_id: &str, value: &str, at: Point) -> SymbolInstance {
    SymbolInstance { id: id.into(), at, rot: 0, mirrored: false, mirror_y: false, lib_id: lib_id.into(), unit: 1, value: value.into(), footprint: String::new(), datasheet: String::new(), dnp: false, exclude_from_bom: false, exclude_from_board: false, exclude_from_sim: false }
}

fn wire(pts: &[(Nm, Nm)]) -> Wire {
    Wire { id: String::new(), net: String::new(), pins: vec![], pts: pts.iter().map(|(x, y)| p(*x, *y)).collect(), bus: false }
}

/// A fragment in KiCad's frame: `imported_from_kicad` set, the symbols' `at` their library origin.
fn fragment(f: impl FnOnce(&mut SchematicSection)) -> SchFragment {
    let mut section = SchematicSection { imported_from_kicad: true, ..Default::default() };
    f(&mut section);
    let lib_symbols = ["Device:R", "Device:C", "power:GND"].iter().filter_map(|id| eda_model::symbol::builtin(id)).map(|s| LibrarySymbol::from_engine_symbol(&s)).collect();
    SchFragment { section, lib_symbols, notes: vec![], sheet_screens: Default::default() }
}

fn paste(f: &SchFragment, dx: Nm, dy: Nm, mode: PasteMode) -> Cmd {
    Cmd::PasteSch { fragment: f.clone(), dx, dy, mode }
}

fn refs(sch: &SchematicSection) -> Vec<String> {
    sch.symbols.iter().map(|s| s.id.clone()).collect()
}

#[test]
fn a_paste_adds_every_kind_of_item_translated_with_new_ids_and_no_locks_as_one_command() {
    let m = ConstraintModel::default();
    let mut b = Board::new(empty_design(), &m, mm(0.1), mm(0.3));
    let mut f = fragment(|s| {
        s.symbols.push(symbol("R1", "Device:R", "10k", p(mm(100.0), mm(50.0))));
        s.wires.push(wire(&[(mm(100.0), mm(53.81)), (mm(110.0), mm(53.81)), (mm(110.0), mm(60.0))]));
        s.labels.push(NetLabel { id: "lbl_old".into(), net: "SDA".into(), at: p(mm(110.0), mm(60.0)), kind: LabelKind::Local });
        s.texts.push(SchematicText { id: String::new(), content: "note".into(), at: p(mm(90.0), mm(40.0)), angle: 90_000, size_nm: mm(1.27) });
        s.no_connects.push(NoConnect { id: String::new(), at: p(mm(120.0), mm(50.0)), pin: "R9.1".into() });
        s.junctions.push(eda_model::ir::Junction { id: String::new(), at: p(mm(130.0), mm(50.0)) });
        s.lines.push(SchLine { id: String::new(), pts: vec![p(Nm::ZERO, Nm::ZERO), p(mm(5.0), mm(5.0))], width_nm: mm(0.25) });
        s.extras.locked.push("R1".into());
    });
    f.section.symbols[0].footprint = "Resistor_SMD:R_0603".into();
    f.section.user_fields.insert("R1".into(), [("MPN".to_string(), "RC0603".to_string())].into());
    b.apply(&paste(&f, mm(2.54), mm(-1.27), PasteMode::Unique)).unwrap();
    let sch = b.design().schematic.as_ref().unwrap();
    assert_eq!(refs(sch), vec!["R1"], "a reference nothing has is kept");
    let r = &sch.symbols[0];
    assert_eq!((r.value.as_str(), r.footprint.as_str(), r.lib_id.as_str()), ("10k", "Resistor_SMD:R_0603", "Device:R"));
    assert_eq!(sch.wires.len(), 1);
    assert_eq!(sch.wires[0].pts, vec![p(mm(102.54), mm(52.54)), p(mm(112.54), mm(52.54)), p(mm(112.54), mm(58.73))]);
    assert!(sch.wires[0].id.starts_with("wire_") && sch.wires[0].net.is_empty() && sch.wires[0].pins.is_empty(), "a new wire; its net is traced after the edit");
    assert_eq!((sch.labels[0].net.as_str(), sch.labels[0].at), ("SDA", p(mm(112.54), mm(58.73))));
    assert_ne!(sch.labels[0].id, "lbl_old", "ids are made anew");
    assert_eq!((sch.texts[0].at, sch.texts[0].angle), (p(mm(92.54), mm(38.73)), 90_000));
    assert_eq!((sch.no_connects[0].at, sch.no_connects[0].pin.as_str()), (p(mm(122.54), mm(48.73)), ""), "the pin it flagged is found again");
    assert_eq!(sch.junctions[0].at, p(mm(132.54), mm(48.73)));
    assert_eq!(sch.lines[0].pts[1], p(mm(7.54), mm(3.73)));
    assert!(sch.extras.locked.is_empty(), "locks are dropped on paste");
    assert_eq!(sch.user_fields.get("R1").and_then(|f| f.get("MPN")).map(String::as_str), Some("RC0603"));
    assert_eq!(Cmd::PasteSch { fragment: f, dx: Nm::ZERO, dy: Nm::ZERO, mode: PasteMode::Unique }.domain(), Domain::Schematic);
}

#[test]
fn a_symbol_lands_where_the_sheet_places_symbols_a_derived_sheet_by_the_corner_of_the_box() {
    let m = ConstraintModel::default();
    let f = fragment(|s| s.symbols.push(symbol("R1", "Device:R", "10k", p(mm(100.0), mm(50.0)))));
    // A sheet read from a KiCad file keeps KiCad's origin ...
    let mut kicad_sheet = empty_design();
    kicad_sheet.schematic = Some(SchematicSection { imported_from_kicad: true, ..Default::default() });
    let mut b = Board::new(kicad_sheet, &m, mm(0.1), mm(0.3));
    b.apply(&paste(&f, Nm::ZERO, Nm::ZERO, PasteMode::Unique)).unwrap();
    assert_eq!(b.design().schematic.as_ref().unwrap().symbols[0].at, p(mm(100.0), mm(50.0)));
    // ... a derived one places Device:R by the top-left corner of its box: the library origin is 1.27 mm right and 2.54 mm below it.
    let mut b = Board::new(empty_design(), &m, mm(0.1), mm(0.3));
    b.apply(&paste(&f, Nm::ZERO, Nm::ZERO, PasteMode::Unique)).unwrap();
    assert_eq!(b.design().schematic.as_ref().unwrap().symbols[0].at, p(mm(98.73), mm(47.46)));
}

#[test]
fn the_numbers_are_unique_across_every_sheet() {
    let m = ConstraintModel::default();
    let mut b = Board::new(empty_design(), &m, mm(0.1), mm(0.3));
    // R1 and R2 on the root, R3 and R4 on a sheet.
    b.apply(&Cmd::AddSheet { name: "Child".into(), file: "child.kicad_sch".into(), at: p(Nm::ZERO, Nm::ZERO), size: (mm(10.0), mm(10.0)) }).unwrap();
    let sheet_id = b.design().schematic.as_ref().unwrap().sheets[0].id.clone();
    for (id, x) in [("R1", 0), ("R2", 10_000_000)] {
        b.apply(&Cmd::AddSymbol { id: id.into(), lib_id: "Device:R".into(), at: p(Nm::from_raw_shim(x), mm(50.0)), rot_millideg: 0, value: "10k".into(), footprint: String::new(), unit: 1 }).unwrap();
    }
    for (id, x) in [("R3", 0), ("R4", 10_000_000)] {
        b.apply(&Cmd::OnSheet { sheet: sheet_id.clone(), cmd: Box::new(Cmd::AddSymbol { id: id.into(), lib_id: "Device:R".into(), at: p(Nm::from_raw_shim(x), mm(50.0)), rot_millideg: 0, value: "10k".into(), footprint: String::new(), unit: 1 }) }).unwrap();
    }
    // Pasting R1 and R3 onto the child sheet: both references exist (on one sheet or the other), so each takes the next free number from its own.
    let f = fragment(|s| {
        s.symbols.push(symbol("R1", "Device:R", "10k", p(Nm::ZERO, Nm::ZERO)));
        s.symbols.push(symbol("R3", "Device:R", "10k", p(mm(20.0), Nm::ZERO)));
    });
    b.apply(&Cmd::OnSheet { sheet: sheet_id.clone(), cmd: Box::new(paste(&f, Nm::ZERO, mm(100.0), PasteMode::Unique)) }).unwrap();
    let d = b.design();
    let child = d.sheet_contents.as_ref().unwrap().get("child.kicad_sch").unwrap();
    assert_eq!(refs(child), vec!["R3", "R4", "R5", "R6"], "R1 -> R5 (R2..R4 are taken), R3 -> R6");
    let root = d.schematic.as_ref().unwrap();
    assert_eq!(refs(root), vec!["R1", "R2"], "the root is untouched");
    // And once more onto the root: nothing is ever twice in the design.
    b.apply(&paste(&f, Nm::ZERO, mm(200.0), PasteMode::Unique)).unwrap();
    let mut all: Vec<String> = refs(b.design().schematic.as_ref().unwrap());
    all.extend(refs(b.design().sheet_contents.as_ref().unwrap().get("child.kicad_sch").unwrap()));
    let mut unique = all.clone();
    unique.sort();
    unique.dedup();
    assert_eq!(unique.len(), all.len(), "{all:?}");
    assert_eq!(all.len(), 8);
}

#[test]
fn paste_special_keeps_or_resets_the_numbers() {
    let m = ConstraintModel::default();
    let mut b = Board::new(empty_design(), &m, mm(0.1), mm(0.3));
    b.apply(&Cmd::AddSymbol { id: "R1".into(), lib_id: "Device:R".into(), at: p(Nm::ZERO, Nm::ZERO), rot_millideg: 0, value: "10k".into(), footprint: String::new(), unit: 1 }).unwrap();
    let f = fragment(|s| {
        s.symbols.push(symbol("R1", "Device:R", "10k", p(Nm::ZERO, Nm::ZERO)));
        s.symbols.push(symbol("R7", "Device:R", "10k", p(mm(10.0), Nm::ZERO)));
    });
    let mut keep = b.fork();
    keep.apply(&paste(&f, Nm::ZERO, mm(50.0), PasteMode::Keep)).unwrap();
    assert_eq!(refs(keep.design().schematic.as_ref().unwrap()), vec!["R1", "R2", "R7"], "R7 is kept; R1 is taken, so it is numbered anew");
    let mut reset = b.fork();
    reset.apply(&paste(&f, Nm::ZERO, mm(50.0), PasteMode::Remove)).unwrap();
    assert_eq!(refs(reset.design().schematic.as_ref().unwrap()), vec!["R1", "R2", "R3"], "every pasted symbol is numbered from the first free number");
}

#[test]
fn a_cut_and_paste_keeps_the_references_nothing_else_holds() {
    let m = ConstraintModel::default();
    let mut b = Board::new(empty_design(), &m, mm(0.1), mm(0.3));
    b.apply(&Cmd::AddSymbol { id: "R1".into(), lib_id: "Device:R".into(), at: p(Nm::ZERO, Nm::ZERO), rot_millideg: 0, value: "10k".into(), footprint: String::new(), unit: 1 }).unwrap();
    b.apply(&Cmd::DeleteSymbol { id: "R1".into(), unit: None }).unwrap();
    b.apply(&paste(&fragment(|s| s.symbols.push(symbol("R1", "Device:R", "10k", p(Nm::ZERO, Nm::ZERO)))), mm(5.0), Nm::ZERO, PasteMode::Unique)).unwrap();
    assert_eq!(refs(b.design().schematic.as_ref().unwrap()), vec!["R1"]);
}

#[test]
fn a_pasted_power_symbol_takes_the_next_free_power_reference() {
    let m = ConstraintModel::default();
    let mut b = Board::new(empty_design(), &m, mm(0.1), mm(0.3));
    b.apply(&Cmd::AddPowerSymbol { lib_id: "power:GND".into(), at: p(Nm::ZERO, Nm::ZERO), rot_millideg: 0, net: "GND".into(), pin: String::new() }).unwrap();
    let f = fragment(|s| {
        s.power_symbols.push(PowerSymbol { id: "#PWR01".into(), lib_id: "power:GND".into(), at: p(mm(10.0), Nm::ZERO), rot: 0, net: "GND".into(), pin: String::new() });
    });
    b.apply(&paste(&f, Nm::ZERO, Nm::ZERO, PasteMode::Unique)).unwrap();
    let ids: Vec<&str> = b.design().schematic.as_ref().unwrap().power_symbols.iter().map(|x| x.id.as_str()).collect();
    assert_eq!(ids, vec!["#PWR01", "#PWR02"]);
}

#[test]
fn library_symbols_the_design_lacks_are_published_a_generated_box_gets_a_name_of_its_own() {
    let m = ConstraintModel::default();
    let mut b = Board::new(empty_design(), &m, mm(0.1), mm(0.3));
    let mut custom = LibrarySymbol::from_engine_symbol(&eda_model::symbol::builtin("Device:R").unwrap());
    custom.lib_id = "MyLib:Widget".into();
    let mut boxed = custom.clone();
    boxed.lib_id = "eda:U1".into();
    let mut f = fragment(|s| {
        s.symbols.push(symbol("X1", "MyLib:Widget", "w", p(Nm::ZERO, Nm::ZERO)));
        s.symbols.push(symbol("U1", "eda:U1", "mcu", p(mm(30.0), Nm::ZERO)));
        s.symbols.push(symbol("R1", "Device:R", "10k", p(mm(60.0), Nm::ZERO)));
    });
    f.lib_symbols.extend([custom, boxed]);
    b.apply(&paste(&f, Nm::ZERO, Nm::ZERO, PasteMode::Unique)).unwrap();
    let lib = b.design().symbol_library.as_ref().unwrap();
    let ids: Vec<&str> = lib.symbols.iter().map(|s| s.lib_id.as_str()).collect();
    assert_eq!(ids, vec!["MyLib:Widget", "clipboard:U1"], "Device:R is a built-in the design already has");
    assert!(lib.symbols.iter().all(|s| s.published));
    let sch = b.design().schematic.as_ref().unwrap();
    let u1 = sch.symbols.iter().find(|s| s.id == "U1").unwrap();
    assert_eq!(u1.lib_id, "clipboard:U1", "the copy of a generated box draws from the symbol published for it");
    // pasting it again reuses the published symbol
    b.apply(&paste(&f, Nm::ZERO, mm(100.0), PasteMode::Unique)).unwrap();
    assert_eq!(b.design().symbol_library.as_ref().unwrap().symbols.len(), 2);
}

#[test]
fn an_empty_fragment_is_refused_and_nothing_changes() {
    let m = ConstraintModel::default();
    let mut b = Board::new(empty_design(), &m, mm(0.1), mm(0.3));
    let e = b.apply(&paste(&SchFragment::default(), Nm::ZERO, Nm::ZERO, PasteMode::Unique)).unwrap_err();
    assert_eq!(e[0].check, "ops_empty_paste");
}

#[test]
fn the_command_reads_from_and_writes_to_the_json_the_studio_sends() {
    let f = fragment(|s| {
        s.symbols.push(symbol("R1", "Device:R", "10k", p(mm(1.27), mm(2.54))));
        s.wires.push(wire(&[(Nm::ZERO, Nm::ZERO), (mm(1.27), Nm::ZERO)]));
    });
    let cmd = paste(&f, mm(2.54), Nm::ZERO, PasteMode::Remove);
    let text = serde_json::to_string(&cmd).unwrap();
    assert!(text.contains("\"op\":\"paste_sch\"") && text.contains("\"mode\":\"remove\""), "{text}");
    // The studio reads a command from a parsed body (`serde_json::from_value`, as `/api/cmd` does): with `serde_json`'s `arbitrary_precision` on (the `eda` build gets it
    // from starlark) a fractional number inside an internally tagged enum only reads from a `Value`, not straight from text -- the library symbols of the fragment have them.
    let value: serde_json::Value = serde_json::from_str(&text).unwrap();
    let back: Cmd = serde_json::from_value(value).unwrap();
    assert_eq!(back, cmd);
    // dx, dy and mode default
    let minimal = serde_json::json!({ "op": "paste_sch", "fragment": serde_json::to_value(&f).unwrap() });
    let back: Cmd = serde_json::from_value(minimal).unwrap();
    assert!(matches!(back, Cmd::PasteSch { dx: Nm::ZERO, dy: Nm::ZERO, mode: PasteMode::Unique, .. }));
}

/// A two-unit symbol the way a library has it: pins 1 and 2 on unit 1, pins 3 and 4 on unit 2.
fn dual() -> LibrarySymbol {
    use eda_model::ir::LibrarySymbolPin;
    let pin = |number: &str, unit: u32, y: f64| LibrarySymbolPin { id: String::new(), number: number.into(), name: String::new(), electrical_type: "passive".into(), shape: "line".into(), at: eda_model::symbol::SPoint { x: 0.0, y }, angle_deg: 270.0, length_mm: 2.54, unit, body_style: 1, hidden: false, name_size_mm: None, number_size_mm: None, alternates: Vec::new() };
    let mut s = LibrarySymbol { lib_id: "Test:Dual".into(), reference_prefix: "U".into(), unit_count: 2, pins: vec![pin("1", 1, 2.54), pin("2", 1, -2.54), pin("3", 2000, 2.54), pin("4", 2000, -2.54)], ..LibrarySymbol::default() };
    s.assign_missing_ids();
    s
}

#[test]
fn the_units_of_one_part_pasted_together_keep_one_number_and_a_unit_the_part_lacks_joins_it() {
    let m = ConstraintModel::default();
    let unit = |u: u32, x: Nm| SymbolInstance { unit: u, ..symbol("U1", "Test:Dual", "74HC00", p(x, Nm::ZERO)) };
    let mut f = fragment(|s| s.symbols.extend([unit(1, Nm::ZERO), unit(2000, mm(20.0))]));
    f.lib_symbols.push(dual());
    // Both units of U1 are placed already: a copy of both is a part of its own.
    let mut b = Board::new(empty_design(), &m, mm(0.1), mm(0.3));
    b.apply(&paste(&f, Nm::ZERO, Nm::ZERO, PasteMode::Unique)).unwrap();
    b.apply(&paste(&f, Nm::ZERO, mm(50.0), PasteMode::Unique)).unwrap();
    let sch = b.design().schematic.as_ref().unwrap();
    let placed: Vec<(String, u32)> = sch.symbols.iter().map(|s| (s.id.clone(), s.unit)).collect();
    assert_eq!(placed, vec![("U1".to_string(), 1), ("U1".to_string(), 2000), ("U2".to_string(), 1), ("U2".to_string(), 2000)]);
    // One unit alone, when the part has the other: it is the missing unit of that part, not a part.
    let mut one = fragment(|s| s.symbols.push(unit(2000, Nm::ZERO)));
    one.lib_symbols.push(dual());
    let mut b = Board::new(empty_design(), &m, mm(0.1), mm(0.3));
    let mut first = fragment(|s| s.symbols.push(unit(1, Nm::ZERO)));
    first.lib_symbols.push(dual());
    b.apply(&paste(&first, Nm::ZERO, Nm::ZERO, PasteMode::Unique)).unwrap();
    b.apply(&paste(&one, Nm::ZERO, mm(40.0), PasteMode::Unique)).unwrap();
    let placed: Vec<(String, u32)> = b.design().schematic.as_ref().unwrap().symbols.iter().map(|s| (s.id.clone(), s.unit)).collect();
    assert_eq!(placed, vec![("U1".to_string(), 1), ("U1".to_string(), 2000)]);
}

/// A power symbol whose definition draws its pin elsewhere than the symbol the design knows by that name (every `power:<net>` is known: the built-in rail)
/// comes in under a name of its own, `power:PWR_FLAG_2`; the names it tries are not all "known" in turn (the paste used to never end: KiCad's own
/// `erc_multiple_pin_to_pin.kicad_sch` hung Import Sheet).
#[test]
fn a_power_symbol_drawn_otherwise_than_the_one_the_design_knows_comes_in_under_a_name_of_its_own() {
    use eda_model::ir::LibrarySymbolPin;
    let m = ConstraintModel::default();
    let mut b = Board::new(empty_design(), &m, mm(0.1), mm(0.3));
    let pin = LibrarySymbolPin { id: String::new(), number: "1".into(), name: "pwr".into(), electrical_type: "power_out".into(), shape: "line".into(), at: eda_model::symbol::SPoint { x: 0.0, y: 7.62 }, angle_deg: 90.0, length_mm: 0.0, unit: 1, body_style: 1, hidden: false, name_size_mm: None, number_size_mm: None, alternates: Vec::new() };
    let mut flag = LibrarySymbol { lib_id: "power:PWR_FLAG".into(), reference_prefix: "#FLG".into(), unit_count: 1, pins: vec![pin], ..LibrarySymbol::default() };
    flag.assign_missing_ids();
    let mut f = fragment(|s| s.power_symbols.push(PowerSymbol { id: "#FLG01".into(), lib_id: "power:PWR_FLAG".into(), at: p(Nm::ZERO, Nm::ZERO), rot: 90_000, net: "PWR_FLAG".into(), pin: String::new() }));
    f.lib_symbols.push(flag);
    b.apply(&paste(&f, Nm::ZERO, Nm::ZERO, PasteMode::Keep)).unwrap();
    let sch = b.design().schematic.as_ref().unwrap();
    assert_eq!(sch.power_symbols[0].lib_id, "power:PWR_FLAG_2");
}

#[test]
fn two_generated_boxes_that_draw_the_same_share_one_library_symbol() {
    let m = ConstraintModel::default();
    let mut b = Board::new(empty_design(), &m, mm(0.1), mm(0.3));
    let box_of = |name: &str| LibrarySymbol { lib_id: name.into(), ..dual() };
    let mut f = fragment(|s| {
        s.symbols.push(symbol("U1", "eda:U1", "a", p(Nm::ZERO, Nm::ZERO)));
        s.symbols.push(symbol("U2", "eda:U2", "b", p(mm(30.0), Nm::ZERO)));
    });
    f.lib_symbols.extend([box_of("eda:U1"), box_of("eda:U2")]);
    b.apply(&paste(&f, Nm::ZERO, Nm::ZERO, PasteMode::Keep)).unwrap();
    let lib = b.design().symbol_library.as_ref().unwrap();
    assert_eq!(lib.symbols.len(), 1, "{:?}", lib.symbols.iter().map(|s| &s.lib_id).collect::<Vec<_>>());
    let sch = b.design().schematic.as_ref().unwrap();
    assert!(sch.symbols.iter().all(|s| s.lib_id == lib.symbols[0].lib_id));
}

#[test]
fn a_pasted_symbol_keeps_the_places_swap_pins_gave_its_pins_under_its_new_reference() {
    // R1 was copied with pins 1 and 2 traded; it is pasted beside an R1 that is there, so it is numbered anew (R2), and its swap goes with it
    let m = ConstraintModel::default();
    let mut b = Board::new(empty_design(), &m, mm(0.1), mm(0.3));
    let mut f = fragment(|s| s.symbols.push(symbol("R1", "Device:R", "10k", p(mm(100.0), mm(50.0)))));
    f.section.extras.swap_pin_places("R1", "1", "2");
    b.apply(&paste(&f, Nm::ZERO, Nm::ZERO, PasteMode::Unique)).unwrap();
    b.apply(&paste(&f, mm(20.0), Nm::ZERO, PasteMode::Unique)).unwrap();
    let sch = b.design().schematic.as_ref().unwrap();
    assert_eq!(refs(sch), vec!["R1", "R2"]);
    for key in ["R1", "R2"] {
        let swaps = sch.extras.pin_swaps_of(key).unwrap_or_else(|| panic!("{key} lost its swap: {:?}", sch.extras.pin_swaps));
        assert_eq!(swaps.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect::<Vec<_>>(), [("1", "2"), ("2", "1")], "{key}");
    }
    // a symbol with its library pins pastes with none
    let mut b = Board::new(empty_design(), &m, mm(0.1), mm(0.3));
    b.apply(&paste(&fragment(|s| s.symbols.push(symbol("R1", "Device:R", "10k", p(mm(100.0), mm(50.0))))), Nm::ZERO, Nm::ZERO, PasteMode::Unique)).unwrap();
    assert!(b.design().schematic.as_ref().unwrap().extras.pin_swaps.is_empty());
}
