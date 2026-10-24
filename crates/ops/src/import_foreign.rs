//! File > Import > Non-KiCad Board File... and Non-KiCad Schematic...: the two verbs that put what kicad-cli made of another format's file in place of what is open.
//!
//! # What KiCad does, and what this does
//!
//! `BOARD_EDITOR_CONTROL::OpenNonKicadBoard` (`pcbnew/files.cpp`) asks for a file and calls `PCB_EDIT_FRAME::OpenProjectFiles( .., KICTL_NONKICAD_ONLY )`, which, after asking
//! whether to save a modified board, clears the board (`Clear_Pcb`), loads the other board with the plugin of its format and puts it where the open one was: in the project
//! manager the project stays, and the board keeps the name of the file it replaced. It *replaces* the open board; it does not add to it (that is Append Board,
//! `PCB_CONTROL::AppendBoard`, which [`Cmd::PasteClipboard`](crate::Cmd::PasteClipboard) is). `SCH_EDIT_FRAME::OnImportProject` (`eeschema/files-io.cpp`) is the same for
//! the schematic: "This operation replaces the contents of the current schematic, which will be permanently lost" -- and it clears the undo list, because the old schematic
//! cannot be had back there. Here both are one command, so one undo step, and an undo does bring the old board or schematic back.
//!
//! * [`Cmd::ReplaceBoard`](crate::Cmd::ReplaceBoard): the board half of the design -- placement, routing, drawings -- is the file's. The footprints, nets and rules of the file are
//!   a model of their own, and the design names the file that holds it (`DrawingsSection::intent_file`; the server writes the file before the command runs, from the same text),
//!   so what the intent said about the old board is not in the new one: an undo restores the design and the model with it. The schematic, the sheets and the libraries of the
//!   editors stay, as KiCad does not touch the other editor's file; `design.nets` goes, so the board's own nets (the file's pads) are the netlist until the schematic is edited.
//! * [`Cmd::ReplaceSchematic`](crate::Cmd::ReplaceSchematic): the schematic -- the root sheet, every sheet it places, the nets traced from them and the library symbols the
//!   old drawing published -- is the file's; the library symbols come in under the names `merge_lib_symbols` settles, as a paste's do. The board is not touched.

use super::Board;
use eda_model::sheet_file::SheetFile;
use eda_model::CheckResult;

fn fail(check: &str, what: &str, msg: impl Into<String>) -> Vec<CheckResult> {
    vec![CheckResult::fail(check, what, msg.into())]
}

/// The file, under the board's folder, that holds the model of a board imported from the text whose hash is `intent_hash` (`Provenance::intent_hash` of the design
/// `eda_kicad::import_kicad_pcb` makes of it). Content-addressed: the same board is the same file, and a redo finds it where the first run left it. `keep_kicad_layer_names`
/// (the dialog's "Keep KiCad layer names") makes another model of the same text -- one whose layers show their standard names -- and so does the project the importer wrote with
/// the board (`project`: its design rules and net classes), so each is another file.
pub fn imported_intent_file(intent_hash: &str, keep_kicad_layer_names: bool, project: Option<&str>) -> String {
    // FNV-1a over the project's text: a name for a file, not a defence against anyone.
    let project = project.map(|p| format!(".p{:08x}", p.bytes().fold(0xcbf2_9ce4_8422_2325_u64, |h, b| (h ^ u64::from(b)).wrapping_mul(0x100_0000_01b3)) as u32)).unwrap_or_default();
    format!("imports/{}{}{project}.intent.yaml", &intent_hash[..intent_hash.len().min(24)], if keep_kicad_layer_names { ".kicad-names" } else { "" })
}

impl<'a> Board<'a> {
    /// `Cmd::ReplaceBoard`: see the module doc. `text` is the `.kicad_pcb` kicad-cli wrote.
    pub(crate) fn replace_board(&mut self, text: &str, name: &str, keep_kicad_layer_names: bool, project: Option<&str>) -> Result<(), Vec<CheckResult>> {
        let (mut board, _model, _notes) = eda_kicad::import_kicad_pcb(text).map_err(|e| fail("ops_bad_board", name, format!("'{name}' did not convert to a board this reader takes: {}", e.iter().filter_map(|c| c.hint.clone()).collect::<Vec<_>>().join("; "))))?;
        // What stays is the other editor's: the schematic and its sheets, the libraries the editors keep, the bus aliases. The nets the schematic's drawing set are the old
        // board's connectivity: they would override the pads of the board that comes in.
        let old = &mut self.design;
        board.schematic = old.schematic.take();
        board.sheet_contents = old.sheet_contents.take();
        board.symbol_library = old.symbol_library.take();
        board.footprint_library = old.footprint_library.take();
        board.bus_aliases = std::mem::take(&mut old.bus_aliases);
        board.nets = None;
        let file = imported_intent_file(&board.provenance.intent_hash, keep_kicad_layer_names, project);
        board.drawings.get_or_insert_with(Default::default).intent_file = Some(file);
        self.design = board;
        Ok(())
    }

    /// `Cmd::ReplaceSchematic`: see the module doc. `content` is the converted file read by `eda_kicad::load_sheet_file`.
    pub(crate) fn replace_schematic(&mut self, content: &SheetFile, name: &str) -> Result<(), Vec<CheckResult>> {
        let nothing = content.root.symbols.is_empty()
            && content.root.power_symbols.is_empty()
            && content.root.wires.is_empty()
            && content.root.labels.is_empty()
            && content.root.sheets.is_empty()
            && content.screens.values().all(|s| s.symbols.is_empty() && s.wires.is_empty() && s.labels.is_empty() && s.sheets.is_empty());
        if nothing {
            return Err(fail("ops_empty_schematic", name, format!("'{name}' holds no schematic: no symbol, no wire, no label, no sheet")));
        }
        // The symbols the old drawing published go with it; a definition the Symbol Editor holds unpublished stays.
        if let Some(lib) = self.design.symbol_library.as_mut() {
            lib.symbols.retain(|s| !s.published);
        }
        let mut wanted: Vec<String> = content.all_screens().flat_map(|(_, s)| s.symbols.iter().map(|x| x.lib_id.clone()).chain(s.power_symbols.iter().map(|p| p.lib_id.clone()))).collect();
        wanted.sort_unstable();
        wanted.dedup();
        let merge = self.merge_lib_symbols(&wanted, &content.lib_symbols);
        let rename = |lib_id: &mut String| {
            if let Some(now) = merge.lib_name.get(lib_id.as_str()) {
                if now != lib_id {
                    *lib_id = now.clone();
                }
            }
        };
        let mut root = content.root.clone();
        let mut screens = content.screens.clone();
        for sec in std::iter::once(&mut root).chain(screens.values_mut()) {
            for s in &mut sec.symbols {
                rename(&mut s.lib_id);
            }
            for p in &mut sec.power_symbols {
                rename(&mut p.lib_id);
            }
            sec.assign_missing_ids();
        }
        if !merge.publish.is_empty() {
            let lib = self.design.symbol_library.get_or_insert_with(Default::default);
            lib.symbols.extend(merge.publish);
            lib.symbols.sort_by(|a, b| a.lib_id.cmp(&b.lib_id));
        }
        if self.design.symbol_library.as_ref().is_some_and(|l| l.symbols.is_empty()) {
            self.design.symbol_library = None;
        }
        self.design.schematic = Some(root);
        self.design.sheet_contents = (!screens.is_empty()).then_some(screens);
        // The nets of the old drawing are not the new one's: the server traces the new drawing against nothing (`reconcile_schematic`).
        self.design.nets = Some(Vec::new());
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Cmd, Domain};
    use eda_model::ir::{Design, FootprintInstance, PlacementSection, Point, Provenance, Side};
    use eda_model::ConstraintModel;
    use serde_json::json;

    fn design() -> Design {
        serde_json::from_value(json!({
            "schema": 1,
            "provenance": { "engine_version": "0", "intent_hash": "old", "seed": 0 },
            "placement": { "outline": [{ "x": 0, "y": 0 }, { "x": 50000, "y": 0 }, { "x": 50000, "y": 50000 }, { "x": 0, "y": 50000 }], "footprints": [{ "id": "J9", "at": { "x": 10000, "y": 10000 }, "rot": 0, "side": "top" }] },
            "nets": [{ "name": "OLD", "pins": ["J9.1", "J9.2"] }],
        }))
        .unwrap()
    }

    /// A board as kicad-cli 10.99 writes one: the nets named where they are used, a footprint with two pads, a track, a via and a pour on a net no pad is on.
    const FOREIGN_BOARD: &str = r#"(kicad_pcb (version 20260901) (generator "pcbnew") (generator_version "10.99")
        (general (thickness 1.6) (legacy_teardrops no)) (paper "A4")
        (layers (0 "F.Cu" signal) (2 "B.Cu" signal) (25 "Edge.Cuts" user))
        (footprint "eagle-import-fps:0603" (layer "F.Cu") (uuid "11111111-1111-1111-1111-111111111111") (at 20 20)
          (property "Reference" "R1" (at 0 -1 0) (layer "F.SilkS") (uuid "11111111-1111-1111-1111-111111111112") (effects (font (size 1 1) (thickness 0.15))))
          (property "Value" "10k" (at 0 1 0) (layer "F.Fab") (uuid "11111111-1111-1111-1111-111111111113") (effects (font (size 1 1) (thickness 0.15))))
          (pad "1" smd rect (at -0.8 0) (size 0.8 0.9) (layers "F.Cu" "F.Mask" "F.Paste") (net "VCC") (uuid "11111111-1111-1111-1111-111111111114"))
          (pad "2" smd rect (at 0.8 0) (size 0.8 0.9) (layers "F.Cu" "F.Mask" "F.Paste") (net "GND") (uuid "11111111-1111-1111-1111-111111111115")))
        (segment (start 19.2 20) (end 12 20) (width 0.25) (layer "F.Cu") (net "VCC") (uuid "22222222-2222-2222-2222-222222222221"))
        (via (at 12 20) (size 0.8) (drill 0.4) (layers "F.Cu" "B.Cu") (net "VCC") (uuid "33333333-3333-3333-3333-333333333331"))
        (zone (net "SHIELD") (layer "B.Cu") (uuid "44444444-4444-4444-4444-444444444441") (hatch edge 0.5) (polygon (pts (xy 0 0) (xy 40 0) (xy 40 40) (xy 0 40))))
        (gr_line (start 0 0) (end 40 0) (stroke (width 0.1) (type solid)) (layer "Edge.Cuts") (uuid "55555555-5555-5555-5555-555555555551"))
        (gr_line (start 40 0) (end 40 40) (stroke (width 0.1) (type solid)) (layer "Edge.Cuts") (uuid "55555555-5555-5555-5555-555555555552"))
        (gr_line (start 40 40) (end 0 40) (stroke (width 0.1) (type solid)) (layer "Edge.Cuts") (uuid "55555555-5555-5555-5555-555555555553"))
        (gr_line (start 0 40) (end 0 0) (stroke (width 0.1) (type solid)) (layer "Edge.Cuts") (uuid "55555555-5555-5555-5555-555555555554"))
    )"#;

    #[test]
    fn replacing_the_board_swaps_placement_routing_and_drawings_and_names_the_model_file_but_keeps_the_other_editors_half() {
        let m = ConstraintModel::default();
        let mut d = design();
        d.schematic = Some(Default::default());
        d.drawings = Some(eda_model::ir::DrawingsSection { page: Some(Default::default()), ..Default::default() });
        d.symbol_library = Some(Default::default());
        let mut b = Board::new(d, &m, 100_000, 300_000);
        let cmd = Cmd::ReplaceBoard { text: FOREIGN_BOARD.into(), name: "x.brd".into(), keep_kicad_layer_names: false, project: None };
        assert!(cmd.replaces_board() && cmd.spans_both_editors() && !cmd.clears_routing(), "an import is judged by nobody, takes the netlist the drawing set with it, and keeps the copper it brings");
        b.apply(&cmd).unwrap();
        let d = b.design();
        let placed: Vec<&str> = d.placement.as_ref().unwrap().footprints.iter().map(|f| f.id.as_str()).collect();
        assert_eq!(placed, vec!["R1"], "the old board's J9 is gone and the file's R1 is there");
        let routing = d.routing.as_ref().unwrap();
        assert_eq!((routing.tracks.len(), routing.vias.len(), routing.zones.len()), (1, 1, 1));
        assert!(d.schematic.is_some() && d.symbol_library.is_some(), "the schematic and the libraries are the other editor's");
        assert!(d.nets.is_none(), "the netlist the old drawing set would override the pads of the board that came in");
        // The model is a file of its own, named by the hash of the text.
        let file = d.drawings.as_ref().unwrap().intent_file.clone().unwrap();
        assert_eq!(file, imported_intent_file(&d.provenance.intent_hash, false, None));
        assert_ne!(file, imported_intent_file(&d.provenance.intent_hash, true, None), "the same text with its layers named the standard way is another model");
        assert_ne!(file, imported_intent_file(&d.provenance.intent_hash, false, Some("{}")), "and so is the same text with the rules and net classes its importer wrote");
        assert_eq!(imported_intent_file(&d.provenance.intent_hash, false, Some("{}")), imported_intent_file(&d.provenance.intent_hash, false, Some("{}")));
        assert!(file.starts_with("imports/") && file.ends_with(".intent.yaml"), "{file}");
        assert_eq!(d.provenance.intent_hash.len(), 64, "the blake3 hash of the text");
        // What the old design's drawings held (its page) is the old board's.
        assert!(d.drawings.as_ref().unwrap().page.is_none());
        // The same text is the same file: a redo finds it where the first run put it.
        let again = Board::new(design(), &m, 100_000, 300_000);
        let mut again = again;
        again.apply(&cmd).unwrap();
        assert_eq!(again.design().drawings.as_ref().unwrap().intent_file.as_ref(), Some(&file));
        // Not a board: refused, nothing changes.
        let before = serde_json::to_string(b.design()).unwrap();
        let e = b.apply(&Cmd::ReplaceBoard { text: "hello".into(), name: "y.brd".into(), keep_kicad_layer_names: false, project: None }).unwrap_err();
        assert_eq!(e[0].check, "ops_bad_board");
        assert_eq!(serde_json::to_string(b.design()).unwrap(), before);
        let _ = (Side::Top, Point { x: 0, y: 0 }, Provenance { engine_version: String::new(), intent_hash: String::new(), seed: 0, stage_hashes: vec![] }, FootprintInstance { id: String::new(), at: Point { x: 0, y: 0 }, rot: 0, side: Side::Top, label: Default::default() }, PlacementSection { outline: vec![], footprints: vec![], modules: vec![] });
    }

    /// The mcu30 example drawn as module sheets, written as a KiCad project in a scratch folder and read as a file: a root that places three sheets, a screen each, the library
    /// symbols of the files.
    fn a_file() -> SheetFile {
        let dir = std::env::temp_dir().join(format!("eda_ops_replace_schematic_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/mcu_board_30plus.yaml");
        let model: ConstraintModel = serde_yaml::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        let design = eda_engine::derive_schematic_modules(&model, &eda_engine::EngineOptions::new(1, "t")).unwrap();
        let tree = eda_kicad::export_kicad_sch_tree(&design, &model, &eda_kicad::ExportMeta { date: "2026-10-10", title: "t" }, "whole.kicad_sch").unwrap();
        for (file, text) in &tree {
            std::fs::write(dir.join(file), text).unwrap();
        }
        let file = eda_kicad::load_sheet_file(&dir.join("whole.kicad_sch")).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
        file
    }

    #[test]
    fn replacing_the_schematic_swaps_every_sheet_publishes_the_files_symbols_and_leaves_the_board_alone() {
        let m = ConstraintModel::default();
        let mut d = design();
        // The old drawing: a root with a sheet of its own and a symbol the old drawing published (and one the Symbol Editor holds, unpublished).
        let mut old_root = eda_model::ir::SchematicSection::default();
        old_root.symbols.push(crate::sch_annotate_tests::symbol("U9", "Old:Part", "x", 1, Point { x: 0, y: 0 }));
        d.schematic = Some(old_root);
        d.sheet_contents = Some([("old.kicad_sch".to_string(), Default::default())].into_iter().collect());
        let mut lib = eda_model::ir::SymbolLibrarySection::default();
        let mut published = eda_model::ir::LibrarySymbol::from_engine_symbol(&eda_model::symbol::builtin("Device:R").unwrap());
        published.lib_id = "Old:Part".into();
        published.published = true;
        let mut draft = published.clone();
        draft.lib_id = "Mine:Draft".into();
        draft.published = false;
        lib.symbols = vec![published, draft];
        d.symbol_library = Some(lib);
        let placement_before = serde_json::to_string(&d.placement).unwrap();

        let file = a_file();
        let screens: Vec<String> = file.screens.keys().cloned().collect();
        let refs = file.references();
        let mut b = Board::new(d, &m, 100_000, 300_000);
        let cmd = Cmd::ReplaceSchematic { content: file, name: "mcu.SchDoc".into() };
        assert!(cmd.replaces_schematic() && cmd.edits_connectivity() && cmd.domain() == Domain::Schematic);
        b.apply(&cmd).unwrap();
        let d = b.design();
        let root = d.schematic.as_ref().unwrap();
        assert!(root.symbols.is_empty() && root.sheets.len() == 3 && root.imported_from_kicad, "the file's root: three sheets, in KiCad's frame");
        let mut got: Vec<String> = d.sheet_contents.as_ref().unwrap().keys().cloned().collect();
        got.sort();
        let mut want = screens;
        want.sort();
        assert_eq!(got, want, "the screens of the file, and none of the old drawing's");
        assert!(!d.sheet_contents.as_ref().unwrap().contains_key("old.kicad_sch"));
        let placed: std::collections::BTreeSet<String> = d.sheet_contents.as_ref().unwrap().values().flat_map(|s| s.symbols.iter().map(|x| x.id.clone())).collect();
        assert!(placed.contains("U1") && placed.contains("R8") && refs.iter().all(|r| placed.contains(r) || r.starts_with('#')), "{placed:?}");
        assert_eq!(d.nets.as_ref().map(Vec::len), Some(0), "the nets of the old drawing are gone; the server traces the new one against nothing");
        let lib = d.symbol_library.as_ref().unwrap();
        assert!(lib.by_lib_id("Old:Part").is_none(), "what the old drawing published goes with it");
        assert!(lib.by_lib_id("Mine:Draft").is_some(), "a definition the Symbol Editor holds stays");
        // every symbol the new drawing places is drawn from a definition the design has
        for (_, sec) in std::iter::once(("", root)).chain(d.sheet_contents.as_ref().unwrap().iter().map(|(f, s)| (f.as_str(), s))) {
            for s in &sec.symbols {
                assert!(b.design_symbol(&s.lib_id).is_some(), "{} draws from {} which nothing defines", s.id, s.lib_id);
            }
        }
        assert_eq!(serde_json::to_string(&d.placement).unwrap(), placement_before, "the board is the other editor's");

        // A file with nothing in it is refused and changes nothing.
        let before = serde_json::to_string(b.design()).unwrap();
        let e = b.apply(&Cmd::ReplaceSchematic { content: SheetFile::default(), name: "empty.sch".into() }).unwrap_err();
        assert_eq!(e[0].check, "ops_empty_schematic");
        assert_eq!(serde_json::to_string(b.design()).unwrap(), before);
    }
}
