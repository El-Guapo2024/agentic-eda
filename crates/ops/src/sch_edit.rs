//! The schematic editor's edit and drawing tools that act on items already on the sheet, ported from
//! KiCad's `eeschema/tools` (`SCH_EDIT_TOOL`, `SCH_DRAWING_TOOLS`, `SCH_POINT_EDITOR`, ...).
//!
//! One `Cmd` variant carries all of them -- `Cmd::SchEdit(SchCmd)`, `{"op": "sch_edit", "verb": ...}` on the
//! wire -- so this family can grow without touching the flat verb list. Every verb is a pure edit of
//! `design.schematic` (or one of the `sheet_contents` screens) and therefore undoable like any other; each one
//! cites the KiCad function it ports (eeschema at 8303b2ad).

use crate::{Board, Cmd};
use eda_model::ir::{LabelKind, LabelShape, Millideg, Point, SchematicSection, SheetInstance, SheetPin, TextJustify, TextVAlign, Nm};
use eda_model::sch_extras::{LabelSpin, SchColor, SchGraphic, SchGraphicKind, SchLineStyle};
use eda_model::CheckResult;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

/// One schematic edit verb -- see the module doc.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "verb", rename_all = "snake_case")]
pub enum SchCmd {
    /// Lock / Unlock / Toggle Lock (`SCH_EDIT_TOOL::modifyLockSelected`): set `SCH_ITEM::SetLocked` on every
    /// listed item. KiCad skips pins, fields and sheet pins ("they inherit from parent"); so does this (a sheet
    /// pin id is accepted and ignored). The toggle's own rule -- unlock when any selected item is locked, else
    /// lock -- is the caller's: it picks `locked` and sends one verb.
    SetLocked { ids: Vec<String>, locked: bool },
    /// Draw a rectangle, circle, arc, bezier, polygon, text box, rule area or directive label
    /// (`SCH_DRAWING_TOOLS::DrawShape` / `DrawRuleArea` / the class-label branch of `TwoClickPlace`): appended to
    /// the drawing order; its id is assigned here. Degenerate shapes (zero size, fewer points than the kind
    /// needs, collinear arc points) are refused.
    AddGraphic { graphic: SchGraphic },
    /// Delete one drawn graphic (`SCH_EDIT_TOOL::DoDelete`).
    DeleteGraphic { id: String },
    /// Replace a drawn graphic in place, keeping its place in the drawing order and its lock (what the
    /// properties dialogs and the point editor commit).
    EditGraphic { id: String, graphic: SchGraphic },
    /// Delete a placed hierarchical sheet (`SCH_EDIT_TOOL::DoDelete` on a `SCH_SHEET`): the sheet symbol and its
    /// pins go; the file's content stays in the project (another sheet may use it, or it may be placed again).
    DeleteSheet { id: String },
    /// Put a pin on a placed sheet's border (`SCH_SHEET::AddPin`, as `SCH_DRAWING_TOOLS::TwoClickPlace` and
    /// `AutoPlaceAllSheetPins` do): `name` is the hierarchical label it stands for in the sheet's file, `at` a
    /// point on the sheet's border (`SCH_SHEET_PIN::ConstrainOnEdge` is the caller's). An empty name, an unknown
    /// sheet or a point off the border is refused.
    AddSheetPin { sheet: String, name: String, shape: LabelShape, at: Point },
    /// Delete one sheet pin (`SCH_EDIT_TOOL::DoDelete` on a pin; what `CleanupSheetPins` and the sync dialog send).
    DeleteSheetPin { id: String },
    /// Rename, reshape or move one sheet pin (`SCH_SHEET_PIN` properties and the sync dialog's "update"); a field left
    /// out is unchanged. The new position must still be on the border. `size_nm` (0.01 to 1000 mm), `bold` and `italic` are how the pin's text is set
    /// (`DIALOG_SHEET_PIN_PROPERTIES`: `SCH_SHEET_PIN` is an `EDA_TEXT`).
    EditSheetPin {
        id: String,
        #[serde(default)]
        name: Option<String>,
        #[serde(default)]
        shape: Option<LabelShape>,
        #[serde(default)]
        at: Option<Point>,
        #[serde(default)]
        size_nm: Option<Nm>,
        #[serde(default)]
        bold: Option<bool>,
        #[serde(default)]
        italic: Option<bool>,
    },
    /// Change Symbol (`DIALOG_CHANGE_SYMBOLS::processSymbols`, `MODE::CHANGE`): every placed unit of the reference `id`
    /// takes the library symbol `lib_id`; position, orientation, unit and fields stay (fields are the caller's to reset).
    /// Refused when no library symbol has that id, or when the new symbol has fewer units than one of the placed ones
    /// ("new symbol has too few units"), or when the reference already uses it.
    ChangeSymbol { id: String, lib_id: String },
    /// Cycle Body Style (`SCH_EDIT_TOOL::CycleBodyStyle`) and the body style choice of Symbol Properties (`DIALOG_SYMBOL_PROPERTIES::TransferDataFromWindow`,
    /// both through `SCH_EDIT_FRAME::SelectBodyStyle`): every placed unit of the references `ids` is drawn in body style `style` (1 the normal one, 2 the alternate
    /// "De Morgan" one of a library symbol that has it, clamped to the symbol's count), or in the next one when `style` is left out (`GetBodyStyle() + 1`, back to 1
    /// after the last). A symbol whose library symbol has one body style is left as it is, as is one already in `style`; refused when no symbol changes. The pins
    /// of the other body style may be elsewhere: wires stay where they are and are read again (`SCH_SYMBOL::SetBodyStyle` updates the pins, nothing moves the wires).
    SetBodyStyle {
        ids: Vec<String>,
        #[serde(default)]
        style: Option<u32>,
    },
    /// Update Symbol(s) from Library (`DIALOG_CHANGE_SYMBOLS`, `MODE::UPDATE`): the symbols placed with these library ids start
    /// resolving from the project's edited library symbol of that id (`LibrarySymbol::published`, what the Symbol Editor's
    /// "Update Symbol in Schematic" sets). Library ids with no edited symbol, or one already published, are skipped; refused when
    /// none was left to update.
    UpdateLibrarySymbols { lib_ids: Vec<String> },
    /// Label Properties on a local, global or hierarchical label (`DIALOG_LABEL_PROPERTIES::TransferDataFromWindow`): a field left out is
    /// unchanged. `text` is the net name (refused when empty: "Label can not be empty."), `shape` the flag of a global or hierarchical label
    /// (a local label has none), `spin` which way the text runs from the anchor (`SetSpinStyle`).
    ///
    /// `size_nm` (0.01 to 1000 mm, as the dialog validates it), `bold` and `italic` set how the text is drawn (`SetTextSize`, `SetBold`, `SetItalic`); an
    /// item that has them all at their defaults keeps no entry (`SchExtras::label_looks`).
    ///
    /// `auto_rotate` is the "Auto" check of a global or hierarchical label (`SetAutoRotateOnPlacement`, then `AutoRotateItem`): while it is on the label takes
    /// the spin of what it is connected to and `spin` is not applied (`if( !AutoRotateOnPlacement() && spin != selected ) SetSpinStyle( selected )`). Refused
    /// for a local label, which has no such check.
    ///
    /// `fields` is the dialog's grid of the label's own fields (`SCH_LABEL_BASE::m_fields`) as OK leaves them: its net class (`Netclass`, the name "Net Class"
    /// reads as) and the others, each with its text, whether it is shown and its italics; a global label's Sheet References is not among them (Schematic Setup
    /// shows or hides it). An empty, unnamed field is dropped, an empty net class when another is there too, an unnamed one is "untitled"; two fields of one name
    /// are refused ("Field name '%s' already in use.").
    EditLabel {
        id: String,
        #[serde(default)]
        text: Option<String>,
        #[serde(default)]
        shape: Option<LabelShape>,
        #[serde(default)]
        spin: Option<LabelSpin>,
        #[serde(default)]
        size_nm: Option<Nm>,
        #[serde(default)]
        bold: Option<bool>,
        #[serde(default)]
        italic: Option<bool>,
        #[serde(default)]
        auto_rotate: Option<bool>,
        #[serde(default)]
        fields: Option<Vec<eda_model::sch_extras::LabelField>>,
    },
    /// Place Label, Global Label and Hierarchical Label (`SCH_DRAWING_TOOLS::TwoClickPlace` after `createNewLabel`): a label named `net` of `kind` at `at`, with the
    /// properties the dialog gave it, as one step. `spin` is which way the text runs from the anchor (the usual label with none given reads off the wire it ends
    /// at), `size_nm` (0.01 to 1000 mm), `bold` and `italic` set the text, and `auto_rotate` is the dialog's "Auto" check of a global or hierarchical label
    /// (`SCH_LABEL_BASE::SetAutoRotateOnPlacement`): the label then takes the spin of what it is placed on (`AutoRotateItem`) and `spin` is ignored. Refused for
    /// an empty name, and for "Auto" on a local label.
    PlaceLabel {
        net: String,
        at: Point,
        kind: LabelKind,
        #[serde(default)]
        spin: Option<LabelSpin>,
        #[serde(default)]
        size_nm: Option<Nm>,
        #[serde(default)]
        bold: Option<bool>,
        #[serde(default)]
        italic: Option<bool>,
        #[serde(default)]
        auto_rotate: Option<bool>,
    },
    /// Place Text (`SCH_DRAWING_TOOLS::TwoClickPlace` after `createNewText`): a free text at `at` running along the sheet or, with `angle` 90000, up it, justified
    /// `h` and `v` about its anchor (KiCad's last-used justification is centre and centre, which a text placed without one has). Refused for an empty text.
    PlaceText {
        content: String,
        at: Point,
        #[serde(default)]
        angle: Millideg,
        size_nm: Nm,
        #[serde(default)]
        h: Option<TextJustify>,
        #[serde(default)]
        v: Option<TextVAlign>,
    },
    /// Swap Pins (`SCH_EDIT_TOOL::SwapPins`): the pins `pins` (item ids `pin:<reference>:<number>`, in selection order) of one symbol trade places, one after the
    /// other (`SwapPinGeometry`: pin 1 and pin 2, then pin 2 and pin 3, ...), so each takes the place the next one had and the last the first's. The wires stay,
    /// the junctions the moved pins need are added and the sheet is cleaned up. Refused for fewer than two pins, for pins of different symbols and for a
    /// symbol of a sheet that is placed more than once ("Pin swaps are disabled for symbols used by multiple sheet instances").
    SwapPins { pins: Vec<String> },
    /// Swap Pin Labels (`SCH_EDIT_TOOL::SwapPinLabels`): the net labels on the pins `pins` trade texts in selection order. Refused unless every pin has
    /// exactly one net label on its connection and no other pin there ("Each selected pin must have exactly one attached net label and no other pin connections.").
    SwapPinLabels { pins: Vec<String> },
    /// Swap Unit Labels (`SCH_EDIT_TOOL::SwapUnitLabels`): the placed units of the multi-unit reference `ids` (one reference) trade the net labels on their pins,
    /// each unit's labels ordered by pin position. Refused unless every pin has exactly one net label and no other pin on its connection.
    SwapUnitLabels { ids: Vec<String> },
    /// Field Properties on one field of a symbol, a power symbol or a sheet (`DIALOG_FIELD_PROPERTIES::UpdateField`, `SCH_EDIT_TOOL::editFieldText`): `id` is
    /// the field's (`fld:<owner>:<name>`, `eda_engine::fields_edit::field_id`). A member left out is unchanged. `text` renames a symbol (Reference), sets its
    /// Value, Footprint or Datasheet, the net a power symbol asserts, or a sheet's name or file; `at` is where the text's anchor goes on the sheet,
    /// `vertical` whether it runs up the sheet and `h`, `v` how it is justified as it reads there; `size_nm` is 0.01 to 1000 mm; `visible` shows or hides it,
    /// `name_shown` writes `Name: value`, `allow_autoplace` lets Autoplace Fields move it. Changing where it is takes the item out of the autoplaced ones
    /// (`SetFieldsAutoplaced( AUTOPLACE_NONE )`); an item whose fields are still autoplaced has them placed again for the new text, size or visibility.
    EditField {
        id: String,
        #[serde(default)]
        text: Option<String>,
        #[serde(default)]
        at: Option<Point>,
        #[serde(default)]
        vertical: Option<bool>,
        #[serde(default)]
        h: Option<eda_model::ir::TextJustify>,
        #[serde(default)]
        v: Option<eda_model::ir::TextVAlign>,
        #[serde(default)]
        size_nm: Option<Nm>,
        #[serde(default)]
        bold: Option<bool>,
        #[serde(default)]
        italic: Option<bool>,
        #[serde(default)]
        visible: Option<bool>,
        #[serde(default)]
        name_shown: Option<bool>,
        #[serde(default)]
        allow_autoplace: Option<bool>,
    },
    /// Autoplace Fields (`SCH_EDIT_TOOL::AutoplaceFields`): the fields of each listed symbol, power symbol or sheet (or of the item a listed field belongs to)
    /// are placed by the manual routine (`AUTOPLACE_MANUAL`: beside the symbol, on a side that has no pins, clear of what is drawn around it) and stay
    /// autoplaced, so a later turn of the symbol places them again. Refused when every field is already where it would put them.
    AutoplaceFields { ids: Vec<String> },
    /// Pin Helpers (`PIN_TRICKS_MENU`, `SCH_SELECTION_TOOL::Main` on `ID_POPUP_SCH_PIN_TRICKS_*`): `what` -- a wire stub, a no-connect, a local, a hierarchical or a global label --
    /// on every pin (`pin:<reference>:<number>`) and sheet pin (`shpin_..`) of `pins`; anything else in the list is passed over, and a list with no pin in it is refused.
    /// `grid` is the wire grid a stub is one step of (micrometres; 0 is 50 mil). One undo step.
    PinHelper {
        what: crate::sch_pin_helpers::PinHelper,
        pins: Vec<String>,
        #[serde(default)]
        grid: Nm,
    },
    /// Add a user-defined field to a symbol (`DIALOG_SYMBOL_PROPERTIES::OnAddField`): `id` is the symbol's reference (every placed unit has the field), `name` the
    /// field's name (not one of the four standard fields and not one the symbol has, whatever the case) and `text` what it shows. It starts hidden, like a field
    /// KiCad adds (`SetVisible( false )`); show it, place it and move it as any field (`fld:<reference>:<name>`).
    AddUserField {
        id: String,
        name: String,
        #[serde(default)]
        text: String,
    },
    /// Remove a user-defined field from a symbol (`DIALOG_SYMBOL_PROPERTIES::OnDeleteField`: "The first %d fields are mandatory"): the field and the places it
    /// had on every placed unit go.
    RemoveUserField { id: String, name: String },
    /// Text Properties on a free-standing text (`DIALOG_TEXT_PROPERTIES::TransferDataFromWindow`): its text, its size (0.01 to 1000 mm, "don't
    /// allow text to disappear"), its angle (KiCad offers horizontal and vertical: any angle is made upright) and how it is justified about its anchor,
    /// `h` (left, centre, right) and `v` (top, centre, bottom) in the text's own axes.
    EditText {
        id: String,
        #[serde(default)]
        text: Option<String>,
        #[serde(default)]
        size_nm: Option<Nm>,
        #[serde(default)]
        angle: Option<Millideg>,
        #[serde(default)]
        h: Option<TextJustify>,
        #[serde(default)]
        v: Option<TextVAlign>,
    },
    /// Sheet Properties (`DIALOG_SHEET_PROPERTIES::TransferDataFromWindow`): the sheet's name and the file it shows (`SCH_EDIT_FRAME::ChangeSheetFile`:
    /// a file the project already has is linked to; a new name moves the content when this sheet is its only user and copies it when another
    /// sheet shows it too).
    EditSheet {
        id: String,
        #[serde(default)]
        name: Option<String>,
        #[serde(default)]
        file: Option<String>,
    },
    /// Wire/Bus, Line and Junction Properties (`DIALOG_WIRE_BUS_PROPERTIES::TransferDataFromWindow`, `DIALOG_LINE_PROPERTIES`, `DIALOG_JUNCTION_PROPS`):
    /// the stroke of every listed wire, bus, bus entry and graphic line (`width_nm` 0 is the default width, `style`, `color`) and the diameter and colour
    /// of every listed junction (`diameter_nm` 0 is the default). A colour with every channel 0 is `COLOR4D::UNSPECIFIED`, the layer's own colour. A
    /// field left out is unchanged; one that does not apply to an item (a width on a junction, a diameter on a wire) is skipped for that item, as in KiCad.
    SetStroke {
        ids: Vec<String>,
        #[serde(default)]
        width_nm: Option<Nm>,
        #[serde(default)]
        style: Option<SchLineStyle>,
        #[serde(default)]
        color: Option<SchColor>,
        #[serde(default)]
        diameter_nm: Option<Nm>,
    },
    /// Group Items (Ctrl+G, `SCH_GROUP_TOOL::Group`): a new group over `ids` -- symbols, wires, labels, texts, shapes, sheets, ... and other groups, which nest. The
    /// items must be on the sheet and groupable (a pin, a field and a sheet pin go along with the item that has them); at least two. Each id leaves the group it was
    /// in; when they all shared one, the new group takes their place in it. See `sch_groups.rs`.
    Group { ids: Vec<String> },
    /// Ungroup (Ctrl+Shift+G, `GROUP_TOOL::Ungroup`): every listed group dissolves and its members are free of it. A listed id that is no group is skipped; refused when none is.
    Ungroup { ids: Vec<String> },
    /// Add Items (`GROUP_TOOL::AddToGroup`): `ids` join the group `group_id`, each leaving the group it was in. A group is refused as a member of itself or of a group below it.
    AddToGroup { group_id: String, ids: Vec<String> },
    /// Remove Items (`GROUP_TOOL::RemoveFromGroup`): each id leaves the group it is in. A group left with fewer than two members dissolves. Refused when none is in a group.
    RemoveFromGroup { ids: Vec<String> },
    /// Group Properties (`DIALOG_GROUP_PROPERTIES::TransferDataFromWindow`): the group `id` is renamed and its members are exactly `member_ids`, each pulled out of the group
    /// it was in, the previous members not listed released. `design_block` is the dialog's "Library link:" box: `Some("")` clears the link, `Some(text)` sets it as
    /// `LIB_ID::Parse( text, true )` reads it (never refused), `None` leaves it.
    EditGroup {
        id: String,
        name: String,
        member_ids: Vec<String>,
        #[serde(default)]
        design_block: Option<String>,
    },
    /// What a successful Save Selection as Design Block / Update Design Block from Selection does to the sheet (`SCH_EDIT_FRAME::SaveSelectionAsDesignBlock`,
    /// `UpdateDesignBlockFromSelection`): `ids` -- the selection, a group included -- are linked to `block` (`Lib:Name`). A lone group with no link takes it ("Set Group Design
    /// Block Link"); anything else goes in a new group named after the block and linked to it ("Group Items"; it may hold one item). See `sch_groups.rs`.
    LinkDesignBlock { ids: Vec<String>, block: String },
}

impl SchCmd {
    /// The ids this verb names, for the activity log.
    pub fn ids(&self) -> Vec<&str> {
        match self {
            SchCmd::SetLocked { ids, .. } => ids.iter().map(String::as_str).collect(),
            SchCmd::AddGraphic { .. } => vec!["graphic"],
            SchCmd::UpdateLibrarySymbols { lib_ids } => lib_ids.iter().map(String::as_str).collect(),
            SchCmd::SetBodyStyle { ids, .. } => ids.iter().map(String::as_str).collect(),
            SchCmd::DeleteGraphic { id } | SchCmd::EditGraphic { id, .. } | SchCmd::DeleteSheet { id } | SchCmd::DeleteSheetPin { id } | SchCmd::EditSheetPin { id, .. } | SchCmd::ChangeSymbol { id, .. } => vec![id.as_str()],
            SchCmd::EditLabel { id, .. } | SchCmd::EditText { id, .. } | SchCmd::EditSheet { id, .. } | SchCmd::EditField { id, .. } => vec![id.as_str()],
            SchCmd::PlaceLabel { net, .. } => vec![net.as_str()],
            SchCmd::PlaceText { content, .. } => vec![content.as_str()],
            SchCmd::AutoplaceFields { ids } => ids.iter().map(String::as_str).collect(),
            SchCmd::AddUserField { id, .. } | SchCmd::RemoveUserField { id, .. } => vec![id.as_str()],
            SchCmd::PinHelper { pins, .. } => pins.iter().map(String::as_str).collect(),
            SchCmd::SetStroke { ids, .. } => ids.iter().map(String::as_str).collect(),
            SchCmd::SwapPins { pins } | SchCmd::SwapPinLabels { pins } => pins.iter().map(String::as_str).collect(),
            SchCmd::SwapUnitLabels { ids } => ids.iter().map(String::as_str).collect(),
            SchCmd::AddSheetPin { sheet, .. } => vec![sheet.as_str()],
            SchCmd::Group { ids } | SchCmd::Ungroup { ids } | SchCmd::RemoveFromGroup { ids } => ids.iter().map(String::as_str).collect(),
            SchCmd::AddToGroup { group_id, ids } => std::iter::once(group_id.as_str()).chain(ids.iter().map(String::as_str)).collect(),
            SchCmd::EditGroup { id, member_ids, .. } => std::iter::once(id.as_str()).chain(member_ids.iter().map(String::as_str)).collect(),
            SchCmd::LinkDesignBlock { ids, .. } => ids.iter().map(String::as_str).collect(),
        }
    }

    /// The one-line description the CLI journal and the activity feed show.
    pub fn describe(&self) -> String {
        match self {
            SchCmd::SetLocked { ids, locked } => format!("schematic {} {}", if *locked { "lock" } else { "unlock" }, ids.join(" ")),
            SchCmd::AddGraphic { graphic } => format!("schematic draw {}", graphic_kind_name(&graphic.shape)),
            SchCmd::DeleteGraphic { id } => format!("schematic delete-graphic {id}"),
            SchCmd::EditGraphic { id, .. } => format!("schematic edit-graphic {id}"),
            SchCmd::DeleteSheet { id } => format!("schematic delete-sheet {id}"),
            SchCmd::AddSheetPin { sheet, name, .. } => format!("schematic sheet-pin add {sheet} {name}"),
            SchCmd::DeleteSheetPin { id } => format!("schematic sheet-pin delete {id}"),
            SchCmd::EditSheetPin { id, .. } => format!("schematic sheet-pin edit {id}"),
            SchCmd::ChangeSymbol { id, lib_id } => format!("schematic change-symbol {id} {lib_id}"),
            SchCmd::UpdateLibrarySymbols { lib_ids } => format!("schematic update-symbols {}", lib_ids.join(" ")),
            SchCmd::SetBodyStyle { ids, style } => match style {
                Some(n) => format!("schematic body-style {} {n}", ids.join(" ")),
                None => format!("schematic body-style {} next", ids.join(" ")),
            },
            SchCmd::PlaceLabel { net, at, .. } => format!("schematic label {net} --at {:.2},{:.2}", eda_model::units::to_mm(at.x), eda_model::units::to_mm(at.y)),
            SchCmd::PlaceText { content, at, .. } => format!("schematic text add --content {content:?} --at {:.2},{:.2}", eda_model::units::to_mm(at.x), eda_model::units::to_mm(at.y)),
            SchCmd::EditLabel { id, .. } => format!("schematic edit-label {id}"),
            SchCmd::EditField { id, .. } => format!("schematic edit-field {id}"),
            SchCmd::AutoplaceFields { ids } => format!("schematic autoplace-fields {}", ids.join(" ")),
            SchCmd::PinHelper { what, pins, .. } => format!("schematic pin-helper {} {}", serde_json::to_value(what).ok().and_then(|v| v.as_str().map(String::from)).unwrap_or_default(), pins.join(" ")),
            SchCmd::AddUserField { id, name, .. } => format!("schematic field-add {id} {name:?}"),
            SchCmd::RemoveUserField { id, name } => format!("schematic field-remove {id} {name:?}"),
            SchCmd::EditText { id, .. } => format!("schematic edit-text {id}"),
            SchCmd::EditSheet { id, .. } => format!("schematic edit-sheet {id}"),
            SchCmd::SetStroke { ids, .. } => format!("schematic stroke {}", ids.join(" ")),
            SchCmd::SwapPins { pins } => format!("schematic swap-pins {}", pins.join(" ")),
            SchCmd::SwapPinLabels { pins } => format!("schematic swap-pin-labels {}", pins.join(" ")),
            SchCmd::SwapUnitLabels { ids } => format!("schematic swap-unit-labels {}", ids.join(" ")),
            SchCmd::Group { ids } => format!("schematic group {}", ids.join(" ")),
            SchCmd::Ungroup { ids } => format!("schematic ungroup {}", ids.join(" ")),
            SchCmd::AddToGroup { group_id, ids } => format!("schematic group-add {group_id} {}", ids.join(" ")),
            SchCmd::RemoveFromGroup { ids } => format!("schematic group-remove {}", ids.join(" ")),
            SchCmd::EditGroup { id, name, .. } => format!("schematic group-properties {id} {name:?}"),
            SchCmd::LinkDesignBlock { ids, block } => format!("schematic link-design-block {block} {}", ids.join(" ")),
        }
    }

    /// The activity kind this verb files under (`crates/cli/src/board.rs`).
    pub fn kind(&self) -> &'static str {
        match self {
            SchCmd::SetLocked { .. } => "schematic-lock",
            SchCmd::AddGraphic { .. } | SchCmd::DeleteGraphic { .. } | SchCmd::EditGraphic { .. } => "schematic-graphic",
            SchCmd::DeleteSheet { .. } | SchCmd::AddSheetPin { .. } | SchCmd::DeleteSheetPin { .. } | SchCmd::EditSheetPin { .. } => "schematic-sheet",
            SchCmd::ChangeSymbol { .. } | SchCmd::UpdateLibrarySymbols { .. } | SchCmd::SetBodyStyle { .. } => "schematic-symbol",
            SchCmd::EditLabel { .. } | SchCmd::EditText { .. } | SchCmd::PlaceText { .. } => "schematic-text",
            SchCmd::PlaceLabel { .. } => "schematic-label",
            SchCmd::EditField { .. } | SchCmd::AutoplaceFields { .. } | SchCmd::AddUserField { .. } | SchCmd::RemoveUserField { .. } => "schematic-fields",
            SchCmd::EditSheet { .. } => "schematic-sheet",
            SchCmd::SetStroke { .. } => "schematic-stroke",
            SchCmd::SwapPins { .. } | SchCmd::SwapPinLabels { .. } | SchCmd::SwapUnitLabels { .. } | SchCmd::PinHelper { .. } => "schematic-pins",
            SchCmd::Group { .. } | SchCmd::Ungroup { .. } | SchCmd::AddToGroup { .. } | SchCmd::RemoveFromGroup { .. } | SchCmd::EditGroup { .. } | SchCmd::LinkDesignBlock { .. } => "schematic-group",
        }
    }

    /// Can this verb change what is connected? A stroke or a free text never does; a label's text is its net's name and a sheet's name is part
    /// of the names of the nets drawn inside it, so those are read from the drawing like any other edit (`Cmd::edits_connectivity`).
    pub fn edits_connectivity(&self) -> bool {
        match self {
            SchCmd::SetStroke { .. } | SchCmd::EditText { .. } | SchCmd::PlaceText { .. } | SchCmd::AutoplaceFields { .. } | SchCmd::AddUserField { .. } | SchCmd::RemoveUserField { .. } => false,
            // a group is a set of names; it moves, joins and leaves nothing
            SchCmd::Group { .. } | SchCmd::Ungroup { .. } | SchCmd::AddToGroup { .. } | SchCmd::RemoveFromGroup { .. } | SchCmd::EditGroup { .. } | SchCmd::LinkDesignBlock { .. } => false,
            // a field's text is a reference, a net or a sheet's name; its place and look are layout
            SchCmd::EditField { text, .. } => text.is_some(),
            _ => true,
        }
    }
}

impl<'a> Board<'a> {
    /// Apply one [`SchCmd`] to the schematic.
    pub(crate) fn apply_sch_edit(&mut self, cmd: &SchCmd) -> Result<(), Vec<CheckResult>> {
        match cmd {
            SchCmd::SetLocked { ids, locked } => set_locked(self.schematic_mut()?, ids, *locked),
            SchCmd::AddGraphic { graphic } => add_graphic(self.schematic_mut_or_create(), graphic.clone()),
            SchCmd::DeleteGraphic { id } => delete_graphic(self.schematic_mut()?, id),
            SchCmd::EditGraphic { id, graphic } => edit_graphic(self.schematic_mut()?, id, graphic.clone()),
            SchCmd::DeleteSheet { id } => {
                let pins: Vec<Point> = self.schematic_mut()?.sheets.iter().filter(|s| &s.id == id).flat_map(|s| s.pins.iter().map(|p| p.at)).collect();
                delete_sheet(self.schematic_mut()?, id)?;
                // `DoDelete`: `for( SCH_SHEET_PIN* pin : sheet->GetPins() ) pts.insert( pin->GetPosition() )`, then `DeleteJunction` where the junction is no
                // longer explicit, which merges the wires in line there whatever their strokes (r5 B5-L1)
                for at in pins {
                    self.delete_junction_at(at, false)?;
                }
                // the references its placement gave a screen another sheet still shows go with it (`PruneOrphanedSymbolInstances`)
                eda_model::sheet_list::prune_orphaned_overrides(&mut self.design);
                // `DoDelete` puts the sheet's pins' points in `pts` and drops a junction there that joins nothing any more; the commit cleans up (r4 C4-L1)
                self.commit_clean_up()
            }
            SchCmd::AddSheetPin { sheet, name, shape, at } => add_sheet_pin(self.schematic_mut()?, sheet, name, *shape, *at),
            SchCmd::DeleteSheetPin { id } => {
                let at = self.schematic_mut()?.sheets.iter().flat_map(|s| s.pins.iter()).find(|p| &p.id == id).map(|p| p.at);
                delete_sheet_pin(self.schematic_mut()?, id)?;
                // `DoDelete` puts the pin's position in `pts` and runs `DeleteJunction` there when the junction is no longer explicit (r5 B5-L1)
                if let Some(at) = at {
                    self.delete_junction_at(at, false)?;
                }
                self.commit_clean_up()
            }
            SchCmd::EditSheetPin { id, name, shape, at, size_nm, bold, italic } => edit_sheet_pin(self.schematic_mut()?, id, name.as_deref(), *shape, *at, SheetPinLookEdit { size_nm: *size_nm, bold: *bold, italic: *italic }),
            SchCmd::ChangeSymbol { id, lib_id } => self.change_symbol(id, lib_id),
            SchCmd::UpdateLibrarySymbols { lib_ids } => self.update_library_symbols(lib_ids),
            SchCmd::SetBodyStyle { ids, style } => self.set_body_style(ids, *style),
            SchCmd::EditLabel { id, text, shape, spin, size_nm, bold, italic, auto_rotate, fields } => {
                self.edit_sch_label(id, text.as_deref(), *shape, *spin, crate::sch_props::LabelLookEdit { size_nm: *size_nm, bold: *bold, italic: *italic, auto_rotate: *auto_rotate, fields: fields.clone() })
            }
            SchCmd::PlaceLabel { net, at, kind, spin, size_nm, bold, italic, auto_rotate } => {
                // a label is connectable: its commit runs the clean-up (`SCH_COMMIT::pushSchEdit`, `LOCAL_CLEANUP`; r4 C4-L1)
                self.place_label(net, *at, kind, &crate::sch_labels::PlacedLabel { spin: *spin, size_nm: *size_nm, bold: *bold, italic: *italic, auto_rotate: *auto_rotate }).and_then(|_| self.commit_clean_up())
            }
            SchCmd::PlaceText { content, at, angle, size_nm, h, v } => self.place_text(content, *at, *angle, *size_nm, *h, *v),
            SchCmd::EditField { id, text, at, vertical, h, v, size_nm, bold, italic, visible, name_shown, allow_autoplace } => self.edit_sch_field(
                id,
                &crate::sch_fields::FieldEdit {
                    text: text.clone(),
                    at: *at,
                    vertical: *vertical,
                    h: *h,
                    v: *v,
                    size_nm: *size_nm,
                    bold: *bold,
                    italic: *italic,
                    visible: *visible,
                    name_shown: *name_shown,
                    allow_autoplace: *allow_autoplace,
                },
            ),
            SchCmd::AutoplaceFields { ids } => self.autoplace_sch_fields(ids),
            SchCmd::PinHelper { what, pins, grid } => self.pin_helper(*what, pins, *grid),
            SchCmd::AddUserField { id, name, text } => self.add_user_field(id, name, text),
            SchCmd::RemoveUserField { id, name } => self.remove_user_field(id, name),
            SchCmd::EditText { id, text, size_nm, angle, h, v } => self.edit_sch_text(id, text.as_deref(), *size_nm, *angle, *h, *v),
            SchCmd::EditSheet { id, name, file } => self.edit_sch_sheet(id, name.as_deref(), file.as_deref()),
            SchCmd::SetStroke { ids, width_nm, style, color, diameter_nm } => self.set_sch_stroke(ids, *width_nm, *style, *color, *diameter_nm),
            SchCmd::SwapPins { pins } => self.swap_pins(pins),
            SchCmd::SwapPinLabels { pins } => self.swap_pin_labels(pins),
            SchCmd::SwapUnitLabels { ids } => self.swap_unit_labels(ids),
            SchCmd::Group { ids } => self.sch_group_items(ids),
            SchCmd::Ungroup { ids } => self.sch_ungroup_items(ids),
            SchCmd::AddToGroup { group_id, ids } => self.sch_add_to_group(group_id, ids),
            SchCmd::RemoveFromGroup { ids } => self.sch_remove_from_group(ids),
            SchCmd::EditGroup { id, name, member_ids, design_block } => self.sch_edit_group(id, name, member_ids, design_block.as_deref()),
            SchCmd::LinkDesignBlock { ids, block } => self.sch_link_design_block(ids, block),
        }
    }

    /// [`SchCmd::SetBodyStyle`]: `SCH_EDIT_FRAME::SelectBodyStyle` on each placed unit of `ids`.
    fn set_body_style(&mut self, ids: &[String], style: Option<u32>) -> Result<(), Vec<CheckResult>> {
        let sch = self.schematic()?;
        if let Some(missing) = ids.iter().find(|id| !sch.symbols.iter().any(|s| &s.id == *id)) {
            return Err(vec![CheckResult::fail("ops_unknown_symbol", missing, "no symbol with this reference on the sheet")]);
        }
        let mut changes: Vec<(String, u32)> = Vec::new();
        for sym in sch.symbols.iter().filter(|s| ids.contains(&s.id)) {
            // `bodyStyleCount <= 1`: nothing to select between (`LIB_SYMBOL::GetBodyStyleCount`)
            let count = self.design_symbol(&sym.lib_id).map_or(1, |l| l.body_style_count());
            let current = sch.body_style_of(sym);
            if count <= 1 {
                continue;
            }
            // `CycleBodyStyle`: "nextBodyStyle = GetBodyStyle() + 1; if( nextBodyStyle > GetBodyStyleCount() ) nextBodyStyle = 1"; `SelectBodyStyle`: clamped
            let next = match style {
                None => if current + 1 > count { 1 } else { current + 1 },
                Some(n) => n.clamp(1, count),
            };
            if next != current {
                changes.push((eda_model::ir::field_key(&sym.id, sym.unit), next));
            }
        }
        if changes.is_empty() {
            return Err(vec![CheckResult::fail("ops_no_body_style", ids.first().map(String::as_str).unwrap_or("symbol"), "the symbol has no other body style to change to")]);
        }
        let sch = self.schematic_mut()?;
        for (key, next) in changes {
            if next <= 1 {
                sch.extras.body_styles.remove(&key);
            } else {
                sch.extras.body_styles.insert(key, next);
            }
        }
        Ok(())
    }

    fn update_library_symbols(&mut self, lib_ids: &[String]) -> Result<(), Vec<CheckResult>> {
        let mut updated = 0usize;
        if let Some(lib) = self.design.symbol_library.as_mut() {
            for sym in lib.symbols.iter_mut().filter(|s| lib_ids.contains(&s.lib_id) && !s.published) {
                sym.published = true;
                updated += 1;
            }
        }
        if updated == 0 {
            return Err(vec![CheckResult::fail("ops_nothing_to_update", lib_ids.first().map(String::as_str).unwrap_or("symbols"), "no edited library symbol is waiting to be updated from")]);
        }
        Ok(())
    }

    /// How many units the library symbol `lib_id` has: the project's own (edited) symbol first, then the libraries and builtin table
    /// `ConstraintModel::symbol_of` resolves. `None` when no library symbol has that id.
    fn library_unit_count(&self, lib_id: &str) -> Option<u32> {
        if let Some(sym) = self.design.symbol_library.as_ref().and_then(|l| l.by_lib_id(lib_id)) {
            return Some(sym.unit_count.max(1));
        }
        self.model.symbol_of(lib_id).map(|s| s.unit_count.max(1))
    }

    fn change_symbol(&mut self, id: &str, lib_id: &str) -> Result<(), Vec<CheckResult>> {
        let lib_id = lib_id.trim();
        if lib_id.is_empty() {
            return Err(vec![CheckResult::fail("ops_bad_symbol", id, "a symbol needs a library id")]);
        }
        let Some(unit_count) = self.library_unit_count(lib_id) else {
            return Err(vec![CheckResult::fail("ops_unknown_lib_symbol", lib_id, "symbol not found")]);
        };
        let sch = self.schematic_mut()?;
        let placed: Vec<usize> = sch.symbols.iter().enumerate().filter(|(_, s)| s.id == id).map(|(i, _)| i).collect();
        if placed.is_empty() {
            return Err(vec![CheckResult::fail("ops_unknown_symbol", id, "no symbol with this reference on the sheet")]);
        }
        if let Some(&i) = placed.iter().find(|&&i| sch.symbols[i].unit > unit_count) {
            return Err(vec![CheckResult::fail("ops_too_few_units", id, format!("new symbol has too few units: unit {} is placed but {lib_id} has {unit_count}", sch.symbols[i].unit))]);
        }
        if placed.iter().all(|&i| sch.symbols[i].lib_id == lib_id) {
            return Err(vec![CheckResult::fail("ops_symbol_unchanged", id, format!("the symbol already uses {lib_id}"))]);
        }
        for i in placed {
            sch.symbols[i].lib_id = lib_id.to_string();
        }
        Ok(())
    }
}

impl Cmd {
    /// A `Cmd` for one [`SchCmd`] (the wrapper callers and tests build).
    pub fn sch(cmd: SchCmd) -> Cmd {
        Cmd::SchEdit(cmd)
    }
}

fn graphic_kind_name(s: &SchGraphicKind) -> &'static str {
    match s {
        SchGraphicKind::Rectangle { .. } => "rectangle",
        SchGraphicKind::Circle { .. } => "circle",
        SchGraphicKind::Arc { .. } => "arc",
        SchGraphicKind::Bezier { .. } => "bezier",
        SchGraphicKind::Polygon { .. } => "polygon",
        SchGraphicKind::TextBox { .. } => "text-box",
        SchGraphicKind::RuleArea { .. } => "rule-area",
        SchGraphicKind::Directive { .. } => "directive-label",
        SchGraphicKind::Image { .. } => "image",
    }
}

/// Is `a`, `b`, `c` a real arc (three non-collinear points)?
fn non_collinear(a: Point, b: Point, c: Point) -> bool {
    let cross = (b.x - a.x).area(c.y - a.y) - (b.y - a.y).area(c.x - a.x);
    cross != 0
}

/// Refuse a graphic that could never be drawn: the shape-specific sanity `EndEdit` implies.
fn check_graphic(g: &SchGraphic) -> Result<(), Vec<CheckResult>> {
    let bad = |what: &str| Err(vec![CheckResult::fail("ops_bad_graphic", graphic_kind_name(&g.shape), what.to_string())]);
    if g.width_nm < Nm::ZERO {
        return bad("a stroke width cannot be negative");
    }
    match &g.shape {
        SchGraphicKind::Rectangle { start, end, corner_radius_nm } => {
            if start.x == end.x || start.y == end.y {
                return bad("a rectangle needs two corners that differ in both x and y");
            }
            if *corner_radius_nm < Nm::ZERO {
                return bad("a corner radius cannot be negative");
            }
        }
        SchGraphicKind::Circle { radius_nm, .. } => {
            if *radius_nm <= Nm::ZERO {
                return bad("a circle needs a positive radius");
            }
        }
        SchGraphicKind::Arc { start, mid, end } => {
            if !non_collinear(*start, *mid, *end) {
                return bad("an arc needs three points that are not in a line");
            }
        }
        SchGraphicKind::Bezier { start, c1, c2, end } => {
            if start == end && start == c1 && start == c2 {
                return bad("a bezier needs at least two different points");
            }
        }
        SchGraphicKind::Polygon { pts } | SchGraphicKind::RuleArea { pts, .. } => {
            let mut distinct = pts.clone();
            distinct.sort();
            distinct.dedup();
            if distinct.len() < 3 {
                return bad("a polygon needs at least three different points");
            }
        }
        SchGraphicKind::TextBox { start, end, size_nm, .. } => {
            if start.x == end.x || start.y == end.y {
                return bad("a text box needs a width and a height");
            }
            if *size_nm <= Nm::ZERO {
                return bad("a text box needs a positive text size");
            }
        }
        SchGraphicKind::Directive { pin_length_nm, .. } => {
            if *pin_length_nm < Nm::ZERO {
                return bad("a directive label's pole cannot have a negative length");
            }
        }
        SchGraphicKind::Image { scale_ppm, data, .. } => {
            if *scale_ppm <= 0 {
                return bad("an image's scale must be a positive number");
            }
            if eda_model::image_info::header_of_base64(data).is_none() {
                return bad("an image needs the data of a PNG, JPEG, GIF or BMP file");
            }
        }
    }
    Ok(())
}

/// An image's data without the white space a pasted or typed text may have.
fn compact_image(graphic: &mut SchGraphic) {
    if let SchGraphicKind::Image { data, .. } = &mut graphic.shape {
        *data = eda_model::image_info::compact(data);
    }
}

fn add_graphic(sch: &mut SchematicSection, mut graphic: SchGraphic) -> Result<(), Vec<CheckResult>> {
    compact_image(&mut graphic);
    check_graphic(&graphic)?;
    graphic.id = String::new();
    sch.extras.graphics.push(graphic);
    sch.assign_missing_ids();
    Ok(())
}

fn delete_graphic(sch: &mut SchematicSection, id: &str) -> Result<(), Vec<CheckResult>> {
    let before = sch.extras.graphics.len();
    sch.extras.graphics.retain(|g| g.id != id);
    if sch.extras.graphics.len() == before {
        return Err(vec![CheckResult::fail("ops_unknown_graphic", id, "no drawn graphic with this id")]);
    }
    sch.extras.set_locked(id, false);
    Ok(())
}

fn edit_graphic(sch: &mut SchematicSection, id: &str, mut graphic: SchGraphic) -> Result<(), Vec<CheckResult>> {
    let Some(i) = sch.extras.graphics.iter().position(|g| g.id == id) else {
        return Err(vec![CheckResult::fail("ops_unknown_graphic", id, "no drawn graphic with this id")]);
    };
    // The browser does not hold an image's pixels: an edit that sends none keeps the ones the image has.
    if let (SchGraphicKind::Image { data, .. }, SchGraphicKind::Image { data: old, .. }) = (&mut graphic.shape, &sch.extras.graphics[i].shape) {
        if data.is_empty() {
            *data = old.clone();
        }
    }
    compact_image(&mut graphic);
    check_graphic(&graphic)?;
    graphic.id = id.to_string();
    if sch.extras.graphics[i] == graphic {
        return Err(vec![CheckResult::fail("ops_graphic_unchanged", id, "the graphic already has these properties")]);
    }
    sch.extras.graphics[i] = graphic;
    Ok(())
}

fn delete_sheet(sch: &mut SchematicSection, id: &str) -> Result<(), Vec<CheckResult>> {
    let before = sch.sheets.len();
    let pins: Vec<String> = sch.sheets.iter().filter(|s| s.id == id).flat_map(|s| s.pins.iter().map(|p| p.id.clone())).collect();
    sch.sheets.retain(|s| s.id != id);
    if sch.sheets.len() == before {
        return Err(vec![CheckResult::fail("ops_unknown_sheet", id, "no sheet with this id")]);
    }
    sch.extras.set_locked(id, false);
    for pin in pins {
        sch.extras.sheet_pin_looks.remove(&pin);
    }
    // the sheet's own fields and pages go with it (`SCH_SHEET::m_fields`, `m_instances`): a sheet drawn later under the same id must not take them (r3 B3-L3)
    sch.extras.sheet_fields.remove(id);
    sch.extras.sheet_pages.remove(id);
    Ok(())
}

/// Is `at` on the border of the sheet rectangle (`SCH_SHEET_PIN` is always `ConstrainOnEdge`-clamped there)?
fn on_sheet_border(sheet: &SheetInstance, at: Point) -> bool {
    let (w, h) = sheet.size;
    let (left, top, right, bottom) = (sheet.at.x, sheet.at.y, sheet.at.x + w, sheet.at.y + h);
    let in_x = (left..=right).contains(&at.x);
    let in_y = (top..=bottom).contains(&at.y);
    (in_y && (at.x == left || at.x == right)) || (in_x && (at.y == top || at.y == bottom))
}

fn add_sheet_pin(sch: &mut SchematicSection, sheet: &str, name: &str, shape: LabelShape, at: Point) -> Result<(), Vec<CheckResult>> {
    if name.trim().is_empty() {
        return Err(vec![CheckResult::fail("ops_bad_sheet_pin", sheet, "a sheet pin needs a name")]);
    }
    let Some(s) = sch.sheets.iter_mut().find(|s| s.id == sheet) else {
        return Err(vec![CheckResult::fail("ops_unknown_sheet", sheet, "no sheet with this id")]);
    };
    if !on_sheet_border(s, at) {
        return Err(vec![CheckResult::fail("ops_sheet_pin_off_border", sheet, "a sheet pin sits on the border of its sheet")]);
    }
    s.pins.push(SheetPin { id: String::new(), name: name.to_string(), shape, at });
    sch.assign_missing_ids();
    Ok(())
}

fn find_sheet_pin<'a>(sch: &'a mut SchematicSection, id: &str) -> Result<(&'a mut SheetInstance, usize), Vec<CheckResult>> {
    for s in sch.sheets.iter_mut() {
        if let Some(i) = s.pins.iter().position(|p| p.id == id) {
            return Ok((s, i));
        }
    }
    Err(vec![CheckResult::fail("ops_unknown_sheet_pin", id, "no sheet pin with this id")])
}

fn delete_sheet_pin(sch: &mut SchematicSection, id: &str) -> Result<(), Vec<CheckResult>> {
    let (s, i) = find_sheet_pin(sch, id)?;
    s.pins.remove(i);
    sch.extras.sheet_pin_looks.remove(id);
    Ok(())
}

/// What Sheet Pin Properties sets of a pin's text (`EDA_TEXT`); a member left out is unchanged.
#[derive(Debug, Clone, Copy, Default)]
struct SheetPinLookEdit {
    size_nm: Option<Nm>,
    bold: Option<bool>,
    italic: Option<bool>,
}

fn edit_sheet_pin(sch: &mut SchematicSection, id: &str, name: Option<&str>, shape: Option<LabelShape>, at: Option<Point>, look: SheetPinLookEdit) -> Result<(), Vec<CheckResult>> {
    if name.is_some_and(|n| n.trim().is_empty()) {
        return Err(vec![CheckResult::fail("ops_bad_sheet_pin", id, "a sheet pin needs a name")]);
    }
    // `DIALOG_SHEET_PIN_PROPERTIES::TransferDataFromWindow` validates no size: `EDA_TEXT::SetTextSize` clamps it to `TEXT_MIN_SIZE_MM` .. `TEXT_MAX_SIZE_MM`
    // (0.001 .. 250 mm)
    let look = SheetPinLookEdit { size_nm: look.size_nm.map(|size| size.clamp(Nm::new(1_000), Nm::new(250_000_000))), ..look };
    let (s, i) = find_sheet_pin(sch, id)?;
    if let Some(p) = at {
        if !on_sheet_border(s, p) {
            return Err(vec![CheckResult::fail("ops_sheet_pin_off_border", id, "a sheet pin sits on the border of its sheet")]);
        }
    }
    let pin = &mut s.pins[i];
    let before = pin.clone();
    if let Some(n) = name {
        pin.name = n.to_string();
    }
    if let Some(sh) = shape {
        pin.shape = sh;
    }
    if let Some(p) = at {
        pin.at = p;
    }
    let renamed_or_moved = pin.name != before.name || pin.shape != before.shape || pin.at != before.at;
    // the look: the default one (50 mil, regular, upright) is no entry
    let was = sch.extras.sheet_pin_looks.get(id).copied().unwrap_or_default();
    let mut now = was;
    if let Some(size) = look.size_nm {
        now.size_nm = if size == eda_model::ir::DEFAULT_FIELD_SIZE_NM { Nm::ZERO } else { size };
    }
    if let Some(b) = look.bold {
        now.bold = b;
    }
    if let Some(i) = look.italic {
        now.italic = i;
    }
    if now.is_default() {
        sch.extras.sheet_pin_looks.remove(id);
    } else {
        sch.extras.sheet_pin_looks.insert(id.to_string(), now);
    }
    if !renamed_or_moved && now == was {
        return Err(vec![CheckResult::fail("ops_sheet_pin_unchanged", id, "the pin already has these properties")]);
    }
    // The pin keeps its id: it is the same item after its Properties (KiCad keeps the item and its selection), and a pin moved by `sch_move` keeps it too.
    Ok(())
}

/// Every item id of this sheet that Lock can act on (everything but pins and fields; sheet pins are listed
/// separately because they are accepted-and-ignored).
pub(crate) fn lockable_ids(sch: &SchematicSection) -> BTreeSet<&str> {
    let mut out: BTreeSet<&str> = BTreeSet::new();
    // a group is locked as an item is (`SCH_ITEM::SetLocked` on the `SCH_GROUP`): what is below it is locked with it
    out.extend(sch.extras.groups.iter().map(|g| g.id.as_str()));
    out.extend(sch.symbols.iter().map(|s| s.id.as_str()));
    out.extend(sch.power_symbols.iter().map(|p| p.id.as_str()));
    out.extend(sch.wires.iter().map(|w| w.id.as_str()));
    out.extend(sch.labels.iter().map(|l| l.id.as_str()));
    out.extend(sch.texts.iter().map(|t| t.id.as_str()));
    out.extend(sch.no_connects.iter().map(|n| n.id.as_str()));
    out.extend(sch.bus_entries.iter().map(|b| b.id.as_str()));
    out.extend(sch.junctions.iter().map(|j| j.id.as_str()));
    out.extend(sch.lines.iter().map(|l| l.id.as_str()));
    out.extend(sch.sheets.iter().map(|s| s.id.as_str()));
    out.extend(sch.extras.graphics.iter().map(|g| g.id.as_str()));
    out.extend(sch.extras.tables.iter().map(|t| t.id.as_str()));
    out.remove("");
    out
}

fn set_locked(sch: &mut SchematicSection, ids: &[String], locked: bool) -> Result<(), Vec<CheckResult>> {
    if ids.is_empty() {
        return Err(vec![CheckResult::fail("ops_bad_lock", "lock", "no items given")]);
    }
    let known = lockable_ids(sch);
    let sheet_pins: BTreeSet<&str> = sch.sheets.iter().flat_map(|s| s.pins.iter()).map(|p| p.id.as_str()).collect();
    let mut targets: Vec<String> = Vec::new();
    for id in ids {
        if known.contains(id.as_str()) {
            targets.push(id.clone());
        } else if !sheet_pins.contains(id.as_str()) && !crate::sch_fields::is_field_id(id) {
            // (`modifyLockSelected` skips pins, fields and sheet pins: "they inherit from parent")
            return Err(vec![CheckResult::fail("ops_unknown_item", id, "no symbol, wire, label, text, junction, line, sheet or graphic with this id to lock")]);
        }
    }
    let mut changed = false;
    for id in &targets {
        changed |= sch.extras.set_locked(id, locked);
    }
    if !changed && !targets.is_empty() {
        // Nothing to do (already in that state): refuse, so the undo stack never records a step that changed nothing.
        return Err(vec![CheckResult::fail("ops_lock_unchanged", &targets[0], format!("every listed item is already {}", if locked { "locked" } else { "unlocked" }))]);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use eda_model::units::{mm, um};
    use super::*;
    use eda_model::ir::{Design, Provenance, SchematicText, Wire};
    use eda_model::ir::{LabelShape, SheetInstance};
    use eda_model::sch_extras::SchFill;
    use eda_model::ConstraintModel;

    pub(super) fn design_with_schematic(sch: SchematicSection) -> Design {
        Design {
            schema: eda_model::ir::SCHEMA_VERSION,
            provenance: Provenance { engine_version: "0".into(), intent_hash: "x".into(), seed: 0, stage_hashes: vec![] },
            schematic: Some(sch),
            nets: None,
            placement: None,
            routing: None,
            drawings: None,
            footprint_library: None,
            sheet_contents: None,
            bus_aliases: vec![],
            symbol_library: None,
        }
    }

    pub(super) fn empty_sheet() -> SchematicSection {
        crate::empty_schematic_section()
    }

    pub(super) fn p(x: Nm, y: Nm) -> Point {
        Point { x, y }
    }

    #[test]
    fn lock_unlock_round_trips_and_refuses_no_ops() {
        let mut sch = empty_sheet();
        sch.wires.push(Wire { id: "wire_a".into(), net: "N".into(), pins: vec![], pts: vec![p(Nm::ZERO, Nm::ZERO), p(mm(1.0), Nm::ZERO)], bus: false });
        sch.texts.push(SchematicText { id: "txt_a".into(), content: "hi".into(), at: p(Nm::ZERO, Nm::ZERO), angle: 0, size_nm: mm(1.27) });
        let model = ConstraintModel::default();
        let mut b = Board::new(design_with_schematic(sch), &model, mm(0.1), mm(0.3));
        assert_eq!(Cmd::sch(SchCmd::SetLocked { ids: vec!["wire_a".into()], locked: true }).domain(), crate::Domain::Schematic);
        b.apply(&Cmd::sch(SchCmd::SetLocked { ids: vec!["wire_a".into(), "txt_a".into()], locked: true })).unwrap();
        assert_eq!(b.design().schematic.as_ref().unwrap().extras.locked, vec!["txt_a".to_string(), "wire_a".to_string()]);
        // Locking again changes nothing: refused.
        let err = b.apply(&Cmd::sch(SchCmd::SetLocked { ids: vec!["wire_a".into()], locked: true })).unwrap_err();
        assert_eq!(err[0].check, "ops_lock_unchanged");
        // One of two already locked still counts as a change.
        b.apply(&Cmd::sch(SchCmd::SetLocked { ids: vec!["wire_a".into()], locked: false })).unwrap();
        b.apply(&Cmd::sch(SchCmd::SetLocked { ids: vec!["wire_a".into(), "txt_a".into()], locked: true })).unwrap();
        b.apply(&Cmd::sch(SchCmd::SetLocked { ids: vec!["wire_a".into(), "txt_a".into()], locked: false })).unwrap();
        assert!(b.design().schematic.as_ref().unwrap().extras.locked.is_empty());
    }

    #[test]
    fn lock_refuses_unknown_and_empty() {
        let model = ConstraintModel::default();
        let mut b = Board::new(design_with_schematic(empty_sheet()), &model, mm(0.1), mm(0.3));
        assert_eq!(b.apply(&Cmd::sch(SchCmd::SetLocked { ids: vec![], locked: true })).unwrap_err()[0].check, "ops_bad_lock");
        assert_eq!(b.apply(&Cmd::sch(SchCmd::SetLocked { ids: vec!["nope".into()], locked: true })).unwrap_err()[0].check, "ops_unknown_item");
    }

    fn rectangle(x0: Nm, y0: Nm, x1: Nm, y1: Nm) -> SchGraphic {
        SchGraphic::new(SchGraphicKind::Rectangle { start: p(x0, y0), end: p(x1, y1), corner_radius_nm: Nm::ZERO })
    }

    #[test]
    fn graphics_are_added_edited_locked_and_deleted() {
        let model = ConstraintModel::default();
        let mut b = Board::new(design_with_schematic(empty_sheet()), &model, mm(0.1), mm(0.3));
        b.apply(&Cmd::sch(SchCmd::AddGraphic { graphic: rectangle(Nm::ZERO, Nm::ZERO, mm(10.0), mm(5.0)) })).unwrap();
        b.apply(&Cmd::sch(SchCmd::AddGraphic { graphic: SchGraphic::new(SchGraphicKind::Circle { center: p(Nm::new(1), Nm::new(1)), radius_nm: mm(3.0) }) })).unwrap();
        let ids: Vec<String> = b.design().schematic.as_ref().unwrap().extras.graphics.iter().map(|g| g.id.clone()).collect();
        assert_eq!(ids.len(), 2);
        assert!(ids[0].starts_with("shp_") && ids[0] != ids[1]);

        b.apply(&Cmd::sch(SchCmd::SetLocked { ids: vec![ids[0].clone()], locked: true })).unwrap();
        let mut edited = rectangle(Nm::ZERO, Nm::ZERO, mm(20.0), mm(5.0));
        edited.fill = SchFill::Background;
        b.apply(&Cmd::sch(SchCmd::EditGraphic { id: ids[0].clone(), graphic: edited.clone() })).unwrap();
        let sch = b.design().schematic.as_ref().unwrap();
        assert_eq!(sch.extras.graphics[0].id, ids[0], "an edit keeps the id");
        assert_eq!(sch.extras.graphics[0].fill, SchFill::Background);
        assert!(sch.extras.is_locked(&ids[0]), "and the lock");
        assert_eq!(b.apply(&Cmd::sch(SchCmd::EditGraphic { id: ids[0].clone(), graphic: edited })).unwrap_err()[0].check, "ops_graphic_unchanged");

        b.apply(&Cmd::sch(SchCmd::DeleteGraphic { id: ids[0].clone() })).unwrap();
        let sch = b.design().schematic.as_ref().unwrap();
        assert_eq!(sch.extras.graphics.len(), 1);
        assert!(!sch.extras.is_locked(&ids[0]), "a deleted item's lock goes with it");
        assert_eq!(b.apply(&Cmd::sch(SchCmd::DeleteGraphic { id: ids[0].clone() })).unwrap_err()[0].check, "ops_unknown_graphic");
    }

    fn image(x: Nm, y: Nm, scale_ppm: i64, rgb: [u8; 3]) -> SchGraphic {
        SchGraphic::new(SchGraphicKind::Image { at: p(x, y), scale_ppm, data: eda_model::image_info::base64_encode(&eda_model::image_info::solid_png(60, 40, rgb, 254)) })
    }

    /// `SCH_DRAWING_TOOLS::PlaceImage` / `DIALOG_IMAGE_PROPERTIES`: an image is a graphic with the pixels of a file; an edit that sends no pixels keeps them, a size is the
    /// pixels at their definition times the scale, and a file that is no image is refused.
    #[test]
    fn an_image_is_added_edited_with_or_without_its_pixels_moved_and_refused_when_it_is_no_image() {
        let model = ConstraintModel::default();
        let mut b = Board::new(design_with_schematic(empty_sheet()), &model, mm(0.1), mm(0.3));
        let first = image(mm(10.0), mm(20.0), 1_000_000, [1, 2, 3]);
        let data = if let SchGraphicKind::Image { data, .. } = &first.shape { data.clone() } else { unreachable!() };
        b.apply(&Cmd::sch(SchCmd::AddGraphic { graphic: first })).unwrap();
        let id = b.design().schematic.as_ref().unwrap().extras.graphics[0].id.clone();
        assert!(id.starts_with("img_"), "{id}");
        let graphics = |b: &Board| b.design().schematic.as_ref().unwrap().extras.graphics.clone();
        assert_eq!(crate::sch_scene::graphic_bounds_for_test(&graphics(&b)[0]), Some((p(mm(7.0), mm(18.0)), p(mm(13.0), mm(22.0)))), "60 x 40 px at 254 ppi are 6 mm x 4 mm around the centre");

        // Image Properties: the position and the scale; the browser sends no pixels
        b.apply(&Cmd::sch(SchCmd::EditGraphic { id: id.clone(), graphic: SchGraphic::new(SchGraphicKind::Image { at: p(Nm::ZERO, Nm::ZERO), scale_ppm: 2_000_000, data: String::new() }) })).unwrap();
        match &graphics(&b)[0].shape {
            SchGraphicKind::Image { at, scale_ppm, data: kept } => assert_eq!((*at, *scale_ppm, kept.as_str()), (p(Nm::ZERO, Nm::ZERO), 2_000_000, data.as_str()), "the pixels stay"),
            other => panic!("{other:?}"),
        }
        // new pixels replace them
        b.apply(&Cmd::sch(SchCmd::EditGraphic { id: id.clone(), graphic: image(Nm::ZERO, Nm::ZERO, 2_000_000, [9, 9, 9]) })).unwrap();
        assert!(matches!(&graphics(&b)[0].shape, SchGraphicKind::Image { data: d, .. } if *d != data));
        b.apply(&Cmd::sch(SchCmd::SetLocked { ids: vec![id.clone()], locked: true })).unwrap();
        assert!(b.design().schematic.as_ref().unwrap().extras.is_locked(&id), "an image locks like any graphic");

        let refuse = |b: &mut Board, g: SchGraphic| assert_eq!(b.apply(&Cmd::sch(SchCmd::AddGraphic { graphic: g })).unwrap_err()[0].check, "ops_bad_graphic");
        refuse(&mut b, image(Nm::ZERO, Nm::ZERO, 0, [1, 1, 1]));
        refuse(&mut b, image(Nm::ZERO, Nm::ZERO, -1, [1, 1, 1]));
        refuse(&mut b, SchGraphic::new(SchGraphicKind::Image { at: p(Nm::ZERO, Nm::ZERO), scale_ppm: 1_000_000, data: "AAAA".into() }));
        refuse(&mut b, SchGraphic::new(SchGraphicKind::Image { at: p(Nm::ZERO, Nm::ZERO), scale_ppm: 1_000_000, data: String::new() }));
        assert_eq!(graphics(&b).len(), 1);
        b.apply(&Cmd::sch(SchCmd::DeleteGraphic { id })).unwrap();
        assert!(graphics(&b).is_empty());
    }

    #[test]
    fn degenerate_graphics_are_refused() {
        let model = ConstraintModel::default();
        let mut b = Board::new(design_with_schematic(empty_sheet()), &model, mm(0.1), mm(0.3));
        let refuse = |b: &mut Board, g: SchGraphic| assert_eq!(b.apply(&Cmd::sch(SchCmd::AddGraphic { graphic: g })).unwrap_err()[0].check, "ops_bad_graphic");
        refuse(&mut b, rectangle(Nm::ZERO, Nm::ZERO, Nm::ZERO, mm(5.0)));
        refuse(&mut b, SchGraphic::new(SchGraphicKind::Circle { center: p(Nm::ZERO, Nm::ZERO), radius_nm: Nm::ZERO }));
        refuse(&mut b, SchGraphic::new(SchGraphicKind::Arc { start: p(Nm::ZERO, Nm::ZERO), mid: p(Nm::new(1), Nm::new(1)), end: p(um(2.0), um(2.0)) }));
        refuse(&mut b, SchGraphic::new(SchGraphicKind::Polygon { pts: vec![p(Nm::ZERO, Nm::ZERO), p(Nm::new(1), Nm::ZERO), p(Nm::new(1), Nm::ZERO)] }));
        refuse(&mut b, SchGraphic::new(SchGraphicKind::RuleArea { pts: vec![p(Nm::ZERO, Nm::ZERO), p(Nm::new(1), Nm::ZERO)], exclude_from_sim: false, exclude_from_bom: false, exclude_from_board: false, dnp: false }));
        let mut negative = rectangle(Nm::ZERO, Nm::ZERO, Nm::new(1), Nm::new(1));
        negative.width_nm = um(-5.0);
        refuse(&mut b, negative);
        assert!(b.design().schematic.as_ref().unwrap().extras.graphics.is_empty());
    }

    #[test]
    fn deleting_a_sheet_removes_its_symbol_and_pins_but_not_the_file_content() {
        use eda_model::ir::{LabelShape, SheetInstance, SheetPin};
        let mut sch = empty_sheet();
        sch.sheets.push(SheetInstance { id: "sheet_a".into(), name: "A".into(), file: "a.kicad_sch".into(), at: p(Nm::ZERO, Nm::ZERO), size: (mm(10.0), mm(10.0)), pins: vec![SheetPin { id: "pin_a".into(), name: "X".into(), shape: LabelShape::Input, at: p(Nm::ZERO, mm(5.0)) }], page: String::new() });
        let model = ConstraintModel::default();
        let mut design = design_with_schematic(sch);
        design.sheet_contents = Some([("a.kicad_sch".to_string(), empty_sheet())].into_iter().collect());
        let mut b = Board::new(design, &model, mm(0.1), mm(0.3));
        b.apply(&Cmd::sch(SchCmd::DeleteSheet { id: "sheet_a".into() })).unwrap();
        assert!(b.design().schematic.as_ref().unwrap().sheets.is_empty());
        assert!(b.design().sheet_contents.as_ref().unwrap().contains_key("a.kicad_sch"));
        assert_eq!(b.apply(&Cmd::sch(SchCmd::DeleteSheet { id: "sheet_a".into() })).unwrap_err()[0].check, "ops_unknown_sheet");
    }

    fn board_with_sheet() -> (ConstraintModel, Design) {
        let mut sch = empty_sheet();
        sch.sheets.push(SheetInstance { id: "sheet_a".into(), name: "A".into(), file: "a.kicad_sch".into(), at: p(mm(10.0), mm(10.0)), size: (mm(20.0), mm(10.0)), pins: vec![], page: String::new() });
        (ConstraintModel::default(), design_with_schematic(sch))
    }

    #[test]
    fn sheet_pins_are_added_on_the_border_edited_and_deleted() {
        let (model, design) = board_with_sheet();
        let mut b = Board::new(design, &model, mm(0.1), mm(0.3));
        // On the left edge, on the top edge (a corner counts for both).
        b.apply(&Cmd::sch(SchCmd::AddSheetPin { sheet: "sheet_a".into(), name: "VIN".into(), shape: LabelShape::Input, at: p(mm(10.0), mm(15.0)) })).unwrap();
        b.apply(&Cmd::sch(SchCmd::AddSheetPin { sheet: "sheet_a".into(), name: "CLK".into(), shape: LabelShape::Output, at: p(mm(25.0), mm(10.0)) })).unwrap();
        let pins = b.design().schematic.as_ref().unwrap().sheets[0].pins.clone();
        assert_eq!(pins.len(), 2);
        assert!(pins.iter().all(|pin| pin.id.starts_with("shpin_")));
        let vin = pins.iter().find(|pin| pin.name == "VIN").unwrap().clone();

        // Off the border, an empty name and an unknown sheet are refused.
        let add = |b: &mut Board, sheet: &str, name: &str, at: Point| b.apply(&Cmd::sch(SchCmd::AddSheetPin { sheet: sheet.into(), name: name.into(), shape: LabelShape::Passive, at })).unwrap_err()[0].check.clone();
        assert_eq!(add(&mut b, "sheet_a", "X", p(mm(15.0), mm(15.0))), "ops_sheet_pin_off_border");
        assert_eq!(add(&mut b, "sheet_a", " ", p(mm(10.0), mm(12.0))), "ops_bad_sheet_pin");
        assert_eq!(add(&mut b, "nope", "X", p(mm(10.0), mm(12.0))), "ops_unknown_sheet");

        // Renaming and reshaping, then a move along the edge; a no-op is refused.
        b.apply(&Cmd::sch(SchCmd::EditSheetPin { id: vin.id.clone(), name: Some("VBUS".into()), shape: Some(LabelShape::Bidirectional), at: None, size_nm: None, bold: None, italic: None })).unwrap();
        let renamed = b.design().schematic.as_ref().unwrap().sheets[0].pins.iter().find(|pin| pin.name == "VBUS").unwrap().clone();
        assert_eq!(renamed.shape, LabelShape::Bidirectional);
        assert_eq!(renamed.id, vin.id, "the pin keeps its id, so it is still the selected item after its Properties");
        b.apply(&Cmd::sch(SchCmd::EditSheetPin { id: renamed.id.clone(), name: None, shape: None, at: Some(p(mm(10.0), mm(18.0))), size_nm: None, bold: None, italic: None })).unwrap();
        let moved = b.design().schematic.as_ref().unwrap().sheets[0].pins.iter().find(|pin| pin.name == "VBUS").unwrap().clone();
        assert_eq!(moved.at, p(mm(10.0), mm(18.0)));
        assert_eq!(b.apply(&Cmd::sch(SchCmd::EditSheetPin { id: moved.id.clone(), name: Some("VBUS".into()), shape: None, at: None, size_nm: None, bold: None, italic: None })).unwrap_err()[0].check, "ops_sheet_pin_unchanged");
        assert_eq!(b.apply(&Cmd::sch(SchCmd::EditSheetPin { id: moved.id.clone(), name: None, shape: None, at: Some(p(mm(12.0), mm(12.0))), size_nm: None, bold: None, italic: None })).unwrap_err()[0].check, "ops_sheet_pin_off_border");

        b.apply(&Cmd::sch(SchCmd::DeleteSheetPin { id: moved.id.clone() })).unwrap();
        assert_eq!(b.design().schematic.as_ref().unwrap().sheets[0].pins.len(), 1);
        assert_eq!(b.apply(&Cmd::sch(SchCmd::DeleteSheetPin { id: moved.id })).unwrap_err()[0].check, "ops_unknown_sheet_pin");
    }

    #[test]
    fn change_symbol_swaps_the_library_symbol_of_every_unit_and_refuses_what_cannot_be() {
        use eda_model::ir::SymbolInstance;
        let mut sch = empty_sheet();
        let mk = |id: &str, lib: &str, unit: u32| SymbolInstance { id: id.into(), at: p(Nm::ZERO, Nm::ZERO), rot: 0, mirrored: false, mirror_y: false, lib_id: lib.into(), unit, value: "10k".into(), footprint: String::new(), datasheet: String::new(), dnp: false, exclude_from_bom: false, exclude_from_board: false, exclude_from_sim: false };
        sch.symbols.push(mk("R1", "Device:R", 1));
        let model = ConstraintModel::default();
        let mut b = Board::new(design_with_schematic(sch), &model, mm(0.1), mm(0.3));
        b.apply(&Cmd::sch(SchCmd::ChangeSymbol { id: "R1".into(), lib_id: "Device:C".into() })).unwrap();
        let s = &b.design().schematic.as_ref().unwrap().symbols[0];
        assert_eq!((s.lib_id.as_str(), s.value.as_str(), s.unit), ("Device:C", "10k", 1), "the fields stay; resetting them is the caller's");
        let check = |b: &mut Board, id: &str, lib: &str| b.apply(&Cmd::sch(SchCmd::ChangeSymbol { id: id.into(), lib_id: lib.into() })).unwrap_err()[0].check.clone();
        assert_eq!(check(&mut b, "R1", "Device:C"), "ops_symbol_unchanged");
        assert_eq!(check(&mut b, "R1", "nowhere:X"), "ops_unknown_lib_symbol");
        assert_eq!(check(&mut b, "R1", ""), "ops_bad_symbol");
        assert_eq!(check(&mut b, "R9", "Device:R"), "ops_unknown_symbol");
    }

    #[test]
    fn update_library_symbols_publishes_edited_symbols_once() {
        use eda_model::ir::{LibrarySymbol, SymbolLibrarySection};
        let mut design = design_with_schematic(empty_sheet());
        design.symbol_library = Some(SymbolLibrarySection { symbols: vec![LibrarySymbol::new_empty("Device:R"), LibrarySymbol::new_empty("Device:C")], ..Default::default() });
        let model = ConstraintModel::default();
        let mut b = Board::new(design, &model, mm(0.1), mm(0.3));
        let published = |b: &Board, id: &str| b.design().symbol_library.as_ref().unwrap().by_lib_id(id).unwrap().published;
        b.apply(&Cmd::sch(SchCmd::UpdateLibrarySymbols { lib_ids: vec!["Device:R".into(), "Device:Nowhere".into()] })).unwrap();
        assert!(published(&b, "Device:R") && !published(&b, "Device:C"));
        // Already published, unknown and empty requests have nothing to update.
        let err = |b: &mut Board, ids: Vec<&str>| b.apply(&Cmd::sch(SchCmd::UpdateLibrarySymbols { lib_ids: ids.into_iter().map(String::from).collect() })).unwrap_err()[0].check.clone();
        assert_eq!(err(&mut b, vec!["Device:R"]), "ops_nothing_to_update");
        assert_eq!(err(&mut b, vec!["Device:Nowhere"]), "ops_nothing_to_update");
        assert_eq!(err(&mut b, vec![]), "ops_nothing_to_update");
    }

    #[test]
    fn sch_edit_json_shape_is_op_plus_verb() {
        let c = Cmd::sch(SchCmd::SetLocked { ids: vec!["a".into()], locked: true });
        let json = serde_json::to_value(&c).unwrap();
        assert_eq!(json, serde_json::json!({ "op": "sch_edit", "verb": "set_locked", "ids": ["a"], "locked": true }));
        let back: Cmd = serde_json::from_value(json).unwrap();
        assert_eq!(back, c);
    }
}
