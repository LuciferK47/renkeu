// End-to-end tests: a real browser against the real server and the built app.
//
//   npm run build && npm run e2e
//
// Needs the server binary (cargo build -p latex-ai-server; override the path
// with E2E_SERVER_BIN) and, for the PDF preview, libpdfium on the loader path or
// in E2E_PDFIUM_DIR. E2E_CHROME points at an installed Chrome instead of
// Playwright's own Chromium.
import { defineConfig } from '@playwright/test'
import path from 'node:path'

const here = import.meta.dirname
const bin = path.resolve(here, process.env.E2E_SERVER_BIN ?? '../target/debug/latex-ai')
const workspace = path.resolve(here, 'e2e/.workspace')
const home = path.resolve(here, 'e2e/.home')
const pdfium = process.env.E2E_PDFIUM_DIR ? { LATEX_AI_PDFIUM_DIR: process.env.E2E_PDFIUM_DIR } : {}

export default defineConfig({
  testDir: './e2e',
  // One server, one workspace on disk: tests take turns.
  workers: 1,
  fullyParallel: false,
  timeout: 150_000,
  expect: { timeout: 20_000 },
  retries: process.env.CI ? 1 : 0,
  reporter: process.env.CI ? [['list'], ['html', { open: 'never' }]] : 'list',
  use: {
    baseURL: 'http://localhost:4173',
    trace: 'retain-on-failure',
    screenshot: 'only-on-failure',
    launchOptions: process.env.E2E_CHROME ? { executablePath: process.env.E2E_CHROME, args: ['--no-sandbox'] } : { args: ['--no-sandbox'] }
  },
  webServer: [
    {
      // Fresh workspace and data directory every run, then the real server.
      command: `node e2e/prepare.mjs && cd "${workspace}" && LATEX_AI_HOME="${home}" "${bin}" serve --port 3000`,
      url: 'http://127.0.0.1:3000/health',
      env: pdfium,
      reuseExistingServer: false,
      timeout: 120_000
    },
    {
      command: 'npx vite preview --port 4173 --strictPort',
      url: 'http://localhost:4173',
      reuseExistingServer: false,
      timeout: 60_000
    }
  ]
})
