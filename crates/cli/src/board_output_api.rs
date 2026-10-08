//! `/api/fab/{3d,vrml,gencad,ipcd356,ipc2581,odb,pcb_bom}`: the backend for pcbnew's Export and Fabrication
//! Outputs dialogs that kicad-cli has a command for (`web/studio/src/components/BoardExportDialog.tsx`).
//!
//! Same rule as `crate::fab_api`: the export is kicad-cli's, run on the current design revision through
//! `crate::kicad_engine` (and, in the studio, through `offload` and its one-at-a-time lane). Each dialog's JSON
//! becomes kicad-cli arguments in the `*_args` builders below -- the arguments KiCad's own dialog builds, in its
//! order: `DIALOG_EXPORT_STEP::onExportButton` itself runs `kicad-cli pcb export <format> ...`, and
//! `BOARD_EDITOR_CONTROL::ExportVRML`, `ExportGenCAD`, `GenIPC2581File`, `GenerateODBPPFiles` and
//! `GenD356File` map one to one onto the commands of the same name. Output lands in
//! `<dir>/export/kicad/<kind>/`; the reply names the files, relative to `dir`.
//!
//! Not here, because kicad-cli has no command for them: IDF, Hyperlynx, the footprint report (`.rpt`),
//! Specctra DSN/SES (`web/studio/tools/ui-parity-missing.json` says why).

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

/// The centre of the board outline in mm: `BOARD::ComputeBoundingBox( true, true )`'s centre, what the dialogs use
/// for "board centre" (the 3D formats' default origin choice besides the drill origin, and VRML's).
pub(crate) fn board_center_mm(outline: &[eda_model::ir::Point]) -> (f64, f64) {
    if outline.is_empty() {
        return (0.0, 0.0);
    }
    let (x0, x1) = outline.iter().fold((i64::MAX, i64::MIN), |(lo, hi), p| (lo.min(p.x), hi.max(p.x)));
    let (y0, y1) = outline.iter().fold((i64::MAX, i64::MIN), |(lo, hi), p| (lo.min(p.y), hi.max(p.y)));
    ((x0 + x1) as f64 / 2000.0, (y0 + y1) as f64 / 2000.0)
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
    // The dialog's "tolerance" choice: 0.001, 0.01 or 0.1 mm.
    let tolerance = req.get("tolerance").and_then(Value::as_f64).filter(|t| *t > 0.0 && *t <= 1.0).unwrap_or(0.001);
    a.push(format!("--min-distance={tolerance:.3}mm"));
    a.push("-f".into());
    (format, a)
}

/// `BOARD_EDITOR_CONTROL::ExportVRML` with `DIALOG_EXPORT_VRML`'s fields: `{ units: "mm"|"m"|"tenths"|"in",
/// no_unspecified, no_dnp, copy_models, models_dir, relative_paths, user_origin, origin_x, origin_y }`. The origin is
/// always passed: the user's, else the board centre.
pub(crate) fn vrml_args(req: &Value, board_center: (f64, f64)) -> Vec<String> {
    let mut a: Vec<String> = vec!["--units".into(), choice(req, "units", &["mm", "m", "tenths", "in"], "m").into()];
    if flag(req, "no_unspecified", false) {
        a.push("--no-unspecified".into());
    }
    if flag(req, "no_dnp", false) {
        a.push("--no-dnp".into());
    }
    if flag(req, "user_origin", false) {
        a.push(origin_mm(req.get("origin_x").and_then(Value::as_f64).unwrap_or(0.0), req.get("origin_y").and_then(Value::as_f64).unwrap_or(0.0)));
    } else {
        a.push(origin_mm(board_center.0, board_center.1));
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
/// "Supplier and ref"`, one row per value/footprint pair, designators joined with ", " -- as `kicad-cli sch export
/// bom` writes it (the BOM is kicad-cli's; the row number is its ITEM_NUMBER, the last column a field nothing fills).
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

fn outline_center(dir: &Path) -> Result<(f64, f64), String> {
    let (_, design, _) = board::load(dir).map_err(|e| board::reasons(&e))?;
    Ok(board_center_mm(&design.placement.map(|p| p.outline).unwrap_or_default()))
}

/// `POST /api/fab/3d`: STEP, GLB, XAO, BREP, PLY, STL, STPZ, U3D or 3D PDF -- see [`three_d_args`].
pub fn three_d(dir: &Path, body: &[u8]) -> Value {
    let req = request(body);
    let center = match outline_center(dir) {
        Ok(c) => c,
        Err(e) => return fab_api::err(e),
    };
    let (format, args) = three_d_args(&req, center);
    fab_api::reply(kicad_engine::export(dir, format, &args))
}

/// `POST /api/fab/vrml` -- see [`vrml_args`].
pub fn vrml(dir: &Path, body: &[u8]) -> Value {
    let req = request(body);
    let center = match outline_center(dir) {
        Ok(c) => c,
        Err(e) => return fab_api::err(e),
    };
    fab_api::reply(kicad_engine::export(dir, "vrml", &vrml_args(&req, center)))
}

/// `POST /api/fab/gencad` -- see [`gencad_args`].
pub fn gencad(dir: &Path, body: &[u8]) -> Value {
    fab_api::reply(kicad_engine::export(dir, "gencad", &gencad_args(&request(body))))
}

/// `POST /api/fab/ipcd356`: no options.
pub fn ipcd356(dir: &Path) -> Value {
    fab_api::reply(kicad_engine::export(dir, "ipcd356", &[]))
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
pub fn pcb_bom(dir: &Path) -> Value {
    fab_api::reply(kicad_engine::export_sch(dir, "bom", &pcb_bom_args()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use eda_model::ir::Point;
    use serde_json::json;

    fn args(v: Vec<String>) -> String {
        v.join(" ")
    }

    #[test]
    fn the_board_centre_is_the_middle_of_the_outline_in_mm() {
        let outline = [Point { x: 10_000, y: 20_000 }, Point { x: 110_000, y: 20_000 }, Point { x: 110_000, y: 80_000 }, Point { x: 10_000, y: 80_000 }];
        assert_eq!(board_center_mm(&outline), (60.0, 50.0));
        assert_eq!(board_center_mm(&[]), (0.0, 0.0));
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
        for (t, want) in [(json!(0.01), "0.010"), (json!(0), "0.001"), (json!(-1), "0.001"), (json!(5), "0.001"), (json!("fine"), "0.001")] {
            let (_, a) = three_d_args(&json!({ "tolerance": t }), (0.0, 0.0));
            assert!(a.contains(&format!("--min-distance={want}mm")), "{t}: {a:?}");
        }
    }

    #[test]
    fn vrml_takes_units_ignored_parts_and_an_origin_and_embeds_its_models_unless_told_to_copy_them() {
        let a = vrml_args(&Value::Null, (30.0, 40.0));
        assert_eq!(args(a), "--units m --user-origin=30.000000x40.000000mm -f", "metres (the dialog's choice), the board centre");
        let a = vrml_args(&json!({ "units": "tenths", "no_dnp": true, "no_unspecified": true, "user_origin": true, "origin_x": 1.0, "origin_y": 2.0, "copy_models": true, "models_dir": "models", "relative_paths": true }), (30.0, 40.0));
        assert_eq!(args(a), "--units tenths --no-unspecified --no-dnp --user-origin=1.000000x2.000000mm --models-dir models --models-relative -f");
        // Relative paths mean nothing without a folder of models; an empty folder name is the dialog's default.
        assert_eq!(args(vrml_args(&json!({ "relative_paths": true }), (0.0, 0.0))), "--units m --user-origin=0.000000x0.000000mm -f");
        assert!(args(vrml_args(&json!({ "copy_models": true, "models_dir": " " }), (0.0, 0.0))).contains("--models-dir shapes3D"));
        assert!(args(vrml_args(&json!({ "units": "furlongs" }), (0.0, 0.0))).starts_with("--units m "), "an unknown unit falls back");
    }

    #[test]
    fn gencad_options_are_flags_and_nothing_else_is_passed() {
        assert_eq!(args(gencad_args(&Value::Null)), "");
        assert_eq!(args(gencad_args(&json!({ "flip_bottom_pads": true, "unique_pins": true, "unique_footprints": true, "use_drill_origin": true, "store_origin": true }))), "--flip-bottom-pads --unique-pins --unique-footprints --use-drill-origin --store-origin-coord");
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
