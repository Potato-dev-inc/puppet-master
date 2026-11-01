// @vitest-environment jsdom
// The starter picker: six cards from the data, "Make this bot" runs the create, add, save and routine calls, Blank
// makes a plain bot, and a computer-needing card shows its note and the computer's set-up button.
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { fakeCompanionTransport } from '../../../lib/companion';
import { fakePotatoTransport } from '../../../lib/potato';
import { fakeBotSettingsTransport } from '../../../lib/potato-botsettings';
import { fakeRoutinesTransport } from '../../../lib/potato-routines';
import { STARTERS, type StarterTransports } from '../../../lib/potato-starters';
import { StarterPicker } from './StarterPicker';
import { buttonByText } from './test-helpers';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let host: HTMLDivElement;
let root: Root;
beforeEach(() => {
  host = document.createElement('div');
  document.body.appendChild(host);
  root = createRoot(host);
});
afterEach(() => {
  act(() => root.unmount());
  host.remove();
});

function setup(over: { onSetUpComputer?: () => void } = {}) {
  const companions = fakeCompanionTransport();
  const potato = fakePotatoTransport({ bots: [] });
  const settings = fakeBotSettingsTransport();
  const routines = fakeRoutinesTransport();
  const transports: StarterTransports = { companions, potato, settings, routines };
  const spies = {
    create: vi.spyOn(companions, 'create'),
    addBot: vi.spyOn(potato, 'addBot'),
    saveInstructions: vi.spyOn(settings, 'saveInstructions'),
    setTools: vi.spyOn(settings, 'setTools'),
    routineCreate: vi.spyOn(routines, 'create'),
  };
  const onMade = vi.fn();
  const onSetUpByHand = vi.fn();
  const onClose = vi.fn();
  return { spies, onMade, onSetUpByHand, onClose, transports, ...over };
}

const cardButton = (id: string) => host.querySelector<HTMLButtonElement>(`article[data-starter="${id}"] button.ck-potato-starter-make`)!;

async function render(props: ReturnType<typeof setup>) {
  await act(async () =>
    root.render(
      <StarterPicker
        transports={props.transports}
        onMade={props.onMade}
        onSetUpByHand={props.onSetUpByHand}
        onClose={props.onClose}
        onSetUpComputer={props.onSetUpComputer}
      />,
    ),
  );
}

describe('StarterPicker', () => {
  it('renders six cards from the data, each with its one-sentence description', async () => {
    await render(setup());
    const cards = host.querySelectorAll('article.ck-potato-starter');
    expect(cards).toHaveLength(6);
    expect([...cards].map((c) => c.querySelector('h3')?.textContent)).toEqual(STARTERS.map((s) => s.name));
    expect(host.textContent).toContain(STARTERS[1].doing);
  });

  it('picking a card creates the companion with the template fields, adds the bot and makes its routine', async () => {
    const props = setup();
    await render(props);
    await act(async () => cardButton('inbox-helper').click());
    await act(async () => {});
    const inbox = STARTERS[0];
    expect(props.spies.create).toHaveBeenCalledWith(expect.objectContaining({ name: inbox.name, avatar: inbox.avatar, persona: 'plain' }));
    expect(props.spies.addBot).toHaveBeenCalledTimes(1);
    expect(props.spies.saveInstructions).toHaveBeenCalledWith(expect.any(String), inbox.instructions);
    expect(props.spies.routineCreate).toHaveBeenCalledWith(expect.any(String), expect.objectContaining({ title: inbox.routine?.title }));
    expect(props.onMade).toHaveBeenCalledTimes(1);
  });

  it('Blank creates a plain bot with no instructions, tools or routine', async () => {
    const props = setup();
    await render(props);
    await act(async () => cardButton('blank').click());
    await act(async () => {});
    expect(props.spies.create).toHaveBeenCalledWith(expect.objectContaining({ name: 'Blank' }));
    expect(props.spies.saveInstructions).not.toHaveBeenCalled();
    expect(props.spies.setTools).not.toHaveBeenCalled();
    expect(props.spies.routineCreate).not.toHaveBeenCalled();
    expect(props.onMade).toHaveBeenCalledTimes(1);
  });

  it('a computer-needing card shows its note and the computer set-up button', async () => {
    const onSetUpComputer = vi.fn();
    await render(setup({ onSetUpComputer }));
    const research = host.querySelector('article[data-starter="research-buddy"]') as HTMLElement;
    expect(research.textContent).toContain("Needs Fish's computer");
    await act(async () => buttonByText(research, 'Set up the computer')!.click());
    expect(onSetUpComputer).toHaveBeenCalledTimes(1);
    const inbox = host.querySelector('article[data-starter="inbox-helper"]') as HTMLElement;
    expect(inbox.textContent).not.toContain("Needs Fish's computer");
  });

  it('"Set up by hand" opens the old sheet path', async () => {
    const props = setup();
    await render(props);
    await act(async () => buttonByText(host, 'Set up by hand')!.click());
    expect(props.onSetUpByHand).toHaveBeenCalledTimes(1);
  });
});
