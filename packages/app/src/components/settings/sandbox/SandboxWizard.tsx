// The setup wizard of Potato Bot's sandbox (Settings > Potato Bot > Sandbox > Set up). Steps: look, use, then coding and/or daily
// choices, a summary with the download and disk estimate, and the progress. Everything it offers comes from the recipe files
// (`transport.menu()`); the languages are pre-checked from the person's own projects (`transport.detect()`). Nothing is
// installed until the person presses Install on the summary. Monochrome, simple English.
import { useCallback, useEffect, useMemo, useRef, useState, type ReactNode } from 'react';
import { createPortal } from 'react-dom';
import {
  ALWAYS_INCLUDED,
  LOOK_LABEL,
  SIGN_IN_NOTE,
  USE_LABEL,
  answersOf,
  emptyInstall,
  estimateFor,
  formatGb,
  initialDraft,
  joinWords,
  plainSandboxError,
  type Detected,
  type Draft,
  type FeatureAsk,
  type InstallView,
  type Look,
  type Recipe,
  type RecipeOption,
  type SandboxTransport,
  type SandboxView,
} from '../../../lib/potato-sandbox';
import '../../../styles/potato-sandbox.css';
import { useSheetKeys } from '../../cockpit/Sheet';
import { ConfirmSheet } from '../controls';
import { Btn, Note } from '../settings-ui';

export type Step = 'linux' | 'look' | 'use' | 'coding' | 'daily' | 'summary' | 'progress';

const LOOKS: { id: Look; description: string }[] = [
  { id: 'ubuntu', description: 'The standard Ubuntu desktop. Plain and familiar.' },
  { id: 'zorin', description: "Ubuntu with Zorin's light theme, icons, wallpaper and font. Adds about 400 MB." },
  { id: 'light', description: 'A very small desktop. Uses the least memory and disk. Less to look at.' },
];

const optionOf = (menu: Recipe[], recipe: string, option: string): RecipeOption | undefined =>
  menu.find((r) => r.id === recipe)?.options.find((o) => o.id === option);

/** The steps to walk through for this draft, in order. */
export function stepsFor(draft: Draft): Step[] {
  return ['look', 'use', ...(draft.coding ? (['coding'] as const) : []), ...(draft.daily ? (['daily'] as const) : []), 'summary'];
}

const TITLES: Record<Step, string> = {
  linux: "Windows' Linux feature",
  look: 'How should the desktop look?',
  use: 'What will the sandbox be used for?',
  coding: 'Coding',
  daily: 'Daily use',
  summary: 'Ready to set up',
  progress: 'Setting up',
};

const PROGRESS_TITLE: Record<InstallView['state'], string> = {
  idle: 'Setting up',
  running: 'Setting up',
  done: 'All done',
  failed: 'Set up failed',
  cancelled: 'Stopped',
};

function toggle(list: string[], id: string, on: boolean): string[] {
  return on ? (list.includes(id) ? list : [...list, id]) : list.filter((x) => x !== id);
}

// ---- small pieces -----------------------------------------------------------------------------------

function Choice({
  kind,
  name,
  checked,
  onChange,
  title,
  description,
}: {
  kind: 'radio' | 'checkbox';
  name: string;
  checked: boolean;
  onChange: (on: boolean) => void;
  title: string;
  description?: string;
}) {
  return (
    <label className={`sbx-choice${checked ? ' is-on' : ''}`}>
      <input type={kind} name={name} checked={checked} onChange={(e) => onChange(e.target.checked)} />
      <span className="sbx-choice-text">
        <span className="sbx-choice-title">{title}</span>
        {description && <small>{description}</small>}
      </span>
    </label>
  );
}

function Fieldset({ legend, hint, children }: { legend: string; hint?: ReactNode; children: ReactNode }) {
  return (
    <fieldset className="sbx-fieldset">
      <legend>{legend}</legend>
      {hint && <p className="sbx-hint">{hint}</p>}
      <div className="sbx-choices">{children}</div>
    </fieldset>
  );
}

/** The bar and the step text. Also used by the Settings row while a set up runs. */
export function InstallProgress({ install }: { install: InstallView }) {
  const text = install.cancelling ? 'Stopping…' : install.detail || install.title || 'Starting…';
  return (
    <div className="sbx-progress">
      <div className="sbx-bar" role="progressbar" aria-label="Set up progress" aria-valuemin={0} aria-valuemax={100} aria-valuenow={install.percent}>
        <span style={{ width: `${install.percent}%` }} />
      </div>
      <div className="sbx-progress-line">
        <span className="sbx-step-text" role="status" aria-live="polite">
          {text}
        </span>
        <span className="sbx-percent">{install.percent}%</span>
      </div>
      {install.of > 0 && !install.cancelling && (
        <small className="sbx-step-count">
          Step {install.index} of {install.of}
          {install.title && install.detail ? `: ${install.title}` : ''}
        </small>
      )}
    </div>
  );
}

// ---- the wizard -------------------------------------------------------------------------------------

export function SandboxWizard({ transport, onClose }: { transport: SandboxTransport; onClose: () => void }) {
  const [menu, setMenu] = useState<Recipe[] | null>(null);
  const [detected, setDetected] = useState<Detected | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [status, setStatus] = useState<SandboxView | null>(null);
  const [draft, setDraft] = useState<Draft | null>(null);
  const [step, setStep] = useState<Step>('look');
  const [install, setInstall] = useState<InstallView>(emptyInstall());
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [ask, setAsk] = useState<FeatureAsk | null>(null);
  const [restart, setRestart] = useState(false);
  const [confirmHide, setConfirmHide] = useState(false);
  const started = useRef(false);

  // Read the recipes, the person's projects and the status once.
  useEffect(() => {
    let live = true;
    Promise.all([transport.menu(), transport.detect().catch(() => null), transport.status()])
      .then(([recipes, found, current]) => {
        if (!live) return;
        setMenu(recipes);
        setDetected(found);
        setStatus(current);
        setDraft(initialDraft(recipes, found));
        setInstall(current.install);
        setStep(current.install.state !== 'idle' ? 'progress' : current.linuxFeature ? 'look' : 'linux');
      })
      .catch((e) => live && setLoadError(plainSandboxError(e, 'Could not read the sandbox set up.')));
    return () => {
      live = false;
    };
  }, [transport]);

  // Follow the install while it runs.
  useEffect(() => {
    let off: (() => void) | undefined;
    let live = true;
    transport.onInstall?.((view) => live && setInstall(view)).then((un) => {
      if (live) off = un;
      else un();
    });
    return () => {
      live = false;
      off?.();
    };
  }, [transport]);

  const steps = useMemo(() => (draft ? stepsFor(draft) : []), [draft]);
  const patch = useCallback((change: Partial<Draft>) => setDraft((d) => (d ? { ...d, ...change } : d)), []);

  const close = () => {
    if (install.state !== 'running') void transport.dismissInstall().catch(() => undefined);
    onClose();
  };

  // Esc is Cancel. While a set up runs it asks first: closing only hides the window, the set up keeps going.
  const sheetRef = useSheetKeys(() => (install.state === 'running' ? setConfirmHide(true) : close()), !!ask || confirmHide);
  const covered = !!ask || confirmHide;
  useEffect(() => {
    // The sheet that sat on top is gone: take the keyboard back, so Tab and Esc work in the wizard again.
    const box = sheetRef.current;
    if (!covered && box && !box.contains(document.activeElement)) box.focus();
  }, [covered, sheetRef]);

  const go = (delta: number) => {
    setError(null);
    const at = steps.indexOf(step);
    const next = steps[at + delta];
    if (next) setStep(next);
  };

  const answers = draft ? answersOf(draft) : null;
  const estimate = answers ? estimateFor(answers) : null;

  const doInstall = async () => {
    if (!answers) return;
    setBusy(true);
    setError(null);
    try {
      await transport.install(answers);
      started.current = true;
      // The set up runs on its own thread and its events may already have come (a fast one ends before this answer does):
      // only a page that has heard nothing yet starts at the empty bar.
      setInstall((current) => (current.state === 'idle' ? { ...emptyInstall(), state: 'running' } : current));
      setStep('progress');
    } catch (e) {
      const raw = e instanceof Error ? e.message : String(e);
      if (raw.startsWith('SANDBOX_NEEDS_LINUX_FEATURE')) {
        setStatus((s) => (s ? { ...s, linuxFeature: false } : s));
        setStep('linux');
      } else {
        setError(plainSandboxError(e, 'Set up could not start.'));
      }
    } finally {
      setBusy(false);
    }
  };

  const openAsk = async () => {
    setBusy(true);
    setError(null);
    try {
      setAsk(await transport.linuxAsk());
    } catch (e) {
      setError(plainSandboxError(e, "Could not ask Windows about its Linux feature."));
    } finally {
      setBusy(false);
    }
  };

  const confirmAsk = async () => {
    const current = ask;
    setAsk(null);
    if (!current) return;
    setBusy(true);
    try {
      const done = await transport.linuxEnable(current.token);
      if (done.restartNeeded) setRestart(true);
      else {
        setStatus(await transport.status());
        setStep('look');
      }
    } catch (e) {
      setError(plainSandboxError(e, "Windows' Linux feature was not turned on."));
    } finally {
      setBusy(false);
    }
  };

  const stop = async () => {
    setError(null);
    try {
      await transport.cancelInstall();
    } catch (e) {
      setError(plainSandboxError(e, 'Could not stop set up.'));
    }
  };

  const startOver = async () => {
    await transport.dismissInstall().catch(() => undefined);
    setInstall(emptyInstall());
    setError(null);
    setStep('summary');
  };

  // ---- what each step shows ----

  const body = (): ReactNode => {
    if (loadError) return <Note role="alert">{loadError}</Note>;
    if (!menu || !draft || !status) return <p className="sbx-hint" aria-busy="true">Loading…</p>;
    switch (step) {
      case 'linux':
        return (
          <>
            <p>
              The sandbox is a small Linux computer that runs inside Windows. Windows needs its Linux feature (WSL) for that, and it is not turned on yet.
            </p>
            <p>Turning it on needs an administrator's OK and, most likely, a restart of the computer. It only happens when you press the button below.</p>
            {restart ? (
              <Note role="status">Windows' Linux feature is on. Restart the computer, then open Set up again.</Note>
            ) : (
              <div className="sbx-actions-inline">
                <Btn primary disabled={busy} onClick={() => void openAsk()}>
                  Turn on Windows' Linux feature…
                </Btn>
              </div>
            )}
          </>
        );
      case 'look':
        return (
          <Fieldset legend="Desktop look" hint={<>Always included: {ALWAYS_INCLUDED}</>}>
            {LOOKS.map((look) => (
              <Choice key={look.id} kind="radio" name="sbx-look" checked={draft.look === look.id} onChange={() => patch({ look: look.id })} title={LOOK_LABEL[look.id]} description={look.description} />
            ))}
          </Fieldset>
        );
      case 'use':
        return (
          <Fieldset legend="Use" hint="Pick one or both.">
            <Choice kind="checkbox" name="sbx-use-coding" checked={draft.coding} onChange={(on) => patch({ coding: on })} title="Coding" description="Git, Node.js, the GitHub tool, build tools, and the languages you pick next." />
            <Choice kind="checkbox" name="sbx-use-daily" checked={draft.daily} onChange={(on) => patch({ daily: on })} title="Daily use" description="LibreOffice for documents, spreadsheets and slides, plus a media player and an image viewer." />
          </Fieldset>
        );
      case 'coding': {
        const languages = optionOf(menu, 'coding', 'languages');
        const pkg = optionOf(menu, 'coding', 'pkg');
        const extras = optionOf(menu, 'coding', 'extras');
        const found = detected && detected.languages.length > 0;
        return (
          <>
            <p className="sbx-hint">Git, Node.js (current LTS), the GitHub tool and a C compiler are always included.</p>
            {languages && (
              <Fieldset
                legend="Languages"
                hint={
                  found
                    ? `Checked from your projects: ${joinWords(detected.languages.map((id) => languages.choices.find((c) => c.id === id)?.title ?? id))}. Change it if it is not right.`
                    : 'Nothing was found in your projects. Pick what you need.'
                }
              >
                {languages.choices.map((c) => (
                  <Choice key={c.id} kind="checkbox" name={`sbx-lang-${c.id}`} checked={draft.languages.includes(c.id)} onChange={(on) => patch({ languages: toggle(draft.languages, c.id, on) })} title={c.title} description={c.description} />
                ))}
              </Fieldset>
            )}
            {pkg && (
              <Fieldset legend="Package manager" hint={detected?.packageManager ? 'The one most of your projects use is picked.' : undefined}>
                {pkg.choices.map((c) => (
                  <Choice key={c.id} kind="radio" name="sbx-pkg" checked={draft.packageManager === c.id} onChange={() => patch({ packageManager: c.id })} title={c.title} description={c.description} />
                ))}
              </Fieldset>
            )}
            {extras && extras.choices.length > 0 && (
              <Fieldset legend="Extras" hint="Small tools. None are picked.">
                {extras.choices.map((c) => (
                  <Choice key={c.id} kind="checkbox" name={`sbx-extra-${c.id}`} checked={draft.extras.includes(c.id)} onChange={(on) => patch({ extras: toggle(draft.extras, c.id, on) })} title={c.title} description={c.description} />
                ))}
              </Fieldset>
            )}
          </>
        );
      }
      case 'daily': {
        const media = optionOf(menu, 'daily', 'media');
        const images = optionOf(menu, 'daily', 'images');
        return (
          <>
            <p className="sbx-hint">LibreOffice Writer, Calc and Impress are always included.</p>
            {media && (
              <Fieldset legend="Media player">
                {media.choices.map((c) => (
                  <Choice key={c.id} kind="radio" name="sbx-media" checked={draft.media === c.id} onChange={() => patch({ media: c.id })} title={c.title} description={c.description} />
                ))}
              </Fieldset>
            )}
            {images && (
              <Fieldset legend="Image viewer">
                {images.choices.map((c) => (
                  <Choice key={c.id} kind="radio" name="sbx-images" checked={draft.images === c.id} onChange={() => patch({ images: c.id })} title={c.title} description={c.description} />
                ))}
              </Fieldset>
            )}
            <Note>{SIGN_IN_NOTE}</Note>
          </>
        );
      }
      case 'summary': {
        const lines: [string, string][] = [
          ['Look', LOOK_LABEL[draft.look]],
          ['Use', [draft.coding && USE_LABEL.coding, draft.daily && USE_LABEL.daily].filter(Boolean).join(' and ')],
        ];
        if (draft.coding) {
          const titleOf = (option: string, id: string) => optionOf(menu, 'coding', option)?.choices.find((c) => c.id === id)?.title ?? id;
          lines.push(['Languages', draft.languages.length ? joinWords(draft.languages.map((id) => titleOf('languages', id))) : 'None extra']);
          lines.push(['Package manager', titleOf('pkg', draft.packageManager)]);
          if (draft.extras.length) lines.push(['Extras', joinWords(draft.extras.map((id) => titleOf('extras', id)))]);
        }
        if (draft.daily) {
          const titleOf = (option: string, id: string) => optionOf(menu, 'daily', option)?.choices.find((c) => c.id === id)?.title ?? id;
          lines.push(['Media player', titleOf('media', draft.media)], ['Image viewer', titleOf('images', draft.images)]);
        }
        return (
          <>
            <dl className="sbx-summary">
              {lines.map(([k, v]) => (
                <div key={k}>
                  <dt>{k}</dt>
                  <dd>{v}</dd>
                </div>
              ))}
            </dl>
            {estimate && (
              <dl className="sbx-summary sbx-estimate" aria-label="Estimate">
                <div>
                  <dt>Download</dt>
                  <dd>About {formatGb(estimate.downloadGb)}</dd>
                </div>
                <div>
                  <dt>Disk</dt>
                  <dd>
                    About {formatGb(estimate.diskGb)}
                    {draft.resetPoint ? `, and about ${formatGb(estimate.resetPointGb)} more for the reset point` : ''}
                  </dd>
                </div>
                <div>
                  <dt>Time</dt>
                  <dd>
                    About {estimate.minutes[0]} to {estimate.minutes[1]} minutes, depending on your internet
                  </dd>
                </div>
              </dl>
            )}
            <label className="sbx-check">
              <input type="checkbox" checked={draft.resetPoint} onChange={(e) => patch({ resetPoint: e.target.checked })} />
              <span>Save a reset point, so Reset can bring the sandbox back to how it is after set up</span>
            </label>
            <p className="sbx-hint">This makes a private mini computer inside Windows. It cannot see your files, programs or accounts.</p>
          </>
        );
      }
      case 'progress':
        return progressBody();
    }
  };

  const progressBody = (): ReactNode => {
    if (install.state === 'done') {
      return (
        <>
          <p>The sandbox is ready.</p>
          <p className="sbx-hint">It starts by itself the first time a bot needs it, and stops again when it has been idle for a while. You can change that in Settings.</p>
          {draft?.daily && <Note>{SIGN_IN_NOTE}</Note>}
        </>
      );
    }
    if (install.state === 'failed') {
      return (
        <>
          <Note role="alert">{install.error?.message ?? 'Set up did not finish.'}</Note>
          <p className="sbx-hint">Nothing is left half-made. You can try again.</p>
          {install.needsLinuxFeature && (
            <div className="sbx-actions-inline">
              <Btn onClick={() => setStep('linux')}>Windows' Linux feature…</Btn>
            </div>
          )}
        </>
      );
    }
    if (install.state === 'cancelled') {
      return <p>Set up was stopped. Nothing is left behind.</p>;
    }
    return (
      <>
        <InstallProgress install={install} />
        <p className="sbx-hint">This can take a while. You can close this window; set up keeps going.</p>
      </>
    );
  };

  const footer = (): ReactNode => {
    const at = steps.indexOf(step);
    if (loadError || !draft) return <Btn onClick={close}>Close</Btn>;
    if (step === 'linux') return <Btn onClick={close}>Close</Btn>;
    if (step === 'progress') {
      if (install.state === 'done') {
        return (
          <Btn primary onClick={close}>
            Done
          </Btn>
        );
      }
      if (install.state === 'failed' || install.state === 'cancelled') {
        return (
          <>
            <Btn onClick={close}>Close</Btn>
            <Btn primary onClick={() => void startOver()}>
              Try again
            </Btn>
          </>
        );
      }
      return (
        <>
          <Btn onClick={close}>Hide</Btn>
          <Btn disabled={install.cancelling} onClick={() => void stop()}>
            {install.cancelling ? 'Stopping…' : 'Stop set up'}
          </Btn>
        </>
      );
    }
    return (
      <>
        <Btn onClick={close}>Cancel</Btn>
        {at > 0 && <Btn onClick={() => go(-1)}>Back</Btn>}
        {step === 'summary' ? (
          <Btn primary disabled={busy} onClick={() => void doInstall()}>
            Install
          </Btn>
        ) : (
          <Btn primary disabled={step === 'use' && !draft.coding && !draft.daily} onClick={() => go(1)}>
            Next
          </Btn>
        )}
      </>
    );
  };

  const position = steps.indexOf(step);

  // In a portal on the settings window itself (a fixed layer over the whole window), not inside the section card: a card
  // clips and scrolls what it holds. The settings tokens live on `.settings`, so the portal goes there, not to the body.
  const root = document.querySelector('.settings') ?? document.body;
  return createPortal(
    <>
      <div
        className="st-scrim sbx-scrim"
        onMouseDown={(e) => {
          if (e.target === e.currentTarget && install.state !== 'running') close();
        }}
      >
        <div ref={sheetRef} tabIndex={-1} className="st-sheet sbx-sheet" role="dialog" aria-modal="true" aria-labelledby="sbx-title" data-step={step}>
          <header className="sbx-head">
            <h2 id="sbx-title">Set up the sandbox</h2>
            {position >= 0 && steps.length > 0 && (
              <small className="sbx-count" aria-label={`Step ${position + 1} of ${steps.length}`}>
                {position + 1} / {steps.length}
              </small>
            )}
          </header>
          <div className="sbx-body">
            {menu && draft && <h3 className="sbx-sub">{step === 'progress' ? PROGRESS_TITLE[install.state] : TITLES[step]}</h3>}
            {body()}
            {error && <Note role="alert">{error}</Note>}
          </div>
          <div className="st-sheet-actions sbx-actions">{footer()}</div>
        </div>
      </div>
      {ask && <ConfirmSheet title={ask.title} body={ask.body} confirmLabel="Turn on" onConfirm={() => void confirmAsk()} onCancel={() => setAsk(null)} />}
      {confirmHide && (
        <ConfirmSheet
          title="Set up is still running"
          body="Closing this window does not stop it. Set up keeps going, and you can open it again from the Sandbox section."
          confirmLabel="Close window"
          onConfirm={() => {
            setConfirmHide(false);
            close();
          }}
          onCancel={() => setConfirmHide(false)}
        />
      )}
    </>,
    root,
  );
}
