//! What a board footprint holds beyond the engine's footprint, as KiCad's board footprint holds it (UI-LEFT PP5, PP14, FF4): the library footprint's record when it is put on
//! the board, and the footprint's own zones moved, turned and flipped with it.
//!
//! **The library's record.** KiCad's board footprint is a clone of the library's (Place Footprint, `ExchangeFootprint` of Update / Change Footprint and Update PCB from
//! Schematic, `SaveFootprintToBoard`), so it has every pad's layer set, die length and fabrication property, the description, tags, jumper groups, private layers, margins,
//! 3D body, pads with no copper, dimensions, zones and tables of the library's. [`LibraryRecord::of`] makes that record (`eda_kicad::library_on_board`: the library
//! footprint read back by the board's own reader) and puts its pads' facts in the order of the footprint the part is put on, which the writer and Compare with Library
//! read them in.
//!
//! **The footprint's zones.** KiCad keeps a footprint's zones (`FOOTPRINT::Zones()`: its rule areas, keepouts and zones) in the board's frame, and each transform of the
//! footprint is passed on to them (`FOOTPRINT::Move`, `SetOrientation`: `zone->Rotate( rotationCenter, angleChange )`, `Flip`). Here they are the board's zones whose
//! `Zone::parent_footprint` is the footprint, so that the writer, the fill, the DRC and the router find them where they are; the pose they were put for is the record's
//! (`FootprintExtra::zones_pose`), and [`Board::tidy_footprint_zones`] -- run after every command by `Board::apply` -- takes them from that pose to the footprint's, so
//! every verb that moves, turns or flips a footprint (a drag, Rotate, Flip, Move Exact, the placer, an exchange's pad alignment ...) carries them.

use super::pcb_transform::{transform_zone, FlipDirection, Xform};
use super::Board;
use eda_model::footprint::Footprint;
use eda_model::fp_edit::{FootprintAttrs, PadEdit};
use eda_model::fp_items_place::{placement_steps, repose_steps, unplacement_steps, ItemXform, ZonesPose};
use eda_model::ir::{FootprintExtra, FootprintInstance, LibraryFootprint, Nm, PadMaskInfo, Zone};

/// The board's verb transform of a frame step (`ItemXform`, the model's: the same clockwise turn, a flip with its axis).
fn xform_of(x: ItemXform) -> Xform {
    match x {
        ItemXform::Move { dx, dy } => Xform::Move { dx, dy },
        ItemXform::Rotate { pivot, angle } => Xform::Rotate { pivot, angle },
        ItemXform::Flip { pivot, top_bottom } => Xform::Flip { pivot, dir: if top_bottom { FlipDirection::TopBottom } else { FlipDirection::LeftRight } },
    }
}

/// `zone` taken through `steps` (`ZONE::Move` / `Rotate` / `Flip`, the board's `transform_zone`: a flip takes each layer of its set to the other side, an inner copper
/// layer to its opposite on a board of `copper` copper layers, as `BOARD::FlipLayer` does).
pub(crate) fn zone_through(mut zone: Zone, steps: &[ItemXform], copper: usize) -> Zone {
    for &x in steps {
        transform_zone(&mut zone, xform_of(x), copper);
    }
    zone
}

/// A library footprint's record for the board, with its pads' facts in the order of the footprint the part is put on ([`LibraryRecord::of`]).
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct LibraryRecord {
    /// `FootprintExtra::from_library`, in the library's frame; no id, source name or uuid yet.
    pub record: FootprintExtra,
    /// The footprint's own paste margins, which a footprint whose record holds no file form keeps in its edit (`FootprintEdit::solder_paste_margin`, `..ratio`).
    pub paste_margin: Option<Nm>,
    pub paste_ratio: Option<f64>,
    /// Each pad's own margins, as pad edits of the footprint the part is put on.
    pub pad_margins: Vec<PadEdit>,
    /// The library footprint's attributes (`(attr ..)`), when its file says them.
    pub attrs: Option<FootprintAttrs>,
    /// Its zones, in its frame, on no net.
    pub zones: Vec<Zone>,
}

impl LibraryRecord {
    /// The record of the library footprint `lib` on a board of `copper_layers`, put on the part as `target` (the engine footprint the part is on afterwards: its pads are
    /// the ones the record's pad facts are for). A pad of `target` is matched to the library's by its number and place (the k-th of a number in order where places
    /// differ); when one has no match the pads' facts are left out, and the writer gives each pad its kind's layers, as before.
    pub(crate) fn of(lib: &LibraryFootprint, target: &Footprint, copper_layers: &[String]) -> Option<Self> {
        let on = eda_kicad::library_on_board(lib, copper_layers)?;
        let order = pad_order(&on.definition, target);
        let mut record = on.record;
        record.pads = match &order {
            Some(o) if record.pads.len() == on.definition.pads.len() => o.iter().map(|&j| record.pads[j].clone()).collect::<Vec<PadMaskInfo>>(),
            _ => Vec::new(),
        };
        // the pad margins by the place of their pad in `target` (`PadEdit::nth` counts the pads of a number in the footprint's order)
        let nth_in = |f: &Footprint, i: usize| f.pads[..i].iter().filter(|p| p.number == f.pads[i].number).count() as u32 + 1;
        let mut pad_margins = Vec::new();
        if let Some(o) = &order {
            for (i, &j) in o.iter().enumerate() {
                let (number, nth) = (&on.definition.pads[j].number, nth_in(&on.definition, j));
                if let Some(m) = on.pad_margins.iter().find(|m| &m.number == number && m.nth == nth) {
                    pad_margins.push(PadEdit { nth: nth_in(target, i), ..m.clone() });
                }
            }
        }
        Some(LibraryRecord { record, paste_margin: on.paste_margin, paste_ratio: on.paste_ratio, pad_margins, attrs: on.attrs, zones: on.zones })
    }

    /// The record of a library footprint known only by its items (a footprint the engine resolves by itself, the tests): those items, nothing else of its file.
    #[cfg(test)]
    pub(crate) fn of_items(items: eda_model::fp_items::FootprintItems) -> Self {
        LibraryRecord { record: FootprintExtra { items, from_library: true, library_frame: true, ..Default::default() }, ..Default::default() }
    }
}

/// `lib` with the pads that are on a copper layer only: what the board's engine footprint holds of a library footprint, as the board's reader holds a footprint's pads
/// (`parse_pad_geometry` leaves a pad with no copper out). A pad with no copper -- a stencil opening, a mask aperture, a hole on no copper layer -- is the record's
/// (`FootprintExtra::aux_pads`, `holes`, which [`LibraryRecord::of`] gives the footprint), written back as the library has it; as an engine pad it would be written with
/// its kind's copper. A pad whose layers were never set is kept.
pub(crate) fn with_copper_pads_only(lib: &LibraryFootprint) -> LibraryFootprint {
    let mut out = lib.clone();
    out.pads.retain(|p| p.layers.is_empty() || p.layers.iter().any(|l| l.ends_with(".Cu")));
    out
}

/// For each pad of `target`, the pad of `from` it is: the same number at the same place, else the k-th pad of the number. `None` when a pad has none, or the counts differ.
fn pad_order(from: &Footprint, target: &Footprint) -> Option<Vec<usize>> {
    if from.pads.len() != target.pads.len() {
        return None;
    }
    let mut taken = vec![false; from.pads.len()];
    let mut out = Vec::with_capacity(target.pads.len());
    for (i, p) in target.pads.iter().enumerate() {
        let same_place = from.pads.iter().enumerate().position(|(j, q)| !taken[j] && q.number == p.number && q.at == p.at);
        let k = target.pads[..i].iter().filter(|q| q.number == p.number).count();
        let by_count = || from.pads.iter().enumerate().filter(|(_, q)| q.number == p.number).nth(k).map(|(j, _)| j).filter(|j| !taken[*j]);
        let j = same_place.or_else(by_count)?;
        taken[j] = true;
        out.push(j);
    }
    Some(out)
}

impl Board<'_> {
    /// A footprint just put on the board from a library (Place Footprint, Import Netlist's new footprint: `PCB_BASE_FRAME::LoadFootprint`'s footprint added whole) as the
    /// part `reference` on the footprint `name`: what it has of the library footprint `lib` beyond the engine footprint `target` the part is on -- the record (its pads'
    /// file facts, kept forms, items, margins, 3D body: [`LibraryRecord`]), the pads' own margins, the footprint's paste margins and attributes on its edit, its zones on the
    /// board. The fields stay the symbol's and the footprint's edit's (`FootprintExtra::from_library`).
    pub(crate) fn put_library_record(&mut self, reference: &str, name: &str, lib: &LibraryFootprint, target: &Footprint) {
        let Some(library) = LibraryRecord::of(lib, target, &self.model.board.layers) else { return };
        let probe = eda_model::Part { reference: reference.to_string(), mpn: None, lcsc: None, value: None, package: None, footprint: Some(name.to_string()), symbol: None, datasheet: None, pins: vec![], body_nm: None, edge: None };
        let written = eda_model::fp_items_place::written_name(self.model, &probe);
        let record = FootprintExtra { id: reference.to_string(), source_name: written, ..library.record.clone() };
        let (dnp, no_bom) = self.design.symbol_bom_flags(reference);
        let attrs = library.attrs.filter(|a| *a != FootprintAttrs::derived(target, dnp, no_bom));
        let dr = self.drawings_mut();
        dr.footprint_extras.retain(|e| e.id != reference);
        dr.footprint_extras.push(record);
        dr.footprint_extras.sort_by(|a, b| a.id.cmp(&b.id));
        if !library.pad_margins.is_empty() || library.paste_margin.is_some() || library.paste_ratio.is_some() || attrs.is_some() {
            if !dr.footprint_edits.iter().any(|e| e.id == reference) {
                dr.footprint_edits.push(eda_model::fp_edit::FootprintEdit::new(reference));
                dr.footprint_edits.sort_by(|a, b| a.id.cmp(&b.id));
            }
            if let Some(e) = dr.footprint_edits.iter_mut().find(|e| e.id == reference) {
                for m in &library.pad_margins {
                    e.set_pad(m.clone());
                }
                e.solder_paste_margin = library.paste_margin;
                e.solder_paste_ratio = library.paste_ratio;
                if attrs.is_some() {
                    e.attrs = attrs;
                }
            }
        }
        self.set_footprint_zones(reference, library.zones);
    }

    /// The zones of the footprint `reference` (`Zone::parent_footprint`) in its frame, taken off the board for its pose: what Edit Footprint's copy holds of them
    /// (`LoadFootprintFromBoard`'s clone, turned to the front at the origin).
    pub(crate) fn footprint_zones_in_frame(&self, reference: &str) -> Vec<Zone> {
        let Some(pose) = self.pose_of(reference).cloned() else { return Vec::new() };
        let copper = self.model.board.layers.len();
        let steps = unplacement_steps(&pose);
        self.design.routing.iter().flat_map(|rt| rt.zones.iter()).filter(|z| z.parent_footprint.as_deref() == Some(reference)).map(|z| zone_through(Zone { parent_footprint: None, ..z.clone() }, &steps, copper)).collect()
    }

    /// The zones of the footprint `reference` become `zones` (in its frame: the library footprint's, the Footprint Editor copy's), put on the board for its pose, each
    /// a zone of the footprint (`Zone::parent_footprint`) with an id of its own; the record says the pose they were put for (`FootprintExtra::zones_pose`). A zone the
    /// footprint had keeps its uuid when the new one is in its place (`ExchangeFootprint`'s `matchItemsBySimilarity<ZONE>` keeps an old zone's uuid on its match;
    /// here a match is a zone on the same layers with the same outline).
    pub(crate) fn set_footprint_zones(&mut self, reference: &str, zones: Vec<Zone>) {
        let Some(pose) = self.pose_of(reference).cloned() else { return };
        let copper = self.model.board.layers.len();
        let steps = placement_steps(&pose);
        let rt = self.routing_mut();
        let old: Vec<Zone> = rt.zones.iter().filter(|z| z.parent_footprint.as_deref() == Some(reference)).cloned().collect();
        rt.zones.retain(|z| z.parent_footprint.as_deref() != Some(reference));
        let had = !old.is_empty() || !zones.is_empty();
        for z in zones {
            let mut placed = zone_through(z, &steps, copper);
            placed.parent_footprint = Some(reference.to_string());
            placed.id = String::new();
            if let Some(o) = old.iter().find(|o| o.layer_set() == placed.layer_set() && o.outline == placed.outline && o.holes == placed.holes) {
                placed.uuid = o.uuid.clone();
                placed.id = o.id.clone();
            }
            rt.zones.push(placed);
        }
        if had {
            self.routing_mut().assign_missing_ids();
            self.mark_zones_at(reference, &pose);
        }
    }

    /// The record of `reference` says its zones are where its pose `pose` puts them (made when there is none).
    pub(crate) fn mark_zones_at(&mut self, reference: &str, pose: &FootprintInstance) {
        let dr = self.drawings_mut();
        if !dr.footprint_extras.iter().any(|e| e.id == reference) {
            dr.footprint_extras.push(FootprintExtra { id: reference.to_string(), library_frame: true, ..Default::default() });
            dr.footprint_extras.sort_by(|a, b| a.id.cmp(&b.id));
        }
        if let Some(e) = dr.footprint_extras.iter_mut().find(|e| e.id == reference) {
            e.zones_pose = Some(ZonesPose::of(pose));
        }
    }

    /// Before a command: the pose each footprint with zones has (`Board::apply` hands it to [`Self::tidy_footprint_zones`]).
    pub(crate) fn footprint_zone_poses(&self) -> Vec<(String, FootprintInstance)> {
        self.zone_owners().into_iter().filter_map(|r| self.pose_of(&r).map(|p| (r, p.clone()))).collect()
    }

    /// The footprints the board's zones are the children of (`Zone::parent_footprint`).
    fn zone_owners(&self) -> Vec<String> {
        let Some(rt) = &self.design.routing else { return Vec::new() };
        let mut owners: Vec<String> = rt.zones.iter().filter_map(|z| z.parent_footprint.clone()).collect();
        owners.sort();
        owners.dedup();
        owners
    }

    /// After a command: each footprint's zones put where its pose now puts them -- from the pose its record says they were put for, else the pose it had before the
    /// command (`before`, [`Self::footprint_zone_poses`]) -- and the record set to say so. A command nested in a batch has done its own, which the record then says; a
    /// footprint put back on the board (Rip, then Place) takes its zones from where they were put.
    pub(crate) fn tidy_footprint_zones(&mut self, before: Vec<(String, FootprintInstance)>) {
        let copper = self.model.board.layers.len();
        for reference in self.zone_owners() {
            let Some(now) = self.pose_of(&reference).cloned() else { continue };
            let recorded = self.design.drawings.as_ref().and_then(|d| d.footprint_extras.iter().find(|e| e.id == reference)).and_then(|e| e.zones_pose);
            let Some(from) = recorded.map(|p| p.instance()).or_else(|| before.iter().find(|(r, _)| *r == reference).map(|(_, p)| p.clone())) else { continue };
            if ZonesPose::of(&from) == ZonesPose::of(&now) {
                continue;
            }
            let steps = repose_steps(&from, &now);
            if let Some(rt) = self.design.routing.as_mut() {
                for z in rt.zones.iter_mut().filter(|z| z.parent_footprint.as_deref() == Some(reference.as_str())) {
                    *z = zone_through(std::mem::take(z), &steps, copper);
                }
            }
            self.mark_zones_at(&reference, &now);
        }
    }
}
