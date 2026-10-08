// Properties (`E`, a double-click) dialogs of the schematic items that had none once placed (`SCH_EDIT_TOOL::Properties`, eeschema/tools/sch_edit_tool.cpp):
// a label (`DIALOG_LABEL_PROPERTIES`), a free text (`DIALOG_TEXT_PROPERTIES`), a hierarchical sheet (`DIALOG_SHEET_PROPERTIES`), wires / buses / bus entries /
// graphic lines / junctions (`DIALOG_WIRE_BUS_PROPERTIES`, `DIALOG_LINE_PROPERTIES`, `DIALOG_JUNCTION_PROPS`) and a drawn shape or rule area
// (`DIALOG_SHAPE_PROPERTIES`). A text box and a directive label reopen the dialogs that created them (SchToolDialogs.tsx).
//
// Each OK sends one command (kicad-port/schProperties.ts builds it from what the user changed), so it is one undo step; nothing is sent when nothing changed.
// What the drawing model has no place for is not offered: fonts, bold/italic and text colours, a sheet's border and fill and extra fields, a label's fields.
import { useState, type ReactNode } from "react";
import type { LabelShape, SchematicLabel } from "../api/types";
import type { SchColor, SchFill, SchGraphic, SchLineStyle } from "../api/schEditTypes";
import { colorToHex, graphicsEqual, hexToColor, isUnspecified, labelEditCmd, mmToUm, sheetEditCmd, strokeEditCmd, strokeView, textEditCmd, UNSPECIFIED_COLOR, umToMm, type LabelSpinName, type StrokeChange } from "../kicad-port/schProperties";
import { useStudioApi, useStudioDispatch, useStudioState } from "../state/store";
import { SchDialogShell } from "./SchDialogShell";
import { inferSpin } from "./schematic/labelShape";

const LABEL_SHAPES: Array<{ value: LabelShape; label: string }> = [
  { value: "input", label: "Input" },
  { value: "output", label: "Output" },
  { value: "bidirectional", label: "Bidirectional" },
  { value: "tri_state", label: "Tri-state" },
  { value: "passive", label: "Passive" },
];

const SPINS: Array<{ value: LabelSpinName; label: string }> = [
  { value: "right", label: "Right" },
  { value: "left", label: "Left" },
  { value: "up", label: "Up" },
  { value: "bottom", label: "Bottom" },
];

const LINE_STYLES: Array<{ value: SchLineStyle; label: string }> = [
  { value: "default", label: "Default" },
  { value: "solid", label: "Solid" },
  { value: "dash", label: "Dashed" },
  { value: "dot", label: "Dotted" },
  { value: "dash_dot", label: "Dash-dot" },
  { value: "dash_dot_dot", label: "Dash-dot-dot" },
];

function useClose() {
  const dispatch = useStudioDispatch();
  return () => dispatch({ type: "SET_SCH_TOOL_DIALOG", dialog: null });
}

/** The label/value grid the property dialogs lay their fields out in. */
function Grid({ children }: { children: ReactNode }) {
  return (
    <div className="kv-grid" style={{ gridTemplateColumns: "130px 1fr" }}>
      {children}
    </div>
  );
}

/** Send the command a dialog built (null: nothing changed) and close the dialog once the backend took it; a refusal leaves the dialog open. */
function useSend(close: () => void) {
  const api = useStudioApi();
  return async (cmd: Parameters<typeof api.cmd>[0] | null) => {
    if (!cmd || (await api.cmd(cmd))) close();
  };
}

// ---------------------------------------------------------------------------------------------------------------------------------- label

export function LabelPropertiesDialog({ id }: { id: string }) {
  const sch = useStudioState().schematic;
  const label = sch?.labels.find((l) => l.id === id);
  if (!sch || !label) return null;
  return <LabelForm label={label} spin={label.spin ?? inferSpin(sch.wires, label.at)} />;
}

function LabelForm({ label, spin }: { label: SchematicLabel; spin: LabelSpinName }) {
  const close = useClose();
  const send = useSend(close);
  const [text, setText] = useState(label.net);
  const [shape, setShape] = useState<LabelShape>(label.shape ?? "input");
  const [turn, setTurn] = useState<LabelSpinName>(spin);
  const title = label.scope === "global" ? "Global Label Properties" : label.scope === "hierarchical" ? "Hierarchical Label Properties" : "Label Properties";
  // "Label can not be empty." (`DIALOG_LABEL_PROPERTIES::TransferDataFromWindow`)
  const canOk = text.trim().length > 0;
  const submit = () => canOk && void send(labelEditCmd(label, spin, { text, shape, spin: turn }));
  return (
    <SchDialogShell title={title} width={380} onCancel={close} onOk={submit} okLabel="OK" canOk={canOk}>
      <Grid>
        <span>Label</span>
        <input autoFocus value={text} onChange={(e) => setText(e.target.value)} onKeyDown={(e) => e.key === "Enter" && submit()} />
        {label.scope !== "local" && (
          <>
            <span>Shape</span>
            <select value={shape} onChange={(e) => setShape(e.target.value as LabelShape)}>
              {LABEL_SHAPES.map((s) => (
                <option key={s.value} value={s.value}>
                  {s.label}
                </option>
              ))}
            </select>
          </>
        )}
        <span>Orientation</span>
        <select value={turn} onChange={(e) => setTurn(e.target.value as LabelSpinName)}>
          {SPINS.map((s) => (
            <option key={s.value} value={s.value}>
              {s.label}
            </option>
          ))}
        </select>
      </Grid>
    </SchDialogShell>
  );
}

// ---------------------------------------------------------------------------------------------------------------------------------- text

export function TextPropertiesDialog({ id }: { id: string }) {
  const text = useStudioState().schematic?.texts.find((t) => t.id === id);
  return text ? <TextForm key={id} text={text} /> : null;
}

function TextForm({ text }: { text: { id: string; content: string; size_um: number; angle: number } }) {
  const close = useClose();
  const send = useSend(close);
  const [content, setContent] = useState(text.content);
  const [size, setSize] = useState(umToMm(text.size_um));
  const [vertical, setVertical] = useState(Math.round(text.angle) === 90);
  const sizeUm = mmToUm(size);
  // `m_textSize.Validate( 0.01, 1000.0, EDA_UNITS::MM )`: "Don't allow text to disappear"
  const sizeOk = sizeUm !== null && sizeUm >= 10 && sizeUm <= 1_000_000;
  const canOk = content.length > 0 && sizeOk;
  const submit = () => canOk && void send(textEditCmd(text, { content, sizeUm, vertical }));
  return (
    <SchDialogShell title="Text Properties" width={420} onCancel={close} onOk={submit} okLabel="OK" canOk={canOk}>
      <Grid>
        <span>Text</span>
        <textarea autoFocus rows={4} value={content} onChange={(e) => setContent(e.target.value)} />
        <span>Text size (mm)</span>
        <input value={size} onChange={(e) => setSize(e.target.value)} style={sizeOk ? undefined : { outline: "1px solid var(--error, #d33)" }} title="0.01 to 1000 mm" />
        <span>Orientation</span>
        <select value={vertical ? "v" : "h"} onChange={(e) => setVertical(e.target.value === "v")}>
          <option value="h">Horizontal</option>
          <option value="v">Vertical</option>
        </select>
      </Grid>
    </SchDialogShell>
  );
}

// ---------------------------------------------------------------------------------------------------------------------------------- sheet

export function SheetPropertiesDialog({ id }: { id: string }) {
  const sheet = useStudioState().schematic?.sheets.find((s) => s.id === id);
  return sheet ? <SheetForm key={id} sheet={sheet} /> : null;
}

function SheetForm({ sheet }: { sheet: { id: string; name: string; file: string; page?: string } }) {
  const close = useClose();
  const send = useSend(close);
  const [name, setName] = useState(sheet.name);
  const [file, setFile] = useState(sheet.file);
  const [page, setPage] = useState(sheet.page ?? "");
  // A sheet must have a name and a valid file name; a page number is letters and digits only (`wxFILTER_ALPHANUMERIC`).
  const fileOk = file.trim().length > 0 && !/[\\/]/.test(file);
  const pageOk = /^[\p{L}\p{N}]*$/u.test(page.trim());
  const canOk = name.trim().length > 0 && fileOk && pageOk;
  const submit = () => canOk && void send(sheetEditCmd(sheet, { name, file, page }));
  return (
    <SchDialogShell title="Sheet Properties" width={440} onCancel={close} onOk={submit} okLabel="OK" canOk={canOk}>
      <Grid>
        <span>Sheet name</span>
        <input autoFocus value={name} onChange={(e) => setName(e.target.value)} onKeyDown={(e) => e.key === "Enter" && submit()} />
        <span>Sheet file name</span>
        <input value={file} onChange={(e) => setFile(e.target.value)} onKeyDown={(e) => e.key === "Enter" && submit()} title="A file the project has is shown; a new name moves this sheet's content to it (or copies it when another sheet shows the old file)" />
        <span>Page number</span>
        <input value={page} onChange={(e) => setPage(e.target.value)} onKeyDown={(e) => e.key === "Enter" && submit()} placeholder="by order" />
      </Grid>
    </SchDialogShell>
  );
}

// ---------------------------------------------------------------------------------------------------------------------------------- stroke

/** A `<input type="color">` and the "Default" tick next to it (`COLOR_SWATCH` with `SetDefaultColor( COLOR4D::UNSPECIFIED )`). */
function ColorField({ color, mixed, onChange }: { color: SchColor; mixed: boolean; onChange: (c: SchColor) => void }) {
  const unspecified = isUnspecified(color);
  return (
    <span style={{ display: "flex", alignItems: "center", gap: 8 }}>
      <input type="color" value={unspecified ? "#000000" : colorToHex(color)} disabled={unspecified} onChange={(e) => onChange(hexToColor(e.target.value) ?? color)} />
      <label>
        <input type="checkbox" checked={unspecified && !mixed} onChange={(e) => onChange(e.target.checked ? UNSPECIFIED_COLOR : { r: 0, g: 0, b: 0, a: 255 })} /> Default
      </label>
      {mixed && <i style={{ color: "var(--chrome-text-dim)" }}>(mixed)</i>}
    </span>
  );
}

export function StrokePropertiesDialog({ ids }: { ids: string[] }) {
  const sch = useStudioState().schematic;
  return sch ? <StrokeForm key={ids.join(",")} ids={ids} /> : null;
}

function StrokeForm({ ids }: { ids: string[] }) {
  const sch = useStudioState().schematic!;
  const close = useClose();
  const send = useSend(close);
  const view = strokeView(sch, ids);
  const [width, setWidth] = useState(view.widthUm.mixed ? "" : umToMm(view.widthUm.value));
  const [style, setStyle] = useState<SchLineStyle | "">(view.style.mixed ? "" : view.style.value);
  const [color, setColor] = useState<SchColor>(view.color.mixed ? { r: 0, g: 0, b: 0, a: 255 } : view.color.value);
  const [diameter, setDiameter] = useState(view.diameterUm.mixed ? "" : umToMm(view.diameterUm.value));
  // What the user touched is what is sent: a field left alone keeps each item's own value (KiCad shows `INDETERMINATE_ACTION` for these).
  const [touched, setTouched] = useState<{ width?: boolean; style?: boolean; color?: boolean; diameter?: boolean }>({});
  const touch = (k: keyof typeof touched) => setTouched((t) => ({ ...t, [k]: true }));
  const widthUm = mmToUm(width);
  const diameterUm = mmToUm(diameter);
  const canOk = (!touched.width || widthUm !== null) && (!touched.diameter || diameterUm !== null);
  const onlyJunctions = view.hasJunction && !view.hasStroke;
  const onlyLines = ids.length > 0 && ids.every((id) => (sch.lines ?? []).some((l) => l.id === id));
  const title = onlyJunctions ? (ids.length === 1 ? "Junction Properties" : "Junctions Properties") : onlyLines ? "Line Properties" : "Wire/Bus Properties";
  const change = (): StrokeChange => ({
    ...(touched.width && widthUm !== null && view.hasStroke ? { widthUm } : {}),
    ...(touched.style && style !== "" && view.hasStroke ? { style } : {}),
    ...(touched.color ? { color } : {}),
    ...(touched.diameter && diameterUm !== null && view.hasJunction ? { diameterUm } : {}),
  });
  const submit = () => canOk && void send(strokeEditCmd(ids, change()));
  // The "Default" button (`resetDefaults`): width 0, the layer's colour, the default style, the default dot.
  const reset = () => {
    setWidth("0");
    setStyle("default");
    setColor(UNSPECIFIED_COLOR);
    setDiameter("0");
    setTouched({ width: true, style: true, color: true, diameter: true });
  };
  return (
    <SchDialogShell title={title} width={400} onCancel={close} onOk={submit} okLabel="OK" canOk={canOk}>
      <Grid>
        {view.hasStroke && (
          <>
            <span>Line width (mm)</span>
            <input
              autoFocus
              value={width}
              placeholder={view.widthUm.mixed ? "(mixed)" : "0 = default"}
              onChange={(e) => {
                setWidth(e.target.value);
                touch("width");
              }}
              onKeyDown={(e) => e.key === "Enter" && submit()}
              title="0 is the default line width"
            />
            <span>Line style</span>
            <select
              value={style}
              onChange={(e) => {
                setStyle(e.target.value as SchLineStyle);
                touch("style");
              }}
            >
              {view.style.mixed && <option value="">(mixed)</option>}
              {LINE_STYLES.map((s) => (
                <option key={s.value} value={s.value}>
                  {s.label}
                </option>
              ))}
            </select>
          </>
        )}
        {view.hasJunction && (
          <>
            <span>Junction size (mm)</span>
            <input
              autoFocus={!view.hasStroke}
              value={diameter}
              placeholder={view.diameterUm.mixed ? "(mixed)" : "0 = default"}
              onChange={(e) => {
                setDiameter(e.target.value);
                touch("diameter");
              }}
              onKeyDown={(e) => e.key === "Enter" && submit()}
              title="0 is the default junction size"
            />
          </>
        )}
        <span>Color</span>
        <ColorField
          color={color}
          mixed={view.color.mixed && !touched.color}
          onChange={(c) => {
            setColor(c);
            touch("color");
          }}
        />
      </Grid>
      <div style={{ marginTop: 10 }}>
        <button onClick={reset}>Default</button>
      </div>
    </SchDialogShell>
  );
}

// ---------------------------------------------------------------------------------------------------------------------------------- shape

const FILLS: Array<{ value: SchFill; label: string }> = [
  { value: "none", label: "None" },
  { value: "outline", label: "Outline color" },
  { value: "background", label: "Background color" },
  { value: "color", label: "Custom color" },
];

export function ShapePropertiesDialog({ id }: { id: string }) {
  const g = useStudioState().schematic?.graphics?.find((x) => x.id === id);
  return g ? <ShapeForm key={id} graphic={g} /> : null;
}

function ShapeForm({ graphic }: { graphic: SchGraphic }) {
  const close = useClose();
  const send = useSend(close);
  const rule = graphic.shape.type === "rule_area" ? graphic.shape : null;
  const [width, setWidth] = useState(umToMm(graphic.width_um ?? 0));
  const [style, setStyle] = useState<SchLineStyle>(graphic.line_style ?? "default");
  const [color, setColor] = useState<SchColor>(graphic.color ?? UNSPECIFIED_COLOR);
  const [fill, setFill] = useState<SchFill>(graphic.fill ?? "none");
  const [fillColor, setFillColor] = useState<SchColor>(graphic.fill_color ?? { r: 255, g: 255, b: 194, a: 255 });
  const [flags, setFlags] = useState({ sim: !!rule?.exclude_from_sim, bom: !!rule?.exclude_from_bom, board: !!rule?.exclude_from_board, dnp: !!rule?.dnp });
  const widthUm = mmToUm(width);
  const canOk = widthUm !== null && widthUm >= 0;
  const submit = () => {
    if (!canOk) return;
    const next: SchGraphic = { ...graphic, width_um: widthUm, line_style: style, color: isUnspecified(color) ? undefined : color, fill, fill_color: fill === "color" ? fillColor : undefined };
    if (rule && next.shape.type === "rule_area") next.shape = { ...next.shape, exclude_from_sim: flags.sim, exclude_from_bom: flags.bom, exclude_from_board: flags.board, dnp: flags.dnp };
    void send(graphicsEqual(next, graphic) ? null : { op: "sch_edit", verb: "edit_graphic", id: graphic.id, graphic: next });
  };
  const closed = graphic.shape.type !== "arc" && graphic.shape.type !== "bezier";
  return (
    <SchDialogShell title={rule ? "Rule Area Properties" : "Shape Properties"} width={420} onCancel={close} onOk={submit} okLabel="OK" canOk={canOk}>
      <Grid>
        <span>Border width (mm)</span>
        <input autoFocus value={width} onChange={(e) => setWidth(e.target.value)} onKeyDown={(e) => e.key === "Enter" && submit()} title="0 is the default line width" />
        <span>Border style</span>
        <select value={style} onChange={(e) => setStyle(e.target.value as SchLineStyle)}>
          {LINE_STYLES.map((s) => (
            <option key={s.value} value={s.value}>
              {s.label}
            </option>
          ))}
        </select>
        <span>Border color</span>
        <ColorField color={color} mixed={false} onChange={setColor} />
        {closed && !rule && (
          <>
            <span>Fill</span>
            <select value={fill} onChange={(e) => setFill(e.target.value as SchFill)}>
              {FILLS.map((f) => (
                <option key={f.value} value={f.value}>
                  {f.label}
                </option>
              ))}
            </select>
            {fill === "color" && (
              <>
                <span>Fill color</span>
                <input type="color" value={colorToHex(fillColor)} onChange={(e) => setFillColor(hexToColor(e.target.value) ?? fillColor)} />
              </>
            )}
          </>
        )}
        {rule && (
          <>
            <span>Attributes</span>
            <span style={{ display: "flex", flexDirection: "column", gap: 2 }}>
              {(
                [
                  ["sim", "Exclude from simulation"],
                  ["bom", "Exclude from bill of materials"],
                  ["board", "Exclude from board"],
                  ["dnp", "Do not populate"],
                ] as const
              ).map(([k, label]) => (
                <label key={k}>
                  <input type="checkbox" checked={flags[k]} onChange={(e) => setFlags((f) => ({ ...f, [k]: e.target.checked }))} /> {label}
                </label>
              ))}
            </span>
          </>
        )}
      </Grid>
    </SchDialogShell>
  );
}
