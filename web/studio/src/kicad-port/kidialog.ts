// `KIDIALOG` (common/kidialog.cpp at KiCad 8303b2ad): a question that can show a "Do not show again" box (`DoNotShowCheckbox( __FILE__, __LINE__ )`).
// Once it is answered with the box ticked, the same question is answered so at once for as long as the program runs (`g_doNotShowAgainDlgs`, a static
// map, never saved), until Preferences' "Reset Do Not Show Again" (`ClearDoNotShowAgainDialogs`). A Cancel is not remembered while Cancel means
// cancel (`m_cancelMeansCancel`, the default): "we don't save a Cancel value unless the Cancel button is being used for some other function".

/** `g_doNotShowAgainDlgs`: the answers given with "Do not show again" ticked, by question. */
const doNotShowAgain = new Map<string, boolean>();

/** `KIDIALOG::ClearDoNotShowAgainDialogs`. */
export function clearDoNotShowAgain(): void {
  doNotShowAgain.clear();
}

/** `KIDIALOG::ShowModal`'s `g_doNotShowAgainDlgs.find( m_hash )`: the answer a question (`key`) has had, or `undefined` when it is to be asked. */
export function rememberedAnswer(key: string | undefined): boolean | undefined {
  return key === undefined ? undefined : doNotShowAgain.get(key);
}

/** `if( IsCheckBoxChecked() && ( !m_cancelMeansCancel || ret != wxID_CANCEL ) ) g_doNotShowAgainDlgs[m_hash] = ret;` */
export function noteAnswer(key: string | undefined, ticked: boolean, answer: boolean, cancelMeansCancel = true): void {
  if (key === undefined || !ticked) return;
  if (!cancelMeansCancel || answer) doNotShowAgain.set(key, answer);
}
