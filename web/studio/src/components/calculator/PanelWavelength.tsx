// The Wavelength panel (`PANEL_WAVELENGTH`, pcb_calculator/calculator_panels/panel_wavelength.cpp and its _base, KiCad 25ec2335): Frequency, Period,
// Wavelength in vacuum, Wavelength in medium (each typed sets the frequency, if it reads as a number > 0, and rewrites the others), Speed in medium
// (read-only), er (with "..." offering the standard dielectric constants) and mur (each taken when >= 1). Every box is written with
// `wxString( "" ) << value` ("%g"); a unit change rewrites the boxes from the frequency kept. The settings keep the frequency, er, mur and the five
// units (`SaveSettings`). The arithmetic is kicad-port/calc/wavelength.ts.
import { useState } from "react";
import { printfG } from "../../kicad-port/calc/cformat";
import { findMatch, standardRelativeDielectricConstantList, entryValue } from "../../kicad-port/calc/commonData";
import { frequencyFromPeriod, frequencyFromWavelengthMedium, frequencyFromWavelengthVacuum, wavelengthValues } from "../../kicad-port/calc/wavelength";
import { UNIT_SELECTOR_FREQUENCY, UNIT_SELECTOR_LEN_CABLE, UNIT_SELECTOR_SPEED, UNIT_SELECTOR_TIME, toDouble, unitScale } from "../../kicad-port/calc/unitSelectors";
import { getPcbCalculator, updatePcbCalculatorSettings, usePcbCalculatorSettings } from "../../state/pcbCalculator";
import { CalcGrid, CalcInput, CalcPanel, CalcResult, UnitChoice } from "./calcKit";
import { useSingleChoice } from "./SingleChoiceDialog";

type Box = "frequency" | "period" | "vacuum" | "medium";
type Texts = Record<Box | "speed" | "er" | "mur", string>;

interface Units {
  frequencyUnit: number;
  periodUnit: number;
  wavelengthVacuumUnit: number;
  wavelengthMediumUnit: number;
  speedUnit: number;
}

/** `PANEL_WAVELENGTH::update`: every box but the one being typed (`m_updating*`). */
function updated(texts: Texts, freq: number, er: number, mur: number, u: Units, skip: Box | null): Texts {
  const w = wavelengthValues(freq, er, mur);
  const next = { ...texts };
  if (skip !== "frequency") next.frequency = printfG(w.frequency / unitScale(UNIT_SELECTOR_FREQUENCY, u.frequencyUnit));
  if (skip !== "period") next.period = printfG(w.period / unitScale(UNIT_SELECTOR_TIME, u.periodUnit));
  if (skip !== "vacuum") next.vacuum = printfG(w.wavelengthVacuum / unitScale(UNIT_SELECTOR_LEN_CABLE, u.wavelengthVacuumUnit));
  if (skip !== "medium") next.medium = printfG(w.wavelengthMedium / unitScale(UNIT_SELECTOR_LEN_CABLE, u.wavelengthMediumUnit));
  next.speed = printfG(w.speed / unitScale(UNIT_SELECTOR_SPEED, u.speedUnit));
  return next;
}

export function PanelWavelength() {
  const s = usePcbCalculatorSettings().wavelength;
  const [dialog, ask] = useSingleChoice("wavelength-er-list");
  // `LoadSettings`.
  const [texts, setTexts] = useState<Texts>(() => {
    const w = getPcbCalculator().settings.wavelength;
    const t: Texts = { frequency: "", period: "", vacuum: "", medium: "", speed: "", er: printfG(w.permittivity), mur: printfG(w.permeability) };
    return updated(t, w.frequency, w.permittivity, w.permeability, w, null);
  });

  const save = (patch: Partial<typeof s>) => updatePcbCalculatorSettings((c) => void Object.assign(c.wavelength, patch));

  // `OnFrequencyChange` ... `OnWavelengthMediumChange`.
  const typed = (box: Box, text: string) => {
    let next = { ...texts, [box]: text };
    const v = toDouble(text);
    if (v !== null && v > 0) {
      let freq: number;
      if (box === "frequency") freq = v * unitScale(UNIT_SELECTOR_FREQUENCY, s.frequencyUnit);
      else if (box === "period") freq = frequencyFromPeriod(v * unitScale(UNIT_SELECTOR_TIME, s.periodUnit));
      else if (box === "vacuum") freq = frequencyFromWavelengthVacuum(v * unitScale(UNIT_SELECTOR_LEN_CABLE, s.wavelengthVacuumUnit));
      else freq = frequencyFromWavelengthMedium(v * unitScale(UNIT_SELECTOR_LEN_CABLE, s.wavelengthMediumUnit), s.permittivity, s.permeability);
      next = updated(next, freq, s.permittivity, s.permeability, s, box);
      save({ frequency: freq });
    }
    setTexts(next);
  };

  // `OnPermittivityChange` / `OnPermeabilityChange`: taken when >= 1.
  const material = (which: "er" | "mur", text: string) => {
    let next = { ...texts, [which]: text };
    const v = toDouble(text);
    if (v !== null && v >= 1) {
      const er = which === "er" ? v : s.permittivity;
      const mur = which === "mur" ? v : s.permeability;
      next = updated(next, s.frequency, er, mur, s, null);
      save(which === "er" ? { permittivity: v } : { permeability: v });
    }
    setTexts(next);
  };

  // `updateUnits`.
  const unit = (key: keyof Units, index: number) => {
    const u = { ...s, [key]: index };
    setTexts(updated(texts, s.frequency, s.permittivity, s.permeability, u, null));
    save({ [key]: index });
  };

  // `OnButtonPermittivity`: the list with an empty line for no selection, opened on the value in the box.
  const pickEr = async () => {
    const list = [...standardRelativeDielectricConstantList(), ""];
    const [, prev] = findMatch(list, texts.er);
    const index = await ask("Relative Dielectric Constants", list, prev);
    if (index >= 0 && list[index] !== "") material("er", entryValue(list[index]!));
  };

  return (
    <CalcPanel testid="calc-panel-wavelength">
      <CalcGrid
        rows={[
          ["Frequency:", <CalcInput testid="wl-frequency" label="Frequency" value={texts.frequency} onChange={(v) => typed("frequency", v)} />, <UnitChoice testid="wl-frequency-unit" label="Frequency unit" def={UNIT_SELECTOR_FREQUENCY} value={s.frequencyUnit} onChange={(i) => unit("frequencyUnit", i)} />],
          ["Period:", <CalcInput testid="wl-period" label="Period" value={texts.period} onChange={(v) => typed("period", v)} />, <UnitChoice testid="wl-period-unit" label="Period unit" def={UNIT_SELECTOR_TIME} value={s.periodUnit} onChange={(i) => unit("periodUnit", i)} />],
          ["Wavelength in vacuum:", <CalcInput testid="wl-vacuum" label="Wavelength in vacuum" value={texts.vacuum} onChange={(v) => typed("vacuum", v)} />, <UnitChoice testid="wl-vacuum-unit" label="Wavelength in vacuum unit" def={UNIT_SELECTOR_LEN_CABLE} value={s.wavelengthVacuumUnit} onChange={(i) => unit("wavelengthVacuumUnit", i)} />],
          ["Wavelength in medium:", <CalcInput testid="wl-medium" label="Wavelength in medium" value={texts.medium} onChange={(v) => typed("medium", v)} />, <UnitChoice testid="wl-medium-unit" label="Wavelength in medium unit" def={UNIT_SELECTOR_LEN_CABLE} value={s.wavelengthMediumUnit} onChange={(i) => unit("wavelengthMediumUnit", i)} />],
          ["Speed in medium:", <CalcResult testid="wl-speed" label="Speed in medium" value={texts.speed} width={140} />, <UnitChoice testid="wl-speed-unit" label="Speed unit" def={UNIT_SELECTOR_SPEED} value={s.speedUnit} onChange={(i) => unit("speedUnit", i)} />],
          [
            <span title="relative permittivity (dielectric constant)">er:</span>,
            <CalcInput testid="wl-er" label="er" value={texts.er} onChange={(v) => material("er", v)} />,
            <button type="button" data-testid="wl-er-list" onClick={() => void pickEr()}>
              ...
            </button>,
          ],
          [<span title="relative permeability">mur:</span>, <CalcInput testid="wl-mur" label="mur" value={texts.mur} onChange={(v) => material("mur", v)} />, null],
        ]}
      />
      {dialog}
    </CalcPanel>
  );
}
