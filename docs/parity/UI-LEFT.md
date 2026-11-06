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
- `web/studio/src/kicad/editor_toolbar_support.json` lists the tools not yet ported in the library editors: 24 in the Footprint Editor and 7 in the Symbol Editor. **Footprint Editor: 2 left since task-u1-fpedit** (Create Footprint, the Properties pane); **1 since task-u3-fpedit2** (Create Footprint). **Symbol Editor: 3 left since task-u1-symedit** (Find, Find and Replace, the Symbol Checker).

**Top 12, all areas:**
1. X1: the Plot dialog is Gerber only -- **done (task-u1-plot)**
2. P1: teardrops -- **done (task-u1-pcb)**; per-pad parameters **done (task-u5-pcbfp)**
3. S1: placing labels, power symbols and text -- **done (task-u1-schplace)**
4. R1: dragging a footprint with its tracks -- **done (task-u1-drag)**
5. L1: saving into table libraries
6. F1+F2: Footprint Editor fields and dimmed tools -- **done (task-u1-fpedit)**; the Properties pane (Y3) **done (task-u3-fpedit2)**
7. Y1: Symbol Editor Rotate -- **done (task-u1-symedit)**
8. P2/S7: Properties panel rows -- **done (task-u1-props)**
9. S3: Schematic Setup pages -- **done (task-u1-schsetup)**
10. P8: zones -- **done (task-u1-zones)**
11. S2: design variants -- **done (task-u1-variants)**
12. M1: the Simulation Model dialog -- **done (task-u1-simmodel)**

## PCB editor

- **P1 [BD] Teardrops.** **done (task-u1-pcb)**: SMD pads of every shape but a custom one, tracks ending off centre in a via or pad or crossing one (R7-1), each polyline segment its own track; 39 kicad-cli outlines point for point (PARITY-pcb §66). Left:
  - ~~Each via's and pad's own teardrop parameters are not read~~ **done (task-u5-pcbfp)**: read, written, edited (Track & Via and Pad Properties, the Properties panel) and honoured by the generator, kicad-cli's outlines point for point (PARITY-pcb §77). Custom pads get none.
  - One segment, straight edges, no track-to-track; no fit to other nets' clearance, no mask teardrops.
  - Not kept current as you edit: you re-run Add All Teardrops.
  - L. GAPS 8/13/35; PARITY-pcb §15, §62 D6-1, §66; r7 R7-1.
- **P2 [BD] Properties panel.** **done (task-u1-props)**: every class lists KiCad's registered rows in KiCad's groups and order (a test replays the C++); via tenting, covering, plugging, capping, filling, backdrill, post-machining, layers and type, track and shape solder mask, corner radius, text italic, bold, vertical justification, width apart from height, knockout and automatic pen, a pad's orientation, copper layers and overrides, a footprint's reference and (board-file) paste margins are edited; Component Type, which KiCad's panel lacks, is gone (PARITY-pcb §67). Left, read-only with the reason as the tooltip:
  - Fonts, a pad's number, post-machining and backdrill, a dimension's label style (a shape's line style and a leader's text frame are edited since task-u2-lines, P5, P13; the teardrop rows, a pad's pin name and type, fabrication property and pad-to-die since task-u5-pcbfp, P3).
  - Text colour is masked on board text, as in KiCad (a footprint placed from the intent has its own paste margins since task-u5-pcbfp, P14).
  - M. GAPS 10; PARITY-pcb §23, §67.
- **P3 [FM] Track & Via and Pad Properties.** **done (task-u5-pcbfp)**: Track & Via Properties' Padstack mode and Edit layer, Protection features (the IPC-4761 presets), Backdrill with its sizes and must-cut layers, Hole Post-machining and the via's own teardrops; Pad Properties' Fabrication property, pad-to-die length and delay and the pad's own teardrops; the panel's Pin Name and Pin Type; each via's and pad's own teardrop parameters honoured (kicad-cli proof); a zone's force-flash (`ZLO_FORCE_FLASHED`) read from kicad-cli's fill, applied to the flashed layers and written back (kicad-cli proof, with a Solid zone and a pad's FULL connection); a custom padstack's per-layer thermal and clearance kept and written, the dialog editing the front's as KiCad's does (PARITY-pcb §77). Left:
  - A via's padstack diameters are drawn, routed and checked at the front's (kicad-cli reads them all).
  - Curved and two-segment teardrops are kept, not built (P1).
  - M. PARITY-pcb §77.
- **P4 [BD] Board text.** **done (task-u1-pcb)** for Bold: the Text dialog has KiCad's Bold, and a new copper text box is bold as `InferBold` makes it (R7-3; PARITY-pcb §66). Left:
  - The Text dialog has no Italic.
  - A text is centred on its anchor whatever its vertical justification.
  - Outline and TrueType fonts are drawn in the stroke font.
  - M. PARITY-pcb §39, §60 B5-2, §62 A6-3.
- **P5 [BD] Line styles.** **done (task-u2-lines)**: a board shape's line style (dash, dot, dash-dot, dash-dot-dot, the file's default) is read, written, drawn as `STROKE_PARAMS::Stroke` cuts it with the width-scaled lengths and the board's dash ratios (board shapes, footprint graphics on the board and in the Footprint Editor), edited in Shape Properties' "Line style:" and the Properties panel's Line Style; kicad-cli plots a dashed F.SilkS line as dashes (slow proof); the QA round trip 22497 -> 20873 lines (PARITY-pcb §69). Left:
  - A shape drawn in the studio is written `(type solid)`, pcbnew's new shapes `(type default)` (both drawn solid).
  - ~~The Footprint Editor's shape dialog has no Line style row (the library's stroke type is drawn and written, not edited).~~ **done (task-u3-fpedit2)**: the shape dialog's Line style and hatched Fill, and the Properties pane's Line Style (F5, PARITY-fpedit §21).
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
- **P9 [FM] Net Inspector.** **done (task-u3-panels)**: a docked pane under the canvas (View > Panels > Net Inspector, checked; the Appearance panel's button), with the search box and the configure button; Group by Netclass and custom groups (`EDA_COMBINED_MATCHER`, KiCad's prompt), their sums, opened and closed; Show Time Domain Details (the tuning profiles' delays from `CalculateLengthDetails`); Show / Hide Columns; a double click or Enter highlights the rows' nets, the board's highlight selects their rows; Rename / Delete / Add Net with KiCad's prompts and messages, each one undo step (the board's net renamed or deleted on its pads and copper, the schematic's netlist left as it is); the settings kept per person. PARITY-pcb §73. Left:
  - The columns cannot be dragged into another order or resized, so the header menu has no Auto-fit entries; the pane docks at the bottom only.
  - With Strict on, a routed net cannot be deleted (its copper would short on no net); turn Strict off to delete it, as KiCad allows.
  - A renamed net's class follows its new name's patterns, and a custom rule that names it is not rewritten. KiCad's rename and delete are not undoable, ours are.
  - The report is a download (no file dialog).
  - M. GAPS 29; PARITY-pcb §32d, §73.
- **P10 [FM] Search panel.** **done (task-u3-panels)**: a docked pane under the canvas, shown by Search (Ctrl+G), View > Panels > Search and Find's "Show search panel" link, the box focused; the Footprints, Zones, Nets, Ratsnest, Text, Groups and Drills tabs with KiCad's columns and permissive matching; the sort and the heading clicks; selecting rows selects their items on the board and pans or zooms to them (the nets highlighted on Nets and Ratsnest); a double click opens the properties (Board Setup's Net Classes for a net); Ctrl+A, Ctrl+C, Copy; Search Hidden Fields and Search Metadata kept per person. The Activity tab is no longer involved. PARITY-pcb §74. Left:
  - X and Y are from the page origin (no display-origin setting); the columns are fixed, not resizable.
  - A net no item names is not on the Nets tab; the schematic has no Search pane.
  - S. GAPS 21, 29; PARITY-pcb §7a, §32b, §74.
- **P11 [FM] Colour theme.** **done (task-u3-panels)**: Preferences > PCB Editor > Colors (the theme list, New Theme... with KiCad's prompt and "Theme already exists!", each layer's swatch with Copy / Paste / Revert to saved color, Reset Colors to Defaults); the Appearance panel's layer and object swatches edit the editor's theme, and a read-only theme says KiCad's message; the board is drawn in the chosen theme; the themes are the person's, kept in the browser like the other preferences; Use Color from Schematic takes the class's schematic colour; a class renamed in Board Setup keeps its colour (PARITY-pcb §75). Left:
  - KiCad has no Duplicate button: New Theme... starts from the shown theme's colours, which is the duplicate.
  - No preview board (the page draws on the board while it is up) and no theme files in or out ("Open Theme Folder").
  - The Footprint Editor uses the board editor's theme; the schematic and symbol editors have no Colors page.
  - Undo of a net class rename does not bring the colour back to the old name (the colour is outside the undo history).
  - S. GAPS 18; PARITY-pcb §24, §75.
- **P12 [FM] Global edits.** **done (task-u3-panels)**: Edit Text & Graphics Properties walks the footprints first, as `TransferDataFromWindow` does: Reference designators, Values, Other footprint fields (and a `${REFERENCE}` / `${VALUE}` field or text with its box), Footprint graphic items and Footprint text items; By layer, By parent reference designator and By parent footprint library link (wildcards, remembered for the session), Selected items only through the footprint and its group; KiCad's Action rows -- Layer, Visible (fields only), Line thickness, Font ("KiCad Font" or a family; a field's face is now read and written), Bold, Italic, Text width and height apart, Text thickness with auto, Keep upright, Center on footprint -- each "-- leave unchanged --" to begin with, and Set to layer default values over the layer-class grid. Edit Track & Via Properties has the Microvias box (a via's own type), Via annular rings and Via protection features (KiCad's 19 IPC-4761 presets). Each OK is one undo step. PARITY-pcb §76. Left:
  - Footprint dimensions picks nothing: a footprint's dimensions, text boxes, tables and barcodes are kept as the text of their forms (GAPS 25).
  - A footprint's hidden texts and its courtyard's own graphics are not reached; a footprint graphic or text edited gives the instance its own copy of the footprint (the library's art is shared).
  - The library link filter compares the part's footprint name (a built-in package's bare `0603`, not the KiCad footprint it is written as).
  - A dimension's label: one size, no bold, italic or font, its "auto" thickness written as the pen; a value box takes a number, not an expression.
  - S. GAPS 35; PARITY-pcb §13, §76.
- **P13 [BD] Dimensions.** **done (task-u2-lines)**: each kind has `DrawDimension`'s click sequence (an aligned and an orthogonal one's height click, a centre mark, a radial one and a leader done at the second, a leader's dialog at once, the tool armed after) and a live preview of the whole dimension (`POST /api/dimension/preview`); a leader's text frame (none, rectangle, circle) is drawn, edited in the dialog and the Properties panel, and stops its lines; the point editor drags the feature points, the crossbar ends, a radial's knee and the text, a dragged text is placed by hand (`text_position_mode 2`, now read, kept and written; it was read as 0) and follows its crossbar; the dialog has KiCad's Position mode with Manual (PARITY-pcb §69). Left:
  - The height step of a slanted aligned dimension stays on the grid (KiCad turns the grid off); no width keys, layer change or Properties during the draw.
  - A manual text's left or right justification is not flipped past the crossbar's middle (the IR's dimension text is always centred); no font, bold, italic or mirror.
  - A rounded-rectangle frame (`DIM_TEXT_BORDER::ROUNDRECT`, not offered by 8303b2ad's dialog) is kept but drawn as none, as 8303b2ad does.
  - M. PARITY-pcb §18, §69; GAPS 28/35.
- **P14 [BD] Footprint Properties on the board.** **done (task-u5-pcbfp)**: the four mandatory rows (Reference, Value, Datasheet, Description); Reference and Value edited and carried to the symbol on whichever sheet it is (a board-only footprint renamed and revalued on the board); a repeated field name taken as KiCad's reader takes it; the footprint's paste clearance for any footprint (PARITY-pcb §77). Left:
  - A Datasheet or Description edited here is the footprint's own, not carried to the symbol; a user field named like a mandatory one is still refused.
  - The footprint's own mask margin and "Allow bridged solder mask apertures" are not edited here.
  - M. PARITY-pcb §77.
- **P15 [BD] Footprint graphics on the board.** **done (task-u5-pcbfp)**: a footprint's own texts are items of the board (picked, moved, turned, flipped, edited in Text Properties and deleted, in the instance's own copy); the title block's and the board's variables resolved in them; a hatched graphic drawn with its hatch lines (PARITY-pcb §77). Left:
  - A footprint's own graphics are picked as the footprint, as in KiCad's board editor ("Most footprint children can only be selected in the footprint editor"); the global edit and the Footprint Editor edit them.
  - M. PARITY-pcb §77.
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
- **P21 [BD] Footprint exchange and swaps.** **done in part (task-u5-pcbfp)**: Change and Update Footprint match the pads as `ExchangeFootprint` does (by number, then by place and shape; a renumbered pad takes the old pad's net, pin name and type), Update PCB from Schematic's exchange keeping them by number; Compare with Library tests the attribute flags against the library's own, the solder mask bridges and the net ties (PARITY-pcb §77). Left:
  - Library pad layers, die lengths and properties aren't carried onto a footprint placed or updated from the library.
  - Update Schematic from PCB doesn't carry a pin or gate swap back to the schematic (25ec2335's `applyPinSwaps`, `PlanBackannotateUnitSwaps`).
  - M. GAPS 26; PARITY-pcb §30, §45, §61, §77.
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
- **S6 [BD] Text looks.** **done (task-u3-schtext)**: Text, Label and Text Box Properties edit the Font (KiCad's choices and the machine's families), the Color and, for a text and a text box, the Link (the hierarchy's pages, `ValidateHyperlink`'s message) and Exclude from simulation (`set_text_attrs`, `(face ..)` written and read); a free text's bold, italic and colour and a label's and a text box's colour are drawn; a label's fields (the Sheet References included) are items -- clicked, dragged, turned and edited on the canvas (Field Properties with Font and Color), and a field placed by hand stays (no `(fields_autoplaced ..)`, read back from KiCad) until Autoplace Fields; the Properties panel's Font row edits a text's and a label's face (PARITY-sch §60). Left:
  - An outline font is drawn with the stroke font; a text placed with T or a text box being drawn takes its font, colour and link after it is placed.
  - A label's field keeps no size, bold or shown name; a field added to a label placed by hand goes where Autoplace Fields puts it.
  - (was: font, colour and link not editable; a free text's bold, italic and colour not drawn; a label's fields neither clicked nor kept where moved.)
  - M. GAPS 12; PARITY-sch §39, §52, §60.
- **S7 [BD] Properties panel.** **done (task-u1-props)**: every class lists KiCad's registered rows (a test replays the C++), and the symbol's and sheet's fields are rows as KiCad's panel adds them; a label's size, bold, italic, colour and justification, a text's bold, italic, colour and hyperlink, wire ends, Body Style, Exclude From Position Files and user fields are edited; Sheetfile shows the field's own text (B3-M1) (PARITY-sch §55). Left, read-only with the reason as the tooltip:
  - A text box's and a directive label's font; a symbol's pin display (one library symbol per lib_id). (Its Unit is `set_symbol_unit` since task-u2-symprops; a text's and a label's Font and a sheet's border, fill and attributes are editable since task-u3-schtext.)
  - A text box's margins, fill, colour and hyperlink (its dialog edits the last three); an image's transform offset.
  - M. PARITY-sch §17, §30, §45, §55.
- **S8 [FM] Hierarchy.** **done (task-u3-schtext)**: the Navigator's right-click menu -- Edit Page Number, Rename in place, Delete Top-Level Sheet ("Delete top-level sheet '%s'?..." / "Cannot delete the last top-level sheet."), New Top-Level Sheet ("Enter name for new top-level sheet:"), Expand / Collapse All -- and its net highlight (the sheets holding the highlighted net in `LAYER_BRIGHTENED`); a KiCad 10 project's top-level sheets shown as roots of their own (`virtual_root`); a sheet's border width and colour, fill colour and Exclude from simulation / bill of materials / board and Do not populate kept, written, read, drawn (fill, DNP cross) and edited in Sheet Properties and the Properties panel; Add Field, Move and Delete of a sheet's user fields in Sheet Properties (PARITY-sch §60). Left:
  - New Top-Level Sheet on a design whose root is its only top-level sheet (offered disabled); the derived files keep the root that places a project's top-level sheets.
  - A sheet's attributes are its own on every path and variant and are not given to its symbols in the studio's own tables; a sheet with no style of its own is drawn unfilled (written with the old pale fill).
  - The rows read "name file · page N" (A7-L3); a local net on a screen shown twice lights its first path's row only.
  - M. PARITY-sch §30, §40, §50, §52, §60; r2 B-L1, r6 C6-L2, r7 A7-L3.
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
- **S11 [FM] Symbol Fields Table.** **done (task-u3-schtext)**: the scope (Entire Project, Current Sheet Only, Current Sheet and Down, and 25ec2335's Selection); the project's view presets (the box with the built-in and the project's in name order, "Save preset..." -- "BOM preset name:", built-in names refused, "Overwrite existing preset?" -- and "Delete preset...", in `schematic.bom_presets`; the box shows the preset the view is); the field name templates as hidden columns; the project's `bom_settings` view on open; "Save changes?" with Save / Discard Changes / Cancel on a close; Rename Field's and Remove's prompts; the variant columns kept (PARITY-sch §60). Left:
  - "Add field from library" is not a KiCad feature (its only Add is "New field name:"); KiCad has no Rename of a preset either (save under the new name, delete the old).
  - A preset is saved when saved (KiCad at the close) and the closing view is not kept as `bom_settings`; with no saved view the user fields are shown.
  - The Description column, editable BOM Name labels, the grid's context menus, the Group By refusals, the format presets' Save / Delete, Set / Clear Variant Symbol.
  - M. PARITY-sch §7, §25, §60; r5 C5-L9.
- **S12 [FM] Find/Replace.** **done (task-u3-schtext)**: Regular Expression (in Find and Replace, Whole words only winning over it; `wxRegEx`'s syntax with `\N` and `&` in the replacement: `eda_model::wx_regex`, the one engine shared with the net class patterns, no crate); the search reaches sheets' names, files and user fields, sheet pins, power symbols' values, text boxes and directive labels' fields beside the symbols' fields and pins; Replace renames a sheet, a sheet pin, a power symbol's net, a text box, a field, not a sheet's file nor a symbol's pin (as KiCad); "Search the current selection only" in both modes (PARITY-sch §60). Left:
  - Library metadata: KiCad's dialog has no box for `searchMetadata` either; groups and ERC markers are not searched.
  - The engine's match is leftmost-first (POSIX: leftmost-longest), which shows only in Replace's span with alternations; it does not upper-case the pattern as KiCad does; a power symbol's Reference is found, not replaced.
  - S. GAPS 1/4; PARITY-sch §8, §60.
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
- **R2 [FM] Diff pairs. done (task-u3-router)** -- pns Stage 23.
  - ~~No arcs in the pair router or the tuners (F4-2).~~ Done: a pair goes on from two arc track ends (`DP_PRIMITIVE_PAIR` / `BuildFromPrimitivePair` with `ARC_T`); the pair and skew tuners take an arc (`AssembleDiffPair` by centre and radius, `addCornersUntilIndex`'s `AddArc`). KiCad's pair placer itself draws mitered corners only.
  - ~~The blind, buried and microvia commands do nothing in the pair tool.~~ Done: `DIFF_PAIR_PLACER::makeVia`'s via type, committed with it.
  - ~~F finishes at the cursor.~~ Done: `ROUTER::Finish` with the P line, finished on the coupled pair at the nearest anchor, else the pair goes on.
  - Left: from the arcs' ends a cursor steeply off their chord direction fits no head, in both modes (seen in the kicad-cli proof's setup; not compared with KiCad).
  - M. GAPS 7; pns Stages 11-12, 23.
- **R3 [BD] Length tuner. done (task-u3-router)** -- pns Stage 23 (branch `task-u3-router-tune`).
  - ~~No pad-to-die length or delay, and a time-domain rule gives no target.~~ Done: the pads' `(die_length ..)` / `(die_delay ..)` count; a time-domain rule gives the target delay (`EditStart`).
  - ~~A pattern can't be moved or rotated.~~ Done: moved, turned and flipped as a generator with other items; alone it slides along its track and is tuned again.
  - ~~The corner style and single-sided settings are ignored.~~ Done: the tool's settings start from Board Setup's page each time it is entered.
  - ~~The status box sits at the cursor.~~ Done: `TUNING_STATUS_VIEW_ITEM::ViewDraw`'s box at the mouse position.
  - Left: the lone move previews rigidly (the slide is applied at the drop) and its start snaps to segments only; a pair pattern's merged outline; the pattern's message-panel lines; a lone pattern's Duplicate refusal; the time domain for a class whose profile has it off.
  - M. GAPS 7; pns Stages 8, 23; PARITY-pcb §35, §65.
- **R4 [BD] Autoroute. done (task-u3-router)**: the autoroute (Route, the FreeRouting port) routes round the board outline's cutouts, keep-outs on every copper layer as KiCad's DSN export writes them, the edge clearance kept from their rims; kicad-cli's DRC is clean. Left: the generated pours still cover the cutout (the fill clears it); FreeRouting's own reading of the cutout keep-out not re-dumped. M. GAPS 27.
- **R5 [BD] Layers and vias. done (task-u3-router)** -- pns Stage 23.
  - ~~V goes to the layer pair's other layer, not to the nearest ratsnest anchor's.~~ Done (`GetNearestRatnestAnchor`, the sizes' `PairedLayer`), for a route, a pair and V before a route.
  - ~~The clearance isn't re-read after a layer switch.~~ Done: `updateSizesAfterRouterEvent` after every fix, via and layer switch (the width and a pair's gap too, as the placers' `UpdateSizes` allow); the message panel shows them.
  - ~~V armed before a route outlives the tool (R7-R6).~~ Done.
  - ~~Autoroute leaves the active layer changed (R7-R7).~~ Done.
  - Left: the router sizes' layer pairs start afresh with each route (KiCad keeps the last route's via pair in `m_savedSizes`).
  - S. pns Stages 11-21, 23.
- **R6 [BD] The head's shape.**
  - A fan-out elbow isn't straightened (`optimizeTailHeadTransition`).
  - No hull snap in highlight-collisions mode.
  - `reduceTail` and `handlePullback` are not ported.
  - Earlier vias aren't shoved by a later leg.
  - M. pns Stages 2, 10, 12.
- **R7 [BD] Grab radius and widths. done (task-u3-router)** -- pns Stage 23.
  - ~~The start and end grab radius is a fixed 0.5 mm, not the grid size.~~ Done: the grid, measured to the copper (`pickSingleItem`'s `maxSlopRadius`, `snapToItem`), for routes, pairs and drags.
  - ~~Automatically Select Track Width stays on after a toolbar pick.~~ Not KiCad's: at 8303b2ad a toolbar pick sets `m_TempOverrideTrackWidth` and leaves "Automatically select track width" on (`Tracks_and_Vias_Size_Event`'s `ID_AUX_TOOLBAR_PCB_TRACK_WIDTH`); the toolbar's Track Width and Via Size boxes, display-only before, now do exactly that.
  - ~~The width keys cycle.~~ KiCad's already (Stage 18): the first press of a route at an inherited width overrides it, then the index steps and wraps, as `TrackWidthInc` / `Dec`.
  - Left: `pickSingleItem`'s free-pad and no-net (Highlight collisions) special cases.
  - S. pns Stages 12, 23; PARITY-boardctl §4.
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
  - Left: Duplicate and Create Array of a field. (A footprint text's bold, italic, vertical justification and keep-upright are in its dialog since task-u3-fpedit2, F5.)
  - ~~Reference, Value and user fields are not drawn or pickable.~~
  - ~~Texts and graphics have no Properties dialog.~~
  - M. PARITY-fpedit §13, §15, §17.
- **F2 [FM] Dimmed tools.** **done (task-u1-fpedit)**, PARITY-fpedit §20: every tool listed below runs on the footprint's items, one undo step each; a library footprint holds its zones, dimensions, text boxes, points, images, barcodes, tables and groups as items (read and written by the board's readers and writers, kicad-cli keeps them), drawn and editable; Set Anchor moves them (F7's zones and dimensions).
  - Left: Create Footprint (the wizards) (the Properties pane is done, Y3, task-u3-fpedit2); a barcode and a reference image are drawn as their boxes; Duplicate, Create Array and the clipboard of the new kinds; the Appearance manager's presets, viewports and Selection Filter; items still kept as text (a locked item, a filled zone) are not drawn; the board's footprints still carry these items as text (GAPS 25).
  - ~~Mirror, Group and Ungroup, the line modes, the sketch and outline modes, the Appearance manager and the Properties pane.~~
  - ~~Dimensions, rule areas and keepouts, text boxes, tables, barcodes, points and reference images.~~
  - ~~A footprint's zones and dimensions are kept but not drawn.~~
  - L. editor_toolbar_support.json; fpedit §8, §16.
- **F3 [BD] Ctrl+E from the board.** **done (task-u3-fpedit2)**, PARITY-fpedit §21: Edit Footprint opens a copy linked to that board footprint (the info bar, Open in library), and Save / Update Footprint on Board puts it back on that one footprint (`ExchangeFootprint` with the pads matched by position), one undo step of the board; the board's Footprint Properties has Edit Footprint... and Edit Library Footprint...; a library edit stays the library's.
  - Left: the footprint's own paste margins, private layers and jumper groups are not carried onto the board (the board model has no place for them), nor a library pad's layer set, die length or fabrication property (GAPS 26).
  - ~~It edits the library entry, with no link back to that footprint.~~
  - ~~Update Footprint on Board updates every footprint of that name.~~
  - M. fpedit §1, §6, §21.
- **F4 [FM] Footprint Properties.** **done (task-u3-fpedit2)**, PARITY-fpedit §21: the name is editable with `checkFootprintName`'s refusals and the Overwrite question; Clearance Overrides (clearance, mask, paste absolute and relative through `MARGIN_OFFSET_BINDER`, zone connection), Private Layers, Net Ties and jumper groups ("Pads with duplicate numbers are jumpers"), read, written where KiCad writes them and read back, one undo step.
  - ~~The name is read-only.~~
  - ~~Clearances has only the zone connection.~~
  - ~~No Private Layers tab, and no Net Ties or jumper groups.~~
  - M. fpedit §5, §21.
- **F5 [BD] What saving loses.** **done (task-u3-fpedit2)**, PARITY-fpedit §21: a shape's stroke type and hatch, a text's vertical justification, bold, italic, width, knockout and keep-upright and the locked flags are kept by item id, edited in the dialogs, written and read back, and carried to the board and back; a hidden user text is neither drawn nor picked.
  - Left: Duplicate does not copy the looks of what it copies; a hidden user text is written `(hide yes)`, which KiCad's own writer keeps for fields only.
  - ~~Stroke type, hatch, vertical justification, bold, italic, keep-upright and the locked flags are lost on save.~~
  - ~~Hidden user text shows.~~
  - M. GAPS 25; fpedit §4, §16, §21; r4 E4-F2.
- **F6 [BD] Pad Properties.** **done (task-u3-fpedit2)**, PARITY-fpedit §21: the copper choice (`UpdateLayersDropdown`, a PTH pad's unconnected-layer modes) and the technical layer boxes beside the presets; Fabrication property; Pad To Die Length and Delay; Round / Oblong keeps the size and is named "Oblong"; the padstack mode was there.
  - Left: the board's footprints do not carry a library pad's layer set, die length or property (GAPS 26). (E7-F7, the preset against a pad's `*.Mask`, is fixed: the sets are compared with the wildcards spread.)
  - ~~Layers are set only through presets.~~
  - ~~No fabrication property or pad-to-die length.~~
  - ~~Round/Slot resets the size, and the type is named "Slot" where KiCad says "Oblong" (E7-F6/F7).~~
  - M. fpedit §3, §18, §21.
- **F7 [BD] Set Anchor.** **done (task-u3-fpedit2)**, PARITY-fpedit §21: the courtyard and the items kept as the text of their forms (a locked or filled zone ...) move with it too, as `MoveAnchorPosition` moves `Zones()`, `GraphicalItems()` and the courtyard cache; a point stays. Left: nothing. ~~It leaves zones, dimensions and the courtyard override behind (E3-1).~~ S. r3 E3-1. (task-u1-fpedit: the zones, dimensions, text boxes, images, barcodes and tables.)
- **F8 [FM] Missing tools and settings.** **done (task-u3-fpedit2)**, PARITY-fpedit §21: File > Import > Graphics... (and a dropped file) puts a file's shapes into the footprint with the board's importer, one undo step; Preferences > Footprint Editor has Display Options (task-u4-common's), Origins & Axes, Editing Options (the rotation step R turns by, the 45-degree constraint, the arc editing mode, the magnetic points) and Graphics Defaults (what the drawing and Text tools start with).
  - Left: the Grids, Colors, Toolbars, Footprint Defaults and User Layer Names pages; the arc editing mode is kept, not read by the point editor.
  - ~~No Import Graphics and no preference pages.~~
  - ~~The rotation is fixed at 90°.~~
  - ~~No Save Library As.~~ **done (task-u1-libsave)**: a library row's Save As makes a new `.pretty` with its row and copies every footprint into it (PARITY-fpedit §19).
  - M. GAPS 28; fpedit §14, §17; PARITY-common §6.
- **F9 [BD] Footprint Checker.** **done (task-u3-fpedit2)**, PARITY-fpedit §21: the markers are where `runChecks` puts them (a short at `Collide`'s contact point, the hole checks at the first pad, the footprint's findings at its origin), and the scratch board carries the footprint's Reference and Value, so the findings name it. KiCad's checker tests no field itself (`CheckFootprintAttributes` and the DRC providers it runs read none), so there is no Reference or Value test to port. ~~Reference and Value aren't checked, and markers sit on the first item.~~ S. GAPS 29; fpedit §12, §21.
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
- **Y3 [FM] Properties pane.** The Symbol and Footprint editors have none. M. GAPS 10; PARITY-common §12. **Symbol Editor done (task-u1-symedit)**: KiCad's rows of pins, shapes, texts, text boxes, fields and the symbol, edits on the editor's undo stack; left: the rows a library symbol has no place for (font, colours, line style, hyperlink) are read-only. **Footprint Editor done (task-u3-fpedit2)**, PARITY-fpedit §21: `PCB_PROPERTIES_PANEL` beside the Libraries tree on the selected pads, shapes, texts and fields (the footprint with nothing selected), KiCad's rows as a library editor lists them (a test against the PCB registration table), one undo step per edit, Show Properties toggles it; left: a pad's type, shape, hole shape, copper layers, post-machining, backdrill and teardrop rows, fonts, colours, line endings and the footprint's orientation are read-only with the reason as the tooltip.
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

- **B1 [FM] Component Classes.** **done (task-u2-boardsetup)**: Design Rules > Component Classes is KiCad's page -- Assign component class per sheet, the custom assignments with the seven condition kinds of the "+" menu, Import references from selection, Show library, Highlight matching footprints, Match all / any, Validate's "Error with conditions for component class assignment %s" -- one undoable verb (`set_component_classes`) kept in the derived project's `component_class_settings`; kicad-cli's DRC reads a class the page defined through `A.hasComponentClass(..)` (proved; PARITY-pcb §70). Left: nothing known.
- **B2 [FM] DRC Rule Editor.** **done (task-u2-boardsetup)**: Show Matches (`GetItemsMatchingRule` on the rule board: select, frame, brighten, "Show Matches (%d)" / "(error)"); the rule tree's drag-and-drop (one verb, the file's order follows); Check rule syntax expands the text variables (R6-D5) and adds `checkPlausibility`'s warnings; the Custom Rules editor with brace matching, auto-complete and KiCad's Syntax help (PARITY-pcb §71). Left:
  - Show Matches does not see a footprint's own fields, graphics and texts, dimensions or tables (the rule board has no item for them).
  - Dragging a category, a constraint node or an unsaved rule does nothing (KiCad moves the tree item).
  - M. GAPS 3; PARITY-pcb §48, §63, §71.
- **B3 [FM] Physical Stackup.** **done (task-u2-boardsetup, from task-u2-stackup)**: one row per dielectric sublayer (`addsublayer` read and written with every value and `locked`, r2 F17), Add/Remove Dielectric Layer, Impedance controlled, the thickness lock and Adjust Dielectric Thickness, the "Errors" checks; fewer copper layers ask KiCad's question and delete the items in one undoable step (PARITY-pcb §72). Left:
  - The material list ("..."), Export to Clipboard, selecting an item from Show Details, pads with inner-layer shapes in the list.
  - S. PARITY-pcb §72.
- **B4 [FM] Net Classes.** **done (task-u2-boardsetup)**: a bus pattern (`D[0..7]`, `{SDA SCL}`) is stored as its members, in the page, Assign Netclass and the project reader (`ForEachBusMember`); the Tuning Profile column (one undo step with the classes; the Tuning Profiles page's class table is gone, as KiCad has none) and the PCB Color column with Import colors from schematic; Assign Netclass previews the matching nets on the board and the schematic; Schematic Setup's page keeps both (PARITY-pcb §70). Left:
  - The schematic's net class labels are not read as assignments.
  - The PCB colour is the Appearance panel's (`appearance.json`), outside Undo.
  - The Default class is first in the table (KiCad lists it last).
  - The transmission-line calculator is not ported (the Tuning Profiles grid's cells take typed values).
  - S. PARITY-pcb §9, §70.
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
- **L2 [FM] Manage Libraries.** **done (task-u4-libs)**: every tab of Manage Libraries is edited as KiCad's: the Global tab writes the global table into KiCad's configuration folder, a chained table's tab its own file; Reset Libraries (one row naming the installed table) and Migrate Libraries (kicad-cli's `sym upgrade` / `fp upgrade`, `ConvertLibrary`) with KiCad's questions; Edit Options... (`DIALOG_PLUGIN_OPTIONS`); Eagle, Altium, CADSTAR, EasyEDA, PADS Logic, gEDA, Sprint Layout and legacy libraries of the tables read through kicad-cli's conversion into a read-only cache; KiCad's path variables edited in Configure Paths and written to `kicad_common.json`, with the 3D model search paths listed (KiCad 8303b2ad has no page that edits them: they are the path variables). PARITY-common §14.
  - Left: pasting table text into the grid; a row's options are not passed to the conversion (kicad-cli has no flag: a CADSTAR library that needs `csa` converts without it); Database and HTTP libraries are listed, not read; a system-environment variable is read only in Configure Paths (KiCad lets one override it for the session); Add Library's "global table" choice and Save Library As' global entry are not offered.
  - M. GAPS 5; PARITY-common §14.
- **L3 [FM] Choosers.** **done (task-u4-libs)**: `/regex/` (and `^...$`) and relational (`voltage>3.3`, `resistance<10k`) search words in the server's search (on `eda_model::wx_regex`) and the browser's groups; the Symbol Chooser's footprint selector (`FOOTPRINT_SELECT_WIDGET`, the chosen one drawn and placed); the Footprint Chooser's "Show 3D viewer in own window"; Change Symbols' browse buttons open the Symbol Chooser over every library of the tables, and a new id's definition goes in with the change. PARITY-sch §3a.
  - Left: the browser's regex runs on JS RegExp, not `wx_regex`; the selector counts distinct pin numbers (KiCad: graphical pins) and lists the default footprint once; the picker's Recently Used is Place Symbol's.
  - M. GAPS 5/30; PARITY-sch §3a.
- **L4 [FM] Assign Footprints (CvPcb).** **done (task-u4-libs)**: CvPcb's footprint viewer as a pane of the dialog (the picked footprint, else the selected symbol's: `InitDisplay`, `GetFootprint`'s messages); the footprint association files (`.equ`, KiCad 25ec2335's cvpcb/): Manage Footprint Association Files... (`DIALOG_CONFIG_EQUFILES`, the list in the board folder's `cvpcb.json` and the derived `.kicad_pro`) and Automatically Assign Footprints (`AutomaticFootprintMatching`, staged with the dialog's assignments, one undo step). PARITY-sch §13.
  - Left: the viewer is a pane, not a window of its own; a relative `.equ` path is read against the board's folder (KiCad's search stack is not searched); Edit opens no text editor; the CvPcb hotkeys.
  - M. PARITY-sch §13.
- **L5 [BD] Design Blocks.** **done (task-u4-libs)**: the pane docks on the left or the right and floats at KiCad's 800 x 600 (`defaultDesignBlocksPaneInfo`: not top or bottom); a name with a '.' is accepted as KiCad 25ec2335 lists it (`DesignBlockEnumerate`: `BeforeLast( '.' )`); a sheet drawn from a block gets the block's default fields, hidden (`DrawSheet`), and a block saved from a sheet opens with the sheet's fields.
  - Left: the floating pane is a window inside the page; its dock or float, like the pinned libraries and check boxes, is kept in the browser (localStorage); a name starting with '.' is refused.
  - S. GAPS 32; PARITY-sch §34.
- **L6 [BD] Library edge cases.** **done (task-u4-libs)**:
  - Placing a lib_id the design already holds: decided and documented (PARITY-common §14). KiCad places the library's symbol as it is now and, when it differs from the schematic's cache, keeps it under a new cache entry (`Name_1`) so the placed ones keep theirs; the studio's IR has one definition per lib_id and places the design's copy, which is KiCad's answer until the library changes. Left: a placement's own cache entry when the library changed, and an unsaved Symbol Editor edit is placed as edited.
  - A folder library's broken file is reported in KiCad 25ec2335's words ("Unable to read file '%s'" ... "The library cannot be saved until these errors are fixed manually."): the chooser, the editors' tree, the status bar, `GET /api/library/load_errors` and the row's status in Manage Symbol Libraries; the good files are read. Left: a symbol KiCad's parser refuses inside a well-formed file is not found (the reader checks the header and the forms' closing).
  - S. GAPS 5; PARITY-common §14.

## 3D viewer

- **D1 [BD] The scene.** **done (task-u3-3d)**: every pad in its real shape (trapezoid, chamfered, custom, a turned one turned), the body less every through and non-plated hole (`GET /api/board3d`), vias as rings with barrels, inner copper drawn when the body does not hide it and in its own PCB editor colour, the pads' openings in the solder mask, no paste margin for a pad with no copper (V7-1), `render_3d_opengl`'s ambient, specular, shininess and transparency per layer (PARITY-3d §3, §9). Left:
  - A track's, a shape's or an untented via's own mask opening (the pads' openings are done).
  - Tracks and zones are not cut at the holes (a track stops where it ends in one).
  - A custom pad's paste margin, the off-board pads, plated vs bare copper, counterbores and backdrills.
  - The lights are three.js's: the shading is close to KiCad's, not the same.
- **D2 [FM] Controls.** **done (task-u3-3d)**: the right-click menu with its Rotate Board and Move Board submenus, the 3D Navigator, layer presets (Follow PCB Editor, Follow PCB Plot Settings, "legacy colors", saved ones), saved viewports, Preferences > 3D Viewer > General and Realtime Rendering, the PCB selection drawn in the selection colour and a click in 3D selecting the footprint (PARITY-3d §2, §5, §9). Left:
  - The presets' and viewports' Ctrl+Tab / Shift+Tab switchers.
  - The Render Options and While Moving preferences (not offered).
  - The viewports are not written into the derived `.kicad_pro`.
- **D3 [FM] Layer tree.** **done (task-u3-3d)**: the Values, Footprint Text (KiCad's meta-control) and Off-board Silkscreen rows (the silkscreen clipped to the board while it is off), and the 3D Navigator row. Left: the silkscreen is not clipped at the holes or the mask. PARITY-3d §5.
- **D4 [FM] Models.** **done (task-u3-3d)**: X3D, IGES and .wrz models through kicad-cli's conversion; the Extruded 3D Body read, written, drawn and set in the board's 3D Models tab; Migrate 3D Models when the board opens, with KiCad's notice; KiCad 5 aliases (`${KISYS3DMOD}`, `ALIAS:file`, a `3Dresolver.cfg` migrated into path variables); a bottom-side footprint's tab shows the file's numbers (PARITY-3d §6, GAPS 30). Left:
  - The body's Export..., the body in the tab's preview and in the Footprint Editor's dialog.
  - Migrate's folder picker over the whole disk.
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

- **C1 [BD] Mouse and touchpad.** **done (task-u4-common)**: every canvas reads Left button drag (KiCad's three choices), Middle and Right button drag (Pan, Zoom, None) and Pan on mouse movement with key; the library editors draw the selection rectangle and auto-pan; a touchpad pinch zooms as `onMagnify` does (PARITY-common §3, §17). Left:
  - The studio's Left button drag default is "Drag any object" (KiCad's is "Drag selected objects"): a decision for the lead.
  - No pointer warp ("Center and warp cursor on zoom", the infinite drag at the edge): a browser limit.
  - A pinch made with Control held reads as Ctrl and the wheel.
  - M. GAPS 20; PARITY-sch §27.
- **C2 [FM] Grid.** **done (task-u4-common)**: Dots, Lines and Small crosses with the grid thickness and minimum spacing on each editor's Display Options page, one port of `DrawGrid` for the four canvases; Edit Grids has KiCad's Grid Settings (name, X, Linked, Y), drawn and snapped to with X and Y; the schematic context menu has its Zoom and Grid submenus (PARITY-common §10, §17). Left:
  - "Snap to grid:" (Always / When grid shown / Never) is not offered.
  - An editor keeps its grid by its X size: of two grids with the same X the first is current.
  - S. PARITY-pcb §1; PARITY-common §10.
- **C3 [FM] Page Settings.** **done (task-u4-common)**: a port of the `.kicad_wks` reader and draw-item list draws KiCad's default sheet or a chosen one on the board and the sheet; Page Settings chooses, browses or uploads the file; the derived `.kicad_pro` names it and kicad-cli's plots get `--drawing-sheet`; the board's sheet is on by default with its zone references; a `User` paper is clamped and turned for portrait (PARITY-common §9). Left:
  - The board's sheet now shows from the start, and the placer starts boards at the page corner, so the frame crosses them: a decision for the lead.
  - Upload copies the file into the board's folder (KiCad never copies); the file choice is an undo step (in KiCad a project setting).
  - Embedding the sheet, the schematic's "export to other sheets" boxes, `@{..}` expressions, outline fonts.
  - M. PARITY-common §9; GAPS 18.
- **C4 [BD] Small differences.** **done (task-u4-common)**: Enter during a PCB route is `cursorClick` (fixes, End finishes); each editor keeps its own crosshair, Always Show Crosshairs and polar coordinates; the Measure tool's ruler is `RULER_ITEM` with its ticks and x, y, r and θ, and Interactive Offset's ruler its arrowhead (the Measure tool has none in 8303b2ad) (PARITY-common §2, §3, §17). Left:
  - About 310 handlers have never been compared with KiCad's code.
  - The rulers' drop shadows.
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
