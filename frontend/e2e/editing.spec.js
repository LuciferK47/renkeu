import { test, expect, openApp, openFile, endOfEditor, editorLines } from './helpers.js'
import { read, write } from './fixture.js'

test.describe('editing', () => {
  test('typing is saved to disk by itself, and the tab says so', async ({ page }) => {
    await openApp(page)
    await openFile(page, 'main.tex')
    await endOfEditor(page)
    await page.keyboard.type('\n% typed in the browser')

    await expect(page.getByRole('tab', { name: /main\.tex/ })).toContainText('●') // unsaved at first
    await expect.poll(() => read('main.tex')).toContain('% typed in the browser')
    await expect(page.getByRole('tab', { name: /main\.tex/ })).not.toContainText('●')
  })

  test('each tab keeps its own text, and switching never writes one file into another', async ({ page }) => {
    await openApp(page)
    await openFile(page, 'main.tex')
    await endOfEditor(page)
    await page.keyboard.type('\n% only in main')
    await page.getByRole('treeitem', { name: /chapters/ }).click()
    await openFile(page, 'chapters/one.tex')
    await endOfEditor(page)
    await page.keyboard.type('\n% only in one')

    await expect.poll(() => read('chapters/one.tex')).toContain('% only in one')
    await expect.poll(() => read('main.tex')).toContain('% only in main')
    expect(read('main.tex')).not.toContain('only in one')
    expect(read('chapters/one.tex')).not.toContain('only in main')

    await page.getByRole('tab', { name: 'main.tex' }).click()
    expect((await editorLines(page)).join('\n')).toContain('% only in main')
  })

  test('a change made on disk while you have unsaved text asks instead of overwriting', async ({ page }) => {
    await openApp(page)
    // Autosave off, so the text stays unsaved while the file changes underneath.
    await page.getByRole('button', { name: 'Settings', exact: true }).click()
    await page.getByRole('tab', { name: 'Editor' }).click()
    await page.getByLabel('Save automatically after a pause').uncheck()
    await page.getByRole('button', { name: 'Close settings' }).click()

    await openFile(page, 'main.tex')
    await endOfEditor(page)
    await page.keyboard.type('\n% my unsaved line')
    write('main.tex', '% rewritten elsewhere\n')
    await page.evaluate(() => window.dispatchEvent(new Event('focus')))

    const dialog = page.getByRole('alertdialog')
    await expect(dialog).toContainText('main.tex changed on disk')
    expect(read('main.tex')).toBe('% rewritten elsewhere\n') // nothing was overwritten

    await dialog.getByRole('button', { name: 'Load the file from disk' }).click()
    await expect.poll(async () => (await editorLines(page)).join('\n')).toContain('% rewritten elsewhere')
    expect((await editorLines(page)).join('\n')).not.toContain('my unsaved line')
  })

  test('an unchanged file is not marked as conflicting just because the window regains focus', async ({ page }) => {
    await openApp(page)
    await openFile(page, 'main.tex')
    await page.evaluate(() => window.dispatchEvent(new Event('focus')))
    await page.waitForTimeout(500)
    await expect(page.getByRole('alertdialog')).toHaveCount(0)
  })

  test('the open tabs come back after a reload', async ({ page }) => {
    await openApp(page)
    await openFile(page, 'main.tex')
    await page.getByRole('treeitem', { name: /chapters/ }).click()
    await openFile(page, 'chapters/one.tex')
    await page.reload()
    await expect(page.getByRole('tab', { name: 'main.tex' })).toBeVisible()
    await expect(page.getByRole('tab', { name: 'one.tex' })).toBeVisible()
    await expect(page.getByRole('tab', { name: 'one.tex' })).toHaveAttribute('aria-selected', 'true')
  })
})
