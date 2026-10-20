// Small canvas textures: the CRT screen, paper pages, the billboard, labels.
// Drawn once at low resolution and sampled with nearest filtering, so they keep
// the pixel look. Nothing is loaded from the network.

import * as THREE from 'three';

function canvas(w, h) {
  const c = document.createElement('canvas');
  c.width = w;
  c.height = h;
  return c;
}

function texture(c, { repeat = false } = {}) {
  const t = new THREE.CanvasTexture(c);
  t.magFilter = THREE.NearestFilter;
  t.minFilter = THREE.NearestFilter;
  t.generateMipmaps = false;
  t.colorSpace = THREE.SRGBColorSpace;
  if (repeat) t.wrapS = THREE.RepeatWrapping;
  return t;
}

const MONO = '8px "JetBrains Mono", monospace';

/** A page of typeset text. `state`: 'broken' (a red glitch) or 'fixed' (clean). */
export function paperTexture(state = 'fixed') {
  const c = canvas(48, 64);
  const g = c.getContext('2d');
  g.fillStyle = '#f3eee3';
  g.fillRect(0, 0, 48, 64);
  g.fillStyle = '#2a2d31';
  g.fillRect(8, 6, 22, 3); // title rule
  g.fillStyle = '#8c8a83';
  for (let y = 16; y < 56; y += 4) g.fillRect(8, y, y % 12 === 0 ? 24 : 32, 1);
  g.fillStyle = '#2a2d31';
  g.fillRect(16, 38, 16, 2); // equation
  if (state === 'broken') {
    g.fillStyle = '#ff3b4e';
    g.fillRect(8, 24, 32, 3);
    g.fillRect(14, 22, 5, 7);
    g.fillRect(28, 30, 10, 2);
  } else {
    g.fillStyle = '#6ee7a0';
    g.fillRect(36, 56, 6, 2);
    g.fillRect(34, 54, 2, 2);
  }
  return texture(c);
}

/** What the CRT shows. `tab`: 'project' | 'sync' | 'review'. */
export function screenTexture(tab = 'project') {
  const c = canvas(192, 120);
  const g = c.getContext('2d');
  g.fillStyle = '#0b1114';
  g.fillRect(0, 0, 192, 120);
  g.fillStyle = '#1e2a30';
  g.fillRect(0, 0, 192, 9);
  g.fillStyle = '#e2512f';
  g.fillRect(150, 2, 38, 5);
  // file tree
  g.fillStyle = '#10181c';
  g.fillRect(0, 9, 34, 111);
  ['main.tex', 'chapters/', ' one.tex', 'refs.bib', 'plot.png'].forEach((name, i) => {
    g.fillStyle = tab === 'project' && i === 1 ? '#3fe0d0' : '#7f9aa6';
    g.font = MONO;
    g.fillText(name, 3, 22 + i * 10);
  });
  // source
  const code = ['\\documentclass{article}', '\\usepackage{amsmath}', '\\begin{document}', '\\section{Intro}', 'The \\oops sum', '\\end{document}'];
  code.forEach((line, i) => {
    g.font = MONO;
    g.fillStyle = i === 4 ? '#ff3b4e' : '#a6c4d0';
    g.fillText(line, 38, 20 + i * 10);
  });
  if (tab === 'review') {
    g.fillStyle = '#3a1219';
    g.fillRect(38, 36, 70, 9);
    g.fillStyle = '#12301f';
    g.fillRect(38, 46, 70, 9);
    g.font = MONO;
    g.fillStyle = '#ff3b4e';
    g.fillText('- The \\oops sum', 40, 43);
    g.fillStyle = '#6ee7a0';
    g.fillText('+ The \\sum', 40, 53);
    g.fillStyle = '#e2512f';
    g.fillRect(40, 62, 54, 8);
    g.fillStyle = '#14161a';
    g.fillText('Apply 1 change', 42, 69);
  }
  // PDF pane
  g.fillStyle = '#243038';
  g.fillRect(112, 9, 80, 111);
  g.fillStyle = '#f3eee3';
  g.fillRect(120, 16, 64, 96);
  g.fillStyle = '#2a2d31';
  g.fillRect(126, 22, 30, 3);
  g.fillStyle = '#8c8a83';
  for (let y = 32; y < 100; y += 5) g.fillRect(126, y, 46, 1);
  if (tab === 'sync') {
    g.fillStyle = 'rgba(63,224,208,0.55)';
    g.fillRect(124, 52, 56, 7);
    g.fillStyle = '#3fe0d0';
    g.fillRect(38, 46, 70, 1);
  }
  return texture(c);
}

/** A strip of LaTeX that scrolls along the rooftop billboard. */
export function billboardTexture() {
  const c = canvas(512, 64);
  const g = c.getContext('2d');
  g.fillStyle = '#04191b';
  g.fillRect(0, 0, 512, 64);
  g.font = 'bold 20px "JetBrains Mono", monospace';
  g.fillStyle = '#3fe0d0';
  g.fillText('\\begin{document}', 6, 26);
  g.fillStyle = '#ff8a3d';
  g.fillText('\\section{Compile locally.}', 6, 54);
  g.fillStyle = '#3fe0d0';
  g.fillText('\\end{document}', 290, 26);
  g.fillStyle = '#e2512f';
  g.fillText('% keep the project yours', 262, 54);
  g.fillStyle = 'rgba(0,0,0,0.25)';
  for (let y = 0; y < 64; y += 3) g.fillRect(0, y, 512, 1);
  return texture(c, { repeat: true });
}

/** A text label on a plane. */
export function labelTexture(text, { color = '#f3eee3', bg = 'rgba(0,0,0,0)', width = 160, height = 32, size = 14 } = {}) {
  const c = canvas(width, height);
  const g = c.getContext('2d');
  g.fillStyle = bg;
  g.fillRect(0, 0, width, height);
  g.font = `bold ${size}px "JetBrains Mono", monospace`;
  g.textAlign = 'center';
  g.textBaseline = 'middle';
  g.fillStyle = color;
  g.fillText(text, width / 2, height / 2 + 1);
  return texture(c);
}

/** Letters stacked top to bottom, glowing, for a vertical neon blade sign. */
export function bladeTexture(letters, color) {
  const w = 32;
  const h = letters.length * 36;
  const c = canvas(w, h);
  const g = c.getContext('2d');
  g.fillStyle = '#12060f';
  g.fillRect(0, 0, w, h);
  g.strokeStyle = color;
  g.lineWidth = 2;
  g.strokeRect(1, 1, w - 2, h - 2);
  g.font = 'bold 26px "Pixelify Sans", "JetBrains Mono", monospace';
  g.textAlign = 'center';
  g.textBaseline = 'middle';
  letters.forEach((ch, i) => {
    g.fillStyle = 'rgba(255,255,255,0.25)';
    g.fillText(ch, w / 2 + 1, i * 36 + 19);
    g.fillStyle = color;
    g.fillText(ch, w / 2, i * 36 + 18);
  });
  return texture(c);
}

/** A translucent hologram: faint scanlines, a frame and a word. */
export function hologramTexture(text, color) {
  const c = canvas(128, 72);
  const g = c.getContext('2d');
  g.clearRect(0, 0, 128, 72);
  g.strokeStyle = color;
  g.lineWidth = 2;
  g.strokeRect(2, 2, 124, 68);
  g.fillStyle = color;
  g.globalAlpha = 0.18;
  for (let y = 4; y < 70; y += 3) g.fillRect(4, y, 120, 1);
  g.globalAlpha = 1;
  g.font = 'bold 22px "Pixelify Sans", "JetBrains Mono", monospace';
  g.textAlign = 'center';
  g.textBaseline = 'middle';
  g.fillText(text, 64, 30);
  g.font = '10px "JetBrains Mono", monospace';
  g.fillText('\\documentclass', 64, 54);
  return texture(c);
}
