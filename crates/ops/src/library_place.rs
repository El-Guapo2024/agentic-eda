//! Parts that come out of a library: a symbol's definition kept with the schematic ([`Cmd::EmbedLibSymbol`]) and a footprint put on
//! the board as a part of its own ([`Cmd::PlaceFootprint`]).
//!
//! Neither reads a library: the definition travels with the command (the studio's server fills it in from KiCad's installed
//! libraries), so what a command did replays and undoes the same on a machine without them.

use super::Board;
use eda_model::ir::{BoardPart, FootprintInstance, LabelSide, LibraryFootprint, LibrarySymbol, Point, Side};
use eda_model::{CheckResult, Part};
use std::collections::BTreeSet;

/// The reference prefix KiCad's own libraries and conventions give a footprint of this library or name: the letters a user would number
/// it with (`H1` for a mounting hole, `TP3` for a test point). `REF` where nothing says.
pub fn reference_prefix_for(footprint: &str) -> &'static str {
    let (lib, name) = footprint.split_once(':').unwrap_or(("", footprint));
    let (lib, name) = (lib.to_lowercase(), name.to_lowercase());
    let starts = |p: &str| lib.starts_with(p) || name.starts_with(p);
    if starts("mountinghole") {
        "H"
    } else if starts("testpoint") {
        "TP"
    } else if starts("fiducial") {
        "FID"
    } else if lib.starts_with("resistor") || name.starts_with("r_") {
        "R"
    } else if lib.starts_with("capacitor") || name.starts_with("c_") {
        "C"
    } else if lib.starts_with("inductor") || name.starts_with("l_") {
        "L"
    } else if lib.starts_with("led") || lib.starts_with("diode") || name.starts_with("d_") || name.starts_with("led_") {
        "D"
    } else if lib.starts_with("connector") || lib.starts_with("terminalblock") || lib.starts_with("pinheader") || lib.starts_with("pinsocket") {
        "J"
    } else if lib.starts_with("button_switch") {
        "SW"
    } else if lib.starts_with("crystal") {
        "Y"
    } else if lib.starts_with("fuse") {
        "F"
    } else if lib.starts_with("buzzer") {
        "BZ"
    } else if lib.starts_with("battery") {
        "BT"
    } else if lib.starts_with("relay") {
        "K"
    } else if lib.starts_with("jumper") {
        "JP"
    } else if lib.starts_with("transformer") {
        "T"
    } else if lib.starts_with("package_") || lib.starts_with("module") {
        "U"
    } else {
        "REF"
    }
}

impl Board<'_> {
    /// Every reference a new footprint must not take: the intent's parts, the placed footprints, the board's own parts.
    fn taken_references(&self) -> BTreeSet<String> {
        let mut out: BTreeSet<String> = self.model.parts.iter().map(|p| p.reference.clone()).collect();
        out.extend(self.placement().footprints.iter().map(|f| f.id.clone()));
        if let Some(dr) = &self.design.drawings {
            out.extend(dr.board_parts.iter().map(|b| b.reference.clone()));
        }
        out
    }

    /// [`Cmd::EmbedLibSymbol`].
    pub(crate) fn embed_lib_symbol(&mut self, symbol: &LibrarySymbol) -> Result<(), Vec<CheckResult>> {
        let Some((lib, name)) = symbol.lib_id.split_once(':') else {
            return Err(vec![CheckResult::fail("ops_bad_symbol", &symbol.lib_id, "a library symbol is named Library:Name")]);
        };
        if lib.is_empty() || name.is_empty() {
            return Err(vec![CheckResult::fail("ops_bad_symbol", &symbol.lib_id, "a library symbol is named Library:Name")]);
        }
        // The design's own copy wins (`SCH_EDITOR_CONTROL::Paste` looks in the screen's library symbols first), and so does a definition the intent
        // gives the model: neither is replaced by a library's.
        let kept = self.design.symbol_library.as_ref().is_some_and(|l| l.by_lib_id(&symbol.lib_id).is_some());
        if kept || self.model.symbols.iter().any(|s| s.lib_id == symbol.lib_id) {
            return Ok(());
        }
        let mut entry = symbol.clone();
        entry.published = true;
        entry.unit_count = entry.unit_count.max(1);
        for p in &mut entry.pins {
            p.id.clear();
        }
        for g in &mut entry.graphics {
            g.set_id(String::new());
        }
        entry.assign_missing_ids();
        let lib = self.symbol_library_mut();
        lib.symbols.push(entry);
        lib.symbols.sort_by(|a, b| a.lib_id.cmp(&b.lib_id));
        Ok(())
    }

    /// [`Cmd::PlaceFootprint`].
    pub(crate) fn place_footprint(&mut self, footprint: &str, at: Point, reference: &str, value: &str, definition: Option<&LibraryFootprint>) -> Result<(), Vec<CheckResult>> {
        let name = footprint.trim();
        if name.is_empty() {
            return Err(vec![CheckResult::fail("ops_bad_footprint", "footprint", "there is no footprint to place")]);
        }
        // The pads must come from somewhere: the model knows the name (the intent's, a library's it resolved, the built-in table), or the command
        // brought the definition.
        let probe = Part { reference: "?".into(), mpn: None, lcsc: None, value: None, package: None, footprint: Some(name.to_string()), symbol: None, datasheet: None, pins: vec![], body_um: None, edge: None };
        let known = self.model.footprint_of(&probe).is_some();
        if !known && definition.is_none() {
            return Err(vec![CheckResult::fail("ops_unknown_footprint", name, "no footprint with this name is installed or known to the board")]);
        }
        let taken = self.taken_references();
        let reference = if reference.trim().is_empty() {
            let prefix = reference_prefix_for(name);
            (1..).map(|n| format!("{prefix}{n}")).find(|r| !taken.contains(r)).expect("an unbounded range finds a free number")
        } else if taken.contains(reference.trim()) {
            return Err(vec![CheckResult::fail("ops_duplicate_reference", reference, format!("a footprint called '{}' is already on the board", reference.trim()))]);
        } else {
            reference.trim().to_string()
        };
        let value = if value.trim().is_empty() { name.rsplit(':').next().unwrap_or(name).to_string() } else { value.trim().to_string() };
        let stored = if known { None } else { definition.cloned() };
        let at = self.snap_point(at.x, at.y);

        let dr = self.drawings_mut();
        dr.board_parts.push(BoardPart { reference: reference.clone(), value: Some(value), footprint: name.to_string(), definition: stored, pad_nets: Vec::new() });
        dr.board_parts.sort_by(|a, b| a.reference.cmp(&b.reference));
        let fps = &mut self.design.placement.as_mut().expect("a Board always carries a placement section").footprints;
        fps.push(FootprintInstance { id: reference, at, rot: 0, side: Side::Top, label: LabelSide::default() });
        self.sort_footprints();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Cmd;
    use eda_model::ir::Design;
    use eda_model::ConstraintModel;

    /// A design built from JSON, so these tests do not break when another section is added to `Design`.
    fn board(model: &ConstraintModel) -> Board<'_> {
        let design: Design = serde_json::from_str(
            r#"{"schema":1,"provenance":{"engine_version":"0","intent_hash":"x","seed":0,"stage_hashes":[]},
                "placement":{"outline":[{"x":0,"y":0},{"x":100000,"y":0},{"x":100000,"y":100000},{"x":0,"y":100000}],"footprints":[],"modules":[]}}"#,
        )
        .unwrap();
        Board::new(design, model, 100, 300)
    }

    /// `MountingHole_3.2mm_M3.kicad_mod` of KiCad's library, as the file has it.
    const MOUNTING_HOLE: &str = r#"(footprint "MountingHole_3.2mm_M3" (version 20260206) (generator "kicad-footprint-generator") (layer "F.Cu")
        (descr "Mounting Hole 3.2mm, M3, no annular") (tags "mountinghole M3")
        (property "Reference" "REF**" (at 0 -4.15 0) (layer "F.SilkS") (effects (font (size 1 1) (thickness 0.15))))
        (property "Value" "MountingHole_3.2mm_M3" (at 0 4.15 0) (layer "F.Fab") (effects (font (size 1 1) (thickness 0.15))))
        (attr exclude_from_pos_files exclude_from_bom)
        (fp_circle (center 0 0) (end 3.45 0) (stroke (width 0.05) (type solid)) (fill no) (layer "F.CrtYd"))
        (pad "" np_thru_hole circle (at 0 0) (size 3.2 3.2) (drill 3.2) (layers "*.Cu" "*.Mask")))"#;

    fn hole() -> LibraryFootprint {
        let mut fp = eda_kicad::parse_library_footprint(MOUNTING_HOLE).unwrap().footprint;
        fp.name = "MountingHole:MountingHole_3.2mm_M3".into();
        fp
    }

    #[test]
    fn prefixes_follow_kicads_libraries() {
        for (name, prefix) in [
            ("MountingHole:MountingHole_3.2mm_M3", "H"),
            ("TestPoint:TestPoint_Pad_D1.0mm", "TP"),
            ("Fiducial:Fiducial_1mm_Mask2mm", "FID"),
            ("Resistor_SMD:R_0805_2012Metric", "R"),
            ("Capacitor_SMD:C_0603_1608Metric", "C"),
            ("LED_SMD:LED_0603_1608Metric", "D"),
            ("Connector_PinHeader_2.54mm:PinHeader_1x04_P2.54mm_Vertical", "J"),
            ("Package_SO:SOIC-8_3.9x4.9mm_P1.27mm", "U"),
            ("Something:Else", "REF"),
            ("Bare", "REF"),
        ] {
            assert_eq!(reference_prefix_for(name), prefix, "{name}");
        }
    }

    #[test]
    fn embedding_a_symbol_keeps_it_published_and_never_replaces_one_the_design_has() {
        let model = ConstraintModel::default();
        let mut b = board(&model);
        let mut sym = LibrarySymbol::new_empty("Amplifier_Operational:LM358");
        sym.unit_count = 3;
        sym.description = "Dual op-amp".into();
        b.apply(&Cmd::EmbedLibSymbol { symbol: sym.clone() }).unwrap();
        let lib = b.design().symbol_library.as_ref().unwrap();
        let kept = lib.by_lib_id("Amplifier_Operational:LM358").unwrap();
        assert!(kept.published, "it resolves into the model like any published entry");
        assert_eq!((kept.unit_count, kept.description.as_str()), (3, "Dual op-amp"));
        // the second time is a no-op, even for a different definition
        let mut other = sym.clone();
        other.description = "changed".into();
        b.apply(&Cmd::EmbedLibSymbol { symbol: other }).unwrap();
        assert_eq!(b.design().symbol_library.as_ref().unwrap().symbols.len(), 1);
        assert_eq!(b.design().symbol_library.as_ref().unwrap().by_lib_id("Amplifier_Operational:LM358").unwrap().description, "Dual op-amp");
        // a name without a library is refused
        assert_eq!(b.apply(&Cmd::EmbedLibSymbol { symbol: LibrarySymbol::new_empty("LM358") }).unwrap_err()[0].check, "ops_bad_symbol");
    }

    #[test]
    fn a_definition_the_intent_gives_the_model_is_not_replaced_by_a_librarys() {
        let mut model = ConstraintModel::default();
        model.symbols.push(eda_model::LibSymbol { lib_id: "Device:R".into(), graphics: vec![], pins: vec![], power: false, in_bom: true, on_board: true, datasheet: String::new(), description: String::new(), reference_prefix: "R".into(), unit_count: 1, pin_names_hidden: false, pin_numbers_hidden: false, pin_name_offset_mm: 0.0 });
        let mut b = board(&model);
        b.apply(&Cmd::EmbedLibSymbol { symbol: LibrarySymbol::new_empty("Device:R") }).unwrap();
        assert!(b.design().symbol_library.is_none(), "the model already resolves Device:R");
    }

    #[test]
    fn a_placed_footprint_is_a_part_of_its_own_with_the_next_free_reference() {
        let model = ConstraintModel::default();
        let mut b = board(&model);
        let place = |b: &mut Board, x: i64| b.apply(&Cmd::PlaceFootprint { footprint: "MountingHole:MountingHole_3.2mm_M3".into(), at: Point { x, y: 5_000 }, reference: String::new(), value: String::new(), definition: Some(hole()) });
        place(&mut b, 10_000).unwrap();
        place(&mut b, 20_000).unwrap();
        let dr = b.design().drawings.as_ref().unwrap();
        assert_eq!(dr.board_parts.iter().map(|p| p.reference.as_str()).collect::<Vec<_>>(), ["H1", "H2"]);
        assert_eq!(dr.board_parts[0].value.as_deref(), Some("MountingHole_3.2mm_M3"), "KiCad's value is the footprint's name");
        assert_eq!(dr.board_parts[0].footprint, "MountingHole:MountingHole_3.2mm_M3");
        assert!(dr.board_parts[0].definition.is_some(), "the pads travel with the part");
        let poses: Vec<(&str, i64)> = b.design().placement.as_ref().unwrap().footprints.iter().map(|f| (f.id.as_str(), f.at.x)).collect();
        assert_eq!(poses, [("H1", 10_000), ("H2", 20_000)]);
        assert!(b.design().placement.as_ref().unwrap().footprints.iter().all(|f| f.rot == 0 && f.side == Side::Top));
    }

    #[test]
    fn a_footprint_nobody_knows_is_refused_and_a_taken_reference_too() {
        let model = ConstraintModel::default();
        let mut b = board(&model);
        let e = b.apply(&Cmd::PlaceFootprint { footprint: "Nowhere:Nothing".into(), at: Point { x: 0, y: 0 }, reference: String::new(), value: String::new(), definition: None }).unwrap_err();
        assert_eq!(e[0].check, "ops_unknown_footprint");
        let e = b.apply(&Cmd::PlaceFootprint { footprint: "".into(), at: Point { x: 0, y: 0 }, reference: String::new(), value: String::new(), definition: None }).unwrap_err();
        assert_eq!(e[0].check, "ops_bad_footprint");
        b.apply(&Cmd::PlaceFootprint { footprint: "MountingHole:MH".into(), at: Point { x: 0, y: 0 }, reference: "MH1".into(), value: "M3".into(), definition: Some(hole()) }).unwrap();
        let e = b.apply(&Cmd::PlaceFootprint { footprint: "MountingHole:MH".into(), at: Point { x: 0, y: 0 }, reference: "MH1".into(), value: String::new(), definition: Some(hole()) }).unwrap_err();
        assert_eq!(e[0].check, "ops_duplicate_reference");
        assert_eq!(b.design().drawings.as_ref().unwrap().board_parts.len(), 1, "a refused command changes nothing");
        assert_eq!(b.design().drawings.as_ref().unwrap().board_parts[0].value.as_deref(), Some("M3"));
    }
}
