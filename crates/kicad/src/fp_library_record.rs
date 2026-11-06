//! A library footprint as the board holds it once it is put there (UI-LEFT PP5): what KiCad's board footprint is, a clone of the library's.
//!
//! KiCad puts a library footprint on the board whole: Place Footprint adds the footprint `PCB_BASE_FRAME::LoadFootprint` read (`PCB_CONTROL::PlaceFootprint`),
//! Update and Change Footprint exchange the board's for a clone of it (`DIALOG_EXCHANGE_FOOTPRINTS::processFootprint` -> `PCB_EDIT_FRAME::ExchangeFootprint`), and
//! Update PCB from Schematic does the same (`BOARD_NETLIST_UPDATER::replaceFootprint`). So the board's footprint has every pad's layer set, pad-to-die length and
//! fabrication property, the footprint's description, tags, jumper groups, private layers, margins, net ties, 3D body, pads with no copper, holes, dimensions, zones,
//! tables, text boxes and the rest -- what the board's file then says of it.
//!
//! The model holds a board footprint as the engine's `Footprint` (pads, art, courtyard, models) and, for what that has no place for, the record a board file's footprint
//! gets (`FootprintExtra`: the pads' file facts, the kept forms, the items ...). [`library_on_board`] makes that record of a library footprint the way the board's own
//! reader makes it of a board file's footprint: the library footprint is written as `format( const FOOTPRINT* )` writes it ([`crate::export_kicad_mod`]), put on a board
//! at the origin as a board file holds a footprint (no `(version ..)` or `(generator ..)` of its own, an `(at 0 0)`), and read back with the board's reader
//! ([`crate::parse_pcb_clipboard`], `import_kicad_pcb`). The record is the one a board file with that footprint gives, so the writer writes it back as the library had it.

use eda_model::footprint::Footprint;
use eda_model::fp_edit::{FootprintEdit, PadEdit};
use eda_model::ir::{FootprintExtra, LibraryFootprint, Nm, Zone};

use crate::sexpr::{self, Sexpr};

/// What the board holds of a library footprint ([`library_on_board`]), in the footprint's own frame (a top-side footprint at the origin, unturned).
#[derive(Debug, Clone)]
pub struct LibraryOnBoard {
    /// The engine's footprint the board's reader makes of the library's: the pads in the order of [`Self::record`]'s pad facts (`FootprintExtra::pads`).
    pub definition: Footprint,
    /// The record, a library footprint's (`FootprintExtra::from_library`, `library_frame`): no id, uuid, sheet, classes or source name yet (the caller's), no groups (they
    /// name the art of [`Self::definition`] by place, which the footprint the board puts the part on need not share), no embedded files (the data is the library's),
    /// and no paste margins among its tokens (they are [`Self::paste_margin`] and [`Self::paste_ratio`], which a footprint made here keeps in its edit).
    pub record: FootprintExtra,
    /// The footprint's own `(solder_paste_margin ..)` and `(solder_paste_margin_ratio ..)` (`GetLocalSolderPasteMargin`, `..Ratio`).
    pub paste_margin: Option<Nm>,
    pub paste_ratio: Option<f64>,
    /// Each pad's own mask and paste margins (`PAD::GetLocalSolderMaskMargin`, `..SolderPasteMargin`, `..SolderPasteMarginRatio`), as pad edits of
    /// [`Self::definition`]'s pads (`PadEdit::number`, `nth`), the margins alone.
    pub pad_margins: Vec<PadEdit>,
    /// The footprint's attributes (`(attr ..)`, `FOOTPRINT::GetAttributes`) as the board's edit has them, when the reader keeps any.
    pub attrs: Option<eda_model::fp_edit::FootprintAttrs>,
    /// The footprint's zones (`Zones()`): rule areas, keepouts and zones of non-copper layers, on no net, in the footprint's frame, with no parent yet.
    pub zones: Vec<Zone>,
}

/// The board's footprint of the library footprint `lib` (see the module doc), on a board whose copper layers are `copper_layers` (front to back: a pad on `*.Cu` is on
/// each). `None` when the board's reader refuses the footprint the library's writer writes (it never should).
pub fn library_on_board(lib: &LibraryFootprint, copper_layers: &[String]) -> Option<LibraryOnBoard> {
    let text = crate::footprint_lib::export_kicad_mod(lib);
    let tree = sexpr::parse(&text).ok()?;
    let fp = tree.as_list().filter(|l| matches!(sexpr::tag(l), Some("footprint") | Some("module")))?;
    let version = sexpr::find(fp, "version").and_then(|v| sexpr::num(v, 1)).map(|v| v as i64).unwrap_or(20240108);
    // the footprint as a board file has it: its name, then `(at 0 0)`, its children but the library file's own header
    let mut on_board: Vec<Sexpr> = Vec::with_capacity(fp.len() + 1);
    for (i, child) in fp.iter().enumerate() {
        if i >= 2 && child.as_list().and_then(|l| sexpr::tag(l)).is_some_and(|t| matches!(t, "version" | "generator" | "generator_version" | "at")) {
            continue;
        }
        on_board.push(child.clone());
        if i == 1 {
            on_board.push(sexpr::parse("(at 0 0)").ok()?);
        }
    }
    let layers = if copper_layers.len() > 2 {
        let listed: Vec<String> = copper_layers.iter().enumerate().map(|(i, l)| format!("({i} {} signal)", crate::sexpr_str(l))).collect();
        format!(" (layers {})", listed.join(" "))
    } else {
        String::new()
    };
    let board = format!("(kicad_pcb (version {version}) (generator \"pcbnew\"){layers} {})", sexpr::to_text(&Sexpr::List(on_board)));
    let clip = crate::clipboard::parse_pcb_clipboard(&board).ok()?;
    let reference = clip.footprints.first()?.reference.clone();
    let zones: Vec<Zone> = clip
        .zones
        .iter()
        .filter(|z| z.parent_footprint.as_deref().is_some_and(|p| p.split('#').next() == Some(reference.as_str())))
        .map(|z| Zone { net: String::new(), parent_footprint: None, uuid: String::new(), ..z.clone() })
        .collect();
    let cf = clip.footprints.into_iter().next()?;
    let definition = cf.definition?;
    let mut record = cf.extra.unwrap_or_default();
    record.id = String::new();
    record.uuid = None;
    record.unlinked_name = None;
    record.source_name = String::new();
    record.groups.clear();
    record.embedded = Default::default();
    record.from_library = true;
    record.library_frame = true;
    // the paste margins go to the edit (a footprint made here keeps them there: the writer writes them from it)
    let facts = crate::footprint_file_facts(&record.tokens);
    record.tokens.retain(|t| !["(solder_paste_margin ", "(solder_paste_margin_ratio ", "(solder_paste_ratio "].iter().any(|h| t.starts_with(h)));
    let edit: Option<FootprintEdit> = cf.edit;
    let pad_margins = edit
        .as_ref()
        .map(|e| {
            e.pads
                .iter()
                .filter(|p| p.solder_mask_margin.is_some() || p.solder_paste_margin.is_some() || p.solder_paste_margin_ratio.is_some())
                .map(|p| PadEdit { solder_mask_margin: p.solder_mask_margin, solder_paste_margin: p.solder_paste_margin, solder_paste_margin_ratio: p.solder_paste_margin_ratio, ..PadEdit::none(&p.number, p.nth) })
                .collect()
        })
        .unwrap_or_default();
    let attrs = edit.as_ref().and_then(|e| e.attrs);
    Some(LibraryOnBoard { definition, record, paste_margin: facts.paste_margin, paste_ratio: facts.paste_margin_ratio, pad_margins, attrs, zones })
}

#[cfg(test)]
mod tests {
    use super::*;
    use eda_model::units::mm;

    /// A library footprint with what the board's footprint used to lose: an SMD pad with no paste, a pad with a die length and a fabrication property, a paste-only
    /// pad, the description, tags, private layers and jumper groups, the margins, a dimension, a keepout zone and a table.
    pub(crate) fn library_text() -> String {
        r#"(footprint "Lib:Kept" (version 20241229) (generator "pcbnew") (layer "F.Cu")
            (descr "a kept footprint") (tags "kept test")
            (property "Reference" "REF**" (at 0 -3 0) (layer "F.SilkS") (effects (font (size 1 1) (thickness 0.15))))
            (property "Value" "Kept" (at 0 3 0) (layer "F.Fab") (effects (font (size 1 1) (thickness 0.15))))
            (property "Datasheet" "kept.pdf" (at 0 0 0) (layer "F.Fab") (hide yes) (effects (font (size 1.27 1.27))))
            (attr smd exclude_from_pos_files)
            (private_layers "User.1")
            (jumper_pad_groups ("1" "2"))
            (solder_mask_margin 0.05) (solder_paste_margin -0.03) (clearance 0.3)
            (fp_line (start -2 -2) (end 2 -2) (stroke (width 0.12) (type solid)) (layer "F.SilkS"))
            (fp_rect (start -2.5 -2.5) (end 2.5 2.5) (stroke (width 0.05) (type solid)) (fill none) (layer "F.CrtYd"))
            (dimension (type aligned) (layer "F.Fab") (pts (xy -1 2) (xy 1 2)) (height 1) (format (prefix "") (suffix "") (units 2) (units_format 1) (precision 2)) (style (thickness 0.1) (arrow_length 1.27) (text_position_mode 0) (extension_height 0.58642) (extension_offset 0.5) keep_text_aligned))
            (table (column_count 1) (layer "F.Fab") (border (external yes) (header yes) (stroke (width 0.1) (type solid))) (separators (rows yes) (cols yes) (stroke (width 0.1) (type solid))) (column_widths 3) (row_heights 1.5)
              (cells (table_cell "T" (start -1.5 -1) (end 1.5 0.5) (margins 0.2 0.2 0.2 0.2) (span 1 1) (layer "F.Fab") (effects (font (size 1 1) (thickness 0.15)) (justify left top)))))
            (pad "1" smd roundrect (at -1 0) (size 1 1) (layers "F.Cu" "F.Mask") (roundrect_rratio 0.25) (property pad_prop_heatsink) (die_length 0.5))
            (pad "2" smd rect (at 1 0) (size 1 1) (layers "F.Cu" "F.Paste" "F.Mask") (solder_mask_margin 0.02))
            (pad "" smd rect (at 0 1) (size 0.5 0.5) (layers "F.Paste"))
            (zone (net 0) (net_name "") (layer "F.Cu") (hatch edge 0.5) (connect_pads (clearance 0)) (min_thickness 0.25) (keepout (tracks not_allowed) (vias not_allowed) (pads allowed) (copperpour not_allowed) (footprints allowed)) (fill (thermal_gap 0.5) (thermal_bridge_width 0.5)) (polygon (pts (xy -3 3) (xy 3 3) (xy 3 4) (xy -3 4))))
        )"#
        .to_string()
    }

    #[test]
    fn a_library_footprint_on_the_board_keeps_what_its_file_says() {
        let lib = crate::parse_library_footprint(&library_text()).unwrap().footprint;
        let on = library_on_board(&lib, &["F.Cu".into(), "B.Cu".into()]).expect("the board reads the library's footprint");
        let r = &on.record;
        assert!(r.from_library && r.library_frame && !r.holds_file_form());
        assert_eq!(on.definition.pads.len(), 2, "the copper pads");
        assert_eq!(r.pads.len(), 2, "each with its file facts");
        let by_number = |n: &str| on.definition.pads.iter().position(|p| p.number == n).map(|i| &r.pads[i]).unwrap();
        assert!(by_number("1").no_paste, "pad 1 has no paste");
        assert!(by_number("1").tokens.iter().any(|t| t.contains("die_length")) && by_number("1").tokens.iter().any(|t| t.contains("pad_prop_heatsink")));
        assert!(!by_number("2").no_paste);
        assert_eq!(r.aux_pads.len(), 1, "the paste-only pad");
        for (what, head) in [("description", "(descr "), ("tags", "(tags "), ("private layers", "(private_layers"), ("jumper groups", "(jumper_pad_groups"), ("datasheet", "(property \"Datasheet\"")] {
            assert!(r.tokens.iter().any(|t| t.starts_with(head)), "{what}: {:?}", r.tokens);
        }
        assert!(!r.tokens.iter().any(|t| t.starts_with("(solder_paste_margin")), "the paste margin is the edit's");
        assert_eq!(on.paste_margin, Some(mm(-0.03)));
        assert_eq!((r.solder_mask_margin, r.clearance), (Some(mm(0.05)), Some(mm(0.3))));
        assert_eq!(on.pad_margins.iter().map(|p| (p.number.as_str(), p.solder_mask_margin)).collect::<Vec<_>>(), [("2", Some(mm(0.02)))]);
        assert!(on.attrs.is_some_and(|a| a.exclude_from_pos_files));
        assert_eq!((r.items.dimensions.len(), r.items.tables.len()), (1, 1));
        assert_eq!(on.zones.len(), 1);
        assert!(on.zones[0].is_rule_area && on.zones[0].parent_footprint.is_none() && on.zones[0].net.is_empty());
        assert_eq!(on.zones[0].outline.first().map(|p| (p.x, p.y)), Some((mm(-3.0), mm(3.0))), "in the footprint's frame");
    }
}
