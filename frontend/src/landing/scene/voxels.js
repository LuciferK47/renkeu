// Voxel helpers. Everything in the scene is boxes: a sprite is extruded into a
// slab of cubes, and groups of cubes are drawn as one InstancedMesh each, so a
// character costs one draw call however many voxels it has.

import * as THREE from 'three';

const unit = new THREE.BoxGeometry(1, 1, 1);

/**
 * Extrude a pixel map into voxels. `rows` is an array of equal-width strings,
 * top row first; `palette` maps a character to a colour; '.' is empty.
 * The result stands on y = 0, centred on x and z.
 */
export function spriteVoxels(rows, palette, { cell = 0.2, depth = 3, material } = {}) {
  const cells = [];
  const h = rows.length;
  const w = rows[0].length;
  rows.forEach((row, ry) => {
    [...row].forEach((ch, cx) => {
      if (ch === '.' || !palette[ch]) return;
      for (let d = 0; d < depth; d += 1) {
        // Back layers a little darker, so the slab reads as a solid.
        cells.push({ x: cx - (w - 1) / 2, y: h - 1 - ry, z: d - (depth - 1) / 2, color: palette[ch], shade: d === 0 ? 1 : 1 - 0.14 * d });
      }
    });
  });
  const mat = material ?? new THREE.MeshStandardMaterial({ roughness: 0.85, metalness: 0.05 });
  const mesh = new THREE.InstancedMesh(unit, mat, Math.max(1, cells.length));
  const m = new THREE.Matrix4();
  const q = new THREE.Quaternion();
  const c = new THREE.Color();
  cells.forEach((v, i) => {
    m.compose(new THREE.Vector3(v.x * cell, (v.y + 0.5) * cell, v.z * cell), q, new THREE.Vector3(cell, cell, cell));
    mesh.setMatrixAt(i, m);
    mesh.setColorAt(i, c.set(v.color).multiplyScalar(v.shade));
  });
  mesh.count = cells.length;
  mesh.instanceMatrix.needsUpdate = true;
  if (mesh.instanceColor) mesh.instanceColor.needsUpdate = true;
  mesh.userData.voxelCount = cells.length;
  return mesh;
}

/** A box with a plain colour. `emissive` boxes ignore lights (signs, screens). */
export function box(w, h, d, color, { emissive = false, roughness = 0.8, metalness = 0.1, opacity = 1 } = {}) {
  // There is no environment map to reflect, so a metallic surface would just go
  // black. Keep it matte; the lights do the work.
  metalness = Math.min(metalness, 0.1);
  const material = emissive
    ? new THREE.MeshBasicMaterial({ color, transparent: opacity < 1, opacity })
    : new THREE.MeshStandardMaterial({ color, roughness, metalness, transparent: opacity < 1, opacity });
  return new THREE.Mesh(new THREE.BoxGeometry(w, h, d), material);
}

/** Many boxes in one draw call. `items`: [{ pos: [x,y,z], size: [w,h,d], color }]. */
export function boxField(items, { emissive = false, roughness = 0.9 } = {}) {
  const material = emissive ? new THREE.MeshBasicMaterial() : new THREE.MeshStandardMaterial({ roughness, metalness: 0.05 });
  const mesh = new THREE.InstancedMesh(unit, material, Math.max(1, items.length));
  const m = new THREE.Matrix4();
  const q = new THREE.Quaternion();
  const c = new THREE.Color();
  items.forEach((it, i) => {
    m.compose(new THREE.Vector3(...it.pos), q, new THREE.Vector3(...it.size));
    mesh.setMatrixAt(i, m);
    mesh.setColorAt(i, c.set(it.color));
  });
  mesh.count = items.length;
  mesh.instanceMatrix.needsUpdate = true;
  if (mesh.instanceColor) mesh.instanceColor.needsUpdate = true;
  return mesh;
}

/** Dispose of everything under an object (geometries, materials, textures). */
export function disposeTree(root) {
  root.traverse((o) => {
    if (o.geometry && o.geometry !== unit) o.geometry.dispose();
    const mats = Array.isArray(o.material) ? o.material : o.material ? [o.material] : [];
    for (const mat of mats) {
      mat.map?.dispose?.();
      mat.dispose?.();
    }
    o.dispose?.();
  });
}

/** Deterministic pseudo-random numbers, so the city is the same every visit. */
export function rng(seed = 1) {
  let s = seed >>> 0;
  return () => {
    s = (s * 1664525 + 1013904223) >>> 0;
    return s / 4294967296;
  };
}
