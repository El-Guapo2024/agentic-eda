//! File > Import > Specctra Session... (`pcbnew.EditorControl.importSpecctraSession`): [`Cmd::Specctra`](crate::Cmd::Specctra), the routing an outside autorouter wrote
//! back in a session file put on the board in place of the routing it had.
//!
//! `PCB_EDIT_FRAME::ImportSpecctraSession` (`pcbnew/specctra_import_export/specctra_import.cpp`) clears the undo list first ("todo: use undo/redo feature"), reads the file
//! (`LoadSESSION`) and runs `SPECCTRA_DB::FromSESSION` on the board, which deletes every track, arc and via that is not locked, moves the footprints the session's
//! placement names, adds the session's wires and vias, and deletes the DRC markers. Here it is one command, so one undo step that brings the old routing back. The session
//! travels in the command as text: a browser gives the page a file's contents and not its path.
//!
//! What the session holds is read by `eda_kicad::specctra::session::from_session` against the board as KiCad reads it; this module only applies it to the design:
//! * the tracks and vias that are not in [`DrawingsSection::locked_ids`](eda_model::ir::DrawingsSection) go, netless ones too (`RemoveAll( { PCB_TRACE_T } )`), and a
//!   locked one stays (it was exported `(type fix)`, so the session does not bring it back);
//! * a footprint the placement names takes the session's position, side and orientation (`FOOTPRINT::SetPosition`, `Flip`, `SetOrientation`);
//! * every segment, arc and via of the session is added, `(type fix)` ones locked, and a via that is not a through via keeps its type (`DrawingsSection::via_extras`).
//!
//! The DRC markers `DeleteMARKERs` deletes are kicad-cli's report here, which is never stored: the next DRC run reports the new routing. Differs: the board's DRC
//! exclusions (`DrawingsSection::drc_exclusions`) are kept, where KiCad's `DeleteMARKERs()` deletes the excluded markers too, so its next save forgets them.
//!
//! Every track and via of the session is a new item, as KiCad makes them (`new PCB_TRACK`, `new PCB_VIA` with default padstack settings): a segment that comes back
//! unchanged gets its old content id, but not the old item's group, tuning pattern or via settings.

use super::Board;
use eda_model::ir::{DrawingsSection, RoutingSection};
use eda_model::via_extra::{ViaExtra, ViaKind};
use eda_model::CheckResult;

impl<'a> Board<'a> {
    /// `Cmd::Specctra`: see the module doc. `name` is the file the session came from, for the messages.
    pub(crate) fn import_specctra_session(&mut self, session: &str, name: &str) -> Result<(), Vec<CheckResult>> {
        let source = if name.is_empty() { "session" } else { name };
        let import = eda_kicad::specctra::session::from_session(session, source, &self.design, self.model).map_err(|e| vec![CheckResult::fail("ops_specctra_session", source, e)])?;

        let locked: std::collections::BTreeSet<String> = self.design.drawings.as_ref().map(|d| d.locked_ids.iter().cloned().collect()).unwrap_or_default();
        let copper = self.model.board.layers.len();

        // The footprints of the session's placement.
        for place in &import.placements {
            let Some(fp) = self.design.placement.as_mut().and_then(|p| p.footprints.iter_mut().find(|f| f.id == place.reference)) else {
                // `FindFootprintByReference` found it on the board the session was read against; a part the design has not placed has no footprint to move.
                return Err(vec![CheckResult::fail("ops_specctra_session", source, format!("Reference '{}' not found.", place.reference))]);
            };
            fp.at = place.at;
            fp.rot = place.rot;
            let flipped = fp.side != place.side;
            fp.side = place.side;
            if flipped {
                self.flip_footprint_fields(&place.reference, copper);
            }
        }

        // Delete the old tracks and vias but keep the locked ones.
        let rt: &mut RoutingSection = self.routing_mut();
        // Every track and via of the session is a new object (`FromSESSION`: `new PCB_TRACK`, `new PCB_VIA` with default settings, after `group->RemoveItem( track )`
        // of the old ones): an id the content gives back for an unchanged segment is not the old item, so what was said of the old one goes with it.
        let gone: std::collections::BTreeSet<String> = rt.tracks.iter().map(|t| t.id.clone()).chain(rt.vias.iter().map(|v| v.id.clone())).filter(|id| !locked.contains(id)).collect();
        rt.tracks.retain(|t| locked.contains(&t.id));
        rt.vias.retain(|v| locked.contains(&v.id));
        let kept_tracks = rt.tracks.len();
        let kept_vias = rt.vias.len();
        rt.tracks.extend(import.tracks.iter().map(|(t, _)| t.clone()));
        rt.vias.extend(import.vias.iter().map(|v| v.via.clone()));
        rt.assign_missing_ids();
        let new_track_ids: Vec<(String, bool)> = rt.tracks[kept_tracks..].iter().zip(&import.tracks).map(|(t, (_, l))| (t.id.clone(), *l)).collect();
        let new_via_ids: Vec<(String, ViaKind, bool)> = rt.vias[kept_vias..].iter().zip(&import.vias).map(|(v, s)| (v.id.clone(), s.kind, s.locked)).collect();
        let via_ids: std::collections::BTreeSet<String> = rt.vias.iter().map(|v| v.id.clone()).collect();
        let track_ids: std::collections::BTreeSet<String> = rt.tracks.iter().map(|t| t.id.clone()).collect();
        self.sort_routing();

        let dr: &mut DrawingsSection = self.drawings_mut();
        // What was said of a via that is gone goes with it, and so do the old items' places in groups and tuning patterns.
        dr.via_extras.retain(|e| via_ids.contains(&e.id) && !gone.contains(&e.id));
        for g in dr.groups.iter_mut() {
            g.member_ids.retain(|m| !gone.contains(m));
        }
        loop {
            let empty: Vec<String> = dr.groups.iter().filter(|g| g.member_ids.is_empty()).map(|g| g.id.clone()).collect();
            if empty.is_empty() {
                break;
            }
            dr.groups.retain(|g| !g.member_ids.is_empty());
            for g in dr.groups.iter_mut() {
                g.member_ids.retain(|m| !empty.contains(m));
            }
        }
        for p in dr.tuning_patterns.iter_mut() {
            p.member_ids.retain(|m| !gone.contains(m));
        }
        dr.tuning_patterns.retain(|p| !p.member_ids.is_empty());
        for (id, kind, _) in &new_via_ids {
            if *kind != ViaKind::Through {
                dr.via_extras.retain(|e| &e.id != id);
                dr.via_extras.push(ViaExtra { id: id.clone(), kind: *kind, tokens: vec![] });
            }
        }
        // A lock of a track or via that is gone goes too (`locked_ids` is cross-kind: a footprint's or a zone's stays).
        let is_routing_id = |id: &str| id.starts_with("trk_") || id.starts_with("via_");
        dr.locked_ids.retain(|id| !is_routing_id(id) || track_ids.contains(id) || via_ids.contains(id));
        for (id, _) in new_track_ids.iter().filter(|(_, l)| *l) {
            dr.locked_ids.push(id.clone());
        }
        for (id, _, _) in new_via_ids.iter().filter(|(_, _, l)| *l) {
            dr.locked_ids.push(id.clone());
        }
        dr.locked_ids.sort();
        dr.locked_ids.dedup();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use eda_model::units::mm;
    use crate::{Board, Cmd};
    use eda_model::ir::{Design, DrawingsSection, Point, Side};
    use eda_model::via_extra::{ViaExtra, ViaKind};
    use eda_model::{ConstraintModel, Net, Part, Pin, PinKind};
    use serde_json::json;

    fn model() -> ConstraintModel {
        let pin = |n: &str| Pin { number: n.into(), name: None, kind: PinKind::Passive };
        let r1 = Part { reference: "R1".into(), mpn: None, lcsc: None, value: Some("10k".into()), package: Some("0603".into()), footprint: Some("0603".into()), pins: vec![pin("1"), pin("2")], body_nm: None, symbol: None, datasheet: None, edge: None };
        ConstraintModel { parts: vec![r1], nets: vec![Net { name: "VCC".into(), pins: vec!["R1.1".into()] }, Net { name: "GND".into(), pins: vec!["R1.2".into()] }], ..Default::default() }
    }

    /// R1 on a 40 mm board, a track that is locked, one that is not, and a blind via (as the board's file said) that is not locked.
    fn design() -> Design {
        let mut d: Design = serde_json::from_value(json!({
            "schema": 1,
            "provenance": { "engine_version": "0", "intent_hash": "x", "seed": 0 },
            "placement": { "outline": [{ "x": 0, "y": 0 }, { "x": 40000, "y": 0 }, { "x": 40000, "y": 40000 }, { "x": 0, "y": 40000 }], "footprints": [{ "id": "R1", "at": { "x": 20000, "y": 20000 }, "rot": 0, "side": "top" }] },
            "routing": {
                "tracks": [
                    { "net": "VCC", "layer": "F.Cu", "width": 250, "pts": [{ "x": 19200, "y": 20000 }, { "x": 12000, "y": 20000 }] },
                    { "net": "GND", "layer": "F.Cu", "width": 250, "pts": [{ "x": 20800, "y": 20000 }, { "x": 30000, "y": 20000 }] }
                ],
                "vias": [{ "net": "VCC", "at": { "x": 12000, "y": 20000 }, "drill": 300, "diameter": 600, "from_layer": "F.Cu", "to_layer": "B.Cu" }]
            }
        }))
        .unwrap();
        d.assign_missing_ids();
        let rt = d.routing.as_ref().unwrap();
        let locked = rt.tracks.iter().find(|t| t.net == "GND").unwrap().id.clone();
        let via = rt.vias[0].id.clone();
        d.drawings = Some(DrawingsSection { locked_ids: vec![locked], via_extras: vec![ViaExtra { id: via, kind: ViaKind::Blind, tokens: vec![] }], ..Default::default() });
        d
    }

    const SESSION: &str = "(session r.ses (base_design r.dsn)
  (placement (resolution um 10) (component R0603 (place R1 250000 -200000 back 90)))
  (routes (resolution um 10)
    (library_out (padstack \"Via[0-1]_800:400_um\" (shape (circle F.Cu 8000)) (shape (circle B.Cu 8000)) (attach off)))
    (network_out
      (net VCC (wire (path F.Cu 2500 242000 -200000 242000 -150000 200000 -150000)) (via \"Via[0-1]_800:400_um\" 200000 -150000))
      (net GND (wire (path B.Cu 2500 258000 -200000 300000 -200000) (type fix))))))
";

    /// `FromSESSION`: the unlocked tracks and vias go, the locked one stays, the session's come in (a `(type fix)` wire locked), what was said of a via that went goes with
    /// it, and the footprint the placement names is moved, turned over and turned.
    #[test]
    fn a_session_replaces_the_unlocked_routing_and_moves_the_footprints_it_places() {
        let m = model();
        let before = design();
        let locked_before = before.drawings.as_ref().unwrap().locked_ids.clone();
        let mut b = Board::new(before, &m, mm(0.1), mm(0.3));
        let cmd = Cmd::Specctra { session: SESSION.into(), name: "r.ses".into() };
        assert!(cmd.imports_session() && !cmd.clears_routing() && !cmd.spans_both_editors());
        b.apply(&cmd).unwrap();
        let d = b.design();
        let rt = d.routing.as_ref().unwrap();
        let dr = d.drawings.as_ref().unwrap();
        assert!(rt.tracks.iter().any(|t| t.id == locked_before[0]), "the locked track stays");
        assert!(!rt.tracks.iter().any(|t| t.pts.contains(&Point { x: mm(12.0), y: mm(20.0) })), "the unlocked one is gone");
        let vcc: Vec<_> = rt.tracks.iter().filter(|t| t.net == "VCC").map(|t| (t.pts.clone(), t.width)).collect();
        assert_eq!(vcc.len(), 2, "the session's path is two segments");
        assert!(vcc.iter().all(|(_, w)| *w == mm(0.25)));
        assert_eq!(rt.vias.len(), 1);
        assert_eq!((rt.vias[0].at, rt.vias[0].diameter, rt.vias[0].drill), (Point { x: mm(20.0), y: mm(15.0) }, mm(0.8), mm(0.4)));
        assert!(dr.via_extras.is_empty(), "the blind via went, and the new one is a through via");
        let gnd_new = rt.tracks.iter().find(|t| t.layer == "B.Cu").expect("the fixed GND wire");
        assert!(dr.locked_ids.contains(&gnd_new.id) && dr.locked_ids.contains(&locked_before[0]), "{:?}", dr.locked_ids);
        let fp = &d.placement.as_ref().unwrap().footprints[0];
        // KiCad's orientation is 90 + 180 on the back; a footprint made here is written with a half turn in its orientation (KiCad's frame of a flipped
        // footprint, `eda_kicad::pcb::write_footprint`), so the studio's own turn is 180 - 270
        assert_eq!((fp.at, fp.side, fp.rot), (Point { x: mm(25.0), y: mm(20.0) }, Side::Bottom, 270_000));
    }

    /// D2 (r2-editors-exports): the session's routing is new items (`FromSESSION`: `new PCB_TRACK`, `new PCB_VIA`), even where a segment comes back unchanged and so
    /// gets its old content id: it is in no group and has none of the old via's settings.
    #[test]
    fn the_sessions_routing_is_new_items_in_no_group_and_with_no_old_via_settings() {
        let m = model();
        let mut d = design();
        let rt = d.routing.as_ref().unwrap();
        let vcc = rt.tracks.iter().find(|t| t.net == "VCC").unwrap().id.clone();
        let via = rt.vias[0].id.clone();
        let dr = d.drawings.as_mut().unwrap();
        dr.groups = vec![eda_model::ir::Group { id: "grp_1".into(), name: String::new(), member_ids: vec![vcc.clone(), "R1".into()], design_block: String::new() }];
        dr.via_extras = vec![ViaExtra { id: via.clone(), kind: ViaKind::Through, tokens: vec!["(tenting (front yes) (back yes))".into()] }];
        let mut b = Board::new(d, &m, mm(0.1), mm(0.3));
        // The echo of the VCC track and the via: the same points, width, drill and size.
        let echo = "(session r.ses (base_design r.dsn)
  (routes (resolution um 10)
    (library_out (padstack \"Via[0-1]_600:300_um\" (shape (circle F.Cu 6000)) (shape (circle B.Cu 6000)) (attach off)))
    (network_out (net VCC (wire (path F.Cu 2500 192000 -200000 120000 -200000)) (via \"Via[0-1]_600:300_um\" 120000 -200000)))))
";
        b.apply(&Cmd::Specctra { session: echo.into(), name: "r.ses".into() }).unwrap();
        let d = b.design();
        assert!(d.routing.as_ref().unwrap().tracks.iter().any(|t| t.id == vcc), "the unchanged segment comes back with its content id");
        let dr = d.drawings.as_ref().unwrap();
        assert_eq!(dr.groups.iter().map(|g| g.member_ids.clone()).collect::<Vec<_>>(), [vec!["R1".to_string()]], "the new track is in no group");
        assert!(dr.via_extras.iter().all(|e| e.tokens.is_empty()), "the old via's tenting is not the new via's: {:?}", dr.via_extras);
    }

    /// A session KiCad would refuse leaves the board as it was, with KiCad's words.
    #[test]
    fn a_refused_session_changes_nothing() {
        let m = model();
        let mut b = Board::new(design(), &m, mm(0.1), mm(0.3));
        let bad = SESSION.replace("(path B.Cu", "(path In3.Cu");
        let e = b.apply(&Cmd::Specctra { session: bad, name: "r.ses".into() }).unwrap_err();
        assert_eq!(e[0].hint.as_deref(), Some("Session file uses invalid layer id 'In3.Cu'."), "{e:?}");
        assert_eq!(b.design().routing.as_ref().unwrap().tracks.len(), 2);
        // The command is the wire's: `{"op": "specctra", "session": ..}`.
        let c: Cmd = serde_json::from_value(json!({ "op": "specctra", "session": "(session x)" })).unwrap();
        assert_eq!(c, Cmd::Specctra { session: "(session x)".into(), name: String::new() });
    }
}
