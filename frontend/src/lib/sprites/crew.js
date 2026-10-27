// The foundry crew: five animatronic keepers, one per floor of the landing
// page. Original characters in the spirit of a haunted-pizzeria band (a bear, a
// rabbit, a chick, a fox, a golden ghost) with their own faces, palettes and
// names. They are scenery with jobs, not features of the product.
//
// Each has two frames (a small animation: a wave, a flap, a wag). Pixel maps
// are shared by the 2D sprite (Animatronic.svelte) and the 3D voxels.

export const CREW_IDS = ['bruno', 'hazel', 'ember', 'aurum', 'pip'];

const BRUNO = [
  '..dd....dd..',
  '.dbbd..dbbd.',
  '.bbbbbbbbbb.',
  'hbbbbbbbbbbh',
  'hbwkbbbbwkbh',
  'hbbbbmmbbbbh',
  '.bbbmmmmbbb.',
  '.bbbmkkmbbb.',
  '..ssssssss..',
  '.bbbbssbbbb.',
  'bbbbbbbbbbbb',
  'bb.bbbbbb.bb',
  'bb.bbbbbb.bb',
  'dd..dddd..dd'
];

const HAZEL = [
  '..ll....ll..',
  '..lp....lp..',
  '..lp....lp..',
  '..lp....lp..',
  '.llllllllll.',
  'lgkllllllkgl',
  '.llllppllll.',
  '..lwwwwwwl..',
  '..dllllllld.',
  '.llllllllll.',
  'llllllllllll',
  'll.llllll.ll',
  'll.llllll.ll',
  'dd..dddd..dd'
];

const EMBER = [
  'ff........ff',
  'fdf......fdf',
  '.ffffffffff.',
  'fgkffffffkgf',
  'ffffwwwwffff',
  '.fffwwwwfff.',
  '..fwwnnwwf..',
  '...ffwwff...',
  '..dffffffd..',
  '.ffffffffff.',
  'ffffffffffff',
  'ff.ffffff.ff',
  'ff.ffffff.ff',
  'dd..dddd..dd'
];

const AURUM = [
  '....aaaa....',
  '...aaaaaa...',
  '..aaaaaaaa..',
  '..avvvvvva..',
  '.aavevvevaa.',
  '.aavvvvvvaa.',
  '.aaavvvvaaa.',
  '.aaaaaaaaaa.',
  'aaaaaaaaaaaa',
  'aadaaaaaadaa',
  'aaaaaaaaaaaa',
  'aadaaaaaadaa',
  'aaaaaaaaaaaa',
  'a.aa.aa.aa.a'
];

const PIP = [
  '.....yy.....',
  '....yyyy....',
  '..yyyyyyyy..',
  '.yyyyyyyyyy.',
  '.yykyyyykyy.',
  '.yyyyyyyyyy.',
  '.yyyyooyyyy.',
  '.yyyyooyyyy.',
  '..yyyyyyyy..',
  '..ywwwwwwy..',
  '.yywwwwwwyy.',
  '.yywwwwwwyy.',
  '..yyyyyyyy..',
  '..oo....oo..'
];

/** Edits for the second frame: [x, y, char]. */
const SECOND = {
  // waves: the right arm goes up beside the head
  bruno: [[11, 7, 'b'], [11, 8, 'b'], [11, 9, 'b'], [11, 10, '.'], [11, 11, '.']],
  // points with a stick
  hazel: [[11, 6, 't'], [11, 7, 't'], [11, 8, 'l'], [11, 9, 'l'], [11, 10, '.'], [11, 11, '.']],
  // wags
  ember: [[0, 11, 'w'], [0, 12, 'w']],
  // eyes flare
  aurum: [[3, 4, 'e'], [5, 4, 'e'], [6, 4, 'e'], [8, 4, 'e']],
  // flaps
  pip: [[0, 8, 'y'], [0, 9, 'y'], [11, 8, 'y'], [11, 9, 'y']]
};

const FIRST = {
  ember: [[11, 11, 'w'], [11, 12, 'w']]
};

export const CREW_PALETTES = {
  bruno: { b: '#9a6a46', d: '#6b4529', m: '#e0bd94', k: '#14161a', w: '#f3eee3', s: '#e2512f', h: '#3fe0d0' },
  hazel: { l: '#b9a7e8', p: '#f0a8c8', d: '#8672bf', k: '#14161a', w: '#f3eee3', g: '#3fe0d0', t: '#ffc247' },
  ember: { f: '#e8742c', d: '#a8481a', w: '#f3eee3', k: '#14161a', n: '#14161a', g: '#ffc247' },
  aurum: { a: '#f2c14e', d: '#b8892a', v: '#14161a', e: '#fff6c8' },
  pip: { y: '#ffd24a', o: '#ff9a3c', k: '#14161a', w: '#f3eee3' }
};

const BASE = { bruno: BRUNO, hazel: HAZEL, ember: EMBER, aurum: AURUM, pip: PIP };

function applyEdits(rows, edits) {
  const grid = rows.map((r) => r.split(''));
  for (const [x, y, ch] of edits) grid[y][x] = ch;
  return grid.map((r) => r.join(''));
}

/** One frame (0 or 1) of a crew member: equal-width strings, top row first. */
export function crewFrame(id, frame = 0) {
  const base = BASE[id];
  if (!base) throw new Error(`no such crew member: ${id}`);
  return frame & 1 ? applyEdits(base, SECOND[id] ?? []) : applyEdits(base, FIRST[id] ?? []);
}

/** The cells of a frame as `{x, y, fill}`, for the 2D sprite. */
export function crewCells(id, frame = 0) {
  const palette = CREW_PALETTES[id];
  const cells = [];
  crewFrame(id, frame).forEach((row, y) => {
    [...row].forEach((ch, x) => {
      if (ch !== '.' && palette[ch]) cells.push({ x, y, fill: palette[ch] });
    });
  });
  return cells;
}

export const CREW_SIZE = { width: 12, height: 14 };
