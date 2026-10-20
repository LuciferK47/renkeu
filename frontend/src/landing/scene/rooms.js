// The tower: four open-fronted rooms stacked like a dollhouse, a loading dock
// at street level, and a roof with a billboard. Each room is a stage for one
// part of the page; `update` moves its props to match the world state.

import * as THREE from 'three';
import { box, boxField } from './voxels.js';
import { paperTexture, screenTexture, labelTexture } from './textures.js';
import { createCrab, createBot, createCrewMember } from './characters.js';
import { PALETTE } from './palette.js';

export const ROOM_Y = { loop: 27, workspace: 19, engine: 11, boundary: 3 };
const W = 13;
const H = 7;
const D = 6.4;

const damp = (cur, target, rate, dt) => cur + (target - cur) * (1 - Math.exp(-rate * dt));

function shell(y0, trim) {
  const g = new THREE.Group();
  const floor = box(W + 1.4, 0.6, D, PALETTE.steel2, { metalness: 0.4 });
  floor.position.set(0, y0 - 0.3, 0);
  const back = box(W, H, 0.4, PALETTE.steel1);
  back.position.set(0, y0 + H / 2, -D / 2 + 0.2);
  const left = box(0.5, H, D, PALETTE.steel2);
  left.position.set(-W / 2 - 0.25, y0 + H / 2, 0);
  const right = left.clone();
  right.position.x = W / 2 + 0.25;
  const strip = box(W, 0.12, 0.12, trim, { emissive: true });
  strip.position.set(0, y0 + H - 0.1, D / 2 - 0.15);
  const stripFloor = box(W, 0.1, 0.1, trim, { emissive: true });
  stripFloor.position.set(0, y0 + 0.08, D / 2 - 0.1);
  g.add(floor, back, left, right, strip, stripFloor);
  return g;
}

function light(color, intensity, distance, pos) {
  const l = new THREE.PointLight(color, intensity * 3, distance, 2);
  l.position.set(...pos);
  return l;
}

/** Box with a texture on its front face and a plain colour elsewhere. */
function framed(w, h, d, color, map) {
  const plain = new THREE.MeshStandardMaterial({ color, roughness: 0.8 });
  const face = new THREE.MeshBasicMaterial({ map });
  const mesh = new THREE.Mesh(new THREE.BoxGeometry(w, h, d), [plain, plain, plain, plain, face, plain]);
  return mesh;
}

// --- room 1: the repair conveyor --------------------------------------

function loopRoom() {
  const y0 = ROOM_Y.loop;
  const g = new THREE.Group();
  g.add(shell(y0, PALETTE.sodium));

  const belt = box(10.8, 0.3, 1.8, PALETTE.steel3, { metalness: 0.5 });
  belt.position.set(0, y0 + 0.95, 0);
  g.add(belt);
  for (const x of [-4.6, 0, 4.6]) {
    const leg = box(0.3, 0.8, 1.4, PALETTE.steel2);
    leg.position.set(x, y0 + 0.4, 0);
    g.add(leg);
  }
  const slatItems = Array.from({ length: 24 }, (_, i) => ({ pos: [-5.2 + i * 0.45, y0 + 1.13, 0], size: [0.14, 0.06, 1.5], color: '#4c5a6b' }));
  const slats = boxField(slatItems);
  slats.frustumCulled = false;
  g.add(slats);
  const slatMatrix = new THREE.Matrix4();

  const paperBroken = paperTexture('broken');
  const paperFixed = paperTexture('fixed');
  const paper = framed(1.5, 2.0, 0.07, '#f3eee3', paperBroken);
  paper.position.set(-2.2, y0 + 2.2, 0);
  paper.rotation.x = -0.08;
  g.add(paper);

  // Review gate: the arm stays down until the visitor applies the change.
  const gateX = 3.1;
  for (const z of [-1.1, 1.1]) {
    const post = box(0.3, 2.6, 0.3, PALETTE.steel3);
    post.position.set(gateX, y0 + 1.3 + 0.3, z);
    g.add(post);
  }
  const pivot = new THREE.Group();
  pivot.position.set(gateX, y0 + 2.5, -1.1);
  const arm = box(0.22, 0.22, 2.2, PALETTE.err, { emissive: true });
  arm.position.z = 1.1;
  pivot.add(arm);
  g.add(pivot);
  const lamp = box(0.34, 0.34, 0.34, PALETTE.err, { emissive: true });
  lamp.position.set(gateX, y0 + 3.05, 1.1);
  g.add(lamp);

  const stamp = new THREE.Group();
  [[0, 0], [1, -1], [2, 0], [3, 1], [4, 2], [5, 3]].forEach(([x, y]) => {
    const b = box(0.3, 0.3, 0.3, PALETTE.ok, { emissive: true });
    b.position.set(x * 0.3 - 0.75, y * 0.3, 0);
    stamp.add(b);
  });
  stamp.position.set(gateX + 0.6, y0 + 4.4, 0.4);
  stamp.visible = false;
  g.add(stamp);

  const bot = createBot({ scale: 1.15 });
  bot.root.position.set(0.9, y0 + 3.0, 0.2);
  g.add(bot.root);
  const beam = box(0.08, 1.4, 0.08, PALETTE.cyan, { emissive: true });
  beam.visible = false;
  g.add(beam);

  const crab = createCrab({ scale: 0.85 });
  crab.root.position.set(-4.4, y0 + 0.05, 1.9);
  g.add(crab.root);

  // Furnace on the left wall: where the pages come from.
  const furnace = box(2.0, 2.6, 1.6, PALETTE.steel3, { metalness: 0.5 });
  furnace.position.set(-5.4, y0 + 1.3, -1.7);
  const mouth = box(1.2, 0.8, 0.1, PALETTE.sodium, { emissive: true });
  mouth.position.set(-5.4, y0 + 1.0, -0.84);
  g.add(furnace, mouth);
  const fire = light(PALETTE.sodium, 45, 16, [-4.6, y0 + 1.6, 0.6]);
  g.add(fire, light(PALETTE.rust, 28, 20, [3, y0 + 5, 1.5]));

  let crabX = -4.4;
  let paperX = -2.2;
  let gateOpen = 0;
  let walkClock = 0;

  function update(dt, t, world) {
    const loop = world.loop;
    slatItems.forEach((item, i) => {
      const x = ((((item.pos[0] + 5.2 + t * 0.7) % 10.8) + 10.8) % 10.8) - 5.2;
      slatMatrix.makeTranslation(x, item.pos[1], 0).scale(new THREE.Vector3(...item.size));
      slats.setMatrixAt(i, slatMatrix);
    });
    slats.instanceMatrix.needsUpdate = true;

    paperX = damp(paperX, loop.paperX, 3.2, dt);
    paper.position.x = paperX;
    paper.material[4].map = loop.fixed ? paperFixed : paperBroken;
    paper.material[4].needsUpdate = true;
    paper.position.y = y0 + 2.2 + (loop.glitch ? Math.sin(t * 30) * 0.015 : 0);

    gateOpen = damp(gateOpen, loop.gateOpen ? 1 : 0, 5, dt);
    pivot.rotation.x = -gateOpen * Math.PI * 0.5;
    const col = loop.gateOpen ? PALETTE.ok : PALETTE.err;
    arm.material.color.set(col);
    lamp.material.color.set(col);
    stamp.visible = loop.gateOpen;
    stamp.scale.setScalar(1 + Math.sin(t * 6) * 0.04);

    const targetX = Math.max(-5.2, Math.min(4.6, loop.paperX - 2.7));
    const moving = Math.abs(targetX - crabX) > 0.05;
    crabX = damp(crabX, targetX, 2.6, dt);
    crab.root.position.x = crabX;
    walkClock += dt * (moving ? 8 : 2.2);
    crab.set(loop.crabPose, Math.floor(walkClock) & 1);
    crab.root.position.y = y0 + 0.05 + (moving ? Math.abs(Math.sin(walkClock * 2)) * 0.05 : 0);

    bot.set(loop.botMood);
    bot.update(t, dt);
    const working = loop.botMood === 'working';
    beam.visible = working;
    if (working) {
      beam.position.set(paperX, y0 + 2.9, 0.1);
      beam.material.opacity = 0.7;
      beam.scale.y = 0.9 + Math.sin(t * 22) * 0.1;
    }
    fire.intensity = (42 + Math.sin(t * 9) * 6 + Math.sin(t * 23) * 3) * 3;
  }
  return { group: g, update };
}

// --- room 2: the workspace --------------------------------------------

function workspaceRoom() {
  const y0 = ROOM_Y.workspace;
  const g = new THREE.Group();
  g.add(shell(y0, PALETTE.cyan));

  const desk = box(11.5, 0.3, 2.6, PALETTE.steel3, { metalness: 0.4 });
  desk.position.set(0, y0 + 1.45, 0.4);
  g.add(desk);
  for (const x of [-5.2, 5.2]) {
    const leg = box(0.4, 1.3, 2.2, PALETTE.steel2);
    leg.position.set(x, y0 + 0.65, 0.4);
    g.add(leg);
  }

  const screens = { project: screenTexture('project'), sync: screenTexture('sync'), review: screenTexture('review') };
  const crt = new THREE.Group();
  const casing = box(5.6, 4.2, 2.6, PALETTE.steel2, { metalness: 0.3 });
  casing.position.set(0, 2.3, -0.3);
  const screen = new THREE.Mesh(new THREE.PlaneGeometry(4.7, 2.95), new THREE.MeshBasicMaterial({ map: screens.project }));
  screen.position.set(0, 2.45, 1.03);
  const foot = box(2.2, 0.35, 1.6, PALETTE.steel3);
  foot.position.set(0, 0.2, -0.3);
  const glow = box(5.0, 3.5, 0.05, PALETTE.cyan, { emissive: true, opacity: 0.14 });
  glow.position.set(0, 2.45, 1.0);
  crt.add(casing, screen, foot, glow);
  crt.position.set(-1.3, y0 + 1.6, 0);
  g.add(crt);

  const keyboard = box(3.4, 0.14, 1.0, PALETTE.steel3);
  keyboard.position.set(-1.3, y0 + 1.68, 1.5);
  g.add(keyboard);

  // The finished page on the wall, and the beam that links the two.
  const poster = framed(2.6, 3.4, 0.1, '#f3eee3', paperTexture('fixed'));
  poster.position.set(4.3, y0 + 4.0, -2.8);
  g.add(poster);
  const beam = box(1, 0.1, 0.1, PALETTE.cyan, { emissive: true });
  beam.visible = false;
  g.add(beam);

  const mug = box(0.4, 0.5, 0.4, PALETTE.rust);
  mug.position.set(2.7, y0 + 1.8, 1.4);
  g.add(mug);
  g.add(light(PALETTE.cyan, 38, 16, [-1.3, y0 + 3.6, 3.2]), light(PALETTE.sodium, 14, 14, [5, y0 + 5.5, 1]));

  // Hazel works the editor: she types, and points at the page for Find line.
  const hazel = createCrewMember('hazel', { scale: 0.8 });
  hazel.root.position.set(3.6, y0 + 1.6, 0.7); // on the desk top, not in front of it
  g.add(hazel.root);

  let tab = 'project';
  function update(dt, t, world) {
    if (world.workspace.tab !== tab) {
      tab = world.workspace.tab;
      screen.material.map = screens[tab] ?? screens.project;
      screen.material.needsUpdate = true;
    }
    const on = world.workspace.beam;
    beam.visible = on;
    if (on) {
      const x0 = 1.15;
      const x1 = 3.0;
      const reach = (Math.sin(t * 3) * 0.5 + 0.5) * 0.35 + 0.65;
      beam.scale.x = (x1 - x0) * reach;
      beam.position.set(x0 + ((x1 - x0) * reach) / 2, y0 + 3.9, 0.9);
    }
    glow.material.opacity = 0.1 + Math.sin(t * 2) * 0.03;
    hazel.set(tab === 'sync' ? 1 : Math.floor(t * 1.6) & 1);
  }
  return { group: g, update };
}

// --- room 3: the engine bench ------------------------------------------

const CRATES = [
  { id: 'compiler', color: '#e2512f' },
  { id: 'latex_ai_core', color: '#c1875d' },
  { id: 'latex_ai_server', color: '#8fa3b8' },
  { id: 'synctex', color: '#3fe0d0' },
  { id: 'visual', color: '#ffc247' },
  { id: 'latex_ai_tauri', color: '#6ee7a0' }
];

function engineRoom() {
  const y0 = ROOM_Y.engine;
  const g = new THREE.Group();
  g.add(shell(y0, PALETTE.rust));

  const bench = box(12.2, 0.5, 2.8, PALETTE.steel3, { metalness: 0.5 });
  bench.position.set(0, y0 + 1.0, 0.2);
  g.add(bench);
  for (const x of [-5.4, 5.4]) {
    const leg = box(0.5, 0.8, 2.4, PALETTE.steel2);
    leg.position.set(x, y0 + 0.4, 0.2);
    g.add(leg);
  }

  const crates = CRATES.map((c, i) => {
    const grp = new THREE.Group();
    const body = box(1.45, 1.45, 1.45, '#3b4759');
    const cap = box(1.5, 0.22, 1.5, c.color, { emissive: true });
    cap.position.y = 0.78;
    // A big numeral on a plate in the crate's colour: names are too small to
    // read at this resolution, so the number matches the list under the scene.
    const label = new THREE.Mesh(
      new THREE.PlaneGeometry(0.95, 0.95),
      new THREE.MeshBasicMaterial({ map: labelTexture(String(i + 1), { width: 32, height: 32, size: 28, color: '#14161a', bg: c.color }) })
    );
    label.position.set(0, -0.12, 0.74);
    grp.add(body, cap, label);
    const base = new THREE.Vector3(-4.6 + i * 1.84, y0 + 1.25 + 0.72 + 0.12, 0.2);
    grp.userData = { base, i };
    g.add(grp);
    return grp;
  });

  const pipes = Array.from({ length: CRATES.length - 1 }, (_, i) => {
    const p = box(0.4, 0.22, 0.22, PALETTE.steel3, { metalness: 0.6 });
    p.position.set(-4.6 + i * 1.84 + 0.92, y0 + 2.1, 0.2);
    g.add(p);
    return p;
  });
  g.add(light(PALETTE.rust, 36, 18, [0, y0 + 5.5, 3]), light(PALETTE.cyan, 16, 14, [-4, y0 + 3, 2]));

  // Ember runs the bench: she dashes along it while the crates close up.
  const ember = createCrewMember('ember', { scale: 0.62 });
  ember.root.position.set(-6.0, y0 + 1.3, 1.9);
  g.add(ember.root);

  function update(dt, t, world) {
    const a = world.engine.assembled;
    const running = a > 0.02 && a < 0.98;
    ember.root.position.x = -6.0 + a * 11.6;
    ember.set(Math.floor(t * (running ? 8 : 2)) & 1);
    crates.forEach((grp) => {
      const { base, i } = grp.userData;
      const e = 1 - a;
      // The crate picked in the list hops, so the list and the scene agree.
      const hop = world.engine.focus === i ? Math.abs(Math.sin(t * 5)) * 0.35 : 0;
      grp.position.set(
        base.x + e * (i - 2.5) * 0.9,
        base.y + hop + e * (1.1 + (i % 2) * 0.8) + Math.sin(t * 1.4 + i) * 0.05 * e,
        base.z + e * ((i % 3) - 1) * 1.3
      );
      grp.rotation.y = e * (i % 2 ? 0.5 : -0.5);
      grp.rotation.z = e * (i % 2 ? 0.12 : -0.12);
    });
    pipes.forEach((p) => {
      p.scale.x = Math.max(0.001, a);
      p.visible = a > 0.35;
    });
  }
  return { group: g, update };
}

// --- room 4: the vault -------------------------------------------------

function boundaryRoom() {
  const y0 = ROOM_Y.boundary;
  const g = new THREE.Group();
  g.add(shell(y0, PALETTE.cyan));

  const vault = new THREE.Group();
  const body = box(4.4, 4.6, 3.4, PALETTE.steel3, { metalness: 0.6, roughness: 0.5 });
  body.position.y = 2.3;
  const door = box(2.9, 3.0, 0.3, PALETTE.steel2, { metalness: 0.6 });
  door.position.set(0, 2.4, 1.75);
  const wheelH = box(1.2, 0.18, 0.18, PALETTE.sodium, { emissive: true });
  const wheelV = box(0.18, 1.2, 0.18, PALETTE.sodium, { emissive: true });
  wheelH.position.set(0, 2.4, 2.0);
  wheelV.position.set(0, 2.4, 2.0);
  const lock = box(0.4, 0.4, 0.2, PALETTE.cyan, { emissive: true });
  lock.position.set(1.0, 3.5, 1.95);
  vault.add(body, door, wheelH, wheelV, lock);
  vault.position.set(-2.6, y0, 0);
  g.add(vault);

  const plaque = new THREE.Mesh(
    new THREE.PlaneGeometry(6, 0.75),
    new THREE.MeshBasicMaterial({ map: labelTexture('YOUR MACHINE', { width: 320, height: 40, size: 22, color: '#3fe0d0' }), transparent: true })
  );
  plaque.position.set(-1.2, y0 + 6.0, -2.9);
  g.add(plaque);

  // The one pipe that crosses the wall, to the provider outside.
  const pipe = box(10.2, 0.34, 0.34, PALETTE.steel3, { metalness: 0.5 });
  pipe.position.set(2.8, y0 + 2.4, 0.2);
  g.add(pipe);
  const pipeGlow = box(10.2, 0.08, 0.36, PALETTE.cyan, { emissive: true, opacity: 0.25 });
  pipeGlow.position.set(2.8, y0 + 2.58, 0.2);
  g.add(pipeGlow);

  const provider = box(3.2, 3.2, 3.0, PALETTE.steel2, { metalness: 0.5 });
  provider.position.set(10.2, y0 + 3.2, 0.2);
  const providerLabel = new THREE.Mesh(
    new THREE.PlaneGeometry(3, 0.7),
    new THREE.MeshBasicMaterial({ map: labelTexture('YOUR PROVIDER', { width: 240, height: 56, size: 20, color: '#ffc247' }), transparent: true })
  );
  providerLabel.position.set(10.2, y0 + 3.2, 1.72);
  g.add(provider, providerLabel);

  const packetColors = [PALETTE.sodium, PALETTE.cyan, PALETTE.sodium, PALETTE.cyan, PALETTE.sodium];
  const packets = packetColors.map((c) => {
    const p = box(0.46, 0.46, 0.46, c, { emissive: true });
    p.visible = false;
    g.add(p);
    return p;
  });
  g.add(light(PALETTE.cyan, 11, 15, [-1, y0 + 4.5, 3.4]), light(PALETTE.sodium, 18, 14, [8, y0 + 4, 3]));

  // Aurum guards the vault: a golden ghost that flares only when the AI is asked.
  const aurum = createCrewMember('aurum', { scale: 1.05, ghost: true });
  aurum.root.position.set(5.0, y0 + 0.4, 1.4);
  g.add(aurum.root);

  function update(dt, t, world) {
    const on = world.boundary.packets;
    pipeGlow.material.opacity = on ? 0.8 : 0.2;
    packets.forEach((p, i) => {
      p.visible = on;
      if (!on) return;
      const phase = (t * 0.55 + i / packets.length) % 1;
      p.position.set(-0.4 + phase * 10.2, y0 + 2.4, 0.2);
      p.rotation.y = t * 2 + i;
    });
    wheelH.rotation.z = on ? t * 1.2 : 0;
    wheelV.rotation.z = on ? t * 1.2 : 0;
    lock.material.color.set(on ? PALETTE.sodium : PALETTE.cyan);
    aurum.set(on ? 1 : 0);
    aurum.root.position.y = y0 + 0.4 + Math.sin(t * 1.6) * 0.18;
    aurum.material.opacity = 0.6 + Math.sin(t * 3.1) * 0.1 + (on ? 0.2 : 0);
  }
  return { group: g, update };
}

// --- the dock ------------------------------------------------------------

function dock() {
  const g = new THREE.Group();
  const wall = box(W + 1.4, 3.2, 0.5, PALETTE.steel1, { metalness: 0.3 });
  wall.position.set(0, 1.6, D / 2 + 0.2);
  g.add(wall);
  [-4.2, 0, 4.2].forEach((x, i) => {
    const frame = box(3.5, 2.8, 0.15, PALETTE.sodium, { emissive: true });
    frame.position.set(x, 1.5, D / 2 + 0.5);
    const door = box(3.2, 2.5, 0.2, ['#1c1410', '#101a1f', '#1a1218'][i], { metalness: 0.5 });
    door.position.set(x, 1.4, D / 2 + 0.58);
    const inner = box(2.4, 0.12, 0.1, [PALETTE.sodium, PALETTE.cyan, PALETTE.rust][i], { emissive: true });
    inner.position.set(x, 0.5, D / 2 + 0.74);
    const inner2 = inner.clone();
    inner2.position.y = 1.4;
    inner2.scale.x = 0.6;
    g.add(inner, inner2);
    const number = new THREE.Mesh(
      new THREE.PlaneGeometry(1, 0.5),
      new THREE.MeshBasicMaterial({ map: labelTexture(`0${i + 1}`, { width: 64, height: 32, size: 22, color: '#ffc247' }), transparent: true })
    );
    number.position.set(x, 2.8, D / 2 + 0.7);
    g.add(frame, door, number);
  });
  const crateItems = [[-6.4, 0.5, 4.3], [-6.4, 1.4, 4.3], [6.5, 0.5, 4.4], [5.5, 0.5, 4.8]].map(([x, y, z]) => ({ pos: [x, y, z], size: [0.9, 0.9, 0.9], color: '#4a3b2f' }));
  g.add(boxField(crateItems));
  g.add(light(PALETTE.sodium, 40, 16, [0, 3.2, 6.5]));
  // Pip hands out the packages, hopping between the bays.
  const pip = createCrewMember('pip', { scale: 0.75 });
  const stand = box(1.8, 1.0, 1.4, '#4a3b2f');
  stand.position.set(0, 0.5, 4.9);
  g.add(stand);
  pip.root.position.set(0, 1.05, 4.9);
  g.add(pip.root);
  return {
    group: g,
    update(dt, t) {
      pip.set(Math.floor(t * 1.4) & 1);
      pip.root.position.y = 1.05 + Math.abs(Math.sin(t * 3)) * 0.14;
    }
  };
}

export function createTower() {
  const group = new THREE.Group();
  const parts = [loopRoom(), workspaceRoom(), engineRoom(), boundaryRoom(), dock()];
  parts.forEach((p) => group.add(p.group));
  // Columns at the front corners, so the rooms read as one tower.
  for (const x of [-W / 2 - 0.45, W / 2 + 0.45]) {
    const col = box(0.5, 36, 0.5, PALETTE.steel2, { metalness: 0.5 });
    col.position.set(x, 17, D / 2);
    group.add(col);
  }
  // Slabs between rooms and the roof.
  const roof = box(W + 1.6, 0.8, D + 0.4, PALETTE.steel2, { metalness: 0.5 });
  roof.position.set(0, ROOM_Y.loop + H + 0.4, 0);
  group.add(roof);
  return {
    group,
    update(dt, t, world) {
      parts.forEach((p) => p.update(dt, t, world));
    }
  };
}

