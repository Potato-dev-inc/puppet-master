// Settings > Potato Bot > Sandbox: the status, Set up, Start, Stop, Reset, Remove and the idle-stop minutes. The rows are shown only
// while Labs has the sandbox on (`labsFlag`); the Labs switch itself is a row of General > Labs (collected there by `labs`).
// Reset asks first; Remove needs the word "remove" typed; the Windows' Linux feature button opens a confirm dialog and nothing
// runs until its button is pressed.
import { useEffect, useRef, useState } from 'react';
import {
  REMOVE_WORD,
  STATE_WORD,
  formatSize,
  plainSandboxError,
  tauriSandboxTransport,
  type FeatureAsk,
  type SandboxTransport,
  type SandboxView,
} from '../../../lib/potato-sandbox';
import '../../../styles/potato-sandbox.css';
import { ConfirmSheet, NumberControl } from '../controls';
import type { RowDef, SettingsEnv } from '../model';
import { Btn, Note, SettingBlock, SettingRow, SettingToggle } from '../settings-ui';
import { InstallProgress, SandboxWizard } from './SandboxWizard';
import { useSandbox } from './useSandbox';

export const MIN_IDLE = 1;
export const MAX_IDLE = 240;

/** What the sandbox is, in the words under its name. */
export const SANDBOX_ABOUT = 'A small private Linux computer the bot can use, with its own browser and files. It cannot see your files, programs or accounts.';

/** The sheet that asks for the word "remove". The button stays off until it is typed. */
export function RemoveSheet({ busy, onConfirm, onCancel }: { busy?: boolean; onConfirm: () => void; onCancel: () => void }) {
  const [typed, setTyped] = useState('');
  const input = useRef<HTMLInputElement>(null);
  useEffect(() => input.current?.focus(), []);
  const ok = typed.trim().toLowerCase() === REMOVE_WORD;
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
      <div className="st-sheet" role="dialog" aria-modal="true" aria-labelledby="sbx-remove-title">
        <h2 id="sbx-remove-title">Remove the sandbox?</h2>
        <p>Everything in it is deleted: its files, its sign-ins and its reset point. You can set it up again later. To go on, type the word {REMOVE_WORD}.</p>
        <input
          ref={input}
          className="st-field sbx-typed"
          aria-label={`Type ${REMOVE_WORD} to confirm`}
          value={typed}
          autoComplete="off"
          spellCheck={false}
          onChange={(e) => setTyped(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === 'Enter' && ok && !busy) onConfirm();
          }}
        />
        <div className="st-sheet-actions">
          <Btn onClick={onCancel}>Cancel</Btn>
          <Btn primary disabled={!ok || busy} onClick={onConfirm}>
            Remove sandbox
          </Btn>
        </div>
      </div>
    </div>
  );
}

function statusLine(view: SandboxView): string {
  const parts: string[] = [STATE_WORD[view.state]];
  if (view.reason && view.state !== 'installing') parts.push(view.reason);
  return parts.join('. ').replace(/\.\./g, '.');
}

function SandboxPanel({ transport, env }: { transport: SandboxTransport; env: SettingsEnv }) {
  const { view, failed, refresh } = useSandbox(transport);
  const [wizard, setWizard] = useState(false);
  const [removing, setRemoving] = useState(false);
  const [ask, setAsk] = useState<FeatureAsk | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [message, setMessage] = useState<{ ok: boolean; text: string } | null>(null);

  const run = async (what: string, call: () => Promise<unknown>, done?: string) => {
    setBusy(what);
    setMessage(null);
    try {
      await call();
      if (done) setMessage({ ok: true, text: done });
    } catch (error) {
      setMessage({ ok: false, text: plainSandboxError(error, 'That did not work.') });
    } finally {
      setBusy(null);
      await refresh();
    }
  };

  const openAsk = async () => {
    setMessage(null);
    try {
      setAsk(await transport.linuxAsk());
    } catch (error) {
      setMessage({ ok: false, text: plainSandboxError(error, "Could not ask Windows about its Linux feature.") });
    }
  };

  const confirmAsk = async () => {
    const current = ask;
    setAsk(null);
    if (!current) return;
    await run(
      'linux',
      async () => {
        const done = await transport.linuxEnable(current.token);
        setMessage({ ok: true, text: done.restartNeeded ? "Windows' Linux feature is on. Restart the computer, then set up the sandbox." : "Windows' Linux feature is on." });
      },
    );
  };

  const reset = async () => {
    const yes = await env.confirm({
      title: 'Reset the sandbox?',
      body: 'It goes back to how it was at the end of set up. Everything done in it since is lost, including sign-ins.',
      confirmLabel: 'Reset',
    });
    if (yes) await run('reset', () => transport.reset(), 'The sandbox is back to its reset point.');
  };

  const remove = async () => {
    setBusy('remove');
    setMessage(null);
    try {
      await transport.remove(REMOVE_WORD);
      setRemoving(false);
      setMessage({ ok: true, text: 'The sandbox was removed.' });
    } catch (error) {
      setMessage({ ok: false, text: plainSandboxError(error, 'Could not remove the sandbox.') });
    } finally {
      setBusy(null);
      await refresh();
    }
  };

  if (!view) {
    return (
      <SettingBlock label="Sandbox" description={SANDBOX_ABOUT}>
        {failed ? <Note role="alert">{failed}</Note> : <span className="st-value" aria-busy="true">Loading…</span>}
      </SettingBlock>
    );
  }

  const installed = view.state === 'stopped' || view.state === 'running' || view.state === 'starting' || view.state === 'failed';
  const setting = view.state === 'installing';

  return (
    <SettingBlock label="Sandbox" description={SANDBOX_ABOUT}>
      <div className="sbx-status">
        <div className="sbx-status-line">
          <span className="sbx-state" data-state={view.state}>
            {statusLine(view)}
          </span>
          {installed && view.diskBytes !== null && <small>Uses {formatSize(view.diskBytes)} on this computer</small>}
          {installed && view.resetPointBytes !== null && <small>Reset point: {formatSize(view.resetPointBytes)}</small>}
        </div>

        {setting && (
          <>
            <InstallProgress install={view.install} />
            <div className="sbx-buttons">
              <Btn onClick={() => setWizard(true)}>Show</Btn>
              <Btn disabled={view.install.cancelling} onClick={() => void run('cancel', () => transport.cancelInstall())}>
                {view.install.cancelling ? 'Stopping…' : 'Stop set up'}
              </Btn>
            </div>
          </>
        )}

        {view.state === 'not_installed' && (
          <div className="sbx-buttons">
            {!view.linuxFeature && (
              <>
                <Note>Needs Windows' Linux feature (admin + restart).</Note>
                <Btn disabled={busy !== null} onClick={() => void openAsk()}>
                  Turn on Windows' Linux feature…
                </Btn>
              </>
            )}
            <Btn primary disabled={!view.linuxFeature} onClick={() => setWizard(true)}>
              Set up…
            </Btn>
          </div>
        )}

        {installed && (
          <div className="sbx-buttons">
            {view.state === 'running' ? (
              <Btn disabled={busy !== null} onClick={() => void run('stop', () => transport.stop())}>
                {busy === 'stop' ? 'Stopping…' : 'Stop'}
              </Btn>
            ) : (
              <Btn primary disabled={busy !== null || view.state === 'starting'} onClick={() => void run('start', () => transport.start())}>
                {busy === 'start' || view.state === 'starting' ? 'Starting…' : view.state === 'failed' ? 'Start again' : 'Start'}
              </Btn>
            )}
            <Btn disabled={busy !== null || !view.hasResetPoint} title={view.hasResetPoint ? undefined : 'No reset point was saved at set up.'} onClick={() => void reset()}>
              {busy === 'reset' ? 'Resetting…' : 'Reset…'}
            </Btn>
            <Btn disabled={busy !== null} onClick={() => setRemoving(true)}>
              Remove…
            </Btn>
          </div>
        )}

        {message && <Note role={message.ok ? 'status' : 'alert'}>{message.text}</Note>}
      </div>

      {wizard && (
        <SandboxWizard
          transport={transport}
          onClose={() => {
            setWizard(false);
            void refresh();
          }}
        />
      )}
      {removing && <RemoveSheet busy={busy === 'remove'} onConfirm={() => void remove()} onCancel={() => setRemoving(false)} />}
      {ask && <ConfirmSheet title={ask.title} body={ask.body} confirmLabel="Turn on" onConfirm={() => void confirmAsk()} onCancel={() => setAsk(null)} />}
    </SettingBlock>
  );
}

function IdleRow({ transport, env }: { transport: SandboxTransport; env: SettingsEnv }) {
  const { view } = useSandbox(transport, { poll: false });
  const [minutes, setMinutes] = useState<number | null>(null);
  const shown = minutes ?? view?.idleMinutes ?? 15;
  return (
    <SettingRow label="Stop when idle" description="The sandbox stops by itself after this many minutes of not being used. It starts again when a bot needs it.">
      <NumberControl
        aria-label="Stop the sandbox after minutes idle"
        value={shown}
        min={MIN_IDLE}
        max={MAX_IDLE}
        suffix="minutes"
        disabled={!view}
        onCommit={async (next) => {
          env.setError(null);
          try {
            setMinutes(await transport.setIdle(next));
            env.markSaved();
          } catch (error) {
            env.setError(plainSandboxError(error, 'Could not save that.'));
          }
        }}
      />
    </SettingRow>
  );
}

const KEYWORDS = 'sandbox linux wsl vm virtual computer desktop ubuntu zorin chrome coding daily';

export function makeSandboxRows(transport: SandboxTransport = tauriSandboxTransport): RowDef[] {
  return [
    {
      id: 'potato-sandbox-labs',
      label: 'Sandbox',
      hint: "A small private Linux computer a Potato Bot can use. Turn it on to see its settings under Potato Bot. Needs Potato Bot on too.",
      keywords: `${KEYWORDS} labs early`,
      group: 'Labs',
      labs: { name: 'Sandbox', about: "A small private Linux computer the bot can use, away from your files." },
      render: (env) => (
        <SettingToggle
          label="Sandbox"
          description="Set up a private Linux computer inside Windows for Potato Bot. It cannot see your files. Its settings appear under Potato Bot when this is on. It does nothing until you set it up."
          checked={env.settings.potato_sandbox_enabled ?? false}
          onChange={(value) => void env.patch({ potato_sandbox_enabled: value })}
        />
      ),
    },
    {
      id: 'potato-sandbox',
      label: 'Sandbox',
      hint: 'Set up, start, stop, reset or remove the small Linux computer a bot can use.',
      keywords: `${KEYWORDS} set up install start stop reset remove wizard disk`,
      group: 'Sandbox',
      labsFlag: 'potato_sandbox_enabled',
      render: (env) => <SandboxPanel transport={transport} env={env} />,
    },
    {
      id: 'potato-sandbox-idle',
      label: 'Stop when idle',
      hint: 'How many minutes the sandbox may sit unused before it stops by itself.',
      keywords: `${KEYWORDS} idle minutes timeout`,
      group: 'Sandbox',
      labsFlag: 'potato_sandbox_enabled',
      render: (env) => <IdleRow transport={transport} env={env} />,
    },
  ];
}
