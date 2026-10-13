// The reports of Inspect > Clearance Resolution and Constraints Resolution (`DIALOG_BOOK_REPORTER`, pcbnew/dialogs/dialog_book_reporter.cpp): a notebook with a page per question the
// rules answer, each page the lines of an HTML report box in KiCad's wording. The "Clearance" page of items that share several copper layers has a "Layer:" choice over the report of
// each. Like KiCad's the dialog is modeless and only hidden when closed; a new report replaces its pages. What the pages say is the server's (crates/drc/src/inspect.rs), the lines are read
// by kicad-port/inspectReport.ts without ever being given to the DOM as markup, and what opens the dialog is actions/inspectActions.ts. The link "Show design rules." of a report that
// could not compile the custom rules opens Board Setup on its Custom Rules page.
import type { ReactNode } from "react";
import { useStudioDispatch, useStudioState } from "../state/store";
import { setInspectReport, useInspectReport, type InspectKind } from "../state/inspectReports";
import { pageLines, parseReportHtml, type HtmlNode } from "../kicad-port/inspectReport";

function renderNodes(nodes: readonly HtmlNode[], onLink: (href: string) => void, path = ""): ReactNode[] {
  return nodes.map((n, i) => {
    const key = `${path}${i}`;
    if (typeof n === "string") return n;
    const kids = renderNodes(n.children, onLink, `${key}.`);
    switch (n.tag) {
      case "h7":
        return (
          <div key={key} style={{ fontWeight: 700, fontSize: "0.92em" }}>
            {kids}
          </div>
        );
      case "ul":
        return (
          <ul key={key} style={{ margin: "2px 0 2px 18px", padding: 0 }}>
            {kids}
          </ul>
        );
      case "li":
        return <li key={key}>{kids}</li>;
      case "b":
        return <strong key={key}>{kids}</strong>;
      case "i":
        return <em key={key}>{kids}</em>;
      case "br":
        return <br key={key} />;
      case "a":
        return (
          <a
            key={key}
            href="#"
            onClick={(e) => {
              e.preventDefault();
              if (n.href) onLink(n.href);
            }}
          >
            {kids}
          </a>
        );
    }
  });
}

const TITLES: Record<InspectKind, string> = { clearance: "Clearance Report", constraints: "Constraints Report" };

export function InspectReportDialog({ kind }: { kind: InspectKind }) {
  const r = useInspectReport(kind);
  const state = useStudioState();
  const dispatch = useStudioDispatch();
  if (!r.open || !r.reply || state.tab !== "pcb") return null;
  const reply = r.reply;
  const page = reply.pages[Math.min(r.page, reply.pages.length - 1)];
  const close = () => setInspectReport(kind, { open: false });
  const layerIndex = r.layerChoice[r.page] ?? 0;
  const lines = page ? pageLines(page, layerIndex) : [];
  // `$CUSTOM_RULES`: "Show design rules." -- Board Setup on its Custom Rules page.
  const onLink = (href: string) => {
    if (href === "$CUSTOM_RULES") {
      dispatch({ type: "SET_BOARD_SETUP_INITIAL_PAGE", page: "custom_rules" });
      dispatch({ type: "SET_BOARD_SETUP_DIALOG_OPEN", open: true });
    }
  };
  return (
    <div
      className="dialog"
      role="dialog"
      aria-label={TITLES[kind]}
      data-testid={`inspect-${kind}`}
      style={{ position: "fixed", top: 110, left: "50%", transform: "translateX(-50%)", width: "min(680px, calc(100vw - 24px))", maxHeight: "78vh", zIndex: 1900, display: "flex", flexDirection: "column" }}
      onKeyDown={(e) => {
        if (e.key === "Escape") close();
      }}
    >
      <div className="dialog-header">
        <span>{reply.title || TITLES[kind]}</span>
        <button aria-label="Close" onClick={close} style={{ border: "none", background: "transparent", cursor: "pointer", color: "inherit" }}>
          &times;
        </button>
      </div>
      <div className="dialog-body" style={{ padding: "8px 12px", display: "flex", flexDirection: "column", gap: 8, minHeight: 0 }}>
        <div role="tablist" style={{ display: "flex", flexWrap: "wrap", gap: 2, borderBottom: "1px solid var(--chrome-border)" }}>
          {reply.pages.map((p, i) => (
            <button
              key={`${p.title}-${i}`}
              role="tab"
              data-testid={`inspect-tab-${p.title}`}
              aria-selected={i === r.page}
              onClick={() => setInspectReport(kind, { page: i })}
              style={{ borderBottomLeftRadius: 0, borderBottomRightRadius: 0, fontWeight: i === r.page ? 700 : 400, background: i === r.page ? "var(--chrome-active)" : undefined }}
            >
              {p.title}
            </button>
          ))}
        </div>
        {page && page.per_layer.length > 0 && (
          <label style={{ display: "flex", alignItems: "center", gap: 8 }}>
            <span>Layer:</span>
            <select
              data-testid="inspect-layer-choice"
              value={layerIndex}
              onChange={(e) => {
                const choice = [...r.layerChoice];
                choice[r.page] = Number(e.target.value);
                setInspectReport(kind, { layerChoice: choice });
              }}
              style={{ flex: 1 }}
            >
              {page.per_layer.map((l, i) => (
                <option key={l.layer} value={i}>
                  {l.name}
                </option>
              ))}
            </select>
          </label>
        )}
        <div
          data-testid="inspect-lines"
          style={{ overflow: "auto", border: "1px solid var(--chrome-border)", borderRadius: 3, padding: "6px 8px", minHeight: 160, flex: 1, fontSize: 13, lineHeight: 1.45, background: "var(--chrome-panel, transparent)" }}
        >
          {lines.map((l, i) => (
            <div key={i} data-testid="inspect-line" style={{ minHeight: "1.2em" }}>
              {renderNodes(parseReportHtml(l), onLink)}
            </div>
          ))}
        </div>
        <div style={{ display: "flex", justifyContent: "flex-end", gap: 8 }}>
          <button onClick={close} data-testid="inspect-close">
            Close
          </button>
        </div>
      </div>
    </div>
  );
}
