//! Derived symbols in the Symbol Editor: Derive from Existing Symbol, Flatten Symbol, Update Symbol Fields, the parent choice and the fields grid; what a rename, a delete and a
//! put do to the symbols derived from one, and what a derived symbol refuses (the drawing is its parent's).

use super::*;
use eda_model::ir::{Design, LibraryFill};
use eda_model::sym_fields::LibrarySymbolField;
use eda_model::symbol::SPoint;
use eda_model::ConstraintModel;

fn design() -> Design {
    serde_json::from_str(r#"{"schema":1,"provenance":{"engine_version":"0","intent_hash":"x","seed":0,"stage_hashes":[]}}"#).unwrap()
}

fn board(m: &ConstraintModel) -> Board<'_> {
    Board::new(design(), m, Nm::from_raw_shim(100_000), Nm::from_raw_shim(300_000))
}

/// `Lib:Base`: a rectangle, two pins, a datasheet, a footprint and a user field.
fn base() -> LibrarySymbol {
    let mut s = LibrarySymbol::new_empty("Lib:Base");
    s.reference_prefix = "U".into();
    s.description = "the base".into();
    s.keywords = "base chip".into();
    s.graphics.push(LibrarySymbolGraphic::Rectangle { id: String::new(), unit: 1, body_style: 1, start: SPoint { x: -2.54, y: -2.54 }, end: SPoint { x: 2.54, y: 2.54 }, stroke_mm: 0.254, fill: LibraryFill::None });
    for (n, y) in [("1", 1.27), ("2", -1.27)] {
        s.pins.push(LibrarySymbolPin {
            id: String::new(),
            number: n.into(),
            name: format!("P{n}"),
            electrical_type: "passive".into(),
            shape: "line".into(),
            at: SPoint { x: -5.08, y },
            angle_deg: 0.0,
            length_mm: 2.54,
            unit: 1,
            body_style: 1,
            hidden: false,
            name_size_mm: None,
            number_size_mm: None,
            alternates: Vec::new(),
        });
    }
    let mut f = s.all_fields();
    f[1].text = "BASE".into();
    f[2].text = "Package_SO:SOIC-8".into();
    f[3].text = "http://base/ds.pdf".into();
    f.push(LibrarySymbolField { name: "MPN".into(), text: "X-1".into(), at: SPoint { x: 0.0, y: -5.0 }, hidden: true, ..Default::default() });
    s.set_fields(f);
    s.assign_missing_ids();
    s
}

fn with_base(m: &ConstraintModel) -> Board<'_> {
    let mut b = board(m);
    b.apply(&Cmd::PutLibrarySymbol { symbol: base(), overwrite: false }).unwrap();
    b
}

fn derive(parent: &str, lib_id: &str) -> Cmd {
    Cmd::DeriveSymbol { parent: parent.into(), lib_id: lib_id.into(), keep_footprint: false, keep_datasheet: false, transfer_user_fields: false, keep_content_user_fields: false, overwrite: false }
}

fn sym<'b>(b: &'b Board<'_>, lib_id: &str) -> &'b LibrarySymbol {
    b.design().symbol_library.as_ref().unwrap().by_lib_id(lib_id).unwrap_or_else(|| panic!("{lib_id} is in the library"))
}

#[test]
fn deriving_makes_a_symbol_that_extends_its_parent_with_the_parents_fields_and_no_drawing() {
    let m = ConstraintModel::default();
    let mut b = with_base(&m);
    b.apply(&derive("Lib:Base", "Lib:Child")).unwrap();
    let child = sym(&b, "Lib:Child");
    assert_eq!(child.extends.as_deref(), Some("Lib:Base"));
    assert!(child.graphics.is_empty() && child.pins.is_empty(), "no drawing of its own");
    let f = child.all_fields();
    assert_eq!((f[0].text.as_str(), f[1].text.as_str(), f[4].text.as_str()), ("U", "BASE", "the base"), "the Reference, the Value and the Description are copied");
    assert_eq!((f[2].text.as_str(), f[3].text.as_str()), ("", ""), "the Footprint and the Datasheet are not kept unless asked");
    assert!(child.user_fields().is_empty(), "nor the user fields");
    assert_eq!(f[0].at, sym(&b, "Lib:Base").all_fields()[0].at, "the layout comes with the field");

    // what the dialog's boxes keep
    b.apply(&Cmd::DeriveSymbol { parent: "Lib:Base".into(), lib_id: "Lib:Kept".into(), keep_footprint: true, keep_datasheet: true, transfer_user_fields: true, keep_content_user_fields: true, overwrite: false }).unwrap();
    let kept = sym(&b, "Lib:Kept");
    assert_eq!((kept.field("Footprint").unwrap().text.as_str(), kept.field("Datasheet").unwrap().text.as_str()), ("Package_SO:SOIC-8", "http://base/ds.pdf"));
    assert_eq!(kept.field("MPN").unwrap().text, "X-1");
    b.apply(&Cmd::DeriveSymbol { parent: "Lib:Base".into(), lib_id: "Lib:NoContent".into(), keep_footprint: false, keep_datasheet: false, transfer_user_fields: true, keep_content_user_fields: false, overwrite: false }).unwrap();
    assert_eq!(sym(&b, "Lib:NoContent").field("MPN").unwrap().text, "");
    // a symbol can be derived from a derived symbol
    b.apply(&derive("Lib:Child", "Lib:Grandchild")).unwrap();
    assert_eq!(b.design().symbol_library.as_ref().unwrap().parent_chain("Lib:Grandchild").parents, ["Lib:Child", "Lib:Base"]);
}

#[test]
fn deriving_is_refused_for_an_unknown_parent_another_library_a_taken_name_and_a_bad_name() {
    let m = ConstraintModel::default();
    let mut b = with_base(&m);
    assert_eq!(b.apply(&derive("Lib:Nope", "Lib:X")).unwrap_err()[0].check, "ops_unknown_symbol");
    assert_eq!(b.apply(&derive("Lib:Base", "Other:X")).unwrap_err()[0].check, "ops_bad_symbol", "a derived symbol is in its parent's library");
    b.apply(&derive("Lib:Base", "Lib:Child")).unwrap();
    assert_eq!(b.apply(&derive("Lib:Base", "Lib:Child")).unwrap_err()[0].check, "ops_symbol_exists");
    assert_eq!(b.apply(&derive("Lib:Base", "Lib:Base")).unwrap_err()[0].check, "ops_symbol_exists");
    assert!(b.apply(&derive("Lib:Base", "Lib:")).is_err());
    assert!(b.apply(&derive("Lib:Base", "Lib:a:b")).is_err());
    // overwriting a symbol that is its own parent's ancestor would run the chain back into it
    let over = Cmd::DeriveSymbol { parent: "Lib:Child".into(), lib_id: "Lib:Base".into(), keep_footprint: false, keep_datasheet: false, transfer_user_fields: false, keep_content_user_fields: false, overwrite: true };
    assert_eq!(b.apply(&over).unwrap_err()[0].check, "ops_symbol_cycle");
    // overwrite replaces a symbol that is no ancestor
    let over = Cmd::DeriveSymbol { parent: "Lib:Base".into(), lib_id: "Lib:Child".into(), keep_footprint: true, keep_datasheet: false, transfer_user_fields: false, keep_content_user_fields: false, overwrite: true };
    b.apply(&over).unwrap();
    assert_eq!(sym(&b, "Lib:Child").field("Footprint").unwrap().text, "Package_SO:SOIC-8");
    assert_eq!(b.design().symbol_library.as_ref().unwrap().symbols.iter().filter(|s| s.lib_id == "Lib:Child").count(), 1);
}

#[test]
fn flatten_makes_the_derived_symbol_the_flat_symbol_it_stood_for() {
    let m = ConstraintModel::default();
    let mut b = with_base(&m);
    b.apply(&derive("Lib:Base", "Lib:Child")).unwrap();
    let mut fields = sym(&b, "Lib:Child").all_fields();
    fields[1].text = "CHILD".into();
    b.apply(&Cmd::EditLibrarySymbolFields { lib_id: "Lib:Child".into(), fields }).unwrap();
    let expected = b.design().symbol_library.as_ref().unwrap().flatten("Lib:Child").unwrap();
    b.apply(&Cmd::FlattenSymbol { lib_id: "Lib:Child".into() }).unwrap();
    let flat = sym(&b, "Lib:Child");
    assert_eq!(flat.extends, None);
    assert_eq!((flat.graphics.len(), flat.pins.len()), (1, 2), "the parent's drawing is its own now");
    assert!(flat.pins.iter().all(|p| !p.id.is_empty()), "with ids");
    assert_eq!(flat.all_fields(), expected.all_fields());
    assert_eq!((flat.keywords.as_str(), flat.field("Value").unwrap().text.as_str()), ("base chip", "CHILD"));
    // and it is editable now
    b.apply(&Cmd::AddSymbolPin { lib_id: "Lib:Child".into(), pin: flat.pins[0].clone() }).unwrap();
    assert_eq!(b.apply(&Cmd::FlattenSymbol { lib_id: "Lib:Child".into() }).unwrap_err()[0].check, "ops_not_derived", "KiCad offers it for a derived symbol only");
    assert_eq!(b.apply(&Cmd::FlattenSymbol { lib_id: "Lib:Nope".into() }).unwrap_err()[0].check, "ops_unknown_symbol");
}

#[test]
fn a_derived_symbol_refuses_every_edit_of_the_drawing_it_does_not_have() {
    let m = ConstraintModel::default();
    let mut b = with_base(&m);
    b.apply(&derive("Lib:Base", "Lib:Child")).unwrap();
    let pin = sym(&b, "Lib:Base").pins[0].clone();
    let shape = sym(&b, "Lib:Base").graphics[0].clone();
    let at = SPoint { x: 1.0, y: 1.0 };
    let refused = [
        Cmd::AddSymbolPin { lib_id: "Lib:Child".into(), pin: pin.clone() },
        Cmd::MoveSymbolPin { lib_id: "Lib:Child".into(), id: pin.id.clone(), x: 0.0, y: 0.0 },
        Cmd::DeleteSymbolPin { lib_id: "Lib:Child".into(), id: pin.id.clone() },
        Cmd::EditSymbolPin { lib_id: "Lib:Child".into(), id: pin.id.clone(), pin: pin.clone() },
        Cmd::PushPinProperty { lib_id: "Lib:Child".into(), source_pin_id: pin.id.clone(), field: PushPinField::Length, body_style: None },
        Cmd::AddSymbolGraphic { lib_id: "Lib:Child".into(), graphic: shape.clone() },
        Cmd::DeleteSymbolGraphic { lib_id: "Lib:Child".into(), id: shape.id().to_string() },
        Cmd::MoveSymbolGraphic { lib_id: "Lib:Child".into(), id: shape.id().to_string(), dx_mm: 1.0, dy_mm: 1.0 },
        Cmd::EditSymbolGraphic { lib_id: "Lib:Child".into(), id: shape.id().to_string(), stroke_mm: 0.1, fill: LibraryFill::None, unit: None, body_style: None },
        Cmd::SetSymbolAnchor { lib_id: "Lib:Child".into(), at },
    ];
    for cmd in refused {
        let e = b.apply(&cmd).unwrap_err();
        assert_eq!(e[0].check, "ops_derived_symbol", "{cmd:?}");
        assert!(e[0].hint.as_deref().unwrap_or("").contains("Lib:Base"), "it names the parent: {:?}", e[0].hint);
    }
    // the parent is editable, and what is not the drawing is the child's own
    b.apply(&Cmd::AddSymbolPin { lib_id: "Lib:Base".into(), pin }).unwrap();
    let fields = sym(&b, "Lib:Child").all_fields();
    b.apply(&Cmd::EditLibrarySymbolFields { lib_id: "Lib:Child".into(), fields }).unwrap();
}

#[test]
fn update_symbol_fields_makes_the_checked_fields_of_a_derived_symbol_follow_its_parent() {
    let m = ConstraintModel::default();
    let mut b = with_base(&m);
    b.apply(&Cmd::DeriveSymbol { parent: "Lib:Base".into(), lib_id: "Lib:Child".into(), keep_footprint: true, keep_datasheet: true, transfer_user_fields: false, keep_content_user_fields: false, overwrite: false }).unwrap();
    // the child strays: its own Footprint and Description, a field of its own; then the parent changes
    let mut f = sym(&b, "Lib:Child").all_fields();
    f[2].text = "Mine:FP".into();
    f[4].text = "my description".into();
    f.push(LibrarySymbolField::named("Colour", "red"));
    b.apply(&Cmd::EditLibrarySymbolFields { lib_id: "Lib:Child".into(), fields: f }).unwrap();
    let mut pf = sym(&b, "Lib:Base").all_fields();
    pf[3].text = "http://new".into();
    b.apply(&Cmd::EditLibrarySymbolFields { lib_id: "Lib:Base".into(), fields: pf }).unwrap();

    let update = |fields: &[&str], remove_extras: bool| Cmd::UpdateSymbolFields {
        lib_id: "Lib:Child".into(),
        fields: fields.iter().map(|s| s.to_string()).collect(),
        remove_extras,
        reset_empty: false,
        reset_text: true,
        reset_visibility: true,
        reset_effects: true,
        reset_positions: true,
    };
    b.apply(&update(&["Datasheet", "MPN"], false)).unwrap();
    let c = sym(&b, "Lib:Child");
    assert_eq!(c.field("Datasheet").unwrap().text, "http://new", "the Datasheet follows");
    assert_eq!(c.datasheet, "http://new", "and so does the member that holds it");
    assert_eq!(c.field("MPN").unwrap().text, "X-1", "a field of the parent's the child lacked is added");
    assert_eq!((c.field("Footprint").unwrap().text.as_str(), c.description.as_str()), ("Mine:FP", "my description"), "what is not checked stays");
    assert!(c.field("Colour").is_some());
    b.apply(&update(&["Colour"], true)).unwrap();
    assert!(sym(&b, "Lib:Child").field("Colour").is_none(), "Remove fields if not in parent symbol");
    b.apply(&update(&["Footprint", "Description"], false)).unwrap();
    let c = sym(&b, "Lib:Child");
    assert_eq!((c.field("Footprint").unwrap().text.as_str(), c.description.as_str()), ("Package_SO:SOIC-8", "the base"));

    // not for a symbol that is not derived, with KiCad's own words
    let e = b.apply(&Cmd::UpdateSymbolFields { lib_id: "Lib:Base".into(), fields: vec!["Datasheet".into()], remove_extras: false, reset_empty: false, reset_text: true, reset_visibility: true, reset_effects: true, reset_positions: true }).unwrap_err();
    assert_eq!((e[0].check.as_str(), e[0].hint.as_deref()), ("ops_not_derived", Some("Symbol is not derived from another symbol.")));
}

#[test]
fn the_parent_of_a_derived_symbol_can_be_changed_but_never_into_a_cycle() {
    let m = ConstraintModel::default();
    let mut b = with_base(&m);
    let mut other = base();
    other.lib_id = "Lib:Other".into();
    b.apply(&Cmd::PutLibrarySymbol { symbol: other, overwrite: false }).unwrap();
    b.apply(&derive("Lib:Base", "Lib:Child")).unwrap();
    b.apply(&derive("Lib:Child", "Lib:Grandchild")).unwrap();
    b.apply(&Cmd::SetSymbolParent { lib_id: "Lib:Child".into(), parent: "Lib:Other".into() }).unwrap();
    assert_eq!(sym(&b, "Lib:Child").extends.as_deref(), Some("Lib:Other"));
    assert_eq!(b.design().symbol_library.as_ref().unwrap().root_of("Lib:Grandchild").unwrap().lib_id, "Lib:Other", "the grandchild follows its parent");
    assert_eq!(b.apply(&Cmd::SetSymbolParent { lib_id: "Lib:Child".into(), parent: "Lib:Child".into() }).unwrap_err()[0].check, "ops_symbol_cycle", "not itself");
    assert_eq!(b.apply(&Cmd::SetSymbolParent { lib_id: "Lib:Child".into(), parent: "Lib:Grandchild".into() }).unwrap_err()[0].check, "ops_symbol_cycle", "not a symbol derived from it");
    assert_eq!(b.apply(&Cmd::SetSymbolParent { lib_id: "Lib:Child".into(), parent: "Lib:Nope".into() }).unwrap_err()[0].check, "ops_unknown_symbol");
    assert_eq!(b.apply(&Cmd::SetSymbolParent { lib_id: "Lib:Child".into(), parent: "Else:Base".into() }).unwrap_err()[0].check, "ops_unknown_symbol");
    assert_eq!(b.apply(&Cmd::SetSymbolParent { lib_id: "Lib:Other".into(), parent: "Lib:Base".into() }).unwrap_err()[0].check, "ops_not_derived", "a root is not made a child here");
    assert!(b.apply(&Cmd::SetSymbolParent { lib_id: "Lib:Child".into(), parent: String::new() }).is_err());
}

#[test]
fn the_fields_grid_is_validated_like_the_dialog_and_keeps_the_members_in_step() {
    let m = ConstraintModel::default();
    let mut b = with_base(&m);
    let mut f = sym(&b, "Lib:Base").all_fields();
    f[0].text = "IC".into();
    f[3].text = "http://changed".into();
    f[1].at = SPoint { x: 3.0, y: 3.0 };
    f[1].hidden = true;
    f.push(LibrarySymbolField::named("Tolerance", "1%"));
    f.push(LibrarySymbolField::named("", "")); // an empty row of the grid
    b.apply(&Cmd::EditLibrarySymbolFields { lib_id: "Lib:Base".into(), fields: f.clone() }).unwrap();
    let s = sym(&b, "Lib:Base");
    assert_eq!((s.reference_prefix.as_str(), s.datasheet.as_str()), ("IC", "http://changed"), "the members that hold the three texts follow the grid");
    assert!(s.field("Value").unwrap().hidden && s.field("Value").unwrap().at.x == 3.0);
    assert_eq!(s.user_fields().iter().map(|f| f.name.as_str()).collect::<Vec<_>>(), ["MPN", "Tolerance"], "the empty row is dropped");

    let named = |name: &str, text: &str| {
        let mut g = f.clone();
        g.push(LibrarySymbolField::named(name, text));
        Cmd::EditLibrarySymbolFields { lib_id: "Lib:Base".into(), fields: g }
    };
    let e = b.apply(&named("", "no name")).unwrap_err();
    assert_eq!((e[0].check.as_str(), e[0].hint.as_deref()), ("ops_bad_field", Some("Fields must have a name.")));
    assert!(b.apply(&named("MPN", "twice")).unwrap_err()[0].hint.as_deref().unwrap().contains("already in use"));
    assert_eq!(b.apply(&named("ki_keywords", "x")).unwrap_err()[0].check, "ops_bad_field", "the keywords are the symbol's own, not a field");
    let mut big = f.clone();
    big[0].size_mm = 0.0;
    assert_eq!(b.apply(&Cmd::EditLibrarySymbolFields { lib_id: "Lib:Base".into(), fields: big }).unwrap_err()[0].check, "ops_bad_field");
    assert_eq!(b.apply(&Cmd::EditLibrarySymbolFields { lib_id: "Lib:Nope".into(), fields: f }).unwrap_err()[0].check, "ops_unknown_symbol");
}

#[test]
fn a_rename_follows_the_children_and_a_value_that_was_the_name_and_a_delete_takes_the_derived_symbols() {
    let m = ConstraintModel::default();
    let mut b = with_base(&m);
    // the Value of Base is "BASE", not its name: it stays; one that is the name follows
    let mut f = sym(&b, "Lib:Base").all_fields();
    f[1].text = "Base".into();
    b.apply(&Cmd::EditLibrarySymbolFields { lib_id: "Lib:Base".into(), fields: f }).unwrap();
    b.apply(&derive("Lib:Base", "Lib:Child")).unwrap();
    b.apply(&derive("Lib:Child", "Lib:Grandchild")).unwrap();
    b.apply(&Cmd::RenameLibrarySymbol { lib_id: "Lib:Base".into(), new_lib_id: "Lib:Root".into(), overwrite: false }).unwrap();
    assert_eq!(sym(&b, "Lib:Root").field("Value").unwrap().text, "Root");
    assert_eq!(sym(&b, "Lib:Child").extends.as_deref(), Some("Lib:Root"), "the child follows its parent's new name");
    assert_eq!(b.design().symbol_library.as_ref().unwrap().flatten("Lib:Grandchild").unwrap().pins.len(), 2, "and the chain still reaches the drawing");
    b.apply(&Cmd::RenameLibrarySymbol { lib_id: "Lib:Child".into(), new_lib_id: "Lib:Kid".into(), overwrite: false }).unwrap();
    assert_eq!(sym(&b, "Lib:Grandchild").extends.as_deref(), Some("Lib:Kid"));

    b.apply(&Cmd::DeleteLibrarySymbol { lib_id: "Lib:Root".into() }).unwrap();
    assert!(b.design().symbol_library.as_ref().unwrap().symbols.is_empty(), "deleting a base symbol deletes every symbol derived from it");
    let mut b = with_base(&m);
    b.apply(&derive("Lib:Base", "Lib:Child")).unwrap();
    b.apply(&Cmd::DeleteLibrarySymbol { lib_id: "Lib:Child".into() }).unwrap();
    assert!(b.design().symbol_library.as_ref().unwrap().by_lib_id("Lib:Base").is_some(), "a derived symbol alone goes");
}

#[test]
fn a_put_names_the_parent_in_the_symbols_own_library_and_needs_it_there() {
    let m = ConstraintModel::default();
    let mut b = with_base(&m);
    // what a `.kicad_sym` read gives: the bare name of the parent, the library the caller's
    let mut child = LibrarySymbol::new_empty("Lib:FromFile");
    child.extends = Some("Base".into());
    b.apply(&Cmd::PutLibrarySymbol { symbol: child.clone(), overwrite: false }).unwrap();
    assert_eq!(sym(&b, "Lib:FromFile").extends.as_deref(), Some("Lib:Base"));
    let mut orphan = LibrarySymbol::new_empty("Lib:Orphan");
    orphan.extends = Some("Lib:Gone".into());
    assert_eq!(b.apply(&Cmd::PutLibrarySymbol { symbol: orphan, overwrite: false }).unwrap_err()[0].check, "ops_symbol_parent_missing");
    let mut selfish = LibrarySymbol::new_empty("Lib:Self");
    selfish.extends = Some("Lib:Self".into());
    assert_eq!(b.apply(&Cmd::PutLibrarySymbol { symbol: selfish, overwrite: false }).unwrap_err()[0].check, "ops_symbol_cycle");
    // overwriting Base with a symbol that extends something derived from Base would close a loop
    let mut loopy = LibrarySymbol::new_empty("Lib:Base");
    loopy.extends = Some("Lib:FromFile".into());
    assert_eq!(b.apply(&Cmd::PutLibrarySymbol { symbol: loopy, overwrite: true }).unwrap_err()[0].check, "ops_symbol_cycle");
}

#[test]
fn the_new_verbs_run_in_the_symbol_editors_domain_and_read_from_json_with_the_dialogs_defaults() {
    let cmds = [
        derive("Lib:A", "Lib:B"),
        Cmd::FlattenSymbol { lib_id: "Lib:B".into() },
        Cmd::SetSymbolParent { lib_id: "Lib:B".into(), parent: "Lib:A".into() },
        Cmd::EditLibrarySymbolFields { lib_id: "Lib:B".into(), fields: vec![] },
    ];
    for c in cmds {
        assert_eq!(c.domain(), Domain::SymbolEditor, "{c:?}");
    }
    match serde_json::from_value::<Cmd>(serde_json::json!({ "op": "update_symbol_fields", "lib_id": "Lib:B", "fields": ["Datasheet"] })).unwrap() {
        Cmd::UpdateSymbolFields { remove_extras, reset_empty, reset_text, reset_visibility, reset_effects, reset_positions, .. } => {
            // DIALOG_UPDATE_SYMBOL_FIELDS: text, visibilities, sizes and styles, positions on; "remove" and "reset empty" off
            assert_eq!((remove_extras, reset_empty, reset_text, reset_visibility, reset_effects, reset_positions), (false, false, true, true, true, true));
        }
        other => panic!("{other:?}"),
    }
    match serde_json::from_value::<Cmd>(serde_json::json!({ "op": "derive_symbol", "parent": "Lib:A", "lib_id": "Lib:B", "keep_footprint": true })).unwrap() {
        Cmd::DeriveSymbol { keep_footprint, keep_datasheet, transfer_user_fields, overwrite, .. } => assert_eq!((keep_footprint, keep_datasheet, transfer_user_fields, overwrite), (true, false, false, false)),
        other => panic!("{other:?}"),
    }
}
