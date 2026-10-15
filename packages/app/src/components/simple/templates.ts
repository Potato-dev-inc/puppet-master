// "Make something": three tap questions, then the words PM will send, from templates that need no
// build step (a plain html file). Nothing here runs code; a template is only words.
export interface Question {
  id: 'what' | 'who' | 'look';
  ask: string;
  /** Tap answers. The first question also takes words of her own. */
  choices: string[];
  allowOther?: boolean;
}

export interface Template {
  id: 'tool' | 'page';
  title: string;
  hint: string;
  /** The default project name when she gives no words. */
  defaultName: string;
  /** The file the agent is asked to make. */
  file: string;
  questions: Question[];
  /** The words PM sends; `{what}`, `{who}`, `{look}` are her answers. */
  prompt: string;
}

/** What PM always asks, folded under "PM also asks it to". Plain words; she can read them. */
export const HOUSE_RULES: string[] = [
  'Make it as one file. Put everything in that file.',
  'It must work without the internet.',
  'Do not use other programs. Do not delete or move any files.',
  'Keep the words on the page short and simple.',
  'At the end, say what you made in 3 short sentences.',
];

export const TEMPLATES: Template[] = [
  {
    id: 'tool',
    title: 'A small tool',
    hint: 'A quiz, a calculator or a timer. One file.',
    defaultName: 'My tool',
    file: 'tool.html',
    questions: [
      { id: 'what', ask: 'What should it do?', choices: ['A quiz with 10 questions', 'A price calculator', 'A class timer', 'A list that I can tick'], allowOther: true },
      { id: 'who', ask: 'Who will use it?', choices: ['Just me', 'My students or customers', 'My team'] },
      { id: 'look', ask: 'How should it look?', choices: ['Simple and clean', 'Bright and fun', 'Big and easy to read'] },
    ],
    prompt: 'Make {what} for {who}. It should look {look}. Make it as ONE file called tool.html.',
  },
  {
    id: 'page',
    title: 'A page for my idea',
    hint: 'A web page about you, your work or your shop. One file.',
    defaultName: 'My page',
    file: 'index.html',
    questions: [
      { id: 'what', ask: 'What is the page about?', choices: ['My small shop', 'My work and my skills', 'A class or a club', 'An event'], allowOther: true },
      { id: 'who', ask: 'Who will read it?', choices: ['Just me', 'My students or customers', 'My team'] },
      { id: 'look', ask: 'How should it look?', choices: ['Simple and clean', 'Bright and fun', 'Big and easy to read'] },
    ],
    prompt: 'Make a web page about {what}, for {who}. It should look {look}. Make it as ONE file called index.html.',
  },
];

export type Answers = Partial<Record<Question['id'], string>>;

/** A choice like "My small shop" fills a sentence, so its first word loses the capital. Her own
 *  words stay as she wrote them (a name such as "Excel" keeps its capital). */
const STARTERS = new Set(['A', 'An', 'My', 'The', 'Just', 'Simple', 'Bright', 'Big']);
function lowerFirst(text: string): string {
  const first = text.split(' ')[0];
  return STARTERS.has(first) ? text[0].toLowerCase() + text.slice(1) : text;
}

/** The words she sees in the box, from her answers. She can change them. */
export function visiblePrompt(template: Template, answers: Answers): string {
  const fill = (id: Question['id']) => lowerFirst((answers[id] ?? '').trim()) || '…';
  return template.prompt
    .replace('{what}', fill('what'))
    .replace('{who}', fill('who'))
    .replace('{look}', fill('look'));
}

/** What PM sends: the words in the box, then the house rules. */
export function fullPrompt(visible: string): string {
  return `${visible.trim()} ${HOUSE_RULES.join(' ')}`;
}

/** A short project name from her first answer: "A quiz with 10 questions" -> "Quiz with 10 questions". */
export function projectName(template: Template, answers: Answers): string {
  const what = (answers.what ?? '').trim().replace(/^(an?|the|my)\s+/i, '');
  const words = what.split(/\s+/).filter(Boolean).slice(0, 4).join(' ');
  if (!words) return template.defaultName;
  return words.charAt(0).toUpperCase() + words.slice(1);
}

/** For the language test: what PM writes (the questions and the sentences), not what she types. */
export const STRINGS: string[] = [
  ...HOUSE_RULES,
  ...TEMPLATES.flatMap((t) => [
    t.title,
    t.hint,
    ...t.questions.flatMap((q) => [q.ask, ...q.choices]),
    visiblePrompt(t, { what: t.questions[0].choices[0], who: t.questions[1].choices[0], look: t.questions[2].choices[0] }),
  ]),
];
