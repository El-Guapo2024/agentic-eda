//! Zone fills: KiCad's, through kicad-cli, and nobody else's.
//!
//! There is no zone filler in the studio. Fill All Zones (B) runs `kicad-cli pcb drc --refill-zones --save-board` on a copy of the derived board
//! (`eda_kicad_engine::refill_zones`; it is a kicad-cli route, so it runs through `studio::offload` and the one lane) and keeps the zones' `(filled_polygon ..)` beside
//! `design.json` as derived data (`<board>/.kicad/zone_fills.json`, `eda_kicad::zone_fills`), keyed by zone with the revision of the design they were made at. A board
//! imported from a `.kicad_pcb` starts with the fills the file was saved with.
//!
//! Between fills the board shows the last fill, as KiCad does with auto-refill off: `GET /api/fill` serves it, and the derived `.kicad_pcb` every kicad-cli run is fed
//! (the DRC's, the 3D view's, File > Save As) is written with it. A zone that was never filled, or was edited since, has none and is drawn as an outline -- no copper
//! in that zone yet, here and in the live connectivity -- until the next B.

use crate::board;
use eda_kicad::zone_fills::{self, KicadFills, ScannedFill};
use eda_model::ir::Design;
use eda_model::CheckResult;
use serde_json::{json, Value};
use std::path::Path;

/// A label for the design as it is: the stamps of `design.json` and its activity log, which is what `/api/version` is made of less the routing job. It says which
/// revision a fill was made at; nothing compares it (a fill is current by its zone).
pub(crate) fn design_stamp(dir: &Path) -> String {
    let stamp = |name: &str| std::fs::metadata(dir.join(name)).and_then(|m| m.modified()).ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map_or(0, |d| d.as_nanos());
    format!("{}-{}", stamp("design.json"), stamp("activity.jsonl"))
}

/// The fills kicad-cli made of the board in `dir`, if any.
pub(crate) fn cached(dir: &Path) -> Option<KicadFills> {
    zone_fills::load(dir)
}

/// The zone ids a request names: `{"zones": ["id", ..]}`; `None` (every zone) when it names none.
pub(crate) fn zones_asked(body: &[u8]) -> Option<Vec<String>> {
    let v: Value = serde_json::from_slice(body).ok()?;
    let ids: Vec<String> = v.get("zones")?.as_array()?.iter().filter_map(|z| z.as_str().map(String::from)).collect();
    (!ids.is_empty()).then_some(ids)
}

/// Fill All Zones (B), or Fill Selected Zones when `only` names zones: kicad-cli's filler runs, and its fills are kept. `{ ok, engine, seconds, layers, rings, zones }`
/// or `{ ok: false, message }`. kicad-cli fills every zone whatever is asked (a zone is knocked out by the zones above it); the fills of the zones not asked for are
/// the ones they had.
pub(crate) fn refill(dir: &Path, only: Option<Vec<String>>) -> Value {
    let done = refill_once(dir, only.clone());
    // `ZONE_FILLER_TOOL`'s fill: `regenerateDirtyGenerators` (a via stitching whose net's zone was refilled under it is dirty, `OnZoneFillChanged`, and is updated over
    // the new fill), then `refillAroundGenerators` (the zones round the vias it made filled again, so they clear them).
    if done["ok"] == true && regenerate_stitchings(dir, only.as_deref()) {
        return refill_once(dir, only);
    }
    done
}

/// The via stitchings the fill just stored has made dirty (`PCB_VIA_STITCH::OnZoneFillChanged`: a refilled zone of its net whose box meets its own), updated
/// (`eda_drc::via_stitch::plan` over the new fills) in one step when their vias move. True when a stitching's vias changed.
fn regenerate_stitchings(dir: &Path, only: Option<&[String]>) -> bool {
    let Ok((_, design, model)) = board::load(dir) else { return false };
    let bbox = |pts: &[eda_model::ir::Point]| pts.iter().fold((i64::MAX, i64::MAX, i64::MIN, i64::MIN), |(a, b, c, d), p| (a.min(p.x), b.min(p.y), c.max(p.x), d.max(p.y)));
    let meets = |a: (i64, i64, i64, i64), b: (i64, i64, i64, i64)| a.0 <= b.2 && b.0 <= a.2 && a.1 <= b.3 && b.1 <= a.3;
    let zones: Vec<&eda_model::ir::Zone> = design.routing.iter().flat_map(|r| r.zones.iter()).filter(|z| zone_fills::fillable(z) && only.is_none_or(|ids| ids.contains(&z.id))).collect();
    let vias = |ids: &[String]| -> std::collections::BTreeSet<(i64, i64)> { design.routing.iter().flat_map(|r| r.vias.iter()).filter(|v| ids.contains(&v.id)).map(|v| (v.at.x, v.at.y)).collect() };
    let mut cmds = Vec::new();
    for v in design.drawings.iter().flat_map(|d| d.board_items.via_stitches.iter()) {
        if v.net.is_empty() || v.outline.len() < 3 {
            continue;
        }
        let own = bbox(&v.outline);
        if !zones.iter().any(|z| z.net == v.net && z.outline.len() >= 3 && meets(bbox(&z.outline), own)) {
            continue;
        }
        let p = crate::via_stitch_api::plan(dir, &design, &model, v);
        if p.vias.iter().map(|(_, at)| (at.x, at.y)).collect::<std::collections::BTreeSet<_>>() == vias(&v.member_ids) {
            continue;
        }
        let planned = p.vias.into_iter().map(|(cell, at)| eda_ops::pcb_items::PlannedVia { cell, at }).collect();
        cmds.push(eda_ops::Cmd::PcbItem(eda_ops::pcb_items::PcbItemCmd::SetViaStitch { id: Some(v.id.clone()), stitch: p.stitch, vias: Some(planned) }));
    }
    !cmds.is_empty() && board::step(dir, eda_ops::Cmd::Batch { cmds }, false, "fill").is_ok()
}

fn refill_once(dir: &Path, only: Option<Vec<String>>) -> Value {
    let revision = design_stamp(dir);
    let (_, design, model) = match board::load(dir) {
        Ok(loaded) => loaded,
        Err(e) => return json!({ "ok": false, "message": board::reasons(&e) }),
    };
    let mut fresh = match eda_kicad_engine::refill_zones(&design, &model, &dir.join(".kicad")) {
        Ok(f) => f,
        Err(e) => return json!({ "ok": false, "message": board::reasons(&e) }),
    };
    fresh.revision = revision;
    let mut fills = match &only {
        None => fresh.clone(),
        Some(_) => cached(dir).unwrap_or_else(|| KicadFills::new(String::new(), String::new(), 0, Default::default())),
    };
    if only.is_some() {
        fills.take_from(fresh, only.as_deref());
    }
    if let Err(e) = zone_fills::store(dir, &fills) {
        return json!({ "ok": false, "message": e });
    }
    let (layers, rings) = fills.counts();
    let (zones, _) = fills.tally(&design);
    json!({ "ok": true, "engine": fills.engine, "seconds": fills.seconds(), "layers": layers, "rings": rings, "zones": zones, "revision": fills.revision })
}

/// "Check zone fills before plotting" (`kicad-cli ... --check-zones`, "Check and refill zones if required"): KiCad's filler runs when some copper zone has no fill that is
/// current (never filled, unfilled, or edited since), and not otherwise. `Err` says why it could not.
pub(crate) fn ensure_filled(dir: &Path) -> Result<(), String> {
    let (_, design, _) = board::load(dir).map_err(|e| board::reasons(&e))?;
    let pours = design.routing.as_ref().map_or(0, |r| r.zones.iter().filter(|z| zone_fills::fillable(z)).count());
    if pours == 0 {
        return Ok(());
    }
    let current = cached(dir).map_or(0, |k| k.tally(&design).0);
    if current == pours {
        return Ok(());
    }
    let done = refill(dir, None);
    if done["ok"] == true {
        Ok(())
    } else {
        Err(done["message"].as_str().unwrap_or("kicad-cli could not fill the zones").to_string())
    }
}

/// Unfill All Zones (Ctrl+B), or Unfill Selected Zones when `only` names zones: the fills are taken away, and the zones are outlines until the next B.
pub(crate) fn clear(dir: &Path, only: Option<Vec<String>>) -> Value {
    match only {
        None => zone_fills::forget(dir),
        Some(ids) => {
            if let Some(mut fills) = cached(dir) {
                fills.remove(&ids);
                if let Err(e) = zone_fills::store(dir, &fills) {
                    return json!({ "ok": false, "message": e });
                }
            }
        }
    }
    json!({ "ok": true })
}

/// `GET /api/zone_priorities/auto`: the Zone Manager's Auto-Assign Priorities (`AutoAssignZonePriorities`, `eda_ops::zone_priority`) worked out for the board as it is, not
/// applied: `{ changed, priorities: { zone id: priority } }`. The dialog stages the priorities and applies them on OK, as KiCad's does with its clones of the zones.
pub(crate) fn auto_priorities(dir: &Path) -> Value {
    let (_, design, model) = match board::load(dir) {
        Ok(loaded) => loaded,
        Err(e) => return json!({ "changed": false, "error": board::reasons(&e) }),
    };
    match eda_ops::zone_priority::auto_assign_zone_priorities(&design, &model) {
        Some(priorities) => json!({ "changed": true, "priorities": priorities }),
        None => json!({ "changed": false, "priorities": {} }),
    }
}

/// A board imported from a `.kicad_pcb` shows the fills the file was saved with (KiCad's own, as saved): `scanned` is what the importer found in the file, filed for the
/// zones of `design` (which has its zone ids).
pub(crate) fn keep_imported(dir: &Path, design: &Design, scanned: Vec<ScannedFill>) {
    if scanned.is_empty() {
        return;
    }
    let mut design = design.clone();
    design.assign_missing_ids();
    let zones = zone_fills::entries_of_imported_file(&design, scanned);
    if zones.is_empty() {
        return;
    }
    let fills = KicadFills::new("the .kicad_pcb that was imported".into(), design_stamp(dir), 0, zones);
    let _ = zone_fills::store(dir, &fills);
}

/// `GET /api/fill`: the zones' fills as KiCad made them, as polygons in the raw-micrometre space `/api/ratsnest` uses, for the UI to draw. One entry per zone and layer:
/// `filled` says the zone has a fill that is current (a zone never filled, or edited since, is an outline: `fragments` is empty); each fragment is a closed ring, KiCad's
/// own (an island with its holes bridged to its outline, already slitted, never a separate holes list). `with_polys` (`?polys=1`) adds `polys`: the same fill unfractured,
/// one `{ outline, holes }` per island -- what the "Draw Zone Fill Triangulation" display triangulates (`kicad-port/polyTriangulate.ts`), as KiCad's `POLYGON_TRIANGULATION`
/// does (it bridges the holes itself rather than reading the fractured ring). `source` says whose fill it is: `{ engine, revision, seconds, zones, stale }` or `null`.
pub(crate) fn report(dir: &Path, with_polys: bool) -> Result<Value, Vec<CheckResult>> {
    let (_, design, _) = board::load(dir)?;
    let cache = cached(dir);
    let copper = cache.as_ref().map(|k| k.copper(&design)).unwrap_or_default();
    let zones: &[eda_model::ir::Zone] = design.routing.as_ref().map(|r| r.zones.as_slice()).unwrap_or(&[]);

    // One entry per zone and layer: a zone on several layers (`Zone::layers`) has a fill on each, under the same id.
    let zones_json: Vec<Value> = zones
        .iter()
        // A rule area has no fill, ever, and nor has an outline of fewer than three corners or a footprint's own zone (kicad-cli fills none of them): they are not in the
        // report, unless a file stored a fill for one.
        .filter(|z| zone_fills::fillable(z) || (!z.is_rule_area && z.outline.len() >= 3 && cache.as_ref().is_some_and(|k| k.current(z).is_some())))
        .flat_map(|z| z.layer_set().into_iter().map(move |layer| (z, layer)))
        .map(|(z, layer)| {
            let fill = copper.get(&z.id, layer);
            let filled = cache.as_ref().is_some_and(|k| k.current(z).is_some());
            let fragments: Vec<Value> = fill.map(|f| f.polys.iter().map(|poly| json!(poly[0].iter().map(|p| [p.x, p.y]).collect::<Vec<_>>())).collect()).unwrap_or_default();
            let mut zone = json!({
                "id": z.id, "net": z.net, "layer": layer,
                "filled": filled,
                "area_um2": fill.map(|f| f.area()).unwrap_or(0.0),
                "fragments": fragments,
            });
            if with_polys {
                let ring = |chain: &Vec<eda_clipper2::Point64>| chain.iter().map(|p| [p.x, p.y]).collect::<Vec<_>>();
                let polys: Vec<Value> = fill
                    .map(|f| {
                        let mut whole = f.clone();
                        whole.unfracture();
                        whole.polys.iter().map(|poly| json!({ "outline": ring(&poly[0]), "holes": poly[1..].iter().map(ring).collect::<Vec<_>>() })).collect()
                    })
                    .unwrap_or_default();
                zone["polys"] = Value::Array(polys);
            }
            zone
        })
        .collect();
    let source = cache.as_ref().map(|k| {
        let (current, stale) = k.tally(&design);
        json!({ "engine": k.engine, "revision": k.revision, "seconds": k.seconds(), "zones": current, "stale": stale })
    });
    Ok(json!({ "zones": zones_json, "source": source }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use eda_kicad::zone_fills::{zone_signature, KicadRing, ZoneEntry};
    use eda_model::ir::{PlacementSection, Point, Provenance, RoutingSection, Zone};
    use eda_model::{ConstraintModel, Net};
    use std::collections::BTreeMap;
    use std::path::PathBuf;

    fn square(id: &str, uuid: &str, layer: &str, x: i64) -> Zone {
        let p = |x, y| Point { x, y };
        Zone { id: id.into(), uuid: uuid.into(), net: "GND".into(), layer: layer.into(), outline: vec![p(x, 0), p(x + 10_000, 0), p(x + 10_000, 10_000), p(x, 10_000)], ..Default::default() }
    }

    /// A board directory with an intent of two nets, no part, and `zones`.
    fn board_dir(name: &str, zones: Vec<Zone>) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("eda_fill_api_{}_{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let model = ConstraintModel { nets: vec![Net { name: "GND".into(), pins: vec![] }, Net { name: "VCC".into(), pins: vec![] }], ..Default::default() };
        std::fs::write(dir.join("intent.yaml"), serde_yaml::to_string(&model).unwrap()).unwrap();
        write_zones(&dir, zones);
        std::fs::write(dir.join("board.json"), serde_json::to_string(&json!({ "intent": dir.join("intent.yaml").display().to_string(), "snap_um": 100, "spacing_um": 500 })).unwrap()).unwrap();
        dir
    }

    fn write_zones(dir: &Path, zones: Vec<Zone>) {
        let design = Design {
            schema: 1,
            provenance: Provenance { engine_version: "t".into(), intent_hash: "t".into(), seed: 0, stage_hashes: vec![] },
            schematic: None,
            nets: None,
            placement: Some(PlacementSection { outline: vec![Point { x: 0, y: 0 }, Point { x: 40_000, y: 0 }, Point { x: 40_000, y: 20_000 }, Point { x: 0, y: 20_000 }], footprints: vec![], modules: vec![] }),
            routing: Some(RoutingSection { tracks: vec![], vias: vec![], zones, track_width_presets: vec![], via_presets: vec![], teardrop_settings: Default::default() }),
            drawings: None,
            footprint_library: None,
            sheet_contents: None,
            bus_aliases: vec![],
            symbol_library: None,
        };
        std::fs::write(dir.join("design.json"), design.canonical_bytes().unwrap()).unwrap();
    }

    fn fill_of(zone: &Zone, layer: &str) -> ZoneEntry {
        let ring = KicadRing { island: false, pts: vec![[1_000_000, 1_000_000], [9_000_000, 1_000_000], [9_000_000, 9_000_000], [1_000_000, 9_000_000]] };
        ZoneEntry { sig: zone_signature(zone), layers: BTreeMap::from([(layer.to_string(), vec![ring])]) }
    }

    #[test]
    fn a_request_names_the_zones_it_is_about_or_all_of_them() {
        assert_eq!(zones_asked(br#"{"zones": ["a", "b"]}"#), Some(vec!["a".to_string(), "b".to_string()]));
        assert_eq!(zones_asked(br#"{"zones": []}"#), None, "none named: every zone");
        assert_eq!(zones_asked(b""), None);
        assert_eq!(zones_asked(br#"{"other": 1}"#), None);
    }

    #[test]
    fn the_design_stamp_changes_with_the_files_it_is_made_of() {
        let dir = std::env::temp_dir().join(format!("eda_fill_api_stamp_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let none = design_stamp(&dir);
        std::fs::write(dir.join("design.json"), "{}").unwrap();
        assert_ne!(design_stamp(&dir), none);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// `GET /api/fill`: a zone never filled is an outline (`filled: false`, no fragments), one kicad-cli filled has its rings in micrometres, a zone edited since is an outline again
    /// and counts as out of date in the source, and a rule area, or an outline with fewer than three corners, is not in the report at all.
    #[test]
    fn the_report_says_which_zones_have_a_current_fill_and_leaves_rule_areas_and_degenerate_outlines_out() {
        let mut keepout = square("ka", "", "F.Cu", 20_000);
        keepout.is_rule_area = true;
        let mut line = square("zl", "", "F.Cu", 40_000);
        line.outline.truncate(2);
        let (a, b) = (square("za", "", "F.Cu", 0), square("zb", "", "B.Cu", 0));
        let dir = board_dir("report", vec![a.clone(), b.clone(), keepout.clone(), line.clone()]);

        // design.json keeps its zones in a canonical order, so they are found by id.
        let by_id = |r: &Value, id: &str| r["zones"].as_array().unwrap().iter().find(|z| z["id"] == id).cloned().unwrap_or(Value::Null);
        let r = report(&dir, false).unwrap();
        assert_eq!(r["source"], Value::Null, "nothing has been filled");
        let mut ids: Vec<&str> = r["zones"].as_array().unwrap().iter().map(|z| z["id"].as_str().unwrap()).collect();
        ids.sort();
        assert_eq!(ids, ["za", "zb"], "the rule area has no fill, ever, and two corners enclose nothing");
        assert!(r["zones"].as_array().unwrap().iter().all(|z| z["filled"] == false && z["fragments"].as_array().unwrap().is_empty() && z["area_um2"] == 0.0));

        let fills = KicadFills::new("kicad-cli 10.99.0".into(), "rev".into(), 2_500, BTreeMap::from([("za".to_string(), fill_of(&a, "F.Cu"))]));
        zone_fills::store(&dir, &fills).unwrap();
        let r = report(&dir, true).unwrap();
        let za = by_id(&r, "za");
        assert_eq!((za["filled"].clone(), za["fragments"][0][2].clone(), za["area_um2"].as_f64()), (json!(true), json!([9_000, 9_000]), Some(64_000_000.0)), "{za}");
        assert_eq!(za["polys"][0]["outline"].as_array().unwrap().len(), 4, "with `polys` the unfractured islands");
        assert_eq!(by_id(&r, "zb")["filled"], false, "zb was never filled");
        assert_eq!(r["source"], json!({ "engine": "kicad-cli 10.99.0", "revision": "rev", "seconds": 2.5, "zones": 1, "stale": 0 }));

        // Zone A is edited: its fill is no longer its own.
        let mut edited = a.clone();
        edited.min_thickness += 10;
        write_zones(&dir, vec![edited, b, keepout, line]);
        let r = report(&dir, false).unwrap();
        assert_eq!(by_id(&r, "za")["filled"], false);
        assert_eq!((r["source"]["zones"].clone(), r["source"]["stale"].clone()), (json!(0), json!(1)), "one zone out of date: {}", r["source"]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// "Check zone fills before plotting" starts kicad-cli only when a pour has no current fill: a board with none, or whose pours are all current, is left alone.
    #[test]
    fn checking_the_zone_fills_fills_nothing_that_is_filled_or_that_has_no_zone() {
        let none = board_dir("ensure_none", vec![]);
        assert!(ensure_filled(&none).is_ok(), "no zone, nothing to fill");
        let a = square("za", "", "F.Cu", 0);
        let dir = board_dir("ensure_current", vec![a.clone()]);
        zone_fills::store(&dir, &KicadFills::new("kicad-cli 10.99.0".into(), String::new(), 1, BTreeMap::from([("za".to_string(), fill_of(&a, "F.Cu"))]))).unwrap();
        assert!(ensure_filled(&dir).is_ok(), "every pour is current: kicad-cli is not started");
        // A footprint's own zone is not one kicad-cli fills (`FillAllZones` walks the board's zones): it is not waited for, and not in the report.
        let mut inside = square("zf", "", "F.Cu", 30_000);
        inside.parent_footprint = Some("U1".into());
        let with_footprint_zone = board_dir("ensure_footprint_zone", vec![a.clone(), inside]);
        zone_fills::store(&with_footprint_zone, &KicadFills::new("kicad-cli 10.99.0".into(), String::new(), 1, BTreeMap::from([("za".to_string(), fill_of(&a, "F.Cu"))]))).unwrap();
        assert!(ensure_filled(&with_footprint_zone).is_ok(), "the zone of a footprint has no fill to wait for");
        let r = report(&with_footprint_zone, false).unwrap();
        assert_eq!(r["zones"].as_array().unwrap().iter().map(|z| z["id"].as_str().unwrap()).collect::<Vec<_>>(), ["za"], "{r}");
        let _ = std::fs::remove_dir_all(&none);
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_dir_all(&with_footprint_zone);
    }

    /// Unfill All Zones forgets every fill; Unfill Selected Zones only the named ones.
    #[test]
    fn unfilling_forgets_all_the_fills_or_the_selected_zones_only() {
        let (a, b) = (square("za", "", "F.Cu", 0), square("zb", "", "B.Cu", 0));
        let dir = board_dir("clear", vec![a.clone(), b.clone()]);
        let both = KicadFills::new("kicad-cli".into(), String::new(), 1, BTreeMap::from([("za".to_string(), fill_of(&a, "F.Cu")), ("zb".to_string(), fill_of(&b, "B.Cu"))]));
        zone_fills::store(&dir, &both).unwrap();
        assert_eq!(clear(&dir, Some(vec!["za".into()])), json!({ "ok": true }));
        assert_eq!(cached(&dir).unwrap().zones.keys().collect::<Vec<_>>(), ["zb"]);
        assert_eq!(clear(&dir, None), json!({ "ok": true }));
        assert!(cached(&dir).is_none(), "Unfill All Zones leaves nothing");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// An imported board shows the fills its file was saved with: the zones are found by the uuid they had in the file, and each gets the fingerprint of the zone as imported.
    #[test]
    fn an_imported_boards_fills_are_filed_for_its_zones_by_the_uuid_they_had() {
        let dir = std::env::temp_dir().join(format!("eda_fill_api_import_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let (a, b) = (square("za", "uuid-a", "F.Cu", 0), square("zb", "uuid-b", "B.Cu", 0));
        let design = Design {
            schema: 1,
            provenance: Provenance { engine_version: "t".into(), intent_hash: "t".into(), seed: 0, stage_hashes: vec![] },
            schematic: None,
            nets: None,
            placement: None,
            routing: Some(RoutingSection { tracks: vec![], vias: vec![], zones: vec![a.clone(), b.clone()], track_width_presets: vec![], via_presets: vec![], teardrop_settings: Default::default() }),
            drawings: None,
            footprint_library: None,
            sheet_contents: None,
            bus_aliases: vec![],
            symbol_library: None,
        };
        let scanned = vec![
            ScannedFill { zone: "uuid-a".into(), layer: "F.Cu".into(), island: false, pts: vec![[0, 0], [1_000_000, 0], [1_000_000, 1_000_000]] },
            ScannedFill { zone: "uuid-a".into(), layer: "F.Cu".into(), island: true, pts: vec![[5_000_000, 5_000_000], [6_000_000, 5_000_000], [6_000_000, 6_000_000]] },
            ScannedFill { zone: "somebody else".into(), layer: "F.Cu".into(), island: false, pts: vec![[0, 0], [1, 0], [1, 1]] },
        ];
        keep_imported(&dir, &design, scanned);
        let kept = cached(&dir).expect("the fills were kept");
        assert_eq!(kept.zones.keys().collect::<Vec<_>>(), ["za"], "zb's file had no fill and the stranger's is nobody's");
        assert_eq!(kept.rings(&a, "F.Cu").len(), 2, "both rings, one an island");
        assert!(kept.rings(&a, "F.Cu")[1].island);
        assert!(kept.current(&b).is_none());
        // A file with no fill leaves nothing behind.
        let empty = std::env::temp_dir().join(format!("eda_fill_api_import_empty_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&empty);
        std::fs::create_dir_all(&empty).unwrap();
        keep_imported(&empty, &design, vec![]);
        assert!(cached(&empty).is_none());
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_dir_all(&empty);
    }
}
