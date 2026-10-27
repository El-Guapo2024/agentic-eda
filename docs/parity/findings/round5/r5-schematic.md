# Round 5 findings: the schematic editor and its files (`r5-schematic`, 2026-10-28)

**Scope.** GAPS items 1, 4, 6, 11 (ERC half), 12 and 20: `crates/engine` (the tracer), `crates/kicad`'s `.kicad_sch` reader and writer, `crates/ops` `sch_*`, the studio's schematic code, and `web/studio/PARITY-sch.md`. Main is at `8a8e47f1`, read only. No code was changed anywhere.

**Priority.**
- (a) Round 4's merges (PARITY-sch §47; e3d9699b, 628d36c1, 4d3a8797, cc0dcd1a, 3946f4d2): slices A and B.
- (b) Rows no round had reviewed:
  - Find / Find and Replace and the Symbol Fields Table (§7, §8): slice C.
  - The Schematic Control rows (§13): slice D.
  - All 130 of KiCad's QA schematic roots run through `eda import-kicad` and judged by kicad-cli: slice D.

**KiCad sources.** Connectivity and ERC are checked against 25ec2335 (the box's kicad-cli 10.99); files and tools against 8303b2ad (`$KICAD_SRC_DIR`).

**Binary.** Main `8a8e47f1`, built by another round-5 reviewer from a `git archive` (checked identical for `ops/lib.rs`, `engine/nets.rs`, `kicad/lib.rs`, `cli/studio.rs`, `cli/board.rs`). It is copied to `/tmp/findings-r5-schematic.V0fy/bin/eda`.

**Scratch.** Everything is under `/tmp/findings-r5-schematic.V0fy/`:
- `gen/` holds round 4's generators plus `cases9-11.py` and `run5.sh` (slice A);
- `sliceB/` holds the cases and `cmd.sh` used against a studio on port 8840, which is now stopped (slice B);
- `bin/qa1.sh`, `bin/roots.py`, `qa/<root>/` and `sweep.txt` are the QA sweep (slice D).

**Already known?** Every finding was checked against `docs/parity/GAPS.md` and `docs/parity/findings/` (rounds 1-4, P11-P18).

**Read first.**
- 108 of KiCad's 130 QA roots now round-trip alike, against the 100 the ratchet counts. Four new causes explain 16 of the other 22: the root's legacy `symbol_instances` (D5-H1, 11 files), a root symbol's own instance (D5-H2, 2), dropped pin alternates (D5-H3, 1) and the `"Net Class"` field (D5-H4, 2). Two more are §47's documented `lib_name` "Differs" (D5-M1), and the last four are known unannotated / multi-unit cases.
- Round 4's pin-fact fixes hold on every case re-run. Two edges are still open:
  - one pin number in two units of a symbol (A5-H1);
  - since A4-M1, a `no_connect` pin whose instance chooses an alternate function joins nothing (D5-H3, QA `issue22286`).
- 3946f4d2's instant merge in `delete_junction` brings back a batch delete that fails or loses wire, when the dot is listed before a half (B5-H1).
- The premise of r3 C3-M3 and r4 C4-L3 is wrong: KiCad's `SCH_LABEL_BASE::doIsConnected` is `GetTextPos() == aPosition`, so a wire finishes on any label or sheet pin anchor (B5-M1).

## Slice A: the round-4 connectivity fixes (e3d9699b), against kicad-cli 25ec2335

**Scope.** e3d9699b (r4 A4-H1..H4, A4-M1, A4-M2, B4-H1); later commits on main that touch these paths (`crates/engine`, `crates/cli/src/board.rs`, `sch_import.rs`, `crates/kicad/src/lib.rs`, `symbol.rs`, `sch_extras.rs`) are only merges and unrelated work. Every diff hunk was read against 25ec2335's `conn_facts.cpp` (the symbol-pin block, about lines 560-680), `conn_islands.cpp::BuildScreenIslands` (`PROPAGATES`, `NC`, `ENTRY`, `BUS_SIDE`, `jumperedWith`), `lib_symbol.cpp` (`IsLocalPower`, `Flatten`), and 8303b2ad's parser for `body_style` / `lib_name`.

**Harness.** Round 4's generator is copied to `/tmp/findings-r5-schematic.V0fy/gen/`. `run5.sh <case>` = run4.sh using the binary of main 8a8e47f1, `/tmp/findings-r5-schematic.V0fy/bin/eda`. New cases:
- `cases9.py` → `cases9/`: entries e1-e5; stacked s2-s6; NC pins n2-n4; jumpers across units and duplicate numbers across units j4-j6; duplicate jumpers with a stacked literal, d2.
- `cases10.py` → `cases10/`: h1-h4, the pin facts on a screen shown twice.
- `cases11.py` → `cases11/dm1`: a jumper symbol placed in its De Morgan body style.

Each run prints three net lists: kicad-cli's netlist of the original file, ours (`design.json`), and kicad-cli's netlist of the derived file. Round 4's j3, g6 and s1 were re-run (`r4copy/`) and still hold.

### A5-H1. One pin number used in two units of a symbol: the tracer keeps one unit's pin and loses the other's net (high; what the A4-H3 fix leaves out)

**Fixed** on task-r5-schematic: 3783b91e

**What differs.**
- Ours: `crates/cli/src/board.rs:785-792` (`pin_key`) adds the unit to the key only for a pin that is common to every unit (`common && ...` → `pin_copy_key`). Any other pin is keyed `REF.NUMBER`.
- So when unit A and unit B of `J1` each have a pin `1`, and both units are placed on one screen, both get the key `J1.1`. `pin_world.insert(key, at)` (`board.rs:610-616` for a drawn symbol, `:648-652` for an imported one) keeps the last one only.
- The A4-H3 fix (`pin_twin_key`) counts repeats within one placed symbol's pins only (`seen` is per symbol), so it does not cover this case.
- KiCad 25ec2335: each placed unit is its own `SCH_SYMBOL`, and each of its pins is its own `ITEM_FACT` (`conn_facts.cpp`: `fact.owner = symbol.m_Uuid`). `BuildScreenIslands` adds a port for every active pin:
  ```cpp
  if( pin.unit != 0 && unit->second != 0 && pin.unit != unit->second )
      continue;
  active.push_back( pin.id );
  addPort( index, pin.id, pin.position, pin.type == ELECTRICAL_PINTYPE::PT_NC ? NC : ANCHOR, 1 );
  ```
  Both pins are contacts, and the netlist lists `J1.1` on both nets.

**How to see it.** `cases9/j5`: symbol `t:MU1`, unit 1 has pin `1` "A" and unit 2 has pin `1` "B". J1A is wired to R1.1 and J1B to R2.1.
- kicad-cli (original and derived): `Net-(J1A-A) [J1.1, R1.1]`, `Net-(J1B-B) [J1.1, R2.1]`.
- Ours: `Net-(J1A-B) [J1.1, R2.1]` and `unconnected-(R1-Pad1) [R1.1]`. R1.1 loses its net, and the surviving net takes the wrong unit letter.
- `cases9/j6`, the same symbol with `(duplicate_pin_numbers_are_jumpers yes)`, gives the same result. That is right for KiCad: `joinApart` acts within one `SCH_SYMBOL`, so units are never joined.

Test: `crates/cli/src/sch_hierarchy_tests.rs::a_pin_number_repeated_in_two_units_is_a_contact_in_each`.
- Input: the j5 file.
- Assert: `design.nets` has `J1.1` on `Net-(J1A-A)` with R1.1, and on `Net-(J1B-B)` with R2.1 (or, under the documented "a pin once" Differs, R1.1 and R2.1 are each on a net named as KiCad names it, and neither is `unconnected-`).
- Fix direction: give a non-common pin of a unit that another placed unit of the same reference also has the unit-copy key (as `pin_key` does for common pins).

**Already known?** No. PARITY-sch §47 "Two pins of one number: ported" covers one placed unit only.

### A5-M1. Two bus wire entries that meet end to end join in ours; in KiCad an entry never propagates to another entry (medium)

**Fixed** on task-r5-schematic: 3783b91e

**What differs.**
- Ours: `crates/engine/src/nets.rs:397-405` seeds both ends of each entry that is off every bus and unites them. Two entries that share an end point land on the same seeded id, so they join.
- KiCad 25ec2335 `conn_islands.cpp`, the `ENTRY` row of `PROPAGATES` (columns ANCHOR, WIRE, BUS, ENTRY, ENTRY_BUS, NC, BUS_SIDE):
  ```cpp
  { true,  true,  false, false, false, false, false },
  ```
  `PROPAGATES[ENTRY][ENTRY]` is false. An entry joins what is at its ends only through a wire or an anchor (a pin, label, junction or power pin).
- So a zig-zag of two 45-degree entries with no wire between them connects nothing in KiCad.

**How to see it.** `cases9/e1`: R1.1 → wire → entry (73.66,38.1)→(76.2,35.56) → entry (76.2,35.56)→(78.74,33.02) → wire → R3.1.
- kicad-cli (original and derived): R1.1 and R3.1 are each `unconnected-(..)`.
- Ours: `Net-(R1-Pad1) [R1.1, R3.1]`.

The derived file is right, so only `design.nets`, and the board built from it, are wrong.

Test: `nets::tests::two_bus_entries_end_to_end_do_not_join`.
- Input: the e1 geometry.
- Assert: two nets.
- Fix direction: join an entry's end only to non-entry items at that point (wires, pins, labels, junctions, power pins), not to another entry's end.

**Already known?** No. §47 says an entry "joins what is at both".

### A5-M2. A symbol placed in its alternate (De Morgan) body style loses its jumper pin groups and its power kind in the tracer (medium)

**Fixed** on task-r5-schematic: 3783b91e

**What differs.**
- `crates/model/src/symbol.rs:330-348` (`LibSymbol::in_style`) builds the style-2 copy with `connectivity: Default::default()`. That drops `jumper_pin_groups`, `duplicate_pin_numbers_are_jumpers` and `power_local`.
- The tracer takes pin facts from that copy:
  - `board.rs:630`: `let lib = if lib.has_alternate_body() { lib.in_style(..) } ..` then `pin_contacts(&keyed, &lib.connectivity)`;
  - `board.rs:602`, `model.real_symbol_of_instance` → `real_symbol_in_style` (`crates/model/src/lib.rs:1482-1489`).
- KiCad keeps one `LIB_SYMBOL` for every body style. `conn_facts.cpp` reads `library->JumperPinGroups()` and `GetDuplicatePinNumbersAreJumpers()` whatever the symbol's body style.
- The writer reads the tokens from `real_symbol_of`, not from `in_style` (`lib.rs:1364-1373`), so the derived file keeps them.

**How to see it.** `cases11/dm1`: `t:DM` has two body styles and `(jumper_pin_groups ("1" "2"))`; J1 is placed with `(body_style 2)`, with R1.1 on pin 1 and R2.1 on pin 2.
- kicad-cli (original and derived): `Net-(J1-A) [J1.1, J1.2, R1.1, R2.1]`.
- Ours: `Net-(J1-A) [J1.1, R1.1]`, `Net-(J1-B) [J1.2, R2.1]`.

Test: `crates/model` `in_style_keeps_the_connectivity` (assert `sym.in_style(2).connectivity == sym.connectivity`), plus the dm1 file as an import test asserting one net.

Fix: `connectivity: self.connectivity.clone()` in `in_style`.

**Already known?** No.

### A5-L1. Two pins of one number on a screen shown twice: the derived file swaps the two nets' names relative to ours and to the original (low)

**Open**: the twin pin's uuid is made, not the file's; connectivity is the same

**What differs.**
- KiCad names the two nets `Net-(J1-A)` and `Net-(J1-A)_1` in an order set by the items' KIIDs.
- Ours gives the twin pin a fresh uuid, `duid_for("pin:{id}:{n}:{nth}")` (`crates/kicad/src/lib.rs:494`). On a shared screen this reorders them.

**How to see it.** `cases10/h3`:
- original: `Net-(J1-A) [J1.1, R1.1]`, `Net-(J1-A)_1 [J1.1, R2.1]`;
- ours: the same names;
- derived: `Net-(J1-A) [J1.1, R2.1]`, `Net-(J1-A)_1 [J1.1, R1.1]`.

So `design.nets` and kicad-cli's view of the derived file name the nets differently. Connectivity is the same. In the single-sheet `j3` the order happens to match.

Test: keep the pin uuids the file gave (as the instance pins' `(uuid ..)`), and assert that kicad-cli's netlist of the derived h3 has the same name per pin set as the original's.

**Already known?** In part: §47's j3 row notes "a pin once". The name order is not noted.

### Checked and found to match (slice A)

All checked by reading 25ec2335 and by running the case (original, ours, derived alike unless noted):
- **Stacked numbers**:
  - `[1-2]` with a jumper group naming `2` and `3` joins pin 3 (s2: `numbers[number]` for `GetStackedPinNumbers`);
  - unconnected naming `unconnected-(J1-A-Pad1)` (s3);
  - a named stacked pin wired is `Net-(J1-A)` (s4);
  - a `no_connect` stacked pin is `unconnected-(J1-NC-Pad1) [J1.1, J1.2]` (s5);
  - an unsorted `[3,1]` (s6: kicad-cli names it `Net-(J1-Pad3)`, and ours matches);
  - on a shared screen (h4).
- **NC pins** (`addPort(.., NC ..)`, `if( a.kind == NC || b.kind == NC ) continue;`, `groups.find` skipped for `nc`):
  - a local label on an NC pin's tip does not join it (n2);
  - an NC pin in a jumper group is left out of it (n3: `!nc && hasJumperGroups`);
  - an NC pin with a no-connect flag (n4);
  - on a shared screen (h2);
  - an NC pin's own net name `unconnected-(U1-NC-Pad1)`.
  - Not repeated here: the NC rule ignoring a chosen alternate pin function, which the main r5-schematic reviewer reports (QA issue22286).
- **Jumpers**:
  - a group across two units on two placed symbols does not join (j4: `vertices[found->second]->owner == fact.owner`);
  - duplicates with `1` and `[1-2]` as different literals are not joined by `joinApart` (d2: the `groups` key is `GetNumber()`, `numbers` is filled only for jumper groups);
  - on a shared screen, per path (h1, `take_facts` → `pins_under_placement`).
- **Bus wire entries**:
  - an end on a pin joins (e2: `[ENTRY][ANCHOR]`);
  - an end on a wire's interior joins (e4);
  - an end on a label joins (e5);
  - round 4's g6.
- **Local power**:
  - `LIB_SYMBOL::IsLocalPower` / `Flatten` take the root symbol's `m_options`, as `build_symbol`'s `connectivity_of(root)` does;
  - jumper groups and `duplicate_pin_numbers_are_jumpers` come from the root too (the `LIB_SYMBOL` copy constructor, lines 329-330 / 662-663);
  - `power_is_local`'s precedence: a screen override, then the library;
  - `sheet_file.rs`'s normalisation keeps an override only where a screen's cache disagrees with the winning library symbol;
  - Update Symbol clears both sets;
  - the writer's `(power local)` and the tracer read the same source (`model.symbols`, then builtins; no installed `power.kicad_sym` symbol is `(power local)`);
  - Paste's `power_kind` comparison with the cached copy (`clipLibSymbol->IsLocalPower() != ..`) renames a clipboard copy of the other kind.
- **B4-H1**: an alias entry stands under the id only where the id has none, both in `import_screen` and in `collect_lib_symbols`. A `(power local)` alias reaches the tracer through the library symbol, because `local_libs` alone would miss it (it lists cache names).
- **The writer**:
  - `(duplicate_pin_numbers_are_jumpers yes)` and `(jumper_pin_groups ..)` in the schematic cache;
  - the k-th pin of a number at the library's k-th place, in the writer, in `pin_points` and in the geometry ports.

## Slice B: round 4's edit, file and ERC fixes (628d36c1, 4d3a8797, cc0dcd1a, 3946f4d2)

**Scope.** Each diff read with `git show`, and the functions it touches read against KiCad 8303b2ad (`$KICAD_SRC_DIR`) for the tools and files and 25ec2335 for ERC.

**Runs.** Binary: main 8a8e47f1, `/tmp/findings-r5-schematic.V0fy/bin/eda`. Studio server on port 8840, stopped. Commands were POSTed to `/api/cmd` with `/tmp/findings-r5-schematic.V0fy/sliceB/cmd.sh`, and the sheet was listed with `sliceB/show.py`.

Scratch cases:
- `sliceB/t1`: r4's `tj1` with the through wire split at the T into two wires, plus a junction. It is imported, because the studio's own `add_wire` merges collinear pieces into one polyline, so it cannot make split halves.
- `sliceB/s2`: r4's `cases7/s1` with the wires removed and a label `X` on each of the two pin ends.

### B5-H1. A delete batch that lists a junction before a wire half: refused, or the other half is deleted too (high; new in 3946f4d2)

**Fixed** on task-r5-schematic: 46e9451f

**What differs.**
- Since 3946f4d2, `delete_junction` merges the in-line wires at the dot straight away (`crates/ops/src/lib.rs:5039-5055`: `scene.merge_at(at); let merged = scene.finish(); ... *sch = merged;`). The merged wire keeps the first wire's id, and the second wire's id is gone.
- 628d36c1 defers only `commit_clean_up` to the end of the batch (`lib.rs:3001-3029`). This merge is not deferred.
- The studio sends one delete verb per selected item, in selection order (`web/studio/src/kicad-port/schDelete.ts:46-70`, which iterates `kept` in the order of `state.selection`, an insertion-ordered `Set`, `actions/useActionRunner.ts:186`). A selection built by clicks, with the dot picked first, sends `delete_junction` before `delete_wire`.
- Two outcomes follow:
  - **The dot, then the second half.** `delete_wire` of the second half fails with `ops_unknown_wire` "no wire with this id". `Cmd::Batch` is atomic, so nothing is deleted. This is C4-H1 again, in another order.
  - **The dot, then the first half.** The first half's id now names the whole merged through wire, so `delete_wire` removes both halves. The user asked for one half and lost the other: data loss and a different net.

KiCad never deletes a junction before the other items. `tools/sch_edit_tool.cpp::DoDelete` flags junctions and removes them at the end:
```cpp
if( sch_item->Type() == SCH_JUNCTION_T )
{
    sch_item->SetFlags( STRUCT_DELETED );
    // clean up junctions at the end
}
...
for( const VECTOR2I& point : pts )
{
    SCH_ITEM* junction = screen->GetItem( point, 0, SCH_JUNCTION_T );
    ...
    if( junction->HasFlag( STRUCT_DELETED ) || !screen->IsExplicitJunction( point ) )
        m_frame->DeleteJunction( &commit, junction );
}
```
By the time `DeleteJunction` runs (`bus-wire-junction.cpp`: it takes the lines with `line->IsEndPoint( aJunction->GetPosition() ) && !( line->GetEditFlags() & STRUCT_DELETED )` and runs `MergeOverlap( screen, firstLine, false )`), the deleted half is already gone. So KiCad keeps the half that was not selected, and the stem.

**How to see it.** Run on main's binary, case `sliceB/t1`. The halves are `wire_0acd4af43d50` (50.8 to 63.5) and `wire_cc0686c7a47c` (63.5 to 76.2), the stem is `wire_30446558fc3b`, and the dot is `jct_d63fcdeb1217`.
- `{"op":"batch","cmds":[{"op":"delete_junction","id":"jct_d63fcdeb1217"},{"op":"delete_wire","id":"wire_cc0686c7a47c"}]}` replies `{"message":"no wire with this id","ok":false}`. Nothing changes.
- `{"op":"batch","cmds":[{"op":"delete_junction","id":"jct_d63fcdeb1217"},{"op":"delete_wire","id":"wire_0acd4af43d50"}]}` replies `ok`. Only the stem is left: the 63.5 to 76.2 half is gone too. KiCad keeps it.
- The wire-first order (r4's C4-H1 batch) deletes everything, as it should.

Test to write, in `crates/ops/src/tests.rs`: `a_junction_listed_before_a_wire_half_is_deleted_last`.
- Input: a T made of two half wires, a stem and a dot (import, or push the wires straight into `sch.wires`).
- Apply `Batch[delete_junction(dot), delete_wire(half2)]`. Assert `Ok`, with `half1` and the stem left and no junction.
- Apply `Batch[delete_junction(dot), delete_wire(half1)]`. Assert the 63.5 to 76.2 segment is still there.

Fix direction, either:
- inside a batch, defer `delete_junction` (with its `merge_at`) to the batch's end, as `DoDelete` does; or
- order the studio's `deleteCmds` with junctions last.

**Already known?** No. C4-H1's fix (628d36c1) and C4-L2's fix (3946f4d2) each hold on their own; this comes from the two together.

### B5-M1. `IsTerminalPoint`: KiCad finishes a wire or bus on any label or sheet pin at its point, connected or not; ours needs a wire or pin there (medium; the premise of r3 C3-M3 and r4 C4-L3 is wrong)

**Fixed** on task-r5-schematic: ffc896e0

**What differs.**
- Ours, `web/studio/src/kicad-port/schTerminalPoint.ts:31-33`:
  ```ts
  const reached = () => sch.wires.some((w) => onPolyline(w.pts as Pt[], p)) || pins.some((q) => same(q, p));
  const sheetPinConnected = () => sch.sheets.some((s) => s.pins.some((sp) => same(sp.at, p))) && reached();
  const labelConnected = () => sch.labels.some((l) => same(l.at, p)) && reached();
  ```
  The test `schTerminalPoint.test.ts` asserts the difference: `"a label nothing reaches is not connected"` is `false`, and so is `"nothing there but the label"` on the bus layer.
- KiCad 8303b2ad, `sch_screen.cpp::IsTerminalPoint`, `LAYER_WIRE` case:
  ```cpp
  label = GetLabel( aPosition, 1 );
  if( label && !label->IsNew() && label->IsConnected( aPosition ) )
      return true;
  sheetPin = GetSheetPin( aPosition );
  if( sheetPin && sheetPin->IsConnected( aPosition ) )
      return true;
  ```
  The `LAYER_BUS` case is the same, with `GetLabel( aPosition )`.
- `SCH_ITEM::IsConnected` is `return doIsConnected( aPosition );` (`sch_item.cpp:482-488`), and labels override `doIsConnected` (`sch_label.h:441`, `:603`, `:668`, for `SCH_LABEL`, `SCH_GLOBALLABEL` and `SCH_HIERLABEL`):
  ```cpp
  bool doIsConnected( const VECTOR2I& aPosition ) const override
  {
      return EDA_TEXT::GetTextPos() == aPosition;
  }
  ```
  `SCH_SHEET_PIN` derives from `SCH_HIERLABEL` (`sch_sheet_pin.h:65`), so it inherits the same test.
- So in KiCad "connected" means "the label's or sheet pin's anchor is this point". It does not consult the connection map. A wire, or a bus, finishes on any local, global or hierarchical label or sheet pin it ends on, even one nothing else reaches.
- Round 3 (C3-M3) and round 4 (C4-L3) both took `IsConnected` to mean "the items the connection map gives it". 3946f4d2 added pins to that rule; the rule itself is not KiCad's.

**How to see it.**
- Put a local label at a point where nothing else is.
- Start a wire elsewhere and click on the label's anchor. KiCad finishes the wire; ours goes on drawing.
- The same for a sheet pin with nothing wired to it: a common case, since that is how you start wiring a new sheet pin.

Test: in `schTerminalPoint.test.ts`, change the two assertions above to `true`, and add a sheet pin with no wire to the wire and bus layers (`true`).

**Severity.** Medium: the wire tool's finish differs on the common "wire up a fresh sheet pin or label" path.

**Already known?** No. C3-M3 and C4-L3 are marked Fixed on this wrong reading.

### B5-M2. A stacked pin (`[1-2]`) has no net in the studio: Highlight Net on it, or on a label sitting on it, does not find the net (medium; C4-M2's fix and A4-H4 do not meet)

**Fixed** on task-r5-schematic: 32ef2aa2

**What differs.**
- `crates/cli/src/studio.rs:1797` builds `pin_net` from `model.nets`. Since A4-H4 (e3d9699b), a stacked pin is listed there as its pads (`J1.1`, `J1.2`).
- The studio looks pins up by the library number:
  - `studio.rs:1820` for the symbol pins' `"net"`;
  - `studio.rs:1977-1984`, the C4-M2 label-on-a-pin fallback: `pin_points_in(..)` gives `"[1-2]"`, and `pin_net.get("J1.[1-2]")` is `None`.
- KiCad: the pin is a subgraph member like any other (`HighlightNet` → `conn->Name()`), and the net is `/X`.

**How to see it.** Case `sliceB/s2`: `J1` pin `[1-2]` and `R1` pin 1, each with a local label `X` on the pin end, and no wires.
- kicad-cli's netlist: `/X [J1.1, J1.2, R1.1]`.
- `/api/schematic`:
  - the label on R1.1 has `net_name "/X"`;
  - the label on J1's stacked pin has `net_name null`, so Highlight Net on it uses the text `X`;
  - J1's pin `[1-2]` has `net null`.

Test, in `crates/cli/src/sch_hierarchy_tests.rs`: `a_label_on_a_stacked_pin_has_the_pins_net`. Import `s2`, take `schematic_json`, and assert both labels and pin `[1-2]` carry `/X`. Look the pin up by its smallest pad (`expand_stacked_pin_notation(..).0[0]`), or by any of its pads.

**Already known?** No.

### B5-L1. Delete Sheet Pin and Delete Sheet clean up only after the deletion; KiCad also deletes the junction at the pin through `DeleteJunction`'s merge (low)

**Fixed** on task-r5-schematic: 46e9451f

**What differs.**
- KiCad: `DoDelete` puts a deleted sheet pin's position (and each pin of a deleted sheet, `for( SCH_SHEET_PIN* pin : sheet->GetPins() ) pts.insert( pin->GetPosition() )`) into `pts`. Then it calls `DeleteJunction`, which merges the in-line wires that end at the dot whatever their strokes (`MergeOverlap( .., false )`).
- Ours: `SchCmd::DeleteSheet` and `DeleteSheetPin` (`crates/ops/src/sch_edit.rs:390-397`) run only `commit_clean_up`. That drops the junction (no longer explicit), but its merge skips wires whose strokes differ (`IsStrokeEquivalent`).
- So two in-line wires of different strokes that met at a dot on the deleted sheet pin stay apart in ours. KiCad merges them.

**How to see it.** Two in-line wires, one dashed and one default, meeting at a sheet pin with a junction there. Delete the sheet pin. KiCad leaves one wire; ours leaves two.

Test: `deleting_a_sheet_pin_merges_the_wires_at_its_junction` (`sch_sheet_pin_tests.rs`).

**Already known?** No. C4-L1's fix covers the junction, not the merge. Cosmetic: connectivity is the same.

### B5-L2. A unit per sheet path is lost when a sheet is pasted or imported: `path_units` keeps the copied screen's old path keys (low)

**Fixed** on task-r5-schematic: beaba393 (a sheet pasted as another placement of its screen; a sheet copied into new screens or brought by Import Sheet is still **Open**)

**What differs.**
- `SchExtras::path_units` is keyed by `path.join("/")` (`crates/model/src/sheet_list.rs:406`).
- `paste_sheets` and `import_sheet` (`crates/ops/src/sch_sheets.rs:467-598`) give the copied screens new sheet ids. They push new `instance_overrides` for each new path (`:588`), but never re-key `path_units` (`grep path_units crates/ops/src/sch_sheets.rs` finds nothing).
- So a copied shared screen whose second path drew unit B falls back to the symbol's own unit (A) on the new paths. That changes the traced pins and the written `(unit ..)`.
- KiCad: `SCH_EDITOR_CONTROL::updatePastedSymbol` finds the clip instance by the clip path and keeps its unit for the new path: `newInstance.m_Unit` from the clip instance, then `aSymbol->SetUnit( newInstance.m_Unit )`, with `newInstance.m_Path = aPastePath.Path()`.

**How to see it.**
- Copy a sheet whose child screen is shown twice, with `U1` drawn as unit B on the second path (the `crates/cli/tests/fixtures/per_path_unit` design), and paste it.
- Assert that `unit_under(.., <new second path>) == 2`. Today it is 1.

**Already known?** No. A4-M3 covered renames and deletes only. Rare, hence low.

### Checked and found to match (slice B)

**628d36c1.**
- The batch clean-up:
  - an outer batch only (`pending_clean_up.is_none()`), so a nested batch adds its sheets to the outer set;
  - one clean-up per sheet the verbs focused (`OnSheet` inside a batch records its sheet), with the focus restored;
  - the design rolled back on any error, the clean-up's included; one undo step.
- r4's C4-H1 order (wires, then the dot) on split halves deletes all of it on main's binary.
- `place_label`, `delete_sheet` and `delete_sheet_pin` now clean up (`pushSchEdit`'s `LOCAL_CLEANUP`); B5-L1 is the exception.
- `path_units` and `excluded_from_pos_files` move with:
  - Edit Reference (`rename_symbol_keys`);
  - Annotate (`rename_in_section`: removes first, then inserts, so swapped references keep their own);
  - Show / Reorganize (`sheet_units::restore`);
  - Delete (`drop_symbol_keys`).
- Annotate never changes a unit (`SecRename` keeps `unit`), so the key's unit is right.
- Paste and Duplicate (the same path, `origin: "duplicate"`) carry `(in_pos_files no)`. The clipboard writer writes the symbol's own flag (`FormatBool( "in_pos_files", !GetExcludedFromPosFiles() )`), and the importer reads it back.
- A pasted symbol on a shared screen keeps its own unit on every path, as `updatePastedSymbol` does.

**4d3a8797.**
- `sheet_file_in_work` (Rust) and `sheetFileInWork` (TS) agree, for `..`, `.`, an absolute path and a plain name.
- `saveSheetCopy`'s default name is the file's own name (`curr_fn.GetFullName()` in `SaveCurrSheetCopyAs`).
- `sheet_made` writes the screen from where the derived tree puts it.
- `tidy_shared_screens` with the command's numbering:
  - FirstFree in Unique mode, starting from the first path's number. This equals `ReannotateByOptions` from the start number, because every number below the first path's (the first free one) is taken.
  - The sheet methods use `page * step + 1` of each path (`SetSheetNumber( path->GetPageNumberAsInt() )`).
  - `Cmd::numbering` passes through `OnSheet` and `InGroup`.
- The first path's reference coming from the page in view is the documented Differs in PARITY-sch §47, so it is not raised again. Note that r4's own D4-M1 scenario (placing while viewing page 3 under "x 100") still gives R301 on page 2.

**cc0dcd1a.**
- `selectionGridSize` against `EE_GRID_HELPER::GetSelectionGrid` (the first item's grid, then any strictly larger) and `GetItemGrid`'s categories:
  - a pin goes to its symbol's grid, a sheet pin to the connectable grid;
  - a table to graphics;
  - a graphic line to graphics, a wire, bus or entry to the wires grid.
- Overrides disabled give the current grid.
- The defaults match `app_settings.cpp`: connected, wires and text on, graphics off; indices 1, 1, 3, 2.
- The label-on-a-pin fallback only looks when no wire is under the label; B5-M2 is the exception.

**3946f4d2.**
- `merge_at` against `DeleteJunction`:
  - wires and buses that end at the dot;
  - `IsParallel`, the same layer (the `GetLayer()` test in `MergeOverlap`);
  - an identical line is dropped;
  - no stroke or junction test.
- `SeverityFromString`: "warning" and "ignore", anything else an error, and a non-object `rule_severities` is ignored.
- The three new marker rules match 25ec2335. `erc.cpp:2323-2329` has `markers.Place( .., pin.sheet, pin.position, ITEM_PATHS::MAIN )`. `reportSourceErrors` (`:192-198`) has `markers.Report( aCode, { source.item }, source.sheet, source.position, ITEM_PATHS::MAIN )`. The legacy branch (`:768-800`) is also sheet-specific, on the pin.


## Slice C: Find / Find and Replace and the Symbol Fields Table (rows no round had reviewed)

**Scope.** `web/studio/PARITY-sch.md` section 8 (Find / Find and Replace) and section 7 (Symbol Fields Table core: grouping, `GetValue`, the sort, `Shorthand`, `ApplyData`, the export), read function by function against KiCad 8303b2ad: `eeschema/tools/sch_find_replace_tool.cpp`, `common/eda_item.cpp::Matches` / `Replace`, `eeschema/sch_field.cpp::Matches` / `Replace`, `sch_symbol.cpp::Matches`, `sch_label.cpp::SCH_LABEL_BASE::Matches` / `Replace`, `sch_pin.cpp::Matches` / `Replace`, `dialogs/dialog_sch_find.cpp` (+ `_base.cpp`), `fields_data_model.cpp`, `dialogs/dialog_symbol_fields_table.cpp`, `common/settings/bom_settings.cpp`, `common/string_utils.cpp` (`StrNumCmp`, `ValueStringCompare`, `SplitString`), `sch_reference_list.cpp` (`Shorthand`, `SCH_REFERENCE::Split`), `sch_sheet_path.cpp::AppendSymbol`. Our side: `crates/ops/src/search.rs`, `crates/ops/src/lib.rs` (`replace_text`), `crates/cli/src/sch_api.rs` (`find`, `fields_table`, `bom_export`), `crates/ops/src/fields_table.rs`, `web/studio/src/kicad-port/schFind.ts`, `components/schematic/{findNavigation,findReplaceOps}.ts`, `components/FindReplaceDialog.tsx`, `components/SymbolFieldsTableDialog.tsx`. Read only; no binary run for this slice.

**Already known.** `docs/parity/findings/p11-p18.md` P11-4 (Find ignores the stored field visibility), P11-5 (one hit set per reference, not per unit), P11-6 (power symbols, text boxes, directive labels, sheets, groups, markers not searched), P11-7 (a reused sheet's references), P11-8 (fields ordered at the symbol's position) are all still open in the code (`search.rs:240`, `:282-289`, `:275-339`, `:248`, `:337`) and are not repeated below. PARITY section 8 lists regex and permissive modes, `CTX_NETNAME` escaping of the *replace* strings, and a label's net-name search as partial; section 7 lists the simplified `ValueStringCompare` and filter matcher, presets, scope and variants.

### C5-M1. Find Next with "Search only selected objects" searches the whole sheet and replaces the selection with the hit (medium)

**Fixed** on task-r5-schematic: 1113c6d2

**What differs.**
- Ours: `web/studio/src/components/schematic/findNavigation.ts:65` asks the server with no scope:
  ```ts
  const reply = await fetchSchFind({ ...search, search_and_replace: false }, undefined, state.currentSheetPath, allSheets);
  ```
  `searchesAllSheets(currentSheetOnly, selectedOnly)` (`kicad-port/schFind.ts`) turns the hierarchy walk off for a selection search, so the walk is the whole sheet in view, not the selection. Then `visitMatch` (`findNavigation.ts:30-31`) always does `SET_SELECTION [match.id]`, so the user's selection is gone after the first Find, and with it what "selected objects" meant. `replaceAndFindNext` (`findReplaceOps.ts:28`, `:41`) fetches without scope too, so a Replace in this mode can replace an item that was never selected. Only Replace All (`findReplaceOps.ts:55-58`) and the highlight (`:80`) pass the scope.
- KiCad, `sch_find_replace_tool.cpp::nextMatch`:
  ```cpp
  if( selectedOnly )
  {
      for( EDA_ITEM* item : m_selectionTool->GetSelection() )
          addItem( static_cast<SCH_ITEM*>( item ) );
  }
  ```
  and `FindNext`:
  ```cpp
  if( !selectedOnly )
  {
      m_selectionTool->ClearSelection();
      m_selectionTool->AddItemToSel( item );
  }
  ```
  `getCurrentMatch()` is `selectedOnly ? m_afterItem : m_selectionTool->GetSelection().Front()`, so Replace in this mode replaces the match the cursor is on among the selection.
- PARITY-sch section 8 says "Search only selected objects | identical for find and replace-all".

**How to see it.** Two resistors R1 and R2, both Value `10k`; select R2 only; Find and Replace, tick "Search only selected objects", Find `10k`. KiCad goes to R2's Value and keeps R2 selected; Find again stays inside R2 (its Reference field, then the end). Ours goes to R1 (the leftmost on the sheet) and selects R1.
Test: `findNavigation.test.ts` (or `schFind.test.ts` with an injected fetch), `find_next_with_selected_only_walks_the_selection`: state `{ selectedOnly: true, selection: {"R2"} }`, server matches for R1 and R2; assert the request carries `scope: ["R2"]`, the visited match is R2's, and `SET_SELECTION` is not dispatched.

**Severity.** Medium: the option gives a wrong result, and Replace can touch an unselected item.

**Already known?** No (PARITY claims identical).

### C5-M2. Replace All in references is refused when one replacement's result is another symbol's old reference, though the end state is unique (medium)

**Fixed** on task-r5-schematic: 674109ac

**What differs.**
- Ours: `crates/ops/src/lib.rs` `replace_text_here` collects `(old, new)` pairs in hit order (ascending x, y: `search.rs:337`) and then renames one by one (`lib.rs:5588`, `for (old, new) in renames { self.rename_symbol(&old, &new)?; }`). `rename_symbol_in_schematic` (`lib.rs:5647-5652`) refuses a name another symbol still holds ("a symbol with this reference is already on the sheet"), and `Cmd` is atomic, so Replace All fails as a whole.
- KiCad: `ReplaceAll` runs `doReplace` per match, and `SCH_FIELD::Replace` sets each reference with no uniqueness test:
  ```cpp
  text = parentSymbol->GetRef( (SCH_SHEET_PATH*) aAuxData );
  isReplaced = EDA_ITEM::Replace( aSearchData, text );
  if( isReplaced )
      parentSymbol->SetRef( (SCH_SHEET_PATH*) aAuxData, text );
  ```
  Then `commit.Push( wxS( "Find and Replace All" ) )`.
- The model needs a unique reference per part (the known limit "Keep annotations cannot make a duplicate reference"), but here every reference is unique after the replace. Only the order in which the renames run makes a collision.

**How to see it.** R1 at x 10 mm and R10 at x 50 mm. Find `R1`, Replace `R10`, "Replace matches in reference designators" on, Plain mode, Replace All. KiCad ends with R10 (was R1) and R100 (was R10). Ours: `ops_duplicate_symbol` "a symbol with this reference is already on the sheet", nothing changes. With R10 placed left of R1, ours succeeds.
Test: `crates/ops/src/tests.rs::replace_all_in_references_renames_through_a_chain`: the two symbols above, `Cmd::ReplaceText { search: { find: "R1", replace: "R10", replace_references: true, .. }, items: None, all_sheets: false }`. Assert `Ok`, and the references are `{R10, R100}` with the board parts following. Fix direction: rename through temporary names, as Increment Annotations already does (PARITY row 495: "renames through temporary names").

**Severity.** Medium: the action is refused, and whether it works depends on where the symbols sit.

**Already known?** No.

### C5-M3. The Symbol Fields Table and its export preview list a shared screen's symbols once, with the first placement's references; KiCad lists each sheet path (medium)

**Fixed** on task-r5-schematic: ef49e9a7

**What differs.**
- Ours: `crates/ops/src/fields_table.rs:427-438` `project_view` adds every screen's `symbols` once ("a screen shared by two sheet symbols is one screen, listed once ... so no row is listed twice"). The second placement's references (`SchematicSection::instance_overrides`, `SymbolPathOverride { sheet_path, reference, .. }`, read by `sheet_list::reference_under`) never reach the table. So the grid, `${QUANTITY}`, `${ITEM_NUMBER}` and the Export tab's live preview (`sch_api.rs::bom_export` -> `fields_table::export_bom_over`) are short by every symbol of every later placement.
- The BOM *file* is written by kicad-cli (`POST /api/sch/bom`, PARITY-sch row 498), and kicad-cli does list every path. So the studio's preview and the file it writes disagree. This is also active code that duplicates kicad-cli's output (the brief's reuse rule): the preview could be kicad-cli's own export into a scratch file.
- KiCad: the dialog fills its list per sheet path, `m_parent->Schematic().Hierarchy().GetSymbols( m_symbolsList, SYMBOL_FILTER_NON_POWER )` (`dialog_symbol_fields_table.cpp:201`), and `SCH_SHEET_PATH::AppendSymbol` makes one `SCH_REFERENCE( aSymbol, *this )` for each path. The data store key is `makeDataStoreKey( aSheetPath, aSymbol )`.

**How to see it.** A screen with R1 (10k) placed twice; the second placement's R1 is R5 (the case of `crates/cli/src/sch_hierarchy_tests.rs`' shared-screen fixtures). Grouped By Value: KiCad's row is `R1, R5 | 10k | Qty 2`; ours is `R1 | 10k | Qty 1`. kicad-cli's exported file says 2.
Test: `fields_table::tests::a_shared_screen_lists_a_reference_per_placement`: build `project_view` from a design whose sheet screen has `instance_overrides` for a second path. Assert the grouped row's references are `R1, R5` and `${QUANTITY}` is `2`. Also assert the export text equals `kicad-cli sch export bom` with the same preset (slow tier).

**Severity.** Medium: the editing table shows a wrong quantity. The file written is right.

**Already known?** No. GAPS item 1 lists "no variants and no scope selector" only, and PARITY section 7 says "covers the design's symbols on every sheet".

### C5-M4. A label whose text holds an escaped character (`/` stored as `{slash}`) is matched on the escaped text (medium)

**Fixed** on task-r5-schematic: 674109ac

**What differs.**
- Ours: `search.rs:328` `matches_text(&l.net, data)`. `l.net` is the stored text, escaped as KiCad stores it: the studio writes it through `EscapeString( CTX_NETNAME )` (`kicad-port/schNetName.ts`), and the import keeps the file's text.
- KiCad, `sch_label.cpp::SCH_LABEL_BASE::Matches`:
  ```cpp
  if( SCH_ITEM::Matches( UnescapeString( GetText() ), aSearchData ) )
      return true;
  ```
  `SCH_FIELD::Matches` likewise uses `wxString text = UnescapeString( GetText() );`. `eda_model::text_vars::unescape_string` already ports `UnescapeString`.
- PARITY section 8 lists only the *replace* side of the escaping ("`CTX_NETNAME` escaping of find/replace strings is not applied") as partial.

**How to see it.** A local label `SDA/SCL` (stored `SDA{slash}SCL`). Find `SDA/SCL`: KiCad finds it; ours says "No match found." Find `slash`: ours finds it, KiCad does not.
Test: `search::tests::a_label_matches_on_its_unescaped_text`: a label with `net: "SDA{slash}SCL"`; assert `find_items(.. "SDA/SCL")` has the label and `find_items(.. "slash")` does not. The Replace side (`SCH_LABEL_BASE::Replace`, `EscapeString( aSearchData.findString, CTX_NETNAME )`) goes with it: Replace `/` with `_` gives `SDA_SCL` in KiCad.

**Severity.** Medium: a common net-name character makes Find miss.

**Already known?** Partly (the replace half is in PARITY section 8; the matching half is not).

### C5-L1. The Reference field also matches when one of the symbol's pins or body texts matches (`SCH_SYMBOL::Matches`); ours checks the fields only (low)

**Fixed** on task-r5-schematic: 674109ac

**What differs.**
- Ours: `search.rs:259-261` `symbol_matches` tests the non-Reference fields.
- KiCad 8303b2ad, `sch_symbol.cpp::SCH_SYMBOL::Matches`, after the fields:
  ```cpp
  // Search non-field lib draw items (pins, graphical text) for completeness.
  for( const SCH_ITEM& drawItem : GetLibSymbolRef()->GetDrawItems() )
  {
      if( drawItem.Type() == SCH_FIELD_T )
          continue;
      if( drawItem.Matches( aSearchData, aAuxData ) )
          return true;
  }
  ```
  `SCH_TEXT::Matches` is `SCH_ITEM::Matches( GetText(), aSearchData )`. So a body text of the library symbol, or a pin name or number with "Search pin names and numbers" on, makes the symbol's Reference field a match too.

**How to see it.** U1 is an op-amp whose library body has a text `OPA`. Find `OPA`: KiCad stops on U1's Reference; ours finds nothing. With "Search pin names and numbers" on, Find `VCC`: KiCad stops on the pin and on U1's Reference; ours on the pin only.
Test: `search::tests::a_symbols_body_text_makes_its_reference_match`.

**Severity.** Low. **Already known?** No.

### C5-L2. A label's own fields (net class, Sheet References, user fields) are not searched (low)

**Fixed** on task-r5-schematic: 674109ac

**What differs.**
- Ours: `search.rs:324-331` tests a label's text only. `SchExtras::label_fields` (PARITY section 39) is not walked.
- KiCad, `nextMatch`'s `addItem`:
  ```cpp
  else if( item->IsType( { SCH_LABEL_LOCATE_ANY_T } ) )
  {
      SCH_LABEL_BASE* label = static_cast<SCH_LABEL_BASE*>( item );
      for( SCH_FIELD& field : label->GetFields() )
          sorted_items.push_back( &field );
  }
  ```
  Each one is matched by `SCH_FIELD::Matches`, so it is subject to visibility and "Include hidden fields".

**How to see it.** A global label with a shown `Netclass` field `HV`. Find `HV`: KiCad stops on the field; ours finds nothing.
Test: `search::tests::a_labels_shown_field_is_found`.

**Severity.** Low. **Already known?** No. P11-6 lists other item kinds, not label fields.

### C5-L3. The Find dialog offers a "Wildcards (* and ?)" mode KiCad's schematic dialog does not have; labels and the selection box differ (low)

**Fixed** on task-r5-schematic: 1113c6d2 (the replace dialog's Regular Expression box stays not ported: no regex engine)

**What differs.**
- Ours: `components/FindReplaceDialog.tsx:102-104` has a Wildcards checkbox (`mode: "wildcard"`). The file's own header says it is "the third EDA_SEARCH_MATCH_MODE the dialog does not expose".
  - In KiCad a `*` in Plain mode is literal: `R*` finds the text `R*` only. In ours, with the box ticked, it finds every R.
  - "Search only selected objects" is shown in Find mode too. KiCad shows `m_cbSelectedOnly` only for the replace dialog (`dialog_sch_find.cpp`: `if( aStyle & wxFR_REPLACEDIALOG ) { ... m_cbSelectedOnly->Show( true ); m_cbReplaceReferences->Show( true ); m_checkRegexMatch->Show( true ); }`).
- KiCad's labels (`dialog_sch_find_base.cpp`), against ours:

  | KiCad | Ours |
  |---|---|
  | "Match &case" | "Match case" |
  | "Whole &words only" | "Whole word" |
  | "Search pin &names and numbers" | "Search pins" |
  | "Include &hidden fields" | "Search all fields (incl. hidden)" |
  | "Search the current selection &only" | "Search only selected objects" |
  | "Replace matches in reference designators" | "Replace in reference designators" |
- `updateFlags` sets only `WHOLEWORD`, `REGEX` (when shown) or `PLAIN`.

**How to see it.** Open Find (Ctrl+F): ours has eight checkboxes including Wildcards and the selection one; KiCad has Match case, Whole words only, Search pin names and numbers, Include hidden fields, Search the current sheet only, Search net names.

**Severity.** Low (an extra mode and wording). **Already known?** No (PARITY section 8 calls the wildcard row identical).

### C5-L4. Find's end and wrap: "No match found." where KiCad says "Reached end of ...", and the wrap never times out (low)

**Fixed** on task-r5-schematic: 1113c6d2

**What differs.**
- Ours, `findNavigation.ts:76`:
  ```ts
  const message = reply.matches.length === 0 ? "No match found." : endReachedMessage(allSheets);
  ```
  After the end, `pickMatch` stores `WRAP_CURSOR`, so the next Find wraps however long afterwards.
- KiCad, `FindNext`:
  ```cpp
  m_frame->ShowFindReplaceStatus( msg + wxS( " " ) + _( "Find again to wrap around to the start." ), 4000 );
  m_wrapAroundTimer.StartOnce( 4000 );
  ```
  The message is shown with no match at all too. Only a Find within 4 s (`if( m_wrapAroundTimer.IsRunning() )`) wraps. A later one searches on from `m_afterItem` again and reports the end again.
- Replace All with nothing to replace: KiCad pushes nothing and says nothing (`if( !commit.Empty() )`). Ours fails the verb with `ops_nothing_to_replace` (`lib.rs:5522`) and shows "Nothing replaced." / "Nothing to replace in the selection." (`findReplaceOps.ts:60`, `:65`).

**How to see it.** Find `zzz` on any sheet: KiCad shows "Reached end of schematic. Find again to wrap around to the start."; ours "No match found.".

**Severity.** Low. **Already known?** No.

### C5-L5. Replace takes the Find cursor, not the current selection; KiCad replaces the selected item when it matches (low)

**Fixed** on task-r5-schematic: 1113c6d2

**What differs.**
- Ours: `findReplaceOps.ts:20-34` replaces the match named by `state.schFind.cursor`. With no cursor it only finds.
- KiCad: `ReplaceAndFindNext` replaces `getCurrentMatch()`, which is `m_selectionTool->GetSelection().Front()` (when not selected-only), and only `if( item && HasMatch() )`.
  - A user who clicks another matching item after a Find has that item replaced.
  - A user who clicks a non-matching item gets no replace, just a Find Next.
  - Ours replaces the earlier cursor's item in both cases.

**How to see it.** Find `10k` (goes to R1), click R3 (also `10k`), Replace. KiCad replaces R3's Value; ours R1's.

**Severity.** Low. **Already known?** No.

### C5-L6. Replace refuses to empty a label; KiCad empties it (low)

**Fixed** on task-r5-schematic: 674109ac

**What differs.**
- Ours: `lib.rs:5569` fails the whole Replace / Replace All with "replacing would leave a net label empty".
- KiCad: `SCH_LABEL_BASE::Replace` -> `EDA_TEXT::Replace` sets the empty text. 25ec2335's ERC then reports `empty_label_name`, a check our ERC table has.

**How to see it.** A label `X`, Replace All `X` with an empty string. KiCad: the label is empty (ERC "empty label name"). Ours: refused, and every other replacement of the batch is lost.

**Severity.** Low. **Already known?** No.

### C5-L7. In the replace dialog the highlight skips References and pins; KiCad's `searchAndReplace` is never set by its dialog (low)

**Fixed** on task-r5-schematic: 1113c6d2

**What differs.**
- Ours: `findReplaceOps.ts:80` sets `search_and_replace: state.schDialog === "replace"` for the brightening. `search.rs:70-73` documents the flag as "set by the Replace paths (Find and Replace dialog's Replace buttons)".
- KiCad 8303b2ad: `searchAndReplace` is only read from and written to the config:
  - `common/eda_draw_frame.cpp:814`: `m_findReplaceData->searchAndReplace = aCfg->m_FindReplace.search_and_replace;`
  - its default is false (`app_settings.cpp`: `PARAM<bool>( "find_replace.search_and_replace", .., false )`);
  - no dialog or tool sets it (grep: `eda_draw_frame.cpp:814/837`, `eda_item.cpp:174`, `sch_field.cpp:580`).

  So in KiCad's replace dialog, References and pins are brightened and visited like in Find. Replace on them is a no-op: `SCH_FIELD::Replace` returns false without `replaceReferences`, and `SCH_PIN::Replace` does nothing on a schematic pin. Ours already gets the same Replace / Find Next result by other means. Only the highlight differs, and the doc comment's claim is wrong.

**How to see it.** Open Find and Replace, find `R1`: KiCad brightens R1's Reference; ours does not.

**Severity.** Low. **Already known?** No.

### C5-L8. A reference with a leading zero (`R01`) is shown and grouped as `R1` in the Fields Table (low)

**Fixed** on task-r5-schematic: ef49e9a7

**What differs.**
- Ours: `fields_table.rs:116-134` `Ref::parse` keeps the number as `u64`, and `number_text` prints `n.to_string()`.
  - `R01` shows as `R1` in the Reference column and in `Shorthand`.
  - `unit_match` (`fields_table.rs:546-548`, `l.r.num == r.r.num`) takes `R01` and `R1` for units of one symbol and merges them into one row, counted once in `${QUANTITY}`.
- KiCad: `SCH_REFERENCE::Split` keeps `m_numRefStr = std::string( refText, ll + 1 )` ("01"). `GetRefNumber()` returns `m_numRefStr`, and `unitMatch` compares `lhRef.GetRefNumber() == rhRef.GetRefNumber()`.

**How to see it.** Symbols `R01` and `R1` (both annotated, a KiCad file can hold both). KiCad shows two rows `R01` and `R1`; ours one row `R1`, Qty 1.
Test: `fields_table::tests::a_leading_zero_reference_is_its_own_symbol`.

**Severity.** Low (rare references). **Already known?** No.

### C5-L9. Fields Table details: the empty-value edit, the Description column, the export preview's variables, the close question (low)

**Fixed** on task-r5-schematic: ef49e9a7 in part: the empty edit, the preview's variables (a symbol's own), the close question's wording. **Open**: the Description column (no per-symbol Description in the IR) and the Save / Discard / Cancel dialog

**What differs.**
- **An empty edit creates a field.** `fields_table.rs:983-985` inserts a user field for an edit even when the value is empty. `ApplyData` creates one only `!destField && ( !srcValue.IsEmpty() || userAdded )`. Setting a group cell of `MPN` to empty adds an empty `MPN` property to every member that had none, and the writer then writes it.
- **No Description column.** `LoadFieldNames` adds `addMandatoryField( FIELD_T::DESCRIPTION, false, false )` (hidden, ready to show). Ours has no Description column: `is_mandatory` lists four fields, and the dialog's field list comes from `user_fields`.
- **The preview does not resolve variables.** The export preview prints raw field text (`row_value`, `fields_table.rs:557-592`). KiCad's `Export` calls `GetExportValue` -> `GetValue( .., resolveVars = true, listMixedValues = true )`, which expands `${..}` through `ref.GetSymbol()->ResolveTextVar`. The file from kicad-cli is resolved, so the preview differs from it when a field holds `${VALUE}` or a project variable.
- **The close question.** `SymbolFieldsTableDialog.tsx:157` asks "Discard the changes that have not been applied?" (OK / Cancel). KiCad's `OnClose` asks `HandleUnsavedChanges( this, _( "Save changes?" ), .. )`: Save applies, Discard drops, Cancel stays.

**How to see it.** Each is one assertion in `fields_table::tests` or `fieldsTableStage.test.ts`:
- an edit `{ id: "R2", field: "MPN", value: "" }` on an R2 with no MPN leaves `user_fields["R2"]` without `MPN`;
- the default view has a hidden `Description` column;
- a user field `${VALUE}-X` exports as `10k-X`.

**Severity.** Low. **Already known?** No. The applied-first export (PARITY row 498) is documented and not raised.

### Checked and found to match (slice C)

- **`EDA_ITEM::Matches`, PLAIN / WHOLEWORD / WILDCARD:** both sides upper-cased unless Match case; the word-character rule `wxIsalnum( c ) || c == '_'`; the whole-word loop that retries from `ii + 1`; `wxString::Matches` anchored on the whole string (`search.rs:96-158`).
- **`EDA_ITEM::Replace`:** the literal search (also in wildcard mode); the text between hits copied from the original, not the upper-cased text; the whole-word guard; "replaced" only when a hit was swapped (`search.rs:164-191`).
- **`SCH_FIELD::Matches` for the Reference:**
  - skipped when `searchAndReplace && !replaceReferences`;
  - a match when the symbol matches;
  - or the reference, or the reference with its unit letter for a multi-unit symbol (`SubReference`).
- **`SCH_FIELD::Replace`:** a Reference is replaced only with `replaceReferences`. `SCH_PIN::Replace` is a no-op on a placed symbol (`/* TODO: waiting on a way to override pins */`), as ours (`ItemKind::Pin => {}`).
- **`SCH_PIN::Matches`:** the name or number with `searchAllPins`, the net name with `searchNetNames` (the per-unit split is P11-5).
- **`nextMatch`'s order:** x, then y, reversed for Previous (the uuid tie-break is P11-8).
- **`FindNext` over the hierarchy:**
  - the sheets in page order (`SortByPageNumbers`);
  - the current sheet first;
  - the sheets before it only after a wrap;
  - `changeSheet` to the match;
  - "Reached end of schematic." / "Reached end of sheet." plus " Find again to wrap around to the start.", word for word;
  - an empty search opens the dialog.
- **`ReplaceAll` over every screen, once each, as one commit;** "Search the current sheet only" or "selected only" limits it to the sheet in view. The per-path References of later placements are P11-7.
- **`SCH_REFERENCE_LIST::Shorthand`:**
  - a run of exactly two is listed;
  - an empty range delimiter never ranges;
  - prefix and consecutive `m_numRef`;
  - joined by `refDelimiter`.
- **`StrNumCmp`:** digit runs by value, then the characters (upper-cased with `aIgnoreCase`); the shorter string first.
- **`GetValue( group, .. )`:**
  - the Reference, Qty and Item columns sort the references with `StrNumCmp` and drop units of one annotated symbol (`std::unique` that keeps `?`);
  - other columns give `INDETERMINATE_STATE` on the first difference;
  - with `listMixedValues`, the sorted set of non-empty values joined by `,`;
  - the Item number is blank on child rows.
- **`cmp` / `Sort`:**
  - the refs inside each row first;
  - then rows by the sort column's trimmed value, with `ValueStringCompare` (simplified, documented);
  - equal values and the Reference column compare the first references with `StrNumCmp`;
  - descending reverses;
  - item numbers after the sort.
- **`groupMatch` / `unitMatch`:** grouping by Reference compares the prefix only; the other grouped columns compare the data store's raw values; `unitMatch` never for `?`. C5-L8 is the exception.
- **`RebuildRows`:**
  - the filter on the full reference;
  - `m_excludeDNP` and `!m_includeExcluded` read the symbol itself, not the store;
  - the ungrouped fast path for single-unit symbols.
- **`Export`:**
  - the header of shown labels;
  - `\r` and `\n` dropped unless "keep line breaks", `\t` unless "keep tabs";
  - the string delimiter doubled inside a field;
  - `\n` after the last shown column;
  - child rows never exported;
  - `includeExcludedFromBOM` forced off before an export.
- **`BOM_FMT_PRESET::CSV` / `TSV` / `Semicolons`:** field, string, reference and range delimiters equal (`,` `"` `,` `""`; tab `""` `,` `""`; `;` `'` `,` `""`).
- **`BOM_PRESET::DefaultEditing`, `GroupedByValue`, `GroupedByValueFootprint`, `Attributes`:** the columns, labels, show and group flags, sort by Reference ascending, and the two filters.
- **`ApplyData`:**
  - Reference is not editable;
  - generated `${..}` fields are skipped, attributes excepted;
  - attributes are set on every unit;
  - a user-added column creates the field empty;
  - a removed column erases the field.
- **The add-field errors:** "Field must have a name." and "Field name '%s' already in use." (`fieldsTableStage.ts:96-102`).


## Slice D: KiCad's 130 QA schematics through `eda import-kicad`, and the Schematic Control rows (PARITY-sch section 13)

**Harness.** `/tmp/findings-r5-schematic.V0fy/bin/qa1.sh <qa root>` copies one QA root's folder (without its `.kicad_pro` / `.kicad_prl`, as the ratchet test `slow_kicad_qa_schematics_written_back_are_judged_as_the_originals` does) under `/tmp/findings-r5-schematic.V0fy/qa/<tag>/`. It runs kicad-cli's kicadxml netlist and ERC JSON on the original, then `eda import-kicad` and `eda board erc -C`, then kicad-cli's netlist and ERC on the derived `out/.kicad/board.kicad_sch`. It compares the pin groups of the nets and the ERC type counts, leaving out `lib_symbol_issues` and `footprint_link_issues`. The roots are listed by `bin/roots.py` with the test's rule (130 roots). The binary is main `8a8e47f1`, at `/tmp/findings-r5-schematic.V0fy/bin/eda`. The results are in `/tmp/findings-r5-schematic.V0fy/sweep.txt`.

**Result.** 108 of 130 are alike. The ratchet test, which goes through Import Sheet, says 100. The 22 that differ fall into six causes, each confirmed by reading the files:

| Cause | QA roots | Finding |
|---|---|---|
| The root's `(symbol_instances ..)` of a KiCad 6 / early-7 file: paths of root-sheet symbols are skipped, and value and footprint are never applied | `issue11926`, `erc_pin_not_connected_basic`, `issue6588`, `issue7203`, `spice_netlists/{chirp, instance_params, sources, legacy_sources, legacy_rectifier, legacy_laser_driver, legacy_sallen_key}` (11) | D5-H1 |
| A root-sheet symbol's own `(instances (path "/<root uuid>" ..))` is skipped, so the `Reference` property wins | `netlists/issue14494`, `netlists/test_hier_no_connect` (2) | D5-H2 |
| A placed pin's alternate function is neither read nor written, and since A4-M1 a `no_connect` library pin with an alternate joins nothing | `issue22286/bugtest` (1) | D5-H3 |
| A directive label's `(property "Net Class" ..)` is not read | `api_kitchen_sink`, `issue23962` (2) | D5-H4 |
| A `lib_name` copy that differs from the `lib_id` copy (§47 "Differs") | `netlists/legacy_power` (wrong net), `same_local_global_power` (ERC check lost) (2) | D5-M1 |
| Known: unannotated `R?` / `J?` named otherwise, the multi-unit reannotation projects | `netlists/complex_hierarchy_shared`, `netlists/issue16915`, `netlists/test_multiunit_reannotate_4`, `_5` (4) | known (PARITY-sch §40) |

PARITY-sch section 40 lists "a power symbol's `${#}` and `empty_power_value` of legacy power symbols (6)" as a cause left unexplained. Those six files are in D5-H1's list: the power symbol's Value is empty in the symbol and is given only by `symbol_instances`.

### D5-H1. A KiCad 6 / early-7 root's `symbol_instances`: root-sheet symbols lose their references, and no symbol gets its value or footprint from it (high)

**Fixed** on task-r5-schematic: 088a5836

**What differs.**
- Ours: `crates/kicad/src/sch_import.rs:1026` (`legacy_symbol_instances`).
  - It needs at least a sheet and a symbol in the path: `let [sheets @ .., sheet, symbol] = parts.as_slice() else { continue };` (`:1040`). A symbol on the root sheet has the path `"/<symbol uuid>"` (one part), so it is skipped. Such a symbol keeps its `(property "Reference" ..)`. In the early-7 format (`20220622`..`20220904`, `(default_instance (reference "") ..)`), that property is `""` or `"U"`, so the symbol gets `U?n`.
  - It reads only `reference` and `unit`. The path's `(value ..)` and `(footprint ..)` are never applied, at any depth.
- KiCad 8303b2ad, `sch_sheet_path.cpp::SCH_SHEET_LIST::UpdateSymbolInstanceData`. It is called for files older than 20221002 (`eeschema_helpers.cpp:140`: `if( schematic->RootScreen()->GetFileFormatVersionAtLoad() < 20221002 ) sheetList.UpdateSymbolInstanceData( ..GetSymbolInstances() )`; also `eeschema.cpp:137`, `files-io.cpp:701`). It walks every sheet path, the root included:
  ```cpp
  for( SCH_SHEET_PATH& sheetPath : *this )
  {
      for( SCH_ITEM* item : sheetPath.LastScreen()->Items().OfType( SCH_SYMBOL_T ) )
      {
          KIID_PATH sheetPathWithSymbolUuid = sheetPath.Path();
          sheetPathWithSymbolUuid.push_back( symbol->m_Uuid );
          ...
          symbol->AddHierarchicalReference( sheetPath.Path(), it->m_Reference, it->m_Unit );
          symbol->GetField( FIELD_T::REFERENCE )->SetText( it->m_Reference );

          if( !it->m_Value.IsEmpty() )
              symbol->SetValueFieldText( it->m_Value );

          if( !it->m_Footprint.IsEmpty() )
              symbol->SetFootprintFieldText( it->m_Footprint );

          symbol->UpdatePrefix();
  ```

**How to see it.**
- QA `issue11926.kicad_sch` (version 20220904). Three `Device:R` symbols, each with `(property "Reference" "" ..)` and `(property "Value" "" ..)`; the root's `(symbol_instances (path "/5dcb91f5-.." (reference "R1") (unit 1) (value "R") ..) ..)`.
  - kicad-cli: `Net-(R1-Pad2) [R1.2, R2.1]`, and so on.
  - Ours (`design.json` and derived): `U?1`, `U?2`, `U?3`, with Value `U?1`.., and the cached symbol's Reference `U`.
- QA `spice_netlists/legacy_rectifier`: `PWR_FLAG` and `GND` have `(property "Value" "" ..)`, and `symbol_instances` gives `(value "GND")`. The derived file gets `empty_power_value` (1), and the GND net is lost from the netlist groups. `legacy_laser_driver` and `legacy_sallen_key` also get `pin_to_pin` 2: the same cause, the power symbols' nets are unnamed.
- QA `issue6588`: `U2` and `U3` become `U?2`/`U?3`, and one of them lands on `U1`'s output net (`U1.1 U1.4 U?2.1 U?2.4`).
- Test: `sch_import::tests::a_kicad6_roots_symbol_instances_give_root_symbols_their_reference_value_and_footprint`.
  - Input: a root of version 20220622 with one `Device:R` whose properties Reference, Value and Footprint are `""`, and `(symbol_instances (path "/<its uuid>" (reference "R7") (unit 1) (value "10k") (footprint "R_0603")))`.
  - Assert the design's symbol is `R7` with value `10k` and footprint `R_0603`. Add a second case on a sheet path: a value in `symbol_instances` replaces the symbol's own.

**Severity.** High: wrong references, values and nets on files KiCad itself wrote. 11 of the 22 QA differences.

**Already known?** No. GAPS item 4 says "KiCad 6's root `symbol_instances` ... are read" (section 39 review). The root-level paths and the value / footprint are not covered, and the doc comment at `:1021` describes only sheet paths. PARITY-sch §40 counts six of these files as "`empty_power_value` of legacy power symbols" with no cause.

### D5-H2. A root-sheet symbol's own `(instances (path "/<root uuid>" ..))` is ignored: the `Reference` property wins where KiCad takes the instance (high)

**Fixed** on task-r5-schematic: 088a5836

**What differs.**
- Ours: `crates/kicad/src/sch_import.rs:1266` (`parse_instance_overrides`) drops the root's uuid and skips a path that is then empty:
  ```rust
  let sheet_path: Vec<String> = path_str.split('/').filter(|s| !s.is_empty()).skip(1).map(String::from).collect();
  if sheet_path.is_empty() {
      continue;
  }
  ```
  Its doc says such a symbol's "reference is its own", that is, the `(property "Reference" ..)` read at `:230`.
- KiCad 8303b2ad: `sch_symbol.cpp::SCH_SYMBOL::GetRef( const SCH_SHEET_PATH* sheet, .. )` (`:876`) takes the instance of the path first. The field is only a fallback ("If it was not found in m_Paths array, then see if it is in m_Field[REFERENCE] -- if so, use this as a default for this path"):
  ```cpp
  for( const SCH_SYMBOL_INSTANCE& instance : m_instances )
  {
      if( instance.m_Path == path )
      {
          ref = instance.m_Reference;
          subRef = SubReference( instance.m_Unit );
          break;
      }
  }
  if( ref.IsEmpty() && !GetField( FIELD_T::REFERENCE )->GetText().IsEmpty() )
  ```
  The root's `SCH_SHEET_PATH::Path()` is `/<root uuid>`, so for a root-sheet symbol the `(path "/<root uuid>" ..)` entry names it. Copy and paste leave a stale property and other paths' entries behind, so this happens in real files.

**How to see it.**
- QA `netlists/issue14494` (version 20230121). Two root resistors have `(property "Reference" "R3")`, with `(path "/9c65c6c4-.." (reference "R2"))` and `(reference "R4")`.
  - kicad-cli: `/Y [R1.1, R2.1]`, `/X [R1.2, R2.2]`, `/subsheet_01/C [R3.1, R4.1]`.
  - Ours: the two root `R3` collide with the sub-sheet's `R3`; one becomes `R?1`. The nets are `/Y [R1.1, R3.1]` and `/subsheet_01/C [R3.1, R?1.1]`, so `R3.1` is on two nets.
- QA `netlists/test_hier_no_connect`: the root's `U4` property has `(path "/a68fb8f1-..")` `U5`.
  - kicad-cli: `unconnected-(U5-PIN1-Pad1) [U5.1, U5.2]`.
  - Ours: a second `U4` (sub3 already has `U4`), and the design holds one part `U4` for two symbols.
- Test: `sch_import::tests::a_root_symbols_own_instance_names_it`.
  - Input: a root `(uuid "r")` with a symbol `(property "Reference" "R3") (instances (project "p" (path "/r/x" (reference "R3")) (path "/r" (reference "R2"))))`.
  - Assert the root symbol is `R2`. Do the same for `(unit ..)` on the root path, which per-path units already read for sheet paths.

**Severity.** High: duplicate references and pins on the wrong nets on files that kicad-cli reads cleanly.

**Already known?** No. PARITY-sch §42 "The reader" row states the root-uuid drop. It does not say that the root's own path is lost.

### D5-H3. A placed pin's alternate function is dropped. Since A4-M1 (e3d9699b), a `no_connect` library pin whose instance picks a connectable alternate joins nothing (high; regression in effect)

**Fixed** on task-r5-schematic: 3783b91e (the studio cannot choose an alternate yet, nor draw its name)

**What differs.**
- Ours:
  - The tracer takes a pin's type from the library pin alone. `crates/cli/src/board.rs:559`: `Some(lp) => lp.electrical_type == "no_connect"`; also `:614`, `:636`. Those keys go to `nc_contacts` (`:682-692`), and `nets.rs:361-393` keeps them out of every island.
  - The instance's `(pin "3" (uuid ..) (alternate "3.out"))` is not read. `symbol_import.rs:549` reads only the library pin's list of alternates. The derived file has no `(alternate ..)`.
- KiCad 25ec2335 `sch_pin.cpp::SCH_PIN::GetType`:
  ```cpp
  if( !m_alt.IsEmpty() )
  {
      if( !m_libPin )
          return ELECTRICAL_PINTYPE::PT_UNSPECIFIED;
      return m_libPin->GetAlt( m_alt ).m_Type;
  }
  ```
  `conn_facts.cpp:596`: `const bool nc = pin->GetType() == ELECTRICAL_PINTYPE::PT_NC;`. A pin whose alternate is `power_in` / `output` / `input` is an ordinary contact. ERC reads the alternate's type too.

**How to see it.**
- QA `issue22286/bugtest.kicad_sch`: `J1` (`custom:MOLEX_PICOBLADE_8PHOR`), every library pin `no_connect`, with `(alternate "1.pow" power_in line)` and others. The instance picks `1.pow`, `2.pow`, `3.out`, `7.in` and `8.pow`.
  - kicad-cli: `GND [J1.1, J1.8]`, `+5V [J1.2]`, `Net-(J1-Pad3) [J1.3, R1.1]`, `Net-(J1-Pad7) [J1.7, R1.2]`.
  - Ours and derived: `J1.1`, `J1.2`, `J1.3`, `J1.7`, `J1.8`, `R1.1` and `R1.2` each `unconnected-(..)`. The derived file's ERC has `no_connect_connected` 5 and `pin_not_connected` 4, which the original does not have.
- Before e3d9699b these pins were joined and only the ERC types were off. A4-M1's rule has made it wrong connectivity, in `design.nets` and so on the board.
- Test: `nets::tests::a_no_connect_pin_with_a_chosen_alternate_joins_its_wire` (or `sch_hierarchy_tests` with the bugtest file as a fixture). Assert `J1.3` and `R1.1` share a net. Also assert that the derived file keeps `(alternate "3.out")` on `J1`'s pin 3.
- Fix direction: read the instance's `(alternate ..)` into the placed symbol (a per-pin map like `pin_swaps`), take the alternate's type wherever `electrical_type` is read (NC rule, ERC pin types, `quiet_pins`), and write it back.

**Severity.** High: wrong connectivity on a file KiCad reads cleanly.

**Already known?** Partly. GAPS item 19 lists "a placed symbol ... has no alternate pin functions" as missing, and r4 cases8 `a1` checked only the net's name. The connectivity consequence is new with A4-M1, and §47 says "a `PT_NC` pin joins nothing" without the alternate.

### D5-H4. A directive label's `(property "Net Class" ..)` is read as no net class (high)

**Fixed** on task-r5-schematic: 43ce360f

**What differs.**
- Ours: `crates/kicad/src/sch_extras_io.rs:460-461` reads a `netclass_flag`'s class only under the canonical name:
  ```rust
  let field = |name: &str| sexpr::find_all(item, "property").find(|p| sexpr::txt(p, 1) == Some(name))...;
  ... netclass: field("Netclass"), component_class: field("Component Class") };
  ```
  A file that writes `"Net Class"` loses the class. The writer then puts `(property "Netclass" "" ..)` (`sch_extras_io.rs:241`). The label-field reader already maps the name (`label_fields.rs:112-113`: "`GetCanonicalName`: "Net Class" is the net class field"); the directive reader does not.
- KiCad 8303b2ad `sch_field.cpp::SCH_FIELD::GetCanonicalName`:
  ```cpp
  if( m_parent && m_parent->IsType( labelTypes ) )
  {
      // These should be stored in canonical format, but just in case:
      if( m_name == _( "Net Class" ) || m_name == wxT( "Net Class" ) )
          return wxT( "Netclass" );
  }
  ```
  `connection_graph.cpp:509` (`field->GetCanonicalName() == wxT( "Netclass" )`) gives the net its class from either spelling. `labelTypes` includes `SCH_DIRECTIVE_LABEL_T`.

**How to see it.**
- QA `issue23962/sub.kicad_sch`: 8 directive labels `(property "Net Class" "Ch0")` .. `"Ch7"`. All 8 are written `(property "Netclass" "" ..)` in the derived `sub.kicad_sch`.
  - With the project's classes, kicad-cli gives the nets `Ch0`..; ours gives `Default`.
  - Without the project, the original has `undefined_netclass` 32 and ours 0.
- QA `api_kitchen_sink`: the `HV` directive label is lost (`undefined_netclass` 2 -> 1).
- 4 QA files write `"Net Class"`. The others are `netlists/hierarchical_component_classes/{hierarchical_component_classes, subsheet_1}`.
- Test: `sch_extras_io::tests::a_directive_labels_net_class_field_is_read_under_either_name`. Input `(netclass_flag "" (at 0 0 0) (property "Net Class" "HV" ..))`; assert `netclass == "HV"`, and that the written file has `(property "Netclass" "HV" ..)`.

**Severity.** High: the nets' net classes are lost on a KiCad-written file, and with them the class's track widths and clearances on the board.

**Already known?** No.

### D5-M1. One library symbol per id: where a `lib_name` copy differs from the `lib_id` copy, the id's is traced, and the nets differ (medium; §47's documented "Differs", effect now measured)

**Open** (documented Differs, PARITY-sch section 48): the model keeps one library symbol per id; the two QA files are named there

**What differs.**
- §47's row: "**Differs**: where both entries are cached and differ (beyond Swap Pins' places), the id's is drawn (one library symbol per id)". B4-H1's fix applies `lib_name` only where the id has no entry (`sch_import.rs:183-186` and following).
- KiCad (`sch_symbol.cpp::GetSchSymbolLibraryName`, `SCH_SCREEN::UpdateSymbolLinks`) links each placed symbol to its own `lib_name` copy.

The two QA cases where it shows:
- `netlists/legacy_power` (version 20211123). `#PWR01002` is `(lib_name "+3.3V_1") (lib_id "power:+3.3V")`, and `+3.3V_1`'s hidden power pin is named `VCC` where `power:+3.3V`'s is `+3.3V`. A legacy power symbol is named by its pin's name.
  - kicad-cli: `VCC [U1001.1, U1001.5]`, `+3.3V [U1001.2]`.
  - Ours: `+3.3V [U1001.1, U1001.2]`, `VCC [U1001.5]`.
  - The netlist the file ships with (`legacy_power.net`) agrees with kicad-cli.
- `same_local_global_power`: `GND_1` / `VCC_1` are `(power global)` and `power:GND` / `power:VCC` are `(power local)`, on one screen. Ours writes every placement `(power local)`, and the two `same_local_global_power` errors are gone. The screen's `local_power` / `global_power` overrides are per id, so they cannot separate two placements of one id on one screen.

**How to see it.** Test: `sch_import::tests::a_lib_name_copy_that_differs_is_the_one_traced`. Use `legacy_power` as a fixture and assert `U1001.1` shares a net with `U1001.5`, named `VCC`.

**Severity.** Medium: the model's limit, documented, but it gives wrong connectivity on legacy files where a symbol was updated in one place only.

**Already known?** Yes in principle (§47 Differs, r4 B4-H1). The connectivity effect and these two QA files are not stated; listed so the Differs row can name them.

### D5-M2. Do not Populate / Exclude from ...: the units of the symbol on other sheets are not changed, and do not count for the new state (medium)

**Fixed** on task-r5-schematic: 4944d606 on the server (every annotated unit on every sheet). **Open**: the studio computes the new state from the sheet in view

**What differs.**
- Ours:
  - `crates/ops/src/sch_control.rs:74-101` (`set_symbol_attrs`) changes `self.schematic_mut()?`'s symbols only, the sheet in view.
  - `web/studio/src/actions/schControlActions.ts:191-199` computes `nextAttributeState` from `sch.symbols` of the sheet in view.
  - A multi-unit `U1` with unit A here and unit B on another sheet ends with unit A `(dnp yes)` and unit B `(dnp no)` in the written file.
- KiCad 8303b2ad `tools/sch_edit_tool.cpp::SCH_EDIT_TOOL::SetAttribute`:
  ```cpp
  // The attributes should be kept in sync in multi-unit parts.
  // Of course the symbol must be annotated to collect other units.
  if( symbol->IsAnnotated( &m_frame->GetCurrentSheet() ) )
  { ...
      for( SCH_SHEET_PATH& sheet : m_frame->Schematic().Hierarchy() )
      {
          SCH_SCREEN* screen = sheet.LastScreen();
          std::vector<SCH_SYMBOL*> otherUnits;
          CollectOtherUnits( ref, unit, libId, sheet, &otherUnits );
          for( SCH_SYMBOL* otherUnit : otherUnits )
              collectedItems.insert( { otherUnit, screen } );
  ```
  `new_state` is computed over `collectedItems`, the other sheets' units included.
- The row in PARITY-sch §13 says "identical: the whole selection (every unit of a multi-unit symbol) goes to one state".

**How to see it.** Test: `sch_control::tests::dnp_reaches_the_units_on_other_sheets`.
- Input: `U1` unit 1 on the root and unit 2 on sheet `/a`. With focus on the root, `SetSymbolAttrs { ids: ["U1"], dnp: Some(true) }`.
- Assert unit 2 in `sheet_contents["a.kicad_sch"]` has `dnp == true`.
- Studio side: with unit 2 already DNP, toggling on the root sets, not clears, as KiCad's `new_state`.

**Severity.** Medium: mixed attributes across units of one part, which kicad-cli's BOM and position files then read per unit.

**Already known?** No (B-L1 is a sheet's own flags; KiCad's `SetAttribute` also takes sheets and rule areas, which that row covers).

### D5-M3. Increment Annotations: "All sheets" is disabled whenever there are sheets, and a shared screen's other paths are not renumbered (medium)

**Fixed** on task-r5-schematic: 4944d606

**What differs.**
- Ours:
  - `web/studio/src/components/schControl/IncrementAnnotationsDialog.tsx:13,45-46`: `const hasSubSheets = (state.schematic?.sheets.length ?? 0) > 0;`, `disabled={hasSubSheets}`, titled "Sub-sheets cannot be edited here yet". The option can be chosen only when it means the same as "Current sheet only".
  - The chosen `scope` is never sent (`api.cmd({ op: "increment_annotations", start, increment })`).
  - The verb renames `self.schematic()`'s own symbol ids (`crates/ops/src/sch_control.rs:201-234`). On a screen shown twice, the other paths' references (`instance_overrides`) stay.
- KiCad 8303b2ad `tools/sch_editor_control.cpp::SCH_EDITOR_CONTROL::IncrementAnnotations`:
  ```cpp
  if( dlg.m_AllSheets->GetValue() )
      schematic->Hierarchy().GetSymbols( references, SYMBOL_FILTER_ALL );
  else
      schematic->CurrentSheet().GetSymbols( references, SYMBOL_FILTER_ALL );
  ...
  commit.Modify( ref.GetSymbol(), sheet.LastScreen(), RECURSE_MODE::NO_RECURSE );
  ref.GetSymbol()->SetRef( &sheet, From_UTF8( fullRef.c_str() ) );
  ```
  That is one reference per sheet path, and every sheet when asked.
- PARITY-sch §13's row does not mention the scope.

**How to see it.** Test: `sch_control::tests::increment_annotations_over_all_sheets`.
- Input: `R5` on the root and `R6` on sheet `/a`. `IncrementAnnotations { start: "R5", increment: 1, all_sheets: true }`.
- Assert `R6` and `R7`.
- Browser: the dialog's "All sheets" radio is enabled on a design with sheets.

**Severity.** Medium: half of the dialog does nothing on any hierarchical design.

**Already known?** No.

### Checked and found to match (slice D)

- The other 108 QA roots: kicad-cli's netlist pin groups and ERC type counts of the original and the derived file are alike. These include all of round 4's `lib_name` files (`issue16223`, `issue1768`, `ground_pin_test_ok`), jumpers (`jumpers`), the bus hierarchies, `issue10926_1`, `netlists/video` and `ERC_dynamic_power_symbol_test`.
- `SCH_EDIT_TOOL::EditPageNumber`: one sheet only (`GetSize() > 1` returns); the hovered sheet; the sheet path's own page; `wxFILTER_ALPHANUMERIC` (`page_number_is_valid`: `char::is_alphanumeric`, Unicode as `wxIsalnum`). The root's own page is the known r2 B-L4.
- `IncrementAnnotations`'s rules otherwise: an empty start is refused (`wxFILTER_EMPTY`); a start with no number does nothing (`IsSplitNeeded`); only references with the same letters and a number at or above the start move; the step is 1..64.
  - Ours refuses a collision with another sheet's reference, where KiCad would make a duplicate. That is the model's limit (a part per reference) and is documented.
- `SetAttribute`'s `new_state` rule on one sheet: set when any lacks it (`nextAttributeState`); the check mark when all have it (`attributeChecked`).

## Summary

| ID | Finding | Severity | Confirmed | Known? |
|---|---|---|---|---|
| D5-H1 | The root's legacy `symbol_instances`: root-sheet paths skipped; value and footprint never applied (11 QA files, the "empty_power_value" ones included) | high | run (QA sweep) | no |
| D5-H2 | A root symbol's own `(instances (path "/<root>"))` is ignored; the stale `Reference` property wins (duplicate references, wrong nets) | high | run (`issue14494`, `test_hier_no_connect`) | no |
| D5-H3 | A placed pin's alternate function is dropped; since A4-M1, a `no_connect` pin with a chosen alternate joins nothing | high | run (`issue22286`) | partly (GAPS 19: no alternates) |
| D5-H4 | A directive label's `"Net Class"` field is read as no class | high | run (`issue23962`, `api_kitchen_sink`) | no |
| A5-H1 | One pin number in two units of a symbol: the tracer keeps one pin | high | run (cases9 j5, j6) | no (what A4-H3 left) |
| B5-H1 | A delete batch with the junction before a wire half: refused, or the other half is deleted too | high | run (port 8840) | no (new in 3946f4d2) |
| D5-M1 | A `lib_name` copy that differs from the id's: the id's is traced (`legacy_power` wrong net; `same_local_global_power` ERC lost) | medium | run | yes, §47 Differs (effect new) |
| D5-M2 | Do not Populate / Exclude: the units on other sheets are left alone | medium | code | no |
| D5-M3 | Increment Annotations: "All sheets" disabled whenever there are sheets; other paths' references not moved | medium | code | no |
| A5-M1 | Two bus wire entries end to end join in ours (`PROPAGATES[ENTRY][ENTRY]` false) | medium | run (cases9 e1) | no |
| A5-M2 | A De Morgan placement loses jumper groups and power kind (`in_style` drops `connectivity`) | medium | run (cases11 dm1) | no |
| B5-M1 | `IsTerminalPoint`: KiCad finishes on any label or sheet pin anchor; ours needs a wire or pin | medium | code | no (r3 C3-M3, r4 C4-L3 premise wrong) |
| B5-M2 | A stacked pin `[1-2]` has no net in the studio's JSON (Highlight Net) | medium | run (sliceB s2) | no |
| C5-M1 | Find "Search only selected objects" searches everything and replaces the selection | medium | code | no |
| C5-M2 | Replace All in references refused on an intermediate collision | medium | code | no |
| C5-M3 | Fields Table and its export preview count a shared screen's symbols once | medium | code | no |
| C5-M4 | Find matches a label's escaped text (`{slash}`) | medium | code | no |
| A5-L1 | Two pins of one number on a shared screen: the derived file's two net names are swapped | low | run (cases10 h3) | no |
| B5-L1 | Delete Sheet / Sheet Pin: no `DeleteJunction` merge across strokes | low | code | partly (r4 C4-L1) |
| B5-L2 | `path_units` not re-keyed on a sheet paste or import | low | code | no |
| C5-L1..L9 | Find and Fields Table details (Reference through pins, label fields, dialog options, end and wrap message, Replace target, emptying a label, highlight, `R01`, the table's small items) | low | code | no |

Counts: **high 6, medium 11, low 12** (C5-L1 to L9 are nine).
