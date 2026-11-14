// The `.cmp` footprint link file the board editor writes (`SCH_EDITOR_CONTROL::processCmpToFootprintLinkFile`, eeschema/tools/assign_footprints.cpp at
// 8303b2ad): blocks from `BeginCmp` to `EndCmp`, each with lines like `Reference = R1;` and `IdModule  = Resistor_SMD:R_0603_1608Metric;`.
// Used by `eeschema.EditorControl.importFPAssignments`.

export interface CmpLink {
  reference: string;
  footprint: string;
}

/**
 * Every block that names a reference, in file order. A value is what stands between the first `=` and the last `;` of its line, trimmed
 * (`AfterFirst( '=' )`, `BeforeLast( ';' )`: a line without a `;` has no value). A block without a reference is skipped; one without
 * `IdModule` links the symbol to an empty footprint, like the C++ (`SetFootprintFieldText( footprint )` runs either way).
 */
export function parseCmpFile(text: string): CmpLink[] {
  const lines = text.split(/\r\n|\r|\n/);
  const links: CmpLink[] = [];
  for (let i = 0; i < lines.length; i++) {
    if (!lines[i]!.startsWith("BeginCmp")) continue;
    let reference = "";
    let footprint = "";
    for (i++; i < lines.length; i++) {
      const line = lines[i]!;
      if (line.startsWith("EndCmp")) break;
      const eq = line.indexOf("=");
      const afterEq = eq >= 0 ? line.slice(eq + 1) : "";
      const semi = afterEq.lastIndexOf(";");
      const value = semi >= 0 ? afterEq.slice(0, semi).trim() : "";
      if (line.startsWith("Reference")) reference = value;
      else if (line.startsWith("IdModule")) footprint = value;
    }
    if (reference !== "") links.push({ reference, footprint });
  }
  return links;
}

/** `ImportFPAssignments`' question (`wxSingleChoiceDialog( m_frame, .., _( "Change Visibility" ), choices )`), word for word (UI-LEFT-3 S3-25). */
export const FP_VISIBILITY_TITLE = "Change Visibility";
export const FP_VISIBILITY_PROMPT = "Select the footprint field visibility setting.";
export const FP_VISIBILITY_CHOICES: readonly string[] = ["Keep existing footprint field visibility", "Show all footprint fields", "Hide all footprint fields"];

/** The choice's meaning (`forceVisibility = GetSelection() != 0`, `visibilityState = GetSelection() == 1`): the visibility every listed symbol's Footprint field
 *  is given, or null to keep each one's. */
export const forcedFootprintVisibility = (choice: number): boolean | null => (choice === 0 ? null : choice === 1);
