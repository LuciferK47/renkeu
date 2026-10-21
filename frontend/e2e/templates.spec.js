// Starting from a template, then changing its details.
import { test, expect } from './helpers.js'
import { emptyWorkspace, read, exists } from './fixture.js'

test('an empty project starts from a template and compiles', async ({ page }) => {
  emptyWorkspace()
  await page.goto('/#/editor')
  await page.getByRole('button', { name: 'Start from a template…' }).click()

  const dialog = page.getByRole('dialog', { name: 'Start from a template' })
  await dialog.getByRole('tab', { name: 'Papers' }).click()
  await dialog.getByRole('button', { name: /IEEE conference/ }).click()

  await page.getByLabel('Title', { exact: true }).fill('Cats & Dogs: 100% of everything')
  await page.getByLabel('Author 1 name').fill('Ada Lovelace')
  await page.getByLabel('Author 1 affiliation').fill('Analytical Engine Co.')
  await page.getByLabel('Abstract').fill('We study cats and dogs.')
  await page.getByRole('button', { name: 'Create project' }).click()

  await page.getByRole('tab', { name: 'main.tex' }).waitFor()
  expect(exists('main.tex')).toBe(true)
  expect(exists('refs.bib')).toBe(true)
  expect(read('main.tex')).toContain('\\title{Cats \\& Dogs: 100\\% of everything}')

  // It compiles on its own after creation, and a page shows up.
  await page.locator('.slot img').first().waitFor({ timeout: 120_000 })
})

test('document details rewrite only the title block', async ({ page }) => {
  emptyWorkspace()
  await page.goto('/#/editor')
  await page.getByRole('button', { name: 'Start from a template…' }).click()
  await page.getByRole('button', { name: /^Article/ }).click()
  await page.getByLabel('Title', { exact: true }).fill('First title')
  await page.getByLabel('Author 1 name').fill('Ada Lovelace')
  await page.getByRole('button', { name: 'Create project' }).click()
  await page.getByRole('tab', { name: 'main.tex' }).waitFor()
  await page.locator('.monaco-editor .view-line').first().waitFor()

  await page.keyboard.press('F1')
  await page.getByPlaceholder('Type a command…').fill('Document details')
  await page.keyboard.press('Enter')
  await page.getByLabel('Title', { exact: true }).fill('Second title')
  await page.getByRole('button', { name: 'Update document' }).click()

  await expect.poll(() => read('main.tex'), { timeout: 15_000 }).toContain('\\title{Second title}')
  expect(read('main.tex')).toContain('\\section{Introduction}')
  expect(read('main.tex')).toContain('"title":"Second title"')
})

test('a project that already has a tex file says so instead of mixing templates in', async ({ page, request }) => {
  await page.goto('/#/editor')
  await page.getByRole('tree').waitFor()
  const res = await request.post('http://127.0.0.1:3000/api/project/template', { data: { template: 'article', fields: {} } })
  expect(res.status()).toBe(409)
  expect(read('main.tex')).toContain('The quick brown fox')
})
