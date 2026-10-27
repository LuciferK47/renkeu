// The Layout tab: page count, a page limit, overfull boxes, short last lines.
import { test, expect, openApp, openFile, compileOk } from './helpers.js'
import { write } from './fixture.js'

const THREE_PAGES = [
  '\\documentclass{article}',
  '\\begin{document}',
  'First page text.',
  '',
  `\\noindent\\texttt{${'a'.repeat(110)}}`,
  '',
  '\\newpage',
  'Second page text.',
  '\\newpage',
  'Third page text.',
  '\\end{document}',
  ''
].join('\n')

async function openLayout(page) {
  await openApp(page)
  await openFile(page, 'main.tex')
  await compileOk(page)
  await page.getByRole('tab', { name: 'Layout' }).click()
}

test('the layout tab counts pages, checks a limit and names the line that no longer fits', async ({ page }) => {
  write('main.tex', THREE_PAGES)
  await openLayout(page)
  await expect(page.getByText('3 pages')).toBeVisible()

  await page.getByLabel('Page limit').fill('2')
  await expect(page.getByText(/3 pages against a limit of 2/)).toBeVisible()
  const first = page.getByRole('button', { name: /First line past page 2/ })
  await expect(first).toBeVisible()
  await first.click()
  await expect(page.locator('.hit')).toBeAttached()

  await page.getByLabel('Page limit').fill('3')
  await expect(page.getByText(/Within the limit: 3 of 3 pages/)).toBeVisible()
})

test('an overfull line is listed and points at the page', async ({ page }) => {
  write('main.tex', THREE_PAGES)
  await openLayout(page)
  const box = page.getByRole('button', { name: /Overfull line by/ })
  await expect(box).toBeVisible()
  await box.click()
  await expect(page.locator('.hit')).toBeAttached()
})

test('a plain document has nothing to report and the limit is remembered', async ({ page }) => {
  await openLayout(page)
  await expect(page.getByText(/1 page\b/)).toBeVisible()
  await expect(page.getByText('No overfull or underfull boxes.')).toBeVisible()
  await expect(page.getByText('All fonts are embedded.')).toBeVisible()

  await page.getByLabel('Page limit').fill('5')
  await expect(page.getByText(/Within the limit: 1 of 5 pages/)).toBeVisible()
  await page.reload()
  await page.getByRole('tab', { name: 'Layout' }).click()
  await expect(page.getByLabel('Page limit')).toHaveValue('5')
})
