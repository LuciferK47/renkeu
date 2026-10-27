// The repair bot: a small hovering unit with a propeller and an antenna light.
// Pixel map shared by the 2D sprite and the landing scene's voxels.
//
// h hull, d hull shadow, v visor, l antenna light, a arm, w window glow.

export const BOT_PALETTE = {
  h: '#8fa3b8',
  d: '#4a5a6c',
  v: '#14161a',
  l: '#3fe0d0',
  a: '#c1875d',
  w: '#3fe0d0',
  z: '#ff8a3d'
};

const BODY = [
  '.....l.....',
  '.....d.....',
  '..hhhhhhh..',
  '.hhvvvvvhh.',
  '.hhvwvwvhh.',
  '.hhvvvvvhh.',
  '..hhhhhhh..',
  'a.hddddhh.a',
  'aa.hhhhh.aa',
  '...d...d...'
];

/** `mood`: 'idle' | 'working' | 'asleep'. */
export function botFrame(mood = 'idle') {
  const grid = [...BODY];
  const set = (x, y, ch) => {
    grid[y] = grid[y].slice(0, x) + ch + grid[y].slice(x + 1);
  };
  if (mood === 'asleep') {
    set(5, 0, 'd'); // light off
    set(4, 4, 'v');
    set(6, 4, 'v');
  }
  if (mood === 'working') {
    set(4, 4, 'z');
    set(6, 4, 'z');
  }
  return grid;
}

export const BOT_MOODS = ['idle', 'working', 'asleep'];
