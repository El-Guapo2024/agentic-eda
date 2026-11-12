// User-defined Signals' expression editor (KiCad 8303b2ad eeschema/dialogs/dialog_user_defined_signals.cpp): each row's cell is a
// `GRID_CELL_STC_EDITOR( true, true, .. )` -- a single-line Scintilla editor that ignores case -- whose `onScintillaCharAdded` offers the current
// plot's vectors and ngspice's functions through `SCINTILLA_TRICKS::DoAutocomplete` (kicad-port/ruleCompletion.ts `doAutocomplete`, the list
// components/RuleTextEditor.tsx shows), and `OnFormattingHelp`'s "Syntax Help" page (eeschema/sim/user_defined_signals_help.md).
import type { CharAdded } from "../ruleCompletion";
import type { RawPlot } from "./simSignals";

/** `onScintillaCharAdded`'s functions, in its order. */
export const USER_SIGNAL_FUNCTIONS = [
  "sqrt(x)", "sin(x)", "cos(x)", "tan(x)", "sinh(x)", "cosh(x)", "tanh(x)", "asin(x)", "acos(x)", "atan(x)", "asinh(x)", "acosh(x)", "atanh(x)",
  "arctan(x)", "exp(x)", "ln(x)", "log(x)", "abs(x)", "nint(x)", "int(x)", "floor(x)", "ceil(x)", "pow(x,y)", "pwr(x,y)", "min(x,y)", "max(x,y)",
  "sgn(x)", "ternary_fcn(x,y,z)", "gauss(nom,rvar,sigma)", "agauss(nom,avar,sigma)", "unif(nom,rvar)", "aunif(nom,avar)", "limit(nom,avar)",
] as const;

/** `user_defined_signals_help.md`, word for word: what "Syntax help" shows. */
export const USER_SIGNALS_HELP = `SPICE functions:

    sqrt(x)
    sin(x)
    cos(x)
    tan(x)
    sinh(x)
    cosh(x)
    tanh(x)
    asin(x)
    acos(x)
    atan(x)
    asinh(x)
    acosh(x)
    atanh(x)
    arctan(x)
    exp(x)
    ln(x)
    log(x)
    abs(x)
    nint(x)
    int(x)
    floor(x)
    ceil(x)
    pow(x, y)
    pwr(x, y)
    min(x, y)
    max(x, y)
    sgn(x)
    ternary_fcn(x, y, z)
    gauss(nom, rvar, sigma)
    agauss(nom, avar, sigma)
    unif(nom, rvar)
    aunif(nom, avar)
    limit(nom, avar)
`;

/**
 * libngspice's current plot before any run, `const` (ngspice's constants): `ngSpice_AllVecs( ngSpice_CurPlot() )` as KiCad's own libngspice
 * (libngspice-kicad, ngspice 43, on the AWS box, 2026-11-12) lists it, in its order.
 */
export const CONST_PLOT_VECTORS = ["0", "planck", "boltz", "echarge", "kelvin", "i", "c", "e", "pi", "FALSE", "no", "TRUE", "yes"] as const;

/**
 * `SIMULATOR_FRAME_UI::SimPlotVectors` (`NGSPICE::AllVectors` of the current plot): the vectors as libngspice names them. The run's raw file names
 * a node's voltage `v(<node>)` and a source's current `i(<source>)`, where libngspice has `<node>` and `<source>#branch`; the other names (`time`,
 * `frequency`, a device current `i(r1:1)` of `.probe alli`, `user0`) are given as the file has them. No run yet: the `const` plot's constants
 * ([`CONST_PLOT_VECTORS`]), libngspice's current plot then.
 */
export function simPlotVectors(plot: RawPlot | undefined): string[] {
  if (!plot) return [...CONST_PLOT_VECTORS];
  return plot.vectors.map((v) => {
    const m = /^([vi])\((.*)\)$/i.exec(v.name);
    if (!m) return v.name;
    if (m[1]!.toLowerCase() === "v") return m[2]!;
    return /[:@[]/.test(m[2]!) ? v.name : `${m[2]!}#branch`;
  });
}

/**
 * `DIALOG_USER_DEFINED_SIGNALS::onScintillaCharAdded`: the word before the caret -- back over letters, digits and '/' to the first other character
 * (a second '(' stops it after itself), and past one '(' to the letter before it, where the scan stops on that letter ("V(/o" is the word of
 * "2*V(/o"; "x(V" of "max(V") -- completed from the plot's vectors and the functions. `caret` is `GetCurrentPos()`; at 0 nothing is offered
 * (`start` is -1), and a scan that reaches the first character takes the text from there.
 */
export function userSignalCharAdded(text: string, caret: number, vectors: readonly string[]): CharAdded {
  const tokens = [...vectors, ...USER_SIGNAL_FUNCTIONS];
  let start: number;
  let parenCount = 0;
  const isAlpha = (c: string) => /^\p{L}$/u.test(c);
  const isAlnum = (c: string) => /^[\p{L}\p{N}]$/u.test(c);
  for (start = caret - 1; start > 0; start--) {
    const c = text[start] ?? "";
    if (c === "(") {
      if (parenCount) {
        start += 1;
        break;
      }
      parenCount++;
    } else if (isAlpha(c) && parenCount) {
      break;
    } else if (!isAlnum(c) && c !== "/") {
      start += 1;
      break;
    }
  }
  if (start < 0) return { tokens: [], partial: "", cancel: false, insert: null };
  return { tokens, partial: text.slice(start, caret), cancel: false, insert: null };
}
