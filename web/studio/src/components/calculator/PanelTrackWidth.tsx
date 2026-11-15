// The Track Width panel (`PANEL_TRACK_WIDTH`, pcb_calculator/calculator_panels/panel_track_width.cpp and its _base, KiCad 25ec2335): the Parameters
// (Current (I), Temperature rise (ΔT), Conductor length, the copper resistivity shown read-only), External Layer Tracks and Internal Layer Tracks
// (width, thickness, cross-section area, resistance, voltage drop, power loss), the formula pane and Reset to Defaults. Typing the current or one of
// the widths makes it the controlling value (`TW_MASTER_*`, shown in bold: `TWUpdateModeDisplay`) and the other two follow; any other box or unit
// recalculates from the controlling value (`OnTWParametersChanged`). The boxes and units are settings (`track_width.*`, as typed); the resistivity
// stays copper's (`LoadSettings` ignores the stored one: "the IPC formula is valid for copper traces"). The arithmetic is kicad-port/calc/trackWidth.ts.
import { useEffect, useState } from "react";
import { printfG } from "../../kicad-port/calc/cformat";
import { calculateTrackWidth, type TwMode } from "../../kicad-port/calc/trackWidth";
import { TRACKS_WIDTH_VERSUS_CURRENT_FORMULA } from "../../kicad-port/calc/formulaHelp";
import type { PcbCalculatorSettings } from "../../kicad-port/calc/calcSettings";
import { UNIT_SELECTOR_LEN, UNIT_SELECTOR_THICKNESS, doubleFromString, unitScale } from "../../kicad-port/calc/unitSelectors";
import { updatePcbCalculatorSettings, usePcbCalculatorSettings } from "../../state/pcbCalculator";
import { CalcBox, CalcGrid, CalcHelp, CalcInput, CalcPanel, CalcResult, UnitChoice } from "./calcKit";

type Tw = PcbCalculatorSettings["track_width"];

/** The track width formula is valid only for copper material. */
const COPPER_RESISTIVITY = 1.72e-8;
const RESISTIVITY_TEXT = printfG(COPPER_RESISTIVITY);

const num = (t: string) => Math.abs(doubleFromString(t));

/** The `OnTWCalculateFrom*` handler of `mode` and `TWDisplayValues`: the results and the boxes it writes (all but the controlling one). */
function compute(mode: TwMode, t: Tw) {
  const extScale = unitScale(UNIT_SELECTOR_LEN, t.ext_track_width_units);
  const intScale = unitScale(UNIT_SELECTOR_LEN, t.int_track_width_units);
  const extThickness = num(t.ext_track_thickness) * unitScale(UNIT_SELECTOR_THICKNESS, t.ext_track_thickness_units);
  const intThickness = num(t.int_track_thickness) * unitScale(UNIT_SELECTOR_THICKNESS, t.int_track_thickness_units);
  const r = calculateTrackWidth(mode, {
    current: num(t.current),
    extWidth: num(t.ext_track_width) * extScale,
    intWidth: num(t.int_track_width) * intScale,
    extThickness,
    intThickness,
    deltaT_C: num(t.delta_tc),
    rho: num(RESISTIVITY_TEXT),
    trackLen: num(t.track_len) * unitScale(UNIT_SELECTOR_LEN, t.track_len_units),
  });
  const boxes: Partial<Tw> = {};
  if (mode !== "current") boxes.current = printfG(r.current);
  if (mode !== "ext_width") boxes.ext_track_width = printfG(r.extWidth / extScale);
  if (mode !== "int_width") boxes.int_track_width = printfG(r.intWidth / intScale);
  return {
    boxes,
    extArea: printfG((r.extWidth * extThickness) / (extScale * extScale)),
    intArea: printfG((r.intWidth * intThickness) / (intScale * intScale)),
    extAreaUnit: `${UNIT_SELECTOR_LEN.names[t.ext_track_width_units] ?? ""}²`,
    intAreaUnit: `${UNIT_SELECTOR_LEN.names[t.int_track_width_units] ?? ""}²`,
    extR: printfG(r.extResistance),
    intR: printfG(r.intResistance),
    extV: printfG(r.extV),
    intV: printfG(r.intV),
    extLoss: printfG(r.extLoss),
    intLoss: printfG(r.intLoss),
  };
}

/** The settings after a change in `mode`: the change, then the boxes the calculation writes; the resistivity saved as shown. */
function apply(mode: TwMode, patch: Partial<Tw>): void {
  updatePcbCalculatorSettings((s) => {
    Object.assign(s.track_width, patch);
    Object.assign(s.track_width, compute(mode, s.track_width).boxes);
    s.track_width.resistivity = RESISTIVITY_TEXT;
  });
}

export function PanelTrackWidth() {
  const t = usePcbCalculatorSettings().track_width;
  const [mode, setMode] = useState<TwMode>("current");
  // `LoadSettings`: the initial calculation, from the current.
  useEffect(() => apply("current", {}), []);
  const r = compute(mode, t);

  /** A box that makes itself the controlling value (`OnTWCalculateFromCurrent` / `FromExtWidth` / `FromIntWidth`). */
  const master = (m: TwMode, patch: Partial<Tw>) => {
    setMode(m);
    apply(m, patch);
  };
  /** `OnTWParametersChanged`. */
  const param = (patch: Partial<Tw>) => apply(mode, patch);

  // `OnTWResetButtonClick`: the values and units back (ChangeValue: no recalculation), then the current set to 1, which recalculates from it.
  const reset = () =>
    master("current", {
      delta_tc: printfG(10.0),
      track_len: printfG(20.0),
      track_len_units: 0,
      ext_track_width: printfG(0.2),
      ext_track_width_units: 0,
      ext_track_thickness: printfG(35.0),
      ext_track_thickness_units: 1,
      int_track_width: printfG(0.2),
      int_track_width_units: 0,
      int_track_thickness: printfG(35.0),
      int_track_thickness_units: 1,
      current: printfG(1.0),
    });

  const bold = (m: TwMode) => (mode === m ? { fontWeight: 700 } : undefined);
  const val = (id: string, v: string) => <span data-testid={id}>{v}</span>;
  const thickness = (side: "ext" | "int") => {
    const key = side === "ext" ? "ext_track_thickness" : "int_track_thickness";
    const ukey = side === "ext" ? "ext_track_thickness_units" : "int_track_thickness_units";
    return [
      "Track thickness (H):",
      <CalcInput testid={`tw-${side}-thickness`} label={`${side === "ext" ? "External" : "Internal"} track thickness`} value={t[key]} onChange={(v) => param({ [key]: v })} />,
      <UnitChoice testid={`tw-${side}-thickness-unit`} label="Track thickness unit" def={UNIT_SELECTOR_THICKNESS} value={t[ukey]} onChange={(i) => param({ [ukey]: i })} />,
    ];
  };
  const results = (side: "ext" | "int") => {
    const e = side === "ext";
    return [
      ["Cross-section area:", val(`tw-${side}-area`, e ? r.extArea : r.intArea), val(`tw-${side}-area-unit`, e ? r.extAreaUnit : r.intAreaUnit)],
      ["Resistance:", val(`tw-${side}-resistance`, e ? r.extR : r.intR), "Ω"],
      ["Voltage drop:", val(`tw-${side}-vdrop`, e ? r.extV : r.intV), "V"],
      ["Power loss:", val(`tw-${side}-loss`, e ? r.extLoss : r.intLoss), "W"],
    ];
  };

  return (
    <CalcPanel testid="calc-panel-track_width" style={{ flexDirection: "row", alignItems: "flex-start", flexWrap: "wrap" }}>
      <div style={{ display: "flex", flexDirection: "column", gap: 8, flex: 1, minWidth: 320 }}>
        <CalcBox title="Parameters">
          <CalcGrid
            rows={[
              [<span style={bold("current")}>Current (I):</span>, <CalcInput testid="tw-current" label="Current" bold={mode === "current"} value={t.current} onChange={(v) => master("current", { current: v })} />, "A"],
              ["Temperature rise (ΔT):", <CalcInput testid="tw-delta-t" label="Temperature rise" value={t.delta_tc} onChange={(v) => param({ delta_tc: v })} />, "°C"],
              [
                "Conductor length:",
                <CalcInput testid="tw-length" label="Conductor length" value={t.track_len} onChange={(v) => param({ track_len: v })} />,
                <UnitChoice testid="tw-length-unit" label="Conductor length unit" def={UNIT_SELECTOR_LEN} value={t.track_len_units} onChange={(i) => param({ track_len_units: i })} />,
              ],
              ["Copper resistivity:", <CalcResult testid="tw-resistivity" label="Copper resistivity" value={RESISTIVITY_TEXT} />, "Ω⋅m"],
            ]}
          />
        </CalcBox>
        <CalcHelp testid="tw-formula" markdown={TRACKS_WIDTH_VERSUS_CURRENT_FORMULA} style={{ flex: 1 }} />
      </div>
      <div style={{ display: "flex", flexDirection: "column", gap: 8 }}>
        <CalcBox title="External Layer Tracks">
          <CalcGrid
            rows={[
              [
                <span style={bold("ext_width")}>Track width (W):</span>,
                <CalcInput testid="tw-ext-width" label="External track width" bold={mode === "ext_width"} value={t.ext_track_width} onChange={(v) => master("ext_width", { ext_track_width: v })} />,
                <UnitChoice testid="tw-ext-width-unit" label="External track width unit" def={UNIT_SELECTOR_LEN} value={t.ext_track_width_units} onChange={(i) => param({ ext_track_width_units: i })} />,
              ],
              thickness("ext"),
              ...results("ext"),
            ]}
          />
        </CalcBox>
        <CalcBox title="Internal Layer Tracks">
          <CalcGrid
            rows={[
              [
                <span style={bold("int_width")}>Track width (W):</span>,
                <CalcInput testid="tw-int-width" label="Internal track width" bold={mode === "int_width"} value={t.int_track_width} onChange={(v) => master("int_width", { int_track_width: v })} />,
                <UnitChoice testid="tw-int-width-unit" label="Internal track width unit" def={UNIT_SELECTOR_LEN} value={t.int_track_width_units} onChange={(i) => param({ int_track_width_units: i })} />,
              ],
              thickness("int"),
              ...results("int"),
            ]}
          />
        </CalcBox>
        <div>
          <button type="button" data-testid="tw-reset" onClick={reset}>
            Reset to Defaults
          </button>
        </div>
      </div>
    </CalcPanel>
  );
}
