import { defineConfig } from 'vite'
import { svelte } from '@sveltejs/vite-plugin-svelte'

import path from 'path';

export default defineConfig({
  plugins: [svelte()],
  resolve: {
    alias: {
      // svelte-splitpanes imports SvelteKit's `$app/environment` for its `browser`
      // flag. There is no SvelteKit here, so it is answered by a one-line stand-in.
      '$app/environment': path.resolve(__dirname, './src/app-env-mock.js'),
      // dictionary-en's package "exports" only exposes a Node loader; the browser
      // fetches the two Hunspell files directly.
      'dictionary-en-files': path.resolve(__dirname, './node_modules/dictionary-en')
    }
  },
  server: {
    host: '127.0.0.1',
    port: 5173,
    proxy: {
      '/api': {
        // Point dev proxy to the Rust axum backend (default port 3000)
        target: 'http://localhost:3000',
        changeOrigin: true
      },
      '/ws': {
        target: 'ws://localhost:3000',
        ws: true
      }
    }
  },
  optimizeDeps: {
    exclude: ['$app/environment']
  },
  build: {
    outDir: 'dist',
    emptyOutDir: true,
    chunkSizeWarningLimit: 1500,
    rollupOptions: {
      external: ['fsevents'],
      output: {
        manualChunks: {
          monaco: ['monaco-editor'],
          vendor: ['svelte', '@sveltejs/vite-plugin-svelte']
        }
      }
    }
  },
  worker: {
    // Modern ES module format for web workers (Monaco editor)
    format: 'es'
  }
})
