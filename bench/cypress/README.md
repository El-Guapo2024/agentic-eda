# Cypress: native CPU build (default) or GPU pod

**2026-09-04: Cypress runs natively on this Mac, CPU only.** `eda place --placer
cypress` and `bash bench/cypress/run_local.sh` use `~/ws/Cypress/install`
(conda env `cypress`, `CYPRESS_PYTHON=~/miniconda3/envs/cypress/bin/python`).
The Modal GPU path below is optional now.

## CPU-path fixes applied in ~/ws/Cypress (uncommitted there, on top of the
## 12 build patches)

Cypress marks every PCB part as a "macro" on purpose (`update_macros`,
thresholds 0) — that is the authors' PCB flow and was never the problem.
The CPU path was broken by five separate things:

1. **Two OpenMP runtimes in one process** (torch's `libiomp5` + Homebrew's
   `libomp` linked into every kernel) → segfault in the first parallel loop.
   Fixed by configuring CMake's OpenMP to torch's `libiomp5.dylib` plus a
   no-op `__kmpc_dispatch_deinit` shim in `ops/utility/src/kmp_shim.cpp`
   (Apple clang 17 emits that symbol; the older runtime lacks it).
2. **torch ≥ 2.0 `zero_grad()` sets grads to None** → Nesterov skipped the
   position tensor and crashed on an empty list. `zero_grad(set_to_none=False)`.
3. **`pin_pos` CPU kernel had no rotation** → ported the CUDA rotation math
   (theta/h/w, `[grad_x | grad_y | grad_theta]` layout) so CPU == GPU contract.
4. **`net_crossing` CPU kernel produced NaN for parallel/degenerate segment
   pairs** (x/0 slopes × 0 bell weight) → skip pairs with zero determinant or
   outside the bell support. This was the ~1-in-8 "diverged" run.
5. **Nesterov step-size 0/0 guard** for tiny problems.

Cypress exits 0 even when it diverged (it writes the input placement back);
`eda-cypress` reads `DREAMPlace.log` and fails with `cypress_diverged` instead.
Runs are `deterministic_flag: 1, num_threads: 1` — same seed, same answer.

**stop_overflow is 0.30 by default (`CYPRESS_STOP_OVERFLOW`).** Cypress'
overflow metric has a geometric floor on macro-only boards (every PCB part
is a macro), so the chip default 0.07 is unreachable on some boards — they
then run all iterations and Cypress skips legalisation ("diverged" in the
report) even though the global placement is fine. With 0.30 all 11 corpus
boards place, pass our placement gate, route, and pass kicad-cli DRC
(report_local.md); with 0.07 two of them never stop. Legality is our gate's
call, not the overflow number.
Bookshelf: courtyards round UP to the 100 µm grid, board rounds DOWN, nets
are emitted as `n<i>` (a net named `SCL` is a Bookshelf keyword and breaks
the parser); a refdes that is a keyword is a hard error.


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
