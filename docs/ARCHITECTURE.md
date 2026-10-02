# Architecture: one design, edited live by people and agents

Decided 2026-10-02. This replaces the earlier rule that every check must
run on our own Rust ports.

## Source of truth

`design.json` (plus intent) in the board directory is the only source of
truth. KiCad files (`.kicad_pcb`, `.kicad_sch`) are always derived from it,
never edited as masters.

## Who edits, and how

- **The studio UI**: every edit is an `/api/cmd` verb (`crates/ops` `Cmd`)
  that writes `design.json`.
- **An agent**: runs the same verbs through the CLI (`eda board ...`, with
  `EDA_ACTOR=agent`), or edits `design.json` directly.
- **Live view**: the studio polls `/api/version` (a stamp of `design.json`
  + `activity.jsonl`) and reloads the scene, checks and history whenever
  anything changed, whoever changed it. The agent reads the file fresh on
  every command, so it sees the person's edits too.
- **Shared view state**: `view.json` (`/api/view`, `eda board gui`) holds
  the active tab, selection, camera and a one-shot zoom-to. The studio
  pushes its own as `ui` and applies anyone else's, so an agent can see
  what is selected and drive the view.
- **One shared history**: `.history/undo|redo` snapshots, each tagged with
  its author (`ui`, `cli`, `agent`, ...). A direct edit of `design.json`
  (detected against `.history/head.json`) becomes its own `file` step, so
  undo reverts it first instead of silently overwriting it, and redo
  brings it back.
- **No merging**: two writers touching the file at the same instant ->
  last write wins. Edits one after another, or to different things, are
  fine.

## Engines

- **kicad-cli is the only engine** for batch work it already does exactly:
  DRC, ERC, plots, Gerber/drill/position/STEP exports, netlist, BOM. The
  backend exports the current `design.json` revision, runs kicad-cli, and
  maps the report back to our item ids (`crates/cli/src/kicad_engine.rs`,
  `eda board drc --kicad`, `GET /api/drc?engine=kicad`; derived files go
  to `.kicad/` beside `design.json`). Target: KiCad **nightly** (10.99,
  kicad-dev-nightly PPA, matching the master source we port from),
  installed by `tools/install-kicad-nightly.sh`; `EDA_KICAD_CLI` overrides
  the binary. Zones are refilled by kicad-cli (`--refill-zones`) when the
  installed version has it (nightly does; 9.0 does not).
- **Our own ports** are kept for what kicad-cli cannot do or cannot do fast
  enough interactively:
  - the interactive router (`crates/pns`, a port of KiCad's PNS),
  - the editor UI, tools, dialogs and hotkeys (KiCad look and feel),
  - live connectivity / ratsnest / zone fill while editing,
  - our own additions: intent, placement, schematic derivation.
- **Never duplicate kicad-cli in Rust**, not even as a faster version
  (decided later on 2026-10-02, replacing "the Rust DRC/ERC ports stay as
  fast previews"). One answer per question, no parity work. A kicad-cli
  DRC run takes about 3.5 s on a 30-part board, which is fine for the
  dialogs, the canvas markers and the placement gates.
  - **To delete** (cleanup in progress): the Rust DRC rule providers
    (`crates/drc/src/providers/`), the KiCad ERC port
    (`crates/kicad/src/erc.rs`), `crates/fab` (Gerber/drill/position), and
    the plot/netlist/BOM exporters in `crates/kicad`, except any part a
    live feature needs (on-screen rendering, live connectivity, the
    schematic-to-PCB sync).
  - **To keep**: the geometry core in `crates/drc` (shapes, clearance
    rules, rule expressions, rtree, zone fill, stroke font). The router,
    zone filler and renderer are built on it.
  - **Our own checks** that KiCad does not have (placement quality,
    schematic readability, dangling detection for live connectivity) live
    in a separate fast checker, `lint`. It is never named DRC or ERC and
    never holds a copy of a KiCad rule.
