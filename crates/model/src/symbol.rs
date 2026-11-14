//! Library symbol geometry -- the KiCad `lib_symbol` a schematic symbol
//! instance draws from: graphics, and pins with electrical type, graphic
//! shape, position, angle, length and number.
//!
//! Local frame: millimetres, +x right, +y **up**, origin at the symbol's
//! own electrical origin -- KiCad's own convention for a `.kicad_sym`/
//! `lib_symbols` entry, not `ir`'s integer-nanometre/+y-down screen
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
/// from `ir::Nm` (integer nanometres, the sheet/board frame) so the two
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
/// convention), 1.. is a specific unit -- see `ir::SchematicSection::symbols`'s
/// own doc for how a real multi-unit part's several placed instances each
/// pick out their own unit's graphics/pins by this field.
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

/// `DEFAULT_PIN_NAME_OFFSET`: 20 mils.
pub const DEFAULT_PIN_NAME_OFFSET_MM: f64 = 0.508;

fn d_pin_name_offset() -> f64 {
    DEFAULT_PIN_NAME_OFFSET_MM
}

fn is_default_pin_name_offset(v: &f64) -> bool {
    (*v - DEFAULT_PIN_NAME_OFFSET_MM).abs() < 1e-9
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
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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
    /// `"no_connect"` (the closed set kicad-cli's ERC matches against;
    /// this field is the raw text).
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
    /// Which unit this pin belongs to (1-based; 0 means common to every
    /// unit) -- see `ir::SchematicSection::symbols`'s own doc.
    #[serde(default = "d_unit_one")]
    pub unit: u32,
    /// `(hide yes)`: the pin is not drawn (`SCH_PIN::IsVisible`). An invisible power input of a symbol that is not a power symbol is a global
    /// power pin, the net of its name (`SCH_PIN::IsGlobalPower`, "legacy support": the hidden VCC and GND pins of 74xx and memory symbols).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub hidden: bool,
}

/// The other body of a library symbol that has two body styles (KiCad's "De Morgan" one, `LIB_SYMBOL::HasAlternateBodyStyle`): the `graphics` and `pins` a placed
/// symbol draws in the style the symbol itself is not in. In a `.kicad_sym` they are the `Name_<unit>_2` sub-blocks; the items both styles draw (`Name_<unit>_0`) are
/// in both bodies, so each is complete for its style.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AlternateBody {
    /// The body style these `graphics` and `pins` are of: 2, the alternate one, in a library symbol as it is read. 1 in a symbol that [`LibSymbol::in_style`] has put in
    /// style 2: its own `graphics` and `pins` are then the alternate body and this is the normal one it keeps, so the symbol's box (both bodies') does not depend on the
    /// style it is in and the symbol does not move when its style does.
    #[serde(default = "d_alternate_style", skip_serializing_if = "is_alternate_style")]
    pub style: u32,
    #[serde(default)]
    pub graphics: Vec<SymbolGraphic>,
    #[serde(default)]
    pub pins: Vec<LibPin>,
}

fn d_alternate_style() -> u32 {
    2
}
fn is_alternate_style(s: &u32) -> bool {
    *s == 2
}

impl AlternateBody {
    /// The alternate body (style 2) with these items.
    pub fn new(graphics: Vec<SymbolGraphic>, pins: Vec<LibPin>) -> Self {
        AlternateBody { style: 2, graphics, pins }
    }

    /// The body of style `style` with these items (a third or later custom body style, [`LibConnectivity::extra_bodies`]).
    pub fn of_style(style: u32, graphics: Vec<SymbolGraphic>, pins: Vec<LibPin>) -> Self {
        AlternateBody { style, graphics, pins }
    }
}

/// A library symbol: everything `lib_symbols` embeds for one `lib_id`.
///
/// `graphics` and `pins` are the symbol's normal body style (style 1: the `Name_<unit>_1` sub-blocks and the shared `_0` ones); a symbol that has an
/// alternate one keeps it in [`alternate`](Self::alternate). [`in_style`](Self::in_style) is the symbol as a placed one in a given style draws it.
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
    /// Highest unit index used by `pins`/`graphics`; 1 for a single-unit
    /// symbol. A real multi-unit library symbol (quad op-amp, logic gate
    /// array, ...) can place up to this many separate `SymbolInstance`s
    /// sharing one reference -- see `ir::SchematicSection::symbols`'s own
    /// doc.
    #[serde(default = "d_unit_one")]
    pub unit_count: u32,
    /// `(pin_names (hide yes))`: the symbol draws no pin names (`Connector_Generic:Conn_01xNN`, the power symbols).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub pin_names_hidden: bool,
    /// `(pin_numbers (hide yes))`: the symbol draws no pin numbers (`Device:R`, `C`, `L`, `D`, `LED`, the power symbols).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub pin_numbers_hidden: bool,
    /// `(pin_names (offset x))`, mm: the pin names sit inside the body, this far from the end of the pin; 0 draws them over
    /// the pin line. KiCad's default when the symbol says nothing is 0.508 (`DEFAULT_PIN_NAME_OFFSET`, 20 mils).
    #[serde(default = "d_pin_name_offset", skip_serializing_if = "is_default_pin_name_offset")]
    pub pin_name_offset_mm: f64,
    /// The alternate ("De Morgan") body style: what a placed symbol in style 2 draws. `None` for a symbol with one body style, which every symbol was
    /// until this field existed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alternate: Option<Box<AlternateBody>>,
    /// What the connectivity engine reads of the symbol beyond its pins (see [`LibConnectivity`]).
    #[serde(default, skip_serializing_if = "LibConnectivity::is_default")]
    pub connectivity: LibConnectivity,
}

/// What KiCad 25ec2335's connectivity engine reads of a library symbol beyond its pins (`conn_facts.cpp`, `SCH_PIN::IsLocalPower`): whether a power symbol is a
/// local one, and which of its pins are jumpered together. Read from `(power local)`, `(duplicate_pin_numbers_are_jumpers yes)` and `(jumper_pin_groups ..)` of a
/// library or a schematic's cache, or taken from the design library's symbol (`LibrarySymbol::power_local`, `LibrarySymbolKeep`).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LibConnectivity {
    /// `(power local)` (`LIB_SYMBOL::IsLocalPower`): a power symbol whose pin names its net on its own sheet only (`PRIORITY::LOCAL_POWER_PIN`, `SCOPE::SHEET`).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub power_local: bool,
    /// `(duplicate_pin_numbers_are_jumpers yes)` (`LIB_SYMBOL::GetDuplicatePinNumbersAreJumpers`): every pin of one number is one node wherever it sits.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub duplicate_pin_numbers_are_jumpers: bool,
    /// `(jumper_pin_groups ("1" "2") ..)` (`LIB_SYMBOL::JumperPinGroups`): the pins of each group, by number, are one node.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub jumper_pin_groups: Vec<Vec<String>>,
    /// The alternate functions of the pins, by pin number (`(pin .. (alternate "name" <type> <shape>))`, `SCH_PIN::GetAlternates`), each pin's sorted by
    /// name as KiCad's `std::map` keeps them. A placed symbol picks one per pin (`SchExtras::pin_alternates`), and the pin is then of the alternate's
    /// electrical type ([`LibSymbol::placed_pin_type`]). Kept here, not on [`LibPin`], so the pin's literals stay as they are.
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub pin_alternates: std::collections::BTreeMap<String, Vec<crate::ir::PinAlternate>>,
    /// The units' display names (`(unit_name "..")` of a unit's sub-symbol, `LIB_SYMBOL::GetUnitDisplayNames`), by unit number. KiCad 25ec2335 names a unit
    /// by it in place of its letter ([`LibSymbol::sub_reference`]), so a net's default name has it (`unconnected-(A1TopLeft_UnderSide-+3.3V-PadJ1.1)`, r6 B6-H1).
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub unit_names: std::collections::BTreeMap<u32, String>,
    /// The library symbol's user fields (`SCH_FIELD`s beyond the mandatory five and the `ki_` keys), name and text, in their order. Not connectivity: the
    /// schematic's cache writes them (`SCH_IO_KICAD_SEXPR_LIB_CACHE::SaveSymbol`), and a body text of the symbol resolves `${LOCATION}` by them
    /// (`LIB_SYMBOL::ResolveTextVar`). Kept here, as [`Self::pin_alternates`] is, so the symbol's literals stay as they are.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub user_fields: Vec<(String, String)>,
    /// The library symbol's keywords (`LIB_SYMBOL::GetKeyWords`, `(property "ki_keywords" ..)`), which the schematic's cache writes back
    /// (`SCH_IO_KICAD_SEXPR_LIB_CACHE::saveDcmInfoAsFields`, r7 H3). Not connectivity; kept here as [`Self::user_fields`] is.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub keywords: String,
    /// The library symbol's footprint filters as the file has them (`(property "ki_fp_filters" ..)`: `GetFPFilters` joined by spaces, a space in a filter
    /// escaped), written back the same way (r7 H3).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub fp_filters: String,
    /// The pins' name and number text sizes, mm (`SCH_PIN::m_nameTextSize`, `m_numTextSize`: a pin's `(name .. (effects (font (size ..))))` and `(number ..)`), by
    /// pin number, where either is not 50 mil (1.27). Compare Symbol with Library compares them (`PIN_COMPARISON_VIEW::Compare`, UI-LEFT-3 S3-22). Kept here, as
    /// [`Self::pin_alternates`] is, so the pin's literals stay as they are.
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub pin_text_sizes: std::collections::BTreeMap<String, (f64, f64)>,
    /// `(in_pos_files no)` (`LIB_SYMBOL::GetExcludedFromPosFiles`, "Exclude from position files"), which Compare Symbol with Library compares (UI-LEFT-3 S3-22).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub exclude_from_pos_files: bool,
    /// The library symbol's text boxes (`SCH_TEXTBOX`es of its draw items, `(text_box "text" ..)`), each a [`crate::ir::LibrarySymbolGraphic::TextBox`] in the symbol's mm
    /// with Y up, of its unit and body style (0: every one), none private to the Symbol Editor (`SCH_PAINTER::draw( SCH_TEXTBOX )` returns for one on a sheet). Not
    /// connectivity: [`LibSymbol::graphics`] has each box's border and fill as a rectangle ([`crate::ir::LibrarySymbolGraphic::to_engine_graphics`]), and this list is
    /// the rest of it -- the text a placed symbol draws in the box (`crate::sym_textbox`) and the `(text_box ..)` the schematic's cache writes in place of that
    /// rectangle. Kept here, as [`Self::pin_alternates`] is, so the symbol's literals and `SymbolGraphic`'s matches stay as they are.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub text_boxes: Vec<crate::ir::LibrarySymbolGraphic>,
    /// The library symbol's Value and Footprint field texts (`LIB_SYMBOL::GetValueField`, `GetFootprintField`), which a variant that draws this symbol in
    /// place of its own shows where it has no text of its own for them (KiCad 25ec2335 `SCH_SYMBOL::GetFieldText` with the variant:
    /// `altSymbol->GetField( .. )` when its text is not empty; [`crate::variant_symbol`]). Not connectivity; kept here as [`Self::user_fields`] is.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub value: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub footprint: String,
    /// What the symbol's shapes have beyond a [`SymbolGraphic`]'s members (UI-LEFT-2 YY2, PARITY-sch §87): the file's fill (`FILL_T`, which `filled` cannot
    /// tell apart), the line style, the border and fill colours, a hatch, a rectangle's corner radius; and the shapes private to the Symbol Editor, which
    /// [`LibSymbol::graphics`] leaves out (`SCH_PAINTER::draw( SCH_SHAPE )` draws none on a sheet) and the schematic's cache writes with `private`
    /// (`SCH_IO_KICAD_SEXPR_LIB_CACHE::SaveSymbol`, `formatRect` .. `formatPoly`'s `aIsPrivate`). Each by the graphic as the symbol holds it; kept here, as
    /// [`Self::text_boxes`] are, so `SymbolGraphic`'s literals stay as they are. Only a shape with a fill or a look of its own has an entry.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub shape_looks: Vec<ShapeLook>,
    /// What the symbol's texts have beyond a [`SymbolGraphic::Text`]'s members (UI-LEFT-3 S3-5): the justification, bold and italic, the face, the colour, the pen
    /// and the hidden flag (`EDA_TEXT::Format`'s `(effects (font (face ..) (size ..) (thickness ..) bold italic (color ..)) (justify ..) (hide yes))`), which a placed
    /// symbol draws its texts with (`SCH_PAINTER::draw( SCH_TEXT )` of the temp copy) and the schematic's cache writes. Each by the text as [`LibSymbol::graphics`] (or an
    /// alternate body) holds it; kept here, as [`Self::shape_looks`] are, so `SymbolGraphic`'s literals stay as they are. Only a text with a look of its own has an entry.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub text_looks: Vec<TextLook>,
    /// The library symbol's fields' face, colour and pen (`EDA_TEXT::Format`'s `(face ..)`, `(thickness ..)`, `(color ..)` in a field's `(effects (font ..))`),
    /// by the field's name: the schematic's cache writes them, and a placed symbol's field has them as placing it copied them (`SCH_SYMBOL::SCH_SYMBOL( const
    /// LIB_SYMBOL& .. )`: every field of the library symbol), unless Symbol Properties gave it its own (`SchExtras::field_looks`).
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub field_looks: std::collections::BTreeMap<String, crate::lib_keep::KeptFieldLooks>,
    /// The bodies of a symbol of more than two custom body styles beyond [`LibSymbol::alternate`] (KiCad 25ec2335's `LIB_SYMBOL::GetBodyStyleCount` is the number of
    /// `GetBodyStyleNames`, and a placed symbol draws any of them, `SCH_SYMBOL::SetBodyStyle`): style 3 and on, each tagged with its style. Every body but the one the
    /// symbol's own `graphics` and `pins` are is tagged with its style, here or in `alternate`, so the style a symbol is drawn in is the one no body is tagged with
    /// ([`LibSymbol::drawn_style`]): a symbol [`LibSymbol::in_style`] has put in style 3 keeps its normal body here, tagged 1. Kept here, as [`Self::pin_alternates`]
    /// is, so the symbol's literals stay as they are.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub extra_bodies: Vec<AlternateBody>,
    /// The names of a symbol's custom body styles (`(body_styles "Front" "Side" ..)`, `LIB_SYMBOL::GetBodyStyleNames`), empty for a single body style and for De Morgan
    /// ([`LibSymbol::body_style_description`]); the schematic's cache writes them back (`SaveSymbol`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub body_style_names: Vec<String>,
}

/// See [`LibConnectivity::shape_looks`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ShapeLook {
    /// The shape, as [`LibSymbol::graphics`] (or the alternate body's) holds it.
    pub graphic: SymbolGraphic,
    /// The body style of a private shape (0: every one); a shape that is drawn is found in the body it is in.
    #[serde(default, skip_serializing_if = "is_zero_style")]
    pub body_style: u32,
    /// The file's fill (`(fill (type ..))`).
    #[serde(default)]
    pub fill: crate::ir::LibraryFill,
    /// The line style (`(stroke (type ..))`) when it is not `default`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stroke_type: Option<String>,
    /// Private, the colours, the hatch, the corner radius.
    #[serde(default, skip_serializing_if = "crate::lib_keep::KeptGraphicLooks::is_default")]
    pub looks: crate::lib_keep::KeptGraphicLooks,
}

/// See [`LibConnectivity::text_looks`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TextLook {
    /// The text, as [`LibSymbol::graphics`] (or the alternate body's) holds it.
    pub graphic: SymbolGraphic,
    #[serde(default, skip_serializing_if = "is_center_h")]
    pub h_align: crate::sym_fields::FieldHAlign,
    #[serde(default, skip_serializing_if = "is_center_v")]
    pub v_align: crate::sym_fields::FieldVAlign,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub bold: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub italic: bool,
    /// The face (`(font (face ..))`); `None` is the default font.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub font: Option<String>,
    /// The colour (`EDA_TEXT::GetTextColor`); `None` is `COLOR4D::UNSPECIFIED`, `LAYER_DEVICE`'s.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text_color: Option<crate::sch_extras::SchColor>,
    /// The pen, mm (`(thickness ..)`); `None` is the automatic one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thickness_mm: Option<f64>,
    /// `(hide yes)`: drawn only with Show Hidden Fields, in the hidden colour (`SCH_PAINTER::draw( SCH_TEXT )`).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub hidden: bool,
}

fn is_center_h(a: &crate::sym_fields::FieldHAlign) -> bool {
    *a == crate::sym_fields::FieldHAlign::Center
}

fn is_center_v(a: &crate::sym_fields::FieldVAlign) -> bool {
    *a == crate::sym_fields::FieldVAlign::Center
}

impl TextLook {
    /// The look of a text: `None` for one of no look of its own (centred, plain, the default face, colour and pen, shown), which needs no entry.
    #[allow(clippy::too_many_arguments)]
    pub fn of(graphic: SymbolGraphic, h_align: crate::sym_fields::FieldHAlign, v_align: crate::sym_fields::FieldVAlign, bold: bool, italic: bool, font: Option<String>, text_color: Option<crate::sch_extras::SchColor>, thickness_mm: Option<f64>, hidden: bool) -> Option<TextLook> {
        let l = TextLook { graphic, h_align, v_align, bold, italic, font: font.filter(|f| !f.is_empty()), text_color, thickness_mm, hidden };
        (!is_center_h(&l.h_align) || !is_center_v(&l.v_align) || l.bold || l.italic || l.font.is_some() || l.text_color.is_some() || l.thickness_mm.is_some() || l.hidden).then_some(l)
    }
}

fn is_zero_style(s: &u32) -> bool {
    *s == 0
}

impl ShapeLook {
    /// The look of a shape: `None` for a shape of no fill and no look of its own, which needs no entry.
    pub fn of(graphic: SymbolGraphic, body_style: u32, fill: crate::ir::LibraryFill, stroke_type: Option<String>, looks: crate::lib_keep::KeptGraphicLooks) -> Option<ShapeLook> {
        let stroke_type = stroke_type.filter(|t| t != "default");
        (fill != crate::ir::LibraryFill::None || stroke_type.is_some() || !looks.is_default()).then_some(ShapeLook { graphic, body_style, fill, stroke_type, looks })
    }
}

impl LibConnectivity {
    pub fn is_default(&self) -> bool {
        *self == LibConnectivity::default()
    }

    /// The look of the drawn shape `g` (not a private one), if it has one.
    pub fn shape_look(&self, g: &SymbolGraphic) -> Option<&ShapeLook> {
        self.shape_looks.iter().find(|l| !l.looks.private && l.graphic == *g)
    }

    /// The look of the text `g`, if it has one.
    pub fn text_look(&self, g: &SymbolGraphic) -> Option<&TextLook> {
        self.text_looks.iter().find(|l| l.graphic == *g)
    }

    /// The shapes private to the Symbol Editor of unit `unit` that a symbol in body style `style` has.
    pub fn private_shapes(&self, unit: u32, style: u32) -> impl Iterator<Item = &ShapeLook> {
        self.shape_looks.iter().filter(move |l| l.looks.private && l.graphic.unit() == unit && (l.body_style == 0 || l.body_style == style))
    }

    /// The text boxes a symbol drawn in body style `style` has: those of that style and those of every style (body style 0, `SCH_PAINTER::draw( LIB_SYMBOL )`:
    /// `if( aBodyStyle && item.GetBodyStyle() && aBodyStyle != item.GetBodyStyle() ) continue`).
    pub fn text_boxes_of_style(&self, style: u32) -> impl Iterator<Item = &crate::ir::LibrarySymbolGraphic> {
        self.text_boxes.iter().filter(move |t| t.body_style() == 0 || t.body_style() == style)
    }
}

/// `SCH_DRAWING_TOOLS::PlaceSymbol`'s rewrite of a power symbol's keywords and description when it makes a global one local ([`LibSymbol::power_texts`]):
/// every "global power" becomes "local power", every "global label" "local label".
pub fn made_local_power_texts(keywords: &str, description: &str) -> (String, String) {
    (keywords.replace("global power", "local power"), description.replace("global label", "local label"))
}

impl LibSymbol {
    /// The keywords and the description of this power symbol's copy of the kind `local` (UI-LEFT-3 S3-11). KiCad 8303b2ad / 25ec2335
    /// `SCH_DRAWING_TOOLS::PlaceSymbol` (`eeschema/tools/sch_drawing_tools.cpp`): a power symbol that is not local, placed with the preference Local
    /// (`SetLocalPower()`), has "Adjust the KiCad library default fields to match the new power symbol type": every "global power" of its keywords becomes
    /// "local power", every "global label" of its description "local label" (`wxString::Replace`, all of them). One made global from a local one keeps
    /// both ("We do not currently have local power symbols in the KiCad library, so don't update any fields"), as does a copy of its own kind.
    pub fn power_texts(&self, local: bool) -> (String, String) {
        let (keywords, description) = (self.connectivity.keywords.clone(), self.description.clone());
        if !(self.power && local && !self.connectivity.power_local) {
            return (keywords, description);
        }
        made_local_power_texts(&keywords, &description)
    }

    /// `SCH_SYMBOL::SubReference( aUnit, false )` (KiCad 25ec2335, `sch_symbol.cpp`): "A custom unit display name overrides the default A/B/C (or numeric)
    /// suffix", else the unit's letters (`LIB_SYMBOL::LetterSubReference( aUnit, 'A' )`), with KiCad's default unit notation (no separator, letters from A); [`Self::sub_reference_in`] takes a project's.
    pub fn sub_reference(&self, unit: u32) -> String {
        self.sub_reference_in(unit, &crate::annotate::SubpartIds::default(), true)
    }

    /// `SCH_SYMBOL::SubReference( aUnit, aAddSeparator )` with the project's unit notation (`SCHEMATIC::Settings()`, [`crate::annotate::SubpartIds`]): a unit's
    /// display name after the separator, else `SCHEMATIC_SETTINGS::SubReference`. `GetRef( &aPath, true )` adds the separator (`aAddSeparator` defaults to true);
    /// a `${REFERENCE(pin)}`/`${UNIT(pin)}` does not (r7 H5).
    pub fn sub_reference_in(&self, unit: u32, ids: &crate::annotate::SubpartIds, add_separator: bool) -> String {
        match self.connectivity.unit_names.get(&unit) {
            Some(name) => format!("{}{name}", ids.separator_text(add_separator)),
            None => ids.sub_reference(unit, add_separator),
        }
    }

    /// The electrical type of the library pin `pin` on a placed symbol that picked the alternate function `alt` for it (`SCH_PIN::GetType`: an alternate's
    /// `m_Type` when the pin has one, `m_libPin->GetAlt( m_alt )`). `SCH_PIN::SetAlt` keeps no alternate that is the pin's own name or that the library
    /// pin does not have, so for those, and for no alternate, it is the library pin's type (r5 D5-H3).
    pub fn placed_pin_type<'a>(&'a self, pin: &'a LibPin, alt: Option<&str>) -> &'a str {
        self.kept_alternate(pin, alt).map_or(pin.electrical_type.as_str(), |x| x.electrical_type.as_str())
    }

    /// The alternate a placed pin keeps of the one its symbol picked (`SCH_PIN::SetAlt`: none that is the pin's own name or that the library pin does not
    /// have), which is then its shown name (`SCH_PIN::GetShownName`: `m_alt`) and its type (`GetType`).
    pub fn kept_alternate<'a>(&'a self, pin: &LibPin, alt: Option<&str>) -> Option<&'a crate::ir::PinAlternate> {
        alt.filter(|a| !a.is_empty() && *a != pin.name).and_then(|a| self.connectivity.pin_alternates.get(&pin.number).and_then(|alts| alts.iter().find(|x| x.name == a)))
    }

    pub fn pin_by_number(&self, number: &str) -> Option<&LibPin> {
        self.pins.iter().find(|p| p.number == number)
    }

    /// The `k`-th pin (from 0) of the number `number`: a symbol may have several pins of one number at several places (KiCad keeps every pin object,
    /// `SCH_SYMBOL::GetRawPins`), and a part lists them in the library's order, so its `k`-th pin of a number is the library's `k`-th. The first one when
    /// the library has fewer.
    pub fn nth_pin_by_number(&self, number: &str, k: usize) -> Option<&LibPin> {
        self.pins.iter().filter(|p| p.number == number).nth(k).or_else(|| self.pin_by_number(number))
    }

    /// `LIB_SYMBOL::HasAlternateBodyStyle`.
    pub fn has_alternate_body(&self) -> bool {
        self.alternate.is_some()
    }

    /// `LIB_SYMBOL::GetBodyStyleCount`: 1, 2 for a symbol with an alternate body style, and one more for each further custom body style (KiCad 25ec2335).
    pub fn body_style_count(&self) -> u32 {
        1 + self.alternate.is_some() as u32 + self.connectivity.extra_bodies.len() as u32
    }

    /// `LIB_SYMBOL::GetBodyStyleDescription( aBodyStyle, false )` (KiCad 25ec2335): a custom style's name, "Standard" / "Alternate" for De Morgan, else empty.
    pub fn body_style_description(&self, style: u32) -> String {
        if !self.connectivity.body_style_names.is_empty() {
            return self.connectivity.body_style_names.get(style.saturating_sub(1) as usize).cloned().unwrap_or_default();
        }
        match (self.alternate.is_some(), style) {
            (true, 1) => "Standard".into(),
            (true, 2) => "Alternate".into(),
            _ => String::new(),
        }
    }

    /// The bodies of other styles than the one this symbol's `graphics` and `pins` are of: [`Self::alternate`] and [`LibConnectivity::extra_bodies`].
    pub fn other_bodies(&self) -> impl Iterator<Item = &AlternateBody> {
        self.alternate.as_deref().into_iter().chain(self.connectivity.extra_bodies.iter())
    }

    /// The body style this symbol's own `graphics` and `pins` are of: the one no other body is tagged with (1, unless [`in_style`](Self::in_style) has put it in another).
    pub fn drawn_style(&self) -> u32 {
        let tagged: std::collections::BTreeSet<u32> = self.other_bodies().map(|b| b.style).collect();
        (1..=self.body_style_count()).find(|s| !tagged.contains(s)).unwrap_or(1)
    }

    /// The `(graphics, pins)` of body style `style`, `None` for a style the symbol does not have.
    #[allow(clippy::type_complexity)]
    pub fn body_of(&self, style: u32) -> Option<(&[SymbolGraphic], &[LibPin])> {
        if style == self.drawn_style() {
            return Some((&self.graphics, &self.pins));
        }
        self.other_bodies().find(|b| b.style == style).map(|b| (b.graphics.as_slice(), b.pins.as_slice()))
    }

    /// Both bodies of a De Morgan symbol, by style: the `(graphics, pins)` of the normal one (style 1) and, for a symbol that has two or more, of the second one.
    #[allow(clippy::type_complexity)]
    pub fn bodies(&self) -> ((&[SymbolGraphic], &[LibPin]), Option<(&[SymbolGraphic], &[LibPin])>) {
        let normal = self.body_of(1).unwrap_or((&self.graphics, &self.pins));
        (normal, if self.body_style_count() >= 2 { self.body_of(2) } else { None })
    }

    /// Every body, by style, from 1.
    #[allow(clippy::type_complexity)]
    pub fn all_bodies(&self) -> Vec<(u32, &[SymbolGraphic], &[LibPin])> {
        (1..=self.body_style_count()).filter_map(|s| self.body_of(s).map(|(g, p)| (s, g, p))).collect()
    }

    /// The symbol as a placed one in body style `style` draws it (`SCH_SYMBOL::SetBodyStyle`, KiCad 25ec2335: any of `GetBodyStyleCount()` styles): that body's `graphics` and
    /// `pins` in place of the normal one's, every other body kept, tagged with its style ([`Self::alternate`], [`LibConnectivity::extra_bodies`]), so what depends on the box
    /// of the symbol as a whole (`real_symbol_bbox`: the corner a derived sheet places it by) is the same in every style. A style the symbol does not have (a symbol with one
    /// body style draws it whatever style it is placed in) is style 1. Putting a symbol in the style it is in already changes nothing.
    pub fn in_style(&self, style: u32) -> std::borrow::Cow<'_, LibSymbol> {
        let count = self.body_style_count();
        let want = if (1..=count).contains(&style) { style } else { 1 };
        if count < 2 || self.drawn_style() == want {
            return std::borrow::Cow::Borrowed(self);
        }
        let body = |s: u32| self.body_of(s).map(|(g, p)| (g.to_vec(), p.to_vec())).unwrap_or_default();
        let (graphics, pins) = body(want);
        // style 2's body is the alternate, unless the symbol is drawn in style 2: then the alternate is the normal body; the same for each further style
        let tagged = |s: u32| {
            let (g, p) = if s == want { body(1) } else { body(s) };
            AlternateBody { style: if s == want { 1 } else { s }, graphics: g, pins: p }
        };
        let alternate = Some(Box::new(tagged(2)));
        let mut connectivity = self.connectivity.clone();
        connectivity.extra_bodies = (3..=count).map(tagged).collect();
        // one `LIB_SYMBOL` for every body style: its jumper groups, power kind and pin alternates hold in either (`conn_facts.cpp` reads
        // `JumperPinGroups()` whatever the style, r5 A5-M2)
        std::borrow::Cow::Owned(LibSymbol { lib_id: self.lib_id.clone(), graphics, pins, alternate, connectivity, ..self.clone() })
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
    // no library symbol: a generated one (`crate::gensym`). The id says which kind of drawing the schematic holds: a design written before
    // generated symbols has `eda:<ref>`, the box the layout engine sized, and keeps drawing that.
    let synthetic = || format!("{}{}", crate::gensym::GENERATED_PREFIX, part.reference);

    // the generic symbols of the library number their pins 1, 2, 3 ...: a part whose pins have other numbers (a USB-C receptacle's A1 ..
    // B12 and SH) has no library symbol to take them, and gets a generated one
    let numbered_from_one = (1..=part.pins.len()).all(|n| part.pins.iter().any(|p| p.number == n.to_string()));

    let first_letter = |s: &str| s.trim_start_matches(['+', '-']).chars().next().map(|c| c.to_ascii_uppercase());
    let kind_letter = [Some(part.reference.as_str()), part.package.as_deref(), part.value.as_deref()]
        .into_iter()
        .flatten()
        .find_map(first_letter);

    if part.pins.len() == 2 && numbered_from_one {
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
    if ref_upper.starts_with('J') && (1..=40).contains(&part.pins.len()) && numbered_from_one {
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
    lib_id.starts_with("eda:") || crate::gensym::is_generated_lib_id(lib_id)
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
    LibPin { number: number.into(), name: String::new(), electrical_type: etype.into(), shape: "line".into(), at: SPoint::new(x, y), angle_deg, length_mm, unit: 1, hidden: false }
}

/// `Device:R` -- transcribed from the installed `Device.kicad_sym`.
fn device_r() -> LibSymbol {
    LibSymbol { connectivity: Default::default(),
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
        pin_names_hidden: false,
        pin_numbers_hidden: true,
        pin_name_offset_mm: 0.0,
        alternate: None,
    }
}

/// `Device:C` -- two plates, transcribed proportionally from the real symbol.
fn device_c() -> LibSymbol {
    LibSymbol { connectivity: Default::default(),
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
        pin_names_hidden: false,
        pin_numbers_hidden: true,
        pin_name_offset_mm: 0.254,
        alternate: None,
    }
}

/// `Device:L` -- kept simple (a rectangle, like `Device:R`) rather than
/// the real hump-arc artwork: readable and electrically identical, and
/// avoids needing this port's arc writer to reproduce KiCad's exact bulge
/// geometry for a part none of this task's examples actually use.
fn device_l() -> LibSymbol {
    LibSymbol { connectivity: Default::default(),
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
        pin_names_hidden: true,
        pin_numbers_hidden: true,
        pin_name_offset_mm: 1.016,
        alternate: None,
    }
}

/// `Device:D` -- a diode: triangle + bar.
fn device_d() -> LibSymbol {
    LibSymbol { connectivity: Default::default(),
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
        pin_names_hidden: true,
        pin_numbers_hidden: true,
        pin_name_offset_mm: 1.016,
        alternate: None,
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
    LibSymbol { connectivity: Default::default(),
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
        pin_names_hidden: true,
        pin_numbers_hidden: false,
        pin_name_offset_mm: 1.016,
        alternate: None,
    }
}

/// `power:GND` -- the ground tines glyph, transcribed from `power.kicad_sym`.
fn power_gnd() -> LibSymbol {
    LibSymbol { connectivity: Default::default(),
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
        pin_names_hidden: true,
        pin_numbers_hidden: true,
        pin_name_offset_mm: 0.0,
        alternate: None,
    }
}

/// `power:PWR_FLAG` -- the standard "this net has a source" marker: a
/// `power_out` pin, so it satisfies ERC's power-input-driven rule for any
/// net it is dropped on.
fn power_flag() -> LibSymbol {
    LibSymbol { connectivity: Default::default(),
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
        reference_prefix: "#FLG".into(),
        unit_count: 1,
        pin_names_hidden: true,
        pin_numbers_hidden: true,
        pin_name_offset_mm: 0.0,
        alternate: None,
    }
}

/// A generic power rail symbol (`power:VCC`-style upward arrow) labeled
/// with any net name -- covers every rail that is not `GND` without
/// needing one hand-transcribed glyph per rail name, exactly the way a
/// KiCad user makes a custom rail symbol in practice (copy `VCC`, rename
/// its Value).
fn power_rail(net: &str) -> LibSymbol {
    LibSymbol { connectivity: Default::default(),
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
        pin_names_hidden: true,
        pin_numbers_hidden: true,
        pin_name_offset_mm: 0.0,
        alternate: None,
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
            body_nm: None,
            edge: None,
        }
    }

    /// r5 A5-M2: the symbol in its alternate body style keeps what the connectivity engine reads of it (one `LIB_SYMBOL` for every body style).
    #[test]
    fn in_style_keeps_the_connectivity() {
        let mut sym = device_r();
        sym.alternate = Some(Box::new(AlternateBody { style: 2, graphics: sym.graphics.clone(), pins: sym.pins.clone() }));
        sym.connectivity.jumper_pin_groups = vec![vec!["1".into(), "2".into()]];
        sym.connectivity.duplicate_pin_numbers_are_jumpers = true;
        let styled = sym.in_style(2);
        assert!(matches!(styled, std::borrow::Cow::Owned(_)));
        assert_eq!(styled.connectivity, sym.connectivity);
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
    fn pins_the_generic_symbols_do_not_number_get_a_generated_symbol() {
        // the generic connector and the Device symbols number their pins 1, 2, 3 ...
        let mut j = part("J1", None, None, 3);
        for (p, n) in j.pins.iter_mut().zip(["A1", "A4", "SH"]) {
            p.number = n.to_string();
        }
        assert_eq!(resolve_lib_id(&j), "gen:J1");
        let mut d = part("D1", None, None, 2);
        d.pins[0].number = "A".into();
        d.pins[1].number = "K".into();
        assert_eq!(resolve_lib_id(&d), "gen:D1");
        let mut r = part("R1", None, None, 2);
        r.pins.swap(0, 1);
        assert_eq!(resolve_lib_id(&r), "Device:R", "the order of the pins does not matter");
    }

    #[test]
    fn multi_pin_ic_falls_back_to_synthetic() {
        let id = resolve_lib_id(&part("U1", None, None, 20));
        assert!(is_synthetic_lib_id(&id));
        assert_eq!(id, "gen:U1");
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
