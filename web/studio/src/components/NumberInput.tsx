// A number field (`UNIT_BINDER`, common/widgets/unit_binder.cpp) of a value that may be indeterminate (`INDETERMINATE_STATE`: null, shown empty with "<...>"),
// keeping the text as typed while it is being edited.
import { useRef, useState } from "react";

/** A number field (`UNIT_BINDER`) of a value that may be indeterminate (null, shown empty): `scale` turns the stored value into the shown one, `unscale` back. */
export function NumberInput({ label, value, onChange, scale = (v) => v, unscale = (v) => v, width = 90 }: { label: string; value: number | null; onChange: (v: number | null) => void; scale?: (v: number) => number; unscale?: (v: number) => number; width?: number }) {
  const shownOf = (v: number | null) => (v == null ? "" : String(Number(scale(v).toFixed(6))));
  const [text, setText] = useState(() => shownOf(value));
  const last = useRef(value);
  if (last.current !== value && unscale(Number(text)) !== value) {
    last.current = value;
    setText(shownOf(value));
  }
  return (
    <input
      aria-label={label}
      value={text}
      placeholder={value == null ? "<...>" : undefined}
      onChange={(e) => {
        const t = e.target.value;
        setText(t);
        const n = Number(t);
        const v = t.trim() === "" || !Number.isFinite(n) ? null : unscale(n);
        last.current = v;
        onChange(v);
      }}
      style={{ width }}
    />
  );
}

