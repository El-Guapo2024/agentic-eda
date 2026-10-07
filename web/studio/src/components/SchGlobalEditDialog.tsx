// Edit Text & Graphics Properties (`DIALOG_GLOBAL_EDIT_TEXT_AND_GRAPHICS`): properties to set on every text, text box, shape, rule area or graphic line of the
// sheet, or on the selected ones only; a property left "unchanged" stays as it is. The rules are kicad-port/schGlobalEdit.ts.
import { useState } from "react";
import type { SchFill, SchHAlign, SchLineStyle, SchToolDialog, SchVAlign } from "../api/schEditTypes";
import { emptyEdit, planGlobalEdit, type GlobalEditSpec } from "../kicad-port/schGlobalEdit";
import { useStudioApi, useStudioDispatch, useStudioState } from "../state/store";
import { SchDialogShell } from "./SchDialogShell";

const KEEP = "";

/** A number field that is empty when the property is to stay as it is. */
const parseNum = (s: string): number | null => (s.trim() === "" || !Number.isFinite(Number(s)) ? null : Math.max(0, Math.round(Number(s))));
const parseTri = (s: string): boolean | null => (s === "yes" ? true : s === "no" ? false : null);

export function GlobalEditDialog({ dialog }: { dialog: Extract<SchToolDialog, { kind: "edit_text_graphics" }> }) {
  const state = useStudioState();
  const dispatch = useStudioDispatch();
  const api = useStudioApi();
  const [selectedOnly, setSelectedOnly] = useState(dialog.selected.length > 0);
  const [kinds, setKinds] = useState(emptyEdit().kinds);
  const [textSize, setTextSize] = useState("");
  const [bold, setBold] = useState(KEEP);
  const [italic, setItalic] = useState(KEEP);
  const [hAlign, setHAlign] = useState(KEEP);
  const [vAlign, setVAlign] = useState(KEEP);
  const [lineWidth, setLineWidth] = useState("");
  const [lineStyle, setLineStyle] = useState(KEEP);
  const [fill, setFill] = useState(KEEP);
  const close = () => dispatch({ type: "SET_SCH_TOOL_DIALOG", dialog: null });

  const spec = (): GlobalEditSpec => ({
    selectedOnly,
    kinds,
    textSize: parseNum(textSize),
    bold: parseTri(bold),
    italic: parseTri(italic),
    hAlign: (hAlign || null) as SchHAlign | null,
    vAlign: (vAlign || null) as SchVAlign | null,
    lineWidth: parseNum(lineWidth),
    lineStyle: (lineStyle || null) as SchLineStyle | null,
    fill: (fill || null) as SchFill | null,
  });
  const apply = () => {
    if (!state.schematic) return;
    const cmds = planGlobalEdit(state.schematic, new Set(dialog.selected), spec());
    if (cmds.length === 0) dispatch({ type: "TOAST", message: "Nothing to change.", kind: "info" });
    else void api.cmdBatch(cmds);
    close();
  };

  const tri = (value: string, set: (v: string) => void) => (
    <select value={value} onChange={(e) => set(e.target.value)}>
      <option value={KEEP}>(unchanged)</option>
      <option value="yes">Yes</option>
      <option value="no">No</option>
    </select>
  );
  const kindBox = (key: keyof typeof kinds, label: string) => (
    <label style={{ display: "block" }}>
      <input type="checkbox" checked={kinds[key]} onChange={(e) => setKinds({ ...kinds, [key]: e.target.checked })} /> {label}
    </label>
  );

  return (
    <SchDialogShell title="Edit Text and Graphics Properties" width={440} onCancel={close} onOk={apply} okLabel="OK">
      <fieldset style={{ border: "1px solid var(--chrome-border)", margin: "0 0 8px", padding: "6px 8px" }}>
        <legend>Items</legend>
        <label style={{ display: "block", marginBottom: 4 }}>
          <input type="checkbox" checked={selectedOnly} disabled={dialog.selected.length === 0} onChange={(e) => setSelectedOnly(e.target.checked)} /> Edit selected items only
        </label>
        {kindBox("texts", "Text items")}
        {kindBox("textBoxes", "Text boxes")}
        {kindBox("shapes", "Graphic shapes")}
        {kindBox("ruleAreas", "Rule areas")}
        {kindBox("lines", "Graphic lines")}
      </fieldset>
      <div className="kv-grid" style={{ gridTemplateColumns: "130px 1fr" }}>
        <span>Text size (um)</span>
        <input type="number" min={0} step={50} placeholder="(unchanged)" value={textSize} onChange={(e) => setTextSize(e.target.value)} />
        <span>Bold</span>
        {tri(bold, setBold)}
        <span>Italic</span>
        {tri(italic, setItalic)}
        <span>Horizontal alignment</span>
        <select value={hAlign} onChange={(e) => setHAlign(e.target.value)}>
          <option value={KEEP}>(unchanged)</option>
          <option value="left">Left</option>
          <option value="center">Center</option>
          <option value="right">Right</option>
        </select>
        <span>Vertical alignment</span>
        <select value={vAlign} onChange={(e) => setVAlign(e.target.value)}>
          <option value={KEEP}>(unchanged)</option>
          <option value="top">Top</option>
          <option value="center">Center</option>
          <option value="bottom">Bottom</option>
        </select>
        <span>Line width (um)</span>
        <input type="number" min={0} step={10} placeholder="(unchanged)" value={lineWidth} onChange={(e) => setLineWidth(e.target.value)} />
        <span>Line style</span>
        <select value={lineStyle} onChange={(e) => setLineStyle(e.target.value)}>
          <option value={KEEP}>(unchanged)</option>
          <option value="default">Default</option>
          <option value="solid">Solid</option>
          <option value="dash">Dashed</option>
          <option value="dot">Dotted</option>
          <option value="dash_dot">Dash-dot</option>
          <option value="dash_dot_dot">Dash-dot-dot</option>
        </select>
        <span>Fill</span>
        <select value={fill} onChange={(e) => setFill(e.target.value)}>
          <option value={KEEP}>(unchanged)</option>
          <option value="none">None</option>
          <option value="outline">Outline color</option>
          <option value="background">Background color</option>
        </select>
      </div>
    </SchDialogShell>
  );
}
