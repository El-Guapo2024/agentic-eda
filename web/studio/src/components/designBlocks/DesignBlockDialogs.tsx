// Design Block Properties (`DIALOG_DESIGN_BLOCK_PROPERTIES`, common/dialogs/dialog_design_block_properties.cpp) and the overwrite question (`OKOrCancelDialog`) of the Design Blocks
// actions. The dialog: Name, Keywords, Description, and the "Default Fields" grid (Name and Value, with the add, move up, move down and delete buttons). Save as Design Block, Update
// Design Block from the current sheet, and Properties all open it; an update keeps the block's name (`aDisableName`). OK is refused when `TransferDataFromWindow` /
// `TransferDataFromGrid` would refuse it: a name with a character a library item may not have (or `/`), an empty one, two fields of one name.
import { useEffect, useRef, useState } from "react";
import { answered, hostMounted, useBlockQuestions, type BlockValues, type PropertiesQuestion } from "../../state/designBlockDialogs";
import { cleanFields, nameProblem } from "../../kicad-port/designBlocks";

export function DesignBlockDialogs() {
  useEffect(() => hostMounted(), []);
  const top = useBlockQuestions();
  if (!top) return null;
  if (top.kind === "properties") {
    return (
      <PropertiesDialog
        key={top.id}
        q={top.q}
        onDone={(v) => {
          top.resolve(v);
          answered(top.id);
        }}
      />
    );
  }
  const q = top.q;
  const done = (v: boolean) => {
    top.resolve(v);
    answered(top.id);
  };
  return (
    <div className="dialog-backdrop" onClick={() => done(false)}>
      <div className="dialog" style={{ width: 440 }} onClick={(e) => e.stopPropagation()} onKeyDown={(e) => e.key === "Escape" && done(false)} role="dialog" aria-label={q.title}>
        <div className="dialog-header">
          <span>{q.title}</span>
        </div>
        <div className="dialog-body" style={{ whiteSpace: "pre-wrap" }}>
          <div>{q.message}</div>
          {q.detail && <div style={{ marginTop: 8, color: "var(--chrome-text-dim)" }}>{q.detail}</div>}
        </div>
        <div className="dialog-footer">
          <button onClick={() => done(false)}>{q.cancelLabel ?? "Cancel"}</button>
          <button className="primary" autoFocus onClick={() => done(true)}>
            {q.okLabel}
          </button>
        </div>
      </div>
    </div>
  );
}

interface Row {
  name: string;
  value: string;
}

function PropertiesDialog({ q, onDone }: { q: PropertiesQuestion; onDone: (v: BlockValues | null) => void }) {
  const [name, setName] = useState(q.initial.name);
  const [keywords, setKeywords] = useState(q.initial.keywords);
  const [description, setDescription] = useState(q.initial.description);
  const [rows, setRows] = useState<Row[]>(q.initial.fields.map((f) => ({ ...f })));
  const [at, setAt] = useState<number | null>(null);
  const [problem, setProblem] = useState<string | null>(null);
  const nameRef = useRef<HTMLInputElement>(null);
  useEffect(() => {
    (q.nameLocked ? undefined : nameRef.current)?.focus();
  }, [q.nameLocked]);

  const ok = () => {
    // `TransferDataFromWindow`
    const bad = nameProblem(name);
    if (bad) return setProblem(bad);
    // `TransferDataFromGrid`
    const cleaned = cleanFields(rows);
    if ("problem" in cleaned) return setProblem(cleaned.problem);
    onDone({ name, description, keywords, fields: cleaned.fields });
  };
  const edit = (i: number, patch: Partial<Row>) => {
    setProblem(null);
    setRows((now) => now.map((r, k) => (k === i ? { ...r, ...patch } : r)));
  };
  const swap = (i: number, j: number) => {
    if (j < 0 || j >= rows.length) return;
    setRows((now) => {
      const out = [...now];
      [out[i], out[j]] = [out[j] as Row, out[i] as Row];
      return out;
    });
    setAt(j);
  };

  return (
    <div className="dialog-backdrop" onClick={() => onDone(null)}>
      <div className="dialog" style={{ width: 520, maxWidth: "94vw" }} onClick={(e) => e.stopPropagation()} onKeyDown={(e) => e.key === "Escape" && onDone(null)} role="dialog" aria-label="Design Block Properties">
        <div className="dialog-header">
          <span>Design Block Properties</span>
        </div>
        <div className="dialog-body" style={{ maxHeight: "74vh", overflow: "auto" }}>
          <fieldset style={{ border: "1px solid var(--chrome-border)", margin: 0, padding: "6px 8px 8px" }}>
            <legend style={{ fontSize: 11, color: "var(--chrome-text-dim)" }}>Default Fields</legend>
            <div style={{ border: "1px solid var(--chrome-border)", maxHeight: 150, overflow: "auto" }} data-testid="db-fields">
              <table style={{ width: "100%", borderCollapse: "collapse", fontSize: 12 }} aria-label="Default fields">
                <thead>
                  <tr>
                    <th style={{ textAlign: "left", padding: "2px 6px", width: "40%" }}>Name</th>
                    <th style={{ textAlign: "left", padding: "2px 6px" }}>Value</th>
                  </tr>
                </thead>
                <tbody>
                  {rows.map((r, i) => (
                    <tr key={i} className={at === i ? "selected" : undefined} style={at === i ? { background: "var(--chrome-selected-bg)" } : undefined} onMouseDown={() => setAt(i)}>
                      <td>
                        <input aria-label={`Name of field ${i + 1}`} value={r.name} spellCheck={false} style={{ width: "100%", boxSizing: "border-box" }} onChange={(e) => edit(i, { name: e.target.value })} onFocus={() => setAt(i)} />
                      </td>
                      <td>
                        <input aria-label={`Value of field ${i + 1}`} value={r.value} spellCheck={false} style={{ width: "100%", boxSizing: "border-box" }} onChange={(e) => edit(i, { value: e.target.value })} onFocus={() => setAt(i)} />
                      </td>
                    </tr>
                  ))}
                  {rows.length === 0 && (
                    <tr>
                      <td colSpan={2} style={{ padding: "6px 8px", color: "var(--chrome-text-dim)" }}>
                        No default fields.
                      </td>
                    </tr>
                  )}
                </tbody>
              </table>
            </div>
            <div style={{ display: "flex", gap: 6, marginTop: 6 }}>
              <button
                title="Add field"
                aria-label="Add field"
                onClick={() => {
                  setRows((now) => [...now, { name: "Untitled Field", value: "" }]);
                  setAt(rows.length);
                }}
              >
                {"+"}
              </button>
              <button title="Move up" aria-label="Move up" disabled={at === null || at <= 0} onClick={() => at !== null && swap(at, at - 1)}>
                {"↑"}
              </button>
              <button title="Move down" aria-label="Move down" disabled={at === null || at >= rows.length - 1} onClick={() => at !== null && swap(at, at + 1)}>
                {"↓"}
              </button>
              <button
                title="Delete field"
                aria-label="Delete field"
                disabled={at === null}
                onClick={() => {
                  if (at === null) return;
                  setRows((now) => now.filter((_, k) => k !== at));
                  setAt(null);
                }}
              >
                {"🗑"}
              </button>
            </div>
          </fieldset>
          <div className="kv-grid" style={{ gridTemplateColumns: "90px 1fr", marginTop: 10 }}>
            <label htmlFor="db-name">Name:</label>
            <input
              id="db-name"
              ref={nameRef}
              value={name}
              readOnly={q.nameLocked}
              spellCheck={false}
              onChange={(e) => {
                setProblem(null);
                setName(e.target.value);
              }}
              onKeyDown={(e) => e.key === "Enter" && ok()}
            />
            <label htmlFor="db-keywords">Keywords:</label>
            <input id="db-keywords" value={keywords} spellCheck={false} onChange={(e) => setKeywords(e.target.value)} onKeyDown={(e) => e.key === "Enter" && ok()} />
            <label htmlFor="db-description" style={{ alignSelf: "start" }}>
              Description:
            </label>
            <textarea id="db-description" rows={4} value={description} spellCheck={false} onChange={(e) => setDescription(e.target.value)} />
          </div>
          <div className="lt-note" style={{ marginTop: 8 }}>
            {q.nameLocked ? `Updating a block of library '${q.lib}'.` : `The block is kept in library '${q.lib}'.`}
          </div>
          {problem && (
            <div role="alert" style={{ color: "#ff6666", marginTop: 8 }}>
              {problem}
            </div>
          )}
        </div>
        <div className="dialog-footer">
          <button onClick={() => onDone(null)}>Cancel</button>
          <button className="primary" onClick={ok}>
            {q.okLabel ?? "OK"}
          </button>
        </div>
      </div>
    </div>
  );
}
