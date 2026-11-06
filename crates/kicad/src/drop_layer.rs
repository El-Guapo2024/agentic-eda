//! A layer of a board file taken out, with what is on it: what an importer does with a source layer the person left unmatched in the layer dialog.
//!
//! `DIALOG_MAP_LAYERS` (`pcbnew/dialogs/dialog_map_layers.cpp`) answers the importer's layer callback with `UNDEFINED_LAYER` for every layer left in the Unmatched list
//! (`GetSelectedLayerName` finds no pair; `RunModal` returns the map with those layers out of it, and the importers' `kicad_layer()` / `getKicadLayer()` give
//! `UNDEFINED_LAYER`), and the importers leave out what is on such a layer: Eagle's `if( layer != UNDEFINED_LAYER )` around every wire, text, circle, rectangle and
//! polygon ("Ignoring a text since Eagle layer '%s' (%d) was not mapped", `pcb_io_eagle.cpp`), Altium's `if( klayer == UNDEFINED_LAYER ) continue;`, and so on.
//!
//! kicad-cli takes a layer map that can say where a layer goes but not leave it out (`PCBNEW_JOBS_HANDLER::JobImport`: a name it cannot resolve is "an unknown KiCad layer;
//! using automatic mapping instead"). So the studio sends the layers to leave out to a user layer nothing else goes to, and [`drop_board_layer`] takes that layer and what is
//! on it out of the board kicad-cli wrote -- the board the importer would have made had the callback said `UNDEFINED_LAYER`, but for two things a board file cannot say
//! otherwise: a footprint's Reference or Value field that was on the layer stays (a footprint always has them) on its usual layer and hidden, as Eagle leaves a package's
//! `>NAME` it could not place (`footprint->Reference().SetVisible( false )`); and a pad loses the layer from its layer set.

use crate::sexpr::{self, Sexpr};

/// The layer named by a list's `(layer "X")` child, if it has one.
fn layer_of(list: &[Sexpr]) -> Option<&str> {
    list.iter().filter_map(Sexpr::as_list).find(|l| sexpr::tag(l) == Some("layer")).and_then(|l| sexpr::txt(l, 1))
}

/// Take `layer` out of a list of a board file: the items on it go, a field on it is hidden on its usual layer, a layer set loses it. `side_back` is the side of the
/// footprint the list is in (for a field's usual layer).
fn strip(items: &mut Vec<Sexpr>, layer: &str, in_footprint: Option<bool>) -> usize {
    let mut gone = 0;
    let mut i = 1; // the tag is item 0
    while i < items.len() {
        let Sexpr::List(child) = &mut items[i] else {
            i += 1;
            continue;
        };
        let tag = sexpr::tag(child).unwrap_or("").to_string();
        match tag.as_str() {
            // the board's layer table: `(127 "User.45" user)`
            "layers" if in_footprint.is_none() && child.get(1).is_some_and(|c| c.as_list().is_some()) => {
                child.retain(|e| e.as_list().map_or(true, |l| sexpr::txt(l, 1) != Some(layer)));
                i += 1;
            }
            "footprint" | "module" => {
                let back = layer_of(child).is_some_and(|l| l.starts_with("B."));
                gone += strip(child, layer, Some(back));
                i += 1;
            }
            "property" | "fp_text" if in_footprint.is_some() && layer_of(child) == Some(layer) && is_field(child) => {
                // A footprint keeps its Reference and Value: on the layer a new footprint has them (`FOOTPRINT::FOOTPRINT`: the Reference on F.SilkS, the Value on F.Fab,
                // flipped with the footprint), and not shown.
                let reference = matches!(sexpr::txt(child, 1), Some("Reference") | Some("reference"));
                let back = in_footprint == Some(true);
                let usual = match (reference, back) {
                    (true, false) => "F.SilkS",
                    (true, true) => "B.SilkS",
                    (false, false) => "F.Fab",
                    (false, true) => "B.Fab",
                };
                for c in child.iter_mut() {
                    if let Sexpr::List(l) = c {
                        if sexpr::tag(l) == Some("layer") && l.len() > 1 {
                            sexpr::set_text(&mut l[1], usual);
                        }
                    }
                }
                child.retain(|c| !(c.as_list().is_some_and(|l| sexpr::tag(l) == Some("hide")) || c.text() == Some("hide")));
                child.push(Sexpr::List(vec![Sexpr::Atom("hide".into()), Sexpr::Atom("yes".into())]));
                i += 1;
            }
            _ if layer_of(child) == Some(layer) => {
                items.remove(i);
                gone += 1;
            }
            _ => {
                // a layer set: `(layers "F.Cu" "User.45")` of a pad or a zone
                let mut emptied = false;
                for c in child.iter_mut() {
                    if let Sexpr::List(l) = c {
                        if sexpr::tag(l) == Some("layers") && l.iter().skip(1).any(|a| a.text() == Some(layer)) {
                            l.retain(|a| a.text() != Some(layer));
                            emptied = l.len() <= 1;
                        }
                    }
                }
                if emptied {
                    items.remove(i);
                    gone += 1;
                } else {
                    i += 1;
                }
            }
        }
    }
    gone
}

/// A footprint's Reference or Value field (`(property "Reference" ..)`, or a legacy `(fp_text reference ..)` / `(fp_text value ..)`), which a footprint cannot lose.
fn is_field(list: &[Sexpr]) -> bool {
    match sexpr::tag(list) {
        Some("property") => matches!(sexpr::txt(list, 1), Some("Reference") | Some("Value")),
        Some("fp_text") => matches!(sexpr::txt(list, 1), Some("reference") | Some("value")),
        _ => false,
    }
}

/// The board file `text` with `layer` (its board-file name, `User.45`) and what is on it taken out: see the module doc. Returns the new text and how many items went;
/// `None` for a text that is not a board.
pub fn drop_board_layer(text: &str, layer: &str) -> Option<(String, usize)> {
    let Ok(Sexpr::List(mut root)) = sexpr::parse(text) else { return None };
    if sexpr::tag(&root) != Some("kicad_pcb") {
        return None;
    }
    let gone = strip(&mut root, layer, None);
    Some((sexpr::to_text(&Sexpr::List(root)), gone))
}

#[cfg(test)]
mod tests {
    use super::*;

    const BOARD: &str = r#"(kicad_pcb (version 20260616) (generator "pcbnew")
  (layers (0 "F.Cu" signal) (2 "B.Cu" signal) (5 "F.SilkS" user "F.Silkscreen") (35 "F.Fab" user) (127 "User.45" user))
  (net 0 "")
  (footprint "lib:R" (layer "F.Cu") (at 10 10)
    (property "Reference" "R1" (at 0 -1 0) (layer "User.45") (uuid "a1") (effects (font (size 1 1))))
    (property "Value" "10k" (at 0 1 0) (layer "F.Fab") (uuid "a2") (effects (font (size 1 1))))
    (fp_line (start 0 0) (end 1 0) (stroke (width 0.1) (type solid)) (layer "User.45") (uuid "a3"))
    (fp_line (start 0 1) (end 1 1) (stroke (width 0.1) (type solid)) (layer "F.SilkS") (uuid "a4"))
    (pad "1" smd rect (at 0 0) (size 1 1) (layers "F.Cu" "User.45") (uuid "a5")))
  (footprint "lib:R" (layer "B.Cu") (at 20 10)
    (property "Reference" "R2" (at 0 -1 0) (layer "User.45") (hide yes) (uuid "b1") (effects (font (size 1 1))))
    (property "Value" "10k" (at 0 1 0) (layer "User.45") (uuid "b2") (effects (font (size 1 1)))))
  (gr_line (start 0 0) (end 5 0) (stroke (width 0.1) (type solid)) (layer "User.45") (uuid "c1"))
  (gr_text "kept" (at 0 0 0) (layer "F.SilkS") (uuid "c2") (effects (font (size 1 1))))
  (gr_text "gone" (at 0 0 0) (layer "User.45") (uuid "c3") (effects (font (size 1 1))))
  (zone (net 0) (net_name "") (layers "F.Cu" "User.45") (uuid "c4") (polygon (pts (xy 0 0) (xy 1 0) (xy 1 1))))
  (zone (net 0) (net_name "") (layer "User.45") (uuid "c5") (polygon (pts (xy 0 0) (xy 1 0) (xy 1 1))))
)"#;

    /// What an unmatched layer of `DIALOG_MAP_LAYERS` leaves of a board: nothing on it, the layer out of the table, the footprints' fields kept, hidden, on their usual layers.
    #[test]
    fn the_layer_and_what_is_on_it_go_and_a_footprint_keeps_its_fields_hidden() {
        let (text, gone) = drop_board_layer(BOARD, "User.45").unwrap();
        assert_eq!(gone, 4, "the footprint line, the board line, the board text and the zone on that layer alone: {text}");
        assert!(!text.contains("\"User.45\""), "{text}");
        assert!(text.contains("\"kept\"") && !text.contains("\"gone\""));
        let (design, _, _) = crate::import_kicad_pcb(&text).expect("a board the reader takes");
        let d = design.drawings.unwrap();
        assert_eq!(d.texts.len(), 1);
        // the zone on F.Cu and User.45 keeps F.Cu
        assert!(text.contains("(layers \"F.Cu\") (uuid \"c4\")") || text.contains("(layers F.Cu) (uuid c4)"), "{text}");
        // R1's Reference on F.SilkS, hidden; R2 on the back: B.SilkS and B.Fab
        let r1 = text.find("\"R1\"").unwrap();
        assert!(text[r1..r1 + 120].contains("F.SilkS") && text[r1..r1 + 200].contains("(hide yes)"), "{}", &text[r1..r1 + 200]);
        let r2 = text.find("\"R2\"").unwrap();
        assert!(text[r2..r2 + 120].contains("B.SilkS"));
        assert_eq!(text[r2..].matches("(hide yes)").count(), 2, "one hide each, not two on R2's Reference");
        // the pad keeps F.Cu
        assert!(text.contains("(layers \"F.Cu\") (uuid \"a5\")"), "{text}");
        assert!(drop_board_layer("(kicad_sch)", "User.45").is_none());
    }
}
