// A click of `DRAWING_TOOL::DrawZone` (pcbnew/tools/drawing_tool.cpp at 8303b2ad), which draws zones, rule areas, cutouts, similar zones, via stitchings and graphic
// polygons, over `POLYGON_GEOM_MANAGER` (kicad-port/polygonGeom.ts, the port the schematic's rule area runs on):
//
//   LEADER_MODE angleSnap = GetAngleSnapMode();
//   if( evt->Modifier( MD_CTRL ) ) angleSnap = LEADER_MODE::DIRECT;
//   ...
//   polyGeomMgr.SetLeaderMode( angleSnap );
//   ...
//   const bool endPolygon = evt->IsDblClick( BUT_LEFT ) || evt->IsAction( &PCB_ACTIONS::closeOutline ) || polyGeomMgr.NewPointClosesOutline( cursorPos );
//   if( endPolygon ) { polyGeomMgr.SetFinished(); polyGeomMgr.Reset(); cleanup(); m_frame->PopTool( aEvent ); break; }
//   else if( polyGeomMgr.AddPoint( cursorPos ) ) ...
//
// A click on the first corner closes the outline; in the 45- and 90-degree line modes a click locks in the leader's bend and its end (two corners).
import { addPoint, finalOutline, newPointClosesOutline, setCursorPosition, type LeaderMode, type PolyGeom, type Pt } from "./polygonGeom";
import type { BoardState, Cmd, RuleAreaFields, Zone, ZoneDefaults, ZoneSettingsFields } from "../api/types";
import type { ZoneDrawMode } from "./pcbParityState";
import { padById } from "./pcbItems";
import { zoneIsOnCopper } from "./zoneLayers";
import { isCopper as isCopperLayerName } from "./layerPresets";

export type AngleSnapMode = "direct" | "90" | "45";

/** `GetAngleSnapMode()`, `LEADER_MODE::DIRECT` while Ctrl is held. */
export function leaderModeOf(mode: AngleSnapMode | undefined, ctrl: boolean): LeaderMode {
  if (ctrl || mode === "direct") return "direct";
  return mode === "90" ? "deg90" : "deg45";
}

const geomOf = (pts: readonly Pt[], mode: LeaderMode): PolyGeom => ({ mode, locked: pts.map((p): Pt => [p[0], p[1]]), leader: [], loop: [] });

/** What a click at `at` does to the corners `pts`: closes the outline (on the first corner), or locks in the next corner(s). */
export function zoneClick(pts: readonly Pt[], at: Pt, mode: LeaderMode): { finish: true } | { finish: false; pts: [number, number][] } {
  const g = setCursorPosition(geomOf(pts, mode), at);
  if (newPointClosesOutline(g, at)) return { finish: true };
  return { finish: false, pts: addPoint(g, at).locked.map((p): [number, number] => [p[0], p[1]]) };
}

/** `m_leaderPts` for the cursor at `cursor`: the rubber band from the last corner, bent in the 45- and 90-degree modes. Empty with no corner. */
export function zoneLeader(pts: readonly Pt[], cursor: Pt, mode: LeaderMode): [number, number][] {
  return setCursorPosition(geomOf(pts, mode), cursor).leader.map((p): [number, number] => [p[0], p[1]]);
}

/** `m_loopPts` for the cursor at `cursor`: the path closing the outline from the cursor back to the first corner, bent in the 45- and 90-degree modes
 * (`build45DegLeader( aEndPoint, m_lockedPoints.Reverse() ).Reverse()`); empty in the free mode, where the preview closes straight. */
export function zoneLoop(pts: readonly Pt[], cursor: Pt, mode: LeaderMode): [number, number][] {
  return setCursorPosition(geomOf(pts, mode), cursor).loop.map((p): [number, number] => [p[0], p[1]]);
}

/**
 * The outline a zone, rule area, cutout, similar zone, via stitching or graphic polygon is made with when it is finished with the cursor at `cursor`:
 * `ZONE_CREATE_HELPER::OnComplete` (pcbnew/tools/zone_create_helper.cpp) -- the locked corners, then in the 45- and 90-degree modes the leader's and the
 * loop's corners ("we may have intermediate points in the leader that should be included as they are shown in the preview"), `Simplify( true )`, and the
 * start point dropped when it lies on the line between its neighbours (polygonGeom.ts `finalOutline`). Null with fewer than three corners ("just scrap the
 * zone in progress"). Round 5's A5-5.
 */
export function zoneFinish(pts: readonly Pt[], cursor: Pt, mode: LeaderMode): [number, number][] | null {
  if (pts.length < 3) return null;
  const out = finalOutline(setCursorPosition(geomOf(pts, mode), cursor));
  return out ? out.map((p): [number, number] => [p[0], p[1]]) : null;
}

// ------------------------------------------------------------------------------------------------ the order of the zone tools

/**
 * The settings a zone being drawn with Add Filled Zone or Add Rule Area takes (`ZONE_SETTINGS zoneInfo`, then `zoneInfo.ExportSetting( *newZone )`): what the zone
 * dialog was OK'd with at the first corner (`ZONE_CREATE_HELPER::createNewZone`), or again with Properties (E) while drawing.
 */
export interface NewZoneSettings {
  /** The zone's net ("" = `<no net>`, net code 0). */
  net: string;
  /** `m_Layers`, the first one being the zone's layer. */
  layers: string[];
  name: string;
  locked: boolean;
  settings: ZoneSettingsFields & RuleAreaFields;
}

/**
 * `ZONE_CREATE_HELPER::setUniquePriority`: "By default, new zones get the first unused priority" -- the lowest priority no copper zone of the board that is not a
 * teardrop and not a rule area holds.
 */
export function uniqueZonePriority(zones: readonly Pick<Zone, "priority" | "teardrop" | "is_rule_area" | "layer" | "layers">[]): number {
  const used = new Set<number>();
  for (const z of zones) if (!z.teardrop && !z.is_rule_area && zoneIsOnCopper(z)) used.add(z.priority);
  // `for( unsigned exist_priority : priorities ) { if( priority != exist_priority ) break; ++priority; }` over the sorted set.
  let priority = 0;
  for (const p of [...used].sort((a, b) => a - b)) {
    if (priority !== p) break;
    priority++;
  }
  return priority;
}

/**
 * `createNewZone`'s net: the first highlighted net (`board->GetHighLightNetCodes()`), else the net of the first selected item when it is a connected item (a pad, a
 * track, a via, a zone: `BOARD_CONNECTED_ITEM`), else none (`-1`, which the dialog shows as `<no net>`: `SetSelectedNetcode( std::max( 0, m_Netcode ) )`).
 */
export function newZoneNet(board: BoardState, highlighted: readonly string[], selection: readonly string[]): string {
  if (highlighted.length > 0 && highlighted[0]) return highlighted[0];
  const first = selection[0];
  if (first === undefined) return "";
  const rt = board.routing;
  const net = rt?.tracks.find((t) => t.id === first)?.net ?? rt?.vias.find((v) => v.id === first)?.net ?? rt?.zones.find((z) => z.id === first)?.net ?? padById(board, first)?.pad.net;
  return net ?? "";
}

/**
 * What the zone dialog opens on for a new zone (`createNewZone`): the board's default zone settings (`GetDefaultZoneSettings()`; here `ZONE_SETTINGS`' own, as Board
 * Setup has no Zones page and Zone Properties' OK does not make them the next zone's: GAPS 13), on the one layer the tool is drawing on (`m_Layers.reset().set(
 * m_params.m_layer )`, "Do not copy over layer properties"), the net above (none for a rule area), a rule area when the tool is Add Rule Area (`SetIsRuleArea( m_params.m_keepout )`),
 * and, on a copper layer, the first unused priority (`setUniquePriority`).
 */
/**
 * `ZONE_SETTINGS::ZONE_SETTINGS()` (pcbnew/zone_settings.cpp): `SetDoNotAllowZoneFills( false ); SetDoNotAllowVias( true ); SetDoNotAllowTracks( true );
 * SetDoNotAllowPads( true ); SetDoNotAllowFootprints( false );` -- what a new rule area keeps out (Add Rule Area's dialog opens on it; found by task-u1-fpedit,
 * UI-LEFT P29).
 */
export const ZONE_SETTINGS_KEEPOUTS: Pick<RuleAreaFields, "keepout_tracks" | "keepout_vias" | "keepout_pads" | "keepout_copper_pour" | "keepout_footprints"> = {
  keepout_tracks: true,
  keepout_vias: true,
  keepout_pads: true,
  keepout_copper_pour: false,
  keepout_footprints: false,
};

export function newZoneInitial(
  board: BoardState,
  opts: { layer: string; ruleArea: boolean; highlighted: readonly string[]; selection: readonly string[]; defaults: ZoneSettingsFields & RuleAreaFields }
): NewZoneSettings {
  const sticky = board.drawings?.zone_defaults;
  // `GetDefaultZoneSettings()` (UI-LEFT-2 PP10): the last OK'd zone's settings over `ZONE_SETTINGS`' own; a rule area's keep-outs are those of the last rule area
  // (a copper zone here holds none of its own, where KiCad's carries `ZONE_SETTINGS()`' tracks, vias and pads)
  const own = stickySettings(sticky);
  const keepouts = sticky?.is_rule_area ? {} : ZONE_SETTINGS_KEEPOUTS;
  const settings: ZoneSettingsFields & RuleAreaFields = { ...opts.defaults, ...own, ...(opts.ruleArea ? keepouts : {}), is_rule_area: opts.ruleArea };
  if (isCopperLayerName(opts.layer)) settings.priority = uniqueZonePriority(board.routing?.zones ?? []);
  // A rule area takes no net (`ExportSetting`: `if( !m_isRuleArea ) aTarget.SetNetCode( m_Netcode )`); the name is the defaults' (`m_Name`).
  return { net: opts.ruleArea ? "" : newZoneNet(board, opts.highlighted, opts.selection), layers: [opts.layer], name: opts.ruleArea ? "" : (sticky?.name ?? ""), locked: false, settings };
}

/** The keys of `ZONE_SETTINGS` a zone's dialog edits that the defaults carry over (`createNewZone` resets the layers and their properties, the net and the rule area flag). */
const STICKY_KEYS = [
  "clearance", "min_thickness", "thermal_gap", "thermal_spoke_width", "pad_connection", "priority", "island_removal_mode", "min_island_area", "fill_mode", "hatch_thickness",
  "hatch_gap", "hatch_orientation_mdeg", "hatch_smoothing_level", "hatch_smoothing_value", "hatch_hole_min_area", "hatch_border_algorithm", "smoothing", "corner_radius",
  "border_style", "border_hatch_pitch", "keepout_tracks", "keepout_vias", "keepout_pads", "keepout_copper_pour", "keepout_footprints",
] as const;

/** What of the board's default zone settings a new zone takes (`GetDefaultZoneSettings()` less the layers, layer properties, net and rule-area flag). */
export function stickySettings(d: ZoneDefaults | undefined): Partial<ZoneSettingsFields & RuleAreaFields> {
  const out: Record<string, unknown> = {};
  if (!d) return out;
  for (const k of STICKY_KEYS) if (d[k] !== undefined && d[k] !== null) out[k] = d[k];
  return out as Partial<ZoneSettingsFields & RuleAreaFields>;
}

/**
 * `PCB_EDIT_FRAME::Edit_Zone_Params`' `SetDefaultZoneSettings( zoneInfo )` after Zone Properties' OK (on a zone of the board, or Properties during a draw): the
 * zone's settings, layers and name (`ZONE_SETTINGS << *aZone`), the net restored to none (`m_Netcode = NETINFO_LIST::ORPHANED`). 8303b2ad; 25ec2335 moved the
 * defaults to Board Setup's Zones page.
 */
export function zoneDefaultsCmd(form: Partial<ZoneSettingsFields & RuleAreaFields>, layers: readonly string[], name: string): Cmd {
  const zone: Record<string, unknown> = { net: "", layer: layers[0] ?? "F.Cu", outline: [] };
  for (const k of STICKY_KEYS) if (form[k] !== undefined && form[k] !== null) zone[k] = form[k];
  zone.is_rule_area = !!form.is_rule_area;
  if (layers.length > 1) zone.layers = [...layers];
  if (name) zone.name = name;
  return { op: "set_zone_defaults", zone: zone as ZoneDefaults & { net: string; layer: string; outline: [] } };
}

/**
 * `ZONE_CREATE_HELPER::createZoneFromExisting( *m_params.m_sourceZone )`: the zone Add Zone Cutout and Add Similar Zone draw, which Properties (E) during the draw
 * opens (`OnEditItemRequest( zoneTool.GetZone() )`, UI-LEFT-2 PP10): the source's settings (`ZONE_SETTINGS << aSrcZone`), layers, net and name.
 */
export function zoneFromExisting(source: Zone, locked: boolean): NewZoneSettings {
  const settings = { ...source } as unknown as ZoneSettingsFields & RuleAreaFields;
  return { net: source.is_rule_area ? "" : source.net, layers: source.layers && source.layers.length > 0 ? [...source.layers] : [source.layer], name: source.name ?? "", locked, settings };
}

/**
 * What the first corner of `DRAWING_TOOL::DrawZone` does (`POLYGON_GEOM_MANAGER::AddPoint` -> `ZONE_CREATE_HELPER::OnFirstPoint`):
 * - `"ask"`: Add Filled Zone and Add Rule Area (`ZONE_MODE::ADD`, no source zone) ask for the zone's settings first (`createNewZone`: `InvokeCopperZonesEditor`,
 *   `InvokeRuleAreaEditor` or `InvokeNonCopperZonesEditor`, by the layer); the corner is locked in only when the dialog is OK'd, and Cancel leaves the tool armed with
 *   no corner (`OnFirstPoint` returns false, so `AddPoint` does, and `started` stays false: the next click asks again, Escape leaves the tool);
 * - `"draw"`: Add Zone Cutout and Add Similar Zone take the source zone's settings (`createZoneFromExisting`: `ZONE_SETTINGS << aSrcZone`), and KiCad 10's via
 *   stitching area its own dialog at the end -- the corner is locked in at once, no dialog.
 */
export function zoneFirstCorner(mode: ZoneDrawMode | null): "ask" | "draw" {
  return mode ? "draw" : "ask";
}

/**
 * How a zone outline that was finished is committed (`ZONE_CREATE_HELPER::commitZone`, `m_params.m_mode`):
 * - `cutout`: subtracted from the source zone (`performZoneCutout`, "Add Zone Cutout"; a source that is gone, nothing);
 * - `similar`: a new zone with the source's settings and layers ("Draw Zone");
 * - `stitch`: KiCad 10's via stitching area (25ec2335's `ZONE_MODE::STITCH`);
 * - `add`: a new zone with the settings the dialog gave at the first corner ("Draw Zone", then `selectItem` on it);
 * - `ask`: a plain draw that has no settings (a draw state from before this order was ported): the dialog after the outline, as the studio did.
 * The tool is left after any of them (`endPolygon`: `cleanup(); m_frame->PopTool( aEvent ); break;`).
 */
export type ZoneCommit =
  | { kind: "cutout"; sourceId: string }
  /** `zone`: the settings Properties (E) gave the zone being drawn (UI-LEFT-2 PP10), over the source's. */
  | { kind: "similar"; sourceId: string; zone?: NewZoneSettings }
  | { kind: "stitch" }
  | { kind: "add"; zone: NewZoneSettings }
  | { kind: "ask" }
  | { kind: "none" };

export function zoneCommitPlan(mode: ZoneDrawMode | null, sourceExists: boolean, settings: NewZoneSettings | undefined): ZoneCommit {
  if (mode?.mode === "cutout") return sourceExists ? { kind: "cutout", sourceId: mode.sourceId } : { kind: "none" };
  if (mode?.mode === "similar") return sourceExists ? { kind: "similar", sourceId: mode.sourceId, ...(settings ? { zone: settings } : {}) } : { kind: "none" };
  if (mode?.mode === "stitch") return { kind: "stitch" };
  return settings ? { kind: "add", zone: settings } : { kind: "ask" };
}
