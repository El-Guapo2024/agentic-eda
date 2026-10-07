// The frame every schematic control dialog shares: the studio's `.dialog` chrome (backdrop, header, body, footer), closed by a click outside it or Escape.
import { useEffect, type ReactNode } from "react";

export function SchDialogFrame({ title, width = 420, onClose, footer, children }: { title: string; width?: number | string; onClose: () => void; footer?: ReactNode; children: ReactNode }) {
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);
  return (
    <div className="dialog-backdrop" onClick={onClose}>
      <div className="dialog" style={{ width, maxWidth: "94vw" }} onClick={(e) => e.stopPropagation()}>
        <div className="dialog-header">
          <span>{title}</span>
        </div>
        <div className="dialog-body" style={{ maxHeight: "72vh", overflow: "auto" }}>
          {children}
        </div>
        <div className="dialog-footer">
          {footer}
          <button onClick={onClose}>{footer ? "Cancel" : "Close"}</button>
        </div>
      </div>
    </div>
  );
}
