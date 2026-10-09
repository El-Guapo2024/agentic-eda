// The Checker window's list of markers -- what `RC_TREE_MODEL` (common/rc_item.cpp) draws in the DRC and ERC dialogs: one block for each marker, its
// own line in bold ("Error: Clearance violation ..."), the items it names under it, and an exclusion's comment last. An excluded marker is dim and italic
// (`GetAttr`: "Strikethrough would be better, if wxWidgets supported it"). A click selects the marker (the dialog cross-probes it); a right click opens its
// menu (`OnDRCItemRClick` / `OnERCItemRClick`).
//
// Pure presentation: the dialogs build the rows (components/DrcDialog.tsx, ErcDialog.tsx) and decide what a click and a menu do.
import type { RcKind } from "../kicad-port/rcItems";

export interface RcRow {
  /** The marker's index in its report (what a click and a menu name). */
  index: number;
  kind: RcKind;
  /** "Error: ", "Warning: ", "Excluded error: " ... (`markerPrefix`). */
  prefix: string;
  message: string;
  /** The items the marker names, one line each. */
  items: string[];
  /** An exclusion's comment. */
  comment: string;
  selected: boolean;
  /** Our own findings (the Lint pages) look different and have no menu. */
  lint?: boolean;
  /** Offer the row's Exclude / Remove button. */
  canToggle: boolean;
  /** Tooltip of the marker's line (the check's name, and anything the row has to say about itself). */
  title?: string;
  /** A line of its own under the items (a lint finding's suggested fix). */
  extra?: string;
}

export function RcList({ rows, onSelect, onMenu, onToggle }: { rows: readonly RcRow[]; onSelect: (index: number) => void; onMenu?: (index: number, x: number, y: number) => void; onToggle?: (index: number) => void }) {
  return (
    <div className="rc-list" role="list">
      {rows.map((r) => (
        <div
          key={`${r.lint ? "l" : "m"}${r.index}`}
          role="listitem"
          data-rc-index={r.index}
          className={`problem-row${r.kind === "warning" ? " warn" : ""}${r.kind === "exclusion" ? " excluded" : ""}${r.lint ? " lint" : ""}${r.selected ? " selected" : ""}`}
          onClick={() => onSelect(r.index)}
          onContextMenu={
            onMenu && !r.lint
              ? (e) => {
                  e.preventDefault();
                  onMenu(r.index, e.clientX, e.clientY);
                }
              : undefined
          }
        >
          <div className="rc-head">
            <span className="rc-title" title={r.title}>
              {r.prefix}
              {r.message}
            </span>
            {r.canToggle && onToggle && (
              <button
                className="rc-button"
                title={r.kind === "exclusion" ? "Remove exclusion: report this violation again" : "Exclude: stop reporting this violation"}
                onClick={(e) => {
                  e.stopPropagation();
                  onToggle(r.index);
                }}
              >
                {r.kind === "exclusion" ? "Un-exclude" : "Exclude"}
              </button>
            )}
          </div>
          {r.items.map((it, i) => (
            <small key={i} className="rc-item">
              {it}
            </small>
          ))}
          {r.comment !== "" && <small className="rc-comment">{r.comment}</small>}
          {r.extra && (
            <small style={{ color: "var(--chrome-accent, #4ea1ff)" }}>{r.extra}</small>
          )}
        </div>
      ))}
    </div>
  );
}
