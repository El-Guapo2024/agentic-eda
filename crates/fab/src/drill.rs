//! Excellon drill writer.
//!
//! A port of KiCad's `EXCELLON_WRITER` (`pcbnew/exporters/
//! gendrill_excellon_writer.cpp`): the `M48` header, a tool table grouped
//! plated-first (PTH, including vias) then non-plated (NPTH), each group
//! sorted by diameter ascending, then one coordinate block per tool
//! (round holes as a plain flash, oblong holes as a `G85` canned slot),
//! `M30` to close. Checked byte-for-byte against a real `kicad-cli pcb
//! export drill` run on the same board (tool table, `G90`/`G05` mode
//! lines, coordinate formatting, the `G85` slot pairing).
//!
//! Units are always millimetres and the zeros format is always KiCad's own
//! default ("decimal": an explicit decimal point, trailing zeros trimmed
//! but at least one kept) -- the other `kicad-cli` options
//! (`suppressleading`/`suppresstrailing`/`keep`, inches) are not ported;
//! nothing in this workspace's pipeline asks for them.

use crate::gerber::{placed_pads_full, PlacedPadFull};
use eda_model::ir::{Design, Point, Um};
use eda_model::{CheckResult, ConstraintModel, PadKind, Part};
use std::collections::BTreeMap;
use std::fmt::Write as _;

pub struct DrillMeta {
    pub title: String,
    pub date: String,
    pub generator_version: String,
}

#[derive(Clone, Copy, Default)]
pub struct DrillOptions {
    /// `--excellon-separate-th`: independent PTH/NPTH files. Default
    /// `false` matches `kicad-cli`'s own default (and this is what the
    /// comparator test runs against).
    pub separate_th: bool,
}

pub struct DrillFile {
    pub filename: String,
    pub content: String,
}

#[derive(Clone, Copy, PartialEq)]
enum HoleShape {
    Round(Um),
    /// `(start, end)` of the canned-slot path, tool diameter is the slot's
    /// own narrower dimension.
    Slot(Point, Point, Um),
}

#[derive(Clone, Copy)]
struct Hole {
    plated: bool,
    via: bool,
    shape: HoleShape,
    /// Sort / first-flash position: a round hole's own centre, a slot's
    /// start point -- matches `EXCELLON_WRITER`'s own position-ordered
    /// hole list.
    at: Point,
}

impl Hole {
    fn diameter(&self) -> Um {
        match self.shape {
            HoleShape::Round(d) => d,
            HoleShape::Slot(_, _, d) => d,
        }
    }
}

/// Every drillable hole on the board: one per through-hole/non-plated pad
/// (round or oblong) and one per via. A pad's hole position/orientation is
/// this module's own minimal re-derivation (see `gerber::PlacedPadFull`'s
/// own doc comment for why it isn't shared with `eda_model::footprint::
/// PlacedPad`).
fn collect_holes(design: &Design, model: &ConstraintModel) -> Result<Vec<Hole>, Vec<CheckResult>> {
    let mut holes = Vec::new();
    if let Some(pl) = &design.placement {
        let parts_by_ref: BTreeMap<&str, &Part> = model.parts.iter().map(|p| (p.reference.as_str(), p)).collect();
        let mut footprints: Vec<_> = pl.footprints.iter().collect();
        footprints.sort_by(|a, b| a.id.cmp(&b.id));
        for fp in &footprints {
            let Some(part) = parts_by_ref.get(fp.id.as_str()) else {
                return Err(vec![CheckResult::fail("fab.drill", fp.id.clone(), "footprint has no matching part")]);
            };
            let Some(pads) = placed_pads_full(model, part, fp) else {
                return Err(vec![CheckResult::fail("fab.drill", fp.id.clone(), "part has no resolvable footprint geometry")]);
            };
            for pad in &pads {
                if pad.kind == PadKind::Smd {
                    continue;
                }
                let plated = pad.kind == PadKind::ThroughHole;
                if let Some(d) = pad.drill {
                    holes.push(Hole { plated, via: false, shape: HoleShape::Round(d), at: pad.center });
                } else if let Some((sw, sh)) = pad.drill_slot {
                    holes.push(slot_hole(plated, pad, sw, sh));
                }
            }
        }
    }
    if let Some(routing) = &design.routing {
        for v in &routing.vias {
            holes.push(Hole { plated: true, via: true, shape: HoleShape::Round(v.drill), at: v.at });
        }
    }
    Ok(holes)
}

fn slot_hole(plated: bool, pad: &PlacedPadFull, sw: Um, sh: Um) -> Hole {
    let (tool, half_path, horizontal) = if sw >= sh { (sh, (sw - sh) / 2, true) } else { (sw, (sh - sw) / 2, false) };
    let (start, end) = if horizontal {
        (Point { x: pad.center.x - half_path, y: pad.center.y }, Point { x: pad.center.x + half_path, y: pad.center.y })
    } else {
        (Point { x: pad.center.x, y: pad.center.y - half_path }, Point { x: pad.center.x, y: pad.center.y + half_path })
    };
    Hole { plated, via: false, shape: HoleShape::Slot(start, end, tool.max(1)), at: start }
}

/// Coordinate: millimetres, "decimal" zeros format -- an explicit point,
/// trailing zeros trimmed but at least one kept (`X8.0`, not `X8` or
/// `X8.000000`). Every value here is an exact multiple of 1 µm = 0.001 mm,
/// so trimming a 6-decimal format never loses precision.
fn fmt_xy(um: Um) -> String {
    let mut s = format!("{:.6}", um as f64 / 1000.0);
    while s.ends_with('0') {
        s.pop();
    }
    if s.ends_with('.') {
        s.push('0');
    }
    if s == "-0.0" {
        s = "0.0".into();
    }
    s
}

fn fmt_tool_dia(um: Um) -> String {
    format!("{:.3}", um as f64 / 1000.0)
}

fn write_section(out: &mut String, meta: &DrillMeta, file_function: &str, holes: &[Hole]) {
    writeln!(out, "M48").unwrap();
    writeln!(out, "; DRILL file eda-fab {} date {}", meta.generator_version, meta.date).unwrap();
    out.push_str("; FORMAT={-:-/ absolute / metric / decimal}\n");
    writeln!(out, "; #@! TF.CreationDate,{}", meta.date).unwrap();
    writeln!(out, "; #@! TF.GenerationSoftware,EdaFab,eda-fab,{}", meta.generator_version).unwrap();
    writeln!(out, "; #@! TF.FileFunction,{file_function}").unwrap();
    out.push_str("FMAT,2\n");
    out.push_str("METRIC\n");

    // Tool table: plated (vias first by convention, then component holes)
    // before non-plated, each group sorted by diameter ascending -- see
    // the module doc.
    let mut diam_order: Vec<(bool, Um)> = holes.iter().map(|h| (h.plated, h.diameter())).collect();
    diam_order.sort();
    diam_order.dedup();
    diam_order.sort_by(|a, b| (!a.0, a.1).cmp(&(!b.0, b.1)));
    let mut tool_of: BTreeMap<(bool, Um), u32> = BTreeMap::new();
    for (i, key) in diam_order.iter().enumerate() {
        tool_of.insert(*key, i as u32 + 1);
        let is_via = holes.iter().any(|h| h.plated == key.0 && h.diameter() == key.1 && h.via);
        let func = match (key.0, is_via) {
            (true, true) => "Plated,PTH,ViaDrill",
            (true, false) => "Plated,PTH,ComponentDrill",
            (false, _) => "NonPlated,NPTH,ComponentDrill",
        };
        writeln!(out, "; #@! TA.AperFunction,{func}").unwrap();
        writeln!(out, "T{}C{}", i + 1, fmt_tool_dia(key.1)).unwrap();
    }
    out.push_str("%\n");
    out.push_str("G90\n");
    out.push_str("G05\n");

    for key in &diam_order {
        let tool = tool_of[key];
        let mut group: Vec<&Hole> = holes.iter().filter(|h| h.plated == key.0 && h.diameter() == key.1).collect();
        group.sort_by(|a, b| (a.at.x, -a.at.y).cmp(&(b.at.x, -b.at.y)));
        writeln!(out, "T{tool}").unwrap();
        for h in group {
            match h.shape {
                HoleShape::Round(_) => {
                    writeln!(out, "X{}Y{}", fmt_xy(h.at.x), fmt_xy(-h.at.y)).unwrap();
                }
                HoleShape::Slot(start, end, _) => {
                    writeln!(out, "X{}Y{}G85X{}Y{}", fmt_xy(start.x), fmt_xy(-start.y), fmt_xy(end.x), fmt_xy(-end.y)).unwrap();
                    out.push_str("G05\n");
                }
            }
        }
    }
    out.push_str("M30\n");
}

/// Write the board's drill file(s): one `{title}.drl` mixing PTH and NPTH
/// (KiCad's/`kicad-cli`'s own default), or, with `opts.separate_th`, a
/// `{title}-PTH.drl` / `{title}-NPTH.drl` pair.
pub fn write_drill(design: &Design, model: &ConstraintModel, meta: &DrillMeta, opts: DrillOptions) -> Result<Vec<DrillFile>, Vec<CheckResult>> {
    let holes = collect_holes(design, model)?;
    if !opts.separate_th {
        let mut content = String::new();
        write_section(&mut content, meta, "MixedPlating,1,2", &holes);
        return Ok(vec![DrillFile { filename: format!("{}.drl", meta.title), content }]);
    }
    let pth: Vec<Hole> = holes.iter().copied().filter(|h| h.plated).collect();
    let npth: Vec<Hole> = holes.iter().copied().filter(|h| !h.plated).collect();
    let mut out = Vec::new();
    if !pth.is_empty() {
        let mut content = String::new();
        write_section(&mut content, meta, "Plated,1,2", &pth);
        out.push(DrillFile { filename: format!("{}-PTH.drl", meta.title), content });
    }
    if !npth.is_empty() {
        let mut content = String::new();
        write_section(&mut content, meta, "NonPlated,1,2", &npth);
        out.push(DrillFile { filename: format!("{}-NPTH.drl", meta.title), content });
    }
    Ok(out)
}

/// A cheap, text-only drill summary -- not KiCad's own rendered drill
/// *map* (a PDF/Gerber/SVG legend of one symbol per tool, which needs a
/// symbol-per-tool renderer this writer does not have), but the same
/// counts a fabricator's own drill report states: one line per tool
/// giving its diameter, hole count, and plated/non-plated status.
pub fn drill_report(design: &Design, model: &ConstraintModel) -> Result<String, Vec<CheckResult>> {
    let holes = collect_holes(design, model)?;
    let mut by_tool: BTreeMap<(bool, Um), (u32, bool)> = BTreeMap::new();
    for h in &holes {
        let e = by_tool.entry((h.plated, h.diameter())).or_insert((0, h.via));
        e.0 += 1;
    }
    let mut out = String::from("Tool  Diameter(mm)  Plated  Count\n");
    for (i, ((plated, dia), (count, _))) in by_tool.iter().enumerate() {
        writeln!(out, "T{:<4} {:<13} {:<7} {}", i + 1, fmt_tool_dia(*dia), if *plated { "PTH" } else { "NPTH" }, count).unwrap();
    }
    Ok(out)
}
