// Field Properties (`E`, a double-click on a field; `SCH_EDIT_TOOL::EditField` / `editFieldText`, `DIALOG_FIELD_PROPERTIES`): the text of one field of a symbol, a power symbol
// or a sheet, where it is, how it is set and whether it is shown. OK is one `edit_field` command (kicad-port/schFieldEdit.ts builds it from what changed), so it is one
// undo step, and nothing is sent when nothing changed.
//
// Differences from KiCad's dialog, deliberate: the orientation, the position and the justification are those the field has on the sheet (KiCad's dialog shows the
// angle it stores, which a turned symbol's own transform turns again; the position is the sheet's, as KiCad's `m_field->GetPosition()` is). No font face and no colour:
// the drawing model has no place for either. The text of a sheet's file is edited from Sheet Properties, as KiCad's dialog disables it too.
import { useState } from "react";
import { fieldSizeUm, fieldEditCmd, findField, FIELD_SIZE_MAX_UM, FIELD_SIZE_MIN_UM, type FieldChanges } from "../kicad-port/schFieldEdit";
import { mmToUm, umToMm } from "../kicad-port/schProperties";
import { useStudioApi, useStudioDispatch, useStudioState } from "../state/store";
import { SchDialogShell } from "./SchDialogShell";
import type { SchField } from "../api/types";

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

export function FieldPropertiesDialog({ id }: { id: string }) {
  const sch = useStudioState().schematic;
  const item = sch ? findField(sch, id) : null;
  return item ? <FieldForm key={id} owner={item.ownerKind} field={item.field} /> : null;
}

function FieldForm({ owner, field }: { owner: "symbol" | "power" | "sheet"; field: SchField & { id: string } }) {
  const api = useStudioApi();
  const dispatch = useStudioDispatch();
  const close = () => dispatch({ type: "SET_SCH_TOOL_DIALOG", dialog: null });
  const [text, setText] = useState(field.text);
  const [x, setX] = useState(umToMm(field.at[0]));
  const [y, setY] = useState(umToMm(field.at[1]));
  const [size, setSize] = useState(umToMm(fieldSizeUm(field)));
  const [vertical, setVertical] = useState(field.vertical);
  const [bold, setBold] = useState(!!field.bold);
  const [italic, setItalic] = useState(!!field.italic);
  const [h, setH] = useState<SchField["h"]>(field.h);
  const [v, setV] = useState<SchField["v"]>(field.v);
  const [visible, setVisible] = useState(field.visible);
  const [nameShown, setNameShown] = useState(!!field.name_shown);
  const [allowAutoplace, setAllowAutoplace] = useState(field.allow_autoplace ?? true);
  const xUm = mmToUm(x);
  const yUm = mmToUm(y);
  const sizeUm = mmToUm(size);
  // `m_textSize.Validate( 0.01, 1000.0, EDA_UNITS::MM )`: "Don't allow text to disappear"
  const sizeOk = sizeUm !== null && sizeUm >= FIELD_SIZE_MIN_UM && sizeUm <= FIELD_SIZE_MAX_UM;
  const textFixed = field.name === "Sheetfile" || (owner === "power" && field.name === "Reference");
  // A reference, a value, a sheet's name cannot be emptied (`FIELD_VALIDATOR`)
  const textOk = textFixed || field.name === "Footprint" || field.name === "Datasheet" || text.trim().length > 0;
  const canOk = sizeOk && xUm !== null && yUm !== null && textOk;
  const submit = () => {
    if (!canOk) return;
    const changes: FieldChanges = {
      ...(textFixed ? {} : { text }),
      at: [xUm!, yUm!],
      vertical,
      h,
      v,
      sizeUm: sizeUm!,
      bold,
      italic,
      visible,
      nameShown,
      allowAutoplace,
    };
    const cmd = fieldEditCmd(field, changes);
    if (!cmd) return close();
    void api.cmd(cmd).then((ok) => ok && close());
  };
  return (
    <SchDialogShell title={`Edit ${field.name} Field`} width={440} onCancel={close} onOk={submit} okLabel="OK" canOk={canOk}>
      <div className="kv-grid" style={{ gridTemplateColumns: "130px 1fr" }}>
        <span>{field.name}:</span>
        <input autoFocus disabled={textFixed} value={text} onChange={(e) => setText(e.target.value)} onKeyDown={(e) => e.key === "Enter" && submit()} title={textFixed ? "Edited from the properties of the item" : undefined} />
        <span>Position X (mm)</span>
        <input value={x} onChange={(e) => setX(e.target.value)} onKeyDown={(e) => e.key === "Enter" && submit()} style={xUm === null ? { outline: "1px solid var(--error, #d33)" } : undefined} />
        <span>Position Y (mm)</span>
        <input value={y} onChange={(e) => setY(e.target.value)} onKeyDown={(e) => e.key === "Enter" && submit()} style={yUm === null ? { outline: "1px solid var(--error, #d33)" } : undefined} />
        <span>Text size (mm)</span>
        <input value={size} onChange={(e) => setSize(e.target.value)} onKeyDown={(e) => e.key === "Enter" && submit()} style={sizeOk ? undefined : { outline: "1px solid var(--error, #d33)" }} title="0.01 to 1000 mm" />
        <span>Orientation</span>
        <select value={vertical ? "v" : "h"} onChange={(e) => setVertical(e.target.value === "v")}>
          <option value="h">Horizontal</option>
          <option value="v">Vertical</option>
        </select>
        <span>Style</span>
        <span style={{ display: "flex", gap: 14 }}>
          <label>
            <input type="checkbox" checked={bold} onChange={(e) => setBold(e.target.checked)} /> Bold
          </label>
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
          <label>
            <input type="checkbox" checked={visible} onChange={(e) => setVisible(e.target.checked)} /> Visible
          </label>
          <label>
            <input type="checkbox" checked={nameShown} onChange={(e) => setNameShown(e.target.checked)} /> Show field name
          </label>
          <label>
            <input type="checkbox" checked={allowAutoplace} onChange={(e) => setAllowAutoplace(e.target.checked)} /> Allow autoplacement
          </label>
        </span>
      </div>
    </SchDialogShell>
  );
}
