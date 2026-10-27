import { vitePreprocess } from '@sveltejs/vite-plugin-svelte'

export default {
  preprocess: vitePreprocess(),

  vitePlugin: {
    // Runes everywhere in this project, so nothing can quietly fall back to the
    // old `export let` / `$:` / store-subscription semantics. Dependencies are
    // left alone: svelte-splitpanes still ships legacy-syntax components, which
    // are not ours to force into runes mode.
    dynamicCompileOptions({ filename }) {
      return filename.includes('node_modules') ? undefined : { runes: true }
    }
  }
}
