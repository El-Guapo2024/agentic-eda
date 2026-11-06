// `ACTIONS::pageSettings` -> DIALOG_PAGES_SETTINGS (common/dialogs/dialog_page_settings.cpp), as `BOARD_EDITOR_CONTROL::PageSettings` runs it for the
// board and `SCH_EDITOR_CONTROL::PageSetup` for the schematic: the paper (a standard format and its orientation, or a custom size), the drawing sheet
// file (`.kicad_wks`) and the title block (date with the calendar's Apply, revision, title, company, comments 1 to 9), with a preview of the page drawn
// with the chosen sheet (`UpdateDrawingSheetExample`). OK is one undo step (`set_board_page` / `set_schematic_page`, and `set_drawing_sheet` in the same
// batch when the sheet changed), and nothing is sent when nothing changed.
//
// The drawing sheet (`OnWksFileSelection`): the browse button opens the server-side file browser on the `.kicad_wks` files (the board's folder first,
// KiCad's template folder one click away) -- a page cannot open the machine's own file dialog -- and the file chosen is read and previewed, and kept by the
// name KiCad keeps it under: relative to the board's folder when it is in it, else with a path variable, else whole. "Upload..." puts a sheet from the
// person's computer into the board's folder first. A name typed into the File box is taken on OK (`SavePageSettings`), with KiCad's "Error loading drawing
// sheet" when it cannot be read; an empty name is KiCad's default sheet.
//
// Not ported: the schematic dialog's "export to other sheets" boxes (the editor works on one sheet), and embedding the sheet in the document
// (`FILEDLG_HOOK_EMBED_FILE`).
import { Fragment, useEffect, useRef, useState } from "react";
import { useStudioApi, useStudioState } from "../state/store";
import { setPageSettingsOpen, useCommonDialogs } from "../state/commonDialogs";
import { nmFrom, nmTo } from "../state/units";
import {
  COMMENT_COUNT,
  DEFAULT_USER_SIZE_NM,
  MAX_PAGE_SIZE_EESCHEMA_NM,
  MAX_PAGE_SIZE_PCBNEW_NM,
  PAGE_FORMATS,
  USER_PAPER,
  USER_PAPER_LABEL,
  isoDate,
  normalizePaper,
  paperOf,
  paperSizeNm,
  paperToCmd,
  samePaper,
  titleBlockFields,
  titleBlockToSend,
  userSizeError,
  type Paper,
  type TitleBlockFields,
} from "../kicad-port/pageSettings";
import type { DsPageContext } from "../kicad-port/dsDataModel";
import type { Cmd } from "../api/types";
import { paintDrawingSheet, sheetModel } from "./drawingSheet/dsPainter";
import { KICAD_VERSION_TEXT, schematicSheetPage } from "./drawingSheet/sheetInfo";
import { layerColor } from "./canvas/layers";
import { FolderBrowser } from "./libraryTables/FolderBrowser";
import { drawingSheetFolders, pickDrawingSheet, uploadDrawingSheet } from "../api/drawingSheetClient";

/** `MAX_PAGE_EXAMPLE_SIZE`: the longest side of the sketch of the page, px. */
const EXAMPLE_PX = 200;

/**
 * `UpdateDrawingSheetExample`: the page at `MAX_PAGE_EXAMPLE_SIZE` on its longest side, in the editor's background colour, with the chosen drawing sheet
 * drawn on it in the editor's drawing sheet colour (`LAYER_SCHEMATIC_DRAWINGSHEET` in the schematic) for the title block as typed.
 */
function PageExample({ page, text, schematic }: { page: DsPageContext; text: string | null; schematic: boolean }) {
  const ref = useRef<HTMLCanvasElement>(null);
  const w = Math.max(page.widthNm, 1);
  const h = Math.max(page.heightNm, 1);
  const scale = EXAMPLE_PX / Math.max(w, h);
  const px = [Math.round(w * scale) + 1, Math.round(h * scale) + 1] as const;
  useEffect(() => {
    const canvas = ref.current;
    const ctx = canvas?.getContext("2d");
    if (!canvas || !ctx) return;
    ctx.setTransform(1, 0, 0, 1, 0, 0);
    ctx.fillStyle = layerColor(schematic ? "LAYER_SCHEMATIC_BACKGROUND" : "background");
    ctx.fillRect(0, 0, canvas.width, canvas.height);
    ctx.scale(scale, scale);
    paintDrawingSheet(ctx, sheetModel(text).model, page, { tag: "sketch", color: layerColor(schematic ? "LAYER_SCHEMATIC_DRAWINGSHEET" : "LAYER_DRAWINGSHEET"), pixelNm: 1 / scale });
  });
  return <canvas ref={ref} width={px[0]} height={px[1]} role="img" aria-label="Page example" style={{ border: "1px solid var(--chrome-border)" }} />;
}

export function PageSettingsDialog() {
  const target = useCommonDialogs().page;
  const state = useStudioState();
  const api = useStudioApi();
  const unit = state.units;

  const [paper, setPaper] = useState<string>("A4");
  const [portrait, setPortrait] = useState(false);
  const [userW, setUserW] = useState("");
  const [userH, setUserH] = useState("");
  const [tb, setTb] = useState<TitleBlockFields>(titleBlockFields(null));
  const [pick, setPick] = useState(isoDate(new Date()));
  const [busy, setBusy] = useState(false);
  // The drawing sheet: the File box (`m_textCtrlFilePicker`), the sheet the preview draws (`m_drawingSheet`) and a message for the person
  const [sheetPath, setSheetPath] = useState("");
  const [sheetText, setSheetText] = useState<string | null>(null);
  const [sheetMessage, setSheetMessage] = useState<string | null>(null);
  const [browsing, setBrowsing] = useState(false);
  const [templates, setTemplates] = useState<string | null>(null);
  const fileInput = useRef<HTMLInputElement>(null);

  const current =
    target === "pcb"
      ? { page: state.board?.page, titleBlock: state.board?.title_block, sheet: state.board?.drawing_sheet }
      : { page: state.schematic?.page, titleBlock: state.schematic?.title_block, sheet: state.schematic?.drawing_sheet };

  // Seed the form when the dialog opens (`TransferDataToWindow`): the document's paper, drawing sheet and title block as they are now.
  useEffect(() => {
    if (!target) return;
    const p = normalizePaper(paperOf(current.page));
    setPaper(p.paper);
    setPortrait(p.portrait);
    // The custom size fields hold the last custom size (`GetCustomWidthMils`) whatever the format, KiCad's 17 x 11 in before one was set.
    const digits = unit === "mm" ? 3 : unit === "mil" ? 1 : 4;
    setUserW(nmTo(p.userSize?.[0] ?? DEFAULT_USER_SIZE_NM[0], unit).toFixed(digits));
    setUserH(nmTo(p.userSize?.[1] ?? DEFAULT_USER_SIZE_NM[1], unit).toFixed(digits));
    setTb(titleBlockFields(current.titleBlock));
    setPick(isoDate(new Date()));
    setSheetPath(current.sheet?.path ?? "");
    setSheetText(current.sheet?.text ?? null);
    setSheetMessage(current.sheet?.error ? `Error loading drawing sheet '${current.sheet.file}'.\n${current.sheet.error}` : null);
    setBrowsing(false);
    void drawingSheetFolders().then((f) => setTemplates(f.templates ?? null), () => setTemplates(null));
    // Only when the dialog opens -- not on every edit that changes the document under it.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [target]);

  if (!target) return null;
  const close = () => setPageSettingsOpen(null);
  const maxNm = target === "pcb" ? MAX_PAGE_SIZE_PCBNEW_NM : MAX_PAGE_SIZE_EESCHEMA_NM;

  const isUser = paper === USER_PAPER;
  const userSizeNm: [number, number] = [nmFrom(Number(userW), unit), nmFrom(Number(userH), unit)];
  const edited: Paper = normalizePaper({ paper, portrait, userSize: isUser ? userSizeNm : null });
  const error = isUser ? userSizeError(userSizeNm[0], userSizeNm[1], maxNm) : null;
  // `GetPageLayoutInfoFromDialog`: the sketch follows the choice as it is made; a User size that is not a size yet sketches the old page.
  const sketch = error ? paperSizeNm(normalizePaper(paperOf(current.page))) : paperSizeNm(edited);

  const before = normalizePaper(paperOf(current.page));
  const beforeTb = titleBlockToSend(titleBlockFields(current.titleBlock));
  const afterTb = titleBlockToSend(tb);
  const pageChanged = !samePaper(before, edited) || JSON.stringify(beforeTb) !== JSON.stringify(afterTb);
  const sheetChanged = sheetPath.trim() !== (current.sheet?.path ?? "");
  const changed = pageChanged || sheetChanged;

  // The page the preview draws: the document's own page (the board's page 1 of 1; the schematic's sheet in view) on the paper and title block as typed.
  const tbShown = { title: tb.title, date: tb.date, rev: tb.rev, company: tb.company, comments: tb.comments };
  const examplePage: DsPageContext =
    target === "schematic" && state.schematic
      ? { ...schematicSheetPage(state.schematic, state.board?.name), widthNm: sketch.width, heightNm: sketch.height, paperName: edited.paper, titleBlock: tbShown }
      : { widthNm: sketch.width, heightNm: sketch.height, paperName: edited.paper, titleBlock: tbShown, fileName: `${state.board?.name ?? "board"}.kicad_pcb`, sheetName: "", sheetPath: "", pageNumber: "1", sheetCount: 1, isFirstPage: true, kicadVersion: KICAD_VERSION_TEXT };

  const setField = (key: "title" | "date" | "rev" | "company", value: string) => setTb({ ...tb, [key]: value });
  const setComment = (i: number, value: string) => setTb({ ...tb, comments: tb.comments.map((c, j) => (j === i ? value : c)) });

  /** `OnWksFileSelection` after the file dialog: read the file (an error keeps the old sheet), then keep its short name and preview it. */
  const takeSheet = async (path: string) => {
    const r = await pickDrawingSheet(path);
    if (!r.ok) {
      setSheetMessage(r.error ?? "Error loading drawing sheet.");
      return;
    }
    setSheetPath(r.path ?? path);
    setSheetText(r.text ?? null);
    setSheetMessage(null);
  };

  const upload = async (file: File) => {
    const r = await uploadDrawingSheet(file.name, await file.text());
    if (!r.ok) {
      setSheetMessage(r.error ?? "The drawing sheet could not be written into the board's folder.");
      return;
    }
    setSheetPath(r.path ?? file.name);
    setSheetText(r.text ?? null);
    setSheetMessage(null);
  };

  const ok = async () => {
    if (error) return;
    if (!changed) return close();
    setBusy(true);
    try {
      const cmds: Cmd[] = [];
      if (sheetChanged) {
        // `SavePageSettings`: the sheet is loaded on OK; one that cannot be read is said so, and its name kept all the same
        const name = sheetPath.trim();
        if (name) {
          const r = await pickDrawingSheet(name);
          if (!r.ok && r.error) setSheetMessage(r.error);
        }
        cmds.push({ op: "set_drawing_sheet", schematic: target === "schematic", path: name || null });
      }
      if (pageChanged) cmds.push({ op: target === "pcb" ? "set_board_page" : "set_schematic_page", page: paperToCmd(edited), title_block: afterTb });
      const done = await api.cmd(cmds.length === 1 ? cmds[0]! : { op: "batch", cmds });
      if (done) close();
    } finally {
      setBusy(false);
    }
  };

  const field = (label: string, value: string, onChange: (v: string) => void, id: string) => (
    <Fragment key={id}>
      <span>{label}</span>
      <input id={id} aria-label={label} value={value} onChange={(e) => onChange(e.target.value)} />
    </Fragment>
  );

  // The browser starts where the sheet is (`OnWksFileSelection`: its folder when the name is whole or expands to a whole path, else the project's).
  const browseStart = sheetPath.includes("/") ? sheetPath.slice(0, sheetPath.lastIndexOf("/")) : "";

  return (
    <div className="dialog-backdrop" onClick={close}>
      <div className="dialog" style={{ width: 680, maxHeight: "90vh" }} onClick={(e) => e.stopPropagation()} onKeyDown={(e) => e.key === "Enter" && (e.target as HTMLElement).tagName === "INPUT" && void ok()} role="dialog" aria-label="Page Settings">
        <div className="dialog-header">
          <span>Page Settings</span>
        </div>
        <div className="dialog-body" style={{ display: "grid", gridTemplateColumns: "1fr 1fr", gap: 16 }}>
          <div>
            <div style={{ fontWeight: 600, marginBottom: 6 }}>Paper</div>
            <div className="kv-grid" style={{ gridTemplateColumns: "90px 1fr" }}>
              <span>Size</span>
              <select id="page-size" aria-label="Size" value={paper} onChange={(e) => setPaper(e.target.value)}>
                {PAGE_FORMATS.map((f) => (
                  <option key={f.name} value={f.name}>
                    {f.label}
                  </option>
                ))}
                <option value={USER_PAPER}>{USER_PAPER_LABEL}</option>
              </select>
              <span>Orientation</span>
              <select id="page-orientation" aria-label="Orientation" value={isUser ? (sketch.width < sketch.height ? "portrait" : "landscape") : portrait ? "portrait" : "landscape"} disabled={isUser} onChange={(e) => setPortrait(e.target.value === "portrait")}>
                <option value="landscape">Landscape</option>
                <option value="portrait">Portrait</option>
              </select>
              <span>{`Width (${unit})`}</span>
              <input id="page-user-w" aria-label="Width" value={userW} disabled={!isUser} onChange={(e) => setUserW(e.target.value)} />
              <span>{`Height (${unit})`}</span>
              <input id="page-user-h" aria-label="Height" value={userH} disabled={!isUser} onChange={(e) => setUserH(e.target.value)} />
            </div>
            {error && (
              <div role="alert" style={{ marginTop: 8, fontSize: 12, color: "var(--chrome-error, #e55)" }}>
                {error}
              </div>
            )}
            <div style={{ fontWeight: 600, margin: "12px 0 6px" }}>Preview</div>
            <PageExample page={examplePage} text={sheetText} schematic={target === "schematic"} />
            <div style={{ fontWeight: 600, margin: "12px 0 6px" }}>Drawing Sheet</div>
            <div style={{ display: "flex", gap: 6, alignItems: "center" }}>
              <span>File:</span>
              <input
                id="page-drawing-sheet"
                aria-label="Drawing sheet file"
                style={{ flex: 1, minWidth: 0 }}
                value={sheetPath}
                placeholder="(KiCad's default sheet)"
                spellCheck={false}
                onChange={(e) => setSheetPath(e.target.value)}
              />
              <button title="Browse for a drawing sheet file" aria-label="Browse" onClick={() => setBrowsing(true)}>
                …
              </button>
            </div>
            <div style={{ display: "flex", gap: 6, marginTop: 6 }}>
              <button title="Put a drawing sheet from this computer into the board's folder and use it" onClick={() => fileInput.current?.click()}>
                Upload...
              </button>
              <button title="KiCad's default drawing sheet" disabled={!sheetPath} onClick={() => { setSheetPath(""); setSheetText(null); setSheetMessage(null); }}>
                Default Sheet
              </button>
              <input
                ref={fileInput}
                type="file"
                accept=".kicad_wks"
                aria-label="Upload a drawing sheet"
                data-testid="page-drawing-sheet-upload"
                style={{ display: "none" }}
                onChange={(e) => {
                  const f = e.target.files?.[0];
                  e.target.value = "";
                  if (f) void upload(f);
                }}
              />
            </div>
            {sheetMessage && (
              <div role="alert" style={{ marginTop: 6, fontSize: 12, whiteSpace: "pre-wrap", color: "var(--chrome-error, #e55)" }}>
                {sheetMessage}
              </div>
            )}
          </div>
          <div>
            <div style={{ fontWeight: 600, marginBottom: 6 }}>Title Block</div>
            <div className="kv-grid" style={{ gridTemplateColumns: "90px 1fr" }}>
              {field("Issue Date", tb.date, (v) => setField("date", v), "page-date")}
              <span />
              <div style={{ display: "flex", gap: 6 }}>
                <input type="date" value={pick} onChange={(e) => setPick(e.target.value)} aria-label="Pick a date" />
                <button onClick={() => pick && setField("date", pick)}>Apply</button>
              </div>
              {field("Revision", tb.rev, (v) => setField("rev", v), "page-rev")}
              {field("Title", tb.title, (v) => setField("title", v), "page-title")}
              {field("Company", tb.company, (v) => setField("company", v), "page-company")}
              {Array.from({ length: COMMENT_COUNT }, (_, i) => field(`Comment${i + 1}`, tb.comments[i] ?? "", (v) => setComment(i, v), `page-comment-${i + 1}`))}
            </div>
          </div>
        </div>
        <div className="dialog-footer">
          <button onClick={close}>Cancel</button>
          <button className="primary" disabled={!!error || busy} onClick={() => void ok()}>
            OK
          </button>
        </div>
      </div>
      {browsing && (
        <div onClick={(e) => e.stopPropagation()}>
          <FolderBrowser
            title="Drawing Sheet File"
            what="drawing_sheet"
            start={browseStart}
            places={templates ? [{ label: "KiCad templates", path: templates }] : undefined}
            onCancel={() => setBrowsing(false)}
            onPick={(path) => {
              setBrowsing(false);
              void takeSheet(path);
            }}
          />
        </div>
      )}
    </div>
  );
}
