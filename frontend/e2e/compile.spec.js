import { test, expect, openApp, openFile, compileOk, cursorToLineStart } from './helpers.js'

test.describe('compiling and the PDF', () => {
  test('compiling shows the pages and says it compiled', async ({ page }) => {
    await openApp(page)
    await openFile(page, 'main.tex')
    await compileOk(page)
    await expect(page.locator('.status-bar')).toContainText('Compiled')
    await expect(page.getByRole('region', { name: 'PDF preview' }).getByRole('link', { name: 'Export PDF' })).toBeVisible()
    await expect(page.locator('.slot img').first()).toHaveJSProperty('complete', true)
  })

  test('an error is listed with its file and line, and clicking it goes there', async ({ page }) => {
    await openApp(page)
    await openFile(page, 'main.tex')
    await cursorToLineStart(page, 'The quick brown fox')
    await page.keyboard.type('\\oops ')
    // Wherever the cursor landed is the line TeX should complain about.
    const line = (await page.locator('.status-bar').innerText()).match(/Ln (\d+)/)[1]
    await page.keyboard.press('Control+Enter')

    const problem = page.getByRole('button', { name: /error:.*Undefined control sequence/ })
    await expect(problem).toBeVisible({ timeout: 120_000 })
    await expect(problem).toContainText(`main.tex:${line}`)
    // It is also drawn on the line itself.
    await expect(page.locator('.squiggly-error').first()).toBeVisible()

    await cursorToLineStart(page, '\\documentclass')
    await problem.click()
    await expect(page.locator('.status-bar')).toContainText(`Ln ${line}`)
  })

  test('the last PDF is still there after a reload, without compiling again', async ({ page }) => {
    await openApp(page)
    await openFile(page, 'main.tex')
    await compileOk(page)
    await page.reload()
    await expect(page.locator('.slot img').first()).toBeVisible({ timeout: 30_000 })
  })

  test('Ctrl+wheel zooms the page', async ({ page }) => {
    await openApp(page)
    await openFile(page, 'main.tex')
    await compileOk(page)
    const zoom = page.locator('.controls .zoom')
    const before = await zoom.innerText()
    await page.locator('.scroller').hover()
    await page.keyboard.down('Control')
    await page.mouse.wheel(0, -300)
    await page.keyboard.up('Control')
    await expect(zoom).not.toHaveText(before)
  })

  // A short window, so the chapter text is below the fold until scrolled to.
  test.describe('in a short window', () => {
    test.use({ viewport: { width: 1280, height: 560 } })

    test('Find line flashes where the cursor line landed, and a double-click goes back to the source', async ({ page }) => {
      await openApp(page)
      await openFile(page, 'main.tex')
      await compileOk(page)

      // Line 4 is the sentence in the main file.
      await cursorToLineStart(page, 'The quick brown fox')
      await page.keyboard.press('Control+Alt+j')
      await expect(page.locator('.hit')).toBeVisible()

      // Double-click the text that came from chapters/one.tex.
      const build = (await page.locator('.slot img').first().getAttribute('src')).match(/build=([0-9a-f]+)/)[1]
      const fwd = await (await page.request.get(`/api/synctex/forward?file=chapters/one.tex&line=2&build=${build}`)).json()
      const r = fwd.region
      const zoom = Number((await page.locator('.controls .zoom').innerText()).replace('%', '')) / 100
      const pxPerPt = (zoom * 96) / 72

      // Bring the target into the visible part of the pane first: where it falls
      // depends on the window size, and a click outside the pane hits something else.
      const scroller = page.locator('.scroller')
      await scroller.evaluate((el, top) => (el.scrollTop = top), Math.max(0, (r.y + r.height / 2) * pxPerPt - 120))
      const img = await page.locator('.slot img').first().boundingBox()
      const pane = await scroller.boundingBox()
      const x = img.x + (r.x + r.width / 2) * pxPerPt
      const y = img.y + (r.y + r.height / 2) * pxPerPt
      expect(y).toBeGreaterThan(pane.y)
      expect(y).toBeLessThan(pane.y + pane.height)
      await page.mouse.dblclick(x, y)

      await expect(page.getByRole('tab', { name: 'one.tex' })).toHaveAttribute('aria-selected', 'true')
      await expect(page.locator('.status-bar')).toContainText('Ln 2')
    })
  })
})
