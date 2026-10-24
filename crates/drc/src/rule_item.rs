//! A board item as the rule engine sees it (`BOARD_ITEM` for `DRC_ENGINE::EvalRules` and the expression language): its class, the layers it is on, its net,
//! the properties an expression can read (the names and the labels of the Properties panel's registry, `kicad-port/pcbProperties.ts`), the shapes it occupies
//! and the facts of the footprint it belongs to. [`crate::rule_board`] builds them from a design.
//!
//! Lengths are nanometres here, as in KiCad and the studio: `A.Width > 0.25mm` compares nanometres.

use crate::kimath::Shape;
use eda_model::ir::Point;
use std::collections::HashMap;
use std::rc::Rc;

/// What a property holds, as the language reads it (`PCBEXPR_VAR_REF::GetValue`).
#[derive(Debug, Clone, PartialEq)]
pub enum Prop {
    /// A length in nanometres, an area in nm^2, or a plain integer.
    Int(i64),
    /// A real number (an angle in degrees, a ratio).
    Double(f64),
    Bool(bool),
    Str(String),
    /// A choice: its label (`ENUM_MAP`).
    Enum(String),
    /// A layer: its name in the board file.
    Layer(String),
}

/// The class of an item (`TYPE_HASH( *item )`): what KiCad's property manager keys the properties of an item by.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Class {
    Footprint,
    Pad,
    Track,
    Arc,
    Via,
    Zone,
    Text,
    Field,
    Shape,
    Dimension,
    Leader,
    Group,
}

impl Class {
    /// `ENUM_MAP<KICAD_T>`: the name `A.Type` has.
    pub fn type_name(self) -> &'static str {
        match self {
            Class::Footprint => "Footprint",
            Class::Pad => "Pad",
            Class::Track | Class::Arc => "Track",
            Class::Via => "Via",
            Class::Zone => "Zone",
            Class::Text | Class::Field => "Text",
            Class::Shape => "Graphic",
            Class::Dimension => "Dimension",
            Class::Leader => "Leader",
            Class::Group => "Group",
        }
    }

    /// A `BOARD_CONNECTED_ITEM`: it has a net and a net class.
    pub fn is_connected(self) -> bool {
        matches!(self, Class::Pad | Class::Track | Class::Arc | Class::Via | Class::Zone | Class::Shape)
    }
}

/// What the language asks of the footprint an item belongs to, or of a footprint item itself.
#[derive(Debug, Clone, Default)]
pub struct FootprintFacts {
    pub reference: String,
    /// `GetFPIDAsString`: "Library:Name", or the bare name of a built-in footprint.
    pub fpid: String,
    /// The fields by name (`GetField( name )`).
    pub fields: Vec<(String, String)>,
    /// On the back side (`IsFlipped`).
    pub flipped: bool,
    /// `GetCourtyard( F_CrtYd )` and `B_CrtYd` as closed outlines in board space; empty = none.
    pub courtyard_front: Vec<Vec<Point>>,
    pub courtyard_back: Vec<Vec<Point>>,
    /// The sheet the footprint came from (`GetSheetname`); the studio's footprints are not given one.
    pub sheet: String,
}

/// The local settings an item overrides the rules with (`GetClearanceOverrides`, `GetZoneConnectionOverrides`, `GetLocalThermalGapOverride`,
/// `GetLocalSpokeWidthOverride`, `GetLocalSolderMaskMargin`, `GetLocalSolderPasteMargin(Ratio)`): the value and where it comes from ("pad", "footprint R1", "zone").
/// Lengths are nanometres.
#[derive(Debug, Clone, Default)]
pub struct Overrides {
    pub clearance: Option<(i64, String)>,
    pub zone_connection: Option<(crate::constraints::ZoneConnection, String)>,
    pub thermal_gap: Option<(i64, String)>,
    pub spoke_width: Option<(i64, String)>,
    pub solder_mask_margin: Option<i64>,
    pub solder_paste_margin: Option<i64>,
    pub solder_paste_ratio: Option<f64>,
}

/// A zone as an area (`searchAreas`): a rule area or a copper zone, by name and id, with its outline.
#[derive(Debug, Clone)]
pub struct AreaFacts {
    pub id: String,
    pub name: String,
    pub layers: Vec<String>,
    pub outline: Vec<Point>,
    /// The holes of the outline (`ZONE::Outline()`'s holes): nothing inside one is in the area.
    pub holes: Vec<Vec<Point>>,
    pub parent_footprint: Option<String>,
}

#[derive(Debug, Clone)]
pub struct RuleItem {
    pub id: String,
    pub class: Class,
    /// `GetLayerSet()`, in the board's order (front copper first).
    pub layers: Vec<String>,
    /// The net of a connected item, "" for none (net 0).
    pub net: String,
    /// The properties by lower-cased name, with blanks (`pad type`).
    pub props: HashMap<String, Prop>,
    /// The footprint the item belongs to (`GetParentFootprint`), a footprint's own facts for a footprint.
    pub footprint: Option<Rc<FootprintFacts>>,
    /// The names of the groups the item is in, innermost first (an item of a footprint counts the footprint's groups too).
    pub groups: Vec<String>,
    /// The copper or other shape the item occupies, board space (`GetEffectiveShape`); empty for what has none.
    pub shapes: Vec<Shape>,
    /// A drilled hole's shape (`GetEffectiveHoleShape`), for a pad or a via.
    pub hole: Option<Shape>,
    pub position: Point,
    /// `GetItemDescription( units, true )`.
    pub description: String,
    /// A rule area's flags, for `Class::Zone`.
    pub rule_area: bool,
    /// The item's own overrides of the rules (`GetClearanceOverrides` and the pad's other local settings).
    pub overrides: Overrides,
    /// `GetLocalClearance( &source )`: a copper zone's own clearance, ...
    pub local_clearance: Option<(i64, String)>,
    /// The footprint an item of it belongs to, as an item (`GetParentFootprint`).
    pub parent: Option<Rc<RuleItem>>,
    /// `GetLayer()`.
    pub layer: Option<String>,
    /// A zone that is a rule area with keepout parameters (`isKeepoutZone( item, false )`).
    pub keepout: bool,
    /// The nets (by name, "" for none) an item of a net-tie footprint may touch (`FOOTPRINT::GetNetTieCache( item )`).
    pub net_tie: Vec<String>,
    /// `IsTented( F_Mask )` and `IsTented( B_Mask )`.
    pub tented: [bool; 2],
    /// A zone's pad connection (`GetPadConnection`), its thermal relief gap and spoke width, and its minimum thickness: nanometres.
    pub zone_pad_connection: Option<crate::constraints::ZoneConnection>,
    pub zone_thermal_gap: i64,
    pub zone_spoke_width: i64,
    pub zone_min_thickness: i64,
    /// A pad's attribute is PTH, NPTH, SMD or connector; its drill size (`GetDrillSize().x`), nanometres.
    pub pad_attribute: &'static str,
    pub drill: i64,
    /// A text or field's height and thickness, nanometres.
    pub text_height: i64,
    pub text_thickness: i64,
    /// `PAD::FlashLayer( layer )`: the layers it is flashed on.
    pub flashed: Vec<String>,
    /// A via is micro, blind, buried or through: `Via Type`.
    pub via_type: Option<&'static str>,
    pub plated: bool,
    /// `HasDrilledHole` / `HasHole`.
    pub has_hole: bool,
    pub has_drilled_hole: bool,
    /// The area outline of a zone or rule area (for `intersectsArea` on it).
    pub outline: Vec<Point>,
    /// The holes of that outline.
    pub holes: Vec<Vec<Point>>,
    /// `IsTeardropArea`.
    pub teardrop: bool,
}

impl RuleItem {
    pub fn new(id: impl Into<String>, class: Class) -> RuleItem {
        RuleItem {
            id: id.into(),
            class,
            layers: Vec::new(),
            net: String::new(),
            props: HashMap::new(),
            footprint: None,
            groups: Vec::new(),
            shapes: Vec::new(),
            hole: None,
            position: Point { x: 0, y: 0 },
            description: String::new(),
            rule_area: false,
            overrides: Overrides::default(),
            local_clearance: None,
            parent: None,
            layer: None,
            keepout: false,
            net_tie: Vec::new(),
            tented: [false; 2],
            zone_pad_connection: None,
            zone_thermal_gap: 0,
            zone_spoke_width: 0,
            zone_min_thickness: 0,
            pad_attribute: "",
            drill: 0,
            text_height: 0,
            text_thickness: 0,
            flashed: Vec::new(),
            via_type: None,
            plated: false,
            has_hole: false,
            has_drilled_hole: false,
            outline: Vec::new(),
            holes: Vec::new(),
            teardrop: false,
        }
    }

    pub fn on_layer(&self, layer: &str) -> bool {
        self.layers.iter().any(|l| l == layer)
    }

    pub fn is_connected(&self) -> bool {
        self.class.is_connected()
    }

    /// `IsOnCopperLayer`.
    pub fn on_copper(&self) -> bool {
        self.layers.iter().any(|l| l.ends_with(".Cu"))
    }

    /// The property `name` (as the registry spells it, case ignored).
    pub fn prop(&self, name: &str) -> Option<&Prop> {
        self.props.get(&name.to_lowercase())
    }

    pub fn set(&mut self, name: &str, value: Prop) {
        self.props.insert(name.to_lowercase(), value);
    }
}
