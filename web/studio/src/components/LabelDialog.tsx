// `L`/Ctrl+`L`/`H` (sch_drawing_tools.cpp TwoClickPlace/createNewLabel):
// SchematicView.tsx's onPointerDown captures the click position into
// state.schLabelPending (scope already decided by which hotkey armed the
// tool), and this dialog is what turns that into a real label --
// same "click/draw first, small dialog last" shape ZoneDialog/TextDialog
// already use, not source's own "dialog pops up immediately, item then
// follows the cursor" order (see PARITY-sch.md for that documented
// adaptation).
//
// The dialog is `DIALOG_LABEL_PROPERTIES` as `createNewLabel` opens it: the text, the
// shape of a global or hierarchical label, which way the text runs, its size and style,
// and the "Auto" check of a global or hierarchical label
// (`SCH_LABEL_BASE::AutoRotateOnPlacement`), which has the label take the spin of what
// it is placed on (`SCH_EDIT_FRAME::AutoRotateItem`; kicad-port/schAutoRotate.ts shows
// the spin it will give). What was last chosen is the next dialog's start, as the
// tool's `m_lastTextOrientation`, `m_lastGlobalLabelShape` and
// `m_lastAutoLabelRotateOnPlacement` are (kicad-port/schLastPlaced.ts). OK is one
// `place_label` command, one undo step.
//
// Auto-increment (dialog_label_properties doesn't have this itself --
// it's `createNewLabel`'s *caller*, via `SCH_LABEL_BASE::IncrementLabel`,
// that seeds a chained placement's next suggestion): state.lastLabelText
// is incremented once per successful placement, so stamping down a bus of
// labels (DATA0, DATA1, DATA2...) only means confirming the suggestion,
// not re-typing it.
import { useEffect, useMemo, useState } from "react";
import { useStudioApi, useStudioDispatch, useStudioState } from "../state/store";
import { incrementLabelText } from "../kicad-port/incrementLabelText";
import { labelOrientationForPoint, orientationSceneOf } from "../kicad-port/schAutoRotate";
import { lastPlaced, rememberPlaced, type LabelSpinName } from "../kicad-port/schLastPlaced";
import { mmToUm, umToMm } from "../kicad-port/schProperties";
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

export function LabelDialog() {
  const state = useStudioState();
  const dispatch = useStudioDispatch();
  const api = useStudioApi();
  const pending = state.schLabelPending;

  const [text, setText] = useState("");
  const [shape, setShape] = useState<LabelShape>("input");
  // "" is "From the wire": the spin is read off the wire the label ends at when it is placed, and kept (KiCad's label always has a spin of its own)
  const [spin, setSpin] = useState<LabelSpinName | "">("right");
  const [auto, setAuto] = useState(false);
  const [size, setSize] = useState(umToMm(1270));
  const [bold, setBold] = useState(false);
  const [italic, setItalic] = useState(false);

  // Re-seed on every fresh click (not on every keystroke) -- same
  // dependency shape TextDialog.tsx's own re-seed effect uses.
  useEffect(() => {
    if (!pending) return;
    const last = lastPlaced();
    setText(incrementLabelText(state.lastLabelText));
    setShape(last.shape);
    setSpin(last.spin);
    setAuto(last.autoRotate);
    setSize(umToMm(1270));
    setBold(last.bold);
    setItalic(last.italic);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [pending?.at[0], pending?.at[1], pending?.scope]);

  const sch = state.schematic;
  const supportsAuto = pending !== null && pending.scope !== "local";
  // what the label would be without "Auto", and what "Auto" makes of it where it is put down
  const plainSpin: LabelSpinName | null = useMemo(() => (pending && sch ? spin || inferSpin(sch.wires, pending.at) : null), [pending, sch, spin]);
  const autoSpin: LabelSpinName | null = useMemo(
    () => (pending && sch && plainSpin ? labelOrientationForPoint(orientationSceneOf(sch), pending.at, plainSpin) : null),
    [pending, sch, plainSpin]
  );
  const autoOn = supportsAuto && auto;

  if (!pending) return null;
  const close = () => dispatch({ type: "SET_SCH_LABEL_PENDING", pending: null });

  const sizeUm = mmToUm(size);
  // `m_textSize.Validate( 0.01, 1000.0, EDA_UNITS::MM )`: "Don't allow text to disappear"
  const sizeOk = sizeUm !== null && sizeUm >= 10 && sizeUm <= 1_000_000;
  const canAdd = text.trim().length > 0 && sizeOk;

  const submit = () => {
    if (!canAdd) return;
    const kind = pending.scope === "local" ? ({ scope: "local" } as const) : ({ scope: pending.scope, shape } as const);
    void api.cmd({
      op: "sch_edit",
      verb: "place_label",
      net: text,
      at: { x: pending.at[0], y: pending.at[1] },
      kind,
      ...(spin || plainSpin ? { spin: (spin || plainSpin) as LabelSpinName } : {}),
      ...(sizeUm !== null && sizeUm !== 1270 ? { size_um: sizeUm } : {}),
      ...(bold ? { bold: true } : {}),
      ...(italic ? { italic: true } : {}),
      ...(autoOn ? { auto_rotate: true } : {}),
    });
    // the tool remembers what it was last given (`m_lastTextOrientation`, `m_lastTextBold`, `m_lastTextItalic`, `m_lastGlobalLabelShape`, `m_lastAutoLabelRotateOnPlacement`)
    rememberPlaced({ spin, bold, italic, ...(supportsAuto ? { shape, autoRotate: auto } : {}) });
    dispatch({ type: "SET_LAST_LABEL_TEXT", text });
    close();
  };

  return (
    <div className="dialog-backdrop" onClick={close}>
      <div className="dialog" style={{ width: 360 }} onClick={(e) => e.stopPropagation()}>
        <div className="dialog-header">
          <span>{TITLE[pending.scope]}</span>
        </div>
        <div className="dialog-body">
          <div className="kv-grid" style={{ gridTemplateColumns: "96px 1fr" }}>
            <span>Net name</span>
            <input
              autoFocus
              value={text}
              onChange={(e) => setText(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter") submit();
              }}
            />
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
            Add
          </button>
        </div>
      </div>
    </div>
  );
}
