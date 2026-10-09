// pcbnew/dialogs/dialog_footprint_properties.cpp -- "Footprint Properties" of a footprint on the board (`pcbnew.InteractiveEdit.properties` on a
// footprint, a double-click, or on one of its fields). The field grid (`PCB_FIELDS_GRID_TABLE`: name, value, show, width, height, thickness, italic, layer,
// orientation, keep upright, X and Y offset, knockout, mirrored, and the justification and bold the Properties panel has), Add and Delete for the user fields, the
// general part (position, orientation, board side, locked), the component type and the attributes (board only, exclude from position files and from the BOM,
// do not populate, exempt from the courtyard requirement).
//
// OK sends ONE batch, which is `TransferDataFromWindow`'s one `BOARD_COMMIT`: the fields and the attributes first (`edit_board_footprint`), then the
// position, the orientation, the side and the lock through the verbs the move, rotate, flip and lock tools use -- so a side change flips the layers and the
// mirroring the grid just set, as `FOOTPRINT::Flip` does.
//
// Not ported: the local clearance and mask/paste margins, the zone connection (the zone fill's), the 3D models and embedded files, jumper pad groups, Update
// and Change Footprint (they need the footprint chooser). The Reference and Value texts come from the schematic and the intent: they are shown, not edited.
import { useEffect, useMemo, useState } from "react";
import type { Cmd, FieldInfo, FieldLayoutCmd, FootprintAttrsCmd, Part } from "../api/types";
import { checkFieldRow, fieldsOf, layoutOf, localAngleOf, newFieldLayout, newFieldName, parseFieldId, attrsOf, REFERENCE, VALUE } from "../kicad-port/fpFields";
import { TECH_LAYERS } from "../kicad-port/pcbProperties";
import { useStudioApi, useStudioDispatch, useStudioState } from "../state/store";
import { formatLength, umFrom, umTo } from "../state/units";

/** One row of the grid. `from` is the field it started as (null for a row added here). */
interface Row {
  key: number;
  from: string | null;
  name: string;
  text: string;
  layout: FieldLayoutCmd;
}

const norm360 = (a: number): number => ((a % 360) + 360) % 360;

export function FootprintPropertiesDialog() {
  const state = useStudioState();
  const dispatch = useStudioDispatch();
  const api = useStudioApi();
  const open = state.footprintPropertiesOpen;
  const board = state.board;
  const units = state.units;

  // The footprint the selection stands for: the footprint itself, or the one a selected pad or field belongs to (`GetParentFootprint`).
  const first = [...state.selection][0];
  const ref = first ? (parseFieldId(first)?.ref ?? first.split(".")[0]!) : undefined;
  const part: Part | undefined = ref && board ? board.parts.find((p) => p.placed && p.ref === ref) : undefined;

  const [rows, setRows] = useState<Row[]>([]);
  const [attrs, setAttrs] = useState<FootprintAttrsCmd | null>(null);
  const [x, setX] = useState(0);
  const [y, setY] = useState(0);
  const [orientation, setOrientation] = useState(0);
  const [bottom, setBottom] = useState(false);
  const [locked, setLocked] = useState(false);
  const [selected, setSelected] = useState(0);
  const [more, setMore] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  // Fill the form when the dialog opens on a footprint -- not on every poll, or a keystroke would be lost to the next refresh.
  useEffect(() => {
    if (!open || !part) return;
    setRows(fieldsOf(part).map((f, i) => ({ key: i, from: f.name, name: f.name, text: f.text, layout: layoutOf(f) })));
    setAttrs(attrsOf(part));
    setX(part.at?.[0] ?? 0);
    setY(part.at?.[1] ?? 0);
    // KiCad's orientation is counter-clockwise; the model's `rot` runs clockwise.
    setOrientation(norm360(-(part.rot ?? 0)));
    setBottom(part.side === "bottom");
    setLocked((board?.locked ?? []).includes(part.ref));
    setSelected(0);
    setError(null);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open, part?.ref]);

  const layers = useMemo(() => [...(board?.layers ?? []), ...TECH_LAYERS], [board?.layers]);
  if (!open || !part || !attrs) return null;
  const close = () => dispatch({ type: "SET_FOOTPRINT_PROPERTIES_OPEN", open: false });

  const infos = new Map<string, FieldInfo>(fieldsOf(part).map((f) => [f.name, f]));
  const nets = [...new Set((part.pads ?? []).map((q) => q.net).filter((n): n is string => !!n))];
  const len = (um: number) => formatLength(um, units);

  const setRow = (i: number, patch: Partial<Row>) => setRows((rs) => rs.map((r, j) => (j === i ? { ...r, ...patch } : r)));
  const setLayout = (i: number, patch: Partial<FieldLayoutCmd>) => setRows((rs) => rs.map((r, j) => (j === i ? { ...r, layout: { ...r.layout, ...patch } } : r)));
  const num = (v: string, fallback: number): number => (Number.isFinite(Number(v)) && v.trim() !== "" ? Number(v) : fallback);

  /** A length cell: the value in the current unit, written back in um. */
  const lengthCell = (um: number, onChange: (um: number) => void, width = 62) => (
    <input type="number" step="any" value={Number(umTo(um, units).toFixed(4))} onChange={(e) => onChange(Math.round(umFrom(num(e.target.value, umTo(um, units)), units)))} style={{ width }} />
  );

  // A bottom-side footprint's frame is the top-side one mirrored: the dialog shows the offset as the file stores it, x un-mirrored (`GetFPRelativePosition`).
  const sense = part.side === "bottom" ? -1 : 1;

  const nameClash = (name: string, i: number): boolean => rows.some((r, j) => j !== i && r.name.trim() === name.trim());

  const submit = async () => {
    setError(null);
    for (let i = 0; i < rows.length; i++) {
      const r = rows[i]!;
      const message = checkFieldRow(r.name, { size: r.layout.size, thickness: r.layout.thickness }, len);
      if (message) return setError(`${r.name || `Field ${i + 1}`}: ${message}`);
      if (i >= 2 && (r.name === REFERENCE || r.name === VALUE || r.name === "Datasheet" || r.name === "Description")) return setError(`"${r.name}" is the name of a mandatory field.`);
      if (nameClash(r.name, i)) return setError(`There are two fields called "${r.name}".`);
    }
    const [ref0, val0, ...users] = rows;
    const original = (name: string): FieldInfo | undefined => infos.get(name);
    const same = (a: unknown, b: unknown): boolean => JSON.stringify(a) === JSON.stringify(b);

    const cmds: Cmd[] = [];
    const edit: Extract<Cmd, { op: "edit_board_footprint" }> = { op: "edit_board_footprint", part: part.ref };
    if (ref0 && !same(ref0.layout, layoutOf(original(REFERENCE)!))) edit.reference = ref0.layout;
    if (val0 && !same(val0.layout, layoutOf(original(VALUE)!))) edit.value = val0.layout;
    const userFields = users.map((r) => ({ name: r.name.trim(), text: r.text, layout: r.layout }));
    const before = fieldsOf(part)
      .filter((f) => f.name !== REFERENCE && f.name !== VALUE)
      .map((f) => ({ name: f.name, text: f.text, layout: layoutOf(f) }));
    if (!same(userFields, before)) edit.fields = userFields;
    if (!same(attrs, attrsOf(part))) edit.attrs = attrs;
    if (edit.reference || edit.value || edit.fields || edit.attrs) cmds.push(edit);

    // The pose, as the move, rotate, flip and lock tools do it (the footprint's own position and side; the fields follow).
    const at = part.at ?? [0, 0];
    const [dx, dy] = [Math.round(x - at[0]), Math.round(y - at[1])];
    if (dx !== 0 || dy !== 0) cmds.push({ op: "move_items", ids: [part.ref], dx, dy });
    const turn = norm360(orientation) - norm360(-(part.rot ?? 0));
    if (Math.abs(turn) > 1e-9) {
      // `rotate_items` turns clockwise; KiCad's orientation is the other way.
      const pivot = { x: Math.round(x), y: Math.round(y) };
      cmds.push({ op: "rotate_items", ids: [part.ref], pivot, angle_millideg: Math.round(-turn * 1000) });
    }
    if (bottom !== (part.side === "bottom")) cmds.push({ op: "flip_items", ids: [part.ref], pivot: { x: Math.round(x), y: Math.round(y) }, direction: "left_right" });
    if (locked !== (board?.locked ?? []).includes(part.ref)) cmds.push({ op: "set_locked", ids: [part.ref], locked });
    if (cmds.length === 0) return close();
    setBusy(true);
    try {
      if (await api.cmdBatch(cmds)) close();
    } finally {
      setBusy(false);
    }
  };

  const attr = <K extends keyof FootprintAttrsCmd>(key: K, value: FootprintAttrsCmd[K]) => setAttrs((a) => (a ? { ...a, [key]: value } : a));
  const check = (label: string, key: "board_only" | "exclude_from_pos_files" | "exclude_from_bom" | "dnp" | "allow_missing_courtyard") => (
    <label style={{ display: "block" }}>
      <input type="checkbox" checked={attrs[key]} onChange={(e) => attr(key, e.target.checked)} /> {label}
    </label>
  );

  const cell = { padding: "1px 3px" } as const;
  return (
    <div className="dialog-backdrop" onClick={close}>
      <div className="dialog" style={{ width: more ? 1100 : 760, maxWidth: "96vw" }} onClick={(e) => e.stopPropagation()}>
        <div className="dialog-header">
          <span>Footprint Properties</span>
          <span>{part.ref}</span>
        </div>
        <div className="dialog-body" style={{ maxHeight: "74vh", overflowY: "auto" }} data-testid="footprint-properties">
          <div style={{ overflowX: "auto" }}>
            <table style={{ borderCollapse: "collapse", fontSize: 12 }} data-testid="footprint-fields-grid">
              <thead>
                <tr style={{ textAlign: "left" }}>
                  <th style={cell}>Name</th>
                  <th style={cell}>Value</th>
                  <th style={cell}>Show</th>
                  <th style={cell}>Width</th>
                  <th style={cell}>Height</th>
                  <th style={cell}>Thickness</th>
                  {more && <th style={cell}>Italic</th>}
                  {more && <th style={cell}>Bold</th>}
                  <th style={cell}>Layer</th>
                  {more && <th style={cell}>Orientation</th>}
                  {more && <th style={cell}>Keep Upright</th>}
                  {more && <th style={cell}>X Offset</th>}
                  {more && <th style={cell}>Y Offset</th>}
                  {more && <th style={cell}>Knockout</th>}
                  {more && <th style={cell}>Mirrored</th>}
                  {more && <th style={cell}>H</th>}
                  {more && <th style={cell}>V</th>}
                </tr>
              </thead>
              <tbody>
                {rows.map((r, i) => {
                  const mandatory = i < 2;
                  const abs = norm360(sense * ((r.layout.angle ?? 0) / 1000) - (part.rot ?? 0));
                  return (
                    <tr key={r.key} onClick={() => setSelected(i)} style={{ background: i === selected ? "var(--chrome-hover, rgba(128,128,128,.18))" : undefined }} data-testid={`field-row-${i}`}>
                      <td style={cell}>
                        <input value={r.name} disabled={mandatory} onChange={(e) => setRow(i, { name: e.target.value })} style={{ width: 110 }} />
                      </td>
                      <td style={cell}>
                        <input value={r.text} disabled={mandatory} onChange={(e) => setRow(i, { text: e.target.value })} style={{ width: 130 }} />
                      </td>
                      <td style={cell}>
                        <input type="checkbox" checked={r.layout.visible !== false} onChange={(e) => setLayout(i, { visible: e.target.checked })} />
                      </td>
                      <td style={cell}>{lengthCell(r.layout.size[0], (w) => setLayout(i, { size: [w, r.layout.size[1]] }))}</td>
                      <td style={cell}>{lengthCell(r.layout.size[1], (h) => setLayout(i, { size: [r.layout.size[0], h] }))}</td>
                      <td style={cell}>{lengthCell(r.layout.thickness ?? 0, (t) => setLayout(i, { thickness: t }))}</td>
                      {more && (
                        <td style={cell}>
                          <input type="checkbox" checked={!!r.layout.italic} onChange={(e) => setLayout(i, { italic: e.target.checked })} />
                        </td>
                      )}
                      {more && (
                        <td style={cell}>
                          <input type="checkbox" checked={!!r.layout.bold} onChange={(e) => setLayout(i, { bold: e.target.checked })} />
                        </td>
                      )}
                      <td style={cell}>
                        <select value={r.layout.layer} onChange={(e) => setLayout(i, { layer: e.target.value })} style={{ width: 92 }}>
                          {(layers.includes(r.layout.layer) ? layers : [...layers, r.layout.layer]).map((l) => (
                            <option key={l} value={l}>
                              {l}
                            </option>
                          ))}
                        </select>
                      </td>
                      {more && (
                        <td style={cell}>
                          <input
                            type="number"
                            step="any"
                            value={Number(abs.toFixed(3))}
                            onChange={(e) => setLayout(i, { angle: localAngleOf(part, Math.round(norm360(num(e.target.value, abs)) * 1000)) })}
                            style={{ width: 58 }}
                          />
                        </td>
                      )}
                      {more && (
                        <td style={cell}>
                          <input type="checkbox" checked={r.layout.keep_upright !== false} onChange={(e) => setLayout(i, { keep_upright: e.target.checked })} />
                        </td>
                      )}
                      {more && <td style={cell}>{lengthCell(sense * r.layout.at.x, (v) => setLayout(i, { at: { ...r.layout.at, x: sense * v } }))}</td>}
                      {more && <td style={cell}>{lengthCell(r.layout.at.y, (v) => setLayout(i, { at: { ...r.layout.at, y: v } }))}</td>}
                      {more && (
                        <td style={cell}>
                          <input type="checkbox" checked={!!r.layout.knockout} onChange={(e) => setLayout(i, { knockout: e.target.checked })} />
                        </td>
                      )}
                      {more && (
                        <td style={cell}>
                          <input type="checkbox" checked={!!r.layout.mirror} onChange={(e) => setLayout(i, { mirror: e.target.checked })} />
                        </td>
                      )}
                      {more && (
                        <td style={cell}>
                          <select value={r.layout.halign ?? 0} onChange={(e) => setLayout(i, { halign: Number(e.target.value) as -1 | 0 | 1 })}>
                            <option value={-1}>Left</option>
                            <option value={0}>Center</option>
                            <option value={1}>Right</option>
                          </select>
                        </td>
                      )}
                      {more && (
                        <td style={cell}>
                          <select value={r.layout.valign ?? 0} onChange={(e) => setLayout(i, { valign: Number(e.target.value) as -1 | 0 | 1 })}>
                            <option value={-1}>Top</option>
                            <option value={0}>Center</option>
                            <option value={1}>Bottom</option>
                          </select>
                        </td>
                      )}
                    </tr>
                  );
                })}
              </tbody>
            </table>
          </div>
          <div style={{ display: "flex", gap: 6, margin: "6px 0 10px" }}>
            <button
              data-testid="add-field"
              onClick={() => {
                setRows((rs) => [...rs, { key: Math.max(-1, ...rs.map((r) => r.key)) + 1, from: null, name: newFieldName(rs.map((r) => r.name), rs.length), text: "", layout: newFieldLayout(part) }]);
                setSelected(rows.length);
              }}
            >
              Add Field
            </button>
            <button
              data-testid="delete-field"
              disabled={selected < 2 || selected >= rows.length}
              title={selected < 2 ? "The first two fields are mandatory." : "Delete the selected field"}
              onClick={() => {
                setRows((rs) => rs.filter((_, j) => j !== selected));
                setSelected((s) => Math.max(0, s - 1));
              }}
            >
              Delete Field
            </button>
            <label style={{ marginLeft: "auto" }}>
              <input type="checkbox" checked={more} onChange={(e) => setMore(e.target.checked)} /> Show all columns
            </label>
          </div>

          <div style={{ display: "grid", gridTemplateColumns: "1fr 1fr", gap: 14 }}>
            <div>
              <p style={{ margin: "0 0 4px", fontWeight: 600, fontSize: 11, color: "var(--chrome-text-dim)" }}>Placement</p>
              <div className="kv-grid" style={{ gridTemplateColumns: "110px 1fr" }}>
                <span>Position X</span>
                {lengthCell(x, setX, 90)}
                <span>Position Y</span>
                {lengthCell(y, setY, 90)}
                <span>Orientation</span>
                <span>
                  <input type="number" step="any" value={Number(orientation.toFixed(3))} onChange={(e) => setOrientation(num(e.target.value, orientation))} style={{ width: 90 }} data-testid="fp-orientation" />°
                </span>
                <span>Board side</span>
                <select value={bottom ? "bottom" : "top"} onChange={(e) => setBottom(e.target.value === "bottom")}>
                  <option value="top">Front (F.Cu)</option>
                  <option value="bottom">Back (B.Cu)</option>
                </select>
                <span>Locked</span>
                <input type="checkbox" checked={locked} onChange={(e) => setLocked(e.target.checked)} />
                <span>Library link</span>
                <span style={{ fontSize: 11 }}>{part.footprint ?? part.package ?? "–"}</span>
                <span>Pads / nets</span>
                <span style={{ fontSize: 11 }}>
                  {part.pads?.length ?? 0} / {nets.join(", ") || "–"}
                </span>
              </div>
            </div>
            <div>
              <p style={{ margin: "0 0 4px", fontWeight: 600, fontSize: 11, color: "var(--chrome-text-dim)" }}>Attributes</p>
              <label style={{ display: "block", marginBottom: 4 }}>
                Component type{" "}
                <select value={attrs.kind} onChange={(e) => attr("kind", e.target.value as FootprintAttrsCmd["kind"])} data-testid="fp-type">
                  <option value="through_hole">Through hole</option>
                  <option value="smd">SMD</option>
                  <option value="unspecified">Unspecified</option>
                </select>
              </label>
              {check("Not in schematic", "board_only")}
              {check("Exclude from position files", "exclude_from_pos_files")}
              {check("Exclude from bill of materials", "exclude_from_bom")}
              {check("Do not populate", "dnp")}
              {check("Exempt from courtyard requirement", "allow_missing_courtyard")}
            </div>
          </div>
          {error && (
            <p style={{ color: "var(--error, #e5534b)", margin: "10px 0 0", fontSize: 12, whiteSpace: "pre-line" }} data-testid="footprint-properties-error">
              {error}
            </p>
          )}
          <p style={{ color: "var(--chrome-text-dim)", fontSize: 11, marginTop: 10 }}>
            The Reference and Value come from the schematic. A footprint&apos;s position lands on the placement grid. Do not populate and exclude from BOM also set the schematic symbol.
          </p>
        </div>
        <div className="dialog-footer">
          <button onClick={() => dispatch({ type: "SET_MOVE_EXACT_DIALOG_OPEN", open: true })}>Move Exactly...</button>
          <button onClick={close}>Cancel</button>
          <button className="primary" onClick={() => void submit()} disabled={busy} data-testid="footprint-properties-ok">
            OK
          </button>
        </div>
      </div>
    </div>
  );
}
