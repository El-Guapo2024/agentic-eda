// The tree of a chooser (`LIB_TREE`'s wxDataViewCtrl): a row per library, per symbol of an open library and, under an open multi-unit symbol, per unit,
// in the Item column and the columns Select Columns... shows (Description by default; a right click on the header offers the chooser,
// `LIB_TREE::onHeaderContextMenu`, kicad-port/libTreeColumns.ts). The rows are kicad-port/libChooser.ts's `visibleRows`; this draws them and tells the
// dialog what was clicked.
// A click selects, a double click (or Enter) chooses an item and opens or folds a library, a click on the arrow does the same without choosing. The
// context menu of a library row pins or unpins it (`LIB_TREE_MODEL_ADAPTER::PinLibrary`).
import { useEffect, useMemo, useRef, useState } from "react";
import { PIN_GLYPH } from "../../kicad-port/libraryTreeState";
import { availableColumns, cellText, columnsTemplate, columnsWidth, extraFieldsOf, SELECT_COLUMNS_ACTION, SELECT_COLUMNS_LABEL, treeColumns } from "../../kicad-port/libTreeColumns";
import { setLibTreeColumns, useLibTreeColumnWidths, useLibTreeColumns } from "../../state/libTreeColumns";
import { ColumnResizer } from "../library/ColumnResizer";
import { SelectColumnsDialog } from "../library/SelectColumnsDialog";
import type { ChooserRow } from "../../kicad-port/libChooser";
import { unitLetter } from "../../kicad-port/unitLetter";
import { setLibraryPinned, type TreeKind } from "../../state/libraryTree";
import { ContextMenu, type MenuEntry } from "../canvas/ContextMenu";

export interface LibChooserTreeProps {
  kind: TreeKind;
  rows: readonly ChooserRow[];
  selectedKey: string | null;
  loading: ReadonlySet<string>;
  onSelect: (key: string) => void;
  /** Enter / double click on an item or unit. */
  onChoose: (row: ChooserRow) => void;
  onToggleGroup: (lib: string) => void;
  onToggleItem: (key: string) => void;
  /** The empty tree says why (nothing installed, no match). */
  emptyText: string;
}

export function LibChooserTree({ kind, rows, selectedKey, loading, onSelect, onChoose, onToggleGroup, onToggleItem, emptyText }: LibChooserTreeProps) {
  const holder = useRef<HTMLDivElement>(null);
  const [menu, setMenu] = useState<{ x: number; y: number; entries: MenuEntry[] } | null>(null);
  const treeId = kind === "symbol" ? "symbol_chooser" : "footprint_chooser";
  const shown = useLibTreeColumns(treeId);
  // the dialog is this tree's own (a chooser is opened from its header's menu only; KiCad's action has no handler of its own there)
  const [choosing, setChoosing] = useState(false);
  const columns = useMemo(() => treeColumns(shown), [shown]);
  const extra = columns.slice(1);
  // `addColumnIfNecessary` for each library loaded: the fields of its items
  const available = useMemo(() => availableColumns(rows.filter((r) => r.kind === "group").map((r) => (r.kind === "group" ? extraFieldsOf(r.group.items) : []))), [rows]);
  // each column at its kept width (`column_widths`; Item 300 and Description 600 at first), the last one filling the rest (`FinishTreeInitialization`)
  const widths = useLibTreeColumnWidths(treeId);
  const grid = { gridTemplateColumns: columnsTemplate(columns, widths), minWidth: columnsWidth(columns, widths) };

  // The selected row stays in view as the arrow keys move it.
  useEffect(() => {
    if (!selectedKey) return;
    const el = holder.current?.querySelector<HTMLElement>(`[data-key="${CSS.escape(selectedKey)}"]`);
    el?.scrollIntoView({ block: "nearest" });
  }, [selectedKey]);

  return (
    <div className="chooser-tree" ref={holder} role="tree" aria-label="Libraries" data-chooser-tree={kind}>
      <div
        className="chooser-header"
        style={grid}
        data-chooser-header
        onContextMenu={(e) => {
          e.preventDefault();
          setMenu({ x: e.clientX, y: e.clientY, entries: [{ label: SELECT_COLUMNS_LABEL, hint: SELECT_COLUMNS_ACTION, onSelect: () => setChoosing(true) }] });
        }}
      >
        {columns.map((c) => (
          <div key={c} data-column={c} style={{ position: "relative" }}>
            {c}
            <ColumnResizer tree={treeId} header={c} />
          </div>
        ))}
      </div>
      {rows.length === 0 && <div style={{ padding: "10px 12px", color: "var(--chrome-text-dim)" }}>{emptyText}</div>}
      {rows.map((row) => {
        const selected = row.key === selectedKey;
        const common = { "data-key": row.key, "aria-selected": selected, className: `chooser-row ${row.kind}${selected ? " selected" : ""}`, style: grid } as const;
        if (row.kind === "group") {
          const g = row.group;
          const count = loading.has(g.source ?? g.lib) ? "…" : g.count !== undefined ? String(g.count) : g.items.length > 0 ? String(g.items.length) : "";
          return (
            <div
              key={row.key}
              role="treeitem"
              aria-expanded={row.open}
              data-library={g.lib}
              {...common}
              onClick={() => {
                onSelect(row.key);
                onToggleGroup(g.lib);
              }}
              onContextMenu={(e) => {
                e.preventDefault();
                onSelect(row.key);
                if (g.pseudo) return;
                setMenu({ x: e.clientX, y: e.clientY, entries: [{ label: g.pinned ? "Unpin Library" : "Pin Library", onSelect: () => setLibraryPinned(kind, g.lib, !g.pinned) }] });
              }}
            >
              <div title={g.lib}>
                <span className="twisty">{row.open ? "▾" : "▸"}</span>
                {g.pinned ? PIN_GLYPH : ""}
                {g.lib} {count !== "" && <span className="dim">({count})</span>}
              </div>
              {extra.map((c) => (
                <div key={c} title={c === "Description" ? g.description : undefined}>
                  {c === "Description" ? (g.description ?? "") : ""}
                </div>
              ))}
            </div>
          );
        }
        if (row.kind === "item") {
          return (
            <div
              key={row.key}
              role="treeitem"
              aria-expanded={row.expandable ? row.open : undefined}
              data-id={row.item.id}
              {...common}
              onClick={() => onSelect(row.key)}
              onDoubleClick={() => onChoose(row)}
            >
              <div title={row.item.id}>
                <span
                  className="twisty"
                  onClick={
                    row.expandable
                      ? (e) => {
                          e.stopPropagation();
                          onSelect(row.key);
                          onToggleItem(row.key);
                        }
                      : undefined
                  }
                >
                  {row.expandable ? (row.open ? "▾" : "▸") : ""}
                </span>
                {row.item.name}
              </div>
              {extra.map((c) => {
                const text = cellText(row.item, c);
                return (
                  <div key={c} title={text} data-column={c}>
                    {text}
                  </div>
                );
              })}
            </div>
          );
        }
        return (
          <div key={row.key} role="treeitem" data-id={row.item.id} data-unit={row.unit} {...common} onClick={() => onSelect(row.key)} onDoubleClick={() => onChoose(row)}>
            <div>Unit {unitLetter(row.unit)}</div>
            {extra.map((c) => (
              <div key={c} />
            ))}
          </div>
        );
      })}
      {menu && <ContextMenu x={menu.x} y={menu.y} entries={menu.entries} onClose={() => setMenu(null)} />}
      {choosing && <SelectColumnsDialog available={available} shown={shown} onOk={(c) => setLibTreeColumns(treeId, c)} onClose={() => setChoosing(false)} />}
    </div>
  );
}
