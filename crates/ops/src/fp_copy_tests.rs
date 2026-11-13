//! Copies inside the Footprint Editor ([`crate::fp_copy`], UI-LEFT-2 FF1 and FF2): Duplicate and Create Array copy every kind of item with its looks, a group with what
//! is below it, never a field; Copy writes KiCad's clipboard footprint and Paste reads it back (and KiCad's own text), new and moved, with the looks.

use super::fp_copy::clipboard_footprint;
use super::fp_items::{FpItem, FpItemCmd};
use super::pcb_transform_tests::{dimension, model, p};
use super::*;
use eda_model::ir::{Design, DimensionKind, TextJustify};
use eda_model::pcb_items::TextBox;
use eda_model::units::mm;

fn design() -> Design {
    serde_json::from_str(r#"{"schema":1,"provenance":{"engine_version":"0","intent_hash":"x","seed":0,"stage_hashes":[]}}"#).unwrap()
}

const NAME: &str = "Lib:Copy";

/// `Lib:Copy`: a dashed line, a bold italic text and a solid circle, a dimension and a text box (ids assigned), the line and the text grouped.
fn with_footprint(m: &ConstraintModel) -> Board<'_> {
    let mut b = Board::new(design(), m, mm(0.1), mm(0.3));
    let mut fp = LibraryFootprint::new_empty(NAME);
    fp.graphics = vec![
        Shape::Segment { id: String::new(), layer: "F.SilkS".into(), stroke_width: mm(0.12), filled: false, start: p(mm(-3.0), mm(-1.0)), end: p(mm(3.0), mm(-1.0)) },
        Shape::Circle { id: String::new(), layer: "F.Fab".into(), stroke_width: mm(0.1), filled: false, center: p(Nm::ZERO, Nm::ZERO), end: p(mm(1.0), Nm::ZERO) },
    ];
    fp.texts = vec![Text { id: String::new(), content: "T1".into(), at: p(mm(1.0), mm(2.0)), angle: 0, layer: "F.SilkS".into(), size_nm: mm(1.0), stroke_width: mm(0.15), justify: TextJustify::Left, mirror: false }];
    fp.assign_missing_ids();
    let line = fp.graphics[0].id().to_string();
    let text = fp.texts[0].id.clone();
    fp.kicad.stroke_types.insert(line.clone(), "dash".into());
    fp.kicad.text_extras.push(eda_model::board_text::TextExtra { id: text.clone(), bold: true, italic: true, ..Default::default() });
    fp.kicad.unlocked_texts.insert(text.clone());
    b.apply(&Cmd::PutLibraryFootprint { footprint: fp, overwrite: false }).unwrap();
    b.apply(&Cmd::FpItem(FpItemCmd::Add { footprint: NAME.into(), item: FpItem::Dimension(dimension("", DimensionKind::Aligned { height: mm(1.0) }, p(Nm::ZERO, mm(5.0)), p(mm(4.0), mm(5.0)))) })).unwrap();
    b.apply(&Cmd::FpItem(FpItemCmd::Add { footprint: NAME.into(), item: FpItem::TextBox(TextBox::new("F.Fab", p(mm(1.0), mm(1.0)), p(mm(4.0), mm(2.0)), mm(1.0), mm(0.1))) })).unwrap();
    let ids = ids_of(&b);
    b.apply(&Cmd::FpItem(FpItemCmd::Group { footprint: NAME.into(), ids: vec![ids.line.clone(), ids.text.clone()] })).unwrap();
    b
}

fn fp<'a>(b: &'a Board<'_>) -> &'a LibraryFootprint {
    b.design().footprint_library.as_ref().unwrap().by_name(NAME).unwrap()
}

struct Ids {
    line: String,
    text: String,
    dim: String,
    tbx: String,
}

fn ids_of(b: &Board<'_>) -> Ids {
    let f = fp(b);
    Ids {
        line: f.graphics.iter().find(|g| matches!(g, Shape::Segment { .. })).unwrap().id().to_string(),
        text: f.texts[0].id.clone(),
        dim: f.items.dimensions.first().map(|d| d.id.clone()).unwrap_or_default(),
        tbx: f.items.text_boxes.first().map(|t| t.id.clone()).unwrap_or_default(),
    }
}

#[test]
fn duplicate_copies_every_kind_with_its_looks_and_a_group_with_its_members_but_no_field() {
    let m = model(&["F.Cu", "B.Cu"]);
    let mut b = with_footprint(&m);
    let ids = ids_of(&b);
    let group = fp(&b).items.groups[0].id.clone();
    b.apply(&Cmd::FpItem(FpItemCmd::Duplicate { footprint: NAME.into(), ids: vec![group.clone(), ids.dim.clone(), ids.tbx.clone(), "field:Reference".into()], pad_number: None })).unwrap();
    let f = fp(&b);
    assert_eq!(f.graphics.iter().filter(|g| matches!(g, Shape::Segment { .. })).count(), 2);
    assert_eq!(f.texts.len(), 2);
    assert_eq!(f.items.dimensions.len(), 2);
    assert_eq!(f.items.text_boxes.len(), 2);
    let line2 = f.graphics.iter().find(|g| matches!(g, Shape::Segment { .. }) && g.id() != ids.line).unwrap().id().to_string();
    let text2 = f.texts.iter().find(|t| t.id != ids.text).unwrap().id.clone();
    assert_eq!(f.kicad.stroke_types.get(&line2).map(String::as_str), Some("dash"), "the line's style goes with the copy");
    let extra = f.kicad.text_extra(&text2).expect("the text's looks go with the copy");
    assert!(extra.bold && extra.italic);
    assert!(f.kicad.unlocked_texts.contains(&text2), "and its keep-upright");
    assert_eq!(f.items.groups.len(), 2, "the group is copied with its members");
    let copy = f.items.groups.iter().find(|g| g.id != group).unwrap();
    let mut members = copy.member_ids.clone();
    members.sort();
    let mut want = vec![line2.clone(), text2.clone()];
    want.sort();
    assert_eq!(members, want);
    // the copies sit where the originals are, until the Move tool carries them
    assert_eq!(f.items.dimensions[0].start, f.items.dimensions[1].start);
    assert!(f.fields.is_empty() && f.reference.is_none(), "no field is copied");
    // a field alone is refused
    let e = b.apply(&Cmd::FpItem(FpItemCmd::Duplicate { footprint: NAME.into(), ids: vec!["field:Value".into()], pad_number: None })).unwrap_err();
    assert_eq!(e[0].check, "ops_bad_selection");
}

#[test]
fn create_array_copies_dimensions_text_boxes_and_looks_to_every_point() {
    let m = model(&["F.Cu", "B.Cu"]);
    let mut b = with_footprint(&m);
    let ids = ids_of(&b);
    let geometry = ArrayGeometry::Grid { nx: 3, ny: 1, dx: mm(10.0), dy: Nm::ZERO, offset_x: Nm::ZERO, offset_y: Nm::ZERO, centred: false, stagger: 1, stagger_rows: true, horizontal_then_vertical: true, reverse_alternate: false };
    b.apply(&Cmd::CreateFootprintArray { footprint: NAME.into(), ids: vec![ids.dim.clone(), ids.line.clone()], geometry, arrange: false, numbering: None }).unwrap();
    let f = fp(&b);
    let mut xs: Vec<Nm> = f.items.dimensions.iter().map(|d| d.start.x).collect();
    xs.sort();
    assert_eq!(xs, vec![Nm::ZERO, mm(10.0), mm(20.0)], "a dimension at each point");
    let lines: Vec<&Shape> = f.graphics.iter().filter(|g| matches!(g, Shape::Segment { .. })).collect();
    assert_eq!(lines.len(), 3);
    assert!(lines.iter().all(|l| f.kicad.stroke_types.get(l.id()).map(String::as_str) == Some("dash")), "every copy of the line is dashed");
    assert_eq!(f.items.text_boxes.len(), 1, "what is not selected is not copied");
}

#[test]
fn copy_writes_kicads_clipboard_footprint_and_paste_reads_it_back_moved_with_its_looks() {
    let m = model(&["F.Cu", "B.Cu"]);
    let mut b = with_footprint(&m);
    let ids = ids_of(&b);
    let clip = clipboard_footprint(fp(&b), &[ids.line.clone(), ids.tbx.clone(), ids.dim.clone(), "field:Reference".into()], p(mm(-3.0), mm(-1.0)), "00000000-0000-4000-8000-000000000001").unwrap();
    let text = eda_kicad::export_kicad_mod(&clip);
    assert!(text.starts_with("(footprint \"clipboard:00000000-0000-4000-8000-000000000001\""), "{}", &text[..80.min(text.len())]);
    assert!(text.contains("(type dash)"), "the line is written dashed");
    assert!(text.contains("(fp_text_box") || text.contains("(text_box"), "the text box is in it");
    // measured from the reference point: the line now starts at the origin
    assert!(text.contains("(start 0 0)"), "{text}");
    b.apply(&Cmd::FpItem(FpItemCmd::Paste { footprint: NAME.into(), text: text.clone(), at: p(mm(10.0), mm(10.0)) })).unwrap();
    let f = fp(&b);
    let pasted = f.graphics.iter().find(|g| matches!(g, Shape::Segment { start, .. } if *start == p(mm(10.0), mm(10.0)))).expect("the line, its start on the paste point");
    assert_eq!(f.kicad.stroke_types.get(pasted.id()).map(String::as_str), Some("dash"));
    assert_eq!(f.items.dimensions.len(), 2);
    assert_eq!(f.items.text_boxes.len(), 2);
    assert_eq!(f.texts.len(), 1, "the mandatory fields of the clipboard are not pasted");
    // a text that is no footprint is refused (the studio pastes it as a text)
    let e = b.apply(&Cmd::FpItem(FpItemCmd::Paste { footprint: NAME.into(), text: "hello".into(), at: p(Nm::ZERO, Nm::ZERO) })).unwrap_err();
    assert_eq!(e[0].check, "ops_bad_paste");
}

#[test]
fn paste_takes_kicads_own_footprint_editor_clipboard() {
    let m = model(&["F.Cu", "B.Cu"]);
    let mut b = with_footprint(&m);
    let before = fp(&b).clone();
    // what KiCad 10's Footprint Editor puts on the clipboard for a dashed line and a pad (`CLIPBOARD_IO::SaveSelection`)
    let kicad = r#"(footprint "clipboard:6c7e3ce6-1f11-4bd2-9c3b-5a6f2fb1a5c0"
	(version 20250114)
	(generator "pcbnew")
	(layer "F.Cu")
	(fp_line (start -1 0) (end 1 0) (stroke (width 0.2) (type dot)) (layer "F.SilkS") (uuid "a1b2c3d4-0000-4000-8000-000000000001"))
	(pad "7" smd rect (at 0 2) (size 1 1) (layers "F.Cu" "F.Paste" "F.Mask") (uuid "a1b2c3d4-0000-4000-8000-000000000002"))
)"#;
    b.apply(&Cmd::FpItem(FpItemCmd::Paste { footprint: NAME.into(), text: kicad.into(), at: p(mm(5.0), Nm::ZERO) })).unwrap();
    let f = fp(&b);
    assert_eq!(f.pads.len(), before.pads.len() + 1);
    let pad = f.pads.iter().find(|p| p.number == "7").unwrap();
    assert_eq!(pad.at, p(mm(5.0), mm(2.0)));
    let line = f.graphics.iter().find(|g| matches!(g, Shape::Segment { start, .. } if *start == p(mm(4.0), Nm::ZERO))).expect("the line, moved");
    assert_eq!(f.kicad.stroke_types.get(line.id()).map(String::as_str), Some("dot"));
    assert!(!f.kicad.uuids.contains_key(line.id()), "a pasted item is new: it keeps no uuid of the clipboard's");
}

/// The other way round (slow, `EDA_SLOW_TESTS=1`): what the studio's Copy puts on the clipboard is a footprint KiCad reads -- kicad-cli's `fp upgrade` loads it and
/// writes it back with the dashed line, the dimension and the text box, all measured from the reference point.
#[test]
fn slow_kicad_cli_reads_the_clipboard_footprint_our_copy_writes() {
    if std::env::var_os("EDA_SLOW_TESTS").is_none() {
        eprintln!("skipped: slow test; set EDA_SLOW_TESTS=1 to run it");
        return;
    }
    let cli = std::process::Command::new("which").arg("kicad-cli").output().ok().filter(|o| o.status.success()).map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string());
    let Some(cli) = cli.filter(|c| !c.is_empty()) else {
        eprintln!("skipped: no kicad-cli");
        return;
    };
    let m = model(&["F.Cu", "B.Cu"]);
    let b = with_footprint(&m);
    let ids = ids_of(&b);
    let clip = clipboard_footprint(fp(&b), &[ids.line.clone(), ids.tbx.clone(), ids.dim.clone()], p(mm(-3.0), mm(-1.0)), "6c7e3ce6-1f11-4bd2-9c3b-5a6f2fb1a5c0").unwrap();
    let text = eda_kicad::export_kicad_mod(&clip);
    let dir = std::env::temp_dir().join(format!("eda_fp_clip_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let lib = dir.join("clip.pretty");
    std::fs::create_dir_all(&lib).unwrap();
    std::fs::write(lib.join("Clip.kicad_mod"), &text).unwrap();
    let up = dir.join("up.pretty");
    let out = std::process::Command::new(&cli).args(["fp", "upgrade", "--force", "-o"]).arg(&up).arg(&lib).output().expect("runs kicad-cli");
    assert!(out.status.success(), "kicad-cli fp upgrade: {}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    let kicad = std::fs::read_dir(&up).unwrap().filter_map(|e| e.ok()).find(|e| e.path().extension().is_some_and(|x| x == "kicad_mod")).map(|e| std::fs::read_to_string(e.path()).unwrap()).expect("kicad-cli wrote the footprint");
    assert!(kicad.contains("(type dash)"), "the dashed line: {kicad}");
    assert_eq!(kicad.matches("\n\t(dimension").count(), 1, "{kicad}");
    assert_eq!(kicad.matches("\n\t(fp_text_box").count(), 1, "{kicad}");
    // read back by the studio's reader, what KiCad wrote is the clipboard's line, from the reference point
    let back = eda_kicad::parse_library_footprint(&kicad).unwrap().footprint;
    assert!(back.graphics.iter().any(|g| matches!(g, Shape::Segment { start, end, .. } if *start == p(Nm::ZERO, Nm::ZERO) && *end == p(mm(6.0), Nm::ZERO))), "{:?}", back.graphics);
    let _ = std::fs::remove_dir_all(&dir);
}

/// A board's clipboard (what the board editor copies) pasted into the Footprint Editor (UI-LEFT-3 F3-10, `PCB_CONTROL::Paste` -> `pasteFootprintItemsToFootprintEditor`):
/// the items of a footprint the board had at (10, 5) turned 90 degrees come in KiCad's frame -- a pad where the board had it, at its board angle (`SetParent` only); a
/// text turned back by the footprint's angle in the footprint's frame; a dimension taken whole from the board's frame into the footprint's -- and the board's own
/// drawings (a line, a text) and its group of them come too; the footprint, a member of that group, leaves it; a point of the footprint is not pasted.
#[test]
fn a_boards_clipboard_pastes_its_footprints_in_the_editors_frame_and_its_own_drawings() {
    let m = model(&["F.Cu", "B.Cu"]);
    let mut b = with_footprint(&m);
    let before = fp(&b).clone();
    let board = r#"(kicad_pcb (version 20240108) (generator "pcbnew")
	(footprint "R" (layer "F.Cu") (uuid "f0000000-0000-4000-8000-000000000001") (at 10 5 90)
		(fp_line (start -1 0) (end 1 0) (stroke (width 0.12) (type solid)) (layer "F.SilkS") (uuid "f0000000-0000-4000-8000-000000000002"))
		(fp_text user "Q" (at 0 -2 90) (layer "F.Fab") (uuid "f0000000-0000-4000-8000-000000000003") (effects (font (size 1 1) (thickness 0.15))))
		(dimension (type aligned) (layer "F.Fab") (uuid "f0000000-0000-4000-8000-000000000004") (pts (xy 10 5) (xy 12 5)) (height 1)
			(gr_text "2" (at 11 4 0) (layer "F.Fab") (uuid "f0000000-0000-4000-8000-000000000005") (effects (font (size 1 1) (thickness 0.15))))
			(format (units 3) (units_format 1) (precision 4)) (style (thickness 0.1) (arrow_length 1.27) (text_position_mode 0) (extension_height 0.58642) (extension_offset 0.5) keep_text_aligned))
		(point (at 10 5) (size 1) (layer "F.Fab") (uuid "f0000000-0000-4000-8000-000000000006"))
		(pad "1" smd rect (at 1 0 90) (size 1 1) (layers "F.Cu" "F.Paste" "F.Mask") (uuid "f0000000-0000-4000-8000-000000000007"))
	)
	(gr_line (start 20 20) (end 25 20) (stroke (width 0.2) (type dash)) (layer "Dwgs.User") (uuid "f0000000-0000-4000-8000-000000000010"))
	(gr_text "BOARD" (at 20 22 0) (layer "Dwgs.User") (uuid "f0000000-0000-4000-8000-000000000011") (effects (font (size 1 1) (thickness 0.15))))
	(segment (start 0 0) (end 1 0) (width 0.2) (layer "F.Cu") (net 0) (uuid "f0000000-0000-4000-8000-000000000012"))
	(group "" (uuid "f0000000-0000-4000-8000-000000000020") (members "f0000000-0000-4000-8000-000000000010" "f0000000-0000-4000-8000-000000000011" "f0000000-0000-4000-8000-000000000001"))
)"#;
    b.apply(&Cmd::FpItem(FpItemCmd::Paste { footprint: NAME.into(), text: board.into(), at: p(Nm::ZERO, Nm::ZERO) })).unwrap();
    let f = fp(&b);
    // the pad: `pad->SetParent( editorFootprint )` -- where the board had it, (10, 5) + (1, 0) turned 90 degrees counter-clockwise, at its board angle
    let pad = f.pads.iter().find(|p| p.number == "1").expect("the pad");
    assert_eq!(pad.at, p(mm(10.0), mm(4.0)));
    // the graphic: in the footprint's frame, as the file holds it
    assert!(f.graphics.iter().any(|g| matches!(g, Shape::Segment { start, end, .. } if *start == p(mm(-1.0), Nm::ZERO) && *end == p(mm(1.0), Nm::ZERO))));
    // the text: its angle less the footprint's (`SetTextAngle( GetTextAngle() - aClipFootprint->GetOrientation() )`)
    let q = f.texts.iter().find(|t| t.content == "Q").expect("the footprint's text");
    assert_eq!((q.at, q.angle), (p(Nm::ZERO, mm(-2.0)), 0));
    // the dimension: from the board's frame into the footprint's -- (10, 5)-(12, 5) less the footprint's place, turned back 90 degrees
    let dim = f.items.dimensions.iter().find(|d| !before.items.dimensions.iter().any(|o| o.id == d.id)).expect("the dimension");
    assert_eq!((dim.start, dim.end), (p(Nm::ZERO, Nm::ZERO), p(Nm::ZERO, mm(2.0))));
    // no point (`Points()` is not taken)
    assert_eq!(f.items.points.len(), before.items.points.len());
    // the board's own drawings, where the board had them, with their looks; not its track
    let line = f.graphics.iter().find(|g| matches!(g, Shape::Segment { start, .. } if *start == p(mm(20.0), mm(20.0)))).expect("the board's line");
    assert_eq!(f.kicad.stroke_types.get(line.id()).map(String::as_str), Some("dash"));
    let text = f.texts.iter().find(|t| t.content == "BOARD").expect("the board's text");
    // and its group of them, the footprint (not pasted) gone from it
    let g = f.items.groups.iter().find(|g| g.member_ids.contains(&line.id().to_string())).expect("the board's group");
    assert_eq!(g.member_ids.len(), 2);
    assert!(g.member_ids.contains(&text.id));
}

/// UI-LEFT-3 F3-3: with a group of the footprint entered, what a command adds joins it (`BOARD_COMMIT::Push`'s `enteredGroup->AddItem( boardItem )`): a drawn line, a
/// pasted group as itself; a point does not (it is not groupable), nor an item a command left in a group of its own.
#[test]
fn what_is_added_while_a_footprint_group_is_entered_joins_that_group() {
    let m = model(&["F.Cu", "B.Cu"]);
    let mut b = with_footprint(&m);
    let group = fp(&b).items.groups[0].id.clone();
    let in_group = |cmd: Cmd| Cmd::InGroup { group: group.clone(), cmd: Box::new(cmd) };
    let line = Shape::Segment { id: String::new(), layer: "F.SilkS".into(), stroke_width: mm(0.12), filled: false, start: p(mm(7.0), Nm::ZERO), end: p(mm(9.0), Nm::ZERO) };
    let wrapped = in_group(Cmd::AddFootprintGraphic { footprint: NAME.into(), shape: line });
    assert_eq!(wrapped.domain(), Domain::FootprintEditor, "the footprint editor's undo history keeps it");
    b.apply(&wrapped).unwrap();
    let f = fp(&b);
    let added = f.graphics.iter().find(|g| matches!(g, Shape::Segment { start, .. } if *start == p(mm(7.0), Nm::ZERO))).unwrap().id().to_string();
    let members = |b: &Board<'_>| fp(b).items.groups.iter().find(|g| g.id == group).unwrap().member_ids.clone();
    assert!(members(&b).contains(&added), "the drawn line joined the entered group");
    // a point is not groupable
    b.apply(&in_group(Cmd::FpItem(FpItemCmd::Add { footprint: NAME.into(), item: FpItem::Point(eda_model::pcb_items::PcbPoint::new("F.Fab", p(mm(3.0), mm(3.0)))) }))).unwrap();
    let point = fp(&b).items.points[0].id.clone();
    assert!(!members(&b).contains(&point));
    // a pasted group joins as itself, its members staying in it
    let clip = clipboard_footprint(fp(&b), &[group.clone()], p(Nm::ZERO, Nm::ZERO), "00000000-0000-4000-8000-000000000002").unwrap();
    let groups_before: Vec<String> = fp(&b).items.groups.iter().map(|g| g.id.clone()).collect();
    b.apply(&in_group(Cmd::FpItem(FpItemCmd::Paste { footprint: NAME.into(), text: eda_kicad::export_kicad_mod(&clip), at: p(mm(1.0), Nm::ZERO) }))).unwrap();
    let pasted = fp(&b).items.groups.iter().find(|g| !groups_before.contains(&g.id)).expect("the pasted group").clone();
    assert!(members(&b).contains(&pasted.id), "the pasted group is a member of the entered one");
    assert_eq!(pasted.member_ids.len(), 3, "its line, its text and the line drawn into it stay in it");
    assert!(!members(&b).iter().any(|m| pasted.member_ids.contains(m)));
    // with no group of that id the command is applied all the same
    let other = Cmd::InGroup { group: "nothing".into(), cmd: Box::new(Cmd::AddFootprintGraphic { footprint: NAME.into(), shape: Shape::Circle { id: String::new(), layer: "F.Fab".into(), stroke_width: mm(0.1), filled: false, center: p(mm(20.0), Nm::ZERO), end: p(mm(21.0), Nm::ZERO) } }) };
    b.apply(&other).unwrap();
    assert!(fp(&b).graphics.iter().any(|g| matches!(g, Shape::Circle { center, .. } if *center == p(mm(20.0), Nm::ZERO))));
}

/// UI-LEFT-3 F3-10: with cells of a footprint table selected, a board's clipboard holding a table pastes into them (`PCB_CONTROL::Paste`'s `hasTableCells` branch,
/// `pasteCellsIntoSelection`), and a selection KiCad refuses is refused with its words.
#[test]
fn a_boards_table_pastes_into_the_selected_cells_of_a_footprint_table() {
    use super::table_clipboard_tests::{add_pcb, pcb_table};
    use eda_model::table::cell_item_id;
    let m = model(&["F.Cu", "B.Cu"]);
    let mut b = with_footprint(&m);
    let mut source = pcb_table(1, 2, "F.SilkS");
    for c in source.cells.iter_mut() {
        c.text = format!("new {}", c.text);
    }
    let src = add_pcb(&mut b, source);
    let text = eda_kicad::export_pcb_clipboard(b.design(), &m, &[src], p(Nm::ZERO, Nm::ZERO)).unwrap();
    b.apply(&Cmd::FpItem(FpItemCmd::Add { footprint: NAME.into(), item: FpItem::Table(pcb_table(2, 2, "F.Fab")) })).unwrap();
    let target = fp(&b).items.tables[0].id.clone();
    let paste = |ids: Vec<String>| Cmd::FpItem(FpItemCmd::PasteCells { footprint: NAME.into(), ids, text: text.clone() });
    b.apply(&paste(vec![cell_item_id(&target, 1, 0)])).unwrap();
    let t = fp(&b).items.tables[0].clone();
    assert_eq!(t.texts(), vec![vec!["r0c0", "r0c1"], vec!["new r0c0", "new r0c1"]]);
    let e = b.apply(&paste(vec![cell_item_id(&target, 0, 0), cell_item_id(&target, 1, 1)])).unwrap_err();
    assert_eq!(e[0].hint.clone().unwrap_or_default(), "Selected cells must form a contiguous block");
    // a footprint's clipboard is no board: no table is looked for in it
    let fp_clip = eda_kicad::export_kicad_mod(&clipboard_footprint(fp(&b), &[target.clone()], p(Nm::ZERO, Nm::ZERO), "00000000-0000-4000-8000-000000000003").unwrap());
    let e = b.apply(&Cmd::FpItem(FpItemCmd::PasteCells { footprint: NAME.into(), ids: vec![cell_item_id(&target, 0, 0)], text: fp_clip })).unwrap_err();
    assert_eq!(e[0].check, "ops_no_clipboard_table");
}
