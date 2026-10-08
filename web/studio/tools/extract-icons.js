#!/usr/bin/env node
// Copies the SVG icons that the extracted actions/toolbars reference
// into web/studio/public/icons/{light,dark}/, and writes
// web/studio/public/icons/LICENSE. Run from a terminal with git access
// to ~/ws/kicad-mirror, AFTER extract-actions.js and extract-toolbars.js
// have populated src/kicad/actions.json and src/kicad/toolbars.json (this
// script reads the icon names out of those, so it only copies icons this
// app can actually reference):
//
//   node tools/extract-actions.js && node tools/extract-toolbars.js && node tools/extract-icons.js
//
// This uses `git archive` (a purely local copy out of ~/ws/kicad-mirror,
// no network involved), exactly as instructed -- unlike the other
// extract-*.js scripts, it doesn't parse arbitrary C++ into structured
// data, so it isn't a "confidence" concern the way those are. Its one
// real assumption: BITMAPS::<enum_member> corresponds to a source file
// named "<enum_member>.svg" (KiCad's bitmap names and their SVG sources
// have matched 1:1 like this for a long time). include/bitmaps/bitmaps_list.h
// is scanned for an explicit override in case some entries don't follow
// that convention; unmatched icons are reported and skipped, not guessed.

import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";
import { readFileSync, existsSync, mkdirSync, copyFileSync, rmSync, writeFileSync } from "node:fs";
import { readKicadFile, listKicadDir, extractPaths, resolveCommit, meta, writeJson } from "./lib/kicadSource.js";

const __dirname = dirname(fileURLToPath(import.meta.url));
const ROOT = join(__dirname, "..");
const ACTIONS_JSON = join(ROOT, "src", "kicad", "actions.json");
const TOOLBARS_JSON = join(ROOT, "src", "kicad", "toolbars.json");
// The other editors' toolbars (extract-sch-toolbars.js, extract-editor-toolbars.js): their groups name an icon too, and the 3D viewer's own actions
// (which actions.json does not hold) carry theirs next to the toolbar.
const OTHER_TOOLBARS_JSON = ["sch_toolbars.json", "fp_toolbars.json", "sym_toolbars.json", "viewer3d_toolbars.json"].map((f) => join(ROOT, "src", "kicad", f));
const ICONS_JSON = join(ROOT, "src", "kicad", "icons.json");
const ICONS_ROOT = join(ROOT, "public", "icons");
const BITMAPS_LIST_FILE = "include/bitmaps/bitmaps_list.h";
const SOURCE_DIRS = { light: "resources/bitmaps_png/sources/light", dark: "resources/bitmaps_png/sources/dark" };

function readJsonIfPresent(path) {
  if (!existsSync(path)) return null;
  return JSON.parse(readFileSync(path, "utf8"));
}

/** BITMAPS enum member -> requested icon names, gathered from every place actions.json/toolbars.json can name one. */
function collectWantedIcons() {
  const wanted = new Set();
  const actionsFile = readJsonIfPresent(ACTIONS_JSON);
  // INVALID_BITMAP is KiCad's "no icon" sentinel, not an icon.
  for (const a of actionsFile?.actions ?? []) if (a.icon && a.icon !== "INVALID_BITMAP") wanted.add(a.icon);
  for (const path of [TOOLBARS_JSON, ...OTHER_TOOLBARS_JSON]) {
    const toolbarsFile = readJsonIfPresent(path);
    for (const t of toolbarsFile?.toolbars ?? []) for (const item of t.items) if (item.type === "group" && item.icon) wanted.add(item.icon);
    for (const a of [...(toolbarsFile?.actions ?? []), ...(toolbarsFile?.otherActions ?? [])]) if (a.icon) wanted.add(a.icon);
  }
  return wanted;
}

/** Explicit BITMAPS-member -> filename overrides, if bitmaps_list.h spells any out directly (best-effort regex; see header comment -- the 1:1 naming convention is the primary path, this only catches exceptions to it). */
function readExplicitOverrides() {
  const overrides = new Map();
  let text;
  try {
    text = readKicadFile(BITMAPS_LIST_FILE);
  } catch {
    return overrides;
  }
  const re = /(\w+)\s*,\s*"([\w.\-/]+)"/g;
  let m;
  while ((m = re.exec(text))) overrides.set(m[1], m[2].replace(/\.(png|svg)$/, ""));
  return overrides;
}

function main() {
  const wanted = collectWantedIcons();
  if (wanted.size === 0) {
    console.warn("no icons referenced by src/kicad/actions.json / toolbars.json yet -- run extract-actions.js and extract-toolbars.js first. Nothing to do.");
    writeJson(ICONS_JSON, { meta: meta([BITMAPS_LIST_FILE], "nothing to extract: actions.json/toolbars.json reference no icons yet"), icons: {} });
    return;
  }

  const overrides = readExplicitOverrides();
  const available = { light: new Set(listKicadDir(SOURCE_DIRS.light)), dark: new Set(listKicadDir(SOURCE_DIRS.dark)) };

  const resolved = new Map(); // enum member -> filename (e.g. "add_tracks.svg")
  const missing = [];
  for (const enumMember of wanted) {
    const base = overrides.get(enumMember) ?? enumMember;
    const filename = `${base}.svg`;
    if (available.light.has(filename) || available.dark.has(filename)) {
      resolved.set(enumMember, filename);
    } else {
      missing.push(enumMember);
    }
  }

  const tmp = join(ROOT, ".icon-extract-tmp");
  rmSync(tmp, { recursive: true, force: true });
  mkdirSync(join(ICONS_ROOT, "light"), { recursive: true });
  mkdirSync(join(ICONS_ROOT, "dark"), { recursive: true });

  let copied = 0;
  for (const theme of ["light", "dark"]) {
    const paths = [...resolved.values()].map((f) => `${SOURCE_DIRS[theme]}/${f}`).filter((p) => available[theme].has(p.split("/").pop()));
    if (paths.length === 0) continue;
    extractPaths(paths, tmp);
    for (const f of resolved.values()) {
      const src = join(tmp, SOURCE_DIRS[theme], f);
      if (existsSync(src)) {
        copyFileSync(src, join(ICONS_ROOT, theme, f));
        copied++;
      }
    }
  }
  rmSync(tmp, { recursive: true, force: true });

  const { sha, date } = resolveCommit();
  writeFileSync(
    join(ICONS_ROOT, "LICENSE"),
    `KiCad icons\n` +
      `Source: https://gitlab.com/kicad/code/kicad (resources/bitmaps_png/sources/{light,dark})\n` +
      `Commit: ${sha} (${date})\n` +
      `License: CC-BY-SA 4.0 -- https://creativecommons.org/licenses/by-sa/4.0/\n`
  );

  const icons = Object.fromEntries(resolved);
  const notes = [];
  if (missing.length > 0) notes.push(`${missing.length} icon(s) not found under either theme dir (checked "<name>.svg" plus bitmaps_list.h overrides): ${missing.slice(0, 20).join(", ")}`);
  writeJson(ICONS_JSON, { meta: meta([BITMAPS_LIST_FILE, SOURCE_DIRS.light, SOURCE_DIRS.dark], notes.join("; ") || undefined), icons });
  console.log(`copied ${copied} file(s) across light+dark for ${resolved.size}/${wanted.size} requested icon(s); ${missing.length} missing`);
}

main();
