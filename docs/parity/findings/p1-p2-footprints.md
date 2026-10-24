# Findings: P1 (footprint graphics) and P2 (footprint assignment, exchange, rename): `p1-p2-footprints`

Reviewer: `p1-p2-footprints`, 2026-10-23. Findings only: no code was changed. Our code was read from main's checkout (`be562f2e`). KiCad is `$KICAD_SRC_DIR` = `kicad-src-8303b2ad` unless a finding names `kicad-src-25ec2335`. Scratch work is in `/tmp/findings-p1-p2-footprints.px2p`.

## The lead: 76 schematic-parity differences on the routed `mcu_board_30plus`

**Reproduced exactly.** I ran `eda place`, then `eda route`, then `eda board new`, copied `design.json` over, and ran `eda board drc --schematic-parity`. The result is `net_conflict` 38, `footprint_symbol_mismatch` 30 and `footprint_symbol_field_mismatch` 8: 76 in all. The report is `/tmp/findings-p1-p2-footprints.px2p/drc.json`, and the derived files are in `/tmp/findings-p1-p2-footprints.px2p/b/.kicad/`.

**Every board made here has the problem, not just this one.** The same run on two other examples:
- `examples/opamp_filter.yaml`, a flat sheet: 15 net conflicts and 7 footprint mismatches.
- `examples/ldo.yaml`, a flat sheet: 10 net conflicts, 6 footprint mismatches and 2 field mismatches.

So the board and its derived schematic disagree on every project made here. KiCad's own Update PCB from Schematic (F8) would rewrite every net name and every footprint link of such a board.

The 76 have three independent causes: F1, F2 and F3. F4 makes it worse when KiCad itself runs F8.

KiCad's test, quoted from `pcbnew/drc/drc_test_provider_schematic_parity.cpp::testNetlist`:
```cpp
if( component->GetFPID().GetUniStringLibId() != footprint->GetFPID().GetUniStringLibId() ...
    msg.Printf( _( "%s doesn't match footprint given by symbol (%s)" ), ...
...
fpFieldsAsMap[field->GetName()] = field->GetText();   // every field but Reference/Value/Component Class
compFields.erase( Reference, Value, Footprint, "Component Class" );
for( const auto& [name, value] : compFields )
    if( it == fpFieldsAsMap.end() ) { mismatchDetail = wxString::Format( _( "Missing symbol field '%s' in footprint" ), name ); break; }
    if( it->second != value )       { mismatchDetail = wxString::Format( _( "Field '%s' differs (PCB: '%s', Schematic: '%s')" ), ...); break; }
...
else if( pcb_netname.IsEmpty() && !sch_net.GetNetName().IsEmpty() )
    msg.Printf( _( "Pad missing net given by schematic (%s)" ), sch_net.GetNetName() );
else if( pcb_netname != sch_net.GetNetName() && !( unconnected-/no-connect prefixes ) )
    msg.Printf( _( "Pad net (%s) doesn't match net given by schematic (%s)" ), pcb_netname, sch_net.GetNetName() );
```
The test stops at the first mismatched field (`break`), so each footprint reports at most one field mismatch.

---

### F1. The board's net names are the intent's; the derived schematic's KiCad net names are path-prefixed or default names. HIGH.

**Fixed** on task-fix-a-kicad-names: fc8ee220 (option (a), the user's decision: the tracer names as `recacheName` does, the board loader gives every net KiCad's name, `crates/cli/src/net_names.rs`; `crates/cli/tests/kicad_names.rs` finds 0 parity differences on `opamp_filter`, `ldo` and `mcu_board_30plus`; web/studio/PARITY-pcb.md section 53).

This causes 34 of the 38 net conflicts here. The other 4 come from F1b and F3.

**What differs.**
- **The board side.** `write_footprint` names a pad's net after the intent net that holds `REF.PIN`: `crates/kicad/src/pcb.rs:1224` (`pin_net`, which reads `model.nets[].name`). The nets this board writes are `PA0` … `PA7`, `LED1` … `LED8`, `RESET`, `SWD`, `GND` and `VDD`.
- **The schematic side.** The derived schematic draws:
  - each inter-sheet net as a `hierarchical_label` in the sub-sheets, with sheet pins on the root;
  - a sheet-local net with no label at all (the LED-to-resistor nets);
  - a flat design's nets as local labels.
- **How KiCad names those nets.** `eeschema/sch_connection.cpp::SCH_CONNECTION::recacheName` puts the sheet path in front of every driver except a global label and a global power pin:
  ```cpp
  case SCH_GLOBAL_LABEL_T: prepend_path = false; break;
  case SCH_PIN_T: prepend_path = pin->IsLocalPower(); break;
  ...
  m_cached_name_with_path = prepend_path ? m_sheet.PathHumanReadable( true, false, true ) << m_cached_name : m_cached_name;
  ```
  A net nothing drives takes `SCH_PIN::GetDefaultNetName`, e.g. `Net-(D1-Pad1)`. So kicad-cli's netlist names the nets as follows:

  | Net | Name in kicad-cli's netlist |
  |---|---|
  | `PA7` | `/LED channels (D1–D8)/PA7` |
  | `RESET` | `/Header (J1)/RESET` |
  | `LED1` | `Net-(D1-Pad1)` |
  | `IN` (opamp, a flat sheet) | `/IN` |
- **Our own tracer agrees with neither.** `crates/engine/src/nets.rs:523` adds the sheet path only when two nets would share a name:
  ```rust
  let candidate = if used.contains(&n) && f.local_name { format!("{}/{}", spath[f.screen], n) } else { n };
  ```
  For a net nothing drives, `nets.rs:541` (`wire_hints`) takes "the name its wires already carry". The `.kicad_sch` file does not hold those wire names, so KiCad never sees them. The studio therefore shows `LED1` and `PA7`, while every KiCad tool (netlist, ERC, F8, the parity DRC) sees `Net-(D1-Pad1)` and `/LED channels (D1–D8)/PA7`.
- **What KiCad's F8 would do.** `BOARD_NETLIST_UPDATER::updateComponentPadConnections` gives each pad `net.GetNetName()` from the netlist ("Reconnect %s pin %s from %s to %s."), so it would rename every net of our board. A net class or rule keyed by the old names would stop applying.

**How to see it.** The repro above: `U1.10 Pad net (PA7) doesn't match net given by schematic (/LED channels (D1–D8)/PA7)` and `D1.1 Pad net (LED1) doesn't match net given by schematic (Net-(D1-Pad1))`.

Test: `kicad_cli_parity_is_clean_on_a_board_made_from_an_example` (slow tier).
- Input: `eda board new examples/opamp_filter.yaml`, then `place`.
- Assertion: `drc_with(dir, false, true)["schematic_parity"]` holds no `net_conflict`.

A fast test for the tracer: `a_local_label_on_the_root_names_its_net_with_the_root_path`. One screen holding the label `IN`, traced with `wire_hints: false`; assert the name is `/IN`, as `recacheName` gives.

**Fix direction.** Choose one:
- (a) The board takes KiCad's names from the derived schematic, which is what F8 does: fix the tracer to `recacheName`'s rule and drop the wire hints for export.
- (b) The derived schematic names every intent net so that KiCad gets the intent's name: a global label on each named net.

Option (a) renames nets across the studio: rules, net classes and `design.nets`. Option (b) changes only the drawing.

**Unit-sensitive:** no.

### F1b. A no-connect pin's pad gets no net; KiCad gives it `unconnected-(...)`. MEDIUM.

**Fixed** on task-fix-a-kicad-names: fc8ee220 (`pcb::unconnected_pin_nets`: every pin of a footprint made here on no net gets `unconnected-(..)`; the `_%d` de-duplication of no-connect pads is not ported).

**What differs.**
- **KiCad.** `BOARD_NETLIST_UPDATER::updateComponentPadConnections` connects every pad the netlist names, single-pin nets included. A no-connect pad gets a unique `"%s_%d"` copy of the name: `if( pad->IsNoConnectPad() ) { ... netName = wxString::Format( wxS( "%s_%d" ), ...` (`board_netlist_updater.cpp:1151`).
- **Ours.** `pcb.rs:1224` writes no `(net ..)` for a pin that is in no intent net, so the pad is left with none.

**How to see it.** `U1.12 Pad missing net given by schematic (unconnected-(U1-NC1-Pad12))`, and the same for `U1.13`. On `ldo.yaml`: `J1.4 ... (unconnected-(J1-NC-Pad4))`.

Test: `a_no_connect_pin_is_written_on_its_unconnected_net`. Assert that `pad "12"` of `U1` contains `(net N "unconnected-(U1-NC1-Pad12)")` and `(pintype "no_connect")`.

**Unit-sensitive:** no.

### F2. The derived schematic's Footprint field is empty where the board writes `eda:<package>`. HIGH.

**Fixed** on task-fix-a-kicad-names: fc8ee220 (`eda_model::footprint::footprint_lib_id`, called by both writers; `pending_footprint_changes` and the Footprint-field follower accept either name, `names_footprint_of`).

This causes all 30 `footprint_symbol_mismatch` reports.

**What differs.**
- **The schematic writer.** `crates/kicad/src/lib.rs:397`:
  ```rust
  let footprint = if !sym.footprint.is_empty() { sym.footprint.as_str() } else { part.footprint.as_deref().unwrap_or("") };
  ```
  A part given by `package:` (every part of the examples) has `part.footprint == None`, so the symbol's Footprint is `""`.
- **The board writer.** `crates/kicad/src/pcb.rs:990`:
  ```rust
  let lib_name = part.footprint.clone().unwrap_or_else(|| footprint.name.clone());
  ... format!("eda:{lib_name}")
  ```
  For the same part this writes `(footprint "eda:0603")`.
- **A third spelling.** `crates/ops/src/fp_exchange.rs:128` `footprint_name_of` says `0603`, with no library. `pending_footprint_changes` (`:141`) compares a symbol's field with that bare name. So a user who types `eda:0603`, the name the file shows, gets a pending change that changes nothing.
- **What KiCad does with an empty field.** For a symbol with no footprint, `PCB_EDIT_FRAME::LoadFootprints` (`pcbnew/netlist.cpp:189`) reports the error "No footprint defined for symbol %s." The parity DRC reports `"%s doesn't match footprint given by symbol (%s)"`.
- **`eda:` is not a library.** No project `fp-lib-table` entry names it (`.kicad/` holds none), so KiCad cannot resolve `eda:0603` either.
- **A better name exists.** The 3D model and the art already come from the installed twin (`eda_model::footprint::kicad_footprint_of_part`, used by `fp_art_io.rs::attach_installed_art`), e.g. `LED_SMD:LED_0603_1608Metric`. That is the name KiCad could resolve.

**How to see it.** `D1: eda:0603 doesn't match footprint given by symbol ()` and `U1: eda:SOIC-16 doesn't match footprint given by symbol ()`.

Test: `the_derived_symbol_names_the_footprint_the_board_writes`.
- Input: a one-part intent with `package: "0603"`.
- Assertion: export both files; the `(footprint "X")` of the `.kicad_pcb` equals the `(property "Footprint" "X")` of the `.kicad_sch`, and `X` is not empty.

Why the slow tier never caught it: the P2 tests use a fixture whose parts name `Test:2PAD` explicitly, and they compare against a `baseline` taken first (`crates/cli/src/fp_assign_tests.rs:143, 256, 449`). Their doc comments claim "no footprint, value or net that differs", but the assertions only check equality with the baseline and the absence of the text "footprint given by symbol" after the edit. The baseline itself holds the noise.

**Fix direction.** One function gives a part's `Library:Name`, ideally the installed twin's. The board writer, the schematic writer and `footprint_name_of` all call it.

**Unit-sensitive:** no.

### F3. The symbol's fields are never copied onto the footprint: Sim.*, Datasheet, Description and user fields. MEDIUM.

**Fixed** on task-fix-a-kicad-names: fc8ee220 (`eda_kicad::symbol_links` gives the fields as the schematic writer writes them; hidden on F.Fab/B.Fab at the origin, as `updateFootprintParameters` adds them).

This causes the 8 `footprint_symbol_field_mismatch` reports.

**What differs.**
- **KiCad.** `BOARD_NETLIST_UPDATER::updateFootprintParameters` (`board_netlist_updater.cpp:510-620`) sets every netlist field except Reference, Value, Footprint and Component Class on the footprint, adding a hidden field on F.Fab or B.Fab for any the footprint lacks:
  ```cpp
  if( aFootprint->HasField( name ) ) aFootprint->GetField( name )->SetText( value );
  else { PCB_FIELD* newField = new PCB_FIELD( aFootprint, FIELD_T::USER ); ... newField->SetVisible( false ); newField->SetLayer( aFootprint->GetLayer() == F_Cu ? F_Fab : B_Fab ); ...
  ```
  The message is "Updated %s fields.".
- **Ours.** `crates/kicad/src/fp_fields.rs:92` `write_fields` writes only the Reference, the Value and the user fields of a `FootprintEdit`.
- **Where the extra fields come from.** The schematic writer adds the library's `Sim.*` fields (`lib.rs:416-421`, `model.symbol_sim_fields`) and the resolved Datasheet (`lib.rs:398-402`). The board has neither.

**How to see it.**
- mcu30: `D1 … D8: Missing symbol field 'Sim.Pins' in footprint`.
- ldo: `U1: Field 'Datasheet' differs (PCB: '', Schematic: 'http://www.advanced-monolithic.com/pdf/ds1117.pdf')` and `D1: Missing symbol field 'Sim.Device' in footprint`.

Test: `a_symbols_fields_are_on_its_footprint`.
- Input: an intent with a `Device:LED`.
- Assertion: the D1 footprint block of the exported board contains `(property "Sim.Pins" "1=K 2=A"` with `(hide yes)` on `F.Fab`.

**Unit-sensitive:** no.

### F4. The board writes no `(path ...)` link to the symbol, so KiCad's default F8 matches no footprint. HIGH.

**Fixed** on task-fix-a-kicad-names: fc8ee220 (`(path)`, `sheetname`, `sheetfile` from `symbol_links`, checked against kicad-cli's netlist `tstamps`; an imported footprint keeps the sheet its file named).

**What differs.**
- **Ours.** `write_footprint` never writes `(path "/<sheet uuid>/<symbol uuid>")`. `(sheetname)` and `(sheetfile)` are written only from an imported board's extras (`pcb.rs:1029-1034`). The test at `pcb.rs:3179` asserts that the path is absent, "the derived schematic's symbols have uuids of their own".
- **The uuids are deterministic.** The symbol's uuid is `duid_for("sym:<ref>")` (`lib.rs:372`), so the board could write the link.
- **An imported board loses its link too.** Its `(path ..)` is dropped on re-export (the same test).
- **KiCad matches by that path by default.** `BOARD_NETLIST_UPDATER::UpdateNetlist`:
  ```cpp
  if( m_lookupByTimestamp ) { ... if( footprint->GetPath() == base ) match = true; }
  else match = footprint->GetReference().CmpNoCase( component->GetReference() ) == 0;
  ```
  `DIALOG_UPDATE_PCB` sets `SetLookupByTimestamp( !m_cbRelinkFootprints->GetValue() )`, and "Re-link footprints to schematic symbols based on their reference designators" is unchecked by default (`dialog_update_pcb_base.cpp:27`).
- **What a user would see.** Opening our derived project in KiCad and pressing F8 with the defaults matches no footprint. KiCad adds a second footprint for every symbol (`addNewFootprint`) and leaves the placed ones. With "Delete footprints with no symbols" on, it deletes the placed and routed ones. KiCad also uses `sheetname`/`sheetfile` for sheet-based component classes and placement rule areas.

**How to see it.** `grep -c '(path ' /tmp/findings-p1-p2-footprints.px2p/b/.kicad/board.kicad_pcb` prints 0.

Test: `every_footprint_links_to_its_symbol`. Assert that each footprint's `(path ..)` equals `/<sheet path uuids>/<symbol uuid>`, as the derived `.kicad_sch` writes them under `(instances (project ..) (path ..))`, and that `(sheetname ..)` and `(sheetfile ..)` are present.

**Unit-sensitive:** no.

### F5. Pads of a board made here carry no `pinfunction` or `pintype`. MEDIUM.

**Fixed** on task-fix-a-kicad-names: fc8ee220 (the netlist's: `PA0_3` -- name and number, as kicad-cli 25ec2335 writes it, not `PA0` as this finding guessed -- and the electrical type).

**What differs.**
- **Ours.** `pcb.rs:1232-1237` writes them only from an imported board's `PadMaskInfo`.
- **KiCad.** `updateComponentPadConnections` sets `pad->SetPinFunction( net.GetPinFunction() ); pad->SetPinType( net.GetPinType() );` on every update.
- **Why it matters.** `PAD::IsNoConnectPad` (`pintype "no_connect"`) is what the parity test's no-connect exemption and the updater's `unconnected` de-duplication key on. Without it the F1b fix cannot be exact. The pad's pin name (shown on the canvas and in the 3D/pos outputs) is missing too.

**How to see it.** `grep -c pinfunction …/b/.kicad/board.kicad_pcb` prints 0.

Test: `pads_carry_their_pins_function_and_type`, on U1 pin 3: assert `(pinfunction "PA0") (pintype "bidirectional")` (or whatever the symbol pin type is).

**Unit-sensitive:** no.

---

## P2: footprint assignment, exchange and reference rename

### F6. A rename leaves `${REF:FIELD}` cross-references behind; KiCad stores them as KIIDs. MEDIUM.

**What differs.**
- **KiCad.** When a text is set, KiCad stores a cross-reference by KIID rather than by designator:
  - the board: `BOARD::ConvertCrossReferencesToKIIDs`, in `dialog_text_properties.cpp:505/515`, `dialog_textbox_properties.cpp:405`, `dialog_footprint_properties.cpp:584`, `dialog_table_properties.cpp:367` and `dialog_dimension_properties.cpp:384-389`;
  - the schematic: `SCHEMATIC::ConvertRefsToKIIDs`, in `dialog_text_properties.cpp:472`, `dialog_label_properties.cpp:499/658` and `fields_data_model.cpp:1345`.

  So after Edit Reference, Annotate or Geographical Reannotate, `${U1:VALUE}` still follows the part.
- **Ours.** Neither conversion exists in the crates (no match for `ConvertCrossReferencesToKIIDs` or `ConvertRefsToKIIDs`). `eda_model::text_vars::SchSheet::resolve_cross_reference` (`crates/model/src/text_vars.rs:633`) resolves by designator, and `Board::rename_part` (`crates/ops/src/part_rename.rs:31`) does not rewrite texts. A text that names `U1` names nothing, or another part, after `U1` becomes `U5`.

**How to see it.** Put a schematic text `${U1:Value}`, rename U1 to U5 (Edit Reference), and read the text: KiCad shows U5's value, ours shows `??` or unresolved. Test: `a_cross_reference_follows_a_rename`.

**Unit-sensitive:** no.

### F7. Update PCB from Schematic says nothing for a symbol with no footprint. LOW.

**What differs.**
- **Ours.** `pending_footprint_changes` skips such a symbol (`ops/src/fp_exchange.rs:141`: `if to.is_empty() … continue`).
- **KiCad.** `PCB_EDIT_FRAME::LoadFootprints` reports `"No footprint defined for symbol %s."` (`RPT_SEVERITY_ERROR`) for it.

With F2 in place, every symbol of a board made here would raise that error in KiCad.

**Unit-sensitive:** no.

### F8. Exchange text carry: effects copy knockout, and the position rule differs from KiCad's for a rotated footprint with a pad shift. LOW.

`model/src/fp_exchange.rs:221-241` `carry_layout` against `pcb_edit_frame.cpp::processTextItem`:
- **Effects.** With effects not reset, KiCad does `aDest.SetAttributes( aSrc )`. `EDA_TEXT_ATTRIBUTES` holds visibility but not knockout, which is `PCB_TEXT`'s own, so KiCad copies the old text's visibility even when "layers" is reset. Ours copies `knockout` (line 235) and does not copy `visible` under effects.
- **Positions.** With positions not reset, KiCad computes `GetRotated( aSrc.GetFPRelativePosition() - aPosShift, -aAngleShift )`, where `aPosShift` is already in the board frame (`GetRotated( translation, orientation )` in `ComputeFootprintShift`). On a footprint whose orientation is not 0 and with a non-zero shift, KiCad's text therefore does not stay exactly where it was. Ours keeps it exactly on the board (`set_from_board`). Ours is arguably better, but it is a deviation.

**Unit-sensitive:** no.

### F9 (note, not a defect). The pad alignment of a bottom-side footprint follows newer KiCad, not 8303b2ad.

`model/src/fp_exchange.rs:48` `uniquely_numbered_pads` mirrors both footprints by `side`.

- **8303b2ad.** `ComputeFootprintShift` compares the existing (flipped) footprint's `GetFPRelativePosition` with the not-yet-flipped library footprint. A symmetric bottom-side 0603 therefore realigns by 180°: pad 1 lands on pad 2.
- **`kicad-src-25ec2335`.** This is fixed there: `footprint_utils.cpp:95` `if( aExisting.IsFlipped() != aNew.IsFlipped() ) for(...) MIRROR( pos.y, 0 );`.

Ours matches the fixed behaviour. Keep it, and say so in the doc comment, which currently claims 8303b2ad.

**Unit-sensitive:** `EPSILON_UM = 0.01` (`fp_exchange.rs:25`) must become 10 nm, and the truncating mean (`trunc()`) changes granularity under nm.

---

## P1: footprint graphics

### F10. A rounded rectangle on a footprint at a cardinal turn becomes a polygon; KiCad keeps it a rectangle. LOW.

**What differs.**
- **Ours.** `crates/model/src/fp_art.rs:235` `FpShape::placed`: `if self.radius > 0 { Shape::Polygon … rounded_rect_points … }`, whatever the turn.
- **KiCad.** `common/eda_shape.cpp::EDA_SHAPE::rotate` keeps a `RECTANGLE` (with its corner radius) for `aAngle.IsCardinal()` and converts it to POLY only otherwise:
  ```cpp
  case SHAPE_T::RECTANGLE:
      if( aAngle.IsCardinal() ) { RotatePoint( m_start, … ); RotatePoint( m_end, … ); }
      else { ROUNDRECT rr( …, m_cornerRadius ); m_shape = SHAPE_T::POLY; rr.TransformToPolygon( GetPolyShape(), getMaxError() ); … }
  ```

The file writer works from the local shape (`fp_art_io.rs::write_shape`) and is unaffected. Only the canvas, the 3D view, the in-house checks and hit-testing see a tessellated outline instead of an exact arc corner.

**Unit-sensitive:** the tessellation error is `outline::MAX_ERROR_UM` (it scales).

### F11. A hidden `fp_text user` is written with `(hide yes)`; KiCad 8303b2ad never writes it for a non-field text. LOW.

**What differs.**
- **KiCad.** `pcb_io_kicad_sexpr.cpp::format( const PCB_TEXT* )`: `if( field && !field->IsVisible() ) KICAD_FORMAT::FormatBool( m_out, "hide", true );`, so only a field gets it. `EDA_TEXT::Format` writes no `hide` either.
- **Ours.** `fp_art_io.rs::write_text` writes ` (hide yes)` for an invisible text.

KiCad's parser reads it (`parsePCB_TEXT`, `case T_hide: if( parentFP ) aText->SetVisible( !hide )`), so nothing is lost, but the text is not what `format()` produces. Harmless; byte-diffs against KiCad will show it.

**Unit-sensitive:** no.

---

## Checked and found to match (no need to re-review)

- `ORTHO_ITEM_REALIGNER::GetTransform` (`common/item_realignment.cpp`) against `model/src/fp_exchange.rs::realign`: the baseline search, `Snap90Degrees`, the "more than half" fixed-pad rule and the mean fallback all match. `PadAlignment::apply` against `GetRotated( translation, orientation )`: the signs check out.
- `PCB_EDIT_FRAME::ExchangeFootprint`'s Value rule ("reset value text only when it is a proxy for the footprint ID") is ported (`ops/src/fp_exchange.rs:352`). The Reference string is never reset, as in KiCad.
- The `EDA_TEXT::Similarity` and `matchItemsBySimilarity` greedy matching for texts (`match_texts`) match.
- `DIALOG_BOARD_REANNOTATE` against `model/src/reannotate.rs`:
  - `BuildFootprintList`: the exclusion tokens and `*` patterns, the precedence of locked, selection, front and back, and the empty and invalid refdes;
  - `RoundToGrid`, the sign of the C remainder;
  - `FootprintCompare`, the swaps for Y-first and descending;
  - `GetOrBuildRefDesInfo` (`max(start-1,0)`), `BuildUnavailableRefsList`, `BuildChangeArray` (adding and removing a prefix, restarting at the start number) and the "Duplicate instances of %s" scan.

  Two deviations, both deliberate and harmless: ours pre-sorts by reference for a stable order, and it also reserves the numbers of parts not on the board (`elsewhere`).
- `PCB_TEXT::GetDrawRotation` (keep upright folding into `]-90, 90]`) against `fp_art.rs::draw_rotation`: they match.
- `format( const PCB_TEXT* )`'s position and angle frame (FP-relative position with the orientation removed, absolute angle) against `FieldLayout::file_position` and `file_angle_deg`: they match, including a bottom-side footprint (x mirrored, the angle's sense flipped). `(unlocked yes)` is written exactly when the text is not kept upright, as KiCad does.
- KiCad 10's `(transform ...)`: in `kicad-src-25ec2335`, `parseFOOTPRINT_unchecked` `case T_transform` and `parsePCB_TEXT` (lib-frame position for files of version 20260616 and later, the angle left absolute) agree with `crates/kicad/src/fp_transform.rs`'s module doc and normalization. A scale other than 1:1 is reported in `ImportNotes::footprints_scaled`.
- The parity provider's field comparison: an empty Datasheet or Description raises no mismatch, because `FOOTPRINT` always has those mandatory fields. Only non-empty symbol fields (F3) do.

Not reviewed, for lack of time: the canvas painter (`fpArtPaint.ts`) against `pcb_painter.cpp::draw( FOOTPRINT )`, the 3D silkscreen, `FootprintNeedsUpdate`/`fp_compare`, the Compare Footprint dialog, and the Assign Footprints (cvpcb) dialog's own behaviour.

## Summary

| # | Finding | Severity | Unit-sensitive |
|---|---|---|---|
| F1 | Board net names are the intent's; KiCad's netlist names the derived schematic's nets with path prefixes or `Net-(…)`, and our tracer agrees with neither (34 of the 38 net conflicts, every board made here) | high | no |
| F1b | No-connect pins get no `unconnected-(…)` net on the pad | medium | no |
| F2 | Derived symbol Footprint `""` vs board `eda:<pkg>` vs `footprint_name_of` `<pkg>`: three spellings (30 mismatches); `eda:` is no library | high | no |
| F3 | Symbol fields (Sim.*, Datasheet, …) are not copied to the footprint as `updateFootprintParameters` does (8 mismatches) | medium | no |
| F4 | No `(path)`, `sheetname` or `sheetfile` on a board made here; KiCad's default F8 matches no footprint and adds a second copy of every footprint | high | no |
| F5 | No `pinfunction` or `pintype` on pads of a board made here | medium | no |
| F6 | `${REF:FIELD}` cross-references are kept by designator, not KIID, so a rename leaves them behind | medium | no |
| F7 | No "No footprint defined for symbol %s." for an empty Footprint field | low | no |
| F8 | Exchange text carry: knockout and visibility, and the position on a rotated footprint with a shift | low | no |
| F9 | Bottom-side pad alignment follows newer KiCad (note) | note | yes (epsilon) |
| F10 | A rounded rectangle at a cardinal turn becomes a polygon in `placed` | low | yes (tessellation) |
| F11 | `(hide yes)` written on a non-field `fp_text user` | low | no |

Counts: high 3, medium 4, low 4, one note.
