// The Board Classes memo (pcb_calculator/calculator_panels/panel_board_class.cpp at KiCad 25ec2335): the minimum line width, clearance and annular
// differences of the six board classes, shown in the length unit chosen (`BoardClassesUpdateData`). The UI is components/calculator/PanelBoardClass.tsx.
//
// Pure: no React, no DOM.
import { printfG as formatG } from "./cformat";
import { UNIT_MM } from "./unitSelectors";

/** `BOARD_MIN_SIZE_VALUES`, metres; -1.0 where a value is irrelevant in a class. */
export interface BoardMinSizeValues {
  lines: number;
  clearance: number;
  viaDiamDiff: number;
  padDiamDiffPlated: number;
  padDiamDiffNotPlated: number;
}

const v = (lines: number, clearance: number, via: number, padPlated: number, padNotPlated: number): BoardMinSizeValues => ({
  lines,
  clearance,
  viaDiamDiff: via,
  padDiamDiffPlated: padPlated,
  padDiamDiffNotPlated: padNotPlated,
});

/** `clist` (`BRDCLASS_COUNT` = 6). */
export const BOARD_CLASSES: readonly BoardMinSizeValues[] = [
  v(0.8 * UNIT_MM, 0.68 * UNIT_MM, -1.0, 1.19 * UNIT_MM, 1.57 * UNIT_MM),
  v(0.5 * UNIT_MM, 0.5 * UNIT_MM, -1.0, 0.78 * UNIT_MM, 1.13 * UNIT_MM),
  v(0.31 * UNIT_MM, 0.31 * UNIT_MM, 0.45 * UNIT_MM, 0.6 * UNIT_MM, 0.9 * UNIT_MM),
  v(0.21 * UNIT_MM, 0.21 * UNIT_MM, 0.34 * UNIT_MM, 0.49 * UNIT_MM, -1.0),
  v(0.15 * UNIT_MM, 0.15 * UNIT_MM, 0.24 * UNIT_MM, 0.39 * UNIT_MM, -1.0),
  v(0.12 * UNIT_MM, 0.12 * UNIT_MM, 0.2 * UNIT_MM, 0.35 * UNIT_MM, -1.0),
];

/** The grid's labels. */
export const BOARD_CLASS_COLUMNS = ["Class 1", "Class 2", "Class 3", "Class 4", "Class 5", "Class 6"] as const;
export const BOARD_CLASS_ROWS = ["Lines width", "Minimum clearance", "Via: (diameter - drill)", "Plated Pad: (diameter - drill)", "NP Pad: (diameter - drill)"] as const;

/** `BoardClassesUpdateData`: 5 rows x 6 classes, each `%g` of the value over the unit's scale, "--" where it does not apply. */
export function boardClassCells(unitScale: number): string[][] {
  const cell = (x: number) => (x > -1.0 ? formatG(x / unitScale) : "--");
  return [
    BOARD_CLASSES.map((c) => cell(c.lines)),
    BOARD_CLASSES.map((c) => cell(c.clearance)),
    BOARD_CLASSES.map((c) => cell(c.viaDiamDiff)),
    BOARD_CLASSES.map((c) => cell(c.padDiamDiffPlated)),
    BOARD_CLASSES.map((c) => cell(c.padDiamDiffNotPlated)),
  ];
}
