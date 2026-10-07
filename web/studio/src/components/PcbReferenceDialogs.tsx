// The dialog of the reference-point commands (actions/pcbReferenceSweep.ts):
//
//   OffsetItemDialog   DIALOG_OFFSET_ITEM (pcbnew/dialogs/dialog_offset_item.cpp): "Offset Item". The offset
//                      the two clicks of POSITION_RELATIVE_TOOL::InteractiveOffset measured, as X / Y or as a
//                      distance and an angle ("Use polar coordinates"), each with a Reset button that goes back
//                      to the measured value.
import { useState } from "react";
import { closeSweepDialog } from "../actions/pcbSweepDialogs";
import { polarTranslation, toPolarDeg } from "../kicad-port/pcbEditActions";
import type { Vec } from "../kicad-port/pcbReference";
import { useStudioState } from "../state/store";
import { umFrom, umTo } from "../state/units";
import { DialogShell } from "./pcbDialogKit";

export function OffsetItemDialog({ offset, onResult }: { offset: Vec; onResult: (offset: Vec | null) => void }) {
  const units = useStudioState().units;
  // A length as the field shows it: the display units, to the resolution that still names one micrometre.
  const disp = (um: number) => Number(umTo(um, units).toFixed(units === "mm" ? 4 : units === "mil" ? 2 : 5));
  const [polar, setPolar] = useState(false);
  const [a, setA] = useState(disp(offset.x)); // Offset X, or the distance (display units)
  const [b, setB] = useState(disp(offset.y)); // Offset Y, or the angle (degrees)

  const original = toPolarDeg(offset.x, offset.y);

  // OnPolarChanged: carry the entered values over to the other coordinate system.
  const switchPolar = (next: boolean) => {
    if (next) {
      const p = toPolarDeg(umFrom(a, units), umFrom(b, units));
      setA(disp(p.r));
      setB(Number(p.deg.toFixed(4)));
    } else {
      const t = polarTranslation(umFrom(a, units), b);
      setA(disp(t.x));
      setB(disp(t.y));
    }
    setPolar(next);
  };

  // OnClear: back to the measured value of that field.
  const resetA = () => setA(polar ? disp(original.r) : disp(offset.x));
  const resetB = () => setB(polar ? Number(original.deg.toFixed(4)) : disp(offset.y));

  const close = (result: Vec | null) => {
    closeSweepDialog();
    onResult(result);
  };
  // TransferDataFromWindow
  const submit = () => {
    const entered = polar ? polarTranslation(umFrom(a, units), b) : { x: Math.round(umFrom(a, units)), y: Math.round(umFrom(b, units)) };
    close(entered);
  };

  const small = { padding: "0 6px" } as const;
  return (
    <DialogShell title="Offset Item" width={420} onCancel={() => close(null)} onOk={submit}>
      <div className="kv-grid" style={{ gridTemplateColumns: "90px 1fr auto auto", alignItems: "center" }}>
        <span>{polar ? "Distance:" : "Offset X:"}</span>
        <input autoFocus type="number" step="any" value={a} onChange={(e) => setA(Number(e.target.value))} onKeyDown={(e) => e.key === "Enter" && submit()} aria-label={polar ? "Distance" : "Offset X"} />
        <span>{units}</span>
        <button style={small} title={polar ? "Reset to the current distance from the reference position." : "Reset to the current X offset from the reference position."} onClick={resetA}>
          Reset
        </button>
        <span>{polar ? "Angle:" : "Offset Y:"}</span>
        <input type="number" step="any" value={b} onChange={(e) => setB(Number(e.target.value))} onKeyDown={(e) => e.key === "Enter" && submit()} aria-label={polar ? "Angle" : "Offset Y"} />
        <span>{polar ? "deg" : units}</span>
        <button style={small} title={polar ? "Reset to the current angle from the reference position." : "Reset to the current Y offset from the reference position."} onClick={resetB}>
          Reset
        </button>
      </div>
      <label className="filter-row" style={{ marginTop: 10 }}>
        <input type="checkbox" checked={polar} onChange={(e) => switchPolar(e.target.checked)} /> Use polar coordinates
      </label>
    </DialogShell>
  );
}
