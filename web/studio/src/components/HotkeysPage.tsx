// Preferences > Hotkeys (`PANEL_HOTKEYS_EDITOR`, common/dialogs/panel_hotkeys_editor.cpp at KiCad 8303b2ad) and its list (`WIDGET_HOTKEY_LIST`,
// common/widgets/widget_hotkey_list.cpp); the rules are kicad-port/hotkeyStore.ts', the person's keys state/userHotkeys.ts'.
//
//   * The list: "Type filter text" (`HOTKEY_FILTER`: the command or the hotkey's name), a section per application (3D Viewer, Common, Schematic Editor, PCB Editor;
//     Close and Quit at the top of Common and the Gestures last, read-only), columns "Command (double-click to edit)" / Hotkey / Alternate / Description, " *" on a
//     row whose hotkey is not the saved one. A click on a column's title sorts by it, a second click the other way (`wxCOL_SORTABLE`, hotkeyStore.ts
//     `sortHotkeySections`; UI-LEFT-3 C3-4).
//   * Double click (or Enter) on a row opens "Set Hotkey" ("Press a new hotkey, or press Esc to cancel...", Command:, Current key:, "Clear assigned hotkey"): the
//     key pressed becomes the hotkey once it is released (`HK_PROMPT_DIALOG::OnKeyUp`); Esc changes nothing. A right click: Edit..., Edit Alternate..., Undo Changes,
//     Clear Assigned Hotkey, Clear Assigned Alternate, Restore Defaults (`onContextMenu`, `resetItem`). A key another action of the section or of Common has asks
//     "'%s' is already assigned to '%s' in section '%s'. Are you sure you want to change its assignment?" ("Confirm change", No the default); Yes takes the key off
//     the other action's hotkey (`resolveKeyConflicts`). Ctrl+Tab and Ctrl+Shift+Tab are refused ("'%s' is a reserved hotkey in KiCad and cannot be assigned.").
//   * "Undo All Changes" (`ResetAllHotkeys( false )`), "Import Hotkeys..." (a `.hotkeys` file, `ImportHotKeys`: the keys it names replace the list's), and the
//     dialog's "Reset Hotkeys to Defaults" (`ResetPanel`: every hotkey to KiCad's). OK (`TransferDataFromWindow`) saves the list as `user.hotkeys`
//     (`SaveAllHotkeys`, `WriteHotKeyConfig`); every action then answers to its new keys.
//
// Different from KiCad: "Export Hotkeys..." is the studio's -- KiCad writes user.hotkeys into its settings folder, where a person can copy it; the browser has no
// such folder, so the button saves the list as it stands as a `user.hotkeys` file. The rows of the platform commands and the gestures are read-only here (KiCad
// lets them be edited, but they are no actions and nothing it writes keeps the change).
import { useEffect, useMemo, useRef, useState, type KeyboardEvent as ReactKeyboardEvent, type ReactNode } from "react";
import { createPortal } from "react-dom";
import type { PreferencesPageHandle } from "./preferencesPage";
import { ContextMenu } from "./canvas/ContextMenu";
import { hotkeyActions } from "../actions/hotkeyActions";
import { getUserHotkeyConfig, writeUserHotkeys } from "../state/userHotkeys";
import { ADVANCED_CFG_ONLY_ACTIONS } from "../kicad/advancedConfig";
import { saveTextFile } from "../api/libraryClient";
import { isMac } from "../platform";
import {
  CONFLICT_TITLE,
  HOTKEY_MENU,
  PROMPT_TEXT,
  applyHotkeyChange,
  conflictMessage,
  filterMatches,
  findHotkey,
  hotkeyColumns,
  hotkeyRow,
  importHotkeys,
  initHotkeySections,
  isEditable,
  mapKeypressToKeycode,
  planHotkeyChange,
  readHotKeyConfig,
  reservedMessage,
  resetAllHotkeys,
  resetSteps,
  savedKeys,
  nextHotkeySort,
  sortHotkeySections,
  writeHotKeyConfig,
  type Hotkey,
  type HotkeyAction,
  type HotkeyMenuId,
  type HotkeySection,
  type HotkeySort,
} from "../kicad-port/hotkeyStore";

const isWindows = () => typeof navigator !== "undefined" && /^Win/.test(navigator.platform ?? "");

/** `HOTKEY_STORE::Init( m_actions, true )` (`TransferDataToWindow` always lists the read-only commands). */
function initialSections(): HotkeySection[] {
  return initHotkeySections(hotkeyActions(), true, { windows: isWindows(), excluded: ADVANCED_CFG_ONLY_ACTIONS });
}

interface Prompt {
  action: HotkeyAction;
  alternate: boolean;
  name: string;
  current: string;
}

interface Question {
  title: string;
  message: string;
  /** Yes / No (a question), or OK only (an error). */
  yesNo: boolean;
  onYes: () => void;
  onNo: () => void;
}

/** A small modal over the Preferences dialog (the studio's dialog style). */
function Modal({ label, title, width = 420, onBackdrop, onKeyDown, onKeyUp, children, footer }: { label: string; title: string; width?: number; onBackdrop: () => void; onKeyDown?: (e: ReactKeyboardEvent) => void; onKeyUp?: (e: ReactKeyboardEvent) => void; children: ReactNode; footer?: ReactNode }) {
  return createPortal(
    <div className="dialog-backdrop" style={{ zIndex: 3000 }} onClick={onBackdrop}>
      <div role="dialog" aria-label={label} className="dialog" style={{ width }} onClick={(e) => e.stopPropagation()} onDoubleClick={(e) => e.stopPropagation()} onKeyDown={onKeyDown} onKeyUp={onKeyUp}>
        <div className="dialog-header">
          <span>{title}</span>
        </div>
        <div className="dialog-body">{children}</div>
        {footer && <div className="dialog-footer">{footer}</div>}
      </div>
    </div>,
    document.body
  );
}

/** `HK_PROMPT_DIALOG`: the key pressed (on its release), 0 for "Clear assigned hotkey", null for Esc or a click outside. */
function SetHotkeyPrompt({ prompt, onDone }: { prompt: Prompt; onDone: (key: number | null) => void }) {
  const pending = useRef<number | undefined>(undefined);
  const focusRef = useRef<HTMLDivElement>(null);
  useEffect(() => focusRef.current?.focus(), []);
  const mac = isMac();
  return (
    <Modal
      label={PROMPT_TEXT.title}
      title={PROMPT_TEXT.title}
      width={380}
      onBackdrop={() => onDone(null)}
      // `OnCharHook` / `OnChar`: every key is the prompt's -- no editor action, no button press, no browser shortcut a page may stop
      onKeyDown={(e) => {
        e.preventDefault();
        e.stopPropagation();
        const key = mapKeypressToKeycode(e, mac);
        if (key !== null) pending.current = key;
      }}
      // `OnKeyUp`: a key was pressed in the prompt, so the dialog closes (a key held when it opened, the Enter that opened it, does not)
      onKeyUp={(e) => {
        e.preventDefault();
        e.stopPropagation();
        if (pending.current === undefined) return;
        // `PromptForKey`: "The ESC key was used to close the dialog" -- no change
        onDone(pending.current ? pending.current : null);
      }}
    >
      <div ref={focusRef} tabIndex={0} data-testid="set-hotkey-prompt" style={{ outline: "none" }}>
        <div style={{ textAlign: "center", margin: "4px 0 10px" }}>{PROMPT_TEXT.instruction}</div>
        <hr style={{ border: 0, borderTop: "1px solid var(--chrome-border)", margin: "4px 0 8px" }} />
        <div className="kv-grid" style={{ gridTemplateColumns: "auto 1fr", rowGap: 6, columnGap: 10 }}>
          <span>{PROMPT_TEXT.command}</span>
          <b data-testid="set-hotkey-command">{prompt.name}</b>
          <span>{PROMPT_TEXT.current}</span>
          <b data-testid="set-hotkey-current">{prompt.current}</b>
        </div>
        <div style={{ textAlign: "center", marginTop: 10 }}>
          <button
            onClick={(e) => {
              e.stopPropagation();
              onDone(0);
            }}
          >
            {PROMPT_TEXT.clear}
          </button>
        </div>
      </div>
      <div style={{ fontSize: 11, opacity: 0.6, marginTop: 8, textAlign: "center" }}>Keys the browser keeps for itself (such as Ctrl+W or Ctrl+T) cannot be pressed here.</div>
    </Modal>
  );
}

export interface HotkeysPanel {
  element: ReactNode;
  /** `TransferDataFromWindow`: `SaveAllHotkeys`, `WriteHotKeyConfig`. */
  commit: () => void;
  /** `ResetPanel`: `ResetAllHotkeys( true )`. */
  resetToDefaults: () => void;
}

/**
 * The panel's state while its dialog is up (`open`): the Preferences page (`readOnly` false) or List Hotkeys' read-only panel (`DIALOG_LIST_HOTKEYS`:
 * `PANEL_HOTKEYS_EDITOR( aParent, this, true )` -- the list cannot be edited, its buttons and OK still work). `extraButtons` go at the right of the button row
 * (List Hotkeys' OK and Cancel, `GetBottomSizer()->Add( sdb_sizer )`).
 */
export function useHotkeysPanel(open: boolean, readOnly: boolean, extraButtons?: ReactNode): HotkeysPanel {
  const mac = isMac();
  const [sections, setSectionsState] = useState<HotkeySection[]>([]);
  const sectionsRef = useRef<HotkeySection[]>([]);
  const setSections = (next: HotkeySection[]) => {
    sectionsRef.current = next;
    setSectionsState(next);
  };
  const [filter, setFilter] = useState("");
  /** The column the list is sorted by (a click on its title), none until one is clicked: a new list each time the dialog opens. */
  const [sort, setSort] = useState<HotkeySort | null>(null);
  const [selected, setSelected] = useState<HotkeyAction | null>(null);
  const [collapsed, setCollapsed] = useState<ReadonlySet<string>>(new Set());
  const [prompt, setPrompt] = useState<Prompt | null>(null);
  const [question, setQuestion] = useState<Question | null>(null);
  const [menu, setMenu] = useState<{ x: number; y: number; hotkey: Hotkey } | null>(null);
  const fileRef = useRef<HTMLInputElement>(null);

  // `TransferDataToWindow`: the list from the actions' current keys each time the dialog opens
  useEffect(() => {
    if (!open) return;
    setSections(initialSections());
    setFilter("");
    setSort(null);
    setSelected(null);
    setCollapsed(new Set());
    setPrompt(null);
    setQuestion(null);
    setMenu(null);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open]);

  /** `changeHotkey` once per step, the conflict question answered between steps (`resetItem`'s two calls). */
  const runSteps = (action: HotkeyAction, steps: Array<{ key: number; alternate: boolean }>) => {
    const [step, ...rest] = steps;
    if (!step) return;
    const h = findHotkey(sectionsRef.current, action);
    if (!h) return;
    const plan = planHotkeyChange(sectionsRef.current, h, step.key);
    if (plan.kind === "none") return runSteps(action, rest);
    if (plan.kind === "apply") {
      setSections(applyHotkeyChange(sectionsRef.current, h, step.key, step.alternate));
      return runSteps(action, rest);
    }
    setQuestion({
      title: CONFLICT_TITLE,
      message: conflictMessage(step.key, plan.conflict, mac),
      yesNo: true,
      onYes: () => {
        setQuestion(null);
        const now = findHotkey(sectionsRef.current, action);
        const other = findHotkey(sectionsRef.current, plan.conflict.action);
        if (now) setSections(applyHotkeyChange(sectionsRef.current, now, step.key, step.alternate, other));
        runSteps(action, rest);
      },
      onNo: () => {
        setQuestion(null);
        runSteps(action, rest);
      },
    });
  };

  /** `editItem`: the prompt, then the reserved keys' refusal, then `changeHotkey`. */
  const editItem = (h: Hotkey, alternate: boolean) => {
    if (readOnly || !isEditable(h)) return;
    const row = hotkeyRow(h, mac);
    setPrompt({ action: h.action, alternate, name: row.label, current: alternate ? row.alt : row.key });
  };
  const promptDone = (key: number | null) => {
    const p = prompt;
    setPrompt(null);
    if (!p || key === null) return;
    const reserved = reservedMessage(key);
    if (reserved) {
      setQuestion({ title: "Error", message: reserved, yesNo: false, onYes: () => setQuestion(null), onNo: () => setQuestion(null) });
      return;
    }
    runSteps(p.action, [{ key, alternate: p.alternate }]);
  };

  const onMenu = (id: HotkeyMenuId, h: Hotkey) => {
    if (id === "edit" || id === "edit_alt") editItem(h, id === "edit_alt");
    else runSteps(h.action, resetSteps(h, id));
  };

  /** `ImportHotKeys`: the file's keys over the list's (not saved until OK). */
  const importFile = async (file: File | undefined) => {
    if (!file) return;
    const text = await file.text();
    setSections(importHotkeys(sectionsRef.current, readHotKeyConfig(text, mac)));
  };

  const shownSections = useMemo(() => sortHotkeySections(sections.map((s) => ({ ...s, hotkeys: s.hotkeys.filter((h) => filterMatches(h, filter, mac)) })), sort, mac), [sections, filter, sort, mac]);
  const columns = hotkeyColumns(readOnly);

  const cell = { padding: "2px 8px", whiteSpace: "nowrap" as const, overflow: "hidden", textOverflow: "ellipsis" };
  const element = (
    <div data-testid="hotkeys-panel" data-readonly={readOnly ? "true" : "false"} style={{ display: "flex", flexDirection: "column", gap: 6, minWidth: 0 }}>
      <input type="search" aria-label="Filter hotkeys" placeholder="Type filter text" value={filter} onChange={(e) => setFilter(e.target.value)} style={{ padding: "4px 8px" }} />
      <div style={{ height: 400, overflow: "auto", border: "1px solid var(--chrome-border)" }} role="tree" aria-label="Hotkeys">
        <table style={{ borderCollapse: "collapse", width: "100%", fontSize: 12, tableLayout: "fixed" }}>
          <colgroup>
            <col style={{ width: "36%" }} />
            <col style={{ width: "14%" }} />
            <col style={{ width: "14%" }} />
            <col style={{ width: "36%" }} />
          </colgroup>
          <thead>
            <tr style={{ position: "sticky", top: 0, background: "var(--chrome-bg, #222)" }}>
              {columns.map((c, i) => {
                const sorted = sort?.column === i ? (sort.ascending ? "ascending" : "descending") : "none";
                return (
                  <th
                    key={c}
                    aria-sort={sorted}
                    data-hotkey-column={i}
                    title="Click to sort by this column"
                    style={{ ...cell, textAlign: "left", fontWeight: 600, cursor: "pointer", userSelect: "none" }}
                    onClick={() => setSort((now) => nextHotkeySort(now, i as HotkeySort["column"]))}
                  >
                    {c}
                    {sorted !== "none" && <span aria-hidden style={{ marginLeft: 4 }}>{sorted === "ascending" ? "▲" : "▼"}</span>}
                  </th>
                );
              })}
            </tr>
          </thead>
          <tbody>
            {shownSections.map((s) => (
              <SectionRows
                key={s.name}
                section={s}
                collapsed={collapsed.has(s.name)}
                onToggle={() =>
                  setCollapsed((c) => {
                    const n = new Set(c);
                    if (n.has(s.name)) n.delete(s.name);
                    else n.add(s.name);
                    return n;
                  })
                }
              >
                {s.hotkeys.map((h) => {
                  const r = hotkeyRow(h, mac);
                  const sel = selected === h.action;
                  return (
                    <tr
                      key={h.action.pseudo ? `pseudo:${h.action.label}` : h.action.name}
                      role="treeitem"
                      aria-selected={sel}
                      tabIndex={0}
                      data-hotkey-action={h.action.pseudo ? undefined : h.action.name}
                      data-hotkey-label={h.action.label}
                      style={{ cursor: "default", background: sel ? "var(--chrome-accent-bg, rgba(74,163,255,0.25))" : undefined }}
                      onClick={() => setSelected(h.action)}
                      onDoubleClick={() => editItem(h, false)}
                      onKeyDown={(e) => {
                        if (e.key === "Enter") {
                          e.preventDefault();
                          e.stopPropagation();
                          editItem(h, false);
                        }
                      }}
                      onContextMenu={(e) => {
                        e.preventDefault();
                        setSelected(h.action);
                        if (!readOnly && isEditable(h)) setMenu({ x: e.clientX, y: e.clientY, hotkey: h });
                      }}
                    >
                      <td style={{ ...cell, paddingLeft: 22 }} title={r.label} data-col="command">
                        {r.label}
                      </td>
                      <td style={cell} data-col="hotkey">
                        {r.key}
                      </td>
                      <td style={cell} data-col="alternate">
                        {r.alt}
                      </td>
                      <td style={{ ...cell, opacity: 0.75 }} title={r.description} data-col="description">
                        {r.description}
                      </td>
                    </tr>
                  );
                })}
              </SectionRows>
            ))}
          </tbody>
        </table>
      </div>
      {/* `installButtons`: the left-hand buttons (`BUTTON_ROW_PANEL`) */}
      <div style={{ display: "flex", gap: 8, alignItems: "center" }}>
        <button title="Undo all changes made so far in this dialog" onClick={() => setSections(resetAllHotkeys(sectionsRef.current, false))}>
          Undo All Changes
        </button>
        <button title="Import hotkey definitions from an external file, replacing the current values" onClick={() => fileRef.current?.click()}>
          Import Hotkeys...
        </button>
        <button
          title="Save the hotkeys as they stand in this dialog as a user.hotkeys file (KiCad keeps that file in its settings folder)"
          onClick={() => saveTextFile(writeHotKeyConfig(getUserHotkeyConfig(), savedKeys(sectionsRef.current), mac), "user.hotkeys")}
        >
          Export Hotkeys...
        </button>
        <input
          ref={fileRef}
          type="file"
          accept=".hotkeys"
          aria-label="Import Hotkeys File:"
          data-testid="hotkeys-import-file"
          style={{ display: "none" }}
          onChange={(e) => {
            const input = e.currentTarget;
            void importFile(input.files?.[0]).finally(() => {
              input.value = "";
            });
          }}
        />
        {extraButtons && <span style={{ marginLeft: "auto", display: "flex", gap: 8 }}>{extraButtons}</span>}
      </div>
      {prompt && <SetHotkeyPrompt prompt={prompt} onDone={promptDone} />}
      {question && (
        <Modal
          label={question.title}
          title={question.title}
          onBackdrop={question.onNo}
          footer={
            question.yesNo ? (
              <>
                <button onClick={question.onYes}>Yes</button>
                {/* `wxYES_NO | wxNO_DEFAULT` */}
                <button className="primary" autoFocus onClick={question.onNo}>
                  No
                </button>
              </>
            ) : (
              <button className="primary" autoFocus onClick={question.onYes}>
                OK
              </button>
            )
          }
        >
          <div style={{ whiteSpace: "pre-line" }} data-testid="hotkeys-question">
            {question.message}
          </div>
        </Modal>
      )}
      {menu && (
        <ContextMenu
          x={menu.x}
          y={menu.y}
          onClose={() => setMenu(null)}
          entries={HOTKEY_MENU.map((m) => ({
            label: m.label,
            onSelect: () => {
              const h = findHotkey(sectionsRef.current, menu.hotkey.action);
              if (h) onMenu(m.id, h);
            },
          }))}
        />
      )}
    </div>
  );

  return {
    element,
    commit: () => writeUserHotkeys(savedKeys(sectionsRef.current)),
    resetToDefaults: () => setSections(resetAllHotkeys(sectionsRef.current, true)),
  };
}

/** A section's tree row (`AppendItem( GetRootItem(), section.m_SectionName )`, expanded) and its hotkeys. */
function SectionRows({ section, collapsed, onToggle, children }: { section: HotkeySection; collapsed: boolean; onToggle: () => void; children: ReactNode }) {
  return (
    <>
      <tr data-hotkey-section={section.name}>
        <td colSpan={4} style={{ padding: "3px 4px", fontWeight: 600, cursor: "pointer" }} onClick={onToggle}>
          <span style={{ display: "inline-block", width: 14 }}>{collapsed ? "▸" : "▾"}</span>
          {section.name}
        </td>
      </tr>
      {!collapsed && children}
    </>
  );
}

/** Preferences > Hotkeys. */
export function useHotkeysPage(open: boolean, shown: boolean): PreferencesPageHandle {
  const panel = useHotkeysPanel(open, false);
  return {
    element: shown ? panel.element : null,
    commit: panel.commit,
    cancel: () => {},
    reset: panel.resetToDefaults,
    resetLabel: "Reset Hotkeys to Defaults",
    resetTooltip: "Reset all hotkeys to the built-in KiCad defaults",
  };
}
