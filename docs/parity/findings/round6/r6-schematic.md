# Round 6 findings: the schematic editor and its files (`r6-schematic`, 2026-10-28)

**Scope.** GAPS items 1, 4, 6, 11 (ERC half), 12 and 20: `crates/engine` (the tracer), `crates/kicad`'s `.kicad_sch` reader and writer, `crates/ops` `sch_*`, the studio's schematic code, and `web/studio/PARITY-sch.md`. Main is at `7d772e6e`, read only. No code was changed anywhere.

**Priority.**
- (a) Round 5's merges (PARITY-sch §48): 3783b91e (pin alternates, one number in two units, entries end to end, De Morgan), 088a5836 (`symbol_instances`, root instances), 43ce360f (directive "Net Class"), ffc896e0 (wire finish on a label / sheet pin), 46e9451f (junctions last), 32ef2aa2 (stacked pin net), beaba393, 1113c6d2 / 674109ac (Find and Replace), ef49e9a7 (Fields Table), 4944d606 (Schematic Control). Slice A. (The box clock is 2026-10-27, so `git log --since=2026-10-28` lists nothing; the round-5 commits were taken by name.)
- (b) The 130 QA roots through `eda import-kicad`, judged by kicad-cli, and the 7 still differing grouped by cause. Slice B.
- (c) Rows no round had reviewed: the bus tools, the Net Navigator and hierarchy, the Symbol Editor's schematic side, the ERC dialog entry points. Slice C.

**KiCad sources.** Connectivity, naming and ERC against 25ec2335 (the box's kicad-cli); files and tools against 8303b2ad (`$KICAD_SRC_DIR`).

**Binary.** Main `7d772e6e` built from a `git archive` (own `CARGO_TARGET_DIR`, deleted after), copied to `/tmp/findings-r6-schematic.UqWz/bin/eda`.

**Scratch.** `/tmp/findings-r6-schematic.UqWz/`:
- `bin/qa1.sh`, `bin/runset.sh` (round 5's harness, plus a net-name comparison into `names.txt`), `bin/qa/<root>/`, `sweep.txt`;
- `alt1/` (A6-M2's case), `ra/` (A6-M1's two-sheet project; studio run on port 8840, stopped).

**Already known?** Every finding was checked against `docs/parity/GAPS.md`, `docs/parity/findings/` (rounds 2-5, p11-p18, p12) and PARITY-sch §40-§48.

**Read first.**
- The sweep holds: **123 of 130** alike in pin groups and ERC counts. The 7 that differ have the three known causes (slice B table). But comparing net *names* too finds one new cause: KiCad 25ec2335 names a unit by its library **unit display name** (`(unit_name ..)`). Our schematic cache drops it, and the tracer uses the letter (B6-H1, QA `issue21980`, 40 nets named otherwise).
- Replace All in References is refused for a multi-unit symbol whose units are on two sheets (A6-M1, run on main).
- A placed pin's alternate does not take part in KiCad's "duplicate pin name" test, so the names of two nets differ (A6-M2, run against kicad-cli).

## Slice A: round 5's merges

### A6-M1. Replace All in References: a multi-unit symbol with units on two sheets is refused as a whole, "a symbol with this reference is already on another sheet" (medium) **Fixed** on task-r6-schematic: ae5feeec
### A6-M1. Replace All in References: a multi-unit symbol with units on two sheets is refused as a whole, "a symbol with this reference is already on another sheet" (medium)

**What differs.**
- Ours: `crates/ops/src/lib.rs:5763` (`apply_reference_renames`, from 674109ac, r5 C5-M2) renames each hit through a temporary name in two passes. Each pass goes through `rename_symbol_in_schematic` (`lib.rs:5843`), which still refuses a new id that a symbol on another sheet holds (`lib.rs:5851`, `symbols_elsewhere`). With Replace All over every sheet, U1 unit A (root) and U1 unit B (sheet `a`) are two renames, `(None, U1, U2)` and `(Some(a), U1, U2)`:
  - pass 1 renames root U1 to `\u{1}replace0` and sheet a's U1 to `\u{1}replace1`;
  - pass 2 renames root's to `U2`, then sheet a's `\u{1}replace1 -> U2` is refused, because root already holds `U2`.
  
  `Cmd` is atomic, so nothing is replaced.
- Increment Annotations (4944d606) meets the same case through `rename_symbol_here_unchecked` (`lib.rs:5863`: "a multi-unit symbol's units on two sheets move together"). Replace All does not use it.
- KiCad 8303b2ad, `sch_field.cpp::SCH_FIELD::Replace`:
  ```cpp
  if( m_id == FIELD_T::REFERENCE && m_parent && m_parent->Type() == SCH_SYMBOL_T )
  {
      SCH_SYMBOL* parentSymbol = static_cast<SCH_SYMBOL*>( m_parent );
      if( !replaceReferences )
          return false;
      wxCHECK_MSG( aAuxData, false, wxT( "Need sheetpath to replace in refdes." ) );
      text = parentSymbol->GetRef( (SCH_SHEET_PATH*) aAuxData );
      isReplaced = EDA_ITEM::Replace( aSearchData, text );
  ```
  followed by `parentSymbol->SetRef( sheet, text )`, with no test against other symbols. Both units become U2.

**How to see it (run on main).** `/tmp/findings-r6-schematic.UqWz/ra/src`: `t:DUAL` (two units), U1 unit 1 on the root and U1 unit 2 on sheet `a`. Import it, serve it, and POST `{"op":"replace_text","search":{"find":"U1","replace":"U2","replace_references":true},"items":null,"all_sheets":true}`. The reply is `{"message":"a symbol with this reference is already on another sheet","ok":false}`.

Test: `sch_control_tests::replace_all_in_references_moves_a_multi_unit_symbol_on_two_sheets`.
- Input: `two_sheet_design()` (already there: U1 unit 1 on the root, U1 unit 2 on `a.kicad_sch`).
- Apply: `ReplaceText { search: { find: "U1", replace: "U2", replace_references: true }, items: None, all_sheets: true }`.
- Assert `Ok`, with `U2` on both sheets.

Fix direction: check the end state once, as the function's doc already says ("only the end state counts"), and rename with `rename_symbol_here_unchecked`.

**Severity.** Medium: the command fails on a common hierarchical case, though connectivity is not affected.

**Already known?** No. C5-M2's fix covers chains on one sheet.

### A6-M2. A placed pin's alternate is left out of the "duplicate pin name" test: two pins that share a base name but chose different alternates are named `Net-(J1-IO-Pad3)` / `-Pad7` where KiCad names them `Net-(J1-IO)` / `Net-(J1-IO)_1` (medium) **Fixed** on task-r6-schematic: 0b63e63d (the duplicate-name rule; which of two equal names takes `_1` follows KiCad's island anchor KIID, not kept: Open, a named Differs)
### A6-M2. A placed pin's alternate is left out of the "duplicate pin name" test: two pins that share a base name but chose different alternates are named `Net-(J1-IO-Pad3)` / `-Pad7` where KiCad names them `Net-(J1-IO)` / `Net-(J1-IO)_1` (medium)

**What differs.**
- KiCad 25ec2335, `sch_pin.cpp::SCH_PIN::GetDefaultNetName`. The name part is the **library** pin's name, but the duplicate test compares the **placed** pins' shown names. `SCH_PIN::GetShownName` returns `m_alt` when it is set (`sch_pin.cpp`, `if( !m_alt.IsEmpty() ) return m_alt;`):
  ```cpp
  fact.name = m_libPin ? m_libPin->GetShownName() : wxString( "??" );
  ...
  for( const SCH_PIN* pin : symbol->GetPins( &aPath ) )
  {
      if( pin->GetShownName() == GetShownName()
              && pin->GetShownNumber() != GetShownNumber()
              && pin->GetType() != ELECTRICAL_PINTYPE::PT_NC )
      {
          fact.hasDuplicateName = true;
  ```
- Ours: `crates/engine/src/nets.rs:880` (`shown_name_counts`) counts duplicates on `facts.shown(pin_ref)`, the library pin names that `crates/cli/src/board.rs:559` fills (`pin_names.insert(key, p.name)`). The alternate (`SchExtras::pin_alternates`, 3783b91e) is read only for the pin's type, never for this test.
- So:
  - two pins with the same base name that chose different alternates are "duplicates" in ours, giving `-PadN`, but not in KiCad;
  - two pins with different base names that chose alternates of the same name are duplicates in KiCad, but not in ours.

**How to see it (run).** `/tmp/findings-r6-schematic.UqWz/alt1/bugtest.kicad_sch` is QA `issue22286/bugtest` with library pins 3 and 7 renamed `IO`. The instance picks `3.out` and `7.in`.
- kicad-cli 25ec2335: `Net-(J1-IO)_1 [J1.3, R1.1]` and `Net-(J1-IO) [J1.7, R1.2]`.
- Ours (`design.json`): `Net-(J1-IO-Pad3)` and `Net-(J1-IO-Pad7)`.

Test: `sch_hierarchy_tests::a_pins_alternate_is_its_shown_name_for_the_duplicate_name_test`.
- Input: the alt1 file.
- Assert that the pin sets carry kicad-cli's names.
- Fix direction: put the placed pin's shown name (its alternate, if any) in a second map, used only for the duplicate count, and keep `pin_names` for the name part.

**Severity.** Medium: an edge case of net naming (the KiCad-names decision). Connectivity is the same.

**Already known?** No. §48's row lists the NC rule, `unconnected-`, power input and pin kind.

### A6-L1. Delete of a wire, symbol, label or bus entry: KiCad runs `DeleteJunction` on a junction at the deleted item's points that is no longer explicit, merging the in-line wires whatever their strokes; ours only cleans up (low; B5-L1's fix covers sheet pins only) **Fixed** on task-r6-schematic: 327bb669
### A6-L1. Delete of a wire, symbol, label or bus entry: KiCad runs `DeleteJunction` on a junction at the deleted item's points that is no longer explicit, merging the in-line wires whatever their strokes; ours only cleans up (low; B5-L1's fix covers sheet pins only)

**What differs.**
- KiCad 8303b2ad, `tools/sch_edit_tool.cpp::SCH_EDIT_TOOL::DoDelete` collects the connection points of **every** connectable deleted item:
  ```cpp
  if( sch_item->IsConnectable() )
  {
      std::vector<VECTOR2I> tmp_pts = sch_item->GetConnectionPoints();
      pts.insert( pts.end(), tmp_pts.begin(), tmp_pts.end() );
  }
  ...
  for( const VECTOR2I& point : pts )
  {
      SCH_ITEM* junction = screen->GetItem( point, 0, SCH_JUNCTION_T );
      if( !junction )
          continue;
      if( junction->HasFlag( STRUCT_DELETED ) || !screen->IsExplicitJunction( point ) )
          m_frame->DeleteJunction( &commit, junction );
  }
  ```
  `DeleteJunction` merges the in-line lines that end there with `MergeOverlap( screen, firstLine, false )`, with no stroke test (r4 C4-L2, r5 B5-L1).
- Ours: since 46e9451f, only `DeleteSheet` and `DeleteSheetPin` call `delete_junction_at(at, false)` (`crates/ops/src/sch_edit.rs:395`, `:408`). `delete_wire` (`crates/ops/src/lib.rs:5142`), `delete_symbol`, `delete_label` and `delete_bus_entry` leave the dot to `commit_clean_up`. That drops the junction, but its merge keeps wires of different strokes apart (`sch_scene.rs:779`, `!self.stroke_equivalent(i1, i2)`).

**How to see it.** A T: two in-line halves, one dashed and one default, a stem, and a junction. Delete the stem.
- KiCad leaves one through wire.
- Ours leaves two.

Test: `tests::deleting_a_ts_stem_merges_the_halves_whatever_their_strokes`.
- Input: a T whose halves have different `extras.strokes`.
- Apply `delete_wire(stem)`.
- Assert one wire and no junction.

The same holds for a symbol whose pin was the third branch.

**Severity.** Low: cosmetic, connectivity is the same.

**Already known?** Partly. B5-L1 named sheet pins only. This is the general case of the same `DoDelete` loop.

### A6-L2. The alternate of a pin on a shared screen's later path is not seen by the `unconnected-` naming: `pin_alternates_by_pin` keys by the symbol's id, `nc_pins` looks up the part's reference (low) **Fixed** on task-r6-schematic: 0b63e63d
### A6-L2. The alternate of a pin on a shared screen's later path is not seen by the `unconnected-` naming: `pin_alternates_by_pin` keys by the symbol's id, `nc_pins` looks up the part's reference (low)

**What differs.**
- `crates/cli/src/board.rs:497` (`pin_alternates_by_pin`) builds `"{sym.id}.{number}"`.
- `board.rs:571` (`nc_pins`) looks up `"{part.reference}.{number}"`. For a screen shown twice, the second path's part is `R2`, not the screen's `R1`, so the lookup misses. The pin falls back to the library type (`no_connect`) and is named as a no-connect.
- The tracer's contacts read the alternate per placed symbol (`sch.extras.pin_alternate(..)`, `board.rs:636`, `:660`), so connectivity is right. Only the name differs.
- KiCad: `fact.noConnect = GetType() == PT_NC`, per `SCH_PIN`, and `m_alt` belongs to the symbol, so it is the same on every path.

**How to see it (code).** QA `issue22286` placed in a sheet that two sheet symbols show. The second path's J1 pin 3 (a `no_connect` library pin, alternate `3.out`) gets `unconnected-(..)`-style naming, or `Net-(..)` with the `-Pad` form, where KiCad gives `Net-(J?-Pad3)`.

Test: as `round_5s_connectivity_cases_are_traced_as_kicad_cli_traces_them`, with the `a22286` fixture under a sheet placed twice.

**Severity.** Low. **Already known?** No.

### A6-L3. A symbol instance's `(value "~")` / `(footprint "~")` is read as empty in any file version; KiCad does that only below 20250318 (low) **Fixed** on task-r6-schematic: f1ccd18c
### A6-L3. A symbol instance's `(value "~")` / `(footprint "~")` is read as empty in any file version; KiCad does that only below 20250318 (low)

**What differs.**
- `crates/kicad/src/sch_import.rs:1420` (`own_instance_fields`, 088a5836): `if t == "~" { String::new() }`.
- KiCad 8303b2ad, `sch_io_kicad_sexpr_parser.cpp::parseSchematicSymbol`, `T_instances`:
  ```cpp
  if( m_requiredVersion < 20250318 && CurStr() == "~" )
      symbol->SetValueFieldText( wxEmptyString );
  else
      symbol->SetValueFieldText( FromUTF8() );
  ```
  The `footprint` case is the same.

**How to see it.** A 20250318+ file with a symbol whose instance has `(value "~")` (a literal tilde, which KiCad 9+ writes as is): KiCad's value is `~`, ours is empty.

Test: `sch_import::tests::a_tilde_value_is_literal_in_a_new_file`.

**Severity.** Low. **Already known?** No.

### Checked and found to match (slice A)

- **3783b91e.**
  - `LibSymbol::placed_pin_type` against `SCH_PIN::SetAlt`: an empty alternate, the pin's own base name, or one the library pin lacks is ignored. `GetType` returns the alternate's `m_Type`.
  - The writer's `(pin "n" (uuid ..) (alternate ..))` against `SCH_IO_KICAD_SEXPR::saveSymbol`: left out when empty or the base name.
  - The cache's `(alternate "name" type shape)` lines.
  - `IsGlobalPower` / `IsLocalPower` through `GetType()`, with a hidden pin.
  - The `PT_NC` contact rule per placed symbol.
  - Alternates move with Annotate, Edit Reference, Delete and Paste.
  - `UpdatePins` keeps an alternate by number, as ours (the map is kept, and an alternate the new pin lacks is ignored by `placed_pin_type`).
  - `number_in_units` and `pin_key`: a contact per placed unit (`fact.owner`).
  - `PROPAGATES[ENTRY][ENTRY]` false: the entry joins only seeded points or wire interiors, and only on the wire pass.
  - `in_style` keeps `connectivity`.
- **088a5836.**
  - `UpdateSymbolInstanceData` gated `< 20221002`, the root paths included.
  - Value and footprint only when non-empty; `~` empty for such old files.
  - `GetRef( sheet )` takes the `/<root uuid>` instance first.
  - A path's `value` / `footprint` of any project sets the field (`T_instances`). A6-L3 is the exception.
- **43ce360f:** `GetCanonicalName` maps "Net Class" to "Netclass" for every `labelTypes` member, the directive label included. The QA directive labels carry only `Netclass` and `Component Class` fields (12 files surveyed), so the two-field model loses nothing there.
- **ffc896e0:** `IsTerminalPoint`, case by case.
  - Wire: bus entry (`GetItem( .., 1, SCH_BUS_WIRE_ENTRY_T )`), junction, pin end (`GetPin( .., true )`), wire, label anchor, sheet pin anchor.
  - Bus: bus, sheet pin, label.
  - Notes: a line.
  - A directive label never finishes a wire: `SCH_DIRECTIVE_LABEL` does not override `doIsConnected`, which is `false` in `SCH_ITEM`. Ours keeps directive labels out of `sch.labels`.
- **46e9451f:**
  - a junction in a batch goes after the other items, as `STRUCT_DELETED` "clean up junctions at the end";
  - a sheet's and a sheet pin's points go through `DeleteJunction` only when no longer explicit;
  - a failed merge rolls the batch back.
- **32ef2aa2:** the stacked pin takes the smallest pad's net (`GetSmallestStackedPadNumber`).
- **1113c6d2:**
  - selection-only Find walks the selection and keeps it (`if( !selectedOnly ) { ClearSelection(); AddItemToSel( item ); }`);
  - the 4 s wrap (`m_wrapAroundTimer.StartOnce( 4000 )`);
  - the end messages word for word.
- **ef49e9a7:** a row per later path of a shared screen (`AppendSymbol` per path); `GetRefNumber` keeps `01`.
- **4944d606:** Increment Annotations "All sheets" moves a multi-unit symbol on two sheets together (`rename_symbol_here_unchecked`). This is the contrast with A6-M1.

## Slice B: the 130 QA roots, rerun on main 7d772e6e

**Harness.** Round 5's `qa1.sh` (copy without `.kicad_pro` / `.kicad_prl`, kicad-cli netlist and ERC of the original, `eda import-kicad`, kicad-cli netlist and ERC of `out/.kicad/board.kicad_sch`, pin groups and ERC type counts compared, `lib_symbol_issues` / `footprint_link_issues` left out), with one addition: the nets' names are compared too (`names.txt`). Root list: `/tmp/r5s-sweep/all.txt` (130, the ratchet's rule).

**Result: 123 of 130 alike**, the same as round 5's after-fix count. The 7 that differ, by cause:

| Cause | QA roots | Status |
|---|---|---|
| Unannotated or duplicate references. A design names a part by its reference, so a second `U2` or an `R?` is renamed `U?1`, `P?1`, and so on. Connectivity is the same; the pin groups compare by reference, so they show as differing. | `netlists/complex_hierarchy_shared/ampli_ht` (`P?` -> `P?1`), `netlists/issue16915` (`J?`/`U?` -> `J?1`/`U?1`), `netlists/test_multiunit_reannotate_4` (`U?` -> `U?1`, so `missing_unit` and the others counted per renamed symbol), `netlists/test_multiunit_reannotate_5` (two units each of `U2` with three units: KiCad keeps the duplicate `U2`, so its `different_unit_net` 8 is lost) | known: PARITY-sch §40 ("units and unannotated `R?`", "the multi-unit reannotation projects"), §42 `unique_references`; the model's limit |
| A `lib_name` cache copy that differs from the `lib_id` copy (one library symbol per id) | `netlists/legacy_power` (`VCC` / `+3.3V` swapped on U1001.1), `same_local_global_power` (the 2 `same_local_global_power` errors lost) | known: D5-M1, the §48 Differs |
| The library symbol's own user fields are not written in the cache, so its body text `${LOCATION}` is unresolved in the derived file | `issue22286/bugtest` (`unresolved_variable` 2; the nets now match, after D5-H3) | known: §48 "Left". **Fixed** on task-r6-schematic: ddc91f1a |
| The library symbol's own user fields are not written in the cache, so its body text `${LOCATION}` is unresolved in the derived file | `issue22286/bugtest` (`unresolved_variable` 2; the nets now match, after D5-H3) | known: §48 "Left" |

**Net names (new comparison).** Of the 123, two are alike in pin groups but not in names:
- `netlists/complex_hierarchy_shared/complex_hierarchy` (68 lines): `Net-(C?-a501e6b4-Pad1)` against ours `Net-(C?-9f14b212-Pad1)`. This is the unannotated name's uuid hash. Known: r2 A-N13 / B-L3, PARITY §46 "Left".
- `issue21980` (40 lines): a new cause, B6-H1.

### B6-H1. A unit's display name (`(unit_name "..")`) is dropped from the schematic's library cache, and the tracer names units by letter: KiCad 25ec2335 names `A1TopLeft_UnderSide`, ours `A1A` (high: a file KiCad reads differently) **Fixed** on task-r6-schematic: 9d05af7a
### B6-H1. A unit's display name (`(unit_name "..")`) is dropped from the schematic's library cache, and the tracer names units by letter: KiCad 25ec2335 names `A1TopLeft_UnderSide`, ours `A1A` (high: a file KiCad reads differently)

**What differs.**
- KiCad 25ec2335, `sch_symbol.cpp::SCH_SYMBOL::SubReference`. A custom unit name replaces the letter in `GetRef( path, true )`, and so in every net name `GetDefaultNetName` builds (`reference{ symbol->GetRef( &aPath, false ), symbol->GetRef( &aPath, true ), .. }`), and in the reference shown on the sheet:
  ```cpp
  // A custom unit display name overrides the default A/B/C (or numeric) suffix.
  if( m_part )
  {
      const std::map<int, wxString>& names = m_part->GetUnitDisplayNames();
      auto                           it = names.find( aUnit );
      if( it != names.end() )
      {
          wxString subRef;
          ...
          subRef << it->second;
          return subRef;
  ```
- KiCad writes the name in the schematic's cache as in a library. 8303b2ad `sch_io_kicad_sexpr_lib_cache.cpp::SaveSymbol`:
  ```cpp
  // if the unit has a display name, write that
  if( aSymbol->GetUnitDisplayNames().contains( unit.m_unit ) )
  {
      name = aSymbol->GetUnitDisplayNames().at( unit.m_unit );
      aFormatter.Print( "(unit_name %s)", aFormatter.Quotes( name ).c_str() );
  }
  ```
- Ours:
  - The reader keeps the names (`crates/kicad/src/symbol_import.rs:293-297`, into `kicad.unit_names`), and the `.kicad_sym` writer writes them (`symbol_lib.rs:649-650`).
  - The schematic's cache writer does not: `crates/kicad/src/lib.rs:1460` (`write_body_style_blocks`) writes `(symbol "{bare}_{u}_{style}"` with no `(unit_name ..)`. `grep -c unit_name` on the derived `board.kicad_sch` gives 0, against 9 names in the original.
  - The tracer names units by letter (`crates/cli/src/board.rs:561`, `eda_model::annotate::unit_letters(unit)`), so `design.json` has `A1A` too.

**How to see it (run).** QA `issue21980/issue21980.kicad_sch` (version 20250922), `A1` with units `TopLeft_UnderSide`, ..., `Eqep2`.
- kicad-cli on the original: `unconnected-(A1TopLeft_UnderSide-+3.3V-PadJ1.1)`, and 19 more like it.
- Ours (`design.json`) and kicad-cli on the derived file: `unconnected-(A1A-+3.3V-PadJ1.1)`.

Pin groups and ERC counts are alike, which is why the round-5 sweep counted it as the same.

Tests:
- `crates/kicad` writer: `a_units_display_name_is_written_in_the_cache`. Import issue21980 and assert the derived text contains `(unit_name "TopLeft_UnderSide")`.
- `sch_hierarchy_tests` (slow): `a_units_display_name_names_its_nets`. Assert `design.nets` has `unconnected-(A1TopLeft_UnderSide-+3.3V-PadJ1.1)`.
- Also check the studio's shown reference (`A1TopLeft_UnderSide`, with `m_SubpartIdSeparator`).

**Severity.** High per the brief: the derived file loses the unit names (data loss), so kicad-cli and KiCad show other references and net names. Connectivity is the same.

**Already known?** No. GAPS line 683 says only that Symbol Properties has no unit display names grid ("kept in the file", the library file). Neither PARITY-sch nor the findings mention `unit_name` in the schematic.

## Slice C: rows no round had reviewed (bus tools, Net Navigator and hierarchy, the Symbol Editor's schematic side, the ERC dialog)

**Scope.**
- Bus tools: PARITY-sch §11 `C` Unfold from Bus (`kicad-port/schBusUnfold.ts`, `components/BusUnfoldDialog.tsx`, `components/schematic/schHit.ts::hitBus`, `SchematicView.tsx::commitDrawn`, `crates/cli/src/studio.rs` wire `members`, `crates/kicad/src/bus.rs`).
- Navigation and hierarchy:
  - §13 Next / Previous / Enter / Leave Sheet and Back / Forward (`kicad-port/navHistory.ts`, `actions/useActionRunner.ts`);
  - §43 the Hierarchy Navigator (`components/panels/HierarchyPanel.tsx`, `kicad-port/sheetPages.ts`);
  - §11 and §13 the Net Navigator and Tab / Shift+Tab (`kicad-port/netNavigator.ts`, `components/schControl/NetNavigatorPanel.tsx`).
- The Symbol Editor's schematic side: §13 Compare Symbol with Library and the Symbol Checker (`kicad-port/symbolDiff.ts`, `kicad-port/symbolChecker.ts`, `components/schControl/SymbolCheckerDialog.tsx`), and Edit with Symbol Editor (`useActionRunner.ts:1459-1476`).
- ERC dialog rows (§4, §28): r2 and r3 read the menu, sort and exclusions; only the marker-navigation entry points were read again.

**KiCad 8303b2ad.**
- `tools/sch_line_wire_bus_tool.cpp` (`BUS_UNFOLD_MENU`, `UnfoldBus`, `doUnfoldBus`, `getBusForUnfolding`, `doDrawSegments`);
- `net_navigator.cpp`, `widgets/hierarchy_pane.cpp`, `tools/sch_navigate_tool.cpp`;
- `sch_edit_frame.cpp::UpdateHierarchyNavigator` / `SaveSymbolToSchematic`, `tools/sch_editor_control.cpp::EditWithSymbolEditor`;
- `tools/sch_inspection_tool.cpp` (`CheckSymbol`, `DiffSymbol`, `NextMarker`, `PrevMarker`, `ExcludeMarker`), `symbol_checker.cpp`;
- `lib_symbol.cpp::Compare`, `sch_item.cpp::compare`, `sch_shape.cpp::compare`, `common/eda_shape.cpp::Compare`.

**Runs.** Code only. One fact is from round 5's import output: `/tmp/findings-r5-schematic.V0fy/qa/issue18346_kicad_sch/out/design.json` has the label `{"net": "BAR"}` and the wires `{"net": "/BAR"}`.

### C6-M1. Net Navigator: a local or hierarchical label is listed under its own text, not under the net the wires are on, so the net is split in two and Tab / Shift+Tab skips the label (medium) **Fixed** on task-r6-schematic: d53c20b8
### C6-M1. Net Navigator: a local or hierarchical label is listed under its own text, not under the net the wires are on, so the net is split in two and Tab / Shift+Tab skips the label (medium)

**What differs.**
- Since the KiCad net-names decision (batch A), a wire carries the net's KiCad name (`"/BAR"`), but a label's `net` is still its text (`"BAR"`). The traced name is a separate field, `net_name` (`crates/cli/src/studio.rs:2002`: `json!({ "id": l.id, "net": l.net, "net_name": traced, ... })`).
- The navigator keys on the label's `net`:
  - `components/schControl/NetNavigatorPanel.tsx:25-31` (`sheetNets`) adds `l.net` and `w.net` as separate names, so the tree gets both `/BAR` and `BAR`;
  - `kicad-port/netNavigator.ts:71-72` puts a label under a net only when `l.net === net`, so the label is never under `/BAR`;
  - Tab / Shift+Tab (`useActionRunner.ts:2285-2296`) builds `netNavigatorItems(nav, state.netHighlight, ..)`; with `/BAR` highlighted the label is not in the list.
- KiCad 8303b2ad, `net_navigator.cpp::RefreshNetNavigator`: one node per entry of `ConnectionGraph()->GetNetMap()`, `wxString displayName = UnescapeString( net.first.Name );`. `MakeNetNavigatorNode` adds every item of every subgraph of that net (`connectionGraph->GetAllSubgraphs( aNetName )`), labels included, worded `"Label '%s' at (%s, %s)"`.
- A smaller gap in the same function: `netNavigator.ts:55-64` lists a symbol pin only where a wire ends on it (`w.pins`). A pin joined by a label on its end, or by touching another pin, is left out. KiCad lists every `SCH_PIN` of the subgraph: `"Symbol '%s' pin '%s'"`.

**How to see it.** QA `issue18346.kicad_sch` imported: local labels `BAR` and `FOO` on wires of `/BAR` / `/FOO`.
- The tree lists `/BAR`, `/FOO`, `BAR`, `FOO`. KiCad lists `/BAR` and `/FOO`, each with its label and pins.
- Highlight `/BAR`, select the label, press Tab: nothing is selected.

Test: `netNavigator.test.ts::a_label_is_an_item_of_the_net_it_is_traced_to`.
- Input: a wire `{ net: "/BAR", pins: ["J1.2"] }` and a label `{ net: "BAR", net_name: "/BAR" }`.
- Assert that `netNavigatorItems(nav, "/BAR")` contains `Label 'BAR' at ...`, and that `sheetNets` gives `["/BAR"]`.
- Fix direction: key on `net_name ?? net` for labels, and take pins from the pins' `net` (B5-M2's field), not only wire ends.

**Severity.** Medium: a wrong panel and Tab walk on every sheet with local labels. Connectivity is not affected.

**Already known?** No. PARITY-sch §13's Net Navigator row predates batch A's naming.

### C6-M2. Unfold from Bus: the bus entry never turns toward the cursor, and the member's label goes on the wire's last point, not on the first click (medium) **Fixed** on task-r6-schematic: 7595d15c (a bus member as a submenu, the label ghost while moving, and the messages as disabled menu items: Open)
### C6-M2. Unfold from Bus: the bus entry never turns toward the cursor, and the member's label goes on the wire's last point, not on the first click (medium)

**What differs.**
- Ours:
  - the entry is fixed at `+100 mil, +100 mil` (`BusUnfoldDialog.tsx:19-20`, `const size = [ENTRY_SIZE_NM, ENTRY_SIZE_NM]`);
  - the wire starts at `entryAt + size`;
  - `commitDrawn` (`SchematicView.tsx:497-507`) sends `add_label` at `points[points.length - 1]`.
- KiCad 8303b2ad, `sch_line_wire_bus_tool.cpp::doDrawSegments`, the motion branch, until the first click:
  ```cpp
  if( m_busUnfold.in_progress && !m_busUnfold.label_placed )
  {
      VECTOR2I cursor_delta = cursorPos - m_busUnfold.origin;
      ...
      bool flipX = ( cursor_delta.x < 0 );
      bool flipY = ( cursor_delta.y < 0 );
      ...
          size.x = std::abs( size.x ) * xSign;
          size.y = std::abs( size.y ) * ySign;
          entry->SetSize( size );
      ...
          m_wires.front()->SetStartPoint( entry->GetEnd() );
      }
      // Update the label "ghost" position
      m_busUnfold.label->SetPosition( cursorPos );
  ```
  The click branch then places the label where the first click is ("First click when unfolding places the label and wire-to-bus entry", `m_busUnfold.label_placed = true`).
- Smaller points:
  - KiCad lists a group member that is itself a bus as a submenu (`member->Type() == CONNECTION_TYPE::BUS` → `AppendSubMenu`); ours flattens every member (`bus.rs::parse_group`).
  - KiCad's messages are disabled menu items, "No bus selected" and "Bus has no members". Ours are toasts, "Unfold from Bus: put the cursor on a bus first." and "`<bus>` has no member nets to unfold." (`useActionRunner.ts:2321,2325`).
  - `getBusForUnfolding` uses a selected bus wherever the cursor is (`RequestSelection( { SCH_ITEM_LOCATE_BUS_T } )`); ours needs the cursor on the bus (`hitBus`).
  - After the wire, KiCad stays in the wire tool (`doDrawSegments( aEvent, commit, LAYER_WIRE, false )`); ours returns to select (`SchematicView.tsx:509`).

**How to see it.** A bus `D[0..3]` at y = 50 mm. Put the cursor on it at x = 100, press `C`, pick `D0`, click at (90, 40), and double-click at (80, 40).
- KiCad: the entry runs (100, 50)→(97.46, 47.46), and `D0` is at (90, 40).
- Ours: the entry runs (100, 50)→(102.54, 52.54), and `D0` is at (80, 40).

Test: `schBusUnfold.test.ts::the_entry_leans_toward_the_cursor_and_the_label_takes_the_first_click`.
- A pure `unfoldEntrySize(origin, cursor)` with cursor (90, 40) and origin (100, 50) gives `[-2_540_000, -2_540_000]`.
- `commitDrawn`'s label `at` is `pts[1]`.

**Severity.** Medium: wrong geometry from a tool PARITY calls ported. The net is the same.

**Already known?** No. PARITY-sch §11 lists only "the label's persisted spin style" as not ported.

### C6-M3. Edit with Symbol Editor (Ctrl+E) edits the shared library entry, so an edit made from R1 reaches every placement of that `lib_id`; KiCad edits the placed symbol's own copy (medium) **Fixed in part** on task-r6-schematic: 8685a920 (the hovered symbol, the first of several, the broken-link refusal); editing the placed symbol's own copy is a named Differs (one library symbol per id, as D5-M1)
### C6-M3. Edit with Symbol Editor (Ctrl+E) edits the shared library entry, so an edit made from R1 reaches every placement of that `lib_id`; KiCad edits the placed symbol's own copy (medium)

**What differs.**
- Ours: `useActionRunner.ts:1459-1476` binds both `editWithSymbolEditor` and `editLibSymbolWithSymbolEditor` to `symApi.openSymbol(libId)`. The code comment says why: "This IR has one symbol library (no separate schematic-local copy), so both open the same entry".
- KiCad 8303b2ad, `sch_editor_control.cpp::EditWithSymbolEditor`:
  ```cpp
  if( aEvent.IsAction( &SCH_ACTIONS::editWithLibEdit ) )
  {
      symbolEditor->LoadSymbolFromSchematic( symbol );
  }
  else if( aEvent.IsAction( &SCH_ACTIONS::editLibSymbolWithLibEdit ) )
  ```
  The save is `SCH_EDIT_FRAME::SaveSymbolToSchematic`, which touches only the principal symbol and the units of its reference:
  ```cpp
  if( candidateSymbol == principalSymbol
      || ( candidateSymbol->IsAnnotated( &path )
           && candidateSymbol->GetRef( &path, false ) == principalRef ) )
  ```
- Also in that function:
  - KiCad takes the hovered symbol when nothing is selected, and `selection.Front()` when several are; ours needs exactly one selected (`state.selection.size !== 1`).
  - For a broken library link KiCad refuses with "Symbols with broken library symbol links cannot be edited."; ours opens a blank symbol `eda:<ref>`.

**How to see it.** R1 and R2 (`Device:R`). Select R1, press Ctrl+E, add a pin, and save to the schematic.
- KiCad: only R1 has the new pin.
- Ours: every `Device:R` on every sheet has it.

Test: `editing_one_placed_symbols_copy_leaves_the_others` (needs a per-placement library override, as D5-M1's `lib_name` copy would). Until then, add a PARITY "Differs" row next to D5-M1's.

**Severity.** Medium: an edit reaches symbols the user did not pick. It is the documented one-symbol-per-id model, but this route to it is not stated.

**Already known?** In principle (D5-M1). The Ctrl+E consequence is not listed in PARITY-sch or PARITY-symedit.

### C6-M4. Compare Symbol with Library reports what KiCad's `Compare( .., ERC, reporter )` does not, and leaves out the field rows it does report (medium) **Fixed** on task-r6-schematic: 6c545baa, 02f258df (line style and corner radius are not in either side's JSON, keywords and footprint filters not on the schematic side: Open)
### C6-M4. Compare Symbol with Library reports what KiCad's `Compare( .., ERC, reporter )` does not, and leaves out the field rows it does report (medium)

**What differs.** `kicad-port/symbolDiff.ts:79-108` against `lib_symbol.cpp::LIB_SYMBOL::Compare` and `sch_inspection_tool.cpp::DiffSymbol` (8303b2ad).
- **Pins: KiCad compares their place only.** It calls `aPin->SCH_ITEM::compare( *bPin, aCompareFlags )`. `SCH_ITEM::compare` (`sch_item.cpp:722`) tests the type, unit, body style, `IsPrivate()` and position, and returns 0 under `COMPARE_FLAGS::ERC`. Ours (`pinSame`, `:34`) also compares the name, electrical type, shape, angle and length, and reports "Pin %s differs" for them. KiCad says "No relevant differences detected.".
- **Missing pins and fields: never printed by KiCad.** Its second loop looks the library's pin up in the library itself (`const SCH_PIN* aPin = aRhs.GetPin( bPin->GetNumber(), bPin->GetUnit(), bPin->GetBodyStyle() );`, where `aRhs` is the library symbol), so it is always found. The same holds for "Missing field in schematic symbol". Ours (`:105-106`) reports every library pin the schematic lacks.
- **Texts are not graphic items in KiCad.** Only `SCH_SHAPE_T` items go into `aShapes`. Ours puts `text` / `text_box` in the shape lists (`graphicOfResolved` / `graphicOfLibrary`), so a changed body text gives "Graphic item count differs." or "Graphic item differs".
- **Shapes.** `EDA_SHAPE::Compare` compares points within `EPSILON 2` nm, the corner radius, the Bezier controls, the stroke width, the **line style** and the fill. Ours compares a text line with no line style, rounded to 0.1 µm.
- **Fields.**
  - `DiffSymbol` gives the flattened copy the placed symbol's fields (`flattenedSchSymbol->SetFields( fields )`, moved by `-symbol->GetPosition()`).
  - `Compare` then reports "Extra field in schematic symbol: %s." and "Field '%s' differs: %s; %s.".
  - Ours compares no fields (`:6`), and PARITY-sch §13 calls that identical.
- **Power.** KiCad tests `m_options != aRhs.m_options`. Ours tests a boolean (`!!schematic.power !== library.power`), so local against global is silent.

**How to see it.** One case each in `symbolDiff.test.ts`:
- `a_pin_renamed_in_the_library_is_not_a_difference` → `[]`;
- `a_pin_only_the_library_has_is_not_reported` → `[]`;
- `a_body_text_is_not_a_graphic_item` → `[]`;
- `a_user_field_the_library_lacks_is_an_extra_field` → `Extra field in schematic symbol: ...`.

**Severity.** Medium: the report differs in both directions. It is a diagnostic only, with no data effect.

**Already known?** No. PARITY-sch §13 calls these "identical rules".

### C6-L1. Back / Forward keep sheets that were deleted; KiCad's `CleanHistory` drops them on every hierarchy change, and `ResetHistory` clears the history on load (low) **Fixed** on task-r6-schematic: 8ffb4585
### C6-L1. Back / Forward keep sheets that were deleted; KiCad's `CleanHistory` drops them on every hierarchy change, and `ResetHistory` clears the history on load (low)

- Ours: `kicad-port/navHistory.ts` has no clean step. `state.schNav` is set once (`store.tsx:950`). `goBack` to a dead path (`useActionRunner.ts:1437-1443`) lets `resolve_sheet` (`studio.rs:1742`) fall back to the parent while `currentSheetPath` keeps the dead path.
- KiCad 8303b2ad: `SCH_EDIT_FRAME::UpdateHierarchyNavigator` (`sch_edit_frame.cpp:1376`) calls `CleanHistory()`:
  ```cpp
  // Search through our history, and removing any entries
  // that the no longer point to a sheet on the schematic
  ...
  if( std::find( sheets.begin(), sheets.end(), *entry ) != sheets.end() )
  {
      // Don't allow multiple consecutive instances of the same history.
      if( ( entry != m_navHistory.begin() ) && ( *entry == *std::prev( entry ) ) )
          entry = m_navHistory.erase( entry );
  ```
  `ResetHistory()` runs on load (`files-io.cpp:827`, `sch_edit_frame.cpp:456`).
- **How to see it:** enter sheet A, go back, delete A, press Alt+Left. KiCad rings the bell; ours "goes back" to A's dead path.
- **Test:** `navHistory.test.ts::clean_history_drops_paths_no_longer_in_the_hierarchy`. `cleanHistory({ entries: [[], ["A"], []], index: 2 }, [[]])` gives `{ entries: [[]], index: 0 }`.
- **Already known?** No.

### C6-L2. The Hierarchy Navigator has no context menu, expand / collapse or net highlight; an unnamed sheet shows an empty label (low) **Fixed in part** on task-r6-schematic: 8cc119ae (root by project name, nameless sheet by file, fold state, Expand / Collapse All); Edit Page Number, Rename, net highlight, top-level sheets: Open
### C6-L2. The Hierarchy Navigator has no context menu, expand / collapse or net highlight; an unnamed sheet shows an empty label (low)

`components/panels/HierarchyPanel.tsx` is a flat, indented list. KiCad 8303b2ad `widgets/hierarchy_pane.cpp` also has:
- the right-click menu (`onRightClick`):
  - "Edit Page Number", asking "Enter page number for sheet path %s" in a box titled "Edit Sheet Page Number", `wxFILTER_ALPHANUMERIC`;
  - "Rename" ("Change name of this sheet", in place, `renameIdenticalSheets`);
  - "Expand All" and "Collapse All";
  - New / Delete Top-Level Sheet, which need multi-root, not in the model;
- collapsed nodes kept in `m_SchHierarchyCollapsed`;
- `UpdateNetHighlight`, which colours the sheets carrying the highlighted net;
- the empty-name fallback to the file's name (`if( sheetNameBase.IsEmpty() ) { wxFileName fn( sheet->GetFileName() ); sheetNameBase = fn.GetName(); }`);
- the root labelled with the project name (`getRootString`), where ours says "Root".

"name (page N)" is §43's documented Differs.

Test: a Playwright check, `hierarchy-navigator.check.js`.

**Already known?** No PARITY row lists these.

### C6-L3. Symbol Checker: a local power symbol's hidden power-input pin gets no "Hidden power pin" note (low) **Fixed** on task-r6-schematic: c7cee1cd
### C6-L3. Symbol Checker: a local power symbol's hidden power-input pin gets no "Hidden power pin" note (low)

- Ours: `kicad-port/symbolChecker.ts:162` tests `!symbol.power && ..`.
- KiCad 8303b2ad `symbol_checker.cpp::CheckLibSymbol`:
  ```cpp
  if( !aSymbol->IsGlobalPower()
          && pin->GetType() == ELECTRICAL_PINTYPE::PT_POWER_IN
          && !pin->IsVisible() )
  ```
  A `(power local)` symbol is not global, so KiCad gives "Info: Hidden power pin %s ...".
- **Test:** `symbolChecker.test.ts`, with `{ power: true, power_local: true, pins: [{ electrical_type: "power_in", hidden: true, .. }] }`. Assert the messages include `Info: Hidden power pin 1 ...`.
- **Already known?** No.

### Checked and found to match (slice C)

- **`CheckLibSymbol`**, C6-L3 apart:
  - the reference-prefix checks;
  - `CheckDuplicatePins` (stacked numbers expanded, the sort key, the body-style-0 rule, all four message forms, KiCad's raw `prev.pin->GetName()` in the first);
  - the power symbol's checks;
  - off grid against `max( grid, 25 mil )`, with "of" / "in" and `'A' + unit - 1`;
  - y printed as `-GetPosition().y`;
  - a zero circle and rectangle;
  - "Symbol Warnings" / "No symbol issues found.".
- **`SCH_NAVIGATE_TOOL`:**
  - `pushToHistory` (drops the forward tail, never repeats the last entry);
  - `CanGoBack` / `CanGoForward`;
  - Back / Forward do not push, and clear the selection and tool;
  - `LeaveSheet` / `Up` do nothing at the root;
  - Next / Previous in `BuildSheetListSortedByPageNumbers` order;
  - `changeSheet` clears the selection.
- **Hierarchy Navigator:** children sorted by `ComparePageNum`; a click runs `changeSheet` with the path.
- **`SelectNextPrevNetNavigatorItem`** (C6-M1 apart):
  - leaves only;
  - wires, junctions and bus entries skipped;
  - wraps at both ends;
  - the item texts "Symbol '%s' pin '%s' (%s)", "Label / Global label / Hierarchical label '%s' at (%s, %s)", "No-Connect at (%s, %s)";
  - `SortChildren`;
  - the `WildCompareString` filter, off while a net is highlighted.
- **`doUnfoldBus`** (C6-M2 apart):
  - the root at `GetSeg().NearestPoint( cursor )`;
  - the wire starts at `entry->GetEnd()`;
  - one commit, and Escape removes the entry and label;
  - member names by `FullLocalName()` / `prefix.member`.
- **ERC markers:** `NextMarker` / `PrevMarker` / `ExcludeMarker` entry points. The menu, sort and exclusion rows were read word for word in r2 and r3.

## Summary

| ID | Finding | Severity | Confirmed | Known? |
|---|---|---|---|---|
| B6-H1 | A unit's display name (`unit_name`) is dropped from the schematic cache, and units are named by letter (`A1A` against KiCad's `A1TopLeft_UnderSide`) | high | run (QA `issue21980`) | no |
| A6-M1 | Replace All in References is refused for a multi-unit symbol with units on two sheets | medium | run (port 8840) | no |
| A6-M2 | A placed pin's alternate is left out of the duplicate-pin-name test (`Net-(J1-IO-Pad3)` against `Net-(J1-IO)_1`) | medium | run (alt1 against kicad-cli) | no |
| C6-M1 | Net Navigator lists a label under its text (`BAR`) beside the net (`/BAR`); Tab skips it; pins joined without a wire are missing | medium | code (+ r5 import output) | no |
| C6-M2 | Unfold from Bus: the entry never leans toward the cursor; the label lands on the last point | medium | code | no |
| C6-M3 | Ctrl+E edits the shared library entry, not the placed symbol's copy | medium | code | in principle (D5-M1) |
| C6-M4 | Compare Symbol with Library: pins, missing items, texts, line style, fields and power kind compared differently | medium | code | no |
| A6-L1 | Delete of a wire, symbol or label: no stroke-blind `DeleteJunction` merge at a junction left non-explicit | low | code | partly (B5-L1, sheet pins) |
| A6-L2 | The `unconnected-` naming misses a later path's pin alternate (keyed by the symbol's id, looked up by the part's reference) | low | code | no |
| A6-L3 | An instance's `"~"` value or footprint is read as empty in any version (KiCad: below 20250318 only) | low | code | no |
| C6-L1 | Back / Forward keep deleted sheets (no `CleanHistory` / `ResetHistory`) | low | code | no |
| C6-L2 | The Hierarchy Navigator lacks KiCad's context menu, collapse state, net highlight and empty-name fallback | low | code | no |
| C6-L3 | Symbol Checker: no "Hidden power pin" note for a local power symbol | low | code | no |

QA sweep: **123 of 130 alike** (pin groups and ERC counts). The 7 differing are three known causes:
- unannotated or duplicate references, 4;
- `lib_name` copies (D5-M1), 2;
- the cache's missing library user fields, 1.

The new net-name comparison adds B6-H1 (`issue21980`) and the known uuid-hash names of `complex_hierarchy`.

Counts: **high 1, medium 6, low 6**.
