// The Insert dialog: figure, table, equation, citation (project and DOI) and
// cross-reference, each placed at the cursor and each compiling.
import { test, expect, openApp, openFile, cursorToLineStart, compileOk } from './helpers.js'
import { read } from './fixture.js'

// A 1x1 PNG, so \includegraphics has a real image to load.
const PNG = Buffer.from('iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg==', 'base64')

async function uploadPng(request, path) {
  const res = await request.post(`http://127.0.0.1:3000/api/upload?path=${encodeURIComponent(path)}`, { data: PNG })
  expect(res.ok()).toBe(true)
}

async function openInsert(page, tab) {
  await page.getByRole('button', { name: /Insert/ }).first().click()
  const dialog = page.getByRole('dialog', { name: 'Insert' })
  await dialog.getByRole('tab', { name: tab }).click()
  return dialog
}

/** Put the cursor on its own line before "Closing words." and open the file. */
async function ready(page) {
  await openApp(page)
  await openFile(page, 'main.tex')
  await cursorToLineStart(page, 'Closing words.')
}

const saved = () => expect.poll(() => read('main.tex'), { timeout: 20_000 })

test('a figure and a table are inserted with their packages and the document compiles', async ({ page, request }) => {
  await uploadPng(request, 'figures/dot.png')
  await ready(page)

  let dialog = await openInsert(page, 'Figure')
  await dialog.getByLabel('Image').selectOption('figures/dot.png')
  await dialog.getByLabel('Caption').fill('A single dot')
  await expect(dialog.getByLabel('Label')).toHaveValue('fig:a-single-dot')
  await dialog.getByRole('button', { name: 'Insert' }).click()

  await cursorToLineStart(page, 'Closing words.')
  dialog = await openInsert(page, 'Table')
  await dialog.getByLabel('Row 1, column 1').fill('Case')
  await dialog.getByLabel('Row 1, column 2').fill('Time')
  await dialog.getByLabel('Row 2, column 1').fill('A & B')
  await dialog.getByLabel('Row 2, column 2').fill('1.2')
  await dialog.getByRole('button', { name: 'Insert' }).click()

  await saved().toContain('\\toprule')
  const text = read('main.tex')
  expect(text).toContain('\\includegraphics[width=0.8\\linewidth]{figures/dot.png}')
  expect(text).toContain('\\label{fig:a-single-dot}')
  expect(text).toContain('A \\& B & 1.2')
  expect(text).toContain('\\usepackage{graphicx}')
  expect(text).toContain('\\usepackage{booktabs}')
  expect(text.indexOf('\\usepackage{graphicx}')).toBeLessThan(text.indexOf('\\begin{document}'))
  await compileOk(page)
})

test('the equation editor loads and a numbered equation is inserted', async ({ page }) => {
  await ready(page)
  const dialog = await openInsert(page, 'Equation')
  await expect(dialog.locator('math-field')).toBeVisible({ timeout: 30_000 })
  await dialog.getByLabel('Equation LaTeX').fill('x^2 + y^2 = z^2')
  await dialog.getByLabel('As').selectOption('numbered')
  await dialog.getByLabel('Label').fill('eq:pythagoras')
  await dialog.getByRole('button', { name: 'Insert' }).click()
  await saved().toContain('\\begin{equation}')
  const text = read('main.tex')
  expect(text).toContain('\\label{eq:pythagoras}')
  expect(text).toContain('x^2 + y^2 = z^2')
  expect(text).toContain('\\usepackage{amsmath}')
  await compileOk(page)
})

test('a citation from the project bibliography and a cross-reference are inserted', async ({ page }) => {
  await ready(page)
  let dialog = await openInsert(page, 'Citation')
  await dialog.getByRole('checkbox').first().check()
  await dialog.getByRole('button', { name: 'Insert' }).click()

  await cursorToLineStart(page, 'Closing words.')
  dialog = await openInsert(page, 'Reference')
  await dialog.getByRole('button', { name: /sec:intro/ }).click()

  await saved().toContain('\\cite{knuth84}')
  expect(read('main.tex')).toContain('\\ref{sec:intro}')
})

test('a citation added by DOI lands in the .bib file with a readable key', async ({ page }) => {
  await page.route('**/api/doi*', (route) =>
    route.fulfill({
      json: {
        doi: '10.1000/demo',
        bibtex: '@article{Lamport_1994, title={LaTeX: A Document Preparation System}, author={Lamport, Leslie}, year={1994}}'
      }
    })
  )
  await ready(page)
  const dialog = await openInsert(page, 'Citation')
  await dialog.getByLabel('DOI').fill('https://doi.org/10.1000/demo')
  await dialog.getByRole('button', { name: 'Look up' }).click()
  await expect(dialog.getByText('lamport1994latex')).toBeVisible()
  await dialog.getByRole('button', { name: 'Insert' }).click()

  await saved().toContain('\\cite{lamport1994latex}')
  await expect.poll(() => read('refs.bib')).toContain('@article{lamport1994latex,')
  expect(read('refs.bib')).toContain('@article{knuth84,')
})

test('an undefined command offers the package that defines it', async ({ page }) => {
  await ready(page)
  await page.keyboard.type('\\toprule ')
  await page.keyboard.press('Control+S')
  await page.keyboard.press('Control+Enter')
  await expect(page.getByText(/Undefined control sequence/).first()).toBeVisible({ timeout: 120_000 })
  await page.getByText(/Undefined control sequence/).first().click()
  await page.keyboard.press('Control+.')
  const fix = page.getByRole('option', { name: /Add \\usepackage\{booktabs\}/ })
  await expect(fix).toBeVisible()
  await page.keyboard.press('Enter')
  await saved().toContain('\\usepackage{booktabs}')
})
