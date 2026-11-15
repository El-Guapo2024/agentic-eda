// The Board Classes memo (`PANEL_BOARD_CLASS`, pcb_calculator/calculator_panels/panel_board_class.cpp and its _base at KiCad 25ec2335): a length unit
// chooser at the left, "Note: Values are minimal values" and the 5 x 6 grid of minimum sizes per class at the right (kicad-port/calc/boardClass.ts).
// The unit chosen is the `board_class_units` setting.
import { useState } from "react";
import { getPcbCalculator, updatePcbCalculatorSettings } from "../../state/pcbCalculator";
import { UNIT_SELECTOR_LEN, unitScale } from "../../kicad-port/calc/unitSelectors";
import { BOARD_CLASS_COLUMNS, BOARD_CLASS_ROWS, boardClassCells } from "../../kicad-port/calc/boardClass";
import { CalcPanel, UnitChoice } from "./calcKit";

export function PanelBoardClass() {
  // LoadSettings: the unit, then `BoardClassesUpdateData`.
  const [units, setUnits] = useState(() => getPcbCalculator().settings.board_class_units);
  const cells = boardClassCells(unitScale(UNIT_SELECTOR_LEN, units));
  return (
    <CalcPanel testid="calc-panel-board_class">
      <div style={{ display: "flex", gap: 10, alignItems: "flex-start" }}>
        <div style={{ margin: "32px 32px 32px 10px" }}>
          <UnitChoice
            def={UNIT_SELECTOR_LEN}
            value={units}
            testid="calc-bc-units"
            label="Units"
            onChange={(u) => {
              setUnits(u);
              updatePcbCalculatorSettings((s) => void (s.board_class_units = u));
            }}
          />
        </div>
        <div style={{ display: "flex", flexDirection: "column", gap: 6 }}>
          <div style={{ textAlign: "center", fontStyle: "italic", fontWeight: 700 }}>Note: Values are minimal values</div>
          <table className="calc-table" data-testid="calc-bc-grid">
            <thead>
              <tr>
                <th />
                {BOARD_CLASS_COLUMNS.map((c) => (
                  <th key={c} style={{ minWidth: 70 }}>
                    {c}
                  </th>
                ))}
              </tr>
            </thead>
            <tbody>
              {cells.map((row, i) => (
                <tr key={i}>
                  <th style={{ textAlign: "left" }}>{BOARD_CLASS_ROWS[i]}</th>
                  {row.map((c, j) => (
                    <td key={j} data-testid={`calc-bc-cell-${i}-${j}`}>
                      {c}
                    </td>
                  ))}
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      </div>
    </CalcPanel>
  );
}
