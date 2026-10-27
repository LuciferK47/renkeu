// The renderer: builds the scene, draws it small, and scales it up.
//
// The look comes from three steps. RenderPixelatedPass renders the scene into a
// target a fraction of the screen's size, with depth and normal edge lines; the
// result is magnified without smoothing. DitherPass then quantises the colours
// with an ordered (Bayer) pattern aligned to those big pixels. Haze, emissive
// signs and rain do the rest.

import * as THREE from 'three';
import { EffectComposer } from 'three/examples/jsm/postprocessing/EffectComposer.js';
import { RenderPixelatedPass } from 'three/examples/jsm/postprocessing/RenderPixelatedPass.js';
import { OutputPass } from 'three/examples/jsm/postprocessing/OutputPass.js';
import { ShaderPass } from 'three/examples/jsm/postprocessing/ShaderPass.js';
import { createTower } from './rooms.js';
import { createCity } from './city.js';
import { worldAt, KEYFRAMES } from './story.js';
import { settingsFor } from './quality.js';
import { disposeTree } from './voxels.js';
import { PALETTE } from './palette.js';

const DitherShader = {
  uniforms: { tDiffuse: { value: null }, uPixel: { value: 4 }, uLevels: { value: 9 } },
  vertexShader: `
    varying vec2 vUv;
    void main() {
      vUv = uv;
      gl_Position = projectionMatrix * modelViewMatrix * vec4(position, 1.0);
    }`,
  fragmentShader: `
    uniform sampler2D tDiffuse;
    uniform float uPixel;
    uniform float uLevels;
    varying vec2 vUv;
    float bayer(vec2 cell) {
      int x = int(mod(cell.x, 4.0));
      int y = int(mod(cell.y, 4.0));
      float m[16] = float[16](0., 8., 2., 10., 12., 4., 14., 6., 3., 11., 1., 9., 15., 7., 13., 5.);
      return m[x + y * 4] / 16.0;
    }
    void main() {
      vec3 c = texture2D(tDiffuse, vUv).rgb;
      vec2 cell = floor(gl_FragCoord.xy / uPixel);
      c = floor(c * uLevels + 0.5 + (bayer(cell) - 0.5) * 0.55) / uLevels;
      vec2 d = vUv - 0.5;
      c *= 1.0 - dot(d, d) * 0.6;
      c *= mod(cell.y, 3.0) < 0.5 ? 0.96 : 1.0;
      gl_FragColor = vec4(c, 1.0);
    }`
};

const damp = (cur, target, rate, dt) => cur + (target - cur) * (1 - Math.exp(-rate * dt));

/**
 * @param {{ canvas: HTMLCanvasElement, tier: 'full'|'lite' }} options
 */
export function createFoundry({ canvas, tier = 'full' }) {
  const renderer = new THREE.WebGLRenderer({ canvas, antialias: false, powerPreference: 'high-performance' });
  renderer.setPixelRatio(1);
  renderer.toneMapping = THREE.NoToneMapping;

  const scene = new THREE.Scene();
  scene.background = new THREE.Color(PALETTE.void);
  scene.fog = new THREE.FogExp2('#1c0d1a', 0.011);

  const camera = new THREE.PerspectiveCamera(42, 1.6, 0.1, 400);

  scene.add(new THREE.AmbientLight('#8aa0d0', 3.0));
  const moon = new THREE.DirectionalLight('#aac4ff', 2.4);
  moon.position.set(-14, 40, 46);
  scene.add(moon);

  const tower = createTower();
  const city = createCity();
  scene.add(tower.group, city.group);

  let settings = settingsFor(tier, { height: 900 });
  const composer = new EffectComposer(renderer);
  const pixelPass = new RenderPixelatedPass(settings.pixelSize, scene, camera, { normalEdgeStrength: 0.3, depthEdgeStrength: 0.45 });
  const dither = new ShaderPass(DitherShader);
  composer.addPass(pixelPass);
  composer.addPass(new OutputPass());
  composer.addPass(dither);
  city.rain.setVisible(settings.rain);

  // What the visitor is doing, as the scene sees it.
  const state = { position: 0, shown: 0, input: {}, pointer: { x: 0, y: 0 }, pointerSmooth: { x: 0, y: 0 } };
  const lookAt = new THREE.Vector3();
  const eye = new THREE.Vector3();
  let aspect = 1.6;

  function resize() {
    const w = Math.max(2, canvas.clientWidth);
    const h = Math.max(2, canvas.clientHeight);
    aspect = w / h;
    renderer.setSize(w, h, false);
    composer.setSize(w, h);
    camera.aspect = aspect;
    camera.updateProjectionMatrix();
    settings = settingsFor(tier, { height: h });
    pixelPass.setPixelSize(settings.pixelSize);
    dither.uniforms.uPixel.value = settings.pixelSize;
    city.rain.setVisible(settings.rain);
  }

  function frame(dt, t) {
    state.shown = damp(state.shown, state.position, 5, dt);
    state.pointerSmooth.x = damp(state.pointerSmooth.x, state.pointer.x, 4, dt);
    state.pointerSmooth.y = damp(state.pointerSmooth.y, state.pointer.y, 4, dt);

    const world = worldAt(state.shown, state.input);
    const { pos, look } = world.camera;
    // A narrow window needs the camera further back to keep the rooms in frame.
    const back = Math.min(2.4, Math.max(1, 1.45 / aspect));
    // On a tall, narrow screen the text plate takes the lower part of the
    // window: pull the camera toward the tower's axis and look lower, so the
    // scene sits in the strip above the plate.
    const portrait = aspect < 1;
    const squeeze = portrait ? Math.max(0.3, aspect) : 1;
    lookAt.set(look[0] * squeeze, look[1], look[2]);
    eye.set(pos[0] * squeeze, pos[1], pos[2]).sub(lookAt).multiplyScalar(back).add(lookAt);
    if (portrait) {
      const visibleHeight = 2 * eye.distanceTo(lookAt) * Math.tan(THREE.MathUtils.degToRad(camera.fov / 2));
      const drop = visibleHeight * 0.3 * Math.min(1, (1 - aspect) / 0.5);
      eye.y -= drop;
      lookAt.y -= drop;
    }
    // A little parallax from the pointer, strongest on the street.
    const heroWeight = Math.max(0, 1 - state.shown * 2);
    eye.x += state.pointerSmooth.x * 1.6 * heroWeight;
    eye.y += state.pointerSmooth.y * 0.9 * heroWeight;
    camera.position.copy(eye);
    camera.lookAt(lookAt);

    tower.update(dt, t, world);
    city.roof.update(dt, t, state.pointerSmooth);
    city.signs.update(t);
    city.cars.update(dt);
    city.rain.setVisible(settings.rain && world.rain);
    city.rain.update(dt, camera);
    composer.render(dt);
    return world;
  }

  // The loop. It stops when the canvas is off screen or the tab is hidden.
  let raf = 0;
  let last = 0;
  let clock = 0;
  let visible = true;
  let onFrame = null;
  const minStep = 1000 / settings.maxFps;

  function loop(now) {
    raf = requestAnimationFrame(loop);
    if (now - last < minStep - 1) return;
    const dt = Math.min(0.1, (now - last) / 1000 || 0.016);
    last = now;
    clock += dt;
    const world = frame(dt, clock);
    onFrame?.(world);
  }

  function start() {
    if (raf || !visible || document.hidden) return;
    last = performance.now();
    raf = requestAnimationFrame(loop);
  }
  function stop() {
    cancelAnimationFrame(raf);
    raf = 0;
  }
  const onVisibility = () => (document.hidden ? stop() : start());
  document.addEventListener('visibilitychange', onVisibility);

  resize();
  const ro = typeof ResizeObserver === 'function' ? new ResizeObserver(resize) : null;
  ro?.observe(canvas);

  return {
    start,
    stop,
    resize,
    setOnFrame(fn) {
      onFrame = fn;
    },
    setPosition(p) {
      state.position = p;
    },
    setInput(input) {
      state.input = input;
    },
    setPointer(x, y) {
      state.pointer.x = x;
      state.pointer.y = y;
    },
    setVisible(v) {
      visible = v;
      if (v) start();
      else stop();
    },
    /** Jump to a floor with no smoothing and draw it once (for posters). */
    renderStill(position, input = {}) {
      state.position = position;
      state.shown = position;
      state.input = input;
      state.pointerSmooth.x = 0;
      state.pointerSmooth.y = 0;
      return frame(0.016, 1.2);
    },
    dispose() {
      stop();
      ro?.disconnect();
      document.removeEventListener('visibilitychange', onVisibility);
      disposeTree(scene);
      composer.dispose?.();
      renderer.dispose();
    },
    floors: KEYFRAMES.length,
    /** For debugging in the console and for tests; not used by the page. */
    internals: { renderer, scene, camera, composer, dither, pixelPass }
  };
}
