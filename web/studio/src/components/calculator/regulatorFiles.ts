// Where the Regulators panel's data files live in a browser. KiCad reads and writes the file named by `regulators.data_file` (`ReadDataFile` /
// `WriteDataFile`, datafile_read_write.cpp); a page has no file system, so this browser's local storage stands in for it: key
// "eda-studio.pcb_calculator.regulators" holds { <file name>: <the file's text, KiCad's *.pcbcalc s-expression> }. A file opened with Browse is copied
// in; a write stores the text and also hands it to the browser as a download, so the person gets the real `.pcbcalc` file KiCad would have written.

const STORAGE_KEY = "eda-studio.pcb_calculator.regulators";

function readAll(): Record<string, string> {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    const v = raw ? (JSON.parse(raw) as unknown) : null;
    if (v && typeof v === "object" && !Array.isArray(v)) {
      const out: Record<string, string> = {};
      for (const [k, t] of Object.entries(v as Record<string, unknown>)) if (typeof t === "string") out[k] = t;
      return out;
    }
  } catch {
    // no storage, or a broken entry
  }
  return {};
}

/** The stored text of a data file, or null when there is none (`wxFopen` fails). */
export function readRegulatorFile(name: string): string | null {
  if (!name) return null;
  return readAll()[name] ?? null;
}

/** Stores a data file's text; false when the browser refuses (`FILE_OUTPUTFORMATTER` throwing). */
export function storeRegulatorFile(name: string, text: string): boolean {
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify({ ...readAll(), [name]: text }));
    return true;
  } catch {
    return false;
  }
}

/** The file to the browser's downloads (the copy on disk KiCad's write leaves). */
export function downloadRegulatorFile(name: string, text: string): void {
  const base = name.split(/[\\/]/).pop() || name;
  const url = URL.createObjectURL(new Blob([text], { type: "text/plain" }));
  const a = document.createElement("a");
  a.href = url;
  a.download = base;
  document.body.appendChild(a);
  a.click();
  a.remove();
  setTimeout(() => URL.revokeObjectURL(url), 1000);
}
