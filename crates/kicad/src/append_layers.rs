//! Append Board's layer remap (KiCad 25ec2335 `PCB_IO_KICAD_SEXPR_PARSER::remapAppendedLayers`, `pcbnew/pcb_io/kicad_sexpr/pcb_io_kicad_sexpr_parser.cpp`).
//!
//! `PCB_CONTROL::AppendBoard` registers `DIALOG_MAP_LAYERS::RunModal` as the parser's layer callback, and the parser, once it has read the file's `(layers ..)` table, asks it
//! when the file does not fit the board: when one of the file's layers is not enabled on the board (`!aDestInitialEnabled.Contains( id )`), or has a name of its own that is
//! not the board's name of that layer (`!src.m_userName.IsEmpty() && src.m_userName != m_board->GetLayerName( id )`). Each of the file's layers is then a row of the dialog
//! ([`AppendLayerDesc`]): named by its own name (else its standard one), proposed for the board's layer of that name when there is one (`AutoMapLayer`), else for its own
//! layer; a copper layer may go to any copper layer of the larger of the two stacks (`LSET::AllCuMask( std::max( srcCopperCount, aDestInitialCopperCount ) )`), any other only
//! to itself (`LSET( { PCB_LAYER_ID( src.m_number ) } )`), and none is required.
//!
//! The answer ([`remap_appended_layers`]) moves every item of a layer to the layer it was matched to (`m_layerIndices[name] = target`), a target new to the board keeping the
//! file's descriptor of the layer (its name and type: `SetLayerDescr( target, descr )`). A layer left unmatched is `UNDEFINED_LAYER`: its name leaves the parser's maps, so what
//! is on it is read onto `Rescue` and, after the parser's "Undefined Layers Warning" ("Items found on undefined layers (%s).\nDo you wish to rescue them to the %s layer?\n\nZones
//! will need to be refilled.", Rescue / Cancel), moved to `Cmts.User`: a blind via's end on it goes to `F.Cu` or `B.Cu`, a through via is left as it is
//! (`PCB_IO_KICAD_SEXPR_PARSER::parseBOARD_unchecked`'s rescue pass). The page asks that question; the server does the rest on the file's text, before the clipboard reader
//! reads it ([`crate::parse_pcb_clipboard`]).

use crate::sexpr::{self, Sexpr};
use eda_model::layers::{is_copper_layer_name, pcb_layer_id, standard_layer_name};
use std::collections::BTreeMap;

/// The layer `remapAppendedLayers`' rescue pass moves the items of an unmatched layer to (`PCB_LAYER_ID destLayer = Cmts_User`).
pub const RESCUE_LAYER: &str = "Cmts.User";

/// One row of the layer dialog Append Board opens (`INPUT_LAYER_DESC` as `remapAppendedLayers` fills it).
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct AppendLayerDesc {
    /// `desc.Name`: the file's own name of the layer, else its standard name. The key of the answer.
    pub name: String,
    /// The layer's board-file name in the file (`F.Cu`, `User.3`).
    pub layer: String,
    /// `desc.AutoMapLayer`: the board's layer shown with the same name, else the file's own layer.
    pub auto_map: String,
    /// `desc.PermittedLayers`, in `LSET::UIOrder`.
    pub permitted: Vec<String>,
    /// Whether anything of the file is on the layer: an unmatched layer with nothing on it raises no "Undefined Layers Warning" (`m_undefinedLayers` is filled by `lookUpLayer`).
    pub used: bool,
}

/// One entry of the file's layer table: `(number "name" type ["user name"])`.
struct SourceLayer {
    name: String,
    user_name: String,
}

fn source_layers(root: &[Sexpr]) -> Vec<SourceLayer> {
    let Some(table) = sexpr::find(root, "layers") else { return vec![] };
    table[1..]
        .iter()
        .filter_map(Sexpr::as_list)
        .filter_map(|l| {
            let name = sexpr::txt(l, 1)?.to_string();
            // `parseLayer`: the user name is the optional string after the type; `hide` is the old visibility flag
            let user_name = sexpr::txt(l, 3).filter(|s| *s != "hide").unwrap_or("").to_string();
            (pcb_layer_id(&name).is_some()).then_some(SourceLayer { name, user_name })
        })
        .collect()
}

/// Every layer name the items of the board use (`(layer "X")`, `(layers "X" ..)`), the layer table left out.
fn used_layers(items: &[Sexpr], out: &mut std::collections::BTreeSet<String>, top: bool) {
    for it in items.iter().skip(1) {
        let Some(l) = it.as_list() else { continue };
        match sexpr::tag(l) {
            Some("layers") if top && l.get(1).is_some_and(|c| c.as_list().is_some()) => {}
            Some("layer") => {
                if let Some(n) = sexpr::txt(l, 1) {
                    out.insert(n.to_string());
                }
            }
            Some("layers") => out.extend(l[1..].iter().filter_map(Sexpr::text).map(str::to_string)),
            _ => used_layers(l, out, false),
        }
    }
}

/// `remapAppendedLayers`' question: the rows of the dialog for the board file `text`, or an empty list when every layer of the file fits the board (no dialog). `board_enabled`
/// is the board's layers before the append (its copper and the technical and user layers it enables, `aDestInitialEnabled`), `board_name` its name of a layer
/// (`m_board->GetLayerName`), `board_copper` its copper layer count (`aDestInitialCopperCount`).
pub fn append_layer_descs(text: &str, board_enabled: &[String], board_name: &dyn Fn(&str) -> String, board_copper: usize) -> Vec<AppendLayerDesc> {
    let Ok(Sexpr::List(root)) = sexpr::parse(text) else { return vec![] };
    let sources = source_layers(&root);
    let src_copper = sources.iter().filter(|s| is_copper_layer_name(&s.name)).count();
    let mismatch = sources.iter().any(|s| !board_enabled.contains(&s.name) || (!s.user_name.is_empty() && s.user_name != board_name(&s.name)));
    if !mismatch {
        return vec![];
    }
    let mut used = std::collections::BTreeSet::new();
    used_layers(&root, &mut used, true);
    let copper = copper_stack(src_copper.max(board_copper));
    let mut enabled_ui: Vec<&String> = board_enabled.iter().collect();
    enabled_ui.sort_by_key(|l| pcb_layer_id(l).unwrap_or(u32::MAX));
    sources
        .iter()
        .map(|s| {
            let name = if s.user_name.is_empty() { s.name.clone() } else { s.user_name.clone() };
            // "Prefer a destination layer with the same name as the auto-map suggestion": the board's enabled layers in `Seq()` order (by `PCB_LAYER_ID`)
            let auto_map = enabled_ui.iter().find(|l| board_name(l) == name).map_or_else(|| s.name.clone(), |l| (*l).clone());
            let permitted = if is_copper_layer_name(&s.name) { copper.clone() } else { vec![s.name.clone()] };
            AppendLayerDesc { used: used.contains(&s.name), name, layer: s.name.clone(), auto_map, permitted }
        })
        .collect()
}

/// `LSET::AllCuMask( n ).UIOrder()`: `F.Cu`, `In1.Cu` .. `In(n-2).Cu`, `B.Cu`.
fn copper_stack(n: usize) -> Vec<String> {
    let mut v = vec!["F.Cu".to_string()];
    v.extend((1..n.saturating_sub(1)).map(|i| format!("In{i}.Cu")));
    v.push("B.Cu".to_string());
    v
}

/// The board file `text` with its layers moved as the dialog's answer says: `answer` maps a row's name ([`AppendLayerDesc::name`]) to the board layer it was matched to, or to
/// `None` when it was left unmatched (its items rescued to [`RESCUE_LAYER`]). A layer the answer does not name keeps its own (`target = PCB_LAYER_ID( src.m_number )`), so an
/// empty answer -- the dialog cancelled, `RunModal` returning `{}` -- changes nothing. Returns `None` for a text that is not a board.
pub fn remap_appended_layers(text: &str, answer: &BTreeMap<String, Option<String>>) -> Option<String> {
    let Ok(Sexpr::List(mut root)) = sexpr::parse(text) else { return None };
    if sexpr::tag(&root) != Some("kicad_pcb") {
        return None;
    }
    let sources = source_layers(&root);
    // board-file name -> Some(target) / None (rescued)
    let mut map: BTreeMap<String, Option<String>> = BTreeMap::new();
    for s in &sources {
        let key = if s.user_name.is_empty() { &s.name } else { &s.user_name };
        if let Some(target) = answer.get(key) {
            map.insert(s.name.clone(), target.clone().filter(|t| pcb_layer_id(t).is_some()));
        }
    }
    if map.iter().all(|(k, v)| v.as_deref() == Some(k.as_str())) {
        return Some(text.to_string());
    }
    rewrite(&mut root, &map, true);
    Some(sexpr::to_text(&Sexpr::List(root)))
}

fn rewrite(items: &mut [Sexpr], map: &BTreeMap<String, Option<String>>, top: bool) {
    let tag = sexpr::tag(items).unwrap_or("").to_string();
    let via = tag == "via";
    for it in items.iter_mut().skip(1) {
        let Sexpr::List(l) = it else { continue };
        match sexpr::tag(l) {
            // the layer table. `parseLayers` has enabled every layer of the file on the board before the remap (`SetEnabledLayers( enabledLayers |
            // m_board->GetEnabledLayers() )`, the copper count from the table) and the remap only adds: a target new to the board takes the file's descriptor of the layer
            // matched to it (`SetLayerDescr( target, descr )`), and a rescue enables User.Comments. So every entry stays, and the targets are added.
            Some("layers") if top && l.get(1).is_some_and(|c| c.as_list().is_some()) => {
                let have: std::collections::BTreeSet<String> = l[1..].iter().filter_map(|e| e.as_list().and_then(|en| sexpr::txt(en, 1)).map(str::to_string)).collect();
                let mut added: Vec<Sexpr> = Vec::new();
                let mut taken = have.clone();
                for e in l[1..].iter() {
                    let Some(en) = e.as_list() else { continue };
                    let Some(n) = sexpr::txt(en, 1) else { continue };
                    match map.get(n) {
                        Some(Some(t)) if !taken.contains(t) => {
                            let mut copy = en.to_vec();
                            if let Some(id) = pcb_layer_id(t) {
                                sexpr::set_text(&mut copy[0], &id.to_string());
                            }
                            sexpr::set_text(&mut copy[1], t);
                            taken.insert(t.clone());
                            added.push(Sexpr::List(copy));
                        }
                        Some(None) if !taken.contains(RESCUE_LAYER) => {
                            taken.insert(RESCUE_LAYER.to_string());
                            added.push(Sexpr::List(vec![
                                Sexpr::Atom(pcb_layer_id(RESCUE_LAYER).unwrap_or(19).to_string()),
                                sexpr::quoted_atom(RESCUE_LAYER),
                                Sexpr::Atom("user".to_string()),
                                sexpr::quoted_atom(&standard_layer_name(RESCUE_LAYER)),
                            ]));
                        }
                        _ => {}
                    }
                }
                l.extend(added);
                // `LSET::Seq()` order, as KiCad writes the table
                let head = l.remove(0);
                l.sort_by_key(|e| e.as_list().and_then(|en| sexpr::txt(en, 1)).and_then(pcb_layer_id).unwrap_or(u32::MAX));
                l.insert(0, head);
            }
            Some("layer") => {
                if let Some(n) = sexpr::txt(l, 1).map(str::to_string) {
                    if let Some(t) = map.get(&n) {
                        let t = t.clone().unwrap_or_else(|| RESCUE_LAYER.to_string());
                        sexpr::set_text(&mut l[1], &t);
                    }
                }
            }
            Some("layers") => {
                let names: Vec<String> = l[1..].iter().filter_map(Sexpr::text).map(str::to_string).collect();
                for (k, n) in names.iter().enumerate() {
                    let Some(t) = map.get(n) else { continue };
                    let t = match t {
                        Some(t) => t.clone(),
                        // a via's end: `if( top_layer == Rescue ) top_layer = F_Cu; if( bottom_layer == Rescue ) bottom_layer = B_Cu;`
                        None if via => if k == 0 { "F.Cu".to_string() } else { "B.Cu".to_string() },
                        None => RESCUE_LAYER.to_string(),
                    };
                    sexpr::set_text(&mut l[k + 1], &t);
                }
                // a set that now names a layer twice names it once (`LSET`)
                let mut seen = std::collections::BTreeSet::new();
                let head = l.remove(0);
                l.retain(|e| e.text().is_none_or(|t| seen.insert(t.to_string())));
                l.insert(0, head);
            }
            _ => rewrite(l, map, false),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FILE: &str = r#"(kicad_pcb (version 20241229) (generator "pcbnew")
  (layers (0 "F.Cu" signal) (4 "In1.Cu" signal) (6 "In2.Cu" signal) (2 "B.Cu" signal) (25 "Edge.Cuts" user) (43 "User.3" front "Notes") (45 "User.4" user "Docs"))
  (net 0 "") (net 1 "A")
  (segment (start 0 0) (end 5 0) (width 0.2) (layer "In1.Cu") (net 1) (uuid "a1"))
  (via blind (at 1 1) (size 0.6) (drill 0.3) (layers "F.Cu" "In2.Cu") (net 1) (uuid "a2"))
  (gr_line (start 0 2) (end 5 2) (stroke (width 0.1) (type solid)) (layer "User.3") (uuid "a3"))
  (gr_line (start 0 3) (end 5 3) (stroke (width 0.1) (type solid)) (layer "User.4") (uuid "a4"))
)"#;

    fn board_name(l: &str) -> String {
        if l == "User.1" { "Notes".to_string() } else { standard_layer_name(l) }
    }

    /// `remapAppendedLayers`' test: nothing to ask when every layer of the file is on the board under the board's name of it; a row per layer otherwise, with the board's
    /// layer of the same name proposed and the copper layers free to go to any copper layer of the larger stack.
    #[test]
    fn the_dialog_is_asked_only_when_a_layer_does_not_fit_and_proposes_the_boards_layer_of_that_name() {
        let fits = r#"(kicad_pcb (version 20241229) (layers (0 "F.Cu" signal) (2 "B.Cu" signal) (25 "Edge.Cuts" user)))"#;
        let enabled: Vec<String> = ["F.Cu", "B.Cu", "Edge.Cuts", "User.1"].iter().map(|s| s.to_string()).collect();
        assert!(append_layer_descs(fits, &enabled, &board_name, 2).is_empty());
        let rows = append_layer_descs(FILE, &enabled, &board_name, 2);
        assert_eq!(rows.len(), 7);
        let notes = rows.iter().find(|r| r.layer == "User.3").unwrap();
        assert_eq!((notes.name.as_str(), notes.auto_map.as_str(), notes.permitted.clone()), ("Notes", "User.1", vec!["User.3".to_string()]));
        let in1 = rows.iter().find(|r| r.layer == "In1.Cu").unwrap();
        assert_eq!(in1.permitted, vec!["F.Cu", "In1.Cu", "In2.Cu", "B.Cu"]);
        assert!(in1.used && notes.used && !rows.iter().find(|r| r.layer == "Edge.Cuts").unwrap().used);
    }

    /// The answer moves the items, gives a new target the file's descriptor, and rescues an unmatched layer's items to User.Comments (a blind via's end to the outer copper).
    #[test]
    fn the_answer_moves_the_items_and_an_unmatched_layer_is_rescued_to_user_comments() {
        let answer: BTreeMap<String, Option<String>> = [("Notes".to_string(), Some("User.1".to_string())), ("In2.Cu".to_string(), None), ("Docs".to_string(), None)].into_iter().collect();
        let out = remap_appended_layers(FILE, &answer).unwrap();
        assert!(out.contains(r#"(layer "User.1") (uuid "a3")"#), "{out}");
        assert!(out.contains(r#"(layers "F.Cu" "B.Cu")"#), "the blind via's end on the unmatched layer goes to B.Cu: {out}");
        assert!(out.contains(r#"(layer "Cmts.User") (uuid "a4")"#), "{out}");
        assert!(out.contains(r#"(39 "User.1" front "Notes")"#), "the target takes the file's descriptor: {out}");
        assert!(out.contains(r#"(19 "Cmts.User" user "User.Comments")"#), "the rescue enables User.Comments: {out}");
        assert!(out.contains(r#"(6 "In2.Cu" signal)"#) && out.contains(r#"(45 "User.4" user "Docs")"#), "the file's layers stay enabled, as `parseLayers` enabled them: {out}");
        assert!(!out.contains(r#"(layer "User.4")"#) && !out.contains(r#""In2.Cu")"#), "nothing is left on an unmatched layer: {out}");
        assert_eq!(remap_appended_layers(FILE, &BTreeMap::new()).unwrap(), FILE, "Cancel: nothing moves");
    }
}
