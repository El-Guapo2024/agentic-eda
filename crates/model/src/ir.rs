//! The geometry IR — `design.json`.
//!
//! One nested document that fills in stage by stage (schematic → placement →
//! routing). Frozen sections never change; the running loop regenerates only
//! its own section across N candidates. All coordinates are integer
//! micrometers: floats in a hashed file are a canonicalization landmine.
//!
//! Canonical form contract (what makes byte-hash == content-hash):
//! - serialization uses sorted object keys (serde_json with BTreeMap-backed
//!   structs / preserve_order off) and no floats anywhere
//! - all IDs are derived from intent (reference designators, net names),
//!   never random — the same intent always yields the same IDs
//! - vectors are sorted by `id` before serialization

use serde::{Deserialize, Serialize};

/// Integer micrometers. 1 mm = 1_000 um.
pub type Um = i64;

/// Rotation in millidegrees (0..360_000) so 45° etc. stay exact integers.
pub type Millideg = u32;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default, Serialize, Deserialize)]
pub struct Point {
    pub x: Um,
    pub y: Um,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Design {
    /// Schema version of this document; migrations live in the kernel.
    pub schema: u32,
    pub provenance: Provenance,
    /// Filled by loop 1 (E1). Artifact 1 when frozen.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schematic: Option<SchematicSection>,
    /// Schematic-derived net list: `None` for every design built by the
    /// ordinary intent -> schematic -> placement -> routing pipeline (the
    /// intent's own `ConstraintModel::nets` is already authoritative
    /// there, and `crates/cli/src/board.rs::load` leaves it alone). Set,
    /// and from then on authoritative, the first time a studio schematic
    /// `Cmd` (`crates/ops::Cmd::domain` -- `Domain::Schematic`) lands on
    /// this design: `board::reconcile_schematic` retraces `schematic`'s
    /// own wires/labels/power symbols/no-connects (the same union-find
    /// `eda_kicad::sch_import::reconcile` runs on an imported `.kicad_sch`)
    /// and writes the result here, so a hand-drawn wire that merges two
    /// nets, or a label that renames one, is reflected everywhere a net
    /// matters -- the PCB ratsnest, ERC's pin-electrical checks -- without
    /// ever touching the intent file itself. See GAPS.md #1's "one
    /// netlist" rule and `crates/ops::Cmd`'s eeschema verbs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub nets: Option<Vec<crate::Net>>,
    /// Filled by loop 2 (E2). Artifact 2 when frozen.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub placement: Option<PlacementSection>,
    /// Filled by loop 3 (E3). Artifact 3 when frozen.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub routing: Option<RoutingSection>,
    /// Free-standing board graphics and text -- KiCad's `PCB_SHAPE` and
    /// `PCB_TEXT` -- addressable the same way routing items are. Not a
    /// staged artifact like the three above (nothing freezes it): it exists
    /// once anything has drawn on the board, whichever stage that happened
    /// in, so it is its own optional section rather than folded into
    /// placement or routing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub drawings: Option<DrawingsSection>,
    /// The Footprint Editor's own content (GAPS.md #8): footprint
    /// definitions the user created or edited here, by name ("Lib:Name",
    /// or a bare name for one authored from scratch). This is the one
    /// section `crate::footprint::Footprint`/`Pad` (the *intent*-derived,
    /// frozen `ConstraintModel::footprints`) has an editable counterpart
    /// of -- see `LibraryFootprint`'s own doc for why it is a distinct
    /// type rather than reusing `crate::footprint::Pad` directly. Absent
    /// on a `design.json` written before this editor existed, same as
    /// every other optional section here; a board footprint keeps
    /// pointing at its `Part::footprint` name regardless of whether that
    /// name has ever been opened here.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub footprint_library: Option<FootprintLibrarySection>,
    /// The Symbol Editor tab's own content: symbol definitions the user
    /// created or edited here, by `lib_id` -- the exact schematic-side
    /// counterpart of `footprint_library` above (see `SymbolLibrarySection`'s
    /// own doc for why it is a distinct section rather than folded into
    /// `schematic`). Absent on a `design.json` written before this editor
    /// existed, same convention as every other optional section here.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub symbol_library: Option<SymbolLibrarySection>,
    /// Every non-root sheet's own drawn content (GAPS.md #6), keyed by its
    /// `SheetInstance::file` name -- KiCad's own "one `SCH_SCREEN` per
    /// unique file" convention: two `SheetInstance`s naming the same file
    /// (the same screen placed more than once) share the one entry here.
    /// `schematic` above is always the *root* sheet's own content, never an
    /// entry in this map -- a single-sheet design (everything before this
    /// field existed, and most designs even now) has this as `None`, same
    /// "absent means nothing to add" convention as every other optional
    /// section. See `crate::hierarchy`'s own doc (eda-kicad) for how this
    /// flattens into one netlist, and `SchematicSection::instance_overrides`
    /// for how a multiply-placed screen's own symbols get a different
    /// Reference per placement.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sheet_contents: Option<std::collections::BTreeMap<String, SchematicSection>>,
    /// Project-scoped bus aliases (GAPS.md #20) — see [`BusAlias`]'s own
    /// doc for why these live here (one list for the whole design) rather
    /// than inside each `SchematicSection`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub bus_aliases: Vec<BusAlias>,
}

impl Design {
    /// Backfill `id` on every routing/drawing item that does not have one
    /// yet (empty string: absent from an old `design.json`, or from a
    /// `RoutingSection`/`DrawingsSection` built by hand in a test). Ids are
    /// derived from each item's own content (see `RoutingSection` and
    /// `DrawingsSection`), so calling this on the same design ever again is
    /// a no-op, and calling it on two structurally-identical designs gives
    /// the same ids both times.
    pub fn assign_missing_ids(&mut self) {
        if let Some(sch) = &mut self.schematic {
            sch.assign_missing_ids();
        }
        if let Some(rt) = &mut self.routing {
            rt.assign_missing_ids();
        }
        if let Some(dr) = &mut self.drawings {
            dr.assign_missing_ids();
        }
        if let Some(lib) = &mut self.footprint_library {
            lib.assign_missing_ids();
        }
        if let Some(lib) = &mut self.symbol_library {
            lib.assign_missing_ids();
        }
        if let Some(screens) = &mut self.sheet_contents {
            for sch in screens.values_mut() {
                sch.assign_missing_ids();
            }
        }
    }
}

/// Every design.json can be traced to exactly what produced it.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Provenance {
    pub engine_version: String,
    /// blake3 of the canonical intent slice this document was derived from.
    pub intent_hash: String,
    pub seed: u64,
    /// Per-stage hashes of the consumed intent slice, so adding a routing
    /// constraint later does not unfreeze the schematic (staged intent).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub stage_hashes: Vec<StageHash>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StageHash {
    pub stage: Stage,
    pub intent_slice_hash: String,
    /// Set when the user acked this stage's render; freezes the section.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub frozen_at: Option<String>, // RFC 3339
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Stage {
    Schematic,
    Placement,
    Routing,
}

// ---------- stage 1: schematic ----------

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SchematicSection {
    /// Sorted by `(id, unit)`. Several entries can share one `id`: a
    /// multi-unit part (an op-amp's gates, a logic chip's one shared
    /// power unit, ...) is one `ConstraintModel::Part`/one footprint, one
    /// reference, placed as several `SymbolInstance`s -- one per used unit
    /// -- that all carry that same `id` and differ only by `unit`. A
    /// single-unit part (everything before multi-unit support existed, and
    /// the overwhelming majority of parts even now) still has exactly one.
    pub symbols: Vec<SymbolInstance>,
    /// Sorted by (net, then first point).
    pub wires: Vec<Wire>,
    /// Net label placements, sorted by (net, at).
    #[serde(default)]
    pub labels: Vec<NetLabel>,
    /// `T`: free-standing text, sorted by (content, at). See
    /// [`SchematicText`]'s own doc for why it's separate from `labels`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub texts: Vec<SchematicText>,
    /// Power symbols (KiCad's `power:GND`/`power:VCC`/... instances) —
    /// one per power/ground pin, in place of a wire to a rail. Sorted by
    /// `id`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub power_symbols: Vec<PowerSymbol>,
    /// No-connect flags (KiCad's `no_connect`): one at every pin the
    /// intent marks `nc`, so ERC does not report it unconnected. Sorted
    /// by `at`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub no_connects: Vec<NoConnect>,
    /// Bus entries (GAPS.md #20) — see [`BusEntry`]'s own doc.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub bus_entries: Vec<BusEntry>,
    /// Explicit junctions (`J`) -- see [`Junction`]. Additive.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub junctions: Vec<Junction>,
    /// Graphic lines on the notes layer (`I`) -- see [`SchLine`]. Additive.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub lines: Vec<SchLine>,
    /// Drawn shapes, text boxes, rule areas, directive labels, locks and body styles -- see
    /// [`crate::sch_extras::SchExtras`]. Additive; empty (and not written) for every design that
    /// never used those tools.
    #[serde(default, skip_serializing_if = "crate::sch_extras::SchExtras::writes_nothing")]
    pub extras: crate::sch_extras::SchExtras,
    /// Accepted ("excluded") ERC findings -- `dialog_erc.cpp`'s own
    /// per-sheet `SCHEMATIC::RecordERCExclusions`. Sorted by (check,
    /// location); see [`ErcExclusion`]'s own doc for why it has no `id`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub erc_exclusions: Vec<ErcExclusion>,
    /// Per-design ERC pin-to-pin conflict matrix override --
    /// `ERC_SETTINGS::m_PinMap` (`erc_settings.cpp`), edited by
    /// `panel_setup_pinmap.cpp`. `None` (every design written before this
    /// existed) means KiCad's own default map (`m_defaultPinMap`): the
    /// derived project file then carries no `pin_map` and kicad-cli's ERC
    /// uses its own table. See [`ErcPinMap`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub erc_pin_map: Option<ErcPinMap>,
    /// User-defined symbol fields (`SCH_FIELD`s beyond Reference/Value/
    /// Footprint/Datasheet), edited from the Symbol Fields Table
    /// (`dialog_symbol_fields_table.cpp`): reference -> (field name ->
    /// text). Part-wide, keyed by reference, for the same reason
    /// `value`/`footprint`/`datasheet` are (every unit of a multi-unit
    /// part shares them). A key with an empty value is a user-added
    /// column KiCad's `ApplyData` would also create as an empty field
    /// (`userAdded`).
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub user_fields: std::collections::BTreeMap<String, std::collections::BTreeMap<String, String>>,
    /// Where the fields of each placed symbol, power symbol and sheet are drawn, by [`field_key`] (a power symbol's id, a sheet's id):
    /// see [`FieldPlacement`]. An item with no entry has its fields placed the way KiCad's Autoplace Fields would
    /// (`eda_engine::fields`), so a design written before this existed reads the same as one that has them.
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub field_layout: std::collections::BTreeMap<String, Vec<FieldPlacement>>,
    /// Title block. `None` keeps relying on the caller-supplied
    /// `ExportMeta` (title/date) the way every export always has.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title_block: Option<TitleBlock>,
    /// Child hierarchical sheets placed directly on *this* sheet -- each
    /// one's own content lives in `Design::sheet_contents[sheet.file]`, not
    /// here (GAPS.md #6). Empty for a single-sheet design, same as always.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sheets: Vec<SheetInstance>,
    /// `SCH_SYMBOL_INSTANCE`-style per-placement overrides for this
    /// screen's own symbols, keyed by `SymbolPathOverride::parent_sheet_instance_id` --
    /// only ever non-empty on a screen placed by more than one
    /// [`SheetInstance`] (the content this `SchematicSection` belongs to is
    /// reused), and empty on the root sheet (nothing ever places it). See
    /// [`SymbolPathOverride`]'s own doc for why `at`, not a stored id, is
    /// the per-symbol key.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub instance_overrides: Vec<SymbolPathOverride>,
    /// True when this section was built by `eda_kicad::import_kicad_sch`
    /// from a real `.kicad_sch` file, rather than by this project's own
    /// `derive_schematic`. The two disagree on what `SymbolInstance::at`
    /// means: `derive_schematic` always places it at the engine's own
    /// "top-left of a synthesized box" corner (every downstream consumer —
    /// rendering, the exporter's `baked_local`, and the schematic checks'
    /// own pin resolution — agrees on that convention), while a real file's
    /// `(symbol (at X Y))` is the symbol's *native* KiCad origin, which for
    /// a real library symbol is almost never a bounding-box corner. Mixing
    /// the two conventions silently computes the wrong absolute pin
    /// position for an imported real symbol (`resolve_pins`' original bug
    /// this field exists to fix — see its own doc comment); setting this
    /// flag lets that one function recover the correct point the same way
    /// `import_kicad_sch::reconcile` itself does, without changing
    /// anything for a schematic this project generated itself. `#[serde(default)]`
    /// so every existing `design.json`/engine-constructed section (and
    /// every other crate's existing `SchematicSection { .. }` literal)
    /// keeps today's (correct, for them) engine-box behavior unchanged.
    #[serde(default)]
    pub imported_from_kicad: bool,
}

fn d_unit_one() -> u32 {
    1
}
fn is_unit_one(u: &u32) -> bool {
    *u == 1
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SymbolInstance {
    /// Reference designator from intent ("U1") — the stable ID.
    pub id: String,
    pub at: Point,
    pub rot: Millideg,
    /// `X` ("Mirror Horizontally", KiCad's `SYM_MIRROR_Y`): negates local X
    /// before rotation. See [`Cmd::MirrorSymbol`](../../eda_ops/enum.Cmd.html)'s
    /// own doc.
    #[serde(default)]
    pub mirrored: bool,
    /// `Y` ("Mirror Vertically", KiCad's `SYM_MIRROR_X`): the other axis --
    /// never both at once in practice (KiCad's own symbols never carry two
    /// mirror flags; `transform_local_point` composes them as "cancel the
    /// library Y-up/Y-down flip instead of negating local X" rather than
    /// a true second negation, see its own doc comment for why). Added
    /// after `mirrored` already existed and was widely depended on, so
    /// this is a new, separate field rather than widening `mirrored` into
    /// an enum -- see PARITY-sch.md's own note on why that was deferred
    /// originally.
    #[serde(default)]
    pub mirror_y: bool,
    /// KiCad library id this instance draws from ("Device:R",
    /// "Regulator_Linear:AMS1117-3.3"), resolved by
    /// `eda_model::symbol::resolve_lib_id` — see [`crate::Part::symbol`].
    /// Empty on a `design.json` written before this field existed; the
    /// exporter treats that the same as the synthetic `"eda:<id>"` form
    /// (a generic box synthesized from the part's own pins).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub lib_id: String,
    /// KiCad unit index (1-based) -- which unit of a multi-unit symbol this
    /// placed instance draws (see `SchematicSection::symbols`'s own doc on
    /// several instances sharing one `id`). Always 1 for a single-unit
    /// part, which is every part this port places on its own
    /// (`derive_schematic`'s own generator never splits a resolved
    /// multi-unit symbol across several placed instances -- a documented
    /// scope limit, not a bug: every unit it needs is still present as
    /// `ConstraintModel::Part::pins`, just all drawn on one instance) --
    /// real multi-unit placement is read from an imported `.kicad_sch`, or
    /// built up one `Cmd::AddSymbol` at a time in the studio editor.
    #[serde(default = "d_unit_one", skip_serializing_if = "is_unit_one")]
    pub unit: u32,
    /// The instance's own `Value` field — carried here (not just read from
    /// `ConstraintModel::Part::value`) so a schematic read from a foreign
    /// `.kicad_sch`, with no accompanying intent, is still self-contained.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub value: String,
    /// The instance's own `Footprint` field, same reasoning as `value`.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub footprint: String,
    /// The instance's own `Datasheet` field, same reasoning as `value`.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub datasheet: String,
    /// `SCH_SYMBOL::GetDNP` ("Do not populate", `eeschema.EditorControl.setDNP`): the part is drawn but not
    /// assembled. The derived `.kicad_sch` carries it as `(dnp yes)`, so kicad-cli's BOM, netlist and ERC see it.
    /// Kept in step across every placed unit of one reference (`SCH_EDIT_TOOL::SetAttribute` collects the other units).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub dnp: bool,
    /// `GetExcludedFromBOM` (`setExcludeFromBOM`): left out of the bill of materials -- `(in_bom no)`.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub exclude_from_bom: bool,
    /// `GetExcludedFromBoard` (`setExcludeFromBoard`): no footprint for it on the board -- `(on_board no)`.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub exclude_from_board: bool,
    /// `GetExcludedFromSim` (`setExcludeFromSimulation`): left out of the SPICE netlist -- `(exclude_from_sim yes)`.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub exclude_from_sim: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Wire {
    /// Stable id (`wire_xxxxxxxxxxxx`), deterministic from `pts` — same
    /// `next_item_id` scheme as `Track`/`Shape`/etc (see
    /// `SchematicSection::assign_missing_ids`). Empty on a `design.json`
    /// written before the studio editor could select/delete a wire by id;
    /// backfilled the same way those other items are.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub id: String,
    pub net: String,
    /// Pins this wire lands on, as "REF.PIN" ("U1.3") — borrowed from
    /// Circuit JSON's first-class ports: gates check connectivity exactly
    /// instead of re-inferring it from geometry.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pins: Vec<String>,
    /// Polyline in sheet coordinates.
    pub pts: Vec<Point>,
    /// True for a bus wire (KiCad's `LAYER_BUS` vs `LAYER_WIRE` -- the same
    /// `SCH_LINE`, just a different layer; GAPS.md #20). `net` on a bus
    /// wire carries whatever name/vector/group text a touching label gave
    /// it, same mechanism as a plain wire -- this flag is what tells
    /// `crate::bus` (eda-kicad) that name needs bus *expansion* rather than
    /// being one plain net, and is also what `bus_to_net_conflict` compares
    /// across a resolved group to catch a plain wire touching a bus
    /// directly with no [`BusEntry`] between them.
    #[serde(default)]
    pub bus: bool,
}

impl Wire {
    fn id_seed(&self) -> String {
        let pts: Vec<String> = self.pts.iter().map(|p| format!("{},{}", p.x, p.y)).collect();
        pts.join(";")
    }
}

/// Local-label shape: unused (locals carry no shape), but a global/
/// hierarchical label's own signal-flow shape — KiCad's own 5-value
/// vocabulary, shared with a hierarchical sheet pin's shape.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LabelShape {
    Input,
    Output,
    Bidirectional,
    TriState,
    #[default]
    Passive,
}

/// Which of KiCad's three label kinds a [`NetLabel`] is. Local connects
/// same-named nets anywhere on *this* sheet (what `derive_schematic`
/// emits today — a single-sheet design has no need for the other two);
/// Global connects same-named nets across the whole schematic/hierarchy;
/// Hierarchical connects a sheet's own net out to a parent sheet's pin of
/// the same name. Kept as a real enum (not inferred from context) so a
/// schematic read from a real, hierarchical KiCad file round-trips which
/// kind each label actually was.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(tag = "scope", rename_all = "snake_case")]
pub enum LabelKind {
    #[default]
    Local,
    Global { shape: LabelShape },
    Hierarchical { shape: LabelShape },
}

impl LabelKind {
    pub fn is_local(&self) -> bool {
        *self == LabelKind::Local
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NetLabel {
    /// Stable id (`lbl_xxxxxxxxxxxx`) — see `Wire::id`'s doc.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub id: String,
    pub net: String,
    pub at: Point,
    #[serde(default, skip_serializing_if = "LabelKind::is_local")]
    pub kind: LabelKind,
}

impl NetLabel {
    fn id_seed(&self) -> String {
        format!("{}|{},{}", self.net, self.at.x, self.at.y)
    }
}

/// `T`: free-standing text (KiCad's own `(text ...)`) — purely cosmetic,
/// unlike [`NetLabel`]: it names no net and never participates in
/// [`crate::symbol::builtin`]/`reconcile`'s connectivity pass, so adding it
/// touches none of the "one netlist" machinery `Wire`/`NetLabel`/
/// `PowerSymbol`/`NoConnect` all feed. Deliberately minimal next to KiCad's
/// own `SCH_TEXT` (no bold/italic/justify/color yet) — those are easy,
/// independent additions later if a real need shows up; starting minimal
/// keeps this first cut small and easy to review, per the IR's own
/// "extend additively" rule.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SchematicText {
    /// Stable id (`txt_xxxxxxxxxxxx`) — see `Wire::id`'s doc.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub id: String,
    pub content: String,
    pub at: Point,
    #[serde(default)]
    pub angle: Millideg,
    pub size_um: Um,
}

impl SchematicText {
    fn id_seed(&self) -> String {
        format!("{}|{},{}", self.content, self.at.x, self.at.y)
    }
}

/// A KiCad power symbol instance (`power:GND`, `power:VCC`, a custom
/// rail...) — what a power/ground pin gets instead of a wire to a rail.
/// Placed with its own connection pin exactly on the host pin's stub tip
/// (KiCad treats two coincident pins as joined with no wire needed), so
/// this never needs a `Wire` entry of its own.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PowerSymbol {
    /// Synthetic reference, KiCad's own auto-numbered convention
    /// ("#PWR01"). Unique within the sheet, but deliberately not a
    /// `ConstraintModel::Part` reference — a power symbol is not a part.
    pub id: String,
    /// "power:GND", "power:VCC", "power:+3V3", ... — see
    /// `eda_model::symbol::builtin`.
    pub lib_id: String,
    pub at: Point,
    pub rot: Millideg,
    /// The net this symbol asserts (its own `Value` field in KiCad).
    pub net: String,
    /// The real part pin this symbol is attached to, as "REF.PIN" — so
    /// ERC and any other consumer can tell which physical pin a power
    /// symbol speaks for without re-deriving it from position.
    pub pin: String,
}

/// A KiCad `no_connect` flag: an explicit "this pin is deliberately
/// unconnected" marker, required at every `nc`-kind pin or KiCad's ERC
/// reports it unconnected.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NoConnect {
    /// Stable id (`nc_xxxxxxxxxxxx`) — see `Wire::id`'s doc.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub id: String,
    pub at: Point,
    /// The pin this flag marks, as "REF.PIN".
    pub pin: String,
}

impl NoConnect {
    fn id_seed(&self) -> String {
        format!("{},{}", self.at.x, self.at.y)
    }
}

/// A bus entry (`SCH_BUS_WIRE_ENTRY`, GAPS.md #20): a short diagonal stub
/// tying one specific member net into a bus. `at` and `at + size` are its
/// two endpoints in sheet coordinates (`size` carries the sign of each
/// axis, same as KiCad's own `(at)(size)` pair -- a negative component
/// picks one of the other three diagonal quadrants); which endpoint is
/// "the bus side" is never stored, only read off geometry at ERC/connectivity
/// time (`crate::bus` (eda-kicad): whichever endpoint lands on a [`Wire`]
/// with `bus: true`). KiCad's companion `SCH_BUS_BUS_ENTRY` (bus-to-bus) is
/// deliberately not ported: real KiCad's own file writer never emits one
/// any more (it silently downgrades to a plain bus line on save) and no UI
/// action in current KiCad creates one, so there is nothing to round-trip.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BusEntry {
    /// Stable id (`bent_xxxxxxxxxxxx`) — see `Wire::id`'s doc.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub id: String,
    pub at: Point,
    /// Signed offset from `at` to this entry's other endpoint — real KiCad
    /// always uses a fixed ±100mil (±2540um) magnitude on each axis, but
    /// nothing here enforces that; an imported file's own stored size is
    /// kept exactly.
    pub size: Point,
}

impl BusEntry {
    fn id_seed(&self) -> String {
        format!("{},{}|{},{}", self.at.x, self.at.y, self.size.x, self.size.y)
    }
}

/// An explicit junction (`SCH_JUNCTION`, the `J` tool): joins every wire that passes through or ends
/// at `at`, which is what turns two wires that merely *cross* into one net -- wires that end on each
/// other, or an end landing on another wire's middle, already join by geometry without one. It is
/// written to the `.kicad_sch` as a `(junction ...)` item and read back from one. Additive: absent
/// from every design that never placed one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Junction {
    /// Stable id (`jct_xxxxxxxxxxxx`) -- see `Wire::id`'s doc.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub id: String,
    pub at: Point,
}

impl Junction {
    fn id_seed(&self) -> String {
        format!("{},{}", self.at.x, self.at.y)
    }
}

/// A graphic polyline on the schematic's notes layer (`SCH_LINE` on `LAYER_NOTES`, drawn by the
/// `Draw Lines` tool, `I`): decoration with no electrical meaning -- never part of any net, never a
/// connection point. Written to the `.kicad_sch` as a `(polyline ...)` item and read back from one.
/// `width_um` 0 means the default line width. Additive: absent from every design with none.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SchLine {
    /// Stable id (`sln_xxxxxxxxxxxx`) -- see `Wire::id`'s doc.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub id: String,
    pub pts: Vec<Point>,
    #[serde(default, skip_serializing_if = "is_zero_um")]
    pub width_um: Um,
}

fn is_zero_um(v: &Um) -> bool {
    *v == 0
}

impl SchLine {
    fn id_seed(&self) -> String {
        let pts: Vec<String> = self.pts.iter().map(|p| format!("{},{}", p.x, p.y)).collect();
        pts.join(";")
    }
}

/// A project-scoped bus alias (`BUS_ALIAS`, GAPS.md #20): a name that
/// stands in for a fixed list of member net names anywhere a bus vector/
/// group name could otherwise be written (`{MY_ALIAS}`) or, bare, as a bus
/// wire's own name directly. Real KiCad stores these in the `.kicad_pro`
/// project file (`SCHEMATIC::updateProjectBusAliases`) with a *legacy*
/// per-screen `(bus_alias ...)` `.kicad_sch` reader kept only for
/// backward compatibility (`SCH_IO_KICAD_SEXPR_PARSER::parseBusAlias`,
/// forwarding into the same project-level list) -- this project has no
/// `.kicad_pro` reader/writer at all yet, so `crate::import`/`crate::lib`
/// (eda-kicad) round-trip aliases through that same legacy per-screen
/// `(bus_alias "NAME" (members "A" "B"))` block instead, written into the
/// root screen's own file on export and collected from every screen on
/// import. Scoped at the `Design` level (not per-sheet) because that is
/// how real KiCad actually resolves them: `SCHEMATIC::GetBusAlias` searches
/// the one project-wide list regardless of which sheet is asking.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BusAlias {
    pub name: String,
    pub members: Vec<String>,
}

/// ERC pin-to-pin conflict matrix: `ERC_SETTINGS::m_PinMap`
/// (`erc_settings.cpp`), serialized the way KiCad's own `pin_map`
/// project-file entry is -- a 12x12 grid of `PIN_ERROR` ints (0 = OK,
/// 1 = warning, 2 = error), row/column in `ELECTRICAL_PINTYPE` order
/// (input, output, bidirectional, tri_state, passive, free, unspecified,
/// power_in, power_out, open_collector, open_emitter, no_connect).
/// `eda_kicad::custom_erc_pin_map` validates the shape and falls back to the
/// default for anything malformed, like `ERC_SETTINGS`'s own loader (a grid
/// that is not `ELECTRICAL_PINTYPES_TOTAL` square is ignored).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ErcPinMap {
    pub matrix: Vec<Vec<u8>>,
}

impl ErcPinMap {
    /// KiCad's default map as the owned grid this struct stores.
    pub fn default_matrix() -> Vec<Vec<u8>> {
        DEFAULT_ERC_PIN_MAP.iter().map(|r| r.to_vec()).collect()
    }
}

const OK: u8 = 0;
const WAR: u8 = 1;
const ERR: u8 = 2;

/// KiCad's default pin-to-pin conflict matrix (`erc_settings.cpp`'s
/// `m_defaultPinMap`), transcribed verbatim: row = first pin's type,
/// column = second pin's type, in `ELECTRICAL_PINTYPE` order. `OK`/`WAR`/
/// `ERR` = no error / warning / error, exactly as KiCad ships it.
#[rustfmt::skip]
pub const DEFAULT_ERC_PIN_MAP: [[u8; 12]; 12] = [
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

/// An accepted ERC finding (`dialog_erc.cpp`'s own "Exclude this
/// violation" / `SCHEMATIC::RecordERCExclusions`): `(check, location)`
/// is the key the studio applies to kicad-cli's ERC report
/// (`eda_kicad_engine::ErcReport::to_json`): a finding with a listed key
/// reports as `excluded`, with no translation. No `id`
/// field -- unlike `Wire`/`NetLabel`/etc., this has nothing geometric to
/// derive one from, and the `(check, location)` pair is already a stable,
/// natural key (unlike those others, there is never more than one
/// exclusion for the same finding to disambiguate between).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ErcExclusion {
    /// kicad-cli's own ERC check name ("pin_not_connected", ...).
    pub check: String,
    /// "REF", "REF.PIN", or whatever else `CheckResult::location` carried
    /// for this finding -- a finding with no location at all can never be
    /// excluded (nothing to key on), same limitation `Exclusions` itself
    /// already has.
    pub location: String,
}

/// A waived DRC violation: `BOARD_DESIGN_SETTINGS::m_DrcExclusions` with its `m_DrcExclusionComments`
/// (`dialog_drc.cpp`'s "Exclude this violation", "Exclude with comment...", Exclude Marker).
///
/// KiCad keys an exclusion by the marker's serialization (`PCB_MARKER::SerializeToString`): the check's settings key, the marker's
/// position and the uuids of the items it names. The derived project writes these as `board.design_settings.drc_exclusions`, so
/// kicad-cli reports the violation as excluded and leaves it out of a report that does not ask for exclusions. Here the identity of a
/// violation is `(check, items)` -- the uuids of the derived board's own items, which `eda_kicad::export_kicad_pcb_mapped` mints
/// deterministically, so the key survives an export and a restart -- and the report is judged against it by the studio too, since
/// kicad-cli's report carries no marker position (see [`positions_nm`](Self::positions_nm)).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DrcExclusion {
    /// The check's KiCad settings key (`clearance`, `unconnected_items`, ...): `RC_ITEM::GetSettingsKey`.
    pub check: String,
    /// The uuids of the items the violation names in the derived board, main item first (`RC_ITEM::GetMainItemID` then
    /// `GetAuxItemID`); never empty.
    pub items: Vec<String>,
    /// Our ids for the same items, in the same order (`R1.2`, a track id, ...): what the studio selects. Empty when unknown.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ids: Vec<String>,
    /// Where kicad-cli's marker for this violation may sit, in nanometres (KiCad's board unit). KiCad matches an exclusion to a marker by
    /// the exact serialization, position included, and kicad-cli's report does not say where a marker is -- only where its items are. So
    /// these are the positions worth trying (the items' own, a track's middle, the middle of the first two items): the derived project
    /// lists one serialization for each, and the one that is the marker's is the one kicad-cli matches (the rest are dropped on load,
    /// as KiCad drops an exclusion no marker matches). Empty: the exclusion is the studio's alone.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub positions_nm: Vec<[i64; 2]>,
    /// `PCB_MARKER::GetComment`: why it was waived. Empty for none.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub comment: String,
}

impl DrcExclusion {
    /// What identifies the violation: the check and the items it names.
    pub fn key(&self) -> DrcExclusionKey {
        DrcExclusionKey { check: self.check.clone(), items: self.items.clone() }
    }

    /// Whether this waives the violation `(check, items)`.
    pub fn matches(&self, check: &str, items: &[String]) -> bool {
        self.check == check && self.items == items
    }
}

/// The identity of a [`DrcExclusion`]: what `Cmd::DeleteDrcExclusions` names.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DrcExclusionKey {
    pub check: String,
    pub items: Vec<String>,
}

/// Title block. Every field optional/empty by default; the exporter falls
/// back to its `ExportMeta` argument for `title`/`date` when this whole
/// section is absent, so existing callers are unaffected.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TitleBlock {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub title: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub date: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub rev: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub company: String,
    /// KiCad's `comment 1`..`comment 9`, in that order (index 0 = comment 1).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub comments: Vec<String>,
    /// The paper this sheet is drawn on ("A4", "A3", ..., KiCad's name; landscape). KiCad's Page Settings dialog edits it together
    /// with the title block, and so does this field. Empty is A4, what every sheet before this field existed was.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub paper: String,
}

/// A child hierarchical sheet, as placed on its parent sheet (GAPS.md #6).
/// `id` is this *placement*'s own stable identity -- a real file's own
/// sheet `(uuid ...)` when imported, or a deterministic id assigned the
/// same way a wire/label gets one when this project creates a sheet fresh
/// -- distinct from `file`, the name of the *content* this placement
/// shows: the same `file` can be placed more than once (KiCad's own
/// `SCH_SCREEN` sharing -- see `Design::sheet_contents`'s own doc), each
/// such placement getting its own `id` and, through it, its own
/// `SchematicSection::instance_overrides` entries for that shared
/// content's symbols. `derive_schematic` never creates one; the reader
/// descends into `file` (`sch_import::import_kicad_sch_tree`) when given a
/// directory to resolve sibling sheet files against, and only records the
/// placement without descending (as before) when given bare text with no
/// filesystem context.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SheetInstance {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub id: String,
    pub name: String,
    pub file: String,
    pub at: Point,
    pub size: (Um, Um),
    /// Sheet pins on this placement's own border (`SCH_SHEET_PIN`), each
    /// tied *by name* (not by any stored link) to a hierarchical label of
    /// the same name in `file`'s own content -- kicad-cli's ERC
    /// (`hier_label_mismatch`) judges that name-only matching rule (shape
    /// is cosmetic, confirmed against
    /// `connection_graph.cpp::ercCheckHierSheets`, which never compares
    /// it).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pins: Vec<SheetPin>,
    /// This placement's page number (`SCH_SHEET::getPageNumber`, `(instances (project .. (path .. (page "2"))))`), set by
    /// Edit Sheet Page Number (`eeschema.EditorControl.editPageNumber`). Empty -- every sheet before this field existed -- means
    /// the sheet's place in the hierarchy (its 1-based virtual page number); Next/Previous Sheet and the plot's page order sort by it.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub page: String,
}

/// One pin on a [`SheetInstance`]'s own border. `shape` is the same
/// 5-value vocabulary a hierarchical label carries (`LabelShape`) --
/// KiCad's own `SCH_SHEET_PIN : public SCH_HIERLABEL` inheritance, which is
/// why the two share a shape type here too.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SheetPin {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub shape: LabelShape,
    /// Sheet coordinates (not sheet-local) -- always on the placement's
    /// own border rectangle, same as a real `SCH_SHEET_PIN` is always
    /// `ConstrainOnEdge`-clamped there.
    pub at: Point,
}

impl SheetPin {
    fn id_seed(&self) -> String {
        format!("{}|{},{}", self.name, self.at.x, self.at.y)
    }
}

/// One placed symbol's Reference/Value/Footprint/unit *as seen through one
/// specific parent sheet placement*, for a screen ([`Design::sheet_contents`]
/// entry) that is placed more than once -- real KiCad's `SCH_SYMBOL_INSTANCE`
/// (`sch_sheet_path.h`), keyed there by a full root-to-leaf sheet-uuid path;
/// keyed here by just `parent_sheet_instance_id` (the *immediate* parent
/// [`SheetInstance::id`] that placed this screen), since that one id is
/// already globally unique across the whole design and nothing deeper is
/// needed to tell two placements of the same file apart. `at` identifies
/// *which* symbol in this screen's own content the override is for (a
/// screen's own drawn content is placement-invariant, so a symbol's own
/// position is already a stable, unique-enough key within it -- the same
/// reasoning `Wire`/`NetLabel` id derivation already leans on elsewhere in
/// this file). Absent entirely for the overwhelming common case (a sheet
/// placed exactly once), where [`SymbolInstance::id`] is already the only
/// answer.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SymbolPathOverride {
    pub at: Point,
    pub parent_sheet_instance_id: String,
    pub reference: String,
    #[serde(default = "d_unit_one")]
    pub unit: u32,
}

impl SchematicSection {
    /// The body style `sym` is drawn in (`SCH_SYMBOL::GetBodyStyle`): 1, the normal one, unless `SchExtras::body_styles` says 2, the alternate ("De Morgan")
    /// one. The style of a symbol whose library symbol has no alternate is whatever is stored; [`crate::symbol::LibSymbol::in_style`] draws the normal body
    /// for it, as `SCH_SYMBOL::GetLibSymbolRef` draws a one-style symbol in any style.
    pub fn body_style_of(&self, sym: &SymbolInstance) -> u32 {
        self.extras.body_styles.get(&field_key(&sym.id, sym.unit)).copied().unwrap_or(1).max(1)
    }

    /// Assign a deterministic id to every wire/label/no-connect whose `id`
    /// is still empty — same contract as `RoutingSection::assign_missing_ids`/
    /// `DrawingsSection::assign_missing_ids` (stable per-kind processing
    /// order, so a tie only ever breaks the same way twice).
    pub fn assign_missing_ids(&mut self) {
        let mut existing: std::collections::BTreeSet<String> = self
            .wires
            .iter()
            .map(|w| &w.id)
            .chain(self.labels.iter().map(|l| &l.id))
            .chain(self.texts.iter().map(|t| &t.id))
            .chain(self.no_connects.iter().map(|nc| &nc.id))
            .chain(self.bus_entries.iter().map(|be| &be.id))
            .chain(self.junctions.iter().map(|j| &j.id))
            .chain(self.lines.iter().map(|l| &l.id))
            .filter(|s| !s.is_empty())
            .cloned()
            .collect();

        let mut order: Vec<usize> = (0..self.wires.len()).collect();
        order.sort_by(|&a, &b| self.wires[a].pts.first().cmp(&self.wires[b].pts.first()));
        for i in order {
            if self.wires[i].id.is_empty() {
                let id = next_item_id("wire", &self.wires[i].id_seed(), &existing);
                existing.insert(id.clone());
                self.wires[i].id = id;
            }
        }

        let mut order: Vec<usize> = (0..self.labels.len()).collect();
        order.sort_by(|&a, &b| (&self.labels[a].net, self.labels[a].at).cmp(&(&self.labels[b].net, self.labels[b].at)));
        for i in order {
            if self.labels[i].id.is_empty() {
                let id = next_item_id("lbl", &self.labels[i].id_seed(), &existing);
                existing.insert(id.clone());
                self.labels[i].id = id;
            }
        }

        let mut order: Vec<usize> = (0..self.texts.len()).collect();
        order.sort_by(|&a, &b| (&self.texts[a].content, self.texts[a].at).cmp(&(&self.texts[b].content, self.texts[b].at)));
        for i in order {
            if self.texts[i].id.is_empty() {
                let id = next_item_id("txt", &self.texts[i].id_seed(), &existing);
                existing.insert(id.clone());
                self.texts[i].id = id;
            }
        }

        let mut order: Vec<usize> = (0..self.no_connects.len()).collect();
        order.sort_by(|&a, &b| self.no_connects[a].at.cmp(&self.no_connects[b].at));
        for i in order {
            if self.no_connects[i].id.is_empty() {
                let id = next_item_id("nc", &self.no_connects[i].id_seed(), &existing);
                existing.insert(id.clone());
                self.no_connects[i].id = id;
            }
        }

        let mut order: Vec<usize> = (0..self.bus_entries.len()).collect();
        order.sort_by(|&a, &b| self.bus_entries[a].at.cmp(&self.bus_entries[b].at));
        for i in order {
            if self.bus_entries[i].id.is_empty() {
                let id = next_item_id("bent", &self.bus_entries[i].id_seed(), &existing);
                existing.insert(id.clone());
                self.bus_entries[i].id = id;
            }
        }

        let mut order: Vec<usize> = (0..self.junctions.len()).collect();
        order.sort_by(|&a, &b| self.junctions[a].at.cmp(&self.junctions[b].at));
        for i in order {
            if self.junctions[i].id.is_empty() {
                let id = next_item_id("jct", &self.junctions[i].id_seed(), &existing);
                existing.insert(id.clone());
                self.junctions[i].id = id;
            }
        }

        let mut order: Vec<usize> = (0..self.lines.len()).collect();
        order.sort_by(|&a, &b| self.lines[a].pts.first().cmp(&self.lines[b].pts.first()));
        for i in order {
            if self.lines[i].id.is_empty() {
                let id = next_item_id("sln", &self.lines[i].id_seed(), &existing);
                existing.insert(id.clone());
                self.lines[i].id = id;
            }
        }

        self.extras.assign_missing_ids(&mut existing);

        // Sheets and their own pins: imported from a real file carries
        // real uuids already (see `sch_import`'s own sheet-parsing loop),
        // so this only ever fires for a sheet this project placed itself
        // (the `S` tool, once it exists) with no id yet.
        existing.extend(self.sheets.iter().map(|s| s.id.clone()).filter(|s| !s.is_empty()));
        existing.extend(self.sheets.iter().flat_map(|s| s.pins.iter()).map(|p| p.id.clone()).filter(|s| !s.is_empty()));
        let mut order: Vec<usize> = (0..self.sheets.len()).collect();
        order.sort_by(|&a, &b| (&self.sheets[a].file, self.sheets[a].at).cmp(&(&self.sheets[b].file, self.sheets[b].at)));
        for i in order {
            if self.sheets[i].id.is_empty() {
                let id = next_item_id("sheet", &format!("{}|{},{}", self.sheets[i].file, self.sheets[i].at.x, self.sheets[i].at.y), &existing);
                existing.insert(id.clone());
                self.sheets[i].id = id;
            }
            let mut pin_order: Vec<usize> = (0..self.sheets[i].pins.len()).collect();
            pin_order.sort_by(|&a, &b| (&self.sheets[i].pins[a].name, self.sheets[i].pins[a].at).cmp(&(&self.sheets[i].pins[b].name, self.sheets[i].pins[b].at)));
            for j in pin_order {
                if self.sheets[i].pins[j].id.is_empty() {
                    let id = next_item_id("shpin", &self.sheets[i].pins[j].id_seed(), &existing);
                    existing.insert(id.clone());
                    self.sheets[i].pins[j].id = id;
                }
            }
        }
    }
}

// ---------- stage 2: placement ----------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlacementSection {
    pub outline: Vec<Point>,
    /// Sorted by `id`.
    pub footprints: Vec<FootprintInstance>,
    /// The floorplan this placement was built under, when one was used.
    /// Carried in the design so the gate can judge the placement against
    /// the plan it was given, and so a reader can see the blocks.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub modules: Vec<ModuleRegion>,
}

/// One floorplan block as recorded in a placement.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModuleRegion {
    pub name: String,
    pub refs: Vec<String>,
    /// (x0, y0, x1, y1) µm.
    pub rect: (Um, Um, Um, Um),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FootprintInstance {
    /// Same stable ID as the schematic symbol ("U1").
    pub id: String,
    pub at: Point,
    pub rot: Millideg,
    pub side: Side,
    /// Which side of the courtyard the refdes label sits on. The placer
    /// chooses it (a label between two rule partners makes their
    /// proximity rule infeasible); every consumer reads it from here.
    #[serde(default, skip_serializing_if = "LabelSide::is_above")]
    pub label: LabelSide,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LabelSide {
    #[default]
    Above,
    Below,
    Left,
    Right,
}

impl LabelSide {
    pub fn is_above(&self) -> bool {
        *self == LabelSide::Above
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Side {
    Top,
    Bottom,
}

// ---------- stage 3: routing ----------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RoutingSection {
    /// Sorted by (net, layer, first point).
    pub tracks: Vec<Track>,
    /// Sorted by (net, at).
    pub vias: Vec<Via>,
    #[serde(default)]
    pub zones: Vec<Zone>,
    /// `BOARD_DESIGN_SETTINGS::m_TrackWidthList` -- extra track widths the
    /// W/Shift+W cycling hotkeys offer, beyond the board's own default
    /// (`BoardRules::track_width`, which a consumer should always treat as
    /// the implicit first entry, same as KiCad's own "use netclass width"
    /// list head). Lives here (the editable `Design` IR), not on the
    /// immutable `ConstraintModel`, so the studio's Board Setup dialog can
    /// actually write it through a `Cmd`. Additive: absent in an older
    /// `design.json` reads as "no custom widths saved yet", same as a
    /// freshly created KiCad board.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub track_width_presets: Vec<Um>,
    /// `BOARD_DESIGN_SETTINGS::m_ViaSizeList` -- same idea as
    /// `track_width_presets`, for the via-size cycling hotkey.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub via_presets: Vec<ViaPreset>,
    /// Task item 4: `TEARDROP_PARAMETERS` -- Board Setup > Teardrops.
    /// Lives here (not `BoardRules`) for the same reason `track_width_presets`
    /// does: the studio needs to *write* it through a `Cmd`, and `BoardRules`
    /// is the read-only intent-derived model. Additive: absent in an older
    /// `design.json` reads as KiCad's own real factory defaults (disabled).
    #[serde(default)]
    pub teardrop_settings: TeardropSettings,
}

/// `TEARDROP_PARAMETERS` + the subset of `TEARDROP_PARAMETERS_LIST` this
/// port models (`pcbnew/teardrop/teardrop_parameters.h`). KiCad keeps three
/// separate `TEARDROP_PARAMETERS` (round/rect/track targets) plus a
/// `TEARDROP_PARAMETERS_LIST`'s own per-target-kind enable flags; this port
/// collapses that into one settings block (round shapes only -- see
/// `eda_connectivity::teardrop`'s own doc for why) with one set of size
/// ratios shared by every target kind, since this model has no per-target-
/// kind size tuning need yet. `m_CurvedEdges`, `m_AllowUseTwoTracks` and
/// `m_TdOnPadsInZones` are not modeled at all (no curved/Bezier edges, no
/// multi-segment-track extension, no in-zone pad filter -- see that
/// module's doc for the full scope).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TeardropSettings {
    /// `TEARDROP_PARAMETERS::m_Enabled` -- collapsed from upstream's three
    /// per-target-kind (round/rect/track) enables into one, since this
    /// port shares one `TEARDROP_PARAMETERS` block across every kind (see
    /// this struct's own doc).
    #[serde(default)]
    pub enabled: bool,
    /// `TEARDROP_PARAMETERS_LIST::m_TargetVias`.
    #[serde(default = "default_td_target")]
    pub target_vias: bool,
    /// `TEARDROP_PARAMETERS_LIST::m_TargetPTHPads`.
    #[serde(default = "default_td_target")]
    pub target_pth_pads: bool,
    /// `TEARDROP_PARAMETERS_LIST::m_TargetSMDPads`. In practice this only
    /// ever matches a *round* SMD pad -- see `eda_connectivity::teardrop`'s
    /// doc on why non-round shapes aren't ported.
    #[serde(default = "default_td_target")]
    pub target_smd_pads: bool,
    /// `TEARDROP_PARAMETERS::m_BestLengthRatio`.
    #[serde(default = "default_td_length_ratio")]
    pub best_length_ratio: f64,
    /// `TEARDROP_PARAMETERS::m_BestWidthRatio`.
    #[serde(default = "default_td_width_ratio")]
    pub best_width_ratio: f64,
    /// `m_TdMaxLen`, µm. <= 0 disables the constraint.
    #[serde(default = "default_td_max_len")]
    pub max_len_um: Um,
    /// `m_TdMaxWidth`, µm. <= 0 disables the constraint.
    #[serde(default = "default_td_max_width")]
    pub max_width_um: Um,
    /// `m_WidthtoSizeFilterRatio`: a track narrower than
    /// `anchor_diameter * this` gets a teardrop; 1.0 = always, 0.0 = never.
    #[serde(default = "default_td_width_to_size_ratio")]
    pub width_to_size_filter_ratio: f64,
}

fn default_td_target() -> bool {
    true // TEARDROP_PARAMETERS_LIST() ctor: every target kind on by default
}
fn default_td_length_ratio() -> f64 {
    0.5
}
fn default_td_width_ratio() -> f64 {
    1.0
}
fn default_td_max_len() -> Um {
    1000 // TEARDROP_PARAMETERS() ctor: pcbIUScale.mmToIU(1.0)
}
fn default_td_max_width() -> Um {
    2000 // ctor: pcbIUScale.mmToIU(2.0)
}
fn default_td_width_to_size_ratio() -> f64 {
    0.9
}

impl Default for TeardropSettings {
    /// `TEARDROP_PARAMETERS()`/`TEARDROP_PARAMETERS_LIST()`'s own ctor
    /// defaults: every target kind enabled, but globally disabled until
    /// the user turns Teardrops on (`m_Enabled` starts `false` upstream
    /// too).
    fn default() -> Self {
        TeardropSettings {
            enabled: false,
            target_vias: default_td_target(),
            target_pth_pads: default_td_target(),
            target_smd_pads: default_td_target(),
            best_length_ratio: default_td_length_ratio(),
            best_width_ratio: default_td_width_ratio(),
            max_len_um: default_td_max_len(),
            max_width_um: default_td_max_width(),
            width_to_size_filter_ratio: default_td_width_to_size_ratio(),
        }
    }
}

/// One entry of `RoutingSection::via_presets`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ViaPreset {
    pub diameter: Um,
    pub drill: Um,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Track {
    /// Stable id (`trk_xxxxxxxxxxxx`). Deterministic from `net`, `layer`
    /// and `pts` -- not `width`, so `SetTrackWidth` can change the width
    /// without the id moving out from under a caller holding it. Empty in
    /// a `design.json` written before ids existed; back-filled the same
    /// deterministic way by [`Design::assign_missing_ids`].
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub id: String,
    pub net: String,
    /// Pad endpoints as "REF.PIN", when the track terminates on pads.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pins: Vec<String>,
    /// Stackup layer name ("F.Cu").
    pub layer: String,
    pub width: Um,
    pub pts: Vec<Point>,
    /// Set when this track is a KiCad arc (`PCB_ARC`, `(arc (start)(mid)
    /// (end))`): its `mid` point, stored as an offset from `pts[0]` so a
    /// plain translation (move, array) keeps it valid. `pts` always holds
    /// the arc's tessellation ([`tessellate_arc`]), so every consumer that
    /// only knows polylines keeps working; consumers that need the true
    /// primitive (DRC, the `.kicad_pcb` exporter) go through
    /// [`Track::arc`], which only reports the arc while `pts` still *is*
    /// its tessellation -- any edit that reshapes the track silently turns
    /// it back into a plain polyline instead of leaving a stale arc behind.
    /// Additive: absent everywhere a track was never an arc.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub arc_mid_offset: Option<Point>,
}

/// Segment count [`tessellate_arc`] uses for a track arc's `pts`.
pub const TRACK_ARC_SEGMENTS: usize = 32;

/// Approximate an arc given as three points on its circumference (start,
/// mid, end -- KiCad's `PCB_ARC`/`SHAPE_ARC` convention: the circle
/// through all three, swept from start *through mid* to end, exactly
/// `SHAPE_ARC::GetCentralAngle`'s choice) as a polyline of `segments`
/// chords. Endpoints are kept bit-exact. Collinear points (no circle)
/// give the straight chord.
pub fn tessellate_arc(start: Point, mid: Point, end: Point, segments: usize) -> Vec<Point> {
    let (sx, sy) = (start.x as f64, start.y as f64);
    let (mx, my) = (mid.x as f64, mid.y as f64);
    let (ex, ey) = (end.x as f64, end.y as f64);

    // Circumcenter of the three points (`CalcArcCenter`).
    let d = 2.0 * (sx * (my - ey) + mx * (ey - sy) + ex * (sy - my));
    if d.abs() < 1e-6 {
        return vec![start, end];
    }
    let ux = ((sx * sx + sy * sy) * (my - ey) + (mx * mx + my * my) * (ey - sy) + (ex * ex + ey * ey) * (sy - my)) / d;
    let uy = ((sx * sx + sy * sy) * (ex - mx) + (mx * mx + my * my) * (sx - ex) + (ex * ex + ey * ey) * (mx - sx)) / d;
    let r = ((sx - ux).powi(2) + (sy - uy).powi(2)).sqrt();

    let ang = |x: f64, y: f64| (y - uy).atan2(x - ux);
    let two_pi = std::f64::consts::TAU;
    let norm = |a: f64| a.rem_euclid(two_pi);
    let (a0, a1, a2) = (ang(sx, sy), ang(mx, my), ang(ex, ey));
    let mut sweep = norm(a2 - a0);
    if norm(a1 - a0) > sweep {
        // The short way around does not pass through mid: sweep the other way.
        sweep -= two_pi;
    }

    let n = segments.max(1);
    let mut pts = Vec::with_capacity(n + 1);
    for i in 0..=n {
        let a = a0 + sweep * (i as f64 / n as f64);
        pts.push(Point { x: (ux + r * a.cos()).round() as i64, y: (uy + r * a.sin()).round() as i64 });
    }
    pts[0] = start;
    *pts.last_mut().expect("n + 1 >= 1") = end;
    pts
}

impl Track {
    /// A KiCad-arc track, from its start, mid and end points.
    pub fn new_arc(net: String, layer: String, width: Um, start: Point, mid: Point, end: Point) -> Track {
        Track {
            id: String::new(),
            net,
            pins: vec![],
            layer,
            width,
            pts: tessellate_arc(start, mid, end, TRACK_ARC_SEGMENTS),
            arc_mid_offset: Some(Point { x: mid.x - start.x, y: mid.y - start.y }),
        }
    }

    /// `(start, mid, end)` when this track is still exactly the arc
    /// `arc_mid_offset` describes (see that field's doc), else `None`.
    pub fn arc(&self) -> Option<(Point, Point, Point)> {
        let off = self.arc_mid_offset?;
        let (start, end) = (*self.pts.first()?, *self.pts.last()?);
        let mid = Point { x: start.x + off.x, y: start.y + off.y };
        let expect = tessellate_arc(start, mid, end, TRACK_ARC_SEGMENTS);
        // 1 µm slack: a translated arc re-tessellates through float
        // round-off that can land a sample one unit over.
        let same = expect.len() == self.pts.len() && expect.iter().zip(&self.pts).all(|(a, b)| (a.x - b.x).abs() <= 1 && (a.y - b.y).abs() <= 1);
        same.then_some((start, mid, end))
    }

    fn id_seed(&self) -> String {
        let pts: Vec<String> = self.pts.iter().map(|p| format!("{},{}", p.x, p.y)).collect();
        format!("{}|{}|{}", self.net, self.layer, pts.join(";"))
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Via {
    /// Stable id (`via_xxxxxxxxxxxx`). Deterministic from every field
    /// *at creation time* (including `at`), so two vias the router or a
    /// person adds in the same spot with the same net never collide.
    /// `MoveVia` afterward changes `at` without touching `id` -- ids are
    /// assigned once, never recomputed.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub id: String,
    pub net: String,
    pub at: Point,
    pub drill: Um,
    pub diameter: Um,
    pub from_layer: String,
    pub to_layer: String,
}

impl Via {
    fn id_seed(&self) -> String {
        format!("{}|{},{}|{}|{}|{}|{}", self.net, self.at.x, self.at.y, self.drill, self.diameter, self.from_layer, self.to_layer)
    }
}

/// `ZONE_CONNECTION` (`pcbnew/zones.h`), minus `INHERITED`: that value only
/// matters with per-netclass connection overrides, which this model doesn't
/// have -- a zone's own `pad_connection` is the only source consulted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum PadConnection {
    None,
    /// `ZONE_SETTINGS::ZONE_SETTINGS()`: `m_padConnection = ZONE_CONNECTION::THERMAL`.
    #[default]
    Thermal,
    Full,
    ThtThermal,
}

/// `ISLAND_REMOVAL_MODE` (`pcbnew/zone_settings.h`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum IslandRemovalMode {
    /// `m_removeIslands = ISLAND_REMOVAL_MODE::ALWAYS`.
    #[default]
    Always,
    Never,
    Area,
}

/// `ZONE_FILL_MODE` (`pcbnew/zone_settings.h`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum FillMode {
    #[default]
    Polygons,
    HatchPattern,
}

// `ZONE_SETTINGS::ZONE_SETTINGS()`'s hardcoded defaults (`pcbnew/zone_settings.cpp`),
// converted from KiCad's mm literals to this model's Um (micrometers).
fn default_zone_clearance() -> Um {
    500 // ZONE_CLEARANCE_MM = 0.5
}
fn default_zone_min_thickness() -> Um {
    250 // ZONE_THICKNESS_MM = 0.25
}
fn default_thermal_gap() -> Um {
    500 // ZONE_THERMAL_RELIEF_GAP_MM = 0.5
}
fn default_thermal_spoke_width() -> Um {
    500 // ZONE_THERMAL_RELIEF_COPPER_WIDTH_MM = 0.5
}
fn default_min_island_area() -> i64 {
    10_000_000 // 10 * IU_PER_MM^2, i.e. 10 mm^2, in um^2
}
fn default_hatch_thickness() -> Um {
    1000 // max(min_thickness(250)*4, 1mm(1000))
}
fn default_hatch_gap() -> Um {
    1500 // max(min_thickness(250)*6, 1.5mm(1500))
}
fn default_hatch_smoothing_value() -> f64 {
    0.1
}
fn default_hatch_hole_min_area() -> f64 {
    0.15
}
fn default_hatch_border_algorithm() -> i32 {
    1
}

/// `ZONE_BORDER_DISPLAY_STYLE` as the file stores it: `(hatch none|edge|full <pitch>)`
/// (`pcb_io_kicad_sexpr.cpp` `format( const ZONE* )`). How the zone's outline is
/// drawn on screen; it never changes the copper. `Edge` is KiCad's own default for a
/// new zone (and what every zone this exporter wrote before this field existed said).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ZoneBorderStyle {
    None,
    #[default]
    Edge,
    Full,
}

impl ZoneBorderStyle {
    pub fn is_default(&self) -> bool {
        *self == ZoneBorderStyle::Edge
    }
}

/// `ZONE_SETTINGS::SMOOTHING_*` (`(fill (smoothing chamfer|fillet) (radius r))`): how the
/// zone outline's corners are rounded or cut before the fill is made. The studio's own
/// filler does not smooth yet (`docs/parity/GAPS.md` item 13); the setting is carried so
/// kicad-cli, which does, gets the zone as the user drew it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ZoneSmoothing {
    #[default]
    None,
    Chamfer,
    Fillet,
}

impl ZoneSmoothing {
    pub fn is_none(&self) -> bool {
        *self == ZoneSmoothing::None
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Zone {
    /// Stable id (`zone_xxxxxxxxxxxx`). Deterministic from `net`, `layer`
    /// and `outline`.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub id: String,
    pub net: String,
    pub layer: String,
    pub outline: Vec<Point>,

    // ---- fill settings (ZONE_SETTINGS), added additively with KiCad's own
    // defaults so existing design.json files without these fields keep
    // behaving exactly as before. Fills are always *derived* from these
    // (plus the outline/net/layer above) -- never stored as a second master.
    /// `m_ZoneClearance`.
    #[serde(default = "default_zone_clearance")]
    pub clearance: Um,
    /// `m_ZoneMinThickness`.
    #[serde(default = "default_zone_min_thickness")]
    pub min_thickness: Um,
    /// `m_ThermalReliefGap`.
    #[serde(default = "default_thermal_gap")]
    pub thermal_gap: Um,
    /// `m_ThermalReliefSpokeWidth`.
    #[serde(default = "default_thermal_spoke_width")]
    pub thermal_spoke_width: Um,
    /// `ZONE_SETTINGS::GetPadConnection()`.
    #[serde(default)]
    pub pad_connection: PadConnection,
    /// `m_ZonePriority`.
    #[serde(default)]
    pub priority: u32,
    /// `ZONE_SETTINGS::GetIslandRemovalMode()`.
    #[serde(default)]
    pub island_removal_mode: IslandRemovalMode,
    /// `ZONE_SETTINGS::GetMinIslandArea()`, in um^2.
    #[serde(default = "default_min_island_area")]
    pub min_island_area: i64,
    /// `m_FillMode`.
    #[serde(default)]
    pub fill_mode: FillMode,
    /// `m_HatchThickness` (only meaningful when `fill_mode == HatchPattern`).
    #[serde(default = "default_hatch_thickness")]
    pub hatch_thickness: Um,
    /// `m_HatchGap`.
    #[serde(default = "default_hatch_gap")]
    pub hatch_gap: Um,
    /// `m_HatchOrientation`, in millidegrees.
    #[serde(default)]
    pub hatch_orientation_mdeg: Millideg,
    /// `m_HatchSmoothingLevel`.
    #[serde(default)]
    pub hatch_smoothing_level: i32,
    /// `m_HatchSmoothingValue`.
    #[serde(default = "default_hatch_smoothing_value")]
    pub hatch_smoothing_value: f64,
    /// `m_HatchHoleMinArea`.
    #[serde(default = "default_hatch_hole_min_area")]
    pub hatch_hole_min_area: f64,
    /// `m_HatchBorderAlgorithm`.
    #[serde(default = "default_hatch_border_algorithm")]
    pub hatch_border_algorithm: i32,

    // ---- rule area / keepout (`ZONE::GetIsRuleArea`, task item 3). A
    // zone's `net`/outline/layer are shared with a copper-pour zone (same
    // dialog, same "draw a filled zone" tool -- see `tools/drawing_tool.cpp`
    // and `dialog_copper_zones.cpp`'s own single dialog that swaps panels on
    // `IsRuleArea()`); only these six fields distinguish a rule area, and
    // every `ZONE_SETTINGS` fill field above is simply unused for one
    // (matching source, which still stores them unread rather than making
    // them a separate type). Additive: absent in an older `design.json`
    // reads as "an ordinary copper-pour zone", exactly as before this field
    // existed.
    /// `ZONE::GetIsRuleArea()`.
    #[serde(default)]
    pub is_rule_area: bool,
    /// `GetDoNotAllowTracks()`.
    #[serde(default)]
    pub keepout_tracks: bool,
    /// `GetDoNotAllowVias()`.
    #[serde(default)]
    pub keepout_vias: bool,
    /// `GetDoNotAllowPads()`.
    #[serde(default)]
    pub keepout_pads: bool,
    /// `GetDoNotAllowZoneFills()` -- disallow copper pours (zone fills)
    /// under this area, not "fill this zone with copper" (that is
    /// `fill_mode`/`FillMode`, meaningless for a rule area anyway).
    #[serde(default)]
    pub keepout_copper_pour: bool,
    /// `GetDoNotAllowFootprints()`.
    #[serde(default)]
    pub keepout_footprints: bool,

    /// Task item 4: true for a teardrop this app generated
    /// (`eda_connectivity::teardrop::generate_teardrops`), false for an
    /// ordinary user-drawn zone. KiCad stores a teardrop as a real `ZONE`
    /// too (`TEARDROP_MANAGER::createTeardrop`) -- this is the one bit this
    /// port needs beyond that to find and replace its own generated set on
    /// demand (`Cmd::AddAllTeardrops`/`RemoveAllTeardrops`) without
    /// disturbing a zone the user drew by hand. A teardrop's `outline` is
    /// its own final shape already (an exact-tangent pentagon against its
    /// round anchor) -- it is rendered and exported as solid copper
    /// directly, never run through the knockout/thermal-relief fill
    /// pipeline the way a drawn zone's settings fields describe.
    #[serde(default)]
    pub teardrop: bool,

    /// The footprint this zone belongs to (`ZONE::GetParentFootprint()`),
    /// for a zone imported from inside a `(footprint ...)`; `None` for a
    /// board-level zone. A footprint's own rule area never tests the
    /// footprint itself (`intersectsArea`'s `aArea->GetParent() == item`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_footprint: Option<String>,

    /// `ZONE::GetZoneName()`: the label the Zone Manager shows (`(name "..")`).
    /// Empty = unnamed, KiCad's own default.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name: String,
    /// `ZONE::GetHatchStyle()`: `(hatch none|edge|full ..)`. Display only.
    #[serde(default, skip_serializing_if = "ZoneBorderStyle::is_default")]
    pub border_style: ZoneBorderStyle,
    /// `ZONE::GetCornerSmoothingType()`.
    #[serde(default, skip_serializing_if = "ZoneSmoothing::is_none")]
    pub smoothing: ZoneSmoothing,
    /// `ZONE::GetCornerRadius()`, µm; only meaningful with `smoothing`.
    #[serde(default, skip_serializing_if = "is_zero_um")]
    pub corner_radius: Um,
}

impl Zone {
    fn id_seed(&self) -> String {
        let pts: Vec<String> = self.outline.iter().map(|p| format!("{},{}", p.x, p.y)).collect();
        format!("{}|{}|{}", self.net, self.layer, pts.join(";"))
    }
}

impl Default for Zone {
    /// KiCad's own `ZONE_SETTINGS` defaults for every setting field (see
    /// each field's `default_*` function above); `id`/`net`/`layer`/`outline`
    /// are empty, matching a zone that's about to be filled in.
    fn default() -> Self {
        Zone {
            id: String::new(),
            net: String::new(),
            layer: String::new(),
            outline: Vec::new(),
            clearance: default_zone_clearance(),
            min_thickness: default_zone_min_thickness(),
            thermal_gap: default_thermal_gap(),
            thermal_spoke_width: default_thermal_spoke_width(),
            pad_connection: PadConnection::default(),
            priority: 0,
            island_removal_mode: IslandRemovalMode::default(),
            min_island_area: default_min_island_area(),
            fill_mode: FillMode::default(),
            hatch_thickness: default_hatch_thickness(),
            hatch_gap: default_hatch_gap(),
            hatch_orientation_mdeg: 0,
            hatch_smoothing_level: 0,
            hatch_smoothing_value: default_hatch_smoothing_value(),
            hatch_hole_min_area: default_hatch_hole_min_area(),
            hatch_border_algorithm: default_hatch_border_algorithm(),
            is_rule_area: false,
            keepout_tracks: false,
            keepout_vias: false,
            keepout_pads: false,
            keepout_copper_pour: false,
            keepout_footprints: false,
            teardrop: false,
            parent_footprint: None,
            name: String::new(),
            border_style: ZoneBorderStyle::default(),
            smoothing: ZoneSmoothing::default(),
            corner_radius: 0,
        }
    }
}

impl RoutingSection {
    /// Assign a deterministic id to every track/via/zone whose `id` is
    /// still empty (see the field docs on [`Track`]/[`Via`]/[`Zone`]).
    /// Processes each kind in its own canonical sort order so that, given
    /// the same routing, several items needing a fresh id in one call
    /// always get them in the same order -- which only matters when two
    /// items hash the same (an exact duplicate) and have to be told apart
    /// by a `_2`, `_3`, ... suffix.
    pub fn assign_missing_ids(&mut self) {
        let mut existing: std::collections::BTreeSet<String> =
            self.tracks.iter().map(|t| &t.id).chain(self.vias.iter().map(|v| &v.id)).chain(self.zones.iter().map(|z| &z.id)).filter(|s| !s.is_empty()).cloned().collect();

        let mut order: Vec<usize> = (0..self.tracks.len()).collect();
        order.sort_by(|&a, &b| {
            let (ta, tb) = (&self.tracks[a], &self.tracks[b]);
            (&ta.net, &ta.layer, ta.pts.first()).cmp(&(&tb.net, &tb.layer, tb.pts.first()))
        });
        for i in order {
            if self.tracks[i].id.is_empty() {
                let id = next_item_id("trk", &self.tracks[i].id_seed(), &existing);
                existing.insert(id.clone());
                self.tracks[i].id = id;
            }
        }

        let mut order: Vec<usize> = (0..self.vias.len()).collect();
        order.sort_by(|&a, &b| (&self.vias[a].net, self.vias[a].at).cmp(&(&self.vias[b].net, self.vias[b].at)));
        for i in order {
            if self.vias[i].id.is_empty() {
                let id = next_item_id("via", &self.vias[i].id_seed(), &existing);
                existing.insert(id.clone());
                self.vias[i].id = id;
            }
        }

        let mut order: Vec<usize> = (0..self.zones.len()).collect();
        order.sort_by(|&a, &b| (&self.zones[a].net, &self.zones[a].layer).cmp(&(&self.zones[b].net, &self.zones[b].layer)));
        for i in order {
            if self.zones[i].id.is_empty() {
                let id = next_item_id("zone", &self.zones[i].id_seed(), &existing);
                existing.insert(id.clone());
                self.zones[i].id = id;
            }
        }
    }
}

// ---------- free-standing board graphics & text ----------

/// A KiCad-style graphic primitive drawn directly on the board (its
/// `PCB_SHAPE`): silkscreen art, fab-layer outlines, courtyard-adjacent
/// decoration -- anything that is not copper and not a footprint. Modelled
/// as one enum (tagged `kind`, like [`crate::PlacementRule`]) rather than a
/// struct wrapping a geometry enum, so every field a caller needs is at the
/// top level of the JSON object with no nested "geometry" key to unwrap.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Shape {
    Segment {
        #[serde(default, skip_serializing_if = "String::is_empty")]
        id: String,
        /// KiCad layer name ("F.SilkS", "Edge.Cuts", "F.Cu", ...).
        layer: String,
        stroke_width: Um,
        /// Meaningless for a segment (KiCad never fills a line); carried
        /// for schema uniformity across every `Shape` variant and ignored
        /// by the exporter for this one, same as `Arc`.
        #[serde(default)]
        filled: bool,
        start: Point,
        end: Point,
    },
    Arc {
        #[serde(default, skip_serializing_if = "String::is_empty")]
        id: String,
        layer: String,
        stroke_width: Um,
        #[serde(default)]
        filled: bool,
        start: Point,
        mid: Point,
        end: Point,
    },
    Rect {
        #[serde(default, skip_serializing_if = "String::is_empty")]
        id: String,
        layer: String,
        stroke_width: Um,
        #[serde(default)]
        filled: bool,
        start: Point,
        end: Point,
    },
    Circle {
        #[serde(default, skip_serializing_if = "String::is_empty")]
        id: String,
        layer: String,
        stroke_width: Um,
        #[serde(default)]
        filled: bool,
        /// A point on the circumference, KiCad's own way of storing the
        /// radius (`(circle (center ..) (end ..))`) -- so the file's
        /// radius survives byte-for-byte instead of round-tripping through
        /// a computed float.
        center: Point,
        end: Point,
    },
    Polygon {
        #[serde(default, skip_serializing_if = "String::is_empty")]
        id: String,
        layer: String,
        stroke_width: Um,
        #[serde(default)]
        filled: bool,
        pts: Vec<Point>,
    },
    /// `PCB_SHAPE` of type `BEZIER` (`gr_curve`): a cubic Bezier curve from
    /// `start` to `end` with control points `c1` (near the start) and `c2`
    /// (near the end), exactly KiCad's `(gr_curve (pts (xy start) (xy c1)
    /// (xy c2) (xy end)))` order. Everything downstream that needs a polyline
    /// (DRC, plot, board edge, export to a plain-geometry consumer) uses
    /// [`Shape::bezier_points`] -- `BEZIER_POLY::GetPoly` at the board's
    /// `m_MaxError`, the same flattening KiCad itself does
    /// (`RebuildBezierToSegmentsPointsList`). Added after the other five
    /// kinds existed, so nothing already on disk carries it.
    Bezier {
        #[serde(default, skip_serializing_if = "String::is_empty")]
        id: String,
        layer: String,
        stroke_width: Um,
        /// Meaningless for an open curve (KiCad never fills one); carried for schema uniformity, same as `Segment`/`Arc`.
        #[serde(default)]
        filled: bool,
        start: Point,
        c1: Point,
        c2: Point,
        end: Point,
    },
}

impl Shape {
    pub fn id(&self) -> &str {
        match self {
            Shape::Segment { id, .. } | Shape::Arc { id, .. } | Shape::Rect { id, .. } | Shape::Circle { id, .. } | Shape::Polygon { id, .. } | Shape::Bezier { id, .. } => id,
        }
    }
    pub fn set_id(&mut self, new_id: String) {
        match self {
            Shape::Segment { id, .. } | Shape::Arc { id, .. } | Shape::Rect { id, .. } | Shape::Circle { id, .. } | Shape::Polygon { id, .. } | Shape::Bezier { id, .. } => *id = new_id,
        }
    }
    pub fn layer(&self) -> &str {
        match self {
            Shape::Segment { layer, .. } | Shape::Arc { layer, .. } | Shape::Rect { layer, .. } | Shape::Circle { layer, .. } | Shape::Polygon { layer, .. } | Shape::Bezier { layer, .. } => layer,
        }
    }
    /// `PCB_SHAPE::SetLayer` -- part of `dialog_pcb_shape_properties`'s own
    /// editable field set (GAPS.md #11), via `eda_ops::Cmd::EditShape`.
    pub fn set_layer(&mut self, new_layer: String) {
        match self {
            Shape::Segment { layer, .. } | Shape::Arc { layer, .. } | Shape::Rect { layer, .. } | Shape::Circle { layer, .. } | Shape::Polygon { layer, .. } | Shape::Bezier { layer, .. } => *layer = new_layer,
        }
    }
    /// `PCB_SHAPE::SetWidth` (`STROKE_PARAMS`'s width -- see `set_layer`'s doc).
    pub fn set_stroke_width(&mut self, width: Um) {
        match self {
            Shape::Segment { stroke_width, .. } | Shape::Arc { stroke_width, .. } | Shape::Rect { stroke_width, .. } | Shape::Circle { stroke_width, .. } | Shape::Polygon { stroke_width, .. } | Shape::Bezier { stroke_width, .. } => {
                *stroke_width = width
            }
        }
    }
    pub fn stroke_width(&self) -> Um {
        match self {
            Shape::Segment { stroke_width, .. } | Shape::Arc { stroke_width, .. } | Shape::Rect { stroke_width, .. } | Shape::Circle { stroke_width, .. } | Shape::Polygon { stroke_width, .. } | Shape::Bezier { stroke_width, .. } => *stroke_width,
        }
    }
    pub fn is_filled(&self) -> bool {
        match self {
            Shape::Segment { filled, .. } | Shape::Arc { filled, .. } | Shape::Rect { filled, .. } | Shape::Circle { filled, .. } | Shape::Polygon { filled, .. } | Shape::Bezier { filled, .. } => *filled,
        }
    }
    /// `PCB_SHAPE::SetFilled` (see `set_layer`'s doc).
    pub fn set_filled(&mut self, filled_value: bool) {
        match self {
            Shape::Segment { filled, .. } | Shape::Arc { filled, .. } | Shape::Rect { filled, .. } | Shape::Circle { filled, .. } | Shape::Polygon { filled, .. } | Shape::Bezier { filled, .. } => *filled = filled_value,
        }
    }
    /// Every point the geometry is made of, in a stable order -- used both
    /// to seed the id hash and, in `eda-ops`, to validate the shape has
    /// enough of them (a polygon needs 3+).
    pub fn points(&self) -> Vec<Point> {
        match self {
            Shape::Segment { start, end, .. } => vec![*start, *end],
            Shape::Arc { start, mid, end, .. } => vec![*start, *mid, *end],
            Shape::Rect { start, end, .. } => vec![*start, *end],
            Shape::Circle { center, end, .. } => vec![*center, *end],
            Shape::Polygon { pts, .. } => pts.clone(),
            Shape::Bezier { start, c1, c2, end, .. } => vec![*start, *c1, *c2, *end],
        }
    }
    /// A `Bezier`'s flattened polyline (`EDA_SHAPE::RebuildBezierToSegmentsPointsList`:
    /// `BEZIER_POLY::GetPoly` at the board's `m_MaxError`, [`crate::bezier::BEZIER_MAX_ERROR_UM`]);
    /// `None` for every other kind.
    pub fn bezier_points(&self) -> Option<Vec<Point>> {
        match self {
            Shape::Bezier { start, c1, c2, end, .. } => Some(crate::bezier::bezier_polyline(*start, *c1, *c2, *end, crate::bezier::BEZIER_MAX_ERROR_UM)),
            _ => None,
        }
    }
    /// Shift every point of the geometry by `(dx, dy)` -- what dragging a
    /// hand-drawn shape does; there is no single "position" field to set
    /// the way there is for a via or a text, since a shape's geometry is
    /// two or more points.
    pub fn translate(&mut self, dx: Um, dy: Um) {
        let shift = |p: &mut Point| {
            p.x += dx;
            p.y += dy;
        };
        match self {
            Shape::Segment { start, end, .. } | Shape::Rect { start, end, .. } => {
                shift(start);
                shift(end);
            }
            Shape::Arc { start, mid, end, .. } => {
                shift(start);
                shift(mid);
                shift(end);
            }
            Shape::Circle { center, end, .. } => {
                shift(center);
                shift(end);
            }
            Shape::Polygon { pts, .. } => pts.iter_mut().for_each(shift),
            Shape::Bezier { start, c1, c2, end, .. } => {
                shift(start);
                shift(c1);
                shift(c2);
                shift(end);
            }
        }
    }
    fn id_seed(&self) -> String {
        let kind = match self {
            Shape::Segment { .. } => "segment",
            Shape::Arc { .. } => "arc",
            Shape::Rect { .. } => "rect",
            Shape::Circle { .. } => "circle",
            Shape::Polygon { .. } => "polygon",
            Shape::Bezier { .. } => "bezier",
        };
        let pts: Vec<String> = self.points().iter().map(|p| format!("{},{}", p.x, p.y)).collect();
        format!("{kind}|{}|{}", self.layer(), pts.join(";"))
    }
}

/// Horizontal text justification, KiCad's `justify left|right` (absent =
/// centred).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TextJustify {
    Left,
    #[default]
    Center,
    Right,
}

/// Vertical text justification, KiCad's `justify top|bottom` (absent = centred).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TextVAlign {
    Top,
    #[default]
    Center,
    Bottom,
}

/// Where one field of a symbol, a power symbol or a sheet is drawn: KiCad's `SCH_FIELD` position, angle, justification and
/// visibility (`eeschema/sch_field.cpp`), kept per instance because the library only knows where a field goes on an unplaced
/// symbol, and Autoplace Fields moves them.
///
/// The placement is in the frame of the *unturned, unmirrored* item, measured from its origin (a symbol's box corner, a power symbol's
/// pin, a sheet's top-left corner), the way a pin is: a move, a turn or a mirror carries the field along with no edit of its own.
/// `SymbolInstance::rot`/`mirrored` take it to the sheet, as KiCad's `TRANSFORM` does for `SCH_FIELD::GetPosition`
/// (`eda_engine::fields::page_field`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FieldPlacement {
    /// `Reference`, `Value`, `Footprint`, `Datasheet`; a sheet's `Sheetname`, `Sheetfile`; a user field's own name.
    pub name: String,
    /// The text's anchor, micrometres from the item's origin.
    pub dx: Um,
    pub dy: Um,
    /// The text's own angle before the item turns it: 0 (horizontal) or 90_000 (vertical).
    #[serde(default, skip_serializing_if = "is_zero_angle")]
    pub angle: Millideg,
    #[serde(default, skip_serializing_if = "is_default")]
    pub h: TextJustify,
    #[serde(default, skip_serializing_if = "is_default")]
    pub v: TextVAlign,
    /// Drawn on the sheet (a hidden field keeps its place for when it is shown).
    #[serde(default = "d_true_field", skip_serializing_if = "is_true_field")]
    pub visible: bool,
    /// The text's size (its height and its width), micrometres: `EDA_TEXT::GetTextWidth`. 0 is KiCad's default for a field, 50 mil
    /// (`DEFAULT_SIZE_TEXT`, 1.27 mm), which is what every field has until Field Properties sets another.
    #[serde(default, skip_serializing_if = "is_zero_um")]
    pub size_um: Um,
    /// `EDA_TEXT::IsBold` / `IsItalic`: the pen is a fifth of the size instead of an eighth, the strokes lean.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub bold: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub italic: bool,
    /// `SCH_FIELD::IsNameShown` (`(show_name yes)`): the text is drawn as `Name: value`.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub name_shown: bool,
    /// `!SCH_FIELD::CanAutoplace` (`(do_not_autoplace yes)`): Autoplace Fields leaves this field where it is.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub no_autoplace: bool,
}

impl FieldPlacement {
    /// A visible field named `name` at its owner's origin, horizontal, centred, in the default size.
    pub fn at_origin(name: &str) -> FieldPlacement {
        FieldPlacement { name: name.to_string(), dx: 0, dy: 0, angle: 0, h: TextJustify::Center, v: TextVAlign::Center, visible: true, size_um: 0, bold: false, italic: false, name_shown: false, no_autoplace: false }
    }

    /// The size the text is set in, micrometres (the default when none was set).
    pub fn text_size_um(&self) -> Um {
        if self.size_um > 0 {
            self.size_um
        } else {
            DEFAULT_FIELD_SIZE_UM
        }
    }
}

/// `DEFAULT_SIZE_TEXT` (50 mils): the size of a field, a label and a text until another is set.
pub const DEFAULT_FIELD_SIZE_UM: Um = 1_270;

fn d_true_field() -> bool {
    true
}
fn is_true_field(b: &bool) -> bool {
    *b
}
fn is_zero_angle(a: &Millideg) -> bool {
    *a == 0
}
fn is_default<T: Default + PartialEq>(v: &T) -> bool {
    *v == T::default()
}

/// The key of a placed symbol's fields in [`SchematicSection::field_layout`]: its reference, with the unit after a `#` for any
/// unit but the first (each placed unit draws its own).
pub fn field_key(id: &str, unit: u32) -> String {
    if unit <= 1 {
        id.to_string()
    } else {
        format!("{id}#{unit}")
    }
}

/// Free-standing board text (KiCad's `PCB_TEXT` / `gr_text`): silkscreen
/// labels, fab notes -- anything that is not a footprint's own reference or
/// value field (those stay on `FootprintInstance`/`Part`, exactly as
/// today).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Text {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub id: String,
    pub content: String,
    pub at: Point,
    #[serde(default)]
    pub angle: Millideg,
    /// KiCad layer name ("F.SilkS", "F.Fab", ...).
    pub layer: String,
    /// Font size, µm, applied equally to height and width (every text this
    /// model produces is drawn at a square aspect, like the schematic
    /// exporter's ref/value labels).
    pub size_um: Um,
    pub stroke_width: Um,
    #[serde(default)]
    pub justify: TextJustify,
    /// Read from the back of the board, KiCad's `justify mirror`.
    #[serde(default)]
    pub mirror: bool,
}

impl Text {
    fn id_seed(&self) -> String {
        format!("{}|{},{}|{}|{}", self.content, self.at.x, self.at.y, self.angle, self.layer)
    }
}

/// Task item 5: `common/tool/group_tool.cpp` / `pcbnew/pcb_group.cpp`'s
/// `PCB_GROUP` -- a named set of member item ids, no geometry of its own
/// (its on-screen box is always derived from its members). Lives on
/// `DrawingsSection` rather than a new top-level `Design` field purely
/// for the lowest construction-site ripple (`DrawingsSection` already
/// derives `Default` and every existing literal already spreads it); a
/// group can reference a footprint/track/via/zone/shape/text id, not
/// just a drawing, so this is a storage-convenience choice, not a claim
/// that a group *is* a drawing.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Group {
    /// Stable id (`grp_xxxxxxxxxxxx`). Deterministic from the member id
    /// set (sorted, so member order never matters) -- see `Track`/`Zone`'s
    /// own docs for why every id here is content-derived, never random.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub id: String,
    /// `PCB_GROUP::GetName()`. Empty = unnamed (KiCad's own default).
    #[serde(default)]
    pub name: String,
    /// Ids of every direct member -- a part reference, a track/via/zone/
    /// shape/text/dimension id, or another group's id (`EDA_GROUP::m_items`
    /// holds any item, so groups nest). An id is a member of one group at
    /// most and the nesting never loops; `DrawingsSection::group_leaves`
    /// and friends (`groups.rs`) read the tree.
    pub member_ids: Vec<String>,
}

impl Group {
    /// What the id of a group is derived from: its members, sorted (`next_item_id( "grp", .. )`). Public for the importer, which
    /// names inner groups before the groups that hold them.
    pub fn id_seed(&self) -> String {
        let mut members = self.member_ids.clone();
        members.sort();
        members.join(",")
    }
}

/// Task item 7: `pcbnew/pcb_dimension.h`'s `PCB_DIMENSION_BASE` hierarchy
/// (`PCB_DIM_ALIGNED`/`PCB_DIM_ORTHOGONAL`/`PCB_DIM_RADIAL`/`PCB_DIM_LEADER`/
/// `PCB_DIM_CENTER`), collapsed into one struct with a kind-specific tag
/// instead of five item types, matching this model's existing `Shape`
/// enum's own shape (one struct per real KiCad class would ripple the
/// same way `Shape` avoids). `start`/`end` are the two "feature points"
/// every kind has (source's own term) -- what they mean depends on
/// `kind`, documented on each variant.
///
/// The measured value and the dimension's own displayed text are never
/// stored: both are recomputed fresh from `start`/`end` plus these
/// formatting fields, the same way source's own `Update()`/`GetValueText()`
/// do (`override_text`, when set, replaces the computed number but still
/// goes through prefix/suffix/units-suffix formatting same as source's
/// `m_overrideTextEnabled`). `crates/connectivity::dimension` computes
/// the geometry (crossbar/extension lines/arrows/leader/text position)
/// and formatted text fresh from this struct on every read.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Dimension {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub id: String,
    pub layer: String,
    pub kind: DimensionKind,
    /// The first feature point. An `Aligned`/`Orthogonal` dimension's own
    /// first measured point; a `Radial`/`Center`'s circle/arc centre; a
    /// `Leader`'s arrow tip.
    pub start: Point,
    /// The second feature point. An `Aligned`/`Orthogonal`'s second
    /// measured point; a `Radial`'s point on the circle/arc (defines the
    /// radius); a `Leader`'s knee (where the line bends toward the text);
    /// a `Center`'s own arm endpoint (defines the cross's size and angle).
    pub end: Point,
    #[serde(default)]
    pub prefix: String,
    #[serde(default)]
    pub suffix: String,
    /// `PCB_DIMENSION_BASE::m_overrideTextEnabled`/`m_valueString`
    /// collapsed into one option, rather than a bool plus a string that's
    /// only meaningful when the bool is set.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub override_text: Option<String>,
    pub units: DimensionUnits,
    pub units_format: DimensionUnitsFormat,
    /// Decimal places, 0-5. A flat count -- source's own `DIM_PRECISION`
    /// also has four unit-dependent "V_VVV"-style levels (fewer decimals
    /// for mm than inch at the "same" precision); not ported, since a
    /// fixed decimal count covers the overwhelming majority of real
    /// dimensioned drawings and keeps one precision concept instead of two.
    pub precision: u8,
    pub suppress_trailing_zeros: bool,
    pub text_position: DimensionTextPosition,
    pub keep_text_aligned: bool,
    /// Only meaningful when `keep_text_aligned` is false -- otherwise the
    /// geometry pass overwrites this every time, same as source recomputing
    /// `GetTextAngle()` from the crossbar/knee angle on every `Update()`.
    #[serde(default)]
    pub text_angle: Millideg,
    pub text_size_um: Um,
    pub stroke_width: Um,
    pub arrow_length: Um,
    pub extension_offset: Um,
    pub extension_height: Um,
    pub arrow_direction: ArrowDirection,
    /// `EDA_TEXT::GetTextThickness()` of the dimension's text, µm: the pen the label is
    /// drawn with, which the text-dimension DRC checks read. `None` = 15 % of
    /// `text_size_um`, KiCad's own pen for its default 1 mm label. Additive: absent in an
    /// older `design.json`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text_thickness_um: Option<Um>,
}

impl Dimension {
    fn id_seed(&self) -> String {
        format!("{:?}|{},{}|{},{}", self.kind, self.start.x, self.start.y, self.end.x, self.end.y)
    }
}

/// `PCB_DIMENSION_BASE`'s five concrete subclasses, as a tag instead of
/// five item types -- see [`Dimension`]'s own doc.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DimensionKind {
    /// `PCB_DIM_ALIGNED`: a crossbar parallel to `end - start`, offset
    /// perpendicular by `height` (signed -- which side of the feature
    /// line the crossbar falls on).
    Aligned { height: Um },
    /// `PCB_DIM_ORTHOGONAL`: like `Aligned`, but the crossbar is locked
    /// horizontal or vertical and only that axis of `end - start` is
    /// measured, with a second, independent extension line compensating
    /// for `end` not actually lying on the (axis-locked) crossbar.
    Orthogonal { height: Um, horizontal: bool },
    /// `PCB_DIM_RADIAL`: `start` is the circle/arc centre (also where a
    /// small fixed-size `+` mark is drawn, `PCB_DIM_RADIAL::updateGeometry`'s
    /// own `centerArm` cross), `end` a point on it; a leader runs outward
    /// from `end` by `leader_length` to a knee, then on to the text.
    Radial { leader_length: Um },
    /// `PCB_DIM_LEADER`: a line from `start` (arrow tip) to `end` (knee),
    /// then on to the text. No text-border styling (rect/circle around
    /// the text) -- see PARITY-pcb.md section 18.
    Leader,
    /// `PCB_DIM_CENTER`: a `+` mark at `start`, sized and oriented by
    /// `end - start` (one arm along that vector, the other rotated 90°).
    /// Never shows text in practice (nothing stops it, same as source).
    Center,
}

/// `DIM_UNITS_MODE` (`AUTOMATIC` follows the app's own display unit,
/// `state.units` on the frontend -- same split `EDA_UNITS`/`DIM_UNITS_MODE`
/// have in source, where a dimension can pin its own units independent of
/// the frame's).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DimensionUnits {
    Mm,
    Mil,
    Inch,
    Automatic,
}

/// `DIM_UNITS_FORMAT`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DimensionUnitsFormat {
    NoSuffix,
    BareSuffix,
    ParenSuffix,
}

/// `DIM_TEXT_POSITION`. `MANUAL` is not ported -- no point-editor-style
/// manual text drag for a sub-element, same restriction this model's
/// zones/shapes already have on their own control points.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DimensionTextPosition {
    Outside,
    Inline,
}

/// `DIM_ARROW_DIRECTION`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArrowDirection {
    Inward,
    Outward,
}

/// `BOARD_DESIGN_SETTINGS`'s `m_Dimension*` fields -- see [`Dimension`]'s
/// own doc on why these are only applied once, at creation.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DimensionSettings {
    pub units: DimensionUnits,
    pub units_format: DimensionUnitsFormat,
    pub precision: u8,
    pub suppress_trailing_zeros: bool,
    pub text_position: DimensionTextPosition,
    pub keep_text_aligned: bool,
    pub text_size_um: Um,
    pub stroke_width: Um,
    pub arrow_length: Um,
    pub extension_offset: Um,
    /// `PCB_DIM_ALIGNED`'s own constructor default (`m_arrowLength *
    /// sin(27.5deg)`) rather than a `BOARD_DESIGN_SETTINGS` field -- source
    /// has no separate board-wide setting for this either.
    pub extension_height: Um,
}

impl Default for DimensionSettings {
    /// `BOARD_DESIGN_SETTINGS::BOARD_DESIGN_SETTINGS()`'s own real
    /// defaults (`board_design_settings.cpp`): precision X_XXXX (4
    /// decimals here, this struct's flat scale), units automatic, no unit
    /// suffix, suppress trailing zeroes, text outside, kept aligned,
    /// 50 mil arrow length, 0.5 mm extension offset. `text_size_um`/
    /// `stroke_width` fall back to this model's own general text/line
    /// defaults (source pulls these from the generic per-layer text/line
    /// style, not a dimension-specific constant).
    fn default() -> Self {
        let arrow_length = 1270; // 50 mil
        DimensionSettings {
            units: DimensionUnits::Automatic,
            units_format: DimensionUnitsFormat::NoSuffix,
            precision: 4,
            suppress_trailing_zeros: true,
            text_position: DimensionTextPosition::Outside,
            keep_text_aligned: true,
            text_size_um: 1000,
            stroke_width: 200,
            arrow_length,
            extension_offset: 500, // 0.5mm
            extension_height: (arrow_length as f64 * 27.5f64.to_radians().sin()).round() as Um,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DrawingsSection {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub shapes: Vec<Shape>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub texts: Vec<Text>,
    /// Task item 5. See [`Group`]'s own doc for why it lives here.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub groups: Vec<Group>,
    /// Task item 7. See [`Dimension`]'s own doc.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub dimensions: Vec<Dimension>,
    /// `BOARD_DESIGN_SETTINGS`'s dimension-related fields -- the defaults
    /// a freshly drawn [`Dimension`] is styled from (`StyleFromSettings`),
    /// then carries its own copies of afterward (this struct is never
    /// re-applied automatically, matching source).
    #[serde(default)]
    pub dimension_settings: DimensionSettings,
    /// Per-placed-footprint solder-mask/silk facts an imported board needs
    /// but a library [`crate::Footprint`] + [`FootprintInstance`] cannot
    /// carry (pad layer sets and mask margins, footprint graphics on the
    /// silk/mask layers, net-tie groups, `allow_soldermask_bridges`). Read
    /// only by `eda_drc`'s silk/solder-mask providers; empty for a design
    /// that did not come from a `.kicad_pcb`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub footprint_extras: Vec<FootprintExtra>,
    /// Zone-connection edits made on the board to placed footprints and their pads (Footprint Properties and Pad Properties:
    /// Zone connection, relief gap, spoke width and angle, clearance). They sit over what a `.kicad_pcb` import left in
    /// [`FootprintExtra`], and over a published library footprint's own: see [`Design::pad_zone_facts`]. Additive: absent in an
    /// older `design.json` reads as "no edits".
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub zone_overrides: Vec<FootprintZoneOverrides>,
    /// Explicit per-via tenting overrides (`(via ... (tenting ..))`), looked
    /// up by position+net; a via with no entry follows the board setting.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub via_tenting: Vec<ViaTenting>,
    /// Board-level `gr_text` on a silkscreen layer with its full layout
    /// attributes (vertical justification, glyph width/height), imported
    /// from a `.kicad_pcb`. When non-empty these replace `texts`' silkscreen
    /// entries in the silk DRC.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub silk_texts: Vec<FootprintText>,
    /// Board-level `gr_text` on a copper layer: real copper, clearance-
    /// checked against other copper and knocked out of zone fills.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub copper_texts: Vec<FootprintText>,
    /// `BOARD_ITEM::IsLocked()`: ids of every locked item -- a placed part's
    /// reference, or a track/via/zone/shape/text id. Cross-kind on purpose,
    /// same storage-convenience reason [`Group`] lives here (one set, one
    /// `Cmd::SetLocked`, no per-struct field to thread through every
    /// construction site). Sorted and de-duplicated; `eda_ops` prunes ids
    /// whose item no longer exists whenever it rewrites the set. Additive:
    /// absent in an older `design.json` reads as "nothing locked".
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub locked_ids: Vec<String>,
    /// `BOARD_DESIGN_SETTINGS::GetAuxOrigin()`: the drill/place file origin
    /// (`pcbnew.EditorControl.drillOrigin`), the point drill files, position
    /// files and the Gerber plots measure from when they ask for it. `None` is
    /// KiCad's default, (0, 0). Written to the derived `.kicad_pcb` as
    /// `(aux_axis_origin x y)`. Additive: absent in an older `design.json`
    /// reads as "no origin set".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub aux_origin: Option<Point>,
    /// `BOARD::GetPageSettings()`: the board's paper (Page Settings, `common.Control.pageSettings`), written to the
    /// derived `.kicad_pcb` as `(paper ...)`. `None` is KiCad's default, A4 landscape. Additive.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub page: Option<crate::page::PageSettings>,
    /// `BOARD::GetTitleBlock()`: the board's title block (Page Settings), written as `(title_block ...)`. `None` is an empty
    /// one. Additive.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title_block: Option<TitleBlock>,
    /// `BOARD_DESIGN_SETTINGS::GetGridOrigin()`: the point the editing grid is anchored at (`common.Control.gridSetOrigin` /
    /// `gridResetOrigin` / `editGridOrigin`), so a grid point is `origin + n * grid`. `None` is KiCad's default, (0, 0). Written to the
    /// derived `.kicad_pcb` as `(grid_origin x y)`. Additive: absent in an older `design.json` reads as "no origin set".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub grid_origin: Option<Point>,
    /// Footprints that exist only on the board: copies of a footprint, and footprints pasted in from another board or from KiCad's
    /// clipboard, which have no part in the intent and no symbol in the schematic (KiCad's footprint without a schematic link).
    /// `board::load` folds each into the model as a part of its own -- see [`BoardPart`]. Additive: absent in an older
    /// `design.json` reads as "every footprint is one of the intent's parts".
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub board_parts: Vec<BoardPart>,
    /// Board Setup's edits to the board's rules (net classes, constraints, solder mask and paste, text defaults,
    /// the stackup, violation severities, custom rules): laid over the intent's rules each time the board is
    /// loaded (`crate::rules::RulesOverlay::apply`, `crates/cli/src/board.rs::load`), the way `Design::nets` is.
    /// It lives here and not on `Design` because this is the one optional board-level section that derives
    /// `Default` -- `Design` is written out field by field in some fifty places. Additive: absent in an older
    /// `design.json` reads as "the intent's rules stand".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rules: Option<crate::rules::RulesOverlay>,
    /// The DRC violations the user waived (`dialog_drc.cpp`'s "Exclude this violation"): `BOARD_DESIGN_SETTINGS::m_DrcExclusions`, written
    /// to the derived `.kicad_pro` as `board.design_settings.drc_exclusions`. Sorted by key, one entry per `(check, items)`. Additive:
    /// absent in an older `design.json` reads as "nothing waived". Lives here for the reason `rules` does.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub drc_exclusions: Vec<DrcExclusion>,
    /// `true` when the board's outline is made of the shapes on layer `Edge.Cuts` in [`Self::shapes`] and of nothing else: an
    /// imported board whose Edge.Cuts have arcs, circles, rectangles, curves, cutouts or several loops, which a closed polygon of
    /// straight edges cannot hold. `placement.outline` is then only their summary (the outer ring of the largest outline, curves
    /// flattened -- `eda_drc::outline::refresh_outline_summary`), kept for the readers that want one polygon, and it is not written
    /// to the `.kicad_pcb`. `false` (the default, and an intent's outline) leaves `placement.outline` itself an Edge.Cuts item and any
    /// shape on Edge.Cuts one more of them, a cutout drawn on a board. See `eda_model::outline::edge_cuts_shapes`. Additive: absent in
    /// an older `design.json` reads as `false`. Lives here for the reason `rules` does.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub outline_is_shapes: bool,
}

/// A footprint that lives on the board and nowhere else: what Duplicate and Paste make when they copy a footprint, since the part
/// the footprint stands for has to exist for it to be placed. It plays the role of an intent part for everything downstream --
/// `board::load` adds a [`crate::Part`] named `reference` to the model, joins its pads to the nets in `pad_nets` (a net the board
/// does not have yet is created), and gives its footprint name `definition`'s pads when nothing else resolves that name. The pose
/// is an ordinary `FootprintInstance` in `placement.footprints`; Delete removes the pose and this entry together.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BoardPart {
    /// The reference designator, unique among every part of the board.
    pub reference: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
    /// The footprint's name ("Lib:Name" or a bare name), as `Part::footprint` carries it.
    pub footprint: String,
    /// The footprint as the copy was made from it: its pads and courtyard, used when the model resolves nothing under `footprint`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub definition: Option<LibraryFootprint>,
    /// Pad number -> net name, sorted by pad number. A pad not listed is on no net.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pad_nets: Vec<(String, String)>,
}

/// `PADSTACK`/`PAD` facts for one imported pad that [`crate::Pad`] has no
/// field for. One entry per retained pad, in the same order as the
/// footprint's `pads`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PadMaskInfo {
    /// Every layer named in the pad's `(layers ..)`, wildcards expanded
    /// (`*.Cu` -> every copper layer, `*.Mask` -> `F.Mask`+`B.Mask`).
    #[serde(default)]
    pub layers: Vec<String>,
    /// `(solder_mask_margin ..)`: the pad's own margin override
    /// (`PADSTACK::SolderMaskMargin`); `None` inherits.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub solder_mask_margin: Option<Um>,
    /// `(tenting ..)` per outer side (`PADSTACK::*OuterLayers().has_solder_mask`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tent_front: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tent_back: Option<bool>,
    /// `(pintype "..")`, e.g. `"free"` (see `PAD::IsFreePad`).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub pin_type: String,
    /// `(clearance C)` (`PAD::GetLocalClearance`): the pad's own copper clearance, which replaces the net class's and the
    /// zone's for this pad (`DRC_ENGINE::EvalRules`: a local override wins over everything but the board minimum). `None`
    /// inherits the footprint's, then the rules'. Pre-9.0 files: 0 meant "inherit".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub clearance: Option<Um>,
    /// `(zone_connect N)` (`PAD::GetLocalZoneConnection`): how a zone connects to this pad, overriding the footprint's
    /// and then the zone's own `pad_connection`. `None` is `INHERITED`. Read by the zone filler
    /// (`ZONE_FILLER::knockoutThermalReliefs` -> `DRC_ENGINE::EvalZoneConnection`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub zone_connection: Option<PadConnection>,
    /// `(thermal_gap g)` (`PAD::GetLocalThermalGapOverride`): the pad's own thermal relief gap; `None` inherits the zone's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thermal_gap: Option<Um>,
    /// `(thermal_bridge_width w)` (`PAD::GetLocalThermalSpokeWidthOverride`): the pad's own spoke width; `None` inherits the zone's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thermal_spoke_width: Option<Um>,
    /// `(thermal_bridge_angle a)` (`PAD::GetThermalSpokeAngle`), millidegrees in KiCad's own sign convention (as in the
    /// file). `None` is the shape's default: 90 degrees for an oval or (rounded) rectangle, 45 for a circle.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thermal_spoke_angle_mdeg: Option<Millideg>,
}

/// One placed pad's zone-connection facts, resolved: its own override over its footprint's, then the zone's own
/// `pad_connection` (applied by the filler, `DRC_ENGINE::EvalZoneConnection`). See [`Design::pad_zone_facts`].
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct PadZoneFacts {
    /// The pad's own `(clearance ..)`; `None` inherits the footprint's. See [`PadZoneFacts::clearance`].
    pub pad_clearance: Option<Um>,
    /// The footprint's `(clearance ..)`.
    pub footprint_clearance: Option<Um>,
    /// The pad's own `(zone_connect ..)`; `None` inherits.
    pub connection: Option<PadConnection>,
    /// The footprint's `(zone_connect ..)`; `None` inherits (consulted when the pad inherits).
    pub footprint_connection: Option<PadConnection>,
    pub thermal_gap: Option<Um>,
    pub thermal_spoke_width: Option<Um>,
    /// Millidegrees, KiCad's sign convention; `None` is the shape's default.
    pub thermal_spoke_angle_mdeg: Option<Millideg>,
}

/// An edit, made on the board, of the zone-connection facts of the pads that carry one pad number in one footprint (a number
/// can be shared by several pads, all of which go on the same pin): every field is the pad's own and `None` inherits.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PadZoneOverride {
    pub number: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub clearance: Option<Um>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub zone_connection: Option<PadConnection>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thermal_gap: Option<Um>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thermal_spoke_width: Option<Um>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thermal_spoke_angle_mdeg: Option<Millideg>,
}

/// The footprint-level facts of a [`FootprintZoneOverrides`]: `FOOTPRINT::SetLocalZoneConnection` and `SetLocalClearance`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FootprintZoneFacts {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub zone_connection: Option<PadConnection>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub clearance: Option<Um>,
}

/// Board-level zone-connection edits of one placed footprint (`Cmd::SetFootprintZoneConnection`, `Cmd::SetPadZoneOverrides`).
/// A footprint with `footprint` set has its footprint-level facts taken from here whole (`None` fields inherit); a pad number
/// listed in `pads` has its facts taken from here whole. Whatever is not listed keeps the imported or published facts.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FootprintZoneOverrides {
    /// The footprint instance's reference designator.
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub footprint: Option<FootprintZoneFacts>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pads: Vec<PadZoneOverride>,
}

impl Design {
    /// The zone-connection facts of pad number `pad_number` (index `pad_idx` of `pad_count`) of placed footprint `fp_id`
    /// whose definition is `footprint_name`, from the first source that has them for that pad: an edit made on the board
    /// ([`DrawingsSection::zone_overrides`]); else the facts the footprint instance carries from a `.kicad_pcb` import
    /// ([`FootprintExtra`], where each board pad holds its own copy, exactly KiCad); else a *published* library footprint of
    /// that name (`Cmd::UpdateFootprintOnBoard`), since the board's pads then come from it.
    pub fn pad_zone_facts(&self, fp_id: &str, footprint_name: &str, pad_idx: usize, pad_count: usize, pad_number: &str) -> PadZoneFacts {
        let dr = self.drawings.as_ref();
        // The pad's own clearance and its footprint's are kept apart until the end (the pad's wins).
        let (mut pad_clearance, mut fp_clearance, mut facts) = (None, None, PadZoneFacts::default());
        if let Some(extra) = dr.and_then(|d| d.footprint_extras.iter().find(|e| e.id == fp_id)) {
            let pad = if extra.pads.len() == pad_count { extra.pads.get(pad_idx) } else { None };
            pad_clearance = pad.and_then(|p| p.clearance);
            fp_clearance = extra.clearance;
            facts = PadZoneFacts {
                pad_clearance: None,
                footprint_clearance: None,
                connection: pad.and_then(|p| p.zone_connection),
                footprint_connection: extra.zone_connection,
                thermal_gap: pad.and_then(|p| p.thermal_gap),
                thermal_spoke_width: pad.and_then(|p| p.thermal_spoke_width),
                thermal_spoke_angle_mdeg: pad.and_then(|p| p.thermal_spoke_angle_mdeg),
            };
        } else if let Some(lib) = self.footprint_library.as_ref().and_then(|l| l.by_name(footprint_name)).filter(|f| f.published) {
            let pad = if lib.pads.len() == pad_count { lib.pads.get(pad_idx) } else { None };
            pad_clearance = pad.and_then(|p| p.clearance_override);
            facts = PadZoneFacts {
                pad_clearance: None,
                footprint_clearance: None,
                connection: pad.and_then(|p| p.zone_connection),
                footprint_connection: lib.zone_connection,
                thermal_gap: pad.and_then(|p| p.thermal_gap_override),
                thermal_spoke_width: pad.and_then(|p| p.thermal_spoke_width_override),
                thermal_spoke_angle_mdeg: pad.and_then(|p| p.thermal_spoke_angle_mdeg),
            };
        }
        // Edits made on the board replace what they list, whole.
        if let Some(edit) = dr.and_then(|d| d.zone_overrides.iter().find(|e| e.id == fp_id)) {
            if let Some(f) = edit.footprint {
                facts.footprint_connection = f.zone_connection;
                fp_clearance = f.clearance;
            }
            if let Some(p) = edit.pads.iter().find(|p| p.number == pad_number) {
                facts.connection = p.zone_connection;
                facts.thermal_gap = p.thermal_gap;
                facts.thermal_spoke_width = p.thermal_spoke_width;
                facts.thermal_spoke_angle_mdeg = p.thermal_spoke_angle_mdeg;
                pad_clearance = p.clearance;
            }
        }
        facts.pad_clearance = pad_clearance;
        facts.footprint_clearance = fp_clearance;
        facts
    }
}

impl Design {
    /// The zone-connection facts of placed footprint `fp_id` and its pads (`pad_numbers`, in pad order; `footprint_name` is its
    /// definition's name) as the board edit that gives another footprint the same ones, filed under `fp_id`: what a Copy, a Paste
    /// and a Duplicate hand on, since a copy is a new footprint that has no import behind it. `None` when neither the footprint nor
    /// any pad sets anything. See [`Design::pad_zone_facts`].
    pub fn zone_overrides_of(&self, fp_id: &str, footprint_name: &str, pad_numbers: &[&str]) -> Option<FootprintZoneOverrides> {
        let mut edit = FootprintZoneOverrides { id: fp_id.to_string(), ..Default::default() };
        for (i, number) in pad_numbers.iter().enumerate() {
            let f = self.pad_zone_facts(fp_id, footprint_name, i, pad_numbers.len(), number);
            if edit.footprint.is_none() && (f.footprint_clearance.is_some() || f.footprint_connection.is_some()) {
                edit.footprint = Some(FootprintZoneFacts { zone_connection: f.footprint_connection, clearance: f.footprint_clearance });
            }
            let own = PadZoneOverride {
                number: (*number).to_string(),
                clearance: f.pad_clearance,
                zone_connection: f.connection,
                thermal_gap: f.thermal_gap,
                thermal_spoke_width: f.thermal_spoke_width,
                thermal_spoke_angle_mdeg: f.thermal_spoke_angle_mdeg,
            };
            let is_set = own.clearance.is_some() || own.zone_connection.is_some() || own.thermal_gap.is_some() || own.thermal_spoke_width.is_some() || own.thermal_spoke_angle_mdeg.is_some();
            // Pads that share a number share their facts (an edit names the number).
            if is_set && !edit.pads.iter().any(|p| p.number == *number) {
                edit.pads.push(own);
            }
        }
        edit.pads.sort_by(|a, b| a.number.cmp(&b.number));
        (edit.footprint.is_some() || !edit.pads.is_empty()).then_some(edit)
    }
}

impl PadZoneFacts {
    /// `PAD::GetClearanceOverrides`: the pad's own clearance, else its footprint's; `None` when neither sets one.
    pub fn clearance(&self) -> Option<Um> {
        self.pad_clearance.or(self.footprint_clearance)
    }
}

/// A footprint-owned graphic (or mask-only pad) in board space.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FootprintGraphic {
    pub shape: Shape,
    /// `PCB_SHAPE::m_solderMaskMargin`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub solder_mask_margin: Option<Um>,
    /// `Some(number)` when this is really a pad that has no copper layer
    /// (a mask-only aperture pad), which KiCad models as `PCB_PAD_T`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pad_number: Option<String>,
    /// Net of a mask-only pad (empty = none).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub net: String,
    /// `(pintype ..)` of a mask-only pad.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub pin_type: String,
}

/// See [`DrawingsSection::footprint_extras`].
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FootprintExtra {
    /// The footprint instance's reference designator.
    pub id: String,
    /// Footprint-level `(solder_mask_margin ..)` (`GetLocalSolderMaskMargin`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub solder_mask_margin: Option<Um>,
    /// `(attr .. allow_soldermask_bridges)`.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub allow_soldermask_bridges: bool,
    /// `(net_tie_pad_groups "1,2" "3")`, verbatim.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub net_tie_pad_groups: Vec<String>,
    /// `(clearance C)` on the footprint (`FOOTPRINT::GetLocalClearance`): the copper clearance of every pad that does not
    /// set its own. `None` inherits the rules'.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub clearance: Option<Um>,
    /// `(zone_connect N)` on the footprint (`FOOTPRINT::GetLocalZoneConnection`): how zones connect to every pad that
    /// does not set its own. `None` is `INHERITED` (the zone's `pad_connection` applies).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub zone_connection: Option<PadConnection>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pads: Vec<PadMaskInfo>,
    /// Footprint graphics on silk/mask layers, board space.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub graphics: Vec<FootprintGraphic>,
    /// Visible footprint text (reference/value/user) on silk layers.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub texts: Vec<FootprintText>,
}

/// A visible footprint text item (`PCB_FIELD`/`PCB_TEXT` parented to a
/// footprint) on a silkscreen layer, with everything `EDA_TEXT::
/// GetEffectiveTextShape` needs to lay out its stroke-font glyphs.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FootprintText {
    /// Shown text, `${REFERENCE}`/`${VALUE}` already resolved.
    pub text: String,
    pub layer: String,
    /// Anchor in board space.
    pub at: Point,
    /// The file's absolute angle, KiCad's sign convention (counter-clockwise
    /// positive), millidegrees.
    pub angle_file_mdeg: i64,
    pub size: (Um, Um),
    /// `(thickness ..)` as written (0 = unset).
    pub thickness: Um,
    /// `-1` left, `0` centre, `1` right.
    pub halign: i8,
    /// `-1` top, `0` centre, `1` bottom.
    pub valign: i8,
    pub mirror: bool,
    /// `PCB_TEXT::IsKeepUpright` (footprint text default).
    pub keep_upright: bool,
    pub bold: bool,
}

/// See [`DrawingsSection::via_tenting`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ViaTenting {
    pub at: Point,
    pub net: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub front: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub back: Option<bool>,
}

impl DrawingsSection {
    /// Assign a deterministic id to every shape/text/group/dimension
    /// whose `id` is still empty. See [`RoutingSection::assign_missing_ids`]
    /// -- same contract, same reason for a stable per-kind processing order.
    pub fn assign_missing_ids(&mut self) {
        let mut existing: std::collections::BTreeSet<String> = self.shapes.iter().map(Shape::id).chain(self.texts.iter().map(|t| t.id.as_str())).chain(self.groups.iter().map(|g| g.id.as_str())).chain(self.dimensions.iter().map(|d| d.id.as_str())).filter(|s| !s.is_empty()).map(String::from).collect();

        let mut order: Vec<usize> = (0..self.shapes.len()).collect();
        order.sort_by(|&a, &b| (self.shapes[a].layer(), self.shapes[a].points().first().copied()).cmp(&(self.shapes[b].layer(), self.shapes[b].points().first().copied())));
        for i in order {
            if self.shapes[i].id().is_empty() {
                let id = next_item_id("shp", &self.shapes[i].id_seed(), &existing);
                existing.insert(id.clone());
                self.shapes[i].set_id(id);
            }
        }

        let mut order: Vec<usize> = (0..self.texts.len()).collect();
        order.sort_by(|&a, &b| (&self.texts[a].content, self.texts[a].at).cmp(&(&self.texts[b].content, self.texts[b].at)));
        for i in order {
            if self.texts[i].id.is_empty() {
                let id = next_item_id("txt", &self.texts[i].id_seed(), &existing);
                existing.insert(id.clone());
                self.texts[i].id = id;
            }
        }

        let mut order: Vec<usize> = (0..self.groups.len()).collect();
        order.sort_by(|&a, &b| self.groups[a].id_seed().cmp(&self.groups[b].id_seed()));
        for i in order {
            if self.groups[i].id.is_empty() {
                let id = next_item_id("grp", &self.groups[i].id_seed(), &existing);
                existing.insert(id.clone());
                self.groups[i].id = id;
            }
        }

        let mut order: Vec<usize> = (0..self.dimensions.len()).collect();
        order.sort_by(|&a, &b| (self.dimensions[a].start, self.dimensions[a].end).cmp(&(self.dimensions[b].start, self.dimensions[b].end)));
        for i in order {
            if self.dimensions[i].id.is_empty() {
                let id = next_item_id("dim", &self.dimensions[i].id_seed(), &existing);
                existing.insert(id.clone());
                self.dimensions[i].id = id;
            }
        }
    }
}

// ---------- footprint library (the Footprint Editor's own content) ----------

/// GAPS.md #8's editable footprint library: one entry per footprint name
/// the user has opened in the Footprint Editor. Additive, same convention
/// as every other `Design` section -- a `design.json` written before this
/// editor existed simply has none.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FootprintLibrarySection {
    /// Sorted by `name`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub footprints: Vec<LibraryFootprint>,
}

/// KiCad's `FOOTPRINT_ATTR_T` flags (`pcbnew/footprint.h:83-91`:
/// `FP_THROUGH_HOLE=0x0001, FP_SMD=0x0002, FP_EXCLUDE_FROM_POS_FILES=0x0004,
/// FP_EXCLUDE_FROM_BOM=0x0008, FP_BOARD_ONLY=0x0010, FP_DNP=0x0040`) plus
/// the dialog's two related-but-not-attribute-bit checkboxes
/// (`m_noCourtyards`/`AllowMissingCourtyard`, `m_allowBridges`/
/// `AllowSolderMaskBridges`) -- all on the Footprint Properties dialog's
/// General tab (`dialog_footprint_properties_fp_editor.cpp`). Not mutually
/// exclusive (KiCad itself allows a footprint with neither `smd` nor
/// `through_hole` set -- "Normal", e.g. a pure-mechanical part).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FootprintAttributes {
    #[serde(default)]
    pub smd: bool,
    #[serde(default)]
    pub through_hole: bool,
    #[serde(default)]
    pub exclude_from_bom: bool,
    #[serde(default)]
    pub exclude_from_position_files: bool,
    /// "Board Only" -- not placed from the schematic/BOM at all (a
    /// mechanical-only footprint with no symbol counterpart).
    #[serde(default)]
    pub board_only: bool,
    /// "Do Not Populate" (`FP_DNP`) -- greyed out on the board, excluded
    /// from the BOM/position files regardless of those two flags' own
    /// state.
    #[serde(default)]
    pub dnp: bool,
    /// Suppresses the "missing courtyard" footprint-checker warning for a
    /// footprint that genuinely has none by design.
    #[serde(default)]
    pub allow_missing_courtyard: bool,
    /// Suppresses the "silkscreen clipped by solder mask" warning where
    /// this footprint intentionally overlaps the two.
    #[serde(default)]
    pub allow_soldermask_bridges: bool,
}

fn d_true() -> bool {
    true
}

/// One named field on a footprint (KiCad's `PCB_FIELD`, e.g. a custom
/// "Vendor" or "Datasheet" field beyond the built-in Reference/Value) --
/// `dialog_footprint_properties_fp_editor.cpp`'s Fields tab. Position/
/// orientation/layer are deferred (see PARITY-fpedit.md): only name/value/
/// visibility are modeled, enough to author one and read it back.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FootprintField {
    pub name: String,
    pub value: String,
    #[serde(default = "d_true")]
    pub visible: bool,
}

/// One pad on a [`LibraryFootprint`], in the footprint's own local frame
/// (see that type's doc). Mirrors `crate::footprint::Pad` field-for-field
/// (shape/kind/drill/drill_slot/rot/roundrect_ratio all mean exactly the
/// same thing there) plus what the Pad Properties dialog needs beyond
/// placement geometry: a stable `id` (pad *numbers* are deliberately not
/// unique -- see `crate::footprint::PlacedPad::number`'s own doc -- so
/// addressing one pad for move/edit/delete needs a real id, same reason
/// `Track`/`Via`/`Shape`/`Text` each have one) and per-pad clearance/
/// thermal overrides (`dialog_pad_properties.cpp`'s "Clearance Overrides
/// and Settings" panel; `None` = "use board/zone default", KiCad's own
/// 0-means-inherit convention).
///
/// Deliberately a distinct type from `crate::footprint::Pad` rather than
/// adding `id`/overrides to that one directly: `Pad` is constructed by
/// struct literal (not just `Pad::simple`) in a dozen call sites across
/// `crates/kicad`, `crates/connectivity`, `crates/freeroute`,
/// `crates/interchange` and more -- every one *outside* this task's
/// footprint-editor scope -- and a new required field there would be a
/// breaking change to all of them for a concept (editor identity/
/// overrides) only the editor's own JSON needs. `to_engine_pad`/
/// `from_engine_pad` below are the one seam between the two.
///
/// `shape` is this editor's OWN shape enum ([`LibraryPadShape`]), not
/// `crate::footprint::PadShape` -- it needs two more members (Trapezoid,
/// ChamferedRect, `dialog_pad_properties.cpp`'s shape dropdown) the engine
/// enum doesn't have, for the same "don't touch a type a dozen unrelated
/// call sites construct by struct literal" reason `LibraryPad` itself is
/// a distinct type (see this type's own doc above). `to_engine_pad` maps
/// both of those down to the closest engine shape (documented there).
///
/// `layers` is likewise editor/export-only metadata `crate::footprint::
/// Pad` has no field for at all (the engine infers through-hole-ness from
/// `kind` alone -- see `PlacedPad::through_hole` -- and never needed a
/// real per-pad layer set); this editor keeps it so the Layers tab has
/// something real to show/edit and the derived `.kicad_mod` export (step
/// 6) can write a correct `(layers ...)` line instead of guessing one
/// from `kind`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LibraryPad {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub id: String,
    pub number: String,
    pub at: Point,
    pub size: (Um, Um),
    /// `PAD::SetOffset` -- the copper shape's center, relative to `at`
    /// (the pad's drill/anchor position). `{0,0}` for the overwhelming
    /// majority of pads; non-zero mainly shows up on an SMD pad shaped to
    /// land a lead off-center from its footprint-grid position.
    #[serde(default)]
    pub offset: Point,
    #[serde(default)]
    pub shape: LibraryPadShape,
    #[serde(default)]
    pub kind: crate::footprint::PadKind,
    #[serde(default)]
    pub drill: Option<Um>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub drill_slot: Option<(Um, Um)>,
    #[serde(default)]
    pub rot: Millideg,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub roundrect_ratio: Option<f64>,
    /// `PAD::SetTrapezoidDeltaSize` -- (dx, dy), KiCad's own convention of
    /// only ever one axis non-zero (the dialog's axis radio picks which).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trapezoid_delta: Option<(Um, Um)>,
    /// `PAD::SetChamferRectRatio` -- fraction of the shorter side, same
    /// shape as `roundrect_ratio`. Only meaningful for `ChamferedRect`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chamfer_ratio: Option<f64>,
    /// `PAD::SetChamferPositions` -- which corners are cut.
    #[serde(default)]
    pub chamfer_corners: ChamferCorners,
    /// KiCad layer names this pad occupies ("F.Cu", "F.Paste", "F.Mask",
    /// "*.Cu", ...) -- see this type's own doc on why the engine's `Pad`
    /// has nothing equivalent. Empty means "not set yet"; the Pad
    /// Properties dialog's layer-preset buttons (SMD/THT/NPTH/connector)
    /// are what normally fill this in.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub layers: Vec<String>,
    /// `PAD::GetLocalClearance()` -- `None` = inherit the board/footprint default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub clearance_override: Option<Um>,
    /// `PAD::GetLocalThermalGapOverride()`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thermal_gap_override: Option<Um>,
    /// `PAD::GetLocalThermalSpokeWidthOverride()`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thermal_spoke_width_override: Option<Um>,
    /// `PAD::GetLocalZoneConnection()` -- the Pad Properties dialog's "Pad connection" choice; `None` is `INHERITED`
    /// ("From parent footprint").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub zone_connection: Option<PadConnection>,
    /// `PAD::GetThermalSpokeAngle()` -- the dialog's "Spoke angle", millidegrees as in the file; `None` is the shape's
    /// default (90 degrees for an oval or (rounded) rectangle, 45 for a circle).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thermal_spoke_angle_mdeg: Option<Millideg>,
}

/// `PAD_SHAPE` (`pcbnew/padstack.h`) as the Footprint Editor exposes it --
/// see `LibraryPad`'s own doc for why this is a distinct enum from
/// `crate::footprint::PadShape` rather than an extension of it. Custom
/// (primitive-based) pad shapes are not modeled -- KiCad's own Explode/
/// Recombine editing mode for them is a substantial separate subsystem
/// (see PARITY-fpedit.md); a `.kicad_mod` with a custom-shape pad can
/// still be *opened* (the pad's anchor shape/size loads, the custom
/// primitives are dropped with a warning), just not authored here.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LibraryPadShape {
    Circle,
    #[default]
    Rect,
    Oval,
    RoundRect,
    Trapezoid,
    ChamferedRect,
}

/// `PAD::SetChamferPositions`' four corner flags (`RECT_CHAMFER_TOP_LEFT`
/// etc, `pcbnew/padstack.h`), un-rotated pad-local corners.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChamferCorners {
    #[serde(default)]
    pub top_left: bool,
    #[serde(default)]
    pub top_right: bool,
    #[serde(default)]
    pub bottom_left: bool,
    #[serde(default)]
    pub bottom_right: bool,
}

impl LibraryPad {
    fn id_seed(&self) -> String {
        format!("{}|{},{}|{}x{}|{:?}|{:?}", self.number, self.at.x, self.at.y, self.size.0, self.size.1, self.shape, self.kind)
    }

    /// The same pad, as the engine's `crate::footprint::Pad` -- drops
    /// `id`, `layers`, and the override fields (see this type's own doc),
    /// and maps the two shapes the engine doesn't have down to the
    /// closest one it does, so placement/routing/DRC still see a sane
    /// bounding shape even though they can't draw the true outline:
    /// `Trapezoid` -> `Rect` (its own bounding box -- `size` already is
    /// the trapezoid's full bounding width/height, same convention
    /// `PAD_SHAPE::TRAPEZOID`'s own `GetBoundingBox` uses), `ChamferedRect`
    /// -> `RoundRect` (reuses `chamfer_ratio` as the roundrect ratio --
    /// both express "fraction of the shorter side cut from the corner",
    /// so the clearance-relevant corner inset is the same order of
    /// magnitude even though one cuts a chamfer and the other a fillet).
    pub fn to_engine_pad(&self) -> crate::footprint::Pad {
        let (shape, roundrect_ratio) = match self.shape {
            LibraryPadShape::Circle => (crate::footprint::PadShape::Circle, None),
            LibraryPadShape::Rect | LibraryPadShape::Trapezoid => (crate::footprint::PadShape::Rect, None),
            LibraryPadShape::Oval => (crate::footprint::PadShape::Oval, None),
            LibraryPadShape::RoundRect => (crate::footprint::PadShape::RoundRect, self.roundrect_ratio),
            LibraryPadShape::ChamferedRect => (crate::footprint::PadShape::RoundRect, self.chamfer_ratio),
        };
        crate::footprint::Pad { opposite_side: false,
            number: self.number.clone(),
            at: (self.at.x, self.at.y),
            size: self.size,
            shape,
            kind: self.kind,
            drill: self.drill,
            drill_slot: self.drill_slot,
            rot: self.rot,
            roundrect_ratio,
        }
    }

    /// The reverse of `to_engine_pad`, for materializing an editable copy
    /// of a resolved `crate::footprint::Footprint` (builtin table, intent-
    /// declared, or a real `.kicad_mod` library file) the first time the
    /// Footprint Editor opens it -- `id` is left empty (the caller backfills
    /// via `LibraryFootprint::assign_missing_ids`) and every editor-only
    /// field (layers, overrides, trapezoid/chamfer) starts empty/`None`,
    /// since the engine's own `Pad` never had one to read back.
    pub fn from_engine_pad(p: &crate::footprint::Pad) -> Self {
        let shape = match p.shape {
            crate::footprint::PadShape::Rect => LibraryPadShape::Rect,
            crate::footprint::PadShape::RoundRect => LibraryPadShape::RoundRect,
            crate::footprint::PadShape::Circle => LibraryPadShape::Circle,
            crate::footprint::PadShape::Oval => LibraryPadShape::Oval,
        };
        LibraryPad {
            id: String::new(),
            number: p.number.clone(),
            at: Point { x: p.at.0, y: p.at.1 },
            size: p.size,
            offset: Point::default(),
            shape,
            kind: p.kind,
            drill: p.drill,
            drill_slot: p.drill_slot,
            rot: p.rot,
            roundrect_ratio: p.roundrect_ratio,
            trapezoid_delta: None,
            chamfer_ratio: None,
            chamfer_corners: ChamferCorners::default(),
            layers: Vec::new(),
            clearance_override: None,
            thermal_gap_override: None,
            thermal_spoke_width_override: None,
            zone_connection: None,
            thermal_spoke_angle_mdeg: None,
        }
    }
}

/// A footprint definition as the Footprint Editor shows/edits it --
/// GAPS.md #8's "project footprint library section in the IR". Local
/// frame: micrometers, origin at `anchor` (KiCad's "Place Footprint
/// Anchor" point -- `{0,0}` until that tool moves it), +x right, +y down,
/// un-rotated, always as seen from the top side -- same convention
/// `crate::footprint` already documents for the engine's own `Footprint`/
/// `Pad`, so a `to_engine_footprint`/`from_engine_footprint` round trip
/// never needs an axis flip.
///
/// Reuses `Shape`/`Text` (this module's own board-drawing types) for
/// `graphics`/`texts` rather than inventing parallel footprint-graphic
/// types: the fields (layer/stroke_width/filled/points, or content/at/
/// angle/size/justify/mirror) mean exactly the same thing, just read in
/// this footprint's local frame instead of board space -- `Shape::
/// translate`/`set_layer`/`set_stroke_width`/`set_filled` and each type's
/// own `id`/`set_id` all come along for free.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LibraryFootprint {
    /// "Lib:Name" (opened from a loaded library) or a bare name (authored
    /// from scratch here) -- the same string a `Part::footprint`/
    /// `Part::package` names, and this section's own addressing key (no
    /// separate id: footprint names are already unique the way a
    /// Track/Via/Shape/Text's content never is).
    pub name: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub keywords: String,
    #[serde(default)]
    pub attributes: FootprintAttributes,
    /// Sorted by `id`.
    #[serde(default)]
    pub pads: Vec<LibraryPad>,
    /// Free-standing graphics (F.SilkS/F.Fab/F.CrtYd/...), sorted by `id`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub graphics: Vec<Shape>,
    /// Sorted by `id`. The footprint's own Reference/Value text fields are
    /// NOT in this list (same split `FootprintField`'s doc notes) -- see
    /// `reference_visible`/`value_visible` below for the little this
    /// editor models of those two.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub texts: Vec<Text>,
    /// Extra named fields beyond Reference/Value/Footprint (KiCad's own
    /// built-in three), e.g. a custom "Vendor" or "Datasheet" field.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fields: Vec<FootprintField>,
    #[serde(default = "d_true")]
    pub reference_visible: bool,
    #[serde(default = "d_true")]
    pub value_visible: bool,
    /// Courtyard half-extents -- same field/meaning as `crate::footprint::
    /// Footprint::courtyard` (`None` derives one from the pad bbox).
    #[serde(default)]
    pub courtyard: Option<(Um, Um)>,
    /// `(model "...")` 3D model path -- same field/meaning as
    /// `crate::footprint::Footprint::model`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// KiCad's footprint anchor (`footprint_editor_control.cpp`'s "Place
    /// Footprint Anchor" tool): the local-frame point every pad/graphic/
    /// text coordinate above is relative to. `{0,0}` (KiCad's
    /// overwhelmingly common case) until that tool moves it.
    #[serde(default)]
    pub anchor: Point,
    /// Whether a board footprint instance naming this footprint should
    /// resolve its pads/courtyard from here (`crate::board::load`'s
    /// overlay onto `ConstraintModel::footprints`, filtered on this flag)
    /// -- KiCad's "Update Footprint from Library", kept an explicit,
    /// user-triggered action rather than instant/automatic propagation:
    /// editing pads here never touches the board on its own; only
    /// `Cmd::UpdateFootprintOnBoard` sets this `true`. Starts `false`, so
    /// opening a real library footprint and nudging a pad can never
    /// change what's already placed without that separate, explicit step.
    #[serde(default)]
    pub published: bool,
    /// `FOOTPRINT::GetLocalZoneConnection()` -- the Footprint Properties dialog's "Zone connection": how a zone connects to
    /// every pad of this footprint that does not set its own. `None` is `INHERITED`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub zone_connection: Option<PadConnection>,
}

impl Default for LibraryFootprint {
    fn default() -> Self {
        LibraryFootprint {
            name: String::new(),
            description: String::new(),
            keywords: String::new(),
            attributes: FootprintAttributes::default(),
            pads: Vec::new(),
            graphics: Vec::new(),
            texts: Vec::new(),
            fields: Vec::new(),
            reference_visible: true,
            value_visible: true,
            courtyard: None,
            model: None,
            anchor: Point { x: 0, y: 0 },
            published: false,
            zone_connection: None,
        }
    }
}

impl LibraryFootprint {
    /// A brand-new, empty footprint named `name` -- "New Footprint"
    /// (`footprint_editor_control.cpp`'s `NewFootprint`) before anything
    /// has been drawn in it.
    pub fn new_empty(name: impl Into<String>) -> Self {
        LibraryFootprint { name: name.into(), ..Default::default() }
    }

    /// An editable copy of an already-resolved engine `Footprint`
    /// (builtin table, intent-declared, or a real `.kicad_mod` library
    /// file parsed by `eda_kicad::footprint_lib`) -- what the Footprint
    /// Editor materializes into `FootprintLibrarySection` the first time
    /// an edit touches a footprint that was not already there. Every pad
    /// gets a fresh empty id (backfilled by the caller via
    /// `assign_missing_ids`); attributes/keywords/fields/anchor all start
    /// at their defaults since `crate::footprint::Footprint` has no
    /// concept of any of them yet.
    pub fn from_engine_footprint(fp: &crate::footprint::Footprint) -> Self {
        LibraryFootprint {
            name: fp.name.clone(),
            pads: fp.pads.iter().map(LibraryPad::from_engine_pad).collect(),
            courtyard: fp.courtyard,
            model: fp.model.clone(),
            ..Default::default()
        }
    }

    /// The reverse of `from_engine_footprint`/the basis for exporting a
    /// derived `.kicad_mod` (GAPS.md #8 step 6): this footprint's pads
    /// and courtyard/model as the engine's own `Footprint` -- graphics/
    /// text/fields/attributes stay editor-only (the engine has no use for
    /// silkscreen art or a BOM-exclude flag), same split `LibraryPad::
    /// to_engine_pad`'s doc explains.
    pub fn to_engine_footprint(&self) -> crate::footprint::Footprint {
        crate::footprint::Footprint { name: self.name.clone(), pads: self.pads.iter().map(LibraryPad::to_engine_pad).collect(), courtyard: self.courtyard, model: self.model.clone(), courtyard_outlines: vec![] }
    }

    /// Assign a deterministic id to every pad/graphic/text whose `id` is
    /// still empty -- same contract as `RoutingSection`/`DrawingsSection`'s
    /// own `assign_missing_ids` (stable per-kind processing order, `_2`/
    /// `_3`/... suffix on an exact-content duplicate).
    pub fn assign_missing_ids(&mut self) {
        let mut existing: std::collections::BTreeSet<String> = self.pads.iter().map(|p| p.id.clone()).filter(|s| !s.is_empty()).collect();
        existing.extend(self.graphics.iter().map(|g| g.id().to_string()).filter(|s| !s.is_empty()));
        existing.extend(self.texts.iter().map(|t| t.id.clone()).filter(|s| !s.is_empty()));

        let mut order: Vec<usize> = (0..self.pads.len()).collect();
        order.sort_by(|&a, &b| (&self.pads[a].number, self.pads[a].at).cmp(&(&self.pads[b].number, self.pads[b].at)));
        for i in order {
            if self.pads[i].id.is_empty() {
                let id = next_item_id("pad", &self.pads[i].id_seed(), &existing);
                existing.insert(id.clone());
                self.pads[i].id = id;
            }
        }

        let mut order: Vec<usize> = (0..self.graphics.len()).collect();
        order.sort_by(|&a, &b| (self.graphics[a].layer(), self.graphics[a].points().first().copied()).cmp(&(self.graphics[b].layer(), self.graphics[b].points().first().copied())));
        for i in order {
            if self.graphics[i].id().is_empty() {
                let pts: Vec<String> = self.graphics[i].points().iter().map(|p| format!("{},{}", p.x, p.y)).collect();
                let seed = format!("{}|{}", self.graphics[i].layer(), pts.join(";"));
                let id = next_item_id("fpg", &seed, &existing);
                existing.insert(id.clone());
                self.graphics[i].set_id(id);
            }
        }

        let mut order: Vec<usize> = (0..self.texts.len()).collect();
        order.sort_by(|&a, &b| (&self.texts[a].content, self.texts[a].at).cmp(&(&self.texts[b].content, self.texts[b].at)));
        for i in order {
            if self.texts[i].id.is_empty() {
                let seed = format!("{}|{},{}", self.texts[i].content, self.texts[i].at.x, self.texts[i].at.y);
                let id = next_item_id("fpt", &seed, &existing);
                existing.insert(id.clone());
                self.texts[i].id = id;
            }
        }
    }

    /// `FOOTPRINT::GetNextPadNumber` (`pad_tool.cpp`'s `PAD_PLACER`,
    /// `footprint.cpp:3357`): split `last` into its non-numeric prefix and
    /// trailing integer (ASCII digits only -- a non-ASCII pad number, rare
    /// in practice, keeps its whole prefix), then increment the integer
    /// until `prefix+integer` collides with no existing pad number on this
    /// footprint -- not just "+1": a manually-renumbered or out-of-order
    /// board is still handled correctly, same as source.
    pub fn next_pad_number_after(&self, last: &str) -> String {
        let digit_count = last.chars().rev().take_while(|c| c.is_ascii_digit()).count();
        let split_at = last.len() - digit_count;
        let prefix = &last[..split_at];
        let mut num: u64 = last[split_at..].parse().unwrap_or(0);
        let used: std::collections::BTreeSet<&str> = self.pads.iter().map(|p| p.number.as_str()).collect();
        loop {
            num += 1;
            let candidate = format!("{prefix}{num}");
            if !used.contains(candidate.as_str()) {
                return candidate;
            }
        }
    }

    /// `next_pad_number_after`, seeded from this footprint's own
    /// highest-numbered existing pad (KiCad's `PAD_TOOL::m_lastPadNumber`,
    /// which this editor has no standing interactive-session state to
    /// carry between Cmds for -- the highest existing number is the same
    /// "continue the sequence" answer in the overwhelmingly common case of
    /// placing pads in order). `"1"` for the first pad on an empty
    /// footprint, matching `m_lastPadNumber`'s own `"1"` reset default.
    pub fn next_pad_number(&self) -> String {
        let last = self
            .pads
            .iter()
            .max_by_key(|p| {
                let n = p.number.as_str();
                let digit_count = n.chars().rev().take_while(|c| c.is_ascii_digit()).count();
                n[n.len() - digit_count..].parse::<u64>().unwrap_or(0)
            })
            .map(|p| p.number.as_str())
            .unwrap_or("0");
        self.next_pad_number_after(last)
    }
}

impl FootprintLibrarySection {
    pub fn by_name(&self, name: &str) -> Option<&LibraryFootprint> {
        self.footprints.iter().find(|f| f.name == name)
    }
    pub fn by_name_mut(&mut self, name: &str) -> Option<&mut LibraryFootprint> {
        self.footprints.iter_mut().find(|f| f.name == name)
    }
    pub fn assign_missing_ids(&mut self) {
        for fp in &mut self.footprints {
            fp.assign_missing_ids();
        }
    }
}

// ---------- symbol library (the Symbol Editor's own content) ----------
//
// The Symbol Editor tab's editable library content -- same role for
// `crate::symbol::LibSymbol` that `FootprintLibrarySection` above plays for
// `crate::footprint::Footprint`: an *editable*, addressable (every graphic/
// pin has a stable `id`) copy of a symbol, materialized into
// `design.json` the first time it is opened for editing, with its own
// undo domain (`eda_ops::Domain::SymbolEditor`). `crate::symbol::LibSymbol`/
// `SymbolGraphic`/`LibPin` stay exactly as they are -- resolved, read-only
// geometry used by a dozen call sites outside this editor's scope
// (`schematic_json`, ERC, the exporter, the builtin table) -- rather than
// growing an `id`/`body_style` onto them directly, the same reasoning
// `LibraryPad`'s own doc gives for not touching `crate::footprint::Pad`.

/// Sorted by `lib_id`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SymbolLibrarySection {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub symbols: Vec<LibrarySymbol>,
}

impl SymbolLibrarySection {
    pub fn by_lib_id(&self, lib_id: &str) -> Option<&LibrarySymbol> {
        self.symbols.iter().find(|s| s.lib_id == lib_id)
    }
    pub fn by_lib_id_mut(&mut self, lib_id: &str) -> Option<&mut LibrarySymbol> {
        self.symbols.iter_mut().find(|s| s.lib_id == lib_id)
    }
    pub fn assign_missing_ids(&mut self) {
        for s in &mut self.symbols {
            s.assign_missing_ids();
        }
    }
}

/// `FILL_T` as the Shape Properties dialog actually offers it: KiCad's
/// `color`/hatch modes are real file values but not authorable from this
/// editor's own dialog -- the same simplification `crate::symbol::
/// SymbolGraphic`'s single `filled: bool` already makes for every other
/// caller, just with one more usable state (a filled-with-background-color
/// shape, e.g. a DeMorgan box, is common enough in real libraries to be
/// worth the real three-way choice here even though the engine's own type
/// only ever reads it as "filled or not" -- see `to_engine_graphic`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LibraryFill {
    #[default]
    None,
    Outline,
    Background,
}

fn d_body_style_one() -> u32 {
    1
}
fn is_body_style_one(b: &u32) -> bool {
    *b == 1
}
fn d_pin_shape() -> String {
    "line".to_string()
}
fn d_pin_name_offset_mm() -> f64 {
    0.508 // KiCad's own default (20 mil) -- `TEXT_OFFSET_RATIO` applied to the standard 1.27mm pin length.
}

/// One drawn primitive of a [`LibrarySymbol`] -- same shapes/fields as
/// `crate::symbol::SymbolGraphic`, plus a stable `id` (addressing, for
/// move/edit/delete -- see this section's own intro) and `body_style`
/// (KiCad's DeMorgan alternate; `SymbolGraphic` has no such field at all,
/// since no placed-instance renderer in this app resolves one today --
/// see [`LibrarySymbol::to_engine_symbol`]'s own doc on what that means
/// here). `unit`/`body_style` keep `SymbolGraphic`'s own "0 = shared by
/// every unit/style" convention.
///
/// Serialized as a `kind`-tagged object; *read* by the hand-written `Deserialize` below (see
/// [`LibrarySymbolGraphicWire`] for why serde's own tagged-enum reader is not used).
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum LibrarySymbolGraphic {
    Rectangle {
        #[serde(default, skip_serializing_if = "String::is_empty")]
        id: String,
        #[serde(default)]
        unit: u32,
        #[serde(default = "d_body_style_one", skip_serializing_if = "is_body_style_one")]
        body_style: u32,
        start: crate::symbol::SPoint,
        end: crate::symbol::SPoint,
        stroke_mm: f64,
        #[serde(default)]
        fill: LibraryFill,
    },
    Polyline {
        #[serde(default, skip_serializing_if = "String::is_empty")]
        id: String,
        #[serde(default)]
        unit: u32,
        #[serde(default = "d_body_style_one", skip_serializing_if = "is_body_style_one")]
        body_style: u32,
        pts: Vec<crate::symbol::SPoint>,
        stroke_mm: f64,
        #[serde(default)]
        fill: LibraryFill,
    },
    Circle {
        #[serde(default, skip_serializing_if = "String::is_empty")]
        id: String,
        #[serde(default)]
        unit: u32,
        #[serde(default = "d_body_style_one", skip_serializing_if = "is_body_style_one")]
        body_style: u32,
        center: crate::symbol::SPoint,
        radius_mm: f64,
        stroke_mm: f64,
        #[serde(default)]
        fill: LibraryFill,
    },
    Arc {
        #[serde(default, skip_serializing_if = "String::is_empty")]
        id: String,
        #[serde(default)]
        unit: u32,
        #[serde(default = "d_body_style_one", skip_serializing_if = "is_body_style_one")]
        body_style: u32,
        start: crate::symbol::SPoint,
        mid: crate::symbol::SPoint,
        end: crate::symbol::SPoint,
        stroke_mm: f64,
        #[serde(default)]
        fill: LibraryFill,
    },
    Text {
        #[serde(default, skip_serializing_if = "String::is_empty")]
        id: String,
        #[serde(default)]
        unit: u32,
        #[serde(default = "d_body_style_one", skip_serializing_if = "is_body_style_one")]
        body_style: u32,
        text: String,
        at: crate::symbol::SPoint,
        #[serde(default)]
        angle_deg: f64,
        size_mm: f64,
    },
}

/// The flat shape `LibrarySymbolGraphic` is read from: the `kind` tag, the fields every kind has, and one optional field for every field some kind
/// has. An internally tagged enum would read the object into serde's buffer first, to find the tag; with `serde_json`'s `arbitrary_precision`
/// feature -- which the `eda` build gets from `starlark` -- a fractional number held in that buffer comes back out as a map, "invalid type:
/// map, expected f64", so a `design.json` with a single graphic in a symbol of its library (all in mm) could not be loaded by the studio or the CLI.
/// Reading typed fields straight off the object has no buffer. The JSON is the same as ever, and a field that does not belong to the `kind`
/// is still refused (`deny_unknown_fields` on the enum did that).
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LibrarySymbolGraphicWire {
    kind: String,
    #[serde(default)]
    id: String,
    #[serde(default)]
    unit: u32,
    #[serde(default = "d_body_style_one")]
    body_style: u32,
    #[serde(default)]
    fill: Option<LibraryFill>,
    #[serde(default)]
    stroke_mm: Option<f64>,
    #[serde(default)]
    start: Option<crate::symbol::SPoint>,
    #[serde(default)]
    mid: Option<crate::symbol::SPoint>,
    #[serde(default)]
    end: Option<crate::symbol::SPoint>,
    #[serde(default)]
    center: Option<crate::symbol::SPoint>,
    #[serde(default)]
    radius_mm: Option<f64>,
    #[serde(default)]
    pts: Option<Vec<crate::symbol::SPoint>>,
    #[serde(default)]
    text: Option<String>,
    #[serde(default)]
    at: Option<crate::symbol::SPoint>,
    #[serde(default)]
    angle_deg: Option<f64>,
    #[serde(default)]
    size_mm: Option<f64>,
}

impl<'de> Deserialize<'de> for LibrarySymbolGraphic {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        use serde::de::Error;
        let w = LibrarySymbolGraphicWire::deserialize(d)?;
        let allowed: &'static [&'static str] = match w.kind.as_str() {
            "rectangle" => &["stroke_mm", "fill", "start", "end"],
            "polyline" => &["stroke_mm", "fill", "pts"],
            "circle" => &["stroke_mm", "fill", "center", "radius_mm"],
            "arc" => &["stroke_mm", "fill", "start", "mid", "end"],
            "text" => &["text", "at", "angle_deg", "size_mm"],
            other => return Err(D::Error::unknown_variant(other, &["rectangle", "polyline", "circle", "arc", "text"])),
        };
        let present = [
            ("stroke_mm", w.stroke_mm.is_some()),
            ("fill", w.fill.is_some()),
            ("start", w.start.is_some()),
            ("mid", w.mid.is_some()),
            ("end", w.end.is_some()),
            ("center", w.center.is_some()),
            ("radius_mm", w.radius_mm.is_some()),
            ("pts", w.pts.is_some()),
            ("text", w.text.is_some()),
            ("at", w.at.is_some()),
            ("angle_deg", w.angle_deg.is_some()),
            ("size_mm", w.size_mm.is_some()),
        ];
        for (name, there) in present {
            if there && !allowed.contains(&name) {
                return Err(D::Error::unknown_field(name, allowed));
            }
        }
        let LibrarySymbolGraphicWire { kind, id, unit, body_style, fill, stroke_mm, start, mid, end, center, radius_mm, pts, text, at, angle_deg, size_mm } = w;
        let need = |name: &'static str| D::Error::missing_field(name);
        let fill = fill.unwrap_or_default();
        Ok(match kind.as_str() {
            "rectangle" => LibrarySymbolGraphic::Rectangle { id, unit, body_style, start: start.ok_or_else(|| need("start"))?, end: end.ok_or_else(|| need("end"))?, stroke_mm: stroke_mm.ok_or_else(|| need("stroke_mm"))?, fill },
            "polyline" => LibrarySymbolGraphic::Polyline { id, unit, body_style, pts: pts.ok_or_else(|| need("pts"))?, stroke_mm: stroke_mm.ok_or_else(|| need("stroke_mm"))?, fill },
            "circle" => LibrarySymbolGraphic::Circle { id, unit, body_style, center: center.ok_or_else(|| need("center"))?, radius_mm: radius_mm.ok_or_else(|| need("radius_mm"))?, stroke_mm: stroke_mm.ok_or_else(|| need("stroke_mm"))?, fill },
            "arc" => LibrarySymbolGraphic::Arc {
                id,
                unit,
                body_style,
                start: start.ok_or_else(|| need("start"))?,
                mid: mid.ok_or_else(|| need("mid"))?,
                end: end.ok_or_else(|| need("end"))?,
                stroke_mm: stroke_mm.ok_or_else(|| need("stroke_mm"))?,
                fill,
            },
            _ => LibrarySymbolGraphic::Text { id, unit, body_style, text: text.ok_or_else(|| need("text"))?, at: at.ok_or_else(|| need("at"))?, angle_deg: angle_deg.unwrap_or_default(), size_mm: size_mm.ok_or_else(|| need("size_mm"))? },
        })
    }
}

impl LibrarySymbolGraphic {
    pub fn id(&self) -> &str {
        use LibrarySymbolGraphic::*;
        match self {
            Rectangle { id, .. } | Polyline { id, .. } | Circle { id, .. } | Arc { id, .. } | Text { id, .. } => id,
        }
    }
    pub fn set_id(&mut self, new_id: String) {
        use LibrarySymbolGraphic::*;
        match self {
            Rectangle { id, .. } | Polyline { id, .. } | Circle { id, .. } | Arc { id, .. } | Text { id, .. } => *id = new_id,
        }
    }
    pub fn unit(&self) -> u32 {
        use LibrarySymbolGraphic::*;
        match self {
            Rectangle { unit, .. } | Polyline { unit, .. } | Circle { unit, .. } | Arc { unit, .. } | Text { unit, .. } => *unit,
        }
    }
    pub fn body_style(&self) -> u32 {
        use LibrarySymbolGraphic::*;
        match self {
            Rectangle { body_style, .. } | Polyline { body_style, .. } | Circle { body_style, .. } | Arc { body_style, .. } | Text { body_style, .. } => *body_style,
        }
    }
    pub fn set_body_style(&mut self, style: u32) {
        use LibrarySymbolGraphic::*;
        match self {
            Rectangle { body_style, .. } | Polyline { body_style, .. } | Circle { body_style, .. } | Arc { body_style, .. } | Text { body_style, .. } => *body_style = style,
        }
    }
    /// Every point this graphic touches, local mm -- for `assign_missing_ids`'s seed and a move/translate.
    pub fn points(&self) -> Vec<crate::symbol::SPoint> {
        use LibrarySymbolGraphic::*;
        match self {
            Rectangle { start, end, .. } => vec![*start, *end],
            Polyline { pts, .. } => pts.clone(),
            Circle { center, .. } => vec![*center],
            Arc { start, mid, end, .. } => vec![*start, *mid, *end],
            Text { at, .. } => vec![*at],
        }
    }
    pub fn translate(&mut self, dx_mm: f64, dy_mm: f64) {
        use LibrarySymbolGraphic::*;
        let t = |p: &mut crate::symbol::SPoint| {
            p.x += dx_mm;
            p.y += dy_mm;
        };
        match self {
            Rectangle { start, end, .. } => {
                t(start);
                t(end);
            }
            Polyline { pts, .. } => pts.iter_mut().for_each(t),
            Circle { center, .. } => t(center),
            Arc { start, mid, end, .. } => {
                t(start);
                t(mid);
                t(end);
            }
            Text { at, .. } => t(at),
        }
    }
    /// The same graphic as `crate::symbol::SymbolGraphic` -- drops `id`
    /// and `body_style` (see this type's own doc; a caller that cares
    /// about body style, i.e. `LibrarySymbol::to_engine_symbol`, filters
    /// by it *before* calling this) and collapses `fill` down to the
    /// engine's own `filled: bool` (`Background`/`Outline` both read as
    /// `true` -- a real difference in real KiCad rendering this port's
    /// engine-side type has never modeled, same approximation
    /// `LibraryPad::to_engine_pad`'s shape collapse already documents).
    pub fn to_engine_graphic(&self) -> crate::symbol::SymbolGraphic {
        use crate::symbol::SymbolGraphic as EG;
        let filled = |f: LibraryFill| f != LibraryFill::None;
        match self {
            LibrarySymbolGraphic::Rectangle { unit, start, end, stroke_mm, fill, .. } => EG::Rectangle { unit: *unit, start: *start, end: *end, stroke_mm: *stroke_mm, filled: filled(*fill) },
            LibrarySymbolGraphic::Polyline { unit, pts, stroke_mm, fill, .. } => EG::Polyline { unit: *unit, pts: pts.clone(), stroke_mm: *stroke_mm, filled: filled(*fill) },
            LibrarySymbolGraphic::Circle { unit, center, radius_mm, stroke_mm, fill, .. } => EG::Circle { unit: *unit, center: *center, radius_mm: *radius_mm, stroke_mm: *stroke_mm, filled: filled(*fill) },
            LibrarySymbolGraphic::Arc { unit, start, mid, end, stroke_mm, fill, .. } => EG::Arc { unit: *unit, start: *start, mid: *mid, end: *end, stroke_mm: *stroke_mm, filled: filled(*fill) },
            LibrarySymbolGraphic::Text { unit, text, at, angle_deg, size_mm, .. } => EG::Text { unit: *unit, text: text.clone(), at: *at, angle_deg: *angle_deg, size_mm: *size_mm },
        }
    }
    /// The reverse of `to_engine_graphic`, for materializing an editable
    /// copy of an already-resolved `crate::symbol::LibSymbol` -- `id`
    /// starts empty (backfilled by `assign_missing_ids`), `body_style`
    /// always 1 (the engine type has no alternate-style concept to read
    /// one back from), `fill` is `None`/`Background` (the engine's
    /// `filled` has no "outline-only" state of its own to distinguish --
    /// an edit that re-saves an opened symbol unchanged keeps whichever
    /// of those two this maps to, a real but narrow fidelity gap, see
    /// PARITY-symedit.md).
    pub fn from_engine_graphic(g: &crate::symbol::SymbolGraphic) -> Self {
        use crate::symbol::SymbolGraphic as EG;
        let fill = |f: bool| if f { LibraryFill::Background } else { LibraryFill::None };
        match g {
            EG::Rectangle { unit, start, end, stroke_mm, filled } => LibrarySymbolGraphic::Rectangle { id: String::new(), unit: *unit, body_style: 1, start: *start, end: *end, stroke_mm: *stroke_mm, fill: fill(*filled) },
            EG::Polyline { unit, pts, stroke_mm, filled } => LibrarySymbolGraphic::Polyline { id: String::new(), unit: *unit, body_style: 1, pts: pts.clone(), stroke_mm: *stroke_mm, fill: fill(*filled) },
            EG::Circle { unit, center, radius_mm, stroke_mm, filled } => LibrarySymbolGraphic::Circle { id: String::new(), unit: *unit, body_style: 1, center: *center, radius_mm: *radius_mm, stroke_mm: *stroke_mm, fill: fill(*filled) },
            EG::Arc { unit, start, mid, end, stroke_mm, filled } => LibrarySymbolGraphic::Arc { id: String::new(), unit: *unit, body_style: 1, start: *start, mid: *mid, end: *end, stroke_mm: *stroke_mm, fill: fill(*filled) },
            EG::Text { unit, text, at, angle_deg, size_mm } => LibrarySymbolGraphic::Text { id: String::new(), unit: *unit, body_style: 1, text: text.clone(), at: *at, angle_deg: *angle_deg, size_mm: *size_mm },
        }
    }
}

/// One pin of a [`LibrarySymbol`] -- same fields as `crate::symbol::
/// LibPin`, plus `id` (addressing), `body_style`, and the Pin Properties
/// dialog's own name/number text-size fields (`LibPin` has none of these
/// three -- editor-only metadata, same split `LibraryPad`'s own doc
/// explains for its own overrides).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LibrarySymbolPin {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub id: String,
    pub number: String,
    #[serde(default)]
    pub name: String,
    /// Same raw-string convention as `crate::symbol::LibPin::electrical_type`
    /// (KiCad's own token, not a closed Rust enum) -- see that field's doc.
    pub electrical_type: String,
    #[serde(default = "d_pin_shape")]
    pub shape: String,
    pub at: crate::symbol::SPoint,
    pub angle_deg: f64,
    pub length_mm: f64,
    #[serde(default = "d_unit_one", skip_serializing_if = "is_unit_one")]
    pub unit: u32,
    #[serde(default = "d_body_style_one", skip_serializing_if = "is_body_style_one")]
    pub body_style: u32,
    /// `(hide yes)` -- see `crate::symbol::LibPin`'s own doc (this app's
    /// read-only resolved type has no such field at all; this editor is
    /// the only place it is modeled, same as a footprint pad's clearance
    /// overrides).
    #[serde(default)]
    pub hidden: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name_size_mm: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub number_size_mm: Option<f64>,
}

impl LibrarySymbolPin {
    fn id_seed(&self) -> String {
        format!("{}|{}|{:.4},{:.4}", self.unit, self.number, self.at.x, self.at.y)
    }
    /// The same pin as `crate::symbol::LibPin` -- drops `id`/`hidden`/
    /// `body_style`/the text-size overrides (see this type's own doc).
    pub fn to_engine_pin(&self) -> crate::symbol::LibPin {
        crate::symbol::LibPin { number: self.number.clone(), name: self.name.clone(), electrical_type: self.electrical_type.clone(), shape: self.shape.clone(), at: self.at, angle_deg: self.angle_deg, length_mm: self.length_mm, unit: self.unit }
    }
    /// The reverse of `to_engine_pin` -- `id` empty (backfilled), `body_style`
    /// 1, `hidden`/text sizes at their defaults (the engine type never had
    /// them to read back).
    pub fn from_engine_pin(p: &crate::symbol::LibPin) -> Self {
        LibrarySymbolPin { id: String::new(), number: p.number.clone(), name: p.name.clone(), electrical_type: p.electrical_type.clone(), shape: p.shape.clone(), at: p.at, angle_deg: p.angle_deg, length_mm: p.length_mm, unit: p.unit, body_style: 1, hidden: false, name_size_mm: None, number_size_mm: None }
    }
}

/// A symbol definition as the Symbol Editor shows/edits it -- the Symbol
/// Editor tab's own "project symbol library section in the IR" (see this
/// section's own intro). `lib_id` is both the addressing key (like
/// `LibraryFootprint::name`) and the string a `SymbolInstance`/
/// `PowerSymbol::lib_id` names: `"Lib:Name"` when opened from a real/
/// builtin library or from an already-placed instance, or a bare/`"eda:
/// <ref>"`-prefixed name for one authored from scratch or opened from an
/// instance with no resolvable library symbol (see `Cmd::OpenSymbolForEdit`'s
/// own doc).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LibrarySymbol {
    pub lib_id: String,
    /// The library's "Reference" field default ("R", "U", "#PWR", ...).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub reference_prefix: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub keywords: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub datasheet: String,
    /// `dialog_lib_symbol_properties.cpp`'s "Define as power symbol".
    #[serde(default)]
    pub power: bool,
    #[serde(default = "d_true")]
    pub in_bom: bool,
    #[serde(default = "d_true")]
    pub on_board: bool,
    #[serde(default)]
    pub pin_numbers_hidden: bool,
    #[serde(default)]
    pub pin_names_hidden: bool,
    #[serde(default = "d_pin_name_offset_mm")]
    pub pin_name_offset_mm: f64,
    /// How many units this symbol declares (an op-amp's 4 gates, say) --
    /// same meaning as `crate::symbol::LibSymbol::unit_count`.
    #[serde(default = "d_unit_one", skip_serializing_if = "is_unit_one")]
    pub unit_count: u32,
    /// KiCad's DeMorgan alternate body style -- whether a second
    /// (`body_style == 2`) set of graphics/pins exists at all. See
    /// `to_engine_symbol`'s own doc for the real, documented gap in how
    /// far this app's placed-instance rendering follows it.
    #[serde(default)]
    pub has_alternate_body_style: bool,
    /// `dialog_lib_symbol_properties.cpp`'s Footprint Filters list (glob
    /// patterns a footprint-chooser would narrow to for this symbol) --
    /// stored for round-tripping through `.kicad_sym`; nothing in this
    /// app's own footprint resolution reads it yet.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub footprint_filters: Vec<String>,
    /// Sorted by `id`.
    #[serde(default)]
    pub graphics: Vec<LibrarySymbolGraphic>,
    /// Sorted by `id`.
    #[serde(default)]
    pub pins: Vec<LibrarySymbolPin>,
    /// Whether a schematic symbol/power-symbol instance naming this
    /// `lib_id` should resolve its graphics/pins from here
    /// (`crate::board::load`'s overlay onto `ConstraintModel::symbols`,
    /// filtered on this flag) -- KiCad's "Update Symbol from Library",
    /// same explicit, user-triggered convention `LibraryFootprint::
    /// published`'s own doc describes. Starts `false`.
    #[serde(default)]
    pub published: bool,
}

impl Default for LibrarySymbol {
    fn default() -> Self {
        LibrarySymbol {
            lib_id: String::new(),
            reference_prefix: "U".to_string(),
            description: String::new(),
            keywords: String::new(),
            datasheet: String::new(),
            power: false,
            in_bom: true,
            on_board: true,
            pin_numbers_hidden: false,
            pin_names_hidden: false,
            pin_name_offset_mm: d_pin_name_offset_mm(),
            unit_count: 1,
            has_alternate_body_style: false,
            footprint_filters: Vec::new(),
            graphics: Vec::new(),
            pins: Vec::new(),
            published: false,
        }
    }
}

impl LibrarySymbol {
    /// A brand-new, empty symbol named `lib_id` -- KiCad's "New Symbol"
    /// and "Edit Symbol" on an unresolvable name are the same verb here,
    /// same precedent `LibraryFootprint::new_empty`'s own doc explains.
    pub fn new_empty(lib_id: impl Into<String>) -> Self {
        LibrarySymbol { lib_id: lib_id.into(), ..Default::default() }
    }

    /// An editable copy of an already-resolved engine `LibSymbol`
    /// (real library file, builtin table, or an already-placed instance's
    /// own resolution) -- what the Symbol Editor materializes the first
    /// time an edit touches a symbol not already in `SymbolLibrarySection`.
    pub fn from_engine_symbol(sym: &crate::symbol::LibSymbol) -> Self {
        // A symbol with an alternate body: what both bodies draw alike (an item equal in the two) is shared (`body_style` 0, KiCad's `Name_<unit>_0`
        // sub-block), the rest belongs to its own style -- the reverse of what `to_engine_symbol` does.
        let (mut graphics, mut pins): (Vec<LibrarySymbolGraphic>, Vec<LibrarySymbolPin>) = (Vec::new(), Vec::new());
        let (normal, alternate) = sym.bodies();
        match alternate {
            None => {
                graphics.extend(normal.0.iter().map(LibrarySymbolGraphic::from_engine_graphic));
                pins.extend(normal.1.iter().map(LibrarySymbolPin::from_engine_pin));
            }
            Some(alt) => {
                for (list, other, style) in [(normal.0, alt.0, 1u32), (alt.0, normal.0, 2u32)] {
                    for g in list {
                        let shared = other.contains(g);
                        if shared && style == 2 {
                            continue;
                        }
                        let mut item = LibrarySymbolGraphic::from_engine_graphic(g);
                        item.set_body_style(if shared { 0 } else { style });
                        graphics.push(item);
                    }
                }
                for (list, other, style) in [(normal.1, alt.1, 1u32), (alt.1, normal.1, 2u32)] {
                    for p in list {
                        let shared = other.contains(p);
                        if shared && style == 2 {
                            continue;
                        }
                        let mut item = LibrarySymbolPin::from_engine_pin(p);
                        item.body_style = if shared { 0 } else { style };
                        pins.push(item);
                    }
                }
            }
        }
        LibrarySymbol {
            lib_id: sym.lib_id.clone(),
            reference_prefix: sym.reference_prefix.clone(),
            description: sym.description.clone(),
            keywords: String::new(),
            datasheet: sym.datasheet.clone(),
            power: sym.power,
            in_bom: sym.in_bom,
            on_board: sym.on_board,
            pin_names_hidden: sym.pin_names_hidden,
            pin_numbers_hidden: sym.pin_numbers_hidden,
            pin_name_offset_mm: sym.pin_name_offset_mm,
            unit_count: sym.unit_count.max(1),
            has_alternate_body_style: sym.alternate.is_some(),
            graphics,
            pins,
            ..Default::default()
        }
    }

    /// The reverse of `from_engine_symbol`, and the basis for both
    /// `board::load`'s publish overlay and the derived `.kicad_sym` export:
    /// this symbol's graphics/pins as the engine's own `LibSymbol`.
    ///
    /// The normal body style is the items of `body_style` 0 (shared) and 1; a symbol that declares an alternate one (`has_alternate_body_style`)
    /// also gets [`crate::symbol::LibSymbol::alternate`], the shared items and those of `body_style` 2, which is what a placed symbol in body style 2
    /// draws and is written with (`LIB_SYMBOL::GetBodyStyleCount`). A symbol that declares none keeps no alternate even if a stray item is of
    /// `body_style` 2: KiCad would not show it either.
    pub fn to_engine_symbol(&self) -> crate::symbol::LibSymbol {
        let alternate = self.has_alternate_body_style.then(|| {
            Box::new(crate::symbol::AlternateBody::new(
                self.graphics.iter().filter(|g| g.body_style() == 0 || g.body_style() == 2).map(|g| g.to_engine_graphic()).collect(),
                self.pins.iter().filter(|p| p.body_style == 0 || p.body_style == 2).map(|p| p.to_engine_pin()).collect(),
            ))
        });
        crate::symbol::LibSymbol {
            lib_id: self.lib_id.clone(),
            graphics: self.graphics.iter().filter(|g| g.body_style() <= 1).map(|g| g.to_engine_graphic()).collect(),
            pins: self.pins.iter().filter(|p| p.body_style <= 1).map(|p| p.to_engine_pin()).collect(),
            power: self.power,
            in_bom: self.in_bom,
            on_board: self.on_board,
            datasheet: self.datasheet.clone(),
            description: self.description.clone(),
            reference_prefix: self.reference_prefix.clone(),
            unit_count: self.unit_count.max(1),
            pin_names_hidden: self.pin_names_hidden,
            pin_numbers_hidden: self.pin_numbers_hidden,
            pin_name_offset_mm: self.pin_name_offset_mm,
            alternate,
        }
    }

    /// Assign a deterministic id to every pin/graphic whose `id` is still
    /// empty -- same contract as `FootprintLibrarySection`'s own (stable
    /// per-kind order, content-hash id, `_2`/`_3`... on an exact-content
    /// collision).
    pub fn assign_missing_ids(&mut self) {
        let mut existing: std::collections::BTreeSet<String> = self.pins.iter().map(|p| p.id.clone()).filter(|s| !s.is_empty()).collect();
        existing.extend(self.graphics.iter().map(|g| g.id().to_string()).filter(|s| !s.is_empty()));

        let mut order: Vec<usize> = (0..self.pins.len()).collect();
        order.sort_by(|&a, &b| (self.pins[a].unit, &self.pins[a].number, self.pins[a].at.x, self.pins[a].at.y).partial_cmp(&(self.pins[b].unit, &self.pins[b].number, self.pins[b].at.x, self.pins[b].at.y)).unwrap_or(std::cmp::Ordering::Equal));
        for i in order {
            if self.pins[i].id.is_empty() {
                let id = next_item_id("pin", &self.pins[i].id_seed(), &existing);
                existing.insert(id.clone());
                self.pins[i].id = id;
            }
        }

        let mut order: Vec<usize> = (0..self.graphics.len()).collect();
        order.sort_by(|&a, &b| {
            let ka = (self.graphics[a].unit(), self.graphics[a].body_style(), self.graphics[a].points().first().map(|p| (p.x, p.y)));
            let kb = (self.graphics[b].unit(), self.graphics[b].body_style(), self.graphics[b].points().first().map(|p| (p.x, p.y)));
            ka.partial_cmp(&kb).unwrap_or(std::cmp::Ordering::Equal)
        });
        for i in order {
            if self.graphics[i].id().is_empty() {
                let pts: Vec<String> = self.graphics[i].points().iter().map(|p| format!("{:.4},{:.4}", p.x, p.y)).collect();
                let seed = format!("{}|{}|{}", self.graphics[i].unit(), self.graphics[i].body_style(), pts.join(";"));
                let id = next_item_id("sym", &seed, &existing);
                existing.insert(id.clone());
                self.graphics[i].set_id(id);
            }
        }
    }

    /// `FOOTPRINT::GetNextPadNumber`'s pin-number analogue
    /// (`SYMBOL_EDITOR_PIN_TOOL`'s own auto-increment while placing pins
    /// in sequence): split `last` into its non-numeric prefix and
    /// trailing integer, increment until `prefix+integer` collides with
    /// no existing pin number *on any unit* of this symbol (KiCad numbers
    /// pins uniquely across the whole part by default; a deliberately
    /// interchangeable multi-unit symbol that wants the same number
    /// reused per unit is not this function's concern -- the dialog can
    /// still type one by hand).
    pub fn next_pin_number_after(&self, last: &str) -> String {
        let digit_count = last.chars().rev().take_while(|c| c.is_ascii_digit()).count();
        let split_at = last.len() - digit_count;
        let prefix = &last[..split_at];
        let mut num: u64 = last[split_at..].parse().unwrap_or(0);
        let used: std::collections::BTreeSet<&str> = self.pins.iter().map(|p| p.number.as_str()).collect();
        loop {
            num += 1;
            let candidate = format!("{prefix}{num}");
            if !used.contains(candidate.as_str()) {
                return candidate;
            }
        }
    }

    /// `next_pin_number_after`, seeded from this symbol's own
    /// highest-numbered existing pin -- `"1"` for the first pin on a
    /// blank symbol, same convention `next_pad_number`'s own doc explains.
    pub fn next_pin_number(&self) -> String {
        let last = self
            .pins
            .iter()
            .max_by_key(|p| {
                let n = p.number.as_str();
                let digit_count = n.chars().rev().take_while(|c| c.is_ascii_digit()).count();
                n[n.len() - digit_count..].parse::<u64>().unwrap_or(0)
            })
            .map(|p| p.number.as_str())
            .unwrap_or("0");
        self.next_pin_number_after(last)
    }
}

/// Deterministic, dependency-free 64-bit hash (FNV-1a) of a byte string,
/// hex-encoded. Not cryptographic, and deliberately not
/// `std::hash::DefaultHasher`: the standard library does not promise that
/// hasher's algorithm stays the same across releases, and an id that moved
/// because the toolchain changed would break every caller holding one.
/// FNV-1a is a fixed, tiny algorithm we own outright.
fn fnv1a_hex(bytes: &[u8]) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{h:016x}")
}

/// A short, stable id (`<prefix>_<12 hex chars>`) from a content hash of
/// `seed`, deduplicated against `existing` by appending `_2`, `_3`, ... when
/// two items hash the same (an exact duplicate, e.g. a hand-add repeated
/// verbatim) -- so ids are always unique within one design, and, given the
/// same seed and the same existing set, always the same.
///
/// Public so a verb that makes several linked items at once (a pasted group and its members, a duplicated footprint and its
/// copper) can name them all before it inserts any.
pub fn next_item_id(prefix: &str, seed: &str, existing: &std::collections::BTreeSet<String>) -> String {
    let base = format!("{prefix}_{}", &fnv1a_hex(seed.as_bytes())[..12]);
    if !existing.contains(&base) {
        return base;
    }
    let mut n = 2u32;
    loop {
        let candidate = format!("{base}_{n}");
        if !existing.contains(&candidate) {
            return candidate;
        }
        n += 1;
    }
}

impl Design {
    /// Canonical bytes: sorted vectors, then compact JSON. Hash these.
    pub fn canonical_bytes(&self) -> serde_json::Result<Vec<u8>> {
        let mut d = self.clone();
        if let Some(s) = &mut d.schematic {
            s.symbols.sort_by(|a, b| a.id.cmp(&b.id));
            s.wires.sort_by(|a, b| (&a.net, a.pts.first()).cmp(&(&b.net, b.pts.first())));
            s.labels.sort_by(|a, b| (&a.net, a.at).cmp(&(&b.net, b.at)));
            s.power_symbols.sort_by(|a, b| a.id.cmp(&b.id));
            s.no_connects.sort_by(|a, b| a.at.cmp(&b.at));
            s.sheets.sort_by(|a, b| a.name.cmp(&b.name));
            s.extras.canonicalize();
        }
        if let Some(p) = &mut d.placement {
            p.footprints.sort_by(|a, b| a.id.cmp(&b.id));
        }
        if let Some(r) = &mut d.routing {
            r.tracks
                .sort_by(|a, b| (&a.net, &a.layer, a.pts.first()).cmp(&(&b.net, &b.layer, b.pts.first())));
            r.vias.sort_by(|a, b| (&a.net, a.at).cmp(&(&b.net, b.at)));
            r.zones.sort_by(|a, b| (&a.net, &a.layer).cmp(&(&b.net, &b.layer)));
        }
        if let Some(dr) = &mut d.drawings {
            dr.shapes.sort_by(|a, b| a.id().cmp(b.id()));
            dr.texts.sort_by(|a, b| a.id.cmp(&b.id));
        }
        if let Some(lib) = &mut d.footprint_library {
            lib.footprints.sort_by(|a, b| a.name.cmp(&b.name));
            for fp in &mut lib.footprints {
                fp.pads.sort_by(|a, b| a.id.cmp(&b.id));
                fp.graphics.sort_by(|a, b| a.id().cmp(b.id()));
                fp.texts.sort_by(|a, b| a.id.cmp(&b.id));
            }
        }
        serde_json::to_vec(&d)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Design {
        Design {
            schema: 1,
            provenance: Provenance {
                engine_version: "0.1.0".into(),
                intent_hash: "b3a9".into(),
                seed: 7,
                stage_hashes: vec![],
            },
            schematic: Some(SchematicSection {
                symbols: vec![
                    SymbolInstance { id: "U1".into(), at: Point { x: 50_800, y: 63_500 }, rot: 0, mirrored: false, mirror_y: false, lib_id: String::new(), unit: 1, value: String::new(), footprint: String::new(), datasheet: String::new() , dnp: false, exclude_from_bom: false, exclude_from_board: false, exclude_from_sim: false },
                    SymbolInstance { id: "C1".into(), at: Point { x: 38_100, y: 63_500 }, rot: 90_000, mirrored: false, mirror_y: false, lib_id: String::new(), unit: 1, value: String::new(), footprint: String::new(), datasheet: String::new() , dnp: false, exclude_from_bom: false, exclude_from_board: false, exclude_from_sim: false },
                ],
                wires: vec![Wire { id: String::new(), net: "VIN".into(), pins: vec!["U1.3".into(), "C1.1".into()], pts: vec![Point { x: 35_000, y: 60_000 }, Point { x: 48_000, y: 60_000 }], bus: false }],
                labels: vec![],
                texts: vec![],
                power_symbols: vec![],
                no_connects: vec![],
                bus_entries: vec![],
                erc_exclusions: vec![], erc_pin_map: None, user_fields: Default::default(), field_layout: Default::default(), imported_from_kicad: false,
                title_block: None,
                sheets: vec![],
                instance_overrides: vec![],
                junctions: vec![],
                lines: vec![], extras: Default::default(),
            }),
            nets: None,
            placement: None,
            routing: None,
            drawings: None,
            footprint_library: None,
            sheet_contents: None,
            bus_aliases: vec![], symbol_library: None,
        }
    }

    #[test]
    fn roundtrip() {
        let d = sample();
        let json = serde_json::to_string(&d).unwrap();
        let back: Design = serde_json::from_str(&json).unwrap();
        assert_eq!(back.schematic.as_ref().unwrap().symbols.len(), 2);
    }

    #[test]
    fn canonical_is_order_independent() {
        let d1 = sample();
        let mut d2 = sample();
        d2.schematic.as_mut().unwrap().symbols.reverse();
        assert_eq!(d1.canonical_bytes().unwrap(), d2.canonical_bytes().unwrap());
    }

    #[test]
    fn unknown_fields_rejected() {
        let json = r#"{"schema":1,"provenance":{"engine_version":"0","intent_hash":"x","seed":0},"bogus":1}"#;
        assert!(serde_json::from_str::<Design>(json).is_err());
    }

    #[test]
    fn empty_sections_omitted() {
        let mut d = sample();
        d.schematic = None;
        let json = serde_json::to_string(&d).unwrap();
        assert!(!json.contains("schematic"));
    }

    // -------------------------------------------------------- item ids

    fn track(net: &str, layer: &str, pts: &[(Um, Um)]) -> Track {
        Track { id: String::new(), net: net.into(), pins: vec![], layer: layer.into(), width: 200, pts: pts.iter().map(|&(x, y)| Point { x, y }).collect(), arc_mid_offset: None }
    }
    fn via(net: &str, x: Um, y: Um) -> Via {
        Via { id: String::new(), net: net.into(), at: Point { x, y }, drill: 300, diameter: 600, from_layer: "F.Cu".into(), to_layer: "B.Cu".into() }
    }

    /// A `design.json` written before ids existed: no `id` key on any
    /// track/via. It must still load (old field simply absent, `#[serde(default)]`
    /// fills empty), and a normalization pass must then back-fill ids
    /// deterministically and leave the rest of the document untouched.
    #[test]
    fn an_old_design_without_ids_loads_and_backfills_them() {
        let json = r#"{
            "schema": 1,
            "provenance": {"engine_version": "0", "intent_hash": "x", "seed": 0},
            "routing": {
                "tracks": [{"net": "GND", "layer": "F.Cu", "width": 200, "pts": [{"x": 0, "y": 0}, {"x": 1000, "y": 0}]}],
                "vias": [{"net": "GND", "at": {"x": 500, "y": 500}, "drill": 300, "diameter": 600, "from_layer": "F.Cu", "to_layer": "B.Cu"}],
                "zones": [{"net": "GND", "layer": "B.Cu", "outline": [{"x": 0, "y": 0}, {"x": 1000, "y": 0}, {"x": 1000, "y": 1000}]}]
            }
        }"#;
        let mut d: Design = serde_json::from_str(json).expect("an old design.json with no ids must still parse");
        let rt = d.routing.as_ref().unwrap();
        assert!(rt.tracks[0].id.is_empty());
        assert!(rt.vias[0].id.is_empty());
        assert!(rt.zones[0].id.is_empty());

        d.assign_missing_ids();
        let rt = d.routing.as_ref().unwrap();
        assert!(!rt.tracks[0].id.is_empty());
        assert!(!rt.vias[0].id.is_empty());
        assert!(!rt.zones[0].id.is_empty());
    }

    /// Same routing, assigned twice (independently, e.g. on two different
    /// machines opening the same old file) -> the same ids. And a second
    /// call on an already-assigned design changes nothing, so a load ->
    /// save -> load round trip is stable.
    #[test]
    fn id_assignment_is_deterministic_and_stable_on_resave() {
        let build = || RoutingSection { tracks: vec![track("GND", "F.Cu", &[(0, 0), (1000, 0)]), track("VCC", "F.Cu", &[(0, 0), (0, 1000)])], vias: vec![via("GND", 500, 500)], zones: vec![], track_width_presets: vec![], via_presets: vec![], teardrop_settings: Default::default() };

        let mut a = build();
        a.assign_missing_ids();
        let mut b = build();
        b.assign_missing_ids();
        assert_eq!(a.tracks[0].id, b.tracks[0].id);
        assert_eq!(a.tracks[1].id, b.tracks[1].id);
        assert_eq!(a.vias[0].id, b.vias[0].id);

        let before = a.clone();
        a.assign_missing_ids(); // second pass: must be a no-op
        assert_eq!(a.tracks[0].id, before.tracks[0].id);
        assert_eq!(a.tracks[1].id, before.tracks[1].id);
    }

    /// Ids come from content, not position in the array: reordering the
    /// same tracks must not change which id lands on which track.
    #[test]
    fn ids_do_not_depend_on_array_order() {
        let mut rt1 = RoutingSection { tracks: vec![track("GND", "F.Cu", &[(0, 0), (1000, 0)]), track("VCC", "F.Cu", &[(0, 0), (0, 1000)])], vias: vec![], zones: vec![], track_width_presets: vec![], via_presets: vec![], teardrop_settings: Default::default() };
        let mut rt2 = RoutingSection { tracks: vec![rt1.tracks[1].clone(), rt1.tracks[0].clone()], vias: vec![], zones: vec![], track_width_presets: vec![], via_presets: vec![], teardrop_settings: Default::default() };
        rt1.assign_missing_ids();
        rt2.assign_missing_ids();
        let gnd1 = rt1.tracks.iter().find(|t| t.net == "GND").unwrap();
        let gnd2 = rt2.tracks.iter().find(|t| t.net == "GND").unwrap();
        assert_eq!(gnd1.id, gnd2.id);
    }

    /// Two tracks that happen to be identical (a hand-add repeated
    /// verbatim) must still get two distinct ids.
    #[test]
    fn duplicate_content_gets_distinct_ids() {
        let mut rt = RoutingSection { tracks: vec![track("GND", "F.Cu", &[(0, 0), (1000, 0)]), track("GND", "F.Cu", &[(0, 0), (1000, 0)])], vias: vec![], zones: vec![], track_width_presets: vec![], via_presets: vec![], teardrop_settings: Default::default() };
        rt.assign_missing_ids();
        assert_ne!(rt.tracks[0].id, rt.tracks[1].id);
    }

    /// `SetTrackWidth`-style edits (anything that changes `width` but not
    /// `net`/`layer`/`pts`) must not change the id that was already
    /// assigned -- callers (the web UI, `DeleteTrack`) hold onto it.
    #[test]
    fn changing_width_after_assignment_does_not_move_the_id() {
        let mut rt = RoutingSection { tracks: vec![track("GND", "F.Cu", &[(0, 0), (1000, 0)])], vias: vec![], zones: vec![], track_width_presets: vec![], via_presets: vec![], teardrop_settings: Default::default() };
        rt.assign_missing_ids();
        let id = rt.tracks[0].id.clone();
        rt.tracks[0].width = 500; // what `SetTrackWidth` does
        assert_eq!(rt.tracks[0].id, id);
    }

    #[test]
    fn shape_and_text_ids_backfill_the_same_way() {
        let mut dr = DrawingsSection {
            shapes: vec![Shape::Segment { id: String::new(), layer: "F.SilkS".into(), stroke_width: 150, filled: false, start: Point { x: 0, y: 0 }, end: Point { x: 1000, y: 0 } }],
            texts: vec![Text { id: String::new(), content: "REV A".into(), at: Point { x: 0, y: 0 }, angle: 0, layer: "F.SilkS".into(), size_um: 1000, stroke_width: 150, justify: TextJustify::Center, mirror: false }],
            ..Default::default()
        };
        dr.assign_missing_ids();
        assert!(!dr.shapes[0].id().is_empty());
        assert!(!dr.texts[0].id.is_empty());
        assert!(dr.shapes[0].id().starts_with("shp_"));
        assert!(dr.texts[0].id.starts_with("txt_"));
    }

    #[test]
    fn shape_translate_moves_every_point() {
        let mut s = Shape::Rect { id: "shp_1".into(), layer: "F.SilkS".into(), stroke_width: 100, filled: false, start: Point { x: 0, y: 0 }, end: Point { x: 100, y: 100 } };
        s.translate(10, 20);
        assert_eq!(s.points(), vec![Point { x: 10, y: 20 }, Point { x: 110, y: 120 }]);
    }

    // -------------------------------------------------- footprint library

    fn lib_pad(number: &str, x: Um, y: Um) -> LibraryPad {
        LibraryPad {
            id: String::new(),
            number: number.into(),
            at: Point { x, y },
            offset: Point::default(),
            size: (1000, 1000),
            shape: LibraryPadShape::RoundRect,
            kind: crate::footprint::PadKind::Smd,
            drill: None,
            drill_slot: None,
            rot: 0,
            roundrect_ratio: Some(0.25),
            trapezoid_delta: None,
            chamfer_ratio: None,
            chamfer_corners: ChamferCorners::default(),
            layers: vec!["F.Cu".into()],
            clearance_override: None,
            thermal_gap_override: None,
            thermal_spoke_width_override: None,
            zone_connection: None,
            thermal_spoke_angle_mdeg: None,
        }
    }

    #[test]
    fn footprint_library_pad_ids_are_assigned_and_stable() {
        let mut fp = LibraryFootprint::new_empty("Test:Lib");
        fp.pads = vec![lib_pad("1", 0, 0), lib_pad("2", 1000, 0)];
        fp.assign_missing_ids();
        assert!(fp.pads.iter().all(|p| !p.id.is_empty()), "{:?}", fp.pads);
        assert_ne!(fp.pads[0].id, fp.pads[1].id);
        let ids_before: Vec<String> = fp.pads.iter().map(|p| p.id.clone()).collect();
        fp.assign_missing_ids(); // a second pass must be a no-op
        let ids_after: Vec<String> = fp.pads.iter().map(|p| p.id.clone()).collect();
        assert_eq!(ids_before, ids_after);
    }

    #[test]
    fn next_pad_number_increments_and_skips_collisions() {
        let empty = LibraryFootprint::new_empty("Test:Lib");
        assert_eq!(empty.next_pad_number(), "1");

        let mut fp = LibraryFootprint::new_empty("Test:Lib");
        fp.pads = vec![lib_pad("1", 0, 0), lib_pad("2", 1000, 0)];
        assert_eq!(fp.next_pad_number(), "3");

        // A manually-added "4" alongside a hole left at "3" must still
        // skip the now-occupied "4" when continuing from "3".
        fp.pads.push(lib_pad("4", 2000, 0));
        assert_eq!(fp.next_pad_number_after("3"), "5");
    }

    #[test]
    fn library_pad_round_trips_through_engine_pad_for_simple_shapes() {
        let p = lib_pad("1", 500, -500);
        let engine = p.to_engine_pad();
        assert_eq!(engine.number, "1");
        assert_eq!(engine.at, (500, -500));
        assert_eq!(engine.shape, crate::footprint::PadShape::RoundRect);
        assert_eq!(engine.roundrect_ratio, Some(0.25));

        let back = LibraryPad::from_engine_pad(&engine);
        assert_eq!(back.number, engine.number);
        assert_eq!(back.at, Point { x: 500, y: -500 });
        assert_eq!(back.shape, LibraryPadShape::RoundRect);
        assert!(back.id.is_empty(), "from_engine_pad must leave id for the caller to assign");
    }

    #[test]
    fn trapezoid_and_chamfered_pads_approximate_down_for_the_engine() {
        let mut trap = lib_pad("1", 0, 0);
        trap.shape = LibraryPadShape::Trapezoid;
        trap.trapezoid_delta = Some((200, 0));
        assert_eq!(trap.to_engine_pad().shape, crate::footprint::PadShape::Rect);

        let mut chf = lib_pad("1", 0, 0);
        chf.shape = LibraryPadShape::ChamferedRect;
        chf.chamfer_ratio = Some(0.2);
        chf.roundrect_ratio = None;
        let engine = chf.to_engine_pad();
        assert_eq!(engine.shape, crate::footprint::PadShape::RoundRect);
        assert_eq!(engine.roundrect_ratio, Some(0.2), "chamfer ratio stands in for roundrect ratio in the engine approximation");
    }

    #[test]
    fn footprint_library_round_trips_through_engine_footprint() {
        let engine = crate::footprint::builtin("0603").unwrap();
        let lib = LibraryFootprint::from_engine_footprint(&engine);
        assert_eq!(lib.pads.len(), engine.pads.len());
        assert!(lib.pads.iter().all(|p| p.id.is_empty()));
        assert!(!lib.published, "opening a footprint must never auto-publish it to the board");
        let mut lib = lib;
        lib.assign_missing_ids();
        let back = lib.to_engine_footprint();
        assert_eq!(back.pads.len(), engine.pads.len());
        assert_eq!(back.courtyard, engine.courtyard);
    }

    #[test]
    fn footprint_library_section_is_additive_on_an_old_design() {
        // A `design.json` written before this editor existed has no
        // `footprint_library` key at all; it must still load, and a
        // normalization pass must not invent one out of thin air.
        let json = r#"{"schema": 1, "provenance": {"engine_version": "0", "intent_hash": "x", "seed": 0}}"#;
        let mut d: Design = serde_json::from_str(json).expect("an old design.json with no footprint_library must still parse");
        assert!(d.footprint_library.is_none());
        d.assign_missing_ids();
        assert!(d.footprint_library.is_none(), "assign_missing_ids must not create a section that was never there");
    }

    // ---------------------------------------------------- symbol library

    fn lib_pin(number: &str, unit: u32, x: f64, y: f64) -> LibrarySymbolPin {
        LibrarySymbolPin {
            id: String::new(),
            number: number.into(),
            name: String::new(),
            electrical_type: "passive".into(),
            shape: "line".into(),
            at: crate::symbol::SPoint::new(x, y),
            angle_deg: 270.0,
            length_mm: 2.54,
            unit,
            body_style: 1,
            hidden: false,
            name_size_mm: None,
            number_size_mm: None,
        }
    }

    #[test]
    fn symbol_library_pin_and_graphic_ids_are_assigned_and_stable() {
        let mut sym = LibrarySymbol::new_empty("Test:Lib");
        sym.pins = vec![lib_pin("1", 1, 0.0, 3.81), lib_pin("2", 1, 0.0, -3.81)];
        sym.graphics = vec![LibrarySymbolGraphic::Rectangle { id: String::new(), unit: 1, body_style: 1, start: crate::symbol::SPoint::new(-1.0, -1.0), end: crate::symbol::SPoint::new(1.0, 1.0), stroke_mm: 0.254, fill: LibraryFill::None }];
        sym.assign_missing_ids();
        assert!(sym.pins.iter().all(|p| !p.id.is_empty()), "{:?}", sym.pins);
        assert!(sym.graphics.iter().all(|g| !g.id().is_empty()));
        assert_ne!(sym.pins[0].id, sym.pins[1].id);
        let ids_before: Vec<String> = sym.pins.iter().map(|p| p.id.clone()).collect();
        sym.assign_missing_ids(); // a second pass must be a no-op
        let ids_after: Vec<String> = sym.pins.iter().map(|p| p.id.clone()).collect();
        assert_eq!(ids_before, ids_after);
    }

    #[test]
    fn next_pin_number_increments_and_skips_collisions() {
        let empty = LibrarySymbol::new_empty("Test:Lib");
        assert_eq!(empty.next_pin_number(), "1");

        let mut sym = LibrarySymbol::new_empty("Test:Lib");
        sym.pins = vec![lib_pin("1", 1, 0.0, 3.81), lib_pin("2", 1, 0.0, -3.81)];
        assert_eq!(sym.next_pin_number(), "3");

        // A manually-added "4" alongside a hole left at "3" must still
        // skip the now-occupied "4" when continuing from "3".
        sym.pins.push(lib_pin("4", 1, 2.54, 0.0));
        assert_eq!(sym.next_pin_number_after("3"), "5");
    }

    #[test]
    fn symbol_library_round_trips_through_engine_symbol() {
        let engine = crate::symbol::builtin("Device:R").unwrap();
        let lib = LibrarySymbol::from_engine_symbol(&engine);
        assert_eq!(lib.pins.len(), engine.pins.len());
        assert!(lib.pins.iter().all(|p| p.id.is_empty()));
        assert!(!lib.published, "opening a symbol must never auto-publish it to the board");
        let mut lib = lib;
        lib.assign_missing_ids();
        let back = lib.to_engine_symbol();
        assert_eq!(back.pins.len(), engine.pins.len());
        assert_eq!(back.graphics.len(), engine.graphics.len());
        assert_eq!(back.reference_prefix, engine.reference_prefix);
    }

    #[test]
    fn symbol_library_to_engine_keeps_the_alternate_body_style_as_its_own_body() {
        // `to_engine_symbol` is the publish path a placed instance's rendering reads (`board::load`'s overlay). The normal body style (`body_style` 0 and 1)
        // is the symbol; the alternate one (0 and 2) is `LibSymbol::alternate`, which a placed symbol in style 2 draws -- never both at once.
        let mut sym = LibrarySymbol::new_empty("Test:Lib");
        sym.has_alternate_body_style = true;
        sym.pins = vec![lib_pin("1", 1, 0.0, 3.81), {
            let mut alt = lib_pin("1", 1, 0.0, 3.81);
            alt.body_style = 2;
            alt.at.x = 1.27;
            alt
        }];
        let back = sym.to_engine_symbol();
        assert_eq!(back.pins.len(), 1, "the normal body style publishes its own pins only");
        let alt = back.alternate.as_ref().expect("a symbol that declares an alternate body style has one");
        assert_eq!(alt.pins.len(), 1);
        assert_eq!(alt.pins[0].at.x, 1.27, "the alternate body has the alternate style's pin");
        assert_eq!(back.in_style(2).pins[0].at.x, 1.27, "a placed symbol in style 2 draws it");
        assert_eq!(back.in_style(1).pins[0].at.x, back.pins[0].at.x);
        // one that declares none has none, whatever stray item is of style 2
        sym.has_alternate_body_style = false;
        let single = sym.to_engine_symbol();
        assert!(single.alternate.is_none());
        assert_eq!(single.pins.len(), 1);
        assert_eq!(single.in_style(2).pins.len(), 1, "a one-style symbol draws its body in any style");
    }

    #[test]
    fn symbol_library_alternate_body_style_round_trips_through_the_engine_symbol() {
        let mut sym = LibrarySymbol::new_empty("Test:Gate");
        sym.has_alternate_body_style = true;
        let rect = |style: u32, x: f64| LibrarySymbolGraphic::Rectangle { id: String::new(), unit: 1, body_style: style, start: crate::symbol::SPoint::new(-x, -1.0), end: crate::symbol::SPoint::new(x, 1.0), stroke_mm: 0.254, fill: LibraryFill::None };
        // a shared outline, one body per style, a shared pin and one pin of the alternate style only
        sym.graphics = vec![rect(0, 3.0), rect(1, 1.0), rect(2, 2.0)];
        let mut pin_alt = lib_pin("2", 1, 0.0, 0.0);
        pin_alt.body_style = 2;
        let mut pin_shared = lib_pin("1", 1, 0.0, 3.81);
        pin_shared.body_style = 0;
        sym.pins = vec![pin_shared, pin_alt];
        let engine = sym.to_engine_symbol();
        assert_eq!(engine.graphics.len(), 2, "the shared outline and the normal body");
        assert_eq!(engine.alternate.as_ref().unwrap().graphics.len(), 2, "the shared outline and the alternate body");
        assert_eq!((engine.pins.len(), engine.alternate.as_ref().unwrap().pins.len()), (1, 2));
        let back = LibrarySymbol::from_engine_symbol(&engine);
        assert!(back.has_alternate_body_style);
        let by_style = |style: u32| back.graphics.iter().filter(|g| g.body_style() == style).count();
        assert_eq!((by_style(0), by_style(1), by_style(2)), (1, 1, 1), "shared stays shared");
        let pins_by_style = |style: u32| back.pins.iter().filter(|p| p.body_style == style).count();
        assert_eq!((pins_by_style(0), pins_by_style(1), pins_by_style(2)), (1, 0, 1));
        // and publishing it again changes nothing
        let again = back.to_engine_symbol();
        assert_eq!((again.graphics.len(), again.pins.len()), (engine.graphics.len(), engine.pins.len()));
        assert_eq!(again.alternate.as_ref().unwrap().pins.len(), 2);
    }

    #[test]
    fn symbol_library_section_is_additive_on_an_old_design() {
        // A `design.json` written before this editor existed has no
        // `symbol_library` key at all; it must still load, and a
        // normalization pass must not invent one out of thin air.
        let json = r#"{"schema": 1, "provenance": {"engine_version": "0", "intent_hash": "x", "seed": 0}}"#;
        let mut d: Design = serde_json::from_str(json).expect("an old design.json with no symbol_library must still parse");
        assert!(d.symbol_library.is_none());
        d.assign_missing_ids();
        assert!(d.symbol_library.is_none(), "assign_missing_ids must not create a section that was never there");
    }
    /// Additive IR: a `design.json` written before `erc_pin_map` /
    /// `user_fields` existed still parses (absent = KiCad default / no user
    /// fields) and re-serializes without them; set values round-trip.
    #[test]
    fn erc_pin_map_and_user_fields_are_additive_in_the_json() {
        let old = r#"{"symbols":[],"wires":[]}"#;
        let sch: SchematicSection = serde_json::from_str(old).unwrap();
        assert!(sch.erc_pin_map.is_none());
        assert!(sch.user_fields.is_empty());
        let back = serde_json::to_string(&sch).unwrap();
        assert!(!back.contains("erc_pin_map") && !back.contains("user_fields"), "absent stays absent: {back}");

        let mut sch = sch;
        let mut matrix = ErcPinMap::default_matrix();
        matrix[1][1] = 0;
        sch.erc_pin_map = Some(ErcPinMap { matrix });
        sch.user_fields.entry("R1".into()).or_default().insert("MPN".into(), "ERJ-3".into());
        let json = serde_json::to_string(&sch).unwrap();
        let round: SchematicSection = serde_json::from_str(&json).unwrap();
        assert_eq!(round.erc_pin_map, sch.erc_pin_map);
        assert_eq!(round.user_fields, sch.user_fields);
        assert_eq!(round.erc_pin_map.unwrap().matrix[1][1], 0);
    }
}
