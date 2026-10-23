// Sheet Pin Properties (`DIALOG_SHEET_PIN_PROPERTIES`, eeschema/dialogs/dialog_sheet_pin_properties.cpp at 8303b2ad), opened by `E` or a double-click on one selected pin of a sheet
// (`SCH_EDIT_TOOL::Properties`: `case SCH_SHEET_PIN_T`).
//
// The name is a combo of the hierarchical labels the sheet's own file has (`TransferDataToWindow`: every `SCH_HIER_LABEL_T` of the sheet's screen), and choosing one takes that
// label's shape (`onComboBox`); the shape is one of the five a hierarchical label has; the text's size, bold and italic are set as a label's are (the size between 0.01 and 1000 mm).
// OK sends one `edit_sheet_pin` -- one undo step, nothing when nothing changed. Not here because the drawing model has no place for them: the pin's font face and colour.
import { useEffect, useState } from "react";
import type { LabelShape } from "../api/types";
import { mmToUm, umToMm } from "../kicad-port/schProperties";
import { sheetPinEditCmd, SHEET_PIN_DEFAULT_SIZE_UM, SHEET_PIN_SHAPES } from "../kicad-port/schSheetPins";
import { useStudioApi, useStudioDispatch, useStudioState } from "../state/store";
import { SchDialogShell } from "./SchDialogShell";
import { loadHierLabels } from "./schematic/schPinTool";
import type { HierLabel } from "../kicad-port/schSheetPins";

export function SheetPinPropertiesDialog({ id }: { id: string }) {
  const state = useStudioState();
  const sheet = state.schematic?.sheets.find((s) => s.pins.some((p) => p.id === id));
  const pin = sheet?.pins.find((p) => p.id === id);
  if (!sheet || !pin) return null;
  return <Form key={id} sheetId={sheet.id} pin={pin} />;
}

function Form({ sheetId, pin }: { sheetId: string; pin: { id: string; name: string; shape: LabelShape; size_um?: number; bold?: boolean; italic?: boolean } }) {
  const state = useStudioState();
  const dispatch = useStudioDispatch();
  const api = useStudioApi();
  const [name, setName] = useState(pin.name);
  const [shape, setShape] = useState<LabelShape>(pin.shape);
  const [size, setSize] = useState(umToMm(pin.size_um && pin.size_um > 0 ? pin.size_um : SHEET_PIN_DEFAULT_SIZE_UM));
  const [bold, setBold] = useState(!!pin.bold);
  const [italic, setItalic] = useState(!!pin.italic);
  const [labels, setLabels] = useState<HierLabel[]>([]);
  // the labels of the sheet's own file, read from its page
  useEffect(() => {
    let alive = true;
    void loadHierLabels(state.currentSheetPath, sheetId).then((l) => alive && setLabels(l), () => {});
    return () => {
      alive = false;
    };
  }, [state.currentSheetPath, sheetId]);
  const close = () => dispatch({ type: "SET_SCH_TOOL_DIALOG", dialog: null });
  // `TransferDataFromWindow` validates no size: `SetTextSize` clamps it to 0.001..250 mm (`sheetPinEditCmd`)
  const sizeUm = mmToUm(size);
  const sizeOk = sizeUm !== null && sizeUm > 0;
  const canOk = name.trim().length > 0 && sizeOk;
  const pick = (text: string, picked: boolean) => {
    setName(text);
    // `onComboBox`: a label picked from the list (not a name typed) gives the pin that label's shape (`FindString`, without regard to case)
    if (!picked) return;
    const known = labels.find((l) => l.name.toLowerCase() === text.toLowerCase());
    if (known?.shape) setShape(known.shape);
  };
  const submit = async () => {
    if (!canOk) return;
    const cmd = sheetPinEditCmd(pin, { name, shape, sizeUm: sizeUm ?? undefined, bold, italic });
    if (!cmd || (await api.cmd(cmd))) close();
  };
  return (
    <SchDialogShell title="Sheet Pin Properties" width={420} onCancel={close} onOk={() => void submit()} okLabel="OK" canOk={canOk}>
      <div className="kv-grid" style={{ gridTemplateColumns: "110px 1fr" }}>
        <span>Name</span>
        <span>
          <input data-testid="sheet-pin-name" autoFocus list="sheet-pin-labels" value={name} onChange={(e) => pick(e.target.value, (e.nativeEvent as InputEvent).inputType === "insertReplacementText" || (e.nativeEvent as InputEvent).inputType === undefined)} onKeyDown={(e) => e.key === "Enter" && void submit()} style={{ width: "100%" }} />
          <datalist id="sheet-pin-labels">
            {labels.map((l) => (
              <option key={l.name} value={l.name} />
            ))}
          </datalist>
        </span>
        <span>Text size (mm)</span>
        <input data-testid="sheet-pin-size" value={size} onChange={(e) => setSize(e.target.value)} onKeyDown={(e) => e.key === "Enter" && void submit()} title="0.01 to 1000 mm" style={sizeOk ? { width: 80 } : { width: 80, outline: "1px solid var(--error, #d33)" }} />
        <span>Style</span>
        <span style={{ display: "flex", gap: 14 }}>
          <label>
            <input type="checkbox" data-testid="sheet-pin-bold" checked={bold} onChange={(e) => setBold(e.target.checked)} /> Bold
          </label>
          <label>
            <input type="checkbox" data-testid="sheet-pin-italic" checked={italic} onChange={(e) => setItalic(e.target.checked)} /> Italic
          </label>
        </span>
        <span>Shape</span>
        <span style={{ display: "flex", flexDirection: "column", gap: 2 }}>
          {SHEET_PIN_SHAPES.map((s) => (
            <label key={s.value}>
              <input type="radio" name="sheet-pin-shape" data-testid={`sheet-pin-shape-${s.value}`} checked={shape === s.value} onChange={() => setShape(s.value)} /> {s.label}
            </label>
          ))}
        </span>
      </div>
    </SchDialogShell>
  );
}
