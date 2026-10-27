# Round 4 findings: the schematic editor and its files (`r4-schematic`, 2026-10-28)

**Scope.** GAPS items 1, 4, 6, 12, 20 and the ERC half of 11: `crates/engine` (the tracer), `crates/kicad`'s `.kicad_sch` reader and writer, `crates/ops` `sch_*`, `crates/cli`'s trace / import / ERC glue, the studio's schematic code, `web/studio/PARITY-sch.md`. Main is at `01ffc984`, read only.

**Priority.**
- (a) The code merged in round 3 (PARITY-sch sections 45-46; merges f3c7f17c `task-r2-leftovers` and 5a9ff245 `task-r3-sch-files`): local power symbols, a unit per sheet path, the new engine's naming (A3), the junction clean-up and `J`, sheets outside the root's folder, the ERC table.
- (b) Ported or identical rows no round had checked against kicad-cli: the island rules and pin facts of 25ec2335's connectivity engine, the ERC marker rules left open by section 45.

**KiCad sources.** Connectivity and ERC against 25ec2335 (`~/src/kicad-src-25ec2335`, the box's kicad-cli 10.99, the user's choice); files and tools against 8303b2ad (`$KICAD_SRC_DIR`).

**Binary.** A fresh build of main `01ffc984` from a `git archive` copy: `/tmp/findings-r4-schematic.HMLq/target/debug/eda`. Every run used scratch copies under `/tmp/findings-r4-schematic.HMLq/`; the one studio server (port 8841, slice C) is stopped.

**Harness.** Round 3's (`/tmp/findings-r3-schematic.jjpH/gen/`) is copied and extended in `/tmp/findings-r4-schematic.HMLq/gen/`: `run4.sh <case>` prints kicad-cli's netlist of the original, `eda import-kicad`'s `design.json` nets, and kicad-cli's netlist of the derived file (`eda board erc -C out`). New generators: `cases4.py` (local power), `cases5.py` (islands, jumper / duplicate / NC pins), `cases6.py` (naming), `cases7.py` (stacked numbers), `cases8.py` (alternates), `cases4c.py` (slice C's T cases). Slices B/D: `slice-BD/rt.sh`. Slice C: `slice-C/cmd.sh`.

**Already known?** Each finding was checked against `docs/parity/GAPS.md` and `docs/parity/findings/` (rounds 1-3).

**Read first.**
- The round-3 connectivity fixes hold: all of round 3's cases now match kicad-cli except the documented leftovers, and the local power symbols of an imported file match in six cases. But "local" lives only in an import-time set, so a local power symbol placed or pasted in the editor becomes global (A4-H1).
- 25ec2335's pin facts are not ported: jumper pin groups and "duplicate pins are jumpers" (A4-H2), two pins of one number (A4-H3, the writer also stacks them on one place), stacked numbers `[1,15,38,39]` (A4-H4, already used by 30 installed library symbols) and `PT_NC` pins (A4-M1).
- The importer drops a symbol whose cache entry is its `(lib_name ..)` (B4-H1; 8 QA files, confirmed on `ground_pin_test_ok`).
- 8bb401b6's per-verb clean-up makes the studio's Delete of a window over a T fail as a whole (C4-H1).
- The ERC table (201f2183) matches 25ec2335 check for check, and section 45's open item (the connectivity marker rules) can be closed.

## Slice A: schematic connectivity (the tracer, `eda import-kicad`, the derived files)

Scope:
- **(a) Round-3 merges:** 22b21f9e (local power symbols), b96fd002 (a unit per sheet path), 8f93aeee (A3: common pins per unit, `drivesRecord`, `#` references, `Align`, the holder of a shared name), 1e96d3ea (A3-H3, `nets::follow`).
- **(b) Ported rows no round had run against kicad-cli:** the island rules of 25ec2335 `conn_islands.cpp` (T without a junction, a pin on a wire's interior, crossings, labels on a wire's interior, no-connect flags, bus entries, a wire on a bus), the pin facts of `conn_facts.cpp` (jumper pin groups, duplicate pin numbers, stacked pin numbers, `PT_NC` pins, alternate pin functions), and the claim edges of `conn_claims.cpp::BuildIslandRecord` for power symbols and global / hierarchical labels.

KiCad: 25ec2335 `eeschema/connectivity/` (the user's choice) and `sch_pin.cpp`; 8303b2ad where it agrees (`SCH_PIN::ConnectionPropagatesTo`).

**Harness.** `/tmp/findings-r4-schematic.HMLq/gen/` holds round 3's generator (`gen.py`) and my cases (`cases4.py` local power, `cases5.py` islands and pin facts, `cases6.py` naming, `cases7.py` stacked numbers, `cases8.py` alternates). `gen/run4.sh <case dir>` prints three net lists: kicad-cli's netlist of the original file, the nets `eda import-kicad` stores in `design.json`, and kicad-cli's netlist of the derived `out/.kicad/board.kicad_sch` written by `eda board erc -C out`. Binary: a fresh build of main `01ffc984` (`/tmp/findings-r4-schematic.HMLq/target/debug/eda`). Round 3's 24 cases were re-run on it: all match kicad-cli except the known c1/c2/t8 (kicad-cli's netlist leaves out `#` and off-board symbols; the names match), c3/c3b (A3-H1's documented "Differs": a common pin listed once) and m1 (A3-L1, open).

### A4-H1. A local power symbol placed, pasted or imported into the editor is traced and written as a global one (high)

**What differs.** The tracer takes "local" only from `SchExtras::local_power`, a per-screen set of library ids that only `eda import-kicad` fills:
- `crates/engine/src/nets.rs:452`: `if sch.extras.local_power.contains(&ps.lib_id) { .. LOCAL_POWER_PIN .. }`;
- `crates/kicad/src/lib.rs:405`: the writer writes `(power local)` only when `sch.extras.local_power.contains(lib_id)`;
- `crates/kicad/src/sch_import.rs:687-691` is the only place that fills it.

No verb in `crates/ops` writes `local_power` (`grep -rn local_power crates/ops` finds nothing). So:
- `Board::add_power_symbol` (`crates/ops/src/lib.rs:5241-5263`) pushes the `PowerSymbol` and leaves the set alone: a power symbol from a library whose `LibrarySymbol::power_local` is true (`crates/model/src/ir.rs:4507`, read from `(power local)` by `symbol_import.rs:266`, settable in Symbol Properties) is a global power pin;
- Paste: `sch_clipboard.rs:86` carefully keeps the clipboard's local copy under a name of its own (`power_kind`, `paste_keeps_the_clipboards_local_power_symbol` asserts `published(..).power_local`), but the pasted `PowerSymbol`'s new lib id is not in the screen's `local_power`, so the tracer gives it `GLOBAL_POWER_PIN` and the writer writes `(power)`. A local power symbol cut on one sheet and pasted on another loses its locality the same way;
- Import Sheet (`sch_sheets.rs:186-200`) renames a colliding lib id through `lib_name`, and the copied screen's `local_power` keeps the old id.

KiCad decides from the library symbol of the placed item, not from a separate list (25ec2335):

```cpp
// sch_pin.cpp
bool SCH_PIN::IsLocalPower() const
{
    return GetType() == ELECTRICAL_PINTYPE::PT_POWER_IN
           && GetParentSymbol()->IsLocalPower();
}
// conn_claims.cpp::PrepareInstanceClaims
priority = pin->globalPower  ? PRIORITY::GLOBAL_POWER_PIN
           : pin->localPower ? PRIORITY::LOCAL_POWER_PIN
                             : PRIORITY::PIN;
// conn_claims.cpp::BuildIslandRecord
case PRIORITY::GLOBAL:
case PRIORITY::GLOBAL_POWER_PIN:
    result.edges.push_back( { SCOPE::GLOBAL, 0, claim.name } );
    [[fallthrough]];
case PRIORITY::LOCAL_LABEL:
case PRIORITY::LOCAL_POWER_PIN:
    result.edges.push_back( { SCOPE::SHEET, aInputs.instance, claim.name } );
```

So in KiCad a local `+48V` placed on two sheets is two nets (`/+48V`, `/A/+48V`); in the studio, once placed or pasted, it is one global `+48V` net, in `design.nets` and in the derived file kicad-cli reads (`(power)`).

The two sources of truth also drift the other way: a library symbol turned global in Symbol Properties (`library_editors.rs:53-57`) stays local on every screen whose `local_power` lists it.

**How to see it.**
- `crates/ops/src/sch_clipboard_tests.rs`, new test `a_pasted_local_power_symbol_is_a_net_of_its_sheet`: paste `lvcc` (as in `paste_keeps_the_clipboards_local_power_symbol`) onto the root and onto a child sheet, each on a resistor pin; trace; assert two nets `/VCC` and `/<sheet>/VCC`, and that the written `.kicad_sch` has `(power local)` for the pasted lib id. Today: one net `VCC`, `(power)`.
- The same for `Cmd::AddPowerSymbol` with a design library symbol whose `power_local` is true.
- Fix direction: decide "local" from the placed lib id's library symbol (`power && power_local`) wherever `local_power` is read, or keep `local_power` in step in every verb that adds or renames a power symbol.

**Already known?** No. PARITY-sch section 46 says "A local power symbol: **ported** ... written back `(power local)`"; it holds for an imported file only (my cases p1-p6 below all match kicad-cli).

### A4-H2. A symbol's jumper pin groups and "duplicate pin numbers are jumpers" join nothing, and the derived file drops them (high)

**What differs.** 25ec2335 joins the pins of a jumper group into one island, and with `duplicate_pin_numbers_are_jumpers` every pin of one number:

```cpp
// conn_facts.cpp (symbol pins)
// Unjumpered duplicates join only when stacked, so ERC can report pins wired to different nets
const bool joinApart = library && library->GetDuplicatePinNumbersAreJumpers();
...
const VECTOR2I contact = joinApart ? VECTOR2I() : pin->GetPosition();
const auto key = std::make_tuple( pin->GetNumber(), contact.x, contact.y );
...
if( hasJumperGroups )
{
    for( const JUMPER_GROUP& group : library->JumperPinGroups().GetAll() )
// conn_islands.cpp::BuildScreenIslands
// Pins merged by number are stacked or jumpered, so each touches the others wherever they sit
...
for( const KIID& other : fact.jumperedWith )
    ...
        sets.Unite( index, found->second );
```

Ours: the library side reads and writes both (`crates/model/src/lib_keep.rs:28-33`, `symbol_import.rs:291-307`, `symbol_lib.rs:593-594`), but nothing in the tracer (`crates/engine/src/nets.rs`, `crates/cli/src/board.rs::trace_design`) looks at them (`grep -rn jumper crates/engine/src crates/cli/src/board.rs` finds nothing), and the schematic writer's library cache (`crates/kicad/src/lib.rs::write_lib_symbol_entry`) writes neither token, so the derived file loses the jumper.

**How to see it.** `/tmp/findings-r4-schematic.HMLq/cases5/j1` (a 3-pin box `J1` with `(jumper_pin_groups ("1" "2"))`, R1.1 wired to J1.1 and R2.1 to J1.2):
- kicad-cli: `Net-(J1-A) [J1.1, J1.2, R1.1, R2.1]`;
- ours and the derived file: `Net-(J1-A) [J1.1, R1.1]`, `Net-(J1-B) [J1.2, R2.1]`.

`cases5/j2` (`(duplicate_pin_numbers_are_jumpers yes)`, two pins `1` at two places): kicad-cli `Net-(J1-A) [J1.1, R1.1, R2.1]`; ours and derived `Net-(J1-A) [J1.1, R1.1]` and R2.1 alone.

Test to write: `crates/cli/src/sch_hierarchy_tests.rs::a_symbols_jumper_pins_are_one_net_as_kicad_cli_reads_them`, the two files above as fixtures, asserting `design.nets` and kicad-cli's netlist of the derived file equal kicad-cli's netlist of the original.

**Already known?** No. GAPS item 19 lists the library editors' "jumper pins" as fixed (read and written in a `.kicad_sym`); the schematic side is not mentioned.

### A4-H3. Two pins of one number at two places: one of them is lost by the tracer, and the writer stacks both on the first (high)

**What differs.** KiCad keeps every pin object: two pins `1` at two places are two items (the `key` above has the position when `joinApart` is false), so each joins what touches it, and the netlist lists the pin on both nets (`Net-(J1-A)` and `Net-(J1-A)_1`).

Ours keys pins by `REF.PIN`, so the second overwrites the first:
- `crates/cli/src/board.rs:600` and `:622`: `pin_world.insert(pin_key(..), at)`;
- the writer looks the pin up by number, `crates/kicad/src/lib.rs:1449`: `let real_pin = resolved.and_then(|s| s.pin_by_number(&pin.number));` (`pin_by_number` is `find`, the first match, `crates/model/src/symbol.rs:257`), so both pins `1` are written at the first one's place, and the instance gets two `(pin "1" (uuid ..))` with the same uuid.

The derived file kicad-cli reads has the second pin moved: whatever was wired to it is dangling.

**How to see it.** `cases5/j3` (a box with two pins `1`, no jumper flag):
- kicad-cli: `Net-(J1-A) [J1.1, R1.1]`, `Net-(J1-A)_1 [J1.1, R2.1]`;
- ours: `Net-(J1-A) [J1.1, R1.1]`, R2.1 `unconnected-(R2-Pad1)`;
- derived: the same as ours, and `out/.kicad/board.kicad_sch` has both `(pin passive line (at 0 1.27 270) .. (number "1" ..))` at one place.

Test: `crates/kicad/tests/` `two_pins_of_one_number_keep_their_places_in_the_written_cache`: write the case and assert two pins `1` at the library's two points (and, under `EDA_SLOW_TESTS`, kicad-cli's netlist as above).

**Already known?** No. GAPS item 36's "pads that share a number but not a net" is the board side.

### A4-H4. Stacked pin numbers (`[1,15,38,39]`) are taken as one pin of that literal number (high)

**What differs.** 25ec2335 reads a pin number in brackets as the pins it stacks (`SCH_PIN::GetStackedPinNumbers` -> `ExpandStackedPinNotation`), puts each stacked number in `numbers` (`conn_facts.cpp`: `for( const wxString& number : pin->GetStackedPinNumbers() ) numbers[number].push_back( index );`), names the net by the smallest (`GetSmallestStackedPadNumber`), and the netlist lists every pad. The installed library already uses it: 30 symbols in `/usr/share/kicad-nightly/symbols` (`Interface_USB` 4, `Driver_Motor` 6, `Regulator_Switching` 4, `Connector` 2, ...; e.g. `(number "[1,15,38,39]"`).

Ours: the tracer keys the pin `REF.[1-2]` and stores it in `design.nets` as such; the board has no pad of that name, so pads 1 and 2 get no net. `eda_model::fp_units::expand_stacked_pin_notation` (`crates/model/src/fp_units.rs:36`) already ports `ExpandStackedPinNotation`, but only the footprint-units code uses it.

**How to see it.** `/tmp/findings-r4-schematic.HMLq/cases7/s1` (a box `J1` with pins `[1-2]` and `3`, `[1-2]` wired to R1.1):
- kicad-cli (original and derived): `Net-(J1-Pad1) [J1.1, J1.2, R1.1]`;
- ours: `Net-(J1-Pad[1-2]) [J1.[1-2], R1.1]`.

Test: `nets::tests::a_stacked_pin_number_is_each_of_its_pads` (pins `J1.1`, `J1.2` on R1.1's net, named `Net-(J1-Pad1)`); and an `import-kicad` test with `Interface_USB` placed from the installed library.

**Already known?** No.

### A4-M1. A pin of type "no connect" (`PT_NC`) joins the wire on it; KiCad never connects such a pin (medium)

**What differs.** KiCad gives a `PT_NC` pin a contact of kind `NC`, which joins nothing (25ec2335 `conn_islands.cpp::BuildScreenIslands`):

```cpp
addPort( index, pin.id, pin.position, pin.type == ELECTRICAL_PINTYPE::PT_NC ? NC : ANCHOR, 1 );
...
if( a.kind == NC || b.kind == NC )
{
    ncContacts.emplace_back( pair.first, pair.second );
    continue;
}
```

and 8303b2ad agrees (`sch_pin.cpp::ConnectionPropagatesTo`: `return GetType() != ELECTRICAL_PINTYPE::PT_NC;`). Ours puts the pin at its point like any other (`board.rs::trace_design`'s `pins_of`; `nets.rs` uses `nc_pins` only for the `unconnected-(` name).

**How to see it.** `cases5/n1` (U1 pin 1 of type `no_connect`, wired to R1.1):
- kicad-cli (original and derived): `unconnected-(R1-Pad1) [R1.1]`, `unconnected-(U1-NC-Pad1) [U1.1]`;
- ours: `Net-(R1-Pad1) [R1.1, U1.1]`, so the board would connect the pads.

It is an ERC error in KiCad ("pin_not_connected"/`no_connect_connected`), hence medium, but `design.nets` and the board disagree with KiCad. Test: `nets::tests::a_no_connect_type_pin_joins_nothing`.

**Already known?** No.

### A4-M2. A bus wire entry between two wires connects them in KiCad; the tracer ignores bus entries (medium)

**What differs.** In 25ec2335 a wire entry's two ends are `ENTRY` contacts, and `PROPAGATES[ENTRY][WIRE]` is true (`conn_islands.cpp`, the table's row `{ true, true, false, false, false, false, false }`); only an end that sits on a bus becomes `BUS_SIDE`, which joins nothing. So a bus entry used as a 45-degree jog between two wires joins them. Ours: `crates/engine/src/nets.rs:36` "Not ported: bus entries take no part (KiCad's do not either: a wire is on a member by its label)". The second half is right for an entry on a bus, not for one between wires.

**How to see it.** `cases5/g6` (R1.1's wire ends at the entry's corner, the entry's other end starts R3.1's wire, no bus anywhere): kicad-cli `Net-(R1-Pad1) [R1.1, R3.1]` (original and derived); ours two `unconnected-(..)` nets. Test: `nets::tests::a_bus_entry_between_two_wires_joins_them` (and an entry with one end on a bus joins nothing).

Severity: wrong connectivity, but only for this drawing; medium.

**Already known?** No; the doc comment claims KiCad agrees.

### A4-M3. A unit per sheet path is lost when the symbol is renamed, and stays behind when it is deleted (medium)

**What differs.** `SchExtras::path_units` is keyed by `field_key(&sym.id, sym.unit)` (`crates/model/src/sch_extras.rs:107-116`, read by `sheet_list::unit_under`, `crates/model/src/sheet_list.rs:402-407`). The keys kept per placed unit are moved on a rename and dropped on a delete for `field_layout`, `fields_autoplaced`, `body_styles` and `pin_swaps` (`crates/ops/src/sch_fields.rs:507-534` `rename_symbol_keys` / `drop_symbol_keys`, `crates/ops/src/sch_annotate.rs:596-617`, `crates/ops/src/sheet_units.rs:181-190`), but not for `path_units` (`grep -rn path_units crates/ops` finds nothing outside tests). After Annotate or an Edit Reference renames `U1` to `U5` on a shared screen whose path `/b` draws unit B, `/b` falls back to unit A: the tracer takes unit A's pins there (`board.rs:593-612`) and the writer writes `(unit 1)` for `/b`. A symbol later renamed to `U1` inherits the stale entry.

KiCad keeps the unit in the instance itself (`SCH_SYMBOL_INSTANCE::m_Unit`, moved with the symbol by `SCH_SYMBOL::SetRef` / `AddHierarchicalReference`), so a rename never changes which unit a path draws.

**How to see it.** `crates/ops/src/sch_hierarchy_tests.rs`, `a_renamed_symbol_keeps_its_unit_on_each_path`: import `crates/cli/tests/fixtures/per_path_unit`, apply `Cmd::RenameSymbol { id: "U1", new_id: "U5" }` (the path of `Board::rename_symbol`, `crates/ops/src/lib.rs:5502-5570`; Annotate goes through `sch_annotate.rs:596-617`), assert `unit_under(.., ["b"]) == 2` and the traced nets unchanged.

**Already known?** No (PARITY-sch section 46's "Differs" is about an edit of the unit, not of the reference).

### Checked and found to match (slice A)

Each by reading 25ec2335 and by running the case through kicad-cli (original and derived) and `eda import-kicad`:

- **22b21f9e, local power on import** (cases4 p1-p6): a local `+48V` on the root and on a sheet (`/+48V`, `/A/+48V`); with a global label `+48V` on the same sheet (one net `+48V` with the other sheet's global label); under a hierarchical label whose sheet pin a local label names (`/A/+48V` wins by priority 5 over 4); with a global power `+48V` on the same sheet (one global net); on a screen shown twice (`/A/+48V`, `/B/+48V`); with a local label of its name (one net `/+48V`). The claim's depth and path (`unscoped` is false for `LOCAL_POWER_PIN`), `Driver::new`'s `full` with the path, and the `(power local)` the writer writes all match.
- `SCH_PIN::IsGlobalPower` / `IsLocalPower` against `naming.is_global_power` and `power_pins_of` (a local power symbol's invisible pin is never a legacy global one: `power_pins_of` skips `lib.power`).
- **b96fd002** read against `GetUnitSelection( aSheet )` and `conn_islands.cpp`'s unit filter (`pin.unit != 0 && unit != 0 && pin.unit != unit`): `unit_under` in `pins_of`, `power_pins_of` and the unit letters of `trace_kicad_names`.
- **8f93aeee** against `conn_publish.cpp::AssignNames`: `preferred` (`Strong()` = priority >= `HIER_LABEL`, then `CLAIM_LESS` among strong ones), the bucket per name; the remaining nets' order (`nodeLess`) is the documented "Differs". `canDrive &= DoNetList() && !GetExcludedFromBoard() && !reference.StartsWith("#")` applies to ordinary pins only, power pins keep it (the `member.globalPower || member.localPower` branch), as `quiet_pins` does. Round 3's c4, c5, c5b, c5d, c7, f1 now match kicad-cli.
- **1e96d3ea** `nets::follow`: an undriven changed net takes the tracer's (KiCad's) name; driven names first.
- Islands (cases5 g1-g5, g7-g9): a T with no junction joins (`SEG::Contains` on the interior); a pin on a wire's interior joins; crossing wires do not, a junction joins them; a label on a wire's interior names it; a no-connect flag on a wire's interior leaves the net whole and names it `unconnected-(..)`; a pin on a pin joins; a wire ending on a bus's interior joins nothing.
- Naming (cases6 h1-h12): a hierarchical label on the root (`/X`); a power symbol and a global label of one name on two sheets (one net); of two global labels the alphabetically first (`NameLess`); spaces, a leading space, `~{RST}` and `D{slash}E` in label text; a power symbol whose Value differs from its library name (`GetValue`); a hidden power input `AVDD` against a global label (joins) and a sheet's local label (apart); a sheet pin's net that a global label of its name elsewhere does not join.
- Alternate pin functions (cases8 a1): the net name keeps the library pin's name (`libPin->GetShownName()`), as kicad-cli does.

## Slices B and D: files, hierarchy, clipboard, Annotate, ERC data (`r4-schematic`, fork B+D)

**Scope.** Round-3 fixes merged into main: 179c5cf5, 733537a4, cda452b1, 48f86d0f (B3); bd21173b (r2 B-M2); 94a1fb37 (r2 B-L2, B-L4); 201f2183, 5aea27d7, d9fb4d14, 70bddba8 (D3); a9ef6e65 and d87cec9c (r2 D-C3, D-C4). Plus one unreviewed item that PARITY-sch §45 leaves open: the connectivity rows of `erc_exclusions::rule` compared line by line with 25ec2335's `ERC_MARKER_PLACER` calls.

**KiCad sources.** File I/O and tools against 8303b2ad. ERC and `RepairPageNumbers` against 25ec2335, which is the box's kicad-cli.

**Binary.** A fresh build of main `01ffc984`, from a `git archive` copy: `/tmp/findings-r4-schematic.HMLq/target/debug/eda`.

**Scratch runs.** Under `/tmp/findings-r4-schematic.HMLq/slice-BD/`, with `KICAD_CONFIG_HOME` pointing at `slice-BD/kcfg`. `slice-BD/rt.sh <src dir> <root> <name>` is round 3's harness pointed at the new binary. It runs `eda import-kicad`, then `eda board erc -C`, then kicad-cli's kicadxml netlist of both the original and the derived file.

### B4-H1. A symbol whose cache entry is its `(lib_name ..)` is dropped on import; one whose `lib_name` copy differs is drawn and traced from the `lib_id` entry (high)

**What differs.**
- Ours: `crates/kicad/src/sch_import.rs:183-186` resolves a placed symbol by its `lib_id` alone:
  ```rust
  let Some(resolved) = lib_table.get(lib_id) else {
      notes.unresolved_symbols += 1;
      continue;
  };
  ```
  `(lib_name ..)` is read only to work out Swap Pins' traded places (`:207-208`, `pin_swaps_between`).
  - A symbol written `(lib_name "Test_Symbol_GND_1") (lib_id "Test:Test_Symbol_GND")` is left out of the design when the file's `lib_symbols` holds only `"Test_Symbol_GND_1"`. Its pins, its nets and its reference all go.
  - The only trace is the note `1 symbol(s) had no library symbol in the file and were left out` (`sheet_file.rs:43`).
  - When both entries exist, the symbol takes the `lib_id` entry's graphics and pins. Only pin *places* that differ come through, as "swaps".
- KiCad: the parser keeps the cache name. `sch_io_kicad_sexpr_parser.cpp::parseSchematicSymbol`, `case T_lib_name`:
  ```cpp
  libName = FromUTF8();
  symbol->SetSchSymbolLibraryName( libName );
  ```
  Then `SCH_SCREEN::UpdateSymbolLinks` / `SCH_SYMBOL::GetSchSymbolLibraryName()` look the symbol up by that name ("the name used to look up the cached library symbol; the library id when empty"). So the symbol is drawn, netlisted and ERC-checked from its own `lib_name` copy.
  - KiCad writes such copies itself. `SCH_SCREEN::Append` gives a symbol a `<name>_<n>` copy when its library symbol differs from the one already cached under that name.

**How to see it.**
- QA `eeschema/ground_pin_test_ok.kicad_sch` (scratch `slice-BD/pw/`):
  - kicad-cli's netlist of the original has `<comp ref="U1">`;
  - the derived file's netlist has no `U1`;
  - `import.log` says 1 symbol was left out.
- 8 QA files have a `lib_name` with no `lib_id` cache entry: `issue16223` (11 symbols), `issue1768` (5), `spice_netlists/legacy_laser_driver` (4), `legacy_sallen_key` (2), `legacy_pspice`, `legacy_rectifier`, `netlists/legacy_power` and `ground_pin_test_ok`.

Test: `sch_import::tests::a_symbol_is_resolved_by_its_lib_name`.
- Input: a file with `(lib_symbols (symbol "R_1" ..pins 1 2..))` and one `(symbol (lib_name "R_1") (lib_id "Device:R") ..)`.
- Assert: `notes.unresolved_symbols == 0`, the design has the symbol, and its pins are `R_1`'s.
- A second case: both `Device:R` and `R_1` are cached, with different pin numbers. Assert the placed symbol has `R_1`'s numbers.

**Severity.** High: data loss and wrong connectivity on KiCad's own files.

**Already known?** No. GAPS item 1's `lib_name` note covers Swap Pins only.

### B4-M1. Save Sheet as Design Block and Save Current Sheet Copy As fail for a sheet above the root's folder (medium; regression from 179c5cf5)

**What differs.**
- 179c5cf5 makes `export_kicad_sch_tree` name a screen `sheet_file_in_work(key)`: `__up__/shared/amp.kicad_sch`, or `__root__/abs/..` for an absolute key (`crates/kicad/src/lib.rs:931`). Two callers still look the file up by the key (`../shared/amp.kicad_sch`):
  - **`crates/cli/src/design_block_api.rs:304-306` (`sheet_made`).** It takes `wanted = file` from `sheet_file_and_name` (`SheetInstance::file`, the key), then `files.into_iter().find(|(n, _)| *n == wanted)`. The result is the error "The file of the sheet (../shared/amp.kicad_sch) was not written."
  - **`web/studio/src/actions/schControlExports.ts:26` (`saveSheetCopy`).** It looks up `files.find((f) => f.name === sheet?.file)`, where `sheet.file` is `walk(..).file`, the key (`sch_control_api.rs:21`). The result is "This sheet has no file to copy."
- KiCad: both work for any sheet:
  - `SCH_DESIGN_BLOCK_CONTROL::SaveSheetAsDesignBlock` saves the sheet's screen;
  - `SCH_EDITOR_CONTROL::SaveCurrSheetCopyAs` calls `saveSchematicFile( curr_sheet, newFilename )`.

**How to see it.**
- Test: `design_block_api::tests::a_sheet_above_the_root_folder_saves_as_a_design_block`.
  - Input: import a root whose sheet's file is `../shared/amp.kicad_sch` (the fixture of `crates/kicad-engine/tests/sheet_outside.rs`).
  - Run `sheet_made(dir, "<sheet id>")`.
  - Assert `Ok` and that its text is the amp screen.
- The same lookup for the studio: compare `sheet_file_in_work(sheet.file)`, or have the API return the written name next to the key.

**Severity.** Medium: two menu actions are refused on a supported layout.

**Already known?** No; this is new in 179c5cf5.

### B4-M2. "Exclude from position files" is keyed by reference and does not follow Annotate or Paste (medium; new in 94a1fb37)

**What differs.**
- Ours: `SchExtras::excluded_from_pos_files` holds `field_key(reference, unit)`. It is read at `sch_import.rs:349` and written at `lib.rs:448`.
  - Annotate moves the other per-reference extras when it renames: `crates/ops/src/sch_annotate.rs:597-614` moves `field_layout`, `fields_autoplaced`, `body_styles` and `pin_swaps`. It does not move this set.
  - The same holds for `sch_clipboard.rs:318` (Paste carries `body_styles` only), `sheet_units.rs:188-196` and `sch_sheets.rs:720`.
  - So after a renumber the flag stays on the old reference: the symbol loses it, and whichever symbol later takes that reference gains it.
- KiCad: the flag is the symbol's own attribute (`SCH_SYMBOL::m_excludedFromPosFiles`, written by `saveSymbol`: `KICAD_FORMAT::FormatBool( m_out, "in_pos_files", !aSymbol->GetExcludedFromPosFiles() )`). It moves with the symbol through any annotation, paste or duplicate.

**How to see it.** Test: `sch_annotate_tests::annotate_keeps_exclude_from_position_files`.
- Input: import a file where `R5` (the only R) is `(in_pos_files no)`. Annotate Reset with first-free.
- Assert: `R1` is written `(in_pos_files no)`, and no other symbol is.
- Do the same for a Paste of `R5`: the pasted copy carries `(in_pos_files no)`, as KiCad's clipboard keeps the flag.

**Severity.** Medium: a wrong attribute lands in the file KiCad reads, and the position file changes.

**Already known?** No; r2 B-L2's fix covered only reading and writing.

### D4-M1. A symbol placed or pasted on a screen shown more than once numbers its later sheet paths with the defaults, not Schematic Setup's method and each path's page (medium)

**What differs.**
- Ours: `crates/ops/src/sch_sheets.rs:381` numbers the new symbols on every later path of a shared screen: `annotate_paste(&self.used_references(), &pasted, PasteMode::Unique, Default::default(), 1)`. That is first-free from the symbol's own number, with sheet number 1, whatever the project's method or start number. `number_later_placements` (`:295`) does the same for Import Sheet and Paste of sheets.
  - The first path's own reference comes from the page *in view* (D3-M3, `lib.rs` `page_number`), not the first path's page.
  - A screen on pages 2 and 3 with "sheet number x 100", placing `R` while viewing page 3:
    - ours: the symbol's own id (the first path) gets `R301`, and the page-3 path gets `R302`;
    - KiCad: `R201` on page 2 and `R301` on page 3.
- KiCad: `tools/sch_drawing_tools.cpp::PlaceSymbol`'s `annotate` lambda loops over every instance of the screen, sorted by page:
  ```cpp
  SCH_SHEET_LIST newInstances = hierarchy.FindAllSheetsForScreen( m_frame->GetCurrentSheet().LastScreen() );
  newInstances.SortByPageNumbers();
  ...
  for( SCH_SHEET_PATH& instance : newInstances )
  {
      SCH_REFERENCE newReference( symbol, instance );
      ...
          refs.ReannotateByOptions( (ANNOTATE_ORDER_T) schSettings.m_AnnotateSortOrder,
                                    (ANNOTATE_ALGO_T) schSettings.m_AnnotateMethod,
                                    schSettings.m_AnnotateStartNum, existingRefs, false,
                                    &hierarchy );
  ```
  `ReannotateByOptions` sets `ref.SetSheetNumber( path->GetPageNumberAsInt() )` for each path. `SCH_EDITOR_CONTROL::Paste` likewise loops over `sheetPathsForScreen` with `annotateAlgo` (`ReannotateDuplicates( existingRefs, annotateAlgo )`, or `ReannotateByOptions( annotateOrder, annotateAlgo, annotateStartNum, .. )`).

**How to see it.** Test: `sch_hierarchy_tests::a_symbol_placed_on_a_shared_screen_numbers_each_path_by_its_page`.
- Input: `power_design()` with the power screen placed twice, on pages 2 and 3. `AddSymbol` an `R` with x 100, viewing the page-3 path.
- Assert: `reference_under` is `R201` on the page-2 path and `R301` on the page-3 path.

**Severity.** Medium: wrong references under a non-default annotation setup.

**Already known?** No. D3-M5's leftover is about power symbols, which have no per-path references; §46's "Differs" covers a pasted sheet's contents only.

### D4-L1. `RepairPageNumbers` reserves "1" for the root, so a root on another page moves a sub-sheet's "1" (low)

**What differs.**
- Ours: `crates/model/src/sheet_list.rs:222-246` (`repair_page_numbers`):
  ```rust
  let root = "1".to_string();
  let reserved = std::iter::once(root.clone())...
  let mut assigned = std::iter::once(root).collect();
  ```
  It also skips the root in its "all empty" test (`entries.iter().skip(1)`).
- KiCad 25ec2335: `sch_sheet_path.cpp::SCH_SHEET_LIST::RepairPageNumbers` treats the root like any other path, with its own stored page:
  ```cpp
  for( SCH_SHEET_PATH& instance : *this ) { ... const wxString pageNumber = instance.GetPageNumber();
      // Keep the first sheet to claim a given page number.
      if( !pageNumber.IsEmpty() && assignedPageIds.insert( pageNumber ).second ) continue;
  ```
  `AllSheetPageNumbersEmpty` counts the root too.

**How to see it.** Scratch `slice-BD/pg_src`: the root is `(sheet_instances (path "/" (page "3")))`, sheet A is on page "1" and sheet B on "".
- `kicad-cli sch upgrade --force` (which runs `EESCHEMA_HELPERS::LoadSchematic` → `RepairPageNumbers`) keeps A on "1", B on "#" and the root on "3".
- Our derived file has A on "2" and the root on "1".

Test: `sheet_list::tests::a_root_on_page_3_leaves_a_sheets_page_1`.

**Severity.** Low: rare, and it rides on the root page being fixed at "1", which r2 B-L4 marks as left.

**Already known?** In part: B-L4's "the root's own page is always 1" is left. The renumbering of a real sub-sheet page is its new side effect, through bd21173b.

### D4-L2. An imported power symbol loses its own flags and its hand-placed Value position (low)

**What differs.**
- Ours: `PowerSymbol` (`crates/model/src/ir.rs:517-533`) has no flags. Since 94a1fb37, the writer puts `(exclude_from_sim no) (in_bom yes) (on_board yes) (dnp no)` on every power symbol (`lib.rs:497`), and no `in_pos_files`.
  - A KiCad power symbol with `(exclude_from_sim yes) (in_bom no) (on_board no)` comes back as `no / yes / yes`.
  - Its Value, placed by hand (no `fields_autoplaced`), is autoplaced again: `GND` at (135.89, 95.25) has Value at y 99.06 in the file and 100.33 in ours.
- KiCad: `saveSymbol` writes the symbol's own `GetExcludedFromSim()`, `GetExcludedFromBOM()`, `GetExcludedFromBoard()`, `GetExcludedFromPosFiles()` and `GetDNP()`, and each field's own position.

**How to see it.** Scratch `slice-BD/pw_src/root.kicad_sch` is QA `ground_pin_test_ok` with `#PWR01`'s flags flipped. The derived `slice-BD/pw/out/.kicad/board.kicad_sch:129` shows the defaults.

Test: `sch_import::tests::a_power_symbols_flags_and_value_position_are_kept`.

**Severity.** Low: the flags do nothing on a power symbol in netlist or BOM, and the field position is cosmetic. Strictly, though, the file is read differently.

**Already known?** No. r2 B-L4 dealt only with the default the writer uses.

### D4-L3. A project's `rule_severities` value other than error/warning/ignore is skipped; KiCad reads it as error (low)

**What differs.**
- Ours: `crates/cli/src/import_kicad.rs:398-399` keeps only `["error", "warning", "ignore"]`. Any other value leaves the check at its default.
- KiCad 25ec2335: the `rule_severities` loader in `erc_settings.cpp` does `m_ERCSeverities[code] = SeverityFromString( aJson[key] )`, and `common/widgets/ui_common.cpp::SeverityFromString` maps anything else to error:
  ```cpp
  if( aSeverity == wxT( "warning" ) ) return RPT_SEVERITY_WARNING;
  else if( aSeverity == wxT( "ignore" ) ) return RPT_SEVERITY_IGNORE;
  else return RPT_SEVERITY_ERROR;
  ```
  So `"label_dangling": "Warning"` (capitalised) or `"info"` is an error in KiCad and the default warning in ours.

**How to see it.** Test: extend `a_kicad_projects_erc_settings_come_in_with_the_schematic` with `"label_dangling": "info"`, and assert the severity is `"error"`.

**Severity.** Low. **Already known?** No.

### D4-L4. Three 25ec2335 checks with simple markers have no marker rule, so their exclusions fall back to the (check, location) key (low)

**What differs.**
- Ours: `crates/kicad/src/erc_exclusions.rs:149-191` (`rule`) has no row for `wired_implicit_power`, `empty_label_name` or `empty_power_value`. `marker()` returns `None` for them, so an exclusion carries no marker into the derived project, and kicad-cli does not waive it.
- KiCad 25ec2335's markers are fully determined, so rows can be added:
  - `erc/erc.cpp` 2323-2329: `ercItem->SetItems( pin.item ); markers.Place( std::move( ercItem ), pin.sheet, pin.position, ITEM_PATHS::MAIN );`
  - `reportSourceErrors( *m_schematic, ERCE_EMPTY_POWER_VALUE, .. )` (766) and `ERCE_EMPTY_LABEL_NAME` (818): `markers.Report( aCode, { source.item }, source.sheet, source.position, ITEM_PATHS::MAIN )` (195).

  All three are `r(Main, SpecificMain)`.

**How to see it.**
- Test: `erc_exclusions::tests::the_new_checks_have_markers`.
- Assert `marker("wired_implicit_power", "warning", &[item(..)], "/r/s", 2)` is `Some` with `sheet == main_sheet == ["r","s"]`, and likewise for the two empty-name checks.

**Severity.** Low (an exclusion of these is rare). **Already known?** No; 201f2183 added the checks without markers.

### D4-L5. Paste takes the design's library symbol where KiCad takes the current screen's, and renames a clipboard copy whose pins differ where KiCad uses the screen's copy (low)

**What differs.**
- Ours: `crates/ops/src/sch_clipboard.rs` (d87cec9c):
  - `in_cache` is true when *any* sheet of the design places the `lib_id` (`places_lib_id`);
  - a clipboard copy whose pins differ from the design's (`!same_pins`) is given a name of its own and keeps its pins.
- KiCad: `tools/sch_editor_control.cpp::Paste` looks only in the current screen's cache:
  ```cpp
  auto it = currentScreen->GetLibSymbols().find( symbol->GetSchSymbolLibraryName() );
  ...
  if( it != end && clipLibSymbol ) { if( power kinds differ ) libSymbol = clip copy; else libSymbol = new LIB_SYMBOL( *it->second ); }
  else if( it != end ) libSymbol = ..it->second; else if( clipLibSymbol ) libSymbol = ..clipLibSymbol;
  ```
  - A sheet that does not place `Device:R` takes the clipboard's copy, even when another sheet places it.
  - A sheet that does place it takes its own copy, even when the pins differ. KiCad never renames on paste; `SCH_SCREEN::Append` does that later only for a differing copy.

**How to see it.**
- Test `paste_takes_the_current_screens_copy_only`. Design: `Device:R` placed on sheet A only; paste an edited `Device:R` on sheet B.
- Assert that sheet B draws the clipboard's artwork. The "one copy per name per design" model is the limit here.

**Severity.** Low: different artwork or pins in rare cross-project pastes, much of it the model's limit.

**Already known?** In part: PARITY §46 lists d87cec9c as ported. The per-screen cache and the pins case are not noted.

### Checked and found to match (slices B and D)

**201f2183, the ERC table, mechanically against 25ec2335's `erc_item.cpp` and `erc_settings.cpp`.**
- The 56 user-editable checks of `allItemTypes` (before `heading_internal`) and `crates/model/src/erc_checks.rs` are equal as sets. Every default severity equals `m_ERCSeverities` (error unless set). The order is the same, except that `pin_to_pin` is put last on purpose.
- Every title in `web/studio/src/kicad/erc_checks.json` equals the `_HKI` text, apart from the `\"` escapes.
- Every `ERROR_TYPES` row of `erc_exclusions.rs` maps its key's `ERCE_*` to the `ERCET_*` that 25ec2335's `api/api_sch_enums.cpp::ToProtoEnum` gives.

**`erc_exclusions::rule`'s connectivity rows against 25ec2335's `ERC_MARKER_PLACER` calls (the open §45 item).** All match:

| Check | KiCad 25ec2335 | Our row |
|---|---|---|
| `pin_not_connected` | `Report(.., MAIN)`; a sheet pin's `DANGLING_PIN` gets `MAIN`; the root's hierarchical label gets `NONE` and is not sheet-specific (`conn_engine_erc.cpp` 1766, 1802, `erc.cpp` 2462) | our `hier_label` rule (only `ROOT_LABEL` yields a hierarchical label's `pin_not_connected`) |
| `no_connect_dangling` | items `{flag}`, at the flag, `MAIN` (`NoConnectFlagErrors`) | match |
| `single_global_label` | `MAIN` | match |
| `wire_dangling` | at the first wire of the group, `NONE` (`FloatingWires`) | match |
| `label_dangling`, `isolated_pin_label` | `NONE` | match |
| `bus_to_net_conflict` | `SetItems( net, bus )` at the net | match |
| `bus_to_bus_conflict` | at the first or canonical claim | match |
| `net_not_bus_member` | `SetItems( entry, bus )` at the entry | match |
| `multiple_net_names` | `SetItems( first, second )` at the second claim's item, `BOTH`: "the marker sits on the second source" | `At::Aux` |
| `pin_to_pin` | `SetItems( a, b )` at `a`, paths `a.sheet` / `b.sheet` | match |
| `different_unit_net` | `SetItems( other, first )` at `other`, paths other / first | match |
| `different_unit_footprint` | `SetItems( reference, unit )` at `unit`, sheet-specific on `unit.sheet`, main `reference->sheet`, aux `unit.sheet` | match |
| `duplicate_sheet_names` | `BOTH` | match |
| `pin_not_driven`, `power_pin_not_driven`, `ground_pin_not_ground`, `stacked_pin_name`, `missing_*` | `MAIN` | match |

**179c5cf5.**
- `sheet_file_in_work` and `sheet_file_relative_to` from the copy's folder. A child under `__up__/shared/` names a sibling `b.kicad_sch`.
- An absolute `Sheetfile` key is kept as written (`if( !fileName.IsAbsolute() ) fileName.MakeAbsolute( m_currentPath.top() )`).
- The `export_schematic` guard.
- B4-M1 is the exception.

**733537a4.** `sheet_file_text` against `SCH_SHEET::GetFileName` for files in subfolders.

**cda452b1.** A sheet's user fields move with Paste, Duplicate and Import Sheet. The copied screens' `sheet_pages` are cleared, as `updatePastedSheet` gives new pages. Delete takes the sheet's own fields and pages.

**48f86d0f, against `loadHierarchy` (the "already appears as a direct ancestor" check from `aParentSheetPath` up) and `BuildSheetList( .., false )` (`wxCHECK2_MSG( sheet->GetFileName() != aSheet->GetFileName() ..)`).** Round 3's scratch cases `miss`, `rec`, `mp` and `mp2` were re-run on the new binary, and the original's and the derived file's sheet lists are alike in all four.

**bd21173b, against 25ec2335 `RepairPageNumbers`.** It reserves every non-empty page first, keeps the first claimant, and gives the lowest free number. D4-L1, the root, is the exception.

**94a1fb37.** `(in_pos_files ..)` is read and written per `saveSymbol`; D4-M2 is the exception. The last `(path ..)` wins, as `AddHierarchicalReference`'s `RemoveInstance( m_Path )` has it.

**5aea27d7.** The pin map loader against 25ec2335's `pin_map` `PARAM_LAMBDA`:
- it needs 12 rows;
- it stops at the first bad row and keeps the earlier ones;
- it skips a cell that is not an integer or is out of range.

Our range 0..2, against KiCad's 0..3 (`UNCONNECTED`), is documented as Differs. `CompareLibId` in `units_available`.

**d9fb4d14.**
- `Annotate`'s `minRefId` is `sheetNum * step + 1`, or `aStartNumber + 1`.
- The `#FLG` prefix comes from the library.
- `formatRefStr` writes `"0%d"`.
- The used set covers every screen's symbols, power symbols and per-path references.
- The page for a shared screen is the first by page order, which equals KiCad's first instance; the rest is the known D3-M5 leftover.

**70bddba8.** Only segments that run on from one another are joined, and no segment is reversed, so `GetTopLeftItem`'s starts are kept.

**a9ef6e65.**
- Unique mode is `ReannotateDuplicates( existingRefs, annotateAlgo )`: `UNSORTED` sorts by X, `aStartAtCurrent` is on, and it is turned off for x100 and x1000 (`AnnotateByOptions`).
- The other modes are `ReannotateByOptions( order, algo, start, .., false, &hierarchy )` with each path's sheet number.
- Keep and Remove differ from KiCad on purpose, as documented on `PasteMode`: KiCad keeps duplicates and leaves `R?`.

**`effective_erc_severities` and import.** The nine 25ec2335 checks are kept, and the library-link checks stay at ignore.

## Slice C: schematic edit tools (round 4)

Scope:
- **(a) Round-3 commits** (merge f3c7f17c, `task-r2-leftovers`), each read with `git show` and checked against KiCad function by function:
  8bb401b6 (DeleteJunction, the commit-time CleanUp, `J`, `IsStrokeEquivalent`, `IsTerminalPoint`), 4cdc30e7 (Align to Grid, pivots, Align's
  clean-up, `#` autoplace), 20f1c16c (Highlight Net on a label), 9132b39d + ca076f83 (a label named after its wire), 53b1473b (power symbols'
  Autoplace Fields), 10369883, 98533828 (`onWheel`), e733a458 (Annotate dialog), 9858b752 (Duplicate of a hover), f849c0e3 (the rc lists' sort).
- **(b)** the editing rows these commits touch (Delete, Cut, `J`, the wire tool's finish, Paste's finish, Repeat).

KiCad: 8303b2ad `eeschema` for the tools; 25ec2335 for `createNewLabel` and for what kicad-cli's connectivity does.

**Binary and runs.** A fresh build of main `01ffc984` (`git archive`, own `CARGO_TARGET_DIR`): `/tmp/findings-r4-schematic.HMLq/target/debug/eda`.
Practical checks ran on a scratch board (`eda board new examples/two_pin_nets.yaml -o /tmp/findings-r4-schematic.HMLq/slice-C/b1`) served on port
8841 (stopped), with commands POSTed to `/api/cmd` (`slice-C/cmd.sh`, `slice-C/show.py`). kicad-cli cases: `/tmp/findings-r4-schematic.HMLq/cases/tj1`,
`tj2` (generator `gen/cases4c.py`, compared with `gen/run.sh`).

**Read first.** kicad-cli 25ec2335 connects a wire end, or a pin end, that lands on the *interior* of another wire, with no junction (cases tj1 and
tj2: `R3.1` is on `Net-(R1-Pad1)` in kicad-cli's netlist, as in ours). So a dot-less T is connected in the engine the user chose, and our tracer's
"a wire's every point" rule agrees with it. C4-L2 is low for that reason.

### C4-H1. Delete (and Cut) of a selection holding a wire and a junction on it is refused as a whole: "no junction with this id" (high)

**What differs.** The studio deletes a selection as one batch of per-item verbs (`web/studio/src/kicad-port/schDelete.ts:55-63`, `deleteCmds`). Since
8bb401b6 each of those verbs runs the commit-time clean-up on its own (`crates/ops/src/lib.rs:3015-3055`:
`Cmd::DeleteWire { id } => self.delete_wire(id).and_then(|_| self.commit_clean_up())`, and the same for symbols, labels, power symbols, bus entries,
no-connects and junctions). The first deletion's `CleanUp` removes a junction that no longer joins three items, and merges the wires in line there
(the merged wire keeps the first wire's id; `sch_scene.rs:877-923`, `Scene::finish`). A later verb of the same batch names an item that is gone:
`delete_junction` fails with `ops_unknown_junction` "no junction with this id" (`lib.rs:4993`), `delete_wire` with `ops_unknown_wire` "no wire with
this id" (`lib.rs:4931`). `Cmd::Batch` is atomic (`lib.rs:2999-3007`: `self.design = saved; return Err(e)`), so nothing is deleted.

The order makes it common: a box or Select All lists wires before junctions (`components/schematic/schItems.ts:85-87`, `allItems`), so any window over
a T, or over a wire and the dot on it, puts the wire first.

KiCad deletes everything first and cleans up once, in one commit (`tools/sch_edit_tool.cpp::DoDelete`):

```
for( EDA_ITEM* item : items )
{ ...
    if( sch_item->Type() == SCH_JUNCTION_T )
    {
        sch_item->SetFlags( STRUCT_DELETED );
        // clean up junctions at the end
    }
    ...
}

for( const VECTOR2I& point : pts )
{
    SCH_ITEM* junction = screen->GetItem( point, 0, SCH_JUNCTION_T );
    if( !junction )
        continue;
    if( junction->HasFlag( STRUCT_DELETED ) || !screen->IsExplicitJunction( point ) )
        m_frame->DeleteJunction( &commit, junction );
}

commit.Push( _( "Delete" ) );
```

then one `LOCAL_CLEANUP` (`sch_commit.cpp:191`, `SCHEMATIC::RecalculateConnections` -> `CleanUp`).

The same per-verb clean-up can also drop an id that a later `delete_wire` of a Break with several wires (`kicad-port/schBreak.ts:87-97`) names, when the
first deletion lets two other wires merge.

**How to see it.** Run on main's build (port 8841): `add_wire` (10.16,40.64)-(30.48,40.64), `add_wire` (20.32,50.8)-(20.32,40.64) (the T's junction is
added). Then the batch the studio sends for a window over the T:
`{"op":"batch","cmds":[{"op":"delete_wire","id":<the through wire>},{"op":"delete_wire","id":<the stem>},{"op":"delete_junction","id":<the dot>}]}`.
The reply is `{"message":"no junction with this id","ok":false}`; both wires and the dot are still there.

Test: `deleting_a_t_with_its_junction_in_one_batch_deletes_all_three` (`crates/ops/src/tests.rs`): draw the T as above, apply the batch in that
order, and assert `Ok`, with no wires and no junctions left.

Fix direction: run `commit_clean_up` once at the end of a batch (the commit), not after each verb; or let a delete verb accept an id the batch's own
clean-up already removed.

**Already known?** No. New in 8bb401b6 (the C3-H1 / C3-M1 fix).

### C4-M1. Align to Grid always aligns to 50 mil; KiCad uses the selection's grid (`GetSelectionGrid`) (medium)

**What differs.** The studio sends `{ op: "sch_move", verb: "align_to_grid", ids }` with no grid (`actions/useActionRunner.ts:1465`), and
`SchMoveCmd::AlignToGrid` falls back to `GRID_NM` (`crates/ops/src/sch_move.rs:209-212`, `grid_of`, `sch_move.rs:218-224`). KiCad picks the coarsest grid
of the selected items' categories (`tools/sch_move_tool.cpp::AlignToGrid`):

```
GRID_HELPER_GRIDS selectionGrid = grid.GetSelectionGrid( selection );
...
AlignSchematicItemsToGrid( m_frame->GetScreen(), items, grid, selectionGrid, callbacks );
```

with `EE_GRID_HELPER::GetItemGrid` (`tools/ee_grid_helper.cpp`): symbols, sheets, pins, labels, no-connects and rule areas `GRID_CONNECTABLE`; fields
and texts `GRID_TEXT`; shapes, text boxes and images `GRID_GRAPHICS`; wires, junctions and bus entries `GRID_WIRES`; a graphic line `GRID_GRAPHICS`.
Each category's size comes from the grid overrides, or from the current grid when its override is off. The studio already has this rule
(`kicad-port/schGridHelper.ts`, `getSelectionGrid`; `kicad-port/gridOverrides.ts`, text override 10 mil on by default) but does not use it here.

Consequences:
- a selection of texts or fields goes to 50 mil, not 10 mil;
- a graphic line, with the graphics override off, goes to 50 mil, not the current grid;
- a user's own overrides or current grid are ignored.

**How to see it.** A text at (13 mil, 0) selected alone, Align to Grid. KiCad (default overrides) moves it to (10 mil, 0); ours to (0, 0).

Test: `align_to_grid_uses_the_selections_grid` (`web/studio/src/actions` or `kicad-port`): the command for a text-only selection carries `grid` =
10 mil (254000 nm); for a symbol, 50 mil.

**Already known?** No. PARITY-sch row 178 and section 46 do not mention the grid.

### C4-M2. Highlight Net on a label that sits on no wire uses its text, not its net (medium; the rest of r3 A3-M4)

**What differs.** The server gives a label a `net_name` only from a wire under it (`crates/cli/src/studio.rs:1959`:
`sch.wires.iter().find(|w| !w.net.is_empty() && w.pts.windows(2).any(|s| point_on_segment(s[0], s[1], l.at)))`). Otherwise the studio falls back to
the label's text (`kicad-port/schNetAtPoint.ts`, `labelNet`: `l.net_name ?? l.net`; `components/schematic/painter.ts:1058`). A local label put directly on
a pin end (a common way to name a pin) has no wire, so Highlight Net on it lights `X`, and a highlight of its net `/X` (from the pin or a wire elsewhere)
does not light it. Hierarchical labels on a sub-sheet (`/sub/X`) behave the same way.

KiCad takes the label's own connection in every case (`tools/sch_editor_control.cpp::HighlightNet`, `conn->Name()`). A label on a pin is a subgraph
member like any other.

**How to see it.** A resistor with a local label `X` on its pin 1 end and no wire. Highlight Net on the label: KiCad highlights `/X`, which lights the
label and the pin. Ours highlights `X`.

Test: in `crates/cli` (studio API), a label on a pin end with no wire: assert its `net_name` is `/X`, i.e. take it from the tracer's net of the
label's point, not from a wire.

**Already known?** Partly: r3 A3-M4 is marked Fixed. This is the case the fix leaves out.

### C4-L1. The label tool's own verb runs no commit clean-up; the C3-M1 fix covers only `add_label` (low)

**What differs.** 8bb401b6 added `commit_clean_up` to `Cmd::AddLabel` (`lib.rs:3050`). The studio's label tool, though, places with
`{ op: "sch_edit", verb: "place_label" }` (`kicad-port/schLabelName.ts`, `placeLabelCmd`), which goes to `SchCmd::PlaceLabel` ->
`place_label` (`crates/ops/src/sch_edit.rs:404-406`, `sch_labels.rs:185`) with no clean-up. `add_label` is used only by Repeat, Convert and the bus
unfold.

KiCad's label placement pushes a commit with a connectable item, so `SCH_COMMIT::pushSchEdit` runs `LOCAL_CLEANUP`. So do `delete_sheet` and
`delete_sheet_pin` (`sch_edit.rs:389-396`): `DoDelete` puts the sheet pins' points into `pts` and calls `DeleteJunction` there.

**How to see it.** It changes the sheet only when the sheet is not already clean (an imported file with collinear pieces, or a junction left on a
deleted sheet pin). Example: delete a sheet pin that had a wire and a junction on it. KiCad drops the junction; ours keeps it.

Test: `deleting_a_sheet_pin_drops_its_junction` (`crates/ops/src/sch_sheet_pin_tests.rs`).

**Already known?** Partly: r3 C3-M1's note lists Swap and Edit Properties as left. The label tool's verb and the sheet deletions are not listed, and
PARITY-sch section 46 says the label placement is done.

### C4-L2. Deleting the dot of a T leaves the through wire split there (and the derived file writes the dot again); KiCad merges it (low)

**What differs.** `delete_junction` (`lib.rs:4986-4999`) removes the dot and runs `SCHEMATIC::CleanUp`'s merge, which skips a touching pair when
`IsJunction` holds at the point. At a T it does (three exit angles). KiCad's `SCH_EDIT_FRAME::DeleteJunction` (`bus-wire-junction.cpp`) merges the
wires that end at the dot with `aCheckJunctions = false`, and without `IsStrokeEquivalent`:

```
if( ( firstLine->GetEditFlags() & STRUCT_DELETED ) || ( secondLine->GetEditFlags() & STRUCT_DELETED ) || !secondLine->IsParallel( firstLine ) )
    return;
...
if( SCH_LINE* new_line = secondLine->MergeOverlap( screen, firstLine, false ) )
```

So in KiCad the two halves of the T become one wire, and the stem ends on its interior. In ours the halves stay apart. On the scratch board the
derived `board.kicad_sch` (`slice-C/b1copy/.kicad/`) still has `(junction (at 20.32 10.16) ...)` after the dot was deleted.

The same goes for two in-line wires of different strokes meeting at a deleted dot: KiCad merges them; ours keeps them apart.

**Connectivity.** No difference with kicad-cli 25ec2335: it connects a wire end on another wire's interior (cases tj1 and tj2). So the stem stays on the
net in both, and the commit message's "a T's stem is cut off" is not what either does.

**How to see it.** On the scratch board: a T at (20.32, 10.16) with labels `TA` and `TB`, then `delete_junction`. The wire keeps its vertex at 20.32,
`TB` is on `/TA`, and the derived file has the junction.

Test: `deleting_a_t_junction_merges_the_through_wire` (`crates/ops/src/tests.rs`): assert one through wire with no vertex at the T, and no junction in
`sch.junctions`.

**Already known?** No. The r3 C3-H1 fix covers the crossing case only (`delete_junction_at_crossing_unjoins`).

### C4-L3. The wire tool's finish: a label or sheet pin counts as connected only when a wire reaches it (low)

**What differs.** `kicad-port/schTerminalPoint.ts` (`labelConnected`, `sheetPinConnected`) asks for a wire at the point. KiCad
(`sch_screen.cpp::IsTerminalPoint`) asks `label->IsConnected( aPosition )` and `sheetPin->IsConnected( aPosition )`. Those count every item the
connection map gives the point, a pin included (`SCH_ITEM::IsConnected`). KiCad also takes `GetLabel( aPosition, 1 )` by the label's hit test.

**How to see it.** Put a local label on a pin end with no wire, then draw a wire from elsewhere and click on that point. KiCad finishes the wire;
ours goes on drawing.

Test: in `schTerminalPoint.test.ts`, a label on a pin tip with no wire is a terminal point for a wire.

**Already known?** No.

### Checked and found to match (slice C)

- **8bb401b6:**
  - `IsExplicitJunctionAllowed` = `AnalyzePoint( .., true )` with the bus-entry rule.
  - `J` refused with "Junction location contains no joinable wires and/or pins." (word for word, `SingleClickPlace`).
  - `AddJunction` then `CleanUp`; a click on an existing dot does nothing (ours refuses it, so there is no empty undo step).
  - `IsStrokeEquivalent` (DEFAULT = SOLID; width and colour).
  - `IsTerminalPoint`'s layers: a bus entry, junction, pin end, wire, label or sheet pin for a wire; a bus, sheet pin or label for a bus (never a pin);
    a line for a graphic line. It is asked only when a segment already exists (`doDrawSegments`).
  - Deleting the dot of a crossing gives two unjoined wires.
  - `DoDelete`'s "junction that is no longer explicit goes" comes out the same through `CleanUp`'s first loop.
- **4cdc30e7 against `AlignSchematicItemsToGrid` (8303b2ad):**
  - `selectedSheetPinPositions`, `pinPosToWires` (first wire), `originalWireEndpoints` and `canDragWires`;
  - a pin follows the selected wire's current end;
  - text by its position; each end of a graphic line on the grid;
  - a shape, text box, table or image stays (no connection points);
  - the finish is `TrimOverLappingWires`, `AddJunctionsIfNeeded`, `CleanUp` (`SCH_MOVE_TOOL::AlignToGrid`).

  C4-M1 is the exception.
- **Pivots:**
  - a symbol's selection box with its shown fields;
  - a sheet pin turned or mirrored about the centre of the sheet's box with its fields;
  - a lone sheet turned about `GetNearestHalfGridPosition( GetBoundingBox().Centre() )`.
- **53b1473b against `AUTOPLACER::getPreferredSides`:**
  - the power orders by `orient_angle` (0: top, bottom, right, left; 90: left, right, top, bottom; 180: bottom, top, left, right; 270: right, left, top,
    bottom);
  - mirroring is ignored for power symbols;
  - the `h_mirrored` swap and the `w/h > 3` swap apply otherwise;
  - power means `!IsInNetlist()` (a `#` reference).
- **9132b39d / ca076f83 against 25ec2335 `createNewLabel` / `findWire` / `findWireLabelDriverName`:**
  - the first wire whose box holds the point (not a bus);
  - only a local or global label driver gives a name (`IsType( { SCH_LABEL_T, SCH_GLOBAL_LABEL_T } )`), as `LocalName()`;
  - a hierarchical label always opens the dialog;
  - a global label is the strongest claim (`PRIORITY::GLOBAL`, `conn_claims.h`), so "the net's name is a global label's text" picks the driver
    correctly.
- **e733a458 against `DIALOG_ANNOTATE`:**
  - only "Regroup symbol units" is disabled (`Enable( options >= 1 )`, `OnOptionChanged`);
  - the scope and "Recurse into subsheets" are restored as saved (`TransferDataToWindow`).
- **9858b752:** `m_duplicateIsHoverSelection`, `SetIsHover`, then the move's `selectionClear` at its end. Nothing is left selected after a Duplicate
  of a hover.
- **f849c0e3:** the ERC Exclude Marker keeps `ercSelected` as an index into the unsorted list, so the same marker stays selected after the list is
  sorted again. It is cleared when exclusions are hidden.
- **98533828:** Shift+wheel pans vertically off macOS (`scroll_modifier_pan_v` Shift); the browser's swapped delta is undone.
- **Paste's finish (b):** a pasted wire ending on another wire's interior is connected here, and the derived file gets the dot. KiCad's move finish
  after Paste (`TrimOverLappingWires`, `AddJunctionsIfNeeded`, `CleanUp`) gives the same connectivity. The trim and merge of pasted overlaps are not
  run: same family as r2 C-L-series; not raised.
- **Repeat (b):** PARITY row 407 says partial (no move mode after a repeated symbol, fixed increment and offset); not raised again.

## Summary

| ID | Finding | Severity | Confirmed | Known? |
|---|---|---|---|---|
| A4-H1 | A local power symbol placed, pasted or brought by Import Sheet is traced and written as global (`local_power` only set by import) | high | code (import side run: matches) | no |
| A4-H2 | Jumper pin groups and `duplicate_pin_numbers_are_jumpers` join nothing; the derived file drops them | high | run (j1, j2) | no |
| A4-H3 | Two pins of one number: the tracer keeps one, the writer stacks both on the first's place | high | run (j3) | no |
| A4-H4 | Stacked pin numbers (`[1-2]`) are one literal pin; pads get no net | high | run (s1) | no |
| B4-H1 | A symbol cached under its `(lib_name ..)` is dropped on import, or drawn from the `lib_id` copy | high | run (QA `ground_pin_test_ok`) | no |
| C4-H1 | Delete / Cut of a wire and its junction in one selection is refused ("no junction with this id") | high | run (port 8841) | no (new in 8bb401b6) |
| A4-M1 | A `PT_NC` pin joins the wire on it; KiCad never connects it | medium | run (n1) | no |
| A4-M2 | A bus wire entry between two wires joins them in KiCad; ignored here | medium | run (g6) | no |
| A4-M3 | `path_units` not moved on a rename, not dropped on delete | medium | code | no |
| B4-M1 | Save Sheet as Design Block / Save Current Sheet Copy As fail for a sheet above the root's folder | medium | code | no (regression, 179c5cf5) |
| B4-M2 | "Exclude from position files" keyed by reference: lost by Annotate and Paste | medium | code | no (new in 94a1fb37) |
| D4-M1 | A symbol placed on a shared screen numbers its later paths with the defaults | medium | code | no |
| C4-M1 | Align to Grid always uses 50 mil, not `GetSelectionGrid` | medium | code | no |
| C4-M2 | Highlight Net on a label with no wire under it uses its text | medium | code | partly (r3 A3-M4) |
| D4-L1 | `RepairPageNumbers` reserves page 1 for the root | low | run | partly (r2 B-L4) |
| D4-L2 | An imported power symbol loses its flags and its hand-placed Value position | low | run | no |
| D4-L3 | A severity other than error/warning/ignore is skipped; KiCad reads it as error | low | code | no |
| D4-L4 | `wired_implicit_power`, `empty_label_name`, `empty_power_value` have no marker rule | low | code | no |
| D4-L5 | Paste takes the design's library copy, not the current screen's | low | code | partly (§46) |
| C4-L1 | `place_label`, `delete_sheet`, `delete_sheet_pin` run no commit clean-up | low | code | partly (r3 C3-M1) |
| C4-L2 | Deleting a T's dot leaves the through wire split; the derived file writes the dot again | low | run | no |
| C4-L3 | The wire tool finishes on a label or sheet pin only when a wire reaches it | low | code | no |

Counts: **high 6, medium 8, low 8**.

PARITY-sch section 45's "Left: the connectivity rows of `erc_exclusions::rule` were not compared line by line" can be closed (slice D's table: all match).
