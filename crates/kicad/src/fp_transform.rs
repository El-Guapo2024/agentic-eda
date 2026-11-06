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
//! translation). The model has no footprint scale, so a footprint whose scale is not 1:1 has the scale baked into its children ([`bake_scale`]: what KiCad 25ec2335's
//! getters give for each child of a scaled footprint, `PAD::derivePadBoardSize`, `PCB_TEXT::GetTextSize`, `PCB_SHAPE::rebakeFromTransform`), so the board is the board
//! KiCad shows, and its reference is returned (the importer says so: `ImportNotes::footprints_scaled`). The board file written back holds the footprint at 1:1.
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
        bake_scale(fp, t.angle.parse::<f64>().unwrap_or(0.0), t.scale);
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

/// A scaled footprint's children as KiCad 25ec2335 shows them (`FOOTPRINT::GetTransform()` applied by each child's getters), in the footprint's own frame -- the rotation
/// and the translation are the footprint's `(at ..)`, which every reader applies:
///
/// * every position and point of a pad, text or graphic is scaled (`TRANSFORM_TRS::Apply` is the scale, then the rotation, then the translation);
/// * a pad's size, drill and offset by `scaleInChildFrame` of the footprint's scale in the pad's frame (`derivePadBoardSize`, `derivePadBoardDrill`,
///   `derivePadBoardOffset`: `sx c² + sy s²`, `sx s² + sy c²` by the pad's angle to the footprint), a round pad and a round drill by the mean of the two
///   (`( sx + sy ) * 0.5`: "Circular pads use a uniform mean of Sx and Sy so they stay circular");
/// * a text's size by `|sx|` across and `|sy|` up (`PCB_TEXT::GetTextSize`), its thickness by the mean (`GetTextThickness`);
/// * a graphic's points (`rebakeFromTransform`), and its line width by the mean (`PCB_SHAPE::GetWidth`: `fpScaleLinear`).
///
/// **Approximated**: under a scale that differs across and up, KiCad makes a circle an ellipse and an arc an elliptical arc (`SHAPE_T::ELLIPSE`), which the model has not:
/// their points are scaled, so a circle keeps the radius its point on the circle gives. A custom pad's primitives are not scaled.
fn bake_scale(fp: &mut [Sexpr], fp_angle: f64, (sx, sy): (f64, f64)) {
    let point = |p: (f64, f64)| (p.0 * sx, p.1 * sy);
    let mean = (sx.abs() + sy.abs()) * 0.5;
    for child in fp.iter_mut() {
        let Sexpr::List(item) = child else { continue };
        match sexpr::tag(item) {
            Some("pad") => {
                let angle = sexpr::find(item, "at").and_then(|a| sexpr::num(a, 3)).unwrap_or(0.0);
                // `scaleInChildFrame( sx, sy, relOrient, .. )`
                let (c, s) = { let r = (angle - fp_angle).to_radians(); (r.cos(), r.sin()) };
                let (lx, ly) = (sx.abs() * c * c + sy.abs() * s * s, sx.abs() * s * s + sy.abs() * c * c);
                let round = item.get(3).and_then(Sexpr::text) == Some("circle");
                // A rounded or chamfered corner is the ratio times the smaller side of the pad stack's own size, the library's (`PADSTACK::RoundRectRadius`:
                // `std::min( size.x, size.y ) * RoundRectRadiusRatio()`), so the corner keeps its size while the pad grows: the ratio of the baked pad is that radius
                // over its own smaller side.
                let lib_size = sexpr::find(item, "size").and_then(|l| Some((sexpr::num(l, 1)?, sexpr::num(l, 2)?)));
                let board_min = lib_size.map(|(w, h)| if round { (w * mean).min(h * mean) } else { (w * lx).min(h * ly) });
                let corner = |ratio: f64| match (lib_size, board_min) {
                    (Some((w, h)), Some(b)) if b > 0.0 => (ratio * w.min(h) / b).clamp(0.0, 0.5),
                    _ => ratio,
                };
                for part in item.iter_mut() {
                    let Sexpr::List(l) = part else { continue };
                    if matches!(sexpr::tag(l), Some("roundrect_rratio") | Some("chamfer_ratio")) {
                        if let Some(r) = sexpr::num(l, 1) {
                            l[1] = atom(&format!("{}", (corner(r) * 1e6).round() / 1e6));
                        }
                    }
                }
                for part in item.iter_mut() {
                    let Sexpr::List(l) = part else { continue };
                    match sexpr::tag(l) {
                        Some("at") => map_xy(l, &point),
                        Some("size") => map_xy(l, &|(w, h)| if round { (w * mean, h * mean) } else { (w * lx, h * ly) }),
                        Some("drill") => {
                            let oval = l.get(1).and_then(Sexpr::text) == Some("oval");
                            let first = if oval { 2 } else { 1 };
                            let factors = if oval { (lx, ly) } else { (mean, mean) };
                            for (k, f) in [(first, factors.0), (first + 1, factors.1)] {
                                if let Some(v) = l.get(k).and_then(Sexpr::text).and_then(|t| t.parse::<f64>().ok()) {
                                    l[k] = atom(&mm(v * f));
                                }
                            }
                            for o in l.iter_mut() {
                                if let Sexpr::List(ol) = o {
                                    if sexpr::tag(ol) == Some("offset") {
                                        map_xy(ol, &|(x, y)| (x * lx, y * ly));
                                    }
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
            Some("fp_text") | Some("property") | Some("fp_text_box") => {
                for part in item.iter_mut() {
                    let Sexpr::List(l) = part else { continue };
                    match sexpr::tag(l) {
                        Some("at") | Some("start") | Some("end") => map_xy(l, &point),
                        Some("pts") => map_pts_in(l, &point),
                        Some("effects") => {
                            for e in l.iter_mut() {
                                let Sexpr::List(font) = e else { continue };
                                if sexpr::tag(font) != Some("font") {
                                    continue;
                                }
                                for f in font.iter_mut() {
                                    let Sexpr::List(fl) = f else { continue };
                                    match sexpr::tag(fl) {
                                        // `(size height width)`
                                        Some("size") => map_xy(fl, &|(h, w)| (h * sy.abs(), w * sx.abs())),
                                        Some("thickness") => {
                                            if let Some(t) = sexpr::num(fl, 1) {
                                                fl[1] = atom(&mm(t * mean));
                                            }
                                        }
                                        _ => {}
                                    }
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
            Some("fp_line") | Some("fp_arc") | Some("fp_circle") | Some("fp_rect") | Some("fp_poly") | Some("fp_curve") => {
                // `PCB_SHAPE::GetWidth`: the line width times `fpScaleLinear`, `( sx + sy ) * 0.5` (signed, as KiCad has it)
                let linear = (sx + sy) * 0.5;
                for part in item.iter_mut() {
                    let Sexpr::List(l) = part else { continue };
                    match sexpr::tag(l) {
                        Some("start") | Some("end") | Some("mid") | Some("center") => map_xy(l, &point),
                        Some("pts") => map_pts_in(l, &point),
                        Some("stroke") => {
                            for w in l.iter_mut() {
                                if let Sexpr::List(wl) = w {
                                    if sexpr::tag(wl) == Some("width") {
                                        if let Some(v) = sexpr::num(wl, 1) {
                                            wl[1] = atom(&mm((v * linear).max(0.0)));
                                        }
                                    }
                                }
                            }
                        }
                        // a file before strokes: `(width ..)`
                        Some("width") => {
                            if let Some(v) = sexpr::num(l, 1) {
                                l[1] = atom(&mm((v * linear).max(0.0)));
                            }
                        }
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }
}

/// The `(xy ..)` points of one `(pts ..)` list.
fn map_pts_in(pts: &mut [Sexpr], f: &dyn Fn((f64, f64)) -> (f64, f64)) {
    for p in pts.iter_mut().skip(1) {
        if let Sexpr::List(pl) = p {
            if sexpr::tag(pl) == Some("xy") {
                map_xy(pl, f);
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

    /// A footprint at a scale of 2 across and 1.5 up (KiCad 25ec2335's getters): its pad at (1, 1) is at (2, 1.5), a rectangular pad of 1 x 0.5 is 2 x 0.75, the same pad
    /// turned a quarter is 1.5 x 1 (the scale in its own frame), a round pad of 1 is 1.75 (the mean), a round drill 0.7, a text of 1 mm is 2 across and 1.5 up with a
    /// thickness of 0.15 x 1.75, a line's points are scaled and so is its width, by the mean.
    #[test]
    fn a_scaled_footprint_has_the_scale_baked_into_its_children() {
        let mut tree = parse(
            r#"(kicad_pcb (version 20260901) (footprint "X" (transform (translate 10 10) (rotate 0) (scale 2 1.5))
                (property "Reference" "U1" (at 0 -2 0) (layer "F.SilkS") (effects (font (size 1 1) (thickness 0.15))))
                (fp_line (start -1 -1) (end 1 -1) (stroke (width 0.12) (type solid)) (layer "F.SilkS"))
                (pad "1" smd roundrect (at 1 1) (size 1 0.5) (layers "F.Cu") (roundrect_rratio 0.25))
                (pad "2" smd rect (at -1 1 90) (size 1 0.5) (layers "F.Cu"))
                (pad "3" thru_hole circle (at 0 0) (size 1 1) (drill 0.4) (layers "*.Cu"))))"#,
        );
        assert_eq!(normalize(&mut tree), vec!["U1".to_string()]);
        let text = to_text(&tree);
        // the corner stays 0.125 (0.25 of the library's 0.5): 0.125 / 0.75
        assert!(text.contains("(at 2 1.5) (size 2 0.75) (layers F.Cu) (roundrect_rratio 0.166667)") || text.contains("(at 2 1.5) (size 2 0.75) (layers \"F.Cu\") (roundrect_rratio 0.166667)"), "{text}");
        assert!(text.contains("(at -2 1.5 90) (size 1.5 1)"), "{text}");
        assert!(text.contains("(size 1.75 1.75) (drill 0.7)"), "{text}");
        assert!(text.contains("(at 0 -3 0)") && text.contains("(size 1.5 2) (thickness 0.2625)"), "{text}");
        assert!(text.contains("(start -2 -1.5) (end 2 -1.5) (stroke (width 0.21)"), "{text}");
    }

    fn to_text(tree: &Sexpr) -> String {
        crate::sexpr::to_text(tree)
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
