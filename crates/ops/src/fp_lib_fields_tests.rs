//! The fields grid of the Footprint Editor's Footprint Properties (`DIALOG_FOOTPRINT_PROPERTIES_FP_EDITOR`): the Reference's and the Value's layout, the user fields, the rules
//! the dialog's rows are judged by, and that the grid is one step with the rest of the panel.

use super::pcb_transform_tests::{board, model, p};
use super::*;
use eda_model::fp_edit::FieldLayout;
use eda_model::ir::FootprintField;

fn layout(x: Nm, y: Nm, layer: &str) -> FieldLayout {
    FieldLayout::new(p(x, y), layer)
}

fn field(name: &str, value: &str, visible: bool, l: Option<FieldLayout>) -> FootprintField {
    FootprintField { name: name.into(), value: value.into(), visible, layout: l }
}

/// The whole panel (the Reference shown, the Value hidden) with the grid `reference`, `value` and `fields` in it.
fn panel(name: &str, description: &str, reference: Option<FieldLayout>, value: Option<FieldLayout>, fields: Option<Vec<FootprintField>>) -> Cmd {
    panel_shown(name, description, true, false, reference, value, fields)
}

fn panel_shown(name: &str, description: &str, reference_visible: bool, value_visible: bool, reference: Option<FieldLayout>, value: Option<FieldLayout>, fields: Option<Vec<FootprintField>>) -> Cmd {
    Cmd::EditFootprintProperties {
        name: name.into(),
        description: description.into(),
        keywords: String::new(),
        attributes: FootprintAttributes::default(),
        reference_visible,
        value_visible,
        model: None,
        models3d: None,
        zone_connection: None,
        reference,
        value,
        fields,
        datasheet: None,
        description_field: None,
    }
}

fn refusal(b: &mut Board<'_>, cmd: Cmd) -> String {
    let e = b.apply(&cmd).unwrap_err();
    format!("{}: {}", e[0].check, e[0].hint.clone().unwrap_or_default())
}

fn fp<'a>(b: &'a Board<'_>, name: &str) -> &'a LibraryFootprint {
    b.design().footprint_library.as_ref().unwrap().by_name(name).unwrap()
}

#[test]
fn the_grid_is_stored_with_the_panel_and_a_visibility_is_the_footprints_own() {
    let m = model(&["F.Cu", "B.Cu"]);
    let mut b = board(&m);
    b.apply(&Cmd::NewFootprint { name: "Test:F".into() }).unwrap();
    let mut reference = layout(Nm::from_raw_shim(500_000), Nm::from_raw_shim(-2_500_000), "F.SilkS");
    reference.size = (Nm::from_raw_shim(1_200_000), Nm::from_raw_shim(1_000_000));
    reference.text = Some("J**".into());
    reference.visible = false; // the check box says shown: the panel's `reference_visible` wins
    let mut mpn = layout(Nm::from_raw_shim(1_500_000), Nm::from_raw_shim(3_000_000), "F.Fab");
    mpn.bold = true;
    mpn.visible = true; // the field says hidden
    b.apply(&panel("Test:F", "a connector", Some(reference.clone()), Some(layout(Nm::ZERO, Nm::from_raw_shim(2_200_000), "F.Fab")), Some(vec![field("MPN", "ABC-1", false, Some(mpn.clone())), field("Vendor", "ACME", true, None)]))).unwrap();
    let f = fp(&b, "Test:F");
    assert_eq!(f.description, "a connector");
    let r = f.reference.as_ref().expect("the Reference's layout");
    assert_eq!((r.at, r.size, r.text.as_deref(), r.visible), (p(Nm::from_raw_shim(500_000), Nm::from_raw_shim(-2_500_000)), (Nm::from_raw_shim(1_200_000), Nm::from_raw_shim(1_000_000)), Some("J**"), true), "`reference_visible` is the Show check box");
    let v = f.value.as_ref().expect("the Value's layout");
    assert!(!v.visible && !f.value_visible, "the Value is hidden, in both places");
    assert_eq!(f.fields.len(), 2);
    assert_eq!((f.fields[0].name.as_str(), f.fields[0].value.as_str(), f.fields[0].visible), ("MPN", "ABC-1", false));
    let l = f.fields[0].layout.as_ref().unwrap();
    assert_eq!((l.at, l.bold, l.visible), (p(Nm::from_raw_shim(1_500_000), Nm::from_raw_shim(3_000_000)), true, false), "a field's layout follows its own `visible`");
    assert!(f.fields[1].layout.is_none(), "a field with no layout keeps none: the file gives it the default");
}

#[test]
fn a_panel_that_leaves_the_grid_out_keeps_it_and_an_empty_list_clears_the_fields() {
    let m = model(&["F.Cu", "B.Cu"]);
    let mut b = board(&m);
    b.apply(&Cmd::NewFootprint { name: "Test:F".into() }).unwrap();
    b.apply(&panel("Test:F", "one", Some(layout(Nm::ZERO, Nm::from_raw_shim(-2_000_000), "F.SilkS")), None, Some(vec![field("MPN", "x", true, None)]))).unwrap();
    b.apply(&panel("Test:F", "two", None, None, None)).unwrap();
    let f = fp(&b, "Test:F");
    assert_eq!(f.description, "two");
    assert_eq!((f.reference.is_some(), f.fields.len()), (true, 1), "left out, the grid stays");
    b.apply(&panel("Test:F", "three", None, None, Some(vec![]))).unwrap();
    assert!(fp(&b, "Test:F").fields.is_empty(), "an empty list removes the user fields");
    // the visibility of a Reference that is kept follows the panel's check box
    b.apply(&panel_shown("Test:F", "four", false, true, None, None, None)).unwrap();
    let f = fp(&b, "Test:F");
    assert_eq!((f.reference_visible, f.reference.as_ref().map(|l| l.visible), f.value_visible), (false, Some(false), true));
}

#[test]
fn a_row_the_dialog_would_refuse_is_refused_before_anything_of_the_panel_is_taken() {
    let m = model(&["F.Cu", "B.Cu"]);
    let mut b = board(&m);
    b.apply(&Cmd::NewFootprint { name: "Test:F".into() }).unwrap();
    let mut tiny = layout(Nm::ZERO, Nm::ZERO, "F.Fab");
    tiny.size = (Nm::ZERO, Nm::from_raw_shim(1_000_000));
    let mut thick = layout(Nm::ZERO, Nm::ZERO, "F.Fab");
    thick.thickness = Nm::from_raw_shim(900_000);
    let cases: Vec<(Vec<FootprintField>, &str)> = vec![
        (vec![field("", "text", true, None)], "Fields must have a name."),
        (vec![field(" MPN", "x", true, None)], "starts or ends with a space"),
        (vec![field("MPN", "a", true, None), field("mpn", "b", true, None)], "There are two fields called"),
        (vec![field("Reference", "x", true, None)], "is the name of a mandatory field"),
        (vec![field("Datasheet", "x", true, None)], "is the name of a mandatory field"),
        (vec![field("ki_keywords", "x", true, None)], "not a field"),
        (vec![field("A", "x", true, Some(tiny.clone()))], "Text width must be at least"),
        (vec![field("A", "x", true, Some(thick))], "thickness is too large"),
        (vec![field("A", "x", true, Some(layout(Nm::ZERO, Nm::ZERO, "Nowhere.Layer")))], "is not a layer of this board"),
    ];
    for (fields, expected) in cases {
        let said = refusal(&mut b, panel("Test:F", "changed", None, None, Some(fields)));
        assert!(said.starts_with("ops_bad_field") && said.contains(expected), "{said:?} for {expected:?}");
        assert_eq!(fp(&b, "Test:F").description, "", "a refused panel takes nothing: {expected}");
    }
    let said = refusal(&mut b, panel("Test:F", "changed", Some(tiny), None, None));
    assert!(said.contains("Text width must be at least"), "{said}");
    assert!(fp(&b, "Test:F").fields.is_empty());
}
