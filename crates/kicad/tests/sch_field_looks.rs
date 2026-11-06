//! UI-LEFT S4: what Symbol Properties keeps of a field and of a power symbol, read from a `.kicad_sch` and written back as `SCH_IO_KICAD_SEXPR::saveSymbol` and
//! `EDA_TEXT::Format` write it -- a field's font face and colour (`(effects (font (face ..) .. (color ..)))`, `SchExtras::field_looks`), and a power symbol's
//! Footprint, Datasheet, flags and user fields (`SchExtras::power_props`).

use eda_kicad::{export_kicad_sch, import_kicad_sch, ExportMeta};

const META: ExportMeta<'static> = ExportMeta { date: "2026-11-06", title: "field looks" };

const FILE: &str = r##"(kicad_sch (version 20250114) (generator "eeschema") (generator_version "9.0")
	(uuid "11111111-1111-1111-1111-111111111111")
	(paper "A4")
	(lib_symbols
		(symbol "Device:R" (pin_numbers (hide yes)) (pin_names (offset 0)) (exclude_from_sim no) (in_bom yes) (on_board yes)
			(property "Reference" "R" (at 2.032 0 90) (effects (font (size 1.27 1.27))))
			(property "Value" "R" (at 0 0 90) (effects (font (size 1.27 1.27))))
			(property "Footprint" "" (at -1.778 0 90) (effects (font (size 1.27 1.27)) (hide yes)))
			(property "Datasheet" "~" (at 0 0 0) (effects (font (size 1.27 1.27)) (hide yes)))
			(symbol "R_0_1" (rectangle (start -1.016 -2.54) (end 1.016 2.54) (stroke (width 0.254) (type default)) (fill (type none))))
			(symbol "R_1_1"
				(pin passive line (at 0 3.81 270) (length 1.27) (name "~" (effects (font (size 1.27 1.27)))) (number "1" (effects (font (size 1.27 1.27)))))
				(pin passive line (at 0 -3.81 90) (length 1.27) (name "~" (effects (font (size 1.27 1.27)))) (number "2" (effects (font (size 1.27 1.27)))))
			)
		)
		(symbol "power:GND" (power) (pin_names (offset 0)) (exclude_from_sim no) (in_bom yes) (on_board yes)
			(property "Reference" "#PWR" (at 0 -6.35 0) (effects (font (size 1.27 1.27)) (hide yes)))
			(property "Value" "GND" (at 0 -3.81 0) (effects (font (size 1.27 1.27))))
			(property "Footprint" "" (at 0 0 0) (effects (font (size 1.27 1.27)) (hide yes)))
			(property "Datasheet" "" (at 0 0 0) (effects (font (size 1.27 1.27)) (hide yes)))
			(symbol "GND_0_1" (polyline (pts (xy 0 0) (xy 0 -1.27) (xy 1.27 -1.27) (xy 0 -2.54) (xy -1.27 -1.27) (xy 0 -1.27)) (stroke (width 0) (type default)) (fill (type none))))
			(symbol "GND_1_1" (pin power_in line (at 0 0 270) (length 0) (hide yes) (name "GND" (effects (font (size 1.27 1.27)))) (number "1" (effects (font (size 1.27 1.27))))))
		)
	)
	(symbol (lib_id "Device:R") (at 100 100 0) (unit 1) (exclude_from_sim no) (in_bom yes) (on_board yes) (dnp no)
		(uuid "21111111-1111-1111-1111-111111111111")
		(property "Reference" "R1" (at 102 99 0) (effects (font (size 1.27 1.27)) (justify left)))
		(property "Value" "10k" (at 102 101 0) (effects (font (face "DejaVu Sans") (size 1.27 1.27) (color 255 0 0 1)) (justify left)))
		(property "Footprint" "" (at 100 100 0) (effects (font (size 1.27 1.27)) (hide yes)))
		(property "Datasheet" "~" (at 100 100 0) (effects (font (size 1.27 1.27)) (hide yes)))
		(pin "1" (uuid "31111111-1111-1111-1111-111111111111"))
		(pin "2" (uuid "31111111-1111-1111-1111-111111111112"))
		(instances (project "x" (path "/11111111-1111-1111-1111-111111111111" (reference "R1") (unit 1))))
	)
	(symbol (lib_id "power:GND") (at 100 120 0) (unit 1) (exclude_from_sim yes) (in_bom no) (on_board yes) (dnp yes)
		(uuid "41111111-1111-1111-1111-111111111111")
		(property "Reference" "#PWR01" (at 100 126.35 0) (effects (font (size 1.27 1.27)) (hide yes)))
		(property "Value" "GND" (at 100 123.81 0) (effects (font (face "KiCad Font") (size 1.27 1.27))))
		(property "Footprint" "Fiducial:Mark" (at 100 120 0) (effects (font (size 1.27 1.27)) (hide yes)))
		(property "Datasheet" "https://example.com/gnd" (at 100 120 0) (effects (font (size 1.27 1.27)) (hide yes)))
		(property "MPN" "none" (at 100 120 0) (effects (font (size 1.27 1.27)) (hide yes)))
		(pin "1" (uuid "51111111-1111-1111-1111-111111111111"))
		(instances (project "x" (path "/11111111-1111-1111-1111-111111111111" (reference "#PWR01") (unit 1))))
	)
)
"##;

/// The item of `text` that starts with `head`, to its closing parenthesis at the indent it opened at.
fn item<'a>(text: &'a str, head: &str) -> &'a str {
    let start = text.find(head).unwrap_or_else(|| panic!("no {head} in\n{text}"));
    let rest = &text[start..];
    let end = rest.find("\n\t)").map_or(rest.len(), |e| e + 3);
    &rest[..end]
}

/// The `(property "name" ..)` of `item`, its effects included.
fn property<'a>(item: &'a str, name: &str) -> &'a str {
    let head = format!("(property \"{name}\"");
    let start = item.find(&head).unwrap_or_else(|| panic!("no {head} in\n{item}"));
    let rest = &item[start..];
    let end = rest.find("\n\t\t)").map_or(rest.len(), |e| e + 4);
    &rest[..end]
}

#[test]
fn a_fields_font_face_and_colour_are_read_and_written_back() {
    let (design, model, _) = import_kicad_sch(FILE).unwrap_or_else(|e| panic!("{e:?}"));
    let sch = design.schematic.as_ref().unwrap();
    let look = sch.extras.field_look("R1", "Value").expect("the Value's look is kept");
    assert_eq!(look.font.as_deref(), Some("DejaVu Sans"));
    assert_eq!(look.color.map(|c| (c.r, c.g, c.b, c.a)), Some((255, 0, 0, 255)));
    assert!(sch.extras.field_look("R1", "Reference").is_none(), "a field with the default font and colour keeps no entry");
    let out = export_kicad_sch(&design, &model, &META).unwrap();
    let value = property(item(&out, "(symbol (lib_id \"Device:R\")"), "Value");
    assert!(value.contains("(font (face \"DejaVu Sans\") (size 1.27 1.27) (color 255 0 0 1))"), "`EDA_TEXT::Format`'s order: face, size, colour\n{value}");
    // a power symbol's field too (`KICAD_FONT_NAME`)
    assert_eq!(sch.extras.field_look("#PWR01", "Value").and_then(|l| l.font.as_deref()), Some("KiCad Font"));
    assert!(property(item(&out, "(symbol (lib_id \"power:GND\")"), "Value").contains("(face \"KiCad Font\")"), "{out}");
    let (again, model2, _) = import_kicad_sch(&out).unwrap();
    assert_eq!(export_kicad_sch(&again, &model2, &META).unwrap(), out, "a second round trip writes the same file");
}

#[test]
fn a_power_symbols_footprint_datasheet_flags_and_user_fields_are_read_and_written_back() {
    let (design, model, _) = import_kicad_sch(FILE).unwrap_or_else(|e| panic!("{e:?}"));
    let props = design.schematic.as_ref().unwrap().extras.power_props.get("#PWR01").cloned().expect("the power symbol's properties are kept");
    assert_eq!((props.footprint.as_str(), props.datasheet.as_str()), ("Fiducial:Mark", "https://example.com/gnd"));
    assert_eq!((props.exclude_from_sim, props.exclude_from_bom, props.exclude_from_board, props.dnp, props.exclude_from_pos_files), (true, true, false, true, false));
    assert_eq!(props.fields.iter().map(|f| (f.name.as_str(), f.text.as_str(), f.visible)).collect::<Vec<_>>(), [("MPN", "none", false)]);
    let out = export_kicad_sch(&design, &model, &META).unwrap();
    let gnd = item(&out, "(symbol (lib_id \"power:GND\")");
    assert!(gnd.contains("(exclude_from_sim yes) (in_bom no) (on_board yes) (in_pos_files yes) (dnp yes)"), "{gnd}");
    assert!(property(gnd, "Footprint").contains("\"Fiducial:Mark\""), "{gnd}");
    assert!(property(gnd, "Datasheet").contains("\"https://example.com/gnd\""), "{gnd}");
    assert!(property(gnd, "MPN").contains("(hide yes)"), "{gnd}");
}
