// Hotkey smoke test in a real browser (same setup as collab.e2e.mjs): arrow cursor,
// Alt+1 grid, O select-unconnected, T get-and-move, Shift+P position relative,
// Escape closes dialogs, schematic tab + Shift+Space, no console errors.
import { open } from "./browser.mjs";
import { execFileSync } from "node:child_process";
const BASE = process.env.STUDIO_URL || "http://localhost:8811", DIR = process.env.BOARD, EDA = process.env.EDA_BIN || "eda";
const gui = (...a) => JSON.parse(execFileSync(EDA, ["board", "gui", ...a, "-C", DIR], { env: { ...process.env, EDA_ACTOR: "agent" } }).toString());
const results = [];
const check = (name, ok, detail = "") => { results.push(ok); console.log((ok ? "PASS " : "FAIL ") + name + (detail ? "  -- " + detail : "")); };
const waitFor = async (fn, ms = 4000) => { const t = Date.now(); while (Date.now() - t < ms) { try { const v = await fn(); if (v) return v; } catch {} await new Promise((r) => setTimeout(r, 150)); } return null; };
const { b, p, errors, shot } = await open();
const text = () => p.evaluate(() => document.body.innerText);
await p.goto(BASE + "/", { waitUntil: "load" });
await p.waitForTimeout(1500);
const canvas = p.locator(".pcb-canvas-container").first();
const focusCanvas = async () => { await p.mouse.move(800, 500); await canvas.click({ position: { x: 5, y: 5 } }).catch(() => {}); await p.keyboard.press("Escape"); };
await focusCanvas();

// common: arrow keys move the cursor (status bar coordinates)
const coords = async () => (await text()).match(/Z [0-9.]+\s+([-0-9.]+), ([-0-9.]+)/)?.slice(1).join(",");
await p.mouse.move(800, 500); await p.waitForTimeout(200);
const c0 = await coords(); await p.keyboard.press("ArrowRight"); await p.waitForTimeout(300); const c1 = await coords();
check("ArrowRight moves the cursor one grid step", !!c0 && !!c1 && c0 !== c1, `${c0} -> ${c1}`);

// common: Alt+1 switches to fast grid 1 (status bar grid)
const grid = async () => (await text()).match(/grid ([0-9.]+ mm)/)?.[1];
const g0 = await grid(); await p.keyboard.press("Alt+1"); await p.waitForTimeout(300); const g1 = await grid();
check("Alt+1 switches the grid", !!g1 && g0 !== g1, `${g0} -> ${g1}`);

// pcbnew: O adds the footprints at the far end of the selection's ratsnest
// (PCB_SELECTION_TOOL::selectUnconnected works from the current selection)
await p.keyboard.press("Escape"); await p.waitForTimeout(600); gui("--select", "U1"); await p.waitForTimeout(1500);
await p.mouse.move(800, 500); await p.keyboard.press("o");
const sel = await waitFor(async () => { const v = gui(); return v.by === "ui" && v.selection.length > 1 ? v.selection : null; });
check("O (with U1 selected) adds U1's unconnected neighbours, keeping U1", !!sel && sel.includes("U1"), sel ? `${sel.length} selected: ${sel.slice(0, 8).join(",")}` : "");
await p.keyboard.press("Escape");

// pcbnew: T opens get-and-move footprint
await focusCanvas(); await p.keyboard.press("t");
const tDlg = await waitFor(async () => /get and move|footprint/i.test((await p.locator(".dialog").first().innerText().catch(() => "")) || ""));
check("T opens the Get and Move Footprint dialog", !!tDlg, (await p.locator(".dialog-header").first().innerText().catch(() => "none")));
await p.keyboard.press("Escape");

// pcbnew: Shift+P position relative (needs a selection)
gui("--select", "C1"); await p.waitForTimeout(1000); await focusCanvas(); gui("--select", "C1"); await p.waitForTimeout(800);
await p.keyboard.press("Shift+P");
const pDlg = await waitFor(async () => /position relative/i.test(await p.locator(".dialog-header").first().innerText().catch(() => "")));
check("Shift+P opens Position Relative To", !!pDlg, (await p.locator(".dialog-header").first().innerText().catch(() => "none")));

await p.keyboard.press("Escape");
await p.waitForTimeout(300);
const stillOpen = await p.locator(".dialog-header").count();
check("Escape closes the Position Relative dialog", stillOpen === 0, `${stillOpen} dialog(s) still open`);
if (stillOpen) { await p.getByRole("button", { name: /cancel|close/i }).first().click().catch(() => {}); }

// schematic tab: Shift+Space cycles line mode without errors
await p.getByText("Schematic", { exact: true }).first().click(); await p.waitForTimeout(1500);
const e0 = errors.length; await p.mouse.move(800, 500); await p.keyboard.press("Shift+Space"); await p.waitForTimeout(300);
check("Schematic tab loads and Shift+Space runs cleanly", errors.length === e0, errors.slice(e0).join(";"));

check("no console/page errors", errors.length === 0, errors.slice(0, 3).join("; "));
console.log(`\n${results.filter(Boolean).length}/${results.length} passed`);
await b.close();
process.exitCode = results.every(Boolean) ? 0 : 1;
