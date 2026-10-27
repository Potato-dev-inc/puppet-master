// The eight companion faces (SPEC-1028 section 10). Each face is a 12 x 12 dot bitmap drawn in currentColor.
// Cells: `#` full dot, `+` dim dot, `.` empty, `e` eye cell (empty when open in the `hole` style, a full dot
// when open in the `dot` style). Row 0 is the top row. The bitmaps are monochrome: no colour anywhere.
import type { AvatarId } from '../../lib/companion';

/** Width and height of a face in cells. */
export const GRID = 12;

export type EyeStyle = 'hole' | 'dot';

export interface AvatarDef {
  id: AvatarId;
  /** The default name; the person can rename a companion. */
  name: string;
  eyes: EyeStyle;
  /** Exactly GRID rows of GRID characters. */
  rows: readonly string[];
}

/** The eight faces, keyed by id. */
export const AVATARS: Record<AvatarId, AvatarDef> = {
  mochi: {
    id: 'mochi',
    name: 'Mochi',
    eyes: 'hole',
    rows: [
      '............',
      '............',
      '....####....',
      '..########..',
      '.##########.',
      '.##e####e##.',
      '###e####e###',
      '##+######+##',
      '############',
      '.##########.',
      '..########..',
      '............',
    ],
  },
  bit: {
    id: 'bit',
    name: 'Bit',
    eyes: 'dot',
    rows: [
      '.....##.....',
      '.....++.....',
      '..########..',
      '..#......#..',
      '..#.e..e.#..',
      '..#.e..e.#..',
      '..#..++..#..',
      '..########..',
      '....####....',
      '...######...',
      '...#....#...',
      '...#....#...',
    ],
  },
  nib: {
    id: 'nib',
    name: 'Nib',
    eyes: 'hole',
    rows: [
      '............',
      '....####....',
      '..########..',
      '.##########.',
      '.##e####e##.',
      '.##e####e##.',
      '.####..####.',
      '.##########.',
      '.##########.',
      '.##########.',
      '.#.##..##.#.',
      '............',
    ],
  },
  tofu: {
    id: 'tofu',
    name: 'Tofu',
    eyes: 'hole',
    rows: [
      '............',
      '...++++++++.',
      '..########+.',
      '..########+.',
      '..#e####e#+.',
      '..########+.',
      '..###..###+.',
      '..########+.',
      '..########..',
      '...#....#...',
      '............',
      '............',
    ],
  },
  lumen: {
    id: 'lumen',
    name: 'Lumen',
    eyes: 'hole',
    rows: [
      '..#......#..',
      '...#....#...',
      '....#..#....',
      '.++.####.++.',
      '+++#e##e#+++',
      '++++####++++',
      '.+++####+++.',
      '..++####++..',
      '...+####+...',
      '.....##.....',
      '............',
      '............',
    ],
  },
  hoot: {
    id: 'hoot',
    name: 'Hoot',
    eyes: 'dot',
    rows: [
      '............',
      '.#........#.',
      '.###....###.',
      '.##########.',
      '.#..####..#.',
      '.#.e####e.#.',
      '.#..#++#..#.',
      '.##########.',
      '.####++####.',
      '.##########.',
      '..##....##..',
      '............',
    ],
  },
  sprout: {
    id: 'sprout',
    name: 'Sprout',
    eyes: 'hole',
    rows: [
      '.......++...',
      '......++++..',
      '......#.....',
      '....####....',
      '..########..',
      '.##e####e##.',
      '.##########.',
      '.####..####.',
      '.##########.',
      '..########..',
      '...######...',
      '............',
    ],
  },
  kit: {
    id: 'kit',
    name: 'Kit',
    eyes: 'hole',
    rows: [
      '............',
      '#..........#',
      '##........##',
      '###......###',
      '############',
      '##e######e##',
      '.##########.',
      '..###++###..',
      '....####....',
      '.....##.....',
      '............',
      '............',
    ],
  },
};

export type CellKind = 'full' | 'dim' | 'eye' | 'empty';

/**
 * Reads a bitmap into a grid of cell kinds, row by row. Throws when the bitmap is not GRID x GRID or a character
 * is unknown, so a typo in the data fails loudly in the tests instead of drawing a hole.
 */
export function parseBitmap(rows: readonly string[]): CellKind[][] {
  if (rows.length !== GRID) throw new Error(`bitmap needs ${GRID} rows, got ${rows.length}`);
  return rows.map((row, r) => {
    if (row.length !== GRID) throw new Error(`bitmap row ${r} needs ${GRID} cells, got ${row.length}`);
    return Array.from(row, (ch): CellKind => {
      switch (ch) {
        case '#':
          return 'full';
        case '+':
          return 'dim';
        case 'e':
          return 'eye';
        case '.':
          return 'empty';
        default:
          throw new Error(`bitmap row ${r} has an unknown cell "${ch}"`);
      }
    });
  });
}
