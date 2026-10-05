// The schematic editor's edit and drawing tools that the hotkey sweeps left unwired
// (docs/parity/UI-ACTIONS.md: eeschema.InteractiveEdit / InteractiveDrawing / PointEditor), registered into
// useActionRunner's handler map. Every handler cites the KiCad function it ports (eeschema/tools at 8303b2ad);
// the logic with no React in it lives in `kicad-port/sch*.ts` with unit tests, the verbs in crates/ops/src/sch_edit.rs.
import type { Dispatch } from "react";
import type { Schematic } from "../api/types";
import { GRID } from "../components/schematic/layout";
import { inferSpin } from "../components/schematic/labelShape";
import { allItems } from "../components/schematic/schItems";
import { measureStrokeText } from "../components/text/strokeFont";
import { alignToGrid } from "../kicad-port/gridSnap";
import { convertCmds, type ConvertSource, type ConvertTarget } from "../kicad-port/schConvertText";
import { lockCmd, type LockMode } from "../kicad-port/schLock";
import type { Action, StudioApi, StudioState } from "../state/store";
import type { SymbolEditorApi, SymAction } from "../state/symbolEditorStore";

export interface SchEditContext {
  state: StudioState;
  dispatch: Dispatch<Action>;
  api: StudioApi;
  symApi: SymbolEditorApi;
  symDispatch: Dispatch<SymAction>;
  /** `SCH_SELECTION_TOOL::RequestSelection`: the selection, or the item under the cursor. */
  requestSelection: () => string[];
  /** Adopt the hovered item as the selection when nothing is selected. */
  adoptHovered: () => string[];
  /** The cursor, grid-snapped (um), or null when the pointer is off the canvas. */
  cursorSnapped: () => [number, number] | null;
}

export function registerSchEditActions(m: Map<string, () => void>, ctx: SchEditContext): void {
  const { state, api, requestSelection } = ctx;
  const schematicOnly = (fn: () => void) => () => {
    if (state.tab === "schematic") fn();
  };
  const sch = state.schematic;

  // Lock / Unlock / Toggle Lock -- SCH_EDIT_TOOL::modifyLockSelected: RequestSelection, then `SetLocked` on every selected item (Toggle: unlock
  // when any selected item is locked, else lock). Which items change state is decided in kicad-port/schLock.ts; one verb, one undo step.
  const lock = (mode: LockMode) =>
    schematicOnly(() => {
      if (!sch) return;
      const cmd = lockCmd(mode, requestSelection(), new Set(sch.locked ?? []));
      if (cmd) void api.cmd({ op: "sch_edit", ...cmd });
    });
  m.set("eeschema.InteractiveEdit.lock", lock("lock"));
  m.set("eeschema.InteractiveEdit.unlock", lock("unlock"));
  m.set("eeschema.InteractiveEdit.toggleLock", lock("toggle"));

  // Change To Label / Global Label / Hierarchical Label / Directive Label / Text / Text Box -- SCH_EDIT_TOOL::ChangeTextType (kicad-port/schConvertText.ts):
  // every selected label, text, text box or directive label that is not already of the target type is replaced by one that is, in one undo step, and the
  // new items become the selection.
  const convert = (target: ConvertTarget) =>
    schematicOnly(() => {
      if (!sch) return;
      const sources = convertSources(sch, requestSelection());
      const { cmds, predictedIds } = convertCmds(sources, target, {
        measure: measureStrokeText,
        snap: (p) => {
          const a = alignToGrid({ x: p[0], y: p[1] }, GRID, { x: 0, y: 0 }, { ctrlOrCmd: false });
          return [a.x, a.y];
        },
        takenIds: new Set(allItems(sch).map((r) => r.id)),
      });
      if (cmds.length === 0) return;
      void api.cmdBatch(cmds).then((ok) => {
        if (ok) ctx.dispatch({ type: "SET_SELECTION", refs: predictedIds });
      });
    });
  m.set("eeschema.InteractiveEdit.toLabel", convert("label"));
  m.set("eeschema.InteractiveEdit.toGLabel", convert("global_label"));
  m.set("eeschema.InteractiveEdit.toHLabel", convert("hier_label"));
  m.set("eeschema.InteractiveEdit.toCLabel", convert("directive_label"));
  m.set("eeschema.InteractiveEdit.toText", convert("text"));
  m.set("eeschema.InteractiveEdit.toTextBox", convert("text_box"));
}

/** The labels, texts, text boxes and directive labels among `ids`, as the conversion's sources (a label's spin is read off its wire, as the painter does). */
function convertSources(sch: Schematic, ids: readonly string[]): ConvertSource[] {
  const out: ConvertSource[] = [];
  for (const id of ids) {
    const l = sch.labels.find((x) => x.id === id);
    if (l) {
      out.push({ kind: "label", id, net: l.net, at: l.at, scope: l.scope, shape: l.shape, spin: inferSpin(sch.wires, l.at) });
      continue;
    }
    const t = sch.texts.find((x) => x.id === id);
    if (t) {
      out.push({ kind: "text", id, content: t.content, at: t.at, angleDeg: t.angle, sizeUm: t.size_um });
      continue;
    }
    const g = (sch.graphics ?? []).find((x) => x.id === id);
    if (g?.shape.type === "text_box") out.push({ kind: "text_box", id, graphic: g });
    else if (g?.shape.type === "directive") out.push({ kind: "directive", id, graphic: g });
  }
  return out;
}
