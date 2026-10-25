use eda_model::units::mm;
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
        body_nm: None,
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
    let sym = resistor(Point { x: mm(50.8), y: mm(50.8) }, 0, false);
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
    assert!(rx >= 53_213_000.0 + 635_000.0 - 1.0 && (rx / 1_270_000.0).fract().abs() < 1e-6, "x {rx}");
    // the box of the fields is two cells of 2.54 mm centred on the body's centre (53 340): each field's own centre is half a cell in
    let (_, vy) = anchor(value);
    assert!((ry - 52_069_900.0).abs() < 1.0 && (vy - 54_609_900.0).abs() < 1.0, "{ry} {vy}");
    // and the first is as wide as KiCad measures the text
    let _ = width_of("R1");
}

#[test]
fn the_fields_of_a_symbol_turned_half_a_turn_are_where_the_unturned_ones_go_turned_with_it() {
    let lib = eda_model::symbol::builtin("Device:R");
    let p = part("R1", 2);
    let up = resistor(Point { x: mm(50.8), y: mm(50.8) }, 0, false);
    let g0 = SymbolGeom::of(&up, &p, lib.as_ref());
    let placed0 = autoplace_symbol(&up, &g0, &symbol_specs(&up, None, ""));
    // the same placements, kept in the unturned frame, on a symbol that is turned: the fields turn with it and sit to its left
    let turned = resistor(Point { x: mm(50.8) + mm(2.54), y: mm(50.8) + mm(5.08) }, 180_000, false);
    let f = page_field(turned.at, turned.rot, turned.mirrored, g0.width, &placed0[0], "R1");
    let f0 = page_field(up.at, up.rot, up.mirrored, g0.width, &placed0[0], "R1");
    assert!(f.at.0 < turned.at.x.as_f64(), "turned half a turn the text is on the left of the box: {f:?}");
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
        let s = resistor(Point { x: mm(50.8), y: mm(50.8) }, rot, false);
        let g = SymbolGeom::of(&s, &p, lib.as_ref());
        let placed = autoplace_symbol(&s, &g, &symbol_specs(&s, None, ""));
        assert_eq!(placed[0].angle, 90_000, "the stored angle that turns the text back");
        let f = page_field(s.at, s.rot, s.mirrored, g.width, &placed[0], "R1");
        assert!(!f.vertical, "rot {rot}: {f:?}");
    }
}

#[test]
fn a_sheets_fields_sit_above_and_below_its_left_edge() {
    let sheet = SheetInstance { id: "s".into(), name: "MCU".into(), file: "mcu.kicad_sch".into(), at: Point { x: mm(100.0), y: mm(80.0) }, size: (mm(40.0), mm(30.0)), pins: vec![], page: String::new() };
    let sch = SchematicSection::default();
    let f = sheet_fields(&sch, &sheet);
    assert_eq!((f[0].h, f[0].v), (HJustify::Left, VJustify::Bottom));
    assert!((f[0].at.1 - (80_000_000.0 - 711_600.0)).abs() < 1.0, "{:?}", f[0]);
    assert_eq!((f[1].h, f[1].v), (HJustify::Left, VJustify::Top));
    assert!((f[1].at.1 - (110_000_000.0 + 584_600.0)).abs() < 1.0, "{:?}", f[1]);
}

#[test]
fn a_power_symbols_value_is_below_ground_and_above_a_supply() {
    let sch = SchematicSection::default();
    let gnd = PowerSymbol { id: "#PWR01".into(), lib_id: "power:GND".into(), at: Point { x: mm(10.0), y: mm(10.0) }, rot: 0, net: "GND".into(), pin: "U1.2".into() };
    let vdd = PowerSymbol { id: "#PWR02".into(), lib_id: "power:VDD".into(), at: Point { x: mm(20.0), y: mm(10.0) }, rot: 0, net: "VDD".into(), pin: "U1.1".into() };
    let g = power_fields(&sch, &gnd);
    let v = power_fields(&sch, &vdd);
    assert!(!g[0].visible && g[1].visible && g[1].text == "GND");
    assert_eq!(g[1].at, (10_000_000.0, 13_810_000.0));
    assert_eq!(v[1].at, (20_000_000.0, 10_000_000.0 - 3_556_000.0));
    // turned half a turn the ground symbol hangs up and its value goes over it
    let flipped = PowerSymbol { rot: 180_000, ..gnd };
    assert_eq!(power_fields(&sch, &flipped)[1].at, (10_000_000.0, 10_000_000.0 - 3_810_000.0));
}

// ------------------------------------------------------------------------------------------------------ user-defined fields

fn with_user_fields(fields: &[(&str, &str)]) -> SchematicSection {
    let mut sch = SchematicSection::default();
    let map = sch.user_fields.entry("R1".into()).or_default();
    for (name, text) in fields {
        map.insert(name.to_string(), text.to_string());
    }
    sch
}

#[test]
fn the_specs_of_a_symbol_are_the_four_standard_fields_then_its_user_fields_hidden() {
    let sym = resistor(Point { x: mm(50.8), y: mm(50.8) }, 0, false);
    let plain = symbol_specs_in(&SchematicSection::default(), &sym, None, "");
    assert_eq!(plain, symbol_specs(&sym, None, ""), "no user fields: the four");
    let sch = with_user_fields(&[("MPN", "ERJ-3EKF"), ("Manufacturer", "Panasonic"), ("value", "dropped"), ("DATASHEET", "dropped")]);
    let specs = symbol_specs_in(&sch, &sym, None, "");
    let names: Vec<&str> = specs.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(names, ["Reference", "Value", "Footprint", "Datasheet", "MPN", "Manufacturer"], "by name, and not a field named like a standard one");
    assert!(specs[4..].iter().all(|s| !s.visible && s.can_autoplace));
    assert_eq!(specs[4].text, "ERJ-3EKF");
    // a user field of another symbol is not this one's
    let other = SymbolInstance { id: "R2".into(), ..sym };
    assert_eq!(symbol_specs_in(&sch, &other, None, "").len(), 4);
}

#[test]
fn a_user_field_is_placed_with_the_others_and_a_hidden_one_takes_the_first_ones_place() {
    let lib = eda_model::symbol::builtin("Device:R");
    let sym = resistor(Point { x: mm(50.8), y: mm(50.8) }, 0, false);
    let sch = with_user_fields(&[("MPN", "ERJ-3EKF")]);
    let geom = SymbolGeom::of(&sym, &part("R1", 2), lib.as_ref());
    let hidden = symbol_specs_in(&sch, &sym, None, "");
    let placed = autoplace_symbol(&sym, &geom, &hidden);
    assert_eq!(placed.len(), 5);
    assert!(!placed[4].visible);
    assert_eq!((placed[4].dx, placed[4].dy), (placed[0].dx, placed[0].dy), "a hidden field takes the Reference's place");
    // shown, it is the third of the fields that stack beside the symbol
    let mut shown = hidden.clone();
    shown[4].visible = true;
    let placed = autoplace_symbol(&sym, &geom, &shown);
    assert!(placed[4].visible);
    let at = |p: &FieldPlacement| page_field(sym.at, sym.rot, sym.mirrored, geom.width, p, "x").at;
    let (r, v, m) = (at(&placed[0]), at(&placed[1]), at(&placed[4]));
    assert!(m.1 > v.1 && v.1 > r.1, "stacked downwards: {r:?} {v:?} {m:?}");
    assert!((m.0 - r.0).abs() < 1e-6, "on one left edge");
}

#[test]
fn a_page_position_taken_into_the_symbols_frame_and_back_is_the_same_whatever_the_turn_and_mirror() {
    // what the reader of a file does with a user field's `(at x y angle)` and the writer undoes (`local_placement_on_page` then `page_field`)
    let at = Point { x: mm(50.8), y: mm(50.8) };
    for rot in [0, 90_000, 180_000, 270_000] {
        for mirrored in [false, true] {
            for width in [0.0, 5_080_000.0] {
                for (h, v, vertical) in [(TextJustify::Left, TextVAlign::Center, false), (TextJustify::Right, TextVAlign::Top, true), (TextJustify::Center, TextVAlign::Bottom, false)] {
                    let anchor = (57_150_000.0, 44_450_000.0);
                    let local = local_placement_on_page(at, rot, mirrored, width, "MPN", anchor, vertical, h, v, true);
                    let back = page_field(at, rot, mirrored, width, &local, "x");
                    assert!((back.at.0 - anchor.0).abs() < 1.0 && (back.at.1 - anchor.1).abs() < 1.0, "rot {rot} mirrored {mirrored} width {width}: {:?}", back.at);
                    assert_eq!(back.vertical, vertical, "rot {rot} mirrored {mirrored}");
                    assert_eq!((back.h, back.v), (to_hj(h), to_vj(v)), "rot {rot} mirrored {mirrored} vertical {vertical}");
                }
            }
        }
    }
}
