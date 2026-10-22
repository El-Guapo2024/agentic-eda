// Edit Pad as Graphic Shapes and Finish Pad Edit (`pcbnew.PadTool.explodePad` / `pcbnew.PadTool.recombinePad`, both Ctrl+E): what `PAD_TOOL::EditPad`,
// `PAD_TOOL::Init` (the menu conditions), `enterPadEditMode`/`ExitPadEditMode` and `FOOTPRINT_EDIT_FRAME::SaveFootprint` decide, as pure functions of the
// footprint the Footprint Editor has open. The pad edit mode is part of the footprint (`LibraryFootprint.pad_edit`, the `ENTERED` flag of the pad), so
// undo, redo and a reload bring it back, as `PAD_TOOL::OnUndoRedo` does by looking for the flagged pad.
import type { Cmd, LibraryFootprint, LibraryPad } from "../api/types";

/** `PAD_TOOL::InPadEditMode`: a pad is being edited as graphic shapes. */
export function inPadEditMode(fp: Pick<LibraryFootprint, "pad_edit"> | null | undefined): boolean {
  return !!fp?.pad_edit;
}

/**
 * `PAD_TOOL::Init`'s `explodeCondition` and `recombineCondition`, which are the two entries the Footprint Editor's selection menu has for the tool: "Edit Pad as
 * Graphic Shapes" while no pad is edited and exactly one pad is selected, "Finish Pad Edit" while one is.
 */
export function padEditMenu(fp: LibraryFootprint | null | undefined, selection: ReadonlySet<string>): { explode: boolean; recombine: boolean } {
  if (!fp) return { explode: false, recombine: false };
  const editing = inPadEditMode(fp);
  const onePad = selection.size === 1 && fp.pads.some((p) => !!p.id && selection.has(p.id));
  return { explode: !editing && onePad, recombine: editing };
}

/**
 * What `PAD_TOOL::EditPad` does when either action runs: with a pad being edited it recombines it (whichever of the two actions was asked); with none being
 * edited and a single pad selected it explodes that pad; otherwise nothing (`selection.Size() == 1 && selection[0]->Type() == PCB_PAD_T`).
 */
export function editPadCommand(footprint: string, fp: LibraryFootprint | null | undefined, selection: ReadonlySet<string>): Cmd | null {
  if (!fp) return null;
  if (fp.pad_edit) return { op: "recombine_pad", footprint };
  const only = selection.size === 1 ? [...selection][0] : undefined;
  const pad = only ? fp.pads.find((p) => p.id === only) : undefined;
  return pad?.id ? { op: "explode_pad", footprint, id: pad.id } : null;
}

const TECH_UI_ORDER = ["F.Adhes", "B.Adhes", "F.Paste", "B.Paste", "F.SilkS", "B.SilkS", "F.Mask", "B.Mask", "Dwgs.User", "Cmts.User", "Eco1.User", "Eco2.User", "Edge.Cuts", "Margin", "F.CrtYd", "B.CrtYd", "F.Fab", "B.Fab"];

/** A layer's place in `LSET::UIOrder()`: the copper stack, then `TechAndUserUIOrder()`, then the numbered user layers. */
export function uiOrderRank(layer: string): number {
  if (layer === "F.Cu") return 0;
  if (layer === "B.Cu") return 10_000;
  const inner = /^In(\d+)\.Cu$/.exec(layer);
  if (inner) return Number(inner[1]);
  const tech = TECH_UI_ORDER.indexOf(layer);
  if (tech >= 0) return 20_000 + tech;
  const user = /^User\.(\d+)$/.exec(layer);
  if (user) return 30_000 + Number(user[1]);
  return 40_000;
}

/**
 * `explodePad`'s layer: the pad's `F.Cu` when it is on it, else `B.Cu`, else the first of its layers in the layer lists' order (`GetLayerSet().UIOrder().front()`);
 * the editor's active layer becomes it. A pad whose layer list is empty has its kind's default (`PAD::SMDMask`, `PTHMask`).
 */
export function padEditLayer(pad: Pick<LibraryPad, "layers" | "kind">): string {
  const layers = pad.layers.length > 0 ? pad.layers : pad.kind === "smd" ? ["F.Cu", "F.Paste", "F.Mask"] : ["*.Cu", "*.Mask"];
  const expanded = layers.flatMap((l) => (l === "*.Cu" || l === "F&B.Cu" ? ["F.Cu", "B.Cu"] : l.startsWith("*.") ? [`F.${l.slice(2)}`, `B.${l.slice(2)}`] : [l]));
  if (expanded.includes("F.Cu")) return "F.Cu";
  if (expanded.includes("B.Cu")) return "B.Cu";
  return [...expanded].sort((a, b) => uiOrderRank(a) - uiOrderRank(b))[0] ?? "F.Cu";
}

/** `PAD_TOOL::enterPadEditMode`'s info bar: `Pad Edit Mode.  Press %s again to exit.` (the two actions share the hotkey). */
export const PAD_EDIT_MODE_MESSAGE = "Pad Edit Mode.  Press Ctrl+E again to exit.";

/**
 * `PAD_TOOL::EnumeratePads` and `PAD_TOOL::PadTable` both start with `if( !m_isFootprintEditor || InPadEditMode() ) return 0;`: while a pad is edited as
 * shapes neither the Renumber Pads tool nor the Pad Table opens.
 */
export function padToolsBlocked(fp: LibraryFootprint | null | undefined): boolean {
  return inPadEditMode(fp);
}

/**
 * `enterPadEditMode` / `ExitPadEditMode` and the high contrast mode: entering turns the dimmed contrast on (`if( opts.m_ContrastModeDisplay == NORMAL )
 * opts.m_ContrastModeDisplay = DIMMED`), remembering what it was; leaving puts it back (`if( m_previousHighContrastMode != opts.m_ContrastModeDisplay )`). The
 * editor's flag is "dimmed or not", so `now` is the flag when the mode begins and the result is what to remember.
 */
export function enterContrast(now: boolean): { highContrast: boolean; remembered: boolean } {
  return { highContrast: true, remembered: now };
}

/** The flag to leave the mode with: the one remembered on entering (`null` when the mode never began here: the flag as it is). */
export function exitContrast(remembered: boolean | null, now: boolean): boolean {
  return remembered ?? now;
}
