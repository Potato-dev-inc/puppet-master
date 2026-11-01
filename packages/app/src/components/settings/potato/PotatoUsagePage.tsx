// Settings > Potato Bot > Usage (a sub-page): messages, tokens and cost for the last 30 days, per bot and in total, and
// today against each bot's daily limit and sandbox cap. Plain bars made of divs, no chart library. Read only.
import { useEffect, useState } from 'react';
import { dollars } from '../../../lib/potato';
import {
  barPercent,
  dayLabel,
  limitPercent,
  limitWord,
  moneyText,
  NANO_PER_CENT,
  tauriUsageTransport,
  tokenText,
  usageError,
  type PotatoUsageTransport,
  type UsageDay,
  type UsageReport,
  type UsageTotals,
} from '../../../lib/potato-usage';
import { Note } from '../settings-ui';

/** A labelled bar: the track is the whole, the fill is the part that is used. */
export function Meter({ label, value, top, text, hint }: { label: string; value: number; top: number; text: string; hint?: string }) {
  const percent = barPercent(value, top);
  return (
    <div className="pot-meter">
      <div className="pot-meter-text">
        <span>{label}</span>
        <span>{text}</span>
      </div>
      <div className="pot-bar" role="meter" aria-label={label} aria-valuemin={0} aria-valuemax={100} aria-valuenow={Math.round(percent)}>
        <span className="pot-bar-fill" style={{ width: `${percent}%` }} />
      </div>
      {hint && <small className="pot-meter-hint">{hint}</small>}
    </div>
  );
}

/** One day: its name, a bar against the top day of its list, and the cost and message count. */
function DayRow({ day, today, top }: { day: UsageDay; today: string; top: number }) {
  return (
    <li className="pot-day">
      <span className="pot-day-date">{dayLabel(day.date, today)}</span>
      <div className="pot-bar" role="presentation">
        <span className="pot-bar-fill" style={{ width: `${barPercent(day.costNano, top)}%` }} />
      </div>
      <span className="pot-day-figures">
        {moneyText(day.costNano)} · {day.messages} {day.messages === 1 ? 'message' : 'messages'}
      </span>
    </li>
  );
}

function Stats({ totals }: { totals: UsageTotals }) {
  return (
    <div className="pot-stats">
      <div className="pot-stat">
        <small>Messages</small>
        <b>{totals.messages}</b>
      </div>
      <div className="pot-stat">
        <small>Tokens</small>
        <b>{tokenText(totals.tokens)}</b>
      </div>
      <div className="pot-stat">
        <small>Cost</small>
        <b>{moneyText(totals.costNano)}</b>
      </div>
    </div>
  );
}

export function UsagePage({ transport = tauriUsageTransport }: { transport?: PotatoUsageTransport }) {
  const [report, setReport] = useState<UsageReport | null>(null);
  const [failed, setFailed] = useState<string | null>(null);

  useEffect(() => {
    let live = true;
    transport
      .usage()
      .then((found) => {
        if (live) setReport(found);
      })
      .catch((error) => {
        if (live) setFailed(usageError(error));
      });
    return () => {
      live = false;
    };
  }, [transport]);

  if (failed) return <Note role="alert">{failed}</Note>;
  if (!report) return <span className="pot-sub-loading" aria-busy="true">Loading…</span>;

  const limitNano = report.limitCents * NANO_PER_CENT;
  const topBotCost = Math.max(0, ...report.bots.map((bot) => bot.total.costNano));
  const topDayCost = Math.max(0, ...report.days.map((day) => day.costNano));
  const newestFirst = [...report.days].reverse();

  return (
    <div className="pot-usage">
      <section className="pot-sub-section" aria-labelledby="pot-usage-today">
        <h3 id="pot-usage-today">Today</h3>
        {report.bots.length === 0 && <p>No bots yet.</p>}
        {report.bots.map((bot) => {
          const spent = limitPercent(bot.today.costNano, report.limitCents);
          const word = limitWord(spent);
          return (
            <div key={bot.id} className="pot-today-bot">
              <Meter
                label={bot.name}
                value={bot.today.costNano}
                top={limitNano}
                text={`${moneyText(bot.today.costNano)} of ${dollars(report.limitCents)}`}
                hint={word || `Daily limit: ${dollars(report.limitCents)} a day.`}
              />
              <Meter
                label={`${bot.name} sandbox actions`}
                value={bot.today.sandboxActions}
                top={report.sandboxActionsPerDay}
                text={`${bot.today.sandboxActions} of ${report.sandboxActionsPerDay}`}
                hint="Clicks, typing and screen pictures on its computer."
              />
            </div>
          );
        })}
        <p className="pot-sub-note">Costs are estimates from the prices Puppet Master knows. Your provider&apos;s bill is the real one.</p>
      </section>

      <section className="pot-sub-section" aria-labelledby="pot-usage-30">
        <h3 id="pot-usage-30">Last 30 days</h3>
        <Stats totals={report.total} />
      </section>

      <section className="pot-sub-section" aria-labelledby="pot-usage-bots">
        <h3 id="pot-usage-bots">By bot</h3>
        {report.bots.length === 0 && <p>No bots yet.</p>}
        {report.bots.map((bot) => {
          const topBotDay = Math.max(0, ...bot.days.map((day) => day.costNano));
          return (
            <div key={bot.id} className="pot-bot-row">
              <div className="pot-bot-head">
                <span className="pot-bot-name">{bot.name}</span>
                <span className="pot-bot-figures">
                  {bot.total.messages} messages · {tokenText(bot.total.tokens)} tokens · {moneyText(bot.total.costNano)}
                </span>
              </div>
              <div className="pot-bar" role="meter" aria-label={`${bot.name} cost, share of the top bot`} aria-valuemin={0} aria-valuemax={100} aria-valuenow={Math.round(barPercent(bot.total.costNano, topBotCost))}>
                <span className="pot-bar-fill" style={{ width: `${barPercent(bot.total.costNano, topBotCost)}%` }} />
              </div>
              <details className="pot-bot-days">
                <summary>Each day</summary>
                <ol className="pot-day-list" aria-label={`${bot.name} each day, newest first`}>
                  {[...bot.days].reverse().map((day) => (
                    <DayRow key={day.date} day={day} today={report.today} top={topBotDay} />
                  ))}
                </ol>
              </details>
            </div>
          );
        })}
      </section>

      <section className="pot-sub-section" aria-labelledby="pot-usage-days">
        <h3 id="pot-usage-days">Each day</h3>
        <ol className="pot-day-list" aria-label="Each day, newest first">
          {newestFirst.map((day) => (
            <DayRow key={day.date} day={day} today={report.today} top={topDayCost} />
          ))}
        </ol>
      </section>
      {report.todayTotal.messages === 0 && report.total.messages === 0 && <p className="pot-sub-note">Nothing used in the last 30 days.</p>}
    </div>
  );
}

