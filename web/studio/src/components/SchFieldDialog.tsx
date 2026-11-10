// Field Properties (`E`, a double-click on a field; `SCH_EDIT_TOOL::EditField` / `editFieldText`, `DIALOG_FIELD_PROPERTIES`): the text of one field of a symbol, a power symbol
// or a sheet, where it is, how it is set and whether it is shown. OK is one `edit_field` command (kicad-port/schFieldEdit.ts builds it from what changed), so it is one
// undo step, and nothing is sent when nothing changed.
//
// Differences from KiCad's dialog, deliberate: the orientation, the position and the justification are those the field has on the sheet (KiCad's dialog shows the
// angle it stores, which a turned symbol's own transform turns again; the position is the sheet's, as KiCad's `m_field->GetPosition()` is). The font face and colour
// (`m_font`, `m_textColorSwatch`) are `set_field_looks` (`SchExtras::field_looks`), in the batch of the OK. The text of a sheet's file is edited from Sheet
// Properties, as KiCad's dialog disables it too.
//
// A label's field (UI-LEFT S6: its net class, a global label's Sheet References, any other) opens here too: moved, turned or justified by hand it stays where it was
// put (the label's fields are no longer autoplaced). Its text size, bold and "Show field name" are its own (`LabelField`, UI-LEFT-2 SS12). **Differs**: the
// Sheet References keep their text and Schematic Setup's visibility, the default size, regular, without their name (the studio stores nothing of that field), and
// a label's field has no "Allow autoplacement" (the label's fields are placed together).
import { schematicWithCarry, useSchCarry } from "../state/schCarryStore";
import { editCarried } from "../actions/schPlaceActions";
import { useState } from "react";
import { fieldDialogCaption, fieldSizeNm, fieldEditCmd, findField, isStandardSymbolField, FIELD_SIZE_MAX_NM, FIELD_SIZE_MIN_NM, type FieldChanges } from "../kicad-port/schFieldEdit";
import { mmToNm, nmToMm } from "../kicad-port/schProperties";
import { useStudioApi, useStudioDispatch, useStudioState } from "../state/store";
import { SchDialogShell } from "./SchDialogShell";
import type { Cmd, SchField } from "../api/types";
import type { FieldOwnerKind } from "../kicad-port/schFieldEdit";
import { chosenFont, DEFAULT_FONT, oneStep } from "../kicad-port/textLooks";
import { ColorSwatch, FontSelect, useFontFamilies } from "./TextLookControls";

const H_ALIGN: Array<{ value: SchField["h"]; label: string }> = [
  { value: "left", label: "Left" },
  { value: "center", label: "Center" },
  { value: "right", label: "Right" },
];
const V_ALIGN: Array<{ value: SchField["v"]; label: string }> = [
  { value: "top", label: "Top" },
  { value: "center", label: "Center" },
  { value: "bottom", label: "Bottom" },
];

/** `carried`: a field of the symbol or power symbol a placing tool holds (U, V, F while it rides): read off the page's drawing of it, and OK changes the item in the hand. */
export function FieldPropertiesDialog({ id, carried }: { id: string; carried?: boolean }) {
  const base = useStudioState().schematic;
  const carry = useSchCarry();
  const sch = carried && base && carry ? schematicWithCarry(base, carry) : base;
  const item = sch ? findField(sch, id) : null;
  return item ? <FieldForm key={id} owner={item.ownerKind} field={item.field} carried={!!carried} /> : null;
}

function FieldForm({ owner, field, carried }: { owner: FieldOwnerKind; field: SchField & { id: string }; carried: boolean }) {
  const api = useStudioApi();
  const dispatch = useStudioDispatch();
  const close = () => dispatch({ type: "SET_SCH_TOOL_DIALOG", dialog: null });
  const [text, setText] = useState(field.text);
  const [x, setX] = useState(nmToMm(field.at[0]));
  const [y, setY] = useState(nmToMm(field.at[1]));
  const [size, setSize] = useState(nmToMm(fieldSizeNm(field)));
  const [vertical, setVertical] = useState(field.vertical);
  const [bold, setBold] = useState(!!field.bold);
  const [italic, setItalic] = useState(!!field.italic);
  const [h, setH] = useState<SchField["h"]>(field.h);
  const [v, setV] = useState<SchField["v"]>(field.v);
  const [visible, setVisible] = useState(field.visible);
  const [nameShown, setNameShown] = useState(!!field.name_shown);
  const [allowAutoplace, setAllowAutoplace] = useState(field.allow_autoplace ?? true);
  const families = useFontFamilies();
  const [font, setFont] = useState(field.font || DEFAULT_FONT);
  const [color, setColor] = useState(field.color && field.color.a > 0 ? field.color : null);
  const label = owner === "label";
  const isr = label && field.name === "Intersheetrefs";
  const xNm = mmToNm(x);
  const yNm = mmToNm(y);
  const sizeNm = mmToNm(size);
  // `m_textSize.Validate( 0.01, 1000.0, EDA_UNITS::MM )`: "Don't allow text to disappear"
  const sizeOk = sizeNm !== null && sizeNm >= FIELD_SIZE_MIN_NM && sizeNm <= FIELD_SIZE_MAX_NM;
  const textFixed = field.name === "Sheetfile" || (owner === "power" && field.name === "Reference") || isr;
  // A reference, a value, a sheet's name cannot be emptied (`FIELD_VALIDATOR`); a footprint, a datasheet and a user-defined field of a symbol can
  // a symbol's Value has no validator in `DIALOG_FIELD_PROPERTIES::init`, like its Footprint, Datasheet and user fields: only the Reference must not be empty
  const textOk = textFixed || label || field.name === "Footprint" || field.name === "Datasheet" || (owner === "symbol" && (field.name === "Value" || !isStandardSymbolField(field.name))) || text.trim().length > 0;
  const canOk = sizeOk && xNm !== null && yNm !== null && textOk;
  const submit = () => {
    if (!canOk) return;
    const changes: FieldChanges = {
      ...(textFixed ? {} : { text }),
      at: [xNm!, yNm!],
      vertical,
      h,
      v,
      italic,
      ...(isr ? {} : { visible }),
      // a label's field has its own size, bold and shown name, but no autoplace flag; the Sheet References keep none of them
      ...(isr ? {} : label ? { sizeNm: sizeNm!, bold, nameShown } : { sizeNm: sizeNm!, bold, nameShown, allowAutoplace }),
    };
    const nextFont = chosenFont(font);
    const fontChanged = nextFont !== undefined && nextFont !== (field.font ?? "");
    const colorChanged = (color === null) !== !(field.color && field.color.a > 0) || (color !== null && field.color && (color.r !== field.color.r || color.g !== field.color.g || color.b !== field.color.b || color.a !== field.color.a));
    const looks = fontChanged || colorChanged ? ({ op: "sch_edit", verb: "set_field_looks", ids: [field.id], ...(fontChanged ? { font: nextFont } : {}), ...(colorChanged ? { color: color ?? { r: 0, g: 0, b: 0, a: 0 } } : {}) } as never) : null;
    const cmd = oneStep([fieldEditCmd(field, changes), looks]);
    if (!cmd) return close();
    if (carried) {
      // the item in the hand takes the edit (actions/schPlaceActions.ts `editCarried`); its place is Autoplace Fields' once the click puts it down
      // (`AutoplaceFields( screen, AUTOPLACE_AUTO )`), so a position typed here is not kept
      const parts = (cmd.op === "batch" ? cmd.cmds : [cmd]).map((c) => Object.fromEntries(Object.entries(c).filter(([k]) => k !== "at")) as unknown as Cmd);
      editCarried(parts.filter((c) => Object.keys(c).some((k) => !["op", "verb", "id"].includes(k))));
      return close();
    }
    void api.cmd(cmd).then((ok) => ok && close());
  };
  return (
    <SchDialogShell title={fieldDialogCaption(field.name, owner === "sheet")} width={440} onCancel={close} onOk={submit} okLabel="OK" canOk={canOk}>
      <div className="kv-grid" style={{ gridTemplateColumns: "130px 1fr" }}>
        <span>{field.name}:</span>
        <input autoFocus disabled={textFixed} value={text} onChange={(e) => setText(e.target.value)} onKeyDown={(e) => e.key === "Enter" && submit()} title={textFixed ? "Edited from the properties of the item" : undefined} />
        <span>Position X (mm)</span>
        <input value={x} onChange={(e) => setX(e.target.value)} onKeyDown={(e) => e.key === "Enter" && submit()} style={xNm === null ? { outline: "1px solid var(--error, #d33)" } : undefined} />
        <span>Position Y (mm)</span>
        <input value={y} onChange={(e) => setY(e.target.value)} onKeyDown={(e) => e.key === "Enter" && submit()} style={yNm === null ? { outline: "1px solid var(--error, #d33)" } : undefined} />
        {!isr && (
          <>
            <span>Text size (mm)</span>
            <input value={size} onChange={(e) => setSize(e.target.value)} onKeyDown={(e) => e.key === "Enter" && submit()} style={sizeOk ? undefined : { outline: "1px solid var(--error, #d33)" }} title="0.01 to 1000 mm" />
          </>
        )}
        <span>Font:</span>
        <FontSelect families={families} current={field.font} value={font} onChange={setFont} />
        <span>Color:</span>
        <ColorSwatch label="Field color" color={color} onChange={setColor} />
        <span>Orientation</span>
        <select value={vertical ? "v" : "h"} onChange={(e) => setVertical(e.target.value === "v")}>
          <option value="h">Horizontal</option>
          <option value="v">Vertical</option>
        </select>
        <span>Style</span>
        <span style={{ display: "flex", gap: 14 }}>
          {!isr && (
            <label>
              <input type="checkbox" checked={bold} onChange={(e) => setBold(e.target.checked)} /> Bold
            </label>
          )}
          <label>
            <input type="checkbox" checked={italic} onChange={(e) => setItalic(e.target.checked)} /> Italic
          </label>
        </span>
        <span>Horizontal justification</span>
        <select value={h} onChange={(e) => setH(e.target.value as SchField["h"])}>
          {H_ALIGN.map((o) => (
            <option key={o.value} value={o.value}>
              {o.label}
            </option>
          ))}
        </select>
        <span>Vertical justification</span>
        <select value={v} onChange={(e) => setV(e.target.value as SchField["v"])}>
          {V_ALIGN.map((o) => (
            <option key={o.value} value={o.value}>
              {o.label}
            </option>
          ))}
        </select>
        <span>Display</span>
        <span style={{ display: "flex", flexDirection: "column", gap: 4 }}>
          <label title={isr ? "Intersheet reference visibility is controlled globally from Schematic Setup > General > Formatting" : undefined}>
            <input type="checkbox" checked={visible} disabled={isr} onChange={(e) => setVisible(e.target.checked)} /> Visible
          </label>
          {!isr && (
            <label>
              <input type="checkbox" checked={nameShown} onChange={(e) => setNameShown(e.target.checked)} /> Show field name
            </label>
          )}
          {!label && (
            <>
              <label>
                <input type="checkbox" checked={allowAutoplace} onChange={(e) => setAllowAutoplace(e.target.checked)} /> Allow autoplacement
              </label>
            </>
          )}
        </span>
      </div>
    </SchDialogShell>
  );
}
