import { test, expect, openApp, openFile, endOfEditor } from './helpers.js'
import { read } from './fixture.js'

test.describe('settings and history', () => {
  test('the theme applies at once and is still there after a reload', async ({ page }) => {
    await openApp(page)
    await page.getByRole('button', { name: 'Settings', exact: true }).click()
    await page.getByLabel('Theme').selectOption('dark')
    await expect(page.locator('html')).toHaveAttribute('data-theme', 'dark')
    await page.getByRole('button', { name: 'Close settings' }).click()
    await page.reload()
    await expect(page.locator('html')).toHaveAttribute('data-theme', 'dark')
  })

  test('the editor font size setting changes the editor', async ({ page }) => {
    await openApp(page)
    await openFile(page, 'main.tex')
    await page.getByRole('button', { name: 'Settings', exact: true }).click()
    await page.getByRole('tab', { name: 'Editor' }).click()
    await page.getByLabel('Font size').fill('22')
    await page.getByLabel('Font size').dispatchEvent('change')
    await page.getByRole('button', { name: 'Close settings' }).click()
    await expect.poll(() => page.evaluate(() => getComputedStyle(document.querySelector('.view-line span')).fontSize)).toBe('22px')
  })

  test('the assistant says it is off without a key, and Settings is one click away', async ({ page }) => {
    await openApp(page)
    await expect(page.getByRole('note')).toContainText('API key')
    await expect(page.getByLabel('Message the assistant')).toBeDisabled()
    await page.getByRole('button', { name: 'Open settings' }).click()
    await expect(page.getByRole('tab', { name: 'Assistant' })).toBeVisible()
  })

  test('the privacy tab says what is sent and builds a report without project text', async ({ page }) => {
    await openApp(page)
    await page.getByRole('button', { name: 'Settings', exact: true }).click()
    await page.getByRole('tab', { name: 'Privacy' }).click()
    await expect(page.getByText('no telemetry')).toBeVisible()
    await expect(page.getByRole('link', { name: /issues\/new/ })).toBeVisible()
    await page.getByRole('button', { name: 'Copy diagnostics report' }).click()
    const report = page.getByLabel('Diagnostics report')
    await expect(report).toHaveValue(/^Renkeu \d/)
    await expect(report).toHaveValue(/Project folder:/)
    await expect(report).not.toHaveValue(/documentclass/)
  })

  test('history lists a saved change and can put the file back as it was before it', async ({ page }) => {
    await openApp(page)
    await openFile(page, 'main.tex')
    const original = read('main.tex')
    await endOfEditor(page)
    await page.keyboard.type('\n% a change to undo')
    await expect.poll(() => read('main.tex')).toContain('% a change to undo')

    await page.keyboard.press('Alt+h')
    const panel = page.getByRole('dialog', { name: 'History' })
    await expect(panel.locator('.badge').first()).toBeVisible()
    page.once('dialog', (d) => d.accept())
    await panel.getByRole('button', { name: 'Undo this change' }).first().click()

    await expect.poll(() => read('main.tex')).toBe(original)
  })

  test('the keyboard shortcuts tab lists what the app really binds', async ({ page }) => {
    await openApp(page)
    await page.getByRole('button', { name: 'Settings', exact: true }).click()
    await page.getByRole('tab', { name: 'Shortcuts' }).click()
    await expect(page.getByRole('row', { name: /Compile/ })).toBeVisible()
    await expect(page.getByRole('row', { name: /Show this line in the PDF/ })).toBeVisible()
  })
})
