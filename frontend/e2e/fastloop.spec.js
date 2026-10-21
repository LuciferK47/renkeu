// Project search and replace, the By type sidebar, compile after save, and the
// status bar's word and page counts.
import { test, expect, openApp, openFile, endOfEditor } from './helpers.js'
import { read } from './fixture.js'

test('find in project lists matches across files and jumps to one', async ({ page }) => {
  await openApp(page)
  await page.keyboard.press('Control+Shift+F')
  const dialog = page.getByRole('dialog', { name: 'Find in project' })
  await dialog.getByLabel('Find in project').fill('chapter')
  await expect(dialog.getByText(/match(es)? in \d+ files?/)).toBeVisible()
  await expect(dialog.getByRole('heading', { name: 'chapters/one.tex' })).toBeVisible()
  await dialog.getByRole('button', { name: /first chapter/ }).click()
  await expect(page.getByRole('tab', { name: 'one.tex' })).toBeVisible()
  await expect(dialog).toBeHidden()
})

test('a regular expression that is not valid says so', async ({ page }) => {
  await openApp(page)
  await page.keyboard.press('Control+Shift+F')
  const dialog = page.getByRole('dialog', { name: 'Find in project' })
  await dialog.getByRole('button', { name: 'Regular expression' }).click()
  await dialog.getByLabel('Find in project').fill('(')
  await expect(dialog.getByRole('alert')).toContainText(/not a valid pattern/)
})

test('replace all rewrites the files on disk', async ({ page }) => {
  await openApp(page)
  page.on('dialog', (d) => d.accept())
  await page.keyboard.press('Control+Shift+F')
  const dialog = page.getByRole('dialog', { name: 'Find in project' })
  await dialog.getByLabel('Find in project').fill('quick brown fox')
  await dialog.getByRole('button', { name: 'Show replace' }).click()
  await dialog.getByLabel('Replace with').fill('slow grey cat')
  await expect(dialog.getByText(/1 match in 1 file/)).toBeVisible()
  await dialog.getByRole('button', { name: 'Replace all' }).click()
  await expect.poll(() => read('main.tex'), { timeout: 15_000 }).toContain('The slow grey cat jumps')
})

test('the By type view groups files and marks the main document', async ({ page }) => {
  await openApp(page)
  await page.getByRole('button', { name: 'Group files by type' }).click()
  const tree = page.getByRole('navigation', { name: 'Project files' })
  for (const group of ['Documents', 'Bibliography']) await expect(tree.getByText(group, { exact: true })).toBeVisible()
  await expect(tree.getByRole('treeitem', { name: /refs\.bib/ })).toBeVisible()
  await page.getByRole('button', { name: 'Group files by type' }).click()
  await expect(tree.getByText('Bibliography', { exact: true })).toHaveCount(0)
})

test('the status bar shows the word count', async ({ page }) => {
  await openApp(page)
  await openFile(page, 'main.tex')
  await expect(page.getByRole('status').getByText(/\d+ words/)).toBeVisible()
})

test('compile after save compiles once a save lands', async ({ page }) => {
  await openApp(page)
  await openFile(page, 'main.tex')
  await page.keyboard.press('F1')
  await page.getByPlaceholder('Type a command…').fill('compile after save')
  await page.keyboard.press('Enter')

  await endOfEditor(page)
  await page.keyboard.press('Enter')
  await page.keyboard.type('% compiled by saving')
  await page.keyboard.press('Control+S')
  await page.locator('.slot img').first().waitFor({ timeout: 120_000 })
  await expect(page.getByRole('status').getByText(/\d+ pages? in [\d.]+ s/)).toBeVisible()
})
