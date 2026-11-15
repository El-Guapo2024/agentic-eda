// The lists of material constants the Calculator Tools offer behind their "..." buttons (pcb_calculator/common_data.cpp, KiCad 25ec2335) and
// `findMatch` (pcb_calculator_utils.cpp), which finds the entry a text box already holds so the list opens on it. Each entry is "<value> \t<name>";
// the panels take the text before the first space (`BeforeFirst( ' ' )`) as the value.
//
// Pure: no React, no DOM.

/** `StandardRelativeDielectricConstantList`. */
export function standardRelativeDielectricConstantList(): string[] {
  return [
    "4.5 \tFR4",
    "3.67 \tIsola FR408",
    "4.04 \tIsola 370HR",
    "3.55 \tRogers RO4003C",
    "3.66 \tRogers R4350B",
    "9.8 \talumina (Al2O3)",
    "3.78 \tfused quartz",
    "3.38 \tRO4003",
    "2.2 \tRT/duroid 5880",
    "10.2 \tRT/duroid 6010LM",
    "2.1 \tteflon (PTFE)",
    "4.0 \tPVC",
    "2.3 \tPE",
    "6.6 \tberyllia (BeO)",
    "8.7 \taluminum nitride",
    "11.9 \tsilicon",
    "12.9 \tGaAs",
  ];
}

/** `StandardLossTangentList`. */
export function standardLossTangentList(): string[] {
  return [
    "0.02 \tFR4 @ 1GHz",
    "0.012 \tIsola FR408 @ 2 GHz",
    "0.021 \tIsola 370HR @ 2 GHz",
    "0.0027 \tRogers RO4003C @ 10 GHz",
    "0.0021 \tRogers RO4003C @ 2.5 GHz",
    "0.0037 \tRogers RO4350B @ 10 GHz",
    "0.0031 \tRogers RO4350B @ 2.5 GHz",
    "3e-4 \tberyllia @ 10GHz",
    "2e-4 \taluminia (Al2O3) @ 10GHz",
    "1e-4 \tfused quartz @ 10GHz",
    "0.002 \tRO4003 @ 10GHz",
    "9e-4 \tRT/duroid 5880 @ 10GHz",
    "2e-4 \tteflon (PTFE) @ 1MHz",
    "0.05 \tPVC @ 1MHz",
    "2e-4 \tPE @ 1MHz",
    "0.001 \taluminum nitride @ 10GHz",
    "0.015 \tsilicon @ 10GHz",
    "0.002 \tGaAs @ 10GHz",
  ];
}

/** `StandardResistivityList`: specific resistance in ohms * meters (rho). */
export function standardResistivityList(): string[] {
  return [
    "2.4e-8 \tgold",
    "1.72e-8 \tcopper",
    "1.62e-8 \tsilver",
    "12.4e-8 \ttin",
    "10.5e-8 \tplatinum",
    "2.62e-8 \taluminum",
    "6.9e-8 \tnickel",
    "3.9e-8 \tbrass (66Cu 34Zn)",
    "9.71e-8 \tiron",
    "6.0e-8 \tzinc",
  ];
}

/** `StandardCableConductorList` (lined up with `StandardCableTempCoefList`). */
export function standardCableConductorList(): string[] {
  return ["1.72e-8 \tCu, Copper", "2.62e-8 \tAl, Aluminum", "100e-8 \tNiCr, Nichrome", "9.71e-8 \tFe, Iron", "5.6e-8 \tW, Tungsten"];
}

/** `StandardCableTempCoefList`: temperature coefficient at 20 C. */
export function standardCableTempCoefList(): string[] {
  return ["3.93e-3 \tCu, Copper", "4.29e-3 \tAl, Aluminum", "0.4e-3 \tNiCr, Nichrome", "5e-3 \tFe, Iron", "4.5e-3 \tW, Tungsten"];
}

/** `wxString::BeforeFirst( ' ' )`: an entry's value. */
export function entryValue(entry: string): string {
  const i = entry.indexOf(" ");
  return i < 0 ? entry : entry.slice(0, i);
}

/**
 * `findMatch( aList, aValue, aIdx )`: the index of the entry whose value is the text (compared as strings with ',' read as '.', then as numbers),
 * stopping at the first empty entry (the "no selection" line callers append). Returns [found, index]; when nothing matches, the index is where the
 * search stopped (the empty line, or the list's length).
 */
export function findMatch(list: readonly string[], value: string): [boolean, number] {
  const cvalue = value.replace(/,/g, ".");
  let idx = 0;
  for (const text of list) {
    if (text === "") break;
    if (entryValue(text).replace(/,/g, ".") === cvalue) return [true, idx];
    idx++;
  }
  // `cvalue.ToDouble( &curr_value )`: a text that is not a number leaves curr_value uninitialised in KiCad; here it matches nothing.
  const t = cvalue.trim();
  const cur = t !== "" && /^[+-]?(\d+\.?\d*|\.\d+)([eE][+-]?\d+)?$/.test(t) ? Number(t) : Number.NaN;
  idx = 0;
  for (const text of list) {
    if (text === "") break;
    const vs = entryValue(text).trim();
    const val = /^[+-]?(\d+\.?\d*|\.\d+)([eE][+-]?\d+)?$/.test(vs) ? Number(vs) : Number.NaN;
    if (cur === val) return [true, idx];
    idx++;
  }
  return [false, idx];
}
