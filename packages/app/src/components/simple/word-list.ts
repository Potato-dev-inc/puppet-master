// The fixed list of words Simple mode may use. It is a curated list of everyday words (about the
// A1 to B1 level of English) plus the product words below. It is NOT a copy of a published word
// list: a new word is added here on purpose, in a code review, only when it is simple and there is
// no simpler one. `language.test.ts` fails any Simple string that uses a word that is not here.
//
// Product words: names of things in PM and of the agents (never translated).
const PRODUCT = 'pm claude code codex html';

const FUNCTION_WORDS =
  'a an the this that these those it its i you your yours we our they them their he she his her ' +
  'and or but if so then than because while when until before after again also only just not no yes ' +
  'to of in on at by for from with without into out up down over under about as like more most less ' +
  'is are was were be been being am do does did done doing have has had having can could will would ' +
  'may might must should shall cannot can\'t don\'t doesn\'t didn\'t won\'t isn\'t aren\'t wasn\'t ' +
  'there here where what which who whom whose why how any some all each every both either neither ' +
  'one two three four five six seven eight nine ten first second last next new old own other another ' +
  'too very much many few little long short big small same different right left ' +
  'now still already never always often soon later today yesterday ago';

const VERBS =
  'add answer ask be become begin believe bring call change check choose close come copy cut delete ' +
  'do draw end enter fill find finish fix follow get give go happen have hear help hide hold keep ' +
  'know learn leave let like listen look lose make mean move need open pick press put read ' +
  'remember remove run say see send set show sign start stay stop take talk tell think throw try ' +
  'use wait want watch work write save pause continue wake click tap name copy paste type ' +
  'read search store come back go works worked made makes making wants wanted asks asked says said ' +
  'shows showed shown shows changes changed changing keeps kept keeping starts started starting ' +
  'stops stopped stopping sends sent sending tells told uses used using tries tried takes took ' +
  'opens opened closes closed finishes finished finishing waits waiting working looks looked ' +
  'reads reading read means meant needs needed picks picked puts putting returns returned return ' +
  'found finds finding goes went gone coming comes came gets got getting gives gave given ' +
  'knows knew known lets letting says saying sees saw seen sets setting signs signed ' +
  'thinks thought tries trying types typing writes wrote written wrote tick ticks';

const NOUNS =
  'answer answers button buttons card change changes choice choices class club code computer ' +
  'conversation copy day days end event file files folder folders hour hours idea internet ' +
  'internet it key list message messages minute minutes name page pages part password passwords ' +
  'person place plan problem program programs project projects question questions report ' +
  'score screen settings shop skill skills step steps students customers team teacher thing things ' +
  'time timer tool tools version versions way week while word words work world year ' +
  'quiz calculator price prices check browser wifi plan usage limit reply sentence sentences ' +
  'file folder start middle top bottom side line lines text box mistake help support ' +
  'sign money number numbers phone window windows mind life home office hand ' +
  'look looks safe safety work works';

const ADJECTIVES =
  'able afraid bright clean clear easy fast fun good great hard high kind large late low main ' +
  'nice ready real safe short simple slow small sure true used useful whole wrong ' +
  'big bigger easy open own plain quick quiet ready red right sad same short slow sorry ' +
  'strong sure tired young stuck';

const MORE =
  'me my mine off else fewer full harm itself last lost mode moment plant saved save saves smaller switch ' +
  'time times undo web cancel copied control removed ran reach download expect called agent\'s ai helper ' +
  'online unsafe showing delete deleted deletes outside inside danger know knows';

const PEOPLE_AND_OTHER =
  'someone something anything nothing everything everyone anyone nobody ' +
  'please thanks hello ok okay maybe sometimes together away back ' +
  'inside outside near far across around through during since ' +
  'not yet even instead enough really well ' +
  'new newer newest older oldest';

const SIMPLE_PRODUCT_WORDS =
  'preview details detail summary simple look copy copies keep thrown throw away go back ' +
  'sign signed refuse refused ' +
  'real stay stays stayed fine ' +
  'careful fast need needs ' +
  'later early';

export const ALLOWED_WORDS: Set<string> = new Set(
  [PRODUCT, MORE, FUNCTION_WORDS, VERBS, NOUNS, ADJECTIVES, PEOPLE_AND_OTHER, SIMPLE_PRODUCT_WORDS]
    .join(' ')
    .split(/\s+/)
    .filter(Boolean),
);
