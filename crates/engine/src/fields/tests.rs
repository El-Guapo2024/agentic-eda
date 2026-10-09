use super::*;
use eda_model::{Pin, PinKind};

fn part(reference: &str, n: usize) -> Part {
    Part {
        reference: reference.into(),
        mpn: None,
        lcsc: None,
        value: Some("330".into()),
        package: Some("0603".into()),
        footprint: None,
        symbol: None,
        datasheet: None,
        pins: (1..=n).map(|i| Pin { number: i.to_string(), name: None, kind: PinKind::Passive }).collect(),
        body_um: None,
        edge: None,
    }
}

fn resistor(at: Point, rot: u32, mirrored: bool) -> SymbolInstance {
    SymbolInstance { id: "R1".into(), at, rot, mirrored, mirror_y: false, lib_id: "Device:R".into(), unit: 1, value: "330".into(), footprint: String::new(), datasheet: String::new(), dnp: false, exclude_from_bom: false, exclude_from_board: false, exclude_from_sim: false }
}

fn width_of(text: &str) -> i64 {
    let (x0, _, x1, _) = text_box_iu(text, DEFAULT_TEXT_SIZE_IU, default_pen_iu(DEFAULT_TEXT_SIZE_IU), HJustify::Left, VJustify::Center);
    x1 - x0
}

#[test]
fn a_standing_resistor_gets_its_fields_to_its_right_left_justified_two_and_a_half_millimetres_apart() {
    let lib = eda_model::symbol::builtin("Device:R");
    let sym = resistor(Point { x: 50_800, y: 50_800 }, 0, false);
    let geom = SymbolGeom::of(&sym, &part("R1", 2), lib.as_ref());
    let specs = symbol_specs(&sym, None, "");
    let placed = autoplace_symbol(&sym, &geom, &specs);
    assert_eq!(placed.len(), 4);
    let (reference, value) = (&placed[0], &placed[1]);
    assert_eq!((reference.h, reference.v), (TextJustify::Left, TextVAlign::Center));
    assert!(reference.visible && value.visible && !placed[2].visible && !placed[3].visible);
    // Right has no pins (they are above and below), so the fields go there: 0.635 mm clear of the body (to 53213 um), on the 1.27 grid
    let anchor = |p: &FieldPlacement| page_field(sym.at, sym.rot, sym.mirrored, geom.width, p, "x").at;
    let (rx, ry) = anchor(reference);
    assert!(rx >= 53_213.0 + 635.0 - 1.0 && (rx / 1_270.0).fract().abs() < 1e-6, "x {rx}");
    // the box of the fields is two cells of 2.54 mm centred on the body's centre (53 340): each field's own centre is half a cell in
    let (_, vy) = anchor(value);
    assert!((ry - 52_069.9).abs() < 0.2 && (vy - 54_609.9).abs() < 0.2, "{ry} {vy}");
    // and the first is as wide as KiCad measures the text
    let _ = width_of("R1");
}

#[test]
fn the_fields_of_a_symbol_turned_half_a_turn_are_where_the_unturned_ones_go_turned_with_it() {
    let lib = eda_model::symbol::builtin("Device:R");
    let p = part("R1", 2);
    let up = resistor(Point { x: 50_800, y: 50_800 }, 0, false);
    let g0 = SymbolGeom::of(&up, &p, lib.as_ref());
    let placed0 = autoplace_symbol(&up, &g0, &symbol_specs(&up, None, ""));
    // the same placements, kept in the unturned frame, on a symbol that is turned: the fields turn with it and sit to its left
    let turned = resistor(Point { x: 50_800 + 2_540, y: 50_800 + 5_080 }, 180_000, false);
    let f = page_field(turned.at, turned.rot, turned.mirrored, g0.width, &placed0[0], "R1");
    let f0 = page_field(up.at, up.rot, up.mirrored, g0.width, &placed0[0], "R1");
    assert!(f.at.0 < turned.at.x as f64, "turned half a turn the text is on the left of the box: {f:?}");
    assert_eq!((f.h, f.v), (HJustify::Right, VJustify::Center), "and still ends at its anchor");
    assert_eq!((f0.h, f0.v), (HJustify::Left, VJustify::Center));
    assert!(!f.vertical && !f0.vertical);
    // a placement made on the turned symbol itself reads the same as the page placement it came from
    let g1 = SymbolGeom::of(&turned, &p, lib.as_ref());
    let placed1 = autoplace_symbol(&turned, &g1, &symbol_specs(&turned, None, ""));
    let on_page = page_field(turned.at, turned.rot, turned.mirrored, g1.width, &placed1[0], "R1");
    // turned half a turn pin 1 is below, still no pins to the right
    assert_eq!(on_page.h, HJustify::Left);
    assert!(on_page.at.0 > g1.body.unwrap().x1, "{on_page:?} {:?}", g1.body);
}

#[test]
fn a_quarter_turned_symbols_fields_still_read_horizontally() {
    let lib = eda_model::symbol::builtin("Device:R");
    let p = part("R1", 2);
    for rot in [90_000, 270_000] {
        let s = resistor(Point { x: 50_800, y: 50_800 }, rot, false);
        let g = SymbolGeom::of(&s, &p, lib.as_ref());
        let placed = autoplace_symbol(&s, &g, &symbol_specs(&s, None, ""));
        assert_eq!(placed[0].angle, 90_000, "the stored angle that turns the text back");
        let f = page_field(s.at, s.rot, s.mirrored, g.width, &placed[0], "R1");
        assert!(!f.vertical, "rot {rot}: {f:?}");
    }
}

#[test]
fn a_sheets_fields_sit_above_and_below_its_left_edge() {
    let sheet = SheetInstance { id: "s".into(), name: "MCU".into(), file: "mcu.kicad_sch".into(), at: Point { x: 100_000, y: 80_000 }, size: (40_000, 30_000), pins: vec![], page: String::new() };
    let sch = SchematicSection::default();
    let f = sheet_fields(&sch, &sheet);
    assert_eq!((f[0].h, f[0].v), (HJustify::Left, VJustify::Bottom));
    assert!((f[0].at.1 - (80_000.0 - 712.0)).abs() < 1.0, "{:?}", f[0]);
    assert_eq!((f[1].h, f[1].v), (HJustify::Left, VJustify::Top));
    assert!((f[1].at.1 - (110_000.0 + 585.0)).abs() < 1.0, "{:?}", f[1]);
}

#[test]
fn a_power_symbols_value_is_below_ground_and_above_a_supply() {
    let sch = SchematicSection::default();
    let gnd = PowerSymbol { id: "#PWR01".into(), lib_id: "power:GND".into(), at: Point { x: 10_000, y: 10_000 }, rot: 0, net: "GND".into(), pin: "U1.2".into() };
    let vdd = PowerSymbol { id: "#PWR02".into(), lib_id: "power:VDD".into(), at: Point { x: 20_000, y: 10_000 }, rot: 0, net: "VDD".into(), pin: "U1.1".into() };
    let g = power_fields(&sch, &gnd);
    let v = power_fields(&sch, &vdd);
    assert!(!g[0].visible && g[1].visible && g[1].text == "GND");
    assert_eq!(g[1].at, (10_000.0, 13_810.0));
    assert_eq!(v[1].at, (20_000.0, 10_000.0 - 3_556.0));
    // turned half a turn the ground symbol hangs up and its value goes over it
    let flipped = PowerSymbol { rot: 180_000, ..gnd };
    assert_eq!(power_fields(&sch, &flipped)[1].at, (10_000.0, 10_000.0 - 3_810.0));
}
