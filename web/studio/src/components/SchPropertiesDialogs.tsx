// Properties (`E`, a double-click) dialogs of the schematic items that had none once placed (`SCH_EDIT_TOOL::Properties`, eeschema/tools/sch_edit_tool.cpp):
// a label (`DIALOG_LABEL_PROPERTIES`), a free text (`DIALOG_TEXT_PROPERTIES`), a hierarchical sheet (`DIALOG_SHEET_PROPERTIES`), wires / buses / bus entries /
// graphic lines / junctions (`DIALOG_WIRE_BUS_PROPERTIES`, `DIALOG_LINE_PROPERTIES`, `DIALOG_JUNCTION_PROPS`) and a drawn shape or rule area
// (`DIALOG_SHAPE_PROPERTIES`). A text box and a directive label reopen the dialogs that created them (SchToolDialogs.tsx).
//
// Each OK sends one command (kicad-port/schProperties.ts builds it from what the user changed), so it is one undo step; nothing is sent when nothing changed.
// A free text's and a label's font, colour, a text's bold, italic, link and "Exclude from simulation" (UI-LEFT S6) are `set_text_attrs` (kicad-port/textLooks.ts),
// batched with the dialog's other command. A label's own fields (its net class, ...) are the grid of Label Properties; a global label's Sheet References row is
// shown, Schematic Setup shows or hides it.
import { useState, type ReactNode } from "react";
import type { Cmd, LabelShape, Schematic, SchematicLabel, SchematicText, Sheet } from "../api/types";
import type { SchColor, SchFill, SchGraphic, SchLineStyle } from "../api/schEditTypes";
import { colorToHex, graphicsEqual, hexToColor, isUnspecified, labelEditCmd, mmToNm, sheetEditCmd, strokeEditCmd, strokeView, textEditCmd, UNSPECIFIED_COLOR, nmToMm, type LabelSpinName, type StrokeChange } from "../kicad-port/schProperties";
import { useStudioApi, useStudioDispatch, useStudioState } from "../state/store";
import { SchDialogShell } from "./SchDialogShell";
import { LabelFieldsGrid, labelRowsOk } from "./LabelFieldsGrid";
import { itemColorsOverridden, SchColorOverrideNote } from "./SchColorOverrideNote";
import { noteAnswer, rememberedAnswer } from "../kicad-port/kidialog";
import { inferSpin } from "./schematic/labelShape";
import { labelOrientationForPoint, orientationSceneOf } from "../kicad-port/schAutoRotate";
import { textLie } from "../kicad-port/schTextItem";
import type { SchHAlign, SchVAlign } from "../api/schEditTypes";
import { chosenFont, DEFAULT_FONT, HYPERLINK_ERROR, oneStep, textAttrsCmd, validHyperlink } from "../kicad-port/textLooks";
import { ColorSwatch, FontSelect, LinkField, useFontFamilies, useLinkChoices } from "./TextLookControls";
import { useExistingLabels } from "./LabelDialog";
import { sheetFlags } from "../kicad-port/designVariants";
import { newSheetFieldRow, sheetFieldCmds, sheetFieldError, sheetFieldRow, SHEET_FIELD_COLUMNS, type SheetFieldRow } from "../kicad-port/sheetFieldLooks";
import { columnMenu, DEFAULT_SHOWN_COLUMNS, shownColumns, toggleColumn, type FieldColumn } from "../kicad-port/symbolFieldsGrid";
import { fontChoices } from "../kicad-port/symbolPropsDialog";
import { labelFieldEditOf, labelGridRows, labelLookCmds, labelPlaceCmds, type LabelGridRow } from "../kicad-port/labelFieldsGrid";
import { lookCellOf } from "./fieldGridCells";
import { ContextMenu } from "./canvas/ContextMenu";

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
  return <LabelForm label={label} spin={label.spin ?? inferSpin(sch.wires, label.at)} sch={sch} />;
}

function LabelForm({ label, spin, sch }: { label: SchematicLabel; spin: LabelSpinName; sch: Schematic }) {
  const close = useClose();
  const send = useSend(close);
  const [text, setText] = useState(label.net);
  // the Label combo box's list (`m_existingLabelArray`): every screen's labels of the kind, the power nets like them, the bus aliases
  const existing = useExistingLabels(label.scope, label.id);
  const [shape, setShape] = useState<LabelShape>(label.shape ?? "input");
  const [turn, setTurn] = useState<LabelSpinName>(spin);
  // the "Auto" check of a global or hierarchical label (`SCH_LABEL_BASE::AutoRotateOnPlacement`; a plain label does not have it)
  const supportsAuto = label.scope !== "local";
  const [auto, setAuto] = useState(supportsAuto && !!label.auto_rotate);
  const [size, setSize] = useState(nmToMm(label.size_nm && label.size_nm > 0 ? label.size_nm : 1_270_000));
  const [bold, setBold] = useState(!!label.bold);
  const [italic, setItalic] = useState(!!label.italic);
  // the Font choice and the Color swatch (`m_fontCtrl`, `m_textColorSwatch`), `set_text_attrs`
  const families = useFontFamilies();
  const [font, setFont] = useState(label.font || DEFAULT_FONT);
  const [color, setColor] = useState(label.color ?? null);
  // the grid of the label's fields (`m_grid` over `m_fields`, kicad-port/labelFieldsGrid.ts): a global label's Sheet References first, then its own
  const [rows, setRows] = useState<LabelGridRow[]>(() => labelGridRows((label.fields ?? []).filter((f) => !f.intersheet_refs || label.scope === "global"), label.at));
  const own = rows.filter((r) => !r.isr);
  const fieldSizesOk = labelRowsOk(rows);
  const title = label.scope === "global" ? "Global Label Properties" : label.scope === "hierarchical" ? "Hierarchical Label Properties" : "Label Properties";
  const sizeNm = mmToNm(size);
  // `m_textSize.Validate( 0.01, 1000.0, EDA_UNITS::MM )`: "Don't allow text to disappear"
  const sizeOk = sizeNm !== null && sizeNm >= 10_000 && sizeNm <= 1_000_000_000;
  // "Label can not be empty." (`DIALOG_LABEL_PROPERTIES::TransferDataFromWindow`)
  const canOk = text.trim().length > 0 && sizeOk && fieldSizesOk;
  // while "Auto" is on the label takes the spin of what it is connected to (`AutoRotateItem`): the select shows the one it will get
  const autoSpin = auto ? labelOrientationForPoint(orientationSceneOf(sch), label.at, turn) : null;
  // `TransferDataFromWindow`: the places first (`positioningChanged` takes the fields out of the autoplaced ones, each keeping its grid place), the label and its
  // fields' texts and looks, then the fonts and colours (kicad-port/labelFieldsGrid.ts)
  const submit = () =>
    canOk &&
    void send(
      oneStep([
        ...labelPlaceCmds(label.id, rows),
        labelEditCmd(label, spin, { text, shape, spin: turn, sizeNm, bold, italic, autoRotate: supportsAuto ? auto : undefined, fields: own.map((r) => ({ ...labelFieldEditOf(r), name: r.name === "Net Class" ? "Netclass" : r.name })) }),
        textAttrsCmd(label.id, { font: label.font, color: label.color }, { font: chosenFont(font), color }, false),
        ...labelLookCmds(label.id, rows),
      ])
    );
  return (
    <SchDialogShell title={title} width={520} onCancel={close} onOk={submit} okLabel="OK" canOk={canOk}>
      <Grid>
        <span>Label</span>
        <input autoFocus aria-label="Label" list={existing.length > 0 ? "label-props-existing" : undefined} value={text} onChange={(e) => setText(e.target.value)} onKeyDown={(e) => e.key === "Enter" && submit()} />
        {existing.length > 0 && (
          <datalist id="label-props-existing">
            {existing.map((n) => (
              <option key={n} value={n} />
            ))}
          </datalist>
        )}
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
        <select aria-label="Orientation" value={autoSpin ?? turn} disabled={auto} title={auto ? "Auto: the label takes the spin of what it is connected to" : undefined} onChange={(e) => setTurn(e.target.value as LabelSpinName)}>
          {SPINS.map((s) => (
            <option key={s.value} value={s.value}>
              {s.label}
            </option>
          ))}
        </select>
        {supportsAuto && (
          <>
            <span>Auto</span>
            <label>
              <input type="checkbox" aria-label="Auto" checked={auto} onChange={(e) => setAuto(e.target.checked)} /> Rotate to what it is connected to
            </label>
          </>
        )}
        <span>Font:</span>
        <FontSelect families={families} current={label.font} value={font} onChange={setFont} />
        <span>Text size (mm)</span>
        <input value={size} onChange={(e) => setSize(e.target.value)} onKeyDown={(e) => e.key === "Enter" && submit()} style={sizeOk ? undefined : { outline: "1px solid var(--error, #d33)" }} title="0.01 to 1000 mm" />
        <span>Style</span>
        <span style={{ display: "flex", gap: 14 }}>
          <label>
            <input type="checkbox" checked={bold} onChange={(e) => setBold(e.target.checked)} /> Bold
          </label>
          <label>
            <input type="checkbox" checked={italic} onChange={(e) => setItalic(e.target.checked)} /> Italic
          </label>
        </span>
        <span>Color:</span>
        <ColorSwatch label="Label color" color={color} onChange={setColor} />
      </Grid>
      <LabelFieldsGrid rows={rows} setRows={setRows} isrVisible={null} />

    </SchDialogShell>
  );
}

// ---------------------------------------------------------------------------------------------------------------------------------- text

export function TextPropertiesDialog({ id }: { id: string }) {
  const text = useStudioState().schematic?.texts.find((t) => t.id === id);
  return text ? <TextForm key={id} text={text} /> : null;
}

function TextForm({ text }: { text: SchematicText }) {
  const close = useClose();
  const send = useSend(close);
  const lie = textLie(text);
  const [content, setContent] = useState(text.content);
  const [size, setSize] = useState(nmToMm(text.size_nm));
  const [vertical, setVertical] = useState(lie.vertical);
  // how the text sits on its anchor, in its own axes (`m_hAlignLeft` ... `m_vAlignBottom`)
  const [h, setH] = useState<SchHAlign>(lie.h);
  const [v, setV] = useState<SchVAlign>(lie.v);
  // the looks `DIALOG_TEXT_PROPERTIES` sets beyond these (UI-LEFT S6): Font, Bold, Italic, Color, Link, Exclude from simulation
  const families = useFontFamilies();
  const links = useLinkChoices();
  const [font, setFont] = useState(text.font || DEFAULT_FONT);
  const [bold, setBold] = useState(!!text.bold);
  const [italic, setItalic] = useState(!!text.italic);
  const [color, setColor] = useState(text.color ?? null);
  const [href, setHref] = useState(text.href ?? "");
  const [excludeFromSim, setExcludeFromSim] = useState(!!text.exclude_from_sim);
  const [problem, setProblem] = useState<string | null>(null);
  const sizeNm = mmToNm(size);
  // `m_textSize.Validate( 0.01, 1000.0, EDA_UNITS::MM )`: "Don't allow text to disappear"
  const sizeOk = sizeNm !== null && sizeNm >= 10_000 && sizeNm <= 1_000_000_000;
  const canOk = content.length > 0 && sizeOk;
  const submit = () => {
    if (!canOk) return;
    // `EDA_TEXT::ValidateHyperlink`: the dialog stays open on a link it refuses
    if (!validHyperlink(href.trim())) return setProblem(HYPERLINK_ERROR);
    void send(
      oneStep([
        textEditCmd(text, { content, sizeNm, vertical, h, v }),
        textAttrsCmd(text.id, { font: text.font, color: text.color, href: text.href, bold: text.bold, italic: text.italic, excludeFromSim: text.exclude_from_sim }, { font: chosenFont(font), color, href, bold, italic, excludeFromSim }, true),
      ])
    );
  };
  return (
    <SchDialogShell title="Text Properties" width={460} onCancel={close} onOk={submit} okLabel="OK" canOk={canOk}>
      <Grid>
        <span>Text</span>
        <textarea autoFocus rows={4} value={content} onChange={(e) => setContent(e.target.value)} />
        <span />
        <LinkField href={href} onChange={(h2) => (setHref(h2), setProblem(null))} choices={links} />
        <span />
        <label>
          <input type="checkbox" aria-label="Exclude from simulation" checked={excludeFromSim} onChange={(e) => setExcludeFromSim(e.target.checked)} /> Exclude from simulation
        </label>
        <span>Font:</span>
        <FontSelect families={families} current={text.font} value={font} onChange={setFont} />
        <span>Style</span>
        <span style={{ display: "flex", gap: 14 }}>
          <label>
            <input type="checkbox" aria-label="Bold" checked={bold} onChange={(e) => setBold(e.target.checked)} /> Bold
          </label>
          <label>
            <input type="checkbox" aria-label="Italic" checked={italic} onChange={(e) => setItalic(e.target.checked)} /> Italic
          </label>
        </span>
        <span>Color:</span>
        <ColorSwatch label="Text color" color={color} onChange={setColor} />
        <span>Text size (mm)</span>
        <input value={size} onChange={(e) => setSize(e.target.value)} style={sizeOk ? undefined : { outline: "1px solid var(--error, #d33)" }} title="0.01 to 1000 mm" />
        <span>Orientation</span>
        <select aria-label="Orientation" value={vertical ? "v" : "h"} onChange={(e) => setVertical(e.target.value === "v")}>
          <option value="h">Horizontal</option>
          <option value="v">Vertical</option>
        </select>
        <span>Horizontal justification</span>
        <select aria-label="Horizontal justification" value={h} onChange={(e) => setH(e.target.value as SchHAlign)}>
          <option value="left">Left</option>
          <option value="center">Center</option>
          <option value="right">Right</option>
        </select>
        <span>Vertical justification</span>
        <select aria-label="Vertical justification" value={v} onChange={(e) => setV(e.target.value as SchVAlign)}>
          <option value="top">Top</option>
          <option value="center">Center</option>
          <option value="bottom">Bottom</option>
        </select>
      </Grid>
      {problem && (
        <p style={{ color: "var(--chrome-danger)", fontSize: 11, marginTop: 8 }} data-testid="text-properties-error">
          {problem}
        </p>
      )}
    </SchDialogShell>
  );
}

// ---------------------------------------------------------------------------------------------------------------------------------- sheet

export function SheetPropertiesDialog({ id }: { id: string }) {
  const sheet = useStudioState().schematic?.sheets.find((s) => s.id === id);
  return sheet ? <SheetForm key={id} sheet={sheet} /> : null;
}

/** The look the writer has always given a sheet with no style of its own (`SheetProps::legacy`): a 6 mil border, a pale yellow fill. */
/** `DIALOG_SHEET_PROPERTIES::TransferDataFromWindow`'s warning, word for word (`checkboxLabel` is the Colors page's `m_optOverrideColors` label). */
const SHEET_COLOR_WARNING = "Note: item colors are overridden in the current color theme.";
const SHEET_COLOR_WARNING_DETAIL = "To see individual item colors uncheck 'Override individual item colors'\nin Preferences > Schematic Editor > Colors.";
/** `DoNotShowCheckbox( __FILE__, __LINE__ )`: the question's hash. */
const SHEET_COLOR_WARNING_KEY = "dialog_sheet_properties.cpp:item-colors-overridden";

const LEGACY_SHEET = { border_width_nm: 152_400, border_color: null as SchColor | null, fill_color: { r: 255, g: 255, b: 194, a: 255 } as SchColor | null };


/**
 * Sheet Properties (`DIALOG_SHEET_PROPERTIES`): the name, file and page, the user fields (`SCH_FIELD`s of `FIELD_T::SHEET_USER`: Add Field gives `Field<N>`,
 * `GetUserFieldName( m_fields->size() )`, hidden; Move Up / Down; Delete), the Attributes (Exclude from simulation, bill of materials, board; Do not populate)
 * and the Style (the border's width and colour, the fill colour). OK is one step: `edit_sheet` / `set_sheet_page`, `set_sheet_fields`, `set_sheet_props`.
 */
function SheetForm({ sheet }: { sheet: Sheet }) {
  const here = useStudioState().currentSheetPath;
  // the Attributes as the current design variant has them (`m_cbDNP->SetValue( m_sheet->GetDNP( &instance, variantName ) )` ...; UI-LEFT-2 SS15)
  const variant = useStudioState().schVariant;
  const flags = sheetFlags(sheet, variant);
  const close = useClose();
  const send = useSend(close);
  const [name, setName] = useState(sheet.name);
  const [file, setFile] = useState(sheet.file);
  const [page, setPage] = useState(sheet.page ?? "");
  const own = sheet.props ?? null;
  // the user fields' rows (`FIELDS_GRID_TABLE`): the texts and every look (UI-LEFT-2 SS15)
  // in a variant a field's text is the variant's (`m_sheet->GetFieldText( field->GetName(), &instance, variantName )`, KiCad 25ec2335; UI-LEFT-2 SS16)
  const variantTexts = (variant !== "" ? sheet.variants?.[variant]?.fields : undefined) ?? {};
  const [fields, setFields] = useState<SheetFieldRow[]>(() => (sheet.user_fields ?? []).map((f) => sheetFieldRow({ ...f, text: variantTexts[f.name] ?? f.text }, sheet.user_field_looks?.[f.name])));
  // `m_grid->ShowHideColumns( "0 1 2 3 4 5 6 7" )`; a right-click on the column labels shows or hides the others
  const [shownCols, setShownCols] = useState<Set<FieldColumn>>(() => shownColumns(DEFAULT_SHOWN_COLUMNS));
  const [columnsMenu, setColumnsMenu] = useState<{ x: number; y: number } | null>(null);
  const fontList = fontChoices(useFontFamilies());
  const [excludeFromSim, setExcludeFromSim] = useState(flags.exclude_from_sim);
  const [excludeFromBom, setExcludeFromBom] = useState(flags.exclude_from_bom);
  // `GetExcludedFromBoard()` and `SetExcludedFromBoard( value )`: the sheet's own, in every variant
  const [excludeFromBoard, setExcludeFromBoard] = useState(!!own?.exclude_from_board);
  const [dnp, setDnp] = useState(flags.dnp);
  const [border, setBorder] = useState(nmToMm(own ? (own.border_width_nm ?? 0) : LEGACY_SHEET.border_width_nm));
  const [borderColor, setBorderColor] = useState<SchColor | null>(own ? (own.border_color ?? null) : LEGACY_SHEET.border_color);
  const [fillColor, setFillColor] = useState<SchColor | null>(own ? (own.fill_color ?? null) : LEGACY_SHEET.fill_color);
  const [problem, setProblem] = useState<string | null>(null);
  // A sheet must have a name and a valid file name; a page number is letters and digits only (`wxFILTER_ALPHANUMERIC`).
  const fileOk = file.trim().length > 0 && !/[\\/]/.test(file);
  const pageOk = /^[\p{L}\p{N}]*$/u.test(page.trim());
  const borderNm = mmToNm(border);
  const canOk = name.trim().length > 0 && fileOk && pageOk && borderNm !== null && borderNm >= 0;
  const setField = (i: number, change: Partial<SheetFieldRow>) => setFields((fs) => fs.map((f, k) => (k === i ? { ...f, ...change } : f)));
  const move = (i: number, by: -1 | 1) =>
    setFields((fs) => {
      const j = i + by;
      if (j < 0 || j >= fs.length) return fs;
      const next = [...fs];
      [next[i], next[j]] = [next[j]!, next[i]!];
      return next;
    });
  const submit = () => {
    if (!canOk) return;
    // `Validate`: "Fields must have a name." for a field with a text and no name
    if (fields.some((f) => f.name.trim() === "" && f.text !== "")) return setProblem("Fields must have a name.");
    const names = fields.filter((f) => f.name.trim() !== "").map((f) => f.name.trim());
    const dup = names.find((n, i) => names.indexOf(n) !== i || ["sheetname", "sheetfile"].includes(n.toLowerCase()));
    if (dup) return setProblem(`Field name '${dup}' already in use.`);
    const sizeProblem = sheetFieldError(fields);
    if (sizeProblem) return setProblem(sizeProblem);
    // in a variant an existing field's text goes to the variant (`tmp->SetText( variantText, &instance, variantName )`) and the field keeps its own (`SetText( defaultText,
    // &instance )`); a new field is the sheet's
    const variantFields: Record<string, string> = {};
    const baseRows = variant
      ? fields.map((r) => {
          const own = (sheet.user_fields ?? []).find((f) => f.name === r.name.trim());
          if (!own) return r;
          if (r.text !== (variantTexts[own.name] ?? own.text)) variantFields[own.name] = r.text;
          return { ...r, text: own.text };
        })
      : fields;
    const fieldCmds = sheetFieldCmds(sheet.id, baseRows, sheet.user_fields ?? [], sheet.user_field_looks ?? {});
    const was2 = own ?? LEGACY_SHEET;
    const props: Record<string, unknown> = {};
    const sameColor = (a: SchColor | null | undefined, b: SchColor | null | undefined) => JSON.stringify(a && a.a > 0 ? a : null) === JSON.stringify(b && b.a > 0 ? b : null);
    if (borderNm !== (was2.border_width_nm ?? 0)) props.border_width_nm = borderNm;
    if (!sameColor(borderColor, was2.border_color)) props.border_color = borderColor ?? UNSPECIFIED_COLOR;
    if (!sameColor(fillColor, was2.fill_color)) props.fill_color = fillColor ?? UNSPECIFIED_COLOR;
    // `SetExcludedFromSim( .., &instance, variantName )`, `SetExcludedFromBOM`, `SetDNP`: the variant's record while one is current (`set_sheet_variant`)
    const inVariant: Record<string, unknown> = {};
    const flag = (key: "exclude_from_sim" | "exclude_from_bom" | "dnp", value: boolean) => {
      if (value === flags[key]) return;
      if (variant) inVariant[key] = value;
      else props[key] = value;
    };
    flag("exclude_from_sim", excludeFromSim);
    flag("exclude_from_bom", excludeFromBom);
    flag("dnp", dnp);
    if (excludeFromBoard !== !!own?.exclude_from_board) props.exclude_from_board = excludeFromBoard;
    const propsCmd = Object.keys(props).length > 0 ? ({ op: "sch_edit", verb: "set_sheet_props", id: sheet.id, ...props } as unknown as Cmd) : null;
    if (Object.keys(variantFields).length > 0) inVariant.fields = variantFields;
    const variantCmd = Object.keys(inVariant).length > 0 ? ({ op: "sch_edit", verb: "set_sheet_variant", id: sheet.id, variant, ...inVariant } as unknown as Cmd) : null;
    const edit = sheetEditCmd(sheet, { name, file, page }, [...here, sheet.id]);
    const editCmds: Cmd[] = edit ? ((edit as { op: string; cmds?: Cmd[] }).op === "batch" ? ((edit as { cmds: Cmd[] }).cmds ?? []) : [edit]) : [];
    const go = () => void send(oneStep([propsCmd, variantCmd, ...fieldCmds, ...editCmds]));
    // `TransferDataFromWindow`: a border or background colour set while the theme overrides the items' colours is warned of first (`KIDIALOG` with
    // `KD_WARNING` and `DoNotShowCheckbox`); its only button goes on
    const colorsSet = !sameColor(borderColor, was2.border_color) || !sameColor(fillColor, was2.fill_color);
    if (colorsSet && itemColorsOverridden("schematic") && rememberedAnswer(SHEET_COLOR_WARNING_KEY) === undefined) return setColorWarning(() => go);
    go();
  };
  const [colorWarning, setColorWarning] = useState<(() => void) | null>(null);
  const [warningTicked, setWarningTicked] = useState(false);
  return (
    <SchDialogShell title="Sheet Properties" width={560} onCancel={close} onOk={submit} okLabel="OK" canOk={canOk} note={<SchColorOverrideNote />}>
      {colorWarning && (
        <div className="dialog-backdrop" onClick={(e) => e.stopPropagation()}>
          <div className="dialog" style={{ width: 440 }} role="dialog" aria-label="Warning" data-testid="sheet-color-warning">
            <div className="dialog-header">
              <span>Warning</span>
            </div>
            <div className="dialog-body" style={{ whiteSpace: "pre-wrap" }}>
              {SHEET_COLOR_WARNING}
              <p style={{ fontSize: 11, color: "var(--chrome-text-dim)" }}>{SHEET_COLOR_WARNING_DETAIL}</p>
              <label style={{ display: "block", marginTop: 10 }}>
                <input type="checkbox" checked={warningTicked} onChange={(e) => setWarningTicked(e.target.checked)} data-testid="do-not-show-again" /> Do not show again
              </label>
            </div>
            <div className="dialog-footer">
              <button
                className="primary"
                autoFocus
                onClick={() => {
                  noteAnswer(SHEET_COLOR_WARNING_KEY, warningTicked, true);
                  const go = colorWarning;
                  setColorWarning(null);
                  go();
                }}
              >
                OK
              </button>
            </div>
          </div>
        </div>
      )}
      <Grid>
        <span>Sheet name</span>
        <input autoFocus value={name} onChange={(e) => setName(e.target.value)} onKeyDown={(e) => e.key === "Enter" && submit()} />
        <span>Sheet file name</span>
        <input value={file} onChange={(e) => setFile(e.target.value)} onKeyDown={(e) => e.key === "Enter" && submit()} title="A file the project has is shown; a new name moves this sheet's content to it (or copies it when another sheet shows the old file)" />
      </Grid>
      <datalist id="sheet-props-fonts">
        {fontList.map((f) => (
          <option key={f} value={f} />
        ))}
      </datalist>
      <div style={{ overflowX: "auto", marginTop: 8 }}>
        <table className="label-fields" aria-label="Sheet fields" style={{ width: "100%", borderCollapse: "collapse" }}>
          <thead
            // `GRID_TRICKS::onGridLabelRightClick`: a check item per column
            onContextMenu={(e) => {
              e.preventDefault();
              setColumnsMenu({ x: e.clientX, y: e.clientY });
            }}
            title="Right-click to show or hide columns"
          >
            <tr>
              {SHEET_FIELD_COLUMNS.filter((c) => shownCols.has(c.key)).map((c) => (
                <th key={c.key} data-col={c.key} style={{ textAlign: c.key === "name" || c.key === "value" ? "left" : "center", whiteSpace: "nowrap" }}>
                  {c.label}
                </th>
              ))}
              <th />
            </tr>
          </thead>
          <tbody>
            {fields.map((f, i) => (
              <tr key={i}>
                {SHEET_FIELD_COLUMNS.filter((c) => shownCols.has(c.key)).map((c) => (
                  <td key={c.key} style={c.key === "name" || c.key === "value" ? undefined : { textAlign: "center" }}>
                    {c.key === "name" ? (
                      <input aria-label={`Sheet field ${i + 1} name`} value={f.name} onChange={(e) => setField(i, { name: e.target.value })} style={{ width: 110 }} />
                    ) : c.key === "value" ? (
                      <input aria-label={`Sheet field ${i + 1} value`} value={f.text} onChange={(e) => setField(i, { text: e.target.value })} />
                    ) : c.key === "shown" ? (
                      <input type="checkbox" aria-label={`Sheet field ${i + 1} shown`} checked={f.visible} onChange={(e) => setField(i, { visible: e.target.checked })} />
                    ) : (
                      lookCellOf(`Sheet field ${i + 1}`, c.key, f.look, (patch) => setField(i, { look: { ...f.look, ...patch } }), "sheet-props-fonts")
                    )}
                  </td>
                ))}
                <td style={{ whiteSpace: "nowrap" }}>
                  <button type="button" title="Move up" disabled={i === 0} onClick={() => move(i, -1)}>
                    ↑
                  </button>
                  <button type="button" title="Move down" disabled={i === fields.length - 1} onClick={() => move(i, 1)}>
                    ↓
                  </button>
                  <button type="button" aria-label={`Delete sheet field ${i + 1}`} title="Delete field" onClick={() => setFields((fs) => fs.filter((_, k) => k !== i))}>
                    ✕
                  </button>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
      {columnsMenu && (
        <ContextMenu
          x={columnsMenu.x}
          y={columnsMenu.y}
          entries={columnMenu(shownCols)
            .filter((c) => c.key !== "allow_autoplace")
            .map((c) => ({ label: `${c.checked ? "\u2713" : "\u2003"} ${c.label}`, checked: c.checked, onSelect: () => setShownCols((cols) => toggleColumn(cols, c.key)) }))}
          onClose={() => setColumnsMenu(null)}
        />
      )}
      <button
        type="button"
        aria-label="Add field"
        title="Add field"
        style={{ marginTop: 4 }}
        // `OnAddField`: `GetUserFieldName( m_fields->size() )` over the two mandatory rows and the user ones, hidden
        onClick={() => setFields((fs) => [...fs, newSheetFieldRow(fs, !!sheet.fields?.find((f) => f.name === "Sheetname")?.vertical)])}
      >
        + Add Field
      </button>
      <div style={{ display: "flex", gap: 16, marginTop: 10, flexWrap: "wrap" }}>
        <fieldset style={{ flex: 1, minWidth: 220 }}>
          <legend>Attributes</legend>
          <Grid>
            <span>Page number:</span>
            <input value={page} onChange={(e) => setPage(e.target.value)} onKeyDown={(e) => e.key === "Enter" && submit()} placeholder="by order" />
          </Grid>
          <label style={{ display: "block" }}>
            <input type="checkbox" checked={excludeFromSim} onChange={(e) => setExcludeFromSim(e.target.checked)} /> Exclude from simulation
          </label>
          <label style={{ display: "block" }} title={"This is useful for adding symbols for board footprints such as fiducials\nand logos that you do not want to appear in the bill of materials export"}>
            <input type="checkbox" checked={excludeFromBom} onChange={(e) => setExcludeFromBom(e.target.checked)} /> Exclude from bill of materials
          </label>
          <label style={{ display: "block" }} title={"This is useful for adding symbols that only get exported to the bill of materials but\nnot required to layout the board such as mechanical fasteners and enclosures"}>
            <input type="checkbox" checked={excludeFromBoard} onChange={(e) => setExcludeFromBoard(e.target.checked)} /> Exclude from board
          </label>
          <label style={{ display: "block" }}>
            <input type="checkbox" checked={dnp} onChange={(e) => setDnp(e.target.checked)} /> Do not populate
          </label>
        </fieldset>
        <fieldset style={{ flex: 1, minWidth: 220 }}>
          <legend>Style</legend>
          <Grid>
            <span>Border width (mm):</span>
            <input aria-label="Border width" value={border} onChange={(e) => setBorder(e.target.value)} title="0 is the schematic's default line width" />
            <span>Border color:</span>
            <ColorSwatch label="Border color" color={borderColor} onChange={setBorderColor} />
            <span>Fill color:</span>
            <ColorSwatch label="Background color" color={fillColor} onChange={setFillColor} />
          </Grid>
        </fieldset>
      </div>
      {problem && (
        <p style={{ color: "var(--chrome-danger)", fontSize: 11, marginTop: 8 }} data-testid="sheet-properties-error">
          {problem}
        </p>
      )}
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
  const [width, setWidth] = useState(view.widthNm.mixed ? "" : nmToMm(view.widthNm.value));
  const [style, setStyle] = useState<SchLineStyle | "">(view.style.mixed ? "" : view.style.value);
  const [color, setColor] = useState<SchColor>(view.color.mixed ? { r: 0, g: 0, b: 0, a: 255 } : view.color.value);
  const [diameter, setDiameter] = useState(view.diameterNm.mixed ? "" : nmToMm(view.diameterNm.value));
  // What the user touched is what is sent: a field left alone keeps each item's own value (KiCad shows `INDETERMINATE_ACTION` for these).
  const [touched, setTouched] = useState<{ width?: boolean; style?: boolean; color?: boolean; diameter?: boolean }>({});
  const touch = (k: keyof typeof touched) => setTouched((t) => ({ ...t, [k]: true }));
  const widthNm = mmToNm(width);
  const diameterNm = mmToNm(diameter);
  const canOk = (!touched.width || widthNm !== null) && (!touched.diameter || diameterNm !== null);
  const onlyJunctions = view.hasJunction && !view.hasStroke;
  const onlyLines = ids.length > 0 && ids.every((id) => (sch.lines ?? []).some((l) => l.id === id));
  const title = onlyJunctions ? (ids.length === 1 ? "Junction Properties" : "Junctions Properties") : onlyLines ? "Line Properties" : "Wire/Bus Properties";
  const change = (): StrokeChange => ({
    ...(touched.width && widthNm !== null && view.hasStroke ? { widthNm } : {}),
    ...(touched.style && style !== "" && view.hasStroke ? { style } : {}),
    ...(touched.color ? { color } : {}),
    ...(touched.diameter && diameterNm !== null && view.hasJunction ? { diameterNm } : {}),
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
              placeholder={view.widthNm.mixed ? "(mixed)" : "0 = default"}
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
              placeholder={view.diameterNm.mixed ? "(mixed)" : "0 = default"}
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
  const [width, setWidth] = useState(nmToMm(graphic.width_nm ?? 0));
  const [style, setStyle] = useState<SchLineStyle>(graphic.line_style ?? "default");
  const [color, setColor] = useState<SchColor>(graphic.color ?? UNSPECIFIED_COLOR);
  const [fill, setFill] = useState<SchFill>(graphic.fill ?? "none");
  const [fillColor, setFillColor] = useState<SchColor>(graphic.fill_color ?? { r: 255, g: 255, b: 194, a: 255 });
  const [flags, setFlags] = useState({ sim: !!rule?.exclude_from_sim, bom: !!rule?.exclude_from_bom, board: !!rule?.exclude_from_board, dnp: !!rule?.dnp });
  const widthNm = mmToNm(width);
  const canOk = widthNm !== null && widthNm >= 0;
  const submit = () => {
    if (!canOk) return;
    const next: SchGraphic = { ...graphic, width_nm: widthNm, line_style: style, color: isUnspecified(color) ? undefined : color, fill, fill_color: fill === "color" ? fillColor : undefined };
    if (rule && next.shape.type === "rule_area") next.shape = { ...next.shape, exclude_from_sim: flags.sim, exclude_from_bom: flags.bom, exclude_from_board: flags.board, dnp: flags.dnp };
    void send(graphicsEqual(next, graphic) ? null : { op: "sch_edit", verb: "edit_graphic", id: graphic.id, graphic: next });
  };
  const closed = graphic.shape.type !== "arc" && graphic.shape.type !== "bezier";
  return (
    <SchDialogShell title={rule ? "Rule Area Properties" : "Shape Properties"} width={420} onCancel={close} onOk={submit} okLabel="OK" canOk={canOk} note={<SchColorOverrideNote />}>
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
