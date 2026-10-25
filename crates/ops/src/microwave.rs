//! The microwave tools (GAPS.md item 9, package P19): Draw Microwave Gaps, Stubs, Arc Stubs, Polygonal Shapes and Lines
//! (`pcbnew.MicrowaveTool.createGap`, `createStub`, `createStubArc`, `createFunctionShape`, `createLine`).
//!
//! Each makes a footprint of its own -- no schematic part, no library -- and puts it on the board: a [`BoardPart`] with its definition, a pose, the
//! edit that gives its fields KiCad's layout and the facts a footprint read from a board file would have (pads on the copper layer only, no courtyard).
//! The verbs are one variant of [`crate::Cmd`], `{"op": "microwave", "verb": "gap", ...}`; what the dialogs ask (the size, the angle, the length, the name,
//! the shape file and its three options) travels in them, so a replay needs no dialog and the whole tool is one undo step.
//!
//! Ported from `MICROWAVE_TOOL::createFootprint`, `createBaseFootprint` (`pcbnew/microwave/microwave_footprint.cpp`), `createMicrowaveInductor`,
//! `BuildCornersList_S_Shape` and `gen_arc` (`microwave_inductor.cpp`), `createPolygonShape` and `MWAVE_POLYGONAL_SHAPE_DLG::ReadDataShapeDescr`
//! (`microwave_polygon.cpp`), with `PCB_BASE_FRAME::CreateNewFootprint` (`footprint_libraries_utils.cpp`) for the fields every one of them starts from.
//! KiCad works in nanometres, as `int`s: the arithmetic here is theirs, in nanometres, and so are the IR's coordinates.

use super::pcb_transform::FlipDirection;
use super::Board;
use eda_model::footprint::{PadAnchor, PadCustom, PadKind, PadPrimitive};
use eda_model::fp_edit::{FieldLayout, FootprintAttrs, FootprintEdit, FootprintKind};
use eda_model::ir::{BoardPart, FootprintAttributes, FootprintExtra, FootprintInstance, LabelSide, LibraryFootprint, LibraryPad, LibraryPadShape, PadMaskInfo, Point, Shape, Side, Text, TextJustify, Nm};
use eda_model::rules::TextGraphicsDefaults;
use eda_model::CheckResult;
use serde::{Deserialize, Serialize};

fn fail(check: &str, what: &str, msg: impl Into<String>) -> Vec<CheckResult> {
    vec![CheckResult::fail(check, what, msg.into())]
}

// ---------------------------------------------------------------------------------------------------------------------------------- the verbs

/// What was done to the footprint while it was in the hand, before the click that put it down (`doInteractiveItemPlacement`'s `IPO_ROTATE | IPO_FLIP`: R turns it, F turns it over):
/// the turn is `Cmd::RotateItems`'s (clockwise-positive millidegrees, about the footprint's own anchor), then the flip is `Cmd::FlipItems`'s.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Hand {
    #[serde(default)]
    pub rotate_millideg: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub flip: Option<FlipDirection>,
}

impl Hand {
    pub fn is_none(&self) -> bool {
        *self == Hand::default()
    }
}

/// `MWAVE_POLYGONAL_SHAPE_DLG`'s Shape box: how the points of the shape file are used (`g_PolyShapeType`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ShapeKind {
    /// "Normal": the polygon as the file has it, above the line between the two pads.
    #[default]
    Normal,
    /// "Symmetrical": the polygon and its mirror image about that line.
    Symmetrical,
    /// "Mirrored": the polygon below the line (`g_ShapeScaleY = -g_ShapeScaleY`).
    Mirrored,
}

fn default_inductor_name() -> String {
    "L".to_string()
}

/// One microwave tool's work after its dialogs. On the wire `{"op": "microwave", "verb": "<verb>", ...}`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "verb", rename_all = "snake_case")]
pub enum MicrowaveCmd {
    /// Draw Microwave Gaps (`createFootprint( GAP )`, after "Gap Size:"): two square pads of the current track width, `size_nm` apart, on a footprint named `muwave_gap`. `at` is where the click
    /// put the footprint; `track_width_nm` is `GetCurrentTrackWidth()`, which sizes the pads and the texts.
    Gap {
        at: Point,
        size_nm: Nm,
        track_width_nm: Nm,
        #[serde(default, skip_serializing_if = "Hand::is_none")]
        hand: Hand,
    },
    /// Draw Microwave Stubs (`createFootprint( STUB )`, after "Stub Size:"): a square pad and, `size_nm` long, the stub on the footprint `muwave_stub`.
    Stub {
        at: Point,
        size_nm: Nm,
        track_width_nm: Nm,
        #[serde(default, skip_serializing_if = "Hand::is_none")]
        hand: Hand,
    },
    /// Draw Microwave Arc Stubs (`createFootprint( STUB_ARC )`, after "Arc Stub Radius Value:" and "Angle in degrees:"): one custom pad, a pie slice of `radius_nm` and `angle_millideg`
    /// (clamped to 0..180 degrees as `createFootprint` does) on the footprint `muwave_arcstub`.
    ArcStub {
        at: Point,
        radius_nm: Nm,
        angle_millideg: i64,
        track_width_nm: Nm,
        #[serde(default, skip_serializing_if = "Hand::is_none")]
        hand: Hand,
    },
    /// Draw Microwave Polygonal Shapes (`createPolygonShape`, after the Complex Shape dialog): `file` is the shape description file's text (`Unit=`, `XScale=`, `YScale=`, `$COORD` ..
    /// `$ENDCOORD`, see [`read_shape_file`]); `kind` is the Shape box; `size_x_nm` and `size_y_nm` are the Size boxes -- the file's own scales when absent (the boxes show them once a file
    /// is read). The footprint is `muwave_polygon`: two pads, `size_x` apart, and the polygon between them on the copper layer.
    FunctionShape {
        at: Point,
        file: String,
        #[serde(default)]
        kind: ShapeKind,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        size_x_nm: Option<Nm>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        size_y_nm: Option<Nm>,
        track_width_nm: Nm,
        #[serde(default, skip_serializing_if = "Hand::is_none")]
        hand: Hand,
    },
    /// Draw Microwave Lines (`createInductorBetween` after the two clicks, and `createMicrowaveInductor` after "Length of track:" and "Component value:"): the S-shaped coil of `length_nm`
    /// between `start` and `end`, drawn with the current track width, on a footprint named `mw_inductor` whose value is `name`. Refused with KiCad's words when the length cannot be
    /// made ("Requested length < minimum length", "Requested length too large", "Requested length too small", "Requested length can't be represented").
    Inductor {
        start: Point,
        end: Point,
        length_nm: Nm,
        #[serde(default = "default_inductor_name")]
        name: String,
        track_width_nm: Nm,
    },
}

impl MicrowaveCmd {
    /// The ids the command names, for the journal and the activity feed.
    pub fn ids(&self) -> Vec<&str> {
        vec![match self {
            MicrowaveCmd::Gap { .. } => GAP_NAME,
            MicrowaveCmd::Stub { .. } => STUB_NAME,
            MicrowaveCmd::ArcStub { .. } => ARC_STUB_NAME,
            MicrowaveCmd::FunctionShape { .. } => POLYGON_NAME,
            MicrowaveCmd::Inductor { .. } => INDUCTOR_NAME,
        }]
    }

    /// The one-line description the CLI journal and the activity feed show.
    pub fn describe(&self) -> String {
        let mm = |v: Nm| format!("{:.3}", eda_model::units::to_mm(v));
        match self {
            MicrowaveCmd::Gap { at, size_nm, track_width_nm, .. } => format!("microwave gap --at {},{} --size {} --track-width {}", mm(at.x), mm(at.y), mm(*size_nm), mm(*track_width_nm)),
            MicrowaveCmd::Stub { at, size_nm, track_width_nm, .. } => format!("microwave stub --at {},{} --size {} --track-width {}", mm(at.x), mm(at.y), mm(*size_nm), mm(*track_width_nm)),
            MicrowaveCmd::ArcStub { at, radius_nm, angle_millideg, track_width_nm, .. } => {
                format!("microwave arc-stub --at {},{} --radius {} --angle {} --track-width {}", mm(at.x), mm(at.y), mm(*radius_nm), *angle_millideg as f64 / 1000.0, mm(*track_width_nm))
            }
            MicrowaveCmd::FunctionShape { at, file, kind, track_width_nm, .. } => format!("microwave shape --at {},{} --kind {:?} --track-width {} ({} bytes of shape file)", mm(at.x), mm(at.y), kind, mm(*track_width_nm), file.len()),
            MicrowaveCmd::Inductor { start, end, length_nm, name, track_width_nm } => {
                format!("microwave inductor --from {},{} --to {},{} --length {} --name {name:?} --track-width {}", mm(start.x), mm(start.y), mm(end.x), mm(end.y), mm(*length_nm), mm(*track_width_nm))
            }
        }
    }

    /// The kind of step it is, for the journal's summary line.
    pub fn kind(&self) -> &'static str {
        "microwave"
    }
}

/// The footprint names the tools give (`cmp_name` of `createFootprint`, `createPolygonShape`; `LIB_ID( wxEmptyString, wxT( "mw_inductor" ) )`).
pub const GAP_NAME: &str = "muwave_gap";
pub const STUB_NAME: &str = "muwave_stub";
pub const ARC_STUB_NAME: &str = "muwave_arcstub";
pub const POLYGON_NAME: &str = "muwave_polygon";
pub const INDUCTOR_NAME: &str = "mw_inductor";

// ------------------------------------------------------------------------------------------------------------------------ KiCad's arithmetic

/// `ARC_HIGH_DEF`: the error `gen_arc` allows an arc approximated by segments, 0.005 mm.
const ARC_HIGH_DEF_NM: i64 = 5_000;
/// `pcbIUScale.IU_PER_MM` and the inch the shape file's `Unit=inch` means (`IU_PER_MILS * 1000`).
const IU_PER_MM: f64 = 1_000_000.0;
const IU_PER_INCH: f64 = 25_400_000.0;

/// A point in KiCad's units.
type P = (i64, i64);

fn point_nm(p: P) -> Point {
    Point { x: Nm::from_raw_shim(p.0), y: Nm::from_raw_shim(p.1) }
}

/// `KiROUND`: `llround`, half away from zero.
fn ki_round(v: f64) -> i64 {
    v.round() as i64
}

const DEGREES_TO_RADIANS: f64 = std::f64::consts::PI / 180.0;

/// `EDA_ANGLE( const VECTOR2D& )`, in degrees: the cardinal and diagonal directions exactly, `atan2` for the rest.
fn angle_of_vector(x: f64, y: f64) -> f64 {
    if x == 0.0 && y == 0.0 {
        0.0
    } else if y == 0.0 {
        if x >= 0.0 {
            0.0
        } else {
            -180.0
        }
    } else if x == 0.0 {
        if y >= 0.0 {
            90.0
        } else {
            -90.0
        }
    } else if x == y {
        if x >= 0.0 {
            45.0
        } else {
            -180.0 + 45.0
        }
    } else if x == -y {
        if x >= 0.0 {
            -45.0
        } else {
            180.0 - 45.0
        }
    } else {
        y.atan2(x) / DEGREES_TO_RADIANS
    }
}

/// `EDA_ANGLE::Normalize`: into [0, 360).
fn normalize(mut degrees: f64) -> f64 {
    while degrees < 0.0 {
        degrees += 360.0;
    }
    while degrees >= 360.0 {
        degrees -= 360.0;
    }
    degrees
}

/// `EDA_ANGLE::Sin`: exact for the multiples of 45 degrees.
fn sin_deg(degrees: f64) -> f64 {
    let t = normalize(degrees);
    if t == 0.0 || t == 180.0 {
        0.0
    } else if t == 45.0 || t == 135.0 {
        std::f64::consts::FRAC_1_SQRT_2
    } else if t == 225.0 || t == 315.0 {
        -std::f64::consts::FRAC_1_SQRT_2
    } else if t == 90.0 {
        1.0
    } else if t == 270.0 {
        -1.0
    } else {
        (degrees * DEGREES_TO_RADIANS).sin()
    }
}

/// `EDA_ANGLE::Cos`.
fn cos_deg(degrees: f64) -> f64 {
    let t = normalize(degrees);
    if t == 0.0 {
        1.0
    } else if t == 180.0 {
        -1.0
    } else if t == 90.0 || t == 270.0 {
        0.0
    } else if t == 45.0 || t == 315.0 {
        std::f64::consts::FRAC_1_SQRT_2
    } else if t == 135.0 || t == 225.0 {
        -std::f64::consts::FRAC_1_SQRT_2
    } else {
        (degrees * DEGREES_TO_RADIANS).cos()
    }
}

/// `RotatePoint( int*, int*, EDA_ANGLE )` (`libs/kimath/src/trigo.cpp`): about the origin, KiCad's sign (y down, a positive angle turns x toward -y).
fn rotate_point(p: P, degrees: f64) -> P {
    let a = normalize(degrees);
    if a == 0.0 {
        p
    } else if a == 90.0 {
        (p.1, -p.0)
    } else if a == 180.0 {
        (-p.0, -p.1)
    } else if a == 270.0 {
        (-p.1, p.0)
    } else {
        let (s, c) = (sin_deg(a), cos_deg(a));
        (ki_round(p.1 as f64 * s + p.0 as f64 * c), ki_round(p.1 as f64 * c - p.0 as f64 * s))
    }
}

/// `RotatePoint( int*, int*, int cx, int cy, EDA_ANGLE )`.
fn rotate_point_about(p: P, centre: P, degrees: f64) -> P {
    let r = rotate_point((p.0 - centre.0, p.1 - centre.1), degrees);
    (r.0 + centre.0, r.1 + centre.1)
}

/// `VECTOR2<int>::EuclideanNorm`: rounded, with the axes and the diagonals taken exactly.
fn euclidean_norm(p: P) -> i64 {
    if p.0.abs() == p.1.abs() {
        return ki_round(p.0.abs() as f64 * std::f64::consts::SQRT_2);
    }
    if p.0 == 0 {
        return p.1.abs();
    }
    if p.1 == 0 {
        return p.0.abs();
    }
    ki_round((p.0 as f64).hypot(p.1 as f64))
}

/// `VECTOR2<double>::EuclideanNorm`.
fn euclidean_norm_f(x: f64, y: f64) -> f64 {
    if x.abs() == y.abs() {
        return x.abs() * std::f64::consts::SQRT_2;
    }
    if x == 0.0 {
        return y.abs();
    }
    if y == 0.0 {
        return x.abs();
    }
    x.hypot(y)
}

/// `GetArcToSegmentCount` (`libs/kimath/src/geometry/geometry_utils.cpp`): how many chords approximate an arc of `arc_degrees` within `error_max` of the circle.
fn arc_to_segment_count(radius: i64, error_max: i64, arc_degrees: f64) -> i64 {
    let radius = radius.max(1);
    let error_max = error_max.max(1);
    let rel_error = error_max as f64 / radius as f64;
    let arc_increment = 180.0 / std::f64::consts::PI * (1.0 - rel_error).acos() * 2.0;
    // At least `MIN_SEGCOUNT_FOR_CIRCLE` (8) chords to a circle; a NaN (a radius under the error) takes the cap.
    let arc_increment = arc_increment.min(360.0 / 8.0);
    ki_round(arc_degrees.abs() / arc_increment).max(2)
}

// ------------------------------------------------------------------------------------------------------------------- the inductor's S shape

/// What `BuildCornersList_S_Shape` answers when it cannot make the coil.
#[derive(Debug, Clone, PartialEq)]
pub enum SShape {
    /// The corners, in nanometres, from the start point to the end point.
    Corners(Vec<(i64, i64)>),
    TooLong,
    TooShort,
    NoRepresentation,
}

/// `gen_arc` (`microwave_inductor.cpp`): the points of an arc approximated by chords, from `start` (not included) round `centre` by `arc_degrees`.
fn gen_arc(out: &mut Vec<P>, start: P, centre: P, arc_degrees: f64) {
    let (fx, fy) = ((start.0 - centre.0) as f64, (start.1 - centre.1) as f64);
    let radius = euclidean_norm_f(fx, fy);
    let seg_count = arc_to_segment_count(radius as i64, ARC_HIGH_DEF_NM, arc_degrees);
    let increment = arc_degrees * DEGREES_TO_RADIANS / seg_count as f64;
    for ii in 1..=seg_count {
        let rot = increment * ii as f64;
        let (fcos, fsin) = (rot.cos(), rot.sin());
        let x = ki_round(fx * fcos + fy * fsin);
        let y = ki_round(fy * fcos - fx * fsin);
        out.push((centre.0 + x, centre.1 + y));
    }
}

/// `BuildCornersList_S_Shape` (`microwave_inductor.cpp`): the corners of an S-shaped coil of `length` between `start` and `end`, drawn `width` wide -- all in nanometres. The coil is
/// worked out for a vertical one (strands along x, the two stubs on y) and turned to where the two points are.
pub fn build_corners_s_shape(start: (i64, i64), end: (i64, i64), length: i64, width: i64) -> SShape {
    // The scale factor that adjusts the arc length for the arcs being chords.
    const ADJUST_SIZE: f64 = 0.988;
    let pt = (end.0 - start.0, end.1 - start.1);
    let mut angle = angle_of_vector(pt.0 as f64, pt.1 as f64);
    let min_len = euclidean_norm(pt);
    angle = -angle;

    // The size of the area (for a vertical shape).
    let size = (min_len / 2, min_len);
    // A reasonable starting radius for the arcs.
    let mut radius = (width * 5).min(size.0 / 4);

    let mut segm_count: i64 = 0; // full-length strands (the first and the last are half ones and not counted)
    let mut stubs_len;
    let mut segm_len;
    let mut full_len;
    loop {
        stubs_len = (size.1 - radius * 2 * (segm_count + 2)) / 2;
        if stubs_len < size.1 / 10 {
            // Reduce the radius.
            stubs_len = size.1 / 10;
            radius = (size.1 - 2 * stubs_len) / (2 * (segm_count + 2));
            if radius < width {
                // The requested length is too large for the room.
                return SShape::TooLong;
            }
        }
        segm_len = size.0 - radius * 2;
        full_len = 2 * stubs_len; // the connections to the coil
        full_len += segm_len * segm_count; // the full-length strands
        full_len += ki_round((segm_count + 2) as f64 * std::f64::consts::PI * ADJUST_SIZE * radius as f64); // the arcs
        full_len += segm_len - 2 * radius; // the first and the last strand
        if full_len >= length {
            break;
        }
        segm_count += 1;
        // KiCad's loop ends because the radius shrinks below the width; a width that is not a width would never end it.
        if segm_count > 1_000_000 {
            return SShape::TooLong;
        }
    }

    // Adjust the length by adjusting segm_len: reduce the strands (the full ones and the two half ones, which make one more).
    let delta_size = full_len - length;
    segm_len -= delta_size / (segm_count + 1);

    // At this point the requested length can still be too short (four quarter circles are too long): rejected, as KiCad does.
    let min_total_length = (2 * stubs_len) as f64 + 2.0 * std::f64::consts::PI * ADJUST_SIZE * radius as f64;
    if min_total_length as i64 > length {
        return SShape::TooShort;
    }
    if segm_len - 2 * radius < 0 {
        // This exact length cannot be made with this number of strands (adding one adds a half circle too).
        return SShape::NoRepresentation;
    }

    let mut buffer: Vec<P> = Vec::new();
    // The first stub and the first arc (a quarter turn).
    let mut pt = start;
    buffer.push(pt);
    pt.1 += stubs_len;
    buffer.push(pt);
    let mut centre = pt;
    centre.0 -= radius;
    gen_arc(&mut buffer, pt, centre, -90.0);
    pt = *buffer.last().expect("the arc has points");

    let half_size_seg_len = segm_len / 2 - radius;
    if half_size_seg_len != 0 {
        pt.0 -= half_size_seg_len;
        buffer.push(pt);
    }

    // The strands.
    let mut sign: i64 = 1;
    segm_count += 1; // one more, for the last half-size strand
    for ii in 0..segm_count {
        sign = if ii & 1 != 0 { -1 } else { 1 }; // odd arcs turn the other way
        centre = pt;
        centre.1 += radius;
        gen_arc(&mut buffer, pt, centre, 180.0 * sign as f64);
        pt = *buffer.last().expect("the arc has points");
        pt.0 += segm_len * sign;
        buffer.push(pt);
    }

    // The last point is the end of a full strand, and must be the end of the second half-size one.
    sign *= -1;
    buffer.last_mut().expect("the strands have points").0 = start.0 + radius * sign;

    // The last arc.
    pt = *buffer.last().expect("the strands have points");
    centre = pt;
    centre.1 += radius;
    gen_arc(&mut buffer, pt, centre, 90.0 * sign as f64);

    // Turn the coil to where the two points are.
    angle += 90.0;
    for p in buffer.iter_mut() {
        *p = rotate_point_about(*p, start, angle);
    }
    buffer.push(end);
    SShape::Corners(buffer)
}

// ------------------------------------------------------------------------------------------------------------------ the shape description file

/// `MWAVE_POLYGONAL_SHAPE_DLG::ReadDataShapeDescr`'s result: the points of `$COORD` (`g_PolyEdges`, normalized to 0..1) and the scales (`g_ShapeScaleX`, `g_ShapeScaleY`) in nanometres:
/// `XScale` and `YScale` times the unit.
#[derive(Debug, Clone, PartialEq)]
pub struct ShapeFile {
    pub edges: Vec<(f64, f64)>,
    pub scale_x: f64,
    pub scale_y: f64,
}

/// C's `atof`: the longest numeric prefix of `s` (leading blanks skipped), 0 when there is none.
fn atof(s: &str) -> f64 {
    let s = s.trim_start();
    let b = s.as_bytes();
    let mut end = 0;
    if end < b.len() && (b[end] == b'+' || b[end] == b'-') {
        end += 1;
    }
    let digits_from = end;
    while end < b.len() && b[end].is_ascii_digit() {
        end += 1;
    }
    if end < b.len() && b[end] == b'.' {
        end += 1;
        while end < b.len() && b[end].is_ascii_digit() {
            end += 1;
        }
    }
    if end == digits_from || (end == digits_from + 1 && b[digits_from] == b'.') {
        return 0.0;
    }
    // an exponent only counts with digits after it
    if end < b.len() && (b[end] == b'e' || b[end] == b'E') {
        let mut e = end + 1;
        if e < b.len() && (b[e] == b'+' || b[e] == b'-') {
            e += 1;
        }
        let exp_digits = e;
        while e < b.len() && b[e].is_ascii_digit() {
            e += 1;
        }
        if e > exp_digits {
            end = e;
        }
    }
    s[..end].parse().unwrap_or(0.0)
}

/// C's `strtok`: the token of `s` that starts at or after `from` (leading `delims` skipped), and where the next call goes on -- past the one delimiter that ended the token, which `strtok`
/// overwrites with a NUL. An empty token is `strtok`'s NULL.
fn strtok(s: &str, from: usize, delims: &[char]) -> (usize, usize, usize) {
    let bytes = s.as_bytes();
    let is_delim = |i: usize| delims.iter().any(|d| *d as u32 == bytes[i] as u32 && d.is_ascii());
    let mut start = from.min(s.len());
    while start < s.len() && is_delim(start) {
        start += 1;
    }
    let mut end = start;
    while end < s.len() && !is_delim(end) {
        end += 1;
    }
    (start, end, (end + 1).min(s.len()))
}

fn starts_with_ci(s: &str, prefix: &str) -> bool {
    s.len() >= prefix.len() && s.is_char_boundary(prefix.len()) && s[..prefix.len()].eq_ignore_ascii_case(prefix)
}

/// `ReadDataShapeDescr`, over the text of the file:
///
/// ```text
/// Unit=MM
/// XScale=271.501
/// YScale=1.00133
///
/// $COORD
/// 0                      0.6112600148417837
/// 0.001851851851851852   0.6104800531118608
/// $ENDCOORD
/// ```
///
/// Each `$COORD` line is an X and a Y, normalized from 0 to 1. Lines that start with `#` (and empty ones) are skipped (`FILTER_READER`); `Unit` is `inch` or `mm` (the default), the scales
/// default to 1. As in KiCad, the first token of a line is split at blanks and `=`, the second at blanks and tabs (so `XScale = 3` has `=` for its value, and no scale), and the keywords
/// match by their first letters, any case -- `XScale` and `YScale` only at the very start of the line.
pub fn read_shape_file(text: &str) -> ShapeFile {
    let mut unit = IU_PER_MM;
    let (mut scale_x, mut scale_y) = (1.0_f64, 1.0_f64);
    let mut edges: Vec<(f64, f64)> = Vec::new();
    // `FILTER_READER`: a line starting with '#', '\n' or '\r' is not read.
    let mut lines = text.lines().filter(|l| !(l.is_empty() || l.starts_with('#')));
    while let Some(line) = lines.next() {
        let (s1, e1, next) = strtok(line, 0, &[' ', '=', '\n', '\r']);
        let param1 = &line[s1..e1];
        let (s2, e2, _) = strtok(line, next, &[' ', '\t', '\n', '\r']);
        let param2 = &line[s2..e2];
        if starts_with_ci(param1, "Unit") {
            if starts_with_ci(param2, "inch") {
                unit = IU_PER_INCH;
            }
            if starts_with_ci(param2, "mm") {
                unit = IU_PER_MM;
            }
        }
        if starts_with_ci(param1, "$ENDCOOR") {
            break;
        }
        if starts_with_ci(param1, "$COORD") {
            for inner in lines.by_ref() {
                let (a1, b1, n) = strtok(inner, 0, &[' ', '\t', '\n', '\r']);
                let (a2, b2, _) = strtok(inner, n, &[' ', '\t', '\n', '\r']);
                if starts_with_ci(&inner[a1..b1], "$ENDCOOR") {
                    break;
                }
                edges.push((atof(&inner[a1..b1]), atof(&inner[a2..b2])));
            }
        }
        // `Line` itself, cut at the end of the first token by `strtok`: leading blanks included.
        let head = &line[..e1];
        if starts_with_ci(head, "XScale") {
            scale_x = atof(param2);
        }
        if starts_with_ci(head, "YScale") {
            scale_y = atof(param2);
        }
    }
    ShapeFile { edges, scale_x: scale_x * unit, scale_y: scale_y * unit }
}

// ------------------------------------------------------------------------------------------------------------------------- the footprints

/// A footprint a tool built, before it is put on the board: its definition, the layouts of its two fields and the value the part shows.
pub struct Built {
    /// The footprint as the board part carries it (`name` is the base name; the board makes it unique).
    pub footprint: LibraryFootprint,
    pub reference: FieldLayout,
    pub value: FieldLayout,
    pub value_text: String,
}

/// `PCB_BASE_FRAME::CreateNewFootprint` and `MICROWAVE_TOOL::createBaseFootprint`: the footprint every tool starts from -- the Reference `REF**` on the silkscreen, the Value on the fabrication
/// layer and the `${REFERENCE}` text there, each of the size the board's Text & Graphics defaults give its layer (`text_size` overrides the size of the two fields, thickness a fifth of it, when
/// above 0); the attributes `exclude from position files` and `exclude from BOM`; `pad_count` square SMD pads of the track width at the origin, on the front copper layer and nowhere else.
fn create_base_footprint(defaults: &TextGraphicsDefaults, name: &str, value: &str, text_size: Nm, pad_count: usize, track_width: Nm) -> Built {
    // `default_pos` of `CreateNewFootprint`: the Reference half a text height above the origin, the Value below it, the next text under that.
    let silk_h = defaults.text_size_nm("F.SilkS").1;
    let fab_h = defaults.text_size_nm("F.Fab").1;
    let mut y = Nm::ZERO;
    y -= silk_h / 2;
    let reference_y = y;
    y += silk_h;
    y += fab_h / 2;
    let value_y = y;
    y += fab_h;
    y += fab_h / 2;
    let user_y = y;

    let styled = |at_y: i64, layer: &str| {
        let mut l = FieldLayout::new(Point { x: Nm::ZERO, y: Nm::from_raw_shim(at_y) }, layer);
        l.size = defaults.text_size_nm(layer);
        l.thickness = defaults.text_thickness_nm(layer);
        let (italic, upright) = defaults.text_italic_upright(layer);
        l.italic = italic;
        l.keep_upright = upright;
        l
    };
    let mut reference = styled(reference_y.raw_shim(), "F.SilkS");
    let mut value_layout = styled(value_y.raw_shim(), "F.Fab");
    if text_size > Nm::ZERO {
        for l in [&mut reference, &mut value_layout] {
            l.size = (text_size, text_size);
            l.thickness = text_size / 5;
        }
    }

    let pads: Vec<LibraryPad> = (1..=pad_count)
        .map(|n| LibraryPad {
            id: String::new(),
            stack: Default::default(),
            number: n.to_string(),
            at: Point { x: Nm::ZERO, y: Nm::ZERO },
            size: (track_width, track_width),
            offset: Point::default(),
            shape: LibraryPadShape::Rect,
            kind: PadKind::Smd,
            drill: None,
            drill_slot: None,
            rot: 0,
            roundrect_ratio: None,
            trapezoid_delta: None,
            chamfer_ratio: None,
            chamfer_corners: Default::default(),
            custom: None,
            layers: vec!["F.Cu".to_string()],
            clearance_override: None,
            thermal_gap_override: None,
            thermal_spoke_width_override: None,
            zone_connection: None,
            thermal_spoke_angle_mdeg: None,
            kicad: Default::default(),
        })
        .collect();

    // `m_DefaultFPTextItems`' third item: `${REFERENCE}` on the fabrication layer, a text of the footprint.
    let (_, user_h) = defaults.text_size_nm("F.Fab");
    let user_text = Text {
        id: String::new(),
        content: "${REFERENCE}".to_string(),
        at: Point { x: Nm::ZERO, y: user_y },
        angle: 0,
        layer: "F.Fab".to_string(),
        size_nm: user_h,
        stroke_width: defaults.text_thickness_nm("F.Fab"),
        justify: TextJustify::Center,
        mirror: false,
    };

    let footprint = LibraryFootprint {
        name: name.to_string(),
        attributes: FootprintAttributes { exclude_from_bom: true, exclude_from_position_files: true, ..Default::default() },
        pads,
        texts: vec![user_text],
        reference: Some(reference.clone()),
        value: Some(value_layout.clone()),
        // KiCad's tools make no courtyard, and the file has none (`FootprintExtra::no_courtyard`); the engine would otherwise derive a keep-out from the pads, and the placement gates
        // would refuse (under Strict) what these footprints are for -- elements that touch and overlap, a stub on its line. A point (1 nm) keeps them out of that; the copper is still judged
        // by the routing and DRC checks.
        courtyard: Some((Nm::from_raw_shim(1), Nm::from_raw_shim(1))),
        ..LibraryFootprint::new_empty(name)
    };
    Built { footprint, reference, value: value_layout, value_text: value.to_string() }
}

fn check_width(what: &str, w: Nm) -> Result<(), Vec<CheckResult>> {
    if w <= Nm::ZERO {
        return Err(fail("ops_bad_microwave", what, "the track width must be greater than zero"));
    }
    Ok(())
}

/// `createFootprint( GAP )`: two pads `gap` apart, the first on the left of the origin, the second on the right.
fn build_gap(defaults: &TextGraphicsDefaults, gap_nm: Nm, track_width: Nm) -> Result<Built, Vec<CheckResult>> {
    check_width(GAP_NAME, track_width)?;
    if gap_nm <= Nm::ZERO {
        return Err(fail("ops_bad_microwave", GAP_NAME, "the size of a gap must be greater than zero"));
    }
    // The texts take the track width as their size (`text_size = gap_size` is read before the dialog changes `gap_size`).
    let mut built = create_base_footprint(defaults, GAP_NAME, GAP_NAME, track_width, 2, track_width);
    let (gap, w) = (gap_nm, track_width);
    let offset_x = -(gap + w) / 2;
    built.footprint.pads[0].at.x = offset_x;
    built.footprint.pads[1].at.x = offset_x + gap + w;
    Ok(built)
}

/// `createFootprint( STUB )`: a square pad at the origin and, above it, the stub: a pad `track width` wide and `stub` long.
fn build_stub(defaults: &TextGraphicsDefaults, stub_nm: Nm, track_width: Nm) -> Result<Built, Vec<CheckResult>> {
    check_width(STUB_NAME, track_width)?;
    if stub_nm <= Nm::ZERO {
        return Err(fail("ops_bad_microwave", STUB_NAME, "the size of a stub must be greater than zero"));
    }
    let mut built = create_base_footprint(defaults, STUB_NAME, STUB_NAME, track_width, 2, track_width);
    let (stub, h) = (stub_nm, track_width);
    let offset_y = -(stub + h) / 2;
    built.footprint.pads[1].size = (track_width, stub_nm);
    built.footprint.pads[1].at.y = offset_y;
    Ok(built)
}

/// `createFootprint( STUB_ARC )`: one custom pad with a rectangular anchor and a polygon primitive, a pie slice of `radius` and `angle_millideg` (0 to 180 degrees) opening upward from the origin.
fn build_arc_stub(defaults: &TextGraphicsDefaults, radius_nm: Nm, angle_millideg: i64, track_width: Nm) -> Result<Built, Vec<CheckResult>> {
    check_width(ARC_STUB_NAME, track_width)?;
    if radius_nm <= Nm::ZERO {
        return Err(fail("ops_bad_microwave", ARC_STUB_NAME, "the radius of an arc stub must be greater than zero"));
    }
    // `if( angle < ANGLE_0 ) angle = -angle; if( angle > ANGLE_180 ) angle = ANGLE_180;`
    let angle = (angle_millideg.abs().min(180_000)) as f64 / 1000.0;
    // The texts keep the board's default size (`text_size` is 0 for the arc stub).
    let mut built = create_base_footprint(defaults, ARC_STUB_NAME, ARC_STUB_NAME, Nm::ZERO, 1, track_width);
    let radius = radius_nm;

    let num_points = (angle / 5.0 + 3.0) as i64;
    let mut poly: Vec<P> = Vec::with_capacity(num_points.max(3) as usize);
    poly.push((0, 0));
    let mut theta = -angle / 2.0;
    for _ in 1..(num_points - 1) {
        poly.push(rotate_point((0, (-radius).raw_shim()), theta));
        theta += 5.0;
        if theta > angle / 2.0 {
            theta = angle / 2.0;
        }
    }
    // Close the polygon.
    poly.push(poly[0]);

    let pad = &mut built.footprint.pads[0];
    pad.shape = LibraryPadShape::Custom;
    pad.custom = Some(PadCustom {
        anchor: PadAnchor::Rect,
        zone: Default::default(),
        primitives: vec![PadPrimitive { shape: Shape::Polygon { id: String::new(), layer: String::new(), stroke_width: Nm::ZERO, filled: true, pts: poly.into_iter().map(point_nm).collect() }, proxy: false }],
    });
    Ok(built)
}

/// `createPolygonShape`: two pads `size_x` apart and, between them, the polygon the file describes, filled, on the copper layer with no outline. The scales are the file's unless the dialog's
/// Size boxes say otherwise; `Mirrored` turns the polygon below the line, `Symmetrical` adds its mirror image.
fn build_function_shape(defaults: &TextGraphicsDefaults, file: &str, kind: ShapeKind, size_x_nm: Option<Nm>, size_y_nm: Option<Nm>, track_width: Nm) -> Result<Built, Vec<CheckResult>> {
    check_width(POLYGON_NAME, track_width)?;
    let shape = read_shape_file(file);
    let scale_x = size_x_nm.map_or(shape.scale_x, |v| v.raw_shim() as f64);
    let mut scale_y = size_y_nm.map_or(shape.scale_y, |v| v.raw_shim() as f64);
    if kind == ShapeKind::Mirrored {
        scale_y = -scale_y;
    }
    // `g_ShapeSize` is the scales, rounded; the points are placed by the scales themselves.
    let size = (ki_round(scale_x), ki_round(scale_y));
    if size.0 == 0 || size.1 == 0 {
        return Err(fail("ops_bad_microwave", POLYGON_NAME, "Shape has a null size."));
    }
    if shape.edges.is_empty() {
        return Err(fail("ops_bad_microwave", POLYGON_NAME, "Shape has no points."));
    }

    let mut built = create_base_footprint(defaults, POLYGON_NAME, POLYGON_NAME, Nm::ZERO, 2, track_width);
    // The anchor in the middle of the shape's length.
    let offset = (-size.0 / 2, 0_i64);
    built.footprint.pads[0].at.x = Nm::from_raw_shim(offset.0);
    built.footprint.pads[1].at.x = Nm::from_raw_shim(offset.0 + size.0);

    let mut poly: Vec<P> = Vec::with_capacity(shape.edges.len() * 2 + 2);
    poly.push((offset.0, 0));
    let mut last: P = (0, 0);
    for &(x, y) in &shape.edges {
        last = (ki_round(x * scale_x), -ki_round(y * scale_y));
        last = (last.0 + offset.0, last.1 + offset.1);
        poly.push(last);
    }
    // Finish the polygon.
    if last.1 != 0 {
        poly.push((last.0, 0));
    }
    if kind == ShapeKind::Symmetrical {
        // The mirror image about the line between the pads, in reverse order.
        for i in (0..poly.len()).rev() {
            let p = poly[i];
            poly.push((p.0, -p.1));
        }
    }
    built.footprint.graphics.push(Shape::Polygon { id: String::new(), layer: "F.Cu".to_string(), stroke_width: Nm::ZERO, filled: true, pts: poly.into_iter().map(point_nm).collect() });
    Ok(built)
}

/// What `createMicrowaveInductor` asks of the length once the first dialog has answered, before it asks the value: the corners of the coil in nanometres, or the words of the info bar that
/// says why not (`aErrorMessage`). The studio asks it before the "Component value:" prompt, as KiCad's order of dialogs does (`Cmd::Microwave` re-asks it, so a replay needs no prompt).
pub fn inductor_corners(start: Point, end: Point, length_nm: Nm, width_nm: Nm) -> Result<Vec<(i64, i64)>, String> {
    if width_nm <= Nm::ZERO {
        return Err("the track width must be greater than zero".to_string());
    }
    let (s, e) = ((start.x.raw_shim(), start.y.raw_shim()), (end.x.raw_shim(), end.y.raw_shim()));
    let min_len = euclidean_norm((e.0 - s.0, e.1 - s.1));
    if min_len == 0 {
        return Err("the two ends of the inductor are the same point".to_string());
    }
    let length = length_nm;
    if length < Nm::from_raw_shim(min_len) {
        return Err("Requested length < minimum length".to_string());
    }
    match build_corners_s_shape(s, e, length.raw_shim(), width_nm.raw_shim()) {
        SShape::Corners(c) => Ok(c),
        SShape::TooLong => Err("Requested length too large".to_string()),
        SShape::TooShort => Err("Requested length too small".to_string()),
        SShape::NoRepresentation => Err("Requested length can't be represented".to_string()),
    }
}

/// `FOOTPRINT_NAME_VALIDATOR`'s list (`%$<>\t\n\r"\/:`): the characters the "Component value:" box does not take.
pub const ILLEGAL_NAME_CHARS: &str = "%$<>\t\n\r\"\\/:";

/// What `createMicrowaveInductor` makes of the two points, in nanometres, relative to the footprint (whose anchor is the end point): the segments of the coil and the two pads.
fn build_inductor(defaults: &TextGraphicsDefaults, start: Point, end: Point, length_nm: Nm, name: &str, width_nm: Nm) -> Result<Built, Vec<CheckResult>> {
    check_width(INDUCTOR_NAME, width_nm)?;
    let (s, e) = ((start.x.raw_shim(), start.y.raw_shim()), (end.x.raw_shim(), end.y.raw_shim()));
    let corners = inductor_corners(start, end, length_nm, width_nm).map_err(|m| fail("ops_bad_microwave", INDUCTOR_NAME, m))?;
    let name = name.trim();
    if name.is_empty() {
        return Err(fail("ops_bad_microwave", INDUCTOR_NAME, "the inductor needs a value"));
    }
    if let Some(c) = name.chars().find(|c| ILLEGAL_NAME_CHARS.contains(*c)) {
        return Err(fail("ops_bad_microwave", INDUCTOR_NAME, format!("the value of an inductor cannot have {c:?} in it")));
    }

    let mut built = create_base_footprint(defaults, INDUCTOR_NAME, name, Nm::ZERO, 0, width_nm);
    let local = |p: P| point_nm((p.0 - e.0, p.1 - e.1));
    for w in corners.windows(2) {
        built.footprint.graphics.push(Shape::Segment { id: String::new(), layer: "F.Cu".to_string(), stroke_width: width_nm, filled: false, start: local(w[0]), end: local(w[1]) });
    }
    // A pad on each end of the coil: round, the width of the track, numbered 1 at the end point (the anchor) and 2 at the start.
    let round_pad = |number: &str, at: Point| LibraryPad {
        id: String::new(),
        stack: Default::default(),
        number: number.to_string(),
        at,
        size: (width_nm, width_nm),
        offset: Point::default(),
        shape: LibraryPadShape::Circle,
        kind: PadKind::Smd,
        drill: None,
        drill_slot: None,
        rot: 0,
        roundrect_ratio: None,
        trapezoid_delta: None,
        chamfer_ratio: None,
        chamfer_corners: Default::default(),
        custom: None,
        layers: vec!["F.Cu".to_string()],
        clearance_override: None,
        thermal_gap_override: None,
        thermal_spoke_width_override: None,
        zone_connection: None,
        thermal_spoke_angle_mdeg: None,
        kicad: Default::default(),
    };
    built.footprint.pads = vec![round_pad("1", Point { x: Nm::ZERO, y: Nm::ZERO }), round_pad("2", local(s))];

    // The texts at the middle of the coil, one text height above it and below it.
    let mid = ((s.0 + e.0) / 2, (s.1 + e.1) / 2);
    let silk_h = defaults.text_size_nm("F.SilkS").1;
    let fab_h = defaults.text_size_nm("F.Fab").1;
    built.reference.at = local((mid.0, mid.1 - silk_h.raw_shim()));
    built.value.at = local((mid.0, mid.1 + fab_h.raw_shim()));
    built.footprint.reference = Some(built.reference.clone());
    built.footprint.value = Some(built.value.clone());
    Ok(built)
}

// ----------------------------------------------------------------------------------------------------------------------- putting it on the board

/// The shape of a footprint definition, for telling whether two parts that share a name may share it: everything but the name and the ids.
fn same_shape(a: &LibraryFootprint, b: &LibraryFootprint) -> bool {
    let strip = |f: &LibraryFootprint| {
        let mut f = f.clone();
        f.name.clear();
        for p in &mut f.pads {
            p.id.clear();
        }
        for g in &mut f.graphics {
            g.set_id(String::new());
        }
        for t in &mut f.texts {
            t.id.clear();
        }
        f
    };
    strip(a) == strip(b)
}

impl Board<'_> {
    /// [`Cmd::Microwave`](crate::Cmd::Microwave).
    pub(crate) fn apply_microwave(&mut self, cmd: &MicrowaveCmd) -> Result<(), Vec<CheckResult>> {
        let defaults = self.model.board.text_graphics;
        match cmd {
            MicrowaveCmd::Gap { at, size_nm, track_width_nm, hand } => {
                let built = build_gap(&defaults, *size_nm, *track_width_nm)?;
                let at = self.snap_point(at.x, at.y);
                self.put_microwave(built, at, *hand)
            }
            MicrowaveCmd::Stub { at, size_nm, track_width_nm, hand } => {
                let built = build_stub(&defaults, *size_nm, *track_width_nm)?;
                let at = self.snap_point(at.x, at.y);
                self.put_microwave(built, at, *hand)
            }
            MicrowaveCmd::ArcStub { at, radius_nm, angle_millideg, track_width_nm, hand } => {
                let built = build_arc_stub(&defaults, *radius_nm, *angle_millideg, *track_width_nm)?;
                let at = self.snap_point(at.x, at.y);
                self.put_microwave(built, at, *hand)
            }
            MicrowaveCmd::FunctionShape { at, file, kind, size_x_nm, size_y_nm, track_width_nm, hand } => {
                let built = build_function_shape(&defaults, file, *kind, *size_x_nm, *size_y_nm, *track_width_nm)?;
                let at = self.snap_point(at.x, at.y);
                self.put_microwave(built, at, *hand)
            }
            MicrowaveCmd::Inductor { start, end, length_nm, name, track_width_nm } => {
                let built = build_inductor(&defaults, *start, *end, *length_nm, name, *track_width_nm)?;
                // The footprint is anchored at the end point, exactly where the click was.
                self.put_microwave(built, *end, Hand::default())
            }
        }
    }

    /// A footprint name no other footprint of the board takes: `base`, or `base_1`, `base_2` .. (`CreateNewFootprint`'s way of making a name unique). The model finds a footprint by its name, so
    /// two gaps of different sizes cannot both be `muwave_gap`; two that are alike share it.
    fn microwave_name(&self, base: &str, def: &LibraryFootprint) -> String {
        let parts: Vec<&BoardPart> = self.design.drawings.as_ref().map(|d| d.board_parts.iter().collect()).unwrap_or_default();
        for n in 0.. {
            let name = if n == 0 { base.to_string() } else { format!("{base}_{n}") };
            let users: Vec<&&BoardPart> = parts.iter().filter(|p| p.footprint == name).collect();
            let usable = if users.is_empty() { self.model.footprints.iter().all(|f| f.name != name) } else { users.iter().all(|p| p.definition.as_ref().is_some_and(|d| same_shape(d, def))) };
            if usable {
                return name;
            }
        }
        unreachable!("an unbounded range finds a free name")
    }

    /// The part, its pose and what makes it the footprint KiCad's tool makes.
    fn put_microwave(&mut self, built: Built, at: Point, hand: Hand) -> Result<(), Vec<CheckResult>> {
        let Built { mut footprint, reference: reference_layout, value: value_layout, value_text } = built;
        footprint.assign_missing_ids();
        let name = self.microwave_name(&footprint.name, &footprint);
        footprint.name = name.clone();
        let taken = self.all_references();
        let prefix = super::library_place::reference_prefix_for(&name);
        let reference = (1..).map(|n| format!("{prefix}{n}")).find(|r| !taken.contains(r)).expect("an unbounded range finds a free number");

        let pad_count = footprint.pads.len();
        let dr = self.drawings_mut();
        dr.board_parts.push(BoardPart { reference: reference.clone(), value: Some(value_text), footprint: name.clone(), definition: Some(footprint), pad_nets: Vec::new() });
        dr.board_parts.sort_by(|a, b| a.reference.cmp(&b.reference));
        // The fields as `CreateNewFootprint` laid them out, and the attributes `createBaseFootprint` set: neither SMD nor through hole, left out of the position files and the BOM.
        dr.footprint_edits.retain(|e| e.id != reference);
        dr.footprint_edits.push(FootprintEdit {
            id: reference.clone(),
            reference: Some(reference_layout),
            value: Some(value_layout),
            attrs: Some(FootprintAttrs { kind: FootprintKind::Unspecified, board_only: false, exclude_from_pos_files: true, exclude_from_bom: true, dnp: false, allow_missing_courtyard: false }),
            ..FootprintEdit::new(&reference)
        });
        dr.footprint_edits.sort_by(|a, b| a.id.cmp(&b.id));
        // What a footprint read from a board file with these pads would have: copper on the front layer only (no paste, no mask opening), and no courtyard -- the tools make none.
        dr.footprint_extras.retain(|e| e.id != reference);
        dr.footprint_extras.push(FootprintExtra {
            id: reference.clone(),
            no_courtyard: true,
            // `LIB_ID( wxEmptyString, name )`: no library, so the file says `(footprint "muwave_gap" ..)` and not `"eda:muwave_gap"`.
            unlinked_name: Some(name.clone()),
            pads: (0..pad_count).map(|_| PadMaskInfo { layers: vec!["F.Cu".to_string()], no_paste: true, no_mask: true, ..Default::default() }).collect(),
            ..Default::default()
        });
        dr.footprint_extras.sort_by(|a, b| a.id.cmp(&b.id));

        let fps = &mut self.design.placement.as_mut().expect("a Board always carries a placement section").footprints;
        fps.push(FootprintInstance { id: reference.clone(), at, rot: 0, side: Side::Top, label: LabelSide::default() });
        self.sort_footprints();

        // R and F pressed while the footprint was in the hand (`newItem->Rotate( newItem->GetPosition(), angle )`, `newItem->Flip( newItem->GetPosition(), direction )`).
        let ids = vec![reference];
        if hand.rotate_millideg != 0 {
            self.rotate_items(&ids, at, hand.rotate_millideg)?;
        }
        if let Some(direction) = hand.flip {
            self.flip_items(&ids, at, direction)?;
        }
        Ok(())
    }
}
