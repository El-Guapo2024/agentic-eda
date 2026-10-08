// `ACTIONS::about` -> DIALOG_ABOUT (common/dialog_about/dialog_about.cpp over AboutDialog_main.cpp): the application's title and version, the
// Donate / Report Bug / Copy Version Info buttons and a notebook with the About, Version and License pages. Ported: the three pages and the
// four buttons (Donate and Report Bug run the same actions the Help menu does, Copy Version Info copies `GetVersionInfoData`'s text). Not
// ported: the contributor pages (Developers, Doc Writers, Librarians, Artists, Translators, Packagers) -- they list KiCad's people, which the
// About page points to instead of copying.
import { useState } from "react";
import pkg from "../../package.json";
import { setAboutOpen, useCommonDialogs } from "../state/commonDialogs";
import { useStudioState } from "../state/store";
import { useActionRunner } from "../actions/useActionRunner";
import { makeVersionEnv } from "../actions/versionEnv";
import { URL_SOURCE, versionInfoText, type HelpTab } from "../kicad-port/appLinks";

type Page = "about" | "version" | "license";

function Link({ href, children }: { href: string; children?: React.ReactNode }) {
  return (
    <a href={href} target="_blank" rel="noopener noreferrer" style={{ color: "var(--chrome-link, #6aa9ff)" }}>
      {children ?? href}
    </a>
  );
}

/** `buildKicadAboutBanner`'s description: what the program is, and the sites around it. */
function AboutPage() {
  const heading: React.CSSProperties = { fontWeight: 700, textDecoration: "underline", margin: "10px 0 4px" };
  const list: React.CSSProperties = { margin: "0 0 0 20px", padding: 0 };
  return (
    <div style={{ fontSize: 12, lineHeight: 1.5 }}>
      <div style={{ ...heading, marginTop: 0 }}>Description</div>
      <p style={{ margin: "0 0 4px" }}>
        EDA Studio is a browser port of the KiCad PCB Editor, Schematic Editor, Footprint Editor and Symbol Editor. It edits one design (design.json); the KiCad files, the design rule and
        electrical rule checks, the plots and the other exports come from kicad-cli.
      </p>
      <div style={heading}>KiCad on the web</div>
      <ul style={list}>
        <li>
          The official KiCad website - <Link href="http://www.kicad.org" />
        </li>
        <li>
          Developer website - <Link href="https://go.kicad.org/dev" />
        </li>
        <li>
          Official KiCad library repositories - <Link href="https://go.kicad.org/libraries" />
        </li>
      </ul>
      <div style={heading}>Bug tracker</div>
      <ul style={list}>
        <li>
          Report or examine bugs in EDA Studio - <Link href={`${URL_SOURCE}/issues`} />
        </li>
        <li>
          Report or examine bugs in KiCad - <Link href="https://go.kicad.org/bugs" />
        </li>
      </ul>
      <div style={heading}>KiCad users group and community</div>
      <ul style={list}>
        <li>
          KiCad forum - <Link href="https://go.kicad.org/forum" />
        </li>
      </ul>
      <div style={heading}>Credits</div>
      <p style={{ margin: 0 }}>
        The windows, tools and behaviour are ported from the KiCad source by the KiCad Developers; the people behind it are listed at <Link href="https://go.kicad.org/dev" />.
      </p>
    </div>
  );
}

export function AboutDialog() {
  const open = useCommonDialogs().about;
  const studio = useStudioState();
  const { run } = useActionRunner();
  const [page, setPage] = useState<Page>("about");
  const [copied, setCopied] = useState<"idle" | "done" | "failed">("idle");
  if (!open) return null;

  const env = makeVersionEnv(studio.tab as HelpTab);
  const title = env.title;
  const text = versionInfoText(env);
  const close = () => {
    setAboutOpen(false);
    setCopied("idle");
    setPage("about");
  };
  // `DIALOG_ABOUT::onCopyVersionInfo`: the version information to the clipboard; the button says it was copied.
  const copy = () => {
    navigator.clipboard
      .writeText(text)
      .then(() => setCopied("done"))
      .catch(() => setCopied("failed"));
  };

  const pages: { id: Page; label: string }[] = [
    { id: "about", label: "About" },
    { id: "version", label: "Version" },
    { id: "license", label: "License" },
  ];

  return (
    <div className="dialog-backdrop" onClick={close}>
      <div className="dialog" style={{ width: 560 }} onClick={(e) => e.stopPropagation()} role="dialog" aria-label={`About EDA Studio ${title}`}>
        <div className="dialog-header">
          <span>{`About EDA Studio ${title}`}</span>
        </div>
        <div className="dialog-body" style={{ display: "flex", flexDirection: "column", gap: 8 }}>
          <div>
            <div style={{ fontSize: 18, fontWeight: 700 }}>{`EDA Studio ${title}`}</div>
            <div style={{ fontSize: 12 }}>{`Version: ${env.version}, ${env.mode} build`}</div>
            <div style={{ fontSize: 11, color: "var(--chrome-text-dim)", whiteSpace: "pre-line" }}>{`React ${env.reactVersion}\nPlatform: ${env.platform || "unknown"}`}</div>
          </div>
          <div style={{ display: "flex", gap: 6 }}>
            <button onClick={() => run("common.SuiteControl.donate")}>Donate</button>
            <button onClick={() => run("common.SuiteControl.reportBug")}>Report Bug</button>
            <button onClick={copy}>{copied === "done" ? "Copied..." : "Copy Version Info"}</button>
          </div>
          {copied === "failed" && <div style={{ fontSize: 11, color: "var(--chrome-error, #e55)" }}>Could not open clipboard to write version information.</div>}
          <div role="tablist" style={{ display: "flex", gap: 2, borderBottom: "1px solid var(--chrome-border)" }}>
            {pages.map((p) => (
              <button
                key={p.id}
                role="tab"
                aria-selected={page === p.id}
                onClick={() => setPage(p.id)}
                style={{ borderBottomLeftRadius: 0, borderBottomRightRadius: 0, fontWeight: page === p.id ? 700 : 400, background: page === p.id ? "var(--chrome-active)" : undefined }}
              >
                {p.label}
              </button>
            ))}
          </div>
          <div style={{ minHeight: 220, maxHeight: 300, overflowY: "auto", border: "1px solid var(--chrome-border)", padding: "8px 10px" }}>
            {page === "about" && <AboutPage />}
            {page === "version" && <pre style={{ margin: 0, fontSize: 11, whiteSpace: "pre-wrap", userSelect: "text" }}>{text}</pre>}
            {page === "license" && (
              <div style={{ textAlign: "center", fontSize: 12, lineHeight: 1.6, paddingTop: 40 }}>
                The complete KiCad EDA Suite is released under the
                <br />
                <Link href="http://www.gnu.org/licenses">GNU General Public License (GPL) version 3 or any later version</Link>
                <br />
                <br />
                {`EDA Studio is released under the same terms (${pkg.license}).`}
                <br />
                <Link href={URL_SOURCE} />
              </div>
            )}
          </div>
        </div>
        <div className="dialog-footer">
          <button className="primary" onClick={close} autoFocus>
            Close
          </button>
        </div>
      </div>
    </div>
  );
}
