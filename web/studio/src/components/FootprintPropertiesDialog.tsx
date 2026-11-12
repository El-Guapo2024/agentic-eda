// pcbnew/dialogs/dialog_footprint_properties.cpp -- "Footprint Properties" of a footprint on the board (`pcbnew.InteractiveEdit.properties` on a
// footprint, a double-click, or on one of its fields). The field grid (`PCB_FIELDS_GRID_TABLE`: name, value, show, width, height, thickness, italic, layer,
// orientation, keep upright, X and Y offset, knockout, mirrored, and the justification and bold the Properties panel has), Add and Delete for the user fields, the
// general part (position, orientation, board side, locked), the component type and the attributes (board only, exclude from position files and from the BOM,
// do not populate, exempt from the courtyard requirement), and the zone fields of the Clearances tab and of Pad Properties: the footprint's "Zone connection"
// and "Clearance", and, for each pad number, its "Pad connection", "Relief gap", "Spoke width", "Spoke angle" and "Clearance" -- the overrides the zone filler
// reads (`kicad-port/padZone.ts`; `set_footprint_zone_connection` / `set_pad_zone_overrides`).
//
// OK sends ONE batch, which is `TransferDataFromWindow`'s one `BOARD_COMMIT`: the fields and the attributes first (`edit_board_footprint`), then the
// position, the orientation, the side and the lock through the verbs the move, rotate, flip and lock tools use -- so a side change flips the layers and the
// mirroring the grid just set, as `FOOTPRINT::Flip` does -- then the zone connection of the footprint and of each pad whose fields were changed.
//
// Update Footprint... and Change Footprint... (`FP_PROPS_UPDATE_FP`, `FP_PROPS_CHANGE_FP`) close the dialog and open the exchange dialog on this footprint
// (components/PcbGlobalEditDialogs.tsx, `ShowExchangeFootprintsDialog( aFootprint, updateMode, true )`).
//
// The 3D Models tab (`PANEL_FP_PROPERTIES_3D_MODEL`, components/footprint/FootprintModelsPanel.tsx) edits a copy of the footprint's list of models; OK sends it as one
// `set_board_models` in the same batch when it differs (kicad-port/fpModels.ts `boardModelsCmds`), and the tab's Extruded 3D Body box one `set_extruded_body` (`bodyCmds`).
//
// The Embedded Files tab is the shared `PANEL_EMBEDDED_FILES` (components/EmbeddedFilesPanel.tsx) on the footprint's own list: the dialog holds the panel's copy, and OK sends
// it as one `embedded apply` in the same batch (`TransferDataFromWindow`, kicad-port/embeddedFiles.ts `applyCmd`).
//
// UI-LEFT P14 (task-u5-pcbfp): the grid's four mandatory rows (`PCB_FIELD::IsMandatory`: Reference, Value, Datasheet, Description; "The first 4 fields are
// mandatory."), the Reference and Value texts edited and carried to the schematic symbol as Update Schematic from PCB would (`BACK_ANNOTATE`'s references and
// values: `edit_symbol_fields`, then `rename_symbol` last, so the batch's other commands name the footprint as it was; a footprint no symbol is for is renamed on
// the board alone), the Datasheet and Description the footprint's own once edited; a repeated field name taken as KiCad's reader takes it
// (`mergeDuplicateFields`); the footprint's Solder paste absolute and relative clearance (`set_footprint_paste_margin`, any footprint).
//
// UI-LEFT-2 PP18 (task-v5-pp18-fpprops): a Datasheet or Description edited here is the footprint's and is carried to the symbol, as Update Schematic from PCB's "Other
// fields" would (`kicad-port/fpFields.ts` `backAnnotateEdits`: `set_symbol_fields`, before the rename); the footprint's own "Solder mask expansion:" and "Allow
// bridged solder mask apertures between pads" (`set_footprint_solder_mask`, any footprint); a user row named like a mandatory field is taken as KiCad's reader takes
// it (its text and look on that field: `mergeDuplicateFields`), no longer refused.
//
// Not ported: jumper pad groups (shown by KiCad, not edited), the user fields carried to the symbol (Update Schematic from PCB's "Other fields" adds and deletes them).
import { useEffect, useMemo, useState } from "react";
import type { Cmd, FieldInfo, FieldLayoutCmd, FootprintAttrsCmd, PadConnection, Part } from "../api/types";
import { checkFieldRow, fieldsOf, layoutOf, localAngleOf, backAnnotateEdits, mergeDuplicateFields, newFieldLayout, newFieldName, parseFieldId, attrsOf, MANDATORY_ROW_COUNT, REFERENCE, VALUE } from "../kicad-port/fpFields";
import { footprintZoneForm, padNumbers, padZoneForm, zoneCmds, type FootprintZoneForm, type PadZoneForm } from "../kicad-port/padZone";
import { PAD_CONNECTION_OPTIONS } from "../kicad-port/padSettings";
import { useActionRunner } from "../actions/useActionRunner";
import { boardLayerList, layerDisplayName } from "../kicad-port/boardLayers";
import { useStudioApi, useStudioDispatch, useStudioState } from "../state/store";
import { formatLength, nmFrom, nmTo } from "../state/units";
import { coordField, coordValue, useOriginTransforms } from "../state/originTransforms";
import { bodyCmds, bodyDraftOf, bodyDraftProblem, boardModelsCmds, tabRows, type BodyDraft, type ModelRow } from "../kicad-port/fpModels";
import { fpOrientation } from "../kicad-port/pcbProperties";
import { FootprintModelsPanel } from "./footprint/FootprintModelsPanel";
import { EmbeddedFilesPanel } from "./EmbeddedFilesPanel";
import { applyCmd, EMPTY_DRAFT, type EmbeddedDraft } from "../kicad-port/embeddedFiles";
import type { EmbeddedListing } from "../api/embeddedTypes";
import { AngleOverrideInput, OverrideInput } from "./footprint/PadPropertiesDialog";

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
  const { run } = useActionRunner();
  const open = state.footprintPropertiesOpen;
  const board = state.board;
  const units = state.units;
  const xf = useOriginTransforms();

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
  // The zone fields, read from the part whenever the dialog opens on a (different) footprint -- never on every render, so a keystroke is not overwritten by
  // the next poll of the board. Each pad number's form is kept once it was opened, so a pad left for another keeps what was typed; `padZones` holds the
  // forms that were opened, and OK sends the ones that differ from what the part carries.
  const [fpZone, setFpZone] = useState<FootprintZoneForm>({ connection: null, clearance: null });
  // "Solder paste absolute clearance" and "relative clearance" (`m_solderPaste`, `MARGIN_OFFSET_BINDER`): null is the board's
  const [paste, setPaste] = useState<{ margin: number | null; ratio: number | null }>({ margin: null, ratio: null });
  // "Solder mask expansion:" (`m_solderMask`, null is the board's) and "Allow bridged solder mask apertures between pads" (`m_allowSolderMaskBridges`)
  const [mask, setMask] = useState<{ margin: number | null; bridges: boolean }>({ margin: null, bridges: false });
  const [padNum, setPadNum] = useState("");
  const [padZones, setPadZones] = useState<Record<string, PadZoneForm>>({});
  // The 3D Models tab: a copy of the footprint's list, which OK sends when it differs.
  const [tab, setTab] = useState<"general" | "models" | "embedded">("general");
  // The Embedded Files tab's copy of the footprint's list (`m_localFiles`), which OK sends
  const [embedded, setEmbedded] = useState<{ draft: EmbeddedDraft; listing: EmbeddedListing | null }>({ draft: EMPTY_DRAFT, listing: null });
  const [models, setModels] = useState<ModelRow[]>([]);
  // and its Extruded 3D Body box (`EXTRUDED_3D_BODY`): KiCad's controls, which OK sends as one `set_extruded_body` when they changed the body
  const [body, setBody] = useState<BodyDraft>(() => bodyDraftOf({}));

  // Fill the form when the dialog opens on a footprint -- not on every poll, or a keystroke would be lost to the next refresh.
  useEffect(() => {
    if (!open || !part) return;
    setRows(fieldsOf(part).map((f, i) => ({ key: i, from: f.name, name: f.name, text: f.text, layout: layoutOf(f) })));
    setAttrs(attrsOf(part));
    setX(part.at?.[0] ?? 0);
    setY(part.at?.[1] ?? 0);
    // KiCad's orientation is counter-clockwise; the model's `rot` runs clockwise.
    setOrientation(fpOrientation(part));
    setBottom(part.side === "bottom");
    setLocked((board?.locked ?? []).includes(part.ref));
    // Asked for a field (a double click on it, or E), the dialog starts on that field's row.
    const asked = first ? parseFieldId(first) : null;
    setSelected(Math.max(0, asked ? fieldsOf(part).findIndex((f) => f.name === asked.name) : 0));
    setFpZone(footprintZoneForm(part));
    setPaste({ margin: part.paste_margin ?? null, ratio: part.paste_margin_ratio ?? null });
    setMask({ margin: part.mask_margin ?? null, bridges: !!part.allow_soldermask_bridges });
    setPadNum(padNumbers(part)[0] ?? "");
    setPadZones({});
    // the numbers the file has (`kicad-port/fpModels.ts` `tabRows`)
    setModels(tabRows(part));
    setBody(bodyDraftOf(part));
    setTab("general");
    setEmbedded({ draft: EMPTY_DRAFT, listing: null });
    setError(null);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open, part?.ref]);

  // The layers the board has (Board Setup > Board Editor Layers).
  const layerSetup = board?.board_rules?.layer_setup;
  const layers = useMemo(() => boardLayerList(board?.layers ?? [], layerSetup), [board?.layers, layerSetup]);
  if (!open || !part || !attrs) return null;
  const close = () => dispatch({ type: "SET_FOOTPRINT_PROPERTIES_OPEN", open: false });

  const infos = new Map<string, FieldInfo>(fieldsOf(part).map((f) => [f.name, f]));
  const nets = [...new Set((part.pads ?? []).map((q) => q.net).filter((n): n is string => !!n))];
  const len = (um: number) => formatLength(um, units);

  // The form of the pad number the zone fields show: what was typed there, else what the part carries.
  const padZone: PadZoneForm = padZones[padNum] ?? padZoneForm(part, padNum);
  const setPadZone = (patch: Partial<PadZoneForm>) => setPadZones((z) => ({ ...z, [padNum]: { ...(z[padNum] ?? padZoneForm(part, padNum)), ...patch } }));
  // `PADSTACK::DefaultThermalSpokeAngleForShape`: an X for a circle, a + for everything else this view can tell apart.
  const shownPad = part.pads?.find((q) => q.num === padNum);
  const padIsCircle = !!shownPad && shownPad.round && shownPad.w === shownPad.h;

  const setRow = (i: number, patch: Partial<Row>) => setRows((rs) => rs.map((r, j) => (j === i ? { ...r, ...patch } : r)));
  const setLayout = (i: number, patch: Partial<FieldLayoutCmd>) => setRows((rs) => rs.map((r, j) => (j === i ? { ...r, layout: { ...r.layout, ...patch } } : r)));
  const num = (v: string, fallback: number): number => (Number.isFinite(Number(v)) && v.trim() !== "" ? Number(v) : fallback);

  /** A length cell: the value in the current unit, written back in um. */
  const lengthCell = (um: number, onChange: (um: number) => void, width = 62) => (
    <input type="number" step="any" value={Number(nmTo(um, units).toFixed(4))} onChange={(e) => onChange(Math.round(nmFrom(num(e.target.value, nmTo(um, units)), units)))} style={{ width }} />
  );
  // Position X / Y are `ABS_X_COORD` / `ABS_Y_COORD` (`m_posX.SetCoordType`): counted from Origins & Axes' origin, each axis either way (UI-LEFT-2 PP9).
  const coordCell = (um: number, onChange: (um: number) => void, type: "abs_x" | "abs_y", aria: string) => (
    <input type="number" step="any" aria-label={aria} value={Number(coordField(xf, um, type, units).toFixed(4))} onChange={(e) => onChange(Math.round(coordValue(xf, num(e.target.value, coordField(xf, um, type, units)), type, units)))} style={{ width: 90 }} />
  );

  // A bottom-side footprint's frame is the top-side one mirrored: the dialog shows the offset as the file stores it, x un-mirrored (`GetFPRelativePosition`).
  const sense = part.side === "bottom" ? -1 : 1;

  const submit = async () => {
    setError(null);
    for (let i = 0; i < rows.length; i++) {
      const r = rows[i]!;
      const message = checkFieldRow(r.name, { size: r.layout.size, thickness: r.layout.thickness }, len);
      if (message) return setError(`${r.name || `Field ${i + 1}`}: ${message}`);
    }
    // A repeated field name as KiCad's reader takes it: the later row's text and look on the first row of the name, a mandatory one's too.
    const merged = mergeDuplicateFields(rows);
    const [ref0, val0, ...users] = merged;
    const original = (name: string): FieldInfo | undefined => infos.get(name);
    const same = (a: unknown, b: unknown): boolean => JSON.stringify(a) === JSON.stringify(b);

    const cmds: Cmd[] = [];
    const edit: Extract<Cmd, { op: "edit_board_footprint" }> = { op: "edit_board_footprint", part: part.ref };
    if (ref0 && !same(ref0.layout, layoutOf(original(REFERENCE)!))) edit.reference = ref0.layout;
    // A new Value text is the footprint's own (`FOOTPRINT::SetValue`: the Value field's text), and the symbol's below
    const valueChanged = !!val0 && val0.text !== (original(VALUE)?.text ?? "");
    if (val0 && (valueChanged || !same(val0.layout, layoutOf(original(VALUE)!)))) edit.value = valueChanged ? { ...val0.layout, text: val0.text } : val0.layout;
    // The user fields, and the Datasheet and Description when the footprint has its own or they were edited (else they stay the symbol's or the file's).
    const mandatoryKept = (r: Row): boolean => {
      const o = original(r.name);
      return !(r.name === "Datasheet" || r.name === "Description") || !!o?.custom || !o || r.text !== o.text || !same(r.layout, layoutOf(o));
    };
    const userFields = users.filter(mandatoryKept).map((r) => ({ name: r.name.trim(), text: r.text, layout: r.layout }));
    const before = fieldsOf(part)
      .filter((f) => f.name !== REFERENCE && f.name !== VALUE && (!(f.name === "Datasheet" || f.name === "Description") || f.custom))
      .map((f) => ({ name: f.name, text: f.text, layout: layoutOf(f) }));
    if (!same(userFields, before)) edit.fields = userFields;
    if (!same(attrs, attrsOf(part))) edit.attrs = attrs;
    if (edit.reference || edit.value || edit.fields || edit.attrs) cmds.push(edit);
    // The 3D Models tab: the whole list, when it is not the footprint's own (`m_footprint->Models() = ...` in `TransferDataFromWindow`).
    cmds.push(...boardModelsCmds(part, models));
    // The Extruded 3D Body box: `TransferDataFromWindow` refuses the OK, with its message, for a height or a standoff it does not take.
    const bodyProblem = bodyDraftProblem(body);
    if (bodyProblem) {
      setTab("models");
      return setError(`Extruded 3D Body: ${bodyProblem}`);
    }
    cmds.push(...bodyCmds(part, body));

    // The pose, as the move, rotate, flip and lock tools do it (the footprint's own position and side; the fields follow).
    const at = part.at ?? [0, 0];
    const [dx, dy] = [Math.round(x - at[0]), Math.round(y - at[1])];
    if (dx !== 0 || dy !== 0) cmds.push({ op: "move_items", ids: [part.ref], dx, dy });
    // the difference, in (-180, 180] (KiCad's orientation of a back footprint is half a turn from `-rot`; the turn is the same)
    const turn = norm360(orientation - fpOrientation(part) + 180) - 180;
    if (Math.abs(turn) > 1e-9) {
      // `rotate_items` turns clockwise; KiCad's orientation is the other way.
      const pivot = { x: Math.round(x), y: Math.round(y) };
      cmds.push({ op: "rotate_items", ids: [part.ref], pivot, angle_millideg: Math.round(-turn * 1000) });
    }
    // A change of side flips the footprint across the axis the preferences say (`m_FlipDirection`, `DIALOG_FOOTPRINT_PROPERTIES::TransferDataFromWindow`).
    if (bottom !== (part.side === "bottom")) cmds.push({ op: "flip_items", ids: [part.ref], pivot: { x: Math.round(x), y: Math.round(y) }, direction: state.prefs.flipDirection });
    if (locked !== (board?.locked ?? []).includes(part.ref)) cmds.push({ op: "set_locked", ids: [part.ref], locked });

    // The zone connection of the footprint, and of every pad whose fields were changed: the overlay the zone filler reads.
    cmds.push(...zoneCmds(part, fpZone, padZones));
    // The Embedded Files tab's copy (`PANEL_EMBEDDED_FILES::TransferDataFromWindow`)
    const embeddedCmd = applyCmd({ kind: "footprint", reference: part.ref }, embedded.listing, embedded.draft);
    if (embeddedCmd) cmds.push(embeddedCmd);
    // The footprint's own paste margins (`SetLocalSolderPasteMargin`, `SetLocalSolderPasteMarginRatio`).
    if (paste.margin !== (part.paste_margin ?? null) || paste.ratio !== (part.paste_margin_ratio ?? null)) cmds.push({ op: "set_footprint_paste_margin", part: part.ref, margin: paste.margin, ratio: paste.ratio });
    // The footprint's own mask expansion and bridging (`SetLocalSolderMaskMargin`, `SetAllowSolderMaskBridges`).
    if (mask.margin !== (part.mask_margin ?? null) || mask.bridges !== !!part.allow_soldermask_bridges) cmds.push({ op: "set_footprint_solder_mask", part: part.ref, margin: mask.margin, allow_bridges: mask.bridges });
    // The Reference and Value texts, carried to the schematic's symbol as Update Schematic from PCB would (`BACK_ANNOTATE::applyChangelist`: the value, then the
    // reference); the rename last, as every command before it names the footprint by its old reference.
    if (val0 && valueChanged) cmds.push({ op: "edit_symbol_fields", id: part.ref, value: val0.text });
    // The Datasheet and the Description the dialog changed, carried to the symbol as Update Schematic from PCB's "Other fields" would (UI-LEFT-2 PP18).
    const symbolEdits = backAnnotateEdits(part, attrs.board_only, new Map([...infos].map(([n, f]) => [n, f.text])), merged);
    if (symbolEdits.length > 0) cmds.push({ op: "set_symbol_fields", edits: symbolEdits });
    if (ref0 && ref0.text.trim() !== part.ref) cmds.push({ op: "rename_symbol", id: part.ref, new_id: ref0.text.trim() });
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
          <div style={{ display: "flex", gap: 2, borderBottom: "1px solid var(--chrome-border, #444)", marginBottom: 8 }} role="tablist">
            {(
              [
                ["general", "General"],
                ["models", "3D Models"],
                ["embedded", "Embedded Files"],
              ] as const
            ).map(([id, label]) => (
              <button
                key={id}
                role="tab"
                aria-selected={tab === id}
                onClick={() => setTab(id)}
                style={{ borderBottom: tab === id ? "2px solid var(--chrome-accent, #4a90e2)" : "2px solid transparent", fontWeight: tab === id ? 600 : 400, borderRadius: 0 }}
                data-testid={`footprint-properties-tab-${id}`}
              >
                {label}
              </button>
            ))}
          </div>
          {tab === "models" && <FootprintModelsPanel rows={models} onRows={setModels} previewOf={{ part: part.ref }} units={units} body={body} onBody={setBody} exportPart={part.ref} />}
          {tab === "embedded" && (
            <EmbeddedFilesPanel owner={{ kind: "footprint", reference: part.ref }} run={api.cmd} version={state.version} draft={embedded.draft} onDraft={(draft, listing) => setEmbedded({ draft, listing })} />
          )}
          <div style={{ display: tab === "general" ? undefined : "none" }}>
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
                  const mandatory = i < MANDATORY_ROW_COUNT;
                  const abs = norm360(sense * ((r.layout.angle ?? 0) / 1000) - (part.rot ?? 0));
                  return (
                    <tr key={r.key} onClick={() => setSelected(i)} style={{ background: i === selected ? "var(--chrome-hover, rgba(128,128,128,.18))" : undefined }} data-testid={`field-row-${i}`}>
                      <td style={cell}>
                        <input value={r.name} disabled={mandatory} onChange={(e) => setRow(i, { name: e.target.value })} style={{ width: 110 }} />
                      </td>
                      <td style={cell}>
                        <input value={r.text} aria-label={`${r.name} text`} onChange={(e) => setRow(i, { text: e.target.value })} style={{ width: 130 }} />
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
                              {layerDisplayName(layerSetup, l)}
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
                setRows((rs) => [...rs, { key: Math.max(-1, ...rs.map((r) => r.key)) + 1, from: null, name: newFieldName([], rs.length) /* `GetUserFieldName( m_fields->size() )`: Field4 after the four mandatory rows, whatever names the rows have */, text: "", layout: newFieldLayout(part, state.board?.board_rules?.text_graphics) }]);
                setSelected(rows.length);
              }}
            >
              Add Field
            </button>
            <button
              data-testid="delete-field"
              disabled={selected < MANDATORY_ROW_COUNT || selected >= rows.length}
              title={selected < MANDATORY_ROW_COUNT ? `The first ${MANDATORY_ROW_COUNT} fields are mandatory.` : "Delete the selected field"}
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
                {coordCell(x, setX, "abs_x", "Position X")}
                <span>Position Y</span>
                {coordCell(y, setY, "abs_y", "Position Y")}
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
          {part.placed && (
            <div style={{ display: "grid", gridTemplateColumns: "1fr 1fr", gap: 14, marginTop: 12 }} data-testid="fp-zone">
              <div>
                <p style={{ margin: "0 0 4px", fontWeight: 600, fontSize: 11, color: "var(--chrome-text-dim)" }}>Zone connection (footprint)</p>
                <div className="kv-grid" style={{ gridTemplateColumns: "110px 1fr" }}>
                  <span>Pad connection</span>
                  <select
                    value={fpZone.connection ?? ""}
                    onChange={(e) => setFpZone((f) => ({ ...f, connection: e.target.value === "" ? null : (e.target.value as PadConnection) }))}
                    title="FOOTPRINT::GetLocalZoneConnection: how a zone connects to the pads of this footprint that do not set their own"
                    data-testid="fp-zone-connection"
                  >
                    {PAD_CONNECTION_OPTIONS.map((o) => (
                      <option key={o.value} value={o.value}>
                        {o.value === "" ? "Inherited" : o.label}
                      </option>
                    ))}
                  </select>
                  <span>Clearance</span>
                  <OverrideInput testid="fp-zone-clearance" valueNm={fpZone.clearance} units={units} onChange={(v) => setFpZone((f) => ({ ...f, clearance: v }))} />
                  {/* the Clearance Overrides' "Solder mask expansion:" (empty for the board's) and "Allow bridged solder mask apertures between pads" */}
                  <span title={"This is the local clearance between pads and the solder mask for this footprint.\nThis value can be superseded by a pad local value.\nIf 0, the global value is used."}>Solder mask expansion</span>
                  <OverrideInput testid="fp-mask-margin" valueNm={mask.margin} units={units} onChange={(v) => setMask((m) => ({ ...m, margin: v }))} />
                  <label style={{ gridColumn: "1 / span 2" }}>
                    <input type="checkbox" checked={mask.bridges} onChange={(e) => setMask((m) => ({ ...m, bridges: e.target.checked }))} data-testid="fp-mask-bridges" /> Allow bridged solder mask apertures between pads
                  </label>
                  {/* the Clearance Overrides' solder paste: "Solder paste absolute clearance:" and "Solder paste relative clearance:" (%), empty for the board's */}
                  <span title="This is the local clearance between this footprint's pads and the solder paste. This value can be superseded by local pad values.">Solder paste absolute clearance</span>
                  <OverrideInput testid="fp-paste-margin" valueNm={paste.margin} units={units} onChange={(v) => setPaste((p) => ({ ...p, margin: v }))} />
                  <span title="This is the local clearance ratio in percent between this footprint's pads and the solder paste.">Solder paste relative clearance</span>
                  <span>
                    <input
                      type="number"
                      step="any"
                      aria-label="Solder paste relative clearance"
                      data-testid="fp-paste-ratio"
                      value={paste.ratio == null ? "" : Number((paste.ratio * 100).toFixed(4))}
                      onChange={(e) => setPaste((p) => ({ ...p, ratio: e.target.value === "" || !Number.isFinite(Number(e.target.value)) ? null : Number(e.target.value) / 100 }))}
                      style={{ width: 70 }}
                    />{" "}
                    %
                  </span>
                </div>
              </div>
              {padNumbers(part).length > 0 && (
                <div>
                  <p style={{ margin: "0 0 4px", fontWeight: 600, fontSize: 11, color: "var(--chrome-text-dim)" }}>Zone connection (pad)</p>
                  <div className="kv-grid" style={{ gridTemplateColumns: "110px 1fr" }}>
                    <span>Pad</span>
                    <select value={padNum} onChange={(e) => setPadNum(e.target.value)} data-testid="fp-zone-pad">
                      {padNumbers(part).map((n) => (
                        <option key={n} value={n}>
                          {n}
                        </option>
                      ))}
                    </select>
                    <span>Pad connection</span>
                    <select
                      value={padZone.connection ?? ""}
                      onChange={(e) => setPadZone({ connection: e.target.value === "" ? null : (e.target.value as PadConnection) })}
                      title="PAD::GetLocalZoneConnection: over the footprint's and the zone's own setting"
                      data-testid="fp-zone-pad-connection"
                    >
                      {PAD_CONNECTION_OPTIONS.map((o) => (
                        <option key={o.value} value={o.value}>
                          {o.label}
                        </option>
                      ))}
                    </select>
                    <span>Relief gap</span>
                    <OverrideInput testid="fp-zone-pad-gap" valueNm={padZone.gap} units={units} onChange={(v) => setPadZone({ gap: v })} />
                    <span>Spoke width</span>
                    <OverrideInput testid="fp-zone-pad-spoke-width" valueNm={padZone.spokeWidth} units={units} onChange={(v) => setPadZone({ spokeWidth: v })} />
                    <span>Spoke angle</span>
                    <AngleOverrideInput testid="fp-zone-pad-spoke-angle" valueMdeg={padZone.spokeAngleMdeg} defaultDeg={padIsCircle ? 45 : 90} onChange={(v) => setPadZone({ spokeAngleMdeg: v })} />
                    <span>Clearance</span>
                    <OverrideInput testid="fp-zone-pad-clearance" valueNm={padZone.clearance} units={units} onChange={(v) => setPadZone({ clearance: v })} />
                  </div>
                </div>
              )}
            </div>
          )}
          </div>
          {error && (
            <p style={{ color: "var(--chrome-danger, #e5534b)", margin: "10px 0 0", fontSize: 12, whiteSpace: "pre-line" }} data-testid="footprint-properties-error">
              {error}
            </p>
          )}
          {tab === "general" && (
            <p style={{ color: "var(--chrome-text-dim)", fontSize: 11, marginTop: 10 }}>
              The Reference and Value come from the schematic. A footprint&apos;s position lands on the placement grid. Do not populate and exclude from BOM also set the schematic symbol.
            </p>
          )}
        </div>
        <div className="dialog-footer">
          <button onClick={() => dispatch({ type: "SET_MOVE_EXACT_DIALOG_OPEN", open: true })}>Move Exactly...</button>
          <button
            title="Update this footprint from its library copy"
            data-testid="footprint-properties-update"
            onClick={() => {
              close();
              run("pcbnew.GlobalEdit.updateFootprint");
            }}
          >
            Update Footprint...
          </button>
          <button
            title="Put this footprint on another footprint of the libraries"
            data-testid="footprint-properties-change"
            onClick={() => {
              close();
              run("pcbnew.GlobalEdit.changeFootprint");
            }}
          >
            Change Footprint...
          </button>
          {/* `FP_PROPS_EDIT_BOARD_FP` / `FP_PROPS_EDIT_LIBRARY_FP` (`PCB_EDIT_FRAME::ShowFootprintPropertiesDialog`): this footprint in the Footprint Editor, linked to it
              (`LoadFootprintFromBoard`, UI-LEFT F3), or the library footprint it names (`LoadFootprintFromLibrary`) */}
          <button
            data-testid="footprint-properties-edit-footprint"
            onClick={() => {
              close();
              run("pcbnew.EditorControl.EditFpInFpEditor");
            }}
          >
            Edit Footprint...
          </button>
          <button
            data-testid="footprint-properties-edit-library-footprint"
            onClick={() => {
              close();
              run("pcbnew.EditorControl.EditLibFpInFpEditor");
            }}
          >
            Edit Library Footprint...
          </button>
          <button onClick={close}>Cancel</button>
          <button className="primary" onClick={() => void submit()} disabled={busy} data-testid="footprint-properties-ok">
            OK
          </button>
        </div>
      </div>
    </div>
  );
}
