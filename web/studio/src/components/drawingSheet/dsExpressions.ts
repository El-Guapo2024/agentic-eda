// The `@{..}` of a drawing sheet's texts (KiCad 25ec2335 `DS_DRAW_ITEM_LIST::BuildFullText`: `EXPRESSION_EVALUATOR().Evaluate( retv )` after the variables), evaluated
// by the server's evaluator -- the one every other shown text goes through (`eda_model::text_vars::evaluate_math`: the grammar, the functions, the clock's and the
// version control's) -- through `POST /api/text/eval`. A canvas draws synchronously: a text not answered yet is drawn as it is written, and the canvases repaint when
// the answer comes (`noteAssetArrived`). The answers are kept per text; one with the clock's functions (`today`, `now`, `random` ..) is asked again after a minute.
import { setDsExpressionEvaluator } from "../../kicad-port/dsDataModel";
import { noteAssetArrived } from "../canvas/imageCache";
import { localClock } from "../../kicad-port/textVariablesRules";

const answers = new Map<string, { text: string; at: number }>();
const asked = new Set<string>();
let pending: string[] = [];
let timer: ReturnType<typeof setTimeout> | null = null;

/** A text whose value changes with the clock (`today`, `now`, `random`, `timeformat( now() )` ..). */
const CLOCK = /\b(today|now|random)\s*\(/;

async function flush(): Promise<void> {
  timer = null;
  const texts = pending;
  pending = [];
  try {
    const r = await fetch(`/api/text/eval?clock=${encodeURIComponent(localClock())}`, { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ texts }) });
    const j = (await r.json()) as { texts?: string[] };
    texts.forEach((t, i) => answers.set(t, { text: j.texts?.[i] ?? t, at: Date.now() }));
  } catch {
    texts.forEach((t) => answers.set(t, { text: t, at: Date.now() }));
  }
  texts.forEach((t) => asked.delete(t));
  noteAssetArrived();
}

/** The text with its `@{..}` evaluated, once the server has answered; as written until then. */
export function evaluateDsText(text: string): string {
  const hit = answers.get(text);
  const stale = hit && CLOCK.test(text) && Date.now() - hit.at > 60_000;
  if ((!hit || stale) && !asked.has(text) && typeof fetch !== "undefined") {
    asked.add(text);
    pending.push(text);
    timer ??= setTimeout(() => void flush(), 0);
  }
  return hit ? hit.text : text;
}

setDsExpressionEvaluator(evaluateDsText);
