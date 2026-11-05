// `L`/Ctrl+`L`/`H` (sch_drawing_tools.cpp TwoClickPlace/createNewLabel): the dialog a new label is made with, KiCad's order -- the tool's first click (or
// its hotkey, where the cursor is) opens it, and OK puts the label in the hand: it rides the cursor (actions/schPlaceActions.ts, drawn by
// SchematicView) until the click that places it with one `place_label` command, one undo step. Cancel makes nothing and the tool stays armed
// (`createNewLabel` returns false).
//
// The dialog is `DIALOG_LABEL_PROPERTIES` as `createNewLabel` opens it on a new label (`aNew`): the text, starting empty, with the labels of the same
// kind on the sheet to pick from (KiCad's combo box: the labels of every screen, a global label's also the global power symbols' nets; here the
// sheet in view's), the shape of a global or hierarchical label, which way the text runs, its size and style, and the "Auto" check of a global or
// hierarchical label (`SCH_LABEL_BASE::AutoRotateOnPlacement`), which has the label take the spin of what it is placed on
// (`SCH_EDIT_FRAME::AutoRotateItem`; kicad-port/schAutoRotate.ts shows the spin it will give). "Multiple label input" takes one label per line
// (`m_cbMultiLine`, new labels only): the first rides the cursor and each click brings the next (`itemsToPlace`). What was last chosen is the next
// dialog's start, as the tool's `m_lastTextOrientation`, `m_lastGlobalLabelShape` and `m_lastAutoLabelRotateOnPlacement` are (kicad-port/schLastPlaced.ts).
//
// Properties (`E`) of the label in the hand opens this dialog on it (`editing`): OK changes the label that rides, Cancel leaves it as it was.
//
// A label's auto-increment is KiCad's: Repeat Last Item (Insert) places another copy with its text stepped on (`IncrementLabel`), and Increment acts on
// the label in the hand (kicad-port/schPlaceTool.ts). Until 2026-11-05 this dialog suggested the last text stepped on instead; KiCad's starts empty.
import { useEffect, useMemo, useState } from "react";
import { useStudioApi, useStudioDispatch, useStudioState } from "../state/store";
import { labelOrientationForPoint, orientationSceneOf } from "../kicad-port/schAutoRotate";
import { lastPlaced, rememberPlaced, type LabelSpinName } from "../kicad-port/schLastPlaced";
import { mmToNm, nmToMm } from "../kicad-port/schProperties";
import { LABEL_TOOL_OF, labelsOfEntry, type CarriedLabel } from "../kicad-port/schPlaceTool";
import { carryItem, changeCarried } from "../actions/schPlaceActions";
import { getSchCarry, setSchCarry, useSchCarry } from "../state/schCarryStore";
import { inferSpin } from "./schematic/labelShape";
import type { LabelShape } from "../api/types";

const SHAPES: LabelShape[] = ["input", "output", "bidirectional", "tri_state", "passive"];

const SPINS: Array<{ value: LabelSpinName; label: string }> = [
  { value: "right", label: "Right" },
  { value: "left", label: "Left" },
  { value: "up", label: "Up" },
  { value: "bottom", label: "Bottom" },
];

const TITLE: Record<"local" | "global" | "hierarchical", string> = {
  local: "Label Properties",
  global: "Global Label Properties",
  hierarchical: "Hierarchical Label Properties",
};

const DEFAULT_SIZE_NM = 1_270_000; // SCHEMATIC_SETTINGS::m_DefaultTextSize, 50 mil

/** The label in the hand whose Properties are open, if they are. */
function carriedBeingEdited(): CarriedLabel | null {
  const c = getSchCarry();
  return c?.editing && c.item.kind === "label" ? c.item : null;
}

export function LabelDialog() {
  const state = useStudioState();
  const dispatch = useStudioDispatch();
  const api = useStudioApi();
  const pending = state.schLabelPending;
  const carry = useSchCarry();
  const editing = carry?.editing && carry.item.kind === "label" ? carry.item : null;

  const [text, setText] = useState("");
  const [multiLine, setMultiLine] = useState(false);
  const [shape, setShape] = useState<LabelShape>("input");
  // "" is "From the wire": the spin is read off the wire the label ends at when it is placed, and kept (KiCad's label always has a spin of its own)
  const [spin, setSpin] = useState<LabelSpinName | "">("right");
  const [auto, setAuto] = useState(false);
  const [size, setSize] = useState(nmToMm(DEFAULT_SIZE_NM));
  const [bold, setBold] = useState(false);
  const [italic, setItalic] = useState(false);

  // Seeded on every opening, not on every keystroke: from the label in the hand, or from what the tool last placed.
  useEffect(() => {
    if (!pending) return;
    const from = carriedBeingEdited();
    const last = lastPlaced();
    setText(from?.text ?? "");
    setMultiLine(false);
    setShape(from?.shape ?? last.shape);
    setSpin(from ? from.spin : last.spin);
    setAuto(from?.autoRotate ?? last.autoRotate);
    setSize(nmToMm(from?.sizeNm ?? DEFAULT_SIZE_NM));
    setBold(from?.bold ?? last.bold);
    setItalic(from?.italic ?? last.italic);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [pending?.at[0], pending?.at[1], pending?.scope]);

  const sch = state.schematic;
  const supportsAuto = pending !== null && pending.scope !== "local";
  // what the label would be without "Auto", and what "Auto" makes of it where it is made (`TransferDataFromWindow` runs `AutoRotateItem` there)
  const plainSpin: LabelSpinName | null = useMemo(() => (pending && sch ? spin || inferSpin(sch.wires, pending.at) : null), [pending, sch, spin]);
  const autoSpin: LabelSpinName | null = useMemo(
    () => (pending && sch && plainSpin ? labelOrientationForPoint(orientationSceneOf(sch), pending.at, plainSpin) : null),
    [pending, sch, plainSpin]
  );
  const autoOn = supportsAuto && auto;
  // The labels of the same kind to pick from (`m_existingLabelArray`), and the nets of the power symbols for a global label.
  const existing = useMemo(() => {
    if (!pending || !sch || pending.scope === "hierarchical") return [];
    const names = new Set(sch.labels.filter((l) => l.scope === pending.scope).map((l) => l.net));
    if (pending.scope === "global") for (const p of sch.power_symbols) names.add(p.net);
    return [...names].sort();
  }, [pending, sch]);

  if (!pending) return null;
  const close = () => {
    const now = getSchCarry();
    if (now?.editing) setSchCarry({ ...now, editing: false });
    dispatch({ type: "SET_SCH_LABEL_PENDING", pending: null });
  };

  const sizeNm = mmToNm(size);
  // `m_textSize.Validate( 0.01, 1000.0, EDA_UNITS::MM )`: "Don't allow text to disappear"
  const sizeOk = sizeNm !== null && sizeNm >= 10_000 && sizeNm <= 1_000_000_000;
  const canAdd = text.trim().length > 0 && sizeOk;

  const submit = () => {
    if (!canAdd || sizeNm === null) return;
    // With Auto the label has the spin of what it was made on (`AutoRotateItem` in `TransferDataFromWindow`), and keeps it while it rides
    const madeSpin: LabelSpinName | "" = autoOn && autoSpin ? autoSpin : spin;
    const base: CarriedLabel = { kind: "label", scope: pending.scope, text, shape, spin: madeSpin, sizeNm: sizeNm === DEFAULT_SIZE_NM ? null : sizeNm, bold, italic, autoRotate: autoOn };
    dispatch({ type: "SET_SCH_LABEL_PENDING", pending: null });
    if (editing) {
      changeCarried(base);
      return;
    }
    const labels = labelsOfEntry(base, text, multiLine);
    if (labels.length === 0) return;
    // the tool remembers what it was last given (`m_lastTextOrientation`, `m_lastTextBold`, `m_lastTextItalic`, `m_lastGlobalLabelShape`, `m_lastAutoLabelRotateOnPlacement`)
    rememberPlaced({ spin: madeSpin, bold, italic, ...(supportsAuto ? { shape, autoRotate: auto } : {}) });
    carryItem({ api }, LABEL_TOOL_OF[pending.scope], labels[0]!, [pending.at[0], pending.at[1]], labels.slice(1));
  };

  return (
    <div className="dialog-backdrop" onClick={close}>
      <div className="dialog" style={{ width: 360 }} onClick={(e) => e.stopPropagation()}>
        <div className="dialog-header">
          <span>{TITLE[pending.scope]}</span>
        </div>
        <div className="dialog-body">
          <div className="kv-grid" style={{ gridTemplateColumns: "96px 1fr" }}>
            <span>Label</span>
            {multiLine ? (
              <textarea autoFocus aria-label="Label" rows={5} value={text} onChange={(e) => setText(e.target.value)} />
            ) : (
              <input
                autoFocus
                aria-label="Label"
                list={existing.length > 0 ? "label-dialog-existing" : undefined}
                value={text}
                onChange={(e) => setText(e.target.value)}
                onKeyDown={(e) => {
                  if (e.key === "Enter") submit();
                }}
              />
            )}
            {existing.length > 0 && (
              <datalist id="label-dialog-existing">
                {existing.map((n) => (
                  <option key={n} value={n} />
                ))}
              </datalist>
            )}
            {!editing && (
              <>
                <span />
                <label>
                  <input
                    type="checkbox"
                    aria-label="Multiple label input"
                    checked={multiLine}
                    onChange={(e) => {
                      // `onMultiLabelCheck`: back to one line keeps the first line
                      if (!e.target.checked) setText((t) => t.split("\n")[0] ?? "");
                      setMultiLine(e.target.checked);
                    }}
                  />{" "}
                  Multiple label input
                </label>
              </>
            )}
            {pending.scope !== "local" && (
              <>
                <span>Shape</span>
                <select value={shape} onChange={(e) => setShape(e.target.value as LabelShape)}>
                  {SHAPES.map((s) => (
                    <option key={s} value={s}>
                      {s}
                    </option>
                  ))}
                </select>
              </>
            )}
            <span>Orientation</span>
            <select
              aria-label="Orientation"
              value={autoOn && autoSpin ? autoSpin : spin}
              disabled={autoOn}
              title={autoOn ? "Auto: the label takes the spin of what it is placed on" : undefined}
              onChange={(e) => setSpin(e.target.value as LabelSpinName | "")}
            >
              {!autoOn && <option value="">From the wire</option>}
              {SPINS.map((s) => (
                <option key={s.value} value={s.value}>
                  {s.label}
                </option>
              ))}
            </select>
            {supportsAuto && (
              <>
                <span>Auto</span>
                <label>
                  <input type="checkbox" aria-label="Auto" checked={auto} onChange={(e) => setAuto(e.target.checked)} /> Rotate to what it is placed on
                </label>
              </>
            )}
            <span>Text size (mm)</span>
            <input value={size} onChange={(e) => setSize(e.target.value)} onKeyDown={(e) => e.key === "Enter" && submit()} style={sizeOk ? undefined : { outline: "1px solid var(--error, #d33)" }} title="0.01 to 1000 mm" />
            <span>Style</span>
            <span style={{ display: "flex", gap: 14 }}>
              <label>
                <input type="checkbox" checked={bold} onChange={(e) => setBold(e.target.checked)} /> Bold
              </label>
              <label>
                <input type="checkbox" checked={italic} onChange={(e) => setItalic(e.target.checked)} /> Italic
              </label>
            </span>
          </div>
        </div>
        <div className="dialog-footer">
          <button onClick={close}>Cancel</button>
          <button className="primary" disabled={!canAdd} onClick={submit}>
            OK
          </button>
        </div>
      </div>
    </div>
  );
}
