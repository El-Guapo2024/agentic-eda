//! A pad of the board taken off its footprint or turned over on its own, as "Allow free pads" lets the board editor do (`PCBNEW_SETTINGS::m_AllowFreePads`; the edit tools'
//! `FilterCollectorForFreePads` then keeps a selected pad as itself; UI-LEFT-3 R3-7 left these two, `crates/pns/PARITY.md` stage 28).
//!
//! * **Delete** (`EDIT_TOOL::DeleteItems`, `pcbnew/tools/edit_tool.cpp` at KiCad 8303b2ad): `case PCB_PAD_T: if( IsFootprintEditor() ||
//!   frame()->GetPcbNewSettings()->m_AllowFreePads ) { commit.Remove( board_item ); itemsDeleted++; }` -- the pad leaves its footprint, the tracks routed to it stay.
//!   Here a placed footprint's pads are its definition's (the library's, or the board's own copy) with the edits laid on, so the removal is an edit too
//!   ([`eda_model::fp_edit::FootprintEdit::removed_pads`], the pad's place in the footprint before any was taken off). Everything that names the footprint's pads by
//!   their place or their `REF.NUM#k` follows it in the same step: the board file's pad records (`FootprintExtra::pads`, which the writer, the checks' board, the
//!   router and the studio's mask data read by place while their count is the footprint's) lose the pad's record, the pad edits and the footprint's groups
//!   (`pad:NUM#k`) of the later pads that share its number move down one. The footprint's pads are then the board's: `Board::pads_footprint`. An exchange of the
//!   footprint (Update / Change Footprint) brings the pad back, as KiCad's new footprint has every pad.
//! * **Flip** (`EDIT_TOOL::Flip`, a free pad kept by `FilterCollectorForFreePads`): `PAD::Flip( refPt, m_FlipDirection )` (`pcbnew/pad.cpp`) -- the position mirrored about
//!   the selection's reference point (a lone pad's own position), each copper layer's offset and trapezoid delta mirrored (`MIRROR( .., VECTOR2I{ 0, 0 }, dir )`), the
//!   orientation relative to the footprint negated (`SetFPRelativeOrientation( -GetFPRelativeOrientation() )`), a chamfered rectangle's corners mirrored
//!   (`mirrorBitFlags`: left and right, or top and bottom), the padstack's layers turned over (`PADSTACK::FlipLayers`: the front's shape to the back), the layer set
//!   flipped (`SetLayerSet( flipped )`: an SMD pad's copper, paste and mask on the other side) and a custom pad's shapes mirrored (`FlipPrimitives`). The pad keeps all of
//!   it as a pad edit in its footprint's frame ([`eda_model::fp_edit::PadEdit::opposite_side`] for its side), and its board file record's layers turn over with it
//!   (`PadMaskInfo`: its other layers, an SMD pad's several, a hole's paste and mask, the tenting of each side).

use super::Board;
use crate::pcb_transform::FlipDirection;
use eda_model::fp_edit::{base_pad_index, pad_index, parse_pad_id, PadEdit, RemovedPad};
use eda_model::ir::{ChamferCorners, Millideg, Nm, Point};
use eda_model::padstack::{flip_copper_layer, PadLayerShape, PadStackMode};
use eda_model::{CheckResult, Pad, PadKind};

fn fail(check: &str, what: &str, msg: impl Into<String>) -> Vec<CheckResult> {
    vec![CheckResult::fail(check, what, msg.into())]
}

fn norm360(a: i64) -> i64 {
    a.rem_euclid(360_000)
}

/// `mirrorBitFlags` of `PAD::Flip`: a chamfered rectangle's corners across the flip's axis.
fn mirror_corners(c: ChamferCorners, dir: FlipDirection) -> ChamferCorners {
    match dir {
        FlipDirection::LeftRight => ChamferCorners { top_left: c.top_right, top_right: c.top_left, bottom_left: c.bottom_right, bottom_right: c.bottom_left },
        FlipDirection::TopBottom => ChamferCorners { top_left: c.bottom_left, top_right: c.bottom_right, bottom_left: c.top_left, bottom_right: c.top_right },
    }
}

/// One layer's shape of `PAD::Flip`'s `ForEachUniqueLayer`: the offset and the trapezoid delta mirrored about the pad's origin, the chamfered corners swapped, a custom
/// pad's shapes mirrored (`FlipPrimitives`).
fn flip_layer_shape(s: &mut PadLayerShape, dir: FlipDirection) {
    let m = |(x, y): (Nm, Nm)| match dir {
        FlipDirection::LeftRight => (-x, y),
        FlipDirection::TopBottom => (x, -y),
    };
    s.offset = s.offset.map(m);
    s.trapezoid_delta = s.trapezoid_delta.map(m);
    s.chamfer_corners = mirror_corners(s.chamfer_corners, dir);
    if let Some(c) = s.custom.as_mut() {
        for prim in &mut c.primitives {
            crate::fp_items::mirror_shape(&mut prim.shape, Point { x: Nm::ZERO, y: Nm::ZERO }, dir);
        }
    }
}

/// `PAD::Flip`'s shape half on `pad` (in its footprint's frame): every copper layer's shape mirrored ([`flip_layer_shape`]), then `PADSTACK::FlipLayers` -- a
/// front/inner/back stack's front and back trade shapes, a custom stack's layers go to their flipped layers (`FlipLayer`, the inner ones through the middle of a stack of
/// `copper` layers) -- and an SMD pad's copper to the other side (`SetLayerSet( flipped )`).
pub(crate) fn flip_pad_shape(pad: &mut Pad, dir: FlipDirection, copper: usize) {
    let mut front = PadLayerShape::of(pad);
    flip_layer_shape(&mut front, dir);
    let stack = &mut pad.ext.stack;
    for s in stack.inner.iter_mut().chain(stack.back.iter_mut()).chain(stack.layers.values_mut()) {
        flip_layer_shape(s, dir);
    }
    match stack.mode {
        PadStackMode::Normal => front.put_on(pad),
        PadStackMode::FrontInnerBack => {
            let back = stack.back.take().unwrap_or_else(|| front.clone());
            // (a layer's own tokens stay with its layer)
            let (front_tokens, back_tokens) = (std::mem::take(&mut front.tokens), back.tokens.clone());
            let mut new_back = front;
            new_back.tokens = back_tokens;
            let mut new_front = back;
            new_front.tokens = front_tokens;
            pad.ext.stack.back = Some(new_back);
            new_front.put_on(pad);
        }
        PadStackMode::Custom => {
            let old = std::mem::take(&mut stack.layers);
            let shape_of = |role: &str| if role == "F.Cu" { Some(front.clone()) } else { old.get(role).cloned() };
            let roles: Vec<String> = old.keys().cloned().chain(std::iter::once("F.Cu".to_string())).collect();
            let mut new_front = front.clone();
            for role in roles {
                let to = flip_copper_layer(&role, copper);
                let Some(s) = shape_of(&role) else { continue };
                if to == "F.Cu" {
                    new_front = s;
                } else {
                    pad.ext.stack.layers.insert(to, s);
                }
            }
            new_front.put_on(pad);
        }
    }
    if pad.kind == PadKind::Smd {
        pad.opposite_side = !pad.opposite_side;
    }
}

impl Board<'_> {
    /// `Cmd::RemoveBoardPad`: `EDIT_TOOL::DeleteItems` of a free pad. See the module doc. Refused for an id that is no pad of a placed footprint.
    pub(crate) fn remove_board_pad(&mut self, id: &str) -> Result<(), Vec<CheckResult>> {
        let Some((reference, number, nth)) = parse_pad_id(id) else {
            return Err(fail("ops_unknown_pad", id, "no pad of a footprint on the board with this id"));
        };
        self.require_placed(&reference)?;
        let part = self.model.part(&reference).cloned().ok_or_else(|| fail("ops_unknown_part", &reference, "no part with this reference exists in the model"))?;
        let pads = self.pads_footprint(&part).ok_or_else(|| fail("ops_no_footprint", &reference, "this part has no footprint geometry to edit"))?;
        let Some(index) = pad_index(&pads, &number, nth) else {
            return Err(fail("ops_unknown_pad", id, format!("{reference} has no pad {number}")));
        };
        let removed = self.design.footprint_edit(&reference).map(|e| e.removed_pads.clone()).unwrap_or_default();
        let base_index = base_pad_index(&removed, index);
        // The later pads of the number are one place lower: `NUM#k` for k above the pad's is `NUM#k-1`.
        let renamed = |k: u32| if k > nth { k - 1 } else { k };
        if let Some(extra) = self.design.drawings.as_mut().and_then(|d| d.footprint_extras.iter_mut().find(|e| e.id == reference)) {
            // the pad's record, while the records are the footprint's pads
            if extra.pads.len() == pads.pads.len() {
                extra.pads.remove(index);
            }
            let gone = format!("pad:{number}#{nth}");
            for g in extra.groups.iter_mut() {
                g.members.retain(|m| *m != gone);
                for m in g.members.iter_mut() {
                    if let Some((n, k)) = m.strip_prefix("pad:").and_then(|r| r.rsplit_once('#')) {
                        if let (true, Ok(k)) = (n == number, k.parse::<u32>()) {
                            *m = format!("pad:{n}#{}", renamed(k));
                        }
                    }
                }
            }
            extra.groups.retain(|g| !g.members.is_empty());
            // the uuids Update Footprint on Board kept for the pads, by their places
            let pads_of_number: Vec<(String, String)> = extra.child_uuids.iter().filter(|(p, _)| p.strip_prefix("pad:").and_then(|r| r.rsplit_once('#')).is_some_and(|(n, _)| n == number)).map(|(p, u)| (p.clone(), u.clone())).collect();
            for (place, u) in pads_of_number {
                extra.child_uuids.remove(&place);
                let k: u32 = place.rsplit_once('#').and_then(|(_, k)| k.parse().ok()).unwrap_or(0);
                if k != nth {
                    extra.child_uuids.insert(format!("pad:{number}#{}", renamed(k)), u);
                }
            }
        }
        let edit = self.edit_slot(&reference);
        edit.removed_pads.push(RemovedPad { index: base_index, number: number.clone() });
        edit.removed_pads.sort_by_key(|r| r.index);
        for mut e in std::mem::take(&mut edit.pads) {
            if e.number == number {
                if e.nth.max(1) == nth {
                    continue;
                }
                e.nth = renamed(e.nth.max(1));
            }
            edit.set_pad(e);
        }
        self.tidy_edit(&reference);
        Ok(())
    }

    /// `PAD::Flip( pivot, dir )` of each free pad of `pads` (see the module doc), kept as pad edits of its footprint.
    pub(crate) fn flip_pads(&mut self, pads: &std::collections::BTreeSet<String>, pivot: Point, dir: FlipDirection) -> Result<(), Vec<CheckResult>> {
        let copper = self.model.board.layers.len();
        for id in pads {
            let Some((reference, number, nth, pad)) = self.pad_of(id) else {
                return Err(fail("ops_unknown_item", id, "no pad with this id"));
            };
            let fp = self.require_placed(&reference)?;
            // `MIRROR( m_pos, aCentre, aFlipDirection )`
            let at = crate::pcb_transform::Xform::Flip { pivot, dir }.point(eda_model::footprint::to_board(&fp, pad.at));
            let mut flipped = pad.clone();
            // a bottom-side footprint's frame is the top view's, mirrored left-right: a top-bottom flip on the board is one there too, and a left-right one is
            // a left-right one (mirroring commutes)
            flip_pad_shape(&mut flipped, dir, copper);
            let mut edit = self.design.footprint_edit(&reference).and_then(|e| e.pad(&number, nth)).cloned().unwrap_or_else(|| PadEdit::none(&number, nth));
            edit.at = Some(crate::pcb_transform::local_point(&fp, at));
            // `SetFPRelativeOrientation( -GetFPRelativeOrientation() )`
            edit.rot = Some(norm360(-(pad.rot as i64)) as Millideg);
            edit.shape = Some(flipped.shape);
            edit.size = Some(flipped.size);
            edit.roundrect_ratio = flipped.roundrect_ratio.or(edit.roundrect_ratio);
            edit.offset = Some(flipped.ext.offset.map(|(x, y)| Point { x, y }).unwrap_or_default());
            if flipped.ext.trapezoid_delta.is_some() || edit.trapezoid_delta.is_some() {
                edit.trapezoid_delta = Some(flipped.ext.trapezoid_delta.unwrap_or((Nm::ZERO, Nm::ZERO)));
            }
            if flipped.ext.chamfer_corners != pad.ext.chamfer_corners || edit.chamfer_corners.is_some() {
                edit.chamfer_corners = Some(flipped.ext.chamfer_corners);
            }
            edit.chamfer_ratio = flipped.ext.chamfer_ratio.or(edit.chamfer_ratio);
            if flipped.ext.custom.is_some() {
                edit.custom = flipped.ext.custom.clone();
            }
            if flipped.ext.stack != pad.ext.stack || edit.stack.is_some() {
                edit.stack = Some(flipped.ext.stack.clone());
            }
            edit.opposite_side = Some(flipped.opposite_side);
            self.edit_slot(&reference).set_pad(edit);
            self.tidy_edit(&reference);
            // the board file's record of the pad: its layers turned over (`SetLayerSet( flipped )`)
            let board_pads = self.model.part(&reference).and_then(|p| self.pads_footprint(p));
            let (count, index) = board_pads.map_or((0, None), |f| (f.pads.len(), pad_index(&f, &number, nth)));
            if let (Some(index), Some(extra)) = (index, self.design.drawings.as_mut().and_then(|d| d.footprint_extras.iter_mut().find(|e| e.id == reference))) {
                if extra.pads.len() == count {
                    let info = &mut extra.pads[index];
                    let flip = |l: &mut String| *l = crate::pcb_transform::flip_layer(l, copper);
                    info.layers.iter_mut().for_each(flip);
                    info.art_layers.iter_mut().for_each(flip);
                    info.smd_layers.iter_mut().for_each(flip);
                    if let Some(t) = info.hole_tech_layers.as_mut() {
                        t.iter_mut().for_each(flip);
                    }
                    std::mem::swap(&mut info.tent_front, &mut info.tent_back);
                }
            }
        }
        Ok(())
    }
}
