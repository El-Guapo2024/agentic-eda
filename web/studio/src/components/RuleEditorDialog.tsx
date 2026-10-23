// Port of the DRC Rule Editor (pcbnew.DRETool.drcRuleEditor, Inspect > DRC Rule Editor; pcbnew/drc/rule_editor/dialog_drc_rule_editor.cpp,
// panel_drc_rule_editor.cpp, panel_drc_group_header.cpp, the drc_re_*_overlay_panel.cpp, drc_re_condition_*_panel.cpp, drc_re_custom_rule_panel.cpp and
// common/dialogs/rule_editor_dialog_base.cpp at KiCad 8303b2ad). The tree of rules on the left -- KiCad's categories and a node per constraint type, the
// board's rules under them -- with its filter box, its buttons (New Rule, Copy Rule, Move Up, Move Down, Delete Rule), its context menu and double
// click; on the right the group header (Rule type, Rule name, Comment) for a category, or the rule's panel: Name, Comment, the constraint's picture with
// its fields, the condition rows, the layer, Save / Close.
//
// The rules file is the state (`GET /api/rule_editor`, read as `DRC_RULE_LOADER` reads it, crates/drc/src/rule_editor.rs). Save asks the backend what
// Save says of the panel (`POST /api/rule_editor/check`: `onSaveButtonClicked`'s validation, KiCad's words) and, when it passes, sends one
// `edit_drc_rule` verb, as Delete, Move Up and Move Down do: each is one undo step, and kicad-cli judges the board by the file. A new or copied rule
// lives here until it is saved (`IsNew`), as in KiCad; Close drops it. Lengths are in millimetres.
//
// Not ported: Show Matches (`DRC_ENGINE::GetItemsMatchingRule`), the Syntax help window, the condition text's auto-completion, drag and drop in the
// tree, the units of the frame (fields are in mm), a new rule's Move Up/Down (KiCad moves its tree item; here only the file's rules move, and the file's
// order follows, where KiCad's move leaves the file as it is).
import { useCallback, useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { fetchRuleEditor, postRuleEditorCheck } from "../api/client";
import { useColorScheme } from "../hooks/useColorScheme";
import {
  CONDITION_LABELS,
  CUSTOM_RULE_PLACEHOLDER,
  OPERATOR_LABELS,
  buildCondition,
  buildTree,
  childrenOf,
  duplicateBaseName,
  filterTree,
  groupRows,
  isEnabled,
  needsTwoObjects,
  overlaySpec,
  parseCondition,
  readMm,
  showMm,
  toggleOrientation,
  uniqueNodeName,
  unsavedChangesMessage,
  type BoolOperator,
  type ConditionRow,
  type ConditionType,
  type OverlayField,
  type PanelType,
  type RuleDraft,
  type RuleEditorView,
  type RuleEditPayload,
  type RuleFields,
  type TreeNode,
  type ViaType,
} from "../kicad-port/ruleEditor";
import { useStudioApi, useStudioDispatch, useStudioState } from "../state/store";
import "../styles/ruleEditor.css";

/** The rule panel as it is being edited (`PANEL_DRC_RULE_EDITOR` and its constraint panel). */
interface PanelState {
  nodeId: number;
  panel: PanelType;
  entry: number | null;
  /** The name the file has for the rule (the verb's check that the file did not change). */
  entryName: string;
  name: string;
  comment: string;
  layer: string;
  rows: ConditionRow[];
  fields: RuleFields;
  /** Each number field as typed. */
  text: Record<string, string>;
  customText: string;
  draft: RuleDraft | null;
}

/** A modal message (`DisplayErrorMessage`, `OKOrCancelDialog`, `wxMessageBox`): its buttons, resolved with the one pressed. */
interface Ask {
  title: string;
  message: string;
  buttons: { label: string; value: string; primary?: boolean }[];
  resolve: (v: string) => void;
}

let draftIds = 100000;

function numberText(panel: PanelType, f: RuleFields): Record<string, string> {
  const spec = overlaySpec(panel);
  const t: Record<string, string> = {};
  for (const fld of spec?.fields ?? []) {
    if (fld.kind !== "number") continue;
    const v = f[fld.key as keyof RuleFields] as number;
    t[fld.key] = spec?.integer ? String(Math.trunc(v)) : showMm(v);
  }
  return t;
}

function panelFrom(node: TreeNode, view: RuleEditorView): PanelState | null {
  if (node.type !== "rule" || !node.panel) return null;
  const src = node.draft ?? (node.entry !== undefined ? view.entries[node.entry] : undefined);
  if (!src) return null;
  const layer = node.draft ? node.draft.layer_token : (view.entries[node.entry!]?.layer_token ?? "");
  return {
    nodeId: node.id,
    panel: node.panel,
    entry: node.draft ? null : (node.entry ?? null),
    entryName: node.draft ? "" : src.rule_name,
    name: src.rule_name,
    comment: src.comment,
    layer,
    rows: parseCondition(src.rule_condition, view.nets),
    fields: { ...src.fields },
    text: numberText(node.panel, src.fields),
    customText: src.fields.rule_text === "" ? CUSTOM_RULE_PLACEHOLDER : src.fields.rule_text,
    draft: node.draft ?? null,
  };
}

/** The panel's rule as Save sends it (`TransferDataFromWindow`). */
function payloadOf(p: PanelState, takenNames: string[]): RuleEditPayload {
  const spec = overlaySpec(p.panel);
  const fields: RuleFields = { ...p.fields };
  let valueText: string | null = null;
  for (const fld of spec?.fields ?? []) {
    if (fld.kind !== "number") continue;
    if (fld.key === "value") valueText = p.text.value ?? "";
    else (fields as unknown as Record<string, number>)[fld.key] = readMm(p.text[fld.key] ?? "");
  }
  if (p.panel === "custom_rule") fields.rule_text = p.customText;
  return {
    panel: p.panel,
    name: p.name,
    comment: p.comment,
    condition: p.panel === "custom_rule" ? "" : buildCondition(p.rows, needsTwoObjects(p.panel)),
    layer: p.layer,
    fields,
    value_text: valueText,
    taken_names: takenNames,
    severity: p.draft ? p.draft.severity : null,
    constraint_code: p.draft ? p.draft.constraint_code : null,
  };
}

function Icon({ name, size = 16 }: { name: string; size?: number }) {
  const scheme = useColorScheme();
  return <img src={`/icons/${scheme}/${name}.svg`} width={size} height={size} alt="" draggable={false} />;
}

export function RuleEditorDialog() {
  const state = useStudioState();
  const dispatch = useStudioDispatch();
  const api = useStudioApi();
  const open = state.ruleEditorDialogOpen;

  const [view, setView] = useState<RuleEditorView | null>(null);
  const [drafts, setDrafts] = useState<TreeNode[]>([]);
  const [selected, setSelected] = useState<number | null>(null);
  const [expanded, setExpanded] = useState<Set<number>>(new Set());
  const [filter, setFilter] = useState("");
  const [panel, setPanel] = useState<PanelState | null>(null);
  const [modified, setModified] = useState(false);
  const [starred, setStarred] = useState<Set<number>>(new Set());
  const [busy, setBusy] = useState(false);
  const [ask, setAsk] = useState<Ask | null>(null);
  const [menu, setMenu] = useState<{ x: number; y: number; node: TreeNode } | null>(null);
  const [syntaxTip, setSyntaxTip] = useState<string | null>(null);
  /** After a reload, the rule to select again: its panel, name and which of that name. */
  const reselect = useRef<{ panel: PanelType; name: string } | null>(null);

  const tree = useMemo(() => (view && view.ok ? buildTree(view.entries, drafts) : []), [view, drafts]);
  const nodeById = useCallback((id: number | null) => tree.find((n) => n.id === id), [tree]);

  const question = (title: string, message: string, buttons: Ask["buttons"]) => new Promise<string>((resolve) => setAsk({ title, message, buttons, resolve }));
  const showError = (message: string) => question("Error", message, [{ label: "OK", value: "ok", primary: true }]);

  const load = useCallback(async () => {
    try {
      setView(await fetchRuleEditor());
    } catch (e) {
      setView({ ok: false, message: `The rules could not be read: ${e instanceof Error ? e.message : String(e)}`, entries: [], layer_options: {}, codes: {}, defaults: {} as RuleFields, nets: [], netclasses: [], areas: [] });
    }
  }, []);

  // Opened: the file read afresh, the tree collapsed, the first visible item selected (`InitRuleTreeItems`).
  useEffect(() => {
    if (!open) return;
    setDrafts([]);
    setPanel(null);
    setModified(false);
    setStarred(new Set());
    setFilter("");
    setExpanded(new Set());
    setSelected(2);
    void load();
  }, [open, load]);

  // The board changed (an edit here, an agent, Board Setup): the file is read again, as KiCad rebuilds the dialog when the file changed on disk.
  useEffect(() => {
    if (open) void load();
  }, [state.version]); // eslint-disable-line react-hooks/exhaustive-deps

  // After a reload: select the rule that was saved or moved.
  useEffect(() => {
    const want = reselect.current;
    if (!want || !view?.ok) return;
    const hit = [...tree].reverse().find((n) => n.type === "rule" && !n.draft && n.panel === want.panel && n.name === want.name);
    reselect.current = null;
    if (hit) selectNode(hit, true);
  }, [tree]); // eslint-disable-line react-hooks/exhaustive-deps

  const markModified = (nodeId?: number) => {
    setModified(true);
    const id = nodeId ?? selected;
    if (id !== null) setStarred((s) => new Set(s).add(id));
  };
  const clearModified = () => {
    setModified(false);
    setStarred(new Set());
  };

  const ancestorsOf = (n: TreeNode): number[] => {
    const out: number[] = [];
    let p = n.parent;
    while (p !== null) {
      out.push(p);
      p = nodeById(p)?.parent ?? null;
    }
    return out;
  };

  // `RuleTreeItemSelectionChanged`: a rule opens its panel, any other node the group header.
  const selectNode = (n: TreeNode, force = false) => {
    if (!force && n.id === selected) return;
    setSelected(n.id);
    setExpanded((e) => {
      const next = new Set(e);
      for (const a of ancestorsOf(n)) next.add(a);
      return next;
    });
    setSyntaxTip(null);
    setPanel(view ? panelFrom(n, view) : null);
  };

  const close = () => {
    setAsk(null);
    setMenu(null);
    dispatch({ type: "SET_RULE_EDITOR_DIALOG_OPEN", open: false });
  };

  // `buildRuleTreeNode`: a new rule under the constraint node (or the rule's), named after it.
  const addRule = (from: TreeNode, copyOf?: TreeNode) => {
    if (!view) return;
    const constraint = from.type === "constraint" ? from : nodeById(from.parent);
    if (!constraint?.panel) return;
    const base = copyOf ? duplicateBaseName(copyOf.name) : constraint.name;
    const name = uniqueNodeName(tree, base);
    const src = copyOf && (copyOf.draft ?? (copyOf.entry !== undefined ? view.entries[copyOf.entry] : undefined));
    const draft: RuleDraft = src
      ? {
          panel: constraint.panel,
          rule_name: name,
          comment: src.comment,
          rule_condition: src.rule_condition,
          constraint_code: src.constraint_code,
          layer_token: src.layer_token,
          severity: src.severity,
          fields: { ...src.fields },
        }
      : { panel: constraint.panel, rule_name: name, comment: "", rule_condition: "", constraint_code: view.codes[constraint.panel] ?? "", layer_token: "", severity: "undefined", fields: { ...view.defaults } };
    draftIds += 1;
    const node: TreeNode = { id: draftIds, name, type: "rule", parent: constraint.id, panel: constraint.panel, draft };
    setDrafts((d) => [...d, node]);
    setModified(true);
    setSelected(node.id);
    setExpanded((e) => new Set([...e, constraint.id, ...ancestorsOf(constraint)]));
    setPanel(panelFrom(node, view));
  };

  /** The names of the tree's rules the file does not have (`validateRuleName` also sees the new ones). */
  const draftNames = (except: number) => drafts.filter((d) => d.id !== except).map((d) => d.name);

  // Save (`onSaveButtonClicked` -> `saveRule`): checked first, then one verb.
  const save = async (): Promise<boolean> => {
    if (!panel || busy) return false;
    const rule = payloadOf(panel, draftNames(panel.nodeId));
    setBusy(true);
    try {
      const checked = await postRuleEditorCheck({ entry: panel.entry, rule });
      if (!checked.ok || !checked.valid) {
        await showError(checked.message);
        return false;
      }
      const ok = await api.cmd({ op: "edit_drc_rule", action: "save", entry: panel.entry, name: panel.entryName, rule });
      if (!ok) return false;
      if (panel.draft) setDrafts((d) => d.filter((n) => n.id !== panel.nodeId));
      reselect.current = { panel: panel.panel, name: panel.name };
      clearModified();
      await load();
      return true;
    } finally {
      setBusy(false);
    }
  };

  // `RemoveRule`: a new rule goes at once, a saved one after "Are you sure you want to delete?".
  const removeRule = async (n: TreeNode) => {
    if (n.type !== "rule") return;
    if (n.draft) {
      setDrafts((d) => d.filter((x) => x.id !== n.id));
      if (n.id === selected) {
        setPanel(null);
        setSelected(n.parent);
      }
      clearModified();
      return;
    }
    const answer = await question("Confirmation", "Are you sure you want to delete?", [
      { label: "Cancel", value: "cancel" },
      { label: "Delete", value: "ok", primary: true },
    ]);
    if (answer !== "ok" || !view) return;
    setBusy(true);
    try {
      const ok = await api.cmd({ op: "edit_drc_rule", action: "delete", entry: n.entry ?? null, name: n.name, rule: undefined });
      if (ok) {
        setPanel(null);
        setSelected(n.parent);
        clearModified();
        await load();
      }
    } finally {
      setBusy(false);
    }
  };

  /** Move Up / Move Down: among the file's rules of the same node. */
  const persistedSiblings = (n: TreeNode) => (n.parent === null ? [] : childrenOf(tree, n.parent).filter((s) => !s.draft));
  const canMove = (n: TreeNode | undefined, up: boolean) => {
    if (!n || n.type !== "rule" || n.draft) return false;
    const sib = persistedSiblings(n);
    const at = sib.findIndex((s) => s.id === n.id);
    return up ? at > 0 : at >= 0 && at < sib.length - 1;
  };
  const moveRule = async (n: TreeNode, up: boolean) => {
    if (!canMove(n, up)) return;
    setBusy(true);
    try {
      const ok = await api.cmd({ op: "edit_drc_rule", action: up ? "move_up" : "move_down", entry: n.entry ?? null, name: n.name, rule: undefined });
      if (ok) {
        reselect.current = { panel: n.panel!, name: n.name };
        await load();
      }
    } finally {
      setBusy(false);
    }
  };

  // Close / Discard (`OnCancel`): a new rule's panel is cancelled first (the rule goes, the dialog stays); unsaved changes are asked about.
  const cancel = async () => {
    if (panel?.draft) {
      const id = panel.nodeId;
      const rest = drafts.filter((d) => d.id !== id);
      setDrafts(rest);
      setPanel(null);
      setSelected(nodeById(id)?.parent ?? 2);
      // `Cancel` -> `RemoveRule`, which clears the modified state whatever else is new (`OnCancel`'s own check after it finds it cleared).
      clearModified();
      return;
    }
    if (modified) {
      const names = [...drafts.map((d) => d.name), ...(panel && starred.has(panel.nodeId) ? [panel.name] : [])];
      if (names.length > 0) {
        const answer = await question("Save Changes?", unsavedChangesMessage(names), [
          { label: "Cancel", value: "cancel" },
          { label: "No", value: "no" },
          { label: "Yes", value: "yes", primary: true },
        ]);
        if (answer === "cancel") return;
        if (answer === "yes" && panel && starred.has(panel.nodeId)) {
          const rule = payloadOf(panel, draftNames(panel.nodeId));
          const checked = await postRuleEditorCheck({ entry: panel.entry, rule });
          if (!checked.ok || !checked.valid) {
            await showError(`Cannot save due to validation errors in rule '${panel.name}':\n\n${checked.message}`);
            return;
          }
          if (!(await save())) return;
        }
      }
    }
    close();
  };

  // Ctrl+S saves (`OnCharHook`), Escape is Close.
  useEffect(() => {
    if (!open) return;
    const onKey = (e: KeyboardEvent) => {
      if (ask) return;
      if (e.key.toLowerCase() === "s" && (e.ctrlKey || e.metaKey) && !e.shiftKey && !e.altKey) {
        e.preventDefault();
        e.stopPropagation();
        void save();
      }
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  });

  if (!open) return null;

  const sel = nodeById(selected);
  const visible = filter ? filterTree(tree, filter, (n) => n.name) : null;
  const update = (patch: Partial<PanelState>) => {
    if (!panel) return;
    setPanel({ ...panel, ...patch });
    markModified(panel.nodeId);
  };

  const renderNode = (n: TreeNode, depth: number): ReactNode => {
    if (visible && !visible.has(n.id)) return null;
    const kids = childrenOf(tree, n.id);
    const isOpen = expanded.has(n.id) || !!visible;
    const text = n.draft || starred.has(n.id) ? `${n.name} *` : n.name;
    return (
      <li key={n.id}>
        <div
          className={`re-item${n.id === selected ? " selected" : ""}`}
          style={{ paddingLeft: 4 + depth * 14 }}
          data-testid={`re-node-${n.type}`}
          data-name={n.name}
          onClick={() => selectNode(n)}
          onDoubleClick={() => (isEnabled(n, "duplicate_rule") ? addRule(n, n) : isEnabled(n, "add_rule") ? addRule(n) : undefined)}
          onContextMenu={(e) => {
            e.preventDefault();
            selectNode(n);
            setMenu({ x: e.clientX, y: e.clientY, node: n });
          }}
        >
          <span
            className="re-twisty"
            onClick={(e) => {
              e.stopPropagation();
              setExpanded((s) => {
                const next = new Set(s);
                if (next.has(n.id)) next.delete(n.id);
                else next.add(n.id);
                return next;
              });
            }}
          >
            {kids.length > 0 ? (isOpen ? "▾" : "▸") : ""}
          </span>
          {text}
        </div>
        {kids.length > 0 && isOpen && <ul>{kids.map((k) => renderNode(k, depth + 1))}</ul>}
      </li>
    );
  };

  const root = tree.find((n) => n.type === "root");
  const canAdd = !!sel && isEnabled(sel, "add_rule");
  const canRule = !!sel && isEnabled(sel, "delete_rule");

  return (
    <div className="dialog-backdrop" onClick={() => setMenu(null)}>
      <div className="dialog re-dialog" data-testid="rule-editor" onClick={(e) => e.stopPropagation()}>
        <div className="dialog-header">
          <span>Design Rule Editor</span>
        </div>
        <div className="re-body">
          <div className="re-tree-pane">
            <input className="re-filter" type="search" placeholder="Type filter text" value={filter} data-testid="re-filter" onChange={(e) => setFilter(e.target.value)} />
            <ul className="re-tree" data-testid="re-tree">
              {root && childrenOf(tree, root.id).map((n) => renderNode(n, 0))}
            </ul>
            <div className="re-tree-buttons">
              <button title="New Rule" data-testid="re-add" disabled={!canAdd || busy} onClick={() => sel && addRule(sel)}>
                <Icon name="small_plus" />
              </button>
              <button title="Copy Rule" data-testid="re-copy" disabled={!canRule || busy} onClick={() => sel && addRule(sel, sel)}>
                <Icon name="copy" />
              </button>
              <button title="Move Up" data-testid="re-up" disabled={!canMove(sel, true) || busy} onClick={() => sel && void moveRule(sel, true)}>
                <Icon name="small_up" />
              </button>
              <button title="Move Down" data-testid="re-down" disabled={!canMove(sel, false) || busy} onClick={() => sel && void moveRule(sel, false)}>
                <Icon name="small_down" />
              </button>
              <span style={{ width: 20 }} />
              <button title="Delete Rule" data-testid="re-delete" disabled={!canRule || busy} onClick={() => sel && void removeRule(sel)}>
                <Icon name="small_trash" />
              </button>
            </div>
          </div>
          <div className="re-content" data-testid="re-content">
            {!view ? (
              <p className="re-note">Reading the rules...</p>
            ) : !view.ok ? (
              <p className="re-note re-error" data-testid="re-load-error">
                {view.message}
              </p>
            ) : panel ? (
              <RulePanel
                key={panel.nodeId}
                p={panel}
                view={view}
                constraintName={nodeById(nodeById(panel.nodeId)?.parent ?? null)?.name ?? ""}
                update={update}
                onEnter={() => void save()}
                syntaxTip={syntaxTip}
                checkSyntax={async () => {
                  const r = await postRuleEditorCheck({ syntax: { name: panel.name, body: panel.customText } });
                  setSyntaxTip(r.message);
                }}
              />
            ) : sel ? (
              <GroupHeader rows={groupRows(tree, sel.id, (n) => (n.draft ? n.draft.comment : n.entry !== undefined ? (view.entries[n.entry]?.comment ?? "") : ""))} />
            ) : null}
          </div>
        </div>
        <div className="dialog-footer">
          <button data-testid="re-cancel" onClick={() => void cancel()}>
            {modified ? "Discard" : "Close"}
          </button>
          <button className="primary" data-testid="re-save" disabled={!modified || !panel || busy} onClick={() => void save()}>
            Save
          </button>
        </div>
        {menu && (
          <div className="re-menu" style={{ left: menu.x, top: menu.y }} data-testid="re-menu" onClick={() => setMenu(null)}>
            {isEnabled(menu.node, "add_rule") && <button onClick={() => addRule(menu.node)}>New Rule</button>}
            {isEnabled(menu.node, "add_rule") && <button onClick={() => (menu.node.type === "rule" ? addRule(menu.node, menu.node) : undefined)}>Copy Rule</button>}
            {isEnabled(menu.node, "delete_rule") && <button onClick={() => void removeRule(menu.node)}>Delete Rule</button>}
            {canMove(menu.node, true) && <button onClick={() => void moveRule(menu.node, true)}>Move Up</button>}
            {canMove(menu.node, false) && <button onClick={() => void moveRule(menu.node, false)}>Move Down</button>}
          </div>
        )}
        {ask && (
          <div className="dialog-backdrop re-ask-backdrop">
            <div className="dialog re-ask" data-testid="re-ask">
              <div className="dialog-header">
                <span>{ask.title}</span>
              </div>
              <div className="dialog-body">
                <pre className="re-ask-text" data-testid="re-ask-text">
                  {ask.message}
                </pre>
              </div>
              <div className="dialog-footer">
                {ask.buttons.map((b) => (
                  <button
                    key={b.value}
                    className={b.primary ? "primary" : undefined}
                    data-testid={`re-ask-${b.value}`}
                    onClick={() => {
                      const r = ask.resolve;
                      setAsk(null);
                      r(b.value);
                    }}
                  >
                    {b.label}
                  </button>
                ))}
              </div>
            </div>
          </div>
        )}
      </div>
    </div>
  );
}

/** `PANEL_DRC_GROUP_HEADER`. */
function GroupHeader({ rows }: { rows: { type: string; name: string; comment: string }[] }) {
  return (
    <table className="re-grid" data-testid="re-group">
      <thead>
        <tr>
          <th>Rule type</th>
          <th>Rule name</th>
          <th>Comment</th>
        </tr>
      </thead>
      <tbody>
        {rows.map((r, i) => (
          <tr key={i}>
            <td>{r.type}</td>
            <td>{r.name}</td>
            <td>{r.comment}</td>
          </tr>
        ))}
      </tbody>
    </table>
  );
}

/** `PANEL_DRC_RULE_EDITOR`: name and comment, the constraint panel, the conditions, the layer. */
function RulePanel({
  p,
  view,
  constraintName,
  update,
  onEnter,
  syntaxTip,
  checkSyntax,
}: {
  p: PanelState;
  view: RuleEditorView;
  constraintName: string;
  update: (patch: Partial<PanelState>) => void;
  /** Enter in the name, the comment or a value field saves (`OnEnterKey`, the overlay fields' `wxEVT_TEXT_ENTER`). */
  onEnter: () => void;
  syntaxTip: string | null;
  checkSyntax: () => Promise<void>;
}) {
  const options = view.layer_options[p.panel] ?? [];
  const custom = p.panel === "custom_rule";
  const twoObjects = needsTwoObjects(p.panel);
  return (
    <div className="re-panel" data-testid="re-panel" data-panel={p.panel}>
      <div className="re-row">
        <label>
          Name:{" "}
          <input data-testid="re-name" value={p.name} onChange={(e) => update({ name: e.target.value })} onKeyDown={(e) => e.key === "Enter" && onEnter()} />
        </label>
        <label>
          Comment:{" "}
          <input data-testid="re-comment" value={p.comment} onChange={(e) => update({ comment: e.target.value })} onKeyDown={(e) => e.key === "Enter" && onEnter()} />
        </label>
      </div>
      <h4 className="re-section">{constraintName} Constraint</h4>
      {custom ? (
        <div className="re-custom">
          <textarea data-testid="re-custom-text" spellCheck={false} value={p.customText} onChange={(e) => update({ customText: e.target.value })} />
          <div className="re-custom-check">
            {syntaxTip && (
              <span className="re-tip" data-testid="re-syntax-tip">
                {syntaxTip}
              </span>
            )}
            <button data-testid="re-check-syntax" onClick={() => void checkSyntax()}>
              Check Syntax
            </button>
          </div>
        </div>
      ) : (
        <Overlay p={p} update={update} onEnter={onEnter} />
      )}
      {!custom && (
        <>
          <h4 className="re-section">Conditions</h4>
          <ConditionGroup rows={p.rows} twoObjects={twoObjects} view={view} onChange={(rows) => update({ rows })} />
          {options.length > 1 && (
            <>
              <h4 className="re-section">Layer</h4>
              <select data-testid="re-layer" value={p.layer} onChange={(e) => update({ layer: e.target.value })}>
                {options.map((o) => (
                  <option key={o.token} value={o.token}>
                    {o.label}
                  </option>
                ))}
              </select>
            </>
          )}
        </>
      )}
    </div>
  );
}

/** `DRC_RE_BITMAP_OVERLAY_PANEL`: the constraint's picture with its fields where `GetFieldPositions` puts them. */
function Overlay({ p, update, onEnter }: { p: PanelState; update: (patch: Partial<PanelState>) => void; onEnter: () => void }) {
  const scheme = useColorScheme();
  const spec = overlaySpec(p.panel);
  if (!spec) return null;
  const right = Math.max(...spec.fields.map((f) => f.x1 + (f.label ? 8 * f.label.length + 12 : 0)));
  const bottom = Math.max(...spec.fields.map((f) => f.y + 28));
  const field = (f: OverlayField) => {
    const style = { left: f.x0, top: f.y, width: Math.max(f.x1 - f.x0, 14) };
    if (f.kind === "number") {
      return (
        <input
          key={f.key}
          className="re-field"
          style={style}
          data-testid={`re-field-${f.key}`}
          value={p.text[f.key] ?? ""}
          onChange={(e) => update({ text: { ...p.text, [f.key]: e.target.value } })}
          onKeyDown={(e) => e.key === "Enter" && onEnter()}
        />
      );
    }
    if (f.kind === "choice") {
      return (
        <select key={f.key} className="re-field" style={style} data-testid="re-field-via_type" value={p.fields.via_type} onChange={(e) => update({ fields: { ...p.fields, via_type: e.target.value as ViaType } })}>
          <option value="any">Any</option>
          <option value="through">Through</option>
          <option value="micro">Micro</option>
          <option value="blind">Blind</option>
          <option value="buried">Buried</option>
        </select>
      );
    }
    const key = f.key as keyof RuleFields;
    return (
      <input
        key={f.key}
        type="checkbox"
        className="re-check"
        style={{ left: f.x0, top: f.y }}
        data-testid={`re-field-${f.key}`}
        checked={Boolean(p.fields[key])}
        onChange={(e) => update({ fields: p.panel === "allowed_orientation" ? toggleOrientation(p.fields, key, e.target.checked) : { ...p.fields, [key]: e.target.checked } })}
      />
    );
  };
  return (
    <div className="re-overlay" style={{ minWidth: right, minHeight: bottom }} data-testid="re-overlay">
      <img src={`/icons/${scheme}/${spec.bitmap}.png`} alt="" draggable={false} />
      {(spec.decor ?? []).map((d, i) => (
        <span key={`decor${i}`} className="re-label" style={{ left: d.x, top: d.y + 3 }}>
          {d.text}
        </span>
      ))}
      {spec.fields.map((f) => (
        <span key={f.key}>
          {field(f)}
          {f.plusMinus && (
            <span className="re-label" style={{ left: f.x0 - 10, top: f.y + 3 }}>
              ±
            </span>
          )}
          {f.label && f.labelAt === "right" && (
            <span className="re-label" style={{ left: f.x1 + (f.kind === "check" ? 6 : 4), top: f.y + 3 }}>
              {f.label}
            </span>
          )}
          {f.label && f.labelAt === "left" && (
            <span className="re-label re-label-left" style={{ right: `calc(100% - ${f.x0 - 4}px)`, top: f.y + 3 }}>
              {f.label}
            </span>
          )}
        </span>
      ))}
    </div>
  );
}

/** `DRC_RE_CONDITION_GROUP_PANEL` and its `DRC_RE_CONDITION_ROW_PANEL`s. */
function ConditionGroup({ rows, twoObjects, view, onChange }: { rows: ConditionRow[]; twoObjects: boolean; view: RuleEditorView; onChange: (rows: ConditionRow[]) => void }) {
  const set = (i: number, patch: Partial<ConditionRow>) => onChange(rows.map((r, j) => (j === i ? { ...r, ...patch } : r)));
  const listFor = (t: ConditionType, value: string) => {
    const list = t === "net" ? view.nets : t === "netclass" ? view.netclasses : view.areas;
    return value && !list.includes(value) ? [value, ...list] : list;
  };
  return (
    <div className="re-conditions" data-testid="re-conditions">
      <div className="re-cond-label">{twoObjects ? "Where conditions match:" : "Where object matches:"}</div>
      {rows.map((r, i) => (
        <div className="re-cond-row" key={i} data-testid="re-cond-row">
          {i > 0 && (
            <select data-testid="re-cond-op" value={r.op} onChange={(e) => set(i, { op: e.target.value as BoolOperator })}>
              {(Object.keys(OPERATOR_LABELS) as BoolOperator[]).map((o) => (
                <option key={o} value={o}>
                  {OPERATOR_LABELS[o]}
                </option>
              ))}
            </select>
          )}
          {twoObjects && (
            <select data-testid="re-cond-object" value={r.object} onChange={(e) => set(i, { object: e.target.value as "A" | "B" })}>
              <option value="A">Object A</option>
              <option value="B">Object B</option>
            </select>
          )}
          <select data-testid="re-cond-type" value={r.type} onChange={(e) => set(i, { type: e.target.value as ConditionType, value: e.target.value === r.type ? r.value : "" })}>
            {(Object.keys(CONDITION_LABELS) as ConditionType[]).map((t) => (
              <option key={t} value={t}>
                {CONDITION_LABELS[t]}
              </option>
            ))}
          </select>
          {(r.type === "net" || r.type === "netclass" || r.type === "within_area") && (
            <select data-testid="re-cond-value" value={r.value} onChange={(e) => set(i, { value: e.target.value })}>
              <option value=""></option>
              {listFor(r.type, r.value).map((v) => (
                <option key={v} value={v}>
                  {v}
                </option>
              ))}
            </select>
          )}
          {r.type === "custom" && <textarea data-testid="re-cond-custom" className="re-cond-custom" spellCheck={false} value={r.value} onChange={(e) => set(i, { value: e.target.value })} />}
          {rows.length > 1 && (
            <button title="Remove this condition" data-testid="re-cond-remove" onClick={() => onChange(rows.filter((_, j) => j !== i))}>
              <Icon name="trash" />
            </button>
          )}
        </div>
      ))}
      <div className="re-cond-add">
        <button title="Add another condition" data-testid="re-cond-add" onClick={() => onChange([...rows, { op: "and", object: "A", type: "any", value: "" }])}>
          <Icon name="small_plus" />
        </button>
      </div>
    </div>
  );
}
