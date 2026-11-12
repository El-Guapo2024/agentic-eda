//! Board Setup's verbs (`pcbnew/dialogs/dialog_board_setup.cpp` and its pages): each one stores a page of
//! the dialog in `design.drawings.rules` ([`eda_model::rules::RulesOverlay`]), and the loader lays the
//! overlay over the intent's rules (`crates/cli/src/board.rs::load`). Same shape as
//! `Cmd::SetTeardropSettings`/`SetDimensionSettings`: whole page in, whole page replaced, one undo step
//! (the design snapshot the CLI keeps takes the overlay back with everything else).
//!
//! Every verb validates like the page it comes from -- `BOARD_DESIGN_SETTINGS::ValidateDesignRules` and
//! the panels' own checks -- and refuses with the setting's name in `location`, so the dialog can put the
//! message next to the field.

use std::collections::BTreeMap;

use eda_model::component_classes::ComponentClassSettings;
use eda_model::ir::Design;
use eda_model::layers::LayerSetup;
use eda_model::rules::{self, Constraints, MaskPaste, NetClassSettings, RulesOverlay, StackupSettings, TextGraphicsDefaults};
use eda_model::setup_pages::{self, FormattingSettings, HatchOffsets, TuningPatterns, TuningProfiles};
use eda_model::via_extra::ViaKind;
use eda_model::{CheckResult, ConstraintModel};

type Done = Result<(), Vec<CheckResult>>;

fn overlay(design: &mut Design) -> &mut RulesOverlay {
    design.drawings.get_or_insert_with(Default::default).rules.get_or_insert_with(Default::default)
}

/// The first `(setting, message)` a page's range check found, as a refusal.
fn first(check: &str, problems: Vec<(&'static str, String)>) -> Done {
    match problems.into_iter().next() {
        Some((setting, message)) => Err(vec![CheckResult::fail(check, setting, message)]),
        None => Ok(()),
    }
}

/// `Cmd::SetNetClasses`. Each class's patterns are kept as `NET_SETTINGS::SetNetclassPatternAssignment` keeps them: a bus pattern (`D[0..7]`, `{SDA SCL}`)
/// is its members, one pattern each, an exact repeat once (`eda_model::bus_pattern::expand_patterns`).
pub(crate) fn set_net_classes(design: &mut Design, settings: &NetClassSettings) -> Done {
    rules::validate_net_classes(settings).map_err(|c| vec![c])?;
    let mut settings = settings.clone();
    for c in &mut settings.classes {
        c.nets = eda_model::bus_pattern::expand_patterns(&c.nets);
    }
    overlay(design).net_classes = Some(settings);
    Ok(())
}

/// `Cmd::SetConstraints`.
pub(crate) fn set_constraints(design: &mut Design, constraints: &Constraints) -> Done {
    first("ops_bad_constraints", constraints.validate())?;
    overlay(design).constraints = Some(constraints.clone());
    Ok(())
}

/// `Cmd::SetMaskPaste`: `PANEL_SETUP_MASK_AND_PASTE::TransferDataFromWindow` stores the page and returns true -- it has no checks of its own.
pub(crate) fn set_mask_paste(design: &mut Design, settings: &MaskPaste) -> Done {
    overlay(design).mask_paste = Some(settings.clone());
    Ok(())
}

/// `Cmd::SetTextGraphicsDefaults`.
pub(crate) fn set_text_graphics(design: &mut Design, settings: &TextGraphicsDefaults) -> Done {
    first("ops_bad_text_defaults", settings.validate())?;
    overlay(design).text_graphics = Some(*settings);
    Ok(())
}

/// The items a layer holds that would be left on a layer the board no longer has: board-level graphics, texts and dimensions, and the footprint fields whose
/// layout was set on it. (A footprint's Reference and Value where they have always been, on the silkscreen and fabrication layers, are not counted: KiCad
/// asks about them and goes on, and every board with a footprint would be refused.)
fn items_on_layer(design: &Design, layer: &str) -> usize {
    let Some(d) = design.drawings.as_ref() else { return 0 };
    d.shapes.iter().filter(|s| s.layer() == layer).count()
        + d.texts.iter().filter(|t| t.layer == layer).count()
        + d.dimensions.iter().filter(|x| x.layer == layer).count()
        + d.footprint_edits.iter().map(|e| e.reference.iter().chain(e.value.iter()).filter(|l| l.layer == layer).count() + e.fields.iter().filter(|f| f.layout.layer == layer).count()).sum::<usize>()
}

/// `Cmd::SetBoardLayers`: Board Editor Layers (`PANEL_SETUP_LAYERS::TransferDataFromWindow`). The page is checked like the panel checks it (the names, the mandatory
/// layers, the types); a layer turned off while items are still on it is refused with the layer's name and how many (KiCad deletes them after asking, which one
/// Undo of a settings dialog cannot be trusted to bring back: move or delete them first).
pub(crate) fn set_board_layers(design: &mut Design, model: &ConstraintModel, settings: &LayerSetup) -> Done {
    settings.validate(&model.board.layers).map_err(|c| vec![c])?;
    let next = settings.canonical();
    for layer in model.board.layer_setup.enabled_in_order() {
        if next.enabled.contains(&layer) {
            continue;
        }
        let n = items_on_layer(design, &layer);
        if n > 0 {
            let shown = model.board.layer_setup.display_name(&layer);
            return Err(vec![CheckResult::fail("ops_bad_board_layers", &layer, format!("{n} item(s) are on {shown}, which the board would no longer have; move or delete them first"))]);
        }
    }
    overlay(design).board_layers = Some(next);
    Ok(())
}

/// `Cmd::SetFormatting`: Text & Graphics > Formatting.
pub(crate) fn set_formatting(design: &mut Design, settings: &FormattingSettings) -> Done {
    settings.validate().map_err(|c| vec![c])?;
    overlay(design).formatting = Some(settings.clone());
    Ok(())
}

/// `Cmd::SetZoneHatchOffsets`: Board Stackup > Zone Hatch Offsets. The page is the layers that have an offset; a layer set to 0, 0 has none.
pub(crate) fn set_zone_hatch_offsets(design: &mut Design, model: &ConstraintModel, offsets: &HatchOffsets) -> Done {
    setup_pages::validate_hatch_offsets(offsets, &model.board.layers).map_err(|c| vec![c])?;
    overlay(design).zone_hatch_offsets = Some(setup_pages::canonical_hatch_offsets(offsets));
    Ok(())
}

/// `Cmd::SetTuningPatterns`: Design Rules > Length-tuning Patterns.
pub(crate) fn set_tuning_patterns(design: &mut Design, settings: &TuningPatterns) -> Done {
    settings.validate().map_err(|c| vec![c])?;
    overlay(design).tuning_patterns = Some(settings.clone());
    Ok(())
}

/// `Cmd::SetTuningProfiles`: Design Rules > Tuning Profiles, and the profile each net class names.
pub(crate) fn set_tuning_profiles(design: &mut Design, model: &ConstraintModel, settings: &TuningProfiles) -> Done {
    settings.validate(&model.board.layers).map_err(|c| vec![c])?;
    overlay(design).tuning_profiles = Some(settings.canonical());
    Ok(())
}

/// `Cmd::SetStackup`: the copper layer count and the layers, with their sublayers (Board Stackup > Physical Stackup, `PANEL_SETUP_BOARD_STACKUP::TransferDataFromWindow`).
///
/// Fewer copper layers than the board has, while something is still on a layer that would go ([`copper_layer_items`], `BOARD::HasItemsOnLayer`), is refused with the
/// layer's name and how many -- unless `delete_items` says to delete them, which is what KiCad does once its question ("Items have been found on removed layers. This
/// operation will delete all items from removed layers and cannot be undone.", `PANEL_SETUP_LAYERS::TransferDataFromWindow`) is answered OK: every layer that goes is
/// emptied as `BOARD::RemoveAllItemsOnLayer` empties it ([`remove_all_items_on_layer`]), in the same step as the new stackup. The page asks KiCad's question first and
/// sends the flag; an agent's call without it cannot delete by accident. KiCad then clears the board editor's undo list (`m_frame->ClearUndoRedoList()`, "Undo state
/// may have copies of pointers deleted above"); the studio's history lives beside `design.json`, so the board directory's step does that once this one has been
/// applied ([`deletes_items_on_removed_layers`] tells it so; crates/cli/src/board.rs `clear_board_history`).
pub(crate) fn set_stackup(design: &mut Design, model: &ConstraintModel, settings: &StackupSettings, delete_items: bool) -> Done {
    settings.validate().map_err(|why| vec![CheckResult::fail("ops_bad_stackup", "stackup", why)])?;
    let keep = rules::copper_layer_names(settings.copper_layers as usize);
    // `getRemovedLayersWithItems`: the layers of the board the new count has not got that still have something on them, in the board's order
    let removed: Vec<(String, usize)> = model.board.layers.iter().filter(|l| !keep.contains(l)).map(|l| (l.clone(), copper_layer_items(design, l))).filter(|(_, n)| *n > 0).collect();
    if let Some((layer, n)) = removed.first() {
        if !delete_items {
            return Err(vec![CheckResult::fail(
                "ops_bad_stackup",
                layer.as_str(),
                format!("{n} item(s) are on {layer}, which a {}-layer board does not have; move or delete them first (or set delete_items_on_removed_layers to delete them, as KiCad does once asked)", settings.copper_layers),
            )]);
        }
    }
    if delete_items {
        for (layer, _) in &removed {
            remove_all_items_on_layer(design, layer);
        }
    }
    overlay(design).stackup = Some(settings.clone());
    Ok(())
}

/// Whether `cmd` (a `set_stackup` with `delete_items_on_removed_layers`, alone or in a batch) deletes something from `design` as it stands before it: some copper
/// layer a board of the new count has not got still has an item on it (`PANEL_SETUP_LAYERS::TransferDataFromWindow`'s `!removedLayers.empty()`, which is when KiCad
/// runs `RemoveAllItemsOnLayer` and then `ClearUndoRedoList`). A layer the board never had has nothing on it, so every copper name past the new count is asked.
pub fn deletes_items_on_removed_layers(design: &Design, cmd: &crate::Cmd) -> bool {
    match cmd {
        crate::Cmd::SetStackup { settings, delete_items_on_removed_layers: true } => {
            let keep = rules::copper_layer_names(settings.copper_layers as usize);
            rules::copper_layer_names(32).iter().filter(|l| !keep.contains(l)).any(|l| copper_layer_items(design, l) > 0)
        }
        crate::Cmd::Batch { cmds } => cmds.iter().any(|c| deletes_items_on_removed_layers(design, c)),
        _ => false,
    }
}

/// The type of the board's via `id` (`PCB_VIA::GetViaType`): a through via unless the board says otherwise ([`eda_model::via_extra::ViaExtra::kind`]).
fn via_kind(design: &Design, id: &str) -> ViaKind {
    design.drawings.as_ref().and_then(|d| d.via_extras.iter().find(|e| e.id == id)).map_or(ViaKind::Through, |e| e.kind)
}

/// `BOARD::HasItemsOnLayer` for a copper layer, as a count: the board-level items on `layer` (`PCB_LAYER_COLLECTOR` over `GENERAL_COLLECTOR::BoardLevelItems`,
/// footprints and what they own left out) -- the tracks, the zones and rule areas whose layers include it, the blind, buried and micro vias that start or end on it ("Vias
/// are on multiple adjacent layers, but only the top and the bottom layers are stored": a through via never counts), and the shapes, texts, dimensions, text boxes,
/// reference images, points, barcodes, tables and tuning patterns on it.
pub(crate) fn copper_layer_items(design: &Design, layer: &str) -> usize {
    let mut n = 0;
    if let Some(rt) = &design.routing {
        n += rt.tracks.iter().filter(|t| t.layer == layer).count();
        n += rt.zones.iter().filter(|z| z.is_on_layer(layer)).count();
        n += rt.vias.iter().filter(|v| via_kind(design, &v.id) != ViaKind::Through && (v.from_layer == layer || v.to_layer == layer)).count();
    }
    if let Some(d) = &design.drawings {
        n += d.shapes.iter().filter(|s| s.layer() == layer).count();
        n += d.texts.iter().filter(|t| t.layer == layer).count();
        n += d.dimensions.iter().filter(|x| x.layer == layer).count();
        n += d.board_items.text_boxes.iter().filter(|x| x.layer == layer).count();
        n += d.board_items.images.iter().filter(|x| x.layer == layer).count();
        n += d.board_items.points.iter().filter(|x| x.layer == layer).count();
        n += d.barcodes.iter().filter(|x| x.layer == layer).count();
        n += d.tables.iter().filter(|x| x.layer == layer).count();
        n += d.tuning_patterns.iter().filter(|x| x.layer == layer).count();
    }
    n
}

/// `BOARD::RemoveAllItemsOnLayer` for a copper layer: every item [`copper_layer_items`] counts goes, except that a zone or rule area on other layers too only loses
/// this one (`layers.reset( aLayer ); if( layers.any() ) item->SetLayerSet( layers ) else { Remove( item ); delete item; }`), and a through via stays ("blind/buried
/// vias with a top or bottom layer on a removed layer are removed"). A through via the model has with an end on the layer (one `AddVia` made between an outer and an
/// inner layer, which a file writes as a through via) is kept with its ends on the outer layers, which is what `PCB_VIA::LayerPair` says a through via spans. The
/// overlays of what goes (a text's extras, a shape's style, a drill chart's table) go with it; a group or a tuning pattern left without members is tidied by
/// [`crate::Board::apply`].
pub(crate) fn remove_all_items_on_layer(design: &mut Design, layer: &str) {
    let gone_vias: Vec<String> = design.routing.iter().flat_map(|rt| rt.vias.iter()).filter(|v| via_kind(design, &v.id) != ViaKind::Through && (v.from_layer == layer || v.to_layer == layer)).map(|v| v.id.clone()).collect();
    if let Some(rt) = design.routing.as_mut() {
        rt.tracks.retain(|t| t.layer != layer);
        rt.vias.retain(|v| !gone_vias.contains(&v.id));
        for v in rt.vias.iter_mut().filter(|v| v.from_layer == layer || v.to_layer == layer) {
            v.from_layer = "F.Cu".into();
            v.to_layer = "B.Cu".into();
        }
        rt.zones.retain_mut(|z| {
            if !z.is_on_layer(layer) {
                return true;
            }
            let rest: Vec<String> = z.layer_set().into_iter().filter(|l| *l != layer).map(String::from).collect();
            if rest.is_empty() {
                return false;
            }
            z.set_layer_set(&rest);
            true
        });
    }
    if let Some(d) = design.drawings.as_mut() {
        let shapes: Vec<String> = d.shapes.iter().filter(|s| s.layer() == layer).map(|s| s.id().to_string()).collect();
        d.shapes.retain(|s| s.layer() != layer);
        for id in &shapes {
            d.board_items.forget_shape(id);
        }
        let texts: Vec<String> = d.texts.iter().filter(|t| t.layer == layer).map(|t| t.id.clone()).collect();
        d.texts.retain(|t| t.layer != layer);
        d.text_extras.retain(|x| !texts.contains(&x.id));
        d.copper_texts.retain(|t| t.layer != layer);
        d.dimensions.retain(|x| x.layer != layer);
        d.board_items.text_boxes.retain(|x| x.layer != layer);
        d.board_items.images.retain(|x| x.layer != layer);
        d.board_items.points.retain(|x| x.layer != layer);
        d.barcodes.retain(|x| x.layer != layer);
        let tables: Vec<String> = d.tables.iter().filter(|t| t.layer == layer).map(|t| t.id.clone()).collect();
        d.tables.retain(|x| x.layer != layer);
        d.board_items.drill_charts.retain(|c| !tables.contains(&c.table_id));
        d.tuning_patterns.retain(|x| x.layer != layer);
        d.via_extras.retain(|e| !gone_vias.contains(&e.id));
    }
}

/// `Cmd::SetRuleSeverities`: DRC settings key -> `error` | `warning` | `ignore`, for any of the checks Violation
/// Severity lists.
pub(crate) fn set_severities(design: &mut Design, severities: &BTreeMap<String, String>) -> Done {
    for (key, severity) in severities {
        if !rules::DRC_CHECKS.iter().any(|(k, _)| k == key) {
            return Err(vec![CheckResult::fail("ops_bad_severity", key.as_str(), "not a DRC check (see Violation Severity for the list)")]);
        }
        if !rules::is_severity(severity) {
            return Err(vec![CheckResult::fail("ops_bad_severity", key.as_str(), format!("{severity:?} is not error, warning or ignore"))]);
        }
    }
    overlay(design).severities = Some(severities.clone());
    Ok(())
}

/// `Cmd::SetTextVariables`: the project's text variables (`PANEL_TEXT_VARIABLES::TransferDataFromWindow`), the whole set. Once stored the page is
/// the project's variables and the board folder's `path-variables.json` is only the seed it started from ([`rules::effective_text_variables`]).
pub(crate) fn set_text_variables(design: &mut Design, variables: &BTreeMap<String, String>) -> Done {
    rules::validate_text_variables(variables).map_err(|c| vec![c])?;
    overlay(design).text_variables = Some(variables.clone());
    Ok(())
}

/// `Cmd::SetComponentClasses`: Design Rules > Component Classes (`PANEL_ASSIGN_COMPONENT_CLASSES::TransferDataFromWindow`), the project's
/// `component_class_settings` as the page leaves them -- "Assign component class per sheet" and the custom assignments, in the page's order. `Validate` first:
/// each assignment is compiled as the board would compile it (`COMPONENT_CLASS_MANAGER::CompileAssignmentRule`) and the first that gives no rule is refused
/// with KiCad's message, "Error with conditions for component class assignment %s" (the class's name; an assignment with no class has none to show).
/// The page sends each condition as `CONDITION_DATA::GetPrimaryField` / `GetSecondaryField` give it (trimmed, quotes taken out but for a custom expression).
pub(crate) fn set_component_classes(design: &mut Design, model: &ConstraintModel, settings: &ComponentClassSettings) -> Done {
    let board = eda_drc::board::build(design, model);
    let rb = eda_drc::rule_board::RuleBoard::new(design, model, &board, eda_drc::constraints::Units::Mm, None);
    if let Some(bad) = settings.assignments.iter().find(|a| !rb.assignment_compiles(a)) {
        return Err(vec![CheckResult::fail("ops_bad_component_classes", bad.class.as_str(), format!("Error with conditions for component class assignment {}", bad.class))]);
    }
    overlay(design).component_classes = Some(settings.clone());
    Ok(())
}

/// The longest `.kicad_dru` text the dialog stores (a real rules file is a few kilobytes).
pub const MAX_CUSTOM_RULES_BYTES: usize = 256 * 1024;

/// `Cmd::SetCustomRules`: the text of the board's `.kicad_dru`, stored as written. Whether kicad-cli accepts it is
/// asked separately (`POST /api/check_rules`, which runs kicad-cli on it); like KiCad's own panel, saving rules
/// that do not compile is allowed -- DRC then runs without them.
pub(crate) fn set_custom_rules(design: &mut Design, text: &str) -> Done {
    if text.len() > MAX_CUSTOM_RULES_BYTES {
        return Err(vec![CheckResult::fail("ops_bad_custom_rules", "custom_rules", format!("the rules text is {} bytes; the most stored is {MAX_CUSTOM_RULES_BYTES}", text.len()))]);
    }
    if text.contains('\0') {
        return Err(vec![CheckResult::fail("ops_bad_custom_rules", "custom_rules", "the rules text has a NUL byte in it")]);
    }
    overlay(design).custom_rules = Some(text.to_string());
    Ok(())
}
