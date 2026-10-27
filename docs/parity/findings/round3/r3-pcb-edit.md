# Findings round 3: PCB editing and its UI (`r3-pcb-edit`)

Reviewer `r3-pcb-edit`, 2026-10-27/28. This is a findings-only review: no code was changed, nothing was built and no server was started.
- **Code read:** main's checkout `/home/team_braneboard_com/work/agentic-eda`, HEAD `5cac9357`.
- **KiCad compared against:** `8303b2ad` (`/home/team_braneboard_com/src/kicad-src-8303b2ad`), and `25ec2335` for KiCad 10 items.
- **Scratch:** `/tmp/findings-r3-pcb-edit.4rt7`.
- **Paths:** ours are from the repo root, and `web/` means `web/studio/src/`. KiCad paths are from the KiCad source root.

Scope: GAPS items 8, 9, 10, 14-18, 21, 22, 29 and 35.
- **Priority (a):** the round-2 merge `task-r2-pcb-edit` (PARITY-pcb section 55, the commits since 2026-10-26). It is covered in Parts A (transforms), B (clipboard and groups) and C (dialogs and selection).
- **Priority (b):** rows marked ported or identical that no earlier round reviewed. These are covered in Part D: Cleanup Tracks & Vias, Select Connection, microwave tools, Break Track, global text edits, shape modification.

Finding ids are round-3 ids (`A-1`, ...), distinct from round 2's `A1`.

---

# Part A: transforms, zone cutout, dimensions, flip, orientation (round-2 R1-R10, C1, C4, A2)

### A-1 (medium). Flip onto (or within) a user layer typed front or back does not toggle the mirror of a text or a dimension, so the C4 fix is incomplete
- **Status:** **Fixed** on task-r3-pcb-edit: 904bc77e (texts and text boxes; a dimension has no mirror flag here, left open).
- **What differs.**
  - KiCad, `pcbnew/pcb_text.cpp::PCB_TEXT::Flip`:
    ```
    SetLayer( GetBoard()->FlipLayer( GetLayer() ) );
    if( IsSideSpecific() ) SetMirrored( !IsMirrored() );
    ```
  - `pcbnew/board_item.cpp::BOARD_ITEM::IsSideSpecific` is true for `SideSpecificMask()` layers, and also when `board->GetLayerType( m_layer )` is `LT_FRONT` or `LT_BACK`.
  - `PCB_TEXTBOX::Flip` and `PCB_DIMENSION_BASE::Mirror` run the same `if( IsSideSpecific() ) SetMirrored( !IsMirrored() );`.
  - Ours, texts: `crates/ops/src/pcb_transform.rs:503-509` (`transform_text`) toggles only when `side_specific(&t.layer)` (`pcb_transform.rs:106`) holds. That test looks at layer names only (F., B., In*.Cu). The C4 remap to the paired layer (b5e0d278, about lines 300-325) runs after `transform_text`, and never toggles.
  - Ours, dimensions: `crates/kicad/src/pcb_items.rs:333` writes `(justify mirror)` only for `d.layer.starts_with("B.")`.
- **How to see it.**
  - Setup: User.1 is typed front and named "F.Notes"; User.2 is typed back and named "B.Notes".
  - A text on User.1, flipped left-right, lands on User.2 with `mirror == false`. KiCad gives `true`.
  - A text on User.5 (typed front, no pair) also stays unmirrored. KiCad toggles it.
  - Test: extend `pcb_transform_tests.rs::flip_takes_a_front_user_layer_to_its_back_pair` with `assert!(texts[0].mirror)`. Add a text on User.1 asserting `layer == "User.2" && mirror`.
- **Already known?** No. GAPS (line 397) lists only "a footprint's own drawings on a user layer".

### A-2 (medium). Swap Layers leaves a tuning pattern (and a via stitch) on its old layer while its tracks move
- **Status:** **Fixed** on task-r3-pcb-edit: 904bc77e.
- **What differs.**
  - KiCad, `pcbnew/tools/global_edit_tool.cpp::GLOBAL_EDIT_TOOL::SwapLayers`: after the tracks are swapped, `for( PCB_GENERATOR* generator : frame()->GetBoard()->Generators() ) hasChanges |= swapBoardItem( generator, layerMap );`.
  - `PCB_GENERATOR::GetLayerSet` is `PCB_GROUP::GetLayerSet() | LSET( { GetLayer() } )`: its members' layers (already moved) plus its own.
  - `swapBoardItem` maps that set. When it collapses to one layer, `BOARD_ITEM::SetLayerSet` calls `SetLayer`, so a one-way move F.Cu to In1.Cu puts the pattern on In1.Cu.
  - A KiCad quirk to keep: for a two-way F.Cu<->B.Cu swap, {F,B} maps to {B,F}, so the pattern stays where it is. Port `swapBoardItem` on the generator's layer set, not a plain layer remap.
  - Ours: `crates/ops/src/lib.rs:4236-4300` (`swap_layers`) never touches `drawings.tuning_patterns[*].layer` (`crates/model/src/tuning.rs:186`) or `board_items.via_stitches[*].layer`. The file then has `(generated .. (layer "F.Cu"))` while its member tracks are on In1.Cu, so KiCad's regenerate tunes on the wrong layer.
- **How to see it.**
  - Test `swap_layers_moves_a_tuning_pattern_one_way`: F.Cu to In1.Cu, every other row unchanged; the pattern's layer is "In1.Cu".
  - Second test: an F<->B swap; the layer stays "F.Cu".
- **Already known?** No. R9 covered tables and barcodes only.

### A-3 (medium). A rounded rectangle turned by a non-quarter angle: KiCad's polygon keeps four true arcs; ours is flattened at a fixed 5 µm
- **Status:** **Fixed** in part on task-r3-pcb-edit: 904bc77e (the board's maximum error; the arcs need arcs in the IR's polygon, open; Create Array is task-r2-misc-leftovers', Edit Pad as Graphic Shapes open).
- **What differs.**
  - KiCad, `common/eda_shape.cpp::EDA_SHAPE::rotate`: `rr.TransformToPolygon( GetPolyShape(), getMaxError() )`.
  - `libs/kimath/src/geometry/roundrect.cpp::ROUNDRECT::TransformToPolygon` builds the corners with `outline.Append( MakeCornerArcCw90(..), maxError )`.
  - `SHAPE_LINE_CHAIN::Append( const SHAPE_ARC&, int )` keeps the arc (`m_arcs.push_back( aArc )`), and `PCB_IO_KICAD_SEXPR::formatPolyPts` writes `(arc (start ..) (mid ..) (end ..))` in the `gr_poly`'s `pts`.
  - Ours: `crates/ops/src/pcb_transform.rs:459` stores only the points of `rounded_rect_ring(.., MAX_ERROR_NM)`. That error is a constant 5000 nm (`crates/model/src/outline.rs:29`), not the board's `max_error_nm` (`BOARD_ITEM::GetMaxError`).
  - Root cause: the IR polygon has no arcs (the importer flattens `gr_poly` arcs too, `crates/kicad/src/import.rs:2622`).
- **How to see it.**
  - Rotate a 10 x 5 mm rectangle with 2 mm corners by 45 degrees and export. KiCad's `gr_poly` has four `(arc ..)` entries; ours has none, and KiCad's point editor shows dozens of vertices.
  - The added test `a_rounded_rectangle_turned_45_degrees_is_the_polygon_of_its_rounded_rectangle` compares against our own `rounded_rect_ring`, so it cannot catch this.
  - Smallest fix test: with `board.max_error_nm = 1000`, the vertex count equals that of `rounded_rect_ring(.., 1000)`.
- **Already known?** Not for Rotate. GAPS 397 names the four-corner leftover in Create Array and Repeat Layout only. The same four-corner shortcut is also in `crates/ops/src/fp_array.rs:83` (Footprint Editor Create Array) and `crates/ops/src/pad_edit.rs:98-105` (Edit Pad as Graphic Shapes; KiCad's explode also calls `shape->Rotate`).

### A-4 (low). The canvas paints a dimension's text turned the wrong way (vertical and diagonal dimensions)
- **Status:** **Fixed** on task-r3-pcb-edit: 5300a295.
- **What differs.**
  - `crates/connectivity/src/dimension.rs:279` and `:359-366` compute KiCad's counter-clockwise angle (`PCB_DIM_ALIGNED::updateText`: `FULL_CIRCLE - EDA_ANGLE( crossbarCenter )`; `ANGLE_VERTICAL` for a vertical orthogonal crossbar).
  - `web/components/canvas/painter.ts:1026` draws `computed_text_angle` with no sign flip, and its comment calls the angle "canvas-native clockwise-positive".
  - `drawTexts` (`painter.ts:983`) negates the same convention: `(-t.angle / 1000)`.
  - `web/kicad-port/pcbItems.ts:193` carries the same wrong assumption, but a centred box is symmetric, so the bounds are unaffected.
  - The file side is right.
- **How to see it.** A vertical orthogonal dimension reads top to bottom here and bottom to top in KiCad.
  - Painter test: `computed_text_angle: 90000` gives `drawStrokeText` an angle of `-Math.PI/2`.
- **Already known?** No. This predates round 2.

### A-5 (low). A kept-aligned dimension's text angle is written to the millidegree; KiCad writes `{:.10g}`
- **Status:** Open (low; KiCad recomputes the angle on load).
- **What differs.** `common/eda_units.cpp::FormatAngle` is `fmt::format( "{:.10g}", aAngle.AsDegrees() )`, which gives 33.69006753. We write 33.69 (`angle_of`, after R10). KiCad recomputes the angle on load, so only the text differs.
- **Already known?** No.

### A-6 (low). Delete from a zone cutout snaps the cursor to the grid even with grid snapping off
- **Status:** Not applicable here: the studio has no grid-snapping switch (`gal->GetGridSnapping()`); Ctrl turns the grid off per move only.
- **What differs.**
  - KiCad, `common/view/wx_view_controls.cpp::GetCursorPosition`: snaps only `if( aEnableSnapping && gal->GetGridSnapping() )`.
  - Ours: `web/actions/useActionRunner.ts` `deleteRefs` always calls `snapPoint(.., state.gridNm)`.
- **How to see it.** With snapping off, a cursor just inside a cutout's edge can snap outside it, and the whole zone goes.
- **Already known?** No.

### Part A: checked and found to match (do not re-review)
- **R1 (fc9a579e):**
  - `DeleteItems`' `PCB_ZONE_T` case: `!aIsCut && aItems.GetSize() == 1` after the lock filter; `HitTestCutout`, first hole, only that hole removed, the zone reselected.
  - Cut and several items remove the zone.
  - The no-undo difference still holds in 25ec2335.
  - The Interactive Delete picker's raw cursor.
- **R2/R3/R10 (8cce0125):**
  - `PCB_DIMENSION_BASE::Rotate`'s `GetTextAngle() + aAngle`.
  - `GetValueText`'s precision levels (INCH p-4, MILS max(0,p-7), MM p-5).
  - Trailing-zero suppression (equal to 25ec2335's `precision > 0`; 8303b2ad's integer-stripping bug not copied).
  - The ten `ENUM_MAP<DIM_PRECISION>` labels.
  - Precision 0-9 both ways.
  - The extension height truncation.
- **C1 (81948dba):** `IsCardinal`, `ROUNDRECT` normalising and the radius clamp, point order, the `maxError / 5` step (apart from A-3); the radius dropped; leftovers documented.
- **C4 (b5e0d278):** `BOARD::recalcOpposites` line by line (both passes, the bounds; identical in 25ec2335); the kinds remapped.
- **R4 (8c0c6a33):** `PCB_TEXT::Mirror` (justification flipped only at exactly horizontal or vertical), `PCB_POINT::Mirror`, `MirrorableItems`.
- **R6 (df133408):** the turn about `BestDragOrigin( originalMousePos, .. )`. The commit has no test.
- **R7 (900ef80b):** `itemPassesFilter`'s `PCB_GENERATOR_T` branch (8303b2ad and 25ec2335).
- **R8 (de75bf1e):** `PCB_SHAPE::GetBoundingBox`'s `max( 0, GetWidth() ) / 2`. We take the widest courtyard line; negligible.
- **R9 (dcacc7df):** tables and barcodes; footprints are not swapped; only enabled copper layers mapped.
- **A2 (181caa86):** `kicad_footprint_orientation` equals the writer's angle; `Normalize180` in (-180, 180]; Properties and Footprint Properties turn by the difference before the side flip.

---

# Part B: clipboard, plain-text paste, Copy as Text, groups (round-2 C2, C7, B2, A1, A3, A4, A9, A10, A11)

### B-1 (high). Cut of selected table cells deletes the whole table; KiCad only clears those cells. Copy of cells copies the whole table
- **Status:** **Fixed** on task-r3-pcb-edit: 291dfd71.
- **What differs.**
  - Ours: Cut (`web/actions/useActionRunner.ts:346-354`) runs the selection through `editableSelection(.., { keepPads: true })`. `web/kicad-port/pcbTransform.ts:73` turns every table cell into its table's id (`FilterCollectorForTableCells`), so `deleteRefs([tableId], true)` deletes the table and `export_pcb_clipboard` writes it whole.
  - Plain Delete is right: `web/kicad-port/deleteCmds.ts:72-84` gets the raw cell ids.
  - KiCad's copy (`pcbnew/tools/edit_tool.cpp:3723-3730`, `EDIT_TOOL::copyToClipboard`) filters only by hierarchy, markers and (for Cut) locks. There is no table-cell filter.
  - KiCad's Cut then runs `DeleteItems` on exactly that selection. Its `case PCB_TABLECELL_T:` is `// Clear contents of table cell`, `commit.Modify( board_item ); static_cast<PCB_TABLECELL*>( board_item )->SetText( wxEmptyString );`.
  - `pcbnew/kicad_clipboard.cpp::CLIPBOARD_IO::SaveSelection` promotes a cell to a clone of its table (`promotedTables`), then `deleteUnselectedCells` keeps only the block between the selected cells' min and max rows and columns.
- **How to see it.** On a 3 x 3 table, select cell (0,0) and press Ctrl+X. KiCad keeps the table and empties the cell; ours deletes the table.
  - Test `pcbCut.test.ts`, "cutting a table cell clears it, the table stays": the commands are a cell text edit to "", not a table delete.
  - Rust `clipboard.rs` test: copying the cells of one row writes a 1-row table.
- **Already known?** In part. PARITY-pcb line 1769 says "Copy, Cut, Paste and Duplicate of a table or of cells" is not done. Section 55 says a table is copied. The data loss on Cut is listed nowhere.

### B-2 (medium). Copy of a group whose only member is a footprint is written as a bare footprint, so the group and the pads' nets are lost
- **Status:** **Fixed** on task-r3-pcb-edit: 291dfd71.
- **What differs.**
  - Ours: `crates/kicad/src/clipboard.rs:356` tests `picked.total() == 1 && picked.footprints.len() == 1`. `total()` (lines 159-174) does not count `groups`, so a selected group of one (kept since A1) takes the lone-footprint branch: no `(group ..)`, pads on no net.
  - KiCad: `if( aSelected.Size() == 1 && aSelected.Front()->Type() == PCB_FOOTPRINT_T )`. A selected group is `PCB_GROUP_T`, so KiCad takes the board branch (`static_cast<PCB_GROUP*>( boardItem )->DeepClone()`, then each descendant formatted, pads with their nets).
- **How to see it.** Group R1 alone, select the group, copy and paste. KiCad's paste is in a new group with its nets; ours has neither.
  - Test `a_group_of_one_footprint_copies_as_a_board_with_its_group`: the text starts with `(kicad_pcb` and contains `(group` and `(net `.
- **Already known?** No.

### B-3 (low). Group Items and the Group Properties "Locked" box ignore the lock of the item's parent group
- **Status:** **Fixed** on task-r3-pcb-edit: 8b3a2a07.
- **What differs.**
  - KiCad, `PCB_GROUP_TOOL::Group`: `if( static_cast<BOARD_ITEM*>( eda_item )->IsLocked() ) group->SetLocked( true );`. `pcbnew/board_item.cpp::BOARD_ITEM::IsLocked` starts with `if( EDA_GROUP* group = GetParentGroup() ) { if( group->AsEdaItem()->IsLocked() ) return true; }`.
  - KiCad, `DIALOG_GROUP_PROPERTIES` constructor: `m_locked->SetValue( aGroup->AsEdaItem()->IsLocked() );`.
  - The writer `format( const PCB_GROUP* )` writes `(locked yes)` on `IsLocked()`, so a group inside a locked group is written as locked.
  - Ours tests only the item's own lock: `crates/ops/src/pcb_groups.rs:76` (`dr.locked_ids.contains(id)`), `web/components/CommonDialogs.tsx:38`, and `crates/kicad/src/pcb_items.rs:399`.
- **How to see it.** Lock group G holding T1 and T2, enter G, select both, press Ctrl+G. KiCad's new group is locked; ours is not.
  - Test `group_items_inside_a_locked_group_is_locked`.
- **Already known?** No. A4 handled the item's own lock only.

### B-4 (low). Table cells can become group members; points, grid items and drill maps are outside the group bookkeeping
- **Status:** **Fixed** on task-r3-pcb-edit: ce191e83.
- **What differs.**
  - **Table cells:** `web/state/store.tsx:2474-2477` filters out only pads and fields, and `make_group` checks no ids, so selected cells become members under their cell ids. Such a group has no uuid to write, so it is lost on save. KiCad's `Group` filter drops anything that fails `IsGroupableType()`, and `PCB_TABLECELL_T` fails it.
  - **Points, grid items, drill maps:** the importer puts these in groups (`crates/kicad/src/import_items.rs:150-153`), as 25ec2335's `IsGroupableType` allows (`PCB_POINT_T`, `PCB_GRID_ITEM_T`, `PCB_DRILL_CHART_T`, `PCB_DRILL_MAP_T`). But `universe()` (`crates/ops/src/pcb_groups.rs:41-66`) leaves them out. So `tidy_groups` never removes a deleted one from its group, and `in_group` never adds new ones to the entered group.
- **How to see it.** Import a KiCad 10 board with a group {point, line} and delete the point: the group still lists the point's id.
  - Test `deleting_a_grouped_point_leaves_its_group`.
- **Already known?** No.

### B-5 (low). Plain-text paste of whitespace pastes the old session copy; the long-text threshold counts UTF-16 units
- **Status:** **Fixed** on task-r3-pcb-edit: 30547418 (an empty clipboard still falls back to the session copy, the browser's case).
- **What differs.**
  - Ours: `web/state/store.tsx:2557` tests `system.trim() !== ""`, so a clipboard of "  " falls back to the in-session board copy.
  - KiCad, `pcbnew/tools/pcb_control.cpp::PCB_CONTROL::Paste`: `if( clipText.empty() ) return 0;`, and any other text becomes a `PCB_TEXT`.
  - The threshold `clipText.size() > m_MaxPastedTextLength` counts code points (wxString is UTF-32). Ours (`plainTextPaste.ts`) uses JS `.length`, so 60 emoji ask "Pasting a long text text string may be very slow.  Do you want to continue?".
- **Already known?** No.

### B-6 (low). Copy as Text on the board has no hover fallback
- **Status:** **Fixed** on task-r3-pcb-edit: e373e87d.
- **What differs.**
  - Ours: `web/actions/useActionRunner.ts:1644` uses `state.selection` on the board; only the schematic picks the hovered item.
  - KiCad, `EDIT_TOOL::copyToClipboardAsText`: `RequestSelection` falls back to the item under the cursor, then `if( selection.IsHover() ) m_selectionTool->ClearSelection();`.
- **Already known?** No (B2 fixed the item types).

### B-7 (low). A copied pad pasted while its parent reference is not on the board brings back the whole parent footprint
- **Status:** Open.
- **What differs.**
  - Ours: the pad's clipboard footprint keeps the parent's reference (documented in section 55). `crates/ops/src/pcb_paste.rs:713` (`reuse_unplaced`, set for every clipboard footprint at line 586) re-places part R1 as the model knows it, with all its pads, and ignores the one-pad definition.
  - KiCad pastes a fresh, unnamed one-pad footprint.
- **How to see it.** Copy pad R1.1, delete R1, paste: R1 comes back whole.
- **Already known?** The reference difference is documented; this consequence of it is not.

### B-8 (low). Paste of a clipboard image is missing
- **Status:** Open.
- **What differs.**
  - KiCad, `PCB_CONTROL::Paste`, when the clipboard text does not parse: `if( std::unique_ptr<wxBitmap> clipImg = GetImageFromClipboard() )` makes a `PCB_REFERENCE_IMAGE`.
  - Ours reads text only (`readClipboardText`, `web/api/libraryClient.ts:95`).
- **Already known?** Not in GAPS or PARITY.

### B-9 (low). A footprint's nested groups are flattened or dropped on import
- **Status:** Open (overlaps r2-board-io F7/F14).
- **What differs.**
  - Ours: `crates/kicad/src/import.rs:1731-1736` keeps only members found in `slots` (art, text, field, pad, courtyard). A member that is another group's uuid is dropped, so an outer group holding only groups disappears.
  - KiCad, `pcb_io_kicad_sexpr_parser.cpp::resolveGroups` with a footprint parent: "Keep the footprint lookup map in sync with newly added groups" (every group added to `fpItemMap`).
- **Already known?** No. This overlaps r2-board-io F7/F14.

### B-10 (low). Group Properties writes a library link KiCad would not write
- **Status:** Open.
- **What differs.**
  - Ours: `fix_lib_link` (`crates/model/src/groups.rs`) turns "MyBlock" or "Lib:" into a link, and `write_group` (`crates/kicad/src/pcb_items.rs:402`) writes `(lib_id ..)` whenever the link is non-empty.
  - KiCad writes it only `if( aGroup->HasDesignBlockLink() )`, i.e. `m_designBlockLibId.IsValid()`, which needs both names. Our reader drops such a link too (`check_design_block_id`), so the outcome on reload is the same; only the file differs.
  - Side note: KiCad's parser throws "Group library link %s contains invalid character '%c'". Our reader ignores bad links, and `check_design_block_id` misses `<`, `>` and `"`.
- **Already known?** No.

### Part B: checked and found to match (do not re-review)
- **`SaveSelection`, board branch:**
  - A field becomes a `PCB_TEXT` with position, layer, attributes, `GetDrawRotation`, and `${VALUE}`/`${REFERENCE}` resolved; the keep-upright range (-90, 90].
  - A pad goes in its own footprint and keeps its net.
  - Everything is unlocked, group descendants too.
  - The offset is minus the reference point; groups are written after their members.
- **Lone footprint:** pads on no net, unlocked.
- **Cut:**
  - Deletes only after a good copy.
  - Locked items filtered on Cut and kept on Copy.
  - A pad is copied but not deleted.
  - `FilterCollectorForHierarchy` handled by de-duplication.
- **Plain-text paste:** not trimmed; 1.27 mm (`DEFAULT_SIZE_TEXT`); the automatic pen `KiROUND(size/8)`; the active layer; the 100-character question word for word; the anchor at the text's position.
- **Copy as Text:** the types, cells tab-separated and rows newline-separated, each item trimmed, items joined with newlines.
- **`resolveGroups` (board):** every group kept whatever its size; an empty one not written; the lock read and written; generators as members.
- **8303b2ad `IsGroupableType` against `universe`:** match (see B-4 for 25ec2335).
- **`PCB_GROUP_TOOL::Group`:** two or more items, parent-footprint items filtered, the new group selected.
- **`DIALOG_GROUP_PROPERTIES::TransferDataFromWindow`:** Locked shown for `PCB_GROUP_T` only; `LIB_ID::Parse( .., true )` never fails; the group selected after.
- **`format( PCB_GROUP )`:** token order.

---

# Part C: property dialogs, drawing tools, selection, context menu, Net Inspector (round-2 C3, A5, A8, C5, C6, C8, C9, B1, B3-B6, R5)

### C-1 (high). On the board, Delete and Undo while a zone, rule area or graphic polygon is being drawn do not remove the last corner
- **Status:** **Fixed** on task-r3-pcb-edit: 5f553a9d.
- **What differs.**
  - KiCad, `pcbnew/tools/drawing_tool.cpp::DRAWING_TOOL::DrawZone` (lines 3608-3622). It serves zones, rule areas, cutouts, similar zones and graphic polygons:
    ```
    else if( started && (   evt->IsAction( &PCB_ACTIONS::deleteLastPoint )
                         || evt->IsAction( &ACTIONS::doDelete )
                         || evt->IsAction( &ACTIONS::undo ) ) )
    {
        if( std::optional<VECTOR2I> last = polyGeomMgr.DeleteLastCorner() ) { ... }
        else { cleanup(); }
    }
    ```
  - **Undo:** `web/actions/useActionRunner.ts:649` (added by 68e1c0f0) sends every kind in `BOARD_DRAWING_KINDS` (`web/kicad-port/drawingCancel.ts:15`, `"shape", "zone", ...`) to `ESCAPE`. That drops the whole outline; KiCad drops one corner.
  - **Delete:** `useActionRunner.ts:358-361` checks for a polygon in progress on the schematic tab only. On the board, Delete during a zone goes to `requestSelection()` and deletes the selection or the item under the cursor, leaving the zone half-drawn. The footprint and symbol editors guard this case (`editorFrameActions.ts:105-108`).
  - Backspace (`deleteLastPoint`) is already right (`useActionRunner.ts:1951-1970`).
- **How to see it.** Arm Add Filled Zone, click three corners over a track, press Delete. KiCad drops the third corner; ours deletes the track. With Ctrl+Z, ours drops the whole outline.
  - Test `drawingCancel.test.ts`, "Undo or Delete with a zone or polygon started removes the last corner (DrawZone)": `{kind:"zone", pts:[a,b,c]}` becomes `pts [a,b]`, and no board item is deleted.
- **Already known?** No.

### C-2 (medium). Track & Via Properties changes the net without KiCad's short and pad questions; pads are never updated
- **Status:** **Fixed** on task-r3-pcb-edit: 3d208446.
- **What differs.**
  - KiCad, `pcbnew/dialogs/dialog_track_via_properties.cpp::TransferDataFromWindow` (lines 1567-1637) gathers `shortingNets` and `changingPads` over the connected tracks.
  - `confirmShortingNets` asks "Applying these changes will short net %s with %s." or "... will short net %s with other nets.". The title is "Confirmation", the buttons "Apply Anyway" / "Cancel Changes", and Cancel calls `commit.Revert()`.
  - `confirmPadChange` asks one of:
    - "Changing the net will also update %s pad %s to %s."
    - "Changing the net will also update %s pad %s and %s pad %s to %s."
    - "Changing the net will also update %lu connected pads to %s."
  - Its buttons are "Change Nets" / "Leave Nets Unchanged", and "Change Nets" moves the pads too.
  - Ours: `web/kicad-port/trackViaDialog.ts:79` sends `set_item_net` on the selected ids only.
- **How to see it.** Set a track that ends on R1.1 (net A) to net B. KiCad asks and moves the pad to B; ours leaves a short.
  - Test `trackViaDialog.test.ts`, "a net change on a track touching a pad of its net asks to update the pad".
- **Already known?** No (A5's leftovers are the via layer pair, the mask and backdrill only).

### C-3 (medium). Track & Via Properties' width error is not KiCad's message, and the title varies
- **Status:** **Fixed** on task-r3-pcb-edit: 3d208446.
- **What differs.**
  - KiCad: `m_trackWidth.Validate( GEOMETRY_MIN_SIZE, INT_MAX )`. `common/widgets/unit_binder.cpp::UNIT_BINDER::Validate` builds `_( "%s must be at least %s." )` from the label without its colon, giving "Track width must be at least 0.001 mm.".
  - Ours: `web/kicad-port/trackViaDialog.ts:73` says "Value must be greater than or equal to ...".
  - Low addendum: `TrackViaPropertiesDialog.tsx:77` retitles the dialog "Via Properties" or "Track Properties". KiCad's title is always "Track & Via Properties" (`dialog_track_via_properties_base.h:191`, no `SetTitle`).
- **How to see it.** Test: a width of 500 nm gives "Track width must be at least 0.001 mm.".
- **Already known?** No.

### C-4 (medium). Pad Properties lacks several `PAD::doCheckPad` warnings for fields the dialog edits; the minimum-drill text differs
- **Status:** **Fixed** on task-r3-pcb-edit: 8aefbd9e (still a toast, not an HTML_MESSAGE_BOX).
- **What differs.**
  - KiCad, `pcbnew/pad.cpp::PAD::doCheckPad` warns (shown by `padValuesOK` with "Warning: " in front):
    - "(negative local clearance values have no effect)"
    - "(negative solder mask clearance is larger than pad; no solder mask will be generated)"
    - "(negative solder paste margin is larger than pad; no solder paste mask will be generated)"
    - for an NPTH hole whose centre is outside the copper, "(pad hole not inside pad shape)".
  - Ours: `web/kicad-port/fpFields.ts::padCheck` has none of these, though `BoardPadPropertiesDialog.tsx:293-299` edits the clearance and the margins.
  - Our "not fully inside" test assumes a centred hole, but the dialog edits a shape offset (`:79`).
  - Low: KiCad's minimum-drill error uses `StringFromValue( 4, true )`, which is "0.000004 mm". Ours prints "0.000 mm", and `fpFields.test.ts` asserts "0.004 mm" against µm-scaled input, which is not KiCad's text.
  - Low: KiCad lists the messages in an `HTML_MESSAGE_BOX` titled "Pad Properties Errors" / "Pad Properties Warnings"; ours shows a toast.
- **How to see it.** Test: `padCheck` on a 1 mm pad with a -2 mm mask margin includes the mask warning.
- **Already known?** In part. Section 55 names the centred hole and the square corner, not these.

### C-5 (medium). Footprint Properties' Add Field ignores the board's Fab text defaults
- **Status:** **Fixed** on task-r3-pcb-edit: eb2172ab.
- **What differs.**
  - KiCad, `dialog_footprint_properties.cpp::OnAddField`: `newField.StyleFromSettings( m_frame->GetDesignSettings(), true )`. `PCB_TEXT::StyleFromSettings` takes the layer class's size, thickness, italic and keep-upright, and the mirror from `IsBackLayer`.
  - Ours: `web/kicad-port/fpFields.ts:186-189` (`newFieldLayout`) hard-codes 1 mm, 0.15 mm, upright, not italic.
- **How to see it.** Set the Fab text in Board Setup to 0.8 mm / 0.12 mm italic, then Add Field: KiCad's field takes those values, ours does not.
  - Test `fpFields.test.ts`, "a new field takes Board Setup's Fab text defaults".
- **Already known?** No (C8's Field4 is right).

### C-6 (low). Undo in the measure tool clears the ruler instead of undoing the board
- **Status:** **Fixed** on task-r3-pcb-edit: 5f553a9d.
- **What differs.** KiCad, `pcbnew/tools/pcb_viewer_tools.cpp::MeasureTool`, handles only `IsCancelInteractive()`, so Undo reaches the frame. 68e1c0f0 put `measure` in `BOARD_DRAWING_KINDS`, so Ctrl+Z becomes `ESCAPE`.
- **Already known?** No.

### C-7 (low). Doc drift: PARITY-pcb line 1086 gives a false reason for the read-only via rows
- **Status:** **Fixed** on task-r3-pcb-edit: 6d43abc3.
- **What differs.**
  - The row says Layer Top/Bottom and Via Type are read-only because "KiCad's own dialog does not either".
  - In KiCad, the Properties panel has setters for them (`pcb_track.cpp:3195-3198`), and `DIALOG_TRACK_VIA_PROPERTIES` sets the type (`m_ViaTypeChoice`, lines 1470-1475) and the layers.
  - The difference itself is known (A5's leftovers); only the reason is wrong.

### C-8 (low). ValueStringCompare when a number does not fully parse
- **Status:** **Fixed** on task-r3-pcb-edit: 6d43abc3.
- **What differs.**
  - KiCad: when `convertSeparators` fails, `wxString::ToCDouble` still keeps the strtod prefix (`1.2.3` reads as 1.2).
  - Ours: `web/kicad-port/valueStringCompare.ts:114-118` returns 0 unless the whole string parses.
- **Effect.** Only odd names such as `N1.2.3` against `N1.5` sort differently.

### Noted, not checked
`DrawZone` pops the tool after a zone is finished (`drawing_tool.cpp:3579-3587`). Check whether our zone tool stays armed (`web/components/Canvas.tsx:790`).

### Part C: checked and found to match (do not re-review)
- **C3 (4090011e):** `SetTrackSegmentWidth( .., true )`, opt else min > 0 else the item's own value, through the `TRACK_WIDTH` / `VIA_DIAMETER` / `HOLE_SIZE` constraints; the diff-pair and microvia implicit rules (`crates/drc/src/constraints.rs:843-883`). The span-typed microvia is a known GAPS item.
- **A5 (409c35da):**
  - Shared or indeterminate fields; an indeterminate field left alone; the 3-state lock; per-axis start/end/via X/Y.
  - `validateViaParameters` word for word, including "No via diameter defined.".
  - The width check only with tracks; one batch.
- **A8:** `ENUM_MAP<VIATYPE>`: Through, Blind, Buried, Micro.
- **C5:** the error/warning split of `padValuesOK`; "(PTH pad hole leaves no copper)", "(PTH pad hole not fully inside copper)", "Error: Through hole pad has no hole."; the 4 IU minimum; the ratio clamps.
- **C6:** Escape, and Undo with the item started, in `drawShape`, arc, Bézier, dimension, table, barcode and image placement (apart from C-1 and C-6).
- **C8:** `GetUserFieldName( m_fields->size() )` with four mandatory rows; the field starts hidden at (0,0) on F.Fab/B.Fab.
- **C9:** `GuessSelectionCandidates`' silk and courtyard first pass, line by line.
- **B1:** `showNetMenuFunc`, `MirrorableItems`, `canCopyAsText`.
- **B3-B5:**
  - `ValueStringCompare`, `SplitString`, `convertSeparators`, `ApplyModifier`, `IsOldSchoolDecimalSeparator` (apart from C-8).
  - The class filter on `GetName()` with "Effective for net: %s".
  - The report in model order.
- **B6:** our own migration, which is sound.
- **R5:** the dialog kept between calls and rebuilt only on a `GetUserUnits()` change; `s_anchorType` static.

---

# Part D: ported/identical rows no round reviewed (Cleanup Tracks & Vias, Select Connection, microwave, Break Track, global edits, shape modification)

### D-1 (high). Cleanup "Delete tracks unconnected at one end" removes a whole multi-point Track when only one of its segments dangles
- **Status:** **Fixed** on task-r3-pcb-edit: 19e8c01f.
- **What differs.**
  - Ours: `crates/connectivity/src/cleanup.rs:369` maps the dangling segment `trk#N` back to its Track (`d.id.split('#').next()`) and removes the whole Track.
  - KiCad, `pcbnew/tracks_cleaner.cpp::TRACKS_CLEANER::deleteDanglingTracks`, removes only the flagged `PCB_TRACK` (`m_brd->Remove( track ); m_commit.Removed( track );`) and iterates.
  - So the segments leading to a junction (a via, a branch, a pad at an inner vertex) are kept by KiCad and deleted here. The next pass then dangles what hung off them, which breaks connections.
  - Multi-point tracks are common: `out/design.json` has about 230 tracks of 3 to 16 points.
  - The module header documents whole-Track granularity only for the zero-length and duplicate passes.
- **How to see it.** Test `cleanup_dangling_removes_only_the_dangling_segment_of_a_polyline`:
  - Input: T1 = [R1.1 centre, Q=(5,0), R=(10,0)] and T2 = [Q, R2.1 centre], both on N.
  - KiCad deletes only Q-R. Ours removes T1, then T2, and R1 and R2 come apart.
  - Assert: T2 is kept, and [R1.1, Q] remains.
- **Already known?** No. The PARITY section 12 row says identical.

### D-2 (high). Cleanup "Delete tracks connecting different nets" removes a whole polyline (high); it skips no-net items and counts zones (medium)
- **Status:** **Fixed** on task-r3-pcb-edit: 19e8c01f.
- **What differs.** `crates/connectivity/src/cleanup.rs:310-350`:
  - (a) It removes the whole Track when one segment shorts. KiCad (`removeShortingTrackSegments`) removes the segment.
  - (b) It skips when either side has no net (`it.net.is_empty()` at line 317, `other.net.is_empty()` at line 322). KiCad tests `segment->GetNetCode() != testedPad->GetNetCode()` over `GetConnectedPads` and `GetConnectedTracks`, with no exception for net 0. A GND track on a no-net mounting pad is deleted by KiCad and kept here.
  - (c) Zones count as neighbours here. KiCad looks only at pads, tracks, arcs and vias.
- **How to see it.**
  - Test `shorting_track_to_a_no_net_pad_is_removed`: the kind is `ShortingTrack`.
  - A 3-point track whose last segment shorts keeps its first segment.
- **Already known?** No.

### D-3 (medium). Cleanup's dangling pass uses DRC's `aIgnoreTracksInPads = true`; KiCad's cleaner passes `false`
- **Status:** **Fixed** on task-r3-pcb-edit: 19e8c01f.
- **What differs.**
  - `crates/connectivity/src/dangling.rs:9` and `:160` hard-code `true`; `tracks_cleaner.cpp:304` calls `TestTrackEndpointDangling( track, false )`.
  - So a track with both ends under one pad and nothing else attached is deleted by KiCad and kept here.
  - The PARITY section 12 "identical" is wrong.
- **How to see it.** Test: a short track inside R1.1, with nothing else attached, is removed.
- **Already known?** No.

### D-4 (medium). "Delete tracks fully inside pads" ignores the track's width
- **Status:** **Fixed** on task-r3-pcb-edit: 19e8c01f.
- **What differs.**
  - Ours: `cleanup.rs:294` tests only that both ends are inside (`signed_distance <= 0`).
  - KiCad, `deleteTracksInPads`: `TransformShapeToPolygon(..)`, then `poly.BooleanSubtract( *pad->GetEffectivePolygon(..) )`, and deletes only if `poly.IsEmpty()`.
- **How to see it.** Test: a 0.8 mm wide track from (-0.3, 0) to (0.3, 0) mm in a 1 x 1 mm pad overhangs the pad and is kept.
- **Already known?** No. The PARITY row's "equivalent" is wrong.

### D-5 (medium). "Delete redundant vias" on a through-hole pad only at the pad's exact centre
- **Status:** **Fixed** on task-r3-pcb-edit: 19e8c01f (the upside-down duplicate too).
- **What differs.**
  - Ours: `cleanup.rs:231` tests `full_stack_pad_centers.contains(&v.at)` (the set is built at line 511).
  - KiCad uses `GetConnectedPads( via )` with `( pad->GetLayerSet() & all_cu ) == all_cu`: any touching through-hole pad counts.
  - Low addendum: duplicate vias are compared by their from/to strings, while KiCad compares `GetLayerSet()` and `GetViaType()`, so a via written upside down does not match.
- **How to see it.** Test: a via 0.3 mm off the centre of a 1.7 mm through-hole pad is removed.
- **Already known?** No.

### D-6 (medium). Select Connection (U) walks through pads, ignores layers, and misses branches at inner vertices
- **Status:** **Fixed** on task-r3-pcb-edit: 586d125c.
- **What differs.**
  - Ours: `web/kicad-port/pcbConnection.ts:58` keeps only each track's two ends. `web/kicad-port/expandConnection.ts:134` stops the junction stage only when more than two tracks or a via meet.
  - KiCad, `PCB_SELECTION_TOOL::selectAllConnectedTracks`, stops on `pt_count > 2 || gotVia || gotNonStartPad`. It counts only tracks on `layerSetCu`, and joins off-centre vias within their radius.
  - As a result:
    - (a) the first U press goes through a pad where two tracks meet;
    - (b) tracks on different layers that end at one point join without a via;
    - (c) a branch from an inner vertex of a multi-point Track is never reached.
- **How to see it.**
  - Test `stop_at_junction_halts_at_a_non_start_pad`: tracks A, B and C meet at R2.1, and A gives {A, B} (ours gives {A, B, C}).
  - Test `a_branch_at_an_inner_vertex_is_followed`.
- **Already known?** PARITY line 129 documents only "pads are start points".

### D-7 (medium). Microwave Gap, Stub and Polygonal Shape: pad numbers are swapped
- **Status:** **Fixed** on task-r3-pcb-edit: d7082fdb.
- **What differs.**
  - KiCad, `pcbnew/microwave/microwave_footprint.cpp::createBaseFootprint`: `footprint->Add( pad, ADD_MODE::INSERT )` (line 218). `FOOTPRINT::Add` with INSERT is `m_pads.push_front` (`footprint.cpp:1433`), so `Pads().begin()` is pad "2". The same holds in 25ec2335.
  - Hence:
    - Gap: pad "2" is on the left and "1" on the right.
    - Stub: `pad->SetNumber( wxT( "1" ) )` renames the pad "2" left at the origin, so both pads are "1".
    - Polygonal shape (`createPolygonShape`, `pad1 = *Pads().begin()`): pad "2" is on the left.
  - Ours: `crates/ops/src/microwave.rs:719-720`, `:733-734` and `:796-797` take `pads[0]` as "1".
- **How to see it.** Tests `stub_pads_are_both_numbered_1` and `gap_pad_2_is_on_the_left`.
- **Already known?** No. The PARITY section 41 rows are wrong.

### D-8 (medium). Break Track on a locked track is refused; KiCad asks and breaks it
- **Status:** **Fixed** on task-r3-pcb-edit: 04a71b87 (the do-not-show box and the break point's grid origin open).
- **What differs.**
  - Ours: `web/actions/pcbEditSweep.ts:441-443` shows a toast of our own, "The selected item is locked. Unlock it to break the track.".
  - KiCad, `pcbnew/router/router_tool.cpp::ROUTER_TOOL::InlineBreakTrack`: `KIDIALOG( frame(), _( "The selected item is locked." ), _( "Confirmation" ), wxOK | wxCANCEL | wxICON_WARNING )` with `SetOKLabel( _( "Break Track" ) )` and the do-not-show checkbox. Both halves stay locked.
  - KiCad clears the selection before this check; ours clears it only on success.
  - Low addendum: our break point is `round(p / grid) * grid`, which ignores the grid origin, the snap switch and Shift.
- **Already known?** No.

### D-9 (medium). Edit Text & Graphics Properties leaves out text boxes, table cells and barcodes
- **Status:** **Fixed** on task-r3-pcb-edit: ed91d108.
- **What differs.**
  - Ours: `web/kicad-port/globalEditTextGraphics.ts:66-67` takes `dr.shapes`, `dr.texts` and `dr.dimensions` only.
  - KiCad, `dialog_global_edit_text_and_graphics.cpp::TransferDataFromWindow`, takes:
    - `PCB_TEXT_T || PCB_TEXTBOX_T`, and table cells through `RunOnChildren`, under `m_boardText`;
    - `PCB_SHAPE_T || PCB_BARCODE_T` under `m_boardGraphics`.
- **Already known?** No. The PARITY section 13 row ("no table/barcode item") is stale since sections 38 and 39.

### D-10 (medium). Simplify Shapes deletes and re-adds a polygon, so it loses its id and group
- **Status:** **Fixed** on task-r3-pcb-edit: bbcd3bcb (a zone's holes are still not simplified).
- **What differs.**
  - Ours: `web/actions/pcbEditSweep.ts:331` sends `delete_shape` + `add_shape`, keeping only layer, width and fill.
  - KiCad, `EDIT_TOOL::SimplifyPolygons`: `commit.Modify( item ); poly.SimplifyOutlines(..)` on the same item.
  - This is the defect p6 F2 fixed elsewhere with `replace_shape`.
  - Low addendum: a zone's holes are not simplified (`SimplifyOutlines` runs over every path).
- **How to see it.** Test: a grouped polygon stays in its group after Simplify.
- **Already known?** No.

### D-11 (low). Polygon booleans: messages and the all-failed case
- **Status:** **Fixed** in part on task-r3-pcb-edit: e2cba0ac (the all-failed words; "Some of the polygons could not be ..." and the push with no success open).
- **What differs.**
  - Ours: `crates/ops/src/pcb_edit.rs:214` writes "unable to {verb} the selected polygons".
  - KiCad's `GetStatusMessage` texts are "Unable to merge the selected polygons." and "Some of the polygons could not be merged." (likewise subtracted and intersected).
  - With zero successes KiCad still pushes, so the first shape comes back as a polygon. We refuse.
  - Our "Select at least two polygons, rectangles or circles to ..." is our own wording.

### D-12 (low). Cleanup leftovers
- **Status:** **Fixed** in part on task-r3-pcb-edit: 19e8c01f ((a), (c), (d); (b) open: a merged track is new).
- (a) For duplicates, KiCad deletes the earlier-iterated track; we delete the later one.
- (b) A merged pair becomes a new Track with `id: ""` and no `pins`. KiCad modifies `aSeg1` in place, keeping its uuid, group and mask.
- (c) We compare arcs in the duplicate pass. KiCad's covers `PCB_TRACE_T` only.
- (d) `pad_geoms` puts an SMD pad on F.Cu or B.Cu by the footprint's side, not by the pad's own layers.

### Part D: checked and found to match (do not re-review)
- **`EDIT_TOOL::BooleanPolygons`:**
  - The kinds; the last-added item swapped to the front.
  - The Subtract retry by box area; the donor's width, layer and fill; Intersect's skip.
  - `GetRectCorners`; the circle polyline at `ARC_HIGH_DEF/5`.
  - `Finalize` (one shape per outline).
- **`EDIT_TOOL::ModifyLines`:** decomposition, `segmentCount`, both refusals word for word, `LINE_FILLET_ROUTINE::ProcessLinePair`, the status messages.
- **`HealShapes` / `ConnectBoardShapes`:** `connectPair` for every kind pair. Tiny difference: KiCad's kd-tree keeps the original ends, ours reads the moved ones.
- **`SimplifyPolygons` / `SHAPE_LINE_CHAIN::Simplify` / `TestSegmentHit`:** line for line (apart from D-10).
- **`EDIT_TOOL::FilletTracks`:** messages, the static radius from 0, the duplicate and collinear skips, `setIfPointOnSeg`.
- **`InlineBreakTrack`:** `snapToItem`, `AlignToSegment`, `SplitAdjacentSegments`' joint refusal (apart from D-8).
- **`TRACKS_CLEANER::CleanupBoard`:** the call order, `removeNullSegments`, duplicates always run, the final merge only after a dangling deletion, the dialog's defaults.
- **Microwave `createFootprint`:** the prompts, "Incorrect number, abort", the angle clamps, the arc-stub point count and theta steps, `createBaseFootprint`'s attributes and texts, the gap and stub offsets (apart from D-7).
- **Layer defaults:** `BOARD_DESIGN_SETTINGS::GetLayerClass`, `GetLineThickness`, `GetTextSize` against `web/kicad-port/layerDefaults.ts`.

---

# Summary

| # | Finding | Severity |
|---|---|---|
| B-1 | Cut of table cells deletes the whole table (KiCad clears the cells); Copy of cells copies the whole table | high |
| C-1 | Board Delete/Undo while drawing a zone or polygon: Delete removes the item under the cursor, Undo drops the whole outline; KiCad removes the last corner | high |
| D-1 | Cleanup dangling pass removes whole multi-point tracks, breaking connections | high |
| D-2 | Cleanup shorting pass removes whole polylines (high); skips no-net items, counts zones (medium) | high |
| A-1 | Flip on user layers typed front/back does not toggle text/dimension mirror | medium |
| A-2 | Swap Layers leaves tuning patterns / via stitches on the old layer | medium |
| A-3 | Rotated rounded rectangle: flattened at a fixed 5 µm, no arcs (also fp Create Array, Edit Pad as Shapes) | medium |
| B-2 | Copy of a group of one footprint loses the group and the nets | medium |
| C-2 | Track & Via net change: no short/pad questions, pads never updated | medium |
| C-3 | Track width error not KiCad's message; title varies | medium |
| C-4 | Pad Properties lacks the clearance/mask/paste/NPTH warnings, ignores the offset | medium |
| C-5 | Add Field ignores Board Setup's Fab text defaults | medium |
| D-3 | Cleanup dangling: tracks inside one pad kept (`aIgnoreTracksInPads`) | medium |
| D-4 | Tracks-in-pads ignores track width | medium |
| D-5 | Redundant via on a through-hole pad only at its exact centre | medium |
| D-6 | Select Connection walks through pads, ignores layers and inner vertices | medium |
| D-7 | Microwave pad numbers (gap swapped; stub both "1") | medium |
| D-8 | Break Track on a locked track refused (KiCad asks, then breaks) | medium |
| D-9 | Global Text & Graphics edit skips text boxes, table cells, barcodes | medium |
| D-10 | Simplify Shapes loses the polygon's id and group | medium |
| A-4 | Canvas paints dimension text angle with the wrong sign | low |
| A-5 | Dimension text angle written to the millidegree, not `{:.10g}` | low |
| A-6 | Delete-from-cutout snaps with grid snapping off | low |
| B-3 | Group lock ignores the parent group's lock | low |
| B-4 | Table cells groupable; points, grid items, drill maps outside the group bookkeeping | low |
| B-5 | Whitespace plain-text paste falls back to the old copy; UTF-16 length | low |
| B-6 | Copy as Text on the board: no hover fallback | low |
| B-7 | Pad paste with its parent not on the board restores the whole footprint | low |
| B-8 | No clipboard image paste | low |
| B-9 | A footprint's nested groups dropped on import | low |
| B-10 | Group library link written where KiCad would not | low |
| C-6 | Undo in the measure tool clears the ruler, not the board | low |
| C-7 | PARITY line 1086's false reason for the read-only via rows | low |
| C-8 | ValueStringCompare on a partly parsed number | low |
| D-11 | Polygon boolean messages and the all-failed case | low |
| D-12 | Cleanup leftovers (duplicate choice, merged id, arcs, SMD layer) | low |

Counts: high 4, medium 16 (D-2's medium half is counted under D-2's high row), low 16.

Round-2 fixes that came out right are listed in each part's "checked" list: R1, R2/R3/R10, C1 (apart from A-3), C4 (apart from A-1), R4-R9, A2, C2 (apart from B-1, B-2), C7, B2, A1, A3, A4/A9 (apart from B-3), A11, C3, A5, A8, C5, C6 (apart from C-1, C-6), C8, C9, B1, B3-B6 and R5.
