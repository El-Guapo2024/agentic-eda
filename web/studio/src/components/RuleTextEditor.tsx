// The text editor of KiCad's custom rules: the `wxStyledTextCtrl` (Scintilla) with the `SCINTILLA_TRICKS` (common/scintilla_tricks.cpp at KiCad
// 8303b2ad) that Board Setup > Custom Rules (`PANEL_SETUP_RULES`), the DRC Rule Editor's Custom rule (`DRC_RE_CUSTOM_RULE_PANEL`) and its condition
// rows' Custom Query (`DRC_RE_CONDITION_ROW_PANEL`) edit their text in. The rules editors have no lexer: what Scintilla adds to a plain text box
// there is the brace highlight of `()` (`onScintillaUpdateUI`, kicad-port/scintillaTricks.ts `braceLight`: the brace at the caret and its match
// in KiCad's "brace light" -- the selection's text colour on a 20% wash of the text colour -- or a brace with no match in red), the line numbers
// of Custom Rules (`SetMarginType( 0, wxSTC_MARGIN_NUMBER )`, as wide as "_99999"), a monospaced font with tabs every 4 columns, and the
// completion list (kicad-port/ruleCompletion.ts: the panel's `onScintillaCharAdded` -- given as `onCharAdded` -- and `DoAutocomplete`).
//
// The list, as Scintilla's (`AutoCompSetIgnoreCase( true )`, `AutoCompSetMaxHeight( 20 )`, `AutoCompSetFillUps( ")" )`): it opens under the
// word it completes; Up / Down, Page Up / Page Down, Home / End move in it; Enter takes the item (and selects the last `<...>` in it, as
// SCINTILLA_TRICKS does), Tab and a click twice take it, `)` takes it and is typed after it; Escape closes it and keeps it closed until a key
// that is not a letter (`m_suppressAutocomplete`); Ctrl+Space opens it where the caret is; any other caret movement closes it, and so does
// Backspace past where it opened (`cancelAtStartPos`), after which the panel offers again for what is left. Every character typed (and every
// Backspace while it is open) asks the panel again.
//
// The keys of `SCINTILLA_TRICKS::onCharHook` a browser text box does not do itself: Ctrl+/ (Cmd+/ on a Mac) comments or uncomments the lines,
// Tab indents (with or without Shift: KiCad calls `Tab()` for both), smart quotes and dashes typed or pasted become plain ones, Shift+Enter is
// the panel's `onAcceptFn` when it has one (`onAccept`). Undo, Redo, Cut, Copy, Paste, Select All and the word deletions are the text box's own.
// The right-click menu is `PANEL_SETUP_RULES::OnContextMenu` (`contextMenu`): Undo, Redo, Cut, Copy, Paste, Delete, Select All, Zoom In,
// Zoom Out; the zoom (Scintilla's points added to the font, -10 to +20) is this browser's (localStorage), as KiCad keeps one
// `text_editor_zoom` for all its text editors.
//
// How: a textarea whose text is transparent lies over a copy of the text drawn with those highlights (same font, same metrics, no wrapping),
// in one box that scrolls both. Edits the editor makes itself go through `document.execCommand( "insertText" )` so the browser's undo has
// them. Escape is taken in the window's capture phase, before a dialog's own Escape (Board Setup's closes the dialog): with the list open it
// only closes the list; otherwise `onEscape` (onCharHook's "Cancel Changes?") may take it, or it goes on to the dialog.
//
// Not ported: the indentation guides (`SetIndentationGuides( true )` and the highlighted guide of the matched brace), Ctrl+wheel zoom,
// Backspace un-indenting, Ctrl+Tab moving to the next control (the browser keeps Ctrl+Tab).
import { forwardRef, useCallback, useEffect, useImperativeHandle, useLayoutEffect, useMemo, useRef, useState, type ChangeEvent, type KeyboardEvent as ReactKeyboardEvent, type ReactNode } from "react";
import { createPortal } from "react-dom";
import { ContextMenu, type MenuEntry } from "./canvas/ContextMenu";
import { isMac } from "../platform";
import { autoCompSelect, doAutocomplete, substitutionSelection, type AutoCompList, type CharAdded } from "../kicad-port/ruleCompletion";
import { braceLight, changedRange, convertSmartQuotesAndDashes, tabIndent, toggleComment } from "../kicad-port/scintillaTricks";
import "../styles/ruleTextEditor.css";

export interface RuleTextEditorHandle {
  focus(): void;
  /** `OnErrorLinkClicked`: `GotoPos( PositionFromLine( line - 1 ) + ( offset - 1 ) )` and the focus. */
  goTo(line: number, offset: number): void;
  /** The whole text replaced by `text` as one edit the browser can undo (`SetText`), then all of it selected (`SelectAll`). */
  replaceAll(text: string): void;
  selectAll(): void;
}

export interface RuleTextEditorProps {
  value: string;
  onChange: (text: string) => void;
  /** The panel's `onScintillaCharAdded`: what to offer at the caret. None: the editor completes nothing (the condition rows' editors). */
  onCharAdded?: (text: string, caret: number) => CharAdded;
  /** The line number margin (Custom Rules has one). */
  lineNumbers?: boolean;
  /** `onCharHook`'s Escape when no list is open: true when the panel took the key (else it goes on to the dialog). */
  onEscape?: () => boolean;
  /** Shift+Enter: the panel's `onAcceptFn` (none: a new line, as the DRC Rule Editor's panels do). */
  onAccept?: () => void;
  /** `OnContextMenu`: KiCad's right-click menu (else the browser's). */
  contextMenu?: boolean;
  ariaLabel?: string;
  testId?: string;
  className?: string;
  placeholder?: string;
  /** The text failed its check: drawn with the error border. */
  invalid?: boolean;
}

/** An open list: Scintilla's state after `AutoCompShow`, with `posStart` the caret when it opened (`AutoCompPosStart()`). */
interface OpenList extends AutoCompList {
  posStart: number;
}

/** Scintilla's zoom range (`SCI_ZOOMIN` stops at +20 points, `SCI_ZOOMOUT` at -10). */
const ZOOM_MIN = -10;
const ZOOM_MAX = 20;
const ZOOM_KEY = "eda.textEditorZoom";
/** `AutoCompSetMaxHeight( 20 )`: the rows the list shows before it scrolls. */
const LIST_ROWS = 20;

function readZoom(): number {
  try {
    const v = Number(window.localStorage.getItem(ZOOM_KEY));
    return Number.isFinite(v) ? Math.max(ZOOM_MIN, Math.min(ZOOM_MAX, Math.round(v))) : 0;
  } catch {
    return 0;
  }
}

function writeZoom(z: number): void {
  try {
    window.localStorage.setItem(ZOOM_KEY, String(z));
  } catch {
    /* a browser without storage keeps it for the page */
  }
}

/** Where the caret is: the end of the selection Scintilla moves (`GetCurrentPos`). */
const caretOf = (el: HTMLTextAreaElement): number => (el.selectionDirection === "backward" ? el.selectionStart : el.selectionEnd);
const anchorOf = (el: HTMLTextAreaElement): number => (el.selectionDirection === "backward" ? el.selectionEnd : el.selectionStart);

/** `PositionFromLine( line )` (from 0); the end of the text past the last line. */
function positionFromLine(text: string, line: number): number {
  let pos = 0;
  for (let n = 0; n < line; n++) {
    const next = text.indexOf("\n", pos);
    if (next < 0) return text.length;
    pos = next + 1;
  }
  return pos;
}

export const RuleTextEditor = forwardRef<RuleTextEditorHandle, RuleTextEditorProps>(function RuleTextEditor(props, handle) {
  const { value, onChange, onCharAdded, lineNumbers, onAccept, contextMenu, ariaLabel, testId, className, placeholder, invalid } = props;
  const area = useRef<HTMLTextAreaElement>(null);
  const scroller = useRef<HTMLDivElement>(null);
  const overlay = useRef<HTMLPreElement>(null);
  const listEl = useRef<HTMLDivElement>(null);
  const [sel, setSel] = useState({ anchor: 0, caret: 0, focused: false });
  const [list, setListState] = useState<OpenList | null>(null);
  const [listAt, setListAt] = useState<{ left: number; top: number; above: boolean } | null>(null);
  const [menu, setMenuState] = useState<{ x: number; y: number } | null>(null);
  const [zoom, setZoom] = useState(readZoom);
  const [, setScrolled] = useState(0);
  // The values the window's key listener and the native input listeners read (they outlive a render).
  const listRef = useRef<OpenList | null>(null);
  const menuRef = useRef<{ x: number; y: number } | null>(null);
  const suppress = useRef(false); // `m_suppressAutocomplete`
  const programmatic = useRef(false); // an edit the editor makes: not a character typed
  const reveal = useRef(false); // bring the caret into view after the next render
  const props_ = useRef(props);
  props_.current = props;

  const setList = useCallback((l: OpenList | null) => {
    listRef.current = l;
    setListState(l);
  }, []);
  const setMenu = useCallback((m: { x: number; y: number } | null) => {
    menuRef.current = m;
    setMenuState(m);
  }, []);

  const readSel = useCallback(() => {
    const el = area.current;
    if (!el) return;
    const next = { anchor: anchorOf(el), caret: caretOf(el), focused: document.activeElement === el };
    setSel((s) => (s.anchor === next.anchor && s.caret === next.caret && s.focused === next.focused ? s : next));
  }, []);

  /**
   * Replaces `[from, to)` with `insert` the way typing would (so the browser's undo has it), then selects `[anchor, caret)`. The change
   * reaches `onChange` through the input event; it is not taken for a typed character.
   */
  const edit = useCallback(
    (from: number, to: number, insert: string, anchor: number, caret: number) => {
      const el = area.current;
      if (!el) return;
      el.focus();
      el.setSelectionRange(from, to);
      programmatic.current = true;
      let done = false;
      try {
        done = insert === "" ? (from === to ? true : document.execCommand("delete")) : document.execCommand("insertText", false, insert);
      } catch {
        done = false;
      }
      if (!done) {
        el.setRangeText(insert, from, to, "end");
        props_.current.onChange(el.value);
      }
      programmatic.current = false;
      el.setSelectionRange(Math.min(anchor, caret), Math.max(anchor, caret), caret < anchor ? "backward" : "forward");
      reveal.current = true;
      readSel();
    },
    [readSel]
  );

  /** A whole-text change (Tab, Ctrl+/) sent as the one range it changed. */
  const applyEdit = useCallback(
    (before: string, after: { text: string; anchor: number; caret: number }) => {
      const r = changedRange(before, after.text);
      edit(r.from, r.to, r.insert, after.anchor, after.caret);
    },
    [edit]
  );

  /** `DoAutocomplete` for what the panel said, after what Scintilla did on its own (`list` as it stands then). */
  const offer = useCallback(
    (text: string, caret: number, current: OpenList | null) => {
      const ask = props_.current.onCharAdded;
      if (!ask) return current;
      const r = ask(text, caret);
      let next = r.cancel ? null : current; // `AutoCompCancel()` while the scan opened an s-expression
      if (r.insert !== null) {
        // `AddText( "\"" )`: no character-added event of its own
        edit(caret, caret, r.insert, caret + r.insert.length, caret + r.insert.length);
        return next;
      }
      if (r.tokens.length && !suppress.current) {
        const shown = doAutocomplete(r.partial, r.tokens);
        if (shown.show) next = shown.list ? { ...shown.list, posStart: caret } : null;
      }
      return next;
    },
    [edit]
  );

  /** `AutoCompleteMoveToCurrentWord`: the list selects the item the word typed so far begins, or hides when none does. */
  const moveToCurrentWord = (l: OpenList, text: string, caret: number): OpenList | null => {
    const word = text.slice(l.posStart - l.lenEntered, caret);
    const selected = autoCompSelect(l.items, word);
    return selected < 0 ? null : { ...l, selected };
  };

  /** A character was typed (`wxEVT_STC_CHARADDED`): Scintilla's list follows the word, then the panel offers. */
  const charAdded = useCallback(
    (text: string, caret: number) => {
      const l = listRef.current;
      const scintilla = l ? moveToCurrentWord(l, text, caret) : null;
      setList(offer(text, caret, scintilla));
    },
    [offer, setList]
  );

  /** Backspace with the list open (`AutoCompleteCharacterDeleted`, then `wxEVT_STC_AUTOCOMP_CHAR_DELETED` to the panel). */
  const charDeleted = useCallback(
    (text: string, caret: number) => {
      const l = listRef.current;
      if (!l) return;
      const scintilla = caret < l.posStart - l.lenEntered || caret <= l.posStart ? null : moveToCurrentWord(l, text, caret);
      setList(offer(text, caret, scintilla));
    },
    [offer, setList]
  );

  /** `AutoCompleteCompleted`: the word typed so far (from where the list opened, back `lenEntered`) replaced by the selected item. */
  const accept = useCallback(
    (how: "enter" | "other") => {
      const el = area.current;
      const l = listRef.current;
      if (!el || !l) return;
      setList(null);
      const item = l.items[l.selected];
      if (item === undefined) return;
      const firstPos = l.posStart - l.lenEntered;
      const endPos = caretOf(el);
      if (endPos < firstPos) return;
      const finish = firstPos + item.length;
      edit(firstPos, endPos, item, finish, finish);
      if (how === "enter") {
        const s = substitutionSelection(el.value, l.posStart, finish);
        if (s) {
          el.setSelectionRange(s[0], s[1]);
          readSel();
        }
      }
    },
    [edit, readSel, setList]
  );

  useImperativeHandle(
    handle,
    () => ({
      focus: () => area.current?.focus(),
      goTo: (line, offset) => {
        const el = area.current;
        if (!el) return;
        const pos = Math.max(0, Math.min(el.value.length, positionFromLine(el.value, line - 1) + (offset - 1)));
        el.focus();
        el.setSelectionRange(pos, pos);
        reveal.current = true;
        readSel();
      },
      replaceAll: (text) => {
        const el = area.current;
        if (!el) return;
        setList(null);
        edit(0, el.value.length, text, 0, text.length);
      },
      selectAll: () => {
        area.current?.select();
        readSel();
      },
    }),
    [edit, readSel, setList]
  );

  // Escape, before any dialog's own (Board Setup's listens on the document in the capture phase).
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== "Escape") return;
      const consume = () => {
        e.preventDefault();
        e.stopPropagation();
      };
      if (menuRef.current) {
        setMenu(null);
        consume();
        return;
      }
      if (e.target !== area.current) return;
      if (listRef.current) {
        // SCINTILLA_TRICKS: `AutoCompCancel()`, and no list again on the next character
        setList(null);
        suppress.current = true;
        consume();
        return;
      }
      if (props_.current.onEscape?.()) consume();
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [setList, setMenu]);

  // Typed text the editor rewrites before it lands: a smart quote or dash (`ConvertSmartQuotesAndDashes`), and `)` with the list open
  // (the fill-up character takes the item first, then is typed).
  useEffect(() => {
    const el = area.current;
    if (!el) return;
    const onBeforeInput = (e: InputEvent) => {
      if (programmatic.current || e.isComposing) return;
      if (e.inputType !== "insertText" || !e.data) return;
      if (listRef.current && e.data === ")") {
        e.preventDefault();
        accept("other");
        const at = caretOf(el);
        edit(at, el.selectionEnd, ")", at + 1, at + 1);
        charAdded(el.value, at + 1);
        return;
      }
      const plain = convertSmartQuotesAndDashes(e.data);
      if (plain !== e.data) {
        e.preventDefault();
        const from = el.selectionStart;
        edit(from, el.selectionEnd, plain, from + plain.length, from + plain.length);
      }
    };
    const onPaste = (e: ClipboardEvent) => {
      setList(null);
      const text = e.clipboardData?.getData("text/plain");
      if (text === undefined) return;
      const plain = convertSmartQuotesAndDashes(text);
      if (plain === text) return;
      e.preventDefault();
      const from = el.selectionStart;
      edit(from, el.selectionEnd, plain, from + plain.length, from + plain.length);
    };
    el.addEventListener("beforeinput", onBeforeInput);
    el.addEventListener("paste", onPaste);
    return () => {
      el.removeEventListener("beforeinput", onBeforeInput);
      el.removeEventListener("paste", onPaste);
    };
  }, [accept, charAdded, edit, setList]);

  const onInput = (e: ChangeEvent<HTMLTextAreaElement>) => {
    const el = e.target;
    onChange(el.value);
    reveal.current = true;
    readSel();
    if (programmatic.current) return;
    const ne = e.nativeEvent as InputEvent;
    const caret = caretOf(el);
    if (ne.isComposing || ne.inputType === "insertCompositionText") return;
    if ((ne.inputType === "insertText" && ne.data) || ne.inputType === "insertLineBreak") charAdded(el.value, caret);
    else if (ne.inputType === "deleteContentBackward" && listRef.current) charDeleted(el.value, caret);
    else if (listRef.current) setList(null);
  };

  const onKeyDown = (e: ReactKeyboardEvent<HTMLTextAreaElement>) => {
    const el = e.currentTarget;
    const l = listRef.current;
    if (!/^[a-zA-Z]$/.test(e.key)) suppress.current = false; // `if( !isalpha( aEvent.GetKeyCode() ) )`
    const plain = !e.ctrlKey && !e.metaKey && !e.altKey;
    if (l) {
      const move = (to: number) => {
        e.preventDefault();
        setList({ ...l, selected: Math.max(0, Math.min(l.items.length - 1, to)) });
      };
      const rows = Math.min(LIST_ROWS, l.items.length);
      switch (e.key) {
        case "ArrowDown":
          return move(l.selected + 1);
        case "ArrowUp":
          return move(l.selected - 1);
        case "PageDown":
          return move(l.selected + rows);
        case "PageUp":
          return move(l.selected - rows);
        case "Home":
          return move(0);
        case "End":
          return move(l.items.length - 1);
        case "Enter":
          e.preventDefault();
          return accept("enter");
        case "Tab":
          e.preventDefault();
          return accept("other");
        case "Backspace":
        case "Shift":
        case "Control":
        case "Alt":
        case "Meta":
        case "CapsLock":
          return;
        default:
          // A character goes on into the text; any other key cancels the list (Scintilla's `KeyCommand`).
          if (e.key.length !== 1 || !plain) setList(null);
      }
    }
    if (e.key === " " && e.ctrlKey && !e.metaKey && !e.altKey && !e.shiftKey) {
      // Ctrl+Space: `m_suppressAutocomplete = false` and the panel asked with no character added
      e.preventDefault();
      suppress.current = false;
      setList(offer(el.value, caretOf(el), listRef.current));
      return;
    }
    if (e.key === "/" && (isMac() ? e.metaKey && !e.ctrlKey : e.ctrlKey && !e.metaKey)) {
      e.preventDefault();
      applyEdit(el.value, toggleComment(el.value, el.selectionStart, el.selectionEnd));
      return;
    }
    if (e.key === "Tab" && !e.ctrlKey && !e.metaKey && !e.altKey) {
      e.preventDefault();
      applyEdit(el.value, tabIndent(el.value, anchorOf(el), caretOf(el)));
      return;
    }
    if (e.key === "Enter" && e.shiftKey && onAccept) {
      e.preventDefault();
      onAccept();
    }
  };

  // ------------------------------------------------------------------------------------------------------------ the highlights and the list

  const light = useMemo(() => (sel.focused || list ? braceLight(value, sel.caret) : null), [value, sel.caret, sel.focused, list]);
  const wordStart = list ? list.posStart - list.lenEntered : -1;

  /** The text as the overlay draws it: the lit braces in their spans, markers where the caret and the list's word are. */
  const drawn = useMemo(() => {
    const marks = new Map<number, ReactNode[]>();
    const add = (at: number, node: ReactNode) => marks.set(at, [...(marks.get(at) ?? []), node]);
    if (wordStart >= 0) add(wordStart, <span key="w" data-mark="word" />);
    add(sel.caret, <span key="c" data-mark="caret" />);
    const lit = new Map<number, "match" | "bad">();
    if (light?.kind === "match") {
      lit.set(light.at[0], "match");
      lit.set(light.at[1], "match");
    } else if (light?.kind === "bad") lit.set(light.at, "bad");
    const out: ReactNode[] = [];
    const cuts = [...new Set([0, value.length, ...marks.keys(), ...[...lit.keys()].flatMap((p) => [p, p + 1])])].filter((p) => p >= 0 && p <= value.length).sort((a, b) => a - b);
    for (let i = 0; i < cuts.length; i++) {
      const p = cuts[i]!;
      for (const m of marks.get(p) ?? []) out.push(m);
      const next = cuts[i + 1];
      if (next === undefined) break;
      const piece = value.slice(p, next);
      const kind = lit.get(p);
      out.push(kind ? <span key={`b${p}`} className={kind === "match" ? "rte-brace" : "rte-brace-bad"} data-brace={kind}>{piece}</span> : piece);
    }
    // A last empty line has a height only with something on it.
    if (value === "" || value.endsWith("\n")) out.push(" ");
    return out;
  }, [value, sel.caret, light, wordStart]);

  const lines = useMemo(() => {
    let n = 1;
    for (let i = 0; i < value.length; i++) if (value[i] === "\n") n++;
    return Array.from({ length: n }, (_, i) => String(i + 1)).join("\n");
  }, [value]);

  // After each render: the caret into view when it moved by typing or by the editor, and the list under its word.
  useLayoutEffect(() => {
    const box = scroller.current;
    const pre = overlay.current;
    if (!box || !pre) return;
    if (reveal.current) {
      reveal.current = false;
      const caret = pre.querySelector<HTMLElement>('[data-mark="caret"]');
      if (caret) {
        const b = box.getBoundingClientRect();
        const c = caret.getBoundingClientRect();
        const lineH = parseFloat(getComputedStyle(pre).lineHeight) || 16;
        const gutter = lineNumbers ? (box.querySelector(".rte-gutter")?.getBoundingClientRect().width ?? 0) : 0;
        if (c.top < b.top) box.scrollTop -= b.top - c.top + 2;
        else if (c.top + lineH > b.bottom - 14) box.scrollTop += c.top + lineH - (b.bottom - 14);
        if (c.left < b.left + gutter) box.scrollLeft -= b.left + gutter - c.left + 24;
        else if (c.left > b.right - 20) box.scrollLeft += c.left - (b.right - 20) + 24;
      }
    }
    if (!list) {
      if (listAt) setListAt(null);
      return;
    }
    const word = pre.querySelector<HTMLElement>('[data-mark="word"]');
    if (!word) return;
    const r = word.getBoundingClientRect();
    const lineH = parseFloat(getComputedStyle(pre).lineHeight) || 16;
    const below = r.top + lineH;
    const height = listEl.current?.getBoundingClientRect().height ?? 0;
    const above = below + height > window.innerHeight - 4 && r.top - height > 4;
    const at = { left: Math.max(4, r.left - 3), top: above ? r.top - height : below, above };
    if (!listAt || listAt.left !== at.left || listAt.top !== at.top || listAt.above !== at.above) setListAt(at);
  });

  // The list keeps its selected row in view.
  useEffect(() => {
    if (!list) return;
    listEl.current?.querySelector<HTMLElement>('[aria-selected="true"]')?.scrollIntoView({ block: "nearest" });
  }, [list]);

  const setZoomTo = (z: number) => {
    const v = Math.max(ZOOM_MIN, Math.min(ZOOM_MAX, z));
    setZoom(v);
    writeZoom(v);
  };

  const menuEntries = (): MenuEntry[] => {
    const el = area.current;
    const run = (fn: () => void) => () => {
      el?.focus();
      fn();
      readSel();
    };
    const exec = (cmd: string) => run(() => document.execCommand(cmd));
    return [
      { label: "Undo", onSelect: exec("undo") },
      { label: "Redo", onSelect: exec("redo") },
      { label: "", onSelect: () => {}, separator: true },
      { label: "Cut", onSelect: exec("cut") },
      { label: "Copy", onSelect: exec("copy") },
      {
        label: "Paste",
        onSelect: run(() => {
          const target = area.current;
          if (!target) return;
          const from = target.selectionStart;
          const to = target.selectionEnd;
          void navigator.clipboard?.readText().then(
            (text) => {
              const plain = convertSmartQuotesAndDashes(text);
              edit(from, to, plain, from + plain.length, from + plain.length);
            },
            () => {
              /* the browser did not let the page read the clipboard: Ctrl+V still pastes */
            }
          );
        }),
      },
      {
        label: "Delete",
        onSelect: run(() => {
          const target = area.current;
          if (target && target.selectionEnd > target.selectionStart) edit(target.selectionStart, target.selectionEnd, "", target.selectionStart, target.selectionStart);
        }),
      },
      { label: "", onSelect: () => {}, separator: true },
      { label: "Select All", onSelect: run(() => area.current?.select()) },
      { label: "", onSelect: () => {}, separator: true },
      { label: "Zoom In", onSelect: run(() => setZoomTo(zoom + 1)) },
      { label: "Zoom Out", onSelect: run(() => setZoomTo(zoom - 1)) },
    ];
  };

  // Scintilla's zoom adds points to every style's size (a point is 4/3 of a CSS pixel); never below 2 points.
  const fontSize = Math.max(8 / 3, 12 + (zoom * 4) / 3);

  return (
    <div
      ref={scroller}
      className={`rte${invalid ? " rte-invalid" : ""}${className ? ` ${className}` : ""}`}
      style={{ fontSize }}
      data-testid={testId ? `${testId}-box` : undefined}
      // The list follows its word when the text scrolls under it (the layout effect places it again).
      onScroll={() => list && setScrolled((n) => n + 1)}
    >
      <div className={`rte-grid${lineNumbers ? " rte-with-gutter" : ""}`}>
        {lineNumbers && (
          <pre className="rte-gutter rte-metrics" aria-hidden="true">
            {lines}
          </pre>
        )}
        <pre ref={overlay} className="rte-overlay rte-metrics" aria-hidden="true">
          {drawn}
        </pre>
        <textarea
          ref={area}
          className="rte-input rte-metrics"
          aria-label={ariaLabel}
          data-testid={testId}
          value={value}
          placeholder={placeholder}
          spellCheck={false}
          autoCapitalize="off"
          autoComplete="off"
          autoCorrect="off"
          wrap="off"
          rows={1}
          cols={1}
          aria-autocomplete={onCharAdded ? "list" : undefined}
          onChange={onInput}
          onKeyDown={onKeyDown}
          onSelect={readSel}
          onKeyUp={readSel}
          onMouseUp={readSel}
          onFocus={readSel}
          onBlur={() => {
            setList(null);
            readSel();
          }}
          onMouseDown={() => list && setList(null)}
          onContextMenu={
            contextMenu
              ? (e) => {
                  e.preventDefault();
                  setList(null);
                  setMenu({ x: e.clientX, y: e.clientY });
                }
              : undefined
          }
        />
      </div>
      {list &&
        createPortal(
          <div
            ref={listEl}
            className="rte-list rte-metrics"
            role="listbox"
            data-testid={testId ? `${testId}-completions` : "rte-completions"}
            style={{ fontSize, left: listAt?.left ?? -9999, top: listAt?.top ?? -9999, maxHeight: `calc(${LIST_ROWS} * 1.4em + 4px)` }}
            // The list never takes the focus from the text (Scintilla's is a popup the editor keeps the keys of).
            onMouseDown={(e) => e.preventDefault()}
          >
            {list.items.map((item, i) => (
              <div
                key={`${i}:${item}`}
                role="option"
                aria-selected={i === list.selected}
                className={`rte-item${i === list.selected ? " selected" : ""}`}
                onClick={() => setList({ ...list, selected: i })}
                onDoubleClick={() => accept("other")}
              >
                {item}
              </div>
            ))}
          </div>,
          document.body
        )}
      {menu && createPortal(<ContextMenu x={menu.x} y={menu.y} entries={menuEntries()} onClose={() => setMenu(null)} />, document.body)}
    </div>
  );
});
