# Findings-only review (eda lane, while `task-nanometres` changes the IR's unit)

You are a reviewer on the agentic-eda repo, a Rust + TypeScript port of KiCad's editors. Another agent is changing the design file's lengths from micrometres to nanometres across the whole codebase right now, on `task-nanometres` in `~/work/agentic-eda-specctra`. So **you change no code anywhere**:
- no edits, commits or branches in any worktree, nor in `~/work/agentic-eda` (main's checkout);
- read the code from main's checkout: `/home/team_braneboard_com/work/agentic-eda`;
- write only your findings file and scratch files in a directory of your own (`mktemp -d -p /tmp findings-<name>.XXXX`).

## What to produce
One file, `docs/parity/findings/ (on the box: ~/work/findings/)<name>.md`, that a fixer can act on later without redoing your reading. For each finding:
- **What differs.** Our behaviour vs KiCad's, with `file:line` on our side and `kicad_file.cpp::Function` on KiCad's. Quote the relevant lines of KiCad's code, and its messages word for word.
- **How to see it.** A concrete scenario, and if you can, the failing test you would write: its name, its input and the assertion.
- **Severity.**
  - high: wrong result, data loss, wrong connectivity, or a file KiCad reads differently;
  - medium: a wrong default, message, undo step or edge case;
  - low: cosmetic.
- **Unit-sensitive or not.** Would the µm → nm change fix it or touch it?

Also list what you checked and found to match, so it is not reviewed again. End with a short summary table.

## Parity is against the real code
KiCad's source is `$KICAD_SRC_DIR` (`/home/team_braneboard_com/src/kicad-src-8303b2ad`). Items newer than that are in `~/src/kicad-src-25ec2335`. Read the C++ of every behaviour you judge, never from memory. Go function by function over everything the package's PARITY rows mark "ported" or "identical", most-used behaviour first. A claim in our doc comments is a claim to check, not evidence.

## Practical checks (allowed, read-only)
You may run main's built binaries and kicad-cli on scratch copies to confirm a finding:
- `/home/team_braneboard_com/work/agentic-eda/target/debug/eda`: `eda board new examples/<x>.yaml -o <your tmp>`, `eda board ...` on that copy;
- kicad-cli: `kicad-cli pcb drc`, `sch erc`, `upgrade` on scratch copies;
- KiCad's QA boards: `$KICAD_SRC_DIR/qa/data`, copied to your tmp first. Never write into the KiCad source.

Don't run `cargo build` or `cargo test` in any worktree; read the tests instead. Don't start a studio server on 8766, and use only ports 8820-8829 if you need one at all. Kill only processes you started. Keep the box usable: `nice` anything heavy.

## Report
Your final message: the path of your findings file, the counts by severity, and the 3 findings that matter most.
