// Port of common/tool/selection_tool.cpp (SELECTION_TOOL::setModifiersState,
// ::hasModifier) and the decision logic inside pcbnew/tools/
// pcb_selection_tool.cpp's PCB_SELECTION_TOOL::selectPoint and
// ::GuessSelectionCandidates. Pure logic only: Canvas.tsx/
// components/canvas/selectionCandidates.ts supply the actual candidate
// geometry (hit distance, area, layer) this module decides between.
//
// Modifier semantics, read directly from selection_tool.cpp lines 49-84:
//
//   m_subtractive      = aCtrlState && aShiftState;
//   m_additive         = !aCtrlState && aShiftState;
//   if( ctrlClickHighlights() ) { m_exclusive_or = false; ... }
//   else                        { m_exclusive_or = aCtrlState && !aShiftState; }
//   m_drag_additive    = ( aCtrlState || aShiftState ) && !aAltState;
//   m_drag_subtractive = aCtrlState && aShiftState && !aAltState;
//   m_skip_heuristics  = aAltState;  // (source special-cases this OFF on
//                                    //  Windows only, a wx/MSW system-menu
//                                    //  conflict that doesn't apply here)
//
// `ctrlClickHighlights()` is `PCBNEW_SETTINGS::m_CtrlClickHighlight`,
// which defaults OFF (pcbnew_settings.cpp: `m_CtrlClickHighlight( false )`)
// and has no Preferences UI equivalent in this app -- so this module
// always takes the `else` branch above, exactly like a stock KiCad
// install. Ctrl/Cmd-click is therefore always the exclusive-or (toggle)
// modifier here, never "highlight this net instead."
//
// Ctrl vs Cmd: KiCad's own TOOL_EVENT::Modifier(MD_CTRL) is wx's
// wxMOD_CONTROL, which wxWidgets itself maps to the Cmd key on macOS (not
// the physical Control key) -- the same substitution this app already
// makes everywhere else (gridSnap.ts, viewControls.ts). Callers pass
// `ctrlOrCmd` pre-resolved the same way (isMac() ? e.metaKey : e.ctrlKey).

export interface ClickModifiers {
  /** Shift+click (without Ctrl/Cmd): add the hit to the selection. */
  additive: boolean;
  /** Ctrl/Cmd+Shift+click: remove the hit from the selection (selectPoint also restricts the candidate pool to already-selected items first -- see selectionCandidates.ts). */
  subtractive: boolean;
  /** Ctrl/Cmd+click alone: toggle the hit's membership. */
  exclusiveOr: boolean;
  /** Alt held: skip GuessSelectionCandidates and always show the clarification menu for 2+ overlapping candidates. */
  skipHeuristics: boolean;
}

export function computeClickModifiers(shiftKey: boolean, ctrlOrCmd: boolean, altKey: boolean): ClickModifiers {
  return {
    subtractive: ctrlOrCmd && shiftKey,
    additive: shiftKey && !ctrlOrCmd,
    exclusiveOr: ctrlOrCmd && !shiftKey,
    skipHeuristics: altKey,
  };
}

/** selection_tool.cpp SELECTION_TOOL::hasModifier. */
export function hasModifier(m: Pick<ClickModifiers, "additive" | "subtractive" | "exclusiveOr">): boolean {
  return m.additive || m.subtractive || m.exclusiveOr;
}

export interface DragModifiers {
  /** Ctrl/Cmd-drag OR Shift-drag (not both, and not with Alt): add the box's contents to the selection instead of replacing it. */
  additive: boolean;
  /** Ctrl/Cmd+Shift-drag (without Alt): remove the box's contents from the selection. */
  subtractive: boolean;
}

export function computeDragModifiers(shiftKey: boolean, ctrlOrCmd: boolean, altKey: boolean): DragModifiers {
  return {
    additive: (ctrlOrCmd || shiftKey) && !altKey,
    subtractive: ctrlOrCmd && shiftKey && !altKey,
  };
}

/**
 * pcb_selection_tool.cpp SelectRectArea/SelectMultiple: which way the box
 * was dragged decides window (fully-enclosed) vs. crossing (touching) --
 * "Left > Right: fully enclosed. Right > Left: crossed." (greedySelection
 * in source == this module's `crossing`). Source also flips this when the
 * view is mirrored in X; this app's view is never mirrored, so that branch
 * doesn't apply.
 */
export function isCrossingSelection(startXUm: number, endXUm: number): boolean {
  return endXUm < startXUm;
}

/**
 * pcb_selection_tool.cpp selectPoint's own apply step (lines ~850-877),
 * for a single resolved hit `id`:
 *
 *   if( !additive && !subtractive && !exclusive_or ) { if (selected) ClearSelection(); }
 *   for each collector item:
 *     if( subtractive || (exclusive_or && item->IsSelected()) ) unselect(item);
 *     else select(item);
 *
 * which collapses (for exactly one resolved item) to: no modifier ->
 * replace the whole selection with just this one; additive -> add it
 * (never removes, even if already present); subtractive -> remove it;
 * exclusiveOr -> toggle it.
 */
export function applySingleClickModifier(current: ReadonlySet<string>, id: string, modifiers: Pick<ClickModifiers, "additive" | "subtractive" | "exclusiveOr">): string[] {
  if (modifiers.subtractive) return [...current].filter((r) => r !== id);
  if (modifiers.exclusiveOr) return current.has(id) ? [...current].filter((r) => r !== id) : [...current, id];
  if (modifiers.additive) return current.has(id) ? [...current] : [...current, id];
  return [id];
}

/**
 * pcb_selection_tool.cpp SelectMultiple's per-item apply (lines
 * ~1328-1336): `if (aSubtractive || (aExclusiveOr && item->IsSelected())) unselect(item); else select(item);`
 * -- note there is no "additive" branch here at all; whether the
 * pre-existing selection was cleared is decided once, before the box is
 * even drawn (see `hasModifier`'s own doc comment), not per matched item.
 */
export function applyBoxSelectionModifiers(current: ReadonlySet<string>, hitIds: readonly string[], modifiers: Pick<ClickModifiers, "subtractive" | "exclusiveOr">): string[] {
  const next = new Set(current);
  for (const id of hitIds) {
    if (modifiers.subtractive || (modifiers.exclusiveOr && next.has(id))) next.delete(id);
    else next.add(id);
  }
  return [...next];
}

// ------------------------------------------------------- GuessSelectionCandidates

/** What GuessSelectionCandidates needs to know about each overlapping candidate. */
export interface GuessCandidate {
  /** pcb_selection_tool.cpp hitTestDistance: 0 = exact/inside hit, world µm distance to the item's own outline otherwise, already clamped to some reasonable max by the caller. */
  slopUm: number;
  /** An approximate "coverage area" (board µm²) used only to compare candidates relatively -- pcb_selection_tool.cpp's itemsByArea (FOOTPRINT::GetCoverageArea, with vias/zone-borders artificially shrunk). Smaller wins when one item sits inside a much bigger one. */
  areaUm2: number;
  /** The layer this candidate is considered "on" for the active-layer preference pass, or null for an item that doesn't belong to a single layer (a footprint as a whole, a through via) -- treated as always-on-layer, same effect as source's items never entering `rejected` for not matching `activeLayer`. */
  layer: string | null;
}

/** pcb_selection_tool.cpp: "Prefer exact hits to sloppy ones" -- MAX_SLOP's role is in the caller's own hitTestDistance clamp; this constant is the *second* threshold, `singlePixel`, source adds on top of the best candidate's own slop. */
export function sloppinessThresholdUm(onePixelUm: number): number {
  return onePixelUm;
}

/** pcb_selection_tool.cpp GuessSelectionCandidates: "If the user clicked on a small item within a much larger one then it's pretty clear they're trying to select the smaller one." */
export const SIZE_RATIO = 1.5;

/**
 * Port of PCB_SELECTION_TOOL::GuessSelectionCandidates, narrowed to the
 * three passes that translate to this app's simpler item model (no
 * silk/courtyard-active-layer heuristic -- this app has no distinct
 * silkscreen-text item type to special-case, and no footprint
 * coverage-ratio override -- no Clipper-based exact polygon coverage
 * here, just the approximate `areaUm2` the caller computed):
 *
 *   1. Prefer exact/near hits over sloppy ones (within one screen pixel
 *      of the single best hit).
 *   2. Prefer the smallest of a set of nested items (never rejecting
 *      every candidate).
 *   3. Prefer whatever's on the currently active layer, if anything is.
 *
 * Returns the pruned list (still &gt;1 entries means a real ambiguity --
 * the caller shows the disambiguation menu, matching source's `if
 * (collector.GetCount() > 1) { ... doSelectionMenu ... }`).
 */
export function guessSelectionCandidates<T extends GuessCandidate>(candidates: readonly T[], onePixelUm: number, activeLayer: string | null): T[] {
  if (candidates.length <= 1) return [...candidates];

  let pool: readonly T[] = candidates;

  // 1. Prefer exact hits to sloppy ones.
  const minSlop = Math.min(...pool.map((c) => c.slopUm));
  const threshold = minSlop + sloppinessThresholdUm(onePixelUm);
  const afterSlop = pool.filter((c) => c.slopUm <= threshold);
  if (afterSlop.length > 0) pool = afterSlop;

  if (pool.length === 1) return [...pool];

  // 2. Prefer the smallest of a set of similarly-sloppy nested items.
  const byArea = [...pool].sort((a, b) => a.areaUm2 - b.areaUm2);
  const rejected = new Set<T>();
  let rejecting = false;
  for (let i = 1; i < byArea.length; i++) {
    if (byArea[i]!.areaUm2 > byArea[i - 1]!.areaUm2 * SIZE_RATIO) rejecting = true;
    if (rejecting) rejected.add(byArea[i]!);
  }
  if (rejected.size > 0 && rejected.size < pool.length) {
    pool = pool.filter((c) => !rejected.has(c));
  }

  if (pool.length === 1 || activeLayer == null) return [...pool];

  // 3. Reject anything not on the active layer, as long as something is
  // (layer-agnostic candidates -- `layer: null` -- always survive this
  // pass, same as source never adding a non-layered item to `rejected`).
  const hasActive = pool.some((c) => c.layer === activeLayer);
  if (hasActive) {
    pool = pool.filter((c) => c.layer === null || c.layer === activeLayer);
  }

  return [...pool];
}
