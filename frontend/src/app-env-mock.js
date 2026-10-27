// Stand-in for SvelteKit's `$app/environment`, which svelte-splitpanes imports
// (see the alias in vite.config.js). Only `browser` is used by that library.
export const browser = typeof window !== 'undefined';
export const dev = true;
export const building = false;
export const version = '1.0.0';
