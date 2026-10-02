// Hotkey smoke test in a real browser (same setup as collab.e2e.mjs): arrow cursor,
// Alt+1 grid, O select-unconnected, T get-and-move, Shift+P position relative,
// Escape closes dialogs, schematic tab + Shift+Space, no console errors; plus the
// RequestSelection / multi-select / one-undo-step / editor-scoped undo fixes
// (hover+R, multi R = one undo step, Ctrl+X keeps footprints, N wraps, schematic
// Ctrl+A / F1 / multi-R, Ctrl+Z in the Footprint editor leaves the PCB alone).
import { open } from "./browser.mjs";
import { execFileSync } from "node:child_process";
import { readdirSync } from "node:fs";
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

// ---- RequestSelection hover fallback + multi-select + one undo step (pcbnew edit_tool.cpp Rotate)
const state = async () => (await fetch(BASE + "/api/state")).json();
const rotOf = async (ref) => (await state()).parts.find((q) => q.ref === ref)?.rot;
const undoCount = () => { try { return readdirSync(DIR + "/.history/undo").length; } catch { return 0; } };
const center = async () => { const bx = await canvas.boundingBox(); return [bx.x + bx.width / 2, bx.y + bx.height / 2]; };
gui("--select", "R1"); await p.waitForTimeout(1500); await focusCanvas();
{
  const [cx, cy] = await center();
  const r0 = await rotOf("R1");
  await p.mouse.move(cx, cy); await p.waitForTimeout(250);
  await p.keyboard.press("r");
  const r1 = await waitFor(async () => { const v = await rotOf("R1"); return v !== r0 ? { v } : null; });
  check("hover + R (nothing selected) rotates the footprint under the cursor", r1 != null, `${r0} -> ${r1?.v}`);
  await p.keyboard.press("Control+z"); await waitFor(async () => (await rotOf("R1")) === r0);
}
{
  const [r0u, r0r] = [await rotOf("R6"), await rotOf("R7")];
  gui("--select", "R6,R7"); await p.waitForTimeout(1200); await p.mouse.move(800, 500);
  const u0 = undoCount();
  await p.keyboard.press("r");
  const both = await waitFor(async () => (await rotOf("R6")) !== r0u && (await rotOf("R7")) !== r0r);
  check("R with two footprints selected rotates both", !!both);
  check("multi-footprint rotate is ONE undo step", undoCount() - u0 === 1, `${undoCount() - u0} step(s)`);
  await p.keyboard.press("Control+z");
  const back = await waitFor(async () => (await rotOf("R6")) === r0u && (await rotOf("R7")) === r0r);
  check("one Ctrl+Z restores both footprints", !!back);
}
{
  gui("--select", "U1"); await p.waitForTimeout(1000); await p.mouse.move(800, 500);
  await p.keyboard.press("Control+x"); await p.waitForTimeout(700);
  const still = (await state()).parts.find((q) => q.ref === "U1")?.placed;
  check("Ctrl+X on a footprint does not delete it (nothing was copied)", still === true);
}
{
  // N / Shift+N wrap around the grid list instead of sticking at the ends
  await focusCanvas(); await p.mouse.move(800, 500);
  const seq = [];
  for (let i = 0; i < 40; i++) { await p.keyboard.press("n"); await p.waitForTimeout(60); seq.push(await grid()); }
  const L = new Set(seq).size;
  check("N never sticks at the last grid", seq.every((g, i) => i === 0 || g !== seq[i - 1]), seq.slice(0, 4).join(","));
  check("N wraps: the grid sequence repeats with the list length", L > 1 && seq.slice(0, 40 - L).every((g, i) => g === seq[i + L]), `${L} grids: ${seq.slice(0, 24).join(",")}`);
  await p.keyboard.press("Shift+n"); await p.waitForTimeout(150); const back = await grid();
  await p.keyboard.press("n"); await p.waitForTimeout(150);
  check("Shift+N steps back", (await grid()) !== back);
}
{
  // Ctrl+A on the PCB: every unlocked footprint
  await focusCanvas(); await p.mouse.move(800, 500); await p.keyboard.press("Control+a");
  const nSel = await waitFor(async () => { const v = gui(); return v.by === "ui" && v.selection.length > 3 ? v.selection.length : null; });
  check("Ctrl+A selects the footprints", !!nSel, `${nSel}`);
  await p.keyboard.press("Escape");
}
// Ctrl+Z in the Footprint editor must not undo the PCB (own undo scope)
{
  const r1start = await rotOf("R1");
  gui("--select", "R1"); await p.waitForTimeout(1000); await focusCanvas(); gui("--select", "R1"); await p.waitForTimeout(800);
  await p.keyboard.press("r");
  const rr = (await waitFor(async () => (await rotOf("R1")) !== r1start ? { v: await rotOf("R1") } : null))?.v;
  const u0 = undoCount();
  await p.getByText("Footprint", { exact: true }).first().click(); await p.waitForTimeout(800);
  await p.keyboard.press("Control+z"); await p.waitForTimeout(700);
  check("Ctrl+Z on the Footprint editor tab leaves the PCB edit alone", rr != null && (await rotOf("R1")) === rr && undoCount() === u0, `rot ${rr} -> ${await rotOf("R1")}`);
  await p.getByText("PCB", { exact: true }).first().click(); await p.waitForTimeout(800);
  await focusCanvas(); await p.keyboard.press("Control+z"); await waitFor(async () => (await rotOf("R1")) === r1start);
  check("Ctrl+Z back on the PCB tab undoes it", (await rotOf("R1")) === r1start);
}

// schematic tab: Shift+Space cycles line mode without errors
await p.getByText("Schematic", { exact: true }).first().click(); await p.waitForTimeout(1500);
const e0 = errors.length; await p.mouse.move(800, 500); await p.keyboard.press("Shift+Space"); await p.waitForTimeout(300);
check("Schematic tab loads and Shift+Space runs cleanly", errors.length === e0, errors.slice(e0).join(";"));

// schematic: Ctrl+A / F1 / multi-R work too (they used to be PCB-only)
{
  const sch = async () => (await fetch(BASE + "/api/schematic")).json();
  const rots = async () => (await sch()).symbols.map((x) => x.rot).join(",");
  const cv = p.locator(".pcb-canvas-container").first();
  await p.mouse.move(800, 500);
  const shot0 = await cv.screenshot(); await p.keyboard.press("F1"); await p.waitForTimeout(400); const shot1 = await cv.screenshot();
  check("F1 zooms in on the Schematic tab", !shot0.equals(shot1));
  await p.keyboard.press("Control+Home"); await p.waitForTimeout(400);
  check("Ctrl+Home zoom-to-fit works on the Schematic tab", !(await cv.screenshot()).equals(shot1));
  const r0 = await rots(); const u0 = undoCount();
  await p.keyboard.press("Control+a"); await p.waitForTimeout(500);
  const sel = gui().selection;
  check("Ctrl+A selects schematic items", gui().by === "ui" && sel.length > 1, `${sel.length}`);
  await p.keyboard.press("r");
  const r1 = await waitFor(async () => (await rots()) !== r0 ? await rots() : null);
  const n = (await sch()).symbols.length;
  check("R rotates ALL selected schematic symbols", !!r1 && r1.split(",").every((v, i) => v !== r0.split(",")[i]), `${n} symbols`);
  check("multi-symbol rotate is ONE undo step", undoCount() - u0 === 1, `${undoCount() - u0}`);
  await p.keyboard.press("Control+z"); await waitFor(async () => (await rots()) === r0);
  check("Ctrl+Z undoes the whole schematic rotate", (await rots()) === r0);
}

check("no console/page errors", errors.length === 0, errors.slice(0, 3).join("; "));
console.log(`\n${results.filter(Boolean).length}/${results.length} passed`);
await b.close();
process.exitCode = results.every(Boolean) ? 0 : 1;
