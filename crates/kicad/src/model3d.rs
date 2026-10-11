//! The 3D models of a placed part, as the 3D view and the exported board show them.
//!
//! A footprint's `(model ...)` lines are KiCad's: the file, and how it sits on the footprint (offset, scale, rotation). Where they come from, first match
//! wins:
//!   1. the footprint the board holds for the part (`ConstraintModel::footprints`: a library footprint read for its pads, an imported board's own
//!      footprint) -- its `models3d`, else its single `model` path as it comes;
//!   2. the installed KiCad footprint the part names (`Lib:Name`), read from the library on disk;
//!   3. the KiCad footprint its generic package name stands for (`eda_model::footprint::kicad_footprint_for`: `0603` on an `R` is
//!      `Resistor_SMD:R_0603_1608Metric`, `PINHEADER-4` is a 1x04 pin header), read from the installed library, so the model has the placement its own
//!      footprint gives it (a connector's model is often turned or lifted) and not the identity.
//!
//! For (2) and (3) the part's pads are the studio's own built-in ones, not the library footprint's: a built-in 1x04 header runs along x with its centre
//! at the origin, KiCad's 1x04 header runs along y from pin 1. The library's model would stand across the pads. [`fit_pads`] finds the turn and shift that
//! put the library's pads on the part's (matched by pad number, least squares) and folds it into the model's placement; when the pads do not agree
//! closely enough for that to mean anything, the model keeps its own placement.
//!
//! Installed footprints are read once and kept.

use crate::footprint_lib::{default_footprint_library_root, find_footprint_file, footprint_models};
use eda_model::footprint::{kicad_footprint_of_part, Model3d};
use eda_model::{ConstraintModel, Footprint, Part, PadKind};
use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock, PoisonError};

/// How KiCad's 3D viewer sorts a footprint's models (`BOARD_ADAPTER::IsFootprintShown`): `smd` is an SMD model, `through_hole` a through-hole model, and a
/// footprint with neither is a virtual one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelKind {
    Smd,
    ThroughHole,
    Virtual,
}

impl ModelKind {
    /// The word the state JSON and the 3D viewer's layer tree use.
    pub fn as_str(self) -> &'static str {
        match self {
            ModelKind::Smd => "smd",
            ModelKind::ThroughHole => "tht",
            ModelKind::Virtual => "virtual",
        }
    }

    /// The kind of a footprint with no `(attr ...)` to say: through-hole when it has a plated through-hole pad, SMD when it has an SMD pad, else virtual.
    pub fn of_pads(footprint: &Footprint) -> ModelKind {
        if footprint.pads.iter().any(|p| p.kind == PadKind::ThroughHole) {
            ModelKind::ThroughHole
        } else if footprint.pads.iter().any(|p| p.kind == PadKind::Smd) {
            ModelKind::Smd
        } else {
            ModelKind::Virtual
        }
    }
}

/// What the 3D view needs of an installed KiCad footprint (`Lib:Name`).
#[derive(Debug, Clone, PartialEq)]
pub struct InstalledFootprint {
    pub models: Vec<Model3d>,
    /// `(pad number, centre)` of every pad, µm, in the footprint's own frame.
    pub pads: Vec<(String, (f64, f64))>,
    pub kind: ModelKind,
}

fn installed_cache() -> &'static Mutex<HashMap<(std::path::PathBuf, String), Option<Arc<InstalledFootprint>>>> {
    static CACHE: OnceLock<Mutex<HashMap<(std::path::PathBuf, String), Option<Arc<InstalledFootprint>>>>> = OnceLock::new();
    CACHE.get_or_init(Default::default)
}

/// The installed footprint `id` (`Lib:Name`) names, read from `root` once. `None` for an id that names no footprint file there.
pub fn installed_footprint_in(root: &std::path::Path, id: &str) -> Option<Arc<InstalledFootprint>> {
    let key = (root.to_path_buf(), id.to_string());
    if let Some(hit) = installed_cache().lock().unwrap_or_else(PoisonError::into_inner).get(&key) {
        return hit.clone();
    }
    let found = (|| {
        let (lib, item) = id.split_once(':')?;
        // A library nickname and a footprint name are file names: nothing in them may lead out of the library.
        if [lib, item].iter().any(|p| p.is_empty() || p.starts_with('.') || p.contains(['/', '\\']) || p.contains("..")) {
            return None;
        }
        let text = std::fs::read_to_string(find_footprint_file(root, id)?).ok()?;
        let parsed = crate::parse_library_footprint(&text).ok()?;
        let attrs = &parsed.footprint.attributes;
        let kind = if attrs.smd {
            ModelKind::Smd
        } else if attrs.through_hole {
            ModelKind::ThroughHole
        } else {
            ModelKind::Virtual
        };
        let pads = parsed.footprint.pads.iter().map(|p| (p.number.clone(), (p.at.x as f64, p.at.y as f64))).collect();
        Some(Arc::new(InstalledFootprint { models: footprint_models(&text), pads, kind }))
    })();
    installed_cache().lock().unwrap_or_else(PoisonError::into_inner).insert(key, found.clone());
    found
}

/// [`installed_footprint_in`] the installed KiCad's footprint libraries (`EDA_KICAD_FOOTPRINTS`, else the macOS bundle).
pub fn installed_footprint(id: &str) -> Option<Arc<InstalledFootprint>> {
    installed_footprint_in(&default_footprint_library_root(), id)
}

/// A turn and a shift of a library footprint's frame onto the studio footprint's: `p_part = R(rot) p_lib + (dx, dy)`, `R` the matrix `to_board` rotates by,
/// both in the footprint's own frame (+y down), micrometres.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Fit {
    pub rot_deg: f64,
    pub dx_um: f64,
    pub dy_um: f64,
}

/// The largest root-mean-square distance (µm) between the library's pad centres and the part's, after the fit, for the fit to count. A built-in 0603 and
/// KiCad's `R_0603_1608Metric` differ by tenths of a millimetre; a different package that happens to share pad numbers differs by millimetres.
const FIT_TOLERANCE_UM: f64 = 600.0;

/// Where the library footprint's pads would have to move, by a turn and a shift, to sit on the part's: pads are matched by number (the mean centre when
/// several pads share one), the turn is the least-squares one (snapped to a quarter turn when it is within 5 degrees of one) and the shift puts the centroids
/// together. `None` when no pad number is common to both, when the fit is the identity already, or when it leaves the pads further apart than
/// [`FIT_TOLERANCE_UM`] -- then the two are not the same footprint and the model keeps its own placement.
pub fn fit_pads(lib: &[(String, (f64, f64))], part: &[(String, (f64, f64))]) -> Option<Fit> {
    fn centres(pads: &[(String, (f64, f64))]) -> std::collections::BTreeMap<&str, (f64, f64)> {
        let mut sums: std::collections::BTreeMap<&str, (f64, f64, f64)> = Default::default();
        for (number, (x, y)) in pads {
            let e = sums.entry(number.as_str()).or_insert((0.0, 0.0, 0.0));
            *e = (e.0 + x, e.1 + y, e.2 + 1.0);
        }
        sums.into_iter().map(|(k, (x, y, n))| (k, (x / n, y / n))).collect()
    }
    let (lib_c, part_c) = (centres(lib), centres(part));
    let pairs: Vec<((f64, f64), (f64, f64))> = lib_c.iter().filter_map(|(n, l)| part_c.get(n).map(|p| (*l, *p))).collect();
    if pairs.is_empty() {
        return None;
    }
    let count = pairs.len() as f64;
    let (cl, cp) = (
        (pairs.iter().map(|(l, _)| l.0).sum::<f64>() / count, pairs.iter().map(|(l, _)| l.1).sum::<f64>() / count),
        (pairs.iter().map(|(_, p)| p.0).sum::<f64>() / count, pairs.iter().map(|(_, p)| p.1).sum::<f64>() / count),
    );
    // The 2-D Procrustes turn: the angle that carries the centred library points onto the centred part points.
    let (mut cross, mut dot) = (0.0, 0.0);
    for (l, p) in &pairs {
        let (a, b) = ((l.0 - cl.0, l.1 - cl.1), (p.0 - cp.0, p.1 - cp.1));
        cross += a.0 * b.1 - a.1 * b.0;
        dot += a.0 * b.0 + a.1 * b.1;
    }
    let mut rot = if pairs.len() < 2 || (cross == 0.0 && dot == 0.0) { 0.0 } else { cross.atan2(dot).to_degrees() };
    let quarter = (rot / 90.0).round() * 90.0;
    if (rot - quarter).abs() <= 5.0 {
        rot = quarter;
    }
    if rot == -180.0 {
        rot = 180.0;
    }
    let (sin, cos) = rot.to_radians().sin_cos();
    let (dx, dy) = (cp.0 - (cl.0 * cos - cl.1 * sin), cp.1 - (cl.0 * sin + cl.1 * cos));
    let squares: f64 = pairs
        .iter()
        .map(|(l, p)| {
            let (x, y) = (l.0 * cos - l.1 * sin + dx, l.0 * sin + l.1 * cos + dy);
            (x - p.0).powi(2) + (y - p.1).powi(2)
        })
        .sum();
    if (squares / count).sqrt() > FIT_TOLERANCE_UM {
        return None;
    }
    let fit = Fit { rot_deg: rot, dx_um: dx, dy_um: dy };
    // Less than a hundredth of a degree and of a millimetre is the identity.
    (fit.rot_deg.abs() > 0.01 || fit.dx_um.abs() > 10.0 || fit.dy_um.abs() > 10.0).then_some(fit)
}

fn round6(v: f64) -> f64 {
    let r = (v * 1e6).round() / 1e6;
    if r == 0.0 {
        0.0
    } else {
        r
    }
}

impl Fit {
    /// The model's placement with this fit folded in: the library footprint's frame is turned by `rot` and shifted by `(dx, dy)` onto the part's, so the
    /// model is too. In the model's own frame (y up, the footprint's y flipped) the turn about z is `-rot`: `T(dx, -dy, 0) Rz(-rot) T(offset) Rz(-rotate.z) ..`
    /// is `offset' = (dx, -dy, 0) + Rz(-rot) offset` and `rotate.z' = rotate.z + rot`.
    pub fn apply(&self, m: &Model3d) -> Model3d {
        let (sin, cos) = self.rot_deg.to_radians().sin_cos();
        let [ox, oy, oz] = m.offset;
        let offset = [round6(self.dx_um / 1000.0 + ox * cos + oy * sin), round6(-self.dy_um / 1000.0 - ox * sin + oy * cos), oz];
        let mut z = (m.rotate[2] + self.rot_deg).rem_euclid(360.0);
        if z > 180.0 {
            z -= 360.0;
        }
        Model3d { offset, rotate: [m.rotate[0], m.rotate[1], round6(z)], ..m.clone() }
    }
}

/// A part's 3D models.
#[derive(Debug, Clone, PartialEq)]
pub struct PartModels {
    /// In the studio's frame for the footprint on the top side (a bottom-side one turns them with the part: `Model3d::flipped_frame` is the KiCad file's
    /// form of that).
    pub models: Vec<Model3d>,
    pub kind: ModelKind,
}

/// The 3D models of `part`, whose footprint the model resolved to `footprint` (`ConstraintModel::footprint_of`); `None` when it has none at all.
pub fn part_models(model: &ConstraintModel, part: &Part, footprint: &Footprint) -> Option<PartModels> {
    part_models_in(&default_footprint_library_root(), model, part, footprint)
}

/// [`part_models`] reading the installed footprints from `root`.
pub fn part_models_in(root: &std::path::Path, model: &ConstraintModel, part: &Part, footprint: &Footprint) -> Option<PartModels> {
    let explicit = model.footprints.iter().any(|f| f.name == footprint.name);
    let own: Vec<Model3d> = if !footprint.models3d.is_empty() {
        footprint.models3d.clone()
    } else if explicit {
        footprint.model.iter().map(Model3d::identity).collect()
    } else {
        Vec::new()
    };
    let named_installed = [part.footprint.as_deref(), part.package.as_deref()].into_iter().flatten().find_map(|n| n.contains(':').then(|| installed_footprint_in(root, n)).flatten());

    if !own.is_empty() {
        // An explicit footprint's pads are its own (a library footprint is read for them, an imported board brings them): its models sit on them as written.
        let kind = named_installed.as_ref().map(|i| i.kind).unwrap_or_else(|| ModelKind::of_pads(footprint));
        return Some(PartModels { models: own, kind });
    }

    // The part's pads are the studio's built-in ones: the model of the KiCad footprint it stands for, fitted onto them.
    let generic = kicad_footprint_of_part(part);
    let installed = named_installed.or_else(|| generic.as_ref().and_then(|g| installed_footprint_in(root, &g.id())));
    match installed {
        Some(inst) if !inst.models.is_empty() => {
            let part_pads: Vec<(String, (f64, f64))> = footprint.pads.iter().map(|p| (p.number.clone(), (p.at.0 as f64, p.at.1 as f64))).collect();
            let fit = fit_pads(&inst.pads, &part_pads);
            let models = inst.models.iter().map(|m| fit.map_or_else(|| m.clone(), |f| f.apply(m))).collect();
            Some(PartModels { models, kind: inst.kind })
        }
        _ => {
            // The table's own path for the generic name, as it comes (the library file was not found, or names no model).
            let g = generic?;
            Some(PartModels { models: vec![Model3d::identity(g.model_path())], kind: ModelKind::of_pads(footprint) })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pads(list: &[(&str, f64, f64)]) -> Vec<(String, (f64, f64))> {
        list.iter().map(|(n, x, y)| (n.to_string(), (*x, *y))).collect()
    }

    #[test]
    fn identical_pads_are_no_fit() {
        let p = pads(&[("1", -800.0, 0.0), ("2", 800.0, 0.0)]);
        assert_eq!(fit_pads(&p, &p), None);
    }

    #[test]
    fn a_header_that_runs_along_y_is_turned_onto_one_along_x() {
        // KiCad's PinHeader_1x04: pin 1 at the origin, the others down the y axis. The built-in one: centred, along +x.
        let lib = pads(&[("1", 0.0, 0.0), ("2", 0.0, 2540.0), ("3", 0.0, 5080.0), ("4", 0.0, 7620.0)]);
        let part = pads(&[("1", -3810.0, 0.0), ("2", -1270.0, 0.0), ("3", 1270.0, 0.0), ("4", 3810.0, 0.0)]);
        let fit = fit_pads(&lib, &part).expect("the same footprint, turned");
        assert_eq!(fit.rot_deg, -90.0);
        assert!((fit.dx_um + 3810.0).abs() < 1e-6 && fit.dy_um.abs() < 1e-6, "{fit:?}");
        // The model's pin k, 2.54 k below pin 1 on the footprint (y up in the model: -2.54 k), lands on the part's pad x = -3.81 + 2.54 k.
        let m = fit.apply(&Model3d { offset: [0.0, -5.08, 0.0], ..Model3d::identity("x.step") });
        assert!((m.offset[0] - 1.27).abs() < 1e-9 && m.offset[1].abs() < 1e-9, "pin 3 of the model sits on pad 3: {:?}", m.offset);
        assert_eq!(m.rotate, [0.0, 0.0, -90.0]);
    }

    #[test]
    fn a_small_shift_is_kept_and_a_different_footprint_is_not_fitted() {
        let lib = pads(&[("1", -825.0, 0.0), ("2", 825.0, 0.0)]);
        let near = pads(&[("1", -750.0, 0.0), ("2", 750.0, 0.0)]);
        assert_eq!(fit_pads(&lib, &near), None, "a 0603 and its built-in twin: pads 75 um apart about one centre are the identity");
        let moved = pads(&[("1", -250.0, 0.0), ("2", 1250.0, 0.0)]);
        let fit = fit_pads(&lib, &moved).expect("the same pair, shifted half a millimetre");
        assert_eq!(fit.rot_deg, 0.0);
        assert!((fit.dx_um - 500.0).abs() < 1e-6 && fit.dy_um.abs() < 1e-6, "{fit:?}");
        let far = pads(&[("1", -9000.0, 0.0), ("2", 9000.0, 0.0)]);
        assert_eq!(fit_pads(&lib, &far), None, "pads 16 mm further apart are not the same footprint");
        assert_eq!(fit_pads(&lib, &pads(&[("A", 0.0, 0.0)])), None, "no pad number in common");
    }

    #[test]
    fn a_pad_number_shared_by_several_pads_counts_once_at_their_mean() {
        let lib = pads(&[("1", 0.0, 0.0), ("2", 1000.0, 0.0), ("2", 1000.0, 2000.0)]);
        let part = pads(&[("1", 100.0, 0.0), ("2", 1100.0, 1000.0)]);
        let fit = fit_pads(&lib, &part).expect("mean of the two pads numbered 2 is (1000, 1000)");
        assert_eq!(fit.rot_deg, 0.0);
        assert!((fit.dx_um - 100.0).abs() < 1e-6 && (fit.dy_um - 0.0).abs() < 1e-6, "{fit:?}");
    }

    #[test]
    fn flipping_a_models_frame_twice_is_the_model_again() {
        let m = Model3d { path: "a.step".into(), offset: [1.0, 2.0, 3.0], scale: [1.0, 1.0, 1.0], rotate: [10.0, 20.0, 30.0], opacity: 1.0, show: true };
        let flipped = m.flipped_frame();
        assert_eq!(flipped.offset, [-1.0, -2.0, 3.0]);
        assert_eq!(flipped.rotate, [10.0, 20.0, -150.0]);
        assert_eq!(flipped.flipped_frame(), m);
        assert_eq!(Model3d::identity("a.step").flipped_frame().flipped_frame(), Model3d::identity("a.step"), "0 - 180 is 180 and back");
    }

    /// The brief's parts, with KiCad.app installed: a 0603 resistor has its library model on it as it comes; a 1x04 header, built in along x, has KiCad's
    /// header model turned onto its pads.
    #[test]
    fn the_generic_names_get_the_models_of_the_installed_footprints_fitted_to_their_pads() {
        let root = default_footprint_library_root();
        if !root.is_dir() {
            return;
        }
        let part = |r: &str, pkg: &str| Part { reference: r.into(), mpn: None, lcsc: None, value: None, package: Some(pkg.into()), footprint: Some(pkg.into()), pins: vec![], body_um: None, symbol: None, datasheet: None, edge: None };
        let model = ConstraintModel::default();
        let resolve = |p: &Part| {
            let fp = ConstraintModel { parts: vec![p.clone()], ..Default::default() }.footprint_of(p).expect("a built-in footprint");
            part_models_in(&root, &model, p, &fp).expect("models")
        };
        let r = resolve(&part("R1", "0603"));
        assert_eq!(r.kind, ModelKind::Smd);
        assert_eq!(r.models.len(), 1);
        assert!(r.models[0].path.ends_with("Resistor_SMD.3dshapes/R_0603_1608Metric.step"), "{:?}", r.models[0].path);
        assert!(r.models[0].offset.iter().all(|v| v.abs() < 0.2), "a resistor is not moved: {:?}", r.models[0]);
        let h = resolve(&part("J1", "PINHEADER-4"));
        assert_eq!(h.kind, ModelKind::ThroughHole);
        assert!(h.models[0].path.ends_with("PinHeader_1x04_P2.54mm_Vertical.step"));
        assert_eq!(h.models[0].rotate[2], -90.0, "KiCad's header runs along y; the built-in one along x: {:?}", h.models[0]);
        assert!((h.models[0].offset[0] + 3.81).abs() < 1e-6, "pin 1 at the built-in header's first pad: {:?}", h.models[0].offset);
    }

    #[test]
    fn an_explicit_footprint_keeps_its_own_models_as_written() {
        let model = ConstraintModel {
            footprints: vec![Footprint { name: "stones:C_1206_Flat".into(), pads: vec![], courtyard: None, model: Some("${KICAD9_3DMODEL_DIR}/C.step".into()), courtyard_outlines: vec![], models3d: vec![Model3d { rotate: [90.0, 0.0, 0.0], ..Model3d::identity("${KICAD9_3DMODEL_DIR}/C.step") }] }],
            ..Default::default()
        };
        let part = Part { reference: "C1".into(), mpn: None, lcsc: None, value: None, package: None, footprint: Some("stones:C_1206_Flat".into()), pins: vec![], body_um: None, symbol: None, datasheet: None, edge: None };
        let fp = model.footprint_of(&part).unwrap();
        let got = part_models_in(std::path::Path::new("/nonexistent"), &model, &part, &fp).unwrap();
        assert_eq!(got.models.len(), 1);
        assert_eq!(got.models[0].rotate, [90.0, 0.0, 0.0]);
    }
}
