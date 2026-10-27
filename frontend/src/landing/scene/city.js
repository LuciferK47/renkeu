// Everything outside the rooms: the wet street, the skyline, the rooftop with
// its billboard and chimney, the hero characters, and the rain.

import * as THREE from 'three';
import { box, boxField, rng } from './voxels.js';
import { billboardTexture, bladeTexture, hologramTexture, labelTexture } from './textures.js';
import { createCrab, createCrewMember } from './characters.js';
import { PALETTE } from './palette.js';
import { ROOM_Y } from './rooms.js';

const ROOF_Y = ROOM_Y.loop + 7 + 0.8;

/** A dusk gradient behind the skyline: smog-brown at the horizon, void above. */
function sky() {
  const c = document.createElement('canvas');
  c.width = 4;
  c.height = 256;
  const g = c.getContext('2d');
  const grad = g.createLinearGradient(0, 0, 0, 256);
  grad.addColorStop(0, '#05060a');
  grad.addColorStop(0.4, '#140a1e');
  grad.addColorStop(0.7, '#3a1342');
  grad.addColorStop(0.9, '#6b2a3a');
  grad.addColorStop(1, '#8a3a1e');
  g.fillStyle = grad;
  g.fillRect(0, 0, 4, 256);
  const tex = new THREE.CanvasTexture(c);
  tex.colorSpace = THREE.SRGBColorSpace;
  const plane = new THREE.Mesh(new THREE.PlaneGeometry(600, 260), new THREE.MeshBasicMaterial({ map: tex, fog: false }));
  plane.position.set(0, 60, -150);
  return plane;
}

function street() {
  const g = new THREE.Group();
  const ground = new THREE.Mesh(
    new THREE.PlaneGeometry(260, 200),
    new THREE.MeshStandardMaterial({ color: '#0a0d12', roughness: 0.25, metalness: 0.7 })
  );
  ground.rotation.x = -Math.PI / 2;
  ground.position.set(0, 0, 0);
  g.add(ground);
  // Neon lane lines and puddle glints.
  const items = [];
  for (let z = -30; z < 60; z += 6) items.push({ pos: [0, 0.02, z], size: [0.25, 0.02, 2.6], color: PALETTE.sodium });
  for (let x = -40; x <= 40; x += 8) items.push({ pos: [x, 0.02, 12], size: [3.5, 0.02, 0.18], color: x % 16 ? PALETTE.cyan : PALETTE.rust });
  g.add(boxField(items, { emissive: true }));
  return g;
}

function skyline() {
  const rand = rng(7);
  const buildings = [];
  const windows = [];
  const tints = ['#1a212b', '#202836', '#161c25', '#252d3b'];
  const lit = [PALETTE.cyan, PALETTE.sodium, '#ffd9a0', PALETTE.rust, '#7fb8c8', PALETTE.magenta, PALETTE.magenta];
  const place = (x, z, w, h, d) => {
    buildings.push({ pos: [x, h / 2, z], size: [w, h, d], color: tints[Math.floor(rand() * tints.length)] });
    const cols = Math.floor(w / 1.1);
    const rows = Math.floor(h / 1.5);
    for (let r = 1; r < rows; r += 1) {
      for (let c = 0; c < cols; c += 1) {
        if (rand() > 0.34) continue;
        windows.push({
          pos: [x - w / 2 + 0.7 + c * 1.1, r * 1.5, z + d / 2 + 0.03],
          size: [0.5, 0.65, 0.05],
          color: lit[Math.floor(rand() * lit.length)]
        });
      }
    }
  };
  for (let i = 0; i < 30; i += 1) {
    const side = i % 2 ? 1 : -1;
    const x = side * (13 + rand() * 46);
    place(x, -26 + rand() * 40, 4 + rand() * 6, 10 + rand() * 46, 4 + rand() * 6);
  }
  for (let i = 0; i < 14; i += 1) {
    const side = i % 2 ? 1 : -1;
    place(side * (24 + rand() * 40), -48 - rand() * 20, 8 + rand() * 8, 26 + rand() * 40, 6);
  }
  const g = new THREE.Group();
  g.add(boxField(buildings, { roughness: 0.9 }));
  g.add(boxField(windows, { emissive: true }));
  return g;
}

function rooftop() {
  const g = new THREE.Group();
  const board = new THREE.Mesh(
    new THREE.PlaneGeometry(12, 3),
    new THREE.MeshBasicMaterial({ map: billboardTexture() })
  );
  board.material.map.repeat.set(0.5, 1);
  board.position.set(0.8, ROOF_Y + 4.0, -2.2);
  const frame = box(12.5, 3.5, 0.3, PALETTE.steel2, { metalness: 0.5 });
  frame.position.set(0.8, ROOF_Y + 4.0, -2.45);
  const glow = box(12.8, 3.8, 0.1, PALETTE.cyan, { emissive: true, opacity: 0.12 });
  glow.position.set(0.8, ROOF_Y + 4.0, -2.3);
  for (const x of [-4.4, 6]) {
    const leg = box(0.35, 2.6, 0.35, PALETTE.steel3);
    leg.position.set(x, ROOF_Y + 1.3, -2.3);
    g.add(leg);
  }

  const chimney = box(1.3, 4.2, 1.3, PALETTE.steel3, { metalness: 0.5 });
  chimney.position.set(-5.4, ROOF_Y + 2.1, -1.2);
  const chimneyGlow = box(1.1, 0.12, 1.1, PALETTE.sodium, { emissive: true });
  chimneyGlow.position.set(-5.4, ROOF_Y + 4.25, -1.2);

  const mast = box(0.22, 12, 0.22, PALETTE.steel3);
  mast.position.set(5.9, ROOF_Y + 6, -1.4);
  const mastLight = box(0.5, 0.5, 0.5, PALETTE.err, { emissive: true });
  mastLight.position.set(5.9, ROOF_Y + 12.1, -1.4);

  const sheets = Array.from({ length: 16 }, (_, i) => {
    const s = new THREE.Mesh(new THREE.BoxGeometry(0.7, 0.9, 0.03), new THREE.MeshBasicMaterial({ color: PALETTE.paper }));
    s.userData = { phase: i / 16, sway: 0.6 + (i % 5) * 0.25, spin: (i % 3) - 1 };
    g.add(s);
    return s;
  });

  const crab = createCrab({ scale: 2.4 });
  crab.root.position.set(2.6, ROOF_Y + 0.05, 2.4);
  // Bruno, the host, waves from the roof.
  const bruno = createCrewMember('bruno', { scale: 1.8 });
  bruno.root.position.set(-1.9, ROOF_Y + 0.05, 2.0);

  const sodium = new THREE.PointLight(PALETTE.sodium, 160, 30, 2);
  sodium.position.set(-5.4, ROOF_Y + 5, 0);
  const sign = new THREE.PointLight(PALETTE.cyan, 110, 30, 2);
  sign.position.set(0.8, ROOF_Y + 4, 2.5);

  g.add(board, frame, glow, chimney, chimneyGlow, mast, mastLight, crab.root, bruno.root, sodium, sign);

  let walk = 0;
  return {
    group: g,
    update(dt, t, pointer) {
      board.material.map.offset.x = (t * 0.035) % 1;
      mastLight.visible = Math.floor(t * 1.5) % 2 === 0;
      sodium.intensity = 150 + Math.sin(t * 8) * 20;
      sheets.forEach((s) => {
        const p = (t * 0.045 + s.userData.phase) % 1;
        s.position.set(-5.4 + Math.sin(p * 9 + s.userData.phase * 20) * s.userData.sway + p * 3.2, ROOF_Y + 4.4 + p * 16, -1.2 + Math.cos(p * 7) * 0.8);
        s.rotation.y = p * 8 * s.userData.spin;
        s.rotation.z = Math.sin(p * 5) * 0.4;
        s.material.opacity = 1;
      });
      walk += dt * 2;
      crab.set('idle', Math.floor(walk) & 1);
      crab.root.rotation.y = pointer.x * 0.5;
      bruno.set(Math.floor(t * 1.4) & 1);
      bruno.root.rotation.y = pointer.x * 0.35;
    }
  };
}

function rain(count = 260) {
  const geo = new THREE.BoxGeometry(0.035, 0.9, 0.035);
  const mat = new THREE.MeshBasicMaterial({ color: '#7fb8c8', transparent: true, opacity: 0.55 });
  const mesh = new THREE.InstancedMesh(geo, mat, count);
  mesh.frustumCulled = false;
  const rand = rng(11);
  const drops = Array.from({ length: count }, () => ({ x: (rand() - 0.5) * 40, y: rand() * 28, z: (rand() - 0.5) * 34, v: 14 + rand() * 8 }));
  const m = new THREE.Matrix4();
  const dir = new THREE.Vector3();
  return {
    mesh,
    setVisible(v) {
      mesh.visible = v;
    },
    update(dt, camera) {
      if (!mesh.visible) return;
      camera.getWorldDirection(dir);
      mesh.position.copy(camera.position).addScaledVector(dir, 16);
      drops.forEach((d, i) => {
        d.y -= d.v * dt;
        if (d.y < -4) d.y += 28;
        m.makeTranslation(d.x, d.y - 8, d.z);
        mesh.setMatrixAt(i, m);
      });
      mesh.instanceMatrix.needsUpdate = true;
    }
  };
}

/** Neon signs and a hologram around the tower: the part that says "city". */
function neon() {
  const g = new THREE.Group();
  const sign = (map, w, h, pos, opacity = 1) => {
    const m = new THREE.Mesh(new THREE.PlaneGeometry(w, h), new THREE.MeshBasicMaterial({ map, transparent: true, opacity, side: THREE.DoubleSide }));
    m.position.set(...pos);
    g.add(m);
    return m;
  };
  const blade = sign(bladeTexture(['R', 'E', 'N', 'K', 'E', 'U'], PALETTE.magenta), 1.9, 10.6, [-10.6, 19, 5.4]);
  const bladeGlow = box(2.3, 11, 0.1, PALETTE.magenta, { emissive: true, opacity: 0.18 });
  bladeGlow.position.set(-10.6, 19, 5.2);
  const arm = box(2.6, 0.2, 0.2, PALETTE.steel3);
  arm.position.set(-9.4, 24.6, 5.2);
  const tex = sign(labelTexture('TEX', { width: 96, height: 32, size: 26, color: PALETTE.cyan, bg: '#04141a' }), 5.2, 1.7, [11.4, 14, 4.6]);
  const pdf = sign(labelTexture('PDF', { width: 96, height: 32, size: 26, color: PALETTE.sodium, bg: '#1a0c04' }), 5.2, 1.7, [11.4, 11.6, 4.6]);
  const holo = sign(hologramTexture('compile', PALETTE.cyan), 9, 5, [-17, 31, -2], 0.85);
  const magentaGlow = new THREE.PointLight(PALETTE.magenta, 70, 22, 2);
  magentaGlow.position.set(-9, 19, 8);
  const cyanGlow = new THREE.PointLight(PALETTE.cyan, 28, 18, 2);
  cyanGlow.position.set(11, 13, 7);
  g.add(bladeGlow, arm, magentaGlow, cyanGlow);
  return {
    group: g,
    update(t) {
      // A tube that stutters now and then, as neon does.
      const stutter = Math.sin(t * 17) > 0.96 ? 0.35 : 1;
      blade.material.opacity = stutter;
      magentaGlow.intensity = 70 * stutter;
      holo.position.y = 31 + Math.sin(t * 0.8) * 0.4;
      holo.material.opacity = 0.7 + Math.sin(t * 4) * 0.1;
      tex.material.opacity = 1;
      pdf.material.opacity = 1;
    }
  };
}

/** Cars on sky lanes, each a bright streak, crossing in front of and behind the tower. */
function traffic(count = 18) {
  const mesh = new THREE.InstancedMesh(new THREE.BoxGeometry(1, 1, 1), new THREE.MeshBasicMaterial(), count);
  mesh.frustumCulled = false;
  const rand = rng(23);
  const colors = [PALETTE.cyan, PALETTE.magenta, PALETTE.sodium];
  const cars = Array.from({ length: count }, (_, i) => {
    const near = i % 3 === 0;
    return {
      x: (rand() - 0.5) * 160,
      y: near ? 6 + rand() * 12 : 16 + rand() * 32,
      z: near ? 20 + rand() * 12 : -8 - rand() * 36,
      v: (rand() < 0.5 ? -1 : 1) * (5 + rand() * 9),
      len: 2 + rand() * 2
    };
  });
  const c = new THREE.Color();
  cars.forEach((car, i) => mesh.setColorAt(i, c.set(colors[i % colors.length])));
  const m = new THREE.Matrix4();
  const q = new THREE.Quaternion();
  return {
    mesh,
    update(dt) {
      cars.forEach((car, i) => {
        car.x += car.v * dt;
        if (car.x > 80) car.x = -80;
        if (car.x < -80) car.x = 80;
        m.compose(new THREE.Vector3(car.x, car.y, car.z), q, new THREE.Vector3(car.len, 0.22, 0.4));
        mesh.setMatrixAt(i, m);
      });
      mesh.instanceMatrix.needsUpdate = true;
    }
  };
}

export function createCity() {
  const group = new THREE.Group();
  group.add(sky(), street(), skyline());
  const roof = rooftop();
  group.add(roof.group);
  const drops = rain();
  const signs = neon();
  const cars = traffic();
  group.add(drops.mesh, signs.group, cars.mesh);
  return { group, roof, rain: drops, signs, cars };
}
