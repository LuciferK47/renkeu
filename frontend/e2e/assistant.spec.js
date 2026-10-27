// The assistant, against a scripted event stream: the real server needs an LLM
// key, but everything the page does with the stream (timeline, review, apply,
// stop) is deterministic.
import { test, expect, openApp, openFile } from './helpers.js'
import { read } from './fixture.js'

const sse = (events) => events.map(([event, data]) => `event: ${event}\ndata: ${JSON.stringify(data)}\n\n`).join('')

/** Give the page a (fake) key so the composer is enabled, and remove it afterwards. */
async function withKey(page, run) {
  await page.getByRole('button', { name: 'Open settings' }).click()
  await page.getByRole('tab', { name: 'Assistant' }).click()
  await page.getByLabel('Groq API key').fill('gsk_e2e_not_a_real_key')
  await page.getByRole('button', { name: 'Save key' }).click()
  await page.getByText('Key saved.').waitFor()
  await expect(page.getByLabel('Replace key')).toHaveValue('') // write-only: never shown again
  await page.getByRole('button', { name: 'Close settings' }).click()
  try {
    await run()
  } finally {
    await page.request.delete('http://127.0.0.1:3000/api/config/api-key')
  }
}

test.describe('assistant', () => {
  test('a run shows its steps, then the proposal; applying writes the file', async ({ page }) => {
    await openApp(page)
    await openFile(page, 'main.tex')

    await withKey(page, async () => {
      const file = await (await page.request.get('/api/file?path=main.tex')).json()
      const edit = { file_path: 'main.tex', op: 'replace_range', start_line: 8, end_line: 8, text: 'Closing words.', insert_text: 'Closing words, reviewed.' }
      const preview = await (await page.request.post('/api/agent/preview', { data: { file_path: 'main.tex', edits: [edit] } })).json()
      const proposed = { ...edit, hunk_id: preview.hunks[0].hunk_id }

      await page.route('**/api/agent/stream', (route) =>
        route.fulfill({
          status: 200,
          contentType: 'text/event-stream',
          body: sse([
            ['thinking', { iteration: 1, message: 'reading the file' }],
            ['step', { iteration: 1, edit_count: 1 }],
            ['applying', { message: 'on a copy' }],
            ['compiled', { success: false, error_summary: 'Undefined control sequence' }],
            ['thinking', { iteration: 2, message: 'trying again' }],
            ['compiled', { success: true }],
            ['proposal', { verified: true, edit_count: 1, edits: [proposed], etag: file.etag, files: [{ path: 'main.tex', etag: file.etag, edit_count: 1 }] }],
            ['awaiting_approval', {}],
            ['done', { success: true, awaiting_approval: true }]
          ])
        })
      )

      const box = page.getByLabel('Message the assistant')
      await box.fill('make the ending friendlier')
      await box.press('Enter')

      const steps = page.getByRole('list', { name: 'What the assistant did' })
      await expect(steps).toContainText('It did not compile')
      await expect(steps).toContainText('Undefined control sequence')
      await expect(steps).toContainText('It compiles')
      await expect(steps).toContainText('Ready for your review.')

      const review = page.getByRole('region', { name: 'Proposed changes' })
      await expect(review).toContainText('main.tex')
      await expect(review.locator('.diff')).toContainText('+Closing words, reviewed.')
      await review.getByRole('button', { name: /Apply 1 change/ }).click()

      await expect.poll(() => read('main.tex')).toContain('Closing words, reviewed.')
      await expect(review).toHaveCount(0)
    })
  })

  test('Stop ends a run that never answers, and the next request can be sent', async ({ page }) => {
    await openApp(page)
    await openFile(page, 'main.tex')

    await withKey(page, async () => {
      let aborted = false
      page.on('requestfailed', (r) => r.url().includes('agent/stream') && (aborted = true))
      await page.route('**/api/agent/stream', () => {
        /* never answered */
      })

      const assistant = page.getByRole('region', { name: 'Assistant' })
      const box = page.getByLabel('Message the assistant')
      await box.fill('this will hang')
      await box.press('Enter')
      await assistant.getByRole('button', { name: 'Stop' }).click()

      await expect(assistant.getByText('Stopped.')).toBeVisible()
      await expect(assistant.getByRole('button', { name: 'Send' })).toBeVisible()
      expect(aborted).toBe(true)
    })
  })

  test('what the model writes is shown as text, never run as markup', async ({ page }) => {
    await openApp(page)
    await openFile(page, 'main.tex')

    await withKey(page, async () => {
      await page.route('**/api/agent/stream', (route) =>
        route.fulfill({
          status: 200,
          contentType: 'text/event-stream',
          body: sse([['error', { message: '<img src=x onerror="window.__pwned=1">' }], ['done', { success: false, message: 'x' }]])
        })
      )
      const box = page.getByLabel('Message the assistant')
      await box.fill('anything')
      await box.press('Enter')
      await expect(page.getByRole('list', { name: 'What the assistant did' })).toContainText('<img src=x')
      expect(await page.evaluate(() => window.__pwned)).toBeUndefined()
      await expect(page.getByRole('region', { name: 'Assistant' }).locator('img')).toHaveCount(0)
    })
  })
})
