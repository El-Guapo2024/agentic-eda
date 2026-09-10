---
name: agentic-eda
description: How to run the agentic-eda board toolkit through its container: intent authoring checklist, hard-fail rules, batch and review flow.
---
# agentic-eda

The engine runs in a container. Always call it through `${CLAUDE_PLUGIN_ROOT}/bin/eda`; never install Rust, Python or Cypress on the host for it. Paths are relative to the current directory, which is mounted at /work.

## Rules that do not bend
- Hard fail, no fallbacks. A gate failure names its cause; report it as-is. Do not retry with looser settings unless the user says so.
- Parts come from live distributor search or the user, never from memory.
- Every solver knob is in the intent: `allow:` bounds what solve may vary, `solver:` sets the placer, `board.tuning:` the router. Defaults exist for all; write only what the user changed.

## Intent checklist (ask before running if missing)
1. Board outline in µm, or "pick one" (then start from the template's and note it).
2. Connectors that must sit on an edge, and which edge.
3. Anything with current or heat: regulators, drivers, power connectors.
4. Layer budget: what `allow.max_layers` may reach.
5. Placers allowed: `cypress` (default, analytical), `anneal` (native).

## Flow
`/board` draft the intent → user corrects in words → `/batch` several seeds × placers → `/review` one page → user points at a run → edit the intent → `/batch` again.

## Reading results
- `log.txt` per run: gate lines `schematic|placement|routing gates: N checks, F fail, W warn`, then `PASS`/`FAIL <gate> @ <where>: <why>`.
- `strategy.txt`: the ladder rungs tried and which one passed.
- `judge_placement.png`, `judge_routing.png`: renders. `render3d.png` only where KiCad exists on the host.
