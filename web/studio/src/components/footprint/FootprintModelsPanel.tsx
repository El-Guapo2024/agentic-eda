// The 3D Models tab of the Footprint Properties dialogs (`PANEL_FP_PROPERTIES_3D_MODEL`, pcbnew/dialogs/panel_fp_properties_3d_model.cpp; the board's dialog is
// components/FootprintPropertiesDialog.tsx, the Footprint Editor's components/footprint/FootprintPropertiesDialog.tsx): the footprint's models as a grid -- a status cell, the file
// name, Show -- with Add (a blank row), Browse (the model chooser), Remove and Configure Paths, and beside the grid what `PANEL_PREVIEW_3D_MODEL` has for the selected row: the Scale,
// Rotation and Offset fields with their spin buttons and wheel, the Opacity slider, and the 3D view of the footprint with the models on it.
//
// The list is the dialog's working copy (`m_shapes3D_list`): nothing is sent until its OK (kicad-port/fpModels.ts `boardModelsCmds`, or the Footprint Editor's whole panel).
// The rows are the file's numbers; the rotation fields show them the other way round, as the dialog does. The status cell is the server's (`POST /api/3dmodel/check`).
//
// Not ported: the "Extruded 3D Body" box of this tab (`EXTRUDED_3D_BODY`: a body made from the courtyard or fab outline, its height and standoff, a material and a colour, and its export) and
// the embedded files (`PANEL_EMBEDDED_FILES`): GAPS.md item 30.
import { useEffect, useMemo, useRef, useState } from "react";
import { useNonPassiveWheel } from "../../hooks/useNonPassiveWheel";
import type { Part } from "../../api/types";
import { checkModels, fetchPreviewPart, type PreviewOf } from "../../api/model3dClient";
import {
  cleanPath,
  formatOffset,
  formatRotation,
  formatScale,
  isNumberText,
  movedRows,
  opacityOf,
  plainRow,
  readNumber,
  readOffset,
  readRotation,
  sliderOf,
  statusIcon,
  stepOffset,
  stepRotation,
  stepScale,
  type ModelCheck,
  type ModelRow,
  type Vec3,
} from "../../kicad-port/fpModels";
import { openConfigurePaths } from "../../state/libraryTableDialogs";
import type { LengthUnit } from "../../state/units";
import { Model3dChooserDialog } from "./Model3dChooserDialog";
import { ModelPreview3D } from "./ModelPreview3D";

export interface FootprintModelsPanelProps {
  rows: ModelRow[];
  onRows: (rows: ModelRow[]) => void;
  /** The footprint the preview shows under the models. */
  previewOf: PreviewOf | null;
  units: LengthUnit;
}

const AXES = ["X", "Y", "Z"] as const;

/** One of the nine fields: the text is the draft while it is typed in and the formatted value otherwise; a change is committed as soon as the text is a number (`wxEVT_COMMAND_TEXT_UPDATED`). */
function Field({ shown, onText, onStep, testId, disabled }: { shown: string; onText: (text: string) => void; onStep: (sign: 1 | -1, fine: boolean) => void; testId: string; disabled: boolean }) {
  const [draft, setDraft] = useState<string | null>(null);
  const input = useRef<HTMLInputElement>(null);
  // `onMouseWheelScale` and its kin: a wheel turned over the field steps it -- up lowers the value, down raises it (`if( event.GetWheelRotation() >= 0 ) step = -step`) -- and the
  // dialog does not scroll under it; a field that does not have the focus leaves the wheel to the dialog.
  useNonPassiveWheel(input, (e) => {
    if (document.activeElement !== input.current) return;
    e.preventDefault();
    onStep(e.deltaY < 0 ? -1 : 1, e.shiftKey);
    setDraft(null);
  });
  return (
    <span style={{ display: "inline-flex", alignItems: "center" }}>
      <input
        ref={input}
        value={disabled ? "" : (draft ?? shown)}
        disabled={disabled}
        style={{ width: 92 }}
        data-testid={testId}
        onFocus={(e) => e.currentTarget.select()}
        onChange={(e) => {
          setDraft(e.target.value);
          onText(e.target.value);
        }}
        onBlur={() => setDraft(null)}
        onKeyDown={(e) => {
          if (e.key === "Enter") {
            setDraft(null);
          } else if (e.key === "ArrowUp" || e.key === "ArrowDown") {
            e.preventDefault();
            onStep(e.key === "ArrowUp" ? 1 : -1, e.shiftKey);
            setDraft(null);
          }
        }}
      />
      <span style={{ display: "inline-flex", flexDirection: "column", marginLeft: 1 }}>
        <button style={{ padding: 0, lineHeight: "9px", fontSize: 8, height: 11 }} disabled={disabled} tabIndex={-1} title="Increase (Shift for a fine step)" onClick={(e) => onStep(1, e.shiftKey)} data-testid={`${testId}-up`}>
          +
        </button>
        <button style={{ padding: 0, lineHeight: "9px", fontSize: 8, height: 11 }} disabled={disabled} tabIndex={-1} title="Decrease (Shift for a fine step)" onClick={(e) => onStep(-1, e.shiftKey)} data-testid={`${testId}-down`}>
          -
        </button>
      </span>
    </span>
  );
}

const withAxis = (v: Vec3, axis: number, value: number): Vec3 => v.map((x, i) => (i === axis ? value : x)) as Vec3;

export function FootprintModelsPanel({ rows, onRows, previewOf, units }: FootprintModelsPanelProps) {
  const [selected, setSelected] = useState(0);
  const [checks, setChecks] = useState<Record<string, ModelCheck>>({});
  const [chooser, setChooser] = useState(false);
  const [part, setPart] = useState<Part | null>(null);
  const rowsRef = useRef(rows);
  rowsRef.current = rows;
  const sel = Math.min(Math.max(selected, 0), rows.length - 1);
  const row: ModelRow | undefined = rows[sel];

  // The footprint to draw under the models: its own frame, top side, not turned.
  const previewKey = previewOf ? JSON.stringify(previewOf) : "";
  useEffect(() => {
    let cancelled = false;
    setPart(null);
    if (previewOf) void fetchPreviewPart(previewOf).then((p) => !cancelled && setPart(p));
    return () => {
      cancelled = true;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [previewKey]);

  // The status of each file name, asked once typing has stopped for a moment.
  const names = rows.map((r) => r.path).join("\n");
  useEffect(() => {
    const wanted = [...new Set(names.split("\n"))];
    const timer = setTimeout(() => {
      void checkModels(wanted).then((results) => {
        const next: Record<string, ModelCheck> = {};
        results.forEach((r, i) => (next[wanted[i] ?? r.name] = r));
        setChecks(next);
      });
    }, 200);
    return () => clearTimeout(timer);
  }, [names]);

  const update = (i: number, patch: Partial<ModelRow>) => onRows(rows.map((r, j) => (j === i ? { ...r, ...patch } : r)));
  const add = (path: string) => {
    onRows([...rows, plainRow(path)]);
    setSelected(rows.length);
  };
  const remove = () => {
    if (rows.length === 0) return;
    onRows(rows.filter((_, j) => j !== sel));
    setSelected(Math.max(0, Math.min(sel, rows.length - 2)));
  };
  const move = (to: number) => {
    if (to < 0 || to >= rows.length) return;
    onRows(movedRows(rows, sel, to));
    setSelected(to);
  };

  const scaleField = (axis: number) => (
    <Field
      key={`s${axis}`}
      testId={`model3d-scale-${AXES[axis]!.toLowerCase()}`}
      disabled={!row}
      shown={row ? formatScale(row.scale[axis]!) : ""}
      onText={(t) => row && isNumberText(t) && update(sel, { scale: withAxis(row.scale, axis, readNumber(t, row.scale[axis]!)) })}
      onStep={(sign, fine) => row && update(sel, { scale: withAxis(row.scale, axis, stepScale(row.scale[axis]!, sign, fine)) })}
    />
  );
  // the rotation is stored clockwise and shown counter-clockwise
  const rotationField = (axis: number) => (
    <Field
      key={`r${axis}`}
      testId={`model3d-rotation-${AXES[axis]!.toLowerCase()}`}
      disabled={!row}
      shown={row ? formatRotation(-row.rotate[axis]!) : ""}
      onText={(t) => row && isNumberText(t) && update(sel, { rotate: withAxis(row.rotate, axis, readRotation(t)) })}
      onStep={(sign, fine) => row && update(sel, { rotate: withAxis(row.rotate, axis, -stepRotation(-row.rotate[axis]!, sign, fine) || 0) })}
    />
  );
  const offsetField = (axis: number) => (
    <Field
      key={`o${axis}`}
      testId={`model3d-offset-${AXES[axis]!.toLowerCase()}`}
      disabled={!row}
      shown={row ? formatOffset(row.offset[axis]!, units) : ""}
      onText={(t) => row && isNumberText(t) && update(sel, { offset: withAxis(row.offset, axis, readOffset(t, units)) })}
      onStep={(sign, fine) => row && update(sel, { offset: withAxis(row.offset, axis, stepOffset(row.offset[axis]!, sign, fine, units)) })}
    />
  );

  const box = (title: string, field: (axis: number) => React.ReactNode) => (
    <fieldset style={{ margin: 0, padding: "2px 6px 4px" }}>
      <legend style={{ fontSize: 11, padding: "0 3px" }}>{title}</legend>
      {[0, 1, 2].map((axis) => (
        <div key={axis} style={{ display: "flex", alignItems: "center", gap: 4, marginTop: 2 }}>
          <span style={{ width: 14, fontSize: 11 }}>{AXES[axis]}:</span>
          {field(axis)}
        </div>
      ))}
    </fieldset>
  );

  const shownRows = useMemo(() => rows.map((r) => ({ ...r, path: cleanPath(r.path) })), [rows]);

  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 8 }} data-testid="footprint-models">
      <div style={{ border: "1px solid var(--chrome-border, #444)", maxHeight: 130, overflowY: "auto" }}>
        <table style={{ borderCollapse: "collapse", width: "100%", fontSize: 12 }} data-testid="model3d-grid">
          <thead>
            <tr style={{ textAlign: "left" }}>
              <th style={{ width: 22 }} />
              <th>3D Model(s)</th>
              <th style={{ width: 50 }}>Show</th>
            </tr>
          </thead>
          <tbody>
            {rows.map((r, i) => {
              const check = checks[r.path] ?? checks[cleanPath(r.path)];
              const icon = statusIcon(r.path.trim() === "" ? "empty" : check?.status);
              const why = r.path.trim() === "" ? "No filename entered" : (check?.message ?? "");
              return (
                <tr key={i} onClick={() => setSelected(i)} style={{ background: i === sel ? "var(--chrome-hover, rgba(128,128,128,.25))" : undefined }} data-testid={`model3d-row-${i}`}>
                  <td style={{ textAlign: "center" }}>
                    {icon && (
                      <span
                        title={why}
                        data-testid={`model3d-status-${i}`}
                        data-status={icon}
                        style={{ display: "inline-block", width: 14, height: 14, lineHeight: "14px", borderRadius: 7, fontSize: 10, fontWeight: 700, color: "#fff", background: icon === "warning" ? "#d4a017" : "#c0392b" }}
                      >
                        {icon === "warning" ? "!" : "x"}
                      </span>
                    )}
                  </td>
                  <td>
                    <input
                      value={r.path}
                      placeholder="${KICAD10_3DMODEL_DIR}/..."
                      style={{ width: "100%", boxSizing: "border-box" }}
                      onFocus={() => setSelected(i)}
                      onChange={(e) => update(i, { path: e.target.value })}
                      onBlur={(e) => e.target.value !== cleanPath(e.target.value) && update(i, { path: cleanPath(e.target.value) })}
                      data-testid={`model3d-path-${i}`}
                    />
                  </td>
                  <td style={{ textAlign: "center" }}>
                    <input type="checkbox" checked={r.show} onChange={(e) => update(i, { show: e.target.checked })} data-testid={`model3d-show-${i}`} />
                  </td>
                </tr>
              );
            })}
            {rows.length === 0 && (
              <tr>
                <td />
                <td colSpan={2} style={{ color: "var(--chrome-text-dim)", padding: "4px 2px" }}>
                  No 3D model.
                </td>
              </tr>
            )}
          </tbody>
        </table>
      </div>
      <div style={{ display: "flex", gap: 4, alignItems: "center" }}>
        <button title="Add 3D model" onClick={() => add("")} data-testid="model3d-add">
          Add
        </button>
        <button title="Browse for a 3D model" onClick={() => setChooser(true)} data-testid="model3d-browse">
          Browse...
        </button>
        <button title="Remove 3D model" disabled={rows.length === 0} onClick={remove} data-testid="model3d-remove">
          Remove
        </button>
        <span style={{ width: 12 }} />
        <button title="Move the model up" disabled={rows.length === 0 || sel === 0} onClick={() => move(sel - 1)} data-testid="model3d-up">
          Up
        </button>
        <button title="Move the model down" disabled={rows.length === 0 || sel >= rows.length - 1} onClick={() => move(sel + 1)} data-testid="model3d-down">
          Down
        </button>
        <button style={{ marginLeft: "auto" }} onClick={() => openConfigurePaths()} data-testid="model3d-paths">
          Configure Paths...
        </button>
      </div>
      <div style={{ display: "grid", gridTemplateColumns: "190px minmax(0, 1fr)", gap: 10 }}>
        <div style={{ display: "flex", flexDirection: "column", gap: 4 }}>
          {box("Scale", scaleField)}
          {box("Rotation", rotationField)}
          {box("Offset", offsetField)}
          <fieldset style={{ margin: 0, padding: "2px 6px 4px" }}>
            <legend style={{ fontSize: 11, padding: "0 3px" }}>Opacity</legend>
            <input
              type="range"
              min={0}
              max={100}
              disabled={!row}
              value={row ? sliderOf(row.opacity) : 100}
              onChange={(e) => update(sel, { opacity: opacityOf(Number(e.target.value)) })}
              style={{ width: "100%" }}
              data-testid="model3d-opacity"
            />
          </fieldset>
        </div>
        <ModelPreview3D part={part} rows={shownRows} height={290} />
      </div>
      {chooser && (
        <Model3dChooserDialog
          onCancel={() => setChooser(false)}
          onConfigurePaths={() => openConfigurePaths()}
          onChoose={(path) => {
            setChooser(false);
            // `OnAdd3DModel`: a model chosen starts with KiCad's defaults and is shown
            add(path);
          }}
        />
      )}
    </div>
  );
}
