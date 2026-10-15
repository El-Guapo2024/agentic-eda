//! KiCad 10's footprint transform, read back into the layout the importer reads.
//!
//! From file version 20260616 on (`FIRST_FP_AFFINE_TRANSFORM` in `pcbnew/pcb_io/kicad_sexpr/pcb_io_kicad_sexpr.h`: "Footprint affine transform: lib-frame
//! storage and (transform) block"), a board file places a footprint with `(transform (translate x y) (rotate a) (scale sx sy))` where older files
//! wrote `(at x y a)`, and stores a footprint's own zones in the footprint's library frame where older files stored them in the board's
//! (`PCB_IO_KICAD_SEXPR_PARSER::parseFOOTPRINT_unchecked`: the `T_transform` case, and "Legacy footprint zone outlines were in board frame. Convert to
//! lib."). The rest of a footprint keeps its meaning across the change: pads and texts are footprint-relative positions with board-frame angles in
//! both (`PCB_IO_KICAD_SEXPR::format( const PAD* )`, `format( const PCB_TEXT* )`), and the graphics are in the footprint's frame in both.
//!
//! [`normalize`] rewrites a newer tree into the older layout, so every reader of footprints in this crate works on either: the transform becomes an
//! `(at ...)` and the zones' points go to the board frame (`TRANSFORM_TRS::Apply`: the scale, then `RotatePoint` by the orientation, then the
//! translation). The model has no footprint scale, so a footprint whose scale is not 1:1 is read at 1:1 and its reference is returned.
//!
//! What says a footprint is in the new layout is its `(transform ...)` itself, not the file's `(version ...)`: the token and the library-frame storage
//! came in the same version, and a footprint pasted from KiCad 10 reaches the importer inside a board header that [`crate::parse_pcb_clipboard`]
//! writes with an older version.

use crate::sexpr::{self, Sexpr};

/// `FIRST_FP_AFFINE_TRANSFORM`: the first board file version that stores a footprint's children in its library frame.
pub const FIRST_FP_AFFINE_TRANSFORM: i64 = 20260616;

/// Rewrite every footprint `(transform ...)` of a `(kicad_pcb ...)` tree as the `(at ...)` older files have, and move the footprints' zones to the
/// board frame when the file stores them in the library frame. Returns the references of the footprints whose scale is not 1:1.
pub fn normalize(tree: &mut Sexpr) -> Vec<String> {
    let Sexpr::List(root) = tree else { return Vec::new() };
    let mut scaled = Vec::new();
    for child in root.iter_mut() {
        if let Sexpr::List(fp) = child {
            if matches!(sexpr::tag(fp), Some("footprint") | Some("module")) {
                normalize_footprint(fp, &mut scaled);
            }
        }
    }
    scaled
}

/// What a `(transform ...)` holds; the numbers as the file wrote them, so an `(at ...)` made from them reads back exactly.
struct Transform {
    x: String,
    y: String,
    angle: String,
    scale: (f64, f64),
}

fn read_transform(t: &[Sexpr]) -> Transform {
    let mut out = Transform { x: "0".into(), y: "0".into(), angle: "0".into(), scale: (1.0, 1.0) };
    // "Guard against corrupt files: a zero, negative, or non-finite scale would make the footprint transform degenerate."
    let guard = |s: Option<f64>| s.filter(|v| v.is_finite() && *v > 0.0).unwrap_or(1.0);
    for item in t.iter().skip(1) {
        let Some(l) = item.as_list() else { continue };
        match sexpr::tag(l) {
            Some("translate") => {
                if let (Some(x), Some(y)) = (sexpr::txt(l, 1), sexpr::txt(l, 2)) {
                    out.x = x.to_string();
                    out.y = y.to_string();
                }
            }
            Some("rotate") => {
                if let Some(a) = sexpr::txt(l, 1) {
                    out.angle = a.to_string();
                }
            }
            Some("scale") => out.scale = (guard(sexpr::num(l, 1)), guard(sexpr::num(l, 2))),
            _ => {}
        }
    }
    out
}

fn normalize_footprint(fp: &mut Vec<Sexpr>, scaled: &mut Vec<String>) {
    let Some(idx) = fp.iter().position(|c| c.as_list().is_some_and(|l| sexpr::tag(l) == Some("transform"))) else { return };
    let t = read_transform(fp[idx].as_list().unwrap_or(&[]));
    if fp.iter().any(|c| c.as_list().is_some_and(|l| sexpr::tag(l) == Some("at"))) {
        fp.remove(idx);
    } else {
        fp[idx] = Sexpr::List(vec![atom("at"), atom(&t.x), atom(&t.y), atom(&t.angle)]);
    }
    if (t.scale.0 - 1.0).abs() > 1e-9 || (t.scale.1 - 1.0).abs() > 1e-9 {
        scaled.push(reference(fp));
    }
    let origin = (t.x.parse::<f64>().unwrap_or(0.0), t.y.parse::<f64>().unwrap_or(0.0));
    let angle = t.angle.parse::<f64>().unwrap_or(0.0);
    let apply = |p: (f64, f64)| apply_transform(p, origin, angle, t.scale);
    for child in fp.iter_mut() {
        if let Sexpr::List(zone) = child {
            if sexpr::tag(zone) == Some("zone") {
                map_pts(zone, &apply);
            }
        }
    }
}

/// `TRANSFORM_TRS::Apply` on a point of a footprint's frame: the scale, then the rotation by the orientation, then the translation. The unit is the caller's (the map is
/// linear): [`normalize`] maps a footprint's zones with it in millimetres, the reader of a refilled board's fills (`zone_fills::scan_zone_fills`) in nanometres.
pub(crate) fn apply_transform((x, y): (f64, f64), origin: (f64, f64), angle_deg: f64, scale: (f64, f64)) -> (f64, f64) {
    let (rx, ry) = rotate_point((x * scale.0, y * scale.1), angle_deg);
    (rx + origin.0, ry + origin.1)
}

/// `RotatePoint( VECTOR2I&, const EDA_ANGLE& )` (libs/kimath/src/trigo.cpp), in millimetres: the exact quarter turns first, then
/// `x' = y sin + x cos`, `y' = y cos - x sin`.
fn rotate_point((x, y): (f64, f64), angle_deg: f64) -> (f64, f64) {
    let a = angle_deg.rem_euclid(360.0);
    if a == 0.0 {
        (x, y)
    } else if a == 90.0 {
        (y, -x)
    } else if a == 180.0 {
        (-x, -y)
    } else if a == 270.0 {
        (-y, x)
    } else {
        let (s, c) = a.to_radians().sin_cos();
        (y * s + x * c, y * c - x * s)
    }
}

/// Every point of every `(pts ...)` under `list` -- the outline's `(xy x y)` and the `(arc (start ..) (mid ..) (end ..))` of a zone's polygons and fills.
fn map_pts(list: &mut [Sexpr], f: &dyn Fn((f64, f64)) -> (f64, f64)) {
    for child in list.iter_mut() {
        let Sexpr::List(l) = child else { continue };
        if sexpr::tag(l) == Some("pts") {
            for p in l.iter_mut().skip(1) {
                let Sexpr::List(pl) = p else { continue };
                match sexpr::tag(pl) {
                    Some("xy") => map_xy(pl, f),
                    Some("arc") => {
                        for q in pl.iter_mut().skip(1) {
                            if let Sexpr::List(ql) = q {
                                if matches!(sexpr::tag(ql), Some("start") | Some("mid") | Some("end")) {
                                    map_xy(ql, f);
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }
        } else {
            map_pts(l, f);
        }
    }
}

fn map_xy(l: &mut [Sexpr], f: &dyn Fn((f64, f64)) -> (f64, f64)) {
    let (Some(x), Some(y)) = (sexpr::num(l, 1), sexpr::num(l, 2)) else { return };
    let (nx, ny) = f((x, y));
    l[1] = atom(&mm(nx));
    l[2] = atom(&mm(ny));
}

/// A length the way KiCad writes one: millimetres to the nanometre, trailing zeros dropped.
fn mm(v: f64) -> String {
    let s = format!("{:.6}", (v * 1e6).round() / 1e6);
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s == "-0" { "0".to_string() } else { s.to_string() }
}

fn atom(s: &str) -> Sexpr {
    Sexpr::Atom(s.to_string())
}

/// The footprint's Reference, for the note: `(property "Reference" "U1" ...)`, or `(fp_text reference "U1" ...)` in older files.
fn reference(fp: &[Sexpr]) -> String {
    sexpr::find_all(fp, "property")
        .find(|p| sexpr::txt(p, 1) == Some("Reference"))
        .and_then(|p| sexpr::txt(p, 2))
        .or_else(|| sexpr::find_all(fp, "fp_text").find(|t| sexpr::txt(t, 1) == Some("reference")).and_then(|t| sexpr::txt(t, 2)))
        .unwrap_or("?")
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(text: &str) -> Sexpr {
        sexpr::parse(text).unwrap()
    }

    #[test]
    fn a_transform_becomes_the_at_older_files_have() {
        let mut tree = parse(r#"(kicad_pcb (version 20260901) (footprint "R:R_0603" (layer "F.Cu") (transform (translate 100.4 68.2) (rotate -90) (scale 1 1)) (pad "1" smd rect (at -0.8 0 270))))"#);
        let scaled = normalize(&mut tree);
        let root = tree.as_list().unwrap();
        let fp = sexpr::find(root, "footprint").unwrap();
        let at = sexpr::find(fp, "at").unwrap();
        assert_eq!((sexpr::txt(at, 1), sexpr::txt(at, 2), sexpr::txt(at, 3)), (Some("100.4"), Some("68.2"), Some("-90")));
        assert!(sexpr::find(fp, "transform").is_none());
        // A pad keeps its own (at ...): footprint-relative, with the board-frame angle, as before.
        let pad = sexpr::find(fp, "pad").unwrap();
        assert_eq!(sexpr::txt(sexpr::find(pad, "at").unwrap(), 3), Some("270"));
        assert!(scaled.is_empty());
    }

    #[test]
    fn a_transform_without_rotate_or_translate_reads_as_zero() {
        let mut tree = parse(r#"(kicad_pcb (version 20260901) (footprint "X" (transform (scale 1 1))))"#);
        normalize(&mut tree);
        let at = sexpr::find(sexpr::find(tree.as_list().unwrap(), "footprint").unwrap(), "at").unwrap().to_vec();
        assert_eq!((sexpr::num(&at, 1), sexpr::num(&at, 2), sexpr::num(&at, 3)), (Some(0.0), Some(0.0), Some(0.0)));
    }

    #[test]
    fn a_footprint_zone_goes_from_the_library_frame_to_the_board_frame() {
        // The footprint at (10, 20) turned 90 degrees: the library point (1, 0) is at (10 + 0, 20 - 1) on the board (`RotatePoint` by 90: (y, -x)).
        let new = r#"(kicad_pcb (version 20260901) (footprint "X" (transform (translate 10 20) (rotate 90) (scale 1 1))
            (zone (layer "F.Cu") (polygon (pts (xy 1 0) (xy 2 0.5) (arc (start 0 1) (mid 0.5 1.5) (end 1 1)))) (filled_polygon (layer "F.Cu") (pts (xy 1 0))))))"#;
        let mut tree = parse(new);
        normalize(&mut tree);
        let fp = sexpr::find(tree.as_list().unwrap(), "footprint").unwrap().to_vec();
        let zone = sexpr::find(&fp, "zone").unwrap().to_vec();
        let pts = sexpr::find(sexpr::find(&zone, "polygon").unwrap(), "pts").unwrap().to_vec();
        let xy: Vec<(f64, f64)> = sexpr::find_all(&pts, "xy").map(|p| (sexpr::num(p, 1).unwrap(), sexpr::num(p, 2).unwrap())).collect();
        assert_eq!(xy, vec![(10.0, 19.0), (10.5, 18.0)]);
        let arc = sexpr::find(&pts, "arc").unwrap().to_vec();
        let start = sexpr::find(&arc, "start").unwrap();
        assert_eq!((sexpr::num(start, 1), sexpr::num(start, 2)), (Some(11.0), Some(20.0)));
        let fill = sexpr::find(sexpr::find(&zone, "filled_polygon").unwrap(), "pts").unwrap().to_vec();
        let f = sexpr::find(&fill, "xy").unwrap();
        assert_eq!((sexpr::num(f, 1), sexpr::num(f, 2)), (Some(10.0), Some(19.0)));
    }

    fn first_zone_point(tree: &Sexpr) -> (Option<f64>, Option<f64>) {
        let fp = sexpr::find(tree.as_list().unwrap(), "footprint").unwrap().to_vec();
        let pts = sexpr::find(sexpr::find(sexpr::find(&fp, "zone").unwrap(), "polygon").unwrap(), "pts").unwrap().to_vec();
        let p = sexpr::find(&pts, "xy").unwrap();
        (sexpr::num(p, 1), sexpr::num(p, 2))
    }

    #[test]
    fn a_footprint_placed_with_at_keeps_its_board_frame_zones() {
        let mut tree = parse(r#"(kicad_pcb (version 20240108) (footprint "X" (at 10 20 90) (zone (polygon (pts (xy 1 0))))))"#);
        normalize(&mut tree);
        assert_eq!(first_zone_point(&tree), (Some(1.0), Some(0.0)));
    }

    #[test]
    fn a_footprint_pasted_from_kicad_10_has_its_zones_moved_whatever_the_header_says() {
        // `parse_pcb_clipboard` puts a bare footprint on a board header of its own, with an older version than the footprint was written in.
        let mut tree = parse(r#"(kicad_pcb (version 20241229) (generator "pcbnew") (footprint "X" (transform (translate 10 20) (rotate 90) (scale 1 1)) (zone (polygon (pts (xy 1 0))))))"#);
        normalize(&mut tree);
        assert_eq!(first_zone_point(&tree), (Some(10.0), Some(19.0)));
    }

    #[test]
    fn a_scaled_footprint_is_named_and_a_bad_scale_reads_as_one() {
        let mut tree = parse(
            r#"(kicad_pcb (version 20260901) (footprint "X" (transform (translate 0 0) (rotate 0) (scale 2 2)) (property "Reference" "U7"))
                (footprint "Y" (transform (scale 0 -1)) (property "Reference" "U8")))"#,
        );
        assert_eq!(normalize(&mut tree), vec!["U7".to_string()]);
    }

    #[test]
    fn rotate_point_matches_kicads_quarter_turns_and_general_angles() {
        assert_eq!(rotate_point((1.0, 0.0), 90.0), (0.0, -1.0));
        assert_eq!(rotate_point((1.0, 0.0), -90.0), (0.0, 1.0));
        assert_eq!(rotate_point((1.0, 2.0), 180.0), (-1.0, -2.0));
        let (x, y) = rotate_point((1.0, 0.0), 45.0);
        assert!((x - 0.5f64.sqrt()).abs() < 1e-12 && (y + 0.5f64.sqrt()).abs() < 1e-12);
    }

    #[test]
    fn lengths_are_written_to_the_nanometre() {
        assert_eq!(mm(10.0), "10");
        assert_eq!(mm(-0.0000001), "0");
        assert_eq!(mm(1.23456789), "1.234568");
    }
}
