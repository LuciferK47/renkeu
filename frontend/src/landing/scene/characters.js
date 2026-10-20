// The two characters, built from the shared pixel maps: the courier crab and
// the repair bot. Each pose is its own small InstancedMesh; only one is shown.

import * as THREE from 'three';
import { CRAB_PALETTE, CRAB_POSES, crabFrame } from '../../lib/sprites/crab.js';
import { BOT_PALETTE, BOT_MOODS, botFrame } from '../../lib/sprites/bot.js';
import { CREW_PALETTES, crewFrame } from '../../lib/sprites/crew.js';
import { spriteVoxels, box } from './voxels.js';

const CELL = 0.2;

export function createCrab({ scale = 1 } = {}) {
  const root = new THREE.Group();
  const material = new THREE.MeshStandardMaterial({ roughness: 0.85, metalness: 0.05 });
  const variants = new Map();
  for (const pose of CRAB_POSES) {
    for (const frame of [0, 1]) {
      const mesh = spriteVoxels(crabFrame(pose, frame), CRAB_PALETTE, { cell: CELL, depth: 4, material });
      mesh.visible = false;
      root.add(mesh);
      variants.set(`${pose}:${frame}`, mesh);
    }
  }
  root.scale.setScalar(scale);
  let shown = null;

  /** Show one pose and leg frame. Cheap enough to call every frame. */
  function set(pose, frame = 0) {
    const key = `${pose}:${frame & 1}`;
    if (key === shown) return;
    if (shown) variants.get(shown).visible = false;
    variants.get(key).visible = true;
    shown = key;
  }
  set('idle', 0);
  return { root, set, width: 14 * CELL * scale, height: 10 * CELL * scale };
}

export function createBot({ scale = 1 } = {}) {
  const root = new THREE.Group();
  const body = new THREE.Group();
  root.add(body);
  const material = new THREE.MeshStandardMaterial({ roughness: 0.7, metalness: 0.25 });
  const variants = new Map();
  for (const mood of BOT_MOODS) {
    const mesh = spriteVoxels(botFrame(mood), BOT_PALETTE, { cell: CELL, depth: 4, material });
    mesh.visible = false;
    body.add(mesh);
    variants.set(mood, mesh);
  }
  // Propeller above the antenna: two crossed blades that spin while awake.
  const prop = new THREE.Group();
  const blade = box(1.3, 0.05, 0.12, 0xc1875d);
  const blade2 = box(0.12, 0.05, 1.3, 0xc1875d);
  prop.add(blade, blade2);
  prop.position.y = 10 * CELL + 0.18;
  body.add(prop);
  root.scale.setScalar(scale);

  let mood = 'idle';
  variants.get(mood).visible = true;

  function set(next) {
    if (next === mood) return;
    variants.get(mood).visible = false;
    variants.get(next).visible = true;
    mood = next;
  }

  function update(t, dt) {
    const awake = mood !== 'asleep';
    prop.rotation.y += dt * (awake ? (mood === 'working' ? 26 : 12) : 0);
    body.position.y = awake ? Math.sin(t * 2.2) * 0.08 : 0;
  }

  return { root, set, update, mood: () => mood };
}

/**
 * One of the five crew (lib/sprites/crew.js). `ghost` makes it translucent and
 * self-lit, for the golden one.
 */
export function createCrewMember(id, { scale = 1, ghost = false } = {}) {
  const root = new THREE.Group();
  const material = ghost
    ? new THREE.MeshStandardMaterial({ roughness: 0.6, transparent: true, opacity: 0.78, emissive: '#6b4a10', emissiveIntensity: 0.9 })
    : new THREE.MeshStandardMaterial({ roughness: 0.85, metalness: 0.05 });
  const frames = [0, 1].map((f) => {
    const mesh = spriteVoxels(crewFrame(id, f), CREW_PALETTES[id], { cell: CELL, depth: 4, material });
    mesh.visible = f === 0;
    root.add(mesh);
    return mesh;
  });
  root.scale.setScalar(scale);
  let shown = 0;
  function set(frame) {
    const f = frame & 1;
    if (f === shown) return;
    frames[shown].visible = false;
    frames[f].visible = true;
    shown = f;
  }
  return { root, set, material, width: 12 * CELL * scale, height: 14 * CELL * scale };
}
