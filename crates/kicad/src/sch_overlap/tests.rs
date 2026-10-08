use super::*;

/// A page with a resistor-like symbol (`Device:R`: names empty, numbers hidden, 1.27 mm pins, a 2.032 x 5.08 mm body) and a
/// four-pin box (`U`: names inside at the 1.016 mm offset, 2.54 mm pins).
const LIBS: &str = r#"
    (lib_symbols
      (symbol "Device:R" (pin_numbers (hide yes)) (pin_names (offset 0))
        (symbol "R_0_1" (rectangle (start -1.016 -2.54) (end 1.016 2.54) (stroke (width 0.254) (type default)) (fill (type none))))
        (symbol "R_1_1"
          (pin passive line (at 0 3.81 270) (length 1.27) (name "" (effects (font (size 1.27 1.27)))) (number "1" (effects (font (size 1.27 1.27)))))
          (pin passive line (at 0 -3.81 90) (length 1.27) (name "" (effects (font (size 1.27 1.27)))) (number "2" (effects (font (size 1.27 1.27)))))))
      (symbol "eda:U" (pin_names (offset 1.016))
        (symbol "U_0_1" (rectangle (start -7.62 5.08) (end 7.62 -5.08) (stroke (width 0.254) (type default)) (fill (type none))))
        (symbol "U_1_1"
          (pin bidirectional line (at -10.16 2.54 0) (length 2.54) (name "PA0" (effects (font (size 1.27 1.27)))) (number "1" (effects (font (size 1.27 1.27)))))
          (pin bidirectional line (at -10.16 0 0) (length 2.54) (name "PA1" (effects (font (size 1.27 1.27)))) (number "2" (effects (font (size 1.27 1.27)))))
          (pin bidirectional line (at 10.16 2.54 180) (length 2.54) (name "PA4" (effects (font (size 1.27 1.27)))) (number "3" (effects (font (size 1.27 1.27)))))
          (pin bidirectional line (at 10.16 0 180) (length 2.54) (name "PA5" (effects (font (size 1.27 1.27)))) (number "4" (effects (font (size 1.27 1.27))))))))"#;

fn page_with(libs: &str, body: &str) -> String {
    format!("(kicad_sch (version 20250114) (paper \"A4\") {libs} {body})")
}

fn page(body: &str) -> String {
    page_with(LIBS, body)
}

fn resistor(at: (f64, f64), fields: &str) -> String {
    format!(r#"(symbol (lib_id "Device:R") (at {} {} 0) (unit 1) {fields} (pin "1" (uuid "a")) (pin "2" (uuid "b")))"#, at.0, at.1)
}

fn field(name: &str, value: &str, at: (f64, f64), justify: &str) -> String {
    format!(r#"(property "{name}" "{value}" (at {} {} 0) (effects (font (size 1.27 1.27)) {justify}))"#, at.0, at.1)
}

fn report(body: &str) -> SheetReport {
    check_sheet("t.kicad_sch", &page(body)).expect("the page reads")
}

#[test]
fn a_resistor_with_its_fields_beside_it_is_clean() {
    let fields = format!("{} {}", field("Reference", "R1", (103.0, 98.0), "(justify left)"), field("Value", "330", (103.0, 102.0), "(justify left)"));
    let r = report(&resistor((100.0, 100.0), &fields));
    assert_eq!(r.count(), 0, "{}", r.render());
    assert!(r.items >= 5, "body, two pins, two fields: {}", r.items);
}

#[test]
fn two_fields_at_one_point_overlap() {
    let fields = format!("{} {}", field("Reference", "R1", (103.0, 100.0), "(justify left)"), field("Value", "330", (103.0, 100.0), "(justify left)"));
    let r = report(&resistor((100.0, 100.0), &fields));
    assert!(r.overlaps.iter().any(|o| o.kinds == (Kind::Field, Kind::Field)), "{}", r.render());
}

#[test]
fn a_field_over_the_symbols_own_body_is_found() {
    let fields = field("Reference", "R1", (99.0, 100.0), "(justify left)");
    let r = report(&resistor((100.0, 100.0), &fields));
    assert!(r.overlaps.iter().any(|o| o.kinds.0 == Kind::Body || o.kinds.1 == Kind::Body), "{}", r.render());
}

#[test]
fn a_field_at_the_page_corner_is_outside_the_frame() {
    let r = report(&resistor((100.0, 100.0), &field("Reference", "R1", (0.0, -2.0), "")));
    assert_eq!(r.outside.len(), 1, "{}", r.render());
}

#[test]
fn a_wire_ending_on_a_pin_tip_touches_it_and_nothing_overlaps() {
    // the resistor's top pin tip is at (100, 96.19); a wire goes up from it
    let wire = r#"(wire (pts (xy 100 96.19) (xy 100 90)) (stroke (width 0) (type default)) (uuid "w"))"#;
    let r = report(&format!("{} {wire}", resistor((100.0, 100.0), "")));
    assert_eq!(r.count(), 0, "{}", r.render());
    // one that runs along the pin and the body instead is an overlap
    let through = r#"(wire (pts (xy 100 96.19) (xy 100 104)) (stroke (width 0) (type default)) (uuid "w"))"#;
    let r = report(&format!("{} {through}", resistor((100.0, 100.0), "")));
    assert!(r.count() > 0, "{}", r.render());
}

fn u1(at: (f64, f64)) -> String {
    format!(
        r#"(symbol (lib_id "eda:U") (at {} {} 0) (unit 1) {} (pin "1" (uuid "a")) (pin "2" (uuid "b")) (pin "3" (uuid "c")) (pin "4" (uuid "d")))"#,
        at.0,
        at.1,
        field("Reference", "U1", (at.0, at.1 - 7.5), "")
    )
}

#[test]
fn a_pins_own_texts_inside_its_body_do_not_overlap_it_but_a_label_on_them_does() {
    let r = report(&u1((100.0, 100.0)));
    assert_eq!(r.count(), 0, "{}", r.render());
    // PA0's tip is at (89.84, 97.46) (the library's y is up); a label there reading right runs over its pin and its name
    let label = r#"(hierarchical_label "PA0" (shape bidirectional) (at 89.84 97.46 0) (effects (font (size 1.27 1.27)) (justify left)) (uuid "l"))"#;
    let r = report(&format!("{} {label}", u1((100.0, 100.0))));
    assert!(r.overlaps.iter().any(|o| o.a.contains("PA0") && o.b.contains("PA0")), "{}", r.render());
    // the same label reading left, the way KiCad turns a label on a left-hand pin, is clear
    let label = r#"(hierarchical_label "PA0" (shape bidirectional) (at 89.84 97.46 0) (effects (font (size 1.27 1.27)) (justify right)) (uuid "l"))"#;
    let r = report(&format!("{} {label}", u1((100.0, 100.0))));
    assert_eq!(r.count(), 0, "{}", r.render());
}

#[test]
fn pins_at_one_spot_and_pin_names_that_run_into_each_other_are_found() {
    // two pins 1.27 mm apart: their name texts (1.75 mm tall boxes) collide
    let tight = LIBS.replace("(at -10.16 0 0)", "(at -10.16 1.27 0)");
    let r = check_sheet("t", &page_with(&tight, &u1((100.0, 100.0)))).unwrap();
    assert!(r.overlaps.iter().any(|o| o.kinds == (Kind::PinName, Kind::PinName)), "{}", r.render());
    // the same pin at the same spot as another
    let same = LIBS.replace("(at -10.16 0 0)", "(at -10.16 2.54 0)");
    let r = check_sheet("t", &page_with(&same, &u1((100.0, 100.0)))).unwrap();
    assert!(r.overlaps.iter().any(|o| o.kinds.0 == Kind::Pin && o.kinds.1 == Kind::Pin), "{}", r.render());
}

#[test]
fn a_rotated_symbol_turns_its_pins_and_its_body() {
    // a resistor turned a quarter lies on its side: its pins are 3.81 mm either side of it along x
    let rot = r#"(symbol (lib_id "Device:R") (at 100 100 90) (unit 1) (pin "1" (uuid "a")) (pin "2" (uuid "b")))"#;
    let (items, _) = items_of_sheet(&page(rot)).unwrap();
    let pins: Vec<&Item> = items.iter().filter(|i| i.kind == Kind::Pin).collect();
    assert_eq!(pins.len(), 2);
    for p in pins {
        let (a, _) = p.seg.unwrap();
        assert!((a.1 - 100_000.0).abs() < 1.0 && ((a.0 - 100_000.0).abs() - 3_810.0).abs() < 1.0, "{a:?}");
    }
    let body = items.iter().find(|i| i.kind == Kind::Body).unwrap();
    assert!(body.rect.w() > body.rect.h(), "{:?}", body.rect);
}

#[test]
fn segments_cross_or_run_along_one_another() {
    assert!(segs_conflict(((0.0, 0.0), (10_000.0, 0.0)), ((5_000.0, -1_000.0), (5_000.0, 1_000.0))).is_some());
    assert!(segs_conflict(((0.0, 0.0), (10_000.0, 0.0)), ((5_000.0, 0.0), (15_000.0, 0.0))).is_some());
    // end to end
    assert!(segs_conflict(((0.0, 0.0), (10_000.0, 0.0)), ((10_000.0, 0.0), (10_000.0, 5_000.0))).is_none());
    assert!(segs_conflict(((0.0, 0.0), (10_000.0, 0.0)), ((10_000.0, 0.0), (20_000.0, 0.0))).is_none());
    // a T onto the inside of a line
    assert!(segs_conflict(((0.0, 0.0), (10_000.0, 0.0)), ((5_000.0, 0.0), (5_000.0, 5_000.0))).is_some());
}
