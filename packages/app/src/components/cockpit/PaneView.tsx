import { useCallback, useEffect, useRef, useState } from 'react';
import type { CockpitApi } from '../../hooks/useCockpit';
import { cockpitApi, type PaneRunInfo, type PaneTranscript } from '../../lib/cockpit-api';
import { projectName } from '../../lib/mobile-agent-tabs';
import { TerminalViewport } from '../TerminalViewport';
import { DotSpinner } from './DotSpinner';
import { IconClose, IconCopy, IconMore, IconReflow, IconStop } from './Icons';
import { formatElapsed, promptSummary, receiptRows, spinnerStatus, stateText } from './model';
import { SegTabs } from './SegTabs';

type Tab = 'terminal' | 'transcript' | 'receipt';

/** Run info + transcript for the Transcript and Receipt tabs. `null` = the pane has no run. */
function useRunData(paneId: string, active: boolean, status: string) {
  const [info, setInfo] = useState<PaneRunInfo | null | undefined>(undefined);
  const [transcript, setTranscript] = useState<PaneTranscript | null | undefined>(undefined);
  const [error, setError] = useState<string | null>(null);
  const [nonce, setNonce] = useState(0);

  useEffect(() => {
    setInfo(undefined);
    setTranscript(undefined);
    setError(null);
  }, [paneId]);

  useEffect(() => {
    if (!active) return;
    let cancelled = false;
    Promise.all([cockpitApi.paneRunInfo(paneId), cockpitApi.paneTranscript(paneId)])
      .then(([i, t]) => {
        if (cancelled) return;
        setInfo(i);
        setTranscript(t);
        setError(null);
      })
      .catch((err) => !cancelled && setError(err instanceof Error ? err.message : String(err)));
    return () => {
      cancelled = true;
    };
  }, [paneId, active, status, nonce]);

  return { info, transcript, error, refresh: () => setNonce((n) => n + 1) };
}

function Empty({ title, body }: { title: string; body: string }) {
  return (
    <div className="ck-empty">
      <h2 className="ck-section">{title}</h2>
      <p className="ck-sub">{body}</p>
    </div>
  );
}

export function PaneView({ c, paneId }: { c: CockpitApi; paneId: string }) {
  const pane = c.registry.panes.get(paneId);
  const [tab, setTab] = useState<Tab>('terminal');
  const [menu, setMenu] = useState(false);
  const [confirm, setConfirm] = useState(false);
  const [reflowKey, setReflowKey] = useState(0);
  const menuRef = useRef<HTMLDivElement>(null);
  const getSelectionRef = useRef<(() => string) | null>(null);
  const run = useRunData(paneId, tab !== 'terminal', pane?.status ?? '');

  useEffect(() => {
    setTab('terminal');
    setMenu(false);
    setConfirm(false);
  }, [paneId]);

  useEffect(() => {
    if (!menu) return;
    const away = (e: MouseEvent) => {
      if (!menuRef.current?.contains(e.target as Node)) setMenu(false);
    };
    const esc = (e: KeyboardEvent) => e.key === 'Escape' && setMenu(false);
    window.addEventListener('mousedown', away);
    window.addEventListener('keydown', esc);
    return () => {
      window.removeEventListener('mousedown', away);
      window.removeEventListener('keydown', esc);
    };
  }, [menu]);

  const copySelection = useCallback(async () => {
    const text = tab === 'terminal' ? (getSelectionRef.current?.() ?? '') : (window.getSelection()?.toString() ?? '');
    if (!text) {
      c.notify('Nothing selected');
      return;
    }
    try {
      await navigator.clipboard.writeText(text);
      c.notify('Copied');
    } catch {
      c.notify('Copy failed');
    }
  }, [c, tab]);

  if (!pane) {
    return (
      <div className="ck-spawn">
        <h1 className="ck-title">Opening pane</h1>
        <p className="ck-sub">Waiting for the backend to list it.</p>
      </div>
    );
  }

  const { info } = pane;
  const since = c.statusSince.get(paneId);
  const prompt = c.prompts[paneId];
  const name = c.names.get(paneId) ?? info.agent_type;
  const stateLine =
    pane.status === 'waiting_input' && since ? `Needs you · ${formatElapsed(c.now - since)}` : stateText(pane.status);

  return (
    <div className="ck-pane">
      <header className="ck-pane-head">
        <div className="ck-pane-id">
          <h1 className="ck-title ck-title--sm">{name}</h1>
          <span className="ck-sub-inline">{projectName(info.cwd)}</span>
        </div>
        <div className="ck-pane-state">
          <DotSpinner status={spinnerStatus(pane.status)} />
          <span>{stateLine}</span>
        </div>
        <span className="ck-grid-size">
          {info.cols}×{info.rows}
        </span>
        <div className="ck-menu-wrap" ref={menuRef}>
          <button
            type="button"
            className="ck-icon-btn"
            aria-label="Pane actions"
            aria-haspopup="menu"
            aria-expanded={menu}
            onClick={() => setMenu((v) => !v)}
          >
            <IconMore />
          </button>
          {menu && (
            <div className="ck-menu" role="menu">
              <button
                type="button"
                role="menuitem"
                className="ck-menu-item"
                onClick={() => {
                  setMenu(false);
                  setTab('terminal');
                  setReflowKey((k) => k + 1);
                }}
              >
                <span className="ck-menu-ic"><IconReflow /></span>
                <span>Reflow</span>
              </button>
              <button
                type="button"
                role="menuitem"
                className="ck-menu-item"
                onClick={() => {
                  setMenu(false);
                  void copySelection();
                }}
              >
                <span className="ck-menu-ic"><IconCopy /></span>
                <span>Copy selection</span>
              </button>
              <button
                type="button"
                role="menuitem"
                className="ck-menu-item"
                onClick={() => {
                  setMenu(false);
                  void c.interrupt(paneId);
                }}
              >
                <span className="ck-menu-ic"><IconStop /></span>
                <span>Interrupt</span>
                <kbd className="ck-kbd">⌃C</kbd>
              </button>
              <div className="ck-menu-sep" />
              <button
                type="button"
                role="menuitem"
                className="ck-menu-item"
                onClick={() => {
                  setMenu(false);
                  if (pane.status === 'running') setConfirm(true);
                  else void c.closePane(paneId);
                }}
              >
                <span className="ck-menu-ic"><IconClose /></span>
                <span>Close</span>
              </button>
            </div>
          )}
        </div>
      </header>

      {pane.status === 'waiting_input' && prompt && prompt.choices.length > 0 && (
        <div className="ck-ask" role="group" aria-label="Agent question">
          <span className="ck-ask-q">{promptSummary(prompt)}</span>
          <span className="ck-ask-actions">
            {prompt.choices.map((ch, i) => (
              <button
                key={ch.id}
                type="button"
                className={`ck-btn ck-btn--sm ${i === 0 ? 'ck-btn--primary' : 'ck-btn--secondary'}`}
                onClick={() => void c.answer(paneId, prompt.promptId, ch.id, ch.label)}
              >
                {ch.label}
              </button>
            ))}
          </span>
        </div>
      )}

      <div className="ck-tabs-bar">
        <SegTabs
          label="Pane view"
          value={tab}
          onChange={setTab}
          options={[
            { id: 'terminal', label: 'Terminal' },
            { id: 'transcript', label: 'Transcript' },
            { id: 'receipt', label: 'Receipt' },
          ]}
        />
      </div>

      <div className="ck-pane-body">
        {tab === 'terminal' && (
          <div className="ck-term-slot" key={paneId}>
            <TerminalViewport pane={pane} registry={c.registry} chrome={false}
              reflowKey={reflowKey}
              getSelectionRef={getSelectionRef}
            />
          </div>
        )}
        {tab === 'transcript' && (
          <div className="ck-scroll ck-fade-in" key="transcript">
            {run.error ? (
              <Empty title="Transcript unavailable" body={run.error} />
            ) : run.transcript === undefined ? (
              <p className="ck-sub ck-pad">Loading…</p>
            ) : run.transcript === null || run.transcript.chunks.length === 0 ? (
              <Empty
                title="No transcript"
                body={
                  run.info === null
                    ? 'This pane has no recorded run, so there is nothing to show. The terminal tab has the live session.'
                    : 'The run has not produced any output yet.'
                }
              />
            ) : (
              <div className="ck-transcript">
                {run.transcript.chunks.map((chunk, i) => (
                  <div key={i} className="ck-chunk">
                    <span className="ck-chunk-stream">{chunk.stream}</span>
                    <pre>{chunk.text}</pre>
                  </div>
                ))}
              </div>
            )}
          </div>
        )}
        {tab === 'receipt' && (
          <div className="ck-scroll ck-fade-in" key="receipt">
            {run.error ? (
              <Empty title="Receipt unavailable" body={run.error} />
            ) : run.info === undefined ? (
              <p className="ck-sub ck-pad">Loading…</p>
            ) : run.info === null ? (
              <Empty
                title="No run receipt"
                body="This pane was not started by a delegated run, so there is no receipt. Receipts appear for runs started from the orchestrator or MCP."
              />
            ) : (
              <div className="ck-receipt">
                <dl className="ck-kv">
                  <dt>Handle</dt>
                  <dd>{run.info.handle}</dd>
                  <dt>Status</dt>
                  <dd>{run.info.status}</dd>
                </dl>
                {run.info.receipt === null || run.info.receipt === undefined ? (
                  <p className="ck-sub">No receipt yet. It is written when the turn settles.</p>
                ) : (
                  <dl className="ck-kv">
                    {receiptRows(run.info.receipt).map((r) => (
                      <div className="ck-kv-row" key={r.key}>
                        <dt>{r.key}</dt>
                        <dd>{r.value}</dd>
                      </div>
                    ))}
                  </dl>
                )}
              </div>
            )}
            <button type="button" className="ck-btn ck-btn--plain ck-pad" onClick={run.refresh}>
              Refresh
            </button>
          </div>
        )}
      </div>

      {confirm && (
        <div className="ck-scrim" onMouseDown={() => setConfirm(false)}>
          <div className="ck-sheet" role="alertdialog" aria-labelledby="ck-close-t" onMouseDown={(e) => e.stopPropagation()}>
            <h2 className="ck-section" id="ck-close-t">
              Close {name}?
            </h2>
            <p className="ck-sub">It is still running. Closing ends the process and its session.</p>
            <div className="ck-sheet-actions">
              <button type="button" className="ck-btn ck-btn--secondary" autoFocus onClick={() => setConfirm(false)}>
                Keep running
              </button>
              <button
                type="button"
                className="ck-btn ck-btn--primary"
                onClick={() => {
                  setConfirm(false);
                  void c.closePane(paneId);
                }}
              >
                Close pane
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
