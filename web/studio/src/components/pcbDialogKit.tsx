// Small pieces shared by the pcbnew edit-tool dialogs (components/PcbSweepDialogs.tsx and the
// other Pcb*Dialogs.tsx files): the modal shell with the app's own dialog classes, the Escape
// key, and a length field that shows the display units and parses a unit suffix.
import { useEffect, useState, type ReactNode } from "react";
import { useStudioState } from "../state/store";
import { umFrom, umTo } from "../state/units";
import { closeSweepDialog } from "../actions/pcbSweepDialogs";

/** Escape closes the dialog (`closeSweepDialog` unless a dialog handles its own stack). The key is the dialog's: it must not go on to the canvas' own Escape (cancel the tool, then clear the selection). */
export function useEscape(onClose: () => void = closeSweepDialog) {
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== "Escape") return;
      e.stopPropagation();
      e.preventDefault();
      onClose();
    };
    document.addEventListener("keydown", onKey, true);
    return () => document.removeEventListener("keydown", onKey, true);
  }, [onClose]);
}

/** Display text of a length: the number in the display units, without trailing noise. */
export function lengthText(valueUm: number, units: "mm" | "mil" | "in"): string {
  return String(Number(umTo(valueUm, units).toFixed(units === "mm" ? 4 : units === "mil" ? 2 : 5)));
}

/** Parse "1.5", "0.2mm", "10mil", `0.01"` -> micrometres; a bare number is in the display units. Null when it is not a length. */
export function parseLength(text: string, units: "mm" | "mil" | "in"): number | null {
  const m = /^\s*(-?\d*\.?\d+)\s*(mm|mil|in|")?\s*$/i.exec(text);
  if (!m) return null;
  const unit = m[2] ? (m[2] === '"' ? "in" : (m[2].toLowerCase() as "mm" | "mil" | "in")) : units;
  return Math.round(umFrom(parseFloat(m[1]!), unit));
}

/** A length field in the display units, parsed with a unit suffix allowed ("1.5", "0.2mm", "10mil"). */
export function useLengthField(valueUm: number) {
  const units = useStudioState().units;
  const [text, setText] = useState(() => lengthText(valueUm, units));
  const parse = (): number | null => parseLength(text, units);
  return { text, setText, parse, units };
}

/** The dialog frame: backdrop (a click outside cancels), header, body and the Cancel / OK footer. */
export function DialogShell({
  title,
  width = 360,
  onCancel,
  onOk,
  okDisabled,
  okLabel = "OK",
  children,
}: {
  title: string;
  width?: number;
  onCancel: () => void;
  onOk: () => void;
  okDisabled?: boolean;
  okLabel?: string;
  children: ReactNode;
}) {
  useEscape(onCancel);
  return (
    <div className="dialog-backdrop" onClick={onCancel}>
      <div className="dialog" style={{ width }} onClick={(e) => e.stopPropagation()}>
        <div className="dialog-header">
          <span>{title}</span>
        </div>
        <div className="dialog-body">{children}</div>
        <div className="dialog-footer">
          <button onClick={onCancel}>Cancel</button>
          <button className="primary" disabled={okDisabled} onClick={onOk}>
            {okLabel}
          </button>
        </div>
      </div>
    </div>
  );
}

/** One labelled length input row (label, input, unit suffix) -- the C++ dialogs' `UNIT_BINDER` row. */
export function LengthRow({ label, field, onEnter, autoFocus, disabled }: { label: string; field: ReturnType<typeof useLengthField>; onEnter?: () => void; autoFocus?: boolean; disabled?: boolean }) {
  return (
    <label style={{ display: "flex", alignItems: "center", gap: 8, fontSize: 12, marginBottom: 6, opacity: disabled ? 0.5 : 1 }}>
      <span style={{ minWidth: 90 }}>{label}</span>
      <input
        autoFocus={autoFocus}
        disabled={disabled}
        style={{ flex: 1 }}
        value={field.text}
        aria-label={label}
        onChange={(e) => field.setText(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === "Enter") onEnter?.();
        }}
      />
      <span>{field.units}</span>
    </label>
  );
}
