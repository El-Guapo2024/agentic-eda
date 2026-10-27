//! Duplicate and Paste for every kind of PCB item: the copies a command makes, and how they join the board.
//!
//! Ports `EDIT_TOOL::Duplicate` (`pcbnew/tools/edit_tool.cpp`: `BOARD_ITEM::Duplicate` for a footprint, text, shape,
//! track, via, zone or dimension, `PCB_GROUP::DeepDuplicate` for a group, a duplicate joining its original's group) and
//! `PCB_CONTROL::Paste` (`pcbnew/tools/pcb_control.cpp`) with `PCB_CONTROL::placeBoardItems`: nets are mapped by name onto the
//! board's own (`BOARD::MapNets`), copper on a layer the board does not have is dropped (`pruneItemLayers`), pasted items
//! are never locked, and a footprint whose reference is taken gets the next free number
//! (`BOARD_REANNOTATE_TOOL::ReannotateDuplicates`). What is pasted is [`eda_kicad::parse_pcb_clipboard`]'s reading of KiCad's
//! clipboard text.
//!
//! # Footprints
//!
//! A footprint on this board is a part of the intent, and a part exists once. So a copy of a footprint is a new part: a
//! [`BoardPart`] (`DrawingsSection::board_parts`) that `board::load` adds to the model, alongside its pose in
//! `placement.footprints`. A pasted footprint whose reference is a part of this board that is not placed (one that was
//! deleted, or cut) is placed again instead, the way an undo of the delete would.

use super::pcb_paste_special::{PasteMode, DEFAULT_REFERENCE};
use super::Board;
use eda_kicad::{ClipRef, Clipboard};
use eda_model::fp_edit::FootprintEdit;
use eda_model::ir::{next_item_id, BoardPart, Dimension, FootprintExtra, FootprintInstance, FootprintZoneOverrides, Group, LabelSide, LibraryFootprint, Millideg, Point, Shape, Side, Text, Track, Via, Zone};
use eda_model::barcode::Barcode;
use eda_model::pcb_items::{PcbPoint, RefImage, ShapeStyle, TextBox};
use eda_model::{CheckResult, Footprint, Part};
use std::collections::{BTreeMap, BTreeSet};

/// An item among the copies being made: the position of a copy in its list.
#[derive(Debug, Clone, Copy)]
pub(crate) enum Member {
    Track(usize),
    Via(usize),
    Zone(usize),
    Shape(usize),
    Text(usize),
    Dimension(usize),
    Footprint(usize),
    /// A `PCB_POINT` (`Copies::points`) and a `PCB_BARCODE` (`Copies::barcodes`): `EDIT_TOOL::Duplicate` duplicates them like any item.
    Point(usize),
    Barcode(usize),
    /// A `PCB_TEXTBOX` (`Copies::text_boxes`) and a `PCB_REFERENCE_IMAGE` (`Copies::images`), duplicated, pasted and arrayed like any item.
    TextBox(usize),
    Image(usize),
    /// A `PCB_TABLE` (`Copies::tables`): `ARRAY_TOOL::onDialogClosed` and `EDIT_TOOL::Duplicate` duplicate it with its cells (`item->Duplicate( true, .. )`).
    Table(usize),
    /// A group copy (`Copies::groups`); the groups a group holds are listed before it.
    Group(usize),
}

/// A footprint to be copied onto the board.
#[derive(Debug, Clone)]
pub(crate) struct FootprintCopy {
    pub(crate) reference: String,
    pub(crate) value: Option<String>,
    /// The name `Part::footprint` will carry.
    pub(crate) footprint: String,
    /// The pads and courtyard, stored with the copy when the model cannot resolve `footprint` itself.
    pub(crate) definition: Option<Footprint>,
    pub(crate) at: Point,
    pub(crate) rot: Millideg,
    pub(crate) side: Side,
    pub(crate) label: LabelSide,
    pub(crate) pad_nets: Vec<(String, String)>,
    /// What was edited on the footprint (its fields, attributes and pad overrides): a copy has them too, under its own reference.
    pub(crate) edit: Option<FootprintEdit>,
    /// The reference the copy shows when it is not its own (Create Array's "Keep original reference designators": the id is unique, the
    /// text is the original's).
    pub(crate) shown_reference: Option<String>,
    /// How zones connect to the footprint and to its pads, as the original had it (`PAD::GetLocalZoneConnection`, the thermal
    /// relief and the clearance overrides): the copy is filed under its own reference in `DrawingsSection::zone_overrides`.
    pub(crate) zone_overrides: Option<FootprintZoneOverrides>,
    /// What the original's board file said of it that the model has no field for (`DrawingsSection::footprint_extras`: its pads' layers, a footprint with no courtyard, ..): the
    /// copy has it too, under its own reference and uuid -- a copy of a microwave stub is still copper on the front layer only, with no courtyard.
    pub(crate) extra: Option<FootprintExtra>,
    /// A paste puts a part of this board that is not placed back, rather than making a second one.
    pub(crate) reuse_unplaced: bool,
    /// What the reference text of the copy says when its id is not the reference it came with (`PASTE_MODE`, [`PasteMode`]). A Duplicate and
    /// an array make unique references; Paste's default is KiCad's (keep).
    pub(crate) annotation: PasteMode,
}

impl FootprintCopy {
    /// The text the copy's Reference field shows, given the id (the reference) it ends up under: nothing special when it is the id.
    /// `shown_reference` is an array's "Keep original reference designators"; otherwise it is the mode's
    /// (`PCB_CONTROL::Paste`: `KEEP_ANNOTATIONS` leaves the reference the clipboard had, even if another footprint has it;
    /// `REMOVE_ANNOTATIONS` sets `REF**`; `UNIQUE_ANNOTATIONS` renumbered it, so the text is the id).
    pub(crate) fn shown_text(&self, id: &str) -> Option<String> {
        self.shown_reference.clone().or_else(|| match self.annotation {
            PasteMode::Unique => None,
            PasteMode::Keep => (id != self.reference).then(|| self.reference.clone()),
            PasteMode::Remove => Some(DEFAULT_REFERENCE.to_string()),
        })
    }
}

#[derive(Debug, Clone)]
pub(crate) struct GroupCopy {
    pub(crate) name: String,
    pub(crate) members: Vec<Member>,
    /// The design block the original group was linked to (`m_designBlockLibId` is copied with the group), empty for none.
    pub(crate) design_block: String,
}

/// Everything a Duplicate or a Paste adds.
#[derive(Debug, Default)]
pub(crate) struct Copies {
    pub(crate) tracks: Vec<Track>,
    pub(crate) vias: Vec<Via>,
    pub(crate) zones: Vec<Zone>,
    pub(crate) shapes: Vec<Shape>,
    /// The style each of `shapes` had -- its hatch, line endings and net (the same positions; a copy without an entry has none).
    pub(crate) shape_style: Vec<Option<ShapeStyle>>,
    pub(crate) texts: Vec<Text>,
    pub(crate) dimensions: Vec<Dimension>,
    pub(crate) footprints: Vec<FootprintCopy>,
    pub(crate) points: Vec<PcbPoint>,
    pub(crate) barcodes: Vec<Barcode>,
    pub(crate) text_boxes: Vec<TextBox>,
    pub(crate) images: Vec<RefImage>,
    pub(crate) tables: Vec<eda_model::table::Table>,
    pub(crate) groups: Vec<GroupCopy>,
    /// A copy of a member of a group that is not itself copied joins that group (`addToParentGroup`).
    pub(crate) join: Vec<(Member, String)>,
    /// The copies of locked items (`BOARD_ITEM::Duplicate` is a `Clone()`: the lock comes along). Only Duplicate fills it.
    pub(crate) locked: Vec<Member>,
    /// Each copy [`Board::collect_copies`] made, with the id of what it is a copy of.
    pub(crate) sources: Vec<(Member, String)>,
}

impl Copies {
    /// Add every copy of `other` to this set, its groups' members and joins following to their new places.
    pub(crate) fn absorb(&mut self, other: Copies) {
        let shift = |m: Member, this: &Copies| -> Member {
            match m {
                Member::Track(i) => Member::Track(i + this.tracks.len()),
                Member::Via(i) => Member::Via(i + this.vias.len()),
                Member::Zone(i) => Member::Zone(i + this.zones.len()),
                Member::Shape(i) => Member::Shape(i + this.shapes.len()),
                Member::Text(i) => Member::Text(i + this.texts.len()),
                Member::Dimension(i) => Member::Dimension(i + this.dimensions.len()),
                Member::Footprint(i) => Member::Footprint(i + this.footprints.len()),
                Member::Point(i) => Member::Point(i + this.points.len()),
                Member::Barcode(i) => Member::Barcode(i + this.barcodes.len()),
                Member::TextBox(i) => Member::TextBox(i + this.text_boxes.len()),
                Member::Image(i) => Member::Image(i + this.images.len()),
                Member::Table(i) => Member::Table(i + this.tables.len()),
                // A group copy that holds another group copy (nested groups).
                Member::Group(i) => Member::Group(i + this.groups.len()),
            }
        };
        let groups: Vec<GroupCopy> = other.groups.iter().map(|g| GroupCopy { name: g.name.clone(), members: g.members.iter().map(|m| shift(*m, self)).collect(), design_block: g.design_block.clone() }).collect();
        let join: Vec<(Member, String)> = other.join.iter().map(|(m, g)| (shift(*m, self), g.clone())).collect();
        let locked: Vec<Member> = other.locked.iter().map(|m| shift(*m, self)).collect();
        let sources: Vec<(Member, String)> = other.sources.iter().map(|(m, id)| (shift(*m, self), id.clone())).collect();
        self.tracks.extend(other.tracks);
        self.vias.extend(other.vias);
        self.zones.extend(other.zones);
        // The styles stay beside their shapes: both lists are made as long as their shapes before the others' follow.
        self.shape_style.resize(self.shapes.len(), None);
        let mut other_style = other.shape_style;
        other_style.resize(other.shapes.len(), None);
        self.shape_style.extend(other_style);
        self.shapes.extend(other.shapes);
        self.texts.extend(other.texts);
        self.dimensions.extend(other.dimensions);
        self.footprints.extend(other.footprints);
        self.points.extend(other.points);
        self.barcodes.extend(other.barcodes);
        self.text_boxes.extend(other.text_boxes);
        self.images.extend(other.images);
        self.tables.extend(other.tables);
        self.groups.extend(groups);
        self.join.extend(join);
        self.locked.extend(locked);
        self.sources.extend(sources);
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.tracks.is_empty() && self.vias.is_empty() && self.zones.is_empty() && self.shapes.is_empty() && self.texts.is_empty() && self.dimensions.is_empty() && self.footprints.is_empty() && self.points.is_empty() && self.barcodes.is_empty() && self.text_boxes.is_empty() && self.images.is_empty() && self.tables.is_empty()
    }
}

/// `UTIL::GetRefDesPrefix`: the reference without the digits and question marks that end it.
fn refdes_prefix(r: &str) -> &str {
    r.trim_end_matches(|c: char| c == '?' || c.is_ascii_digit())
}

/// `UTIL::GetRefDesNumber`: the number from the first digit on, `-1` when there is none or it is not all digits.
fn refdes_number(r: &str) -> i64 {
    match r.find(|c: char| c.is_ascii_digit()) {
        Some(i) => r[i..].parse::<i64>().unwrap_or(-1),
        None => -1,
    }
}

/// `BOARD_REANNOTATE_TOOL::ReannotateDuplicates` for one footprint: its reference while no other footprint has it, else the
/// stem with the next number up (`value < 0 ? 1 : value + 1`) until nobody does.
pub(crate) fn unique_reference(reference: &str, used: &BTreeSet<String>) -> String {
    if !used.contains(reference) {
        return reference.to_string();
    }
    let stem = refdes_prefix(reference).to_string();
    let mut value = refdes_number(reference);
    let mut candidate = reference.to_string();
    while used.contains(&candidate) {
        value = if value < 0 { 1 } else { value + 1 };
        candidate = format!("{stem}{value}");
    }
    candidate
}

fn is_copper_layer(layer: &str) -> bool {
    layer.ends_with(".Cu")
}

impl Board<'_> {
    /// Every id on the board a new item must not take.
    fn all_item_ids(&self) -> BTreeSet<String> {
        let mut out: BTreeSet<String> = BTreeSet::new();
        if let Some(rt) = &self.design.routing {
            out.extend(rt.tracks.iter().map(|t| t.id.clone()));
            out.extend(rt.vias.iter().map(|v| v.id.clone()));
            out.extend(rt.zones.iter().map(|z| z.id.clone()));
        }
        if let Some(dr) = &self.design.drawings {
            out.extend(dr.shapes.iter().map(|s| s.id().to_string()));
            out.extend(dr.texts.iter().map(|t| t.id.clone()));
            out.extend(dr.dimensions.iter().map(|d| d.id.clone()));
            out.extend(dr.groups.iter().map(|g| g.id.clone()));
        }
        out
    }

    /// Every reference a new footprint must not take: the intent's parts, the placed footprints, the board's own parts.
    pub(crate) fn all_references(&self) -> BTreeSet<String> {
        let mut out: BTreeSet<String> = self.model.parts.iter().map(|p| p.reference.clone()).collect();
        out.extend(self.placement().footprints.iter().map(|f| f.id.clone()));
        if let Some(dr) = &self.design.drawings {
            out.extend(dr.board_parts.iter().map(|b| b.reference.clone()));
        }
        out
    }

    /// A net name the board knows: the model's, or one a pasted footprint is about to bring (`extra`).
    fn net_or_none(&self, net: &str, extra: &BTreeSet<String>) -> String {
        if net.is_empty() || self.model.nets.iter().any(|n| n.name == net) || extra.contains(net) {
            net.to_string()
        } else {
            String::new()
        }
    }

    /// The edit a footprint that comes off the clipboard has: what its text says beyond the geometry (the fields, the attributes, the pads' offsets and
    /// margins), and, when the model already knows the footprint by that name, the pads of the text that are not the library's own -- the shape, size,
    /// hole, type and rotation of a pad edited on the board the copy was made on. (A footprint the model does not know keeps the pads of the text as its
    /// own definition instead.) The text's extras of a pad and the geometry that differs are one edit of that pad.
    fn clip_edit(&self, f: &eda_kicad::ClipFootprint) -> Option<FootprintEdit> {
        let mut edit = f.edit.clone();
        let Some(def) = &f.definition else { return edit };
        // The library's own footprint by that name: a probe with no reference, so that the copy's source (a footprint of this board with this
        // reference, whose pads may be edited) is not what is found.
        let probe = Part { reference: String::new(), mpn: None, lcsc: None, value: None, package: None, footprint: Some(f.name.clone()), symbol: None, datasheet: None, pins: vec![], body_nm: None, edge: None };
        let Some(known) = self.model.footprint_of(&probe) else { return edit };
        for d in eda_model::fp_edit::pad_edits_between(&known, def) {
            let e = edit.get_or_insert_with(|| FootprintEdit::new(&f.reference));
            let mut pad = e.pad(&d.number, d.nth).cloned().unwrap_or_else(|| eda_model::fp_edit::PadEdit::none(&d.number, d.nth));
            (pad.kind, pad.shape, pad.size, pad.drill, pad.drill_slot, pad.rot, pad.roundrect_ratio) = (d.kind, d.shape, d.size, d.drill, d.drill_slot, d.rot, d.roundrect_ratio);
            e.set_pad(pad);
        }
        edit
    }

    // ------------------------------------------------------------- duplicate

    /// `Cmd::Duplicate`: exact copies of the named items, at the same place, under new ids -- footprints as new parts, a group
    /// with copies of its members. See the module doc.
    pub(crate) fn duplicate_all(&mut self, ids: &[String]) -> Result<(), Vec<CheckResult>> {
        let mut copies = self.collect_copies(ids)?;
        // `BOARD_ITEM::Duplicate` is a `Clone()` with a new uuid: a locked item's copy is locked (the Move tool that carries the copies then leaves it out).
        copies.locked = copies.sources.iter().filter(|(_, id)| self.is_locked_id(id)).map(|(m, _)| *m).collect();
        self.insert_copies_all(copies).map(|_| ())
    }

    fn is_locked_id(&self, id: &str) -> bool {
        self.design.drawings.as_ref().is_some_and(|d| d.locked_ids.iter().any(|l| l == id))
    }

    /// The copies `Cmd::Duplicate` would make of `ids`, not yet on the board: exact copies at the same place, a group with copies of
    /// its members, an id that is nothing duplicable skipped. Create Array makes one set for every point of the array.
    pub(crate) fn collect_copies(&self, ids: &[String]) -> Result<Copies, Vec<CheckResult>> {
        if ids.is_empty() {
            return Err(vec![CheckResult::fail("ops_bad_duplicate", "duplicate", "no ids given")]);
        }
        let mut copies = Copies::default();
        let mut done: BTreeMap<String, Member> = BTreeMap::new();

        // Which group, if any, holds each item that is not itself copied along with its group.
        let group_of = |id: &str| -> Option<String> { self.design.drawings.as_ref().and_then(|d| d.parent_group(id)).map(|g| g.id.clone()) };

        for id in ids {
            if self.design.drawings.as_ref().is_some_and(|d| d.is_group(id)) {
                // A group is copied with everything below it, into a group of its own, which joins the group the original is in.
                if let Some(copy) = self.copy_group(id, &mut copies, &mut done, &mut Vec::new()) {
                    copies.sources.push((copy, id.clone()));
                    if let Some(above) = group_of(id) {
                        copies.join.push((copy, above));
                    }
                }
            } else if !done.contains_key(id) {
                // An id that is nothing duplicable is skipped, as in `Duplicate` over a mixed selection.
                if let Some(m) = self.copy_of(id, &mut copies) {
                    done.insert(id.clone(), m);
                    copies.sources.push((m, id.clone()));
                    // A copy that is not part of a copied group joins the group its original is in.
                    if let Some(g) = group_of(id) {
                        copies.join.push((m, g));
                    }
                }
            }
        }
        if copies.is_empty() {
            return Err(vec![CheckResult::fail("ops_unknown_duplicate", "duplicate", "none of the given ids name a footprint, track, via, zone, shape, text, dimension, point, barcode, text box, image or table")]);
        }
        Ok(copies)
    }

    /// The copy of the group `id` and of everything below it (`PCB_GROUP::DeepDuplicate`): its items are copied once whoever asks, the
    /// groups it holds before it. `None` for an id that is no group, or a loop.
    fn copy_group(&self, id: &str, copies: &mut Copies, done: &mut BTreeMap<String, Member>, visiting: &mut Vec<String>) -> Option<Member> {
        let dr = self.design.drawings.as_ref()?;
        let g = dr.group(id)?;
        if visiting.iter().any(|v| v == id) {
            return None;
        }
        visiting.push(id.to_string());
        let mut members: Vec<Member> = Vec::new();
        for m in &g.member_ids {
            let copy = if dr.is_group(m) {
                let c = self.copy_group(m, copies, done, visiting);
                if let Some(c) = c {
                    copies.sources.push((c, m.clone()));
                }
                c
            } else if let Some(c) = done.get(m) {
                Some(*c)
            } else {
                let c = self.copy_of(m, copies);
                if let Some(c) = c {
                    done.insert(m.clone(), c);
                    copies.sources.push((c, m.clone()));
                }
                c
            };
            members.extend(copy);
        }
        visiting.pop();
        copies.groups.push(GroupCopy { name: g.name.clone(), members, design_block: g.design_block.clone() });
        Some(Member::Group(copies.groups.len() - 1))
    }

    /// The copy of the item `id` names, filed into `copies`; `None` when it names nothing duplicable.
    pub(crate) fn copy_of(&self, id: &str, copies: &mut Copies) -> Option<Member> {
        if let Some(fp) = self.pose_of(id) {
            let part = self.model.part(id)?;
            let nets = self.pad_nets_of(id);
            // A part with no footprint at all has nothing to copy.
            let name = part.footprint.clone().or_else(|| part.package.clone())?;
            let zone_overrides = self.model.footprint_of(part).and_then(|d| {
                let numbers: Vec<&str> = d.pads.iter().map(|p| p.number.as_str()).collect();
                self.design.zone_overrides_of(id, &d.name, &numbers)
            });
            copies.footprints.push(FootprintCopy {
                reference: part.reference.clone(),
                value: part.value.clone(),
                footprint: name,
                definition: None,
                at: fp.at,
                rot: fp.rot,
                side: fp.side,
                label: fp.label,
                pad_nets: nets,
                edit: self.design.footprint_edit(id).cloned(),
                shown_reference: None,
                zone_overrides,
                extra: self.design.drawings.as_ref().and_then(|d| d.footprint_extras.iter().find(|e| e.id == id)).cloned(),
                reuse_unplaced: false,
                annotation: PasteMode::Unique,
            });
            return Some(Member::Footprint(copies.footprints.len() - 1));
        }
        if let Some(rt) = &self.design.routing {
            if let Some(t) = rt.tracks.iter().find(|t| t.id == id) {
                copies.tracks.push(t.clone());
                return Some(Member::Track(copies.tracks.len() - 1));
            }
            if let Some(v) = rt.vias.iter().find(|v| v.id == id) {
                copies.vias.push(v.clone());
                return Some(Member::Via(copies.vias.len() - 1));
            }
            if let Some(z) = rt.zones.iter().find(|z| z.id == id) {
                copies.zones.push(z.clone());
                return Some(Member::Zone(copies.zones.len() - 1));
            }
        }
        if let Some(dr) = &self.design.drawings {
            if let Some(s) = dr.shapes.iter().find(|s| s.id() == id) {
                copies.shape_style.resize(copies.shapes.len(), None);
                copies.shape_style.push(dr.board_items.style_of(s.id()).cloned());
                copies.shapes.push(s.clone());
                return Some(Member::Shape(copies.shapes.len() - 1));
            }
            if let Some(t) = dr.texts.iter().find(|t| t.id == id) {
                copies.texts.push(t.clone());
                return Some(Member::Text(copies.texts.len() - 1));
            }
            if let Some(d) = dr.dimensions.iter().find(|d| d.id == id) {
                copies.dimensions.push(d.clone());
                return Some(Member::Dimension(copies.dimensions.len() - 1));
            }
            if let Some(p) = dr.board_items.points.iter().find(|p| p.id == id) {
                copies.points.push(p.clone());
                return Some(Member::Point(copies.points.len() - 1));
            }
            if let Some(b) = dr.barcodes.iter().find(|b| b.id == id) {
                copies.barcodes.push(b.clone());
                return Some(Member::Barcode(copies.barcodes.len() - 1));
            }
            if let Some(t) = dr.board_items.text_boxes.iter().find(|t| t.id == id) {
                copies.text_boxes.push(t.clone());
                return Some(Member::TextBox(copies.text_boxes.len() - 1));
            }
            if let Some(i) = dr.board_items.images.iter().find(|i| i.id == id) {
                copies.images.push(i.clone());
                return Some(Member::Image(copies.images.len() - 1));
            }
            if let Some(t) = dr.tables.iter().find(|t| t.id == id) {
                let mut t = t.clone();
                super::pcb_tables::forget_uuids(&mut t);
                copies.tables.push(t);
                return Some(Member::Table(copies.tables.len() - 1));
            }
        }
        None
    }

    /// Pad number -> net name for the pads of `reference` the model puts on a net.
    fn pad_nets_of(&self, reference: &str) -> Vec<(String, String)> {
        let prefix = format!("{reference}.");
        let mut out: Vec<(String, String)> = Vec::new();
        for n in &self.model.nets {
            for pin in n.pins.iter().filter(|p| p.starts_with(&prefix)) {
                out.push((pin[prefix.len()..].to_string(), n.name.clone()));
            }
        }
        out.sort();
        out.dedup();
        out
    }

    // ----------------------------------------------------------------- paste

    /// `Cmd::PasteClipboard`: the KiCad clipboard `text`, put on the board with the clipboard's origin (the copy's reference
    /// point) at `at`. A plain Paste is `PASTE_MODE::KEEP_ANNOTATIONS` with the nets kept (`PCB_CONTROL::Paste`); Paste Special
    /// is [`Board::paste_special`].
    pub(crate) fn paste_clipboard(&mut self, text: &str, at: Point) -> Result<(), Vec<CheckResult>> {
        self.paste_special(text, at, PasteMode::default(), false)
    }

    pub(super) fn copies_from_clipboard(&self, clip: &Clipboard, at: Point, mode: PasteMode, clear_nets: bool) -> Copies {
        self.copies_from_clipboard_on(clip, at, mode, clear_nets, &self.model.board.layers, &BTreeSet::new())
    }

    /// [`Board::copies_from_clipboard`] onto a board whose copper layers are `layers` and which also knows the nets `more_nets` (Place Design
    /// Block grows both first, as `AppendBoard` does).
    pub(super) fn copies_from_clipboard_on(&self, clip: &Clipboard, at: Point, mode: PasteMode, clear_nets: bool, layers: &[String], more_nets: &BTreeSet<String>) -> Copies {
        let mv = |p: Point| Point { x: p.x + at.x, y: p.y + at.y };
        let copper_ok = |l: &str| !is_copper_layer(l) || layers.iter().any(|b| b == l);
        // The nets a pasted footprint brings with it count as the board's own for the copper pasted alongside.
        let mut new_nets: BTreeSet<String> = clip.footprints.iter().flat_map(|f| f.pad_nets.iter().map(|(_, n)| n.clone())).collect();
        new_nets.extend(more_nets.iter().cloned());
        // "Clear net assignments" (`item->SetNet( NETINFO_LIST::OrphanedItem() )` for every connected item of a pasted board; the dialog does
        // not offer it for a lone footprint, whose copy has no nets anyway).
        let clear = clear_nets && !clip.bare_footprint;
        let net_of = |net: &str| -> String { if clear { String::new() } else { self.net_or_none(net, &new_nets) } };

        let mut copies = Copies::default();
        let mut track_ix: Vec<Option<usize>> = Vec::new();
        for t in &clip.tracks {
            if !copper_ok(&t.layer) {
                track_ix.push(None);
                continue;
            }
            let mut c = t.clone();
            c.net = net_of(&t.net);
            c.pins.clear();
            c.pts = t.pts.iter().map(|p| mv(*p)).collect();
            track_ix.push(Some(copies.tracks.len()));
            copies.tracks.push(c);
        }
        let mut via_ix: Vec<Option<usize>> = Vec::new();
        for v in &clip.vias {
            if !copper_ok(&v.from_layer) || !copper_ok(&v.to_layer) {
                via_ix.push(None);
                continue;
            }
            let mut c = v.clone();
            c.net = net_of(&v.net);
            c.at = mv(v.at);
            via_ix.push(Some(copies.vias.len()));
            copies.vias.push(c);
        }
        let mut zone_ix: Vec<Option<usize>> = Vec::new();
        for z in &clip.zones {
            if !z.layer_set().into_iter().all(copper_ok) {
                zone_ix.push(None);
                continue;
            }
            let mut c = z.clone();
            c.net = if z.is_rule_area { String::new() } else { net_of(&z.net) };
            c.parent_footprint = None;
            c.outline = z.outline.iter().map(|p| mv(*p)).collect();
            c.holes = z.holes.iter().map(|h| h.iter().map(|p| mv(*p)).collect()).collect();
            zone_ix.push(Some(copies.zones.len()));
            copies.zones.push(c);
        }
        let mut shape_ix: Vec<usize> = Vec::new();
        for (k, s) in clip.shapes.iter().enumerate() {
            let mut c = s.clone();
            c.translate(at.x, at.y);
            shape_ix.push(copies.shapes.len());
            copies.shape_style.resize(copies.shapes.len(), None);
            copies.shape_style.push(clip.shape_style.get(k).cloned().flatten());
            copies.shapes.push(c);
        }
        let mut text_ix: Vec<usize> = Vec::new();
        for t in &clip.texts {
            let mut c = t.clone();
            c.at = mv(t.at);
            text_ix.push(copies.texts.len());
            copies.texts.push(c);
        }
        let mut dim_ix: Vec<usize> = Vec::new();
        for d in &clip.dimensions {
            let mut c = d.clone();
            eda_connectivity::dimension::translate_dimension(&mut c, at.x, at.y);
            dim_ix.push(copies.dimensions.len());
            copies.dimensions.push(c);
        }
        for q in &clip.points {
            let mut c = q.clone();
            c.at = mv(q.at);
            copies.points.push(c);
        }
        let mut barcode_ix: Vec<usize> = Vec::new();
        for b in &clip.barcodes {
            let mut c = b.clone();
            c.at = mv(b.at);
            barcode_ix.push(copies.barcodes.len());
            copies.barcodes.push(c);
        }
        let mut box_ix: Vec<usize> = Vec::new();
        for t in &clip.text_boxes {
            let mut c = t.clone();
            c.start = mv(t.start);
            c.end = mv(t.end);
            for q in c.pts.iter_mut() {
                *q = mv(*q);
            }
            box_ix.push(copies.text_boxes.len());
            copies.text_boxes.push(c);
        }
        let mut image_ix: Vec<usize> = Vec::new();
        for i in &clip.images {
            let mut c = i.clone();
            c.at = mv(i.at);
            image_ix.push(copies.images.len());
            copies.images.push(c);
        }
        let mut fp_ix: Vec<usize> = Vec::new();
        for f in &clip.footprints {
            fp_ix.push(copies.footprints.len());
            copies.footprints.push(FootprintCopy {
                reference: f.reference.clone(),
                value: f.value.clone(),
                footprint: f.name.clone(),
                definition: f.definition.clone(),
                at: mv(f.at),
                rot: f.rot,
                side: f.side,
                label: f.label,
                // KiCad's single-footprint clipboard has no nets: its pads stay on none.
                pad_nets: if clear { vec![] } else { f.pad_nets.clone() },
                edit: self.clip_edit(f),
                shown_reference: None,
                zone_overrides: f.zone_overrides.clone(),
                extra: f.extra.clone(),
                reuse_unplaced: true,
                annotation: mode,
            });
        }
        for g in &clip.groups {
            let members: Vec<Member> = g
                .members
                .iter()
                .filter_map(|m| match *m {
                    ClipRef::Track(i) => track_ix.get(i).copied().flatten().map(Member::Track),
                    ClipRef::Via(i) => via_ix.get(i).copied().flatten().map(Member::Via),
                    ClipRef::Zone(i) => zone_ix.get(i).copied().flatten().map(Member::Zone),
                    ClipRef::Shape(i) => shape_ix.get(i).copied().map(Member::Shape),
                    ClipRef::Text(i) => text_ix.get(i).copied().map(Member::Text),
                    ClipRef::Dimension(i) => dim_ix.get(i).copied().map(Member::Dimension),
                    ClipRef::Footprint(i) => fp_ix.get(i).copied().map(Member::Footprint),
                    ClipRef::Barcode(i) => barcode_ix.get(i).copied().map(Member::Barcode),
                    ClipRef::TextBox(i) => box_ix.get(i).copied().map(Member::TextBox),
                    ClipRef::Image(i) => image_ix.get(i).copied().map(Member::Image),
                    // An inner group is listed before the group that holds it: it is already a copy.
                    ClipRef::Group(i) => (i < copies.groups.len()).then_some(Member::Group(i)),
                })
                .collect();
            copies.groups.push(GroupCopy { name: g.name.clone(), members, design_block: g.design_block.clone() });
        }
        copies
    }

    // ---------------------------------------------------------------- insert

    /// Inserts `copies`: ids for everything (`next_item_id`, so they read like every other id), footprints placed or made
    /// into parts of their own, groups made over the new ids.
    ///
    /// Returns what the copies put at the top of the board: the ids of the new items that are in no new group, and of the new groups that are in no other (what the
    /// selection holds after a paste, and what Place Design Block groups).
    pub(crate) fn insert_copies_all(&mut self, mut copies: Copies) -> Result<Vec<String>, Vec<CheckResult>> {
        let mut taken = self.all_item_ids();
        let mut mint = |prefix: &str, seed: String| -> String {
            let id = next_item_id(prefix, &seed, &taken);
            taken.insert(id.clone());
            id
        };

        let mut track_ids: Vec<String> = Vec::new();
        for (i, t) in copies.tracks.iter_mut().enumerate() {
            let pts: Vec<String> = t.pts.iter().map(|p| format!("{},{}", p.x, p.y)).collect();
            t.id = mint("trk", format!("copy{i}|{}|{}|{}", t.net, t.layer, pts.join(";")));
            track_ids.push(t.id.clone());
        }
        let mut via_ids: Vec<String> = Vec::new();
        for (i, v) in copies.vias.iter_mut().enumerate() {
            v.id = mint("via", format!("copy{i}|{}|{},{}|{}|{}|{}|{}", v.net, v.at.x, v.at.y, v.drill, v.diameter, v.from_layer, v.to_layer));
            via_ids.push(v.id.clone());
        }
        let mut zone_ids: Vec<String> = Vec::new();
        for (i, z) in copies.zones.iter_mut().enumerate() {
            let pts: Vec<String> = z.outline.iter().map(|p| format!("{},{}", p.x, p.y)).collect();
            z.id = mint("zone", format!("copy{i}|{}|{}|{}", z.net, z.layer, pts.join(";")));
            z.uuid.clear();
            zone_ids.push(z.id.clone());
        }
        let mut shape_ids: Vec<String> = Vec::new();
        for (i, s) in copies.shapes.iter_mut().enumerate() {
            let pts: Vec<String> = s.points().iter().map(|p| format!("{},{}", p.x, p.y)).collect();
            let id = mint("shp", format!("copy{i}|{}|{}", s.layer(), pts.join(";")));
            s.set_id(id.clone());
            shape_ids.push(id);
        }
        let mut text_ids: Vec<String> = Vec::new();
        for (i, t) in copies.texts.iter_mut().enumerate() {
            t.id = mint("txt", format!("copy{i}|{}|{},{}|{}|{}", t.content, t.at.x, t.at.y, t.angle, t.layer));
            text_ids.push(t.id.clone());
        }
        let mut dim_ids: Vec<String> = Vec::new();
        for (i, d) in copies.dimensions.iter_mut().enumerate() {
            d.id = mint("dim", format!("copy{i}|{:?}|{},{}|{},{}", d.kind, d.start.x, d.start.y, d.end.x, d.end.y));
            dim_ids.push(d.id.clone());
        }
        let mut point_ids: Vec<String> = Vec::new();
        for (i, p) in copies.points.iter_mut().enumerate() {
            p.id = mint("pnt", format!("copy{i}|{}|{},{}", p.layer, p.at.x, p.at.y));
            point_ids.push(p.id.clone());
        }
        let mut barcode_ids: Vec<String> = Vec::new();
        for (i, b) in copies.barcodes.iter_mut().enumerate() {
            b.id = mint("bar", format!("copy{i}|{}|{},{}", b.layer, b.at.x, b.at.y));
            barcode_ids.push(b.id.clone());
        }
        let mut box_ids: Vec<String> = Vec::new();
        for (i, t) in copies.text_boxes.iter_mut().enumerate() {
            t.id = mint("tbx", format!("copy{i}|{}", t.id_seed()));
            box_ids.push(t.id.clone());
        }
        let mut image_ids: Vec<String> = Vec::new();
        for (i, m) in copies.images.iter_mut().enumerate() {
            m.id = mint("img", format!("copy{i}|{}", m.id_seed()));
            image_ids.push(m.id.clone());
        }
        let mut table_ids: Vec<String> = Vec::new();
        for (i, t) in copies.tables.iter_mut().enumerate() {
            t.id = mint("tbl", format!("copy{i}|{}", t.id_seed()));
            table_ids.push(t.id.clone());
        }

        // Footprints: placed again, or new parts under references nobody has.
        let mut references = self.all_references();
        let mut fp_refs: Vec<String> = Vec::new();
        let mut new_parts: Vec<(BoardPart, FootprintInstance)> = Vec::new();
        let mut replaced: Vec<FootprintInstance> = Vec::new();
        let mut new_edits: Vec<FootprintEdit> = Vec::new();
        let mut new_extras: Vec<FootprintExtra> = Vec::new();
        // The copy's edit under its own reference, showing that reference (the text of a field follows the footprint it is on).
        let own_edit = |f: &FootprintCopy, reference: &str, pose: &FootprintInstance| -> Option<FootprintEdit> {
            let mut e = f.edit.clone().unwrap_or_else(|| FootprintEdit::new(reference));
            e.id = reference.to_string();
            let shown = f.shown_text(reference);
            if let Some(l) = e.reference.as_mut() {
                l.text = shown;
            } else if let Some(shown) = shown.as_ref() {
                // Keeping the original's reference text: the layout the Reference had by default, with that text.
                let part = self.model.part(&f.reference);
                let footprint = part.and_then(|p| self.model.footprint_of(p)).or_else(|| f.definition.clone()).unwrap_or(Footprint { name: String::new(), pads: vec![], courtyard: None, courtyard_outlines: vec![], model: None, models3d: vec![], art: Default::default() });
                let mut l = eda_model::fp_edit::default_reference_layout(pose, &footprint);
                l.text = Some(shown.clone());
                e.reference = Some(l);
            }
            (!e.is_empty()).then_some(e)
        };
        // The zone-connection edits of the copies, each under the reference its footprint ends up with.
        let mut zone_edits: Vec<FootprintZoneOverrides> = Vec::new();
        for f in &copies.footprints {
            let at = self.snap_point(f.at.x, f.at.y);
            if f.reuse_unplaced && self.model.part(&f.reference).is_some() && self.pose_of(&f.reference).is_none() && !replaced.iter().any(|r| r.id == f.reference) {
                let pose = FootprintInstance { id: f.reference.clone(), at, rot: f.rot, side: f.side, label: f.label };
                new_edits.extend(own_edit(f, &f.reference, &pose));
                zone_edits.extend(f.zone_overrides.iter().map(|z| FootprintZoneOverrides { id: f.reference.clone(), ..z.clone() }));
                replaced.push(pose);
                fp_refs.push(f.reference.clone());
                continue;
            }
            let reference = unique_reference(&f.reference, &references);
            references.insert(reference.clone());
            zone_edits.extend(f.zone_overrides.iter().map(|z| FootprintZoneOverrides { id: reference.clone(), ..z.clone() }));
            // Keep the pads and courtyard with the copy unless the model already knows the footprint by that name.
            let probe = Part { reference: reference.clone(), mpn: None, lcsc: None, value: None, package: None, footprint: Some(f.footprint.clone()), symbol: None, datasheet: None, pins: vec![], body_nm: None, edge: None };
            let definition = match (&f.definition, self.model.footprint_of(&probe)) {
                (Some(def), None) => Some(LibraryFootprint::from_engine_footprint(def)),
                _ => None,
            };
            let mut pad_nets = f.pad_nets.clone();
            pad_nets.sort();
            let pose = FootprintInstance { id: reference.clone(), at, rot: f.rot, side: f.side, label: f.label };
            new_edits.extend(own_edit(f, &reference, &pose));
            // (a uuid names one footprint: the copy is given one of its own when the file is written)
            new_extras.extend(f.extra.iter().map(|e| FootprintExtra { id: reference.clone(), uuid: None, ..e.clone() }));
            new_parts.push((BoardPart { reference: reference.clone(), value: f.value.clone(), footprint: f.footprint.clone(), definition, pad_nets }, pose));
            fp_refs.push(reference);
        }

        // Groups over the new ids, the inner ones first (a group names the groups it holds by their new ids).
        let mut group_ids: Vec<Option<String>> = Vec::new();
        let id_of = |m: &Member, group_ids: &[Option<String>]| -> Option<String> {
            match *m {
                Member::Track(i) => Some(track_ids[i].clone()),
                Member::Via(i) => Some(via_ids[i].clone()),
                Member::Zone(i) => Some(zone_ids[i].clone()),
                Member::Shape(i) => Some(shape_ids[i].clone()),
                Member::Text(i) => Some(text_ids[i].clone()),
                Member::Dimension(i) => Some(dim_ids[i].clone()),
                Member::Footprint(i) => Some(fp_refs[i].clone()),
                Member::Point(i) => Some(point_ids[i].clone()),
                Member::Barcode(i) => Some(barcode_ids[i].clone()),
                Member::TextBox(i) => Some(box_ids[i].clone()),
                Member::Image(i) => Some(image_ids[i].clone()),
                Member::Table(i) => Some(table_ids[i].clone()),
                Member::Group(i) => group_ids.get(i).cloned().flatten(),
            }
        };
        let mut new_groups: Vec<Group> = Vec::new();
        for g in &copies.groups {
            let member_ids: Vec<String> = g.members.iter().filter_map(|m| id_of(m, &group_ids)).collect();
            // Every group comes over, a group of one too (`PCB_GROUP::DeepDuplicate` in Duplicate and Create Array, the clipboard's parser's
            // `resolveGroups` in Paste); only one left with no member is not made.
            if member_ids.is_empty() {
                group_ids.push(None);
                continue;
            }
            let id = next_item_id("grp", &format!("copy|{}|{}", g.name, member_ids.join(",")), &taken_with(&taken, &new_groups));
            group_ids.push(Some(id.clone()));
            new_groups.push(Group { id, name: g.name.clone(), member_ids, design_block: g.design_block.clone() });
        }
        // the top of what is coming in: no new group holds it
        let held: BTreeSet<&str> = new_groups.iter().flat_map(|g| g.member_ids.iter().map(String::as_str)).collect();
        let top: Vec<String> = track_ids.iter().chain(&via_ids).chain(&zone_ids).chain(&shape_ids).chain(&text_ids).chain(&dim_ids).chain(&point_ids).chain(&barcode_ids).chain(&box_ids).chain(&image_ids).chain(&table_ids).chain(&fp_refs).chain(new_groups.iter().map(|g| &g.id)).filter(|id| !held.contains(id.as_str())).cloned().collect();
        let joins: Vec<(String, String)> = copies.join.iter().filter_map(|(m, g)| id_of(m, &group_ids).map(|id| (id, g.clone()))).collect();
        let locked_copies: Vec<String> = copies.locked.iter().filter_map(|m| id_of(m, &group_ids)).collect();

        // Land them.
        if !copies.tracks.is_empty() || !copies.vias.is_empty() || !copies.zones.is_empty() {
            let rt = self.routing_mut();
            rt.tracks.append(&mut copies.tracks);
            rt.vias.append(&mut copies.vias);
            rt.zones.append(&mut copies.zones);
            self.sort_routing();
        }
        if !copies.shapes.is_empty() || !copies.texts.is_empty() || !copies.dimensions.is_empty() || !copies.points.is_empty() || !copies.barcodes.is_empty() || !copies.text_boxes.is_empty() || !copies.images.is_empty() || !copies.tables.is_empty() || !new_groups.is_empty() || !new_parts.is_empty() || !joins.is_empty() || !new_edits.is_empty() || !new_extras.is_empty() || !locked_copies.is_empty() {
            let dr = self.drawings_mut();
            dr.board_items.points.append(&mut copies.points);
            dr.barcodes.append(&mut copies.barcodes);
            dr.board_items.text_boxes.append(&mut copies.text_boxes);
            dr.board_items.images.append(&mut copies.images);
            dr.tables.append(&mut copies.tables);
            if !locked_copies.is_empty() {
                dr.locked_ids.extend(locked_copies);
                dr.locked_ids.sort();
                dr.locked_ids.dedup();
            }
            dr.shapes.append(&mut copies.shapes);
            // A copy of a hatched shape is hatched the same way, and has the same line endings and net.
            for (id, style) in shape_ids.iter().zip(&copies.shape_style) {
                if style.is_some() {
                    dr.board_items.set_style(id, style.as_ref());
                }
            }
            dr.texts.append(&mut copies.texts);
            dr.dimensions.append(&mut copies.dimensions);
            for (id, group) in joins {
                if let Some(g) = dr.groups.iter_mut().find(|g| g.id == group) {
                    if !g.member_ids.contains(&id) {
                        g.member_ids.push(id);
                    }
                }
            }
            dr.groups.append(&mut new_groups);
            for (bp, _) in &new_parts {
                dr.board_parts.push(bp.clone());
            }
            dr.board_parts.sort_by(|a, b| a.reference.cmp(&b.reference));
            if !new_extras.is_empty() {
                dr.footprint_extras.retain(|e| !new_extras.iter().any(|n| n.id == e.id));
                dr.footprint_extras.append(&mut new_extras);
                dr.footprint_extras.sort_by(|a, b| a.id.cmp(&b.id));
            }
            if !new_edits.is_empty() {
                dr.footprint_edits.retain(|e| !new_edits.iter().any(|n| n.id == e.id));
                dr.footprint_edits.append(&mut new_edits);
                dr.footprint_edits.sort_by(|a, b| a.id.cmp(&b.id));
            }
        }
        if !zone_edits.is_empty() {
            let dr = self.drawings_mut();
            for e in zone_edits {
                dr.zone_overrides.retain(|x| x.id != e.id);
                dr.zone_overrides.push(e);
            }
            dr.zone_overrides.sort_by(|a, b| a.id.cmp(&b.id));
        }
        if !new_parts.is_empty() || !replaced.is_empty() {
            let fps = &mut self.design.placement.as_mut().expect("a Board always carries a placement section").footprints;
            fps.extend(replaced);
            fps.extend(new_parts.into_iter().map(|(_, f)| f));
            self.sort_footprints();
        }
        Ok(top)
    }
}

fn taken_with(taken: &BTreeSet<String>, groups: &[Group]) -> BTreeSet<String> {
    let mut out = taken.clone();
    out.extend(groups.iter().map(|g| g.id.clone()));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn used(list: &[&str]) -> BTreeSet<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn a_taken_reference_gets_the_next_number_as_the_reannotate_tool_gives_it() {
        assert_eq!(unique_reference("R1", &used(["R1"].as_slice())), "R2");
        assert_eq!(unique_reference("R1", &used(["R1", "R2", "R3"].as_slice())), "R4");
        assert_eq!(unique_reference("C12", &used(["C12", "C13"].as_slice())), "C14");
        assert_eq!(unique_reference("R5", &used(["R1"].as_slice())), "R5", "a reference nobody has is kept");
        // No number to count up from: start at 1 (`value < 0 ? 1`).
        assert_eq!(unique_reference("REF**", &used(["REF**"].as_slice())), "REF**1");
        assert_eq!(unique_reference("U1A", &used(["U1A", "U1A1"].as_slice())), "U1A2");
    }
}
