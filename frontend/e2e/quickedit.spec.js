// Alt+click on the PDF opens the source line behind the spot for a quick edit.
import { test, expect, openApp, openFile, compileOk } from './helpers.js'
import { read } from './fixture.js'

/** Where a source line landed on screen: the middle of its first box, in page pixels. */
async function pointOnLine(page, file, line) {
  const build = (await page.locator('.slot img').first().getAttribute('src')).match(/build=([0-9a-f]+)/)[1]
  const found = await (await page.request.get(`/api/synctex/forward?file=${file}&line=${line}&build=${build}`)).json()
  const r = found.region
  const zoom = Number((await page.locator('.controls .zoom').innerText()).replace('%', '')) / 100
  const pxPerPt = (zoom * 96) / 72
  const img = await page.locator('.slot img').first().boundingBox()
  return { x: img.x + (r.x + r.width / 2) * pxPerPt, y: img.y + (r.y + r.height / 2) * pxPerPt }
}

/** Alt+click the text that source line 3 (the section heading) produced. */
async function altClickFirstText(page, spot = null) {
  const at = spot ?? (await pointOnLine(page, 'main.tex', 3))
  await page.keyboard.down('Alt')
  await page.mouse.click(at.x, at.y)
  await page.keyboard.up('Alt')
}

test('Alt+click edits the source line behind a spot and recompiles', async ({ page }) => {
  await openApp(page)
  await openFile(page, 'main.tex')
  await compileOk(page)

  await altClickFirstText(page)
  const box = page.getByRole('form', { name: 'Quick edit' })
  await expect(box).toBeVisible()
  const field = box.getByRole('textbox')
  const before = await field.inputValue()
  // The heading, found by clicking on it: SyncTeX names its source line.
  await expect(box.getByText('main.tex:3')).toBeVisible()
  expect(before).toBe('\\section{Introduction}\\label{sec:intro}')

  await field.fill(`${before} EDITED`)
  await box.getByRole('button', { name: 'Save and recompile' }).click()

  await expect(box).toBeHidden()
  await expect.poll(() => read('main.tex'), { timeout: 20_000 }).toContain(`${before} EDITED`)
  // The rest of the file is as it was.
  expect(read('main.tex')).toContain('\\input{chapters/one}')
  await page.locator('.slot img').first().waitFor({ timeout: 120_000 })
})

test('a click that is not on text says it found the nearest line', async ({ page }) => {
  await openApp(page)
  await openFile(page, 'main.tex')
  await compileOk(page)
  const img = await page.locator('.slot img').first().boundingBox()
  await altClickFirstText(page, { x: img.x + img.width * 0.08, y: img.y + img.height * 0.6 })
  await expect(page.getByRole('form', { name: 'Quick edit' }).getByText(/nearest line/)).toBeVisible()
})

test('Escape closes the box without touching the file', async ({ page }) => {
  await openApp(page)
  await openFile(page, 'main.tex')
  await compileOk(page)
  const original = read('main.tex')

  await altClickFirstText(page)
  const box = page.getByRole('form', { name: 'Quick edit' })
  await expect(box).toBeVisible()
  await box.getByRole('textbox').fill('something else')
  await page.keyboard.press('Escape')
  await expect(box).toBeHidden()
  expect(read('main.tex')).toBe(original)
})

test('"Open in editor" goes to the line instead', async ({ page }) => {
  await openApp(page)
  await openFile(page, 'main.tex')
  await compileOk(page)
  await altClickFirstText(page)
  const box = page.getByRole('form', { name: 'Quick edit' })
  await box.getByRole('button', { name: 'Open in editor' }).click()
  await expect(box).toBeHidden()
  await expect(page.getByRole('tab', { name: 'main.tex' })).toBeVisible()
})
