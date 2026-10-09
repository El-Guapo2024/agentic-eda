// The description pane under a chooser's preview (`GenerateAliasInfo`, `GenerateFootprintInfo`): the name in bold, the description with its links, the
// keywords and a small table of the fields. Text, not HTML: nothing a library file holds is ever injected as markup.
import type { ReactNode } from "react";

const URL_RE = /(https?:\/\/[^\s"'<>)]+[^\s"'<>).,;:])/g;

/** A text with its web addresses as links (`LinkifyHTML`). */
export function Linkified({ text, max = 0 }: { text: string; max?: number }) {
  const parts: ReactNode[] = [];
  let last = 0;
  for (const m of text.matchAll(URL_RE)) {
    const at = m.index ?? 0;
    if (at > last) parts.push(text.slice(last, at));
    const url = m[0];
    const shown = max > 0 && url.length > max ? `${url.slice(0, max - 3)}...` : url;
    parts.push(
      <a key={at} href={url} target="_blank" rel="noreferrer">
        {shown}
      </a>
    );
    last = at + url.length;
  }
  if (last < text.length) parts.push(text.slice(last));
  return <>{parts}</>;
}

export interface DetailRow {
  name: string;
  value: ReactNode;
}

export function ChooserDetails({ name, derivedFrom, description, rows }: { name: string; derivedFrom?: string | null; description: string; rows: DetailRow[] }) {
  return (
    <div className="chooser-details" data-chooser-details>
      <div className="name">{name}</div>
      {derivedFrom && <div style={{ fontStyle: "italic" }}>Derived from {derivedFrom}</div>}
      {description && (
        <div>
          <Linkified text={description} />
        </div>
      )}
      {rows.length > 0 && (
        <>
          <hr style={{ border: 0, borderTop: "1px solid var(--chrome-border)", margin: "6px 0 2px" }} />
          <table>
            <tbody>
              {rows.map((r) => (
                <tr key={r.name}>
                  <td>{r.name}</td>
                  <td>{r.value}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </>
      )}
    </div>
  );
}
