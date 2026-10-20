// The Design Blocks pane (`DESIGN_BLOCK_PANE` with `PANEL_DESIGN_BLOCK_CHOOSER`; common/widgets/design_block_pane.cpp, panel_design_block_chooser.cpp,
// eeschema/widgets/sch_design_block_pane.cpp, pcbnew/widgets/pcb_design_block_pane.cpp): the search box and the tree of the design block libraries, the preview of the selected
// block under it with its description, keywords and default fields, and the check boxes that say how a block is placed -- Place repeated copies, Place as group, Place as sheet
// and Keep annotations on the sheet; Place repeated copies and Place as group on the board (the board's Keep annotations box does nothing in KiCad, `AppendBoard` never reads it).
//
// A double click on a block (or Enter) places it; the context menu of a row has KiCad's entries (`DESIGN_BLOCK_CONTROL::AddContextMenuItems` and the editors' controls): Pin or
// Unpin Library, New Library..., Place Design Block, Properties..., Save Current Sheet / Board as Design Block..., Save Selection as Design Block..., Update Design Block from
// Current Sheet / Board and from Selection, Delete Design Block, Hide Library Tree. What each does is `actions/designBlockActions.ts`'s; the rules of the tree are
// kicad-port/designBlocks.ts's.
import { useEffect, useMemo, useRef, useState } from "react";
import { blockContextMenu, blockInfo, composeBlockGroups, findBlock, parseBlockId, type BlockMenuId } from "../../kicad-port/designBlocks";
import { initialSelection, itemKey, moveSelection, visibleRows, groupKey, type ChooserRow } from "../../kicad-port/libChooser";
import { PIN_GLYPH } from "../../kicad-port/libraryTreeState";
import { addToHistory, ensureDesignBlocks, refreshDesignBlocks, selectDesignBlock, setDesignBlockSearch, setPlaceOptions, useDesignBlocksUi, type BlockEditor } from "../../state/designBlocks";
import { useActionRunner } from "../../actions/useActionRunner";
import { ContextMenu, type MenuEntry } from "../canvas/ContextMenu";
import { BlockPreview } from "./BlockPreview";
import "../../styles/designBlocks.css";

/** The action names of the pane's entries, per editor (`SCH_ACTIONS`, `PCB_ACTIONS`). */
const ACTIONS: Record<BlockEditor, Record<"place" | "properties" | "saveSheet" | "saveSelection" | "updateSheet" | "updateSelection" | "delete", string>> = {
  schematic: {
    place: "eeschema.InteractiveDrawing.placeDesignBlock",
    properties: "eeschema.SchDesignBlockControl.editDesignBlockProperties",
    saveSheet: "eeschema.SchDesignBlockControl.saveSheetAsDesignBlock",
    saveSelection: "eeschema.SchDesignBlockControl.saveSelectionAsDesignBlock",
    updateSheet: "eeschema.SchDesignBlockControl.updateDesignBlockFromSheet",
    updateSelection: "eeschema.SchDesignBlockControl.updateDesignBlockFromSelection",
    delete: "eeschema.SchDesignBlockControl.deleteDesignBlock",
  },
  pcb: {
    place: "pcbnew.InteractiveDrawing.placeDesignBlock",
    properties: "pcbnew.PcbDesignBlockControl.editDesignBlockProperties",
    saveSheet: "pcbnew.PcbDesignBlockControl.saveBoardAsDesignBlock",
    saveSelection: "pcbnew.PcbDesignBlockControl.saveSelectionAsDesignBlock",
    updateSheet: "pcbnew.PcbDesignBlockControl.updateDesignBlockFromBoard",
    updateSelection: "pcbnew.PcbDesignBlockControl.updateDesignBlockFromSelection",
    delete: "pcbnew.PcbDesignBlockControl.deleteDesignBlock",
  },
};

export function DesignBlocksPane({ editor, hasSelection }: { editor: BlockEditor; hasSelection: boolean }) {
  const ui = useDesignBlocksUi();
  const { run } = useActionRunner();
  const names = ACTIONS[editor];
  const [openLibs, setOpenLibs] = useState<ReadonlySet<string>>(new Set());
  const [menu, setMenu] = useState<{ x: number; y: number; entries: MenuEntry[] } | null>(null);
  const holder = useRef<HTMLDivElement>(null);

  useEffect(() => {
    void ensureDesignBlocks();
  }, []);

  const libraries = ui.reply?.libraries ?? [];
  const searching = ui.search.trim() !== "";
  const groups = useMemo(() => composeBlockGroups({ libraries, query: ui.search, history: ui.history, pinned: ui.pinned }), [libraries, ui.search, ui.history, ui.pinned]);
  const rows = useMemo(() => visibleRows(groups, { groups: openLibs, items: new Set(), searching }, "footprint"), [groups, openLibs, searching]);

  // the selected row: the block (or library) the store holds, in the first real library that has it
  const selectedKey = useMemo(() => {
    const id = ui.selected;
    if (!id) return null;
    const { lib, name } = parseBlockId(id);
    if (name === "") return rows.find((r) => r.kind === "group" && r.group.lib === lib)?.key ?? null;
    const real = rows.find((r) => r.kind === "item" && r.item.id === id && !r.group.pseudo);
    return (real ?? rows.find((r) => r.kind === "item" && r.item.id === id))?.key ?? null;
  }, [ui.selected, rows]);

  // after a search the best match is selected and its library opened (`showResults`); with none, a selected block stays
  const lastQuery = useRef("");
  useEffect(() => {
    if (!searching || lastQuery.current === ui.search) return;
    lastQuery.current = ui.search;
    const sel = initialSelection(groups, ui.search, null);
    if (sel.select) {
      const row = rows.find((r) => r.key === sel.select);
      if (row && row.kind === "item") selectDesignBlock(row.item.id);
    }
  }, [searching, ui.search, groups, rows]);
  useEffect(() => {
    if (!searching) lastQuery.current = "";
  }, [searching]);

  // a selected block's library is open (a block just saved, a library just made, a block placed again from a linked group)
  useEffect(() => {
    const { lib } = parseBlockId(ui.selected);
    if (lib && parseBlockId(ui.selected).name !== "") setOpenLibs((o) => (o.has(lib) ? o : new Set([...o, lib])));
  }, [ui.selected]);

  // the selected row stays in view as the arrow keys move it
  useEffect(() => {
    if (!selectedKey) return;
    holder.current?.querySelector<HTMLElement>(`[data-key="${CSS.escape(selectedKey)}"]`)?.scrollIntoView({ block: "nearest" });
  }, [selectedKey]);

  const block = findBlock(libraries, ui.selected);
  const { lib: selectedLib } = parseBlockId(ui.selected);

  const toggle = (lib: string) => setOpenLibs((o) => (o.has(lib) ? new Set([...o].filter((x) => x !== lib)) : new Set([...o, lib])));
  const choose = (row: ChooserRow) => {
    if (row.kind === "group") return toggle(row.group.lib);
    // `PANEL_DESIGN_BLOCK_CHOOSER::onDesignBlockChosen`: the selection handler places the block and it goes in the history
    selectDesignBlock(row.item.id);
    addToHistory(row.item.id);
    run(names.place);
  };

  const rowMenu = (e: React.MouseEvent, row: ChooserRow) => {
    e.preventDefault();
    const pseudo = row.kind === "group" && !!row.group.pseudo;
    // the row under the pointer is the current node (`getCurrentTreeNode`): the menu acts on it
    selectDesignBlock(row.kind === "group" ? (pseudo ? null : `${row.group.lib}:`) : row.kind === "item" ? row.item.id : null);
    const items = blockContextMenu(editor, { kind: row.kind === "group" ? "library" : "item", pinned: row.kind === "group" && row.group.pinned, pseudo }, hasSelection);
    const actionOf: Record<BlockMenuId, string> = {
      pin: "common.Control.pinLibrary",
      unpin: "common.Control.unpinLibrary",
      newLibrary: "common.Control.newLibrary",
      place: names.place,
      properties: names.properties,
      saveSheet: names.saveSheet,
      saveSelection: names.saveSelection,
      updateSheet: names.updateSheet,
      updateSelection: names.updateSelection,
      delete: names.delete,
      hide: "common.Control.hideLibraryTree",
    };
    const entries: MenuEntry[] = items.map((i) => (i.separator ? { label: "", separator: true, onSelect: () => {} } : { label: i.label, onSelect: () => run(actionOf[i.id!]) }));
    setMenu({ x: e.clientX, y: e.clientY, entries });
  };

  const onKeyDown = (e: React.KeyboardEvent) => {
    if (e.target instanceof HTMLInputElement && e.key !== "ArrowDown" && e.key !== "ArrowUp" && e.key !== "Enter") return;
    const move = (d: number) => {
      e.preventDefault();
      const key = moveSelection(rows, selectedKey, d);
      const row = rows.find((r) => r.key === key);
      if (row) selectDesignBlock(row.kind === "group" ? `${row.group.lib}:` : row.item.id);
    };
    if (e.key === "ArrowDown") move(1);
    else if (e.key === "ArrowUp") move(-1);
    else if (e.key === "ArrowRight" || e.key === "ArrowLeft") {
      const row = rows.find((r) => r.key === selectedKey);
      if (row?.kind === "group") {
        e.preventDefault();
        setOpenLibs((o) => (e.key === "ArrowRight" ? new Set([...o, row.group.lib]) : new Set([...o].filter((x) => x !== row.group.lib))));
      }
    } else if (e.key === "Enter") {
      const row = rows.find((r) => r.key === selectedKey);
      if (row) {
        e.preventDefault();
        choose(row);
      }
    }
  };

  const optionBox = (label: string, tip: string, key: "repeated" | "asGroup" | "asSheet" | "keepAnnotations") => (
    <label title={tip} key={key}>
      <input type="checkbox" data-testid={`db-opt-${key}`} checked={ui.options[key]} onChange={(e) => setPlaceOptions({ [key]: e.target.checked })} /> {label}
    </label>
  );

  const problems = ui.reply?.problems ?? [];
  const notes = ui.reply?.notes ?? [];
  const emptyText = ui.error
    ? ui.error
    : ui.loading && !ui.reply
      ? "Loading design block libraries..."
      : libraries.length === 0
        ? "There are no design block libraries. Preferences > Manage Design Block Libraries... adds one, or right-click here and choose New Library..."
        : searching
          ? "Nothing matches the search."
          : "";

  return (
    <div className="db-pane" data-testid="db-pane" data-editor={editor} onKeyDown={onKeyDown}>
      <div className="db-search">
        <input
          type="search"
          aria-label="Search design blocks"
          data-testid="db-search"
          placeholder="Filter"
          value={ui.search}
          spellCheck={false}
          onChange={(e) => setDesignBlockSearch(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Escape" && ui.search !== "") {
              // the first Escape clears the search (`PANEL_DESIGN_BLOCK_CHOOSER::OnChar`)
              e.preventDefault();
              e.stopPropagation();
              setDesignBlockSearch("");
            }
          }}
        />
        <button title="Read the libraries again" aria-label="Refresh libraries" onClick={() => void refreshDesignBlocks()}>
          {"⟳"}
        </button>
      </div>
      <div className="chooser-tree db-tree" ref={holder} role="tree" aria-label="Design block libraries" data-chooser-tree="design_block" tabIndex={0}>
        <div className="chooser-header">
          <div>Item</div>
          <div>Description</div>
        </div>
        {rows.length === 0 && <div className="db-empty">{emptyText}</div>}
        {rows.map((row) => {
          const selected = row.key === selectedKey;
          const common = { "data-key": row.key, "aria-selected": selected, className: `chooser-row ${row.kind}${selected ? " selected" : ""}` } as const;
          if (row.kind === "group") {
            const g = row.group;
            return (
              <div
                key={row.key}
                role="treeitem"
                aria-expanded={row.open}
                data-library={g.lib}
                {...common}
                onClick={() => {
                  selectDesignBlock(g.pseudo ? null : `${g.lib}:`);
                  toggle(g.lib);
                }}
                onContextMenu={(e) => rowMenu(e, row)}
              >
                <div title={g.lib}>
                  <span className="twisty">{row.open ? "▾" : "▸"}</span>
                  {g.pinned ? PIN_GLYPH : ""}
                  {g.lib} {g.count !== undefined && !g.pseudo && <span className="dim">({g.count})</span>}
                </div>
                <div title={g.description}>{g.description ?? ""}</div>
              </div>
            );
          }
          if (row.kind !== "item") return null;
          return (
            <div key={row.key} role="treeitem" data-id={row.item.id} {...common} onClick={() => selectDesignBlock(row.item.id)} onDoubleClick={() => choose(row)} onContextMenu={(e) => rowMenu(e, row)}>
              <div title={row.item.id}>
                <span className="twisty" />
                {row.item.name}
              </div>
              <div title={row.item.description}>{row.item.description}</div>
            </div>
          );
        })}
      </div>
      <div className="db-details">
        {block ? (
          <>
            <BlockPreview lib={selectedLib} name={block.name} what={editor === "schematic" ? "sch" : "pcb"} has={editor === "schematic" ? block.schematic : block.board} />
            <div className="db-info" data-testid="db-info">
              <b>{blockInfo(block).name}</b>
              {block.description && <div>{block.description}</div>}
              {block.keywords && <div className="dim">Keywords: {block.keywords}</div>}
              {block.fields.length > 0 && (
                <table>
                  <tbody>
                    {block.fields.map((f) => (
                      <tr key={f.name}>
                        <td>
                          <b>{f.name}</b>
                        </td>
                        <td>{f.value}</td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              )}
              <div className="dim">{[block.schematic ? "schematic" : null, block.board ? "layout" : null].filter(Boolean).join(" and ") || "no design"}</div>
            </div>
          </>
        ) : (
          <div className="db-info dim">{selectedLib ? `Library ${selectedLib}.` : "Select a design block to see it."}</div>
        )}
      </div>
      <div className="db-options">
        {optionBox("Place repeated copies", "Place copies of the design block on subsequent clicks.", "repeated")}
        {optionBox("Place as group", "Place the design block as a group.", "asGroup")}
        {editor === "schematic" && optionBox("Place as sheet", "Place the design block as a new sheet.", "asSheet")}
        {editor === "schematic" && optionBox("Keep annotations", "Preserve reference designators in the source schematic. Otherwise, clear then reannotate according to settings.", "keepAnnotations")}
      </div>
      {(problems.length > 0 || notes.length > 0) && (
        <details className="db-problems" data-testid="db-problems">
          <summary>{problems.length + notes.length} thing(s) could not be read</summary>
          {notes.map((n) => (
            <div key={n}>{n}</div>
          ))}
          {problems.map((p, i) => (
            <div key={i}>{`${p.nickname}${p.block ? `:${p.block}` : ""}: ${p.error}`}</div>
          ))}
        </details>
      )}
      {menu && <ContextMenu x={menu.x} y={menu.y} entries={menu.entries} onClose={() => setMenu(null)} />}
    </div>
  );
}

// the group row's key and an item's key, for the tests that look rows up
export { groupKey, itemKey };
