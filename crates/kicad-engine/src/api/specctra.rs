//! File > Import > Specctra Session... by KiCad itself: the api-server's `ImportSpecctraSession` (`pcbnew/api/api_handler_pcb.cpp::
//! API_HANDLER_PCB::handleImportSpecctraSession`, which runs `DSN::ImportSpecctraSession` -- `SPECCTRA_DB::LoadSESSION` and `FromSESSION` -- on the open
//! board in one commit), read back onto the design.
//!
//! The board is the one the studio derives from the design (`export_board_with`, as every kicad-cli run gets it). The server opens it, lists its tracks,
//! arcs, vias and footprints (`GetItems`), imports the session, and lists them again; what the session put on the board goes back to the design as an
//! [`eda_kicad::specctra::session::SessionImport`], the same answer our own port of `FromSESSION` gives, which `eda_ops`' `Cmd::Specctra` applies:
//! * the tracks, arcs and vias whose KIID was not on the board before (`FromSESSION` makes every one anew: `new PCB_TRACK`, `new PCB_ARC`, `new PCB_VIA`);
//!   the locked ones that stayed (a locked track is exported `(type fix)` and left alone) are the design's already, by their old KIIDs;
//! * the footprints whose position, orientation or side the session changed (`FOOTPRINT::SetPosition`, `Flip`, `SetOrientation`), keyed by reference
//!   (the design's footprint id), in the design's frame (`eda_kicad::specctra::session::to_design_frame`).
//!
//! Nothing is saved: the server's board is closed unsaved (`CloseAllDocuments { force }`). Units are KiCad's nanometres both sides, so the positions are
//! KiCad's to the nanometre (`mapPt`, `scale`: `KiROUND( factor * distance / resValue )`).
//!
//! KiCad 25ec2335's `FromSESSION` is later than the 8303b2ad our port follows: a `(place ..)` of a reference the board has not got is skipped and counted
//! (`m_skippedCount`, "A single unresolvable place, wire, or via ... must not sink the whole session") where 8303b2ad threw "Reference '%s' not found.";
//! [`SessionByServer::skipped`] and [`SessionByServer::report`] carry KiCad's count and words.

use super::proto::google::protobuf::Any;
use super::proto::kiapi::board::commands::{ImportSpecctraSession, ImportSpecctraSessionResponse};
use super::proto::kiapi::board::types::{Arc, BoardLayer, FootprintInstance, Track as KTrack, Via as KVia, ViaType};
use super::proto::kiapi::common::commands::{GetItems, GetItemsResponse};
use super::proto::kiapi::common::types::{DocumentSpecifier, ItemHeader, KiCadObjectType, Kiid, LockedState, Vector2};
use super::server::{with_server, ApiError, Client};
use eda_kicad::specctra::session::{rot_of_file_orientation, to_design_frame, SessionImport, SessionPlace, SessionVia};
use eda_model::ir::{Design, Point, Side, Track, Via};
use eda_model::units::Nm;
use eda_model::via_extra::ViaKind;
use eda_model::ConstraintModel;
use prost::Message;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

/// What the server's import gave: the import for the design, and KiCad's own account of it.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SessionByServer {
    pub import: SessionImport,
    /// `ImportSpecctraSessionResponse::skipped_count`: places, wires and vias KiCad could not resolve and left out.
    pub skipped: u32,
    /// `ImportSpecctraSessionResponse::report`: the reporter's messages (what was imported, and the warnings).
    pub report: String,
    /// The server's KiCad version, for the reply's `engine`.
    pub engine: String,
}

/// `BoardLayer` as the board file names the layer: `BL_F_Cu` is `F.Cu`, `BL_In1_Cu` is `In1.Cu` (the design keeps the canonical names).
fn layer_name(layer: i32) -> String {
    let name = BoardLayer::try_from(layer).map(|l| l.as_str_name()).unwrap_or("BL_UNKNOWN");
    let bare = name.strip_prefix("BL_").unwrap_or(name);
    match bare.split_once('_') {
        Some((a, b)) => format!("{a}.{b}"),
        None => bare.to_string(),
    }
}

fn pt(v: &Option<Vector2>) -> Point {
    let v = v.clone().unwrap_or_default();
    Point { x: Nm::new(v.x_nm), y: Nm::new(v.y_nm) }
}

fn locked(state: i32) -> bool {
    state == LockedState::LsLocked as i32
}

fn kiid(id: &Option<Kiid>) -> String {
    id.as_ref().map(|k| k.value.clone()).unwrap_or_default()
}

/// What `GetItems` gives of one item, decoded by its type URL.
enum Item {
    Track(KTrack),
    Arc(Arc),
    Via(KVia),
    Footprint(Box<FootprintInstance>),
    Other,
}

fn decode(any: &Any) -> Result<Item, ApiError> {
    let bad = |e: prost::DecodeError| ApiError::Decode(format!("{}: {e}", any.type_url));
    let name = any.type_url.rsplit('/').next().unwrap_or("");
    Ok(match name {
        "kiapi.board.types.Track" => Item::Track(KTrack::decode(any.value.as_slice()).map_err(bad)?),
        "kiapi.board.types.Arc" => Item::Arc(Arc::decode(any.value.as_slice()).map_err(bad)?),
        "kiapi.board.types.Via" => Item::Via(KVia::decode(any.value.as_slice()).map_err(bad)?),
        "kiapi.board.types.FootprintInstance" => Item::Footprint(Box::new(FootprintInstance::decode(any.value.as_slice()).map_err(bad)?)),
        _ => Item::Other,
    })
}

/// `GetItems` of the routing and the footprints of the open board.
fn items(c: &mut Client, doc: &DocumentSpecifier) -> Result<Vec<Item>, ApiError> {
    let types = [KiCadObjectType::KotPcbTrace, KiCadObjectType::KotPcbArc, KiCadObjectType::KotPcbVia, KiCadObjectType::KotPcbFootprint].map(|t| t as i32).to_vec();
    let r: GetItemsResponse = c.call(&GetItems { header: Some(ItemHeader { document: Some(doc.clone()), container: None, field_mask: None }), types }, crate::limit(crate::EXPORT_TIMEOUT))?;
    r.items.iter().map(decode).collect()
}

/// A footprint as the session may move it: reference, position, orientation (degrees) and side.
fn footprint_place(f: &FootprintInstance) -> (String, Point, f64, Side) {
    let reference = f.reference_field.as_ref().and_then(|r| r.text.as_ref()).and_then(|t| t.text.as_ref()).map(|t| t.text.clone()).unwrap_or_default();
    let side = if f.layer == BoardLayer::BlBCu as i32 { Side::Bottom } else { Side::Top };
    (reference, pt(&f.position), f.orientation.as_ref().map_or(0.0, |a| a.value_degrees), side)
}

/// The via's kind: the server's `ViaType` (`VT_BLIND_BURIED`, the type of a KiCad before blind and buried were two, by its layers: blind when it reaches an
/// outer layer).
fn via_kind(v: &KVia, from: &str, to: &str) -> ViaKind {
    match ViaType::try_from(v.r#type).unwrap_or(ViaType::VtThrough) {
        ViaType::VtMicro => ViaKind::Micro,
        ViaType::VtBlind => ViaKind::Blind,
        ViaType::VtBuried => ViaKind::Buried,
        ViaType::VtBlindBuried if [from, to].iter().any(|l| *l == "F.Cu" || *l == "B.Cu") => ViaKind::Blind,
        ViaType::VtBlindBuried => ViaKind::Buried,
        _ => ViaKind::Through,
    }
}

/// `ImportSpecctraSession` of the session file `ses` on the board file `pcb`, read back against `design` (see the module doc).
pub fn import_on_board(pcb: &Path, ses: &Path, design: &Design, model: &ConstraintModel) -> Result<SessionByServer, ApiError> {
    let ses = std::fs::canonicalize(ses).unwrap_or_else(|_| ses.to_path_buf());
    with_server(|c| {
        let engine = c.version().unwrap_or_default();
        let doc = c.open_board(pcb)?;
        let result = (|| {
            let before = items(c, &doc)?;
            let r: ImportSpecctraSessionResponse = c.call(&ImportSpecctraSession { board: Some(doc.clone()), session_path: ses.to_string_lossy().into_owned() }, crate::limit(crate::EXPORT_TIMEOUT))?;
            let after = items(c, &doc)?;
            Ok((before, r, after))
        })();
        let _ = c.close_all();
        let (before, r, after) = result?;
        Ok(SessionByServer { import: read_back(&before, &after, design, model), skipped: r.skipped_count, report: r.report, engine })
    })
}

/// What the import put on the board: the items of `after` that `before` had not, and the footprints it moved (see the module doc).
fn read_back(before: &[Item], after: &[Item], design: &Design, model: &ConstraintModel) -> SessionImport {
    let old_ids: BTreeSet<String> = before
        .iter()
        .filter_map(|i| match i {
            Item::Track(t) => Some(kiid(&t.id)),
            Item::Arc(a) => Some(kiid(&a.id)),
            Item::Via(v) => Some(kiid(&v.id)),
            _ => None,
        })
        .collect();
    let was: BTreeMap<String, (Point, f64, Side)> = before
        .iter()
        .filter_map(|i| match i {
            Item::Footprint(f) => {
                let (r, at, deg, side) = footprint_place(f);
                Some((r, (at, deg, side)))
            }
            _ => None,
        })
        .collect();
    let net_of = |n: &Option<super::proto::kiapi::board::types::Net>| n.as_ref().map(|n| n.name.clone()).unwrap_or_default();
    let mut import = SessionImport::default();
    for item in after {
        match item {
            Item::Track(t) if !old_ids.contains(&kiid(&t.id)) => {
                let track = Track { id: String::new(), net: net_of(&t.net), pins: vec![], layer: layer_name(t.layer), width: Nm::new(t.width.as_ref().map_or(0, |w| w.value_nm)), pts: vec![pt(&t.start), pt(&t.end)], arc_mid_offset: None };
                import.tracks.push((track, locked(t.locked)));
            }
            Item::Arc(a) if !old_ids.contains(&kiid(&a.id)) => {
                let track = Track::new_arc(net_of(&a.net), layer_name(a.layer), Nm::new(a.width.as_ref().map_or(0, |w| w.value_nm)), pt(&a.start), pt(&a.mid), pt(&a.end));
                import.tracks.push((track, locked(a.locked)));
            }
            Item::Via(v) if !old_ids.contains(&kiid(&v.id)) => {
                let ps = v.pad_stack.clone().unwrap_or_default();
                let drill = ps.drill.clone().unwrap_or_default();
                // A through via's drill spans F.Cu to B.Cu; a drill without layers is one too.
                let (from, to) = if drill.start_layer == 0 { ("F.Cu".to_string(), "B.Cu".to_string()) } else { (layer_name(drill.start_layer), layer_name(drill.end_layer)) };
                let diameter = ps.copper_layers.first().and_then(|l| l.size.as_ref()).map_or(0, |s| s.x_nm);
                let kind = via_kind(v, &from, &to);
                let net = net_of(&v.net);
                // `makeVIA` leaves the drill undefined (`UNDEFINED_DRILL_DIAMETER`, -1) when the padstack's name gives the Default class's: the API hands the
                // padstack's own value, and the via's drill is then `PCB_VIA::GetDrillValue`'s, its net class's (as our port answers it).
                let drill_nm = drill.diameter.as_ref().map_or(0, |d| d.x_nm);
                let drill_nm = if drill_nm > 0 { drill_nm } else { model.board.via_drill_of(&net).get() };
                let via = Via { id: String::new(), net, at: pt(&v.position), drill: Nm::new(drill_nm), diameter: Nm::new(diameter), from_layer: from, to_layer: to };
                import.vias.push(SessionVia { via, kind, locked: locked(v.locked) });
            }
            Item::Footprint(f) => {
                let (reference, at, deg, side) = footprint_place(f);
                if was.get(&reference).is_some_and(|w| *w == (at, deg, side)) {
                    continue;
                }
                import.placements.push(SessionPlace { reference, at, rot: rot_of_file_orientation(deg), side });
            }
            _ => {}
        }
    }
    to_design_frame(&mut import.placements, design);
    import
}

/// `ImportSpecctraSession` of a session given as text (`source` is its file's name) on the board the studio derives from `design`, in a scratch folder.
/// The import `Cmd::Specctra` applies, through `eda_ops::set_session_importer`; an [`ApiError`] that [`ApiError::falls_back`] sends the caller to our own
/// port of `FromSESSION` (`eda_kicad::specctra`, INACTIVE but for that).
pub fn import_session(text: &str, source: &str, design: &Design, model: &ConstraintModel) -> Result<SessionByServer, ApiError> {
    if let Some(why) = super::unavailable() {
        return Err(ApiError::Unavailable(why));
    }
    crate::with_scratch(|work| {
        let (pcb, _) = crate::export_board_with(design, model, work, "board", None).map_err(|e| ApiError::Unavailable(e.iter().map(|c| c.hint.clone().unwrap_or_else(|| c.check.clone())).collect::<Vec<_>>().join("; ")))?;
        // The session under its own name (KiCad's reporter names it), in the scratch folder.
        let name = Path::new(source).file_name().and_then(|n| n.to_str()).filter(|n| !n.is_empty()).unwrap_or("session.ses");
        let ses = work.join(name);
        std::fs::write(&ses, text).map_err(|e| ApiError::Unavailable(format!("cannot write {}: {e}", ses.display())))?;
        import_on_board(&pcb, &ses, design, model)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn board_layers_are_named_as_the_board_file_names_them() {
        assert_eq!(layer_name(BoardLayer::BlFCu as i32), "F.Cu");
        assert_eq!(layer_name(BoardLayer::BlIn1Cu as i32), "In1.Cu");
        assert_eq!(layer_name(BoardLayer::BlIn30Cu as i32), "In30.Cu");
        assert_eq!(layer_name(BoardLayer::BlBCu as i32), "B.Cu");
    }
}
