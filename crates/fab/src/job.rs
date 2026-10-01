//! Gerber job file (`.gbrjob`) writer.
//!
//! A port of `GERBER_JOBFILE_WRITER` (`pcbnew/exporters/
//! gerber_jobfile_writer.cpp`): the JSON sidecar listing every plotted
//! Gerber file's function/polarity alongside the board's general specs
//! and stackup. Checked field-for-field against a real `kicad-cli`
//! `.gbrjob` from the same board (`Header`/`GeneralSpecs`/`DesignRules`/
//! `FilesAttributes`/`MaterialStackup` -- every key name and nesting
//! matches; `GenerationSoftware` names this writer rather than claiming
//! to be KiCad, which is the one deliberate difference).

use crate::gerber::{FabMeta, PlottedFile};
use eda_model::ir::{Design, Um};
use eda_model::ConstraintModel;
use serde_json::{json, Value};

/// `BOARD_STACKUP`'s default 2-layer thickness (`(thickness 1.6)`,
/// `eda_kicad::pcb`'s own hard-coded value -- this model carries no
/// stackup thickness of its own yet).
const BOARD_THICKNESS_MM: f64 = 1.6;
const COPPER_THICKNESS_MM: f64 = 0.035;
const MASK_THICKNESS_MM: f64 = 0.01;

fn board_size_mm(design: &Design) -> (f64, f64) {
    let Some(pl) = &design.placement else { return (0.0, 0.0) };
    let (mut x0, mut y0, mut x1, mut y1) = (Um::MAX, Um::MAX, Um::MIN, Um::MIN);
    for p in &pl.outline {
        x0 = x0.min(p.x);
        y0 = y0.min(p.y);
        x1 = x1.max(p.x);
        y1 = y1.max(p.y);
    }
    if pl.outline.is_empty() {
        return (0.0, 0.0);
    }
    (((x1 - x0) as f64 / 1000.0 * 100.0).round() / 100.0, ((y1 - y0) as f64 / 1000.0 * 100.0).round() / 100.0)
}

/// Same fold `eda_kicad::pcb::export_kicad_pro` uses for the `.kicad_pro`
/// design-rule floors: the narrowest clearance/track width any net class
/// (or the board default) actually asks for.
fn min_rules(model: &ConstraintModel) -> (f64, f64) {
    let clearance = model.board.net_classes.iter().filter_map(|c| c.clearance).fold(model.board.clearance, Um::min);
    let track = model.board.net_classes.iter().filter_map(|c| c.track_width).fold(model.board.track_width, Um::min);
    (clearance as f64 / 1000.0, track as f64 / 1000.0)
}

fn material_stackup(n_copper: usize) -> Vec<Value> {
    let mut v = vec![
        json!({"Type": "Legend", "Name": "Top Silk Screen"}),
        json!({"Type": "SolderPaste", "Name": "Top Solder Paste"}),
        json!({"Type": "SolderMask", "Thickness": MASK_THICKNESS_MM, "Name": "Top Solder Mask"}),
    ];
    let dielectric_mm = if n_copper > 1 { ((BOARD_THICKNESS_MM - n_copper as f64 * COPPER_THICKNESS_MM) / (n_copper - 1) as f64).max(0.05) } else { 0.0 };
    for i in 0..n_copper {
        let name = if i == 0 { "F.Cu".to_string() } else if i + 1 == n_copper { "B.Cu".to_string() } else { format!("In{i}.Cu") };
        v.push(json!({"Type": "Copper", "Thickness": COPPER_THICKNESS_MM, "Name": name}));
        if i + 1 < n_copper {
            let next = if i + 2 == n_copper { "B.Cu".to_string() } else { format!("In{}.Cu", i + 1) };
            v.push(json!({
                "Type": "Dielectric", "Thickness": (dielectric_mm * 100.0).round() / 100.0, "Material": "FR4",
                "Name": format!("{name}/{next}"),
                "Notes": format!("Type: dielectric layer {} (from {name} to {next})", i + 1),
            }));
        }
    }
    v.push(json!({"Type": "SolderMask", "Thickness": MASK_THICKNESS_MM, "Name": "Bottom Solder Mask"}));
    v.push(json!({"Type": "SolderPaste", "Name": "Bottom Solder Paste"}));
    v.push(json!({"Type": "Legend", "Name": "Bottom Silk Screen"}));
    v
}

/// Build the job file's JSON. `files` is the same slice `gerber::plot_all`
/// returned -- `FilesAttributes` lists them in that same (plotted) order.
pub fn job_json(design: &Design, model: &ConstraintModel, meta: &FabMeta, files: &[PlottedFile]) -> Value {
    let n_copper = model.board.layers.len().max(2);
    let (w, h) = board_size_mm(design);
    let (clearance, track) = min_rules(model);
    json!({
        "Header": {
            "GenerationSoftware": {"Vendor": "EdaFab", "Application": "eda-fab", "Version": meta.generator_version},
            "CreationDate": meta.date,
        },
        "GeneralSpecs": {
            "ProjectId": {"Name": meta.title, "GUID": meta.project_guid(), "Revision": meta.rev},
            "Size": {"X": w, "Y": h},
            "LayerNumber": n_copper,
            "BoardThickness": BOARD_THICKNESS_MM,
            "Finish": "None",
        },
        "DesignRules": [
            {"Layers": "Outer", "PadToPad": clearance, "PadToTrack": clearance, "TrackToTrack": clearance, "MinLineWidth": track}
        ],
        "FilesAttributes": files.iter().map(|f| {
            let mut e = json!({"Path": f.filename, "FileFunction": f.file_function});
            if let Some(p) = f.polarity {
                e["FilePolarity"] = json!(p);
            }
            e
        }).collect::<Vec<_>>(),
        "MaterialStackup": material_stackup(n_copper),
    })
}

/// `job_json`, serialized with the same 2-space indentation `kicad-cli`
/// writes.
pub fn write_job(design: &Design, model: &ConstraintModel, meta: &FabMeta, files: &[PlottedFile]) -> String {
    let mut s = serde_json::to_string_pretty(&job_json(design, model, meta, files)).unwrap_or_default();
    s.push('\n');
    s
}
