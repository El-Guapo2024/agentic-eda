// The Calculator Tools' Transmission Lines panel (`PANEL_TRANSLINE`, pcb_calculator/calculator_panels/panel_transline_base.cpp for the layout and
// labels, KiCad 25ec2335): the line type at the left; the substrate (dielectric model, soldermask overlay, the type's substrate rows with the "..."
// lists of common materials) and component parameters (frequency, spec frequency) in the middle; the physical parameters, Analyze / Synthesize, the
// electrical parameters and the result lines at the right; Reset to Defaults. The logic is kicad-port/calc/translinePanel.ts, which keeps the panel's
// controls as KiCad does (one set of boxes relabelled for each type); this component draws them. The type's picture is not drawn.
//
// Settings: `translines.*` and `trans_line.<type>.values / units` (`PANEL_TRANSLINE::SaveSettings`), written whenever something on the panel changes
// (KiCad writes them when the window closes) and read when the panel is made (`LoadSettings`).
import { useReducer, useRef, useState } from "react";
import { CalcBox, CalcInput, CalcPanel, Choice, UnitChoice } from "./calcKit";
import { getPcbCalculator, updatePcbCalculatorSettings } from "../../state/pcbCalculator";
import { UNIT_SELECTOR_FREQUENCY } from "../../kicad-port/calc/unitSelectors";
import {
  DIELECTRIC_MODEL_CHOICES,
  ELEC_SLOTS,
  PHYS_SLOTS,
  SLOT_UNITS,
  STANDARD_LOSS_TANGENTS,
  STANDARD_RELATIVE_DIELECTRIC_CONSTANTS,
  STANDARD_RESISTIVITIES,
  SUBS_SLOTS,
  TRANSLINE_TYPES,
  TranslinePanelModel,
  findMatch,
  listValue,
  soldermaskShown,
  type SlotKey,
} from "../../kicad-port/calc/translinePanel";

/** The "..." buttons (`OnTranslineEpsilonR_Button` / `TanD` / `Rho`): the dialog title and the list. */
const LISTS: Partial<Record<SlotKey, { title: string; rows: readonly string[] }>> = {
  subs0: { title: "Relative Dielectric Constants", rows: STANDARD_RELATIVE_DIELECTRIC_CONSTANTS },
  subs1: { title: "Dielectric Loss Factor", rows: STANDARD_LOSS_TANGENTS },
  subs2: { title: "Specific Resistance", rows: STANDARD_RESISTIVITIES },
};

/** `wxGetSingleChoiceIndex`: the list (with KiCad's empty last row for "no selection"), the row matching the box preselected, OK / Cancel. */
function ChoiceList(props: { title: string; rows: readonly string[]; initial: number; onPick: (row: string | null) => void }) {
  const [sel, setSel] = useState(props.initial);
  const rows = [...props.rows, ""];
  return (
    <div className="dialog-backdrop" style={{ zIndex: 2300 }} onClick={() => props.onPick(null)}>
      <div className="dialog" role="dialog" aria-label={props.title} data-testid="tl-choice-dialog" style={{ width: 340 }} onClick={(e) => e.stopPropagation()}>
        <div className="dialog-header">
          <span>{props.title}</span>
        </div>
        <div className="dialog-body">
          <select
            size={Math.min(rows.length, 14)}
            style={{ width: "100%", font: "inherit" }}
            value={sel}
            data-testid="tl-choice-list"
            onChange={(e) => setSel(Number(e.target.value))}
            onDoubleClick={() => props.onPick(rows[sel] ?? null)}
          >
            {rows.map((r, i) => (
              <option key={i} value={i}>
                {r.replace("\t", "  ")}
              </option>
            ))}
          </select>
        </div>
        <div className="dialog-footer">
          <button onClick={() => props.onPick(null)}>Cancel</button>
          <button className="primary" data-testid="tl-choice-ok" onClick={() => props.onPick(rows[sel] ?? null)}>
            OK
          </button>
        </div>
      </div>
    </div>
  );
}

export function PanelTransline() {
  const model = useRef<TranslinePanelModel | null>(null);
  if (!model.current) {
    model.current = new TranslinePanelModel();
    model.current.loadSettings(getPcbCalculator().settings);
  }
  const m = model.current;
  const [, redraw] = useReducer((n: number) => n + 1, 0);
  const [list, setList] = useState<SlotKey | null>(null);

  /** A change on the panel: apply it, store the settings (`SaveSettings`), draw again. */
  const change = (fn: () => void) => {
    fn();
    updatePcbCalculatorSettings((s) => m.saveSettings(s));
    redraw();
  };

  const mask = soldermaskShown(m.current);
  const enables = m.soldermaskEnables;
  const def = TRANSLINE_TYPES[m.current]!;

  const row = (key: SlotKey) => {
    const slot = m.slots[key];
    const unitDef = SLOT_UNITS[key];
    const lists = LISTS[key];
    const radio = key === "phys0" ? 1 : key === "phys1" ? 2 : 0;
    return (
      <div key={key} style={{ display: "contents" }}>
        <span title={slot.tooltip} data-testid={`tl-${key}-label`}>
          {slot.label}
        </span>
        <CalcInput value={slot.text} disabled={!slot.enabled} status={slot.status} testid={`tl-${key}`} label={slot.label || key} onChange={(t) => change(() => void (slot.text = t))} />
        <span style={{ display: "flex", gap: 4, alignItems: "center" }}>
          {lists ? (
            <button type="button" data-testid={`tl-${key}-list`} title={lists.title} onClick={() => setList(key)}>
              ...
            </button>
          ) : null}
          {key === "subs2" ? <span>Ω ∙ m</span> : null}
          {unitDef && slot.unitShown ? <UnitChoice def={unitDef} value={slot.unit} testid={`tl-${key}-unit`} label={`${slot.label} unit`} onChange={(i) => change(() => void (slot.unit = i))} /> : null}
          {radio && def.hasPrmSelection ? (
            <input
              type="radio"
              name="tl-prm-selection"
              data-testid={`tl-radio${radio}`}
              aria-label={`Synthesize ${slot.label}`}
              checked={m.radio === radio}
              onChange={() => change(() => void (m.radio = radio as 1 | 2))}
            />
          ) : null}
        </span>
      </div>
    );
  };

  const grid = (keys: readonly SlotKey[]) => (
    <div className="calc-grid" style={{ gridTemplateColumns: "auto auto auto" }}>
      {keys.map(row)}
    </div>
  );

  return (
    <CalcPanel testid="calc-panel-transline">
      <div style={{ display: "flex", gap: 10, alignItems: "flex-start", flexWrap: "wrap" }}>
        <CalcBox title="Transmission Line Type" testid="tl-type">
          <div role="radiogroup" aria-label="Transmission Line Type" style={{ display: "flex", flexDirection: "column", gap: 3 }}>
            {TRANSLINE_TYPES.map((t, i) => (
              <label key={t.name} style={{ display: "flex", gap: 4, alignItems: "center", whiteSpace: "nowrap" }}>
                <input type="radio" name="tl-type" data-testid={`tl-type-${i}`} checked={m.current === i} onChange={() => change(() => m.selectType(i))} />
                {t.label}
              </label>
            ))}
          </div>
        </CalcBox>

        <div style={{ display: "flex", flexDirection: "column", gap: 8 }}>
          <CalcBox title="Substrate Parameters">
            <div style={{ display: "flex", flexDirection: "column", gap: 4, marginBottom: 6 }}>
              <label style={{ display: "flex", gap: 6, alignItems: "center" }}>
                Dielectric model:
                <Choice options={DIELECTRIC_MODEL_CHOICES} value={m.dielectricModel} testid="tl-dielectric-model" label="Dielectric model" onChange={(i) => change(() => void (m.dielectricModel = i))} />
              </label>
              {mask.group ? (
                <>
                  <div style={{ display: "flex", gap: 12 }}>
                    <label title="Enable solder resist / LPI overlay correction.  Affects εeff, Z0, and dielectric loss for microstrip, coupled microstrip, CPW, and CBCPW.">
                      <input type="checkbox" data-testid="tl-mask-present" checked={m.soldermaskPresent} onChange={(e) => change(() => void (m.soldermaskPresent = e.target.checked))} /> Soldermask present
                    </label>
                    {mask.fillsGaps ? (
                      <label title={"Enable when the mask fills the CPW slots (standard LPI process).\nDisable for selective mask that covers only the traces."}>
                        <input type="checkbox" data-testid="tl-mask-fills" disabled={!enables.fillsGaps} checked={m.soldermaskFillsGaps} onChange={(e) => change(() => void (m.soldermaskFillsGaps = e.target.checked))} /> Mask
                        fills gaps
                      </label>
                    ) : null}
                  </div>
                  <div className="calc-grid" style={{ gridTemplateColumns: "auto auto auto" }}>
                    <span title="Cured mask thickness.  Typical LPI is 15-30 um; set to 0 or uncheck to disable.">Mask thickness:</span>
                    <CalcInput value={m.soldermaskThicknessText} disabled={!enables.fields} testid="tl-mask-thickness" label="Mask thickness" onChange={(t) => change(() => void (m.soldermaskThicknessText = t))} />
                    <span>µm</span>
                    <span title="Mask relative permittivity.  Default 3.5 for standard green LPI.  Range 3.3-3.8 for typical resins.">Mask εr:</span>
                    <CalcInput value={m.soldermaskEpsilonRText} disabled={!enables.fields} testid="tl-mask-er" label="Mask εr" onChange={(t) => change(() => void (m.soldermaskEpsilonRText = t))} />
                    <span />
                    <span title="Mask loss tangent.  Default 0.025 for LPI.">Mask tan δ:</span>
                    <CalcInput value={m.soldermaskTanDText} disabled={!enables.fields} testid="tl-mask-tand" label="Mask tan δ" onChange={(t) => change(() => void (m.soldermaskTanDText = t))} />
                    <span />
                  </div>
                </>
              ) : null}
            </div>
            {grid(SUBS_SLOTS)}
          </CalcBox>
          <CalcBox title="Component Parameters">
            <div className="calc-grid" style={{ gridTemplateColumns: "auto auto auto" }}>
              {row("freq0")}
              <span title={"'Constant': εr and tan δ applied at all frequencies.\n'Djordjevic-Sarkar': causal wideband Debye anchored at the spec frequency."}>Spec frequency:</span>
              <CalcInput value={m.specFrequencyText} disabled={!m.specFrequencyEnabled} testid="tl-spec-freq" label="Spec frequency" onChange={(t) => change(() => void (m.specFrequencyText = t))} />
              <UnitChoice
                def={UNIT_SELECTOR_FREQUENCY}
                value={m.specFrequencyUnit}
                disabled={!m.specFrequencyEnabled}
                testid="tl-spec-freq-unit"
                label="Spec frequency unit"
                onChange={(i) => change(() => void (m.specFrequencyUnit = i))}
              />
            </div>
          </CalcBox>
        </div>

        <div style={{ display: "flex", flexDirection: "column", gap: 8 }}>
          <CalcBox title="Physical Parameters">{grid(PHYS_SLOTS)}</CalcBox>
          <div style={{ display: "flex", gap: 8, justifyContent: "center" }}>
            <button type="button" data-testid="tl-analyze" onClick={() => change(() => m.analyze())}>
              {"▼ Analyze"}
            </button>
            <button type="button" data-testid="tl-synthesize" onClick={() => change(() => m.synthesize())}>
              {"▲ Synthesize"}
            </button>
          </div>
          <CalcBox title="Electrical Parameters">{grid(ELEC_SLOTS)}</CalcBox>
          <CalcBox title="Results" testid="tl-results">
            <div className="calc-grid" style={{ gridTemplateColumns: "auto auto" }}>
              {m.messages.map((l, i) => (
                <div key={i} style={{ display: "contents" }}>
                  <span data-testid={`tl-result-label-${i}`}>{l.label}</span>
                  <span data-testid={`tl-result-${i}`} style={{ userSelect: "text" }}>
                    {l.value}
                  </span>
                </div>
              ))}
            </div>
          </CalcBox>
        </div>
      </div>
      <div>
        <button type="button" data-testid="tl-reset" onClick={() => change(() => m.reset())}>
          Reset to Defaults
        </button>
      </div>
      {list && LISTS[list] ? (
        <ChoiceList
          title={LISTS[list]!.title}
          rows={LISTS[list]!.rows}
          initial={findMatch(LISTS[list]!.rows, m.slots[list].text)}
          onPick={(r) => {
            const key = list;
            setList(null);
            // `if( index >= 0 && !list.Item( index ).IsEmpty() )`
            if (r) change(() => void (m.slots[key].text = listValue(r)));
          }}
        />
      ) : null}
    </CalcPanel>
  );
}
