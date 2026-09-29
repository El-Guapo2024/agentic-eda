// Shared helper for the tools/extract-*.js scripts: read files out of a
// local clone of KiCad at a pinned ref, without ever checking it out or
// modifying it.
//
// Reads go through `git show <ref>:<path>` and `git archive <ref>`
// (never `git checkout`), exactly as the task that produced this file
// specified. That command cannot currently be run from inside a
// worktree-isolated Claude Code agent session -- this repo's own
// .claude worktree sandbox refuses any `git` invocation whose target
// isn't the agent's own worktree, even read-only ones, even nested
// inside a `node -e` subprocess call. Run these scripts from a plain
// terminal instead (see the repo README or ask whoever set up
// ~/ws/kicad-mirror). See src/kicad/actions.json's `meta.note` for the
// full story.
//
// No npm dependencies: only Node's own child_process/fs/path.

import { execFileSync } from "node:child_process";
import { mkdirSync, writeFileSync } from "node:fs";
import { dirname } from "node:path";

export const KICAD_MIRROR_DIR = process.env.KICAD_MIRROR_DIR || `${process.env.HOME}/ws/kicad-mirror`;
export const KICAD_REF = process.env.KICAD_REF || "origin/master";

function git(args, opts = {}) {
  return execFileSync("git", ["-C", KICAD_MIRROR_DIR, ...args], { encoding: "utf8", maxBuffer: 1 << 28, ...opts });
}

let cachedCommit = null;

/** The exact commit KICAD_REF resolves to right now, so every generated file can be stamped with it. Cached per process. */
export function resolveCommit() {
  if (cachedCommit) return cachedCommit;
  const sha = git(["rev-parse", KICAD_REF]).trim();
  const date = git(["show", "-s", "--format=%cI", sha]).trim();
  cachedCommit = { sha, date };
  return cachedCommit;
}

/** One file's text content at KICAD_REF. Throws if the path doesn't exist there. */
export function readKicadFile(path) {
  return git(["show", `${KICAD_REF}:${path}`]);
}

/**
 * Copy `paths` (files or directories, repo-relative) out of the repo at
 * KICAD_REF into `destDir`, preserving their relative layout under
 * destDir. Pure local archive + extract; no network, no checkout of the
 * mirror itself.
 */
export function extractPaths(paths, destDir) {
  mkdirSync(destDir, { recursive: true });
  const archive = execFileSync("git", ["-C", KICAD_MIRROR_DIR, "archive", KICAD_REF, ...paths], { maxBuffer: 1 << 30 });
  execFileSync("tar", ["-x", "-C", destDir], { input: archive });
}

/** List filenames directly inside a directory at KICAD_REF (non-recursive), via `git ls-tree`. */
export function listKicadDir(path) {
  const out = git(["ls-tree", "--name-only", `${KICAD_REF}:${path}`]);
  return out.split("\n").filter(Boolean);
}

/** Every file under `dirs` (repo-relative) whose content matches `pattern` (a plain string, `git grep -F`), via `git grep`. Empty array if none match -- `git grep` exits non-zero for "no matches", which is not an error here. */
export function discoverFilesContaining(pattern, dirs) {
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
