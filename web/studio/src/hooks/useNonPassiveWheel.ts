// A wheel listener that may call `preventDefault()`.
//
// React attaches `onWheel` (like `onTouchStart`/`onTouchMove`) as a PASSIVE listener at the root, so a `preventDefault()` inside it is ignored and the
// browser logs "Unable to preventDefault inside passive event listener invocation" on every tick, with the page free to scroll under the wheel zoom.
// The fix is the one the browser asks for: attach the listener to the canvas itself with `{ passive: false }`.
//
// `handler` is read through a ref, so the listener is attached once per element and still sees the latest props and state of the render that
// installed it; a handler that returns early without calling `preventDefault()` (as the schematic canvas does before its first fit) leaves the
// event to the browser as usual.
import { useEffect, useRef, type RefObject } from "react";

export function useNonPassiveWheel<T extends HTMLElement>(ref: RefObject<T | null>, handler: (e: WheelEvent) => void): void {
  const latest = useRef(handler);
  latest.current = handler;
  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    const onWheel = (e: WheelEvent) => latest.current(e);
    el.addEventListener("wheel", onWheel, { passive: false });
    return () => el.removeEventListener("wheel", onWheel);
  }, [ref]);
}
