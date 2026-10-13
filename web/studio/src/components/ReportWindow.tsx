// The message window of the footprint dialogs (`WX_HTML_REPORT_PANEL`, `RPT_SEVERITY_*`): Update PCB from Schematic, Change and Update Footprints, Compare Footprint with
// Library and Reannotate all report their work as lines of text, coloured by how serious they are.
import type { ReportLine } from "../kicad-port/pcbExchange";

const COLOR: Record<ReportLine["severity"], string> = { info: "var(--chrome-text-dim)", action: "var(--chrome-text)", warning: "#d4a017", error: "#e5484d" };

export function ReportWindow({ lines, testId, height = 150, empty }: { lines: readonly ReportLine[]; testId: string; height?: number; empty?: string }) {
  return (
    <div data-testid={testId} style={{ height, overflow: "auto", border: "1px solid var(--chrome-border, #444)", padding: 4, fontFamily: "var(--mono, monospace)", fontSize: 11 }}>
      {lines.length === 0 && empty && <div style={{ color: COLOR.info }}>{empty}</div>}
      {lines.map((l, i) => (
        <div key={i} data-severity={l.severity} style={{ color: COLOR[l.severity] }}>
          {l.text}
        </div>
      ))}
    </div>
  );
}
