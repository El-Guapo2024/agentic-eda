# datasets/

A data-access layer letting the agentic-eda engine browse thousands of real
PCB/CAD designs for inspiration, without bulk-downloading them. Two things
live here: a **catalog** of what's actually out there and usable, and an
**access layer** for lazily fetching files out of it.

This directory is separate from `crates/` (the Rust engine) — nothing here
touches Rust code. It's a research/data-ops layer the agent (or a human)
queries on demand.

## What's here

- `CATALOG.md` — live-searched inventory of HF datasets (PCB family + 3D CAD
  family) and which ones are actually usable (license, gating, real file
  formats) vs. dead ends (gated, empty, images-only). **Read this first** —
  it explains why the PCB sample comes from GitHub-direct fetches rather
  than a Hugging Face dataset (no ungated HF dataset of raw `.kicad_pcb`
  files exists at the time of writing).
- `hfds.py` — CLI + Python module for lazy listing/reading/sampling files
  inside a Hugging Face dataset repo, without downloading the whole repo.
- `mount.sh` + `docker-fuse.md` — a real FUSE mount of a dataset repo as a
  directory tree (Linux/Docker primary target; macOS optional, needs
  macFUSE).
- `stats.py` — parses sampled `.kicad_pcb` files and reports layer count,
  outline size, footprint/net/track/via counts, and connector-to-edge
  distances.
- `samples/pcb/`, `samples/cad/` — small (~33MB total) real samples pulled
  this session, plus `manifest.json` provenance for the PCB ones.
- `SAMPLE_REPORT.md` — the `stats.py` output and what it means for the
  placement engine (the one-line version: **only ~3% of J\* footprints sit
  within 2mm of the board edge** in real designs — "connector near edge" is
  not a safe default).
- `cache/` — on-disk blob cache for `hfds.py`/`mount.sh` (HF_HOME points
  here). Gitignored-equivalent — it's scratch, safe to delete.
- `.venv/` — Python venv with `huggingface_hub`, `fsspec`, `fusepy`,
  `sexpdata` installed (used because `~/miniconda3/envs/cypress` didn't have
  `huggingface_hub`).

## Two modes

1. **Lazy CLI (`hfds.py`)** — works anywhere Python + `huggingface_hub` is
   available, including this Mac, with no kernel driver needed. Use this by
   default.

   ```bash
   cd datasets
   .venv/bin/python hfds.py ls electron-rare/kicad9plus-permissive
   .venv/bin/python hfds.py find Thingi10K/Thingi10K --ext .stl
   .venv/bin/python hfds.py cat electron-rare/kicad9plus-permissive dataset.jsonl | head
   .venv/bin/python hfds.py sample Thingi10K/Thingi10K --ext .stl -n 5 --out samples/cad
   ```

2. **FUSE mount (`mount.sh`)** — a real mounted directory tree where `ls`
   and `cat` transparently hit the Hub lazily. Needs a FUSE kernel driver:
   - **Linux / inside the `agentic-eda` Docker image**: works out of the
     box with `--cap-add SYS_ADMIN --device /dev/fuse` (see
     `docker-fuse.md` for the exact `docker run`/compose invocation).
   - **macOS**: needs macFUSE (a kernel extension requiring manual
     Privacy & Security approval + reboot — not something this script
     installs for you). Checked this session: **not installed**
     (`/Library/Filesystems/macfuse.fs` absent), so the mount was not
     tested on this Mac. `mount.sh` detects this and refuses with an
     explanation rather than silently no-op'ing.

## What's a stub

- `mount.sh` is **untested on macOS** (no macFUSE here) — it is written and
  guarded, not verified end-to-end. It should work as-is inside the Linux
  Docker image; that path also wasn't executed this session (would need a
  container run, which this task didn't require since the CLI path proved
  everything needed for `stats.py`).
- The 3D-CAD sample uses Thingi10K **mesh** (STL) files as a stand-in for
  STEP/B-rep, because every STEP-bearing dataset found (ABC, Fusion 360
  Gallery, DeepCAD) is either request-gated or hosted outside any API this
  session could call within the 2GB/no-manual-download constraints. See
  CATALOG.md Family B conclusion.
- The PCB sample is **not** from a Hugging Face dataset download — it's 12
  files fetched directly from GitHub repos that the `kicad9plus-*` HF
  dataset's own metadata pointed to as real provenance. `hfds.py` itself
  only talks to Hugging Face; it was not extended to fetch arbitrary GitHub
  repos, since that was a one-off discovery step, not a reusable access
  pattern worth productionizing here.

## Running the analysis end-to-end

```bash
cd datasets
.venv/bin/python stats.py samples/pcb
```

Output is captured verbatim in `SAMPLE_REPORT.md`.
