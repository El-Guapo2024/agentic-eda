// Small pieces the Board Setup pages share: a length field that reads units, the label/control/message row, the page frame with its Apply
// button, and the draft a page edits until Apply (KiCad's panels edit a copy too and write it on OK).
import { useEffect, useMemo, useState, type ReactNode } from "react";
import type { Cmd } from "../../api/types";
import { useStudioApi } from "../../state/store";
import type { LengthUnit } from "../../state/units";
import { lengthText, parseLength } from "../pcbDialogKit";

/** What the dialog gives each page: whether it is the one showing, and a way to say the page has edits not applied yet (the page tree marks it). */
export interface PageProps {
  hidden: boolean;
  onDirty: (dirty: boolean) => void;
}

/** The text of a length field as the value reads: blank is `undefined` (a net class cell that inherits), text that is no length is `NaN`. */
function readLength(text: string, units: LengthUnit, optional: boolean): number | undefined {
  const t = text.trim();
  if (t === "") return optional ? undefined : Number.NaN;
  // "5." is a number being typed: read it as 5.
  const parsed = parseLength(t.replace(/\.(\s*(?:mm|mil|in|")?)$/i, "$1"), units);
  return parsed === null ? Number.NaN : parsed;
}

function showLength(um: number | undefined, units: LengthUnit): string {
  return um === undefined || Number.isNaN(um) ? "" : lengthText(um, units);
}

export interface LengthFieldProps {
  /** µm. `undefined` is a blank cell when `optional`; `NaN` stands for text that is not a length (the page's checks name it). */
  value: number | undefined;
  onChange: (um: number | undefined) => void;
  units: LengthUnit;
  /** A blank cell is a value (inherit the Default class' size) and not a mistake. */
  optional?: boolean;
  invalid?: boolean;
  disabled?: boolean;
  title?: string;
  ariaLabel?: string;
  width?: number;
  placeholder?: string;
}

/**
 * A length in the display units that also takes a suffix ("0.2", "8mil", `0.01"`), as KiCad's UNIT_BINDER does. It keeps the text typed and
 * hands the page the value in µm as it goes; the page validates on Apply, as KiCad's panels do on OK.
 */
export function LengthField({ value, onChange, units, optional = false, invalid, disabled, title, ariaLabel, width, placeholder }: LengthFieldProps) {
  const [text, setText] = useState(() => showLength(value, units));
  // Follow the value when it changes from outside the field (the page reseeded, the units changed), but not while the text already says it:
  // 0.1234567 reads as 123 um, and rewriting it to 0.123 under the user's fingers would be rude.
  useEffect(() => {
    if (!Object.is(readLength(text, units, optional), value)) setText(showLength(value, units));
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [value, units]);
  return (
    <input
      type="text"
      className={`bs-input${invalid ? " bs-invalid" : ""}`}
      style={width ? { width } : undefined}
      value={text}
      disabled={disabled}
      title={title}
      aria-label={ariaLabel}
      placeholder={placeholder}
      spellCheck={false}
      onChange={(e) => {
        setText(e.target.value);
        onChange(readLength(e.target.value, units, optional));
      }}
    />
  );
}

export interface NumberFieldProps {
  value: number | undefined;
  onChange: (v: number | undefined) => void;
  /** A blank field is `undefined`, not a mistake. */
  optional?: boolean;
  invalid?: boolean;
  disabled?: boolean;
  title?: string;
  ariaLabel?: string;
  width?: number;
}

/** A plain number (a count, a percentage, a dielectric constant). Text that is no number is `NaN`. */
export function NumberField({ value, onChange, optional = false, invalid, disabled, title, ariaLabel, width }: NumberFieldProps) {
  const read = (t: string): number | undefined => (t.trim() === "" ? (optional ? undefined : Number.NaN) : Number(t.trim().replace(/,/g, ".").replace(/\.$/, "")));
  const show = (v: number | undefined) => (v === undefined || Number.isNaN(v) ? "" : String(v));
  const [text, setText] = useState(() => show(value));
  useEffect(() => {
    if (!Object.is(read(text), value)) setText(show(value));
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [value]);
  return (
    <input
      type="text"
      className={`bs-input${invalid ? " bs-invalid" : ""}`}
      style={width ? { width } : undefined}
      value={text}
      disabled={disabled}
      title={title}
      aria-label={ariaLabel}
      spellCheck={false}
      onChange={(e) => {
        setText(e.target.value);
        onChange(read(e.target.value));
      }}
    />
  );
}

/** A label, the control and what is wrong with it, on one line of the page's grid. */
export function FieldRow({ label, title, error, unit, children }: { label: string; title?: string; error?: string | null; unit?: string; children: ReactNode }) {
  return (
    <div className="bs-row" title={title}>
      <span className="bs-label">{label}</span>
      <span className="bs-control">
        {children}
        {unit && <span className="bs-unit">{unit}</span>}
      </span>
      <span className="bs-error">{error ?? ""}</span>
    </div>
  );
}

export function Check({ checked, onChange, children, title, disabled }: { checked: boolean; onChange: (v: boolean) => void; children: ReactNode; title?: string; disabled?: boolean }) {
  return (
    <label className="bs-check" title={title}>
      <input type="checkbox" checked={checked} disabled={disabled} onChange={(e) => onChange(e.target.checked)} /> {children}
    </label>
  );
}

/**
 * One page of the dialog: its title, the content, and the Apply / Revert buttons with what happened. The page stays mounted while the dialog is
 * open (a hidden section), so what was typed on it survives a visit to another page.
 */
export function PageFrame({
  title,
  hidden,
  dirty,
  busy,
  message,
  bad,
  onApply,
  onRevert,
  applyLabel = "Apply",
  extraActions,
  children,
}: {
  title: string;
  hidden: boolean;
  dirty: boolean;
  busy: boolean;
  message: string | null;
  /** The message is a refusal, not a confirmation. */
  bad?: boolean;
  onApply: () => void;
  onRevert: () => void;
  applyLabel?: string;
  extraActions?: ReactNode;
  children: ReactNode;
}) {
  return (
    <section className="bs-page" hidden={hidden} aria-label={title}>
      <h3 className="bs-title">{title}</h3>
      {children}
      <div className="bs-actions">
        <button className="primary" disabled={busy} onClick={onApply}>
          {applyLabel}
        </button>
        <button disabled={busy || !dirty} onClick={onRevert}>
          Revert
        </button>
        {extraActions}
        <span className={`bs-msg${bad ? " bs-bad" : ""}`} role="status">
          {message ?? (dirty ? "Changes not applied yet." : "")}
        </span>
      </div>
    </section>
  );
}

/**
 * The draft of a page: a copy of what the board says, edited until Apply. While nothing has been edited the draft follows the board (so an
 * Undo from outside the dialog shows up); once the page is edited it is the user's until Apply or Revert.
 */
export function usePageDraft<T>(source: T | undefined) {
  const key = useMemo(() => (source === undefined ? "" : JSON.stringify(source)), [source]);
  const [draft, setDraft] = useState<T | undefined>(source);
  const [dirty, setDirty] = useState(false);
  useEffect(() => {
    if (!dirty) setDraft(source);
    // `source` changes identity with every poll of the board; `key` is what it says.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [key, dirty]);
  return {
    draft,
    dirty,
    /** Replace the draft with `fn` of it and mark the page edited. */
    edit: (fn: (d: T) => T) => {
      setDraft((d) => (d === undefined ? d : fn(d)));
      setDirty(true);
    },
    /** The page was applied: follow the board again. */
    markClean: () => setDirty(false),
    /** Back to what the board says. */
    revert: () => {
      setDraft(source);
      setDirty(false);
    },
  };
}

/**
 * Sends a page's command and keeps what the page says about it: busy while it runs, then "Saved." or that it was refused. The refusal's own
 * words are the toast `api.cmd` raises; the page points at it.
 */
export function usePageApply() {
  const api = useStudioApi();
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState<{ text: string; bad: boolean } | null>(null);
  return {
    busy,
    message,
    setMessage,
    /** Run `cmd`; on success call `onOk` (mark the page clean) and say `okText`. Resolves to whether it was accepted. */
    run: async (cmd: Cmd, onOk: () => void, okText = "Saved."): Promise<boolean> => {
      setBusy(true);
      setMessage(null);
      try {
        const ok = await api.cmd(cmd);
        if (ok) onOk();
        setMessage(ok ? { text: okText, bad: false } : { text: "Not saved: the backend refused it (the reason is in the message at the top of the window).", bad: true });
        return ok;
      } finally {
        setBusy(false);
      }
    },
  };
}
