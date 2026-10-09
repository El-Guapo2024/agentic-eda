// The small dialogs the schematic edit and drawing tools open (`StudioState.schToolDialog`): a drawn text box's text and look
// (`DIALOG_TEXT_PROPERTIES` as `DrawShape` opens it), a directive label's fields (`DIALOG_LABEL_PROPERTIES` as `createNewLabel` opens it).
import { useState, type ReactNode } from "react";
import type { DirectiveShape, SchFill, SchGraphic, SchHAlign, SchLineStyle, SchToolDialog, SchVAlign } from "../api/schEditTypes";
import { graphicsEqual } from "../kicad-port/schProperties";
import { useStudioApi, useStudioDispatch, useStudioState } from "../state/store";
import { SchDialogShell } from "./SchDialogShell";
import { ChangeSymbolsDialog } from "./SchChangeSymbolsDialog";
import { FieldPropertiesDialog } from "./SchFieldDialog";
import { GlobalEditDialog } from "./SchGlobalEditDialog";
import { CleanupPinsDialog, SyncPinsDialog } from "./SchPinDialogs";
import { LabelPropertiesDialog, SheetPropertiesDialog, ShapePropertiesDialog, StrokePropertiesDialog, TextPropertiesDialog } from "./SchPropertiesDialogs";
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
    // Properties (`E`, a double-click): SchPropertiesDialogs.tsx; a text box and a directive label reopen the dialogs that drew them.
    case "props_label":
      return <LabelPropertiesDialog key={`pl:${dialog.id}`} id={dialog.id} />;
    case "props_field":
      return <FieldPropertiesDialog key={`pf:${dialog.id}`} id={dialog.id} />;
    case "props_text":
      return <TextPropertiesDialog key={`pt:${dialog.id}`} id={dialog.id} />;
    case "props_sheet":
      return <SheetPropertiesDialog key={`ps:${dialog.id}`} id={dialog.id} />;
    case "props_stroke":
      return <StrokePropertiesDialog key={`pk:${dialog.ids.join(",")}`} ids={dialog.ids} />;
    case "props_graphic": {
      const g = state.schematic.graphics?.find((x) => x.id === dialog.id);
      if (!g) return null;
      if (g.shape.type === "text_box") return <TextBoxDialog key={`pg:${g.id}`} dialog={{ kind: "text_box", start: g.shape.start, end: g.shape.end }} edit={g} />;
      if (g.shape.type === "directive") return <DirectiveDialog key={`pg:${g.id}`} dialog={{ kind: "directive", at: g.shape.at }} edit={g} />;
      return <ShapePropertiesDialog key={`pg:${g.id}`} id={g.id} />;
    }
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

/**
 * A drawn text box's text and look (`DrawShape` -> `DIALOG_TEXT_PROPERTIES`); with `edit` it is that text box's Properties instead (`SCH_EDIT_TOOL::Properties`):
 * the same fields start from the box's own, and OK replaces the box in place (its corners, id, lock and place in the drawing order stay).
 */
function TextBoxDialog({ dialog, edit }: { dialog: Extract<SchToolDialog, { kind: "text_box" }>; edit?: SchGraphic }) {
  const state = useStudioState();
  const dispatch = useStudioDispatch();
  const api = useStudioApi();
  const box = edit?.shape.type === "text_box" ? edit.shape : null;
  const [text, setText] = useState(box?.text ?? "");
  const [size, setSize] = useState(box?.size_um ?? DEFAULT_TEXT_SIZE_UM);
  const [bold, setBold] = useState(box?.bold ?? false);
  const [italic, setItalic] = useState(box?.italic ?? false);
  const [h, setH] = useState<SchHAlign>(box?.h_align ?? "left");
  const [v, setV] = useState<SchVAlign>(box?.v_align ?? "top");
  const [vertical, setVertical] = useState((box?.angle ?? 0) === 90_000);
  const [width, setWidth] = useState(edit?.width_um ?? 0);
  const [style, setStyle] = useState<SchLineStyle>(edit?.line_style ?? "default");
  const [fill, setFill] = useState<SchFill>(edit?.fill ?? "none");
  const close = () => dispatch({ type: "SET_SCH_TOOL_DIALOG", dialog: null });
  const submit = () => {
    if (!text.trim() || !state.schematic) return;
    const shape = { type: "text_box" as const, start: dialog.start, end: dialog.end, text, angle: vertical ? 90_000 : 0, size_um: size, bold, italic, h_align: h, v_align: v, margin_um: box?.margin_um };
    if (edit) {
      const next: SchGraphic = { ...edit, shape, width_um: width, line_style: style, fill };
      if (graphicsEqual(next, edit)) return close();
      void api.cmd({ op: "sch_edit", verb: "edit_graphic", id: edit.id, graphic: next }).then((ok) => ok && close());
      return;
    }
    commitGraphic({ sch: state.schematic, dispatch, api }, { shape, width_um: width, line_style: style, fill });
    close();
  };
  return (
    <Shell title="Text Box Properties" onCancel={close} onOk={submit} okLabel="OK" canOk={text.trim().length > 0 && size >= 10 && size <= 1_000_000}>
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

/** A new directive label's fields (`createNewLabel` -> `DIALOG_LABEL_PROPERTIES`); with `edit`, that directive label's Properties: its fields start from the label's own and OK replaces it in place. */
function DirectiveDialog({ dialog, edit }: { dialog: Extract<SchToolDialog, { kind: "directive" }>; edit?: SchGraphic }) {
  const state = useStudioState();
  const dispatch = useStudioDispatch();
  const api = useStudioApi();
  const flag = edit?.shape.type === "directive" ? edit.shape : null;
  const [netclass, setNetclass] = useState(flag?.netclass ?? "");
  const [componentClass, setComponentClass] = useState(flag?.component_class ?? "");
  const [shape, setShape] = useState<DirectiveShape>(flag?.shape ?? "round"); // m_lastNetClassFlagShape starts as F_ROUND
  const [orientation, setOrientation] = useState(flag?.orientation ?? 0); // m_lastTextOrientation starts as SPIN_STYLE::RIGHT
  const close = () => dispatch({ type: "SET_SCH_TOOL_DIALOG", dialog: null });
  const submit = () => {
    if (!state.schematic) return;
    if (edit && flag) {
      const next: SchGraphic = { ...edit, shape: { ...flag, orientation, shape, netclass, component_class: componentClass } };
      if (graphicsEqual(next, edit)) return close();
      void api.cmd({ op: "sch_edit", verb: "edit_graphic", id: edit.id, graphic: next }).then((ok) => ok && close());
      return;
    }
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
