// One page of the Preferences dialog (`PAGED_DIALOG`'s `wxTreebook`, common/widgets/paged_dialog.cpp at KiCad 8303b2ad) that keeps its own working copy while
// the dialog is up, the way a `wxPanel` page does: `TransferDataToWindow` when the dialog opens, `TransferDataFromWindow` on OK (`commit`), nothing on Cancel
// (`cancel` undoes what the page showed live, e.g. a colour preview), and `RESETTABLE_PANEL::ResetPanel` behind the dialog's "Reset %s to Defaults" button
// (`PAGED_DIALOG::onResetButton`, shown for a page that has one). Each page is a hook `useXxxPage( open, shown )`: `open` is the dialog being up (the page's
// state lives as long as the dialog, as a page of a `wxTreebook` outlives a switch to another page), `shown` the page being the one on screen (`element` is null
// otherwise).
import type { ReactNode } from "react";

export interface PreferencesPageHandle {
  /** The page's controls, or null while another page is shown. */
  element: ReactNode;
  /** OK: `TransferDataFromWindow`. */
  commit: () => void;
  /** Cancel: what the page showed live is put back. */
  cancel: () => void;
  /** `ResetPanel`, offered by the dialog's reset button while the page is shown. */
  reset?: () => void;
  /** The reset button's text, "Reset %s to Defaults" with the page's title (`PAGED_DIALOG::onPageChanged`). */
  resetLabel?: string;
  /** The reset button's tooltip (`RESETTABLE_PANEL::GetResetTooltip`). */
  resetTooltip?: string;
  /** False refuses OK (a page's `TransferDataFromWindow` returning false keeps the dialog up). */
  valid?: boolean;
  /** A question OK asks first (`TransferDataFromWindow`'s, e.g. the schematic Colors page's `validateSave`): false keeps the dialog up and saves nothing. */
  confirmOk?: () => Promise<boolean>;
}
