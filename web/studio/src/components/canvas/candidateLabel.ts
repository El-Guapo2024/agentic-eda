// The line the board's clarification menu lists an item by (`EDA_ITEM::GetItemDescription` in KiCad; the words are the studio's), shared by a click's menu
// (Canvas.tsx `disambiguate`) and the edit tools' (actions/useActionRunner.ts, `RequestSelection` -> `selectPoint` -> `doSelectionMenu`, UI-LEFT-2 PP2).
import type { BoardState } from "../../api/types";
import { padById } from "../../kicad-port/pcbItems";
import { extraItemLabel } from "../../kicad-port/pcbExtraItems";
import { fieldById } from "../../kicad-port/fpFields";
import { cellAddress, cellById, tableById } from "../../kicad-port/tables";
import type { SelectionCandidate } from "./selectionCandidates";

/** A human-readable label for a disambiguation-menu row. */
export function candidateLabel(board: BoardState | null | undefined, c: Pick<SelectionCandidate, "kind" | "id" | "layer">): string {
  switch (c.kind) {
    case "part": {
      const p = board?.parts.find((q) => q.ref === c.id);
      return `${c.id}${p?.value ? ` (${p.value})` : ""} [footprint]`;
    }
    case "track":
      return `Track [${board?.routing?.tracks.find((t) => t.id === c.id)?.layer ?? c.id}]`;
    case "via":
      return `Via`;
    case "zone":
      return `Zone [${board?.routing?.zones.find((z) => z.id === c.id)?.layer ?? c.id}]`;
    case "shape":
      return `${board?.drawings?.shapes.find((s) => s.id === c.id)?.kind ?? "Shape"} [${c.layer ?? ""}]`;
    case "text":
      return `Text "${board?.drawings?.texts.find((t) => t.id === c.id)?.content ?? ""}"`;
    case "dimension": {
      const kind = board?.drawings?.dimensions.find((d) => d.id === c.id)?.kind ?? "dimension";
      return `${kind[0]!.toUpperCase()}${kind.slice(1)} Dimension`;
    }
    case "pad": {
      const hit = board ? padById(board, c.id) : null;
      return hit ? `Pad ${hit.pad.num} of ${hit.part.ref}${hit.pad.net ? ` (${hit.pad.net})` : ""}` : `Pad ${c.id}`;
    }
    case "field": {
      const hit = board ? fieldById(board, c.id) : null;
      return hit ? `${hit.field.name} field of ${hit.part.ref}${hit.field.name === "Reference" ? "" : ` (${hit.field.text})`}` : `Field ${c.id}`;
    }
    // `PCB_TABLE::GetItemDescription`, `PCB_TABLECELL::GetItemDescription`.
    case "table": {
      const t = board ? tableById(board, c.id) : undefined;
      return t ? `${t.cols} column table` : `Table ${c.id}`;
    }
    case "tablecell": {
      const hit = board ? cellById(board, c.id) : undefined;
      return hit ? `Table cell ${cellAddress(hit.cell.row, hit.cell.col)}` : `Table cell ${c.id}`;
    }
    default:
      // The text boxes, images, points and barcodes (`GetItemDescription`).
      return extraItemLabel(board, c.id);
  }
}
