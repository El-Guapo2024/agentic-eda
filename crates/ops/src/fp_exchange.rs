//! Change Footprint(s), Update Footprint(s) from Library and the schematic's Footprint field followed: [`Cmd::ExchangeFootprints`].
//!
//! Ports `PCB_EDIT_FRAME::ExchangeFootprint` (`pcbnew/pcb_edit_frame.cpp` at KiCad 8303b2ad) as the three callers use it: `DIALOG_EXCHANGE_FOOTPRINTS::processFootprint`
//! (Change Footprint and Update Footprint from the dialog, with its options) and `BOARD_NETLIST_UPDATER::replaceFootprint` (Update PCB from Schematic: every option on,
//! pad positions matched). The footprint a part is on is a per-instance assignment ([`eda_model::fp_assign::FootprintAssignment`]); what the exchange does to the
//! part is:
//!
//! * the pose keeps the side, the label side, the lock and the group; with "Match pad positions" the position and the orientation move so that the pads whose number is
//!   unique land where the old ones were ([`eda_model::fp_exchange::pad_alignment`], `ComputeFootprintShift`);
//! * the pads are the new footprint's. In Change and Update Footprint each takes the net, pin name and pin type of the old pad it matches as KiCad matches them
//!   (`matchItemsBySimilarity<PAD>`, [`eda_model::fp_exchange::match_pads`]: by number, then by place and shape; UI-LEFT P21) -- `design.nets` names `REF.PIN`, so a pad
//!   the new footprint numbers otherwise moves the old pin's place in the net list to its own, and a new pad no old one matches is on no net, until Update PCB from
//!   Schematic; that update's own exchange gives the nets by number (`ExchangeOptions::pads_by_number`, the netlist's `updateComponentPadConnections`). A footprint
//!   that exists only on the board keeps its `pad_nets` likewise;
//! * the pad edits (`footprint_edits[].pads`) are dropped: they were edits of the old pads;
//! * the footprint's texts: the Reference and Value layouts, the library texts and the user fields follow "update/reset text layers and visibilities", "sizes and styles",
//!   "position and rotation" and "content" ([`eda_model::fp_exchange::carry_text`]); a text of the old footprint the new one has no match for is kept where it was on the board
//!   unless "remove text items if not in library footprint" is on;
//! * the fabrication attributes (`footprint_edits[].attrs`) are the new footprint's unless "update/reset fabrication attributes" is off, the footprint-level clearance, mask
//!   and zone-connection overrides (`zone_overrides`, `footprint_extras`) go with "update/reset clearance overrides", the 3D models with "update/reset 3D models";
//! * the schematic's Footprint field follows the board in Change Footprint, as the back-annotation would carry it (the two editors are one design; the symbol is not left
//!   naming a footprint the board does not have).
//!
//! The verbs do not read libraries (a command has to replay and undo the same on a machine without them): `library` carries the library copy of each footprint the command
//! needs, filled in by the studio's server (`crates/cli/src/library_place.rs`).

use eda_model::units::Nm;
use super::{Board, Cmd};
use eda_model::footprint::Footprint;
use eda_model::fp_assign::FootprintAssignment;
use eda_model::fp_compare::{FootprintOverrides, InstanceFacts, PadOverrides};
use eda_model::fp_edit::{default_reference_layout, default_value_layout, FootprintAttrs, FootprintEdit};
use eda_model::fp_exchange::{carry_layout, carry_text, keep_text, match_pads, match_texts, pad_alignment, TextReset};
use eda_model::ir::{FootprintExtra, FootprintInstance, LabelSide, LibraryFootprint, Point, Side};
use eda_model::{CheckResult, ConstraintModel, Part};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

fn fail(check: &str, what: &str, msg: impl Into<String>) -> Vec<CheckResult> {
    vec![CheckResult::fail(check, what, msg.into())]
}

/// `DIALOG_EXCHANGE_FOOTPRINTS`' options, and the flags of `PCB_EDIT_FRAME::ExchangeFootprint`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExchangeOptions {
    /// "Match pad positions": move and turn the new footprint so its pads are where the old ones were.
    #[serde(default = "yes")]
    pub match_pad_positions: bool,
    /// "Remove text items if not in library footprint" (`deleteExtraTexts`).
    #[serde(default = "yes")]
    pub remove_extra_texts: bool,
    /// "Update/reset text layers and visibilities".
    #[serde(default = "yes")]
    pub reset_text_layers: bool,
    /// "Update/reset text sizes and styles".
    #[serde(default = "yes")]
    pub reset_text_effects: bool,
    /// "Update/reset text position and rotation".
    #[serde(default = "yes")]
    pub reset_text_positions: bool,
    /// "Update/reset text content".
    #[serde(default = "yes")]
    pub reset_text_content: bool,
    /// "Update/reset fabrication attributes".
    #[serde(default = "yes")]
    pub reset_fabrication_attrs: bool,
    /// "Update/reset clearance overrides".
    #[serde(default = "yes")]
    pub reset_clearance_overrides: bool,
    /// "Update/reset 3D models".
    #[serde(default = "yes")]
    pub reset_3d_models: bool,
    /// The pads' nets by number, as `BOARD_NETLIST_UPDATER::replaceFootprint`'s exchange has them once `updateComponentPadConnections` has run (Update PCB from
    /// Schematic: the netlist gives each pad its pin's net). Off, the dialogs' exchange: each new pad takes the net, pin name and pin type of the old pad it matches
    /// (`matchItemsBySimilarity<PAD>`: by number, then by place and shape; UI-LEFT P21).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub pads_by_number: bool,
}

fn yes() -> bool {
    true
}

impl Default for ExchangeOptions {
    fn default() -> Self {
        Self::replace()
    }
}

impl ExchangeOptions {
    /// What `BOARD_NETLIST_UPDATER::replaceFootprint` asks of `ExchangeFootprint`: pad positions matched and every other option left at its default, on.
    pub fn replace() -> Self {
        ExchangeOptions {
            match_pad_positions: true,
            remove_extra_texts: true,
            reset_text_layers: true,
            reset_text_effects: true,
            reset_text_positions: true,
            reset_text_content: true,
            reset_fabrication_attrs: true,
            reset_clearance_overrides: true,
            reset_3d_models: true,
            pads_by_number: true,
        }
    }

    /// The Change Footprint dialog as it opens (`DIALOG_EXCHANGE_FOOTPRINTS` with `updateMode` off): the texts, the attributes, the overrides and the models are the new
    /// footprint's, extra text items are kept, pad positions are matched.
    pub fn change() -> Self {
        ExchangeOptions { remove_extra_texts: false, pads_by_number: false, ..Self::replace() }
    }

    /// The Update Footprint dialog as it opens (`updateMode` on): what the board has of texts and attributes stays, the library's clearance overrides and models are taken.
    pub fn update() -> Self {
        ExchangeOptions { remove_extra_texts: false, reset_text_layers: false, reset_text_effects: false, reset_text_positions: false, reset_text_content: false, reset_fabrication_attrs: false, pads_by_number: false, ..Self::replace() }
    }

    fn text_reset(&self) -> TextReset {
        TextReset { layers: self.reset_text_layers, effects: self.reset_text_effects, positions: self.reset_text_positions, content: self.reset_text_content }
    }
}

/// Why the footprint a symbol names is not the footprint the board has, for Update PCB from Schematic: the symbol of `reference` says `to`, the board has `from`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FootprintChange {
    pub reference: String,
    /// The footprint the board has (empty when the part has none).
    pub from: String,
    /// The footprint the symbol's Footprint field names.
    pub to: String,
    /// `FOOTPRINT::IsLocked`: Update PCB from Schematic does not change a locked footprint (`replaceFootprint`: "Cannot change %s footprint ... (footprint is locked)").
    pub locked: bool,
}

/// The name of the footprint a part is on: the footprint the intent or the board gave it, else the package a built-in footprint stands for.
pub fn footprint_name_of(part: &Part) -> String {
    part.footprint.clone().or_else(|| part.package.clone()).unwrap_or_default()
}

/// Whether `name` names the footprint `part` is on: [`footprint_name_of`], or the name the files give it (`eda_model::footprint::footprint_lib_id`: a built-in
/// `0603` is `Resistor_SMD:R_0603_1608Metric` on the board and in its symbol's Footprint field), so that a field holding what the files say is no change.
pub fn names_footprint_of(part: &Part, name: &str) -> bool {
    footprint_name_of(part) == name || eda_model::footprint::footprint_lib_id(part) == name
}

/// The parts whose symbol names another footprint than the board has (`BOARD_NETLIST_UPDATER::replaceFootprint`'s test: the netlist component's `GetFPID()` against the
/// footprint's): what Update PCB from Schematic lists and applies, and what Update Schematic from PCB would carry back. A symbol with no Footprint field, one excluded from the
/// board and a part the model has none of say nothing.
pub fn pending_footprint_changes(design: &eda_model::ir::Design, model: &ConstraintModel) -> Vec<FootprintChange> {
    let mut out: Vec<FootprintChange> = Vec::new();
    let locked: BTreeSet<&str> = design.drawings.iter().flat_map(|d| d.locked_ids.iter().map(String::as_str)).collect();
    for screen in design.schematic.iter().chain(design.sheet_contents.iter().flat_map(|c| c.values())) {
        for sym in &screen.symbols {
            let to = sym.footprint.trim();
            if to.is_empty() || sym.exclude_from_board || out.iter().any(|c| c.reference == sym.id) {
                continue;
            }
            let Some(part) = model.part(&sym.id) else { continue };
            if !names_footprint_of(part, to) {
                let from = eda_model::footprint::footprint_lib_id(part);
                out.push(FootprintChange { reference: sym.id.clone(), from, to: to.to_string(), locked: locked.contains(sym.id.as_str()) });
            }
        }
    }
    out.sort_by(|a, b| a.reference.cmp(&b.reference));
    out
}

/// A footprint as the board has it, with what the board knows of it beyond the pads and the drawings: the thing to compare with a library's copy
/// ([`eda_model::fp_compare::footprint_differences`]).
#[derive(Debug, Clone)]
pub struct BoardFootprint {
    /// The reference the part goes by (`FOOTPRINT::GetReference`: the report's "Segment of R1 on F.SilkS differs.").
    pub reference: String,
    /// The pads with the edits laid on, the graphics, the courtyard.
    pub footprint: Footprint,
    pub attrs: FootprintAttrs,
    pub overrides: FootprintOverrides,
    pub pad_overrides: Vec<PadOverrides>,
    /// `AllowSolderMaskBridges()` and `GetNetTiePadGroups().size()` as the board's file has them (`FootprintExtra`; a footprint placed here writes none).
    pub allow_soldermask_bridges: bool,
    pub net_tie_groups: usize,
}

impl BoardFootprint {
    pub fn facts(&self) -> InstanceFacts<'_> {
        InstanceFacts { footprint: &self.footprint, reference: &self.reference, attrs: self.attrs, overrides: self.overrides.clone(), pad_overrides: self.pad_overrides.clone(), allow_soldermask_bridges: self.allow_soldermask_bridges, net_tie_groups: self.net_tie_groups }
    }
}

/// The footprint of the part `reference` as the board has it: its pads (the edits of Pad Properties laid on), the attributes in effect and the overrides it carries
/// (`footprint_extras`, `zone_overrides`, the margins of its pad edits). `None` for a part with no footprint.
pub fn board_footprint(design: &eda_model::ir::Design, model: &ConstraintModel, reference: &str) -> Option<BoardFootprint> {
    let part = model.part(reference)?;
    let footprint = model.footprint_of(part)?;
    let attrs = design.effective_attrs(reference, &footprint);
    let name = footprint_name_of(part);
    let dr = design.drawings.as_ref();
    let extra = dr.and_then(|d| d.footprint_extras.iter().find(|e| e.id == reference));
    let edit = design.footprint_edit(reference);
    let n = footprint.pads.len();
    let first = footprint.pads.first().map(|p| design.pad_zone_facts(reference, &name, 0, n, &p.number));
    // The footprint-level paste margins are kept as the file wrote them (`FootprintExtra::tokens`: the model has no field for them).
    let paste = |tag: &str| extra.and_then(|e| eda_model::lib_keep::kept_mm(&e.tokens, tag));
    let overrides = FootprintOverrides {
        clearance: first.and_then(|f| f.footprint_clearance),
        solder_mask_margin: extra.and_then(|e| e.solder_mask_margin),
        solder_paste_margin: paste("solder_paste_margin"),
        solder_paste_margin_ratio: extra.and_then(|e| eda_model::lib_keep::kept_number(&e.tokens, "solder_paste_margin_ratio").or_else(|| eda_model::lib_keep::kept_number(&e.tokens, "solder_paste_ratio"))),
        zone_connection: first.and_then(|f| f.footprint_connection),
    };
    let mut pad_overrides = Vec::new();
    for (i, pad) in footprint.pads.iter().enumerate() {
        let facts = design.pad_zone_facts(reference, &name, i, n, &pad.number);
        let pad_edit = edit.and_then(|e| e.pad(&pad.number, footprint.pads[..i].iter().filter(|p| p.number == pad.number).count() as u32 + 1));
        // The pad's own file facts (`PadMaskInfo`), when the board was read from one and the counts still agree.
        let file_pad = extra.filter(|e| e.pads.len() == n).map(|e| &e.pads[i]);
        let o = PadOverrides {
            number: pad.number.clone(),
            clearance: facts.pad_clearance,
            solder_mask_margin: pad_edit.and_then(|e| e.solder_mask_margin).or_else(|| file_pad.and_then(|p| p.solder_mask_margin)),
            solder_paste_margin: pad_edit.and_then(|e| e.solder_paste_margin),
            solder_paste_margin_ratio: pad_edit.and_then(|e| e.solder_paste_margin_ratio),
            zone_connection: facts.connection,
            thermal_gap: facts.thermal_gap,
            thermal_spoke_width: facts.thermal_spoke_width,
            thermal_spoke_angle_mdeg: facts.thermal_spoke_angle_mdeg,
            // the pad's edit (Pad Properties on the board, UI-LEFT P3), else its file's
            pad_to_die_length: pad_edit.and_then(|e| e.die_length).filter(|l| *l != Nm::ZERO).or_else(|| file_pad.and_then(|p| eda_model::lib_keep::kept_mm(&p.tokens, "die_length")).filter(|_| pad_edit.and_then(|e| e.die_length).is_none())),
            fabrication_property: pad_edit.and_then(|e| e.property.clone()).or_else(|| file_pad.and_then(|p| eda_model::lib_keep::kept_atom(&p.tokens, "property"))).filter(|p| p != "pad_prop_none"),
            // The set the board's file has for the pad (`padNeedsUpdate`'s "%s layers differ."), from the kept facts or the defaults of its kind.
            layers: Some(eda_model::fp_compare::written_pad_layers(pad, file_pad)),
        };
        if o != (PadOverrides { number: pad.number.clone(), ..Default::default() }) {
            pad_overrides.push(o);
        }
    }
    Some(BoardFootprint {
        reference: reference.to_string(),
        footprint,
        attrs,
        overrides,
        pad_overrides,
        allow_soldermask_bridges: extra.is_some_and(|e| e.allow_soldermask_bridges),
        net_tie_groups: extra.map_or(0, |e| e.net_tie_pad_groups.len()),
    })
}

impl Board<'_> {
    /// The library copy of footprint `name` for an exchange: the Footprint Editor's entry for it (where the footprint was edited, the project's library is the one that
    /// has the newest copy), else the one the command carries, and -- when the footprint is changed to another (`update` off) -- one the model already knows by that name
    /// (another part's, a built-in package).
    fn footprint_to_put(&self, name: &str, part: &Part, library: &BTreeMap<String, Footprint>, update: bool) -> Option<Footprint> {
        let mut found = self.design.footprint_library.as_ref().and_then(|l| l.by_name(name)).map(LibraryFootprint::to_engine_footprint).or_else(|| library.get(name).cloned());
        if found.is_none() && !update {
            let probe = Part { reference: part.reference.clone(), mpn: None, lcsc: None, value: part.value.clone(), package: None, footprint: Some(name.to_string()), symbol: None, datasheet: None, pins: vec![], body_nm: None, edge: None };
            found = self.model.footprint_of(&probe);
        }
        found.map(|mut f| {
            f.name = name.to_string();
            f
        })
    }

    /// [`Cmd::ExchangeFootprints`]. All or nothing: a footprint that cannot be found, or a reference the board has no part for, refuses the whole command.
    pub(crate) fn exchange_footprints(&mut self, references: &[String], footprint: Option<&str>, options: &ExchangeOptions, library: &BTreeMap<String, Footprint>) -> Result<(), Vec<CheckResult>> {
        let target = footprint.map(str::trim);
        if target == Some("") {
            return Err(fail("ops_bad_footprint", "footprint", "there is no footprint to change to"));
        }
        let mut seen = BTreeSet::new();
        let refs: Vec<&String> = references.iter().filter(|r| seen.insert(r.as_str())).collect();
        if refs.is_empty() {
            return Err(fail("ops_nothing_to_change", "footprints", "no footprint to change"));
        }
        let update = target.is_none();
        // Everything that can be refused is refused before anything changes.
        let mut plan: Vec<(String, String, Footprint, Footprint)> = Vec::new();
        for r in refs {
            let part = self.model.part(r).ok_or_else(|| fail("ops_unknown_part", r, "no footprint with this reference is on the board"))?;
            let name = target.map(str::to_string).unwrap_or_else(|| footprint_name_of(part));
            if name.is_empty() {
                return Err(fail("ops_unknown_footprint", r, format!("{r} has no footprint to update")));
            }
            let new = self.footprint_to_put(&name, part, library, update).ok_or_else(|| fail("ops_unknown_footprint", &name, format!("library footprint '{name}' not found")))?;
            let old = self.model.footprint_of(part).unwrap_or_default();
            plan.push((r.to_string(), name, old, new));
        }
        for (reference, name, old, new) in plan {
            self.exchange_one(&reference, &name, &old, new, options, !update);
        }
        Ok(())
    }

    /// One footprint (`ExchangeFootprint`): `old` is the footprint the part is on as the board has it (pad edits laid on), `new` the one it is put on.
    pub(crate) fn exchange_one(&mut self, reference: &str, name: &str, old: &Footprint, mut new: Footprint, options: &ExchangeOptions, change: bool) {
        let placed_pose = self.pose_of(reference).cloned();
        let old_pose = placed_pose.clone().unwrap_or_else(|| FootprintInstance { id: reference.to_string(), at: Point { x: Nm::ZERO, y: Nm::ZERO }, rot: 0, side: Side::Top, label: LabelSide::default() });

        // "Update/reset 3D models": off keeps the models of the footprint on the board
        if !options.reset_3d_models {
            new.model = old.model.clone();
            new.models3d = old.models3d.clone();
        }

        // Where the new footprint goes: the old position and orientation, moved so that the pads are where they were ("Match pad positions")
        let new_pose = match (&placed_pose, options.match_pad_positions) {
            (Some(p), true) => pad_alignment(old, &new, p.side).map(|a| a.apply(p)).unwrap_or_else(|| p.clone()),
            _ => old_pose.clone(),
        };

        // The footprint's own texts: each of the old footprint's is carried over its match in the new one, or kept where it was, or dropped
        let reset = options.text_reset();
        let pairs = match_texts(&old.art.texts, &new.art.texts);
        let matched: BTreeSet<usize> = pairs.iter().map(|(i, _)| *i).collect();
        for (i, j) in &pairs {
            let mut text = new.art.texts[*j].clone();
            carry_text(&old.art.texts[*i], &mut text, &old_pose, &new_pose, &reset);
            new.art.texts[*j] = text;
        }
        if !options.remove_extra_texts {
            for (i, text) in old.art.texts.iter().enumerate() {
                if !matched.contains(&i) {
                    new.art.texts.push(keep_text(text, &old_pose, &new_pose));
                }
            }
        }

        // The Reference and the Value: their text is the schematic's; their layout is carried like any text's
        let was = self.design.footprint_edit(reference).cloned().unwrap_or_else(|| FootprintEdit::new(reference));
        let mut edit = was.clone();
        let old_reference = was.reference.clone().unwrap_or_else(|| default_reference_layout(&old_pose, old));
        let mut reference_layout = default_reference_layout(&new_pose, &new);
        let default_reference = reference_layout.clone();
        carry_layout(&old_reference, &mut reference_layout, &old_pose, &new_pose, &reset);
        edit.reference = (reference_layout != default_reference).then_some(reference_layout);
        let old_value = was.value.clone().unwrap_or_else(|| default_value_layout(&old_pose));
        let mut value_layout = default_value_layout(&new_pose);
        let default_value = value_layout.clone();
        carry_layout(&old_value, &mut value_layout, &old_pose, &new_pose, &reset);
        edit.value = (value_layout != default_value).then_some(value_layout);
        // the user fields are text items the library footprint has none of
        if options.remove_extra_texts {
            edit.fields.clear();
        }
        // the fabrication attributes: the new footprint's own, or the old footprint's as they stood
        edit.attrs = if options.reset_fabrication_attrs {
            None
        } else {
            let (dnp, no_bom) = self.design.symbol_bom_flags(reference);
            let kept = self.design.effective_attrs(reference, old);
            (kept != FootprintAttrs::derived(&new, dnp, no_bom)).then_some(kept)
        };
        // the pads are the new footprint's (with the pin names and types a renumbered pad carries over)
        edit.pads.clear();
        // the 3D models are the new footprint's own; with "update/reset 3D models" off they are the footprint's as it stood (`new` carries them now), and a list the board had
        // set stays set -- an empty one too, which is a footprint with no model, not one that has the package's generic one
        edit.models3d = (!options.reset_3d_models && was.models3d.is_some()).then(|| new.models3d.clone());

        // The pads' nets, pin names and pin types (`ExchangeFootprint`: `matchItemsBySimilarity<PAD>`, each new pad the old one it matched: `SetNetCode(
        // oldPad->GetNetCode() )`, `SetPinFunction`, `SetPinType`), unless the netlist gives them by number (Update PCB from Schematic). A pad matched by its own
        // number keeps what it had; one the new footprint numbers otherwise takes the old pad's, and a new pad no old one matches has none.
        let pad_matches = if options.pads_by_number { Vec::new() } else { match_pads(old, &old_pose, &new, &new_pose) };
        let renumbered: Vec<(String, String)> = pad_matches.iter().map(|&(i, j)| (old.pads[i].number.clone(), new.pads[j].number.clone())).collect();
        let moves_pins = !options.pads_by_number && (renumbered.iter().any(|(a, b)| a != b) || new.pads.iter().any(|p| !renumbered.iter().any(|(_, b)| b == &p.number)));
        if moves_pins {
            self.carry_pad_nets(reference, &renumbered);
        }
        // the pin name and type of a renumbered pad: the old pad's, as the board had them (its file's, else the schematic pin's), on the new pad's edit
        let link = self.model.symbol_links.get(reference).cloned();
        let old_infos = self.design.drawings.as_ref().and_then(|d| d.footprint_extras.iter().find(|e| e.id == reference)).map(|e| e.pads.clone()).filter(|p| p.len() == old.pads.len());
        let mut pin_edits: Vec<eda_model::fp_edit::PadEdit> = Vec::new();
        for &(i, j) in &pad_matches {
            let (a, b) = (&old.pads[i].number, &new.pads[j].number);
            if a == b {
                continue;
            }
            let function = old_infos.as_ref().map(|p| p[i].pin_function.clone()).filter(|f| !f.is_empty()).or_else(|| link.as_ref().and_then(|l| l.pins.get(a)).map(|(f, _)| f.clone()));
            let pin_type = old_infos.as_ref().map(|p| p[i].pin_type.clone()).filter(|t| !t.is_empty()).or_else(|| link.as_ref().and_then(|l| l.pins.get(a)).map(|(_, t)| t.clone()));
            let nth = new.pads[..j].iter().filter(|p| &p.number == b).count() as u32 + 1;
            pin_edits.push(eda_model::fp_edit::PadEdit { pin_function: Some(function.unwrap_or_default()), pin_type: Some(pin_type.unwrap_or_default()), ..eda_model::fp_edit::PadEdit::none(b, nth) });
        }

        // The footprint-level overrides stay with "update/reset clearance overrides" off; the pads' are the old pads'
        let keep_overrides = !options.reset_clearance_overrides;
        let board_part_pads: BTreeSet<String> = new.pads.iter().map(|p| p.number.clone()).collect();
        let carried_name = new.name.clone();
        let item_name = name.rsplit(':').next().unwrap_or(name).to_string();
        let old_item_name = self.part_footprint_name(reference).rsplit(':').next().unwrap_or("").to_string();

        // What the model resolves under that name already: when it is the footprint the board now has, the assignment needs no copy of its own
        let probe = self.model.part(reference).map(|p| Part { footprint: Some(name.to_string()), package: None, ..p.clone() });
        let known = probe.as_ref().and_then(|p| self.model.library_footprint_of(p));
        let definition = (known.as_ref() != Some(&new)).then(|| new.clone());

        for e in pin_edits {
            edit.set_pad(e);
        }
        let dr = self.drawings_mut();
        // the edit
        dr.footprint_edits.retain(|e| e.id != reference);
        if !edit.is_empty() {
            dr.footprint_edits.push(edit);
            dr.footprint_edits.sort_by(|a, b| a.id.cmp(&b.id));
        }
        // the zone overrides and the imported mask facts
        for z in dr.zone_overrides.iter_mut().filter(|z| z.id == reference) {
            z.pads.clear();
            if !keep_overrides {
                z.footprint = None;
            }
        }
        dr.zone_overrides.retain(|z| z.footprint.is_some() || !z.pads.is_empty());
        for ex in dr.footprint_extras.iter_mut().filter(|e| e.id == reference) {
            let kept = if keep_overrides { Some((ex.solder_mask_margin, ex.allow_soldermask_bridges, ex.clearance, ex.zone_connection)) } else { None };
            let (solder_mask_margin, allow_soldermask_bridges, clearance, zone_connection) = kept.unwrap_or((None, false, None, None));
            *ex = FootprintExtra { id: reference.to_string(), solder_mask_margin, allow_soldermask_bridges, clearance, zone_connection, ..Default::default() };
        }
        dr.footprint_extras.retain(|e| *e != FootprintExtra { id: e.id.clone(), ..Default::default() });
        // a footprint that exists only on the board is its own record
        if let Some(bp) = dr.board_parts.iter_mut().find(|b| b.reference == reference) {
            // "reset value text only when it is a proxy for the footprint ID"
            if bp.value.as_deref() == Some(old_item_name.as_str()) && options.reset_text_content {
                bp.value = Some(item_name.clone());
            }
            bp.footprint = name.to_string();
            let mut def = LibraryFootprint::from_engine_footprint(&new);
            def.name = carried_name;
            def.assign_missing_ids();
            bp.definition = Some(def);
            if moves_pins {
                // each new pad the net of the old pad it matched
                let old_nets = bp.pad_nets.clone();
                bp.pad_nets = renumbered.iter().filter_map(|(a, b)| old_nets.iter().find(|(p, _)| p == a).map(|(_, n)| (b.clone(), n.clone()))).collect();
                bp.pad_nets.sort();
                bp.pad_nets.dedup_by(|x, y| x.0 == y.0);
            }
            bp.pad_nets.retain(|(pad, _)| board_part_pads.contains(pad));
        }
        // the footprint the part is on
        let unchanged = self.part_footprint_name(reference) == name && definition.is_none() && self.design.footprint_assignment(reference).is_none();
        if !unchanged {
            self.drawings_mut().set_footprint_assignment(FootprintAssignment { reference: reference.to_string(), footprint: name.to_string(), definition });
        }
        // the pose
        if placed_pose.is_some() {
            if let Some(f) = self.design.placement.as_mut().and_then(|p| p.footprints.iter_mut().find(|f| f.id == reference)) {
                f.at = new_pose.at;
                f.rot = new_pose.rot;
            }
        }
        // the symbol names the footprint the board has
        if change {
            for screen in self.design.schematic.iter_mut().chain(self.design.sheet_contents.iter_mut().flat_map(|c| c.values_mut())) {
                for sym in screen.symbols.iter_mut().filter(|s| s.id == reference && !s.footprint.is_empty() && s.footprint != name) {
                    sym.footprint = name.to_string();
                }
            }
        }
    }

    /// The nets of `reference`'s pins once its old pads' nets are on the new pads they matched (`renumbered`: old number -> new number), in the net list in force
    /// (`design.nets`, else the intent's, which becomes the design's): the board then differs from the schematic, as KiCad's does after Change Footprint, until
    /// Update PCB from Schematic. A footprint only the board has keeps its nets in its own record (`BoardPart::pad_nets`, done by the caller).
    fn carry_pad_nets(&mut self, reference: &str, renumbered: &[(String, String)]) {
        if self.design.drawings.as_ref().is_some_and(|d| d.board_parts.iter().any(|b| b.reference == reference)) {
            return;
        }
        let mut nets = self.design.nets.clone().unwrap_or_else(|| self.model.nets.clone());
        let prefix = format!("{reference}.");
        let net_of = |pin: &str| nets.iter().find(|n| n.pins.iter().any(|p| p == pin)).map(|n| n.name.clone());
        let mut carried: Vec<(String, String)> = Vec::new();
        for (a, b) in renumbered {
            if carried.iter().any(|(pin, _)| *pin == format!("{prefix}{b}")) {
                continue;
            }
            if let Some(net) = net_of(&format!("{prefix}{a}")) {
                carried.push((format!("{prefix}{b}"), net));
            }
        }
        for n in nets.iter_mut() {
            n.pins.retain(|p| !p.starts_with(&prefix));
        }
        for (pin, net) in carried {
            if let Some(n) = nets.iter_mut().find(|n| n.name == net) {
                n.pins.push(pin);
                n.pins.sort();
            }
        }
        self.design.nets = Some(nets);
    }

    /// The name of the footprint the part is on now (the model's, before this command).
    fn part_footprint_name(&self, reference: &str) -> String {
        self.design.footprint_assignment(reference).map(|a| a.footprint.clone()).or_else(|| self.model.part(reference).map(footprint_name_of)).unwrap_or_default()
    }

    /// The part's footprint before its pad edits: the copy the board keeps of it, else what the model resolves.
    pub(crate) fn unedited_footprint(&self, part: &Part) -> Option<Footprint> {
        self.design.footprint_assignment(&part.reference).and_then(|a| a.definition.clone()).or_else(|| self.model.library_footprint_of(part))
    }
}

impl Cmd {
    /// A command that puts the parts named on another footprint, the way the symbol's Footprint field says it ([`ExchangeOptions::replace`]).
    pub fn replace_footprint(reference: &str, footprint: &str) -> Cmd {
        Cmd::ExchangeFootprints { references: vec![reference.to_string()], footprint: Some(footprint.to_string()), options: ExchangeOptions::replace(), library: BTreeMap::new() }
    }
}
