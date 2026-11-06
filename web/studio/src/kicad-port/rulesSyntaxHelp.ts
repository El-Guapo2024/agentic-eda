// The Syntax Help windows of the custom rules editors: `PANEL_SETUP_RULES::OnSyntaxHelp` (pcbnew/dialogs/panel_setup_rules.cpp, Board Setup > Design
// Rules > Custom Rules) and `PANEL_DRC_RULE_EDITOR::onSyntaxHelp` (pcbnew/drc/rule_editor/panel_drc_rule_editor.cpp, the DRC Rule Editor's
// conditions) at KiCad 8303b2ad. Both put the markdown texts KiCad compiles in (src/kicad/rules_help.json, copied from the
// `panel_setup_*_help_*.h` by tools/extract-rules-help.js) one after the other, write `Cmd+` for `Ctrl+` on a Mac, convert the whole with
// `ConvertMarkdown2Html` (markdownHtml.ts) and give every table cell `valign=top`, then show it in a modeless `HTML_MESSAGE_BOX` titled
// "Syntax Help" (`SetDialogSizeInDU( 420, 320 )`). `wxGetTranslation` of each text is the text itself: the studio is in English.
//
// Pure: no React, no DOM.
import { convertMarkdown2Html } from "./markdownHtml";

/** The window's title (`_( "Syntax Help" )`). */
export const SYNTAX_HELP_TITLE = "Syntax Help";

/** `SetDialogSizeInDU( 420, 320 )`: the window's size in dialog units (a quarter of the average character width across, an eighth of its height down). */
export const SYNTAX_HELP_SIZE_DU = { width: 420, height: 320 } as const;

/** The help's markdown: `msg_txt << wxGetTranslation( i )` for each text in order, and on a Mac `msg_txt.Replace( "Ctrl+", "Cmd+" )`. */
export function syntaxHelpMarkdown(texts: readonly string[], mac: boolean): string {
  const all = texts.join("");
  return mac ? all.replaceAll("Ctrl+", "Cmd+") : all;
}

/** What the window shows: `ConvertMarkdown2Html( msg_txt, html_txt )`, then `html_txt.Replace( "<td", "<td valign=top" )`. */
export function syntaxHelpHtml(texts: readonly string[], mac: boolean): string {
  return convertMarkdown2Html(syntaxHelpMarkdown(texts, mac)).replaceAll("<td", "<td valign=top");
}
