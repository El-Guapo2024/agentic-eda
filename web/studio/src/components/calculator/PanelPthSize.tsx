// Calculator Tools > Footprint design > Through-hole Size (`PANEL_PTH_SIZE`, pcb_calculator/calculator_panels/panel_pth_size.cpp and its _base at
// KiCad 25ec2335): the lead (shape, min / max size), the hole size standard (IPC-2222B, its level, rounding step and direction) and the annular ring
// give the recommended hole and pad sizes, a drawing of the pad with optional tolerance overlays, a report (with a copy button) and the standard's
// summary table (kicad-port/calc/pthSize.ts). KiCad saves no settings for this panel. The PCB Editor's pad properties dialog opens the calculator
// on this page (`DIALOG_PAD_PROPERTIES::OnHoleSizeCalculator`).
import { useMemo, useRef, useState } from "react";
import {
  PTH_DEFAULTS,
  PTH_HOLE_SIZE_STANDARDS,
  formatMm,
  pthSummaryCells,
  recalculatePth,
  sizeXLabel,
  type PthInputs,
  type PthLeadDef,
  type PthLeadShape,
  type PthOutcome,
} from "../../kicad-port/calc/pthSize";
import { CalcBox, CalcInput, CalcPanel, CalcResult, Choice } from "./calcKit";

const SHAPES: PthLeadShape[] = ["round", "square", "rectangular"];

/**
 * `drawPthSizePreview` at 200 px: the copper pad with the hole cut out, the lead in the hole (its minimum dashed), the outlines, and the dashed
 * allowable hole range. Drawn as SVG over the panel's own background (KiCad renders on white and black to recover a transparent background).
 */
function PthPreview(props: { lead: PthLeadDef; hole: number; holeMin: number; holeMax: number; pad: number; showMinLead: boolean; showMinHole: boolean; showMaxHole: boolean }) {
  const size = 200;
  const { lead, hole, holeMin, holeMax, pad } = props;
  if (pad <= 0) return <svg width={size} height={size} data-testid="pth-preview" />;
  const outline = 1;
  const scale = (size - 2 * outline) / Math.max(pad, holeMax);
  const px = (iu: number) => Math.round(iu * scale);
  const c = size / 2;
  const rect = lead.shape === "rectangular";
  const leadShape = (w: number, h: number, props2: React.SVGProps<SVGCircleElement> & React.SVGProps<SVGRectElement>) =>
    lead.shape === "round" ? <circle cx={c} cy={c} r={Math.trunc(w / 2)} {...props2} /> : <rect x={c - Math.trunc(w / 2)} y={c - Math.trunc(h / 2)} width={w} height={h} {...props2} />;
  const dash = "3 3";
  return (
    <svg width={size} height={size} data-testid="pth-preview" style={{ flex: "none" }}>
      <circle cx={c} cy={c} r={Math.trunc(px(pad) / 2)} fill="#fcb23c" />
      <circle cx={c} cy={c} r={Math.trunc(px(hole) / 2)} fill="var(--chrome-bg-raised)" />
      {leadShape(px(lead.maxX), px(rect ? lead.maxY : lead.maxX), { fill: "#c8c8c8", stroke: "#c8c8c8", strokeWidth: outline })}
      {props.showMinLead && leadShape(px(lead.minX), px(rect ? lead.minY : lead.minX), { fill: "none", stroke: "#808080", strokeWidth: outline, strokeDasharray: dash })}
      <circle cx={c} cy={c} r={Math.trunc(px(pad) / 2)} fill="none" stroke="#808080" strokeWidth={outline} />
      <circle cx={c} cy={c} r={Math.trunc(px(hole) / 2)} fill="none" stroke="#808080" strokeWidth={outline} />
      {props.showMinHole && <circle cx={c} cy={c} r={Math.trunc(px(holeMin) / 2)} fill="none" stroke="#0060f0" strokeWidth={outline} strokeDasharray={dash} />}
      {props.showMaxHole && <circle cx={c} cy={c} r={Math.trunc(px(holeMax) / 2)} fill="none" stroke="#0060f0" strokeWidth={outline} strokeDasharray="8 4" />}
    </svg>
  );
}

export function PanelPthSize() {
  const [inp, setInp] = useState<PthInputs>({ ...PTH_DEFAULTS });
  const [showMinLead, setShowMinLead] = useState(true);
  const [showMinHole, setShowMinHole] = useState(false);
  const [showMaxHole, setShowMaxHole] = useState(false);
  // The last valid calculation: the result boxes and the drawing keep it while an input is invalid; the report is emptied.
  const last = useRef<PthOutcome | null>(null);
  const outcome = useMemo(() => recalculatePth(inp), [inp]);
  if (outcome) last.current = outcome;
  const shown = last.current;
  const std = PTH_HOLE_SIZE_STANDARDS[inp.standard] ?? PTH_HOLE_SIZE_STANDARDS[0]!;
  const set = (patch: Partial<PthInputs>) => setInp((o) => ({ ...o, ...patch }));
  const text = (k: "sizeXMin" | "sizeXMax" | "sizeYMin" | "sizeYMax" | "annular" | "rounding", label: string) => (
    <CalcInput testid={`pth-${k}`} label={label} value={inp[k]} onChange={(v) => set({ [k]: v })} width={80} />
  );
  const rect = inp.shape === "rectangular";
  const report = outcome?.report ?? "";
  const cell = (k: string, c: React.ReactNode) => (
    <div key={k} className="calc-cell" style={{ textAlign: "center" }}>
      {c}
    </div>
  );
  return (
    <CalcPanel testid="calc-panel-pth_size">
      <div style={{ display: "flex", gap: 10, flexWrap: "wrap", alignItems: "flex-start" }}>
        <div style={{ display: "flex", flexDirection: "column", gap: 8 }}>
          <CalcBox title="Properties">
            <div className="calc-grid" style={{ gridTemplateColumns: "auto auto auto auto", justifyContent: "start" }}>
              {cell("a0", "Lead shape:")}
              {cell("a1", <Choice testid="pth-shape" label="Lead shape" options={["Round", "Square", "Rectangular"]} value={SHAPES.indexOf(inp.shape)} onChange={(i) => set({ shape: SHAPES[i]! })} />)}
              {cell("a2", null)}
              {cell("a3", null)}
              {cell("b0", null)}
              {cell("b1", "Min")}
              {cell("b2", "Max")}
              {cell("b3", null)}
              {cell("c0", <span data-testid="pth-sizeX-label">{sizeXLabel(inp.shape)}</span>)}
              {cell("c1", text("sizeXMin", "Minimum size X"))}
              {cell("c2", text("sizeXMax", "Maximum size X"))}
              {cell("c3", "mm")}
              {rect && [cell("d0", "Size Y:"), cell("d1", text("sizeYMin", "Minimum size Y")), cell("d2", text("sizeYMax", "Maximum size Y")), cell("d3", "mm")]}
            </div>
          </CalcBox>
          <CalcBox title="Hole size standard">
            <div className="calc-grid" style={{ gridTemplateColumns: "auto auto auto", justifyContent: "start" }}>
              <div className="calc-cell">Standard:</div>
              <div className="calc-cell">
                {/* `updateStandard`: a new standard lists its levels and selects B (or the last). */}
                <Choice testid="pth-standard" label="Standard" options={PTH_HOLE_SIZE_STANDARDS.map((s) => s.displayName)} value={inp.standard} onChange={(i) => set({ standard: i, level: Math.min(1, (PTH_HOLE_SIZE_STANDARDS[i]?.levelCount ?? 1) - 1) })} />
              </div>
              <div className="calc-cell" />
              <div className="calc-cell">Level:</div>
              <div className="calc-cell">
                <Choice testid="pth-level" label="Level" options={Array.from({ length: std.levelCount }, (_, l) => std.levelName(l))} value={inp.level} onChange={(i) => set({ level: i })} />
              </div>
              <div className="calc-cell" />
              <div className="calc-cell">Round to:</div>
              <div className="calc-cell">{text("rounding", "Round to")}</div>
              <div className="calc-cell">mm</div>
              <div className="calc-cell">Rounding:</div>
              <div className="calc-cell">
                <Choice testid="pth-rounding-choice" label="Rounding" options={["Up", "Nearest"]} value={inp.roundingChoice === "up" ? 0 : 1} onChange={(i) => set({ roundingChoice: i === 0 ? "up" : "nearest" })} />
              </div>
              <div className="calc-cell" />
            </div>
          </CalcBox>
          <CalcBox title="Annular ring">
            <div className="calc-grid" style={{ gridTemplateColumns: "auto auto auto", justifyContent: "start" }}>
              <div className="calc-cell">Annular ring:</div>
              <div className="calc-cell">{text("annular", "Annular ring")}</div>
              <div className="calc-cell">mm</div>
            </div>
          </CalcBox>
        </div>
        <CalcBox title="Result" style={{ flex: 1, minWidth: 360 }}>
          <div style={{ display: "flex", gap: 10, alignItems: "flex-start" }}>
            {shown ? (
              <PthPreview lead={shown.lead} hole={shown.hole} holeMin={shown.range.holeMin} holeMax={shown.range.holeMax} pad={shown.pad} showMinLead={showMinLead} showMinHole={showMinHole} showMaxHole={showMaxHole} />
            ) : (
              <svg width={200} height={200} data-testid="pth-preview" />
            )}
            <div style={{ display: "flex", flexDirection: "column", gap: 4 }}>
              <label>
                <input type="checkbox" data-testid="pth-show-min-lead" checked={showMinLead} onChange={(e) => setShowMinLead(e.target.checked)} /> Show minimum lead size
              </label>
              <label>
                <input type="checkbox" data-testid="pth-show-min-hole" checked={showMinHole} onChange={(e) => setShowMinHole(e.target.checked)} /> Show minimum hole size
              </label>
              <label>
                <input type="checkbox" data-testid="pth-show-max-hole" checked={showMaxHole} onChange={(e) => setShowMaxHole(e.target.checked)} /> Show maximum hole size
              </label>
            </div>
          </div>
          <div className="calc-grid" style={{ gridTemplateColumns: "auto auto auto", justifyContent: "start", marginTop: 6 }}>
            <div className="calc-cell">Recommended hole size:</div>
            <div className="calc-cell">
              <CalcResult testid="pth-hole" label="Recommended hole size" value={shown ? formatMm(shown.hole) : ""} />
            </div>
            <div className="calc-cell">mm</div>
            <div className="calc-cell">Recommended pad size:</div>
            <div className="calc-cell">
              <CalcResult testid="pth-pad" label="Recommended pad size" value={shown ? formatMm(shown.pad) : ""} />
            </div>
            <div className="calc-cell">mm</div>
          </div>
          <div style={{ display: "flex", gap: 4, marginTop: 6 }}>
            <textarea readOnly data-testid="pth-report" aria-label="Report" value={report} style={{ flex: 1, minHeight: 250, fontFamily: "monospace", fontSize: 12, whiteSpace: "pre" }} />
            <button type="button" data-testid="pth-copy" title="Copy the report to the clipboard" aria-label="Copy the report to the clipboard" style={{ alignSelf: "flex-start" }} onClick={() => void navigator.clipboard?.writeText(report).catch(() => {})}>
              ⧉
            </button>
          </div>
        </CalcBox>
      </div>
      <div data-testid="pth-summary">
        <div style={{ fontWeight: 600, marginBottom: 4 }}>{`${std.displayName} Summary`}</div>
        <table className="calc-table">
          <thead>
            <tr>
              <th />
              {pthSummaryCells(std).map((c) => (
                <th key={c.level}>{c.level}</th>
              ))}
            </tr>
          </thead>
          <tbody>
            <tr>
              <th style={{ textAlign: "left" }}>Min. hole diameter</th>
              {pthSummaryCells(std).map((c) => (
                <td key={c.level}>{c.min}</td>
              ))}
            </tr>
            <tr>
              <th style={{ textAlign: "left" }}>Max. hole diameter</th>
              {pthSummaryCells(std).map((c) => (
                <td key={c.level}>{c.max}</td>
              ))}
            </tr>
          </tbody>
        </table>
      </div>
    </CalcPanel>
  );
}
