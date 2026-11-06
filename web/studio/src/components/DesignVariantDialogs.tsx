// The design variant dialogs of the schematic editor (a KiCad 10 feature, ported from KiCad 25ec2335):
//   * New Design Variant -- `SCH_EDIT_FRAME::ShowAddVariantDialog` (eeschema/toolbars_sch_editor.cpp): "Variant name:" and "Description (optional):";
//   * Remove Design Variant... -- `RemoveVariant` (eeschema/sch_edit_frame.cpp): a single-choice dialog, "Select variant name to remove:";
//   * Edit Variant Description... -- `EditVariantDescription`: "Select variant to edit description:", then "Edit Description for '%s'" with "Description:";
//   * Rename Design Variant... -- `RenameVariant`: "Select variant to rename:", then "Enter new variant name:";
//   * Copy Design Variant... -- `CopyVariant`: "Select variant to copy:", then "Enter name for the copied variant:" (the source's name with "_copy").
// The Symbol Fields Table's buttons open the same dialogs on the variant its list has selected, skipping the choice (`onRenameVariant`,
// `onCopyVariant`, `onEditVariantDescription`); its Copy checks the name as the table does (`validateCopyNameInTable`).
//
// A name is trimmed (`Trim().Trim( false )`) and checked as `ValidateNewVariantName` checks it after the dialog closes: a refused name is said on the info
// bar (`ShowMessageFor( .., 10000, wxICON_ERROR )`, the studio's toast) and nothing changes. Each change is one undoable verb (`add_design_variant`, ...),
// and the current variant follows as KiCad's frame makes it follow: the new variant after Add and Copy, the new name after renaming the current one,
// the default after removing the current one.
import { useEffect, useRef, useState } from "react";
import { useSchControlDispatch, useSchControlState, type VariantDialogAction } from "../state/schControlStore";
import { useStudioApi, useStudioDispatch, useStudioState } from "../state/store";
import { trimName, validateCopyNameInTable, validateNewVariantName, variantChoices, variantDescription, variantNamesForUI } from "../kicad-port/designVariants";
import { SchDialogShell } from "./SchDialogShell";

/** The info bar's error for ten seconds (`ShowMessageFor( msg, 10000, wxICON_ERROR )`); an information one for the "nothing to rename / copy" notes. */
type Say = (message: string, kind?: "error" | "info") => void;

/** The first dialog of each action: its title and the single-choice prompt (the spaces KiCad pads the prompt with only widen the dialog). */
const CHOOSER: Record<Exclude<VariantDialogAction, "add">, { title: string; prompt: string }> = {
  remove: { title: "Remove Design Variant", prompt: "Select variant name to remove:" },
  describe: { title: "Edit Variant Description", prompt: "Select variant to edit description:" },
  rename: { title: "Rename Design Variant", prompt: "Select variant to rename:" },
  copy: { title: "Copy Design Variant", prompt: "Select variant to copy:" },
};

/** What an action says when there is no variant to act on (`RemoveVariant` has no such check: its chooser is just empty). */
const NONE_TO: Partial<Record<VariantDialogAction, { message: string; kind: "error" | "info" }>> = {
  describe: { message: "No design variants to edit.", kind: "error" },
  rename: { message: "No variants to rename.", kind: "info" },
  copy: { message: "No variants to copy.", kind: "info" },
};

export function DesignVariantDialogs() {
  const { dialog } = useSchControlState();
  const controlDispatch = useSchControlDispatch();
  const dispatch = useStudioDispatch();
  const state = useStudioState();
  if (dialog?.kind !== "variant") return null;
  const close = () => controlDispatch({ type: "CLOSE_DIALOG" });
  const say: Say = (message, kind = "error") => dispatch({ type: "TOAST", message, kind, ms: 10000 });
  const variants = state.schematic?.variants ?? [];
  if (dialog.action === "add") return <AddVariantDialog key="add" onClose={close} say={say} />;
  // the choice of the variant, unless the caller named it
  if (dialog.name === undefined) {
    const choices = variantChoices(variants);
    const none = NONE_TO[dialog.action];
    if (choices.length === 0 && none) {
      // shown once, as the frame shows it before any dialog
      return <SayAndClose message={none.message} kind={none.kind} say={say} onClose={close} />;
    }
    const c = CHOOSER[dialog.action];
    return <SingleChoiceDialog key={dialog.action} title={c.title} prompt={c.prompt} choices={choices} onCancel={close} onChoose={(name) => (name === "" ? close() : controlDispatch({ type: "OPEN_DIALOG", dialog: { ...dialog, name } }))} />;
  }
  const name = dialog.name;
  switch (dialog.action) {
    case "remove":
      return <RemoveVariant key={`remove:${name}`} name={name} onClose={close} />;
    case "describe":
      return <DescriptionDialog key={`describe:${name}`} name={name} initial={variantDescription(variants, name)} onClose={close} />;
    case "rename":
      return <RenameDialog key={`rename:${name}`} name={name} onClose={close} say={say} />;
    case "copy":
      return <CopyDialog key={`copy:${name}`} name={name} fromTable={!!dialog.fromTable} onClose={close} say={say} />;
  }
}

/** Says `message` and closes the dialog slot (the frame's guard before any dialog opens). */
function SayAndClose({ message, kind, say, onClose }: { message: string; kind: "error" | "info"; say: Say; onClose: () => void }) {
  const done = useRef(false);
  useEffect(() => {
    if (done.current) return;
    done.current = true;
    say(message, kind);
    onClose();
  }, [message, kind, say, onClose]);
  return null;
}

/** `wxSingleChoiceDialog`: the prompt, the list (its first entry selected), OK and Cancel; a double click is OK. OK with an empty list chooses nothing. */
export function SingleChoiceDialog({ title, prompt, choices, onCancel, onChoose }: { title: string; prompt: string; choices: readonly string[]; onCancel: () => void; onChoose: (name: string) => void }) {
  const [picked, setPicked] = useState(0);
  return (
    <SchDialogShell title={title} width={360} onCancel={onCancel} onOk={() => onChoose(choices[picked] ?? "")}>
      <div style={{ marginBottom: 6 }}>{prompt}</div>
      <select size={Math.max(4, Math.min(10, choices.length))} style={{ width: "100%" }} value={choices[picked] ?? ""} onChange={(e) => setPicked(Math.max(0, choices.indexOf(e.target.value)))} onDoubleClick={() => onChoose(choices[picked] ?? "")} data-testid="variant-choice">
        {choices.map((c) => (
          <option key={c} value={c}>
            {c}
          </option>
        ))}
      </select>
    </SchDialogShell>
  );
}

/** `wxTextEntryDialog`: the prompt and one line of text, OK and Cancel (Enter is OK). */
export function TextEntryDialog({ title, prompt, initial, onCancel, onOk }: { title: string; prompt: string; initial: string; onCancel: () => void; onOk: (text: string) => void }) {
  const [text, setText] = useState(initial);
  return (
    <SchDialogShell title={title} width={360} onCancel={onCancel} onOk={() => onOk(text)}>
      <div style={{ marginBottom: 6 }}>{prompt}</div>
      <input
        autoFocus
        style={{ width: "100%" }}
        value={text}
        onChange={(e) => setText(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === "Enter") onOk(text);
        }}
        data-testid="variant-name-entry"
      />
    </SchDialogShell>
  );
}

/** `ShowAddVariantDialog`: "New Design Variant", the name and the optional description; then `ValidateNewVariantName`, `AddVariant`, `SetVariantDescription` and `SetCurrentVariant`. */
export function AddVariantDialog({ onClose, say, onAdded }: { onClose: () => void; say: Say; onAdded?: (name: string) => void }) {
  const state = useStudioState();
  const dispatch = useStudioDispatch();
  const api = useStudioApi();
  const [name, setName] = useState("");
  const [description, setDescription] = useState("");
  const ok = async () => {
    onClose();
    const variantName = trimName(name);
    const variantDesc = trimName(description);
    const why = validateNewVariantName(state.schematic?.variants, variantName);
    if (why) return say(why);
    if (!(await api.cmd({ op: "add_design_variant", name: variantName, description: variantDesc }))) return;
    dispatch({ type: "SET_SCH_VARIANT", variant: variantName });
    onAdded?.(variantName);
  };
  return (
    <SchDialogShell title="New Design Variant" width={340} onCancel={onClose} onOk={() => void ok()}>
      <div style={{ display: "flex", flexDirection: "column", gap: 3 }} data-testid="new-variant-dialog">
        <label htmlFor="new-variant-name">Variant name:</label>
        <input id="new-variant-name" autoFocus value={name} onChange={(e) => setName(e.target.value)} data-testid="new-variant-name" />
        <label htmlFor="new-variant-desc" style={{ marginTop: 8 }}>
          Description (optional):
        </label>
        <textarea id="new-variant-desc" rows={3} style={{ minWidth: 300 }} value={description} onChange={(e) => setDescription(e.target.value)} data-testid="new-variant-description" />
      </div>
    </SchDialogShell>
  );
}

/** `RemoveVariant` after its choice: `DeleteVariant` (one undo step: "Delete Variant"), and the default variant when the removed one was the current one. */
function RemoveVariant({ name, onClose }: { name: string; onClose: () => void }) {
  const api = useStudioApi();
  const dispatch = useStudioDispatch();
  const state = useStudioState();
  const done = useRef(false);
  useEffect(() => {
    if (done.current) return;
    done.current = true;
    onClose();
    const wasCurrent = state.schVariant === name;
    void api.cmd({ op: "remove_design_variant", name }).then((ok) => {
      if (ok && wasCurrent) dispatch({ type: "SET_SCH_VARIANT", variant: "" });
    });
  }, [api, dispatch, name, onClose, state.schVariant]);
  return null;
}

/** `SetVariantDescription` with the text of the description dialog trimmed. */
function DescriptionDialog({ name, initial, onClose }: { name: string; initial: string; onClose: () => void }) {
  const api = useStudioApi();
  return (
    <DescriptionEntryDialog
      name={name}
      initial={initial}
      onCancel={onClose}
      onOk={(text) => {
        onClose();
        void api.cmd({ op: "edit_design_variant_description", name, description: trimName(text) });
      }}
    />
  );
}

/** "Edit Description for '%s'": "Description:" and a multi-line text, OK and Cancel. */
export function DescriptionEntryDialog({ name, initial, onCancel, onOk }: { name: string; initial: string; onCancel: () => void; onOk: (text: string) => void }) {
  const [text, setText] = useState(initial);
  const ok = () => onOk(text);
  const onClose = onCancel;
  return (
    <SchDialogShell title={`Edit Description for '${name}'`} width={440} onCancel={onClose} onOk={ok}>
      <div style={{ display: "flex", flexDirection: "column", gap: 3 }}>
        <label htmlFor="variant-desc">Description:</label>
        <textarea id="variant-desc" autoFocus rows={3} style={{ minWidth: 400 }} value={text} onChange={(e) => setText(e.target.value)} data-testid="variant-description" />
      </div>
    </SchDialogShell>
  );
}

/** `RenameVariant` after its choice: "Enter new variant name:" with the old name; the same name changes nothing; `ValidateNewVariantName( new, old )`; the current variant follows. */
function RenameDialog({ name, onClose, say }: { name: string; onClose: () => void; say: Say }) {
  const api = useStudioApi();
  const state = useStudioState();
  const dispatch = useStudioDispatch();
  const ok = async (text: string) => {
    onClose();
    const newName = trimName(text);
    if (newName === name) return;
    const why = validateNewVariantName(state.schematic?.variants, newName, name);
    if (why) return say(why);
    const wasCurrent = state.schVariant === name;
    if ((await api.cmd({ op: "rename_design_variant", name, new_name: newName })) && wasCurrent) dispatch({ type: "SET_SCH_VARIANT", variant: newName });
  };
  return <TextEntryDialog title="Rename Design Variant" prompt="Enter new variant name:" initial={name} onCancel={onClose} onOk={(t) => void ok(t)} />;
}

/** `CopyVariant` after its choice: "Enter name for the copied variant:" with "<name>_copy"; the copy becomes the current variant. */
function CopyDialog({ name, fromTable, onClose, say }: { name: string; fromTable: boolean; onClose: () => void; say: Say }) {
  const api = useStudioApi();
  const state = useStudioState();
  const dispatch = useStudioDispatch();
  const ok = async (text: string) => {
    onClose();
    const newName = trimName(text);
    const why = fromTable ? validateCopyNameInTable(variantNamesForUI(state.schematic?.variants), newName) : validateNewVariantName(state.schematic?.variants, newName);
    if (why) return say(why);
    if (await api.cmd({ op: "copy_design_variant", name, new_name: newName })) dispatch({ type: "SET_SCH_VARIANT", variant: newName });
  };
  return <TextEntryDialog title="Copy Design Variant" prompt="Enter name for the copied variant:" initial={`${name}_copy`} onCancel={onClose} onOk={(t) => void ok(t)} />;
}
