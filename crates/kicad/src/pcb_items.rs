//! The board items the PCB writer ([`crate::pcb`]) writes besides pads, tracks and graphics:
//! zones (copper pours, rule areas, teardrops), dimensions and groups.
//!
//! Each one is ported from the matching `PCB_IO_KICAD_SEXPR::format( const ... )` in
//! `pcbnew/pcb_io/kicad_sexpr/pcb_io_kicad_sexpr.cpp` (`ZONE`, `PCB_DIMENSION_BASE`, `PCB_GROUP`),
//! and every token here is one `pcb_io_kicad_sexpr_parser.cpp` (`parseZONE`, `parseDIMENSION`,
//! `parseGROUP`) reads back. kicad-cli judges the file this module helps write, so whatever the
//! writer drops, DRC never sees: before this module a rule area was written as a pour (and failed
//! the whole export for having no net), every zone got the board's clearance and width instead of
//! its own, and dimensions and groups were not written at all.

use std::fmt::Write as _;

use eda_model::ir::{ArrowDirection, Dimension, DimensionKind, DimensionTextPosition, DimensionUnits, DimensionUnitsFormat, Group, IslandRemovalMode, PadConnection, Zone, ZoneBorderStyle, ZoneSmoothing};

use crate::{fmt_mm_f, mm, sexpr_str};

/// `ZONE_SETTINGS`'s default hatch pitch (`ZONE::GetDefaultHatchPitch()`, 0.5 mm): the second
/// number of `(hatch edge 0.5)`. Display only.
const DEFAULT_HATCH_PITCH: &str = "0.5";

/// A teardrop zone's own settings, as `TEARDROP_MANAGER::createTeardrop` (`pcbnew/teardrop/teardrop.cpp`)
/// gives them: no clearance of its own, the thinnest zone KiCad allows (0.0254 mm), full pad
/// connection, islands kept. Its outline is the final shape, so the fill must not eat into it.
const TEARDROP_MIN_THICKNESS: &str = "0.0254";

/// What [`write_zone`] needs besides the zone itself.
pub(crate) struct ZoneArgs<'a> {
    /// The KiCad net number (1-based, from the board's net table); 0 for a zone with no net.
    pub net: usize,
    pub locked: bool,
    pub uuid: &'a str,
    /// The fill the studio's own filler computed, one ring per `(filled_polygon ..)`, in µm.
    pub fill: &'a [Vec<(i64, i64)>],
}

/// `PCB_IO_KICAD_SEXPR::format( const ZONE* )`.
///
/// A rule area is a zone with `(keepout ..)` and `(placement ..)` and no net, and never a fill
/// (`GetIsRuleArea()`). A pour carries its own priority, clearance, minimum width, pad connection,
/// thermal gap and spoke width, island removal and hatch settings, name and lock; a teardrop
/// carries `(attr (teardrop ..))` and the settings KiCad's teardrop manager gives its own.
pub(crate) fn write_zone(out: &mut String, z: &Zone, a: &ZoneArgs) {
    let rule_area = z.is_rule_area;
    write!(out, "\t(zone").unwrap();
    // Only a pour on a copper layer has a net (`aZone->IsOnCopperLayer() && !aZone->GetIsRuleArea() && GetNetCode() > 0`).
    if !rule_area && a.net > 0 {
        write!(out, " (net {}) (net_name {})", a.net, sexpr_str(&z.net)).unwrap();
    }
    if a.locked {
        write!(out, " (locked yes)").unwrap();
    }
    writeln!(out, " (layer {}) (uuid \"{}\")", sexpr_str(&z.layer), a.uuid).unwrap();
    if !z.name.is_empty() && !z.teardrop {
        writeln!(out, "\t\t(name {})", sexpr_str(&z.name)).unwrap();
    }
    // `INVISIBLE_BORDER` (a teardrop's) is written as `none`, like `NO_HATCH`.
    let hatch = if z.teardrop {
        "none"
    } else {
        match z.border_style {
            ZoneBorderStyle::None => "none",
            ZoneBorderStyle::Edge => "edge",
            ZoneBorderStyle::Full => "full",
        }
    };
    writeln!(out, "\t\t(hatch {hatch} {DEFAULT_HATCH_PITCH})").unwrap();
    if z.priority > 0 {
        writeln!(out, "\t\t(priority {})", z.priority).unwrap();
    }
    if z.teardrop {
        writeln!(out, "\t\t(attr (teardrop (type padvia)))").unwrap();
    }

    let (clearance, min_thickness, connection, island_mode) = if z.teardrop {
        ("0".to_string(), TEARDROP_MIN_THICKNESS.to_string(), PadConnection::Full, IslandRemovalMode::Never)
    } else {
        (mm(z.clearance), mm(z.min_thickness), z.pad_connection, z.island_removal_mode)
    };
    // `ZONE_CONNECTION::THERMAL` is the default and is not written (`case THERMAL: // Default option not saved or loaded`).
    let connect = match connection {
        PadConnection::Thermal => "",
        PadConnection::ThtThermal => " thru_hole_only",
        PadConnection::Full => " yes",
        PadConnection::None => " no",
    };
    writeln!(out, "\t\t(connect_pads{connect} (clearance {clearance}))").unwrap();
    // `(filled_areas_thickness no)`: the file version this writer stamps is older than 20250210, and
    // the parser reads a fill from before that as the legacy stroked one unless told otherwise
    // (`isStrokedFill`). Every fill written here is already the final outline.
    writeln!(out, "\t\t(min_thickness {min_thickness}) (filled_areas_thickness no)").unwrap();

    if rule_area {
        let tok = |not_allowed: bool| if not_allowed { "not_allowed" } else { "allowed" };
        writeln!(
            out,
            "\t\t(keepout (tracks {}) (vias {}) (pads {}) (copperpour {}) (footprints {}))",
            tok(z.keepout_tracks),
            tok(z.keepout_vias),
            tok(z.keepout_pads),
            tok(z.keepout_copper_pour),
            tok(z.keepout_footprints)
        )
        .unwrap();
        // Multichannel placement settings: this model has none, so the area never places anything.
        writeln!(out, "\t\t(placement (enabled no))").unwrap();
    }

    // `(fill [yes] [(mode hatch)] (thermal_gap ..) (thermal_bridge_width ..) [smoothing] (island_removal_mode ..) ..)`.
    write!(out, "\t\t(fill").unwrap();
    if !rule_area {
        write!(out, " yes").unwrap();
    }
    let hatch_fill = !z.teardrop && z.fill_mode == eda_model::ir::FillMode::HatchPattern;
    if hatch_fill {
        write!(out, " (mode hatch)").unwrap();
    }
    if !z.teardrop {
        write!(out, " (thermal_gap {}) (thermal_bridge_width {})", mm(z.thermal_gap), mm(z.thermal_spoke_width)).unwrap();
    }
    if !rule_area && z.smoothing != ZoneSmoothing::None {
        write!(out, " (smoothing {})", if z.smoothing == ZoneSmoothing::Chamfer { "chamfer" } else { "fillet" }).unwrap();
        if z.corner_radius != 0 {
            write!(out, " (radius {})", mm(z.corner_radius)).unwrap();
        }
    }
    let island_code = match island_mode {
        IslandRemovalMode::Always => 0,
        IslandRemovalMode::Never => 1,
        IslandRemovalMode::Area => 2,
    };
    write!(out, " (island_removal_mode {island_code})").unwrap();
    if island_mode == IslandRemovalMode::Area {
        // `GetMinIslandArea() / IU_PER_MM`: nm^2 over nm per mm, which is the number of mm^2 once printed as a length.
        write!(out, " (island_area_min {})", fmt_mm_f(z.min_island_area as f64 / 1_000_000.0)).unwrap();
    }
    if hatch_fill {
        write!(out, " (hatch_thickness {}) (hatch_gap {}) (hatch_orientation {})", mm(z.hatch_thickness), mm(z.hatch_gap), fmt_mm_f(z.hatch_orientation_mdeg as f64 / 1000.0)).unwrap();
        if z.hatch_smoothing_level > 0 {
            write!(out, " (hatch_smoothing_level {}) (hatch_smoothing_value {})", z.hatch_smoothing_level, fmt_mm_f(z.hatch_smoothing_value)).unwrap();
        }
        write!(
            out,
            " (hatch_border_algorithm {}) (hatch_min_hole_area {})",
            if z.hatch_border_algorithm != 0 { "hatch_thickness" } else { "min_thickness" },
            fmt_mm_f(z.hatch_hole_min_area)
        )
        .unwrap();
    }
    writeln!(out, ")").unwrap();

    writeln!(out, "\t\t(polygon (pts").unwrap();
    for p in &z.outline {
        writeln!(out, "\t\t\t(xy {} {})", mm(p.x), mm(p.y)).unwrap();
    }
    writeln!(out, "\t\t))").unwrap();

    if !rule_area {
        for ring in a.fill {
            if ring.len() < 3 {
                continue;
            }
            writeln!(out, "\t\t(filled_polygon").unwrap();
            writeln!(out, "\t\t\t(layer {})", sexpr_str(&z.layer)).unwrap();
            writeln!(out, "\t\t\t(pts").unwrap();
            for &(x, y) in ring {
                writeln!(out, "\t\t\t\t(xy {} {})", mm(x), mm(y)).unwrap();
            }
            writeln!(out, "\t\t\t)").unwrap();
            writeln!(out, "\t\t)").unwrap();
        }
    }
    writeln!(out, "\t)").unwrap();
}

/// `DIM_UNITS_MODE`: inch 0, mils 1, mm 2, automatic 3.
fn units_code(u: DimensionUnits) -> u8 {
    match u {
        DimensionUnits::Inch => 0,
        DimensionUnits::Mil => 1,
        DimensionUnits::Mm => 2,
        DimensionUnits::Automatic => 3,
    }
}

/// What [`write_dimension`] needs besides the dimension itself.
pub(crate) struct DimensionArgs<'a> {
    pub uuid: &'a str,
    pub text_uuid: &'a str,
    pub locked: bool,
    /// The text KiCad shows and where: `eda_connectivity::dimension::compute_dimension_geometry`,
    /// the one place that geometry is worked out (KiCad recomputes both on load, `dim->Update()`).
    pub text: &'a str,
    pub text_at: (i64, i64),
    pub text_angle_millideg: u32,
}

/// `PCB_IO_KICAD_SEXPR::format( const PCB_DIMENSION_BASE* )`.
pub(crate) fn write_dimension(out: &mut String, d: &Dimension, a: &DimensionArgs) {
    let ty = match d.kind {
        DimensionKind::Aligned { .. } => "aligned",
        DimensionKind::Orthogonal { .. } => "orthogonal",
        DimensionKind::Radial { .. } => "radial",
        DimensionKind::Leader => "leader",
        DimensionKind::Center => "center",
    };
    write!(out, "\t(dimension (type {ty})").unwrap();
    if a.locked {
        write!(out, " (locked yes)").unwrap();
    }
    writeln!(out, " (layer {}) (uuid \"{}\")", sexpr_str(&d.layer), a.uuid).unwrap();
    writeln!(out, "\t\t(pts (xy {} {}) (xy {} {}))", mm(d.start.x), mm(d.start.y), mm(d.end.x), mm(d.end.y)).unwrap();
    match d.kind {
        DimensionKind::Aligned { height } => writeln!(out, "\t\t(height {})", mm(height)).unwrap(),
        DimensionKind::Orthogonal { height, horizontal } => {
            writeln!(out, "\t\t(height {})", mm(height)).unwrap();
            // `PCB_DIM_ORTHOGONAL::DIR`: HORIZONTAL 0, VERTICAL 1.
            writeln!(out, "\t\t(orientation {})", if horizontal { 0 } else { 1 }).unwrap();
        }
        DimensionKind::Radial { leader_length } => writeln!(out, "\t\t(leader_length {})", mm(leader_length)).unwrap(),
        DimensionKind::Leader | DimensionKind::Center => {}
    }
    let center = matches!(d.kind, DimensionKind::Center);
    if !center {
        let fmt_code = match d.units_format {
            DimensionUnitsFormat::NoSuffix => 0,
            DimensionUnitsFormat::BareSuffix => 1,
            DimensionUnitsFormat::ParenSuffix => 2,
        };
        write!(
            out,
            "\t\t(format (prefix {}) (suffix {}) (units {}) (units_format {fmt_code}) (precision {})",
            sexpr_str(&d.prefix),
            sexpr_str(&d.suffix),
            units_code(d.units),
            d.precision
        )
        .unwrap();
        if let Some(text) = &d.override_text {
            write!(out, " (override_value {})", sexpr_str(text)).unwrap();
        }
        if d.suppress_trailing_zeros {
            write!(out, " (suppress_zeroes yes)").unwrap();
        }
        writeln!(out, ")").unwrap();
    }
    // `(style (thickness ..) (arrow_length ..) (text_position_mode ..) ..)`; `DIM_TEXT_POSITION`: outside 0, inline 1, manual 2.
    write!(
        out,
        "\t\t(style (thickness {}) (arrow_length {}) (text_position_mode {})",
        mm(d.stroke_width),
        mm(d.arrow_length),
        if d.text_position == DimensionTextPosition::Inline { 1 } else { 0 }
    )
    .unwrap();
    if matches!(d.kind, DimensionKind::Aligned { .. } | DimensionKind::Orthogonal { .. }) {
        write!(out, " (arrow_direction {})", if d.arrow_direction == ArrowDirection::Inward { "inward" } else { "outward" }).unwrap();
    }
    if matches!(d.kind, DimensionKind::Aligned { .. } | DimensionKind::Orthogonal { .. }) {
        write!(out, " (extension_height {})", mm(d.extension_height)).unwrap();
    }
    if matches!(d.kind, DimensionKind::Leader) {
        // `DIM_TEXT_BORDER::NONE`: this model has no text frame.
        write!(out, " (text_frame 0)").unwrap();
    }
    write!(out, " (extension_offset {})", mm(d.extension_offset)).unwrap();
    if d.keep_text_aligned {
        write!(out, " (keep_text_aligned yes)").unwrap();
    }
    writeln!(out, ")").unwrap();
    if !center {
        // The dimension's text goes last "to be sure the text options are known when reading the file"; always `gr_text`.
        // Text on a back layer is read from below, so it is written mirrored.
        let mirror = if d.layer.starts_with("B.") { " (justify mirror)" } else { "" };
        let size = mm(d.text_size_um);
        let thickness = mm(d.text_thickness_um.unwrap_or_else(|| (d.text_size_um as f64 * 0.15).round() as i64));
        writeln!(
            out,
            "\t\t(gr_text {} (at {} {} {}) (layer {}) (uuid \"{}\")\n\t\t\t(effects (font (size {size} {size}) (thickness {thickness})){mirror})\n\t\t)",
            sexpr_str(a.text),
            mm(a.text_at.0),
            mm(a.text_at.1),
            fmt_mm_f(a.text_angle_millideg as f64 / 1000.0),
            sexpr_str(&d.layer),
            a.text_uuid
        )
        .unwrap();
    }
    writeln!(out, "\t)").unwrap();
}

/// `PCB_IO_KICAD_SEXPR::format( const PCB_GROUP* )`: a name, a uuid and the sorted uuids of the
/// members. A group with no member on the board is not written (`if( memberIds.empty() ) return;`).
pub(crate) fn write_group(out: &mut String, g: &Group, uuid: &str, locked: bool, mut members: Vec<String>) {
    if members.is_empty() {
        return;
    }
    members.sort();
    members.dedup();
    write!(out, "\t(group {} (uuid \"{uuid}\")", sexpr_str(&g.name)).unwrap();
    if locked {
        write!(out, " (locked yes)").unwrap();
    }
    write!(out, " (members").unwrap();
    for m in &members {
        write!(out, " \"{m}\"").unwrap();
    }
    writeln!(out, "))").unwrap();
}
