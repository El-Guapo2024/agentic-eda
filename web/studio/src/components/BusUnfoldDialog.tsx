// `C` (SCH_LINE_WIRE_BUS_TOOL::UnfoldBus): the member nets of the bus under the cursor, as `BUS_UNFOLD_MENU`
// lists them in a popup at the cursor -- a small modal list here (the studio has no popup-menu primitive).
// Picking one arms the wire tool from the far end of a new bus entry: the entry (rooted on the bus at the
// cursor, `doUnfoldBus`) leans toward the cursor until the first click, which places the member's label
// (`doDrawSegments`, kicad-port/schBusUnfold.ts); the entry, the wire and the label are committed together
// when the wire is finished (SchematicView.tsx's `commitDrawn`).
import { useStudioDispatch, useStudioState } from "../state/store";
import { BUS_ENTRY_SIZE_NM as ENTRY_SIZE_NM, unfoldLabelSpin } from "../kicad-port/schBusUnfold";
import { busUnfoldMenu, type UnfoldEntry } from "../kicad-port/busUnfoldMenu";

export function BusUnfoldDialog() {
  const state = useStudioState();
  const dispatch = useStudioDispatch();
  const picker = state.busUnfoldPicker;
  if (!picker) return null;
  const close = () => dispatch({ type: "SET_BUS_UNFOLD_PICKER", picker: null });

  const choose = (net: string) => {
    const size: [number, number] = [ENTRY_SIZE_NM, ENTRY_SIZE_NM];
    const start: [number, number] = [picker.entryAt[0] + size[0], picker.entryAt[1] + size[1]];
    dispatch({ type: "SET_BUS_UNFOLD_PICKER", picker: null });
    // `m_busUnfold.label->SetSpinStyle( busUnfoldPersistentSettings.label_spin_style )`
    dispatch({ type: "SET_BUS_UNFOLD", unfold: { net, entryAt: picker.entryAt, size, spin: unfoldLabelSpin() } });
    // `startSegments( aCommit, LAYER_WIRE, m_busUnfold.entry->GetEnd() )`: the wire starts at the entry's far end.
    dispatch({ type: "SET_ACTIVE_TOOL", tool: "wire" });
    dispatch({ type: "SET_DRAW_STATE", draw: { kind: "wire", pts: [start] } });
  };

  return (
    <div className="dialog-backdrop" onClick={close}>
      <div className="dialog" style={{ width: 300 }} onClick={(e) => e.stopPropagation()}>
        <div className="dialog-header">
          <span>Unfold from Bus {picker.bus}</span>
        </div>
        <div className="dialog-body">
          <div style={{ fontSize: 11, opacity: 0.7, marginBottom: 6 }}>Choose the net to break out of the bus:</div>
          <div style={{ maxHeight: 240, overflowY: "auto", display: "flex", flexDirection: "column", gap: 2 }}>
            {/* `BUS_UNFOLD_MENU::update`: a member that is a bus itself (a vector or an alias in a group) is a submenu of its own members (UI-LEFT S19), and the
                members of a differential pair are a submenu of their own (UI-LEFT-2 SS18, kicad-port/busUnfoldMenu.ts) */}
            <UnfoldEntries entries={busUnfoldMenu(picker.tree ?? picker.members.map((name) => ({ name, members: [] as string[] })))} choose={choose} />
          </div>
        </div>
        <div className="dialog-footer">
          <button onClick={close}>Cancel</button>
        </div>
      </div>
    </div>
  );
}

/** The menu's entries: an item is a button that unfolds its member (the label is `PrintBusForUI` of its name), a submenu an open list of its own entries. */
function UnfoldEntries({ entries, choose }: { entries: readonly UnfoldEntry[]; choose: (net: string) => void }) {
  return (
    <>
      {entries.map((e) =>
        e.kind === "item" ? (
          <button key={`i:${e.name}`} data-unfold-member={e.name} style={{ textAlign: "left" }} onClick={() => choose(e.name)}>
            {e.label}
          </button>
        ) : (
          <details key={`m:${e.title}`} data-unfold-submenu={e.title} open>
            <summary style={{ cursor: "pointer" }}>{e.label}</summary>
            <div style={{ display: "flex", flexDirection: "column", gap: 2, marginLeft: 14 }}>
              <UnfoldEntries entries={e.entries} choose={choose} />
            </div>
          </details>
        )
      )}
    </>
  );
}
