// The property grid of the Properties panel (`wxPropertyGrid` in common/widgets/properties_panel.cpp): a caption (`m_caption`), the groups of the selection's common
// properties as category rows, and under each a name and an editor per property -- a text box that takes a unit (`PG_UNIT_EDITOR`), a check box that can be
// indeterminate (`PG_CHECKBOX_EDITOR`), a choice, a net list (`PG_NET_SELECTOR_EDITOR`), a colour. A value the selected items differ in shows `<...>`
// (`SetUnspecifiedValueAppearance`); a read-only property is greyed (`SetCellDisabledTextColour`). Enter commits and goes to the next property
// (`wxPG_ACTION_NEXT_PROPERTY`), Escape puts the old value back; a refused value shows its message above the grid (`ShowInfoBarError` of `valueChanging`).
//
// The model comes from kicad-port/propertyGrid.ts (`buildGrid`); what an edit does is the caller's `onCommit` (it plans the commands and sends them as one batch).
//
// KiCad 25ec2335's category buttons (`PROPERTIES_PANEL::addCategoryButton`), when the caller hands `categories`: "Custom Properties" ("Add Custom Property", kicad-port/
// pcbCustomProperties.ts) and "Fields" ("Add Field"). The button adds a row and puts its name in edit, blank (`beginLabelEdit( name, true )`); a double click on the name
// of a row whose name can be edited (`isKeyEditable`) edits it (`onPropertyLabelEdited`); a right click on the group or a row opens `buildContextMenu`'s Remove / Add.
import { DatasheetUrlButton } from "../DatasheetUrlButton";
import { useEffect, useRef, useState, type KeyboardEvent } from "react";
import { editText, formatValue, parseValue, type GridModel, type GridRow } from "../../kicad-port/propertyGrid";
import type { PropValue } from "../../kicad-port/propertyManager";
import type { LengthUnit } from "../../state/units";
import type { OriginTransforms } from "../../kicad-port/originTransforms";

/** What a cell shows when the items of the selection differ (`SetUnspecifiedValueAppearance( wxPGCell( "<...>" ) )`). */
export const UNSPECIFIED = "<...>";

const MIXED = "\u0000mixed";

/** The unit shown after an editable number. */
function unitSuffix(row: Pick<GridRow, "kind" | "display">, units: LengthUnit): string {
  if (row.kind === "string" || row.kind === "bool" || row.kind === "enum" || row.kind === "net" || row.kind === "color") return "";
  switch (row.display) {
    case "size":
    case "coord":
      return units;
    case "area":
      return "mm²";
    case "degree":
      return "°";
    default:
      return "";
  }
}

/** What a category with a button does (the caller's `PCB_PROPERTIES_PANEL` handlers); each resolves to a refusal's message or null. */
export interface CategoryHandlers {
  /** The button's and the menus' words: "Custom Property" or "Field" ("Add Custom Property", "Remove Field"). */
  noun: string;
  /** `addBlankCustomProperty` / `addBlankField`: the name of the row added, or null when none was. */
  add: () => Promise<string | null>;
  /** `isKeyEditable`: a row whose name can be edited and which can be removed. */
  editable: (name: string) => boolean;
  /** A name edited (`onPropertyLabelEdited`); `pending` for the row just added. */
  label: (oldName: string, newName: string, pending: boolean) => Promise<string | null>;
  /** `removeCustomProperty` / `removeField`. */
  remove: (name: string) => Promise<string | null>;
  /** The text of a row typed in; absent: the grid's own `onCommit`. */
  commit?: (name: string, text: string) => Promise<string | null>;
}

export interface PropertyGridProps {
  model: GridModel;
  /** The categories with a button, by group name ("Custom Properties", "Fields"); a group marked `custom` without its handlers is not shown. */
  categories?: Readonly<Record<string, CategoryHandlers>>;
  units: LengthUnit;
  /** The frame's origin transforms (`GetOriginTransforms()`), which the `PT_COORD` rows are shown and typed through; absent: none (the base class's). */
  xf?: OriginTransforms;
  /** Applies an edit of the property `name` to the selection; resolves to the message of a refusal, or null when it was applied. */
  onCommit: (name: string, value: PropValue) => Promise<string | null>;
  /** Changes whenever the selection does, so a message about the old selection goes. */
  selectionKey: string;
}

export function PropertyGrid({ model, units, xf, onCommit, selectionKey, categories }: PropertyGridProps) {
  const [error, setError] = useState<string | null>(null);
  /** The row whose name is being edited, its group, and whether it is the one just added (`m_pendingNewKey`). */
  const [labelEdit, setLabelEdit] = useState<{ group: string; name: string; pending: boolean } | null>(null);
  /** A category's context menu (`buildContextMenu`): where, its group, and the row it was opened on (null: the group's category row). */
  const [menu, setMenu] = useState<{ x: number; y: number; group: string; name: string | null } | null>(null);
  useEffect(() => {
    setError(null);
    setLabelEdit(null);
    setMenu(null);
  }, [selectionKey]);

  const commit = async (name: string, value: PropValue): Promise<void> => {
    setError(await onCommit(name, value));
  };
  const add = async (group: string): Promise<void> => {
    setMenu(null);
    const h = categories?.[group];
    if (!h) return;
    const name = await h.add();
    if (name) setLabelEdit({ group, name, pending: true });
  };
  const remove = async (group: string, name: string): Promise<void> => {
    setMenu(null);
    const h = categories?.[group];
    if (h) setError(await h.remove(name));
  };
  const finishLabel = async (oldName: string, newName: string | null): Promise<void> => {
    const edit = labelEdit;
    setLabelEdit(null);
    const h = edit ? categories?.[edit.group] : undefined;
    if (!h || !edit) return;
    // Escape on the new one leaves its name blank (`onNewItemLeftBlank`), on an old one puts it back
    if (newName === null) {
      if (edit.pending) setError(await h.label(oldName, "", true));
      return;
    }
    setError(await h.label(oldName, newName, edit.pending));
  };
  const groups = model.groups.filter((g) => !g.custom || !!categories?.[g.name]);

  return (
    <div className="prop-grid" data-testid="properties-grid">
      <div className="prop-caption">{model.caption}</div>
      {error && (
        <div className="prop-error" role="alert">
          {error}
        </div>
      )}
      {groups.map((g) => {
        const h = categories?.[g.name];
        if (!h) {
          return (
            <div key={g.name} className="prop-group" data-group={g.caption}>
              <div className="prop-group-caption">{g.caption}</div>
              {g.rows.map((r) => (
                <PropertyRow key={r.name} row={r} units={units} xf={xf} commit={commit} fail={setError} />
              ))}
            </div>
          );
        }
        const rowCommit = h.commit ? async (name: string, value: PropValue) => setError(await h.commit!(name, String(value))) : commit;
        return (
          <div key={g.name} className="prop-group" data-group={g.caption}>
            <div
              className="prop-group-caption"
              style={{ display: "flex", alignItems: "center" }}
              onContextMenu={(e) => {
                e.preventDefault();
                setMenu({ x: e.clientX, y: e.clientY, group: g.name, name: null });
              }}
            >
              <span style={{ flex: 1 }}>{g.caption}</span>
              <button className="prop-group-button" title={`Add ${h.noun}`} aria-label={`Add ${h.noun}`} data-testid={g.custom ? "prop-add-custom" : `prop-add-${g.name.toLowerCase()}`} onClick={() => void add(g.name)}>
                +
              </button>
            </div>
            {g.rows.map((r) =>
              h.editable(r.name) || g.custom ? (
                <KeyedRow
                  key={r.name}
                  row={r}
                  units={units}
                  xf={xf}
                  custom={!!g.custom}
                  editing={labelEdit?.group === g.name && labelEdit.name === r.name ? labelEdit : null}
                  onBeginLabel={() => setLabelEdit({ group: g.name, name: r.name, pending: false })}
                  onEndLabel={(text) => void finishLabel(r.name, text)}
                  onMenu={(x, y) => setMenu({ x, y, group: g.name, name: r.name })}
                  commit={rowCommit}
                  fail={setError}
                />
              ) : (
                <div key={r.name} onContextMenu={(e) => (e.preventDefault(), setMenu({ x: e.clientX, y: e.clientY, group: g.name, name: null }))}>
                  <PropertyRow row={r} units={units} xf={xf} commit={rowCommit} fail={setError} />
                </div>
              ),
            )}
          </div>
        );
      })}
      {menu && (
        <>
          <div style={{ position: "fixed", inset: 0, zIndex: 999 }} onClick={() => setMenu(null)} onContextMenu={(e) => (e.preventDefault(), setMenu(null))} />
          <div className="context-menu" role="menu" data-testid="prop-custom-menu" style={{ position: "fixed", left: menu.x, top: menu.y, zIndex: 1000, background: "var(--chrome-bg-raised)", border: "1px solid var(--chrome-border)", borderRadius: 6, padding: 4 }}>
            {menu.name !== null && (
              <div className="context-menu-item" role="menuitem" style={{ padding: "3px 10px", cursor: "pointer" }} onClick={() => void remove(menu.group, menu.name!)}>
                Remove {categories?.[menu.group]?.noun}
              </div>
            )}
            <div className="context-menu-item" role="menuitem" style={{ padding: "3px 10px", cursor: "pointer" }} onClick={() => void add(menu.group)}>
              Add {categories?.[menu.group]?.noun}
            </div>
          </div>
        </>
      )}
    </div>
  );
}

/** A row whose name can be edited (a custom property, a footprint's user field): its name, which a double click edits (blank for the one just added), and its value. */
function KeyedRow({
  row,
  units,
  xf,
  custom,
  editing,
  onBeginLabel,
  onEndLabel,
  onMenu,
  commit,
  fail,
}: {
  row: GridRow;
  units: LengthUnit;
  xf?: OriginTransforms;
  custom: boolean;
  editing: { pending: boolean } | null;
  onBeginLabel: () => void;
  /** The name typed, or null for Escape. */
  onEndLabel: (text: string | null) => void;
  onMenu: (x: number, y: number) => void;
  commit: (name: string, value: PropValue) => Promise<void>;
  fail: (message: string) => void;
}) {
  const [draft, setDraft] = useState("");
  const done = useRef(false);
  useEffect(() => {
    // `beginLabelEdit( key, aStartBlank )`: the new one starts blank, an old one with its name
    if (editing) {
      setDraft(editing.pending ? "" : row.name);
      done.current = false;
    }
  }, [editing, row.name]);
  const end = (text: string | null) => {
    if (done.current) return;
    done.current = true;
    onEndLabel(text);
  };
  return (
    <div
      className="prop-row"
      data-prop={row.name}
      data-custom-prop={custom ? row.name : undefined}
      data-keyed-prop={row.name}
      title={row.name}
      onContextMenu={(e) => {
        e.preventDefault();
        onMenu(e.clientX, e.clientY);
      }}
    >
      {editing ? (
        <input
          className="prop-label"
          data-testid="prop-key-label"
          autoFocus
          value={draft}
          onChange={(e) => setDraft(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter") end(draft);
            else if (e.key === "Escape") end(null);
          }}
          onBlur={() => end(draft)}
        />
      ) : (
        <span className="prop-label" onDoubleClick={onBeginLabel}>
          {row.name}
        </span>
      )}
      <div className="prop-value">{row.writable ? <Editor row={row} units={units} xf={xf} commit={commit} fail={fail} /> : <ReadOnly row={row} units={units} xf={xf} mixed={row.value === null} />}</div>
    </div>
  );
}

interface RowProps {
  row: GridRow;
  units: LengthUnit;
  xf?: OriginTransforms;
  commit: (name: string, value: PropValue) => Promise<void>;
  fail: (message: string) => void;
}

/** Focus the next editor of the grid after `from`, the way Enter does in a `wxPropertyGrid`. */
function focusNext(from: HTMLElement): void {
  const grid = from.closest(".prop-grid");
  if (!grid) return;
  const inputs = [...grid.querySelectorAll<HTMLElement>("[data-prop-input]:not([disabled])")];
  const at = inputs.indexOf(from);
  const next = inputs[at + 1];
  if (next) {
    next.focus();
    if (next instanceof HTMLInputElement && next.type === "text") next.select();
  }
}

function PropertyRow({ row, units, xf, commit, fail }: RowProps) {
  const mixed = row.value === null;
  return (
    // A row the studio cannot edit where KiCad can says why in its tooltip (`GridRow.note`).
    <div className={`prop-row${row.writable ? "" : " readonly"}`} data-prop={row.name} data-note={row.note} title={row.note ? `${row.name}: ${row.note}` : row.name}>
      <span className="prop-label">{row.name}</span>
      <div className="prop-value">{row.writable ? <Editor row={row} units={units} xf={xf} commit={commit} fail={fail} /> : <ReadOnly row={row} units={units} xf={xf} mixed={mixed} />}</div>
    </div>
  );
}

function ReadOnly({ row, units, xf, mixed }: { row: GridRow; units: LengthUnit; xf?: OriginTransforms; mixed: boolean }) {
  if (mixed) return <span className="prop-ro">{UNSPECIFIED}</span>;
  const value = row.value as PropValue;
  if (row.kind === "bool") return <input type="checkbox" checked={value === true} disabled readOnly aria-label={row.name} />;
  if (row.kind === "color") return value === "" ? <span className="prop-ro">default</span> : <span className="prop-swatch" style={{ background: String(value) }} title={String(value)} />;
  const text = formatValue(row, value, units, xf);
  return (
    <span className="prop-ro" title={row.note ? `${text} (${row.note})` : text}>
      {text}
    </span>
  );
}

function Editor({ row, units, xf, commit, fail }: RowProps) {
  switch (row.kind) {
    case "bool":
      return <BoolEditor row={row} commit={commit} />;
    case "enum":
    case "net":
      return <ChoiceEditor row={row} commit={commit} />;
    case "color":
      return <ColorEditor row={row} commit={commit} />;
    default:
      return <TextEditor row={row} units={units} xf={xf} commit={commit} fail={fail} />;
  }
}

function BoolEditor({ row, commit }: Pick<RowProps, "row" | "commit">) {
  const ref = useRef<HTMLInputElement>(null);
  const mixed = row.value === null;
  useEffect(() => {
    if (ref.current) ref.current.indeterminate = mixed;
  }, [mixed, row.value]);
  return <input ref={ref} type="checkbox" data-prop-input aria-label={row.name} checked={row.value === true} onChange={(e) => void commit(row.name, e.target.checked)} />;
}

function ChoiceEditor({ row, commit }: Pick<RowProps, "row" | "commit">) {
  const mixed = row.value === null;
  const choices = row.choices ?? [];
  const current = mixed ? MIXED : String(row.value);
  const known = mixed || choices.some((c) => String(c.value) === current);
  return (
    <select
      data-prop-input
      aria-label={row.name}
      value={current}
      onChange={(e) => {
        const hit = choices.find((c) => String(c.value) === e.target.value);
        if (hit) void commit(row.name, hit.value);
      }}
      onKeyDown={(e) => {
        if (e.key === "Enter") {
          e.preventDefault();
          focusNext(e.currentTarget);
        }
      }}
    >
      {mixed && (
        <option value={MIXED} disabled>
          {UNSPECIFIED}
        </option>
      )}
      {!known && <option value={current}>{current}</option>}
      {choices.map((c) => (
        <option key={String(c.value)} value={String(c.value)}>
          {c.label}
        </option>
      ))}
    </select>
  );
}

function ColorEditor({ row, commit }: Pick<RowProps, "row" | "commit">) {
  const mixed = row.value === null;
  const value = mixed ? "" : String(row.value);
  const [draft, setDraft] = useState(value);
  const input = useRef<HTMLInputElement>(null);
  /** The colour already sent: a second event before the board answers does not send it again. `touched`: a colour was picked (a blur alone sends nothing). */
  const sent = useRef<string | null>(null);
  const touched = useRef(false);
  const latest = useRef({ value, commit, name: row.name });
  latest.current = { value, commit, name: row.name };
  useEffect(() => {
    setDraft(value);
    sent.current = null;
    touched.current = false;
  }, [value]);
  // The colour dialog reports its pick with the native `change` event (React's `onChange` is the `input` event, which fires all the way through a drag): that is the edit.
  useEffect(() => {
    const el = input.current;
    if (!el) return;
    const done = (): void => {
      const { value: now, commit: send, name } = latest.current;
      if (!touched.current || el.value === now || sent.current === el.value) return;
      sent.current = el.value;
      void send(name, el.value);
    };
    el.addEventListener("change", done);
    el.addEventListener("blur", done);
    return () => {
      el.removeEventListener("change", done);
      el.removeEventListener("blur", done);
    };
  }, []);
  return (
    <>
      <input
        ref={input}
        type="color"
        data-prop-input
        aria-label={row.name}
        value={draft === "" ? "#000000" : draft}
        className={mixed || (draft === "" && value === "") ? "prop-color unset" : "prop-color"}
        onChange={(e) => {
          sent.current = null;
          touched.current = true;
          setDraft(e.target.value);
        }}
      />
      {mixed ? <span className="prop-ro">{UNSPECIFIED}</span> : value === "" ? <span className="prop-ro">default</span> : <span className="prop-ro">{value}</span>}
      {!mixed && value !== "" && (
        <button type="button" className="prop-clear" title="Use the colour of the layer" aria-label={`Clear ${row.name}`} onClick={() => void commit(row.name, "")}>
          {"×"}
        </button>
      )}
    </>
  );
}

function TextEditor({ row, units, xf, commit, fail }: RowProps) {
  const mixed = row.value === null;
  const initial = mixed ? "" : editText(row, row.value as PropValue, units, xf);
  const [draft, setDraft] = useState(initial);
  // After an edit the draft is whatever the board says: the new value when it was applied, the old one when it was refused. `settled` re-runs the sync.
  const [settled, setSettled] = useState(0);
  /** The text already sent: Enter commits and moves on, and the blur of moving on must not send it a second time. Typing again, or the board answering, clears it. */
  const sent = useRef<string | null>(null);
  useEffect(() => {
    setDraft(initial);
    sent.current = null;
  }, [initial, settled]);
  const suffix = unitSuffix(row, units);
  /** Escape puts the old value back: the blur it causes must not commit what was typed. */
  const cancelled = useRef(false);

  const submit = async (input: HTMLElement | null): Promise<void> => {
    if (cancelled.current) {
      cancelled.current = false;
      return;
    }
    if (sent.current === draft || draft === initial || (mixed && draft.trim() === "")) return;
    const parsed = parseValue(row, draft, units, xf);
    if (!parsed.ok) {
      fail(`${row.name}: ${parsed.error}`);
      setSettled((n) => n + 1);
      return;
    }
    sent.current = draft;
    await commit(row.name, parsed.value);
    setSettled((n) => n + 1);
    if (input) focusNext(input);
  };

  const onKeyDown = (e: KeyboardEvent<HTMLInputElement>): void => {
    if (e.key === "Enter") {
      e.preventDefault();
      const el = e.currentTarget;
      if (draft === initial || (mixed && draft.trim() === "")) focusNext(el);
      else void submit(el);
    } else if (e.key === "Escape") {
      e.preventDefault();
      e.stopPropagation();
      cancelled.current = true;
      setDraft(initial);
      e.currentTarget.blur();
      // A blur that did not happen (the input was not focused) leaves no flag behind.
      cancelled.current = false;
    }
  };

  return (
    <>
      <input
        type="text"
        data-prop-input
        aria-label={row.name}
        spellCheck={false}
        value={draft}
        placeholder={mixed ? UNSPECIFIED : undefined}
        onChange={(e) => {
          sent.current = null;
          setDraft(e.target.value);
        }}
        onBlur={() => void submit(null)}
        onKeyDown={onKeyDown}
      />
      {suffix && <span className="prop-unit">{suffix}</span>}
      {/* `PG_URL_EDITOR` on a Datasheet row: no files stack (`GetAssociatedDocument( m_frame, filename, &m_frame->Prj() )`), so a kicad-embed:// link opens nothing */}
      {row.name === "Datasheet" && row.kind === "string" && !mixed && <DatasheetUrlButton value={initial} stack={[]} />}
    </>
  );
}
