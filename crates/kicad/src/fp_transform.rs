//! KiCad 10's footprint transform, read back into the layout the importer reads.
//!
//! From file version 20260616 on (`FIRST_FP_AFFINE_TRANSFORM` in `pcbnew/pcb_io/kicad_sexpr/pcb_io_kicad_sexpr.h`: "Footprint affine transform: lib-frame
//! storage and (transform) block"), a board file places a footprint with `(transform (translate x y) (rotate a) (scale sx sy))` where older files
//! wrote `(at x y a)`, and stores a footprint's own zones and points in the footprint's library frame where older files stored them in the board's
//! (`PCB_IO_KICAD_SEXPR_PARSER::parseFOOTPRINT_unchecked`: the `T_transform` case, "Legacy footprint zone outlines were in board frame. Convert to
//! lib.", and the `T_point` case's "Legacy files stored footprint points in board frame"). The rest of a footprint keeps its meaning across the change: pads and texts are footprint-relative positions with board-frame angles in
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
            // KiCad 25ec2335's points too ("Legacy files stored footprint points in board frame": `format( PCB_POINT )` writes `GetLibraryPosition()`; UI-LEFT P17)
            if sexpr::tag(zone) == Some("point") {
                for at in zone.iter_mut() {
                    if let Sexpr::List(l) = at {
                        if sexpr::tag(l) == Some("at") {
                            map_xy(l, &apply);
                        }
                    }
                }
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
/// Under a scale that differs across and up KiCad makes a circle an ellipse (`SHAPE_T::ELLIPSE`) and an arc an elliptical arc (`SHAPE_T::ELLIPSE_ARC`), which the
/// model has not: the circle becomes the closed polyline KiCad draws and plots that ellipse as ([`ellipse_polyline`]), the arc the open chain of lines it draws and plots
/// the elliptical arc as ([`elliptical_arc_chain`], UI-LEFT-3 P3-28). A custom pad's primitives are not scaled.
fn bake_scale(fp: &mut Vec<Sexpr>, fp_angle: f64, (sx, sy): (f64, f64)) {
    let point = |p: (f64, f64)| (p.0 * sx, p.1 * sy);
    let mean = (sx.abs() + sy.abs()) * 0.5;
    // the arcs that became chains of lines: (index, the lines), put in place after the walk
    let mut splices: Vec<(usize, Vec<Sexpr>)> = Vec::new();
    for (index, child) in fp.iter_mut().enumerate() {
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
            // A circle under a scale that differs across and up is an ellipse (KiCad 25ec2335 `PCB_SHAPE::rebakeFromTransform`: `SHAPE_T::ELLIPSE`), which the model has not:
            // it becomes the polyline KiCad draws and plots the ellipse as (`SHAPE_ELLIPSE::ConvertToPolyline( GetMaxError() )`, closed; `PlotPoly` /
            // `PlotPolyAsRegion` in `PLOT_BRDITEMS_PLOTTER::PlotShape`), a polygon with the circle's stroke, fill, layer and uuid.
            Some("fp_circle") if sx.abs() != sy.abs() => {
                let c = sexpr::find(item, "center").and_then(|l| Some((sexpr::num(l, 1)?, sexpr::num(l, 2)?)));
                let e = sexpr::find(item, "end").and_then(|l| Some((sexpr::num(l, 1)?, sexpr::num(l, 2)?)));
                if let (Some(c), Some(e)) = (c, e) {
                    let r = ((e.0 - c.0).powi(2) + (e.1 - c.1).powi(2)).sqrt();
                    let pts = ellipse_polyline(point(c), r, (sx.abs(), sy.abs()));
                    let mut poly = vec![atom("fp_poly"), Sexpr::List(std::iter::once(atom("pts")).chain(pts.iter().map(|(x, y)| Sexpr::List(vec![atom("xy"), atom(&mm(*x)), atom(&mm(*y))]))).collect())];
                    poly.extend(item.iter().skip(1).filter(|p| !matches!(p.as_list().and_then(sexpr::tag), Some("center") | Some("end"))).cloned());
                    *item = poly;
                    // the stroke as any graphic's: the width times the mean
                    let linear = (sx + sy) * 0.5;
                    for part in item.iter_mut() {
                        let Sexpr::List(l) = part else { continue };
                        if sexpr::tag(l) == Some("stroke") {
                            for w in l.iter_mut() {
                                if let Sexpr::List(wl) = w {
                                    if sexpr::tag(wl) == Some("width") {
                                        if let Some(v) = sexpr::num(wl, 1) {
                                            wl[1] = atom(&mm((v * linear).max(0.0)));
                                        }
                                    }
                                }
                            }
                        } else if sexpr::tag(l) == Some("width") {
                            if let Some(v) = sexpr::num(l, 1) {
                                l[1] = atom(&mm((v * linear).max(0.0)));
                            }
                        }
                    }
                }
            }
            // An arc under a scale that differs across and up is an elliptical arc (`rebakeFromTransform`: `SHAPE_T::ELLIPSE_ARC`), which the model has not: it becomes
            // the open chain KiCad draws and plots it as (`SHAPE_ELLIPSE::ConvertToPolyline( GetMaxError() )`, `PlotPoly( chain, FILL_T::NO_FILL, .. )`), a line per chord with
            // the arc's stroke (its width times the mean), layer and the rest; the first line keeps the arc's uuid, the others take one made from it
            Some("fp_arc") if sx.abs() != sy.abs() => {
                let xy = |tag: &str| sexpr::find(item, tag).and_then(|l| Some((sexpr::num(l, 1)?, sexpr::num(l, 2)?)));
                let (Some(start), Some(mid), Some(end)) = (xy("start"), xy("mid"), xy("end")) else { continue };
                let Some(chain) = elliptical_arc_chain(start, mid, end, (sx, sy)) else { continue };
                let linear = (sx + sy) * 0.5;
                let rest: Vec<Sexpr> = item.iter().skip(1).filter(|p| !matches!(p.as_list().and_then(sexpr::tag), Some("start") | Some("mid") | Some("end"))).cloned().collect();
                let mut lines: Vec<Sexpr> = Vec::new();
                for (k, w) in chain.windows(2).enumerate() {
                    let mut line = vec![atom("fp_line"), Sexpr::List(vec![atom("start"), atom(&mm(w[0].0)), atom(&mm(w[0].1))]), Sexpr::List(vec![atom("end"), atom(&mm(w[1].0)), atom(&mm(w[1].1))])];
                    for part in &rest {
                        let mut part = part.clone();
                        if let Sexpr::List(l) = &mut part {
                            match sexpr::tag(l) {
                                Some("stroke") => {
                                    for wl in l.iter_mut() {
                                        if let Sexpr::List(wl) = wl {
                                            if sexpr::tag(wl) == Some("width") {
                                                if let Some(v) = sexpr::num(wl, 1) {
                                                    wl[1] = atom(&mm((v * linear).max(0.0)));
                                                }
                                            }
                                        }
                                    }
                                }
                                Some("width") => {
                                    if let Some(v) = sexpr::num(l, 1) {
                                        l[1] = atom(&mm((v * linear).max(0.0)));
                                    }
                                }
                                Some("uuid") | Some("tstamp") if k > 0 => {
                                    if let Some(u) = sexpr::txt(l, 1).map(str::to_string) {
                                        l[1] = sexpr::quoted_atom(&chord_uuid(&u, k));
                                    }
                                }
                                _ => {}
                            }
                        }
                        line.push(part);
                    }
                    lines.push(Sexpr::List(line));
                }
                splices.push((index, lines));
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
    for (index, lines) in splices.into_iter().rev() {
        fp.splice(index..=index, lines);
    }
}

/// The uuid of the `k`-th line an elliptical arc becomes (the first keeps the arc's): made from the arc's, so a file read twice gives the same.
fn chord_uuid(arc_uuid: &str, k: usize) -> String {
    let h = blake3::hash(format!("{arc_uuid}/ellipse-arc/{k}").as_bytes()).to_hex().to_string();
    format!("{}-{}-4{}-8{}-{}", &h[0..8], &h[8..12], &h[13..16], &h[17..20], &h[20..32])
}

/// The open chain an arc of a footprint becomes under the scale `(sx, sy)` when the two differ: KiCad 25ec2335 `PCB_SHAPE::rebakeFromTransform`'s
/// `SHAPE_T::ELLIPSE_ARC` -- the centre of the library arc (`CalcArcCenter`) and its radius scaled to the two semi-axes (`KiROUND( libRadius * GetScaleX() )`,
/// `.. GetScaleY()`, the larger the major one, a quarter turn and the angles less 90 degrees when it is the one up), the parameters the library points' angles
/// about the centre (`EDA_ANGLE( VECTOR2D( m_libStart - libCenter ) )`, start and end swapped when the middle is not between them counter-clockwise, the end
/// past the start) -- cut as `SHAPE_ELLIPSE::ConvertToPolyline( aMaxError )` cuts it (`sweepRange`, `subdivideEllipseArc`). The points are in millimetres, in the
/// footprint's frame (the footprint's turn is its `(at ..)`'s, as `-xform.GetRotate()` is the ellipse's rotation on the board). `None` for a degenerate arc.
fn elliptical_arc_chain(start: (f64, f64), mid: (f64, f64), end: (f64, f64), (sx, sy): (f64, f64)) -> Option<Vec<(f64, f64)>> {
    let nm = |p: (f64, f64)| ((p.0 * 1e6).round() as i64, (p.1 * 1e6).round() as i64);
    let (ls, lm, le) = (nm(start), nm(mid), nm(end));
    // `CalcArcCenter( VECTOR2I, VECTOR2I, VECTOR2I )`: the deltas exact, the numerators in 128 bits, one rounding
    let (bx, by) = ((lm.0 - ls.0) as i128, (lm.1 - ls.1) as i128);
    let (cx, cy) = ((le.0 - ls.0) as i128, (le.1 - ls.1) as i128);
    let d = (bx * cy - by * cx) * 2;
    if d == 0 {
        return None;
    }
    let (b2, c2) = (bx * bx + by * by, cx * cx + cy * cy);
    let centre_x = ((ls.0 as i128 * d + b2 * cy - c2 * by) as f64 / d as f64).round();
    let centre_y = ((ls.1 as i128 * d + c2 * bx - b2 * cx) as f64 / d as f64).round();
    let radius = ((ls.0 as f64 - centre_x).hypot(ls.1 as f64 - centre_y)).round();
    // `xform.Apply( libCenter )`: the scale (the turn and the move are the footprint's)
    let (ox, oy) = ((centre_x * sx).round(), (centre_y * sy).round());
    let mut major = (radius * sx).round().abs();
    let mut minor = (radius * sy).round().abs();
    let angle = |p: (i64, i64)| (p.1 as f64 - centre_y).atan2(p.0 as f64 - centre_x).to_degrees();
    let (mut a_start, a_mid, mut a_end) = (angle(ls), angle(lm), angle(le));
    let wrap = |mut a: f64, base: f64| {
        while a < base {
            a += 360.0;
        }
        a
    };
    if wrap(a_mid, a_start) > wrap(a_end, a_start) {
        std::mem::swap(&mut a_start, &mut a_end);
    }
    while a_end < a_start {
        a_end += 360.0;
    }
    let mut rotation = 0.0_f64;
    if minor > major {
        std::mem::swap(&mut major, &mut minor);
        rotation += 90.0;
        a_start -= 90.0;
        a_end -= 90.0;
    }
    let (sin_rot, cos_rot) = if rotation == 90.0 { (1.0, 0.0) } else { (0.0, 1.0) };
    let eval = |t: f64| -> (i64, i64) {
        let (ct, st) = (t.cos(), t.sin());
        let (lx, ly) = (major * ct, minor * st);
        ((ox + lx * cos_rot - ly * sin_rot).round() as i64, (oy + lx * sin_rot + ly * cos_rot).round() as i64)
    };
    // `sweepRange`
    let two_pi = 2.0 * std::f64::consts::PI;
    let t_start = a_start.to_radians();
    let mut t_end = a_end.to_radians();
    let sweep = t_end - t_start;
    if sweep >= two_pi || sweep <= -two_pi {
        t_end = t_start + two_pi;
    } else if t_end < t_start {
        t_end += two_pi;
    }
    let (p_start, p_end) = (eval(t_start), eval(t_end));
    let mut out = vec![p_start];
    subdivide_ellipse_arc(t_start, p_start, t_end, p_end, MAX_ERROR_NM * MAX_ERROR_NM, 20, &eval, &mut out);
    // `SHAPE_LINE_CHAIN::Append` passes over a point equal to the last one
    out.dedup();
    (out.len() >= 2).then(|| out.into_iter().map(|(x, y)| (x as f64 / 1e6, y as f64 / 1e6)).collect())
}

/// `PCB_SHAPE::GetMaxError()`: the board's `m_MaxError`, `ARC_HIGH_DEF` (0.005 mm) by default.
const MAX_ERROR_NM: f64 = 5_000.0;

/// `subdivideEllipseArc` (KiCad 25ec2335 `libs/kimath/src/geometry/shape_ellipse.cpp`): the chord from `t0` to `t1` is split at the middle parameter until its
/// middle is within the max error of the curve, at most `depth` levels; the points after `p0` are appended.
#[allow(clippy::too_many_arguments)]
fn subdivide_ellipse_arc(t0: f64, p0: (i64, i64), t1: f64, p1: (i64, i64), max_err_sq: f64, depth: i32, eval: &dyn Fn(f64) -> (i64, i64), out: &mut Vec<(i64, i64)>) {
    if depth <= 0 {
        out.push(p1);
        return;
    }
    let tm = 0.5 * (t0 + t1);
    let pm = eval(tm);
    let (mx, my) = (0.5 * (p0.0 as f64 + p1.0 as f64), 0.5 * (p0.1 as f64 + p1.1 as f64));
    let (ex, ey) = (pm.0 as f64 - mx, pm.1 as f64 - my);
    if ex * ex + ey * ey <= max_err_sq {
        out.push(p1);
        return;
    }
    subdivide_ellipse_arc(t0, p0, tm, pm, max_err_sq, depth - 1, eval, out);
    subdivide_ellipse_arc(tm, pm, t1, p1, max_err_sq, depth - 1, eval, out);
}

/// The ellipse a circle of radius `r` (mm, in the footprint's library frame) becomes under the scale `(|sx|, |sy|)`, centred on `centre` (mm, scaled), as KiCad 25ec2335's
/// `SHAPE_ELLIPSE::ConvertToPolyline( aMaxError )` cuts it: the major axis is the longer of the two (rotation 0 across, 90 degrees up), the points `eval( t )` rounded to the
/// nanometre, from `t = 0` round to `2 pi` by `subdivideEllipseArc` (the chord's middle within `aMaxError` of the curve, at most 20 levels deep), the closing repeat dropped.
/// Millimetres back.
fn ellipse_polyline(centre: (f64, f64), r: f64, (ax, ay): (f64, f64)) -> Vec<(f64, f64)> {
    let (a, b) = (r * ax.max(ay) * 1e6, r * ax.min(ay) * 1e6);
    let (cx, cy) = (centre.0 * 1e6, centre.1 * 1e6);
    // `m_sinRot`, `m_cosRot` of 0 or 90 degrees
    let (sin_rot, cos_rot) = if ay > ax { (1.0, 0.0) } else { (0.0, 1.0) };
    let eval = |t: f64| -> (i64, i64) {
        let (ct, st) = (t.cos(), t.sin());
        let (lx, ly) = (a * ct, b * st);
        ((cx + lx * cos_rot - ly * sin_rot).round() as i64, (cy + lx * sin_rot + ly * cos_rot).round() as i64)
    };
    let two_pi = 2.0 * std::f64::consts::PI;
    let (p_start, p_end) = (eval(0.0), eval(two_pi));
    let mut out = vec![p_start];
    subdivide_ellipse_arc(0.0, p_start, two_pi, p_end, MAX_ERROR_NM * MAX_ERROR_NM, 20, &eval, &mut out);
    // `SHAPE_LINE_CHAIN::Append` passes over a point equal to the last one
    out.dedup();
    if out.len() > 1 && out.first() == out.last() {
        out.pop();
    }
    out.into_iter().map(|(x, y)| (x as f64 / 1e6, y as f64 / 1e6)).collect()
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

    /// KiCad 25ec2335 writes a footprint's point in its library frame (`GetLibraryPosition`); the older layout has it on the board -- the board position of `parsePCB_POINT`'s
    /// legacy branch (`point->SetPosition( filePos )`). The numbers are kicad-cli 10.99's rewrite of a point at (3, 2) of a footprint turned 30 degrees and flipped.
    #[test]
    fn a_footprints_point_goes_to_the_board_frame() {
        let mut tree = parse(r#"(kicad_pcb (version 20260901) (footprint "X" (layer "B.Cu") (transform (translate 55 37) (rotate -30) (scale 1 1)) (point (at -3 2) (size 1) (layer "B.Fab"))))"#);
        normalize(&mut tree);
        let fp = sexpr::find(tree.as_list().unwrap(), "footprint").unwrap();
        let at = sexpr::find(sexpr::find(fp, "point").unwrap(), "at").unwrap();
        assert_eq!((sexpr::txt(at, 1), sexpr::txt(at, 2)), (Some("51.401924"), Some("37.232051")));
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

    /// KiCad 25ec2335: a circle of a footprint scaled 2 across and 1 up is an ellipse of semi-axes 2 r and r, drawn and plotted as `ConvertToPolyline( maxError )`'s closed
    /// chain: every point on the ellipse, the chords within 5 um of it, the stroke and the layer the circle's.
    #[test]
    fn a_circle_under_a_scale_that_differs_across_and_up_is_the_ellipses_polyline() {
        let mut tree = parse(r#"(kicad_pcb (version 20260901) (footprint "X" (transform (translate 0 0) (rotate 0) (scale 2 1)) (property "Reference" "U9")
            (fp_circle (center 1 0) (end 2 0) (stroke (width 0.1) (type solid)) (fill no) (layer "F.Fab") (uuid "c1"))))"#);
        normalize(&mut tree);
        let fp = sexpr::find(tree.as_list().unwrap(), "footprint").unwrap().to_vec();
        assert!(sexpr::find(&fp, "fp_circle").is_none());
        let poly = sexpr::find(&fp, "fp_poly").expect("the ellipse's polyline").to_vec();
        assert_eq!(sexpr::txt(sexpr::find(&poly, "layer").unwrap(), 1), Some("F.Fab"));
        let width = sexpr::find(sexpr::find(&poly, "stroke").unwrap(), "width").unwrap().to_vec();
        assert_eq!(sexpr::num(&width, 1), Some(0.15), "the width times the mean scale");
        let pts: Vec<(f64, f64)> = sexpr::find_all(sexpr::find(&poly, "pts").unwrap(), "xy").map(|p| (sexpr::num(p, 1).unwrap(), sexpr::num(p, 2).unwrap())).collect();
        assert!(pts.len() > 32, "{}", pts.len());
        assert_eq!(pts[0], (4.0, 0.0), "t = 0: the end of the major axis");
        for &(x, y) in &pts {
            let on = ((x - 2.0) / 2.0).powi(2) + (y / 1.0).powi(2);
            assert!((on - 1.0).abs() < 1e-5, "({x}, {y}) is on the ellipse: {on}");
        }
        for w in pts.windows(2) {
            let m = ((w[0].0 + w[1].0) / 2.0, (w[0].1 + w[1].1) / 2.0);
            // the chord's middle is within the max error of the curve (radially, near enough for a check)
            let k = (((m.0 - 2.0) / 2.0).powi(2) + m.1.powi(2)).sqrt();
            assert!(1.0 - k < 0.0051, "{w:?}");
        }
    }

    /// KiCad 25ec2335 (UI-LEFT-3 P3-28): an arc of a footprint scaled 2 across and 1 up is an elliptical arc (`SHAPE_T::ELLIPSE_ARC`), drawn and plotted as
    /// `ConvertToPolyline( maxError )`'s open chain: lines from one end of the arc to the other along the ellipse, the chords within 5 um of it, the stroke, the
    /// layer and the first uuid the arc's.
    #[test]
    fn an_arc_under_a_scale_that_differs_across_and_up_is_the_elliptical_arcs_chain() {
        let mut tree = parse(r#"(kicad_pcb (version 20260901) (footprint "X" (transform (translate 0 0) (rotate 0) (scale 2 1)) (property "Reference" "U9")
            (fp_arc (start 2 0) (mid 1 -1) (end 0 0) (stroke (width 0.1) (type solid)) (layer "F.Fab") (uuid "a1"))
            (fp_line (start 0 1) (end 1 1) (stroke (width 0.1) (type solid)) (layer "F.Fab") (uuid "l1"))))"#);
        normalize(&mut tree);
        let fp = sexpr::find(tree.as_list().unwrap(), "footprint").unwrap().to_vec();
        assert!(sexpr::find(&fp, "fp_arc").is_none(), "no circular arc is left");
        let lines: Vec<&[Sexpr]> = sexpr::find_all(&fp, "fp_line").collect();
        assert!(lines.len() > 16, "{}", lines.len());
        let chain: Vec<&[Sexpr]> = lines[..lines.len() - 1].to_vec();
        assert_eq!(sexpr::txt(sexpr::find(lines[lines.len() - 1], "uuid").unwrap(), 1), Some("l1"), "the footprint's other line stays where it was, after the chain");
        let xy = |l: &[Sexpr], tag: &str| { let p = sexpr::find(l, tag).unwrap(); (sexpr::num(p, 1).unwrap(), sexpr::num(p, 2).unwrap()) };
        assert_eq!(xy(chain[0], "start"), (0.0, 0.0), "from the end of the major axis at t = 180 degrees");
        assert_eq!(xy(chain[chain.len() - 1], "end"), (4.0, 0.0));
        let mut uuids = std::collections::BTreeSet::new();
        for (k, w) in chain.iter().enumerate() {
            assert_eq!(sexpr::txt(sexpr::find(w, "layer").unwrap(), 1), Some("F.Fab"));
            let width = sexpr::find(sexpr::find(w, "stroke").unwrap(), "width").unwrap().to_vec();
            assert_eq!(sexpr::num(&width, 1), Some(0.15), "the width times the mean scale");
            let u = sexpr::txt(sexpr::find(w, "uuid").unwrap(), 1).unwrap().to_string();
            if k == 0 {
                assert_eq!(u, "a1");
            }
            assert!(uuids.insert(u), "each line its own uuid");
            if k > 0 {
                assert_eq!(xy(chain[k - 1], "end"), xy(w, "start"), "a chain");
            }
            for (x, y) in [xy(w, "start"), xy(w, "end")] {
                let on = ((x - 2.0) / 2.0).powi(2) + (y / 1.0).powi(2);
                assert!((on - 1.0).abs() < 1e-5 && y <= 1e-9, "({x}, {y}) is on the upper half of the ellipse: {on}");
            }
            let (a, b) = (xy(w, "start"), xy(w, "end"));
            let m = ((a.0 + b.0) / 2.0, (a.1 + b.1) / 2.0);
            let k = (((m.0 - 2.0) / 2.0).powi(2) + m.1.powi(2)).sqrt();
            assert!(1.0 - k < 0.0051, "{a:?} {b:?}");
        }
        // the same file read again gives the same uuids
        let mut again = parse(r#"(kicad_pcb (version 20260901) (footprint "X" (transform (translate 0 0) (rotate 0) (scale 2 1)) (property "Reference" "U9")
            (fp_arc (start 2 0) (mid 1 -1) (end 0 0) (stroke (width 0.1) (type solid)) (layer "F.Fab") (uuid "a1"))))"#);
        normalize(&mut again);
        let fp2 = sexpr::find(again.as_list().unwrap(), "footprint").unwrap().to_vec();
        let second = sexpr::find_all(&fp2, "fp_line").nth(1).map(|l| sexpr::txt(sexpr::find(l, "uuid").unwrap(), 1).unwrap().to_string());
        assert_eq!(second, sexpr::txt(sexpr::find(chain[1], "uuid").unwrap(), 1).map(str::to_string));
    }

    /// Under a scale of one ratio an arc stays an arc (`rebakeFromTransform`'s `SetArcGeometry( xform.Apply( .. ) )`).
    #[test]
    fn an_arc_under_a_uniform_scale_stays_an_arc() {
        let mut tree = parse(r#"(kicad_pcb (version 20260901) (footprint "X" (transform (translate 0 0) (rotate 0) (scale 2 2)) (property "Reference" "U9")
            (fp_arc (start 2 0) (mid 1 -1) (end 0 0) (stroke (width 0.1) (type solid)) (layer "F.Fab") (uuid "a1"))))"#);
        normalize(&mut tree);
        let fp = sexpr::find(tree.as_list().unwrap(), "footprint").unwrap().to_vec();
        let arc = sexpr::find(&fp, "fp_arc").expect("still an arc").to_vec();
        assert_eq!(sexpr::num(sexpr::find(&arc, "mid").unwrap(), 2), Some(-2.0));
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
