//! Preferences > Schematic Editor > Editing Options > "Power Symbols:" (`EESCHEMA_SETTINGS::m_Drawing.new_power_symbols`, `POWER_SYMBOLS::DEFAULT` / `GLOBAL` /
//! `LOCAL`), applied to the power symbol Place Power places ([`crate::sch_edit::SchCmd::SetNewPowerKind`], UI-LEFT-2 SS12).
//!
//! Ported from KiCad 8303b2ad `eeschema/tools/sch_drawing_tools.cpp::PlaceSymbol` (the same at 25ec2335): before the new `SCH_SYMBOL` is made from the
//! chosen `LIB_SYMBOL`, "Only convert between power symbol types. Regular (non-power) symbols must never be promoted to power symbols just because the default
//! is set to Global or Local. The preference's Default option means 'follow the symbol definition'": a power symbol that is not local becomes local
//! (`SetLocalPower()`) when the preference is Local, one that is not global becomes global (`SetGlobalPower()`) when it is Global. The placed symbol carries
//! that copy of the library symbol, so it names a net on its own sheet only, or everywhere (`SCH_PIN::IsLocalPower`).
//!
//! The studio sends the placing click's `add_power_symbol` and then this verb with the preference's kind, in one batch (one undo step). The port keeps the
//! kind a screen gives a power symbol by its library id (`SchExtras::local_power` / `global_power`, where the screen's cache disagrees with the design's
//! library symbol), so the converted kind is that screen's kind for the id. **Differs**: KiCad keeps a second, renamed cache entry (`+5V_1`) when the sheet
//! already has the library's kind of that symbol and the new one differs; here a sheet has one kind per power symbol, so the conversion is refused while
//! another power symbol of that id on the sheet has the other kind (rather than turning it too). The library symbol's keywords and description are not
//! rewritten ("global power" -> "local power", "global label" -> "local label").

use crate::{Board, CheckResult};

fn fail(check: &str, what: &str, msg: impl Into<String>) -> Vec<CheckResult> {
    vec![CheckResult::fail(check, what, msg)]
}

impl<'m> Board<'m> {
    /// [`crate::sch_edit::SchCmd::SetNewPowerKind`]: give the power symbol of `lib_id` just placed on the sheet in view (the last one of that id) the kind
    /// `local` says, as `PlaceSymbol` converts the library symbol before it makes the instance. Nothing changes for a symbol that is not a power symbol, or
    /// one that already has that kind.
    pub(crate) fn set_new_power_kind(&mut self, lib_id: &str, local: bool) -> Result<(), Vec<CheckResult>> {
        // "Only convert between power symbol types": a symbol the design has no library symbol for, or one that is no power symbol, is left as it is
        let Some(lib) = self.design_symbol(lib_id) else { return Ok(()) };
        if !lib.power {
            return Ok(());
        }
        let lib_local = lib.connectivity.power_local;
        let sch = self.schematic_mut()?;
        let placed = sch.power_symbols.iter().filter(|p| p.lib_id == lib_id).count();
        if placed == 0 {
            return Err(fail("ops_unknown_power_symbol", lib_id, "no power symbol of this library symbol on the sheet"));
        }
        if sch.extras.power_is_local(lib_id, Some(lib_local)) == local {
            return Ok(());
        }
        if placed > 1 {
            let others = if local { "global" } else { "local" };
            return Err(fail("ops_power_kind_mixed", lib_id, format!("the other {lib_id} power symbols of this sheet are {others}: a sheet keeps one kind of each power symbol")));
        }
        // the screen's kind for the id, where it differs from the library symbol's (`SchExtras::power_is_local`)
        if local {
            sch.extras.global_power.remove(lib_id);
            if !lib_local {
                sch.extras.local_power.insert(lib_id.to_string());
            }
        } else {
            sch.extras.local_power.remove(lib_id);
            if lib_local {
                sch.extras.global_power.insert(lib_id.to_string());
            }
        }
        Ok(())
    }
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
        // one sheet keeps one kind of each power symbol: the other kind is refused while one of the first is there
        let err = b.apply(&place("lp:+5V", 40.0, Some(false))).unwrap_err();
        assert_eq!(err[0].check, "ops_power_kind_mixed", "{err:?}");
        // the refused click places nothing: the batch is one step, all or none
        assert_eq!(b.design().schematic.as_ref().unwrap().power_symbols.len(), 3);
        // "Only convert between power symbol types": a symbol the design has no library symbol for is left alone
        b.apply(&Cmd::SchEdit(SchCmd::SetNewPowerKind { lib_id: "nope:X".into(), local: true })).unwrap();
        assert!(!b.design().schematic.as_ref().unwrap().extras.local_power.contains("nope:X"));
    }
}
