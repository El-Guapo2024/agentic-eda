// Preferences > <editor> > Toolbars: `PANEL_TOOLBAR_CUSTOMIZATION` (common/dialogs/panel_toolbar_customization.cpp and its _base, KiCad 8303b2ad), the page every
// frame's Preferences has (pcbnew.cpp `PANEL_PCB_TOOLBARS`, `PANEL_FP_TOOLBARS`, `PANEL_3DV_TOOLBARS`; eeschema.cpp `PANEL_SCH_TOOLBARS`, `PANEL_SYM_TOOLBARS`).
// The rules are kicad-port/toolbarCustomization.ts'; the person's toolbars are state/toolbarSettings.ts'.
//
//   * "Customize toolbars" (`m_CustomToolbars`): off, the editor shows KiCad's toolbars and every other control of the page is dimmed (`enableCustomControls`).
//   * "Toolbar:" picks one of the frame's toolbars ("Left", "Right", "Top main", "Top auxiliary": those it has a default for); its items are the tree on the left
//     (a group's tools are its children). Picking another toolbar keeps the shown one's edits in the page's shadow copy (`onTbChoiceSelect`).
//   * "Insert Separator" (its menu: "Insert Spacer", "Insert Group") puts the item after the selected row, never into a group; Move Up / Move Down move the
//     selected row among its siblings; the trash button deletes it (the row before is selected next). A double click on a group renames it.
//   * On the right the frame's actions (icons only, the hidden ones left out) and its controls, sorted by name, with "Filter actions"; the arrow button (or a
//     double click on an entry) adds the selected entry after the selected row, or at the end of a selected group; a control is moved, never copied, between the
//     frame's toolbars.
//   * The dialog's "Reset Toolbars to Defaults" (`ResetPanel`) puts KiCad's toolbars back in every shadow copy; OK (`TransferDataFromWindow`) keeps "Customize
//     toolbars" and stores every toolbar, each control on its first toolbar only.
//
// Different from KiCad: a group's new name is kept, as KiCad 25ec2335's `onTreeEndLabelEdit` keeps it (8303b2ad's drops it; an empty name is refused in both); a
// spacer's text cannot be edited (8303b2ad lets the label change and keeps nothing). No drag and drop in the tree (KiCad has none either: "TODO (ISM): Enable
// draging"). A page that was never shown stores nothing on OK, as KiCad's lazily built page is never made.
import { useEffect, useMemo, useState } from "react";
import type { PreferencesPageHandle } from "./preferencesPage";
import { ActionIcon } from "./Toolbar";
import { ContextMenu } from "./canvas/ContextMenu";
import { TOOLBAR_ACTIONS, defaultConfigsOf, frameToolbarSettings, setFrameToolbarSettings, toolbarAction } from "../state/toolbarSettings";
import {
  TOOLBAR_LOC_NAME,
  actionEntries,
  addEntry,
  dedupeControls,
  defaultShadowToolbars,
  deleteSelected,
  filterActionEntries,
  insertItem,
  moveSelected,
  parseToolbarTree,
  populateToolbarTree,
  renameGroup,
  shadowToolbars,
  toolbarChoices,
  treeLabel,
  type ActionListEntry,
  type StoredToolbars,
  type ToolbarFrame,
  type ToolbarLoc,
  type ToolbarTree,
  type TreeNode,
} from "../kicad-port/toolbarCustomization";

export type ToolbarsPageEditor = "pcb" | "schematic" | "footprint" | "symbol" | "3d";

const EMPTY_TREE: ToolbarTree = { nodes: [], selected: null, nextId: 1 };

const hasTool = (name: string) => toolbarAction(name) !== undefined;
const labelOf = (name: string) => toolbarAction(name)?.label ?? name;

interface PageState {
  custom: boolean;
  shadow: StoredToolbars;
  current: ToolbarLoc | null;
  tree: ToolbarTree;
}

/** `TransferDataToWindow`: the shadow toolbars as the frame would show them with customisation allowed, the first toolbar shown, the frame's "Customize toolbars". */
function loadPage(frame: ToolbarFrame): PageState {
  const defaults = defaultConfigsOf(frame);
  const s = frameToolbarSettings(frame);
  const shadow = shadowToolbars(defaults, s.toolbars);
  const current = toolbarChoices(defaults)[0] ?? null;
  return { custom: s.custom, shadow, current, tree: current ? populateToolbarTree(shadow[current] ?? [], { frame, hasTool }) : EMPTY_TREE };
}

export function useToolbarsPage(editor: ToolbarsPageEditor, open: boolean, shown: boolean): PreferencesPageHandle {
  const frame: ToolbarFrame = editor;
  const defaults = defaultConfigsOf(frame);
  const choices = useMemo(() => toolbarChoices(defaults), [defaults]);
  const [page, setPage] = useState<PageState>(() => loadPage(frame));
  /** The page was shown while the dialog was up (KiCad builds the page lazily: one never shown is never transferred). */
  const [visited, setVisited] = useState(false);
  const [filter, setFilter] = useState("");
  const [picked, setPicked] = useState<string | null>(null);
  const [renaming, setRenaming] = useState<{ id: number; text: string } | null>(null);
  const [insertMenu, setInsertMenu] = useState<{ x: number; y: number } | null>(null);

  useEffect(() => {
    if (open) {
      setPage(loadPage(frame));
      setVisited(false);
      setFilter("");
      setRenaming(null);
    }
  }, [open, frame]);
  useEffect(() => {
    if (open && shown) setVisited(true);
  }, [open, shown]);

  const entries = useMemo(() => actionEntries(frame, TOOLBAR_ACTIONS), [frame]);
  const shownEntries = useMemo(() => filterActionEntries(entries, filter), [entries, filter]);
  // `applyActionFilter`: the first entry is selected after each filter change
  useEffect(() => setPicked(shownEntries[0]?.name ?? null), [shownEntries]);

  const setTree = (tree: ToolbarTree) => setPage((p) => ({ ...p, tree }));
  const enabled = page.custom && page.current !== null;

  /** `onTbChoiceSelect`: the shown toolbar's tree into its shadow copy, then the other one's tree. */
  const pickToolbar = (loc: ToolbarLoc) =>
    setPage((p) => {
      const shadow = p.current ? { ...p.shadow, [p.current]: parseToolbarTree(p.tree) } : p.shadow;
      return { ...p, shadow, current: loc, tree: populateToolbarTree(shadow[loc] ?? [], { frame, hasTool }) };
    });

  /** `onBtnAddAction`, then "Move the action to the next available one, to be nice". */
  const add = (entry: ActionListEntry | undefined) => {
    if (!entry || !page.current) return;
    const r = addEntry(page.tree, page.shadow, page.current, { kind: entry.kind, name: entry.name });
    setPage((p) => ({ ...p, tree: r.tree, shadow: r.shadow }));
    const i = shownEntries.findIndex((e) => e.name === entry.name);
    if (i >= 0 && i + 1 < shownEntries.length) setPicked(shownEntries[i + 1]!.name);
  };

  const commitRename = () => {
    if (!renaming) return;
    const t = renameGroup(page.tree, renaming.id, renaming.text);
    if (t) setTree(t);
    setRenaming(null);
  };

  const row = (n: TreeNode, depth: number) => {
    const selected = page.tree.selected === n.id;
    const label = treeLabel(n, frame, labelOf);
    return (
      <div
        key={n.id}
        role="treeitem"
        aria-selected={selected}
        aria-level={depth + 1}
        data-node-id={n.id}
        data-kind={n.kind}
        data-name={n.kind === "tool" || n.kind === "control" ? n.name : undefined}
        onClick={() => enabled && setTree({ ...page.tree, selected: n.id })}
        onDoubleClick={() => enabled && n.kind === "group" && setRenaming({ id: n.id, text: n.name })}
        style={{
          display: "flex",
          alignItems: "center",
          gap: 6,
          padding: "1px 4px",
          paddingLeft: 4 + depth * 18,
          cursor: enabled ? "pointer" : "default",
          background: selected ? "var(--chrome-accent-bg, rgba(74,163,255,0.2))" : undefined,
          whiteSpace: "nowrap",
        }}
      >
        {n.kind === "tool" ? <ActionIcon iconName={toolbarAction(n.name)?.icon ?? null} size={16} /> : <span style={{ width: 16, flexShrink: 0 }} />}
        {renaming && renaming.id === n.id ? (
          <input
            autoFocus
            aria-label="Group name"
            value={renaming.text}
            onChange={(e) => setRenaming({ id: n.id, text: e.target.value })}
            onBlur={commitRename}
            onKeyDown={(e) => {
              if (e.key === "Enter") {
                e.preventDefault();
                commitRename();
              } else if (e.key === "Escape") {
                // the edit is cancelled, not the dialog
                e.preventDefault();
                e.stopPropagation();
                setRenaming(null);
              }
            }}
            style={{ fontSize: 12, padding: "0 2px" }}
          />
        ) : (
          <span style={{ fontStyle: n.kind === "separator" || n.kind === "spacer" ? "italic" : undefined }}>{label}</span>
        )}
      </div>
    );
  };

  const smallButton = (label: string, glyph: string, onClick: () => void, testid: string, disabled = !enabled) => (
    <button aria-label={label} title={label} disabled={disabled} onClick={onClick} data-testid={testid} style={{ minWidth: 28 }}>
      {glyph}
    </button>
  );

  const element = !shown ? null : (
    <div data-testid={`toolbars-page-${editor}`} style={{ display: "flex", flexDirection: "column", gap: 6 }}>
      <label className="filter-row" style={{ margin: 0 }}>
        <input
          type="checkbox"
          data-testid="toolbars-customize"
          checked={page.custom}
          onChange={(e) => {
            const custom = e.target.checked;
            setPage((p) => ({ ...p, custom }));
          }}
        />
        Customize toolbars
      </label>
      <hr style={{ border: 0, borderTop: "1px solid var(--chrome-border)", margin: "2px 0" }} />
      <div style={{ display: "flex", gap: 8, minHeight: 0 }}>
        {/* bLeftCol: the toolbar choice, the tree, and the buttons under it */}
        <div style={{ flex: 1, minWidth: 0, display: "flex", flexDirection: "column", gap: 4 }}>
          <div className="filter-row" style={{ margin: 0, gap: 6 }}>
            <span>Toolbar:</span>
            <select
              aria-label="Toolbar"
              data-testid="toolbars-choice"
              disabled={!page.custom || choices.length === 0}
              value={page.current ?? ""}
              onChange={(e) => pickToolbar(e.target.value as ToolbarLoc)}
              style={{ flex: 1 }}
            >
              {choices.map((loc) => (
                <option key={loc} value={loc}>
                  {TOOLBAR_LOC_NAME[loc]}
                </option>
              ))}
            </select>
          </div>
          <div
            role="tree"
            aria-label="Toolbar items"
            aria-disabled={!enabled}
            data-testid="toolbars-tree"
            style={{ height: 330, overflowY: "auto", border: "1px solid var(--chrome-border)", fontSize: 12, opacity: enabled ? 1 : 0.5 }}
          >
            {page.tree.nodes.map((n) => (n.kind === "group" ? [row(n, 0), ...n.children.map((c) => row(c, 1))] : row(n, 0)))}
          </div>
          <div style={{ display: "flex", gap: 6, alignItems: "center" }}>
            {/* SPLIT_BUTTON: the button inserts a separator, its menu a spacer or a group */}
            <span style={{ display: "inline-flex" }}>
              <button disabled={!enabled} data-testid="toolbars-insert" onClick={() => setTree(insertItem(page.tree, "separator"))}>
                Insert Separator
              </button>
              <button
                aria-label="Insert menu"
                disabled={!enabled}
                data-testid="toolbars-insert-menu"
                onClick={(e) => {
                  const r = e.currentTarget.getBoundingClientRect();
                  setInsertMenu({ x: r.left, y: r.bottom });
                }}
                style={{ padding: "0 4px" }}
              >
                ▾
              </button>
            </span>
            {smallButton("Move Up", "▲", () => setTree(moveSelected(page.tree, -1)), "toolbars-move-up")}
            {smallButton("Move Down", "▼", () => setTree(moveSelected(page.tree, 1)), "toolbars-move-down")}
            <span style={{ width: 20 }} />
            {smallButton("Delete", "🗑", () => setTree(deleteSelected(page.tree)), "toolbars-delete")}
          </div>
        </div>
        {/* bSizer6: the add button between the two lists */}
        <div style={{ display: "flex", alignItems: "center" }}>{smallButton("Add", "◀", () => add(shownEntries.find((e) => e.name === picked)), "toolbars-add")}</div>
        {/* bSizer9: the filter and the actions list */}
        <div style={{ flex: 1, minWidth: 0, display: "flex", flexDirection: "column", gap: 4 }}>
          <input type="search" placeholder="Filter actions" aria-label="Filter actions" data-testid="toolbars-filter" disabled={!enabled} value={filter} onChange={(e) => setFilter(e.target.value)} />
          <div role="listbox" aria-label="Actions" aria-disabled={!enabled} data-testid="toolbars-actions" style={{ height: 360, overflowY: "auto", border: "1px solid var(--chrome-border)", fontSize: 12, opacity: enabled ? 1 : 0.5 }}>
            {shownEntries.map((e) => (
              <div
                key={`${e.kind}:${e.name}`}
                role="option"
                aria-selected={picked === e.name}
                data-entry={e.name}
                title={e.tooltip || undefined}
                onClick={() => enabled && setPicked(e.name)}
                onDoubleClick={() => enabled && add(e)}
                style={{ display: "flex", alignItems: "center", gap: 6, padding: "1px 4px", cursor: enabled ? "pointer" : "default", background: picked === e.name ? "var(--chrome-accent-bg, rgba(74,163,255,0.2))" : undefined, whiteSpace: "nowrap" }}
              >
                {e.kind === "tool" ? <ActionIcon iconName={e.icon} size={16} /> : <span style={{ width: 16, flexShrink: 0 }} />}
                <span>{e.label}</span>
              </div>
            ))}
          </div>
        </div>
      </div>
      {insertMenu && (
        <ContextMenu
          x={insertMenu.x}
          y={insertMenu.y}
          onClose={() => setInsertMenu(null)}
          entries={[
            { label: "Insert Spacer", onSelect: () => setTree(insertItem(page.tree, "spacer")) },
            { label: "Insert Group", onSelect: () => setTree(insertItem(page.tree, "group")) },
          ]}
        />
      )}
    </div>
  );

  return {
    element,
    commit: () => {
      if (!visited) return;
      const shadow = page.current ? { ...page.shadow, [page.current]: parseToolbarTree(page.tree) } : page.shadow;
      setFrameToolbarSettings(frame, { custom: page.custom, toolbars: dedupeControls(shadow) });
    },
    cancel: () => {},
    reset: () =>
      setPage((p) => {
        const shadow = defaultShadowToolbars(defaults);
        const current = choices[0] ?? null;
        return { ...p, shadow, current, tree: current ? populateToolbarTree(shadow[current] ?? [], { frame, hasTool }) : EMPTY_TREE };
      }),
    resetLabel: "Reset Toolbars to Defaults",
    resetTooltip: "Reset all settings on this page to their default",
  };
}
