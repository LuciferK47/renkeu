// Fitting a page limit, against a scripted stream: the real server needs an LLM
// key, but what the page sends and shows (the dialog, the request, the pages
// before and after) is deterministic. The server side is covered in Rust.
import { test, expect, openApp, openFile, compileOk } from './helpers.js'
import { write } from './fixture.js'

const sse = (events) => events.map(([event, data]) => `event: ${event}\ndata: ${JSON.stringify(data)}\n\n`).join('')

const THREE_PAGES = [
  '\\documentclass{article}',
  '\\begin{document}',
  'First page text.',
  '',
  '\\newpage',
  'Second page text.',
  '\\newpage',
  'Third page text.',
  '\\end{document}',
  ''
].join('\n')

async function withKey(page, run) {
  await page.getByRole('button', { name: 'Open settings' }).click()
  await page.getByRole('tab', { name: 'Assistant' }).click()
  await page.getByLabel('Groq API key').fill('gsk_e2e_not_a_real_key')
  await page.getByRole('button', { name: 'Save key' }).click()
  await page.getByText('Key saved.').waitFor()
  await page.getByRole('button', { name: 'Close settings' }).click()
  try {
    await run()
  } finally {
    await page.request.delete('http://127.0.0.1:3000/api/config/api-key')
  }
}

test('the layout tab offers to fit an over-long document, and the review shows pages before and after', async ({ page }) => {
  write('main.tex', THREE_PAGES)
  await openApp(page)
  await openFile(page, 'main.tex')
  await compileOk(page)

  await withKey(page, async () => {
    const edit = { file_path: 'main.tex', op: 'replace_range', start_line: 6, end_line: 6, text: 'Second page text.', insert_text: 'Second page.' }
    const preview = await (await page.request.post('/api/agent/preview', { data: { file_path: 'main.tex', edits: [edit] } })).json()
    const file = await (await page.request.get('/api/file?path=main.tex')).json()
    const proposed = { ...edit, hunk_id: preview.hunks[0].hunk_id }

    let sent = null
    await page.route('**/api/agent/stream', (route) => {
      sent = route.request().postDataJSON()
      return route.fulfill({
        status: 200,
        contentType: 'text/event-stream',
        body: sse([
          ['progress', { message: '3 pages, limit 2. Looking for lines to save...' }],
          ['thinking', { iteration: 1, message: 'planning' }],
          ['compiled', { success: false, pages: 3, layout: true, error_summary: 'The document compiles to 3 pages; the limit is 2.' }],
          ['thinking', { iteration: 2, message: 'planning' }],
          ['compiled', { success: true }],
          ['proposal', { verified: true, edit_count: 1, edits: [proposed], etag: file.etag, files: [{ path: 'main.tex', etag: file.etag, edit_count: 1 }], fit: { limit: 2, pages_before: 3, pages_after: 2 } }],
          ['awaiting_approval', {}],
          ['done', { success: true, awaiting_approval: true }]
        ])
      })
    })

    await page.getByRole('tab', { name: 'Layout' }).click()
    await page.getByLabel('Page limit').fill('2')
    await page.getByRole('button', { name: /Fit to 2 pages with the assistant/ }).click()

    const dialog = page.getByRole('dialog', { name: 'Fit to a page limit' })
    await expect(dialog.getByLabel('Page limit')).toHaveValue('2')
    await dialog.getByLabel(/Anything to keep/).fill('keep the abstract')
    await dialog.getByRole('button', { name: 'Fit to 2 pages' }).click()

    const steps = page.getByRole('list', { name: 'What the assistant did' })
    await expect(steps).toContainText('Still 3 pages')
    await expect(steps).toContainText('Proposed 1 edit · 3 → 2 pages')
    await expect(page.getByRole('region', { name: 'Proposed changes' })).toContainText('3 → 2 pages (limit 2)')
    expect(sent).toMatchObject({ file_path: 'main.tex', pages: 2, instruction: 'keep the abstract', max_iterations: 6 })
  })
})

test('the command menu lists the fit command', async ({ page }) => {
  await openApp(page)
  await page.keyboard.press('F1')
  await page.getByPlaceholder(/command/i).fill('page limit')
  await expect(page.getByText('Fit the document to a page limit…')).toBeVisible()
})
