// Shared helper for the tools/extract-*.js scripts: read files out of
// KiCad's source at a pinned ref, without ever checking it out or
// modifying anything.
//
// Two modes:
//   - KICAD_SRC_DIR set: read plain files from a directory holding a
//     checkout of that commit (pcbnew/, common/, include/, resources/
//     bitmaps_png/sources/, ...) plus a `COMMIT` file with the full
//     hash. This is the only mode that works from inside a
//     worktree-isolated Claude Code agent session -- that sandbox
//     refuses any `git` invocation whose target isn't the agent's own
//     worktree, even read-only ones against an unrelated repo, even
//     nested inside a `node -e` subprocess call.
//   - KICAD_SRC_DIR unset: fall back to `git show <ref>:<path>` / `git
//     archive <ref>` against KICAD_MIRROR_DIR (never `git checkout`),
//     for anyone running this from a normal terminal with git access to
//     the mirror.
//
// No npm dependencies: only Node's own fs/path/child_process.

import { execFileSync } from "node:child_process";
import { mkdirSync, writeFileSync, readFileSync, readdirSync, statSync, cpSync, existsSync } from "node:fs";
import { dirname, join, relative } from "node:path";

export const KICAD_SRC_DIR = process.env.KICAD_SRC_DIR || "";
export const KICAD_MIRROR_DIR = process.env.KICAD_MIRROR_DIR || `${process.env.HOME}/ws/kicad-mirror`;
export const KICAD_REF = process.env.KICAD_REF || "origin/master";

function git(args) {
  return execFileSync("git", ["-C", KICAD_MIRROR_DIR, ...args], { encoding: "utf8", maxBuffer: 1 << 28 });
}

let cachedCommit = null;

/** The exact commit this extraction is against, so every generated file can be stamped with it. Cached per process. */
export function resolveCommit() {
  if (cachedCommit) return cachedCommit;
  if (KICAD_SRC_DIR) {
    const sha = readFileSync(join(KICAD_SRC_DIR, "COMMIT"), "utf8").trim();
    // COMMIT is just the hash; no date file alongside it, so the date comes from KICAD_COMMIT_DATE when the caller knows it (else null).
    cachedCommit = { sha, date: process.env.KICAD_COMMIT_DATE || null };
  } else {
    const sha = git(["rev-parse", KICAD_REF]).trim();
    const date = git(["show", "-s", "--format=%cI", sha]).trim();
    cachedCommit = { sha, date };
  }
  return cachedCommit;
}

/** One file's text content. Throws if the path doesn't exist. */
export function readKicadFile(path) {
  if (KICAD_SRC_DIR) return readFileSync(join(KICAD_SRC_DIR, path), "utf8");
  return git(["show", `${KICAD_REF}:${path}`]);
}

/** Copy `paths` (files or directories, repo-relative) into `destDir`, preserving their relative layout under destDir. Purely local; no network, no checkout of anything. */
export function extractPaths(paths, destDir) {
  mkdirSync(destDir, { recursive: true });
  if (KICAD_SRC_DIR) {
    for (const p of paths) {
      const src = join(KICAD_SRC_DIR, p);
      const dest = join(destDir, p);
      mkdirSync(dirname(dest), { recursive: true });
      if (existsSync(src)) cpSync(src, dest, { recursive: true });
    }
    return;
  }
  const archive = execFileSync("git", ["-C", KICAD_MIRROR_DIR, "archive", KICAD_REF, ...paths], { maxBuffer: 1 << 30 });
  execFileSync("tar", ["-x", "-C", destDir], { input: archive });
}

/** Filenames directly inside a directory (non-recursive). */
export function listKicadDir(path) {
  if (KICAD_SRC_DIR) return readdirSync(join(KICAD_SRC_DIR, path));
  return git(["ls-tree", "--name-only", `${KICAD_REF}:${path}`])
    .split("\n")
    .filter(Boolean);
}

function walk(dir) {
  const out = [];
  for (const entry of readdirSync(dir)) {
    const p = join(dir, entry);
    const s = statSync(p);
    if (s.isDirectory()) out.push(...walk(p));
    else out.push(p);
  }
  return out;
}

/** Every file under `dirs` (repo-relative) whose content contains the plain string `pattern`. Empty array if none match. */
export function discoverFilesContaining(pattern, dirs) {
  if (KICAD_SRC_DIR) {
    const hits = [];
    for (const d of dirs) {
      const abs = join(KICAD_SRC_DIR, d);
      if (!existsSync(abs)) continue;
      for (const file of walk(abs)) {
        let text;
        try {
          text = readFileSync(file, "utf8");
        } catch {
          continue; // binary file etc.
        }
        if (text.includes(pattern)) hits.push(relative(KICAD_SRC_DIR, file));
      }
    }
    return hits;
  }
  try {
    const out = execFileSync("git", ["-C", KICAD_MIRROR_DIR, "grep", "-F", "-l", pattern, KICAD_REF, "--", ...dirs], {
      encoding: "utf8",
      maxBuffer: 1 << 28,
    });
    return out
      .split("\n")
      .filter(Boolean)
      .map((line) => line.replace(new RegExp(`^${KICAD_REF}:`), ""));
  } catch {
    return [];
  }
}

export function meta(sourceFiles, note) {
  const { sha, date } = resolveCommit();
  const m = {
    generated: true,
    kicadCommit: sha,
    kicadCommitDate: date,
    extractedAt: new Date().toISOString(),
    sourceFiles,
  };
  if (note) m.note = note;
  return m;
}

export function writeJson(path, data) {
  mkdirSync(dirname(path), { recursive: true });
  writeFileSync(path, JSON.stringify(data, null, 2) + "\n");
  console.log(`wrote ${path}`);
}
