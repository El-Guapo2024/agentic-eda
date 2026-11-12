//! What a library symbol's or a library footprint's file says that the library editors do not edit, kept so that the writers give it back.
//!
//! The Symbol Editor and the Footprint Editor edit [`crate::ir::LibrarySymbol`] and [`crate::ir::LibraryFootprint`], which hold what their dialogs and tools change. A
//! symbol or a footprint of KiCad's own libraries says more: a line's dash style, a text's bold or its vertical justification, a pad's fabrication property and mask
//! margins, a footprint's net-tie pad groups, its zones and points, a symbol's jumper pins. Read into these overlays (additive, `#[serde(default)]`), they are written
//! back where KiCad writes them (`SCH_IO_KICAD_SEXPR_LIB_CACHE::SaveSymbol`, `PCB_IO_KICAD_SEXPR::format( const FOOTPRINT* )`), so a symbol or footprint opened from a
//! library and saved again is the one KiCad wrote. The round trip of KiCad's installed libraries through the editors' readers and writers is checked against kicad-cli
//! (`crates/kicad/tests/library_roundtrip.rs`).
//!
//! What is kept by item id (a graphic's line style, a text's looks) stays with the item through moves and edits; a copy of a footprint's item (Duplicate, Create Array,
//! Paste) takes its original's ([`LibraryFootprintKeep::copy_looks`], [`LibraryFootprintKeep::copy_looks_from`], UI-LEFT-2 FF2). A footprint form kept verbatim (a zone,
//! a point, a dimension) is not moved by Set Anchor.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::board_text::TextExtra;
use crate::ir::Nm;

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
    /// What a text's `(effects ..)` says beyond its size (`LibrarySymbolGraphic::Text` has the text, its place, angle and size), by the text's id. A text box's pen
    /// (`thickness_mm`) is kept here too (UI-LEFT-2 YY2).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub text_styles: BTreeMap<String, KeptTextStyle>,
    /// The drawn items the editor does not draw or edit (a `(bezier ..)`), as the file wrote them, each with its unit and body style; written back into their
    /// sub-symbol. Move Symbol Anchor does not move them.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub items: Vec<KeptSymbolItem>,
    /// What a graphic has that its kind does not hold as members (UI-LEFT-2 YY2): a shape's Private flag, border and fill colours, hatched fill and a rectangle's
    /// corner radius, a text's or text box's hyperlink -- by the graphic's id. Edited by `set_symbol_graphic_looks` (the Properties pane, Shape Properties); read
    /// and written where `SCH_IO_KICAD_SEXPR_LIB_CACHE::SaveSymbol` has them. Additive: absent reads as none.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub graphic_looks: BTreeMap<String, KeptGraphicLooks>,
    /// A field's font face and colour (`EDA_TEXT::Format`'s `(face ..)` and `(color ..)` in the field's `(effects (font ..))`), by the field's name (UI-LEFT-2 YY2).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub field_looks: BTreeMap<String, KeptFieldLooks>,
}

/// See [`LibrarySymbolKeep::graphic_looks`].
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KeptGraphicLooks {
    /// `SCH_ITEM::IsPrivate` of a shape (`(rectangle private ..)`, Shape Properties' "Private to Symbol Editor"); a text and a text box hold their own.
    #[serde(default, skip_serializing_if = "is_false")]
    pub private: bool,
    /// A shape's border colour (`STROKE_PARAMS::GetColor`, `(stroke .. (color r g b a))`); `None` is `COLOR4D::UNSPECIFIED`. A text box holds its own.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stroke_color: Option<crate::sch_extras::SchColor>,
    /// A shape's fill colour (`EDA_SHAPE::GetFillColor`, `formatFill`'s `(color ..)` of a colour or hatched fill); `None` is `COLOR4D::UNSPECIFIED`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fill_color: Option<crate::sch_extras::SchColor>,
    /// A hatched fill (`FILL_T::HATCH` / `REVERSE_HATCH` / `CROSS_HATCH`: `hatch`, `reverse_hatch`, `cross_hatch`), the shape's `fill` then being
    /// [`crate::ir::LibraryFill::Color`] (a hatch is drawn in the fill colour; `formatFill` writes its colour as for `color`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hatch: Option<String>,
    /// A rectangle's corner radius, mm (`EDA_SHAPE::GetCornerRadius`, `formatRect`'s `(radius ..)`); 0 is square corners.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub radius_mm: f64,
    /// A text's or text box's hyperlink (`EDA_TEXT::GetHyperlink`, `(href "..")` in its `(effects ..)`).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub hyperlink: String,
}

fn is_zero(v: &f64) -> bool {
    *v == 0.0
}

impl KeptGraphicLooks {
    pub fn is_default(&self) -> bool {
        *self == KeptGraphicLooks::default()
    }
}

/// See [`LibrarySymbolKeep::field_looks`].
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KeptFieldLooks {
    /// The face (`EDA_TEXT::GetFont`); `None` is the default font.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub font: Option<String>,
    /// The colour (`EDA_TEXT::GetTextColor`); `None` is `COLOR4D::UNSPECIFIED`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<crate::sch_extras::SchColor>,
    /// The pen, mm (`(thickness ..)`); `None` is Auto Thickness.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thickness_mm: Option<f64>,
}

impl KeptFieldLooks {
    pub fn is_default(&self) -> bool {
        *self == KeptFieldLooks::default()
    }
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
    /// `(jumper_pad_groups ..)`, and the items [`crate::fp_items::FootprintItems`] cannot hold: a zone, point, dimension, image, text box, table, barcode or group the
    /// board's reader of its kind refuses, a locked one (a library footprint keeps no lock), a zone with a fill, thieving, per-layer settings or arcs, and a group that is
    /// locked or names one of those (its members are named by uuid, see [`Self::uuids`]: a member that is gone is left out of the group by KiCad's reader). Every other
    /// such item is read into `LibraryFootprint::items` since UI-LEFT F2; a `design.json` written before that still has them here. The writer puts each where
    /// `format( FOOTPRINT )` writes its kind.
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
    /// The graphics and texts that are `(locked yes)` (`BOARD_ITEM::IsLocked`; a text of KiCad 5.99 says a bare `locked`), by id (UI-LEFT F5). A pad keeps its
    /// `(locked yes)` among its own kept forms ([`LibraryPadKeep::tokens`]).
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub locked: BTreeSet<String>,
    /// The texts that are hidden (`fp_text user .. (hide yes)`, or a bare `hide` in an older file's effects: `parsePCB_TEXT` sets `SetVisible( false )` on a text of a
    /// footprint), by id (UI-LEFT F5). Not drawn and not picked, as `PCB_TEXT::ViewGetLOD` hides a text that is not visible. KiCad's writer drops the flag of a user
    /// text (`format( const PCB_TEXT* )` writes `(hide yes)` for a field only), so KiCad shows the text again once it has saved the file; the studio's writer keeps it.
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub hidden_texts: BTreeSet<String>,
    /// A graphic's hatched fill (`(fill hatch|reverse_hatch|cross_hatch)`, `EDA_SHAPE::SetFillMode`), by id; the graphic's `filled` is then false (UI-LEFT F5).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub hatches: BTreeMap<String, crate::fp_art::Hatch>,
    /// KiCad 10's line endings of a line, an arc or a curve (`(start_shape ..)`, `(end_shape ..)` after the stroke, `EDA_SHAPE::SetStartEnding` / `SetEndEnding`, KiCad
    /// 25ec2335), by the graphic's id (UI-LEFT-2 FF3: the Properties pane's Start Shape .. End Stroke Width rows). Additive.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub endings: BTreeMap<String, crate::line_ending::LineEndings>,
    /// A zone's `(filled_polygon ..)`s as the file stored them (UI-LEFT-2 FF6: KiCad's Footprint Editor runs no zone filler, and draws and writes the fill a footprint
    /// zone brought, `ZONE::m_FilledPolysList`, from a board by Ctrl+E or the file), by the zone's id. Additive.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub zone_fills: BTreeMap<String, KeptZoneFill>,
    /// A copper thieving zone's `(thieving ..)` settings, by the zone's id, as the board keeps them (`DrawingsSection::zone_thieving`). Additive.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub zone_thieving: BTreeMap<String, String>,
    /// A zone's per-layer `(property ..)` forms the reader cannot take apart, by the zone's id (`DrawingsSection::zone_layer_props`). Additive.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub zone_layer_props: BTreeMap<String, String>,
    /// The rings of a zone outline drawn with arcs, as the file wrote them (`DrawingsSection::zone_arcs`, the id inside), written while the zone keeps the outline read.
    /// Additive.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub zone_arcs: Vec<crate::ir::ZoneArcs>,
}

/// The fill a footprint's zone brought (`LibraryFootprintKeep::zone_fills`): its rings on each layer, in nanometres in the footprint's frame, whether the file said the
/// zone is filled (`(fill yes ..)`, `ZONE::IsFilled`), and the zone's outline, holes and layers it was made for, which a fill follows when the zone is moved, turned or
/// mirrored (`ZONE::Move` / `Rotate` / `Mirror` move `m_FilledPolysList`) and loses when it is reshaped (the point editor's `UnFill()`).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KeptZoneFill {
    /// The fingerprint of the zone the fill was read with (`eda_kicad::zone_fills::zone_signature`).
    #[serde(default)]
    pub sig: String,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub unfilled: bool,
    /// Layer -> its rings, each `(island, points)`.
    #[serde(default)]
    pub layers: BTreeMap<String, Vec<(bool, Vec<[i64; 2]>)>>,
    pub outline: Vec<crate::ir::Point>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub holes: Vec<Vec<crate::ir::Point>>,
    pub zone_layers: Vec<String>,
}

impl LibraryFootprintKeep {
    pub fn is_empty(&self) -> bool {
        *self == LibraryFootprintKeep::default()
    }

    pub fn text_extra(&self, id: &str) -> Option<&TextExtra> {
        self.text_extras.iter().find(|e| e.id == id)
    }

    /// What is kept by item id of `from` (its line style, hatch, text looks, keep-upright, lock, hidden flag), copied to the item `to`: a copy of the item
    /// (`BOARD_ITEM::Duplicate` clones every attribute but the uuid) looks as the original does.
    pub fn copy_looks(&mut self, from: &str, to: &str) {
        if let Some(t) = self.stroke_types.get(from).cloned() {
            self.stroke_types.insert(to.to_string(), t);
        }
        if let Some(h) = self.hatches.get(from).copied() {
            self.hatches.insert(to.to_string(), h);
        }
        if let Some(e) = self.endings.get(from).cloned() {
            self.endings.insert(to.to_string(), e);
        }
        if let Some(mut e) = self.text_extra(from).cloned() {
            e.id = to.to_string();
            self.text_extras.retain(|x| x.id != to);
            self.text_extras.push(e);
        }
        for set in [&mut self.unlocked_texts, &mut self.locked, &mut self.hidden_texts] {
            if set.contains(from) {
                set.insert(to.to_string());
            }
        }
        self.copy_zone_keep(from, to);
    }

    /// A copied zone's fill (`ZONE::InitDataFromSrcInCopyCtor` copies `m_FilledPolysList`; it follows the copy where it is put), thieving and per-layer settings.
    fn copy_zone_keep(&mut self, from: &str, to: &str) {
        if let Some(f) = self.zone_fills.get(from).cloned() {
            self.zone_fills.insert(to.to_string(), f);
        }
        if let Some(t) = self.zone_thieving.get(from).cloned() {
            self.zone_thieving.insert(to.to_string(), t);
        }
        if let Some(p) = self.zone_layer_props.get(from).cloned() {
            self.zone_layer_props.insert(to.to_string(), p);
        }
    }

    /// What `other` keeps by the id `from` (another footprint's: the clipboard's, or the one copied to it), kept here by the id `to` -- a pasted or copied item looks
    /// as it did there. Nothing of `to` is kept here before.
    pub fn copy_looks_from(&mut self, other: &LibraryFootprintKeep, from: &str, to: &str) {
        self.forget(to);
        if let Some(t) = other.stroke_types.get(from) {
            self.stroke_types.insert(to.to_string(), t.clone());
        }
        if let Some(h) = other.hatches.get(from) {
            self.hatches.insert(to.to_string(), *h);
        }
        if let Some(e) = other.endings.get(from) {
            self.endings.insert(to.to_string(), e.clone());
        }
        if let Some(e) = other.text_extra(from) {
            self.text_extras.push(TextExtra { id: to.to_string(), ..e.clone() });
        }
        for (dst, src) in [(&mut self.unlocked_texts, &other.unlocked_texts), (&mut self.locked, &other.locked), (&mut self.hidden_texts, &other.hidden_texts)] {
            if src.contains(from) {
                dst.insert(to.to_string());
            }
        }
        if let Some(f) = other.zone_fills.get(from) {
            self.zone_fills.insert(to.to_string(), f.clone());
        }
        if let Some(t) = other.zone_thieving.get(from) {
            self.zone_thieving.insert(to.to_string(), t.clone());
        }
        if let Some(p) = other.zone_layer_props.get(from) {
            self.zone_layer_props.insert(to.to_string(), p.clone());
        }
    }

    /// Forget what is kept by the id `id` (the item is gone).
    pub fn forget(&mut self, id: &str) {
        self.stroke_types.remove(id);
        self.hatches.remove(id);
        self.endings.remove(id);
        self.text_extras.retain(|e| e.id != id);
        for set in [&mut self.unlocked_texts, &mut self.locked, &mut self.hidden_texts] {
            set.remove(id);
        }
        self.zone_fills.remove(id);
        self.zone_thieving.remove(id);
        self.zone_layer_props.remove(id);
        self.zone_arcs.retain(|a| a.id != id);
    }
}

/// The number of a kept `(tag value)` token. The kept tokens ([`LibraryFootprintKeep::tokens`], [`crate::ir::FootprintExtra::tokens`],
/// [`crate::ir::PadMaskInfo::tokens`]) are the file's forms as the reader serializes them back -- one list, its atoms one space apart -- so the
/// value the parser read is still there for a comparison that needs it (`FootprintNeedsUpdate`'s margins).
pub fn kept_number(tokens: &[String], tag: &str) -> Option<f64> {
    let head = format!("({tag} ");
    tokens.iter().find_map(|t| t.strip_prefix(&head)?.strip_suffix(')')?.trim().parse().ok())
}

/// The length of a kept `(tag value)` token, read as KiCad reads a board length ([`Nm::from_mm`]).
pub fn kept_mm(tokens: &[String], tag: &str) -> Option<Nm> {
    kept_number(tokens, tag).map(Nm::from_mm)
}

/// The word of a kept `(tag word)` token, one that is not a number (`(property pad_prop_heatsink)`).
pub fn kept_atom(tokens: &[String], tag: &str) -> Option<String> {
    let head = format!("({tag} ");
    tokens.iter().find_map(|t| {
        let word = t.strip_prefix(&head)?.strip_suffix(')')?.trim();
        (!word.is_empty() && !word.contains(' ')).then(|| word.to_string())
    })
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
    /// `(solder_mask_margin ..)`, nm (`PAD::GetLocalSolderMaskMargin`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "crate::units::de")]
    pub solder_mask_margin: Option<Nm>,
    /// `(solder_paste_margin ..)`, nm.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "crate::units::de")]
    pub solder_paste_margin: Option<Nm>,
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

/// Whether a kept footprint form ([`LibraryFootprintKeep::tokens`]) is an item `FOOTPRINT::MoveAnchorPosition` moves: one of `GraphicalItems()` (a dimension, an image, a
/// text box, a table, a barcode) or `Zones()`. A point (`m_points` is in neither list), a group (no place of its own) and the footprint's settings do not move.
pub fn kept_form_moves_with_anchor(token: &str) -> bool {
    let tag = token.trim_start().trim_start_matches('(').split(|c: char| c.is_whitespace() || c == '(' || c == ')').next().unwrap_or("");
    matches!(tag, "zone" | "dimension" | "image" | "fp_text_box" | "gr_text_box" | "table" | "barcode")
}

/// `BOARD_ITEM::Move( aMoveVector )` of an item kept as the text of its form (UI-LEFT F7, `FOOTPRINT::MoveAnchorPosition` of what the editor holds as text: a locked zone,
/// a filled one, a group's member ...): every point of the form -- the first two numbers of each `(xy ..)`, `(start ..)`, `(end ..)`, `(mid ..)`, `(center ..)` and `(at ..)` --
/// moved by `(dx, dy)`, written in millimetres as KiCad writes lengths; the rest of the text is left as it was.
pub fn translate_kept_form(token: &str, dx: Nm, dy: Nm) -> String {
    const POINT_HEADS: &[&str] = &["xy", "start", "end", "mid", "center", "at"];
    let b = token.as_bytes();
    let mut out = String::with_capacity(token.len() + 16);
    let mut i = 0;
    // how many numbers of the innermost list still move, and which axis comes next
    let mut stack: Vec<u8> = Vec::new();
    while i < b.len() {
        match b[i] {
            b'"' => {
                let start = i;
                i += 1;
                while i < b.len() && b[i] != b'"' {
                    if b[i] == b'\\' {
                        i += 1;
                    }
                    i += 1;
                }
                i = (i + 1).min(b.len());
                out.push_str(&token[start..i]);
            }
            b'(' => {
                out.push('(');
                i += 1;
                let start = i;
                while i < b.len() && !matches!(b[i], b' ' | b'\t' | b'\r' | b'\n' | b'(' | b')') {
                    i += 1;
                }
                let head = &token[start..i];
                out.push_str(head);
                stack.push(if POINT_HEADS.contains(&head) { 2 } else { 0 });
            }
            b')' => {
                out.push(')');
                stack.pop();
                i += 1;
            }
            c if c.is_ascii_whitespace() => {
                out.push(c as char);
                i += 1;
            }
            _ => {
                let start = i;
                while i < b.len() && !matches!(b[i], b' ' | b'\t' | b'\r' | b'\n' | b'(' | b')' | b'"') {
                    i += 1;
                }
                let atom = &token[start..i];
                match (stack.last_mut(), atom.parse::<f64>()) {
                    (Some(left @ 1..=2), Ok(v)) => {
                        let d = if *left == 2 { dx } else { dy };
                        *left -= 1;
                        out.push_str(&mm_text(Nm::from_mm(v) + d));
                    }
                    _ => out.push_str(atom),
                }
            }
        }
    }
    out
}

/// A length in millimetres as KiCad's files write one (`FormatInternalUnits`: up to six decimals, no trailing zeros).
fn mm_text(v: Nm) -> String {
    let s = format!("{:.6}", v.mm());
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s == "-0" || s.is_empty() {
        "0".to_string()
    } else {
        s.to_string()
    }
}

#[cfg(test)]
mod anchor_tests {
    use super::*;

    #[test]
    fn a_kept_zone_and_dimension_move_with_the_anchor_and_a_point_does_not() {
        let zone = "(zone (net 0) (net_name \"\") (layer \"F.Cu\") (locked yes) (hatch edge 0.5) (polygon (pts (xy 0 0) (xy 1.5 0) (arc (start 1.5 0) (mid 2 0.5) (end 1.5 1)))) (filled_polygon (layer \"F.Cu\") (pts (xy 0.1 0.1) (xy 1 0.1) (xy 1 1))))";
        assert!(kept_form_moves_with_anchor(zone));
        let moved = translate_kept_form(zone, Nm::from_mm(-1.0), Nm::from_mm(0.25));
        assert_eq!(
            moved,
            "(zone (net 0) (net_name \"\") (layer \"F.Cu\") (locked yes) (hatch edge 0.5) (polygon (pts (xy -1 0.25) (xy 0.5 0.25) (arc (start 0.5 0.25) (mid 1 0.75) (end 0.5 1.25)))) (filled_polygon (layer \"F.Cu\") (pts (xy -0.9 0.35) (xy 0 0.35) (xy 0 1.25))))"
        );
        let dim = "(dimension (type aligned) (layer \"F.Fab\") (pts (xy 0 0) (xy 2 0)) (height 1) (gr_text \"2 mm\" (at 1 -1 0) (layer \"F.Fab\") (effects (font (size 1 1) (thickness 0.15)))))";
        assert_eq!(translate_kept_form(dim, Nm::from_mm(1.0), Nm::ZERO), "(dimension (type aligned) (layer \"F.Fab\") (pts (xy 1 0) (xy 3 0)) (height 1) (gr_text \"2 mm\" (at 2 -1 0) (layer \"F.Fab\") (effects (font (size 1 1) (thickness 0.15)))))");
        assert!(!kept_form_moves_with_anchor("(point (at 1 2) (size 1) (layer \"F.Fab\"))"));
        assert!(!kept_form_moves_with_anchor("(group \"\" (members \"a\"))"));
        assert!(!kept_form_moves_with_anchor("(stackup (layer \"F.Cu\"))"));
    }
}
