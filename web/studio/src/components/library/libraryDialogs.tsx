// Small modal questions the library actions ask (a name for Rename, "Delete this symbol?", "Overwrite?"). The
// actions are plain async functions run from the action registry, outside any React tree, so a question is a request on
// this module-level bus; `LibraryDialogHost` -- mounted by the two library editors' views -- shows the oldest one and
// resolves the action's promise with the answer. With no host mounted (neither editor on screen) a question is answered
// "no" at once rather than left hanging.
import { useEffect, useRef, useState } from "react";

export interface PromptOptions {
  title: string;
  label: string;
  initial?: string;
  okLabel?: string;
  /** Returns an error message to keep the dialog open on, or `null` to accept. */
  validate?: (value: string) => string | null;
  note?: string;
}

export interface ConfirmOptions {
  title: string;
  message: string;
  okLabel?: string;
  cancelLabel?: string;
}

type Request = { id: number } & ({ kind: "prompt"; options: PromptOptions; resolve: (v: string | null) => void } | { kind: "confirm"; options: ConfirmOptions; resolve: (v: boolean) => void });

let nextId = 1;
let sink: ((r: Request) => void) | null = null;

/** Ask for a line of text; `null` when cancelled. */
export function askText(options: PromptOptions): Promise<string | null> {
  return new Promise((resolve) => {
    if (!sink) return resolve(null);
    sink({ id: nextId++, kind: "prompt", options, resolve });
  });
}

/** Ask a yes/no question. */
export function askConfirm(options: ConfirmOptions): Promise<boolean> {
  return new Promise((resolve) => {
    if (!sink) return resolve(false);
    sink({ id: nextId++, kind: "confirm", options, resolve });
  });
}

export function LibraryDialogHost() {
  const [queue, setQueue] = useState<Request[]>([]);
  const queueRef = useRef<Request[]>([]);
  queueRef.current = queue;
  useEffect(() => {
    sink = (r) => {
      queueRef.current = [...queueRef.current, r];
      setQueue(queueRef.current);
    };
    return () => {
      sink = null;
      // A host that goes away answers its unanswered questions "no" (an action must never wait forever on a dialog nobody can see).
      for (const r of queueRef.current) {
        if (r.kind === "prompt") r.resolve(null);
        else r.resolve(false);
      }
      queueRef.current = [];
    };
  }, []);
  const top = queue[0];
  if (!top) return null;
  const finish = () => {
    queueRef.current = queueRef.current.slice(1);
    setQueue(queueRef.current);
  };
  return top.kind === "prompt" ? (
    <PromptDialog
      key={top.id}
      options={top.options}
      onDone={(v) => {
        top.resolve(v);
        finish();
      }}
    />
  ) : (
    <ConfirmDialog
      key={top.id}
      options={top.options}
      onDone={(v) => {
        top.resolve(v);
        finish();
      }}
    />
  );
}

function PromptDialog({ options, onDone }: { options: PromptOptions; onDone: (v: string | null) => void }) {
  const [value, setValue] = useState(options.initial ?? "");
  const [error, setError] = useState<string | null>(null);
  const ref = useRef<HTMLInputElement>(null);
  useEffect(() => {
    ref.current?.focus();
    ref.current?.select();
  }, []);
  const submit = () => {
    const msg = options.validate?.(value) ?? null;
    if (msg) return setError(msg);
    onDone(value);
  };
  return (
    <div className="dialog-backdrop" onClick={() => onDone(null)}>
      <div className="dialog" style={{ width: 420 }} onClick={(e) => e.stopPropagation()} role="dialog" aria-label={options.title}>
        <div className="dialog-header">
          <span>{options.title}</span>
        </div>
        <div className="dialog-body">
          <label style={{ display: "block", marginBottom: 6 }}>{options.label}</label>
          <input
            ref={ref}
            value={value}
            style={{ width: "100%", boxSizing: "border-box" }}
            onChange={(e) => {
              setValue(e.target.value);
              setError(null);
            }}
            onKeyDown={(e) => {
              if (e.key === "Enter") submit();
              if (e.key === "Escape") onDone(null);
            }}
          />
          {options.note && <p style={{ color: "var(--chrome-text-dim)", fontSize: 11, margin: "8px 0 0" }}>{options.note}</p>}
          {error && <p style={{ color: "var(--chrome-danger)", margin: "8px 0 0" }}>{error}</p>}
        </div>
        <div className="dialog-footer">
          <button onClick={() => onDone(null)}>Cancel</button>
          <button className="primary" onClick={submit}>
            {options.okLabel ?? "OK"}
          </button>
        </div>
      </div>
    </div>
  );
}

function ConfirmDialog({ options, onDone }: { options: ConfirmOptions; onDone: (v: boolean) => void }) {
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onDone(false);
    };
    document.addEventListener("keydown", onKey, true);
    return () => document.removeEventListener("keydown", onKey, true);
  }, [onDone]);
  return (
    <div className="dialog-backdrop" onClick={() => onDone(false)}>
      <div className="dialog" style={{ width: 440 }} onClick={(e) => e.stopPropagation()} role="dialog" aria-label={options.title}>
        <div className="dialog-header">
          <span>{options.title}</span>
        </div>
        <div className="dialog-body" style={{ whiteSpace: "pre-wrap" }}>
          {options.message}
        </div>
        <div className="dialog-footer">
          <button onClick={() => onDone(false)}>{options.cancelLabel ?? "Cancel"}</button>
          <button className="primary" autoFocus onClick={() => onDone(true)}>
            {options.okLabel ?? "OK"}
          </button>
        </div>
      </div>
    </div>
  );
}
