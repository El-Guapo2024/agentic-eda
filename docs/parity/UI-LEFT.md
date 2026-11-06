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
- `web/studio/src/kicad/editor_toolbar_support.json` lists the tools not yet ported in the library editors: 24 in the Footprint Editor and 7 in the Symbol Editor. **Footprint Editor: 2 left since task-u1-fpedit** (Create Footprint, the Properties pane).

**Top 12, all areas:**
1. X1: the Plot dialog is Gerber only -- **done (task-u1-plot)**
2. P1: teardrops -- **done (task-u1-pcb)**; per-pad parameters left
3. S1: placing labels, power symbols and text -- **done (task-u1-schplace)**
4. R1: dragging a footprint with its tracks -- **done (task-u1-drag)**
5. L1: saving into table libraries
6. F1+F2: Footprint Editor fields and dimmed tools -- **done (task-u1-fpedit)**; the Properties pane (Y3) left
7. Y1: Symbol Editor Rotate
8. P2/S7: Properties panel rows -- **done (task-u1-props)**
9. S3: Schematic Setup pages -- **done (task-u1-schsetup)**
10. P8: zones -- **done (task-u1-zones)**
11. S2: design variants -- **done (task-u1-variants)**
12. M1: the Simulation Model dialog -- **done (task-u1-simmodel)**

## PCB editor

- **P1 [BD] Teardrops.** **done (task-u1-pcb)**: SMD pads of every shape but a custom one, tracks ending off centre in a via or pad or crossing one (R7-1), each polyline segment its own track; 39 kicad-cli outlines point for point (PARITY-pcb §66). Left:
  - Each via's and pad's own teardrop parameters are not read (the file's tokens are kept, unread); Board Setup's one block serves every anchor. Custom pads get none.
  - One segment, straight edges, no track-to-track; no fit to other nets' clearance, no mask teardrops.
  - Not kept current as you edit: you re-run Add All Teardrops.
  - L. GAPS 8/13/35; PARITY-pcb §15, §62 D6-1, §66; r7 R7-1.
- **P2 [BD] Properties panel.** **done (task-u1-props)**: every class lists KiCad's registered rows in KiCad's groups and order (a test replays the C++); via tenting, covering, plugging, capping, filling, backdrill, post-machining, layers and type, track and shape solder mask, corner radius, text italic, bold, vertical justification, width apart from height, knockout and automatic pen, a pad's orientation, copper layers and overrides, a footprint's reference and (board-file) paste margins are edited; Component Type, which KiCad's panel lacks, is gone (PARITY-pcb §67). Left, read-only with the reason as the tooltip:
  - Fonts, every teardrop row (P1's per-item parameters), a pad's number, pin name and type, post-machining, backdrill, fabrication property and pad-to-die, a dimension's label style (a shape's line style and a leader's text frame are edited since task-u2-lines, P5, P13).
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
- **P5 [BD] Line styles.** **done (task-u2-lines)**: a board shape's line style (dash, dot, dash-dot, dash-dot-dot, the file's default) is read, written, drawn as `STROKE_PARAMS::Stroke` cuts it with the width-scaled lengths and the board's dash ratios (board shapes, footprint graphics on the board and in the Footprint Editor), edited in Shape Properties' "Line style:" and the Properties panel's Line Style; kicad-cli plots a dashed F.SilkS line as dashes (slow proof); the QA round trip 22497 -> 20873 lines (PARITY-pcb §69). Left:
  - A shape drawn in the studio is written `(type solid)`, pcbnew's new shapes `(type default)` (both drawn solid).
  - The Footprint Editor's shape dialog has no Line style row (the library's stroke type is drawn and written, not edited).
  - The 3D view's silkscreen stand-in draws a dashed line solid (KiCad's 3D viewer dashes it, `create_3Dgraphic_brd_items.cpp`).
  - M. GAPS 2; PARITY-pcb §37, §69; r5 B5-5.
- **P6 [BD] Swap (Alt+S).** **done (task-u1-pcb)** for zones: a zone stays put and two zones swap layers, as `ZONE::SetPosition` is a no-op (R7-2; PARITY-pcb §66). Left:
  - Arcs, dimensions, groups, tables, text boxes, barcodes and points are not swapped, and neither is a pad with a pad.
  - M. r7 R7-2; r6 E6-2.
- **P7 [BD] Strict mode, on by default.**
  - It refuses a move, swap or delete that breaks a complete net, and a pasted copy keeps its nets, so moving the copy is refused.
  - KiCad never refuses. This may be an intended safety rule.
  - S. GAPS 8; PARITY-pcb §22.
- **P8 [BD] Zones.** **done (task-u1-zones)**: Add Filled Zone and Add Rule Area open the zone dialog at the first corner (`createNewZone`: the net of the highlight or the selection, the first unused priority), Cancel draws nothing and keeps the tool armed, the zone takes the dialog's settings in one undo step and is selected, E during the draw edits them; the outline display (Line / Hatched / Fully hatched), its pitch and the per-zone hatch offsets are read, written, edited and drawn (`ZONE::HatchBorder`); the Zone Manager's Update Displayed Zones; holes in the point editor, Simplify, Create Lines / Tracks, Create Zone / Polygon from Selection and the hit test; an unfill sticks and Undo gives the fill back (R7-Z1); no standing "not filled" note, KiCad's 5 s warning on Draw Zone Fills (R7-Z2). PARITY-pcb §67. Left:
  - A zone's settings are not sticky (`Edit_Zone_Params`' `SetDefaultZoneSettings`; Board Setup has no Zones page).
  - Properties (E) while drawing a cutout or a similar zone does nothing; Escape in the dialog is its Cancel.
  - A zone picked by its inside stands in for `HitTestFilledArea`: an unfilled pour is picked by a click inside it.
  - Add / remove / chamfer corner and Simplify empty the zone's fill (KiCad keeps the stale fill: only the corner drag `UnFill()`s).
  - The IR polygon has no holes, so a zone made a polygon is a slit ring (its holes come back when it is made a zone again).
  - M. PARITY-pcb §8, §11, §52, §68; GAPS 13.
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
- **P13 [BD] Dimensions.** **done (task-u2-lines)**: each kind has `DrawDimension`'s click sequence (an aligned and an orthogonal one's height click, a centre mark, a radial one and a leader done at the second, a leader's dialog at once, the tool armed after) and a live preview of the whole dimension (`POST /api/dimension/preview`); a leader's text frame (none, rectangle, circle) is drawn, edited in the dialog and the Properties panel, and stops its lines; the point editor drags the feature points, the crossbar ends, a radial's knee and the text, a dragged text is placed by hand (`text_position_mode 2`, now read, kept and written; it was read as 0) and follows its crossbar; the dialog has KiCad's Position mode with Manual (PARITY-pcb §69). Left:
  - The height step of a slanted aligned dimension stays on the grid (KiCad turns the grid off); no width keys, layer change or Properties during the draw.
  - A manual text's left or right justification is not flipped past the crossbar's middle (the IR's dimension text is always centred); no font, bold, italic or mirror.
  - A rounded-rectangle frame (`DIM_TEXT_BORDER::ROUNDRECT`, not offered by 8303b2ad's dialog) is kept but drawn as none, as 8303b2ad does.
  - M. PARITY-pcb §18, §69; GAPS 28/35.
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
- **P29 [BD] Found during the u1 batch (2026-11-05), not yet fixed.**
  - A new board rule area starts with every keepout off; KiCad keeps out tracks, vias and pads (found by task-u1-fpedit).
  - `layerPresets.ts` `layerGroupOp` leaves Edge.Cuts out of the copper group; KiCad's `presetAllCopper` counts it (task-u1-fpedit).
  - Barcode Properties validates the old preview if OK is pressed within about 200 ms of typing (task-u1-fpedit).
  - The existing single-track D drag snaps the cursor onto items of any net; KiCad snaps only onto the dragged line's net (task-u1-drag).
  - Add / Remove / Chamfer Corner and Simplify empty a zone's fill; in KiCad only the corner drag calls `UnFill()` (task-u1-zones).
  - S. The tasks' PARITY sections.

## Schematic editor

- **S1 [BD] Placing labels, power symbols and text.** **done (task-u1-schplace)**: the dialog first (Place Power: the chooser filtered to power symbols), then the item rides the cursor, R / X / Y turn it there, one undo step per placement (PARITY-sch §54).
  - Left: Properties of a power symbol in the hand; the right-button menu of the item in the hand; the label dialog lists the sheet in view's labels only (no other sheets, bus aliases); "Place all units" and the global/local power preference; the placed item is not left selected; Repeat puts a power symbol 100 mil down, not at the cursor.
  - (was: you click first and the dialog comes after, and nothing follows the cursor before the click; Place Power is a text field with presets, its rotation set in the dialog.)
  - M. PARITY-sch §3, §3a, §54.
- **S1b [BD] Place Symbol (A) carries the symbol.** **done (task-u2-symprops)**: A opens the chooser at once, OK puts the symbol in the hand numbered, it rides the cursor drawn by `POST /api/sch/place_preview`, R / Shift+R / X / Y turn it, the click places it (one undo step); "Place all units" (on) brings the next unit and "Place repeated copies" a copy with the next reference, Esc with it reopens the chooser; Place Next Symbol Unit and Add Symbol to Schematic hand the tool a symbol that rides at once and ends the tool (PARITY-sch §59). Left:
  - Properties, U / V / F, Cycle Body Style and the attribute toggles on the symbol in the hand are refused (KiCad edits it), and the right-button menu's Unit / Body Style for it.
  - After a mirror in the hand the fields are not autoplaced again on the click; a unit placed only on another sheet counts as free for "Place all units".
  - (was: after the chooser it waited for a click with nothing drawn at the cursor.)
  - S. PARITY-sch §54, §59.
- **S2 [FM] Design variants.** **done (task-u1-variants)**: Add, Remove, Edit Description, Rename and Copy Design Variant (Tools > Variants, the schematic toolbar's selector with "Add New Design Variant...", the Fields Table's "Schematic variants:" list), each in KiCad 25ec2335's words; a symbol's DNP, exclusion flags and field texts per variant, shown and edited for the current variant on the canvas, in Symbol Properties (the title names the variant), the Properties panel, the attribute toggles and the Fields Table (the variant's cells highlighted); KiCad's `(variant ..)` records and `schematic.variants` read and written, the board's `(variants ..)` and footprint variants derived; the PCB editor's own selector; "Design variant:" enabled in Plot, Generate Placement Files and Plot Schematic, and the Fields Table's BOM export is its variant's: kicad-cli's `--variant` (PARITY-sch §58). Left:
  - A variant's own symbol (Set / Clear Variant Symbol) and pin map; a sheet's own variants; `${VARIANT}` / `${VARIANT_DESC}` in texts; a variant field's `${..}` on the canvas.
  - The PCB canvas and Properties panel per board variant (the board's selector feeds the dialogs only); a variant footprint; a pasted symbol's records.
  - A record is the reference's, on every path of a screen shown twice.
  - XL. PARITY-sch §13, §25, §58.
- **S3 [FM] Schematic Setup.** **done (task-u1-schsetup)**: every page of KiCad's tree -- Formatting's every row, Annotation, Field Name Templates, BOM Presets, Net Classes (Board Setup's page with the Eeschema columns, one table for both), Bus Alias Definitions, Text Variables -- each Apply one undoable verb, the settings in the derived `.kicad_pro` kicad-cli reads; ERC's "Edit Netclasses…" opens Net Classes (PARITY-sch §56). Left:
  - Import Settings from Another Project… (Board Setup has none either).
  - Hop-overs are not drawn; the text sizes, label ratios, pin symbol size and overbar ratio reach kicad-cli but not the studio's own drawing.
  - "Allow reference reuse" is stored, not applied (no designator tracker); the URL column has no URL editor; the studio saves no BOM presets of its own.
  - The net class table lists Default first (KiCad: last).
  - L. PARITY-sch §9, §28, §56.
- **S4 [BD] Symbol Properties.** **done (task-u2-symprops)**: General and Pin Functions tabs; the fields grid's Show Name, H / V Align, Italic, Bold, Text Size, Font (KiCad's two choices and the machine's families) and Color (`SchExtras::field_looks`, drawn, written, read back); the Unit and Body style choices; the pin table (assignments synced to the reference's units on the sheet); Update Symbol from Library..., Change Symbol..., Edit Symbol..., Edit Library Symbol...; U / V / F open Field Properties on the one field; a power symbol's Properties has the Footprint, Datasheet, the five flags, user fields, the field looks and the pin table (`SchExtras::power_props`) (PARITY-sch §59). Left:
  - Angle and Mirror, Show pin numbers / names (S10), Embedded Files; the grid's Orientation, Position, Allow Autoplacement and Private columns, Move Up / Down.
  - A unit already placed on the sheet is refused (KiCad duplicates it); both Edit buttons open the one library copy (S10); a field added in the dialog takes its looks after OK.
  - The canvas draws an outline font's field with the stroke font; a power symbol's user fields are kept and written, not drawn.
  - (was: no unit choice, pin table, font or colour; U, V and F opened the full dialog; a power symbol's Properties had only Reference and Value.)
  - M. PARITY-sch §1, §26, §30, §59.
- **S5 [FM] Placed symbols.** Pin alternates **done (task-u2-symprops)**: a placed pin's alternate is chosen in Symbol Properties' Pin Functions tab and in the pin's Pin Function submenu (`set_pin_alternates`, one undo step), drawn with the alternate's name, type and shape, with View > Show Pin Alternate Icons (on by default); the Symbol Unit submenu (units, "(already placed)", "Place unit %s", Swap); connectivity and kicad-cli's netlist follow the alternate (`pinfunction "<alt>_<n>"`, `pintype`, a slow kicad-cli proof) (PARITY-sch §59). Left:
  - A library text box shows only its border.
  - Only the Normal and De Morgan body styles exist.
  - Alternates are synced to the reference's units on the sheet in view only; the Symbol Unit submenu's Duplicate is not possible.
  - M. GAPS 12/19; PARITY-sch §48, §59; r5 D5-H3.
- **S6 [BD] Text looks.**
  - Font face, colour and hyperlink can't be edited.
  - A free text's bold, italic and colour are kept but not drawn.
  - A label's own fields can't be clicked or moved, and a hand-moved one is autoplaced again.
  - M. GAPS 12; PARITY-sch §39, §52.
- **S7 [BD] Properties panel.** **done (task-u1-props)**: every class lists KiCad's registered rows (a test replays the C++), and the symbol's and sheet's fields are rows as KiCad's panel adds them; a label's size, bold, italic, colour and justification, a text's bold, italic, colour and hyperlink, wire ends, Body Style, Exclude From Position Files and user fields are edited; Sheetfile shows the field's own text (B3-M1) (PARITY-sch §55). Left, read-only with the reason as the tooltip:
  - Fonts; a symbol's pin display (one library symbol per lib_id). (Its Unit is `set_symbol_unit` since task-u2-symprops.)
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

- **F1 [FM] Fields and texts on the canvas.** **done (task-u1-fpedit)**, PARITY-fpedit §20: the Reference, the Value and the user fields are drawn, picked, dragged, turned and mirrored (a delete hides one); a double click or E opens the item's dialog (Footprint Reference / Value / Field / Text Properties, the shape's Properties, Pad Properties, the new kinds' dialogs, Group Properties).
  - Left: a footprint text's bold, italic, vertical justification and keep-upright are not in the model (F5), so its dialog leaves them out (a field has them); Duplicate and Create Array of a field.
  - ~~Reference, Value and user fields are not drawn or pickable.~~
  - ~~Texts and graphics have no Properties dialog.~~
  - M. PARITY-fpedit §13, §15, §17.
- **F2 [FM] Dimmed tools.** **done (task-u1-fpedit)**, PARITY-fpedit §20: every tool listed below runs on the footprint's items, one undo step each; a library footprint holds its zones, dimensions, text boxes, points, images, barcodes, tables and groups as items (read and written by the board's readers and writers, kicad-cli keeps them), drawn and editable; Set Anchor moves them (F7's zones and dimensions).
  - Left: the Properties pane (Y3, another task); Create Footprint (the wizards); a barcode and a reference image are drawn as their boxes; Duplicate, Create Array and the clipboard of the new kinds; the Appearance manager's presets, viewports and Selection Filter; items still kept as text (a locked item, a filled zone) are not drawn; the board's footprints still carry these items as text (GAPS 25).
  - ~~Mirror, Group and Ungroup, the line modes, the sketch and outline modes, the Appearance manager and the Properties pane.~~
  - ~~Dimensions, rule areas and keepouts, text boxes, tables, barcodes, points and reference images.~~
  - ~~A footprint's zones and dimensions are kept but not drawn.~~
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
- **F7 [BD] Set Anchor.** It leaves zones, dimensions and the courtyard override behind (E3-1). S. r3 E3-1. (task-u1-fpedit: the zones, dimensions, text boxes, images, barcodes and tables move with it now, as `MoveAnchorPosition` does; the courtyard override and the items still kept as text do not.)
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

- **Y1 [BD] Rotate.** It turns only the pins; the selected shapes and text stay put. M. PARITY-symedit.
- **Y2 [FM] Mirror and beziers.**
  - Mirror H and V are not ported.
  - No bezier tool, beziers aren't drawn, and Import Symbol drops them.
  - M. editor_toolbar_support.json; PARITY-symedit.
- **Y3 [FM] Properties pane.** The Symbol and Footprint editors have none. M. GAPS 10; PARITY-common §12.
- **Y4 [FM] Symbol Properties and text.**
  - No unit display-name grid, no Pin Connections tab and no named body styles.
  - Plain text has no Properties dialog.
  - No border colour, custom fill, Private flag or font.
  - M. GAPS 19; PARITY-symedit.
- **Y5 [BD] Pins and the clipboard.**
  - The pin tool takes a click, then a dialog, with no ghost.
  - Duplicate and Paste drop in place.
  - The editor uses its own clipboard, not `.kicad_sym` text.
  - No increment of a pin's number or name.
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

- **X1 [FM] PCB Plot is Gerber only.** **done (task-u1-plot)**: Gerber, PostScript, SVG, DXF, PDF, PNG with every option of `DIALOG_PLOT`, Plot on All Layers, the output folder, Run DRC, the mask alert. HPGL is not offered (KiCad's dialog dropped it; kicad-cli: "No longer supported"); 8303b2ad's dialog has no values/references boxes. The job file box works (unticked, the job file kicad-cli always writes is deleted). Left, disabled with the reason: the origin outside Gerber/DXF, SVG precision, PDF metadata, the PDF popups apart, DXF's font. (The design variant is done: S2.)
  - L. GAPS 37; PARITY-boardctl §5, §8.
- **X2 [FM] Drill files.** **done (task-u1-plot)**: Excellon and Gerber X2, map files and their format, units, zeros, precision, mirror, minimal header, tenting layers, the output folder, Generate Report File. Left: the report is the selected format's writer's (KiCad's dialog inverts the test). M. GAPS 37; PARITY-boardctl §8.
- **X3 [FM] Schematic exports.** **done (task-u1-plot)**: Plot Schematic has PostScript, PDF, SVG, DXF, PNG, the colour theme, the PDF and PNG options, the output folder, Plot All / Current Page; Export Netlist has KiCad, OrcadPCB2, Allegro, CadStar, PADS, SPICE, SPICE Model (and XML). Left, disabled with the reason (no kicad-cli flag): page size, minimum line width, DXF units, the SPICE page's options, Open file after plot, custom exporters. (The variant is done: S2.)
  - S. PARITY-sch §10, §53.
- **X4 [FM] Placement files.** **done (task-u1-plot)** as far as kicad-cli goes: kicad-cli 10.99 (4fe1ca2060) still has no Exclude from BOM flag for `pcb export pos` (disabled with the reason; the api-server's position job has `exclude_from_bom`, unused); the variant choice is `--variant` since S2 (task-u1-variants); the dialog's output folder is wired. S. GAPS 37; r2 X8.
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

- **M1 [FM] Simulation Model dialog.** **done (task-u1-simmodel)**: "Built-in SPICE model" with KiCad's Device and Device type choices, the Parameters grid (every model's parameters copied from the C++ by `tools/extract-sim-models.js`, in KiCad's categories), the Code tab, "Save parameter '%s (%s)' in Value field", the inference of passives and DC sources, OK writing the fields as `WriteFields` does; IBIS files list their components, pins, pin models, differential pins and waveforms; kicad-cli's netlist of what the dialog wrote is KiCad's generator's (PARITY-sch §57). Left:
  - IBIS: KIBIS's model tables, checks and SPICE (kicad-cli writes the SPICE).
  - A library `.model`'s parameters in the grid; arithmetic in the grid; K's inductor choice.
  - OK is its own undo step and adds hidden fields (KiCad's inference adds visible ones).
  - XL. GAPS 34; PARITY-sch §49, §51, §57.
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
