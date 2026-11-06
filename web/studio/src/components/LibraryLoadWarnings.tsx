// The status bar's library load warnings (`KISTATUSBAR::AddWarningMessages( "load", .. )`, which the frames call with
// `LIBRARY_MANAGER_ADAPTER::GetLibraryLoadErrors()` once the libraries are loaded, eeschema/cross-probing.cpp, KiCad 25ec2335): a warning icon while a symbol
// library loaded with errors, the messages in a window when it is clicked. The list is state/libraryLoadErrors.ts's, asked of the server once the page is up
// and again when the library tables change; a chooser that reads such a library adds to it.
import { useEffect, useState } from "react";
import { loadErrorDetails, loadErrorSummary } from "../kicad-port/libraryLoadErrors";
import { refreshLoadErrors, useLibraryLoadErrors } from "../state/libraryLoadErrors";

export function LibraryLoadWarnings() {
  const errors = useLibraryLoadErrors();
  const [open, setOpen] = useState(false);
  // KiCad preloads the libraries when a frame opens and says what failed then; here the server is asked once the page has settled (reading every library
  // folder is a few seconds the first time, and it runs on a request of its own)
  useEffect(() => {
    const t = setTimeout(() => void refreshLoadErrors(), 4000);
    return () => clearTimeout(t);
  }, []);
  if (errors.length === 0) return null;
  return (
    <span style={{ position: "relative" }}>
      <button className="field" style={{ color: "var(--chrome-warn)", background: "none", border: "none", cursor: "pointer", padding: "0 4px" }} title={loadErrorDetails(errors)} onClick={() => setOpen((v) => !v)} data-library-load-warnings>
        {"⚠ "}
        {loadErrorSummary(errors)}
      </button>
      {open && (
        <div
          role="dialog"
          aria-label="Library load errors"
          data-library-load-errors
          style={{ position: "absolute", bottom: "100%", left: 0, width: "min(640px, 80vw)", maxHeight: "50vh", overflow: "auto", background: "var(--chrome-bg)", border: "1px solid var(--chrome-border)", padding: 8, zIndex: 50, boxShadow: "0 2px 10px rgba(0,0,0,0.35)" }}
        >
          <div style={{ display: "flex", alignItems: "center", marginBottom: 6 }}>
            <strong style={{ flex: 1 }}>Library load errors</strong>
            <button onClick={() => setOpen(false)} aria-label="Close">
              {"×"}
            </button>
          </div>
          <pre style={{ margin: 0, whiteSpace: "pre-wrap", fontSize: 11 }}>{loadErrorDetails(errors)}</pre>
        </div>
      )}
    </span>
  );
}
