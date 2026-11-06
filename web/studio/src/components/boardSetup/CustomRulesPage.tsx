// Board Setup > Design Rules > Custom Rules -- port of pcbnew/dialogs/panel_setup_rules.cpp: the text of the board's `.kicad_dru`, edited
// as text. Apply stores it (`set_custom_rules`); the derived project hands it to kicad-cli, which judges the board by it, and this app's
// own DRC reads the same text. "Check rule syntax" is `OnCompile`: the ported rules parser (`eda_drc::dru::parse_reporting`, POST
// /api/rule_editor/check `{ compile }`, R6-D5) says what KiCad's says -- each error at its line and offset, a link that goes there
// (`OnErrorLinkClicked`), or "No errors found.".
//
// The editor is KiCad's (components/RuleTextEditor.tsx): line numbers, the brace highlight, Ctrl+/, Tab, the right-click menu
// (`OnContextMenu`) and the completion of `onScintillaCharAdded` (kicad-port/ruleCompletion.ts `rulesCharAdded`) over the property names and
// functions KiCad's rules know (src/kicad/rule_completions.json) and the board's net classes and nets. Escape with the text changed asks
// "Cancel Changes?" (`onCharHook`, `IsOK`: Yes / No, asked in the page) and Yes puts back the text the board has. The "Syntax help" link
// (`OnSyntaxHelp`) opens KiCad's help -- the markdown texts pcbnew compiles in (src/kicad/rules_help.json) as its converter renders them
// (kicad-port/rulesSyntaxHelp.ts) -- in a modeless "Syntax Help" window that the link brings back.
//
// Not ported: the extra warnings KiCad's own compiler adds (`checkPlausibility`: two rules sharing a condition, a rule naming an undefined net
// class or layer); Shift+Enter (`onAcceptFn`: the dialog's OK -- the studio's Board Setup has no OK, each page applies on its own).
import { useEffect, useMemo, useRef, useState } from "react";
import { postCompileRules } from "../../api/client";
import { useStudioState } from "../../state/store";
import { isMac } from "../../platform";
import { boardNetNames } from "../../kicad-port/boardSetupRules";
import { rulesCharAdded } from "../../kicad-port/ruleCompletion";
import { SYNTAX_HELP_SIZE_DU, SYNTAX_HELP_TITLE, syntaxHelpHtml } from "../../kicad-port/rulesSyntaxHelp";
import ruleCompletions from "../../kicad/rule_completions.json";
import rulesHelp from "../../kicad/rules_help.json";
import { HtmlMessageBox } from "../HtmlMessageBox";
import { RuleTextEditor, type RuleTextEditorHandle } from "../RuleTextEditor";
import { PageFrame, usePageApply, usePageDraft, type PageProps } from "./fields";

const EXAMPLE = `(version 1)

# A rule applies to the items its condition selects.
(rule "Power nets keep more room"
  (condition "A.NetClass == 'power'")
  (constraint clearance (min 0.5mm)))

(rule "Signal tracks are not thinner than 0.15 mm"
  (condition "A.NetClass == 'signal'")
  (constraint track_width (min 0.15mm) (opt 0.2mm)))
`;

type CompileReply = Awaited<ReturnType<typeof postCompileRules>>;

/** A reporter message cut into its text and its `<a href='line:offset'>..</a>` links (`WX_HTML_REPORT_BOX`'s links). */
function messageParts(text: string): ({ text: string } | { link: string; line: number; offset: number })[] {
  const out: ({ text: string } | { link: string; line: number; offset: number })[] = [];
  const re = /<a href='(\d+):(\d+)'>(.*?)<\/a>/g;
  let last = 0;
  for (let m = re.exec(text); m; m = re.exec(text)) {
    if (m.index > last) out.push({ text: text.slice(last, m.index) });
    out.push({ link: m[3]!, line: Number(m[1]), offset: Number(m[2]) });
    last = m.index + m[0].length;
  }
  if (last < text.length) out.push({ text: text.slice(last) });
  return out;
}

/** `OnSyntaxHelp`'s texts, in its order. */
const HELP_TEXTS = (rulesHelp.setupRules as string[]).map((f) => (rulesHelp.texts as Record<string, string>)[f] ?? "");

export function CustomRulesPage({ hidden, onDirty }: PageProps) {
  const state = useStudioState();
  const board = state.board;
  const text = board?.board_rules?.custom_rules_text;
  const known = board?.board_rules !== undefined && board.board_rules.custom_rules_text !== undefined;
  const page = usePageDraft<string>(known ? (text ?? "") : undefined);
  const apply = usePageApply();
  useEffect(() => onDirty(page.dirty), [page.dirty, onDirty]);
  const editor = useRef<RuleTextEditorHandle>(null);
  const [checking, setChecking] = useState(false);
  const [report, setReport] = useState<{ reply: CompileReply; checked: string } | null>(null);
  const [helpOpen, setHelpOpen] = useState(false);
  const [askCancel, setAskCancel] = useState(false);
  const yes = useRef<HTMLButtonElement>(null);
  const draft = page.draft;
  // `m_originalText`: what the board has.
  const original = text ?? "";

  // What `NetClass == '` and `NetName == '` offer: `GetNetclasses()` (the classes other than Default) and `GetNetClassAssignmentCandidates()`.
  const names = useMemo(
    () => ({ netClasses: (board?.board_rules?.net_classes ?? []).map((c) => c.name).sort(), netNames: board ? boardNetNames(board) : [] }),
    [board]
  );
  const helpHtml = useMemo(() => syntaxHelpHtml(HELP_TEXTS, isMac()), []);

  const check = async () => {
    if (draft === undefined) return;
    setChecking(true);
    setReport(null);
    try {
      setReport({ reply: await postCompileRules(draft), checked: draft });
    } catch (e) {
      setReport({ reply: { ok: false, message: `The check could not run: ${e instanceof Error ? e.message : String(e)}` }, checked: draft });
    } finally {
      setChecking(false);
    }
  };

  // `onCharHook`: Escape with the text not as it was asks first (and is then the page's, not the dialog's).
  const onEscape = () => {
    if (draft === undefined || draft === original) return false;
    setAskCancel(true);
    return true;
  };
  const answer = (cancel: boolean) => {
    setAskCancel(false);
    if (cancel) {
      // `m_textEditor->SetText( m_originalText ); m_textEditor->SelectAll();`
      editor.current?.replaceAll(original);
      page.revert();
      apply.setMessage(null);
      editor.current?.selectAll();
    } else editor.current?.focus();
  };
  useEffect(() => {
    if (!askCancel) return;
    yes.current?.focus(); // `wxOK_DEFAULT`
    // Escape answers No, and must not reach the dialog behind.
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== "Escape") return;
      e.preventDefault();
      e.stopPropagation();
      answer(false);
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [askCancel]);

  const verdict = report?.reply;
  const messages = verdict?.messages ?? [];
  const failed = messages.some((m) => m.severity === "error");
  const stale = report !== null && report.checked !== draft;

  return (
    <PageFrame
      title="Custom Rules"
      hidden={hidden}
      dirty={page.dirty}
      busy={apply.busy}
      message={apply.message?.text ?? null}
      bad={apply.message?.bad}
      onApply={() => draft !== undefined && void apply.run({ op: "set_custom_rules", text: draft }, page.markClean)}
      onRevert={() => (page.revert(), apply.setMessage(null), setReport(null))}
      extraActions={
        <>
          <button onClick={() => void check()} disabled={checking || draft === undefined} title="Check the rules with KiCad's rules parser">
            {checking ? "Checking..." : "Check rule syntax"}
          </button>
          <button onClick={() => page.edit(() => EXAMPLE)} disabled={draft === undefined || (draft ?? "").trim() !== ""} title="Fill an empty editor with two example rules">
            Insert example
          </button>
        </>
      }
    >
      {draft === undefined ? (
        <p className="bs-note">This backend does not report the board's custom rules; update the eda binary.</p>
      ) : (
        <>
          <p className="bs-note">
            DRC rules in KiCad's rule language (<code>.kicad_dru</code>). Check the syntax before relying on them: kicad-cli's DRC stops on a rules file it cannot compile.
          </p>
          {/* `m_title` "DRC Rules" and the `m_syntaxHelp` hyperlink at the right */}
          <div className="bs-rules-head">
            <span>DRC Rules</span>
            <button className="bs-hyperlink" title="Show syntax help window" data-testid="rules-syntax-help" onClick={() => setHelpOpen(true)}>
              Syntax help
            </button>
          </div>
          <RuleTextEditor
            ref={editor}
            className="bs-rules-editor"
            ariaLabel="DRC Rules"
            testId="custom-rules-editor"
            value={draft}
            invalid={failed && !stale}
            lineNumbers
            contextMenu
            placeholder={'(version 1)\n(rule "name"\n  (condition "...")\n  (constraint clearance (min 0.2mm)))'}
            onChange={(t) => page.edit(() => t)}
            onCharAdded={(t, caret) => rulesCharAdded(t, caret, ruleCompletions, names)}
            onEscape={onEscape}
          />
          {askCancel && (
            <div className="bs-ask" role="alertdialog" aria-label="Confirmation" data-testid="rules-cancel-changes">
              <span>Cancel Changes?</span>
              <button ref={yes} onClick={() => answer(true)}>
                Yes
              </button>
              <button onClick={() => answer(false)}>No</button>
            </div>
          )}
          {(checking || verdict) && (
            <div className="bs-report" role="status" data-rules-report>
              {checking && <span>Checking...</span>}
              {verdict && !checking && !verdict.ok && <span className="bs-error">{verdict.message}</span>}
              {verdict && !checking && verdict.ok && (
                <>
                  {messages.map((m, i) => (
                    <div key={i} className={m.severity === "error" ? "bs-error" : m.severity === "warning" ? "bs-warning" : "bs-ok"}>
                      {messageParts(m.text).map((part, j) =>
                        "link" in part ? (
                          <button key={j} className="bs-link" onClick={() => editor.current?.goTo(part.line, part.offset)}>
                            {part.link}
                          </button>
                        ) : (
                          <span key={j}>{part.text}</span>
                        )
                      )}
                    </div>
                  ))}
                  {stale && <div className="bs-unit">The text has changed since; check it again.</div>}
                </>
              )}
            </div>
          )}
          <HtmlMessageBox title={SYNTAX_HELP_TITLE} html={helpHtml} open={helpOpen} onClose={() => setHelpOpen(false)} sizeDu={SYNTAX_HELP_SIZE_DU} testId="rules-syntax-help-window" />
        </>
      )}
    </PageFrame>
  );
}
