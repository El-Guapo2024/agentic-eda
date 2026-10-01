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

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SchematicSection {
    /// Sorted by `id` (reference designator).
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
    /// Accepted ("excluded") ERC findings -- `dialog_erc.cpp`'s own
    /// per-sheet `SCHEMATIC::RecordERCExclusions`. Sorted by (check,
    /// location); see [`ErcExclusion`]'s own doc for why it has no `id`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub erc_exclusions: Vec<ErcExclusion>,
    /// Title block. `None` keeps relying on the caller-supplied
    /// `ExportMeta` (title/date) the way every export always has.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title_block: Option<TitleBlock>,
    /// Child hierarchical sheets. Always empty today (one sheet only) —
    /// carried so the format can grow into a hierarchy without another
    /// schema break; the reader accepts and preserves sheets it finds in
    /// an imported file without descending into their own content.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sheets: Vec<SheetInstance>,
    /// True when this section was built by `eda_kicad::import_kicad_sch`
    /// from a real `.kicad_sch` file, rather than by this project's own
    /// `derive_schematic`. The two disagree on what `SymbolInstance::at`
    /// means: `derive_schematic` always places it at the engine's own
    /// "top-left of a synthesized box" corner (every downstream consumer —
    /// rendering, the exporter's `baked_local`, and `eda_kicad::erc`'s own
    /// `resolve_pins` — agrees on that convention), while a real file's
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
    /// KiCad unit index (1-based); multi-unit placement is deferred, so
    /// this is always 1 for anything this port places.
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

/// An accepted ERC finding (`dialog_erc.cpp`'s own "Exclude this
/// violation" / `SCHEMATIC::RecordERCExclusions`): `(check, location)`
/// matches `eda_kicad::erc::Exclusions`'s own key shape exactly (a
/// `BTreeSet<(String, String)>`) so `crates/cli/src/studio.rs::erc_json`
/// can build one directly from this list with no translation. No `id`
/// field -- unlike `Wire`/`NetLabel`/etc., this has nothing geometric to
/// derive one from, and the `(check, location)` pair is already a stable,
/// natural key (unlike those others, there is never more than one
/// exclusion for the same finding to disambiguate between).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ErcExclusion {
    /// `eda_kicad::erc`'s own check name ("pin_not_connected", ...).
    pub check: String,
    /// "REF", "REF.PIN", or whatever else `CheckResult::location` carried
    /// for this finding -- a finding with no location at all can never be
    /// excluded (nothing to key on), same limitation `Exclusions` itself
    /// already has.
    pub location: String,
}

/// Title block. Every field optional/empty by default; the exporter falls
/// back to its `ExportMeta` argument for `title`/`date` when this whole
/// section is absent, so existing callers are unaffected.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
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
}

/// A child hierarchical sheet, as placed on its parent sheet. Deferred:
/// `derive_schematic` never creates one, and the reader does not descend
/// into `file` — this only records that a sheet symbol was there (its
/// name/file/position/size), so re-exporting a file that had one does not
/// silently drop it, and so the schema has a place to grow into real
/// hierarchy later.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SheetInstance {
    pub name: String,
    pub file: String,
    pub at: Point,
    pub size: (Um, Um),
}

impl SchematicSection {
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
}

impl Track {
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
}

impl Shape {
    pub fn id(&self) -> &str {
        match self {
            Shape::Segment { id, .. } | Shape::Arc { id, .. } | Shape::Rect { id, .. } | Shape::Circle { id, .. } | Shape::Polygon { id, .. } => id,
        }
    }
    pub fn set_id(&mut self, new_id: String) {
        match self {
            Shape::Segment { id, .. } | Shape::Arc { id, .. } | Shape::Rect { id, .. } | Shape::Circle { id, .. } | Shape::Polygon { id, .. } => *id = new_id,
        }
    }
    pub fn layer(&self) -> &str {
        match self {
            Shape::Segment { layer, .. } | Shape::Arc { layer, .. } | Shape::Rect { layer, .. } | Shape::Circle { layer, .. } | Shape::Polygon { layer, .. } => layer,
        }
    }
    /// `PCB_SHAPE::SetLayer` -- part of `dialog_pcb_shape_properties`'s own
    /// editable field set (GAPS.md #11), via `eda_ops::Cmd::EditShape`.
    pub fn set_layer(&mut self, new_layer: String) {
        match self {
            Shape::Segment { layer, .. } | Shape::Arc { layer, .. } | Shape::Rect { layer, .. } | Shape::Circle { layer, .. } | Shape::Polygon { layer, .. } => *layer = new_layer,
        }
    }
    /// `PCB_SHAPE::SetWidth` (`STROKE_PARAMS`'s width -- see `set_layer`'s doc).
    pub fn set_stroke_width(&mut self, width: Um) {
        match self {
            Shape::Segment { stroke_width, .. } | Shape::Arc { stroke_width, .. } | Shape::Rect { stroke_width, .. } | Shape::Circle { stroke_width, .. } | Shape::Polygon { stroke_width, .. } => {
                *stroke_width = width
            }
        }
    }
    /// `PCB_SHAPE::SetFilled` (see `set_layer`'s doc).
    pub fn set_filled(&mut self, filled_value: bool) {
        match self {
            Shape::Segment { filled, .. } | Shape::Arc { filled, .. } | Shape::Rect { filled, .. } | Shape::Circle { filled, .. } | Shape::Polygon { filled, .. } => *filled = filled_value,
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
        }
    }
    fn id_seed(&self) -> String {
        let kind = match self {
            Shape::Segment { .. } => "segment",
            Shape::Arc { .. } => "arc",
            Shape::Rect { .. } => "rect",
            Shape::Circle { .. } => "circle",
            Shape::Polygon { .. } => "polygon",
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

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DrawingsSection {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub shapes: Vec<Shape>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub texts: Vec<Text>,
}

impl DrawingsSection {
    /// Assign a deterministic id to every shape/text whose `id` is still
    /// empty. See [`RoutingSection::assign_missing_ids`] -- same contract,
    /// same reason for a stable per-kind processing order.
    pub fn assign_missing_ids(&mut self) {
        let mut existing: std::collections::BTreeSet<String> =
            self.shapes.iter().map(Shape::id).chain(self.texts.iter().map(|t| t.id.as_str())).filter(|s| !s.is_empty()).map(String::from).collect();

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
fn next_item_id(prefix: &str, seed: &str, existing: &std::collections::BTreeSet<String>) -> String {
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
                    SymbolInstance { id: "U1".into(), at: Point { x: 50_800, y: 63_500 }, rot: 0, mirrored: false, mirror_y: false, lib_id: String::new(), unit: 1, value: String::new(), footprint: String::new(), datasheet: String::new() },
                    SymbolInstance { id: "C1".into(), at: Point { x: 38_100, y: 63_500 }, rot: 90_000, mirrored: false, mirror_y: false, lib_id: String::new(), unit: 1, value: String::new(), footprint: String::new(), datasheet: String::new() },
                ],
                wires: vec![Wire { id: String::new(), net: "VIN".into(), pins: vec!["U1.3".into(), "C1.1".into()], pts: vec![Point { x: 35_000, y: 60_000 }, Point { x: 48_000, y: 60_000 }] }],
                labels: vec![],
                texts: vec![],
                power_symbols: vec![],
                no_connects: vec![],
                erc_exclusions: vec![], imported_from_kicad: false,
                title_block: None,
                sheets: vec![],
            }),
            nets: None,
            placement: None,
            routing: None,
            drawings: None,
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
        Track { id: String::new(), net: net.into(), pins: vec![], layer: layer.into(), width: 200, pts: pts.iter().map(|&(x, y)| Point { x, y }).collect() }
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
        let build = || RoutingSection { tracks: vec![track("GND", "F.Cu", &[(0, 0), (1000, 0)]), track("VCC", "F.Cu", &[(0, 0), (0, 1000)])], vias: vec![via("GND", 500, 500)], zones: vec![], track_width_presets: vec![], via_presets: vec![] };

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
        let mut rt1 = RoutingSection { tracks: vec![track("GND", "F.Cu", &[(0, 0), (1000, 0)]), track("VCC", "F.Cu", &[(0, 0), (0, 1000)])], vias: vec![], zones: vec![], track_width_presets: vec![], via_presets: vec![] };
        let mut rt2 = RoutingSection { tracks: vec![rt1.tracks[1].clone(), rt1.tracks[0].clone()], vias: vec![], zones: vec![], track_width_presets: vec![], via_presets: vec![] };
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
        let mut rt = RoutingSection { tracks: vec![track("GND", "F.Cu", &[(0, 0), (1000, 0)]), track("GND", "F.Cu", &[(0, 0), (1000, 0)])], vias: vec![], zones: vec![], track_width_presets: vec![], via_presets: vec![] };
        rt.assign_missing_ids();
        assert_ne!(rt.tracks[0].id, rt.tracks[1].id);
    }

    /// `SetTrackWidth`-style edits (anything that changes `width` but not
    /// `net`/`layer`/`pts`) must not change the id that was already
    /// assigned -- callers (the web UI, `DeleteTrack`) hold onto it.
    #[test]
    fn changing_width_after_assignment_does_not_move_the_id() {
        let mut rt = RoutingSection { tracks: vec![track("GND", "F.Cu", &[(0, 0), (1000, 0)])], vias: vec![], zones: vec![], track_width_presets: vec![], via_presets: vec![] };
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
}
