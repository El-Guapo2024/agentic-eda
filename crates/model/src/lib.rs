//! Constraint model — the SOURCE OF TRUTH for a design.
//! Coordinates never appear here; they are derived by the engine.
//! JSON internally, YAML at LLM-facing edges.

pub mod bezier;
pub mod board;
mod drc_checks;
pub mod erc_checks;
pub mod floorplan;
pub mod footprint;
pub mod fp_edit;
pub mod gensym;
pub mod ir;
pub mod kicad_font;
pub mod kicad_geom;
pub mod modules;
pub mod page;
pub mod rules;
pub mod sch_clipboard;
pub mod sch_extras;
pub mod symbol;

pub use footprint::{Footprint, Pad, PadKind, PadShape};
pub use symbol::{is_synthetic_lib_id, resolve_lib_id, LibPin, LibSymbol, SymbolGraphic};

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConstraintModel {
    #[serde(default)]
    pub parts: Vec<Part>,
    #[serde(default)]
    pub nets: Vec<Net>,
    #[serde(default)]
    pub clusters: Vec<Cluster>,
    #[serde(default)]
    pub placement_rules: Vec<PlacementRule>,
    #[serde(default)]
    pub stackup: Option<Stackup>,
    #[serde(default)]
    pub impedance_targets: Vec<ImpedanceTarget>,
    /// Explicit footprint definitions; anything not listed here falls to
    /// the built-in package library (`footprint::builtin`).
    #[serde(default)]
    pub footprints: Vec<Footprint>,
    /// A placed footprint's own pads, when the board edited them (Pad Properties): reference -> the footprint as that instance has it
    /// ([`fp_edit::patched_footprint`]). Filled by the board's loader from `DrawingsSection::footprint_edits`; [`ConstraintModel::footprint_of`]
    /// answers with it first, so the gates, the router and the exports all see the edited pad. Never part of an intent.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub instance_footprints: BTreeMap<String, Footprint>,
    /// Resolved library symbols, keyed by their own `lib_id`: filled in by
    /// `eda-kicad`'s symbol-library loader (real installed `.kicad_sym`
    /// files, falling back to `symbol::builtin`) before the schematic is
    /// exported, exactly the way `footprints` is filled in by
    /// `resolve_library_footprints` before placement. A part not covered
    /// here falls back to a synthesized generic box at export time.
    #[serde(default)]
    pub symbols: Vec<LibSymbol>,
    /// Board rules consumed by the placer, router and routing gates.
    #[serde(default)]
    pub board: BoardRules,
    /// What the solver may change on its own when the rules as written do
    /// not route (see `eda solve`). Absent = nothing: the board is built
    /// exactly as specified or fails.
    #[serde(default)]
    pub allow: Allowances,
    /// Stage choices and placement tuning (see [`SolverSettings`]); router
    /// tuning lives in `board.tuning`.
    #[serde(default)]
    pub solver: SolverSettings,
}

/// Design-rule latitude granted to the solver. Every field is an upper or
/// lower bound the user is willing to accept; the solver walks the
/// cheapest-first ladder of variants inside these bounds and takes the
/// first one that passes every gate, logging each attempt.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct Allowances {
    /// Most copper layers the solver may go to (2 or 4). None = as written.
    #[serde(default)]
    pub max_layers: Option<usize>,
    /// Narrowest track the solver may fall back to, µm. None = as written.
    #[serde(default)]
    pub min_track: Option<ir::Um>,
    /// Smallest clearance the solver may fall back to, µm. None = as written.
    #[serde(default)]
    pub min_clearance: Option<ir::Um>,
    /// Placers the solver may try, in preference order (`anneal`, `cypress`).
    /// Empty = only the one given on the command line.
    #[serde(default)]
    pub placers: Vec<String>,
}

/// Physical design rules. Integer µm throughout.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BoardRules {
    /// Routing grid pitch.
    #[serde(default = "d_grid")]
    pub grid: ir::Um,
    #[serde(default = "d_track")]
    pub track_width: ir::Um,
    #[serde(default = "d_clearance")]
    pub clearance: ir::Um,
    #[serde(default = "d_via_drill")]
    pub via_drill: ir::Um,
    #[serde(default = "d_via_dia")]
    pub via_diameter: ir::Um,
    /// Copper layer names, outer first: `["F.Cu", "B.Cu"]`.
    #[serde(default = "d_layers")]
    pub layers: Vec<String>,
    /// Per-class track width and routing order. A board without classes
    /// routes every net at `track_width` in one undifferentiated pass,
    /// which is not how anyone lays out a board: power wants copper, and
    /// wants it before the signals have taken the channels.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub net_classes: Vec<NetClass>,
    /// Board outline override, µm. When absent the placer sizes a rectangle.
    #[serde(default)]
    pub outline: Option<Vec<ir::Point>>,
    /// Refdes silkscreen font size override, µm. None = 1/40 of the shorter
    /// board side, clamped to 600..=1000 (KiCad's default text is 1 mm; on
    /// a 100 mm board the unclamped rule gave 2.5 mm labels that dwarfed
    /// every passive and sealed their pads).
    #[serde(default)]
    pub refdes_font_um: Option<ir::Um>,
    /// Copper pours. A poured net is not track-routed at all: its pads
    /// reach each other through the plane, which is how every real board
    /// carries GND. Declaring one is a claim the gates then have to check
    /// -- an unreachable pad is a hard fail, not a silent island.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pours: Vec<Pour>,
    /// Routing tuning. Every value has a default; all are settable here.
    #[serde(default)]
    pub tuning: RoutingTuning,
    /// Minimum edge-to-edge gap between two drilled holes (KiCad's
    /// `hole_to_hole_min`, default 250 µm) — mechanical, not electrical:
    /// applies between any two round-drilled pads/vias regardless of net.
    #[serde(default = "d_hole_to_hole_min")]
    pub hole_to_hole_min_um: ir::Um,
    /// Minimum clearance from a drilled hole's edge to other copper (KiCad's
    /// `hole_clearance`, default 250 µm).
    #[serde(default = "d_hole_clearance")]
    pub hole_clearance_um: ir::Um,
    /// `BOARD_DESIGN_SETTINGS::m_CopperEdgeClearance` as DRC checks it
    /// (`rules.min_copper_edge_clearance` in the `.kicad_pro`, or a legacy
    /// board's `(setup (edge_clearance ..))`); `None` = KiCad's 0.5 mm
    /// default. Separate from `tuning.edge_clearance_um`, which is the
    /// placer/router's own keep-back and stays floored at 0.5 mm.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub copper_edge_clearance_um: Option<ir::Um>,
    /// Minimum silkscreen-to-silkscreen (and silk-to-exposed-copper)
    /// clearance (KiCad's `silk_clearance`, default 0).
    #[serde(default = "d_silk_clearance")]
    pub silk_clearance_um: ir::Um,
    /// Minimum copper annular ring width on a via or plated through-hole pad
    /// (KiCad's `via_min_annular_width`; factory default derives from
    /// `(via_diameter - via_drill) / 2`, i.e. exactly the via's own nominal
    /// ring, so a via routed at the board's own default via size never
    /// trips it).
    #[serde(default = "d_annular_width_min")]
    pub annular_width_min_um: ir::Um,
    /// Minimum silkscreen text height (KiCad's `min_silk_text_height`,
    /// factory default 0.8 mm = 80% of the 1 mm default silk text size).
    #[serde(default = "d_min_silk_text_height")]
    pub min_silk_text_height_um: ir::Um,
    /// Minimum silkscreen text stroke thickness (KiCad's
    /// `min_silk_text_thickness`, factory default 0.08 mm = 80% of the
    /// 0.1 mm default silk text width).
    #[serde(default = "d_min_silk_text_thickness")]
    pub min_silk_text_thickness_um: ir::Um,
    /// `bds.m_TrackMinWidth`: the board-wide absolute track-width floor
    /// (KiCad's `rules.min_track_width` in a `.kicad_pro`, or the legacy
    /// `(setup (trace_min ...))` token in a bare `.kicad_pcb`), factory
    /// default 0.2 mm. Independent of any net class: a net class's own
    /// `track_width` (on [`NetClass`], falling back to this struct's own
    /// [`track_width`](Self::track_width)) is only ever the *nominal*
    /// width a new route is drawn at -- `nc->GetTrackWidth()` becomes a
    /// constraint's advisory `Opt`, never its `Min`, in
    /// `DRC_ENGINE::loadImplicitRules` (`pcbnew/drc/drc_engine.cpp`, read
    /// directly from the source: every per-netclass `TRACK_WIDTH_CONSTRAINT`
    /// rule sets `Min` to this same board-wide floor, never to the class's
    /// own width). Conflating the two -- checking every track against its
    /// net class's nominal width as if it were this floor -- was GAPS.md
    /// #10, responsible for ~2900 false positives on a single QA board
    /// (`issue11814`) alone; see `eda_drc::constraints::track_width_min`.
    #[serde(default = "d_track_width_min")]
    pub track_width_min_um: ir::Um,
    /// `bds.m_MinClearance`: an absolute board-wide copper clearance floor
    /// that applies on top of every net-class clearance value (KiCad's
    /// `rules.min_clearance`; factory default 0, a no-op on most boards,
    /// but real projects do raise it). Distinct from
    /// [`clearance`](Self::clearance), which is the *netclass* default/
    /// fallback value (itself a real `CLEARANCE_CONSTRAINT`, not merely
    /// advisory, unlike track width) -- see `eda_drc::constraints::clearance`.
    #[serde(default)]
    pub min_clearance_um: ir::Um,
    /// `bds.m_ViasMinSize`: the absolute via-diameter floor (KiCad's
    /// `rules.min_via_diameter`), independent of any net class's own
    /// nominal via size ([`via_diameter`](Self::via_diameter)/
    /// `NetClass::via_diameter`) -- same nominal-vs-minimum distinction as
    /// [`track_width_min_um`](Self::track_width_min_um).
    #[serde(default = "d_via_diameter_min")]
    pub via_diameter_min_um: ir::Um,
    /// `bds.m_MinThroughDrill`: the absolute through-hole/via-drill floor
    /// (KiCad's `rules.min_through_hole_diameter`), independent of any net
    /// class's own nominal drill ([`via_drill`](Self::via_drill)/
    /// `NetClass::via_drill`).
    #[serde(default = "d_via_drill_min")]
    pub via_drill_min_um: ir::Um,
    /// Per-DRC-type severity overrides, imported from a `.kicad_pro`'s
    /// `board.design_settings.rule_severities` (KiCad 7+ JSON project
    /// settings -- `BOARD_DESIGN_SETTINGS`'s `rule_severities` `PARAM_LAMBDA`,
    /// `pcbnew/board_design_settings.cpp`). Keyed by the same settings-key
    /// string as `eda_drc::ErrorType::key()`; valued `"error"`/`"warning"`/
    /// `"ignore"` exactly as `SeverityToString`/`SeverityFromString`
    /// (`common/widgets/ui_common.cpp`) round-trip them. Empty = every type
    /// keeps this port's own `ErrorType::default_severity()`, matching a
    /// board with no sidecar project. KiCad always writes the *complete*
    /// resolved table on save (every known type, not just user-touched
    /// ones), so this is populated wholesale from the project file, never
    /// merged key-by-key -- see `eda_kicad::merge_project_rule_severities`.
    /// A type set to `"ignore"` here means KiCad never even runs that
    /// check: kicad-cli (which gets these in the exported project file)
    /// reports nothing of that type at all, not a de-prioritized version
    /// of it.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub rule_severities: BTreeMap<String, String>,
    /// A `.kicad_dru` custom-rule file found next to an imported board
    /// (`eda_kicad::parse_custom_rules`, ported from `drc_rule_parser.cpp`),
    /// kept verbatim (task item 4: "Store rules in the IR (text plus
    /// parsed); KiCad files stay derived") so a rule this port's evaluator
    /// subset does not understand is still visible on the model rather than
    /// silently dropped, even though only [`custom_rules`](Self::custom_rules)
    /// is actually evaluated. `None` = no sidecar `.kicad_dru` (the
    /// overwhelming majority of boards).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub custom_rules_text: Option<String>,
    /// The parsed subset of `custom_rules_text`'s rules -- see
    /// [`CustomRule`] and `eda_drc::pcbexpr` (the PCBEXPR condition
    /// evaluator subset that reads `condition` back out at DRC time).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub custom_rules: Vec<CustomRule>,
    /// Whether `eda_kicad::import_kicad_pcb` found the board outline's
    /// Edge.Cuts graphics chained into a genuinely closed loop
    /// (`ImportNotes::outline_open`, negated, carried forward here since
    /// that struct itself is import-time-only and does not reach
    /// the checks). `None` = not applicable/not tracked -- a board
    /// this workspace's own pipeline produced (always a clean closed
    /// rectangle by construction), an outline reconstructed from a single
    /// `gr_poly`/`gr_circle` (inherently closed), or a `design.json`
    /// written before this field existed. Lives on `BoardRules` rather
    /// than `PlacementSection` (the more obvious home for an outline-shape
    /// fact) because the latter has no `Default` impl and is built via a
    /// full field literal in several `crates/pns`/`crates/freeroute`
    /// router files this task was told to leave alone -- adding a
    /// required field there would have forced edits across all of them
    /// for one DRC check. See `providers::outline`'s doc comment for why
    /// this -- not a guess from the point list alone -- is the only
    /// faithful way to know this at DRC time: an implicitly-closed polygon
    /// (first point not repeated as the last, this model's own convention
    /// for *every* board) and a genuinely broken import-time chain are
    /// geometrically indistinguishable from the point list by itself.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outline_closed: Option<bool>,
    /// Board-level solder-mask settings (`BOARD_DESIGN_SETTINGS`'s
    /// `m_SolderMaskExpansion`/`m_SolderMaskMinWidth`/
    /// `m_SolderMaskToCopperClearance`/`m_AllowSoldermaskBridgesInFPs`/
    /// `m_TentVias*`), read by `eda_drc`'s solder-mask provider.
    #[serde(default, skip_serializing_if = "SolderMaskRules::is_default")]
    pub solder_mask: SolderMaskRules,

    // ---- the rest of Board Setup (`rules::RulesOverlay` edits these; the defaults are KiCad's own) ----
    /// `rules.min_connection`: the narrowest copper neck a connection may have. KiCad's default is 0.
    #[serde(default, skip_serializing_if = "is_zero_um")]
    pub min_connection_um: ir::Um,
    /// `rules.min_microvia_diameter`.
    #[serde(default = "d_microvia_diameter_min")]
    pub microvia_diameter_min_um: ir::Um,
    /// `rules.min_microvia_drill`.
    #[serde(default = "d_microvia_drill_min")]
    pub microvia_drill_min_um: ir::Um,
    /// `rules.min_groove_width`.
    #[serde(default, skip_serializing_if = "is_zero_um")]
    pub min_groove_width_um: ir::Um,
    /// `rules.min_resolved_spokes`: fewer connected thermal spokes than this is a starved thermal.
    #[serde(default = "d_min_resolved_spokes")]
    pub min_resolved_spokes: u32,
    /// `rules.max_error`: how closely arcs are approximated by segments (`ARC_HIGH_DEF`, 5 um).
    #[serde(default = "d_max_error")]
    pub max_error_um: ir::Um,
    /// `rules.use_height_for_length_calcs`.
    #[serde(default = "d_true")]
    pub use_height_for_length_calcs: bool,
    /// `zones_allow_external_fillets`.
    #[serde(default)]
    pub zones_allow_external_fillets: bool,
    /// Whether the `*_min_um` constraints above were stated -- by a `.kicad_pro` the board came with or by Board
    /// Setup -- rather than left at KiCad's factory values. A board built from an intent never states them, and
    /// its derived `.kicad_pro` lowers them to the narrowest class it uses (an escape class routed under the
    /// default 0.2 mm is not a violation of rules nobody wrote); a stated floor is written exactly.
    #[serde(default, skip_serializing_if = "is_false")]
    pub constraints_explicit: bool,
    /// `BOARD_DESIGN_SETTINGS::GetBoardThickness()`, um: the board's thickness in the `.kicad_pcb`'s
    /// `(general (thickness ..))` and the 3D export. KiCad's default is 1.6 mm.
    #[serde(default = "d_board_thickness")]
    pub board_thickness_um: ir::Um,
    /// Text & Graphics > Defaults: the line widths and text the layer classes start from.
    #[serde(default, skip_serializing_if = "rules::TextGraphicsDefaults::is_default")]
    pub text_graphics: rules::TextGraphicsDefaults,
    /// The Default net class as Board Setup shows it: its microvia and differential-pair sizes, which have no
    /// board-wide field of their own. Its track width, clearance and via size are `track_width`, `clearance`,
    /// `via_diameter` and `via_drill` above.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_class: Option<NetClass>,
}

fn is_zero_um(v: &ir::Um) -> bool {
    *v == 0
}

fn is_false(v: &bool) -> bool {
    !*v
}

/// The board-wide solder-mask settings `drc_test_provider_solder_mask.cpp`
/// consumes. Defaults are KiCad's factory values (all margins 0, vias
/// tented on both sides -- `BOARD_DESIGN_SETTINGS::BOARD_DESIGN_SETTINGS`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SolderMaskRules {
    /// `m_SolderMaskExpansion`: `(setup (pad_to_mask_clearance ..))`.
    #[serde(default)]
    pub expansion_um: ir::Um,
    /// `m_SolderMaskMinWidth`: `(setup (solder_mask_min_width ..))` -- the
    /// "web width" the bridge test and whole-board-mask silk test use.
    #[serde(default)]
    pub min_width_um: ir::Um,
    /// `m_SolderMaskToCopperClearance`: `.kicad_pro`
    /// `rules.solder_mask_to_copper_clearance`.
    #[serde(default)]
    pub to_copper_clearance_um: ir::Um,
    /// `m_AllowSoldermaskBridgesInFPs`.
    #[serde(default)]
    pub allow_bridges_in_footprints: bool,
    /// `m_TentViasFront` / `m_TentViasBack` (default true).
    #[serde(default = "d_true")]
    pub tent_vias_front: bool,
    #[serde(default = "d_true")]
    pub tent_vias_back: bool,
    /// `m_SolderPasteMargin`: `(setup (pad_to_paste_clearance ..))`, the absolute paste margin, um.
    #[serde(default)]
    pub paste_margin_um: ir::Um,
    /// `m_SolderPasteMarginRatio`: `(setup (pad_to_paste_clearance_ratio ..))`, the margin as a fraction of the pad.
    #[serde(default)]
    pub paste_margin_ratio: f64,
}

fn d_true() -> bool {
    true
}

impl Default for SolderMaskRules {
    fn default() -> Self {
        SolderMaskRules { expansion_um: 0, min_width_um: 0, to_copper_clearance_um: 0, allow_bridges_in_footprints: false, tent_vias_front: true, tent_vias_back: true, paste_margin_um: 0, paste_margin_ratio: 0.0 }
    }
}

impl SolderMaskRules {
    pub fn is_default(&self) -> bool {
        *self == SolderMaskRules::default()
    }
}

/// One `(rule ...)` block from a `.kicad_dru` file
/// (`pcbnew/drc/drc_rule_parser.cpp`'s `DRC_RULE`), reduced to the fields
/// this port can actually apply. `condition`/`layer` are kept as the raw
/// PCBEXPR text KiCad itself would store there; `eda_drc::pcbexpr` parses
/// and evaluates `condition` against a specific item pair at DRC time
/// (parsed once per rule per run today, not cached -- real `.kicad_dru`
/// files are a handful of rules, not thousands). A field this parser could
/// not find in the file is `None`/empty, never a guessed default -- a rule
/// with no recognized `constraint` type, in particular, is kept (so it is
/// still visible in `custom_rules_text`) but never matches anything in
/// `eda_drc`'s application of it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct CustomRule {
    pub name: String,
    /// KiCad's own constraint-type keyword: `"clearance"`, `"hole_clearance"`,
    /// `"track_width"`, `"hole_to_hole"`, `"silk_clearance"`,
    /// `"edge_clearance"`, `"annular_width"`, `"disallow"`, `"assertion"`,
    /// and others this crate's evaluator does not act on (see
    /// `eda_drc::custom_rules`'s doc comment for exactly which do).
    pub constraint_type: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min: Option<ir::Um>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max: Option<ir::Um>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub opt: Option<ir::Um>,
    /// `(layer "...")`, when the rule restricts itself to one layer.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub layer: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub severity: Option<String>,
    /// The raw PCBEXPR text of `(condition "...")`, unevaluated -- `None`
    /// means the rule applies unconditionally (matches every pair).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub condition: Option<String>,
}

/// One copper plane: a net flooded across a whole layer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Pour {
    pub net: String,
    /// Stackup layer name ("B.Cu").
    pub layer: String,
}

impl BoardRules {
    /// Reject a board that cannot exist, at the door.
    ///
    /// Every value here is divided by, measured against, or indexed with
    /// somewhere downstream. A zero grid divides by zero; an empty stackup
    /// leaves the outer layers undefined; a via whose drill is wider than
    /// its pad has no annular ring to solder to. The readers used to cope
    /// -- `grid_um.max(1)`, `layers.first().unwrap_or("F.Cu")` -- and
    /// coping means inventing a number nobody wrote down, then measuring
    /// the board against it and reporting the result as fact.
    ///
    /// Validating once at parse time is what makes those guesses
    /// unnecessary rather than merely discouraged. Every problem is
    /// reported, not just the first: an agent fixing one value at a time
    /// across a dozen runs learns nothing a single list would not tell it.
    pub fn validate(&self) -> Vec<CheckResult> {
        let mut out = Vec::new();
        let mut bad = |what: &str, why: String| {
            out.push(CheckResult::fail("board_rules", format!("board.{what}"), why));
        };
        if self.grid <= 0 {
            bad("grid", format!("routing grid is {} µm; it is the divisor for every cell index on the board", self.grid));
        }
        if self.track_width <= 0 {
            bad("track_width", format!("track width is {} µm; copper with no width has no clearance either", self.track_width));
        }
        if self.clearance < 0 {
            bad("clearance", format!("clearance is {} µm; a negative gap is copper overlapping on purpose", self.clearance));
        }
        if self.layers.is_empty() {
            bad("layers", "board declares no copper layers; there is no such thing as a default stackup".into());
        }
        {
            let mut seen = std::collections::HashSet::new();
            for l in &self.layers {
                if !seen.insert(l) {
                    bad("layers", format!("copper layer {l:?} is listed twice; layer order is the stackup"));
                }
            }
        }
        if self.via_diameter <= 0 || self.via_drill <= 0 {
            bad("via", format!("via is {} µm across a {} µm drill; both have to be positive", self.via_diameter, self.via_drill));
        } else if self.via_drill >= self.via_diameter {
            bad(
                "via_drill",
                format!(
                    "via drill {} µm is not smaller than the {} µm pad, so the via has no annular ring: \
                     nothing for the plating to land on",
                    self.via_drill, self.via_diameter
                ),
            );
        }
        if let Some(o) = &self.outline {
            if o.len() < 3 {
                bad("outline", format!("board outline has {} point(s); a board is at least a triangle", o.len()));
            }
        }
        for c in &self.net_classes {
            if let Some(w) = c.track_width {
                if w <= 0 {
                    bad("net_classes", format!("net class {:?} sets a track width of {w} µm", c.name));
                }
            }
            if let Some(cl) = c.clearance {
                if cl < 0 {
                    bad("net_classes", format!("net class {:?} sets a clearance of {cl} µm", c.name));
                }
            }
        }
        for p in &self.pours {
            if !self.layers.contains(&p.layer) {
                bad(
                    "pours",
                    format!("pour on net {:?} names layer {:?}, which is not in the stackup {:?}", p.net, p.layer, self.layers),
                );
            }
        }
        self.tuning.validate(&mut out);
        out
    }

    /// Refdes font for this board: the override, else 1/40 of the shorter
    /// rendered side (outline bbox + 2 mm margin), clamped to 600..=1000 µm.
    pub fn refdes_font(&self, outline: &[ir::Point]) -> ir::Um {
        if let Some(f) = self.refdes_font_um {
            return f;
        }
        let (mut x0, mut y0, mut x1, mut y1) = (ir::Um::MAX, ir::Um::MAX, ir::Um::MIN, ir::Um::MIN);
        for p in outline {
            x0 = x0.min(p.x);
            y0 = y0.min(p.y);
            x1 = x1.max(p.x);
            y1 = y1.max(p.y);
        }
        if outline.is_empty() {
            return 600;
        }
        let m = 2000;
        let (vw, vh) = (x1 - x0 + 2 * m, y1 - y0 + 2 * m);
        (vw.min(vh) / 40).clamp(600, 1000)
    }
}

/// The routing rules' tunable distances, with defaults. µm, and cells of
/// the board's routing grid for the placement preflight.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct RoutingTuning {
    /// Copper-to-board-edge clearance (KiCad's default 0.5 mm).
    pub edge_clearance_um: ir::Um,
    /// Same-footprint SMD pad gaps narrower than this are kept clear of
    /// tracks.
    pub between_pads_max_gap_um: ir::Um,
    /// Cells a pad's free pocket must reach (or a via site / own pad) to
    /// pass the placement preflight.
    pub preflight_reach_cells: usize,
    /// Passes the post-route optimizer may make, rerouting each via and
    /// trace for fewer vias and less copper; it stops sooner once a pass
    /// improves the board too little. 0 skips it.
    pub optimizer_passes: u32,
}

/// KiCad's default board-setup copper-to-edge clearance, which
/// `kicad-cli pcb drc` holds every pad, track and via to against Edge.Cuts.
pub const KICAD_EDGE_CLEARANCE_UM: ir::Um = 500;

impl RoutingTuning {
    /// How far copper keeps from the board edge: the tuning's clearance,
    /// never under KiCad's own.
    pub fn copper_edge_clearance(&self) -> ir::Um {
        self.edge_clearance_um.max(KICAD_EDGE_CLEARANCE_UM)
    }

    /// Reject a tuning that cannot describe a board: a zero here is a loop
    /// bound downstream. Checking once at the boundary is what lets every
    /// consumer use it without a guard.
    pub fn validate(&self, out: &mut Vec<CheckResult>) {
        if self.preflight_reach_cells == 0 {
            out.push(CheckResult::fail("board_rules", "board.tuning.preflight_reach_cells", "a pad pocket must reach 0 cells, which every buried pad satisfies"));
        }
    }
}
impl Default for RoutingTuning {
    fn default() -> Self {
        RoutingTuning { edge_clearance_um: 500, between_pads_max_gap_um: 2000, preflight_reach_cells: 2000, optimizer_passes: 10 }
    }
}

/// Stage choices and placement tuning the intent may set. Defaults apply
/// when absent; the CLI flags override when given explicitly.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct SolverSettings {
    /// `anneal` (default) or `cypress`.
    /// Which placer runs. Defaults to `build`, the gradual constructive
    /// placer: one part at a time, gates checked after each, so a
    /// failure is attributable to the step that caused it.
    ///
    /// `anneal` currently clears more of the ladder, but it clears it by
    /// shuffling a whole board until the score improves, which makes a
    /// remaining failure a property of the run rather than of any
    /// decision. Defaulting to `build` keeps the open failures in front
    /// of us instead of behind a placer that happens to pass.
    pub placer: String,
    /// Anneal placer: keep-apart margin around courtyards (routing channel
    /// space), moves per part, placement snap grid, all µm / counts.
    pub place_spacing_um: ir::Um,
    pub place_moves_per_part: usize,
    pub place_snap_um: ir::Um,
    /// Cypress: weight of the synthetic proximity-rule nets.
    pub cypress_proximity_weight: f64,
    /// Cypress: shrink a rectangular `board.outline` (treated as the
    /// largest allowed board) until the parts' keep-out area is this
    /// fraction of the board, so a small design does not float in a big
    /// blank rectangle. 0 = keep the outline as written.
    pub fit_board_utilization: f64,
    /// How many placement seeds one rung of the solver's ladder may try
    /// before escalating. Placement is the cheap stage to repeat and most
    /// of its failures are near misses that a different arrangement
    /// clears; escalating re-places anyway, under coarser settings. 1 =
    /// the old single-shot behaviour.
    pub place_attempts: usize,
    /// Cut the board into functional modules and give each one a region
    /// before placing any part (see [`floorplan`]). This is how a human
    /// team lays out a board; on a hundred-part design it turns one large
    /// placement into a dozen small ones. Off by default: it changes the
    /// shape of every placement, so an intent opts in.
    #[serde(default)]
    pub floorplan: bool,
}
impl Default for SolverSettings {
    fn default() -> Self {
        SolverSettings { placer: "build".into(), place_spacing_um: 600, place_moves_per_part: 4000, place_snap_um: 100, cypress_proximity_weight: 50.0, fit_board_utilization: 0.25, place_attempts: 3, floorplan: false }
    }
}

/// Max courtyard gap, µm, from a decoupling capacitor to the IC it
/// decouples (shares both of its nets with). The placement gate judges it;
/// the placers pull toward it.
pub const DECOUPLING_MAX_GAP_UM: ir::Um = 3000;

/// A part the placer can freely move or swap to uncross a stub: two pins,
/// not a connector or an IC.
pub fn is_free_two_pin(part: &Part) -> bool {
    part.pins.len() == 2 && !part.reference.starts_with('J') && !part.reference.starts_with('U')
}

/// Decoupling pairs: a 2-pin `C…` part between a power net and a ground
/// net (judged by the pin kinds on those nets), and a `U…` part that has
/// pins on both. A cap from a signal net to ground is a filter, not
/// decoupling. Shared by the gate and the placers.
pub fn decoupling_pairs(model: &ConstraintModel) -> Vec<(String, String)> {
    let net_of_pin: std::collections::HashMap<&str, &str> =
        model.nets.iter().flat_map(|n| n.pins.iter().map(move |p| (p.as_str(), n.name.as_str()))).collect();
    let kind_of_pin: std::collections::HashMap<String, PinKind> =
        model.parts.iter().flat_map(|p| p.pins.iter().map(move |pin| (format!("{}.{}", p.reference, pin.number), pin.kind))).collect();
    let net_kind = |name: &str| -> (bool, bool) {
        let Some(n) = model.nets.iter().find(|n| n.name == name) else { return (false, false) };
        let kinds: Vec<PinKind> = n.pins.iter().filter_map(|p| kind_of_pin.get(p).copied()).collect();
        (kinds.contains(&PinKind::Power), kinds.contains(&PinKind::Ground))
    };
    let mut pairs = Vec::new();
    for c in &model.parts {
        if !c.reference.starts_with('C') || c.pins.len() != 2 {
            continue;
        }
        let nets: Vec<&str> = c.pins.iter().filter_map(|p| net_of_pin.get(format!("{}.{}", c.reference, p.number).as_str()).copied()).collect();
        if nets.len() != 2 || nets[0] == nets[1] {
            continue;
        }
        let (k0, k1) = (net_kind(nets[0]), net_kind(nets[1]));
        let rail_to_ground = (k0.0 && k1.1) || (k1.0 && k0.1);
        if !rail_to_ground {
            continue;
        }
        for u in &model.parts {
            if !u.reference.starts_with('U') {
                continue;
            }
            let has = |net: &str| u.pins.iter().any(|p| net_of_pin.get(format!("{}.{}", u.reference, p.number).as_str()) == Some(&net));
            if has(nets[0]) && has(nets[1]) {
                pairs.push((c.reference.clone(), u.reference.clone()));
            }
        }
    }
    pairs
}
fn d_grid() -> ir::Um { 254 }
fn d_track() -> ir::Um { 200 }
fn d_clearance() -> ir::Um { 200 }
fn d_via_drill() -> ir::Um { 300 }
fn d_via_dia() -> ir::Um { 600 }
fn d_layers() -> Vec<String> { vec!["F.Cu".into(), "B.Cu".into()] }
fn d_hole_to_hole_min() -> ir::Um { 250 }
fn d_hole_clearance() -> ir::Um { 250 }
fn d_silk_clearance() -> ir::Um { 0 }
fn d_annular_width_min() -> ir::Um { 100 }
fn d_min_silk_text_height() -> ir::Um { 800 }
fn d_min_silk_text_thickness() -> ir::Um { 80 }
fn d_track_width_min() -> ir::Um { 200 }
fn d_via_diameter_min() -> ir::Um { 500 }
fn d_via_drill_min() -> ir::Um { 300 }
fn d_microvia_diameter_min() -> ir::Um { 200 }
fn d_microvia_drill_min() -> ir::Um { 100 }
fn d_min_resolved_spokes() -> u32 { 2 }
fn d_max_error() -> ir::Um { 5 }
fn d_board_thickness() -> ir::Um { 1600 }
impl BoardRules {
    /// The class owning `net`, if any: first match wins.
    pub fn class_of(&self, net: &str) -> Option<&NetClass> {
        self.net_classes.iter().find(|c| c.matches(net))
    }

    /// Track width `net` must be routed at.
    pub fn width_of(&self, net: &str) -> ir::Um {
        self.class_of(net).and_then(|c| c.track_width).unwrap_or(self.track_width)
    }

    /// Clearance `net`'s own copper keeps, to itself and to everything
    /// else. Absent from the net's class (or no class) = the board default.
    pub fn clearance_of(&self, net: &str) -> ir::Um {
        self.class_of(net).and_then(|c| c.clearance).unwrap_or(self.clearance)
    }

    /// Via copper diameter `net`'s vias should use. Absent from the net's
    /// class (or no class) = the board default.
    pub fn via_diameter_of(&self, net: &str) -> ir::Um {
        self.class_of(net).and_then(|c| c.via_diameter).unwrap_or(self.via_diameter)
    }

    /// Via drill diameter `net`'s vias should use. Absent from the net's
    /// class (or no class) = the board default.
    pub fn via_drill_of(&self, net: &str) -> ir::Um {
        self.class_of(net).and_then(|c| c.via_drill).unwrap_or(self.via_drill)
    }

    /// Differential-pair trace width for `net`'s class. Unlike every
    /// resolver above, there is no board-wide `diff_pair_width` default
    /// field to fall back to (`NetClass::diff_pair_width` was carried
    /// additively on import with nothing reading it until now -- see that
    /// field's own doc comment); absent a class value, this falls back to
    /// `SIZES_SETTINGS`'s own hardcoded upstream default
    /// (`pns_sizes_settings.h`'s constructor: `m_diffPairWidth(125000)`,
    /// nanometers in KiCad's own internal unit -- 125um here).
    pub fn diff_pair_width_of(&self, net: &str) -> ir::Um {
        self.class_of(net).and_then(|c| c.diff_pair_width).unwrap_or(125)
    }

    /// Differential-pair gap for `net`'s class -- see
    /// `diff_pair_width_of`'s own doc comment; upstream's matching default
    /// is `m_diffPairGap(180000)` (180um).
    pub fn diff_pair_gap_of(&self, net: &str) -> ir::Um {
        self.class_of(net).and_then(|c| c.diff_pair_gap).unwrap_or(180)
    }

    /// Differential-pair via-to-via gap for `net`'s class -- upstream's
    /// `SIZES_SETTINGS::DiffPairViaGap()` defaults to "same as the trace
    /// gap" (`m_diffPairViaGapSameAsTraceGap(true)`), so this falls back to
    /// [`Self::diff_pair_gap_of`] rather than its own separate 180um
    /// literal when the class doesn't set one either.
    pub fn diff_pair_via_gap_of(&self, net: &str) -> ir::Um {
        self.class_of(net).and_then(|c| c.diff_pair_via_gap).unwrap_or_else(|| self.diff_pair_gap_of(net))
    }

    /// Widest track any net on this board can take. The router's clearance
    /// summaries are precomputed for one querying width, so they are built
    /// at this one: conservative for narrow nets, correct for every net.
    pub fn widest_track(&self) -> ir::Um {
        self.net_classes.iter().filter_map(|c| c.track_width).fold(self.track_width, ir::Um::max)
    }

    /// Routing order key: higher priority first. Power before signals.
    pub fn priority_of(&self, net: &str) -> i32 {
        self.class_of(net).map_or(0, |c| c.priority)
    }
}

impl Default for BoardRules {
    fn default() -> Self {
        BoardRules {
            grid: d_grid(), track_width: d_track(), clearance: d_clearance(), via_drill: d_via_drill(), via_diameter: d_via_dia(), layers: d_layers(),
            net_classes: Vec::new(), outline: None, refdes_font_um: None, pours: Vec::new(), tuning: RoutingTuning::default(),
            hole_to_hole_min_um: d_hole_to_hole_min(), hole_clearance_um: d_hole_clearance(), silk_clearance_um: d_silk_clearance(),
            annular_width_min_um: d_annular_width_min(), min_silk_text_height_um: d_min_silk_text_height(), min_silk_text_thickness_um: d_min_silk_text_thickness(),
            track_width_min_um: d_track_width_min(), min_clearance_um: 0, via_diameter_min_um: d_via_diameter_min(), via_drill_min_um: d_via_drill_min(),
            rule_severities: BTreeMap::new(), custom_rules_text: None, custom_rules: Vec::new(), outline_closed: None,
            copper_edge_clearance_um: None, solder_mask: SolderMaskRules::default(),
            min_connection_um: 0, microvia_diameter_min_um: d_microvia_diameter_min(), microvia_drill_min_um: d_microvia_drill_min(), min_groove_width_um: 0,
            min_resolved_spokes: d_min_resolved_spokes(), max_error_um: d_max_error(), use_height_for_length_calcs: true, zones_allow_external_fillets: false,
            constraints_explicit: false, board_thickness_um: d_board_thickness(), text_graphics: rules::TextGraphicsDefaults::default(), default_class: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Part {
    /// Reference designator, unique: "U1", "C3".
    pub reference: String,
    #[serde(default)]
    pub mpn: Option<String>,
    /// LCSC catalog number ("C123456"), JLCPCB's own distributor key.
    /// Distinct from `mpn` -- a manufacturer part number is not what a
    /// specific distributor calls it -- and what a JLCPCB-format BOM/CPL
    /// actually keys the assembler's reels by. `None` prints as a blank
    /// "LCSC Part #" cell rather than a guess: an assembler cannot fill a
    /// missing part number from a value string, and a wrong one places the
    /// wrong part.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lcsc: Option<String>,
    #[serde(default)]
    pub value: Option<String>,
    #[serde(default)]
    pub package: Option<String>,
    /// KiCad footprint id, e.g. "Capacitor_SMD:C_0402_1005Metric".
    #[serde(default)]
    pub footprint: Option<String>,
    /// KiCad library symbol id, e.g. "Device:R" or "Regulator_Linear:AMS1117-3.3"
    /// -- overrides the by-kind default [`symbol::resolve_lib_id`] would
    /// otherwise pick.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub symbol: Option<String>,
    /// Datasheet URL, carried onto the exported symbol instance's
    /// `Datasheet` field. Unset falls back to the resolved library
    /// symbol's own Datasheet property, when it has one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub datasheet: Option<String>,
    #[serde(default)]
    pub pins: Vec<Pin>,
    /// Physical body size (width, height) in µm, as the distributor
    /// reports it for this exact MPN -- not the package name's nominal
    /// size. Set when the part is sourced; it is the only thing that
    /// lets a gate check the land pattern against the part rather than
    /// against a string. See `source_footprint_body_mismatch`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body_um: Option<(ir::Um, ir::Um)>,
    /// Cable comes in from outside: the part must sit on a board edge
    /// (`placement_edge_connector`). Unset = decided from the value/mpn
    /// (USB, jack, terminal block, receptacle…); a bare `J` header is
    /// *not* an edge part by itself — in real boards most `J` headers are
    /// interior (programming, jumpers), so say `edge: true` for the ones
    /// a cable plugs into.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub edge: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Pin {
    pub number: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub kind: PinKind,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PinKind {
    Power,
    Ground,
    #[default]
    Signal,
    Passive,
    Nc,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Net {
    pub name: String,
    /// Pin refs as "U1.3".
    #[serde(default)]
    pub pins: Vec<String>,
}

/// The atomic unit for both schematic and PCB.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Cluster {
    pub anchor: String,
    #[serde(default)]
    pub members: Vec<String>,
    #[serde(default)]
    pub orientation: Orientation,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Orientation {
    #[default]
    Up,
    Down,
    Left,
    Right,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum PlacementRule {
    /// Two parts must end up within `max_mm` of each other, measured
    /// courtyard gap to courtyard gap.
    Proximity {
        a: String,
        b: String,
        max_mm: f64,
        /// Why the rule exists, in the author's own words. Carried into
        /// the gate's failure detail, so a rule can be judged without
        /// reading whatever produced it. A rule proposed from circuit
        /// intuition rather than a datasheet says so here.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reason: Option<String>,
    },
    /// Two parts must end up at least `min_mm` apart. The mirror of
    /// `Proximity`, and the reason it exists: heat, noise coupling and
    /// high-voltage clearance are all repulsive, and until this variant
    /// the intent could only ever ask for parts to be *closer*. Note that
    /// the Cypress placer cannot honour it -- proximity reaches Cypress as
    /// a synthetic weighted net, and a net can only pull -- so a design
    /// carrying separation rules is an anneal-placer design until that
    /// changes. The gate fails either way rather than ignoring the rule.
    Separation {
        a: String,
        b: String,
        min_mm: f64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reason: Option<String>,
    },
    Keepout { zone: String, refs: Vec<String> },
    ThermalGroup { refs: Vec<String> },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Stackup {
    pub layers: Vec<StackupLayer>,
    /// `(copper_finish ..)`: "ENIG", "HASL", ... ; `None` = not specified.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub copper_finish: Option<String>,
    /// `(dielectric_constraints yes)`.
    #[serde(default, skip_serializing_if = "is_false")]
    pub dielectric_constraints: bool,
    /// `(edge_connector yes|bevelled)`: 0 none, 1 yes, 2 bevelled.
    #[serde(default, skip_serializing_if = "is_zero_u8")]
    pub edge_connector: u8,
    /// `(edge_plating yes)`.
    #[serde(default, skip_serializing_if = "is_false")]
    pub edge_plating: bool,
}

fn is_zero_u8(v: &u8) -> bool {
    *v == 0
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StackupLayer {
    pub name: String,
    #[serde(default)]
    pub material: Option<String>,
    #[serde(default)]
    pub thickness_mm: Option<f64>,
    /// KiCad's stackup item type, as the file names it: `copper`, `core`, `prepreg`, `Top Silk Screen`,
    /// `Top Solder Paste`, `Top Solder Mask` and their bottom counterparts. `None` = a layer written before
    /// the stackup was editable: `copper` when the name is a copper layer, else not written to the file.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    /// `(epsilon_r ..)`, the dielectric constant of the material.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub epsilon_r: Option<f64>,
    /// `(loss_tangent ..)`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub loss_tangent: Option<f64>,
}

/// A group of nets that route alike. Matched by glob over the net name.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NetClass {
    pub name: String,
    /// Globs over net names: `["GND", "VBAT*", "3V3"]`. First class whose
    /// pattern matches a net owns it, so order the list most-specific first.
    pub nets: Vec<String>,
    /// Track width for this class. Absent = the board default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub track_width: Option<ir::Um>,
    /// Clearance this class's copper keeps -- to itself and to every other
    /// class's copper alike, not only to its own nets. Absent = the board
    /// default. An "escape" class asking for less room than the rest of
    /// the board (a fine-pitch connector row a wider default clearance
    /// cannot thread) is the reason this is a class-level override rather
    /// than only a board-wide number: it narrows the gap where one net
    /// needs it narrowed, and leaves every other net at the board's own
    /// clearance. See `crates/freeroute/src/design.rs` for how this
    /// reaches the router's clearance matrix.
    ///
    /// DRC (`eda_drc`) resolves the clearance between two items as the
    /// *larger* of their two nets' resolved class clearance -- the same
    /// rule KiCad's own `DRC_ENGINE::EvalRules` netclass fast path uses.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub clearance: Option<ir::Um>,
    /// Via *copper* diameter for this class (KiCad `net_settings.classes[].
    /// via_diameter`). Absent = the board default (`BoardRules::
    /// via_diameter`). Additive field: every existing `NetClass` literal
    /// predates this and compiles unchanged as `None` (serde default too,
    /// for a `.kicad_pro`/older intent file with no such key).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub via_diameter: Option<ir::Um>,
    /// Via drill diameter for this class. Absent = the board default
    /// (`BoardRules::via_drill`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub via_drill: Option<ir::Um>,
    /// Microvia copper diameter for this class. This workspace's router and
    /// DRC don't model microvias as a distinct item (see `crates/drc`'s own
    /// fidelity notes), so this is carried for round-tripping a `.kicad_pro`
    /// faithfully but not yet read by any constraint resolution here.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub microvia_diameter: Option<ir::Um>,
    /// Microvia drill diameter for this class (see `microvia_diameter`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub microvia_drill: Option<ir::Um>,
    /// Differential-pair trace width for this class. Not yet read by any
    /// provider (this workspace has no diff-pair concept in routing/DRC
    /// yet -- `docs/parity/GAPS.md` #22), carried additively so a
    /// `.kicad_pro` import doesn't silently drop it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub diff_pair_width: Option<ir::Um>,
    /// Differential-pair gap for this class (see `diff_pair_width`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub diff_pair_gap: Option<ir::Um>,
    /// Differential-pair via gap for this class (see `diff_pair_width`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub diff_pair_via_gap: Option<ir::Um>,
    /// Routed before lower numbers. Power belongs first: it needs copper,
    /// it sets the return paths, and it is the hardest thing to squeeze in
    /// once signal nets have taken the channels. Default 0; signals sit at
    /// 0 and power is given a higher number in the intent.
    #[serde(default)]
    pub priority: i32,
}

impl NetClass {
    /// Whether `net` belongs to this class. `*` matches any run of characters,
    /// which covers the way power nets are actually named (`VBAT_RAW`,
    /// `VBAT_FUSED`), and `?` any one character; the pattern must match the
    /// whole name (KiCad's `EDA_PATTERN_MATCH_WILDCARD_ANCHORED`).
    pub fn matches(&self, net: &str) -> bool {
        self.nets.iter().any(|pat| glob_match(pat, net))
    }
}


#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ImpedanceTarget {
    /// Glob over net names, e.g. "USB_D*".
    pub net_pattern: String,
    pub ohms: f64,
    #[serde(default)]
    pub tolerance_pct: Option<f64>,
}

// ---------------------------------------------------------------- check shape
/// Every checker — schema, ERC, metrics, critic — returns this one shape.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckResult {
    pub check: String,
    pub status: CheckStatus,
    #[serde(default)]
    pub location: Option<String>,
    #[serde(default)]
    pub hint: Option<String>,
    /// Machine-readable failure detail (what blocks, where, which knob
    /// would change it). Absent for passes and for checks that have not
    /// been upgraded yet.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CheckStatus {
    Pass,
    Fail,
    Warn,
    /// A finding a human has reviewed and waived -- KiCad's own ERC/DRC
    /// "Exclude" action: the violation still shows up in the report (its
    /// `hint` and `location` are untouched) so the review isn't silently
    /// lost, but it counts as neither a failure nor a warning. See
    /// `ErcExclusion` -- the studio applies the schematic's exclusions to
    /// kicad-cli's ERC report.
    Excluded,
}

impl CheckResult {
    pub fn fail(check: &str, location: impl Into<String>, hint: impl Into<String>) -> Self {
        Self { check: check.into(), status: CheckStatus::Fail,
               location: Some(location.into()), hint: Some(hint.into()), detail: None }
    }
    pub fn pass(check: &str) -> Self {
        Self { check: check.into(), status: CheckStatus::Pass, location: None, hint: None, detail: None }
    }
    pub fn with_detail(mut self, detail: serde_json::Value) -> Self {
        self.detail = Some(detail);
        self
    }
}

impl ConstraintModel {
    pub fn part(&self, reference: &str) -> Option<&Part> {
        self.parts.iter().find(|p| p.reference == reference)
    }
    /// Resolve the physical footprint for a part: explicit model
    /// definitions first (by `Part::footprint` name, then by `package`),
    /// then the built-in library. `None` means the part is not physically
    /// realisable and every physical stage must fail on it.
    pub fn footprint_of(&self, part: &Part) -> Option<Footprint> {
        if let Some(own) = self.instance_footprints.get(&part.reference) {
            return Some(own.clone());
        }
        self.library_footprint_of(part)
    }
    /// [`ConstraintModel::footprint_of`] without the instance's own edits: the footprint as the library (or the intent) defines it.
    pub fn library_footprint_of(&self, part: &Part) -> Option<Footprint> {
        for key in [part.footprint.as_deref(), part.package.as_deref()].into_iter().flatten() {
            if let Some(fp) = self.footprints.iter().find(|f| f.name == key) {
                return Some(fp.clone());
            }
            let norm = footprint::normalize_name(key);
            if let Some(fp) = self.footprints.iter().find(|f| footprint::normalize_name(&f.name) == norm) {
                return Some(fp.clone());
            }
        }
        for key in [part.footprint.as_deref(), part.package.as_deref()].into_iter().flatten() {
            if let Some(mut fp) = footprint::builtin(key) {
                fp.model = footprint::kicad_footprint_for(&footprint::normalize_name(key), &part.reference, part.value.as_deref()).map(|f| f.model_path());
                return Some(fp);
            }
        }
        None
    }
    /// Resolve a `lib_id` (`"Device:R"`, `"power:GND"`, ...) to its
    /// `LibSymbol`: an explicit entry in `self.symbols` first (however it
    /// got there -- the real-library loader or a hand-written intent),
    /// then [`symbol::builtin`]. `None` means "no real or built-in
    /// definition" -- the exporter's cue to synthesize a generic box from
    /// the part's own pins, exactly as it always has.
    pub fn symbol_of(&self, lib_id: &str) -> Option<LibSymbol> {
        if let Some(s) = self.symbols.iter().find(|s| s.lib_id == lib_id) {
            return Some(s.clone());
        }
        symbol::builtin(lib_id)
    }
    /// The library id a derivation draws `part` with: what [`resolve_lib_id`] names, unless that is a library symbol the model holds that
    /// does not take all of the part's pins (an `nc` pin the library's symbol has no pin for): drawing it under the library's name would be a
    /// box of our own passing for the library's, so the part gets a generated symbol.
    pub fn lib_id_of(&self, part: &Part) -> String {
        self.fitting_lib_id(part, resolve_lib_id(part))
    }
    /// `lib_id` as it is, or the generated symbol's id when `lib_id` is a library symbol the model holds that does not take all of
    /// `part`'s pins.
    pub fn fitting_lib_id(&self, part: &Part, lib_id: String) -> String {
        if !lib_id.is_empty() && !symbol::is_synthetic_lib_id(&lib_id) {
            if let Some(sym) = self.symbol_of(&lib_id) {
                if !part.pins.iter().all(|p| sym.pin_by_number(&p.number).is_some()) {
                    return format!("{}{}", gensym::GENERATED_PREFIX, part.reference);
                }
            }
        }
        lib_id
    }
    /// [`Self::symbol_of`], but `None` outright for an empty or synthetic
    /// (`"eda:..."`) lib_id, without even trying `self.symbols`/`builtin` --
    /// and `None` too when the symbol that *did* resolve doesn't actually
    /// speak for `part`'s own pins. The common "does this instance have a
    /// real symbol to place real geometry from" question
    /// `eda_engine::geometry`'s real-symbol-aware
    /// `node_size`/`build_ports`/`nc_pin_local_points`, the ERC port, and
    /// the `.kicad_sch` writer all ask before falling back to the
    /// synthetic box.
    ///
    /// `resolve_lib_id` matches a connector-shaped part to
    /// `Connector_Generic:Conn_01x<N>` by pin *count* alone, whose pins are
    /// numbered "1".."N" sequentially -- fine for a part whose own pins are
    /// numbered the same way (`examples/nc_pins.yaml`'s J1), but a real-world
    /// connector's own pad names rarely are
    /// (`examples/ladder/l1_usb_mcu.yaml`'s J1, a USB-C receptacle, numbers
    /// its pins `"A1".."B12"`/`"SH"`). Using that symbol's geometry anyway
    /// would silently drop every pin `build_ports_from_real_symbol` can't
    /// find by number -- not just mis-drawing the part, but crashing the
    /// layout engine the first time it tries to wire one of those "missing"
    /// pins (`"NC pin cannot be wired"`), since nothing downstream expects a
    /// non-`Nc` pin to come back with no port. Requiring every one of
    /// `part`'s own pins (`Nc`-kind ones too, so a part's full pin count is
    /// always drawn) to resolve by number -- or none of them to, falling
    /// back to the honest synthetic box exactly as if nothing had resolved
    /// -- keeps that guarantee without each of this method's many call
    /// sites having to re-derive it.
    pub fn real_symbol_of(&self, lib_id: &str, part: &Part) -> Option<LibSymbol> {
        if lib_id.is_empty() {
            return None;
        }
        // a generated symbol is made from the part (`gensym`), unless the Symbol Editor has published one of its own under the same id
        if gensym::is_generated_lib_id(lib_id) {
            if let Some(s) = self.symbols.iter().find(|s| s.lib_id == lib_id) {
                if part.pins.iter().all(|p| s.pin_by_number(&p.number).is_some()) {
                    return Some(s.clone());
                }
            }
            return Some(gensym::generate(part, lib_id));
        }
        if symbol::is_synthetic_lib_id(lib_id) {
            return None;
        }
        let sym = self.symbol_of(lib_id)?;
        if part.pins.iter().all(|p| sym.pin_by_number(&p.number).is_some()) { Some(sym) } else { None }
    }
    /// Simple glob match ('*' wildcard) over net names.
    pub fn nets_matching(&self, pattern: &str) -> Vec<&Net> {
        self.nets.iter().filter(|n| glob_match(pattern, &n.name)).collect()
    }
}

pub fn glob_match(pattern: &str, s: &str) -> bool {
    // '*' matches any run (incl. empty) and '?' any one character -- the wildcards
    // `EDA_PATTERN_MATCH_WILDCARD_ANCHORED` reads in a net class pattern; everything else literal.
    fn rec(p: &[char], s: &[char]) -> bool {
        match (p.first(), s.first()) {
            (None, None) => true,
            (Some('*'), _) => rec(&p[1..], s) || (!s.is_empty() && rec(p, &s[1..])),
            (Some('?'), Some(_)) => rec(&p[1..], &s[1..]),
            (Some(a), Some(b)) if a == b => rec(&p[1..], &s[1..]),
            _ => false,
        }
    }
    let (p, s): (Vec<char>, Vec<char>) = (pattern.chars().collect(), s.chars().collect());
    rec(&p, &s)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn glob() {
        assert!(glob_match("SPI_*", "SPI_MOSI"));
        assert!(glob_match("VBUS", "VBUS"));
        assert!(!glob_match("SPI_*", "I2C_SCL"));
        assert!(glob_match("*", "anything"));
    }
    #[test]
    fn roundtrip() {
        let m = ConstraintModel::default();
        let j = serde_json::to_string(&m).unwrap();
        let _: ConstraintModel = serde_json::from_str(&j).unwrap();
    }

    #[test]
    fn net_class_clearance_overrides_the_board_default_only_for_its_own_nets() {
        let mut rules = BoardRules { clearance: 200, ..BoardRules::default() };
        rules.net_classes.push(NetClass { name: "cc_escape".into(), nets: vec!["CC1".into(), "CC2".into()], track_width: Some(150), clearance: Some(150), via_diameter: None, via_drill: None, microvia_diameter: None, microvia_drill: None, diff_pair_width: None, diff_pair_gap: None, diff_pair_via_gap: None, priority: 0 });
        assert_eq!(rules.clearance_of("CC1"), 150);
        assert_eq!(rules.clearance_of("CC2"), 150);
        assert_eq!(rules.clearance_of("GND"), 200, "a net outside the class keeps the board default");
        assert!(rules.validate().is_empty(), "{:?}", rules.validate());
    }

    #[test]
    fn diff_pair_resolvers_fall_back_to_upstreams_own_sizes_settings_defaults() {
        let rules = BoardRules::default();
        // No net class at all -- SIZES_SETTINGS's own hardcoded constructor
        // defaults (pns_sizes_settings.h): 125um width, 180um gap, via gap
        // same as trace gap.
        assert_eq!(rules.diff_pair_width_of("USB_DP"), 125);
        assert_eq!(rules.diff_pair_gap_of("USB_DP"), 180);
        assert_eq!(rules.diff_pair_via_gap_of("USB_DP"), 180);
    }

    #[test]
    fn a_net_classs_own_diff_pair_fields_override_the_defaults() {
        let mut rules = BoardRules::default();
        rules.net_classes.push(NetClass { name: "usb".into(), nets: vec!["USB_*".into()], track_width: None, clearance: None, via_diameter: None, via_drill: None, microvia_diameter: None, microvia_drill: None, diff_pair_width: Some(200), diff_pair_gap: Some(150), diff_pair_via_gap: Some(300), priority: 0 });
        assert_eq!(rules.diff_pair_width_of("USB_DP"), 200);
        assert_eq!(rules.diff_pair_gap_of("USB_DP"), 150);
        assert_eq!(rules.diff_pair_via_gap_of("USB_DP"), 300);
        // A net outside the class keeps the upstream defaults, same as
        // every other per-class resolver.
        assert_eq!(rules.diff_pair_width_of("GND"), 125);
    }

    #[test]
    fn diff_pair_via_gap_falls_back_to_the_classs_own_trace_gap_not_the_180um_literal() {
        let mut rules = BoardRules::default();
        rules.net_classes.push(NetClass { name: "usb".into(), nets: vec!["USB_*".into()], track_width: None, clearance: None, via_diameter: None, via_drill: None, microvia_diameter: None, microvia_drill: None, diff_pair_width: None, diff_pair_gap: Some(150), diff_pair_via_gap: None, priority: 0 });
        assert_eq!(rules.diff_pair_via_gap_of("USB_DP"), 150, "SIZES_SETTINGS::DiffPairViaGap()'s own \"same as trace gap\" default");
    }

    #[test]
    fn a_negative_net_class_clearance_fails_validation() {
        let mut rules = BoardRules::default();
        rules.net_classes.push(NetClass { name: "bad".into(), nets: vec!["X".into()], track_width: None, clearance: Some(-1), via_diameter: None, via_drill: None, microvia_diameter: None, microvia_drill: None, diff_pair_width: None, diff_pair_gap: None, diff_pair_via_gap: None, priority: 0 });
        let fails = rules.validate();
        assert!(fails.iter().any(|c| c.location.as_deref() == Some("board.net_classes")), "{fails:?}");
    }
}
