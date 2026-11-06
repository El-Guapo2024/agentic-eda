// Whether a Control key is down, from the page's own key events -- what tells a touchpad pinch from Ctrl and the wheel (`isPinchWheel`,
// kicad-port/viewControls.ts): the browser reports a pinch as a wheel event with `ctrlKey` set while no Control key is pressed.
//
// One window listener for the whole page, installed on first use. A key released while the page had no focus never reaches it, so the state is
// dropped when the window loses focus.
let held = false;
let installed = false;

function install(): void {
  if (installed || typeof window === "undefined") return;
  installed = true;
  window.addEventListener("keydown", (e) => {
    if (e.key === "Control") held = true;
  }, true);
  window.addEventListener("keyup", (e) => {
    if (e.key === "Control") held = false;
  }, true);
  window.addEventListener("blur", () => {
    held = false;
  });
}

/** A Control key is down now. */
export function controlKeyHeld(): boolean {
  install();
  return held;
}

install();
