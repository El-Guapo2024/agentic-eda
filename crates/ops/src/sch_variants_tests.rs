//! `Board::apply` for the design-variant verbs of `sch_variants.rs` (UI-LEFT S2, KiCad 25ec2335).
use super::*;
use eda_model::ir::Provenance;
use eda_model::units::mm;
use eda_model::variants;

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

fn board(m: &ConstraintModel) -> Board<'_> {
    let mut b = Board::new(empty_design(), m, mm(0.1), mm(0.3));
    for (i, id) in ["R1", "R2"].iter().enumerate() {
        b.apply(&Cmd::AddSymbol { annotate: None, id: (*id).into(), lib_id: "Device:R".into(), at: Point { x: mm(10.0 * i as f64), y: mm(10.0) }, rot_millideg: 0, value: "10k".into(), footprint: String::new(), unit: 1 }).unwrap();
    }
    b
}

fn sch<'b>(b: &'b Board<'_>) -> &'b SchematicSection {
    b.design().schematic.as_ref().unwrap()
}

fn add(b: &mut Board<'_>, name: &str, description: &str) -> Result<(), Vec<CheckResult>> {
    b.apply(&Cmd::AddDesignVariant { name: name.into(), description: description.into() })
}

fn set(variant: &str, ids: &[&str], dnp: Option<bool>, fields: &[(&str, &str)]) -> Cmd {
    Cmd::SetSymbolVariant {
        variant: variant.into(),
        ids: ids.iter().map(|s| s.to_string()).collect(),
        dnp,
        exclude_from_bom: None,
        exclude_from_board: None,
        exclude_from_sim: None,
        exclude_from_pos_files: None,
        fields: (!fields.is_empty()).then(|| fields.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()),
    }
}

#[test]
fn a_variant_is_added_with_its_trimmed_name_and_description_and_refused_as_kicad_refuses_it() {
    let m = ConstraintModel::default();
    let mut b = board(&m);
    add(&mut b, "  Lite ", " no radio ").unwrap();
    assert_eq!(sch(&b).extras.variants.get("Lite").map(String::as_str), Some("no radio"), "`Trim().Trim( false )` of both");
    let why = |r: Result<(), Vec<CheckResult>>| r.unwrap_err()[0].hint.clone().unwrap_or_default();
    assert_eq!(why(add(&mut b, "  ", "")), "Variant name cannot be empty.");
    assert_eq!(why(add(&mut b, "< DEFAULT >", "")), "'< Default >' is a reserved variant name.");
    assert_eq!(why(add(&mut b, "LITE", "")), "Variant 'Lite' already exists.");
    add(&mut b, "Full", "").unwrap();
    assert_eq!(variants::names_for_ui(b.design()), ["< Default >", "Full", "Lite"]);
}

#[test]
fn a_symbols_record_holds_the_variants_flags_and_texts_and_follows_the_variant_list() {
    let m = ConstraintModel::default();
    let mut b = board(&m);
    add(&mut b, "Lite", "").unwrap();
    add(&mut b, "Full", "").unwrap();
    assert_eq!(b.apply(&set("Nope", &["R1"], Some(true), &[])).unwrap_err()[0].check, "ops_unknown_variant");
    assert_eq!(b.apply(&set("", &["R1"], Some(true), &[])).unwrap_err()[0].check, "ops_no_variant");
    assert_eq!(b.apply(&set("Lite", &["R1"], None, &[("MPN", "x")])).unwrap_err()[0].check, "ops_unknown_item", "a field the symbol does not have");
    b.apply(&set("Lite", &["R1"], Some(true), &[("Value", "4k7")])).unwrap();
    let r1 = sch(&b).symbols.iter().find(|s| s.id == "R1").unwrap().clone();
    assert!(variants::resolved_flags(sch(&b), &r1, "Lite").dnp);
    assert!(!variants::resolved_flags(sch(&b), &r1, "Full").dnp, "another variant is the symbol's own");
    assert!(!r1.dnp, "the symbol itself is untouched");
    assert_eq!(variants::field_text(sch(&b), "R1", "Lite", "Value", "10k"), "4k7");
    // a text equal to the symbol's own takes the override away
    b.apply(&set("Lite", &["R1"], None, &[("Value", "10k")])).unwrap();
    assert!(variants::record(sch(&b), "R1", "Lite").unwrap().fields.is_empty());
    // copy, rename, describe and remove carry every record
    b.apply(&Cmd::CopyDesignVariant { name: "Lite".into(), new_name: "Lite_copy".into() }).unwrap();
    assert!(variants::record(sch(&b), "R1", "Lite_copy").is_some_and(|r| r.dnp == Some(true)));
    b.apply(&Cmd::RenameDesignVariant { name: "Lite".into(), new_name: "Basic".into() }).unwrap();
    assert!(variants::record(sch(&b), "R1", "Lite").is_none() && variants::record(sch(&b), "R1", "Basic").is_some());
    assert_eq!(b.apply(&Cmd::RenameDesignVariant { name: "Basic".into(), new_name: "full".into() }).unwrap_err()[0].hint.as_deref().unwrap(), "Variant 'Full' already exists.");
    b.apply(&Cmd::RenameDesignVariant { name: "Basic".into(), new_name: "BASIC".into() }).unwrap();
    assert!(sch(&b).extras.variants.contains_key("BASIC"), "a rename may change the case of its own name");
    b.apply(&Cmd::EditDesignVariantDescription { name: "BASIC".into(), description: "  cheap  ".into() }).unwrap();
    assert_eq!(sch(&b).extras.variants["BASIC"], "cheap");
    b.apply(&Cmd::RemoveDesignVariant { name: "BASIC".into() }).unwrap();
    assert!(variants::record(sch(&b), "R1", "BASIC").is_none(), "`DeleteVariant` drops every record of it");
    assert_eq!(variants::names(b.design()), ["Full", "Lite_copy"]);
}

#[test]
fn a_renamed_or_deleted_symbol_takes_its_records_along() {
    let m = ConstraintModel::default();
    let mut b = board(&m);
    add(&mut b, "Lite", "").unwrap();
    b.apply(&set("Lite", &["R2"], Some(true), &[])).unwrap();
    b.apply(&Cmd::RenameSymbol { id: "R2".into(), new_id: "R7".into() }).unwrap();
    assert!(variants::record(sch(&b), "R7", "Lite").is_some() && variants::record(sch(&b), "R2", "Lite").is_none());
    b.apply(&Cmd::DeleteSymbol { id: "R7".into(), unit: None }).unwrap();
    assert!(sch(&b).extras.symbol_variants.is_empty());
}

/// The Symbol Fields Table while it shows a variant (`SYMBOL_FIELDS_EDITOR_GRID_DATA_MODEL::SetCurrentVariant`): its staged edits make records, the grid shows
/// the variant's values, and a cell is marked where a symbol differs from its own value (`GetAttr`'s highlight).
#[test]
fn the_fields_table_shows_a_variant_and_marks_what_differs() {
    use crate::fields_table::{self, TableSpec, VariantFieldEdit};
    let m = ConstraintModel::default();
    let mut b = board(&m);
    add(&mut b, "Lite", "").unwrap();
    let mut s = sch(&b).clone();
    let edit = |id: &str, field: &str, value: &str| VariantFieldEdit { variant: "Lite".into(), id: id.into(), field: field.into(), value: value.into() };
    assert!(fields_table::apply_variant_edits(&mut s, &m, &[edit("R1", fields_table::EXCLUDE_FROM_BOARD, "1")]).is_err(), "`fieldSupportsVariants`: not ${{EXCLUDE_FROM_BOARD}}");
    fields_table::apply_variant_edits(&mut s, &m, &[edit("R1", "Value", "1k"), edit("R2", fields_table::DNP, "1")]).unwrap();
    let mut spec = TableSpec::default_editing();
    spec.group_symbols = false;
    for c in ["${DNP}"] {
        spec.columns.push(fields_table::Column { name: c.into(), label: c.into(), show: true, group_by: false });
    }
    let shown = variants::resolve_section(&s, "Lite");
    let mut t = fields_table::build_table(&shown, &m, &spec);
    fields_table::mark_variant_overrides(&mut t.rows, &shown, &s, &m, &spec);
    let col = |n: &str| spec.columns.iter().position(|c| c.name == n).unwrap();
    let row = |id: &str| t.rows.iter().find(|r| r.refs == [id.to_string()]).unwrap();
    assert_eq!(row("R1").cells[col("Value")], "1k");
    assert!(row("R1").overrides[col("Value")] && !row("R1").overrides[col("Reference")]);
    assert_eq!(row("R2").cells[col("${DNP}")], "1");
    assert!(row("R2").overrides[col("${DNP}")] && !row("R2").overrides[col("Value")]);
    // the default variant's grid is the symbols' own
    let own = fields_table::build_table(&s, &m, &spec);
    assert_eq!(own.rows.iter().find(|r| r.refs == ["R1".to_string()]).unwrap().cells[col("Value")], "10k");
}

/// KiCad 25ec2335 Set Variant Symbol... / Clear Variant Symbol (`SCH_EDIT_TOOL::SetVariantSymbol`, `ClearVariantSymbol`; UI-LEFT-2 SS16): a pin-compatible library symbol is
/// the variant's, one that is not is refused with KiCad's message, and Clear takes it away; the default variant has none.
#[test]
fn a_variant_symbol_is_set_when_pin_compatible_and_cleared() {
    let base = eda_model::symbol::builtin("Device:R").unwrap();
    let mut alt = base.clone();
    alt.lib_id = "Mine:R_alt".into();
    alt.connectivity.value = "R_alt".into();
    let mut bad = base.clone();
    bad.lib_id = "Mine:R_bad".into();
    bad.pins[0].at.y += 2.54;
    let m = ConstraintModel { symbols: vec![alt, bad], ..Default::default() };
    let mut b = board(&m);
    add(&mut b, "Lite", "").unwrap();
    let set_sym = |lib: Option<&str>| Cmd::SetVariantSymbol { variant: "Lite".into(), id: "R1".into(), lib_id: lib.map(str::to_string) };
    let why = |r: Result<(), Vec<CheckResult>>| r.unwrap_err()[0].hint.clone().unwrap_or_default();
    assert!(why(b.apply(&set_sym(Some("Mine:R_bad")))).starts_with("The selected symbol 'Mine:R_bad' is not pin-compatible with the base symbol 'Device:R'.\n\n\u{2022} Unit 1, body style 1: pin '1' position mismatch"));
    assert_eq!(why(b.apply(&set_sym(Some("Mine:Nope")))), "Could not load symbol 'Mine:Nope' from library.");
    b.apply(&set_sym(Some("Mine:R_alt"))).unwrap();
    assert_eq!(variants::record(sch(&b), "R1", "Lite").unwrap().symbol_override.as_deref(), Some("Mine:R_alt"));
    // a field equal to what the variant symbol gives is no text of the record's own (`SetFieldText`'s "Diff against the value the variant resolves to")
    b.apply(&set("Lite", &["R1"], None, &[("Value", "R_alt")])).unwrap();
    assert!(variants::record(sch(&b), "R1", "Lite").unwrap().fields.is_empty());
    b.apply(&set_sym(None)).unwrap();
    assert!(variants::record(sch(&b), "R1", "Lite").is_none_or(|r| r.symbol_override.is_none()));
    assert!(b.apply(&set_sym(None)).is_err(), "nothing to clear");
    assert!(b.apply(&Cmd::SetVariantSymbol { variant: String::new(), id: "R1".into(), lib_id: Some("Mine:R_alt".into()) }).is_err(), "the default variant has no variant symbol");
}

/// UI-LEFT-3 S3-18 (KiCad 25ec2335 `SCH_EDIT_TOOL::SetAttribute` in a variant: `new_state` over every collected unit, `SetDNP( new_state, sheet, variant )`):
/// Do not Populate in a variant reads the records of every unit of the reference on every sheet, so a unit DNP in the variant on the sheet in view and one on
/// another sheet that is not make the toggle set it -- on both -- and the next toggle clear it on both; a sheet toggled in the variant gets its record, its own
/// flag left alone.
#[test]
fn a_toggle_in_a_variant_reads_and_sets_every_unit_on_every_sheet() {
    let m = ConstraintModel::default();
    let mut b = board(&m);
    add(&mut b, "Lite", "").unwrap();
    b.apply(&Cmd::AddSheet { name: "Sub".into(), file: "sub".into(), at: Point { x: mm(60.0), y: mm(10.0) }, size: (mm(20.0), mm(20.0)) }).unwrap();
    let mut d = b.design().clone();
    let sheet_id = d.schematic.as_ref().unwrap().sheets[0].id.clone();
    let key = d.schematic.as_ref().unwrap().sheets[0].file.clone();
    let unit2 = eda_model::ir::SymbolInstance { unit: 2, at: Point { x: mm(30.0), y: mm(30.0) }, ..d.schematic.as_ref().unwrap().symbols[0].clone() };
    d.sheet_contents.get_or_insert_with(Default::default).entry(key.clone()).or_default().symbols.push(unit2);
    let mut rec = variants::SymbolVariant::initialized(variants::SymbolFlags::default());
    rec.dnp = Some(true);
    d.schematic.as_mut().unwrap().extras.symbol_variants.entry("R1".into()).or_default().insert("Lite".into(), rec);
    let mut b = Board::new(d, &m, mm(0.1), mm(0.3));
    let toggle = |ids: &[&str]| Cmd::SchEdit(crate::sch_edit::SchCmd::ToggleSymbolAttribute { ids: ids.iter().map(|s| s.to_string()).collect(), attribute: "dnp".into(), variant: "Lite".into() });
    let dnp_in_lite = |b: &Board<'_>| -> (bool, bool) {
        let d = b.design();
        let root = d.schematic.as_ref().unwrap();
        let sub = &d.sheet_contents.as_ref().unwrap()[&key];
        (variants::resolved_flags(root, &root.symbols[0], "Lite").dnp, variants::resolved_flags(sub, &sub.symbols[0], "Lite").dnp)
    };
    assert_eq!(dnp_in_lite(&b), (true, false));
    b.apply(&toggle(&["R1"])).unwrap();
    assert_eq!(dnp_in_lite(&b), (true, true), "the unit on the other sheet lacked it: set on both");
    assert!(!b.design().schematic.as_ref().unwrap().symbols[0].dnp, "the symbol's own flag is untouched");
    b.apply(&toggle(&["R1"])).unwrap();
    assert_eq!(dnp_in_lite(&b), (false, false), "every unit had it: cleared on both");
    b.apply(&toggle(&[sheet_id.as_str()])).unwrap();
    let root = b.design().schematic.as_ref().unwrap();
    assert!(variants::sheet_flags(root, &sheet_id, "Lite").dnp && !root.extras.sheet_props_of(&sheet_id).dnp, "the sheet's record of the variant, not its own flag");
}
