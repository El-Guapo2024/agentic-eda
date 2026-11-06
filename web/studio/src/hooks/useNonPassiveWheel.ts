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
    // Safari's touchpad pinch is no wheel event but WebKit's `gesturestart` / `gesturechange`, with the scale since the gesture began: each step becomes the pinch
    // wheel the other browsers send (`ctrlKey` with no Control key down, `deltaY = -100 * ln( step )`), so it zooms as `WX_VIEW_CONTROLS::onMagnify` does -- the
    // macOS path of KiCad's pinch, where `onZoomGesture` is not built (`#ifndef __WXOSX__`). Cancelling them keeps Safari from zooming the page itself.
    let lastScale = 1;
    const onGesture = (e: Event) => {
      const g = e as Event & { scale?: number; clientX?: number; clientY?: number };
      e.preventDefault();
      if (e.type === "gesturestart") lastScale = 1;
      const scale = typeof g.scale === "number" && g.scale > 0 ? g.scale : 1;
      const step = scale / lastScale;
      lastScale = scale;
      if (step === 1 || e.type === "gesturestart") return;
      latest.current(new WheelEvent("wheel", { deltaY: -100 * Math.log(step), ctrlKey: true, clientX: g.clientX ?? 0, clientY: g.clientY ?? 0 }));
    };
    el.addEventListener("gesturestart", onGesture, { passive: false });
    el.addEventListener("gesturechange", onGesture, { passive: false });
    return () => {
      el.removeEventListener("wheel", onWheel);
      el.removeEventListener("gesturestart", onGesture);
      el.removeEventListener("gesturechange", onGesture);
    };
  }, [ref]);
}
