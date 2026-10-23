// The microwave tools' dialogs (GAPS.md item 9, package P19; `pcbnew/microwave/` at KiCad 8303b2ad): the text entry dialog every tool asks its size, angle, length and value with
// (`WX_TEXT_ENTRY_DIALOG( frame, message, "Create Microwave Footprint", value )`) and the Complex Shape dialog of Draw Microwave Polygonal Shapes (`MWAVE_POLYGONAL_SHAPE_DLG`: the Shape
// box, the Size boxes, "Read Shape Description File..."). A tool awaits them (state/microwaveDialogs.ts); this component is mounted once, in App.tsx.
import { useRef, useState } from "react";
import { readShapeFile } from "../api/microwaveClient";
import type { MicrowaveShapeKind } from "../api/microwaveTypes";
import { shapeSizeText } from "../kicad-port/microwave";
import { answerShape, answerText, useMicrowavePrompt, type ShapePrompt, type TextPrompt } from "../state/microwaveDialogs";
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

const SHAPES: Array<{ value: MicrowaveShapeKind; label: string }> = [
  { value: "normal", label: "Normal" },
  { value: "symmetrical", label: "Symmetrical" },
  { value: "mirrored", label: "Mirrored" },
];

/**
 * `MWAVE_POLYGONAL_SHAPE_DLG`: the Shape box (Normal, Symmetrical, Mirrored), the Size boxes (empty until a file is read, then the file's scales) and the button that reads a shape description
 * file (`ReadDataShapeDescr`: a file chooser, then `m_sizeX->SetValue( (int) g_ShapeScaleX )`). The file's text goes to the server, which reads it as the tool will.
 */
function ShapeDialog({ prompt }: { prompt: ShapePrompt }) {
  const [kind, setKind] = useState<MicrowaveShapeKind>("normal");
  const [sizeX, setSizeX] = useState("");
  const [sizeY, setSizeY] = useState("");
  const [shown, setShown] = useState<[string, string]>(["", ""]);
  const [file, setFile] = useState<{ name: string; text: string; points: number } | null>(null);
  const [problem, setProblem] = useState<string | null>(null);
  const chooser = useRef<HTMLInputElement>(null);

  /** `ReadDataShapeDescr`'s work once a file is chosen. */
  const load = async (picked: File | undefined): Promise<void> => {
    if (!picked) return;
    try {
      const text = await picked.text();
      const info = await readShapeFile(text);
      if (!info.ok) {
        setProblem(info.message ?? "The shape description file could not be read.");
        return;
      }
      const x = shapeSizeText(info.size_x_nm ?? 0, prompt.units);
      const y = shapeSizeText(info.size_y_nm ?? 0, prompt.units);
      setSizeX(x);
      setSizeY(y);
      setShown([x, y]);
      setFile({ name: picked.name, text, points: info.points ?? 0 });
      setProblem(null);
    } catch (e) {
      setProblem(e instanceof Error ? e.message : String(e));
    }
  };

  const ok = () => answerShape({ kind, sizeX, sizeY, shownX: shown[0], shownY: shown[1], file: file?.text ?? null });
  return (
    <DialogShell title="Complex Shape" width={460} onCancel={() => answerShape(null)} onOk={ok}>
      <div style={{ display: "flex", gap: 12 }}>
        <fieldset style={{ flex: 1, fontSize: 12 }} aria-label="Shape">
          <legend>Shape</legend>
          {SHAPES.map((s) => (
            <label key={s.value} style={{ display: "block", marginBottom: 4 }}>
              <input type="radio" name="mw-shape-kind" data-testid={`mw-shape-${s.value}`} checked={kind === s.value} onChange={() => setKind(s.value)} /> {s.label}
            </label>
          ))}
        </fieldset>
        <fieldset style={{ flex: 1, fontSize: 12 }} aria-label="Size">
          <legend>Size</legend>
          <label style={{ display: "flex", alignItems: "center", gap: 6, marginBottom: 6 }}>
            X:
            <input data-testid="mw-size-x" aria-label="Size X" style={{ flex: 1, minWidth: 0 }} value={sizeX} onChange={(e) => setSizeX(e.target.value)} />
            {prompt.units}
          </label>
          <label style={{ display: "flex", alignItems: "center", gap: 6 }}>
            Y:
            <input data-testid="mw-size-y" aria-label="Size Y" style={{ flex: 1, minWidth: 0 }} value={sizeY} onChange={(e) => setSizeY(e.target.value)} />
            {prompt.units}
          </label>
        </fieldset>
      </div>
      <div style={{ display: "flex", alignItems: "center", gap: 8, marginTop: 10, fontSize: 12 }}>
        <button type="button" data-testid="mw-read-file" onClick={() => chooser.current?.click()}>
          Read Shape Description File...
        </button>
        <input ref={chooser} type="file" data-testid="mw-shape-file" style={{ display: "none" }} onChange={(e) => {
            const picked = e.target.files?.[0];
            e.target.value = ""; // the same file chosen again is a change too
            void load(picked);
          }}
        />
        <span data-testid="mw-shape-status" style={{ opacity: 0.8 }}>
          {file ? `${file.name}: ${file.points} point${file.points === 1 ? "" : "s"}` : "No file read"}
        </span>
      </div>
      {problem && (
        <div data-testid="mw-shape-problem" style={{ fontSize: 11, color: "var(--danger, #c33)", marginTop: 6 }}>
          {problem}
        </div>
      )}
    </DialogShell>
  );
}

export function MicrowaveDialogs() {
  const prompt = useMicrowavePrompt();
  if (!prompt) return null;
  return prompt.kind === "text" ? <TextEntry key={prompt.id} prompt={prompt} /> : <ShapeDialog key={prompt.id} prompt={prompt} />;
}
