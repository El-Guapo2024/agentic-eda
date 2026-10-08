// The suite-level links of the shared tools -- Help, Getting Started, Get Involved, Donate, Report Bug -- and the version text of the About
// dialog and the bug report. Ported from common/tool/common_control.cpp (COMMON_CONTROL::ShowHelp / GetInvolved / Donate / ReportBug) and
// common/build_version.cpp (`GetVersionInfoData`), commit 8303b2ad. Pure: the handlers (actions/commonSuiteActions.ts) open the links and
// the dialog (components/AboutDialog.tsx) shows the text.
//
// Where the studio differs from KiCad, on purpose: the help pages are KiCad's (the studio's windows are KiCad's, so its manual is the manual),
// and Get Involved / Donate point at the KiCad project like the original, but Report Bug files the issue against this program's repository --
// a bug in the studio is not a bug in KiCad, and the KiCad tracker is not the place for it.

/** The editor whose Help menu was used (the studio's `EditorTab`, spelled out so this module stays free of the React store). */
export type HelpTab = "pcb" | "schematic" | "footprint" | "symbol" | "3d";

export const URL_GET_INVOLVED = "https://go.kicad.org/contribute/";
export const URL_DONATE = "https://go.kicad.org/app-donate";
export const URL_DOCUMENTATION = "https://go.kicad.org/docs/";
/** Where `ACTIONS::reportBug` opens a new issue. KiCad's own is "https://gitlab.com/kicad/code/kicad/-/issues/new?issuable_template=bare&issue[description]=%s". */
export const URL_BUG_REPORT = "https://github.com/El-Guapo2024/agentic-eda/issues/new";
export const URL_SOURCE = "https://github.com/El-Guapo2024/agentic-eda";

/** `GetMajorMinorVersion()`: the nightly the source is from (docs/ARCHITECTURE.md: "KiCad nightly (10.99 ...)"). */
export const KICAD_MAJOR_MINOR_VERSION = "10.99";

/**
 * `m_frame->help_name()`: the name of the frame's help file, `Kiface().GetHelpFileName()` -- "pcbnew" for the PCB Editor, the Footprint Editor
 * and the 3D viewer, "eeschema" for the Schematic and the Symbol Editor.
 */
export function helpNameFor(tab: HelpTab): string {
  return tab === "schematic" || tab === "symbol" ? "eeschema" : "pcbnew";
}

/** `baseUrl + name + "/"`: "https://go.kicad.org/docs/<version>/<language>/<name>/". `language` is the locale's language, "en" for en_US. */
export function helpUrl(name: string, language = "en", version = KICAD_MAJOR_MINOR_VERSION): string {
  return `${URL_DOCUMENTATION}${version}/${language}/${name}/`;
}

/** The document for beginners, the same for every KiCad program (`ACTIONS::gettingStarted`). */
export function gettingStartedUrl(language = "en", version = KICAD_MAJOR_MINOR_VERSION): string {
  return helpUrl("getting_started_in_kicad", language, version);
}

/** The language of a browser locale as `Pgm().GetLocale()->GetName().BeforeLast( '_' )` takes it from "en_US": "en-US" -> "en". */
export function languageOf(locale: string | undefined): string {
  const lang = (locale ?? "en").split(/[-_]/)[0]?.toLowerCase() ?? "en";
  return /^[a-z]{2,3}$/.test(lang) ? lang : "en";
}

/** libcurl's `curl_easy_escape` (`KICAD_CURL_EASY::Escape`): everything but A-Z a-z 0-9 - . _ ~ is percent-encoded (`encodeURIComponent` keeps ! ' ( ) * too). */
export function curlEscape(text: string): string {
  return encodeURIComponent(text).replace(/[!'()*]/g, (c) => `%${c.charCodeAt(0).toString(16).toUpperCase()}`);
}

/** The issue template (`COMMON_CONTROL::m_bugReportTemplate`, "not translated"): the version information in a code block. */
export function bugReportTemplate(versionInfo: string): string {
  return `\`\`\`\n${versionInfo}\n\`\`\``;
}

/** The URL `ACTIONS::reportBug` launches: a new issue whose description is the template with the (brief) version information. */
export function bugReportUrl(versionInfo: string): string {
  return `${URL_BUG_REPORT}?body=${curlEscape(bugReportTemplate(versionInfo))}`;
}

/** What the version information is built from; the browser's own values are passed in so the text is the same wherever it is made. */
export interface VersionEnv {
  /** `GetAboutTitle()` of the editor, e.g. "PCB Editor". */
  title: string;
  /** The studio's own version (package.json). */
  version: string;
  /** "release" or "debug" (`#ifdef DEBUG`): the production build or the dev server. */
  mode: "release" | "debug";
  reactVersion: string;
  /** The pinned KiCad source's commit (src/kicad/*.json `meta.kicadCommit`) and when the action and menu data were read from it. */
  kicadCommit: string | null;
  kicadExtractedAt: string | null;
  userAgent: string;
  platform: string;
  language: string;
  /** `aBrief`: leave out the build information (the bug report's version text is brief, the About dialog's Version page is not). */
  brief?: boolean;
}

/**
 * `GetVersionInfoData( aTitle, aHtml = false, aBrief )`: "Application:", "Version:", "Libraries:", "Platform:", "Build Info:" and
 * "Locale:" blocks, in KiCad's order, with the studio's counterparts of the C++ build facts (the browser stands where wxWidgets and the
 * OS stand; the compiler, Boost, OCC and ngspice lines have no counterpart here).
 */
export function versionInfoText(env: VersionEnv): string {
  const indent = "\t";
  const lines: string[] = [];
  lines.push(`Application: EDA Studio ${env.title}`, "");
  lines.push(`Version: ${env.version}, ${env.mode} build`, "");
  lines.push("Libraries:", `${indent}React ${env.reactVersion}`, "");
  lines.push(`Platform: ${env.platform || "unknown"}, ${env.userAgent}`, "");
  if (!env.brief) lines.push("Build Info:");
  if (env.kicadCommit) lines.push(`${indent}KiCad source: ${env.kicadCommit.slice(0, 10)} (KiCad ${KICAD_MAJOR_MINOR_VERSION})${env.kicadExtractedAt ? `, read ${env.kicadExtractedAt.slice(0, 10)}` : ""}`);
  lines.push(`${indent}DRC, ERC and the exports: kicad-cli, run by the studio server`);
  lines.push("", "Locale:", `${indent}Lang: ${env.language}`);
  return lines.join("\n");
}
