// The look and place cells of Symbol Properties' Fields grid (`FIELDS_GRID_TABLE::GetValue` / `SetValue`, `GetAttr`: Show Name .. Allow Autoplacement), shared by
// a symbol's dialog (components/SymbolPropertiesDialog.tsx) and a power symbol's (components/PowerSymbolPropertiesDialog.tsx), which KiCad's one
// `DIALOG_SYMBOL_PROPERTIES` both are. The columns are kicad-port/symbolFieldsGrid.ts's; the values kicad-port/symbolPropsDialog.ts's `FieldLook`.
import type React from "react";
import type { FieldColumn } from "../kicad-port/symbolFieldsGrid";
import type { FieldLook } from "../kicad-port/symbolPropsDialog";

/**
 * The cell of the column `col` for the field `name` whose look and place are `l`; `set` changes them; `fontList` is the id of the Font column's datalist. Null for a
 * column the grid's own cells hold (Name, Value, Show) and for the place columns of a field the server sent no place in its own frame for.
 */
export function lookCellOf(name: string, col: FieldColumn, l: FieldLook, set: (patch: Partial<FieldLook>) => void, fontList: string): React.ReactNode {
  const mm = (v: number | undefined) => (v === undefined ? "" : String(Math.round(v * 1e4) / 1e4));
  switch (col) {
    case "show_name":
      return <input type="checkbox" aria-label={`${name} show name`} checked={l.nameShown} onChange={(e) => set({ nameShown: e.target.checked })} />;
    case "h_align":
      return (
        <select aria-label={`${name} h align`} value={l.h} onChange={(e) => set({ h: e.target.value as FieldLook["h"] })}>
          <option value="left">Left</option>
          <option value="center">Center</option>
          <option value="right">Right</option>
        </select>
      );
    case "v_align":
      return (
        <select aria-label={`${name} v align`} value={l.v} onChange={(e) => set({ v: e.target.value as FieldLook["v"] })}>
          <option value="top">Top</option>
          <option value="center">Center</option>
          <option value="bottom">Bottom</option>
        </select>
      );
    case "italic":
      return <input type="checkbox" aria-label={`${name} italic`} checked={l.italic} onChange={(e) => set({ italic: e.target.checked })} />;
    case "bold":
      return <input type="checkbox" aria-label={`${name} bold`} checked={l.bold} onChange={(e) => set({ bold: e.target.checked })} />;
    case "text_size":
      return <input aria-label={`${name} text size`} title="Millimetres" style={{ width: 52 }} type="number" step="0.01" min={0.01} value={l.sizeMm} onChange={(e) => set({ sizeMm: Number(e.target.value) })} />;
    case "orientation":
      // `FDC_ORIENTATION`: "Horizontal" / "Vertical", the text's angle before the symbol turns it
      return l.vertical === undefined ? null : (
        <select aria-label={`${name} orientation`} value={l.vertical ? "Vertical" : "Horizontal"} onChange={(e) => set({ vertical: e.target.value === "Vertical" })}>
          <option value="Horizontal">Horizontal</option>
          <option value="Vertical">Vertical</option>
        </select>
      );
    case "pos_x":
    case "pos_y": {
      // `FDC_POSX` / `FDC_POSY`: the field's position from the symbol's, in its unturned frame
      const axis = col === "pos_x" ? "xMm" : "yMm";
      return l[axis] === undefined ? null : (
        <input
          aria-label={`${name} ${col === "pos_x" ? "x" : "y"} position`}
          title="Millimetres from the symbol's position, before it is turned"
          style={{ width: 64 }}
          type="number"
          step="0.01"
          value={mm(l[axis])}
          onChange={(e) => Number.isFinite(Number(e.target.value)) && set({ [axis]: Number(e.target.value) })}
        />
      );
    }
    case "font":
      return <input aria-label={`${name} font`} style={{ width: 110 }} list={fontList} value={l.font} onChange={(e) => set({ font: e.target.value })} />;
    case "color":
      return (
        <span style={{ whiteSpace: "nowrap" }}>
          <input aria-label={`${name} color`} type="color" value={l.color || "#000000"} onChange={(e) => set({ color: e.target.value })} style={{ width: 28, padding: 0, opacity: l.color ? 1 : 0.35 }} />
          {l.color && (
            <button type="button" title="The layer's colour (unspecified)" aria-label={`${name} color clear`} onClick={() => set({ color: "" })}>
              ×
            </button>
          )}
        </span>
      );
    case "allow_autoplace":
      return <input type="checkbox" aria-label={`${name} allow autoplacement`} checked={l.allowAutoplace ?? true} onChange={(e) => set({ allowAutoplace: e.target.checked })} />;
    default:
      return null;
  }
}
