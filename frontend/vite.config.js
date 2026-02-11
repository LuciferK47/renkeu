import { defineConfig } from 'vite'
import { svelte } from '@sveltejs/vite-plugin-svelte'

export default defineConfig({
  plugins: [svelte()],
  server: {
    port: 5173,
    proxy: {
      '/api': {
        // Point dev proxy to the FastAPI backend (default port 3000)
        target: 'http://localhost:3000',
        changeOrigin: true
      },
      '/ws': {
        target: 'ws://localhost:3000',
        ws: true
      }
    }
  },
  build: {
    outDir: '../src/latex_ai/static',
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
