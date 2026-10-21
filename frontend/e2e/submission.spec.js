// The Submission tab: pick a profile, see what it finds in the project and the
// PDF, and jump to the line.
import { test, expect, openApp, openFile, compileOk } from './helpers.js'
import { write } from './fixture.js'

const NAMED = [
  '\\documentclass{article}',
  '\\author{Ada Lovelace}',
  '\\begin{document}',
  'As shown in our previous work, it holds.',
  '\\end{document}',
  ''
].join('\n')

test('a profile finds what it should, and each finding with a line opens it', async ({ page }) => {
  write('main.tex', NAMED)
  await openApp(page)
  await openFile(page, 'main.tex')
  await compileOk(page)

  await page.getByRole('tab', { name: 'Submission' }).click()
  await page.getByLabel('Profile').selectOption({ label: 'Double-blind review' })

  await expect(page.getByText(/to fix/)).toBeVisible()
  const author = page.getByRole('button', { name: /names: Ada Lovelace/ })
  await expect(author).toBeVisible()
  await expect(author).toContainText('main.tex:2')
  await expect(page.getByRole('button', { name: /reads as the authors citing themselves/ })).toBeVisible()

  await author.click()
  await expect(page.locator('.monaco-editor .current-line').first()).toBeVisible()
})

test('a profile says where its rules come from, and a clean document has nothing blocking', async ({ page }) => {
  write('main.tex', '\\documentclass{article}\n\\begin{document}\nHello.\n\\end{document}\n')
  await openApp(page)
  await openFile(page, 'main.tex')
  await compileOk(page)

  await page.getByRole('tab', { name: 'Submission' }).click()
  await page.getByLabel('Profile').selectOption({ label: 'arXiv' })
  await expect(page.getByRole('link', { name: /info\.arxiv\.org/ })).toBeVisible()
  await expect(page.getByText('Nothing blocking')).toBeVisible()
})
