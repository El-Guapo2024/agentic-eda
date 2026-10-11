// Port of pcbnew/dialogs/dialog_create_array{,_base}.cpp -- "Create Array" (pcbnew.Array.createArray, Ctrl+T) for the board editor: a grid or a circular array of the
// selection, as copies ("Duplicate selection", the default) or as a new arrangement of the selected items ("Arrange selection"), with the dialog's own entries:
//   Grid Array      Grid Array Size, Items Spacing (spacing and offset), Stagger Settings (Rows / Columns), Grid Position (Source items remain in place / Centre on
//                   source items);
//   Circular Array  Center position (typed, or "Select Point..." / "Select Item..." on the board), Duplication Settings (Full circle, Direction, Angle between items,
//                   Item count, First item angle, Rotate items);
//   below the tabs  Item Source (Duplicate / Arrange selection) and, for copies, Footprint Annotation (Keep existing reference designators / Assign unique reference
//                   designators: `ReannotateDuplicates`).
// The entries are text, read when OK is pressed (`TransferDataFromWindow`): a bad one is named, with what was typed, and the dialog stays. The last values that went
// through are kept for the next time (`s_arrayOptions`). The circular centre starts at `ARRAY_TOOL::CreateArray`'s `origin` -- the item, or the middle of the selection --
// which the dialog of this KiCad version is handed and never uses; here it is the default.
//
// Not KiCad's: a small picture of where the items go, drawn from the entries as they are typed. Not ported: the footprint editor's half of the dialog (pad numbering with
// `ARRAY_AXIS`), which has no multi-pad selection to work on here, and which the board editor never shows either (`enableArrayNumbering = m_isFootprintEditor`).
import { useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { picker, pickItem, pickPoint } from "../actions/pcbPicker";
import {
  DEFAULT_ARRAY_OPTIONS,
  arrayCmd,
  arrayOrigin,
  entriesOf,
  lengthEntry,
  optionsFromEntries,
  previewPoints,
  rememberedOptions,
  type ArrayEntries,
  type ArrayOptions,
} from "../kicad-port/arrayOptions";
import { itemBounds, itemPosition } from "../kicad-port/pcbItems";
import { useStudioApi, useStudioDispatch, useStudioState } from "../state/store";
import type { LengthUnit } from "../state/units";

/** `s_arrayOptions`: what the last Create Array that went through held. */
let remembered: ArrayOptions = DEFAULT_ARRAY_OPTIONS;

export function CreateArrayDialog() {
  return useStudioState().createArrayDialogOpen ? <ArrayForm /> : null;
}

const MAX_DRAWN = 400;

/** The picture: the positions the first item goes to, the centre and the circle for a circular array. */
function Preview({ points, circle }: { points: Array<[number, number]>; circle: { centre: [number, number]; through: [number, number] } | null }) {
  const W = 436;
  const H = 96;
  const edge = 10;
  const drawn = points.slice(0, MAX_DRAWN);
  const all: Array<[number, number]> = circle ? [...drawn, circle.centre] : drawn;
  if (all.length === 0) {
    return (
      <div data-testid="array-preview" style={{ height: H, display: "flex", alignItems: "center", justifyContent: "center", color: "var(--chrome-text-dim)", fontSize: 11 }}>
        Nothing to show until the entries make an array.
      </div>
    );
  }
  const xs = all.map((p) => p[0]);
  const ys = all.map((p) => p[1]);
  let radius = 0;
  if (circle) radius = Math.hypot(circle.through[0] - circle.centre[0], circle.through[1] - circle.centre[1]);
  const x0 = Math.min(...xs, ...(circle ? [circle.centre[0] - radius] : []));
  const x1 = Math.max(...xs, ...(circle ? [circle.centre[0] + radius] : []));
  const y0 = Math.min(...ys, ...(circle ? [circle.centre[1] - radius] : []));
  const y1 = Math.max(...ys, ...(circle ? [circle.centre[1] + radius] : []));
  const scale = Math.min((W - 2 * edge) / Math.max(x1 - x0, 1), (H - 2 * edge) / Math.max(y1 - y0, 1));
  const px = (x: number) => edge + (W - 2 * edge - (x1 - x0) * scale) / 2 + (x - x0) * scale;
  const py = (y: number) => edge + (H - 2 * edge - (y1 - y0) * scale) / 2 + (y - y0) * scale;
  return (
    <svg data-testid="array-preview" width="100%" viewBox={`0 0 ${W} ${H}`} style={{ display: "block", maxHeight: H }} role="img" aria-label={`${points.length} positions`}>
      {circle && (
        <>
          <circle cx={px(circle.centre[0])} cy={py(circle.centre[1])} r={radius * scale} fill="none" stroke="var(--chrome-border)" strokeDasharray="3 3" />
          <path d={`M${px(circle.centre[0]) - 4} ${py(circle.centre[1])}h8M${px(circle.centre[0])} ${py(circle.centre[1]) - 4}v8`} stroke="var(--chrome-text-dim)" />
        </>
      )}
      {drawn.map((p, i) => (
        <circle key={i} cx={px(p[0])} cy={py(p[1])} r={i === 0 ? 4 : 2.5} fill={i === 0 ? "none" : "var(--chrome-accent)"} stroke="var(--chrome-accent)" strokeWidth={i === 0 ? 1.5 : 0} />
      ))}
    </svg>
  );
}

/** One labelled entry of the dialog, with the unit after it. */
function Entry({ label, tip, value, onChange, unit, disabled, testid }: { label: string; tip?: string; value: string; onChange: (v: string) => void; unit?: string; disabled?: boolean; testid: string }) {
  return (
    <label title={tip} style={{ display: "flex", alignItems: "center", gap: 8, marginBottom: 4, opacity: disabled ? 0.5 : 1 }}>
      <span style={{ width: 128, flexShrink: 0 }}>{label}</span>
      <input value={value} disabled={disabled} onChange={(e) => onChange(e.target.value)} aria-label={label.replace(/:$/, "")} data-testid={testid} style={{ flex: 1, minWidth: 0 }} />
      <span style={{ width: 26, flexShrink: 0, color: "var(--chrome-text-dim)" }}>{unit ?? ""}</span>
    </label>
  );
}

function Radio({ checked, onChange, children, tip, testid }: { checked: boolean; onChange: () => void; children: ReactNode; tip?: string; testid: string }) {
  return (
    <label title={tip} style={{ display: "block", marginBottom: 2 }}>
      <input type="radio" checked={checked} onChange={onChange} data-testid={testid} /> {children}
    </label>
  );
}

function ArrayForm() {
  const state = useStudioState();
  const dispatch = useStudioDispatch();
  const api = useStudioApi();
  const units: LengthUnit = state.units;
  const board = state.board;
  const ids = useMemo(() => [...state.selection], [state.selection]);
  const origin = useMemo(() => (board ? arrayOrigin(ids, (id) => itemPosition(board, id), (id) => itemBounds(board, id)) : null), [board, ids]);

  const [flags, setFlags] = useState<ArrayOptions>(() => remembered);
  const [entries, setEntries] = useState<ArrayEntries>(() => entriesOf({ ...remembered, centerX: origin?.[0] ?? remembered.centerX, centerY: origin?.[1] ?? remembered.centerY }, units));
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  // `OnSelectCenterButton`: the centre can be picked on the board while the dialog waits (it steps aside, keeping what was typed).
  const [picking, setPicking] = useState(false);
  const pickingRef = useRef(false);
  useEffect(
    () => () => {
      if (pickingRef.current) picker.cancel(true);
    },
    []
  );
  useEffect(() => {
    if (picking) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== "Escape") return;
      e.stopPropagation();
      e.preventDefault();
      dispatch({ type: "SET_CREATE_ARRAY_DIALOG_OPEN", open: false });
    };
    document.addEventListener("keydown", onKey, true);
    return () => document.removeEventListener("keydown", onKey, true);
  }, [picking, dispatch]);

  const set = (key: keyof ArrayEntries, value: string) => setEntries((e) => ({ ...e, [key]: value }));
  const flag = <K extends keyof ArrayOptions>(key: K, value: ArrayOptions[K]) => setFlags((f) => ({ ...f, [key]: value }));
  const runPick = async <T,>(pick: () => Promise<T | null>): Promise<T | null> => {
    pickingRef.current = true;
    setPicking(true);
    const result = await pick();
    pickingRef.current = false;
    setPicking(false);
    return result;
  };
  const setCentre = (x: number, y: number) => setEntries((e) => ({ ...e, centerX: lengthEntry(Math.round(x), units), centerY: lengthEntry(Math.round(y), units) }));
  /** `UpdatePickedItem`: the centre becomes the item's position. */
  const selectCentreItem = async () => {
    const id = await runPick(() => pickItem("Select center item..."));
    const at = id && board ? itemPosition(board, id) : null;
    if (at) setCentre(at[0], at[1]);
  };
  /** `UpdatePickedPoint`: the centre becomes the point. */
  const selectCentrePoint = async () => {
    const p = await runPick(() => pickPoint("Select center point..."));
    if (p) setCentre(p.x, p.y);
  };

  if (picking) return null;

  const close = () => dispatch({ type: "SET_CREATE_ARRAY_DIALOG_OPEN", open: false });
  const parsed = optionsFromEntries(flags, entries, units);
  const points = parsed.ok && origin ? previewPoints(parsed.options, origin) : [];
  const circular = flags.tab === "circular";
  // With "Full circle" the angle entry is disabled and shows the division (`calculateCircularArrayProperties`).
  const count = Number(entries.count);
  const angleShown = flags.fullCircle && Number.isInteger(count) && count > 0 ? String(Number((360 / count).toFixed(4))) : entries.angle;

  const submit = async () => {
    const r = optionsFromEntries(flags, entries, units);
    if (!r.ok) return setError(r.error);
    if (ids.length === 0) return setError("Select at least one item first.");
    setError(null);
    setBusy(true);
    try {
      if (await api.cmd(arrayCmd(r.options, ids))) {
        remembered = rememberedOptions(r.options);
        close();
      }
    } finally {
      setBusy(false);
    }
  };

  const group = (legend: string, children: ReactNode) => (
    <fieldset style={{ marginBottom: 8 }}>
      <legend>{legend}</legend>
      {children}
    </fieldset>
  );

  return (
    <div className="dialog-backdrop" onClick={close}>
      <div className="dialog" style={{ width: 500 }} onClick={(e) => e.stopPropagation()}>
        <div className="dialog-header">
          <span>Create Array</span>
          <span>
            {ids.length} item{ids.length === 1 ? "" : "s"}
          </span>
        </div>
        <div
          className="dialog-body"
          style={{ maxHeight: "76vh", overflowY: "auto" }}
          data-testid="create-array"
          onKeyDown={(e) => {
            if (e.key === "Enter" && e.target instanceof HTMLInputElement && e.target.type === "text") void submit();
          }}
        >
          <div style={{ display: "flex", gap: 4, marginBottom: 10 }}>
            <button className={!circular ? "primary" : undefined} onClick={() => flag("tab", "grid")} style={{ flex: 1 }} data-testid="array-tab-grid">
              Grid Array
            </button>
            <button className={circular ? "primary" : undefined} onClick={() => flag("tab", "circular")} style={{ flex: 1 }} data-testid="array-tab-circular">
              Circular Array
            </button>
          </div>

          <div style={{ marginBottom: 6 }}>
            <Preview points={points} circle={circular && parsed.ok && origin ? { centre: [parsed.options.centerX, parsed.options.centerY], through: origin } : null} />
            <div style={{ fontSize: 11, color: "var(--chrome-text-dim)", textAlign: "right" }} data-testid="array-count-note">
              {points.length > 0 ? `${points.length} position${points.length === 1 ? "" : "s"}${ids.length > 1 && !flags.arrange ? `, ${points.length * ids.length} items` : ""}` : ""}
            </div>
          </div>
          {!circular && (
            <>
              {group(
                "Grid Array Size",
                <>
                  <Entry label="Horizontal count:" tip="Number of columns" value={entries.nx} onChange={(v) => set("nx", v)} testid="array-nx" />
                  <Entry label="Vertical count:" tip="Number of rows" value={entries.ny} onChange={(v) => set("ny", v)} testid="array-ny" />
                </>
              )}
              {group(
                "Items Spacing",
                <>
                  <Entry label="Horizontal spacing:" tip="Distance between columns" value={entries.dx} onChange={(v) => set("dx", v)} unit={units} testid="array-dx" />
                  <Entry label="Vertical spacing:" tip="Distance between rows" value={entries.dy} onChange={(v) => set("dy", v)} unit={units} testid="array-dy" />
                  <Entry label="Horizontal offset:" tip="Offset added to the next row position." value={entries.offsetX} onChange={(v) => set("offsetX", v)} unit={units} testid="array-offset-x" />
                  <Entry label="Vertical offset:" tip="Offset added to the next column position" value={entries.offsetY} onChange={(v) => set("offsetY", v)} unit={units} testid="array-offset-y" />
                </>
              )}
              {group(
                "Stagger Settings",
                <div style={{ display: "flex", alignItems: "center", gap: 14 }}>
                  <div style={{ flex: 1 }}>
                    <Entry label="Stagger:" tip="Value -1, 0 or 1 disable this option." value={entries.stagger} onChange={(v) => set("stagger", v)} testid="array-stagger" />
                  </div>
                  <Radio checked={flags.staggerRows} onChange={() => flag("staggerRows", true)} testid="array-stagger-rows">
                    Rows
                  </Radio>
                  <Radio checked={!flags.staggerRows} onChange={() => flag("staggerRows", false)} testid="array-stagger-columns">
                    Columns
                  </Radio>
                </div>
              )}
              {group(
                "Grid Position",
                <>
                  <Radio checked={!flags.centred} onChange={() => flag("centred", false)} testid="array-in-place">
                    Source items remain in place
                  </Radio>
                  <Radio checked={flags.centred} onChange={() => flag("centred", true)} testid="array-centred">
                    Centre on source items
                  </Radio>
                </>
              )}
            </>
          )}

          {circular && (
            <>
              {group(
                "Center position",
                <>
                  <Entry label="Center pos X:" value={entries.centerX} onChange={(v) => set("centerX", v)} unit={units} testid="array-center-x" />
                  <Entry label="Center pos Y:" value={entries.centerY} onChange={(v) => set("centerY", v)} unit={units} testid="array-center-y" />
                  <div style={{ display: "flex", gap: 8, marginTop: 4 }}>
                    <button type="button" style={{ flex: 1 }} onClick={() => void selectCentrePoint()} data-testid="array-select-point">
                      Select Point...
                    </button>
                    <button type="button" style={{ flex: 1 }} onClick={() => void selectCentreItem()} data-testid="array-select-item">
                      Select Item...
                    </button>
                  </div>
                </>
              )}
              {group(
                "Duplication Settings",
                <>
                  <label style={{ display: "block", marginBottom: 4 }}>
                    <input type="checkbox" checked={flags.fullCircle} onChange={(e) => flag("fullCircle", e.target.checked)} data-testid="array-full-circle" /> Full circle
                  </label>
                  <div style={{ marginBottom: 4 }}>
                    <span style={{ color: "var(--chrome-text-dim)" }}>Direction</span>
                    <div style={{ display: "flex", gap: 16 }}>
                      <Radio checked={flags.clockwise} onChange={() => flag("clockwise", true)} testid="array-clockwise">
                        Clockwise
                      </Radio>
                      <Radio checked={!flags.clockwise} onChange={() => flag("clockwise", false)} testid="array-anticlockwise">
                        Anti-clockwise
                      </Radio>
                    </div>
                  </div>
                  <Entry
                    label="Angle between items:"
                    tip={'Positive angles represent an anti-clockwise rotation. An angle of 0 will produce a full circle divided evenly into "Count" portions.'}
                    value={angleShown}
                    onChange={(v) => set("angle", v)}
                    unit="deg"
                    disabled={flags.fullCircle}
                    testid="array-angle"
                  />
                  <Entry label="Item count:" tip="How many items in the array." value={entries.count} onChange={(v) => set("count", v)} testid="array-count" />
                  <Entry label="First item angle:" tip="Angle offset of the first item in the array" value={entries.offsetAngle} onChange={(v) => set("offsetAngle", v)} unit="deg" testid="array-offset-angle" />
                  <label title="Rotate the item as well as move it - multi-selections will be rotated together" style={{ display: "block", marginTop: 4 }}>
                    <input type="checkbox" checked={flags.rotateItems} onChange={(e) => flag("rotateItems", e.target.checked)} data-testid="array-rotate-items" /> Rotate items
                  </label>
                </>
              )}
            </>
          )}

          {group(
            "Item Source",
            <>
              <Radio checked={!flags.arrange} onChange={() => flag("arrange", false)} testid="array-duplicate">
                Duplicate selection
              </Radio>
              <Radio checked={flags.arrange} onChange={() => flag("arrange", true)} tip="This can conflict with reference designators in the schematic that have not yet been synchronized with the board." testid="array-arrange">
                Arrange selection
              </Radio>
            </>
          )}
          {!flags.arrange &&
            group(
              "Footprint Annotation",
              <>
                <Radio checked={!flags.reannotate} onChange={() => flag("reannotate", false)} testid="array-keep-refs">
                  Keep existing reference designators
                </Radio>
                <Radio checked={flags.reannotate} onChange={() => flag("reannotate", true)} tip="This can conflict with reference designators in the schematic that have not yet been synchronized with the board." testid="array-unique-refs">
                  Assign unique reference designators
                </Radio>
              </>
            )}

          {ids.length === 0 && <div style={{ fontSize: 11, opacity: 0.7, marginTop: 4 }}>Select at least one item first.</div>}
          {error && (
            <p style={{ color: "var(--chrome-danger, #e5534b)", margin: "8px 0 0", fontSize: 12, whiteSpace: "pre-line" }} data-testid="array-error">
              {error}
            </p>
          )}
        </div>
        <div className="dialog-footer">
          <button onClick={close} data-testid="array-cancel">
            Cancel
          </button>
          <button className="primary" onClick={() => void submit()} disabled={busy || ids.length === 0} data-testid="array-ok">
            OK
          </button>
        </div>
      </div>
    </div>
  );
}
