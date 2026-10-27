// The first-run screen: shown while the server says setup is not done, offers the
// assistant choices, and remembers that it was dealt with.
import { test, expect } from './helpers.js'

test('the welcome screen prepares TeX, offers a local model and is remembered', async ({ page }) => {
  let done = false
  let finished = 0
  await page.route('**/api/setup', (route) => {
    if (route.request().method() === 'PUT') {
      done = true
      finished += 1
    }
    return route.fulfill({ json: { done } })
  })
  await page.route('**/api/setup/prepare', (route) => route.fulfill({ json: { ok: true, message: null } }))
  await page.route('**/api/ai/local', (route) =>
    route.fulfill({ json: { running: true, models: [{ name: 'qwen2.5:7b', size_bytes: 4_700_000_000 }] } })
  )

  await page.goto('/#/editor')
  const dialog = page.getByRole('dialog', { name: 'Welcome to Renkeu' })
  await expect(dialog).toBeVisible()
  await expect(dialog.getByText('Ready. Compiling works')).toBeVisible()

  await dialog.getByLabel(/A model on this computer/).check()
  await expect(dialog.getByText(/Ollama is running with 1 model/)).toBeVisible()
  await expect(dialog.getByRole('option', { name: /qwen2.5:7b/ })).toBeAttached()

  await dialog.getByRole('button', { name: 'Start writing' }).click()
  await expect(dialog).toBeHidden()
  expect(finished).toBe(1)

  // Dealt with: a reload does not show it again.
  await page.reload()
  await expect(page.getByRole('dialog', { name: 'Welcome to Renkeu' })).toBeHidden()
})

test('a failed TeX download shows the reason and can be retried', async ({ page }) => {
  let calls = 0
  await page.route('**/api/setup', (route) => route.fulfill({ json: { done: false } }))
  await page.route('**/api/setup/prepare', (route) => {
    calls += 1
    return route.fulfill({ json: calls === 1 ? { ok: false, message: 'Check your internet connection.' } : { ok: true, message: null } })
  })
  await page.route('**/api/ai/local', (route) => route.fulfill({ json: { running: false, models: [] } }))

  await page.goto('/#/editor')
  const dialog = page.getByRole('dialog', { name: 'Welcome to Renkeu' })
  await expect(dialog.getByText('Check your internet connection.')).toBeVisible()
  await dialog.getByRole('button', { name: 'Try again' }).click()
  await expect(dialog.getByText('Ready. Compiling works')).toBeVisible()

  await dialog.getByLabel(/A model on this computer/).check()
  await expect(dialog.getByText(/Ollama is not running/)).toBeVisible()
})
