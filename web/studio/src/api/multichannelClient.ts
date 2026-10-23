// The requests of the multichannel tool's dialogs (crates/cli/src/multichannel_api.rs): the rows of Generate Placement Rule Areas, the footprints Rule Area Properties offers as sources,
// and the table of Repeat Layout (every other placement rule area matched against the reference). What the tool does is a command (`multichannel` in api/types.ts), not a request here.

/** What a rule area would be made from (`GeneratePotentialRuleAreas`). */
export interface PotentialArea {
  kind: "sheet" | "component_class" | "group";
  /** The sheet's path (`/ch1/`), the class's name or the group's name. */
  key: string;
  /** The sheet's file; empty for the others. */
  file: string;
  /** The references of the footprints it would hold. */
  components: string[];
}

export interface PlacementSources {
  sheets: string[];
  classes: string[];
  groups: string[];
}

/** One target of Repeat Layout: whether its footprints match the reference area's, and if not why. */
export interface TargetCompat {
  id: string;
  name: string;
  ok: boolean;
  message: string;
  reasons: string[];
  components: string[];
  pairs: [string, string][];
}

export interface RepeatPlan {
  id: string;
  name: string;
  components: string[];
  targets: TargetCompat[];
}

export interface Refused {
  ok: false;
  message: string;
}

async function post<T>(url: string, body: unknown): Promise<T> {
  const r = await fetch(url, { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify(body) });
  return (await r.json()) as T;
}

/** The rows of the Generate dialog. */
export async function fetchPotentialAreas(): Promise<{ ok: true; areas: PotentialArea[] } | Refused> {
  return post("/api/multichannel/areas", {});
}

/** The sheets, component classes and groups the Placement tab offers. */
export async function fetchPlacementSources(): Promise<{ ok: true } & PlacementSources | Refused> {
  return post("/api/multichannel/sources", {});
}

/** `CheckRACompatibility`: the reference rule area (a zone id) matched against every other placement rule area. */
export async function fetchRepeatPlan(reference: string): Promise<({ ok: true } & RepeatPlan) | Refused> {
  return post("/api/multichannel/compat", { reference });
}
