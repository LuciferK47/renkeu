import { describe, it, expect } from 'vitest';
import { pickTier, settingsFor } from '../src/landing/scene/quality.js';
import { floorPosition, cameraAt, worldAt, KEYFRAMES } from '../src/landing/scene/story.js';
import { floors } from '../src/landing/copy.js';
import { crabFrame, crabCells } from '../src/lib/sprites/crab.js';
import { botFrame } from '../src/lib/sprites/bot.js';

describe('pickTier', () => {
  it('draws the full scene where it can', () => expect(pickTier({})).toBe('full'));
  it('falls back to posters without WebGL or with reduced motion', () => {
    expect(pickTier({ webgl: false })).toBe('poster');
    expect(pickTier({ reducedMotion: true })).toBe('poster');
  });
  it('goes light on small, low-memory or data-saving devices', () => {
    expect(pickTier({ width: 390 })).toBe('lite');
    expect(pickTier({ memoryGb: 2 })).toBe('lite');
    expect(pickTier({ saveData: true })).toBe('lite');
  });
  it('can be forced from the address', () => expect(pickTier({ webgl: false, forced: 'full' })).toBe('full'));
  it('has no live settings for posters, and a coarser pixel for lite', () => {
    expect(settingsFor('poster')).toBeNull();
    expect(settingsFor('lite', { height: 900 }).pixelSize).toBeGreaterThan(settingsFor('full', { height: 900 }).pixelSize);
    expect(settingsFor('lite').rain).toBe(false);
  });
});

describe('camera and world', () => {
  it('has one keyframe per floor', () => expect(KEYFRAMES.map((k) => k.id)).toEqual(floors.map((f) => f.id)));

  it('finds the section the viewport is in, and how far through it', () => {
    const at = (tops) => floorPosition(tops.map((top) => ({ top, height: 1000 })));
    expect(at([200, 1200])).toBe(0);
    expect(at([-500, 500])).toBe(0.5);
    expect(at([-1000, 0, 1000])).toBeCloseTo(1);
    expect(at([-2000, -1000])).toBe(1); // past the last section: clamps
  });

  it('holds each keyframe, then travels to the next', () => {
    expect(cameraAt(1).pos).toEqual(KEYFRAMES[1].pos);
    const mid = cameraAt(1.92).pos[1];
    expect(mid).toBeLessThan(KEYFRAMES[1].pos[1]);
    expect(mid).toBeGreaterThan(KEYFRAMES[2].pos[1]);
    expect(cameraAt(99).pos).toEqual(KEYFRAMES.at(-1).pos);
  });

  it('keeps the review gate shut until the visitor applies, however far they scroll', () => {
    expect(worldAt(1, { step: 3, applied: false }).loop.gateOpen).toBe(false);
    expect(worldAt(1, { step: 3, applied: true }).loop.gateOpen).toBe(true);
    expect(worldAt(1, { step: 2, applied: true }).loop.gateOpen).toBe(false);
  });

  it('poses the crab for each step, and puts both characters to sleep when the AI is off', () => {
    expect([0, 1, 2, 3].map((step) => worldAt(1, { step }).loop.crabPose)).toEqual(['error', 'carry', 'wait', 'idle']);
    expect(worldAt(1, { step: 3, applied: true }).loop.crabPose).toBe('stamp');
    const off = worldAt(1, { step: 1, aiOff: true }).loop;
    expect([off.crabPose, off.botMood]).toEqual(['asleep', 'asleep']);
  });

  it('sends packets through the pipe only while the visitor is asking', () => {
    expect(worldAt(4, {}).boundary.packets).toBe(false);
    expect(worldAt(4, { asking: true }).boundary.packets).toBe(true);
  });
});

describe('sprites', () => {
  it('have rectangular frames, and every pose differs from idle', () => {
    for (const pose of ['idle', 'carry', 'wait', 'stamp', 'error', 'asleep']) {
      const f = crabFrame(pose);
      expect(new Set(f.map((r) => r.length)).size).toBe(1);
    }
    for (const pose of ['carry', 'stamp', 'error', 'asleep']) expect(crabFrame(pose)).not.toEqual(crabFrame('idle'));
  });
  it('alternate legs between frames', () => expect(crabFrame('idle', 0)).not.toEqual(crabFrame('idle', 1)));
  it('produce a filled cell list', () => expect(crabCells('idle').length).toBeGreaterThan(40));
  it('have a bot that changes with its mood', () => {
    expect(botFrame('asleep')).not.toEqual(botFrame('idle'));
    expect(botFrame('working')).not.toEqual(botFrame('idle'));
  });
});
