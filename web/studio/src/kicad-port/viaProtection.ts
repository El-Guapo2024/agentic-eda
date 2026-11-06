// `VIA_PROTECTION_UI_MIXIN` (pcbnew/via_protection_ui_mixin.h at KiCad 8303b2ad): the IPC-4761 presets of Track & Via Properties' "Protection features:" --
// `m_IPC4761Presets` (what each preset sets: the tenting, covering and plugging of each side, the filling and capping of the hole), `getViaConfiguration` (the
// preset a via's settings are, else "custom"), `setViaConfiguration` (a preset onto a via; `CUSTOM` and past change nothing). The Rust side has the same table for
// the global edit (`crates/ops/src/pcb_global_edit.rs` `Ipc4761Preset::apply`); the dialog needs it here to show the via's preset and to fold the preset into the
// one `set_via_props` it sends per via with the backdrill and post-machining.
//
// Pure: no React, no DOM (compiled by `npm run test:unit`).
import type { Ipc4761Preset, ViaProps } from "../api/types";

/** `IPC4761_SURFACE` (`CUSTOM` only as `getProtectionSurface`'s answer). */
type Surface = "from_board" | "none" | "front" | "back" | "both" | "custom";
/** `IPC4761_DRILL`. */
type Drill = "from_board" | "not_set" | "set";

interface Configuration {
  tent: Surface;
  cover: Surface;
  plug: Surface;
  fill: Drill;
  cap: Drill;
}

/** `IPC4761_CONFIGURATION`'s defaults: nothing on either side, the hole neither filled nor capped. */
const NONE: Configuration = { tent: "none", cover: "none", plug: "none", fill: "not_set", cap: "not_set" };

/** `m_IPC4761Presets`, in `IPC4761_PRESET`'s order up to `CUSTOM`. */
export const IPC4761_CONFIGS: readonly [Ipc4761Preset, Configuration][] = [
  ["from_board", { tent: "from_board", cover: "from_board", plug: "from_board", fill: "from_board", cap: "from_board" }],
  ["none", NONE],
  ["ia", { ...NONE, tent: "front" }],
  ["ib", { ...NONE, tent: "both" }],
  ["ia_inverted", { ...NONE, tent: "back" }],
  ["iia", { ...NONE, tent: "front", cover: "front" }],
  ["iib", { ...NONE, tent: "both", cover: "both" }],
  ["iia_inverted", { ...NONE, tent: "back", cover: "back" }],
  ["iiia", { ...NONE, plug: "front" }],
  ["iiib", { ...NONE, plug: "both" }],
  ["iiia_inverted", { ...NONE, plug: "back" }],
  ["iva", { ...NONE, tent: "front", plug: "front" }],
  ["ivb", { ...NONE, tent: "both", plug: "both" }],
  ["iva_inverted", { ...NONE, tent: "back", plug: "back" }],
  ["v", { ...NONE, fill: "set" }],
  ["via", { ...NONE, tent: "front", fill: "set" }],
  ["vib", { ...NONE, tent: "both", fill: "set" }],
  ["via_inverted", { ...NONE, tent: "back", fill: "set" }],
  ["vii", { ...NONE, fill: "set", cap: "set" }],
];

/** `getProtectionSurface`. */
function surfaceOf(front: boolean | undefined, back: boolean | undefined): Surface {
  const f: Surface = front === undefined ? "from_board" : front ? "front" : "none";
  if (back === undefined) return f === "from_board" ? "from_board" : "custom";
  if (back) return f === "front" ? "both" : f === "none" ? "back" : "custom";
  return f === "front" ? "front" : f === "none" ? "none" : "custom";
}

/** `getProtectionDrill`. */
const drillOf = (v: boolean | undefined): Drill => (v === undefined ? "from_board" : v ? "set" : "not_set");

/** `getViaConfiguration`: the preset the via's settings are, else `"custom"` (`IPC4761_PRESET::CUSTOM`). */
export function viaConfiguration(p: ViaProps | null | undefined): Ipc4761Preset | "custom" {
  const q = p ?? {};
  const c: Configuration = {
    tent: surfaceOf(q.front_tenting, q.back_tenting),
    cover: surfaceOf(q.front_covering, q.back_covering),
    plug: surfaceOf(q.front_plugging, q.back_plugging),
    cap: drillOf(q.capping),
    fill: drillOf(q.filling),
  };
  const hit = IPC4761_CONFIGS.find(([, k]) => k.tent === c.tent && k.cover === c.cover && k.plug === c.plug && k.fill === c.fill && k.cap === c.cap);
  return hit ? hit[0] : "custom";
}

/** `setSurfaceProtection`: the two sides of one feature. */
function setSurface(p: ViaProps, front: "front_tenting" | "front_covering" | "front_plugging", back: "back_tenting" | "back_covering" | "back_plugging", s: Surface) {
  switch (s) {
    case "from_board":
      delete p[front];
      delete p[back];
      break;
    case "none":
      p[front] = false;
      p[back] = false;
      break;
    case "front":
      p[front] = true;
      p[back] = false;
      break;
    case "back":
      p[front] = false;
      p[back] = true;
      break;
    case "both":
      p[front] = true;
      p[back] = true;
      break;
    case "custom":
      break;
  }
}

/** `setDrillProtection`. */
function setDrill(p: ViaProps, key: "filling" | "capping", d: Drill) {
  if (d === "from_board") delete p[key];
  else p[key] = d === "set";
}

/** `setViaConfiguration`: the via's settings with `preset` applied (tenting, plugging, covering, filling, capping, in KiCad's order); `"custom"` changes nothing. */
export function withPreset(p: ViaProps | null | undefined, preset: Ipc4761Preset | "custom"): ViaProps {
  const next: ViaProps = { ...(p ?? {}) };
  const c = IPC4761_CONFIGS.find(([k]) => k === preset)?.[1];
  if (!c) return next;
  setSurface(next, "front_tenting", "back_tenting", c.tent);
  setSurface(next, "front_plugging", "back_plugging", c.plug);
  setSurface(next, "front_covering", "back_covering", c.cover);
  setDrill(next, "filling", c.fill);
  setDrill(next, "capping", c.cap);
  return next;
}
