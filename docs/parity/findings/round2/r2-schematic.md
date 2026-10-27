# Round 2 findings: the schematic editor and its files (`r2-schematic`, 2026-10-27)

Reviewer: `r2-schematic` (findings only, no code changed). Main at `5cca5fff`. KiCad: `8303b2ad` (`/home/team_braneboard_com/src/kicad-src-8303b2ad`), newer items `25ec2335`.
Done as four parallel slices: **A** connectivity and net names (batches A and D, PARITY-sch §41), **B** hierarchy and the `.kicad_sch` reader/writer (§42 nested shared sheets, §14/19/22/32/37/39), **C** edit tools, labels, fields, symbol placement (GAPS 1, 12; §1-3, 12, 21-24, 29-31), **D** clipboard, Annotate, the ERC dialog, view controls (GAPS 6, 11, 20; §4, 5, 9, 15, 26-28).
Finding IDs carry the slice letter (`A-N1`, `B-H1`, `C-H1`, `D-C1`); a bare ID inside a slice means that slice's. Scratch cases and scripts: `/tmp/findings-r2-schematic.x5lr/<slice>/`.

## Read first: three things that affect the whole review program

1. **The judge runs a different connectivity engine from the one we port.** The box's `kicad-cli` is 10.99.0 (25ec2335), where `common/advanced_config.cpp:302` sets `m_ConnectivityEngine = true;` and eeschema uses the new `eeschema/connectivity/conn_*.cpp` engine (`conn_claims.cpp::CLAIM_LESS`, `BuildIslandRecord`), not `connection_graph.cpp`. Our tracer and every PARITY connectivity row cite 8303b2ad's `connection_graph.cpp`, which has no `connectivity/` folder. Slice A says per finding whether the old engine agrees (A-N4, A-N5, A-N6 hold on both; A-N1 is a 25ec2335 behaviour). Decide which one is the target; kicad-cli (what F8, ERC and the netlist do) argues for 25ec2335.
2. **Main's `target/debug/eda` is stale** (2026-10-23 11:08), before batch A, batch D and §42. Slice B ran `~/work/ds-agentic-eda/target/debug/eda` (built 10-26 at 4c7fb03f, which main merged as 5cca5fff) on scratch copies; slices A, C and D read our side from the code.
3. (**Fixed** on task-r2-schematic: 8613ea1a, 52ac71ac: the test goes through `eda import-kicad`, copies subfolders and compares every pin's net name, ours and kicad-cli's of the derived file too; before the fixes 11 of its 24 projects traced alike and 10 named alike, now all 24.) **The QA parity test does not test what §41/§42 claim it proves.** `slow_kicad_qa_hierarchies_trace_as_kicad_cli_reads_them` (`crates/cli/src/sch_hierarchy_tests.rs:1245-1311`, `qa_traced`) compares pin groups only, never net names (A), calls `import_kicad_sch_tree` directly, skipping `Cmd::ReplaceSchematic` / `place_by_box_corner` that `eda import-kicad` runs (B-H1), and copies only the root folder's files, so a project with sheets in a subfolder compares nothing (B-H2).

**Duplicate across slices:** B-M5 and D-C1 are the same defect (the clipboard does not know the sheet path it was copied from, so Copy on the second placement of a shared screen writes the first path's reference and unit; KiCad's `saveSymbol( aForClipboard, aRelativePath )`). Counted once (D-C1); B-M5 holds the API-side detail.


---

## r2-schematic, slice A-nets: schematic connectivity and net naming

Scope: batch A (KiCad's net names) and batch D (PARITY-sch.md section 41). Our code: `crates/engine/src/nets.rs` (`trace_nets_with`, `driver_order`, `default_net_name`, `kicad_net_name`), `crates/engine/src/net_labels.rs`, `crates/cli/src/board.rs` (`trace_kicad_names`, `trace_design`, `power_value_under`). Read at main `5cca5fff`.

### Read this first: the reference engine is not the one kicad-cli runs

- `kicad-cli` on this box is KiCad 25ec2335 (10.99.0). In that tree, eeschema's connectivity is a new engine, `eeschema/connectivity/conn_*.cpp` (`SCH_CONNECTIVITY::ENGINE`, `FACADE`). `common/advanced_config.cpp:302` turns it on by default: `m_ConnectivityEngine = true;`. `conn_overview.h` says: "The advanced configuration flag ADVANCED_CFG::m_ConnectivityEngine selects this engine instead of CONNECTION_GRAPH."
- Our tracer and every PARITY row cite `connection_graph.cpp` of 8303b2ad (`ResolveDrivers`, `compareDrivers`, `propagateToNeighbors`, `processSubGraphs`). That tree has no `eeschema/connectivity/` folder at all. Only `default_net_name` cites the new engine (`conn_pin_name.cpp`).
- So "verify against KiCad" means two different things for connectivity, and only 25ec2335 is what `kicad-cli sch export netlist`, the ERC and F8 do. Every finding below was confirmed by running the box's kicad-cli on a small scratch case. Most of them also follow from the new engine's code. For each one I also say whether the old engine would agree.
- In the new engine, the net name is the greatest `CLAIM` over the whole net. `conn_claims.cpp::CLAIM_LESS` ranks claims like this:
  ```
  return std::tuple( aClaim.priority, -static_cast<int>( aClaim.depth ), aClaim.globalPowerParent,
                     aClaim.localPowerParent, aClaim.outputShape, !aClaim.hasPad );
  ...
  if( aLeft.fullName != aRight.fullName )
      return keys.NameLess( aRight.fullName, aLeft.fullName );
  ```
  The priority order is `NONE, PIN, SHEET_PIN, HIER_LABEL, LOCAL_LABEL, LOCAL_POWER_PIN, BUS_MEMBER, GLOBAL_POWER_PIN, GLOBAL` (`conn_claims.h`). Claims join through name edges (`BuildIslandRecord`):
  - GLOBAL and GLOBAL_POWER_PIN emit a GLOBAL edge **and** a SHEET edge;
  - LOCAL_LABEL and LOCAL_POWER_PIN emit a SHEET edge;
  - HIER_LABEL emits a SHEET edge and a PORT edge;
  - SHEET_PIN emits a PORT edge only.
- Two process notes:
  - `target/debug/eda` on main is dated Oct 23 11:08, which is before batch A (fc8ee220, 10-24 17:29) and batch D (4f07d67a, 10-24 23:28). It does not run the code under review, so "ours" below is read from the code, not run.
  - `slow_kicad_qa_hierarchies_trace_as_kicad_cli_reads_them` (`crates/cli/src/sch_hierarchy_tests.rs:1245-1311`, `qa_traced`) compares **pin groups only**, never names. No QA test holds our net *names* to kicad-cli's.

Scratch cases (the generator and every case): `/tmp/findings-r2-schematic.x5lr/A-nets/gen/cases.py` writes `t1`..`t17` under `/tmp/findings-r2-schematic.x5lr/A-nets/`. To run one: `cd /tmp/findings-r2-schematic.x5lr/A-nets/tN && kicad-cli sch export netlist -o n.net t.kicad_sch && python3 -I ../gen/nets.py n.net`.

---

### A-N1. A global label or power symbol on one sheet does not join a local or hierarchical label of its name on that sheet (high)

**Fixed** on task-r2-schematic: bc4f7c1d (KiCad 25ec2335 `BuildIslandRecord` edges, the user's choice of the new engine)

**What differs.**
- **Ours:** `nets.rs:383-390` files a local label (and a hierarchical label) under `local` and a global label or power symbol under `global`. Phase B (`:434-458`) unions within `local` and within `global`, never across them. So on one sheet, a power symbol `VCC` and a local label `VCC` are two nets, `VCC` and `/VCC`. The same holds for a global label `SIG` and a local label `SIG`, and for a power symbol `VCC` inside a sheet and the hierarchical label `VCC` of that sheet.
- **KiCad 25ec2335:** `conn_claims.cpp::BuildIslandRecord`:
  ```
  case PRIORITY::GLOBAL:
  case PRIORITY::GLOBAL_POWER_PIN:
      result.edges.push_back( { SCOPE::GLOBAL, 0, claim.name } );
      [[fallthrough]];
  case PRIORITY::LOCAL_LABEL:
  case PRIORITY::LOCAL_POWER_PIN:
      result.edges.push_back( { SCOPE::SHEET, aInputs.instance, claim.name } );
      break;
  case PRIORITY::HIER_LABEL:
      result.edges.push_back( { SCOPE::SHEET, aInputs.instance, claim.name } );
  ```
  A global name also emits the sheet-scoped edge that a local label of its name emits, so the two are one net. Through a hierarchical label, the parent's sheet-pin net then joins the global net too.

**How to see it.** Confirmed with kicad-cli:
- `t11`: R1.1 has power symbol VCC, R2.1 has a local label `VCC` on the same sheet, not wired together. kicad-cli: `VCC ['R1.1', 'R2.1']`.
- `t12`: the same with global label `SIG`. kicad-cli: `SIG ['R1.1', 'R2.1']`.
- `t13`: child sheet with hierarchical label `VCC` on R3.1 and power symbol `VCC` on R4.1, and the root's sheet pin `VCC` on R1.1. kicad-cli: `VCC ['R1.1', 'R3.1', 'R4.1']`.

Ours (from the code) gives `VCC` / `/VCC`, `SIG` / `/SIG`, and `/sub/VCC` [R1.1, R3.1] / `VCC` [R4.1].

Test: `nets.rs::a_local_label_joins_the_global_net_of_its_name_on_its_sheet`. Root: R1.1 on power `VCC`; R2.1 on local label `VCC`. Assert one net `VCC` with both pins. Repeat with a global label, and with a hierarchical label in a child plus a power symbol there.

The old engine names these differently (`/VCC` is a distinct name in `m_net_name_to_subgraphs_map`), so this is a 25ec2335 behaviour; kicad-cli is the reference.

**Already known?** Not listed. PARITY-sch row "Nets across sheets" (`:543`) claims the opposite ("a local or hierarchical label is a net on its sheet, a global label and a power symbol across all"). The studio's ERC waives `same_local_global_*` (`PARITY-sch.md:865`).

### A-N2. A sheet pin joins a local label of its name on the parent sheet (high)

**Fixed** on task-r2-schematic: bc4f7c1d

**What differs.**
- **Ours:** `nets.rs:451-458`: a sheet pin that is the only one of its name on the sheet is added to `local` and joined with the parent's local and hierarchical labels of that name. The module doc says so too (`:13`: "a sheet pin joins only when no other sheet pin on the sheet has its name").
- **KiCad 25ec2335:** a SHEET_PIN claim emits only `SCOPE::PORT` edges:
  ```
  case PRIORITY::SHEET_PIN:
      if( const auto& instance = aInputs.items.at( claim.source.item ).portInstance )
          result.edges.push_back( { SCOPE::PORT, *instance, claim.name, result.kind } );
  ```
  So it never joins a same-named label on its own sheet without a wire.

**How to see it.** Confirmed with kicad-cli, case `t9`:
- Setup: root sheet pin `FOO` wired to R1.1; elsewhere on the root, a local label `FOO` on R2.1; the child has hierarchical label `FOO` on R3.1.
- kicad-cli: `/FOO ['R2.1']` and `/sub/FOO ['R1.1', 'R3.1']`.
- Ours: one net `/FOO` with R1.1, R2.1 and R3.1.

Test: `nets.rs::a_sheet_pin_does_not_join_a_label_of_its_name_on_its_own_sheet`.

**Already known?** Not listed. It is the documented design of the tracer, ported from the old `processSubGraphs` promotion.

### A-N3. Two sheet pins of one name on one sheet symbol are not joined (medium)

**Fixed** on task-r2-schematic: bc4f7c1d

**What differs.**
- **Ours:** `nets.rs:451-458` joins sheet pins by name only when they are unique, and the hierarchy join (`:460-476`) needs a hierarchical label inside. So two pins `X` of one sheet symbol on different parent wires, with no `X` label inside, stay two nets (`/X`, `/X_2`).
- **KiCad 25ec2335:** both PORT edges are `{ PORT, <child instance>, "X" }`, which is one node, so the two are one net.

**How to see it.** Confirmed with kicad-cli, case `t15`: kicad-cli gives `/X ['R1.1', 'R2.1']`. It is an ERC-level drawing, hence medium.

**Already known?** Not listed.

### A-N4. Global labels and global power names tie-break by sheet depth; KiCad's are unscoped (depth 0) (high)

**Fixed** on task-r2-schematic: bc4f7c1d

**What differs.**
- **Ours:** `nets.rs:204-214` (`driver_order`) compares priority, then `depth`, then the name, for every driver. A global label or power symbol carries the depth of its sheet (`Driver::new(.., depth[si], ..)`, `:383`, `:405`). So among global names on one net, the one nearest the root wins, not the alphabetically first.
- **KiCad 25ec2335:** `conn_claims.cpp::PrepareInstanceClaims`:
  ```
  const bool unscoped =
          priority == PRIORITY::GLOBAL || priority == PRIORITY::GLOBAL_POWER_PIN || priority == PRIORITY::PIN;
  claim.depth = unscoped ? 0 : aScope.depth;
  ```
  The old engine agrees: its global pre-pass uses `compareDrivers`, which looks at priority and then the name, with no depth.

**How to see it.** Confirmed with kicad-cli, case `t1`:
- Setup: root R1.1 has global label `ZZZ`; the child has `ZZZ` and `AAA` on one wire with R2.1.
- kicad-cli: `AAA ['R1.1', 'R2.1']`. Ours: `ZZZ`.
- The same happens with power symbols: `VSS` at the root, and `VSS` plus `AGND` together in a child.

Test: `nets.rs::global_names_on_one_net_are_ranked_without_their_sheets_depth`.

**Already known?** Not listed. The PARITY-pcb row "A net's name" (`PARITY-pcb.md:2112`, marked **identical**) and the doc at `nets.rs:200-203` describe the depth rule as KiCad's.

### A-N5. A power symbol does not outrank an invisible power-input pin of an ordinary symbol (high)

**Fixed** on task-r2-schematic: bc4f7c1d

**What differs.**
- **Ours:** both kinds are `GLOBAL_POWER_PIN` drivers (`nets.rs:367-370` for the pin, `:405` for the power symbol). `driver_order` then compares depth and the name, so a hidden `AVDD` pin wired to a `VCC` power symbol names the net `AVDD`.
- **KiCad 25ec2335:** `CLAIM_LESS` ranks `globalPowerParent` ("Pin of a global power symbol") right after priority and depth, before the name. The old engine's `compareDrivers` rule 3 agrees:
  ```
  bool aGlobal = parentA && parentA->IsGlobalPower();
  bool bGlobal = parentB && parentB->IsGlobalPower();
  if( aGlobal != bGlobal )
      return aGlobal ? -1 : 1;
  ```

**How to see it.** Confirmed with kicad-cli, case `t2`:
- Setup: U1 (a project symbol) has pin 1 as a hidden `power_in` named `AVDD`, wired to a `power:VCC` symbol.
- kicad-cli: `VCC ['U1.1']`. Ours: `AVDD` (because "AVDD" < "VCC").
- This happens on any board that ties an IC with legacy hidden power pins (CMOS logic, old MCUs) to a power symbol of another name.

Test: `sch_hierarchy_tests.rs::a_power_symbol_names_the_net_of_a_hidden_power_pin_it_is_wired_to`.

**Already known?** Not listed. Section 41's "An invisible power input" row covers only the joining.

### A-N6. A pin name shared across units gets "-Pad<n>" in ours; KiCad counts only the pins of the unit drawn (high)

**Fixed** on task-r2-schematic: 5e1074a8

**What differs.**
- **Ours:** `nets.rs:709-720` (`shown_name_counts`) counts the pins of each name over **all** pins of the reference: `pin_names` is every pin of the part (`board.rs:497-507`, `shown_pin_names`), and the count is keyed by `(reference, name)`. A dual op-amp whose units A and B both have pins `+` and `-` therefore gets `has_multiple` on every input, and `default_net_name` adds the pad number.
- **KiCad 25ec2335:** `conn_facts.cpp` builds `numbersByName` from `cached.symbol->GetPins( &aPath )`. `SCH_SYMBOL::GetPins` (`sch_symbol.cpp:2719-2738`) keeps only this unit's pins and the common ones:
  ```
  if( unit != ALL_UNITS && pin->GetUnit() != ALL_UNITS && pin->GetUnit() != unit )
      continue;
  ```
  and then:
  ```
  name.hasDuplicateName = numbers.size() > 1 || !numbers.contains( pin->GetShownNumber() );
  ```
  8303b2ad's `SCH_PIN::GetDefaultNetName` does the same (`symbol->GetPins( &aPath )`).

**How to see it.** Confirmed with kicad-cli on QA `netlists/complex_hierarchy` (copied to `/tmp/findings-r2-schematic.x5lr/A-nets/complex_hierarchy`):
- kicad-cli names `Net-(U3A--)`, `Net-(U3B-+)`, `Net-(U4A--)`, `Net-(U4B-+)`.
- Ours, from the code: `Net-(U3A---Pad2)` and so on.
- The QA trace test passes only because it compares pins, not names.
- This hits every LM358, TL072 or LM393-style part on a net nothing labels.

Test: `sch_hierarchy_tests.rs::a_pin_name_repeated_in_another_unit_adds_no_pad_number`. An LM358 (units A/B with `+`/`-`) and U1A's `+` wired to R1.1; assert `Net-(U1A-+)`. Or assert names in `qa_traced` for `complex_hierarchy`. The existing test at `sch_hierarchy_tests.rs:385-466` uses `IN1+`/`IN2+`, which are unique, so it misses this.

**Already known?** Not listed. PARITY-pcb.md:2112 ("identical") says "a pin's name shared by other pins of its part has its pad number added", which is the bug written as the rule.

### A-N7. A bus member names its net above local and hierarchical labels (BUS_MEMBER priority) (medium)

**Fixed** on task-r2-schematic: bc4f7c1d

**What differs.**
- **Ours:** the virtual member node takes the bus's best driver's priority (`nets.rs:531`: `Driver::new(best.prio, m.clone(), ..)`). That is LOCAL_LABEL for a bus label, or HIER_LABEL. A member net that also carries a local label of another name is then named by the alphabetically first label. Conversely, a member of a global bus gets GLOBAL (7) and outranks a power symbol on its net.
- **KiCad 25ec2335:** `conn_bindings.cpp::BindBundle` gives every slot `slot.claim.priority = PRIORITY::BUS_MEMBER;`, which sits between LOCAL_POWER_PIN and GLOBAL_POWER_PIN. Its depth and path are those of the canonical bus claim, and `fullName = path + leaf.name`.

**How to see it.** Confirmed with kicad-cli, case `t3`:
- Setup: root bus label `D[0..1]` on a bus; a wire with local labels `D0` and `AAA` and R1.1, R2.1.
- kicad-cli: `/D0 ['R1.1', 'R2.1']`. Ours: `/AAA`.

Test: `nets.rs::a_bus_member_names_its_net_above_a_local_label`.

**Already known?** Not listed.

### A-N8. A no-connect flag takes its pin out of the net; KiCad keeps it, and the whole net is `unconnected-(..)` (medium)

**Fixed** on task-r2-schematic: d931295c

**What differs.**
- **Ours:** `nets.rs:362-365` (and `:657-660`) skips every pin under a no-connect flag, so the pin is on no net. The rest of the wire is named without it. The pin alone then gets its pad's net from the board writer (`unconnected_pin_nets`).
- **KiCad 25ec2335:** the pin stays in its island. `conn_signals.cpp::SignalBaseName`:
  ```
  if( claim.priority == PRIORITY::PIN && ( !aSummary.pinWitnesses[1] || aSummary.noConnect ) )
      return claim.ncName;
  ```
  So the net keeps every pin, and with only pins driving it, it is named by the best pin's `unconnected-(..)` name.

**How to see it.** Confirmed with kicad-cli, case `t6`:
- Setup: R1.1 and R2.1 wired, with a no-connect flag on R1.1.
- kicad-cli: `unconnected-(R1-Pad1) ['R1.1', 'R2.1']`.
- Ours: R2.1 alone in `unconnected-(R2-Pad1)`, and R1.1 on no traced net. After F8, KiCad's board would join the two pads.

Test: change `nets.rs::a_no_connect_flag_keeps_its_pin_out_of_every_net` into `a_no_connect_flag_on_a_wired_pin_names_the_net_unconnected`. The lone-pin case also stays a net of its pin in kicad-cli's netlist.

**Already known?** Not listed. It is a deliberate choice in the tests, and also an ERC case (`no_connect_connected`), hence medium.

### A-N9. A pin of a symbol excluded from the board can name a net (medium)

**Fixed** on task-r2-schematic: d931295c

**What differs.**
- **Ours:** `trace_design`'s `pins_of` (`board.rs:569-600`) seeds every symbol's pins whatever `exclude_from_board` says, and `kicad_net_name` (`nets.rs:754-760`) picks among all pins. So R1.1 wired to A1.1, where A1 is `(on_board no)`, is named `Net-(A1-Pad1)`.
- **KiCad 25ec2335:** `conn_facts.cpp`:
  ```
  text.canDrive &= symbol->DoNetList() && !symbol->GetExcludedFromBoard( &aPath, variant )
                   && !reference.reference.StartsWith( wxS( "#" ) );
  ```
  A pin that cannot drive makes no claim. It still counts as a pin witness, so the net is not `unconnected-`.

**How to see it.** Confirmed with kicad-cli, case `t8`: kicad-cli gives `Net-(R1-Pad1) ['R1.1']`, and the netlist leaves A1 out. Ours names it `Net-(A1-Pad1)` and lists A1.1.

Test: `sch_hierarchy_tests.rs::a_pin_of_a_symbol_excluded_from_the_board_names_no_net`.

**Already known?** Not listed.

### A-N10. Text variables in labels are not resolved for connectivity (high)

**Fixed** on task-r2-schematic: f29b4fb7 (with `@{..}` arithmetic; QA issue23962 and issue24044 now alike)

**What differs.**
- **Ours:** a label's driver name is its stored text (`nets.rs:383`, `l.net`). Only power symbols' values are resolved (`board.rs:561`: `has_vars` looks at `power_symbols` alone; `power_value_under`). So on a screen shown twice, the global label `${SHEETNAME}_EN` is one global net, `${SHEETNAME}_EN`, that shorts both placements. A local label `L_${#}` is named `/A/L_${#}`.
- **KiCad 25ec2335:** `conn_facts.cpp`: `text.name = EscapeString( label->GetShownText( &path, FOR_NETNAME ), CTX_NETNAME );`, per instance. 8303b2ad's `driverName` agrees: `label->GetShownText( &m_sheet, false )`.

**How to see it.** Confirmed with kicad-cli, case `t17`:
- Setup: screen `sub` placed as sheets `A` and `B`, with global label `${SHEETNAME}_EN` on R3/R4 pin 1 and local label `L_${#}` on pin 2.
- kicad-cli: `A_EN ['R3.1']`, `B_EN ['R4.1']`, `/A/L_2`, `/B/L_3`.
- Ours: one net `${SHEETNAME}_EN` with R3.1 and R4.1, which is wrong connectivity.
- The same is true for a sheet pin's text (`driverName` resolves it with the child pushed on the path).

Test: `nets.rs::a_labels_sheet_variable_is_resolved_per_placement`, beside `a_power_symbols_page_variable_is_resolved_per_placement`.

**Already known?** Partly. PARITY-sch.md:712 (19b) lists labels as "Not done" for the *display*. Section 41 fixed only power values. Connectivity is not listed.

### A-N11. Two nets that share a name: KiCad suffixes `_1`, ours `_2`; the PARITY row says KiCad gives both the name (medium)

**Fixed** on task-r2-schematic: d931295c (`_1`, `_2`; which net keeps the bare name still differs: KiCad picks by item uuids)

**What differs.**
- **Ours:** `nets.rs:617-627` starts at `k = 2` (`{n}_2`).
- **KiCad 25ec2335:** `conn_publish.cpp::AssignNames`: the holder of the bucket keeps the bare name, and the rest get `Suffix( suffix )` from `nextSuffix.try_emplace( .., 1 )`, i.e. `_1`, `_2`, and so on. The holder is the bucket's minimum by `preferred`: strong first, then `CLAIM_LESS`, else the predecessor's weight, then the node key.

**How to see it.** Confirmed with kicad-cli, case `t16`:
- Setup: two sheet symbols each with pin `X`, wired to R2.1 and R1.1, and no `X` label inside.
- kicad-cli: `/X ['R2.1']` and `/X_1 ['R1.1']`. Ours: `/X` and `/X_2`.
- Which net keeps the bare name also differs (ours: `(depth, name, first pin)`).

**Already known?** Listed wrongly. PARITY-pcb.md:2112 ("**Differs**: ... where KiCad gives both the name (and F8 would join their pads)") and the comment at `nets.rs:600-601` are both false for the box's kicad-cli.

### A-N12. A sheet pin of output shape is preferred over the name order (low)

**Fixed** on task-r2-schematic: bc4f7c1d

**What differs.**
- **Ours:** `driver_order` has no shape.
- **KiCad 25ec2335:** `CLAIM_LESS` ranks `outputShape` before the name. The old `compareDrivers` rule 4 does the same ("For two sheet pins, an OUTPUT shape beats an INPUT shape").

**How to see it.** Confirmed with kicad-cli, case `t10`: sheet pins `AAA` (input) and `ZZZ` (output) on one parent wire, with no hierarchical labels inside. kicad-cli: `/ZZZ`. Ours: `/AAA`. This only matters when no hierarchical label (higher priority) is on the net.

**Already known?** Not listed.

### A-N13. An unannotated reference: KiCad adds a hash of the symbol's uuid (low, unconfirmed on ours)

**Open** after task-r2-schematic: needs the symbol's own uuid kept (B-L3) and `KIID::Hash`

**What differs.**
- **Ours:** `default_net_name` (`nets.rs:726-743`) treats `R?` like any reference, giving `Net-(R?-Pad1)`.
- **KiCad 25ec2335:** `conn_pin_name.cpp::RenderPinNetName`:
  ```
  name << aReference.reference
       << wxString::Format( wxS( "-%08x" ), static_cast<unsigned>( KIID( aReference.symbolUuid ).Hash() & 0xFFFFFFFF ) );
  name << "-Pad" << number << ")";
  ```
  When `aReference.reference.Last() == '?'`, two unannotated `R?` get distinct names.

**How to see it.** Two unannotated resistors each on their own wire with a third pin. To confirm, run kicad-cli on such a file and compare with `trace_kicad_names`.

**Already known?** Not listed. ERC flags unannotated symbols anyway.

---

### Checked and found to match (do not review again)

- `escape_net_name` = `EscapeString( .., CTX_NETNAME )` (`common/string_utils.cpp:196-203`): `/` becomes `{slash}`, CR/LF dropped. Applied to label, sheet pin and power value names, and to sheet names in the path (`PathHumanReadable( true, false, true )`: root `/`, child `/<name>/`; `depths()`).
- The path prefix (`recacheName`; 25ec2335's `claim.fullName = path + name`):
  - no path for a global label, a global power pin or a pin;
  - a path for local labels, hierarchical labels and sheet pins;
  - a root local label is `/NAME` (round 1's "root without `/`" is fixed; `Driver::new` with `sheet_path = ""`).
- A sheet pin's claim lives on the parent with the parent's path and depth (`/X`).
- The priority order PIN < SHEET_PIN < HIER_LABEL < LOCAL_LABEL < GLOBAL_POWER_PIN < GLOBAL, and priority before depth across the hierarchy: a deeper local label beats a shallower hierarchical label.
- `-Pad` names are demoted before the alphabetical order, both among pins and among labels.
- The default pin name (`RenderPinNetName`):
  - `Net-(` vs `unconnected-(` (forced, or a `PT_NC` pin);
  - a name that equals the number is treated as no name;
  - `~` and empty names fall back to `<ref>-Pad<n>` without the unit;
  - a named pin uses `GetRef( &aPath, true )` with the unit letters only for a multi-unit symbol (`pin_units` only when `unit_count > 1`);
  - `-Pad` is added for unconnected pins or duplicates;
  - NC pins are left out of the duplicate count.
  (Except N6 and N13, and stacked pins, which are known.)
- A net of one pin is `unconnected-(..)` (`!pinWitnesses[1]`); among pins, the best is the non-`-Pad` name and then the alphabetical first.
- Hidden power inputs of ordinary symbols (`SCH_PIN::IsGlobalPower`'s legacy rule: `PT_POWER_IN`, not a power or local-power parent, invisible) name their net by the lib pin's `GetName()`, escaped, wired or not, and join every net of that name. PWR_FLAG (power output) names nothing.
- A power symbol's Value is resolved per placement (`${#}`, `${SHEETNAME}`, title blocks) before escaping (section 41; a project variable is a known gap).
- A screen shown twice is traced per placement, with its pins under each placement's references.
- Hierarchical label ↔ sheet pin of the same placement by name (PORT edge), on the same layer: a bus never joins a net.
- `net_labels.rs`: it skips default names (`Net-(`, `unconnected-(`) and nets a label, power symbol or sheet pin already names on the sheet. A local label gives KiCad `/<path>/<name>`, as the module says.
- Not re-reviewed (known and listed): local power symbols (LOCAL_POWER_PIN), stacked pins (`GetSmallestStackedPadNumber`), a project `${VAR}` in a power value, and the group-member `VectorIndex` fallback.

### Summary

| ID | Finding | Severity | Confirmed with kicad-cli | Known? |
|---|---|---|---|---|
| N1 | A global label or power symbol does not join a local or hierarchical label of its name on its sheet | high | yes (t11, t12, t13) | no; the PARITY-sch row says the opposite |
| N2 | A sheet pin joins a same-named local label on the parent | high | yes (t9) | no; documented as KiCad's |
| N3 | Two sheet pins of one name on one sheet symbol are not joined | medium | yes (t15) | no |
| N4 | Global names tie-break by sheet depth; KiCad's are unscoped | high | yes (t1) | no; PARITY-pcb row "identical" |
| N5 | A power symbol does not outrank a hidden power pin's name | high | yes (t2) | no |
| N6 | `-Pad` added for a pin name repeated in another unit (`Net-(U3A--)`) | high | yes (QA complex_hierarchy) | no; PARITY-pcb row states the bug as the rule |
| N7 | Bus member slot priority (BUS_MEMBER) not ported | medium | yes (t3) | no |
| N8 | A no-connect flag takes its pin off the net; KiCad keeps it, net `unconnected-(..)` | medium | yes (t6) | no |
| N9 | A pin of a symbol excluded from the board names a net | medium | yes (t8) | no |
| N10 | Label and sheet-pin text variables not resolved for connectivity | high | yes (t17) | display-only "Not done" in 19b |
| N11 | Name collisions: KiCad `_1` and so on, ours `_2`; the PARITY row says KiCad shares the name | medium | yes (t16) | listed wrongly |
| N12 | An output-shape sheet pin is not preferred | low | yes (t10) | no |
| N13 | Unannotated reference: no uuid hash in the default name | low | KiCad code only | no |

Meta: the box's kicad-cli runs the new `SCH_CONNECTIVITY` engine (25ec2335, `m_ConnectivityEngine = true`), not the `connection_graph.cpp` that the tracer and the PARITY rows cite. The QA trace test compares pin groups, not names. Main's `target/debug/eda` predates batches A and D.

---

## r2-schematic, slice B-hier-io: hierarchical sheets and the .kicad_sch reader/writer

Reviewer: sub-reviewer `B-hier-io` (findings only, no code changed). Main's checkout at `5cca5fff`.
KiCad source: `/home/team_braneboard_com/src/kicad-src-8303b2ad`.

**Binary used.** `~/work/agentic-eda/target/debug/eda` is from 2026-10-23, which is before the nested-shared-sheet commits (97496dc2, db432823 on 10-26). So the practical checks used `~/work/ds-agentic-eda/target/debug/eda`. It was built 2026-10-26 23:49 at `4c7fb03f`, the DeepSeek lane's head, which main merged as 5cca5fff, so it contains both commits. I only ran it on scratch copies.

The round-trip script is `/tmp/findings-r2-schematic.x5lr/B-hier-io/rt.sh <qa dir> <root file> <name>`. It does three things:
- runs `eda import-kicad` on a copy of the QA folder;
- runs `eda board erc -C out`, which writes the derived `.kicad/*.kicad_sch`;
- runs `kicad-cli sch export netlist --format kicadxml` on the original and on the derived file. `cmp.py` then compares the components with their sheet paths, the nets of two or more pins, and the `<sheet>` list.

Each QA case left its scratch output in `/tmp/findings-r2-schematic.x5lr/B-hier-io/<name>/`: `i10926b`, `ha`, `complex_hierarchy`, `legacy`, `test_hier_renaming`, `hierarchical_component_classes` and `chs`.

---

### B-H1. `eda import-kicad x.kicad_sch` (Cmd::ReplaceSchematic) drops every per-path reference: each placement of a shared screen gets the first one's references (high)

**Fixed** on task-r2-schematic: 8613ea1a (the overrides move with their symbol and the first path's reference is the symbol's id)

**What differs.**
- `Board::replace_schematic` (`crates/ops/src/import_foreign.rs:86-90`) calls `crate::sch_sheets::place_by_box_corner(sec, ..)` on every screen.
  - `place_by_box_corner` (`crates/ops/src/sch_sheets.rs:649-663`) moves each `SymbolInstance::at`, from the library origin to the corner of its box.
  - It does not move `sec.instance_overrides[..].at`. A `SymbolPathOverride` names its symbol by `at` and `unit` (`sheet_list::override_under`, `crates/model/src/sheet_list.rs:200`).
- `Board::apply` then calls `tidy_shared_screens(before)` (`crates/ops/src/lib.rs:2718-2720`). Before the command the design had no shared screens, so `before` is empty.
  - In `retain_mut` (`sch_sheets.rs:323-337`), `here.contains(&(o.at, o.unit))` is false for every override, because the symbol moved.
  - `was` is empty, so `return false`: **every override is deleted**.
- The import also runs with `ImportOptions { unique_references: true }` (`crates/kicad/src/sheet_file.rs:27`).
  - A symbol whose `Reference` property is unannotated or stale (`U?`) becomes `U?1`. It no longer has an override to give it its per-path reference.
  - The placeholder counter is local to one `import_kicad_sch_with` call (`sch_import.rs:215-241`). So two files of one tree both mint `U?1`, and the model then holds two parts named `U?1`. That half is medium on its own.
- **KiCad.** `SCH_SYMBOL::GetRef( const SCH_SHEET_PATH* )` looks the reference up in `m_instances` by the whole `aSheet->Path()` (`sch_symbol.cpp:876-899`). `parseSchematicSymbol` `T_instances` adds one `SCH_SYMBOL_INSTANCE` per `(path ..)`, and `AddHierarchicalReference` keeps them all (`sch_symbol.cpp:851-873`). The `Reference` property is only a default: "If it was not found in m_Paths array, then see if it is in m_Field[REFERENCE]".
- **The PARITY claim does not hold through this path.** Section 41 ("A schematic brought in whole ... **fixed**: its screens go into the design's frame as Import Sheet's do (`place_by_box_corner` ..)") and section 42 ("QA `issue10926_1` ... now trace as kicad-cli reads them") are both proved by `qa_traced`. That test calls `import_kicad_sch_tree` directly: no `ReplaceSchematic`, no `place_by_box_corner` and no `unique_references`. The command users and `eda import-kicad` actually run is not covered. The only slow test of it (`slow_an_imported_kicad_schematic_derives_one_kicad_cli_reads_as_the_original`) uses `bus_connection`, which has no shared screen.

**How to see it.** I confirmed it with the binary on six QA hierarchies (`rt.sh`):
- **`issue10926_1`:** KiCad gives U1..U6 on six paths. Ours gives `U?1` on all six, and the design has no `instance_overrides` at all (`out/design.json`). kicad-cli's netlist of ours merges them, and the nets `U1.2-U5.2`, `U2.2-U3.2` and `U4.2-U6.2` are gone.
- **`netlists/hierarchy_aliases`:** KiCad gives R3/R4 on `/Second Subsheet/Sub2/`. Ours gives R1/R2 there (duplicates), and the net `R1.1 R2.1 R3.1 R4.1` becomes `R1.1 R2.1`.
- **`netlists/complex_hierarchy` and `legacy_hierarchy` (KiCad 6, root `symbol_instances`):** the second placement repeats the first's references (C201.. on both). The derived ERC shows `different_unit_net`, `missing_unit`, `missing_power_pin` and `missing_input_pin`, which the original does not have.
- **`netlists/hierarchical_component_classes`, `test_hier_renaming`:** the same.

Commands:
```
rt.sh $KICAD_SRC_DIR/qa/data/eeschema issue10926_1.kicad_sch i10926b
rt.sh $KICAD_SRC_DIR/qa/data/eeschema/netlists/hierarchy_aliases hierarchy_aliases.kicad_sch ha
```

Tests to write:
- `crates/ops/src/import_foreign.rs`, `replace_schematic_keeps_the_references_of_each_sheet_path`. Load `issue10926_1` with `eda_kicad::load_sheet_file`, apply `Cmd::ReplaceSchematic`, and assert that `walk()` gives U1..U6 (`reference_under` on each path) and that `instance_overrides` is not empty.
- A slow test with the same body as `slow_an_imported_kicad_schematic_derives_one_kicad_cli_reads_as_the_original`, run on `hierarchy_aliases`: kicad-cli's nets of the derived file equal the original's.

Two notes for the fix:
- `place_by_box_corner` must move each override's `at` with its symbol. Equivalently, it could fill `SharedScreens` before the command.
- Once the overrides survive, every path of an imported shared screen has an override, the first path included. Annotate and Edit Reference rename only `SymbolInstance::id` (no code outside `sch_sheets.rs` touches `instance_overrides`). So on such a screen they would change nothing that is shown. Either drop the override that equals the symbol's own id when reading, or make the rename verbs write the override of the path in view.

**Severity.** High: wrong connectivity and duplicate references for any KiCad project with a shared screen imported the way users import it.

**Already known?** No. Round 1 and GAPS list the per-path work as done. The place_by_box_corner fix (GAPS item 4, "Also fixed: the schematic `eda import-kicad x.kicad_sch` derives kept the file's frame") is the change that broke it.

### B-H2. Sheet files in a subfolder: the export fails, and screens are keyed by the name the placing sheet gives them, not by the resolved file (high)

**Fixed** on task-r2-schematic: 85b05524

**What differs.**
- **The writer.** `export_schematic` (`crates/kicad-engine/src/lib.rs:546-562`) writes `work.join(name)` for each screen key and never creates the parent folder. A key of `ampli_ht/ampli_ht.kicad_sch` fails with `FAIL kicad_engine_write @ out/.kicad/ampli_ht/ampli_ht.kicad_sch: No such file or directory`. The ERC, the plots and the netlist then fail for the whole board.
- **The writer's doc says "`Sheetfile` names are relative to the root's folder".** KiCad resolves a sheet's file against the folder of the file that places it:
  ```
  fileName.MakeAbsolute( m_currentPath.top() );
  ...
  m_currentPath.push( fileName.GetPath() );
  ```
  (`SCH_IO_KICAD_SEXPR::loadHierarchy`, `sch_io_kicad_sexpr.cpp:200-206`). So `filter.kicad_sch`, placed by `ampli_ht/ampli_ht.kicad_sch`, is `ampli_ht/filter.kicad_sch` to KiCad. Our writer would write it at the root.
- **The reader** (`import_kicad_sch_tree_with`, `crates/kicad/src/sch_import.rs:766-769`) does resolve against the parent's folder. But it de-duplicates screens and files them by the `Sheetfile` text (`read_files.insert(file.clone())`, `sheet_contents.insert(file, ..)`), not by the resolved path. Two consequences:
  - Two different files with the same relative name in different folders become one screen. This is wrong content and wrong connectivity.
  - One file reached under two spellings becomes two screens.
- **KiCad** finds a screen by the full path (`SCH_SHEET::SearchHierarchy( fn.GetFullPath() ..)` in Paste and `LoadSheetFromFile`, `SCH_SCREENS` by file name).
- **The slow test that claims this QA project traces alike is vacuous.** `qa_traced` (`crates/cli/src/sch_hierarchy_tests.rs:1240-1260`) copies only the files at the root folder's level (`read_dir(..).filter(is_file)`). So `netlists/complex_hierarchy_shared`, which is in `QA_TRACED_ALIKE`, is compared with kicad-cli's netlist of a root whose sheets are missing.

**How to see it.** Confirmed: `cp -r $KICAD_SRC_DIR/qa/data/eeschema/netlists/complex_hierarchy_shared src`, then `eda import-kicad src/complex_hierarchy.kicad_sch -o out` and `eda board erc -C out`. The output is the `kicad_engine_write` failure above; only `board.kicad_sch` is written. Scratch: `/tmp/findings-r2-schematic.x5lr/B-hier-io/chs/`.

Tests to write:
- `export_schematic_writes_a_sheet_in_a_subfolder`: export the design of `complex_hierarchy_shared`, then assert that `.kicad/ampli_ht/ampli_ht.kicad_sch` and `.kicad/ampli_ht/filter.kicad_sch` exist and that kicad-cli's netlist has 5 sheets.
- A reader test: two folders `a/` and `b/`, each with its own `x.kicad_sch`, placed by `a.kicad_sch` and `b.kicad_sch`. Assert two screens.

**Severity.** High: the export fails outright on one of KiCad's own QA layouts, and a file can be read as another.

**Already known?** No.

### B-M1. The walk visits a screen's sheets in stored order; KiCad sorts them by position (x, then y, then uuid) (medium)

**Fixed** on main by task-hierarchy-pages (962541a8: `sheet_list::sheets_in_order`)

**What differs.**
- **Ours.** `sheet_list::walk` (`crates/model/src/sheet_list.rs`, `visit`: `for child in &sch.sheets`) and the writer's own walk (`export_kicad_sch_tree`, `crates/kicad/src/lib.rs`, `fn walk`: `for s in &sch.sheets`) go in the order the sheets were read or created.
- **KiCad.** `SCH_SHEET_LIST::BuildSheetList` takes the children from `SCH_SCREEN::GetSheets`, which sorts them (`sch_screen.cpp:1213-1233`):
  ```
  if( a->GetPosition().x == b->GetPosition().x ) { if( a->GetPosition().y == b->GetPosition().y ) return a->m_Uuid < b->m_Uuid; return a->GetPosition().y < b->GetPosition().y; } else return a->GetPosition().x < b->GetPosition().x;
  ```
- **Every use of the virtual page number is off when the stored order differs:**
  - the writer's automatic page numbers, and `SetInitialPageNumbers` for a file without pages;
  - the `SortByPageNumbers` tie-break;
  - Annotate's "sheet number x 100/1000" when pages are unset;
  - Next/Previous Sheet;
  - which sheet path is "first": the one that keeps `SymbolInstance::id`, which `number_later_placements` and `tidy_shared_screens` skip.

**How to see it.** Confirmed on `netlists/complex_hierarchy` (KiCad 6, no page numbers). kicad-cli numbers the original `2=/ampli_ht_vertical/` (x=71.12) and `3=/ampli_ht_horizontal/` (x=154.94). Our derived file writes horizontal page 2 and vertical page 3, so its `<sheet>` order is swapped.

Test to write: `walk_orders_a_screens_sheets_by_x_then_y`. Store two placements with the right one (x = 150 mm) first and the left one second; assert that `walk()[1]` is the left one.

**Severity.** Medium.

**Already known?** No.

### B-M2. The writer's fallback page number collides with pages the user set; the doc's claim that KiCad falls back to the virtual page is wrong (medium)

**Open** after task-r2-schematic: page numbering, the task-hierarchy-pages lane's area (main 962541a8 numbers new paths; an imported file with some pages set still falls back to walk numbers)

**What differs.**
- **Ours.** `export_kicad_sch_tree` (`crates/kicad/src/lib.rs:674-684`) writes `s.page` when it is set. Otherwise it writes the sheet's place in its own walk (`w.pages + 1`), whether or not another sheet already has that number as a page set by the user.
- **The doc.** `SheetEntry::effective_page` and the module doc (`sheet_list.rs`: "else its virtual page number (`GetPageNumber` falls back that way)") claim KiCad does the same.
- **KiCad.** `SCH_SHEET_PATH::GetPageNumber` returns the instance's page or `wxEmptyString` (`sch_sheet_path.cpp:731-745`, `SCH_SHEET::getPageNumber`, `sch_sheet.cpp:1794-1808`). There is no fallback.
  - Pages are only made up when *every* page is empty (`files-io.cpp:456`, `if( sheetList.AllSheetPageNumbersEmpty() ) sheetList.SetInitialPageNumbers();`).
  - A new or pasted sheet gets the first page number not in use: `while( hierarchy.PageNumberExists( pageNum ) ) pageNum = wxString::Format( "%d", ++page );` (`sch_editor_control.cpp::Paste`).

**How to see it.** Confirmed on `netlists/hierarchy_aliases`. The original has root sheets on pages "2" and "3", and the nested `Sub2` has no page. Ours writes `Sub2` as page "3" on `/First Subsheet/` and page "5" on `/Second Subsheet/`, so page "3" is used twice. kicad-cli's `<sheet>` order of the derived file differs from the original's (`3=/First Subsheet/Sub2/` against `3=/Second Subsheet/`).

Test to write: a design with root sheets on pages "2" and "3" and a nested sheet with no page. Assert that the written `(page ..)` values are unique.

**Severity.** Medium.

**Already known?** Partly. GAPS item 4 lists "first-free page numbers" as left, for new sheets. The collision on write of an imported file and the false doc claim are not listed.

### B-M3. KiCad 6 / early-7 root `(sheet_instances ..)` page numbers are not read (medium)

**Fixed** on task-r2-schematic: 66d47b36

**What differs.**
- **Ours.** The reader takes a sheet's page only from that sheet's own `(instances (project (path (page ..))))` (`sch_import.rs:553-561`). It never reads the root file's `(sheet_instances (path "/<sheet>/.." (page ..)) ..)`; `sheet_instances` appears in `sch_import.rs` only in a test fixture.
- **KiCad.** `parseSchSheetInstances` (`sch_io_kicad_sexpr_parser.cpp:2531-2618`) collects them, putting `m_rootUuid` in front for files older than 20221002, and turning an empty page into `#`: "Empty page numbers are not permitted". Then the loaders apply them:
  - `files-io.cpp:703-704`: `if( rootScreen && rootScreen->GetFileFormatVersionAtLoad() < 20221110 ) sheetList.UpdateSheetInstanceData( rootScreen->GetSheetInstances());`
  - `eeschema_helpers.cpp:143` (kicad-cli) does the same, unconditionally.
- So a KiCad 6 project's own page numbers ("A", "10", a reordering) are lost. They are replaced by walk numbers (M1 and M2 then apply).

**How to see it.** QA examples with such blocks:
- `netlists/complex_hierarchy_shared` (pages 2..5);
- `prefix_bus_alias`;
- `group_bus_matching`;
- `test_hier_renaming` (pages "", which KiCad reads as "#").

Test to write: a root at version 20211123 whose `(sheet_instances (path "/<s>/" (page "7")))` names one sheet. Assert that `SheetInstance::page == "7"`.

**Severity.** Medium. The page numbers are data the user set.

**Already known?** No. The section 39 review fixed only the root `symbol_instances` half.

### B-M4. One unit per symbol: a per-path unit in the file is lost, and so is that path's reference (medium)

**Open** after task-r2-schematic: a per-path unit needs the tracer and the writer to draw a symbol as another unit per path

**What differs.**
- **Ours.**
  - The model keeps `SymbolInstance::unit` only.
  - The reader files each `(path .. (unit N))` override with that path's unit (`parse_instance_overrides`, `sch_import.rs:1067-1083`).
  - `override_under` matches `o.unit == sym.unit` (`sheet_list.rs:200`). An override whose unit differs from the symbol's `(unit ..)` (the ordinal path's unit) is never found: that path falls back to `sym.id`, with the ordinal unit.
  - The writer writes `(unit {sym.unit})` on every path (`crates/kicad/src/lib.rs:440`).
- **KiCad** keeps a unit per path. `SCH_SYMBOL::SetUnitSelection( const SCH_SHEET_PATH*, int )` is set by:
  - Symbol Properties and Field Properties (`dialog_field_properties.cpp:661`);
  - Annotate (`SCH_REFERENCE::Annotate`, `sch_reference_list.cpp:820-822`: `m_rootSymbol->SetUnitSelection( &m_sheetPath, m_unit );`);
  - back-annotation and the Eagle importer.

  Its saveSymbol writes `instance.m_Unit` per `(path ..)`. A shared channel sheet using U1A on one path and U1B on another is valid KiCad.

**How to see it.** Code-confirmed only; no KiCad QA file has differing units per path (searched with `units.py`). Test to write: in `sch_import.rs`, a screen placed twice whose symbol has `(unit 1)` and instances `/r/a (reference "U1") (unit 1)` and `/r/b (reference "U1") (unit 2)`. Assert that the second path's pins are unit 2's. Today it fails, and the reference on `/r/b` also falls back.

**Severity.** Medium: wrong pins, so wrong connectivity, for that layout.

**Already known?** No.

### B-M5. The clipboard does not know the sheet path it was copied from (medium)

**Fixed** on task-r2-schematic: d06ab0a8 (the copy half (D-C1); a pasted sheet still takes its references from the first path that shows the copied screen)

**What differs.**
- **Ours.** `write_clipboard` (`crates/kicad/src/sch_clipboard.rs:215-222`, `CopyInput` has no sheet path) writes each symbol under its own id. The reader clears `instance_overrides` (`sch_clipboard.rs:121`).
  - Copy a symbol while viewing the second placement of a shared screen, and the clipboard carries the first placement's reference.
  - Paste of a sheet takes the "copied" references from the *first* walk entry whose last id is the sheet's (`share_pasted_sheet`, `sch_sheets.rs`, `copied_from = before.iter().skip(1).find(..)`), not from the path it was copied on.
- **KiCad.** `Format( SCH_SELECTION*, SCH_SHEET_PATH* aSelectionPath, .., aForClipboard )` writes the Reference field from the instance on `aRelativePath`, and makes the instance paths relative to it (`saveSymbol`):
  ```
  else if( aForClipboard && aSymbol->GetInstances().size() && aRelativePath && ( id == FIELD_T::REFERENCE ) ) { ... if( aSymbol->GetInstance( instance, aRelativePath->Path() ) ) field->SetText( instance.m_Reference ); }
  ```
  and `tmp.MakeRelativeTo( aRelativePath->Path() )`. `updatePastedSymbol` takes the instance whose path `EndsWith( aClipPath )`.

**How to see it.** Paste Special with "Keep existing annotations" of R1 copied while viewing placement 2 of a shared screen, where that placement shows R3. KiCad pastes R3; ours pastes R1. With the default mode, `ReannotateDuplicates` renumbers both the same way, so only Keep shows the difference.

Test to write: `crates/ops/src/sch_hierarchy_tests.rs`, `a_copy_on_the_second_placement_keeps_its_reference_on_paste_keep`.

**Severity.** Medium: a wrong reference in a documented mode.

**Already known?** No. PARITY-sch.md section 39 says "The verbs still name a symbol by its id" for edits, not for the clipboard.

### B-L1. A sheet's user fields and its attributes are dropped by the reader and never written (low)

**Fixed** on task-r2-schematic: 52ac71ac (the user fields, which fixed QA ERC_dynamic_power_symbol_test's derived file; the DNP / BOM / board flags and the stroke and fill are open)

**What differs.**
- **Ours.** The sheet reader (`sch_import.rs:530-563`) keeps uuid, `Sheetname`, `Sheetfile`, place, size, pins and the first page. `write_sheet_item` (`crates/kicad/src/lib.rs:720-772`) writes only those, plus a fixed stroke and fill.
- **KiCad** writes, in `saveSheet`:
  - `exclude_from_sim`, `in_bom`, `on_board` and `dnp`;
  - the stroke width and colour, and the fill colour;
  - every field (`for( SCH_FIELD& field : aSheet->GetFields() ) saveField( &field );`).
- **What is lost:**
  - A user field on a sheet, which `SCH_SHEET::ResolveTextVar` resolves for the sheet's contents as `${<field name>}` (`sch_sheet.cpp:382-392`).
  - A sheet marked DNP or excluded from the board or BOM. `SCH_SHEET_PATH::GetExcludedFromBoard()` and `GetDNP()` apply this to every symbol below (`sch_sheet_path.cpp:359-415`; `netlist_exporter_xml.cpp:313-316`). So the derived file puts a DNP sheet's parts on the board.

**Severity.** Low to medium.

**Already known?** Partly. PARITY-sch.md section 25 lists "a sheet's own DNP flag (`ResolveDNP` reads it; the IR has none)" and GAPS lists "the sheet's fill colour". The user fields and the board/BOM exclusion are not listed.

### B-L2. Symbol `(in_pos_files no)` is dropped (low)

**Open** after task-r2-schematic: needs a symbol flag in the IR

**What differs.**
- **Ours.** The reader reads only `dnp`, `in_bom`, `on_board` and `exclude_from_sim` (`sch_import.rs:191-193`). The writer never writes `in_pos_files`.
- **KiCad** writes `KICAD_FORMAT::FormatBool( m_out, "in_pos_files", !aSymbol->GetExcludedFromPosFiles() );` (`saveSymbol`).

So an "exclude from position files" symbol comes back included.

**Severity.** Low.

**Already known?** No.

### B-L3. Symbol uuids are minted from the reference, so an imported file's uuids are not kept and a rename changes the uuid (low, impact unconfirmed)

**Open** after task-r2-schematic: not changed

**What differs.**
- **Ours.** `duid_for(&format!("sym:{}", sym.id), &sym.id)` (`crates/kicad/src/lib.rs:372`). The file's `(uuid ..)` is read only for the uuid -> id map.
- **KiCad** keeps `m_Uuid`, which is the last element of a footprint's `(path ..)` in the `.kicad_pcb`.

The derived pair (board and schematic) stays consistent (`symbol_links`). But the original KiCad board of an imported project no longer links to the derived schematic, and after an Edit Reference KiCad's Update PCB sees a new symbol (unless relinking by reference).

**Severity.** Low.

**Already known?** No. Zones and footprints keep their uuids per GAPS; schematic symbols do not.

### B-L4. Writer cosmetics against `Format` (low)

**Open** after task-r2-schematic: not changed

**What differs.**
- **`(sheet_instances (path "/" (page "1")))` and `(embedded_fonts no)` are written into every screen's file** (`lib.rs:692-697`, `embedded::write_owner(.., true)`).
  - KiCad writes the root instance only when `aSheet->HasRootInstance()`, and `embedded_fonts` only in "the schematic's first top-level sheet" (`Format`, `sch_io_kicad_sexpr.cpp`).
  - The root's own page is always "1".
- **Power symbols are written `(in_bom no) (on_board no)`** (`lib.rs:468`). KiCad copies the library's flags (`power:` symbols say `yes`).
- **A sheet pin's side is taken from its place, not from the file's angle.**
  - The reader ignores `(at x y angle)`'s angle. The writer picks the side by testing x before y (`lib.rs:745-753`).
  - KiCad's `parseSchSheetPin` sets the side from the angle (`SetSide` snaps the pin to that edge).
  - So a pin on a sheet's corner that KiCad has on TOP comes back LEFT. Its text turns, but its point does not move.
- **An empty title or date gets the export's title (the stem) and today's date, and a title block without comments gets three provenance comments** (`lib.rs:253-271`). KiCad writes an empty title block as it is.
- **Of two `(instances (project ..))` entries for one path, our reader uses the first** (`override_under` finds the first). KiCad's `AddHierarchicalReference` calls `RemoveInstance( aInstance.m_Path )` first, so the last wins.
- **The reader drops page "1" on a sheet** (`sch_import.rs:559`, "what this project's own writer puts"). A KiCad file whose sub-sheet really is page 1 loses it.

**Severity.** Low.

**Already known?** No; "instance paths in walk order" is known, and these are not.

---

### Checked and found to match

- **`SymbolPathOverride::sheet_path` and `reference_under`** (`sheet_list.rs:185-201`) against `SCH_SYMBOL::GetRef` (`sch_symbol.cpp:876`): lookup by the whole path, falling back to the symbol's own reference. Matches; per-path unit is the exception (M4).
- **`parse_instance_overrides`** (`sch_import.rs:1067`) against `parseSchematicSymbol` `T_instances`: every project's paths are read and the root uuid is stripped. Instances of other projects are read too, as KiCad reads them; they are later pruned here, while KiCad keeps other projects' instances on save. That only matters for a file shared between projects, which the derived files are not.
- **`legacy_symbol_instances`** against `parseSchSymbolInstances` and `UpdateSymbolInstanceData`: the path without the root uuid, `[sheets.., sheet]`, and the unit taken when the file's symbol has none. Matches.
- **`prune_orphaned_overrides`** against `SCH_SCREENS::PruneOrphanedSymbolInstances`: an override is kept while its whole path shows its screen. Matches for a single project.
- **`migrate_override_paths`** (incl. db432823): the old lookup was "last id"; the migration files each old key under every path that ends in it, and the first of two duplicates wins as the old `find` did. Logic correct. It runs only when a length-1 key is not a path of its screen.
- **Writer `saveSymbol` `(instances)`**: one `(path ..)` per sheet path of the screen, with that path's reference. **Writer `saveSheet` `(instances)`**: one `(path parent (page ..))` per path of the screen the sheet is drawn on, the root's instance excluded. Both match, apart from the known items (sort order, project name "eda-kicad", one page per `SheetInstance`).
- **Sheet pin angle to side, and justification**, against `getSheetPinAngle` and `SCH_SHEET_PIN::SetSide`: 0 right, 90 top, 180 left, 270 bottom; right-justified for right and top. Matches (corner case in L4).
- **Reader `(mirror x|y)`, `(body_style)` / `(convert)`, `(exclude_from_sim)`, `(in_bom)`, `(on_board)`, `(dnp)`, `(locked)` and `(fields_autoplaced)`** against `parseSchematicSymbol`. Match (except `in_pos_files`, L2).
- **`share_pasted_sheet`** against `Paste` / `updatePastedSheet` / `updatePastedSymbol`:
  - the new sheet is added on every path of the destination screen (`FindAllSheetsForScreen`, sorted by page);
  - recursion through the sub-sheets;
  - each new path's references come from the instance under the clipboard path;
  - numbering with `ReannotateDuplicates`, each path counting as used;
  - power symbols kept.

  Matches except the copy path (M5) and the first-free page (known).
- **`number_later_placements`** (Import Sheet of a tree showing a screen twice): the later paths are numbered against the whole design. Matches `updatePastedSheet` per path.
- **db432823's Annotate change**: other paths' references count as taken (`SCH_SHEET_LIST::GetSymbols`, one `SCH_REFERENCE` per path). It prevents duplicates; "differs, narrowed" is already documented.
- **Title block reader** (`import_title_block`) against `TITLE_BLOCK::Format`: comment numbers kept, A4 default. Matches; the writer's fallbacks are in L4.
- **Practical check: `netlists/test_multiunit_reannotate_3` through `eda import-kicad`**: the same components, nets and sheets as kicad-cli's netlist of the original.

### Summary

| ID | Finding | Severity | Known? |
|---|---|---|---|
| H1 | `eda import-kicad` / ReplaceSchematic drops every per-path reference (`place_by_box_corner` moves symbols, not overrides; `tidy_shared_screens` deletes them); `U?` placeholders numbered per file | high | no (the section 41 fix caused it; the section 42 proof bypasses it) |
| H2 | Sheet files in a subfolder: the export fails (no parent folder), screens keyed by relative name, not resolved path; `complex_hierarchy_shared` "alike" test vacuous | high | no |
| M1 | The walk does not sort a screen's sheets by x, y, uuid (`SCH_SCREEN::GetSheets`) | medium | no |
| M2 | The fallback page collides with user pages; false "GetPageNumber falls back" claim | medium | partly (first-free pages) |
| M3 | KiCad 6 root `(sheet_instances)` pages not read | medium | no |
| M4 | Per-path unit not representable; such a path also loses its reference | medium | no |
| M5 | The clipboard has no copy path: a copy from placement 2 carries placement 1's references | medium | no |
| L1 | A sheet's user fields, DNP and board/BOM exclusions dropped | low-medium | partly |
| L2 | `in_pos_files` dropped | low | no |
| L3 | Symbol uuids minted from the reference | low | no |
| L4 | Writer cosmetics (root instance and embedded_fonts in every file, power flags, corner pin side, title fallbacks, duplicate-path order, page "1") | low | no |

Counts: high 2, medium 5, low 4.

---

## r2-schematic, slice C-edit: schematic edit tools, labels, fields, symbol placement

Scope: Move / Drag / Rotate / Mirror / Align (`crates/ops/src/sch_move.rs`, `sch_drag.rs`, `sch_scene.rs`), wires and junctions
(`lib.rs::add_wire`, `Scene::analyze_point` / `clean_up` / `trim_wire`), labels and AutoRotate (`sch_labels.rs`, `kicad-port/schAutoRotate.ts`),
Autoplace Fields (`crates/engine/src/fields.rs`, `fields_edit.rs`, `crates/ops/src/sch_fields.rs`), symbol placement (`lib.rs::add_symbol`,
`SchematicView.tsx`), Align (`kicad-port/schAlign.ts`). KiCad: `/home/team_braneboard_com/src/kicad-src-8303b2ad/eeschema` (`tools/sch_move_tool.cpp`,
`tools/sch_edit_tool.cpp`, `tools/sch_line_wire_bus_tool.cpp`, `tools/sch_drawing_tools.cpp`, `tools/sch_align_tool.cpp`, `sch_item_alignment.cpp`,
`bus-wire-junction.cpp`, `schematic.cpp`, `junction_helpers.cpp`, `sch_screen.cpp`, `autoplace_fields.cpp`, `sch_label.cpp`, `sch_text.cpp`, `sch_sheet_pin.cpp`,
`symb_transforms_utils.cpp`, `sch_edit_frame.cpp`).

How it was checked: code read side by side, function by function. The move verbs have no CLI entry point (`eda board` has no `sch_move`; they go
through the studio server, which this review may not start), so nothing was run through `eda`. One claim was measured against KiCad's own files: the
places KiCad's Autoplace Fields gives power symbols, read from every `power:*` symbol in `qa/data/eeschema/**/*.kicad_sch` (script
`/tmp/findings-r2-schematic.x5lr/C-edit/pwr.py`, see M3). Code merged since 2026-10-24 on these paths is the nanometre typing and the
nested-shared-screen work; none of it changed the logic reviewed here.

---

### C-H1. A dragged wire or sheet pin loses the sheet pin it ends on: `m_specialCaseSheetPins` is never filled (high)

**Fixed** on task-r2-schematic: 89e27c22

**What differs.** KiCad keeps a wire's end on a sheet pin that moves along its sheet's border. When it builds the drag cache, `getConnectedItems` runs
on *both* ends of every line in the selection (drag additions included: they are in the selection by then, and `AddItemToSel` gives them `SELECTED`).
At the end that is on a selected sheet pin it records the pair:

```
// sch_move_tool.cpp::setupItemsForDrag
for( const VECTOR2I& point : line->GetConnectionPoints() )
    getConnectedItems( line, point, m_lineConnectionCache[line] );

// sch_move_tool.cpp::getConnectedItems, case SCH_SHEET_T
if( pin->IsConnected( aPoint ) )
{
    if( pin->IsSelected() )
        m_specialCaseSheetPins[pin] = { line, line->GetStartPoint() == aPoint };
```

and `performItemMove`, after every item it moves, puts that end on the pin where `ConstrainOnEdge` left it ("Update any lines connected to sheet
pins to the sheet pin's location (which may not exactly follow the splitDelta as the pins are constrained along the sheet edges)").

Ours calls `get_connected_items` only for the end of a partly selected line that stays put (`crates/ops/src/sch_drag.rs:155`, `if start != end`,
with the comment that only that end matters). The only place that fills `st.special_pins` (`sch_drag.rs:211`) needs a selected sheet pin at that
fixed end, which cannot happen (a selected pin flags the wire end on it, so that end moves). So `special_pins` is always empty, the loop at
`sch_drag.rs:509` does nothing, and the wire end moves by the raw offset while the pin is held on the border (`move_item`, `sch_drag.rs:472`,
`constrain_on_edge`).

The round-1 "Difference" note on PARITY-sch.md row `G: drag` covers a free wire end getting no bend. It does not cover this case, where the wire ends up
off the pin.

**How to see it.** Two everyday cases:
1. A wire runs from a label at (80, 60) to sheet pin `IN` on the left border x = 100 (the sheet in `sch_sheet_pin_tests.rs::design()`). Pick the wire
   whole and drag it by (+2.54, 0). In KiCad the pin is drag-added and stays at x = 100, and the wire's right end follows it: the wire is now
   (82.54, 60)-(100, 60). Ours: the pin stays at (100, 60), the wire becomes (82.54, 60)-(102.54, 60), and its end is inside the sheet, off the pin.
   The net SIG no longer reaches the sheet.
2. Drag the pin alone by (0, 25.4), past the bottom corner (the sheet's bottom is at y = 80). The pin clamps to (100, 80). Ours puts the wire end at (100, 85.4).

Test: `sch_sheet_pin_tests.rs::dragging_a_wire_into_a_sheet_pin_sideways_keeps_it_on_the_pin`: input `design()`, `drag(&["wire_in"], mm(2.54), Nm::ZERO)`.
Assert that a wire ends at `pin_at(.., "shpin_in")` and that `nets(..)["R1.1"] == "/SIG"`. A second case: `drag(&["shpin_in"], Nm::ZERO, mm(25.4))`.
Assert the pin is at (100, 80) and a wire ends there.

**Severity.** High: it breaks connectivity on a common edit.

**Already known?** Not listed (GAPS item 1, round-1 findings and the PARITY rows do not mention it).

**Fix hint.** Build the cache for both ends, as the C++ does: for every line in the selection call `get_connected_items` at `a` and at `b`, with "selected"
counting drag additions too (`is_user_selected || has_by_drag`). That also brings back KiCad's bends for a free far end, the known difference: decide
whether that row should stay a deliberate difference, and if so record the sheet pins for both ends anyway.

---

### C-H2. A wire drawn across a two-pin part is not trimmed between the pins; ours puts a junction on each pin and shorts the part (high)

**Fixed** on task-r2-schematic: 9decd1b9

**What differs.** `SCH_LINE_WIRE_BUS_TOOL::finishSegments` does three things `Board::add_wire` (`crates/ops/src/lib.rs:4723`) does not:

```
// Remove segments backtracking over others
simplifyWireList();
...
// Correct and remove segments that need to be merged.
m_frame->Schematic().CleanUp( &aCommit );

for( SCH_ITEM* symbol : symbols )
{
    std::vector<VECTOR2I> pts = symbol->GetConnectionPoints();
    if( pts.size() > 2 )
        continue;
    for( auto pt = pts.begin(); pt != pts.end(); pt++ )
        for( auto secondPt = pt + 1; secondPt != pts.end(); secondPt++ )
            m_frame->TrimWire( &aCommit, *pt, *secondPt );
}

for( const VECTOR2I& pt : new_ends )
    if( m_frame->GetScreen()->IsExplicitJunctionNeeded( pt ) )
        AddJunction( &aCommit, m_frame->GetScreen(), pt );
```

`add_wire` only runs `add_junctions_if_needed`. When a new wire passes over both pins of a resistor:
- KiCad: `TrimWire` (`bus-wire-junction.cpp::SCH_EDIT_FRAME::TrimWire`) removes the piece between the two pins. At each pin one wire end and the pin
  meet (two exit angles), so no junction is added. The result is the part in series.
- Ours: each pin lies inside the wire. `analyze_point` there sees the pin plus the wire's two directions through it (three exit angles), so
  `add_junction` adds a dot and breaks the wire at both pins. The piece between the pins stays, and our tracer (`crates/engine/src/nets.rs:327`, "A
  point landing on the interior of a wire joins that wire") puts both pins on one net: the part is shorted.

Without the `CleanUp`, a new wire that overlaps or doubles back over an existing collinear wire also stays as two overlapping wires; KiCad merges them.

**How to see it.** `sch_move_tests.rs`-style test `a_wire_drawn_through_a_resistor_is_cut_between_its_pins`: an R at (20.32, 25.4) (pins at y 21.59 and
29.21). `draw(s, &[p(mm(20.32), mm(15.0)), p(mm(20.32), mm(35.0))])`. Assert there are no junctions, that no wire segment covers (20.32, 25.4), and that
the traced nets put R.1 and R.2 on different nets. Today: two junctions, and R.1 and R.2 on one net.

**Severity.** High: wrong connectivity, and a file KiCad reads differently (its pins are not wire ends, so in KiCad they are unconnected).

**Already known?** Not listed. PARITY section 2's `W` row is "partial" for its snapping only.

---

### C-H3. Placing a symbol on a wire does not trim the wire or add junctions (`PlaceSymbol` does both) (high)

**Fixed** on task-r2-schematic: 9decd1b9 (symbols and power symbols; the label half is **not a bug**: `TwoClickPlace` runs neither TrimOverLappingWires nor AddJunctionsIfNeeded, in 8303b2ad and 25ec2335)

**What differs.** `SCH_DRAWING_TOOLS::PlaceSymbol`, on the placing click:

```
lwbTool->TrimOverLappingWires( &commit, &m_selectionTool->GetSelection() );
lwbTool->AddJunctionsIfNeeded( &commit, &m_selectionTool->GetSelection() );
commit.Push( _( "Place Symbol" ) );
```

`Board::add_symbol` (`crates/ops/src/lib.rs:5055`) only pushes the `SymbolInstance`. The studio (`SchematicView.tsx:770`, `op: "add_symbol"`) sends
nothing after it. Dropping a resistor onto a wire, the common way to insert it in series, leaves the wire running under the body through both pins.
Our tracer then shorts the part, and kicad-cli sees two pins on a wire's interior: no connection, and no junction.

`Cmd::SchMove` already does this for a moved symbol (`finalize` calls `trim_overlapping_wires`); only placement is missing it. The same holds for
`add_power_symbol` and `add_label` on a wire's middle (`AddJunctionsIfNeeded`).

**How to see it.** Test `placing_a_resistor_on_a_wire_cuts_it_between_the_pins`: a wire (20.32, 10)-(20.32, 40), then
`Cmd::AddSymbol { id: "R1", lib_id: "Device:R", at: (20.32, 25.4), .. }`. Assert the wire is split into (20.32, 10)-(20.32, 21.59) and
(20.32, 29.21)-(20.32, 40), and that R1.1 and R1.2 are on different nets.

**Severity.** High: wrong connectivity.

**Already known?** Not listed (PARITY section 3, `A` row: "done as KiCad's chooser" says nothing about what happens to the wire).

---

### C-M1. `clean_up` merges collinear wires whatever their stroke, and a wire split into two runs loses its stroke on the second run (medium)

**Fixed** on task-r2-schematic: c539e423

**What differs.** `SCHEMATIC::CleanUp` merges two lines only when they have the same stroke and layer:

```
if( !secondLine->IsParallel( firstLine ) || !secondLine->IsStrokeEquivalent( firstLine )
    || secondLine->GetLayer() != firstLine->GetLayer() )
    continue;
```

`Scene::clean_up` (`crates/ops/src/sch_scene.rs:771`) checks direction and bus only. The merged segment keeps the first segment's `from`, so it takes
the first wire's id and stroke (`extras.strokes`, keyed by wire id), and the other wire's width, style and colour are lost. `clean_up` runs over the
whole sheet after every move, drag, turn, mirror and align, so unrelated wires merge too.

Separately, `Scene::finish` (`sch_scene.rs:882`) gives every run after the first of a split polyline `id: String::new()`, so no stroke entry and no
`pins`. KiCad's pieces are separate `SCH_LINE`s, each keeping the stroke (`BreakAt` copies the line).

**How to see it.** Two wires end to end on one line, `w1` with the default stroke and `w2` with a 0.5 mm dashed red stroke. Move any symbol elsewhere
on the sheet. Ours leaves one wire with the default stroke; KiCad leaves both. Second case: a dashed wire (0,0)-(10,0)-(10,10) that a part's two pins
trim in its middle (`trim_wire`). The second run comes back undashed.

Test: `move_keeps_two_wires_of_different_strokes_apart` and `a_split_wire_keeps_its_stroke_on_both_pieces`.

**Severity.** Medium: data loss of a property the user set, and a file KiCad reads differently.

**Already known?** Not listed.

---

### C-M2. A directive label (net class flag) mirrors about the wrong axis (medium)

**Fixed** on task-r2-schematic: 3aba7fed

**What differs.** In the file and in our own drawing, the orientation angle is the spin. 0 is RIGHT, 90 UP, 180 LEFT, 270 BOTTOM
(`parseSchText`: `case 90: SetSpinStyle( SPIN_STYLE::UP )`). `SCH_DIRECTIVE_LABEL::CreateGraphicShape` draws the pole from (0, 0) to
(0, pinLength) and turns it: RIGHT `RotatePoint( aPoint, ANGLE_180 )` (pole up), UP `-ANGLE_90` (pole left), BOTTOM `ANGLE_90` (pole right), LEFT
not turned (pole down). `kicad-port/schItemGeom.ts::directiveShape` and our writer (`sch_extras_io.rs:244`) agree. So 0 and 180 are vertical poles,
90 and 270 horizontal ones.

KiCad mirrors the flag geometrically:
- one directive label: `textItem->MirrorSpinStyle( !vertical )`, which `SCH_DIRECTIVE_LABEL::MirrorSpinStyle` turns into
  `SCH_TEXT::MirrorSpinStyle( !aLeftRight )` ("The 'text' is in fact a graphic shape. For a horizontal 'text', it looks like a vertical shape");
- several items: `SCH_DIRECTIVE_LABEL::MirrorHorizontally` does `SetSpinStyle( GetSpinStyle().MirrorX() )` (UP and BOTTOM swap), and
  `MirrorVertically` does `MirrorY()` (LEFT and RIGHT swap).

So X swaps 90 and 270 (a pole pointing left now points right), and Y swaps 0 and 180.

Ours: `mirror_orientation( o, left_right = !vertical )` (`crates/ops/src/sch_move.rs:915`) swaps 0 and 180 for X, and 90 and 270 for Y, the
plain-label rule, both for one item (`mirror_item_in_place`) and for several (`mirror_graphic`, `sch_move.rs:1150`). X therefore turns an up pole
down and leaves a sideways pole alone.

**How to see it.** Test `a_directive_label_mirrors_its_pole_across_the_mirror_axis`: a `Directive { orientation: 90_000, .. }`, `mirror(&[id], false)`.
Assert orientation 270_000 (ours keeps 90_000). With `orientation: 0`, `mirror(.., true)` should give 180_000 (ours keeps 0).

**Severity.** Medium: wrong result, and the angle written to the file differs from KiCad's.

**Already known?** Not listed. No test covers a directive label's mirror.

---

### C-M3. Power symbols are not placed by Autoplace Fields: a hand-set Value spot, written with `(fields_autoplaced yes)`, and a PWR_FLAG's Value hidden (medium)

**Open** after task-r2-schematic: power symbols still use the library's offsets

**What differs.** KiCad runs the same `AUTOPLACER` for a power symbol as for any symbol (`PlaceSymbol` runs `AutoplaceFields( screen, AUTOPLACE_AUTO )`,
and so do Autoplace Fields `O` and a lone rotate). The power case has its own preferred sides (`getPreferredSides`, `m_is_power_symbol =
!IsInNetlist()`: "For power symbols, we generally want the label at the top first": TOP, BOTTOM, RIGHT, LEFT at 0 degrees), counts hidden pins
(`pinsOnSide`: `if( !each_pin->IsVisible() && !m_is_power_symbol ) continue;`), and rounds to the 50 mil grid.

Ours never runs it for a power symbol:
- `fields::power_value_placement` (`crates/engine/src/fields.rs:740`) uses the library's own offsets (GND 3.81 mm below, a supply 3.556 mm above);
- a symbol turned a quarter gets "its Value read along the row, just past the glyph's tip" (glyph reach + 0.64 mm, right- or left-justified), which
  is our own rule;
- `OwnerCtx::autoplace` for `OwnerKind::Power` (`fields_edit.rs:179`) returns that same spot for `O`;
- `fill_layout` stores it with `fields_autoplaced = Auto`;
- a PWR_FLAG's Value is hidden (`visible: !flag`, `fields.rs:759`). KiCad's library shows it, and autoplace keeps it shown.

Measured on KiCad's QA schematics (every `power:*` symbol with `(fields_autoplaced yes)`):

| symbol, rotation | KiCad Value offset (dx, dy) | ours |
|---|---|---|
| GND, 0 (30 cases) | (0, +5.08), centred | (0, +3.81) |
| VCC / +5V / +1V0, 0 (18) | (0, -5.08) | (0, -3.556) |
| +1V0, 90 | (-3.81, 0), centred, stored angle 90 | right-justified at -(glyph reach + 0.64) |
| +1V0, 270 | (+3.81, 0) | left-justified at +(reach + 0.64) |
| PWR_FLAG, 0 / 180 | (0, -5.08) / (0, +5.08), shown | hidden |

(Unplaced library copies keep 3.81 and 3.556: `power:GND rot 0 -- dy=3.810`, `power:+5V rot 0 -- dy=3.556`.)

**How to see it.** `python3 /tmp/findings-r2-schematic.x5lr/C-edit/pwr.py $(find $KICAD_SRC_DIR/qa/data/eeschema -name '*.kicad_sch') | sort | uniq -c`.
Test `power_fields_are_autoplaced_like_kicad`: GND at (100, 100) rot 0. Assert the `power_placements` Value is at dy = +5.08 mm (and dy = -5.08 for
VCC, dx = -3.81 at rot 90, and a PWR_FLAG Value visible at dy = -5.08).

**Severity.** Medium: the default positions are wrong, and a file says "autoplaced" for fields KiCad would place elsewhere.

**Already known?** Not listed. GAPS item 12 says "`m_AutoplaceFields.enable` is always on", nothing about power symbols.

---

### C-M4. Autoplace Fields counts hidden pins, and its body box leaves out the pin roots (medium)

**Fixed** on task-r2-schematic: 527995b1

**What differs.** `AUTOPLACER::pinsOnSide` and `fieldBoxPlacement::getPinsBox` skip a symbol's invisible pins (`if( !each_pin->IsVisible() &&
!m_is_power_symbol ) continue;`). `m_symbol_bbox = GetBodyBoundingBox()` is `LIB_SYMBOL::GetBodyBoundingBox( .., aIncludePins = false, .. )`, which
still merges the root of every visible pin ("the roots of the pins are always included for symbols that don't have a well-defined body":
`bbox.Merge( pin.GetPinRoot() )`).

Ours: `SymbolGeom::of_corner_symbol` (`crates/engine/src/symgeom.rs`) puts every pin in `geom.pins`, hidden or not (`LibPin::hidden` exists but is
not read). `autoplace_page` counts and boxes them all (`fields.rs:440`, and the `pins_box` loop), and `geom.body` holds only the graphics.

A symbol with hidden power pins (the legacy 74xx/4000 style, hidden VCC on top and GND on the bottom) has "no pins" on top in KiCad, which takes TOP
(or RIGHT when that side is free). Ours sees one pin on each of those sides and falls back to the side with the fewest. A symbol whose pin roots
stick out past its graphics gets a smaller body box in ours, so the fields sit closer to the body.

**How to see it.** Test `autoplace_ignores_hidden_pins`: a `LibSymbol` with a body, one visible pin on the left and one on the right, and a hidden
power-input pin on the top and on the bottom. Preferred order is Right, Top, Left, Bottom; Right has a visible pin, so KiCad takes TOP, the first
side with no visible pin. Ours sees one pin on every side and falls back to fewest pins. Assert the fields sit above the body.

**Severity.** Medium: a wrong default placement.

**Already known?** Not listed.

---

### C-M5. Manual Autoplace Fields merges all units of a multi-unit symbol into one collider (medium)

**Fixed** on task-r2-schematic: 34f656dd

**What differs.** `fields_edit.rs::surroundings` (`:237`-`:247`) unions the obstacle boxes by `o.owner`, which `obstacles::of_section_but_wires`
sets to `sym.id`, the reference shared by every unit. It skips the owner equal to `exclude_key`, which is the unit's field key (`U1` for unit 1,
`U1#2` for unit 2: `ir::field_key`). As a result:
- Autoplacing unit 1 of U1 drops U1's other units from the colliders. In KiCad each unit is its own `SCH_SYMBOL`, and `getPossibleCollisions` keeps
  every symbol but `m_symbol`.
- Autoplacing unit 2 (key `U1#2`) keeps the single box spanning all of U1's units, its own body included. Every side collides
  (`COLLIDE_OBJECTS`), so the choice drops to fewest-pins. Any symbol drawn between U1's units also sees that large box as an obstacle.

KiCad tests each candidate against `item_comp->GetBodyAndPinsBoundingBox()` per symbol (`filterCollisions`).

**How to see it.** Test `manual_autoplace_treats_each_unit_as_its_own_symbol`: U1 unit 1 at (50, 50), unit 2 at (150, 50), R5 at (100, 50) with a free
right side. `SchCmd::AutoplaceFields { ids: ["R5"] }`. Assert R5's fields go RIGHT; with the union box they are pushed off it.

**Severity.** Medium.

**Already known?** Not listed.

---

### C-M6. Align Left / Right / Top / Bottom / Center drags the items (wires stretch); KiCad moves them rigidly (medium)

**Fixed** on task-r2-schematic: 160a67f2

**What differs.** `SCH_ALIGN_TOOL::moveItem` is a plain move:

```
VECTOR2I delta = adjustDeltaForGrid( aItem, aDelta );
...
aItem->Move( delta );
```

It then runs `doAlignCleanup` (`TrimOverLappingWires`, `AddJunctionsIfNeeded`, `CleanUp`). The wires on an aligned symbol stay where they are.

Ours, `sch_move.rs::align` (`:1268`), runs `setup_items_for_drag` and `perform_item_move( .., true )` with ortho bends for each item: the attached wires
stretch and bends are added. PARITY-sch.md section 1 says "identical: each item moves by its own offset ... with its wires, so pins keep landing on
the grid and keep connecting", and `useActionRunner.ts:1412` repeats it. KiCad does not do that.

The box is also different. KiCad takes the alignment edge from `schItem->GetBoundingBox()` (a symbol with its fields); the studio measures `itemBounds`
(`schAlign.ts`, "what the studio draws and picks the item by"). Unconfirmed whether that box includes fields; confirm with `itemBounds` for a symbol.

**How to see it.** Two resistors with wires, Align Left. KiCad leaves the wires and the pins come off them. Ours keeps them connected through new
bends. Test `align_moves_items_rigidly_like_kicad`, or record it as a chosen difference in the PARITY row.

**Severity.** Medium: a different result; the PARITY row is wrong.

**Already known?** Not listed (the row says "identical").

---

### C-M7. The manual-autoplace collision test misses touching boxes and wires on the box edge (medium)

**Fixed** on task-r2-schematic: 34f656dd (a wire's own stroke width is not read, the default 6 mils is)

**What differs.** `BOX2I::Intersects` counts boxes that touch ("if a common area exists, it must have a positive (null accepted) size":
`left <= right && top <= bottom`). A wire's box is also grown by half its pen (`SCH_LINE::GetBoundingBox`).

Ours: `fields.rs::boxes_intersect` (`:380`) uses strict `<`, documented as "the boxes share some area", and wire colliders are the bare segment
`Rect::of( a, b )` (`fields_edit.rs:252`), a zero-height box for a horizontal wire.

Fields boxes often sit on the grid. Example: no pins on the right, two fields, so `fbox_h` is 2 x 50 mil and the box top is `center - 2.54`. A
horizontal wire exactly on that top edge collides in KiCad (`COLLIDE_H_WIRES`, so that side goes last and `fitFieldsBetweenWires` applies); ours sees
no collision.

**How to see it.** Test `a_wire_on_the_field_box_edge_collides`: a vertical R at (50, 50), a horizontal wire at y = 47.46 from x = 51 to 60.
`AutoplaceFields`. Assert the fields do not take the right side as if it were free.

**Severity.** Medium: the edge case is common on a 50 mil grid.

**Already known?** Not listed.

---

### C-L1. Align to Grid moves items KiCad leaves alone, and finishes like a drag (low)

**Open** after task-r2-schematic: not changed

**What differs.** `AlignSchematicItemsToGrid` handles `SCH_LINE_T` (each end separately, with what is attached to it), `SCH_FIELD_T` and `SCH_TEXT_T`
(by position) and `SCH_SHEET_T`. Everything else is aligned by the most common shift of its connection points. Shapes, text boxes, bitmaps and tables
have none, so `most_common` stays (0, 0) and they do not move.

Ours (`sch_move.rs:1188`, `:1253`):
- a shape, text box, rule area, image or table moves by its position;
- a graphic line moves rigidly by its first point, where KiCad aligns each end;
- a field picked on its own is not aligned (KiCad aligns `SCH_FIELD_T`);
- for sheet pins, KiCad keeps the pin's y only when a wire on it goes to something other than a selected sheet's pin (`canDragWires`), and follows a
  selected wire's aligned end (`pinPosToWires`). Ours keeps y whenever anything is attached.

`SCH_MOVE_TOOL::AlignToGrid` ends with only `TrimOverLappingWires`, `AddJunctionsIfNeeded` and `CleanUp`. Ours calls `finalize( Some( &st ), .. )`,
which also adds junctions at the internal points, runs `trim_dangling_lines` and auto-rotates labels.

**Test.** `align_to_grid_leaves_shapes_and_text_boxes_where_they_are`; `align_to_grid_aligns_each_end_of_a_graphic_line`.

**Severity.** Low. **Already known?** Not listed.

---

### C-L2. Turn and mirror pivots use the body box where KiCad's include fields (low; ours only says so in a code comment)

**Open** after task-r2-schematic: not changed

**What differs.**
- `SELECTION::GetCenter` merges `item->GetBoundingBox()` of every non-text item, and for a symbol that is `doGetBoundingBox( true, true )` (body, pins
  and visible fields). `selection_center` uses `symbol_box` (body and pins). The module doc of `sch_move.rs` lists this; PARITY-sch.md rows R / Rotate
  pivot say "identical".
- One sheet mirrored: `GetNearestHalfGridPosition( item->GetBoundingBox().Centre() )`, which includes the sheet's fields. One sheet pin rotated or
  mirrored: `sheet->GetBoundingBox().GetCenter()`, fields included. In a multi-selection the C++ uses `GetBodyBoundingBox()`. Ours uses the body box
  in all three cases (`sch_move.rs:715`, `:882`, `mirror_single` `Item::Sheet`).

`SCH_SHEET_PIN::Rotate` mirrors across `aCenter` when the pin stays on its side (`SetTextPos( aCenter.x - delta.x, .. )`), so a sheet whose name is
wider than the sheet puts the pin at a different x.

**Test.** `rotating_a_lone_sheet_pin_uses_the_box_with_the_sheet_fields`: a sheet with a long name.

**Severity.** Low. **Already known?** Only in the `sch_move.rs` header comment (the symbol case); the sheet cases are not listed.

---

### C-L3. AutoRotate runs on the items a drag added (low)

**Fixed** on task-r2-schematic: d727abc2

**What differs.** `finalizeMoveOperation` removes the drag additions from the selection (`m_selectionTool->RemoveItemsFromSel( &m_dragAdditions,
QUIET_MODE )`) before "Auto-rotate any moved labels" loops over `aSelection`. Ours loops `self.in_selection()`, drag additions included
(`sch_drag.rs:809`). A global label with Auto that a dragged wire carried along can change spin in ours and not in KiCad.

PARITY-sch.md section 1, row `M`, still says "Not ported: ... `AutoRotateItem` after a label lands". That is stale: the code does it.

**Severity.** Low. **Already known?** Not listed.

---

### C-L4. The finish of a move gives TrimOverLappingWires and AddJunctionsIfNeeded the drag additions (low)

**Fixed** on task-r2-schematic: d727abc2

**What differs.** KiCad's `selectionCopy` is built after the drag additions leave the selection: the user's items, `m_newDragLines` and
`m_changedDragLines`. `GetNeededJunctions`' `GetConnections()` also skips items with `IS_MOVING` ("Avoid items that are changing"). Ours passes
`in_selection()` (drag additions included) plus the new and changed lines (`sch_drag.rs:795`-`:804`), and `add_junctions_if_needed` takes connection
points from every item (`sch_drag.rs:822`).

A wire stretched by a drag across the end of another wire gets a junction (and is broken there) in ours; in KiCad it does not.

**Severity.** Low (rare geometry). **Already known?** Not listed.

---

### C-L5. A label placed on a wire is not named after the wire's label (low; the PARITY row is partial)

**Open** after task-r2-schematic: not changed

**What differs.** `SCH_DRAWING_TOOLS::createNewLabel`, for a local or global label: `if( SCH_LINE* wire = findWire( aPosition ) ) netName =
findWireLabelDriverName( wire );`. When the wire's driver is a local or global label, the new label takes `wireConnection->LocalName()` and no dialog
opens. Neither the studio (`LabelDialog`) nor `place_label` does this.

**Severity.** Low. **Already known?** Not listed (row `L`/Ctrl+`L`/`H` is "partial").

---

### C-L6. Autoplace Fields on a selection of labels is refused (low)

**Open** after task-r2-schematic: not changed

**What differs.** `SCH_EDIT_TOOL::AutoplaceFields` takes every `SCH_COLLECTOR::FieldOwners` item, labels included, and runs
`AutoplaceFields( screen, AUTOPLACE_MANUAL )` with no error. `autoplace_sch_fields` refuses a selection of only labels with "nothing here has fields to
place" (`sch_fields.rs`). Our label fields are always autoplaced, so only the message differs (KiCad says nothing).

**Severity.** Low. **Already known?** GAPS item 12 has "a label's field moved by hand is placed by AutoplaceFields again"; this message is not listed.

---

### C-L7. A sheet pin's drag accumulates from its constrained place, not from its stored place (low, unconfirmed in effect)

**Open** after task-r2-schematic: not changed (unconfirmed)

**What differs.** `MoveSchematicItem` does `pin->SetStoredPos( pin->GetStoredPos() + aDelta ); pin->ConstrainOnEdge( pin->GetStoredPos(), true );`,
so the unconstrained point accumulates across the x and y splits (and across events). Ours adds each split to the already constrained place
(`sch_drag.rs:472`-`:476`). The two can choose different borders when the pointer goes well outside the sheet, and the corner tie-break differs too:
`side_of` prefers left, `NearestSegment` prefers top.

**Severity.** Low. **To confirm:** a drag of (-10, +30) mm from a left-edge pin.

---

### C-L8. `trim_dangling_lines` rules differ from `trimDanglingLines` (low, unconfirmed)

**Open** after task-r2-schematic: not changed (unconfirmed)

**What differs.** In KiCad:
- the new bend lines lose `IS_NEW` before the trim (`for( SCH_LINE* newLine : m_newDragLines ) newLine->ClearEditFlags()`);
- the both-ends rule needs `!line->HasFlag( IS_BROKEN )`;
- a separate rule removes `IS_BROKEN` lines that are dangling at either end, but only when `TestDanglingEnds` reports a change of state.

Ours removes any `F_NEW` line with both ends dangling, including pieces `break_seg` made (`break_seg` sets `F_NEW`, `sch_scene.rs:699`), and has no
`IS_BROKEN` rule. They differ when a wire with a free end is broken by a junction the drag added.

**Severity.** Low. **To confirm:** drag a wire end onto the middle of a wire whose two ends are both free.

---

### C-L9. Small `getConnectedDragItems` / `AnalyzePoint` details (low)

**Open** after task-r2-schematic: not changed

- The label-splits-a-wire branch: KiCad assigns the stub to the shared `newWire`, so no second stub is made at that point for a symbol, junction or
  label. Ours uses a local `nw` (`sch_drag.rs:312`) and also fills `line_conn` for the split, which KiCad does only in its unreachable `else` branch.
  Harmless today.
- `analyze_point` (`sch_scene.rs:613`-`:629`) lets a directive label break lines. KiCad's `AnalyzePoint` does not list `SCH_DIRECTIVE_LABEL_T`. It
  also tests bus names on every label kind; KiCad does so only for `SCH_LABEL_T` (a global or hierarchical label always sets `breakLines[WIRES]`).
  Its `hasBusEntry` is set for any bus entry whose box overlaps the point, not only at its ends.
- `setup_items_for_drag` puts a selected sheet pin in stage two; KiCad's stage two has only the four label types.

---

### Checked and found to match (do not re-review)

- `getConnectedDragItems`: the candidate filter (selected non-lines skipped, sheet pins by exact position), `ptHasUnselectedJunction`, flagging the
  line end at the point and skipping it if it is already selected or drag-added, splitting a line at a label or bus entry with a junction and a stub
  whose stored angle is `line->Angle() + 90`, the labels on a one-end line (`m_specialCaseLabels`), the no-connect, label and sheet-pin branches
  (including "don't grab labels connected directly to the unselected endpoint"), bus entries and their far end (recursion), `makeNewWire`'s bus choice.
- `getConnectedItems` (as far as it runs): junction over symbol priority ("connected to a junction on a pin ... only connected to the pin"), lines at
  their flagged end skipped, labels anywhere along a line.
- `orthoLineDrag`: the parallel test (`EDA_ANGLE::IsParallelTo`, a null line is angle 0), the found-line choice (prefers a line with length), the
  `preferOriginalLine` rule with the stored angle, the single line at a junction, `m_changedDragLines`, the remerge of two new zero-length segments
  with the label re-attachment, the 90-degree bend with the x/y bend counters and grid, label `trackMovingEnd`, the cache rewrites. (One difference:
  KiCad takes `cache[foundLine][0]` whenever the key exists, ours needs exactly one entry. No reachable case.)
- `performItemMove`: the x then y split, `GetItemsSortedByTypeAndXY` (ties by position, then a stable key), `!line->HasFlag( SELECTED_BY_DRAG |
  IS_NEW )` (all-of, as `HasFlag` is), the special-label placement with `NearestPoint` and `trackMovingEnd`. `moveItem`: line ends by flag, a
  special-case label is not moved, a field's offset goes through the inverse transform and clears the parent's autoplace flag.
- `initializeMoveOperation`'s hidden junctions (a junction on a selected line's end that `AnalyzePoint` with the selection skipped calls no junction
  goes); the internal-points junctions in `finalizeMoveOperation`.
- `TrimOverLappingWires` (non-line connectable items, at most three points, exactly two cuts the wire) and `TrimWire` (skips moving lines, never a
  whole wire, two breaks); `AddJunctionsIfNeeded` / `GetNeededJunctions` (connection points plus points that end on a selected line);
  `IsExplicitJunction`, `IsExplicitJunctionNeeded`; `AnalyzePoint`'s merge of collinear lines unless there is a dot, exit angles, unique angles for
  pins, sheet pins and bus entries, mid-lines counted only when the point breaks; `AddJunction` breaking the wires through the point.
- `SCHEMATIC::CleanUp`: junction removal (kept when selected or `SELECTED_BY_DRAG`), duplicate junctions and no-connects, null lines, identical
  lines, overlap and touching merge with the junction check at the touch point (stroke aside, M1).
- `SCH_EDIT_TOOL::Rotate`: the principal count without drag additions, the reference point while moving, connectable item turns about its position,
  others about the half-grid centre of their box; a lone line about its other end with both flags set; text and labels by `Rotate90`; a field
  toggles its angle and clears autoplace; a lone group and a table turn on themselves and move back to the half-grid point; a sheet about its
  rotation centre (body box); a lone symbol re-autoplaces when AUTO or MANUAL (multi-selection does not); no-connects follow sheet pins; the finish
  (`TrimOverLappingWires`, `AddJunctionsIfNeeded`, `CleanUp`). `SCH_FIELD::Rotate` justification flips. `SCH_SHEET::Rotate` (size sign fix, pins via
  `SCH_SHEET_PIN::Rotate`, fields re-autoplaced).
- `SCH_EDIT_TOOL::Mirror`: one item vs several, a symbol mirrored in place clearing autoplace, a label's `MirrorSpinStyle( !vertical )`, `SCH_TEXT`
  mirrors (`FlipHJustify` only for the text along the axis), sheet pins and sheets, the mirror point never taken from the held reference.
- `SCH_LABEL_BASE::Rotate` / `Rotate90` spin turns (RIGHT clockwise is BOTTOM, counter-clockwise is UP); `SPIN_STYLE` mirrors for plain labels.
- `SCH_EDIT_FRAME::AutoRotateItem` (global and hierarchical labels only, only with Auto); `GetLabelOrientationForPoint` (`KICAD_T` order with the
  last match winning, line boxes grown by half the pen, `Normalize90` with (-45, 45], the bus-entry rule on a bus drawn downwards or rightwards,
  the first pin on the point); `GetPinSpinStyle` (checked to equal the geometric "away from the body" rule for all 12 orientations). The TS copy
  `schAutoRotate.ts` matches the Rust.
- `AUTOPLACER` for ordinary symbols: field box size (visible, non-empty, autoplaceable fields sorted by ordinal; heights rounded up to 50 mil), the
  preferred sides with KiCad's own `h_mirrored` (only `GetOrientation() == SYM_MIRROR_X + 0/180`, folded here as `(180, mirrored)`) and the w/h > 3
  swap, `chooseSideForFields` / `chooseSideFiltered` (OBJECTS then H_WIRES, the fallback with the fewest pins, the later of equals),
  `fieldBoxPlacement` (paddings, the pins-box offsets), `justifyField`, `fieldHPlacement`, `fieldVPlacement` with dynamic and wire spacing,
  `fitFieldsBetweenWires`, grid rounding by side, `m_field_angle` (stored vertical for 90/270 so it reads horizontal), the drawable area.
- `PlaceSymbol` / `TwoClickPlace` for labels: Auto from the last-used value, spin default RIGHT, `AutoRotateItem` on the second click for a
  connectable item.

### Summary

| ID | Finding | Severity | Known? |
|---|---|---|---|
| H1 | A dragged wire or sheet pin leaves the sheet pin (`m_specialCaseSheetPins` never set) | high | no |
| H2 | A wire drawn across a 2-pin part is not trimmed; junctions on both pins short the part; no `CleanUp` merge | high | no |
| H3 | Placing a symbol (power symbol, label) on a wire: no `TrimOverLappingWires` / `AddJunctionsIfNeeded` | high | no |
| M1 | `clean_up` merges wires of different strokes; a split run loses its stroke and pins | medium | no |
| M2 | A directive label mirrors about the wrong axis (0/180 vs 90/270 swapped) | medium | no |
| M3 | Power symbols' fields are not KiCad's Autoplace (5.08 mm, rotated cases, PWR_FLAG Value hidden) | medium | no |
| M4 | Autoplace counts hidden pins; its body box has no pin roots | medium | no |
| M5 | Manual autoplace unions every unit of a reference into one collider | medium | no |
| M6 | Align L/R/T/B/C drags with stretching wires; KiCad moves rigidly (PARITY says identical) | medium | no |
| M7 | Collision test strict, wires without pen width: touching boxes miss | medium | no |
| L1 | Align to Grid moves shapes, text boxes, images and tables; graphic lines rigid; drag-style finish | low | no |
| L2 | Pivot boxes without fields (multi-select symbols; one sheet mirror; one sheet pin) | low | partly (code comment) |
| L3 | AutoRotate runs on drag additions; PARITY row `M` stale on AutoRotate | low | no |
| L4 | The move's finish gives TrimOverLapping and AddJunctions the drag additions | low | no |
| L5 | A label on a wire is not named after the wire's label | low | no (partial row) |
| L6 | Autoplace Fields on labels only is refused | low | no |
| L7 | Sheet pin drag from the constrained place, not the stored one | low (unconfirmed) | no |
| L8 | `trim_dangling_lines` vs `IS_BROKEN` / cleared `IS_NEW` | low (unconfirmed) | no |
| L9 | Small `getConnectedDragItems` / `AnalyzePoint` details | low | no |

Counts: high 3, medium 7, low 9.

---

## D-clip-ann-erc: schematic clipboard, Annotate, the ERC dialog and settings, view controls (round 2, r2-schematic)

KiCad source: `/home/team_braneboard_com/src/kicad-src-8303b2ad` (newer: `~/src/kicad-src-25ec2335`). Our code: main at 5cca5fff. No code changed. E1 was confirmed with `eda` and `kicad-cli` (scratch in `/tmp/findings-r2-schematic.x5lr/D-clip-ann-erc/e/`). Everything else comes from reading both codebases.

### Part 1: Schematic clipboard (GAPS item 6; PARITY-sch.md sections 15, 26)

KiCad is 8303b2ad. These come from reading both codebases. None was confirmed by running the binary.

### D-C1. Copy on the second placement of a shared screen writes the screen's own reference, not this path's (medium)

**Fixed** on task-r2-schematic: d06ab0a8
- **What differs.**
  - **Ours:** `crates/kicad/src/sch_clipboard.rs:378` (`property(out, "Reference", &sym.id, ...)`) and `:399-402` (`(instances ... (reference {sym.id}) (unit {sym.unit}))`) always write `SymbolInstance::id`.
    - The section comes from `crates/cli/src/studio.rs:1664 resolve_sheet`, which returns the raw screen.
    - `crates/cli/src/sch_clipboard_api.rs:93` passes no sheet path, and `CopyInput` has no field for one.
    - The studio meanwhile shows the per-path reference (`schematic_json_with`: `reference_under(&sch, s, &sheet_ids)`).
  - **KiCad:** `sch_io_kicad_sexpr.cpp::saveSymbol( aForClipboard, aRelativePath )`, where `aRelativePath` is the current sheet:
    - Reference: `else if( aForClipboard && aSymbol->GetInstances().size() && aRelativePath && ( id == FIELD_T::REFERENCE ) ) { SCH_SYMBOL_INSTANCE instance; if( aSymbol->GetInstance( instance, aRelativePath->Path() ) ) field->SetText( instance.m_Reference ); }`
    - Unit: `if( aSymbol->GetInstance( unitInstance, aRelativePath->Path() ) ) unit = unitInstance.m_Unit;`
- **How to see it.**
  - On a screen shown twice (R1 under placement A, R5 under B), copy R5 on B: the text says R1. Unique mode then numbers from 1 instead of 5 (`aStartAtCurrent`), and real KiCad pasting our text gets R1.
  - Test: `copy_on_a_second_placement_writes_that_paths_reference`. Call `write_clipboard` with B's path and assert `(property "Reference" "R5"`.
- **Already known?** Not listed. It came with the whole-path references (97496dc2 / db432823, 2026-10-26).

### D-C2. A paste's anchor looks only at the first point of a wire polyline; KiCad looks at every segment's start (medium)

**Fixed** on task-r2-schematic: 3b1f43ab
- **What differs.**
  - **Ours:** `positions()` in `crates/model/src/sch_clipboard.rs` (~:395) uses `w.pts.first()` for wires and lines, and `top_left_anchor` picks from these.
    - `join_bends` rejoins KiCad's segments into one polyline on read, so this affects fragments from either side.
  - **KiCad:** one `SCH_LINE` per segment. `SCH_SELECTION::GetTopLeftItem` takes each segment's `GetPosition()` (its start).
    - `Paste`: `selection.SetReferencePoint( item->GetPosition() )`.
    - The move tool: `delta = m_cursor - *m_anchorPos`.
- **How to see it.**
  - Copy only an L wire (20,10) → (10,10) → (10,20) and paste. KiCad puts (10,10) under the cursor; ours puts (20,10) there, 10 mm off.
  - Test: `top_left_anchor_counts_every_segment_start`, asserting (10,10).
- **Already known?** Not listed. PARITY section 15 marks Paste identical.

### D-C3. Paste ignores the annotation method and start number (medium)

**Open** after task-r2-schematic: needs the method and start number in `Cmd::PasteSch`
- **What differs.**
  - **Ours:** `annotate_paste` (`crates/model/src/sch_clipboard.rs`) takes no method or start, and `paste_sch` calls it with `(used, pasted, mode)`. The studio does hold `algo` and `start` (`web/studio/src/kicad-port/schAnnotate.ts`).
  - **KiCad `Paste`:**
    - Unique mode: `ReannotateDuplicates( existingRefs, annotateAlgo )`.
    - Keep and Remove modes: `ReannotateByOptions( annotateOrder, annotateAlgo, annotateStartNum, existingRefs, false, &hierarchy )`.
    - `AnnotateByOptions`: `case SHEET_NUMBER_X_100: useSheetNum = true; idStep = 100; aStartAtCurrent = false;`.
    - `Annotate`: `minRefId = m_sheetNum * aSheetIntervalId + 1`.
- **How to see it.**
  - Method "First free after sheet number X 100": paste a free R5 on page 2. KiCad gives R201; ours keeps R5.
  - Start "after 10" in Keep mode: a pasted `#PWR` becomes `#PWR011` in KiCad and `#PWR01` here.
  - Test: `annotate_paste_sheet_x100_numbers_from_the_page`.
- **Already known?** Not listed.

### D-C4. A paste prefers the installed or built-in library symbol over the clipboard's cached one, and has no power-type check (medium)

**Open** after task-r2-schematic: not changed
- **What differs.**
  - **Ours:** `merge_lib_symbols` (`crates/ops/src/sch_clipboard.rs`) asks `design_symbol`, which falls through to `model.symbol_of` and then the built-ins (`crates/model/src/lib.rs:1408`).
    - The design's symbol wins unless the pins differ.
    - `power_local` is never compared.
  - **KiCad `Paste`:** it never opens the installed library. It looks at the screen's cache, then the clipboard's:
    `if( it != end && clipLibSymbol ) { if( clipLibSymbol->IsLocalPower() != it->second->IsLocalPower() || clipLibSymbol->IsGlobalPower() != it->second->IsGlobalPower() ) libSymbol = new LIB_SYMBOL( *clipLibSymbol ); else libSymbol = new LIB_SYMBOL( *it->second ); } else if( it != end ) ... else if( clipLibSymbol ) libSymbol = new LIB_SYMBOL( *clipLibSymbol );`
- **How to see it.**
  - Paste a fragment whose cached `power:VCC` is `(power local)`: ours draws and writes the built-in global one.
  - An edited `Device:R` cache with the same pins is drawn with the installed artwork.
  - Test: `paste_keeps_the_clipboards_local_power_symbol` (`power_local == true`).
- **Severity.** Medium. The file KiCad reads differs; our connectivity does not read `power_local` yet.
- **Already known?** Not listed.

### D-C5. The schematic Paste Special lacks "Clear net assignments" (low)

**Fixed** on task-r2-schematic: 4d5db9e6
- **What differs.** `SchPasteSpecialDialog.tsx` leaves the box out, as if it belonged to the board editor. In fact only pcbnew hides it (`pcb_control.cpp:1221 dlg.HideClearNets();`, verified). eeschema shows "Clear net assignments" (tooltip "Remove the net information from all connected items before pasting") and then ignores it.
- **Already known?** No. PARITY section 15 calls the row identical.

### D-C6. The long-text paste question is not word for word, and counts characters instead of bytes (low)

**Fixed** on task-r2-schematic: 4d5db9e6
- **What differs.**
  - Wording: `schClipboardActions.ts:78` says "Pasting a long text string may be very slow.  Do you want to continue?". KiCad (`sch_editor_control.cpp:2095`, verified) says `_( "Pasting a long text text string may be very slow.  " "Do you want to continue?" )`, with "text" twice.
  - Threshold: `sch_clipboard_api.rs:200` uses `chars().count() > 100`; KiCad uses `content.size()` on a UTF-8 `std::string`, which counts bytes.
- **How to see it.** Paste 60 non-ASCII characters (more than 100 bytes): KiCad asks first, ours does not.
- **Already known?** Not listed.

### D-C7. A copied power symbol is written `(in_bom no)`; KiCad's are `(in_bom yes)` (low)

**Fixed** on task-r2-schematic: 4d5db9e6 (the clipboard; the main writer's `(in_bom no) (on_board no)` (B-L4) is unchanged)
- **What differs.** `crates/kicad/src/sch_clipboard.rs:421` writes `(in_bom no)`, so KiCad pastes it with Exclude from BOM set. KiCad's power library (GND) and QA `ground_pin_test_ok.kicad_sch` have `(in_bom yes) (on_board yes)`.
- Outside this slice: the main writer `crates/kicad/src/lib.rs:455` writes `(in_bom no) (on_board no)` for every power symbol.
- **Already known?** Not listed.

### D-C8. Duplicate of a hovered item leaves the copy selected; KiCad leaves nothing selected (low)

**Open** after task-r2-schematic: not changed
- **What differs.** `schClipboardActions.ts:97` always selects the new items. KiCad:
  - `doCopy( true )` records the hover state (`m_duplicateIsHoverSelection`);
  - `Paste` restores it with `SetIsHover`;
  - the move tool clears the selection when the move ends (`sch_move_tool.cpp:1077`).
- **How to see it.** With nothing selected, hover a resistor, press Ctrl+D and click: KiCad's selection is empty, ours is the copy.
- **Already known?** Not listed.

### D-C9. When a multi-unit part joins an existing number, ours compares the full lib_id; KiCad compares only the item name (low)

**Fixed** on task-r2-schematic: 4d5db9e6
- **What differs.** `units_available` checks `h.lib_id == lib_id`. KiCad's `areUnitsAvailable` and `GetUnitsMatchingRef` use `CompareLibName`, which compares `GetLibItemName()` only.
- **How to see it.** Paste unit B of `MyLib:LM358` (`U?`) where `Amplifier_Operational:LM358` unit A is U1 with the same value: KiCad gives U1, ours U2.
- **Already known?** Not listed.

### Checked and found to match (clipboard)
- **`doCopy`, `Cut`:**
  - markers are left out;
  - a sheet's screen goes into the side buffer;
  - Duplicate uses its own buffer;
  - Cut deletes only after a copy that worked and keeps locked items (`FilterSelectionForLockedItems`).
- **`ParseSchematic( aIsCopyableOnly )`:**
  - the accepted forms;
  - `paper` / `page` / `title_block` / `bus_alias` are refused;
  - anything else becomes a text item at (0,0);
  - `m_MaxPastedTextLength` is 100.
- **`updatePastedSymbol`:**
  - the Reference property is used when no instance is found;
  - the `(path "")` branch;
  - locks are dropped and every item gets a new id.
- **`ReannotateDuplicates` (Unique):**
  - the unsorted option falls through to sort-by-X;
  - prefixes compare case-insensitively;
  - the minimum starts at the pasted number and carries across a prefix;
  - references still being numbered are skipped;
  - the units checks match apart from C9;
  - a `?` reference is not locked;
  - power references get a leading 0, and `AlwaysAnnotate` holds;
  - the existing references span the whole hierarchy, other paths included;
  - `reuse_designators` is true.
- **`DIALOG_PASTE_SPECIAL`:** the three labels, the "Reference Designators" title, the tooltips and the default (Unique).
- **Sheet paste names:** the base name and uniquifier loop, and the `StrNumCmp` order.
- **Duplicate's anchor:** connection points first, then the other points, with the first winning a tie. The cursor is unsnapped and the drop point snapped (`SCH_MOVE_TOOL`).
- **Paste's anchor:** connectable items are preferred and the left-then-top item is chosen, apart from C2.
- **The clipboard format:** `lib_symbols` first, then the items flat with no `kicad_sch` wrapper.

### Part 2: Annotate (PARITY-sch.md section 5)

KiCad is 8303b2ad. None of these was confirmed with the binary: `eda` has no verb that applies a Cmd. Every one comes from reading both codebases.

### D-A1. A symbol placed with `A`, or repeated, gets the highest number + 1, not KiCad's first free number (medium)

**Fixed** on task-r2-schematic: 96cc869b (the first free number by the project's method; a unit does not yet join a part with that unit free)
- **What differs.**
  - **Ours:** `web/studio/src/kicad-port/nextReference.ts:19-26` does `${prefix}${max + 1}`. It is called from `components/SchematicView.tsx:767` (the `sch_place_symbol` click) and `actions/useActionRunner.ts:2275` (Repeat). It:
    - ignores the numbering method and the start number;
    - fills no gaps;
    - never joins a multi-unit part that has a free unit;
    - skips prefixes that are not plain letters (`^([A-Za-z]+)(\d+)$`).
  - **KiCad:** `sch_drawing_tools.cpp::PlaceSymbol`, the `annotate` lambda:
    ```
    if( cfg->m_AnnotatePanel.automatic || newReference.AlwaysAnnotate() )
        refs.ReannotateByOptions( (ANNOTATE_ORDER_T) schSettings.m_AnnotateSortOrder,
                                  (ANNOTATE_ALGO_T) schSettings.m_AnnotateMethod,
                                  schSettings.m_AnnotateStartNum, existingRefs, false, &hierarchy );
    ```
    - `existingRefs` holds the whole hierarchy (`SYMBOL_FILTER_ALL`).
    - The new symbol gets `FindFirstUnusedReference( ref_unit, minRefId, {} )`; a multi-unit symbol passes `{ unit }` and so joins a part whose unit is free (`areUnitsAvailable`).
    - `sch_edit_tool.cpp::RepeatDrawItem` does `ClearAnnotation` then `AnnotateSymbols( ANNOTATE_SELECTION, annotateOrder, annotateAlgo, true, annotateStartNum, ... )`.
- **How to see it.**
  - With R1 and R3 on the sheet, place a resistor: KiCad gives R2, ours R4.
  - With the method "First free after sheet number X 100", on page 2: KiCad gives R201.
  - With LM358 U1 unit A placed, place unit B: KiCad gives U1, ours U2.
  - Test: `nextReference fills the first gap`, input `[R1,R3]`, assert "R2". The better fix routes placement through `annotate::annotate_by_options` with the project's settings.
- **Already known?** No. PARITY-sch.md section 5 even claims "Add Symbol, paste and import number their own symbols the way KiCad's automatic annotation does". Section 13 (line 504) mentions `nextReference` only for the missing Annotate Automatically toggle.

### D-A2. A placed power symbol is numbered per screen, and its number is written `#PWR10` where KiCad writes `#PWR010` (medium)

**Fixed** on task-r2-schematic: cd6e6bf2
- **What differs.**
  - **Ours:** `crates/ops/src/lib.rs:5022-5034` (`add_power_symbol`) starts at `n = sch.power_symbols.len() + 1` with `format!("#PWR{n:02}")` and checks only the focused screen. As a result:
    - the same `#PWR01` can sit on two sheets;
    - gaps are not reused: with #PWR01 and #PWR03 it gives #PWR04, KiCad gives #PWR02;
    - from 10 on it writes `#PWR10`.
  - **KiCad:** `PlaceSymbol` always annotates power symbols (`AlwaysAnnotate()`: `IsPower() || ref[0] == '#'`) against the whole hierarchy. `SCH_REFERENCE::formatRefStr` puts a "0" before the number. Our own `Entry::format_number` (`crates/model/src/annotate.rs:248`) does that too, but placement does not use it.
- **How to see it.**
  - Place GND on the root (`#PWR01`), then on a sub-sheet: ours gives `#PWR01` again. KiCad's ERC dialog (`CheckAnnotate(..., ANNOTATE_ALL)`) would say "Duplicate items #PWR01". kicad-cli probably stays silent because `ERC_TESTER::RunTests` calls `AnnotatePowerSymbols()` first (unconfirmed: needs a two-sheet scratch project).
  - With ten on one sheet, the tenth is `#PWR10` (KiCad: `#PWR010`).
  - Test: `add_power_symbol_is_unique_across_sheets_and_formatted_like_formatRefStr`.
- **Already known?** No. PARITY section 5's "power numbering identical" covers Annotate only.

### D-A3. Annotate's Selection scope ignores a selected group (medium)

**Fixed** on task-r2-schematic: 96cc869b
- **What differs.**
  - **Ours:** `web/studio/src/components/AnnotateDialog.tsx:28-31` builds the scope from `[...state.selection].filter((id) => api.symbolById(id))`. A click on a grouped item selects the group id (`kicad-port/schGroups.ts::substituteSchSelection`), so a selected group adds no symbols, and with only a group selected the Selection radio is disabled.
  - **KiCad:** `annotate.cpp::getInferredSymbols` recurses into groups: `case SCH_GROUP_T: group->RunOnChildren( ... SCH_SYMBOL_T ..., RECURSE_MODE::RECURSE )`. `CheckAnnotate` does the same. Our `schGroups.ts::leafIds` already does this expansion.
- **How to see it.** Group R? and C?, click the group, Annotate with the Selection scope: KiCad numbers both; ours cannot choose Selection. Test: an `AnnotateSelection` built from a group id lists the member references.
- **Already known?** No.

### D-A4. The Annotate dialog disables controls that KiCad leaves enabled (low)

**Open** after task-r2-schematic: not changed
- **What differs.** `AnnotateDialog.tsx`:
  - "Recurse into subsheets" is disabled for the Entire schematic scope (:94);
  - the start-number field is disabled unless "Use first free number after" is chosen (:122);
  - "Selection" is disabled with nothing selected and carries a "(N symbols, M sheets)" suffix (:92);
  - a remembered "selection" scope opens as "all" (`schAnnotate.ts:61-65`).

  KiCad (`dialog_annotate.cpp`, `dialog_annotate_base.cpp`) enables and disables only "Regroup symbol units": `m_checkRegroupUnits->Enable( m_rbReset_Annotations->GetValue() )`. Its labels are plain, and `TransferDataToWindow` restores the saved scope as it was.
- **Already known?** No.

### Checked and found to match (Annotate)
- **`sortByXPosition` / `sortByYPosition`:**
  - prefix (case-insensitive), then sheet number, then X,Y or Y,X; the last tie-break is the entry key where KiCad's is the UUID (documented);
  - 8303b2ad does no grid rounding of `m_symbolPos`.
- **`AnnotateByOptions` / `Annotate`:**
  - minRefId = start + 1, or sheet × step + 1, reset per prefix (and per sheet for the sheet algorithms);
  - additional refs are split, marked not new, appended after the sort and removed at the end;
  - `inUseRefs` / `buildFullReference`;
  - the three paths: single-unit, locked (`GetUnitsMatchingRef`), unlocked new multi-unit with `{unit}`.
- **`FindFirstUnusedReference`, `REFDES_TRACKER::GetNextRefDesForUnits` / `areUnitsAvailable`:** reuse is on by default.
- **`SCH_REFERENCE` helpers:** `Split`, `IsSplitNeeded`, `Annotate()`, `formatRefStr`, `RemoveAnnotation`, `LetterSubReference`, a value of "~".
- **`CheckAnnotation`:** the `sortByRefAndValue` order, the first pass stopping at its first finding, the ERCE codes, and the messages word for word:
  - "Item not annotated: %s%s (unit %d)" / "Item not annotated: %s%s"
  - "Error: symbol %s%s%s (unit %d) exceeds units defined (%d)"
  - "Duplicate items %s%s%s\n"
  - "Differing unit counts for item %s%s%s and %s%s%s\n"
  - "Different values for %s%d%s (%s) and %s%d%s (%s)"
- **`AnnotateSymbols`:**
  - scope sets: the current sheet takes power symbols but its sub-sheets do not; the selection uses `NON_POWER`;
  - locked groups (`GetMultiUnitSymbols`): no `?`, keyed by the reference text, skipped on regroup, and with reset a group holding a unit twice is dropped;
  - `mapExistingAnnotation` / `IsAnnotated`;
  - the messages "Updated %s (unit %s) from %s to %s.", "Updated %s from %s to %s.", "Annotated %s (unit %s) as %s.", "Annotated %s as %s.", and "Annotation complete." only when the check is clean.
- **`CheckAnnotate` scopes:** the entire schematic takes power symbols, the other scopes do not.
- **Dialog defaults:** scope all, recursive on, regroup off, keep. Project defaults: `SORT_BY_X_POSITION`, `INCREMENTAL_BY_REF`, start 0. The labels match.
- **db432823:** other paths' references are taken with key `usize::MAX`, which cannot collide with a locked group's key (the remaining difference is already logged in GAPS item 4 and PARITY section 42).

### Part 3: ERC dialog, settings, pin map, exclusions (GAPS item 11 schematic half; PARITY-sch.md sections 4, 9, 28)

Commits checked: 18db02a5, 132bd8da, e6cc0ab4, 01230f08.

### D-E1. A KiCad project's own ERC severities, pin map and connection grid are dropped on import; only its exclusions come in (high)

**Fixed** on task-r2-schematic: 5d66ea32
- **What differs.** `eda import-kicad x.kicad_sch` reads the project's `erc.erc_exclusions` (`crates/cli/src/import_kicad.rs:370-391`, `keep_project_erc_exclusions`) and nothing else of the `erc` section: `crates/kicad/src/sch_import.rs:584` sets `erc_pin_map: None` and `extras` without `erc_severities`; nothing under `crates/kicad/src` or `crates/cli/src` reads `erc.rule_severities`, `erc.pin_map` or `schematic.connection_grid_size` from a `.kicad_pro` (only writers). The board side does merge `board.design_settings.rule_severities` (`import_kicad.rs:234`, `merge_project_rule_severities`); the schematic side has nothing like it. KiCad keeps these in the project and kicad-cli judges by them. In `erc_settings.cpp` (the `ERC_SETTINGS` constructor), the `rule_severities` loader does `if( aJson.contains( key ) ) m_ERCSeverities[code] = SeverityFromString( aJson[key] );` and the `pin_map` loader does `SetPinMapValue( i, j, static_cast<PIN_ERROR>( val ) )`. The derived project then carries the defaults (`effective_erc_severities`, `custom_erc_pin_map` -> None).
- **How to see it.** Confirmed by running both tools.
  - Original: `cp -r $KICAD_SRC_DIR/qa/data/eeschema/issue22938 orig; cd orig; kicad-cli sch erc --format json -o ../orig_erc.json issue22938.kicad_sch`. Its project sets `pin_not_connected: ignore`; the report lists that check under `ignored_checks` and has 0 `pin_not_connected`.
  - Ours: `eda import-kicad orig/issue22938.kicad_sch -o imp; eda board erc -C imp` reports 50 `pin_not_connected` errors, and `ignored_checks` has no `pin_not_connected`. The binary dates from 10-23; the import code that matters has not changed since (checked in source).
  - Other QA projects that show it:
    - a custom `pin_map`: `erc_multiple_pin_to_pin`, `netlists/noconnects`, `netlists/bus_junctions`, `netlists/test_hier_renaming`;
    - `pin_to_pin` ignored: `netlists/issue14657`;
    - `pin_not_connected` ignored: `issue18606`, `netlists/issue14494`.
  - Test: `a_kicad_projects_erc_settings_come_in_with_the_schematic`. Input: a `.kicad_pro` with `erc.rule_severities.pin_not_connected="ignore"`, `pin_map[1][1]=0` and `schematic.connection_grid_size=100`. Assert `effective_erc_severities(design)["pin_not_connected"]=="ignore"`, `custom_erc_pin_map(design).unwrap()[1][1]==0` and `extras.connection_grid_mils==Some(100)`.
  - Scratch: `/tmp/findings-r2-schematic.x5lr/D-clip-ann-erc/e/`.
- **Severity.** High: the derived ERC reports findings KiCad does not report on the original project.
- **Already known?** Not listed. p13-p16 F6 covered only the exclusions (fixed in 132bd8da); PARITY-sch.md section 41 does not mention severities or the pin map.

### D-E2. A legacy (schema 0) exclusion listed twice, first without and then with a comment, keeps the empty comment; KiCad keeps the comment (medium)

**Fixed** on task-r2-schematic: 45f4c161
- **What differs.** `crates/kicad/src/erc_exclusions.rs:413-416` (`from_project`) keeps the first of a `(check, location, marker)` duplicate. KiCad's `schematic.cpp::ResolveERCExclusions` (25ec2335) replaces it with the commented one:
  ```
  // Legacy format can sometimes have the same exclusion multiple times,
  // without and with a comment.  If this happens, replace the existing one
  // if we can go from no comment to comment
  auto [it, inserted] = settings.m_ErcExclusions.insert( exclusion );
  if( !inserted && !comment.empty() && it->GetComment().empty() ) { ... updated.SetComment( comment ); ... }
  ```
- **How to see it.** Test `a_legacy_exclusion_listed_twice_keeps_its_comment`. Input: `{"erc":{"meta":{"version":0},"erc_exclusions":["pin_not_connected|13335|8128|f-pin|00000000-0000-0000-0000-000000000000|||", ["pin_not_connected|13335|8128|f-pin|00000000-0000-0000-0000-000000000000|||","why"]]}}`. Assert one exclusion with `comment=="why"` (today it is `""`).
- **Already known?** Not listed.

### D-E3. The violations list is in report order; KiCad lists every error, then warnings, then exclusions (low)

**Fixed** on task-r2-schematic: 4d5db9e6 (the DRC list too: drc_item.cpp sorts the same way)
- **What differs.** `web/studio/src/kicad-port/rcItems.ts:62-68` (`listedIndexes`) and `ErcDialog.tsx:99` keep kicad-cli's report order. `ERC_REPORT::WriteJsonReport` groups that order by sheet, so it is severity-sorted only within each sheet. KiCad sorts the whole filtered list once, in `erc_settings.cpp::SHEETLIST_ERC_ITEMS_PROVIDER::SetSeverities`: `// Sort markers so that errors appear before warnings` `std::stable_sort( m_filteredMarkers.begin(), m_filteredMarkers.end(), []( const SCH_MARKER* a, const SCH_MARKER* b ) { return a->GetSeverity() > b->GetSeverity(); } );` (ERROR 0x20 > WARNING 0x10 > EXCLUSION 0x04). Next and Previous Marker step in that order too.
- **How to see it.** On two sheets, put a warning on sheet 1 and an error on sheet 2: KiCad lists the error first, the studio lists the warning first. Test in rcItems.test.ts: input `[warning(s1), error(s2), excluded(s1)]`, assert the listed order is `[1,0,2]`.
- **Already known?** No. PARITY-sch.md section 4 says "a flat list in report order" and marks it identical.

### D-E4. The Violation Severity page says "the two library checks start at Ignore"; the code ignores three (low)

**Fixed** on task-r2-schematic: 4d5db9e6
- **What differs.** `crates/kicad/src/pcb.rs:894` (`effective_erc_severities`) forces `lib_symbol_issues`, `lib_symbol_mismatch` and `footprint_link_issues` to ignore (3f4e2997, 10-24). `ErcSeveritiesPage.tsx:75` and the PARITY-sch.md section 9 row say "two". KiCad's defaults (`erc_settings.cpp` 111-113) make all three `RPT_SEVERITY_WARNING`.
- **Already known?** The deviation is listed; the count is wrong.

### Note (unconfirmed, no finding)
25ec2335 `erc_report.cpp` files a sheet-specific marker that has no main-item sheet under `GetSpecificSheetPath()`, not under `sheetList[0]`. Our `erc_exclusions.rs:119-121` (`Paths::Specific`) assumes the root, so on a multi-sheet design `wire_dangling`, `label_dangling` and the bus checks get no marker. That is conservative (the waiver stays the studio's alone), not wrong. To confirm: whether nightly 10.99.0-5334 has this code. Separately, the module doc says "the schematic's internal unit is a nanometre now", but 25ec2335 `base_units.h` keeps `SCH_IU_PER_MM = 1e4`. Doc only.

### Checked and found to match (ERC)
- `DEFAULT_ERC_PIN_MAP` (`crates/model/src/ir.rs`) against `m_defaultPinMap`, cell by cell. `Cmd::SetErcPinMapCell` against `changeErrorLevel` (symmetric). `ercPinMap.ts` against `CommentERC_H`/`V`, `PINMAP_TYPE_COUNT` 11, `(level+1)%3` and the tooltips "No error or warning" / "Generate warning" / "Generate error".
- `erc_checks.json` against `erc_item.cpp` (script-checked): all 46 titles word for word, the settings keys, the ERCE codes and the pin-map row; the rows, order and headings against `allItemTypes` / `GetItemsWithSeverities`, with the pin-map special case last. The default severities against `ERC_SETTINGS` lines 95-125.
- `rcMenu` (ERC) against `OnERCItemRClick`, word for word: Remove exclusion / Edit exclusion comment... / Exclude this violation / Exclude with comment..., with their hints; Change severity to Error/Warning for all '%s' violations; Ignore all '%s' violations; pin_to_pin with no severity change and "Edit pin-to-pin conflict map..."; Edit violation severities...; Edit connection grid spacing... ("Open the Schematic Setup dialog").
- `rcPrefix` against `RC_TREE_MODEL::GetValue`. `base_severity` of an excluded pin_to_pin error. Delete All Markers' question and buttons against `OnDeleteAllClick`. The Ignored Tests menu against `OnIgnoredItemRClick`. The tab titles "Violations (%s)" / "Ignored Tests (%s)".
- `read_project` against 25ec2335's `erc_exclusions` loader, `migrateSchema0to1`, `NESTED_SETTINGS::LoadFromFile` and `ERC_EXCLUSION::FromLegacyStrings`:
  - texts versus objects by schema version;
  - 5 or 8 fields, the first-match `ERC_ITEM::Create` (`pin_to_pin` = the warning);
  - nil and empty uuids skipped; generic and unresolved_variable child texts left out;
  - 100 nm legacy positions; snake-case and camel-case field names both read.
- `entry()` against `to_json` / `from_json`.
- `same_sheet`, the `ErcReport::to_json` match, the `add_erc_exclusion` / `delete_erc_exclusion` sheet filter and `ercKeyHits` (18db02a5) against `SetSheetSpecificPath`. `ErcReport::listed` dropping stray excluded copies. e6cc0ab4's mm-to-Nm `kiround(x*1e6)`.

### Part 4: View controls (GAPS item 20, PARITY-sch.md section 27)

### D-V1. `input.zoom_speed` defaults to 5 everywhere; KiCad's default is 1 off macOS, so a Zoom drag is 5x too fast (medium)

**Fixed** on task-r2-schematic: 96cc869b
- **What differs.** `web/studio/src/kicad-port/preferences.ts:87` `zoomSpeed: 5` (comment at :74 says "COMMON_SETTINGS factory defaults"); `viewControls.ts:183` doc says "`zoomSpeed` is `input.zoom_speed`, 5 by default". KiCad `common/settings/common_settings.cpp` (same in 8303b2ad and 25ec2335):
  ```
  #ifdef __WXMAC__
      int default_zoom_speed = 5;
  #else
      int default_zoom_speed = 1;
  #endif
      m_params.emplace_back( new PARAM<int>( "input.zoom_speed", &m_Input.zoom_speed, default_zoom_speed ) );
  ```
  `WX_VIEW_CONTROLS::LoadSettings` copies it to `m_settings.m_zoomSpeed` whatever `zoom_speed_auto` says, and `onMotion` (DRAG_ZOOMING) uses it unconditionally: `double scale = exp( d.y * m_settings.m_zoomSpeed * 0.001 );`. Ours: `SchematicView.tsx:897` `dragZoomView(..., state.prefs.zoomSpeed)`. So with stock settings on Linux/Windows a right/middle Zoom drag of 60 px scales by exp(0.3)=1.35 here vs exp(0.06)=1.06 in KiCad. Also the Preferences slider (`PreferencesDialog.tsx:92`) starts at 5 when "Automatic" is unticked (KiCad: 1 off Mac), and the manual constant controller would be `0.001*5` instead of `0.001*1`.
- **How to see it.** Test `preferences.test.ts`: `DEFAULT_PREFERENCES.zoomSpeed === (isMac ? 5 : 1)` (needs a platform-dependent default, as `pickDefaultZoomController` already is). The browser check `e2e/sch-details.check.js` asserts "a right drag zooms by exp(60 x 5 / 1000)"; KiCad on Linux gives exp(60 x 1 / 1000).
- **Already known?** Not listed (PARITY-sch.md section 27 calls the drag "identical" and quotes `zoomSpeed` without its default).

### D-V2. Borders of the auto-pan zone are not truncated to whole pixels (low)

**Fixed** on task-r2-schematic: 4d5db9e6
- **What differs.** `viewControls.ts:211` `borderStart = Math.max(Math.min(margin*w, margin*h), 2)` is a float; KiCad `WX_VIEW_CONTROLS::handleAutoPanning`: `int borderStart = std::min( m_settings.m_autoPanMargin * ...x, ... * ...y ); borderStart = std::max( borderStart, 2 );` (an `int`, truncated) and the pointer `p` is a `VECTOR2I`. A 733 px high canvas gives a 14 px border in KiCad, 14.66 here; `m_panDirection` is integral in KiCad, fractional here. (onTimer's `borderSize` is a double in both, so that part matches.)
- **How to see it.** `computeAutoPanDirection({x: 14.5, y: 300}, {width: 1000, height: 733})` returns x = -0.16 here, 0 in KiCad.
- **Already known?** Not listed.

### D-V3. (unconfirmed) Shift+wheel: a browser that turns Shift+wheel into a horizontal delta makes it pan sideways, KiCad pans up/down (low)

**Open** after task-r2-schematic: unconfirmed, not changed
- **What differs.** `viewControls.ts:116` treats any event with |deltaX| > |deltaY| as a native horizontal wheel and pans horizontally regardless of modifiers. KiCad's `onWheel` only does that for `axis == wxMOUSE_WHEEL_HORIZONTAL`; a vertical wheel with Shift (default `scroll_modifier_pan_h` = WXK_CONTROL, so Shift is not the H modifier) falls to `scrollY = -scrollVec.y` (vertical pan). Chromium on Windows (and, I believe, Linux) delivers Shift+vertical-wheel as `deltaX` with `deltaY = 0`, so here Shift+wheel would pan horizontally. To confirm: in Chromium on Linux/Windows, log `e.deltaX/e.deltaY` for a physical Shift+wheel notch over the schematic canvas; if deltaX is set, the fix is to treat `shiftKey && deltaY == 0` as the vertical axis with the swapped delta.
- **Already known?** Not listed.

### Checked and found to match (view controls)
- `VC_SETTINGS::Reset` / `common_settings.cpp` defaults: auto_pan false, auto_pan_acceleration 5, m_autoPanMargin 0.02, drag_middle/drag_right PAN, scroll_modifier_zoom 0, scroll_modifier_pan_h WXK_CONTROL, scroll_modifier_pan_v WXK_SHIFT, reverse_* false, zoom_acceleration false, zoom_speed_auto true (all but zoom_speed, V1).
- `onWheel`: modifier priority (Shift > Ctrl > Alt, `nMods > 1` forwarded), zoom when the modifier equals `m_scrollModifierZoom` with reverse, pan vector `ToWorld(screenSize) * rotation * 0.001`, `scrollX = hReverse ? v.x : -v.x`, `scrollY = -v.y`, `SetCenter(center + delta)` (our `panByWorldDelta` adds to the centre); the horizontal-wheel sign comes out the same as KiCad's once `toRotation`'s negation is counted (the comment at :120 "NOT negated" is misleading but the result matches); zoom about the cursor (the `center_on_zoom` warp is a documented deviation).
- `zoom_controller.cpp`: CONSTANT (clamp +-100, `1+r*s` / `1/(1-r*s)`), ACCELERATING (timeout 500, MIN_STEP 1.05, same-direction rule), `GetZoomControllerForPlatform` and the manual branch of `LoadSettings` (`MANUAL_SCALE_FACTOR * zoom_speed`, `ACCELERATING(zoom_speed)`); GTK3_SCALE not used in a browser is documented.
- `onButton`: middle/right PAN or ZOOM from IDLE, zoom anchored at the press point (`m_zoomStartPoint`), `dragActionOf`.
- `onMotion` DRAG_ZOOMING: `exp(d.y * zoomSpeed * 0.001)` with d.y = start - pos (up zooms in), incremental per motion event (`drag.lastY`).
- `handleAutoPanning` direction (minus V2), `onTimer` step: `borderSize = min(margin*w, margin*h)`, `accel = 0.5 + acc/5`, Resize to `borderSize*accel` at or beyond borderSize, to `borderSize` above half, `ToWorld(dir,false)`, `SetCenter(center + dir)`. The tick is rAF instead of the 250/60 ms timer (documented in `AUTO_PAN_TIMER_MS`; the pan speed per second therefore differs by the frame rate, unconfirmed how much in practice).

### Summary

| ID | Finding | Severity | Known? |
|---|---|---|---|
| E1 | A KiCad project's ERC severities, pin map and connection grid are not imported (the derived ERC reports what KiCad ignores; confirmed on QA issue22938) | high | no |
| C1 | Copy on a shared screen's second placement writes the screen's reference, not the path's | medium | no (merged 10-26) |
| C2 | The paste anchor uses only a polyline's first point | medium | no |
| C3 | Paste ignores the annotation method and start number | medium | no |
| C4 | The installed or built-in symbol beats the clipboard's cache, with no power-type check | medium | no |
| A1 | Placement and Repeat number max+1, not the first free number by the project's method; they never join a multi-unit part | medium | no (PARITY 5 claims parity) |
| A2 | A placed power symbol is numbered per screen (duplicates across sheets, no gaps reused) and written `#PWR10` | medium | no |
| A3 | Annotate's Selection scope drops a selected group | medium | no |
| E2 | A legacy duplicate exclusion loses its comment | medium | no |
| V1 | `input.zoom_speed` defaults to 5; KiCad's is 1 off macOS (a Zoom drag is 5x too fast) | medium | no |
| C5 | The schematic Paste Special has no "Clear net assignments" box | low | no |
| C6 | The long-text question's wording ("text text") and characters vs bytes | low | no |
| C7 | A copied power symbol is written `(in_bom no)` | low | no |
| C8 | Duplicate of a hovered item leaves the copy selected | low | no |
| C9 | Multi-unit joining compares the full lib_id, not the item name | low | no |
| A4 | The Annotate dialog disables controls and changes labels where KiCad does not | low | no |
| E3 | The ERC list is not sorted errors, then warnings, then exclusions | low | no |
| E4 | "Two" library checks start at Ignore; the code ignores three | low | the count is not |
| V2 | The auto-pan border is not truncated to whole pixels | low | no |
| V3 | (unconfirmed) Shift+wheel turned into deltaX by the browser pans horizontally | low | no |

Counts: high 1, medium 9, low 10 (one of them unconfirmed).

---

## Summary (all slices)

| Slice | High | Medium | Low |
|---|---|---|---|
| A connectivity, net names | 6 | 5 | 2 |
| B hierarchy, reader/writer | 2 | 4 (M5 = D-C1) | 4 |
| C edit tools, fields, placement | 3 | 7 | 9 |
| D clipboard, Annotate, ERC, view | 1 | 9 | 10 |
| **Total** | **12** | **25** | **25** |

| ID | Sev | One line |
|---|---|---|
| B-H1 | high | `eda import-kicad x.kicad_sch` drops every per-path reference of a shared screen (`place_by_box_corner` moves symbols, not `instance_overrides`; `tidy_shared_screens` deletes them): duplicate refs, merged nets on 6 QA hierarchies |
| B-H2 | high | Sheet files in a subfolder: the writer never creates the folder (export fails), and screens are keyed by `Sheetfile` text, not the resolved path |
| A-N1 | high | Global label / power symbol does not join a same-named local or hier label on its sheet (kicad-cli 25ec2335 joins) |
| A-N2 | high | A sheet pin joins a same-named local label on the parent sheet; kicad-cli does not |
| A-N4 | high | Global names tie-break by sheet depth; KiCad ignores depth for globals (both engines) |
| A-N5 | high | A power symbol does not outrank a hidden power-input pin's name (both engines) |
| A-N6 | high | `-Pad<n>` added when a pin name repeats in another unit (`shown_name_counts` counts all units; KiCad `GetPins( &aPath )`): every dual op-amp |
| A-N10 | high | Text variables in labels not resolved for connectivity: `${SHEETNAME}_EN` shorts a shared screen's placements |
| C-H1 | high | Dragging a wire on a sheet pin pulls it off the pin (`m_specialCaseSheetPins` never filled) |
| C-H2 | high | A wire drawn across a two-pin part is not trimmed (`TrimWire`); ours shorts the part |
| C-H3 | high | Placing a symbol on a wire: no `TrimOverLappingWires` / `AddJunctionsIfNeeded` |
| D-E1 | high | Import keeps a project's ERC exclusions but drops its severities, pin map and connection grid (QA `issue22938`: 50 spurious `pin_not_connected`) |
| mediums | | A-N3, N7, N8, N9, N11; B-M1..M4; C-M1..M7; D-C1..C4, A1..A3, E2, V1 (detail in each slice) |
| lows | | A-N12, N13; B-L1..L4; C-L1..L9; D-C5..C9, A4, E3, E4, V2, V3 |
