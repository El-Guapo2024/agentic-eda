// The grip on the right edge of a library tree's column header (`wxDATAVIEW_COL_RESIZABLE`, `doAddColumn`): dragged, the column takes the new width, which the
// tree keeps for the next time it is made (`LIB_TREE_MODEL_ADAPTER::SaveSettings`' `column_widths`; state/libTreeColumns.ts). UI-LEFT-2 LL4.
import { useRef } from "react";
import { setLibTreeColumnWidth, type LibTreeId } from "../../state/libTreeColumns";

export function ColumnResizer({ tree, header }: { tree: LibTreeId; header: string }) {
  const drag = useRef<{ x: number; w: number } | null>(null);
  return (
    <span
      data-column-resizer={header}
      role="separator"
      aria-orientation="vertical"
      onClick={(e) => e.stopPropagation()}
      onPointerDown={(e) => {
        e.preventDefault();
        e.stopPropagation();
        const cell = (e.currentTarget.parentElement as HTMLElement | null)?.getBoundingClientRect();
        drag.current = { x: e.clientX, w: cell?.width ?? 0 };
        try {
          e.currentTarget.setPointerCapture(e.pointerId);
        } catch {
          // a pointer the browser does not know (a synthetic one): the drag still follows this element's moves
        }
      }}
      onPointerMove={(e) => {
        const d = drag.current;
        if (d) setLibTreeColumnWidth(tree, header, d.w + e.clientX - d.x);
      }}
      onPointerUp={(e) => {
        drag.current = null;
        try {
          e.currentTarget.releasePointerCapture(e.pointerId);
        } catch {
          // not captured
        }
      }}
      style={{ position: "absolute", top: 0, right: 0, bottom: 0, width: 5, cursor: "col-resize", zIndex: 2 }}
    />
  );
}
