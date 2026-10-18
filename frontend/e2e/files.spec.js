import { test, expect, openApp, openFile } from './helpers.js'
import { exists, read } from './fixture.js'

test.describe('project files', () => {
  test('a new file in a new folder is created, opened, and bad names are refused', async ({ page }) => {
    await openApp(page)
    await page.getByRole('button', { name: 'New file' }).click()
    const name = page.getByRole('textbox', { name: 'Name' })

    await name.fill('../escape.tex')
    await expect(page.getByRole('button', { name: 'Create' })).toBeDisabled()

    await name.fill('.hidden.tex')
    await page.getByRole('button', { name: 'Create' }).click()
    await expect(page.getByText(/cannot start with a dot/)).toBeVisible()
    expect(exists('.hidden.tex')).toBe(false)

    await page.getByRole('button', { name: 'New file' }).click()
    await page.getByRole('textbox', { name: 'Name' }).fill('appendix/extra.tex')
    await page.getByRole('button', { name: 'Create' }).click()
    await expect(page.getByRole('tab', { name: 'extra.tex' })).toBeVisible()
    expect(exists('appendix/extra.tex')).toBe(true)
  })

  test('renaming an open file moves it on disk and the tab follows', async ({ page }) => {
    await openApp(page)
    await page.getByRole('treeitem', { name: /chapters/ }).click()
    await openFile(page, 'chapters/one.tex')
    await page.getByRole('treeitem', { name: /one\.tex/ }).click({ button: 'right' })
    await page.getByRole('menuitem', { name: 'Rename…' }).click()
    await page.getByRole('textbox', { name: 'Name' }).fill('chapters/first.tex')
    await page.getByRole('button', { name: 'Rename' }).click()

    await expect(page.getByRole('tab', { name: 'first.tex' })).toBeVisible()
    expect(exists('chapters/one.tex')).toBe(false)
    expect(read('chapters/first.tex')).toContain('first chapter')
  })

  test('renaming onto an existing file is refused and nothing moves', async ({ page }) => {
    await openApp(page)
    await page.getByRole('treeitem', { name: /main\.tex/ }).click({ button: 'right' })
    await page.getByRole('menuitem', { name: 'Rename…' }).click()
    await page.getByRole('textbox', { name: 'Name' }).fill('chapters/one.tex')
    await page.getByRole('button', { name: 'Rename' }).click()
    await expect(page.getByText(/already exists/)).toBeVisible()
    expect(exists('main.tex')).toBe(true)
    expect(read('chapters/one.tex')).toContain('first chapter')
  })

  test('deleting asks first, then removes the file and closes its tab', async ({ page }) => {
    await openApp(page)
    await openFile(page, 'refs.bib')
    page.once('dialog', (d) => d.dismiss())
    await page.getByRole('treeitem', { name: /refs\.bib/ }).click({ button: 'right' })
    await page.getByRole('menuitem', { name: 'Delete…' }).click()
    expect(exists('refs.bib')).toBe(true)

    page.once('dialog', (d) => d.accept())
    await page.getByRole('treeitem', { name: /refs\.bib/ }).click({ button: 'right' })
    await page.getByRole('menuitem', { name: 'Delete…' }).click()
    await expect.poll(() => exists('refs.bib')).toBe(false)
    await expect(page.getByRole('tab', { name: 'refs.bib' })).toHaveCount(0)
  })

  test('an uploaded image is stored, can be previewed, and a script is refused', async ({ page }) => {
    await openApp(page)
    const png = Buffer.from('iVBORw0KGgoAAAANSUhEUgAAAAgAAAAICAIAAABLbSncAAAAEUlEQVR4nGP8z8DAwMDEAAYAFQgBAf6AFwoAAAAASUVORK5CYII=', 'base64')
    await page.locator('input[type=file]').setInputFiles([{ name: 'plot.png', mimeType: 'image/png', buffer: png }])
    await expect(page.getByRole('treeitem', { name: /plot\.png/ })).toBeVisible()
    expect(exists('plot.png')).toBe(true)

    await page.getByRole('treeitem', { name: /plot\.png/ }).click()
    const img = page.getByRole('dialog', { name: /Preview of plot\.png/ }).getByRole('img')
    await expect(img).toBeVisible()
    await expect(img).toHaveJSProperty('naturalWidth', 8)
    await page.keyboard.press('Escape')

    await page.locator('input[type=file]').setInputFiles([{ name: 'run.sh', mimeType: 'text/plain', buffer: Buffer.from('rm -rf /') }])
    await expect(page.getByText(/cannot be added to a project/)).toBeVisible()
    expect(exists('run.sh')).toBe(false)
  })

  test('the chosen main file is kept by the server and survives a reload', async ({ page }) => {
    await openApp(page)
    await page.getByRole('treeitem', { name: /main\.tex/ }).click({ button: 'right' })
    await page.getByRole('menuitem', { name: 'Set as main file' }).click()
    await expect(page.getByRole('treeitem', { name: /main\.tex/ })).toContainText('main')
    await page.reload()
    await page.getByRole('tree').waitFor()
    await expect(page.getByRole('treeitem', { name: /main\.tex/ })).toContainText('main')
  })

  test('Open file (Ctrl+P) finds a file by part of its name', async ({ page }) => {
    await openApp(page)
    await page.keyboard.press('Control+p')
    await page.getByLabel('Search files').fill('refs')
    await expect(page.getByRole('option', { name: /refs\.bib/ })).toBeVisible()
    await page.keyboard.press('Enter')
    await expect(page.getByRole('tab', { name: 'refs.bib' })).toBeVisible()
  })
})
