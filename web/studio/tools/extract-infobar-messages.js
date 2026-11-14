#!/usr/bin/env node
// Extracts what KiCad's board editor says in its frame's info bar (`EDA_BASE_FRAME::ShowInfoBarError` / `ShowInfoBarWarning` / `ShowInfoBarMsg`, and
// `GetInfoBar()->ShowMessageFor( msg, ms, icon )`) into src/kicad/pcb_infobar_messages.json: each message as a pattern (a literal, or the `Printf` /
// `wxString::Format` format a `msg` variable got, its `%s` / `%d` / `%zu` .. as wildcards), the icon (error, warning, info), the time it shows (`ShowInfoBarError`
// 8 s, `ShowInfoBarWarning` 6 s, `ShowInfoBarMsg` 8 s, `ShowMessageFor`'s own) and whether it has a close button (the call's `true`). The studio's board editor
// shows a message that matches one of them in its info bar over the canvas (kicad-port/pcbInfoBar.ts), not as a toast (UI-LEFT-3 P3-32 / C3 follow-up).
// Read: pcbnew/ (but its dialogs, whose info bars are their own) and pcbnew/tools, pcbnew/generators, pcbnew/microwave, pcbnew/router.
//
//   KICAD_SRC_DIR=$HOME/src/kicad-src-8303b2ad node tools/extract-infobar-messages.js
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";
import { readKicadFile, meta, writeJson, discoverFilesContaining } from "./lib/kicadSource.js";

const __dirname = dirname(fileURLToPath(import.meta.url));
const OUT = join(__dirname, "..", "src", "kicad", "pcb_infobar_messages.json");

/** The adjacent string literals of a `_( "a" "b" )`, joined, their escapes undone. */
const literal = (text) => [...text.matchAll(/"((?:[^"\\]|\\.)*)"/g)].map((m) => m[1].replace(/\\(.)/g, (_, c) => (c === "n" ? "\n" : c))).join("");

/** A printf format as a regular expression's source: the conversions as wildcards, the rest escaped. */
function formatPattern(fmt) {
  let out = "";
  for (let i = 0; i < fmt.length; i++) {
    const c = fmt[i];
    if (c === "%" && fmt[i + 1] === "%") {
      out += "%";
      i++;
    } else if (c === "%") {
      const m = /^%[-+ #0]*\d*(?:\.\d+)?(?:hh|h|ll|l|z|L)?([sdifuxgc])/.exec(fmt.slice(i));
      if (!m) {
        out += "%";
        continue;
      }
      out += m[1] === "s" || m[1] === "c" ? "[\\s\\S]*?" : "-?[\\d.]+";
      i += m[0].length - 1;
    } else out += c.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  }
  return out;
}

function main() {
  const files = discoverFilesContaining("InfoBar", ["pcbnew"]).filter((f) => !f.startsWith("pcbnew/dialogs/") && f.endsWith(".cpp"));
  const messages = new Map();
  for (const f of files) {
    const text = readKicadFile(f);
    const re = /(ShowInfoBar(Error|Warning|Msg)|(?:GetInfoBar\(\)|infobar)\s*->\s*ShowMessageFor)\s*\(\s*([\s\S]*?)\)\s*;/g;
    let m;
    while ((m = re.exec(text))) {
      const kindName = m[2] ?? "For";
      const args = m[3];
      // the message: a literal, or a variable whose `Printf` / `Format` / `= _( .. )` came before
      let pattern = null;
      const lit = /^_\(\s*((?:"(?:[^"\\]|\\.)*"\s*)+)\)/.exec(args.trim());
      if (lit) pattern = formatPattern(literal(lit[1]).replace(/%/g, "%%"));
      else {
        const v = /^(\*?)(\w+)/.exec(args.trim());
        if (v) {
          const before = text.slice(Math.max(0, m.index - 3000), m.index);
          const name = v[2];
          const fm = [...before.matchAll(new RegExp(`${name}\\s*(?:\\.Printf\\(|=\\s*wxString::Format\\(|=\\s*)\\s*_\\(\\s*((?:"(?:[^"\\\\]|\\\\.)*"\\s*)+)\\)`, "g"))].pop();
          if (fm) pattern = formatPattern(literal(fm[1]));
        }
      }
      if (!pattern) continue;
      let kind = "info";
      let ms = 8000;
      if (kindName === "Error") (kind = "error"), (ms = 8000);
      else if (kindName === "Warning") (kind = "warning"), (ms = 6000);
      else if (kindName === "Msg") (kind = "info"), (ms = 8000);
      else {
        const t = /,\s*(\d+)/.exec(args);
        ms = t ? Number(t[1]) : 8000;
        kind = /wxICON_ERROR/.test(args) ? "error" : /wxICON_WARNING/.test(args) ? "warning" : "info";
      }
      const close = kindName !== "For" && /,\s*true\b/.test(args);
      if (!messages.has(pattern)) messages.set(pattern, { pattern, kind, ms, close, source: f });
    }
  }
  const list = [...messages.values()].sort((a, b) => (a.pattern < b.pattern ? -1 : a.pattern > b.pattern ? 1 : 0));
  writeJson(OUT, { meta: meta(files), messages: list });
  console.log(`${list.length} message(s) from ${files.length} file(s)`);
}

main();
