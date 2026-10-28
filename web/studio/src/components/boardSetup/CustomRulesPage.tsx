// Board Setup > Design Rules > Custom Rules -- port of pcbnew/dialogs/panel_setup_rules.cpp: the text of the board's `.kicad_dru`, edited
// as text. Apply stores it (`set_custom_rules`); the derived project hands it to kicad-cli, which judges the board by it, and this app's
// own DRC reads the same text. "Check rule syntax" is `OnCompile`: the ported rules parser (`eda_drc::dru::parse_reporting`, POST
// /api/rule_editor/check `{ compile }`, R6-D5) says what KiCad's says -- each error at its line and offset, a link that goes there
// (`OnErrorLinkClicked`), or "No errors found.".
//
// Not ported: the syntax-highlighting editor with auto-completion, the Syntax Help window, and the extra warnings KiCad's own compiler adds
// (`checkPlausibility`: two rules sharing a condition, a rule naming an undefined net class or layer).
import { useEffect, useRef, useState } from "react";
import { postCompileRules } from "../../api/client";
import { useStudioState } from "../../state/store";
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

/** The offsets, in `text`, of line `line` (from 1): where it starts and where it ends. */
function lineSpan(text: string, line: number): [number, number] {
  let start = 0;
  for (let n = 1; n < line; n++) {
    const next = text.indexOf("\n", start);
    if (next < 0) return [text.length, text.length];
    start = next + 1;
  }
  const end = text.indexOf("\n", start);
  return [start, end < 0 ? text.length : end];
}

export function CustomRulesPage({ hidden, onDirty }: PageProps) {
  const state = useStudioState();
  const text = state.board?.board_rules?.custom_rules_text;
  const known = state.board?.board_rules !== undefined && state.board.board_rules.custom_rules_text !== undefined;
  const page = usePageDraft<string>(known ? (text ?? "") : undefined);
  const apply = usePageApply();
  useEffect(() => onDirty(page.dirty), [page.dirty, onDirty]);
  const area = useRef<HTMLTextAreaElement>(null);
  const [checking, setChecking] = useState(false);
  const [report, setReport] = useState<{ reply: CompileReply; checked: string } | null>(null);
  const draft = page.draft;

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

  // `OnErrorLinkClicked`: the caret to the line and offset the link names
  const goTo = (line: number, offset: number) => {
    const el = area.current;
    if (!el || draft === undefined) return;
    const [a, b] = lineSpan(draft, line);
    const at = Math.min(b, a + Math.max(0, offset - 1));
    el.focus();
    el.setSelectionRange(at, at);
    el.scrollTop = Math.max(0, (line - 3) * 17);
  };

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
          <textarea
            ref={area}
            className={`bs-textarea${failed && !stale ? " bs-invalid" : ""}`}
            aria-label="DRC Rules"
            value={draft}
            spellCheck={false}
            placeholder={'(version 1)\n(rule "name"\n  (condition "...")\n  (constraint clearance (min 0.2mm)))'}
            onChange={(e) => page.edit(() => e.target.value)}
          />
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
                          <button key={j} className="bs-link" onClick={() => goTo(part.line, part.offset)}>
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
        </>
      )}
    </PageFrame>
  );
}
