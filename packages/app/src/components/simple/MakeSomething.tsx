// "Make something": three tap questions, then she reads the words PM will send, then Start.
// No blank prompt, no build step: the templates make one html file. The task always runs on a copy
// (Rust decides that; nothing here can change it).
import { useState } from 'react';
import { agentLabel, useAgentRegistry } from '../../lib/agentRegistry';
import { DotSpinner } from '../cockpit/DotSpinner';
import { pickAgent } from './agent-pick';
import { F, T } from './copy';
import { errorCode, simpleApi, type SimpleApi, type Started } from './simple-api';
import { fullPrompt, HOUSE_RULES, projectName, TEMPLATES, visiblePrompt, type Answers, type Template } from './templates';

type Step = 'kind' | 0 | 1 | 2 | 'see';

/** What each Rust refusal says to her, in her words. */
export function refusalText(code: string | null): string {
  switch (code) {
    case 'FOLDER_TOO_BROAD':
    case 'FOLDER_TOO_BIG':
      return T.makeFolderTooBroad;
    case 'FOLDER_MISSING':
      return T.makeFolderMissing;
    case 'FOLDER_NEEDS_SETUP':
      return T.makeFolderNeedsSetup;
    default:
      return T.makeFailed;
  }
}

export function MakeSomething({
  onStarted,
  onCancel,
  onOpenSettings,
  api = simpleApi,
}: {
  onStarted: (started: Started) => void;
  onCancel: () => void;
  onOpenSettings: () => void;
  api?: Pick<SimpleApi, 'start' | 'checkFolder' | 'pickFolder'>;
}) {
  const listing = useAgentRegistry();
  const agent = pickAgent(listing);
  const agentName = agent ? agentLabel(listing, agent.id) : '';
  const [step, setStep] = useState<Step>('kind');
  const [template, setTemplate] = useState<Template | null>(null);
  const [answers, setAnswers] = useState<Answers>({});
  const [other, setOther] = useState('');
  const [text, setText] = useState('');
  const [folder, setFolder] = useState<{ path: string; name: string } | null>(null);
  const [problem, setProblem] = useState<string | null>(null);
  const [starting, setStarting] = useState(false);

  const questions = template?.questions ?? [];
  const number = typeof step === 'number' ? step : null;

  const go = (next: Step, nextAnswers: Answers = answers) => {
    if (next === 'see' && template) setText(visiblePrompt(template, nextAnswers));
    setStep(next);
  };

  const answer = (value: string) => {
    if (number === null) return;
    const next = { ...answers, [questions[number].id]: value };
    setAnswers(next);
    setOther('');
    go(number === 2 ? 'see' : ((number + 1) as Step), next);
  };

  const back = () => {
    setProblem(null);
    if (step === 'kind') return onCancel();
    if (step === 'see') return setStep(2);
    if (step === 0) return setStep('kind');
    if (typeof step === 'number') setStep((step - 1) as Step);
  };

  const chooseFolder = async () => {
    setProblem(null);
    try {
      const picked = await api.pickFolder();
      if (!picked) return;
      const check = await api.checkFolder(picked.path);
      if (check.ok) setFolder(picked);
      else {
        setFolder(null);
        setProblem(refusalText(check.code));
      }
    } catch {
      setProblem(T.makeFailed);
    }
  };

  const start = async () => {
    if (!template || !agent) return;
    setStarting(true);
    setProblem(null);
    try {
      const started = await api.start({
        name: folder ? folder.name : projectName(template, answers),
        prompt: fullPrompt(text),
        agent: agent.id,
        ...(folder ? { existing_folder: folder.path } : {}),
      });
      onStarted(started);
    } catch (err) {
      setProblem(refusalText(errorCode(err)));
      setStarting(false);
    }
  };

  if (!agent && step === 'kind') {
    return (
      <div className="sx-page sx-make">
        <h1 className="ck-title">{T.makeTitle}</h1>
        <p className="sx-lead" data-testid="no-agent">{T.makeNoAgent}</p>
        <div className="sx-actions">
          <button type="button" className="ck-btn ck-btn--secondary" onClick={onCancel}>
            {T.makeBack}
          </button>
          <button type="button" className="ck-btn ck-btn--primary" onClick={onOpenSettings}>
            {T.makeAddAgent}
          </button>
        </div>
      </div>
    );
  }

  return (
    <div className="sx-page sx-make">
      <h1 className="ck-title">{T.makeTitle}</h1>

      {step === 'kind' && (
        <>
          <p className="sx-lead">{T.makeLead}</p>
          <p className="sx-ask">{T.makeChooseKind}</p>
          <ul className="sx-choices">
            {TEMPLATES.map((t) => (
              <li key={t.id}>
                <button
                  type="button"
                  className="sx-choice"
                  onClick={() => {
                    setTemplate(t);
                    setAnswers({});
                    setStep(0);
                  }}
                >
                  <span className="sx-choice-title">{t.title}</span>
                  <span className="sx-choice-sub">{t.hint}</span>
                </button>
              </li>
            ))}
          </ul>
        </>
      )}

      {number !== null && template && (
        <>
          <p className="sx-quiet">{F.stepOf(number + 1)}</p>
          <p className="sx-ask">{questions[number].ask}</p>
          <ul className="sx-choices">
            {questions[number].choices.map((choice) => (
              <li key={choice}>
                <button type="button" className="sx-choice" onClick={() => answer(choice)}>
                  <span className="sx-choice-title">{choice}</span>
                </button>
              </li>
            ))}
          </ul>
          {questions[number].allowOther && (
            <div className="sx-other">
              <input
                className="sx-input"
                aria-label={T.makeOther}
                placeholder={T.makeOtherPlaceholder}
                value={other}
                onChange={(e) => setOther(e.target.value)}
                onKeyDown={(e) => e.key === 'Enter' && other.trim() && answer(other.trim())}
              />
              <button type="button" className="ck-btn ck-btn--secondary" disabled={!other.trim()} onClick={() => answer(other.trim())}>
                {T.makeNext}
              </button>
            </div>
          )}
        </>
      )}

      {step === 'see' && template && (
        <>
          <p className="sx-ask">{F.askingAgent(agentName)}</p>
          <p className="sx-quiet">{T.makeSeeLead}</p>
          <textarea className="sx-input sx-textarea" aria-label={T.makeSeeTitle} value={text} onChange={(e) => setText(e.target.value)} rows={4} />
          <details className="sx-rules">
            <summary>{T.makeRulesTitle}</summary>
            <ul>
              {HOUSE_RULES.map((r) => (
                <li key={r}>{r}</li>
              ))}
            </ul>
          </details>
          <p className="sx-note">{folder ? T.makeFolderOk : T.copyNote}</p>
          {folder ? (
            <button type="button" className="sx-link" onClick={() => setFolder(null)}>
              {T.makeNewFolder}
            </button>
          ) : (
            <button type="button" className="sx-link" onClick={() => void chooseFolder()}>
              {T.makeFolder}
            </button>
          )}
        </>
      )}

      {problem && (
        <p className="sx-problem" role="alert">
          {problem}
        </p>
      )}

      <div className="sx-actions">
        <button type="button" className="ck-btn ck-btn--plain" onClick={back} disabled={starting}>
          {T.makeBack}
        </button>
        {step === 'see' && (
          <button type="button" className="ck-btn ck-btn--primary" disabled={starting || !text.trim()} onClick={() => void start()}>
            {starting ? (
              <>
                <span aria-hidden="true">
                  <DotSpinner status="running" />
                </span>{' '}
                {T.makeStarting}
              </>
            ) : (
              T.makeStart
            )}
          </button>
        )}
      </div>
    </div>
  );
}
