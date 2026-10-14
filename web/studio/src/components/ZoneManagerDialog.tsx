// pcbnew.Control.zonesManager ("Zone Manager...") -- port of pcbnew/zone_manager/dialog_zone_manager.cpp, model_zones_overview.cpp and zone_preview_notebook.cpp:
// every copper zone in one list, highest priority first, that can be filtered by name/net text and by layer, whose rows move up/down/to the top/to the bottom (the zones
// trade priorities, `MODEL_ZONES_OVERVIEW::MoveZoneIndex`) and whose priorities are written back together on OK, like `ZONE_SETTINGS_BAG` does: consecutive, the top zone the
// highest. "Auto-Assign Priorities" works the order out from what the overlaps hold (`AutoAssignZonePriorities`, `eda_ops::zone_priority`) and stages it. A preview on the right
// shows the selected zone one layer per tab (`ZONE_PREVIEW_NOTEBOOK`, components/ZonePreview.tsx). Moves are staged in the dialog; Cancel drops them. "Refill zones" runs
// KiCad's filler (kicad-cli) when OK is pressed. "Edit zone..." applies them and opens the Zone Properties dialog on the selected zone (the properties panel KiCad embeds here is
// that dialog in this studio).
//
// Not ported: "Update Displayed Zones" (a fill of the dialog's staged copies; the preview shows the fills the board holds, and the staged priorities take effect with OK and a fill).
import { useEffect, useMemo, useState } from "react";
import type { Cmd } from "../api/types";
import { fetchAutoZonePriorities } from "../api/client";
import { ZonePreview } from "./ZonePreview";
import { filterZones, moveZone, rankPriorities, zoneOrder, type ZoneMove } from "../kicad-port/boardControl";
import { useStudioApi, useStudioDispatch, useStudioState } from "../state/store";
import { zoneIsOnCopper, zoneLayers } from "../kicad-port/zoneLayers";

export function ZoneManagerDialog() {
  const state = useStudioState();
  const dispatch = useStudioDispatch();
  const api = useStudioApi();
  const open = state.bcx.zoneManagerOpen;
  // `ZONE_SETTINGS_BAG`: the copper pours -- no rule area, no teardrop, no zone without a copper layer.
  const zones = useMemo(() => (state.board?.routing?.zones ?? []).filter((z) => !z.is_rule_area && !z.teardrop && zoneIsOnCopper(z)), [state.board]);
  const [staged, setStaged] = useState<string[]>([]);
  const [selected, setSelected] = useState<string | null>(null);
  const [text, setText] = useState("");
  const [byName, setByName] = useState(true);
  const [byNet, setByNet] = useState(true);
  const [layer, setLayer] = useState("");
  // The priorities Auto-Assign staged (KiCad's can leave several zones with one priority; the rows are then in that order), until a row is moved by hand.
  const [exact, setExact] = useState<Record<string, number> | null>(null);
  const [refill, setRefill] = useState(false);
  const [note, setNote] = useState("");

  useEffect(() => {
    if (!open) return;
    setStaged(zoneOrder(zones));
    setText("");
    setLayer("");
    setSelected(null);
    setExact(null);
    setRefill(false);
    setNote("");
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open]);

  if (!open) return null;

  // The staged order, kept to the zones that still exist, with any that appeared since appended in priority order.
  const ids = new Set(zones.map((z) => z.id));
  const order = [...staged.filter((id) => ids.has(id)), ...zoneOrder(zones.filter((z) => !staged.includes(z.id)))];
  const byId = new Map(zones.map((z) => [z.id, z]));
  const rank = exact && zones.every((z) => exact[z.id] !== undefined) ? exact : rankPriorities(order);
  const shown = filterZones(order.map((id) => byId.get(id)!), text, byName, byNet, layer || null);
  const shownIds = shown.map((z) => z.id);
  const filledIds = new Set(state.zoneFill?.zones.filter((f) => f.filled).map((f) => f.id));
  const selectedZone = selected ? (byId.get(selected) ?? null) : null;
  const usedLayers = [...new Set(zones.flatMap((z) => zoneLayers(z)))];
  const changed = zones.filter((z) => rank[z.id] !== z.priority);

  const close = () => dispatch({ type: "BCX", patch: { zoneManagerOpen: false } });
  const move = (how: ZoneMove) => {
    if (!selected) return;
    setStaged(moveZone(order, shownIds, selected, how));
    setExact(null); // by hand: the rows take consecutive priorities again
  };
  // `OnAutoAssignClick`: the priorities `AutoAssignZonePriorities` gives, staged; nothing changes on the board until OK.
  const autoAssign = async () => {
    try {
      const got = await fetchAutoZonePriorities();
      if (!got.changed) {
        setNote("Auto-assign found nothing to change: the priorities are as they would be given.");
        return;
      }
      const priorities = Object.fromEntries(zones.map((z) => [z.id, got.priorities[z.id] ?? z.priority]));
      setExact(priorities);
      setStaged(zoneOrder(zones.map((z) => ({ id: z.id, priority: priorities[z.id]! }))));
      setNote("Priorities assigned from the overlaps; press OK to apply them.");
    } catch (e) {
      setNote(`Auto-assign failed: ${e instanceof Error ? e.message : String(e)}`);
    }
  };
  const apply = async (): Promise<boolean> => {
    // `*zone = *zoneClone`: each zone with a new priority takes it with every other setting as it is.
    const cmds: Cmd[] = changed.map((z) => ({ op: "edit_zone", ...z, priority: rank[z.id]! }));
    return cmds.length === 0 ? true : api.cmdBatch(cmds);
  };
  const ok = async () => {
    if (!(await apply())) return;
    close();
    if (refill) void api.fillZones(); // `GetRepourOnClose`: "Refill zones after changes made on board"
  };
  const edit = async () => {
    if (!selected || !(await apply())) return;
    close();
    dispatch({ type: "SET_ZONE_EDIT_ID", id: selected });
  };
  const pickRow = (id: string) => {
    setSelected(id);
    dispatch({ type: "SET_SELECTION", refs: [id] });
  };
  const row = selected ? shownIds.indexOf(selected) : -1;
  const arrow = (label: string, how: ZoneMove, enabled: boolean, title: string) => (
    <button disabled={!enabled} onClick={() => move(how)} title={title}>
      {label}
    </button>
  );

  return (
    <div className="dialog-backdrop" onClick={close}>
      <div className="dialog" style={{ width: 940 }} onClick={(e) => e.stopPropagation()}>
        <div className="dialog-header">Zone Manager</div>
        <div className="dialog-body" style={{ paddingTop: 10 }}>
          <div style={{ display: "flex", gap: 8, alignItems: "center", marginBottom: 8 }}>
            <input value={text} placeholder="Filter" onChange={(e) => setText(e.target.value)} style={{ flex: 1 }} />
            <label className="toggle">
              <input type="checkbox" checked={byName} onChange={(e) => setByName(e.target.checked)} /> Name
            </label>
            <label className="toggle">
              <input type="checkbox" checked={byNet} onChange={(e) => setByNet(e.target.checked)} /> Net
            </label>
            <span>Layer:</span>
            <select value={layer} onChange={(e) => setLayer(e.target.value)}>
              <option value="">All Layers</option>
              {usedLayers.map((l) => (
                <option key={l} value={l}>
                  {l}
                </option>
              ))}
            </select>
          </div>
          <div style={{ display: "flex", gap: 12 }}>
          <div style={{ display: "flex", gap: 8, flex: 1, minWidth: 0 }}>
            <div style={{ flex: 1, maxHeight: 280, overflowY: "auto", border: "1px solid var(--chrome-border, #444)" }}>
              <table style={{ width: "100%", borderCollapse: "collapse", fontSize: 12 }}>
                <thead>
                  <tr style={{ textAlign: "left", position: "sticky", top: 0, background: "var(--chrome-bg, #222)" }}>
                    <th style={{ padding: "3px 6px" }}>Name</th>
                    <th style={{ padding: "3px 6px" }}>Net</th>
                    <th style={{ padding: "3px 6px" }}>Layers</th>
                    <th style={{ padding: "3px 6px", textAlign: "right" }}>Priority</th>
                    <th style={{ padding: "3px 6px" }}>Fill</th>
                  </tr>
                </thead>
                <tbody>
                  {shown.map((z) => (
                    <tr key={z.id} onClick={() => pickRow(z.id)} style={{ cursor: "default", background: z.id === selected ? "var(--chrome-accent-dim, rgba(74,163,255,0.25))" : undefined }}>
                      <td style={{ padding: "2px 6px" }}>{z.id}</td>
                      <td style={{ padding: "2px 6px" }}>{z.net || "(no net)"}</td>
                      <td style={{ padding: "2px 6px" }}>{zoneLayers(z).join(", ")}</td>
                      <td style={{ padding: "2px 6px", textAlign: "right" }}>{rank[z.id]}</td>
                      <td style={{ padding: "2px 6px" }}>{filledIds.has(z.id) ? "filled" : ""}</td>
                    </tr>
                  ))}
                  {shown.length === 0 && (
                    <tr>
                      <td colSpan={5} style={{ padding: 10, color: "var(--chrome-text-dim)" }}>
                        {zones.length === 0 ? "There are no copper zones on this board." : "No zone matches the filter."}
                      </td>
                    </tr>
                  )}
                </tbody>
              </table>
            </div>
            <div style={{ display: "flex", flexDirection: "column", gap: 4 }} title="Top zone has the highest priority. When a zone is inside another zone, if its priority is higher, its outlines are removed from the other zone.">
              {arrow("Top", "top", row > 0, "Move to the top of the list: the highest priority")}
              {arrow("Up", "up", row > 0, "Move up one row, trading priorities with it")}
              {arrow("Down", "down", row >= 0 && row < shownIds.length - 1, "Move down one row, trading priorities with it")}
              {arrow("Bottom", "bottom", row >= 0 && row < shownIds.length - 1, "Move to the bottom of the list: the lowest priority")}
              <button disabled={zones.length < 2} onClick={() => void autoAssign()} data-testid="zone-auto-assign" title="Automatically assign zone priorities based on connectivity analysis of overlapping regions.">
                Auto-Assign
              </button>
            </div>
          </div>
          <ZonePreview zone={selectedZone} fills={state.zoneFill?.zones ?? []} outlinePolys={state.board?.outline_polys} outline={state.board?.outline ?? null} />
          </div>
          <div style={{ fontSize: 11, color: "var(--chrome-text-dim)", marginTop: 8 }} data-testid="zone-manager-note">
            {note || (changed.length > 0 ? `${changed.length} zone(s) will change priority.` : "Priorities are as the board has them.")}
          </div>
        </div>
        <div className="dialog-footer">
          <label className="toggle" style={{ marginRight: "auto" }} title="Refill zones after changes made on board">
            <input type="checkbox" checked={refill} onChange={(e) => setRefill(e.target.checked)} data-testid="zone-manager-refill" /> Refill zones
          </label>
          <button disabled={!selected} onClick={() => void edit()} title="Apply the priority changes and edit the selected zone's properties">
            Edit zone…
          </button>
          <button className="primary" onClick={() => void ok()}>
            OK
          </button>
          <button onClick={close}>Cancel</button>
        </div>
      </div>
    </div>
  );
}
