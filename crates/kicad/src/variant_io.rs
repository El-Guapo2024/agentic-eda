//! The design variants in the derived `.kicad_pcb` (UI-LEFT S2, KiCad 25ec2335): the board's variant list and each footprint's records, so that kicad-cli's
//! `pcb export pos --variant`, `gerbers --variant`, `pdf --variant` ... see the variant (`BOARD::SetCurrentVariant` takes only a name the board has, and
//! `PLACE_FILE_EXPORTER` asks the footprint: `GetDNPForVariant`, `GetExcludedFromPosFilesForVariant`, `GetFieldValueForVariant`).
//!
//! KiCad's board gets them from the schematic by Update PCB from Schematic: the netlist gives each component a variant's differences against the symbol
//! (`NETLIST_EXPORTER_XML`: a flag where the variant resolves it other than the symbol does, a field whose text is not the symbol's own), and
//! `BOARD_NETLIST_UPDATER::applyComponentVariants` puts on the footprint each one that is not what the footprint has already (`footprint->AddVariant( name )`,
//! which starts as the footprint's flags, then the flag or the field). The design's board is derived from the schematic, so the writer does the same here.
//!
//! **Differs**: no variant footprint (a variant's `Footprint` field naming another footprint, `applyComponentVariants`' several footprints of one reference);
//! the variants of a board read from a `.kicad_pcb` alone are not kept; a footprint of a screen shown twice gets the records of the screen's symbol only
//! under that symbol's own reference.

use crate::sexpr_str;
use eda_model::ir::{Design, SchematicSection, SymbolInstance};
use eda_model::variants::{Flag, SymbolFlags};
use std::fmt::Write;

/// `PCB_IO_KICAD_SEXPR::formatVariants`: `(variants (variant (name ..) [(description ..)]) ..)` after the board's setup, for a design with variants.
pub(crate) fn write_board_variants(out: &mut String, design: &Design) {
    let names = eda_model::variants::names(design);
    if names.is_empty() {
        return;
    }
    let descriptions = design.schematic.as_ref().map(|s| &s.extras.variants);
    write!(out, "\t(variants").unwrap();
    for name in &names {
        write!(out, " (variant (name {})", sexpr_str(name)).unwrap();
        if let Some(d) = descriptions.and_then(|d| d.get(name)).filter(|d| !d.is_empty()) {
            write!(out, " (description {})", sexpr_str(d)).unwrap();
        }
        write!(out, ")").unwrap();
    }
    writeln!(out, ")").unwrap();
}

/// The symbol of reference `id` and the section it is on (the root's, else a sheet screen's).
fn symbol_of<'a>(design: &'a Design, id: &str) -> Option<(&'a SchematicSection, &'a SymbolInstance)> {
    design.schematic.iter().chain(design.sheet_contents.iter().flat_map(|c| c.values())).find_map(|sch| sch.symbols.iter().find(|s| s.id == id).map(|s| (sch, s)))
}

/// A field's own text on the symbol (what the footprint's field of that name has, as the board writer copies the symbol's fields): the Value, Datasheet and
/// user fields; `part_value` is the part's Value for a symbol whose own is empty.
fn own_field_text<'a>(sch: &'a SchematicSection, sym: &'a SymbolInstance, part_value: Option<&'a str>, field: &str) -> &'a str {
    match field {
        "Value" if sym.value.is_empty() => part_value.unwrap_or(""),
        "Value" => &sym.value,
        "Footprint" => &sym.footprint,
        "Datasheet" => &sym.datasheet,
        other => sch.user_fields.get(&sym.id).and_then(|m| m.get(other)).map_or("", String::as_str),
    }
}

/// `PCB_IO_KICAD_SEXPR::format( FOOTPRINT )`'s `(variant (name ..) (dnp ..) (exclude_from_bom ..) (exclude_from_sim ..) (exclude_from_pos_files ..) (field (name
/// ..) (value ..)))` blocks of footprint `id`, whose own flags are `fp_dnp`, `fp_no_bom` and `fp_no_pos` (its attributes): for each variant the symbol has a
/// record of, the flags where the variant differs from the symbol and from the footprint, and the field texts (not `Footprint`) that differ from the
/// symbol's own (`applyComponentVariants`, then the writer's `if( variant.GetDNP() != baseDnp )`). A variant with nothing to say is no block, as the updater
/// adds no footprint variant for it.
pub(crate) fn write_footprint_variants(out: &mut String, design: &Design, part_value: Option<&str>, id: &str, fp_dnp: bool, fp_no_bom: bool, fp_no_pos: bool) {
    let Some((sch, sym)) = symbol_of(design, id) else { return };
    let base = SymbolFlags::of(sch, sym);
    let yes_no = |b: bool| if b { "yes" } else { "no" };
    for (name, rec) in eda_model::variants::written_records(sch, sym) {
        let flags = rec.flags(base);
        let mut body = String::new();
        // the footprint's own flag for each the board writes; a footprint is never excluded from simulation here (the writer gives it no such attribute)
        for (f, word, fp_own) in [(Flag::Dnp, "dnp", fp_dnp), (Flag::ExcludeFromBom, "exclude_from_bom", fp_no_bom), (Flag::ExcludeFromSim, "exclude_from_sim", false), (Flag::ExcludeFromPosFiles, "exclude_from_pos_files", fp_no_pos)] {
            let v = flags.get(f);
            if v != base.get(f) && v != fp_own {
                write!(body, " ({word} {})", yes_no(v)).unwrap();
            }
        }
        for (field, text) in &rec.fields {
            if field.eq_ignore_ascii_case("Footprint") || field == "Reference" || text == own_field_text(sch, sym, part_value, field) {
                continue;
            }
            write!(body, " (field (name {}) (value {}))", sexpr_str(field), sexpr_str(text)).unwrap();
        }
        if !body.is_empty() {
            writeln!(out, "\t\t(variant (name {}){body})", sexpr_str(name)).unwrap();
        }
    }
}
