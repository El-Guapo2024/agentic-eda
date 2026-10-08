// The docked panes of the editor frames and the rules that decide which of them show -- a port of the AUI layout of eeschema's frame
// (eeschema/sch_edit_frame.cpp's constructor, eeschema/eeschema_settings.cpp's default*PaneInfo, 8303b2ad) onto this app's one window:
//
//   - Every side pane of the schematic frame docks in ONE column on the left (`.Left().Layer( 3 )`), ordered by `Position`:
//     Net Navigator 0 (hidden by default), Schematic Hierarchy 1, Properties 2, Selection Filter 4. Between that column and the canvas
//     (layer 2) sit the options toolbar (left) and the drawing toolbar (right); there is NO dock on the right of the sheet (the Design Blocks
//     and Remote Symbols panes would go there, hidden by default, and the studio has neither).
//   - Hierarchy and Properties are shown by default (`aui.show_schematic_hierarchy`, `aui.show_properties` default true); the toolbar buttons
//     `showHierarchy` / `showProperties` toggle them (`SCH_EDIT_FRAME::setupUIConditions`' CHECK( ... IsShown() )).
//   - The Selection Filter has no switch of its own: `SCH_EDIT_FRAME::updateSelectionFilterVisbility` shows it while the hierarchy, the net
//     navigator or the properties pane is shown and docked, and it is a fixed-size pane (`dock_proportion = 0`, 180 wide, no vertical growth).
//   - pcbnew's frame (pcb_edit_frame.cpp) docks Properties on the left and the Appearance manager (with the selection filter) on the right.
//
// What a browser window adds that wxAUI's does not need: an 800 px window cannot hold 300 px of properties next to a canvas, so each column
// can be folded to a handle (`leftCollapsed` / `rightCollapsed`) and each pane to its caption (`folded`), and a window narrower than
// `NARROW_WINDOW_PX` starts with both columns folded, so the canvas keeps most of the width. The studio's own panes (the Appearance dock)
// follow the same two switches.

export type DockPaneId = "hierarchy" | "properties" | "selectionFilter";

/** `wxAuiPaneInfo::Position()` of each pane of the schematic frame's left column (lower = higher up); the order panes are drawn in. */
export const SCH_LEFT_COLUMN_ORDER: readonly DockPaneId[] = ["hierarchy", "properties", "selectionFilter"];

/** The width, in CSS px, of the docked column: Properties' `MinSize( 240, 60 )`; the Selection Filter is narrower (180) and fits inside it. */
export const DOCK_COLUMN_WIDTH_PX = 240;

/** Below this window width a frame starts with its side columns folded to their handles. */
export const NARROW_WINDOW_PX = 1100;

/** The columns that fold: the frame's left and right docks, and the library editors' tree column (`Libraries`, a separate pane at the far left of those frames). */
export type DockColumnId = "left" | "right" | "tree";

export interface DockLayout {
  /** The Footprint and Symbol editors' library tree column is folded to its handle. Open to begin with: the tree is how an item is opened there. */
  treeCollapsed: boolean;
  /** The left column is folded to its handle (the canvas takes the width). */
  leftCollapsed: boolean;
  /** The right column (the board editor's Appearance dock, which is not a closable pane) is folded to its handle. */
  rightCollapsed: boolean;
  /** `wxAuiPaneInfo::IsShown`: a hidden pane is gone from the column until its toolbar button shows it again. */
  shown: Record<DockPaneId, boolean>;
  /** A shown pane rolled up to its caption bar. */
  folded: Record<DockPaneId, boolean>;
}

/** The layout of a first run in a window `windowWidth` px wide. */
export function defaultDockLayout(windowWidth: number): DockLayout {
  const narrow = windowWidth < NARROW_WINDOW_PX;
  return {
    treeCollapsed: false,
    leftCollapsed: narrow,
    rightCollapsed: narrow,
    // `show_schematic_hierarchy` and `show_properties` default true; the Appearance manager is shown by default in pcbnew too.
    shown: { hierarchy: true, properties: true, selectionFilter: true },
    folded: { hierarchy: false, properties: false, selectionFilter: false },
  };
}

/**
 * `SCH_EDIT_FRAME::updateSelectionFilterVisbility`: "Don't give the selection filter its own visibility controls; instead show it if anything else
 * is visible" -- the hierarchy, the net navigator or the properties pane, docked. (The studio has no net navigator pane.)
 */
export function selectionFilterShown(layout: DockLayout): boolean {
  return layout.shown.hierarchy || layout.shown.properties;
}

/** Whether `id` is drawn at all: the filter by `updateSelectionFilterVisbility`'s rule, every other pane by its own flag. */
export function paneVisible(layout: DockLayout, id: DockPaneId): boolean {
  return id === "selectionFilter" ? selectionFilterShown(layout) : layout.shown[id];
}

/**
 * A pane's toolbar toggle (`ACTIONS::showProperties`, `SCH_ACTIONS::showHierarchy`: `PANE_INFO.Show( !IsShown() )`): showing a pane also unfolds it
 * and brings its column back, because a pane that is "shown" inside a folded column would look like the button did nothing.
 */
export function togglePane(layout: DockLayout, id: DockPaneId): DockLayout {
  const show = !layout.shown[id];
  const next: DockLayout = { ...layout, shown: { ...layout.shown, [id]: show } };
  if (show) {
    next.folded = { ...layout.folded, [id]: false };
    next.leftCollapsed = false; // every pane here is docked in the left column
  }
  return next;
}

/** The caption bar's chevron: roll the pane up to its caption, or open it again. */
export function toggleFolded(layout: DockLayout, id: DockPaneId): DockLayout {
  return { ...layout, folded: { ...layout.folded, [id]: !layout.folded[id] } };
}

export function setColumnCollapsed(layout: DockLayout, column: DockColumnId, collapsed: boolean): DockLayout {
  switch (column) {
    case "left":
      return { ...layout, leftCollapsed: collapsed };
    case "right":
      return { ...layout, rightCollapsed: collapsed };
    case "tree":
      return { ...layout, treeCollapsed: collapsed };
  }
}

/** Panes of the left column of the schematic frame that are drawn, top to bottom (`Position` order). */
export function schLeftColumn(layout: DockLayout): DockPaneId[] {
  return SCH_LEFT_COLUMN_ORDER.filter((id) => paneVisible(layout, id));
}

/** Read a stored layout back: anything missing or of the wrong type falls back to the default for this window (a layout from an older build must not break the app). */
export function parseDockLayout(raw: unknown, windowWidth: number): DockLayout {
  const base = defaultDockLayout(windowWidth);
  if (!raw || typeof raw !== "object") return base;
  const o = raw as Record<string, unknown>;
  const flags = (v: unknown, fallback: Record<DockPaneId, boolean>): Record<DockPaneId, boolean> => {
    const out = { ...fallback };
    if (v && typeof v === "object") {
      for (const id of Object.keys(fallback) as DockPaneId[]) {
        const f = (v as Record<string, unknown>)[id];
        if (typeof f === "boolean") out[id] = f;
      }
    }
    return out;
  };
  return {
    treeCollapsed: typeof o.treeCollapsed === "boolean" ? o.treeCollapsed : base.treeCollapsed,
    leftCollapsed: typeof o.leftCollapsed === "boolean" ? o.leftCollapsed : base.leftCollapsed,
    rightCollapsed: typeof o.rightCollapsed === "boolean" ? o.rightCollapsed : base.rightCollapsed,
    shown: flags(o.shown, base.shown),
    folded: flags(o.folded, base.folded),
  };
}
