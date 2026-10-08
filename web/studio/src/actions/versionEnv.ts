// The facts the version information (kicad-port/appLinks.ts `versionInfoText`) is made of, read from the running page: the studio's version
// (package.json), React's, the pinned KiCad source the action and menu data were read from (src/kicad/actions.json `meta`) and the browser's
// platform. Shared by the About dialog's Version page and Report Bug.
import { version as reactVersion } from "react";
import pkg from "../../package.json";
import actionsData from "../kicad/actions.json";
import type { HelpTab, VersionEnv } from "../kicad-port/appLinks";

/** `GetAboutTitle()` of each editor frame. */
export const ABOUT_TITLES: Record<HelpTab, string> = {
  pcb: "PCB Editor",
  schematic: "Schematic Editor",
  footprint: "Footprint Editor",
  symbol: "Symbol Editor",
  "3d": "3D Viewer",
};

export function makeVersionEnv(tab: HelpTab, brief = false): VersionEnv {
  const meta = (actionsData as { meta: { kicadCommit: string | null; extractedAt: string | null } }).meta;
  return {
    title: ABOUT_TITLES[tab] ?? ABOUT_TITLES.pcb,
    version: pkg.version,
    mode: import.meta.env.PROD ? "release" : "debug",
    reactVersion,
    kicadCommit: meta.kicadCommit,
    kicadExtractedAt: meta.extractedAt,
    userAgent: navigator.userAgent,
    platform: navigator.platform,
    language: navigator.language,
    brief,
  };
}
