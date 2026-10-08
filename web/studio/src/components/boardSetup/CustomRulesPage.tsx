// Board Setup > Design Rules > Custom Rules -- port of pcbnew/dialogs/panel_setup_rules.cpp: the text of the board's `.kicad_dru`, edited
// as text. Apply stores it (`set_custom_rules`); the derived project hands it to kicad-cli, which judges the board by it, and this app's
// own DRC reads the same text. "Check rule syntax" asks kicad-cli itself (POST /api/check_rules): it has no syntax checker of its own and
// silently drops a rules file it cannot parse, so the backend loads the text beside a probe board and, when the file does not load, finds
// the first top-level rule that stops it. No rule language is read here.
//
// Not ported: the syntax-highlighting editor with auto-completion, the Syntax Help window, and the extra warnings KiCad's own compiler adds
// (two rules sharing a condition, a rule naming an undefined net class or layer).
import { useEffect, useRef, useState } from "react";
import { postCheckRules } from "../../api/client";
import type { RulesCheckReply } from "../../api/types";
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
  const [report, setReport] = useState<{ reply: RulesCheckReply; checked: string } | null>(null);
  const draft = page.draft;

  const check = async () => {
    if (draft === undefined) return;
    setChecking(true);
    setReport(null);
    try {
      setReport({ reply: await postCheckRules(draft), checked: draft });
    } catch (e) {
      setReport({ reply: { ok: false, message: `The check could not run: ${e instanceof Error ? e.message : String(e)}` }, checked: draft });
    } finally {
      setChecking(false);
    }
  };

  const goTo = (line: number) => {
    const el = area.current;
    if (!el || draft === undefined) return;
    const [a, b] = lineSpan(draft, line);
    el.focus();
    el.setSelectionRange(a, b);
    el.scrollTop = Math.max(0, (line - 3) * 17);
  };

  const bad = report?.reply.bad ?? null;
  const verdict = report?.reply;
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
          <button onClick={() => void check()} disabled={checking || draft === undefined} title="Ask kicad-cli whether it loads these rules">
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
            DRC rules in KiCad's rule language (<code>.kicad_dru</code>). kicad-cli ignores a rules file it cannot parse without saying so, so check the syntax before relying on it.
          </p>
          <textarea
            ref={area}
            className={`bs-textarea${verdict && verdict.ok && verdict.valid === false && !stale ? " bs-invalid" : ""}`}
            aria-label="DRC Rules"
            value={draft}
            spellCheck={false}
            placeholder={'(version 1)\n(rule "name"\n  (condition "...")\n  (constraint clearance (min 0.2mm)))'}
            onChange={(e) => page.edit(() => e.target.value)}
          />
          {(checking || verdict) && (
            <div className="bs-report" role="status">
              {checking && <span>Asking kicad-cli...</span>}
              {verdict && !checking && (
                <>
                  <span className={verdict.ok && verdict.valid ? "bs-ok" : "bs-error"}>
                    {verdict.message}
                    {stale ? " (The text has changed since; check it again.)" : ""}
                  </span>
                  {bad && (
                    <>
                      {" "}
                      <button onClick={() => goTo(bad.line)}>Go to line {bad.line}</button>
                      <pre>{bad.text}</pre>
                    </>
                  )}
                  {verdict.engine && <div className="bs-unit">{verdict.engine}</div>}
                </>
              )}
            </div>
          )}
        </>
      )}
    </PageFrame>
  );
}
