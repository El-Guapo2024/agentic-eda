//! The Footprint Editor's verbs on any item of the open footprint (UI-LEFT F1 and F2): its pads, graphics, texts and fields, and the items [`eda_model::fp_items`] adds
//! (dimensions, zones, text boxes, points, reference images, barcodes, tables and groups).
//!
//! KiCad's footprint editor runs the board editor's tools on the one footprint of its board (`FOOTPRINT_EDIT_FRAME` registers `EDIT_TOOL`, `DRAWING_TOOL`,
//! `PCB_GROUP_TOOL` ...), so a Move, a Rotate, a Mirror or a Delete takes every kind of item the footprint has, and each kind does what its own `Move` / `Rotate` /
//! `Mirror` does. These verbs are one variant of [`crate::Cmd`], `{"op": "fp_item", "verb": "mirror", "footprint": "..", ...}`, the way the board's
//! [`crate::pcb_items::PcbItemCmd`] is, and each is one undo step of the Footprint Editor (`Domain::FootprintEditor`).
//!
//! An item is named by its id. A field is named `field:<name>` (`field:Reference`, `field:Value`, `field:Datasheet`, `field:Description`, `field:<user field>`),
//! the key the footprint's kept uuids already use; a group stands for the items below it. The transforms are the board's ([`crate::pcb_transform`], whose turns are
//! clockwise-positive on the screen where every angle of an item is KiCad's, counter-clockwise).
//!
//! Ported from `EDIT_TOOL::Move` / `Rotate` / `Mirror` / `DeleteItems` (`pcbnew/tools/edit_tool.cpp`, `mirrorPad`), `PCB_TEXT::Rotate` / `Mirror` (`pcbnew/pcb_text.cpp`),
//! `PCB_SHAPE::Mirror` -> `EDA_SHAPE::flip` (`common/eda_shape.cpp`), `PCB_TEXTBOX::Mirror`, `ZONE::Mirror`, `PCB_GROUP::Mirror`, `PCB_GROUP_TOOL::Group` / `Ungroup`
//! (`pcbnew/tools/pcb_group_tool.cpp`, `common/tool/group_tool.cpp`), `DIALOG_TEXT_PROPERTIES::TransferDataFromWindow` for a field and `FOOTPRINT_EDIT_FRAME::OnEditItemRequest`
//! (`pcbnew/footprint_editor_utils.cpp`) for what opens on which item.

use super::pcb_items::{transform_barcode, transform_point, transform_ref_image, transform_text_box};
use super::pcb_tables::transform_table;
use super::pcb_transform::{transform_dimension, transform_shape, transform_text, transform_zone, FlipDirection, Xform};
use super::Board;
use eda_model::barcode::Barcode;
use eda_model::fp_edit::FieldLayout;
use eda_model::groups::{GroupHost, GroupTree};
use eda_model::ir::{Dimension, LibraryFootprint, LibraryPad, Millideg, Nm, Point, Shape, Text, TextJustify, Zone};
use eda_model::pcb_items::{PcbPoint, RefImage, TextBox};
use eda_model::table::Table;
use eda_model::CheckResult;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

/// `TEXT_MIN_SIZE_MM` and `TEXT_MAX_SIZE_MM` (`include/eda_text.h`): the glyph sizes the text dialogs accept.
const TEXT_MIN_SIZE_NM: Nm = Nm::new(1_000);
const TEXT_MAX_SIZE_NM: Nm = Nm::new(250_000_000);

/// The prefix of a field's id (see the module doc).
pub const FIELD_PREFIX: &str = "field:";

fn fail(check: &str, what: &str, msg: impl Into<String>) -> Vec<CheckResult> {
    vec![CheckResult::fail(check, what, msg.into())]
}

fn norm360(a: i64) -> i64 {
    a.rem_euclid(360_000)
}

/// One item of the kinds [`eda_model::fp_items`] adds, as `add` and `set` carry it: `{"dimension": {..}}`, `{"zone": {..}}` ...
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FpItem {
    Dimension(Dimension),
    Zone(Zone),
    TextBox(TextBox),
    Point(PcbPoint),
    Image(RefImage),
    Barcode(Barcode),
    Table(Table),
}

/// One edit of the items of a library footprint. On the wire `{"op": "fp_item", "verb": "<verb>", "footprint": "<name>", ...}`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "verb", rename_all = "snake_case")]
pub enum FpItemCmd {
    /// `EDIT_TOOL::Move` (a drag, or the armed Move dropping what it carries): every item `ids` names by `(dx, dy)`. A field moves without the footprint, as it does on the
    /// board (`PCB_FIELD::Move`).
    Move { footprint: String, ids: Vec<String>, dx: Nm, dy: Nm },
    /// `EDIT_TOOL::Rotate` (R, Shift+R): every item turns about `pivot` by `angle_millideg`, positive clockwise on the screen (the sense of `Cmd::RotateItems`), and its own
    /// orientation follows: a pad's orientation, a text's, a field's, a text box's, a barcode's and a table's angle, a dimension's text.
    Rotate { footprint: String, ids: Vec<String>, pivot: Point, angle_millideg: i64 },
    /// `EDIT_TOOL::Mirror` (`pcbnew.InteractiveEdit.mirrorHoriontally` / `mirrorVertically`): the mirrorable items among `ids` (`EDIT_TOOL::MirrorableItems`: shapes,
    /// fields, texts, text boxes, zones, pads, groups, points) mirror across the axis through `pivot`, on the layer they are on. The others (a dimension, an image, a barcode,
    /// a table) are left as they are, as KiCad leaves them -- unless they are members of a group, which mirrors every member (`PCB_GROUP::Mirror`).
    Mirror { footprint: String, ids: Vec<String>, pivot: Point, direction: FlipDirection },
    /// `EDIT_TOOL::DeleteItems`: pads, graphics, texts and the items of [`eda_model::fp_items`] go; a group goes with every item below it; a field is hidden (`case PCB_FIELD_T:
    /// field->SetVisible( false )`), never removed.
    Delete { footprint: String, ids: Vec<String> },
    /// `DIALOG_TEXT_PROPERTIES` on a field (`FOOTPRINT_EDIT_FRAME::OnEditItemRequest`: `case PCB_FIELD_T: ShowTextPropertiesDialog`) and the canvas's field edits: the field
    /// `name` shows `text` and is laid out as `layout` says (its `visible` is the field's Show). An empty `text` leaves the field's text as it is (`if(
    /// !m_SingleLineText->GetValue().IsEmpty() ) m_item->SetText( .. )`). Refused for a field the footprint does not have, a glyph size out of range and a layer that is empty.
    EditField { footprint: String, name: String, text: String, layout: FieldLayout },
    /// `DIALOG_SHAPE_PROPERTIES::TransferDataFromWindow` (`ShowGraphicItemPropertiesDialog`): the graphic replaced by `shape`, keeping its id and its kept line style.
    SetGraphic { footprint: String, id: String, shape: Shape },
    /// `DIALOG_TEXT_PROPERTIES` on a text of the footprint (`case PCB_TEXT_T`): the text replaced by `text`, its place included, keeping its id; an empty content leaves
    /// the text's own (the dialog sets the text only when the box is not empty).
    SetText { footprint: String, id: String, text: Text },
    /// `DRAWING_TOOL` placing an item of [`FpItem`]'s kinds in the footprint (Draw Dimension, Draw Rule Area, Draw Text Box, Place Point, Place Reference Image, Add Barcode,
    /// Draw Table): the item's `id`, if the caller sent one, is ignored.
    Add { footprint: String, item: FpItem },
    /// The Properties dialog of such an item (`DIALOG_DIMENSION_PROPERTIES`, the rule area and zone editors, `DIALOG_TEXTBOX_PROPERTIES`, ...): every field replaced, in place;
    /// the id and the groups stay. The kind must be the item's.
    Set { footprint: String, id: String, item: FpItem },
    /// `PCB_GROUP_TOOL::Group` in the footprint editor (Ctrl+G): a new group of the groupable items among `ids` (`BOARD_ITEM::IsGroupableType`: every kind but the point),
    /// at least two. See [`eda_model::groups::GroupHost::make_group`].
    Group { footprint: String, ids: Vec<String> },
    /// `GROUP_TOOL::Ungroup` (Ctrl+Shift+G): the groups among `ids` dissolve.
    Ungroup { footprint: String, ids: Vec<String> },
    /// `DIALOG_GROUP_PROPERTIES`: the group's name.
    SetGroupName { footprint: String, id: String, name: String },
}

impl FpItemCmd {
    /// The footprint the verb edits.
    pub fn footprint(&self) -> &str {
        match self {
            FpItemCmd::Move { footprint, .. }
            | FpItemCmd::Rotate { footprint, .. }
            | FpItemCmd::Mirror { footprint, .. }
            | FpItemCmd::Delete { footprint, .. }
            | FpItemCmd::EditField { footprint, .. }
            | FpItemCmd::SetGraphic { footprint, .. }
            | FpItemCmd::SetText { footprint, .. }
            | FpItemCmd::Add { footprint, .. }
            | FpItemCmd::Set { footprint, .. }
            | FpItemCmd::Group { footprint, .. }
            | FpItemCmd::Ungroup { footprint, .. }
            | FpItemCmd::SetGroupName { footprint, .. } => footprint,
        }
    }

    /// A short description for the activity log.
    pub fn describe(&self) -> String {
        let verb = match self {
            FpItemCmd::Move { ids, .. } => format!("move {} item(s)", ids.len()),
            FpItemCmd::Rotate { ids, .. } => format!("rotate {} item(s)", ids.len()),
            FpItemCmd::Mirror { ids, .. } => format!("mirror {} item(s)", ids.len()),
            FpItemCmd::Delete { ids, .. } => format!("delete {} item(s)", ids.len()),
            FpItemCmd::EditField { name, .. } => format!("edit field {name:?}"),
            FpItemCmd::SetGraphic { id, .. } => format!("edit graphic {id}"),
            FpItemCmd::SetText { id, .. } => format!("edit text {id}"),
            FpItemCmd::Add { item, .. } => format!("add {}", item.kind()),
            FpItemCmd::Set { id, item, .. } => format!("edit {} {id}", item.kind()),
            FpItemCmd::Group { ids, .. } => format!("group {} item(s)", ids.len()),
            FpItemCmd::Ungroup { ids, .. } => format!("ungroup {} group(s)", ids.len()),
            FpItemCmd::SetGroupName { id, name, .. } => format!("name group {id} {name:?}"),
        };
        format!("footprint {:?}: {verb}", self.footprint())
    }
}

impl FpItem {
    pub fn kind(&self) -> &'static str {
        match self {
            FpItem::Dimension(_) => "dimension",
            FpItem::Zone(_) => "zone",
            FpItem::TextBox(_) => "text box",
            FpItem::Point(_) => "point",
            FpItem::Image(_) => "reference image",
            FpItem::Barcode(_) => "barcode",
            FpItem::Table(_) => "table",
        }
    }
}

/// The name of the field `id` names, if it names one (`field:Reference` -> `Reference`).
pub fn field_name(id: &str) -> Option<&str> {
    id.strip_prefix(FIELD_PREFIX)
}

/// The layout of the field `name` of `fp`, made KiCad's default first when the footprint has none of its own (the Reference, the Value, a user field placed nowhere
/// yet), so it can be moved or turned. `None` for a field the footprint does not have.
fn field_layout_mut<'a>(fp: &'a mut LibraryFootprint, name: &str) -> Option<&'a mut FieldLayout> {
    use eda_model::fp_edit::{default_library_reference_layout, default_library_value_layout, new_library_field_layout};
    match name {
        "Reference" => {
            let visible = fp.reference_visible;
            Some(fp.reference.get_or_insert_with(|| FieldLayout { visible, ..default_library_reference_layout() }))
        }
        "Value" => {
            let visible = fp.value_visible;
            Some(fp.value.get_or_insert_with(|| FieldLayout { visible, ..default_library_value_layout() }))
        }
        "Datasheet" => fp.datasheet.as_mut(),
        "Description" => fp.description_field.as_mut(),
        _ => fp.fields.iter_mut().find(|f| f.name == name).map(|f| {
            let visible = f.visible;
            f.layout.get_or_insert_with(|| FieldLayout { visible, ..new_library_field_layout() })
        }),
    }
}

/// `PCB_TEXT::Rotate` / `Move` on a field's layout (its angle is KiCad's, counter-clockwise; `Xform::Rotate`'s is clockwise).
fn transform_field(l: &mut FieldLayout, x: Xform) {
    l.at = x.point(l.at);
    if let Xform::Rotate { angle, .. } = x {
        l.angle = norm360(l.angle - angle);
    }
}

/// `PCB_TEXT::Mirror` on a field: "the position and justification are mirrored, but not the text itself" -- the horizontal justification turns over when the text runs
/// across the axis (horizontal for a left-right mirror, vertical for a top-bottom one).
fn mirror_field(l: &mut FieldLayout, pivot: Point, dir: FlipDirection) {
    match dir {
        FlipDirection::TopBottom => {
            if l.angle == 90_000 {
                l.halign = -l.halign;
            }
            l.at.y = 2 * pivot.y - l.at.y;
        }
        FlipDirection::LeftRight => {
            if l.angle == 0 {
                l.halign = -l.halign;
            }
            l.at.x = 2 * pivot.x - l.at.x;
        }
    }
}

fn flip_justify(j: TextJustify) -> TextJustify {
    match j {
        TextJustify::Left => TextJustify::Right,
        TextJustify::Right => TextJustify::Left,
        other => other,
    }
}

/// `PCB_TEXT::Mirror` on a text of the footprint (see [`mirror_field`]).
fn mirror_text(t: &mut Text, pivot: Point, dir: FlipDirection) {
    match dir {
        FlipDirection::TopBottom => {
            if t.angle == 90_000 {
                t.justify = flip_justify(t.justify);
            }
            t.at.y = 2 * pivot.y - t.at.y;
        }
        FlipDirection::LeftRight => {
            if t.angle == 0 {
                t.justify = flip_justify(t.justify);
            }
            t.at.x = 2 * pivot.x - t.at.x;
        }
    }
}

/// `MIRROR( aPoint, aMirrorRef, aFlipDirection )`.
fn mirror_point(p: Point, pivot: Point, dir: FlipDirection) -> Point {
    match dir {
        FlipDirection::LeftRight => Point { x: 2 * pivot.x - p.x, y: p.y },
        FlipDirection::TopBottom => Point { x: p.x, y: 2 * pivot.y - p.y },
    }
}

/// `PCB_SHAPE::Mirror` -> `EDA_SHAPE::flip( aCentre, aFlipDirection )`: every point mirrors, an arc swaps its ends so it still runs the same way, and the layer stays
/// (unlike `PCB_SHAPE::Flip`). A rectangle is not normalised.
pub(crate) fn mirror_shape(s: &mut Shape, pivot: Point, dir: FlipDirection) {
    let m = |p: &mut Point| *p = mirror_point(*p, pivot, dir);
    match s {
        Shape::Segment { start, end, .. } | Shape::Rect { start, end, .. } | Shape::Circle { center: start, end, .. } => {
            m(start);
            m(end);
        }
        Shape::Arc { start, mid, end, .. } => {
            m(start);
            m(mid);
            m(end);
            std::mem::swap(start, end);
        }
        Shape::Polygon { pts, .. } => pts.iter_mut().for_each(m),
        Shape::Bezier { start, c1, c2, end, .. } => {
            m(start);
            m(c1);
            m(c2);
            m(end);
        }
    }
}

/// `mirrorPad` (`edit_tool.cpp`): a custom pad's basic shapes flip in the pad's frame (`FlipPrimitives`), the position mirrors, and so do the offset and a trapezoid's
/// delta; the orientation negates. ("TODO(JE) padstacks": the other layers' shapes are not touched, as in KiCad.)
fn mirror_pad(p: &mut LibraryPad, pivot: Point, dir: FlipDirection) {
    let origin = Point { x: Nm::ZERO, y: Nm::ZERO };
    if let Some(custom) = &mut p.custom {
        for prim in &mut custom.primitives {
            mirror_shape(&mut prim.shape, origin, dir);
        }
    }
    p.at = mirror_point(p.at, pivot, dir);
    p.offset = mirror_point(p.offset, origin, dir);
    if let Some((dx, dy)) = p.trapezoid_delta {
        let d = mirror_point(Point { x: dx, y: dy }, origin, dir);
        p.trapezoid_delta = Some((d.x, d.y));
    }
    p.rot = norm360(-(p.rot as i64)) as Millideg;
}

/// `PCB_TEXTBOX::Mirror`: the shape mirrors on its layer, the text angle is `180 - angle` left-right and `-angle` top-bottom; the text and its justification stay.
fn mirror_text_box(t: &mut TextBox, pivot: Point, dir: FlipDirection) {
    t.start = mirror_point(t.start, pivot, dir);
    t.end = mirror_point(t.end, pivot, dir);
    for p in t.pts.iter_mut() {
        *p = mirror_point(*p, pivot, dir);
    }
    t.angle = match dir {
        FlipDirection::LeftRight => norm360(180_000 - t.angle as i64),
        FlipDirection::TopBottom => norm360(-(t.angle as i64)),
    } as Millideg;
}

/// The ids of every item of `fp`, fields included (`field:<name>` for each field it has), groups too.
fn live_ids(fp: &LibraryFootprint) -> BTreeSet<String> {
    let mut ids: BTreeSet<String> = fp.pads.iter().map(|p| p.id.clone()).chain(fp.graphics.iter().map(|g| g.id().to_string())).chain(fp.texts.iter().map(|t| t.id.clone())).collect();
    ids.extend(fp.items.ids().map(String::from));
    for name in ["Reference", "Value"] {
        ids.insert(format!("{FIELD_PREFIX}{name}"));
    }
    if fp.datasheet.is_some() {
        ids.insert(format!("{FIELD_PREFIX}Datasheet"));
    }
    if fp.description_field.is_some() {
        ids.insert(format!("{FIELD_PREFIX}Description"));
    }
    ids.extend(fp.fields.iter().map(|f| format!("{FIELD_PREFIX}{}", f.name)));
    ids
}

/// The items `ids` stand for: each id checked to name an item of the footprint, each group replaced by the items below it (`PCB_GROUP::RunOnChildren( .., RECURSE )`).
fn targets(fp: &LibraryFootprint, ids: &[String]) -> Result<BTreeSet<String>, Vec<CheckResult>> {
    if ids.is_empty() {
        return Err(fail("ops_bad_selection", &fp.name, "name at least one item"));
    }
    let live = live_ids(fp);
    let tree = GroupTree(&fp.items.groups);
    let mut out = BTreeSet::new();
    for id in ids {
        if !live.contains(id) {
            return Err(fail("ops_unknown_item", id, "no item of this footprint has this id"));
        }
        if tree.is_group(id) {
            out.extend(tree.group_leaves(id));
        } else {
            out.insert(id.clone());
        }
    }
    Ok(out)
}

/// Move or turn every item of `fp` that `ids` holds.
fn transform(fp: &mut LibraryFootprint, ids: &BTreeSet<String>, x: Xform) {
    // A library footprint has no board: the copper count only matters for a flip, which these verbs never do.
    const COPPER: usize = 2;
    for p in fp.pads.iter_mut().filter(|p| ids.contains(&p.id)) {
        p.at = x.point(p.at);
        if let Xform::Rotate { angle, .. } = x {
            // `PAD::Rotate`: `m_orient += aAngle` (KiCad's angle, counter-clockwise).
            p.rot = norm360(p.rot as i64 - angle) as Millideg;
        }
    }
    for g in fp.graphics.iter_mut().filter(|g| ids.contains(g.id())) {
        transform_shape(g, x, COPPER);
    }
    for t in fp.texts.iter_mut().filter(|t| ids.contains(&t.id)) {
        transform_text(t, x, COPPER);
    }
    let names: Vec<String> = ids.iter().filter_map(|id| field_name(id).map(String::from)).collect();
    for name in names {
        if let Some(l) = field_layout_mut(fp, &name) {
            transform_field(l, x);
        }
    }
    let it = &mut fp.items;
    for d in it.dimensions.iter_mut().filter(|d| ids.contains(&d.id)) {
        transform_dimension(d, x, COPPER);
    }
    for z in it.zones.iter_mut().filter(|z| ids.contains(&z.id)) {
        transform_zone(z, x, COPPER);
    }
    for t in it.text_boxes.iter_mut().filter(|t| ids.contains(&t.id)) {
        transform_text_box(t, x, COPPER);
    }
    for p in it.points.iter_mut().filter(|p| ids.contains(&p.id)) {
        transform_point(p, x, COPPER);
    }
    for i in it.images.iter_mut().filter(|i| ids.contains(&i.id)) {
        transform_ref_image(i, x, COPPER);
    }
    for b in it.barcodes.iter_mut().filter(|b| ids.contains(&b.id)) {
        transform_barcode(b, x, COPPER);
    }
    for t in it.tables.iter_mut().filter(|t| ids.contains(&t.id)) {
        transform_table(t, x, COPPER);
    }
}

/// `FOOTPRINT::MoveAnchorPosition`'s part of the items of [`eda_model::fp_items`]: every graphical item (`GraphicalItems()`: the dimensions, text boxes, images, barcodes and
/// tables) and every zone moves by `(dx, dy)`; the points do not (`m_points` is not in KiCad's list, so a point stays where it was in the footprint's frame).
pub(crate) fn move_items_with_anchor(fp: &mut LibraryFootprint, dx: Nm, dy: Nm) {
    let ids: BTreeSet<String> = fp.items.ids().filter(|id| !fp.items.points.iter().any(|p| p.id == *id) && !fp.items.groups.iter().any(|g| g.id == *id)).map(String::from).collect();
    transform(fp, &ids, Xform::Move { dx, dy });
}

/// The checks `DIALOG_TEXT_PROPERTIES::TransferDataFromWindow` and the Properties panel make of a text's look: a layer, and a glyph size in KiCad's range.
fn check_text_look(what: &str, layer: &str, sizes: &[Nm]) -> Result<(), Vec<CheckResult>> {
    if layer.is_empty() {
        return Err(fail("ops_bad_text", what, "text needs a layer"));
    }
    if sizes.iter().any(|s| *s < TEXT_MIN_SIZE_NM || *s > TEXT_MAX_SIZE_NM) {
        return Err(fail("ops_bad_text", what, "the text size must be between 0.001 mm and 250 mm"));
    }
    Ok(())
}

/// What an item of [`FpItem`]'s kinds must be to be kept: a layer, and the geometry its kind needs.
fn check_item(what: &str, item: &FpItem) -> Result<(), Vec<CheckResult>> {
    let layer = match item {
        FpItem::Dimension(d) => &d.layer,
        FpItem::Zone(z) => &z.layer,
        FpItem::TextBox(t) => &t.layer,
        FpItem::Point(p) => &p.layer,
        FpItem::Image(i) => &i.layer,
        FpItem::Barcode(b) => &b.layer,
        FpItem::Table(t) => &t.layer,
    };
    if layer.is_empty() && !matches!(item, FpItem::Zone(z) if !z.layers.is_empty()) {
        return Err(fail("ops_bad_item", what, format!("a {} needs a layer", item.kind())));
    }
    match item {
        FpItem::Zone(z) if z.outline.len() < 3 => Err(fail("ops_bad_zone", what, "a zone outline needs at least three corners")),
        FpItem::TextBox(t) => check_text_look(what, &t.layer, &[t.size_nm]),
        FpItem::Point(p) if p.size_nm <= Nm::ZERO => Err(fail("ops_bad_point", what, "the size of a point must be greater than zero")),
        FpItem::Image(i) if i.scale_ppm <= 0 => Err(fail("ops_bad_image", what, "the scale of an image must be greater than zero")),
        FpItem::Image(i) if i.header().is_none() => Err(fail("ops_bad_image", what, "the image data is not a PNG, JPEG, GIF or BMP file")),
        FpItem::Barcode(b) if b.width_nm <= Nm::ZERO || b.height_nm <= Nm::ZERO => Err(fail("ops_bad_barcode", what, "the size of a barcode must be greater than zero")),
        FpItem::Barcode(b) if b.text_height_nm <= Nm::ZERO => Err(fail("ops_bad_barcode", what, "the text size of a barcode must be greater than zero")),
        FpItem::Table(t) if t.col_widths.is_empty() || t.row_heights.is_empty() || t.cells.len() != t.col_widths.len() * t.row_heights.len() => {
            Err(fail("ops_bad_table", what, "a table needs a cell for every row and column"))
        }
        _ => Ok(()),
    }
}

impl Board<'_> {
    pub(crate) fn apply_fp_item(&mut self, cmd: &FpItemCmd) -> Result<(), Vec<CheckResult>> {
        let fp = self.library_footprint_mut(cmd.footprint())?;
        match cmd {
            FpItemCmd::Move { ids, dx, dy, .. } => {
                let t = targets(fp, ids)?;
                transform(fp, &t, Xform::Move { dx: *dx, dy: *dy });
                Ok(())
            }
            FpItemCmd::Rotate { ids, pivot, angle_millideg, .. } => {
                let t = targets(fp, ids)?;
                transform(fp, &t, Xform::Rotate { pivot: *pivot, angle: *angle_millideg });
                Ok(())
            }
            FpItemCmd::Mirror { ids, pivot, direction, .. } => mirror(fp, ids, *pivot, *direction),
            FpItemCmd::Delete { ids, .. } => delete(fp, ids),
            FpItemCmd::EditField { name, text, layout, .. } => edit_field(fp, name, text, layout),
            FpItemCmd::SetGraphic { id, shape, .. } => {
                if shape.layer().is_empty() {
                    return Err(fail("ops_bad_shape", id, "a shape needs a layer"));
                }
                if let Shape::Polygon { pts, .. } = shape {
                    if pts.len() < 3 {
                        return Err(fail("ops_bad_shape", id, "a polygon needs at least three points"));
                    }
                }
                let slot = fp.graphics.iter_mut().find(|g| g.id() == id).ok_or_else(|| fail("ops_unknown_shape", id, "no shape with this id"))?;
                let mut s = shape.clone();
                s.set_id(id.clone());
                *slot = s;
                Ok(())
            }
            FpItemCmd::SetText { id, text, .. } => {
                check_text_look(id, &text.layer, &[text.size_nm])?;
                let slot = fp.texts.iter_mut().find(|t| &t.id == id).ok_or_else(|| fail("ops_unknown_text", id, "no text with this id"))?;
                let content = if text.content.is_empty() { slot.content.clone() } else { text.content.clone() };
                *slot = Text { id: id.clone(), content, ..text.clone() };
                Ok(())
            }
            FpItemCmd::Add { item, .. } => {
                check_item(item.kind(), item)?;
                let it = &mut fp.items;
                match item.clone() {
                    FpItem::Dimension(mut d) => {
                        d.id.clear();
                        it.dimensions.push(d);
                    }
                    FpItem::Zone(mut z) => {
                        z.id.clear();
                        z.net.clear(); // a zone of a library footprint has no net
                        it.zones.push(z);
                    }
                    FpItem::TextBox(mut t) => {
                        t.id.clear();
                        it.text_boxes.push(t);
                    }
                    FpItem::Point(mut p) => {
                        p.id.clear();
                        it.points.push(p);
                    }
                    FpItem::Image(mut i) => {
                        i.id.clear();
                        i.data = eda_model::image_info::compact(&i.data);
                        it.images.push(i);
                    }
                    FpItem::Barcode(mut b) => {
                        b.id.clear();
                        super::pcb_items::normalize_barcode(&mut b);
                        it.barcodes.push(b);
                    }
                    FpItem::Table(mut t) => {
                        super::pcb_tables::forget_uuids(&mut t);
                        it.tables.push(t);
                    }
                }
                fp.assign_missing_ids();
                Ok(())
            }
            FpItemCmd::Set { id, item, .. } => {
                check_item(id, item)?;
                set_item(fp, id, item.clone())
            }
            FpItemCmd::Group { ids, .. } => {
                let live = live_ids(fp);
                if let Some(bad) = ids.iter().find(|id| !live.contains(*id)) {
                    return Err(fail("ops_unknown_item", bad, "no item of this footprint has this id"));
                }
                // `PCB_GROUP_TOOL::Group`'s filter in the footprint editor: `if( !item->IsGroupableType() ) aCollector.Remove( item )` -- a point is not groupable.
                let groupable: Vec<String> = ids.iter().filter(|id| !fp.items.points.iter().any(|p| &p.id == *id)).cloned().collect();
                fp.make_group(&groupable).map_err(|r| fail(r.check, &r.subject, r.why))
            }
            FpItemCmd::Ungroup { ids, .. } => {
                if let Some(bad) = ids.iter().find(|id| !GroupTree(&fp.items.groups).is_group(id)) {
                    return Err(fail("ops_unknown_group", bad, "no group of this footprint has this id"));
                }
                fp.dissolve_groups(ids);
                Ok(())
            }
            FpItemCmd::SetGroupName { id, name, .. } => {
                let g = fp.items.groups.iter_mut().find(|g| &g.id == id).ok_or_else(|| fail("ops_unknown_group", id, "no group of this footprint has this id"))?;
                g.name = name.clone();
                Ok(())
            }
        }
    }
}

/// `EDIT_TOOL::Mirror` on the footprint's items (see [`FpItemCmd::Mirror`]).
fn mirror(fp: &mut LibraryFootprint, ids: &[String], pivot: Point, dir: FlipDirection) -> Result<(), Vec<CheckResult>> {
    let tree = GroupTree(&fp.items.groups);
    // A group mirrors every member with its own `Mirror` (`PCB_GROUP::Mirror`), the dimensions among them too; the selection's own items only when mirrorable.
    let in_groups: BTreeSet<String> = ids.iter().filter(|id| tree.is_group(id)).flat_map(|id| tree.group_leaves(id)).collect();
    let all = targets(fp, ids)?;
    let dimensions_mirror = |id: &String| in_groups.contains(id);
    for p in fp.pads.iter_mut().filter(|p| all.contains(&p.id)) {
        mirror_pad(p, pivot, dir);
    }
    for g in fp.graphics.iter_mut().filter(|g| all.contains(g.id())) {
        mirror_shape(g, pivot, dir);
    }
    for t in fp.texts.iter_mut().filter(|t| all.contains(&t.id)) {
        mirror_text(t, pivot, dir);
    }
    let names: Vec<String> = all.iter().filter_map(|id| field_name(id).map(String::from)).collect();
    for name in names {
        if let Some(l) = field_layout_mut(fp, &name) {
            mirror_field(l, pivot, dir);
        }
    }
    let it = &mut fp.items;
    for z in it.zones.iter_mut().filter(|z| all.contains(&z.id)) {
        // `ZONE::Mirror`: the outline and its holes, on the zone's own layers
        for p in z.outline.iter_mut().chain(z.holes.iter_mut().flatten()) {
            *p = mirror_point(*p, pivot, dir);
        }
    }
    for t in it.text_boxes.iter_mut().filter(|t| all.contains(&t.id)) {
        mirror_text_box(t, pivot, dir);
    }
    // `PCB_POINT` is in `MirrorableItems` but has no `Mirror` of its own (KiCad then reports "virtual BOARD_ITEM::Mirror used, should not occur"): the studio mirrors
    // its position, which is what the action means.
    for p in it.points.iter_mut().filter(|p| all.contains(&p.id)) {
        p.at = mirror_point(p.at, pivot, dir);
    }
    // `PCB_DIMENSION_BASE::Mirror` (and the aligned and orthogonal ones' height), for a dimension a mirrored group holds: the board's flip of a dimension without its layer.
    for d in it.dimensions.iter_mut().filter(|d| dimensions_mirror(&d.id)) {
        let layer = d.layer.clone();
        transform_dimension(d, Xform::Flip { pivot, dir }, 2);
        d.layer = layer;
    }
    Ok(())
}

/// `EDIT_TOOL::DeleteItems` on the footprint's items (see [`FpItemCmd::Delete`]).
fn delete(fp: &mut LibraryFootprint, ids: &[String]) -> Result<(), Vec<CheckResult>> {
    let tree = GroupTree(&fp.items.groups);
    let groups: BTreeSet<String> = ids.iter().filter(|id| tree.is_group(id)).cloned().collect();
    let mut gone = targets(fp, ids)?;
    // a deleted group's nested groups go with it
    let nested: Vec<String> = fp.items.groups.iter().filter(|g| groups.iter().any(|top| tree.group_holds(top, &g.id))).map(|g| g.id.clone()).collect();
    gone.extend(groups.iter().cloned());
    gone.extend(nested);
    // A field is hidden, never removed.
    let names: Vec<String> = gone.iter().filter_map(|id| field_name(id).map(String::from)).collect();
    for name in &names {
        match name.as_str() {
            "Reference" => fp.reference_visible = false,
            "Value" => fp.value_visible = false,
            _ => {}
        }
        if let Some(l) = field_layout_mut(fp, name) {
            l.visible = false;
        }
        if let Some(f) = fp.fields.iter_mut().find(|f| &f.name == name) {
            f.visible = false;
        }
    }
    fp.pads.retain(|p| !gone.contains(&p.id));
    if fp.pad_edit.as_ref().is_some_and(|m| gone.contains(&m.pad)) {
        fp.pad_edit = None;
    }
    fp.graphics.retain(|g| !gone.contains(g.id()));
    fp.texts.retain(|t| !gone.contains(&t.id));
    let it = &mut fp.items;
    it.dimensions.retain(|d| !gone.contains(&d.id));
    it.zones.retain(|z| !gone.contains(&z.id));
    it.text_boxes.retain(|t| !gone.contains(&t.id));
    it.points.retain(|p| !gone.contains(&p.id));
    it.images.retain(|i| !gone.contains(&i.id));
    it.barcodes.retain(|b| !gone.contains(&b.id));
    it.tables.retain(|t| !gone.contains(&t.id));
    it.groups.retain(|g| !gone.contains(&g.id));
    let live = live_ids(fp);
    fp.items.prune_groups(&live);
    Ok(())
}

/// [`FpItemCmd::EditField`].
fn edit_field(fp: &mut LibraryFootprint, name: &str, text: &str, layout: &FieldLayout) -> Result<(), Vec<CheckResult>> {
    use eda_model::fp_edit::LIBRARY_REFERENCE_TEXT;
    check_text_look(name, &layout.layer, &[layout.size.0, layout.size.1])?;
    // an empty box leaves the text as it is
    let current = current_field_text(fp, name).ok_or_else(|| fail("ops_unknown_field", name, "the footprint has no such field"))?;
    let text = if text.is_empty() { current.as_str() } else { text };
    let shown_default = match name {
        "Reference" => Some(LIBRARY_REFERENCE_TEXT.to_string()),
        "Value" => Some(fp.name.clone()),
        _ => None,
    };
    let mut l = layout.clone();
    match name {
        "Reference" | "Value" => {
            l.text = (shown_default.as_deref() != Some(text)).then(|| text.to_string());
            if name == "Reference" {
                fp.reference_visible = l.visible;
                fp.reference = Some(l);
            } else {
                fp.value_visible = l.visible;
                fp.value = Some(l);
            }
        }
        "Datasheet" | "Description" => {
            let slot = if name == "Datasheet" { &mut fp.datasheet } else { &mut fp.description_field };
            if slot.is_none() {
                return Err(fail("ops_unknown_field", name, "the footprint has no such field"));
            }
            l.text = Some(text.to_string());
            *slot = Some(l);
        }
        _ => {
            let f = fp.fields.iter_mut().find(|f| f.name == name).ok_or_else(|| fail("ops_unknown_field", name, "the footprint has no such field"))?;
            f.value = text.to_string();
            f.visible = l.visible;
            l.text = None;
            f.layout = Some(l);
        }
    }
    Ok(())
}

/// The text the field `name` of `fp` shows (`REF**` and the footprint's name for a Reference and a Value of no text of their own), `None` for a field it does not have.
pub fn current_field_text(fp: &LibraryFootprint, name: &str) -> Option<String> {
    use eda_model::fp_edit::LIBRARY_REFERENCE_TEXT;
    match name {
        "Reference" => Some(fp.reference.as_ref().and_then(|l| l.text.clone()).unwrap_or_else(|| LIBRARY_REFERENCE_TEXT.to_string())),
        "Value" => Some(fp.value.as_ref().and_then(|l| l.text.clone()).unwrap_or_else(|| fp.name.clone())),
        "Datasheet" => fp.datasheet.as_ref().map(|l| l.text.clone().unwrap_or_default()),
        "Description" => fp.description_field.as_ref().map(|l| l.text.clone().unwrap_or_default()),
        _ => fp.fields.iter().find(|f| f.name == name).map(|f| f.value.clone()),
    }
}

/// [`FpItemCmd::Set`]: the item `id` replaced by `item`, which must be of its kind.
fn set_item(fp: &mut LibraryFootprint, id: &str, item: FpItem) -> Result<(), Vec<CheckResult>> {
    let it = &mut fp.items;
    let unknown = || fail("ops_unknown_item", id, "no item of this kind in the footprint has this id");
    match item {
        FpItem::Dimension(mut d) => {
            let slot = it.dimensions.iter_mut().find(|x| x.id == id).ok_or_else(unknown)?;
            d.id = id.to_string();
            *slot = d;
        }
        FpItem::Zone(mut z) => {
            let slot = it.zones.iter_mut().find(|x| x.id == id).ok_or_else(unknown)?;
            z.id = id.to_string();
            z.net.clear();
            *slot = z;
        }
        FpItem::TextBox(mut t) => {
            let slot = it.text_boxes.iter_mut().find(|x| x.id == id).ok_or_else(unknown)?;
            t.id = id.to_string();
            *slot = t;
        }
        FpItem::Point(mut p) => {
            let slot = it.points.iter_mut().find(|x| x.id == id).ok_or_else(unknown)?;
            p.id = id.to_string();
            *slot = p;
        }
        FpItem::Image(mut i) => {
            let slot = it.images.iter_mut().find(|x| x.id == id).ok_or_else(unknown)?;
            i.id = id.to_string();
            i.data = eda_model::image_info::compact(&i.data);
            *slot = i;
        }
        FpItem::Barcode(mut b) => {
            let slot = it.barcodes.iter_mut().find(|x| x.id == id).ok_or_else(unknown)?;
            b.id = id.to_string();
            super::pcb_items::normalize_barcode(&mut b);
            *slot = b;
        }
        FpItem::Table(mut t) => {
            let slot = it.tables.iter_mut().find(|x| x.id == id).ok_or_else(unknown)?;
            t.id = id.to_string();
            if t.uuid.is_empty() {
                t.uuid = slot.uuid.clone();
            }
            *slot = t;
        }
    }
    Ok(())
}
