// The questions the library table dialogs ask in the middle of their work: `IsOK` (Yes / No), `wxMessageBox( msg, title, wxYES_NO | wxCANCEL )` (Yes / No / Cancel) and a
// plain `wxMessageBox( msg )` (OK). The dialog keeps the question in its state with the promise's resolver; this draws it over the dialog, the last button the
// default and Escape the first (No, or Cancel when there is one).
import { useEffect } from "react";

export interface Question {
  title: string;
  message: string;
  /** The buttons, left to right; the answer is the label pressed. */
  buttons: string[];
  resolve: (answer: string) => void;
}

export function TableQuestion({ q, onDone }: { q: Question; onDone: () => void }) {
  const answer = (label: string) => {
    onDone();
    q.resolve(label);
  };
  const escape = q.buttons.includes("Cancel") ? "Cancel" : (q.buttons[0] ?? "OK");
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.stopPropagation();
        answer(escape);
      }
    };
    document.addEventListener("keydown", onKey, true);
    return () => document.removeEventListener("keydown", onKey, true);
  });
  return (
    <div className="dialog-backdrop" onClick={(e) => e.stopPropagation()}>
      <div className="dialog" style={{ width: 460 }} role="alertdialog" aria-label={q.title} onClick={(e) => e.stopPropagation()}>
        <div className="dialog-header">
          <span>{q.title}</span>
        </div>
        <div className="dialog-body lt-question" style={{ whiteSpace: "pre-wrap" }}>
          {q.message}
        </div>
        <div className="dialog-footer">
          {q.buttons.map((b, i) => (
            <button key={b} className={i === q.buttons.length - 1 ? "primary" : undefined} autoFocus={i === q.buttons.length - 1} onClick={() => answer(b)}>
              {b}
            </button>
          ))}
        </div>
      </div>
    </div>
  );
}
