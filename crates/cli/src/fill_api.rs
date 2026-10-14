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
    let pours = design.routing.as_ref().map_or(0, |r| r.zones.iter().filter(|z| !z.is_rule_area && z.outline.len() >= 3).count());
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
        // A rule area has no fill, ever: it is not in the report.
        .filter(|z| !z.is_rule_area)
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
}
