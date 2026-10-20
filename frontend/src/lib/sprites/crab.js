// The courier crab as a pixel map: the one source for the 2D sprite
// (Crab.svelte) and the 3D voxels of the landing scene, so they cannot drift.
// Original artwork. It is not Ferris and not the Rust logo.
//
// Characters: s shell, d shell shadow, c claw, k ink, e eye, m satchel,
// r red diagnostic block, g green approval stamp, x error eye.

export const CRAB_PALETTE = {
  s: '#e2512f',
  d: '#9c3320',
  c: '#ff8a5c',
  k: '#14161a',
  e: '#f3eee3',
  m: '#c1875d',
  r: '#ff3b4e',
  g: '#6ee7a0',
  x: '#ffc247'
};

const BODY = [
  '..k........k..',
  '..e........e..',
  'cc.ssssssss.cc',
  'cc.ssssssss.cc',
  '.c.ssdssdss.c.',
  '...ssssssss...',
  '...smmmmmms...'
];

const LEGS = [
  ['..s.ssssss.s.', '.s..s....s..s', 's..s......s..s'],
  ['..s.ssssss.s.', 's..s......s..s', '.s..s....s..s']
];

const pad = (row, width = 14) => row.padEnd(width, '.');

function put(grid, x, y, ch) {
  if (grid[y] && x >= 0 && x < grid[y].length) grid[y] = grid[y].slice(0, x) + ch + grid[y].slice(x + 1);
}

/**
 * One frame of the crab: an array of equal-width strings, top row first.
 * `frame` (0 or 1) alternates the legs for walking and waiting.
 */
export function crabFrame(pose = 'idle', frame = 0) {
  const grid = [...BODY, ...LEGS[frame & 1]].map((r) => pad(r));
  switch (pose) {
    case 'carry': // a red diagnostic block held between the claws
      put(grid, 6, 1, 'r');
      put(grid, 7, 1, 'r');
      put(grid, 6, 0, 'r');
      put(grid, 7, 0, 'r');
      break;
    case 'stamp': // a green approval mark raised in one claw
      put(grid, 0, 1, 'g');
      put(grid, 1, 1, 'g');
      put(grid, 0, 0, 'g');
      put(grid, 1, 0, 'g');
      break;
    case 'error': // eyes cross, claws up
      put(grid, 2, 1, 'x');
      put(grid, 11, 1, 'x');
      put(grid, 0, 1, 'c');
      put(grid, 13, 1, 'c');
      break;
    case 'asleep': // eyes closed
      put(grid, 2, 1, 'k');
      put(grid, 11, 1, 'k');
      break;
    default:
      break;
  }
  return grid;
}

export const CRAB_POSES = ['idle', 'carry', 'wait', 'stamp', 'error', 'asleep'];

/** The crab as SVG rects, for Crab.svelte. Returns [{x, y, fill}] per filled cell. */
export function crabCells(pose = 'idle', frame = 0) {
  const cells = [];
  crabFrame(pose, frame).forEach((row, y) => {
    [...row].forEach((ch, x) => {
      if (ch !== '.') cells.push({ x, y, fill: CRAB_PALETTE[ch] ?? CRAB_PALETTE.s });
    });
  });
  return cells;
}
