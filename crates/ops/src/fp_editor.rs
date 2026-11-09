//! Edit Footprint (Ctrl+E) from the board and "Update Footprint on Board" (UI-LEFT F3): the Footprint Editor holds a copy of one board footprint, linked to it, and
//! saving the copy puts it back on that one instance.
//!
//! Ported from `FOOTPRINT_EDIT_FRAME::LoadFootprintFromBoard` (pcbnew/load_select_footprint.cpp at KiCad 8303b2ad) and `FOOTPRINT_EDIT_FRAME::SaveFootprintToBoard`
//! (pcbnew/footprint_libraries_utils.cpp), which `SaveFootprint` calls for a footprint with a link (`GetLink() != niluuid`) and `PCB_CONTROL::SaveFpToBoard` ("Insert
//! footprint into PCB") with `aAddNew`. KiCad clones the board footprint (`SetLink( aFootprint->m_Uuid )`), turns it to the front at orientation 0 and position 0, clears
//! its nets and unlocks its pads; Save exchanges the instance the link names for a clone of the edited footprint (`ExchangeFootprint( sourceFootprint, newFootprint, commit,
//! true )`: pad positions matched, every reset on) and touches no other footprint and no library.
//!
//! Here the copy is a [`eda_model::ir::BoardFootprintCopy`] beside the footprint library (its key is not a footprint name: it is never listed, published or saved into a
//! library), made from the footprint as the board has it -- the pads with their edits and overrides, the drawings, its text boxes, images, points and barcodes
//! (`FootprintExtra::items`, UI-LEFT P30), the Reference with the part's reference, the Value, the user fields, the attributes and the footprint's own clearance, margins
//! and net ties -- in the footprint's own frame (the frame of the board's pads, so nothing moves; the layers of a bottom-side footprint's fields turned to the front,
//! `FOOTPRINT::Flip`). The open is a step of the Footprint Editor (KiCad clears its undo list); the save is one step of the board ("Update Footprint"): the part's
//! footprint becomes the copy's ([`crate::fp_exchange`]'s `exchange_one` with [`ExchangeOptions::replace`]) and what the copy has beyond the engine's footprint is laid
//! on the instance's own edits and record. KiCad's copy is a clone of the board footprint, so what the board file said of it and the copy has no place for (its uuid, sheet,
//! description, tags, Datasheet and Description fields and other kept forms, its pads with no copper and its holes) comes back with it: the record stays, with the
//! copy's items and settings laid on it, and what describes the old footprint's file form only while the copy left it as it was (its courtyard items, groups, pads' facts).

use super::fp_exchange::{board_footprint, footprint_name_of, ExchangeOptions};
use super::{Board, Domain};
use eda_model::ConstraintModel;
use eda_model::fp_art::{flip_layer, is_side_specific};
use eda_model::fp_edit::{default_reference_layout, default_value_layout, FieldLayout, FootprintAttrs, FootprintKind, UserField};
use eda_model::ir::{BoardFootprintCopy, FootprintAttributes, FootprintExtra, FootprintField, FootprintZoneFacts, FootprintZoneOverrides, LibraryFootprint, PadZoneOverride, Side};
use eda_model::fp_settings::FootprintSettings;
use eda_model::CheckResult;
use serde::{Deserialize, Serialize};

fn fail(check: &str, what: &str, msg: impl Into<String>) -> Vec<CheckResult> {
    vec![CheckResult::fail(check, what, msg.into())]
}

/// The Footprint Editor's verbs of UI-LEFT F3, F4 and F8. On the wire `{"op": "fp_editor", "verb": "<verb>", ...}`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "verb", rename_all = "snake_case")]
pub enum FpEditorCmd {
    /// `LoadFootprintFromBoard( aFootprint )`: the footprint `reference` of the board, copied into the Footprint Editor under [`BoardFootprintCopy::key_of`] and linked to
    /// it. A copy made before of any footprint is dropped (the editor holds one footprint; `Clear_Pcb`), so this starts from the board again. Refused for a part that is
    /// not on the board.
    OpenFromBoard { reference: String },
    /// `SaveFootprintToBoard( false )`: the copy keyed `footprint` replaces the board footprint it is linked to, and only that one. "Unable to find the footprint on the main
    /// board.\nCannot save." when the instance is gone. A pad being edited as shapes is recombined first by the caller (`SaveFootprint`), as the copy is the Footprint
    /// Editor's and this is the board's step.
    SaveToBoard { footprint: String },
    /// Footprint Properties' Clearance Overrides, Private Layers, Net Ties and jumper groups (`DIALOG_FOOTPRINT_PROPERTIES_FP_EDITOR::TransferDataFromWindow`), the
    /// whole set: the footprint's [`FootprintSettings`] become `settings`. Refused ("Pad '%s' in jumper pad group %d does not exist in this footprint.") for a jumper
    /// group that names a pad the footprint has not, a negative clearance (`m_netClearance.Validate( 0, INT_MAX )`) and a private layer that is no user layer. Sent with
    /// `edit_footprint_properties` in one batch, the dialog's one undo step.
    SetSettings { footprint: String, settings: FootprintSettings },
    /// `DRAWING_TOOL::PlaceImportedGraphics` in the Footprint Editor (File > Import > Graphics..., `pcbnew.InteractiveDrawing.placeImportedGraphics`, UI-LEFT F8): a DXF or
    /// SVG file as the footprint's lines, arcs, circles, polygons, curves and texts on `import.layer` -- `GRAPHICS_IMPORTER_PCBNEW` with the footprint as the items' parent,
    /// the board's own importer ([`crate::import_graphics`]) -- with "Fix discontinuities" and "Group imported items" (a group of the footprint's). One undo step.
    ImportGraphics { footprint: String, import: crate::ImportGraphics },
    /// `PCB_BASE_FRAME::CreateNewFootprint`'s `SetAttributes( footprintAttrs )` for a footprint New Footprint makes in a library of the library tables: the
    /// attributes of the library's last footprint ("Try to infer the footprint attributes from an existing footprint in the library", `fpnames.back()`), which
    /// the studio's server reads from the library's folder and adds to the `new_footprint` it rewrites (UI-LEFT-2 LL2). One undo step with it.
    SetAttributes { footprint: String, attributes: FootprintAttributes },
}

impl FpEditorCmd {
    /// The open and the settings are the Footprint Editor's, the save the board's.
    pub fn domain(&self) -> Domain {
        match self {
            FpEditorCmd::OpenFromBoard { .. } | FpEditorCmd::SetSettings { .. } | FpEditorCmd::ImportGraphics { .. } | FpEditorCmd::SetAttributes { .. } => Domain::FootprintEditor,
            FpEditorCmd::SaveToBoard { .. } => Domain::Pcb,
        }
    }

    /// What the verb names: the reference, or the footprint's name (a copy's key).
    pub fn subject(&self) -> &str {
        match self {
            FpEditorCmd::OpenFromBoard { reference } => reference,
            FpEditorCmd::SaveToBoard { footprint } | FpEditorCmd::SetSettings { footprint, .. } | FpEditorCmd::ImportGraphics { footprint, .. } | FpEditorCmd::SetAttributes { footprint, .. } => footprint,
        }
    }

    pub fn describe(&self) -> String {
        match self {
            FpEditorCmd::OpenFromBoard { reference } => format!("footprint editor: open {reference} from the board"),
            FpEditorCmd::SaveToBoard { footprint } => format!("update footprint {} on the board", BoardFootprintCopy::reference_of_key(footprint).unwrap_or(footprint)),
            FpEditorCmd::SetSettings { footprint, .. } => format!("footprint {footprint:?}: clearance overrides, private layers, net ties"),
            FpEditorCmd::ImportGraphics { footprint, import } => format!("footprint {footprint:?}: import graphics on {}", import.layer),
            FpEditorCmd::SetAttributes { footprint, .. } => format!("footprint {footprint:?}: the attributes of its library's footprints"),
        }
    }
}

/// A layout of a board footprint's field (absolute layer, the side's mirror) as the front-side copy has it, and back (`FOOTPRINT::Flip` of the fields: the layer to the
/// other side, the mirror toggled on a side-specific layer; the position and angle are already in the footprint's frame).
fn turned(mut l: FieldLayout, side: Side) -> FieldLayout {
    if side == Side::Bottom {
        if is_side_specific(&l.layer) {
            l.mirror = !l.mirror;
        }
        l.layer = flip_layer(&l.layer);
    }
    l
}

/// The layers a footprint may keep private: `LAYERS_GRID_TABLE( m_frame, forbiddenLayers )` forbids every copper and technical layer, `Edge.Cuts` and `Margin` --
/// which leaves the user layers (`Dwgs.User`, `Cmts.User`, `Eco1.User`, `Eco2.User`, `User.1` ...).
pub fn is_user_layer(layer: &str) -> bool {
    matches!(layer, "Dwgs.User" | "Cmts.User" | "Eco1.User" | "Eco2.User") || layer.strip_prefix("User.").is_some_and(|n| n.parse::<u32>().is_ok())
}

/// `FOOTPRINT_ATTR_T` of the board's attributes, as the library footprint keeps them.
fn library_attributes(a: &FootprintAttrs, allow_soldermask_bridges: bool) -> FootprintAttributes {
    FootprintAttributes {
        smd: a.kind == FootprintKind::Smd,
        through_hole: a.kind == FootprintKind::ThroughHole,
        exclude_from_bom: a.exclude_from_bom,
        exclude_from_position_files: a.exclude_from_pos_files,
        board_only: a.board_only,
        dnp: a.dnp,
        allow_missing_courtyard: a.allow_missing_courtyard,
        allow_soldermask_bridges,
    }
}

/// The reverse: the board's attributes of the copy's.
fn board_attrs(a: &FootprintAttributes) -> FootprintAttrs {
    FootprintAttrs {
        kind: if a.through_hole {
            FootprintKind::ThroughHole
        } else if a.smd {
            FootprintKind::Smd
        } else {
            FootprintKind::Unspecified
        },
        board_only: a.board_only,
        exclude_from_pos_files: a.exclude_from_position_files,
        exclude_from_bom: a.exclude_from_bom,
        dnp: a.dnp,
        allow_missing_courtyard: a.allow_missing_courtyard,
    }
}

/// The text of a kept `(property "<name>" "<text>" ..)` (`FootprintExtra::tokens`), unescaped; `None` for another token.
fn kept_property_text(token: &str, name: &str) -> Option<String> {
    let rest = token.strip_prefix(&format!("(property \"{name}\" \""))?;
    let mut out = String::new();
    let mut chars = rest.chars();
    while let Some(c) = chars.next() {
        match c {
            '\\' => out.push(chars.next()?),
            '"' => return Some(out),
            c => out.push(c),
        }
    }
    None
}

/// The text the board shows for the footprint `reference`'s mandatory field `name` (`Datasheet`, `Description`): the footprint's own once Footprint Properties set it
/// (`FootprintEdit::fields`), else what its board file gave it (`FootprintExtra::tokens`: a record of a board file's, or a library's when the symbol gives the field no text), else
/// the symbol's (`updateFootprintParameters`). The state's Footprint Properties rows and Edit Footprint's copy show the same.
pub fn board_footprint_field_text(design: &eda_model::ir::Design, model: &ConstraintModel, reference: &str, name: &str) -> String {
    if let Some(f) = design.footprint_edit(reference).and_then(|e| e.fields.iter().find(|f| f.name == name)) {
        return f.text.clone();
    }
    let linked = model.symbol_links.get(reference).and_then(|l| l.fields.iter().find(|(n, _)| n == name)).map(|(_, t)| t.clone());
    // (a library's record gives a field the symbol gives no text: the writer's `library_field`)
    let linked_text = linked.clone().filter(|t| !t.is_empty());
    let record = design.drawings.as_ref().and_then(|d| d.footprint_extras.iter().find(|e| e.id == reference));
    let kept = record.filter(|e| e.holds_file_form() || linked_text.is_none()).and_then(|e| e.tokens.iter().find_map(|t| kept_property_text(t, name)));
    kept.or(linked).unwrap_or_default()
}

/// The tags of the footprint's own forms Edit Footprint's copy edits beside its settings (`FOOTPRINT::GetLibDescription`, `GetKeywords`, `GetPrivateLayers`,
/// `GetDuplicatePadNumbersAreJumpers`, `JumperPadGroups`): the forms the copy's [`library_forms`] give the record back (UI-LEFT FF4).
const LIBRARY_FORM_TAGS: [&str; 5] = ["descr", "tags", "private_layers", "duplicate_pad_numbers_are_jumpers", "jumper_pad_groups"];

/// The tag of a kept form (`(descr "..")` -> `descr`).
fn form_tag(token: &str) -> Option<&str> {
    token.strip_prefix('(').and_then(|r| r.split(|c: char| c.is_whitespace() || c == ')').next())
}

/// The forms of [`LIBRARY_FORM_TAGS`] of the copy `fp`, as `format( const FOOTPRINT* )` writes them: `(descr ..)`, `(tags ..)`, `(private_layers ..)`,
/// `(duplicate_pad_numbers_are_jumpers yes)` (written when on), `(jumper_pad_groups ..)`.
fn library_forms(fp: &LibraryFootprint) -> Vec<String> {
    let q = eda_kicad::sexpr_str;
    let st = &fp.settings;
    let mut out = Vec::new();
    if !fp.description.is_empty() {
        out.push(format!("(descr {})", q(&fp.description)));
    }
    if !fp.keywords.is_empty() {
        out.push(format!("(tags {})", q(&fp.keywords)));
    }
    if !st.private_layers.is_empty() {
        out.push(format!("(private_layers {})", st.private_layers.iter().map(|l| q(l)).collect::<Vec<_>>().join(" ")));
    }
    if st.duplicate_pad_numbers_are_jumpers {
        out.push("(duplicate_pad_numbers_are_jumpers yes)".to_string());
    }
    if !st.jumper_pad_groups.is_empty() {
        let groups: Vec<String> = st.jumper_pad_groups.iter().map(|g| format!("({})", g.iter().map(|n| q(n)).collect::<Vec<_>>().join(" "))).collect();
        out.push(format!("(jumper_pad_groups {})", groups.join(" ")));
    }
    out
}

/// A board footprint's groups (`FootprintExtra::groups`, each member by its place) as the copy's (`FootprintItems::groups`, by the copy's ids, once the copy's items have
/// theirs): `art:<i>` is the i-th graphic of the copy (`from_engine_footprint` lists the art's shapes in order), `arttext:<i>` its i-th text, `pad:<n>#<k>` the k-th pad
/// numbered `n`, `item:<id>` the item, `group:<i>` the i-th group (`resolveGroups` creates every group before it resolves the members). A member the copy has no place
/// for (a field, a courtyard item kept as text) is left out, as KiCad leaves out of a group a member that is gone, and a group left with none is no group. A locked group's
/// lock is the copy's kept lock of its id.
fn copy_groups(groups: &[eda_model::ir::FootprintGroup], fp: &mut LibraryFootprint) {
    let pad_id = |number: &str, k: usize| fp.pads.iter().filter(|p| p.number == number).nth(k.saturating_sub(1)).map(|p| p.id.clone());
    let mut made: Vec<(eda_model::ir::Group, Vec<usize>)> = Vec::new();
    for g in groups {
        let mut members = Vec::new();
        let mut inner = Vec::new();
        for m in &g.members {
            let Some((kind, rest)) = m.split_once(':') else { continue };
            let id = match kind {
                "art" => rest.parse::<usize>().ok().and_then(|i| fp.graphics.get(i)).map(|s| s.id().to_string()),
                "arttext" => rest.parse::<usize>().ok().and_then(|i| fp.texts.get(i)).map(|t| t.id.clone()),
                "pad" => rest.split_once('#').and_then(|(n, k)| pad_id(n, k.parse().ok()?)),
                "item" => fp.items.has(rest).then(|| rest.to_string()),
                "group" => {
                    inner.extend(rest.parse::<usize>().ok());
                    None
                }
                _ => None,
            };
            members.extend(id);
        }
        made.push((eda_model::ir::Group { id: String::new(), name: g.name.clone(), member_ids: members, design_block: String::new() }, inner));
    }
    // the ids, a group's from its members' (`Group::id_seed`), then the groups a group holds by theirs
    let mut existing: std::collections::BTreeSet<String> = fp.items.ids().map(str::to_string).collect();
    let ids: Vec<String> = made
        .iter()
        .map(|(g, _)| {
            let id = eda_model::ir::next_item_id("grp", &g.id_seed(), &existing);
            existing.insert(id.clone());
            id
        })
        .collect();
    for (i, (g, inner)) in made.iter_mut().enumerate() {
        g.id = ids[i].clone();
        g.member_ids.extend(inner.iter().filter_map(|k| ids.get(*k).filter(|_| *k != i).cloned()));
    }
    for (i, g) in groups.iter().enumerate() {
        if g.locked {
            fp.kicad.locked.insert(ids[i].clone());
        }
    }
    fp.items.groups = made.into_iter().map(|(g, _)| g).filter(|g| !g.member_ids.is_empty()).collect();
}

/// The reverse of [`copy_groups`]: the copy's groups as the board footprint's, each member by its place in the footprint the copy becomes (`new`, the copy's engine
/// footprint: the art's shapes are the copy's graphics on the art layers, in order). A member that has no place there is left out, and a group left with none is no group.
fn board_groups(fp: &LibraryFootprint) -> Vec<eda_model::ir::FootprintGroup> {
    let art_ids: Vec<&str> = fp.graphics.iter().filter(|g| eda_model::fp_art::is_art_layer(g.layer())).map(|g| g.id()).collect();
    let index_of_group: std::collections::BTreeMap<&str, usize> = fp.items.groups.iter().enumerate().map(|(i, g)| (g.id.as_str(), i)).collect();
    let place = |id: &str| -> Option<String> {
        if let Some(i) = art_ids.iter().position(|a| *a == id) {
            return Some(format!("art:{i}"));
        }
        if let Some(i) = fp.texts.iter().position(|t| t.id == id) {
            return Some(format!("arttext:{i}"));
        }
        if let Some(i) = fp.pads.iter().position(|p| p.id == id) {
            let number = &fp.pads[i].number;
            let k = fp.pads[..=i].iter().filter(|p| &p.number == number).count();
            return Some(format!("pad:{number}#{k}"));
        }
        if let Some(&g) = index_of_group.get(id) {
            return Some(format!("group:{g}"));
        }
        eda_model::fp_items_place::placed_kinds(&fp.items).has(id).then(|| format!("item:{id}"))
    };
    fp.items
        .groups
        .iter()
        .map(|g| eda_model::ir::FootprintGroup { name: g.name.clone(), locked: fp.kicad.locked.contains(&g.id), members: g.member_ids.iter().filter_map(|m| place(m)).collect() })
        .collect()
}

impl Board<'_> {
    pub(crate) fn apply_fp_editor(&mut self, cmd: &FpEditorCmd) -> Result<(), Vec<CheckResult>> {
        match cmd {
            FpEditorCmd::OpenFromBoard { reference } => self.open_footprint_from_board(reference),
            FpEditorCmd::SaveToBoard { footprint } => self.save_footprint_to_board(footprint),
            FpEditorCmd::SetSettings { footprint, settings } => self.set_footprint_settings(footprint, settings),
            FpEditorCmd::ImportGraphics { footprint, import } => self.import_footprint_graphics(footprint, import),
            FpEditorCmd::SetAttributes { footprint, attributes } => {
                let fp = self.design.footprint_library.as_mut().and_then(|l| l.by_name_mut(footprint)).ok_or_else(|| vec![CheckResult::fail("ops_unknown_footprint", footprint, "this footprint has not been opened in the Footprint Editor yet")])?;
                fp.attributes = attributes.clone();
                Ok(())
            }
        }
    }

    /// See [`FpEditorCmd::ImportGraphics`]: the board's `import_graphics` with the footprint for the board (`GRAPHICS_IMPORTER_PCBNEW::m_parent`).
    fn import_footprint_graphics(&mut self, name: &str, req: &crate::ImportGraphics) -> Result<(), Vec<CheckResult>> {
        if req.layer.is_empty() {
            return Err(fail("ops_bad_import", "import graphics", "Please select a valid layer."));
        }
        if !(req.scale.is_finite() && req.scale != 0.0) {
            return Err(fail("ops_bad_import", "import graphics", "the import scale must be a number other than zero"));
        }
        self.library_footprint_mut(name)?;
        let imported = req.read().map_err(|e| fail("ops_bad_import", "import graphics", e))?;
        let mut mapper = req.mapper();
        let mut items = crate::graphics_board_items(&imported, &mut mapper, &req.layer);
        if items.shapes.is_empty() && items.texts.is_empty() {
            let mut why = String::from("No graphic items found in file.");
            for m in items.messages.iter().chain(imported.messages.iter()) {
                why.push(' ');
                why.push_str(m);
            }
            return Err(fail("ops_empty_import", "import graphics", why));
        }
        if let Some(tol) = req.fix_tolerance_nm {
            crate::fix_shapes::connect_board_shapes(&mut items.shapes, tol.max(eda_model::ir::Nm::ZERO));
        }
        // a library footprint's text keeps no vertical justification: stood where the justified one stands, centred (`crate::import_graphics::centre_vertically`)
        for (i, extra) in &items.text_extras {
            if let Some(t) = items.texts.get_mut(*i) {
                crate::import_graphics::centre_vertically(t, extra.valign);
            }
        }
        let fp = self.library_footprint_mut(name)?;
        let before: std::collections::BTreeSet<String> = fp.graphics.iter().map(|g| g.id().to_string()).chain(fp.texts.iter().map(|t| t.id.clone())).collect();
        for mut s in items.shapes {
            s.set_id(String::new());
            fp.graphics.push(s);
        }
        for mut t in items.texts {
            t.id.clear();
            fp.texts.push(t);
        }
        fp.assign_missing_ids();
        if req.group {
            // "Group imported items": `PCB_GROUP` of the footprint over every item made (`GROUP_TOOL`'s group of one item is no group: KiCad still makes it)
            let made: Vec<String> = fp.graphics.iter().map(|g| g.id().to_string()).chain(fp.texts.iter().map(|t| t.id.clone())).filter(|id| !before.contains(id)).collect();
            if made.len() >= 2 {
                eda_model::groups::GroupHost::make_group(fp, &made).map_err(|r| fail(r.check, &r.subject, r.why))?;
            }
        }
        Ok(())
    }

    /// See [`FpEditorCmd::SetSettings`].
    fn set_footprint_settings(&mut self, name: &str, settings: &FootprintSettings) -> Result<(), Vec<CheckResult>> {
        if settings.clearance.is_some_and(|c| c < eda_model::ir::Nm::ZERO) {
            return Err(fail("ops_bad_clearance", name, "Pad clearance must be at least 0 mm."));
        }
        if let Some(l) = settings.private_layers.iter().find(|l| !is_user_layer(l)) {
            return Err(fail("ops_bad_layer", l, format!("{l} cannot be a private layer: only the user layers can")));
        }
        let fp = self.library_footprint_mut(name)?;
        let pads: std::collections::BTreeSet<String> = fp.pads.iter().map(|p| p.number.clone()).collect();
        // the groups are checked as the dialog's rows (`wxStringTokenizer`), and kept as KiCad's sets
        let rows: Vec<String> = settings.jumper_pad_groups.iter().map(|g| g.join(", ")).collect();
        let jumper_pad_groups = FootprintSettings::jumper_groups_of_rows(&rows, &pads).map_err(|m| fail("ops_bad_jumper_group", name, m))?;
        // `AddNetTiePadGroup` only for a row that is not empty
        let net_tie_pad_groups = settings.net_tie_pad_groups.iter().filter(|g| !g.is_empty()).cloned().collect();
        // an older design's kept forms of the same settings are replaced (the writer writes both)
        let mut kept = std::mem::take(&mut fp.kicad.tokens);
        FootprintSettings::default().absorb_kept(&mut kept);
        fp.kicad.tokens = kept;
        fp.settings = FootprintSettings { jumper_pad_groups, net_tie_pad_groups, ..settings.clone() };
        Ok(())
    }

    /// See [`FpEditorCmd::Open`].
    pub(crate) fn open_footprint_from_board(&mut self, reference: &str) -> Result<(), Vec<CheckResult>> {
        let pose = self.pose_of(reference).cloned().ok_or_else(|| fail("ops_unknown_part", reference, "no footprint with this reference is on the board"))?;
        let part = self.model.part(reference).cloned().ok_or_else(|| fail("ops_unknown_part", reference, "no footprint with this reference is on the board"))?;
        let bf = board_footprint(&self.design, self.model, reference).ok_or_else(|| fail("ops_unknown_footprint", reference, format!("{reference} has no footprint to edit")))?;
        let fpid = footprint_name_of(&part);
        let key = BoardFootprintCopy::key_of(reference);

        // the pads, drawings, courtyard and models as the board has them (`aFootprint->Clone()`)
        let mut fp = LibraryFootprint::from_engine_footprint(&bf.footprint);
        fp.name = key.clone();
        let edit = self.design.footprint_edit(reference).cloned();
        // the Reference shows the part's reference (the clone keeps it), the Value the part's value
        let mut reference_layout = turned(edit.as_ref().and_then(|e| e.reference.clone()).unwrap_or_else(|| default_reference_layout(&pose, &bf.footprint)), pose.side);
        reference_layout.text = Some(reference.to_string());
        fp.reference_visible = reference_layout.visible;
        fp.reference = Some(reference_layout);
        let mut value_layout = turned(edit.as_ref().and_then(|e| e.value.clone()).unwrap_or_else(|| default_value_layout(&pose)), pose.side);
        // (a part with no value shows the footprint's name, as a footprint's Value does when the netlist gives it none)
        value_layout.text = Some(part.value.clone().filter(|v| !v.is_empty()).unwrap_or_else(|| fpid.rsplit(':').next().unwrap_or(&fpid).to_string()));
        fp.value_visible = value_layout.visible;
        fp.value = Some(value_layout);
        fp.fields = edit
            .as_ref()
            .map(|e| e.fields.iter().map(|f| FootprintField { name: f.name.clone(), value: f.text.clone(), visible: f.layout.visible, layout: Some(turned(f.layout.clone(), pose.side)) }).collect())
            .unwrap_or_default();
        // the attributes and the footprint's own overrides
        let extra = self.design.drawings.as_ref().and_then(|d| d.footprint_extras.iter().find(|e| e.id == reference)).cloned();
        fp.attributes = library_attributes(&bf.attrs, extra.as_ref().is_some_and(|e| e.allow_soldermask_bridges));
        fp.zone_connection = bf.overrides.zone_connection;
        fp.settings.clearance = bf.overrides.clearance;
        fp.settings.solder_mask_margin = bf.overrides.solder_mask_margin;
        fp.settings.solder_paste_margin = bf.overrides.solder_paste_margin;
        fp.settings.solder_paste_margin_ratio = bf.overrides.solder_paste_margin_ratio;
        fp.settings.net_tie_pad_groups = extra.as_ref().map(|e| e.net_tie_pad_groups.clone()).unwrap_or_default();
        // each pad's overrides (`PadOverrides`, by number in pad order)
        let mut nth: std::collections::BTreeMap<&str, usize> = std::collections::BTreeMap::new();
        for pad in fp.pads.iter_mut() {
            let k = nth.entry(pad.number.as_str()).or_insert(0);
            let o = bf.pad_overrides.iter().filter(|o| o.number == pad.number).nth(*k);
            *k += 1;
            if let Some(o) = o {
                pad.clearance_override = o.clearance;
                pad.zone_connection = o.zone_connection;
                pad.thermal_gap_override = o.thermal_gap;
                pad.thermal_spoke_width_override = o.thermal_spoke_width;
                pad.thermal_spoke_angle_mdeg = o.thermal_spoke_angle_mdeg;
                pad.kicad.solder_mask_margin = o.solder_mask_margin;
                pad.kicad.solder_paste_margin = o.solder_paste_margin;
                pad.kicad.solder_paste_margin_ratio = o.solder_paste_margin_ratio;
                pad.kicad.property = o.fabrication_property.clone();
            }
        }
        // its text boxes, images, points, barcodes, dimensions and tables (the clone holds every child of the footprint), their ids and locks kept: the board's are the
        // copy's; and its zones, taken off the board to its frame (UI-LEFT PP14, FF4)
        fp.items = eda_model::fp_items_place::items_of(&self.design, self.model, &part).map(eda_model::fp_items_place::placed_kinds).unwrap_or_default();
        fp.items.zones = self.footprint_zones_in_frame(reference);
        if let Some(e) = &extra {
            fp.kicad.locked.extend(e.locked_items.iter().filter(|id| fp.items.has(id)).cloned());
        }
        // the description, keywords, private layers and jumpers its record's kept forms hold (`aFootprint->Clone()` keeps them), while they are the footprint's
        let written = eda_model::fp_items_place::written_name(self.model, &part);
        if let Some(e) = extra.as_ref().filter(|e| e.source_name == written) {
            let facts = eda_kicad::footprint_file_facts(&e.tokens);
            fp.description = facts.description.unwrap_or_default();
            fp.keywords = facts.keywords.unwrap_or_default();
            let mut forms: Vec<String> = e.tokens.iter().filter(|t| form_tag(t).is_some_and(|t| LIBRARY_FORM_TAGS.contains(&t))).cloned().collect();
            let mut st = FootprintSettings::default();
            st.absorb_kept(&mut forms);
            fp.settings.private_layers = st.private_layers;
            fp.settings.duplicate_pad_numbers_are_jumpers = st.duplicate_pad_numbers_are_jumpers;
            fp.settings.jumper_pad_groups = st.jumper_pad_groups;
        }
        // the Datasheet and Description fields as the board shows them (the clone's)
        for name in ["Datasheet", "Description"] {
            let mut l = eda_model::fp_edit::default_library_datasheet_layout();
            if let Some(f) = edit.as_ref().and_then(|e| e.fields.iter().find(|f| f.name == name)) {
                l = turned(f.layout.clone(), pose.side);
            }
            l.text = Some(board_footprint_field_text(&self.design, self.model, reference, name));
            if name == "Datasheet" {
                fp.datasheet = Some(l);
            } else {
                fp.description_field = Some(l);
            }
        }
        fp.assign_missing_ids();
        // its groups, by the copy's ids
        if let Some(e) = extra.as_ref().filter(|e| !e.groups.is_empty() && e.source_name == written) {
            copy_groups(&e.groups, &mut fp);
        }
        let lib = self.footprint_library_mut();
        // the editor holds one footprint: a copy of another board footprint is gone (`Clear_Pcb`)
        lib.board_copies.clear();
        lib.board_copies.push(BoardFootprintCopy { reference: reference.to_string(), fpid, footprint: fp });
        Ok(())
    }

    /// See [`FpEditorCmd::Save`].
    fn save_footprint_to_board(&mut self, key: &str) -> Result<(), Vec<CheckResult>> {
        let copy = self.design.footprint_library.as_ref().and_then(|l| l.board_copy(key)).cloned().ok_or_else(|| fail("ops_unknown_footprint", key, "this footprint was not loaded from the board"))?;
        let reference = copy.reference.clone();
        // `if( !aAddNew && sourceFootprint == nullptr )`: the footprint the copy is linked to was deleted from the board since
        let (Some(pose), Some(part)) = (self.pose_of(&reference).cloned(), self.model.part(&reference).cloned()) else {
            return Err(fail("ops_unknown_part", &reference, "Unable to find the footprint on the main board.\nCannot save."));
        };
        if copy.footprint.pad_edit.is_some() {
            return Err(fail("ops_pad_edit", key, "a pad is being edited as graphic shapes: finish the pad edit first"));
        }
        // the name the copy came from; a built-in package's is the installed footprint the board names (`Resistor_SMD:R_0603_1608Metric` for a `0603`: the clone's
        // `GetFPID()`), so that the part is not left on a footprint named `0603`, which no library has
        let fpid = if part.footprint.is_none() && part.package.as_deref() == Some(copy.fpid.as_str()) { eda_model::footprint::footprint_lib_id(&part) } else { copy.fpid.clone() };
        let mut new = copy.footprint.to_engine_footprint();
        new.name = fpid.clone();
        let old = self.model.footprint_of(&part).unwrap_or_default();
        // the board file's record of the footprint as it stands, and whether its file form is the footprint's (the writer writes its tokens)
        let old_record = self.design.drawings.as_ref().and_then(|d| d.footprint_extras.iter().find(|e| e.id == reference)).cloned();
        let written = eda_model::fp_items_place::written_name(self.model, &part);
        let live = old_record.as_ref().is_some_and(|e| e.holds_file_form() && e.source_name == written);
        let items = eda_model::fp_items_place::placed_kinds(&copy.footprint.items);
        // (the footprint's zones as they are, so that a zone the copy left where it was keeps its uuid)
        let old_zones: Vec<eda_model::ir::Zone> = self.design.routing.iter().flat_map(|r| r.zones.iter()).filter(|z| z.parent_footprint.as_deref() == Some(reference.as_str())).cloned().collect();
        // the Datasheet and Description the board shows, before the exchange: a copy whose own differ was edited
        let shown_fields: Vec<(&str, String)> = ["Datasheet", "Description"].into_iter().map(|n| (n, board_footprint_field_text(&self.design, self.model, &reference, n))).collect();
        // `ExchangeFootprint( sourceFootprint, newFootprint, commit, true )`: pad positions matched, every reset on, the pads matched as the exchange matches them
        // (`matchItemsBySimilarity<PAD>`, not the netlist's by number); the part keeps its symbol's footprint field
        let options = ExchangeOptions { pads_by_number: false, ..ExchangeOptions::replace() };
        self.exchange_one(&reference, &fpid, &old, new.clone(), None, &options, false);
        // (a board with no routing section is not given one: `fp_on_board::set_footprint_zones`)
        if let Some(rt) = self.design.routing.as_mut() {
            rt.zones.retain(|z| z.parent_footprint.as_deref() != Some(reference.as_str()));
            rt.zones.extend(old_zones);
        }

        // What the copy has beyond the engine's footprint, on the instance: its fields' layouts and user fields, its attributes, its overrides (the clone carries them all)
        let fp = &copy.footprint;
        let side = pose.side;
        let mut edit = self.design.footprint_edit(&reference).cloned().unwrap_or_else(|| eda_model::fp_edit::FootprintEdit::new(&reference));
        let mut reference_layout = turned(fp.reference.clone().unwrap_or_else(eda_model::fp_edit::default_library_reference_layout), side);
        // "Copy reference. The initial text is always used, never resetted"
        reference_layout.text = None;
        reference_layout.visible = fp.reference_visible;
        edit.reference = Some(reference_layout);
        let mut value_layout = turned(fp.value.clone().unwrap_or_else(eda_model::fp_edit::default_library_value_layout), side);
        // the value's text is the part's (reset only when it stood for the footprint's name)
        value_layout.text = None;
        value_layout.visible = fp.value_visible;
        edit.value = Some(value_layout);
        let own_mandatory: Vec<UserField> = edit.fields.iter().filter(|f| f.name == "Datasheet" || f.name == "Description").cloned().collect();
        edit.fields = fp
            .fields
            .iter()
            .map(|f| {
                let mut layout = turned(f.layout.clone().unwrap_or_else(eda_model::fp_edit::new_library_field_layout), side);
                layout.visible = f.visible;
                UserField { name: f.name.clone(), text: f.value.clone(), layout }
            })
            .collect();
        // the Datasheet and Description: the footprint's own (Footprint Properties') as they were, and the copy's where it edited them (UI-LEFT FF4)
        for (name, shown) in &shown_fields {
            let copied = if *name == "Datasheet" { fp.datasheet.as_ref() } else { fp.description_field.as_ref() };
            let own = own_mandatory.iter().find(|f| f.name == *name).cloned();
            let field = match copied.filter(|l| l.text.clone().unwrap_or_default() != *shown) {
                Some(l) => {
                    let mut layout = turned(l.clone(), side);
                    let text = layout.text.take().unwrap_or_default();
                    Some(UserField { name: name.to_string(), text, layout })
                }
                None => own,
            };
            if let Some(f) = field {
                edit.fields.retain(|g| g.name != f.name);
                edit.fields.push(f);
            }
        }
        let attrs = board_attrs(&fp.attributes);
        let (dnp, no_bom) = self.design.symbol_bom_flags(&reference);
        edit.attrs = (attrs != FootprintAttrs::derived(&copy.footprint.to_engine_footprint(), dnp, no_bom)).then_some(attrs);
        let bom_changed = (attrs.dnp, attrs.exclude_from_bom) != (dnp, no_bom);
        // the pads' margins, as edits of the pads (`PadEdit`); their clearance and zone settings below
        let mut nth: std::collections::BTreeMap<&str, u32> = std::collections::BTreeMap::new();
        for pad in &fp.pads {
            let k = nth.entry(pad.number.as_str()).or_insert(0);
            *k += 1;
            let mut pe = edit.pad(&pad.number, *k).cloned().unwrap_or_else(|| eda_model::fp_edit::PadEdit { number: pad.number.clone(), nth: *k, ..Default::default() });
            pe.solder_mask_margin = pad.kicad.solder_mask_margin;
            pe.solder_paste_margin = pad.kicad.solder_paste_margin;
            pe.solder_paste_margin_ratio = pad.kicad.solder_paste_margin_ratio;
            edit.set_pad(pe);
        }
        let footprint_facts = FootprintZoneFacts { zone_connection: fp.zone_connection, clearance: fp.settings.clearance };
        // the paste margins: the file's forms of a footprint whose record holds them, else its edit (UI-LEFT P14)
        if !live {
            edit.solder_paste_margin = fp.settings.solder_paste_margin;
            edit.solder_paste_ratio = fp.settings.solder_paste_margin_ratio;
        }
        let pads: Vec<PadZoneOverride> = fp
            .pads
            .iter()
            .map(|p| PadZoneOverride {
                number: p.number.clone(),
                clearance: p.clearance_override,
                zone_connection: p.zone_connection,
                thermal_gap: p.thermal_gap_override,
                thermal_spoke_width: p.thermal_spoke_width_override,
                thermal_spoke_angle_mdeg: p.thermal_spoke_angle_mdeg,
            })
            .filter(|o| *o != PadZoneOverride { number: o.number.clone(), ..Default::default() })
            .collect();
        let edits = [edit.clone()];
        let dr = self.drawings_mut();
        dr.footprint_edits.retain(|e| e.id != reference);
        if !edit.is_empty() {
            dr.footprint_edits.push(edit);
            dr.footprint_edits.sort_by(|a, b| a.id.cmp(&b.id));
        }
        dr.zone_overrides.retain(|z| z.id != reference);
        if footprint_facts != FootprintZoneFacts::default() || !pads.is_empty() {
            dr.zone_overrides.push(FootprintZoneOverrides { id: reference.clone(), footprint: (footprint_facts != FootprintZoneFacts::default()).then_some(footprint_facts), pads });
            dr.zone_overrides.sort_by(|a, b| a.id.cmp(&b.id));
        }
        // The record: the clone is the board footprint, so what its board file said of it comes back -- the record as it was (a footprint made here gets one in the
        // library's frame, as the writer holds every footprint made here) -- with the copy's items and footprint settings laid on it, and what describes the old
        // footprint's file form only where the copy left that as it was.
        let mut extra = old_record.clone().unwrap_or_else(|| FootprintExtra { id: reference.clone(), library_frame: true, ..Default::default() });
        extra.items = items;
        // the copy's items' locks (UI-LEFT PP14); its zones go on the board below
        extra.locked_items = copy.footprint.kicad.locked.iter().filter(|id| extra.items.has(id)).cloned().collect();
        extra.zones_pose = None;
        // the courtyard as the file drew it, while the copy's courtyard is the board's
        if new.courtyard != old.courtyard {
            extra.courtyard_items.clear();
            extra.courtyard_bottom = false;
            extra.no_courtyard = false;
        }
        // each pad's file facts (`PadMaskInfo`, by place in the footprint) while the pads are the same in the same order, the groups (members named by place) while the
        // graphics, texts and pads are
        let numbers = |f: &eda_model::Footprint| f.pads.iter().map(|p| p.number.clone()).collect::<Vec<_>>();
        let same_pads = numbers(&new) == numbers(&old);
        if !same_pads {
            extra.pads.clear();
        }
        // (the board's art as the copy holds it, so a copy nothing was drawn in compares equal); the copy's own groups, each member by its place, when it has any (the copy
        // shows the board's: UI-LEFT FF4)
        let old_art = LibraryFootprint::from_engine_footprint(&old).to_engine_footprint().art;
        if !fp.items.groups.is_empty() {
            extra.groups = board_groups(fp).into_iter().filter(|g| !g.members.is_empty()).collect();
        } else if !same_pads || new.art != old_art || extra.source_name != written {
            extra.groups.clear();
        }
        // the footprint's mask margin, solder mask bridges, net ties, clearance and zone connection (a footprint with no pad writes its own) are the board file's facts
        extra.solder_mask_margin = fp.settings.solder_mask_margin;
        extra.allow_soldermask_bridges = fp.attributes.allow_soldermask_bridges;
        extra.net_tie_pad_groups = fp.settings.net_tie_pad_groups.clone();
        if old_record.is_some() || new.pads.is_empty() {
            extra.clearance = fp.settings.clearance;
            extra.zone_connection = fp.zone_connection;
        }
        if live {
            crate::pcb_props::set_paste_margin_tokens(&mut extra.tokens, fp.settings.solder_paste_margin, fp.settings.solder_paste_margin_ratio);
        }
        // the copy's description, keywords, private layers and jumper settings (UI-LEFT FF4), in place of the record's own forms of them, while the record's forms are the
        // footprint's (`source_name`): a record of a footprint made here becomes a library's (`from_library`, its fields still the symbol's and the edit's)
        let forms = library_forms(fp);
        if extra.source_name.is_empty() && (!forms.is_empty() || !extra.groups.is_empty()) {
            extra.source_name = written.clone();
            extra.from_library = true;
        }
        if extra.source_name == written {
            extra.tokens.retain(|t| !form_tag(t).is_some_and(|t| LIBRARY_FORM_TAGS.contains(&t)));
            for (i, f) in forms.into_iter().enumerate() {
                extra.tokens.insert(i.min(extra.tokens.len()), f);
            }
        }
        dr.footprint_extras.retain(|e| e.id != reference);
        // (a record that says the footprint is in its file's frame is not empty: `FootprintExtra::library_frame`)
        if extra != (FootprintExtra { id: reference.clone(), library_frame: true, ..Default::default() }) {
            dr.footprint_extras.push(extra);
            dr.footprint_extras.sort_by(|a, b| a.id.cmp(&b.id));
        }
        // the copy's zones are the footprint's, put where it is (UI-LEFT PP14, FF4)
        self.set_footprint_zones(&reference, copy.footprint.items.zones.clone());
        // a DNP or BOM flag the copy changed reaches the symbol, as an edit of Footprint Properties on the board does
        if bom_changed {
            self.design.sync_symbol_bom_flags(&edits);
        }
        Ok(())
    }
}

/// What `FOOTPRINT::MoveAnchorPosition` moves that [`fp_items::move_items_with_anchor`](super::fp_items) does not (UI-LEFT F7): the items the editor holds as the text of
/// their forms (a locked or filled zone, a group's dimension ...: [`eda_model::lib_keep::translate_kept_form`]) and the courtyard. KiCad's courtyard is the `F.CrtYd` /
/// `B.CrtYd` graphics, which move with the rest, and its cache (`m_courtyard_cache->front.Move( moveVector )`); the footprint's `courtyard` here is the smallest box centred on
/// the origin that holds them (the reader's `courtyard_from`), so it is that box of the moved graphics -- or, for a footprint whose courtyard has no graphics, the smallest
/// box centred on the origin that holds the old box moved.
pub(crate) fn move_kept_with_anchor(fp: &mut LibraryFootprint, dx: eda_model::ir::Nm, dy: eda_model::ir::Nm) {
    for t in fp.kicad.tokens.iter_mut() {
        if eda_model::lib_keep::kept_form_moves_with_anchor(t) {
            *t = eda_model::lib_keep::translate_kept_form(t, dx, dy);
        }
    }
    if let Some((hw, hh)) = fp.courtyard {
        fp.courtyard = Some(courtyard_of_graphics(fp).unwrap_or((hw + dx.abs(), hh + dy.abs())));
    }
}

/// The reader's `courtyard_from` over the footprint's graphics: the symmetric half extents of the box of every line, rectangle, circle and polygon on `F.CrtYd` or
/// `B.CrtYd`; `None` without one.
fn courtyard_of_graphics(fp: &LibraryFootprint) -> Option<(eda_model::ir::Nm, eda_model::ir::Nm)> {
    use eda_model::ir::{Nm, Shape};
    let mut ext: Option<(Nm, Nm)> = None;
    let mut grow = |x: Nm, y: Nm| {
        let (w, h) = ext.unwrap_or((Nm::ZERO, Nm::ZERO));
        ext = Some((w.max(x.abs()), h.max(y.abs())));
    };
    for g in fp.graphics.iter().filter(|g| matches!(g.layer(), "F.CrtYd" | "B.CrtYd")) {
        match g {
            Shape::Segment { start, end, .. } | Shape::Rect { start, end, .. } => {
                grow(start.x, start.y);
                grow(end.x, end.y);
            }
            Shape::Circle { center, end, .. } => {
                let r = Nm::kiround(((end.x - center.x).get() as f64).hypot((end.y - center.y).get() as f64));
                grow(center.x - r, center.y - r);
                grow(center.x + r, center.y + r);
            }
            Shape::Polygon { pts, .. } => pts.iter().for_each(|p| grow(p.x, p.y)),
            _ => {}
        }
    }
    ext
}
