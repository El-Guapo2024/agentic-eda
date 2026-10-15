//! Additive schematic items and per-item annotations: the data behind the schematic editor's drawing
//! and edit tools that the core schematic items (`SymbolInstance`, `Wire`, `NetLabel`, ...) do not
//! carry. Everything here is optional (`SchematicSection::extras`, default empty and never written
//! when empty), so a design that never used one of these tools serializes exactly as before.
//!
//! Why one `extras` struct instead of a new field per item kind: `SchematicSection` and its items are
//! built with struct literals in many places (tests, importers, the derived-schematic generator), so
//! every new field there means touching every literal. Growing this struct touches none of them.
//!
//! What lives here, each with the KiCad class it mirrors:
//! - [`SchGraphic`]: rectangles, circles, arcs, beziers and polygons (`SCH_SHAPE`), text boxes
//!   (`SCH_TEXTBOX`), rule areas (`SCH_RULE_AREA`) and directive labels (`SCH_DIRECTIVE_LABEL`, the
//!   "netclass flag"). Decoration or annotation: none of them is part of any net.
//! - `locked`: the ids of locked items (`SCH_ITEM::IsLocked`, set by Lock / Unlock / Toggle Lock).
//! - `label_spins`: which way a label's text runs from its anchor (`SCH_LABEL_BASE::GetSpinStyle`), once Rotate or Mirror has set it.
//! - `strokes`, `junction_looks`: the width, line style and colour of a wire, bus, bus entry or graphic line and the diameter and colour
//!   of an explicit junction (`STROKE_PARAMS`, `SCH_JUNCTION::m_diameter`), once Wire/Bus Properties has set them.
//! - `sheet_pin_looks`: the size, bold and italic of a hierarchical sheet pin's text (`SCH_SHEET_PIN` is an `EDA_TEXT`), once Sheet Pin Properties has set them.
//! - `label_looks`: the size, bold and italic of a label's text (`EDA_TEXT`) and whether it rotates itself when placed (`AutoRotateOnPlacement`), once Label Properties has set them.
//! - `fields_autoplaced`: which symbols, power symbols and sheets have their fields where Autoplace Fields put them
//!   (`SCH_ITEM::GetFieldsAutoplaced`), so a turn, a text edit or a Properties OK places them again.
//! - `body_styles`: the body style a placed symbol is drawn in (`SCH_SYMBOL::GetBodyStyle`; 1 is the normal one, 2 KiCad's alternate "De Morgan" one).
//! - `pin_swaps`: which pin of a placed symbol sits where another one of its pins is in the library symbol (`SwapPinGeometry`, Swap Pins).
//! - `text_justify`: how a free-standing text is justified about its anchor (`EDA_TEXT::GetHorizJustify` / `GetVertJustify`), once Text Properties, a turn or a mirror set it.
//! - `groups`: the sheet's groups (`SCH_GROUP`, `(group ...)` in the file), the same [`crate::ir::Group`] the board's drawings keep; see `crate::groups`.

use crate::ir::{Millideg, Point, TextJustify, TextVAlign, Um};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// Items and annotations beyond the core schematic content -- see the module doc.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SchExtras {
    /// Schematic Setup > Violation Severity (`ERC_SETTINGS::m_ERCSeverities`, `panel_setup_severities.cpp`): the ERC checks reported at a
    /// severity other than KiCad's default for them, by settings key (`pin_not_connected`, ...) -> `error` | `warning` | `ignore`.
    /// Written to the derived project as `erc.rule_severities`, so kicad-cli's ERC reports each check at the severity chosen. Absent:
    /// KiCad's defaults. `crate::erc_checks` lists the keys. Additive. (Counts in `is_empty`: a table is a reason to write `extras`.)
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub erc_severities: BTreeMap<String, String>,
    /// Drawn graphics and annotations, in drawing order (later ones paint over earlier ones).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub graphics: Vec<SchGraphic>,
    /// Ids of locked items, sorted and unique. A symbol is keyed by its reference (every unit of a
    /// multi-unit part locks together, the way the studio selects them together).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub locked: Vec<String>,
    /// `SCH_SCREEN::GetPageSettings()`: the sheet's paper (Page Settings, `common.Control.pageSettings`), written as `(paper ...)`.
    /// `None` is KiCad's default, A4 landscape.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub page: Option<crate::page::PageSettings>,
    /// A label's spin (`SCH_LABEL_BASE::GetSpinStyle`: which side of its anchor the text runs to), by label id. A label with no entry
    /// has its spin read off the wire that ends at it (`labelShape.ts::inferSpin`), as every label did before Rotate and Mirror could
    /// set one; an entry is written only when a turn or a mirror set it, so a design that never turned a label serializes as before.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub label_spins: BTreeMap<String, LabelSpin>,
    /// A wire's, bus's, bus entry's or graphic line's stroke (`SCH_LINE::GetStroke`, `SCH_BUS_ENTRY_BASE::GetStroke`), by item id, once
    /// Wire/Bus Properties (`DIALOG_WIRE_BUS_PROPERTIES`) or Line Properties set one. An item with no entry has the default stroke (the
    /// sheet's default width, a solid line, the layer's colour); an entry equal to the default is never stored. A graphic line's width
    /// lives in `SchLine::width_um`, as it always has: its entry here carries only the style and the colour.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub strokes: BTreeMap<String, SchStroke>,
    /// An explicit junction's dot (`SCH_JUNCTION::GetDiameter` / `GetColor`), by junction id, once Junction Properties set one.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub junction_looks: BTreeMap<String, JunctionLook>,
    /// How a label's text is set (`EDA_TEXT::GetTextWidth`, `IsBold`, `IsItalic`), by label id, once Label Properties set it. A label with no entry has the
    /// default text (50 mil, regular, upright), which is what the writer and the painter use for every label that was never edited.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub label_looks: BTreeMap<String, LabelLook>,
    /// How a hierarchical sheet pin's text is set (`SCH_SHEET_PIN` is an `EDA_TEXT`: `DIALOG_SHEET_PIN_PROPERTIES`' size, bold and italic), by sheet pin id, once Sheet Pin
    /// Properties set it, as `label_looks` does for labels (`auto_rotate` is for labels and stays off). A pin with no entry has the default text, 50 mil, regular, upright.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub sheet_pin_looks: BTreeMap<String, LabelLook>,
    /// The items whose fields are where Autoplace Fields put them (`SCH_ITEM::m_fieldsAutoplaced`, `(fields_autoplaced yes)` in the file), by
    /// `field_key` (a symbol's reference, `#<unit>` after it for a unit but the first), a power symbol's or a sheet's id. Moving, turning, re-justifying or
    /// editing the place of one of the fields takes the item out of it (`SetFieldsAutoplaced( AUTOPLACE_NONE )`); turning the symbol, editing a field's
    /// text or showing a hidden field places them again while the item is in it. An item with no entry in `SchematicSection::field_layout` has its
    /// fields placed live, as if it were in it.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub fields_autoplaced: BTreeMap<String, AutoplaceAlgo>,
    /// The body style a placed symbol is drawn in, by `field_key`: 1 is the normal one, 2 the alternate ("De Morgan") one of a library symbol that has
    /// both (`SCH_SYMBOL::SetBodyStyle`, `(body_style 2)` in the file). A symbol with no entry is drawn in style 1.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub body_styles: BTreeMap<String, u32>,
    /// Pins of a placed symbol that Swap Pins (`SCH_EDIT_TOOL::SwapPins`, `SwapPinGeometry` of `sch_tool_utils.cpp`) moved, by `field_key`: pin number ->
    /// the number of the pin whose place (position, orientation and length in the library symbol) it has now. KiCad swaps those in the symbol's own
    /// copy of its library pins and writes that copy to the file; a pin keeps its number, name and type and takes the other's place. A pin with no
    /// entry, or an entry naming itself, is where the library puts it. The entries always form a permutation of the symbol's pin numbers.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub pin_swaps: BTreeMap<String, BTreeMap<String, String>>,
    /// How a free-standing text (`SchematicText`) is justified about its anchor (`EDA_TEXT::GetHorizJustify`, `GetVertJustify`), by text id, once
    /// Text Properties, a turn (`SCH_TEXT::Rotate90`) or a mirror (`MirrorHorizontally`, `MirrorVertically`, `MirrorSpinStyle`) set it. A text with no
    /// entry is [`SchTextJustify::default`], left and bottom -- the way the studio has always set a text on its anchor; an entry equal to that is never
    /// stored. The writer says it in `(effects ... (justify ...))` and the importer reads it back, so KiCad draws the text where the studio does.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub text_justify: BTreeMap<String, SchTextJustify>,
    /// The groups of this sheet (`SCH_GROUP`, written as `(group "name" (uuid ..) (members ..))`): a named set of the ids of items of the sheet -- a symbol's reference (every
    /// placed unit), the id of a wire, label, text, power symbol, no-connect, bus entry, junction, graphic line, drawn shape or sheet -- and of other groups, so groups nest.
    /// A group has no geometry of its own: its box is its members'. A pin, a field and a sheet pin go along with the item that has them and are never members. The tree is
    /// read and edited by `crate::groups` (`GroupTree`, `GroupHost`), the same code as the board's groups.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub groups: Vec<crate::ir::Group>,
}

/// How a free-standing text sits on its anchor: the horizontal and the vertical justification of its `EDA_TEXT`, in the text's own axes (a text turned
/// to run up the sheet keeps them, the way KiCad's file does). The default is left and bottom, what the studio drew before a text could be justified;
/// KiCad's own file default is centre and centre, which a text read from a file without `(justify ...)` is given explicitly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SchTextJustify {
    #[serde(default = "legacy_h")]
    pub h: TextJustify,
    #[serde(default = "legacy_v")]
    pub v: TextVAlign,
}

fn legacy_h() -> TextJustify {
    TextJustify::Left
}

fn legacy_v() -> TextVAlign {
    TextVAlign::Bottom
}

impl Default for SchTextJustify {
    fn default() -> Self {
        SchTextJustify { h: TextJustify::Left, v: TextVAlign::Bottom }
    }
}

impl SchTextJustify {
    /// KiCad's file default, `{center,center}` (`parseEDA_TEXT`: "the file format default is {center,center}").
    pub const FILE_DEFAULT: SchTextJustify = SchTextJustify { h: TextJustify::Center, v: TextVAlign::Center };

    pub fn is_default(&self) -> bool {
        *self == SchTextJustify::default()
    }

    /// `EDA_TEXT::FlipHJustify`: left and right trade places, centre stays.
    pub fn flip_h(self) -> SchTextJustify {
        let h = match self.h {
            TextJustify::Left => TextJustify::Right,
            TextJustify::Right => TextJustify::Left,
            TextJustify::Center => TextJustify::Center,
        };
        SchTextJustify { h, v: self.v }
    }

    /// `SCH_TEXT::Rotate90( aClockwise )` for a text that runs up the sheet (`vertical`, `ANGLE_VERTICAL`) or along it: the angle toggles between the two,
    /// and a horizontal text turned clockwise or a vertical one turned counter-clockwise has its horizontal justification flipped (its anchor stays on
    /// the same end of the text as the text lies the other way).
    pub fn rotated90(self, vertical: bool, clockwise: bool) -> (bool, SchTextJustify) {
        let flip = (!vertical && clockwise) || (vertical && !clockwise);
        (!vertical, if flip { self.flip_h() } else { self })
    }

    /// `SCH_TEXT::MirrorSpinStyle( aLeftRight )`, what `SCH_EDIT_TOOL::Mirror` runs on a lone text (`MirrorSpinStyle( !vertical )`): the angle stays,
    /// the horizontal justification flips for a left-right mirror of a horizontal text and a top-bottom mirror of a vertical one.
    pub fn mirrored_spin(self, vertical: bool, left_right: bool) -> SchTextJustify {
        if (!vertical && left_right) || (vertical && !left_right) {
            self.flip_h()
        } else {
            self
        }
    }

    /// The justification part of `SCH_TEXT::MirrorHorizontally( aCenter )`, what a mirror of several items runs on a text: only a horizontal text flips.
    pub fn mirrored_horizontally(self, vertical: bool) -> SchTextJustify {
        if vertical {
            self
        } else {
            self.flip_h()
        }
    }

    /// The justification part of `SCH_TEXT::MirrorVertically( aCenter )`: only a vertical text flips.
    pub fn mirrored_vertically(self, vertical: bool) -> SchTextJustify {
        if vertical {
            self.flip_h()
        } else {
            self
        }
    }
}

/// `AUTOPLACE_ALGO`: which Autoplace Fields routine put an item's fields where they are. "Auto" is the minimalist one that runs when a symbol is
/// placed or turned, "manual" the one the Autoplace Fields command runs, which also keeps the fields off whatever is drawn around the symbol.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AutoplaceAlgo {
    #[default]
    Auto,
    Manual,
}

/// The part of a label's `EDA_TEXT` that is not its spin or its text: size, bold, italic.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LabelLook {
    /// The text's size (height and width), micrometres; 0 is the default, 50 mil.
    #[serde(default, skip_serializing_if = "is_zero_um")]
    pub size_um: Um,
    #[serde(default, skip_serializing_if = "is_false")]
    pub bold: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub italic: bool,
    /// `SCH_LABEL_BASE::AutoRotateOnPlacement` (the "Auto" check of Label Properties; only a global or a hierarchical label has it): the label takes its spin from what it
    /// is connected to when it is placed, moved or edited (`SCH_EDIT_FRAME::AutoRotateItem`). KiCad does not write it to the file.
    #[serde(default, skip_serializing_if = "is_false")]
    pub auto_rotate: bool,
}

impl LabelLook {
    pub fn is_default(&self) -> bool {
        *self == LabelLook::default()
    }

    /// The size the text is set in, micrometres.
    pub fn text_size_um(&self) -> Um {
        if self.size_um > 0 {
            self.size_um
        } else {
            crate::ir::DEFAULT_FIELD_SIZE_UM
        }
    }
}

/// How a wire, bus, bus entry or graphic line is stroked (`STROKE_PARAMS`): the part of `DIALOG_WIRE_BUS_PROPERTIES` and
/// `DIALOG_LINE_PROPERTIES` that is not the item's place.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SchStroke {
    /// The line width; 0 is the default (`default_line_thickness`, 6 mil for a wire).
    #[serde(default, skip_serializing_if = "is_zero_um")]
    pub width_um: Um,
    #[serde(default, skip_serializing_if = "SchLineStyle::is_default")]
    pub style: SchLineStyle,
    /// `None` is `COLOR4D::UNSPECIFIED`: the colour of the item's layer.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<SchColor>,
}

impl SchStroke {
    /// The stroke every item has until a dialog sets one.
    pub fn is_default(&self) -> bool {
        *self == SchStroke::default()
    }
}

/// How an explicit junction is drawn (`SCH_JUNCTION::m_diameter` and `m_color`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JunctionLook {
    /// The dot's diameter; 0 is the default (`DEFAULT_JUNCTION_DIAM`, 36 mil).
    #[serde(default, skip_serializing_if = "is_zero_um")]
    pub diameter_um: Um,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<SchColor>,
}

impl JunctionLook {
    pub fn is_default(&self) -> bool {
        *self == JunctionLook::default()
    }
}

/// `SPIN_STYLE`: which side of its anchor a label's text sits on and runs to. `Right` is the usual label (text to the right of a wire that
/// comes from the left); `Up` reads upward from the anchor, `Left` ends at it, `Bottom` hangs below it reading upward.
/// Turning a label counter-clockwise goes `Right`, `Up`, `Left`, `Bottom` (`SCH_TEXT::Rotate90`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LabelSpin {
    Right,
    Up,
    Left,
    Bottom,
}

impl LabelSpin {
    /// One quarter turn: counter-clockwise when `ccw` (`SCH_TEXT::Rotate90( !ccw )`: the horizontal/vertical angle toggles and the
    /// justification flips in two of the four cases, which is this cycle).
    pub fn rotated(self, ccw: bool) -> LabelSpin {
        use LabelSpin::*;
        match (self, ccw) {
            (Right, true) | (Left, false) => Up,
            (Up, true) | (Bottom, false) => Left,
            (Left, true) | (Right, false) => Bottom,
            (Bottom, true) | (Up, false) => Right,
        }
    }

    /// `SCH_LABEL_BASE::MirrorSpinStyle( aLeftRight )`: a left-right mirror swaps `Right` and `Left`, a top-bottom one `Up` and `Bottom`;
    /// the other axis leaves the spin as it is.
    pub fn mirrored(self, left_right: bool) -> LabelSpin {
        use LabelSpin::*;
        match (self, left_right) {
            (Right, true) => Left,
            (Left, true) => Right,
            (Up, false) => Bottom,
            (Bottom, false) => Up,
            (s, _) => s,
        }
    }
}

impl SchExtras {
    /// Whether design.json has nothing to say for `extras` (it is then not written): [`is_empty`](Self::is_empty) and no table of ERC severities. The
    /// severities are no part of the schematic KiCad's files draw -- they go to the derived project -- so they are not in `is_empty`.
    pub fn writes_nothing(&self) -> bool {
        self.is_empty() && self.erc_severities.is_empty()
    }
}

impl SchExtras {
    /// The number of the pin whose place pin `number` of the symbol keyed `key` has (itself when nothing moved it).
    pub fn pin_source<'a>(&'a self, key: &str, number: &'a str) -> &'a str {
        self.pin_swaps.get(key).and_then(|m| m.get(number)).map_or(number, String::as_str)
    }

    /// Swap the places of pins `a` and `b` of the symbol keyed `key` (`SwapPinGeometry`): each takes the place the other has now. An entry
    /// that ends up naming its own pin is dropped, and a symbol with no entry left keeps none.
    pub fn swap_pin_places(&mut self, key: &str, a: &str, b: &str) {
        if a == b {
            return;
        }
        let (sa, sb) = (self.pin_source(key, a).to_string(), self.pin_source(key, b).to_string());
        let map = self.pin_swaps.entry(key.to_string()).or_default();
        for (pin, src) in [(a, sb), (b, sa)] {
            if src == pin {
                map.remove(pin);
            } else {
                map.insert(pin.to_string(), src);
            }
        }
        if map.is_empty() {
            self.pin_swaps.remove(key);
        }
    }

    /// The places of the pins of the symbol keyed `key`, as [`permute_pins`] takes them; `None` for a symbol whose pins are all where the library has them.
    pub fn pin_swaps_of(&self, key: &str) -> Option<&BTreeMap<String, String>> {
        self.pin_swaps.get(key)
    }

    /// How the text `id` is justified about its anchor (the default when nothing set it).
    pub fn text_justify_of(&self, id: &str) -> SchTextJustify {
        self.text_justify.get(id).copied().unwrap_or_default()
    }

    /// Set how the text `id` is justified; the default is kept as no entry. Returns whether anything changed.
    pub fn set_text_justify(&mut self, id: &str, j: SchTextJustify) -> bool {
        if self.text_justify_of(id) == j {
            return false;
        }
        if j.is_default() {
            self.text_justify.remove(id);
        } else {
            self.text_justify.insert(id.to_string(), j);
        }
        true
    }
}

/// `items` (a pin number and what sits at that pin's place: a point, a position-and-angle, ...) with the places traded as `swaps` says: pin `n` gets
/// what pin `swaps[n]` had. A number the list does not have is left out of the exchange, so a stale entry (the symbol changed) does nothing.
pub fn permute_pins<T: Clone>(items: Vec<(String, T)>, swaps: &BTreeMap<String, String>) -> Vec<(String, T)> {
    if swaps.is_empty() {
        return items;
    }
    let by_number: BTreeMap<&str, &T> = items.iter().map(|(n, t)| (n.as_str(), t)).collect();
    let moved: Vec<(String, T)> = items
        .iter()
        .map(|(n, t)| match swaps.get(n).and_then(|src| by_number.get(src.as_str())) {
            Some(other) => (n.clone(), (*other).clone()),
            None => (n.clone(), t.clone()),
        })
        .collect();
    moved
}

impl SchExtras {
    pub fn is_empty(&self) -> bool {
        self.graphics.is_empty()
            && self.locked.is_empty()
            && self.page.is_none()
            && self.label_spins.is_empty()
            && self.strokes.is_empty()
            && self.junction_looks.is_empty()
            && self.label_looks.is_empty()
            && self.sheet_pin_looks.is_empty()
            && self.fields_autoplaced.is_empty()
            && self.body_styles.is_empty()
            && self.pin_swaps.is_empty()
            && self.text_justify.is_empty()
            && self.groups.is_empty()
    }

    /// True when `id` is locked.
    pub fn is_locked(&self, id: &str) -> bool {
        self.locked.binary_search_by(|l| l.as_str().cmp(id)).is_ok()
    }

    /// Lock or unlock `id` (keeping `locked` sorted and unique). Returns whether anything changed.
    pub fn set_locked(&mut self, id: &str, locked: bool) -> bool {
        match self.locked.binary_search_by(|l| l.as_str().cmp(id)) {
            Ok(i) if !locked => {
                self.locked.remove(i);
                true
            }
            Err(i) if locked => {
                self.locked.insert(i, id.to_string());
                true
            }
            _ => false,
        }
    }

    /// Backfill `id` on every graphic that has none, deterministically from its content (same
    /// contract as `SchematicSection::assign_missing_ids`). `existing` is the set of ids already
    /// taken in the section; it grows as ids are handed out.
    pub fn assign_missing_ids(&mut self, existing: &mut BTreeSet<String>) {
        existing.extend(self.graphics.iter().map(|g| g.id.clone()).filter(|s| !s.is_empty()));
        for i in 0..self.graphics.len() {
            if self.graphics[i].id.is_empty() {
                let id = crate::ir::next_item_id(self.graphics[i].shape.id_prefix(), &self.graphics[i].id_seed(), existing);
                existing.insert(id.clone());
                self.graphics[i].id = id;
            }
        }
    }

    /// Canonical order for hashing: `locked` is kept sorted by its setter;
    /// `graphics` keeps its drawing order (it is meaningful), so there is nothing to sort.
    pub fn canonicalize(&mut self) {
        self.locked.sort();
        self.locked.dedup();
    }
}

/// Stroke style of a graphic (`LINE_STYLE`): `Default` follows the sheet's own default line style.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SchLineStyle {
    #[default]
    Default,
    Solid,
    Dash,
    Dot,
    DashDot,
    DashDotDot,
}

impl SchLineStyle {
    pub fn is_default(&self) -> bool {
        *self == SchLineStyle::Default
    }

    /// The `(stroke (type ...))` token KiCad writes.
    pub fn token(self) -> &'static str {
        match self {
            SchLineStyle::Default => "default",
            SchLineStyle::Solid => "solid",
            SchLineStyle::Dash => "dash",
            SchLineStyle::Dot => "dot",
            SchLineStyle::DashDot => "dash_dot",
            SchLineStyle::DashDotDot => "dash_dot_dot",
        }
    }

    pub fn from_token(s: &str) -> SchLineStyle {
        match s {
            "solid" => SchLineStyle::Solid,
            "dash" => SchLineStyle::Dash,
            "dot" => SchLineStyle::Dot,
            "dash_dot" => SchLineStyle::DashDot,
            "dash_dot_dot" => SchLineStyle::DashDotDot,
            _ => SchLineStyle::Default,
        }
    }
}

/// How a closed graphic is filled (`FILL_T`): `Outline` fills with the stroke colour, `Background`
/// with the symbol-body background colour, `Color` with [`SchGraphic::fill_color`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SchFill {
    #[default]
    None,
    Outline,
    Background,
    Color,
}

impl SchFill {
    pub fn is_none(&self) -> bool {
        *self == SchFill::None
    }

    /// The `(fill (type ...))` token KiCad writes.
    pub fn token(self) -> &'static str {
        match self {
            SchFill::None => "none",
            SchFill::Outline => "outline",
            SchFill::Background => "background",
            SchFill::Color => "color",
        }
    }

    pub fn from_token(s: &str) -> SchFill {
        match s {
            "outline" => SchFill::Outline,
            "background" => SchFill::Background,
            "color" => SchFill::Color,
            _ => SchFill::None,
        }
    }
}

/// An RGBA colour, each channel 0..=255.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SchColor {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

/// Horizontal text alignment (`GR_TEXT_H_ALIGN_T`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SchHAlign {
    #[default]
    Left,
    Center,
    Right,
}

/// Vertical text alignment (`GR_TEXT_V_ALIGN_T`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SchVAlign {
    #[default]
    Top,
    Center,
    Bottom,
}

/// The outline of a directive label's flag (`LABEL_FLAG_SHAPE`'s `F_*` values).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DirectiveShape {
    Dot,
    #[default]
    Round,
    Diamond,
    Rectangle,
}

impl DirectiveShape {
    pub fn token(self) -> &'static str {
        match self {
            DirectiveShape::Dot => "dot",
            DirectiveShape::Round => "round",
            DirectiveShape::Diamond => "diamond",
            DirectiveShape::Rectangle => "rectangle",
        }
    }

    pub fn from_token(s: &str) -> DirectiveShape {
        match s {
            "dot" => DirectiveShape::Dot,
            "diamond" => DirectiveShape::Diamond,
            "rectangle" => DirectiveShape::Rectangle,
            _ => DirectiveShape::Round,
        }
    }
}

/// What kind of graphic an [`SchGraphic`] is, with its own geometry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum SchGraphicKind {
    /// `SCH_SHAPE` rectangle: two opposite corners (not normalized -- the order they were drawn in).
    Rectangle {
        start: Point,
        end: Point,
        #[serde(default, skip_serializing_if = "is_zero_um")]
        corner_radius_um: Um,
    },
    /// `SCH_SHAPE` circle: centre and radius.
    Circle { center: Point, radius_um: Um },
    /// `SCH_SHAPE` arc: start, a point on the arc between them, and end (KiCad's own three-point form).
    Arc { start: Point, mid: Point, end: Point },
    /// `SCH_SHAPE` bezier: start, control 1, control 2, end.
    Bezier { start: Point, c1: Point, c2: Point, end: Point },
    /// `SCH_SHAPE` polygon: a closed outline of three or more points.
    Polygon { pts: Vec<Point> },
    /// `SCH_TEXTBOX`: a rectangle with text inside. `angle` is 0 or 90000 (KiCad keeps a text box's
    /// text horizontal or vertical only); `margin_um` is the gap between the border and the text.
    TextBox {
        start: Point,
        end: Point,
        text: String,
        #[serde(default, skip_serializing_if = "is_zero_angle")]
        angle: Millideg,
        size_um: Um,
        #[serde(default, skip_serializing_if = "is_false")]
        bold: bool,
        #[serde(default, skip_serializing_if = "is_false")]
        italic: bool,
        #[serde(default)]
        h_align: SchHAlign,
        #[serde(default)]
        v_align: SchVAlign,
        #[serde(default, skip_serializing_if = "is_zero_um")]
        margin_um: Um,
    },
    /// `SCH_RULE_AREA`: a polygon that applies its attributes to everything inside it and to the
    /// nets crossing it (a netclass directive label on its border names the class).
    RuleArea {
        pts: Vec<Point>,
        #[serde(default, skip_serializing_if = "is_false")]
        exclude_from_sim: bool,
        #[serde(default, skip_serializing_if = "is_false")]
        exclude_from_bom: bool,
        #[serde(default, skip_serializing_if = "is_false")]
        exclude_from_board: bool,
        #[serde(default, skip_serializing_if = "is_false")]
        dnp: bool,
    },
    /// `SCH_DIRECTIVE_LABEL` (the "netclass flag"): a flag on a pole that attaches its fields to the
    /// net it touches. `orientation` is the pole's direction in millidegrees (0 right, 90000 up,
    /// 180000 left, 270000 down).
    Directive {
        at: Point,
        #[serde(default, skip_serializing_if = "is_zero_angle")]
        orientation: Millideg,
        #[serde(default)]
        shape: DirectiveShape,
        pin_length_um: Um,
        #[serde(default, skip_serializing_if = "String::is_empty")]
        netclass: String,
        #[serde(default, skip_serializing_if = "String::is_empty")]
        component_class: String,
    },
}

impl SchGraphicKind {
    /// The id prefix for this kind (`next_item_id`'s first argument).
    pub fn id_prefix(&self) -> &'static str {
        match self {
            SchGraphicKind::TextBox { .. } => "tbox",
            SchGraphicKind::RuleArea { .. } => "rarea",
            SchGraphicKind::Directive { .. } => "dirl",
            _ => "shp",
        }
    }

    /// True for the kinds drawn with a stroked outline that may be filled.
    pub fn is_closed_shape(&self) -> bool {
        matches!(self, SchGraphicKind::Rectangle { .. } | SchGraphicKind::Circle { .. } | SchGraphicKind::Polygon { .. } | SchGraphicKind::TextBox { .. } | SchGraphicKind::RuleArea { .. } | SchGraphicKind::Bezier { .. })
    }
}

/// One drawn graphic or annotation -- see [`SchGraphicKind`]. Stroke and fill apply to every kind
/// except `Directive` (which draws with the directive-label colour).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SchGraphic {
    /// Stable id (`shp_`/`tbox_`/`rarea_`/`dirl_` + 12 hex) -- see `Wire::id`'s doc.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub id: String,
    pub shape: SchGraphicKind,
    /// Stroke width, um; 0 means the default line width.
    #[serde(default, skip_serializing_if = "is_zero_um")]
    pub width_um: Um,
    #[serde(default, skip_serializing_if = "SchLineStyle::is_default")]
    pub line_style: SchLineStyle,
    /// Stroke colour; absent means the layer's own colour.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<SchColor>,
    #[serde(default, skip_serializing_if = "SchFill::is_none")]
    pub fill: SchFill,
    /// The fill colour when `fill` is `Color`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fill_color: Option<SchColor>,
}

impl SchGraphic {
    /// A graphic of `shape` with every style at its default.
    pub fn new(shape: SchGraphicKind) -> SchGraphic {
        SchGraphic { id: String::new(), shape, width_um: 0, line_style: SchLineStyle::Default, color: None, fill: SchFill::None, fill_color: None }
    }

    /// The seed `assign_missing_ids` hashes: the kind's own geometry, so the same drawing always gets the same id.
    pub fn id_seed(&self) -> String {
        let p = |a: &Point| format!("{},{}", a.x, a.y);
        match &self.shape {
            SchGraphicKind::Rectangle { start, end, .. } => format!("rect|{}|{}", p(start), p(end)),
            SchGraphicKind::Circle { center, radius_um } => format!("circle|{}|{radius_um}", p(center)),
            SchGraphicKind::Arc { start, mid, end } => format!("arc|{}|{}|{}", p(start), p(mid), p(end)),
            SchGraphicKind::Bezier { start, c1, c2, end } => format!("bezier|{}|{}|{}|{}", p(start), p(c1), p(c2), p(end)),
            SchGraphicKind::Polygon { pts } => format!("poly|{}", pts.iter().map(p).collect::<Vec<_>>().join(";")),
            SchGraphicKind::TextBox { start, end, text, .. } => format!("tbox|{}|{}|{text}", p(start), p(end)),
            SchGraphicKind::RuleArea { pts, .. } => format!("rarea|{}", pts.iter().map(p).collect::<Vec<_>>().join(";")),
            SchGraphicKind::Directive { at, netclass, .. } => format!("dirl|{}|{netclass}", p(at)),
        }
    }
}

fn is_zero_um(v: &Um) -> bool {
    *v == 0
}

fn is_zero_angle(v: &Millideg) -> bool {
    *v == 0
}

fn is_false(v: &bool) -> bool {
    !*v
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(x: Um, y: Um) -> Point {
        Point { x, y }
    }

    #[test]
    fn empty_extras_serialize_to_nothing_and_back() {
        let e = SchExtras::default();
        assert!(e.is_empty());
        assert_eq!(serde_json::to_string(&e).unwrap(), "{}");
        let back: SchExtras = serde_json::from_str("{}").unwrap();
        assert!(back.is_empty());
    }

    #[test]
    fn locking_keeps_the_ids_sorted_and_unique() {
        let mut e = SchExtras::default();
        assert!(e.set_locked("U2", true));
        assert!(e.set_locked("R1", true));
        assert!(!e.set_locked("R1", true), "already locked: no change");
        assert_eq!(e.locked, vec!["R1".to_string(), "U2".to_string()]);
        assert!(e.is_locked("U2") && !e.is_locked("U1"));
        assert!(e.set_locked("R1", false));
        assert!(!e.set_locked("R1", false), "already unlocked: no change");
        assert_eq!(e.locked, vec!["U2".to_string()]);
    }

    #[test]
    fn graphics_round_trip_through_json_and_get_distinct_ids() {
        let mut e = SchExtras::default();
        let mut r = SchGraphic::new(SchGraphicKind::Rectangle { start: p(0, 0), end: p(10_000, 5_000), corner_radius_um: 0 });
        r.fill = SchFill::Background;
        e.graphics.push(r);
        e.graphics.push(SchGraphic::new(SchGraphicKind::Circle { center: p(1, 2), radius_um: 3_000 }));
        e.graphics.push(SchGraphic::new(SchGraphicKind::RuleArea { pts: vec![p(0, 0), p(10, 0), p(10, 10)], exclude_from_sim: false, exclude_from_bom: true, exclude_from_board: false, dnp: false }));
        e.graphics.push(SchGraphic::new(SchGraphicKind::Directive { at: p(5, 5), orientation: 90_000, shape: DirectiveShape::Round, pin_length_um: 2_540, netclass: "HV".into(), component_class: String::new() }));
        let mut taken = BTreeSet::new();
        e.assign_missing_ids(&mut taken);
        let ids: Vec<&str> = e.graphics.iter().map(|g| g.id.as_str()).collect();
        assert!(ids[0].starts_with("shp_") && ids[1].starts_with("shp_") && ids[2].starts_with("rarea_") && ids[3].starts_with("dirl_"), "{ids:?}");
        assert_eq!(taken.len(), 4);
        let json = serde_json::to_string(&e).unwrap();
        let back: SchExtras = serde_json::from_str(&json).unwrap();
        assert_eq!(back, e);
        // Assigning again changes nothing.
        let mut again = back.clone();
        again.assign_missing_ids(&mut BTreeSet::new());
        assert_eq!(again, back);
    }

    fn j(h: TextJustify, v: TextVAlign) -> SchTextJustify {
        SchTextJustify { h, v }
    }

    /// `SCH_TEXT::Rotate90`: counter-clockwise from a horizontal text keeps the justification, clockwise flips it; the next turn undoes it.
    #[test]
    fn a_text_turn_flips_its_justification_as_rotate90_does() {
        use TextJustify::*;
        let left = j(Left, TextVAlign::Bottom);
        // ccw: horizontal -> vertical, no flip
        assert_eq!(left.rotated90(false, false), (true, left));
        // cw: horizontal -> vertical, flipped (it ends at the anchor now, as the text lies the other way)
        assert_eq!(left.rotated90(false, true), (true, j(Right, TextVAlign::Bottom)));
        // vertical -> horizontal: cw keeps it, ccw flips it
        assert_eq!(left.rotated90(true, true), (false, left));
        assert_eq!(left.rotated90(true, false), (false, j(Right, TextVAlign::Bottom)));
        // two counter-clockwise turns make a half turn: horizontal again, the other end on the anchor
        let (v, once) = left.rotated90(false, false);
        let (v2, twice) = once.rotated90(v, false);
        assert_eq!((v2, twice), (false, j(Right, TextVAlign::Bottom)));
        // four turns are the identity
        let mut state = (false, left);
        for _ in 0..4 {
            state = state.1.rotated90(state.0, false);
        }
        assert_eq!(state, (false, left));
        // a centred text has nothing to flip
        assert_eq!(SchTextJustify::FILE_DEFAULT.rotated90(false, true), (true, SchTextJustify::FILE_DEFAULT));
    }

    /// `SCH_TEXT::MirrorSpinStyle`, `MirrorHorizontally` and `MirrorVertically`: the horizontal justification flips only when the mirror is along the text.
    #[test]
    fn a_text_mirror_flips_its_justification_only_along_the_text() {
        use TextJustify::*;
        let left = j(Left, TextVAlign::Bottom);
        let right = j(Right, TextVAlign::Bottom);
        // X (left-right) on a horizontal text, Y (top-bottom) on a vertical one
        assert_eq!(left.mirrored_spin(false, true), right);
        assert_eq!(left.mirrored_spin(true, false), right);
        // the other axis leaves it
        assert_eq!(left.mirrored_spin(false, false), left);
        assert_eq!(left.mirrored_spin(true, true), left);
        // the several-items versions
        assert_eq!(left.mirrored_horizontally(false), right);
        assert_eq!(left.mirrored_horizontally(true), left);
        assert_eq!(left.mirrored_vertically(true), right);
        assert_eq!(left.mirrored_vertically(false), left);
        // twice is the identity
        assert_eq!(left.mirrored_spin(false, true).mirrored_spin(false, true), left);
    }

    #[test]
    fn a_text_justification_is_stored_only_when_it_differs_from_the_default() {
        use TextJustify::*;
        let mut e = SchExtras::default();
        assert!(e.text_justify_of("txt_a").is_default());
        assert!(!e.set_text_justify("txt_a", SchTextJustify::default()), "the default is no change");
        assert!(e.is_empty());
        assert!(e.set_text_justify("txt_a", j(Center, TextVAlign::Center)));
        assert!(!e.is_empty());
        assert!(!e.set_text_justify("txt_a", j(Center, TextVAlign::Center)));
        assert!(e.set_text_justify("txt_a", SchTextJustify::default()));
        assert!(e.is_empty(), "back to the default drops the entry");
        // a design from before the overlay has none and reads back unchanged; an entry with one member missing takes the default for it
        let back: SchExtras = serde_json::from_str(r#"{"text_justify": {"txt_a": {"h": "right"}}}"#).unwrap();
        assert_eq!(back.text_justify_of("txt_a"), j(Right, TextVAlign::Bottom));
        let json = serde_json::to_string(&back).unwrap();
        assert_eq!(serde_json::from_str::<SchExtras>(&json).unwrap(), back);
    }
}
