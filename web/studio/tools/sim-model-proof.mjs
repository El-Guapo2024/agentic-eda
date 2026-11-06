#!/usr/bin/env node
// The slow proof's bridge to the studio's Simulation Model dialog (src/kicad-port/sim/simModelScript.ts): reads a request (JSON, the file named
// first) and prints the answer (JSON). Run with Node's TypeScript support:
//
//   node --experimental-transform-types --no-warnings --import ./tools/lib/ts-register.mjs tools/sim-model-proof.mjs request.json
//
// { "dialog": [DialogScript, ..] }                      -> { "dialog": [DialogResult, ..] }   (what OK writes, the Code tab, the verbs)
// { "netlist": [{ fields, pins, item }, ..] }           -> { "netlist": [{ modelLine, itemLine }, ..] }   (KiCad's generator on those fields)
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { decodeTables } from "../src/kicad-port/sim/simModelBuiltin.ts";
import { netlistLines, runDialog } from "../src/kicad-port/sim/simModelScript.ts";

const here = dirname(fileURLToPath(import.meta.url));
const tables = decodeTables(JSON.parse(readFileSync(join(here, "..", "src", "kicad", "sim_models.json"), "utf8")));
const req = JSON.parse(readFileSync(process.argv[2], "utf8"));
const out = {};
if (req.dialog) out.dialog = req.dialog.map((s) => runDialog(tables, s));
if (req.netlist) out.netlist = req.netlist.map((n) => netlistLines(tables, n.fields, n.pins, n.item));
process.stdout.write(JSON.stringify(out));
