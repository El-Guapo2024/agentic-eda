// common.SuiteControl.listHotKeys ("List Hotkeys...", Ctrl+F1): `DIALOG_LIST_HOTKEYS` (common/dialogs/dialog_hotkey_list.cpp at KiCad 8303b2ad), "Hotkey
// List" -- the Hotkeys page's panel read-only (`PANEL_HOTKEYS_EDITOR( aParent, this, true )`: "Command" for the first column, no editing), with every action's
// current keys (`ReadHotKeyConfigIntoActions`: the person's user.hotkeys), the filter, and the panel's own buttons; OK and Cancel sit at the right of them
// (`GetBottomSizer()->Add( sdb_sizer )`). As in KiCad, OK writes the list (`TransferDataFromWindow` -> `WriteHotKeyConfig`), so hotkeys imported here are kept.
import { useStudioDispatch, useStudioState } from "../state/store";
import { useHotkeysPanel } from "./HotkeysPage";

export function HotkeysDialog() {
  const state = useStudioState();
  const dispatch = useStudioDispatch();
  const open = state.hotkeysDialogOpen;
  const close = () => dispatch({ type: "SET_HOTKEYS_DIALOG_OPEN", open: false });
  const panel = useHotkeysPanel(
    open,
    true,
    <>
      <button onClick={close}>Cancel</button>
      <button
        className="primary"
        onClick={() => {
          panel.commit();
          close();
        }}
      >
        OK
      </button>
    </>
  );

  if (!open) return null;
  return (
    <div className="dialog-backdrop" onClick={close}>
      <div className="dialog" role="dialog" aria-label="Hotkey List" data-testid="hotkey-list-dialog" style={{ width: 900, maxWidth: "95vw" }} onClick={(e) => e.stopPropagation()}>
        <div className="dialog-header">
          <span>Hotkey List</span>
        </div>
        <div className="dialog-body" style={{ paddingTop: 8 }}>
          {panel.element}
        </div>
      </div>
    </div>
  );
}
