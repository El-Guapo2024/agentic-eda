// "Create Array" (pcbnew.Array.createArray, Ctrl+T) in the Footprint Editor: `ARRAY_TOOL::CreateArray` with `m_isFootprintEditor`. The dialog is the board's one
// (components/CreateArrayDialog.tsx, `DIALOG_CREATE_ARRAY`) with its "Pad numbering" panels on and "Footprint Annotation" off (`enableArrayNumbering`); OK sends the
// selected pads, graphics and texts of the open footprint as one `create_footprint_array` (crates/ops/src/fp_array.rs), one undo step of this editor.
import { useMemo } from "react";
import { fpArrayCmd, arrayOrigin, previewPadNumbers } from "../../kicad-port/arrayOptions";
import { fpArrayIds, fpItemBounds, fpItemPosition } from "../../kicad-port/fpArray";
import { useFpApi, useFpDispatch, useFpState } from "../../state/footprintEditorStore";
import { useStudioState } from "../../state/store";
import { CreateArrayForm, type ArrayHost } from "../CreateArrayDialog";

export function FootprintCreateArrayDialog() {
  const state = useFpState();
  return state.createArrayOpen && state.footprint && state.name ? <FootprintArrayForm /> : null;
}

function FootprintArrayForm() {
  const state = useFpState();
  const dispatch = useFpDispatch();
  const api = useFpApi();
  const units = useStudioState().units;
  const fp = state.footprint!;
  const name = state.name!;
  const ids = useMemo(() => fpArrayIds(fp, state.selection), [fp, state.selection]);
  const origin = useMemo(() => arrayOrigin(ids, (id) => fpItemPosition(fp, id), (id) => fpItemBounds(fp, id)), [fp, ids]);
  const selected = useMemo(() => new Set(ids), [ids]);
  const host: ArrayHost = {
    footprintEditor: true,
    itemCount: ids.length,
    units,
    origin,
    positionOf: (id) => fpItemPosition(fp, id),
    // `DIALOG_CREATE_ARRAY::TransferDataFromWindow` (the numbering entries) first, then the one verb
    submit: async (options) => {
      const made = fpArrayCmd(options, name, ids);
      if (!made.ok) return made.error;
      return (await api.cmd(made.cmd)) ? null : undefined;
    },
    close: () => dispatch({ type: "SET_CREATE_ARRAY_OPEN", open: false }),
    padNumbers: (options) => previewPadNumbers(options, fp.pads, selected, 12),
  };
  return <CreateArrayForm host={host} />;
}
