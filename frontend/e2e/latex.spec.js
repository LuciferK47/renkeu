import { test, expect, openApp, openFile, endOfEditor } from './helpers.js'

test.describe('writing LaTeX', () => {
  test('commands, math and comments are highlighted differently', async ({ page }) => {
    await openApp(page)
    await openFile(page, 'main.tex')
    const classes = await page.locator('.view-line').first().locator('span span').evaluateAll((els) => els.map((e) => e.className.split(' ')[0]))
    expect(new Set(classes).size).toBeGreaterThan(1) // \documentclass, braces and text are not one colour
  })

  test('the outline lists the headings, and clicking one moves the cursor there', async ({ page }) => {
    await openApp(page)
    await openFile(page, 'main.tex')
    await expect(page.getByRole('region', { name: 'Outline' }).getByRole('button', { name: 'Introduction' })).toBeVisible()
    await page.getByRole('region', { name: 'Outline' }).getByRole('button', { name: 'Introduction' }).click()
    await expect(page.locator('.status-bar')).toContainText('Ln 3')
  })

  test('typing \\cite{ offers the keys from the .bib file, with author and year', async ({ page }) => {
    await openApp(page)
    await openFile(page, 'main.tex')
    await endOfEditor(page)
    await page.keyboard.press('Enter')
    await page.keyboard.type('\\cite{kn', { delay: 20 })
    const row = page.locator('.suggest-widget .monaco-list-row').first()
    await expect(row).toContainText('knuth84')
    await expect(row).toContainText('Knuth')
    await page.keyboard.press('Enter')
    await expect(page.locator('.view-line').last()).toContainText('\\cite{knuth84}')
  })

  test('\\ref{ offers labels from every file, including ones that are not open', async ({ page }) => {
    await openApp(page)
    await openFile(page, 'main.tex')
    await endOfEditor(page)
    await page.keyboard.press('Enter')
    await page.keyboard.type('\\ref{sec:', { delay: 20 })
    const rows = page.locator('.suggest-widget .monaco-list-row')
    await expect(rows.filter({ hasText: 'sec:intro' })).toBeVisible()
    await expect(rows.filter({ hasText: 'sec:one' })).toBeVisible()
  })

  test('a misspelt word is underlined and can be fixed from the quick-fix menu', async ({ page }) => {
    await openApp(page)
    await openFile(page, 'main.tex')
    await endOfEditor(page)
    await page.keyboard.press('Enter')
    await page.keyboard.type('A sentense with a typo.')
    await expect(page.locator('.squiggly-info').first()).toBeVisible({ timeout: 30_000 })

    await page.keyboard.press('Home')
    for (let i = 0; i < 5; i++) await page.keyboard.press('ArrowRight') // inside "sentense"
    await page.keyboard.press('Control+.')
    await expect(page.getByText(/Change to “sentence”/)).toBeVisible()
    await page.keyboard.press('Enter') // the preferred fix is highlighted
    await expect(page.locator('.view-line').last()).toContainText('A sentence with a typo.')
  })

  test('Ctrl+click on a citation opens the .bib entry, even though that file was not open', async ({ page }) => {
    await openApp(page)
    await openFile(page, 'main.tex')
    await endOfEditor(page)
    await page.keyboard.press('Enter')
    await page.keyboard.type('\\cite{knuth84}')
    const at = await page.evaluate(() => {
      for (const line of document.querySelectorAll('.view-line')) {
        const w = document.createTreeWalker(line, NodeFilter.SHOW_TEXT)
        for (let n = w.nextNode(); n; n = w.nextNode()) {
          const i = n.textContent.indexOf('knuth84')
          if (i >= 0) {
            const r = document.createRange()
            r.setStart(n, i + 1)
            r.setEnd(n, i + 3)
            const b = r.getBoundingClientRect()
            return { x: b.x + b.width / 2, y: b.y + b.height / 2 }
          }
        }
      }
      return null
    })
    await page.keyboard.down('Control')
    await page.mouse.click(at.x, at.y)
    await page.keyboard.up('Control')
    await expect(page.getByRole('tab', { name: 'refs.bib' })).toHaveAttribute('aria-selected', 'true')
  })
})
