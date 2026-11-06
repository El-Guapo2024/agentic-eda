// The preview of Import Graphics' interactive placement (`DRAWING_TOOL::PlaceImportedGraphics`: `m_view->Add( &preview )`): the items the import would put on the board,
// drawn under the cursor until a click places them (`commit.Push( _( "Import Graphics" ) )`) or Escape drops them; nothing is on the board meanwhile. The items come from the
// server (`POST /api/import_graphics/preview`) as the board state has them, at the offset the placement moves from; `reference` is the point they are held by (the
// preview's top left item's position), which goes to the snapped cursor. Plain module state with subscribers (the canvas paints from it); actions set it.
import { useSyncExternalStore } from "react";
import type { PointXY } from "../api/types";

export interface ImportPreview {
  /** The shapes and texts as `BoardState.drawings` has them. */
  shapes: unknown[];
  texts: unknown[];
  reference: PointXY;
}

let current: ImportPreview | null = null;
const listeners = new Set<() => void>();

export function getImportPreview(): ImportPreview | null {
  return current;
}

export function setImportPreview(next: ImportPreview | null): void {
  current = next;
  for (const l of [...listeners]) l();
}

function subscribe(listener: () => void): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

export function useImportPreview(): ImportPreview | null {
  return useSyncExternalStore(subscribe, getImportPreview, getImportPreview);
}
