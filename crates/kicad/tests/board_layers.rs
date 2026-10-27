//! Board Editor Layers through the importer and the writer: the `(layers ..)` of a board file -- the technical and user layers it has, the names it shows for its layers and
//! their types (`PCB_IO_KICAD_SEXPR_PARSER::parseLayers`) -- come in as `eda_model::layers::LayerSetup`, go back out as `formatBoardLayers` writes them, and a board that never
//! had the page written is the board KiCad starts: every standard technical layer. No external tool is needed here; the proof against kicad-cli is in `eda-cli`'s board tests.

use eda_model::units::mm;
use eda_model::units::Nm;
use eda_kicad::{export_kicad_pcb, import_kicad_pcb, ExportMeta};
use eda_model::ir::{Design, DrawingsSection, Shape, Point};
use eda_model::layers::{LayerSetup, LayerType, TECH_LAYERS};
use eda_model::ConstraintModel;

const BOARD: &str = r#"(kicad_pcb (version 20241229) (generator "pcbnew") (generator_version "9.0")
	(general (thickness 1.6) (legacy_teardrops no))
	(paper "A4")
	(layers
		(0 "F.Cu" signal)
		(4 "In1.Cu" power "GND plane")
		(6 "In2.Cu" mixed)
		(2 "B.Cu" jumper)
		(9 "F.Adhes" user "F.Adhesive")
		(5 "F.SilkS" user "Top art")
		(1 "F.Mask" user)
		(17 "Dwgs.User" user "User.Drawings")
		(25 "Edge.Cuts" user)
		(27 "Margin" user)
		(31 "F.CrtYd" user "F.Courtyard")
		(29 "B.CrtYd" user "B.Courtyard")
		(39 "User.1" user "Notes")
		(41 "User.2" front)
		(43 "User.3" back "Rear")
		(45 "User.4" auxiliary)
		(47 "User.5" user)
	)
	(setup (pad_to_mask_clearance 0))
	(net 0 "")
)
"#;

fn export(d: &Design, model: &ConstraintModel) -> String {
    export_kicad_pcb(d, model, &ExportMeta { date: "2026-01-01", title: "layers" }).expect("export_kicad_pcb")
}

fn layers_section(text: &str) -> Vec<String> {
    let start = text.find("\t(layers\n").expect("a layers section");
    let end = text[start..].find("\n\t)\n").expect("its end") + start;
    text[start..end].lines().skip(1).map(|l| l.trim().to_string()).collect()
}

#[test]
fn the_layers_of_a_board_file_come_in_as_the_page_holds_them() {
    let (_, model, _) = import_kicad_pcb(BOARD).expect("imports");
    let setup = &model.board.layer_setup;
    assert_eq!(model.board.layers, ["F.Cu", "In1.Cu", "In2.Cu", "B.Cu"]);
    assert_eq!(setup.names, [("In1.Cu", "GND plane"), ("F.SilkS", "Top art"), ("User.1", "Notes"), ("User.3", "Rear")].into_iter().map(|(a, b)| (a.to_string(), b.to_string())).collect(), "a name that is KiCad's own for the layer is no name");
    assert_eq!(setup.types.len(), 5, "{:?}", setup.types);
    assert_eq!((setup.layer_type("F.Cu"), setup.layer_type("In1.Cu"), setup.layer_type("In2.Cu"), setup.layer_type("B.Cu")), (LayerType::Signal, LayerType::Power, LayerType::Mixed, LayerType::Jumper));
    assert_eq!((setup.layer_type("User.1"), setup.layer_type("User.2"), setup.layer_type("User.3"), setup.layer_type("User.4"), setup.layer_type("User.5")), (LayerType::Auxiliary, LayerType::Front, LayerType::Back, LayerType::Auxiliary, LayerType::Auxiliary));
    assert_eq!(setup.enabled[setup.enabled.len() - 5..], ["User.1", "User.2", "User.3", "User.4", "User.5"]);
    for on in ["F.Adhes", "F.SilkS", "F.Mask", "Edge.Cuts", "F.CrtYd", "B.CrtYd", "Margin", "Dwgs.User"] {
        assert!(setup.enabled.iter().any(|l| l == on), "{on} is on: {:?}", setup.enabled);
    }
    // `BOARD_DESIGN_SETTINGS::SetEnabledLayers` adds only the mandatory layers to the file's list (r2-board-io F17).
    for off in ["B.Adhes", "F.Paste", "B.Paste", "B.SilkS", "B.Mask", "F.Fab", "B.Fab", "Cmts.User", "Eco1.User", "Eco2.User"] {
        assert!(!setup.enabled.iter().any(|l| l == off), "{off} was not in the file and nothing sits on it");
    }
    assert!(setup.validate(&model.board.layers).is_ok(), "what is imported is a page the verb would accept");
}

#[test]
fn the_writer_emits_formatboardlayers_and_the_file_comes_back_the_same() {
    let (design, model, _) = import_kicad_pcb(BOARD).expect("imports");
    let text = export(&design, &model);
    let layers = layers_section(&text);
    assert_eq!(layers[..4], ["(0 \"F.Cu\" signal)", "(1 \"In1.Cu\" power \"GND plane\")", "(2 \"In2.Cu\" mixed)", "(31 \"B.Cu\" jumper)"]);
    for line in [
        "(33 \"F.Adhes\" user \"F.Adhesive\")",
        "(37 \"F.SilkS\" user \"Top art\")",
        "(39 \"F.Mask\" user)",
        "(40 \"Dwgs.User\" user \"User.Drawings\")",
        "(44 \"Edge.Cuts\" user)",
        "(45 \"Margin\" user)",
        "(50 \"User.1\" user \"Notes\")",
        "(51 \"User.2\" front)",
        "(52 \"User.3\" back \"Rear\")",
        "(53 \"User.4\" user)",
        "(54 \"User.5\" user)",
    ] {
        assert!(layers.iter().any(|l| l == line), "{line} is in\n{}", layers.join("\n"));
    }
    // The technical layers come in KiCad's order (`TechAndUserUIOrder`), the user layers after them by number.
    let names: Vec<&str> = layers.iter().skip(4).map(|l| l.split('"').nth(1).unwrap()).collect();
    let mut expect: Vec<&str> = TECH_LAYERS.iter().copied().filter(|l| names.contains(l)).collect();
    expect.extend(["User.1", "User.2", "User.3", "User.4", "User.5"]);
    assert_eq!(names, expect);

    let (_, again, _) = import_kicad_pcb(&text).expect("re-imports");
    assert_eq!(again.board.layer_setup, model.board.layer_setup, "what the writer wrote is what the importer reads");
    assert_eq!(again.board.layers, model.board.layers);
}

#[test]
fn a_board_nobody_set_up_is_the_board_kicad_starts() {
    let (design, model, _) = import_kicad_pcb(r#"(kicad_pcb (version 20241229) (generator "pcbnew") (layers (0 "F.Cu" signal) (2 "B.Cu" signal) (25 "Edge.Cuts" user)) (net 0 ""))"#).expect("imports");
    let layers = layers_section(&export(&design, &model));
    assert_eq!(layers.len(), 2 + 4, "a file that lists three layers has those and the mandatory ones (the courtyards, the outline, the margin), as KiCad has it: {layers:?}");

    let mut m = ConstraintModel::default();
    m.board.layers = vec!["F.Cu".into(), "B.Cu".into()];
    let d = eda_model::ir::Design { schema: eda_model::ir::SCHEMA_VERSION, provenance: eda_model::ir::Provenance { engine_version: "0".into(), intent_hash: "x".into(), seed: 0, stage_hashes: vec![] }, schematic: None, nets: None, placement: Some(eda_model::ir::PlacementSection { outline: vec![Point { x: Nm::ZERO, y: Nm::ZERO }, Point { x: mm(0.01), y: Nm::ZERO }, Point { x: mm(0.01), y: mm(0.01) }, Point { x: Nm::ZERO, y: mm(0.01) }], footprints: vec![], modules: vec![] }), routing: None, drawings: None, footprint_library: None, sheet_contents: None, bus_aliases: vec![], symbol_library: None };
    let layers = layers_section(&export(&d, &m));
    assert_eq!(layers.len(), 2 + 18, "a board with no page is the 18 technical layers KiCad starts a board with");
    assert!(layers.iter().any(|l| l == "(40 \"Dwgs.User\" user \"User.Drawings\")") && layers.iter().any(|l| l == "(45 \"Margin\" user)"));
    assert!(LayerSetup::default().is_default());
}

#[test]
fn a_layer_the_items_are_on_is_a_layer_the_board_has() {
    let text = r#"(kicad_pcb (version 20241229) (generator "pcbnew") (layers (0 "F.Cu" signal) (2 "B.Cu" signal) (25 "Edge.Cuts" user))
	(gr_line (start 1 1) (end 5 1) (stroke (width 0.1) (type default)) (layer "Cmts.User") (uuid "00000000-0000-0000-0000-000000000001"))
	(gr_line (start 1 2) (end 5 2) (stroke (width 0.1) (type default)) (layer "User.7") (uuid "00000000-0000-0000-0000-000000000002"))
	(net 0 ""))"#;
    let (design, model, _) = import_kicad_pcb(text).expect("imports");
    assert!(model.board.layer_setup.enabled.iter().any(|l| l == "Cmts.User") && model.board.layer_setup.enabled.iter().any(|l| l == "User.7"), "{:?}", model.board.layer_setup.enabled);
    assert!(design.drawings.as_ref().is_some_and(|d: &DrawingsSection| d.shapes.iter().any(|s: &Shape| s.layer() == "User.7")));
    let layers = layers_section(&export(&design, &model));
    assert!(layers.iter().any(|l| l == "(56 \"User.7\" user)"), "{layers:?}");
}

/// r3-board-io F13: a file from before 20200922 has no user names (`parseLayer`: `m_name = m_userName = name`), so its technical layers are shown by the file's names
/// (`B.Adhes`, not `B.Adhesive`) and written so that a reader of our file shows them the same (`custom_pads`, `issue4139`, `issue4257`).
#[test]
fn a_legacy_boards_layers_keep_their_file_names() {
    let (design, model, _) = import_kicad_pcb(r#"(kicad_pcb (version 20200829) (generator pcbnew) (layers (0 "F.Cu" signal) (31 "B.Cu" signal) (32 "B.Adhes" user) (37 "F.SilkS" user) (44 "Edge.Cuts" user)) (net 0 ""))"#).expect("imports");
    assert_eq!(model.board.layer_setup.display_name("B.Adhes"), "B.Adhes");
    assert_eq!(model.board.layer_setup.display_name("F.SilkS"), "F.SilkS");
    let layers = layers_section(&export(&design, &model));
    assert!(layers.iter().any(|l| l == "(32 \"B.Adhes\" user \"B.Adhes\")"), "{layers:?}");
    assert!(layers.iter().any(|l| l == "(44 \"Edge.Cuts\" user)"), "a layer whose standard name is its file name needs none: {layers:?}");
}
