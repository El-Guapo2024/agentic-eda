//! Preferences > Schematic Editor > Editing Options > "Power Symbols:" (`EESCHEMA_SETTINGS::m_Drawing.new_power_symbols`, `POWER_SYMBOLS::DEFAULT` / `GLOBAL` /
//! `LOCAL`), applied to the power symbol Place Power places ([`crate::sch_edit::SchCmd::SetNewPowerKind`], UI-LEFT-2 SS12).
//!
//! Ported from KiCad 8303b2ad `eeschema/tools/sch_drawing_tools.cpp::PlaceSymbol` (the same at 25ec2335): before the new `SCH_SYMBOL` is made from the
//! chosen `LIB_SYMBOL`, "Only convert between power symbol types. Regular (non-power) symbols must never be promoted to power symbols just because the default
//! is set to Global or Local. The preference's Default option means 'follow the symbol definition'": a power symbol that is not local becomes local
//! (`SetLocalPower()`) when the preference is Local, one that is not global becomes global (`SetGlobalPower()`) when it is Global. The placed symbol carries
//! that copy of the library symbol, so it names a net on its own sheet only, or everywhere (`SCH_PIN::IsLocalPower`).
//!
//! The studio sends the placing click's `add_power_symbol` and then this verb with the preference's kind, in one batch (one undo step).
//!
//! Where the symbol goes in its screen's library-symbol cache is `SCH_SCREEN::Append`'s (KiCad 8303b2ad `eeschema/sch_screen.cpp`, the same at 25ec2335 but
//! for the embedded files' refresh): the cache holds one entry per name (`m_libSymbols`); a symbol whose library symbol is the entry its name gives (its
//! library id) is drawn from it; one that differs -- here, the other kind of power symbol -- is drawn from an entry of the same library id that it equals
//! (`getLibSymbolNameMatches`: the id, then every `<name>_<number>`), else from a new one named `<name>_<n>`, the first `n` from 1 no entry has
//! (`newName.Printf( wxT( "%s_%d" ), .. )`), linked by `SetSchSymbolLibraryName` (`(lib_name "+5V_1")` in the file). An entry no symbol names is gone
//! (`SCH_SCREEN::Remove`), so a symbol placed where its id's entry is gone makes it anew, of its own kind. The port keeps the id's entry's kind as before
//! (`SchExtras::local_power` / `global_power`, where it disagrees with the design's library symbol) and the renamed entries by symbol
//! (`SchExtras::power_lib_names`); `add_power_symbol` caches the library's kind ([`Board::cache_placed_power_symbol`]), and this verb caches the converted one.
//! (PARITY-sch §82; before it the other kind was refused.) The library symbol's keywords and description of a copy made local read "local power" and
//! "local label" (`PlaceSymbol`'s "Adjust the KiCad library default fields"): the writer derives them from the copy's kind (`LibSymbol::power_texts`,
//! UI-LEFT-3 S3-11), in the cache entry and the instance's Description.

use crate::{Board, CheckResult};

fn fail(check: &str, what: &str, msg: impl Into<String>) -> Vec<CheckResult> {
    vec![CheckResult::fail(check, what, msg)]
}

impl<'m> Board<'m> {
    /// [`crate::sch_edit::SchCmd::SetNewPowerKind`]: give the power symbol of `lib_id` just placed on the sheet in view (the last one of that id) the kind
    /// `local` says, as `PlaceSymbol` converts the library symbol before it makes the instance (`SetLocalPower()` / `SetGlobalPower()`), and cache it as
    /// `SCH_SCREEN::Append` does ([`Self::cache_placed_power_symbol`]). Nothing changes for a symbol that is not a power symbol, or one that already has
    /// that kind.
    pub(crate) fn set_new_power_kind(&mut self, lib_id: &str, local: bool) -> Result<(), Vec<CheckResult>> {
        // "Only convert between power symbol types": a symbol the design has no library symbol for, or one that is no power symbol, is left as it is
        let Some(lib) = self.design_symbol(lib_id) else { return Ok(()) };
        if !lib.power {
            return Ok(());
        }
        let sch = self.schematic_mut()?;
        let Some(index) = sch.power_symbols.iter().rposition(|p| p.lib_id == lib_id) else {
            return Err(fail("ops_unknown_power_symbol", lib_id, "no power symbol of this library symbol on the sheet"));
        };
        cache_power_symbol(sch, index, local, lib.connectivity.power_local);
        Ok(())
    }

    /// `SCH_SCREEN::Append` for the power symbol at `index` of the sheet in view, as `PlaceSymbol` makes it from the library symbol (`new SCH_SYMBOL(
    /// *libSymbol, ..)`, `AddToScreen`): of the library's kind. `add_power_symbol` calls it; a Place Power that converts the kind follows it with
    /// [`Self::set_new_power_kind`], which caches the symbol again with the converted one.
    pub(crate) fn cache_placed_power_symbol(&mut self, index: usize) {
        let Some(lib_id) = self.schematic_mut().ok().and_then(|s| s.power_symbols.get(index)).map(|p| p.lib_id.clone()) else { return };
        let Some(lib) = self.design_symbol(&lib_id).filter(|l| l.power) else { return };
        let lib_local = lib.connectivity.power_local;
        if let Ok(sch) = self.schematic_mut() {
            cache_power_symbol(sch, index, lib_local, lib_local);
        }
    }
}

/// `SCH_SCREEN::Append` (`aUpdateLibSymbol`) of the power symbol at `index` of `sch`, whose copy of its library symbol is local or global as `local` says
/// (`lib_local`: the design's library symbol's kind, against which the screen's own kind for the id is kept). It is looked up by its library id's name.
pub(crate) fn cache_power_symbol(sch: &mut eda_model::ir::SchematicSection, index: usize, local: bool, lib_local: bool) {
    cache_power_symbol_named(sch, index, local, lib_local, None);
}

/// [`cache_power_symbol`] of a symbol looked up by the cache name `name` (`symbol->GetSchSymbolLibraryName()`: a pasted symbol's `(lib_name ..)`, UI-LEFT-3
/// S3-12); `None` (or its library id) is the id's entry. `m_libSymbols.find( name )`: no entry of that name, the symbol's copy is it; one it equals (the same
/// library id and kind), it is drawn from it; one that differs, `getLibSymbolNameMatches` -- the id's entry, then every `<name>_<number>` -- for one it equals,
/// else a new `<name>_<n>`.
pub(crate) fn cache_power_symbol_named(sch: &mut eda_model::ir::SchematicSection, index: usize, local: bool, lib_local: bool, name: Option<&str>) {
    let (id, lib_id) = {
        let p = &sch.power_symbols[index];
        (p.id.clone(), p.lib_id.clone())
    };
    // (cached again: the entry it was given before is not its own any more)
    sch.extras.power_lib_names.remove(&id);
    let names_own = |sch: &eda_model::ir::SchematicSection| sch.power_symbols.iter().enumerate().any(|(i, p)| i != index && p.lib_id == lib_id && !sch.extras.power_lib_names.contains_key(&p.id));
    // the renamed entries of the screen (`m_libSymbols`, a map by name): name -> (library id, kind)
    let entries: std::collections::BTreeMap<String, (String, bool)> = sch
        .power_symbols
        .iter()
        .filter_map(|p| sch.extras.power_lib_names.get(&p.id).map(|e| (e.name.clone(), (p.lib_id.clone(), e.local))))
        .collect();
    let id_entry_local = sch.extras.power_is_local(&lib_id, Some(lib_local));
    match name.filter(|n| *n != lib_id) {
        // `it == m_libSymbols.end()`: no symbol names the id's entry, so the symbol's own copy is it
        None if !names_own(sch) => {
            if local {
                sch.extras.global_power.remove(&lib_id);
                if !lib_local {
                    sch.extras.local_power.insert(lib_id.clone());
                }
            } else {
                sch.extras.local_power.remove(&lib_id);
                if lib_local {
                    sch.extras.global_power.insert(lib_id.clone());
                }
            }
            return;
        }
        None => {}
        Some(n) => match entries.get(n) {
            // no entry of that name (and no library id of the sheet goes by it): the copy is cached under it
            None if !sch.power_symbols.iter().any(|p| p.lib_id == n) && !sch.symbols.iter().any(|s| s.lib_id == n) => {
                sch.extras.power_lib_names.insert(id, eda_model::sch_extras::PowerLibName { name: n.to_string(), local });
                return;
            }
            Some((l, k)) if *l == lib_id && *k == local => {
                sch.extras.power_lib_names.insert(id, eda_model::sch_extras::PowerLibName { name: n.to_string(), local });
                return;
            }
            _ => {}
        },
    }
    // `getLibSymbolNameMatches`: the id's entry first -- one of this kind draws the symbol (`*foundSymbol == *symbol->GetLibSymbolRef()`) ..
    if names_own(sch) && id_entry_local == local {
        return;
    }
    let item = lib_id.rsplit(':').next().unwrap_or(&lib_id).to_string();
    // .. then the first `<item>_<number>` of this library symbol and this kind
    let is_match = |name: &str| name.strip_prefix(item.as_str()).and_then(|r| r.strip_prefix('_')).is_some_and(|n| n.parse::<i64>().is_ok());
    let found = entries.iter().find(|(name, (l, k))| is_match(name) && *l == lib_id && *k == local).map(|(name, _)| name.clone());
    let name = found.unwrap_or_else(|| {
        // `while( m_libSymbols.find( newName ) != m_libSymbols.end() )`: every name of the screen's cache
        let taken = |n: &str| entries.contains_key(n) || sch.power_symbols.iter().any(|p| p.lib_id == n) || sch.symbols.iter().any(|s| s.lib_id == n);
        (1..).map(|cnt| format!("{item}_{cnt}")).find(|n| !taken(n)).unwrap_or_default()
    });
    sch.extras.power_lib_names.insert(id, eda_model::sch_extras::PowerLibName { name, local });
}

#[cfg(test)]
mod tests {
    use crate::sch_edit::SchCmd;
    use crate::{Board, Cmd};
    use eda_model::ir::{Design, Point, SchematicSection};
    use eda_model::units::mm;
    use eda_model::ConstraintModel;

    fn design() -> Design {
        let mut d: Design = serde_json::from_value(serde_json::json!({ "provenance": { "engine_version": "t", "intent_hash": "x", "seed": 0, "stage_hashes": [] } })).unwrap();
        d.schematic = Some(SchematicSection::default());
        d
    }

    /// A one-pin power-input power symbol (the built-in GND's drawing), local or global in its library.
    fn rail(lib_id: &str, local: bool) -> eda_model::LibSymbol {
        let mut lib = eda_model::symbol::builtin("power:GND").unwrap();
        lib.lib_id = lib_id.into();
        lib.connectivity.power_local = local;
        lib
    }

    /// The click of Place Power with the preference: `add_power_symbol`, then the kind, one batch.
    fn place(lib_id: &str, x: f64, local: Option<bool>) -> Cmd {
        let net = lib_id.rsplit(':').next().unwrap().to_string();
        let add = Cmd::AddPowerSymbol { lib_id: lib_id.into(), at: Point { x: mm(x), y: mm(10.0) }, rot_millideg: 0, net, pin: String::new(), annotate: None };
        match local {
            None => add,
            Some(local) => Cmd::Batch { cmds: vec![add, Cmd::SchEdit(SchCmd::SetNewPowerKind { lib_id: lib_id.into(), local })] },
        }
    }

    fn local_here(b: &Board, lib_id: &str, lib_local: bool) -> bool {
        b.design().schematic.as_ref().unwrap().extras.power_is_local(lib_id, Some(lib_local))
    }

    fn lib_name(b: &Board, n: usize) -> Option<(String, bool)> {
        let sch = b.design().schematic.as_ref().unwrap();
        sch.extras.power_lib_names.get(&sch.power_symbols[n].id).map(|e| (e.name.clone(), e.local))
    }

    fn kind_of(b: &Board, n: usize, lib_local: bool) -> bool {
        let sch = b.design().schematic.as_ref().unwrap();
        let p = &sch.power_symbols[n];
        sch.extras.power_symbol_is_local(&p.id, &p.lib_id, Some(lib_local))
    }

    /// `PlaceSymbol`: `new_power_symbols == LOCAL` makes a global power symbol local, `GLOBAL` a local one global, Default follows the library; a regular
    /// symbol is never made a power symbol; the click is one step.
    #[test]
    fn the_power_symbols_preference_converts_the_placed_power_symbol() {
        let m = ConstraintModel { symbols: vec![rail("lp:+5V", false), rail("lp:+48V", true)], ..Default::default() };
        let mut b = Board::new(design(), &m, mm(0.1), mm(0.3));
        b.apply(&place("lp:+5V", 10.0, Some(true))).unwrap();
        assert!(local_here(&b, "lp:+5V", false), "Local: the global library symbol placed as a local one");
        b.apply(&place("lp:+48V", 20.0, Some(false))).unwrap();
        assert!(!local_here(&b, "lp:+48V", true), "Global: the local library symbol placed as a global one");
        // the same kind again on the sheet changes nothing more
        b.apply(&place("lp:+5V", 30.0, Some(true))).unwrap();
        assert!(local_here(&b, "lp:+5V", false));
        assert!(b.design().schematic.as_ref().unwrap().extras.power_lib_names.is_empty(), "each is drawn from its id's entry");
        // "Only convert between power symbol types": a symbol the design has no library symbol for is left alone
        b.apply(&Cmd::SchEdit(SchCmd::SetNewPowerKind { lib_id: "nope:X".into(), local: true })).unwrap();
        assert!(!b.design().schematic.as_ref().unwrap().extras.local_power.contains("nope:X"));
    }

    /// PARITY-sch §82, `SCH_SCREEN::Append`: the other kind of a power symbol the sheet already has is drawn from a renamed entry of the cache, `+5V_1`; a
    /// third of that kind shares it (the entry it equals), and one of the id's entry's kind is drawn from that. A Default placement is of the library's
    /// kind and is cached the same way. An entry no symbol names is gone: placed again, the id's entry is the new symbol's own.
    #[test]
    fn the_other_kind_is_drawn_from_a_renamed_cache_entry() {
        let m = ConstraintModel { symbols: vec![rail("lp:+5V", false)], ..Default::default() };
        let mut b = Board::new(design(), &m, mm(0.1), mm(0.3));
        b.apply(&place("lp:+5V", 10.0, Some(true))).unwrap();
        assert!(local_here(&b, "lp:+5V", false) && lib_name(&b, 0).is_none());
        // Global: the id's entry is local, so a new entry `+5V_1`, global; the first stays local; one step, as the click is
        b.apply(&place("lp:+5V", 20.0, Some(false))).unwrap();
        assert_eq!(lib_name(&b, 1), Some(("+5V_1".into(), false)));
        assert!(kind_of(&b, 0, false) && !kind_of(&b, 1, false));
        // Default: the library's kind (global) -- the `+5V_1` it equals
        b.apply(&place("lp:+5V", 30.0, None)).unwrap();
        assert_eq!(lib_name(&b, 2), Some(("+5V_1".into(), false)));
        // Local again: the id's entry
        b.apply(&place("lp:+5V", 40.0, Some(true))).unwrap();
        assert_eq!(lib_name(&b, 3), None);
        assert!(kind_of(&b, 3, false));
        let ids: Vec<String> = b.design().schematic.as_ref().unwrap().power_symbols.iter().map(|p| p.id.clone()).collect();
        // every local one deleted: the id's entry is gone (`SCH_SCREEN::Remove`); a global placed now makes it anew, global, and joins no `_1`
        for n in [0, 3] {
            b.apply(&Cmd::DeletePowerSymbol { id: ids[n].clone() }).unwrap();
        }
        b.apply(&place("lp:+5V", 50.0, None)).unwrap();
        let sch = b.design().schematic.as_ref().unwrap();
        assert!(!sch.extras.local_power.contains("lp:+5V"), "the id's entry is the library's again");
        let last = sch.power_symbols.len() - 1;
        assert_eq!(lib_name(&b, last), None);
        assert_eq!(sch.extras.power_lib_names.len(), 2, "the two global ones keep `+5V_1`");
        // a local one now: the id's entry is global, `+5V_1` is global too, so `+5V_2`
        b.apply(&place("lp:+5V", 60.0, Some(true))).unwrap();
        let last = b.design().schematic.as_ref().unwrap().power_symbols.len() - 1;
        assert_eq!(lib_name(&b, last), Some(("+5V_2".into(), true)));
        assert!(kind_of(&b, last, false));
        // a deleted symbol's entry goes with it (a new power symbol may take its id)
        let id = b.design().schematic.as_ref().unwrap().power_symbols[last].id.clone();
        b.apply(&Cmd::DeletePowerSymbol { id: id.clone() }).unwrap();
        assert!(!b.design().schematic.as_ref().unwrap().extras.power_lib_names.contains_key(&id));
    }
}
