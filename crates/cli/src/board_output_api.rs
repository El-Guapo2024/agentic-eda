//! `/api/fab/{3d,vrml,gencad,idf,ipcd356,ipc2581,odb,pcb_bom}`: the backend for pcbnew's Export and Fabrication
//! Outputs dialogs that kicad-cli has a command for (`web/studio/src/components/BoardExportDialog.tsx`).
//!
//! Same rule as `crate::fab_api`: the export is kicad-cli's, run on the current design revision through
//! `crate::kicad_engine` (and, in the studio, through `offload` and its one-at-a-time lane). Each dialog's JSON
//! becomes kicad-cli arguments in the `*_args` builders below -- the arguments KiCad's own dialog builds, in its
//! order: `DIALOG_EXPORT_STEP::onExportButton` itself runs `kicad-cli pcb export <format> ...`, and
//! `BOARD_EDITOR_CONTROL::ExportVRML`, `ExportGenCAD`, `ExportIDF`, `GenIPC2581File`, `GenerateODBPPFiles` and
//! `GenD356File` map one to one onto the commands of the same name. Output lands in
//! `<dir>/export/kicad/<kind>/`; the reply names the files, relative to `dir`.
//!
//! IDF and GenCAD are newer kicad-cli commands than the rest: `GET /api/fab/exporters` ([`exporters`]) says whether the kicad-cli in use
//! has them, and the studio greys out their menu entries with its reason when it has not.
//!
//! Not here, because kicad-cli has no command for them: Hyperlynx and the footprint report (`.rpt`)
//! (`web/studio/tools/ui-parity-missing.json` says why). Specctra DSN/SES is our own exporter (`crate::specctra_api`).

use crate::{board, fab_api, kicad_engine};
use serde_json::Value;
use std::path::Path;

fn request(body: &[u8]) -> Value {
    serde_json::from_slice(body).unwrap_or(Value::Null)
}

fn flag(req: &Value, key: &str, default: bool) -> bool {
    req.get(key).and_then(Value::as_bool).unwrap_or(default)
}

fn text<'a>(req: &'a Value, key: &str) -> &'a str {
    req.get(key).and_then(Value::as_str).map(str::trim).unwrap_or("")
}

/// A whole number in `min..=max`, else `default` (a spin control's range).
fn count(req: &Value, key: &str, default: i64, min: i64, max: i64) -> i64 {
    req.get(key).and_then(Value::as_i64).filter(|n| (min..=max).contains(n)).unwrap_or(default)
}

fn choice<'a>(req: &'a Value, key: &str, allowed: &[&'a str], default: &'a str) -> &'a str {
    allowed.iter().copied().find(|a| Some(*a) == req.get(key).and_then(Value::as_str)).unwrap_or(default)
}

/// `--user-origin` as kicad-cli reads it: `<x>x<y>mm`, six decimals like the dialogs.
fn origin_mm(x: f64, y: f64) -> String {
    format!("--user-origin={x:.6}x{y:.6}mm")
}

/// The board centre in mm: the centre of `BOARD::ComputeBoundingBox( true, true )`, the box of every Edge.Cuts shape of the board and of its
/// footprints (`eda_drc::outline::board_edges_bounding_box`), what `DIALOG_EXPORT_STEP` passes for "Board center origin". (0, 0) with no Edge.Cuts.
pub(crate) fn board_center_mm(design: &eda_model::ir::Design, model: Option<&eda_model::ConstraintModel>) -> (f64, f64) {
    match eda_drc::outline::board_edges_bounding_box(design, model) {
        Some((lo, hi)) => ((lo.x + hi.x).as_f64() / 2e6, (lo.y + hi.y).as_f64() / 2e6),
        None => (0.0, 0.0),
    }
}

/// The formats of `DIALOG_EXPORT_STEP`'s choice (`c_formatCommand`), by their kicad-cli command.
pub(crate) const THREE_D_FORMATS: [&str; 9] = ["step", "glb", "xao", "brep", "ply", "stl", "stpz", "u3d", "3dpdf"];

/// `DIALOG_EXPORT_STEP::onExportButton`: `{ format, board_body, components, tracks, pads, zones, inner_copper,
/// silkscreen, soldermask, fuse_shapes, cut_vias_in_body, fill_all_vias, no_unspecified, no_dnp, subst_models,
/// optimize, net_filter, component_filter, origin: "drill"|"grid"|"user"|"board_center", origin_x, origin_y (mm),
/// tolerance (mm) }` as the kicad-cli format and arguments. Missing fields take the dialog's own defaults.
pub(crate) fn three_d_args(req: &Value, board_center: (f64, f64)) -> (&'static str, Vec<String>) {
    let format = THREE_D_FORMATS.iter().copied().find(|f| Some(*f) == req.get("format").and_then(Value::as_str)).unwrap_or("step");
    let mut a: Vec<String> = Vec::new();
    let mut on = |when: bool, arg: &str| {
        if when {
            a.push(arg.to_string());
        }
    };
    on(flag(req, "no_unspecified", false), "--no-unspecified");
    on(flag(req, "no_dnp", false), "--no-dnp");
    on(flag(req, "subst_models", true), "--subst-models");
    on(!flag(req, "optimize", true), "--no-optimize-step");
    on(!flag(req, "board_body", true), "--no-board-body");
    let components = flag(req, "components", true);
    on(!components, "--no-components");
    on(flag(req, "tracks", false), "--include-tracks");
    on(flag(req, "pads", false), "--include-pads");
    on(flag(req, "zones", false), "--include-zones");
    on(flag(req, "inner_copper", false), "--include-inner-copper");
    on(flag(req, "silkscreen", false), "--include-silkscreen");
    on(flag(req, "soldermask", false), "--include-soldermask");
    on(flag(req, "fuse_shapes", false), "--fuse-shapes");
    on(flag(req, "cut_vias_in_body", false), "--cut-vias-in-body");
    on(flag(req, "fill_all_vias", false), "--fill-all-vias");
    let net_filter = text(req, "net_filter");
    if !net_filter.is_empty() {
        a.push("--net-filter".into());
        a.push(net_filter.into());
    }
    // The component filter only means something while components are exported (the dialog greys it out otherwise).
    let component_filter = text(req, "component_filter");
    if components && !component_filter.is_empty() {
        a.push("--component-filter".into());
        a.push(component_filter.into());
    }
    match choice(req, "origin", &["drill", "grid", "user", "board_center"], "drill") {
        "drill" => a.push("--drill-origin".into()),
        "grid" => a.push("--grid-origin".into()),
        "user" => a.push(origin_mm(req.get("origin_x").and_then(Value::as_f64).unwrap_or(0.0), req.get("origin_y").and_then(Value::as_f64).unwrap_or(0.0))),
        _ => a.push(origin_mm(board_center.0, board_center.1)),
    }
    a.push(format!("--min-distance={:.3}mm", three_d_tolerance(req)));
    a.push("-f".into());
    (format, a)
}

/// The dialog's "tolerance" choice: 0.001, 0.01 or 0.1 mm.
fn three_d_tolerance(req: &Value) -> f64 {
    req.get("tolerance").and_then(Value::as_f64).filter(|t| *t > 0.0 && *t <= 1.0).unwrap_or(0.001)
}

/// `BOARD_EDITOR_CONTROL::ExportVRML` with `DIALOG_EXPORT_VRML`'s fields: `{ units: "mm"|"m"|"tenths"|"in",
/// no_unspecified, no_dnp, copy_models, models_dir, relative_paths, user_origin, origin_x, origin_y }`. "User defined origin" is
/// checked at (0, 0) by default (`dialog_export_vrml_base.cpp`: `m_cbUserDefinedOrigin->SetValue(true)`); unchecked, the origin is the
/// board centre, which kicad-cli works out itself as `ExportVRML` does (`PCBNEW_JOBS_HANDLER::JobExportStep`: no `--user-origin` is
/// `brd->ComputeBoundingBox( true, true )`'s centre), so nothing is passed.
pub(crate) fn vrml_args(req: &Value) -> Vec<String> {
    let mut a: Vec<String> = vec!["--units".into(), choice(req, "units", &["mm", "m", "tenths", "in"], "m").into()];
    if flag(req, "no_unspecified", false) {
        a.push("--no-unspecified".into());
    }
    if flag(req, "no_dnp", false) {
        a.push("--no-dnp".into());
    }
    if flag(req, "user_origin", true) {
        a.push(origin_mm(req.get("origin_x").and_then(Value::as_f64).unwrap_or(0.0), req.get("origin_y").and_then(Value::as_f64).unwrap_or(0.0)));
    }
    // "Copy 3D model files to 3D model path": a folder of models beside the board; without it they are embedded.
    if flag(req, "copy_models", false) {
        let dir = text(req, "models_dir");
        a.push("--models-dir".into());
        a.push(if dir.is_empty() { "shapes3D" } else { dir }.into());
        if flag(req, "relative_paths", false) {
            a.push("--models-relative".into());
        }
    }
    a.push("-f".into());
    a
}

/// `DIALOG_GENCAD_EXPORT_OPTIONS`: `{ flip_bottom_pads, unique_pins, unique_footprints, use_drill_origin,
/// store_origin }`. (`-f` means "flip bottom pads" to this command, not "force".)
pub(crate) fn gencad_args(req: &Value) -> Vec<String> {
    let mut a = Vec::new();
    for (key, arg) in [("flip_bottom_pads", "--flip-bottom-pads"), ("unique_pins", "--unique-pins"), ("unique_footprints", "--unique-footprints"), ("use_drill_origin", "--use-drill-origin"), ("store_origin", "--store-origin-coord")] {
        if flag(req, key, false) {
            a.push(arg.to_string());
        }
    }
    a
}

/// `BOARD_EDITOR_CONTROL::ExportIDF` with `DIALOG_EXPORT_IDF3`'s fields: `{ units: "mm"|"mils" (Output units: Millimeters, Mils),
/// set_reference_point ("Set board reference point:"), x_ref, y_ref (mm), no_dnp ("Ignore 'Do not populate' components"), no_unspecified
/// ("Ignore 'Unspecified' components") }`, every one unset or Millimeters by default (`dialog_export_idf_base.cpp`).
///
/// Without a reference point KiCad's handler takes the centre of `BOARD::GetBoardEdgesBoundingBox`, which is kicad-cli's `--origin center`;
/// with one, `--origin user` and the point. kicad-cli (nightly 25ec2335f2) reads `--user-origin-x/-y` in nanometres with `--units mm` and in
/// 1/25400 mm with `--units mils` (it turns the nanometres into mils and hands those to `Export_IDF3` as millimetres), so the point is given in
/// whichever of the two makes it the millimetres the dialog was given; `crates/cli/tests/cli_exports.rs` holds kicad-cli to that. The values
/// are `--flag=value`, so a negative one is not read as a flag. The file name is the studio's (`export/kicad/idf/<board>.emn`, with the
/// `.emp` beside it). kicad-cli's `--origin drill|grid`, `--explicit-height` and `--part-number-field` have no control in KiCad 8303b2ad's
/// dialog and are not passed.
pub(crate) fn idf_args(req: &Value) -> Vec<String> {
    let mils = choice(req, "units", &["mm", "mils"], "mm") == "mils";
    let mut a: Vec<String> = vec!["--units".into(), if mils { "mils" } else { "mm" }.into()];
    if flag(req, "set_reference_point", false) {
        let per_mm = if mils { 25_400.0 } else { 1_000_000.0 };
        let at = |key: &str| (req.get(key).and_then(Value::as_f64).filter(|v| v.is_finite()).unwrap_or(0.0) * per_mm).round() as i64;
        a.extend(["--origin".into(), "user".into(), format!("--user-origin-x={}", at("x_ref")), format!("--user-origin-y={}", at("y_ref"))]);
    } else {
        a.extend(["--origin".into(), "center".into()]);
    }
    if flag(req, "no_unspecified", false) {
        a.push("--no-unspecified".into());
    }
    if flag(req, "no_dnp", false) {
        a.push("--no-dnp".into());
    }
    a
}

/// `DIALOG_EXPORT_2581`: `{ units: "mm"|"in", precision (4..10), version: "B"|"C", compress, bom_rev, col_id,
/// col_mpn, col_mfg, col_dist_pn, col_dist }` -- the BOM columns are field names, passed only when named.
pub(crate) fn ipc2581_args(req: &Value) -> Vec<String> {
    let mut a: Vec<String> = vec![
        "--units".into(),
        choice(req, "units", &["mm", "in"], "mm").into(),
        "--precision".into(),
        count(req, "precision", 6, 4, 10).to_string(),
        "--version".into(),
        choice(req, "version", &["B", "C"], "C").into(),
    ];
    if flag(req, "compress", false) {
        a.push("--compress".into());
    }
    for (key, arg) in [("bom_rev", "--bom-rev"), ("col_id", "--bom-col-int-id"), ("col_mpn", "--bom-col-mfg-pn"), ("col_mfg", "--bom-col-mfg"), ("col_dist_pn", "--bom-col-dist-pn"), ("col_dist", "--bom-col-dist")] {
        let v = text(req, key);
        if !v.is_empty() {
            a.push(arg.into());
            a.push(v.into());
        }
    }
    a
}

/// `DIALOG_EXPORT_ODBPP`: `{ units: "mm"|"in", precision (2..16), compression: "none"|"zip"|"tgz" }`.
pub(crate) fn odb_args(req: &Value) -> Vec<String> {
    vec![
        "--units".into(),
        choice(req, "units", &["mm", "in"], "mm").into(),
        "--precision".into(),
        count(req, "precision", 6, 2, 16).to_string(),
        "--compression".into(),
        choice(req, "compression", &["none", "zip", "tgz"], "zip").into(),
    ]
}

/// `BOARD_EDITOR_CONTROL::GenBOMFileFromBoard`'s file -- `"Id";"Designator";"Footprint";"Quantity";"Designation";
/// "Supplier and ref"`, one row per value/footprint pair, designators joined with ", " -- as kicad-cli's BOM writes it (the row number is
/// its ITEM_NUMBER, the last column a field nothing fills). Run on the board (`kicad-cli pcb export bom`, `PCBNEW_JOBS_HANDLER::JobExportBom`
/// over `board->Footprints()`), so a footprint that is on the board only (a mounting hole, a logo) is in it as in KiCad's.
///
/// Differs (kicad-cli's writer, not `GenBOMFileFromBoard`'s hand-made one): every value is quoted, the Footprint is the whole library id
/// (KiCad's is `GetLibItemName()`), and the rows have no trailing `;;;`.
pub(crate) fn pcb_bom_args() -> Vec<String> {
    [
        "--fields",
        "ITEM_NUMBER,Reference,Footprint,QUANTITY,Value,Supplier and ref",
        "--labels",
        "Id,Designator,Footprint,Quantity,Designation,Supplier and ref",
        "--group-by",
        "Value,Footprint",
        "--sort-field",
        "Reference",
        "--field-delimiter",
        ";",
        "--ref-delimiter",
        ", ",
        "--ref-range-delimiter",
        "",
    ]
    .map(String::from)
    .to_vec()
}

/// `DIALOG_EXPORT_STEP::onExportButton`'s check before it runs kicad-cli: `BuildBoardPolygonOutlines( board, outline, maxError (0.05 mm),
/// chainingEpsilon (the tolerance), false )`, refused in its words when the outline is missing or does not close.
fn step_outline_refusal(design: &eda_model::ir::Design, model: &eda_model::ConstraintModel, tolerance_mm: f64) -> Option<String> {
    let closes = eda_drc::outline::board_outline_closes(design, Some(model), eda_model::units::mm(0.05).as_f64(), eda_model::units::mm(tolerance_mm));
    (!closes).then(|| format!("Board outline is missing or not closed using {tolerance_mm:.3} mm tolerance.\nRun DRC for a full analysis."))
}

/// `POST /api/fab/3d`: STEP, GLB, XAO, BREP, PLY, STL, STPZ, U3D or 3D PDF -- see [`three_d_args`]. A board whose outline does not close
/// at the dialog's tolerance is refused before kicad-cli runs, as KiCad's dialog refuses it ([`step_outline_refusal`]); kicad-cli
/// itself would warn ("Board outline is malformed. Run DRC for a full analysis.") and write the file anyway.
pub fn three_d(dir: &Path, body: &[u8]) -> Value {
    let req = request(body);
    let (design, model) = match board::load(dir) {
        Ok((_, d, m)) => (d, m),
        Err(e) => return fab_api::err(board::reasons(&e)),
    };
    let (format, args) = three_d_args(&req, board_center_mm(&design, Some(&model)));
    if let Some(why) = step_outline_refusal(&design, &model, three_d_tolerance(&req)) {
        return fab_api::err(why);
    }
    fab_api::reply(kicad_engine::export(dir, format, &args))
}

/// `POST /api/fab/vrml` -- see [`vrml_args`].
pub fn vrml(dir: &Path, body: &[u8]) -> Value {
    fab_api::reply(kicad_engine::export(dir, "vrml", &vrml_args(&request(body))))
}

/// An IDF or GenCAD export's reply: a kicad-cli without the command says so (`kicad_engine::missing_pcb_export`); a run that fails is
/// KiCad's "Failed to create file '%s'." (`ExportIDF`, `ExportGenCAD`), the file relative to the board, with what kicad-cli said after it.
fn optional_export(dir: &Path, kind: &str, ext: &str, args: &[String]) -> Value {
    if let Some(why) = kicad_engine::missing_pcb_export(kind) {
        return fab_api::err(why);
    }
    match kicad_engine::export(dir, kind, args) {
        Ok(v) => v,
        Err(e) => {
            let file = format!("export/kicad/{kind}/{}.{ext}", eda_kicad_engine::file_stem(&kicad_engine::project_name(dir)));
            fab_api::err(format!("Failed to create file '{file}'.\n{}", board::reasons(&e)))
        }
    }
}

/// `POST /api/fab/gencad` -- see [`gencad_args`].
pub fn gencad(dir: &Path, body: &[u8]) -> Value {
    optional_export(dir, "gencad", "cad", &gencad_args(&request(body)))
}

/// `POST /api/fab/idf` -- see [`idf_args`]. Writes `<board>.emn` and `<board>.emp`.
pub fn idf(dir: &Path, body: &[u8]) -> Value {
    optional_export(dir, "idf", "emn", &idf_args(&request(body)))
}

/// `GET /api/fab/exporters`: `{ engine, unavailable: { <kind>: <reason> } }` for the exporters a kicad-cli may lack
/// (`kicad_engine::OPTIONAL_PCB_EXPORTS`), each probed once per process (`eda_kicad_engine::cli_has_export`); a kind it has (or any, with no
/// kicad-cli at all) is not listed.
pub fn exporters() -> Value {
    let engine = eda_kicad_engine::find_cli().map(|c| format!("kicad-cli {}", eda_kicad_engine::cli_version(&c)));
    let unavailable: serde_json::Map<String, Value> = kicad_engine::OPTIONAL_PCB_EXPORTS.iter().filter_map(|(kind, _)| kicad_engine::missing_pcb_export(kind).map(|why| (kind.to_string(), Value::String(why)))).collect();
    serde_json::json!({ "engine": engine, "unavailable": unavailable })
}

/// `POST /api/fab/ipcd356`: no options. On success the reply's `message` is the one `BOARD_EDITOR_CONTROL::GenD356File`
/// (pcbnew/exporters/export_d356.cpp) shows, see [`ipcd356_message`].
pub fn ipcd356(dir: &Path) -> Value {
    ipcd356_message(dir, fab_api::reply(kicad_engine::export(dir, "ipcd356", &[])))
}

/// `GenD356File`'s `DisplayInfoMessage( _( "IPC-D-356 netlist file created:\n'%s'." ), fn.GetFullPath() )` as the reply's `message`, the full
/// path of the file written; kicad-cli's own "Successfully created IPC-D-356 file" (`JobExportIpcD356`, which the GUI never shows) stays in
/// the log. None of the dialog's other exports says anything on success in KiCad, so this is the only one with a message.
fn ipcd356_message(dir: &Path, mut reply: Value) -> Value {
    let ok = reply.get("ok").and_then(Value::as_bool).unwrap_or(false);
    let file = reply.get("files").and_then(Value::as_array).and_then(|f| f.first()).and_then(Value::as_str).map(|f| dir.join(f)).map(|p| std::path::absolute(&p).unwrap_or(p));
    if let (true, Some(file), Some(obj)) = (ok, file, reply.as_object_mut()) {
        obj.insert("message".into(), Value::String(format!("IPC-D-356 netlist file created:\n'{}'.", file.display())));
    }
    reply
}

/// `POST /api/fab/ipc2581` -- see [`ipc2581_args`].
pub fn ipc2581(dir: &Path, body: &[u8]) -> Value {
    fab_api::reply(kicad_engine::export(dir, "ipc2581", &ipc2581_args(&request(body))))
}

/// `POST /api/fab/odb` -- see [`odb_args`].
pub fn odb(dir: &Path, body: &[u8]) -> Value {
    fab_api::reply(kicad_engine::export(dir, "odb", &odb_args(&request(body))))
}

/// `POST /api/fab/pcb_bom`: no options -- see [`pcb_bom_args`].
///
/// A board with no footprint is refused in `GenBOMFileFromBoard`'s words. A kicad-cli with no `pcb export bom` (older than the 10.99
/// nightly) makes it from the derived schematic instead, as before: the board-only footprints are then missing.
pub fn pcb_bom(dir: &Path) -> Value {
    match board::load(dir) {
        Ok((_, design, _)) if design.placement.as_ref().is_none_or(|p| p.footprints.is_empty()) => return fab_api::err("Cannot export BOM: there are no footprints on the PCB."),
        Err(e) => return fab_api::err(board::reasons(&e)),
        Ok(_) => {}
    }
    if kicad_engine::missing_pcb_export("bom").is_some() {
        return fab_api::reply(kicad_engine::export_sch(dir, "bom", &pcb_bom_args()));
    }
    fab_api::reply(kicad_engine::export(dir, "bom", &pcb_bom_args()))
}

#[cfg(test)]
mod tests {
    use eda_model::units::mm;
    use super::*;
    use eda_model::ir::Point;
    use serde_json::json;

    fn args(v: Vec<String>) -> String {
        v.join(" ")
    }

    fn blank() -> eda_model::ir::Design {
        serde_json::from_value(json!({ "provenance": { "engine_version": "t", "intent_hash": "x", "seed": 0, "stage_hashes": [] } })).unwrap()
    }

    #[test]
    fn the_board_centre_is_the_middle_of_the_outline_in_mm() {
        let outline = vec![Point { x: mm(10.0), y: mm(20.0) }, Point { x: mm(110.0), y: mm(20.0) }, Point { x: mm(110.0), y: mm(80.0) }, Point { x: mm(10.0), y: mm(80.0) }];
        let mut d = blank();
        assert_eq!(board_center_mm(&d, None), (0.0, 0.0));
        d.placement = Some(eda_model::ir::PlacementSection { outline, footprints: vec![], modules: vec![] });
        assert_eq!(board_center_mm(&d, None), (60.0, 50.0));
    }

    /// X6 (r2-editors-exports): `ComputeBoundingBox( true, true )` counts every Edge.Cuts shape, not the largest outline only.
    #[test]
    fn the_board_centre_counts_every_edge_cuts_shape() {
        let rect = |id: &str, x0: f64, x1: f64| eda_model::ir::Shape::Rect { id: id.into(), layer: "Edge.Cuts".into(), stroke_width: eda_model::units::Nm::ZERO, filled: false, start: Point { x: mm(x0), y: mm(0.0) }, end: Point { x: mm(x1), y: mm(10.0) } };
        let mut d = blank();
        d.drawings = Some(eda_model::ir::DrawingsSection { shapes: vec![rect("a", 0.0, 10.0), rect("b", 20.0, 30.0)], outline_is_shapes: true, ..Default::default() });
        assert_eq!(board_center_mm(&d, None), (15.0, 5.0));
    }

    /// X4 (r2-editors-exports): KiCad's STEP dialog refuses an outline that does not close at its tolerance, in its words.
    #[test]
    fn a_3d_export_of_an_open_outline_is_refused_in_kicads_words() {
        let seg = |id: &str, a: (f64, f64), b: (f64, f64)| eda_model::ir::Shape::Segment { id: id.into(), layer: "Edge.Cuts".into(), stroke_width: mm(0.1), filled: false, start: Point { x: mm(a.0), y: mm(a.1) }, end: Point { x: mm(b.0), y: mm(b.1) } };
        let mut d = blank();
        d.drawings = Some(eda_model::ir::DrawingsSection { shapes: vec![seg("1", (0.0, 0.0), (10.0, 0.0)), seg("2", (10.0, 0.0), (10.0, 10.0)), seg("3", (10.0, 10.0), (0.0, 10.0))], outline_is_shapes: true, ..Default::default() });
        let model = eda_model::ConstraintModel::default();
        assert_eq!(step_outline_refusal(&d, &model, 0.001).as_deref(), Some("Board outline is missing or not closed using 0.001 mm tolerance.\nRun DRC for a full analysis."));
        d.drawings.as_mut().unwrap().shapes.push(seg("4", (0.0, 10.0), (0.0, 0.0)));
        assert_eq!(step_outline_refusal(&d, &model, 0.001), None, "closed");
        assert!(step_outline_refusal(&blank(), &model, 0.01).unwrap().contains("0.010 mm"), "no outline at all is refused too");
    }

    /// X6-1 (r6-editors-exports): Generate IPC-D-356 says what `GenD356File` says, with the file's full path.
    #[test]
    fn ipcd356_says_what_gend356file_says() {
        let dir = Path::new("/tmp/board");
        let r = ipcd356_message(dir, json!({ "ok": true, "engine": "kicad-cli", "files": ["export/kicad/ipcd356/b.d356"], "log": "Successfully created IPC-D-356 file\n" }));
        assert_eq!(r["message"], "IPC-D-356 netlist file created:\n'/tmp/board/export/kicad/ipcd356/b.d356'.");
        assert_eq!(r["log"], "Successfully created IPC-D-356 file\n");
        let failed = ipcd356_message(dir, json!({ "ok": false, "message": "kicad-cli failed" }));
        assert_eq!(failed["message"], "kicad-cli failed");
    }

    #[test]
    fn the_3d_export_defaults_are_the_dialogs_own() {
        // Board body, components, substituted models, optimisation, drill/place origin, 0.001 mm: STEP's defaults.
        let (format, a) = three_d_args(&Value::Null, (1.0, 2.0));
        assert_eq!(format, "step");
        assert_eq!(args(a), "--subst-models --drill-origin --min-distance=0.001mm -f");
    }

    #[test]
    fn every_3d_option_becomes_the_flag_the_dialog_gives_it_in_the_dialogs_order() {
        let req = json!({
            "format": "glb", "no_unspecified": true, "no_dnp": true, "subst_models": true, "optimize": false, "board_body": false, "components": true,
            "tracks": true, "pads": true, "zones": true, "inner_copper": true, "silkscreen": true, "soldermask": true,
            "fuse_shapes": true, "cut_vias_in_body": true, "fill_all_vias": true,
            "net_filter": " GND* ", "component_filter": "R1,C*", "origin": "grid", "tolerance": 0.1
        });
        let (format, a) = three_d_args(&req, (0.0, 0.0));
        assert_eq!(format, "glb");
        assert_eq!(
            args(a),
            "--no-unspecified --no-dnp --subst-models --no-optimize-step --no-board-body --include-tracks --include-pads --include-zones --include-inner-copper \
             --include-silkscreen --include-soldermask --fuse-shapes --cut-vias-in-body --fill-all-vias --net-filter GND* --component-filter R1,C* --grid-origin --min-distance=0.100mm -f"
        );
    }

    #[test]
    fn the_component_filter_is_dropped_when_no_components_are_exported_and_unknown_formats_fall_back_to_step() {
        let (format, a) = three_d_args(&json!({ "format": "dwg", "components": false, "component_filter": "R1", "subst_models": false }), (0.0, 0.0));
        assert_eq!(format, "step");
        assert_eq!(args(a), "--no-components --drill-origin --min-distance=0.001mm -f");
    }

    #[test]
    fn the_3d_origin_is_the_users_or_the_board_centre_in_mm_with_six_decimals() {
        let (_, a) = three_d_args(&json!({ "origin": "user", "origin_x": 12.5, "origin_y": -3.25 }), (99.0, 99.0));
        assert!(a.contains(&"--user-origin=12.500000x-3.250000mm".to_string()), "{a:?}");
        let (_, a) = three_d_args(&json!({ "origin": "board_center" }), (60.0, 50.5));
        assert!(a.contains(&"--user-origin=60.000000x50.500000mm".to_string()), "{a:?}");
        assert!(!a.iter().any(|x| x == "--drill-origin"));
    }

    #[test]
    fn the_tolerance_is_a_positive_distance_of_at_most_a_millimetre() {
        for (t, want) in [(json!(0.01), "0.010"), (json!(0), "0.001"), (json!(-1), "0.001"), (json!(5000), "0.001"), (json!("fine"), "0.001")] {
            let (_, a) = three_d_args(&json!({ "tolerance": t }), (0.0, 0.0));
            assert!(a.contains(&format!("--min-distance={want}mm")), "{t}: {a:?}");
        }
    }

    #[test]
    fn vrml_takes_units_ignored_parts_and_an_origin_and_embeds_its_models_unless_told_to_copy_them() {
        let a = vrml_args(&Value::Null);
        assert_eq!(args(a), "--units m --user-origin=0.000000x0.000000mm -f", "metres (the dialog's choice), the user origin at (0, 0) (X5)");
        let a = vrml_args(&json!({ "units": "tenths", "no_dnp": true, "no_unspecified": true, "user_origin": true, "origin_x": 1.0, "origin_y": 2.0, "copy_models": true, "models_dir": "models", "relative_paths": true }));
        assert_eq!(args(a), "--units tenths --no-unspecified --no-dnp --user-origin=1.000000x2.000000mm --models-dir models --models-relative -f");
        // Unchecked: kicad-cli takes the board centre itself (`ComputeBoundingBox( true, true )`).
        assert_eq!(args(vrml_args(&json!({ "user_origin": false }))), "--units m -f");
        // Relative paths mean nothing without a folder of models; an empty folder name is the dialog's default.
        assert_eq!(args(vrml_args(&json!({ "relative_paths": true }))), "--units m --user-origin=0.000000x0.000000mm -f");
        assert!(args(vrml_args(&json!({ "copy_models": true, "models_dir": " " }))).contains("--models-dir shapes3D"));
        assert!(args(vrml_args(&json!({ "units": "furlongs" }))).starts_with("--units m "), "an unknown unit falls back");
    }

    #[test]
    fn gencad_options_are_flags_and_nothing_else_is_passed() {
        assert_eq!(args(gencad_args(&Value::Null)), "");
        assert_eq!(args(gencad_args(&json!({ "flip_bottom_pads": true, "unique_pins": true, "unique_footprints": true, "use_drill_origin": true, "store_origin": true }))), "--flip-bottom-pads --unique-pins --unique-footprints --use-drill-origin --store-origin-coord");
    }

    #[test]
    fn idf_defaults_to_millimetres_the_board_centre_and_every_component() {
        assert_eq!(args(idf_args(&Value::Null)), "--units mm --origin center");
        // The reference point is only read while "Set board reference point:" is checked.
        assert_eq!(args(idf_args(&json!({ "x_ref": 5.0, "y_ref": 6.0 }))), "--units mm --origin center");
    }

    #[test]
    fn every_idf_option_becomes_its_kicad_cli_flag() {
        let a = idf_args(&json!({ "units": "mm", "set_reference_point": true, "x_ref": 12.5, "y_ref": -3.25, "no_unspecified": true, "no_dnp": true }));
        assert_eq!(args(a), "--units mm --origin user --user-origin-x=12500000 --user-origin-y=-3250000 --no-unspecified --no-dnp", "the point in nanometres, as kicad-cli reads it with mm");
        let a = idf_args(&json!({ "units": "mils", "set_reference_point": true, "x_ref": 1.0, "y_ref": -0.5 }));
        assert_eq!(args(a), "--units mils --origin user --user-origin-x=25400 --user-origin-y=-12700", "with mils kicad-cli reads the point in 1/25400 mm");
        assert_eq!(args(idf_args(&json!({ "units": "inches", "no_dnp": true }))), "--units mm --origin center --no-dnp", "an unknown unit is Millimeters");
        assert!(args(idf_args(&json!({ "set_reference_point": true, "x_ref": "far" }))).contains("--user-origin-x=0 --user-origin-y=0"));
    }

    #[test]
    fn ipc2581_defaults_to_mm_six_digits_version_c_and_names_only_the_bom_columns_it_is_given() {
        assert_eq!(args(ipc2581_args(&Value::Null)), "--units mm --precision 6 --version C");
        let a = ipc2581_args(&json!({ "units": "in", "precision": 8, "version": "B", "compress": true, "bom_rev": "A", "col_mpn": "MPN", "col_mfg": "Manufacturer", "col_dist_pn": " ", "col_dist": "DigiKey", "col_id": "ID" }));
        assert_eq!(args(a), "--units in --precision 8 --version B --compress --bom-rev A --bom-col-int-id ID --bom-col-mfg-pn MPN --bom-col-mfg Manufacturer --bom-col-dist DigiKey");
        assert!(args(ipc2581_args(&json!({ "precision": 99, "version": "Z", "units": "cm" }))).starts_with("--units mm --precision 6 --version C"), "out of range falls back");
    }

    #[test]
    fn odb_defaults_to_mm_zip_and_a_precision_between_two_and_sixteen() {
        assert_eq!(args(odb_args(&Value::Null)), "--units mm --precision 6 --compression zip");
        assert_eq!(args(odb_args(&json!({ "units": "in", "precision": 2, "compression": "none" }))), "--units in --precision 2 --compression none");
        assert_eq!(args(odb_args(&json!({ "precision": 1, "compression": "rar" }))), "--units mm --precision 6 --compression zip");
    }

    #[test]
    fn the_pcb_bom_has_pcbnews_columns_grouped_by_value_and_footprint() {
        let a = pcb_bom_args();
        let at = |flag: &str| a[a.iter().position(|x| x == flag).unwrap() + 1].clone();
        assert_eq!(at("--labels"), "Id,Designator,Footprint,Quantity,Designation,Supplier and ref", "GenBOMFileFromBoard's header");
        assert_eq!(at("--fields").split(',').count(), at("--labels").split(',').count());
        assert_eq!((at("--group-by"), at("--field-delimiter"), at("--ref-delimiter"), at("--ref-range-delimiter")), ("Value,Footprint".to_string(), ";".to_string(), ", ".to_string(), String::new()));
    }
}
