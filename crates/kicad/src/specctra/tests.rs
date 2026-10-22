//! Tests of the Specctra port: the writer held to the DSN file KiCad itself wrote, the parser held to the errors KiCad gives.

use super::model::*;
use super::parse::{load_pcb, load_session};

/// A DSN file KiCad 10.99 exported (`crates/freeroute/parity/fixtures`).
const KICAD_DSN: &str = include_str!("../../../freeroute/parity/fixtures/Issue756-tomu-fpga8-pours.dsn");
const KICAD_DSN_KEEPOUTS: &str = include_str!("../../../freeroute/parity/fixtures/keepouts-with-holes.dsn");

/// The first line two texts differ at, as a message.
fn first_difference(a: &str, b: &str) -> String {
    for (i, (la, lb)) in a.lines().zip(b.lines()).enumerate() {
        if la != lb {
            return format!("line {}:\n  want {la:?}\n  got  {lb:?}", i + 1);
        }
    }
    format!("lengths differ: {} vs {} lines", a.lines().count(), b.lines().count())
}

/// Everything of a DSN file KiCad wrote up to its `(wiring`, which a patched KiCad filled with the pours of the board as `protect` wires -- not what KiCad's `FromBOARD` writes.
#[test]
fn kicads_own_dsn_is_written_back_byte_for_byte() {
    let mut pcb = load_pcb(KICAD_DSN, "tomu.dsn").unwrap();
    assert!(pcb.wiring.as_ref().is_some_and(|w| !w.wires.is_empty()), "the pours are wires");
    pcb.wiring = None;
    let head = &KICAD_DSN[..KICAD_DSN.find("  (wiring").unwrap()];
    let want = format!("{head})\n");
    let text = pcb.to_text();
    assert!(text == want, "{}", first_difference(&want, &text));
}

#[test]
fn a_hand_made_design_with_keepout_windows_parses() {
    let pcb = load_pcb(KICAD_DSN_KEEPOUTS, "k.dsn").unwrap();
    let s = pcb.structure.unwrap();
    assert_eq!(s.keepouts.len(), 5);
    assert!(s.keepouts.iter().all(|k| !k.windows.is_empty()));
    assert_eq!(s.layers.len(), 2);
}

#[test]
fn a_session_without_a_routes_section_parses_to_none() {
    let s = load_session("(session x (base_design y))\n", "x.ses").unwrap();
    assert!(s.route.is_none());
    assert_eq!(s.base_design, "y");
    let _ = Pcb::default();
}
