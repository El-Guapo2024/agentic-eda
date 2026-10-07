// Bulk Edit Symbol Library Links... (`eeschema.EditorControl.editSymbolLibraryLinks`, DIALOG_EDIT_SYMBOLS_LIBID): the schematic's symbols grouped by the
// library symbol they use, a "New Library Reference" cell per group, Map Orphans, and "Update symbol fields from new library". OK commits every changed
// group as one undo step (`set_symbol_lib_ids`).
import { useEffect, useMemo, useState } from "react";
import { useStudioApi, useStudioDispatch, useStudioState } from "../../state/store";
import { fetchSymbolEditorNames } from "../../api/client";
import { libIdIsValid, libLinkChanges, libLinkRows, orphanCandidates } from "../../kicad-port/libLinks";
import { SchDialogFrame } from "./SchDialogFrame";

export function LibLinksDialog({ onClose }: { onClose: () => void }) {
  const state = useStudioState();
  const dispatch = useStudioDispatch();
  const api = useStudioApi();
  const [names, setNames] = useState<string[]>([]);
  const [edits, setEdits] = useState<Record<string, string>>({});
  const [updateFields, setUpdateFields] = useState(false);
  const [note, setNote] = useState<string | null>(null);

  useEffect(() => {
    let alive = true;
    fetchSymbolEditorNames().then(
      (r) => alive && setNames(r.names),
      () => undefined
    );
    return () => {
      alive = false;
    };
  }, []);

  const known = useMemo(() => new Set(names), [names]);
  const rows = useMemo(() => libLinkRows(state.schematic?.symbols ?? [], known), [state.schematic, known]);
  const orphans = rows.filter((r) => r.orphan);
  const changes = libLinkChanges(rows, edits);
  const invalid = changes.find(([, to]) => !libIdIsValid(to));

  // Map Orphans: the first candidate fills the cell; with several, the cell shows the first and the rest are listed in the note.
  const mapOrphans = () => {
    const found = orphanCandidates(rows, names);
    const next = { ...edits };
    let fixed = 0;
    const lines: string[] = [];
    for (const [libId, candidates] of found) {
      if (candidates.length === 0) continue;
      next[libId] = candidates[0]!;
      fixed++;
      if (candidates.length > 1) lines.push(`${libId}: ${candidates.join(", ")}`);
    }
    setEdits(next);
    setNote(fixed < orphans.length ? `${fixed} link(s) mapped, ${orphans.length - fixed} not found` : `All ${fixed} link(s) resolved`);
    if (lines.length > 0) setNote((n) => `${n}. Several candidates, the first was taken (change it if needed) -- ${lines.join("; ")}`);
  };

  const submit = async () => {
    if (changes.length === 0) return onClose();
    if (invalid) return setNote(`Symbol library identifier ${invalid[1]} is not valid.`);
    if (await api.cmd({ op: "set_symbol_lib_ids", changes, update_fields: updateFields })) {
      dispatch({ type: "TOAST", message: `Changed ${changes.length} library link${changes.length === 1 ? "" : "s"}.`, kind: "info" });
      onClose();
    }
  };

  return (
    <SchDialogFrame
      title="Edit Symbol Library Links"
      width={760}
      onClose={onClose}
      footer={
        <button className="primary" onClick={() => void submit()}>
          OK
        </button>
      }
    >
      <datalist id="sch-lib-links-ids">
        {names.map((n) => (
          <option key={n} value={n} />
        ))}
      </datalist>
      <table style={{ width: "100%", borderCollapse: "collapse", fontSize: 12 }}>
        <thead>
          <tr style={{ textAlign: "left" }}>
            <th style={{ padding: "2px 6px", borderBottom: "1px solid var(--chrome-border)" }}>Symbols</th>
            <th style={{ padding: "2px 6px", borderBottom: "1px solid var(--chrome-border)" }}>Current Library Reference</th>
            <th style={{ padding: "2px 6px", borderBottom: "1px solid var(--chrome-border)" }}>New Library Reference</th>
          </tr>
        </thead>
        <tbody>
          {rows.map((r) => (
            <tr key={r.libId || "(none)"}>
              <td style={{ padding: "3px 6px", verticalAlign: "top", maxWidth: 220, wordBreak: "break-word" }}>{r.refs.join(", ")}</td>
              <td style={{ padding: "3px 6px", verticalAlign: "top", fontWeight: r.orphan ? 700 : undefined, fontStyle: r.orphan ? "italic" : undefined }} title={r.orphan ? "No library has this symbol" : undefined}>
                {r.libId || "(no library symbol)"}
              </td>
              <td style={{ padding: "3px 6px", verticalAlign: "top" }}>
                <input
                  list="sch-lib-links-ids"
                  style={{ width: "100%", boxSizing: "border-box" }}
                  value={edits[r.libId] ?? ""}
                  placeholder="Library:Symbol"
                  onChange={(e) => setEdits({ ...edits, [r.libId]: e.target.value })}
                />
              </td>
            </tr>
          ))}
          {rows.length === 0 && (
            <tr>
              <td colSpan={3} className="panel-empty">
                There are no symbols in this schematic.
              </td>
            </tr>
          )}
        </tbody>
      </table>
      <div style={{ display: "flex", alignItems: "center", gap: 12, marginTop: 10 }}>
        <button disabled={orphans.length === 0} onClick={mapOrphans} title="If some symbols are orphaned (the linked symbol is not found anywhere), try to find a candidate having the same name in one of the loaded symbol libraries.">
          Map Orphans
        </button>
        <label title='Replace the current symbol fields by fields from the new library. Warning: the fields "Value" and "Footprint" will be replaced.'>
          <input type="checkbox" checked={updateFields} onChange={(e) => setUpdateFields(e.target.checked)} /> Update symbol fields from new library
        </label>
      </div>
      {note && <div style={{ marginTop: 8, fontSize: 11, color: "var(--chrome-text-dim)" }}>{note}</div>}
    </SchDialogFrame>
  );
}
