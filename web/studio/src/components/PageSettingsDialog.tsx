// `ACTIONS::pageSettings` -> DIALOG_PAGES_SETTINGS (common/dialogs/dialog_page_settings.cpp), as `BOARD_EDITOR_CONTROL::PageSettings` runs it for the
// board and `SCH_EDITOR_CONTROL::PageSetup` for the schematic: the paper (a standard format and its orientation, or a custom size) and the title
// block (date with the calendar's Apply, revision, title, company, comments 1 to 9), with a sketch of the page. OK is one undo step (`set_board_page` /
// `set_schematic_page`), and nothing is sent when nothing changed. Not ported: the drawing sheet file (the sheet is KiCad's default one) and the
// schematic dialog's "export to other sheets" boxes (the editor works on one sheet).
import { Fragment, useEffect, useState } from "react";
import { useStudioApi, useStudioState } from "../state/store";
import { setPageSettingsOpen, useCommonDialogs } from "../state/commonDialogs";
import { umFrom, umTo } from "../state/units";
import {
  COMMENT_COUNT,
  DEFAULT_USER_SIZE_UM,
  MAX_PAGE_SIZE_EESCHEMA_UM,
  MAX_PAGE_SIZE_PCBNEW_UM,
  PAGE_FORMATS,
  USER_PAPER,
  USER_PAPER_LABEL,
  isoDate,
  normalizePaper,
  paperOf,
  paperSizeUm,
  paperToCmd,
  samePaper,
  titleBlockFields,
  titleBlockToSend,
  userSizeError,
  type Paper,
  type TitleBlockFields,
} from "../kicad-port/pageSettings";

/** `MAX_PAGE_EXAMPLE_SIZE`: the longest side of the sketch of the page, px. */
const EXAMPLE_PX = 200;

/** The default sheet's frame (10 mm margin) and title block (110 x 34 mm at the bottom right, 2 mm in), drawn on a page sketch. */
function PageExample({ width, height }: { width: number; height: number }) {
  const w = Math.max(width, 1);
  const h = Math.max(height, 1);
  const scale = EXAMPLE_PX / Math.max(w, h);
  const mm = 1000 * scale;
  const frame = 10 * mm;
  const tbW = 110 * mm;
  const tbH = 34 * mm;
  return (
    <svg width={w * scale + 1} height={h * scale + 1} role="img" aria-label="Page example" style={{ background: "#fff", border: "1px solid var(--chrome-border)" }}>
      <rect x={frame} y={frame} width={Math.max(w * scale - 2 * frame, 0)} height={Math.max(h * scale - 2 * frame, 0)} fill="none" stroke="#c83434" strokeWidth={1} />
      <rect x={w * scale - frame - tbW + 2 * mm} y={h * scale - frame - tbH + 2 * mm} width={Math.max(tbW - 4 * mm, 0)} height={Math.max(tbH - 4 * mm, 0)} fill="none" stroke="#c83434" strokeWidth={1} />
    </svg>
  );
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

  const current = target === "pcb" ? { page: state.board?.page, titleBlock: state.board?.title_block } : { page: state.schematic?.page, titleBlock: state.schematic?.title_block };

  // Seed the form when the dialog opens (`TransferDataToWindow`): the document's paper and title block as they are now.
  useEffect(() => {
    if (!target) return;
    const p = normalizePaper(paperOf(current.page));
    setPaper(p.paper);
    setPortrait(p.portrait);
    // The custom size fields hold the last custom size (`GetCustomWidthMils`) whatever the format, KiCad's 17 x 11 in before one was set.
    const digits = unit === "mm" ? 3 : unit === "mil" ? 1 : 4;
    setUserW(umTo(p.userSize?.[0] ?? DEFAULT_USER_SIZE_UM[0], unit).toFixed(digits));
    setUserH(umTo(p.userSize?.[1] ?? DEFAULT_USER_SIZE_UM[1], unit).toFixed(digits));
    setTb(titleBlockFields(current.titleBlock));
    setPick(isoDate(new Date()));
    // Only when the dialog opens -- not on every edit that changes the document under it.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [target]);

  if (!target) return null;
  const close = () => setPageSettingsOpen(null);
  const maxUm = target === "pcb" ? MAX_PAGE_SIZE_PCBNEW_UM : MAX_PAGE_SIZE_EESCHEMA_UM;

  const isUser = paper === USER_PAPER;
  const userSizeUm: [number, number] = [umFrom(Number(userW), unit), umFrom(Number(userH), unit)];
  const edited: Paper = normalizePaper({ paper, portrait, userSize: isUser ? userSizeUm : null });
  const error = isUser ? userSizeError(userSizeUm[0], userSizeUm[1], maxUm) : null;
  // `GetPageLayoutInfoFromDialog`: the sketch follows the choice as it is made; a User size that is not a size yet sketches the old page.
  const sketch = error ? paperSizeUm(normalizePaper(paperOf(current.page))) : paperSizeUm(edited);

  const before = normalizePaper(paperOf(current.page));
  const beforeTb = titleBlockToSend(titleBlockFields(current.titleBlock));
  const afterTb = titleBlockToSend(tb);
  const changed = !samePaper(before, edited) || JSON.stringify(beforeTb) !== JSON.stringify(afterTb);

  const setField = (key: "title" | "date" | "rev" | "company", value: string) => setTb({ ...tb, [key]: value });
  const setComment = (i: number, value: string) => setTb({ ...tb, comments: tb.comments.map((c, j) => (j === i ? value : c)) });

  const ok = async () => {
    if (error) return;
    if (!changed) return close();
    setBusy(true);
    try {
      const done = await api.cmd({ op: target === "pcb" ? "set_board_page" : "set_schematic_page", page: paperToCmd(edited), title_block: afterTb });
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

  return (
    <div className="dialog-backdrop" onClick={close}>
      <div className="dialog" style={{ width: 640, maxHeight: "90vh" }} onClick={(e) => e.stopPropagation()} onKeyDown={(e) => e.key === "Enter" && (e.target as HTMLElement).tagName === "INPUT" && void ok()} role="dialog" aria-label="Page Settings">
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
            <div style={{ marginTop: 12 }}>
              <PageExample width={sketch.width} height={sketch.height} />
            </div>
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
    </div>
  );
}
