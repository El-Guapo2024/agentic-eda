import { test } from "node:test";
import assert from "node:assert/strict";
import { bugReportTemplate, bugReportUrl, curlEscape, gettingStartedUrl, helpNameFor, helpUrl, languageOf, versionInfoText, URL_DONATE, URL_GET_INVOLVED } from "./appLinks";

test("the help file name is the kiface's: pcbnew for the board side, eeschema for the schematic side", () => {
  assert.equal(helpNameFor("pcb"), "pcbnew");
  assert.equal(helpNameFor("footprint"), "pcbnew");
  assert.equal(helpNameFor("3d"), "pcbnew");
  assert.equal(helpNameFor("schematic"), "eeschema");
  assert.equal(helpNameFor("symbol"), "eeschema");
});

test("help URLs: go.kicad.org/docs/<major.minor>/<language>/<name>/", () => {
  assert.equal(helpUrl("pcbnew"), "https://go.kicad.org/docs/10.99/en/pcbnew/");
  assert.equal(helpUrl("eeschema", "de", "9.0"), "https://go.kicad.org/docs/9.0/de/eeschema/");
  assert.equal(gettingStartedUrl(), "https://go.kicad.org/docs/10.99/en/getting_started_in_kicad/");
  assert.equal(URL_GET_INVOLVED, "https://go.kicad.org/contribute/");
  assert.equal(URL_DONATE, "https://go.kicad.org/app-donate");
});

test("the language is the part of the locale before the region, like GetName().BeforeLast( '_' )", () => {
  assert.equal(languageOf("en-US"), "en");
  assert.equal(languageOf("pt_BR"), "pt");
  assert.equal(languageOf("DE"), "de");
  assert.equal(languageOf(undefined), "en");
  assert.equal(languageOf("???"), "en");
});

test("curl's escape keeps only unreserved characters, so ! ' ( ) * are escaped too", () => {
  assert.equal(curlEscape("a b&c=d"), "a%20b%26c%3Dd");
  assert.equal(curlEscape("(x)*'!"), "%28x%29%2A%27%21");
  assert.equal(curlEscape("A-z0.9_~"), "A-z0.9_~");
  assert.equal(curlEscape("\n`"), "%0A%60");
});

test("the bug report: a new issue whose description is the version information in a code block", () => {
  assert.equal(bugReportTemplate("v"), "```\nv\n```");
  const url = bugReportUrl("Version: 0.1.0");
  assert.ok(url.startsWith("https://github.com/El-Guapo2024/agentic-eda/issues/new?body="));
  assert.equal(decodeURIComponent(url.split("body=")[1]!), "```\nVersion: 0.1.0\n```");
});

test("version information has KiCad's blocks in KiCad's order, and a brief one drops the Build Info header", () => {
  const env = {
    title: "PCB Editor",
    version: "0.1.0",
    mode: "release" as const,
    reactVersion: "19.0.0",
    kicadCommit: "8303b2ada05226fa603e5ac0420d240fd65ce52b",
    kicadExtractedAt: "2026-10-01T04:06:45.439Z",
    userAgent: "UA/1.0",
    platform: "MacIntel",
    language: "en-US",
  };
  const full = versionInfoText(env);
  const order = ["Application: EDA Studio PCB Editor", "Version: 0.1.0, release build", "Libraries:", "\tReact 19.0.0", "Platform: MacIntel, UA/1.0", "Build Info:", "\tKiCad source: 8303b2ada0 (KiCad 10.99), read 2026-10-01", "Locale:", "\tLang: en-US"];
  let at = -1;
  for (const part of order) {
    const i = full.indexOf(part, at + 1);
    assert.ok(i > at, `${part} comes in order`);
    at = i;
  }
  const brief = versionInfoText({ ...env, brief: true });
  assert.ok(!brief.includes("Build Info:"));
  assert.ok(brief.includes("KiCad source: 8303b2ada0"), "the lines under it stay, as in GetVersionInfoData");
});
