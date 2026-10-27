// The assistant against a REAL model. Skipped unless E2E_LLM_MODEL is set (a
// model name `genai` routes to a reachable provider, e.g. `qwen2.5:7b` with a
// local Ollama), because a real model is slow and not deterministic. Run:
//
//   E2E_LLM_MODEL=qwen2.5:7b npm run e2e -- --project=chromium assistant-live
//
// So it asserts what must hold whatever the model says: the run ends in a
// definite state, nothing is written until the user accepts, and an accepted
// change can be undone. It does not assert that the model is clever.
import { test, expect, openApp, openFile } from './helpers.js'
import { read, write } from './fixture.js'

const MODEL = process.env.E2E_LLM_MODEL

test.describe('assistant with a real model', () => {
  test.skip(!MODEL, 'set E2E_LLM_MODEL to run against a real model')
  test.setTimeout(15 * 60_000)

  test('a run reaches a definite end, writes nothing until accepted, and an accepted change can be undone', async ({ page }) => {
    const broken = '\\documentclass{article}\n\\begin{document}\nHello \\oops world.\n\\end{document}\n'
    write('main.tex', broken)

    // A key is only what turns the assistant on; a local model ignores it.
    await page.request.put('http://127.0.0.1:3000/api/config/api-key', { data: { key: 'placeholder-not-used-by-local-models' } })
    await page.request.patch('http://127.0.0.1:3000/api/config', { data: { model: MODEL, timeout_secs: 600 } })
    try {
      await openApp(page)
      await openFile(page, 'main.tex')
      const box = page.getByLabel('Message the assistant')
      await box.fill('The document does not compile. Fix it.')
      await box.press('Enter')

      const assistant = page.getByRole('region', { name: 'Assistant' })
      const steps = assistant.getByRole('list', { name: 'What the assistant did' })
      // A definite end: it is ready for review, or it says why it stopped.
      await expect(steps).toContainText(/Ready for your review\.|Stopped|Finished\.|Stopped without a result\./, { timeout: 12 * 60_000 })
      expect(read('main.tex')).toBe(broken) // nothing written yet

      const review = page.getByRole('region', { name: 'Proposed changes' })
      if (!(await review.count())) {
        // The model did not manage this time. That must be said, not hidden.
        await expect(steps).toContainText(/Max iterations|did not|Error|Stopped/i)
        return
      }
      await expect(review.locator('.diff').first()).toBeVisible()
      await review.getByRole('button', { name: /Apply \d+ change/ }).click()
      await expect.poll(() => read('main.tex'), { timeout: 30_000 }).not.toBe(broken)

      // And it can be put back.
      await page.keyboard.press('Alt+h')
      page.once('dialog', (d) => d.accept())
      await page.getByRole('dialog', { name: 'History' }).getByRole('button', { name: 'Undo this change' }).first().click()
      await expect.poll(() => read('main.tex'), { timeout: 30_000 }).toBe(broken)
    } finally {
      await page.request.delete('http://127.0.0.1:3000/api/config/api-key')
    }
  })
})
