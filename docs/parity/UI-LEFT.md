# The studio UI against KiCad: what is left (inventory of 2026-11-05)

Compiled on main abf1ddb3 from `GAPS.md`, `UI-ACTIONS.md` and `ui-parity-missing.json`, the `PARITY-*.md` files, the round 2-7 findings (Open and Left marks), and HANDOFF's "Kept out by decision". It is the work list for the UI rounds that follow: each batch marks its entries **done (branch)** here.

How to read it:
- Groups run from the most-used editor down; entries in a group run from the most visible down.
- Each entry gives its tag, what a KiCad user meets, a size and the source.
- The tags are FM (feature missing), BD (behaviour differs), CO (cosmetic) and KO (kept out by decision).
- "(unsure)" means the sources disagree, or it may have been fixed since.

Notes:
- Every task branch is merged (`git branch -r --no-merged main` is empty), so every "not merged" in GAPS.md is stale.
- Four medium round-7 PCB findings were never fixed: R7-1, R7-2, R7-3 and R7-5 (`r7-pcb-edit.md`). **Fixed on task-u1-pcb.**
- These findings still have open parts:
  - E3-1 (high);
  - D-A1, B-M4/D3-M5/D4-M1, C3-M1, B3-M1, A-3, D-8, D-10, C-2 and R3-6 (medium, rounds 2-4).
- `web/studio/src/kicad/editor_toolbar_support.json` lists the tools not yet ported in the library editors: 24 in the Footprint Editor and 7 in the Symbol Editor (3 since task-u1-symedit: Find, Find and Replace, the Symbol Checker).

**Top 12, all areas:**
1. X1: the Plot dialog is Gerber only -- **done (task-u1-plot)**
2. P1: teardrops -- **done (task-u1-pcb)**; per-pad parameters left
3. S1: placing labels, power symbols and text -- **done (task-u1-schplace)**
4. R1: dragging a footprint with its tracks -- **done (task-u1-drag)**
5. L1: saving into table libraries
6. F1+F2: Footprint Editor fields and dimmed tools
7. Y1: Symbol Editor Rotate -- **done (task-u1-symedit)**
8. P2/S7: Properties panel rows -- **done (task-u1-props)**
9. S3: Schematic Setup pages
10. P8: zones
11. S2: design variants
12. M1: the Simulation Model dialog

## PCB editor

- **P1 [BD] Teardrops.** **done (task-u1-pcb)**: SMD pads of every shape but a custom one, tracks ending off centre in a via or pad or crossing one (R7-1), each polyline segment its own track; 39 kicad-cli outlines point for point (PARITY-pcb §66). Left:
  - Each via's and pad's own teardrop parameters are not read (the file's tokens are kept, unread); Board Setup's one block serves every anchor. Custom pads get none.
  - One segment, straight edges, no track-to-track; no fit to other nets' clearance, no mask teardrops.
  - Not kept current as you edit: you re-run Add All Teardrops.
  - L. GAPS 8/13/35; PARITY-pcb §15, §62 D6-1, §66; r7 R7-1.
- **P2 [BD] Properties panel.** **done (task-u1-props)**: every class lists KiCad's registered rows in KiCad's groups and order (a test replays the C++); via tenting, covering, plugging, capping, filling, backdrill, post-machining, layers and type, track and shape solder mask, corner radius, text italic, bold, vertical justification, width apart from height, knockout and automatic pen, a pad's orientation, copper layers and overrides, a footprint's reference and (board-file) paste margins are edited; Component Type, which KiCad's panel lacks, is gone (PARITY-pcb §67). Left, read-only with the reason as the tooltip:
  - Fonts, a shape's line style (P5), every teardrop row (P1's per-item parameters), a pad's number, pin name and type, post-machining, backdrill, fabrication property and pad-to-die, a dimension's label style.
  - A footprint placed from the intent has no paste margin of its own; text colour is masked on board text, as in KiCad.
  - M. GAPS 10; PARITY-pcb §23, §67.
- **P3 [FM] Track & Via and Pad Properties.**
  - No via protection (IPC-4761 tenting, covering, plugging, filling), backdrill or post-machining.
  - No pad fabrication property, pin function or pad-to-die length.
  - A custom padstack's per-layer thermal and clearance can't be edited, and neither can via padstacks. A zone's force-flash is kept but not applied.
  - M. PARITY-pcb §10, §26, §43, §44, §56; r2 A5.
- **P4 [BD] Board text.** **done (task-u1-pcb)** for Bold: the Text dialog has KiCad's Bold, and a new copper text box is bold as `InferBold` makes it (R7-3; PARITY-pcb §66). Left:
  - The Text dialog has no Italic.
  - A text is centred on its anchor whatever its vertical justification.
  - Outline and TrueType fonts are drawn in the stroke font.
  - M. PARITY-pcb §39, §60 B5-2, §62 A6-3.
- **P5 [BD] Line styles.** Dashed and dotted lines, arcs and rectangles are drawn and saved solid; the model has no line style. M. GAPS 2; PARITY-pcb §37; r5 B5-5.
- **P6 [BD] Swap (Alt+S).** **done (task-u1-pcb)** for zones: a zone stays put and two zones swap layers, as `ZONE::SetPosition` is a no-op (R7-2; PARITY-pcb §66). Left:
  - Arcs, dimensions, groups, tables, text boxes, barcodes and points are not swapped, and neither is a pad with a pad.
  - M. r7 R7-2; r6 E6-2.
- **P7 [BD] Strict mode, on by default.**
  - It refuses a move, swap or delete that breaks a complete net, and a pasted copy keeps its nets, so moving the copy is refused.
  - KiCad never refuses. This may be an intended safety rule.
  - S. GAPS 8; PARITY-pcb §22.
- **P8 [BD] Zones.**
  - The outline is drawn first and the dialog comes after; KiCad opens the dialog first.
  - No outline display style or per-zone hatch offsets, and no Update Displayed Zones in the Zone Manager.
  - The point editor, Simplify, Convert and the hit test ignore holes.
  - An unfill doesn't stick (R7-Z1), and a "N zones not filled" note stays up (R7-Z2).
  - M. PARITY-pcb §8, §11, §52; GAPS 13.
- **P9 [FM] Net Inspector.**
  - It is a modal dialog, not a docked panel.
  - No Group by Netclass or custom groups, no time-domain columns, and no Rename, Delete or Add Net.
  - M. GAPS 29; PARITY-pcb §32d.
- **P10 [FM] Search panel.** There is no docked panel: Show Search Panel and Interactive.search jump to the Activity tab. M. GAPS 21; PARITY-pcb §7a, §32b.
- **P11 [FM] Colour theme.** It can't be edited: the swatches are read-only and Use Color from Schematic is dimmed. Renaming a net class loses its colour. M. GAPS 18; PARITY-pcb §24.
- **P12 [FM] Global edits.**
  - Edit Text & Graphics has no footprint reference, value, field or graphic scopes, no By-parent filters, and no font, bold, keep-upright or visible rows.
  - Edit Track & Via has no microvia, annular-ring or IPC-4761 presets.
  - M. GAPS 35; PARITY-pcb §13.
- **P13 [BD] Dimensions.**
  - Every kind is placed with two clicks: no height click and no live preview. Radial, Leader and Center lack their own click sequences.
  - A leader has no text border.
  - A text can't be placed by hand, and an imported manual text position is reset.
  - M. PARITY-pcb §18; GAPS 28/35.
- **P14 [BD] Footprint Properties on the board.**
  - Reference and Value can't be edited, because they come from the schematic.
  - It shows 2 mandatory field rows, not 4 (unsure).
  - Paste overrides are read-only, and duplicate field names are refused (r2 C8, kept).
  - M. PARITY-pcb §10, §26, §56; fpedit §16.
- **P15 [BD] Footprint graphics on the board.**
  - They can't be selected or edited one by one.
  - Title-block variables in footprint text aren't resolved.
  - A hatched graphic is drawn as an outline.
  - M. GAPS 25; PARITY-pcb §28, §39.
- **P16 [BD] Tables, on the board and the sheet.**
  - No paste of tables or cells, and no drag-select of cells.
  - Flip is a plain mirror.
  - Cell Properties has no fill or text colour.
  - `@{expr}` shows as typed, and `${REF:FIELD}` doesn't follow a rename.
  - M. GAPS 24; PARITY-pcb §38; PARITY-sch §35.
- **P17 [FM] Item kinds inside footprints.**
  - Text boxes, images, points and barcodes can't sit inside a footprint.
  - Rotate and Flip don't turn an image's pixels.
  - Whether these kinds can be duplicated or arrayed is unsure.
  - M. GAPS 24; PARITY-pcb §39, §58.
- **P18 [FM] KiCad 10 items.**
  - No Drill Chart Properties or Drill Groups dialogs.
  - Grid items have no role in routing or placement.
  - Via Stitching sets its via inline.
  - M. GAPS 2; PARITY-pcb §50.
- **P19 [BD] Placement.**
  - Place Footprint (A) places once and ends.
  - The Microwave tools and the Footprint Editor's Insert drop the part at the first click or the board centre, with no R or F while it is carried.
  - A footprint's position snaps to 0.1 mm whatever the grid.
  - Pack and Move uses a shelf packer.
  - S. PARITY-pcb §2, §11a, §20, §41.
- **P20 [BD] Multichannel and design blocks.**
  - Repeat Layout skips tuning patterns, barcodes, images and tables, and ignores zone holes. Its options last only for the page session.
  - Its sheets come from the inferred modules (R7-D5). The boards it writes have no sheet name, so KiCad's own Repeat Layout finds empty sheets.
  - Save Selection as a block drops groups (P18-3).
  - M. GAPS 24/32; PARITY-pcb §36, §40.
- **P21 [BD] Footprint exchange and swaps.**
  - Change and Update Footprint match pads by number only.
  - Compare with Library misses attribute flags and net ties.
  - Library pad layers aren't carried.
  - Update Schematic from PCB doesn't carry a pin or gate swap back to the schematic.
  - M. GAPS 26; PARITY-pcb §30, §45, §61.
- **P22 [BD] Selection.**
  - The Selection Filter has no "Only…" menu and isn't saved.
  - The clarification menu has no numbered hotkeys.
  - There is no coverage pass for overlapping footprints.
  - Select Connection doesn't follow a chain of graphic lines.
  - S. PARITY-pcb §3, §22; GAPS 18.
- **P23 [BD] Vias and sizes.** **done (task-u1-pcb)** for R7-5: Change Track Width / Via Size and the microwave tools take the sizes the board's lists give now (`GetCurrentTrackWidth`, `GetCurrentViaSize`), a microvia its class's (PARITY-pcb §66). Left:
  - The via ghost isn't drawn at its snap point (C6-7).
  - With Allow DRC violations on, a violating via is placed silently (C6-6).
  - The via check misses unflashed PTH layers, and copper text boxes, tables and dimensions (C6-4/5, R7-4).
  - S. r6/r7-pcb-edit.
- **P24 [BD] Edit tools, small differences.**
  - Cleanup Tracks doesn't merge overlapping collinear tracks, and the tracks it merges get new ids.
  - Cleanup Graphics skips text boxes.
  - Merge Zones has no "could not be merged" message.
  - Create Array skips tuning patterns.
  - An off-quarter rotation flattens a rounded rectangle's arcs.
  - Flip leaves a footprint's user-layer drawings in place.
  - Pasting a pad whose footprint was deleted brings the footprint back (B-7).
  - No "Do not show again" boxes.
  - S. PARITY-pcb §55-60; r3 B-7/D-8/D-10/D-11, r4 C-2, r5 B5-6/16.
- **P25 [BD] Net tools.**
  - Highlight Net drops the no-net net.
  - Local Ratsnest picks a pad by its box.
  - The net-under-cursor pick differs.
  - Repair Board runs in a different order (C7-5/10/11/12).
  - Flip Board View mirrors text that KiCad keeps readable.
  - S. r7-editors-exports; PARITY-boardctl §1, §7.
- **P26 [BD] Embedded Files.**
  - Add and Remove apply at once; KiCad applies them on OK.
  - Export goes file by file.
  - Fonts aren't embedded, and `kicad-embed://` datasheets don't open.
  - S. PARITY-pcb §47.
- **P27 [FM] Board outline.**
  - No outline editor.
  - Fix discontinuities in board outlines does nothing.
  - A footprint's Edge.Cuts don't follow it when it moves.
  - S. GAPS 27.
- **P28 [CO] Cosmetic.**
  - The ratsnest curve and highlight colour, and other nets not dimmed (C7-8/9).
  - Inactive-layer Dim is a flat alpha.
  - The rule-area hatch and the zone debug views.
  - R, F and E on a carried text (A6-6).
  - The Select Net menu's order.
  - S. PARITY-pcb §1, §8, §24.

## Schematic editor

- **S1 [BD] Placing labels, power symbols and text.** **done (task-u1-schplace)**: the dialog first (Place Power: the chooser filtered to power symbols), then the item rides the cursor, R / X / Y turn it there, one undo step per placement (PARITY-sch §54).
  - Left: Properties of a power symbol in the hand; the right-button menu of the item in the hand; the label dialog lists the sheet in view's labels only (no other sheets, bus aliases); "Place all units" and the global/local power preference; the placed item is not left selected; Repeat puts a power symbol 100 mil down, not at the cursor.
  - (was: you click first and the dialog comes after, and nothing follows the cursor before the click; Place Power is a text field with presets, its rotation set in the dialog.)
  - M. PARITY-sch §3, §3a, §54.
- **S1b [BD] Place Symbol (A) has no symbol following the cursor** (found by task-u1-schplace). After the chooser, it waits for a click with nothing drawn at the cursor; KiCad carries the symbol, with R/X/Y while carried. The `POST /api/sch/place_preview` endpoint from task-u1-schplace can draw it. S. PARITY-sch §54.
- **S2 [FM] Design variants.** No Add, Remove or Edit Variant, no per-variant DNP or fields, and no variant columns in the Fields Table or the placement files. XL. ui-parity-missing (3 rows); PARITY-sch §13, §25.
- **S3 [FM] Schematic Setup.**
  - Formatting lacks the text sizes, line widths, and junction and hop-over sizes.
  - No Annotation, Field Name Templates, Net Classes or Bus Alias pages, so ERC's "Edit Netclasses…" fix is missing.
  - L. PARITY-sch §9, §28.
- **S4 [BD] Symbol Properties.**
  - No unit choice, pin table, font or colour.
  - U, V and F open the full dialog.
  - A power symbol's Properties has no footprint, datasheet, exclusion flags, pin table or user fields.
  - M. PARITY-sch §1, §26, §30.
- **S5 [FM] Placed symbols.**
  - A pin's alternate function can't be chosen or drawn, and there is no Show Pin Alternate Icons.
  - A library text box shows only its border.
  - Only the Normal and De Morgan body styles exist.
  - M. GAPS 12/19; PARITY-sch §48; r5 D5-H3.
- **S6 [BD] Text looks.**
  - Font face, colour and hyperlink can't be edited.
  - A free text's bold, italic and colour are kept but not drawn.
  - A label's own fields can't be clicked or moved, and a hand-moved one is autoplaced again.
  - M. GAPS 12; PARITY-sch §39, §52.
- **S7 [BD] Properties panel.** **done (task-u1-props)**: every class lists KiCad's registered rows (a test replays the C++), and the symbol's and sheet's fields are rows as KiCad's panel adds them; a label's size, bold, italic, colour and justification, a text's bold, italic, colour and hyperlink, wire ends, Body Style, Exclude From Position Files and user fields are edited; Sheetfile shows the field's own text (B3-M1) (PARITY-sch §55). Left, read-only with the reason as the tooltip:
  - Fonts; a symbol's pin display (one library symbol per lib_id) and its Unit (no verb switches a placed unit).
  - A sheet's border, fill and attributes (not kept: S8); a text box's margins, fill, colour and hyperlink; an image's transform offset.
  - M. PARITY-sch §17, §30, §45, §55.
- **S8 [FM] Hierarchy.**
  - The Navigator has no Edit Page Number, Rename, New or Delete Top-Level Sheet, and no net highlight.
  - A KiCad 10 project with several top-level sheets is folded under one root.
  - A sheet's colours and DNP and BOM flags aren't kept or editable, and there is no Add Field on a sheet.
  - M. PARITY-sch §30, §40, §50, §52; r2 B-L1, r6 C6-L2, r7 A7-L3.
- **S9 [BD] A shared screen (a sheet placed twice).**
  - Annotate numbers only that screen's references.
  - Every path shares one power reference.
  - A unit edit changes every path.
  - The DNP, library-link and Update Symbol tools act on the sheet in view.
  - M. GAPS 4; r2 B-M4, r3 D3-M5, r4 D4-M1, r5 D5-M2.
- **S10 [KO] One library symbol per lib_id (a model limit).**
  - Ctrl+E edits the shared entry, so every placement changes.
  - A `lib_name` copy is traced as the lib_id's, which can give a wrong net.
  - L. PARITY-sch §48, §50.
- **S11 [FM] Symbol Fields Table.**
  - No scope selector, saved presets, field-name templates or "Add field from library".
  - No Save/Discard question on close.
  - The Description column may be done (§52 H3).
  - M. PARITY-sch §7, §25; r5 C5-L9.
- **S12 [FM] Find/Replace.** No regex, and it doesn't search sheet names, sheet files, pins or library metadata. S. GAPS 1/4; PARITY-sch §8.
- **S13 [FM] Arcs.** Two clicks make a fixed 90° arc, and there is no arc point editor. M. PARITY-sch §12.
- **S14 [BD] Annotation.**
  - Placing a unit never fills a free unit of an existing part (D-A1).
  - `R?` becomes `R?1`.
  - The dialog's settings aren't saved.
  - M. r2 D-A1; PARITY-sch §39, §42.
- **S15 [KO] Annotation by design.** No Clear Annotation and no Annotate Automatically switch, and Paste Special's keep/clear differs: every part has a real reference. S. GAPS 4/6; PARITY-sch §5.
- **S16 [BD] Edit Text & Graphics, Change and Update Symbols.**
  - Edit Text & Graphics skips fields, labels, wires and junctions.
  - Change and Update Symbols can't reset field sizes, positions or visibility.
  - S. PARITY-sch §12.
- **S17 [BD] Import Sheet and dropped schematics.**
  - No "Place as group", and no ghost while carried.
  - A chooser opens instead of putting the sheet in your hand.
  - One file per drop.
  - S. GAPS 4; PARITY-sch §19, §37.
- **S18 [BD] Wires.**
  - Break and Repeat across a two-pin part trims between the pins (C3-L1).
  - Some sheet-pin drag cases differ.
  - Swap and Properties skip the commit clean-up (C3-M1).
  - The no-connect tool doesn't remove an existing flag.
  - Enter doesn't finish a wire. **done (task-u1-schplace)**: Enter is KiCad's `cursorClick`, a click where the cursor is (it does not finish a wire in KiCad either), and End now keeps the segment to the cursor (PARITY-sch §2, §54).
  - S. PARITY-sch §2, §46.
- **S19 [BD] Bus unfold, swap and groups.**
  - Unfold from Bus has no member submenus and no ghost.
  - Alt+S doesn't swap sheets or bus entries.
  - Some small group differences.
  - Reorganize drops a label's spin and the groups.
  - S. PARITY-sch §11, §29, §50; r7 A7-L2.
- **S20 [BD] Fields.**
  - Copy and paste lose a user field's shown state and position.
  - Find treats user fields as hidden.
  - Autoplace ignores swapped pins.
  - An imported power symbol loses its flags and its hand-placed Value (D4-L2).
  - S. PARITY-sch §21, §30.
- **S21 [BD] Sheets.**
  - A duplicate sheet name is refused; KiCad allows it and ERC reports it.
  - No "Untitled Sheet" and no "Link to this file?".
  - The root page is fixed at 1.
  - Sync Sheet Pins can't make labels in the subsheet.
  - No bus-to-bus entry.
  - S. PARITY-sch §32, §39, §40, §47.
- **S22 [BD] Net-name corner cases.**
  - Which of two equal names gets `_1`.
  - A pin number used twice.
  - An `@{}` error keeps its raw text.
  - `${U1A:FIELD}` works only in the letter form.
  - S. PARITY-sch §44, §47; r2 A-N11.

## Router

- **R1 [FM] The footprint and multi-track draggers. done (task-u1-drag)** -- pns Stage 22.
  - ~~D on a footprint doesn't drag it with its tracks (COMPONENT_DRAGGER).~~ Done: the footprint follows the cursor with its pads, the tracks on them follow, one undo step.
  - ~~D on several tracks does nothing (MULTI_DRAGGER).~~ Done: the selected tracks drag together, in the three router modes.
  - Left: the multi-track Shove has no springback; the selection is not cleared at the start, restored on Esc, or set to the leader segments after a multi-track drag; the closest net is not highlighted while dragging; a footprint's own Edge.Cuts / Margin graphics stay (board shapes in this model).
  - L. GAPS 7 R7-R9; pns Stages 21-22.
- **R2 [FM] Diff pairs.**
  - No arcs in the pair router or the tuners (F4-2).
  - The blind, buried and microvia commands do nothing in the pair tool.
  - F finishes at the cursor.
  - M. GAPS 7; pns Stages 11-12.
- **R3 [BD] Length tuner.**
  - No pad-to-die length or delay, and a time-domain rule gives no target.
  - A pattern can't be moved or rotated.
  - The corner style and single-sided settings are ignored.
  - The status box sits at the cursor.
  - M. GAPS 7; pns Stage 8; PARITY-pcb §35, §65.
- **R4 [BD] Autoroute.** It routes inside the outline's summary polygon and ignores cutouts. M. GAPS 27.
- **R5 [BD] Layers and vias.**
  - V goes to the layer pair's other layer, not to the nearest ratsnest anchor's.
  - The clearance isn't re-read after a layer switch.
  - V armed before a route outlives the tool (R7-R6).
  - Autoroute leaves the active layer changed (R7-R7).
  - S. pns Stages 11-21.
- **R6 [BD] The head's shape.**
  - A fan-out elbow isn't straightened (`optimizeTailHeadTransition`).
  - No hull snap in highlight-collisions mode.
  - `reduceTail` and `handlePullback` are not ported.
  - Earlier vias aren't shoved by a later leg.
  - M. pns Stages 2, 10, 12.
- **R7 [BD] Grab radius and widths.**
  - The start and end grab radius is a fixed 0.5 mm, not the grid size.
  - Automatically Select Track Width stays on after a toolbar pick.
  - The width keys cycle.
  - S. pns Stage 12; PARITY-boardctl §4.
- **R8 [BD] Size dialogs.**
  - An extra Optimizer effort row, and "Use Custom Values…".
  - Pre-defined Sizes' Add isn't pre-filled, and a 0 width is refused.
  - Cleanup and Break Track drop the mask opening and the group (R6-R8).
  - S. pns Stages 6, 11, 18, 20.
- **R9 [BD] Obstacles and rules, edge cases.** Castellated pads, a footprint's Margin graphics, text hulls, track-geometry rules, and 1-2 nm tolerances. M. pns Stages 3, 12, 15-17.
- **R10 [CO] Toasts.**
  - Toasts appear on router and corner-mode changes (R7-R8).
  - Drag Anyway has no "Do not show again".
  - Delete during a live drag does nothing.
  - S. GAPS 7.
- **R11 [KO] Kept by decision.**
  - A chain with a width change stays one line per width when shoved.
  - Studio extras: the arc drag is judged, routes start mid-arc, and Backspace works in pairs.
  - S. pns Stages 3, 9, 12.

## Footprint editor

- **F1 [FM] Fields and texts on the canvas.**
  - Reference, Value and user fields are not drawn or pickable.
  - Texts and graphics have no Properties dialog.
  - M. PARITY-fpedit §13, §15, §17.
- **F2 [FM] Dimmed tools.**
  - Mirror, Group and Ungroup, the line modes, the sketch and outline modes, the Appearance manager and the Properties pane.
  - Dimensions, rule areas and keepouts, text boxes, tables, barcodes, points and reference images.
  - A footprint's zones and dimensions are kept but not drawn.
  - L. editor_toolbar_support.json; fpedit §8, §16.
- **F3 [BD] Ctrl+E from the board.**
  - It edits the library entry, with no link back to that footprint.
  - Update Footprint on Board updates every footprint of that name.
  - M. fpedit §1, §6.
- **F4 [FM] Footprint Properties.**
  - The name is read-only.
  - Clearances has only the zone connection.
  - No Private Layers tab, and no Net Ties or jumper groups.
  - M. fpedit §5.
- **F5 [BD] What saving loses.**
  - Stroke type, hatch, vertical justification, bold, italic, keep-upright and the locked flags are lost on save.
  - Hidden user text shows.
  - M. GAPS 25; fpedit §4, §16; r4 E4-F2.
- **F6 [BD] Pad Properties.**
  - Layers are set only through presets.
  - No fabrication property or pad-to-die length.
  - Round/Slot resets the size, and the type is named "Slot" where KiCad says "Oblong" (E7-F6/F7).
  - M. fpedit §3, §18.
- **F7 [BD] Set Anchor.** It leaves zones, dimensions and the courtyard override behind (E3-1). S. r3 E3-1.
- **F8 [FM] Missing tools and settings.**
  - No Import Graphics and no preference pages.
  - The rotation is fixed at 90°.
  - ~~No Save Library As.~~ **done (task-u1-libsave)**: a library row's Save As makes a new `.pretty` with its row and copies every footprint into it (PARITY-fpedit §19).
  - M. GAPS 28; fpedit §14, §17; PARITY-common §6.
- **F9 [BD] Footprint Checker.** Reference and Value aren't checked, and markers sit on the first item. S. GAPS 29; fpedit §12.
- **F10 [CO] Arrays and numbering.**
  - The Create Array preview.
  - Recombine deduplicates by exact match only.
  - Duplicate and Increment on several pads numbers them in sequence.
  - S. fpedit §7, §13.

## Symbol editor

- **Y1 [BD] Rotate.** It turns only the pins; the selected shapes and text stay put. M. PARITY-symedit. **done (task-u1-symedit)**: every kind -- pins, shapes, beziers, texts, text boxes and fields -- about KiCad's pivot, CW and CCW, one undo step.
- **Y2 [FM] Mirror and beziers.** **done (task-u1-symedit)**: Mirror H and V of every kind; beziers read, written, drawn, drawn by the bezier tool and edited by a point editor; left: Import Graphics still cuts curves into segments, and the point editor is for curves only.
  - ~~Mirror H and V are not ported.~~
  - ~~No bezier tool, beziers aren't drawn, and Import Symbol drops them.~~
  - M. editor_toolbar_support.json; PARITY-symedit.
- **Y3 [FM] Properties pane.** The Symbol and Footprint editors have none. M. GAPS 10; PARITY-common §12. **Symbol Editor done (task-u1-symedit)**: KiCad's rows of pins, shapes, texts, text boxes, fields and the symbol, edits on the editor's undo stack; left: the rows a library symbol has no place for (font, colours, line style, hyperlink) are read-only. The Footprint Editor's half is task-u1-fpedit's.
- **Y4 [FM] Symbol Properties and text.**
  - No unit display-name grid, no Pin Connections tab and no named body styles.
  - ~~Plain text has no Properties dialog.~~ **done (task-u1-symedit)**: Text Properties (double click, E, Properties), and a library text keeps bold, italic and its justification.
  - No border colour, custom fill, Private flag or font.
  - M. GAPS 19; PARITY-symedit.
- **Y5 [BD] Pins and the clipboard.** **done (task-u1-symedit)**; left: Increment of the pin being placed, and the clipboard's SVG/PNG images.
  - ~~The pin tool takes a click, then a dialog, with no ghost.~~ KiCad's `TwoClickPlace`: Pin Properties, then the pin follows the cursor until the click.
  - ~~Duplicate and Paste drop in place.~~ The items follow the cursor until a click.
  - ~~The editor uses its own clipboard, not `.kicad_sym` text.~~ Copy writes `.kicad_sym` text; Paste reads KiCad's.
  - ~~No increment of a pin's number or name.~~ Increment (Primary / Secondary, Shift+Alt / Ctrl+Alt wheel) of a pin's number or name and of a text; Repeat Pin's pitch and label increment in Preferences.
  - S. PARITY-symedit; PARITY-sch §13.
- **Y6 [BD] Pin Table.** No summary, no Count or text-size columns, and raw type tokens (E7-S15). S.
- **Y7 [BD] Small differences.**
  - Pin Properties has an extra Delete Pin (E7-S4), and a power symbol's Exclude boxes are disabled too early (E7-S9).
  - ~~Save As doesn't ask before overwriting, doesn't refuse a read-only library~~ **done (task-u1-libsave)** (E7-S19, S20); Duplicate still opens the first copy at unit 1 (E7-S21).
  - ~~Save Library As is a download.~~ **done (task-u1-libsave)**: a new `.kicad_sym` with a row of the project table, or the library's row pointed at it (PARITY-common §16).
  - No Find/Replace.
  - Export SVG uses kicad-cli's theme (X11).
  - S. r7 E7-S*; GAPS 37.
- **Y8 [CO] Labels.** A derived symbol shows "derived from X" instead of nesting under its parent, and the Symbol Checker button is dimmed (unsure). S. PARITY-symedit.

## Fab outputs and exports

- **X1 [FM] PCB Plot is Gerber only.** **done (task-u1-plot)**: Gerber, PostScript, SVG, DXF, PDF, PNG with every option of `DIALOG_PLOT`, Plot on All Layers, the output folder, Run DRC, the mask alert. HPGL is not offered (KiCad's dialog dropped it; kicad-cli: "No longer supported"); 8303b2ad's dialog has no values/references boxes. The job file box works (unticked, the job file kicad-cli always writes is deleted). Left, disabled with the reason: the origin outside Gerber/DXF, SVG precision, PDF metadata, the PDF popups apart, DXF's font, the design variant.
  - L. GAPS 37; PARITY-boardctl §5, §8.
- **X2 [FM] Drill files.** **done (task-u1-plot)**: Excellon and Gerber X2, map files and their format, units, zeros, precision, mirror, minimal header, tenting layers, the output folder, Generate Report File. Left: the report is the selected format's writer's (KiCad's dialog inverts the test). M. GAPS 37; PARITY-boardctl §8.
- **X3 [FM] Schematic exports.** **done (task-u1-plot)**: Plot Schematic has PostScript, PDF, SVG, DXF, PNG, the colour theme, the PDF and PNG options, the output folder, Plot All / Current Page; Export Netlist has KiCad, OrcadPCB2, Allegro, CadStar, PADS, SPICE, SPICE Model (and XML). Left, disabled with the reason (no kicad-cli flag): page size, minimum line width, DXF units, the SPICE page's options, Open file after plot, custom exporters, the variant.
  - S. PARITY-sch §10, §53.
- **X4 [FM] Placement files.** **done (task-u1-plot)** as far as kicad-cli goes: kicad-cli 10.99 (4fe1ca2060) still has no Exclude from BOM flag for `pcb export pos` (disabled with the reason; the api-server's position job has `exclude_from_bom`, unused), and the variant choice is disabled (kicad-cli has `--variant`, the studio has no variants, S2); the dialog's output folder is wired. S. GAPS 37; r2 X8.
- **X5 [BD] Small differences.**
  - Sketched fab pads at 0.1 mm (X3-9).
  - An export with warnings shows none (X5-1).
  - Export Footprints drops the instance's fields (X7-1).
  - Plotting the current page plots both sheets of a duplicated page number (X4-2).
  - S.
- **X6 [KO] Kept by decision.**
  - The board BOM is kicad-cli's.
  - A BOM goes to `export/` only.
  - The legacy BOM runs only the bundled scripts.
  - S.

## Board Setup, rules, DRC, ERC

- **B1 [FM] Component Classes.** There is no page; the rules are read and honoured, but nothing edits them. M. GAPS 3; PARITY-pcb §9.
- **B2 [FM] DRC Rule Editor.**
  - No Show Matches, syntax help, auto-complete or tree drag-and-drop.
  - Custom Rules is a plain textarea.
  - Check Rule Syntax doesn't expand variables (R6-D5).
  - M. GAPS 3; PARITY-pcb §48, §63.
- **B3 [FM] Physical Stackup.**
  - One row per dielectric, with the sublayers summed.
  - No Add or Remove Dielectric, and no impedance control.
  - Removing a used copper layer is refused.
  - M. PARITY-pcb §9; r2 F17.
- **B4 [FM] Net Classes.**
  - No `D[0..7]` bus patterns, schematic net-class labels, colour or Tuning Profile columns.
  - No transmission-line calculator.
  - Assign Netclass has no live preview.
  - M. PARITY-pcb §9.
- **B5 [BD] Text variables.**
  - No `@{}` math functions, VCSHASH or PIN_NAME(n).
  - Not expanded in schematic text boxes or power-symbol fields.
  - S. GAPS 3; PARITY-sch §19b.
- **B6 [BD] Inspection.**
  - In Find by Properties, intersectsCourtyard/Area, enclosedByArea and fromTo always return false, A.Net is the name, and there are no microvias.
  - Clearance and Constraints Resolution lack tuning-profile rules and `${VAR}`.
  - M. GAPS 29; PARITY-pcb §32, §32c.
- **B7 [BD] Exclusions.**
  - Some DRC exclusions, and the ERC exclusions of about 15 check types, stay in the studio.
  - A pin-to-pin Ignore isn't remembered (D3-M2).
  - M. GAPS 11; PARITY-sch §28.
- **B8 [FM] DRC dialog.**
  - No "Report all errors for each track" and no cross-probe options.
  - The Show boxes start ticked (R6-D9).
  - S. PARITY-pcb §9a.

## Libraries and choosers

- **L1 [FM] Saving into table libraries.** **done (task-u1-libsave)**: Save, Save All, Save As, Save Copy As, Save Library As and Revert in both editors write into the `.kicad_sym` file, the `.kicad_symdir` folder or the `.pretty` folder the table names (a New Library is filled by them); every library listed in Save As, read-only ones refused with KiCad's words; New Symbol goes into the target library; the trees mark what is not saved ` *` (PARITY-common §16). Left: Lead note (2026-11-05): a saved `.kicad_sym` file gets our header version 20250324 while the symbols it kept may come from a newer file (KiCad libraries are 20251024); of the parser's version switches only `< 20250827` (De Morgan inferred for a symbol that declares no body styles) can read a kept symbol differently, an edge case the slow test did not meet. Writes into KiCad's own libraries are allowed when the files are writable, as in KiCad.
  - Delete and Rename of a library's symbol or footprint stay in the design (the file keeps the old one); New Footprint is the project library's.
  - Save Library As has no "Do not update library tables" or global-table entry; KiCad 25ec2335's Save As Packed / Unpacked are not ported.
  - The library file is not watched for changes made outside the studio.
  - L. GAPS 5; PARITY-common §14, §16.
- **L2 [FM] Manage Libraries.**
  - No Reset or Migrate Libraries, and no Edit Options.
  - The project table only.
  - Non-KiCad formats are listed but not read.
  - No 3D model search paths.
  - M. GAPS 5; PARITY-common §14.
- **L3 [FM] Choosers.**
  - No `/regex/` or relational (`voltage>3.3`) search.
  - No footprint-selector list, and no "3D viewer in own window".
  - Change Symbols lists only the design's libraries.
  - M. GAPS 5/30; PARITY-sch §3a.
- **L4 [FM] Assign Footprints (CvPcb).** No footprint preview and no equivalence files. M. PARITY-sch §13.
- **L5 [BD] Design Blocks.**
  - The pane is docked, not floating.
  - A name with "." is refused.
  - A placed sheet doesn't get the block's default fields.
  - S. GAPS 32; PARITY-sch §34.
- **L6 [BD] Library edge cases.**
  - Placing a lib_id the design already holds uses the design's copy (unsure).
  - A folder library skips a broken file silently.
  - S. GAPS 33; PARITY-common §14.

## 3D viewer

- **D1 [BD] The scene.**
  - Trapezoid, chamfered and custom pads are drawn as boxes.
  - NPTH holes aren't cut from the board.
  - Inner copper takes the F/B colour.
  - Paste-only pads are grown (V7-1).
  - No per-layer ambient or specular.
  - M. GAPS 23/27/31; PARITY-3d §3, §5.
- **D2 [FM] Controls.**
  - No right-click menu, 3D Navigator, layer presets, saved viewports or preferences page.
  - The PCB selection isn't highlighted.
  - M. PARITY-3d §1-§5.
- **D3 [FM] Layer tree.** No Values, Footprint Text or Off-board Silkscreen rows. S. PARITY-3d §5.
- **D4 [FM] Models.**
  - No X3D or IGES, and no Extruded 3D Body.
  - Migrate 3D Models doesn't run at load.
  - KiCad 5 path aliases aren't used.
  - A bottom-side footprint's tab shows top-side numbers.
  - M. GAPS 30; PARITY-3d §6; PARITY-pcb §31.
- **D5 [KO] Raytracing.** It is out of scope, and a part with no model shows its F.Fab box. XL. PARITY-3d §2, §6.

## Import

- **I1 [BD] Non-KiCad import.**
  - No "Save changes?" question.
  - The layer map can't leave a layer out.
  - Eagle `.brd` and `.sch` are two imports.
  - No Altium variants.
  - An EasyEDA Pro file with several boards fails.
  - No IPC-2581 or ODB++ import.
  - A graphics-only schematic is refused.
  - M. GAPS 28; PARITY-pcb §34; PARITY-sch §33.
- **I2 [BD] Import Graphics.**
  - No placement preview.
  - SVG patterns, gradients, text and images are dropped.
  - DXF text is approximated.
  - M. PARITY-pcb §29; PARITY-sch §20.
- **I3 [BD] Append Board and Specctra.**
  - Append Board doesn't add the file's extra layers or unknown nets, and doesn't group what it appends.
  - Specctra session import refuses a missing part and gives `/FB` net names.
  - S. PARITY-pcb §29, §46.
- **I4 [BD] What imported boards lose.** Hidden board and dimension texts (R4-6), a manual dimension text position, nested footprint groups (B-9), and any footprint scale other than 1:1. S. r3-r7; GAPS 2.
- **I5 [KO] A board with its schematic beside it.** Importing a `.kicad_pcb` ignores the `.kicad_sch` next to it; the schematic is derived instead (F12). S.

## Simulator

- **M1 [FM] Simulation Model dialog.**
  - Only "SPICE model from file".
  - No built-in device or source models with a parameter grid, no IBIS, no Code tab, and no "Save parameter in Value field".
  - XL. GAPS 34; PARITY-sch §49, §51.
- **M2 [KO] Batch runs (`ngspice -b`).**
  - No live plot, and Stop throws the results away.
  - A measurement, Fourier or tuner change starts a new run.
  - L. PARITY-sch §38.
- **M3 [FM] Signals, cursors and tuning.**
  - No S-parameters, probe cursor, multi-run cursor labels, expression auto-completion or potentiometer tuning.
  - The OP overlay ignores its format settings.
  - Tune's Save skips Sim.Device (R7-SM-1).
  - M. PARITY-sch §38, §51.
- **M4 [BD] Shared transforms.** Two copies of a library symbol at different angles share one transform (unsure). S. GAPS 34.

## Common and app shell

- **C1 [BD] Mouse and touchpad.**
  - The drag-button preferences are ignored.
  - No pinch zoom.
  - The library canvases never auto-pan.
  - No pointer warp (a browser limit).
  - M. GAPS 20; PARITY-sch §27.
- **C2 [FM] Grid.**
  - Dots only.
  - Edit Grids has no names and no separate X/Y.
  - The schematic context menu has no Zoom or Grid submenu.
  - S. PARITY-pcb §1; PARITY-common §10.
- **C3 [FM] Page Settings.**
  - No custom drawing sheet (`.kicad_wks`).
  - The board's drawing sheet starts off and has no zone letters.
  - M. PARITY-common §9; GAPS 18.
- **C4 [BD] Small differences.**
  - Enter finishes a PCB route.
  - The crosshair and polar coordinates are shared across editors.
  - The Measure tool has no arrowheads.
  - About 310 handlers have never been compared with KiCad's code.
  - S. GAPS 15; PARITY-common §2-§4.
- **C5 [KO] Browser limits.**
  - The Ctrl+Tab and Alt+Tab switchers (the browser keeps those keys).
  - Save As and reports are downloads.
  - S.

## KiCad actions not wired (ui-parity-missing.json: 47, of which 34 are user-facing and 13 internal)

- **A1 [KO] Kept out by decision** (HANDOFF; GAPS Appendix B):
  - Import Netlist (the user, 2026-10-21).
  - The autoplacer. The user (2026-11-05) asked whether it is needed next to our own placer (`crates/place`); the proposal is to wire Place Selected and Place Off-Board Footprints to that placer, not to port AR_AUTOPLACER.
  - The Footprint Viewer and Symbol Viewer windows.
  - Python plugins and the footprint wizards.
  - Hyperlynx and `.rpt` export.
  - The app shell: New, Open, Revert, Quit, Project Manager, Rescue, open in the file manager, open in a text editor, Calculator Tools, and drop-to-add-library.
- **A2 [FM] Other missing actions.**
  - Remote Symbols.
  - Remap Legacy and Rescue Symbols.
  - Select Columns.
- **A3 [KO] Annotate Automatically.** Kept out by design.

Left out of this list: about 150 mentions of file-format or internal items with no UI effect.

Totals: 121 entries.
- By tag: FM 45, BD 62, CO 4, KO 10.
- By area: PCB 28, Schematic 22, Router 11, Footprint Editor 10, Symbol Editor 8, Board Setup 8, Exports 6, Libraries 6, 3D 5, Import 5, Simulator 4, Common 5, Unwired 3.
