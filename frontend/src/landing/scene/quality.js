// How much of the scene to draw. The page must read as well on a phone, with
// reduced motion, or with no WebGL as it does on a desktop GPU, so the choice
// is a pure function of the environment and easy to test.

/**
 * @returns {'full' | 'lite' | 'poster'}
 */
export function pickTier({ webgl = true, reducedMotion = false, saveData = false, memoryGb = 8, width = 1440, forced = null } = {}) {
  if (forced === 'poster' || forced === 'lite' || forced === 'full') return forced;
  if (!webgl || reducedMotion) return 'poster';
  if (saveData || memoryGb <= 2 || width < 700) return 'lite';
  return 'full';
}

/** Rendering settings for a tier; `poster` draws nothing live. */
export function settingsFor(tier, { height = 900 } = {}) {
  if (tier === 'poster') return null;
  const lite = tier === 'lite';
  // Roughly 170-230 "pixels" tall, whatever the window size: the pixel size
  // follows the height so the art keeps the same look on every screen.
  const target = lite ? 170 : 225;
  const pixelSize = Math.max(2, Math.round(height / target));
  return { pixelSize, rain: !lite, maxFps: lite ? 30 : 60, antialias: false };
}

/** Read the environment. Kept apart so `pickTier` stays pure. */
export function detectEnvironment(win = globalThis.window) {
  if (!win) return { webgl: false };
  let webgl = false;
  try {
    const canvas = win.document.createElement('canvas');
    webgl = !!canvas.getContext('webgl2');
  } catch {
    webgl = false;
  }
  const params = new URLSearchParams(win.location.search);
  const forced = params.has('poster') ? 'poster' : params.get('tier');
  return {
    webgl,
    reducedMotion: !!win.matchMedia?.('(prefers-reduced-motion: reduce)').matches,
    saveData: !!win.navigator?.connection?.saveData,
    memoryGb: win.navigator?.deviceMemory ?? 8,
    width: win.innerWidth,
    forced: ['poster', 'lite', 'full'].includes(forced) ? forced : null
  };
}
