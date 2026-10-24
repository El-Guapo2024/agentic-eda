# Rules for every port agent in the eda lane (agentic-eda, on ws-agentic-eda)

You are a port agent on the agentic-eda repo, a Rust + TypeScript port of KiCad's editors (the web studio is `web/studio`). The lead (another Claude session) reviews, gates and merges your branch. Several agents work on this box at the same time, each in its own git worktree: stay in yours, never touch `/home/team_braneboard_com/work/agentic-eda` (main's checkout) or another worktree, never push or merge `main`.

Read first in your worktree: `docs/HANDOFF.md` ("How the work was run", "Known pitfalls", "Gates"), `docs/ARCHITECTURE.md`, `docs/parity/GAPS.md` for your package's item.

## Parity: port from the real KiCad code
- KiCad's source is `$KICAD_SRC_DIR` (`/home/team_braneboard_com/src/kicad-src-8303b2ad`, commit 8303b2ad). Port from it, never from memory. Read the C++ of every function you port, cite it (`file.cpp::Function`) in the doc comment, and keep KiCad's behaviour, defaults, messages (word for word) and edge cases. Where you must differ, say so in the doc comment and in the PARITY section.
- The rules that do not change: `design.json` plus the intent is the only source of truth (KiCad files are derived); kicad-cli is the only DRC, ERC, plot, export and zone-fill engine (exceptions only as the handoff names them); every edit is one undoable `/api/cmd` verb.

## Code
- Match the surrounding code: its naming, its long doc comments citing KiCad, its idioms.
- IR additions are overlays with serde defaults (a new required field breaks other branches' literals).
- No new crates.io or npm dependency (ask the lead; the lead asks the user).
- Never edit `work/` or `examples/`; build scratch boards from `examples/*.yaml` with `target/debug/eda board new <yaml> -o <tmpdir>`.
- Hub files get small, additive edits only: `crates/ops/src/lib.rs`, `ir.rs`, `studio.rs`, `board.rs`, `useActionRunner.ts`, `store.tsx`, `painter.ts`, `types.ts`.
- `web/studio/node_modules` is a symlink to main's checkout: never commit it; `git add` files by explicit path.
- starlark turns on serde_json's `arbitrary_precision`: read tagged enums that hold numbers field by field.
- A wired action must be counted by `web/studio/tools/ui-parity-audit.mjs`; remove its row from `web/studio/tools/ui-parity-missing.json`. Do not commit `docs/parity/UI-ACTIONS.md` (the lead regenerates it on merge).
- Docs: your PARITY-*.md section (append after the last section number on main; the lead renumbers on clash), your GAPS.md item, your HANDOFF "Open branches" row.

## Build and gates (report the summary lines verbatim)
- Use `CARGO_BUILD_JOBS=4 CARGO_INCREMENTAL=0`; the box is shared.
- Scratch files (boards, logs, PID files) go in a directory of your own, `mktemp -d -p /tmp <branch>.XXXX`. The session scratchpad is shared with other agents, so never write a fixed name like `serve.pid` there, and only kill a PID you started yourself.
1. `cargo check --workspace --tests --keep-going`: zero warnings.
2. `tools/check.sh fast`: 0 failed (main's counts are in your worktree's HANDOFF "Gates").
3. Slow kicad-cli proofs of your change: `EDA_SLOW_TESTS=1 cargo test -p <crate> --test <name>`. kicad-cli is nightly 10.99 at `/usr/local/bin/kicad-cli`.
4. If readers/writers changed: `TMPDIR=$(mktemp -d) KICAD_QA_DATA=$KICAD_SRC_DIR/qa/data EDA_SLOW_TESTS=1 EDA_PARITY_PARTS=reexport python3 tools/parity_report.py` stays 12/12 (revert the parity outputs it rewrites unless asked).
5. Browser check `web/studio/e2e/<name>.check.js` (model it on the others), run headless: `(cd web/studio && npm run build)`, `cargo build -p eda-cli`, serve a scratch board in the background (`target/debug/eda board serve -C <dir> --port <your port> --ui web/studio/dist &`), then `PLAYWRIGHT_MODULE=$HOME/tools/pw/node_modules/playwright/index.mjs node web/studio/e2e/run-check.mjs http://localhost:<port>/ web/studio/e2e/<name>.check.js [minutes]`. Kill your server by PID when done (not `pkill -f` with a pattern that matches your own shell).

## Commits
Logical steps on your branch, messages in the repo's style (`git log`), each ending with:

    Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
    Claude-Session: https://claude.ai/code/session_018eAWc4qjKVQDxtCKwM4JXN

Push your branch to origin. Final report (short): what is done, commits, gate results with numbers, what is unverified or left, and decisions the lead should check.
