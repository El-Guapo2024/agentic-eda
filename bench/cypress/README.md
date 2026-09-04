# Cypress on a GPU pod

[NVlabs/Cypress](https://github.com/NVlabs/Cypress) (Apache-2.0) is the ISPD'25
GPU analytical PCB placer built on DREAMPlace. We never run it locally: this
Mac has no NVIDIA GPU, Docker's daemon is off and the disk can't hold the
NVIDIA PyTorch base image. Instead it runs on a [Modal](https://modal.com)
GPU function; the Mac only ships Bookshelf text files up and a `.pl` back.

Why Bookshelf and not KiCad: Cypress's `pcb-util` KiCad converter is a
library with hard-coded paths, while Bookshelf (`.aux/.nodes/.nets/.pl/.scl`)
is what `Placer.py` actually reads. `eda export` writes it straight from
`design.json` (`crates/interchange/src/bookshelf.rs`, 100 µm units,
courtyards as nodes, pad offsets as pins), and `eda import-pl` reads the
placement back so our gates, HPWL and router judge Cypress exactly like
`eda-place`.

## One-time setup

    pip install modal
    modal setup            # opens a browser login

The first `modal run` builds the image (NVIDIA base + Cypress compiled for
T4/A10G, ~15–25 min); later runs reuse it.

## Run one board

    ./target/debug/eda place examples/ldo.yaml -o out/ldo --seed 0
    ./target/debug/eda export examples/ldo.yaml --design out/ldo/design.json -o out/ldo
    modal run bench/cypress/modal_app.py --bookshelf-dir out/ldo/bookshelf --name ldo
    ./target/debug/eda import-pl examples/ldo.yaml --design out/ldo/design.json --pl out/ldo/cypress/ldo.gp.pl -o out/ldo/cy
    ./target/debug/eda route examples/ldo.yaml --design out/ldo/cy/design.json -o out/ldo/cy

`--gpu 0` runs DREAMPlace's CPU path on the same image (slow, useful to
verify the bridge without a GPU).

## Run the bench

    bash bench/cypress/run.sh 0     # writes bench/cypress/report.md

Columns: our placer's HPWL, Cypress's HPWL (both from `eda` on real pad
centres), the ratio, whether Cypress's placement passes `check_placement`
(courtyard overlap / outline / proximity), whether `eda-router` then routes
it, and kicad-cli DRC on the result.

## Knobs (modal_app.py)

`--target-density` (0.5–1.0, default 0.6), `--bins` (default 64 for boards
this small; Cypress tunes 256–1024 on its benchmarks), `--seed`. The config
keys mirror `test/tune/pcb-configspace.json` defaults.
