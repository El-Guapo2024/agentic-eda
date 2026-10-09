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
  DRC, ERC, plots, Gerber/drill/position/STEP exports, netlist, BOM, board
  statistics. `crates/kicad-engine` exports the current `design.json` revision
  to derived KiCad files (`.kicad/` beside `design.json`, scratch, rewritten on
  every run), runs kicad-cli, and maps the report back to our item ids through
  the exporter's uuid map; `crates/cli/src/kicad_engine.rs` is the board
  directory's face of it. Entry points: `eda board drc|erc|export|lint`,
  `eda fab ...`, `GET /api/drc`, `GET /api/erc`, `POST /api/board_stats`,
  `POST /api/fab/*`, `POST /api/sch/plot|netlist`. Target: KiCad **nightly**
  (10.99, kicad-dev-nightly PPA, matching the master source we port from),
  installed by `tools/install-kicad-nightly.sh`; `EDA_KICAD_CLI` overrides the
  binary. A missing kicad-cli is an error, never a silent skip.
  - The project file written next to the derived files carries our design
    rules, the design's own ERC pin map and the `.kicad_dru` an import brought,
    and ignores the library-link checks (every footprint and symbol is
    embedded, so there is no library to compare it with).
  - Zones: kicad-cli judges the fills the exported board carries (our own
    filler's, the ones on screen). `--refill-zones` is opt-in (`eda board drc
    --refill-zones`, the DRC dialog's checkbox) because kicad-cli 10.99 skips
    its courtyard checks on a run that refills.
  - **The studio stays live while kicad-cli runs.** `/api/drc`, `/api/erc`,
    `/api/board_stats`, `/api/fab/*` and `/api/sch/plot|netlist` run on threads
    of their own (`crates/cli/src/kicad_lane.rs`), so edits, the `/api/version`
    poll and the 3D view never wait for one: an edit answers in milliseconds
    during a DRC. One kicad-cli at a time (they share `.kicad/`, and a DRC, an
    ERC and a STEP export together are not what 16 GB is for); a request for
    what is already running, on the same design revision, joins that run
    instead of starting another. A run that does not finish is killed (its
    whole process group) and answers with an error naming the command: 120 s
    for a DRC or ERC, 300 s for an export (`eda_kicad_engine::REPORT_TIMEOUT`
    and `EXPORT_TIMEOUT`; `EDA_KICAD_TIMEOUT_SECS` replaces both for a slow
    machine or a test), so a hung kicad-cli never holds the lane for good.
    Edits, undo/redo and every other route still
    run one at a time on the serve loop, as before. Every kicad-cli reply
    carries `revision`, the `/api/version` stamp of the design the run started
    from; the studio compares it with the board's current one and shows a
    report whose revision the board has left as out of date ("design changed
    since this check — rerun", dimmed markers) rather than as current. The
    canvas is never locked for a run.
- **Our own ports** are kept for what kicad-cli cannot do or cannot do fast
  enough interactively:
  - the interactive router (`crates/pns`, a port of KiCad's PNS),
  - the editor UI, tools, dialogs and hotkeys (KiCad look and feel),
  - live connectivity / ratsnest / zone fill while editing,
  - our own additions: intent, placement, schematic derivation.
  - The geometry core they share stays in `crates/drc` (shapes, collision,
    clearance and rule expressions, rtree, zone-fill driver, stroke font, and
    the board outline built from Edge.Cuts as KiCad builds it: `outline.rs`,
    which the filler, the router, the placement gates and the 3D body read; the
    malformed-outline finding it makes is the live view of what kicad-cli's
    `invalid_outline` reports on the export, and kicad-cli stays the judge);
    that crate is no longer a checker and keeps its name for now.
- **Never duplicate kicad-cli in Rust**, not even as a faster version
  (decided 2026-10-02, replacing "the Rust DRC/ERC ports stay as fast
  previews"). One answer per question, no parity work. A kicad-cli DRC run
  takes about 4 s on a 30-part board (ERC about 2.5 s, an export under 1.5 s),
  which is fine for the dialogs, the canvas markers and the judges; the studio
  shows a running state meanwhile.
  - **Cleanup done 2026-10-03.** Deleted: every DRC rule provider and the DRC
    types/runner, the KiCad ERC port (and the bus/hierarchy machinery only it
    and the exporters used), `crates/fab` (Gerber/drill/position/BOM/CPL), the
    schematic plotter and netlist exporter in `crates/kicad`, the Board
    Statistics port, connectivity's DRC wrappers, and every DRC/ERC/fab parity
    and perf test. Kept because a live feature needs them: the geometry core
    above (router, zone filler, renderer), connectivity (ratsnest, and the
    dangling-copper detection the Cleanup Tracks tool deletes), the zone fills
    the exporter writes, and the schematic-to-PCB sync.
  - **Our own checks** that KiCad does not have live in a separate fast
    checker, `crates/lint` (`eda-lint`): placement quality (proximity,
    decoupling, edge connectors, board use, net compactness, crossing stubs,
    refdes labels), net-class track width, schematic readability, fab
    readiness. It is never named DRC or ERC and never holds a copy of a KiCad
    rule. In-process and cheap: `GET /api/lint`, shown in the DRC and ERC
    dialogs' Lint tabs, `eda board lint`.
- **Gates have two tiers, by cost.** The in-process gates (`eda-gates`:
  `check_placement`, `check_routing`, the partial placer gate) are ours and
  run on every placer step and studio refresh; they call `eda-lint` and never
  spawn a process. The gates KiCad owns (courtyard overlap, copper-to-edge
  clearance) are `eda_gates::kicad`: one kicad-cli DRC run, filtered by type,
  used by the judges (`eda board check`, the CLI's `--strict`, the pipeline's
  place/route stages, `--fab`). The constructive placer cannot ask kicad-cli on
  every candidate, so it keeps pad copper out of the board-edge band and
  refuses repair moves that add overlaps by construction.
- **Strict has two levels.** The studio's Strict switch and every API path
  (drag, route, tune, cleanup) refuse a move that adds in-process gate
  failures: instant. The CLI's `--strict` also compares KiCad's gates on the
  board before and after through kicad-cli (seconds).
