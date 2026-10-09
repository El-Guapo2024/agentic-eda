# Zone-fill parity against `kicad-cli pcb drc --refill-zones --save-board`

Measured 2026-10-08 with kicad-cli 10.99.0-857 (nightly) on the QA boards of the KiCad snapshot `8303b2ad` (`qa/data/pcbnew`), in a debug build.
**Before** is the filler as it stood at `a85c64b`: four axis-aligned spokes, no hatch, no corner smoothing, no pad or footprint overrides, no
board-edge clearance, and each zone filled on its own. **After** is `task-zone-fill`. Both columns come from the same harness run against both
trees, so they compare like with like.

## Reproduce

```
EDA_SLOW_TESTS=1 cargo test -p eda-zone-filler --test kicad_cli_parity kicad_cli_zone_fill_feature_parity -- --nocapture
```

runs the thirteen boards in `FEATURE_BOARDS` (the ones a debug build fills in about half a minute each) and **fails when any fill is more than
1% of its area, or 1.5% XOR, away from kicad's**, so it is a regression test as well as a table. Needs `kicad-cli` on `PATH` or at
`/Applications/KiCad/KiCad.app/Contents/MacOS/kicad-cli`, and the QA boards (`KICAD_QA_DATA` names `qa/data` of the KiCad sources).
Environment variables:

- `PARITY_BOARDS=a.kicad_pcb,dir/b.kicad_pcb` measures other boards of `qa/data/pcbnew` and does not assert (`stonehenge` and the three large
  boards of the table are measured this way);
- `PARITY_SVG_DIR=dir` writes `<board>-<layer>-<n>.svg`: kicad's fill red under ours blue, purple where they agree;
- `PARITY_CROP=x0,y0,x1,y1` (mm) frames part of the board in the SVG; `PARITY_TMP=dir` is the work directory.

`... kicad_cli_zone_fill_parity -- --ignored --nocapture` is the original harness (three boards, zones picked by hand): since the work its
eight fills read 0.20, 0.12, 0.03, 0.00, 0.04, 0.20, 0.10 and 0.01% of area, and every island count equals kicad's (the old table had
17.31 and 2.87% on `issue7086` and one island count off on `notched_zones`).

## What is compared

The harness copies a QA board, imports it with `import_kicad_pcb` (so every zone setting and every pad's and footprint's zone overrides reach
the filler as the studio sees them), runs `kicad-cli pcb drc --refill-zones --save-board` on the copy, and compares the two fills zone by zone
in `eda_clipper2` integer coordinates: the area difference, the XOR (the symmetric difference of the two fills, as a share of kicad's area:
equal areas can hide a shifted shape, so this is the number that counts) and the island count. A zone is matched with its refilled twin by
outline, net and priority (`stonehenge` has two zones with one outline). Teardrop zones and rule areas are not compared.

## Result

Each cell reads *area difference / XOR / islands*, area and XOR as a share of kicad's area. 14 boards, 37 fills (the large boards are below).

| board | zone (net, priority) | layer | kicad mm² | before: area diff / XOR of kicad / islands | after: area diff / XOR of kicad / islands | kicad islands |
|---|---|---|---|---|---|---|
| issue7086 | /ThisIsNot_a_NoName_net (prio 0) | F.Cu | 543.908 | 17.17% / 18.39% / 1 | 0.20% / 0.22% / 1 | 1 |
| issue7086 | Net-(J3-Pad3) (prio 1) | F.Cu | 113.399 | 2.77% / 2.97% / 1 | 0.12% / 0.15% / 1 | 1 |
| solder_mask_bridge_test | GND (prio 0) | F.Cu | 754.818 | 0.29% / 0.47% / 1 | 0.03% / 0.04% / 1 | 1 |
| notched_zones | GND (prio 5) | F.Cu | 32.555 | 0.07% / 0.07% / 1 | 0.00% / 0.00% / 1 | 1 |
| notched_zones | +5V (prio 1) | F.Cu | 303.694 | 0.11% / 0.14% / 1 | 0.01% / 0.01% / 1 | 1 |
| notched_zones | GND (prio 2) | F.Cu | 54.061 | 0.17% / 0.23% / 2 | 0.00% / 0.00% / 2 | 2 |
| notched_zones | GND (prio 0) | F.Cu | 332.936 | 0.05% / 0.36% / 1 | 0.07% / 0.08% / 3 | 3 |
| notched_zones | GND (prio 0) | B.Cu | 2947.276 | 0.00% / 0.01% / 1 | 0.01% / 0.01% / 1 | 1 |
| zone_filler | GND (prio 0) | F.Cu | 806.856 | 0.44% / 1.13% / 1 | 0.14% / 0.16% / 1 | 1 |
| issue21746 | VDD (prio 1) | F.Cu | 1246.565 | 434.67% / 435.86% / 2 | 0.26% / 0.27% / 1 | 1 |
| issue21746 | GND (prio 0) | F.Cu | 5206.611 | 100.00% / 100.00% / 0 | 0.12% / 0.12% / 1 | 1 |
| issue2568 | GND (prio 0, hatch) | F.Cu | 2389.007 | 35.16% / 37.23% / 1 | 0.26% / 0.92% / 1 | 1 |
| fill_bad | A (prio 1) | F.Cu | 73.611 | 1.20% / 1.23% / 1 | 0.04% / 0.05% / 1 | 1 |
| issue16182 | +3V3 (prio 5) | F.Cu | 8.599 | 1.18% / 1.22% / 1 | 0.19% / 0.19% / 1 | 1 |
| issue16182 | GND (prio 1) | F.Cu | 607.780 | 70.80% / 71.54% / 5 | 0.18% / 0.21% / 4 | 4 |
| issue16182 | GND (prio 5) | F.Cu | 8.840 | 0.24% / 0.24% / 1 | 0.00% / 0.00% / 1 | 1 |
| issue1358 | GND (prio 0) | F.Cu | 1402.602 | 5.16% / 5.81% / 3 | 0.08% / 0.08% / 2 | 2 |
| issue1358 | GND (prio 0) | B.Cu | 1438.182 | 5.36% / 5.36% / 1 | 0.06% / 0.06% / 1 | 1 |
| issue12831 | GND (prio 0) | F.Cu | 67.744 | 27.31% / 33.98% / 1 | 0.53% / 0.60% / 1 | 1 |
| hatch_thermal_connectivity | A (prio 0, hatch) | F.Cu | 60.964 | 115.63% / 115.69% / 1 | 0.07% / 0.58% / 1 | 1 |
| hatch_thermal_connectivity | A (prio 0, hatch) | B.Cu | 60.964 | 115.63% / 115.69% / 1 | 0.07% / 0.58% / 1 | 1 |
| issue2904 | GND (prio 0) | F.Cu | 1581.397 | 6.55% / 13.05% / 6 | 0.21% / 0.28% / 1 | 1 |
| issue2904 | +12V (prio 0) | B.Cu | 1992.070 | 8.49% / 8.67% / 16 | 0.12% / 0.14% / 16 | 16 |
| issue17429 | AGND (prio 40) | F.Cu | 100.004 | 33.59% / 33.59% / 1 | 0.37% / 0.40% / 1 | 1 |
| issue17429 | AGND (prio 70) | F.Cu | 2476.835 | 2.93% / 2.98% / 3 | 0.07% / 0.08% / 2 | 1 |
| issue17429 | AGND (prio 70) | B.Cu | 2445.961 | 3.79% / 3.83% / 8 | 0.07% / 0.08% / 1 | 1 |
| issue17429 | AGND (prio 0) | B.Cu | 100.004 | 33.59% / 33.59% / 1 | 0.33% / 0.46% / 1 | 1 |
| issue17429 | AGND (prio 40) | In1.Cu | 2679.290 | 3.97% / 3.99% / 1 | 0.07% / 0.07% / 1 | 1 |
| issue17429 | (prio 40) | In2.Cu | 2658.114 | 4.00% / 4.02% / 1 | 0.07% / 0.08% / 1 | 1 |
| issue17429 | AGND (prio 40) | In3.Cu | 2661.444 | 4.00% / 4.01% / 1 | 0.07% / 0.08% / 1 | 1 |
| issue17429 | AGND (prio 40) | In4.Cu | 2629.343 | 4.04% / 4.07% / 1 | 0.08% / 0.09% / 1 | 1 |
| issue17429 | (prio 0) | In5.Cu | 2640.127 | 4.03% / 4.05% / 1 | 0.08% / 0.08% / 1 | 1 |
| issue17429 | AGND (prio 0) | In6.Cu | 2657.251 | 4.00% / 4.02% / 1 | 0.07% / 0.08% / 1 | 1 |
| stonehenge | (prio 0) | B.Cu | 0.094 | 100.00% / 100.00% / 0 | 187.85% / 199.33% / 2 | 1 |
| stonehenge | /D9 (prio 2) | B.Cu | 60.701 | 1.12% / 1.19% / 1 | 0.46% / 0.49% / 1 | 1 |
| stonehenge | GND (prio 1) | B.Cu | 1507.492 | 17.90% / 20.46% / 9 | 11.33% / 11.74% / 3 | 3 |
| stonehenge | /D8 (prio 3) | B.Cu | 222.228 | 3.30% / 3.85% / 2 | 0.18% / 0.38% / 1 | 1 |

**Over these 37 fills the area is within 0.5% of kicad's on 34 (8 before), within 1% on 35, and the island count equals kicad's on 35 (26
before).** The two that are not are both on `stonehenge`, whose ring-shaped custom pads are the known gap below.

### The large boards

Measured one board at a time with `PARITY_BOARDS` (debug build; the times include kicad-cli's own fill).

| board | fills | area within 0.5%: before to after | within 1% | median area difference | worst area difference | island counts equal | time |
|---|---|---|---|---|---|---|---|
| issue14559 | 129 | 85 to 125 | 87 to 128 | 0.19% to 0.02% | 15.05% to 1.19% (GND, B.Cu) | 128 to 129 | 93 s |
| issue11814 | 19 | 3 to 9 | 4 to 13 | 5.73% to 0.51% | 232.92% to 119.36%, a 0.005 mm^2 zone (kicad has 7 islands of it, we 1); next 23.44% | 16 to 16 | 23 s |
| issue5093 | 23 | 3 to 14 | 6 to 14 | 3.44% to 0.19% | 1431.83% to 765.68% (`/right/GND`, hatched, F.Cu) | 17 to 19 | 11 min |

**Over all 208 fills of the 17 boards the area is within 1% of kicad's on 190 (105 before), within 0.5% on 182 (99 before), and the island count equals
kicad's on 199 (187 before).** The 18 fills above 1% are 2 on `stonehenge`, 9 on `issue5093`, 6 on `issue11814` and 1 on `issue14559`.

The misses left on `issue5093` are three causes. The 766% row (`/right/GND`, F.Cu; 1008% on a solid-filled copy of the board) is the custom pads
of footprint `R_pads`: 16 large copper shapes of other nets that cover most of that zone, of which the importer keeps only each one's 1.5 mm anchor
(filled solid, kicad-cli's fill keeps 374 mm^2 of the zone's 4700, and 4260 with that one footprint deleted from a copy). The 13.9% row (`/left/P14`, B.Cu, a
strip along the board's arc-shaped edge) is the Edge.Cuts chord below, and it got worse than its 6.6% before because the edge clearance is now kept from
a chord. The 42.7% row (`/left/GND`, a diamond zone inside the hatched `/left/GND`, 32 islands against kicad's 5) and 2.95% (`/left/GND`, B.Cu) are not
explained.

The misses left on `issue11814` are one cause, and it is in the importer: the board edge has a 3.2 mm semicircular notch, and `import_outline`
turns an Edge.Cuts `gr_arc` into its chord (its module doc says "tessellated", the code pushes `(start, end)`), so the fill does not keep
`copper_edge_clearance` from the notch. With the arc tessellated through its mid point, as an experiment that was **not committed** (it
changes the outline of every imported board that has an arc, which is outside this crate, and `REPORT.md`'s round trip numbers would have to be
measured again), the 23.44% row went to 0.20% and the three rows of 2.1 to 3.4% to 0.68, 0.16 and 0.32%: every row of the board but the sliver is
then within 1.2% of the area (the XOR of the second hatched zone stays at 3.1%). `issue17429`, whose two outline arcs are small, moved by about 0.01%.

## What moved the numbers

A board usually mixes several of these, so each line says which features a board exercises, not a measured split.

- **`fillCopperZone`'s clearances.** The fill keeps `copper_edge_clearance` (0.5 mm by default) from the board edge, and a pad's or footprint's
  own `(clearance ...)` replaces the net class's and the zone's for that pad, floored at the board minimum (`DRC_ENGINE::EvalRules`; 0 is not an
  override). The strip along the outline is most of what the old filler got wrong on boards whose zone runs to the edge: the XOR before was
  81 mm^2 on `issue1358` (a 156 mm outline), 107 mm^2 on the inner layers of `issue17429` (213 mm), 23 mm^2 on the 10 by 12 mm board of
  `issue12831`, and `issue7086` (17.2% before) adds pads that ask for 1.7 mm.
- **`ZONE_FILLER::Fill`.** A zone is knocked out by the *fills* of the zones above it, not their outlines, and a zone whose fragments touch no
  pad of its net loses them (`FillIsolatedIslandsMap`: connectivity across tracks, vias and the zones of other layers); the zones below then
  get the space back (the iterative refill, `refillZoneFromCache`). `issue21746`: the VDD zone spans the board but only 1247 mm^2 of it reaches
  a pad (434.7% before, with GND given nothing, 100%; 0.26 and 0.12% now). `issue16182` GND (70.8% before, 5 islands to kicad's 4) and `issue2904`
  GND (6.55% before, 6 islands where kicad has 1) are in the same family.
- **`buildThermalSpokes` and `EvalZoneConnection`.** The spokes follow the pad's rotation, its spoke angle (the pad's own, else 90 degrees for
  an oval or rectangle and 45 otherwise, circles built at 0 degrees and turned), width and gap (the pad's own over the zone's), and only the
  ones that reach copper stay; the connection a pad gets is its own, else its footprint's, else the zone's ("PTH only" thermal-relieves plated
  holes). `issue14559` is the board for it, 452 pad connections and 96 spoke angles on 129 fills (median 0.19% before, 0.02% after);
  `issue12831` and `issue5093` set spoke angles too.
- **`addHatchFillTypeOnZone` and `buildHatchZoneThermalRings`.** `issue2568` (35.2% before; the old filler drew a solid zone),
  `hatch_thermal_connectivity` (115.6%) and the two hatched zones of `issue11814` (232.9% and 149.7%, to 0.20% and 0.40%).
- **`ZONE::BuildSmoothedPoly`.** Chamfer and fillet corner smoothing before the fill, on `fill_bad` (1.20% before, with the pad clearances),
  `notched_zones`, `issue16182`, `issue17429` and 107 zones of `issue14559`.

## Bugs the table caught (kept as a record)

- An earlier version of this file put `issue7086`'s 17.3% down to the net-class resolution in `eda_drc::constraints::clearance`, after a flip of
  the knockout formula made it worse. That was wrong: the cause was the board-edge clearance and the pads' own clearances (above).
- Pads whose own clearance is larger than the zone's were skipped when they lay farther away than the worst clearance the fill considered
  (seen on `fill_bad`): `worst_clearance` now includes the pads' and footprints' overrides.
- The harness matched a zone to its refilled twin by outline only and gave two zones of `stonehenge` (GND and a netless one) the same fill:
  it matches by outline, net and priority now.
- Earlier, from the first version of this file: the knockout by a different-net zone and the subtraction of a same-net zone were applied in both
  directions, where `ZONE_FILLER` only ever knocks out the zone of the lower priority (`issue7086`'s priority-1 zone was wiped out: 100%, then
  2.87%); the harness listed only a hand-picked subset of a board's zones, so `notched_zones`, which has several zones on one net and layer at
  different priorities, was under-knocked-out (three rows from double-digit percentages to under 0.2%); and `keep_spokes` allocated a
  `ShapePolySet` per comparison, O(n^2), which a 600-pad synthetic zone took 5.7 s for and now takes 0.34 s.

## Left, with the evidence

- **Custom pads: the biggest gap left.** `issue5093`'s `/right/GND` (766%, above) and `stonehenge` GND on B.Cu (11.3%: its four mounting pads are rings,
  `(options (anchor circle))` with a `gr_circle` primitive 5 mm across; the pale-blue discs of the SVG overlay are copper we have and kicad has not). The importer
  keeps the anchor of a custom pad, so the knockout, the clearance and the spokes of the real shape are missing. The IR has no custom pad shape; adding one (the
  primitives, written back by the exporter so that kicad-cli sees them too, and `PAD::GetEffectiveShape` for the proxy spokes) is the next step.
  `custom_pads.kicad_pcb` is not measured.
- **Hatch.** The area is within 0.3% but the XOR is 0.6 to 0.9% (`issue2568`, `hatch_thermal_connectivity`), and the overlay puts it inside the
  thermal rings of the hatched zone's pads: about 0.6 mm^2 per pad, where kicad-cli's nightly has bars along the axes inside a ring and the port
  has pieces of the hatch. The snapshot's `buildHatchZoneThermalRings` adds a ring and no spokes; whether the nightly changed that or the port
  differs is not settled.
- **`connect_nearby_polys`** (a robustness pass for concave, nearly touching geometry) is not ported.
- **Outlines and holes (importer).** Edge.Cuts arcs are imported as chords (above; one `tessellate_arc` call in `import_outline` fixes it, and is the
  cheapest remaining gain), an Edge.Cuts outline with interior cutouts is read as its outer loop only, a board whose Edge.Cuts chain is open
  (`issue12609`) gets a bad outline loop, and mask-only NPTH holes are dropped, so none of them knocks copper out. No courtyard
  knockouts or net-tie exemptions; no conditional pad flashing or backdrill.
- **Zones.** One layer per zone, no non-copper zones (`fillNonCopperZone`); teardrop zones are skipped by the harness.
- **Time.** `post_knockout_min_width_prune` (a deflate and a round-cornered inflate, as in KiCad) takes all the time on a hatched zone with a fine pitch:
  `issue5093` has two such zones, gaps of 0.4 mm over a whole board, and a debug build needs 11 minutes for it (on a loaded machine) where the filler of `a85c64b`, which filled
  hatch solid, took under half a minute. Typical boards are unchanged in order of magnitude (`issue2568`, one hatched zone: 20 s against 12 s with kicad-cli's
  own run in both). No release timing was taken.
- **kicad-cli's version.** The nightly in use is newer than the snapshot the code was ported from; the rows above are within what that explains.
