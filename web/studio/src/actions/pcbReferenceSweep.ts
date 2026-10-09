// The reference-point rows of the UI-actions sweep (docs/parity/UI-ACTIONS.md): the commands that ask for a point
// before they act. Each cites KiCad's source at 8303b2ad; the picker they share is actions/pcbPicker.ts
// (PCB_PICKER_TOOL), the arithmetic is kicad-port/pcbReference.ts. Called from `registerPcbEditSweep`.
//
//   pcbnew.InteractiveMove.moveWithReference      EDIT_TOOL::doMoveSelection + EDIT_TOOL::pickReferencePoint
//   pcbnew.InteractiveMove.copyWithReference      EDIT_TOOL::copyToClipboard + EDIT_TOOL::pickReferencePoint
//   pcbnew.PositionRelative.interactiveOffsetTool POSITION_RELATIVE_TOOL::InteractiveOffset + DIALOG_OFFSET_ITEM

import { createElement } from "react";
import { OffsetItemDialog } from "../components/PcbReferenceDialogs";
import { movableItem } from "../kicad-port/pcbEditActions";
import { editableSelection } from "../kicad-port/pcbTransform";
import { interactiveOffsetMove, moveSelectionCommands, offsetVector, type Vec } from "../kicad-port/pcbReference";
import type { PickerSession } from "../kicad-port/pickerHost";
import { applyCommands, createSweepHelpers, type SweepCtx } from "./pcbSweepKit";
import { picker } from "./pcbPicker";
import { openSweepDialog } from "./pcbSweepDialogs";

export function registerPcbReferenceSweep(m: Map<string, () => void>, ctx: SweepCtx): void {
  const { state, dispatch, api } = ctx;
  const board = state.board;
  const { toast, pcbOnly, requestFiltered } = createSweepHelpers(ctx);

  // ---------------------------------------------------------------- Move with Reference Point
  // doMoveSelection(): `RequestSelection` (markers, hierarchy, free pads and locked items filtered), then
  // `pickReferencePoint( "Select reference point for move...", "", "" )`; cancelling it pops the tool and clears a
  // hover selection. The picked point becomes the selection's reference point and the cursor is held on it, i.e. the
  // move is carried by that point from the first motion on -- the move tool's origin here.
  m.set(
    "pcbnew.InteractiveMove.moveWithReference",
    pcbOnly(() => {
      if (!board) return;
      const hovered = state.selection.size === 0;
      const ids = requestFiltered(() => true);
      if (!ids.some((id) => movableItem(board, id) !== null)) {
        toast("Select an item to move.");
        return;
      }
      if (hovered) dispatch({ type: "SET_SELECTION", refs: ids });
      picker.start({
        kind: "point",
        prompt: "Select reference point for move...",
        onPoint: (at) => {
          dispatch({ type: "SET_ACTIVE_TOOL", tool: "move" });
          dispatch({ type: "SET_MOVE_ORIGIN", at });
          return false;
        },
        onCancel: () => {
          if (hovered) dispatch({ type: "CLEAR_SELECTION" });
        },
      });
    })
  );

  // ---------------------------------------------------------------- Copy with Reference Point
  // copyToClipboard(): `RequestSelection` (hierarchy and markers filtered; only Cut drops locked items), then
  // `pickReferencePoint( "Select reference point for the copy...", "Selection copied", "Copy canceled" )`; the
  // picked point is saved as the selection's reference point, which is where a paste is anchored.
  m.set(
    "pcbnew.InteractiveMove.copyWithReference",
    pcbOnly(() => {
      if (!board) return;
      const ids = ctx.requestSelection();
      if (editableSelection(board, ids, { respectLocks: false }).ids.length === 0) {
        toast("Select something to copy.");
        return;
      }
      picker.start({
        kind: "point",
        prompt: "Select reference point for the copy...",
        onPoint: (at) => {
          api.copySelection(ids, at);
          toast("Selection copied");
          return false;
        },
        onCancel: () => toast("Copy canceled"),
      });
    })
  );

  // ---------------------------------------------------------------- Interactive Offset Tool
  // InteractiveOffset(): a tool that stays active until Escape. Its first click is the reference point on the item
  // to move, its second the point to measure the new offset from; DIALOG_OFFSET_ITEM then edits the measured vector
  // and the selection moves by the difference. Escape between the two clicks backs out to the first.
  m.set(
    "pcbnew.PositionRelative.interactiveOffsetTool",
    pcbOnly(() => {
      if (!board) return;
      // `frame()->IsCurrentTool( ACTIONS::measureTool )`
      if (state.activeTool === "measure") return;
      const hovered = state.selection.size === 0;
      const ids = requestFiltered(() => true);
      if (!ids.some((id) => movableItem(board, id) !== null)) {
        toast("Select an item first.");
        return;
      }
      if (hovered) dispatch({ type: "SET_SELECTION", refs: ids });
      // The ruler is the measure tool's; leave a route or drawing in progress alone.
      const ruler = (pts: [number, number][] | null) => {
        const draw = api.getState().drawState;
        if (draw !== null && draw.kind !== "measure") return;
        dispatch({ type: "SET_DRAW_STATE", draw: pts === null ? null : { kind: "measure", pts } });
      };

      const apply = async (first: Vec, second: Vec, edited: Vec) => {
        const live = api.getState();
        const movable = [...live.selection].filter((id) => live.board && movableItem(live.board, id) && !(live.board.locked ?? []).includes(id));
        const cmds = live.board ? moveSelectionCommands(live.board, movable, interactiveOffsetMove(first, second, edited)) : [];
        if (cmds.length) await applyCommands(api, dispatch, cmds);
        else if (movable.length === 0) toast("Select an item first.");
      };

      const stageOne = (): PickerSession => ({
        kind: "point",
        prompt: "Select the reference point on the item to move.",
        onPoint: (first) => stageTwo(first),
        // Escape with nothing started ends the tool.
        onCancel: () => ruler(null),
      });
      const stageTwo = (first: Vec): PickerSession => {
        ruler([[first.x, first.y]]);
        return {
          kind: "point",
          prompt: "Select the point to define the new offset from.",
          onPoint: (second) => {
            // "Leave the arrow in place": the finished ruler stays up while the dialog is open.
            ruler([
              [first.x, first.y],
              [second.x, second.y],
            ]);
            openSweepDialog({
              kind: "element",
              element: createElement(OffsetItemDialog, {
                offset: offsetVector(first, second),
                onResult: (edited: Vec | null) => {
                  if (edited) void apply(first, second, edited);
                  // Back to waiting for a first click. After the key press that closed the dialog has finished, so
                  // that same Escape does not also end the tool.
                  setTimeout(() => picker.start(stageOne()), 0);
                },
              }),
            });
            return false;
          },
          // cleanup(): Escape backs out to waiting for the first click; another tool taking over ends the tool.
          onCancel: (activated) => {
            ruler(null);
            if (!activated) picker.start(stageOne());
          },
        };
      };
      picker.start(stageOne());
    })
  );
}
