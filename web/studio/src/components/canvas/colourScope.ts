// The colour KiCad's painter gives an item is worked out per item (`PCB_RENDER_SETTINGS::GetColor`): High Contrast Mode mixes an inactive layer's colour into
// the background (`color.Mix( background, m_hiContrastFactor )`) and the net highlight darkens every item that is not on the highlighted net. The studio's
// painters set their colours in many places (`ctx.fillStyle = ...`, `ctx.strokeStyle = ...`, the stroke font's own), so a draw is run in a scope instead: every
// colour it sets on the context goes through the scope's transforms first. A scope keeps the item opaque where its colour is (an inactive item covers what
// lies under it, as in KiCad), which a lowered `globalAlpha` does not.
//
// How: the context gets its own `fillStyle` and `strokeStyle` accessors once (`installColourScopes`), which pass a string through the transforms of the
// scopes open on it and leave a gradient or a pattern alone. Outside a scope they set the colour as it is.

type Transform = (css: string) => string;

/** A scope's transform (`memoColours` keeps what one made: a frame sets the same few colours many times). Null: a scope that sets aside the ones outside it (a
 * pad drawn inside a footprint takes the copper's colour, not the footprint's). */
type Scope = Transform | null;

const scopes = new WeakMap<object, Scope[]>();

/** `fn` with the colours it made kept: the same transform object for every scope of a frame that asks with the same `key`. */
const memos = new Map<string, Transform>();
export function memoColours(key: string, fn: Transform): Transform {
  let t = memos.get(key);
  if (!t) {
    const made = new Map<string, string>();
    t = (css) => {
      let out = made.get(css);
      if (out === undefined) {
        out = fn(css);
        made.set(css, out);
      }
      return out;
    };
    if (memos.size > 64) memos.clear();
    memos.set(key, t);
  }
  return t;
}
const installed = new WeakSet<object>();

function nativeAccessor(ctx: object, prop: "fillStyle" | "strokeStyle"): PropertyDescriptor | undefined {
  for (let p: object | null = Object.getPrototypeOf(ctx); p; p = Object.getPrototypeOf(p)) {
    const d = Object.getOwnPropertyDescriptor(p, prop);
    if (d) return d;
  }
  return undefined;
}

/** The colour the scopes open on `ctx` make of `css` (outermost first, from the last scope that sets aside the ones outside it). */
function transformed(ctx: object, css: string): string {
  const open = scopes.get(ctx);
  if (!open || open.length === 0) return css;
  let from = open.length - 1;
  while (from >= 0 && open[from] !== null) from--;
  let out = css;
  for (let i = from + 1; i < open.length; i++) out = open[i]!(out);
  return out;
}

/** The colours the scopes open on a context have set (as the context reads them back): one of them set again is not transformed a second time. */
const produced = new WeakMap<object, Set<string>>();

/**
 * Gives `ctx` the accessors the scopes need (once per context). A colour read back and set again (`ctx.fillStyle = ctx.strokeStyle`) is already the
 * transformed one and is set as it is.
 */
export function installColourScopes(ctx: CanvasRenderingContext2D): void {
  if (installed.has(ctx)) return;
  const fill = nativeAccessor(ctx, "fillStyle");
  const stroke = nativeAccessor(ctx, "strokeStyle");
  if (!fill?.get || !fill.set || !stroke?.get || !stroke.set) return;
  installed.add(ctx);
  const define = (prop: "fillStyle" | "strokeStyle", own: PropertyDescriptor) =>
    Object.defineProperty(ctx, prop, {
      configurable: true,
      enumerable: true,
      get: () => own.get!.call(ctx),
      set: (v: unknown) => {
        const open = scopes.get(ctx);
        const made = produced.get(ctx);
        if (typeof v !== "string" || !open || open.length === 0 || made?.has(v)) return own.set!.call(ctx, v);
        const out = transformed(ctx, v);
        own.set!.call(ctx, out);
        if (out !== v) made?.add(own.get!.call(ctx) as string);
      },
    });
  define("fillStyle", fill);
  define("strokeStyle", stroke);
}

/** Runs `draw` with every colour it sets on `ctx` passed through `fn` (after the scopes already open); `fn` null sets those aside for the draw. */
export function withColourScope(ctx: CanvasRenderingContext2D, fn: Transform | null, draw: () => void): void {
  let open = scopes.get(ctx);
  if (!open) {
    open = [];
    scopes.set(ctx, open);
  }
  if (!produced.has(ctx)) produced.set(ctx, new Set());
  open.push(fn);
  try {
    draw();
  } finally {
    open.pop();
    if (open.length === 0) produced.get(ctx)?.clear();
  }
}
