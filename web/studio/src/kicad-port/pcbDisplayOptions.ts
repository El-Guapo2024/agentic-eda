// Preferences > PCB Editor > Display Options' board settings (`PANEL_DISPLAY_OPTIONS`, pcbnew/dialogs/panel_display_options.cpp; `PCBNEW_SETTINGS::m_Display`,
// pcbnew/pcbnew_settings.cpp) and what the painter makes of them (UI-LEFT-3 P3-1, P3-6):
// - "Net names:" (`m_NetNames`, `pcb_display.net_names_mode`, default 3): the pads' net names (1, 3), the tracks' (2, 3) -- `PCB_PAINTER::draw( const PAD* )`,
//   `draw( const PCB_TRACK* )` and `renderNetNameForSegment`; a via's in its ring whenever it is not 0 (`draw( const PCB_VIA* )`);
// - "Clearance Outlines" > "Tracks:" (`m_TrackClearance`, `pcb_display.track_clearance_mode`, default "Show when routing w/ via clearance at end"): the board's
//   tracks' and vias' outlines on the active copper layer with "Show always" only (`IsClearanceLayer( aLayer ) && m_TrackClearance == SHOW_WITH_VIA_ALWAYS`), the
//   router's preview items' by the mode (`PNS_KICAD_IFACE::DisplayItem`);
// - "Show pad clearance" (`m_PadClearance`, `pcb_display.pad_clearance`, default on): the pads' outlines on the active copper layer.
// The level-of-detail tests are `VIEW_ITEM::lodScaleForThreshold` against the view's zoom (`VIEW::m_scale`, `GAL::m_zoomFactor`).

/** `m_NetNames`: 0 "Do not show", 1 "Show on pads", 2 "Show on tracks", 3 "Show on pads & tracks". */
export type NetNamesMode = 0 | 1 | 2 | 3;

/** `TRACK_CLEARANCE_MODE` (pcbnew/pcbnew_settings.h), in the order of the "Tracks:" choice (`clearanceModeMap`). */
export type TrackClearanceMode = "none" | "routing" | "routing_via" | "routing_editing" | "always";

/** The "Tracks:" choice's entries (`m_OptDisplayTracksClearanceChoices`), each with its mode. */
export const TRACK_CLEARANCE_CHOICES: ReadonlyArray<{ mode: TrackClearanceMode; label: string }> = [
  { mode: "none", label: "Do not show clearances" },
  { mode: "routing", label: "Show when routing" },
  { mode: "routing_via", label: "Show when routing w/ via clearance at end" },
  { mode: "routing_editing", label: "Show when routing and editing" },
  { mode: "always", label: "Show always" },
];

/** The "Net names:" choice's entries (`m_ShowNetNamesOptionChoices`). */
export const NET_NAMES_CHOICES: readonly string[] = ["Do not show", "Show on pads", "Show on tracks", "Show on pads & tracks"];

/** A fresh `PCBNEW_SETTINGS`' values (`PANEL_DISPLAY_OPTIONS::ResetPanel`). */
export const DEFAULT_PCB_DISPLAY: { netNames: NetNamesMode; trackClearance: TrackClearanceMode; padClearance: boolean } = {
  netNames: 3,
  trackClearance: "routing_via",
  padClearance: true,
};

/** `PARAM<int>( "pcb_display.net_names_mode", .., 3, 0, 3 )`: out of range is the default. */
export function sanitizeNetNamesMode(v: unknown, fallback: NetNamesMode = DEFAULT_PCB_DISPLAY.netNames): NetNamesMode {
  return v === 0 || v === 1 || v === 2 || v === 3 ? v : fallback;
}

export function sanitizeTrackClearanceMode(v: unknown, fallback: TrackClearanceMode = DEFAULT_PCB_DISPLAY.trackClearance): TrackClearanceMode {
  return TRACK_CLEARANCE_CHOICES.some((c) => c.mode === v) ? (v as TrackClearanceMode) : fallback;
}

/** `draw( const PAD* )`: "if( displayOpts->m_NetNames == 1 || displayOpts->m_NetNames == 3 ) netname = aPad->GetDisplayNetname();". */
export function netNamesOnPads(mode: NetNamesMode): boolean {
  return mode === 1 || mode === 3;
}

/** `draw( const PCB_TRACK* )`: "if( !pcbconfig() || pcbconfig()->m_Display.m_NetNames < 2 ) return;". */
export function netNamesOnTracks(mode: NetNamesMode): boolean {
  return mode >= 2;
}

/** `draw( const PCB_VIA* )`: `showNets = m_NetNames != 0 && !aVia->GetNetname().empty()`. */
export function netNamesOnVias(mode: NetNamesMode): boolean {
  return mode !== 0;
}

/** The board's tracks, arcs and vias draw their clearance outlines only with "Show always" (`m_TrackClearance == SHOW_WITH_VIA_ALWAYS`). */
export function boardShowsTrackClearance(mode: TrackClearanceMode): boolean {
  return mode === "always";
}

/**
 * `PNS_KICAD_IFACE::DisplayItem`'s `ShowClearance`: whether a preview item of the router shows its clearance -- `kind` a track (a segment, an arc, the
 * routing head) or a via, `edit` an item of a drag (`aEdit`) rather than of a route.
 */
export function routerShowsClearance(mode: TrackClearanceMode, kind: "track" | "via", edit: boolean): boolean {
  switch (mode) {
    case "always":
    case "routing_editing":
      return true;
    case "routing_via":
      return !edit;
    case "routing":
      return kind === "track" && !edit;
    default:
      return false;
  }
}

/** GAL's world scale at a zoom of 1, pixels per nanometre: `m_screenDPI` (`ADVANCED_CFG::m_ScreenDPI`, 91) times `m_worldUnitLength` (1 nm in inches). */
export const GAL_PX_PER_NM_AT_ZOOM_1 = (91 * 1e-9) / 0.0254;

/** `VIEW::m_scale` of a view drawing `pxPerNm` pixels per nanometre. */
export function kicadZoom(pxPerNm: number): number {
  return pxPerNm / GAL_PX_PER_NM_AT_ZOOM_1;
}

/** `lodScaleForThreshold( aView, aWhatIu, aThresholdIu ) < m_scale` (`VIEW::draw`'s `itemLOD < view->m_scale`): is the item drawn at this zoom? */
export function lodShown(pxPerNm: number, whatNm: number, thresholdNm: number): boolean {
  if (whatNm <= 0) return false;
  return thresholdNm / whatNm < kicadZoom(pxPerNm);
}

/** `PrintableCharCount` (common/string_utils.cpp): the characters of `s` drawn, the markup of `~{..}`, `^{..}`, `_{..}` and tabs left out. */
export function printableCharCount(s: string): number {
  let count = 0;
  let overbar = -1;
  let superSub = -1;
  let nesting = 0;
  const chars = [...s];
  for (let i = 0; i < chars.length; i++) {
    const c = chars[i]!;
    if (c === "\t") continue;
    if ((c === "^" || c === "_") && superSub === -1 && chars[i + 1] === "{") {
      i++;
      superSub = nesting;
      nesting++;
      continue;
    }
    if (c === "~" && overbar === -1 && chars[i + 1] === "{") {
      i++;
      overbar = nesting;
      nesting++;
      continue;
    }
    if (c === "{") nesting++;
    else if (c === "}") {
      if (nesting > 0) nesting--;
      if (nesting === superSub) {
        superSub = -1;
        continue;
      }
      if (nesting === overbar) {
        overbar = -1;
        continue;
      }
    }
    count++;
  }
  return count;
}

/** `NETINFO_ITEM::GetDisplayNetname`: the short name (after the last `/` of a hierarchical name), unescaped by the caller. */
export function shortNetname(net: string): string {
  const i = net.lastIndexOf("/");
  return i >= 0 ? net.slice(i + 1) : net;
}

/** One net name drawn along a track: its centre, the canvas rotation (radians, y down), the glyph size and the pen. */
export interface NetNameLabel {
  x: number;
  y: number;
  angleRad: number;
  sizeNm: number;
  penNm: number;
}

/** A box of the board, nanometres. */
export interface ViewBox {
  x0: number;
  y0: number;
  x1: number;
  y1: number;
}

/**
 * `PCB_TRACK::ViewGetLOD( NETNAMES layer )` and `PCB_PAINTER::renderNetNameForSegment`: where a straight track's net name goes.
 * - the name is drawn only when the track is at least as long as its width times the name's length ("Check if the track is long enough to have a netname
 *   displayed"), some of it is in view (`ClipLine`), and the zoom gives it 4 mm of width (`lodScaleForThreshold( aView, m_width, mmToIU( 4.0 ) )`);
 * - along a horizontal track it reads left to right, along a vertical one bottom to top (`ANGLE_VERTICAL`), else along the track turned into -90 .. 90 degrees
 *   (`textOrientation = -EDA_ANGLE( segV ); textOrientation.Normalize90()`);
 * - it is repeated once per view's width (height; diagonal) of track, `num_names` evenly spaced, each one drawn only when its centre is in view;
 * - glyphs of 0.55 of the width (`SetGlyphSize( textSize * 0.55 )`), the pen a twelfth of the width.
 */
export function trackNetNameLabels(a: readonly [number, number], b: readonly [number, number], width: number, name: string, view: ViewBox, pxPerNm: number): NetNameLabel[] {
  if (!name || width <= 0) return [];
  const dx = b[0] - a[0];
  const dy = b[1] - a[1];
  const lenSq = dx * dx + dy * dy;
  // `GetDisplayNetname().size() * GetWidth()` (`wxString::size`, UTF-16 units -- the characters for anything a net name holds)
  const minLen = width * name.length;
  if (lenSq < minLen * minLen) return [];
  if (!clipsBox(a, b, view)) return [];
  if (!lodShown(pxPerNm, width, 4_000_000)) return [];
  const len = Math.sqrt(lenSq);
  const vw = view.x1 - view.x0;
  const vh = view.y1 - view.y0;
  let angleDeg: number;
  let names = 1;
  if (dy === 0) {
    angleDeg = 0;
    names = Math.max(names, Math.round(len / vw));
  } else if (dx === 0) {
    angleDeg = 90;
    names = Math.max(names, Math.round(len / vh));
  } else {
    angleDeg = -((Math.atan2(dy, dx) * 180) / Math.PI);
    while (angleDeg < -90) angleDeg += 180;
    while (angleDeg > 90) angleDeg -= 180;
    names = Math.max(names, Math.round(len / (Math.SQRT2 * Math.min(vw, vh))));
  }
  const out: NetNameLabel[] = [];
  const divisions = names + 1;
  for (let i = 1; i < divisions; i++) {
    const x = a[0] + (dx * i) / divisions;
    const y = a[1] + (dy * i) / divisions;
    if (x < view.x0 || x > view.x1 || y < view.y0 || y > view.y1) continue;
    // a KiCad angle turns counter-clockwise on a y-down board: the canvas turns by its negative
    out.push({ x, y, angleRad: angleDeg === 0 ? 0 : (-angleDeg * Math.PI) / 180, sizeNm: width * 0.55, penNm: width / 12 });
  }
  return out;
}

/** Does the segment `a`-`b` cross the box (`ClipLine` leaves something of it)? */
function clipsBox(a: readonly [number, number], b: readonly [number, number], v: ViewBox): boolean {
  // Liang-Barsky
  let t0 = 0;
  let t1 = 1;
  const dx = b[0] - a[0];
  const dy = b[1] - a[1];
  const edges: [number, number][] = [
    [-dx, a[0] - v.x0],
    [dx, v.x1 - a[0]],
    [-dy, a[1] - v.y0],
    [dy, v.y1 - a[1]],
  ];
  for (const [p, q] of edges) {
    if (p === 0) {
      if (q < 0) return false;
      continue;
    }
    const r = q / p;
    if (p < 0) {
      if (r > t1) return false;
      if (r > t0) t0 = r;
    } else {
      if (r < t0) return false;
      if (r < t1) t1 = r;
    }
  }
  return t1 > t0 || (dx === 0 && dy === 0);
}

/** `PCB_RENDER_SETTINGS::MAX_FONT_SIZE`: 10 mm. */
export const MAX_FONT_SIZE_NM = 10_000_000;

/** What `draw( const PCB_VIA*, NETNAMES layer )` writes in a via: the net name and, for a via that is not through, its layer numbers ("1-2"), each line's offset from the
 * centre (y down), the glyph size and the pen. */
export interface ViaNetNameLayout {
  lines: { text: string; dy: number }[];
  sizeNm: number;
  penNm: number;
}

/**
 * The via's net name and layer pair (`PCB_PAINTER::draw( const PCB_VIA* )`'s description layer): the size is the via's width (at most `MAX_FONT_SIZE`), the
 * text `1.5 * size / max( PrintableCharCount( netname ), 6 or 3 )` but at most the size, times 0.75 "to handle interline, pen size"; with both lines the net name is
 * half a line (`tsize * 1.3`) below the centre and the layers a line above it; the pen a tenth of the size. `layers` is the top and bottom layer numbers (1 ..
 * copper layer count) of a blind, buried or micro via, null for a through one. Null when there is nothing to write.
 */
export function viaNetNameLayout(widthNm: number, netname: string, showNets: boolean, layers: readonly [number, number] | null): ViaNetNameLayout | null {
  const nets = showNets && netname !== "";
  if (!nets && !layers) return null;
  const size = Math.min(widthNm, MAX_FONT_SIZE_NM);
  const minChars = layers ? 6 : 3;
  let tsize = (1.5 * size) / Math.max(printableCharCount(netname), minChars);
  tsize = Math.min(tsize, size) * 0.75;
  let y = 0;
  if (layers && nets) y += (tsize * 1.3) / 2;
  const lines: { text: string; dy: number }[] = [];
  if (nets) lines.push({ text: netname, dy: y });
  if (layers) {
    if (nets) y -= tsize * 1.3;
    lines.push({ text: `${layers[0]}-${layers[1]}`, dy: y });
  }
  return { lines, sizeNm: tsize, penNm: tsize / 10 };
}
