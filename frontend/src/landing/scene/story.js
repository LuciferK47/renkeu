// The scene as a pure function of where the visitor is. Nothing here touches
// three.js: given the section, how far through it, and what the visitor has
// done, it says where the camera is and what each character and prop is doing.
// That keeps the choreography testable without a GPU.

import { floors } from '../copy.js';

/** Camera keyframes, one per section: where it sits and what it looks at. */
export const KEYFRAMES = [
  { id: 'hero', pos: [4, 14, 47], look: [-9, 26.5, 0], hold: 0.3 },
  // The pinned sections (the repair loop, the engine) stay put until their
  // last steps have been read, then the camera moves on.
  { id: 'loop', pos: [-4.5, 30.5, 15.5], look: [-4.5, 30.2, 0], hold: 0.82 },
  { id: 'workspace', pos: [4.5, 22.5, 15], look: [4.5, 22.5, 0], hold: 0.45 },
  { id: 'engine', pos: [0, 12.9, 15.5], look: [0, 12.2, 0], hold: 0.7 },
  { id: 'boundary', pos: [3, 5.2, 15.5], look: [3, 4.6, 0], hold: 0.45 },
  { id: 'install', pos: [0, 1.0, 15], look: [0, 0.0, 0], hold: 0.45 },
  { id: 'closing', pos: [-9, 4.5, 34], look: [0, 12, 0], hold: 0.5 }
];

const clamp01 = (v) => Math.min(1, Math.max(0, v));
const lerp = (a, b, t) => a + (b - a) * t;
const ease = (t) => t * t * (3 - 2 * t);
const mix = (a, b, t) => a.map((v, i) => lerp(v, b[i], t));

/**
 * Where the page is, as one number: the section index plus how far through it
 * (0 to 1), so 2.5 is halfway through the third section.
 */
export function floorPosition(sections) {
  // sections: [{ top, height }] measured against the viewport's centre line.
  for (let i = 0; i < sections.length; i += 1) {
    const { top, height } = sections[i];
    if (top + height > 0) return i + clamp01(-top / height);
  }
  return Math.max(0, sections.length - 1);
}

/** Camera position and target for a floor position. */
export function cameraAt(position) {
  const last = KEYFRAMES.length - 1;
  const p = Math.min(last, Math.max(0, position));
  const i = Math.min(last - 1, Math.floor(p));
  // Hold each keyframe for the first part of its section, then travel.
  const hold = KEYFRAMES[i].hold ?? 0.45;
  const t = ease(clamp01((p - i - hold) / (1 - hold)));
  return {
    pos: mix(KEYFRAMES[i].pos, KEYFRAMES[i + 1].pos, t),
    look: mix(KEYFRAMES[i].look, KEYFRAMES[i + 1].look, t)
  };
}

/** Which floor (index into `floors`) the visitor is on. */
function floorIndex(position) {
  return Math.min(floors.length - 1, Math.max(0, Math.round(position)));
}

/**
 * The state of the world.
 * @param {number} position  floor position from `floorPosition`
 * @param {{ step?: number, applied?: boolean, asking?: boolean, workspaceTab?: string, aiOff?: boolean, crate?: number }} input
 */
export function worldAt(position, input = {}) {
  const { step = 0, applied = false, asking = false, workspaceTab = 'project', aiOff = false, crate = -1 } = input;

  // Repair loop: the four steps drive the conveyor. The gate only opens once
  // the visitor has pressed Apply, never because they scrolled.
  const loopStep = Math.min(3, Math.max(0, step));
  const gateOpen = loopStep === 3 && applied;
  const crabPose = aiOff
    ? 'asleep'
    : loopStep === 0
      ? 'error'
      : loopStep === 1
        ? 'carry'
        : loopStep === 2
          ? 'wait'
          : gateOpen
            ? 'stamp'
            : 'idle';
  const botMood = aiOff ? 'asleep' : loopStep >= 1 && !gateOpen ? 'working' : 'idle';
  // Where the paper sits along the conveyor, from -1 (arriving) to 1 (leaving).
  const paperX = [-2.2, -0.6, 0.9, gateOpen ? 4.4 : 1.9][loopStep];

  // Engine bench: crates start exploded and close up through the section.
  const bench = ease(clamp01((position - 3 - 0.02) / 0.5));

  return {
    camera: cameraAt(position),
    floor: floorIndex(position),
    loop: { step: loopStep, gateOpen, crabPose, botMood, paperX, glitch: loopStep <= 2, fixed: loopStep === 3 },
    workspace: { tab: workspaceTab, beam: workspaceTab === 'sync' },
    engine: { assembled: bench, focus: crate },
    boundary: { packets: asking },
    rain: position < 1.3 || position > 5.6
  };
}
