// Port of common/view/zoom_controller.cpp + include/view/zoom_controller.h:
// turns one wheel-tick's "rotation" magnitude into a zoom scale factor.
// Two strategies, exactly as source has them:
//   - CONSTANT_ZOOM_CONTROLLER: factor scales linearly with rotation
//     magnitude (clamped to +-100), platform-tuned constant.
//   - ACCELERATING_ZOOM_CONTROLLER: factor grows the faster consecutive
//     wheel ticks arrive (a real wall-clock timeout), for mice/platforms
//     where discrete clicky wheel notches benefit from "spin it faster to
//     zoom faster" instead of a fixed per-notch step.
//
// wx_view_controls.cpp's GetZoomControllerForPlatform() picks CONSTANT on
// Mac and GTK3 unconditionally (their wheel events are high-frequency/
// small-rotation, so a constant per-rotation scale already feels smooth),
// and ACCELERATING vs. CONSTANT(MSW_SCALE) elsewhere depending on the
// zoom_acceleration setting. This app's "elsewhere" is just "non-Mac
// browser", folded into pickDefaultZoomController below.

/** wxMouseEvent::GetWheelRotation() has no real DOM equivalent -- see wheel.ts's own header comment for how a DOM WheelEvent's deltaY is mapped onto this "rotation" axis. */
export interface ZoomController {
  getScaleForRotation(rotation: number): number;
}

/** zoom_controller.h CONSTANT_ZOOM_CONTROLLER's four named platform/manual scales, unchanged. */
export const CONSTANT_SCALE = {
  GTK3: 0.002,
  MAC: 0.01,
  MSW: 0.005,
  /** Multiplier for the user's manual "zoom speed" setting (1-10 in KiCad's preferences). */
  MANUAL_FACTOR: 0.001,
} as const;

/** zoom_controller.cpp CONSTANT_ZOOM_CONTROLLER::GetScaleForRotation, byte-for-byte: clamp rotation to +-100, then `1 + rotation*scale` (zooming in) or `1 / (1 - rotation*scale)` (zooming out) -- an asymmetric pair so repeated in/out ticks of the same magnitude are exact inverses of one another. */
export class ConstantZoomController implements ZoomController {
  constructor(private readonly scale: number) {}

  getScaleForRotation(rotation: number): number {
    const r = rotation > 0 ? Math.min(rotation, 100) : Math.max(rotation, -100);
    const dscale = r * this.scale;
    return r > 0 ? 1 + dscale : 1 / (1 - dscale);
  }
}

/** The clock ACCELERATING_ZOOM_CONTROLLER measures tick spacing against. Swappable so tests can control time without real delays (ACCELERATING_ZOOM_CONTROLLER's own TIMESTAMP_PROVIDER interface, which source injects for exactly this reason). */
export type Clock = () => number;

const DEFAULT_TIMEOUT_MS = 500;
const DEFAULT_ACCELERATION_SCALE = 5.0;
/** zoom_controller.cpp: "the minimal step value when changing the current zoom level" -- a floor so a long pause between ticks never decays the per-tick zoom step below a barely-perceptible 5%. */
const MIN_STEP = 1.05;

/** zoom_controller.cpp ACCELERATING_ZOOM_CONTROLLER::GetScaleForRotation, byte-for-byte including its same-direction-only acceleration rule (a reversal always falls back to the unaccelerated MIN_STEP, source's `(aRotation > 0) == m_prevRotationPositive` guard). */
export class AcceleratingZoomController implements ZoomController {
  private prevTimestamp: number;
  private prevRotationPositive = false;

  constructor(
    private readonly scale: number = DEFAULT_ACCELERATION_SCALE,
    private readonly timeoutMs: number = DEFAULT_TIMEOUT_MS,
    private readonly clock: Clock = () => Date.now()
  ) {
    this.prevTimestamp = this.clock();
  }

  getScaleForRotation(rotation: number): number {
    const timestamp = this.clock();
    const timeDiffMs = timestamp - this.prevTimestamp;
    this.prevTimestamp = timestamp;

    let zoomScale: number;
    if (timeDiffMs < this.timeoutMs && rotation > 0 === this.prevRotationPositive) {
      zoomScale = Math.max((2.05 * this.scale) / 5.0 - timeDiffMs / this.timeoutMs, MIN_STEP);
      if (rotation < 0) zoomScale = 1.0 / zoomScale;
    } else {
      zoomScale = rotation > 0 ? MIN_STEP : 1 / MIN_STEP;
    }
    this.prevRotationPositive = rotation > 0;
    return zoomScale;
  }
}

/**
 * wx_view_controls.cpp GetZoomControllerForPlatform(), as reached from
 * LoadSettings() with KiCad's actual factory-default input settings
 * (zoom_speed_auto=true, zoom_acceleration=false -- common_settings.cpp):
 *
 *     #ifdef __WXMAC__       -> CONSTANT(MAC_SCALE)
 *     #elif __WXGTK3__       -> CONSTANT(GTK3_SCALE)
 *     #else                  -> aAcceleration ? ACCELERATING() : CONSTANT(MSW_SCALE)
 *
 * With the default `zoom_acceleration=false`, every platform gets a
 * CONSTANT controller out of the box -- ACCELERATING only turns on if the
 * user explicitly enables "Zoom acceleration" in Preferences (exposed here
 * as `accelerationEnabled`, still false by default). A browser has no
 * GTK3-specific build, so "else" collapses to MSW_SCALE for any non-Mac
 * host, same as a stock Windows KiCad.
 */
export function pickDefaultZoomController(isMacPlatform: boolean, accelerationEnabled = false): ZoomController {
  if (isMacPlatform) return new ConstantZoomController(CONSTANT_SCALE.MAC);
  return accelerationEnabled ? new AcceleratingZoomController() : new ConstantZoomController(CONSTANT_SCALE.MSW);
}
