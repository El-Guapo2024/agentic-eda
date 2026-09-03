# Benches

Every bench in this repo, and how to run it.

## Corpus bench (`crates/bench`)

Runs the full library pipeline (lint -> schematic -> placement -> routing,
each gated) over every `examples/*.yaml` intent at seeds `{0, 1, 2}`,
checks the outcome against a small expectation table, and asserts
determinism (same seed => byte-identical `design.json` across two runs).

```
cargo test -p eda-bench --test corpus -- --nocapture
```

Writes a scorecard (stage reached, fail checks, schematic crossing-count
hint, hpwl, tracks, vias, wall time per case/seed) to
`target/bench/scorecard.md` and prints it to stdout. Finishes in well
under a minute in a debug build. See
`crates/bench/tests/corpus.rs` for the expectation table, including
which cases are deliberately-failing (`ExpectedFail`, e.g.
`unroutable_tiny_outline`) versus `KnownFail` (a genuine engine/place/
router bug the case uncovered, kept failing rather than hidden).

## ELK differential bench (`bench/elk`)

Compares `crates/layout`'s Rust port of the ELK "layered" algorithm
against the original `elkjs` on a shared corpus of graphs, to catch
layouts where the port does meaningfully worse than what it was ported
from.

```
bash bench/elk/run.sh
```

See `bench/elk/README.md`.

## circuit-json interchange bench (`bench/circuit-json`)

Exercises `eda-interchange`'s `to_circuit_json` output against the
`circuit-json`/`tscircuit` JS toolchain (rendering, connectivity-map
extraction) to catch interchange-format drift.

```
cd bench/circuit-json && npm install && npm run bench   # see package.json for the exact script name
```

See `bench/circuit-json/README.md`.

## KiCad export bench (`bench/kicad`)

Exercises `eda-kicad`'s `.kicad_sch`/`.kicad_pcb` export against real
KiCad (or `kicad-cli`) to catch files KiCad itself rejects or renders
oddly. Being built alongside this one — see `bench/kicad/README.md` for
the current state and how to run it.
