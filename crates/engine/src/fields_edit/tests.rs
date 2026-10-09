use super::*;
use eda_model::{Part, Pin, PinKind};

fn part(reference: &str) -> Part {
    Part {
        reference: reference.into(),
        mpn: None,
        lcsc: None,
        value: Some("330".into()),
        package: Some("0603".into()),
        footprint: None,
        symbol: None,
        datasheet: None,
        pins: (1..=2).map(|i| Pin { number: i.to_string(), name: None, kind: PinKind::Passive }).collect(),
        body_um: None,
        edge: None,
    }
}

fn model() -> ConstraintModel {
    ConstraintModel { parts: vec![part("R1")], ..Default::default() }
}

fn resistor(at: Point, rot: u32, mirrored: bool) -> SymbolInstance {
    SymbolInstance { id: "R1".into(), at, rot, mirrored, mirror_y: false, lib_id: "Device:R".into(), unit: 1, value: "330".into(), footprint: String::new(), datasheet: String::new(), dnp: false, exclude_from_bom: false, exclude_from_board: false, exclude_from_sim: false }
}

fn sheet_of(sym: SymbolInstance) -> SchematicSection {
    SchematicSection { symbols: vec![sym], ..Default::default() }
}

#[test]
fn a_field_id_names_its_owner_and_its_field() {
    assert_eq!(field_id("R1", "Value"), "fld:R1:Value");
    assert_eq!(parse_field_id("fld:R1#2:Reference"), Some(("R1#2", "Reference")));
    assert_eq!(parse_field_id("R1"), None);
    assert_eq!(parse_field_id("fld:nocolon"), None);
}

#[test]
fn a_page_position_is_stored_so_that_it_reads_back_at_the_same_place_for_every_turn_and_mirror() {
    let m = model();
    for rot in [0, 90_000, 180_000, 270_000] {
        for mirrored in [false, true] {
            let sch = sheet_of(resistor(Point { x: 50_800, y: 50_800 }, rot, mirrored));
            let ctx = OwnerCtx::new(&sch, &m, "R1").expect("the symbol");
            let old = ctx.current(&sch)[1].clone();
            for vertical in [false, true] {
                for h in [HJustify::Left, HJustify::Center, HJustify::Right] {
                    for v in [VJustify::Top, VJustify::Center, VJustify::Bottom] {
                        let stored = ctx.placed_at(&old, (70_000.0, 40_000.0), vertical, h, v);
                        let back = ctx.page(&stored, "330");
                        assert_eq!(back.at, (70_000.0, 40_000.0), "rot {rot} mirrored {mirrored}");
                        assert_eq!((back.vertical, back.h, back.v), (vertical, h, v), "rot {rot} mirrored {mirrored}");
                    }
                }
            }
        }
    }
}

#[test]
fn a_page_offset_moves_the_stored_position_by_the_offset_taken_back_through_the_symbols_transform() {
    let m = model();
    for rot in [0, 90_000, 180_000, 270_000] {
        for mirrored in [false, true] {
            let sch = sheet_of(resistor(Point { x: 50_800, y: 50_800 }, rot, mirrored));
            let ctx = OwnerCtx::new(&sch, &m, "R1").expect("the symbol");
            let old = ctx.current(&sch)[0].clone();
            let before = ctx.page(&old, "R1").at;
            let (dx, dy) = ctx.local_delta(2_540, -1_270);
            let moved = FieldPlacement { dx: old.dx + dx, dy: old.dy + dy, ..old.clone() };
            let after = ctx.page(&moved, "R1").at;
            assert!((after.0 - before.0 - 2_540.0).abs() < 1.0 && (after.1 - before.1 + 1_270.0).abs() < 1.0, "rot {rot} mirrored {mirrored}: {before:?} -> {after:?}");
        }
    }
}

#[test]
fn power_symbols_and_sheets_have_fields_of_their_own() {
    let mut sch = SchematicSection::default();
    sch.power_symbols.push(PowerSymbol { id: "#PWR01".into(), lib_id: "power:GND".into(), at: Point { x: 10_000, y: 10_000 }, rot: 0, net: "GND".into(), pin: "U1.2".into() });
    sch.sheets.push(SheetInstance { id: "s1".into(), name: "MCU".into(), file: "mcu.kicad_sch".into(), at: Point { x: 100_000, y: 80_000 }, size: (40_000, 30_000), pins: vec![], page: String::new() });
    let m = model();
    let power = OwnerCtx::new(&sch, &m, "#PWR01").expect("the power symbol");
    assert_eq!(power.kind, OwnerKind::Power);
    let f = power.page_fields(&sch);
    assert_eq!(f.iter().map(|f| f.name.as_str()).collect::<Vec<_>>(), ["Reference", "Value"]);
    assert!(!f[0].visible && f[1].visible);
    let sheet = OwnerCtx::new(&sch, &m, "s1").expect("the sheet");
    assert_eq!(sheet.kind, OwnerKind::Sheet);
    assert_eq!(sheet.page_fields(&sch)[0].text, "MCU");
    assert!(OwnerCtx::new(&sch, &m, "nothing").is_none());
}

#[test]
fn a_schematic_read_from_a_kicad_file_has_no_fields_to_edit() {
    let mut sch = sheet_of(resistor(Point { x: 50_800, y: 50_800 }, 0, false));
    sch.imported_from_kicad = true;
    assert!(OwnerCtx::new(&sch, &model(), "R1").is_none());
}

#[test]
fn the_manual_autoplace_keeps_the_fields_off_a_wire_that_runs_where_the_automatic_one_puts_them() {
    let m = model();
    let mut sch = sheet_of(resistor(Point { x: 50_800, y: 50_800 }, 0, false));
    let ctx = OwnerCtx::new(&sch, &m, "R1").expect("the symbol");
    let auto = ctx.autoplace(&sch, &m, AutoplaceAlgo::Manual);
    // nothing around: the same place as the automatic mode
    assert_eq!(auto, ctx.autoplace(&sch, &m, AutoplaceAlgo::Auto));
    let on_right = ctx.page(&auto[0], "R1").at;
    assert!(on_right.0 > 53_000.0, "{on_right:?}");
    // a wire across the right side
    sch.wires.push(eda_model::ir::Wire { id: "w".into(), net: "N".into(), pins: vec![], pts: vec![Point { x: 56_000, y: 40_000 }, Point { x: 56_000, y: 70_000 }], bus: false });
    let ctx = OwnerCtx::new(&sch, &m, "R1").expect("the symbol");
    let moved = ctx.autoplace(&sch, &m, AutoplaceAlgo::Manual);
    let at = ctx.page(&moved[0], "R1").at;
    assert!(at.0 < 50_800.0, "the fields leave the right side the wire crosses: {at:?}");
}
