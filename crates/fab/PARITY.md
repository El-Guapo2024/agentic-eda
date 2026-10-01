# crates/fab parity vs kicad-cli (10.99.0)

Fixture: `crates/fab/tests/kicad_cli_parity.rs`'s hand-built board (every pad
shape/kind, a track per copper layer, a via, a net-attached zone fill).

## Gerber

| layer | kicad apertures | ours | kicad flashes | ours | kicad regions | ours | kicad area um2 | ours | area delta |
|---|---|---|---|---|---|---|---|---|---|
| parity_fixture-F_Cu.gtl | 9 | 9 | 8 | 8 | 0 | 0 | 0 | 0 | 0.00% |
| parity_fixture-B_Cu.gbl | 5 | 5 | 4 | 4 | 1 | 1 | 248166458 | 248166458 | 0.00% |
| parity_fixture-F_Mask.gts | 7 | 7 | 7 | 7 | 0 | 0 | 0 | 0 | 0.00% |
| parity_fixture-B_Mask.gbs | 3 | 3 | 3 | 3 | 0 | 0 | 0 | 0 | 0.00% |
| parity_fixture-F_Paste.gtp | 4 | 4 | 4 | 4 | 0 | 0 | 0 | 0 | 0.00% |
| parity_fixture-B_Paste.gbp | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0.00% |
| parity_fixture-F_Silkscreen.gto | 1 | 1 | 0 | 0 | 0 | 0 | 0 | 0 | 0.00% |
| parity_fixture-B_Silkscreen.gbo | 1 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0.00% |
| parity_fixture-Edge_Cuts.gm1 | 1 | 1 | 0 | 0 | 0 | 0 | 0 | 0 | 0.00% |

## Drill

kicad-cli tools: 4, ours: 4

kicad-cli hole/slot lines: 4, ours: 4


## Position (CSV)

kicad-cli header: `Ref,Val,Package,PosX,PosY,Rot,Side`

ours: `Ref,Val,Package,PosX,PosY,Rot,Side`

rows -- kicad-cli: 2, ours: 2
