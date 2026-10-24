# Findings index (2026-10-23): what the parity reviews found, ranked for fixing

These come from findings-only reviews against KiCad 8303b2ad (and 25ec2335 for the newer items), run while `task-nanometres` changed the IR's unit. Each file holds, for every finding, our `file:line`, KiCad's function, a repro and the test to write. The fixes are cut from main **after** the nanometre merge.

| File | High | Medium | Low |
|---|---|---|---|
| p1-p2-footprints.md | 3 | 4 | 4 |
| p5-router.md | 7 | 21 | 19 |
| p6-pcb-edit.md | 4 | 14 | 4 |
| p7-p8.md | 2 | 4 | 4 |
| p11-p18.md | 2 | 9 | 10 |
| p12-sch-leftovers.md | 8 | 6 | 4 |
| p13-p16.md | 6 | 16 | 17 |
| p22-p24-p25.md | 7 | 13 | 9 |
| **Total** | **39** | **87** | **71** |

## Fix batches, by code area (so batches can run in parallel without conflicts), most urgent first

### A. KiCad compatibility of what we derive (`crates/kicad` writers, `crates/engine` naming). **The user chose option 1 (2026-10-23): KiCad's net names everywhere, the studio's display included.**
**Fixed** on `task-fix-a-kicad-names` (fc8ee220, 860f861a): F1, F1b, F2, F3, F4, F5, P14-2; kicad-cli's parity DRC finds 0 differences on `opamp_filter`, `ldo` and the routed `mcu_board_30plus` (76 before), and its netlist agrees with the board (`crates/cli/tests/kicad_names.rs`). Left: P14-2 for footprints read from a file and pads with an offset or a padstack; `nc_pins`' SOT-223 has no pad 4.
Opening a studio project in KiCad and running Update PCB (F8) renames every net and duplicates every footprint.
- p1-p2 F1: net names on the board are the intent's, not KiCad's (`/sheet/NAME`, `Net-(D1-Pad1)`), and our tracer agrees with neither.
- p1-p2 F2: one footprint has three names (empty Footprint field, `eda:0603`, `0603`). It should be the installed KiCad footprint everywhere.
- p1-p2 F4: no `(path ...)`, `sheetname` or `sheetfile` on the board's footprints.
- p1-p2 F3, F5 (medium): symbol fields not copied to the footprint; no `pinfunction`/`pintype`.
- p13-p16 P14-2: a flipped footprint is written in a frame KiCad never produces.

### B. Board reader and writer losses (`crates/kicad/src/import.rs`, `pcb.rs`)
- p22 F1: a zone with a hole becomes two zones, the hole a filled zone (`ZONE::AddPolygon`). Every board is affected.
- p22 F2: an SMD pad on `*.Cu` keeps only one side.
- p7-p8 F2: paste-only pads are dropped (the stencil loses openings).
- p12 O1: a `gr_curve` whose control point sits on an end point is dropped, which leaves the outline open.
- p12 O2: a footprint's Edge.Cuts are turned into board items (KiCad's footprint outline pass).
- p12 O3 / p13-p16 F16-1: a rectangle's corner radius is lost.
- p13-p16 P14-1: the Extruded 3D Body is dropped.
- p22 F3: the importers' `.kicad_dru` are never read.
- p13-p16 F6 (P15): a project's own DRC/ERC exclusions are dropped on import.

### C. Zone fill and teardrops (`crates/kicad/src/zone_fills.rs`, `fill_api`)
- p13-p16 P13-1: Fill All, and every refill before DRC or a plot, wipes every teardrop's fill, so Gerbers lose the copper. Causes: `(legacy_teardrops no)`, no pad/via teardrop parameters, and uuid-less rebuilt teardrops.

### D. Schematic connectivity (`crates/engine/src/nets.rs`, `crates/kicad/src/symbol_lib.rs`, `sch_import.rs`)
- p12 S1: two buses that share a member name are not joined (`bus_connection`, `issue17771`, 18 of the 20 `video` nets).
- p12 S2: hidden `power_in` pins of ordinary symbols are not global power.
- p12 S3: `${#}` is not resolved per placement, which shorts nets together.
- p12 S4: bus aliases are dropped on import and from `.kicad_pro`.
- p22-p25 P25-2: pins common to all units (unit 0) are kept only in unit A.
- p13-p16 F5 (P15): the ERC exclusion key merges a shared sheet's placements.

### E. Router (`crates/pns`)
- p5 P-1: a walk-around drag commits a detour round the whole board.
- p5 F2-1: a via that ends a route never reaches the board.
- p5 F2-2: Walk around with a via commits a clearance violation.
- p5 F2-3: Esc, or a refused finish, discards the fixed segments.
- p5 F1-1: a head ending in a via shoves other-layer tracks round its own segments.
- p5 F3-1: a via drag in Highlight collisions leaves its tracks at any angle.
- p5 F4-1: the diff-pair gap is raised to the P-N clearance (500 µm where KiCad uses 147 µm).
- Test infrastructure: `qa_regressions.rs` defaults to a macOS corpus path and passes without running. Its thresholds are 80%, where KiCad's tests demand an exact match.

### F. Editing operations (`crates/ops`, `web/studio`)
- p6 F2: point-editing or mirroring re-creates the item, which then leaves its group (data loss).
- p6 F3: an off-centre footprint's bounding box is symmetric about the origin, so Align, Distribute, the rotate pivot and hit testing are wrong.
- p6 F4: R/F while carrying turns about the wrong point; F then R turns the wrong way.
- p6 F1: Position Relative's angle is reversed, and its test asserts the wrong value.
- p22-p25 P25-1 / p7-p8 F1 (the same bug): Finish Pad Edit misses chained shapes.
- p22-p25 P24-1: Repeat Layout copies a copper shape with the reference channel's net (a short).
- p22-p25 P24-2: Repeat Layout puts copies into two groups.
- p11-p18 P18-1: Place Design Block drops copper on missing layers and unknown nets.
- p11-p18 P11-1: the Net Inspector is a stub, and its pad count counts footprints.
- p12 O10: placement uses the outline's bounding box.

## Docs found wrong (fix with the batches)
- PARITY-pcb.md §25: the `issue24078` claim and the description of the closing arc.
- PARITY-pcb.md §29: "keeps the dash" is false on the board.
- PARITY-pcb.md §36: the "identical in effect" claim for design-block references.
- The note that a `--refill-zones` run skips courtyard checks (not true on `fill_bad`).
- CODE-COMPARE-router.md and `dp_placer.rs`: stale notes (listed in p5-router.md).
