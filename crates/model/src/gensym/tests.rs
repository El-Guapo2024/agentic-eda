use super::*;
use crate::{Pin, PinKind};

fn pin(number: &str, name: &str, kind: PinKind) -> Pin {
    Pin { number: number.into(), name: Some(name.into()), kind }
}

fn mcu() -> Part {
    let mut pins = vec![pin("1", "VDD", PinKind::Power), pin("2", "GND", PinKind::Ground)];
    for i in 0..8 {
        pins.push(pin(&(3 + i).to_string(), &format!("PA{i}"), PinKind::Signal));
    }
    pins.push(pin("11", "RESET", PinKind::Signal));
    pins.push(pin("12", "NC1", PinKind::Nc));
    pins.push(pin("13", "NC2", PinKind::Nc));
    pins.push(pin("14", "VDD2", PinKind::Power));
    pins.push(pin("15", "GND2", PinKind::Ground));
    pins.push(pin("16", "EN", PinKind::Signal));
    Part { reference: "U1".into(), mpn: Some("MCU16".into()), lcsc: None, value: Some("MCU".into()), package: None, footprint: None, symbol: None, datasheet: None, pins, body_um: None, edge: None }
}

fn at_grid(v: f64) -> bool {
    ((v / 1.27).round() * 1.27 - v).abs() < 1e-9
}

#[test]
fn the_sides_are_the_box_layouts_power_up_ground_down_inputs_left_outputs_right() {
    let s = assign_sides(&mcu());
    let names = |side: &[usize]| side.iter().map(|&i| mcu().pins[i].name.clone().unwrap()).collect::<Vec<_>>();
    assert_eq!(names(&s.top), ["VDD", "VDD2"]);
    assert_eq!(names(&s.bottom), ["GND", "GND2", "NC1", "NC2"], "no-connect pins come last on the bottom");
    assert_eq!(names(&s.left), ["RESET", "EN", "PA0", "PA1", "PA2", "PA3"]);
    assert_eq!(names(&s.right), ["PA4", "PA5", "PA6", "PA7"]);
}

#[test]
fn a_generated_symbol_has_2_54_mm_pins_2_54_mm_apart_on_the_1_27_mm_grid_and_no_two_at_one_spot() {
    let sym = generate(&mcu(), "gen:U1");
    assert_eq!(sym.pins.len(), 16);
    let mut spots = std::collections::BTreeSet::new();
    for p in &sym.pins {
        assert_eq!(p.length_mm, 2.54);
        assert!(at_grid(p.at.x) && at_grid(p.at.y), "{} at ({}, {}) is off the grid", p.number, p.at.x, p.at.y);
        assert!(spots.insert(((p.at.x * 1000.0).round() as i64, (p.at.y * 1000.0).round() as i64)), "two pins at ({}, {})", p.at.x, p.at.y);
    }
    // pins on one side are a pitch apart
    for angle in [0.0, 180.0] {
        let mut ys: Vec<f64> = sym.pins.iter().filter(|p| p.angle_deg == angle).map(|p| p.at.y).collect();
        ys.sort_by(|a, b| b.partial_cmp(a).unwrap());
        for w in ys.windows(2) {
            assert!((w[0] - w[1] - 2.54).abs() < 1e-9, "{ys:?}");
        }
    }
    // power pins are on the top and ground pins on the bottom, at distinct places
    let by_name = |n: &str| sym.pins.iter().find(|p| p.name == n).unwrap();
    assert!(by_name("VDD").at.y > 0.0 && by_name("GND").at.y < 0.0);
    assert_ne!(by_name("VDD").at.x, by_name("VDD2").at.x);
    assert_ne!(by_name("GND").at.x, by_name("GND2").at.x);
    assert_eq!(by_name("VDD").angle_deg, 270.0, "a top pin runs down into the body");
    assert_eq!(by_name("GND").angle_deg, 90.0);
}

#[test]
fn the_body_is_wider_than_the_names_that_face_each_other_and_taller_than_the_names_that_run_down_it() {
    let sym = generate(&mcu(), "gen:U1");
    let SymbolGraphic::Rectangle { start, end, .. } = &sym.graphics[0] else { panic!("a body") };
    let (w, h) = (end.x - start.x, start.y - end.y);
    // RESET on the left and PA4 on the right, an offset in from each edge, a gap between
    assert!(w >= 2.0 * 1.016 + text_mm("RESET") + text_mm("PA4") + 1.27 - 1e-9, "{w}");
    // the names of the top pins read down into the body from its top edge
    assert!(h >= 1.016 + text_mm("VDD2") + 1.27, "{h}");
    assert!((w / 2.54).fract().abs() < 1e-9 && (h / 2.54).fract().abs() < 1e-9, "sizes are whole pitches: {w} x {h}");
    assert!(sym.pin_numbers_hidden == false && sym.pin_name_offset_mm == 1.016);
}

#[test]
fn a_two_pin_part_gets_a_small_box_with_a_pin_on_each_side() {
    let p = Part { reference: "X1".into(), pins: vec![pin("1", "A", PinKind::Passive), pin("2", "B", PinKind::Passive)], ..mcu() };
    let sym = generate(&p, "gen:X1");
    assert_eq!(sym.pins.len(), 2);
    assert!(sym.pins[0].at.x < 0.0 && sym.pins[1].at.x > 0.0);
    assert_eq!(sym.pins[0].at.y, sym.pins[1].at.y);
}
