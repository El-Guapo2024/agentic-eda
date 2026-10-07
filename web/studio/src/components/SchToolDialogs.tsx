// The small dialogs the schematic edit and drawing tools open (`StudioState.schToolDialog`): a drawn text box's text and look
// (`DIALOG_TEXT_PROPERTIES` as `DrawShape` opens it), a directive label's fields (`DIALOG_LABEL_PROPERTIES` as `createNewLabel` opens it).
import { useState, type ReactNode } from "react";
import type { DirectiveShape, SchFill, SchHAlign, SchLineStyle, SchToolDialog, SchVAlign } from "../api/schEditTypes";
import { useStudioApi, useStudioDispatch, useStudioState } from "../state/store";
import { SchDialogShell } from "./SchDialogShell";
import { ChangeSymbolsDialog } from "./SchChangeSymbolsDialog";
import { GlobalEditDialog } from "./SchGlobalEditDialog";
import { CleanupPinsDialog, SyncPinsDialog } from "./SchPinDialogs";
import { commitGraphic } from "./schematic/schShapeTools";

const DEFAULT_TEXT_SIZE_UM = 1270; // SCHEMATIC_SETTINGS::m_DefaultTextSize, 50 mil
const DIRECTIVE_POLE_UM = 2540; // SCH_DIRECTIVE_LABEL's default pin length, 100 mil

export function SchToolDialogs() {
  const state = useStudioState();
  const dialog = state.schToolDialog;
  if (!dialog || !state.schematic) return null;
  // Keyed so a second text box or label starts from the defaults again rather than from the last one's fields.
  switch (dialog.kind) {
    case "text_box":
      return <TextBoxDialog key={`tb:${dialog.start.x},${dialog.start.y},${dialog.end.x},${dialog.end.y}`} dialog={dialog} />;
    case "directive":
      return <DirectiveDialog key={`dl:${dialog.at.x},${dialog.at.y}`} dialog={dialog} />;
    case "cleanup_pins":
      return <CleanupPinsDialog key={`cp:${dialog.sheetId}`} dialog={dialog} />;
    case "sync_pins":
      return <SyncPinsDialog key={`sp:${dialog.sheetIds.join(",")}`} dialog={dialog} />;
    case "change_symbols":
      return <ChangeSymbolsDialog key={`cs:${dialog.mode}:${dialog.selected.join(",")}`} dialog={dialog} />;
    case "edit_text_graphics":
      return <GlobalEditDialog key={`ge:${dialog.selected.join(",")}`} dialog={dialog} />;
  }
}

/** The label/value grid the property dialogs lay their fields out in. */
function Shell({ title, onCancel, onOk, okLabel, canOk, children }: { title: string; onCancel: () => void; onOk: () => void; okLabel: string; canOk: boolean; children: ReactNode }) {
  return (
    <SchDialogShell title={title} onCancel={onCancel} onOk={onOk} okLabel={okLabel} canOk={canOk}>
      <div className="kv-grid" style={{ gridTemplateColumns: "110px 1fr" }}>
        {children}
      </div>
    </SchDialogShell>
  );
}

function TextBoxDialog({ dialog }: { dialog: Extract<SchToolDialog, { kind: "text_box" }> }) {
  const state = useStudioState();
  const dispatch = useStudioDispatch();
  const api = useStudioApi();
  const [text, setText] = useState("");
  const [size, setSize] = useState(DEFAULT_TEXT_SIZE_UM);
  const [bold, setBold] = useState(false);
  const [italic, setItalic] = useState(false);
  const [h, setH] = useState<SchHAlign>("left");
  const [v, setV] = useState<SchVAlign>("top");
  const [vertical, setVertical] = useState(false);
  const [width, setWidth] = useState(0);
  const [style, setStyle] = useState<SchLineStyle>("default");
  const [fill, setFill] = useState<SchFill>("none");
  const close = () => dispatch({ type: "SET_SCH_TOOL_DIALOG", dialog: null });
  const submit = () => {
    if (!text.trim() || !state.schematic) return;
    commitGraphic(
      { sch: state.schematic, dispatch, api },
      { shape: { type: "text_box", start: dialog.start, end: dialog.end, text, angle: vertical ? 90_000 : 0, size_um: size, bold, italic, h_align: h, v_align: v }, width_um: width, line_style: style, fill }
    );
    close();
  };
  return (
    <Shell title="Text Box Properties" onCancel={close} onOk={submit} okLabel="OK" canOk={text.trim().length > 0}>
      <span>Text</span>
      <textarea autoFocus rows={4} value={text} onChange={(e) => setText(e.target.value)} />
      <span>Text size (um)</span>
      <input type="number" min={100} step={50} value={size} onChange={(e) => setSize(Number(e.target.value))} />
      <span>Style</span>
      <span>
        <label>
          <input type="checkbox" checked={bold} onChange={(e) => setBold(e.target.checked)} /> Bold
        </label>{" "}
        <label>
          <input type="checkbox" checked={italic} onChange={(e) => setItalic(e.target.checked)} /> Italic
        </label>
      </span>
      <span>Horizontal</span>
      <select value={h} onChange={(e) => setH(e.target.value as SchHAlign)}>
        <option value="left">Left</option>
        <option value="center">Center</option>
        <option value="right">Right</option>
      </select>
      <span>Vertical</span>
      <select value={v} onChange={(e) => setV(e.target.value as SchVAlign)}>
        <option value="top">Top</option>
        <option value="center">Center</option>
        <option value="bottom">Bottom</option>
      </select>
      <span>Orientation</span>
      <select value={vertical ? "v" : "h"} onChange={(e) => setVertical(e.target.value === "v")}>
        <option value="h">Horizontal</option>
        <option value="v">Vertical</option>
      </select>
      <span>Border width (um)</span>
      <input type="number" min={0} step={10} value={width} onChange={(e) => setWidth(Number(e.target.value))} title="0 = the default line width" />
      <span>Border style</span>
      <select value={style} onChange={(e) => setStyle(e.target.value as SchLineStyle)}>
        <option value="default">Default</option>
        <option value="solid">Solid</option>
        <option value="dash">Dashed</option>
        <option value="dot">Dotted</option>
        <option value="dash_dot">Dash-dot</option>
        <option value="dash_dot_dot">Dash-dot-dot</option>
      </select>
      <span>Fill</span>
      <select value={fill} onChange={(e) => setFill(e.target.value as SchFill)}>
        <option value="none">None</option>
        <option value="outline">Outline color</option>
        <option value="background">Background color</option>
      </select>
    </Shell>
  );
}

const ORIENTATIONS: Array<{ label: string; millideg: number }> = [
  { label: "Right", millideg: 0 },
  { label: "Up", millideg: 90_000 },
  { label: "Left", millideg: 180_000 },
  { label: "Down", millideg: 270_000 },
];

function DirectiveDialog({ dialog }: { dialog: Extract<SchToolDialog, { kind: "directive" }> }) {
  const state = useStudioState();
  const dispatch = useStudioDispatch();
  const api = useStudioApi();
  const [netclass, setNetclass] = useState("");
  const [componentClass, setComponentClass] = useState("");
  const [shape, setShape] = useState<DirectiveShape>("round"); // m_lastNetClassFlagShape starts as F_ROUND
  const [orientation, setOrientation] = useState(0); // m_lastTextOrientation starts as SPIN_STYLE::RIGHT
  const close = () => dispatch({ type: "SET_SCH_TOOL_DIALOG", dialog: null });
  const submit = () => {
    if (!state.schematic) return;
    commitGraphic({ sch: state.schematic, dispatch, api }, { shape: { type: "directive", at: dialog.at, orientation, shape, pin_length_um: DIRECTIVE_POLE_UM, netclass, component_class: componentClass } });
    close();
  };
  return (
    <Shell title="Directive Label Properties" onCancel={close} onOk={submit} okLabel="OK" canOk>
      <span>Netclass</span>
      <input autoFocus value={netclass} onChange={(e) => setNetclass(e.target.value)} onKeyDown={(e) => e.key === "Enter" && submit()} />
      <span>Component Class</span>
      <input value={componentClass} onChange={(e) => setComponentClass(e.target.value)} onKeyDown={(e) => e.key === "Enter" && submit()} />
      <span>Shape</span>
      <select value={shape} onChange={(e) => setShape(e.target.value as DirectiveShape)}>
        <option value="dot">Dot</option>
        <option value="round">Round</option>
        <option value="diamond">Diamond</option>
        <option value="rectangle">Rectangle</option>
      </select>
      <span>Orientation</span>
      <select value={orientation} onChange={(e) => setOrientation(Number(e.target.value))}>
        {ORIENTATIONS.map((o) => (
          <option key={o.label} value={o.millideg}>
            {o.label}
          </option>
        ))}
      </select>
    </Shell>
  );
}
