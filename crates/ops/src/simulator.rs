//! `Cmd::SetSimulator`: what the SPICE simulator keeps in the design (package P21) -- its project settings (`NGSPICE_SETTINGS`, the
//! `.kicad_pro`'s `schematic.ngspice`) and the workbook Save Workbook wrote (`SIMULATOR_FRAME_UI::SaveWorkbook`, the `.wbk` file's text).
//! KiCad keeps the workbook in a file of the project and the settings in the project file; here both are the design's, and the derived project
//! gets the file and the settings as KiCad writes them (`eda_kicad::export_kicad_pro_for`, `eda_kicad_engine`'s project writer).

use eda_model::ir::Design;
use eda_model::sch_extras::NgspiceSettings;
use eda_model::CheckResult;

type Done = Result<(), Vec<CheckResult>>;

fn fail(check: &str, location: &str, msg: impl Into<String>) -> Vec<CheckResult> {
    vec![CheckResult::fail(check, location, msg.into())]
}

/// The settings and the workbook as given: `None` is KiCad's default settings, and no workbook. A workbook is JSON or a workbook of before
/// version 7 (its first line `version N` or a number, `SIMULATOR_FRAME_UI::LoadWorkbook`); its name is a file of the project's folder.
/// Refused when nothing changes.
pub(crate) fn set_simulator(design: &mut Design, ngspice: Option<&NgspiceSettings>, workbook: Option<&str>) -> Done {
    if let Some(n) = ngspice {
        if n.model_mode > NgspiceSettings::HSPICE {
            return Err(fail("ops_bad_sim_settings", "model_mode", format!("{} is not an ngspice compatibility mode (0 to 5)", n.model_mode)));
        }
        let name = std::path::Path::new(&n.workbook_filename);
        if !n.workbook_filename.is_empty() && (name.is_absolute() || !name.components().all(|c| matches!(c, std::path::Component::Normal(_)))) {
            return Err(fail("ops_bad_sim_settings", &n.workbook_filename, "the workbook is a file of the project's folder"));
        }
    }
    if let Some(text) = workbook {
        let first = text.lines().next().unwrap_or("");
        let legacy = first.starts_with("version ") || first.trim().parse::<i64>().is_ok();
        if !legacy && serde_json::from_str::<serde_json::Value>(text).map(|v| !v.is_object()).unwrap_or(true) {
            return Err(fail("ops_bad_workbook", "workbook", "the workbook is not a simulation workbook's JSON"));
        }
    }
    let sch = design.schematic.get_or_insert_with(crate::empty_schematic_section);
    let ng = ngspice.filter(|n| **n != NgspiceSettings::default()).cloned();
    let wb = workbook.map(str::to_string);
    if sch.extras.ngspice == ng && sch.extras.sim_workbook == wb {
        return Err(fail("ops_nothing_to_change", "simulator", "the simulator's settings and workbook are already these"));
    }
    sch.extras.ngspice = ng;
    sch.extras.sim_workbook = wb;
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::{Board, Cmd};
    use eda_model::ir::{Design, PlacementSection, Point, Provenance};
    use eda_model::sch_extras::NgspiceSettings;
    use eda_model::ConstraintModel;

    fn design() -> Design {
        Design {
            footprint_library: None, sheet_contents: None, bus_aliases: vec![], symbol_library: None,
            schema: 1,
            provenance: Provenance { engine_version: "0".into(), intent_hash: "x".into(), seed: 0, stage_hashes: vec![] },
            schematic: None, nets: None,
            routing: None,
            placement: Some(PlacementSection { outline: vec![Point { x: 0, y: 0 }, Point { x: 50_000, y: 0 }, Point { x: 50_000, y: 50_000 }, Point { x: 0, y: 50_000 }], footprints: vec![], modules: vec![] }),
            drawings: None,
        }
    }

    #[test]
    fn the_simulators_settings_and_workbook_are_one_undoable_step() {
        let m = ConstraintModel::default();
        let mut b = Board::new(design(), &m, 100, 300);
        let settings = NgspiceSettings { workbook_filename: "board.wbk".into(), fix_include_paths: true, model_mode: NgspiceSettings::NGSPICE };
        let wbk = "{\n  \"version\": 7\n}\n".to_string();
        let cmd = Cmd::SetSimulator { ngspice: Some(settings.clone()), workbook: Some(wbk.clone()) };
        // on the wire: through a parsed value, as `/api/cmd` reads it
        let text = serde_json::to_string(&cmd).unwrap();
        assert!(text.contains("\"op\":\"set_simulator\""), "{text}");
        let back: Cmd = serde_json::from_value(serde_json::from_str::<serde_json::Value>(&text).unwrap()).unwrap();
        assert_eq!(back, cmd);
        b.apply(&cmd).unwrap();
        let ex = &b.design().schematic.as_ref().unwrap().extras;
        assert_eq!(ex.ngspice.as_ref(), Some(&settings));
        assert_eq!(ex.sim_workbook.as_deref(), Some(wbk.as_str()));
        assert!(b.apply(&cmd).is_err(), "nothing to change");
        // KiCad's defaults are no settings at all
        b.apply(&Cmd::SetSimulator { ngspice: Some(NgspiceSettings::default()), workbook: Some(wbk.clone()) }).unwrap();
        assert!(b.design().schematic.as_ref().unwrap().extras.ngspice.is_none());
        assert!(b.apply(&Cmd::SetSimulator { ngspice: Some(NgspiceSettings { model_mode: 9, ..settings.clone() }), workbook: None }).is_err());
        assert!(b.apply(&Cmd::SetSimulator { ngspice: Some(NgspiceSettings { workbook_filename: "../x.wbk".into(), ..settings.clone() }), workbook: None }).is_err());
        assert!(b.apply(&Cmd::SetSimulator { ngspice: None, workbook: Some("not json".into()) }).is_err());
        // design.json carries it, and reads it back
        let json = serde_json::to_string(b.design()).unwrap();
        let d: eda_model::ir::Design = serde_json::from_str(&json).unwrap();
        assert_eq!(d.schematic.unwrap().extras.sim_workbook.as_deref(), Some(wbk.as_str()));
    }
}
