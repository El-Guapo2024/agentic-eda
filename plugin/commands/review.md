---
description: Build one page comparing the runs of a batch, with gate results, placement, routing and 3D renders
argument-hint: [runs dir]
---
For the batch directory `$ARGUMENTS` (default: newest under `runs/`): run `${CLAUDE_PLUGIN_ROOT}/bin/eda-rank <dir>` for the ranked table (pass first, then fewer layers, vias, track length, wall time; failed runs show their first failure and the suggested knob), and read each run's judge renders. If kicad-cli exists on this machine, render 3D views with `${CLAUDE_PLUGIN_ROOT}/../bench/loop/render3d.py <run>/<name>.kicad_pcb <run>/render3d.png`; otherwise say the 3D view is unavailable here rather than skipping silently. Publish an Artifact page with a leaderboard and one section per run (placement, routing, 3D), and close with what to change in the intent for the next batch.
