// `wxGetSingleChoiceIndex( message, caption, list, initialSelection )` / `wxGetSingleChoice`, as the Calculator Tools' "..." buttons use it to offer
// a list of material constants (kicad-port/calc/commonData.ts): a modal list titled by the caption, OK and Cancel, a double click choosing a row. The
// entries are "<value> \t<name>"; the tab is shown as a gap.
import { useCallback, useRef, useState, type ReactElement } from "react";

interface Request {
  caption: string;
  list: readonly string[];
  initial: number;
  resolve: (index: number) => void;
}

/** The dialog's element and an `ask( caption, list, initial )` answering the index chosen, or -1 for Cancel (wxGetSingleChoiceIndex's -1). */
export function useSingleChoice(testid: string): [ReactElement | null, (caption: string, list: readonly string[], initial?: number) => Promise<number>] {
  const [req, setReq] = useState<Request | null>(null);
  const [sel, setSel] = useState(0);
  const listRef = useRef<HTMLDivElement>(null);
  const ask = useCallback(
    (caption: string, list: readonly string[], initial = 0) =>
      new Promise<number>((resolve) => {
        setSel(initial);
        setReq({ caption, list, initial, resolve });
      }),
    []
  );
  if (!req) return [null, ask];
  const done = (index: number) => {
    req.resolve(index);
    setReq(null);
  };
  const el = (
    <div className="dialog-backdrop" style={{ zIndex: 2300 }} onClick={() => done(-1)}>
      <div className="dialog" style={{ width: 360 }} role="dialog" aria-label={req.caption} data-testid={testid} onClick={(e) => e.stopPropagation()}>
        <div className="dialog-header">
          <span>{req.caption}</span>
        </div>
        <div
          className="dialog-body"
          ref={listRef}
          tabIndex={0}
          role="listbox"
          style={{ maxHeight: 320, overflowY: "auto", padding: 0 }}
          onKeyDown={(e) => {
            if (e.key === "ArrowDown") setSel((s) => Math.min(req.list.length - 1, s + 1));
            else if (e.key === "ArrowUp") setSel((s) => Math.max(0, s - 1));
            else if (e.key === "Enter") done(sel);
            else if (e.key === "Escape") done(-1);
            else return;
            e.preventDefault();
            e.stopPropagation();
          }}
        >
          {req.list.map((entry, i) => {
            const [value, name] = entry.split("\t");
            return (
              <div
                key={i}
                role="option"
                aria-selected={i === sel}
                data-choice-index={i}
                onClick={() => setSel(i)}
                onDoubleClick={() => done(i)}
                style={{
                  padding: "2px 8px",
                  minHeight: 18,
                  cursor: "default",
                  whiteSpace: "pre",
                  background: i === sel ? "var(--chrome-selected-bg)" : undefined,
                  color: i === sel ? "var(--chrome-selected-text)" : undefined,
                }}
              >
                <span style={{ display: "inline-block", minWidth: 70 }}>{value}</span>
                {name ?? ""}
              </div>
            );
          })}
        </div>
        <div className="dialog-footer">
          <button onClick={() => done(-1)}>Cancel</button>
          <button className="primary" data-testid={`${testid}-ok`} onClick={() => done(sel)}>
            OK
          </button>
        </div>
      </div>
    </div>
  );
  return [el, ask];
}
