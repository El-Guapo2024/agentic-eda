// Edit Options... of a library table row: `DIALOG_PLUGIN_OPTIONS` (common/dialogs/dialog_plugin_options.cpp, 8303b2ad), opened from the row's popup menu
// (`LIB_TABLE_GRID_TRICKS`: one row selected, not a chained table) or a double click on its Options cell (`handleDoubleClick`). Titled "Options for Library '%s'": the
// "Plugin Options" grid (Option, Value; add a row, delete rows) on the left and the "Option Choices" the library's format offers on the right
// (`GetLibraryOptions`), with the help of the one selected and "<<    Append Selected Option" (a double click on a choice does the same: it fills the first row
// with no name, else a new one). OK writes the grid back as the row's options (`TransferDataFromWindow`: trimmed, rows with no name dropped, `FormatOptions`);
// the table is written when the Manage Libraries dialog's OK is.
//
// Differs from KiCad 8303b2ad: its `TransferDataToWindow` puts every option of the row into the grid's first row (the loop never advances its row counter), so
// a row with two options opens showing one and OK keeps only that one; here each option has its row.
import { useState } from "react";
import { libraryOptionChoices, OPTIONS_INITIAL_HELP, optionsFromGrid, parseOptions, type TableKind } from "../../kicad-port/libraryTable";

interface Props {
  kind: TableKind;
  nickname: string;
  type: string;
  options: string;
  onOk: (options: string) => void;
  onCancel: () => void;
}

/** The plugin's help is KiCad's own markup: `<b>` is all it uses. */
function Help({ text }: { text: string }) {
  const parts = text.split(/(<b>.*?<\/b>)/g);
  return (
    <>
      {parts.map((p, i) => (p.startsWith("<b>") ? <b key={i}>{p.slice(3, -4)}</b> : <span key={i}>{p}</span>))}
    </>
  );
}

export function PluginOptionsDialog({ kind, nickname, type, options, onOk, onCancel }: Props) {
  const choices = libraryOptionChoices(kind, type);
  const [rows, setRows] = useState<{ name: string; value: string }[]>(() => {
    const parsed = parseOptions(options).map(([name, value]) => ({ name, value }));
    return parsed.length > 0 ? parsed : [{ name: "", value: "" }];
  });
  const [picked, setPicked] = useState<number>(-1);
  const [choice, setChoice] = useState<string | null>(null);
  const help = choice ? (choices.find((c) => c.name === choice)?.help ?? OPTIONS_INITIAL_HELP) : OPTIONS_INITIAL_HELP;

  /** `appendOption`: the choice into the first row with no name, or a new row. */
  const append = (name: string) =>
    setRows((now) => {
      const at = now.findIndex((r) => r.name === "");
      if (at >= 0) return now.map((r, i) => (i === at ? { ...r, name } : r));
      return [...now, { name, value: "" }];
    });

  return (
    <div className="dialog-backdrop" onClick={(e) => e.stopPropagation()}>
      <div
        className="dialog lt-options"
        role="dialog"
        aria-label={`Options for Library '${nickname}'`}
        onClick={(e) => e.stopPropagation()}
        onKeyDown={(e) => {
          if (e.key === "Escape") {
            e.stopPropagation();
            onCancel();
          }
        }}
      >
        <div className="dialog-header">
          <span>{`Options for Library '${nickname}'`}</span>
        </div>
        <div className="dialog-body lt-options-body">
          <fieldset className="lt-options-grid">
            <legend>Plugin Options</legend>
            <table className="lt-vars" aria-label="Plugin options">
              <thead>
                <tr>
                  <th style={{ width: "40%" }}>Option</th>
                  <th>Value</th>
                </tr>
              </thead>
              <tbody>
                {rows.map((r, i) => (
                  <tr key={i} className={picked === i ? "selected" : undefined} onMouseDown={() => setPicked(i)}>
                    <td>
                      <input
                        type="text"
                        aria-label={`Option ${i + 1}`}
                        value={r.name}
                        spellCheck={false}
                        onChange={(e) => setRows((now) => now.map((x, at) => (at === i ? { ...x, name: e.target.value } : x)))}
                      />
                    </td>
                    <td>
                      <input
                        type="text"
                        aria-label={`Value of option ${i + 1}`}
                        value={r.value}
                        spellCheck={false}
                        onChange={(e) => setRows((now) => now.map((x, at) => (at === i ? { ...x, value: e.target.value } : x)))}
                      />
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
            <div className="lt-toolbar">
              <button title="Add row" aria-label="Add option row" onClick={() => setRows((now) => [...now, { name: "", value: "" }])}>
                {"+"}
              </button>
              <button
                title="Delete row"
                aria-label="Delete option row"
                disabled={picked < 0}
                onClick={() => {
                  setRows((now) => now.filter((_, i) => i !== picked));
                  setPicked(-1);
                }}
              >
                {"🗑"}
              </button>
            </div>
          </fieldset>
          <fieldset className="lt-options-choices">
            <legend>Option Choices</legend>
            <select size={6} aria-label="Option choices" title="Options supported by current plugin" value={choice ?? ""} onChange={(e) => setChoice(e.target.value)} onDoubleClick={() => choice && append(choice)}>
              {choices.map((c) => (
                <option key={c.name} value={c.name}>
                  {c.name}
                </option>
              ))}
            </select>
            <button disabled={!choice} onClick={() => choice && append(choice)}>
              {"<<    Append Selected Option"}
            </button>
            <div className="lt-options-help">
              <Help text={help} />
            </div>
          </fieldset>
        </div>
        <div className="dialog-footer">
          <button onClick={onCancel}>Cancel</button>
          <button className="primary" onClick={() => onOk(optionsFromGrid(rows))}>
            OK
          </button>
        </div>
      </div>
    </div>
  );
}
