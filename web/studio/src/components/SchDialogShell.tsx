// The frame the schematic tool dialogs share (components/SchToolDialogs.tsx, SchPinDialogs.tsx): backdrop, header, body, footer.
import type { ReactNode } from "react";

export function SchDialogShell({ title, width = 380, onCancel, onOk, okLabel, canOk = true, cancelLabel = "Cancel", children }: { title: string; width?: number; onCancel: () => void; onOk?: () => void; okLabel?: string; canOk?: boolean; cancelLabel?: string; children: ReactNode }) {
  return (
    <div className="dialog-backdrop" onClick={onCancel}>
      <div className="dialog" style={{ width }} onClick={(e) => e.stopPropagation()}>
        <div className="dialog-header">
          <span>{title}</span>
        </div>
        <div className="dialog-body">{children}</div>
        <div className="dialog-footer">
          <button onClick={onCancel}>{cancelLabel}</button>
          {onOk && (
            <button className="primary" disabled={!canOk} onClick={onOk}>
              {okLabel ?? "OK"}
            </button>
          )}
        </div>
      </div>
    </div>
  );
}
