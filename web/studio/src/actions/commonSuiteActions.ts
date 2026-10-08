// The suite-level shared actions (`common.SuiteControl.*` and the `common.Control` ones that move between the editors or save): About, Help,
// Getting Started, Get Involved, Donate, Report Bug, the player windows (Footprint Editor, Symbol Editor), Save a Copy, Save All and Update
// Schematic from PCB. COMMON_CONTROL (common/tool/common_control.cpp) runs the first group in every editor frame; `registerCommonActions`
// (commonActions.ts) calls `registerSuiteActions` once while the registry is built.
import { downloadKicadPcb } from "../api/client";
import { setAboutOpen, setPageSettingsOpen } from "../state/commonDialogs";
import { URL_DONATE, URL_GET_INVOLVED, bugReportUrl, gettingStartedUrl, helpNameFor, helpUrl, languageOf, versionInfoText, type HelpTab } from "../kicad-port/appLinks";
import { makeVersionEnv } from "./versionEnv";
import { isCanvasTab } from "./editorAdapter";
import type { ActionHandler, CommonActionContext } from "./commonActions";

/** `wxLaunchDefaultBrowser`: the page in a new browser tab, which the opener cannot reach (`noopener`). */
export function openExternal(url: string): void {
  const a = document.createElement("a");
  a.href = url;
  a.target = "_blank";
  a.rel = "noopener noreferrer";
  document.body.appendChild(a);
  a.click();
  a.remove();
}

export function registerSuiteActions(m: Map<string, ActionHandler>, ctx: CommonActionContext): void {
  const { dispatch } = ctx;
  const tab = ctx.tab;
  const language = () => languageOf(typeof navigator === "undefined" ? undefined : navigator.language);
  const toast = (message: string, kind: "error" | "info" = "info") => dispatch({ type: "TOAST", message, kind });

  // ACTIONS::about -- COMMON_CONTROL::About -> ShowAboutDialog( m_frame ): DIALOG_ABOUT (components/AboutDialog.tsx).
  m.set("common.SuiteControl.about", () => setAboutOpen(true));

  // ACTIONS::help -- COMMON_CONTROL::ShowHelp: the frame's own manual, "https://go.kicad.org/docs/<major.minor>/<language>/<help_name>/"
  // (`m_frame->help_name()`: pcbnew or eeschema). KiCad first looks for an installed copy of the help file and only then offers the
  // online one; the studio has no installed docs, so it is always the online manual.
  m.set("common.SuiteControl.help", () => openExternal(helpUrl(helpNameFor(tab as HelpTab), language())));

  // ACTIONS::gettingStarted -- the same function for the document for beginners, shared by every KiCad program.
  m.set("common.SuiteControl.gettingStarted", () => openExternal(gettingStartedUrl(language())));

  // ACTIONS::getInvolved -- COMMON_CONTROL::GetInvolved: `wxLaunchDefaultBrowser( URL_GET_INVOLVED )`.
  m.set("common.SuiteControl.getInvolved", () => openExternal(URL_GET_INVOLVED));

  // ACTIONS::donate -- COMMON_CONTROL::Donate: `wxLaunchDefaultBrowser( URL_DONATE )`.
  m.set("common.SuiteControl.donate", () => openExternal(URL_DONATE));

  // ACTIONS::reportBug -- COMMON_CONTROL::ReportBug: a new issue whose description is the brief version information in a code block
  // (kicad-port/appLinks.ts `bugReportUrl`; the issue goes to this program's repository, not KiCad's).
  m.set("common.SuiteControl.reportBug", () => openExternal(bugReportUrl(versionInfoText(makeVersionEnv(tab as HelpTab, true)))));

  // ACTIONS::showFootprintEditor / showSymbolEditor -- COMMON_CONTROL::ShowPlayer: `Kiway().Player( FRAME_FOOTPRINT_EDITOR / FRAME_SCH_SYMBOL_EDITOR )`
  // opens the editor's window and raises it. One window with a tab per editor here (as `pcbnew.EditorControl.showEeschema` does), so the
  // tab is the window.
  m.set("common.Control.showFootprintEditor", () => dispatch({ type: "SET_TAB", tab: "footprint" }));
  m.set("common.Control.showSymbolEditor", () => dispatch({ type: "SET_TAB", tab: "symbol" }));

  // ACTIONS::pageSettings -- BOARD_EDITOR_CONTROL::PageSettings / SCH_EDITOR_CONTROL::PageSetup: the Page Settings dialog (paper and title
  // block) of the board or of the schematic on screen (components/PageSettingsDialog.tsx); one undo step when it is accepted.
  if (tab === "pcb" || tab === "schematic") m.set("common.Control.pageSettings", () => setPageSettingsOpen(tab));

  // ACTIONS::saveCopy -- BOARD_EDITOR_CONTROL::SaveCopy: `SaveBoard( true, true )`, the board written to another file while the editor
  // stays on its own. That is what the studio's Save As already is (design.json stays the master; the derived `.kicad_pcb` goes to the
  // browser's Save), so the two share the download. KiCad offers it in the PCB Editor's File menu only.
  if (tab === "pcb") {
    m.set("common.Control.saveCopy", () => {
      const name = ctx.api.getState().board?.name ?? "board";
      downloadKicadPcb(name)
        .then((file) => toast(`Saved ${file} (derived from design.json).`))
        .catch((e: unknown) => toast(e instanceof Error ? e.message : String(e), "error"));
    });
  }

  // ACTIONS::saveAll -- SYMBOL_EDITOR_CONTROL::Save( saveAll ): every modified library. Every edit of the studio is written to
  // design.json when it is made (the same reason `common.Control.save` only reports), so there is nothing modified left to write.
  if (isCanvasTab(tab)) {
    m.set("common.Control.saveAll", () => toast("Everything is saved: each edit is written to design.json as you make it."));
  }

  // ACTIONS::updateSchematicFromPCB -- PCB_EDITOR_CONTROL::UpdateSchematicFromPCB / SCH_EDITOR_CONTROL::UpdateSchematicFromPCB: the
  // back annotation carries what was changed on the board (references, values, footprint assignments, pin and gate swaps) over to the
  // schematic. Here the two editors are views of one design, and the PCB cannot change a footprint's reference, value or assignment on
  // its own (FootprintPropertiesDialog shows them read-only), so there is never anything to carry over: like F8, the action refetches
  // the design and says so.
  if (tab === "pcb" || tab === "schematic") {
    m.set("common.Control.updateSchematicFromPCB", () => {
      void ctx.api.refresh().then(() => toast("Schematic is up to date with the PCB (references, values and footprint assignments are fields of the one design; a board edit cannot change them)."));
    });
  }
}
