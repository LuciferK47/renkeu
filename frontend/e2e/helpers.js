import { test as base, expect } from '@playwright/test'
import { resetWorkspace } from './fixture.js'

/** `test` that puts the workspace and the project settings back before each test. */
export const test = base.extend({
  page: async ({ page, request }, use) => {
    resetWorkspace()
    await request.put('http://127.0.0.1:3000/api/project', { data: { main: null } })
    await use(page)
  }
})

export { expect }

/** Open the app and wait until the file tree is there. */
export async function openApp(page) {
  await page.goto('/#/editor')
  await page.getByRole('tree').waitFor()
}

/** Open a file from the tree and wait for the editor to show it. */
export async function openFile(page, name) {
  const base = name.split('/').pop()
  await page.getByRole('treeitem', { name: new RegExp(`^\\s*${base.replace('.', '\\.')}\\b`) }).click()
  await page.getByRole('tab', { name: base }).waitFor()
  await page.locator('.monaco-editor .view-line').first().waitFor()
}

/** Click into the editor and put the cursor at the very end. */
export async function endOfEditor(page) {
  await page.locator('.monaco-editor .view-lines').click()
  await page.keyboard.press('Control+End')
}

/** Compile with the shortcut and wait for a page image (or the failure). */
export async function compileOk(page) {
  await page.keyboard.press('Control+Enter')
  await page.locator('.slot img').first().waitFor({ timeout: 120_000 })
}

/** The text of the editor's visible lines. */
export const editorLines = async (page) => (await page.locator('.monaco-editor .view-line').allInnerTexts()).map((l) => l.replace(/\u00a0/g, ' '))

/** The editor line that contains `text`, wherever the engine put it in the DOM. */
export const lineWith = (page, text) => page.locator('.monaco-editor .view-line', { hasText: text }).first()

/**
 * Put the cursor at the start of the line containing `text`, by clicking it.
 * (Ctrl+Home is not delivered the same way by every engine, so tests do not use it.)
 */
export async function cursorToLineStart(page, text) {
  await lineWith(page, text).click()
  await page.keyboard.press('Home')
}
