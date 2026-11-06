// `HTML_MESSAGE_BOX` shown modeless (common/dialogs/html_message_box.cpp at KiCad 8303b2ad, `ShowModeless`): a window of HTML that stays
// beside the dialog it came from, with no OK button (`m_sdbSizer1->Show( false )`), closed by its title bar's close box or Escape. It opens in
// the middle of the screen at `SetDialogSizeInDU`'s size (a dialog unit is a quarter of the average character width across and an eighth of the
// character height down: CSS `ch` and `em` here), can be moved by its title and resized. Typing a character opens the search bar at its top
// right (`OnCharHook`, `ShowSearchBar`): every match in the page's text is marked and the current one scrolled to (`updateSearch`,
// kicad-port/htmlMessageBox.ts), Enter or the down button goes to the next, the up button to the one before, Escape closes the bar (and the
// next Escape the window). A link opens in the browser (`OnHTMLLinkClicked`: KiCad's documentation links get the version). Ctrl+A selects the
// page (`m_htmlWindow->SelectAll()`), Ctrl+C copies the selection.
//
// The page is the HTML KiCad's own converter makes of its own help text (kicad-port/markdownHtml.ts over src/kicad/*.json), never anything a
// user typed, so it is given to the DOM as it is, as wxHtmlWindow is given it.
import { useEffect, useLayoutEffect, useMemo, useRef, useState, type PointerEvent as ReactPointerEvent } from "react";
import { createPortal } from "react-dom";
import { openExternal } from "../actions/commonSuiteActions";
import { KICAD_MAJOR_MINOR_VERSION } from "../kicad-port/appLinks";
import { htmlLinkHref, nextMatch, prevMatch, searchHtml } from "../kicad-port/htmlMessageBox";
import "../styles/htmlMessageBox.css";

export function HtmlMessageBox({
  title,
  html,
  open,
  onClose,
  sizeDu,
  testId,
}: {
  title: string;
  html: string;
  open: boolean;
  onClose: () => void;
  /** `SetDialogSizeInDU( x, y )`. */
  sizeDu: { width: number; height: number };
  testId?: string;
}) {
  const win = useRef<HTMLDivElement>(null);
  const page = useRef<HTMLDivElement>(null);
  const searchBox = useRef<HTMLInputElement>(null);
  const [pos, setPos] = useState<{ left: number; top: number } | null>(null);
  const [searching, setSearching] = useState(false);
  const [term, setTerm] = useState("");
  const [current, setCurrent] = useState(0);
  const found = useMemo(() => searchHtml(html, searching ? term : "", current), [html, searching, term, current]);

  // `Center()` the first time it shows; where it was moved to after that. `reload()` each time it shows: the page from its top.
  useLayoutEffect(() => {
    if (!open) return;
    const el = win.current;
    if (el && !pos) {
      const r = el.getBoundingClientRect();
      setPos({ left: Math.max(8, (window.innerWidth - r.width) / 2), top: Math.max(8, (window.innerHeight - r.height) / 2) });
    }
    if (page.current) page.current.scrollTop = 0;
    el?.focus();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open]);

  // `ScrollToAnchor( "kicad_search_<n>" )`.
  useEffect(() => {
    if (!found.anchor) return;
    page.current?.querySelector(`a[name="${found.anchor}"]`)?.nextElementSibling?.scrollIntoView({ block: "nearest" });
  }, [found]);

  // The window's keys, before a dialog's own Escape (Board Setup's listens on the document in the capture phase).
  useEffect(() => {
    if (!open) return;
    const onKey = (e: KeyboardEvent) => {
      const el = win.current;
      if (!el || !(e.target instanceof Node) || !el.contains(e.target)) return;
      const consume = () => {
        e.preventDefault();
        e.stopPropagation();
      };
      const inSearch = e.target === searchBox.current;
      if (searching && e.key === "Escape") {
        // `HideSearchBar()`, the focus back to the page so the next Escape closes the window
        consume();
        setSearching(false);
        setTerm("");
        el.focus();
        return;
      }
      if (searching && inSearch && e.key === "Enter") {
        consume();
        setCurrent((c) => nextMatch(c, found.matches.length));
        return;
      }
      if (inSearch) return;
      if (e.key === "Escape") {
        consume();
        onClose();
        return;
      }
      const mod = e.ctrlKey || e.metaKey;
      if (mod && !e.altKey && e.key.toLowerCase() === "a") {
        consume();
        const range = document.createRange();
        if (page.current) {
          range.selectNodeContents(page.current);
          const s = window.getSelection();
          s?.removeAllRanges();
          s?.addRange(range);
        }
        return;
      }
      if (!mod && !e.altKey && e.key.length === 1) {
        // `ShowSearchBar()` with the character typed
        consume();
        setSearching(true);
        setTerm(e.key);
        setCurrent(0);
        requestAnimationFrame(() => searchBox.current?.focus());
      }
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [open, searching, found.matches.length, onClose]);

  // Moving the window by its title.
  const drag = (e: ReactPointerEvent<HTMLDivElement>) => {
    if ((e.target as HTMLElement).closest("button")) return;
    const el = win.current;
    if (!el) return;
    const r = el.getBoundingClientRect();
    const dx = e.clientX - r.left;
    const dy = e.clientY - r.top;
    const move = (ev: PointerEvent) => setPos({ left: Math.max(0, Math.min(window.innerWidth - 40, ev.clientX - dx)), top: Math.max(0, Math.min(window.innerHeight - 30, ev.clientY - dy)) });
    const up = () => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", up);
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", up);
  };

  return createPortal(
    <div
      ref={win}
      className="dialog hmb-window"
      role="dialog"
      aria-label={title}
      data-testid={testId}
      tabIndex={-1}
      hidden={!open}
      style={{
        left: pos?.left ?? 0,
        top: pos?.top ?? 0,
        visibility: pos ? undefined : "hidden",
        width: `min(${sizeDu.width / 4}ch, calc(100vw - 16px))`,
        height: `min(${sizeDu.height / 8}em * 1.25, calc(100vh - 16px))`,
      }}
      // The window is modeless: a click in it is its own, not the dialog's behind it.
      onClick={(e) => e.stopPropagation()}
      onPointerDown={(e) => e.stopPropagation()}
    >
      <div className="dialog-header hmb-title" onPointerDown={drag}>
        <span>{title}</span>
        <button className="hmb-close" title="Close" aria-label="Close" onClick={onClose}>
          ×
        </button>
      </div>
      {searching && (
        <div className="hmb-search" data-testid={testId ? `${testId}-search` : undefined}>
          <span className="hmb-count">{found.label}</span>
          <input
            ref={searchBox}
            value={term}
            aria-label="Search"
            onChange={(e) => {
              setTerm(e.target.value);
              setCurrent(0);
            }}
          />
          <button className="hmb-step" onClick={() => setCurrent((c) => prevMatch(c, found.matches.length))}>
            ∧
          </button>
          <button className="hmb-step" onClick={() => setCurrent((c) => nextMatch(c, found.matches.length))}>
            ∨
          </button>
        </div>
      )}
      <div
        ref={page}
        className="hmb-page"
        data-testid={testId ? `${testId}-page` : undefined}
        onClick={(e) => {
          const a = (e.target as HTMLElement).closest("a[href]");
          if (!a) return;
          e.preventDefault();
          openExternal(htmlLinkHref(a.getAttribute("href") ?? "", KICAD_MAJOR_MINOR_VERSION));
        }}
        dangerouslySetInnerHTML={{ __html: found.html }}
      />
    </div>,
    document.body
  );
}
