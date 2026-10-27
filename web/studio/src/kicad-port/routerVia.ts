// The router's via commands (pcbnew/router/router_tool.cpp at 8303b2ad): the `static const TOOL_ACTION`s Place Through / Blind/Buried / Micro Via
// and their "Select Layer and Place ..." variants, all `Go( &ROUTER_TOOL::onViaCommand, .. )`, which hand the action's `VIA_ACTION_FLAGS` to
// `ROUTER_TOOL::handleLayerSwitch( aEvent, true )`. What is here is the part of `handleLayerSwitch` that decides -- the via's type, the layer
// pair it is placed on, the layer the route goes on on (`switchLayerOnViaPlacement`), the via's size, and the refusals -- as plain functions
// the action handlers (actions/routerViaActions.ts) call and the unit tests check. The router itself (crates/pns) only places the via it is told.

import type { BoardState, NetClass, Nm, ViaPreset } from "../api/types";
import { classesOfNet } from "./boardSetupRules";
import type { LayerPair } from "./layerPairs";

/** `VIATYPE` as the board file and the route API name it (`(via blind ..)`). The router's commands make no `BURIED` one: `BLIND_VIA` is
 *  `VIATYPE::BLIND` between any two layers (`getViaTypeFromFlags`; `VIA_ACTION_FLAGS::BURIED_VIA` has no action). */
export type ViaType = "through" | "blind" | "buried" | "micro";

/** `VIA_ACTION_FLAGS`: `VIA` / `BLIND_VIA` / `MICROVIA`, with `SELECT_LAYER`. */
export interface ViaCommand {
  type: "through" | "blind" | "micro";
  selectLayer: boolean;
}

/** The router's via actions and their `Parameter<int>( VIA_ACTION_FLAGS::.. )`. */
export const VIA_COMMANDS: Readonly<Record<string, ViaCommand>> = {
  "pcbnew.InteractiveRouter.PlaceVia": { type: "through", selectLayer: false },
  "pcbnew.InteractiveRouter.PlaceBlindVia": { type: "blind", selectLayer: false },
  "pcbnew.InteractiveRouter.PlaceMicroVia": { type: "micro", selectLayer: false },
  "pcbnew.InteractiveRouter.SelLayerAndPlaceVia": { type: "through", selectLayer: true },
  "pcbnew.InteractiveRouter.SelLayerAndPlaceBlindVia": { type: "blind", selectLayer: true },
  "pcbnew.InteractiveRouter.SelLayerAndPlaceMicroVia": { type: "micro", selectLayer: true },
};

/** `handleLayerSwitch`'s infobar message for a via on one layer, word for word. */
export const VIA_NEEDS_TWO_LAYERS = "Via needs 2 different layers.";

/** What a via command arms: the via's type, the layers it spans (top first), the layer pair the router's sizes now hold, and the layer the route goes on on. */
export interface ViaPlan {
  ok: true;
  viaType: ViaType;
  /** `LINE_PLACER::makeVia`: a through via spans the board (`F_Cu` .. `B_Cu`), any other the pair (`m_sizes.GetLayerTop()` .. `GetLayerBottom()`). */
  span: [string, string];
  /** `sizes.AddLayerPair( currentLayer, targetLayer )`. */
  pair: [string, string];
  /** `switchLayerOnViaPlacement`: `Sizes().PairedLayer( currentLayer )`, else `GetLayerTop()`. */
  toLayer: string;
}

/** A refusal: `return 0` from `handleLayerSwitch`, with the infobar message when it shows one. */
export interface ViaRefusal {
  ok: false;
  message: string | null;
}

export interface ViaPlanInput {
  cmd: ViaCommand;
  /** `m_router->GetCurrentLayer()`: the layer being routed; null when no route is in progress (`UNDEFINED_LAYER`). */
  currentLayer: string | null;
  /** The board's copper layers in stack order (`LSET::AllCuMask( copperCount ).UIOrder()`): `F.Cu`, `In1.Cu` .. `B.Cu`. */
  copper: readonly string[];
  /** `m_Route_Layer_TOP` / `m_Route_Layer_BOTTOM`: the board's layer pair (`V`'s, "Set Layer Pair..."). */
  layerPair: LayerPair;
  /** For a `selectLayer` command, what the layer picker returned (`SelectOneLayer`); null: cancelled. Ignored otherwise. */
  picked?: string | null;
}

/** The layers the layer picker offers (`SelectOneLayer( currentLayer, not_allowed_ly )`, `not_allowed_ly = LSET::AllNonCuMask()`, plus the current layer for a via that is not a through via: "One cannot place a blind/buried via on only one layer"). */
export function pickableLayers(cmd: ViaCommand, copper: readonly string[], currentLayer: string): string[] {
  return copper.filter((l) => cmd.type === "through" || l !== currentLayer);
}

const index = (copper: readonly string[], layer: string) => copper.indexOf(layer);

/**
 * `ROUTER_TOOL::handleLayerSwitch( aEvent, aForceVia = true )` for a via command (the target layer is `UNDEFINED_LAYER` until the command
 * picks one):
 *  - Select Layer: nothing without a route in progress; the picker's layer is the target, and a blind or micro via to the current layer is refused;
 *  - a blind/buried or micro via from F.Cu to B.Cu (or back) is a through via ("Convert blind/buried via to a through hole one, if it goes through
 *    all layers") -- tested before an implicit target is chosen, so it only ever applies to a picked one, as in KiCad;
 *  - no target (Place Blind/Buried Via, Place Microvia): on a layer of the pair the via spans the pair, elsewhere it goes from the current layer
 *    to the pair's top; one layer only is "Via needs 2 different layers.";
 *  - then the pair `(currentLayer, targetLayer)` goes into the sizes, and after the via the route goes on on the layer paired with the one it
 *    was on, or the pair's top (`switchLayerOnViaPlacement`).
 * Place Through Via's implicit target (the nearest ratsnest anchor's layer, `GetNearestRatnestAnchor`) is not here: the studio's `V` is the
 * layer pair's other layer (actions/pcbMenuActions.ts).
 */
export function planViaCommand(input: ViaPlanInput): ViaPlan | ViaRefusal {
  const { cmd, copper, layerPair } = input;
  let currentLayer = input.currentLayer;
  if (currentLayer == null) return { ok: false, message: null };
  let viaType: ViaType = cmd.type;
  let targetLayer: string | null = null;
  if (cmd.selectLayer) {
    // "When the currentLayer is undefined, trying to place a via does not work"
    const picked = input.picked ?? null;
    if (picked == null) return { ok: false, message: null }; // canceled by user
    targetLayer = picked;
    // "One cannot place a blind/buried via on only one layer"
    if (viaType !== "through" && currentLayer === targetLayer) return { ok: false, message: null };
  }
  const front = copper[0] ?? "F.Cu";
  const back = copper[copper.length - 1] ?? "B.Cu";
  // "Convert blind/buried via to a through hole one, if it goes through all layers"
  if (viaType !== "through" && ((targetLayer === back && currentLayer === front) || (targetLayer === front && currentLayer === back))) viaType = "through";
  if (targetLayer == null) {
    // the through via's implicit target is not this function's (see the doc comment); a through command always comes with a picked layer here
    if (viaType === "through") return { ok: false, message: null };
    const { a: pairTop, b: pairBottom } = layerPair;
    if (currentLayer === pairTop || currentLayer === pairBottom) {
      // "the current layer is on the defined layer pair, swap to the other side"
      currentLayer = pairTop;
      targetLayer = pairBottom;
    } else {
      // "the current layer is not part of the current layer pair, so fallback and swap to the top layer of the pair by default"
      targetLayer = pairTop;
    }
    // "Do not create a broken via (i.e. a via on only one copper layer)"
    if (currentLayer === targetLayer) return { ok: false, message: VIA_NEEDS_TWO_LAYERS };
  }
  // `SIZES_SETTINGS::AddLayerPair`: top = min, bottom = max (PNS layer order is the stack order)
  const [i, j] = [index(copper, currentLayer), index(copper, targetLayer)];
  const pair: [string, string] = i <= j ? [currentLayer, targetLayer] : [targetLayer, currentLayer];
  const span: [string, string] = viaType === "through" ? [front, back] : pair;
  // `switchLayerOnViaPlacement`: `PairedLayer( m_router->GetCurrentLayer() )`, else `GetLayerTop()`
  const routing = input.currentLayer;
  const toLayer = routing === pair[0] ? pair[1] : routing === pair[1] ? pair[0] : pair[0];
  return { ok: true, viaType, span, pair, toLayer };
}

/** The effective net class's value of a setting for `net` (`NET_SETTINGS::GetEffectiveNetClass`: the classes it is in, highest priority first, then Default). */
function classValue(board: BoardState, net: string, f: (c: NetClass) => Nm | undefined): Nm | undefined {
  const rules = board.board_rules;
  for (const c of classesOfNet(rules?.net_classes ?? [], net)) {
    const v = f(c);
    if (v != null) return v;
  }
  return rules?.default_class ? f(rules.default_class) : undefined;
}

/** `NETCLASS`'s defaults when a class says nothing (`DEFAULT_UVIASIZE` 0.3 mm, `DEFAULT_UVIADRILL` 0.1 mm; the writer's `(uvia_dia 0.3) (uvia_drill 0.1)`). */
export const DEFAULT_MICROVIA = { diameter: 300_000, drill: 100_000 } as const;

/**
 * The via's size, as `handleLayerSwitch` sets it: with the net class's via sizes in use (`bds.UseNetClassVia()`: no via size of the list or
 * custom one chosen) or for a microvia, the net's class's (`VIA_DIAMETER_CONSTRAINT` / `HOLE_SIZE_CONSTRAINT` of a dummy via of that type: a
 * microvia's are the class's `uvia_dia` / `uvia_drill`); otherwise the chosen size (`bds.GetCurrentViaSize()` / `GetCurrentViaDrill()`).
 */
export function viaSizeFor(board: BoardState, net: string, viaType: ViaType, chosen: ViaPreset | null): { diameter: Nm; drill: Nm } {
  const rules = board.board_rules;
  if (viaType === "micro") {
    return { diameter: classValue(board, net, (c) => c.microvia_diameter) ?? DEFAULT_MICROVIA.diameter, drill: classValue(board, net, (c) => c.microvia_drill) ?? DEFAULT_MICROVIA.drill };
  }
  if (chosen) return { diameter: chosen.diameter, drill: chosen.drill };
  return { diameter: classValue(board, net, (c) => c.via_diameter) ?? rules?.via_diameter ?? 600_000, drill: classValue(board, net, (c) => c.via_drill) ?? rules?.via_drill ?? 300_000 };
}

/**
 * The width a route starts at, as `PNS_KICAD_IFACE_BASE::ImportSizes` picks it: undefined while the net class's widths are in use
 * (`BOARD_DESIGN_SETTINGS::UseNetClassTrack()`: `m_trackWidthIndex <= 0 && !m_useCustomTrackVia` -- no width chosen yet, or the list's first entry,
 * which is the net class's), and the server then takes the start net's width through the rules (`CT_WIDTH`); otherwise the chosen width. The
 * list's first entry is told by its width (the board's Default width): a preset of the same width is taken for it.
 */
export function chosenTrackWidth(current: Nm | null, netClassEntry: Nm | undefined, useCustomTrackVia: boolean): Nm | undefined {
  if (useCustomTrackVia) return current ?? undefined;
  if (current == null || current === netClassEntry) return undefined;
  return current;
}

/**
 * The via size `ROUTER_TOOL::handleLayerSwitch` arms a via with, as the studio sends it: null while the net class's sizes are in use
 * (`bds.UseNetClassVia()`: no via size of the list chosen, or its first entry, and no custom size) or for a microvia (`|| viaType == MICROVIA`),
 * and the server then takes the rules' (`VIA_DIAMETER_CONSTRAINT` / `HOLE_SIZE_CONSTRAINT` of a dummy via of the route's net, type and layers);
 * otherwise the chosen size.
 */
export function chosenViaSize(viaType: ViaType, chosen: ViaPreset | null, netClassEntry: ViaPreset | undefined, useCustomTrackVia: boolean): ViaPreset | null {
  if (viaType === "micro" || !chosen) return null;
  if (!useCustomTrackVia && netClassEntry && chosen.diameter === netClassEntry.diameter && chosen.drill === netClassEntry.drill) return null;
  return chosen;
}

/** `BOARD_DESIGN_SETTINGS`' custom track and via size (`m_customTrackWidth`, `m_customViaSize`): what `DIALOG_TRACK_VIA_SIZE` edits. */
export interface CustomTrackVia {
  trackNm: Nm;
  viaDiameterNm: Nm;
  viaDrillNm: Nm;
}

/** `BOARD_DESIGN_SETTINGS()`: `DEFAULT_CUSTOMTRACKWIDTH` 0.2 mm, `DEFAULT_VIASMINSIZE` 0.5 mm, `DEFAULT_MINTHROUGHDRILL` 0.3 mm. */
export const DEFAULT_CUSTOM_TRACK_VIA: CustomTrackVia = { trackNm: 200_000, viaDiameterNm: 500_000, viaDrillNm: 300_000 };

/** `GEOMETRY_MIN_SIZE` (pcb_track.h): 0.001 mm. */
const GEOMETRY_MIN_SIZE_NM = 1000;

/** `PCB_VIA::ValidateViaParameters( diameter, drill, .. )` for a via with no layers given: its message, word for word, or null when the via is good. Null stands for a field that is not a length (`std::nullopt`). */
export function validateViaParameters(diameter: Nm | null, drill: Nm | null): string | null {
  if (diameter != null && diameter < GEOMETRY_MIN_SIZE_NM) return "Via diameter is too small.";
  if (drill != null && drill < GEOMETRY_MIN_SIZE_NM) return "Via drill is too small.";
  if (diameter != null && drill == null) return "No via hole size defined.";
  if (drill != null && diameter == null) return "No via diameter defined.";
  if (diameter != null && drill != null && diameter <= drill) return "Via hole size must be smaller than via diameter";
  return null;
}
