// `WX_PROGRESS_REPORTER`'s window (KiCad 8303b2ad common/widgets/wx_progress_reporters.cpp, a `wxProgressDialog` with `wxPD_APP_MODAL | wxPD_ELAPSED_TIME`
// and `wxPD_CAN_ABORT` with `PR_CAN_ABORT`): the title, the message, the gauge, "Elapsed time:" and Cancel, application-modal while the job runs. The job
// drives it through state/progressReporter.ts.
import { useEffect, useState } from "react";
import { elapsedLabel, updateProgressReport, useProgressReport } from "../state/progressReporter";

export function ProgressReporterDialog() {
  const report = useProgressReport();
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    if (!report) return;
    const t = window.setInterval(() => setNow(Date.now()), 250);
    return () => window.clearInterval(t);
  }, [report]);
  if (!report) return null;
  const abort = () => {
    if (report.aborting || !report.onAbort) return;
    updateProgressReport({ aborting: true });
    report.onAbort();
  };
  return (
    // `wxPD_APP_MODAL`: nothing else takes a click while it is up (`wxWindowDisabler`)
    <div className="dialog-backdrop" data-testid="progress-reporter">
      <div className="dialog" style={{ width: 420 }} role="dialog" aria-label={report.title} onKeyDown={(e) => e.key === "Escape" && abort()}>
        <div className="dialog-header">
          <span data-testid="progress-reporter-title">{report.title}</span>
        </div>
        <div className="dialog-body">
          <div data-testid="progress-reporter-message" style={{ minHeight: "1.2em" }}>
            {report.message}
          </div>
          <progress data-testid="progress-reporter-gauge" max={1000} value={Math.max(0, Math.min(1000, report.progress))} style={{ width: "100%", margin: "8px 0" }} />
          <div style={{ display: "flex", justifyContent: "space-between" }}>
            <span>Elapsed time:</span>
            <span data-testid="progress-reporter-elapsed">{elapsedLabel(now - report.startedAt)}</span>
          </div>
        </div>
        {report.onAbort && (
          <div className="dialog-footer">
            <button data-testid="progress-reporter-cancel" disabled={!!report.aborting} onClick={abort}>
              Cancel
            </button>
          </div>
        )}
      </div>
    </div>
  );
}
