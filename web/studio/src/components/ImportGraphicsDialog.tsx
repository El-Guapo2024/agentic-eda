// Import Graphics (`DIALOG_IMPORT_GRAPHICS`, pcbnew/import_gfx/dialog_import_graphics.cpp; `DIALOG_IMPORT_GFX_SCH` is its schematic twin, used by the schematic editor and the Symbol Editor): a DXF
// or SVG file, the import scale, the DXF's default line width and units, and where it goes. The board's dialog also has the layer, "Group imported items" and "Fix discontinuities" with its
// tolerance; the schematic's and the symbol editor's has "Interactive placement" or "At" X, Y and nothing else.
//
// A browser gives the page a file's text and not its path, so "File:" is filled by a Browse... button (a file chooser) or by a file dropped on the dialog; the page reads it, and
// the server says what it holds (lines, arcs, ...; what it could not handle) before OK. OK sends the one command (crates/ops/src/import_graphics.rs, import_graphics_sch.rs): with "Place at"
// off the items are put at the cursor and carried by the Move tool until a click drops them (Escape takes them back), as a Paste is in this studio. The Symbol Editor has no carried move:
// its items are added at the cursor and selected, to be dragged.
import { useEffect, useRef, useState } from "react";
import { useStudioApi, useStudioDispatch, useStudioState } from "../state/store";
import { useSymApi, useSymDispatch, useSymState } from "../state/symbolEditorStore";
import { fetchLibrarySymbol } from "../api/client";
import { closeImportGraphics, openImportGraphics, useImportGraphicsDialog, type PickedFile } from "../state/importDialogs";
import { DialogShell } from "./pcbDialogKit";
import { pickTextFile } from "../api/libraryClient";
import { readGraphics } from "../api/importClient";
import { DXF_UNITS, buildImportCmd, buildSchImportCmd, defaultImportValues, defaultSchImportValues, describeInfo, graphicsFormatOf, importLayers, type GraphicsInfo, type ImportValues, type SchImportValues } from "../kicad-port/importGraphics";

/** The dialog keeps what it was last set to while the page lives (KiCad saves a dialog's controls between runs). */
let lastValues: ImportValues | null = null;
let lastSchValues: SchImportValues | null = null;

export function ImportGraphicsDialog() {
  const dialog = useImportGraphicsDialog();
  if (!dialog) return null;
  if (dialog.editor === "pcb") return <PcbImportGraphics file={dialog.file} />;
  if (dialog.editor === "schematic" || dialog.editor === "symbol") return <SchImportGraphics editor={dialog.editor} file={dialog.file} />;
  return null;
}

function PcbImportGraphics({ file }: { file: PickedFile | null }) {
  const state = useStudioState();
  const dispatch = useStudioDispatch();
  const api = useStudioApi();
  const units = state.units;
  const [values, setValues] = useState<ImportValues>(() => lastValues ?? { ...defaultImportValues(units), layer: state.activeLayer ?? "Dwgs.User" });
  const [info, setInfo] = useState<GraphicsInfo | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [dragging, setDragging] = useState(false);
  const request = useRef(0);
  const set = (patch: Partial<ImportValues>) => setValues((v) => ({ ...v, ...patch }));

  const format = file ? graphicsFormatOf(file.name) : null;
  const isDxf = format === "dxf";
  const scale = Number(values.scale.trim());

  // What the file holds, read again when the scale or the DXF's unit changes (the size check depends on both).
  useEffect(() => {
    if (!file || !format) {
      setInfo(null);
      return;
    }
    const mine = ++request.current;
    void readGraphics({ format, text: file.text, dxf_units: isDxf ? values.dxfUnits : "", scale: Number.isFinite(scale) && scale > 0 ? scale : 1 })
      .then((i) => request.current === mine && setInfo(i))
      .catch((e) => request.current === mine && setInfo({ ok: false, message: e instanceof Error ? e.message : String(e) }));
  }, [file, format, isDxf, values.dxfUnits, scale]);

  const layers = importLayers(state.board?.layers ?? ["F.Cu", "B.Cu"]);
  const polygonOutline = !!state.board && !state.board.outline_is_shapes;
  const onEdge = values.setLayer && values.layer === "Edge.Cuts";

  const accept = (f: PickedFile | null) => {
    setError(null);
    if (!f) return;
    if (!graphicsFormatOf(f.name)) {
      setError("There is no plugin to handle this file type.");
      return;
    }
    openImportGraphics("pcb", f);
  };
  const browse = async () => {
    try {
      accept(await pickTextFile(".dxf,.svg,.DXF,.SVG"));
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  };

  const close = () => {
    lastValues = values;
    closeImportGraphics();
  };

  const ok = async () => {
    if (!file) {
      setError("Please select a file to import.");
      return;
    }
    if (!format) {
      setError("There is no plugin to handle this file type.");
      return;
    }
    const built = buildImportCmd({ format, text: file.text, values, units, activeLayer: state.activeLayer, info, polygonOutline });
    if (!built.ok) {
      setError(built.error);
      return;
    }
    if (info && info.ok && info.fits === false) {
      setError(info.messages?.find((m) => m.text.startsWith("Imported graphic is too large"))?.text ?? "The graphic is too large.");
      return;
    }
    setBusy(true);
    lastValues = values;
    const placed = await api.placeNewItems((at) => built.build(at), built.interactive);
    setBusy(false);
    if (!placed) return; // the refusal is on screen as a toast
    // `HTML_MESSAGE_BOX`: "Items in the imported file could not be handled properly."
    const warnings = (info?.messages ?? []).map((m) => m.text);
    if (warnings.length > 0) dispatch({ type: "TOAST", message: `Items in the imported file could not be handled properly. ${warnings.join(" ")}`, kind: "info" });
    closeImportGraphics();
  };

  const row = { display: "flex", alignItems: "center", gap: 8, fontSize: 12, marginBottom: 6 } as const;
  const label = { minWidth: 120 } as const;

  return (
    <DialogShell title="Import Graphics" width={460} onCancel={close} onOk={ok} okDisabled={busy || !file}>
      <div
        onDragOver={(e) => {
          if (e.dataTransfer?.types.includes("Files")) {
            e.preventDefault();
            setDragging(true);
          }
        }}
        onDragLeave={() => setDragging(false)}
        onDrop={async (e) => {
          setDragging(false);
          const f = e.dataTransfer?.files?.[0];
          if (!f) return;
          e.preventDefault();
          e.stopPropagation();
          accept({ name: f.name, text: await f.text() });
        }}
        style={{ outline: dragging ? "2px dashed var(--accent, #4a9eff)" : "none", outlineOffset: 4 }}
        data-testid="import-graphics"
      >
        <div style={row}>
          <span style={label} title="Only vectors will be imported.  Bitmaps and fonts will be ignored.">
            File:
          </span>
          <input readOnly aria-label="File" style={{ flex: 1 }} value={file?.name ?? ""} placeholder="Choose or drop a .dxf or .svg file" onClick={browse} />
          <button onClick={browse}>Browse...</button>
        </div>
        {file && info && (
          <div role="status" aria-label="File contents" style={{ fontSize: 12, marginBottom: 8, opacity: 0.85 }}>
            {describeInfo(info)}
            {(info.messages ?? []).map((m) => (
              <div key={m.text} style={{ color: "var(--warn, #c90)" }}>
                {m.text}
                {m.count > 1 ? ` (${m.count})` : ""}
              </div>
            ))}
          </div>
        )}

        <label style={row}>
          <span style={label}>Import scale:</span>
          <input aria-label="Import scale" style={{ width: 120 }} value={values.scale} onChange={(e) => set({ scale: e.target.value })} />
        </label>
        <label style={{ ...row, opacity: isDxf || !file ? 1 : 0.5 }} title="Used when the DXF items in file have no line thickness set">
          <span style={label}>DXF default line width:</span>
          <input aria-label="DXF default line width" disabled={!!file && !isDxf} style={{ width: 120 }} value={values.lineWidth} onChange={(e) => set({ lineWidth: e.target.value })} />
          <span>{units}</span>
        </label>
        <label style={{ ...row, opacity: isDxf || !file ? 1 : 0.5 }} title="Used when the DXF file has no unit set">
          <span style={label}>DXF default units:</span>
          <select aria-label="DXF default units" disabled={!!file && !isDxf} value={values.dxfUnits} onChange={(e) => set({ dxfUnits: e.target.value })}>
            {DXF_UNITS.map((u) => (
              <option key={u.id} value={u.id}>
                {u.label}
              </option>
            ))}
          </select>
        </label>

        <div style={row} title="If not checked: use interactive placement.">
          <label style={{ display: "flex", alignItems: "center", gap: 6, minWidth: 120 }}>
            <input type="checkbox" aria-label="Place at" checked={values.placeAt} onChange={(e) => set({ placeAt: e.target.checked })} />
            Place at:
          </label>
          <span>X:</span>
          <input aria-label="Place at X" disabled={!values.placeAt} style={{ width: 90 }} value={values.x} onChange={(e) => set({ x: e.target.value })} />
          <span>Y:</span>
          <input aria-label="Place at Y" disabled={!values.placeAt} style={{ width: 90 }} value={values.y} onChange={(e) => set({ y: e.target.value })} />
          <span>{units}</span>
        </div>
        <div style={row} title="If checked, use the selected layer in this dialog. If unchecked, use the Board Editor active layer.">
          <label style={{ display: "flex", alignItems: "center", gap: 6, minWidth: 120 }}>
            <input type="checkbox" aria-label="Set layer" checked={values.setLayer} onChange={(e) => set({ setLayer: e.target.checked })} />
            Layer:
          </label>
          <select aria-label="Layer" disabled={!values.setLayer} value={values.layer} onChange={(e) => set({ layer: e.target.value })}>
            {layers.map((l) => (
              <option key={l} value={l}>
                {l}
              </option>
            ))}
          </select>
        </div>
        <label style={row} title="Add all imported items to a new group">
          <input type="checkbox" aria-label="Group imported items" checked={values.group} onChange={(e) => set({ group: e.target.checked })} />
          Group imported items
        </label>
        <div style={row} title="Trim/extend open shapes or add segments to make vertices of shapes coincide">
          <label style={{ display: "flex", alignItems: "center", gap: 6, minWidth: 120 }}>
            <input type="checkbox" aria-label="Fix discontinuities" checked={values.fix} onChange={(e) => set({ fix: e.target.checked })} />
            Fix discontinuities
          </label>
          <span>Tolerance:</span>
          <input aria-label="Tolerance" disabled={!values.fix} style={{ width: 90 }} value={values.tolerance} onChange={(e) => set({ tolerance: e.target.value })} />
          <span>{units}</span>
        </div>
        {onEdge && polygonOutline && (
          <label style={row} title="The board's outline here is the polygon of its intent, which no tool can select or delete: with this checked the imported Edge.Cuts become the outline in its place.">
            <input type="checkbox" aria-label="Use as the board outline" checked={values.replaceOutline} onChange={(e) => set({ replaceOutline: e.target.checked })} />
            Use as the board outline (replaces the outline polygon)
          </label>
        )}
        {error && (
          <div role="alert" style={{ color: "var(--error, #d33)", fontSize: 12 }}>
            {error}
          </div>
        )}
      </div>
    </DialogShell>
  );
}

/** `DIALOG_IMPORT_GFX_SCH` for the schematic editor (`GRAPHICS_IMPORTER_SCH`) and the Symbol Editor (`GRAPHICS_IMPORTER_LIB_SYMBOL`). */
function SchImportGraphics({ editor, file }: { editor: "schematic" | "symbol"; file: PickedFile | null }) {
  const state = useStudioState();
  const dispatch = useStudioDispatch();
  const api = useStudioApi();
  const sym = useSymState();
  const symApi = useSymApi();
  const symDispatch = useSymDispatch();
  const units = state.units;
  const [values, setValues] = useState<SchImportValues>(() => lastSchValues ?? defaultSchImportValues());
  const [info, setInfo] = useState<GraphicsInfo | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [dragging, setDragging] = useState(false);
  const request = useRef(0);
  const set = (patch: Partial<SchImportValues>) => setValues((v) => ({ ...v, ...patch }));

  const format = file ? graphicsFormatOf(file.name) : null;
  const isDxf = format === "dxf";
  const scale = Number(values.scale.trim());

  useEffect(() => {
    if (!file || !format) {
      setInfo(null);
      return;
    }
    const mine = ++request.current;
    void readGraphics({ format, text: file.text, dxf_units: isDxf ? values.dxfUnits : "", scale: Number.isFinite(scale) && scale > 0 ? scale : 1 })
      .then((i) => request.current === mine && setInfo(i))
      .catch((e) => request.current === mine && setInfo({ ok: false, message: e instanceof Error ? e.message : String(e) }));
  }, [file, format, isDxf, values.dxfUnits, scale]);

  const accept = (f: PickedFile | null) => {
    setError(null);
    if (!f) return;
    if (!graphicsFormatOf(f.name)) {
      setError("There is no plugin to handle this file type.");
      return;
    }
    openImportGraphics(editor, f);
  };
  const browse = async () => {
    try {
      accept(await pickTextFile(".dxf,.svg,.DXF,.SVG"));
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  };
  const close = () => {
    lastSchValues = values;
    closeImportGraphics();
  };

  const ok = async () => {
    if (!file) {
      setError("No file selected!");
      return;
    }
    if (!format) {
      setError("There is no plugin to handle this file type.");
      return;
    }
    const symbol = editor === "symbol" && sym.libId ? { libId: sym.libId, unit: sym.activeUnit, bodyStyle: sym.activeBodyStyle } : undefined;
    const built = buildSchImportCmd({ editor, format, text: file.text, values, units, symbol });
    if (!built.ok) {
      setError(built.error);
      return;
    }
    if (info && info.ok && info.fits === false) {
      setError(info.messages?.find((m) => m.text.startsWith("Imported graphic is too large"))?.text ?? "The graphic is too large.");
      return;
    }
    setBusy(true);
    lastSchValues = values;
    let placed: boolean;
    if (editor === "schematic") {
      placed = await api.placeNewSchItems((at) => built.build(at), built.interactive);
    } else {
      // The Symbol Editor: the items go to the grid point under the cursor, and are selected once they are there.
      const st = symApi.getState();
      const grid = st.gridUm > 0 ? st.gridUm : 1270;
      const cursor = st.cursorUm && Number.isFinite(st.cursorUm.x) && Number.isFinite(st.cursorUm.y) ? st.cursorUm : null;
      const at = cursor ? { x: Math.round(cursor.x / grid) * grid, y: Math.round(cursor.y / grid) * grid } : { x: 0, y: 0 };
      const known = new Set((st.symbol?.graphics ?? []).map((g) => g.id));
      placed = await symApi.cmd(built.build(built.interactive ? at : null));
      if (placed && symbol) {
        const after = await fetchLibrarySymbol(symbol.libId).catch(() => null);
        const added = (after?.graphics ?? []).map((g) => g.id).filter((id): id is string => !!id && !known.has(id));
        if (added.length > 0) symDispatch({ type: "SET_SELECTION", refs: added });
      }
    }
    setBusy(false);
    if (!placed) return; // the refusal is on screen as a toast
    // `HTML_MESSAGE_BOX`: "Items in the imported file could not be handled properly."
    const warnings = (info?.messages ?? []).map((m) => m.text);
    if (warnings.length > 0) dispatch({ type: "TOAST", message: `Items in the imported file could not be handled properly. ${warnings.join(" ")}`, kind: "info" });
    closeImportGraphics();
  };

  const row = { display: "flex", alignItems: "center", gap: 8, fontSize: 12, marginBottom: 6 } as const;
  const label = { minWidth: 120 } as const;

  return (
    <DialogShell title="Import Graphics" width={460} onCancel={close} onOk={ok} okDisabled={busy || !file}>
      <div
        onDragOver={(e) => {
          if (e.dataTransfer?.types.includes("Files")) {
            e.preventDefault();
            setDragging(true);
          }
        }}
        onDragLeave={() => setDragging(false)}
        onDrop={async (e) => {
          setDragging(false);
          const f = e.dataTransfer?.files?.[0];
          if (!f) return;
          e.preventDefault();
          e.stopPropagation();
          accept({ name: f.name, text: await f.text() });
        }}
        style={{ outline: dragging ? "2px dashed var(--accent, #4a9eff)" : "none", outlineOffset: 4 }}
        data-testid="import-graphics"
      >
        <div style={row}>
          <span style={label} title="Only vectors will be imported.  Bitmaps and fonts will be ignored.">
            File:
          </span>
          <input readOnly aria-label="File" style={{ flex: 1 }} value={file?.name ?? ""} placeholder="Choose or drop a .dxf or .svg file" onClick={browse} />
          <button onClick={browse}>Browse...</button>
        </div>
        {file && info && (
          <div role="status" aria-label="File contents" style={{ fontSize: 12, marginBottom: 8, opacity: 0.85 }}>
            {describeInfo(info)}
            {(info.messages ?? []).map((m) => (
              <div key={m.text} style={{ color: "var(--warn, #c90)" }}>
                {m.text}
                {m.count > 1 ? ` (${m.count})` : ""}
              </div>
            ))}
          </div>
        )}

        <fieldset style={{ border: "1px solid var(--border, #8884)", marginBottom: 8, padding: "6px 8px" }}>
          <legend style={{ fontSize: 12 }}>Placement</legend>
          <label style={row}>
            <input type="radio" name="import-placement" aria-label="Interactive placement" checked={!values.placeAt} onChange={() => set({ placeAt: false })} />
            Interactive placement
          </label>
          <div style={row}>
            <label style={{ display: "flex", alignItems: "center", gap: 6, minWidth: 120 }}>
              <input type="radio" name="import-placement" aria-label="At" checked={values.placeAt} onChange={() => set({ placeAt: true })} />
              At:
            </label>
            <span>X:</span>
            <input aria-label="At X" disabled={!values.placeAt} style={{ width: 90 }} value={values.x} onChange={(e) => set({ x: e.target.value })} />
            <span>Y:</span>
            <input aria-label="At Y" disabled={!values.placeAt} style={{ width: 90 }} value={values.y} onChange={(e) => set({ y: e.target.value })} />
            <span>{units}</span>
          </div>
        </fieldset>
        <label style={row}>
          <span style={label}>Import scale:</span>
          <input aria-label="Import scale" style={{ width: 120 }} value={values.scale} onChange={(e) => set({ scale: e.target.value })} />
        </label>
        <label style={{ ...row, opacity: isDxf || !file ? 1 : 0.5 }} title="Used when the DXF items in file have no line thickness set">
          <span style={label}>Default line width:</span>
          <input aria-label="Default line width" disabled={!!file && !isDxf} style={{ width: 120 }} value={values.lineWidth} onChange={(e) => set({ lineWidth: e.target.value })} />
          <span>{units}</span>
        </label>
        <label style={{ ...row, opacity: isDxf || !file ? 1 : 0.5 }} title="Used when the DXF file has no unit set">
          <span style={label}>Default units:</span>
          <select aria-label="Default units" disabled={!!file && !isDxf} value={values.dxfUnits} onChange={(e) => set({ dxfUnits: e.target.value })}>
            {DXF_UNITS.map((u) => (
              <option key={u.id} value={u.id}>
                {u.label}
              </option>
            ))}
          </select>
        </label>
        {error && (
          <div role="alert" style={{ color: "var(--error, #d33)", fontSize: 12 }}>
            {error}
          </div>
        )}
      </div>
    </DialogShell>
  );
}
