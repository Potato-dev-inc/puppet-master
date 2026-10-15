// The short steps of the welcome: How do you work, Use your phone, Add Puppet Master to your agents, Done.
import { useEffect, useState } from 'react';
import type { Settings } from '@puppet-master/shared';
import { agentPackApi, groupAgents, visibleRows, type AgentPackApi } from '../../lib/agent-pack';
import type { loadSettings, patchSettings } from '../../lib/settings';
import { parseDevServerPort } from '../../lib/public-bridge-url';
import { DotSpinner } from '../cockpit/DotSpinner';
import { MobilePairingPanel } from '../MobilePairingPanel';
import { PackSheet } from '../settings/pack/AgentPackGroup';
import { T } from './copy';
import { isSimpleLook, LOOKS, type Look } from './model';

export function StepWelcome({ look, onLook }: { look: Look | null; onLook: (look: Look) => void }) {
  return (
    <section className="fr-step fr-step--welcome" aria-labelledby="fr-welcome-t">
      <div className="fr-hero" aria-hidden="true">
        <DotSpinner status="idle" size={5} />
      </div>
      <h1 className="ck-title" id="fr-welcome-t">{T.title}</h1>
      <p className="fr-lead">{T.welcomeLead}</p>
      <h2 className="ck-section fr-ask-q" id="fr-how">{T.howDoYouWork}</h2>
      <div className="fr-choices" role="radiogroup" aria-labelledby="fr-how">
        {LOOKS.map((id) => (
          <button key={id} type="button" role="radio" aria-checked={look === id} className="fr-choice" onClick={() => onLook(id)}>
            <span className="fr-radio" aria-hidden="true" />
            <span className="fr-choice-text">
              <span className="fr-choice-label">{T.looks[id].label}</span>
              <span className="fr-choice-hint">{T.looks[id].hint}</span>
            </span>
          </button>
        ))}
      </div>
      {look !== null && (
        <p className="fr-note" role="status" data-testid="look-note">
          {isSimpleLook(look) ? T.noteSimple : T.noteCode}
        </p>
      )}
    </section>
  );
}

export function StepPhone({ load, patch }: { load: typeof loadSettings; patch: typeof patchSettings }) {
  const [settings, setSettings] = useState<Settings | null>(null);
  useEffect(() => {
    let alive = true;
    load()
      .then((s) => alive && setSettings(s))
      .catch(() => undefined);
    return () => {
      alive = false;
    };
  }, [load]);
  const save = (next: Partial<Settings>) => {
    setSettings((s) => (s ? { ...s, ...next } : s));
    void patch(next as never).catch(() => undefined);
  };
  return (
    <section className="fr-step" aria-labelledby="fr-phone-t">
      <div className="ck-doto fr-num" aria-hidden="true">05</div>
      <h1 className="ck-title" id="fr-phone-t">{T.phoneTitle}</h1>
      <p className="fr-lead">{T.phoneLead}</p>
      <p className="fr-hint">{T.phoneNote}</p>
      {settings && (
        <div className="settings fr-settings-scope">
          <MobilePairingPanel
            compact
            publicPwaUrl={settings.public_pwa_url ?? ''}
            devServerPort={parseDevServerPort(settings.dev_server_port)}
            onPublicPwaUrlChange={(value) => save({ public_pwa_url: value.trim() || undefined })}
            onDevServerPortChange={(port) => save({ dev_server_port: port })}
          />
        </div>
      )}
    </section>
  );
}

/** Which agents the pack would reach, as the Settings group says it. */
export function useFound(api: AgentPackApi): string[] | null {
  const [found, setFound] = useState<string[] | null>(null);
  useEffect(() => {
    let alive = true;
    api
      .preview()
      .then((r) => alive && setFound(visibleRows(groupAgents(r.agents)).filter((x) => x.installed).map((x) => x.labels.join(', '))))
      .catch(() => alive && setFound([]));
    return () => {
      alive = false;
    };
  }, [api]);
  return found;
}

export function StepPack({ api = agentPackApi }: { api?: AgentPackApi }) {
  const found = useFound(api);
  const [reviewing, setReviewing] = useState(false);
  const [notice, setNotice] = useState<string | null>(null);
  return (
    <section className="fr-step" aria-labelledby="fr-pack-t">
      <div className="ck-doto fr-num" aria-hidden="true">06</div>
      <h1 className="ck-title" id="fr-pack-t">{T.packTitle}</h1>
      <p className="fr-lead">{T.packLead}</p>
      <div className="fr-card">
        <p className="fr-card-line" data-testid="pack-found">{found === null ? T.looking : T.packFound(found)}</p>
        {notice && <p className="fr-hint" role="status">{notice}</p>}
        <div className="fr-actions">
          <button type="button" className="ck-btn ck-btn--secondary" onClick={() => setReviewing(true)}>
            {T.packReview}
          </button>
        </div>
      </div>
      {reviewing && (
        <div className="settings fr-settings-scope">
          <PackSheet
            api={api}
            onClose={() => setReviewing(false)}
            onDone={(text) => {
              setReviewing(false);
              setNotice(text);
            }}
          />
        </div>
      )}
    </section>
  );
}

export function StepDone() {
  const tips = [
    { term: T.tipKeys, body: T.tipKeysBody, kbd: true },
    { term: T.tipInbox, body: T.tipInboxBody, kbd: false },
    { term: T.tipHistory, body: T.tipHistoryBody, kbd: false },
  ];
  return (
    <section className="fr-step" aria-labelledby="fr-done-t">
      <div className="fr-hero" aria-hidden="true">
        <DotSpinner status="exited" size={5} />
      </div>
      <h1 className="ck-title" id="fr-done-t">{T.doneTitle}</h1>
      <p className="fr-lead">{T.doneLead}</p>
      <ol className="fr-tips">
        {tips.map((tip, i) => (
          <li key={tip.term} className="fr-tipr">
            <span className="ck-doto fr-tipnum" aria-hidden="true">{String(i + 1).padStart(2, '0')}</span>
            <span className="fr-tipmain">
              {tip.kbd ? <kbd className="fr-kbd">{tip.term}</kbd> : <strong>{tip.term}</strong>}
              <span className="fr-tipbody">{tip.body}</span>
            </span>
          </li>
        ))}
      </ol>
    </section>
  );
}
