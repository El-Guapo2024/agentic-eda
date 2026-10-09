// The small pieces the Appearance panel's tabs share: the eye toggle (`BITMAP_TOGGLE` with the visibility bitmaps), the colour swatch (`COLOR_SWATCH`) with
// the colour dialog it opens (`DIALOG_COLOR_PICKER`: a colour, an opacity, "no colour"), a collapsible pane (`WX_COLLAPSIBLE_PANE`) and the little name / list /
// confirm dialogs of the preset and viewport commands (`wxTextEntryDialog`, `EDA_LIST_DIALOG`, `IsOK`).
import { useState, type MouseEvent, type ReactNode } from "react";
import { createPortal } from "react-dom";
import { DialogShell } from "../../pcbDialogKit";
import { parseCssColor, toCssColor, toHex6, specifiedColor, type Rgba } from "../../../kicad-port/appearance";
import "../../../styles/appearance.css";

export function Eye({ on, title, onToggle, disabled }: { on: boolean; title: string; onToggle: () => void; disabled?: boolean }) {
  return (
    <button type="button" className={`ap-eye${on ? "" : " off"}`} title={title} aria-label={title} aria-pressed={on} disabled={disabled} onClick={(e) => { e.stopPropagation(); onToggle(); }} onContextMenu={(e) => e.stopPropagation()}>
      <svg width="16" height="16" viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.3" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
        <path d="M1.2 8s2.5-4.6 6.8-4.6S14.8 8 14.8 8s-2.5 4.6-6.8 4.6S1.2 8 1.2 8z" />
        <circle cx="8" cy="8" r="2" />
        {!on && <path d="M2.5 13.5l11-11" />}
      </svg>
    </button>
  );
}

/** A colour chip. With `onEdit` a double click or a middle click edits it ("Left double click or middle click for color change, right click for menu"). */
export function Swatch({ css, onEdit, onMenu, title, hidden }: { css: string | null; onEdit?: () => void; onMenu?: (e: MouseEvent) => void; title?: string; hidden?: boolean }) {
  const c = specifiedColor(css);
  const cls = `ap-swatch${c ? "" : " none"}${hidden ? " empty" : ""}${onEdit ? "" : " fixed"}`;
  return (
    <span
      className={cls}
      style={c ? { background: toCssColor(c) } : undefined}
      title={title}
      role={onEdit ? "button" : undefined}
      aria-label={title}
      onDoubleClick={onEdit ? (e) => { e.stopPropagation(); onEdit(); } : undefined}
      onMouseUp={onEdit ? (e) => { if (e.button === 1) { e.preventDefault(); e.stopPropagation(); onEdit(); } } : undefined}
      onContextMenu={onMenu}
    />
  );
}

/** Stops the events of a dialog rendered inside a row from reaching the row (a portal's events still bubble through the React tree). */
function Isolated({ children }: { children: ReactNode }) {
  const stop = (e: { stopPropagation: () => void }) => e.stopPropagation();
  return createPortal(
    <div onClick={stop} onDoubleClick={stop} onContextMenu={stop} onPointerDown={stop} onMouseUp={stop} onKeyDown={stop}>
      {children}
    </div>,
    document.body
  );
}

/** `DIALOG_COLOR_PICKER`'s part that matters here: pick a colour and an opacity, or "no colour" (`COLOR4D::UNSPECIFIED`). Calls back with the colour text, or null for none. */
export function ColorDialog({ title, initial, onPick, onCancel }: { title: string; initial: string | null; onPick: (css: string | null) => void; onCancel: () => void }) {
  const start: Rgba = specifiedColor(initial) ?? { r: 200, g: 52, b: 52, a: 1 };
  const [hex, setHex] = useState(toHex6(start));
  const [alpha, setAlpha] = useState(Math.round(start.a * 100));
  const rgb = parseCssColor(hex) ?? start;
  const out: Rgba = { ...rgb, a: alpha / 100 };
  return (
    <Isolated>
      <DialogShell title={title} width={320} onCancel={onCancel} onOk={() => onPick(toCssColor(out))}>
        <div className="ap-picker">
          <span>Color</span>
          <input type="color" aria-label="Color" value={hex} onChange={(e) => setHex(e.target.value)} />
          <span>Opacity</span>
          <input type="range" min={0} max={100} aria-label="Opacity" value={alpha} onChange={(e) => setAlpha(Number(e.target.value))} />
          <span>Preview</span>
          <div className="ap-preview" style={{ background: toCssColor(out) }} />
          <span />
          <button type="button" onClick={() => onPick(null)}>
            No color
          </button>
        </div>
      </DialogShell>
    </Isolated>
  );
}

/** `wxTextEntryDialog`: a label and a text field. */
export function NameDialog({ title, label, initial = "", onOk, onCancel }: { title: string; label: string; initial?: string; onOk: (name: string) => void; onCancel: () => void }) {
  const [text, setText] = useState(initial);
  return (
    <Isolated>
      <DialogShell title={title} width={340} onCancel={onCancel} onOk={() => onOk(text)} okDisabled={text.trim() === ""}>
        <label style={{ display: "flex", flexDirection: "column", gap: 6, fontSize: 12 }}>
          {label}
          <input autoFocus aria-label={label} value={text} onChange={(e) => setText(e.target.value)} onKeyDown={(e) => { if (e.key === "Enter" && text.trim() !== "") onOk(text); }} />
        </label>
      </DialogShell>
    </Isolated>
  );
}

/** `EDA_LIST_DIALOG` with one column: pick one of `items`. */
export function PickDialog({ title, header, label, items, onOk, onCancel }: { title: string; header: string; label: string; items: string[]; onOk: (item: string) => void; onCancel: () => void }) {
  const [sel, setSel] = useState<string>(items[0] ?? "");
  return (
    <Isolated>
      <DialogShell title={title} width={320} onCancel={onCancel} onOk={() => onOk(sel)} okDisabled={items.length === 0}>
        <div style={{ fontSize: 12, marginBottom: 4 }}>{label}</div>
        <select className="ap-pick-list" size={6} aria-label={header} value={sel} onChange={(e) => setSel(e.target.value)}>
          {items.map((it) => (
            <option key={it} value={it}>
              {it}
            </option>
          ))}
        </select>
      </DialogShell>
    </Isolated>
  );
}

/** `IsOK` / `wxMessageBox`: a question (OK and Cancel) or a message (`onOk` only closes it). */
export function MessageDialog({ title, message, onOk, onCancel }: { title: string; message: string; onOk: () => void; onCancel: () => void }) {
  return (
    <Isolated>
      <DialogShell title={title} width={340} onCancel={onCancel} onOk={onOk}>
        <div style={{ fontSize: 12, whiteSpace: "pre-line" }}>{message}</div>
      </DialogShell>
    </Isolated>
  );
}

/** `WX_COLLAPSIBLE_PANE`: a titled box that opens and closes; closed to begin with, as KiCad's are. */
export function Pane({ title, children, defaultOpen = false }: { title: string; children: ReactNode; defaultOpen?: boolean }) {
  return (
    <details className="ap-pane" open={defaultOpen}>
      <summary>{title}</summary>
      <div>{children}</div>
    </details>
  );
}

/** A group of radio buttons that choose one value. */
export function Radios<T extends string>({ name, value, options, onChange }: { name: string; value: T; options: { value: T; label: string; title: string }[]; onChange: (v: T) => void }) {
  return (
    <div className="ap-radios" role="radiogroup" aria-label={name}>
      {options.map((o) => (
        <label key={o.value} title={o.title}>
          <input type="radio" name={name} checked={value === o.value} onChange={() => onChange(o.value)} />
          {o.label}
        </label>
      ))}
    </div>
  );
}
