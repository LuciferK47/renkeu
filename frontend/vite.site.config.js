// Builds only the landing page, as a static site (for GitHub Pages or any
// static host): `BASE_PATH=/Latex-AI/ npm run build:site`. Output: dist-site/.
import { defineConfig } from 'vite';
import { svelte } from '@sveltejs/vite-plugin-svelte';
import path from 'node:path';

const here = import.meta.dirname;

export default defineConfig({
  root: path.resolve(here, 'site'),
  base: process.env.BASE_PATH ?? '/',
  publicDir: path.resolve(here, 'public'),
  plugins: [svelte()],
  build: { outDir: path.resolve(here, 'dist-site'), emptyOutDir: true }
});
