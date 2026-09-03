#!/usr/bin/env node
// Differential validation: load a circuit.json emitted by our Rust IR
// exporter and validate every element against the official tscircuit
// `circuit-json` zod schema, then render both schematic and PCB SVGs
// with the official `circuit-to-svg` renderer.
//
// Usage: node validate.mjs <circuit.json> [--svg-out-dir DIR]
// Exit code is non-zero if any element fails schema validation or if
// either SVG render throws.

import fs from "node:fs";
import path from "node:path";
import { any_circuit_element } from "circuit-json";
import { convertCircuitJsonToSchematicSvg, convertCircuitJsonToPcbSvg } from "circuit-to-svg";

function main() {
  const args = process.argv.slice(2);
  const inputPath = args[0];
  if (!inputPath) {
    console.error("usage: node validate.mjs <circuit.json> [--svg-out-dir DIR]");
    process.exit(2);
  }
  let svgOutDir = path.dirname(inputPath);
  const svgFlagIdx = args.indexOf("--svg-out-dir");
  if (svgFlagIdx !== -1) svgOutDir = args[svgFlagIdx + 1];
  fs.mkdirSync(svgOutDir, { recursive: true });

  const raw = fs.readFileSync(inputPath, "utf8");
  const elements = JSON.parse(raw);
  if (!Array.isArray(elements)) {
    console.error(`error: ${inputPath} did not parse to a JSON array`);
    process.exit(2);
  }

  const counts = {};
  const errors = [];

  for (let i = 0; i < elements.length; i++) {
    const el = elements[i];
    const type = el && el.type ? el.type : "<missing type>";
    counts[type] = (counts[type] || 0) + 1;
    const result = any_circuit_element.safeParse(el);
    if (!result.success) {
      const id =
        el[`${type}_id`] ??
        el.id ??
        `<index ${i}>`;
      for (const issue of result.error.issues) {
        errors.push({
          element_type: type,
          element_id: id,
          index: i,
          path: issue.path.join("."),
          message: issue.message,
          code: issue.code,
        });
      }
    }
  }

  console.log(`file: ${inputPath}`);
  console.log(`elements: ${elements.length}`);
  console.log("element counts by type:");
  for (const t of Object.keys(counts).sort()) {
    console.log(`  ${t}: ${counts[t]}`);
  }
  console.log(`validation errors: ${errors.length}`);
  for (const e of errors) {
    console.log(
      `  [${e.element_type}#${e.element_id} idx=${e.index}] .${e.path}: ${e.message} (${e.code})`
    );
  }

  const base = path.basename(inputPath, path.extname(inputPath));
  const schematicSvgPath = path.join(svgOutDir, `${base}.schematic.svg`);
  const pcbSvgPath = path.join(svgOutDir, `${base}.pcb.svg`);
  let schematicOk = true;
  let pcbOk = true;
  let schematicErr = null;
  let pcbErr = null;

  try {
    const svg = convertCircuitJsonToSchematicSvg(elements);
    fs.writeFileSync(schematicSvgPath, svg);
  } catch (e) {
    schematicOk = false;
    schematicErr = e;
    console.log(`schematic SVG render FAILED: ${e.stack || e}`);
  }

  try {
    const svg = convertCircuitJsonToPcbSvg(elements);
    fs.writeFileSync(pcbSvgPath, svg);
  } catch (e) {
    pcbOk = false;
    pcbErr = e;
    console.log(`pcb SVG render FAILED: ${e.stack || e}`);
  }

  if (schematicOk) console.log(`schematic SVG: ${schematicSvgPath}`);
  if (pcbOk) console.log(`pcb SVG: ${pcbSvgPath}`);

  const result = {
    file: inputPath,
    elementCount: elements.length,
    counts,
    errorCount: errors.length,
    errors,
    schematicSvg: schematicOk ? schematicSvgPath : null,
    pcbSvg: pcbOk ? pcbSvgPath : null,
  };
  const resultPath = path.join(
    svgOutDir,
    `${base}.validation.json`
  );
  fs.writeFileSync(resultPath, JSON.stringify(result, null, 2));

  if (errors.length > 0 || !schematicOk || !pcbOk) {
    process.exitCode = 1;
  }
}

main();
