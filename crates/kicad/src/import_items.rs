//! The importer's side of the board items the PCB writer ([`crate::pcb_items`]) writes: dimensions,
//! groups and locks, plus the uuid bookkeeping a group needs.
//!
//! Ported from `PCB_IO_KICAD_SEXPR_PARSER::parseDIMENSION` and `parseGROUP`
//! (`pcbnew/pcb_io/kicad_sexpr/pcb_io_kicad_sexpr_parser.cpp`); the `locked` token follows
//! `parseMaybeAbsentBool( true )` as every item parser reads it.
//!
//! A group names its members by their file uuid, and this IR names an item by an id it is given
//! after the whole file has been read ([`eda_model::ir::Design::assign_missing_ids`]). So each
//! item parser records `(uuid, locked)` for the items it emits, in the order it emits them
//! ([`Refs`]), and [`Refs::resolve`] turns those into group member ids and the locked set once
//! the ids exist.

use std::collections::HashMap;

use eda_model::ir::{ArrowDirection, Design, Dimension, DimensionKind, DimensionSettings, DimensionTextPosition, DimensionUnits, DimensionUnitsFormat, Group, Point, Um};

use crate::sexpr::{self, Sexpr};

/// What the importer remembers about one item beyond the IR: its file uuid and whether it is locked.
#[derive(Debug, Clone, Default)]
pub(crate) struct ItemRef {
    pub uuid: Option<String>,
    pub locked: bool,
}

impl ItemRef {
    /// Read the `(uuid ..)`/`(tstamp ..)` and `locked` of one parsed item.
    pub(crate) fn of(item: &[Sexpr]) -> ItemRef {
        ItemRef { uuid: item_uuid(item), locked: is_locked(item) }
    }
}

/// `(uuid "..")`, or the pre-7 `(tstamp ..)`.
pub(crate) fn item_uuid(item: &[Sexpr]) -> Option<String> {
    sexpr::find(item, "uuid").or_else(|| sexpr::find(item, "tstamp")).and_then(|u| sexpr::txt(u, 1)).map(str::to_string)
}

/// `locked` as an item parser reads it: `(locked yes)`, `(locked no)`, or the bare token the
/// pre-6.0 format used among the item's own children.
pub(crate) fn is_locked(item: &[Sexpr]) -> bool {
    if let Some(l) = sexpr::find(item, "locked") {
        return sexpr::txt(l, 1) != Some("no");
    }
    item.iter().skip(1).any(|it| it.text() == Some("locked"))
}

/// One `ItemRef` per emitted IR item, per kind, in emission order (so index `i` is the `i`th
/// item of the matching `Vec` in the finished `Design`).
#[derive(Debug, Default)]
pub(crate) struct Refs {
    pub footprints: Vec<ItemRef>,
    pub tracks: Vec<ItemRef>,
    pub vias: Vec<ItemRef>,
    pub zones: Vec<ItemRef>,
    pub shapes: Vec<ItemRef>,
    pub texts: Vec<ItemRef>,
    pub dimensions: Vec<ItemRef>,
}

impl Refs {
    /// The groups of the file, resolved to this design's item ids, and every locked item's id.
    /// `design` has had its ids assigned already; `raw_groups` come from [`import_groups`].
    pub(crate) fn resolve(&self, design: &Design, raw_groups: &[RawGroup]) -> (Vec<Group>, Vec<String>) {
        let mut by_uuid: HashMap<String, Vec<String>> = HashMap::new();
        let mut locked: Vec<String> = Vec::new();
        let mut note = |refs: &[ItemRef], ids: Vec<&str>| {
            for (r, id) in refs.iter().zip(ids) {
                if id.is_empty() {
                    continue;
                }
                if let Some(u) = &r.uuid {
                    by_uuid.entry(u.clone()).or_default().push(id.to_string());
                }
                if r.locked {
                    locked.push(id.to_string());
                }
            }
        };
        if let Some(pl) = &design.placement {
            note(&self.footprints, pl.footprints.iter().map(|f| f.id.as_str()).collect());
        }
        if let Some(rt) = &design.routing {
            note(&self.tracks, rt.tracks.iter().map(|t| t.id.as_str()).collect());
            note(&self.vias, rt.vias.iter().map(|v| v.id.as_str()).collect());
            note(&self.zones, rt.zones.iter().map(|z| z.id.as_str()).collect());
        }
        if let Some(dr) = &design.drawings {
            note(&self.shapes, dr.shapes.iter().map(|s| s.id()).collect());
            note(&self.texts, dr.texts.iter().map(|t| t.id.as_str()).collect());
            note(&self.dimensions, dr.dimensions.iter().map(|d| d.id.as_str()).collect());
        }

        // A group may name another group: this IR has no nested groups, so the inner group's members are
        // pulled into the outer one and the inner group is not kept (`Cmd::Group`'s own flattening rule).
        let by_group_uuid: HashMap<&str, &RawGroup> = raw_groups.iter().filter_map(|g| g.uuid.as_deref().map(|u| (u, g))).collect();
        fn expand<'a>(g: &'a RawGroup, by_group_uuid: &HashMap<&str, &'a RawGroup>, by_uuid: &HashMap<String, Vec<String>>, seen: &mut Vec<&'a str>, out: &mut Vec<String>) {
            for m in &g.members {
                if let Some(ids) = by_uuid.get(m.as_str()) {
                    out.extend(ids.iter().cloned());
                } else if let Some(inner) = by_group_uuid.get(m.as_str()) {
                    if let Some(u) = inner.uuid.as_deref() {
                        if seen.contains(&u) {
                            continue;
                        }
                        seen.push(u);
                    }
                    expand(inner, by_group_uuid, by_uuid, seen, out);
                }
            }
        }
        let nested: std::collections::HashSet<&str> = raw_groups.iter().flat_map(|g| g.members.iter().map(String::as_str)).filter(|m| by_group_uuid.contains_key(m)).collect();
        let mut groups = Vec::new();
        for g in raw_groups {
            // An inner group is part of its outer one, not a group of its own.
            if g.uuid.as_deref().is_some_and(|u| nested.contains(u)) {
                continue;
            }
            let mut members = Vec::new();
            let mut seen = Vec::new();
            expand(g, &by_group_uuid, &by_uuid, &mut seen, &mut members);
            members.sort();
            members.dedup();
            // A group of one item (or none: its members are not on this board) is nothing to keep.
            if members.len() < 2 {
                continue;
            }
            groups.push(Group { id: String::new(), name: g.name.clone(), member_ids: members });
        }
        locked.sort();
        locked.dedup();
        (groups, locked)
    }
}

/// One `(group ..)` of the file, members still named by uuid.
#[derive(Debug, Clone, Default)]
pub(crate) struct RawGroup {
    pub name: String,
    pub uuid: Option<String>,
    pub members: Vec<String>,
}

/// `PCB_IO_KICAD_SEXPR_PARSER::parseGROUP`: `(group "name" (uuid ..) [(locked yes)] [(lib_id ..)] (members "uuid" ..))`.
pub(crate) fn import_groups(root: &[Sexpr]) -> Vec<RawGroup> {
    sexpr::find_all(root, "group")
        .map(|g| {
            let name = g.get(1).and_then(Sexpr::text).unwrap_or("").to_string();
            let uuid = sexpr::find(g, "uuid").or_else(|| sexpr::find(g, "id")).and_then(|u| sexpr::txt(u, 1)).map(str::to_string);
            let members = sexpr::find(g, "members").map(|m| m.iter().skip(1).filter_map(Sexpr::text).map(str::to_string).collect()).unwrap_or_default();
            RawGroup { name, uuid, members }
        })
        .collect()
}

fn xy(list: &[Sexpr]) -> Option<Point> {
    let (x, y) = (sexpr::num(list, 1)?, sexpr::num(list, 2)?);
    Some(Point { x: crate::mm_to_um(x), y: crate::mm_to_um(y) })
}

fn um_of(item: &[Sexpr], key: &str) -> Option<Um> {
    sexpr::find(item, key).and_then(|v| sexpr::num(v, 1)).map(crate::mm_to_um)
}

/// A flag that is present as `(key yes)`, a bare `key`, or absent (`parseMaybeAbsentBool( true )`).
fn flag(item: &[Sexpr], key: &str) -> bool {
    if let Some(l) = sexpr::find(item, key) {
        return sexpr::txt(l, 1) != Some("no");
    }
    item.iter().skip(1).any(|it| it.text() == Some(key))
}

/// `PCB_IO_KICAD_SEXPR_PARSER::parseDIMENSION`, for the format KiCad 6 and later write
/// (`(type ..)`); the older `(feature1 ..)`/`(crossbar ..)` form is skipped. The dimension's
/// measured value and text are not read: both are recomputed from the feature points and the
/// format settings (`dim->Update()` in KiCad, `compute_dimension_geometry` here).
pub(crate) fn import_dimensions(root: &[Sexpr], refs: &mut Refs) -> Vec<Dimension> {
    let defaults = DimensionSettings::default();
    let mut out = Vec::new();
    for d in sexpr::find_all(root, "dimension") {
        let Some(ty) = sexpr::find(d, "type").and_then(|t| sexpr::txt(t, 1)) else { continue };
        let Some(pts) = sexpr::find(d, "pts") else { continue };
        let mut points = sexpr::find_all(pts, "xy").filter_map(xy);
        let (Some(start), Some(end)) = (points.next(), points.next()) else { continue };
        let layer = sexpr::find(d, "layer").and_then(|l| sexpr::txt(l, 1)).unwrap_or("Cmts.User").to_string();
        let height = um_of(d, "height").unwrap_or(0);
        let kind = match ty {
            "aligned" => DimensionKind::Aligned { height },
            // `PCB_DIM_ORTHOGONAL::DIR`: HORIZONTAL 0, VERTICAL 1.
            "orthogonal" => DimensionKind::Orthogonal { height, horizontal: sexpr::find(d, "orientation").and_then(|o| sexpr::num(o, 1)).unwrap_or(0.0) as i64 == 0 },
            "radial" => DimensionKind::Radial { leader_length: um_of(d, "leader_length").unwrap_or(0) },
            "leader" => DimensionKind::Leader,
            "center" => DimensionKind::Center,
            _ => continue,
        };

        // (format (prefix ..) (suffix ..) (units N) (units_format N) (precision N) [(override_value ..)] [(suppress_zeroes ..)])
        let format = sexpr::find(d, "format");
        let text_of = |key: &str| format.and_then(|f| sexpr::find(f, key)).and_then(|v| sexpr::txt(v, 1)).unwrap_or("").to_string();
        let int_of = |key: &str| format.and_then(|f| sexpr::find(f, key)).and_then(|v| sexpr::num(v, 1)).map(|n| n as i64);
        let units = match int_of("units") {
            Some(0) => DimensionUnits::Inch,
            Some(1) => DimensionUnits::Mil,
            Some(2) => DimensionUnits::Mm,
            _ => DimensionUnits::Automatic,
        };
        let units_format = match int_of("units_format") {
            Some(1) => DimensionUnitsFormat::BareSuffix,
            Some(2) => DimensionUnitsFormat::ParenSuffix,
            _ => DimensionUnitsFormat::NoSuffix,
        };
        // `DIM_PRECISION`: 0..=5 are 0 to 5 decimals; the four unit-dependent `V_VV..` levels (6..=9) are two to five.
        let precision = match int_of("precision") {
            Some(p @ 0..=5) => p as u8,
            Some(p @ 6..=9) => (p - 4) as u8,
            _ => defaults.precision,
        };
        let override_text = format.and_then(|f| sexpr::find(f, "override_value")).and_then(|v| sexpr::txt(v, 1)).map(str::to_string);
        let suppress_trailing_zeros = format.map(|f| flag(f, "suppress_zeroes")).unwrap_or(false);

        // (style (thickness ..) (arrow_length ..) (text_position_mode N) (arrow_direction ..) (extension_height ..) (extension_offset ..) [(keep_text_aligned yes)])
        let style = sexpr::find(d, "style");
        let st = |key: &str| style.and_then(|s| um_of(s, key));
        let text_position = match style.and_then(|s| sexpr::find(s, "text_position_mode")).and_then(|v| sexpr::num(v, 1)) {
            Some(m) if m as i64 == 1 => DimensionTextPosition::Inline,
            _ => DimensionTextPosition::Outside,
        };
        // The pre-6.0 default; the style block overrides it either way.
        let arrow_direction = match style.and_then(|s| sexpr::find(s, "arrow_direction")).and_then(|v| sexpr::txt(v, 1)) {
            Some("inward") => ArrowDirection::Inward,
            _ => ArrowDirection::Outward,
        };
        // "new format: default to keep text aligned off unless token is present"
        let keep_text_aligned = style.map(|s| flag(s, "keep_text_aligned")).unwrap_or(false);

        // The text: its size and pen, and (when the label is not kept aligned) its angle.
        let text = sexpr::find(d, "gr_text");
        let font = text.and_then(|t| sexpr::find(t, "effects")).and_then(|e| sexpr::find(e, "font"));
        let text_size_um = font.and_then(|f| sexpr::find(f, "size")).and_then(|s| sexpr::num(s, 1)).map(crate::mm_to_um).unwrap_or(defaults.text_size_um);
        let text_thickness_um = font.and_then(|f| um_of(f, "thickness"));
        let text_angle = text
            .and_then(|t| sexpr::find(t, "at"))
            .and_then(|a| sexpr::num(a, 3))
            .map(|deg| ((deg * 1000.0).round() as i64).rem_euclid(360_000) as u32)
            .unwrap_or(0);

        refs.dimensions.push(ItemRef::of(d));
        out.push(Dimension {
            id: String::new(),
            layer,
            kind,
            start,
            end,
            prefix: text_of("prefix"),
            suffix: text_of("suffix"),
            override_text,
            units,
            units_format,
            precision,
            suppress_trailing_zeros,
            text_position,
            keep_text_aligned,
            text_angle,
            text_size_um,
            stroke_width: st("thickness").unwrap_or(defaults.stroke_width),
            arrow_length: st("arrow_length").unwrap_or(defaults.arrow_length),
            extension_offset: st("extension_offset").unwrap_or(defaults.extension_offset),
            extension_height: st("extension_height").unwrap_or(defaults.extension_height),
            arrow_direction,
            text_thickness_um,
        });
    }
    out
}
