//! KiCad's PCB clipboard: the text a Copy puts on the system clipboard and a Paste reads back, so a selection travels
//! between this studio and a running KiCad in either direction.
//!
//! Ports `CLIPBOARD_IO` (`pcbnew/kicad_clipboard.cpp` at KiCad 8303b2ad):
//!
//! * `SaveSelection`: one footprint alone is written as a bare `(footprint ..)` with its pad nets taken off, unlocked and
//!   moved so the copy's reference point is the origin; every other selection is written as a `(kicad_pcb ..)` that holds
//!   only the layers and the selected items -- tracks, vias, zones, graphics, text, dimensions, footprints and the groups
//!   among them -- each moved by minus the reference point and unlocked ("locked means locked in place; copied items
//!   therefore can't be locked").
//! * `Parse`: the text is read as a whole board file, or as a single footprint. Nothing else on the clipboard is KiCad's.
//!
//! The item syntax is the one [`crate::pcb`] writes and [`crate::import`] reads (kicad-cli judges that file), so a
//! clipboard from here pastes into KiCad 9 and 10, and a clipboard from KiCad 9 or 10 parses here. One deliberate
//! difference from KiCad 10's own writer: items name their net by code with the net table in the text
//! (`(net 3)` and `(net 3 "GND")`), the form every KiCad since 6 reads, rather than by name (`(net "GND")`, which only
//! KiCad 10 reads). The reader takes both.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;

use eda_model::ir::{Design, Dimension, FootprintInstance, FootprintZoneOverrides, Group, LabelSide, Millideg, Point, Shape, Side, Text, Track, Via, Zone};
use eda_model::{CheckResult, ConstraintModel, Footprint};

use crate::fp_fields::FpExtra;
use crate::pcb::{write_footprint, write_layers, write_shape, write_text};
use crate::pcb_items::{write_dimension, write_groups, write_zone, DimensionArgs, ZoneArgs};
use crate::{duid, mm, sexpr_str};

/// What a clipboard holds, in the clipboard's own coordinates (the copy's reference point is the origin) and with ids of
/// its own: they only tie a group to its members.
#[derive(Debug, Clone, Default)]
pub struct Clipboard {
    pub tracks: Vec<Track>,
    pub vias: Vec<Via>,
    pub zones: Vec<Zone>,
    pub shapes: Vec<Shape>,
    pub texts: Vec<Text>,
    pub dimensions: Vec<Dimension>,
    pub footprints: Vec<ClipFootprint>,
    pub groups: Vec<ClipGroup>,
    /// True when the text was a single bare footprint (`PCB_FOOTPRINT_T` in `PCB_CONTROL::Paste`) rather than a board.
    pub bare_footprint: bool,
}

impl Clipboard {
    pub fn is_empty(&self) -> bool {
        self.tracks.is_empty() && self.vias.is_empty() && self.zones.is_empty() && self.shapes.is_empty() && self.texts.is_empty() && self.dimensions.is_empty() && self.footprints.is_empty()
    }
}

/// A footprint on the clipboard: its pose, its reference and value, its pads and courtyard, and the net of each pad.
#[derive(Debug, Clone)]
pub struct ClipFootprint {
    /// The reference designator the text carries (a repeat of one inside a single clipboard has already lost the `#n` the
    /// importer gave it).
    pub reference: String,
    pub value: Option<String>,
    /// The footprint's name ("Lib:Name"), as `Part::footprint` names it.
    pub name: String,
    pub at: Point,
    pub rot: Millideg,
    pub side: Side,
    pub label: LabelSide,
    /// The pads and courtyard as the text had them, in this model's frame (`eda_model::Footprint`).
    pub definition: Option<Footprint>,
    /// Pad number -> net name, for the pads the text put on a net.
    pub pad_nets: Vec<(String, String)>,
    /// What the text says about the footprint's fields, attributes and pads beyond the geometry ([`eda_model::fp_edit::FootprintEdit`],
    /// under the copy's own id): the Reference and Value placement, user fields, `(attr ..)`, pad offsets and margins.
    pub edit: Option<eda_model::fp_edit::FootprintEdit>,
    /// What the text set for how zones connect to the footprint and to its pads (`zone_connect`, the thermal relief and the
    /// clearance of the pads and of the footprint), as the board edit that gives the pasted footprint the same ones
    /// (`DrawingsSection::zone_overrides`; the `id` is the reference the text carried). `None` when it set none.
    pub zone_overrides: Option<FootprintZoneOverrides>,
}

/// A group on the clipboard: its name and its members.
#[derive(Debug, Clone)]
pub struct ClipGroup {
    pub name: String,
    pub members: Vec<ClipRef>,
}

/// One member of a [`ClipGroup`]: the position of an item in the matching list of the [`Clipboard`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClipRef {
    Track(usize),
    Via(usize),
    Zone(usize),
    Shape(usize),
    Text(usize),
    Dimension(usize),
    Footprint(usize),
    /// A group inside the group (`PCB_GROUP` is an item like any other): the position in [`Clipboard::groups`]. Inner groups are
    /// listed before the groups that hold them.
    Group(usize),
}

fn fail(check: &str, what: &str, msg: impl Into<String>) -> Vec<CheckResult> {
    vec![CheckResult::fail(check, what, msg)]
}

// ---------------------------------------------------------------- selection

/// The items a copy names, borrowed from the board, with each group's members pulled in (`DeepClone`).
#[derive(Default)]
struct Picked<'a> {
    footprints: Vec<&'a FootprintInstance>,
    tracks: Vec<&'a Track>,
    vias: Vec<&'a Via>,
    zones: Vec<&'a Zone>,
    shapes: Vec<&'a Shape>,
    texts: Vec<&'a Text>,
    dimensions: Vec<&'a Dimension>,
    groups: Vec<&'a Group>,
}

impl<'a> Picked<'a> {
    fn total(&self) -> usize {
        self.footprints.len() + self.tracks.len() + self.vias.len() + self.zones.len() + self.shapes.len() + self.texts.len() + self.dimensions.len()
    }

    /// Files `id` under its kind. `false` when the board has nothing under that id.
    fn add_item(&mut self, design: &'a Design, id: &str, seen: &mut BTreeSet<String>) -> bool {
        if !seen.insert(id.to_string()) {
            return true;
        }
        if let Some(f) = design.placement.as_ref().and_then(|p| p.footprints.iter().find(|f| f.id == id)) {
            self.footprints.push(f);
        } else if let Some(t) = design.routing.as_ref().and_then(|r| r.tracks.iter().find(|t| t.id == id)) {
            self.tracks.push(t);
        } else if let Some(v) = design.routing.as_ref().and_then(|r| r.vias.iter().find(|v| v.id == id)) {
            self.vias.push(v);
        } else if let Some(z) = design.routing.as_ref().and_then(|r| r.zones.iter().find(|z| z.id == id)) {
            self.zones.push(z);
        } else if let Some(s) = design.drawings.as_ref().and_then(|d| d.shapes.iter().find(|s| s.id() == id)) {
            self.shapes.push(s);
        } else if let Some(t) = design.drawings.as_ref().and_then(|d| d.texts.iter().find(|t| t.id == id)) {
            self.texts.push(t);
        } else if let Some(d) = design.drawings.as_ref().and_then(|d| d.dimensions.iter().find(|d| d.id == id)) {
            self.dimensions.push(d);
        } else {
            seen.remove(id);
            return false;
        }
        true
    }

    /// A group and everything below it: its items, and the groups it holds with theirs.
    fn add_group(&mut self, design: &'a Design, g: &'a Group, seen: &mut BTreeSet<String>) {
        if !seen.insert(g.id.clone()) {
            return;
        }
        self.groups.push(g);
        for m in &g.member_ids {
            match design.drawings.as_ref().and_then(|d| d.group(m)) {
                Some(inner) => self.add_group(design, inner, seen),
                None => {
                    self.add_item(design, m, seen);
                }
            }
        }
    }

    fn resolve(design: &'a Design, ids: &[String]) -> Result<Picked<'a>, Vec<CheckResult>> {
        let mut out = Picked::default();
        let mut seen = BTreeSet::new();
        for id in ids {
            if let Some(g) = design.drawings.as_ref().and_then(|d| d.group(id)) {
                out.add_group(design, g, &mut seen);
            } else if !out.add_item(design, id, &mut seen) {
                return Err(fail("clipboard.unknown_item", id, "no placed part, track, via, zone, shape, text, dimension or group with this id"));
            }
        }
        Ok(out)
    }
}

/// A footprint's part and geometry, and its pose moved to `at`.
fn fp_pieces<'m>(model: &'m ConstraintModel, fp: &FootprintInstance, at: Point) -> Result<(&'m eda_model::Part, Footprint, FootprintInstance), Vec<CheckResult>> {
    let part = model.part(&fp.id).ok_or_else(|| fail("clipboard.unknown_part", &fp.id, "footprint id has no matching part in the constraint model"))?;
    let footprint = model.footprint_of(part).ok_or_else(|| fail("clipboard.no_footprint", &fp.id, "part has no resolvable footprint geometry"))?;
    Ok((part, footprint, FootprintInstance { at, ..fp.clone() }))
}

// ------------------------------------------------------------------- export

/// `CLIPBOARD_IO::SaveSelection`: the text of a Copy of `ids` (placed parts' references and track, via, zone, shape, text,
/// dimension and group ids), with `reference` as the point the copy is measured from -- the point a Paste will put back on
/// the cursor.
pub fn export_pcb_clipboard(design: &Design, model: &ConstraintModel, ids: &[String], reference: Point) -> Result<String, Vec<CheckResult>> {
    let picked = Picked::resolve(design, ids)?;
    if picked.total() == 0 {
        return Err(fail("clipboard.empty", "copy", "nothing to copy: none of the items is a footprint, track, via, zone, graphic, text or dimension"));
    }
    let (dx, dy) = (-reference.x, -reference.y);
    let mv = |p: Point| Point { x: p.x + dx, y: p.y + dy };
    let fp_pieces = |fp: &FootprintInstance| fp_pieces(model, fp, mv(fp.at));

    // A footprint on its own: bare, its pads on no net (`for( PAD* pad : newFootprint.Pads() ) pad->SetNetCode( 0 )`).
    if picked.total() == 1 && picked.footprints.len() == 1 {
        let (part, footprint, moved) = fp_pieces(picked.footprints[0])?;
        let mut out = String::new();
        write_footprint(&mut out, &moved, part, &footprint, &BTreeMap::new(), model, design, false, "", &FpExtra::of(design, &moved.id, &footprint));
        return Ok(out);
    }

    // Net names the items use, numbered from 1 in name order -- the table the text carries.
    let mut names: BTreeSet<&str> = BTreeSet::new();
    names.extend(picked.tracks.iter().map(|t| t.net.as_str()));
    names.extend(picked.vias.iter().map(|v| v.net.as_str()));
    names.extend(picked.zones.iter().filter(|z| !z.is_rule_area).map(|z| z.net.as_str()));
    for fp in &picked.footprints {
        let prefix = format!("{}.", fp.id);
        for n in &model.nets {
            if n.pins.iter().any(|p| p.starts_with(&prefix)) {
                names.insert(n.name.as_str());
            }
        }
    }
    names.remove("");
    let net_num: BTreeMap<&str, usize> = names.iter().enumerate().map(|(i, n)| (*n, i + 1)).collect();
    let net_n = |name: &str| net_num.get(name).copied().unwrap_or(0);

    let mut out = String::new();
    writeln!(out, "(kicad_pcb").unwrap();
    writeln!(out, "\t(version 20241229)").unwrap();
    writeln!(out, "\t(generator \"pcbnew\")").unwrap();
    writeln!(out, "\t(generator_version \"9.0\")").unwrap();
    write_layers(&mut out, &model.board.layers);
    writeln!(out, "\t(net 0 \"\")").unwrap();
    for (name, n) in &net_num {
        writeln!(out, "\t(net {n} {})", sexpr_str(name)).unwrap();
    }

    // Our item id -> the uuids it was written as, for the groups' `(members ..)`.
    let mut written: BTreeMap<String, Vec<String>> = BTreeMap::new();

    for fp in &picked.footprints {
        let (part, footprint, moved) = fp_pieces(fp)?;
        let uuid = write_footprint(&mut out, &moved, part, &footprint, &net_num, model, design, false, "", &FpExtra::of(design, &moved.id, &footprint));
        written.entry(fp.id.clone()).or_default().push(uuid);
    }
    for t in &picked.tracks {
        let (n, w, layer) = (net_n(&t.net), mm(t.width), sexpr_str(&t.layer));
        if let Some((start, mid, end)) = t.arc() {
            let (s, m, e) = (mv(start), mv(mid), mv(end));
            let uuid = duid(&format!("clip:arc:{}", t.id));
            writeln!(out, "\t(arc (start {} {}) (mid {} {}) (end {} {}) (width {w}) (layer {layer}) (net {n}) (uuid \"{uuid}\"))", mm(s.x), mm(s.y), mm(m.x), mm(m.y), mm(e.x), mm(e.y)).unwrap();
            written.entry(t.id.clone()).or_default().push(uuid);
            continue;
        }
        for (j, pair) in t.pts.windows(2).enumerate() {
            let (a, b) = (mv(pair[0]), mv(pair[1]));
            let uuid = duid(&format!("clip:segment:{}:{j}", t.id));
            writeln!(out, "\t(segment (start {} {}) (end {} {}) (width {w}) (layer {layer}) (net {n}) (uuid \"{uuid}\"))", mm(a.x), mm(a.y), mm(b.x), mm(b.y)).unwrap();
            written.entry(t.id.clone()).or_default().push(uuid);
        }
    }
    for v in &picked.vias {
        let at = mv(v.at);
        let uuid = duid(&format!("clip:via:{}", v.id));
        writeln!(out, "\t(via (at {} {}) (size {}) (drill {}) (layers {} {}) (net {}) (uuid \"{uuid}\"))", mm(at.x), mm(at.y), mm(v.diameter), mm(v.drill), sexpr_str(&v.from_layer), sexpr_str(&v.to_layer), net_n(&v.net)).unwrap();
        written.entry(v.id.clone()).or_default().push(uuid);
    }
    for z in &picked.zones {
        let mut moved = (*z).clone();
        moved.outline = z.outline.iter().map(|p| mv(*p)).collect();
        let n = if z.is_rule_area { 0 } else { net_n(&z.net) };
        let uuid = duid(&format!("clip:zone:{}", z.id));
        write_zone(&mut out, &moved, &ZoneArgs { net: n, locked: false, uuid: &uuid, fill: &[] });
        written.entry(z.id.clone()).or_default().push(uuid);
    }
    for s in &picked.shapes {
        let mut moved = (*s).clone();
        moved.translate(dx, dy);
        let uuid = write_shape(&mut out, &moved, false);
        // `write_shape` seeds its uuid by the shape's own id, which is what a group's members name.
        written.entry(s.id().to_string()).or_default().push(uuid);
    }
    for t in &picked.texts {
        let mut moved = (*t).clone();
        moved.at = mv(t.at);
        let uuid = write_text(&mut out, &moved, false);
        written.entry(t.id.clone()).or_default().push(uuid);
    }
    for d in &picked.dimensions {
        let mut moved = (*d).clone();
        eda_connectivity::dimension::translate_dimension(&mut moved, dx, dy);
        let geom = eda_connectivity::dimension::compute_dimension_geometry(&moved);
        let uuid = duid(&format!("clip:dimension:{}", d.id));
        let text_uuid = duid(&format!("clip:dimension:{}:text", d.id));
        write_dimension(&mut out, &moved, &DimensionArgs { uuid: &uuid, text_uuid: &text_uuid, locked: false, text: &geom.text, text_at: (geom.text_at.x, geom.text_at.y), text_angle_millideg: geom.text_angle });
        written.entry(d.id.clone()).or_default().push(uuid);
    }
    // Groups last, as KiCad writes them: a group names its members by uuid, and the parser resolves them once every item is read.
    write_groups(&mut out, &picked.groups, &written, &|g| duid(&format!("clip:group:{}", g.id)), &|_| false);
    writeln!(out, ")").unwrap();
    Ok(out)
}

// -------------------------------------------------------------------- parse

/// `CLIPBOARD_IO::Parse`: the items of a clipboard text -- a whole `(kicad_pcb ..)`, or one `(footprint ..)`.
pub fn parse_pcb_clipboard(text: &str) -> Result<Clipboard, Vec<CheckResult>> {
    let body = text.trim_start_matches('\u{feff}').trim();
    let bare = body.starts_with("(footprint") || body.starts_with("(module");
    let wrapped = if bare {
        // A footprint alone: put it on a board that has nothing else, as `PCB_IO_KICAD_SEXPR::Parse` hands it back without one.
        format!("(kicad_pcb (version 20241229) (generator \"pcbnew\") {body})")
    } else if body.starts_with("(kicad_pcb") {
        body.to_string()
    } else {
        return Err(fail("clipboard.not_kicad", "paste", "the clipboard does not hold KiCad PCB items"));
    };
    let (design, model, _notes) = crate::import::import_kicad_pcb(&wrapped)?;

    let mut clip = Clipboard { bare_footprint: bare, ..Default::default() };
    if let Some(rt) = &design.routing {
        clip.tracks = rt.tracks.clone();
        clip.vias = rt.vias.clone();
        clip.zones = rt.zones.clone();
    }
    if let Some(dr) = &design.drawings {
        clip.shapes = dr.shapes.clone();
        clip.texts = dr.texts.clone();
        clip.dimensions = dr.dimensions.clone();
    }
    let mut fp_index: BTreeMap<String, usize> = BTreeMap::new();
    for fp in design.placement.iter().flat_map(|p| p.footprints.iter()) {
        let part = model.part(&fp.id);
        let prefix = format!("{}.", fp.id);
        let mut pad_nets: Vec<(String, String)> = Vec::new();
        for n in &model.nets {
            for pin in n.pins.iter().filter(|p| p.starts_with(&prefix)) {
                pad_nets.push((pin[prefix.len()..].to_string(), n.name.clone()));
            }
        }
        pad_nets.sort();
        fp_index.insert(fp.id.clone(), clip.footprints.len());
        let definition = part.and_then(|p| model.footprint_of(p));
        let zone_overrides = definition.as_ref().and_then(|d| {
            let numbers: Vec<&str> = d.pads.iter().map(|p| p.number.as_str()).collect();
            design.zone_overrides_of(&fp.id, &d.name, &numbers)
        });
        clip.footprints.push(ClipFootprint {
            // The importer keeps a repeated reference apart with a `#n` suffix; the text had the plain one.
            reference: fp.id.split('#').next().unwrap_or(&fp.id).to_string(),
            value: part.and_then(|p| p.value.clone()),
            name: part.and_then(|p| p.footprint.clone()).unwrap_or_default(),
            at: fp.at,
            rot: fp.rot,
            side: fp.side,
            label: fp.label,
            definition,
            pad_nets,
            edit: design.footprint_edit(&fp.id).cloned(),
            zone_overrides,
        });
    }
    if let Some(dr) = &design.drawings {
        // The importer lists an inner group before the group that holds it, so a group's position is known by then.
        let mut group_index: BTreeMap<&str, usize> = BTreeMap::new();
        for g in &dr.groups {
            let members: Vec<ClipRef> = g
                .member_ids
                .iter()
                .filter_map(|m| {
                    group_index.get(m.as_str()).copied().map(ClipRef::Group).or_else(|| clip.tracks
                        .iter()
                        .position(|t| &t.id == m)
                        .map(ClipRef::Track)
                        .or_else(|| clip.vias.iter().position(|v| &v.id == m).map(ClipRef::Via))
                        .or_else(|| clip.zones.iter().position(|z| &z.id == m).map(ClipRef::Zone))
                        .or_else(|| clip.shapes.iter().position(|s| s.id() == m).map(ClipRef::Shape))
                        .or_else(|| clip.texts.iter().position(|t| &t.id == m).map(ClipRef::Text))
                        .or_else(|| clip.dimensions.iter().position(|d| &d.id == m).map(ClipRef::Dimension))
                        .or_else(|| fp_index.get(m).copied().map(ClipRef::Footprint)))
                })
                .collect();
            group_index.insert(g.id.as_str(), clip.groups.len());
            clip.groups.push(ClipGroup { name: g.name.clone(), members });
        }
    }
    if clip.is_empty() {
        return Err(fail("clipboard.empty", "paste", "the KiCad text on the clipboard holds no footprint, track, via, zone, graphic, text or dimension"));
    }
    Ok(clip)
}

#[cfg(test)]
mod tests {
    use super::*;
    use eda_model::ir::{ArrowDirection, DimensionKind, DimensionTextPosition, DimensionUnits, DimensionUnitsFormat, DrawingsSection, PlacementSection, Provenance, RoutingSection, TextJustify};
    use eda_model::{Net, Part, Pin, PinKind};

    fn p(x: i64, y: i64) -> Point {
        Point { x, y }
    }

    fn part(reference: &str, package: &str, nets: usize) -> Part {
        let pins = (1..=nets).map(|n| Pin { number: n.to_string(), name: None, kind: PinKind::Passive }).collect();
        Part { reference: reference.into(), mpn: None, lcsc: None, value: Some(format!("{reference}_val")), package: Some(package.into()), footprint: Some(package.into()), pins, body_um: None, symbol: None, datasheet: None, edge: None }
    }

    /// A board with a footprint on a net, and one of every other kind of item, a group over three of them.
    fn fixture() -> (Design, ConstraintModel) {
        let model = ConstraintModel {
            parts: vec![part("C1", "0603", 2), part("C2", "0603", 2)],
            nets: vec![Net { name: "VIN".into(), pins: vec!["C1.1".into(), "C2.1".into()] }, Net { name: "GND".into(), pins: vec!["C1.2".into(), "C2.2".into()] }],
            ..Default::default()
        };
        let dimension = Dimension {
            id: "dim_a".into(),
            layer: "Dwgs.User".into(),
            kind: DimensionKind::Aligned { height: 2_000 },
            start: p(30_000, 30_000),
            end: p(40_000, 30_000),
            prefix: String::new(),
            suffix: String::new(),
            override_text: None,
            units: DimensionUnits::Mm,
            units_format: DimensionUnitsFormat::NoSuffix,
            precision: 2,
            suppress_trailing_zeros: true,
            text_position: DimensionTextPosition::Outside,
            keep_text_aligned: true,
            text_angle: 0,
            text_size_um: 1_000,
            stroke_width: 150,
            arrow_length: 1_000,
            extension_offset: 300,
            extension_height: 500,
            arrow_direction: ArrowDirection::Inward,
            text_thickness_um: None,
        };
        let design = Design {
            footprint_library: None,
            sheet_contents: None,
            bus_aliases: vec![],
            symbol_library: None,
            schema: 1,
            provenance: Provenance { engine_version: "t".into(), intent_hash: "x".into(), seed: 0, stage_hashes: vec![] },
            schematic: None,
            nets: None,
            placement: Some(PlacementSection {
                outline: vec![],
                footprints: vec![
                    FootprintInstance { id: "C1".into(), at: p(110_000, 120_000), rot: 90_000, side: Side::Top, label: LabelSide::Above },
                    FootprintInstance { id: "C2".into(), at: p(120_000, 120_000), rot: 0, side: Side::Bottom, label: LabelSide::Above },
                ],
                modules: vec![],
            }),
            routing: Some(RoutingSection {
                tracks: vec![
                    Track { id: "trk_a".into(), net: "VIN".into(), pins: vec![], layer: "F.Cu".into(), width: 250, pts: vec![p(100_000, 100_000), p(110_000, 100_000), p(110_000, 105_000)], arc_mid_offset: None },
                    Track::new_arc("GND".into(), "B.Cu".into(), 300, p(100_000, 110_000), p(105_000, 105_000), p(110_000, 110_000)),
                ],
                vias: vec![Via { id: "via_a".into(), net: "VIN".into(), at: p(110_000, 105_000), drill: 300, diameter: 600, from_layer: "F.Cu".into(), to_layer: "B.Cu".into() }],
                zones: vec![Zone { id: "zone_a".into(), net: "GND".into(), layer: "F.Cu".into(), outline: vec![p(90_000, 90_000), p(95_000, 90_000), p(95_000, 95_000), p(90_000, 95_000)], ..Default::default() }],
                track_width_presets: vec![],
                via_presets: vec![],
                teardrop_settings: Default::default(),
            }),
            drawings: Some(DrawingsSection {
                shapes: vec![Shape::Segment { id: "shp_a".into(), layer: "F.SilkS".into(), stroke_width: 150, filled: false, start: p(100_000, 130_000), end: p(110_000, 130_000) }],
                texts: vec![Text { id: "txt_a".into(), content: "hello".into(), at: p(100_000, 140_000), angle: 90_000, layer: "F.SilkS".into(), size_um: 1_000, stroke_width: 150, justify: TextJustify::Left, mirror: false }],
                dimensions: vec![dimension],
                groups: vec![Group { id: "grp_a".into(), name: "block".into(), member_ids: vec!["shp_a".into(), "txt_a".into(), "zone_a".into()] }],
                locked_ids: vec!["trk_a".into()],
                ..Default::default()
            }),
        };
        let mut design = design;
        design.assign_missing_ids();
        (design, model)
    }

    fn ids(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    fn arc_track_id(d: &Design) -> String {
        d.routing.as_ref().unwrap().tracks.iter().find(|t| t.arc().is_some()).unwrap().id.clone()
    }

    #[test]
    fn the_corner_smoothing_of_a_zone_travels_with_a_copy() {
        use eda_model::ir::ZoneSmoothing;
        let (mut design, model) = fixture();
        let z = &mut design.routing.as_mut().unwrap().zones[0];
        z.smoothing = ZoneSmoothing::Fillet;
        z.corner_radius = 2_000;
        let text = export_pcb_clipboard(&design, &model, &ids(&["zone_a"]), p(0, 0)).unwrap();
        assert!(text.contains("(smoothing fillet)") && text.contains("(radius 2)"), "{text}");
        let clip = parse_pcb_clipboard(&text).unwrap();
        assert_eq!((clip.zones[0].smoothing, clip.zones[0].corner_radius), (ZoneSmoothing::Fillet, 2_000));
    }

    #[test]
    fn how_zones_connect_to_a_footprint_and_its_pads_travels_with_a_copy() {
        use eda_model::ir::{FootprintZoneFacts, PadConnection, PadZoneOverride};
        let (mut design, model) = fixture();
        design.drawings.as_mut().unwrap().zone_overrides.push(eda_model::ir::FootprintZoneOverrides {
            id: "C1".into(),
            footprint: Some(FootprintZoneFacts { zone_connection: Some(PadConnection::None), clearance: Some(900) }),
            pads: vec![PadZoneOverride { number: "2".into(), zone_connection: Some(PadConnection::Full), thermal_gap: Some(400), thermal_spoke_width: Some(300), thermal_spoke_angle_mdeg: Some(30_000), clearance: Some(600) }],
        });
        // A footprint on its own, and the same one inside a board fragment: the text says the same, and the paste reads it back.
        for names in [ids(&["C1"]), ids(&["C1", "zone_a"])] {
            let text = export_pcb_clipboard(&design, &model, &names, p(110_000, 120_000)).unwrap();
            assert!(text.contains("(zone_connect 0)") && text.contains("(clearance 0.9)"), "the footprint's own: {text}");
            assert!(text.contains("(zone_connect 2)") && text.contains("(thermal_gap 0.4)") && text.contains("(thermal_bridge_width 0.3)") && text.contains("(thermal_bridge_angle 30)"), "the pad's: {text}");
            let clip = parse_pcb_clipboard(&text).unwrap();
            let edit = clip.footprints[0].zone_overrides.as_ref().expect("the paste reads them back");
            assert_eq!(edit.footprint, Some(FootprintZoneFacts { zone_connection: Some(PadConnection::None), clearance: Some(900) }));
            assert_eq!(edit.pads.len(), 1, "{:?}", edit.pads);
            assert_eq!((edit.pads[0].number.as_str(), edit.pads[0].zone_connection, edit.pads[0].thermal_gap, edit.pads[0].thermal_spoke_width, edit.pads[0].thermal_spoke_angle_mdeg, edit.pads[0].clearance), ("2", Some(PadConnection::Full), Some(400), Some(300), Some(30_000), Some(600)));
        }
        // C2 sets nothing, and its copy says so.
        let clip = parse_pcb_clipboard(&export_pcb_clipboard(&design, &model, &ids(&["C2"]), p(0, 0)).unwrap()).unwrap();
        assert!(clip.footprints[0].zone_overrides.is_none());
    }

    #[test]
    fn a_copy_is_a_board_fragment_measured_from_the_reference_point() {
        let (design, model) = fixture();
        let arc = arc_track_id(&design);
        let text = export_pcb_clipboard(&design, &model, &ids(&["trk_a", arc.as_str(), "via_a", "zone_a", "shp_a", "txt_a", "dim_a"]), p(100_000, 100_000)).unwrap();
        assert!(text.starts_with("(kicad_pcb"), "{text}");
        assert!(text.contains("(segment (start 0 0) (end 10 0) (width 0.25) (layer \"F.Cu\") (net 2)"), "{text}");
        assert!(text.contains("(arc (start 0 10) (mid 5 5) (end 10 10) (width 0.3) (layer \"B.Cu\") (net 1)"), "{text}");
        assert!(text.contains("(net 1 \"GND\")") && text.contains("(net 2 \"VIN\")"), "the nets the items use, by name");
        assert!(!text.contains("(locked"), "a copied item cannot be locked: {text}");
        assert!(text.contains("(layers") && text.contains("\"F.Cu\""), "{text}");
        assert!(!text.contains("(general") && !text.contains("(setup") && !text.contains("Edge.Cuts\") (uuid"), "only the items and the layers travel: {text}");
    }

    #[test]
    fn what_a_copy_wrote_pastes_back_as_the_same_items_at_the_origin() {
        let (design, model) = fixture();
        let arc = arc_track_id(&design);
        let text = export_pcb_clipboard(&design, &model, &ids(&["trk_a", arc.as_str(), "via_a", "zone_a", "shp_a", "txt_a", "dim_a", "C1", "C2", "grp_a"]), p(100_000, 100_000)).unwrap();
        let clip = parse_pcb_clipboard(&text).unwrap();

        // The polyline track came back as its two segments, the arc as an arc.
        assert_eq!(clip.tracks.iter().filter(|t| t.net == "VIN").map(|t| t.pts.clone()).collect::<Vec<_>>().len(), 2);
        assert!(clip.tracks.iter().any(|t| t.arc().is_some() && t.net == "GND" && t.layer == "B.Cu" && t.pts[0] == p(0, 10_000)), "{:?}", clip.tracks);
        assert_eq!(clip.vias.len(), 1);
        assert_eq!((clip.vias[0].at, clip.vias[0].net.as_str(), clip.vias[0].from_layer.as_str()), (p(10_000, 5_000), "VIN", "F.Cu"));
        assert_eq!((clip.zones.len(), clip.zones[0].net.as_str(), clip.zones[0].outline[0]), (1, "GND", p(-10_000, -10_000)));
        assert_eq!(clip.shapes[0].points(), vec![p(0, 30_000), p(10_000, 30_000)]);
        assert_eq!((clip.texts[0].content.as_str(), clip.texts[0].at, clip.texts[0].angle), ("hello", p(0, 40_000), 90_000));
        assert_eq!((clip.dimensions[0].start, clip.dimensions[0].end), (p(-70_000, -70_000), p(-60_000, -70_000)));

        assert_eq!(clip.footprints.len(), 2);
        let c1 = clip.footprints.iter().find(|f| f.reference == "C1").unwrap();
        assert_eq!((c1.at, c1.rot, c1.side), (p(10_000, 20_000), 90_000, Side::Top));
        assert_eq!(c1.pad_nets, vec![("1".to_string(), "VIN".to_string()), ("2".to_string(), "GND".to_string())]);
        assert!(c1.definition.as_ref().is_some_and(|d| d.pads.len() == 2), "{c1:?}");
        let c2 = clip.footprints.iter().find(|f| f.reference == "C2").unwrap();
        assert_eq!((c2.at, c2.side), (p(20_000, 20_000), Side::Bottom));

        // The group named three items, and still does.
        assert_eq!(clip.groups.len(), 1);
        assert_eq!(clip.groups[0].name, "block");
        let kinds: Vec<&str> = clip.groups[0].members.iter().map(|m| match m { ClipRef::Shape(_) => "shape", ClipRef::Text(_) => "text", ClipRef::Zone(_) => "zone", _ => "other" }).collect();
        assert_eq!(kinds.len(), 3);
        assert!(kinds.contains(&"shape") && kinds.contains(&"text") && kinds.contains(&"zone"), "{kinds:?}");
    }

    #[test]
    fn a_group_that_holds_a_group_copies_and_pastes_back_nested() {
        let (mut design, model) = fixture();
        let dr = design.drawings.as_mut().unwrap();
        dr.groups = vec![
            Group { id: "grp_outer".into(), name: "outer".into(), member_ids: vec!["grp_inner".into(), "txt_a".into()] },
            Group { id: "grp_inner".into(), name: "inner".into(), member_ids: vec!["shp_a".into(), "zone_a".into()] },
        ];
        // Copying the outer group takes the inner group and what is in it along (`PCB_GROUP::DeepClone`).
        let text = export_pcb_clipboard(&design, &model, &ids(&["grp_outer"]), p(100_000, 100_000)).unwrap();
        assert_eq!(text.matches("\t(group ").count(), 2, "{text}");
        let clip = parse_pcb_clipboard(&text).unwrap();
        assert_eq!((clip.shapes.len(), clip.texts.len(), clip.zones.len()), (1, 1, 1), "the inner group's items travel too");
        assert_eq!(clip.groups.len(), 2);
        let inner = clip.groups.iter().position(|g| g.name == "inner").unwrap();
        let outer = clip.groups.iter().position(|g| g.name == "outer").unwrap();
        assert!(inner < outer, "an inner group is listed before the group that holds it");
        assert!(clip.groups[outer].members.contains(&ClipRef::Group(inner)), "{:?}", clip.groups[outer].members);
        assert_eq!(clip.groups[outer].members.len(), 2);
        assert_eq!(clip.groups[inner].members.len(), 2);
    }

    #[test]
    fn a_footprint_copied_alone_is_bare_and_has_its_pads_on_no_net() {
        let (design, model) = fixture();
        let text = export_pcb_clipboard(&design, &model, &ids(&["C1"]), p(110_000, 120_000)).unwrap();
        assert!(text.trim_start().starts_with("(footprint"), "{text}");
        assert!(!text.contains("(net "), "a lone footprint travels without its nets: {text}");
        assert!(text.contains("(at 0 0 -90)"), "moved so its anchor is the origin: {text}");
        let clip = parse_pcb_clipboard(&text).unwrap();
        assert!(clip.bare_footprint);
        assert_eq!(clip.footprints.len(), 1);
        assert_eq!((clip.footprints[0].reference.as_str(), clip.footprints[0].at, clip.footprints[0].rot), ("C1", p(0, 0), 90_000));
        assert!(clip.footprints[0].pad_nets.is_empty());
    }

    #[test]
    fn kicad_tens_own_clipboard_with_nets_by_name_reads_too() {
        // What `CLIPBOARD_IO::SaveSelection` writes at KiCad 10: no net table, `(net "name")` on each item.
        let text = r#"(kicad_pcb (version 20260410) (generator "pcbnew") (generator_version "10.0")
            (layers (0 "F.Cu" signal) (31 "B.Cu" signal) (37 "F.SilkS" user))
            (segment (start 1 2) (end 6 2) (width 0.2) (layer "F.Cu") (net "Net-(R1-Pad1)") (uuid "11111111-1111-4111-8111-111111111111"))
            (via (at 6 2) (size 0.6) (drill 0.3) (layers "F.Cu" "B.Cu") (net "Net-(R1-Pad1)") (uuid "22222222-2222-4222-8222-222222222222"))
            (gr_text "note" (at 3 4 0) (layer "F.SilkS") (uuid "33333333-3333-4333-8333-333333333333") (effects (font (size 1 1) (thickness 0.15))))
        )"#;
        let clip = parse_pcb_clipboard(text).unwrap();
        assert_eq!((clip.tracks.len(), clip.tracks[0].net.as_str(), clip.tracks[0].pts.clone()), (1, "Net-(R1-Pad1)", vec![p(1_000, 2_000), p(6_000, 2_000)]));
        assert_eq!((clip.vias.len(), clip.vias[0].net.as_str()), (1, "Net-(R1-Pad1)"));
        assert_eq!(clip.texts[0].content, "note");
    }

    #[test]
    fn text_that_is_not_kicad_items_is_refused() {
        assert_eq!(parse_pcb_clipboard("hello world").unwrap_err()[0].check, "clipboard.not_kicad");
        assert_eq!(parse_pcb_clipboard("(kicad_sch (version 1))").unwrap_err()[0].check, "clipboard.not_kicad");
        assert_eq!(parse_pcb_clipboard("(kicad_pcb (version 20241229) (layers (0 \"F.Cu\" signal)))").unwrap_err()[0].check, "clipboard.empty", "a board with nothing in it has nothing to paste");
        assert_eq!(export_pcb_clipboard(&fixture().0, &fixture().1, &ids(&["nope"]), p(0, 0)).unwrap_err()[0].check, "clipboard.unknown_item");
    }
}
