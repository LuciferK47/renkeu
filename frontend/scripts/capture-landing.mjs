// Regenerates the still frames the landing page shows when it cannot draw the
// 3D scene live (no WebGL, reduced motion, data saver): one JPEG per floor of
// the tower, taken from the real scene, plus a screenshot of the real editor.
//
//   npm run dev                      # in one terminal
//   latex-ai serve                   # in a project folder, for the editor shot
//   node scripts/capture-landing.mjs http://127.0.0.1:5173 [--editor]
//
// Set E2E_CHROME to use an installed Chrome instead of Playwright's Chromium.
// SwiftShader renders WebGL without a GPU, so this works on a CI machine too.

import { chromium } from '@playwright/test';
import { mkdirSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const here = path.dirname(fileURLToPath(import.meta.url));
const out = path.resolve(here, '../public/landing');
const base = process.argv[2] ?? 'http://127.0.0.1:5173';
const withEditor = process.argv.includes('--editor');

// One section per floor, in the order the scene's keyframes use. Pinned
// sections are shot a little way in, so the scene is mid-story, not at its start.
const floors = [
  { id: 'hero', into: 0 },
  { id: 'loop', into: 0.1 },
  { id: 'workspace', into: 0.1 },
  { id: 'engine', into: 0.55 },
  { id: 'boundary', into: 0 },
  { id: 'install', into: 0 },
  { id: 'closing', into: 0 }
];

mkdirSync(out, { recursive: true });

const browser = await chromium.launch({
  executablePath: process.env.E2E_CHROME || undefined,
  args: ['--no-sandbox', '--use-gl=angle', '--use-angle=swiftshader', '--enable-unsafe-swiftshader', '--ignore-gpu-blocklist']
});

try {
  const page = await browser.newPage({ viewport: { width: 1280, height: 720 } });
  await page.goto(`${base}/?tier=full#/landing`);
  await page.locator('.scene canvas').waitFor({ timeout: 30000 });
  // Only the scene: the words are real DOM and are not part of the picture.
  await page.addStyleTag({ content: '.landing .hud, .landing main, .landing .foot-bar, .landing .scene .grain { visibility: hidden !important; }' });
  await page.waitForTimeout(2500);

  for (const [i, { id, into }] of floors.entries()) {
    await page.evaluate(
      ({ id, into }) => {
        const el = document.getElementById(id);
        const travel = Math.max(0, el.offsetHeight - innerHeight);
        window.scrollTo({ top: el.getBoundingClientRect().top + scrollY + travel * into, behavior: 'instant' });
      },
      { id, into }
    );
    await page.waitForTimeout(9000); // let the camera settle (slow without a GPU)
    await page.screenshot({ path: path.join(out, `poster-${i}.jpg`), type: 'jpeg', quality: 72 });
    process.stdout.write(`poster ${i} ${id}\n`);
  }

  if (withEditor) {
    const editor = await browser.newPage({ viewport: { width: 1440, height: 810 } });
    await editor.addInitScript(() => localStorage.setItem('latex-ai:ui', JSON.stringify({ theme: 'dark' })));
    await editor.goto(`${base}/#/editor`);
    await editor.getByRole('tree').waitFor();
    await editor.getByRole('treeitem').first().click();
    await editor.locator('.monaco-editor .view-line').first().waitFor();
    await editor.keyboard.press('Control+Enter');
    await editor.locator('.slot img').first().waitFor({ timeout: 120000 });
    await editor.waitForTimeout(800);
    await editor.screenshot({ path: path.join(out, 'editor.jpg'), type: 'jpeg', quality: 80 });
    process.stdout.write('editor\n');
  }
} finally {
  await browser.close();
}
