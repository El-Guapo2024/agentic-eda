// What a page of the drawing sheet is built for in each editor (`DS_PROXY_VIEW_ITEM`'s setters as `PCB_EDIT_FRAME::SetPageSettings` and
// `SCH_VIEW::DisplaySheet` call them): the paper, the title block, the file, the sheet's name and path, the page number and count, and whether it is the
// first page.
import type { Schematic } from "../../api/types";
import type { DsPageContext } from "../../kicad-port/dsDataModel";
import { shownTitleBlock } from "../../kicad-port/pageSettings";

/** `${KICAD_VERSION}`: KiCad writes "KiCad E.D.A. " and its version (`productName`, `GetBaseVersion`); the studio names itself. kicad-cli's plots carry its own. */
export const KICAD_VERSION_TEXT = "KiCad E.D.A. eda studio";

/** The paper of a schematic sheet (`GET /api/schematic`'s `paper`), A4 landscape for a backend that sends none. */
function paperOfSheet(sch: Schematic): { name: string; widthNm: number; heightNm: number } {
  return sch.paper ? { name: sch.paper.name, widthNm: sch.paper.width_nm, heightNm: sch.paper.height_nm } : { name: "A4", widthNm: 297_000_000, heightNm: 210_000_000 };
}

/**
 * The schematic sheet in view as `SCH_VIEW::DisplaySheet` sets its drawing sheet up: the screen's page settings and title block, `${#}` its page number,
 * `${##}` the page count, the first page when its virtual page is 1, `${FILENAME}` the screen's file, `${SHEETNAME}` the sheet's name
 * (`GetScreenDesc`: the last sheet of the path) and `${SHEETPATH}` its path (`SCH_SHEET_PATH::PathHumanReadable`: "/" for the root, "/Sub/" below).
 *
 * The title falls back to the board's name and the date to today when the sheet has no title block at all, as the studio has always drawn it.
 */
export function schematicSheetPage(sch: Schematic, boardName: string | undefined): DsPageContext {
  const paper = paperOfSheet(sch);
  const tb = shownTitleBlock(sch.title_block);
  const crumbs = sch.sheet_path ?? [];
  return {
    widthNm: paper.widthNm,
    heightNm: paper.heightNm,
    paperName: paper.name,
    titleBlock: {
      title: tb?.title || boardName || "untitled",
      date: tb?.date ?? new Date().toISOString().slice(0, 10),
      rev: tb?.rev ?? "",
      company: tb?.company ?? "",
      comments: tb?.comments ?? [],
    },
    fileName: sch.file || `${boardName || "schematic"}.kicad_sch`,
    sheetName: crumbs.length === 0 ? "" : crumbs[crumbs.length - 1]!.name,
    sheetPath: crumbs.length === 0 ? "/" : `/${crumbs.map((c) => c.name).join("/")}/`,
    pageNumber: sch.sheet_page?.number ?? "1",
    sheetCount: sch.sheet_page?.count ?? 1,
    isFirstPage: sch.sheet_page?.first ?? true,
    kicadVersion: KICAD_VERSION_TEXT,
  };
}
