#!/usr/bin/env node
// Extracts PCB layer ids/names/descriptions and the GAL draw order into
// src/kicad/layers.json. Run from a terminal with git access to
// ~/ws/kicad-mirror:
//
//   node tools/extract-layers.js
//
// NOT YET VALIDATED against real KiCad source -- see the per-section
// comments below for what each parser assumes and how confident it is.
// `drawOrder` in particular is a low-confidence heuristic; check it by
// hand against pcb_draw_panel_gal.cpp before trusting the paint order.

import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";
import { readKicadFile, meta, writeJson } from "./lib/kicadSource.js";

const __dirname = dirname(fileURLToPath(import.meta.url));
const OUT = join(__dirname, "..", "src", "kicad", "layers.json");

const LAYER_IDS_FILE = "include/layer_ids.h";
const APPEARANCE_FILE = "pcbnew/widgets/appearance_controls.cpp";
const DRAW_PANEL_FILE = "pcbnew/pcb_draw_panel_gal.cpp";

/**
 * `enum PCB_LAYER_ID { A, B = X, C, ... }` -> [{id: "A", numericId: 0}, ...].
 * Handles plain auto-increment and `= <integer literal>`; an entry whose
 * value is a symbolic expression (`= LAST_ID + 1` etc.) gets numericId
 * null rather than a guess -- everything after it also becomes null,
 * since the running count is no longer known for certain.
 */
function parseLayerIds(fileText) {
  const enumMatch = /enum\s+PCB_LAYER_ID\s*(?::\s*\w+\s*)?\{([\s\S]*?)\}/.exec(fileText);
  if (!enumMatch) return [];
  const body = enumMatch[1];
  const layers = [];
  let next = 0;
  let known = true;
  for (const rawEntry of body.split(",")) {
    const entry = rawEntry.replace(/\/\/.*$/gm, "").replace(/\/\*[\s\S]*?\*\//g, "").trim();
    if (!entry) continue;
    const m = /^([A-Za-z_][A-Za-z0-9_]*)\s*(?:=\s*(.+))?$/.exec(entry);
    if (!m) continue;
    const [, id, valueExpr] = m;
    let numericId = null;
    if (valueExpr === undefined) {
      numericId = known ? next : null;
    } else if (/^-?\d+$/.test(valueExpr.trim())) {
      next = parseInt(valueExpr.trim(), 10);
      numericId = next;
      known = true;
    } else {
      known = false; // symbolic expression (e.g. "F_Cu" or "LAST + 1") -- stop counting
    }
    layers.push({ id, numericId, name: id.replace(/_/g, "."), description: "" });
    if (known) next += 1;
  }
  return layers;
}

/**
 * Best-effort: looks for `{ LAYER_ID, _("Some description") }`-shaped
 * pairs anywhere in appearance_controls.cpp and attaches them by id.
 * KiCad may structure this as a switch/case or a different map shape
 * instead -- if `matched` comes back 0, open the file and rewrite this.
 */
function attachDescriptions(layers, fileText) {
  const re = /\b([A-Za-z_][A-Za-z0-9_]*)\s*,\s*_\(\s*"((?:[^"\\]|\\.)*)"\s*\)/g;
  const byId = new Map(layers.map((l) => [l.id, l]));
  let matched = 0;
  let m;
  while ((m = re.exec(fileText))) {
    const layer = byId.get(m[1]);
    if (layer && !layer.description) {
      layer.description = m[2].replace(/\\"/g, '"');
      matched++;
    }
  }
  return matched;
}

/**
 * Heuristic: the first brace-initializer list in the file with at least
 * 10 comma-separated bare identifiers is guessed to be the GAL render
 * order. This is a guess, not a parse -- pcb_draw_panel_gal.cpp may
 * build the order procedurally instead of as one literal list, in which
 * case this finds nothing (empty result, not a wrong one) and the order
 * needs to be read out by hand.
 */
function guessDrawOrder(fileText) {
  const re = /\{([^{}]{60,}?)\}/g;
  let m;
  let best = null;
  while ((m = re.exec(fileText))) {
    const tokens = m[1]
      .split(",")
      .map((t) => t.replace(/\/\/.*$/gm, "").trim())
      .filter(Boolean);
    const identLike = tokens.filter((t) => /^[A-Za-z_][A-Za-z0-9_]*$/.test(t));
    if (identLike.length >= 10 && identLike.length === tokens.length) {
      if (!best || identLike.length > best.length) best = identLike;
    }
  }
  return best ?? [];
}

function main() {
  const idsText = readKicadFile(LAYER_IDS_FILE);
  const layers = parseLayerIds(idsText);

  const appearanceText = readKicadFile(APPEARANCE_FILE);
  const descCount = attachDescriptions(layers, appearanceText);

  const drawPanelText = readKicadFile(DRAW_PANEL_FILE);
  const drawOrder = guessDrawOrder(drawPanelText);

  const notes = [];
  if (layers.length === 0) notes.push("no PCB_LAYER_ID enum matched in " + LAYER_IDS_FILE);
  if (descCount === 0) notes.push("no layer descriptions matched in " + APPEARANCE_FILE + " -- appearance_controls.cpp likely structures these differently; open it and rewrite attachDescriptions()");
  if (drawOrder.length === 0) notes.push("no draw-order list found in " + DRAW_PANEL_FILE + " (heuristic search) -- read the render loop by hand");
  else notes.push("drawOrder is a heuristic guess (see script comment) -- confirm against " + DRAW_PANEL_FILE + " before trusting paint order");

  writeJson(OUT, {
    meta: meta([LAYER_IDS_FILE, APPEARANCE_FILE, DRAW_PANEL_FILE], notes.join("; ")),
    layers,
    drawOrder,
  });
  console.log(`${layers.length} layer(s), ${descCount} description(s), drawOrder length ${drawOrder.length}`);
}

main();
