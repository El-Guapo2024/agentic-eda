// Pure pieces of two small "text" actions.
//
// Copy as Text -- EDIT_TOOL::copyToClipboardAsText (pcbnew/tools/edit_tool.cpp)
// and SCH_EDITOR_CONTROL::CopyAsText + GetSelectedItemsAsText
// (eeschema/tools/sch_tool_utils.cpp): every selected item's text, each
// trimmed (`Trim(false).Trim(true)`), empties dropped, joined with '\n'.
// Items with no text representation contribute "" and are skipped.
//
// On the schematic `GetSchItemAsText` gives the text of a text, a label of any kind, a sheet pin, a field, a text box and a pin (its name); every other item
// contributes nothing ([`schSelectionTexts`]).
//
// Show Datasheet -- SCH_INSPECTION_TOOL::ShowDatasheet
// (eeschema/tools/sch_inspection_tool.cpp): `datasheet.IsEmpty() ||
// datasheet == "~"` -> "No datasheet defined."; otherwise
// GetAssociatedDocument() launches it. Here only an absolute http(s)/file
// URL can be launched from a browser; anything else (a bare file name that
// KiCad would resolve through the project search stack) cannot.

import type { Schematic } from "../api/types";
import { findField, shownText } from "./schFieldEdit";
import { pinById } from "./schPins";
import { sheetPinById } from "./schSheetPinItems";

/**
 * `GetSchItemAsText` (eeschema/tools/sch_tool_utils.cpp) of each selected item, in selection order: the text of a free text and of every kind of label (`GetShownText( true )`), of a sheet
 * pin, of a field (`Name: value` when its name is shown), of a text box, and a pin's name (`GetShownName()`, "~" being none). An item of any other kind has no text here, and
 * contributes nothing. The shown text is the stored one: text variables (`${...}`) are expanded by the code that owns them.
 */
export function schSelectionTexts(sch: Schematic, ids: readonly string[]): string[] {
  const out: string[] = [];
  for (const id of ids) {
    const label = sch.labels.find((l) => l.id === id);
    if (label) {
      out.push(label.net);
      continue;
    }
    const text = sch.texts.find((t) => t.id === id);
    if (text) {
      out.push(text.content);
      continue;
    }
    const sheetPin = sheetPinById(sch, id);
    if (sheetPin) {
      out.push(sheetPin.name);
      continue;
    }
    const field = findField(sch, id);
    if (field) {
      out.push(shownText(field.field));
      continue;
    }
    const graphic = sch.graphics?.find((g) => g.id === id);
    if (graphic) {
      if (graphic.shape.type === "text_box") out.push(graphic.shape.text);
      continue;
    }
    const pin = pinById(sch, id);
    if (pin) out.push(pin.name === "~" ? "" : (pin.name ?? ""));
  }
  return out;
}

export function selectionAsText(texts: ReadonlyArray<string | null | undefined>): string {
  return texts
    .map((t) => (t ?? "").trim())
    .filter((t) => t !== "")
    .join("\n");
}

export type DatasheetTarget = { kind: "none" } | { kind: "url"; url: string } | { kind: "unresolvable"; text: string };

export function datasheetTarget(field: string | null | undefined): DatasheetTarget {
  const text = (field ?? "").trim();
  if (text === "" || text === "~") return { kind: "none" };
  if (/^(https?|file):\/\//i.test(text)) return { kind: "url", url: text };
  // GetAssociatedDocument also accepts "www.x.com/..." style hosts.
  if (/^www\./i.test(text)) return { kind: "url", url: `https://${text}` };
  return { kind: "unresolvable", text };
}
