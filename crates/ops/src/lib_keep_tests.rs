//! P17 review: what Symbol Properties and the pad tools do with what a library item's file says beyond the editors' own members (`eda_model::lib_keep`), and the
//! attributes of Symbol Properties the panel lacked (`DIALOG_LIB_SYMBOL_PROPERTIES::TransferDataFromWindow`).

use super::*;
use eda_model::ir::Design;
use eda_model::lib_keep::LibraryPadKeep;
use eda_model::ConstraintModel;

fn design() -> Design {
    serde_json::from_str(r#"{"schema":1,"provenance":{"engine_version":"0","intent_hash":"x","seed":0,"stage_hashes":[]}}"#).unwrap()
}

fn board(m: &ConstraintModel) -> Board<'_> {
    Board::new(design(), m, Nm::from_raw_shim(100_000), Nm::from_raw_shim(300_000))
}

fn props(lib_id: &str, power: bool, unit_count: u32, units_locked: Option<bool>, power_local: Option<bool>) -> Cmd {
    Cmd::EditSymbolProperties {
        lib_id: lib_id.into(),
        reference_prefix: "U".into(),
        description: String::new(),
        keywords: String::new(),
        datasheet: String::new(),
        power,
        in_bom: true,
        on_board: true,
        pin_numbers_hidden: false,
        pin_names_hidden: false,
        pin_name_offset_mm: 0.508,
        unit_count,
        has_alternate_body_style: false,
        footprint_filters: vec![],
        units_locked,
        power_local,
        exclude_from_sim: Some(true),
        exclude_from_pos_files: None,
    }
}

fn smd_pad(number: &str, x: Nm) -> LibraryPad {
    let mut p: LibraryPad = serde_json::from_value(serde_json::json!({ "number": number, "at": { "x": x, "y": 0 }, "size": [1_000_000, 600_000], "shape": "rect", "kind": "smd" })).expect("a pad");
    p.layers = vec!["F.Cu".into(), "F.Mask".into()];
    p
}

fn sym<'a>(b: &'a Board<'_>, lib_id: &str) -> &'a LibrarySymbol {
    b.design().symbol_library.as_ref().unwrap().by_lib_id(lib_id).unwrap()
}

#[test]
fn symbol_properties_locks_the_units_of_a_symbol_of_several_units_only_and_sets_a_local_power_symbol_only_with_power() {
    let m = ConstraintModel::default();
    let mut b = board(&m);
    b.apply(&Cmd::NewSymbol { lib_id: "Lib:Q".into() }).unwrap();
    // `LockUnits( GetUnitCount() > 1 && !interchangeable )`: one unit cannot be locked
    b.apply(&props("Lib:Q", false, 1, Some(true), Some(true))).unwrap();
    let q = sym(&b, "Lib:Q");
    assert!(!q.units_locked, "a symbol of one unit has interchangeable units");
    assert!(!q.power_local, "`SetNormal()`: no local power without power");
    assert!(q.exclude_from_sim);
    b.apply(&props("Lib:Q", true, 2, Some(true), Some(true))).unwrap();
    let q = sym(&b, "Lib:Q");
    assert!(q.units_locked && q.power && q.power_local);
    // left out of the command, the flags stay
    b.apply(&props("Lib:Q", true, 2, None, None)).unwrap();
    let q = sym(&b, "Lib:Q");
    assert!(q.units_locked && q.power_local, "the file's side is `crates/kicad/tests/library_keep.rs`");
}

#[test]
fn flatten_takes_the_simulation_and_position_file_exclusions_of_the_nearest_parent() {
    // `Flatten`: "Get excluded flags from the immediate parent (first in chain)" -- sim, BOM, board and position files
    let mut root = LibrarySymbol::new_empty("Lib:Root");
    root.exclude_from_sim = true;
    root.exclude_from_pos_files = true;
    root.units_locked = true;
    let mut child = LibrarySymbol::new_empty("Lib:Child");
    child.extends = Some("Lib:Root".into());
    let section = eda_model::ir::SymbolLibrarySection { symbols: vec![root, child], ..Default::default() };
    let flat = section.flatten("Lib:Child").unwrap();
    assert!(flat.exclude_from_sim && flat.exclude_from_pos_files, "{flat:?}");
    assert!(flat.units_locked, "the root's drawing and unit options");
}

#[test]
fn paste_pad_properties_takes_the_masters_attribute_property_and_margins_but_not_its_pin_to_die_length() {
    // `PAD::ImportSettingsFrom`: `SetAttribute`, `SetProperty`, `SetPadstack` (margins, tenting); "I am not sure the m_LengthPadToDie should be imported" (#if 0)
    let fp = {
        let mut fp = LibraryFootprint::new_empty("Lib:F");
        let mut master = smd_pad("1", Nm::ZERO);
        master.kicad = LibraryPadKeep {
            connector: true,
            property: Some("pad_prop_testpoint".into()),
            solder_mask_margin: Some(Nm::from_raw_shim(50_000)),
            tokens: vec!["(die_length 1.2)".into(), "(tenting front)".into()],
            ..Default::default()
        };
        let mut other = smd_pad("2", Nm::from_raw_shim(2_000_000));
        other.kicad.tokens = vec!["(die_length 3)".into()];
        fp.pads = vec![master, other];
        fp.assign_missing_ids();
        fp
    };
    let mut dst = fp.pads[1].clone();
    library_editors::import_pad_settings(&mut dst, &fp.pads[0]);
    assert!(dst.kicad.connector);
    assert_eq!(dst.kicad.property.as_deref(), Some("pad_prop_testpoint"));
    assert_eq!(dst.kicad.solder_mask_margin, Some(Nm::from_raw_shim(50_000)));
    assert_eq!(dst.kicad.tokens, vec!["(die_length 3)".to_string(), "(tenting front)".to_string()], "its own pin-to-die length, the master's tenting");
}

#[test]
fn footprint_properties_stores_the_datasheet_and_description_rows() {
    let m = ConstraintModel::default();
    let mut b = board(&m);
    b.apply(&Cmd::NewFootprint { name: "Lib:F".into() }).unwrap();
    let mut ds = eda_model::fp_edit::default_library_datasheet_layout();
    ds.text = Some("https://example.com/f.pdf".into());
    let cmd = Cmd::EditFootprintProperties {
        name: "Lib:F".into(),
        description: "lib description".into(),
        keywords: String::new(),
        attributes: Default::default(),
        reference_visible: true,
        value_visible: true,
        model: None,
        models3d: None,
        zone_connection: None,
        reference: None,
        value: None,
        fields: None,
        datasheet: Some(ds.clone()),
        description_field: None,
    };
    b.apply(&cmd).unwrap();
    let fp = b.design().footprint_library.as_ref().unwrap().by_name("Lib:F").unwrap();
    assert_eq!(fp.datasheet.as_ref(), Some(&ds));
    assert_eq!(fp.description, "lib description", "the library description is not the Description field");
    assert!(fp.description_field.is_none(), "a row left out stays");
}
