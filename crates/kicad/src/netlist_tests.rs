//! Tests for `netlist.rs` (included there with `#[path]`).

use super::*;
use crate::sch_plot::tests::{hierarchical, ldo_model, single, well_formed};
use crate::sexpr::{self, Sexpr};
use std::cmp::Ordering::*;

fn meta() -> NetlistMeta {
    NetlistMeta { source: "/proj/ldo.kicad_sch".into(), date: "2026-10-02T12:34:56".into(), tool: "agentic-eda 0.1".into(), project: "ldo".into(), fallback_title: "LDO test".into(), symbol_library_root: None }
}

/// `(tag (k "v") ...)` -> value of the attribute-style child `k`.
fn attr<'a>(list: &'a [Sexpr], key: &str) -> Option<&'a str> {
    sexpr::find(list, key).and_then(|l| sexpr::txt(l, 1))
}

struct ParsedNode {
    reference: String,
    pin: String,
    pinfunction: Option<String>,
    pintype: String,
}

struct ParsedNet {
    code: String,
    name: String,
    class: String,
    nodes: Vec<ParsedNode>,
}

/// The netlist read back through the repo's own s-expression parser.
fn parse_nets(text: &str) -> Vec<ParsedNet> {
    let root = sexpr::parse(text).expect("the .net file parses");
    let items = root.as_list().unwrap();
    assert_eq!(sexpr::tag(items), Some("export"));
    assert_eq!(attr(items, "version"), Some("E"));
    let nets = sexpr::find(items, "nets").expect("(nets ...)");
    sexpr::find_all(nets, "net")
        .map(|n| ParsedNet {
            code: attr(n, "code").unwrap().to_string(),
            name: attr(n, "name").unwrap().to_string(),
            class: attr(n, "class").unwrap().to_string(),
            nodes: sexpr::find_all(n, "node")
                .map(|nd| ParsedNode { reference: attr(nd, "ref").unwrap().to_string(), pin: attr(nd, "pin").unwrap().to_string(), pinfunction: attr(nd, "pinfunction").map(str::to_string), pintype: attr(nd, "pintype").unwrap().to_string() })
                .collect(),
        })
        .collect()
}

fn comp_refs(text: &str) -> Vec<String> {
    let root = sexpr::parse(text).unwrap();
    let items = root.as_list().unwrap();
    let comps = sexpr::find(items, "components").unwrap();
    sexpr::find_all(comps, "comp").map(|c| attr(c, "ref").unwrap().to_string()).collect()
}

#[test]
fn kicad_netlist_round_trips_through_the_sexpr_parser_with_known_connectivity() {
    let (design, model) = single();
    let text = export_netlist(&design, &model, NetlistFormat::Kicad, &meta()).unwrap();
    assert!(text.starts_with("(export\n\t(version \"E\")\n\t(design\n"), "prettified like KICAD_FORMAT::Prettify: {}", &text[..60]);
    assert!(text.ends_with(")\n"));

    // Components: ordered by reference (StrNumCmp), `#` references absent.
    assert_eq!(comp_refs(&text), ["CIN", "COUT", "U1"]);

    // Nets: sorted by name, codes from 1 in that order, nodes by ref then pin.
    let nets = parse_nets(&text);
    let summary: Vec<(String, String, Vec<String>)> = nets.iter().map(|n| (n.code.clone(), n.name.clone(), n.nodes.iter().map(|x| format!("{}.{}", x.reference, x.pin)).collect())).collect();
    assert_eq!(
        summary,
        vec![
            ("1".into(), "GND".into(), vec!["CIN.2".into(), "COUT.2".into(), "U1.2".into()]),
            ("2".into(), "VIN".into(), vec!["CIN.1".into(), "U1.1".into(), "U1.3".into()]),
            ("3".into(), "VOUT".into(), vec!["COUT.1".into(), "U1.4".into()]),
            // the NC pin no net names: `GetDefaultNetName`'s `unconnected-(...)`
            ("4".into(), "unconnected-(U1-NC-Pad5)".into(), vec!["U1.5".into()]),
        ]
    );
    assert!(nets.iter().all(|n| n.class == "Default"));

    // pinfunction = "<name>_<number>" when the pin has a name, absent for "~"; pintype is the canonical type.
    let vin = &nets[1];
    assert_eq!(vin.nodes[0].pinfunction, None);
    assert_eq!(vin.nodes[0].pintype, "passive");
    assert_eq!(vin.nodes[1].pinfunction.as_deref(), Some("VIN_1"));
    assert_eq!(vin.nodes[1].pintype, "power_in");
    assert_eq!(vin.nodes[2].pintype, "bidirectional");
    // a no-connect pin alone on its net is flagged
    assert_eq!(nets[3].nodes[0].pintype, "no_connect+no_connect");

    // total node count = every wired pin once
    assert_eq!(nets.iter().map(|n| n.nodes.len()).sum::<usize>(), 9);
}

#[test]
fn netlist_is_deterministic_and_matches_the_one_netlist_rule() {
    let (design, mut model) = single();
    let a = export_netlist(&design, &model, NetlistFormat::Kicad, &meta()).unwrap();
    assert_eq!(a, export_netlist(&design, &model, NetlistFormat::Kicad, &meta()).unwrap());
    // Editing the one netlist (what `design.nets` does once the schematic is edited) changes the export.
    model.nets.retain(|n| n.name != "VOUT");
    model.nets.push(eda_model::Net { name: "V_OUT_RENAMED".into(), pins: vec!["U1.4".into(), "COUT.1".into()] });
    let b = export_netlist(&design, &model, NetlistFormat::Kicad, &meta()).unwrap();
    assert!(b.contains("V_OUT_RENAMED"));
    // "V_OUT_RENAMED" sorts after "VIN" but before "unconnected-...": codes stay gap-free here.
    let names: Vec<String> = parse_nets(&b).into_iter().map(|n| format!("{}:{}", n.code, n.name)).collect();
    assert_eq!(names, ["1:GND", "2:VIN", "3:V_OUT_RENAMED", "4:unconnected-(U1-NC-Pad5)"]);
}

#[test]
fn xml_netlist_has_the_same_structure() {
    let (design, model) = single();
    let xml = export_netlist(&design, &model, NetlistFormat::Xml, &meta()).unwrap();
    well_formed(&xml).unwrap_or_else(|e| panic!("{e}\n{xml}"));
    assert!(xml.starts_with("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<export version=\"E\">\n  <design>\n"));
    assert!(xml.contains("<net code=\"1\" name=\"GND\" class=\"Default\">"));
    assert!(xml.contains("<node ref=\"U1\" pin=\"1\" pinfunction=\"VIN_1\" pintype=\"power_in\"/>"));
    assert_eq!(xml.matches("<node ").count(), 9);
    assert_eq!(xml.matches("<comp ref=").count(), 3);
    // `.xml` has no groups/variants (those are GNL_OPT_KICAD only)
    assert!(!xml.contains("<groups") && !xml.contains("<variants"));
    let kicad = export_netlist(&design, &model, NetlistFormat::Kicad, &meta()).unwrap();
    assert!(kicad.contains("(groups)") && kicad.contains("(variants)"));
}

#[test]
fn power_and_virtual_references_are_not_in_the_netlist() {
    let (design, mut model) = single();
    // A `#PWR` reference on a net: counted as a pin of the net by the model, never emitted.
    model.nets[0].pins.push("#PWR01.1".into());
    let text = export_netlist(&design, &model, NetlistFormat::Kicad, &meta()).unwrap();
    assert!(!text.contains("#PWR01"));
    // ... and a net holding only virtual pins is skipped entirely, but still consumes its
    // code (`netCodeTxt = i + 1` over the sorted list): "AA_VIRT" sorts first, so GND is code 2.
    model.nets.push(eda_model::Net { name: "AA_VIRT".into(), pins: vec!["#PWR02.1".into()] });
    let text = export_netlist(&design, &model, NetlistFormat::Kicad, &meta()).unwrap();
    assert!(!text.contains("AA_VIRT"));
    let nets = parse_nets(&text);
    assert_eq!((nets[0].code.as_str(), nets[0].name.as_str()), ("2", "GND"));
}

#[test]
fn duplicate_pin_references_are_removed_and_nodes_sorted() {
    let (design, mut model) = single();
    model.nets[0].pins = vec!["U1.3".into(), "CIN.1".into(), "U1.1".into(), "CIN.1".into()];
    let nets = parse_nets(&export_netlist(&design, &model, NetlistFormat::Kicad, &meta()).unwrap());
    let vin = nets.iter().find(|n| n.name == "VIN").unwrap();
    assert_eq!(vin.nodes.iter().map(|n| format!("{}.{}", n.reference, n.pin)).collect::<Vec<_>>(), ["CIN.1", "U1.1", "U1.3"]);
}

#[test]
fn hierarchy_adds_sheet_entries_and_sheetpaths() {
    let (design, model) = hierarchical();
    let text = export_netlist(&design, &model, NetlistFormat::Kicad, &meta()).unwrap();
    let root = sexpr::parse(&text).unwrap();
    let items = root.as_list().unwrap();
    let d = sexpr::find(items, "design").unwrap();
    let sheets: Vec<(String, String, String)> = sexpr::find_all(d, "sheet").map(|s| (attr(s, "number").unwrap().into(), attr(s, "name").unwrap().into(), attr(s, "tstamps").unwrap().into())).collect();
    assert_eq!(sheets, vec![("1".into(), "/".into(), "/".into()), ("2".into(), "/power/".into(), "/sheet1/".into())]);
    let comps = sexpr::find(items, "components").unwrap();
    let r1 = sexpr::find_all(comps, "comp").next().unwrap();
    let sp = sexpr::find(r1, "sheetpath").unwrap();
    assert_eq!((attr(sp, "names"), attr(sp, "tstamps")), (Some("/power/"), Some("/sheet1/")));
    // R1's unwired pin 2 is its own default-named net, and "Net-(" sorts before "SIG".
    let nets = parse_nets(&text);
    assert_eq!(nets.iter().map(|n| n.name.as_str()).collect::<Vec<_>>(), ["Net-(R1-Pad2)", "SIG"]);
}

#[test]
fn no_schematic_is_an_error() {
    let (mut design, model) = single();
    design.schematic = None;
    let err = export_netlist(&design, &model, NetlistFormat::Kicad, &meta()).unwrap_err();
    assert_eq!(err[0].check, "netlist.no_schematic");
}

#[test]
fn prettify_matches_kicad_format_rules() {
    // Lists holding lists break one per line, tab indented; scalars stay inline.
    assert_eq!(prettify("(a (b \"x y\") (c (d \"1\") (e \"2\")))"), "(a\n\t(b \"x y\")\n\t(c\n\t\t(d \"1\")\n\t\t(e \"2\")\n\t)\n)\n");
    // Escaped quotes do not end a string, and whitespace inside strings is preserved.
    assert_eq!(prettify("(a (b \"q\\\"  )(\"))"), "(a\n\t(b \"q\\\"  )(\")\n)\n");
}

#[test]
fn str_num_cmp_is_a_natural_order() {
    assert_eq!(str_num_cmp("R2", "R10", false), Less);
    assert_eq!(str_num_cmp("R10", "R2", false), Greater);
    assert_eq!(str_num_cmp("R2", "R2", false), Equal);
    assert_eq!(str_num_cmp("C1", "C1A", false), Less);
    // case-sensitive: uppercase sorts before lowercase; ignore_case merges them
    assert_eq!(str_num_cmp("VIN", "unconnected", false), Less);
    assert_eq!(str_num_cmp("abc", "ABD", true), Less);
    assert_eq!(str_num_cmp("Net-(R1-Pad1)", "Net-(R1-Pad2)", false), Less);
}

#[test]
fn xnode_format_and_quoting() {
    let mut n = XNode::new("field", "va\"l\\ue\nx").attr("name", "N");
    n.add(XNode::new("child", ""));
    let mut raw = String::new();
    n.format(false, &mut raw);
    assert_eq!(raw, "(field (name \"N\") \"va\\\"l\\\\ue\\nx\"(child))");
    let _ = ldo_model();
}

#[test]
fn unconnected_net_names_follow_get_default_net_name() {
    let named = PinMeta { number: "5".into(), name: "NC".into(), etype: "no_connect".into(), unit: 1, at: (0, 0) };
    assert_eq!(default_net_name("U1", &named, true, false), "unconnected-(U1-NC-Pad5)");
    assert_eq!(default_net_name("U1", &named, false, false), "Net-(U1-NC)");
    assert_eq!(default_net_name("U1", &named, false, true), "Net-(U1-NC-Pad5)");
    let numbered = PinMeta { number: "2".into(), name: String::new(), etype: "passive".into(), unit: 1, at: (0, 0) };
    assert_eq!(default_net_name("R1", &numbered, false, false), "Net-(R1-Pad2)");
}
