//! Copies of a library footprint's items inside the Footprint Editor: Duplicate (Ctrl+D, Ctrl+Shift+D), the copies Create Array makes, and Copy / Cut / Paste
//! through KiCad's clipboard text (UI-LEFT-2 FF1, FF2).
//!
//! KiCad's footprint editor makes every copy with `FOOTPRINT::DuplicateItem` (pcbnew/footprint.cpp at 8303b2ad; 25ec2335 the same, plus tables): the item cloned whole,
//! its uuid reset (`ResetUuidDirect`), so a copy looks exactly as its original -- its line style and hatch, its text's font, bold, italic, justification, width,
//! knockout and keep-upright, its lock -- and a group is copied with everything below it (`PCB_GROUP::DeepDuplicate`). Here what a footprint keeps of an item by its id
//! ([`eda_model::lib_keep::LibraryFootprintKeep`]: the line style, the hatch, the text looks, the lock, the hidden flag) is copied to the copy's id
//! (`LibraryFootprintKeep::copy_looks`); the dimensions, zones, text boxes, points, images, barcodes and tables hold their looks themselves.
//!
//! * [`copy_within`]: the copies of the items `ids` names, in the footprint, each with a new id; a group's copy holds the copies of its members (inner groups first,
//!   each group's id made from its members' as every group id is). Returns the map from each copied id to its copy's.
//! * [`duplicate`]: `EDIT_TOOL::Duplicate` in the footprint editor -- "PCB_FIELD items are specific items (not only graphic, but are properies) and cannot be
//!   duplicated like other footprint items. So skip it"; a pad that `CanHaveNumber()` takes the number Duplicate and Increment gives (every copy the same one: see
//!   PARITY-fpedit section 22). Tables are copied as KiCad 25ec2335 copies them (8303b2ad's "JEY TODO: tables" skipped them).
//! * [`clipboard_footprint`]: `CLIPBOARD_IO::SaveSelection( .., isFootprintEditor = true )` -- the partial footprint the clipboard holds: named `clipboard:<uuid>`
//!   (`LIB_ID( "clipboard", dummy.AsString() )`), with the selected items (a mandatory field skipped: `if( field->IsMandatory() ) continue;`, a group with what is
//!   below it, an item's membership of a group not copied dropped: `copy->SetParentGroup( nullptr )`), every one moved so the reference point is the origin
//!   (`copy->Move( -refPoint )`). Written by the `.kicad_mod` writer, it is the text KiCad's Footprint Editor puts on the clipboard.
//! * [`paste`]: `PCB_CONTROL::Paste` in the footprint editor -> `pasteFootprintItemsToFootprintEditor`: from a clipboard footprint, its pads, its fields that are not
//!   mandatory, its graphical items (shapes, texts, text boxes, dimensions, barcodes, tables, images), its zones, its points and its groups go into the footprint,
//!   new (`placeBoardItems( .., aIsNew = true, aAnchorAtOrigin = true ..)`: `ResetUuid()`, the clipboard's origin on the cursor), moved by `at`. Nothing is pruned
//!   (`pruneItemLayers` returns at once in the footprint editor). Differs: a user field whose name the footprint already has is not pasted (a footprint here holds one
//!   field of a name); a clipboard of a board (`(kicad_pcb ..)`) pastes the items of its footprints, not its own drawings.

use super::fp_items::{transform_ids, FIELD_PREFIX};
use super::pcb_transform::Xform;
use eda_model::groups::GroupTree;
use eda_model::ir::{Group, LibraryFootprint, Point};
use eda_model::CheckResult;
use std::collections::{BTreeMap, BTreeSet};

fn fail(check: &str, what: &str, msg: impl Into<String>) -> Vec<CheckResult> {
    vec![CheckResult::fail(check, what, msg.into())]
}

/// The leaves and the groups `ids` copies: a group stands for itself, the groups below it and every item below it (`DeepDuplicate`). In the order given, each once.
fn expand(fp: &LibraryFootprint, ids: &[String]) -> (Vec<String>, Vec<String>) {
    let tree = GroupTree(&fp.items.groups);
    let mut leaves: Vec<String> = Vec::new();
    let mut groups: Vec<String> = Vec::new();
    for id in ids {
        if tree.is_group(id) {
            for g in tree.group_and_descendants(id) {
                if !groups.contains(&g) {
                    groups.push(g);
                }
            }
            for l in tree.group_leaves(id) {
                if !leaves.contains(&l) {
                    leaves.push(l);
                }
            }
        } else if !leaves.contains(id) {
            leaves.push(id.clone());
        }
    }
    (leaves, groups)
}

/// Where a copied leaf is in its list, so its new id can be read once ids are given out.
#[derive(Clone, Copy)]
enum Slot {
    Pad(usize),
    Graphic(usize),
    Text(usize),
    Dimension(usize),
    Zone(usize),
    TextBox(usize),
    Point(usize),
    Image(usize),
    Barcode(usize),
    Table(usize),
}

/// Copy the leaf `id` into `into` (a new item, no id yet); `None` for a field or an id that names nothing copyable.
fn copy_leaf(from: &LibraryFootprint, id: &str, into: &mut LibraryFootprint) -> Option<Slot> {
    if let Some(p) = from.pads.iter().find(|p| p.id == id) {
        into.pads.push(eda_model::ir::LibraryPad { id: String::new(), ..p.clone() });
        return Some(Slot::Pad(into.pads.len() - 1));
    }
    if let Some(g) = from.graphics.iter().find(|g| g.id() == id) {
        let mut g = g.clone();
        g.set_id(String::new());
        into.graphics.push(g);
        return Some(Slot::Graphic(into.graphics.len() - 1));
    }
    if let Some(t) = from.texts.iter().find(|t| t.id == id) {
        into.texts.push(eda_model::ir::Text { id: String::new(), ..t.clone() });
        return Some(Slot::Text(into.texts.len() - 1));
    }
    let it = &from.items;
    let out = &mut into.items;
    macro_rules! copy_in {
        ($list:ident, $slot:ident, $($clear:ident),*) => {
            if let Some(x) = it.$list.iter().find(|x| x.id == id) {
                let mut x = x.clone();
                $( x.$clear = Default::default(); )*
                out.$list.push(x);
                return Some(Slot::$slot(out.$list.len() - 1));
            }
        };
    }
    copy_in!(dimensions, Dimension, id);
    copy_in!(zones, Zone, id, uuid);
    copy_in!(text_boxes, TextBox, id);
    copy_in!(points, Point, id);
    copy_in!(images, Image, id);
    copy_in!(barcodes, Barcode, id);
    copy_in!(tables, Table, id, uuid);
    None
}

fn slot_id(fp: &LibraryFootprint, s: Slot) -> String {
    let it = &fp.items;
    match s {
        Slot::Pad(i) => fp.pads[i].id.clone(),
        Slot::Graphic(i) => fp.graphics[i].id().to_string(),
        Slot::Text(i) => fp.texts[i].id.clone(),
        Slot::Dimension(i) => it.dimensions[i].id.clone(),
        Slot::Zone(i) => it.zones[i].id.clone(),
        Slot::TextBox(i) => it.text_boxes[i].id.clone(),
        Slot::Point(i) => it.points[i].id.clone(),
        Slot::Image(i) => it.images[i].id.clone(),
        Slot::Barcode(i) => it.barcodes[i].id.clone(),
        Slot::Table(i) => it.tables[i].id.clone(),
    }
}

/// The groups among `groups` of `source`, rebuilt in `into` over the copies `map` names (inner groups first: a group is made once every group it holds is), each
/// with a new id; their ids join `map`. A member that was not copied (a field) is left out, and a group left with no member is not made.
fn copy_groups(source: &[Group], groups: &[String], map: &mut BTreeMap<String, String>, into: &mut LibraryFootprint) {
    let tree = GroupTree(source);
    let mut todo: Vec<String> = groups.to_vec();
    while !todo.is_empty() {
        let before = todo.len();
        todo.retain(|gid| {
            let Some(g) = tree.group(gid) else { return false };
            if g.member_ids.iter().any(|m| tree.is_group(m) && groups.contains(m) && !map.contains_key(m)) {
                return true;
            }
            let members: Vec<String> = g.member_ids.iter().filter_map(|m| map.get(m).cloned()).collect();
            if members.is_empty() {
                return false;
            }
            into.items.groups.push(Group { id: String::new(), name: g.name.clone(), member_ids: members, design_block: String::new() });
            let slot = into.items.groups.len() - 1;
            into.assign_missing_ids();
            map.insert(gid.clone(), into.items.groups[slot].id.clone());
            false
        });
        if todo.len() == before {
            break;
        }
    }
}

/// The copies of the items `ids` names, made in `fp` itself (see the module doc). Fields are not copied. The copied ids map to their copies' ids.
pub(crate) fn copy_within(fp: &mut LibraryFootprint, ids: &[String]) -> BTreeMap<String, String> {
    let source = fp.clone();
    let (leaves, groups) = expand(&source, ids);
    let slots: Vec<(String, Slot)> = leaves.iter().filter(|id| !id.starts_with(FIELD_PREFIX)).filter_map(|id| copy_leaf(&source, id, fp).map(|s| (id.clone(), s))).collect();
    fp.assign_missing_ids();
    let mut map: BTreeMap<String, String> = slots.iter().map(|(old, s)| (old.clone(), slot_id(fp, *s))).collect();
    copy_groups(&source.items.groups, &groups, &mut map, fp);
    for (old, new) in &map {
        fp.kicad.copy_looks(old, new);
    }
    map
}

/// `EDIT_TOOL::Duplicate` in the footprint editor (see the module doc): the copies of `ids` (the fields left out) in place, each pad that can have a number taking
/// `pad_number` when one is given (Duplicate and Increment). Refused when nothing among `ids` can be copied.
pub(crate) fn duplicate(fp: &mut LibraryFootprint, ids: &[String], pad_number: Option<&str>) -> Result<BTreeMap<String, String>, Vec<CheckResult>> {
    let wanted: Vec<String> = ids.iter().filter(|id| !id.starts_with(FIELD_PREFIX)).cloned().collect();
    let map = copy_within(fp, &wanted);
    if map.is_empty() {
        return Err(fail("ops_bad_selection", &fp.name, "nothing selected can be duplicated (a field cannot be)"));
    }
    if let Some(n) = pad_number {
        let copies: BTreeSet<&String> = map.values().collect();
        for p in fp.pads.iter_mut().filter(|p| copies.contains(&p.id)) {
            if super::library_editors::pad_can_have_number(p) {
                p.number = n.to_string();
            }
        }
    }
    Ok(map)
}

/// `CLIPBOARD_IO::SaveSelection` in the footprint editor: the clipboard's partial footprint (see the module doc), `uuid` its name's dummy id.
pub fn clipboard_footprint(fp: &LibraryFootprint, ids: &[String], reference: Point, uuid: &str) -> Result<LibraryFootprint, Vec<CheckResult>> {
    let (leaves, groups) = expand(fp, ids);
    let mut out = LibraryFootprint::new_empty(format!("clipboard:{uuid}"));
    // no mandatory field: the partial footprint's own Reference and Value are what `FOOTPRINT( m_board )` starts with
    out.reference = None;
    out.value = None;
    let mut map: BTreeMap<String, String> = BTreeMap::new();
    let mut slots: Vec<(String, Slot)> = Vec::new();
    for id in &leaves {
        if let Some(name) = id.strip_prefix(FIELD_PREFIX) {
            // a user field goes with the copy; the four mandatory ones do not
            if let Some(f) = fp.fields.iter().find(|f| f.name == name) {
                out.fields.push(f.clone());
                map.insert(id.clone(), id.clone());
            }
            continue;
        }
        if let Some(s) = copy_leaf(fp, id, &mut out) {
            slots.push((id.clone(), s));
        }
    }
    if slots.is_empty() && out.fields.is_empty() {
        return Err(fail("ops_bad_selection", &fp.name, "nothing selected can be copied"));
    }
    out.assign_missing_ids();
    for (old, s) in &slots {
        map.insert(old.clone(), slot_id(&out, *s));
    }
    copy_groups(&fp.items.groups, &groups, &mut map, &mut out);
    for (old, new) in &map {
        out.kicad.copy_looks_from(&fp.kicad, old, new);
    }
    // `copy->Move( -refPoint )`
    let all: BTreeSet<String> = map.values().cloned().collect();
    transform_ids(&mut out, &all, Xform::Move { dx: -reference.x, dy: -reference.y });
    Ok(out)
}

/// `pasteFootprintItemsToFootprintEditor` + `placeBoardItems` (see the module doc): the items of the clipboard footprint `clip` into `fp`, moved by `at`, each new. The
/// ids of what was pasted, the leaves and groups at the top (what the selection takes), in the clipboard's order.
pub(crate) fn paste(fp: &mut LibraryFootprint, clip: &LibraryFootprint, at: Point) -> Result<Vec<String>, Vec<CheckResult>> {
    let tree = GroupTree(&clip.items.groups);
    let mut leaves: Vec<String> = clip.pads.iter().map(|p| p.id.clone()).collect();
    leaves.extend(clip.graphics.iter().map(|g| g.id().to_string()));
    leaves.extend(clip.texts.iter().map(|t| t.id.clone()));
    let it = &clip.items;
    leaves.extend(it.dimensions.iter().map(|x| x.id.clone()).chain(it.zones.iter().map(|x| x.id.clone())).chain(it.text_boxes.iter().map(|x| x.id.clone())));
    leaves.extend(it.points.iter().map(|x| x.id.clone()).chain(it.images.iter().map(|x| x.id.clone())).chain(it.barcodes.iter().map(|x| x.id.clone())).chain(it.tables.iter().map(|x| x.id.clone())));
    let mut slots: Vec<(String, Slot)> = Vec::new();
    for id in &leaves {
        if let Some(s) = copy_leaf(clip, id, fp) {
            slots.push((id.clone(), s));
        }
    }
    let mut map: BTreeMap<String, String> = BTreeMap::new();
    // the fields that are not mandatory, by name (one of a name in a footprint here)
    for f in &clip.fields {
        if fp.fields.iter().any(|g| g.name == f.name) {
            continue;
        }
        fp.fields.push(f.clone());
        let id = format!("{FIELD_PREFIX}{}", f.name);
        map.insert(id.clone(), id);
    }
    if slots.is_empty() && map.is_empty() {
        return Err(fail("ops_bad_paste", &fp.name, "the clipboard footprint has nothing to paste"));
    }
    fp.assign_missing_ids();
    for (old, s) in &slots {
        map.insert(old.clone(), slot_id(fp, *s));
    }
    let groups: Vec<String> = clip.items.groups.iter().map(|g| g.id.clone()).collect();
    copy_groups(&clip.items.groups, &groups, &mut map, fp);
    for (old, new) in &map {
        fp.kicad.copy_looks_from(&clip.kicad, old, new);
    }
    let moved: BTreeSet<String> = map.values().cloned().collect();
    transform_ids(fp, &moved, Xform::Move { dx: at.x, dy: at.y });
    // what the selection takes: the items in no pasted group, and the outermost pasted groups (`if( !item->GetParentGroup() || !alg::contains( aItems, .. ) )`)
    let mut top: Vec<String> = Vec::new();
    for old in leaves.iter().chain(groups.iter()) {
        let Some(new) = map.get(old) else { continue };
        if tree.parent_group(old).is_none_or(|p| !map.contains_key(&p.id)) && !top.contains(new) {
            top.push(new.clone());
        }
    }
    for f in &clip.fields {
        let id = format!("{FIELD_PREFIX}{}", f.name);
        if map.contains_key(&id) && !top.contains(&id) {
            top.push(id);
        }
    }
    Ok(top)
}
