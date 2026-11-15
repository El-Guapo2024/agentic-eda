// Shared pieces of the Calculator Tools' panels (components/calculator/Panel*.tsx): the unit choosers (`UNIT_SELECTOR_*`), a text box that keeps what
// was typed (the panels' wxTextCtrl, read with `DoubleFromString` / `ToDouble`), a read-only result box with KiCad's warning and error backgrounds
// (`TRANSLINE::warnCol` / `errCol`), a titled box (wxStaticBoxSizer), and the help pane (`HTML_WINDOW` showing `ConvertMarkdown2Html` of the panel's
// `*_help.md` / `*_formula.md`).
import type { CSSProperties, ReactNode } from "react";
import type { UnitSelectorDef } from "../../kicad-port/calc/unitSelectors";
import { convertMarkdown2Html } from "../../kicad-port/markdownHtml";

/** How a value is shown: `TRANSLINE_OK` / `TRANSLINE_WARNING` / `TRANSLINE_ERROR`. */
export type CalcStatus = "ok" | "warning" | "error";

/** `TRANSLINE::errCol` (1, 0.63, 0.63) and `warnCol` (1, 1, 0.57); text stays black on them, as on KiCad's light backgrounds. */
export const CALC_ERROR_BG = "rgb(255, 161, 161)";
export const CALC_WARNING_BG = "rgb(255, 255, 145)";

export function statusStyle(status: CalcStatus | undefined): CSSProperties {
  if (status === "error") return { background: CALC_ERROR_BG, color: "#000" };
  if (status === "warning") return { background: CALC_WARNING_BG, color: "#000" };
  return {};
}

/** A `UNIT_SELECTOR_*` (wxChoice): the index of the unit chosen. */
export function UnitChoice(props: { def: UnitSelectorDef; value: number; onChange: (index: number) => void; testid?: string; disabled?: boolean; label?: string }) {
  return (
    <select
      className="calc-unit"
      data-testid={props.testid}
      aria-label={props.label}
      disabled={props.disabled}
      value={props.value}
      onChange={(e) => props.onChange(Number(e.target.value))}
    >
      {props.def.names.map((n, i) => (
        <option key={n} value={i}>
          {n}
        </option>
      ))}
    </select>
  );
}

/** A wxChoice of fixed strings: the index chosen. */
export function Choice(props: { options: readonly string[]; value: number; onChange: (index: number) => void; testid?: string; disabled?: boolean; label?: string; style?: CSSProperties }) {
  return (
    <select className="calc-choice" data-testid={props.testid} aria-label={props.label} disabled={props.disabled} value={props.value} style={props.style} onChange={(e) => props.onChange(Number(e.target.value))}>
      {props.options.map((n, i) => (
        <option key={`${i}:${n}`} value={i}>
          {n}
        </option>
      ))}
    </select>
  );
}

/** A wxTextCtrl the user types a value into; the text is kept as typed. `status` colours it (a parameter KiCad flags out of range). */
export function CalcInput(props: {
  value: string;
  onChange: (text: string) => void;
  testid?: string;
  label?: string;
  status?: CalcStatus;
  disabled?: boolean;
  bold?: boolean;
  width?: number;
  onEnter?: () => void;
}) {
  return (
    <input
      type="text"
      className="calc-input"
      data-testid={props.testid}
      aria-label={props.label}
      data-status={props.status ?? "ok"}
      disabled={props.disabled}
      value={props.value}
      style={{ ...statusStyle(props.status), ...(props.bold ? { fontWeight: 700 } : {}), ...(props.width ? { width: props.width } : {}) }}
      onChange={(e) => props.onChange(e.target.value)}
      onKeyDown={(e) => {
        if (e.key === "Enter" && props.onEnter) props.onEnter();
        e.stopPropagation();
      }}
    />
  );
}

/** A read-only wxTextCtrl / wxStaticText showing a result. */
export function CalcResult(props: { value: string; testid?: string; label?: string; status?: CalcStatus; bold?: boolean; width?: number }) {
  return (
    <input
      type="text"
      readOnly
      tabIndex={-1}
      className="calc-input calc-result"
      data-testid={props.testid}
      aria-label={props.label}
      data-status={props.status ?? "ok"}
      value={props.value}
      style={{ ...statusStyle(props.status), ...(props.bold ? { fontWeight: 700 } : {}), ...(props.width ? { width: props.width } : {}) }}
    />
  );
}

/** A wxStaticBoxSizer: a titled frame round a group of controls. */
export function CalcBox(props: { title?: string; children: ReactNode; style?: CSSProperties; testid?: string }) {
  return (
    <fieldset className="calc-box" style={props.style} data-testid={props.testid}>
      {props.title ? <legend>{props.title}</legend> : null}
      {props.children}
    </fieldset>
  );
}

/**
 * A label / control / unit grid (the wxFlexGridSizer / wxGridBagSizer most panels lay their rows in). Each row is [label, control, unit?, ...]; a
 * cell may be null.
 */
export function CalcGrid(props: { rows: (ReactNode | null)[][]; columns?: number; testid?: string; style?: CSSProperties }) {
  const cols = props.columns ?? Math.max(1, ...props.rows.map((r) => r.length));
  return (
    <div className="calc-grid" data-testid={props.testid} style={{ gridTemplateColumns: `repeat(${cols}, auto)`, ...props.style }}>
      {props.rows.flatMap((r, i) => Array.from({ length: cols }, (_, j) => <div key={`${i}:${j}`} className="calc-cell">{r[j] ?? null}</div>))}
    </div>
  );
}

/** The panel's help pane (`HTML_WINDOW::SetPage( ConvertMarkdown2Html( help ) )`). The markdown is KiCad's own fixed text, never user input. */
export function CalcHelp(props: { markdown: string; testid?: string; style?: CSSProperties }) {
  return <div className="calc-help" data-testid={props.testid} style={props.style} dangerouslySetInnerHTML={{ __html: convertMarkdown2Html(props.markdown) }} />;
}

/** The common panel frame: KiCad's panels are a scrolled wxPanel; the content is laid out by each panel. */
export function CalcPanel(props: { children: ReactNode; testid: string; style?: CSSProperties }) {
  return (
    <div className="calc-panel" data-testid={props.testid} style={props.style}>
      {props.children}
    </div>
  );
}
