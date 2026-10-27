//! INACTIVE (2026-10-28): replaced by `kicad-cli api-server`'s `ImportSpecctraSession` (`eda_kicad_engine::api::specctra`); kept for a future all-Rust build
//! and as `Cmd::Specctra`'s fallback where kicad-cli has no `api-server` or `EDA_KICAD_API=0` (see `super`'s doc). [`SessionImport`] is the answer of both.
//!
//! `SPECCTRA_DB::FromSESSION` (`pcbnew/specctra_import_export/specctra_import.cpp`): what a Specctra session -- the file an outside autorouter such as FreeRouting
//! writes back -- puts on the board.
//!
//! KiCad's `FromSESSION` works on the open `BOARD`: it deletes every track, arc and via that is not locked (a locked one was exported as `(type fix)` and does not come
//! back in the session), moves and turns the footprints the session's `(placement ..)` names, and makes a `PCB_TRACK` of every segment of every `(wire (path ..))`, a
//! `PCB_ARC` of every `(wire (qarc ..))` and a `PCB_VIA` of every vertex of every `(via ..)` of the `(network_out ..)`, on the net the `(net ..)` names.
//!
//! Here the board is the design, which this module does not change: [`from_session`] reads the session against the board as KiCad would read it (the `.kicad_pcb` the
//! studio derives from the design, [`super::board::BoardView`], whose copper layers are `buildLayerMaps`' and whose nets are `FindNet`'s) and answers what the import
//! puts on the board, in the design's units ([`SessionImport`]); `eda_ops`' `Cmd::Specctra` takes the old routing away and puts this in its place, as one undo step.
//! The errors are the `IO_ERROR`s `FromSESSION`, `makeTRACK`, `makeARC` and `makeVIA` throw, in KiCad's words.

use eda_model::units::Nm;
use super::board::BoardView;
use super::geom::{rotate_point_about, Angle, V};
use super::model::*;
use eda_model::ir::{Design, Millideg, Point, Side as IrSide, Track, Via};
use eda_model::via_extra::ViaKind;
use eda_model::ConstraintModel;

/// A footprint the session's `(placement ..)` moves: where `FootprintInstance` puts it.
#[derive(Debug, Clone, PartialEq)]
pub struct SessionPlace {
    /// `PLACE::m_component_id`: the reference, which is the design's footprint id.
    pub reference: String,
    pub at: Point,
    pub rot: Millideg,
    pub side: IrSide,
}

/// A via of the session: the design's via, its `VIATYPE` and whether it is locked (`(type fix)`).
#[derive(Debug, Clone, PartialEq)]
pub struct SessionVia {
    pub via: Via,
    pub kind: ViaKind,
    pub locked: bool,
}

/// What `FromSESSION` puts on the board, in nanometres; the ids are left for the design to give.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SessionImport {
    pub placements: Vec<SessionPlace>,
    /// One track per `PCB_TRACK` (a segment of a path) or `PCB_ARC`, with whether it is locked.
    pub tracks: Vec<(Track, bool)>,
    pub vias: Vec<SessionVia>,
}

/// `scale( double distance, UNIT_RES* aResolution )`: a session distance in nanometres (`KiROUND( factor * distance / resValue )`).
fn scale(distance: f64, res: &UnitRes) -> i64 {
    let factor = match res.units {
        Unit::Inch => 25.4e6,
        Unit::Mil => 25.4e3,
        Unit::Cm => 1e7,
        Unit::Mm => 1e6,
        Unit::Um => 1e3,
    };
    super::geom::kiround(factor * distance / res.value as f64)
}

/// `mapPt`: the y axis of the session points up, the board's down.
fn map_pt(p: &Pt, res: &UnitRes) -> V {
    V::new(scale(p.x, res), -scale(p.y, res))
}

/// A point of the session, mapped (nanometres), as the design holds it.
fn pt(v: V) -> Point {
    Point { x: Nm::new(v.x), y: Nm::new(v.y) }
}

/// `CalcArcMid( aStart, aEnd, aCenter, aMinArcAngle = true )`: the middle of the shorter arc from `start` to `end` about `center`.
fn calc_arc_mid(start: V, end: V, center: V) -> V {
    let angle_of = |v: V| Angle((v.y as f64).atan2(v.x as f64).to_degrees());
    let start_angle = angle_of(start - center);
    let end_angle = angle_of(end - center);
    let mid_rot = Angle((start_angle - end_angle).normalize180().0 / 2.0);
    rotate_point_about(start, center, mid_rot)
}

/// The import's state: `m_layerIds`, `m_pcbLayer2kicad` and the board it reads (`m_sessionBoard`).
struct Db<'a> {
    board: &'a BoardView,
    layer_ids: Vec<String>,
    kicad_layers: Vec<String>,
}

impl Db<'_> {
    /// `SPECCTRA_DB::buildLayerMaps`: the copper layers top first, by the names the board gives them.
    fn new(board: &BoardView) -> Db<'_> {
        Db { board, layer_ids: board.copper.iter().map(|l| l.shown.clone()).collect(), kicad_layers: board.copper.iter().map(|l| l.name.clone()).collect() }
    }

    /// `SPECCTRA_DB::findLayerName`.
    fn find_layer_name(&self, name: &str) -> Option<usize> {
        self.layer_ids.iter().position(|l| l == name)
    }

    /// `SPECCTRA_DB::makeTRACK`: the segment from `points[i]` to `points[i + 1]` of a wire's path.
    fn make_track(&self, wire: &Wire, path: &PathElem, i: usize, net: &str, res: &UnitRes) -> Result<(Track, bool), String> {
        let ndx = self.find_layer_name(&path.layer_id).ok_or_else(|| format!("Session file uses invalid layer id '{}'.", path.layer_id))?;
        let start = pt(map_pt(&path.points[i], res));
        let end = pt(map_pt(&path.points[i + 1], res));
        let width = scale(path.aperture_width, res);
        let track = Track { id: String::new(), net: net.to_string(), pins: vec![], layer: self.kicad_layers[ndx].clone(), width: Nm::new(width), pts: vec![start, end], arc_mid_offset: None };
        // Specctra has 4 types, none exactly KiCad's locked option: `fix` is taken as locked.
        Ok((track, wire.wire_type == Some(WireType::Fix)))
    }

    /// `SPECCTRA_DB::makeARC`: a quarter arc, its third vertex the centre.
    fn make_arc(&self, wire: &Wire, qarc: &Qarc, net: &str, res: &UnitRes) -> Result<(Track, bool), String> {
        let ndx = self.find_layer_name(&qarc.layer_id).ok_or_else(|| format!("Session file uses invalid layer id '{}'.", qarc.layer_id))?;
        let start = map_pt(&qarc.vertex[0], res);
        let end = map_pt(&qarc.vertex[1], res);
        let mid = calc_arc_mid(start, end, map_pt(&qarc.vertex[2], res));
        let width = scale(qarc.aperture_width, res);
        let track = Track::new_arc(net.to_string(), self.kicad_layers[ndx].clone(), Nm::new(width), pt(start), pt(mid), pt(end));
        Ok((track, wire.wire_type == Some(WireType::Fix)))
    }

    /// `SPECCTRA_DB::makeVIA`. `via_drill_default` is the Default net class's via drill (nanometres); `drill_of_net` is `PCB_VIA::GetDrillValue` of a via whose drill is
    /// `UNDEFINED_DRILL_DIAMETER` (the via's net class's), nanometres.
    fn make_via(&self, wire_via: &WireVia, padstack: &Padstack, point: &Pt, net: &str, via_drill_default: i64, drill_of_net: i64, res: &UnitRes) -> Result<SessionVia, String> {
        let shape_count = padstack.shapes.len();
        let copper_layer_count = self.board.copper_count();
        // The drill diameter is encoded in the padstack name if Pcbnew did the DSN export: after the colon and before the last '_'.
        let mut drill_diam_iu: i64 = -1;
        if let Some(colon) = padstack.padstack_id.find(':') {
            let start = colon + 1;
            if let Some(end) = padstack.padstack_id.rfind('_') {
                let diam_txt = padstack.padstack_id.get(start..end).unwrap_or("");
                // `fast_float::from_chars` with `skip_white_space`: the longest number at the start, 0 when there is none.
                let t = diam_txt.trim_start();
                let n = (1..=t.len()).rev().find(|&k| t.is_char_boundary(k) && t[..k].parse::<f64>().is_ok()).unwrap_or(0);
                let drill_nm: f64 = if n == 0 { 0.0 } else { t[..n].parse().unwrap_or(0.0) };
                drill_diam_iu = (drill_nm * (1_000_000.0 / 1000.0)) as i64;
                if drill_diam_iu == via_drill_default {
                    drill_diam_iu = -1; // UNDEFINED_DRILL_DIAMETER
                }
            }
        }
        let circle_of = |s: &ShapeEl| -> Result<Circle, String> {
            match &s.shape {
                Some(ShapeElem::Circle(c)) => Ok(c.clone()),
                other => Err(format!("Unsupported via shape: {}", other.as_ref().map(shape_token).unwrap_or_else(|| "'NONE'".to_string()))),
            }
        };
        let pos = pt(map_pt(point, res));
        let (kind, diameter, from_layer, to_layer) = if shape_count == 0 {
            return Err("Session via padstack has no shapes".to_string());
        } else if shape_count == 1 || shape_count == copper_layer_count {
            let circle = match &padstack.shapes[0].shape {
                Some(ShapeElem::Circle(c)) => c.clone(),
                // `makeVIA` words the one-shape case with a full stop, the all-layers case without.
                other => return Err(format!("Unsupported via shape: {}{}", other.as_ref().map(shape_token).unwrap_or_else(|| "'NONE'".to_string()), if shape_count == 1 { "." } else { "" })),
            };
            (ViaKind::Through, scale(circle.diameter, res), "F.Cu".to_string(), "B.Cu".to_string())
        } else {
            // VIA_MICROVIA or VIA_BLIND_BURIED
            let mut top_layer_ndx: i64 = -1;
            let mut bot_layer_ndx: i64 = i64::MAX;
            let mut via_diam: i64 = -1;
            for s in &padstack.shapes {
                let circle = circle_of(s)?;
                let ndx = self.find_layer_name(&circle.layer_id).ok_or_else(|| format!("Session file uses invalid layer id '{}'", circle.layer_id))? as i64;
                top_layer_ndx = top_layer_ndx.max(ndx);
                bot_layer_ndx = bot_layer_ndx.min(ndx);
                if via_diam == -1 {
                    via_diam = scale(circle.diameter, res);
                }
            }
            let n = copper_layer_count as i64;
            let kind = if (top_layer_ndx == 0 && bot_layer_ndx == 1) || (top_layer_ndx == n - 2 && bot_layer_ndx == n - 1) {
                ViaKind::Micro
            } else if top_layer_ndx > 0 && bot_layer_ndx < n - 1 {
                ViaKind::Buried
            } else {
                ViaKind::Blind
            };
            let top = top_layer_ndx.max(0) as usize;
            let bot = bot_layer_ndx as usize;
            // `SetLayerPair` with the outer layer of the stack first.
            let (a, b) = (top.min(bot), top.max(bot));
            (kind, via_diam, self.kicad_layers[a].clone(), self.kicad_layers[b].clone())
        };
        // `PCB_VIA::GetDrillValue` of an undefined drill is the class's via drill, or its microvia drill for a MICROVIA; but the type tests above never make one
        // of a padstack of two or more shapes (`topLayerNdx` is the largest index, `botLayerNdx` the smallest, so neither `topLayerNdx == 0 && botLayerNdx == 1`
        // nor `topLayerNdx == count - 2 && botLayerNdx == count - 1` can hold): an F.Cu-In1.Cu via comes back BURIED, with the via drill, in KiCad as here.
        let drill = if drill_diam_iu > 0 { drill_diam_iu } else { drill_of_net };
        let via = Via { id: String::new(), net: net.to_string(), at: pos, drill: Nm::new(drill), diameter: Nm::new(diameter), from_layer, to_layer };
        Ok(SessionVia { via, kind, locked: wire_via.via_type == Some(WireType::Fix) })
    }
}

/// `DSNLEXER::GetTokenString( shape->Type() )`: the keyword in single quotes.
fn shape_token(s: &ShapeElem) -> String {
    let t = match s {
        ShapeElem::Rect(_) => "rect",
        ShapeElem::Circle(_) => "circle",
        ShapeElem::Qarc(_) => "qarc",
        ShapeElem::Path(p) => p.name(),
    };
    format!("'{t}'")
}

/// `SPECCTRA_DB::FromSESSION` on a session already read (`LoadSESSION`), against `board`, the board as KiCad reads it. `via_drill_default` is the Default class's via
/// drill in nanometres; `drill_of_net` answers `PCB_VIA::GetDrillValue` of a via of a net whose drill the session left undefined, nanometres.
pub fn from_session_db(session: &Session, board: &BoardView, via_drill_default: i64, drill_of_net: &dyn Fn(&str) -> i64) -> Result<SessionImport, String> {
    let route = session.route.as_ref().ok_or("Session file is missing the \"routes\" section")?;
    let library = route.library.as_ref().ok_or("Session file is missing the \"library_out\" section")?;
    let db = Db::new(board);
    let mut out = SessionImport::default();

    if let Some(placement) = &session.placement {
        // `PLACE::GetUnits`: the placement's own unit, else `UNIT_RES::Default` (the session has none).
        let res = placement.unit.unwrap_or(UnitRes::DEFAULT);
        for component in &placement.components {
            for place in &component.places {
                if !board.footprints.iter().any(|f| f.reference == place.component_id) {
                    return Err(format!("Reference '{}' not found.", place.component_id));
                }
                let Some(vertex) = &place.vertex else { continue };
                let at = pt(map_pt(vertex, &res));
                // KiCad's orientation: the rotation on the front; on the back the rotation plus 180 (the export wrote 180 minus the orientation of the flipped footprint).
                let (orientation, side) = match place.side {
                    Side::Front => (place.rotation, IrSide::Top),
                    Side::Back => (place.rotation + 180.0, IrSide::Bottom),
                };
                out.placements.push(SessionPlace { reference: place.component_id.clone(), at, rot: crate::import::import_rot_millideg(orientation), side });
            }
        }
    }

    let res = route.units();
    // Walk the NET_OUTs and create tracks and vias anew.
    for net_out in &route.net_outs {
        // Page 143 of the spec says a wire's net_id is optional; a net the board does not have is no net.
        let net = if !net_out.net_id.is_empty() && board.nets.iter().any(|(c, n)| *c > 0 && n == &net_out.net_id) { net_out.net_id.clone() } else { String::new() };
        for wire in &net_out.wires {
            match &wire.shape {
                Some(ShapeElem::Path(path)) if !path.polygon => {
                    for i in 0..path.points.len().saturating_sub(1) {
                        out.tracks.push(db.make_track(wire, path, i, &net, &res)?);
                    }
                }
                Some(ShapeElem::Qarc(qarc)) => out.tracks.push(db.make_arc(wire, qarc, &net, &res)?),
                // A polygon is what FreeRouting returns for a zone on a signal layer: KiCad kept its own zones, so this so-called wire is ignored.
                _ => {}
            }
        }
        for wire_via in &net_out.wire_vias {
            let padstack = library.find_padstack(&wire_via.padstack_id).ok_or_else(|| format!("A wire_via refers to missing padstack '{}'.", wire_via.padstack_id))?;
            let drill = drill_of_net(&net);
            for v in &wire_via.vertexes {
                out.vias.push(db.make_via(wire_via, padstack, v, &net, via_drill_default, drill, &res)?);
            }
        }
    }
    Ok(out)
}

/// `DSN::ImportSpecctraSession` for the design: `text` is the session file (`source` its name, for the parser's messages), read by `LoadSESSION` and put against the
/// board the studio derives from `design` by `FromSESSION`. An error is the `IO_ERROR` KiCad's dialog shows under "Board may be corrupted, do not save it. Fix problem and
/// try again" -- here the board is untouched by a refused import.
pub fn from_session(text: &str, source: &str, design: &Design, model: &ConstraintModel) -> Result<SessionImport, String> {
    let session = super::parse::load_session(text, source).map_err(|e| e.what())?;
    let meta = crate::ExportMeta { date: "", title: "" };
    let pcb_text = crate::export_kicad_pcb(design, model, &meta).map_err(|e| e.iter().map(|c| c.hint.clone().unwrap_or_else(|| c.check.clone())).collect::<Vec<_>>().join("; "))?;
    let board = BoardView::parse(&pcb_text)?;
    let b = &model.board;
    let mut out = from_session_db(&session, &board, b.via_drill.get(), &|net: &str| b.via_drill_of(net).get())?;
    to_design_frame(&mut out.placements, design);
    Ok(out)
}

/// A footprint's orientation in a board file (degrees, KiCad's: counter-clockwise, y down) as the design's rotation (`import::import_rot_millideg`).
pub fn rot_of_file_orientation(degrees: f64) -> Millideg {
    crate::import::import_rot_millideg(degrees)
}

/// The placements of an import, whose rotations are the board file's, in the design's frame. A footprint on the back made here is in the board file turned
/// half a turn in its own frame and back in its orientation (`pcb::write_footprint`, KiCad's frame): the orientation the file gives is half a turn from the
/// design's. (A footprint read in the file's frame, by a reader of before `FootprintExtra::library_frame`, is written as it was read.) Also used by the
/// api-server import (`eda_kicad_engine::api::specctra`), whose placements are read off KiCad's own board.
pub fn to_design_frame(placements: &mut [SessionPlace], design: &Design) {
    let read_from_a_file: std::collections::BTreeSet<&str> = design.drawings.iter().flat_map(|d| d.footprint_extras.iter().filter(|e| !e.library_frame).map(|e| e.id.as_str())).collect();
    for p in placements.iter_mut().filter(|p| p.side == IrSide::Bottom && !read_from_a_file.contains(p.reference.as_str())) {
        p.rot = (p.rot + 180_000) % 360_000;
    }
}

/// The session a router that changes nothing would write back for `pcb`, a design file: its placement, its via padstacks as the `library_out` and its wiring as the
/// `network_out`, in a `(resolution um 10)` -- the shape FreeRouting's `SessionFile` gives a session. It is no part of KiCad: the tests make their round trips with it, as no
/// router runs here.
#[doc(hidden)]
pub fn echo_session(pcb: &Pcb, session_name: &str) -> String {
    const RES: f64 = 10.0; // the design is in micrometres (`(unit um)`), the session in tenths of them
    let s = |p: &Pt| Pt::new(p.x * RES, p.y * RES);
    let mut out = super::format::Out::default();
    out.print(0, &format!("(session {}\n", out.q(session_name)));
    out.print(1, &format!("(base_design {})\n", out.q(&pcb.pcbname)));
    if let Some(pl) = &pcb.placement {
        let mut pl = pl.clone();
        pl.unit = Some(UnitRes::resolution(Unit::Um, RES as i32));
        pl.flip_style = None;
        for c in &mut pl.components {
            for p in &mut c.places {
                p.vertex = p.vertex.as_ref().map(s);
                p.part_number.clear();
            }
        }
        pl.format(&mut out, 1);
    }
    out.print(1, "(routes\n");
    UnitRes::resolution(Unit::Um, RES as i32).format(&mut out, 2);
    out.print(2, "(library_out\n");
    // The padstacks of the vias: a design file read back holds them among the others (`doLIBRARY` keeps one list), one just made in `vias`.
    let used: Vec<&str> = pcb.wiring.as_ref().map(|w| w.wire_vias.iter().map(|v| v.padstack_id.as_str()).collect()).unwrap_or_default();
    let library = pcb.library.as_ref();
    let mut via_padstacks: Vec<&Padstack> = library.map(|l| l.vias.iter().chain(l.padstacks.iter()).filter(|p| used.contains(&p.padstack_id.as_str())).collect()).unwrap_or_default();
    via_padstacks.dedup_by(|a, b| a.padstack_id == b.padstack_id);
    for v in via_padstacks {
        let mut v = v.clone();
        for sh in &mut v.shapes {
            if let Some(ShapeElem::Circle(c)) = &mut sh.shape {
                c.diameter *= RES;
                c.vertex = s(&c.vertex);
            }
        }
        v.format(&mut out, 3);
    }
    out.print(2, ")\n");
    out.print(2, "(network_out\n");
    let wiring = pcb.wiring.clone().unwrap_or_default();
    let mut nets: Vec<String> = wiring.wires.iter().map(|w| w.net_id.clone()).chain(wiring.wire_vias.iter().map(|v| v.net_id.clone())).collect();
    nets.sort();
    nets.dedup();
    for net in nets {
        out.print(3, &format!("(net {}\n", out.q(&net)));
        for w in wiring.wires.iter().filter(|w| w.net_id == net && w.wire_type != Some(WireType::Fix)) {
            let mut w = w.clone();
            w.net_id.clear();
            w.wire_type = None;
            if let Some(ShapeElem::Path(p)) = &mut w.shape {
                p.aperture_width *= RES;
                p.points = p.points.iter().map(s).collect();
            }
            w.format(&mut out, 4);
        }
        for v in wiring.wire_vias.iter().filter(|v| v.net_id == net && v.via_type != Some(WireType::Fix)) {
            let mut v = v.clone();
            v.net_id.clear();
            v.via_type = None;
            v.vertexes = v.vertexes.iter().map(s).collect();
            v.format(&mut out, 4);
        }
        out.print(3, ")\n");
    }
    out.print(2, ")\n");
    out.print(1, ")\n");
    out.print(0, ")\n");
    out.into_string()
}
