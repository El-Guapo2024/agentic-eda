//! The library-control verbs of the Footprint Editor and the Symbol Editor.
//!
//! `FOOTPRINT_EDITOR_CONTROL` (`pcbnew/tools/footprint_editor_control.cpp`) and
//! `SYMBOL_EDITOR_CONTROL` (`eeschema/tools/symbol_editor_control.cpp`) work on a
//! library *tree*: cut/copy/paste/duplicate/rename/delete/import an entry of the
//! library the tree has selected. The studio's library is the project's own
//! (`design.footprint_library` / `design.symbol_library`, a flat list keyed by
//! name / `lib_id`), so a tree action is one of the verbs below, run in the
//! matching editor's own undo domain:
//!
//! * [`Cmd::PutLibraryFootprint`] / [`Cmd::PutLibrarySymbol`] -- store a whole
//!   entry under its own name. `FOOTPRINT_EDIT_FRAME::DuplicateFootprint`,
//!   `FOOTPRINT_EDITOR_CONTROL::PasteFootprint`, `ImportFootprint`,
//!   `SaveFootprintAs`, and on the symbol side `SYMBOL_EDIT_FRAME::DuplicateSymbol`
//!   (duplicate and paste), `ImportSymbol` and `saveSymbolCopyAs` all end in the
//!   library adapter's "save this item under that name"; the caller picks the
//!   unique name first (`ensureUniqueName`, `_copy`, `_1`, ...) exactly the way
//!   those functions do, this verb only refuses a name that is taken.
//! * [`Cmd::RenameLibraryFootprint`] / [`Cmd::RenameLibrarySymbol`] --
//!   `RenameFootprint` / `SYMBOL_EDITOR_CONTROL::RenameSymbol`.
//! * [`Cmd::RepairFootprint`] -- `FOOTPRINT_EDITOR_CONTROL::RepairFootprint`.
//! * [`Cmd::SetSymbolAnchor`] -- `SYMBOL_EDITOR_DRAWING_TOOLS::PlaceAnchor`.
//!
//! Every one of them touches the project library of its own editor and nothing
//! else, which is what keeps `restore_domain` (crates/cli/src/board.rs) able to
//! undo it without touching the board or the schematic.

use eda_model::ir::{LibraryFootprint, LibraryPad, LibrarySymbol};
use eda_model::symbol::SPoint;
use eda_model::CheckResult;

use super::Board;

/// `FOOTPRINT::StringLibNameInvalidChars( false )`: the characters a footprint name may not hold.
const FOOTPRINT_NAME_INVALID: &str = "%$<>\t\n\r\"\\/:";

/// `FOOTPRINT::IsLibNameValid`, applied to the item part of a `Lib:Name` footprint name (the
/// library nickname is checked by [`library_nickname_illegal_char`], not here).
pub fn footprint_item_name_illegal_char(name: &str) -> Option<char> {
    name.chars().find(|c| FOOTPRINT_NAME_INVALID.contains(*c))
}

/// `LIB_ID::isLegalChar` (common/lib_id.cpp) with `illegal_filename_chars_allowed = false`:
/// `:` (the nickname separator), tab, newline, return, `\`, `<`, `>` and `"` may not appear in a
/// library item name.
pub fn lib_item_name_illegal_char(name: &str) -> Option<char> {
    name.chars().find(|c| matches!(c, ':' | '\t' | '\n' | '\r' | '\\' | '<' | '>' | '"'))
}

/// `LIB_ID::isLegalLibraryNameChar`: a library nickname may not hold a control character, `\` or `:`.
pub fn library_nickname_illegal_char(nick: &str) -> Option<char> {
    nick.chars().find(|c| (*c as u32) < 0x20 || matches!(c, '\\' | ':'))
}

/// Split `Lib:Name` into its nickname (empty for a bare name) and item name.
pub fn split_lib_name(name: &str) -> (&str, &str) {
    match name.split_once(':') {
        Some((lib, item)) => (lib, item),
        None => ("", name),
    }
}

/// `PAD::CanHaveNumber`: every pad but a non-plated hole (and an aperture pad, which the library pad
/// does not model) takes a number.
pub fn pad_can_have_number(pad: &LibraryPad) -> bool {
    pad.kind != eda_model::footprint::PadKind::NonPlatedHole
}

/// `PAD::ImportSettingsFrom( aMasterPad )` (pcbnew/pad.cpp) for what the library pad models: the
/// padstack (shape, size, offset, drill, corner ratios, trapezoid delta), the layer set, the
/// attribute (pad type), the orientation and the local clearance / thermal overrides move from
/// `master` to `dst`; the number and the position are never touched. Then the three fix-ups the C++
/// makes after the copy: a circle master makes `dst` a true circle (`SetSize( x, x )`), a master that
/// is SMD (or connector) drops the hole, and a pad that cannot have a number loses it
/// (`if( !CanHaveNumber() ) SetNumber( wxEmptyString )`).
///
/// This is the one rule behind Paste Pad Properties (`ApplyPadSettings`), Push Pad Properties and the
/// pad a Place Pad click creates from the default pad (`m_Pad_Master`), so the three cannot drift apart.
pub fn import_pad_settings(dst: &mut LibraryPad, master: &LibraryPad) {
    dst.shape = master.shape;
    dst.size = master.size;
    dst.offset = master.offset;
    dst.layers = master.layers.clone();
    dst.kind = master.kind;
    dst.rot = master.rot;
    dst.drill = master.drill;
    dst.drill_slot = master.drill_slot;
    dst.roundrect_ratio = master.roundrect_ratio;
    dst.trapezoid_delta = master.trapezoid_delta;
    dst.chamfer_ratio = master.chamfer_ratio;
    dst.chamfer_corners = master.chamfer_corners;
    dst.clearance_override = master.clearance_override;
    dst.thermal_gap_override = master.thermal_gap_override;
    dst.thermal_spoke_width_override = master.thermal_spoke_width_override;

    if master.shape == eda_model::ir::LibraryPadShape::Circle {
        dst.size = (dst.size.0, dst.size.0);
    }
    if master.kind == eda_model::footprint::PadKind::Smd {
        dst.drill = None;
        dst.drill_slot = None;
    }
    if !pad_can_have_number(dst) {
        dst.number.clear();
    }
}

fn bad_name(subject: &str, what: &str, c: char) -> Vec<CheckResult> {
    vec![CheckResult::fail("ops_bad_name", subject, format!("{what} cannot contain {c:?}"))]
}

fn check_footprint_name(name: &str) -> Result<(), Vec<CheckResult>> {
    let (lib, item) = split_lib_name(name);
    if item.is_empty() {
        return Err(vec![CheckResult::fail("ops_bad_footprint", name, "a footprint must have a name")]);
    }
    if let Some(c) = library_nickname_illegal_char(lib) {
        return Err(bad_name(name, "a library nickname", c));
    }
    if let Some(c) = footprint_item_name_illegal_char(item) {
        return Err(bad_name(name, "a footprint name", c));
    }
    Ok(())
}

fn check_symbol_lib_id(lib_id: &str) -> Result<(), Vec<CheckResult>> {
    let (lib, item) = split_lib_name(lib_id);
    if item.is_empty() {
        return Err(vec![CheckResult::fail("ops_bad_symbol", lib_id, "a symbol must have a name")]);
    }
    if let Some(c) = library_nickname_illegal_char(lib) {
        return Err(bad_name(lib_id, "a library nickname", c));
    }
    if let Some(c) = lib_item_name_illegal_char(item) {
        return Err(bad_name(lib_id, "a symbol name", c));
    }
    Ok(())
}

impl<'a> Board<'a> {
    /// Whether `name` is already taken as a footprint: in the project library, or resolvable by the
    /// model (intent, a loaded `.kicad_mod`, the builtin table).
    fn footprint_name_taken(&self, name: &str) -> bool {
        self.design.footprint_library.as_ref().and_then(|l| l.by_name(name)).is_some() || self.resolve_named_footprint(name).is_some()
    }

    fn symbol_lib_id_taken(&self, lib_id: &str) -> bool {
        self.design.symbol_library.as_ref().and_then(|l| l.by_lib_id(lib_id)).is_some() || self.model.symbol_of(lib_id).is_some()
    }

    /// See [`Cmd::PutLibraryFootprint`](crate::Cmd::PutLibraryFootprint).
    pub(crate) fn put_library_footprint(&mut self, fp: &LibraryFootprint, overwrite: bool) -> Result<(), Vec<CheckResult>> {
        check_footprint_name(&fp.name)?;
        let in_library = self.design.footprint_library.as_ref().and_then(|l| l.by_name(&fp.name)).is_some();
        if !overwrite && (in_library || self.footprint_name_taken(&fp.name)) {
            return Err(vec![CheckResult::fail("ops_footprint_exists", &fp.name, "a footprint with this name already exists; pick another name")]);
        }
        let mut entry = fp.clone();
        entry.published = false;
        // The project library entry being replaced keeps following the board if it already did: an
        // overwrite is the person saying "this is now the definition of that name".
        if let Some(old) = self.design.footprint_library.as_ref().and_then(|l| l.by_name(&fp.name)) {
            entry.published = old.published;
        }
        let lib = self.footprint_library_mut();
        lib.footprints.retain(|f| f.name != entry.name);
        lib.footprints.push(entry);
        let stored = lib.by_name_mut(&fp.name).expect("just pushed");
        // Ids from another document (a paste, an import) must not collide with each other; blank every
        // one and let the usual deterministic assignment hand them out again.
        for p in &mut stored.pads {
            p.id.clear();
        }
        for g in &mut stored.graphics {
            g.set_id(String::new());
        }
        for t in &mut stored.texts {
            t.id.clear();
        }
        stored.assign_missing_ids();
        Ok(())
    }

    /// See [`Cmd::RenameLibraryFootprint`](crate::Cmd::RenameLibraryFootprint).
    pub(crate) fn rename_library_footprint(&mut self, name: &str, new_name: &str, overwrite: bool) -> Result<(), Vec<CheckResult>> {
        check_footprint_name(new_name)?;
        if self.design.footprint_library.as_ref().and_then(|l| l.by_name(name)).is_none() {
            return Err(vec![CheckResult::fail("ops_unknown_footprint", name, "this footprint is not in the project library; open it in the Footprint Editor first")]);
        }
        if new_name == name {
            return Ok(()); // `if( newName == oldName ) return 0;`
        }
        let clash_in_library = self.design.footprint_library.as_ref().and_then(|l| l.by_name(new_name)).is_some();
        if !overwrite && (clash_in_library || self.footprint_name_taken(new_name)) {
            return Err(vec![CheckResult::fail("ops_footprint_exists", new_name, "a footprint with this name already exists; pick another name")]);
        }
        let lib = self.footprint_library_mut();
        lib.footprints.retain(|f| f.name != new_name);
        lib.by_name_mut(name).expect("checked above").name = new_name.to_string();
        Ok(())
    }

    /// See [`Cmd::RepairFootprint`](crate::Cmd::RepairFootprint).
    pub(crate) fn repair_footprint(&mut self, name: &str) -> Result<(), Vec<CheckResult>> {
        let fp = self.library_footprint_mut(name)?;
        // `RepairFootprint::processItem`: the first item to hold an id keeps it, a later one that
        // repeats it gets a new one (`ResetUuid`). Pads are processed first -- the principal use of an
        // id is a marker pointing at a pad -- then graphics, then text.
        let mut seen = std::collections::BTreeSet::new();
        for p in &mut fp.pads {
            if !p.id.is_empty() && !seen.insert(p.id.clone()) {
                p.id.clear();
            }
        }
        for g in &mut fp.graphics {
            if !g.id().is_empty() && !seen.insert(g.id().to_string()) {
                g.set_id(String::new());
            }
        }
        for t in &mut fp.texts {
            if !t.id.is_empty() && !seen.insert(t.id.clone()) {
                t.id.clear();
            }
        }
        fp.assign_missing_ids();
        Ok(())
    }

    /// See [`Cmd::PutLibrarySymbol`](crate::Cmd::PutLibrarySymbol).
    pub(crate) fn put_library_symbol(&mut self, sym: &LibrarySymbol, overwrite: bool) -> Result<(), Vec<CheckResult>> {
        check_symbol_lib_id(&sym.lib_id)?;
        let in_library = self.design.symbol_library.as_ref().and_then(|l| l.by_lib_id(&sym.lib_id)).is_some();
        if !overwrite && (in_library || self.symbol_lib_id_taken(&sym.lib_id)) {
            return Err(vec![CheckResult::fail("ops_symbol_exists", &sym.lib_id, "a symbol with this name already exists; pick another name")]);
        }
        let mut entry = sym.clone();
        entry.published = self.design.symbol_library.as_ref().and_then(|l| l.by_lib_id(&sym.lib_id)).map(|old| old.published).unwrap_or(false);
        for p in &mut entry.pins {
            p.id.clear();
        }
        for g in &mut entry.graphics {
            g.set_id(String::new());
        }
        entry.unit_count = entry.unit_count.max(1);
        entry.assign_missing_ids();
        let lib = self.symbol_library_mut();
        lib.symbols.retain(|s| s.lib_id != entry.lib_id);
        lib.symbols.push(entry);
        Ok(())
    }

    /// See [`Cmd::RenameLibrarySymbol`](crate::Cmd::RenameLibrarySymbol).
    pub(crate) fn rename_library_symbol(&mut self, lib_id: &str, new_lib_id: &str, overwrite: bool) -> Result<(), Vec<CheckResult>> {
        check_symbol_lib_id(new_lib_id)?;
        if self.design.symbol_library.as_ref().and_then(|l| l.by_lib_id(lib_id)).is_none() {
            return Err(vec![CheckResult::fail("ops_unknown_symbol", lib_id, "this symbol is not in the project library; open it in the Symbol Editor first")]);
        }
        if new_lib_id == lib_id {
            return Ok(());
        }
        let clash_in_library = self.design.symbol_library.as_ref().and_then(|l| l.by_lib_id(new_lib_id)).is_some();
        if !overwrite && (clash_in_library || self.symbol_lib_id_taken(new_lib_id)) {
            return Err(vec![CheckResult::fail("ops_symbol_exists", new_lib_id, "a symbol with this name already exists; pick another name")]);
        }
        let lib = self.symbol_library_mut();
        lib.symbols.retain(|s| s.lib_id != new_lib_id);
        lib.by_lib_id_mut(lib_id).expect("checked above").lib_id = new_lib_id.to_string();
        Ok(())
    }

    /// See [`Cmd::SetSymbolAnchor`](crate::Cmd::SetSymbolAnchor).
    pub(crate) fn set_symbol_anchor(&mut self, lib_id: &str, at: SPoint) -> Result<(), Vec<CheckResult>> {
        let sym = self.library_symbol_mut(lib_id)?;
        // `symbol->Move( -cursorPos )`: every item of every unit and body style moves, so the clicked
        // point becomes the symbol's origin.
        let (dx, dy) = (-at.x, -at.y);
        for g in &mut sym.graphics {
            g.translate(dx, dy);
        }
        for p in &mut sym.pins {
            p.at = SPoint { x: p.at.x + dx, y: p.at.y + dy };
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn item_names_reject_exactly_the_characters_kicad_does() {
        assert_eq!(lib_item_name_illegal_char("R_0603"), None);
        assert_eq!(lib_item_name_illegal_char("My Part 1/2"), None, "a space and a slash are fine in a symbol name");
        assert_eq!(lib_item_name_illegal_char("a:b"), Some(':'));
        assert_eq!(lib_item_name_illegal_char("a\"b"), Some('"'));
        assert_eq!(footprint_item_name_illegal_char("R_0603"), None);
        assert_eq!(footprint_item_name_illegal_char("a/b"), Some('/'), "a footprint name is stricter: no slash");
        assert_eq!(footprint_item_name_illegal_char("50%"), Some('%'));
        assert_eq!(library_nickname_illegal_char("My Lib"), None);
        assert_eq!(library_nickname_illegal_char("a\\b"), Some('\\'));
    }

    #[test]
    fn split_lib_name_separates_the_nickname() {
        assert_eq!(split_lib_name("Device:R"), ("Device", "R"));
        assert_eq!(split_lib_name("Untitled"), ("", "Untitled"));
        assert_eq!(split_lib_name("A:B:C"), ("A", "B:C"));
    }
}

#[cfg(test)]
mod verb_tests {
    use super::*;
    use crate::{Cmd, Domain};
    use eda_model::ir::{Design, LibraryPad, LibrarySymbolGraphic, LibrarySymbolPin};
    use eda_model::ConstraintModel;

    /// A design built from JSON, so these tests do not break when another section is added to
    /// `Design` (every new field carries a serde default).
    fn design() -> Design {
        serde_json::from_str(r#"{"schema":1,"provenance":{"engine_version":"0","intent_hash":"x","seed":0,"stage_hashes":[]}}"#).unwrap()
    }

    fn board(m: &ConstraintModel) -> Board<'_> {
        Board::new(design(), m, 100, 300)
    }

    fn pad(number: &str, x: i64) -> LibraryPad {
        LibraryPad {
            id: String::new(),
            number: number.into(),
            at: eda_model::ir::Point { x, y: 0 },
            size: (1000, 600),
            offset: eda_model::ir::Point::default(),
            shape: eda_model::ir::LibraryPadShape::Rect,
            kind: eda_model::footprint::PadKind::Smd,
            drill: None,
            drill_slot: None,
            rot: 0,
            roundrect_ratio: None,
            trapezoid_delta: None,
            chamfer_ratio: None,
            chamfer_corners: eda_model::ir::ChamferCorners::default(),
            layers: vec![],
            clearance_override: None,
            thermal_gap_override: None,
            thermal_spoke_width_override: None,
        }
    }

    fn footprint_with_pads(name: &str) -> LibraryFootprint {
        let mut fp = LibraryFootprint::new_empty(name);
        fp.pads = vec![pad("1", -1000), pad("2", 1000)];
        fp.assign_missing_ids();
        fp
    }

    fn symbol_with_pins(lib_id: &str) -> LibrarySymbol {
        let mut sym = LibrarySymbol::new_empty(lib_id);
        for (n, y, bs) in [("1", 5.08, 1u32), ("2", -5.08, 1)] {
            sym.pins.push(LibrarySymbolPin {
                id: String::new(),
                number: n.into(),
                name: String::new(),
                electrical_type: "passive".into(),
                shape: "line".into(),
                at: SPoint { x: 0.0, y },
                angle_deg: 270.0,
                length_mm: 2.54,
                unit: 1,
                body_style: bs,
                hidden: false,
                name_size_mm: None,
                number_size_mm: None,
            });
        }
        sym.graphics.push(LibrarySymbolGraphic::Rectangle { id: String::new(), unit: 0, body_style: 1, start: SPoint { x: -2.54, y: -3.81 }, end: SPoint { x: 2.54, y: 3.81 }, stroke_mm: 0.254, fill: eda_model::ir::LibraryFill::None });
        sym.assign_missing_ids();
        sym
    }

    #[test]
    fn import_pad_settings_copies_the_padstack_and_never_the_number_or_position() {
        let mut master = pad("M", 0);
        master.shape = eda_model::ir::LibraryPadShape::Circle;
        master.size = (1600, 900); // an inconsistent circle: the import squares it from the x size
        master.kind = eda_model::footprint::PadKind::ThroughHole;
        master.drill = Some(800);
        master.rot = 90_000;
        master.layers = vec!["*.Cu".into(), "*.Mask".into()];
        master.clearance_override = Some(250);
        let mut dst = pad("7", 5000);
        import_pad_settings(&mut dst, &master);
        assert_eq!(dst.number, "7");
        assert_eq!(dst.at.x, 5000);
        assert_eq!((dst.shape, dst.size, dst.drill, dst.rot), (eda_model::ir::LibraryPadShape::Circle, (1600, 1600), Some(800), 90_000));
        assert_eq!(dst.kind, eda_model::footprint::PadKind::ThroughHole);
        assert_eq!(dst.layers, vec!["*.Cu".to_string(), "*.Mask".to_string()]);
        assert_eq!(dst.clearance_override, Some(250));
    }

    #[test]
    fn import_pad_settings_drops_the_hole_of_an_smd_master_and_the_number_of_an_npth_one() {
        let mut master = pad("M", 0);
        master.kind = eda_model::footprint::PadKind::Smd;
        master.drill = Some(500); // a stray hole on an SMD master is not carried over
        let mut dst = pad("3", 0);
        dst.kind = eda_model::footprint::PadKind::ThroughHole;
        dst.drill = Some(900);
        import_pad_settings(&mut dst, &master);
        assert_eq!(dst.drill, None, "SetDrillSize( 0, 0 ) for an SMD master");

        let mut npth = pad("M", 0);
        npth.kind = eda_model::footprint::PadKind::NonPlatedHole;
        npth.drill = Some(1200);
        let mut dst = pad("4", 0);
        import_pad_settings(&mut dst, &npth);
        assert_eq!(dst.number, "", "a pad that cannot have a number loses it");
        assert_eq!(dst.drill, Some(1200));
    }

    #[test]
    fn push_pad_properties_imports_the_whole_settings_set_into_every_unfiltered_pad() {
        let m = ConstraintModel::default();
        let mut b = board(&m);
        b.apply(&Cmd::PutLibraryFootprint { footprint: footprint_with_pads("P"), overwrite: false }).unwrap();
        let ids: Vec<String> = b.design().footprint_library.as_ref().unwrap().by_name("P").unwrap().pads.iter().map(|p| p.id.clone()).collect();
        let mut src = footprint_with_pads("P").pads[0].clone();
        src.id = ids[0].clone();
        src.rot = 180_000;
        src.size = (2000, 1600);
        src.kind = eda_model::footprint::PadKind::ThroughHole;
        src.drill = Some(700);
        src.layers = vec!["*.Cu".into(), "*.Mask".into()];
        b.apply(&Cmd::EditPad { footprint: "P".into(), id: ids[0].clone(), pad: src }).unwrap();
        b.apply(&Cmd::PushPadProperties { footprint: "P".into(), source_pad_id: ids[0].clone(), filter_shape: false, filter_orientation: false, filter_layers: false, filter_type: false }).unwrap();
        let fp = b.design().footprint_library.as_ref().unwrap().by_name("P").unwrap();
        let target = fp.pads.iter().find(|p| p.id == ids[1]).unwrap();
        assert_eq!((target.rot, target.kind, target.drill), (180_000, eda_model::footprint::PadKind::ThroughHole, Some(700)), "orientation and pad type travel too once their filters are off");
        assert_eq!(target.number, "2");
    }

    #[test]
    fn the_new_verbs_run_in_their_own_editors_undo_domain() {
        let fp = footprint_with_pads("A");
        assert_eq!(Cmd::PutLibraryFootprint { footprint: fp, overwrite: false }.domain(), Domain::FootprintEditor);
        assert_eq!(Cmd::RenameLibraryFootprint { name: "A".into(), new_name: "B".into(), overwrite: false }.domain(), Domain::FootprintEditor);
        assert_eq!(Cmd::RepairFootprint { name: "A".into() }.domain(), Domain::FootprintEditor);
        let sym = symbol_with_pins("eda:A");
        assert_eq!(Cmd::PutLibrarySymbol { symbol: sym, overwrite: false }.domain(), Domain::SymbolEditor);
        assert_eq!(Cmd::RenameLibrarySymbol { lib_id: "eda:A".into(), new_lib_id: "eda:B".into(), overwrite: false }.domain(), Domain::SymbolEditor);
        assert_eq!(Cmd::SetSymbolAnchor { lib_id: "eda:A".into(), at: SPoint { x: 1.0, y: 1.0 } }.domain(), Domain::SymbolEditor);
    }

    #[test]
    fn put_verbs_deserialize_with_overwrite_defaulting_to_false() {
        let json = serde_json::json!({ "op": "put_library_symbol", "symbol": serde_json::to_value(symbol_with_pins("eda:A")).unwrap() });
        match serde_json::from_value::<Cmd>(json).unwrap() {
            Cmd::PutLibrarySymbol { overwrite, symbol } => {
                assert!(!overwrite);
                assert_eq!(symbol.lib_id, "eda:A");
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn put_library_footprint_stores_a_copy_with_fresh_ids_and_refuses_a_taken_name() {
        let m = ConstraintModel::default();
        let mut b = board(&m);
        let mut fp = footprint_with_pads("Lib:Copy");
        fp.published = true; // a pasted/duplicated footprint never arrives already following the board
        b.apply(&Cmd::PutLibraryFootprint { footprint: fp.clone(), overwrite: false }).unwrap();
        let stored = b.design().footprint_library.as_ref().unwrap().by_name("Lib:Copy").unwrap();
        assert_eq!(stored.pads.len(), 2);
        assert!(stored.pads.iter().all(|p| !p.id.is_empty()), "ids are assigned here");
        assert_ne!(stored.pads[0].id, stored.pads[1].id);
        assert!(!stored.published);

        let e = b.apply(&Cmd::PutLibraryFootprint { footprint: fp.clone(), overwrite: false }).unwrap_err();
        assert_eq!(e[0].check, "ops_footprint_exists");

        // a name the builtin table already answers to is taken as well
        let builtin = eda_model::footprint::builtin("0402").map(|f| f.name).unwrap_or_else(|| "0402".into());
        let mut clash = footprint_with_pads(&builtin);
        clash.name = builtin;
        assert_eq!(b.apply(&Cmd::PutLibraryFootprint { footprint: clash.clone(), overwrite: false }).unwrap_err()[0].check, "ops_footprint_exists");
        b.apply(&Cmd::PutLibraryFootprint { footprint: clash, overwrite: true }).expect("overwrite shadows the builtin with a project entry");
    }

    #[test]
    fn put_library_footprint_overwrite_keeps_the_published_flag_and_refuses_bad_names() {
        let m = ConstraintModel::default();
        let mut b = board(&m);
        b.apply(&Cmd::PutLibraryFootprint { footprint: footprint_with_pads("X"), overwrite: false }).unwrap();
        b.apply(&Cmd::UpdateFootprintOnBoard { name: "X".into() }).unwrap();
        let mut replacement = footprint_with_pads("X");
        replacement.pads.truncate(1);
        b.apply(&Cmd::PutLibraryFootprint { footprint: replacement, overwrite: true }).unwrap();
        let stored = b.design().footprint_library.as_ref().unwrap().by_name("X").unwrap();
        assert_eq!(stored.pads.len(), 1, "replaced, not appended");
        assert!(stored.published, "an overwritten entry keeps following the board if it already did");
        assert_eq!(b.design().footprint_library.as_ref().unwrap().footprints.len(), 1);

        for bad in ["", "Lib:", "a/b", "50%", "a\"b"] {
            let mut f = footprint_with_pads("ok");
            f.name = bad.into();
            let e = b.apply(&Cmd::PutLibraryFootprint { footprint: f, overwrite: false }).unwrap_err();
            assert!(e[0].check == "ops_bad_name" || e[0].check == "ops_bad_footprint", "{bad:?}: {}", e[0].check);
        }
    }

    #[test]
    fn rename_library_footprint_moves_the_entry_and_guards_the_new_name() {
        let m = ConstraintModel::default();
        let mut b = board(&m);
        b.apply(&Cmd::PutLibraryFootprint { footprint: footprint_with_pads("Old"), overwrite: false }).unwrap();
        b.apply(&Cmd::PutLibraryFootprint { footprint: footprint_with_pads("Other"), overwrite: false }).unwrap();
        b.apply(&Cmd::RenameLibraryFootprint { name: "Old".into(), new_name: "Old".into(), overwrite: false }).expect("renaming to the same name is accepted without change");
        assert_eq!(b.apply(&Cmd::RenameLibraryFootprint { name: "Old".into(), new_name: "Other".into(), overwrite: false }).unwrap_err()[0].check, "ops_footprint_exists");
        assert_eq!(b.apply(&Cmd::RenameLibraryFootprint { name: "Nope".into(), new_name: "Z".into(), overwrite: false }).unwrap_err()[0].check, "ops_unknown_footprint");
        b.apply(&Cmd::RenameLibraryFootprint { name: "Old".into(), new_name: "New".into(), overwrite: false }).unwrap();
        let lib = b.design().footprint_library.as_ref().unwrap();
        assert!(lib.by_name("Old").is_none());
        assert_eq!(lib.by_name("New").unwrap().pads.len(), 2);
        // the dialog's "Overwrite": the footprint that held the name is replaced
        b.apply(&Cmd::RenameLibraryFootprint { name: "New".into(), new_name: "Other".into(), overwrite: true }).unwrap();
        let lib = b.design().footprint_library.as_ref().unwrap();
        assert_eq!(lib.footprints.len(), 1);
        assert_eq!(lib.footprints[0].name, "Other");
    }

    #[test]
    fn repair_footprint_gives_a_repeated_id_a_new_one_and_keeps_the_first_holder() {
        let m = ConstraintModel::default();
        let mut b = board(&m);
        b.apply(&Cmd::PutLibraryFootprint { footprint: footprint_with_pads("R"), overwrite: false }).unwrap();
        {
            let fp = b.library_footprint_mut("R").unwrap();
            let first = fp.pads[0].id.clone();
            fp.pads[1].id = first; // two pads with one id, as a hand-edited design.json might hold
        }
        let dup = b.design().footprint_library.as_ref().unwrap().by_name("R").unwrap().pads[0].id.clone();
        b.apply(&Cmd::RepairFootprint { name: "R".into() }).unwrap();
        let fp = b.design().footprint_library.as_ref().unwrap().by_name("R").unwrap();
        assert_eq!(fp.pads[0].id, dup, "the first pad to hold the id keeps it");
        assert!(!fp.pads[1].id.is_empty());
        assert_ne!(fp.pads[1].id, dup);
        assert_eq!(b.apply(&Cmd::RepairFootprint { name: "missing".into() }).unwrap_err()[0].check, "ops_unknown_footprint");
    }

    #[test]
    fn put_library_symbol_and_rename_follow_the_same_naming_rules() {
        let m = ConstraintModel::default();
        let mut b = board(&m);
        b.apply(&Cmd::PutLibrarySymbol { symbol: symbol_with_pins("eda:Copy"), overwrite: false }).unwrap();
        let stored = b.design().symbol_library.as_ref().unwrap().by_lib_id("eda:Copy").unwrap();
        assert!(stored.pins.iter().all(|p| !p.id.is_empty()) && stored.graphics.iter().all(|g| !g.id().is_empty()));
        assert_eq!(b.apply(&Cmd::PutLibrarySymbol { symbol: symbol_with_pins("eda:Copy"), overwrite: false }).unwrap_err()[0].check, "ops_symbol_exists");
        // a builtin library symbol's id is taken
        assert_eq!(b.apply(&Cmd::PutLibrarySymbol { symbol: symbol_with_pins("Device:R"), overwrite: false }).unwrap_err()[0].check, "ops_symbol_exists");
        for bad in ["", "eda:", "eda:a:b", "eda:a\"b", "a\\b:c"] {
            let mut s = symbol_with_pins("eda:ok");
            s.lib_id = bad.into();
            let e = b.apply(&Cmd::PutLibrarySymbol { symbol: s, overwrite: false }).unwrap_err();
            assert!(e[0].check == "ops_bad_name" || e[0].check == "ops_bad_symbol", "{bad:?}: {}", e[0].check);
        }

        b.apply(&Cmd::RenameLibrarySymbol { lib_id: "eda:Copy".into(), new_lib_id: "eda:Renamed".into(), overwrite: false }).unwrap();
        let lib = b.design().symbol_library.as_ref().unwrap();
        assert!(lib.by_lib_id("eda:Copy").is_none());
        assert_eq!(lib.by_lib_id("eda:Renamed").unwrap().pins.len(), 2);
        assert_eq!(b.apply(&Cmd::RenameLibrarySymbol { lib_id: "eda:Nope".into(), new_lib_id: "eda:Z".into(), overwrite: false }).unwrap_err()[0].check, "ops_unknown_symbol");
        b.apply(&Cmd::PutLibrarySymbol { symbol: symbol_with_pins("eda:Other"), overwrite: false }).unwrap();
        assert_eq!(b.apply(&Cmd::RenameLibrarySymbol { lib_id: "eda:Renamed".into(), new_lib_id: "eda:Other".into(), overwrite: false }).unwrap_err()[0].check, "ops_symbol_exists");
        b.apply(&Cmd::RenameLibrarySymbol { lib_id: "eda:Renamed".into(), new_lib_id: "eda:Other".into(), overwrite: true }).unwrap();
        assert_eq!(b.design().symbol_library.as_ref().unwrap().symbols.len(), 1);
    }

    #[test]
    fn set_symbol_anchor_shifts_every_pin_and_graphic_so_the_click_becomes_the_origin() {
        let m = ConstraintModel::default();
        let mut b = board(&m);
        b.apply(&Cmd::PutLibrarySymbol { symbol: symbol_with_pins("eda:A"), overwrite: false }).unwrap();
        b.apply(&Cmd::SetSymbolAnchor { lib_id: "eda:A".into(), at: SPoint { x: 2.54, y: 5.08 } }).unwrap();
        let sym = b.design().symbol_library.as_ref().unwrap().by_lib_id("eda:A").unwrap();
        let ys: Vec<f64> = sym.pins.iter().map(|p| p.at.y).collect();
        assert!((ys[0] - 0.0).abs() < 1e-9 && (ys[1] - (-10.16)).abs() < 1e-9, "{ys:?}");
        assert!(sym.pins.iter().all(|p| (p.at.x + 2.54).abs() < 1e-9));
        match &sym.graphics[0] {
            LibrarySymbolGraphic::Rectangle { start, end, .. } => {
                assert!((start.x - (-5.08)).abs() < 1e-9 && (start.y - (-8.89)).abs() < 1e-9);
                assert!((end.x - 0.0).abs() < 1e-9 && (end.y - (-1.27)).abs() < 1e-9);
            }
            other => panic!("{other:?}"),
        }
        assert_eq!(b.apply(&Cmd::SetSymbolAnchor { lib_id: "eda:Nope".into(), at: SPoint { x: 0.0, y: 0.0 } }).unwrap_err()[0].check, "ops_unknown_symbol");
    }

    #[test]
    fn push_pin_length_reaches_shared_and_shown_body_style_pins_only() {
        let m = ConstraintModel::default();
        let mut b = board(&m);
        let mut sym = symbol_with_pins("eda:Styles");
        sym.has_alternate_body_style = true;
        // pin 3: body style 2 (the alternate one), pin 4: shared by every style (0)
        let mut p3 = sym.pins[0].clone();
        p3.number = "3".into();
        p3.body_style = 2;
        p3.length_mm = 1.0;
        let mut p4 = sym.pins[0].clone();
        p4.number = "4".into();
        p4.body_style = 0;
        p4.length_mm = 1.0;
        sym.pins[1].length_mm = 5.0; // pin 2 is the source: 5.0 mm
        sym.pins.push(p3);
        sym.pins.push(p4);
        for p in &mut sym.pins {
            p.id.clear();
        }
        sym.assign_missing_ids();
        b.apply(&Cmd::PutLibrarySymbol { symbol: sym, overwrite: false }).unwrap();
        let src = b.design().symbol_library.as_ref().unwrap().by_lib_id("eda:Styles").unwrap().pins.iter().find(|p| p.number == "2").unwrap().id.clone();
        b.apply(&Cmd::PushPinProperty { lib_id: "eda:Styles".into(), source_pin_id: src, field: crate::PushPinField::Length, body_style: Some(1) }).unwrap();
        let s = b.design().symbol_library.as_ref().unwrap().by_lib_id("eda:Styles").unwrap();
        let len = |n: &str| s.pins.iter().find(|p| p.number == n).unwrap().length_mm;
        assert_eq!(len("1"), 5.0, "same body style");
        assert_eq!(len("4"), 5.0, "a pin shared by every body style is reached too");
        assert_eq!(len("3"), 1.0, "a pin of the other body style is not");
    }
}
