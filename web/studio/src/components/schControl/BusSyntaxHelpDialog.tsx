// Show Bus Syntax Help (`eeschema.InspectionTool.showBusSyntaxHelp`, `SCH_TEXT::ShowSyntaxHelp`): the table of text markup, variables and bus
// definitions KiCad's syntax help window shows -- its text is `eeschema/sch_text_help_md.h` at 8303b2ad.
import { SchDialogFrame } from "./SchDialogFrame";

/** [markup, what it shows] */
type Row = readonly [string, string];

const MARKUP: readonly Row[] = [
  ["^{superscript}", "superscript (raised)"],
  ["Driver Board^{Rev A}", "Driver Board, with Rev A raised"],
  ["_{subscript}", "subscript (lowered)"],
  ["D_{0} - D_{15}", "D0 - D15, with the numbers lowered"],
  ["~{overbar}", "overbar (a line over the text)"],
  ["~{CLK}", "CLK with a line over it"],
  ["${variable}", "variable_value"],
  ["${REVISION}", "2020.1"],
  ["${refdes:field}", "field_value of symbol refdes"],
  ["${R3:VALUE}", "150K"],
  ["${ROW} (in tables)", "0, 1, 2... (0-based)"],
  ["${COL} (in tables)", "0, 1, 2... (0-based)"],
  ["${ADDR} (in tables)", "A0, B1, C2... (0-based)"],
  ["@{expression}", "evaluated_result"],
  ["@{2 + 3}", "5"],
  ["@{${ROW} + 1}", "4 (when ROW=3)"],
];

const CONDITIONAL: readonly Row[] = [
  ['@{"text" == "text"}', "1"],
  ['@{"text" != "other"}', "1"],
  ["@{if(condition, true_val, false_val)}", "Conditional text display"],
  ['@{if("${LAYER}" == "F.Cu", "TOP", "BOTTOM")}', "TOP (on front layer) or BOTTOM"],
  ['@{if(${ROW} > 5, "High", "Low")}', "Numeric comparisons work too"],
];

const PIN_FUNCTIONS: readonly Row[] = [
  ["${refdes:REFERENCE(pin)}", "Full reference with unit for pin"],
  ["${J1:REFERENCE(3)}", "J1B (for multi-unit symbol)"],
  ["${refdes:SHORT_REFERENCE(pin)}", "Reference without unit letter for pin"],
  ["${J1:SHORT_REFERENCE(3)}", "J1"],
  ["${refdes:UNIT(pin)}", "Unit letter only for pin"],
  ["${J1:UNIT(3)}", "B (unit letter for pin 3)"],
  ["${refdes:NET_NAME(pin)}", "Net name connected to pin"],
  ["${R1:NET_NAME(1)}", "VCC"],
  ["${refdes:PIN_NAME(pin)}", "Pin name or selected alternate"],
  ["${U1:PIN_NAME(5)}", "USART1_TX (alternate) or PA9 (base)"],
  ["${refdes:PIN_BASE_NAME(pin)}", "Base pin name (ignoring alternates)"],
  ["${U1:PIN_BASE_NAME(5)}", "PA9"],
  ["${refdes:PIN_ALT_LIST(pin)}", "All alternate pin functions (excludes base name)"],
  ["${U1:PIN_ALT_LIST(5)}", "USART1_TX, TIM1_CH2, I2C1_SCL"],
  ["${refdes:SHORT_NET_NAME(pin)}", "Short net name or NC if unconnected"],
  ["${J1:SHORT_NET_NAME(3)}", "GND or NC"],
  ["${refdes:NET_CLASS(pin)}", "Net class for pin"],
  ["${J1:NET_CLASS(1)}", "Power"],
];

const ESCAPES: readonly Row[] = [
  ["\\${LITERAL}", "${LITERAL} (not expanded)"],
  ["Price: \\$25.00", "Price: $25.00"],
  ["\\@{x+y}", "@{x+y} (not evaluated)"],
];

const NESTED: readonly Row[] = [
  ["${J1:REFERENCE(@{${ROW}+2})}", "J1B (when ROW=0, pin 2 in unit B)"],
  ["${J1:NET_NAME(${COL})}", "Dynamic net lookup in tables"],
];

const CELLS: readonly Row[] = [
  ['${CELL("A0")}', "Evaluated value from cell A0"],
  ["${CELL(0, 1)}", "Value from row 0, column 1"],
  ["${CELL(${ADDR})}", "Dynamic cell reference"],
  ["${CELL(${ROW}-1, ${COL})}", "Value from cell above (if ROW > 0)"],
];

const BUSES: readonly Row[] = [
  ["prefix[m..n]", "prefixm to prefixn"],
  ["D[0..7]", "D0, D1, D2, D3, D4, D5, D6, D7"],
  ["{net1 net2 ...}", "net1, net2, ..."],
  ["{SCL SDA}", "SCL, SDA"],
  ["prefix{net1 net2 ...}", "prefix.net1, prefix.net2, ..."],
  ["USB1{D+ D-}", "USB1.D+, USB1.D-"],
  ["MEM{D[1..2] LATCH}", "MEM.D1, MEM.D2, MEM.LATCH"],
  ["MEM{D_{[1..2]} ~{LATCH}}", "MEM.D1, MEM.D2 (lowered numbers), MEM.LATCH (overbar)"],
];

function Section({ title, left, right, rows }: { title?: string; left: string; right: string; rows: readonly Row[] }) {
  return (
    <>
      {title && <h4 style={{ margin: "14px 0 4px" }}>{title}</h4>}
      <table style={{ width: "100%", borderCollapse: "collapse", fontSize: 12 }}>
        <thead>
          <tr style={{ textAlign: "left" }}>
            <th style={{ width: "48%", padding: "2px 6px", borderBottom: "1px solid var(--chrome-border)" }}>{left}</th>
            <th style={{ padding: "2px 6px", borderBottom: "1px solid var(--chrome-border)" }}>{right}</th>
          </tr>
        </thead>
        <tbody>
          {rows.map(([markup, result]) => (
            <tr key={markup}>
              <td style={{ padding: "2px 6px", fontFamily: "monospace" }}>{markup}</td>
              <td style={{ padding: "2px 6px" }}>{result}</td>
            </tr>
          ))}
        </tbody>
      </table>
    </>
  );
}

export function BusSyntaxHelpDialog({ onClose }: { onClose: () => void }) {
  return (
    <SchDialogFrame title="Syntax Help" width={640} onClose={onClose}>
      <Section left="Markup" right="Result" rows={MARKUP} />
      <Section title="String Comparison & Conditional Text" left="Markup" right="Result" rows={CONDITIONAL} />
      <Section title="Symbol Pin Functions" left="Markup" right="Result" rows={PIN_FUNCTIONS} />
      <Section title="Escape Sequences" left="Markup" right="Result" rows={ESCAPES} />
      <Section title="Nested Variables" left="Markup" right="Result" rows={NESTED} />
      <Section title="Table Cell References" left="Markup" right="Result" rows={CELLS} />
      <Section title="Bus Definition" left="Bus Definition" right="Resultant Nets" rows={BUSES} />
      <p style={{ fontStyle: "italic", marginTop: 12 }}>Note that markup has precedence over bus definitions.</p>
      <p>
        <b>Pin Functions:</b> Automatically find the correct unit placement. For multi-unit symbols, functions like <code>NET_NAME(pin)</code> work even if the pin is in a different unit than the one on the current sheet.
      </p>
      <p>
        <b>Table Cell References:</b> The <code>CELL()</code> function works only in table cells. Use <code>{'${CELL("A0")}'}</code> or <code>{"${CELL(row, col)}"}</code> to reference other cells in the same table. Row and column numbers are 0-based (A0 is the first row, first column). CELL returns the evaluated/displayed value, not the raw cell text.
      </p>
      <p>
        <b>Nested Variables:</b> Variables can contain other variables. Inner variables are expanded first. Maximum nesting depth: 6 levels.
      </p>
      <p>
        <b>Error Messages:</b>
      </p>
      <ul>
        <li>
          <code>&lt;UNRESOLVED: token&gt;</code> - Variable or function cannot be resolved
        </li>
        <li>
          <code>&lt;Unit X not placed&gt;</code> - Pin is in a unit not placed on any sheet
        </li>
        <li>
          <code>&lt;Unresolved: Cell X not found&gt;</code> - Cell address is out of table bounds
        </li>
      </ul>
    </SchDialogFrame>
  );
}
