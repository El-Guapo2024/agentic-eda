//! Load a real KiCad library footprint (a `.kicad_mod` file) into our own
//! `eda_model::Footprint`, so a part can name one directly --
//! `footprint: "Connector_USB:USB_C_Receptacle_HRO_TYPE-C-31-M-12"` -- and
//! get every pad feature that footprint actually uses (slotted drills,
//! non-plated holes, a pad rotated on its own, repeated pad numbers,
//! roundrect ratio) instead of falling through to the built-in package
//! table, which only knows a handful of generic passive/small-IC shapes.
//!
//! This shares its pad grammar with the whole-board `.kicad_pcb` reader
//! (`import::parse_pad_geometry` -- a `.kicad_mod`'s `(pad ...)` nodes are
//! byte-for-byte the same grammar as a footprint's inside a board file) so
//! the two readers cannot drift apart on what a pad means.
//!
//! Lives in `eda-kicad`, not `eda-model`, because `eda-model` cannot depend
//! on the s-expression reader (or on `eda-kicad` at all -- the dependency
//! only goes the other way).

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use eda_model::ir::{Side, Um};
use eda_model::{ConstraintModel, Footprint};

use crate::import::{parse_pad_geometry, ImportNotes};
use crate::sexpr::{self, Sexpr};

/// Environment variable overriding where KiCad's own footprint libraries
/// (a directory of `<Library>.pretty/<Footprint>.kicad_mod` trees) live.
pub const LIBRARY_ROOT_ENV: &str = "EDA_KICAD_FOOTPRINTS";

/// Where KiCad's standard footprint libraries live: `LIBRARY_ROOT_ENV` if
/// set, else KiCad's own default install location on macOS.
pub fn default_footprint_library_root() -> PathBuf {
    if let Ok(p) = std::env::var(LIBRARY_ROOT_ENV) {
        if !p.is_empty() {
            return PathBuf::from(p);
        }
    }
    PathBuf::from("/Applications/KiCad/KiCad.app/Contents/SharedSupport/footprints")
}

/// The `.kicad_mod` file a library id (`"Connector_USB:USB_C_Receptacle_
/// HRO_TYPE-C-31-M-12"`) names, under `lib_root`. `None` for a name that is
/// not `"Library:Footprint"` shaped, or a file that does not exist --
/// either way, the caller falls back to the built-in table.
pub fn find_footprint_file(lib_root: &Path, lib_id: &str) -> Option<PathBuf> {
    let (lib, name) = lib_id.split_once(':')?;
    if lib.is_empty() || name.is_empty() {
        return None;
    }
    let path = lib_root.join(format!("{lib}.pretty")).join(format!("{name}.kicad_mod"));
    path.is_file().then_some(path)
}

/// Parse a `.kicad_mod` file's text into our `Footprint`: every pad (all
/// four hand-editable features, plus roundrect ratio), and a courtyard
/// from `F.CrtYd`/`B.CrtYd` graphics.
pub fn parse_footprint_file(text: &str, name: &str) -> Result<Footprint, String> {
    let tree = sexpr::parse(text).map_err(|e| format!("not a valid s-expression file: {e}"))?;
    let root = tree.as_list().filter(|l| sexpr::tag(l) == Some("footprint")).ok_or("top-level form is not (footprint ...); not a KiCad footprint file")?;

    // A bare footprint file has no board placement, so its own `(at ...)`
    // (present when it was saved from a board, absent from a pristine
    // library file) is not our concern: pad rotations are already relative
    // to the footprint, which is exactly the frame this file's pads are
    // already in. Side is always top; the loader's caller mirrors it for a
    // bottom-side instance the same way it already does for the built-in
    // table.
    let mut notes = ImportNotes::default();
    let mut pads = Vec::new();
    for pad in sexpr::find_all(root, "pad") {
        if let Some(p) = parse_pad_geometry(pad, Side::Top, 0, &mut notes) {
            pads.push(p);
        }
    }
    if pads.is_empty() {
        return Err("footprint has no pads this reader could place (missing at/size?)".into());
    }

    Ok(Footprint { name: name.to_string(), pads, courtyard: courtyard_from(root) })
}

/// A symmetric-about-origin courtyard half-extent enclosing every
/// `F.CrtYd`/`B.CrtYd` graphic. Real footprints are not always centred on
/// their own origin (KiCad has no such rule); `Footprint::courtyard` is,
/// so an off-centre courtyard is conservatively enclosed by the smallest
/// symmetric box that contains it, not represented exactly. `None` when
/// the footprint has no courtyard layer at all -- callers fall back to the
/// pad-bounding-box derivation the same as any other footprint with none.
fn courtyard_from(root: &[Sexpr]) -> Option<(Um, Um)> {
    let on_crtyd = |item: &[Sexpr]| matches!(sexpr::find(item, "layer").and_then(|l| sexpr::txt(l, 1)), Some("F.CrtYd") | Some("B.CrtYd"));
    let mm_to_um = crate::import::mm_to_um;
    let (mut x0, mut y0, mut x1, mut y1) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
    let mut extend = |x: f64, y: f64| {
        x0 = x0.min(x);
        y0 = y0.min(y);
        x1 = x1.max(x);
        y1 = y1.max(y);
    };
    let mut seen = false;
    for line in sexpr::find_all(root, "fp_line").filter(|it| on_crtyd(it)) {
        for tag in ["start", "end"] {
            if let Some(p) = sexpr::find(line, tag) {
                if let (Some(x), Some(y)) = (sexpr::num(p, 1), sexpr::num(p, 2)) {
                    extend(x, y);
                    seen = true;
                }
            }
        }
    }
    for rect in sexpr::find_all(root, "fp_rect").filter(|it| on_crtyd(it)) {
        for tag in ["start", "end"] {
            if let Some(p) = sexpr::find(rect, tag) {
                if let (Some(x), Some(y)) = (sexpr::num(p, 1), sexpr::num(p, 2)) {
                    extend(x, y);
                    seen = true;
                }
            }
        }
    }
    for circle in sexpr::find_all(root, "fp_circle").filter(|it| on_crtyd(it)) {
        if let (Some(c), Some(e)) = (sexpr::find(circle, "center"), sexpr::find(circle, "end")) {
            if let (Some(cx), Some(cy), Some(ex), Some(ey)) = (sexpr::num(c, 1), sexpr::num(c, 2), sexpr::num(e, 1), sexpr::num(e, 2)) {
                let r = ((ex - cx).powi(2) + (ey - cy).powi(2)).sqrt();
                extend(cx - r, cy - r);
                extend(cx + r, cy + r);
                seen = true;
            }
        }
    }
    for poly in sexpr::find_all(root, "fp_poly").filter(|it| on_crtyd(it)) {
        if let Some(pts) = sexpr::find(poly, "pts") {
            for item in &pts[1..] {
                if let Some(xy) = item.as_list().filter(|l| sexpr::tag(l) == Some("xy")) {
                    if let (Some(x), Some(y)) = (sexpr::num(xy, 1), sexpr::num(xy, 2)) {
                        extend(x, y);
                        seen = true;
                    }
                }
            }
        }
    }
    if !seen {
        return None;
    }
    let half_w = mm_to_um(x0.abs().max(x1.abs()));
    let half_h = mm_to_um(y0.abs().max(y1.abs()));
    Some((half_w.max(1), half_h.max(1)))
}

/// For every part naming a `"Library:Footprint"` footprint that is not
/// already in `model.footprints`, try to load it from `lib_root` and add
/// it there -- so `model.footprint_of` (explicit list, then built-in
/// table) resolves it without either of them changing. Best-effort: a
/// part naming a library footprint that cannot be found or parsed is left
/// alone, exactly as it would be today, to fail downstream with the same
/// "no resolvable footprint" a typo in a built-in name already produces.
/// Returns one human-readable warning per footprint that could not be
/// loaded, for the caller to print; an empty result means every named
/// library footprint (if any) resolved.
pub fn resolve_library_footprints(model: &mut ConstraintModel, lib_root: &Path) -> Vec<String> {
    let have: BTreeSet<String> = model.footprints.iter().map(|f| f.name.clone()).collect();
    let mut warnings = Vec::new();
    let mut wanted: Vec<String> = model.parts.iter().filter_map(|p| p.footprint.clone()).filter(|f| f.contains(':') && !have.contains(f)).collect();
    wanted.sort();
    wanted.dedup();
    for lib_id in wanted {
        let Some(path) = find_footprint_file(lib_root, &lib_id) else { continue };
        match std::fs::read_to_string(&path) {
            Ok(text) => match parse_footprint_file(&text, &lib_id) {
                Ok(fp) => model.footprints.push(fp),
                Err(e) => warnings.push(format!("{}: {e}", path.display())),
            },
            Err(e) => warnings.push(format!("{}: {e}", path.display())),
        }
    }
    warnings
}

#[cfg(test)]
mod tests {
    use super::*;
    use eda_model::PadKind;

    fn usb_c_fixture() -> String {
        // A trimmed stand-in for the real HRO USB-C receptacle: one signal
        // pad, one non-plated mounting hole, one slotted shield pad, and an
        // asymmetric F.CrtYd -- every feature this loader exists for, small
        // enough to keep inline.
        r#"(footprint "USB_C_TEST" (version 20260206) (generator "pcbnew") (layer "F.Cu")
            (fp_line (start -5 -5) (end -5 4) (stroke (width 0.05) (type solid)) (layer "F.CrtYd") (uuid "c1"))
            (fp_line (start -5 -5) (end 5 -5) (stroke (width 0.05) (type solid)) (layer "F.CrtYd") (uuid "c2"))
            (fp_line (start -5 4) (end 5 4) (stroke (width 0.05) (type solid)) (layer "F.CrtYd") (uuid "c3"))
            (fp_line (start 5 -5) (end 5 4) (stroke (width 0.05) (type solid)) (layer "F.CrtYd") (uuid "c4"))
            (pad "" np_thru_hole circle (at -2.89 -2.6) (size 0.65 0.65) (drill 0.65) (layers "*.Cu" "*.Mask") (uuid "n1"))
            (pad "A1" smd roundrect (at -3.25 -4.045) (size 0.6 1.45) (layers "F.Cu" "F.Mask" "F.Paste") (roundrect_rratio 0.25) (uuid "p1"))
            (pad "SH" thru_hole oval (at -4.32 -3.13) (size 1 2.1) (drill oval 0.6 1.7) (layers "*.Cu" "*.Mask") (uuid "s1"))
            (pad "SH" thru_hole oval (at 4.32 -3.13) (size 1 2.1) (drill oval 0.6 1.7) (layers "*.Cu" "*.Mask") (uuid "s2"))
        )"#
        .to_string()
    }

    #[test]
    fn parses_every_pad_feature() {
        let fp = parse_footprint_file(&usb_c_fixture(), "Connector_USB:USB_C_TEST").expect("parses");
        assert_eq!(fp.pads.len(), 4);

        let npth = fp.pads.iter().find(|p| p.kind == PadKind::NonPlatedHole).expect("the mounting hole");
        assert_eq!(npth.number, "");
        assert_eq!(npth.drill, Some(650));

        let shields: Vec<_> = fp.pads.iter().filter(|p| p.number == "SH").collect();
        assert_eq!(shields.len(), 2, "both shield pads must survive under the shared number");
        assert_eq!(shields[0].drill_slot, Some((600, 1700)));
        assert_eq!(shields[0].drill, None);

        let signal = fp.pads.iter().find(|p| p.number == "A1").unwrap();
        assert_eq!(signal.roundrect_ratio, Some(0.25));

        // Asymmetric F.CrtYd (y: -5..4) conservatively enclosed by a
        // symmetric half-extent of 5mm either way.
        let (hw, hh) = fp.courtyard.expect("courtyard from F.CrtYd");
        assert_eq!(hw, 5_000);
        assert_eq!(hh, 5_000);

        assert!(fp.validate().is_empty(), "{:?}", fp.validate());
    }

    #[test]
    fn find_footprint_file_needs_lib_colon_name() {
        let dir = std::env::temp_dir().join("eda_kicad_footprint_lib_test");
        std::fs::create_dir_all(dir.join("Connector_USB.pretty")).unwrap();
        std::fs::write(dir.join("Connector_USB.pretty").join("Foo.kicad_mod"), usb_c_fixture()).unwrap();
        assert!(find_footprint_file(&dir, "Connector_USB:Foo").is_some());
        assert!(find_footprint_file(&dir, "Connector_USB:Missing").is_none());
        assert!(find_footprint_file(&dir, "not_a_lib_id").is_none());
    }

    #[test]
    fn resolve_library_footprints_adds_only_what_is_missing() {
        let dir = std::env::temp_dir().join("eda_kicad_footprint_lib_resolve_test");
        std::fs::create_dir_all(dir.join("Connector_USB.pretty")).unwrap();
        std::fs::write(dir.join("Connector_USB.pretty").join("Foo.kicad_mod"), usb_c_fixture()).unwrap();

        let part = |r: &str, footprint: &str| eda_model::Part {
            reference: r.into(),
            mpn: None,
            lcsc: None,
            value: None,
            package: None,
            footprint: Some(footprint.into()),
            pins: vec![],
            body_um: None,
            edge: None,
        };
        let mut model = ConstraintModel {
            parts: vec![part("J1", "Connector_USB:Foo"), part("R1", "0603"), part("J2", "Connector_USB:DoesNotExist")],
            ..Default::default()
        };
        let warnings = resolve_library_footprints(&mut model, &dir);
        assert!(warnings.is_empty(), "a missing library file is a silent fallback to the built-in table, not a warning: {warnings:?}");
        assert_eq!(model.footprints.len(), 1);
        assert_eq!(model.footprints[0].name, "Connector_USB:Foo");

        // Calling it again must not duplicate the now-present footprint.
        let warnings2 = resolve_library_footprints(&mut model, &dir);
        assert!(warnings2.is_empty());
        assert_eq!(model.footprints.len(), 1);
    }

    /// The two real footprints this feature exists for, straight from a
    /// real KiCad install -- not `#[ignore]`d (no kicad-cli needed, just
    /// the app's bundled library files), but skipped gracefully when
    /// they're not there, the same way the kicad-cli tests skip.
    #[test]
    fn loads_the_real_usb_c_and_button_footprints() {
        let root = default_footprint_library_root();
        let Some(usb_c) = find_footprint_file(&root, "Connector_USB:USB_C_Receptacle_HRO_TYPE-C-31-M-12") else {
            eprintln!("KiCad footprint libraries not found at {}; skipping", root.display());
            return;
        };
        let text = std::fs::read_to_string(&usb_c).unwrap();
        let fp = parse_footprint_file(&text, "Connector_USB:USB_C_Receptacle_HRO_TYPE-C-31-M-12").expect("parses");
        assert!(fp.validate().is_empty(), "{:?}", fp.validate());
        assert_eq!(fp.pads.iter().filter(|p| p.kind == PadKind::NonPlatedHole).count(), 2, "the two locating pegs");
        let shield: Vec<_> = fp.pads.iter().filter(|p| p.number == "SH").collect();
        assert_eq!(shield.len(), 4, "the shield's four physical pads, one pin");
        assert!(shield.iter().all(|p| p.drill_slot.is_some()), "the shield pads are slotted");
        assert!(fp.courtyard.is_some());

        let button_path = find_footprint_file(&root, "Button_Switch_SMD:SW_SPST_B3U-1000P").expect("KiCad ships this footprint alongside the connector's");
        let text = std::fs::read_to_string(&button_path).unwrap();
        let button = parse_footprint_file(&text, "Button_Switch_SMD:SW_SPST_B3U-1000P").expect("parses");
        assert!(button.validate().is_empty());
        assert_eq!(button.pads.len(), 2);
    }
}
