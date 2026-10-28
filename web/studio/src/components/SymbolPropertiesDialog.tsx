// `E` (sch_edit_tool.cpp::Properties -- dispatches to one of 6 dialogs by
// item type; this app only has symbols selectable so far, so this is
// always DIALOG_SYMBOL_PROPERTIES's own shape, trimmed to the fields this
// IR has: Reference/Value/Footprint/Datasheet, no unit/pin-table
// editing) and `U`/`V`/`F` (sch_edit_tool.cpp::EditField's own quick
// single-field edits: editReference/editValue/editFootprint) -- real
// eeschema uses a different, smaller dialog for U/V/F
// (DIALOG_FIELD_PROPERTIES, one field only); this app reuses the same
// form and just autofocuses the one field the hotkey named, a deliberate
// simplification (one component to maintain, same backend Cmds either
// way) rather than building two dialogs with identical plumbing.
//
// The "Show" column of the fields table (`DIALOG_SYMBOL_PROPERTIES`) shows or hides each of the four fields and of the user-defined ones that follow them (the text of
// each of those is edited here, Add Field makes one -- hidden, as KiCad's `OnAddField` does -- and the x beside it removes it: one undo step with the rest), and the Body style choice (Standard / Alternate, only for a symbol
// whose library symbol has two) puts the symbol in the other body style (`SCH_EDIT_FRAME::SelectBodyStyle`): both go in the same undo step as the texts. The Footprint
// field's Browse... button opens the Footprint Chooser.
import { Fragment, useEffect, useMemo, useRef, useState } from "react";
import { fetchAnySymbol } from "../api/libraryClient";
import type { Cmd } from "../api/types";
import { uniquePinCount } from "../kicad-port/footprintFilter";
import { bodyStyleCount, setBodyStyleCmd } from "../kicad-port/schBodyStyle";
import { addUserFieldCmd, fieldTextCmd, fieldVisibilityCmds, newUserFieldName, ownerKey, removeUserFieldCmd, STANDARD_SYMBOL_FIELDS, userFieldNameError, userFieldsOf } from "../kicad-port/schFieldEdit";
import { expandStackedPinNotation } from "../kicad-port/stackedPins";
import { useStudioApi, useStudioDispatch, useStudioState } from "../state/store";
import { FootprintChooserDialog } from "./FootprintChooserDialog";
import { SimModelDialog } from "./simulator/SimModelDialog";

const MAIN_FIELDS = STANDARD_SYMBOL_FIELDS;

/** A user-defined field's row: what the symbol has (`was`), or a field added here (`isNew`), as it stands now. */
interface UserRow {
  key: number;
  name: string;
  text: string;
  shown: boolean;
  isNew: boolean;
  removed: boolean;
  was?: { text: string; visible: boolean };
}

export function SymbolPropertiesDialog() {
  const state = useStudioState();
  const dispatch = useStudioDispatch();
  const api = useStudioApi();
  const open = state.symbolProperties;
  const symbol = open ? api.symbolById(open.id) : undefined;

  const [reference, setReference] = useState("");
  const [value, setValue] = useState("");
  const [footprint, setFootprint] = useState("");
  const [datasheet, setDatasheet] = useState("");
  const [error, setError] = useState<string | null>(null);
  /** The "Show" check of each main field, by name; `undefined` for a field the server did not send (an older one). */
  const [shown, setShown] = useState<Record<string, boolean>>({});
  const [bodyStyle, setBodyStyle] = useState(1);
  /** The user-defined fields of the symbol, the ones it has and the ones added here. */
  const [userRows, setUserRows] = useState<UserRow[]>([]);
  const nextKey = useRef(0);
  // The Footprint field's browse button: the Footprint Chooser, narrowed by this symbol's pin count and its library symbol's footprint filters.
  const [choosingFootprint, setChoosingFootprint] = useState(false);
  // "Simulation Model..." (`OnEditSpiceModel`, the button `dialog_symbol_properties.cpp` shows for a symbol that is not a power symbol)
  const [editingSimModel, setEditingSimModel] = useState(false);
  const [libInfo, setLibInfo] = useState<{ filters: string[]; pins: number } | null>(null);
  const libId = symbol?.lib_id ?? null;
  useEffect(() => {
    setLibInfo(null);
    if (!libId) return;
    let cancelled = false;
    // The pins of the whole library symbol, every unit (`GetGraphicalPins( 0, 1 )`): the placed instance lists the pins of its own unit only.
    fetchAnySymbol(libId)
      .then((r) => !cancelled && setLibInfo({ filters: r.symbol.footprint_filters ?? [], pins: uniquePinCount(r.symbol.pins, (n) => expandStackedPinNotation(n).numbers) }))
      .catch(() => undefined);
    return () => {
      cancelled = true;
    };
  }, [libId]);
  const instancePins = useMemo(() => new Set((symbol?.pins ?? []).map((p) => p.number)).size, [symbol]);

  const refInput = useRef<HTMLInputElement>(null);
  const valueInput = useRef<HTMLInputElement>(null);
  const footprintInput = useRef<HTMLInputElement>(null);
  const datasheetInput = useRef<HTMLInputElement>(null);

  useEffect(() => {
    if (!symbol) return;
    setReference(symbol.id);
    setValue(symbol.value ?? "");
    setFootprint(symbol.footprint ?? "");
    setDatasheet(symbol.datasheet ?? "");
    setShown(Object.fromEntries((symbol.fields ?? []).filter((f) => (MAIN_FIELDS as readonly string[]).includes(f.name)).map((f) => [f.name, f.visible])));
    setBodyStyle(symbol.body_style || 1);
    setUserRows(userFieldsOf(symbol.fields).map((f) => ({ key: nextKey.current++, name: f.name, text: f.text, shown: f.visible, isNew: false, removed: false, was: { text: f.text, visible: f.visible } })));
    setError(null);
    const focusTarget = { reference: refInput, value: valueInput, footprint: footprintInput, datasheet: datasheetInput }[open?.field ?? "reference"];
    // Let the dialog mount before stealing focus.
    const t = setTimeout(() => focusTarget.current?.select(), 0);
    return () => clearTimeout(t);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open?.id, open?.field]);

  if (!open || !symbol) return null;
  const close = () => dispatch({ type: "SET_SYMBOL_PROPERTIES", value: null });

  const submit = async () => {
    const trimmedRef = reference.trim();
    if (!trimmedRef) {
      setError("Reference cannot be blank.");
      return;
    }
    const renamed = trimmedRef !== symbol.id;
    if (renamed && api.symbolById(trimmedRef)) {
      setError(`"${trimmedRef}" is already used by another symbol.`);
      return;
    }
    // the fields added here need names that no field of the symbol has (`FIELDS_EDITOR_GRID_DATA_MODEL`: the same name twice is no second field)
    const kept = userRows.filter((r) => !r.removed);
    const added = kept.filter((r) => r.isNew);
    for (const r of added) {
      const taken = kept.filter((o) => o !== r).map((o) => o.name);
      const why = userFieldNameError(r.name, taken);
      if (why) {
        setError(why);
        return;
      }
    }
    // One submit, one undo step: the texts (the footprint is the Browse... pick too), the user-defined fields taken away, added and edited, the shown fields and the body
    // style, all by the reference the symbol has now, and the new reference last (a rename moves what is kept under the old one; the server's parts are not renamed until the
    // step is done, so a field edit after it would miss its symbol)
    const owner = ownerKey({ id: symbol.id, unit: symbol.unit });
    const cmds: Cmd[] = [{ op: "edit_symbol_fields", id: symbol.id, value, footprint, datasheet }];
    for (const r of userRows) if (r.removed && !r.isNew) cmds.push(removeUserFieldCmd(symbol.id, r.name));
    for (const r of added) cmds.push(addUserFieldCmd(symbol.id, r.name, r.text));
    for (const r of kept) if (!r.isNew && r.was && r.text !== r.was.text) cmds.push(fieldTextCmd(owner, r.name, r.text));
    const wanted: Record<string, boolean> = { ...shown };
    for (const r of kept) wanted[r.name.trim()] = r.shown;
    cmds.push(...fieldVisibilityCmds(owner, [...(symbol.fields ?? []), ...added.map((r) => ({ name: r.name.trim(), visible: false }))], wanted));
    if (state.schematic && bodyStyle !== (symbol.body_style || 1)) {
      const change = setBodyStyleCmd(state.schematic, [symbol.id], bodyStyle);
      if (change) cmds.push(change);
    }
    if (renamed) cmds.push({ op: "rename_symbol", id: symbol.id, new_id: trimmedRef });
    if (!(await api.cmdBatch(cmds))) {
      // the server says why (a reference another sheet has, a field of a sheet read from a KiCad file) in a toast
      setError(renamed ? `The changes were not applied ("${trimmedRef}" may be used by another symbol).` : "The changes were not applied.");
      return;
    }
    close();
  };
  const bodyStyles = state.schematic ? bodyStyleCount(state.schematic, symbol) : 1;
  // a sheet read from a KiCad file has no places for its fields yet: the server lists none, and a field added there could not be shown
  const hasPlaces = (symbol.fields?.length ?? 0) > 0;
  /** The "Show" check of a main field's row; nothing for a field the server did not send. */
  const showBox = (name: string) =>
    shown[name] === undefined ? (
      <span />
    ) : (
      <label title={`Show the ${name} field on the sheet`} style={{ whiteSpace: "nowrap" }}>
        <input type="checkbox" checked={shown[name]} onChange={(e) => setShown({ ...shown, [name]: e.target.checked })} /> Show
      </label>
    );

  return (
    <div className="dialog-backdrop" onClick={close}>
      <div className="dialog" style={{ width: 480 }} onClick={(e) => e.stopPropagation()}>
        <div className="dialog-header">
          <span>Symbol Properties</span>
        </div>
        <div className="dialog-body">
          <div className="kv-grid" style={{ gridTemplateColumns: "90px 1fr auto" }}>
            <span>Reference</span>
            <input
              ref={refInput}
              value={reference}
              onChange={(e) => setReference(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter") submit();
              }}
            />
            {showBox("Reference")}
            <span>Value</span>
            <input
              ref={valueInput}
              value={value}
              onChange={(e) => setValue(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter") submit();
              }}
            />
            {showBox("Value")}
            <span>Footprint</span>
            <div style={{ display: "flex", gap: 4 }}>
              <input
                ref={footprintInput}
                style={{ flex: 1, minWidth: 0 }}
                value={footprint}
                onChange={(e) => setFootprint(e.target.value)}
                onKeyDown={(e) => {
                  if (e.key === "Enter") submit();
                }}
              />
              <button onClick={() => setChoosingFootprint(true)} title="Choose a footprint from KiCad's libraries (Footprint Chooser)" data-browse-footprint>
                Browse…
              </button>
            </div>
            {showBox("Footprint")}
            <span>Datasheet</span>
            <input
              ref={datasheetInput}
              value={datasheet}
              onChange={(e) => setDatasheet(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter") submit();
              }}
            />
            {showBox("Datasheet")}
            {userRows
              .filter((r) => !r.removed)
              .map((r) => (
                <Fragment key={r.key}>
                  {r.isNew ? (
                    <input aria-label="New field name" value={r.name} onChange={(e) => setUserRows((rows) => rows.map((o) => (o === r ? { ...o, name: e.target.value } : o)))} style={{ minWidth: 0 }} />
                  ) : (
                    <span title="A user-defined field" style={{ overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>
                      {r.name}
                    </span>
                  )}
                  <div style={{ display: "flex", gap: 4 }}>
                    <input
                      aria-label={`${r.name} value`}
                      style={{ flex: 1, minWidth: 0 }}
                      value={r.text}
                      onChange={(e) => setUserRows((rows) => rows.map((o) => (o === r ? { ...o, text: e.target.value } : o)))}
                      onKeyDown={(e) => {
                        if (e.key === "Enter") submit();
                      }}
                    />
                    <button type="button" title="Remove this field" aria-label={`Remove ${r.name}`} onClick={() => setUserRows((rows) => rows.map((o) => (o === r ? { ...o, removed: true } : o)))}>
                      ×
                    </button>
                  </div>
                  <label title={`Show the ${r.name} field on the sheet`} style={{ whiteSpace: "nowrap" }}>
                    <input type="checkbox" checked={r.shown} onChange={(e) => setUserRows((rows) => rows.map((o) => (o === r ? { ...o, shown: e.target.checked } : o)))} /> Show
                  </label>
                </Fragment>
              ))}
            <span>Library</span>
            <span style={{ opacity: 0.7 }}>{symbol.lib_id ?? "(generic)"}</span>
            <span />
            {bodyStyles > 1 && (
              <>
                <span>Body style</span>
                <select value={bodyStyle} onChange={(e) => setBodyStyle(Number(e.target.value))}>
                  {Array.from({ length: bodyStyles }, (_, i) => i + 1).map((n) => (
                    <option key={n} value={n}>
                      {/* `LIB_SYMBOL::GetBodyStyleDescription`: a De Morgan pair is Standard and Alternate */}
                      {n === 1 ? "Standard" : n === 2 ? "Alternate" : `Style ${n}`}
                    </option>
                  ))}
                </select>
                <span />
              </>
            )}
          </div>
          {hasPlaces && (
            <p style={{ margin: "8px 0 0" }}>
              <button
                type="button"
                title="Add a field of your own to this symbol (hidden until it is shown)"
                onClick={() =>
                  setUserRows((rows) => [
                    ...rows,
                    { key: nextKey.current++, name: newUserFieldName([...MAIN_FIELDS, ...rows.filter((r) => !r.removed).map((r) => r.name)]), text: "", shown: false, isNew: true, removed: false },
                  ])
                }
              >
                Add Field
              </button>
            </p>
          )}
          {error && (
            <p style={{ color: "var(--chrome-danger)", fontSize: 11, marginTop: 8 }}>{error}</p>
          )}
          <p style={{ color: "var(--chrome-text-dim)", fontSize: 11, marginTop: 10 }}>
            Renaming a symbol renames its footprint on the board with it (the pose, the copper on its pads, its fields and edits stay with the part), and one Undo takes both back.
          </p>
        </div>
        <div className="dialog-footer">
          <button style={{ marginRight: "auto" }} onClick={() => setEditingSimModel(true)} data-testid="symbol-sim-model">
            Simulation Model...
          </button>
          <button onClick={close}>Cancel</button>
          <button className="primary" onClick={submit}>
            OK
          </button>
        </div>
      </div>
      {editingSimModel && (
        <SimModelDialog
          reference={symbol.id}
          onClose={() => setEditingSimModel(false)}
          // the Simulation Model dialog wrote the fields itself (its own undo step): the rows show them as the symbol has them now
          onWritten={(written) =>
            setUserRows((rows) => {
              let out = rows.filter((r) => !(r.name in written && written[r.name] === "" && !r.isNew));
              out = out.map((r) => (r.name in written && written[r.name] !== "" ? { ...r, text: written[r.name]!, removed: false, was: { text: written[r.name]!, visible: r.was?.visible ?? r.shown } } : r));
              for (const [name, text] of Object.entries(written)) {
                if (text !== "" && !out.some((r) => r.name === name)) out.push({ key: nextKey.current++, name, text, shown: false, isNew: false, removed: false, was: { text, visible: false } });
              }
              return out;
            })
          }
        />
      )}
      {choosingFootprint && (
        <FootprintChooserDialog
          preselect={footprint.includes(":") ? footprint : null}
          pinCount={libInfo?.pins ?? instancePins}
          fpFilters={libInfo?.filters}
          onCancel={() => setChoosingFootprint(false)}
          onChoose={(pick) => {
            setChoosingFootprint(false);
            if (pick.kind === "footprint") setFootprint(pick.name);
          }}
        />
      )}
    </div>
  );
}
