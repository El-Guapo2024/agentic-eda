//! What a library symbol's or a library footprint's file says that the library editors do not edit, kept so that the writers give it back.
//!
//! The Symbol Editor and the Footprint Editor edit [`crate::ir::LibrarySymbol`] and [`crate::ir::LibraryFootprint`], which hold what their dialogs and tools change. A
//! symbol or a footprint of KiCad's own libraries says more: a line's dash style, a text's bold or its vertical justification, a pad's fabrication property and mask
//! margins, a footprint's net-tie pad groups, its zones and points, a symbol's jumper pins. Read into these overlays (additive, `#[serde(default)]`), they are written
//! back where KiCad writes them (`SCH_IO_KICAD_SEXPR_LIB_CACHE::SaveSymbol`, `PCB_IO_KICAD_SEXPR::format( const FOOTPRINT* )`), so a symbol or footprint opened from a
//! library and saved again is the one KiCad wrote. The round trip of KiCad's installed libraries through the editors' readers and writers is checked against kicad-cli
//! (`crates/kicad/tests/library_roundtrip.rs`).
//!
//! What is kept by item id (a graphic's line style, a text's looks) stays with the item through moves and edits; a copy of the item (Duplicate, Create Array, Paste)
//! is a new item with the editor's defaults. A footprint form kept verbatim (a zone, a point, a dimension) is not moved by Set Anchor.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::board_text::TextExtra;
use crate::ir::Um;

fn is_false(b: &bool) -> bool {
    !*b
}

/// See the module doc: a library symbol's.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LibrarySymbolKeep {
    /// `(duplicate_pin_numbers_are_jumpers yes)` (`LIB_SYMBOL::GetDuplicatePinNumbersAreJumpers`, the Pin Connections tab's "All pins with duplicate numbers are jumpers").
    #[serde(default, skip_serializing_if = "is_false")]
    pub duplicate_pin_numbers_are_jumpers: bool,
    /// `(jumper_pin_groups ("1" "2") ..)` (`LIB_SYMBOL::JumperPinGroups`): each group's pin numbers, sorted as KiCad's `std::set` keeps them.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub jumper_pin_groups: Vec<Vec<String>>,
    /// A graphic's `(stroke (type ..))` when it is not `default` (`solid`, `dash`, `dot`, `dash_dot`, `dash_dot_dot`), by the graphic's id.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub stroke_types: BTreeMap<String, String>,
    /// `(unit_name "..")` of a unit's sub-symbol (`LIB_SYMBOL::GetUnitDisplayNames`), by unit number.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub unit_names: BTreeMap<u32, String>,
    /// What a text's `(effects ..)` says beyond its size (`LibrarySymbolGraphic::Text` has the text, its place, angle and size), by the text's id.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub text_styles: BTreeMap<String, KeptTextStyle>,
    /// The drawn items the editor does not draw or edit (a `(bezier ..)`), as the file wrote them, each with its unit and body style; written back into their
    /// sub-symbol. Move Symbol Anchor does not move them.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub items: Vec<KeptSymbolItem>,
}

/// See [`LibrarySymbolKeep::text_styles`]: `EDA_TEXT::Format`'s `(font (thickness ..) bold italic)`, `(justify ..)` and `(hide yes)`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KeptTextStyle {
    /// The pen, mm; `None` is the automatic one (no `(thickness ..)`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thickness_mm: Option<f64>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub bold: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub italic: bool,
    /// The words of `(justify ..)`, in the file's order (`left`, `right`, `top`, `bottom`, `mirror`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub justify: Vec<String>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub hidden: bool,
}

impl KeptTextStyle {
    pub fn is_default(&self) -> bool {
        *self == KeptTextStyle::default()
    }
}

/// See [`LibrarySymbolKeep::items`].
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KeptSymbolItem {
    pub unit: u32,
    pub body_style: u32,
    pub text: String,
}

impl LibrarySymbolKeep {
    pub fn is_empty(&self) -> bool {
        *self == LibrarySymbolKeep::default()
    }
}

/// See the module doc: a library footprint's.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LibraryFootprintKeep {
    /// The footprint's own forms the editor has no member for, as the file wrote them, in file order: `(clearance ..)`, `(solder_mask_margin ..)`,
    /// `(solder_paste_margin ..)`, `(solder_paste_margin_ratio ..)`, `(net_tie_pad_groups ..)`, `(private_layers ..)`, `(duplicate_pad_numbers_are_jumpers ..)`,
    /// `(jumper_pad_groups ..)`, and the items the editor does not draw: `(zone ..)`, `(point ..)`, `(dimension ..)`, `(image ..)`, `(fp_text_box ..)`, `(table ..)`,
    /// `(barcode ..)`, `(group ..)` (whose members are named by uuid, see [`Self::uuids`]: a member that is gone is left out of the group by KiCad's reader). The writer puts each where `format( FOOTPRINT )` writes its kind.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tokens: Vec<String>,
    /// A graphic's `(stroke (type ..))` when it is not `solid` (`default`, `dash`, `dot`, `dash_dot`, `dash_dot_dot`), by the graphic's id.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub stroke_types: BTreeMap<String, String>,
    /// What a text's `(effects ..)` says beyond [`crate::ir::Text`] (bold, italic, a vertical justification, a glyph width, no thickness, a font), by the text's id
    /// ([`TextExtra::id`]).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub text_extras: Vec<TextExtra>,
    /// The uuid the file gave each pad, graphic and text (by the item's id) and each field (by `field:<name>`): written back as it was, as KiCad keeps an item's uuid,
    /// so a group of the footprint still names its members. A new item, or a copy, has none and is given one made from its id.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub uuids: BTreeMap<String, String>,
    /// The texts that are `(unlocked yes)` (`PCB_TEXT::SetKeepUpright( false )`'s token in a footprint: the text turns with the footprint), by id.
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub unlocked_texts: BTreeSet<String>,
}

impl LibraryFootprintKeep {
    pub fn is_empty(&self) -> bool {
        *self == LibraryFootprintKeep::default()
    }

    pub fn text_extra(&self, id: &str) -> Option<&TextExtra> {
        self.text_extras.iter().find(|e| e.id == id)
    }
}

/// See the module doc: a library pad's.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LibraryPadKeep {
    /// `(pad "1" connect ..)` (`PAD_ATTRIB::CONN`, an edge connector pad): the pad is an SMD pad here ([`crate::footprint::PadKind::Smd`]) and is written back as
    /// `connect`. A pad that is no longer an SMD pad is written as its kind says.
    #[serde(default, skip_serializing_if = "is_false")]
    pub connector: bool,
    /// `(property pad_prop_bga)` and the others (`PAD::GetProperty`, the Pad Properties' "Fabrication property"): the token after `property`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub property: Option<String>,
    /// `(solder_mask_margin ..)`, µm (`PAD::GetLocalSolderMaskMargin`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub solder_mask_margin: Option<Um>,
    /// `(solder_paste_margin ..)`, µm.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub solder_paste_margin: Option<Um>,
    /// `(solder_paste_margin_ratio ..)`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub solder_paste_margin_ratio: Option<f64>,
    /// The pad's other forms as the file wrote them, in file order: `(die_length ..)`, `(die_delay ..)`, `(pinfunction ..)`, `(pintype ..)`, `(teardrops ..)`,
    /// `(tenting ..)`, `(zone_layer_connections ..)`, whatever a newer KiCad adds.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tokens: Vec<String>,
}

impl LibraryPadKeep {
    pub fn is_empty(&self) -> bool {
        *self == LibraryPadKeep::default()
    }

    /// What `PAD::ImportSettingsFrom` takes from a master pad (Paste and Push Pad Properties): the attribute, the property and the padstack's margins and tenting --
    /// not the pin-to-die length ("really specific to a given pad"), the pin function or the pin type.
    pub fn imported_from(&self, master: &LibraryPadKeep) -> LibraryPadKeep {
        let own = |t: &&String| t.starts_with("(die_length") || t.starts_with("(die_delay") || t.starts_with("(pinfunction") || t.starts_with("(pintype");
        let mut tokens: Vec<String> = self.tokens.iter().filter(own).cloned().collect();
        tokens.extend(master.tokens.iter().filter(|t| !own(t)).cloned());
        LibraryPadKeep {
            connector: master.connector,
            property: master.property.clone(),
            solder_mask_margin: master.solder_mask_margin,
            solder_paste_margin: master.solder_paste_margin,
            solder_paste_margin_ratio: master.solder_paste_margin_ratio,
            tokens,
        }
    }
}
