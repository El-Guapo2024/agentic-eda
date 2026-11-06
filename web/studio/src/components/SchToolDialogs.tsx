// The small dialogs the schematic edit and drawing tools open (`StudioState.schToolDialog`): a drawn text box's text and look
// (`DIALOG_TEXT_PROPERTIES` as `DrawShape` opens it), a directive label's fields (`DIALOG_LABEL_PROPERTIES` as `createNewLabel` opens it, before the
// label rides the cursor).
import { useState, type ReactNode } from "react";
import type { DirectiveShape, SchColor, SchFill, SchGraphic, SchHAlign, SchLineStyle, SchToolDialog, SchVAlign } from "../api/schEditTypes";
import { chosenFont, DEFAULT_FONT, HYPERLINK_ERROR, oneStep, textAttrsCmd, validHyperlink } from "../kicad-port/textLooks";
import { ColorSwatch, FontSelect, LinkField, useFontFamilies, useLinkChoices } from "./TextLookControls";
import { graphicsEqual } from "../kicad-port/schProperties";
import { useStudioApi, useStudioDispatch, useStudioState } from "../state/store";
import { SchDialogShell } from "./SchDialogShell";
import { ChangeSymbolsDialog } from "./SchChangeSymbolsDialog";
import { FieldPropertiesDialog } from "./SchFieldDialog";
import { GlobalEditDialog } from "./SchGlobalEditDialog";
import { PowerSymbolPropertiesDialog } from "./PowerSymbolPropertiesDialog";
import { SheetPinPropertiesDialog } from "./SheetPinPropertiesDialog";
import { CleanupPinsDialog, SyncPinsDialog } from "./SchPinDialogs";
import { LabelPropertiesDialog, SheetPropertiesDialog, ShapePropertiesDialog, StrokePropertiesDialog, TextPropertiesDialog } from "./SchPropertiesDialogs";
import { SchImageDialog } from "./SchImageDialog";
import { commitGraphic } from "./schematic/schShapeTools";
import { lastPlaced, rememberPlaced } from "../kicad-port/schLastPlaced";
import { poleOfSpin, spinOfPole, type CarriedDirective } from "../kicad-port/schPlaceTool";
import { carryItem, changeCarried } from "../actions/schPlaceActions";
import { getSchCarry, setSchCarry } from "../state/schCarryStore";
import { setSymbolUnitCmd } from "../kicad-port/schSymbolMenus";

/**
 * `SCH_EDIT_FRAME::SelectUnit` when the unit chosen is already placed (picksymbol.cpp): "Unit Already Placed", "Symbol unit '%s' is already placed (on sheet
 * '%s')", with "Swap '%s' and '%s'" (`wxID_YES`: that symbol takes the unit this one had, "Swap Units") and "Duplicate '%s'" (`wxID_NO`) and Cancel.
 * **Differs**: Duplicate is offered disabled -- two placements of one unit of a reference on a sheet are one the model cannot hold (a placement's key is the
 * reference and the unit) -- and there is no "Do not show again".
 */
function UnitPlacedDialog({ dialog }: { dialog: Extract<SchToolDialog, { kind: "unit_placed" }> }) {
  const api = useStudioApi();
  const dispatch = useStudioDispatch();
  const close = () => dispatch({ type: "SET_SCH_TOOL_DIALOG", dialog: null });
  return (
    <SchDialogShell title="Unit Already Placed" width={420} onCancel={close}>
      <p style={{ margin: 0 }} data-testid="unit-placed-message">
        {dialog.message}
      </p>
      <div style={{ display: "flex", gap: 6, marginTop: 12, justifyContent: "flex-end" }}>
        <button
          className="primary"
          data-testid="unit-placed-swap"
          onClick={() => {
            close();
            void api.cmd(setSymbolUnitCmd(dialog.id, dialog.unit, dialog.newUnit, true));
          }}
        >
          {dialog.swapLabel}
        </button>
        <button disabled title="A sheet holds one placement of each unit of a reference here, so the unit cannot be placed twice.">
          {dialog.duplicateLabel}
        </button>
      </div>
    </SchDialogShell>
  );
}

const DEFAULT_TEXT_SIZE_NM = 1_270_000; // SCHEMATIC_SETTINGS::m_DefaultTextSize, 50 mil
const DIRECTIVE_POLE_NM = 2_540_000; // SCH_DIRECTIVE_LABEL's default pin length, 100 mil

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
      return <FieldPropertiesDialog key={`pf:${dialog.id}`} id={dialog.id} carried={dialog.carried} />;
    case "props_power":
      return <PowerSymbolPropertiesDialog key={`pw:${dialog.id}`} id={dialog.id} carried={dialog.carried} />;
    case "props_text":
      return <TextPropertiesDialog key={`pt:${dialog.id}`} id={dialog.id} />;
    case "props_sheet":
      return <SheetPropertiesDialog key={`ps:${dialog.id}`} id={dialog.id} />;
    case "props_sheet_pin":
      return <SheetPinPropertiesDialog key={`psp:${dialog.id}`} id={dialog.id} />;
    case "props_stroke":
      return <StrokePropertiesDialog key={`pk:${dialog.ids.join(",")}`} ids={dialog.ids} />;
    case "unit_placed":
      return <UnitPlacedDialog key={`up:${dialog.id}:${dialog.unit}:${dialog.newUnit}`} dialog={dialog} />;
    case "props_graphic": {
      const g = state.schematic.graphics?.find((x) => x.id === dialog.id);
      if (!g) return null;
      if (g.shape.type === "text_box") return <TextBoxDialog key={`pg:${g.id}`} dialog={{ kind: "text_box", start: g.shape.start, end: g.shape.end }} edit={g} />;
      if (g.shape.type === "directive") return <DirectiveDialog key={`pg:${g.id}`} dialog={{ kind: "directive", at: g.shape.at }} edit={g} />;
      if (g.shape.type === "image") return <SchImageDialog key={`pg:${g.id}`} id={g.id} />;
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
  // a new text box starts at Schematic Setup's default text size (`DrawShape`: `textbox->SetTextSize( VECTOR2I( sch_settings.m_DefaultTextSize, .. ) )`)
  const [size, setSize] = useState(box?.size_nm ?? state.schematic?.setup?.default_text_size_nm ?? DEFAULT_TEXT_SIZE_NM);
  const [bold, setBold] = useState(box?.bold ?? false);
  const [italic, setItalic] = useState(box?.italic ?? false);
  const [h, setH] = useState<SchHAlign>(box?.h_align ?? "left");
  const [v, setV] = useState<SchVAlign>(box?.v_align ?? "top");
  const [vertical, setVertical] = useState((box?.angle ?? 0) === 90_000);
  const [width, setWidth] = useState(edit?.width_nm ?? 0);
  const [style, setStyle] = useState<SchLineStyle>(edit?.line_style ?? "default");
  const [fill, setFill] = useState<SchFill>(edit?.fill ?? "none");
  // the border's and the fill's colours (`m_borderColorSwatch`, `m_fillColorSwatch`), and for a text box that exists, its text's font, colour, link and
  // "Exclude from simulation" (`SchExtras::text_attrs`, `set_text_attrs`, UI-LEFT S6). **Differs**: a text box being drawn gets these after it is placed.
  const [borderColor, setBorderColor] = useState<SchColor | null>(edit?.color && edit.color.a > 0 ? edit.color : null);
  const [fillColor, setFillColor] = useState<SchColor | null>(edit?.fill_color && edit.fill_color.a > 0 ? edit.fill_color : null);
  const attrs = edit ? state.schematic?.graphic_text_attrs?.[edit.id] : undefined;
  const families = useFontFamilies();
  const links = useLinkChoices();
  const [font, setFont] = useState(attrs?.font || DEFAULT_FONT);
  const [color, setColor] = useState<SchColor | null>(attrs?.color ?? null);
  const [href, setHref] = useState(attrs?.href ?? "");
  const [excludeFromSim, setExcludeFromSim] = useState(!!attrs?.exclude_from_sim);
  const [problem, setProblem] = useState<string | null>(null);
  const close = () => dispatch({ type: "SET_SCH_TOOL_DIALOG", dialog: null });
  const submit = () => {
    if (!text.trim() || !state.schematic) return;
    if (!validHyperlink(href.trim())) return setProblem(HYPERLINK_ERROR);
    const shape = { type: "text_box" as const, start: dialog.start, end: dialog.end, text, angle: vertical ? 90_000 : 0, size_nm: size, bold, italic, h_align: h, v_align: v, margin_nm: box?.margin_nm };
    const colors: Partial<SchGraphic> = { color: borderColor ?? undefined, fill_color: fillColor ?? undefined };
    if (edit) {
      const { color: _c, fill_color: _f, ...rest } = edit;
      const next: SchGraphic = { ...rest, shape, width_nm: width, line_style: style, fill, ...(borderColor ? { color: borderColor } : {}), ...(fillColor ? { fill_color: fillColor } : {}) };
      const cmd = oneStep([
        graphicsEqual(next, edit) ? null : { op: "sch_edit", verb: "edit_graphic", id: edit.id, graphic: next },
        textAttrsCmd(edit.id, { font: attrs?.font, color: attrs?.color, href: attrs?.href, excludeFromSim: attrs?.exclude_from_sim }, { font: chosenFont(font), color, href, excludeFromSim }, false),
      ]);
      if (!cmd) return close();
      void api.cmd(cmd).then((ok) => ok && close());
      return;
    }
    commitGraphic({ sch: state.schematic, dispatch, api }, { shape, width_nm: width, line_style: style, fill, ...colors });
    close();
  };
  return (
    <Shell title="Text Box Properties" onCancel={close} onOk={submit} okLabel="OK" canOk={text.trim().length > 0 && size >= 10_000 && size <= 1_000_000_000}>
      <span>Text</span>
      <textarea autoFocus rows={4} value={text} onChange={(e) => setText(e.target.value)} />
      {edit && (
        <>
          <span />
          <LinkField href={href} onChange={(h2) => (setHref(h2), setProblem(null))} choices={links} />
          <span />
          <label>
            <input type="checkbox" aria-label="Exclude from simulation" checked={excludeFromSim} onChange={(e) => setExcludeFromSim(e.target.checked)} /> Exclude from simulation
          </label>
          <span>Font:</span>
          <FontSelect families={families} current={attrs?.font} value={font} onChange={setFont} />
          <span>Color:</span>
          <ColorSwatch label="Text color" color={color} onChange={setColor} />
        </>
      )}
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
        <option value="hatch">Hatch</option>
        <option value="reverse_hatch">Reverse hatch</option>
        <option value="cross_hatch">Cross-hatch</option>
      </select>
      <span>Border color:</span>
      <ColorSwatch label="Border color" color={borderColor} onChange={setBorderColor} />
      <span>Fill color:</span>
      <ColorSwatch label="Fill color" color={fillColor} onChange={setFillColor} />
      {problem && (
        <>
          <span />
          <p style={{ color: "var(--chrome-danger)", fontSize: 11, margin: 0 }} data-testid="text-box-properties-error">
            {problem}
          </p>
        </>
      )}
    </Shell>
  );
}

const ORIENTATIONS: Array<{ label: string; millideg: number }> = [
  { label: "Right", millideg: 0 },
  { label: "Up", millideg: 90_000 },
  { label: "Left", millideg: 180_000 },
  { label: "Down", millideg: 270_000 },
];

/**
 * A new directive label's fields (`createNewLabel` -> `DIALOG_LABEL_PROPERTIES`): OK puts the label in the hand, riding the cursor until the click that
 * places it (actions/schPlaceActions.ts), and Properties (`E`) of the one in the hand opens this again on it. It starts with the tools' last spin and the
 * last directive shape (`m_lastTextOrientation`, `m_lastNetClassFlagShape`) and leaves them for the next. With `edit`, that placed directive label's
 * Properties: its fields start from the label's own and OK replaces it in place.
 */
function DirectiveDialog({ dialog, edit }: { dialog: Extract<SchToolDialog, { kind: "directive" }>; edit?: SchGraphic }) {
  const state = useStudioState();
  const dispatch = useStudioDispatch();
  const api = useStudioApi();
  const flag = edit?.shape.type === "directive" ? edit.shape : null;
  const carried = !edit ? getSchCarry() : null;
  const inHand = carried?.editing && carried.item.kind === "directive" ? carried.item : null;
  const last = lastPlaced();
  const [netclass, setNetclass] = useState(flag?.netclass ?? inHand?.netclass ?? "");
  const [componentClass, setComponentClass] = useState(flag?.component_class ?? inHand?.componentClass ?? "");
  const [shape, setShape] = useState<DirectiveShape>(flag?.shape ?? inHand?.shape ?? last.directiveShape); // m_lastNetClassFlagShape starts as F_ROUND
  const [orientation, setOrientation] = useState(flag?.orientation ?? inHand?.orientation ?? poleOfSpin(last.spin)); // m_lastTextOrientation starts as SPIN_STYLE::RIGHT
  const close = () => {
    const now = getSchCarry();
    if (!edit && now?.editing) setSchCarry({ ...now, editing: false });
    dispatch({ type: "SET_SCH_TOOL_DIALOG", dialog: null });
  };
  const submit = () => {
    if (!state.schematic) return;
    if (edit && flag) {
      const next: SchGraphic = { ...edit, shape: { ...flag, orientation, shape, netclass, component_class: componentClass } };
      if (graphicsEqual(next, edit)) return close();
      void api.cmd({ op: "sch_edit", verb: "edit_graphic", id: edit.id, graphic: next }).then((ok) => ok && close());
      return;
    }
    const item: CarriedDirective = { kind: "directive", netclass, componentClass, shape, orientation, pinLengthNm: inHand?.pinLengthNm ?? DIRECTIVE_POLE_NM };
    dispatch({ type: "SET_SCH_TOOL_DIALOG", dialog: null });
    if (inHand) {
      changeCarried(item);
      return;
    }
    // `m_lastTextOrientation = labelItem->GetSpinStyle()`, `m_lastNetClassFlagShape = labelItem->GetShape()`
    rememberPlaced({ spin: spinOfPole(orientation), directiveShape: shape });
    carryItem({ api }, "sch_directive", item, [dialog.at.x, dialog.at.y]);
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
