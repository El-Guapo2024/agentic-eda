//! The DRC Rule Editor's round trips: every panel from the editor to `.kicad_dru` text and back, KiCad's own QA rule files read and written back with
//! their rules unchanged, and the loader's, saver's and checks' KiCad behaviours one by one.

use super::*;
use crate::dru;

fn board() -> EditorBoard {
    let mut b = EditorBoard::two_layer();
    b.copper = vec!["F.Cu".into(), "In1.Cu".into(), "In2.Cu".into(), "B.Cu".into()];
    b.enabled.splice(1..1, ["In1.Cu".to_string(), "In2.Cu".to_string()]);
    b
}

fn env() -> Rc<Env> {
    board().env()
}

/// What a parsed rule says, for comparing two files rule by rule.
fn summary(r: &Rule) -> String {
    let mut s = format!(
        "{:?} cond={:?} layer={:?}/{:?} sev={:?}",
        r.name,
        r.condition.as_ref().map(|c| c.expression().to_string()),
        r.layer_source,
        r.layers,
        r.severity
    );
    for c in &r.constraints {
        s += &format!(" [{:?} {:?} {:?} {:?} d={} t={:?} o={}]", c.ctype, c.value.min, c.value.opt, c.value.max, c.disallow, c.test.as_ref().map(|t| t.expression().to_string()), c.options);
    }
    s
}

fn rules_of(text: &str) -> Vec<String> {
    let mut t = text.to_string();
    if !t.contains("(version") {
        t.insert_str(0, "(version 1)\n");
    }
    dru::parse_rules(&t, &env()).unwrap_or_else(|e| panic!("{e}\n{text}")).rules.iter().map(summary).collect()
}

fn edit(panel: PanelType, name: &str, fields: Fields) -> RuleEdit {
    RuleEdit {
        panel,
        name: name.into(),
        comment: String::new(),
        condition: String::new(),
        layer: String::new(),
        fields,
        value_text: None,
        taken_names: Vec::new(),
        severity: None,
        constraint_code: None,
    }
}

/// A new rule saved into an empty file, then the file read back: the text, and the one entry it reads back as.
fn round_trip(e: &RuleEdit) -> (String, Entry) {
    let b = board();
    let mut entries = Vec::new();
    apply_edit(&mut entries, None, e, &b).unwrap_or_else(|m| panic!("{m}"));
    let text = save(&entries, &b);
    let back = load(&text, &b.env()).unwrap_or_else(|err| panic!("{err}\n{text}"));
    assert_eq!(back.len(), 1, "{text}\n{back:#?}");
    // Written again unedited, the file is the same text.
    assert_eq!(save(&back, &b), text);
    (text, back.into_iter().next().unwrap())
}

#[test]
fn every_numeric_panel_writes_its_constraint_and_reads_back() {
    let cases = [
        (PanelType::MinimumClearance, "clearance (min 0.25mm)"),
        (PanelType::CreepageDistance, "creepage (min 0.25mm)"),
        (PanelType::MinimumConnectionWidth, "connection_width (min 0.25mm)"),
        (PanelType::CopperToHoleClearance, "hole_clearance (min 0.25mm)"),
        (PanelType::MinimumAnnularWidth, "annular_width (min 0.25mm)"),
        (PanelType::CopperToEdgeClearance, "edge_clearance (min 0.25mm)"),
        (PanelType::CourtyardClearance, "courtyard_clearance (min 0.25mm)"),
        (PanelType::PhysicalClearance, "physical_clearance (min 0.25mm)"),
        (PanelType::MinimumDrillSize, "hole_size (min 0.25mm)"),
        (PanelType::HoleToHoleDistance, "hole_to_hole (min 0.25mm)"),
        (PanelType::MinimumSoldermaskSliver, "solder_mask_sliver (min 0.25mm)"),
        (PanelType::SoldermaskExpansion, "solder_mask_expansion (min 0.25mm)"),
        (PanelType::SolderpasteExpansion, "solder_paste_abs_margin (min 0.25mm)"),
    ];
    for (panel, clause) in cases {
        let mut f = Fields::default();
        f.value = 0.25;
        let (text, back) = round_trip(&edit(panel, "r1", f));
        assert_eq!(text, format!("(version 1)\n(rule r1\n\t(constraint {clause})\n)\n"), "{panel:?}");
        assert_eq!((back.panel, back.fields.value, back.constraint_code.as_str()), (panel, 0.25, panel.code()), "{panel:?}");
    }
}

#[test]
fn the_counting_panels_write_whole_numbers() {
    let mut e = edit(PanelType::MaximumViaCount, "vias", Fields::default());
    e.value_text = Some("3".into());
    let (text, back) = round_trip(&e);
    assert_eq!(text, "(version 1)\n(rule vias\n\t(constraint via_count (max 3))\n)\n");
    assert_eq!((back.panel, back.fields.value), (PanelType::MaximumViaCount, 3.0));
    let mut e = edit(PanelType::MinimumThermalReliefSpokeCount, "spokes", Fields::default());
    e.value_text = Some("2".into());
    let (text, back) = round_trip(&e);
    assert_eq!(text, "(version 1)\n(rule spokes\n\t(constraint min_resolved_spokes 2)\n)\n");
    assert_eq!((back.panel, back.fields.value), (PanelType::MinimumThermalReliefSpokeCount, 2.0));
}

#[test]
fn the_silk_panels_write_their_layers_into_the_condition() {
    let mut f = Fields::default();
    f.value = 0.1;
    let mut e = edit(PanelType::SilkToSilkClearance, "silk", f.clone());
    e.layer = "F.SilkS".into();
    e.condition = "A.Type == 'Text'".into();
    let (text, back) = round_trip(&e);
    assert_eq!(text, "(version 1)\n(rule silk\n\t(constraint silk_clearance (min 0.1mm))\n\t(condition \"(L == 'F.SilkS') && A.Type == 'Text'\")\n)\n");
    assert_eq!((back.panel, back.layers.clone(), back.rule_condition.as_str()), (PanelType::SilkToSilkClearance, vec!["F.SilkS".to_string()], "A.Type == 'Text'"));
    assert_eq!(selected_token(back.panel, &back.layers, &back.layer_source, &board()), "F.SilkS");
    // Any layer: both sides.
    let (text, back) = round_trip(&edit(PanelType::SilkToSilkClearance, "silk", f.clone()));
    assert!(text.contains("(condition \"L == 'F.SilkS' || L == 'B.SilkS'\")"), "{text}");
    assert_eq!(back.layers, vec!["F.SilkS".to_string(), "B.SilkS".to_string()]);
    // Silk to soldermask: the mask layers, Top and Bottom.
    let mut e = edit(PanelType::SilkToSoldermaskClearance, "mask", f);
    e.layer = "bottom".into();
    let b = board();
    let mut entries = Vec::new();
    apply_edit(&mut entries, None, &e, &b).unwrap();
    // The panel's Bottom is B.Cu (`getSelectedLayers`), which is neither silk layer: the saver writes both masks.
    let text = save(&entries, &b);
    assert!(text.contains("(condition \"L == 'F.Mask' || L == 'B.Mask'\")"), "{text}");
    let back = load(&text, &b.env()).unwrap();
    assert_eq!(back[0].panel, PanelType::SilkToSoldermaskClearance);
}

#[test]
fn via_style_writes_the_via_type_into_the_condition_and_takes_it_out_again() {
    let mut f = Fields::default();
    (f.min_via_diameter, f.max_via_diameter, f.min_via_hole, f.max_via_hole, f.via_type) = (0.4, 0.8, 0.2, 0.4, ViaType::Micro);
    let mut e = edit(PanelType::ViaStyle, "uvia", f.clone());
    e.condition = "A.NetClass == 'HS'".into();
    e.layer = "outer".into();
    let (text, back) = round_trip(&e);
    assert_eq!(
        text,
        "(version 1)\n(rule uvia\n\t(layer outer)\n\t(constraint via_diameter (min 0.4mm) (max 0.8mm))\n\t(constraint hole_size (min 0.2mm) (max 0.4mm))\n\t(condition \"A.Via_Type == 'Micro' && A.NetClass == 'HS'\")\n)\n"
    );
    assert_eq!(back.panel, PanelType::ViaStyle);
    assert_eq!(back.fields, f);
    assert_eq!((back.rule_condition.as_str(), back.layer_source.as_str()), ("A.NetClass == 'HS'", "outer"));
    assert_eq!(selected_token(back.panel, &back.layers, &back.layer_source, &board()), "outer");
    // A diameter alone is one `via_diameter`, read back as Via style (KiCad: no tree node, the rule is lost).
    let mut dia = Fields::default();
    (dia.min_via_diameter, dia.max_via_diameter) = (0.3, 0.6);
    let (text, back) = round_trip(&edit(PanelType::ViaStyle, "dia", dia.clone()));
    assert_eq!(text, "(version 1)\n(rule dia\n\t(constraint via_diameter (min 0.3mm) (max 0.6mm))\n)\n");
    assert_eq!((back.panel, back.fields.clone()), (PanelType::ViaStyle, dia));
}

#[test]
fn text_height_routing_width_and_diff_pair_round_trip() {
    let mut f = Fields::default();
    (f.min_text_height, f.min_text_thickness) = (0.8, 0.12);
    let (text, back) = round_trip(&edit(PanelType::MinimumTextHeightAndThickness, "text", f.clone()));
    assert!(text.contains("\t(constraint text_height (min 0.8mm))\n\t(constraint text_thickness (min 0.12mm))\n"), "{text}");
    assert_eq!((back.panel, back.fields.min_text_height, back.fields.min_text_thickness), (PanelType::MinimumTextHeightAndThickness, 0.8, 0.12));

    let mut f = Fields::default();
    (f.opt_width, f.width_tolerance) = (0.3, 0.05);
    let mut e = edit(PanelType::RoutingWidth, "width", f);
    e.layer = "In1.Cu".into();
    let (text, back) = round_trip(&e);
    assert!(text.contains("\t(layer \"In1.Cu\")\n\t(constraint track_width (min 0.25mm) (opt 0.3mm) (max 0.35mm))\n"), "{text}");
    assert_eq!(back.panel, PanelType::RoutingWidth);
    assert!((back.fields.opt_width - 0.3).abs() < 1e-9 && (back.fields.width_tolerance - 0.05).abs() < 1e-9, "{back:?}");
    assert_eq!(selected_token(back.panel, &back.layers, &back.layer_source, &board()), "In1.Cu");

    let mut f = Fields::default();
    (f.opt_width, f.width_tolerance, f.opt_gap, f.gap_tolerance, f.max_uncoupled) = (0.2, 0.02, 0.15, 0.01, 5.0);
    let (text, back) = round_trip(&edit(PanelType::RoutingDiffPair, "dp", f));
    assert!(
        text.contains("\t(constraint track_width (min 0.18mm) (opt 0.2mm) (max 0.22mm))\n\t(constraint diff_pair_gap (min 0.14mm) (opt 0.15mm) (max 0.16mm))\n\t(constraint diff_pair_uncoupled (max 5mm))\n"),
        "{text}"
    );
    assert_eq!(back.panel, PanelType::RoutingDiffPair);
    assert!((back.fields.gap_tolerance - 0.01).abs() < 1e-9 && back.fields.max_uncoupled == 5.0, "{back:?}");
}

#[test]
fn lengths_round_trip() {
    let mut f = Fields::default();
    (f.opt_length, f.tolerance) = (50.0, 0.5);
    let (text, back) = round_trip(&edit(PanelType::AbsoluteLength, "len", f.clone()));
    assert!(text.contains("\t(constraint length (min 49.5mm) (opt 50mm) (max 50.5mm))\n"), "{text}");
    assert_eq!((back.panel, back.fields.opt_length, back.fields.tolerance), (PanelType::AbsoluteLength, 50.0, 0.5));

    (f.max_skew, f.within_diff_pairs) = (0.1, true);
    let (text, back) = round_trip(&edit(PanelType::MatchedLengthDiffPair, "matched", f.clone()));
    assert!(text.contains("\t(constraint length (min 49.5mm) (opt 50mm) (max 50.5mm))\n\t(constraint skew (max 0.1mm) (within_diff_pairs))\n"), "{text}");
    assert_eq!(back.panel, PanelType::MatchedLengthDiffPair);
    assert_eq!((back.fields.opt_length, back.fields.tolerance, back.fields.max_skew, back.fields.within_diff_pairs), (50.0, 0.5, 0.1, true));
    // A skew alone is the matched-length panel too.
    let mut skew = Fields::default();
    skew.max_skew = 0.2;
    let (text, back) = round_trip(&edit(PanelType::MatchedLengthDiffPair, "skew", skew));
    assert_eq!(text, "(version 1)\n(rule skew\n\t(constraint skew (max 0.2mm))\n)\n");
    assert_eq!((back.panel, back.fields.max_skew, back.fields.within_diff_pairs), (PanelType::MatchedLengthDiffPair, 0.2, false));
}

#[test]
fn the_footprint_panels_round_trip() {
    let mut f = Fields::default();
    f.top_layer = true;
    let (text, back) = round_trip(&edit(PanelType::PermittedLayers, "top_only", f));
    assert!(text.contains("\t(constraint assertion \"A.Layer == 'F.Cu'\")\n"), "{text}");
    assert_eq!((back.panel, back.fields.top_layer, back.fields.bottom_layer), (PanelType::PermittedLayers, true, false));

    let mut f = Fields::default();
    (f.deg0, f.deg180) = (true, true);
    let mut e = edit(PanelType::AllowedOrientation, "orient", f);
    e.layer = "top".into();
    let (text, back) = round_trip(&e);
    assert!(text.contains("\t(layer \"F.Cu\")\n\t(constraint assertion \"A.Orientation == 0 deg || A.Orientation == 180 deg\")\n"), "{text}");
    assert_eq!((back.panel, back.fields.deg0, back.fields.deg90, back.fields.deg180, back.fields.all_degrees), (PanelType::AllowedOrientation, true, false, true, false));
    assert_eq!(selected_token(back.panel, &back.layers, &back.layer_source, &board()), "top");

    let mut f = Fields::default();
    (f.micro_vias, f.blind_vias) = (false, false);
    let (text, back) = round_trip(&edit(PanelType::ViasUnderSmd, "smd", f));
    assert_eq!(text, "(version 1)\n(rule smd\n\t(constraint disallow through_via buried_via)\n\t(condition \"A.Pad_Type == 'SMD'\")\n)\n");
    assert_eq!(back.panel, PanelType::ViasUnderSmd);
    assert_eq!((back.fields.through_vias, back.fields.micro_vias, back.fields.blind_vias, back.fields.buried_vias, back.rule_condition.as_str()), (true, false, false, true, ""));
    // All four: `disallow via`.
    let (text, _) = round_trip(&edit(PanelType::ViasUnderSmd, "smd", Fields::default()));
    assert!(text.contains("(constraint disallow via)"), "{text}");
}

#[test]
fn a_custom_rule_keeps_its_body() {
    let mut f = Fields::default();
    f.rule_text = "\t(constraint clearance (min 0.3mm))\n\t(condition \"A.NetClass == 'HV'\")\n".into();
    let mut e = edit(PanelType::CustomRule, "HV rule", f);
    e.comment = "high voltage".into();
    let (text, back) = round_trip(&e);
    assert_eq!(text, "(version 1)\n(rule \"HV rule\"\n\t# high voltage\n\t(constraint clearance (min 0.3mm))\n\t(condition \"A.NetClass == 'HV'\"))\n");
    // It reads back as the clearance panel it is, with the comment.
    assert_eq!((back.panel, back.fields.value, back.comment.as_str(), back.rule_condition.as_str()), (PanelType::MinimumClearance, 0.3, "high voltage", "A.NetClass == 'HV'"));
    // A rule no panel can show stays a Custom rule.
    let mut f = Fields::default();
    f.rule_text = "\t(constraint via_dangling)".into();
    let (text, back) = round_trip(&edit(PanelType::CustomRule, "dangling", f));
    assert_eq!(text, "(version 1)\n(rule \"dangling\"\n\t(constraint via_dangling))\n");
    assert_eq!((back.panel, back.fields.rule_text.as_str()), (PanelType::CustomRule, "(constraint via_dangling)"));
}

#[test]
fn a_rule_name_is_written_bare_or_quoted_as_kicad_writes_it() {
    assert_eq!(format_rule_name("rule_1.a-b"), "rule_1.a-b");
    assert_eq!(format_rule_name("1st"), "\"1st\"");
    assert_eq!(format_rule_name("two words"), "\"two words\"");
    assert_eq!(format_rule_name("say \"hi\""), "\"say {dblquote}hi{dblquote}\"");
    assert_eq!(format_rule_name(""), "\"\"");
    assert_eq!(format_double(0.1 + 0.2), "0.3");
    assert_eq!(format_double(2.0), "2");
    assert_eq!(format_double(-0.05), "-0.05");
}

#[test]
fn a_severity_and_a_comment_are_written_with_the_rule() {
    let b = board();
    let mut entries = load("(version 1)\n(rule w\n\t(constraint clearance (min 0.2mm))\n\t(severity warning))\n", &b.env()).unwrap();
    assert_eq!(entries[0].severity, RuleSeverity::Warning);
    let mut f = Fields::default();
    f.value = 0.3;
    let mut e = edit(PanelType::MinimumClearance, "w", f);
    e.comment = "line one\nline two".into();
    apply_edit(&mut entries, Some(0), &e, &b).unwrap();
    // `generateRuleText` puts the severity before the rule's last parenthesis.
    assert_eq!(save(&entries, &b), "(version 1)\n(rule w\n\t# line one\n\t# line two\n\t(constraint clearance (min 0.3mm))\n\n\t(severity warning))\n");
    let back = load(&save(&entries, &b), &b.env()).unwrap();
    assert_eq!((back[0].severity, back[0].comment.as_str(), back[0].fields.value), (RuleSeverity::Warning, "line one\nline two", 0.3));
}

#[test]
fn a_rule_with_two_panels_is_two_entries_written_back_as_one_rule() {
    let b = board();
    let text = "(version 1)\n\n(rule myrule\n # just something to see what happens\n (layer inner)\n (condition \"A.Name == 'test'\")\n (constraint annular_width (min 3mm))\n (constraint disallow track))\n";
    let entries = load(text, &b.env()).unwrap();
    assert_eq!(entries.iter().map(|e| e.panel).collect::<Vec<_>>(), vec![PanelType::MinimumAnnularWidth, PanelType::CustomRule]);
    assert_eq!(entries[0].comment, "just something to see what happens");
    assert_eq!((entries[0].layer_source.as_str(), entries[0].layers.len()), ("inner", 30));
    assert_eq!(selected_token(entries[0].panel, &entries[0].layers, &entries[0].layer_source, &b), "inner");
    assert!(!entries[0].original_text.is_empty() && entries[1].original_text.is_empty());
    // Unedited: the rule's own text.
    assert_eq!(save(&entries, &b), "(version 1)\n(rule myrule\n # just something to see what happens\n (layer inner)\n (condition \"A.Name == 'test'\")\n (constraint annular_width (min 3mm))\n (constraint disallow track))\n");
    // The annular width edited: the two panels merge into one rule, and the custom part's constraints are not written (KiCad's
    // `generateMergedRuleText`: a Custom rule's data has no constraint clauses).
    let mut entries = entries;
    let mut f = Fields::default();
    f.value = 2.0;
    let mut e = edit(PanelType::MinimumAnnularWidth, "myrule", f);
    e.layer = "inner".into();
    e.condition = "A.Name == 'test'".into();
    // `validateRuleName`: the other panel of the same rule has the name, so KiCad refuses it.
    let err = apply_edit(&mut entries, Some(0), &e, &b).unwrap_err();
    assert_eq!(err, "1. Rule name must be unique.\n");
}

/// The claims of one priority are in the order KiCad's `std::sort` leaves them in (libstdc++, KiCad's Linux build): the panels of a rule with two
/// constraints of one tier come in that order, and so do the clauses of the rule when it is written merged.
#[test]
fn claims_of_one_priority_are_in_the_order_kicads_sort_leaves_them() {
    let order: Vec<PanelType> = claims().iter().map(|c| c.panel).collect();
    use PanelType as P;
    assert_eq!(
        order,
        vec![
            P::RoutingDiffPair,
            P::ViaStyle,
            P::MinimumTextHeightAndThickness,
            P::RoutingWidth,
            P::MatchedLengthDiffPair,
            P::AbsoluteLength,
            P::MatchedLengthDiffPair,
            P::PermittedLayers,
            P::AllowedOrientation,
            P::SilkToSilkClearance,
            P::CreepageDistance,
            P::PhysicalClearance,
            P::CourtyardClearance,
            P::HoleToHoleDistance,
            P::CopperToHoleClearance,
            P::MinimumClearance,
            P::CopperToEdgeClearance,
            P::MinimumSoldermaskSliver,
            P::MaximumViaCount,
            P::MinimumThermalReliefSpokeCount,
            P::SolderpasteExpansion,
            P::SoldermaskExpansion,
            P::MinimumConnectionWidth,
            P::MinimumAnnularWidth,
            P::MinimumViaDiameter,
            P::HoleSize,
            P::RoutingDiffPair,
            P::RoutingDiffPair,
            P::ViasUnderSmd,
        ]
    );
    let b = board();
    let entries = load("(version 1)\n(rule r (constraint clearance (min 0.2mm)) (constraint creepage (min 1mm)))", &b.env()).unwrap();
    assert_eq!(entries.iter().map(|e| e.panel).collect::<Vec<_>>(), vec![P::CreepageDistance, P::MinimumClearance]);
}

#[test]
fn the_matcher_claims_by_priority() {
    let b = board();
    let one = |t: &str| load(&format!("(version 1)\n(rule r {t})"), &b.env()).unwrap().into_iter().map(|e| e.panel).collect::<Vec<_>>();
    use PanelType as P;
    assert_eq!(one("(constraint track_width (min 0.1mm)) (constraint diff_pair_gap (min 0.1mm)) (constraint clearance (min 0.2mm))"), vec![P::RoutingDiffPair, P::MinimumClearance]);
    assert_eq!(one("(constraint via_diameter (min 0.4mm)) (constraint hole_size (min 0.2mm))"), vec![P::ViaStyle]);
    assert_eq!(one("(constraint length (min 1mm)) (constraint skew (max 1mm))"), vec![P::MatchedLengthDiffPair]);
    assert_eq!(one("(constraint length (min 1mm) (opt 2mm) (max 3mm))"), vec![P::AbsoluteLength]);
    assert_eq!(one("(constraint diff_pair_uncoupled (max 1mm))"), vec![P::RoutingDiffPair]);
    assert_eq!(one("(constraint via_count (max 2))"), vec![P::MaximumViaCount]);
    assert_eq!(one("(constraint min_resolved_spokes 2)"), vec![P::MinimumThermalReliefSpokeCount]);
    assert_eq!(one("(constraint zone_connection solid)"), vec![P::CustomRule]);
    // `disallow` is Vias under SMD only with Pad_Type == 'SMD' in the condition.
    assert_eq!(one("(constraint disallow via)"), vec![P::CustomRule]);
    assert_eq!(one("(constraint disallow via) (condition \"A.Pad_Type == 'SMD'\")"), vec![P::ViasUnderSmd]);
    // An asymmetric min/opt/max is a Custom rule (`isSymmetricMinOptMax`).
    assert_eq!(one("(constraint track_width (min 0.1mm) (opt 0.2mm) (max 0.5mm))"), vec![P::CustomRule]);
    // An orientation assertion with other angles, and a layer assertion on inner layers, are Custom rules.
    assert_eq!(one("(constraint assertion \"A.Orientation == 45 deg\")"), vec![P::CustomRule]);
    assert_eq!(one("(constraint assertion \"A.Orientation == 90 deg\")"), vec![P::AllowedOrientation]);
    assert_eq!(one("(constraint assertion \"A.Layer == 'In1.Cu'\")"), vec![P::CustomRule]);
    // Silk: the mask in the condition makes it silk to soldermask, any other condition a Custom rule.
    assert_eq!(one("(constraint silk_clearance (min 0.1mm)) (condition \"L == 'B.Mask'\")"), vec![P::SilkToSoldermaskClearance]);
    assert_eq!(one("(constraint silk_clearance (min 0.1mm)) (condition \"A.Type == 'Text'\")"), vec![P::CustomRule]);
    assert_eq!(one("(constraint silk_clearance (min 0.1mm))"), vec![P::SilkToSilkClearance]);
    // Not KiCad's: hole_size and via_diameter alone have no tree node; they go to the panels that write them rather than being lost.
    assert_eq!(one("(constraint hole (max 2mm))"), vec![P::CustomRule]);
    assert_eq!(one("(constraint hole_size (min 0.3mm))"), vec![P::MinimumDrillSize]);
    assert_eq!(one("(constraint via_diameter (min 0.3mm))"), vec![P::ViaStyle]);
    // No constraint at all: a Custom rule.
    assert_eq!(one("(condition \"A.Type == 'Pad'\")"), vec![P::CustomRule]);
}

#[test]
fn the_loader_reads_the_values_kicad_reads() {
    let b = board();
    let e = &load("(version 1)\n(rule l (constraint length (min 13.3349mm) (max 13.3551mm)))", &b.env()).unwrap()[0];
    // No opt: `Opt()` is the minimum, and the tolerance half the range (KiCad's reading, kept).
    assert_eq!(e.panel, PanelType::AbsoluteLength);
    assert!((e.fields.opt_length - 13.3349).abs() < 1e-9 && (e.fields.tolerance - 0.0101).abs() < 1e-9, "{e:?}");
    let e = &load("(version 1)\n(rule v (constraint via_diameter (min 0.3mm)) (constraint hole_size (max 0.2mm)) (condition \"A.Via_Type == 'Blind' && A.NetName == 'X'\"))", &b.env()).unwrap()[0];
    // A missing min is 0, a missing max `INT_MAX` nanometres.
    assert_eq!((e.fields.min_via_diameter, e.fields.max_via_diameter, e.fields.min_via_hole, e.fields.max_via_hole), (0.3, 2147.483647, 0.0, 0.2));
    assert_eq!((e.fields.via_type, e.rule_condition.as_str()), (ViaType::Blind, "A.NetName == 'X'"));
    let e = &load("(version 1)\n(rule s (constraint disallow via) (condition \"A.Pad_Type == 'SMD' && B.Type == 'Via'\"))", &b.env()).unwrap()[0];
    assert_eq!((e.panel, e.rule_condition.as_str()), (PanelType::ViasUnderSmd, "B.Type == 'Via'"));
    assert_eq!(clean_stripped_condition("( ) && A.x || "), "( ) && A.x");
    assert_eq!(clean_stripped_condition(" && A.x && && B.y"), "A.x && B.y");
    assert_eq!(clean_stripped_condition("()&& A.x"), "A.x");
}

#[test]
fn extracting_a_rule_finds_it_by_name_and_keeps_two_of_one_name_apart() {
    let text = "(version 1)\n(rule \"Clearance for BGA\" (constraint clearance (min 1mm)))\n(rule Clearance\n (constraint clearance (min 2mm)))\n(rule Clearance (constraint clearance (min 3mm)))";
    assert_eq!(extract_rule_text(text, "Clearance", None), "(rule Clearance\n (constraint clearance (min 2mm)))");
    assert_eq!(extract_rule_text(text, "Clearance", Some(0)), "(rule Clearance\n (constraint clearance (min 2mm)))");
    assert_eq!(extract_rule_text(text, "Clearance", Some(1)), "(rule Clearance (constraint clearance (min 3mm)))");
    assert_eq!(extract_rule_text(text, "Clearance for BGA", None), "(rule \"Clearance for BGA\" (constraint clearance (min 1mm)))");
    assert_eq!(extract_rule_text(text, "Clearance", Some(2)), "");
    // KiCad's search takes a quoted name before an unquoted one; two rules of one name, one quoted, are found each in its place.
    let mixed = "(version 1)\n(rule X (constraint clearance (min 1mm)))\n(rule \"X\" (condition \"A.Type == 'Pad'\") (constraint clearance (min 2mm)))\n";
    assert_eq!(extract_rule_text(mixed, "X", None), "(rule \"X\" (condition \"A.Type == 'Pad'\") (constraint clearance (min 2mm)))");
    assert_eq!(extract_rule_text(mixed, "X", Some(0)), "(rule X (constraint clearance (min 1mm)))");
    let entries = load(mixed, &board().env()).unwrap();
    assert_eq!(save(&entries, &board()), mixed);
    assert_eq!(extract_rule_body("(rule \"x\"\n\t(constraint clearance (min 1mm))\n)"), "(constraint clearance (min 1mm))");
    assert_eq!(extract_rule_comment("(rule x\n  # one\n\t#two\n (constraint ..))"), "one\ntwo");
    let b = board();
    let entries = load(text, &b.env()).unwrap();
    let saved = save(&entries, &b);
    assert_eq!(rules_of(&saved), rules_of(text));
}

#[test]
fn the_checks_say_what_kicad_says() {
    let b = board();
    let check = |e: &RuleEdit| check_edit(&[], None, e, &b).unwrap();
    let mut e = edit(PanelType::MinimumClearance, "r", Fields::default());
    e.value_text = Some("".into());
    assert_eq!(check(&e).message, "1. Value is required.\n");
    e.value_text = Some("abc".into());
    assert_eq!(check(&e).message, "1. Value must be a number.\n");
    e.value_text = Some("-1".into());
    assert_eq!(check(&e).message, "1. Value must be greater than 0.\n");
    e.value_text = Some("0,2".into());
    let ok = check(&e);
    assert!(ok.valid && ok.generated.contains("(min 0.2mm)"), "{ok:?}");
    let mut e = edit(PanelType::MaximumViaCount, "r", Fields::default());
    e.value_text = Some("2.5".into());
    assert_eq!(check(&e).message, "1. Value must be a whole number.\n");
    // The data's own checks, numbered from 1 after the panel's.
    let mut f = Fields::default();
    f.min_via_diameter = 0.8;
    f.max_via_diameter = 0.4;
    let mut e = edit(PanelType::ViaStyle, "r", f);
    e.taken_names = vec!["r".into()];
    assert_eq!(check(&e).message, "1. Rule name must be unique.\n1. Minimum Via Diameter cannot be greater than Maximum Via Diameter\n");
    let f = Fields::default();
    assert_eq!(check(&edit(PanelType::ViaStyle, "r", f.clone())).message, "1. At least one constraint must be specified\n");
    assert_eq!(check(&edit(PanelType::MinimumTextHeightAndThickness, "r", f.clone())).message, "1. Minimum Text Height must be greater than 0\n2. Minimum Text Thickness must be greater than 0\n");
    assert_eq!(check(&edit(PanelType::RoutingWidth, "r", f.clone())).message, "1. Optimum Width must be greater than 0\n2. Width Tolerance must be less than Optimum Width\n");
    assert_eq!(check(&edit(PanelType::RoutingDiffPair, "r", f.clone())).message, "1. At least one constraint must be specified\n");
    assert_eq!(check(&edit(PanelType::AbsoluteLength, "r", f.clone())).message, "1. Optimum Length must be greater than 0\n");
    let mut long = f.clone();
    (long.opt_length, long.tolerance) = (1.0, 2.0);
    assert_eq!(check(&edit(PanelType::AbsoluteLength, "r", long)).message, "1. Tolerance is too large: resulting minimum length is not positive\n");
    assert_eq!(check(&edit(PanelType::MatchedLengthDiffPair, "r", f.clone())).message, "1. At least one constraint must be defined\n");
    assert_eq!(check(&edit(PanelType::PermittedLayers, "r", f.clone())).message, "1. At least one layer must be selected\n");
    assert_eq!(check(&edit(PanelType::AllowedOrientation, "r", f.clone())).message, "1. At least one orientation must be selected\n");
    let mut none = f.clone();
    (none.through_vias, none.micro_vias, none.blind_vias, none.buried_vias) = (false, false, false, false);
    assert_eq!(check(&edit(PanelType::ViasUnderSmd, "r", none)).message, "1. At least one via type must be selected\n");
    // A Custom rule's panel checks nothing.
    assert!(check(&edit(PanelType::CustomRule, "r", f.clone())).valid);
    // A layer the selector does not offer.
    let mut e = edit(PanelType::MinimumClearance, "r", Fields { value: 1.0, ..Default::default() });
    e.layer = "F.SilkS".into();
    assert_eq!(check(&e).message, "1. Layer selection is required.\n");
}

#[test]
fn moving_deleting_and_naming() {
    let b = board();
    let text = "(version 1)\n(rule a (constraint clearance (min 1mm)))\n(rule t (constraint track_width (min 1mm)))\n(rule c (constraint clearance (min 2mm)))\n";
    let mut entries = load(text, &b.env()).unwrap();
    // Move Down: `a` changes places with the next clearance rule, `c`; the track width rule between them stays.
    assert_eq!(move_entry(&mut entries, 0, false), Some(2));
    assert_eq!(entries.iter().map(|e| e.rule_name.as_str()).collect::<Vec<_>>(), vec!["c", "t", "a"]);
    assert_eq!(move_entry(&mut entries, 1, true), None);
    assert_eq!(save(&entries, &b), "(version 1)\n(rule c (constraint clearance (min 2mm)))\n(rule t (constraint track_width (min 1mm)))\n(rule a (constraint clearance (min 1mm)))\n");
    delete_entry(&mut entries, 1).unwrap();
    assert_eq!(save(&entries, &b), "(version 1)\n(rule c (constraint clearance (min 2mm)))\n(rule a (constraint clearance (min 1mm)))\n");
    assert!(delete_entry(&mut entries, 5).is_err());
    let taken: Vec<String> = vec!["Minimum clearance".into(), "Minimum clearance 1".into()];
    assert_eq!(unique_rule_name("Minimum clearance", &taken), "Minimum clearance 2");
    assert_eq!(duplicate_base_name("HV 3"), "HV");
    assert_eq!(duplicate_base_name("HV x"), "HV x");
    assert_eq!(duplicate_base_name("HV"), "HV");
}

#[test]
fn the_layer_selector_offers_what_kicad_offers() {
    let b = board();
    let tokens = |c: LayerCategory| layer_options(c, &b).into_iter().map(|o| o.token).collect::<Vec<_>>();
    assert_eq!(tokens(LayerCategory::CopperOnly), vec!["", "outer", "inner", "F.Cu", "In1.Cu", "In2.Cu", "B.Cu"]);
    assert_eq!(tokens(LayerCategory::TopBottomAny), vec!["", "top", "bottom"]);
    assert_eq!(tokens(LayerCategory::SilkscreenOnly), vec!["", "F.SilkS", "B.SilkS"]);
    assert_eq!(tokens(LayerCategory::NoLayerSelector), vec![""]);
    assert_eq!(layer_options(LayerCategory::SilkscreenOnly, &b)[1].label, "F.Silkscreen");
    assert_eq!(layer_clause(PanelType::CourtyardClearance, "bottom", &b), "(layer \"B.CrtYd\")");
    assert_eq!(layer_clause(PanelType::SilkToSoldermaskClearance, "top", &b), "(condition \"A.Layer == 'F.SilkS' && B.Layer == 'F.Mask'\")");
    assert_eq!(layer_clause(PanelType::PhysicalClearance, "F.SilkS", &b), "(layer \"F.Silkscreen\")");
    assert_eq!(selected_layer_source(PanelType::CourtyardClearance, "top", &b), "F.CrtYd");
    // A courtyard rule read back from `(layer "F.CrtYd")` shows Top.
    let e = &load("(version 1)\n(rule c (layer \"F.CrtYd\") (constraint courtyard_clearance (min 1mm)))", &b.env()).unwrap()[0];
    assert_eq!(selected_token(e.panel, &e.layers, &e.layer_source, &b), "top");
}

#[test]
fn a_file_that_does_not_parse_is_not_opened() {
    let err = load("(version 1)\n(rule x (constraint clearance (min 1mm))", &env()).unwrap_err();
    assert!(err.message.starts_with("ERROR:"), "{err:?}");
    assert_eq!(check_custom_syntax("x", "  \n", &env()), "No rule text to check.");
}

/// Check Syntax on a Custom rule (`DRC_RE_CUSTOM_RULE_PANEL::onCheckSyntax`): the parser with a reporter tells every message, each where it is, and
/// goes on; a text with nothing wrong gets the reporter's "No errors found." under "Syntax check passed with warnings:", as KiCad's tip shows it.
#[test]
fn check_syntax_says_what_kicads_reporter_is_told() {
    let e = env();
    assert_eq!(check_custom_syntax("x", "(constraint clearance (min 1mm))", &e), "Syntax check passed with warnings:\nNo errors found.\n");
    assert_eq!(
        check_custom_syntax("x", "(constraint hole (min 1mm))", &e),
        "Syntax check passed with warnings:\nThe 'hole' keyword has been deprecated.  Please use 'hole_size' instead.\n"
    );
    // An unknown constraint: the error at its place (line 3, `CurOffset()` 13), the rest of the constraint skipped, and the next error told too.
    let two = check_custom_syntax("x", "(constraint nonsense (min 1mm))\n(constraint clearance (min 1mm)) (severity bogus)", &e);
    assert_eq!(
        two,
        "ERROR: <a href='3:13'>Unrecognized item 'nonsense'.</a> Expected assertion, clearance, hole_clearance, edge_clearance, physical_clearance, \
         physical_hole_clearance, courtyard_clearance, silk_clearance, hole_size, hole_to_hole, track_width, track_angle, track_segment_length, \
         annular_width, disallow, zone_connection, thermal_relief_gap, thermal_spoke_width, min_resolved_spokes, solder_mask_expansion, \
         solder_mask_sliver, solder_paste_abs_margin, solder_paste_rel_margin, length, skew, via_count, via_dangling, via_diameter, diff_pair_gap, \
         diff_pair_uncoupled or bridged_mask.\nERROR: <a href='4:44'>Unrecognized item 'bogus'.</a> Expected ignore, warning, error, or exclusion.\n\
         ERROR: <a href='5:1'>Missing ')'.</a>\n"
    );
    // What the lexer throws ends it: KiCad's catch words it.
    assert!(check_custom_syntax("x", "(condition \"A.Type == 'Pad')", &e).starts_with("ERROR at line 3, column "), "{}", check_custom_syntax("x", "(condition \"A.Type == 'Pad')", &e));
    // A condition the compiler does not understand: its messages, then the parser's.
    let bad = check_custom_syntax("x", "(condition \"A.Type == \")", &e);
    assert!(bad.contains("</a>") && bad.ends_with("ERROR: <a href='3:12'>Could not parse expression 'A.Type == '.</a>\n"), "{bad}");
}

/// R6-D5 (`PANEL_SETUP_RULES::OnCompile`): Board Setup > Custom Rules' check is the parser's reporter -- "No errors found." for a good text, each error at
/// its line and offset for a bad one -- and a lexer `PARSE_ERROR` as "ERROR: <a href='line:offset'>problem</a>".
#[test]
fn custom_rules_check_says_what_kicads_compile_says() {
    let e = env();
    let texts = |text: &str| super::compile_report(text, &e).into_iter().map(|r| r.text).collect::<Vec<_>>();
    assert_eq!(texts("(version 1)\n(rule a (constraint clearance (min 1mm)))"), ["No errors found."]);
    let missing = texts("(version 1)\n(rule a (constraint clearance (min 1mm))");
    assert_eq!(missing, ["ERROR: <a href='3:1'>Missing ')'.</a>"], "{missing:?}");
    let lexer = texts("(version 1)\n(rule a (condition \"A.Type == 'Pad'))");
    assert!(lexer.len() == 1 && lexer[0].starts_with("ERROR: <a href='2:") && lexer[0].ends_with("</a>"), "{lexer:?}");
}

/// KiCad's own rule files (`qa/data/pcbnew/*.kicad_dru`): read by the editor and written back unedited, every rule is the same rule -- the same
/// name, condition, layer, severity and constraints, its own text kept verbatim -- and writing the result again changes nothing.
#[test]
fn kicads_qa_rule_files_read_back_unchanged() {
    let Some(src) = std::env::var_os("KICAD_SRC_DIR") else {
        eprintln!("skipped: KICAD_SRC_DIR is not set");
        return;
    };
    let dir = std::path::Path::new(&src).join("qa/data/pcbnew");
    let mut files: Vec<_> = std::fs::read_dir(&dir).unwrap().flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "kicad_dru")).collect();
    files.retain(|p| !p.file_name().unwrap().to_string_lossy().starts_with("._"));
    files.sort();
    assert!(files.len() >= 20, "{} rule files in {}", files.len(), dir.display());
    let b = board();
    for path in files {
        let text = std::fs::read_to_string(&path).unwrap();
        let name = path.file_name().unwrap().to_string_lossy().to_string();
        let entries = load(&text, &b.env()).unwrap_or_else(|e| panic!("{name}: {e}"));
        let saved = save(&entries, &b);
        assert_eq!(rules_of(&saved), rules_of(&text), "{name}:\n{saved}");
        for (i, e) in entries.iter().enumerate().filter(|(_, e)| !e.original_text.is_empty()) {
            assert!(saved.contains(&e.original_text), "{name}: rule {i} is not kept verbatim");
        }
        let again = load(&saved, &b.env()).unwrap();
        assert_eq!(save(&again, &b), saved, "{name}");
    }
}
