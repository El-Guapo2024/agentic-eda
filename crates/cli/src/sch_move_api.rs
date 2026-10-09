//! `POST /api/sch/move_preview`: what a Move, Drag, Rotate or Mirror would do to the sheet in view, for the live preview while the items
//! are held. The studio sends the very commands it would commit (`{ "sheet": "<id>/<id>", "cmds": [...] }`); they are applied to the design
//! as it stands, in memory, and the geometry that came out is returned as a patch the view lays over the sheet it already has
//! (`SchMovePatch`, `web/studio/src/api/schEditTypes.ts`). Nothing is written, no history is kept, no checks run: the preview and the
//! commit are one code path (`eda_ops::Board::apply`), so the wire that is shown with a bend is the wire that is committed with it.

use crate::board;
use eda_model::ir::SchematicSection;
use eda_model::CheckResult;
use eda_ops::{Board, Cmd};
use serde_json::{json, Value};
use std::path::Path;

pub(crate) fn move_preview(dir: &Path, body: &[u8]) -> Value {
    match preview(dir, body) {
        Ok(v) => v,
        Err(e) => json!({ "ok": false, "message": board::reasons(&e) }),
    }
}

fn bad(why: impl Into<String>) -> Vec<CheckResult> {
    vec![CheckResult::fail("move_preview_request", "request", why)]
}

fn preview(dir: &Path, body: &[u8]) -> Result<Value, Vec<CheckResult>> {
    let req: Value = serde_json::from_slice(body).map_err(|e| bad(format!("not JSON: {e}")))?;
    let sheet = req.get("sheet").and_then(Value::as_str).unwrap_or("").to_string();
    let cmds: Vec<Cmd> = serde_json::from_value(req.get("cmds").cloned().unwrap_or(Value::Null)).map_err(|e| bad(format!("not commands: {e}")))?;
    let (meta, mut design, model) = board::load(dir)?;
    // a board with no stored schematic shows the one derived from its intent, and that is what a first edit stores
    if design.schematic.is_none() {
        let derived = board::derived_schematic(&design, &model)?;
        design.schematic = derived.schematic;
        design.sheet_contents = derived.sheet_contents;
    }
    let mut b = Board::new(design, &model, meta.snap_um, meta.spacing_um);
    for cmd in cmds {
        let cmd = if sheet.is_empty() { cmd } else { Cmd::OnSheet { sheet: sheet.clone(), cmd: Box::new(cmd) } };
        b.apply(&cmd)?;
    }
    let (sch, _) = crate::studio::resolve_sheet(b.design(), &sheet);
    Ok(patch_json(&sch, &model))
}

fn pt(p: eda_model::ir::Point) -> Value {
    json!([p.x, p.y])
}

/// The geometry of a sheet's items, in the shapes `GET /api/schematic` reports them (`studio.rs::schematic_json`).
pub(crate) fn patch_json(sch: &SchematicSection, model: &eda_model::ConstraintModel) -> Value {
    use crate::studio::{owner_fields_json, page_field_json};
    // The fields go where their items go: a held symbol shows its Reference and Value with it, a held field shows where it would land.
    let symbols: Vec<Value> = sch
        .symbols
        .iter()
        .map(|s| {
            let owner = eda_model::ir::field_key(&s.id, s.unit);
            json!({ "id": s.id, "unit": s.unit, "at": pt(s.at), "rot": s.rot as f64 / 1000.0, "mirror": if s.mirrored { Some("y") } else if s.mirror_y { Some("x") } else { None }, "fields": owner_fields_json(sch, model, &owner) })
        })
        .collect();
    let labels: Vec<Value> = sch.labels.iter().map(|l| json!({ "id": l.id, "at": pt(l.at), "spin": sch.extras.label_spins.get(&l.id) })).collect();
    json!({
        "ok": true,
        "symbols": symbols,
        "power_symbols": sch.power_symbols.iter().map(|p| json!({ "id": p.id, "at": pt(p.at), "rot": p.rot as f64 / 1000.0, "fields": eda_engine::fields::power_fields(sch, p).iter().map(|f| page_field_json(&p.id, f)).collect::<Vec<_>>() })).collect::<Vec<_>>(),
        "wires": sch.wires.iter().map(|w| json!({ "id": w.id, "net": w.net, "pts": w.pts.iter().map(|p| pt(*p)).collect::<Vec<_>>(), "bus": w.bus })).collect::<Vec<_>>(),
        "labels": labels,
        "texts": sch.texts.iter().map(|t| json!({ "id": t.id, "at": pt(t.at), "angle": t.angle as f64 / 1000.0 })).collect::<Vec<_>>(),
        "no_connects": sch.no_connects.iter().map(|n| json!({ "id": n.id, "at": pt(n.at) })).collect::<Vec<_>>(),
        "bus_entries": sch.bus_entries.iter().map(|b| json!({ "id": b.id, "at": pt(b.at), "size": pt(b.size) })).collect::<Vec<_>>(),
        "junctions": sch.junctions.iter().map(|j| json!({ "id": j.id, "at": pt(j.at) })).collect::<Vec<_>>(),
        "lines": sch.lines.iter().map(|l| json!({ "id": l.id, "pts": l.pts.iter().map(|p| pt(*p)).collect::<Vec<_>>() })).collect::<Vec<_>>(),
        "graphics": serde_json::to_value(&sch.extras.graphics).unwrap_or(Value::Null),
        "sheets": sch.sheets.iter().map(|s| json!({ "id": s.id, "at": pt(s.at), "size": [s.size.0, s.size.1], "pins": s.pins.iter().map(|p| json!({ "id": p.id, "at": pt(p.at) })).collect::<Vec<_>>(), "fields": eda_engine::fields::sheet_fields(sch, s).iter().map(|f| page_field_json(&s.id, f)).collect::<Vec<_>>() })).collect::<Vec<_>>(),
    })
}
