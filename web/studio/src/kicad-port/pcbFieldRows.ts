// The Properties panel's "Fields" group buttons on board footprints (KiCad 25ec2335 pcbnew/widgets/pcb_properties_panel.cpp):
//
//   addBlankField     "Add Field": `GetUserFieldName( n, UNTRANSLATED )` (`Field%d`), the first `n` no selected footprint has a field of (`HasField`), added to each
//                     selected footprint as a user field with an empty text, hidden (`SetVisible( false )`); then its name is edited, blank
//   isKeyEditable     a row's name can be edited and the row removed when the front footprint has the field and it is not mandatory (nor private, which no board field is)
//   onKeyRenamed      "Rename Property": each footprint with the field renames it (`field->SetName( aNewName )`)
//   removeField       "Remove Field": each footprint with the field, not mandatory, loses it (`footprint->Remove( field )`)
//
// A footprint's user fields are edited as Footprint Properties edits them, the whole list in one `edit_board_footprint` per footprint (the symbol's fields are left alone,
// as KiCad's panel leaves the schematic; Update Schematic from PCB carries them). One batch, one undo step. Pure: no React, no DOM.
import type { Cmd, Part, TextGraphicsDefaults } from "../api/types";
import { fieldsOf, layoutOf, newFieldLayout } from "./fpFields";

/** `FIELD::IsMandatory` of a footprint's field. */
export const MANDATORY_FIELDS: ReadonlySet<string> = new Set(["Reference", "Value", "Datasheet", "Description"]);

type UserField = { name: string; text: string; layout: ReturnType<typeof layoutOf> };

/** The user-field list `edit_board_footprint` takes for `part`, as Footprint Properties sends it (a Datasheet or Description the footprint holds a copy of is in it). */
export function userFieldList(part: Part): UserField[] {
  return fieldsOf(part)
    .filter((f) => f.name !== "Reference" && f.name !== "Value" && (!(f.name === "Datasheet" || f.name === "Description") || f.custom))
    .map((f) => ({ name: f.name, text: f.text, layout: layoutOf(f) }));
}

const hasField = (part: Part, name: string): boolean => fieldsOf(part).some((f) => f.name === name);

/** `addBlankField`'s name: `Field%d`, the first `n` none of the footprints has. */
export function newBlankFieldName(parts: readonly Part[]): string {
  for (let n = 0; ; n++) {
    const name = `Field${n}`;
    if (!parts.some((p) => hasField(p, name))) return name;
  }
}

/** `isKeyEditable` for the front footprint. */
export function fieldKeyEditable(front: Part | undefined, name: string): boolean {
  return !!front && hasField(front, name) && !MANDATORY_FIELDS.has(name);
}

/** "Add Field": the field, empty and hidden, on every footprint. */
export function addFieldCmds(parts: readonly Part[], name: string, defaults?: TextGraphicsDefaults): Cmd[] {
  return parts.map((p) => ({ op: "edit_board_footprint", part: p.ref, fields: [...userFieldList(p), { name, text: "", layout: { ...newFieldLayout(p, defaults), visible: false } }] }) as Cmd);
}

/** "Remove Field": the field off every footprint that has it (a mandatory one stays). */
export function removeFieldCmds(parts: readonly Part[], name: string): Cmd[] {
  if (MANDATORY_FIELDS.has(name)) return [];
  return parts.filter((p) => hasField(p, name)).map((p) => ({ op: "edit_board_footprint", part: p.ref, fields: userFieldList(p).filter((f) => f.name !== name) }) as Cmd);
}

/** `onKeyRenamed`: every footprint with the field gives it the new name, its text and look kept. */
export function renameFieldCmds(parts: readonly Part[], from: string, to: string): Cmd[] {
  if (MANDATORY_FIELDS.has(from)) return [];
  return parts.filter((p) => hasField(p, from)).map((p) => ({ op: "edit_board_footprint", part: p.ref, fields: userFieldList(p).map((f) => (f.name === from ? { ...f, name: to } : f)) }) as Cmd);
}

/**
 * `PROPERTIES_PANEL::onPropertyLabelEdited` for a name typed in a row's label: for the row just added (`pending`) a name that is empty or in use removes it
 * (`onNewItemLeftBlank`), another renames it; for an existing row an empty or used name keeps the label (nothing), another renames it. `inUse` is
 * `isKeyNameInUse` (a property of the front item has the name -- its own name too).
 */
export function labelOutcome(newName: string, pending: boolean, inUse: (name: string) => boolean): "remove" | "rename" | null {
  const refused = newName.trim() === "" || inUse(newName);
  if (pending) return refused ? "remove" : "rename";
  return refused ? null : "rename";
}
