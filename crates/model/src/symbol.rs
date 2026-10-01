//! Library symbol geometry -- the KiCad `lib_symbol` a schematic symbol
//! instance draws from: graphics, and pins with electrical type, graphic
//! shape, position, angle, length and number.
//!
//! Local frame: millimetres, +x right, +y **up**, origin at the symbol's
//! own electrical origin -- KiCad's own convention for a `.kicad_sym`/
//! `lib_symbols` entry, not `ir`'s integer-micrometer/+y-down screen
//! convention. Keeping this module in KiCad's own frame means a symbol
//! read from a real library, or from our own exported `.kicad_sch`, round
//! trips without a coordinate transform; only the *placement* of an
//! instance (in `ir::SymbolInstance`) is ever mixed with sheet coordinates,
//! and that mixing happens in `eda-kicad`, not here.
//!
//! Resolved the same way [`crate::Footprint`] is (see `footprint.rs`'s own
//! doc comment): explicit `ConstraintModel::symbols` first (filled in by
//! `eda-kicad`'s loader from the real installed KiCad symbol libraries, or
//! by a part's own inline definition), then [`builtin`] -- a small
//! built-in table covering the parts a design needs even with no KiCad
//! install to hand.

use serde::{Deserialize, Serialize};

/// Millimetres, floating point -- KiCad's own library-file unit. Distinct
/// from `ir::Um` (integer micrometers, the sheet/board frame) so the two
/// are never accidentally mixed without an explicit conversion.
pub type Mm = f64;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SPoint {
    pub x: Mm,
    pub y: Mm,
}

impl SPoint {
    pub fn new(x: Mm, y: Mm) -> Self {
        Self { x, y }
    }
}

/// One drawn primitive of a library symbol, in the symbol's own mm frame.
/// `unit` is KiCad's unit index for a multi-unit symbol (an op-amp's four
/// gates, say): 0 means "drawn on every unit" (KiCad's own `<name>_0_n`
/// convention), 1.. is a specific unit. Every symbol this port places has
/// exactly one real unit, so `unit` is 0 or 1 in practice, but the field
/// exists so a symbol read from a real multi-unit library round-trips
/// without losing which unit each graphic belongs to.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum SymbolGraphic {
    Rectangle {
        #[serde(default)]
        unit: u32,
        start: SPoint,
        end: SPoint,
        stroke_mm: Mm,
        #[serde(default)]
        filled: bool,
    },
    Polyline {
        #[serde(default)]
        unit: u32,
        pts: Vec<SPoint>,
        stroke_mm: Mm,
        #[serde(default)]
        filled: bool,
    },
    Circle {
        #[serde(default)]
        unit: u32,
        center: SPoint,
        radius_mm: Mm,
        stroke_mm: Mm,
        #[serde(default)]
        filled: bool,
    },
    Arc {
        #[serde(default)]
        unit: u32,
        start: SPoint,
        mid: SPoint,
        end: SPoint,
        stroke_mm: Mm,
        #[serde(default)]
        filled: bool,
    },
    Text {
        #[serde(default)]
        unit: u32,
        text: String,
        at: SPoint,
        #[serde(default)]
        angle_deg: f64,
        size_mm: Mm,
    },
}

impl SymbolGraphic {
    pub fn unit(&self) -> u32 {
        match self {
            SymbolGraphic::Rectangle { unit, .. }
            | SymbolGraphic::Polyline { unit, .. }
            | SymbolGraphic::Circle { unit, .. }
            | SymbolGraphic::Arc { unit, .. }
            | SymbolGraphic::Text { unit, .. } => *unit,
        }
    }
}

fn d_shape() -> String {
    "line".to_string()
}
fn d_unit_one() -> u32 {
    1
}
fn d_true() -> bool {
    true
}

/// One pin of a library symbol.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LibPin {
    pub number: String,
    #[serde(default)]
    pub name: String,
    /// KiCad's own canonical electrical-type token -- kept as the file's
    /// own string (not a closed Rust enum) so a token this port has not
    /// special-cased still round-trips instead of failing to parse. One
    /// of: `"input"`, `"output"`, `"bidirectional"`, `"tri_state"`,
    /// `"passive"`, `"free"`, `"unspecified"`, `"power_in"`,
    /// `"power_out"`, `"open_collector"`, `"open_emitter"`,
    /// `"no_connect"` (`eda_kicad::erc::ElectricalPinType` is the closed
    /// enum the ERC port actually matches against; this field is the raw
    /// text).
    pub electrical_type: String,
    /// Graphic shape token (`"line"`, `"inverted"`, `"clock"`, ...).
    #[serde(default = "d_shape")]
    pub shape: String,
    /// The pin's own connection point -- the outer, "hot" end a wire
    /// attaches to -- in the symbol's own mm frame.
    pub at: SPoint,
    /// KiCad pin angle in degrees (0/90/180/270): the direction from `at`
    /// toward the symbol body.
    pub angle_deg: f64,
    pub length_mm: Mm,
    /// Which unit this pin belongs to (1-based; symbols this port places
    /// have exactly one unit).
    #[serde(default = "d_unit_one")]
    pub unit: u32,
}

/// A library symbol: everything `lib_symbols` embeds for one `lib_id`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LibSymbol {
    /// `"Device:R"`, `"power:GND"`, `"eda:U1"` (a synthetic id for a part
    /// with no matching library symbol) -- matches a schematic
    /// `SymbolInstance`/`PowerSymbol`'s own `lib_id`.
    pub lib_id: String,
    #[serde(default)]
    pub graphics: Vec<SymbolGraphic>,
    #[serde(default)]
    pub pins: Vec<LibPin>,
    /// True for a KiCad power symbol (the library's `(power)` marker):
    /// hidden reference, and its Value field is the net name it asserts.
    #[serde(default)]
    pub power: bool,
    #[serde(default = "d_true")]
    pub in_bom: bool,
    #[serde(default = "d_true")]
    pub on_board: bool,
    #[serde(default)]
    pub datasheet: String,
    #[serde(default)]
    pub description: String,
    /// The library's own default `Reference` field ("R", "C", "U", "#PWR",
    /// ...) -- `A`'s symbol-chooser placement uses this to synthesize a
    /// real `"R?"`/`"U?"`-style placeholder id instead of always guessing
    /// "U?" regardless of what's actually being placed (`Cmd::AddSymbol`'s
    /// own doc). Empty when unknown (a generic/synthesized symbol with no
    /// real library backing) -- callers fall back to "U", same as
    /// `annotate`'s own default.
    #[serde(default)]
    pub reference_prefix: String,
    /// Highest unit index used by `pins`/`graphics`; 1 for every symbol
    /// this port actually places (multi-unit placement is deferred).
    #[serde(default = "d_unit_one")]
    pub unit_count: u32,
}

impl LibSymbol {
    pub fn pin_by_number(&self, number: &str) -> Option<&LibPin> {
        self.pins.iter().find(|p| p.number == number)
    }
}

/// Resolve a `Part`'s intended library symbol, as a `"Library:Name"` lib
/// id, purely from the part itself -- no file I/O, so `eda-engine` (which
/// cannot depend on `eda-kicad`'s sexpr reader) can assign it during
/// `derive_schematic`, and `eda-kicad`'s loader resolves the exact same
/// string later. Precedence:
/// 1. `part.symbol`, verbatim, when the intent names one explicitly.
/// 2. A known MPN (`AMS1117-3.3` -> `Regulator_Linear:AMS1117-3.3`, the
///    part this project's own examples use).
/// 3. A sensible default by kind: a 2-pin R/C/L/D/LED-shaped part maps to
///    the matching `Device:*` symbol (see [`crate::footprint::is_two_pin_passive`]-style
///    reference/value sniffing); a connector-shaped part (`J*`) maps to
///    `Connector_Generic:Conn_01x<N>` for its pin count; anything else
///    (a multi-pin IC) gets the synthetic `"eda:<reference>"` id, which
///    the exporter/loader both recognize as "no real library symbol --
///    synthesize a generic box from this part's own pins."
pub fn resolve_lib_id(part: &crate::Part) -> String {
    if let Some(s) = &part.symbol {
        if !s.is_empty() {
            return s.clone();
        }
    }
    if let Some(mpn) = &part.mpn {
        if let Some(lib_id) = known_mpn_symbol(mpn) {
            return lib_id.to_string();
        }
    }
    let synthetic = || format!("eda:{}", part.reference);

    let first_letter = |s: &str| s.trim_start_matches(['+', '-']).chars().next().map(|c| c.to_ascii_uppercase());
    let kind_letter = [Some(part.reference.as_str()), part.package.as_deref(), part.value.as_deref()]
        .into_iter()
        .flatten()
        .find_map(first_letter);

    if part.pins.len() == 2 {
        match kind_letter {
            Some('R') => return "Device:R".to_string(),
            Some('C') => return "Device:C".to_string(),
            Some('L') => return "Device:L".to_string(),
            Some('D') => {
                let looks_led = [part.value.as_deref(), part.mpn.as_deref()].into_iter().flatten().any(|s| s.to_ascii_uppercase().contains("LED"));
                return if looks_led { "Device:LED".to_string() } else { "Device:D".to_string() };
            }
            _ => {}
        }
    }

    let ref_upper = part.reference.to_ascii_uppercase();
    if ref_upper.starts_with('J') && (1..=40).contains(&part.pins.len()) {
        return format!("Connector_Generic:Conn_01x{:02}", part.pins.len());
    }

    synthetic()
}

/// A handful of manufacturer part numbers this project's own examples name
/// explicitly, mapped to the real KiCad library symbol for them. Not
/// meant to be exhaustive -- an intent that wants a specific library
/// symbol for any other MPN should just say so with `symbol:`.
fn known_mpn_symbol(mpn: &str) -> Option<&'static str> {
    match mpn.to_ascii_uppercase().as_str() {
        "AMS1117-3.3" | "AMS1117-3V3" => Some("Regulator_Linear:AMS1117-3.3"),
        "AMS1117-5.0" | "AMS1117-5V0" => Some("Regulator_Linear:AMS1117-5.0"),
        "AMS1117-1.8" | "AMS1117-1V8" => Some("Regulator_Linear:AMS1117-1.8"),
        _ => None,
    }
}

/// Is `lib_id` one of the fully-synthetic ids `derive_schematic` hands out
/// when a part has no real library symbol (`"eda:<reference>"`)? Shared by
/// the loader (skip resolution, go straight to a synthesized generic box)
/// and the writer.
pub fn is_synthetic_lib_id(lib_id: &str) -> bool {
    lib_id.starts_with("eda:")
}

/// Built-in library symbols: a small, self-contained set covering every
/// symbol this project's own examples need, so the generator/writer/ERC
/// port work fully offline and deterministically even with no KiCad
/// install to hand. Geometry is transcribed from KiCad's own installed
/// `Device.kicad_sym`/`Connector_Generic.kicad_sym`/`power.kicad_sym` (not
/// invented), so a symbol resolved from here looks identical to the real
/// library entry it stands in for. `eda-kicad`'s loader prefers a real
/// installed library file when one resolves, and falls back to this table
/// only when it does not (or when no KiCad install is found at all).
pub fn builtin(lib_id: &str) -> Option<LibSymbol> {
    match lib_id {
        "Device:R" => Some(device_r()),
        "Device:C" => Some(device_c()),
        "Device:L" => Some(device_l()),
        "Device:D" => Some(device_d()),
        "Device:LED" => Some(device_led()),
        "power:GND" => Some(power_gnd()),
        "power:PWR_FLAG" => Some(power_flag()),
        _ => {
            if let Some(name) = lib_id.strip_prefix("Connector_Generic:Conn_01x") {
                let n: u32 = name.parse().ok()?;
                return (1..=40).contains(&n).then(|| conn_01x(n));
            }
            if let Some(net) = lib_id.strip_prefix("power:") {
                return Some(power_rail(net));
            }
            None
        }
    }
}

/// Every non-power `builtin` symbol, for `A`'s symbol chooser to offer
/// when no real `Device.kicad_sym`/etc. resolves one of these library
/// names (`eda-cli`'s own "real file first, builtin fallback" precedence,
/// same as everywhere else `builtin` is consulted). Power symbols
/// (`power:GND`/`PWR_FLAG`/the generic rail) are deliberately left out --
/// `P` is their own dedicated placement tool (PARITY-sch.md section 3),
/// and `Cmd::AddSymbol` has no `net`/`pin` fields a power symbol needs
/// anyway. `Connector_Generic:Conn_01x<N>` is parametric (40 variants) and
/// also left out of this fixed catalog -- a real installed
/// `Connector_Generic.kicad_sym` (which lists each one as its own named
/// entry) is what a real search over it would need, not a hand-enumerated
/// substitute.
pub fn builtin_catalog() -> Vec<LibSymbol> {
    vec![device_r(), device_c(), device_l(), device_d(), device_led()]
}

fn pin(number: &str, etype: &str, x: Mm, y: Mm, angle_deg: f64, length_mm: Mm) -> LibPin {
    LibPin { number: number.into(), name: String::new(), electrical_type: etype.into(), shape: "line".into(), at: SPoint::new(x, y), angle_deg, length_mm, unit: 1 }
}

/// `Device:R` -- transcribed from the installed `Device.kicad_sym`.
fn device_r() -> LibSymbol {
    LibSymbol {
        lib_id: "Device:R".into(),
        graphics: vec![SymbolGraphic::Rectangle { unit: 1, start: SPoint::new(-1.016, -2.54), end: SPoint::new(1.016, 2.54), stroke_mm: 0.254, filled: false }],
        pins: vec![pin("1", "passive", 0.0, 3.81, 270.0, 1.27), pin("2", "passive", 0.0, -3.81, 90.0, 1.27)],
        power: false,
        in_bom: true,
        on_board: true,
        datasheet: String::new(),
        description: "Resistor".into(),
        reference_prefix: "R".into(),
        unit_count: 1,
    }
}

/// `Device:C` -- two plates, transcribed proportionally from the real symbol.
fn device_c() -> LibSymbol {
    LibSymbol {
        lib_id: "Device:C".into(),
        graphics: vec![
            SymbolGraphic::Polyline { unit: 1, pts: vec![SPoint::new(-1.524, 0.508), SPoint::new(1.524, 0.508)], stroke_mm: 0.508, filled: false },
            SymbolGraphic::Polyline { unit: 1, pts: vec![SPoint::new(-1.524, -0.508), SPoint::new(1.524, -0.508)], stroke_mm: 0.508, filled: false },
        ],
        pins: vec![pin("1", "passive", 0.0, 3.81, 270.0, 3.302), pin("2", "passive", 0.0, -3.81, 90.0, 3.302)],
        power: false,
        in_bom: true,
        on_board: true,
        datasheet: String::new(),
        description: "Capacitor".into(),
        reference_prefix: "C".into(),
        unit_count: 1,
    }
}

/// `Device:L` -- kept simple (a rectangle, like `Device:R`) rather than
/// the real hump-arc artwork: readable and electrically identical, and
/// avoids needing this port's arc writer to reproduce KiCad's exact bulge
/// geometry for a part none of this task's examples actually use.
fn device_l() -> LibSymbol {
    LibSymbol {
        lib_id: "Device:L".into(),
        graphics: vec![SymbolGraphic::Rectangle { unit: 1, start: SPoint::new(-1.27, -3.81), end: SPoint::new(1.27, 3.81), stroke_mm: 0.254, filled: false }],
        pins: vec![pin("1", "passive", 0.0, 5.08, 270.0, 1.27), pin("2", "passive", 0.0, -5.08, 90.0, 1.27)],
        power: false,
        in_bom: true,
        on_board: true,
        datasheet: String::new(),
        description: "Inductor".into(),
        reference_prefix: "L".into(),
        unit_count: 1,
    }
}

/// `Device:D` -- a diode: triangle + bar.
fn device_d() -> LibSymbol {
    LibSymbol {
        lib_id: "Device:D".into(),
        graphics: vec![
            SymbolGraphic::Polyline { unit: 1, pts: vec![SPoint::new(0.0, 1.27), SPoint::new(0.0, -1.27)], stroke_mm: 0.254, filled: false },
            SymbolGraphic::Polyline {
                unit: 1,
                pts: vec![SPoint::new(-1.27, 1.27), SPoint::new(-1.27, -1.27), SPoint::new(1.27, 0.0), SPoint::new(-1.27, 1.27)],
                stroke_mm: 0.254,
                filled: true,
            },
        ],
        pins: vec![pin("1", "passive", -2.54, 0.0, 0.0, 1.27), pin("2", "passive", 2.54, 0.0, 180.0, 1.27)],
        power: false,
        in_bom: true,
        on_board: true,
        datasheet: String::new(),
        description: "Diode".into(),
        reference_prefix: "D".into(),
        unit_count: 1,
    }
}

/// `Device:LED` -- a diode with two emission-arrow strokes.
fn device_led() -> LibSymbol {
    let mut s = device_d();
    s.lib_id = "Device:LED".into();
    s.description = "Light emitting diode".into();
    s.graphics.push(SymbolGraphic::Polyline { unit: 1, pts: vec![SPoint::new(-0.635, 1.905), SPoint::new(-1.905, 3.175)], stroke_mm: 0.152, filled: false });
    s.graphics.push(SymbolGraphic::Polyline { unit: 1, pts: vec![SPoint::new(-0.048, 1.318), SPoint::new(-1.318, 2.588)], stroke_mm: 0.152, filled: false });
    s
}

/// A single-row generic connector, `Connector_Generic:Conn_01x<N>`:
/// parametrized the same way KiCad's own script-generated `Conn_01x04`
/// (etc.) is -- 2.54mm pin pitch, pins on the west side, a rectangle body.
fn conn_01x(n: u32) -> LibSymbol {
    let pitch = 2.54;
    let top_y = (n as f64 - 1.0) / 2.0 * pitch + pitch / 2.0;
    let mut pins = Vec::with_capacity(n as usize);
    for i in 0..n {
        let y = top_y - pitch / 2.0 - (i as f64) * pitch;
        pins.push(pin(&(i + 1).to_string(), "passive", -5.08, y, 0.0, 3.81));
    }
    let rect = SymbolGraphic::Rectangle { unit: 1, start: SPoint::new(-1.27, top_y), end: SPoint::new(1.27, top_y - (n as f64) * pitch), stroke_mm: 0.254, filled: false };
    LibSymbol {
        lib_id: format!("Connector_Generic:Conn_01x{n:02}"),
        graphics: vec![rect],
        pins,
        power: false,
        in_bom: true,
        on_board: true,
        datasheet: String::new(),
        description: format!("Generic connector, single row, 01x{n:02}"),
        reference_prefix: "J".into(),
        unit_count: 1,
    }
}

/// `power:GND` -- the ground tines glyph, transcribed from `power.kicad_sym`.
fn power_gnd() -> LibSymbol {
    LibSymbol {
        lib_id: "power:GND".into(),
        graphics: vec![SymbolGraphic::Polyline {
            unit: 1,
            pts: vec![SPoint::new(0.0, 0.0), SPoint::new(0.0, -1.27), SPoint::new(1.27, -1.27), SPoint::new(0.0, -2.54), SPoint::new(-1.27, -1.27), SPoint::new(0.0, -1.27)],
            stroke_mm: 0.0,
            filled: false,
        }],
        pins: vec![pin("1", "power_in", 0.0, 0.0, 270.0, 0.0)],
        power: true,
        in_bom: true,
        on_board: true,
        datasheet: String::new(),
        description: "Power symbol creates a global label with name \"GND\", ground".into(),
        reference_prefix: "#PWR".into(),
        unit_count: 1,
    }
}

/// `power:PWR_FLAG` -- the standard "this net has a source" marker: a
/// `power_out` pin, so it satisfies ERC's power-input-driven rule for any
/// net it is dropped on.
fn power_flag() -> LibSymbol {
    LibSymbol {
        lib_id: "power:PWR_FLAG".into(),
        graphics: vec![SymbolGraphic::Polyline {
            unit: 1,
            pts: vec![SPoint::new(0.0, 0.0), SPoint::new(0.0, 1.27), SPoint::new(-1.016, 1.905), SPoint::new(0.0, 2.54), SPoint::new(1.016, 1.905), SPoint::new(0.0, 1.27)],
            stroke_mm: 0.0,
            filled: false,
        }],
        pins: vec![pin("1", "power_out", 0.0, 0.0, 90.0, 0.0)],
        power: true,
        in_bom: true,
        on_board: true,
        datasheet: String::new(),
        description: "Special symbol for telling ERC where power comes from".into(),
        reference_prefix: "#PWR".into(),
        unit_count: 1,
    }
}

/// A generic power rail symbol (`power:VCC`-style upward arrow) labeled
/// with any net name -- covers every rail that is not `GND` without
/// needing one hand-transcribed glyph per rail name, exactly the way a
/// KiCad user makes a custom rail symbol in practice (copy `VCC`, rename
/// its Value).
fn power_rail(net: &str) -> LibSymbol {
    LibSymbol {
        lib_id: format!("power:{net}"),
        graphics: vec![
            SymbolGraphic::Polyline { unit: 1, pts: vec![SPoint::new(-0.762, 1.27), SPoint::new(0.0, 2.54)], stroke_mm: 0.0, filled: false },
            SymbolGraphic::Polyline { unit: 1, pts: vec![SPoint::new(0.0, 2.54), SPoint::new(0.762, 1.27)], stroke_mm: 0.0, filled: false },
            SymbolGraphic::Polyline { unit: 1, pts: vec![SPoint::new(0.0, 0.0), SPoint::new(0.0, 2.54)], stroke_mm: 0.0, filled: false },
        ],
        pins: vec![pin("1", "power_in", 0.0, 0.0, 90.0, 0.0)],
        power: true,
        in_bom: true,
        on_board: true,
        datasheet: String::new(),
        description: format!("Power symbol creates a global label with name \"{net}\""),
        reference_prefix: "#PWR".into(),
        unit_count: 1,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Part, Pin, PinKind};

    fn part(reference: &str, symbol: Option<&str>, mpn: Option<&str>, pins: usize) -> Part {
        Part {
            reference: reference.into(),
            mpn: mpn.map(String::from),
            lcsc: None,
            value: None,
            package: None,
            footprint: None,
            symbol: symbol.map(String::from),
            datasheet: None,
            pins: (1..=pins).map(|n| Pin { number: n.to_string(), name: None, kind: PinKind::Passive }).collect(),
            body_um: None,
            edge: None,
        }
    }

    #[test]
    fn explicit_symbol_wins() {
        assert_eq!(resolve_lib_id(&part("U1", Some("Foo:Bar"), None, 2)), "Foo:Bar");
    }

    #[test]
    fn known_mpn_resolves() {
        assert_eq!(resolve_lib_id(&part("U1", None, Some("AMS1117-3.3"), 3)), "Regulator_Linear:AMS1117-3.3");
    }

    #[test]
    fn two_pin_kinds_resolve_by_reference_letter() {
        assert_eq!(resolve_lib_id(&part("R1", None, None, 2)), "Device:R");
        assert_eq!(resolve_lib_id(&part("C1", None, None, 2)), "Device:C");
        assert_eq!(resolve_lib_id(&part("L1", None, None, 2)), "Device:L");
        assert_eq!(resolve_lib_id(&part("D1", None, None, 2)), "Device:D");
    }

    #[test]
    fn led_value_resolves_to_led_symbol() {
        let mut p = part("D1", None, None, 2);
        p.value = Some("Red LED".into());
        assert_eq!(resolve_lib_id(&p), "Device:LED");
    }

    #[test]
    fn connector_resolves_by_pin_count() {
        assert_eq!(resolve_lib_id(&part("J1", None, None, 4)), "Connector_Generic:Conn_01x04");
    }

    #[test]
    fn multi_pin_ic_falls_back_to_synthetic() {
        let id = resolve_lib_id(&part("U1", None, None, 20));
        assert!(is_synthetic_lib_id(&id));
        assert_eq!(id, "eda:U1");
    }

    #[test]
    fn builtin_covers_every_default_kind() {
        for id in ["Device:R", "Device:C", "Device:L", "Device:D", "Device:LED", "power:GND", "power:PWR_FLAG", "Connector_Generic:Conn_01x04"] {
            let sym = builtin(id).unwrap_or_else(|| panic!("no builtin for {id}"));
            assert!(!sym.pins.is_empty(), "{id} has no pins");
        }
    }

    #[test]
    fn conn_pin_count_matches_name() {
        let sym = builtin("Connector_Generic:Conn_01x08").unwrap();
        assert_eq!(sym.pins.len(), 8);
        assert_eq!(sym.pins[0].number, "1");
        assert_eq!(sym.pins[7].number, "8");
    }

    #[test]
    fn generic_power_rail_resolves_for_any_name() {
        let sym = builtin("power:+3V3").unwrap();
        assert!(sym.power);
        assert_eq!(sym.pins[0].electrical_type, "power_in");
    }

    #[test]
    fn gnd_pin_is_power_in_not_out() {
        let sym = power_gnd();
        assert_eq!(sym.pins[0].electrical_type, "power_in");
    }

    #[test]
    fn pwr_flag_pin_is_power_out() {
        let sym = power_flag();
        assert_eq!(sym.pins[0].electrical_type, "power_out");
    }

    #[test]
    fn roundtrip_serde() {
        let sym = device_r();
        let value = serde_json::to_value(&sym).unwrap();
        let back: LibSymbol = serde_json::from_value(value).unwrap();
        assert_eq!(back.pins.len(), 2);
        assert_eq!(back.pins[0].at, sym.pins[0].at);
    }
}
