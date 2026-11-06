//! Import Netlist (`BOARD_NETLIST_UPDATER::UpdateNetlist`) on a board made here: the dry run and the update tell the same changes in KiCad's words, and each
//! of the dialog's options does what KiCad's does.

use super::*;
use crate::pcb_transform_tests::{design, p};
use crate::Cmd;
use eda_model::footprint::{PadKind, PadShape};
use eda_model::units::mm;
use eda_model::{ConstraintModel, Footprint, Net, Pad, Pin, PinKind};

fn pads(name: &str, numbers: &[&str]) -> Footprint {
    let pad = |n: &str, i: usize| Pad { opposite_side: false, ext: Default::default(), number: n.into(), at: (mm(i as f64 * 2.0 - 3.0), Nm::ZERO), size: (mm(0.8), mm(0.8)), shape: PadShape::Rect, kind: PadKind::Smd, drill: None, drill_slot: None, rot: 0, roundrect_ratio: None };
    Footprint { name: name.into(), pads: numbers.iter().enumerate().map(|(i, n)| pad(n, i)).collect(), courtyard: Some((mm(8.0), mm(3.0))), model: None, courtyard_outlines: vec![], models3d: vec![], art: Default::default() }
}

fn part(r: &str, value: &str) -> Part {
    Part {
        reference: r.into(),
        mpn: None,
        lcsc: None,
        value: Some(value.into()),
        package: None,
        footprint: Some("Test:QUAD".into()),
        pins: ["1", "2", "3", "4"].iter().map(|n| Pin { number: (*n).into(), name: None, kind: PinKind::Passive }).collect(),
        body_nm: None,
        symbol: None,
        datasheet: None,
        edge: None,
    }
}

/// U1 and U2 (`Test:QUAD`), placed; N1 on their pads 1, N2 on their pads 2, U1.3 on N3.
fn model() -> ConstraintModel {
    let net = |name: &str, pins: &[&str]| Net { name: name.into(), pins: pins.iter().map(|p| p.to_string()).collect() };
    let mut m = ConstraintModel {
        parts: vec![part("U1", "A"), part("U2", "B")],
        nets: vec![net("N1", &["U1.1", "U2.1"]), net("N2", &["U1.2", "U2.2"]), net("N3", &["U1.3"])],
        footprints: vec![pads("Test:QUAD", &["1", "2", "3", "4"])],
        ..Default::default()
    };
    m.board.layers = vec!["F.Cu".into(), "B.Cu".into()];
    m
}

fn board(m: &ConstraintModel) -> Board<'_> {
    let mut d = design();
    let rt = d.routing.as_mut().unwrap();
    rt.tracks.clear();
    rt.vias.clear();
    rt.zones.clear();
    Board::new(d, m, mm(0.1), mm(0.3))
}

/// A netlist of U1 and U2: U1 and U2's pads 1 on `/A`, pads 2 on `/B`, U1's pad 3 on no net, `extra` components after them.
fn netlist(u1_value: &str, extra: &str) -> String {
    format!(
        r#"(export (version "E")
  (components
    (comp (ref "U1") (value "{u1_value}") (footprint "Test:QUAD") (fields (field (name "Datasheet")) (field (name "Description") "quad"))
      (property (name "Sheetname") (value "Root")) (property (name "Sheetfile") (value "x.kicad_sch")) (sheetpath (names "/") (tstamps "/")) (tstamps "4c671539-9120-429d-9b52-8f805a6b0fad"))
    (comp (ref "U2") (value "B") (footprint "Test:QUAD") (fields (field (name "Datasheet")) (field (name "Description") "quad"))
      (property (name "Sheetfile") (value "x.kicad_sch")) (sheetpath (names "/") (tstamps "/")) (tstamps "a5325dfa-f0ed-4395-9ec3-24765fab7875"))
    {extra})
  (nets
    (net (code "1") (name "/A") (node (ref "U1") (pin "1") (pinfunction "IN") (pintype "input")) (node (ref "U2") (pin "1")) (node (ref "U9") (pin "1")))
    (net (code "2") (name "/B") (node (ref "U1") (pin "2")) (node (ref "U2") (pin "2")) (node (ref "U9") (pin "2")))
    (net (code "3") (name "unconnected-(U1-Pad4)") (node (ref "U1") (pin "4") (pintype "passive+no_connect")))
    (net (code "4") (name "unconnected-(U2-Pad3)") (node (ref "U2") (pin "3")))
    (net (code "5") (name "unconnected-(U2-Pad4)") (node (ref "U2") (pin "4")))))"#
    )
}

fn by_ref(options: NetlistOptions) -> NetlistOptions {
    NetlistOptions { by_timestamp: false, ..options }
}

fn cmd(text: &str, options: NetlistOptions) -> ImportNetlist {
    let mut c = ImportNetlist::new(text, "t.net", options);
    c.at = Some(p(mm(10.0), mm(120.0)));
    c
}

fn texts(r: &NetlistReport) -> Vec<String> {
    r.lines.iter().map(|l| l.text.clone()).collect()
}

fn has(r: &NetlistReport, line: &str) -> bool {
    r.lines.iter().any(|l| l.text == line)
}

fn net_of(b: &Board<'_>, pin: &str) -> String {
    b.design().nets.as_ref().unwrap().iter().find(|n| n.pins.iter().any(|p| p == pin)).map(|n| n.name.clone()).unwrap_or_default()
}

/// Load and Test Netlist tells what Update PCB then does, in the present and then in the past tense, and changes nothing; the update re-nets every pad.
#[test]
fn the_dry_run_and_the_update_tell_the_same_changes() {
    let m = model();
    let text = netlist("A", "");
    let b = board(&m);
    let before = b.design().clone();
    let dry = test_netlist(before.clone(), &m, mm(0.1), mm(0.3), &cmd(&text, by_ref(NetlistOptions::default())));
    assert!(dry.error.is_none());
    let t = texts(&dry);
    assert_eq!(t[0], "Reading netlist file 't.net'.");
    assert_eq!(t[1], "Using reference designators to match symbols and footprints.");
    assert!(has(&dry, "Processing symbol 'U1:Test:QUAD'."), "{t:?}");
    assert!(has(&dry, "Add net /A."), "{t:?}");
    assert!(has(&dry, "Reconnect U1 pin 1 from N1 to /A."), "{t:?}");
    assert!(has(&dry, "Disconnect U1 pin 3."), "{t:?}");
    assert!(has(&dry, "Update U1 symbol association from  to /4c671539-9120-429d-9b52-8f805a6b0fad."), "{t:?}");
    // the sheet's human-readable path wins over a `Sheetname` property
    assert!(has(&dry, "Update U1 sheetname to '/'."), "{t:?}");
    assert!(has(&dry, "Update U1 fields."), "{t:?}");
    assert_eq!(t.last().unwrap(), "Total warnings: 0, errors: 0.");
    // what the board would have of a pin on no net, the netlist gives it: no message
    assert!(!t.iter().any(|l| l.contains("U2 pin 3")), "{t:?}");

    let mut b = board(&m);
    b.apply(&Cmd::ImportNetlist(cmd(&text, by_ref(NetlistOptions::default())))).unwrap();
    let done = take_report().unwrap();
    assert!(has(&done, "Reconnected U1 pin 1 from N1 to /A."), "{:?}", texts(&done));
    assert!(has(&done, "Disconnected U1 pin 3."), "{:?}", texts(&done));
    // KiCad's "Removed unused net" for the nets no pad is on now
    assert!(has(&done, "Removed unused net N1.") && has(&done, "Removed unused net N3."), "{:?}", texts(&done));
    assert_eq!([net_of(&b, "U1.1"), net_of(&b, "U2.1"), net_of(&b, "U1.2"), net_of(&b, "U1.3")], ["/A", "/A", "/B", ""]);
    // the pin the netlist puts on its own `unconnected-` net stays on none: the board names it so by itself
    assert_eq!(net_of(&b, "U2.3"), "");
    // the record keeps the netlist's names, the footprints' paths, sheets and pins
    let rec = b.design().drawings.as_ref().unwrap().imported_netlist.clone().unwrap();
    assert!(rec.names("/A") && rec.names("/B"));
    let u1 = rec.footprint("U1").unwrap();
    assert_eq!(u1.link.path, "/4c671539-9120-429d-9b52-8f805a6b0fad");
    assert_eq!(u1.link.sheetfile, "x.kicad_sch");
    assert_eq!(u1.link.pins.get("1"), Some(&("IN".to_string(), "input".to_string())));
    assert_eq!(u1.link.fields, [("Datasheet".to_string(), String::new()), ("Description".to_string(), "quad".to_string())]);
}

/// The same netlist again changes nothing: every line is a "Processing" line, and the totals.
#[test]
fn a_second_import_of_the_same_netlist_has_nothing_to_do() {
    let m = model();
    let text = netlist("A", "");
    let mut b = board(&m);
    b.apply(&Cmd::ImportNetlist(cmd(&text, by_ref(NetlistOptions::default())))).unwrap();
    let _ = take_report();
    // the model the next load builds: the record laid over it, the nets the design's
    let mut m2 = m.clone();
    m2.nets = b.design().nets.clone().unwrap();
    b.design().drawings.as_ref().unwrap().imported_netlist.as_ref().unwrap().apply(&mut m2);
    let again = test_netlist(b.design().clone(), &m2, mm(0.1), mm(0.3), &cmd(&text, by_ref(NetlistOptions::default())));
    let actions: Vec<&ReportLine> = again.lines.iter().filter(|l| l.severity == Severity::Action && !l.text.is_empty()).collect();
    assert!(actions.is_empty(), "{actions:?}");
}

/// A component the board has no footprint for is added from the library, at the command's point, its pads on their nets; one no library has is an error.
#[test]
fn a_new_component_is_added_and_a_missing_footprint_is_told() {
    let m = model();
    let extra = r#"(comp (ref "U9") (value "C") (footprint "Test:TWO") (tstamps "11111111-9120-429d-9b52-8f805a6b0fad")) (comp (ref "U8") (value "D") (footprint "Test:NOPE"))"#;
    let text = netlist("A", extra);
    let mut c = cmd(&text, by_ref(NetlistOptions::default()));
    c.library.insert("Test:TWO".into(), pads("Test:TWO", &["1", "2"]));
    c.definitions.insert("Test:TWO".into(), LibraryFootprint::from_engine_footprint(&pads("Test:TWO", &["1", "2"])));
    let dry = test_netlist(board(&m).design().clone(), &m, mm(0.1), mm(0.3), &c);
    assert!(has(&dry, "Add U9 (footprint 'Test:TWO')."), "{:?}", texts(&dry));
    assert!(has(&dry, "Cannot add U8 (footprint 'Test:NOPE' not found)."), "{:?}", texts(&dry));
    assert!(has(&dry, "U8 footprint 'NOPE' not found in any libraries in the footprint library table."), "{:?}", texts(&dry));
    let mut b = board(&m);
    b.apply(&Cmd::ImportNetlist(c)).unwrap();
    let done = take_report().unwrap();
    assert!(has(&done, "Added U9 (footprint 'Test:TWO')."), "{:?}", texts(&done));
    assert!(has(&done, "Changed REF** reference designator to U9."), "{:?}", texts(&done));
    assert!(has(&done, "Connected U9 pin 1 to /A."), "{:?}", texts(&done));
    assert_eq!(done.added, ["U9"]);
    let bp = b.design().drawings.as_ref().unwrap().board_parts.iter().find(|p| p.reference == "U9").unwrap().clone();
    assert_eq!(bp.footprint, "Test:TWO");
    assert_eq!(bp.value.as_deref(), Some("C"));
    assert_eq!(bp.pad_nets, [("1".to_string(), "/A".to_string()), ("2".to_string(), "/B".to_string())]);
    let at = b.design().placement.as_ref().unwrap().footprints.iter().find(|f| f.id == "U9").unwrap().at;
    assert_eq!(at, p(mm(10.0), mm(120.0)));
}

/// "Delete footprints with no components in netlist": a footprint no component matches goes, unless it is locked ("Delete/replace footprints even if locked"
/// off); off, it stays and keeps its nets.
#[test]
fn delete_extra_footprints_removes_the_unmatched_ones_but_a_locked_one() {
    let m = model();
    // a netlist of U1 alone
    let text = r#"(export (version "E") (components (comp (ref "U1") (value "A") (footprint "Test:QUAD"))) (nets (net (code "1") (name "N1") (node (ref "U1") (pin "1")))))"#;
    let keep = test_netlist(board(&m).design().clone(), &m, mm(0.1), mm(0.3), &cmd(text, by_ref(NetlistOptions::default())));
    assert!(!texts(&keep).iter().any(|l| l.contains("unused footprint")), "{:?}", texts(&keep));
    let delete = NetlistOptions { delete_extra_footprints: true, ..by_ref(NetlistOptions::default()) };
    let dry = test_netlist(board(&m).design().clone(), &m, mm(0.1), mm(0.3), &cmd(text, delete));
    assert!(has(&dry, "Remove unused footprint U2."), "{:?}", texts(&dry));
    let mut b = board(&m);
    b.apply(&Cmd::ImportNetlist(cmd(text, delete))).unwrap();
    assert!(has(&take_report().unwrap(), "Removed unused footprint U2."));
    assert!(b.design().placement.as_ref().unwrap().footprints.iter().all(|f| f.id != "U2"), "U2 is off the board");
    // locked: told, and kept
    let mut b = board(&m);
    b.apply(&Cmd::SetLocked { ids: vec!["U2".into()], locked: true }).unwrap();
    let locked = test_netlist(b.design().clone(), &m, mm(0.1), mm(0.3), &cmd(text, delete));
    assert!(has(&locked, "Cannot remove unused footprint U2 (footprint is locked)."), "{:?}", texts(&locked));
    assert_eq!(locked.warnings, 1);
    let overridden = test_netlist(b.design().clone(), &m, mm(0.1), mm(0.3), &cmd(text, NetlistOptions { override_locks: true, ..delete }));
    assert!(has(&overridden, "Remove unused footprint U2."), "{:?}", texts(&overridden));
}

/// "Replace footprints with those specified in netlist": a footprint the component names another one for is exchanged for it (the symbol is left as it is);
/// off, it is not, and `LoadFootprints` warns.
#[test]
fn replace_footprints_exchanges_a_footprint_the_netlist_names_another_one_for() {
    let m = model();
    let text = netlist("A", "").replacen(r#"(comp (ref "U1") (value "A") (footprint "Test:QUAD")"#, r#"(comp (ref "U1") (value "A") (footprint "Test:OTHER")"#, 1);
    let mut c = cmd(&text, by_ref(NetlistOptions::default()));
    c.library.insert("Test:OTHER".into(), pads("Test:OTHER", &["1", "2", "3", "4"]));
    let dry = test_netlist(board(&m).design().clone(), &m, mm(0.1), mm(0.3), &c);
    assert!(has(&dry, "Change U1 footprint from 'Test:QUAD' to 'Test:OTHER'."), "{:?}", texts(&dry));
    let mut b = board(&m);
    b.apply(&Cmd::ImportNetlist(c.clone())).unwrap();
    assert!(has(&take_report().unwrap(), "Changed U1 footprint from 'Test:QUAD' to 'Test:OTHER'."));
    assert_eq!(b.design().footprint_assignment("U1").map(|a| a.footprint.as_str()), Some("Test:OTHER"));
    // off: nothing changes, and the libraries' check says why
    c.options.replace_footprints = false;
    let off = test_netlist(board(&m).design().clone(), &m, mm(0.1), mm(0.3), &c);
    assert!(has(&off, "Footprint of U1 changed: board footprint 'Test:QUAD', netlist footprint 'Test:OTHER'."), "{:?}", texts(&off));
    assert!(!texts(&off).iter().any(|l| l.starts_with("Change U1 footprint")), "{:?}", texts(&off));
}

/// The value and the fabrication attributes follow the component; the Value is the board's own, not the part's.
#[test]
fn the_value_and_the_attributes_follow_the_component() {
    let m = model();
    let text = netlist("Z", "").replacen(r#"(property (name "Sheetname") (value "Root"))"#, r#"(property (name "Sheetname") (value "Root")) (property (name "dnp"))"#, 1);
    let mut b = board(&m);
    b.apply(&Cmd::ImportNetlist(cmd(&text, by_ref(NetlistOptions::default())))).unwrap();
    let done = take_report().unwrap();
    assert!(has(&done, "Changed U1 value from A to Z."), "{:?}", texts(&done));
    assert!(has(&done, "Added U1 'Do not place' fabrication attribute."), "{:?}", texts(&done));
    let rec = b.design().drawings.as_ref().unwrap().imported_netlist.clone().unwrap();
    assert_eq!(rec.footprint("U1").unwrap().value.as_deref(), Some("Z"));
    assert!(b.design().footprint_edit("U1").unwrap().attrs.unwrap().dnp);
}

/// Linked by time stamp, a footprint is the one whose path is the component's; with none, a component is added (here: refused, its reference being taken).
#[test]
fn linking_by_time_stamp_matches_by_path() {
    let m = model();
    let text = netlist("A", "");
    let dry = test_netlist(board(&m).design().clone(), &m, mm(0.1), mm(0.3), &cmd(&text, NetlistOptions::default()));
    assert!(has(&dry, "Using tstamps (unique IDs) to match symbols and footprints."));
    // the board's footprints have no paths (no schematic): none is matched, and the references are taken
    assert!(has(&dry, "Cannot add U1 (a footprint with this reference is on the board)."), "{:?}", texts(&dry));
    // with the paths the netlist gives, the same import matches them
    let mut m2 = m.clone();
    m2.symbol_links.insert("U1".into(), SymbolLink { path: "/4c671539-9120-429d-9b52-8f805a6b0fad".into(), ..Default::default() });
    let linked = test_netlist(board(&m2).design().clone(), &m2, mm(0.1), mm(0.3), &cmd(&text, NetlistOptions::default()));
    assert!(!has(&linked, "Cannot add U1 (a footprint with this reference is on the board)."), "{:?}", texts(&linked));
    assert!(has(&linked, "Reconnect U1 pin 1 from N1 to /A."), "{:?}", texts(&linked));
}

/// A file of no format KiCad reads changes nothing and is refused in KiCad's words.
#[test]
fn a_file_that_is_no_netlist_is_refused() {
    let m = model();
    let mut b = board(&m);
    let e = b.apply(&Cmd::ImportNetlist(cmd("*PADS-PCB*\n", NetlistOptions::default()))).unwrap_err();
    assert_eq!(e[0].hint.as_deref(), Some("Cannot open netlist file 't.net'."));
    let r = test_netlist(board(&m).design().clone(), &m, mm(0.1), mm(0.3), &cmd("*PADS-PCB*\n", NetlistOptions::default()));
    assert_eq!(r.error.as_deref(), Some("Cannot open netlist file 't.net'."));
}

/// A pin of the component the footprint has no pad for is an error (`testConnectivity`), and a pad the netlist gives no net is a warning.
#[test]
fn pins_with_no_pad_and_pads_with_no_pin_are_told() {
    let m = model();
    let text = r#"(export (version "E") (components (comp (ref "U1") (value "A") (footprint "Test:QUAD")))
      (nets (net (code "1") (name "N1") (node (ref "U1") (pin "1")) ) (net (code "2") (name "N9") (node (ref "U1") (pin "9")))))"#;
    let mut b = board(&m);
    b.apply(&Cmd::ImportNetlist(cmd(text, by_ref(NetlistOptions::default())))).unwrap();
    let done = take_report().unwrap();
    assert!(has(&done, "U1 pad 9 not found in Test:QUAD."), "{:?}", texts(&done));
    assert!(has(&done, "No net found for component U1 pad 4 (no pin 4 in symbol)."), "{:?}", texts(&done));
    assert!(done.errors >= 1 && done.warnings >= 1);
}
