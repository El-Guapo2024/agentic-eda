//! Round 3 (`work/findings/round3/r3-editors-exports.md` X3-1, X3-2, X3-5): the legacy tokens of a `(pcbplotparams ..)` block are read as
//! `PCB_PLOT_PARAMS_PARSER::Parse` reads them and are not written back, as `PCB_PLOT_PARAMS::Format` never writes them.

use eda_kicad::{export_kicad_pcb, import_kicad_pcb, ExportMeta};

fn legacy(items: &str) -> String {
    format!(
        r#"(kicad_pcb (version 20230620) (generator pcbnew)
 (general (thickness 1.6)) (paper "A4")
 (layers (0 "F.Cu" signal) (31 "B.Cu" signal) (38 "B.Mask" user) (39 "F.Mask" user) (44 "Edge.Cuts" user))
 (setup (pad_to_mask_clearance 0) (pcbplotparams (layerselection 0x00010fc_ffffffff) {items} (outputdirectory "")))
 (net 0 "")
)"#
    )
}

fn plot_block(text: &str) -> &str {
    let start = text.find("(pcbplotparams").expect("a pcbplotparams block");
    let end = start + text[start..].find("\n\t\t)").expect("its end");
    &text[start..end]
}

/// X3-1: `(viasonmask yes)` is the vias untented on read (`case T_pcbplotparams`), and is never written back after `(tenting ..)`: tented
/// in Board Setup, the board we write reads tented in KiCad and in our own re-import.
#[test]
fn a_legacy_viasonmask_is_not_written_back() {
    let (design, mut model, _) = import_kicad_pcb(&legacy("(viasonmask yes) (svguseinch no) (pscolor true) (plotinvisibletext no) (hpglpennumber 1) (useauxorigin yes)")).expect("parses");
    assert!(!model.board.solder_mask.tent_vias_front && !model.board.solder_mask.tent_vias_back);
    model.board.solder_mask.tent_vias_front = true;
    model.board.solder_mask.tent_vias_back = true;
    let out = export_kicad_pcb(&design, &model, &ExportMeta { date: "2026-01-01", title: "r3" }).expect("export");
    let block = plot_block(&out);
    for gone in ["viasonmask", "svguseinch", "pscolor", "plotinvisibletext", "hpglpennumber"] {
        assert!(!block.contains(gone), "{gone} written back:\n{block}");
    }
    assert!(block.contains("(useauxorigin yes)"), "{block}");
    assert!(out.contains("(tenting (front yes) (back yes))"), "{out}");
    let (_, again, _) = import_kicad_pcb(&out).expect("re-import");
    assert!(again.board.solder_mask.tent_vias_front && again.board.solder_mask.tent_vias_back);
}

/// X3-1, a board imported before the fix: a `viasonmask` already in `design.json`'s items is not written either.
#[test]
fn a_viasonmask_kept_in_design_json_is_dropped_on_write() {
    let (mut design, model, _) = import_kicad_pcb(&legacy("(useauxorigin no)")).expect("parses");
    design.drawings.as_mut().unwrap().plot_params.as_mut().unwrap().items.push("(viasonmask yes)".into());
    let out = export_kicad_pcb(&design, &model, &ExportMeta { date: "2026-01-01", title: "r3" }).expect("export");
    assert!(!plot_block(&out).contains("viasonmask"), "{out}");
}

/// X3-2: `(excludeedgelayer no)` is Edge.Cuts on "Plot on All Layers" (`case T_excludeedgelayer`); `yes` is nothing. Neither is written back.
#[test]
fn excludeedgelayer_no_plots_the_outline_on_every_layer() {
    let (design, model, _) = import_kicad_pcb(&legacy("(excludeedgelayer no)")).expect("parses");
    let plot = design.drawings.as_ref().unwrap().plot_params.clone().unwrap();
    assert_eq!(plot.plot_on_all_layers, vec!["Edge.Cuts".to_string()]);
    assert!(!plot.items.iter().any(|i| i.contains("excludeedgelayer")), "{:?}", plot.items);
    let out = export_kicad_pcb(&design, &model, &ExportMeta { date: "2026-01-01", title: "r3" }).expect("export");
    // Bit 25 is Edge.Cuts, as kicad-cli's `pcb upgrade` writes it.
    assert!(plot_block(&out).contains("(plot_on_all_layers_selection 0x00000000_00000000_00000000_02000000)"), "{out}");
    let (design, _, _) = import_kicad_pcb(&legacy("(excludeedgelayer yes)")).expect("parses");
    assert!(design.drawings.as_ref().unwrap().plot_params.as_ref().unwrap().plot_on_all_layers.is_empty());
}

/// X3-5: `(gerberprecision 4)` is clamped to 5 (`parseInt( gbrDefaultPrecision - 1, gbrDefaultPrecision )`); 6 is the default and not written.
#[test]
fn gerberprecision_is_clamped_as_kicad_reads_it() {
    let (design, _, _) = import_kicad_pcb(&legacy("(gerberprecision 4)")).expect("parses");
    assert_eq!(design.drawings.as_ref().unwrap().plot_params.as_ref().unwrap().item("gerberprecision"), Some("(gerberprecision 5)"));
    let (design, _, _) = import_kicad_pcb(&legacy("(gerberprecision 7)")).expect("parses");
    assert_eq!(design.drawings.as_ref().unwrap().plot_params.as_ref().unwrap().item("gerberprecision"), None);
}
