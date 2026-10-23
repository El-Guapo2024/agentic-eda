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
- **Local appearance settings**: `appearance.json` (`/api/appearance`) holds
  what the Appearance panel chose for this project -- layers and objects
  shown, opacities, net and net class colours, saved layer presets and
  viewports -- split as KiCad splits it: `local` is the `.kicad_prl`, `project`
  what KiCad keeps in the `.kicad_pro`. It is never part of the design, so it
  does not reload the scene or enter undo; the derived `.kicad/` project
  carries it (`<stem>.kicad_prl`, the project's net colours, presets and
  viewports, and a coloured net class's definition).
- **Library tables**: the libraries a `Lib:Name` is looked up in are the ones KiCad's library
  tables name (`crates/cli/src/library_tables.rs`, `crates/kicad/src/library_table.rs`). KiCad's
  global `sym-lib-table` / `fp-lib-table` (in its configuration folder, chaining its own 222 and
  155 libraries) are **read only** -- the studio never writes KiCad's configuration. The board's
  own `sym-lib-table` / `fp-lib-table` and `path-variables.json` (its variables, which are KiCad's
  project text variables) sit beside `design.json`; the Manage Libraries, New / Add Library and
  Configure Paths dialogs edit them (`/api/library_tables`, `/api/path_variables`). They are inputs,
  like the intent, not derived files, and are not part of the design or of undo; the derived
  `.kicad/` project carries them (`<board>/.kicad/sym-lib-table`, `fp-lib-table`, and the variables
  as `text_variables` with `KIPRJMOD` the board's folder), so kicad-cli reads the same libraries.
  `eda import-kicad` brings a real project's tables and text variables along into the board's folder.
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
  statistics, the Specctra DSN export (where the kicad-cli has `pcb export
  specctra`: see below), and reading other EDA formats (below). `crates/kicad-engine` exports the current `design.json` revision
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
  - **kicad-cli never reads the user's KiCad settings.** Every run goes through
    `eda_kicad_engine::cli_command`, which points `KICAD_CONFIG_HOME` at a
    settings folder of ours (KiCad writes its defaults there on first use), so
    a DRC, an ERC or an export answers the same on every machine whatever the
    user's library tables, path variables or colour themes say, and skips
    loading the global library tables (a 30-part DRC: 1.5 s instead of 6 s).
    A project that turns a library check back on gets a second folder with a
    fresh install's tables. `EDA_KICAD_CONFIG_HOME` moves the folders;
    `EDA_KICAD_USER_CONFIG=1` gives kicad-cli the user's settings back.
  - **Zone fill is KiCad's, through kicad-cli, and nobody else's** (decided
    2026-10-13: there is no zone filler in this workspace; `crates/zone-filler`
    is deleted). Fill All Zones (B, `POST /api/fill/kicad`, `eda board fill`)
    runs `kicad-cli pcb drc --refill-zones --save-board` on a COPY of the
    derived board (`.kicad/refill/`: the project's rules, none of the board's
    library tables, every DRC check ignored, so `cli_command` gives it the empty
    settings folder: a DRC job does not start before the global footprint
    libraries are enumerated, 5 s of a 7 s run, and a fill needs none of them;
    2 to 3 s on a 30-part board) and reads each zone's `(filled_polygon ..)` back
    (`eda_kicad::zone_fills`: nanometres, `(island yes)` kept). The fills are
    derived data beside `design.json`, never in it: `.kicad/zone_fills.json`,
    per zone with a fingerprint of the zone filled (outline, layers, net,
    settings) and the design revision. Between fills the board shows the last
    fill, as KiCad does with auto-refill off: an edit elsewhere leaves it, an
    edit to a zone empties that zone (a priority is none of them: the Zone
    Manager's OK changes priorities and puts the fills back), and a zone never
    filled is an outline (and no copper for live connectivity). The fills are
    read by `GET /api/fill` (canvas, 3D stand-in), the `.kicad_pcb` writer
    (`filled_polygon`, so the DRC, the 3D render, the plots and File > Save As
    see them), live connectivity and Cleanup Tracks
    (`eda_drc::zone_copper::ZoneCopper`). A board imported from a `.kicad_pcb`
    starts with the fills it was saved with. kicad-cli fills the board's zones
    only (`FillAllZones` walks `board()->Zones()`): a footprint's own zone is
    no pour to wait for.
    "Refill all zones before performing DRC" and the Plot dialog's "Check zone
    fills before plotting" fill first, on a copy, and then run the DRC or the
    export WITHOUT `--refill-zones` (kicad-cli 10.99 skips its courtyard checks
    on a run that refills). A zone keeps the uuid it had in the imported file
    (`Zone::uuid`): KiCad breaks ties between zones of equal priority by uuid.
  - **Other EDA formats come in through kicad-cli, and nobody else's importer**
    (P22, 2026-10-15: `kicad-cli pcb import` and `sch import`, new in the 10.99
    nightlies, run KiCad's own `PCB_IO` / `SCH_IO` plugins for Eagle, Altium,
    CADSTAR, PADS, P-CAD, Fabmaster, EasyEDA and the rest). `eda_kicad_engine::foreign`
    (`import_foreign_pcb`, `import_foreign_sch`) runs it through `cli_command` in a
    folder of the caller's (a run needs one of its own: kicad-cli writes the
    footprint library it made beside the output and trips over a second run's) and
    hands back the KiCad file's text, the other files it wrote (the `.pretty`
    folder, the `.kicad_sym`, the library tables), its JSON import report (format,
    counts, errors, warnings, where every source layer went) and the importers'
    own error and warning lines, which are the findings shown to the person. Our
    readers (`import_kicad_pcb`, `load_sheet_file`) turn the KiCad file into the
    design; nothing is converted in Rust. `--layer-map` takes the answers of
    `DIALOG_MAP_LAYERS` (a layer it does not name keeps the importer's guess).
    A board's design rules and net classes are in no board file: the importer
    sets them in memory and KiCad saves them with the project, which is what the
    project-level `kicad-cli import -o <stem> FILE` writes (a `.kicad_pro`; it
    pairs nothing and has no report), so the engine runs it a second time for a
    board (`ForeignOptions::project`) and the model is made with the same
    functions that read a project's own `.kicad_pro`.
    Entry points: `eda import-kicad <file> -o <dir>` (Eagle and CADSTAR pair a
    board and a schematic by name, an Altium `.PrjPcb` brings the documents it
    lists) and `POST /api/import_foreign`, then `Cmd::ReplaceBoard` /
    `Cmd::ReplaceSchematic` (File > Import > Non-KiCad Board / Schematic: one
    undo step that brings the old board back, the other's model a file under
    `imports/` that `DrawingsSection::intent_file` names). The file is an
    upload, never a path on the server.
  - **The studio stays live while kicad-cli runs.** `/api/drc`, `/api/erc`,
    `/api/board_stats`, `/api/fab/*`, `/api/sch/plot|netlist`, `/api/specctra/export` and `/api/import_foreign` run on threads
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
  - **Specctra DSN** (File > Export > Specctra DSN..., `eda board export
    specctra`, `crates/cli/src/specctra_api.rs`): kicad-cli's `pcb export
    specctra` when the kicad-cli found has it, which each process asks once
    (`eda_kicad_engine::cli_has_export`, from `pcb export --help`), so it is no
    longer an exception there. A kicad-cli without it (10.99.0-5334 and older)
    gets our own port of `FromBOARD` (`eda_kicad::specctra::export_dsn`), a
    fallback marked DEPRECATED, to be deleted once every machine has the
    command; the reply's `engine` says which wrote the file. The session
    import (SES, `Cmd::Specctra`) stays ours: kicad-cli has no command for it.
  - **IDFv3 and GenCAD** (File > Export > IDFv3... and GenCAD..., `/api/fab/idf`
    and `/api/fab/gencad`, `eda board export --kicad idf|gencad`): kicad-cli's
    `pcb export idf` and `gencad`, asked through the same probe. There is no
    fallback: where the kicad-cli lacks the command the menu entry is
    disabled with the reason (`GET /api/fab/exporters`).
- **Our own ports** are kept for what kicad-cli cannot do or cannot do fast
  enough interactively:
  - the interactive router (`crates/pns`, a port of KiCad's PNS),
  - the editor UI, tools, dialogs and hotkeys (KiCad look and feel),
  - live connectivity / ratsnest while editing (through pours as they were
    last filled by KiCad),
  - our own additions: intent, placement, schematic derivation.
  - The geometry core they share stays in `crates/drc` (shapes, collision,
    clearance and rule expressions, rtree, the copper of the zones as their
    fills hold it, stroke font, and
    the board outline built from Edge.Cuts as KiCad builds it: `outline.rs`,
    which the router, the placement gates and the 3D body read; the
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
    above (router, renderer), connectivity (ratsnest, and the
    dangling-copper detection the Cleanup Tracks tool deletes), the zone fills
    the exporter writes (KiCad's, kept beside the design), and the
    schematic-to-PCB sync.
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
