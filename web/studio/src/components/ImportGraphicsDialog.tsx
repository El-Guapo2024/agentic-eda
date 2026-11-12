// Import Graphics (`DIALOG_IMPORT_GRAPHICS`, pcbnew/import_gfx/dialog_import_graphics.cpp; `DIALOG_IMPORT_GFX_SCH` is its schematic twin, used by the schematic editor and the Symbol Editor): a DXF
// or SVG file, the import scale, the DXF's default line width and units, and where it goes. The board's dialog also has the layer, "Group imported items" and "Fix discontinuities" with its
// tolerance; the schematic's and the symbol editor's has "Interactive placement" or "At" X, Y and nothing else.
//
// A browser gives the page a file's text and not its path, so "File:" is filled by a Browse... button (a file chooser) or by a file dropped on the dialog; the page reads it, and
// the server says what it holds (lines, arcs, ...; what it could not handle) before OK. OK sends the one command (crates/ops/src/import_graphics.rs, import_graphics_sch.rs). On the board,
// with "Place at" off, the items are a preview under the cursor (`PlaceImportedGraphics`: the server makes them on a copy of the board, `POST /api/import_graphics/preview`, and the
// canvas draws them where the snapped cursor holds them, state/importPreview.ts) until a click places them -- the command at that offset -- or Escape drops them. The schematic's
// items are put at the cursor and carried by the Move tool until a click drops them (Escape takes them back), as a Paste is in this studio. The Symbol Editor has no carried
// move: its items are added at the cursor and selected, to be dragged.
import { useEffect, useRef, useState } from "react";
import { useStudioApi, useStudioDispatch, useStudioState } from "../state/store";
import { useSymApi, useSymDispatch, useSymState } from "../state/symbolEditorStore";
import { useFpApi, useFpState } from "../state/footprintEditorStore";
import { fetchFootprint } from "../api/client";
import { FP_EDITOR_LAYERS } from "../kicad-port/fpDrawDefaults";
import { fetchLibrarySymbol } from "../api/client";
import { closeImportGraphics, openImportGraphics, useImportGraphicsDialog, type PickedFile } from "../state/importDialogs";
import { DialogShell } from "./pcbDialogKit";
import { pickTextFile } from "../api/libraryClient";
import { previewGraphics, readGraphics } from "../api/importClient";
import { fetchSchematic, postPlacePreview } from "../api/client";
import { setSchPaste } from "../state/schPasteStore";
import { PREVIEW_AT } from "../kicad-port/schPlaceTool";
import { newIdsAfter } from "../kicad-port/schClipboard";
import { setImportPreview } from "../state/importPreview";
import { pickPoint } from "../actions/pcbPicker";
import { DXF_UNITS, buildImportCmd, buildSchImportCmd, defaultImportValues, defaultSchImportValues, describeInfo, graphicsFormatOf, importLayers, type GraphicsInfo, type ImportValues, type SchImportValues } from "../kicad-port/importGraphics";
import { snapGridNm } from "./canvas/gridHelper";

/** The dialog keeps what it was last set to while the page lives (KiCad saves a dialog's controls between runs). */
let lastValues: ImportValues | null = null;
let lastSchValues: SchImportValues | null = null;

export function ImportGraphicsDialog() {
  const dialog = useImportGraphicsDialog();
  if (!dialog) return null;
  if (dialog.editor === "pcb" || dialog.editor === "footprint") return <PcbImportGraphics editor={dialog.editor} file={dialog.file} />;
  if (dialog.editor === "schematic" || dialog.editor === "symbol") return <SchImportGraphics editor={dialog.editor} file={dialog.file} />;
  return null;
}

/**
 * `DIALOG_IMPORT_GRAPHICS` on the board, and in the Footprint Editor (`DRAWING_TOOL::PlaceImportedGraphics` with the footprint the items' parent, UI-LEFT F8): there the layers are the
 * editor's, the items are the footprint's (`fp_editor import_graphics`, one undo step of the editor), and the interactive placement carries them on the editor's canvas.
 */
function PcbImportGraphics({ file, editor }: { file: PickedFile | null; editor: "pcb" | "footprint" }) {
  const state = useStudioState();
  const dispatch = useStudioDispatch();
  const api = useStudioApi();
  const fpState = useFpState();
  const fpApi = useFpApi();
  const inFootprint = editor === "footprint";
  const activeLayer = inFootprint ? fpState.activeLayer : state.activeLayer;
  const units = state.units;
  const [values, setValues] = useState<ImportValues>(() => lastValues ?? { ...defaultImportValues(units), layer: activeLayer ?? "Dwgs.User" });
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

  const layers = inFootprint ? [...FP_EDITOR_LAYERS] : importLayers(state.board?.layers ?? ["F.Cu", "B.Cu"]);
  const polygonOutline = !inFootprint && !!state.board && !state.board.outline_is_shapes;
  const onEdge = values.setLayer && values.layer === "Edge.Cuts";

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
    const built = buildImportCmd({ format, text: file.text, values, units, activeLayer, info, polygonOutline });
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
    if (inFootprint) {
      // the footprint's items: put with the picture's top-left at the footprint's origin, then carried to where a click drops them (Escape undoes the import)
      const name = fpApi.getState().name;
      const before = fpApi.getState().footprint;
      if (!name || !before) {
        setBusy(false);
        setError("Open a footprint first.");
        return;
      }
      const { op: _op, ...importArgs } = built.build(built.interactive ? { x: 0, y: 0 } : null);
      const had = new Set([...before.graphics.map((g) => g.id), ...before.texts.map((t) => t.id)]);
      const ok = await fpApi.cmd({ op: "fp_editor", verb: "import_graphics", footprint: name, import: importArgs });
      setBusy(false);
      if (!ok) return;
      const after = await fetchFootprint(name);
      const made = [...after.graphics.map((g) => g.id), ...after.texts.map((t) => t.id)].filter((id): id is string => !!id && !had.has(id));
      if (built.interactive && made.length > 0) fpApi.pickUp(made);
      const warnings = (info?.messages ?? []).map((m) => m.text);
      if (warnings.length > 0) dispatch({ type: "TOAST", message: `Items in the imported file could not be handled properly. ${warnings.join(" ")}`, kind: "info" });
      closeImportGraphics();
      return;
    }
    // `HTML_MESSAGE_BOX`: "Items in the imported file could not be handled properly."
    const warnings = (info?.messages ?? []).map((m) => m.text);
    const sayWarnings = () => {
      if (warnings.length > 0) dispatch({ type: "TOAST", message: `Items in the imported file could not be handled properly. ${warnings.join(" ")}`, kind: "info" });
    };
    if (!built.interactive) {
      const placed = await api.placeNewItems((at) => built.build(at), false);
      setBusy(false);
      if (!placed) return; // the refusal is on screen as a toast
      sayWarnings();
      closeImportGraphics();
      return;
    }
    // `DRAWING_TOOL::PlaceImportedGraphics`: the items are a preview under the cursor -- held by the top left item -- until a click places them (one command, one undo step,
    // the new items selected) or Escape drops them; nothing is on the board meanwhile.
    let preview;
    try {
      preview = await previewGraphics(built.atOffset({ x: 0, y: 0 }));
    } catch (e) {
      setBusy(false);
      setError(e instanceof Error ? e.message : String(e));
      return;
    }
    setBusy(false);
    if (!preview.ok || !preview.reference) {
      setError(preview.message ?? "No graphic items found in file.");
      return;
    }
    closeImportGraphics();
    const reference = preview.reference;
    setImportPreview({ shapes: preview.shapes ?? [], texts: preview.texts ?? [], reference });
    const at = await pickPoint("");
    setImportPreview(null);
    if (!at) return;
    const placed = await api.placeNewItems(() => built.atOffset({ x: at.x - reference.x, y: at.y - reference.y }), false);
    if (placed) sayWarnings();
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
    if (editor === "schematic" && built.interactive) {
      // `SCH_DRAWING_TOOLS::ImportGraphics`: the items are a preview, selected, moved by the grid point under the cursor (`delta = cursorPos`: the drawing's origin is on it),
      // until a click adds them -- one command, one undo step ("Import Graphic"), the new items selected -- or Escape deletes them; nothing is on the sheet meanwhile.
      // The preview is the server's drawing of the very command at `PREVIEW_AT` (`POST /api/sch/place_preview`), shifted by the page as a paste in the hand is.
      const sheet = [...api.getState().currentSheetPath];
      const preview = await postPlacePreview({ sheet: sheet.join("/"), cmd: built.build({ x: PREVIEW_AT[0], y: PREVIEW_AT[1] }) });
      setBusy(false);
      if (!preview) {
        setError("No graphic items found in file.");
        return;
      }
      const warnings = (info?.messages ?? []).map((m) => m.text);
      closeImportGraphics();
      dispatch({ type: "CLEAR_SELECTION" });
      setSchPaste({
        fragment: null,
        preview,
        anchor: [PREVIEW_AT[0], PREVIEW_AT[1]],
        sheet,
        mode: "keep",
        origin: "paste",
        notes: [],
        place: async (snapped) => {
          const before = api.getState().schematic;
          const ok = await api.cmd(built.build({ x: snapped[0], y: snapped[1] }));
          if (!ok) return;
          if (warnings.length > 0) dispatch({ type: "TOAST", message: `Items in the imported file could not be handled properly. ${warnings.join(" ")}`, kind: "info" });
          if (!before) return;
          const after = await fetchSchematic(sheet).catch(() => null);
          const ids = after ? newIdsAfter(before, after) : [];
          if (ids.length > 0) dispatch({ type: "SET_SELECTION", refs: ids, raw: true });
        },
      });
      return;
    }
    if (editor === "schematic") {
      placed = await api.placeNewSchItems((at) => built.build(at), built.interactive);
    } else {
      // The Symbol Editor: the items go to the grid point under the cursor, and are selected once they are there.
      const st = symApi.getState();
      // (the cursor on the grid while the Symbol Editor snaps to it: `GetCursorPosition( true )`)
      const grid = snapGridNm(st.gridNm > 0 ? st.gridNm : 1_270_000) || 1;
      const cursor = st.cursorNm && Number.isFinite(st.cursorNm.x) && Number.isFinite(st.cursorNm.y) ? st.cursorNm : null;
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
