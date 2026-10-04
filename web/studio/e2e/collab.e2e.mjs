// End-to-end check of the AI + human co-editing model (docs/ARCHITECTURE.md)
// in a real browser: agent drives the view (eda board gui), UI pushes its
// view back, hotkeys go through undoable /api/cmd verbs, agent edits show up
// live, a hand edit of design.json becomes a 'file' history step, and the
// DRC dialog runs kicad-cli (the only DRC engine; opening the dialog runs it
// and shows a running state) next to a Lint tab of our own checks. Usage:
//   eda board serve -C <board> --port 8811 --ui web/studio/dist &
//   BOARD=<board> EDA_BIN=<path to eda> node web/studio/e2e/collab.e2e.mjs
// The board must have placed parts U1 and R1. Modifies the board.
import { open } from "./browser.mjs";
import { execFileSync } from "node:child_process";
import { readFileSync, writeFileSync } from "node:fs";
const BASE = process.env.STUDIO_URL || "http://localhost:8811";
const DIR = process.env.BOARD;
const EDA = process.env.EDA_BIN || "eda";
const agent = (...args) => execFileSync(EDA, ["board", ...args, "-C", DIR], { env: { ...process.env, EDA_ACTOR: "agent" } }).toString();
const api = async (path) => (await fetch(BASE + path)).json();
const results = [];
const check = (name, ok, detail = "") => { results.push({ name, ok, detail }); console.log((ok ? "PASS " : "FAIL ") + name + (detail ? "  -- " + detail : "")); };
const waitFor = async (fn, ms = 6000) => { const t = Date.now(); while (Date.now() - t < ms) { try { const v = await fn(); if (v) return v; } catch {} await new Promise((r) => setTimeout(r, 200)); } return null; };

const { b, p, errors, shot } = await open();
await p.goto(BASE + "/", { waitUntil: "load" });
await p.waitForTimeout(1500);
check("loads without console errors", errors.length === 0, errors.join("; "));

// 1. agent drives the view: select + zoom to U1
agent("gui", "--select", "U1", "--zoom-to", "U1");
const selShown = await waitFor(async () => (await p.evaluate(() => document.body.innerText)).match(/Properties[\s\S]{0,200}Reference\s+U1/i));
check("agent selection appears in the UI (eda board gui --select U1)", !!selShown);
await shot(process.env.SHOTS ? process.env.SHOTS + "/1-agent-select.png" : "/dev/null");

// 2. UI selection flows back to the agent
const v = await waitFor(async () => { const x = JSON.parse(agent("gui")); return x.by === "ui" && x.selection.includes("U1") ? x : null; });
check("UI pushes its view back (by=ui, selection U1)", !!v, v ? `rev ${v.rev}` : "");

// 3. hotkey L = toggleLock on the selection, with the canvas focused
await p.mouse.move(800, 500);
await p.locator(".pcb-canvas-container").first().focus().catch(() => {});
await p.keyboard.press("l");
const locked = await waitFor(async () => { const s = await api("/api/state"); return (s.locked || []).includes("U1") ? s.locked : null; });
check("L locks the selected footprint (set_locked via /api/cmd)", !!locked, JSON.stringify(locked));

// 4. Ctrl+Z undoes it
await p.keyboard.press("Control+z");
const unlocked = await waitFor(async () => { const s = await api("/api/state"); return !(s.locked || []).includes("U1"); });
check("Ctrl+Z undoes the lock", !!unlocked);

// 5. agent edits via a command while the UI is open -> UI shows the new position
agent("move", "R1", "--to", "60,40");
agent("gui", "--select", "R1");
// Wait for the moved position itself: the selection can land a poll before
// the reloaded design does, briefly showing R1 where it was.
let pos = null;
await waitFor(async () => {
  pos = (await p.evaluate(() => document.body.innerText)).match(/Reference\s+R1[\s\S]{0,200}?Position\s+([0-9.]+), ([0-9.]+) mm/i);
  return pos && pos[1] === "60.000" && pos[2] === "40.000";
}, 8000);
check("agent move shows up live in the UI (Properties: R1 at 60, 40 mm)", !!pos && pos[1] === "60.000" && pos[2] === "40.000", pos ? pos[1] + ", " + pos[2] : "not shown: " + ((await p.evaluate(() => document.body.innerText)).match(/PROPERTIES[\s\S]{0,160}/i)?.[0] ?? "").replace(/\n/g, " | "));
await shot(process.env.SHOTS ? process.env.SHOTS + "/5-agent-move.png" : "/dev/null");

// 6. hand edit of design.json -> recorded as a 'file' history step
const dj = DIR + "/design.json";
writeFileSync(dj, readFileSync(dj, "utf8").replace('"schema": 1', '"schema": 1 '));
const fileStep = await waitFor(async () => { const s = await api("/api/state"); return JSON.stringify(s.activity || []).includes('"file"'); });
check("hand edit of design.json becomes a 'file' history step", !!fileStep);
const acts = (await api("/api/state")).activity || [];
const bys = [...new Set(acts.map((a) => a.by))];
check("shared history has ui + agent + file authors", ["ui", "agent", "file"].every((x) => bys.includes(x)), bys.join(","));

// 7. DRC dialog: kicad-cli is the only engine. Opening the dialog on a board it has
// not judged yet runs it (a few seconds, with a visible running state), there is no
// engine switch, and our own checks have a Lint tab of their own.
await p.getByText("Inspect", { exact: true }).first().click();
await p.getByText(/Design Rules Checker/).first().click();
const sawRunning = await waitFor(async () => /kicad-cli is checking the board/.test(await p.evaluate(() => document.body.innerText)), 3000);
const engine = await waitFor(async () => (await p.evaluate(() => document.body.innerText)).match(/kicad-cli [0-9.]+/)?.[0], 40000);
const uncon = await p.evaluate(() => document.body.innerText.match(/Unconnected Items \((\d+)\)/)?.[1]);
check("DRC dialog runs kicad-cli on open and shows a running state", !!engine && !!sawRunning, `${engine}, running state seen: ${!!sawRunning}, unconnected=${uncon}`);
check("DRC dialog has no engine switch", (await p.locator("select").filter({ hasText: "KiCad" }).count()) === 0);
const lintCount = await p.evaluate(() => document.body.innerText.match(/Lint \((\d+)\)/)?.[1]);
check("DRC dialog has a Lint tab for our own checks", lintCount !== undefined, `lint findings: ${lintCount}`);
await shot(process.env.SHOTS ? process.env.SHOTS + "/7-drc-kicad.png" : "/dev/null");

// 8. no errors at the end
check("no console/page errors during the run", errors.length === 0, errors.slice(0, 3).join("; "));
console.log(`\n${results.filter((r) => r.ok).length}/${results.length} passed`);
process.exitCode = results.every((r) => r.ok) ? 0 : 1;
await b.close();
