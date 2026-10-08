// Export Symbols... (`eeschema.EditorControl.exportSymbolsToLibrary`, `SCH_EDITOR_CONTROL::ExportSymbolsToLibrary`): every library symbol the schematic uses, once, goes into
// a library -- here one `.kicad_sym` file the browser saves, named for the library asked for -- with the two options KiCad's library picker has: "Include power symbols
// in export" and "Update schematic symbols to link to exported symbols". The second one puts the exported symbols into the project's own library under the new
// nickname and moves the placed symbols' links to them, together, in one undo step of the schematic (the link verb edits the root sheet, so a sub-sheet's symbols are
// left as they are).
import { useState } from "react";
import { useStudioApi, useStudioDispatch, useStudioState } from "../../state/store";
import { postExportSymbols } from "../../api/schControlClient";
import { fetchAnySymbol, saveTextFile } from "../../api/libraryClient";
import type { Cmd } from "../../api/types";
import { isLibraryNickname } from "../../kicad-port/libLinks";
import { SchDialogFrame } from "./SchDialogFrame";

export function ExportSymbolsDialog({ onClose }: { onClose: () => void }) {
  const state = useStudioState();
  const dispatch = useStudioDispatch();
  const api = useStudioApi();
  const [name, setName] = useState(`${(state.board?.name || "board").replace(/[^A-Za-z0-9_.+-]/g, "_")}_symbols`);
  const [includePower, setIncludePower] = useState(false);
  const [relink, setRelink] = useState(false);
  const [busy, setBusy] = useState(false);
  const [notes, setNotes] = useState<string[]>([]);
  const nameOk = isLibraryNickname(name);
  const inSubSheet = state.currentSheetPath.length > 0;

  const submit = async () => {
    if (!nameOk || busy) return;
    setBusy(true);
    setNotes([]);
    try {
      const r = await postExportSymbols(includePower);
      if (!r.ok || r.text === undefined) {
        dispatch({ type: "TOAST", message: r.message ?? "Nothing to export.", kind: "error" });
        return;
      }
      const ids = r.ids ?? [];
      if (relink && !inSubSheet) {
        // The exported symbols become the project's entries of the new library (published at once -- they are copies of what the schematic already draws, and
        // kicad-cli resolves a placed symbol through the published ones). That is the library write, which KiCad does not undo either; the link change that
        // follows is the commit "Update Library Identifiers", one undo step of the schematic.
        const library: Cmd[] = [];
        for (const id of ids) {
          const { symbol } = await fetchAnySymbol(id);
          const linked = `${name}:${id.split(":").pop() ?? id}`;
          library.push({ op: "put_library_symbol", symbol: { ...symbol, lib_id: linked }, overwrite: true }, { op: "update_symbol_on_board", lib_id: linked });
        }
        if (!(await api.cmdBatch(library))) return; // the verb's refusal is already on screen
        const used = new Set((state.schematic?.symbols ?? []).map((s) => s.lib_id));
        const changes: Array<[string, string]> = ids.filter((id) => used.has(id)).map((id) => [id, `${name}:${id.split(":").pop() ?? id}`]);
        if (changes.length > 0 && !(await api.cmd({ op: "set_symbol_lib_ids", changes, update_fields: false }))) return;
      }
      saveTextFile(r.text, `${name}.kicad_sym`);
      const said = [`Exported ${ids.length} symbol${ids.length === 1 ? "" : "s"} to ${name}.kicad_sym.`, ...(r.clashes ?? []), ...(r.skipped ?? [])];
      if (relink && inSubSheet) said.push("The symbols were not relinked: go back to the root sheet first (a sub-sheet cannot be edited yet).");
      setNotes(said);
      dispatch({ type: "TOAST", message: said[0]!, kind: "info" });
      if (said.length === 1) onClose();
    } catch (e) {
      dispatch({ type: "TOAST", message: `Could not export the symbols: ${e instanceof Error ? e.message : String(e)}`, kind: "error" });
    } finally {
      setBusy(false);
    }
  };

  return (
    <SchDialogFrame
      title="Export Symbols"
      width={460}
      onClose={onClose}
      footer={
        <button className="primary" disabled={!nameOk || busy} onClick={() => void submit()}>
          {busy ? "Exporting..." : "Export"}
        </button>
      }
    >
      <div className="kv-grid" style={{ gridTemplateColumns: "160px 1fr" }}>
        <span title="The library's name: its nickname in the links of the symbols">Export symbols to library:</span>
        <input autoFocus value={name} onChange={(e) => setName(e.target.value)} onKeyDown={(e) => e.key === "Enter" && void submit()} style={nameOk ? undefined : { outline: "1px solid var(--chrome-error, #d9534f)" }} />
      </div>
      {!nameOk && <div style={{ fontSize: 11, color: "var(--chrome-text-dim)", marginTop: 4 }}>Use letters, digits and _ - . + only (no colon, no path).</div>}
      <label style={{ display: "block", marginTop: 12 }}>
        <input type="checkbox" checked={includePower} onChange={(e) => setIncludePower(e.target.checked)} /> Include power symbols in export
      </label>
      <label style={{ display: "block" }} title={inSubSheet ? "Go back to the root sheet to relink its symbols" : undefined}>
        <input type="checkbox" checked={relink} onChange={(e) => setRelink(e.target.checked)} /> Update schematic symbols to link to exported symbols
      </label>
      {notes.length > 0 && (
        <ul style={{ margin: "12px 0 0", paddingLeft: 18, fontSize: 11, color: "var(--chrome-text-dim)" }}>
          {notes.map((n, i) => (
            <li key={i}>{n}</li>
          ))}
        </ul>
      )}
    </SchDialogFrame>
  );
}
