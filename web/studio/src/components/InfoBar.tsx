// A frame's info bar over its canvas (`WX_INFOBAR`, common/widgets/infobar.cpp at KiCad 8303b2ad): the message with its icon (`wxICON_ERROR`, `wxICON_WARNING`,
// `wxICON_INFORMATION`), and the close button when the caller asked for one (`AddCloseButton`). The Symbol Editor's (components/symbol/SymbolInfoBar.tsx) and the
// board editor's (components/PcbInfoBar.tsx) are this one bar.
export interface InfoBarProps {
  message: string;
  kind: "error" | "warning" | "info";
  /** `AddCloseButton`: the bar's close button, which dismisses it. */
  onClose?: () => void;
  /** `AddButton( wxHyperlinkCtrl )`: a link on the bar (the zone fill warning's "Don't show again"). */
  link?: { label: string; onClick: () => void };
  testid: string;
}

export function InfoBar({ message, kind, onClose, link, testid }: InfoBarProps) {
  const icon = kind === "error" ? "⛔" : kind === "warning" ? "⚠" : "ℹ";
  return (
    <div
      role="alert"
      data-testid={testid}
      data-kind={kind}
      style={{ position: "absolute", top: 0, left: 0, right: 0, zIndex: 6, padding: "4px 10px", fontSize: 12, background: "var(--chrome-bg-raised)", borderBottom: `1px solid ${kind === "error" ? "#ef5b5b" : "var(--chrome-border)"}`, display: "flex", gap: 8, alignItems: "center" }}
    >
      <span aria-hidden>{icon}</span>
      <span data-testid={`${testid}-text`} style={{ flex: 1 }}>
        {message}
      </span>
      {link && (
        <a
          href="#"
          data-testid={`${testid}-link`}
          style={{ color: "inherit" }}
          onClick={(e) => {
            e.preventDefault();
            link.onClick();
          }}
        >
          {link.label}
        </a>
      )}
      {onClose && (
        <button type="button" data-testid={`${testid}-close`} aria-label="Close" title="Close" style={{ background: "none", border: "none", color: "inherit", cursor: "pointer", fontSize: 14, padding: 0 }} onClick={onClose}>
          ×
        </button>
      )}
    </div>
  );
}
