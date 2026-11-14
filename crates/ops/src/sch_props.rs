//! Properties (`E`, a double-click) of the schematic items that had no way to be edited after they were placed: labels, free texts, hierarchical
//! sheets, wires, buses, bus entries, graphic lines and junctions. Ported from `SCH_EDIT_TOOL::Properties` and the dialogs it opens
//! (eeschema/tools/sch_edit_tool.cpp, eeschema/dialogs/dialog_label_properties.cpp, dialog_text_properties.cpp, dialog_sheet_properties.cpp,
//! dialog_wire_bus_properties.cpp, dialog_junction_props.cpp at 8303b2ad).
//!
//! Each dialog is one verb here -- [`SchCmd::EditLabel`], [`SchCmd::EditText`], [`SchCmd::EditSheet`], [`SchCmd::SetStroke`] -- so an OK is one undo
//! step; the properties of a symbol, a sheet pin, a text box, a shape, a rule area and a directive label already have theirs (`Cmd::SetSymbolFields`,
//! [`SchCmd::EditSheetPin`], [`SchCmd::EditGraphic`]). A field left out of a verb is unchanged, and a verb that would change nothing is refused, so the undo
//! stack never records a step that did nothing.
//!
//! What the dialogs have that the drawing model has no place for is not here: fonts, text colours, a sheet's border and fill, its extra fields and its exclusion
//! flags. A label's own fields (its net class, ...) are Label Properties' grid ([`label_fields_after`]).

use crate::{empty_schematic_section, Board};
use eda_model::ir::{LabelKind, LabelShape, Millideg, Point, SchematicSection, TextJustify, TextVAlign, Nm};
use eda_model::sch_extras::{JunctionLook, LabelLook, LabelSpin, SchColor, SchLineStyle, SchStroke};
use eda_model::CheckResult;
use std::collections::{BTreeMap, BTreeSet};

fn fail(check: &str, what: &str, msg: impl Into<String>) -> Vec<CheckResult> {
    vec![CheckResult::fail(check, what, msg.into())]
}

/// `TEXT_MIN_SIZE_MM` / `TEXT_MAX_SIZE_MM` in `DIALOG_LABEL_PROPERTIES` and `DIALOG_TEXT_PROPERTIES`: "Don't allow text to disappear; it can be difficult to
/// correct if you can't select it" (`m_textSize.Validate( 0.01, 1000.0, EDA_UNITS::MM )`).
const TEXT_SIZE_MIN_NM: Nm = Nm::new(10_000);
const TEXT_SIZE_MAX_NM: Nm = Nm::new(1_000_000_000);

/// `COLOR4D::UNSPECIFIED` is all four channels zero: the colour of the item's layer.
fn specified(c: SchColor) -> Option<SchColor> {
    (c != SchColor { r: 0, g: 0, b: 0, a: 0 }).then_some(c)
}

/// One user field's looks in [`SchCmd::SetSheetFieldLooks`](crate::sch_edit::SchCmd::SetSheetFieldLooks): its place from the sheet's corner, angle and justification,
/// its font and colour.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SheetFieldLookEdit {
    pub name: String,
    #[serde(default, deserialize_with = "eda_model::units::de")]
    pub dx: Nm,
    #[serde(default, deserialize_with = "eda_model::units::de")]
    pub dy: Nm,
    #[serde(default)]
    pub angle: eda_model::ir::Millideg,
    #[serde(default)]
    pub h: eda_model::ir::TextJustify,
    #[serde(default)]
    pub v: eda_model::ir::TextVAlign,
    #[serde(default)]
    pub font: Option<String>,
    #[serde(default)]
    pub color: Option<SchColor>,
}

/// A sheet's file as `EnsureFileExtension( name, KiCadSchematicFileExtension )` makes it: a bare file name (no folders, as `Cmd::AddSheet` requires)
/// ending in `.kicad_sch`.
fn sheet_file_name(file: &str) -> Result<String, Vec<CheckResult>> {
    let file = file.trim();
    if file.is_empty() {
        return Err(fail("ops_bad_sheet", "sheet", "A sheet must have a valid file name."));
    }
    if file.contains('/') || file.contains('\\') {
        return Err(fail("ops_bad_sheet", file, "a sheet file is a bare file name (no folders)"));
    }
    Ok(if file.ends_with(".kicad_sch") { file.to_string() } else { format!("{file}.kicad_sch") })
}

/// Does the screen `from` hold (through its sheets, and theirs) a placement of the screen `target`? How a sheet is kept from showing a file that shows it back.
fn holds(contents: &BTreeMap<String, SchematicSection>, from: &str, target: &str, seen: &mut BTreeSet<String>) -> bool {
    if !seen.insert(from.to_string()) {
        return false;
    }
    let Some(screen) = contents.get(from) else { return false };
    screen.sheets.iter().any(|s| s.file == target || holds(contents, &s.file, target, seen))
}

/// What Label Properties sets of a label's text beyond what it says and which way it runs: the size, bold and italic (`SetTextSize`, `SetBold`, `SetItalic`).
#[derive(Debug, Clone, Default)]
pub(crate) struct LabelLookEdit {
    pub size_nm: Option<Nm>,
    pub bold: Option<bool>,
    pub italic: Option<bool>,
    /// The "Auto" check of a global or hierarchical label (`SetAutoRotateOnPlacement`).
    pub auto_rotate: Option<bool>,
    /// The label's own fields, as the dialog's grid leaves them (see [`label_fields_after`]).
    pub fields: Option<Vec<eda_model::sch_extras::LabelField>>,
}

/// `DIALOG_LABEL_PROPERTIES::TransferDataFromWindow` on the grid of a label's fields: an unnamed field with no text is deleted; an empty `Netclass` is deleted when
/// another `Netclass` field is there; an unnamed field with text is called "untitled"; "Net Class" is stored as `Netclass` (`GetCanonicalName`). Two fields of one name
/// are refused, as the grid's `OnGridCellChanging` refuses the second ("Field name '%s' already in use.", names compared without regard to case) -- but for the net
/// class, which Add Field gives again (`OnAddField`) and which puts the net in each class it names -- and so is a field named as the Sheet References are, which
/// only a global label has, once.
pub(crate) fn label_fields_after(id: &str, fields: &[eda_model::sch_extras::LabelField]) -> Result<Vec<eda_model::sch_extras::LabelField>, Vec<CheckResult>> {
    use eda_model::sch_extras::{INTERSHEET_REFS_FIELD, NETCLASS_FIELD};
    let mut out: Vec<eda_model::sch_extras::LabelField> = fields.iter().cloned().map(|mut f| {
        f.name = f.name.trim().to_string();
        if f.name == "Net Class" {
            f.name = NETCLASS_FIELD.to_string();
        }
        // the grid's Text Size of 50 mil is the default a field is kept with (`LabelField::size_nm` 0)
        if f.size_nm == eda_model::ir::DEFAULT_FIELD_SIZE_NM {
            f.size_nm = eda_model::units::Nm::ZERO;
        }
        f
    }).collect();
    for i in (0..out.len()).rev() {
        let (name, text) = (out[i].name.clone(), out[i].text.clone());
        if name.is_empty() && text.is_empty() {
            out.remove(i);
        } else if name == NETCLASS_FIELD && text.is_empty() {
            if out.iter().filter(|f| f.name == NETCLASS_FIELD).count() > 1 {
                out.remove(i);
            }
        } else if name.is_empty() {
            out[i].name = "untitled".to_string();
        }
    }
    let mut seen: BTreeSet<String> = BTreeSet::new();
    for f in &out {
        // (more than one net class is allowed: OnAddField adds another, and the net is then in each class)
        if f.name.eq_ignore_ascii_case(INTERSHEET_REFS_FIELD) || (f.name != NETCLASS_FIELD && !seen.insert(f.name.to_lowercase())) {
            return Err(fail("ops_label_field_name", id, format!("Field name '{}' already in use.", f.name)));
        }
    }
    Ok(out)
}

/// What [`SchCmd::SetTextAttrs`](crate::sch_edit::SchCmd::SetTextAttrs) changes; a member left out is unchanged.
#[derive(Debug, Clone, Default)]
pub(crate) struct TextAttrsEdit {
    pub bold: Option<bool>,
    pub italic: Option<bool>,
    pub color: Option<SchColor>,
    pub href: Option<String>,
    pub font: Option<String>,
    pub exclude_from_sim: Option<bool>,
    pub thickness_nm: Option<Nm>,
}

/// `EDA_TEXT::SetBold( bold )` on the free text, label or text box `id` (`TextAttrs::set_bold_pen`): the stroke font's pen set anew for the weight, its size
/// read off the item.
pub(crate) fn rebold_pen(sch: &mut eda_model::ir::SchematicSection, id: &str, bold: bool) {
    let size = if let Some(t) = sch.texts.iter().find(|t| t.id == id) {
        t.size_nm
    } else if sch.labels.iter().any(|l| l.id == id) {
        sch.extras.label_looks.get(id).copied().unwrap_or_default().text_size_nm()
    } else if let Some(eda_model::sch_extras::SchGraphicKind::TextBox { size_nm, .. }) = sch.extras.graphics.iter().find(|g| g.id == id).map(|g| &g.shape) {
        *size_nm
    } else {
        return;
    };
    let mut a = sch.extras.text_attrs.get(id).cloned().unwrap_or_default();
    a.set_bold_pen(bold, size);
    if a.is_default() {
        sch.extras.text_attrs.remove(id);
    } else {
        sch.extras.text_attrs.insert(id.to_string(), a);
    }
}

/// Whether the free text, label or text box `id` is bold (a free text's in `attrs`, a label's in its look, a text box's in its shape).
fn text_bold(sch: &eda_model::ir::SchematicSection, id: &str, attrs: &eda_model::sch_extras::TextAttrs) -> bool {
    if sch.labels.iter().any(|l| l.id == id) {
        return sch.extras.label_looks.get(id).is_some_and(|l| l.bold);
    }
    if let Some(g) = sch.extras.graphics.iter().find(|g| g.id == id) {
        return matches!(g.shape, eda_model::sch_extras::SchGraphicKind::TextBox { bold: true, .. });
    }
    attrs.bold
}

/// What [`SchCmd::SetSheetProps`](crate::sch_edit::SchCmd::SetSheetProps) changes; a member left out is unchanged.
#[derive(Debug, Clone, Default)]
pub(crate) struct SheetPropsEdit {
    pub border_width_nm: Option<Nm>,
    pub border_color: Option<SchColor>,
    pub fill_color: Option<SchColor>,
    pub exclude_from_sim: Option<bool>,
    pub exclude_from_bom: Option<bool>,
    pub exclude_from_board: Option<bool>,
    pub dnp: Option<bool>,
}

/// `DEFAULT_FONT_NAME` (`include/font/kicad_font_name.h`, "Default Font"): the font choice that is the text's default font, no font of its own.
pub(crate) const DEFAULT_FONT_NAME: &str = "Default Font";

/// `EDA_TEXT::ValidateHyperlink`: none, a page of this schematic (`IsGotoPageHref`: "#" and the page), or a URL with a scheme (`wxURI::Create( aURL ) &&
/// uri.HasScheme()`: letters, digits, `+`, `-` and `.` after a letter, then ':').
pub(crate) fn valid_hyperlink(url: &str) -> bool {
    if url.is_empty() || url.starts_with('#') {
        return true;
    }
    let Some((scheme, _)) = url.split_once(':') else { return false };
    let mut chars = scheme.chars();
    chars.next().is_some_and(|c| c.is_ascii_alphabetic()) && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
}

impl<'a> Board<'a> {
    /// Label Properties: see [`SchCmd::EditLabel`](crate::sch_edit::SchCmd::EditLabel).
    pub(crate) fn edit_sch_label(&mut self, id: &str, text: Option<&str>, shape: Option<LabelShape>, spin: Option<LabelSpin>, look: LabelLookEdit) -> Result<(), Vec<CheckResult>> {
        let text = text.map(str::trim);
        if text == Some("") {
            // DIALOG_LABEL_PROPERTIES::TransferDataFromWindow: `if( text.IsEmpty() && !m_currentLabel->IsNew() )`
            return Err(fail("ops_bad_label", id, "Label can not be empty."));
        }
        if let Some(size) = look.size_nm {
            // `m_textSize.Validate( 0.01, 1000.0, EDA_UNITS::MM )`: "Don't allow text to disappear"
            if !(TEXT_SIZE_MIN_NM..=TEXT_SIZE_MAX_NM).contains(&size) {
                return Err(fail("ops_bad_text_size", id, "The text size must be between 0.01 and 1000 mm."));
            }
        }
        let model = self.model;
        let sch = self.schematic_mut()?;
        let Some(label) = sch.labels.iter_mut().find(|l| l.id == id) else {
            return Err(fail("ops_unknown_item", id, "no label with this id"));
        };
        // the shape sizer is hidden for a plain label (`m_shapeSizer`): refused before anything changes
        if shape.is_some() && label.kind == LabelKind::Local {
            return Err(fail("ops_bad_label_shape", id, "a local label has no shape"));
        }
        // the "Auto" check is hidden for a plain label (`AutoRotateOnPlacementSupported`)
        if look.auto_rotate == Some(true) && !crate::sch_labels::auto_rotate_supported(&label.kind) {
            return Err(fail("ops_label_no_auto_rotate", id, "only a global or a hierarchical label can rotate itself on placement"));
        }
        let auto_supported = crate::sch_labels::auto_rotate_supported(&label.kind);
        let fields_now = look.fields.as_deref().map(|f| label_fields_after(id, f)).transpose()?;
        let mut changed = false;
        if let Some(now) = fields_now {
            let before = sch.extras.label_fields.get(id).cloned().unwrap_or_default();
            if before != now {
                changed = true;
                if now.is_empty() {
                    sch.extras.label_fields.remove(id);
                } else {
                    sch.extras.label_fields.insert(id.to_string(), now);
                }
            }
        }
        if let Some(t) = text {
            if label.net != t {
                label.net = t.to_string();
                changed = true;
            }
        }
        if let (Some(new_shape), LabelKind::Global { shape } | LabelKind::Hierarchical { shape }) = (shape, &mut label.kind) {
            if *shape != new_shape {
                *shape = new_shape;
                changed = true;
            }
        }
        // `SetTextSize`, then `SetBold` and `SetItalic` ("Must come after SetTextSize()") and `SetAutoRotateOnPlacement`; a look equal to the default keeps no entry
        let before: LabelLook = sch.extras.label_looks.get(id).copied().unwrap_or_default();
        let mut now = before;
        if let Some(size) = look.size_nm {
            now.size_nm = if size == eda_model::ir::DEFAULT_FIELD_SIZE_NM { Nm::ZERO } else { size };
        }
        if let Some(b) = look.bold {
            now.bold = b;
        }
        if let Some(i) = look.italic {
            now.italic = i;
        }
        // `SetBold`: the stroke font's pen set anew for the weight
        if now.bold != before.bold {
            rebold_pen(sch, id, now.bold);
        }
        if let Some(auto) = look.auto_rotate {
            now.auto_rotate = auto && auto_supported;
        }
        if now != before {
            changed = true;
            if now.is_default() {
                sch.extras.label_looks.remove(id);
            } else {
                sch.extras.label_looks.insert(id.to_string(), now);
            }
        }
        // "frame->AutoRotateItem( frame->GetScreen(), m_currentLabel )" while the check is on, else "if( !AutoRotateOnPlacement() && GetSpinStyle() != selected ) SetSpinStyle( selected )"
        if now.auto_rotate {
            let mut scene = crate::sch_scene::Scene::new(sch, model);
            let index = scene.sch.labels.iter().position(|l| l.id == id).unwrap_or(0);
            let before_spin = crate::sch_move::effective_spin(&scene, index);
            if scene.auto_rotate_item(crate::sch_scene::Item::Label(index)) {
                let new_spin = scene.sch.extras.label_spins.get(id).copied().unwrap_or(before_spin);
                if sch.extras.label_spins.insert(id.to_string(), new_spin) != Some(new_spin) {
                    // `SetSpinStyle`: the justification the spin says, a centred text or another vertical one gone
                    sch.extras.label_justify.remove(id);
                    changed = true;
                }
            }
        } else if let Some(new_spin) = spin {
            if sch.extras.label_spins.insert(id.to_string(), new_spin) != Some(new_spin) {
                sch.extras.label_justify.remove(id);
                changed = true;
            }
        }
        if !changed {
            return Err(fail("ops_label_unchanged", id, "the label already has these properties"));
        }
        Ok(())
    }

    /// Text Properties: see [`SchCmd::EditText`](crate::sch_edit::SchCmd::EditText).
    pub(crate) fn edit_sch_text(&mut self, id: &str, text: Option<&str>, size_nm: Option<Nm>, angle: Option<Millideg>, h: Option<TextJustify>, v: Option<TextVAlign>) -> Result<(), Vec<CheckResult>> {
        if text == Some("") {
            return Err(fail("ops_bad_text", id, "Text can not be empty."));
        }
        if let Some(size) = size_nm {
            if !(TEXT_SIZE_MIN_NM..=TEXT_SIZE_MAX_NM).contains(&size) {
                return Err(fail("ops_bad_text_size", id, "The text size must be between 0.01 and 1000 mm."));
            }
        }
        if angle.is_some_and(|a| a >= 360_000) {
            return Err(fail("ops_bad_text_angle", id, "a text angle is below 360 degrees"));
        }
        let sch = self.schematic_mut()?;
        let Some(t) = sch.texts.iter_mut().find(|t| t.id == id) else {
            return Err(fail("ops_unknown_item", id, "no text with this id"));
        };
        // a text is only ever along the sheet or up it (`m_vertical->IsChecked() ? ANGLE_VERTICAL : ANGLE_HORIZONTAL`); a legacy angle in between reads as the nearer
        let upright = |a: Millideg| if eda_model::kicad_geom::keep_upright_is_vertical(a as f64 / 1000.0) { 90_000 } else { 0 };
        let before = (t.content.clone(), t.size_nm, upright(t.angle), sch.extras.text_justify_of(id));
        if let Some(text) = text {
            t.content = text.to_string();
        }
        if let Some(size) = size_nm {
            t.size_nm = size;
        }
        t.angle = upright(angle.unwrap_or(t.angle));
        let mut justify = before.3;
        if let Some(h) = h {
            justify.h = h;
        }
        if let Some(v) = v {
            justify.v = v;
        }
        let after = (t.content.clone(), t.size_nm, t.angle, justify);
        // `SetTextSize( VECTOR2I( size, size ) )`: a size set makes the text as wide as it is high (an imported DXF text's own glyph width goes)
        let squared = size_nm.is_some() && sch.extras.text_widths.contains_key(id);
        if before == after && !squared {
            // a legacy angle that was not upright is still made so, but that alone is no edit
            return Err(fail("ops_text_unchanged", id, "the text already has these properties"));
        }
        if size_nm.is_some() {
            sch.extras.text_widths.remove(id);
        }
        sch.extras.set_text_justify(id, justify);
        Ok(())
    }

    /// Sheet Properties: see [`SchCmd::EditSheet`](crate::sch_edit::SchCmd::EditSheet).
    pub(crate) fn edit_sch_sheet(&mut self, id: &str, name: Option<&str>, file: Option<&str>) -> Result<(), Vec<CheckResult>> {
        let new_name = match name.map(str::trim) {
            Some("") => return Err(fail("ops_bad_sheet", id, "a sheet needs a name")),
            other => other.map(str::to_string),
        };
        let new_file = file.map(sheet_file_name).transpose()?;
        let (cur_name, cur_file) = {
            let sch = self.schematic()?;
            let Some(s) = sch.sheets.iter().find(|s| s.id == id) else {
                return Err(fail("ops_unknown_sheet", id, "no sheet with this id"));
            };
            (s.name.clone(), s.file.clone())
        };
        let renamed = new_name.as_ref().is_some_and(|n| *n != cur_name);
        let relinked = new_file.as_ref().is_some_and(|f| *f != cur_file);
        if !renamed && !relinked {
            return Err(fail("ops_sheet_unchanged", id, "the sheet already has these properties"));
        }
        // a name another sheet of the page has is allowed (`DIALOG_SHEET_PROPERTIES` has no such check; ERC's `duplicate_sheet_names` reports it, UI-LEFT S21)
        if let Some(new) = new_file.as_ref().filter(|_| relinked) {
            // `SCH_EDIT_FRAME::ChangeSheetFile`: a sheet may not show a file that shows this sheet's own screen back.
            if let Some(here) = &self.focus {
                let none = BTreeMap::new();
                let contents = self.design.sheet_contents.as_ref().unwrap_or(&none);
                if new == here || holds(contents, new, here, &mut BTreeSet::new()) {
                    return Err(fail("ops_sheet_recursion", new, format!("'{new}' shows this sheet's own screen: a sheet cannot hold itself")));
                }
            }
            // The other placements of the file being left, on the root and on every screen.
            let others = {
                let root = self.design.schematic.iter();
                let screens = self.design.sheet_contents.iter().flat_map(|c| c.values());
                root.chain(screens).flat_map(|s| s.sheets.iter()).filter(|s| s.file == cur_file && s.id != id).count()
            };
            let contents = self.design.sheet_contents.get_or_insert_with(BTreeMap::new);
            if !contents.contains_key(new) {
                // A name the project does not have: "Create new file with the contents of ..." when another sheet still shows the old file, else the
                // file is renamed (`renameFile`); a screen that was never read starts empty, as `Cmd::AddSheet` makes one.
                let content = match contents.get(&cur_file).cloned() {
                    Some(old) => {
                        if others == 0 && self.focus.as_deref() != Some(cur_file.as_str()) {
                            contents.remove(&cur_file);
                        }
                        old
                    }
                    None => empty_schematic_section(),
                };
                contents.insert(new.clone(), content);
            }
        }
        let sch = self.schematic_mut()?;
        let Some(s) = sch.sheets.iter_mut().find(|s| s.id == id) else {
            return Err(fail("ops_unknown_sheet", id, "no sheet with this id"));
        };
        if let Some(n) = new_name {
            s.name = n;
        }
        if let Some(f) = new_file {
            s.file = f;
        }
        Ok(())
    }

    /// The panel's Bold, Italic, Color and Hyperlink of free texts and Color of labels, and the Font, Color, Link and Exclude from simulation of Text, Label and
    /// Text Box Properties: see [`SchCmd::SetTextAttrs`](crate::sch_edit::SchCmd::SetTextAttrs). A label's bold or italic, its hyperlink or its exclusion, and a
    /// text box's bold or italic, are refused: they are Label Properties' (`EditLabel`), `SCH_LABEL_BASE_DESC` masks the hyperlink, and a text box keeps its
    /// own (`EditGraphic`). A hyperlink that is neither "#<page>" nor a URL with a scheme is refused with `DIALOG_TEXT_PROPERTIES::TransferDataFromWindow`'s
    /// message. Refused when nothing changes.
    pub(crate) fn set_sch_text_attrs(&mut self, ids: &[String], edit: &TextAttrsEdit) -> Result<(), Vec<CheckResult>> {
        if ids.is_empty() {
            return Err(fail("ops_nothing_selected", "text", "no items given"));
        }
        if let Some(h) = edit.href.as_deref() {
            if !valid_hyperlink(h.trim()) {
                return Err(fail(
                    "ops_bad_hyperlink",
                    h,
                    "Invalid hyperlink destination. Please enter either a valid URL (e.g. file:// or http(s)://) or \"#<page number>\" to create a hyperlink to a page in this schematic.",
                ));
            }
        }
        let sch = self.schematic_mut()?;
        for id in ids {
            let text = sch.texts.iter().any(|t| &t.id == id);
            let label = sch.labels.iter().any(|l| &l.id == id);
            let text_box = sch.extras.graphics.iter().any(|g| &g.id == id && matches!(g.shape, eda_model::sch_extras::SchGraphicKind::TextBox { .. }));
            // a directive label is an `SCH_TEXT` too: its "Font" and "Color" rows (`EDA_TEXT_DESC`; its bold, italic and link are masked, `SCH_DIRECTIVE_LABEL_DESC`)
            let directive = sch.extras.graphics.iter().any(|g| &g.id == id && matches!(g.shape, eda_model::sch_extras::SchGraphicKind::Directive { .. }));
            // a sheet pin is an `EDA_TEXT` too: Sheet Pin Properties' Font and Color (`m_fontCtrl`, `m_textColorSwatch`; its size, bold and italic are
            // `edit_sheet_pin`'s, UI-LEFT-2 SS15)
            let sheet_pin = sch.sheets.iter().any(|s| s.pins.iter().any(|p| &p.id == id));
            if !text && !label && !text_box && !directive && !sheet_pin {
                return Err(fail("ops_unknown_item", id, "no free text, text box, label or sheet pin with this id"));
            }
            if sheet_pin && (edit.bold.is_some() || edit.italic.is_some() || edit.href.is_some() || edit.exclude_from_sim.is_some()) {
                return Err(fail("ops_bad_text", id, "a sheet pin's bold and italic are set in Sheet Pin Properties, and a sheet pin has no hyperlink or simulation exclusion"));
            }
            if directive && (edit.bold.is_some() || edit.italic.is_some() || edit.href.is_some() || edit.exclude_from_sim.is_some()) {
                return Err(fail("ops_bad_text", id, "a directive label has no bold, italic, hyperlink or simulation exclusion of its own"));
            }
            if label && (edit.bold.is_some() || edit.italic.is_some() || edit.href.is_some() || edit.exclude_from_sim.is_some()) {
                return Err(fail("ops_bad_text", id, "a label's bold and italic are set in Label Properties, and a label has no hyperlink or simulation exclusion"));
            }
            if text_box && (edit.bold.is_some() || edit.italic.is_some()) {
                return Err(fail("ops_bad_text", id, "a text box's bold and italic are set with its shape"));
            }
            // Auto Thickness: `EDA_TEXT_DESC`'s row, which `SCH_TEXT_DESC` and `SCH_TEXTBOX_DESC` leave (they mask the Thickness row only); not a sheet pin's or a
            // directive label's here
            if (directive || sheet_pin) && edit.thickness_nm.is_some() {
                return Err(fail("ops_bad_text", id, "only a free text, a label or a text box has an Auto Thickness row"));
            }
            if sch.extras.is_locked(id) {
                return Err(fail("ops_locked", id, "the item is locked"));
            }
        }
        let before = sch.extras.text_attrs.clone();
        for id in ids {
            let mut a = sch.extras.text_attrs.get(id).cloned().unwrap_or_default();
            if let Some(b) = edit.bold {
                // `EDA_TEXT::SetBold`: a stroke font's pen is set anew for the weight
                if a.bold != b {
                    let size = sch.texts.iter().find(|t| &t.id == id).map(|t| t.size_nm).unwrap_or(eda_model::ir::DEFAULT_FIELD_SIZE_NM);
                    a.set_bold_pen(b, size);
                }
                a.bold = b;
            }
            if let Some(i) = edit.italic {
                a.italic = i;
            }
            if let Some(c) = edit.color {
                a.color = specified(c);
            }
            if let Some(t) = edit.thickness_nm {
                // `SetAutoThickness( aAuto )`: ticked, no pen of its own (a bold text keeps the mark, its bold pen being otherwise implied); unticked, the pen
                // it was drawn with
                let bold = text_bold(sch, id, &a);
                a.thickness_nm = if t > Nm::ZERO { Some(t) } else if bold { Some(Nm::ZERO) } else { None };
            }
            if let Some(h) = edit.href.as_deref() {
                a.href = (!h.trim().is_empty()).then(|| h.trim().to_string());
            }
            if let Some(f) = edit.font.as_deref() {
                // `FONT_CHOICE::GetFontSelection`: the first choice, "Default Font", is no font of the text's own (`nullptr`)
                let f = f.trim();
                a.font = (!f.is_empty() && f != DEFAULT_FONT_NAME).then(|| f.to_string());
            }
            if let Some(x) = edit.exclude_from_sim {
                a.exclude_from_sim = x;
            }
            if a.is_default() {
                sch.extras.text_attrs.remove(id);
            } else {
                sch.extras.text_attrs.insert(id.clone(), a);
            }
        }
        if sch.extras.text_attrs == before {
            return Err(fail("ops_nothing_to_change", "text", "the items already have these attributes"));
        }
        Ok(())
    }

    /// "Transform Offset X/Y" of an image: see [`SchCmd::SetImageOffset`](crate::sch_edit::SchCmd::SetImageOffset). Refused for an item that is no image, a
    /// locked one, and an offset it already has.
    pub(crate) fn set_image_offset(&mut self, id: &str, offset: Point) -> Result<(), Vec<CheckResult>> {
        let sch = self.schematic_mut()?;
        if !sch.extras.graphics.iter().any(|g| g.id == id && matches!(g.shape, eda_model::sch_extras::SchGraphicKind::Image { .. })) {
            return Err(fail("ops_unknown_item", id, "no image with this id"));
        }
        if sch.extras.is_locked(id) {
            return Err(fail("ops_locked", id, "the image is locked"));
        }
        let now = sch.extras.image_offsets.get(id).copied().unwrap_or(Point { x: Nm::ZERO, y: Nm::ZERO });
        if now == offset {
            return Err(fail("ops_nothing_to_change", id, "the image already has this transform offset"));
        }
        if offset == (Point { x: Nm::ZERO, y: Nm::ZERO }) {
            sch.extras.image_offsets.remove(id);
        } else {
            sch.extras.image_offsets.insert(id.to_string(), offset);
        }
        Ok(())
    }

    /// Sheet Properties' Style and Attributes: see [`SchCmd::SetSheetProps`](crate::sch_edit::SchCmd::SetSheetProps).
    pub(crate) fn set_sheet_props(&mut self, id: &str, e: &SheetPropsEdit) -> Result<(), Vec<CheckResult>> {
        let sch = self.schematic_mut()?;
        if !sch.sheets.iter().any(|s| s.id == id) {
            return Err(fail("ops_unknown_sheet", id, "no sheet with this id"));
        }
        if sch.extras.is_locked(id) {
            return Err(fail("ops_locked", id, "the sheet is locked"));
        }
        if e.border_width_nm.is_some_and(|w| w < Nm::ZERO) {
            return Err(fail("ops_bad_sheet", id, "a border cannot be thinner than nothing"));
        }
        let mut p = sch.extras.sheet_props_of(id);
        if let Some(w) = e.border_width_nm {
            p.border_width_nm = w;
        }
        if let Some(c) = e.border_color {
            p.border_color = specified(c);
        }
        if let Some(c) = e.fill_color {
            p.fill_color = specified(c);
        }
        if let Some(v) = e.exclude_from_sim {
            p.exclude_from_sim = v;
        }
        if let Some(v) = e.exclude_from_bom {
            p.exclude_from_bom = v;
        }
        if let Some(v) = e.exclude_from_board {
            p.exclude_from_board = v;
        }
        if let Some(v) = e.dnp {
            p.dnp = v;
        }
        if !sch.extras.set_sheet_props(id, p) {
            return Err(fail("ops_nothing_to_change", id, "the sheet already has these properties"));
        }
        Ok(())
    }

    /// New Top-Level Sheet: see [`SchCmd::AddTopLevelSheet`](crate::sch_edit::SchCmd::AddTopLevelSheet).
    pub(crate) fn add_top_level_sheet(&mut self, name: &str) -> Result<(), Vec<CheckResult>> {
        let name = name.trim();
        let sch = self.schematic()?;
        if !sch.extras.virtual_root {
            return Err(fail("ops_not_top_level", name, "this design's root sheet is its only top-level sheet: a sheet is added to it with Draw Sheet"));
        }
        // `if( dlg.ShowModal() == wxID_OK && !dlg.GetValue().IsEmpty() )`
        if name.is_empty() {
            return Err(fail("ops_bad_sheet", "sheet", "a sheet needs a name"));
        }
        // "Generate a unique filename": `name.Replace( " ", "_" ).Lower()` and `.kicad_sch`
        let stem = name.replace(' ', "_").to_lowercase();
        let stem = stem.strip_suffix(".kicad_sch").map(str::to_string).unwrap_or(stem);
        let taken = |f: &str| self.design.sheet_contents.as_ref().is_some_and(|c| c.contains_key(f));
        let mut file = format!("{stem}.kicad_sch");
        let mut n = 1;
        while taken(&file) {
            file = format!("{stem}_{n}.kicad_sch");
            n += 1;
        }
        // the lowest whole page number no sheet path shows (`PageNumberExists`)
        let pages: BTreeSet<String> = eda_model::sheet_list::walk(&self.design).iter().map(|e| e.effective_page()).collect();
        let page = (1..).find(|n: &u32| !pages.contains(&n.to_string())).unwrap_or(1).to_string();
        // the root's grid of top-level sheets (`combine_top_level_sheets`): five to a row, one inch apart
        let i = sch.sheets.len() as i64;
        const GRID: Nm = Nm::new(25_400_000);
        let at = Point { x: GRID * (1 + 2 * (i % 5)), y: GRID * (1 + 2 * (i / 5)) };
        self.add_sheet(name, &file, at, (Nm::new(12_700_000), Nm::new(3_810_000)))?;
        let sch = self.schematic_mut()?;
        if let Some(s) = sch.sheets.last_mut() {
            s.page = page;
        }
        Ok(())
    }

    /// Sheet Properties' looks of the user fields: see [`SchCmd::SetSheetFieldLooks`](crate::sch_edit::SchCmd::SetSheetFieldLooks).
    pub(crate) fn set_sheet_field_looks(&mut self, id: &str, fields: &[SheetFieldLookEdit]) -> Result<(), Vec<CheckResult>> {
        let sch = self.schematic_mut()?;
        if !sch.sheets.iter().any(|s| s.id == id) {
            return Err(fail("ops_unknown_sheet", id, "no sheet with this id"));
        }
        if sch.extras.is_locked(id) {
            return Err(fail("ops_locked", id, "the sheet is locked"));
        }
        let names: Vec<String> = sch.extras.sheet_fields.get(id).into_iter().flatten().map(|f| f.name.clone()).collect();
        let mut changed = false;
        for f in fields {
            if !names.contains(&f.name) {
                return Err(fail("ops_unknown_field", &f.name, "the sheet has no user field of this name"));
            }
            // `FIELDS_GRID_TABLE::SetValue( FDC_ORIENTATION )`: "Horizontal" or "Vertical"
            let place = eda_model::sch_extras::SheetFieldPlace { dx: f.dx, dy: f.dy, angle: if f.angle.rem_euclid(180_000) == 90_000 { 90_000 } else { 0 }, h: f.h, v: f.v };
            let places = sch.extras.sheet_field_places.entry(id.to_string()).or_default();
            let before = places.get(&f.name).copied().unwrap_or_default();
            if place != before {
                changed = true;
                if place.is_default() {
                    places.remove(&f.name);
                } else {
                    places.insert(f.name.clone(), place);
                }
            }
            if places.is_empty() {
                sch.extras.sheet_field_places.remove(id);
            }
            // the grid has no pen column: the field's own pen stays (`FIELDS_GRID_TABLE`)
            let pen = sch.extras.field_look(id, &f.name).and_then(|l| l.thickness_nm);
            let look = eda_model::sch_extras::FieldFontLook { font: f.font.as_deref().and_then(crate::sch_symbol_props::font_of), color: f.color.and_then(specified), thickness_nm: pen };
            changed |= sch.extras.set_field_look(id, &f.name, look);
        }
        if !changed {
            return Err(fail("ops_nothing_to_change", id, "the sheet's fields already look so"));
        }
        Ok(())
    }

    /// Sheet Properties' user fields: see [`SchCmd::SetSheetFields`](crate::sch_edit::SchCmd::SetSheetFields).
    pub(crate) fn set_sheet_fields(&mut self, id: &str, fields: &[eda_model::sch_extras::LabelField]) -> Result<(), Vec<CheckResult>> {
        let sch = self.schematic_mut()?;
        if !sch.sheets.iter().any(|s| s.id == id) {
            return Err(fail("ops_unknown_sheet", id, "no sheet with this id"));
        }
        if sch.extras.is_locked(id) {
            return Err(fail("ops_locked", id, "the sheet is locked"));
        }
        let mut kept: Vec<eda_model::sch_extras::LabelField> = Vec::new();
        for f in fields {
            let name = f.name.trim();
            // "if( field.GetName( false ).IsEmpty() && field.GetText().IsEmpty() ) continue": an empty row is dropped
            if name.is_empty() && f.text.is_empty() {
                continue;
            }
            if name.is_empty() {
                return Err(fail("ops_bad_field", id, "Fields must have a name."));
            }
            if name.eq_ignore_ascii_case("Sheetname") || name.eq_ignore_ascii_case("Sheetfile") || kept.iter().any(|k| k.name == name) {
                return Err(fail("ops_field_exists", id, format!("Field name '{name}' already in use.")));
            }
            kept.push(eda_model::sch_extras::LabelField { name: name.to_string(), ..f.clone() });
        }
        let before = sch.extras.sheet_fields.get(id).cloned().unwrap_or_default();
        if kept == before {
            return Err(fail("ops_nothing_to_change", id, "the sheet already has these fields"));
        }
        // a field gone (deleted, or renamed: its new name's looks come with `set_sheet_field_looks`) leaves no place or look behind
        if let Some(places) = sch.extras.sheet_field_places.get_mut(id) {
            places.retain(|name, _| kept.iter().any(|k| &k.name == name));
            if places.is_empty() {
                sch.extras.sheet_field_places.remove(id);
            }
        }
        let gone: Vec<String> = sch.extras.field_looks.get(id).into_iter().flat_map(|l| l.keys()).filter(|n| *n != "Sheetname" && *n != "Sheetfile" && !kept.iter().any(|k| &k.name == *n)).cloned().collect();
        for name in gone {
            sch.extras.set_field_look(id, &name, eda_model::sch_extras::FieldFontLook::default());
        }
        if kept.is_empty() {
            sch.extras.sheet_fields.remove(id);
        } else {
            sch.extras.sheet_fields.insert(id.to_string(), kept);
        }
        Ok(())
    }

    /// The panel's Exclude From Position Files of symbols: see [`SchCmd::SetExcludeFromPosFiles`](crate::sch_edit::SchCmd::SetExcludeFromPosFiles). Refused when nothing
    /// changes.
    pub(crate) fn set_exclude_from_pos_files(&mut self, ids: &[String], exclude: bool) -> Result<(), Vec<CheckResult>> {
        if ids.is_empty() {
            return Err(fail("ops_nothing_selected", "attributes", "select a symbol first"));
        }
        let sch = self.schematic_mut()?;
        let mut keys = Vec::new();
        for id in ids {
            let units: Vec<u32> = sch.symbols.iter().filter(|s| &s.id == id).map(|s| s.unit).collect();
            if units.is_empty() {
                return Err(fail("ops_unknown_symbol", id, "no symbol instance with this reference"));
            }
            keys.extend(units.into_iter().map(|u| eda_model::ir::field_key(id, u)));
        }
        let before = sch.extras.excluded_from_pos_files.clone();
        for k in keys {
            if exclude {
                sch.extras.excluded_from_pos_files.insert(k);
            } else {
                sch.extras.excluded_from_pos_files.remove(&k);
            }
        }
        if sch.extras.excluded_from_pos_files == before {
            return Err(fail("ops_nothing_to_change", "attributes", "the symbols already have this attribute"));
        }
        Ok(())
    }

    /// The panel's Start X/Y and End X/Y of a wire, a bus or a graphic line, and the point editor's drag of a graphic line's end: see
    /// [`SchCmd::SetLineEnds`](crate::sch_edit::SchCmd::SetLineEnds). The ends go where they are given even when they meet (`SCH_LINE::SetStartPoint` /
    /// `SetEndPoint` check nothing); then the commit's clean-up (`SCHEMATIC::CleanUp`, KiCad 8303b2ad `eeschema/schematic.cpp`) deletes a wire or bus that is
    /// `IsNull()` -- one undo step brings it back -- and keeps a zero-length graphic line, as it gathers only the `LAYER_WIRE` / `LAYER_BUS` lines (UI-LEFT-3 S3-4).
    pub(crate) fn set_sch_line_ends(&mut self, id: &str, start: Option<Point>, end: Option<Point>) -> Result<(), Vec<CheckResult>> {
        if start.is_none() && end.is_none() {
            return Err(fail("ops_bad_wire", id, "nothing to change: give a new start, a new end, or both"));
        }
        let sch = self.schematic_mut()?;
        if sch.extras.is_locked(id) {
            return Err(fail("ops_locked", id, "the item is locked"));
        }
        let is_wire = sch.wires.iter().any(|w| w.id == id);
        let pts = if let Some(w) = sch.wires.iter_mut().find(|w| w.id == id) {
            &mut w.pts
        } else if let Some(l) = sch.lines.iter_mut().find(|l| l.id == id) {
            &mut l.pts
        } else {
            return Err(fail("ops_unknown_item", id, "no wire, bus or graphic line with this id"));
        };
        let mut next = pts.clone();
        if let (Some(s), Some(first)) = (start, next.first_mut()) {
            *first = s;
        }
        if let (Some(e), Some(last)) = (end, next.last_mut()) {
            *last = e;
        }
        if next == *pts {
            return Err(fail("ops_nothing_to_change", id, "the line already ends there"));
        }
        *pts = next;
        if is_wire {
            self.commit_clean_up()?;
        }
        Ok(())
    }

    /// Wire/Bus, Line and Junction Properties: see [`SchCmd::SetStroke`](crate::sch_edit::SchCmd::SetStroke).
    pub(crate) fn set_sch_stroke(&mut self, ids: &[String], width_nm: Option<Nm>, style: Option<SchLineStyle>, color: Option<SchColor>, diameter_nm: Option<Nm>) -> Result<(), Vec<CheckResult>> {
        if ids.is_empty() {
            return Err(fail("ops_nothing_selected", "stroke", "no items given"));
        }
        let sch = self.schematic_mut()?;
        // What each item is, before anything changes: all or nothing.
        enum Kind {
            /// A wire, a bus or a bus entry (`SCH_LINE`, `SCH_BUS_WIRE_ENTRY`): all of the stroke is in `extras.strokes`.
            Stroked,
            /// A graphic line on the notes layer: its width is `SchLine::width_nm`, the rest is in `extras.strokes`.
            Notes(usize),
            Junction,
        }
        let mut kinds: Vec<Kind> = Vec::with_capacity(ids.len());
        for id in ids {
            kinds.push(if sch.wires.iter().any(|w| &w.id == id) || sch.bus_entries.iter().any(|b| &b.id == id) {
                Kind::Stroked
            } else if let Some(i) = sch.lines.iter().position(|l| &l.id == id) {
                Kind::Notes(i)
            } else if sch.junctions.iter().any(|j| &j.id == id) {
                Kind::Junction
            } else {
                return Err(fail("ops_unknown_item", id, "no wire, bus, bus entry, graphic line or junction with this id"));
            });
        }
        let color = color.map(specified);
        let mut changed = false;
        for (id, kind) in ids.iter().zip(&kinds) {
            match kind {
                Kind::Stroked | Kind::Notes(_) => {
                    let mut stroke: SchStroke = sch.extras.strokes.get(id).copied().unwrap_or_default();
                    let before = stroke;
                    if let (Some(w), Kind::Stroked) = (width_nm, kind) {
                        // `int width = std::max( 0, m_wireWidth.GetIntValue() )`
                        stroke.width_nm = w.max(Nm::ZERO);
                    }
                    if let Some(s) = style {
                        stroke.style = s;
                    }
                    if let Some(c) = color {
                        stroke.color = c;
                    }
                    if let (Some(w), Kind::Notes(i)) = (width_nm, kind) {
                        let w = w.max(Nm::ZERO);
                        changed |= sch.lines[*i].width_nm != w;
                        sch.lines[*i].width_nm = w;
                    }
                    changed |= stroke != before;
                    if stroke.is_default() {
                        sch.extras.strokes.remove(id);
                    } else {
                        sch.extras.strokes.insert(id.clone(), stroke);
                    }
                }
                Kind::Junction => {
                    let mut look: JunctionLook = sch.extras.junction_looks.get(id).copied().unwrap_or_default();
                    let before = look;
                    if let Some(d) = diameter_nm {
                        look.diameter_nm = d.max(Nm::ZERO);
                    }
                    if let Some(c) = color {
                        look.color = c;
                    }
                    changed |= look != before;
                    if look.is_default() {
                        sch.extras.junction_looks.remove(id);
                    } else {
                        sch.extras.junction_looks.insert(id.clone(), look);
                    }
                }
            }
        }
        if !changed {
            // The entries touched above are exactly what they were (an unchanged value is re-inserted as it was).
            return Err(fail("ops_stroke_unchanged", &ids[0], "the items already have these properties"));
        }
        // Entries of items that are gone (deleted, merged into a neighbour) go with them.
        let live: BTreeSet<&str> = sch.wires.iter().map(|w| w.id.as_str()).chain(sch.bus_entries.iter().map(|b| b.id.as_str())).chain(sch.lines.iter().map(|l| l.id.as_str())).collect();
        sch.extras.strokes.retain(|id, _| live.contains(id.as_str()));
        let junctions: BTreeSet<&str> = sch.junctions.iter().map(|j| j.id.as_str()).collect();
        sch.extras.junction_looks.retain(|id, _| junctions.contains(id.as_str()));
        Ok(())
    }
}
