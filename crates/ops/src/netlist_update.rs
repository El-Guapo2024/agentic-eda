//! File > Import > Netlist... (`pcbnew.EditorControl.importNetlist`, `PCB_EDIT_FRAME::InstallNetlistFrame`, `DIALOG_IMPORT_NETLIST::loadNetlist`):
//! `BOARD_NETLIST_UPDATER::UpdateNetlist` (`pcbnew/netlist_reader/board_netlist_updater.cpp` at 8303b2ad) laying a netlist file onto the board, as
//! [`Cmd::ImportNetlist`](crate::Cmd::ImportNetlist) (Update PCB, one undo step) and [`test_netlist`] (Load and Test Netlist, the dry run: the same work,
//! told in the future tense, nothing changed). The file is read by `eda_kicad::netlist_reader`.
//!
//! What the updater does, here:
//!
//! * every component of the netlist (but one excluded from the board) is matched with the footprints on the board, by path (`/<sheet>/../<symbol>`, the
//!   "tstamps" link method, the default) or by reference (case-insensitively); its footprint is replaced by the one it names (`replaceFootprint`: the
//!   studio's exchange, `exchange_footprints` with `ExchangeOptions::replace`), or a footprint is added from the libraries (`addNewFootprint`: a footprint
//!   only the board has, `BoardPart`; an unplaced part of the design with that reference is placed instead, which is the studio's form of the footprint
//!   KiCad's Update PCB stacks beside the board);
//! * the footprint takes the component's reference, value, path, fields, sheet name and file, footprint filters and fabrication attributes
//!   (`updateFootprintParameters`), its group (`updateFootprintGroup`, "Group footprints based on symbol group"), and each pad its pin's net, function and
//!   type (`updateComponentPadConnections`, with the `_N` names of the no-connect pads that share one);
//! * vias and copper zones on a net the netlist does not have follow the pads to their new net (`updateCopperZoneNets`), groups take the netlist's names and
//!   design block links (`updateGroups`);
//! * the footprints no component matched are removed when "Delete footprints with no components in netlist" is on (not a locked one unless "Delete/replace
//!   footprints even if locked"; never a board-only one), the nets no pad is left on are removed, and every component pin no pad has is an error
//!   (`testConnectivity`).
//!
//! Every message is KiCad's, word for word, with its severity; the dialog's own head lines and the totals are [`NetlistReport::lines`]' first and last.
//!
//! **Where it is kept.** The pads' nets go into the design's one net list (`Design::nets`) -- the net list in force, as Swap Pad Nets writes it -- and a
//! footprint only the board has keeps them in its own record (`BoardPart::pad_nets`). What the board's derivation would take back on the next load (the
//! nets' names, which it takes from the schematic; each footprint's path, sheet, fields and pin names, which it takes from its symbol; a value that is not
//! the part's) is the record `DrawingsSection::imported_netlist` (`eda_model::netlist_import`), laid over the model last. The schematic is not changed: a
//! board whose schematic is still derived from the intent has that schematic stored first, in the same step (what the first schematic edit does), so the
//! import cannot reach it -- the schematic is what it was, and the board disagrees with it until Undo or a later netlist (kicad-cli's `--schematic-parity`
//! says how). The one exception is a reference: a footprint matched by path whose reference the netlist changes is renamed with its symbol (the studio keeps
//! a footprint and its symbol under one reference).
//!
//! **Differs from KiCad, by design or for now:**
//!
//! * the new footprints are put at the command's `at` (the studio's carry then spreads them as `SpreadFootprints` does and hands them to the Move tool), or,
//!   with none, where `estimateFootprintInsertionPosition` puts them (below the board's edges, or the page's centre); KiCad pushes the import as one step and
//!   the move as another, the studio's carried placement makes them one;
//! * a footprint's pads are taken in the footprint's order (KiCad sorts them by uuid), and the board's footprints by reference (KiCad: the board's order);
//! * one footprint per reference: a component whose reference is on the board already, unmatched (another path, or a footprint KiCad would keep beside a new
//!   one because "Replace footprints" is off), gets no second footprint -- "Cannot add %s (a footprint with this reference is on the board)." -- and a
//!   design variant's own footprint (`expectedFpids` beyond the base) is not added;
//! * not ported: component classes (`updateComponentClass`), unit metadata (`updateComponentUnits`), design variants (`applyComponentVariants` and the
//!   board's variant list), jumper pad settings, "Delete tracks shorting multiple nets" (which 8303b2ad's dialog shows and passes to nothing);
//! * a zone's pads are the pads on its net (KiCad: the pads its fill touches, `GetConnectedPads( zone )`);
//! * the tracks of a net that is removed take the net of the pads they reach on the next load (`net_names::heal_stale_nets`), where KiCad leaves them on no net.

use super::Board;
use eda_kicad::netlist_reader::{self, ComponentNet, Netlist, NetlistComponent};
use eda_model::fp_items::FootprintItems;
use eda_model::ir::{BoardPart, FootprintInstance, Group, LabelSide, LibraryFootprint, Point, Side};
use eda_model::netlist_import::{ImportedFootprint, ImportedNetlist};
use eda_model::units::Nm;
use eda_model::{CheckResult, Footprint, Net, PadKind, Part, SymbolLink};
use serde::{Deserialize, Serialize};
use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};

fn yes() -> bool {
    true
}

/// The dialog's choices (`DIALOG_IMPORT_NETLIST_BASE`), as it opens: "Link footprints using component tstamps (unique ids)", "Replace footprints with those
/// specified in netlist" and "Group footprints based on symbol group" on, the rest off.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct NetlistOptions {
    /// Link Method (`m_matchByTimestamp`): selection 0, "Link footprints using component tstamps (unique ids)"; off is 1, "Link footprints using reference designators".
    #[serde(default = "yes")]
    pub by_timestamp: bool,
    /// "Delete footprints with no components in netlist" (`m_cbDeleteExtraFootprints`, `SetDeleteUnusedFootprints`).
    #[serde(default)]
    pub delete_extra_footprints: bool,
    /// "Replace footprints with those specified in netlist" (`m_cbUpdateFootprints`, `SetReplaceFootprints`).
    #[serde(default = "yes")]
    pub replace_footprints: bool,
    /// "Group footprints based on symbol group" (`m_cbTransferGroups`, `SetTransferGroups`).
    #[serde(default = "yes")]
    pub transfer_groups: bool,
    /// "Delete/replace footprints even if locked" (`m_cbOverrideLocks`, `SetOverrideLocks`).
    #[serde(default)]
    pub override_locks: bool,
    /// "Delete tracks shorting multiple nets" (`m_cbDeleteShortingTracks`): 8303b2ad's dialog shows it and `loadNetlist` passes it to nothing.
    #[serde(default)]
    pub delete_shorting_tracks: bool,
}

impl Default for NetlistOptions {
    fn default() -> Self {
        NetlistOptions { by_timestamp: true, delete_extra_footprints: false, replace_footprints: true, transfer_groups: true, override_locks: false, delete_shorting_tracks: false }
    }
}

/// [`Cmd::ImportNetlist`](crate::Cmd::ImportNetlist): the file travels with the command, and so do the library footprints it needs, which the studio's server
/// fills in from the library tables when they are left out (a command has to replay and undo the same on a machine without the libraries).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ImportNetlist {
    /// The netlist file's text.
    pub text: String,
    /// Its name, for messages ("Reading netlist file '%s'.").
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub options: NetlistOptions,
    /// Where the footprints the import adds are put (the Move tool picks them up there); `None` is `estimateFootprintInsertionPosition`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub at: Option<Point>,
    /// `Lib:Name` -> the library footprint of each footprint the netlist names that the board's model does not know (`m_frame->LoadFootprint( aFootprintId )`).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub definitions: BTreeMap<String, LibraryFootprint>,
    /// `Lib:Name` -> the same footprint as the engine reads it, for an exchange (`replaceFootprint`).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub library: BTreeMap<String, Footprint>,
    /// `Lib:Name` -> its text boxes, images, points and barcodes (`eda_model::fp_items::FootprintItems`), for an exchange.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub library_items: BTreeMap<String, FootprintItems>,
    /// A bare footprint name (a legacy id, no library) -> the `Lib:Name` the libraries have it under (`LoadFootprintWithOptionalNickname`: the first library,
    /// alphabetically, that has it).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub found_as: BTreeMap<String, String>,
    /// Whether the footprint library table has a library at all (`PCB_EDIT_FRAME::LoadFootprints` checks nothing when it has none).
    #[serde(default = "yes")]
    pub libraries: bool,
    /// The file name the root sheet has (`FOOTPRINT::GetSheetfile` of a footprint on the root sheet: the board's own `.kicad_sch`), which the server fills in.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub root_sheetfile: String,
}

impl ImportNetlist {
    /// A command for the netlist `text` read from `name`, with the dialog's default choices and nothing filled in.
    pub fn new(text: &str, name: &str, options: NetlistOptions) -> Self {
        ImportNetlist { text: text.to_string(), name: name.to_string(), options, at: None, definitions: BTreeMap::new(), library: BTreeMap::new(), library_items: BTreeMap::new(), found_as: BTreeMap::new(), libraries: true, root_sheetfile: String::new() }
    }
}

/// `SEVERITY`: what a line of the report is (the message panel's filters).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Info,
    Action,
    Warning,
    Error,
}

/// One line of the message panel.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReportLine {
    pub severity: Severity,
    pub text: String,
}

/// What `loadNetlist` tells in the dialog's message panel (`WX_HTML_REPORT_PANEL`): the head lines, the updater's lines, the tail.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct NetlistReport {
    pub lines: Vec<ReportLine>,
    /// The footprints the update added (`GetAddedFootprints`): the ones `OnNetlistChanged` spreads and hands to the Move tool.
    pub added: Vec<String>,
    pub warnings: usize,
    pub errors: usize,
    /// The reader's refusal (`DisplayErrorMessage`: "Cannot open netlist file '%s'.", "Error loading netlist.\n%s"), when the file could not be read.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

thread_local! {
    static LAST_REPORT: RefCell<Option<NetlistReport>> = const { RefCell::new(None) };
}

/// The report of the last [`Cmd::ImportNetlist`](crate::Cmd::ImportNetlist) applied on this thread, taken (the studio's `/api/cmd` reply carries it).
pub fn take_report() -> Option<NetlistReport> {
    LAST_REPORT.with(|r| r.borrow_mut().take())
}

/// `EscapeHTML`: the message panel is HTML.
fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;").replace('\'', "&apos;")
}

/// `UnescapeString` of a net name: the `{slash}` and kin a KiCad net name may hold.
fn unesc(s: &str) -> String {
    s.replace("{slash}", "/").replace("{backslash}", "\\").replace("{lt}", "<").replace("{gt}", ">").replace("{colon}", ":").replace("{dblquote}", "\"").replace("{space}", " ").replace("{tab}", "\t").replace("{return}", "\n").replace("{brace}", "{")
}

/// Load and Test Netlist (`loadNetlist( true )`): the report of what Update PCB would do to the board `design` is, with nothing changed.
pub fn test_netlist(design: eda_model::ir::Design, model: &eda_model::ConstraintModel, snap: Nm, spacing: Nm, cmd: &ImportNetlist) -> NetlistReport {
    let mut board = Board::new(design, model, snap, spacing);
    board.update_netlist(cmd, true)
}

/// A footprint on the board as the updater sees it.
#[derive(Clone)]
struct OnBoard {
    reference: String,
    fpid: String,
}

impl Board<'_> {
    /// [`Cmd::ImportNetlist`](crate::Cmd::ImportNetlist): Update PCB (`loadNetlist( false )`). A file that cannot be read changes nothing and is refused with
    /// KiCad's message.
    pub(crate) fn import_netlist(&mut self, cmd: &ImportNetlist) -> Result<(), Vec<CheckResult>> {
        let report = self.update_netlist(cmd, false);
        if let Some(e) = &report.error {
            let why = e.clone();
            LAST_REPORT.with(|r| *r.borrow_mut() = Some(report));
            return Err(vec![CheckResult::fail("ops_bad_netlist", &cmd.name, why)]);
        }
        LAST_REPORT.with(|r| *r.borrow_mut() = Some(report));
        Ok(())
    }

    /// `DIALOG_IMPORT_NETLIST::loadNetlist`: the head lines, `ReadNetlistFromFile` (the reader and `LoadFootprints`' checks), `UpdateNetlist`.
    fn update_netlist(&mut self, cmd: &ImportNetlist, dry: bool) -> NetlistReport {
        let mut u = Updater::new(cmd, dry);
        u.head(Severity::Info, format!("Reading netlist file '{}'.", cmd.name));
        u.head(Severity::Info, if cmd.options.by_timestamp { "Using tstamps (unique IDs) to match symbols and footprints.".into() } else { "Using reference designators to match symbols and footprints.".into() });
        let netlist = match netlist_reader::read_netlist(&cmd.text, &cmd.name) {
            Ok(n) => n,
            Err(e) => {
                let mut r = u.finish_lines();
                r.error = Some(e);
                return r;
            }
        };
        if cmd.libraries {
            u.load_footprints(self, &netlist);
        }
        u.run(self, &netlist);
        u.finish_lines()
    }
}

/// `BOARD_NETLIST_UPDATER`'s state for one run.
struct Updater<'c> {
    cmd: &'c ImportNetlist,
    dry: bool,
    head: Vec<ReportLine>,
    lines: Vec<ReportLine>,
    warnings: usize,
    errors: usize,
    new_footprints: usize,
    /// The footprint each footprint is on now, where this run changed it (replaced, added): ref -> `Lib:Name`.
    fpid_now: BTreeMap<String, String>,
    /// The engine footprint of a footprint this run added or replaced.
    pads_now: BTreeMap<String, Footprint>,
    /// `m_padNets`: the dry run's pads' nets as the update would leave them (`getNetname`, which the zones read).
    pad_net_cache: BTreeMap<(String, String), String>,
    /// `m_addedNets`.
    added_nets: BTreeSet<String>,
    /// `m_oldToNewNets`.
    old_to_new: BTreeMap<String, String>,
    /// `m_schematicNetNames`.
    schematic_net_names: BTreeSet<String>,
    /// The nets marked current (`SetIsCurrent( true )`).
    current: BTreeSet<String>,
    /// The record, as this run leaves it.
    record: ImportedNetlist,
    /// The footprints added (`m_addedFootprints`).
    added: Vec<String>,
    /// Reference changes, old -> new (`SetReference`), made at the end.
    renames: BTreeMap<String, String>,
}

impl<'c> Updater<'c> {
    fn new(cmd: &'c ImportNetlist, dry: bool) -> Self {
        Updater {
            cmd,
            dry,
            head: Vec::new(),
            lines: Vec::new(),
            warnings: 0,
            errors: 0,
            new_footprints: 0,
            fpid_now: BTreeMap::new(),
            pads_now: BTreeMap::new(),
            pad_net_cache: BTreeMap::new(),
            added_nets: BTreeSet::new(),
            old_to_new: BTreeMap::new(),
            schematic_net_names: BTreeSet::new(),
            current: BTreeSet::new(),
            record: ImportedNetlist::default(),
            added: Vec::new(),
            renames: BTreeMap::new(),
        }
    }

    fn head(&mut self, severity: Severity, text: String) {
        self.head.push(ReportLine { severity, text });
    }

    fn say(&mut self, severity: Severity, text: String) {
        self.lines.push(ReportLine { severity, text });
    }

    fn warn(&mut self, text: String) {
        self.say(Severity::Warning, text);
        self.warnings += 1;
    }

    fn error(&mut self, text: String) {
        self.say(Severity::Error, text);
        self.errors += 1;
    }

    /// Present or past tense (`m_isDryRun`).
    fn tense(&self, dry: String, done: String) -> String {
        if self.dry {
            dry
        } else {
            done
        }
    }

    fn finish_lines(&mut self) -> NetlistReport {
        let mut lines = std::mem::take(&mut self.head);
        lines.append(&mut self.lines);
        NetlistReport { lines, added: self.added.clone(), warnings: self.warnings, errors: self.errors, error: None }
    }

    // ---- what the board has ----

    /// The footprints on the board: the placed ones, by reference.
    fn on_board(&self, b: &Board<'_>) -> Vec<OnBoard> {
        b.placement().footprints.iter().map(|f| OnBoard { reference: f.id.clone(), fpid: self.fpid_of(b, &f.id) }).collect()
    }

    /// `FOOTPRINT::GetFPID`, formatted.
    fn fpid_of(&self, b: &Board<'_>, reference: &str) -> String {
        if let Some(f) = self.fpid_now.get(reference) {
            return f.clone();
        }
        b.model.part(reference).map(eda_model::footprint::footprint_lib_id).unwrap_or_default()
    }

    /// The footprint's link to its symbol as it stands: this run's record, else the model's (the schematic's, or an earlier import's).
    fn link_of(&self, b: &Board<'_>, reference: &str) -> SymbolLink {
        self.record.footprint(reference).map(|f| f.link.clone()).or_else(|| b.model.symbol_links.get(reference).cloned()).unwrap_or_default()
    }

    /// The record's entry of a footprint, made from its link as it stands.
    fn entry<'s>(&'s mut self, b: &Board<'_>, reference: &str) -> &'s mut ImportedFootprint {
        if self.record.footprint(reference).is_none() {
            let link = b.model.symbol_links.get(reference).cloned().unwrap_or_default();
            self.record.footprints.push(ImportedFootprint { reference: reference.to_string(), link, value: None, filters: String::new() });
        }
        self.record.footprints.iter_mut().find(|f| f.reference == reference).expect("just made")
    }

    fn board_part<'b>(b: &'b Board<'_>, reference: &str) -> Option<&'b BoardPart> {
        b.design.drawings.as_ref()?.board_parts.iter().find(|p| p.reference == reference)
    }

    /// `FOOTPRINT::GetValue`.
    fn value_of(&self, b: &Board<'_>, reference: &str) -> String {
        if let Some(v) = self.record.footprint(reference).and_then(|f| f.value.clone()) {
            return v;
        }
        if let Some(bp) = Self::board_part(b, reference) {
            return bp.value.clone().unwrap_or_default();
        }
        b.model.part(reference).and_then(|p| p.value.clone()).unwrap_or_default()
    }

    fn locked(b: &Board<'_>, reference: &str) -> bool {
        b.design.drawings.as_ref().is_some_and(|d| d.locked_ids.iter().any(|l| l == reference))
    }

    /// The engine footprint the footprint is on now.
    fn footprint_of(&self, b: &Board<'_>, reference: &str) -> Option<Footprint> {
        if let Some(f) = self.pads_now.get(reference) {
            return Some(f.clone());
        }
        b.model.part(reference).and_then(|p| b.model.footprint_of(p))
    }

    /// The footprint's fabrication attributes (`GetAttributes`).
    fn attrs_of(&self, b: &Board<'_>, reference: &str) -> eda_model::fp_edit::FootprintAttrs {
        let fp = self.footprint_of(b, reference).unwrap_or_default();
        b.design.effective_attrs(reference, &fp)
    }

    /// `PAD::GetNetname` of a pad of the footprint (the update's, in a dry run, where it asks `getNetname`): its pin's net, or the `unconnected-(..)` net the
    /// board gives a pin on no net (`SCH_PIN::GetDefaultNetName`, as the writer names it) while its symbol draws it.
    fn pad_net(&self, b: &Board<'_>, reference: &str, number: &str, cached: bool) -> String {
        if cached && self.dry {
            if let Some(n) = self.pad_net_cache.get(&(reference.to_string(), number.to_string())) {
                return n.clone();
            }
        }
        let pin = format!("{reference}.{number}");
        if let Some(bp) = Self::board_part(b, reference) {
            if let Some((_, n)) = bp.pad_nets.iter().find(|(p, _)| p == number) {
                return n.clone();
            }
        } else if let Some(net) = b.design.nets.as_ref().unwrap_or(&b.model.nets).iter().find(|n| n.pins.iter().any(|p| *p == pin)) {
            return net.name.clone();
        }
        // a pin on no net: the board's own name for it, while its symbol draws it
        let link = self.link_of(b, reference);
        let has_link = self.record.footprint(reference).is_some() || b.model.symbol_links.contains_key(reference);
        if has_link && !link.pins.contains_key(number) {
            return String::new();
        }
        let Some(part) = b.model.part(reference) else { return String::new() };
        let Some(p) = part.pins.iter().find(|p| p.number == number) else { return String::new() };
        eda_engine::nets::unconnected_net_name(&pin, p.name.as_deref(), link.units.get(number).map(String::as_str))
    }

    /// `PAD::GetPinType` as the board has it (the link's, or a pad edit's).
    fn pin_type_of(&self, b: &Board<'_>, reference: &str, number: &str) -> String {
        let edit = b.design.footprint_edit(reference).and_then(|e| e.pad(number, 1)).and_then(|e| e.pin_type.clone());
        edit.or_else(|| self.link_of(b, reference).pins.get(number).map(|(_, t)| t.clone())).unwrap_or_default()
    }

    /// `BOARD::FindNet`: a net the board has.
    fn board_has_net(b: &Board<'_>, name: &str) -> bool {
        b.design.nets.as_ref().unwrap_or(&b.model.nets).iter().any(|n| n.name == name)
            || b.model.nets.iter().any(|n| n.name == name)
            || b.design.drawings.as_ref().is_some_and(|d| d.board_parts.iter().any(|p| p.pad_nets.iter().any(|(_, n)| n == name)) || d.board_nets.iter().any(|n| n == name))
    }

    /// Puts the pad `number` of the footprint on `net` (`""`: no net).
    fn set_pad_net(&mut self, b: &mut Board<'_>, reference: &str, number: &str, net: &str) {
        let pin = format!("{reference}.{number}");
        // a pin the board names `unconnected-(..)` by itself is left on no net: the board file says the same
        let own = {
            let link = self.link_of(b, reference);
            b.model.part(reference).and_then(|p| p.pins.iter().find(|p| p.number == number)).map(|p| eda_engine::nets::unconnected_net_name(&pin, p.name.as_deref(), link.units.get(number).map(String::as_str)))
        };
        let net = if own.as_deref() == Some(net) { "" } else { net };
        let is_board_part = Self::board_part(b, reference).is_some();
        if is_board_part {
            if let Some(bp) = b.drawings_mut().board_parts.iter_mut().find(|p| p.reference == reference) {
                bp.pad_nets.retain(|(p, _)| p != number);
                if !net.is_empty() {
                    bp.pad_nets.push((number.to_string(), net.to_string()));
                    bp.pad_nets.sort();
                }
            }
            return;
        }
        let nets = b.design.nets.get_or_insert_with(Vec::new);
        for n in nets.iter_mut() {
            n.pins.retain(|p| *p != pin);
        }
        if !net.is_empty() {
            match nets.iter_mut().find(|n| n.name == net) {
                Some(n) => {
                    n.pins.push(pin);
                    n.pins.sort();
                }
                None => nets.push(Net { name: net.to_string(), pins: vec![pin] }),
            }
        }
    }

    // ---- PCB_EDIT_FRAME::LoadFootprints ----

    /// `PCB_EDIT_FRAME::LoadFootprints`: before the update, each component (greatest footprint id first) that names no footprint, names another footprint than
    /// its footprint on the board has while "Replace footprints" is off, or names one no library has, is told.
    fn load_footprints(&mut self, b: &Board<'_>, netlist: &Netlist) {
        if netlist.components.is_empty() {
            return;
        }
        let mut last: Option<String> = None;
        let mut last_found = false;
        for c in netlist.sorted_by_fpid() {
            let (_, item) = netlist_reader::split_lib_id(&c.fpid);
            if item.is_empty() {
                self.say(Severity::Error, format!("No footprint defined for symbol {}.", c.reference));
                continue;
            }
            let on_board = self.find_on_board(b, c);
            let mismatch = on_board.as_ref().is_some_and(|fp| if !c.fpid.contains(':') { netlist_reader::split_lib_id(&fp.fpid).1 != item } else { fp.fpid != c.fpid });
            if mismatch && !self.cmd.options.replace_footprints {
                let fp = on_board.as_ref().expect("a mismatch has a footprint");
                self.say(Severity::Warning, format!("Footprint of {} changed: board footprint '{}', netlist footprint '{}'.", c.reference, fp.fpid, c.fpid));
                continue;
            }
            let mismatch = mismatch && self.cmd.options.replace_footprints;
            if on_board.is_some() && !mismatch {
                continue;
            }
            if last.as_deref() != Some(c.fpid.as_str()) {
                if self.library_footprint(b, &c.fpid).is_some() {
                    last = Some(c.fpid.clone());
                    last_found = true;
                } else {
                    last_found = false;
                    self.say(Severity::Error, format!("{} footprint '{}' not found in any libraries in the footprint library table.", c.reference, item));
                    continue;
                }
            } else if !last_found {
                continue;
            }
        }
    }

    /// `FindFootprintByPath` / `FindFootprintByReference` of `LoadFootprints`.
    fn find_on_board(&self, b: &Board<'_>, c: &NetlistComponent) -> Option<OnBoard> {
        let fps = self.on_board(b);
        if self.cmd.options.by_timestamp {
            c.kiids.iter().find_map(|k| {
                let path = netlist_reader::path_string(c.path.iter().chain(std::iter::once(k)));
                fps.iter().find(|f| self.link_of(b, &f.reference).path == path).cloned()
            })
        } else {
            fps.into_iter().find(|f| f.reference == c.reference)
        }
    }

    /// `m_frame->LoadFootprint( aFootprintId )`: the footprint the libraries (or the board's model) have under that id, as the engine reads it and as the
    /// library holds it (`None` for a footprint the model already knows), and the `Lib:Name` it is known by.
    fn library_footprint(&self, b: &Board<'_>, fpid: &str) -> Option<(String, Footprint, Option<LibraryFootprint>)> {
        let name = if fpid.contains(':') { fpid.to_string() } else { self.cmd.found_as.get(fpid).cloned().unwrap_or_else(|| fpid.to_string()) };
        if let Some(def) = self.cmd.definitions.get(&name) {
            let mut fp = self.cmd.library.get(&name).cloned().unwrap_or_else(|| def.to_engine_footprint());
            fp.name = name.clone();
            return Some((name, fp, Some(def.clone())));
        }
        let probe = Part { reference: "?".into(), mpn: None, lcsc: None, value: None, package: None, footprint: Some(name.clone()), symbol: None, datasheet: None, pins: vec![], body_nm: None, edge: None };
        if let Some(mut fp) = self.cmd.library.get(&name).cloned().or_else(|| b.model.footprint_of(&probe)) {
            fp.name = name.clone();
            return Some((name, fp, None));
        }
        None
    }

    // ---- BOARD_NETLIST_UPDATER::UpdateNetlist ----

    fn run(&mut self, b: &mut Board<'_>, netlist: &Netlist) {
        self.record = b.design.drawings.as_ref().and_then(|d| d.imported_netlist.clone()).unwrap_or_default();
        self.record.source = self.cmd.name.clone();
        if !self.dry {
            // The board's nets in the net list in force, which the update writes (a footprint only the board has keeps its own), and the schematic the studio
            // shows stored as it is, so that nothing the update writes reaches it (see the module doc).
            if b.design.schematic.is_none() {
                if let Ok(derived) = eda_engine::derive_schematic_modules_with(b.model, &eda_engine::EngineOptions::default(), b.design.placement.as_ref().map(|p| p.modules.as_slice()).unwrap_or(&[])) {
                    let mut derived = derived;
                    if let Some(dr) = b.design.drawings.as_ref() {
                        derived.sync_symbol_bom_flags(&dr.footprint_edits);
                    }
                    b.design.schematic = derived.schematic;
                    b.design.sheet_contents = derived.sheet_contents;
                }
            }
            let board_parts: BTreeSet<String> = b.design.drawings.as_ref().map(|d| d.board_parts.iter().map(|p| p.reference.clone()).collect()).unwrap_or_default();
            let mut nets = b.design.nets.clone().unwrap_or_else(|| b.model.nets.clone());
            for n in nets.iter_mut() {
                n.pins.retain(|p| p.split_once('.').is_none_or(|(r, _)| !board_parts.contains(r)));
            }
            b.design.nets = Some(nets);
        }
        // `cacheCopperZoneConnections`: the pads of each copper zone's net
        let zone_pads: BTreeMap<String, Vec<(String, String)>> = self.zone_pads(b);
        let pre_existing = self.on_board(b);
        // the board's nets as it is (`GetNetInfo()`): the model's, the ones only footprints of the board have, the `unconnected-(..)` nets of the pins on none
        let mut nets_before: BTreeSet<String> = b.model.nets.iter().map(|n| n.name.clone()).collect();
        nets_before.extend(b.design.drawings.iter().flat_map(|d| d.board_nets.iter().cloned().chain(d.board_parts.iter().flat_map(|p| p.pad_nets.iter().map(|(_, n)| n.clone())))));
        for fp in &pre_existing {
            for p in self.footprint_of(b, &fp.reference).map(|f| f.pads).unwrap_or_default() {
                let n = self.pad_net(b, &fp.reference, &p.number, false);
                if !n.is_empty() {
                    nets_before.insert(n);
                }
            }
        }
        // "Collect all schematic net names so NC pad deduplication can avoid collisions"
        for c in &netlist.components {
            for n in &c.nets {
                self.schematic_net_names.insert(n.net.clone());
            }
        }
        let mut used: BTreeSet<String> = BTreeSet::new();
        let mut footprint_map: BTreeMap<usize, String> = BTreeMap::new();
        for (ci, c) in netlist.components.iter().enumerate() {
            if c.has_property("exclude_from_board") {
                continue;
            }
            self.say(Severity::Info, format!("Processing symbol '{}:{}'.", c.reference, esc(&c.fpid)));
            let base = c.fpid.clone();
            let legacy = !base.is_empty() && !base.contains(':');
            if legacy {
                self.warn(format!("Warning: {} footprint '{}' is missing a library name. Use the full 'Library:Footprint' format to avoid repeated update notifications.", c.reference, esc(&base)));
            }
            let matching: Vec<OnBoard> = pre_existing
                .iter()
                .filter(|fp| {
                    if self.cmd.options.by_timestamp {
                        let path = self.link_of(b, &fp.reference).path;
                        c.kiids.iter().any(|k| netlist_reader::path_string(c.path.iter().chain(std::iter::once(k))) == path)
                    } else {
                        fp.reference.to_lowercase() == c.reference.to_lowercase()
                    }
                })
                .map(|fp| OnBoard { reference: fp.reference.clone(), fpid: self.fpid_of(b, &fp.reference) })
                .collect();
            // "When the schematic-side FPID has no library nickname ... matching should compare only the footprint item name."
            let fpid_matches = |board: &str, expected: &str| if !expected.contains(':') { netlist_reader::split_lib_id(board).1 == expected } else { board == expected };
            let take = |fpid: &str, used: &BTreeSet<String>| matching.iter().find(|fp| !used.contains(&fp.reference) && fpid_matches(&fp.fpid, fpid)).map(|f| f.reference.clone());
            let mut base_fp: Option<String> = if !base.is_empty() { take(&base, &used) } else { matching.first().map(|f| f.reference.clone()) };
            if base_fp.is_none() && self.cmd.options.replace_footprints && !matching.is_empty() {
                let candidate = matching.iter().find(|fp| !used.contains(&fp.reference) && !(!base.is_empty() && fpid_matches(&fp.fpid, &base))).map(|f| f.reference.clone());
                if let Some(candidate) = candidate {
                    let replaced = self.replace_footprint(b, &candidate, c);
                    base_fp = Some(replaced.unwrap_or(candidate));
                }
            }
            if base_fp.is_none() {
                base_fp = self.add_new_footprint(b, c, &base);
            }
            let Some(fp) = base_fp else { continue };
            used.insert(fp.clone());
            footprint_map.insert(ci, fp.clone());
            self.update_footprint_parameters(b, &fp, c);
            self.update_footprint_group(b, &fp, c, netlist);
            self.update_component_pad_connections(b, &fp, c);
        }
        self.update_copper_zone_nets(b, netlist, &zone_pads);
        self.update_groups(b, netlist);

        // "Finally go through the board footprints and update all those that *don't* have matching component entries."
        for fp in &pre_existing {
            let mut do_delete = self.cmd.options.delete_extra_footprints;
            if self.attrs_of(b, &fp.reference).board_only {
                do_delete = false;
            }
            let mut stale_variant = false;
            let matched = if used.contains(&fp.reference) {
                true
            } else {
                let component = if self.cmd.options.by_timestamp { netlist.by_path(&self.link_of(b, &fp.reference).path) } else { netlist.by_reference(&fp.reference) };
                match component.filter(|c| !c.has_property("exclude_from_board")) {
                    Some(c) if self.cmd.options.replace_footprints && !c.variants.is_empty() => {
                        stale_variant = true;
                        false
                    }
                    Some(_) => true,
                    None => false,
                }
            };
            if stale_variant {
                do_delete = true;
            }
            if do_delete && !matched && Self::locked(b, &fp.reference) && !self.cmd.options.override_locks {
                let text = self.tense(format!("Cannot remove unused footprint {} (footprint is locked).", fp.reference), format!("Could not remove unused footprint {} (footprint is locked).", fp.reference));
                self.warn(text);
                do_delete = false;
            }
            if do_delete && !matched {
                if !self.dry {
                    let _ = b.rip(&fp.reference);
                    self.record.footprints.retain(|f| f.reference != fp.reference);
                }
                let text = self.tense(format!("Remove unused footprint {}.", fp.reference), format!("Removed unused footprint {}.", fp.reference));
                self.say(Severity::Action, text);
            } else if !self.dry {
                if !matched {
                    // `footprint->SetPath( KIID_PATH() )`
                    if !self.link_of(b, &fp.reference).path.is_empty() {
                        self.entry(b, &fp.reference).link.path = String::new();
                    }
                }
                let numbers: Vec<String> = self.footprint_of(b, &fp.reference).map(|f| f.pads.iter().map(|p| p.number.clone()).collect()).unwrap_or_default();
                for n in numbers {
                    let net = self.pad_net(b, &fp.reference, &n, false);
                    if !net.is_empty() {
                        self.current.insert(net);
                    }
                }
            }
        }

        if !self.dry {
            self.test_connectivity(b, netlist, &footprint_map);
            // "Removed unused net %s." for every net the board had that no pad is on now
            let removed: Vec<String> = nets_before.iter().filter(|n| !self.current.contains(*n) && !self.added_nets.contains(*n)).cloned().collect();
            for n in &removed {
                self.say(Severity::Action, format!("Removed unused net {}.", esc(&unesc(n))));
            }
            if let Some(nets) = b.design.nets.as_mut() {
                nets.retain(|n| !removed.contains(&n.name));
            }
            if let Some(d) = b.design.drawings.as_mut() {
                d.board_nets.retain(|n| !removed.contains(n));
            }
            self.finish_record(b);
        }
        // the tail: two empty lines, then the totals
        self.say(Severity::Action, String::new());
        self.say(Severity::Action, String::new());
        let (w, e) = (self.warnings, self.errors);
        self.say(Severity::Info, format!("Total warnings: {w}, errors: {e}."));
    }

    /// The record written back, and the references renamed (`SetReference`), last.
    fn finish_record(&mut self, b: &mut Board<'_>) {
        let renames = std::mem::take(&mut self.renames);
        if !renames.is_empty() {
            b.rename_part(&renames);
            for f in self.record.footprints.iter_mut() {
                if let Some(n) = renames.get(&f.reference) {
                    f.reference = n.clone();
                }
            }
            for a in self.added.iter_mut() {
                if let Some(n) = renames.get(a) {
                    *a = n.clone();
                }
            }
        }
        // an entry that says nothing the model would not say is dropped
        let model = b.model;
        self.record.footprints.retain(|f| f.value.is_some() || !f.filters.is_empty() || model.symbol_links.get(&f.reference) != Some(&f.link));
        self.record.footprints.sort_by(|a, b| a.reference.cmp(&b.reference));
        self.record.footprints.dedup_by(|a, b| a.reference == b.reference);
        self.record.nets.sort();
        self.record.nets.dedup();
        let record = std::mem::take(&mut self.record);
        let empty = record.footprints.is_empty() && record.nets.is_empty();
        b.drawings_mut().imported_netlist = (!empty).then_some(record);
    }

    /// `cacheCopperZoneConnections`: each copper zone's pads -- here, the pads on its net (KiCad: the pads its fill touches).
    fn zone_pads(&self, b: &Board<'_>) -> BTreeMap<String, Vec<(String, String)>> {
        let mut out = BTreeMap::new();
        let Some(rt) = b.design.routing.as_ref() else { return out };
        for z in rt.zones.iter().filter(|z| !z.is_rule_area && z.layer_set().iter().any(|l| l.ends_with(".Cu")) && !z.net.is_empty()) {
            let mut pads = Vec::new();
            for f in &b.placement().footprints {
                for p in self.footprint_of(b, &f.id).map(|fp| fp.pads).unwrap_or_default() {
                    if self.pad_net(b, &f.id, &p.number, false) == z.net && !pads.contains(&(f.id.clone(), p.number.clone())) {
                        pads.push((f.id.clone(), p.number.clone()));
                    }
                }
            }
            out.insert(z.id.clone(), pads);
        }
        out
    }

    /// `BOARD_NETLIST_UPDATER::replaceFootprint`. Returns the footprint that stands for the component afterwards (the same reference, put on the new footprint),
    /// `None` when it was not changed (a dry run, a footprint no library has, a locked one).
    fn replace_footprint(&mut self, b: &mut Board<'_>, reference: &str, c: &NetlistComponent) -> Option<String> {
        if c.fpid.is_empty() {
            self.error(format!("Cannot update {} (no footprint assigned).", c.reference));
            return None;
        }
        let Some((name, new, _)) = self.library_footprint(b, &c.fpid) else {
            self.error(format!("Cannot update {} (footprint '{}' not found).", c.reference, esc(&c.fpid)));
            return None;
        };
        let from = self.fpid_of(b, reference);
        if Self::locked(b, reference) && !self.cmd.options.override_locks {
            let text = self.tense(
                format!("Cannot change {} footprint from '{}' to '{}' (footprint is locked).", reference, esc(&from), esc(&c.fpid)),
                format!("Could not change {} footprint from '{}' to '{}' (footprint is locked).", reference, esc(&from), esc(&c.fpid)),
            );
            self.warn(text);
            return None;
        }
        if self.dry {
            self.say(Severity::Action, format!("Change {} footprint from '{}' to '{}'.", reference, esc(&from), esc(&c.fpid)));
            self.new_footprints += 1;
            return None;
        }
        // `ExchangeFootprint( aFootprint, newFootprint, m_commit, true )`: the studio's exchange, every option on. It changes the board and not the symbol.
        let kept = (b.design.schematic.clone(), b.design.sheet_contents.clone());
        let library: BTreeMap<String, Footprint> = [(name.clone(), new.clone())].into_iter().collect();
        let items: BTreeMap<String, FootprintItems> = self.cmd.library_items.get(&name).map(|i| [(name.clone(), i.clone())].into_iter().collect()).unwrap_or_default();
        if b.exchange_footprints(&[reference.to_string()], Some(&name), &crate::fp_exchange::ExchangeOptions::replace(), &library, &items).is_err() {
            self.error(format!("Cannot update {} (footprint '{}' not found).", c.reference, esc(&c.fpid)));
            return None;
        }
        (b.design.schematic, b.design.sheet_contents) = kept;
        self.fpid_now.insert(reference.to_string(), name.clone());
        self.pads_now.insert(reference.to_string(), new);
        self.say(Severity::Action, format!("Changed {} footprint from '{}' to '{}'.", reference, esc(&from), esc(&c.fpid)));
        self.new_footprints += 1;
        Some(reference.to_string())
    }

    /// `BOARD_NETLIST_UPDATER::estimateFootprintInsertionPosition`: below the board's edges by 10 mm, at their centre; the page's centre on an empty board.
    fn insertion_position(b: &Board<'_>) -> Point {
        let outline = &b.placement().outline;
        if !outline.is_empty() {
            let (x0, x1) = (outline.iter().map(|p| p.x).min().unwrap_or_default(), outline.iter().map(|p| p.x).max().unwrap_or_default());
            let y1 = outline.iter().map(|p| p.y).max().unwrap_or_default();
            return Point { x: Nm::new((x0.get() + x1.get()) / 2), y: y1 + Nm::new(10_000_000) };
        }
        // A4 landscape, 297 x 210 mm
        Point { x: Nm::new(148_500_000), y: Nm::new(105_000_000) }
    }

    /// `BOARD_NETLIST_UPDATER::addNewFootprint`. Returns the footprint that stands for the component (in a dry run, none: KiCad's `delete footprint`).
    fn add_new_footprint(&mut self, b: &mut Board<'_>, c: &NetlistComponent, fpid: &str) -> Option<String> {
        if fpid.is_empty() {
            self.error(format!("Cannot add {} (no footprint assigned).", c.reference));
            return None;
        }
        let Some((name, engine, definition)) = self.library_footprint(b, fpid) else {
            self.error(format!("Cannot add {} (footprint '{}' not found).", c.reference, esc(fpid)));
            return None;
        };
        let reference = c.reference.clone();
        // one footprint per reference here: a placed footprint the netlist did not match keeps it
        if b.pose_of(&reference).is_some() {
            self.error(format!("Cannot add {} (a footprint with this reference is on the board).", c.reference));
            return None;
        }
        if self.dry {
            self.say(Severity::Action, format!("Add {} (footprint '{}').", c.reference, esc(fpid)));
            self.new_footprints += 1;
            return None;
        }
        let at = self.cmd.at.unwrap_or_else(|| Self::insertion_position(b));
        let intent_part = b.model.part(&reference).is_some() && Self::board_part(b, &reference).is_none();
        if intent_part {
            // the part of the design that is not on the board yet: it is placed, on the footprint the netlist names
            if self.fpid_of(b, &reference) != name {
                let kept = (b.design.schematic.clone(), b.design.sheet_contents.clone());
                let library: BTreeMap<String, Footprint> = [(name.clone(), engine.clone())].into_iter().collect();
                let items: BTreeMap<String, FootprintItems> = self.cmd.library_items.get(&name).map(|i| [(name.clone(), i.clone())].into_iter().collect()).unwrap_or_default();
                let _ = b.exchange_footprints(std::slice::from_ref(&reference), Some(&name), &crate::fp_exchange::ExchangeOptions::replace(), &library, &items);
                (b.design.schematic, b.design.sheet_contents) = kept;
            }
            // its pins start on no net (`pad->SetNetCode( 0 )`): the update gives them theirs
            if let Some(nets) = b.design.nets.as_mut() {
                let prefix = format!("{reference}.");
                for n in nets.iter_mut() {
                    n.pins.retain(|p| !p.starts_with(&prefix));
                }
            }
        } else {
            let known = b.model.footprints.iter().any(|f| f.name == name);
            let stored = if known { None } else { definition.clone().or_else(|| Some(LibraryFootprint::from_engine_footprint(&engine))) };
            let dr = b.drawings_mut();
            dr.board_parts.retain(|p| p.reference != reference);
            dr.board_parts.push(BoardPart { reference: reference.clone(), value: Some(netlist_reader::split_lib_id(&name).1.to_string()), footprint: name.clone(), definition: stored, pad_nets: Vec::new() });
            dr.board_parts.sort_by(|a, b| a.reference.cmp(&b.reference));
            // a new footprint's own fields (the library's Datasheet and Description), which the update compares the component's with
            let fields = vec![("Datasheet".to_string(), String::new()), ("Description".to_string(), definition.as_ref().map(|d| d.description.clone()).unwrap_or_default())];
            self.record.footprints.retain(|f| f.reference != reference);
            self.record.footprints.push(ImportedFootprint { reference: reference.clone(), link: SymbolLink { fields, ..Default::default() }, value: None, filters: String::new() });
        }
        b.design.placement.as_mut().expect("a Board always carries a placement section").footprints.push(FootprintInstance { id: reference.clone(), at, rot: 0, side: Side::Top, label: LabelSide::default() });
        b.sort_footprints();
        self.fpid_now.insert(reference.clone(), name.clone());
        self.pads_now.insert(reference.clone(), engine);
        self.added.push(reference.clone());
        self.say(Severity::Action, format!("Added {} (footprint '{}').", c.reference, esc(fpid)));
        self.new_footprints += 1;
        Some(reference)
    }

    /// The reference a footprint is shown by in the messages of the rest of its update: the netlist's once the update renamed it (KiCad's `SetReference`
    /// comes first), its own in a dry run.
    fn shown<'s>(&'s self, reference: &'s str) -> &'s str {
        self.renames.get(reference).map(String::as_str).unwrap_or(reference)
    }

    /// `BOARD_NETLIST_UPDATER::updateFootprintParameters`.
    fn update_footprint_parameters(&mut self, b: &mut Board<'_>, reference: &str, c: &NetlistComponent) {
        let fp_fpid = self.fpid_of(b, reference);
        // `firstAssociatedVariant`: a design variant that names the footprint the board has
        let variant = if fp_fpid != c.fpid { c.variants.iter().find(|v| v.fields.iter().any(|(n, val)| n == "Footprint" && *val == fp_fpid)).cloned() } else { None };
        let added_now = self.added.iter().any(|a| a == reference);
        // the reference: a new footprint has the library's (`REF**`)
        let fp_ref = if added_now && Self::board_part(b, reference).is_some() { "REF**".to_string() } else { reference.to_string() };
        if fp_ref != c.reference {
            if self.dry {
                self.say(Severity::Action, format!("Change {} reference designator to {}.", fp_ref, c.reference));
            } else {
                self.say(Severity::Action, format!("Changed {} reference designator to {}.", fp_ref, c.reference));
                if !added_now {
                    let taken = b.pose_of(&c.reference).is_some() && !self.renames.contains_key(&c.reference);
                    if taken {
                        self.error(format!("Cannot change {} reference designator to {} (a footprint called {} is already on the board).", fp_ref, c.reference, c.reference));
                    } else {
                        self.renames.insert(reference.to_string(), c.reference.clone());
                    }
                }
            }
        }
        let shown = self.shown(reference).to_string();
        // the value
        let netlist_value = variant.as_ref().and_then(|v| v.fields.iter().find(|(n, _)| n == "Value").map(|(_, v)| v.clone())).unwrap_or_else(|| c.value.clone());
        let fp_value = self.value_of(b, reference);
        if fp_value != netlist_value {
            let text = self.tense(format!("Change {} value from {} to {}.", shown, esc(&fp_value), esc(&netlist_value)), format!("Changed {} value from {} to {}.", shown, esc(&fp_value), esc(&netlist_value)));
            self.say(Severity::Action, text);
            if !self.dry {
                if Self::board_part(b, reference).is_some() {
                    if let Some(bp) = b.drawings_mut().board_parts.iter_mut().find(|p| p.reference == reference) {
                        bp.value = Some(netlist_value.clone());
                    }
                } else {
                    let own = b.model.part(reference).and_then(|p| p.value.clone()).unwrap_or_default();
                    self.entry(b, reference).value = (own != netlist_value).then(|| netlist_value.clone());
                }
            }
        }
        // the path (`GetPath`, "symbol association")
        let new_path = c.footprint_path();
        let fp_path = self.link_of(b, reference).path;
        if fp_path != new_path {
            let text = self.tense(
                format!("Update {} symbol association from {} to {}.", shown, esc(&fp_path), esc(&new_path)),
                format!("Updated {} symbol association from {} to {}.", shown, esc(&fp_path), esc(&new_path)),
            );
            self.say(Severity::Action, text);
            if !self.dry {
                self.entry(b, reference).link.path = new_path;
            }
        }
        // the fields: the footprint's (but the Reference, the Value and the component class), the component's (but the Reference, the Value, the Footprint
        // and the component class), checked in the symbol's order
        let mut fp_fields: Vec<(String, String)> = self.link_of(b, reference).fields;
        if let Some(e) = b.design.footprint_edit(reference) {
            for f in &e.fields {
                match fp_fields.iter_mut().find(|(n, _)| *n == f.name) {
                    Some(x) => x.1 = f.text.clone(),
                    None => fp_fields.push((f.name.clone(), f.text.clone())),
                }
            }
        }
        let mut comp_fields: Vec<(String, String)> = c.fields.iter().filter(|(n, _)| !matches!(n.as_str(), "Reference" | "Value" | "Footprint" | "Component Class")).cloned().collect();
        if let Some(v) = &variant {
            for (n, val) in &v.fields {
                match comp_fields.iter_mut().find(|(x, _)| x == n) {
                    Some(x) => x.1 = val.clone(),
                    None => comp_fields.push((n.clone(), val.clone())),
                }
            }
        }
        let get = |fields: &[(String, String)], name: &str| fields.iter().find(|(n, _)| n == name).map(|(_, v)| v.clone());
        let remove_only = comp_fields.iter().all(|(n, v)| get(&fp_fields, n).as_ref() == Some(v));
        let same = remove_only && fp_fields.iter().all(|(n, _)| get(&comp_fields, n).is_some());
        // `m_updateFields` is on, `m_removeExtraFields` off (the dialog sets the one and not the other)
        if !same && !remove_only {
            let text = self.tense(format!("Update {shown} fields."), format!("Updated {shown} fields."));
            self.say(Severity::Action, text);
            if !self.dry {
                let entry = self.entry(b, reference);
                for (n, v) in &comp_fields {
                    match entry.link.fields.iter_mut().find(|(x, _)| x == n) {
                        Some(x) => x.1 = v.clone(),
                        None => entry.link.fields.push((n.clone(), v.clone())),
                    }
                }
            }
        }
        // the sheet name and file, the footprint filters
        let sheetname = if !c.human_sheet_path.is_empty() { c.human_sheet_path.clone() } else { c.properties.get("Sheetname").cloned().unwrap_or_default() };
        let sheetfile = c.properties.get("Sheetfile").cloned().unwrap_or_default();
        let filters = c.properties.get("ki_fp_filters").cloned().unwrap_or_default();
        let link = self.link_of(b, reference);
        let fp_sheetfile = if link.sheetfile.is_empty() && !link.path.is_empty() { self.cmd.root_sheetfile.clone() } else { link.sheetfile.clone() };
        if sheetname != link.sheetname {
            let text = self.tense(format!("Update {} sheetname to '{}'.", shown, esc(&sheetname)), format!("Updated {} sheetname to '{}'.", shown, esc(&sheetname)));
            self.say(Severity::Action, text);
            if !self.dry {
                self.entry(b, reference).link.sheetname = sheetname.clone();
            }
        }
        if sheetfile != fp_sheetfile {
            let text = self.tense(format!("Update {} sheetfile to '{}'.", shown, esc(&sheetfile)), format!("Updated {} sheetfile to '{}'.", shown, esc(&sheetfile)));
            self.say(Severity::Action, text);
            if !self.dry {
                self.entry(b, reference).link.sheetfile = sheetfile.clone();
            }
        }
        let fp_filters = self.record.footprint(reference).map(|f| f.filters.clone()).unwrap_or_default();
        if filters != fp_filters {
            let text = self.tense(format!("Update {} footprint filters to '{}'.", shown, esc(&filters)), format!("Updated {} footprint filters to '{}'.", shown, esc(&filters)));
            self.say(Severity::Action, text);
            if !self.dry {
                self.entry(b, reference).filters = filters;
            }
        }
        // the fabrication attributes
        let mut attrs = self.attrs_of(b, reference);
        let was = attrs;
        let v = variant.as_ref();
        let wants = [
            ("exclude from BOM", v.and_then(|v| v.excluded_from_bom).unwrap_or(c.has_property("exclude_from_bom")), attrs.exclude_from_bom),
            ("Do not place", v.and_then(|v| v.dnp).unwrap_or(c.has_property("dnp")), attrs.dnp),
            ("exclude from position files", v.and_then(|v| v.excluded_from_pos_files).unwrap_or(c.has_property("exclude_from_pos_files")), attrs.exclude_from_pos_files),
        ];
        for (i, (label, want, have)) in wants.into_iter().enumerate() {
            if want == have {
                continue;
            }
            let text = match (self.dry, want) {
                (true, true) => format!("Add {shown} '{label}' fabrication attribute."),
                (true, false) => format!("Remove {shown} '{label}' fabrication attribute."),
                (false, true) => format!("Added {shown} '{label}' fabrication attribute."),
                (false, false) => format!("Removed {shown} '{label}' fabrication attribute."),
            };
            self.say(Severity::Action, text);
            if !self.dry {
                match i {
                    0 => attrs.exclude_from_bom = want,
                    1 => attrs.dnp = want,
                    _ => attrs.exclude_from_pos_files = want,
                }
            }
        }
        if !self.dry && attrs != was {
            let dr = b.drawings_mut();
            match dr.footprint_edits.iter_mut().find(|e| e.id == reference) {
                Some(e) => e.attrs = Some(attrs),
                None => {
                    let mut e = eda_model::fp_edit::FootprintEdit::new(reference);
                    e.attrs = Some(attrs);
                    dr.footprint_edits.push(e);
                    dr.footprint_edits.sort_by(|a, b| a.id.cmp(&b.id));
                }
            }
        }
    }

    /// `BOARD_NETLIST_UPDATER::updateFootprintGroup` ("Group footprints based on symbol group"): the footprint leaves a group the component is not in and joins
    /// the one it is in, made with the netlist group's uuid as its id when the board has none of it. A group left with fewer than two items goes.
    fn update_footprint_group(&mut self, b: &mut Board<'_>, reference: &str, c: &NetlistComponent, netlist: &Netlist) {
        if !self.cmd.options.transfer_groups {
            return;
        }
        let new_group = c.group.and_then(|g| netlist.groups.get(g));
        let new_id = new_group.map(|g| g.uuid.clone()).unwrap_or_default();
        let groups = b.design.drawings.as_ref().map(|d| d.groups.clone()).unwrap_or_default();
        let existing = groups.iter().find(|g| g.member_ids.iter().any(|m| m == reference)).cloned();
        let existing_id = existing.as_ref().map(|g| g.id.clone()).unwrap_or_default();
        if new_id == existing_id {
            return;
        }
        let shown = self.shown(reference).to_string();
        if let Some(g) = &existing {
            let text = self.tense(format!("Remove {} from group '{}'.", shown, esc(&g.name)), format!("Removed {} from group '{}'.", shown, esc(&g.name)));
            self.say(Severity::Action, text);
            if !self.dry {
                let dr = b.drawings_mut();
                if let Some(eg) = dr.groups.iter_mut().find(|x| x.id == g.id) {
                    eg.member_ids.retain(|m| m != reference);
                }
                dr.groups.retain(|x| x.id != g.id || x.member_ids.len() >= 2);
                // a group that holds this group loses it too, as `RemoveAll` leaves it
                let gone: BTreeSet<String> = groups.iter().map(|x| x.id.clone()).filter(|id| !dr.groups.iter().any(|x| &x.id == id)).collect();
                for x in dr.groups.iter_mut() {
                    x.member_ids.retain(|m| !gone.contains(m));
                }
            }
        }
        if let Some(ng) = new_group {
            let text = self.tense(format!("Add {} to group '{}'.", shown, esc(&ng.name)), format!("Added {} to group '{}'.", shown, esc(&ng.name)));
            self.say(Severity::Action, text);
            if !self.dry {
                let dr = b.drawings_mut();
                match dr.groups.iter_mut().find(|x| x.id == ng.uuid) {
                    Some(g) => g.member_ids.push(reference.to_string()),
                    None => dr.groups.push(Group { id: ng.uuid.clone(), name: ng.name.clone(), member_ids: vec![reference.to_string()], design_block: String::new() }),
                }
            }
        }
    }

    /// `BOARD_NETLIST_UPDATER::updateGroups`: a board group of a netlist group's uuid takes its name and design block link.
    fn update_groups(&mut self, b: &mut Board<'_>, netlist: &Netlist) {
        if !self.cmd.options.transfer_groups {
            return;
        }
        let groups = b.design.drawings.as_ref().map(|d| d.groups.clone()).unwrap_or_default();
        for g in groups {
            let Some(ng) = netlist.group_by_uuid(&g.id) else { continue };
            if ng.name != g.name {
                let text = self.tense(format!("Change group name from '{}' to '{}'.", esc(&g.name), esc(&ng.name)), format!("Changed group name from '{}' to '{}'.", esc(&g.name), esc(&ng.name)));
                self.say(Severity::Action, text);
                if !self.dry {
                    if let Some(x) = b.drawings_mut().groups.iter_mut().find(|x| x.id == g.id) {
                        x.name = ng.name.clone();
                    }
                }
            }
            if ng.lib_id != g.design_block {
                let text = self.tense(
                    format!("Change group library link from '{}' to '{}'.", esc(&g.design_block), esc(&ng.lib_id)),
                    format!("Changed group library link from '{}' to '{}'.", esc(&g.design_block), esc(&ng.lib_id)),
                );
                self.say(Severity::Action, text);
                if !self.dry {
                    if let Some(x) = b.drawings_mut().groups.iter_mut().find(|x| x.id == g.id) {
                        x.design_block = ng.lib_id.clone();
                    }
                }
            }
        }
    }

    /// `BOARD_NETLIST_UPDATER::updateComponentPadConnections`.
    fn update_component_pad_connections(&mut self, b: &mut Board<'_>, reference: &str, c: &NetlistComponent) {
        let shown = self.shown(reference).to_string();
        let pads = self.footprint_of(b, reference).map(|f| f.pads).unwrap_or_default();
        let mut pad_netnames: BTreeSet<String> = BTreeSet::new();
        let mut seen: BTreeSet<String> = BTreeSet::new();
        for pad in pads {
            // one pin per number here: the first pad of a number stands for every pad that shares it
            if !seen.insert(pad.number.clone()) {
                continue;
            }
            let num = pad.number.clone();
            let net: ComponentNet = c.net(&num);
            let (pin_function, pin_type) = if net.is_valid() { (net.pin_function.clone(), net.pin_type.clone()) } else { (String::new(), String::new()) };
            let copper = pad.kind != PadKind::NonPlatedHole;
            let had_type = self.pin_type_of(b, reference, &num);
            // (the link lists every pin the netlist gives a net, as the netlist the board is otherwise written from does: a pin it does not list is one no
            // symbol draws, and its pad is on no net)
            if !self.dry {
                let entry = self.entry(b, reference);
                if net.is_valid() {
                    entry.link.pins.insert(num.clone(), (pin_function.clone(), pin_type.clone()));
                } else {
                    entry.link.pins.remove(&num);
                }
            }
            let cur = self.pad_net(b, reference, &num, false);
            if !net.is_valid() || !copper {
                if !cur.is_empty() {
                    let text = self.tense(format!("Disconnect {} pin {}.", shown, esc(&num)), format!("Disconnected {} pin {}.", shown, esc(&num)));
                    self.say(Severity::Action, text);
                } else if copper && !num.is_empty() {
                    self.warn(format!("No net found for component {} pad {} (no pin {} in symbol).", shown, esc(&num), esc(&num)));
                }
                if !self.dry {
                    self.set_pad_net(b, reference, &num, "");
                    self.entry(b, reference).link.pins.remove(&num);
                } else {
                    self.pad_net_cache.insert((reference.to_string(), num.clone()), String::new());
                }
                continue;
            }
            let mut net_name = net.net.clone();
            // `pad->IsNoConnectPad()`: the pad's pin type as it stands (the netlist's, once the update set it)
            let no_connect = if self.dry { had_type.contains("no_connect") } else { pin_type.contains("no_connect") };
            if no_connect {
                let mut jj = 1;
                while !pad_netnames.insert(net_name.clone()) || (net_name != net.net && self.schematic_net_names.contains(&net_name)) {
                    net_name = format!("{}_{}", net.net, jj);
                    jj += 1;
                }
            }
            let found = Self::board_has_net(b, &net_name);
            if found && !self.dry {
                self.current.insert(net_name.clone());
            }
            if cur != net_name {
                if !found && !self.added_nets.contains(&net_name) {
                    self.added_nets.insert(net_name.clone());
                    self.say(Severity::Action, format!("Add net {}.", esc(&unesc(&net_name))));
                }
                if !cur.is_empty() {
                    self.old_to_new.insert(cur.clone(), net_name.clone());
                    let text = self.tense(
                        format!("Reconnect {} pin {} from {} to {}.", shown, esc(&num), esc(&unesc(&cur)), esc(&unesc(&net_name))),
                        format!("Reconnected {} pin {} from {} to {}.", shown, esc(&num), esc(&unesc(&cur)), esc(&unesc(&net_name))),
                    );
                    self.say(Severity::Action, text);
                } else {
                    let text = self.tense(format!("Connect {} pin {} to {}.", shown, esc(&num), esc(&unesc(&net_name))), format!("Connected {} pin {} to {}.", shown, esc(&num), esc(&unesc(&net_name))));
                    self.say(Severity::Action, text);
                }
                if !self.dry {
                    self.set_pad_net(b, reference, &num, &net_name);
                } else {
                    self.pad_net_cache.insert((reference.to_string(), num.clone()), net_name.clone());
                }
            }
            if !self.dry {
                self.record.nets.push(net_name.clone());
                self.current.insert(net_name);
            }
        }
    }

    /// `BOARD_NETLIST_UPDATER::updateCopperZoneNets`.
    fn update_copper_zone_nets(&mut self, b: &mut Board<'_>, netlist: &Netlist, zone_pads: &BTreeMap<String, Vec<(String, String)>>) {
        let names: BTreeSet<String> = netlist.components.iter().flat_map(|c| c.nets.iter().map(|n| n.net.clone())).collect();
        let vias: Vec<(String, String)> = b.design.routing.as_ref().map(|r| r.vias.iter().map(|v| (v.id.clone(), v.net.clone())).collect()).unwrap_or_default();
        for (id, net) in vias {
            if names.contains(&net) {
                continue;
            }
            match self.old_to_new.get(&net).cloned() {
                Some(to) if !to.is_empty() => {
                    let text = self.tense(format!("Reconnect via from {} to {}.", esc(&unesc(&net)), esc(&unesc(&to))), format!("Reconnected via from {} to {}.", esc(&unesc(&net)), esc(&unesc(&to))));
                    self.say(Severity::Action, text);
                    if !self.dry {
                        if let Some(v) = b.design.routing.as_mut().and_then(|r| r.vias.iter_mut().find(|v| v.id == id)) {
                            v.net = to;
                        }
                    }
                }
                _ => self.warn(format!("Via connected to unknown net ({}).", esc(&unesc(&net)))),
            }
        }
        // "Footprints that contain multiple overlapping pads with the same number are suffixed with "_N" for internal use."
        let in_netlist = |n: &str| names.contains(n) || names.iter().any(|x| n.starts_with(x.as_str()));
        let zones: Vec<eda_model::ir::Zone> = b.design.routing.as_ref().map(|r| r.zones.clone()).unwrap_or_default();
        for z in zones.iter().filter(|z| !z.is_rule_area && z.layer_set().iter().any(|l| l.ends_with(".Cu"))) {
            if in_netlist(&z.net) {
                continue;
            }
            let mut updated = String::new();
            for (r, n) in zone_pads.get(&z.id).map(Vec::as_slice).unwrap_or_default() {
                let now = self.pad_net(b, r, n, true);
                if now != z.net {
                    updated = now;
                    break;
                }
            }
            if updated.is_empty() {
                updated = self.old_to_new.get(&z.net).cloned().unwrap_or_default();
            }
            if !updated.is_empty() {
                let text = match (self.dry, z.name.is_empty()) {
                    (true, false) => format!("Reconnect copper zone '{}' from {} to {}.", z.name, esc(&unesc(&z.net)), esc(&unesc(&updated))),
                    (true, true) => format!("Reconnect copper zone from {} to {}.", esc(&unesc(&z.net)), esc(&unesc(&updated))),
                    (false, false) => format!("Reconnected copper zone '{}' from {} to {}.", esc(&z.name), esc(&unesc(&z.net)), esc(&unesc(&updated))),
                    (false, true) => format!("Reconnected copper zone from {} to {}.", esc(&unesc(&z.net)), esc(&unesc(&updated))),
                };
                self.say(Severity::Action, text);
                if !self.dry {
                    if let Some(zz) = b.design.routing.as_mut().and_then(|r| r.zones.iter_mut().find(|x| x.id == z.id)) {
                        zz.net = updated;
                    }
                }
            } else if !z.name.is_empty() {
                self.warn(format!("Copper zone '{}' has no pads connected.", esc(&z.name)));
            } else {
                let at = z.outline.first().copied().unwrap_or(Point { x: Nm::ZERO, y: Nm::ZERO });
                let layers = z.layer_set().iter().map(|s| s.to_string()).collect::<Vec<_>>();
                let layers = if layers.len() > 1 { format!("{} and others", layers[0]) } else { layers.first().cloned().unwrap_or_default() };
                self.warn(format!(
                    "Copper zone on {} at ({}, {}) has no pads connected to net \"{}\".",
                    esc(&layers),
                    eda_drc::constraints::message_text_from_value(at.x.get() as f64),
                    eda_drc::constraints::message_text_from_value(at.y.get() as f64),
                    z.net
                ));
            }
        }
    }

    /// `BOARD_NETLIST_UPDATER::testConnectivity`: every pin of a component its footprint has no pad for is an error.
    fn test_connectivity(&mut self, b: &Board<'_>, netlist: &Netlist, footprint_map: &BTreeMap<usize, String>) {
        for (ci, c) in netlist.components.iter().enumerate() {
            let Some(reference) = footprint_map.get(&ci) else { continue };
            let numbers: BTreeSet<String> = self.footprint_of(b, reference).map(|f| f.pads.iter().map(|p| p.number.clone()).collect()).unwrap_or_default();
            let fpid = self.fpid_of(b, reference);
            for n in &c.nets {
                if n.pin.is_empty() {
                    self.error(format!("Symbol {} has pins with no number.  These pins can not be matched to pads in {}.", c.reference, esc(&fpid)));
                } else if !numbers.contains(&n.pin) {
                    self.error(format!("{} pad {} not found in {}.", c.reference, esc(&n.pin), esc(&fpid)));
                }
            }
        }
    }
}

#[cfg(test)]
#[path = "netlist_update_tests.rs"]
mod tests;
