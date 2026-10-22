// The microwave tools' dialogs (GAPS.md item 9, package P19; `pcbnew/microwave/` at KiCad 8303b2ad): the text entry dialog every tool asks its size, angle, length and value with
// (`WX_TEXT_ENTRY_DIALOG( frame, message, "Create Microwave Footprint", value )`). A tool awaits them (state/microwaveDialogs.ts); this component is mounted once, in App.tsx.
import { useState } from "react";
import { answerText, useMicrowavePrompt, type TextPrompt } from "../state/microwaveDialogs";
import { DialogShell } from "./pcbDialogKit";

/** `WX_TEXT_ENTRY_DIALOG`: a message above a text box that starts selected. Enter is OK. */
function TextEntry({ prompt }: { prompt: TextPrompt }) {
  const [text, setText] = useState(prompt.initial);
  const takes = prompt.accept ? prompt.accept(text) : true;
  const ok = () => {
    if (takes) answerText(text);
  };
  return (
    <DialogShell title={prompt.title} width={380} onCancel={() => answerText(null)} onOk={ok} okDisabled={!takes}>
      <label style={{ display: "flex", flexDirection: "column", gap: 6, fontSize: 12 }}>
        {prompt.label}
        <input
          data-testid="mw-entry"
          autoFocus
          aria-label={prompt.label}
          value={text}
          onChange={(e) => setText(e.target.value)}
          onFocus={(e) => e.currentTarget.select()}
          onKeyDown={(e) => {
            if (e.key === "Enter") ok();
          }}
        />
      </label>
      {!takes && prompt.hint && (
        <div data-testid="mw-entry-hint" style={{ fontSize: 11, color: "var(--danger, #c33)", marginTop: 6 }}>
          {prompt.hint}
        </div>
      )}
    </DialogShell>
  );
}

export function MicrowaveDialogs() {
  const prompt = useMicrowavePrompt();
  if (!prompt) return null;
  return <TextEntry key={prompt.id} prompt={prompt} />;
}
