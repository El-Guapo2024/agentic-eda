//! A port of KiCad's schematic ERC (Electrical Rules Check): the pin-type
//! conflict matrix, unconnected pins, dangling wires/labels, power-input
//! pins left undriven, library/footprint-library link issues, off-grid
//! endpoints, isolated labels, and duplicate references. Reports as
//! `CheckResult`s under the exact check names KiCad's own
//! `erc_settings.cpp`/`erc_item.cpp` use as JSON `"type"`/settings-key
//! strings (`pin_to_pin`, `pin_not_connected`, `pin_not_driven`,
//! `power_pin_not_driven`, `label_dangling`, `wire_dangling`,
//! `unconnected_wire_endpoint`, `no_connect_connected`,
//! `no_connect_dangling`, `lib_symbol_issues`, `lib_symbol_mismatch`,
//! `footprint_link_issues`, `endpoint_off_grid`, `isolated_pin_label`,
//! `duplicate_reference`), so a failure here reads the same as one from
//! `kicad-cli sch erc --format json`.
//!
//! Severities match KiCad's own shipped defaults (`ERC_SETTINGS::ERC_SETTINGS()`):
//! `pin_not_connected`/`pin_not_driven`/`power_pin_not_driven`/
//! `label_dangling`/`wire_dangling` are Fail (KiCad: Error); `pin_to_pin` is
//! Fail or Warn per the matrix cell; `unconnected_wire_endpoint`/
//! `no_connect_connected`/`no_connect_dangling`/`lib_symbol_issues`/
//! `lib_symbol_mismatch`/`footprint_link_issues`/`endpoint_off_grid`/
//! `isolated_pin_label` are Warn (KiCad: Warning).
//! `duplicate_reference` is scored Fail here even though upstream KiCad's
//! own `RunTests()` never actually invokes the (otherwise fully
//! implemented) annotation-duplicate check in this source snapshot — see
//! this module's own doc on [`check_duplicate_references`].
//!
//! Connectivity ground truth is `ConstraintModel::nets` (the intent's own
//! declaration of which pins share a net) for the pin-electrical checks
//! (a/b/c/g below); the dangling-wire/dangling-label checks (d/e/f) instead
//! reconcile the *drawn* schematic geometry, since those are inherently
//! about what got exported, not what was intended. This is not a
//! byte-for-byte port of KiCad's `CONNECTION_GRAPH` (subgraphs, bus
//! members, hierarchical-sheet propagation) — it is scoped to what a
//! single-sheet schematic like this project's own generator produces, and
//! to what a human-authored single-sheet KiCad file needs.

use std::collections::{BTreeMap, BTreeSet};

use eda_model::ir::{Design, Point};
use eda_model::{CheckResult, CheckStatus, ConstraintModel};

/// KiCad's `ELECTRICAL_PINTYPE`, in its own declared order (this order is
/// the index into the conflict matrix below — do not reorder).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ElectricalPinType {
    Input,
    Output,
    Bidirectional,
    TriState,
    Passive,
    Free,
    Unspecified,
    PowerIn,
    PowerOut,
    OpenCollector,
    OpenEmitter,
    NoConnect,
}

use ElectricalPinType::*;

const ALL_TYPES: [ElectricalPinType; 12] = [Input, Output, Bidirectional, TriState, Passive, Free, Unspecified, PowerIn, PowerOut, OpenCollector, OpenEmitter, NoConnect];

impl ElectricalPinType {
    fn index(self) -> usize {
        self as usize
    }

    /// KiCad's own canonical (untranslated) token, exactly as a
    /// `.kicad_sym` pin's electrical-type field spells it.
    pub fn canonical_name(self) -> &'static str {
        match self {
            Input => "input",
            Output => "output",
            Bidirectional => "bidirectional",
            TriState => "tri_state",
            Passive => "passive",
            Free => "free",
            Unspecified => "unspecified",
            PowerIn => "power_in",
            PowerOut => "power_out",
            OpenCollector => "open_collector",
            OpenEmitter => "open_emitter",
            NoConnect => "no_connect",
        }
    }

    pub fn from_canonical_name(s: &str) -> Option<Self> {
        ALL_TYPES.into_iter().find(|t| t.canonical_name() == s)
    }

    /// The type this project's coarse `PinKind` maps to when a pin has no
    /// resolved library symbol to read a real electrical type from —
    /// mirrors `eda_kicad`'s own writer (`electrical_type` in `lib.rs`)
    /// exactly, so ERC judges precisely what got exported.
    fn from_pin_kind(kind: eda_model::PinKind, name: Option<&str>) -> Self {
        use eda_model::PinKind;
        match kind {
            PinKind::Power if name.unwrap_or("").to_ascii_uppercase().contains("OUT") => PowerOut,
            PinKind::Power | PinKind::Ground => PowerIn,
            PinKind::Signal => Bidirectional,
            PinKind::Passive => Passive,
            PinKind::Nc => NoConnect,
        }
    }
}

const OK: u8 = 0;
const WAR: u8 = 1;
const ERR: u8 = 2;

/// KiCad's default pin-to-pin conflict matrix (`erc_settings.cpp`'s
/// `m_defaultPinMap`), transcribed verbatim: row = first pin's type,
/// column = second pin's type, in [`ALL_TYPES`] order. `OK`/`WAR`/`ERR` = no
/// error / warning / error, exactly as KiCad ships it — this project
/// applies no customization on top.
#[rustfmt::skip]
const MATRIX: [[u8; 12]; 12] = [
    /*         In,  Out, Bid, 3S,  Pas, Free,Uns, PwrI,PwrO,OC,  OE,  NC  */
    /* In  */ [OK,  OK,  OK,  OK,  OK,  OK,  WAR, OK,  OK,  OK,  OK,  ERR],
    /* Out */ [OK,  ERR, OK,  WAR, OK,  OK,  WAR, OK,  ERR, ERR, ERR, ERR],
    /* Bid */ [OK,  OK,  OK,  OK,  OK,  OK,  WAR, OK,  WAR, OK,  WAR, ERR],
    /* 3S  */ [OK,  WAR, OK,  OK,  OK,  OK,  WAR, WAR, ERR, WAR, WAR, ERR],
    /* Pas */ [OK,  OK,  OK,  OK,  OK,  OK,  WAR, OK,  OK,  OK,  OK,  ERR],
    /* Free*/ [OK,  OK,  OK,  OK,  OK,  OK,  OK,  OK,  OK,  OK,  OK,  ERR],
    /* Uns */ [WAR, WAR, WAR, WAR, WAR, OK,  WAR, WAR, WAR, WAR, WAR, ERR],
    /*PwrI */ [OK,  OK,  OK,  WAR, OK,  OK,  WAR, OK,  OK,  OK,  OK,  ERR],
    /*PwrO */ [OK,  ERR, WAR, ERR, OK,  OK,  WAR, OK,  ERR, ERR, ERR, ERR],
    /* OC  */ [OK,  ERR, OK,  WAR, OK,  OK,  WAR, OK,  ERR, OK,  OK,  ERR],
    /* OE  */ [OK,  ERR, WAR, WAR, OK,  OK,  WAR, OK,  ERR, OK,  OK,  ERR],
    /* NC  */ [ERR, ERR, ERR, ERR, ERR, ERR, ERR, ERR, ERR, ERR, ERR, ERR],
];

fn matrix_lookup(a: ElectricalPinType, b: ElectricalPinType) -> u8 {
    MATRIX[a.index()][b.index()]
}

/// A driving pin type for an ordinary net (KiCad's `DrivingPinTypes`).
fn drives_ordinary_pin(t: ElectricalPinType) -> bool {
    matches!(t, Output | PowerOut | Passive | TriState | Bidirectional)
}
/// A driving pin type for a power net (KiCad's `DrivingPowerPinTypes`):
/// only a power-output pin (a regulator's VOUT, or a `PWR_FLAG`-style
/// symbol's one pin) satisfies it — an ordinary output/passive pin does
/// not, even though it would for a non-power net.
fn drives_power_pin(t: ElectricalPinType) -> bool {
    t == PowerOut
}
/// A pin type that needs a driver somewhere on its net (KiCad's
/// `DrivenPinTypes`).
fn needs_a_driver(t: ElectricalPinType) -> bool {
    matches!(t, Input | PowerIn)
}

/// One resolved pin, everything the checks below need about it.
struct ResolvedPin {
    part_ref: String,
    number: String,
    etype: ElectricalPinType,
    at: Point,
}

impl ResolvedPin {
    fn pin_ref(&self) -> String {
        format!("{}.{}", self.part_ref, self.number)
    }
}

/// One net member for the pin-electrical checks below: a real part's pin,
/// or a placed [`eda_model::ir::PowerSymbol`]'s own pin. In KiCad's own
/// model a power symbol's pin is not a special case — it is exactly one
/// more `SCH_PIN` sharing the net, most commonly a `power:PWR_FLAG`'s
/// single `power_out` pin (the textbook net driver a human, or this
/// project's own generator, places to satisfy `power_pin_not_driven`) or a
/// `power:GND`/`power:<RAIL>`'s `power_in` pin (a sink, coincident with
/// and electrically identical to the real pin it was placed at). Folding
/// these into the same list the real pins use means a `PWR_FLAG` a
/// generator inserted is actually seen as a driver by the checks below,
/// instead of that net still being reported as undriven.
enum Member<'a> {
    Pin(&'a ResolvedPin),
    Power { id: &'a str, etype: ElectricalPinType },
}

impl Member<'_> {
    fn etype(&self) -> ElectricalPinType {
        match self {
            Member::Pin(p) => p.etype,
            Member::Power { etype, .. } => *etype,
        }
    }
    fn label(&self) -> String {
        match self {
            Member::Pin(p) => p.pin_ref(),
            Member::Power { id, .. } => (*id).to_string(),
        }
    }
}

/// A power symbol's own electrical type: its resolved library symbol's
/// (project-declared, or [`eda_model::symbol::builtin`] fallback — the
/// same two-step resolution [`resolve_pins`] uses for real parts) first
/// pin, since every power symbol this project places or reads has exactly
/// one. Defaults to `power_in` (the more conservative reading: a sink,
/// not a driver) on the otherwise-impossible case of a power symbol whose
/// `lib_id` resolves to nothing at all.
fn power_symbol_etype(ps: &eda_model::ir::PowerSymbol, model: &ConstraintModel) -> ElectricalPinType {
    model.symbol_of(&ps.lib_id).and_then(|s| s.pins.first().and_then(|p| ElectricalPinType::from_canonical_name(&p.electrical_type))).unwrap_or(PowerIn)
}

/// Every net member — real pins plus power-symbol pins — for one net,
/// unified into one list so every check below sees a `PWR_FLAG`/`GND`
/// symbol exactly like an ordinary pin, per [`Member`]'s doc.
fn net_members<'a>(net: &eda_model::Net, sch: &'a eda_model::ir::SchematicSection, model: &ConstraintModel, pins_by_ref: &BTreeMap<String, &'a ResolvedPin>) -> Vec<Member<'a>> {
    let mut members: Vec<Member<'a>> = net.pins.iter().filter_map(|p| pins_by_ref.get(p).copied()).map(Member::Pin).collect();
    for ps in &sch.power_symbols {
        if ps.net == net.name {
            members.push(Member::Power { id: ps.id.as_str(), etype: power_symbol_etype(ps, model) });
        }
    }
    members
}

/// Resolve every real part's every pin: its electrical type (real library
/// symbol pin, matched by number, when one resolved; else the same
/// `PinKind` fallback the exporter itself uses) and its absolute position
/// (the same stub tip the exporter draws the pin's own connection point
/// at) — the two facts every check below needs, computed once up front so
/// no check has to re-derive symbol geometry on its own.
fn resolve_pins(design: &Design, model: &ConstraintModel) -> Vec<ResolvedPin> {
    let Some(sch) = &design.schematic else { return Vec::new() };
    let mut out = Vec::new();
    for sym in &sch.symbols {
        let Some(part) = model.part(&sym.id) else { continue };
        let resolved_sym = model.real_symbol_of(&sym.lib_id, part);
        let (width, height) = eda_engine::geometry::node_size(part, resolved_sym.as_ref());
        let (ports, pin_port) = eda_engine::geometry::build_ports(part, width, height, resolved_sym.as_ref());
        let node = eda_layout::Node { id: 0, width, height, ports };
        let angle_deg = sym.rot as f64 / 1000.0;
        for (pin_idx, pin) in part.pins.iter().enumerate() {
            // An `nc`-kind pin is authoritative and wins over whatever a
            // resolved real library says that pin number's ordinary
            // electrical type is: a generic connector's real pins are all
            // "passive" (KiCad's own library has no way to say "pin 4 is
            // unused on this particular board"), but the writer still
            // draws a `no_connect` flag there -- and nowhere else --
            // because `pin.kind` says so (see `derive_schematic`'s
            // no-connect-flag loop and `nc_pin_local_points`), not because
            // of the resolved symbol's type. ERC must judge the same pin
            // the same way, or a real-library-resolved `nc` pin (any
            // connector with an intentionally unused pin, e.g.
            // `examples/nc_pins.yaml`'s J1) fails `pin_not_connected`
            // despite being exactly as intentionally unconnected as one on
            // a synthesized box.
            let etype = if pin.kind == eda_model::PinKind::Nc {
                NoConnect
            } else {
                resolved_sym
                    .as_ref()
                    .and_then(|s| s.pin_by_number(&pin.number))
                    .and_then(|p| ElectricalPinType::from_canonical_name(&p.electrical_type))
                    .unwrap_or_else(|| ElectricalPinType::from_pin_kind(pin.kind, pin.name.as_deref()))
            };
            let at = native_imported_pin_at(sch, sym, pin, resolved_sym.as_ref(), angle_deg).unwrap_or_else(|| match pin_port[pin_idx] {
                // `derive_schematic` never rotates/mirrors a symbol (every
                // `SymbolInstance` it emits has `rot: 0, mirrored: false`,
                // baking any future rotation into the *drawn* geometry
                // instead — see `eda_kicad::baked_local`), and neither does
                // `eda_gates::SymGeo` today; this engine-box fallback
                // inherits that same scope rather than being the first
                // thing in the workspace to handle a rotated *generated*
                // symbol (an imported one is handled above instead, by
                // `native_imported_pin_at`).
                Some(port_idx) => {
                    let tip = node.stub_tip(eda_layout::Point { x: sym.at.x, y: sym.at.y }, port_idx);
                    Point { x: tip.x, y: tip.y }
                }
                // An `nc`-kind pin has no `Port` (see
                // `geometry::nc_pin_local_points`); its position for ERC
                // purposes is wherever `no_connects` says it is — found by
                // pin ref below rather than recomputed here, since only
                // the generator (not this resolver) knows the NC layout
                // convention. `Point` default (0,0) is a harmless
                // placeholder: an NC pin is exempt from every position-
                // sensitive check below (see `check_unconnected_pins`).
                None => sch.no_connects.iter().find(|nc| nc.pin == format!("{}.{}", sym.id, pin.number)).map(|nc| nc.at).unwrap_or(Point { x: 0, y: 0 }),
            });
            out.push(ResolvedPin { part_ref: sym.id.clone(), number: pin.number.clone(), etype, at });
        }
    }
    out
}

/// The absolute point of `pin` when `sch` was read from a real `.kicad_sch`
/// (`SchematicSection::imported_from_kicad` — see its own doc for why this
/// is a separate convention from the engine's box/port system just below,
/// not an alternative implementation of the same thing): `sym.at` is the
/// file's own native symbol origin, so the real per-pin library coordinate
/// transforms off of it exactly the way `sch_import::reconcile`'s own
/// `pin_world` was built in the first place — reusing
/// `sch_import::transform_local_point` directly rather than re-deriving the
/// same math means a real file's recorded rotation/mirror is honored here
/// too, which the engine-box path below has never needed to handle.
///
/// Deliberately *not* skipped for an `nc`-kind pin here, unlike the
/// engine-box fallback: that fallback's own `no_connects` lookup only works
/// for a schematic `derive_schematic` built (its generator populates
/// `NoConnect::pin` itself when it places the flag), but
/// `sch_import::reconcile` *never* backfills `NoConnect::pin` for an
/// imported file — a point an explicit no-connect flag sits on is
/// deliberately excluded from `pins_of_point` there (so it can never
/// contribute a pin to a net), which as a side effect means the field
/// whoever reads `nc.pin` back stays empty forever, not just until some
/// later pass fills it in. Resolving an imported `nc` pin's point the same
/// direct way an ordinary one is here instead -- the real resolved
/// symbol's own per-pin coordinate always has one, whether or not that
/// pin's *library* electrical type happens to be `no_connect` -- is what
/// lets [`check_no_connects`] actually find the real pin KiCad drew a
/// no-connect flag onto, instead of landing on this function's `(0, 0)`
/// placeholder and reporting every single imported no-connect flag as
/// dangling regardless of the file.
///
/// `None` for a generated (non-imported) schematic, or the rare imported
/// pin number a resolved real symbol doesn't actually have — both fall
/// back to the pre-existing engine-box computation.
fn native_imported_pin_at(sch: &eda_model::ir::SchematicSection, sym: &eda_model::ir::SymbolInstance, pin: &eda_model::Pin, resolved_sym: Option<&eda_model::symbol::LibSymbol>, angle_deg: f64) -> Option<Point> {
    if !sch.imported_from_kicad {
        return None;
    }
    let real_pin = resolved_sym?.pin_by_number(&pin.number)?;
    let world = crate::sch_import::transform_local_point(real_pin.at, angle_deg, sym.mirrored, sym.mirror_y);
    Some(Point { x: sym.at.x + crate::import::mm_to_um(world.x), y: sym.at.y + crate::import::mm_to_um(world.y) })
}

/// Runs every ERC check this module implements — the pin-electrical checks
/// below, *and* [`crate::erc_style::check_style`]'s readability/style
/// checks (grid, orthogonality, label placement, sheet density, ...),
/// ported in from the former `eda-gates::check_schematic` so this one
/// function is the whole engine a schematic is judged by, the same way
/// KiCad's own ERC runs its non-electrical checks (similar labels,
/// off-grid pins) alongside the electrical ones rather than as a separate
/// tool. Returns one `CheckResult` per finding, plus a `Pass` for any
/// check that found nothing.
pub fn check_erc(design: &Design, model: &ConstraintModel) -> Vec<CheckResult> {
    let Some(sch) = &design.schematic else {
        let mut out = vec![CheckResult::fail("lib_symbol_issues", "design", "design has no schematic section")];
        out.extend(crate::erc_style::check_style(design, model));
        return out;
    };
    let pins = resolve_pins(design, model);
    let pins_by_ref: BTreeMap<String, &ResolvedPin> = pins.iter().map(|p| (p.pin_ref(), p)).collect();

    let mut out = Vec::new();
    check_pin_to_pin(model, sch, &pins_by_ref, &mut out);
    check_unconnected_pins(model, &pins, &mut out);
    check_driven_pins(model, sch, &pins_by_ref, &mut out);
    check_no_connects(sch, &pins, &mut out);
    check_dangling_wires_and_labels(model, sch, &pins, &mut out);
    check_lib_symbol_issues(sch, model, &mut out);
    check_off_grid_endpoints(sch, &pins, &mut out);
    check_isolated_pin_label(sch, model, &mut out);
    check_footprint_link_issues(sch, &mut out);
    check_duplicate_references(sch, &mut out);
    out.extend(crate::erc_style::check_style(design, model));
    out
}

/// One waived finding: a specific check at a specific location, reviewed
/// and accepted the way KiCad's own ERC/DRC lets a human "Exclude" a
/// violation without deleting it from the report. Matched on the exact
/// `(check, location)` pair a [`CheckResult`] itself carries.
#[derive(Debug, Clone, Default)]
pub struct Exclusions(std::collections::BTreeSet<(String, String)>);

impl Exclusions {
    pub fn new() -> Self {
        Self::default()
    }
    /// Mark every future finding at this exact `(check, location)` as
    /// excluded rather than failing or warning.
    pub fn exclude(&mut self, check: impl Into<String>, location: impl Into<String>) -> &mut Self {
        self.0.insert((check.into(), location.into()));
        self
    }
    fn covers(&self, check: &str, location: Option<&str>) -> bool {
        location.is_some_and(|loc| self.0.contains(&(check.to_string(), loc.to_string())))
    }
}

/// [`check_erc`], with `exclusions` applied afterward: any non-`Pass`
/// result whose `(check, location)` is in `exclusions` is downgraded to
/// [`CheckStatus::Excluded`] in place, exactly KiCad's own "Exclude"
/// action on an ERC/DRC violation -- still visible in the report (hint and
/// location untouched), but no longer a failure or a warning. Applying
/// this as one pass over the finished report, rather than threading
/// `exclusions` through all thirty-odd individual checks, is exactly
/// equivalent: an exclusion only ever suppresses a finding a check already
/// made, it can never change what a check finds.
pub fn check_erc_excluding(design: &Design, model: &ConstraintModel, exclusions: &Exclusions) -> Vec<CheckResult> {
    let mut out = check_erc(design, model);
    for r in &mut out {
        if r.status != CheckStatus::Pass && exclusions.covers(&r.check, r.location.as_deref()) {
            r.status = CheckStatus::Excluded;
        }
    }
    out
}

/// (a) Pin-to-pin electrical conflict, per net: every unordered pair of
/// distinct pins on the net is looked up in [`MATRIX`]; a non-OK cell
/// reports once per pair (KiCad instead reduces every mismatch on a net
/// down to one marker on its "worst" pin — reporting every offending pair
/// is the simplification this port makes, in exchange for never hiding a
/// real conflict behind one that sorted higher).
fn check_pin_to_pin(model: &ConstraintModel, sch: &eda_model::ir::SchematicSection, pins_by_ref: &BTreeMap<String, &ResolvedPin>, out: &mut Vec<CheckResult>) {
    let mut ok = true;
    for net in &model.nets {
        let members = net_members(net, sch, model, pins_by_ref);
        for i in 0..members.len() {
            for j in (i + 1)..members.len() {
                let (a, b) = (&members[i], &members[j]);
                let severity = matrix_lookup(a.etype(), b.etype());
                if severity == OK {
                    continue;
                }
                out.push(CheckResult {
                    check: "pin_to_pin".into(),
                    status: if severity == ERR { CheckStatus::Fail } else { CheckStatus::Warn },
                    location: Some(format!("{}:{}", net.name, a.label())),
                    hint: Some(format!(
                        "net '{}': pins of type {} ({}) and {} ({}) are connected",
                        net.name,
                        a.etype().canonical_name(),
                        a.label(),
                        b.etype().canonical_name(),
                        b.label()
                    )),
                    detail: None,
                });
                ok = false;
            }
        }
    }
    if ok {
        out.push(CheckResult::pass("pin_to_pin"));
    }
}

/// (b) A pin left completely unconnected: not on any net with another
/// member, not flagged no-connect, and not itself an `nc`/`free`-typed
/// pin (KiCad's own exemption: `pin->GetType() != PT_NC && != PT_NIC`).
fn check_unconnected_pins(model: &ConstraintModel, pins: &[ResolvedPin], out: &mut Vec<CheckResult>) {
    let mut net_size: BTreeMap<String, usize> = BTreeMap::new();
    for net in &model.nets {
        for p in &net.pins {
            *net_size.entry(p.clone()).or_default() += net.pins.len();
        }
    }
    let mut ok = true;
    for pin in pins {
        if matches!(pin.etype, NoConnect | Free) {
            continue;
        }
        let on_a_real_net = net_size.get(&pin.pin_ref()).is_some_and(|&n| n >= 2);
        if !on_a_real_net {
            out.push(CheckResult::fail("pin_not_connected", pin.pin_ref(), format!("pin {} ({}) is not connected to anything", pin.pin_ref(), pin.etype.canonical_name())));
            ok = false;
        }
    }
    if ok {
        out.push(CheckResult::pass("pin_not_connected"));
    }
}

/// (c)/(g) A pin that needs a driver (an ordinary Input, or a power-net's
/// Power-Input pins) with none on its net — KiCad's `TestPinToPin`
/// algorithm, section (g): a *power* net (any pin on it is `power_in`)
/// needs a `power_out` pin specifically (an ordinary Output/Passive/
/// Bidirectional/TriState pin does **not** count, even though it would
/// for a non-power net) — this is exactly the mechanism a `PWR_FLAG`
/// symbol works through: its one pin is `power_out`, nothing more special
/// than that. Suppressed net-wide when any pin on the net has an explicit
/// no-connect flag (KiCad: `has_noconnect`).
fn check_driven_pins(model: &ConstraintModel, sch: &eda_model::ir::SchematicSection, pins_by_ref: &BTreeMap<String, &ResolvedPin>, out: &mut Vec<CheckResult>) {
    let mut ok_driven = true;
    let mut ok_power = true;
    for net in &model.nets {
        let members = net_members(net, sch, model, pins_by_ref);
        // No `members.len() < 2` early-out here: KiCad's own `TestPinToPin`
        // (`erc.cpp`) runs this same `needsDriver`/`hasDriver` test over
        // every net regardless of size, so a single `Input`/`power_in` pin
        // that is the *only* thing on its net (never wired to anything --
        // `check_unconnected_pins` already reports that independently, the
        // same way real KiCad's own separate `pin_not_connected` test
        // does) must still fail `pin_not_driven`/`power_pin_not_driven`
        // here: nothing drives it either, and the two checks are not
        // mutually exclusive in real KiCad.
        let is_power_net = members.iter().any(|p| p.etype() == PowerIn);
        let has_driver = if is_power_net { members.iter().any(|p| drives_power_pin(p.etype())) } else { members.iter().any(|p| drives_ordinary_pin(p.etype())) };
        if has_driver {
            continue;
        }
        let Some(needy) = members.iter().find(|p| needs_a_driver(p.etype())) else { continue };
        let check = if is_power_net { "power_pin_not_driven" } else { "pin_not_driven" };
        let hint = if is_power_net {
            format!("net '{}' has a power-input pin ({}) but no power-output pin (a regulator's VOUT, or a PWR_FLAG) anywhere on it", net.name, needy.label())
        } else {
            format!("net '{}' has an input pin ({}) but no output/passive/bidirectional/tri-state pin driving it", net.name, needy.label())
        };
        out.push(CheckResult::fail(check, format!("{}:{}", net.name, needy.label()), hint));
        if is_power_net {
            ok_power = false;
        } else {
            ok_driven = false;
        }
    }
    if ok_driven {
        out.push(CheckResult::pass("pin_not_driven"));
    }
    if ok_power {
        out.push(CheckResult::pass("power_pin_not_driven"));
    }
}

/// (d) A no-connect flag with more than one distinct pin sitting on it
/// (`no_connect_connected`), or with none at all (`no_connect_dangling`).
fn check_no_connects(sch: &eda_model::ir::SchematicSection, pins: &[ResolvedPin], out: &mut Vec<CheckResult>) {
    let mut ok = true;
    for nc in &sch.no_connects {
        let here: BTreeSet<String> = pins.iter().filter(|p| p.at == nc.at).map(|p| p.pin_ref()).collect();
        if here.len() > 1 {
            out.push(CheckResult::fail("no_connect_connected", format!("{},{}", nc.at.x, nc.at.y), format!("a pin with a 'no connection' flag is connected: {}", here.iter().cloned().collect::<Vec<_>>().join(", "))));
            ok = false;
        } else if here.is_empty() {
            out.push(CheckResult {
                check: "no_connect_dangling".into(),
                status: CheckStatus::Warn,
                location: Some(format!("{},{}", nc.at.x, nc.at.y)),
                hint: Some("unconnected 'no connection' flag: no pin sits where it was placed".into()),
                detail: None,
            });
            ok = false;
        }
    }
    if ok {
        out.push(CheckResult::pass("no_connect_connected"));
        out.push(CheckResult::pass("no_connect_dangling"));
    }
}

/// (e)/(f) Dangling wire endpoints and dangling labels: reconciles the
/// *drawn* geometry (not the intent) — a wire endpoint, or a label, is
/// "connected" when it coincides exactly with another point that carries
/// the same net: a real pin declared on that net (`ConstraintModel::nets`
/// is the ground truth here, not which pins a `Wire` happens to list — a
/// net's only member on one side can be a bare label with no wire at all,
/// exactly what `derive_schematic` itself draws for an isolated flag), a
/// point on another same-net wire (an endpoint, or a T-junction landing on
/// a segment's interior), a power symbol, or a label. Scoped to a single
/// sheet, as this project's own schematics are.
fn check_dangling_wires_and_labels(model: &ConstraintModel, sch: &eda_model::ir::SchematicSection, pins: &[ResolvedPin], out: &mut Vec<CheckResult>) {
    let pins_by_ref: BTreeMap<String, Point> = pins.iter().map(|p| (p.pin_ref(), p.at)).collect();
    // Every point, per net, that legitimately carries that net: every pin
    // `ConstraintModel::nets` declares on it (however it ended up drawn —
    // wired, labeled, or via a power symbol), plus every label and power
    // symbol anchor. Wire points are deliberately *not* folded in here:
    // whether a wire's own endpoint counts as corroboration for another
    // wire's endpoint on the same net is exactly the question these checks
    // are asking, so wires are walked separately below instead of being
    // treated as ambient truth.
    let mut net_anchor_points: BTreeMap<&str, BTreeSet<Point>> = BTreeMap::new();
    for net in &model.nets {
        let set = net_anchor_points.entry(net.name.as_str()).or_default();
        for pin_ref in &net.pins {
            if let Some(&at) = pins_by_ref.get(pin_ref) {
                set.insert(at);
            }
        }
    }
    for l in &sch.labels {
        net_anchor_points.entry(l.net.as_str()).or_default().insert(l.at);
    }
    for ps in &sch.power_symbols {
        net_anchor_points.entry(ps.net.as_str()).or_default().insert(ps.at);
    }

    let touches_anchor_or_other_wire = |w: &eda_model::ir::Wire, end: Point| -> bool {
        net_anchor_points.get(w.net.as_str()).is_some_and(|pts| pts.contains(&end))
            || sch.wires.iter().any(|other| !std::ptr::eq(other, w) && other.net == w.net && (other.pts.contains(&end) || point_on_segment_interior_any(other, end)))
    };

    let mut ok_wire = true;
    let mut ok_endpoint = true;
    for w in &sch.wires {
        let ends: Vec<Point> = [w.pts.first(), w.pts.last()].into_iter().flatten().copied().collect();
        let mut any_touch = false;
        for &end in &ends {
            let touches = touches_anchor_or_other_wire(w, end);
            any_touch |= touches;
            if !touches {
                out.push(CheckResult {
                    check: "unconnected_wire_endpoint".into(),
                    status: CheckStatus::Warn,
                    location: Some(format!("{}:{},{}", w.net, end.x, end.y)),
                    hint: Some(format!("unconnected wire endpoint on net '{}' at ({},{})", w.net, end.x, end.y)),
                    detail: None,
                });
                ok_endpoint = false;
            }
        }
        // A wire with *no* end touching anything else is a whole floating
        // island — KiCad's coarser, net-level `wire_dangling`, reported
        // once per wire rather than once per bad endpoint.
        if !any_touch && !ends.is_empty() {
            out.push(CheckResult {
                check: "wire_dangling".into(),
                status: CheckStatus::Fail,
                location: Some(w.net.clone()),
                hint: Some(format!("wire on net '{}' is not connected to anything", w.net)),
                detail: None,
            });
            ok_wire = false;
        }
    }
    if ok_endpoint {
        out.push(CheckResult::pass("unconnected_wire_endpoint"));
    }
    if ok_wire {
        out.push(CheckResult::pass("wire_dangling"));
    }

    // A label is connected when it sits exactly on a real pin the net
    // declares, or touches a wire of the same net (at an endpoint, or a
    // T-junction on a segment's interior) — covers both this project's own
    // two label shapes: an isolated pin's flag (sits exactly on that pin's
    // stub tip) and a merged run's flag (sits on the bus-stub wire joining
    // the run's pins).
    let mut ok_label = true;
    for l in &sch.labels {
        let on_a_declared_pin = model.nets.iter().find(|n| n.name == l.net).is_some_and(|n| n.pins.iter().any(|p| pins_by_ref.get(p) == Some(&l.at)));
        let on_a_wire = sch.wires.iter().any(|w| w.net == l.net && (w.pts.contains(&l.at) || point_on_segment_interior_any(w, l.at)));
        if !on_a_declared_pin && !on_a_wire {
            out.push(CheckResult::fail("label_dangling", format!("{}:{},{}", l.net, l.at.x, l.at.y), format!("label '{}' is not connected to anything", l.net)));
            ok_label = false;
        }
    }
    if ok_label {
        out.push(CheckResult::pass("label_dangling"));
    }
}

fn point_on_segment_interior_any(w: &eda_model::ir::Wire, p: Point) -> bool {
    w.pts.windows(2).any(|seg| point_on_segment_interior(p, seg[0], seg[1]))
}

fn point_on_segment_interior(p: Point, a: Point, b: Point) -> bool {
    if a.x == b.x {
        p.x == a.x && p.y > a.y.min(b.y) && p.y < a.y.max(b.y)
    } else if a.y == b.y {
        p.y == a.y && p.x > a.x.min(b.x) && p.x < a.x.max(b.x)
    } else {
        false
    }
}

/// (i) `lib_symbol_issues`/`lib_symbol_mismatch`, ported from KiCad's own
/// `ERC_TESTER::TestLibSymbolIssues` (`erc.cpp`): for every placed
/// instance's embedded `lib_symbols` copy (a real part's or a power
/// symbol's -- KiCad's own version walks every `SCH_SYMBOL_T` alike, a
/// power symbol is not a distinct item type there), the library nickname
/// must resolve under the real, currently-installed KiCad symbol libraries
/// ([`crate::symbol_lib::default_symbol_library_root`]) and must actually
/// contain that symbol name (`lib_symbol_issues` otherwise -- KiCad's own
/// "configuration does not include the symbol library"/"symbol ... not
/// found in symbol library" messages); once resolved, the embedded copy
/// must still structurally match what that library currently contains
/// (`lib_symbol_mismatch` otherwise -- "doesn't match copy in library").
///
/// Every instance this project ever places carries an embedded cached copy
/// by construction (the exporter's own `lib_symbols` block, or --
/// equivalently, for an imported file -- `sch_import`'s own `lib_table`,
/// the only way a `SymbolInstance`/`PowerSymbol` is created in the first
/// place), so this port skips the "does the schematic even embed a copy of
/// this symbol" precondition KiCad's own version checks first: it always
/// holds here.
///
/// Scoped to the one real library install this workspace ships against,
/// not a real project/global `sym-lib-table`: every schematic this task
/// measures against either carries no project-local library table at all
/// (the KiCad QA corpus this port's numbers are measured against) or only
/// ever names that same real install's own library nicknames, so "is the
/// library nickname one of that install's own `.kicad_sym` files" already
/// agrees with KiCad's own richer "is it in the current configuration"
/// check for every case this port is scored against -- a real project with
/// its own extra project-local `sym-lib-table` entries is the one case
/// this scope gets wrong (it would report `lib_symbol_issues` where a real
/// `sym-lib-table` entry would have resolved).
fn check_lib_symbol_issues(sch: &eda_model::ir::SchematicSection, model: &ConstraintModel, out: &mut Vec<CheckResult>) {
    // KiCad's own shipped default for both (`erc_settings.cpp`'s
    // `ERC_SETTINGS::ERC_SETTINGS()`) is Warning, not Error -- unlike most
    // of this file's other checks (`pin_not_connected`, `wire_dangling`,
    // ...), which really are Fail-severity by KiCad's own defaults. A
    // synthesized generic box (this project's fallback for any part with
    // no real library match) necessarily fails `lib_symbol_issues` the
    // moment this check runs at all -- real KiCad would say exactly the
    // same thing about such a symbol -- so Warn (not Fail) is also what
    // keeps that expected, benign finding from reading as a build-breaking
    // error the way it would if this used `CheckResult::fail` like its
    // neighbors.
    let warn = |check: &str, location: String, hint: String| CheckResult { check: check.into(), status: CheckStatus::Warn, location: Some(location), hint: Some(hint), detail: None };

    let root = crate::symbol_lib::default_symbol_library_root();
    let mut ok = true;

    let mut instances: Vec<(&str, String)> = sch.symbols.iter().map(|s| (s.lib_id.as_str(), s.id.clone())).collect();
    instances.extend(sch.power_symbols.iter().map(|p| (p.lib_id.as_str(), p.id.clone())));

    for (lib_id, location) in instances {
        let Some((lib_name, symbol_name)) = lib_id.split_once(':') else { continue };
        if lib_name.is_empty() || symbol_name.is_empty() {
            continue;
        }
        if crate::symbol_lib::find_symbol_library_file(&root, lib_name).is_none() {
            out.push(warn("lib_symbol_issues", location, format!("The current configuration does not include the symbol library '{lib_name}'")));
            ok = false;
            continue;
        }
        let Some(real_sym) = crate::symbol_lib::resolve_symbol(&root, lib_id) else {
            out.push(warn("lib_symbol_issues", location, format!("Symbol '{symbol_name}' not found in symbol library '{lib_name}'")));
            ok = false;
            continue;
        };
        // Only meaningful on a schematic read back from a real file: there,
        // `model.symbols` holds the file's *own* embedded cache (built by
        // `sch_import` from its `lib_symbols` block), genuinely independent
        // of whatever `real_sym` just loaded fresh from disk, so a real
        // divergence between the two is detectable. For a schematic this
        // project just derived (not yet exported/re-imported), the
        // "cached" entry in `model.symbols` is either absent or -- when a
        // part resolved against a real library -- the very same lookup
        // `real_sym` just repeated, so the two can never structurally
        // differ here even though the file this project's own exporter
        // will eventually write re-bakes that symbol's coordinates into
        // this project's own box-corner frame and so *would* trip
        // `kicad-cli`'s real `Compare()`. Catching that would mean
        // predicting the exporter's own output from inside ERC, which is
        // its own, separate, sizeable task (and arguably an exporter
        // fidelity bug, not an ERC gap) -- left as a known, documented
        // scope limit rather than guessed at here.
        if sch.imported_from_kicad {
            if let Some(cached) = model.symbols.iter().find(|s| s.lib_id == lib_id) {
                if lib_symbols_differ(cached, &real_sym) {
                    out.push(warn("lib_symbol_mismatch", location, format!("Symbol '{symbol_name}' doesn't match copy in library '{lib_name}'")));
                    ok = false;
                }
            }
        }
    }
    if ok {
        out.push(CheckResult::pass("lib_symbol_issues"));
        out.push(CheckResult::pass("lib_symbol_mismatch"));
    }
}

/// Structural equality for [`check_lib_symbol_issues`]'s mismatch test --
/// an approximation of KiCad's own `LIB_SYMBOL::Compare` (which also
/// inspects internal unit/alternate-body-style bookkeeping this project's
/// `LibSymbol` never captured to begin with) over every field this port's
/// readers actually populate: pins (sorted by number first, so a library
/// that merely reordered its own pin list is not a false mismatch),
/// graphics, the power/in_bom/on_board flags, and the datasheet/
/// description/reference-prefix/unit-count metadata.
fn lib_symbols_differ(cached: &eda_model::symbol::LibSymbol, real: &eda_model::symbol::LibSymbol) -> bool {
    let mut cached_pins = cached.pins.clone();
    let mut real_pins = real.pins.clone();
    cached_pins.sort_by(|a, b| a.number.cmp(&b.number));
    real_pins.sort_by(|a, b| a.number.cmp(&b.number));
    cached_pins != real_pins
        || cached.graphics != real.graphics
        || cached.power != real.power
        || cached.in_bom != real.in_bom
        || cached.on_board != real.on_board
        || cached.datasheet != real.datasheet
        || cached.description != real.description
        || cached.reference_prefix != real.reference_prefix
        || cached.unit_count != real.unit_count
}

/// KiCad's own default connection grid (`DEFAULT_CONNECTION_GRID_MILS` =
/// 50 mil, `schematic_settings.cpp`) in this project's integer-micrometer
/// frame: `50 * 25.4 = 1270`. A real project can change
/// `m_ConnectionGridSize` in its own settings; this port has no project
/// settings file to read a non-default value from, so it always checks
/// against the value every schematic ships with until a human changes it.
const CONNECTION_GRID_UM: i64 = 1270;

/// (i2) `endpoint_off_grid`, ported from KiCad's own `ERC_TESTER::
/// TestOffGridEndpoints` (`erc.cpp`): a wire point, or a symbol's pin, that
/// does not land on the connection grid above. One marker per symbol (its
/// first off-grid pin, an `nc`-kind one never counting -- matching KiCad's
/// own `continue`/`break` pair exactly), one marker per *wire point* (every
/// vertex of a drawn polyline, not just its two overall ends: once a
/// multi-bend wire is exported the way this project's own writer always
/// does -- one straight `SCH_LINE` per segment -- every interior vertex is
/// just as much a real `SCH_LINE` endpoint as the two overall ones are).
/// KiCad's third source for this check, a bus-wire-entry's own two
/// endpoints, is skipped: this project's IR has no bus concept to draw one
/// from at all (see `docs/parity/GAPS.md` #20).
fn check_off_grid_endpoints(sch: &eda_model::ir::SchematicSection, pins: &[ResolvedPin], out: &mut Vec<CheckResult>) {
    let off_grid = |p: Point| p.x % CONNECTION_GRID_UM != 0 || p.y % CONNECTION_GRID_UM != 0;
    let mut ok = true;

    let mut pins_by_part: BTreeMap<&str, Vec<&ResolvedPin>> = BTreeMap::new();
    for p in pins {
        pins_by_part.entry(p.part_ref.as_str()).or_default().push(p);
    }
    for (part_ref, part_pins) in pins_by_part {
        if let Some(bad) = part_pins.iter().find(|p| p.etype != NoConnect && off_grid(p.at)) {
            out.push(CheckResult {
                check: "endpoint_off_grid".into(),
                status: CheckStatus::Warn,
                location: Some(part_ref.to_string()),
                hint: Some(format!("pin {} is not on the {CONNECTION_GRID_UM} um connection grid", bad.pin_ref())),
                detail: None,
            });
            ok = false;
        }
    }
    for w in &sch.wires {
        for &p in &w.pts {
            if off_grid(p) {
                out.push(CheckResult {
                    check: "endpoint_off_grid".into(),
                    status: CheckStatus::Warn,
                    location: Some(format!("{}:{},{}", w.net, p.x, p.y)),
                    hint: Some(format!("wire point ({},{}) on net '{}' is not on the {CONNECTION_GRID_UM} um connection grid", p.x, p.y, w.net)),
                    detail: None,
                });
                ok = false;
            }
        }
    }
    if ok {
        out.push(CheckResult::pass("endpoint_off_grid"));
    }
}

/// (i3) `isolated_pin_label`, ported from KiCad's own `CONNECTION_GRAPH::
/// ercCheckLabels` (`connection_graph.cpp`): a label whose net has exactly
/// one real pin anywhere. Scoped to a single sheet the same way this
/// port's other label checks already are, so "anywhere" here means
/// "anywhere in `ConstraintModel::nets`" rather than KiCad's own
/// whole-hierarchy `allPins` tally. KiCad additionally exempts a net
/// carrying an explicit no-connect flag (`has_nc`); this project's own net
/// model never puts a no-connect-flagged pin on a net at all (`reconcile`
/// drops it before a net is ever built), so that exemption can never
/// actually apply here and is omitted rather than kept as dead code.
fn check_isolated_pin_label(sch: &eda_model::ir::SchematicSection, model: &ConstraintModel, out: &mut Vec<CheckResult>) {
    let mut ok = true;
    for l in &sch.labels {
        // A global label is deliberately exempt: its entire purpose is to
        // be resolved by *name* anywhere in the project, not by local pin
        // adjacency, and KiCad polices "this name is suspiciously alone"
        // for one separately (`ERCE_SINGLE_GLOBAL_LABEL`, not ported here
        // -- not yet observed in this port's measured corpus) rather than
        // through this check. A real single-sheet schematic that fans a
        // handful of signals out to global labels for documentation/test-
        // point purposes, with nothing else on the same sheet, is common
        // and *not* an ERC finding in real KiCad -- confirmed directly
        // against a real QA fixture this port's own harness measured
        // (`issue23851.kicad_sch`: 21 once-only global labels, 0 of them
        // flagged by real `kicad-cli sch erc`). A local label is sheet-
        // scoped by definition, so this project's own single-sheet net
        // model judges it exactly the way KiCad would. A hierarchical
        // label has no parent sheet in this project's model at all (no
        // hierarchy support -- see `docs/parity/GAPS.md` #6), which is the
        // same "can never connect to anything else" situation a real
        // top-of-hierarchy sheet's own hierarchical label is in -- also
        // confirmed against this port's one real measured instance
        // (`i2c_thingy.kicad_sch`'s hierarchical `AD0`).
        if matches!(l.kind, eda_model::ir::LabelKind::Global { .. }) {
            continue;
        }
        let pin_count = model.nets.iter().find(|n| n.name == l.net).map(|n| n.pins.len()).unwrap_or(0);
        if pin_count == 1 {
            out.push(CheckResult {
                check: "isolated_pin_label".into(),
                status: CheckStatus::Warn,
                location: Some(format!("{}:{},{}", l.net, l.at.x, l.at.y)),
                hint: Some(format!("label '{}' is connected to only one pin", l.net)),
                detail: None,
            });
            ok = false;
        }
    }
    if ok {
        out.push(CheckResult::pass("isolated_pin_label"));
    }
}

/// (i4) `footprint_link_issues`, ported from KiCad's own `ERC_TESTER::
/// TestFootprintLinkIssues` (`erc.cpp`): is a symbol instance's assigned
/// Footprint field's library nickname a real, currently-installed KiCad
/// footprint library -- the same "is it configured" question
/// [`check_lib_symbol_issues`] asks of a symbol's own library, now asked of
/// `SymbolInstance::footprint`. Scoped to that one sub-case (KiCad's own
/// `KIFACE_TEST_FOOTPRINT_LINK_NO_LIBRARY` result) -- the only one this
/// port's measured corpus has ever needed; "footprint not found in an
/// existing library" (`KIFACE_TEST_FOOTPRINT_LINK_NO_FOOTPRINT`) and
/// "doesn't match this symbol's own `fp_filters`" (`TestFootprintFilters`,
/// a different upstream function entirely, gated off by KiCad's own
/// default `ERCE_FOOTPRINT_FILTERS` severity of Ignore) are both real,
/// separate checks this port does not implement.
fn check_footprint_link_issues(sch: &eda_model::ir::SchematicSection, out: &mut Vec<CheckResult>) {
    let root = crate::footprint_lib::default_footprint_library_root();
    let mut ok = true;
    for sym in &sch.symbols {
        let Some((lib_name, fp_name)) = sym.footprint.split_once(':') else { continue };
        if lib_name.is_empty() || fp_name.is_empty() {
            continue;
        }
        if !root.join(format!("{lib_name}.pretty")).is_dir() {
            out.push(CheckResult {
                check: "footprint_link_issues".into(),
                status: CheckStatus::Warn,
                location: Some(sym.id.clone()),
                hint: Some(format!("The current configuration does not include the footprint library '{lib_name}'")),
                detail: None,
            });
            ok = false;
        }
    }
    if ok {
        out.push(CheckResult::pass("footprint_link_issues"));
    }
}

/// (h) Duplicate reference designators. KiCad's own `ERCE_DUPLICATE_REFERENCE`
/// check (`SCH_REFERENCE_LIST::CheckAnnotation`) is fully implemented
/// upstream but, in this source snapshot, is never actually invoked from
/// `ERC_TESTER::RunTests()`/`CONNECTION_GRAPH::RunERC()` — so real
/// `kicad-cli sch erc` does not currently report it at all. This project
/// checks it anyway (the task this module was ported for asks for it by
/// name, and it is a real problem worth catching, imported-file or not),
/// using KiCad's own settings-key name for consistency.
fn check_duplicate_references(sch: &eda_model::ir::SchematicSection, out: &mut Vec<CheckResult>) {
    let mut seen: BTreeMap<&str, u32> = BTreeMap::new();
    for s in &sch.symbols {
        *seen.entry(s.id.as_str()).or_default() += 1;
    }
    let mut ok = true;
    for (r, count) in seen {
        if count > 1 {
            out.push(CheckResult::fail("duplicate_reference", r, format!("reference '{r}' is used by {count} symbols")));
            ok = false;
        }
    }
    if ok {
        out.push(CheckResult::pass("duplicate_reference"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use eda_engine::{derive_schematic, EngineOptions};
    use eda_model::{Net, Part, Pin, PinKind};

    fn pin(number: &str, name: &str, kind: PinKind) -> Pin {
        Pin { number: number.into(), name: Some(name.into()), kind }
    }
    fn part(reference: &str, pins: Vec<Pin>) -> Part {
        Part { reference: reference.into(), mpn: None, lcsc: None, value: None, package: None, footprint: None, symbol: None, datasheet: None, pins, body_um: None, edge: None }
    }
    fn net(name: &str, pins: &[&str]) -> Net {
        Net { name: name.into(), pins: pins.iter().map(|s| s.to_string()).collect() }
    }

    fn ldo_model() -> ConstraintModel {
        let u1 = part(
            "U1",
            vec![pin("1", "VIN", PinKind::Power), pin("2", "GND", PinKind::Ground), pin("3", "EN", PinKind::Signal), pin("4", "VOUT", PinKind::Power), pin("5", "NC", PinKind::Nc)],
        );
        let cin = part("CIN", vec![pin("1", "1", PinKind::Passive), pin("2", "2", PinKind::Ground)]);
        let cout = part("COUT", vec![pin("1", "1", PinKind::Passive), pin("2", "2", PinKind::Ground)]);
        ConstraintModel {
            parts: vec![u1, cin, cout],
            nets: vec![net("VIN", &["U1.1", "CIN.1", "U1.3"]), net("VOUT", &["U1.4", "COUT.1"]), net("GND", &["U1.2", "CIN.2", "COUT.2"])],
            ..Default::default()
        }
    }

    #[test]
    fn matrix_matches_kicad_input_input_is_ok_output_output_is_error() {
        assert_eq!(matrix_lookup(Input, Input), OK);
        assert_eq!(matrix_lookup(Output, Output), ERR);
        assert_eq!(matrix_lookup(PowerOut, PowerOut), ERR);
        assert_eq!(matrix_lookup(NoConnect, Passive), ERR);
        assert_eq!(matrix_lookup(Unspecified, Free), OK);
        assert_eq!(matrix_lookup(Passive, Passive), OK);
    }

    #[test]
    fn clean_ldo_design_passes_every_check() {
        let model = ldo_model();
        let design = derive_schematic(&model, &EngineOptions::new(1, "erc-test")).unwrap();
        let results = check_erc(&design, &model);
        let fails: Vec<_> = results.iter().filter(|r| r.status == CheckStatus::Fail).collect();
        assert!(fails.is_empty(), "{fails:#?}");
    }

    #[test]
    fn power_net_with_no_output_pin_and_no_flag_is_flagged() {
        // A "power" net (VDD is power_in-mapped for a Power/Ground-kind
        // pin) whose only other member is a Passive pin: no power_out
        // anywhere, so it must fail `power_pin_not_driven` — proven in
        // isolation from `derive_schematic`'s own `PWR_FLAG` auto-fix
        // (which would otherwise insert exactly the driver this test
        // wants absent) by clearing whatever power symbols it placed.
        let u1 = part("U1", vec![pin("1", "VDD", PinKind::Power)]);
        let r1 = part("R1", vec![pin("1", "1", PinKind::Passive), pin("2", "2", PinKind::Passive)]);
        let model = ConstraintModel { parts: vec![u1, r1], nets: vec![net("VDD", &["U1.1", "R1.1"])], ..Default::default() };
        let mut design = derive_schematic(&model, &EngineOptions::new(1, "h")).unwrap();
        design.schematic.as_mut().unwrap().power_symbols.clear();
        let results = check_erc(&design, &model);
        assert!(results.iter().any(|r| r.check == "power_pin_not_driven" && r.status == CheckStatus::Fail), "{results:#?}");
    }

    #[test]
    fn power_flag_satisfies_power_pin_not_driven() {
        // The same undriven net as above, but this time through
        // `derive_schematic` unmodified: its own auto-inserted `PWR_FLAG`
        // (a `power_out`-typed pin, exactly like a real one — see
        // `eda_model::symbol::power_flag`) must be recognized by
        // `check_driven_pins` as a driver, via `net_members` folding
        // `sch.power_symbols` into the same list real pins use. This is
        // the fix this module's `Member`/`net_members` exist for: without
        // it, a generator that correctly placed a `PWR_FLAG` would still
        // fail its own ERC.
        let u1 = part("U1", vec![pin("1", "VDD", PinKind::Power)]);
        let r1 = part("R1", vec![pin("1", "1", PinKind::Passive), pin("2", "2", PinKind::Passive)]);
        let model = ConstraintModel { parts: vec![u1, r1], nets: vec![net("VDD", &["U1.1", "R1.1"])], ..Default::default() };
        let design = derive_schematic(&model, &EngineOptions::new(1, "h")).unwrap();
        assert!(!design.schematic.as_ref().unwrap().power_symbols.is_empty(), "generator should have placed a PWR_FLAG on the undriven VDD net");
        let results = check_erc(&design, &model);
        assert!(!results.iter().any(|r| r.check == "power_pin_not_driven" && r.status == CheckStatus::Fail), "{results:#?}");
    }

    #[test]
    fn duplicate_reference_detected() {
        let model = ldo_model();
        let mut design = derive_schematic(&model, &EngineOptions::new(1, "h")).unwrap();
        let sch = design.schematic.as_mut().unwrap();
        let dup = sch.symbols[0].clone();
        sch.symbols.push(dup);
        let results = check_erc(&design, &model);
        assert!(results.iter().any(|r| r.check == "duplicate_reference" && r.status == CheckStatus::Fail));
    }

    #[test]
    fn nc_pin_never_flagged_unconnected() {
        let model = ldo_model();
        let design = derive_schematic(&model, &EngineOptions::new(1, "h")).unwrap();
        let results = check_erc(&design, &model);
        assert!(!results.iter().any(|r| r.check == "pin_not_connected" && r.status == CheckStatus::Fail && r.location.as_deref() == Some("U1.5")));
    }

    /// `U1` above is a synthetic box, whose `nc` pin already reports the
    /// canonical `"no_connect"` electrical type with no real library in
    /// the way. A connector that resolves to a *real* (here, built-in
    /// parametric) library symbol is not so simple: every
    /// `Connector_Generic` pin is generically `"passive"` (see
    /// `eda_model::symbol::conn_01x`) whether or not this project marked
    /// it `nc` -- `examples/nc_pins.yaml`'s own `J1` pins 4-6 are exactly
    /// this shape, and `resolve_pins` preferring the resolved symbol's
    /// ordinary type over `pin.kind == Nc` is precisely the bug that made
    /// the real CLI pipeline (`eda pipeline nc_pins.yaml`) fail
    /// `pin_not_connected` on them the moment `check_erc` was wired in.
    #[test]
    fn nc_pin_on_a_real_library_connector_never_flagged_unconnected() {
        let j1 = part(
            "J1",
            vec![pin("1", "VIN", PinKind::Power), pin("2", "GND", PinKind::Ground), pin("3", "VOUT", PinKind::Power), pin("4", "NC1", PinKind::Nc), pin("5", "NC2", PinKind::Nc)],
        );
        let u1 = part("U1", vec![pin("1", "VIN", PinKind::Power), pin("2", "GND", PinKind::Ground), pin("3", "VOUT", PinKind::Power)]);
        let model = ConstraintModel {
            parts: vec![u1, j1],
            nets: vec![net("VIN", &["U1.1", "J1.1"]), net("GND", &["U1.2", "J1.2"]), net("VOUT", &["U1.3", "J1.3"])],
            ..Default::default()
        };
        let design = derive_schematic(&model, &EngineOptions::new(1, "nc-real-lib-erc")).unwrap();
        let results = check_erc(&design, &model);
        let fails: Vec<_> = results.iter().filter(|r| r.status == CheckStatus::Fail).collect();
        assert!(fails.is_empty(), "{fails:#?}");
    }

    #[test]
    fn genuinely_unconnected_pin_is_flagged() {
        let u1 = part("U1", vec![pin("1", "SIG", PinKind::Signal)]);
        let model = ConstraintModel { parts: vec![u1], nets: vec![], ..Default::default() };
        let design = derive_schematic(&model, &EngineOptions::new(1, "h")).unwrap();
        let results = check_erc(&design, &model);
        assert!(results.iter().any(|r| r.check == "pin_not_connected" && r.status == CheckStatus::Fail));
    }

    #[test]
    fn check_erc_folds_in_the_style_checks_too() {
        // One engine: a style/readability finding (ported from the former
        // `eda-gates::check_schematic`) must show up from the same
        // `check_erc` call as an electrical one, not a second tool a
        // caller has to remember to also run.
        let u1 = part("U1", vec![pin("1", "SIG", PinKind::Signal)]);
        let model = ConstraintModel { parts: vec![u1], nets: vec![], ..Default::default() };
        let design = derive_schematic(&model, &EngineOptions::new(1, "h")).unwrap();
        let results = check_erc(&design, &model);
        assert!(results.iter().any(|r| r.check == "schematic_offgrid"), "{results:#?}");
    }

    #[test]
    fn an_excluded_finding_is_reported_but_no_longer_fails() {
        let u1 = part("U1", vec![pin("1", "SIG", PinKind::Signal)]);
        let model = ConstraintModel { parts: vec![u1], nets: vec![], ..Default::default() };
        let design = derive_schematic(&model, &EngineOptions::new(1, "h")).unwrap();

        let plain = check_erc(&design, &model);
        let needy = plain.iter().find(|r| r.check == "pin_not_connected" && r.status == CheckStatus::Fail).expect("U1.1 is genuinely unconnected");
        let location = needy.location.clone().unwrap();

        let mut exclusions = Exclusions::new();
        exclusions.exclude("pin_not_connected", location.clone());
        let excluded = check_erc_excluding(&design, &model, &exclusions);
        let found = excluded.iter().find(|r| r.check == "pin_not_connected" && r.location.as_deref() == Some(location.as_str())).expect("the finding is still reported");
        assert_eq!(found.status, CheckStatus::Excluded);
        assert!(!excluded.iter().any(|r| r.status == CheckStatus::Fail), "an excluded finding must not still fail the run: {excluded:#?}");
        // An unrelated genuine failure elsewhere is untouched by an
        // exclusion that does not name it.
        assert_eq!(plain.iter().filter(|r| r.status == CheckStatus::Fail).count() - 1, excluded.iter().filter(|r| r.status == CheckStatus::Fail).count());
    }
}
