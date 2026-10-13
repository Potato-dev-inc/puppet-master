// Autosaving controls. Each one owns its draft and decides WHEN to save; the row decides WHAT to save.
//
//   Switch, Popup (select)  save immediately on change.
//   TextControl             saves after a pause in typing (mode "debounce"), or only on blur ("blur").
//   NumberControl           clamps and saves on blur / Enter (never while you type).
//   SecretControl           saves on blur / Enter only, then clears itself; the value is never echoed.
//
// All of them flush a pending edit when they unmount, so leaving a section or closing the window with
// text still in a field does not lose it.
import { useCallback, useEffect, useRef, useState, type InputHTMLAttributes, type ReactNode, type SelectHTMLAttributes } from 'react';

export const DEBOUNCE_MS = 600;

/** A pending save that can be scheduled, flushed (run now) or cancelled. Flushes on unmount. */
function useCommitter(run: () => void, delayMs: number) {
  const runRef = useRef(run);
  runRef.current = run;
  const timer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const pending = useRef(false);

  const cancel = useCallback(() => {
    if (timer.current) clearTimeout(timer.current);
    timer.current = null;
  }, []);
  const flush = useCallback(() => {
    cancel();
    if (!pending.current) return;
    pending.current = false;
    runRef.current();
  }, [cancel]);
  const schedule = useCallback(() => {
    pending.current = true;
    cancel();
    timer.current = setTimeout(flush, delayMs);
  }, [cancel, delayMs, flush]);
  const mark = useCallback(() => {
    pending.current = true;
  }, []);
  useEffect(() => flush, [flush]);
  return { schedule, flush, cancel, mark, isPending: () => pending.current };
}

export function Switch({
  checked,
  onChange,
  label,
  disabled,
}: {
  checked: boolean;
  onChange: (next: boolean) => void;
  label: string;
  disabled?: boolean;
}) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={checked}
      aria-label={label}
      disabled={disabled}
      className={`st-switch${checked ? ' is-on' : ''}`}
      onClick={() => onChange(!checked)}
    >
      <i />
    </button>
  );
}

/** A popup button (native select, styled). Saves the moment you pick. */
export function Popup(props: SelectHTMLAttributes<HTMLSelectElement>) {
  const { className, ...rest } = props;
  return <select {...rest} className={`st-popup${className ? ` ${className}` : ''}`} />;
}

type InputProps = Omit<InputHTMLAttributes<HTMLInputElement>, 'value' | 'onChange' | 'onBlur' | 'type'>;

export function TextControl({
  value,
  onCommit,
  mode = 'debounce',
  debounceMs = DEBOUNCE_MS,
  normalize = (s: string) => s,
  mono,
  type = 'text',
  className,
  ...rest
}: InputProps & {
  value: string;
  onCommit: (next: string) => void | Promise<void>;
  /** "debounce": save after a pause in typing and on blur. "blur": save only on blur / Enter. */
  mode?: 'debounce' | 'blur';
  debounceMs?: number;
  /** Applied to the text before it is saved, for example trim. */
  normalize?: (text: string) => string;
  mono?: boolean;
  type?: 'text' | 'url' | 'search';
}) {
  const [draft, setDraft] = useState(value);
  const dirty = useRef(false);
  const draftRef = useRef(draft);
  draftRef.current = draft;
  const valueRef = useRef(value);
  valueRef.current = value;

  useEffect(() => {
    if (!dirty.current) setDraft(value);
  }, [value]);

  const committer = useCommitter(() => {
    dirty.current = false;
    const next = normalize(draftRef.current);
    if (next !== valueRef.current) void onCommit(next);
  }, debounceMs);

  return (
    <input
      {...rest}
      type={type}
      value={draft}
      spellCheck={false}
      className={`st-field${mono ? ' is-mono' : ''}${className ? ` ${className}` : ''}`}
      onChange={(e) => {
        dirty.current = true;
        setDraft(e.target.value);
        draftRef.current = e.target.value;
        if (mode === 'debounce') committer.schedule();
        else committer.mark();
      }}
      onBlur={() => committer.flush()}
      onKeyDown={(e) => {
        if (e.key === 'Enter') committer.flush();
      }}
    />
  );
}

export function NumberControl({
  value,
  onCommit,
  min,
  max,
  step = 1,
  normalize,
  suffix,
  className,
  ...rest
}: InputProps & {
  value: number;
  onCommit: (next: number) => void | Promise<void>;
  min?: number;
  max?: number;
  step?: number;
  /** Custom clamp. Defaults to rounding and keeping the value between min and max. */
  normalize?: (n: number) => number;
  suffix?: ReactNode;
}) {
  const [draft, setDraft] = useState(String(value));
  const dirty = useRef(false);
  const draftRef = useRef(draft);
  draftRef.current = draft;
  const valueRef = useRef(value);
  valueRef.current = value;

  useEffect(() => {
    if (!dirty.current) setDraft(String(value));
  }, [value]);

  const clamp = (n: number) => {
    if (normalize) return normalize(n);
    let out = Math.round(n);
    if (min !== undefined) out = Math.max(min, out);
    if (max !== undefined) out = Math.min(max, out);
    return out;
  };

  const committer = useCommitter(() => {
    dirty.current = false;
    const parsed = Number(draftRef.current);
    if (draftRef.current.trim() === '' || !Number.isFinite(parsed)) {
      setDraft(String(valueRef.current)); // not a number: put the saved value back
      return;
    }
    const next = clamp(parsed);
    setDraft(String(next)); // show what was actually saved
    if (next !== valueRef.current) void onCommit(next);
  }, 0);

  return (
    <span className="st-number">
      <input
        {...rest}
        type="number"
        inputMode="numeric"
        min={min}
        max={max}
        step={step}
        value={draft}
        className={`st-field is-mono is-number${className ? ` ${className}` : ''}`}
        onChange={(e) => {
          dirty.current = true;
          setDraft(e.target.value);
          draftRef.current = e.target.value;
          committer.mark();
        }}
        onBlur={() => committer.flush()}
        onKeyDown={(e) => {
          if (e.key === 'Enter') committer.flush();
        }}
      />
      {suffix && <span className="st-suffix">{suffix}</span>}
    </span>
  );
}

/** A password-style field for keys and tokens. Saves on blur / Enter, then clears; never shows the value. */
export function SecretControl({
  onCommit,
  className,
  ...rest
}: InputProps & { onCommit: (secret: string) => void | Promise<void> }) {
  const [draft, setDraft] = useState('');
  const draftRef = useRef('');
  const committer = useCommitter(() => {
    const secret = draftRef.current.trim();
    draftRef.current = '';
    setDraft('');
    if (secret) void onCommit(secret);
  }, 0);

  return (
    <input
      {...rest}
      type="password"
      autoComplete="off"
      spellCheck={false}
      value={draft}
      className={`st-field is-mono${className ? ` ${className}` : ''}`}
      onChange={(e) => {
        draftRef.current = e.target.value;
        setDraft(e.target.value);
        committer.mark();
      }}
      onBlur={() => committer.flush()}
      onKeyDown={(e) => {
        if (e.key === 'Enter') committer.flush();
      }}
    />
  );
}

/** The in-app confirm sheet for destructive actions. Cancel has focus, so Enter never destroys anything. */
export function ConfirmSheet({
  title,
  body,
  confirmLabel,
  onConfirm,
  onCancel,
}: {
  title: string;
  body: string;
  confirmLabel: string;
  onConfirm: () => void;
  onCancel: () => void;
}) {
  const cancelRef = useRef<HTMLButtonElement>(null);
  useEffect(() => {
    cancelRef.current?.focus();
  }, []);
  return (
    <div
      className="st-scrim"
      onMouseDown={(e) => {
        if (e.target === e.currentTarget) onCancel();
      }}
      onKeyDown={(e) => {
        if (e.key === 'Escape') onCancel();
      }}
    >
      <div className="st-sheet" role="dialog" aria-modal="true" aria-labelledby="st-sheet-title">
        <h2 id="st-sheet-title">{title}</h2>
        <p>{body}</p>
        <div className="st-sheet-actions">
          <button ref={cancelRef} type="button" className="st-btn" onClick={onCancel}>
            Cancel
          </button>
          <button type="button" className="st-btn is-primary" onClick={onConfirm}>
            {confirmLabel}
          </button>
        </div>
      </div>
    </div>
  );
}
