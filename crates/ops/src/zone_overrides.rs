//! The zone-connection edits of a placed footprint and its pads: `Cmd::SetFootprintZoneConnection` (Footprint Properties,
//! "Zone connection" and "Clearance") and `Cmd::SetPadZoneOverrides` (Pad Properties, "Pad connection", "Relief gap", "Spoke
//! width", "Spoke angle" and "Clearance"). Each is a whole-panel commit that replaces what it names, stored in
//! `design.drawings.zone_overrides` where `Design::pad_zone_facts` finds it: the zone filler then connects, clears and spokes
//! that pad as set (`ZONE_FILLER::knockoutThermalReliefs` -> `DRC_ENGINE::EvalZoneConnection`). Like every other `Cmd` they
//! are undone with the board's own history; they change no geometry, so routing stays.

use super::*;
use eda_model::ir::{FootprintZoneFacts, FootprintZoneOverrides, PadZoneOverride};

impl Board<'_> {
    fn zone_edit_of(&mut self, part: &str) -> &mut FootprintZoneOverrides {
        let dr = self.drawings_mut();
        let i = match dr.zone_overrides.iter().position(|e| e.id == part) {
            Some(i) => i,
            None => {
                dr.zone_overrides.push(FootprintZoneOverrides { id: part.to_string(), ..Default::default() });
                dr.zone_overrides.len() - 1
            }
        };
        &mut dr.zone_overrides[i]
    }

    /// `Cmd::SetFootprintZoneConnection`.
    pub(crate) fn set_footprint_zone_connection(&mut self, part: &str, zone_connection: Option<PadConnection>, clearance: Option<Um>) -> Result<(), Vec<CheckResult>> {
        self.require_placed(part)?;
        if clearance.is_some_and(|c| c < 0) {
            return Err(vec![CheckResult::fail("ops_bad_clearance", part, "a clearance override cannot be negative")]);
        }
        self.zone_edit_of(part).footprint = Some(FootprintZoneFacts { zone_connection, clearance });
        Ok(())
    }

    /// `Cmd::SetPadZoneOverrides`: every pad of `part` numbered `pad`.
    pub(crate) fn set_pad_zone_overrides(&mut self, part: &str, pad: &str, over: PadZoneOverride) -> Result<(), Vec<CheckResult>> {
        self.require_placed(part)?;
        let part_def = self.model.part(part).ok_or_else(|| vec![CheckResult::fail("ops_unknown_part", part, "no such part")])?;
        let has_pad = self.model.footprint_of(part_def).is_some_and(|f| f.pads.iter().any(|p| p.number == pad));
        if !has_pad {
            return Err(vec![CheckResult::fail("ops_unknown_pad", format!("{part}.{pad}"), "this footprint has no pad with that number")]);
        }
        for (what, v) in [("relief gap", over.thermal_gap), ("spoke width", over.thermal_spoke_width), ("clearance", over.clearance)] {
            if v.is_some_and(|v| v < 0) {
                return Err(vec![CheckResult::fail("ops_bad_pad_override", format!("{part}.{pad}"), format!("the {what} override cannot be negative"))]);
            }
        }
        let over = PadZoneOverride { number: pad.to_string(), thermal_spoke_angle_mdeg: over.thermal_spoke_angle_mdeg.map(|a| a.rem_euclid(360_000)), ..over };
        let edit = self.zone_edit_of(part);
        match edit.pads.iter().position(|p| p.number == pad) {
            Some(i) => edit.pads[i] = over,
            None => {
                edit.pads.push(over);
                edit.pads.sort_by(|a, b| a.number.cmp(&b.number));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use eda_model::ir::{PlacementSection, Provenance};
    use eda_model::{Part, Pin, PinKind};

    fn part(r: &str, pkg: &str) -> Part {
        Part {
            reference: r.into(),
            mpn: None,
            lcsc: None,
            value: None,
            package: Some(pkg.into()),
            footprint: None,
            pins: vec![Pin { number: "1".into(), name: None, kind: PinKind::Passive }, Pin { number: "2".into(), name: None, kind: PinKind::Passive }],
            body_um: None,
            symbol: None,
            datasheet: None,
            edge: None,
        }
    }

    fn design() -> Design {
        Design {
            footprint_library: None,
            sheet_contents: None,
            bus_aliases: vec![],
            symbol_library: None,
            schema: 1,
            provenance: Provenance { engine_version: "0".into(), intent_hash: "x".into(), seed: 0, stage_hashes: vec![] },
            schematic: None,
            nets: None,
            routing: None,
            placement: Some(PlacementSection {
                outline: vec![Point { x: 0, y: 0 }, Point { x: 50_000, y: 0 }, Point { x: 50_000, y: 50_000 }, Point { x: 0, y: 50_000 }],
                footprints: Vec::new(),
                modules: Vec::new(),
            }),
            drawings: None,
        }
    }

    fn placed() -> (ConstraintModel, Design) {
        let m = ConstraintModel { parts: vec![part("C1", "0402")], ..Default::default() };
        let mut d = design();
        d.placement.as_mut().unwrap().footprints.push(FootprintInstance { id: "C1".into(), at: Point { x: 10_000, y: 10_000 }, rot: 0, side: Side::Top, label: Default::default() });
        (m, d)
    }

    fn pad_edit(b: &Board) -> Vec<PadZoneOverride> {
        b.design().drawings.as_ref().and_then(|d| d.zone_overrides.iter().find(|e| e.id == "C1")).map(|e| e.pads.clone()).unwrap_or_default()
    }

    #[test]
    fn a_pad_override_is_stored_and_resolved_for_the_filler() {
        let (m, d) = placed();
        let mut b = Board::new(d, &m, 100, 300);
        b.apply(&Cmd::SetPadZoneOverrides {
            part: "C1".into(),
            pad: "1".into(),
            zone_connection: Some(PadConnection::Full),
            thermal_gap: Some(400),
            thermal_spoke_width: Some(300),
            thermal_spoke_angle_mdeg: Some(405_000),
            clearance: Some(600),
        })
        .unwrap();
        let edit = pad_edit(&b);
        assert_eq!(edit.len(), 1);
        assert_eq!(edit[0].zone_connection, Some(PadConnection::Full));
        assert_eq!(edit[0].thermal_spoke_angle_mdeg, Some(45_000), "an angle is kept within one turn");
        let facts = b.design().pad_zone_facts("C1", "0402", 0, 2, "1");
        assert_eq!((facts.connection, facts.thermal_gap, facts.thermal_spoke_width, facts.clearance()), (Some(PadConnection::Full), Some(400), Some(300), Some(600)));
        // pad 2 was not touched
        assert_eq!(b.design().pad_zone_facts("C1", "0402", 1, 2, "2"), Default::default());
    }

    #[test]
    fn the_footprints_own_connection_is_inherited_by_pads_that_do_not_set_one() {
        let (m, d) = placed();
        let mut b = Board::new(d, &m, 100, 300);
        b.apply(&Cmd::SetFootprintZoneConnection { part: "C1".into(), zone_connection: Some(PadConnection::None), clearance: Some(900) }).unwrap();
        let facts = b.design().pad_zone_facts("C1", "0402", 0, 2, "1");
        assert_eq!((facts.footprint_connection, facts.clearance()), (Some(PadConnection::None), Some(900)), "the footprint's facts reach its pads");
        // a pad's own clearance beats the footprint's
        b.apply(&Cmd::SetPadZoneOverrides { part: "C1".into(), pad: "1".into(), zone_connection: None, thermal_gap: None, thermal_spoke_width: None, thermal_spoke_angle_mdeg: None, clearance: Some(250) }).unwrap();
        assert_eq!(b.design().pad_zone_facts("C1", "0402", 0, 2, "1").clearance(), Some(250));
        assert_eq!(b.design().pad_zone_facts("C1", "0402", 1, 2, "2").clearance(), Some(900));
    }

    #[test]
    fn setting_a_pad_again_replaces_it_and_all_none_inherits_whatever_a_board_import_left() {
        let (m, mut d) = placed();
        // a board import left pad 1 on a solid connection
        d.drawings = Some(DrawingsSection {
            footprint_extras: vec![eda_model::ir::FootprintExtra {
                id: "C1".into(),
                pads: vec![eda_model::ir::PadMaskInfo { zone_connection: Some(PadConnection::Full), ..Default::default() }, Default::default()],
                ..Default::default()
            }],
            ..Default::default()
        });
        let mut b = Board::new(d, &m, 100, 300);
        assert_eq!(b.design().pad_zone_facts("C1", "0402", 0, 2, "1").connection, Some(PadConnection::Full));
        let set = |c: Option<PadConnection>| Cmd::SetPadZoneOverrides { part: "C1".into(), pad: "1".into(), zone_connection: c, thermal_gap: None, thermal_spoke_width: None, thermal_spoke_angle_mdeg: None, clearance: None };
        b.apply(&set(Some(PadConnection::Thermal))).unwrap();
        assert_eq!(b.design().pad_zone_facts("C1", "0402", 0, 2, "1").connection, Some(PadConnection::Thermal), "the edit wins over the import");
        b.apply(&set(None)).unwrap();
        assert_eq!(b.design().pad_zone_facts("C1", "0402", 0, 2, "1").connection, None, "inherit means inherit, not 'whatever the import said'");
        assert_eq!(pad_edit(&b).len(), 1, "set twice, stored once");
    }

    #[test]
    fn a_pad_that_does_not_exist_or_a_negative_value_is_refused() {
        let (m, d) = placed();
        let mut b = Board::new(d, &m, 100, 300);
        let cmd = |pad: &str, gap: Option<Um>| Cmd::SetPadZoneOverrides { part: "C1".into(), pad: pad.into(), zone_connection: None, thermal_gap: gap, thermal_spoke_width: None, thermal_spoke_angle_mdeg: None, clearance: None };
        assert_eq!(b.apply(&cmd("9", None)).unwrap_err()[0].check, "ops_unknown_pad");
        assert_eq!(b.apply(&cmd("1", Some(-5))).unwrap_err()[0].check, "ops_bad_pad_override");
        assert!(b.design().drawings.is_none() || pad_edit(&b).is_empty(), "a refused edit changes nothing");
        let e = b.apply(&Cmd::SetFootprintZoneConnection { part: "U9".into(), zone_connection: None, clearance: None }).unwrap_err();
        assert_eq!(e[0].check, "ops_not_placed");
    }

    #[test]
    fn the_commands_read_the_json_the_studio_sends() {
        let c: Cmd = serde_json::from_value(serde_json::json!({ "op": "set_pad_zone_overrides", "part": "C1", "pad": "1", "zone_connection": "Full", "thermal_gap": 400 })).unwrap();
        assert!(matches!(c, Cmd::SetPadZoneOverrides { zone_connection: Some(PadConnection::Full), thermal_gap: Some(400), thermal_spoke_width: None, .. }));
        let c: Cmd = serde_json::from_value(serde_json::json!({ "op": "set_footprint_zone_connection", "part": "C1", "zone_connection": null })).unwrap();
        assert!(matches!(c, Cmd::SetFootprintZoneConnection { zone_connection: None, clearance: None, .. }));
    }
}
