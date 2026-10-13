//! The 3D models of a footprint, as KiCad's 3D Models tab edits them.
//!
//! A footprint keeps a list of `(model ...)` entries (`FOOTPRINT::Models()`, [`Model3d`]): a file name and how the model sits on the footprint -- an offset, a
//! scale, a rotation, an opacity and a "show" switch. The tab of the Footprint Properties dialogs (`PANEL_FP_PROPERTIES_3D_MODEL`,
//! `pcbnew/dialogs/panel_fp_properties_3d_model.cpp`) adds and removes rows and edits the file name and the Show cell of each; the preview beside it
//! (`PANEL_PREVIEW_3D_MODEL`, `3d-viewer/dialogs/panel_preview_3d_model.cpp`) edits the scale, rotation, offset and opacity of the selected row.
//! This module is that tab's list and its checks, over the list a footprint keeps, with no dialog in it: the board's verbs
//! (`eda_ops::Cmd::EditBoardModels` and its kin) and the Footprint Editor's call it, and so do the tests.
//!
//! # The frame the numbers are in
//!
//! The placement is the footprint file's own, millimetres and degrees, applied in the model's frame before the footprint's position, rotation and side
//! (`RENDER_3D_OPENGL::get3dModelsFromFootprint`, [`Model3d`]). The tab shows the rotation counter-clockwise where the file stores it clockwise (`PANEL_PREVIEW_3D_MODEL`: "Rotation is
//! stored in the file as positive-is-CW, but we use positive-is-CCW in the GUI"): that sign is the dialog's, and a list held here is the file's.
//!
//! # A footprint on the board
//!
//! A footprint on the board takes the list from its library footprint or from the board it was read from. What the 3D Models tab sets on it is
//! [`crate::fp_edit::FootprintEdit::models3d`], the whole list, laid over that footprint by [`crate::fp_edit::edited_footprint`] and recorded in
//! [`crate::ConstraintModel::edited_models`] so that an emptied list stays empty (the 3D view would otherwise find the package's generic model).

use crate::footprint::{Footprint, Model3d};

/// `MAX_SCALE` of the preview panel (`3d-viewer/dialogs/panel_preview_3d_model.h`): the scale fields and their spin buttons keep a scale between this and its inverse.
pub const MAX_SCALE: f64 = 10_000.0;
/// `MAX_OFFSET` of the preview panel, millimetres: the offset fields keep an offset between plus and minus this.
pub const MAX_OFFSET_MM: f64 = 1_000.0;

/// Why a model's file name cannot be used (`MODEL_VALIDATE_ERRORS`, `PANEL_FP_PROPERTIES_3D_MODEL::updateValidateStatus`). That the file is there, or can be opened, is
/// another matter: the tab shows a missing file with an icon and keeps the row (a footprint is made on one machine and read on another), so it is not a problem here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathProblem {
    /// `NO_FILENAME`.
    NoFilename,
    /// `ILLEGAL_FILENAME`.
    Illegal,
}

impl PathProblem {
    /// The words of the tab's status cell.
    pub fn message(self) -> &'static str {
        match self {
            PathProblem::NoFilename => "No filename entered",
            PathProblem::Illegal => "Illegal filename",
        }
    }
}

/// `PANEL_FP_PROPERTIES_3D_MODEL::cleanupFilename`: a file name loses the line breaks and tabs a paste brings (the legacy `ALIAS:path` form that function marks with a
/// leading colon is left as typed: KiCad 6 and later write `${VARIABLE}/path`).
pub fn cleanup_path(name: &str) -> String {
    name.chars().filter(|c| !matches!(c, '\n' | '\r' | '\t')).collect()
}

/// `FILENAME_RESOLVER::ValidateFileName` (`common/filename_resolver.cpp`), as it reads on macOS and Linux (`wxFileName::GetForbiddenChars` there is `*` and `?`):
/// a name is `[ALIAS:]path`, `${VARIABLE}/path` or a `file://` / `kicad-embed://` URI that names something; the alias may not hold `{}[]()%~<>"='`;:.,&?/\|$`, and the
/// path may not hold `*` or `?`. The name must not be empty and must not end in a colon.
pub fn validate_path(name: &str) -> Result<(), PathProblem> {
    if name.is_empty() {
        return Err(PathProblem::NoFilename);
    }
    // an embedded file or a file URI: valid when it names something that is not a path from the root
    for prefix in ["file://", "kicad-embed://"] {
        if let Some(rest) = name.strip_prefix(prefix) {
            return if rest.chars().next().is_some_and(|c| c != '/') { Ok(()) } else { Err(PathProblem::Illegal) };
        }
    }
    let chars: Vec<char> = name.chars().collect();
    let alias_start = usize::from(chars[0] == ':');
    let alias_end = chars.iter().skip(alias_start).position(|c| *c == ':').map(|i| i + alias_start);
    // names may not end with ':'
    if alias_end == Some(chars.len() - 1) {
        return Err(PathProblem::Illegal);
    }
    let path: String = match alias_end {
        Some(end) => {
            // the alias must not be empty
            if end == alias_start {
                return Err(PathProblem::Illegal);
            }
            // `filename.substr( aliasStart, aliasEnd )`: a count, not an end, so a name that starts with a colon reads one character further and always fails here
            let upto = (alias_start + end).min(chars.len());
            if chars[alias_start..upto].iter().any(|c| "{}[]()%~<>\"='`;:.,&?/\\|$".contains(*c)) {
                return Err(PathProblem::Illegal);
            }
            chars[end + 1..].iter().collect()
        }
        None => {
            // `${VARIABLE}/path` and `$(VARIABLE)/path`: the variable is taken off before the path is tested
            let close = if name.starts_with("${") {
                chars.iter().position(|c| *c == '}')
            } else if name.starts_with("$(") {
                chars.iter().position(|c| *c == ')')
            } else {
                None
            };
            match close {
                Some(i) => chars[i + 1..].iter().collect(),
                None => name.to_string(),
            }
        }
    };
    if path.contains(['*', '?']) {
        return Err(PathProblem::Illegal);
    }
    Ok(())
}

/// Why a model cannot be written, or a list operation cannot be made: a `check` code for the board's refusal and the reason in words.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelProblem {
    /// `ops_bad_model` for a model that cannot be written, `ops_bad_model_index` for a position the list has none at, `ops_nothing_to_change` for an operation that changes nothing.
    pub check: &'static str,
    pub reason: String,
}

impl ModelProblem {
    fn bad(reason: impl Into<String>) -> Self {
        ModelProblem { check: "ops_bad_model", reason: reason.into() }
    }
    fn index(reason: impl Into<String>) -> Self {
        ModelProblem { check: "ops_bad_model_index", reason: reason.into() }
    }
    fn unchanged(reason: impl Into<String>) -> Self {
        ModelProblem { check: "ops_nothing_to_change", reason: reason.into() }
    }
}

fn plural(n: usize) -> String {
    match n {
        0 => "no 3D models".to_string(),
        1 => "1 3D model".to_string(),
        n => format!("{n} 3D models"),
    }
}

/// The numbers the preview panel's fields take (`panel_preview_3d_model.h`): an offset up to 1000 mm either way, a scale between 1/10000 and 10000 (negative allowed: a model
/// can be mirrored), any finite rotation and an opacity from 0 to 1 (the slider's 0 to 100%).
fn check_numbers(m: &Model3d) -> Result<(), ModelProblem> {
    let finite = |v: &[f64; 3]| v.iter().all(|x| x.is_finite());
    if !finite(&m.offset) || !finite(&m.scale) || !finite(&m.rotate) || !m.opacity.is_finite() {
        return Err(ModelProblem::bad("the offset, scale, rotation and opacity must be numbers"));
    }
    if let Some(v) = m.offset.iter().find(|v| v.abs() > MAX_OFFSET_MM) {
        return Err(ModelProblem::bad(format!("the offset {v} mm is out of range: it is at most {MAX_OFFSET_MM} mm either way")));
    }
    if let Some(v) = m.scale.iter().find(|v| v.abs() > MAX_SCALE || v.abs() < 1.0 / MAX_SCALE) {
        return Err(ModelProblem::bad(format!("the scale {v} is out of range: it is between {} and {MAX_SCALE}", 1.0 / MAX_SCALE)));
    }
    if !(0.0..=1.0).contains(&m.opacity) {
        return Err(ModelProblem::bad(format!("the opacity {} is out of range: it is between 0 and 1", m.opacity)));
    }
    Ok(())
}

/// One model of a list, after [`cleanup_path`]: its file name must be a legal one and its numbers the ones the panel takes.
fn check_model(m: &Model3d) -> Result<(), ModelProblem> {
    validate_path(&m.path).map_err(|p| ModelProblem::bad(format!("{}{}", p.message(), if m.path.is_empty() { String::new() } else { format!(": {:?}", m.path) })))?;
    check_numbers(m)
}

/// `model` with its file name cleaned, or why it cannot be written.
pub fn checked_model(model: &Model3d) -> Result<Model3d, ModelProblem> {
    let mut m = model.clone();
    m.path = cleanup_path(&m.path);
    check_model(&m)?;
    Ok(m)
}

/// The whole list, every file name cleaned; the first model that cannot be written is named by its position (1-based, as the tab's rows are counted by a person).
pub fn checked_models(list: &[Model3d]) -> Result<Vec<Model3d>, ModelProblem> {
    list.iter()
        .enumerate()
        .map(|(i, m)| checked_model(m).map_err(|p| ModelProblem { reason: format!("model {}: {}", i + 1, p.reason), ..p }))
        .collect()
}

/// What an edit of one model changes; a field left `None` stays (the grid's file name and Show cells, the preview panel's scale, rotation, offset and opacity fields).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ModelPatch {
    pub path: Option<String>,
    pub offset: Option<[f64; 3]>,
    pub scale: Option<[f64; 3]>,
    pub rotate: Option<[f64; 3]>,
    pub opacity: Option<f64>,
    pub show: Option<bool>,
}

impl ModelPatch {
    pub fn is_empty(&self) -> bool {
        *self == ModelPatch::default()
    }

    /// `m` with the patch laid on.
    pub fn applied(&self, m: &Model3d) -> Model3d {
        Model3d {
            path: self.path.clone().unwrap_or_else(|| m.path.clone()),
            offset: self.offset.unwrap_or(m.offset),
            scale: self.scale.unwrap_or(m.scale),
            rotate: self.rotate.unwrap_or(m.rotate),
            opacity: self.opacity.unwrap_or(m.opacity),
            show: self.show.unwrap_or(m.show),
        }
    }
}

fn no_such_model(index: usize, len: usize) -> ModelProblem {
    ModelProblem::index(if len == 0 { format!("there is no model {}: the footprint has no 3D models", index + 1) } else { format!("there is no model {}: the footprint has {}", index + 1, plural(len)) })
}

/// Add a model at the end of the list, or before position `at` (0-based, up to the list's length). Returns the position it landed at. The
/// tab's "Add 3D model" (`OnAdd3DModel`) appends; a position is for a caller that orders the list.
pub fn add_model(list: &mut Vec<Model3d>, model: &Model3d, at: Option<usize>) -> Result<usize, ModelProblem> {
    let model = checked_model(model)?;
    let at = at.unwrap_or(list.len());
    if at > list.len() {
        return Err(ModelProblem::index(format!("a model cannot be added at position {}: the footprint has {}", at + 1, plural(list.len()))));
    }
    list.insert(at, model);
    Ok(at)
}

/// Take the model at `index` (0-based) out of the list (`OnRemove3DModel`).
pub fn remove_model(list: &mut Vec<Model3d>, index: usize) -> Result<Model3d, ModelProblem> {
    if index >= list.len() {
        return Err(no_such_model(index, list.len()));
    }
    Ok(list.remove(index))
}

/// Put the model at `from` where `to` is, the others keeping their order: the list as it reads once the model is there.
pub fn move_model(list: &mut Vec<Model3d>, from: usize, to: usize) -> Result<(), ModelProblem> {
    for i in [from, to] {
        if i >= list.len() {
            return Err(no_such_model(i, list.len()));
        }
    }
    if from == to {
        return Err(ModelProblem::unchanged(format!("model {} is where it is asked to go", from + 1)));
    }
    let m = list.remove(from);
    list.insert(to, m);
    Ok(())
}

/// Change the model at `index` by `patch`. A patch that says nothing, or that leaves the model as it is, is refused: it would be an empty undo step.
pub fn edit_model(list: &mut [Model3d], index: usize, patch: &ModelPatch) -> Result<(), ModelProblem> {
    let Some(was) = list.get(index) else { return Err(no_such_model(index, list.len())) };
    if patch.is_empty() {
        return Err(ModelProblem::unchanged("give a file name, an offset, a scale, a rotation, an opacity or a show switch"));
    }
    let now = checked_model(&patch.applied(was))?;
    if now == *was {
        return Err(ModelProblem::unchanged(format!("model {} is as it was", index + 1)));
    }
    list[index] = now;
    Ok(())
}

/// Give `footprint` the list `models`: `models3d` is the whole list and `model` its first path (`Footprint::models3d`). An empty list leaves it with no model at all.
pub fn set_models(footprint: &mut Footprint, models: &[Model3d]) {
    footprint.models3d = models.to_vec();
    footprint.model = models.first().map(|m| m.path.clone());
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m(path: &str) -> Model3d {
        Model3d::identity(path)
    }

    fn paths(list: &[Model3d]) -> Vec<&str> {
        list.iter().map(|m| m.path.as_str()).collect()
    }

    #[test]
    fn a_file_name_is_checked_as_the_filename_resolver_checks_it() {
        // `ValidateFileName`, one case per rule of its header
        for ok in ["${KICAD10_3DMODEL_DIR}/Resistor_SMD.3dshapes/R_0603_1608Metric.step", "$(KISYS3DMOD)/x.wrl", "models/x.step", "/Users/jeff/kicad/Package_QFP.3dshapes/TQFP-48.wrl", "C:/models/x.step", "ALIAS:rel/path.step", "file://host/x.step", "kicad-embed://x.step"] {
            assert_eq!(validate_path(ok), Ok(()), "{ok}");
        }
        assert_eq!(validate_path(""), Err(PathProblem::NoFilename));
        for bad in ["a:", "x*.step", "models/x?.step", "${DIR}/x*.step", "AL.IAS:x.step", "AL{I}AS:x.step", "AL IAS|:x", "file:///x.step", "kicad-embed:///x.step", "file://", ":ALIAS:x.step"] {
            assert_eq!(validate_path(bad), Err(PathProblem::Illegal), "{bad:?}");
        }
        // the words of the status cell
        assert_eq!((PathProblem::NoFilename.message(), PathProblem::Illegal.message()), ("No filename entered", "Illegal filename"));
    }

    #[test]
    fn a_pasted_name_loses_its_line_breaks_and_tabs() {
        assert_eq!(cleanup_path("${KICAD10_3DMODEL_DIR}/a\r\n.3dshapes/\tb.step\n"), "${KICAD10_3DMODEL_DIR}/a.3dshapes/b.step");
        assert_eq!(cleanup_path("plain.step"), "plain.step");
    }

    #[test]
    fn a_model_is_added_at_the_end_or_before_a_position_and_removed_by_position() {
        let mut list = vec![m("a.step")];
        assert_eq!(add_model(&mut list, &m("b.step"), None), Ok(1));
        assert_eq!(add_model(&mut list, &m("c.step"), Some(0)), Ok(0));
        assert_eq!(paths(&list), ["c.step", "a.step", "b.step"]);
        assert_eq!(add_model(&mut list, &m("d.step"), Some(3)), Ok(3), "the position after the last is the end");
        assert_eq!(remove_model(&mut list, 1).unwrap().path, "a.step");
        assert_eq!(paths(&list), ["c.step", "b.step", "d.step"]);
        // the file name is cleaned on the way in
        add_model(&mut list, &m("e\n.step"), None).unwrap();
        assert_eq!(list.last().unwrap().path, "e.step");
    }

    #[test]
    fn a_position_the_list_has_none_at_is_refused_in_words() {
        let mut list = vec![m("a.step"), m("b.step")];
        let before = list.clone();
        let e = add_model(&mut list, &m("c.step"), Some(3)).unwrap_err();
        assert_eq!((e.check, e.reason.as_str()), ("ops_bad_model_index", "a model cannot be added at position 4: the footprint has 2 3D models"));
        let e = remove_model(&mut list, 2).unwrap_err();
        assert_eq!((e.check, e.reason.as_str()), ("ops_bad_model_index", "there is no model 3: the footprint has 2 3D models"));
        assert_eq!(move_model(&mut list, 0, 2).unwrap_err().check, "ops_bad_model_index");
        assert_eq!(move_model(&mut list, 5, 0).unwrap_err().check, "ops_bad_model_index");
        assert_eq!(edit_model(&mut list, 9, &ModelPatch { show: Some(false), ..Default::default() }).unwrap_err().check, "ops_bad_model_index");
        assert_eq!(remove_model(&mut Vec::new(), 0).unwrap_err().reason, "there is no model 1: the footprint has no 3D models");
        assert_eq!(list, before, "a refused operation leaves the list alone");
    }

    #[test]
    fn a_model_moves_to_a_position_and_the_others_keep_their_order() {
        let mut list = vec![m("a"), m("b"), m("c"), m("d")];
        move_model(&mut list, 3, 0).unwrap();
        assert_eq!(paths(&list), ["d", "a", "b", "c"]);
        move_model(&mut list, 0, 2).unwrap();
        assert_eq!(paths(&list), ["a", "b", "d", "c"]);
        assert_eq!(move_model(&mut list, 1, 1).unwrap_err().check, "ops_nothing_to_change");
    }

    #[test]
    fn a_model_is_edited_a_field_at_a_time_and_an_edit_that_changes_nothing_is_refused() {
        let mut list = vec![m("a.step"), m("b.step")];
        let patch = ModelPatch { offset: Some([1.0, -2.0, 0.5]), scale: Some([2.0, 2.0, 2.0]), rotate: Some([90.0, 0.0, -45.0]), opacity: Some(0.5), show: Some(false), path: Some("c.step".into()) };
        edit_model(&mut list, 1, &patch).unwrap();
        assert_eq!(list[1], Model3d { path: "c.step".into(), offset: [1.0, -2.0, 0.5], scale: [2.0, 2.0, 2.0], rotate: [90.0, 0.0, -45.0], opacity: 0.5, show: false });
        assert_eq!(list[0], m("a.step"), "the other model is untouched");
        // one field leaves the others
        edit_model(&mut list, 1, &ModelPatch { show: Some(true), ..Default::default() }).unwrap();
        assert_eq!((list[1].show, list[1].offset, list[1].opacity), (true, [1.0, -2.0, 0.5], 0.5));
        let e = edit_model(&mut list, 1, &ModelPatch { show: Some(true), ..Default::default() }).unwrap_err();
        assert_eq!((e.check, e.reason.as_str()), ("ops_nothing_to_change", "model 2 is as it was"));
        assert_eq!(edit_model(&mut list, 1, &ModelPatch::default()).unwrap_err().check, "ops_nothing_to_change");
    }

    #[test]
    fn a_name_or_a_number_the_panel_would_not_take_is_refused_with_its_reason() {
        let mut list = vec![m("a.step")];
        let bad = |list: &mut Vec<Model3d>, model: Model3d| add_model(list, &model, None).unwrap_err();
        let e = bad(&mut list, m(""));
        assert_eq!((e.check, e.reason.as_str()), ("ops_bad_model", "No filename entered"));
        assert_eq!(bad(&mut list, m("x*.step")).reason, "Illegal filename: \"x*.step\"");
        assert!(bad(&mut list, Model3d { scale: [0.0, 1.0, 1.0], ..m("x.step") }).reason.contains("scale 0 is out of range"));
        assert!(bad(&mut list, Model3d { scale: [1.0, 20_000.0, 1.0], ..m("x.step") }).reason.contains("scale 20000 is out of range"));
        assert!(bad(&mut list, Model3d { offset: [0.0, 0.0, 1_001.0], ..m("x.step") }).reason.contains("offset 1001 mm is out of range"));
        assert!(bad(&mut list, Model3d { opacity: 1.5, ..m("x.step") }).reason.contains("opacity 1.5 is out of range"));
        assert!(bad(&mut list, Model3d { rotate: [f64::NAN, 0.0, 0.0], ..m("x.step") }).reason.contains("must be numbers"));
        // a scale that mirrors the model is a scale
        add_model(&mut list, &Model3d { scale: [-1.0, 1.0, 1.0], ..m("mirrored.step") }, None).unwrap();
        // an edit is judged as the model it makes
        assert!(edit_model(&mut list, 0, &ModelPatch { opacity: Some(2.0), ..Default::default() }).unwrap_err().reason.contains("opacity 2 is out of range"));
        assert_eq!(edit_model(&mut list, 0, &ModelPatch { path: Some(String::new()), ..Default::default() }).unwrap_err().reason, "No filename entered");
        assert_eq!(list.len(), 2);
        assert_eq!(list[0], m("a.step"), "nothing of a refused edit stays");
    }

    #[test]
    fn a_whole_list_is_checked_and_the_first_bad_model_is_named_by_its_row() {
        let ok = checked_models(&[m("a\n.step"), m("b.step")]).unwrap();
        assert_eq!(paths(&ok), ["a.step", "b.step"]);
        let e = checked_models(&[m("a.step"), m("b.step"), m("c?.step")]).unwrap_err();
        assert_eq!((e.check, e.reason.as_str()), ("ops_bad_model", "model 3: Illegal filename: \"c?.step\""));
        assert!(checked_models(&[]).unwrap().is_empty(), "no models is a list");
    }

    #[test]
    fn a_library_footprint_keeps_its_list_and_one_from_before_the_list_reads_as_it_did() {
        use crate::ir::LibraryFootprint;
        // a design.json from before the list: the one path
        let old: LibraryFootprint = serde_json::from_str(r#"{"name":"Lib:X","model":"a.step"}"#).unwrap();
        assert_eq!((old.model.as_deref(), old.models3d.len()), (Some("a.step"), 0));
        assert!(!serde_json::to_string(&old).unwrap().contains("models3d"), "nothing to write when there is no list");
        assert_eq!(old.to_engine_footprint().model.as_deref(), Some("a.step"));
        // a list goes through the file and comes back, and through the engine's footprint and back
        let mut fp = LibraryFootprint::new_empty("Lib:Y");
        fp.models3d = vec![Model3d { offset: [1.5, 0.0, 0.25], rotate: [90.0, 0.0, 0.0], opacity: 0.5, ..m("a.step") }, Model3d { show: false, ..m("b.step") }];
        fp.model = Some("a.step".into());
        let text = serde_json::to_string(&fp).unwrap();
        assert_eq!(serde_json::from_str::<LibraryFootprint>(&text).unwrap().models3d, fp.models3d);
        let engine = fp.to_engine_footprint();
        assert_eq!((engine.models3d.clone(), engine.model.clone()), (fp.models3d.clone(), Some("a.step".to_string())));
        assert_eq!(LibraryFootprint::from_engine_footprint(&engine).models3d, fp.models3d);
        // the list leads: `model` follows its first path even when the entry says another
        fp.model = Some("stale.step".into());
        assert_eq!(fp.to_engine_footprint().model.as_deref(), Some("a.step"));
    }

    #[test]
    fn a_footprint_given_a_list_has_it_whole_and_its_first_path_as_the_single_model() {
        let mut fp = Footprint { name: "X".into(), pads: vec![], courtyard: None, courtyard_outlines: vec![], model: Some("old.step".into()), models3d: vec![], art: Default::default() };
        set_models(&mut fp, &[m("a.step"), m("b.step")]);
        assert_eq!((fp.model.as_deref(), paths(&fp.models3d)), (Some("a.step"), vec!["a.step", "b.step"]));
        set_models(&mut fp, &[]);
        assert_eq!((fp.model, fp.models3d.len()), (None, 0), "an emptied list leaves no model, not the old path");
    }
}
