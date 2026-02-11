import { vitePreprocess } from '@sveltejs/vite-plugin-svelte'

export default {
  // Consult https://svelte.dev/docs#compile-time-svelte-preprocess
  // for more information about preprocessors
  preprocess: vitePreprocess(),
  
  compilerOptions: {
    // Enable Svelte 5 runes mode (opt-in for new features)
    // Set to false to use compatibility mode for Svelte 4 components
    runes: undefined  // Auto-detect based on component syntax
  }
}
