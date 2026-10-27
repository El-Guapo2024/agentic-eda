# Round 3 findings: the schematic editor and its files (`r3-schematic`, 2026-10-28)

**Scope.** GAPS items 1, 4, 6, 12 and 20, and the ERC half of item 11. The code read:
- `crates/engine` (the net tracer);
- `crates/kicad`'s `.kicad_sch` reader and writer;
- `crates/ops` `sch_*`;
- `crates/cli`'s trace, import and ERC glue;
- the studio's schematic code;
- `web/studio/PARITY-sch.md`.

Main is at `5cac9357`, read only.

**Priority.**
- (a) The code merged in round 2: PARITY-sch sections 42-44, and `git log --since=2026-10-26`. That includes whether KiCad's new connectivity engine (25ec2335 `eeschema/connectivity`, the user's choice of 2026-10-27) is applied everywhere.
- (b) The ported and identical rows that no round had reviewed.

**KiCad sources.** `$KICAD_SRC_DIR` = 8303b2ad. Connectivity, and the ERC that kicad-cli runs, are judged against 25ec2335, which is the box's kicad-cli 10.99.

**Binary.** A fresh build of main `5cac9357`, from a `git archive` copy with its own `CARGO_TARGET_DIR`: `/tmp/findings-r3-schematic.jjpH/target/debug/eda`. Main's own `target/debug/eda` is from Oct 23 and was not used. Every practical check ran on scratch copies under `/tmp/findings-r3-schematic.jjpH/` (slices B and D have their own subfolders). The one server I started, on port 8840, is stopped.

**Already known?** Each finding was checked against `docs/parity/GAPS.md` and `docs/parity/findings/`, round 2 included. A finding that repeats a known item is marked as such.

**Read first: the new connectivity engine is applied to net names and joins, but not to the rest.** Round 2 moved the tracer's claims and name edges to 25ec2335. These still follow 8303b2ad:
- bus binding: A3-H2 and A3-M2;
- the choice of which net keeps a shared name: A3-M3;
- the `#`-reference rule: A3-M1;
- the studio's ERC check table: D3-H1 and D3-M2.

Two more places break the "KiCad's net names everywhere" decision:
- `nets::follow` after an edit: A3-H3;
- the net highlight: A3-M4.

The ERC itself is kicad-cli's, so it runs the new engine by construction.

## Slice A: schematic connectivity, and whether KiCad's new engine is applied everywhere

Scope: the round-2 tracer commits bc4f7c1d, f29b4fb7, d931295c, 5e1074a8 and 8613ea1a (`crates/engine/src/nets.rs`, `crates/cli/src/board.rs` `trace_kicad_names` / `trace_design` / `reconcile_schematic`, `crates/model/src/text_vars.rs`), and every other place that names or matches nets on the schematic side (`nets::follow`, the studio's net highlight, `net_labels.rs`, `hier/`). KiCad: 25ec2335 `eeschema/connectivity/` (`conn_claims.cpp`, `conn_facts.cpp`, `conn_bindings.cpp`, `conn_bus.cpp`, `conn_signals.cpp`, `conn_summary.cpp`, `conn_publish.cpp`, `conn_pin_name.cpp`), `eeschema/sch_pin.cpp`.

**Binary used.** A fresh build of main `5cac9357` from a `git archive` copy: `/tmp/findings-r3-schematic.jjpH/target/debug/eda` (built with its own `CARGO_TARGET_DIR`; main's `target/debug/eda` is from Oct 23).

**Harness.** `/tmp/findings-r3-schematic.jjpH/gen/` has round 2's generator (`gen.py`) and my cases (`cases3.py`). Each case is under `/tmp/findings-r3-schematic.jjpH/cases/<case>/`. `gen/run.sh <case dir>` prints kicad-cli's netlist (`kicad-cli sch export netlist`) and then the nets `eda import-kicad` stores in `design.json`. Round 2's t1 to t17 were re-run on the fresh binary: all of them now match kicad-cli (A-N1 to A-N12 hold).

### A3-H1. A pin common to every unit is placed once, at the unit written last: a wire on the other unit's copy connects nothing (high)

**Fixed** on task-r2-leftovers: 8f93aeee (a copy of the pin per placed unit; the net list keeps the pin on the wired copy's net, where KiCad's netlist also lists the other copy unconnected)

**What differs.**
- **Ours:** `trace_design`'s `pins_of` (`crates/cli/src/board.rs:590` for a studio-drawn symbol, `:611` for an imported one) keys a pin's point by `"REF.PIN"`: `pin_world.insert(format!("{}.{number}", sym.id), at)`. Since section 41 (P25-2), a common pin (unit 0) passes the filter `p.unit == 0 || p.unit == sym.unit` for *every* placed unit. So each unit inserts the same key, and the last unit in `sec.symbols` overwrites the others. The tracer then sees the common pin only at that unit's place. A wire to the pin on any other unit ends on nothing.
- The name is affected too. `pin_units` (`board.rs:536`) uses `or_insert_with`, so the first unit's letter wins whichever unit is wired (`Net-(U1A-C)` where KiCad says `Net-(U1B-C)`).
- **KiCad:** every placed unit has its own `SCH_PIN` of the common pin. `conn_facts.cpp` makes a `PIN_FACT` per pin object; only pins of another unit are skipped: `if( unit && member.unit && unit != member.unit ) { inactivePins.insert( member.id ); continue; }`, where `member.unit` is 0 for a common pin. So both copies are on the sheet, each on its own island. Each is named with its own unit: `referenceWithUnit = cached.symbol->GetRef( &aPath, true )`.

**How to see it.** Case `c3` (`t:DUAL`: pin 1 in unit 1, pin 2 in unit 2, pin 3 "C" in `DUAL_0_1`). U1A is at (50.8, 50.8) and U1B at (101.6, 50.8), written in that order. A wire runs from U1A's pin 3 to R1.1.
- kicad-cli: `Net-(U1A-C) ['R1.1', 'U1.3']` (and U1B's copy is `unconnected-(U1B-C-Pad3)`).
- Ours: `unconnected-(R1-Pad1) ['R1.1']`. R1 loses its connection.

Case `c3b` (wire on U1B's copy): kicad-cli gives `Net-(U1B-C)`; ours gives `Net-(U1A-C)`.

KiCad's ERC reports `different_unit_net` on both layouts, but its netlist keeps the wired copy's connection. Ours drops it, depending on file order. Real library: `Device:D_Dual_CommonAnode_AKK_Split` (PARITY-sch section 41's own example).

Test to write, in `crates/cli/src/board.rs` tests: `a_common_pin_wired_on_the_first_unit_only_stays_on_its_wire`. Use a two-unit symbol with a unit-0 pin, both units placed, and a wire from unit A's copy to R1.1. Assert that `trace_kicad_names` puts R1.1 and U1.3 on one net, named `Net-(U1A-C)`. The pins map needs a key per unit (for example `REF.PIN` per placed unit, merged after tracing) or a list of points per key.

**Already known?** No. P25-2 (fixed in section 41) made unit 0 survive the reader, and that fix exposed this.

### A3-H2. A bus sheet pin finds its members on the parent by the old engine's promotion rule, not 25ec2335's `drivesRecord` (high)

**Fixed** on task-r2-leftovers: 8f93aeee (c5 and c5d now as kicad-cli names them)

**What differs.**
- **Ours:** `nets.rs:529-551` (`promoted`) and `:581` (`if !label && !promoted[g] { continue; }`) port 8303b2ad `connection_graph.cpp::processSubGraphs`. A bus that only a sheet pin names finds the parent's member nets only when:
  - no other bus on the sheet has its name (a vector: its prefix);
  - and no global label of its name is on the sheet.
  The doc comment says so (`nets.rs:529-531`).
- **KiCad 25ec2335:** `conn_bindings.cpp::BindBundle`:
  ```
  // A sheet pin names parent members only while it drives its bus there, and answering labels cover children
  const bool named = !sheetPin || source.drivesRecord;
  ...
  if( named )
  {
      for( const auto& [from, to] : alignment.matched )
      {
          parentEdges( result.slots[to] )
                  .push_back( { SCOPE::SHEET, source.record.inst, source.leaves[from].name } );
  ```
  `drivesRecord` is `&claim == &record.claims.front()` (`PrepareBundle`): the sheet pin is the best claim of its own island. Other buses of the same name on the sheet do not matter. `SignalNodes` then keeps a parent edge whenever a signal on that sheet claims the member's name (`claimedNames.contains( node )`).

**How to see it.**
- Case `c5`: one screen shown by two sheet symbols, A and B. Each has a bus sheet pin `D[0..1]` on an unlabelled bus stub. The root has a net labelled `D0` (R1.1). The child has a hierarchical bus label `D[0..1]` and a net labelled `D0` (R3, which is R4 under B).
  - kicad-cli: `/A/D0 ['R1.1', 'R3.1', 'R4.1']`.
  - Ours: three nets, `/A/D0 ['R3.1']`, `/B/D0 ['R4.1']`, `/D0 ['R1.1']`.
- Case `c5d`: one sheet symbol, plus a bus labelled `D[0..1]` elsewhere on the root.
  - kicad-cli: `/D0 ['R1.1', 'R3.1']`.
  - Ours: `/A/D0 ['R3.1']` and `/D0 ['R1.1']`.
- Case `c5b` (one sheet symbol, nothing else of the name) matches, because the old rule promotes there.

Test to write, in `nets.rs`: `a_bus_sheet_pin_that_drives_its_bus_finds_the_parents_members_whatever_else_has_its_name`. Use two sheet symbols on one sheet with a bus pin `D[0..1]` each, a local label `D0` on the parent, and child member nets `D0`. Assert one net.

**Already known?** No. Section 44 moved the names and joins to 25ec2335 but left the bus binding on 8303b2ad's rules. Sections 39 and 41 still cite `processSubGraphs` and `propagateToNeighbors` for buses.

### A3-H3. After an edit, a net no label names keeps its old name, not KiCad's: `design.nets` and the derived schematic disagree (high)

**Fixed** on task-r2-leftovers: 1e96d3ea (no old name inherited at all; a net the edit did not touch keeps its name)

**What differs.**
- **Ours:** `reconcile_schematic` (`board.rs:774-779`) runs `eda_engine::nets::follow` after every schematic command that changes connectivity. For a net the edit touched with no driver, `follow` takes the name of the old net most of its pins came from (`nets.rs:889`: `let inherited = ranked.iter().map(|(n, _)| *n).find(..)`). It uses the tracer's name (KiCad's) only when no old name is free.
  - `design.nets` is the net list the PCB, the exports and the checks read, and `reconcile_schematic` renames every wire after it. So the studio and the board show a name KiCad never gives that net.
  - Writing the schematic then hands KiCad a drawing whose net KiCad names otherwise, so Update PCB from Schematic (F8) renames it. Avoiding that was the reason for the 2026-10-23 decision ("KiCad's net names everywhere": a studio project opened in KiCad must survive F8 unchanged).
- **KiCad:** a net's name is derived from the drawing every time (`DeriveSignal`, `SignalBaseName`: the best claim's `fullName`, else the best pin's). Only a holder's *suffix* is kept from before (`AssignNames`: `previous( node )`, `weight`).

**How to see it.** Confirmed on case `f1`:
1. Import R1, R2 and R3 with a wire from R2.1 to R3.1. Both sides give `Net-(R2-Pad1)`.
2. Serve a scratch copy on port 8840 and post `{"cmd":{"op":"add_wire","pts":[{"x":50800000,"y":46990000},{"x":76200000,"y":46990000}]}}`. This wire joins R1.1.
3. `design.json` now has `Net-(R2-Pad1) ['R1.1','R2.1','R3.1']`, and every wire carries that name.
4. `eda board export -C work --kicad-sch netlist` gives kicad-cli's name for the derived file: `Net-(R1-Pad1) ['R1.1','R2.1','R3.1']`.

The same holds for a deleted label (its net keeps `/X`; KiCad gives `Net-(..)`) and for a reference renamed (the net keeps `Net-(R1-Pad2)` after R1 became R100).

Test to write, in `crates/cli/src/board.rs` tests: `an_undriven_net_an_edit_touches_takes_kicads_name`. Import the three-resistor file, apply `AddWire` R1.1 to R2.1, and assert `design.nets` names the net `Net-(R1-Pad1)`, as kicad-cli names the derived schematic. If the inheritance must stay for the intent's derived sheets (whose labels may not touch), restrict it to nets whose old name is not KiCad's default form (`Net-(`, `unconnected-(`) or to designs not `imported_from_kicad`.

**Already known?** No. GAPS, PARITY-sch and the findings say nothing about it. `follow`'s doc describes the inheritance as a feature, but it predates the KiCad-names decision.

### A3-M1. An ordinary symbol whose reference starts with `#` claims its net's name (medium)

**Fixed** on task-r2-leftovers: 8f93aeee (`DoNetList()` is not modelled)

**What differs.**
- **Ours:** `trace_kicad_names` (`board.rs:520-530`) puts only the pins of `exclude_from_board` symbols into `quiet_pins`.
- **KiCad 25ec2335** (`conn_facts.cpp`):
  ```
  text.canDrive &= symbol->DoNetList() && !symbol->GetExcludedFromBoard( &aPath, variant )
                   && !reference.reference.StartsWith( wxS( "#" ) );
  ```
  The pin still counts as a pin witness (`pinWitness = !globalPowerParent && !localPowerParent`).

**How to see it.** Case `c1`: `Device:R` with reference `#R1`, wired to R2.1.
- kicad-cli: `Net-(R2-Pad1) ['R2.1']` (the netlist leaves `#R1` out).
- Ours: `Net-(#R1-Pad1) ['#R1.1', 'R2.1']`.

`#` sorts before every letter, so a `#` part always wins the pin order. Test: in `trace_kicad_names`, a symbol with reference `#X1` adds its pins to `quiet_pins`. Assert that the net is `Net-(R2-Pad1)`. `DoNetList()` (`m_doNetlist`, 25ec2335 only) is not modelled either.

**Already known?** No. Round 2's A-N9 fixed the excluded-from-board half only. The QA trace test filters out pins of `#` references (`sch_hierarchy_tests.rs:1334`), so it cannot see this.

### A3-M2. Bus members aligned by name when the canonical bus is a group and the other a vector; KiCad aligns by index unless both are groups (medium)

**Fixed** on task-r2-leftovers: 8f93aeee (an unmatched group leaf still gets no slot of its own)

**What differs.**
- **Ours:** `nets.rs:593-597`. When the best bus driver is a group, a member of another bus on it is matched by its local name first: `best_members.iter().position(|b| group_local_name(..) == group_local_name(&name, m)).or_else(|| (child_vector && ..).then_some(i))`.
- **KiCad 25ec2335** `conn_bus.cpp::Align`:
  ```
  const bool    byName = aLeft.shape == BUS_SCHEMA::SHAPE::GROUP && aRight.shape == BUS_SCHEMA::SHAPE::GROUP;
  ...
  for( size_t left = 0; left < aLeft.leaves.size(); ++left )
  {
      size_t right = left;
  ```
  A vector source on a group canonical is aligned by position. The doc comment's "Differs" about `VectorIndex` (`nets.rs:588-591`) describes the old engine's `matchBusMember`, which 25ec2335 no longer has.

**How to see it.** Case `c4`: one bus wire carries a global `{D1 D0}` (the canonical claim) and a local `D[0..1]`, with nets labelled `D0` (R1) and `D1` (R2).
- kicad-cli: `D0 ['R1.1', 'R2.1']`. Slot 0 takes `D1` from the group and `D0` from the vector.
- Ours: `D0 ['R1.1']`, `D1 ['R2.1']`.

KiCad also reports the mixed shapes (`mixedBusShapes`), so this is an edge case. An unmatched leaf of a group source also gets a slot of its own (`unmappedLeft`, a BUS_MEMBER claim), which ours skips. Test: `a_vector_on_a_group_bus_is_aligned_by_position`.

**Already known?** No.

### A3-M3. Of two nets of one name, the one the label names should keep it: the holder ignores the claims' strength and `CLAIM_LESS` (medium; partly known)

**Fixed** on task-r2-leftovers: 8f93aeee (the last tie, KiCad's node key, is still ours: depth, then first pin)

**What differs.**
- **Ours:** `nets.rs:654` orders the colliding nets by the depth of their screen, then name, then first pin.
- **KiCad** `conn_publish.cpp::AssignNames` (`preferred`):
  ```
  if( left.Strong() != right.Strong() )
      return left.Strong();
  if( left.Strong() )
  {
      if( less( right, left ) )
          return true;
      if( less( left, right ) )
          return false;
  }
  ...
  return nodeLess( a, b );
  ```
  `Strong()` is `priority >= PRIORITY::HIER_LABEL`.

**How to see it.** Case `c7`: on the root, sheet pin `X` (nothing inside answers it) is wired to R1.1, and a local label `X` is on R2.1. They are two nets `/X`.
- kicad-cli: `/X ['R2.1']`, `/X_1 ['R1.1']`.
- Ours: `/X ['R1.1']`, `/X_1 ['R2.1']`. The net the user labelled is the one that gets renamed.

Section 44 lists "which keeps the bare name (KiCad: by item uuids)". That covers only the last tie (`nodeLess`). The strength and the claim order come before it and need no uuid. Test: `a_labelled_net_keeps_its_name_over_a_sheet_pins_net_of_that_name`.

**Already known?** In part (section 44 row "Two nets of one name").

### A3-M4. Highlight Net on a label highlights the label's text, not its net; a label is lit only when the net's name equals its text (medium)

**Fixed** on task-r2-leftovers: 20f1c16c

**What differs.**
- **Ours:**
  - `netAtClick` (`web/studio/src/actions/schControlActions.ts:324`) makes each label an anchor with `net: l.net`. `l.net` is the label's *text*: `studio.rs:1901` sends `"net": l.net` of the `NetLabel`.
  - The painter lights a label only when `opts.netHighlight === l.net` (`components/schematic/painter.ts:1058`), and a wire when `=== w.net`, the *traced* name (`/X`, `/sub/X`, `A_EN`).
  - `netAtPoint` considers anchors before wires, and a label at a wire's end is at distance 0 from both. So clicking a local label `X` highlights the name `X`: no wire of `/X` is lit, and every label whose text is `X`, on any sheet, is.
  - Clicking the wire lights the wires but not the label `X`. The same happens for a hierarchical label (`/sub/X`), a label with a text variable (`${SHEETNAME}_EN` against `A_EN`), and a local label on a global net (A-N1).
- **KiCad:** `SCH_EDITOR_CONTROL::HighlightNet` takes the connection of the item under the cursor: `conn->Name()`, the net's name, for a label too. `SCH_EDIT_FRAME::HighlightConnectionAtPosition` and the painter's `m_highlightNetName` compare that name with each item's `Connection()->Name()`.

**How to see it.** On any imported board with a local label `X` on a wire, use Highlight Nets and click the label's anchor: the wire stays unlit. Test to write (`schControlActions.test.ts`): `netAtClick on a local label returns the net its wire carries`. Give it a wire `{net: "/X"}` and a label `{net: "X"}` at its end, and expect `"/X"`. The API should send each label's traced net beside its text, and the painter should compare that.

**Already known?** No. PARITY-sch.md:494 lists Highlight Nets as done ("a click highlights the net of the wire, label or power symbol under it").

### A3-L1. `@{..}` with an error: KiCad drops the label's net from the netlist, ours names it with the raw text (low)

**Open** after task-r2-leftovers: not changed (low; ERC reports such labels anyway)

Case `m1`, 15 expressions in local labels. 12 match kicad-cli exactly:
- `1/3` gives 0.3333333333333333, `2^3^2` gives 512, `-2^2` gives -4, `7%3` gives 1, `10/4` and `2.50` give 2.5;
- `1e3`, `2*(3+4)`, `1==1`, `3>2`, `0.1+0.2` (0.30000000000000004) and `1000000*1000000` also match.

`abs(-3)` is a function (known: functions not ported). For `1/0` and `5-`, kicad-cli's netlist has no net at all for the label's pin, while ours names it `/N@{1/0}`. ERC reports such labels anyway.

### A3-L2. Doc rows still cite the old engine (low)

**Fixed** on task-r2-leftovers: 8f93aeee (nets.rs, net_labels.rs); PARITY-pcb.md row in the docs commit

PARITY-pcb.md:2112 ("A net's name", marked **identical**) has `recacheName`, `driverName`, `compareDrivers` and `propagateToNeighbors` in its KiCad column. `nets.rs:1-33`'s module doc and `net_labels.rs:3` also name `connection_graph.cpp` for buses and `recacheName` for names. After section 44, they should name `conn_claims.cpp`, `conn_bindings.cpp` and `conn_signals.cpp`.

### Checked and found to match (slice A)

- `CLAIM_LESS` as ported in `driver_order`: priority, `-depth` (unscoped 0 for GLOBAL, GLOBAL_POWER_PIN and PIN), `globalPowerParent`, `outputShape`, `!hasPad` (`Contains("-Pad")` on the claim's own name, labels included), then `fullName` by UTF-8 bytes (`SESSION_KEYS::NameLess` is `utf8 <`, the same as Rust's `String` order).
- `BuildIslandRecord`'s edges: GLOBAL plus SHEET for global labels and global power pins; SHEET for local labels; SHEET plus PORT (its own instance) for hierarchical labels; PORT (the child instance) for sheet pins. They come from every claim of the island, not only the best (our `local_names` / `global_names` / `hier_labels` / `sheet_pins` per group).
- `PrepareInstanceClaims`: `fullName = path + name` with an empty path for unscoped claims; a power pin's text is the escaped `GetValue( &aPath, FOR_NETNAME )` for a power symbol and the lib pin's `GetName()` for a hidden power input; a label's is `EscapeString( GetShownText( &path, FOR_NETNAME ), CTX_NETNAME )`, a sheet pin's with its sheet pushed on the path.
- `SCH_PIN::IsGlobalPower` (25ec2335): `PT_POWER_IN`, and then the parent is a global power symbol, or (legacy) the pin is invisible on a symbol that is not a local power one. This matches `power_pins_of` (skipping `lib.power`) and `is_global_power`.
- A power pin's claim keeps `canDrive = valid` even on a symbol excluded from the board (ours: `power_pins_of` does not filter `exclude_from_board`).
- `SignalBaseName` (`ncName` when the best claim is a pin and `!pinWitnesses[1] || noConnect`) and `SUMMARY::Join` (best by `CLAIM_LESS`, witnesses and no-connect merged): `kicad_net_name`'s `pins.len() == 1 || no_connect` over all non-power pins, quiet ones included.
- `RenderPinNetName` (except A-N13, known), and `numbersByName` from `GetPins( &aPath )` without `PT_NC` (A-N6's fix).
- `BindBundle`'s slot claim: `BUS_MEMBER`, the canonical claim's depth and path, `fullName = path + leaf.name`; a global bus's members have no path. Matches `nets.rs:559-571`.
- Labels' member edges are unconditional (`slot.edges`): two labelled buses on one sheet that share a member name are one net (`virtual_member`).
- The `@{..}` evaluator: 12 of 15 expressions are exactly kicad-cli's (A3-L1).
- `hier/root.rs` and `net_labels.rs`: the derived sheets join a sheet pin to the root through a wire stub with a local label of the net's name on it. This holds under both engines: the label and the pin are one island, and the local labels join by SHEET edges.
- ERC is kicad-cli's own on the derived files (`crates/kicad/tests/kicad_cli_erc.rs`, the studio's ERC dialog), so it runs the new engine by construction.
- Round 2's cases t1-t17 on main `5cac9357`: all alike with kicad-cli.

## Slice B: hierarchical sheets and the .kicad_sch reader/writer

Scope:
- **(a) Round-2 commits:** 97496dc2, db432823 (section 42), f8b4ef93 (section 43), 8613ea1a (B-H1), 85b05524 (B-H2), 66d47b36 (B-M3) and 52ac71ac (B-L1).
- **(b) Ported or identical rows** of PARITY-sch sections 19 and 42-44 on file I/O and the hierarchy.

KiCad: 8303b2ad. The scratch cases are under `/tmp/findings-r3-schematic.jjpH/slice-B/<case>/`. `slice-B/rt.sh <src dir> <root> <name>` runs four steps:
1. `eda import-kicad`;
2. `eda board erc -C out`, which writes the derived files;
3. kicad-cli's kicadxml netlist of the original file and of the derived one;
4. prints both `<sheet>` lists, which kicad-cli orders by page.

### B3-H1. A sheet file outside the root's folder is written outside `.kicad/`, and can overwrite the user's own file (high)

**Fixed** on task-r3-sch-files: 179c5cf5

**What differs.**
- **The reader.** Since 85b05524, `sheet_file_key` (`crates/kicad/src/sch_import.rs`) keys a screen by its path from the root's folder. For a file above that folder the key keeps the `..` in front (its doc says "A file outside the root's folder keeps its path from it with the `..` in front"). For an absolute `Sheetfile` the key is `../..` all the way up to `/`.
- **The writer.** `export_schematic` (`crates/kicad-engine/src/lib.rs:563-567`) writes each screen at `work.join(name)`, after `create_dir_all(parent)`.
- So `../shared/amp.kicad_sch` is written at `<out>/shared/amp.kicad_sch`, which is outside `.kicad/`. A key of `../../shared2/amp.kicad_sch` is written outside the output folder altogether.
- **KiCad.** `SCH_IO_KICAD_SEXPR::loadHierarchy` resolves the name the same way (`if( !fileName.IsAbsolute() ) fileName.MakeAbsolute( m_currentPath.top() );`), but it saves a screen only where it read it. Our derived schematic is a copy in `<out>/.kicad/`, so it must not leave that folder.

**How to see it.** Confirmed by running it, in `slice-B/t1/`:
- `src/proj/root.kicad_sch` places `../shared/amp.kicad_sch`. After `eda import-kicad src/proj/root.kicad_sch -o out` and `eda board erc -C out`, the screen lands at `out/shared/amp.kicad_sch`.
- `src/proj/root2.kicad_sch` places `../../shared2/amp.kicad_sch`, and the output is `-o out2`, which is as deep as `src/proj`. The run overwrote the source's own `t1/shared2/amp.kicad_sch`:
  - `cmp` against a copy taken before differs;
  - the file now starts `(generator "eda-kicad")` and has `(title "../../shared2/amp")`.

Test to write, in `crates/kicad-engine`: `a_screen_above_the_roots_folder_is_written_inside_the_work_folder`. Give it a design whose screen key is `../shared/amp.kicad_sch`. Assert that every file written is under `work`, and that kicad-cli's netlist of the derived root lists `/AMP/`.

Possible fix: map a key that starts with `..`, or an absolute one, to a name inside `work`, and write the placing file's `Sheetfile` relative to that.

**Already known?** No. GAPS item 4 says "sheets in a subfolder are read, keyed and written as KiCad resolves them".

### B3-M1. `Sheetfile` and `${SHEETFILE}` show the root-relative key, not the file's own text (medium)

**Fixed** on task-r3-sch-files: 733537a4 (the canvas, move preview and `${SHEETFILE}`; the Properties panel still shows the key)

**What differs.**
- **Ours.** After 85b05524, `SheetInstance::file` holds the key. For example, a sheet placed by `ampli_ht/ampli_ht.kicad_sch` whose file field says `filter.kicad_sch` has `file = "ampli_ht/filter.kicad_sch"`. Three places use that key as the text:
  - `eda_engine::fields::sheet_fields` (`crates/engine/src/fields.rs:802`), for the drawn `Sheetfile` field;
  - `SchShown::new` (`crates/cli/src/text_vars_json.rs:219`), for `${SHEETFILE}`;
  - `PowerValueView::of` (`crates/cli/src/board.rs:692`), for the tracer.

  Only the writer turns the key back into the file's text (`sheet_file_relative_to`, `crates/kicad/src/lib.rs:704-711`).
- **KiCad.** `GetFileName()` is the `SHEET_FILENAME` field's text. `SCH_SHEET::ResolveTextVar` (`sch_sheet.cpp:382-391`) reads it:
  ```
  wxString fieldName = field.IsMandatory() ? field.GetCanonicalName().Upper() : field.GetName();
  if( token->IsSameAs( fieldName ) ) { *token = field.GetShownText( aPath, false, aDepth + 1 ); return true; }
  ```
- So a label `${SHEETFILE}` names one net in ours and another in kicad-cli, and the canvas and Sheet Properties show the key.

**How to see it.**
- The key, confirmed by running it: QA `netlists/complex_hierarchy_shared` through `eda import-kicad` stores `"file": "ampli_ht/filter.kicad_sch"` (`slice-B/chs/`).
- The display and the text variable: from the code only.

Test to write: import that project, then assert two things:
- the Filter sheet's shown `Sheetfile` is `filter.kicad_sch`;
- a `${SHEETFILE}` text on that screen shows `filter.kicad_sch`.

**Already known?** No.

### B3-M2. A sheet's user fields are lost by Paste, Duplicate and Import Sheet (medium)

**Fixed** on task-r3-sch-files: cda452b1

**What differs.**
- **Ours.** 52ac71ac keeps the fields in the placing screen's `SchExtras::sheet_fields`, keyed by sheet id (`crates/model/src/sch_extras.rs:96-100`).
  - `add_sheet_tree` (`crates/ops/src/sch_sheets.rs:166-176`, `:207-211`) clears the new sheet's id and every inner sheet's id, and never moves `sheet_fields`.
  - `paste_sheets` (`:478-488`) carries `sheet_pin_looks` over to the new ids, but not the fields.
- **KiCad.** The fields are the sheet's own `SCH_SHEET::m_fields`, so they travel with any copy of the sheet.

**How to see it.** From the code. Take QA `ERC_dynamic_power_symbol_test`, whose sheets have a `param` field used by `${param}` labels. Duplicate one sheet: the copy's `${param}` no longer resolves, so its labels name other nets. Import Sheet of that root brings its inner sheets in without `param`.

Test to write: `a_pasted_sheet_keeps_its_user_fields`, and the same for Import Sheet.

**Already known?** No. Section 44 says the fields are "read, written and resolved".

### B3-M3. The studio's shown text does not resolve a sheet's user fields; only the tracer does (medium)

**Fixed** on task-r3-sch-files: 733537a4

**What differs.**
- **Ours.** 52ac71ac added the fields to `PowerValueView::of` (`board.rs:692-694`) only. `SchShown::new` (`text_vars_json.rs:215-221`) still builds each level from `Sheetname` and `Sheetfile` alone.
- **KiCad.** One `SCH_SHEET::ResolveTextVar` serves both the display and the netlist.

**How to see it.** From the code. A `${param}` label on an imported `ERC_dynamic_power_symbol_test` sub-sheet is drawn raw, while its net is `param_subsheet_1`.

Test to write: build `SchShown` for that breadcrumb and assert that `${param}` shows the field's value.

**Already known?** No. Section 44 says "resolved for what the sheet shows".

### B3-M4. A sheet whose file is missing is dropped from the walk, so pages collide in the derived file (medium)

**Fixed** on task-r3-sch-files: 48f86d0f

**What differs.**
- **Ours.**
  - `sheet_list::walk` (`crates/model/src/sheet_list.rs:57-75`) skips a placement that has no screen: `let Some(content) = .. else { continue }`.
  - The writer's own walk still counts that placement (`export_kicad_sch_tree`, `w.pages += 1`).
  - The other sheets take `walk`'s numbers, so two sheets can get one page.
- **KiCad.**
  - `loadHierarchy` (`sch_io_kicad_sexpr.cpp:257-276`) gives such a sheet an empty screen and queues the `IO_ERROR`.
  - `BuildSheetList` lists it like any other sheet, so `SetInitialPageNumbers` numbers it.

**How to see it.** Confirmed by running it, in `slice-B/miss/`: the root places A (`a.kicad_sch`, which is missing, at x=50) and B (at x=150), neither with a page.
- The derived `board.kicad_sch` writes both A and B as `(page "2")`.
- KiCad numbers them 2 and 3.
- The studio's virtual pages, "sheet number x 100" annotation and Next/Previous Sheet all count without A.

Test to write: `a_sheet_with_no_screen_keeps_its_place_in_the_page_numbers`.

**Already known?** No.

### B3-M5. Another project's page for the same placement overrides this project's (medium, narrow)

**Fixed** on task-r3-sch-files: 48f86d0f

**What differs.**
- **Ours.** The sheet reader (`sch_import.rs:564-577`) takes the first `(path ..)` of any project as the placement's page. It files any later differing page under the path with the first component dropped (`.skip(1)`), whatever root uuid that was. So `/<otherRoot>` and `/<thisRoot>` collide on the key `""`, and the later one wins in `sheet_pages`.
- **KiCad.** `SCH_SHEET::getPageNumber( aParentPath )` (`sch_sheet.cpp:1794-1808`) compares the whole path, root uuid included: `if( instance.m_Path == aParentPath )`.

**How to see it.** Confirmed by running it, in `slice-B/mp2/`. Sheet A carries `(project "root" (path "/<root>" (page "3")))` and `(project "other" (path "/9999.." (page "9")))`; sheet B has page 5.
- Ours writes A as `(page "9")`.
- So kicad-cli orders the derived file's sheets B, A, where the original's are A, B.

Fix: compare the first component with the root's uuid, and skip the instances of other roots.

**Already known?** In part: round 2's B-L4 covers the symbol half.

### B3-M6. A sheet file that places itself is followed 32 levels deep; KiCad refuses it (medium, from the code)

**Fixed** on task-r3-sch-files: 48f86d0f (a file that places itself is listed once, as kicad-cli lists 2 sheets; one placing a screen above its parent is listed empty)

**What differs.**
- **Ours.** `sheet_list::walk` has a depth cap (`path.len() > MAX_DEPTH`, 32, at `sheet_list.rs:58-61`) but no ancestor test. So a file that places itself gives 33 entries, and Annotate, `keep_pages` and the navigator all use them. The writer's own walk has an `in_progress` guard.
- **KiCad.** It refuses such a file in two places:
  - `loadHierarchy` (`sch_io_kicad_sexpr.cpp:219-222`) reports: "Could not load sheet '%s' because it already appears as a direct ancestor in the schematic hierarchy."
  - `BuildSheetList` (`sch_sheet_path.cpp:1042-1043`) checks `wxCHECK2_MSG( sheet->GetFileName() != aSheet->GetFileName(), continue, wxT( "Recursion prevented in SCH_SHEET_LIST::BuildSheetList" ) )`.

**How to see it.** In `slice-B/rec/`, kicad-cli lists 2 sheets for the original and for the derived file. The 33 entries are read from the code.

Test to write: `walk(d).len() == 2` for a screen that places its own file.

**Already known?** No.

### B3-L1. Page numbers keep their whitespace; KiCad strips it on read (low)

**Fixed** on task-r3-sch-files: 48f86d0f

Neither the sheet reader (`sch_import.rs:564-570`) nor `legacy_sheet_instances` (`:996`) strips whitespace from a page. KiCad does, in `sch_io_kicad_sexpr_parser.cpp:3877-3887` and `:2585-2590`:
```
// Whitespaces are not permitted
for( wxString ch : whitespaces ) instance.m_PageNumber.Replace( ch, wxEmptyString );
```

### B3-L2. A sheet's own `(page "")` is not read as `#` (low)

**Fixed** on task-r3-sch-files: 48f86d0f

66d47b36 maps an empty page to `#` only in the root's `sheet_instances`. In a sheet's own instances, `sch_import.rs:570` still drops it. KiCad's `parseSchSheet` (`T_instances`, `:3870-3875`) says: "Empty page numbers are not permitted ... Use hash character instead".

### B3-L3. A deleted sheet's `sheet_fields` and `sheet_pages` stay behind (low, unconfirmed)

**Fixed** on task-r3-sch-files: cda452b1

No verb removes them. A sheet drawn again under the same id would take them back. I have not confirmed that ids can repeat.

### Checked and found to match (slice B)

- **66d47b36** against `parseSchSheetInstances` / `UpdateSheetInstanceData`:
  - an empty page becomes `#`;
  - the root uuid is handled;
  - kicad-cli applies the pages on every load (`eeschema_helpers.cpp:143`);
  - the pages are matched by the whole path and override the sheet's own.

  On QA `complex_hierarchy_shared` and `test_hier_renaming`, the original's and the derived file's sheet orders are alike.
- **85b05524** `sheet_file_key` against `loadHierarchy` (`MakeAbsolute( m_currentPath.top() )`, `push( fileName.GetPath() )`, screens found by full path): `complex_hierarchy_shared` writes `ampli_ht/*.kicad_sch`, and kicad-cli reads 5 sheets in the original order. B3-H1 and B3-M1 are the exceptions.
- **f8b4ef93:**
  - `sheets_in_order` against `SCH_SCREEN::GetSheets`;
  - `next_page_number` against `GetNextPageNumber`;
  - `placement_counts`;
  - the nested imported pages not kept, against `AddNewSheetInstances`;
  - `SetInitialPageNumbers` only when every page is empty.
- **8613ea1a:**
  - the overrides move with their symbol;
  - `fold_first_path_references` keeps the id on a clash or on differing units, and skips `R?`.

  Which path's reference becomes the id is a cosmetic difference: KiCad uses `GetOrdinalPath`, the first path in page order; ours uses the first path in walk order.
- **97496dc2:**
  - `reference_under` / `override_under` against `SCH_SYMBOL::GetRef`;
  - `parse_instance_overrides` and `legacy_symbol_instances`;
  - the writer's `(path ..)` per path, for symbols and sheets, against `saveSymbol` / `saveSheet`.
- **52ac71ac**, in the reader, the writer and the tracer:
  - `is_sheet_field` against `parseSchField`;
  - `resolve_level` against `SCH_SHEET::ResolveTextVar`.

## Slice C: schematic edit tools

Scope:
- **(a) Round-2 commits:** 9decd1b9, 89e27c22, c539e423, 160a67f2, 3aba7fed, 527995b1, 34f656dd and d727abc2, each read with `git show` and checked against KiCad function by function.
- **(b) Editing rows of PARITY-sch marked "identical"** that no round had reviewed: Delete, `J`, the wire tool's auto-finish, and Repeat.

KiCad: 8303b2ad `eeschema`. This slice was read from the code only. `eda` has no CLI verbs for wires or junctions, so none of these findings was run.

### C3-H1. Deleting a junction (or an item with a junction on its point) leaves the wires split there; KiCad merges them, so connectivity differs (high)

**Fixed** on task-r2-leftovers: 8bb401b6

**What differs.**
- **Ours:**
  - `delete_junction` (`crates/ops/src/lib.rs:4871`) only drops the dot.
  - `delete_wire` (`:4818`), `delete_symbol` (`:4754`), and the label, bus-entry and no-connect deletes only remove their own item.
  - Nothing looks at the junctions on the deleted items' points, and nothing runs `clean_up`.
  - The studio's Delete (`web/studio/src/kicad-port/schDelete.ts:52-66`) sends just these commands.
- **KiCad:** `sch_edit_tool.cpp::DoDelete` collects the connection points of every deleted item, then:
  ```
  if( junction->HasFlag( STRUCT_DELETED ) || !screen->IsExplicitJunction( point ) )
      m_frame->DeleteJunction( &commit, junction );
  ```
  `bus-wire-junction.cpp::SCH_EDIT_FRAME::DeleteJunction` merges each parallel pair of wires that end at the dot (`secondLine->MergeOverlap( screen, firstLine, false )`). The commit then runs `CleanUp`.
- **Effect on connectivity:** KiCad joins a wire's middle only through a junction ("Junctions connect wires that pass through their position as midpoints").
  - **Crossing:** after the dot is deleted, KiCad has two crossing wires that are not connected.
  - **T:** KiCad has one bar, with the stem's end in its middle and cut off.
  - **Ours:** the 3 or 4 pieces still end on the point, so they stay one net.
- **What it reaches:** KiCad files always store a junction with its wires broken, and since 9decd1b9 our `add_wire` breaks them too. So the defect reaches imported designs and every wire drawn since.

**How to see it.** Test to write: `delete_junction_at_crossing_unjoins`.
1. Make four wire pieces that end at (10,0), with a junction there.
2. Run `delete_junction`.
3. Expect two wires, (0,0)-(20,0) and (10,-10)-(10,10), on two nets.

The smaller cases (medium on their own):
- Deleting the stem of a T: KiCad removes the dot and merges the bar; ours keeps the dot on a plain wire.
- Deleting a symbol whose pin had a junction: KiCad removes the dot, ours keeps it.

**Already known?** No. PARITY-sch.md:164-165 say Delete is identical, and :402 says a junction can be deleted.

### C3-M1. Only drawing a wire and the move/rotate/align tools run KiCad's commit-time `CleanUp` (medium)

**Fixed** on task-r2-leftovers: 8bb401b6 (Delete and the placing verbs; other verbs that edit connectable items, e.g. Swap or Edit Properties, still do not)

**What differs.**
- **KiCad:** `sch_commit.cpp::pushSchEdit` sets `LOCAL_CLEANUP` for any commit that holds a connectable item ("Do a local clean up if there are any connectable objects in the commit."). It then calls `RecalculateConnections( this, connectivityCleanUp )`, which runs `CleanUp`.
- **Ours:** `Scene::clean_up` (`sch_scene.rs:714`) runs from:
  - `add_wire`;
  - the drag finish (`sch_drag.rs:832`, `:914`);
  - rotate and mirror (`sch_move.rs:674`) and align (`:1324`);
  - the pin helpers.

  It does not run after:
  - Delete;
  - `J`;
  - placing a symbol or power symbol;
  - placing a label, a no-connect or a bus entry.

**How to see it.** Place a power symbol in the middle of a wire (a junction is added and the wire splits), then delete it.
- KiCad: the dot goes and the halves merge.
- Ours: the dot and both halves stay. A stale 2-way junction stays until some unrelated later edit cleans it up.

Test to write: `delete_power_symbol_on_wire_cleans_up`.

**Already known?** No.

### C3-M2. `J` neither breaks the wires through the dot nor runs `CleanUp` (medium)

**Fixed** on task-r2-leftovers: 8bb401b6 (a dot on fewer than three items is refused with KiCad's message)

**What differs.**
- **Ours:** `add_junction` (`lib.rs:4859-4868`) only pushes the dot. The writer then writes the wire unbroken through it.
- **KiCad:** `sch_drawing_tools.cpp::SingleClickPlace` runs `lwbTool->AddJunction( &commit, screen, cursorPos ); m_frame->Schematic().CleanUp( &commit );`, and `AddJunction` ends with `BreakSegments( aCommit, aPos, aScreen );`.
- **Effect:** connectivity is the same. Later edits differ: dragging or deleting an arm takes the whole unbroken wire in ours.

**How to see it.** Test to write: `place_junction_breaks_crossing_wires`. Expect four wires after the junction is placed.

**Already known?** No. PARITY-sch.md:192 and :402 say identical.

### C3-M3. The wire tool finishes only on a pin; KiCad finishes on any terminal point, and a bus never finishes on a pin (medium)

**Fixed** on task-r2-leftovers: 8bb401b6

**What differs.**
- **Ours:** `SchematicView.tsx:660-669` finishes on `onPin`, for both the wire and the bus tool.
- **KiCad:** `sch_line_wire_bus_tool.cpp` says "Terminate the command if the end point is on a pin, junction, label, or another wire or bus.", with `if( screen->IsTerminalPoint( cursorPos, segment->GetLayer() ) ) finishSegments( aCommit );`. `SCH_SCREEN::IsTerminalPoint` checks by layer:
  - on `LAYER_WIRE`: a bus-wire entry, a junction, a pin, a wire, a connected label or a connected sheet pin;
  - on `LAYER_BUS`: a bus, a sheet pin or a label, and never a pin.

**How to see it.** Draw a T onto an existing wire: KiCad commits it, ours keeps drawing.

**Already known?** In part. PARITY-sch.md:188 says "identical, pins only", which contradicts itself. The bus case is not listed.

### C3-M4. The clean-up never merges a `solid` wire with a `default` one (medium; new in c539e423)

**Fixed** on task-r2-leftovers: 8bb401b6

**What differs.**
- **Ours:** `sch_scene.rs:767` merges two wires only if `stroke_of(i1) == stroke_of(i2)`. A file wire with `(type solid)` is stored as `SchLineStyle::Solid` (`sch_extras.rs:415`, `:645-647`), and a wire drawn in the studio has no stroke entry. So the two never merge.
- **KiCad:** `sch_line.h::IsStrokeEquivalent` treats DEFAULT and SOLID as equal: `style_a == style_b || ( style_a == LINE_STYLE::DEFAULT && style_b == LINE_STYLE::SOLID ) || ( style_a == LINE_STYLE::SOLID && style_b == LINE_STYLE::DEFAULT )`.

**How to see it.** Test to write: `clean_up_merges_solid_with_default`.

**Already known?** No.

### C3-L1. `add_wire`'s `finishSegments` also runs for Break/Slice and Repeat (low)

**Open** after task-r2-leftovers: not changed (Break and Repeat need verbs of their own); the stale `add_wire` doc is fixed in 8bb401b6 and says so

- **Callers:** `schBreak.ts:92-102` and `schRepeat.ts:68` send `add_wire` too.
- **KiCad finishes those differently:**
  - a break is a move (`finalizeMoveOperation`);
  - `RepeatDrawItem` runs `TrimOverLappingWires`, `AddJunctionsIfNeeded` and `CleanUp`.

  Neither runs `TrimWire` across a two-pin part. So a repeated wire across a resistor's pins shorts it in KiCad, and is cut between the pins in ours.
- **Buses:** ours skips the `TrimWire` pass when a bus is drawn. KiCad runs it for any draw, and `TrimWire` acts on `LAYER_WIRE` only.
- **Stale doc:** `lib.rs:4766-4768` still says that `CleanUp` and `TrimWire` are "Not here".

### C3-L2. Align cleans up only the items that moved (low)

**Fixed** on task-r2-leftovers: 4cdc30e7 (every item cleaned up; "nothing to move" is still refused rather than an empty commit)

- **Ours:** `sch_move.rs:1295-1316` passes only the moved items to the clean-up.
- **KiCad:** `doAlignCleanup` adds every item of `itemsToAlign`, the anchor included.
- **Nothing to move:** ours returns `ops_nothing_to_move`; KiCad pushes an empty commit silently.
- **Stale doc:** `sch_move.rs:1269-1270` still says wires stretch.

### C3-L3. The sheet-pin special case of a drag (89e27c22) differs in two narrow ways (low, unconfirmed effect)

**Open** after task-r2-leftovers: not changed (unconfirmed effect)

- **Past a junction:** ours (`sch_drag.rs:161-172`) records the pin even when a junction is there. KiCad's `getConnectedItems` returns early: "If you're connected to a junction, you're only connected to the junction."
- **Per pin:** KiCad's `m_specialCaseSheetPins` keeps one line per pin; ours keeps every line.

### C3-L4. Autoplace decides "power symbol" by the library flag, not by the `#` reference (low)

**Fixed** on task-r2-leftovers: 4cdc30e7 (the `#` rule; the body box's library text is still a point)

- **KiCad:** `m_is_power_symbol = !schSymbol->IsInNetlist()`, where `m_isInNetlist = !ref.StartsWith( wxT( "#" ) )` (`sch_symbol.cpp:974`).
- **Ours:** `fields.rs:438` uses `lib.power`. The two differ for a `#LOGO`-style symbol.
- **Body box:** `symgeom.rs:104` counts a library text as one point. `GetBodyBoundingBox` uses the text's `GetBoundingBox()`.

### C3-L5. PARITY-sch.md:191 is stale (low)

**Fixed** on task-r2-leftovers: the docs commit (PARITY-sch.md rows 188, 191, 192)

It says T-junction dots are "never a stored item". Since 9decd1b9, `add_wire` and `settle_placed` store them in `sch.junctions`.

### Checked and found to match (slice C)

- **9decd1b9: `add_wire` against `finishSegments`.** The steps are in KiCad's order:
  1. the new ends, plus the connection points that lie on a new segment, are collected before the wire is added;
  2. `CleanUp`;
  3. `TrimWire` for each symbol with at most 2 connection points, hidden pins included, per unit and body style;
  4. `AddJunction` where `IsExplicitJunctionNeeded`.
- **`TrimWire` (`sch_drag.rs:881`):**
  - the `aStart == aEnd` guard;
  - wires only, moving lines skipped, never a whole wire;
  - which piece is kept at each break;
  - it stops after the first match;
  - a new piece is marked new.
- **`settle_placed` against `PlaceSymbol`:** `TrimOverLappingWires` plus `AddJunctionsIfNeeded`. A label gets neither, as in KiCad.
- **89e27c22 against `performItemMove`:** a line end follows the pin only when that end is flagged, and the drag additions count as selected.
- **c539e423:** width, colour and style are compared (C3-M4 is the exception). A merged line keeps the first line's stroke, and a split piece keeps its stroke.
- **160a67f2:** `Move( delta )` after `adjustDeltaForGrid`; then Trim, Junctions, `ClearTempFlags`, `CleanUp`.
- **3aba7fed:** one item uses `MirrorSpinStyle( !vertical )`; several items use `MirrorX` / `MirrorY`; a directive label's turn counter-clockwise is +90.
- **527995b1:** `pinsOnSide` and `getPinsBox` skip invisible pins (power pins excepted), and the body box takes the visible pins' roots.
- **34f656dd:** other units collide; `BOX2I::Intersects` counts touching boxes (`<=`); a wire's box grows by half its pen.
- **d727abc2:**
  - the drag additions are taken out of the selection first;
  - the cleanup gets the selection plus the new and changed drag lines;
  - AutoRotate runs on the user's items only.

## Slice D: clipboard, Annotate, ERC, view controls

Scope: GAPS items 6, 11 (the ERC half) and 20.

- KiCad: 8303b2ad and 25ec2335. The box's kicad-cli is `10.99.0-unknown-25ec2335f2` (`kicad-cli version --format about`).
- Scratch runs: `/tmp/findings-r3-schematic.jjpH/slice-D/scratch/`, with `KICAD_CONFIG_HOME` set to a scratch folder.

**ERC runs on the new engine, but our ERC data does not.** The ERC itself is kicad-cli's (`crates/cli/src/kicad_engine.rs::erc`, run on the derived project, `crates/kicad-engine/src/lib.rs:1181`).

In 25ec2335 the ERC goes through the new engine:
- `m_ConnectivityEngine` is true (`common/advanced_config.cpp:302`);
- `CONNECTION_GRAPH::RunERC` returns `ERC_TESTER::TestConnectivity( *m_schematic )` (`connection_graph.cpp:3971`);
- markers are placed by `ERC_MARKER_PLACER`.

Nothing of ours writes a `kicad_advanced` file, so the ERC follows the user's choice of engine.

What does not follow it is the studio's own ERC data, all of it from 8303b2ad:
- the check table: `crates/model/src/erc_checks.rs` and `web/studio/src/kicad/erc_checks.json`;
- the titles;
- the marker rules in `crates/kicad/src/erc_exclusions.rs`.

D3-H1, D3-M1, D3-M2 and D3-L5 come from that.

### D3-H1. Nine ERC checks of the running kicad-cli are unknown to the studio (high)

**Fixed** on task-r3-sch-files: 201f2183

**What differs.** 25ec2335 `erc_item.cpp` and `erc_settings.cpp` add nine checks:

| Check key | KiCad title | Default severity |
|---|---|---|
| `wired_implicit_power` | "Wired hidden power input pin also connects globally by name" | WARNING |
| `empty_label_name` | "Label has an empty name" | ERROR |
| `empty_power_value` | "Power symbol has an empty value" | ERROR |
| `pin_map_bad_pad` | | |
| `pin_map_unmapped_pin` | | WARNING |
| `pin_map_duplicate_pad` | | |
| `pin_map_stale_pin` | | WARNING |
| `variant_symbol_invalid` | | ERROR |
| `variant_symbol_incompatible` | | WARNING |

All nine are in `allItemTypes`, so the `rule_severities` loader reads them: `if( aJson.contains( key ) ) m_ERCSeverities[code] = SeverityFromString( aJson[key] );`. None of them is in our `ERC_CHECKS` (46 keys). As a result:
- **Import drops the setting.** `import_kicad.rs:383` (`keep_project_erc_settings`, 5d66ea32) keeps a key only if `default_severity(key).is_some_and(|d| d != value)`, so a project's severity for these keys is silently dropped.
- **Board Setup refuses them.** `ops/src/review.rs:83` (`set_erc_severities`) rejects them with "not an ERC check".
- **The marker menu does nothing.** "Ignore all" and "Change severity" on such a marker send nothing (`rcItems.ts:355` `severitiesAfter`, `boardSetupRules.ts:477`).
- **The titles are made up.** `ercTitle` (`checkerOps.ts:48`) falls back to the key, so the menu says "empty label name" where KiCad says "Label has an empty name".

**How to see it.** Confirmed by running both tools, in `slice-D/scratch/el/`:
1. Copy QA `erc_pin_not_connected_basic` and add a `(label "" ..)`. kicad-cli reports `empty_label_name` and `label_dangling`.
2. Set `{"erc":{"rule_severities":{"empty_label_name":"ignore","label_dangling":"ignore"}}}` in the project. kicad-cli now reports neither.
3. `eda import-kicad` says "kept: 1 severities", and `eda board erc` then reports `empty_label_name` as an error.

Test to write: add `"empty_label_name": "ignore"` to `a_kicad_projects_erc_settings_come_in_with_the_schematic` and assert that it is kept. The fix is to regenerate the table (`tools/extract-erc-checks.js`) from 25ec2335.

**Already known?** No. PARITY-sch section 9 says the 46 checks are "identical".

### D3-M1. Three check titles are 8303b2ad's (medium)

**Fixed** on task-r3-sch-files: 201f2183

25ec2335 dropped the suffix " (lower/upper case difference only)" from three titles. `erc_checks.json:227` and its two neighbours still carry it:
- KiCad's `_HKI( "Labels are similar")`;
- "Power pins are similar";
- "Power pin and label are similar".

These titles show on the Severity page and in every menu label, so one row can mix both wordings.

### D3-M2. On 25ec2335, Ignore for `pin_to_pin` no longer silences pin-to-pin errors (medium)

**Not a bug** (as KiCad reads the project): 25ec2335 persists only the warning slot, so KiCad itself reports the errors again after reopening the project and kicad-cli always does; only its in-session menu state differs. Documented in PARITY-sch section 45

**What differs.**
- **In 8303b2ad**, `GetSeverity` made both pin-to-pin codes follow the warning slot: `if( m_ERCSeverities.at( ERCE_PIN_TO_PIN_WARNING ) == RPT_SEVERITY_IGNORE ) return RPT_SEVERITY_IGNORE;`.
- **In 25ec2335** the two slots are separate:
  - `m_ERCSeverities[ERCE_PIN_TO_PIN_ERROR] = RPT_SEVERITY_ERROR` is set on its own;
  - `testCapturedPinToPin` keeps a finding when `aSettings.IsTestEnabled( ercErrorCode( pinError ) )`;
  - the project key `"pin_to_pin"` sets only the warning slot.
- **KiCad's menu** sets both slots: `DIALOG_ERC::OnERCItemRClick` `ID_SET_SEVERITY_TO_IGNORE` then runs `settings.SetSeverity( ERCE_PIN_TO_PIN_WARNING, RPT_SEVERITY_IGNORE )`, and the error code too, in memory.
- **Ours** writes `"pin_to_pin": "ignore"` and reruns kicad-cli, so the errors come back while the Ignored Tests tab lists `pin_to_pin`.

**How to see it.** Confirmed with kicad-cli, in `scratch/p2p/`: QA `erc_multiple_pin_to_pin` gives `pin_to_pin` error x2 both without and with `rule_severities.pin_to_pin = "ignore"`. PARITY-sch section 9 and `erc_checks.rs` ("Ignore or from the map") describe 8303b2ad.

### D3-M3. Placing a symbol with sheet number x 100 / x 1000 always numbers as page 1 (medium; part of the 96cc869b fix)

**Fixed** on task-r3-sch-files: d9fb4d14

- **Ours:** `placementMinRef( algo, start, sheetNumber = 1 )` (`nextReference.ts`) is called with no sheet number by both callers: `SchematicView.tsx:768` and `useActionRunner.ts:2276` (Repeat).
- **KiCad:** `PlaceSymbol` runs `ReannotateByOptions( .., &hierarchy )`, which does `ref.SetSheetNumber( path->GetPageNumberAsInt() )`, so `minRefId = m_sheetNum * aSheetIntervalId + 1`.
- **Example:** a resistor placed on page 2 with X 100 gets R201 in KiCad and R101 in ours.
- **Why the tests pass:** the unit test passes because it gives the sheet number by hand.

**Already known?** No. The commit claims R201.

### D3-M4. A placed PWR_FLAG is named `#PWR0n`; KiCad names it `#FLG0n` (medium)

**Fixed** on task-r3-sch-files: d9fb4d14 (also the built-in PWR_FLAG said #PWR)

- **Ours:** `add_power_symbol` (`crates/ops/src/lib.rs:5103-5106`, cd6e6bf2) formats `#PWR0{n}` and counts only `#PWR` ids. `PowerSymbolDialog.tsx:14` offers `power:PWR_FLAG` through this same command.
- **KiCad:** numbers the library's prefix, `#FLG` (`FindFirstUnusedReference`, `formatRefStr` "0%d"), so the flag is `#FLG01`.
- **Our own generator** already writes `#FLG{n:02}` (`engine/src/flat.rs:233`, `hier/mod.rs:213`).

Test to write: `add_power_symbol` of `power:PWR_FLAG` gives `#FLG01`.

### D3-M5. A placed power symbol ignores the method, the start number, the other placements of a shared screen and the per-path references (medium; the rest of r2 D-A2)

**Fixed** on task-r3-sch-files: d9fb4d14 (method and start number; left: a shared screen still gives one reference, power symbols have no per-path reference in the model)

- **Ours:** cd6e6bf2 takes the first free number over the power-symbol ids of every screen.
- **KiCad's `PlaceSymbol` `annotate`** (`sch_drawing_tools.cpp:220-246`) does three things ours does not:
  - it calls `ReannotateByOptions( m_AnnotateSortOrder, m_AnnotateMethod, m_AnnotateStartNum, existingRefs, false, &hierarchy )`, so the method and the start number count;
  - it loops over `FindAllSheetsForScreen`, so a screen shown twice gives `#PWR01` and `#PWR02`;
  - it fills `existingRefs` with `GetSymbols( .., SYMBOL_FILTER_ALL )`, so every path's references count as taken.
- **Example:** with X 100, a GND on page 2 is `#PWR0201` in KiCad and `#PWR01` here.

**Already known?** Partly: r2 D-C3 (open) is the same gap for Paste.

### D3-M6. The paste anchor is still off when a KiCad clipboard's segments meet end to end (medium; the rest of r2 D-C2)

**Fixed** on task-r3-sch-files: 70bddba8

- **Ours:** `join_bends` (`kicad/src/sch_clipboard.rs:133-185`) reverses a segment it joins at its end (`pts.extend(wires[j].pts.iter().rev().skip(1))`), and only then does `top_left_anchor` (`sch_clipboard_api.rs:197`, `model/src/sch_clipboard.rs:395`) look at the segment starts.
- **KiCad:** `GetTopLeftItem` uses each `SCH_LINE`'s stored start.
- **Example:** two wires, `(xy 20 10) (xy 10 10)` and `(xy 10 20) (xy 10 10)`.
  - KiCad anchors at (10,20).
  - Ours joins them into (20,10)→(10,10)→(10,20) and anchors at (10,10), so the paste lands 10 mm off.

Our own copies are not affected.

### D3-L1. Annotate's Selection scope recurses into sheets inside a selected group (low)

**Fixed** on task-r3-sch-files: 5aea27d7

`annotateSelectionOf` (96cc869b) takes sheets from `leafIds`. KiCad's `AnnotateSymbols` takes sheets from the top-level selected items only (`for( EDA_ITEM* item : selection ) if( item->Type() == SCH_SHEET_T )`).

### D3-L2. The D-C9 fix follows 8303b2ad; 25ec2335 is back to the full lib id (low, version note)

**Fixed** on task-r3-sch-files: 5aea27d7 (follows 25ec2335: CompareLibId)

25ec2335 `refdes_tracker.cpp::areUnitsAvailable` and `sch_reference_list.cpp:229` and `:559` use `CompareLibId`, while 4d5db9e6 moved ours to `CompareLibName`'s item name. 25ec2335 `mapExistingAnnotation` also uses `SYMBOL_FILTER_NON_POWER`.

### D3-L3. Import drops the whole pin map when one cell is bad (low)

**Fixed** on task-r3-sch-files: 5aea27d7

`import_kicad.rs:389-391` and `pcb.rs:871` keep the map only if all 144 cells are `<= 2`. KiCad's loader (25ec2335 `erc_settings.cpp:218-243`) keeps every good cell (`val >= 0 && val <= PIN_ERROR::UNCONNECTED`, which is 0..3) and leaves a bad one at its default.

### D3-L4. Legacy duplicate exclusions with two comments: KiCad's order is sorted, ours is the file's (low)

**Fixed** on task-r3-sch-files: 5aea27d7

25ec2335 keeps `std::set<std::pair<wxString, wxString>>` and never replaces a non-empty comment. So for `[[t,"b"],[t,"a"]]` KiCad keeps "a", and ours keeps "b" (`erc_exclusions.rs:363-367`).

### D3-L5. The marker path rules are still the old engine's (low, unconfirmed)

**Fixed** on task-r3-sch-files: 5aea27d7 (the six non-connectivity rules; footprint_link_issues probed on kicad-cli)

- **Ours:** `erc_exclusions.rs::rule` gives no paths to:
  - `lib_symbol_issues`, `lib_symbol_mismatch`;
  - `footprint_link_issues`;
  - `simulation_model_issue`;
  - `duplicate_sheet_names`;
  - `undefined_netclass`.
- **25ec2335** sets paths for three of them:
  - `TestLibSymbolIssues` at `erc.cpp:2989-2990`;
  - `TestFootprintLinkIssues`;
  - `TestDuplicateSheetNames` at `:400-401`.
- **Why nothing breaks today:** our exclusions still match through `ResolveERCExclusions`' fallback `findPathless`.
- **Stale doc:** the module's doc (`connection_graph.cpp` line numbers, "probed against 10.99.0-5334") is out of date.

### Checked and found to match (slice D)

- **d06ab0a8:** the path's reference in `saveSymbol( aForClipboard, aRelativePath )`.
- **3b1f43ab:** n-1 starts per polyline, connectable items first, left then top (D3-M6 is the exception).
- **cd6e6bf2:** `formatRefStr` "0%d"; the first free number over the hierarchy with `m_reuseRefDes( true )`.
- **96cc869b:**
  - the zoom speed is 5 on macOS and 1 elsewhere;
  - `placementMinRef`'s formulas;
  - a selected group's symbols are taken recursively.
- **4d5db9e6:**
  - "Pasting a long text text string may be very slow.  Do you want to continue?" is word for word in both versions, counted in bytes;
  - a copied power symbol is `(in_bom yes)`;
  - three library checks;
  - the auto-pan border truncation (`wx_view_controls.cpp:1103-1121`);
  - "Clear net assignments" is shown (only `pcb_control.cpp:1230` hides it).
- **3e988e20:** the revert is right (`SetSeverities` sorts only when it refilters).
- **45f4c161:** no comment, then a comment.
- **5d66ea32:**
  - the 46 known keys;
  - the 12 x 12 map (`ELECTRICAL_PINTYPES_TOTAL`);
  - the grid in mils, clamped the same way;
  - the library-link checks stay ignored.
- **Unchanged from 8303b2ad to 25ec2335:** `m_defaultPinMap` and the ERC dialog's strings.

## Summary

| ID | Finding | Severity | Confirmed | Known? |
|---|---|---|---|---|
| A3-H1 | A pin common to all units is traced at the last unit only; a wire to another unit's copy connects nothing | high | run (c3, c3b) | no (exposed by P25-2's fix) |
| A3-H2 | A bus sheet pin finds parent members by the old engine's promotion, not by `drivesRecord` | high | run (c5, c5d) | no |
| A3-H3 | After an edit, an undriven net keeps its old name (`nets::follow`), so `design.nets` differs from KiCad's name of the derived schematic | high | run (f1, serve + export) | no |
| B3-H1 | A sheet file outside the root's folder is written outside `.kicad/`; a run overwrote the source sheet | high | run | no |
| C3-H1 | Deleting a junction leaves the wires split; KiCad merges them, so a crossing or T stays connected in ours | high | code | no (PARITY says identical) |
| D3-H1 | 9 ERC checks of kicad-cli 25ec2335 are unknown: import drops their severities, Setup refuses them | high | run | no |
| A3-M1 | A `#`-reference ordinary symbol claims its net's name | medium | run (c1) | no |
| A3-M2 | A vector on a group bus is aligned by name; 25ec2335 aligns by index | medium | run (c4) | no |
| A3-M3 | Name collision holder ignores claim strength and `CLAIM_LESS` | medium | run (c7) | in part |
| A3-M4 | Highlight Net on a label uses its text, not its net; labels lit by text | medium | code | no |
| B3-M1 | `Sheetfile` / `${SHEETFILE}` show the root-relative key | medium | run + code | no |
| B3-M2 | A sheet's user fields are lost by Paste, Duplicate and Import Sheet | medium | code | no |
| B3-M3 | The studio's shown text does not resolve sheet user fields | medium | code | no |
| B3-M4 | A sheet with a missing file is dropped from the walk; pages collide | medium | run | no |
| B3-M5 | Another project's page instance overrides this one's | medium | run | in part |
| B3-M6 | A self-placing sheet file is walked 32 levels deep | medium | code | no |
| C3-M1 | No commit-time `CleanUp` after Delete, `J` or placing items | medium | code | no |
| C3-M2 | `J` does not break wires and does no `CleanUp` | medium | code | no |
| C3-M3 | The wire tool finishes only on a pin; a bus finishes on a pin | medium | code | in part |
| C3-M4 | Clean-up does not merge solid with default strokes | medium | code | no |
| D3-M1 | Three `similar_*` titles are 8303b2ad's | medium | source | no |
| D3-M2 | `pin_to_pin` Ignore does not ignore errors on 25ec2335 | medium | run | no |
| D3-M3 | Placement with sheet x 100 / x 1000 numbers as page 1 | medium | code | no |
| D3-M4 | A placed PWR_FLAG is `#PWR0n`, not `#FLG0n` | medium | code | no |
| D3-M5 | Power placement ignores method, start, shared placements, per-path refs | medium | code | in part (D-C3) |
| D3-M6 | Paste anchor off when KiCad segments meet end to end | medium | code | no |
| A3-L1 | An `@{..}` error: KiCad drops the pin's net, ours keeps the raw text | low | run | no |
| A3-L2 | PARITY-pcb:2112 and the tracer's doc still cite the old engine | low | doc | no |
| B3-L1 | Page whitespace not stripped | low | code | no |
| B3-L2 | A sheet's own `(page "")` not read as `#` | low | code | no |
| B3-L3 | A deleted sheet's fields and pages stay behind | low | unconfirmed | no |
| C3-L1 | `finishSegments` also runs for Break and Repeat; a bus skips the trim; stale doc | low | code | no |
| C3-L2 | Align cleans up only the moved items; errors when nothing moves | low | code | no |
| C3-L3 | Sheet-pin drag special case recorded past a junction | low | unconfirmed | no |
| C3-L4 | Autoplace's power test is the library flag, not `#` | low | code | no |
| C3-L5 | PARITY-sch.md:191 stale | low | doc | no |
| D3-L1 | Selection scope recurses into sheets inside a group | low | code | no |
| D3-L2 | D-C9 fix is 8303b2ad's; 25ec2335 is back to `CompareLibId` | low | source | no |
| D3-L3 | Pin map dropped whole for one bad cell | low | code | no |
| D3-L4 | Legacy duplicate exclusions: sorted vs file order | low | code | no |
| D3-L5 | Marker path rules are the old engine's (the fallback saves them) | low | unconfirmed | in part |

Counts: **high 6, medium 20, low 15**.
