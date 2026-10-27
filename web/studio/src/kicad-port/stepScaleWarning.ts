// `DIALOG_EXPORT_STEP::DIALOG_EXPORT_STEP`'s "Model Scale Warning" (pcbnew/dialogs/dialog_export_step.cpp): when the Export STEP/GLB/...
// dialog opens, every 3D model of the board's footprints whose scale is not 1 is listed (the scan stops after the footprint that brings the
// count to 5 or more), and a warning the user can turn off ("Do not show this message again", `m_DoNotShowAgain.scaled_3d_models_warning`)
// is shown. Pure: the dialog shows it.

export interface ScaledModelsWarning {
  /** `KIDIALOG`'s caption. */
  title: string;
  /** The message, word for word (two spaces after the full stop). */
  message: string;
  /** `SetExtendedMessage`: "Non-unity scaled models:" and one file name per line, each after a newline. */
  extended: string;
}

interface PartModels {
  placed?: boolean;
  models?: ReadonlyArray<{ name: string; scale: readonly [number, number, number] | number[] }>;
}

/** The warning for the board's footprints (`parts` in board order; an unplaced part has no footprint on the board), or `null` when every model has unity scale. */
export function scaledModelsWarning(parts: readonly PartModels[]): ScaledModelsWarning | null {
  let badScales = "";
  let badCount = 0;
  for (const p of parts) {
    if (!p.placed) continue;
    for (const m of p.models ?? []) {
      if (m.scale[0] !== 1 || m.scale[1] !== 1 || m.scale[2] !== 1) {
        badScales += "\n" + m.name;
        badCount++;
      }
    }
    if (badCount >= 5) break;
  }
  if (!badScales) return null;
  return {
    title: "Model Scale Warning",
    message: "Scaled models detected.  Model scaling is not reliable for mechanical export.",
    extended: "Non-unity scaled models:" + "\n" + badScales,
  };
}

/** `COMMON_SETTINGS::m_DoNotShowAgain.scaled_3d_models_warning`, kept in this browser. */
const KEY = "eda.doNotShowAgain.scaled_3d_models_warning";

export function scaledModelsWarningSuppressed(): boolean {
  try {
    return globalThis.localStorage?.getItem(KEY) === "1";
  } catch {
    return false;
  }
}

export function suppressScaledModelsWarning(): void {
  try {
    globalThis.localStorage?.setItem(KEY, "1");
  } catch {
    // no storage: shown again next time
  }
}
